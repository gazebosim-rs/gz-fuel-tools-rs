// SPDX-License-Identifier: Apache-2.0
//! Typed native Rust API. No Gazebo C++ dependencies are required.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fmt,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ResourceKind {
    Model,
    World,
    Collection,
}
impl ResourceKind {
    pub fn route(self) -> &'static str {
        match self {
            Self::Model => "models",
            Self::World => "worlds",
            Self::Collection => "collections",
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub url: String,
    pub version: String,
    pub api_key: String,
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
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ClientConfig {
    pub servers: Vec<ServerConfig>,
    pub cache_location: PathBuf,
    pub user_agent: String,
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
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ResourceId {
    pub name: String,
    pub owner: String,
    pub server: ServerConfig,
    /// Numeric versions start at one. Zero selects the current tip.
    pub version: u32,
    pub description: String,
    pub file_size: u32,
    pub upload_date: i64,
    pub modify_date: i64,
    pub like_count: u32,
    pub download_count: u32,
    pub license_name: String,
    pub license_url: String,
    pub license_image_url: String,
    pub tags: Vec<String>,
    pub private: bool,
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Error(pub String);
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
#[derive(Clone, Debug)]
pub struct Downloaded {
    pub id: ResourceId,
    pub path: PathBuf,
    pub dependencies: Vec<ResourceId>,
}
#[derive(Clone, Debug)]
pub struct CachedResource {
    pub id: ResourceId,
    pub path: PathBuf,
}
/// Blocking client. Clone it for independent worker ownership. Batch operations
/// use bounded workers and return a result for each unique resource/dependency.
#[derive(Clone, Debug, Default)]
pub struct FuelClient {
    pub config: ClientConfig,
}
impl FuelClient {
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
    pub fn parse(&self, url: &str, kind: ResourceKind) -> Result<ResourceId, Error> {
        let parsed = crate::client::parse_url(url, &json!(self.config), kind.route(), false)
            .ok_or_else(|| Error("Invalid Fuel URL".into()))?;
        serde_json::from_value(parsed["id"].clone()).map_err(|e| Error(e.to_string()))
    }
    pub fn details(
        &self,
        kind: ResourceKind,
        id: &ResourceId,
        headers: &[String],
    ) -> Result<ResourceId, Error> {
        let v = self.invoke("client.details", &self.args(kind, id, headers))?;
        serde_json::from_value(v["id"].clone()).map_err(|e| Error(e.to_string()))
    }
    pub fn download(
        &self,
        kind: ResourceKind,
        id: &ResourceId,
        headers: &[String],
    ) -> Result<Downloaded, Error> {
        let v = self.invoke("client.download", &self.args(kind, id, headers))?;
        decode_download(&v)
    }
    pub fn dependencies(&self, id: &ResourceId, recursive: bool) -> Result<Vec<ResourceId>, Error> {
        let mut a = self.args(ResourceKind::Model, id, &[]);
        a["recursive"] = json!(recursive);
        let v = self.invoke("client.dependencies", &a)?;
        serde_json::from_value(v["dependencies"].clone()).map_err(|e| Error(e.to_string()))
    }
    pub fn cached(&self, kind: ResourceKind, id: &ResourceId) -> Option<PathBuf> {
        let mut a = self.args(kind, id, &[]);
        a["exact"] = json!(true);
        crate::io::dispatch("io.cache_list", &a)?[0]["path"]
            .as_str()
            .map(Into::into)
    }
    /// Enumerate cached versions, optionally matching an identifier's set fields.
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
    pub fn fetch(&self, url: &str) -> Result<PathBuf, Error> {
        let v = self.invoke("client.fetch", &json!({"config":self.config,"url":url}))?;
        v.as_str()
            .filter(|p| !p.is_empty())
            .map(Into::into)
            .ok_or_else(|| Error("Unable to fetch Fuel resource".into()))
    }
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
    pub fn delete(
        &self,
        kind: ResourceKind,
        id: &ResourceId,
        headers: &[String],
    ) -> Result<(), Error> {
        self.invoke("client.delete", &self.args(kind, id, headers))
            .map(|_| ())
    }
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
        }
    }
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
pub struct ResourceIter {
    args: Value,
    page: u32,
    items: std::vec::IntoIter<ResourceId>,
    previous: Value,
    done: bool,
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
        let page = match crate::command::list_page(&self.args) {
            Ok(page) => page,
            Err(error) => {
                self.done = true;
                return Some(Err(Error(error)));
            }
        };
        if page.as_array().is_some_and(Vec::is_empty) {
            self.done = true;
            return None;
        }
        if page == self.previous {
            self.done = true;
            return Some(Err(Error("Fuel server repeated a listing page".into())));
        }
        self.previous = page.clone();
        match serde_json::from_value::<Vec<ResourceId>>(page) {
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
