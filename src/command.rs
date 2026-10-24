// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
//! Command operations and resource metadata conversion, shared by all frontends.
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs};
fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
fn call(op: &str, a: &Value) -> Value {
    crate::execute(op, a).unwrap_or(Value::Null)
}

pub fn parse_pbtxt(input: &str) -> Result<Value, String> {
    crate::metadata::parse_pbtxt(input)
        .map(|metadata| crate::metadata::compatibility_value(&metadata))
        .map_err(|error| error.to_string())
}
#[cfg(test)]
fn parse_config(input: &str) -> Result<Value, String> {
    crate::metadata::parse_config(input)
        .map(|metadata| crate::metadata::compatibility_value(&metadata))
        .map_err(|error| error.to_string())
}
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn config_text(v: &Value) -> Result<String, String> {
    let kind = if v.get("model").is_some() {
        "model"
    } else if v.get("world").is_some() {
        "world"
    } else {
        return Err("Missing resource type".into());
    };
    if s(v, "name").is_empty() || s(&v[kind], "file").is_empty() {
        return Err("Missing resource name or SDF file".into());
    }
    let format_version = &v[kind]["file_format"]["version"];
    let sdf_version = if let Some(text) = format_version.as_str() {
        text.to_owned()
    } else if format_version.is_object() {
        format!(
            "{}.{}",
            format_version["major"].as_u64().unwrap_or(0),
            format_version["minor"].as_u64().unwrap_or(0)
        )
    } else {
        "1.6".into()
    };
    let mut out = format!(
        "<?xml version=\"1.0\"?>\n<{kind}>\n  <name>{}</name>\n  <version>{}</version>\n  <sdf version=\"{}\">{}</sdf>\n",
        xml_escape(s(v, "name")),
        v["version"].as_i64().unwrap_or(1),
        xml_escape(&sdf_version),
        xml_escape(s(&v[kind], "file"))
    );
    for author in v["authors"].as_array().unwrap_or(&vec![]) {
        out.push_str(&format!(
            "  <author><name>{}</name><email>{}</email></author>\n",
            xml_escape(s(author, "name")),
            xml_escape(s(author, "email"))
        ));
    }
    for dep in v["dependencies"].as_array().unwrap_or(&vec![]) {
        out.push_str(&format!(
            "  <depend><model><uri>{}</uri></model></depend>\n",
            xml_escape(s(dep, "uri"))
        ));
    }
    out.push_str(&format!(
        "  <description>{}</description>\n</{kind}>\n",
        xml_escape(s(v, "description"))
    ));
    Ok(out)
}
#[cfg(test)]
use crate::pagination::link_has_next;
fn listing_settings(
    a: &Value,
) -> Result<
    (
        crate::ClientConfig,
        crate::ServerConfig,
        crate::ResourceKind,
        Vec<String>,
    ),
    String,
> {
    let mut config = if a["config"].is_object() {
        crate::core::typed_config(&a["config"]).map_err(|error| error.to_string())?
    } else {
        crate::ClientConfig::default()
    };
    if let Some(user_agent) = a["user_agent"].as_str() {
        config.user_agent = user_agent.into();
    }
    let server = serde_json::from_value::<crate::ServerConfig>(a["server"].clone())
        .map_err(|error| error.to_string())?;
    let kind = match s(a, "kind") {
        "models" => crate::ResourceKind::Model,
        "worlds" => crate::ResourceKind::World,
        "collections" => crate::ResourceKind::Collection,
        _ => return Err("Invalid Fuel resource type".into()),
    };
    let headers = a["headers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    Ok((config, server, kind, headers))
}
pub fn list_page(a: &Value) -> Result<Value, String> {
    let (config, server, kind, headers) = listing_settings(a)?;
    crate::client::ClientCore::new(&config)
        .list_page(
            kind,
            &server,
            s(a, "path"),
            a["page"].as_u64().unwrap_or(1) as u32,
            &headers,
        )
        .map(|page| json!(page.items))
        .map_err(|error| error.to_string())
}
fn page(a: &Value) -> Value {
    list_page(a).unwrap_or_else(|_| json!([]))
}
fn listing(a: &Value) -> Result<Value, String> {
    let (config, server, kind, headers) = listing_settings(a)?;
    let mut pager = crate::pagination::Pager::new(
        config,
        kind,
        server,
        s(a, "path").replace('\\', "/"),
        headers,
    );
    let mut items = vec![];
    while let Some(page) = pager.next_page().map_err(|error| error.to_string())? {
        items.extend(page);
    }
    Ok(json!(items))
}

fn truth(v: &str) -> bool {
    v == "1" || v.eq_ignore_ascii_case("true")
}
fn configured(a: &Value) -> Result<Value, String> {
    let mut config = call("config_default", &json!({}));
    if !s(a, "config_file").is_empty() {
        config["servers"] = json!([]);
        let loaded = call(
            "config_load",
            &json!({"config":config,"path":a["config_file"]}),
        );
        if loaded["ok"] != true {
            return Err("Unable to load configuration\n".into());
        }
        config = loaded["config"].clone();
    }
    config["user_agent"] = json!("FuelTools 12.0.0~pre2");
    Ok(config)
}
fn finish(ok: bool, output: String) -> Value {
    json!({"ok":ok,"output":output})
}
/// Run a frontend action without writing process-global output streams.
pub fn run(a: &Value) -> Value {
    let config = match configured(a) {
        Ok(c) => c,
        Err(e) => return finish(false, e),
    };
    let action = s(a, "action");
    let mut args = json!({"config":config,"headers":if s(a,"header").is_empty(){vec![]}else{vec![s(a,"header")]},"path":a["path"],"private":truth(s(a,"private")),"owner":a["owner"]});
    if action == "list" {
        let url = s(a, "url");
        let kind = s(a, "kind");
        let owner = s(a, "owner").to_ascii_lowercase();
        let raw = truth(s(a, "raw"));
        let servers = if url.is_empty() {
            config["servers"].as_array().cloned().unwrap_or_default()
        } else {
            if url::Url::parse(url).is_err() {
                return finish(false, format!("Invalid URL [{url}]\n"));
            }
            let mut srv = config["servers"]
                .as_array()
                .and_then(|servers| {
                    servers.iter().find(|server| {
                        s(server, "url").trim_end_matches('/') == url.trim_end_matches('/')
                    })
                })
                .cloned()
                .unwrap_or_else(|| call("server_default", &json!({})));
            srv["url"] = json!(url);
            vec![srv]
        };
        let mut output = String::new();
        let mut success = false;
        for server in servers {
            let path = if owner.is_empty() {
                kind.to_owned()
            } else {
                format!("{owner}/{kind}")
            };
            if !raw {
                output.push_str(&format!(
                    "Fetching {} list from {}...\n",
                    kind.trim_end_matches('s'),
                    s(&server, "url")
                ));
            }
            let ids = match listing(
                &json!({"config":config,"server":server,"path":path,"kind":kind,"user_agent":config["user_agent"],"headers":args["headers"]}),
            ) {
                Ok(ids) => ids,
                Err(error) => return finish(false, format!("{error}\n")),
            };
            let mut owners = BTreeSet::new();
            let mut count = 0;
            for id in ids.as_array().unwrap_or(&vec![]) {
                if !owner.is_empty() && !s(id, "owner").eq_ignore_ascii_case(&owner) {
                    continue;
                }
                owners.insert(s(id, "owner").to_owned());
                count += 1;
                output.push_str(&format!(
                    "{}/{}/{}/{}\n",
                    s(&server, "url").trim_end_matches('/'),
                    s(id, "owner"),
                    kind,
                    s(id, "name")
                ));
            }
            if count > 0 {
                success = true;
            }
            if !raw {
                output.push_str(&format!("{} owners, {count} {kind}\n", owners.len()));
            }
        }
        return finish(success, output);
    }
    if action == "update" {
        let mut ok = true;
        for kind in ["models", "worlds"] {
            if (kind == "models" && truth(s(a, "only_worlds")))
                || (kind == "worlds" && truth(s(a, "only_models")))
            {
                continue;
            }
            args["kind"] = json!(kind);
            ok &= call("client.update", &args) == true;
        }
        return finish(ok, String::new());
    }
    if action == "upload" {
        let path = std::path::Path::new(s(a, "path"));
        if !path.is_dir() {
            return finish(
                false,
                format!("The model path[{}] doesn't exist.\n", path.display()),
            );
        }
        let mut id = call("id_default", &json!({}));
        if !s(a, "url").is_empty() {
            if url::Url::parse(s(a, "url")).is_err() {
                return finish(false, "Invalid URL\n".into());
            }
            id["server"] = config["servers"]
                .as_array()
                .and_then(|servers| {
                    servers.iter().find(|server| {
                        s(server, "url").trim_end_matches('/') == s(a, "url").trim_end_matches('/')
                    })
                })
                .cloned()
                .unwrap_or_else(|| call("server_default", &json!({})));
            id["server"]["url"] = a["url"].clone();
        }
        args["id"] = id;
        args["kind"] = json!("models");
        let paths = if path.join("metadata.pbtxt").is_file() || path.join("model.config").is_file()
        {
            vec![path.to_owned()]
        } else {
            match fs::read_dir(path) {
                Ok(it) => it
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| {
                        p.is_dir()
                            && (p.join("metadata.pbtxt").is_file()
                                || p.join("model.config").is_file())
                    })
                    .collect(),
                Err(_) => return finish(false, "Unable to read model directory\n".into()),
            }
        };
        if paths.is_empty() {
            return finish(false, "No models to upload\n".into());
        }
        let mut ok = true;
        for path in paths {
            args["path"] = json!(path.to_string_lossy());
            ok &= call("client.upload", &args)["result"] == 8;
        }
        return finish(
            ok,
            if ok {
                "Upload succeeded\n"
            } else {
                "Upload failed\n"
            }
            .into(),
        );
    }
    let url = s(a, "url");
    if url::Url::parse(url).is_err() {
        return finish(false, format!("Malformed URL [{url}]\n"));
    }
    for kind in ["models", "worlds", "collections"] {
        if let Some(parsed) = crate::client::parse_url(url, &config, kind, false) {
            args["kind"] = json!(kind);
            args["id"] = parsed["id"].clone();
            if action == "download" {
                let ok = if kind == "collections" {
                    let mut ok = true;
                    let filter = s(a, "kind");
                    for sub in ["models", "worlds"] {
                        if !filter.is_empty()
                            && filter != sub
                            && filter != sub.trim_end_matches('s')
                        {
                            continue;
                        }
                        let path = format!(
                            "{}/collections/{}/{sub}",
                            s(&parsed["id"], "owner"),
                            s(&parsed["id"], "name")
                        );
                        let ids = match listing(
                            &json!({"config":config,"server":parsed["id"]["server"],"path":path,"kind":sub,"user_agent":config["user_agent"],"headers":args["headers"]}),
                        ) {
                            Ok(ids) => ids,
                            Err(error) => return finish(false, format!("{error}\n")),
                        };
                        let mut batch = args.clone();
                        batch["kind"] = json!(sub);
                        batch["ids"] = ids;
                        batch["jobs"] = a["jobs"].clone();
                        let results = call("client.batch", &batch);
                        ok &= if sub == "worlds" {
                            results["ok"] == true
                        } else {
                            results.as_array().is_some_and(|a| {
                                a.iter().all(|v| v["result"] == 4 || v["result"] == 5)
                            })
                        };
                    }
                    ok
                } else {
                    call("client.download", &args)["result"] == 4
                };
                return finish(
                    ok,
                    if ok {
                        "Download succeeded\n"
                    } else {
                        "Download failed\n"
                    }
                    .into(),
                );
            }
            if action == "delete" && kind != "collections" {
                return finish(
                    call("client.delete", &args)["result"] == 1,
                    "Delete request completed\n".into(),
                );
            }
            if action == "edit" && kind == "models" {
                let details = call("client.details", &args);
                if details["result"] != 4 {
                    return finish(false, "Failed to fetch model details\n".into());
                }
                if s(a, "private").is_empty() {
                    args["private"] = details["id"]["private"].clone();
                }
                args["id"] = details["id"].clone();
                if s(a, "private").is_empty() && s(a, "path").is_empty() {
                    return finish(true, String::new());
                }
                return finish(
                    call("client.patch", &args)["result"] == 12,
                    "Edit request completed\n".into(),
                );
            }
        }
    }
    finish(false, format!("Invalid URL [{url}]\n"))
}

pub fn dispatch(op: &str, a: &Value) -> Option<Value> {
    Some(match op {
        "command.run" => run(a),
        "command.page" => page(a),
        "command.list" => listing(a).unwrap_or_else(|error| json!({"error":error})),
        "command.config_to_pbtxt" => match fs::read_to_string(s(a, "path"))
            .map_err(|e| e.to_string())
            .and_then(|s| crate::metadata::parse_config(&s).map_err(|error| error.to_string()))
        {
            Ok(v) => json!({"ok":true,"output":v.pbtxt()}),
            Err(e) => json!({"ok":false,"output":e}),
        },
        "command.pbtxt_to_config" => match fs::read_to_string(s(a, "path"))
            .map_err(|e| e.to_string())
            .and_then(|s| parse_pbtxt(&s))
            .and_then(|v| config_text(&v))
        {
            Ok(s) => json!({"ok":true,"output":s}),
            Err(e) => json!({"ok":false,"output":e}),
        },
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cli_honors_configured_pagination_budget_with_explicit_endpoint() {
        let server = crate::client::tests::Server::new(|_| {
            (
                200,
                vec![],
                br#"[{"owner":"alice","name":"sample"}]"#.to_vec(),
            )
        });
        let folder = tempfile::tempdir().unwrap();
        let config = folder.path().join("limits.yaml");
        fs::write(
            &config,
            format!("servers:\n - url: {}\nlimits:\n max_pages: 1\n", server.url),
        )
        .unwrap();
        let output =
            run(&json!({"action":"list","kind":"models","url":server.url,"config_file":config}));
        assert_eq!(output["ok"], false);
        assert!(s(&output, "output").contains("limit"));
        assert_eq!(server.requests.lock().unwrap().len(), 1);
    }
    #[test]
    fn metadata() {
        let v=parse_pbtxt("name: \"A\" tags: \"one\" tags: \"two\" model {file: \"model.sdf\"} legal {license: \"MIT\"} annotations {key: \"k\" value: \"v\"} dependencies {uri: \"http://fuel/x\"} categories {first: \"robots\"}").unwrap();
        assert_eq!(v["license"], "MIT");
        assert_eq!(v["tags"], json!(["one", "two"]));
        assert_eq!(v["annotations"]["k"], "v");
        assert_eq!(v["dependencies"][0]["uri"], "http://fuel/x");
        assert_eq!(v["categories"], json!(["robots"]));
    }
    #[test]
    fn parser_errors() {
        for s in [
            "foo: \"x\"",
            "name: \"x",
            "model {file: \"a\"",
            "}",
            "name:",
            "name: \"\\q\"",
            "model {} world {}",
            "@",
        ] {
            assert!(parse_pbtxt(s).is_err(), "{s}");
        }
        assert!(parse_pbtxt("# comment\nname: 'a' 'b'; version: 2").is_ok());
        assert!(parse_config("<x/>").is_err());
        assert!(parse_config("<model>").is_err());
    }
    #[test]
    fn config_legacy_leading_whitespace_and_decimal_version() {
        let v=parse_config("\n  <?xml version='1.0'?><model><name>legacy</name><version>1.0</version><sdf version='1.6'>model.sdf</sdf></model>").unwrap();
        assert_eq!(v["version"], 1);
        assert_eq!(v["model"]["file"], "model.sdf");
    }
    #[test]
    fn conversion_roundtrip() {
        let v=parse_config("<model><name>A &amp; B</name><version>2</version><sdf version='1.6'>a.sdf</sdf><sdf version='1.10'>b.sdf</sdf><author><name>N</name><email>e</email></author><description>d</description></model>").unwrap();
        assert_eq!(v["model"]["file"], "b.sdf");
        let pb = crate::metadata::parse_config("<model><name>A &amp; B</name><version>2</version><sdf version='1.10'>b.sdf</sdf><author><name>N</name><email>e</email></author><description>d</description></model>").unwrap().pbtxt();
        let parsed = parse_pbtxt(&pb).unwrap();
        assert_eq!(parsed["name"], "A & B");
        let xml = config_text(&parsed).unwrap();
        assert_eq!(parse_config(&xml).unwrap()["name"], "A & B");
        assert!(config_text(&json!({})).is_err());
        assert!(dispatch("unknown", &json!({})).is_none());
    }
    fn serve<F: Fn(&str) -> (u16, Vec<u8>, &'static str) + Send + 'static>(
        f: F,
    ) -> (
        String,
        std::sync::mpsc::Sender<()>,
        std::thread::JoinHandle<Vec<String>>,
    ) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let mut requests = vec![];
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
            while rx.try_recv().is_err() && std::time::Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut conn, _)) => {
                        conn.set_read_timeout(Some(std::time::Duration::from_secs(5)))
                            .unwrap();
                        let mut bytes = vec![];
                        loop {
                            let mut b = [0; 8192];
                            let n = conn.read(&mut b).unwrap();
                            if n == 0 {
                                break;
                            }
                            bytes.extend_from_slice(&b[..n]);
                            if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                                let header = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                                let len = header
                                    .lines()
                                    .find_map(|l| {
                                        l.strip_prefix("content-length:")
                                            .and_then(|s| s.trim().parse::<usize>().ok())
                                    })
                                    .unwrap_or(0);
                                if bytes.len() >= end + 4 + len {
                                    break;
                                }
                            }
                        }
                        let request = String::from_utf8_lossy(&bytes).to_string();
                        let (status, body, ty) = f(&request);
                        write!(conn,"HTTP/1.1 {status} OK\r\nContent-Type: {ty}\r\nX-Ign-Resource-Version: 1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).unwrap();
                        conn.write_all(&body).unwrap();
                        requests.push(request);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(2))
                    }
                    Err(e) => panic!("{e}"),
                }
            }
            requests
        });
        (url, tx, thread)
    }
    fn file_config(dir: &std::path::Path, url: &str) -> String {
        let path = dir.join("config.yaml");
        fs::write(
            &path,
            format!(
                "servers:\n  - url: {url}\n    version: 1.0\ncache:\n  path: {}\n",
                dir.join("cache").display()
            ),
        )
        .unwrap();
        path.to_string_lossy().into_owned()
    }
    #[test]
    fn local_listing_routes_owner_and_terminates_pages() {
        let (url, stop, thread) = serve(|r| {
            assert!(r.contains("User-Agent:") || r.contains("user-agent:"));
            if r.contains("page=1") {
                (
                    200,
                    br#"[{"owner":"Alice","name":"Box"},{"owner":"Bob","name":"Other"}]"#.to_vec(),
                    "application/json",
                )
            } else {
                (200, b"[]".to_vec(), "application/json")
            }
        });
        let out =
            run(&json!({"action":"list","url":url,"owner":"Alice","kind":"models","raw":"true"}));
        assert_eq!(out["ok"], true);
        let output = s(&out, "output");
        assert!(output.contains("/alice/models/box"));
        assert!(!output.contains("bob"));
        assert!(!output.contains("owners"));
        let out = run(&json!({"action":"list","url":url,"kind":"worlds"}));
        assert_eq!(out["ok"], true);
        assert!(s(&out, "output").contains("2 owners, 2 worlds"));
        stop.send(()).unwrap();
        let requests = thread.join().unwrap();
        assert_eq!(requests.len(), 4);
        assert!(requests[0].starts_with("GET /1.0/alice/models?page=1"));
    }
    #[test]
    fn link_relations_respect_quoted_delimiters_and_reject_ambiguous_metadata() {
        assert!(
            link_has_next(r#"<https://host/a,b;c>; title="a\"b, c; rel=prev"; ReL = "PrEv NeXt""#)
                .unwrap()
        );
        assert!(!link_has_next(r#"</page>; title="demo; rel=next"; rel=prev"#).unwrap());
        assert!(link_has_next(r#"</page>; foo; rel="n\ext""#).unwrap());
        for malformed in [
            "",
            "page; rel=next",
            "<>; rel=next",
            "</page>; title=next",
            "</page>; rel=next; rel=prev",
            "</page>; rel=\"\"",
            "</page>; rel",
            "</page>; rel=\"next\" tail",
            "</page>; rel=\"next",
            "</page; rel=next",
            "<<page>>; rel=next",
            ">; rel=next",
        ] {
            assert!(link_has_next(malformed).is_err(), "{malformed}");
        }
    }

    #[test]
    fn listing_and_collection_fail_on_invalid_or_incomplete_pages() {
        use crate::client::tests::Server;
        for failure in ["status", "json", "later"] {
            let server = Server::new(move |request| {
                if failure == "later" && request.contains("page=1") {
                    (
                        200,
                        vec![],
                        br#"[{"owner":"a","name":"b","version":2}]"#.to_vec(),
                    )
                } else if failure == "json" {
                    (200, vec![], b"invalid JSON".to_vec())
                } else {
                    (503, vec![], b"unavailable".to_vec())
                }
            });
            for action in ["list", "download"] {
                let url = if action == "list" {
                    server.url.clone()
                } else {
                    format!("{}/1.0/a/collections/c", server.url)
                };
                let output = run(&json!({"action":action,"url":url,"kind":"models"}));
                assert_eq!(output["ok"], false, "{failure}: {output}");
                assert!(s(&output, "output").contains("listing"));
                assert!(!s(&output, "output").contains("Download succeeded"));
            }
        }
    }

    #[test]
    fn listing_and_collection_send_explicit_headers() {
        use crate::client::tests::{Server, model_zip};
        let cache = tempfile::tempdir().unwrap();
        let server = Server::new(|request| {
            if !request
                .to_ascii_lowercase()
                .contains("x-access-token: explicit")
            {
                return (401, vec![], vec![]);
            }
            if request.contains(".zip") {
                (200, vec![], model_zip(None))
            } else if request.contains("page=1") {
                (
                    200,
                    vec![],
                    br#"[{"owner":"a","name":"b","version":2}]"#.to_vec(),
                )
            } else {
                (200, vec![], b"[]".to_vec())
            }
        });
        let config = cache.path().join("config.yaml");
        fs::write(
            &config,
            format!(
                "cache:\n  path: {}\nservers:\n  - url: {}\n",
                cache.path().display(),
                server.url
            ),
        )
        .unwrap();
        for action in ["list", "download"] {
            let url = if action == "list" {
                server.url.clone()
            } else {
                format!("{}/1.0/a/collections/c", server.url)
            };
            let output = run(
                &json!({"action":action,"url":url,"kind":"models","header":"X-Access-Token: explicit","config_file":config}),
            );
            assert_eq!(output["ok"], true, "{output}");
        }
        assert_eq!(server.requests.lock().unwrap().len(), 5);
    }

    #[test]
    fn repeated_page_guard_and_server_errors() {
        let (url, stop, thread) = serve(|_| {
            (
                200,
                br#"[{"owner":"a","name":"b"}]"#.to_vec(),
                "application/json",
            )
        });
        let ids =
            listing(&json!({"server":{"url":url,"version":"1.0"},"path":"models","kind":"models"}));
        assert!(ids.unwrap_err().contains("repeated"));
        stop.send(()).unwrap();
        assert_eq!(thread.join().unwrap().len(), 2);
        let (url, stop, thread) = serve(|_| (503, b"error".to_vec(), "text/plain"));
        assert_eq!(
            run(&json!({"action":"list","url":url,"kind":"models"}))["ok"],
            false
        );
        assert_eq!(
            run(&json!({"action":"download","url":format!("{url}/1.0/a/models/b")}))["ok"],
            false
        );
        assert_eq!(
            run(&json!({"action":"edit","url":format!("{url}/1.0/a/models/b")}))["ok"],
            false
        );
        stop.send(()).unwrap();
        assert_eq!(thread.join().unwrap().len(), 3);
    }
    #[test]
    fn commands_reject_invalid_inputs_and_conversion_errors() {
        for action in ["list", "download", "delete", "edit"] {
            assert_eq!(
                run(&json!({"action":action,"url":"fake_url","kind":"models"}))["ok"],
                false
            );
        }
        for action in ["download", "delete", "edit", "unknown"] {
            assert_eq!(
                run(&json!({"action":action,"url":"http://example.invalid/wrong"}))["ok"],
                false
            );
        }
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            run(&json!({"action":"upload","path":dir.path().join("missing")}))["ok"],
            false
        );
        assert_eq!(
            run(&json!({"action":"upload","path":dir.path()}))["ok"],
            false
        );
        assert_eq!(
            run(&json!({"action":"upload","path":dir.path(),"url":"bad"}))["ok"],
            false
        );
        assert_eq!(
            run(&json!({"action":"list","config_file":dir.path().join("missing")}))["ok"],
            false
        );
        for op in ["command.config_to_pbtxt", "command.pbtxt_to_config"] {
            assert_eq!(
                dispatch(op, &json!({"path":dir.path().join("missing")})).unwrap()["ok"],
                false
            );
        }
        fs::write(
            dir.path().join("model.config"),
            "<model><name>A</name><sdf version='1.6'>model.sdf</sdf></model>",
        )
        .unwrap();
        let result = dispatch(
            "command.config_to_pbtxt",
            &json!({"path":dir.path().join("model.config")}),
        )
        .unwrap();
        assert_eq!(result["ok"], true);
        fs::write(dir.path().join("metadata.pbtxt"), s(&result, "output")).unwrap();
        assert_eq!(
            dispatch(
                "command.pbtxt_to_config",
                &json!({"path":dir.path().join("metadata.pbtxt")})
            )
            .unwrap()["ok"],
            true
        );
        assert_eq!(
            parse_pbtxt("name: \"a\\n\\r\\t\\\\\\\"\\'b\"").unwrap()["name"],
            "a\n\r\t\\\"'b"
        );
        assert!(parse_pbtxt("name: \"a\\").is_err());
        assert!(parse_pbtxt(":").is_err());
        assert!(
            parse_config("<world><name>W</name><sdf version='1.6'>world.sdf</sdf></world>").is_ok()
        );
    }
    #[test]
    fn upload_delete_edit_and_configured_update() {
        let (url, stop, thread) = serve(|r| {
            if r.starts_with("GET ") {
                (
                    200,
                    br#"{"owner":"a","name":"b","private":true,"version":1}"#.to_vec(),
                    "application/json",
                )
            } else {
                (200, b"{}".to_vec(), "application/json")
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let config = file_config(dir.path(), &url);
        let model = dir.path().join("model");
        fs::create_dir(&model).unwrap();
        fs::write(
            model.join("metadata.pbtxt"),
            "name: \"B\" model {file: \"model.sdf\"}",
        )
        .unwrap();
        fs::write(
            model.join("model.sdf"),
            "<sdf version='1.9'><model name='test'/></sdf>",
        )
        .unwrap();
        assert_eq!(
            run(
                &json!({"action":"upload","url":url,"path":model,"header":"Private-Token: secret","owner":"owner","private":"true"})
            )["ok"],
            true
        );
        assert_eq!(
            run(&json!({"action":"upload","url":url,"path":dir.path()}))["ok"],
            true
        );
        let resource = format!("{url}/1.0/a/models/b");
        assert_eq!(run(&json!({"action":"delete","url":resource}))["ok"], true);
        assert_eq!(run(&json!({"action":"edit","url":resource}))["ok"], true);
        assert_eq!(
            run(&json!({"action":"edit","url":resource,"private":"0"}))["ok"],
            true
        );
        assert_eq!(
            run(&json!({"action":"edit","url":resource,"path":model}))["ok"],
            true
        );
        assert_eq!(
            run(&json!({"action":"update","config_file":config}))["ok"],
            true
        );
        assert_eq!(
            run(&json!({"action":"update","config_file":config,"only_models":"1"}))["ok"],
            true
        );
        assert_eq!(
            run(&json!({"action":"update","config_file":config,"only_worlds":"1"}))["ok"],
            true
        );
        assert_eq!(
            run(&json!({"action":"list","config_file":config,"owner":"a","kind":"models"}))["ok"],
            false
        );
        stop.send(()).unwrap();
        let reqs = thread.join().unwrap();
        assert!(
            reqs.iter()
                .any(|r| r.to_lowercase().contains("private-token: secret"))
        );
        assert!(reqs.iter().any(|r| r.starts_with("PATCH ")));
    }
    #[test]
    fn download_archive_and_collection_filters() {
        use std::io::Write;
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        writer
            .start_file("model.sdf", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(b"<sdf version='1.9'><model name='test'/></sdf>")
            .unwrap();
        writer
            .start_file("metadata.pbtxt", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(b"name: \"B\" model {file: \"model.sdf\"}")
            .unwrap();
        let zip = writer.finish().unwrap().into_inner();
        let (url, stop, thread) = serve(move |r| {
            if r.contains(".zip") {
                (200, zip.clone(), "application/zip")
            } else if r.contains("page=1") {
                (
                    200,
                    br#"[{"owner":"a","name":"b","version":1}]"#.to_vec(),
                    "application/json",
                )
            } else {
                (200, b"[]".to_vec(), "application/json")
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let config = file_config(dir.path(), &url);
        for kind in ["models", "worlds"] {
            let out = run(
                &json!({"action":"download","url":format!("{url}/1.0/a/{kind}/b"),"config_file":config}),
            );
            assert_eq!(out["ok"], true, "{kind}: {out}");
        }
        for kind in ["models", "worlds", ""] {
            assert_eq!(
                run(
                    &json!({"action":"download","url":format!("{url}/1.0/a/collections/c"),"config_file":config,"kind":kind,"jobs":2})
                )["ok"],
                true
            );
        }
        stop.send(()).unwrap();
        assert!(thread.join().unwrap().len() > 6);
    }
    #[test]
    fn protobuf_text_escapes_comments_and_depth_are_checked() {
        for (input, expected) in [
            (r#"name: "\303\251""#, "é"),
            (r#"name: "\xC3\xA9""#, "é"),
            (r#"name: "\u00e9\U0001F600""#, "é😀"),
            (r#"name: "\101\1\12\x4\x41""#, "A\u{1}\n\u{4}A"),
            (r#"name: "\a\b\f\v\?""#, "\u{7}\u{8}\u{c}\u{b}?"),
            ("# comment\nname: 'literal é' # trailing\n", "literal é"),
        ] {
            assert_eq!(parse_pbtxt(input).unwrap()["name"], expected, "{input}");
        }
        for input in [
            r#"name: "\777""#,
            r#"name: "\x""#,
            r#"name: "\xg""#,
            r#"name: "\u12""#,
            r#"name: "\uD800""#,
            r#"name: "\UFFFFFFFF""#,
            r#"name: "\xff""#,
            "/* unterminated",
            "/",
        ] {
            assert!(parse_pbtxt(input).is_err(), "{input}");
        }
        let nested = format!("{}{}", "nested {".repeat(65), "}".repeat(65));
        assert!(parse_pbtxt(&nested).is_err());
    }
}
