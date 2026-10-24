// Copyright (C) 2026 Open Source Robotics Foundation
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
    let complete = meta.map(|text| {
        if text.trim_start().starts_with("name:") {
            text.to_owned()
        } else {
            format!("name: \"box\" {text}")
        }
    });
    let mut entries = vec![
        (
            "model.config",
            "<model><name>box</name><sdf version='1.10'>model.sdf</sdf></model>",
        ),
        ("model.sdf", "<sdf version='1.10'><model name='box'/></sdf>"),
    ];
    if let Some(meta) = &complete {
        entries.push(("metadata.pbtxt", meta.as_str()));
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
    fs::write(
        model.path().join("model.sdf"),
        "<sdf version='1.9'><model name='test'/></sdf>",
    )
    .unwrap();
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
    let data = archive(&[("world.sdf", "<sdf version='1.9'><model name='test'/></sdf>")]);
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
    let server = Server::new(|_| {
        (
            200,
            vec![("Content-Type".into(), "text/plain".into())],
            b"ftp://example.com/archive.zip".to_vec(),
        )
    });
    let a = server.args(temp.path(), "models");
    assert_eq!(dispatch("client.download", &a)["result"], 7);
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
    let payload = archive(&[("world.sdf", "<sdf version='1.9'><model name='test'/></sdf>")]);
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
    fs::write(
        dir.join("model.sdf"),
        "<sdf version='1.9'><model name='test'/></sdf>",
    )
    .unwrap();
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
    let payload = archive(&[("world.sdf", "<sdf version='1.9'><model name='test'/></sdf>")]);
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
    let initial = archive(&[("world.sdf", "<sdf/>")]);
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
            json!({"config":b["config"],"kind":"worlds","id":b["id"],"data":archive(&[("world.sdf","<sdf/>")])})
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
        json!({"config":a["config"],"kind":"worlds","id":a["id"],"data":archive(&[("world.sdf","<sdf/>")])}),
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

#[test]
fn private_details_and_content_only_edits_preserve_visibility_and_sparse_fields() {
    let cache = tempfile::tempdir().unwrap();
    let model = tempfile::tempdir().unwrap();
    fs::write(
        model.path().join("metadata.pbtxt"),
        "name:\"private\" model {file:\"model.sdf\"}",
    )
    .unwrap();
    fs::write(
        model.path().join("model.sdf"),
        "<sdf version='1.9'><model name='private'/></sdf>",
    )
    .unwrap();
    let server = Server::new(|request| {
        if request.starts_with("GET ") {
            reply(
                200,
                r#"{"name":"private","owner":"alice","private":true,"version":2,"description":"keep","license_name":"MIT"}"#,
            )
        } else {
            reply(200, "{}")
        }
    });
    let config = cache.path().join("config.yaml");
    fs::write(
        &config,
        format!(
            "servers:\n - url: {}\n   private-token: secret\ncache:\n path: {}\n",
            server.url,
            cache.path().display()
        ),
    )
    .unwrap();
    let native = crate::FuelClient::new(
        crate::core::load_config(&crate::ClientConfig::default(), &config).unwrap(),
    );
    let id = native
        .parse(
            &format!("{}/1.0/alice/models/private", server.url),
            crate::ResourceKind::Model,
        )
        .unwrap();
    assert!(
        native
            .details(crate::ResourceKind::Model, &id, &[])
            .unwrap()
            .private
    );
    let result = crate::command::run(
        &json!({"action":"edit","url":format!("{}/1.0/alice/models/private",server.url),"config_file":config,"path":model.path()}),
    );
    assert_eq!(result["ok"], true, "{result}");
    let requests = server.requests.lock().unwrap();
    let patch = requests
        .iter()
        .find(|request| request.starts_with("PATCH "))
        .unwrap();
    assert!(patch.contains("name=\"private\"\r\n\r\n1\r\n"));
    assert!(!patch.contains("name=\"license\""));
    assert!(!patch.contains("name=\"description\""));
}

#[test]
fn cli_explicit_endpoint_uses_matching_configured_credentials() {
    let folder = tempfile::tempdir().unwrap();
    fs::write(
        folder.path().join("metadata.pbtxt"),
        "name:\"sample\" model{file:\"model.sdf\"}",
    )
    .unwrap();
    fs::write(
        folder.path().join("model.sdf"),
        "<sdf version='1.9'><model name='sample'/></sdf>",
    )
    .unwrap();
    let server = Server::new(|request| {
        if !request
            .to_ascii_lowercase()
            .contains("private-token: secret")
        {
            return reply(401, "");
        }
        if request.starts_with("GET ") {
            (
                200,
                vec![("X-Total-Count".into(), "1".into())],
                br#"[{"owner":"alice","name":"sample"}]"#.to_vec(),
            )
        } else {
            reply(200, "{}")
        }
    });
    let config = folder.path().join("config.yaml");
    fs::write(
        &config,
        format!(
            "servers:\n - url: {}\n   private-token: secret\ncache:\n path: {}\n",
            server.url,
            folder.path().display()
        ),
    )
    .unwrap();
    for explicit in [false, true] {
        let mut args = json!({"action":"list","kind":"models","config_file":config});
        if explicit {
            args["url"] = json!(server.url);
        }
        let result = crate::command::run(&args);
        assert_eq!(result["ok"], true, "{result}");
    }
    let result = crate::command::run(
        &json!({"action":"upload","url":server.url,"config_file":config,"path":folder.path()}),
    );
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(server.requests.lock().unwrap().len(), 3);
}

#[cfg(unix)]
#[test]
fn native_non_utf8_directories_configs_and_caches_remain_paths() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    let folder = tempfile::tempdir().unwrap();
    let model = folder
        .path()
        .join(OsString::from_vec(b"model-\xff".to_vec()));
    fs::create_dir(&model).unwrap();
    fs::write(
        model.join("metadata.pbtxt"),
        "name:\"sample\" model{file:\"model.sdf\"}",
    )
    .unwrap();
    fs::write(
        model.join("model.sdf"),
        "<sdf version='1.9'><model name='sample'/></sdf>",
    )
    .unwrap();
    let cache = folder
        .path()
        .join(OsString::from_vec(b"cache-\xfe".to_vec()));
    fs::create_dir(&cache).unwrap();
    let configuration = folder
        .path()
        .join(OsString::from_vec(b"configuration-\xfd.yaml".to_vec()));
    let payload = archive(&[("world.sdf", "<sdf/>")]);
    let server = Server::new(move |request| {
        if request.starts_with("GET ") {
            (200, vec![], payload.clone())
        } else {
            reply(200, "{}")
        }
    });
    fs::write(
        &configuration,
        format!(
            "servers:\n - url: {}\n   private-token: secret\n",
            server.url
        ),
    )
    .unwrap();
    let mut config = crate::ClientConfig::default();
    config.load(&configuration).unwrap();
    assert_eq!(config.config_path, configuration);
    config.cache_location = cache.clone();
    let client = crate::FuelClient::new(config);
    let id = crate::ResourceId {
        server: crate::ServerConfig {
            url: server.url.clone(),
            ..Default::default()
        },
        ..Default::default()
    };
    client.upload(&model, &id, &[], false, "").unwrap();
    let fetched = client
        .fetch(&format!(
            "{}/1.0/alice/worlds/sample/1/files/world.sdf",
            server.url
        ))
        .unwrap();
    assert!(fetched.starts_with(&cache));
    assert_eq!(fs::read_to_string(&fetched).unwrap(), "<sdf/>");
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].contains("filename=\"model.sdf\""));
}

#[test]
fn native_resource_decoder_rejects_bad_shapes_and_numeric_overflow() {
    use crate::{ErrorKind, ResourceKind, ServerConfig};
    for value in [
        json!({}),
        json!({"name":"x","owner":".."}),
        json!({"name":"x","owner":"a","private":"true"}),
    ] {
        assert!(
            crate::client_native::decode_resource(
                value,
                &ServerConfig::default(),
                ResourceKind::Model
            )
            .is_err()
        );
    }
    for field in ["version", "filesize", "likes", "downloads"] {
        let mut record = json!({"name":"x","owner":"a"});
        record[field] = json!(u64::from(u32::MAX) + 1);
        assert_eq!(
            crate::client_native::decode_resource(
                record,
                &ServerConfig::default(),
                ResourceKind::Model
            )
            .unwrap_err()
            .kind,
            ErrorKind::Metadata
        );
    }
    let record = json!({"name":"MyCollection","owner":"OpenRobotics","private":true,"createdAt":"2024-01-01T00:00:00Z","updatedAt":"2024-02-01T00:00:00Z"});
    let collection = crate::client_native::decode_resource(
        record.clone(),
        &ServerConfig::default(),
        ResourceKind::Collection,
    )
    .unwrap();
    assert_eq!(collection.name, "MyCollection");
    assert_eq!(collection.owner, "OpenRobotics");
    assert!(collection.private);
    let model = crate::client_native::decode_resource(
        record,
        &ServerConfig::default(),
        ResourceKind::Model,
    )
    .unwrap();
    assert!(model.upload_date > 0);
    assert!(model.modify_date > model.upload_date);
}

#[test]
fn graph_limits_return_structured_failures_for_downloads_batches_and_cached_walks() {
    let address = Arc::new(Mutex::new(String::new()));
    let handler_address = address.clone();
    let server = Server::new(move |request| {
        let name = if request.contains("/models/box/") {
            "one"
        } else {
            "two"
        };
        let dependency = format!(
            "model{{file:\"model.sdf\"}} dependencies{{uri:\"{}/1.0/alice/models/{name}/2\"}}",
            handler_address.lock().unwrap()
        );
        (200, vec![], model_zip(Some(&dependency)))
    });
    *address.lock().unwrap() = server.url.clone();
    let cache = tempfile::tempdir().unwrap();
    let args = server.args(cache.path(), "models");
    let mut config = crate::core::typed_config(&args["config"]).unwrap();
    config.limits.max_dependency_depth = 0;
    let id = serde_json::from_value(args["id"].clone()).unwrap();
    let native = crate::FuelClient::new(config.clone());
    assert_eq!(
        native
            .download(crate::ResourceKind::Model, &id, &[])
            .unwrap_err()
            .kind,
        crate::ErrorKind::Limit
    );
    assert_eq!(
        native.dependencies(&id, true).unwrap_err().kind,
        crate::ErrorKind::Limit
    );
    config.limits.max_dependency_resources = 1;
    let native = crate::FuelClient::new(config);
    let results = native.download_many(crate::ResourceKind::Model, &[id.clone(), id.clone()], 2);
    assert_eq!(results.len(), 2);
    assert!(results[0].is_ok());
    assert_eq!(
        results[1].as_ref().unwrap_err().kind,
        crate::ErrorKind::Limit
    );
    assert_eq!(
        native
            .download(crate::ResourceKind::Collection, &id, &[])
            .unwrap_err()
            .kind,
        crate::ErrorKind::InvalidInput
    );
    assert_eq!(
        native
            .delete(crate::ResourceKind::Collection, &id, &[])
            .unwrap_err()
            .kind,
        crate::ErrorKind::InvalidInput
    );
}

#[test]
fn archive_referral_length_depth_and_http_failures_are_bounded() {
    let address = Arc::new(Mutex::new(String::new()));
    let handler_address = address.clone();
    let server = Server::new(move |request| {
        let page = Server::path(request)
            .trim_start_matches("/hop/")
            .parse::<usize>()
            .unwrap_or(0);
        (
            200,
            vec![("Content-Type".into(), "text/plain".into())],
            format!("{}/hop/{}", handler_address.lock().unwrap(), page + 1).into_bytes(),
        )
    });
    *address.lock().unwrap() = server.url.clone();
    let cache = tempfile::tempdir().unwrap();
    let args = server.args(cache.path(), "models");
    let config = crate::core::typed_config(&args["config"]).unwrap();
    let id = serde_json::from_value(args["id"].clone()).unwrap();
    let native = crate::FuelClient::new(config.clone());
    assert_eq!(
        native
            .download(crate::ResourceKind::Model, &id, &[])
            .unwrap_err()
            .kind,
        crate::ErrorKind::Limit
    );
    assert_eq!(server.requests.lock().unwrap().len(), 9);
    let mut small = config;
    small.limits.max_metadata_bytes = 1;
    let native = crate::FuelClient::new(small);
    assert_eq!(
        native
            .download(crate::ResourceKind::Model, &id, &[])
            .unwrap_err()
            .kind,
        crate::ErrorKind::Limit
    );
    let failed = Server::new(|request| {
        if request.contains("hop") {
            reply(503, "")
        } else {
            (
                200,
                vec![("Content-Type".into(), "text/plain".into())],
                b"http://127.0.0.1:1/hop".to_vec(),
            )
        }
    });
    let args = failed.args(cache.path(), "models");
    assert_eq!(dispatch("client.download", &args)["result"], 7);
}

#[test]
fn batch_resolved_tip_self_dependency_is_downloaded_once() {
    let address = Arc::new(Mutex::new(String::new()));
    let source = address.clone();
    let server = Server::new(move |_| {
        (
            200,
            vec![("X-Ign-Resource-Version".into(), "2".into())],
            model_zip(Some(&format!(
                "model{{file:\"model.sdf\"}} dependencies{{uri:\"{}/1.0/alice/models/box/2\"}}",
                source.lock().unwrap()
            ))),
        )
    });
    *address.lock().unwrap() = server.url.clone();
    let cache = tempfile::tempdir().unwrap();
    let args = server.args(cache.path(), "models");
    let native = crate::FuelClient::new(crate::core::typed_config(&args["config"]).unwrap());
    let mut id: crate::ResourceId = serde_json::from_value(args["id"].clone()).unwrap();
    id.version = 0;
    let results = native.download_many(crate::ResourceKind::Model, &[id], 2);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}

#[test]
fn upload_filesystem_budgets_count_entries_independently_of_metadata_fields() {
    let folder = tempfile::tempdir().unwrap();
    fs::write(
        folder.path().join("metadata.pbtxt"),
        "name:\"sample\" model{file:\"model.sdf\"}",
    )
    .unwrap();
    fs::write(
        folder.path().join("model.sdf"),
        "<sdf><model name='sample'/></sdf>",
    )
    .unwrap();
    let server = Server::new(|_| reply(200, "{}"));
    let mut config = crate::ClientConfig::default();
    config.limits.max_archive_entries = 2;
    let id = crate::ResourceId {
        server: crate::ServerConfig {
            url: server.url.clone(),
            ..Default::default()
        },
        ..Default::default()
    };
    let native = crate::FuelClient::new(config.clone());
    native.upload(folder.path(), &id, &[], false, "").unwrap();
    fs::create_dir_all(folder.path().join("one/two")).unwrap();
    config.limits.max_archive_entries = 100;
    config.limits.max_archive_depth = 1;
    let native = crate::FuelClient::new(config);
    assert_eq!(
        native
            .upload(folder.path(), &id, &[], false, "")
            .unwrap_err()
            .kind,
        crate::ErrorKind::Limit
    );
    fs::remove_dir_all(folder.path().join("one")).unwrap();
    #[cfg(unix)]
    {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        fs::write(
            folder.path().join(OsString::from_vec(vec![0xff])),
            "content",
        )
        .unwrap();
        assert_eq!(
            native
                .upload(folder.path(), &id, &[], false, "")
                .unwrap_err()
                .kind,
            crate::ErrorKind::InvalidInput
        );
    }
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}

#[test]
fn archive_referrals_apply_the_redirect_downgrade_policy() {
    use crate::client_native::archive_referral;
    assert!(
        archive_referral("https://fuel.example/archive", "http://cdn.example/archive").is_err()
    );
    assert!(
        archive_referral(
            "https://fuel.example/archive",
            "https://cdn.example/archive"
        )
        .is_ok()
    );
    assert!(archive_referral("http://fuel.example/archive", "https://cdn.example/archive").is_ok());
    assert!(archive_referral("http://fuel.example/archive", "ftp://cdn.example/archive").is_err());
    assert!(archive_referral("http://fuel.example/archive", "relative/archive").is_err());
}

#[test]
fn batch_resource_allowance_counts_scheduled_nodes_instead_of_resolved_aliases() {
    for (branches, allowance, expected_requests, complete) in [
        (false, 2, 2, true),
        (false, 1, 1, false),
        (true, 4, 4, true),
        (true, 3, 3, false),
    ] {
        let address = Arc::new(Mutex::new(String::new()));
        let response_address = address.clone();
        let server = Server::new(move |request| {
            let route = Server::path(request);
            let name = route
                .split("/models/")
                .nth(1)
                .unwrap()
                .split('/')
                .next()
                .unwrap();
            let dependencies: &[&str] = if branches {
                match name {
                    "root" => &["left", "right"],
                    "left" => &["leaf", "root/1"],
                    "right" => &["leaf", "left/1"],
                    _ => &[],
                }
            } else if name == "root" {
                &["child"]
            } else {
                &[]
            };
            let dependencies = dependencies
                .iter()
                .map(|name| {
                    format!(
                        "dependencies{{uri:\"{}/1.0/alice/models/{name}\"}}",
                        response_address.lock().unwrap()
                    )
                })
                .collect::<String>();
            (
                200,
                vec![("X-Ign-Resource-Version".into(), "1".into())],
                model_zip(Some(&format!(
                    "name:\"{name}\" model{{file:\"model.sdf\"}} {dependencies}"
                ))),
            )
        });
        *address.lock().unwrap() = server.url.clone();
        let cache = tempfile::tempdir().unwrap();
        let mut config =
            crate::core::typed_config(&server.args(cache.path(), "models")["config"]).unwrap();
        config.limits.max_dependency_resources = allowance;
        let client = crate::FuelClient::new(config);
        let root = client
            .parse(
                &format!("{}/1.0/alice/models/root", server.url),
                crate::ResourceKind::Model,
            )
            .unwrap();
        let results = client.download_many(crate::ResourceKind::Model, &[root], 2);
        assert_eq!(
            server.requests.lock().unwrap().len(),
            expected_requests,
            "branches={branches}, allowance={allowance}"
        );
        let downloaded = results
            .iter()
            .filter_map(|result| result.as_ref().ok())
            .collect::<Vec<_>>();
        assert_eq!(downloaded.len(), expected_requests);
        let unique = downloaded
            .iter()
            .map(|item| item.id.name.as_str())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(unique.len(), expected_requests);
        if complete {
            assert!(results.iter().all(Result::is_ok));
        } else {
            assert_eq!(
                results.last().unwrap().as_ref().unwrap_err().kind,
                crate::ErrorKind::Limit
            );
        }
    }
}

#[test]
fn resolved_alias_fanout_fits_its_actual_single_download_budget() {
    let address = Arc::new(Mutex::new(String::new()));
    let response_address = address.clone();
    let server = Server::new(move |request| {
        let route = Server::path(request);
        let name = route
            .split("/models/")
            .nth(1)
            .unwrap()
            .split('/')
            .next()
            .unwrap();
        let dependencies = if name == "root" {
            ["a", "a/1", "b", "b/1"]
                .iter()
                .map(|name| {
                    format!(
                        "dependencies{{uri:\"{}/1.0/alice/models/{name}\"}}",
                        response_address.lock().unwrap()
                    )
                })
                .collect::<String>()
        } else {
            String::new()
        };
        (
            200,
            vec![("X-Ign-Resource-Version".into(), "1".into())],
            model_zip(Some(&format!(
                "name:\"{name}\" model{{file:\"model.sdf\"}} {dependencies}"
            ))),
        )
    });
    *address.lock().unwrap() = server.url.clone();
    let cache = tempfile::tempdir().unwrap();
    let mut config =
        crate::core::typed_config(&server.args(cache.path(), "models")["config"]).unwrap();
    config.limits.max_dependency_resources = 3;
    let client = crate::FuelClient::new(config);
    let root = client
        .parse(
            &format!("{}/1.0/alice/models/root", server.url),
            crate::ResourceKind::Model,
        )
        .unwrap();
    client
        .download(crate::ResourceKind::Model, &root, &[])
        .unwrap();
    assert_eq!(server.requests.lock().unwrap().len(), 3);
}

#[test]
fn dense_cached_dependency_graph_finishes_with_one_request_per_resource() {
    let count = 12;
    let address = Arc::new(Mutex::new(String::new()));
    let response_address = address.clone();
    let server = Server::new(move |request| {
        let route = Server::path(request);
        let name = route
            .split("/models/")
            .nth(1)
            .unwrap()
            .split('/')
            .next()
            .unwrap();
        let dependencies = (0..count)
            .map(|number| {
                format!(
                    "dependencies{{uri:\"{}/1.0/alice/models/node{number}/1\"}}",
                    response_address.lock().unwrap()
                )
            })
            .collect::<String>();
        (
            200,
            vec![("X-Ign-Resource-Version".into(), "1".into())],
            model_zip(Some(&format!(
                "name:\"{name}\" model{{file:\"model.sdf\"}} {dependencies}"
            ))),
        )
    });
    *address.lock().unwrap() = server.url.clone();
    let cache = tempfile::tempdir().unwrap();
    let mut config =
        crate::core::typed_config(&server.args(cache.path(), "models")["config"]).unwrap();
    config.limits.max_dependency_resources = count + 1;
    let dependencies = (0..count)
        .map(|number| {
            format!(
                "dependencies{{uri:\"{}/1.0/alice/models/node{number}/1\"}}",
                server.url
            )
        })
        .collect::<String>();
    for number in 0..count {
        let id = crate::ResourceId {
            name: format!("node{number}"),
            owner: "alice".into(),
            version: 1,
            server: config.servers[0].clone(),
            ..Default::default()
        };
        let bytes = model_zip(Some(&format!(
            "name:\"node{number}\" model{{file:\"model.sdf\"}} {dependencies}"
        )));
        crate::io::save_resource(
            &config,
            crate::ResourceKind::Model,
            &id,
            Cursor::new(bytes),
            true,
        )
        .unwrap();
    }
    let client = crate::FuelClient::new(config);
    let root = client
        .parse(
            &format!("{}/1.0/alice/models/root", server.url),
            crate::ResourceKind::Model,
        )
        .unwrap();
    client
        .download(crate::ResourceKind::Model, &root, &[])
        .unwrap();
    assert_eq!(server.requests.lock().unwrap().len(), 1);
    server.requests.lock().unwrap().clear();
    let results = client.download_many(crate::ResourceKind::Model, &[root], 3);
    assert_eq!(results.len(), count + 1);
    assert!(results.iter().all(Result::is_ok));
    assert_eq!(server.requests.lock().unwrap().len(), count + 1);
}
