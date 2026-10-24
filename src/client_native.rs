// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
//! Typed domain operations shared by native callers and compatibility adapters.
use crate::{
    ClientConfig, Downloaded, Error, ErrorKind, ResourceId, ResourceKind, ServerConfig, identity,
    metadata,
    transport::{self, DownloadBudget, FormField, FormValue, Method, Request},
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

pub(crate) struct ClientCore<'a> {
    config: &'a ClientConfig,
}
fn invalid(message: &str) -> Error {
    Error::new(ErrorKind::InvalidInput, message)
}
fn metadata_error(message: &str) -> Error {
    Error::new(ErrorKind::Metadata, message)
}
pub(crate) fn archive_referral(previous: &str, next: &str) -> Result<url::Url, Error> {
    let next =
        url::Url::parse(next).map_err(|_| metadata_error("Invalid Fuel archive referral URL"))?;
    if !matches!(next.scheme(), "http" | "https") {
        return Err(metadata_error("Invalid Fuel archive referral scheme"));
    }
    if url::Url::parse(previous).is_ok_and(|url| url.scheme() == "https")
        && next.scheme() != "https"
    {
        return Err(metadata_error(
            "HTTPS Fuel archive referral downgrade refused",
        ));
    }
    Ok(next)
}
fn key(id: &ResourceId) -> (String, String, String, u32) {
    (
        id.server.url.trim_end_matches('/').to_owned(),
        id.owner.to_ascii_lowercase(),
        id.name.to_ascii_lowercase(),
        id.version,
    )
}
type ResourceKey = (String, String, String, u32);
/// One bounded frontier; duplicate identifiers never clone another work item.
struct PendingResources {
    items: VecDeque<(ResourceId, usize, String)>,
    keys: HashSet<ResourceKey>,
    limit: usize,
}
impl PendingResources {
    fn new(limit: usize) -> Self {
        Self {
            items: VecDeque::new(),
            keys: HashSet::new(),
            // A tip and its unresolved numeric version may both be pending.
            limit: limit.saturating_mul(2),
        }
    }
    fn push(
        &mut self,
        id: &ResourceId,
        depth: usize,
        origin: &str,
        seen: &HashSet<ResourceKey>,
    ) -> Result<(), Error> {
        let key = key(id);
        if seen.contains(&key) || self.keys.contains(&key) {
            return Ok(());
        }
        if self.keys.len() >= self.limit {
            return Err(Error::new(
                ErrorKind::Limit,
                "Fuel pending dependencies exceed configured resource limit",
            ));
        }
        self.keys.insert(key);
        self.items.push_back((id.clone(), depth, origin.into()));
        Ok(())
    }
    fn pop(&mut self) -> Option<(ResourceId, usize, String)> {
        let item = self.items.pop_front()?;
        self.keys.remove(&key(&item.0));
        Some(item)
    }
    fn resolve_alias(&mut self, id: &ResourceId) {
        let resolved = key(id);
        if self.keys.remove(&resolved) {
            self.items.retain(|(id, _, _)| key(id) != resolved);
        }
    }
}
fn origin(value: &str) -> Option<(String, String, Option<u16>)> {
    url::Url::parse(value).ok().map(|url| {
        (
            url.scheme().into(),
            url.host_str().unwrap_or("").into(),
            url.port_or_known_default(),
        )
    })
}
fn file_field(name: &str, value: String) -> FormField {
    FormField {
        name: name.into(),
        value: FormValue::Text(value),
    }
}
#[derive(Default, Deserialize)]
#[serde(default)]
struct WireResource {
    name: String,
    owner: String,
    version: u32,
    description: String,
    filesize: u32,
    likes: u32,
    downloads: u32,
    license_name: String,
    license_url: String,
    license_image: String,
    tags: Vec<String>,
    private: bool,
    #[serde(rename = "createdAt")]
    created_at: String,
    #[serde(rename = "updatedAt")]
    updated_at: String,
}
pub(crate) fn decode_resource(
    value: serde_json::Value,
    server: &ServerConfig,
    kind: ResourceKind,
) -> Result<ResourceId, Error> {
    let wire: WireResource = serde_json::from_value(value)?;
    if !identity::component(&wire.name) || !identity::component(&wire.owner) {
        return Err(metadata_error(
            "Fuel response lacks a valid resource name or owner",
        ));
    }
    let model = kind == ResourceKind::Model;
    Ok(ResourceId {
        name: identity::normalize(&wire.name, kind),
        owner: identity::normalize(&wire.owner, kind),
        server: server.clone(),
        version: wire.version,
        description: if model {
            wire.description
        } else {
            String::new()
        },
        file_size: if model { wire.filesize } else { 0 },
        like_count: if model { wire.likes } else { 0 },
        download_count: if model { wire.downloads } else { 0 },
        upload_date: if model {
            chrono::DateTime::parse_from_rfc3339(&wire.created_at)
                .map_or(0, |date| date.timestamp())
        } else {
            0
        },
        modify_date: if model {
            chrono::DateTime::parse_from_rfc3339(&wire.updated_at)
                .map_or(0, |date| date.timestamp())
        } else {
            0
        },
        license_name: wire.license_name,
        license_url: wire.license_url,
        license_image_url: wire.license_image,
        tags: wire.tags,
        private: wire.private,
        ..ResourceId::default()
    })
}
impl<'a> ClientCore<'a> {
    pub(crate) fn new(config: &'a ClientConfig) -> Self {
        Self { config }
    }
    fn request(
        &self,
        method: Method,
        url: url::Url,
        id: &ResourceId,
        headers: &[String],
        header_origin: Option<&str>,
        form: Vec<FormField>,
    ) -> Request {
        let mut headers =
            if header_origin.is_none_or(|source| origin(source) == origin(&id.server.url)) {
                headers.to_vec()
            } else {
                Vec::new()
            };
        if !id.server.api_key.is_empty()
            && !headers.iter().any(|line| {
                line.split_once(':')
                    .is_some_and(|(name, _)| name.trim().eq_ignore_ascii_case("private-token"))
            })
        {
            headers.push(format!("Private-Token: {}", id.server.api_key));
        }
        Request {
            method,
            url: url.into(),
            headers,
            user_agent: self.config.user_agent.clone(),
            body: Vec::new(),
            form,
        }
    }
    pub(crate) fn details(
        &self,
        kind: ResourceKind,
        id: &ResourceId,
        headers: &[String],
    ) -> Result<ResourceId, Error> {
        let id = identity::canonicalize(id, kind)?;
        let request = self.request(
            Method::Get,
            identity::resource_url(&id, kind, true, false)?,
            &id,
            headers,
            None,
            vec![],
        );
        let response = transport::send(&request, &self.config.limits)?;
        if response.status != 200 {
            return Err(Error::http(response.status, "Fuel details request failed"));
        }
        decode_resource(serde_json::from_slice(&response.data)?, &id.server, kind)
    }
    pub(crate) fn list_page(
        &self,
        kind: ResourceKind,
        server: &ServerConfig,
        path: &str,
        page: u32,
        headers: &[String],
    ) -> Result<crate::pagination::ListingPage, Error> {
        let mut url = identity::endpoint(server, &[])?;
        let base = url.path().trim_end_matches('/').to_owned();
        url.set_path(&format!("{base}/{}", path.trim_start_matches('/')));
        url.query_pairs_mut().append_pair("page", &page.to_string());
        let mut request = self.request(
            Method::Get,
            url,
            &ResourceId {
                server: server.clone(),
                ..ResourceId::default()
            },
            headers,
            None,
            vec![],
        );
        request.headers.push("Accept: application/json".into());
        let response = transport::send(&request, &self.config.limits)?;
        if response.status != 200 {
            return Err(Error::http(response.status, "Fuel listing request failed"));
        }
        let mut records: serde_json::Value =
            serde_json::from_slice(&response.data).map_err(|error| {
                Error::with_source(ErrorKind::Metadata, "Invalid Fuel listing JSON", error)
            })?;
        let parts = path
            .split('/')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        if records.is_object() && parts.len() == 3 && matches!(parts[1], "models" | "worlds") {
            records = serde_json::json!([records]);
        }
        let records = records
            .as_array()
            .ok_or_else(|| metadata_error("Fuel listing is not an array"))?;
        let items = records
            .iter()
            .map(|value| {
                decode_resource(value.clone(), server, kind).map_err(|error| {
                    Error::with_source(ErrorKind::Metadata, "Invalid Fuel listing resource", error)
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let (total, has_next) = crate::pagination::headers(&response.headers)?;
        Ok(crate::pagination::ListingPage {
            items,
            total,
            has_next,
            response_bytes: response.data.len(),
        })
    }
    fn direct_dependencies(&self, id: &ResourceId) -> Result<Vec<ResourceId>, Error> {
        Ok(
            crate::io::with_cached_resource(self.config, ResourceKind::Model, id, |entry| {
                if !entry.path.join("metadata.pbtxt").exists()
                    && !entry.path.join("model.config").exists()
                {
                    return Ok(vec![]);
                }
                let metadata = metadata::read_with_limits(&entry.path, &self.config.limits)?;
                let mut seen = HashSet::new();
                let mut dependencies = Vec::new();
                for dependency in metadata.dependencies {
                    if let Ok(parsed) =
                        identity::parse(&dependency.uri, self.config, ResourceKind::Model, false)
                        && seen.insert(key(&parsed.id))
                    {
                        if dependencies.len()
                            >= self
                                .config
                                .limits
                                .max_dependency_resources
                                .saturating_mul(2)
                        {
                            return Err(Error::new(
                                ErrorKind::Limit,
                                "Fuel direct dependencies exceed configured limit",
                            ));
                        }
                        dependencies.push(parsed.id);
                    }
                }
                Ok(dependencies)
            })?
            .unwrap_or_default(),
        )
    }
    pub(crate) fn dependencies(
        &self,
        id: &ResourceId,
        recursive: bool,
    ) -> Result<Vec<ResourceId>, Error> {
        let id = identity::canonicalize(id, ResourceKind::Model)?;
        let mut queue = VecDeque::from([(id, 0usize)]);
        let mut seen = HashSet::new();
        let mut output = Vec::new();
        let mut emitted = HashSet::new();
        while let Some((id, depth)) = queue.pop_front() {
            if !seen.insert(key(&id)) {
                continue;
            }
            self.check_graph(seen.len(), depth)?;
            for dependency in self.direct_dependencies(&id)? {
                if emitted.insert(key(&dependency)) {
                    output.push(dependency.clone());
                    if recursive {
                        queue.push_back((dependency, depth + 1));
                    }
                }
            }
        }
        Ok(output)
    }
    fn check_graph(&self, count: usize, depth: usize) -> Result<(), Error> {
        if count > self.config.limits.max_dependency_resources
            || depth > self.config.limits.max_dependency_depth
        {
            Err(Error::new(
                ErrorKind::Limit,
                "Fuel dependency graph exceeds configured limits",
            ))
        } else {
            Ok(())
        }
    }
    fn download_one(
        &self,
        kind: ResourceKind,
        id: &ResourceId,
        headers: &[String],
        header_origin: &str,
        budget: &DownloadBudget,
    ) -> Result<Downloaded, Error> {
        if kind == ResourceKind::Collection {
            return Err(invalid(
                "Collections must be listed before downloading their resources",
            ));
        }
        let id = identity::canonicalize(id, kind)?;
        let mut url = identity::resource_url(&id, kind, true, true)?;
        url.path_segments_mut()
            .map_err(|_| invalid("Invalid resource URL"))?
            .push(&format!("{}.zip", id.name));
        url.query_pairs_mut().append_pair("link", "true");
        let request = self.request(Method::Get, url, &id, headers, Some(header_origin), vec![]);
        let mut response =
            transport::send_download_with_budget(&request, &self.config.limits, budget)?;
        if response.status != 200 {
            return Err(Error::http(response.status, "Fuel download failed"));
        }
        let version = response
            .header("X-Ign-Resource-Version")
            .and_then(|version| version.trim().parse::<u32>().ok())
            .filter(|version| *version > 0)
            .unwrap_or(id.version.max(1));
        let mut referrals = HashSet::new();
        let mut hops = 0;
        loop {
            if response.status != 200 {
                return Err(Error::http(response.status, "Fuel archive referral failed"));
            }
            let content_type = response
                .header("content-type")
                .unwrap_or("")
                .split(';')
                .next()
                .unwrap_or("")
                .trim();
            match content_type {
                "text/plain" => {
                    if hops == 8 {
                        return Err(Error::new(
                            ErrorKind::Limit,
                            "Fuel archive exceeded referral limit",
                        ));
                    }
                    hops += 1;
                    let mut referral = String::new();
                    response
                        .body
                        .as_file_mut()
                        .take(self.config.limits.max_metadata_bytes.saturating_add(1))
                        .read_to_string(&mut referral)?;
                    if referral.len() as u64 > self.config.limits.max_metadata_bytes {
                        return Err(Error::new(
                            ErrorKind::Limit,
                            "Fuel archive referral exceeds byte limit",
                        ));
                    }
                    let referral = referral.trim();
                    let url = archive_referral(&response.final_url, referral)?;
                    if !referrals.insert(url.to_string()) {
                        return Err(metadata_error("Cyclic Fuel archive referral"));
                    }
                    let request = Request {
                        method: Method::Get,
                        url: url.into(),
                        headers: vec![],
                        user_agent: self.config.user_agent.clone(),
                        body: vec![],
                        form: vec![],
                    };
                    response = transport::send_download_with_budget(
                        &request,
                        &self.config.limits,
                        budget,
                    )?;
                }
                "" | "application/zip" | "application/octet-stream" | "binary/octet-stream" => {
                    let mut installed = id.clone();
                    installed.version = version;
                    response.body.as_file_mut().seek(SeekFrom::Start(0))?;
                    let entry = crate::io::save_resource(
                        self.config,
                        kind,
                        &installed,
                        response.body.as_file_mut(),
                        true,
                    )?;
                    let dependencies = if kind == ResourceKind::Model {
                        self.direct_dependencies(&entry.id)?
                    } else {
                        vec![]
                    };
                    return Ok(Downloaded {
                        id: entry.id,
                        path: entry.path,
                        dependencies,
                    });
                }
                _ => {
                    return Err(metadata_error(
                        "Fuel archive response has an unsupported content type",
                    ));
                }
            }
        }
    }
    pub(crate) fn download_shallow(
        &self,
        kind: ResourceKind,
        id: &ResourceId,
        headers: &[String],
    ) -> Result<Downloaded, Error> {
        self.download_one(
            kind,
            id,
            headers,
            &id.server.url,
            &DownloadBudget::new(self.config.limits.max_total_download_bytes),
        )
    }
    pub(crate) fn download(
        &self,
        kind: ResourceKind,
        id: &ResourceId,
        headers: &[String],
    ) -> Result<Downloaded, Error> {
        self.check_graph(1, 0)?;
        let budget = DownloadBudget::new(self.config.limits.max_total_download_bytes);
        let root = self.download_one(kind, id, headers, &id.server.url, &budget)?;
        if kind != ResourceKind::Model {
            return Ok(root);
        }
        let mut seen = HashSet::from([key(id), key(&root.id)]);
        let mut queue = PendingResources::new(self.config.limits.max_dependency_resources);
        for dependency in &root.dependencies {
            queue.push(dependency, 1, &root.id.server.url, &seen)?;
        }
        let mut count = 1;
        while let Some((id, depth, _)) = queue.pop() {
            if !seen.insert(key(&id)) {
                continue;
            }
            count += 1;
            self.check_graph(count, depth)?;
            let downloaded =
                if let Some(entry) = crate::io::lookup_resource(self.config, kind, &id)? {
                    Downloaded {
                        dependencies: self.direct_dependencies(&entry.id)?,
                        id: entry.id,
                        path: entry.path,
                    }
                } else {
                    self.download_one(kind, &id, headers, &root.id.server.url, &budget)?
                };
            seen.insert(key(&downloaded.id));
            queue.resolve_alias(&downloaded.id);
            for dependency in &downloaded.dependencies {
                queue.push(dependency, depth + 1, &root.id.server.url, &seen)?;
            }
        }
        Ok(root)
    }
    pub(crate) fn download_many(
        &self,
        kind: ResourceKind,
        ids: &[ResourceId],
        jobs: usize,
    ) -> Vec<Result<Downloaded, Error>> {
        self.download_many_with_headers(kind, ids, jobs, &[])
    }
    pub(crate) fn download_many_with_headers(
        &self,
        kind: ResourceKind,
        ids: &[ResourceId],
        jobs: usize,
        headers: &[String],
    ) -> Vec<Result<Downloaded, Error>> {
        let budget = DownloadBudget::new(self.config.limits.max_total_download_bytes);
        let jobs = jobs.clamp(1, 64);
        let mut seen = HashSet::new();
        let mut frontier = PendingResources::new(self.config.limits.max_dependency_resources);
        for id in ids {
            if let Err(error) = frontier.push(id, 0, &id.server.url, &seen) {
                return vec![Err(error)];
            }
        }
        let mut output = vec![];
        // Resolved version keys are deduplication aliases, not additional work.
        let mut scheduled = 0usize;
        while !frontier.items.is_empty() {
            let pending = frontier.items.into_iter().collect::<Vec<_>>();
            for (id, _, _) in &pending {
                seen.insert(key(id));
            }
            scheduled = scheduled.saturating_add(pending.len());
            if let Err(error) = self.check_graph(
                scheduled,
                pending
                    .iter()
                    .map(|(_, depth, _)| *depth)
                    .max()
                    .unwrap_or(0),
            ) {
                output.push(Err(error));
                break;
            }
            let mut next = PendingResources::new(self.config.limits.max_dependency_resources);
            let mut frontier_error = None;
            for chunk in pending.chunks(jobs) {
                let wave = std::thread::scope(|scope| {
                    let handles = chunk
                        .iter()
                        .map(|(id, _, source)| {
                            let budget = &budget;
                            scope
                                .spawn(move || self.download_one(kind, id, headers, source, budget))
                        })
                        .collect::<Vec<_>>();
                    handles
                        .into_iter()
                        .map(|handle| {
                            handle.join().unwrap_or_else(|_| {
                                Err(Error::new(
                                    ErrorKind::Operation,
                                    "Fuel download worker panicked",
                                ))
                            })
                        })
                        .collect::<Vec<_>>()
                });
                for (result, (_, depth, source)) in wave.into_iter().zip(chunk) {
                    if kind == ResourceKind::Model
                        && let Ok(downloaded) = &result
                    {
                        seen.insert(key(&downloaded.id));
                        next.resolve_alias(&downloaded.id);
                        for dependency in &downloaded.dependencies {
                            if let Err(error) = next.push(dependency, depth + 1, source, &seen) {
                                frontier_error.get_or_insert(error);
                            }
                        }
                    }
                    output.push(result);
                }
            }
            if let Some(error) = frontier_error {
                output.push(Err(error));
                break;
            }
            frontier = next;
        }
        output
    }
    pub(crate) fn fetch(&self, url: &str) -> Result<PathBuf, Error> {
        for kind in [ResourceKind::Model, ResourceKind::World] {
            for file in [false, true] {
                if let Ok(parsed) = identity::parse(url, self.config, kind, file) {
                    let entry = match crate::io::lookup_resource(self.config, kind, &parsed.id)? {
                        Some(entry) => entry,
                        None => {
                            let downloaded = self.download(kind, &parsed.id, &[])?;
                            crate::CachedResource {
                                id: downloaded.id,
                                path: downloaded.path,
                            }
                        }
                    };
                    if let Some(file) = parsed.file {
                        let path = entry.path.join(file);
                        let root = entry.path.canonicalize()?;
                        let actual = path.canonicalize()?;
                        if !actual.starts_with(root) || !actual.is_file() {
                            return Err(invalid(
                                "Requested file escapes its Fuel resource or is not a file",
                            ));
                        }
                        return Ok(path);
                    }
                    return Ok(entry.path);
                }
            }
        }
        Err(invalid("Invalid Fuel resource URL"))
    }
    fn upload_form(
        &self,
        path: &Path,
        id: &ResourceId,
        headers: &[String],
        patch: bool,
        owner: &str,
    ) -> Result<Vec<FormField>, Error> {
        let metadata = metadata::validate(path, ResourceKind::Model, &self.config.limits)?;
        let mut form = vec![file_field("name", metadata.name.clone())];
        if !patch || !metadata.description.is_empty() {
            form.push(file_field("description", metadata.description.clone()));
        }
        if !owner.is_empty() {
            form.push(file_field("owner", owner.into()));
        }
        if !patch || !metadata.license().is_empty() {
            let license = if metadata.license().is_empty() {
                1
            } else {
                let response = transport::send(
                    &self.request(
                        Method::Get,
                        identity::endpoint(&id.server, &["licenses"])?,
                        id,
                        headers,
                        None,
                        vec![],
                    ),
                    &self.config.limits,
                )?;
                if response.status != 200 {
                    return Err(Error::http(response.status, "Fuel license lookup failed"));
                }
                #[derive(Deserialize)]
                struct License {
                    name: String,
                    #[serde(rename = "ID")]
                    id: u32,
                }
                let licenses: Vec<License> = serde_json::from_slice(&response.data)?;
                licenses
                    .into_iter()
                    .find(|license| license.name == metadata.license())
                    .map(|license| license.id)
                    .ok_or_else(|| metadata_error("Fuel metadata names an unknown license"))?
            };
            form.push(file_field("license", license.to_string()));
        }
        if !metadata.tags.is_empty() {
            form.push(file_field("tags", metadata.tags.join(",")));
        }
        if let Some(categories) = metadata.categories {
            let names = [categories.first, categories.second]
                .into_iter()
                .filter(|name| !name.is_empty())
                .collect::<Vec<_>>();
            if !names.is_empty() {
                form.push(file_field("categories", names.join(",")));
            }
        }
        for (key, value) in metadata.annotations {
            form.push(file_field(
                "metadata",
                serde_json::json!({"key":key,"value":value}).to_string(),
            ));
        }
        fn files(
            directory: &Path,
            root: &Path,
            form: &mut Vec<FormField>,
            limits: &crate::ResourceLimits,
            count: &mut usize,
            depth: usize,
        ) -> Result<(), Error> {
            if depth > limits.max_archive_depth {
                return Err(Error::new(
                    ErrorKind::Limit,
                    "Fuel upload directory depth exceeds configured limit",
                ));
            }
            for entry in fs::read_dir(directory)? {
                let entry = entry?;
                *count = count.checked_add(1).ok_or_else(|| {
                    Error::new(ErrorKind::Limit, "Fuel upload entry counter overflow")
                })?;
                if *count > limits.max_archive_entries {
                    return Err(Error::new(
                        ErrorKind::Limit,
                        "Fuel upload contains too many filesystem entries",
                    ));
                }
                let kind = entry.file_type()?;
                if kind.is_symlink() {
                    return Err(invalid("Symbolic link in Fuel upload"));
                }
                if kind.is_dir() {
                    files(&entry.path(), root, form, limits, count, depth + 1)?;
                } else if kind.is_file() {
                    let filename = entry
                        .path()
                        .strip_prefix(root)
                        .map_err(|_| invalid("Invalid upload file path"))?
                        .to_str()
                        .ok_or_else(|| invalid("Fuel multipart filenames must be UTF-8"))?
                        .replace('\\', "/");
                    form.push(FormField {
                        name: "file".into(),
                        value: FormValue::File {
                            path: entry.path(),
                            filename: Some(filename),
                        },
                    });
                } else {
                    return Err(invalid("Unsupported special file in Fuel upload"));
                }
            }
            Ok(())
        }
        files(path, path, &mut form, &self.config.limits, &mut 0, 0)?;
        Ok(form)
    }
    pub(crate) fn upload(
        &self,
        path: &Path,
        id: &ResourceId,
        headers: &[String],
        private: bool,
        owner: &str,
    ) -> Result<(), Error> {
        let mut form = self.upload_form(path, id, headers, false, owner)?;
        form.push(file_field(
            "private",
            if private { "1" } else { "0" }.into(),
        ));
        let response = transport::send(
            &self.request(
                Method::PostForm,
                identity::endpoint(&id.server, &["models"])?,
                id,
                headers,
                None,
                form,
            ),
            &self.config.limits,
        )?;
        if response.status != 200 {
            return Err(Error::http(response.status, "Fuel upload failed"));
        }
        Ok(())
    }
    pub(crate) fn patch(
        &self,
        id: &ResourceId,
        headers: &[String],
        path: Option<&Path>,
    ) -> Result<(), Error> {
        let mut form = if let Some(path) = path {
            self.upload_form(path, id, headers, true, &id.owner)?
        } else {
            vec![]
        };
        let id = identity::canonicalize(id, ResourceKind::Model)?;
        form.push(file_field(
            "private",
            if id.private { "1" } else { "0" }.into(),
        ));
        let response = transport::send(
            &self.request(
                Method::PatchForm,
                identity::resource_url(&id, ResourceKind::Model, true, false)?,
                &id,
                headers,
                None,
                form,
            ),
            &self.config.limits,
        )?;
        if response.status != 200 {
            return Err(Error::http(response.status, "Fuel patch failed"));
        }
        Ok(())
    }
    pub(crate) fn delete(
        &self,
        kind: ResourceKind,
        id: &ResourceId,
        headers: &[String],
    ) -> Result<(), Error> {
        if kind == ResourceKind::Collection {
            return Err(invalid("Fuel collection deletion is unsupported"));
        }
        let id = identity::canonicalize(id, kind)?;
        let response = transport::send(
            &self.request(
                Method::Delete,
                identity::resource_url(&id, kind, true, false)?,
                &id,
                headers,
                None,
                vec![],
            ),
            &self.config.limits,
        )?;
        if !matches!(response.status, 200 | 204) {
            return Err(Error::http(response.status, "Fuel deletion failed"));
        }
        Ok(())
    }
    pub(crate) fn update(&self, kind: ResourceKind, headers: &[String]) -> Result<(), Error> {
        let mut latest = BTreeMap::new();
        for entry in crate::io::cached_resources(self.config, kind, None)? {
            let key = (
                entry.id.server.url.clone(),
                entry.id.owner.clone(),
                entry.id.name.clone(),
            );
            if latest
                .get(&key)
                .is_none_or(|old: &ResourceId| old.version < entry.id.version)
            {
                latest.insert(key, entry.id);
            }
        }
        let mut failure = None;
        for id in latest.into_values() {
            let update = (|| {
                let cloud = self.details(kind, &id, headers)?;
                if cloud.version > id.version {
                    self.download(kind, &cloud, headers)?;
                }
                Ok::<_, Error>(())
            })();
            if let Err(error) = update {
                failure.get_or_insert(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dense_cyclic_frontier_retains_each_pending_identity_once() {
        let count = 80;
        let ids = (0..count)
            .map(|number| ResourceId {
                name: format!("node{number}"),
                owner: "owner".into(),
                version: 1,
                ..ResourceId::default()
            })
            .collect::<Vec<_>>();
        let mut queue = PendingResources::new(count);
        let mut seen = HashSet::new();
        for id in &ids {
            queue.push(id, 1, "https://fuel.example", &seen).unwrap();
        }
        let mut high_water = queue.items.len();
        let mut processed = 0;
        while let Some((id, _, _)) = queue.pop() {
            seen.insert(key(&id));
            processed += 1;
            for dependency in &ids {
                queue
                    .push(dependency, 2, "https://fuel.example", &seen)
                    .unwrap();
                high_water = high_water.max(queue.items.len());
                assert_eq!(queue.items.len(), queue.keys.len());
            }
        }
        assert_eq!(processed, count);
        assert_eq!(high_water, count);
        let mut queue = PendingResources::new(1);
        let seen = HashSet::new();
        let mut tip = ids[0].clone();
        tip.version = 0;
        queue.push(&tip, 1, "source", &seen).unwrap();
        queue.push(&ids[0], 1, "source", &seen).unwrap();
        assert!(queue.push(&ids[1], 1, "source", &seen).is_err());
        let (tip, _, _) = queue.pop().unwrap();
        assert_eq!(tip.version, 0);
        queue.resolve_alias(&ids[0]);
        assert!(queue.pop().is_none());
    }
}
