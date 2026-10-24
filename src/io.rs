// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
//! Blocking transport, safe archives, and the on-disk Fuel cache.
use crate::{
    CachedResource, ClientConfig, Error, ResourceId, ResourceKind, ResourceLimits, ServerConfig,
};
use serde_json::{Value, json};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Cursor, Read, Seek, Write};
use std::path::{Component, Path, PathBuf};
use zip::write::SimpleFileOptions;

fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
fn bytes(v: &Value) -> Vec<u8> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_u64().and_then(|n| u8::try_from(n).ok()))
                .collect()
        })
        .unwrap_or_else(|| v.as_str().unwrap_or("").as_bytes().to_vec())
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn safe_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path.components().all(|c| matches!(c, Component::Normal(_)))
        && !path.to_string_lossy().contains('\\')
        && !path.to_string_lossy().contains(':')
}
fn no_symlinks(path: &Path) -> io::Result<()> {
    let mut current = PathBuf::new();
    for part in path.components() {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(invalid("symlink in destination"));
            }
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    Ok(())
}

// Check the declared raw entry count before ZipArchive allocates its central
// directory. Its public len() deduplicates names and is too late to bound work.
fn archive_directory_budget<R: Read + Seek>(
    reader: &mut R,
    size: u64,
    limits: &ResourceLimits,
) -> io::Result<()> {
    let tail_size = size.min(65_557) as usize;
    reader.seek(io::SeekFrom::End(-(tail_size as i64)))?;
    let mut tail = vec![0; tail_size];
    reader.read_exact(&mut tail)?;
    let footer = tail
        .windows(4)
        .enumerate()
        .rev()
        .find_map(|(offset, magic)| {
            if magic != b"PK\x05\x06" || offset + 22 > tail.len() {
                return None;
            }
            let comment = u16::from_le_bytes([tail[offset + 20], tail[offset + 21]]) as usize;
            (offset + 22 + comment == tail.len()).then_some(offset)
        })
        .ok_or_else(|| invalid("missing ZIP end record"))?;
    let mut entries = u16::from_le_bytes([tail[footer + 10], tail[footer + 11]]) as u64;
    if footer >= 20 && &tail[footer - 20..footer - 16] == b"PK\x06\x07" {
        let record_offset = u64::from_le_bytes(
            tail[footer - 12..footer - 4]
                .try_into()
                .map_err(|_| invalid("invalid ZIP64 locator"))?,
        );
        reader.seek(io::SeekFrom::Start(record_offset))?;
        let mut record = [0; 56];
        reader.read_exact(&mut record)?;
        if &record[..4] != b"PK\x06\x06" {
            return Err(invalid("invalid ZIP64 end record"));
        }
        let record_size = u64::from_le_bytes(
            record[4..12]
                .try_into()
                .map_err(|_| invalid("invalid ZIP64 record size"))?,
        );
        if record_size > limits.max_metadata_bytes {
            return Err(limit_error("ZIP64 metadata exceeds byte limit"));
        }
        entries = u64::from_le_bytes(
            record[32..40]
                .try_into()
                .map_err(|_| invalid("invalid ZIP64 entry count"))?,
        );
    }
    if entries > limits.max_archive_entries as u64 {
        return Err(limit_error("archive exceeds entry count limit"));
    }
    reader.rewind()?;
    Ok(())
}

fn extract<R: Read + Seek>(reader: R, destination: &Path) -> io::Result<()> {
    extract_with_limits(reader, destination, &ResourceLimits::default())
}

fn extract_with_limits<R: Read + Seek>(
    mut reader: R,
    destination: &Path,
    limits: &ResourceLimits,
) -> io::Result<()> {
    no_symlinks(destination)?;
    let compressed_size = reader.seek(io::SeekFrom::End(0))?;
    if compressed_size > limits.max_download_bytes {
        return Err(limit_error("archive exceeds compressed byte limit"));
    }
    archive_directory_budget(&mut reader, compressed_size, limits)?;
    let mut archive = zip::ZipArchive::new(reader)?;
    if archive.len() > limits.max_archive_entries {
        return Err(limit_error("archive exceeds entry count limit"));
    }
    let mut declared_total = 0_u64;
    // Validate the complete directory before creating files. Enforce both declared
    // sizes here and actual decompressed bytes below: ZIP headers are untrusted.
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        let filename = entry.name()?;
        let name = filename.trim_end_matches('/');
        let path = Path::new(name);
        if !safe_relative(path) || entry.is_symlink() {
            return Err(invalid("unsafe archive entry"));
        }
        if path.components().count() > limits.max_archive_depth {
            return Err(limit_error("archive exceeds path depth limit"));
        }
        if entry.size() > limits.max_archive_entry_bytes {
            return Err(limit_error("archive entry exceeds byte limit"));
        }
        declared_total = declared_total
            .checked_add(entry.size())
            .filter(|size| *size <= limits.max_archive_total_bytes)
            .ok_or_else(|| limit_error("archive exceeds total extracted byte limit"))?;
        no_symlinks(&destination.join(path))?;
    }
    fs::create_dir_all(destination)?;
    let mut actual_total = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let output = destination.join(entry.name()?.as_ref());
        if entry.is_dir() {
            fs::create_dir_all(output)?;
        } else {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            no_symlinks(&output)?;
            let mut file = File::create(output)?;
            let available = limits
                .max_archive_entry_bytes
                .min(limits.max_archive_total_bytes.saturating_sub(actual_total));
            let copied = io::copy(
                &mut entry.by_ref().take(available.saturating_add(1)),
                &mut file,
            )?;
            if copied > available {
                return Err(limit_error("archive exceeds extracted byte limit"));
            }
            actual_total += copied;
        }
    }
    Ok(())
}

fn read_children(path: &Path) -> io::Result<Vec<PathBuf>> {
    let mut entries = fs::read_dir(path)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<io::Result<Vec<_>>>()?;
    entries.sort();
    Ok(entries)
}

// Enumeration intentionally tolerates unreadable cache entries. Compression uses
// read_children directly so a successful archive never hides an unreadable tree.
fn sorted_children(path: &Path) -> Vec<PathBuf> {
    read_children(path).unwrap_or_default()
}
fn add_entry<W: Write + Seek>(
    writer: &mut zip::ZipWriter<W>,
    path: &Path,
    root: &Path,
) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(invalid("cannot archive symbolic links"));
    }
    let name = path
        .strip_prefix(root)
        .map_err(|_| invalid("invalid archive root"))?
        .to_string_lossy()
        .replace('\\', "/");
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    if metadata.is_dir() {
        writer.add_directory(format!("{name}/"), options)?;
        for child in read_children(path)? {
            add_entry(writer, &child, root)?;
        }
    } else if metadata.is_file() {
        writer.start_file(name, options)?;
        io::copy(&mut File::open(path)?, writer)?;
    } else {
        return Err(invalid("unsupported file type"));
    }
    Ok(())
}
pub fn compress(source: &Path, destination: &Path) -> io::Result<()> {
    let source = fs::canonicalize(source)?;
    let parent = source
        .parent()
        .ok_or_else(|| invalid("cannot archive filesystem root"))?;
    // Snapshot to a temporary file outside the input tree; never recursively
    // include the archive currently being written.
    let mut temporary = tempfile::NamedTempFile::new()?;
    let mut writer = zip::ZipWriter::new(temporary.as_file_mut());
    add_entry(&mut writer, &source, parent)?;
    writer.finish()?;
    no_symlinks(destination)?;
    fs::copy(temporary.path(), destination)?;
    Ok(())
}

pub(crate) use crate::transport::request;

fn cache_server_path(server: &ServerConfig) -> io::Result<PathBuf> {
    let (_, rest) = server
        .url
        .split_once("://")
        .ok_or_else(|| invalid("invalid cache server"))?;
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    if authority.is_empty() {
        return Err(invalid("invalid cache server authority"));
    }
    let authority = if server.has_authority {
        authority.replace(':', "%3A").replace('@', "%40")
    } else {
        authority.to_owned()
    };
    let path = PathBuf::from(authority).join(path.trim_matches('/'));
    if path.as_os_str().is_empty()
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        || path.to_string_lossy().contains(['\\', '\0'])
    {
        return Err(invalid("unsafe cache server path"));
    }
    Ok(path)
}

#[cfg(test)]
pub fn server_path(server: &Value) -> Option<PathBuf> {
    cache_server_path(&ServerConfig {
        url: s(server, "url").to_owned(),
        has_authority: server["has_authority"].as_bool().unwrap_or(true),
        ..ServerConfig::default()
    })
    .ok()
}

fn cache_id(id: &ResourceId, kind: ResourceKind) -> Result<ResourceId, Error> {
    if kind == ResourceKind::Collection {
        return Err(Error::new(
            crate::ErrorKind::InvalidInput,
            "collections cannot be cached",
        ));
    }
    let id = crate::identity::canonicalize(id, kind)?;
    if !safe_relative(Path::new(&id.owner))
        || !safe_relative(Path::new(&id.name))
        || id.owner.contains('/')
        || id.name.contains('/')
    {
        return Err(Error::new(
            crate::ErrorKind::InvalidInput,
            "unsafe cache identifier",
        ));
    }
    Ok(id)
}

fn resource_parent(
    config: &ClientConfig,
    kind: ResourceKind,
    id: &ResourceId,
) -> Result<PathBuf, Error> {
    Ok(config
        .cache_location
        .join(cache_server_path(&id.server)?)
        .join(&id.owner)
        .join(kind.route())
        .join(&id.name))
}

fn cached_entry(
    kind: ResourceKind,
    mut id: ResourceId,
    path: PathBuf,
) -> io::Result<Option<CachedResource>> {
    no_symlinks(&path)?;
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !metadata.is_dir() {
        return Ok(None);
    }
    if kind == ResourceKind::Model {
        let mut found = false;
        for name in ["model.config", "metadata.pbtxt"] {
            let file = path.join(name);
            no_symlinks(&file)?;
            found |= file.is_file();
        }
        if !found {
            return Ok(None);
        }
    }
    let path = fs::canonicalize(path)?;
    id.local_path = path.clone();
    Ok(Some(CachedResource { id, path }))
}

/// Look up exactly one resource. Numeric versions touch only their known path;
/// tip scans only that resource's version directories, never unrelated resources.
pub(crate) fn lookup_resource(
    config: &ClientConfig,
    kind: ResourceKind,
    id: &ResourceId,
) -> Result<Option<CachedResource>, Error> {
    let id = cache_id(id, kind)?;
    let parent = resource_parent(config, kind, &id)?;
    let mut best = lookup_parent(kind, &id, &parent)?;
    if id.version != 0 && best.is_some() {
        return Ok(best);
    }
    // Historical caches can contain mixed-case directory spellings. Explicit
    // canonical hits remain O(1); tip compares numeric versions across matching
    // owner/resource siblings without inspecting unrelated resource contents.
    let server = config.cache_location.join(cache_server_path(&id.server)?);
    for owner in matching_children(&server, &id.owner)? {
        for name in matching_children(&owner.join(kind.route()), &id.name)? {
            if name == parent {
                continue;
            }
            if let Some(entry) = lookup_parent(kind, &id, &name)? {
                if id.version != 0 {
                    return Ok(Some(entry));
                }
                if best
                    .as_ref()
                    .is_none_or(|old| old.id.version < entry.id.version)
                {
                    best = Some(entry);
                }
            }
        }
    }
    Ok(best)
}
fn matching_children(parent: &Path, requested: &str) -> io::Result<Vec<PathBuf>> {
    no_symlinks(parent)?;
    let children = match read_children(parent) {
        Ok(children) => children,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    Ok(children
        .into_iter()
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case(requested))
        })
        .filter(|path| no_symlinks(path).is_ok() && path.is_dir())
        .collect())
}
fn lookup_parent(
    kind: ResourceKind,
    id: &ResourceId,
    parent: &Path,
) -> Result<Option<CachedResource>, Error> {
    no_symlinks(parent)?;
    if id.version != 0 {
        return Ok(cached_entry(
            kind,
            id.clone(),
            parent.join(id.version.to_string()),
        )?);
    }
    let children = match read_children(parent) {
        Ok(children) => children,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut versions = children
        .into_iter()
        .filter_map(|path| {
            let version = path.file_name()?.to_str()?.parse::<u32>().ok()?;
            (version > 0).then_some((version, path))
        })
        .collect::<Vec<_>>();
    versions.sort_by_key(|(version, _)| std::cmp::Reverse(*version));
    for (version, path) in versions {
        let mut candidate = id.clone();
        candidate.version = version;
        if let Some(entry) = cached_entry(kind, candidate, path)? {
            return Ok(Some(entry));
        }
    }
    Ok(None)
}

/// Enumerate existing cached versions, omitting unreadable/unusable entries.
pub(crate) fn cached_resources(
    config: &ClientConfig,
    kind: ResourceKind,
    filter: Option<&ResourceId>,
) -> Result<Vec<CachedResource>, Error> {
    if kind == ResourceKind::Collection {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for server in &config.servers {
        if filter.is_some_and(|filter| {
            !filter.server.url.is_empty()
                && filter.server.url.trim_end_matches('/') != server.url.trim_end_matches('/')
        }) {
            continue;
        }
        let Ok(relative) = cache_server_path(server) else {
            continue;
        };
        let root = config.cache_location.join(relative);
        if no_symlinks(&root).is_err() {
            continue;
        }
        for owner in sorted_children(&root) {
            let Some(owner_name) = owner.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let owner_name = owner_name.to_ascii_lowercase();
            if filter.is_some_and(|filter| {
                !filter.owner.is_empty() && !filter.owner.eq_ignore_ascii_case(&owner_name)
            }) {
                continue;
            }
            if no_symlinks(&owner).is_err() || !owner.is_dir() {
                continue;
            }
            for name in sorted_children(&owner.join(kind.route())) {
                let Some(resource_name) = name.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                let resource_name = resource_name.to_ascii_lowercase();
                if filter.is_some_and(|filter| {
                    !filter.name.is_empty() && !filter.name.eq_ignore_ascii_case(&resource_name)
                }) {
                    continue;
                }
                if no_symlinks(&name).is_err() || !name.is_dir() {
                    continue;
                }
                for path in sorted_children(&name) {
                    let Some(version) = path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .and_then(|name| name.parse::<u32>().ok())
                        .filter(|version| *version > 0)
                    else {
                        continue;
                    };
                    let id = ResourceId {
                        name: resource_name.clone(),
                        owner: owner_name.clone(),
                        server: server.clone(),
                        version,
                        ..ResourceId::default()
                    };
                    if cache_id(&id, kind).is_err() {
                        continue;
                    }
                    if let Ok(Some(entry)) = cached_entry(kind, id, path) {
                        result.push(entry);
                    }
                }
            }
        }
    }
    Ok(result)
}

fn resource_lock(parent: &Path, _version: u32, exclusive: bool) -> io::Result<File> {
    no_symlinks(parent)?;
    #[cfg(target_os = "linux")]
    let file = {
        use std::os::unix::fs::OpenOptionsExt;
        // The resource parent is never exchanged. Linux flock supports a
        // read-only directory descriptor, so immutable legacy caches need no
        // writable sidecar. All versions of this resource share publication and
        // metadata-read locks; unrelated resources still proceed independently.
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(parent)?;
        if !file.metadata()?.is_dir() {
            return Err(invalid("cache lock is not a directory"));
        }
        file
    };
    #[cfg(not(target_os = "linux"))]
    let file = {
        let path = parent.join(format!(".fuel-{_version}.lock"));
        no_symlinks(&path)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        let file = options.open(path)?;
        if !file.metadata()?.is_file() {
            return Err(invalid("cache lock is not a regular file"));
        }
        file
    };
    if exclusive {
        file.lock()?;
    } else {
        file.lock_shared()?;
    }
    Ok(file)
}

/// Hold a shared resource publication lock throughout a multi-file read.
pub(crate) fn with_cached_resource<T>(
    config: &ClientConfig,
    kind: ResourceKind,
    id: &ResourceId,
    read: impl FnOnce(&CachedResource) -> Result<T, Error>,
) -> Result<Option<T>, Error> {
    let Some(entry) = lookup_resource(config, kind, id)? else {
        return Ok(None);
    };
    let parent = entry
        .path
        .parent()
        .ok_or_else(|| invalid("cache resource has no parent"))?;
    let _lock = resource_lock(parent, entry.id.version, false)?;
    let Some(entry) = cached_entry(kind, entry.id, entry.path)? else {
        return Ok(None);
    };
    read(&entry).map(Some)
}

fn kind_from_value(a: &Value) -> Option<ResourceKind> {
    match s(a, "kind") {
        "models" => Some(ResourceKind::Model),
        "worlds" => Some(ResourceKind::World),
        _ => None,
    }
}

pub fn cache_list(a: &Value) -> Value {
    let Some(kind) = kind_from_value(a) else {
        return json!([]);
    };
    if !a["config"].is_object() {
        return json!([]);
    }
    let Ok(config) = crate::core::typed_config(&a["config"]) else {
        return json!([]);
    };
    let exact = a["exact"].as_bool().unwrap_or(false);
    let mut filter = if a["id"].is_null() {
        None
    } else {
        serde_json::from_value::<ResourceId>(a["id"].clone()).ok()
    };
    if !a["id"].is_null() && filter.is_none() {
        return json!([]);
    }
    if let Some(filter) = &mut filter
        && a["id"]["server"]["url"].is_null()
    {
        filter.server.url.clear();
    }
    if !exact
        && filter
            .as_ref()
            .is_some_and(|id| id.name.is_empty() && id.owner.is_empty() && id.server.url.is_empty())
    {
        return json!([]);
    }
    let entries = if exact {
        filter
            .as_ref()
            .and_then(|id| lookup_resource(&config, kind, id).ok().flatten())
            .into_iter()
            .collect()
    } else {
        cached_resources(&config, kind, filter.as_ref()).unwrap_or_default()
    };
    json!(
        entries
            .into_iter()
            .map(|entry| json!({"id":entry.id,"path":entry.path}))
            .collect::<Vec<_>>()
    )
}

fn semantic_version(text: &str) -> Vec<u64> {
    text.split('.').map(|s| s.parse().unwrap_or(0)).collect()
}
fn xml_sdf(text: &str) -> Option<String> {
    let document = roxmltree::Document::parse(text.trim_start()).ok()?;
    let root = document.root_element();
    if !matches!(root.tag_name().name(), "model" | "world") {
        return None;
    }
    root.children()
        .filter(|n| n.has_tag_name("sdf"))
        .max_by_key(|n| semantic_version(n.attribute("version").unwrap_or("0.0.1")))?
        .text()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}
fn metadata_file(text: &str) -> Option<String> {
    crate::metadata::parse_pbtxt(text)
        .ok()?
        .resource_file()
        .map(|file| file.file.clone())
}
pub fn sdf_path(path: &Path) -> Option<PathBuf> {
    sdf_path_with_limits(path, &ResourceLimits::default())
        .ok()
        .flatten()
}
fn sdf_path_with_limits(path: &Path, limits: &ResourceLimits) -> io::Result<Option<PathBuf>> {
    let metadata = path.join("metadata.pbtxt");
    let config = path.join("model.config");
    if metadata.exists() || config.exists() {
        let filename = if metadata.exists() {
            metadata_file(&read_text_limited(&metadata, limits.max_metadata_bytes)?)
        } else {
            xml_sdf(&read_text_limited(&config, limits.max_metadata_bytes)?)
        };
        return Ok(filename
            .filter(|filename| safe_relative(Path::new(filename)))
            .map(|filename| path.join(filename)));
    }
    Ok(sorted_children(path).into_iter().find(|path| {
        path.is_file() && path.extension().is_some_and(|extension| extension == "sdf")
    }))
}
fn fix_model_paths(
    path: &Path,
    id: &ResourceId,
    limits: &ResourceLimits,
    relative: &Path,
) -> io::Result<()> {
    let file = path.join(relative);
    let text = read_text_limited(&file, limits.max_metadata_bytes)?;
    let parse_text = text.trim_start();
    let prefix = text.len() - parse_text.len();
    let document = roxmltree::Document::parse(parse_text).map_err(|e| invalid(&e.to_string()))?;
    let mut replacements = Vec::new();
    let mut output_size = text.len() as u64;
    for node in document.descendants().filter(|n| {
        n.is_element()
            && matches!(
                n.tag_name().name(),
                "uri"
                    | "filename"
                    | "albedo_map"
                    | "normal_map"
                    | "environment_map"
                    | "emissive_map"
                    | "light_map"
                    | "metalness_map"
                    | "roughness_map"
                    | "specular_map"
                    | "glossiness_map"
            )
    }) {
        let Some(value) = node.text().and_then(|t| t.strip_prefix("model://")) else {
            continue;
        };
        let Some((_, suffix)) = value.split_once('/') else {
            continue;
        };
        let Some(text_node) = node.children().find(|n| n.is_text()) else {
            continue;
        };
        let mut url = crate::identity::resource_url(id, ResourceKind::Model, true, true)
            .map_err(|error| invalid(&error.to_string()))?;
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| invalid("invalid resource URL"))?;
            segments.push("files");
            for segment in suffix.split('/') {
                let decoded = percent_encoding::percent_decode_str(segment)
                    .decode_utf8()
                    .map_err(|_| invalid("asset URI is not valid UTF-8"))?;
                if decoded.split('/').any(|part| matches!(part, "." | ".."))
                    || decoded.contains(['\\', '\0'])
                {
                    return Err(invalid("unsafe asset URI path"));
                }
                segments.push(&decoded);
            }
        }
        let url = url.to_string();
        let replacement = url.replace('&', "&amp;").replace('<', "&lt;");
        let range = (text_node.range().start + prefix)..(text_node.range().end + prefix);
        output_size = output_size
            .checked_sub(range.len() as u64)
            .and_then(|size| size.checked_add(replacement.len() as u64))
            .filter(|size| *size <= limits.max_metadata_bytes)
            .ok_or_else(|| limit_error("rewritten SDF exceeds metadata byte limit"))?;
        replacements.push((range, replacement));
    }
    drop(document);
    let mut output = text;
    for (range, value) in replacements.into_iter().rev() {
        output.replace_range(range, &value);
    }
    fs::write(file, output)
}
fn read_text_limited(path: &Path, limit: u64) -> io::Result<String> {
    let file = File::open(path)?;
    if file.metadata()?.len() > limit {
        return Err(limit_error("metadata exceeds byte limit"));
    }
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(limit_error("metadata exceeds byte limit"));
    }
    String::from_utf8(bytes).map_err(|error| invalid(&error.to_string()))
}

#[derive(Debug)]
struct LimitExceeded(&'static str);
impl std::fmt::Display for LimitExceeded {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}
impl std::error::Error for LimitExceeded {}
fn limit_error(message: &'static str) -> io::Error {
    io::Error::other(LimitExceeded(message))
}
fn archive_failure(error: io::Error) -> Error {
    let kind = if error
        .get_ref()
        .is_some_and(|source| source.is::<LimitExceeded>())
    {
        crate::ErrorKind::Limit
    } else {
        crate::ErrorKind::Io
    };
    Error::with_source(kind, error.to_string(), error)
}

/// Validate an archive in private staging, then atomically publish it while
/// holding its resource publication lock across the resolved numeric install.
pub(crate) fn save_resource<R: Read + Seek>(
    config: &ClientConfig,
    kind: ResourceKind,
    id: &ResourceId,
    reader: R,
    overwrite: bool,
) -> Result<CachedResource, Error> {
    let id = cache_id(id, kind)?;
    if id.version == 0 {
        return Err(Error::new(
            crate::ErrorKind::InvalidInput,
            "cached versions must be resolved",
        ));
    }
    let parent = resource_parent(config, kind, &id)?;
    let destination = parent.join(id.version.to_string());
    no_symlinks(&destination)?;
    fs::create_dir_all(&parent)?;
    let stage = tempfile::Builder::new()
        .prefix(".fuel-")
        .tempdir_in(&parent)?;
    extract_with_limits(reader, stage.path(), &config.limits).map_err(archive_failure)?;
    if kind == ResourceKind::Model {
        let metadata = crate::metadata::validate(stage.path(), kind, &config.limits)?;
        let file = metadata
            .resource_file()
            .ok_or_else(|| invalid("missing model SDF"))?;
        fix_model_paths(stage.path(), &id, &config.limits, Path::new(&file.file))
            .map_err(archive_failure)?;
    } else {
        let file = sdf_path_with_limits(stage.path(), &config.limits)
            .map_err(archive_failure)?
            .ok_or_else(|| invalid("world archive has no SDF"))?;
        let text =
            read_text_limited(&file, config.limits.max_metadata_bytes).map_err(archive_failure)?;
        let document = roxmltree::Document::parse(text.trim_start()).map_err(|error| {
            Error::with_source(crate::ErrorKind::Metadata, "invalid world SDF", error)
        })?;
        if !document.root_element().has_tag_name("sdf") {
            return Err(Error::new(
                crate::ErrorKind::Metadata,
                "world SDF has no sdf root",
            ));
        }
    }
    // Validation and extraction deliberately precede lock acquisition so slow or
    // invalid downloads do not block consumers of an already usable cache entry.
    let _lock = resource_lock(&parent, id.version, true)?;
    no_symlinks(&destination)?;
    if destination.exists() && !overwrite {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, "resource already cached").into());
    }
    install_stage(stage.path(), &destination)?;
    let path = fs::canonicalize(destination)?;
    let mut id = id;
    id.local_path = path.clone();
    Ok(CachedResource { id, path })
}

fn install_stage(stage: &Path, destination: &Path) -> io::Result<()> {
    if !destination.exists() {
        return fs::rename(stage, destination);
    }
    if !fs::symlink_metadata(destination)?.is_dir() {
        return Err(invalid("cache destination is not a directory"));
    }
    exchange_directories(stage, destination)
}

#[cfg(target_os = "linux")]
fn exchange_directories(stage: &Path, destination: &Path) -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let stage = std::ffi::CString::new(stage.as_os_str().as_bytes())
        .map_err(|_| invalid("invalid staging path"))?;
    let destination = std::ffi::CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| invalid("invalid cache path"))?;
    // SAFETY: both C strings remain live and NUL terminated throughout the call.
    // AT_FDCWD makes their filesystem paths relative to this process as usual;
    // RENAME_EXCHANGE atomically swaps the two existing directories without
    // dereferencing Rust memory or giving ownership of either string to libc.
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            stage.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            libc::RENAME_EXCHANGE,
        )
    };
    if result == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if matches!(
        error.raw_os_error(),
        Some(libc::ENOSYS | libc::EINVAL | libc::EOPNOTSUPP)
    ) {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "filesystem does not support atomic cache replacement",
        ));
    }
    Err(error)
}

#[cfg(not(target_os = "linux"))]
fn exchange_directories(_stage: &Path, _destination: &Path) -> io::Result<()> {
    // Never fall back to rename-away/rename-in: it exposes missing resources and
    // can lose the previous entry when rollback competes with another writer.
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic cache replacement is unsupported on this platform",
    ))
}

pub fn cache_save(a: &Value) -> Value {
    cache_save_bytes(a, &bytes(&a["data"]))
}
pub fn cache_save_bytes(a: &Value, data: &[u8]) -> Value {
    let result = (|| {
        let kind = kind_from_value(a).ok_or_else(|| invalid("invalid resource kind"))?;
        let config = crate::core::typed_config(&a["config"])?;
        let id = serde_json::from_value::<ResourceId>(a["id"].clone())?;
        save_resource(
            &config,
            kind,
            &id,
            Cursor::new(data),
            a["overwrite"].as_bool().unwrap_or(false),
        )
    })();
    match result {
        Ok(entry) => json!({"ok":true,"id":entry.id,"path":entry.path}),
        Err(error) => json!({"ok":false,"id":a["id"],"path":"","error":error.to_string()}),
    }
}
pub fn dispatch(op: &str, a: &Value) -> Option<Value> {
    Some(match op {
        "io.compress" => json!(compress(Path::new(s(a, "src")), Path::new(s(a, "dst"))).is_ok()),
        "io.extract" => json!(
            File::open(s(a, "src"))
                .and_then(|f| extract(f, Path::new(s(a, "dst"))))
                .is_ok()
        ),
        "io.request" => request(a),
        "io.cache_list" => cache_list(a),
        "io.cache_save" => cache_save(a),
        "io.sdf" => json!(
            sdf_path(Path::new(s(a, "path")))
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default()
        ),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{join_url, mime};
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;

    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in entries {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }
    fn id() -> Value {
        json!({"name":"box","owner":"alice","version":2,"server":{"url":"http://localhost:8001/","version":"1.0"}})
    }
    fn config(path: &Path) -> Value {
        json!({"cache_location":path.to_str().unwrap(),"servers":[id()["server"]]})
    }
    fn http(a: Value, body: &[u8], headers: &str) -> (Value, String) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n{}Connection: close\r\n\r\n",
            body.len(),
            headers
        )
        .into_bytes()
        .into_iter()
        .chain(body.iter().copied())
        .collect::<Vec<_>>();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut message = Vec::new();
            loop {
                let mut buffer = [0; 4096];
                let count = socket.read(&mut buffer).unwrap();
                if count == 0 {
                    break;
                }
                message.extend_from_slice(&buffer[..count]);
                if let Some(end) = message.windows(4).position(|v| v == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&message[..end]).to_lowercase();
                    let size = head
                        .lines()
                        .find_map(|l| {
                            l.strip_prefix("content-length:")
                                .and_then(|s| s.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if message.len() >= end + 4 + size {
                        break;
                    }
                }
            }
            socket.write_all(&response).unwrap();
            String::from_utf8_lossy(&message).into_owned()
        });
        let mut a = a;
        a["url"] = json!(format!("http://{address}"));
        let result = request(&a);
        (result, server.join().unwrap())
    }

    #[test]
    fn archives_round_trip_binary_empty_files_and_directories() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("model");
        fs::create_dir_all(source.join("nested/empty")).unwrap();
        fs::write(source.join("binary"), [0, 255, 10, 13, 0, 128]).unwrap();
        fs::write(source.join("nested/zero"), []).unwrap();
        let compressed = dir.path().join("out.zip");
        assert!(
            dispatch("io.compress", &json!({"src":source,"dst":compressed}))
                .unwrap()
                .as_bool()
                .unwrap()
        );
        let destination = dir.path().join("unpacked");
        assert_eq!(
            dispatch("io.extract", &json!({"src":compressed,"dst":destination})),
            Some(json!(true))
        );
        assert_eq!(
            fs::read(destination.join("model/binary")).unwrap(),
            [0, 255, 10, 13, 0, 128]
        );
        assert!(
            fs::read(destination.join("model/nested/zero"))
                .unwrap()
                .is_empty()
        );
        assert!(destination.join("model/nested/empty").is_dir());
        assert!(compress(&source.join("binary"), &compressed).is_ok());
        let invalid = dir.path().join("no.zip");
        fs::write(&invalid, b"garbage").unwrap();
        assert_eq!(
            dispatch("io.extract", &json!({"src":invalid,"dst":destination})),
            Some(json!(false))
        );
        assert_eq!(
            dispatch(
                "io.compress",
                &json!({"src":dir.path().join("missing"),"dst":compressed})
            ),
            Some(json!(false))
        );
        assert_eq!(
            dispatch(
                "io.extract",
                &json!({"src":"/not/a/file","dst":destination})
            ),
            Some(json!(false))
        );
        assert!(compress(&source, &dir.path().join("missing/out.zip")).is_err());
        assert!(dispatch("unknown", &Value::Null).is_none());
    }
    #[test]
    fn archives_reject_traversal_before_writing() {
        let root = tempfile::tempdir().unwrap();
        for name in [
            "../escape",
            "/absolute",
            "C:/drive",
            "folder\\escape",
            "a/../../escape",
        ] {
            let destination = root.path().join("out");
            assert!(
                extract(
                    Cursor::new(archive(&[("first", b"okay"), (name, b"bad")])),
                    &destination
                )
                .is_err(),
                "{name}"
            );
            assert!(!destination.join("first").exists());
        }
        assert!(!safe_relative(Path::new("")));
        assert!(
            extract(
                Cursor::new(archive(&[("a/b/file", b"nested")])),
                &root.path().join("valid")
            )
            .is_ok()
        );
    }
    #[cfg(unix)]
    #[test]
    fn archives_and_cache_refuse_symbolic_links() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let source = root.path().join("input");
        fs::create_dir_all(&source).unwrap();
        symlink(outside.path(), source.join("link")).unwrap();
        assert!(compress(&source, &root.path().join("archive.zip")).is_err());
        assert!(extract(Cursor::new(archive(&[("link/victim", b"bad")])), &source).is_err());
        assert!(!outside.path().join("victim").exists());
        symlink(outside.path(), root.path().join("archive.zip")).unwrap();
        fs::write(root.path().join("single"), b"safe").unwrap();
        assert!(
            compress(
                &root.path().join("single"),
                &root.path().join("archive.zip")
            )
            .is_err()
        );
        symlink(outside.path(), root.path().join("localhost%3A8001")).unwrap();
        assert_eq!(
            cache_list(&json!({"config":config(root.path()),"kind":"models"})),
            json!([])
        );
        let save = cache_save(
            &json!({"config":config(root.path()),"kind":"worlds","id":id(),"data":archive(&[("world.sdf",b"x")])}),
        );
        assert_eq!(save["ok"], false);
    }
    #[test]
    fn transport_preserves_binary_and_encodes_path_once() {
        let (response, message) = http(
            json!({"method":0,"version":"1.0","path":"alice/models/a%20box","query":["page=2","q=x"],"headers":["X-Test: value"],"user_agent":"fuel-rs"}),
            &[0, 255, 13, 10],
            "X-Resource-Version: 12\r\n",
        );
        assert_eq!(response["status"], 200);
        assert_eq!(bytes(&response["data"]), [0, 255, 13, 10]);
        assert_eq!(response["headers"]["X-Resource-Version"], "12");
        assert!(message.starts_with("GET /1.0/alice/models/a%20box?page=2&q=x HTTP/1.1"));
        assert!(message.to_lowercase().contains("x-test: value\r\n"));
        assert!(message.to_lowercase().contains("user-agent: fuel-rs\r\n"));
    }
    #[test]
    fn transport_supports_all_http_methods_and_body() {
        for (method, name) in [(1, "POST"), (2, "DELETE"), (3, "PUT"), (4, "PATCH")] {
            let (response, message) = http(json!({"method":method,"data":[97,0,98]}), b"ok", "");
            assert_eq!(response["status"], 200);
            assert!(message.starts_with(name));
            if method != 2 {
                assert!(message.ends_with("a\0b"));
            }
        }
    }
    #[test]
    fn transport_forms_preserve_repeated_fields_and_filenames() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("asset.PNG");
        fs::write(&file, [0, 255]).unwrap();
        for method in [5, 6] {
            let (response, message) = http(
                json!({"method":method,"form":[["tags","one"],["tags","two"],["files",format!("@{};textures/upload.png",file.display())],["files",format!("@{}",file.display())]]}),
                b"ok",
                "",
            );
            assert_eq!(response["status"], 200);
            assert!(message.contains("filename=\"textures/upload.png\""));
            assert!(message.contains("filename=\"asset.PNG\""));
            assert!(message.contains("Content-Type: image/png"));
            assert_eq!(message.matches("name=\"tags\"").count(), 2);
        }
        assert_eq!(mime("a.weird"), "application/octet-stream");
        for (extension, expected) in [
            ("config", "text/xml"),
            ("gif", "image/gif"),
            ("html", "text/html"),
            ("jpg", "image/jpeg"),
            ("json", "text/json"),
            ("pdf", "application/pdf"),
            ("svg", "image/svg+xml"),
            ("tga", "image/tga"),
            ("txt", "text/plain"),
        ] {
            assert_eq!(mime(format!("file.{extension}")), expected);
        }
    }
    #[test]
    fn transport_errors_are_structured() {
        for input in [
            json!({}),
            json!({"method":99,"url":"http://localhost"}),
            json!({"url":"http://127.0.0.1:1"}),
            json!({"url":"http://localhost","headers":["broken"]}),
            json!({"url":"http://localhost","headers":["bad name: value"]}),
            json!({"url":"http://localhost","method":5,"form":[["file","@/no/such/file"]]}),
            json!({"url":"http://localhost","method":5,"form":[[2,"value"]]}),
            json!({"url":"http://localhost","method":5,"form":[["field",2]]}),
            json!({"url":"http://localhost","path":"%FF"}),
        ] {
            let result = dispatch("io.request", &input).unwrap();
            assert_eq!(result["status"], 0);
            assert!(result["error"].is_string());
        }
        assert_eq!(bytes(&json!("abc")), b"abc");
        assert_eq!(bytes(&json!([0, 256, -1, 255])), [0, 255]);
        assert_eq!(join_url("", "x"), "x");
        assert_eq!(join_url("x", ""), "x");
    }
    #[test]
    fn cache_enumeration_filters_and_chooses_numeric_tip() {
        let root = tempfile::tempdir().unwrap();
        let config = config(root.path());
        for number in [2, 10] {
            let path = root
                .path()
                .join(format!("localhost%3A8001/alice/models/box/{number}"));
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("model.config"), "x").unwrap();
        }
        for name in ["zero/0", "bad/tip", "world/2"] {
            fs::create_dir_all(root.path().join("localhost%3A8001/alice/models").join(name))
                .unwrap();
        }
        fs::write(root.path().join("localhost%3A8001/not-owner"), "x").unwrap();
        assert_eq!(
            cache_list(&json!({"config":config,"kind":"models"}))
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let mut query = id();
        query["version"] = json!(0);
        assert_eq!(
            cache_list(&json!({"config":config,"kind":"models","id":query,"exact":true}))[0]["id"]
                ["version"],
            10
        );
        query["version"] = json!(2);
        assert_eq!(
            cache_list(&json!({"config":config,"kind":"models","id":query,"exact":true}))[0]["id"]
                ["version"],
            2
        );
        query["version"] = json!(3);
        assert_eq!(
            cache_list(&json!({"config":config,"kind":"models","id":query,"exact":true})),
            json!([])
        );
        for filter in [
            json!({"owner":"alice"}),
            json!({"name":"box"}),
            json!({"server":id()["server"]}),
        ] {
            assert_eq!(
                cache_list(&json!({"config":config,"kind":"models","id":filter}))
                    .as_array()
                    .unwrap()
                    .len(),
                2
            );
        }
        for filter in [
            json!({}),
            json!({"name":"missing"}),
            json!({"owner":"missing"}),
            json!({"server":{"url":"http://other"}}),
        ] {
            assert_eq!(
                cache_list(&json!({"config":config,"kind":"models","id":filter})),
                json!([])
            );
        }
        assert_eq!(
            cache_list(&json!({"config":config,"kind":"invalid"})),
            json!([])
        );
        assert_eq!(
            cache_list(&json!({"config":{"servers":[{"url":"invalid"}]},"kind":"models"})),
            json!([])
        );
        assert_eq!(
            server_path(&json!({"url":"http://user@host:80/prefix"})),
            Some(PathBuf::from("user%40host%3A80/prefix"))
        );
        for url in ["garbage", "http:///missing", "http://host/../bad"] {
            assert!(server_path(&json!({"url":url})).is_none());
        }
    }
    #[test]
    fn cache_saves_models_transactionally_and_rewrites_resource_uris() {
        let root = tempfile::tempdir().unwrap();
        let configuration = config(root.path());
        let identifier = id();
        let sdf=b"<sdf version='1.9'><model name='box'><link name='a'><visual name='v'><geometry><mesh><uri>model://box/meshes/a.dae</uri></mesh></geometry><material><pbr><metal><albedo_map>model://box/a.png</albedo_map></metal></pbr></material></visual></link></model><actor name='a'><skin><filename>model://box/skin.dae</filename></skin></actor><uri>file://leave</uri><uri>model://nopath</uri></sdf>";
        let packed = archive(&[
            (
                "model.config",
                b"<model><name>box</name><sdf version='1.9'>model.sdf</sdf></model>",
            ),
            ("model.sdf", sdf),
            ("a.png", &[0, 255, 10, 13]),
        ]);
        let mut input = json!({"config":configuration,"kind":"models","id":identifier,"data":packed,"overwrite":false});
        let saved = cache_save(&input);
        assert_eq!(saved["ok"], true, "{saved}");
        let path = PathBuf::from(s(&saved, "path"));
        let text = fs::read_to_string(path.join("model.sdf")).unwrap();
        assert!(text.contains("http://localhost:8001/1.0/alice/models/box/2/files/meshes/a.dae"));
        assert!(text.contains("http://localhost:8001/1.0/alice/models/box/2/files/skin.dae"));
        assert!(text.contains("file://leave"));
        assert_eq!(fs::read(path.join("a.png")).unwrap(), [0, 255, 10, 13]);
        assert_eq!(cache_save(&input)["ok"], false);
        input["overwrite"] = json!(true);
        assert_eq!(cache_save(&input)["ok"], true);
        input["data"] = json!([1, 2, 3]);
        assert_eq!(cache_save(&input)["ok"], false);
        assert!(path.join("model.sdf").exists());
        assert_eq!(
            cache_list(&json!({"config":configuration,"kind":"models"}))
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn cache_worlds_and_invalid_inputs() {
        let root = tempfile::tempdir().unwrap();
        let configuration = config(root.path());
        let input = json!({"config":configuration,"kind":"worlds","id":id(),"data":archive(&[("world.sdf",b"<sdf/>")])});
        let saved = cache_save(&input);
        assert_eq!(saved["ok"], true);
        assert_eq!(saved["id"]["local_path"], saved["path"]);
        assert_eq!(
            cache_list(&json!({"config":configuration,"kind":"worlds"}))
                .as_array()
                .unwrap()
                .len(),
            1
        );
        for change in [
            json!({"name":"../escape"}),
            json!({"owner":"a/b"}),
            json!({"version":0}),
            json!({"server":{"url":"invalid"}}),
        ] {
            let mut a = input.clone();
            for (key, value) in change.as_object().unwrap() {
                a["id"][key] = value.clone();
            }
            assert_eq!(cache_save(&a)["ok"], false);
        }
        let mut a = input.clone();
        a["kind"] = json!("invalid");
        assert_eq!(cache_save(&a)["ok"], false);
        for data in [
            archive(&[("model.sdf", b"<sdf/>")]),
            archive(&[("model.config", b"bad")]),
            archive(&[("model.config", b"<model><sdf>../escape</sdf></model>")]),
            archive(&[
                ("model.config", b"<model><sdf>x</sdf></model>"),
                ("x", b"bad xml"),
            ]),
        ] {
            let mut a = input.clone();
            a["kind"] = json!("models");
            a["data"] = json!(data);
            assert_eq!(cache_save(&a)["ok"], false);
        }
    }
    #[test]
    fn cache_install_failure_restores_original_and_metadata_models_work() {
        let root = tempfile::tempdir().unwrap();
        let existing = root.path().join("existing");
        fs::create_dir(&existing).unwrap();
        fs::write(existing.join("original"), "good").unwrap();
        assert!(install_stage(&root.path().join("missing-stage"), &existing).is_err());
        assert_eq!(
            fs::read_to_string(existing.join("original")).unwrap(),
            "good"
        );
        let packed = archive(&[
            (
                "metadata.pbtxt",
                b"name: \"box\" model { file: \"model.sdf\" }",
            ),
            ("model.sdf", b"<sdf><model name='box'/></sdf>"),
        ]);
        let configuration = config(root.path());
        let saved =
            cache_save(&json!({"config":configuration,"kind":"models","id":id(),"data":packed}));
        assert_eq!(saved["ok"], true, "{saved}");
        assert_eq!(
            cache_list(&json!({"config":configuration,"kind":"models"}))
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            server_path(&json!({"url":"http://localhost:8001/","has_authority":false})),
            Some(PathBuf::from("localhost:8001"))
        );
        let blocker = root.path().join("blocker");
        fs::write(&blocker, "not a directory").unwrap();
        let mut a = json!({"config":config(&blocker),"kind":"worlds","id":id(),"data":archive(&[("world.sdf",b"x")])});
        assert_eq!(cache_save(&a)["ok"], false);
        a["config"] = config(root.path());
        a["id"]["name"] = json!("not-a-directory");
        let parent = root
            .path()
            .join("localhost%3A8001/alice/worlds/not-a-directory");
        fs::create_dir_all(parent.parent().unwrap()).unwrap();
        fs::write(parent, "block").unwrap();
        assert_eq!(cache_save(&a)["ok"], false);
    }
    #[test]
    fn sdf_selection_metadata_config_and_fallback() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path();
        assert!(sdf_path(path).is_none());
        fs::write(path.join("wrong.sdf.txt"), "x").unwrap();
        fs::create_dir(path.join("directory.sdf")).unwrap();
        fs::write(path.join("fallback.sdf"), "x").unwrap();
        assert_eq!(sdf_path(path), Some(path.join("fallback.sdf")));
        fs::write(
            path.join("model.config"),
            "<model><sdf version='1.6'>old.sdf</sdf><sdf version='1.10'>new.sdf</sdf></model>",
        )
        .unwrap();
        assert_eq!(sdf_path(path), Some(path.join("new.sdf")));
        fs::write(path.join("metadata.pbtxt"),"# model { file: \"wrong\" }\nworld { file: \"world.sdf\" file_format { name: \"sdf\" } } ").unwrap();
        assert_eq!(sdf_path(path), Some(path.join("world.sdf")));
        fs::write(
            path.join("metadata.pbtxt"),
            "model: { file: \"escaped\\\".sdf\" }",
        )
        .unwrap();
        assert_eq!(sdf_path(path), Some(path.join("escaped\".sdf")));
        for invalid in [
            "model { file: \"../escape\" }",
            "model { file: \"unclosed",
            "model { file: \"x\"",
            "}",
            "name: \"no resource\"",
            "model { file: \"bad\\x\" }",
        ] {
            fs::write(path.join("metadata.pbtxt"), invalid).unwrap();
            assert!(sdf_path(path).is_none(), "{invalid}");
        }
        assert!(xml_sdf("<other><sdf>x</sdf></other>").is_none());
        assert!(xml_sdf("<model/>").is_none());
        assert_eq!(
            xml_sdf("<model><sdf>default.sdf</sdf></model>"),
            Some("default.sdf".to_owned())
        );
        assert_eq!(dispatch("io.sdf", &json!({"path":path})), Some(json!("")));
    }

    #[test]
    fn redirects_keep_caller_headers_on_their_origin() {
        use crate::client::tests::Server;
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let hit = Arc::new(AtomicBool::new(false));
        let seen = hit.clone();
        let target = Server::new(move |_| {
            seen.store(true, Ordering::SeqCst);
            (200, vec![], b"done".to_vec())
        });
        let url = target.url.clone();
        let redirect = Server::new(move |_| (302, vec![("Location".into(), url.clone())], vec![]));
        let mut args = json!({"url":redirect.url,"method":0,"headers":["Private-Token: secret"]});
        for header in [
            "Private-Token: secret",
            "X-Access-Token: secret",
            "Authorization: Bearer secret",
        ] {
            args["headers"] = json!([header]);
            let refused = request(&args);
            assert_eq!(refused["status"], 0);
            assert!(!hit.load(Ordering::SeqCst));
        }
        args["headers"] = json!([]);
        assert_eq!(request(&args)["status"], 200);
        assert!(hit.load(Ordering::SeqCst));
        let same = Server::new(|r| {
            if r.starts_with("GET /start") {
                (302, vec![("Location".into(), "/done".into())], vec![])
            } else {
                (200, vec![], b"same".to_vec())
            }
        });
        assert_eq!(
            request(
                &json!({"url":format!("{}/start",same.url),"headers":["Private-token: secret"]})
            )["status"],
            200
        );
        let cycle = Server::new(|_| (302, vec![("Location".into(), "/again".into())], vec![]));
        assert_eq!(request(&json!({"url":cycle.url}))["status"], 0);
    }

    #[test]
    fn legacy_xml_declarations_allow_leading_whitespace() {
        let temp = tempfile::tempdir().unwrap();
        let payload=archive(&[("model.config",b"\n<?xml version='1.0'?><model><name>Box</name><sdf version='1.6'> model.sdf </sdf></model>"),("model.sdf",b"\n<?xml version='1.0'?><sdf><model name='box'><uri>model://box/mesh.dae</uri></model></sdf>")]);
        let saved = cache_save_bytes(
            &json!({"config":config(temp.path()),"id":id(),"kind":"models","overwrite":true}),
            &payload,
        );
        assert_eq!(saved["ok"], true, "{saved}");
        let text = fs::read_to_string(Path::new(saved["path"].as_str().unwrap()).join("model.sdf"))
            .unwrap();
        assert!(text.starts_with("\n<?xml"));
        assert!(!text.contains("model://"));
        assert!(text.contains("/files/mesh.dae"));
    }
}

#[cfg(test)]
#[path = "cache_tests.rs"]
mod cache_tests;
