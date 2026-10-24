// SPDX-License-Identifier: Apache-2.0
use super::*;
use std::io::{Cursor, Read, Write};
use std::net::TcpListener;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use zip::write::SimpleFileOptions;

type Response = (u16, Vec<(String, String)>, Vec<u8>);
pub(crate) struct Server {
    pub(crate) url: String,
    stop: Arc<AtomicBool>,
    pub(crate) requests: Arc<Mutex<Vec<String>>>,
    thread: Option<JoinHandle<()>>,
}
impl Server {
    pub(crate) fn new(handler: impl Fn(&str) -> Response + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(vec![]));
        let signal = stop.clone();
        let seen = requests.clone();
        let thread = thread::spawn(move || {
            while !signal.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut socket, _)) => {
                        socket
                            .set_read_timeout(Some(Duration::from_secs(3)))
                            .unwrap();
                        let mut raw = vec![];
                        loop {
                            let mut b = [0; 4096];
                            match socket.read(&mut b) {
                                Ok(0) | Err(_) => break,
                                Ok(n) => raw.extend_from_slice(&b[..n]),
                            };
                            if let Some(end) = raw.windows(4).position(|b| b == b"\r\n\r\n") {
                                let h = String::from_utf8_lossy(&raw[..end]).to_ascii_lowercase();
                                let n = h
                                    .lines()
                                    .find_map(|l| {
                                        l.strip_prefix("content-length:")
                                            .and_then(|n| n.trim().parse::<usize>().ok())
                                    })
                                    .unwrap_or(0);
                                if raw.len() >= end + 4 + n {
                                    break;
                                }
                            }
                        }
                        let text = String::from_utf8_lossy(&raw).into_owned();
                        seen.lock().unwrap().push(text.clone());
                        let (code, headers, body) = handler(&text);
                        let mut response = format!(
                            "HTTP/1.1 {code} Status\r\nContent-Length: {}\r\nConnection: close\r\n",
                            body.len()
                        );
                        for (k, v) in headers {
                            response += &format!("{k}: {v}\r\n");
                        }
                        response += "\r\n";
                        let _ = socket.write_all(response.as_bytes());
                        let _ = socket.write_all(&body);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(e) => panic!("{e}"),
                }
            }
        });
        Self {
            url,
            stop,
            requests,
            thread: Some(thread),
        }
    }
    fn path(request: &str) -> String {
        percent_encoding::percent_decode_str(request.split_whitespace().nth(1).unwrap_or(""))
            .decode_utf8_lossy()
            .into_owned()
    }
    fn args(&self, cache: &Path, kind: &str) -> Value {
        json!({"config":{"servers":[{"url":self.url,"version":"1.0","api_key":"secret","has_authority":true}],"cache_location":cache.to_string_lossy(),"user_agent":"fuel-rust-test"},"id":{"name":"box","owner":"alice","version":2,"server":{"url":self.url,"version":"1.0","api_key":"secret","has_authority":true}},"kind":kind,"headers":[]})
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
    }
}
fn reply(code: u16, text: &str) -> Response {
    (code, vec![], text.as_bytes().to_vec())
}
fn archive(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(vec![]));
    for (name, text) in entries {
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
pub(crate) fn model_zip(meta: Option<&str>) -> Vec<u8> {
    let mut entries = vec![
        (
            "model.config",
            "<model><name>box</name><sdf version='1.10'>model.sdf</sdf></model>",
        ),
        ("model.sdf", "<sdf version='1.10'><model name='box'/></sdf>"),
    ];
    if let Some(meta) = meta {
        entries.push(("metadata.pbtxt", meta));
    }
    archive(&entries)
}
fn dispatch(op: &str, a: &Value) -> Value {
    super::dispatch(op, a).unwrap()
}

#[test]
fn upload_metadata_is_literal_and_filenames_do_not_use_delimiters() {
    let cache = tempfile::tempdir().unwrap();
    let model = tempfile::tempdir().unwrap();
    let secret = cache.path().join("sentinel");
    fs::write(&secret, "SENTINEL_MUST_NOT_BE_UPLOADED").unwrap();
    fs::write(model.path().join("model.config"), format!(
        "<model><name>@someone</name><description>@{}</description><sdf version='1.9'>model.sdf</sdf></model>", secret.display()
    )).unwrap();
    fs::write(model.path().join("model.sdf"), "<sdf/>").unwrap();
    fs::write(model.path().join("asset;literal.txt"), "asset payload").unwrap();
    let server = Server::new(|_| reply(200, "ok"));
    let mut args = server.args(cache.path(), "models");
    args["path"] = json!(model.path());
    assert_eq!(dispatch("client.upload", &args)["result"], 8);
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].contains("@someone"));
    assert!(requests[0].contains(&format!("@{}", secret.display())));
    assert!(requests[0].contains("asset;literal.txt"));
    assert!(requests[0].contains("asset payload"));
    assert!(!requests[0].contains("SENTINEL_MUST_NOT_BE_UPLOADED"));
}

#[test]
fn dependency_requests_scope_caller_headers_for_recursive_and_batch_downloads() {
    let destination = Server::new(|_| (200, vec![], model_zip(None)));
    let dependency = format!("{}/1.0/alice/models/dependency/2", destination.url);
    let payload = model_zip(Some(&format!(
        "model {{ file: \"model.sdf\" }} dependencies {{ uri: \"{dependency}\" }}"
    )));
    let source = Server::new(move |_| (200, vec![], payload.clone()));
    for batch in [false, true] {
        let cache = tempfile::tempdir().unwrap();
        let mut args = source.args(cache.path(), "models");
        args["headers"] = json!([
            "Private-Token: source-secret",
            "Authorization: Bearer source",
            "X-Custom-Secret: source"
        ]);
        args["config"]["servers"].as_array_mut().unwrap().push(json!({
            "url":destination.url,"version":"1.0","api_key":"destination-secret","has_authority":true
        }));
        if batch {
            args["ids"] = json!([args["id"]]);
            let results = dispatch("client.batch", &args);
            assert_eq!(results.as_array().unwrap().len(), 2);
            assert!(results.as_array().unwrap().iter().all(|r| r["result"] == 4));
        } else {
            assert_eq!(dispatch("client.download", &args)["result"], 4);
        }
    }
    for request in source.requests.lock().unwrap().iter() {
        assert!(
            request
                .to_lowercase()
                .contains("private-token: source-secret")
        );
        assert!(request.to_lowercase().contains("x-custom-secret: source"));
    }
    let requests = destination.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    for request in requests.iter() {
        assert!(
            request
                .to_lowercase()
                .contains("private-token: destination-secret")
        );
        assert!(!request.contains("source-secret"));
        assert!(!request.to_lowercase().contains("authorization:"));
        assert!(!request.to_lowercase().contains("x-custom-secret:"));
    }
}

#[test]
fn recursive_download_traverses_cached_dependencies_and_cycles() {
    let address = Arc::new(Mutex::new(String::new()));
    let address_handler = address.clone();
    let server = Server::new(move |request| {
        let url = address_handler.lock().unwrap();
        let next = if request.contains("/models/box/") {
            "dependency"
        } else {
            "box"
        };
        (
            200,
            vec![],
            model_zip(Some(&format!(
                "model {{ file: \"model.sdf\" }} dependencies {{ uri: \"{url}/1.0/alice/models/{next}/2\" }}"
            ))),
        )
    });
    *address.lock().unwrap() = server.url.clone();
    let cache = tempfile::tempdir().unwrap();
    let args = server.args(cache.path(), "models");
    let mut seed = args.clone();
    seed["id"]["name"] = json!("dependency");
    let payload = model_zip(Some(&format!(
        "model {{ file: \"model.sdf\" }} dependencies {{ uri: \"{}/1.0/alice/models/grandchild/2\" }}",
        server.url
    )));
    assert_eq!(crate::io::cache_save_bytes(&seed, &payload)["ok"], true);
    assert_eq!(dispatch("client.download", &args)["result"], 4);
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .any(|request| request.contains("/models/grandchild/"))
    );
    assert!(
        requests
            .iter()
            .all(|request| !request.contains("/models/dependency/"))
    );
}

#[test]
fn url_parsing_validation_and_configuration_precedence() {
    let config = json!({"servers":[{"url":"http://host/","version":"9.0","api_key":"secret"}]});
    for kind in ["models", "worlds", "collections"] {
        let p = parse_url(
            &format!("http://host/1.0/OpenRobotics/{kind}/My Model"),
            &config,
            kind,
            false,
        )
        .unwrap();
        assert_eq!(p["id"]["server"]["version"], "9.0");
        assert_eq!(
            p["id"]["name"],
            if kind == "collections" {
                "My Model"
            } else {
                "my model"
            }
        );
    }
    for url in [
        "bad",
        "http://host",
        "x!://host/a/models/b",
        "http:///a/models/b",
        "http://bad host/a/models/b",
        "http://host/a/models/b?x=1",
        "http://host/a/models/b#frag",
        "http://host/banana/a/models/b",
        "http://host/a/models/b/banana",
        "http://host/a/models/b/4294967296",
        "http://host/a/models/b/1/extra",
        "http://host/../models/b",
        "http://host/a/models/..",
        "http://host/a/models/%2e%2e",
        "http://host/%2e%2e/models/b",
        "http://host/a/models/name%2fchild",
        "http://host/a/models/%FF",
        "http://host/a/models/%00",
    ] {
        assert!(parse_url(url, &config, "models", false).is_none(), "{url}");
    }
    for url in [
        "http://host/a/models/b/1/files/../secret",
        "http://host/a/models/b/1/files/%2e%2e/secret",
        "http://host/a/models/b/1/files/%FF",
        "http://host/a/models/b/1/other/file",
        "http://host/a/models/b/1/files",
    ] {
        assert!(parse_url(url, &config, "models", true).is_none(), "{url}");
    }
    assert_eq!(
        parse_url(
            "http://host/a/models/b/tip/files/meshes/mesh.dae",
            &config,
            "models",
            true
        )
        .unwrap()["file"],
        "meshes/mesh.dae"
    );
    assert!(
        parse_url(
            "http://host/a/collections/b/1",
            &config,
            "collections",
            false
        )
        .is_none()
    );
    assert!(parse_url("http://host/a/collections/b", &config, "collections", true).is_none());
    let plain = parse_url(
        "http://host/1.0/Open Robotics/models/My Model",
        &config,
        "models",
        false,
    )
    .unwrap();
    let encoded = parse_url(
        "http://host/1.0/Open%20Robotics/models/My%20Model",
        &config,
        "models",
        false,
    )
    .unwrap();
    assert_eq!(plain, encoded);
    let literal_percent = parse_url(
        "http://host/1.0/alice/models/robot%252Fmesh",
        &config,
        "models",
        false,
    )
    .unwrap();
    assert_eq!(literal_percent["id"]["name"], "robot%2fmesh");
    assert!(api_version("1.0"));
    assert!(!api_version("1.x"));
    assert!(!api_version(".0"));
    assert_eq!(
        headers(
            &json!({"headers":["PRIVATE-TOKEN: explicit"]}),
            &json!({"server":{"api_key":"secret"}})
        ),
        json!(["PRIVATE-TOKEN: explicit"])
    );
    assert!(super::dispatch("missing", &Value::Null).is_none());
    assert!(dispatch("client.parse", &json!({"url":"bad","kind":"models"})).is_null());
}
#[test]
fn details_delete_and_error_responses() {
    let temp = tempfile::tempdir().unwrap();
    let server = Server::new(|r| {
        let p = Server::path(r);
        if r.starts_with("DELETE") {
            if p.contains("missing") {
                reply(404, "")
            } else {
                reply(204, "")
            }
        } else if p.contains("missing") {
            reply(404, "")
        } else if p.contains("badjson") {
            reply(200, "invalid")
        } else {
            reply(200, r#"{"name":"box","owner":"alice","version":5}"#)
        }
    });
    let mut a = server.args(temp.path(), "models");
    assert_eq!(dispatch("client.details", &a)["id"]["version"], 5);
    assert_eq!(dispatch("client.delete", &a)["result"], 1);
    a["id"]["name"] = json!("missing");
    assert_eq!(dispatch("client.details", &a)["result"], 7);
    assert_eq!(dispatch("client.delete", &a)["result"], 2);
    a["id"]["name"] = json!("badjson");
    assert_eq!(dispatch("client.details", &a)["result"], 7);
    a["id"]["owner"] = json!("");
    assert_eq!(dispatch("client.details", &a)["result"], 7);
    assert_eq!(dispatch("client.delete", &a)["result"], 3);
    assert_eq!(dispatch("client.download", &a)["result"], 7);
    assert_eq!(dispatch("client.patch", &a)["result"], 11);
    let requests = server.requests.lock().unwrap();
    assert!(
        requests
            .iter()
            .all(|r| r.to_ascii_lowercase().contains("private-token: secret"))
    );
    assert!(requests.iter().all(|r| r.contains("fuel-rust-test")));
}
#[test]
fn world_download_cache_files_fetch_and_explicit_version_fallback() {
    let temp = tempfile::tempdir().unwrap();
    let data = archive(&[("world.sdf", "<sdf/>")]);
    let server = Server::new(move |_| {
        (
            200,
            vec![("Content-Type".into(), "application/zip".into())],
            data.clone(),
        )
    });
    let mut a = server.args(temp.path(), "worlds");
    let downloaded = dispatch("client.download", &a);
    assert_eq!(downloaded["result"], 4);
    assert_eq!(downloaded["id"]["version"], 2);
    assert!(
        Path::new(s(&downloaded, "path"))
            .join("world.sdf")
            .is_file()
    );
    a["url"] = json!(format!(
        "{}/1.0/alice/worlds/box/2/files/world.sdf",
        server.url
    ));
    assert_eq!(dispatch("client.cached_file", &a)["result"], 5);
    assert!(
        dispatch("client.fetch", &a)
            .as_str()
            .unwrap()
            .ends_with("world.sdf")
    );
    let cached_root = Path::new(s(&downloaded, "path"));
    fs::write(cached_root.join("mesh with space.dae"), "mesh").unwrap();
    a["url"] = json!(format!(
        "{}/1.0/alice/worlds/box/2/files/mesh%20with%20space.dae",
        server.url
    ));
    assert_eq!(dispatch("client.cached_file", &a)["result"], 5);
    assert!(
        dispatch("client.cached_file", &a)["path"]
            .as_str()
            .unwrap()
            .ends_with("mesh with space.dae")
    );
    a["url"] = json!(format!(
        "{}/1.0/alice/worlds/box/2/files/%2Fetc%2Fpasswd",
        server.url
    ));
    assert_eq!(dispatch("client.cached_file", &a)["result"], 7);
    a["url"] = json!(format!("{}/1.0/alice/worlds/box/2", server.url));
    assert_eq!(dispatch("client.fetch", &a), downloaded["path"]);
    a["url"] = json!(format!(
        "{}/1.0/alice/worlds/box/2/files/missing",
        server.url
    ));
    assert_eq!(dispatch("client.cached_file", &a)["result"], 7);
    a["url"] = json!(format!("{}/1.0/alice/worlds/space%20name/2", server.url));
    let encoded_path = dispatch("client.fetch", &a);
    assert!(encoded_path.as_str().unwrap().ends_with("space name/2"));
    let before = server.requests.lock().unwrap().len();
    a["url"] = json!(format!("{}/1.0/alice/worlds/space name/2", server.url));
    assert_eq!(dispatch("client.fetch", &a), encoded_path);
    assert_eq!(server.requests.lock().unwrap().len(), before);
    a["url"] = json!("bad");
    assert_eq!(dispatch("client.cached_file", &a)["result"], 7);
    assert_eq!(dispatch("client.fetch", &a), "");
    let mut b = server.args(temp.path(), "worlds");
    b["url"] = json!(format!(
        "{}/1.0/alice/worlds/newbox/2/files/world.sdf",
        server.url
    ));
    assert!(
        dispatch("client.fetch", &b)
            .as_str()
            .unwrap()
            .ends_with("world.sdf")
    );
}
#[test]
fn download_rejects_bad_archives_content_types_and_links() {
    let temp = tempfile::tempdir().unwrap();
    for (code, mime, body) in [
        (404, "application/zip", ""),
        (200, "text/html", "<html/>"),
        (200, "application/zip", "not a zip"),
        (200, "text/plain", "not a URL"),
    ] {
        let server = Server::new(move |_| {
            (
                code,
                vec![("Content-Type".into(), mime.into())],
                body.as_bytes().to_vec(),
            )
        });
        let a = server.args(temp.path(), "models");
        assert_eq!(dispatch("client.download", &a)["result"], 7);
    }
    let bad = json!({"status":200,"headers":{"Content-Type":"text/plain"},"data":"ftp://example.com".as_bytes()});
    assert!(
        zip_bytes(
            &Value::Null,
            crate::io::Response {
                status: bad["status"].as_u64().unwrap_or(200) as u16,
                headers: bad["headers"]
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").into()))
                    .collect(),
                data: vec![],
                error: None
            }
        )
        .is_none()
    );
    assert!(
        zip_bytes(
            &Value::Null,
            crate::io::Response {
                status: 404,
                headers: Default::default(),
                data: vec![],
                error: None
            }
        )
        .is_none()
    );
}
#[test]
fn model_download_dependencies_cycles_and_invalid_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let payload = model_zip(None);
    let server = Server::new(move |_| {
        (
            200,
            vec![
                ("Content-Type".into(), "application/zip".into()),
                ("X-Ign-Resource-Version".into(), "3".into()),
            ],
            payload.clone(),
        )
    });
    let a = server.args(temp.path(), "models");
    let r = dispatch("client.download", &a);
    assert_eq!(r["result"], 4);
    assert_eq!(r["id"]["version"], 3);
    assert_eq!(r["dependencies"], json!([]));
    let base = Path::new(s(&r, "path"));
    let uri = format!("{}/1.0/alice/models/box/3", server.url);
    fs::write(base.join("metadata.pbtxt"),format!("name: \"Box\" model {{ file: \"model.sdf\" }} dependencies {{ uri: \"{uri}\" }} dependencies {{ uri: \"model://not-fetchable\" }}")).unwrap();
    let mut b = a.clone();
    b["id"] = r["id"].clone();
    b["recursive"] = json!(true);
    let deps = dispatch("client.dependencies", &b);
    assert_eq!(deps["result"], 4);
    assert_eq!(deps["dependencies"].as_array().unwrap().len(), 1);
    b["ids"] = json!([r["id"], r["id"]]);
    assert_eq!(dispatch("client.dependencies", &b)["result"], 4);
    fs::write(base.join("metadata.pbtxt"), "model {").unwrap();
    assert_eq!(dispatch("client.dependencies", &b)["result"], 7);
    fs::remove_file(base.join("metadata.pbtxt")).unwrap();
    fs::remove_file(base.join("model.config")).unwrap();
    assert_eq!(dispatch("client.dependencies", &b)["result"], 4);
    let empty = server.args(temp.path(), "models");
    assert_eq!(
        dispatch("client.dependencies", &empty)["dependencies"],
        json!([])
    );
}
#[test]
fn indirect_download_follows_valid_link_and_rejects_cycles() {
    let temp = tempfile::tempdir().unwrap();
    let address = Arc::new(Mutex::new(String::new()));
    let addr = address.clone();
    let payload = archive(&[("world.sdf", "<sdf/>")]);
    let server = Server::new(move |r| {
        if Server::path(r) == "/blob" {
            (
                200,
                vec![("Content-Type".into(), "application/octet-stream".into())],
                payload.clone(),
            )
        } else {
            (
                200,
                vec![("Content-Type".into(), "text/plain; charset=utf-8".into())],
                format!("{}/blob", addr.lock().unwrap()).into_bytes(),
            )
        }
    });
    *address.lock().unwrap() = server.url.clone();
    let a = server.args(temp.path(), "worlds");
    assert_eq!(dispatch("client.download", &a)["result"], 4);
    let addr = address.clone();
    let cycle = Server::new(move |_| {
        (
            200,
            vec![("Content-Type".into(), "text/plain".into())],
            addr.lock().unwrap().as_bytes().to_vec(),
        )
    });
    *address.lock().unwrap() = cycle.url.clone();
    let a = cycle.args(temp.path(), "worlds");
    assert_eq!(dispatch("client.download", &a)["result"], 7);
}
#[test]
fn upload_patch_fields_files_and_licenses() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("upload");
    fs::create_dir_all(dir.join("meshes")).unwrap();
    fs::write(dir.join("meshes/mesh.stl"), "solid mesh").unwrap();
    fs::write(dir.join("model.sdf"), "<sdf/>").unwrap();
    fs::write(dir.join("metadata.pbtxt"),r#"name: "Box" description: "Test" model { file: "model.sdf" } legal { license: "Apache-2.0" } tags: "one" categories { first: "a" second: "b" } annotations { key: "k" value: "v" }"#).unwrap();
    let server = Server::new(|r| {
        if Server::path(r).ends_with("licenses") {
            reply(200, r#"[{"name":"Apache-2.0","ID":9}]"#)
        } else {
            reply(200, "")
        }
    });
    let mut a = server.args(temp.path(), "models");
    a["path"] = json!(dir.to_string_lossy());
    a["private"] = json!(true);
    a["owner"] = json!("team");
    assert_eq!(dispatch("client.upload", &a)["result"], 8);
    assert_eq!(dispatch("client.patch", &a)["result"], 12);
    let requests = server.requests.lock().unwrap();
    let uploaded = requests.iter().find(|r| r.starts_with("POST")).unwrap();
    for value in [
        "name=\"private\"",
        "name=\"owner\"",
        "team",
        "name=\"tags\"",
        "one",
        "name=\"categories\"",
        "name=\"metadata\"",
        "filename=\"meshes/mesh.stl\"",
        "solid mesh",
    ] {
        assert!(uploaded.contains(value), "missing {value} in {uploaded}");
    }
    drop(requests);
    a["path"] = json!("");
    assert_eq!(dispatch("client.patch", &a)["result"], 12);
    a["path"] = json!("/does/not/exist");
    assert_eq!(dispatch("client.upload", &a)["result"], 10);
    fs::write(dir.join("metadata.pbtxt"), "invalid {").unwrap();
    a["path"] = json!(dir.to_string_lossy());
    assert_eq!(dispatch("client.upload", &a)["result"], 10);
    fs::remove_file(dir.join("metadata.pbtxt")).unwrap();
    fs::write(
        dir.join("model.config"),
        "<model><name>box</name><sdf version='1.10'>model.sdf</sdf></model>",
    )
    .unwrap();
    assert_eq!(dispatch("client.upload", &a)["result"], 8);
    let failed = Server::new(|_| reply(500, "failure"));
    let mut b = failed.args(temp.path(), "models");
    b["path"] = json!(dir.to_string_lossy());
    assert_eq!(dispatch("client.upload", &b)["result"], 10);
    assert_eq!(dispatch("client.patch", &b)["result"], 11);
    fs::write(
        dir.join("metadata.pbtxt"),
        r#"name: "Box" legal { license: "unknown" }"#,
    )
    .unwrap();
    assert_eq!(dispatch("client.upload", &a)["result"], 10);
    assert_eq!(dispatch("client.upload", &b)["result"], 10);
    #[cfg(unix)]
    {
        fs::remove_file(dir.join("metadata.pbtxt")).unwrap();
        std::os::unix::fs::symlink("model.sdf", dir.join("symlink")).unwrap();
        assert_eq!(dispatch("client.upload", &a)["result"], 10);
    }
}
#[test]
fn update_worlds_fetches_newer_version_and_reports_server_failure() {
    let temp = tempfile::tempdir().unwrap();
    let payload = archive(&[("world.sdf", "<sdf/>")]);
    let server = Server::new(move |r| {
        if Server::path(r).contains(".zip") {
            (
                200,
                vec![
                    ("Content-Type".into(), "application/zip".into()),
                    ("X-Ign-Resource-Version".into(), "4".into()),
                ],
                payload.clone(),
            )
        } else {
            reply(200, r#"{"name":"box","owner":"alice","version":4}"#)
        }
    });
    let a = server.args(temp.path(), "worlds");
    let initial = archive(&[("world.sdf", "old")]);
    assert_eq!(
        io(
            "io.cache_save",
            json!({"config":a["config"],"kind":"worlds","id":a["id"],"data":initial})
        )["ok"],
        true
    );
    assert_eq!(dispatch("client.update", &a), true);
    assert_eq!(dispatch("client.update", &a), true);
    let failed = Server::new(|_| reply(500, "failure"));
    let b = failed.args(temp.path(), "worlds");
    assert_eq!(
        io(
            "io.cache_save",
            json!({"config":b["config"],"kind":"worlds","id":b["id"],"data":archive(&[("world.sdf","old")])})
        )["ok"],
        true
    );
    assert_eq!(dispatch("client.update", &b), false);
    let native = crate::FuelClient::new(serde_json::from_value(a["config"].clone()).unwrap());
    assert!(
        native
            .parse(
                &format!("{}/1.0/alice/worlds/box", server.url),
                crate::ResourceKind::World
            )
            .is_ok()
    );
    assert!(
        native
            .fetch(&format!("{}/1.0/alice/worlds/box", server.url))
            .is_ok()
    );
    assert!(native.fetch("bad").is_err());
    let _ = crate::FuelClient::default();
}
#[test]
fn batch_jobs_duplicates_list_routes_and_licenses_dispatch() {
    let temp = tempfile::tempdir().unwrap();
    let payload = model_zip(None);
    let server = Server::new(move |r| {
        let p = Server::path(r);
        if p.ends_with("licenses") {
            reply(200, r#"[{"name":"CC0","ID":1}]"#)
        } else if p.contains("missing") {
            reply(404, "")
        } else {
            (
                200,
                vec![
                    ("Content-Type".into(), "application/zip".into()),
                    ("X-Ign-Resource-Version".into(), "2".into()),
                ],
                payload.clone(),
            )
        }
    });
    let mut a = server.args(temp.path(), "models");
    let mut second = a["id"].clone();
    second["name"] = json!("second");
    a["ids"] = json!([a["id"], second, a["id"]]);
    a["jobs"] = json!(0);
    let batch = dispatch("client.batch", &a);
    assert_eq!(batch.as_array().unwrap().len(), 2);
    assert!(batch.as_array().unwrap().iter().all(|r| r["result"] == 4));
    assert_eq!(dispatch("client.licenses", &a)["licenses"]["CC0"], 1);
    assert_eq!(
        dispatch("client.headers", &a),
        json!(["Private-token: secret"])
    );
    for (kind, owner, name, collection, want) in [
        ("models", "alice", "box", false, "alice/models/box"),
        ("models", "alice", "", false, "alice/models"),
        ("models", "", "", false, ""),
        ("worlds", "", "", false, "worlds"),
        (
            "models",
            "Team",
            "Collection",
            true,
            "Team/collections/Collection/models",
        ),
    ] {
        a["kind"] = json!(kind);
        a["id"]["owner"] = json!(owner);
        a["id"]["name"] = json!(name);
        a["collection"] = json!(collection);
        assert_eq!(dispatch("client.list_path", &a), want);
    }
    let mut worlds = server.args(temp.path(), "worlds");
    worlds["ids"] = json!([]);
    assert_eq!(dispatch("client.batch", &worlds)["ok"], true);
    worlds["ids"] = json!([worlds["id"]]);
    assert_eq!(dispatch("client.batch", &worlds)["ok"], true);
    worlds["ids"][0]["name"] = json!("missing");
    assert_eq!(dispatch("client.batch", &worlds)["ok"], false);
    let failure = Server::new(|_| reply(500, "bad"));
    assert_eq!(
        dispatch("client.delete", &failure.args(temp.path(), "models"))["result"],
        3
    );
}
#[test]
fn downloaded_dependency_is_recursive_and_dependency_failure_propagates() {
    let temp = tempfile::tempdir().unwrap();
    let address = Arc::new(Mutex::new(String::new()));
    let addr = address.clone();
    let server = Server::new(move |r| {
        let p = Server::path(r);
        if p.contains("missing") {
            return reply(404, "");
        }
        let dep = if p.contains("/box/") {
            Some(format!(
                "name: \"Box\" model {{ file: \"model.sdf\" }} dependencies {{ uri: \"{}/1.0/alice/models/dependency/2\" }}",
                addr.lock().unwrap()
            ))
        } else {
            None
        };
        (
            200,
            vec![
                ("Content-Type".into(), "application/zip".into()),
                ("X-Ign-Resource-Version".into(), "2".into()),
            ],
            model_zip(dep.as_deref()),
        )
    });
    *address.lock().unwrap() = server.url.clone();
    let mut a = server.args(temp.path(), "models");
    a["recursive"] = json!(false);
    let r = dispatch("client.download", &a);
    assert_eq!(r["result"], 4);
    assert_eq!(r["dependencies"].as_array().unwrap().len(), 1);
    assert_eq!(server.requests.lock().unwrap().len(), 1);
    a["recursive"] = json!(true);
    let r = dispatch("client.download", &a);
    assert_eq!(r["result"], 4);
    assert_eq!(server.requests.lock().unwrap().len(), 3);
    let r = dispatch("client.download", &a);
    assert_eq!(r["result"], 4);
    assert_eq!(server.requests.lock().unwrap().len(), 4);
    let addr = address.clone();
    let failing = Server::new(move |r| {
        if Server::path(r).contains("/missing/") {
            reply(404, "")
        } else {
            (
                200,
                vec![("Content-Type".into(), "application/zip".into())],
                model_zip(Some(&format!(
                    "dependencies {{ uri: \"{}/1.0/alice/models/missing/2\" }}",
                    addr.lock().unwrap()
                ))),
            )
        }
    });
    *address.lock().unwrap() = failing.url.clone();
    let a = failing.args(temp.path(), "models");
    assert_eq!(dispatch("client.download", &a)["result"], 7);
}
#[cfg(unix)]
#[test]
fn cached_file_rejects_symlink_escape() {
    let temp = tempfile::tempdir().unwrap();
    let server = Server::new(|_| reply(404, ""));
    let mut a = server.args(temp.path(), "worlds");
    let saved = io(
        "io.cache_save",
        json!({"config":a["config"],"kind":"worlds","id":a["id"],"data":archive(&[("world.sdf","world")])}),
    );
    assert_eq!(saved["ok"], true);
    let outside = temp.path().join("outside");
    fs::write(&outside, "secret").unwrap();
    std::os::unix::fs::symlink(&outside, Path::new(s(&saved, "path")).join("escape")).unwrap();
    a["url"] = json!(format!(
        "{}/1.0/alice/worlds/box/2/files/escape",
        server.url
    ));
    assert_eq!(dispatch("client.cached_file", &a)["result"], 7);
}
