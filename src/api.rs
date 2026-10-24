// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
//! Typed native Rust API. No Gazebo C++ dependencies are required.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fmt,
    path::{Path, PathBuf},
};

/// Fuel resource category used to select an API route.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ResourceKind {
    /// A downloadable model and its metadata.
    Model,
    /// A downloadable simulation world.
    World,
    /// A named collection containing models or worlds.
    Collection,
}
impl ResourceKind {
    /// Return the plural path segment used by the Fuel HTTP API.
    pub fn route(self) -> &'static str {
        match self {
            Self::Model => "models",
            Self::World => "worlds",
            Self::Collection => "collections",
        }
    }
}
/// Fuel endpoint and its credentials.
///
/// Defaults to `https://fuel.gazebosim.org`, API version `1.0`, an empty API
/// key, and authority-aware cache naming. This type contains credentials; avoid
/// logging its debug or serialized representation when a key is present.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    /// Server base URL, without the API-version or resource route segments.
    pub url: String,
    /// HTTP API version segment, such as `1.0`; an empty string omits it.
    pub version: String,
    /// Private token added to requests unless an explicit Private-Token header exists.
    pub api_key: String,
    /// Whether cache naming escapes `:` and `@` in the URL authority.
    ///
    /// This preserves the legacy C++ URI parser's authority/path distinction.
    pub has_authority: bool,
}
impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            url: "https://fuel.gazebosim.org".into(),
            version: "1.0".into(),
            api_key: String::new(),
            has_authority: true,
        }
    }
}
/// Server, cache, and request settings shared by a blocking client.
///
/// Defaults include the public Fuel server, a versioned Gazebo user agent, and
/// `$HOME/.gz/fuel` (`$USERPROFILE/.gz/fuel` on Windows). A nonempty existing
/// directory in `GZ_FUEL_CACHE_PATH` overrides that default; when this environment
/// variable is set, a cache-local `config.yaml` or `config.yml` is also loaded
/// if present. Configuration files may contain private tokens.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ClientConfig {
    /// Known servers used for URL matching, credentials, and cache enumeration.
    pub servers: Vec<ServerConfig>,
    /// Directory containing cached resources, grouped by server and numeric version.
    pub cache_location: PathBuf,
    /// User-Agent value sent with HTTP requests.
    pub user_agent: String,
    /// Loaded YAML configuration path, or an empty path when none has been loaded.
    pub config_path: PathBuf,
}
impl Default for ClientConfig {
    fn default() -> Self {
        let v =
            crate::core::dispatch("config_default", &json!({})).expect("default config operation");
        Self {
            servers: serde_json::from_value(v["servers"].clone()).expect("default servers"),
            cache_location: v["cache_location"].as_str().unwrap_or("").into(),
            user_agent: v["user_agent"].as_str().unwrap_or("").into(),
            config_path: v["config_path"].as_str().unwrap_or("").into(),
        }
    }
}
impl ClientConfig {
    /// Load YAML settings, merging servers by URL and updating nonempty tokens.
    ///
    /// `GZ_FUEL_CACHE_PATH`, when set, takes precedence over the YAML cache path.
    /// Without either, the home-directory cache is selected. Existing settings
    /// remain unchanged if loading fails.
    ///
    /// # Errors
    /// Returns an error for an unreadable file, invalid YAML, invalid server/cache
    /// settings, or settings that cannot be represented by this typed API.
    pub fn load(&mut self, path: impl AsRef<Path>) -> Result<(), Error> {
        let out =
            crate::core::dispatch("config_load", &json!({"config":self,"path":path.as_ref()}))
                .unwrap();
        if out["ok"] != true {
            return Err(Error("Invalid Fuel configuration".into()));
        }
        *self = serde_json::from_value(out["config"].clone()).map_err(|e| Error(e.to_string()))?;
        Ok(())
    }
}
/// Resource identity and optional server-provided metadata.
///
/// Defaults have empty names/metadata, zero numeric values, no tags, a public
/// visibility flag, an empty local path, and the default public Fuel server.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ResourceId {
    /// Resource name; model/world URL parsing normalizes ASCII letters to lowercase.
    pub name: String,
    /// Resource owner; model/world URL parsing normalizes ASCII letters to lowercase.
    pub owner: String,
    /// Hosting endpoint, API version, and credentials for this resource.
    pub server: ServerConfig,
    /// Numeric versions start at one. Zero selects the current tip.
    pub version: u32,
    /// Descriptive text supplied by the server, or empty when unavailable.
    pub description: String,
    /// Server-reported model file size in bytes, or zero when unavailable.
    pub file_size: u32,
    /// Upload time in Unix seconds; zero when absent or not parseable as RFC 3339.
    pub upload_date: i64,
    /// Modification time in Unix seconds; zero when absent or not parseable as RFC 3339.
    pub modify_date: i64,
    /// Server-reported number of likes, or zero when unavailable.
    pub like_count: u32,
    /// Server-reported number of downloads, or zero when unavailable.
    pub download_count: u32,
    /// Human-readable model license name, or empty when unavailable.
    pub license_name: String,
    /// Model license URL, or empty when unavailable.
    pub license_url: String,
    /// Model license-image URL, or empty when unavailable.
    pub license_image_url: String,
    /// Server-provided model tags.
    pub tags: Vec<String>,
    /// Visibility requested by upload/patch operations; defaults to public (`false`).
    pub private: bool,
    /// Local resource path when known; remote-only identifiers normally leave it empty.
    pub local_path: PathBuf,
}
impl Default for ResourceId {
    fn default() -> Self {
        Self {
            name: String::new(),
            owner: String::new(),
            server: ServerConfig::default(),
            version: 0,
            description: String::new(),
            file_size: 0,
            upload_date: 0,
            modify_date: 0,
            like_count: 0,
            download_count: 0,
            license_name: String::new(),
            license_url: String::new(),
            license_image_url: String::new(),
            tags: vec![],
            private: false,
            local_path: PathBuf::new(),
        }
    }
}
impl ResourceId {
    /// Resource URL with each decoded identifier component escaped exactly once.
    ///
    /// API and resource version segments are not appended. Returns an empty string
    /// if the server URL cannot be parsed or cannot contain path segments.
    pub fn url(&self, kind: ResourceKind) -> String {
        let Ok(mut url) = url::Url::parse(&self.server.url) else {
            return String::new();
        };
        let Ok(mut segments) = url.path_segments_mut() else {
            return String::new();
        };
        segments.pop_if_empty();
        if !self.owner.is_empty() {
            segments.push(&self.owner);
        }
        segments.push(kind.route());
        if !self.name.is_empty() {
            segments.push(&self.name);
        }
        drop(segments);
        url.into()
    }
}
/// A Fuel operation failure, including transport, decoding, or cache failures.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Error(
    /// Human-readable diagnostic; callers should not depend on exact wording.
    pub String,
);
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
/// A successfully installed resource and its direct model dependencies.
#[derive(Clone, Debug)]
pub struct Downloaded {
    /// Installed identifier, including the resolved numeric resource version.
    pub id: ResourceId,
    /// Directory containing the installed resource in the local cache.
    pub path: PathBuf,
    /// Direct dependencies read from model metadata; empty for worlds.
    pub dependencies: Vec<ResourceId>,
}
/// A resource version found in the local cache.
#[derive(Clone, Debug)]
pub struct CachedResource {
    /// Identifier reconstructed from the cache directory hierarchy.
    pub id: ResourceId,
    /// Directory holding this cached resource version.
    pub path: PathBuf,
}
/// Blocking client. Clone it for independent worker ownership. Batch operations
/// use bounded workers and return a result for each unique resource/dependency.
///
/// Methods accepting `headers` expect `Name: value` strings. Configured private
/// tokens fill in a missing Private-Token header. Recursive downloads scope caller
/// headers to the original origin (scheme, host, and effective port); referral
/// downloads receive no caller headers. Cross-origin HTTP redirects are refused
/// whenever request headers are present, and HTTPS downgrades are always refused.
#[derive(Clone, Debug, Default)]
pub struct FuelClient {
    /// Mutable configuration used by subsequent operations.
    pub config: ClientConfig,
}
impl FuelClient {
    /// Construct a client without making any network request.
    pub fn new(config: ClientConfig) -> Self {
        Self { config }
    }
    fn args(&self, kind: ResourceKind, id: &ResourceId, headers: &[String]) -> Value {
        json!({"config":self.config,"kind":kind.route(),"id":id,"headers":headers})
    }
    fn invoke(&self, op: &str, a: &Value) -> Result<Value, Error> {
        let value = crate::execute(op, a).map_err(Error)?;
        if let Some(code) = value["result"].as_u64()
            && ![1, 4, 5, 8, 12].contains(&code)
        {
            let r = crate::core::dispatch("result", &json!({"type":code})).unwrap();
            return Err(Error(
                r["message"]
                    .as_str()
                    .unwrap_or("Fuel request failed")
                    .to_owned(),
            ));
        }
        Ok(value)
    }
    /// Parse a resource URL without fetching it.
    ///
    /// Matching configured servers supply their API version and credentials.
    /// Model/world names and owners are normalized to ASCII lowercase; collection
    /// spelling is retained. Missing resource versions and `tip` become zero.
    ///
    /// # Errors
    /// Returns an error for malformed URLs, mismatched resource kinds, unsafe path
    /// components, or invalid numeric resource versions.
    pub fn parse(&self, url: &str, kind: ResourceKind) -> Result<ResourceId, Error> {
        let parsed = crate::client::parse_url(url, &json!(self.config), kind.route(), false)
            .ok_or_else(|| Error("Invalid Fuel URL".into()))?;
        serde_json::from_value(parsed["id"].clone()).map_err(|e| Error(e.to_string()))
    }
    /// Fetch server metadata for a model or world.
    ///
    /// # Errors
    /// Returns an error if the request fails or the response cannot be decoded.
    pub fn details(
        &self,
        kind: ResourceKind,
        id: &ResourceId,
        headers: &[String],
    ) -> Result<ResourceId, Error> {
        let v = self.invoke("client.details", &self.args(kind, id, headers))?;
        serde_json::from_value(v["id"].clone()).map_err(|e| Error(e.to_string()))
    }
    /// Download and install a model or world in the configured cache.
    ///
    /// Models also download uncached dependencies recursively, deduplicating cycles.
    /// The returned dependency list contains the root model's direct dependencies.
    /// Invalid archives are rejected before installation; failed replacement restores
    /// the previous cache entry.
    ///
    /// # Errors
    /// Returns an error for transport, archive, metadata, cache-write, or dependency
    /// failures. Previously installed resources/dependencies may remain cached.
    pub fn download(
        &self,
        kind: ResourceKind,
        id: &ResourceId,
        headers: &[String],
    ) -> Result<Downloaded, Error> {
        let v = self.invoke("client.download", &self.args(kind, id, headers))?;
        decode_download(&v)
    }
    /// Read model dependencies from local cached metadata without downloading them.
    ///
    /// `recursive` walks available cached dependency metadata with cycle detection.
    /// Missing cache entries or metadata files contribute no dependencies.
    ///
    /// # Errors
    /// Returns an error when existing dependency metadata cannot be read or parsed.
    pub fn dependencies(&self, id: &ResourceId, recursive: bool) -> Result<Vec<ResourceId>, Error> {
        let mut a = self.args(ResourceKind::Model, id, &[]);
        a["recursive"] = json!(recursive);
        let v = self.invoke("client.dependencies", &a)?;
        serde_json::from_value(v["dependencies"].clone()).map_err(|e| Error(e.to_string()))
    }
    /// Find a cached model/world version without a network request.
    ///
    /// Version zero selects the highest numeric cached version; other versions
    /// require an exact match. Returns `None` if no usable entry is found.
    pub fn cached(&self, kind: ResourceKind, id: &ResourceId) -> Option<PathBuf> {
        let mut a = self.args(kind, id, &[]);
        a["exact"] = json!(true);
        crate::io::dispatch("io.cache_list", &a)?[0]["path"]
            .as_str()
            .map(Into::into)
    }
    /// Enumerate cached versions, optionally matching an identifier's set fields.
    ///
    /// Filters match nonempty name, owner, and server URL fields; the filter's
    /// version is not used. Cache scanning is local and omits unusable entries.
    pub fn cached_resources(
        &self,
        kind: ResourceKind,
        filter: Option<&ResourceId>,
    ) -> Vec<CachedResource> {
        let mut args = json!({"config":self.config,"kind":kind.route()});
        if let Some(id) = filter {
            args["id"] = json!(id);
        }
        let entries = crate::io::cache_list(&args);
        entries
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                Some(CachedResource {
                    id: serde_json::from_value(entry["id"].clone()).ok()?,
                    path: entry["path"].as_str()?.into(),
                })
            })
            .collect()
    }
    /// List models or worlds belonging to a Fuel collection, lazily by page.
    ///
    /// No request is made until iteration. Errors follow [`ResourceIter`] semantics.
    pub fn list_collection(&self, collection: &ResourceId, kind: ResourceKind) -> ResourceIter {
        const SEGMENT: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
            .remove(b'-')
            .remove(b'_')
            .remove(b'.')
            .remove(b'~');
        let mut iterator = self.list(kind, &collection.server, None);
        iterator.args["path"] = json!(format!(
            "{}/collections/{}/{}",
            percent_encoding::utf8_percent_encode(&collection.owner, SEGMENT),
            percent_encoding::utf8_percent_encode(&collection.name, SEGMENT),
            kind.route()
        ));
        iterator
    }
    /// Resolve a model/world resource or `/files/` URL to a cached local path.
    ///
    /// A usable cached resource is reused; otherwise its archive and model
    /// dependencies are downloaded. This is not a general-purpose URL fetcher.
    ///
    /// # Errors
    /// Returns an error if the URL is invalid, downloading fails, or the requested
    /// cached file is missing or would escape its resource directory.
    pub fn fetch(&self, url: &str) -> Result<PathBuf, Error> {
        let v = self.invoke("client.fetch", &json!({"config":self.config,"url":url}))?;
        v.as_str()
            .filter(|p| !p.is_empty())
            .map(Into::into)
            .ok_or_else(|| Error("Unable to fetch Fuel resource".into()))
    }
    /// Upload a model directory and its metadata to `id.server`.
    ///
    /// `private` selects visibility; a nonempty `owner` requests upload ownership.
    /// Files are sent recursively; symbolic links are rejected.
    ///
    /// # Errors
    /// Returns an error for invalid/unreadable metadata or files, an unresolved
    /// license, or an unsuccessful server response.
    pub fn upload(
        &self,
        path: impl AsRef<Path>,
        id: &ResourceId,
        headers: &[String],
        private: bool,
        owner: &str,
    ) -> Result<(), Error> {
        let mut a = self.args(ResourceKind::Model, id, headers);
        a["path"] = json!(path.as_ref());
        a["private"] = json!(private);
        a["owner"] = json!(owner);
        self.invoke("client.upload", &a).map(|_| ())
    }
    /// Patch an existing model using `id.private` and `id.owner`.
    ///
    /// A supplied directory uploads replacement metadata/files. `None` updates
    /// visibility without supplying a replacement directory.
    ///
    /// # Errors
    /// Returns an error for invalid replacement content or a failed server request.
    pub fn patch(
        &self,
        id: &ResourceId,
        headers: &[String],
        path: Option<&Path>,
    ) -> Result<(), Error> {
        let mut a = self.args(ResourceKind::Model, id, headers);
        a["path"] = json!(path.unwrap_or(Path::new("")));
        a["private"] = json!(id.private);
        a["owner"] = json!(id.owner);
        self.invoke("client.patch", &a).map(|_| ())
    }
    /// Delete a remote model or world; this does not remove local cache entries.
    ///
    /// # Errors
    /// Returns an error when the server rejects deletion or the request fails.
    pub fn delete(
        &self,
        kind: ResourceKind,
        id: &ResourceId,
        headers: &[String],
    ) -> Result<(), Error> {
        self.invoke("client.delete", &self.args(kind, id, headers))
            .map(|_| ())
    }
    /// Check cached models/worlds for newer versions and download available updates.
    ///
    /// # Errors
    /// Returns an error if any metadata request or download fails. Other resources
    /// may have been updated successfully before the error is returned.
    pub fn update(&self, kind: ResourceKind, headers: &[String]) -> Result<(), Error> {
        let v = self.invoke(
            "client.update",
            &json!({"kind":kind.route(),"config":self.config,"headers":headers}),
        )?;
        if v == true {
            Ok(())
        } else {
            Err(Error("Some Fuel resources could not be updated".into()))
        }
    }
    /// Create a lazy remote listing, optionally restricted to one owner.
    ///
    /// The supplied server controls the endpoint and configured private token.
    /// Requests start on the first call to `next`; see [`ResourceIter`] for
    /// pagination boundaries and error behavior.
    pub fn list(
        &self,
        kind: ResourceKind,
        server: &ServerConfig,
        owner: Option<&str>,
    ) -> ResourceIter {
        const SEGMENT: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
            .remove(b'-')
            .remove(b'_')
            .remove(b'.')
            .remove(b'~');
        let path = owner
            .map(|o| {
                format!(
                    "{}/{}",
                    percent_encoding::utf8_percent_encode(o, SEGMENT),
                    kind.route()
                )
            })
            .unwrap_or_else(|| kind.route().to_owned());
        ResourceIter {
            args: json!({"server":server,"kind":kind.route(),"path":path,"path_encoded":true,"user_agent":self.config.user_agent}),
            page: 0,
            items: vec![].into_iter(),
            previous: Value::Null,
            done: false,
            received: 0,
            total: None,
        }
    }
    /// Download unique resources and model dependencies with bounded workers.
    ///
    /// `jobs` is clamped to 1–64. Duplicate server/owner/name/version requests are
    /// merged. Results include discovered dependencies, so their count need not
    /// equal `ids.len()`. Each item reports its own success or failure; a failed
    /// item does not cancel other scheduled resources.
    pub fn download_many(
        &self,
        kind: ResourceKind,
        ids: &[ResourceId],
        jobs: usize,
    ) -> Vec<Result<Downloaded, Error>> {
        let v = crate::client::dispatch(
            "client.batch",
            &json!({"kind":kind.route(),"config":self.config,"ids":ids,"jobs":jobs}),
        )
        .unwrap();
        let array = if kind == ResourceKind::World {
            &v["results"]
        } else {
            &v
        };
        array
            .as_array()
            .into_iter()
            .flatten()
            .map(|item| {
                if item["result"] == 4 {
                    decode_download(item)
                } else {
                    Err(Error("Fuel download failed".into()))
                }
            })
            .collect()
    }
}
fn decode_download(v: &Value) -> Result<Downloaded, Error> {
    Ok(Downloaded {
        id: serde_json::from_value(v["id"].clone()).map_err(|e| Error(e.to_string()))?,
        path: v["path"].as_str().unwrap_or("").into(),
        dependencies: serde_json::from_value(
            v.get("dependencies").cloned().unwrap_or_else(|| json!([])),
        )
        .map_err(|e| Error(e.to_string()))?,
    })
}
/// Lazy paginated listing. Transport, malformed response and repeated-page errors
/// are yielded once; the iterator then remains exhausted.
///
/// Requests are made one page at a time from the configured endpoint. Advertised
/// total counts and Link relations determine completion; without either, an empty
/// page ends iteration. Inconsistent pagination metadata and any HTTP failure
/// produce an error rather than an apparently successful partial listing. Items
/// already yielded remain available to the caller. This is a fused iterator.
pub struct ResourceIter {
    args: Value,
    page: u32,
    items: std::vec::IntoIter<ResourceId>,
    previous: Value,
    done: bool,
    received: usize,
    total: Option<usize>,
}
impl Iterator for ResourceIter {
    type Item = Result<ResourceId, Error>;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(id) = self.items.next() {
            return Some(Ok(id));
        }
        if self.done {
            return None;
        }
        let Some(next) = self.page.checked_add(1) else {
            self.done = true;
            return Some(Err(Error("Fuel listing page counter overflow".into())));
        };
        self.page = next;
        self.args["page"] = json!(self.page);
        let page = match crate::command::list_page_with_metadata(&self.args) {
            Ok(page) => page,
            Err(error) => {
                self.done = true;
                return Some(Err(Error(error)));
            }
        };
        self.received += page.items.as_array().map_or(0, Vec::len);
        self.total = page.total.or(self.total);
        match page.complete(self.received, self.total) {
            Ok(done) => self.done = done,
            Err(error) => {
                self.done = true;
                return Some(Err(Error(error)));
            }
        }
        if page.items.as_array().is_some_and(Vec::is_empty) {
            return None;
        }
        if page.items == self.previous {
            self.done = true;
            return Some(Err(Error("Fuel server repeated a listing page".into())));
        }
        self.previous = page.items.clone();
        match serde_json::from_value::<Vec<ResourceId>>(page.items) {
            Ok(items) => self.items = items.into_iter(),
            Err(error) => {
                self.done = true;
                return Some(Err(Error(format!("Invalid Fuel listing: {error}"))));
            }
        }
        self.items.next().map(Ok)
    }
}
impl std::iter::FusedIterator for ResourceIter {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn listing_honors_advertised_boundaries_and_rejects_truncation() {
        use crate::client::tests::Server;
        for mode in [
            "count",
            "full-count",
            "link",
            "split-link",
            "escaped-link",
            "quoted-title",
            "missing-rel",
            "broken-link",
            "first-404",
            "later-503",
            "later-404",
            "truncated",
            "conflict",
            "empty-next",
        ] {
            let server = Server::new(move |request| {
                let first = request.contains("page=1");
                let item = if first {
                    br#"[{"owner":"a","name":"first"}]"#.to_vec()
                } else {
                    br#"[{"owner":"a","name":"second"}]"#.to_vec()
                };
                match mode {
                    "count" if first => (200, vec![("X-Total-Count".into(), "1".into())], item),
                    "full-count" if first || request.contains("page=2") => {
                        (200, vec![("X-Total-Count".into(), "2".into())], item)
                    }
                    "link" if first => (
                        200,
                        vec![("Link".into(), "</1.0/models?page=2>; rel=\"next\"".into())],
                        item,
                    ),
                    "link" if request.contains("page=2") => (
                        200,
                        vec![("Link".into(), "</1.0/models?page=1>; rel=\"prev\"".into())],
                        item,
                    ),
                    "split-link" if first => (
                        200,
                        vec![
                            ("Link".into(), r#"</1.0/models?page=2>; ReL = "NeXt"; title="demo, next; rel=prev""#.into()),
                            ("Link".into(), r#"</1.0/models?page=1>; rel="prev""#.into()),
                        ],
                        item,
                    ),
                    "split-link" if request.contains("page=2") => (200, vec![("Link".into(), r#"</1.0/models?page=1>; rel="prev""#.into())], item),
                    "escaped-link" if first => (200,vec![("Link".into(),r#"</1.0/models?page=2>; foo; rel="n\ext""#.into())],item),
                    "escaped-link" if request.contains("page=2") => (200,vec![("Link".into(),r#"</1.0/models?page=1>; foo; rel="prev""#.into())],item),
                    "quoted-title" => (200,vec![("Link".into(),r#"</1.0/models?page=1>; title="demo; rel=next, not a relation"; REL = "PrEv""#.into())],item),
                    "missing-rel" => (200,vec![("Link".into(),r#"</1.0/models?page=2>; title="next""#.into())],item),
                    "broken-link" => (200,vec![("Link".into(),r#"</1.0/models?page=2>; rel="next"#.into())],item),
                    "first-404" => (404, vec![], vec![]),
                    "later-503" | "later-404" if first => (200, vec![], item),
                    "later-503" => (503, vec![], vec![]),
                    "truncated" if first => (200, vec![("X-Total-Count".into(), "2".into())], item),
                    "truncated" => (200, vec![], b"[]".to_vec()),
                    "conflict" => (
                        200,
                        vec![
                            ("X-Total-Count".into(), "2".into()),
                            ("Link".into(), "</1.0/models?page=1>; rel=\"prev\"".into()),
                        ],
                        item,
                    ),
                    "empty-next" => (
                        200,
                        vec![("Link".into(), "</1.0/models?page=2>; rel=\"next\"".into())],
                        b"[]".to_vec(),
                    ),
                    _ => (404, vec![], vec![]),
                }
            });
            let server_config = ServerConfig {
                url: server.url.clone(),
                ..Default::default()
            };
            let client = FuelClient::default();
            let mut iterator = client.list(ResourceKind::Model, &server_config, None);
            let items = iterator.by_ref().collect::<Vec<_>>();
            let success = matches!(
                mode,
                "count" | "full-count" | "link" | "split-link" | "escaped-link" | "quoted-title"
            );
            assert_eq!(
                items.iter().all(Result::is_ok),
                success,
                "{mode}: {items:?}"
            );
            assert!(iterator.next().is_none());
            let cli =
                crate::command::run(&json!({"action":"list","url":server.url,"kind":"models"}));
            assert_eq!(cli["ok"], success, "{mode}: {cli}");
            let expected_pages = if matches!(
                mode,
                "count"
                    | "quoted-title"
                    | "missing-rel"
                    | "broken-link"
                    | "first-404"
                    | "conflict"
                    | "empty-next"
            ) {
                1
            } else {
                2
            };
            assert_eq!(
                server.requests.lock().unwrap().len(),
                2 * expected_pages,
                "{mode}"
            );
        }
    }

    #[test]
    fn typed_values_and_errors() {
        let c = FuelClient::default();
        let id = c
            .parse(
                "https://fuel.gazebosim.org/1.0/Owner/models/Robot/4",
                ResourceKind::Model,
            )
            .unwrap();
        assert_eq!(id.name, "robot");
        assert!(id.url(ResourceKind::Model).contains("robot"));
        let escaped = ResourceId {
            name: "robot %2F mesh".into(),
            owner: "Open Robotics".into(),
            server: ServerConfig {
                url: "http://example.com/prefix/".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            escaped.url(ResourceKind::Model),
            "http://example.com/prefix/Open%20Robotics/models/robot%20%252F%20mesh"
        );
        let invalid = ResourceId {
            server: ServerConfig {
                url: "not a URL".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(invalid.url(ResourceKind::Model).is_empty());
        let non_base = ResourceId {
            server: ServerConfig {
                url: "mailto:user@example.com".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(non_base.url(ResourceKind::Model).is_empty());
        assert!(c.parse("bad", ResourceKind::World).is_err());
        assert_eq!(ResourceKind::Collection.route(), "collections");
        let e = Error("example".into());
        assert_eq!(e.to_string(), "example");
        let bad = ResourceId {
            server: ServerConfig {
                url: String::new(),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(c.details(ResourceKind::Model, &bad, &[]).is_err());
        assert!(c.download(ResourceKind::World, &bad, &[]).is_err());
        assert!(c.delete(ResourceKind::Model, &bad, &[]).is_err());
        assert!(c.patch(&bad, &[], None).is_err());
        assert!(c.upload("/does-not-exist", &bad, &[], false, "").is_err());
        assert!(c.fetch("bad").is_err());
        assert!(c.cached(ResourceKind::Model, &bad).is_none());
        assert!(c.dependencies(&bad, true).unwrap().is_empty());
        assert!(c.download_many(ResourceKind::Model, std::slice::from_ref(&bad), 0)[0].is_err());
        assert!(c.download_many(ResourceKind::World, &[bad], 2)[0].is_err());
        assert!(
            c.list(
                ResourceKind::World,
                &ServerConfig {
                    url: String::new(),
                    ..Default::default()
                },
                Some("owner")
            )
            .next()
            .unwrap()
            .is_err()
        );
    }
    #[test]
    fn typed_configuration() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.yaml");
        let mut c = ClientConfig::default();
        assert!(c.load(&p).is_err());
        std::fs::write(&p, "servers:\n  - url: https://example.org\n").unwrap();
        c.load(&p).unwrap();
        assert!(c.servers.iter().any(|s| s.url == "https://example.org"));
        let client = FuelClient::new(ClientConfig {
            cache_location: d.path().into(),
            ..c
        });
        assert!(client.update(ResourceKind::Model, &[]).is_ok());
        assert!(client.download_many(ResourceKind::World, &[], 1).is_empty());
        let value = json!({"id":ResourceId::default(),"path":"/cache"});
        assert_eq!(
            decode_download(&value).unwrap().path,
            PathBuf::from("/cache")
        );
        assert!(decode_download(&json!({})).is_err());
    }

    #[test]
    fn typed_client_complete_local_workflow() {
        use crate::client::tests::{Server, model_zip};
        let archive = model_zip(None);
        let server = Server::new(move |request| {
            if request.starts_with("GET ") && request.contains(".zip") {
                return (
                    200,
                    vec![("X-Ign-Resource-Version".into(), "2".into())],
                    archive.clone(),
                );
            }
            if request.starts_with("GET ") && request.contains("page=1") {
                return (
                    200,
                    vec![],
                    br#"[{"name":"box","owner":"alice","version":2}]"#.to_vec(),
                );
            }
            if request.starts_with("GET ") && request.contains("page=") {
                return (200, vec![], b"[]".to_vec());
            }
            (
                200,
                vec![],
                br#"{"name":"box","owner":"alice","version":2}"#.to_vec(),
            )
        });
        let temp = tempfile::tempdir().unwrap();
        let srv = ServerConfig {
            url: server.url.clone(),
            ..Default::default()
        };
        let client = FuelClient::new(ClientConfig {
            servers: vec![srv.clone()],
            cache_location: temp.path().into(),
            ..Default::default()
        });
        let id = ResourceId {
            name: "box".into(),
            owner: "alice".into(),
            version: 2,
            server: srv.clone(),
            ..Default::default()
        };
        assert_eq!(
            client
                .details(ResourceKind::Model, &id, &[])
                .unwrap()
                .version,
            2
        );
        let downloaded = client.download(ResourceKind::Model, &id, &[]).unwrap();
        assert!(downloaded.path.is_dir());
        assert_eq!(
            client.cached(ResourceKind::Model, &id).unwrap(),
            downloaded.path
        );
        let cached = client.cached_resources(ResourceKind::Model, Some(&id));
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].id.version, 2);
        assert_eq!(cached[0].path, downloaded.path);
        assert!(
            !client
                .cached_resources(ResourceKind::Model, None)
                .is_empty()
        );
        assert!(
            client
                .list_collection(&id, ResourceKind::Model)
                .next()
                .unwrap()
                .is_ok()
        );

        assert_eq!(
            client.fetch(&id.url(ResourceKind::Model)).unwrap(),
            downloaded.path
        );
        assert!(client.dependencies(&id, true).unwrap().is_empty());
        client
            .upload(&downloaded.path, &id, &[], true, "alice")
            .unwrap();
        client.patch(&id, &[], Some(&downloaded.path)).unwrap();
        client.delete(ResourceKind::Model, &id, &[]).unwrap();
        client.update(ResourceKind::Model, &[]).unwrap();
        assert!(client.download_many(ResourceKind::Model, std::slice::from_ref(&id), 2)[0].is_ok());
        assert!(client.download_many(ResourceKind::World, std::slice::from_ref(&id), 2)[0].is_ok());
        let mut tip = id.clone();
        tip.version = 0;
        let mut duplicate = tip.clone();
        duplicate.description = "metadata does not change resource identity".into();
        let before = server.requests.lock().unwrap().len();
        let tip_results = client.download_many(ResourceKind::Model, &[tip, duplicate], 2);
        assert_eq!(tip_results.len(), 1);
        assert_eq!(tip_results[0].as_ref().unwrap().id.version, 2);
        assert_eq!(server.requests.lock().unwrap().len(), before + 1);
        let mut listing = client.list(ResourceKind::Model, &srv, None);
        assert_eq!(listing.next().unwrap().unwrap().name, "box");
        assert!(listing.next().is_none());
        assert!(listing.next().is_none());
        let mut fail = client.clone();
        fail.config.cache_location = temp.path().join("empty");
        std::fs::create_dir_all(
            fail.config
                .cache_location
                .join(crate::io::server_path(&json!(srv)).unwrap())
                .join("alice/models/box/1"),
        )
        .unwrap();
        let cache = fail
            .config
            .cache_location
            .join(crate::io::server_path(&json!(srv)).unwrap())
            .join("alice/models/box/1");
        std::fs::write(cache.join("model.config"), "x").unwrap();
        // A newer cloud version is downloaded by the updater.
        assert!(fail.update(ResourceKind::Model, &[]).is_ok());
    }

    #[test]
    fn listing_errors_are_observable_once() {
        use crate::client::tests::Server;
        for (status, body) in [
            (500, "failure"),
            (200, "invalid JSON"),
            (200, r#"[{"name":"box","version":4294967296}]"#),
        ] {
            let server = Server::new(move |_| (status, vec![], body.as_bytes().to_vec()));
            let client = FuelClient::default();
            let srv = ServerConfig {
                url: server.url.clone(),
                ..Default::default()
            };
            let mut iter = client.list(ResourceKind::Model, &srv, None);
            assert!(iter.next().unwrap().is_err());
            assert!(iter.next().is_none());
            assert!(iter.next().is_none());
        }
        let repeated = Server::new(|_| {
            (
                200,
                vec![],
                br#"[{"name":"box","owner":"alice","version":2}]"#.to_vec(),
            )
        });
        let client = FuelClient::default();
        let srv = ServerConfig {
            url: repeated.url.clone(),
            ..Default::default()
        };
        let mut iter = client.list(ResourceKind::Model, &srv, None);
        assert_eq!(iter.next().unwrap().unwrap().name, "box");
        assert!(iter.next().unwrap().is_err());
        assert!(iter.next().is_none());
        let mut overflow = client.list(ResourceKind::Model, &srv, None);
        overflow.page = u32::MAX;
        assert!(overflow.next().unwrap().is_err());
        assert!(overflow.next().is_none());
    }
}
