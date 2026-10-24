//! Blocking transport, safe archives, and the on-disk Fuel cache.
use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use reqwest::blocking::{Client, multipart};
use serde_json::{Value, json};
use std::fs::{self, File};
use std::io::{self, Cursor, Read, Seek, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
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

fn extract<R: Read + Seek>(reader: R, destination: &Path) -> io::Result<()> {
    no_symlinks(destination)?;
    let mut archive = zip::ZipArchive::new(reader)?;
    // Validate every entry before writing anything. Backslashes and drive prefixes
    // are rejected on every platform, including when creating a Windows cache.
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        let filename = entry.name()?;
        let name = filename.trim_end_matches('/');
        if !safe_relative(Path::new(name)) || entry.is_symlink() {
            return Err(invalid("unsafe archive entry"));
        }
        no_symlinks(&destination.join(name))?;
    }
    fs::create_dir_all(destination)?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let output = destination.join(entry.name()?.as_ref());
        if entry.is_dir() {
            fs::create_dir_all(output)?;
        } else {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut file = File::create(output)?;
            io::copy(&mut entry, &mut file)?;
        }
    }
    Ok(())
}

fn sorted_children(path: &Path) -> Vec<PathBuf> {
    let mut entries = fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect::<Vec<_>>();
    entries.sort();
    entries
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
        for child in sorted_children(path) {
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

fn join_url(a: &str, b: &str) -> String {
    if a.is_empty() {
        return b.to_owned();
    }
    if b.is_empty() {
        return a.to_owned();
    }
    format!("{}/{}", a.trim_end_matches('/'), b.trim_start_matches('/'))
}
fn mime(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "config" | "dae" | "sdf" | "urdf" | "xml" => "text/xml",
        "gif" => "image/gif",
        "htm" | "html" => "text/html",
        "jpg" | "jpeg" => "image/jpeg",
        "json" | "pbtxt" => "text/json",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "tga" => "image/tga",
        "bvh" | "erb" | "obj" | "glsl" | "material" | "md" | "mtl" | "stl" | "txt" => "text/plain",
        _ => "application/octet-stream",
    }
}
/// HTTP response with an owned byte buffer. Native downloads never turn archive
/// bytes into JSON values; serialization is confined to the C++ Rest adapter.
#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub headers: std::collections::BTreeMap<String, String>,
    pub data: Vec<u8>,
    pub error: Option<String>,
}
fn request_inner(a: &Value) -> Result<Response, Box<dyn std::error::Error>> {
    let mut endpoint = join_url(s(a, "url"), s(a, "version"));
    if !s(a, "path").is_empty() {
        const SEGMENT: &percent_encoding::AsciiSet = &NON_ALPHANUMERIC
            .remove(b'-')
            .remove(b'_')
            .remove(b'.')
            .remove(b'~');
        let encoded = if a["path_encoded"] == true {
            s(a, "path").to_owned()
        } else {
            s(a, "path")
                .split('/')
                .map(|part| {
                    percent_decode_str(part)
                        .decode_utf8()
                        .map(|decoded| utf8_percent_encode(&decoded, SEGMENT).to_string())
                })
                .collect::<Result<Vec<_>, _>>()?
                .join("/")
        };
        endpoint = join_url(&endpoint, &encoded);
    }
    let query = a["query"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("&");
    if !query.is_empty() {
        endpoint.push('?');
        endpoint.push_str(&query);
    }
    let method = match a["method"].as_u64().unwrap_or(0) {
        0 => reqwest::Method::GET,
        1 | 5 => reqwest::Method::POST,
        2 => reqwest::Method::DELETE,
        3 => reqwest::Method::PUT,
        4 | 6 => reqwest::Method::PATCH,
        _ => return Err(invalid("unknown HTTP method").into()),
    };
    // Custom authentication headers have no universal names. Keep every caller
    // header on its original origin instead of guessing which contain secrets.
    let caller_headers = a["headers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .next()
        .is_some();
    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= 3 {
                return attempt.error("too many redirects");
            }
            if let Some(previous) = attempt.previous().last() {
                let next = attempt.url();
                if previous.scheme() == "https" && next.scheme() != "https" {
                    return attempt.error("HTTPS downgrade redirect refused");
                }
                if caller_headers
                    && (
                        previous.scheme(),
                        previous.host_str(),
                        previous.port_or_known_default(),
                    ) != (next.scheme(), next.host_str(), next.port_or_known_default())
                {
                    return attempt.error("cross-origin redirect with caller headers refused");
                }
            }
            attempt.follow()
        }))
        .build()?;
    let mut request = client
        .request(method, endpoint)
        .header(reqwest::header::USER_AGENT, s(a, "user_agent"));
    for header in a["headers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        let (name, value) = header
            .split_once(':')
            .ok_or_else(|| invalid("invalid HTTP header"))?;
        request = request.header(
            reqwest::header::HeaderName::from_bytes(name.trim().as_bytes())?,
            value.trim(),
        );
    }
    if matches!(a["method"].as_u64(), Some(5 | 6)) {
        let mut form = multipart::Form::new();
        for pair in a["form"].as_array().into_iter().flatten() {
            let key = pair[0]
                .as_str()
                .ok_or_else(|| invalid("invalid form field"))?
                .to_owned();
            if let Some(path) = pair[1]["file"].as_str() {
                let filename = pair[1]["filename"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        Path::new(path)
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned()
                    });
                form = form.part(
                    key,
                    multipart::Part::bytes(fs::read(path)?)
                        .file_name(filename)
                        .mime_str(mime(path))?,
                );
            } else {
                let value = pair[1]
                    .as_str()
                    .ok_or_else(|| invalid("invalid form value"))?;
                form = form.text(key, value.to_owned());
            }
        }
        request = request.multipart(form);
    } else if matches!(a["method"].as_u64(), Some(1 | 3 | 4)) {
        request = request.body(bytes(&a["data"]));
    }
    let response = request.send()?;
    let status = response.status().as_u16();
    let mut headers = std::collections::BTreeMap::<String, String>::new();
    for (key, value) in response.headers() {
        // HTTP fields are case insensitive. Keep the conventional names consumed
        // by the historical facade as aliases for old callers.
        let mut value = value.to_str()?.trim().to_owned();
        // Link is a list-valued field. Preserve every field line so a later
        // rel=prev cannot overwrite rel=next and truncate a paginated listing.
        if key.as_str() == "link"
            && let Some(previous) = headers.get("link")
        {
            value = format!("{previous}, {value}");
        }
        headers.insert(key.to_string(), value.clone());
        let conventional = key
            .as_str()
            .split('-')
            .map(|p| {
                let mut c = p.chars();
                c.next()
                    .map(|h| h.to_ascii_uppercase().to_string() + c.as_str())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join("-");
        headers.insert(conventional, value);
    }
    Ok(Response {
        status,
        headers,
        data: response.bytes()?.to_vec(),
        error: None,
    })
}
pub fn transport(a: &Value) -> Response {
    request_inner(a).unwrap_or_else(|e| Response {
        status: 0,
        headers: Default::default(),
        data: vec![],
        error: Some(e.to_string()),
    })
}
pub fn request(a: &Value) -> Value {
    // The historical Rest API encodes file fields as @path;filename. Keep that
    // convention at this adapter boundary; internal text is always literal.
    let mut a = a.clone();
    if let Some(form) = a["form"].as_array_mut() {
        for pair in form {
            if let Some(file) = pair[1].as_str().and_then(|v| v.strip_prefix('@')) {
                let (path, filename) = file.split_once(';').unwrap_or((file, ""));
                pair[1] = if filename.is_empty() {
                    json!({"file":path})
                } else {
                    json!({"file":path,"filename":filename})
                };
            }
        }
    }
    let response = transport(&a);
    json!({"status":response.status,"headers":response.headers,"data":response.data,"error":response.error})
}

pub fn server_path(server: &Value) -> Option<PathBuf> {
    let raw = s(server, "url");
    let (_, rest) = raw.split_once("://")?;
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    if authority.is_empty() {
        return None;
    }
    let authority = if server["has_authority"].as_bool().unwrap_or(true) {
        authority.replace(':', "%3A").replace('@', "%40")
    } else {
        authority.to_owned()
    };
    let path = PathBuf::from(authority).join(path.trim_matches('/'));
    // The legacy URI parser permits a colon inside a path component when its
    // authority parsing is disabled. Preserve that spelling for cache lookup.
    let safe = !path.as_os_str().is_empty()
        && path.components().all(|c| matches!(c, Component::Normal(_)))
        && !path.to_string_lossy().contains('\\');
    safe.then_some(path)
}
fn version(id: &Value) -> u64 {
    id["version"].as_u64().unwrap_or(0)
}
fn id_matches(id: &Value, candidate: &Value, exact: bool) -> bool {
    ["name", "owner"]
        .iter()
        .all(|key| (!exact && s(id, key).is_empty()) || s(id, key) == s(candidate, key))
        && ((!exact && s(&id["server"], "url").is_empty())
            || s(&id["server"], "url") == s(&candidate["server"], "url"))
        && (!exact || version(id) == 0 || version(id) == version(candidate))
}
pub fn cache_list(a: &Value) -> Value {
    let config = &a["config"];
    let exact = a["exact"].as_bool().unwrap_or(false);
    let kind = s(a, "kind");
    if !matches!(kind, "models" | "worlds") {
        return json!([]);
    }
    let filter = &a["id"];
    if !filter.is_null()
        && !exact
        && ["name", "owner"].iter().all(|k| s(filter, k).is_empty())
        && s(&filter["server"], "url").is_empty()
    {
        return json!([]);
    }
    let mut servers = config["servers"].as_array().cloned().unwrap_or_default();
    if exact {
        servers = vec![filter["server"].clone()];
    }
    let mut result = Vec::new();
    for server in servers {
        let Some(relative) = server_path(&server) else {
            continue;
        };
        let root = Path::new(s(config, "cache_location")).join(relative);
        if no_symlinks(&root).is_err() {
            continue;
        }
        for owner in sorted_children(&root).into_iter().filter(|p| p.is_dir()) {
            for name in sorted_children(&owner.join(kind))
                .into_iter()
                .filter(|p| p.is_dir())
            {
                for path in sorted_children(&name).into_iter().filter(|p| p.is_dir()) {
                    if no_symlinks(&path).is_err() {
                        continue;
                    }
                    let Some(number) = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .and_then(|n| n.parse::<u32>().ok())
                        .filter(|n| *n > 0)
                    else {
                        continue;
                    };
                    if kind == "models"
                        && !path.join("model.config").is_file()
                        && !path.join("metadata.pbtxt").is_file()
                    {
                        continue;
                    }
                    let Ok(path) = fs::canonicalize(path) else {
                        continue;
                    };
                    let id = json!({"name":name.file_name().unwrap_or_default().to_string_lossy().to_lowercase(),"owner":owner.file_name().unwrap_or_default().to_string_lossy().to_lowercase(),"server":server,"version":number,"local_path":path.to_string_lossy()});
                    if filter.is_null() || id_matches(filter, &id, exact) {
                        result.push(json!({"id":id,"path":path.to_string_lossy()}));
                    }
                }
            }
        }
    }
    if exact {
        result.sort_by_key(|v| std::cmp::Reverse(version(&v["id"])));
        result.truncate(1);
    }
    json!(result)
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
    let metadata = crate::command::parse_pbtxt(text).ok()?;
    metadata["model"]["file"]
        .as_str()
        .or_else(|| metadata["world"]["file"].as_str())
        .map(str::to_owned)
}
pub fn sdf_path(path: &Path) -> Option<PathBuf> {
    let metadata = path.join("metadata.pbtxt");
    let config = path.join("model.config");
    if metadata.exists() || config.exists() {
        let filename = if metadata.exists() {
            metadata_file(&fs::read_to_string(metadata).ok()?)
        } else {
            xml_sdf(&fs::read_to_string(config).ok()?)
        }?;
        return safe_relative(Path::new(&filename)).then(|| path.join(filename));
    }
    sorted_children(path)
        .into_iter()
        .find(|p| p.is_file() && p.extension().is_some_and(|e| e == "sdf"))
}
fn fix_model_paths(path: &Path, id: &Value) -> io::Result<()> {
    if !path.join("metadata.pbtxt").is_file() && !path.join("model.config").is_file() {
        return Err(invalid("missing model metadata"));
    }
    let file = sdf_path(path).ok_or_else(|| invalid("missing or invalid model metadata"))?;
    let text = fs::read_to_string(&file)?;
    let parse_text = text.trim_start();
    let prefix = text.len() - parse_text.len();
    let document = roxmltree::Document::parse(parse_text).map_err(|e| invalid(&e.to_string()))?;
    let mut replacements = Vec::new();
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
        let url = format!(
            "{}/{}/{}/models/{}/{}/files/{}",
            s(&id["server"], "url").trim_end_matches('/'),
            s(&id["server"], "version"),
            s(id, "owner"),
            s(id, "name"),
            version(id),
            suffix
        );
        replacements.push((
            (text_node.range().start + prefix)..(text_node.range().end + prefix),
            url.replace('&', "&amp;").replace('<', "&lt;"),
        ));
    }
    drop(document);
    let mut output = text;
    for (range, value) in replacements.into_iter().rev() {
        output.replace_range(range, &value);
    }
    fs::write(file, output)
}
fn cache_save_inner(a: &Value, data: &[u8]) -> io::Result<Value> {
    let id = &a["id"];
    let kind = s(a, "kind");
    if !matches!(kind, "models" | "worlds")
        || version(id) == 0
        || !safe_relative(Path::new(s(id, "name")))
        || !safe_relative(Path::new(s(id, "owner")))
        || s(id, "name").contains('/')
        || s(id, "owner").contains('/')
    {
        return Err(invalid("incomplete identifier"));
    }
    let relative = server_path(&id["server"]).ok_or_else(|| invalid("invalid server"))?;
    let parent = Path::new(s(&a["config"], "cache_location"))
        .join(relative)
        .join(s(id, "owner"))
        .join(kind)
        .join(s(id, "name"));
    let destination = parent.join(version(id).to_string());
    no_symlinks(&destination)?;
    if destination.exists() && !a["overwrite"].as_bool().unwrap_or(false) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "resource already cached",
        ));
    }
    fs::create_dir_all(&parent)?;
    let stage = tempfile::Builder::new()
        .prefix(".fuel-")
        .tempdir_in(&parent)?;
    extract(Cursor::new(data), stage.path())?;
    if kind == "models" {
        fix_model_paths(stage.path(), id)?;
    }
    install_stage(stage.path(), &destination)?;
    let mut id = id.clone();
    id["local_path"] = json!(destination.to_string_lossy());
    Ok(json!({"ok":true,"id":id,"path":destination.to_string_lossy()}))
}
fn install_stage(stage: &Path, destination: &Path) -> io::Result<()> {
    if !destination.exists() {
        return fs::rename(stage, destination);
    }
    let parent = destination
        .parent()
        .ok_or_else(|| invalid("cache has no parent"))?;
    let backup = tempfile::Builder::new()
        .prefix(".fuel-backup-")
        .tempdir_in(parent)?;
    let previous = backup.path().join("previous");
    fs::rename(destination, &previous)?;
    if let Err(install_error) = fs::rename(stage, destination) {
        if let Err(rollback_error) = fs::rename(&previous, destination) {
            let retained = backup.keep();
            return Err(io::Error::other(format!(
                "cache installation failed: {install_error}; rollback failed: {rollback_error}; original retained at {}",
                retained.display()
            )));
        }
        return Err(install_error);
    }
    Ok(())
}
pub fn cache_save(a: &Value) -> Value {
    cache_save_bytes(a, &bytes(&a["data"]))
}
pub fn cache_save_bytes(a: &Value, data: &[u8]) -> Value {
    cache_save_inner(a, data)
        .unwrap_or_else(|e| json!({"ok":false,"id":a["id"],"path":"","error":e.to_string()}))
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
    use std::net::TcpListener;
    use std::thread;

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
            assert_eq!(mime(&format!("file.{extension}")), expected);
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
                b"<model><sdf version='1.9'>model.sdf</sdf></model>",
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
            ("metadata.pbtxt", b"model { file: \"model.sdf\" }"),
            ("model.sdf", b"<sdf/>"),
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
        let payload=archive(&[("model.config",b"\n<?xml version='1.0'?><model><name>Box</name><sdf version='1.6'> model.sdf </sdf></model>"),("model.sdf",b"\n<?xml version='1.0'?><sdf><model><uri>model://box/mesh.dae</uri></model></sdf>")]);
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
