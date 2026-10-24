//! Compatibility value semantics, configuration and Fuel JSON codecs.
use serde_json::{Value, json};
use std::{env, fs, path::Path};

pub fn server_default() -> Value {
    json!({"url":"https://fuel.gazebosim.org","version":"1.0","api_key":""})
}
pub fn id_default() -> Value {
    json!({"name":"","owner":"","server":server_default(),"version":0,"description":"","file_size":0,"upload_date":0,"modify_date":0,"like_count":0,"download_count":0,"license_name":"","license_url":"","license_image_url":"","tags":[],"private":false,"local_path":""})
}
pub fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
fn sanitize(v: &str) -> String {
    v.replace(':', "%3A").replace('@', "%40")
}
fn home_cache() -> String {
    Path::new(&env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap_or_default())
        .join(".gz/fuel")
        .to_string_lossy()
        .into_owned()
}
fn config_default() -> Value {
    let mut c = json!({"servers":[server_default()],"cache_location":home_cache(),"config_path":"","user_agent":"GazeboFuelTools-12.0.0~pre2"});
    if let Ok(p) = env::var("GZ_FUEL_CACHE_PATH") {
        if !p.is_empty() && Path::new(&p).is_dir() {
            c["cache_location"] = json!(p);
        }
        for name in ["config.yaml", "config.yml"] {
            let file = Path::new(s(&c, "cache_location")).join(name);
            if file.exists() {
                return config_load(&c, &file.to_string_lossy())["config"].clone();
            }
        }
    }
    c
}
fn join(parts: &[&str]) -> String {
    let mut out = String::new();
    for p in parts {
        if p.is_empty() {
            continue;
        }
        if !out.is_empty() && !out.ends_with('/') {
            out.push('/');
        }
        out.push_str(p.trim_start_matches('/'));
    }
    out
}
fn uri_path(v: &Value) -> String {
    let p = s(v, "path").strip_prefix('/').unwrap_or(s(v, "path"));
    let a = s(v, "authority").trim_start_matches("//");
    if a.is_empty() {
        p.into()
    } else {
        join(&[&sanitize(a), p])
    }
}
fn unique(id: &Value, kind: &str) -> String {
    let url = s(&id["server"], "url");
    let base = url.split_once("://").map_or(url, |(_, v)| v);
    let base = if id["server"]["has_authority"].as_bool().unwrap_or(true) {
        let (a, p) = base.split_once('/').unwrap_or((base, ""));
        join(&[&sanitize(a), p])
    } else {
        base.into()
    };
    let mut v = join(&[&base, s(id, "owner"), kind, s(id, "name")]);
    if s(id, "name").is_empty() && !v.ends_with('/') {
        v.push('/');
    }
    v
}
fn version(v: &Value) -> String {
    let n = v["version"].as_u64().unwrap_or(0);
    if n == 0 { "tip".into() } else { n.to_string() }
}
fn line(prefix: &str, key: &str, value: &str, pretty: bool) -> String {
    if pretty {
        format!("{prefix}\x1b[96m\x1b[1m{key}: \x1b[0m\x1b[37m{value}\x1b[0m\n")
    } else {
        format!("{prefix}{key}: {value}\n")
    }
}
fn heading(prefix: &str, key: &str, pretty: bool) -> String {
    if pretty {
        format!("{prefix}\x1b[96m\x1b[1m{key}:\x1b[0m\n")
    } else {
        format!("{prefix}{key}:\n")
    }
}
fn server_format(v: &Value, p: &str, pretty: bool) -> String {
    let mut out = String::new();
    for (k, label) in [
        ("url", "URL"),
        ("version", "Version"),
        ("api_key", "API key"),
    ] {
        let x = s(v, k);
        if !pretty || !x.is_empty() {
            out.push_str(&line(p, label, x, pretty));
        }
    }
    out
}
fn id_format(id: &Value, kind: &str, p: &str, pretty: bool) -> String {
    let mut out = String::new();
    for (k, label) in [("name", "Name"), ("owner", "Owner")] {
        if !pretty || !s(id, k).is_empty() {
            out += &line(p, label, s(id, k), pretty);
        }
    }
    if kind != "collections" && (!pretty || id["version"] != 0) {
        out += &line(p, "Version", &version(id), pretty);
    }
    if !pretty {
        out += &line(p, "Unique name", &unique(id, kind), false);
    }
    if kind == "models" {
        for (k, label) in [
            ("description", "Description"),
            ("file_size", "File size"),
            ("upload_date", "Upload date"),
            ("like_count", "Likes"),
            ("download_count", "Downloads"),
            ("license_name", "License name"),
            ("license_url", "License URL"),
            ("license_image_url", "License image URL"),
        ] {
            let x = if let Some(t) = id[k].as_str() {
                t.into()
            } else {
                id[k].as_i64().unwrap_or(0).to_string()
            };
            if !pretty || (!x.is_empty() && x != "0") {
                out += &line(p, label, &x, pretty);
            }
        }
        let tags = id["tags"].as_array().cloned().unwrap_or_default();
        if !pretty || !tags.is_empty() {
            if pretty {
                out += &format!("{p}\x1b[96m\x1b[1mTags: \x1b[0m\n");
            } else {
                out += &format!("{p}Tags: \n");
            }
            for tag in tags {
                let t = tag.as_str().unwrap_or("");
                if pretty {
                    out += &format!("{p}\x1b[96m\x1b[1m- \x1b[0m\x1b[37m{t}\x1b[0m\n");
                } else {
                    out += &format!("{p}- {t}\n");
                }
            }
        }
    } else if kind == "worlds" && !pretty {
        out += &line(p, "Local path", s(id, "local_path"), false);
    }
    out += &heading(p, "Server", pretty);
    out += &server_format(&id["server"], &format!("{p}  "), pretty);
    out
}
fn config_load(c: &Value, path: &str) -> Value {
    let Ok(content) = fs::read_to_string(path) else {
        return json!({"ok":false,"config":c});
    };
    let mut c = c.clone();
    c["config_path"] = json!(path);
    let Ok(doc) = serde_saphyr::from_str::<Value>(&content) else {
        return json!({"ok":false,"config":c});
    };
    let mut ok = true;
    if let Some(list) = doc["servers"].as_array() {
        for srv in list {
            let url = s(srv, "url");
            if url.is_empty() {
                ok = false;
                continue;
            }
            let token = s(srv, "private-token");
            let servers = c["servers"]
                .as_array_mut()
                .expect("configuration has servers");
            if let Some(old) = servers.iter_mut().find(|v| s(v, "url") == url) {
                if !token.is_empty() {
                    old["api_key"] = json!(token);
                }
            } else {
                let mut new = server_default();
                new["url"] = json!(url);
                new["has_authority"] = json!(false);
                new["api_key"] = json!(token);
                servers.push(new);
            }
        }
    }
    let configured = s(&doc["cache"], "path");
    if doc.get("cache").is_some() && configured.is_empty() {
        ok = false;
    }
    c["cache_location"] =
        json!(
            env::var("GZ_FUEL_CACHE_PATH").unwrap_or_else(|_| if configured.is_empty() {
                home_cache()
            } else {
                configured.into()
            })
        );
    json!({"ok":ok,"config":c})
}
fn parse_json(text: &str) -> Value {
    serde_json::from_str(text)
        .or_else(|_| {
            let t = text.trim();
            if let Some(v) = t.strip_suffix("]}") {
                serde_json::from_str(&format!("{v}}}"))
            } else {
                Err(serde_json::from_str::<Value>("").unwrap_err())
            }
        })
        .unwrap_or(Value::Null)
}
fn date(t: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(t)
        .map(|d| d.timestamp())
        .unwrap_or(0)
}
fn parse_id(v: &Value, server: &Value, kind: &str) -> Option<Value> {
    v.as_object()?;
    let mut id = id_default();
    id["server"] = server.clone();
    for k in ["name", "owner"] {
        if let Some(x) = v[k].as_str() {
            id[k] = json!(x.to_ascii_lowercase());
        }
    }
    if let Some(n) = v["version"].as_u64() {
        id["version"] = json!(n);
    }
    if kind == "models" {
        for (from, to) in [
            ("description", "description"),
            ("license_name", "license_name"),
            ("license_url", "license_url"),
            ("license_image", "license_image_url"),
        ] {
            if let Some(x) = v[from].as_str() {
                id[to] = json!(x);
            }
        }
        for (from, to) in [
            ("filesize", "file_size"),
            ("likes", "like_count"),
            ("downloads", "download_count"),
        ] {
            if let Some(n) = v[from].as_u64() {
                id[to] = json!(n);
            }
        }
        for (from, to) in [("createdAt", "upload_date"), ("updatedAt", "modify_date")] {
            if let Some(t) = v[from].as_str() {
                id[to] = json!(date(t));
            }
        }
        if let Some(tags) = v["tags"].as_array()
            && tags.iter().all(Value::is_string)
        {
            id["tags"] = json!(tags);
        }
    }
    Some(id)
}
pub fn dispatch(op: &str, a: &Value) -> Option<Value> {
    Some(match op {
        "server_default" => server_default(),
        "server_clear" => json!({"url":"","version":"1.0","api_key":""}),
        "server_format" => json!(server_format(
            &a["server"],
            s(a, "prefix"),
            a["pretty"].as_bool().unwrap_or(false)
        )),
        "id_default" => id_default(),
        "id_set" => {
            let mut id = a["id"].clone();
            let field = s(a, "field");
            let mut value = a["value"].clone();
            let mut ok = true;
            if ["name", "owner"].contains(&field) && s(a, "kind") != "collections" {
                value = json!(value.as_str().unwrap_or("").to_ascii_lowercase());
            }
            if field == "version_str" {
                let t = value.as_str().unwrap_or("");
                let n = if t.is_empty() || t == "tip" {
                    Some(0)
                } else {
                    t.parse::<u32>().ok()
                };
                if let Some(n) = n {
                    id["version"] = json!(n);
                } else {
                    ok = false;
                }
            } else {
                if ["license_url", "license_image_url"].contains(&field) {
                    ok = value
                        .as_str()
                        .is_some_and(|u| u.contains(':') && !u.contains(' '));
                }
                if ok {
                    id[field] = value;
                }
            }
            json!({"ok":ok,"id":id})
        }
        "id_unique" => json!(unique(&a["id"], s(a, "kind"))),
        "id_url" => json!(join(&[
            s(&a["id"]["server"], "url"),
            s(&a["id"], "owner"),
            s(a, "kind"),
            s(&a["id"], "name")
        ])),
        "id_format" => json!(id_format(
            &a["id"],
            s(a, "kind"),
            s(a, "prefix"),
            a["pretty"].as_bool().unwrap_or(false)
        )),
        "version" => json!(version(&a["id"])),
        "config_default" => config_default(),
        "config_clear" => {
            json!({"servers":[],"cache_location":"","config_path":"","user_agent":"GazeboFuelTools-12.0.0~pre2"})
        }
        "config_load" => config_load(&a["config"], s(a, "path")),
        "config_format" => {
            let c = &a["config"];
            let p = s(a, "prefix");
            let mut out = line(p, "Config path", s(c, "config_path"), false)
                + &line(p, "Cache location", s(c, "cache_location"), false)
                + &heading(p, "Servers", false);
            if let Some(servers) = c["servers"].as_array() {
                for srv in servers {
                    out += &format!("{p}  ---\n{p}{}", server_format(srv, "  ", false));
                }
            }
            json!(out)
        }
        "sanitize_authority" => json!(sanitize(s(a, "text"))),
        "uri_path" => json!(uri_path(a)),
        "result" => {
            let n = a["type"].as_u64().unwrap_or(0) as usize;
            let messages = [
                "Unknown result",
                "Successfully deleted",
                "Could not delete, model not found",
                "Delete failed. Other errors",
                "Successfully fetched from server",
                "Already in cache, did not fetch from server",
                "Model not found",
                "Fetch failed. Other errors",
                "Successfully uploaded to server",
                "Model already exists",
                "Upload failed. Other errors",
                "Patch failed.",
                "Successfully sent patch request to the server",
            ];
            json!({"ok":([1,4,5,8,12].contains(&n)),"message":messages.get(n).unwrap_or(&messages[0])})
        }
        "json_parse" => {
            let v = parse_json(s(a, "text"));
            if a["many"].as_bool().unwrap_or(false) {
                let ids: Vec<_> = v
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map_while(|v| parse_id(v, &a["server"], s(a, "kind")))
                    .collect();
                json!(ids)
            } else {
                parse_id(&v, &a["server"], s(a, "kind")).unwrap_or_else(|| {
                    let mut id = id_default();
                    id["server"] = a["server"].clone();
                    id
                })
            }
        }
        "json_build" => {
            let id = &a["id"];
            let mut pairs = Vec::new();
            if s(a, "kind") == "models" {
                pairs.push(("description", id["description"].clone()));
            }
            pairs.push(("name", id["name"].clone()));
            pairs.push(("version", id["version"].clone()));
            let fields: Vec<_> = pairs
                .into_iter()
                .map(|(k, v)| format!("\t\"{k}\" : {v}"))
                .collect();
            json!(format!("{{\n{}\n}}", fields.join(",\n")))
        }
        "json_licenses" => {
            let v = parse_json(s(a, "text"));
            let mut licenses = serde_json::Map::new();
            if let Some(list) = v.as_array() {
                for l in list {
                    if l.is_object() {
                        let n = s(l, "name");
                        if let Some(id) = l["ID"].as_u64() {
                            licenses.entry(n).or_insert(json!(id));
                        }
                    }
                }
            }
            json!({"ok":v.is_array(),"licenses":licenses})
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(op: &str, a: Value) -> Value {
        dispatch(op, &a).unwrap()
    }
    #[test]
    fn identifier_semantics_and_version_errors() {
        let id = id_default();
        assert_eq!(id["server"]["url"], "https://fuel.gazebosim.org");
        for kind in ["models", "worlds", "collections"] {
            let mut id = id.clone();
            for (field, value) in [
                ("name", json!("My Robot")),
                ("owner", json!("OpenRobotics")),
                ("version", json!(4)),
                ("description", json!("description")),
                ("file_size", json!(128)),
                ("upload_date", json!(12)),
                ("like_count", json!(1)),
                ("download_count", json!(2)),
                ("license_name", json!("CC0")),
                ("license_url", json!("https://example.com")),
                ("license_image_url", json!("https://example.com/image")),
                ("tags", json!(["one", "two"])),
                ("local_path", json!("/cache")),
            ] {
                let r = run(
                    "id_set",
                    json!({"id":id,"kind":kind,"field":field,"value":value}),
                );
                assert_eq!(r["ok"], true);
                id = r["id"].clone();
            }
            assert_eq!(
                id["name"],
                if kind == "collections" {
                    "My Robot"
                } else {
                    "my robot"
                }
            );
            let url = run("id_url", json!({"id":id,"kind":kind}));
            assert!(url.as_str().unwrap().contains(kind));
            let unique = run("id_unique", json!({"id":id,"kind":kind}));
            assert!(unique.as_str().unwrap().contains(kind));
            for pretty in [false, true] {
                for sample in [&id, &id_default()] {
                    let out = run(
                        "id_format",
                        json!({"id":sample,"kind":kind,"prefix":"! ","pretty":pretty}),
                    );
                    assert!(out.as_str().unwrap().contains("Server:"));
                }
            }
            for (text, ok, n) in [
                ("", true, 0),
                ("tip", true, 0),
                ("123", true, 123),
                ("not-number", false, 123),
                ("4294967296", false, 123),
            ] {
                let r = run(
                    "id_set",
                    json!({"id":id,"field":"version_str","value":text}),
                );
                assert_eq!(r["ok"], ok);
                if ok {
                    id = r["id"].clone();
                    assert_eq!(id["version"], n);
                }
            }
            assert_eq!(run("version", json!({"id":id})), "123");
            assert_eq!(run("version", json!({"id":id_default()})), "tip");
        }
        for field in ["license_url", "license_image_url"] {
            assert_eq!(
                run("id_set", json!({"id":id,"field":field,"value":"bad url"}))["ok"],
                false
            );
        }
        let mut id = id_default();
        id["server"] = json!({"url":"http://localhost:8000/path","has_authority":true});
        id["name"] = json!("box");
        assert_eq!(
            run("id_unique", json!({"id":id,"kind":"models"})),
            "localhost%3A8000/path/models/box"
        );
        id["server"]["has_authority"] = json!(false);
        assert_eq!(
            run("id_unique", json!({"id":id,"kind":"models"})),
            "localhost:8000/path/models/box"
        );
        assert!(dispatch("unknown", &Value::Null).is_none());
    }
    #[test]
    fn helpers_formats_and_results() {
        assert_eq!(
            run("sanitize_authority", json!({"text":"user@localhost:8000"})),
            "user%40localhost%3A8000"
        );
        for (a, p, want) in [
            (
                "//localhost:8000",
                "/some/path/",
                "localhost%3A8000/some/path/",
            ),
            ("", "/some/path", "some/path"),
            ("//host", "", "host"),
            ("", "", ""),
        ] {
            assert_eq!(run("uri_path", json!({"authority":a,"path":p})), want);
        }
        let d = run("server_default", Value::Null);
        assert_eq!(d, server_default());
        let c = run("server_clear", Value::Null);
        assert_eq!(c["url"], "");
        for server in [d, c, json!({"url":"x://y","version":"2","api_key":"token"})] {
            for pretty in [false, true] {
                let f = run(
                    "server_format",
                    json!({"server":server,"pretty":pretty,"prefix":"# "}),
                );
                assert!(f.as_str().unwrap().contains("Version"));
            }
        }
        for n in 0..=13 {
            let r = run("result", json!({"type":n}));
            assert!(!r["message"].as_str().unwrap().is_empty());
            assert_eq!(r["ok"], json!([1, 4, 5, 8, 12].contains(&n)));
        }
        let c = run("config_default", Value::Null);
        assert!(!s(&c, "cache_location").is_empty());
        let f = run("config_format", json!({"config":c,"prefix":"# "}));
        assert!(f.as_str().unwrap().contains("Servers:"));
        assert_eq!(run("config_clear", Value::Null)["servers"], json!([]));
    }
    #[test]
    fn yaml_config_validation_and_duplicate_tokens() {
        let temp = tempfile::tempdir().unwrap();
        let p = temp.path().join("fuel.yaml");
        let path = p.to_str().unwrap();
        let c = config_default();
        assert_eq!(
            run("config_load", json!({"config":c,"path":path}))["ok"],
            false
        );
        for (text, ok, count) in [
            ("servers: [", false, 1),
            (
                "servers:\n - url: https://example.com\n   private-token: first\n - url: https://example.com\n   private-token: second\ncache:\n path: /tmp/fuel\n",
                true,
                2,
            ),
            ("servers:\n - other: value\ncache: {}", false, 1),
            ("servers:\n - url: ''\ncache:\n path: ''", false, 1),
            ("servers: []", true, 1),
        ] {
            fs::write(&p, text).unwrap();
            let r = run("config_load", json!({"config":c,"path":path}));
            assert_eq!(r["ok"], ok, "{text}");
            assert_eq!(r["config"]["servers"].as_array().unwrap().len(), count);
            assert_eq!(r["config"]["config_path"], path);
            if count == 2 {
                assert_eq!(r["config"]["servers"][1]["api_key"], "second");
            }
        }
        fs::write(
            &p,
            "servers:\n - url: https://fuel.gazebosim.org\n - url: https://example.com\n",
        )
        .unwrap();
        let r = run("config_load", json!({"config":c,"path":path}));
        assert_eq!(r["config"]["servers"].as_array().unwrap().len(), 2);
    }
    #[test]
    fn fuel_json_roundtrip_dates_malformed_and_licenses() {
        let server = json!({"url":"banana://testServer","version":"1.0","api_key":""});
        let sample = json!({"name":"Car","owner":"OSRF","version":3,"description":"car description","filesize":64,"likes":2,"downloads":8,"license_name":"CC0","license_url":"https://example.com","license_image":"https://example.com/image","tags":["a","b"],"createdAt":"2012-04-21T19:25:44.511Z","updatedAt":"2012-04-23T18:25:43.511Z"});
        for kind in ["models", "worlds"] {
            let id = run(
                "json_parse",
                json!({"text":sample.to_string(),"server":server,"kind":kind,"many":false}),
            );
            assert_eq!(id["name"], "car");
            assert_eq!(id["server"], server);
            assert_eq!(id["version"], 3);
            if kind == "models" {
                assert_eq!(id["upload_date"], 1335036344_i64);
                assert_eq!(id["modify_date"], 1335205543_i64);
                assert_eq!(id["tags"], json!(["a", "b"]));
            }
            let built = run("json_build", json!({"id":id,"kind":kind}));
            let decoded: Value = serde_json::from_str(built.as_str().unwrap()).unwrap();
            assert_eq!(decoded["name"], "car");
            for text in [
                "not json",
                "{}",
                "null",
                "[{}]",
                "[{},null,{}]",
                "{\"name\":\"car\",\"version\":1]}",
            ] {
                for many in [false, true] {
                    let r = run(
                        "json_parse",
                        json!({"text":text,"server":server,"kind":kind,"many":many}),
                    );
                    if many {
                        assert!(r.is_array());
                    } else {
                        assert!(r.is_object());
                    }
                }
            }
        }
        assert_eq!(date("bad date"), 0);
        let bad = json!({"tags":[1],"name":1,"version":"bad"});
        let r = parse_id(&bad, &server, "models").unwrap();
        assert_eq!(r["tags"], json!([]));
        let text = r#"[{"name":"CC0","ID":1},{"name":"CC0","ID":2},null,{"name":"broken"}]"#;
        let r = run("json_licenses", json!({"text":text}));
        assert_eq!(r["ok"], true);
        assert_eq!(r["licenses"]["CC0"], 1);
        assert_eq!(run("json_licenses", json!({"text":"{}"}))["ok"], false);
        assert_eq!(run("id_default", Value::Null), id_default());
    }
}
