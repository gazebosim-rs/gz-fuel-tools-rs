// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
//! Shared model/world client operations. All storage and HTTP use the same IO layer.
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    fs,
    path::{Component, Path},
};

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
fn core(op: &str, a: Value) -> Value {
    crate::core::dispatch(op, &a).unwrap_or(Value::Null)
}
fn io(op: &str, a: Value) -> Value {
    crate::io::dispatch(op, &a).unwrap_or(Value::Null)
}
fn result(code: i32) -> Value {
    json!({"result":code})
}
fn resource_key(id: &Value) -> (String, String, String, u64) {
    (
        s(&id["server"], "url").trim_end_matches('/').to_owned(),
        s(id, "owner").to_owned(),
        s(id, "name").to_owned(),
        id["version"].as_u64().unwrap_or(0),
    )
}
fn valid_component(value: &str) -> bool {
    !value.is_empty() && value != "." && value != ".." && !value.contains(['/', '\\', '\0'])
}

fn api_version(value: &str) -> bool {
    value.split_once('.').is_some_and(|(x, y)| {
        !x.is_empty() && !y.is_empty() && x.bytes().chain(y.bytes()).all(|b| b.is_ascii_digit())
    })
}

/// Parse a Fuel URL while retaining the resource's spaces and existing escapes.
/// Configured server versions and credentials take precedence, as in the C++ API.
pub fn parse_url(url: &str, config: &Value, kind: &str, file: bool) -> Option<Value> {
    let (scheme, tail) = url.split_once("://")?;
    if scheme.is_empty()
        || !scheme
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".+-".contains(&b))
    {
        return None;
    }
    let (authority, path) = tail.split_once('/')?;
    if authority.is_empty()
        || authority.chars().any(char::is_whitespace)
        || url.contains(['?', '#', '\0', '\\'])
    {
        return None;
    }
    let mut parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    let api = if parts.first().is_some_and(|x| api_version(x)) {
        parts.remove(0)
    } else {
        ""
    };
    if parts.len() < 3 || parts[1] != kind {
        return None;
    }
    let owner = percent_encoding::percent_decode_str(parts[0])
        .decode_utf8()
        .ok()?;
    let name = percent_encoding::percent_decode_str(parts[2])
        .decode_utf8()
        .ok()?;
    if !valid_component(&owner) || !valid_component(&name) {
        return None;
    }
    let mut id = core("id_default", json!({"kind":kind}));
    id["owner"] = json!(if kind == "collections" {
        owner.into_owned()
    } else {
        owner.to_ascii_lowercase()
    });
    id["name"] = json!(if kind == "collections" {
        name.into_owned()
    } else {
        name.to_ascii_lowercase()
    });
    let mut server = json!({"url":format!("{scheme}://{authority}"),"version":api,"api_key":"","has_authority":true});
    if let Some(servers) = config["servers"].as_array() {
        for srv in servers {
            if s(srv, "url").trim_end_matches('/') == s(&server, "url") {
                server = srv.clone();
                break;
            }
        }
    }
    id["server"] = server;
    if kind == "collections" {
        return (parts.len() == 3 && !file).then(|| json!({"id":id,"file":""}));
    }
    let ver = parts.get(3).copied().unwrap_or("");
    let version = if ver.is_empty() || ver == "tip" {
        0
    } else {
        ver.parse::<u32>().ok()?
    };
    id["version"] = json!(version);
    let file_path = if file {
        if parts.len() < 6 || parts[4] != "files" {
            return None;
        }
        let path = parts[5..].join("/");
        // Reject both literal and escaped parent components before joining a cache path.
        let decoded = percent_encoding::percent_decode_str(&path)
            .decode_utf8()
            .ok()?;
        if decoded.split(['/', '\\']).any(|c| c == ".." || c == ".") {
            return None;
        }
        path
    } else {
        if parts.len() > 4 {
            return None;
        }
        String::new()
    };
    Some(json!({"id":id,"file":file_path}))
}
fn headers(a: &Value, id: &Value) -> Value {
    let mut out =
        if a["header_origin"].is_null() || a["header_origin"] == origin(s(&id["server"], "url")) {
            a["headers"].as_array().cloned().unwrap_or_default()
        } else {
            vec![]
        };
    if !out.iter().any(|h| {
        h.as_str()
            .and_then(|x| x.split_once(':'))
            .is_some_and(|(k, _)| k.eq_ignore_ascii_case("private-token"))
    }) && !s(&id["server"], "api_key").is_empty()
    {
        out.push(json!(format!(
            "Private-token: {}",
            s(&id["server"], "api_key")
        )));
    }
    json!(out)
}
fn origin(value: &str) -> Value {
    url::Url::parse(value)
        .ok()
        .map(|url| json!([url.scheme(), url.host_str(), url.port_or_known_default()]))
        .unwrap_or_else(|| json!(value))
}
fn request(
    a: &Value,
    id: &Value,
    method: i32,
    path: &str,
    query: Value,
    form: Value,
) -> crate::io::Response {
    const SEGMENT: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'~');
    let encoded_path = path
        .split('/')
        .map(|part| percent_encoding::utf8_percent_encode(part, SEGMENT).to_string())
        .collect::<Vec<_>>()
        .join("/");
    crate::io::transport(
        &json!({"method":method,"url":id["server"]["url"],"version":id["server"]["version"],"path":encoded_path,"path_encoded":true,"query":query,"headers":headers(a,id),"data":[],"form":form,"user_agent":a["config"]["user_agent"]}),
    )
}
fn body(resp: &crate::io::Response) -> String {
    String::from_utf8(resp.data.clone()).unwrap_or_default()
}
fn header<'a>(resp: &'a crate::io::Response, key: &str) -> Option<&'a str> {
    resp.headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.as_str())
}
fn version(id: &Value) -> String {
    match id["version"].as_u64().unwrap_or(0) {
        0 => "tip".into(),
        n => n.to_string(),
    }
}
fn route(id: &Value, kind: &str) -> String {
    format!("{}/{}/{}", s(id, "owner"), kind, s(id, "name"))
}
fn configured(id: &Value) -> bool {
    valid_component(s(id, "name"))
        && valid_component(s(id, "owner"))
        && !s(&id["server"], "url").is_empty()
        && !s(&id["server"], "version").is_empty()
}
fn cached(a: &Value, id: &Value, kind: &str) -> Option<Value> {
    io(
        "io.cache_list",
        json!({"config":a["config"],"kind":kind,"id":id,"exact":true}),
    )
    .as_array()?
    .first()
    .cloned()
}
fn details(a: &Value) -> Value {
    let id = &a["id"];
    let kind = s(a, "kind");
    if !configured(id) {
        return result(7);
    }
    let resp = request(a, id, 0, &route(id, kind), json!([]), json!([]));
    if resp.status != 200 {
        return result(7);
    }
    let value = core(
        "json_parse",
        json!({"text":body(&resp),"kind":kind,"server":id["server"],"many":false}),
    );
    if s(&value, "name").is_empty() {
        return result(7);
    }
    json!({"result":4,"id":value})
}
fn zip_bytes(a: &Value, mut resp: crate::io::Response) -> Option<Vec<u8>> {
    let mut seen = HashSet::new();
    for _ in 0..8 {
        if resp.status != 200 {
            return None;
        }
        match header(&resp, "content-type")
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
        {
            "text/plain" => {
                let url = body(&resp).trim().to_owned();
                if !url.starts_with("http://") && !url.starts_with("https://")
                    || !seen.insert(url.clone())
                {
                    return None;
                }
                resp = crate::io::transport(
                    &json!({"method":0,"url":url,"version":"","path":"","query":[],"headers":[],"data":[],"form":[],"user_agent":a["config"]["user_agent"]}),
                );
            }
            "" | "application/zip" | "binary/octet-stream" | "application/octet-stream" => {
                return Some(resp.data);
            }
            _ => return None,
        }
    }
    None
}
fn download_one(a: &Value) -> Value {
    let id = &a["id"];
    let kind = s(a, "kind");
    if !configured(id) {
        return result(7);
    }
    let path = format!("{}/{}/{}.zip", route(id, kind), version(id), s(id, "name"));
    let resp = request(a, id, 0, &path, json!(["link=true"]), json!([]));
    if resp.status != 200 {
        return result(7);
    }
    let v = header(&resp, "X-Ign-Resource-Version")
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|n| *n > 0)
        .unwrap_or_else(|| id["version"].as_u64().filter(|n| *n > 0).unwrap_or(1) as u32);
    let Some(bytes) = zip_bytes(a, resp) else {
        return result(7);
    };
    let mut new_id = id.clone();
    new_id["version"] = json!(v);
    let saved = crate::io::cache_save_bytes(
        &json!({"config":a["config"],"kind":kind,"id":new_id,"overwrite":true}),
        &bytes,
    );
    if saved["ok"] != true {
        return result(7);
    }
    json!({"result":4,"id":saved["id"],"path":saved["path"]})
}
fn metadata(path: &Path) -> Option<Value> {
    let pb = path.join("metadata.pbtxt");
    if pb.is_file() {
        crate::command::parse_pbtxt(&fs::read_to_string(pb).ok()?).ok()
    } else {
        crate::command::parse_config(&fs::read_to_string(path.join("model.config")).ok()?).ok()
    }
}
fn dependencies(a: &Value, id: &Value) -> Option<Vec<Value>> {
    let Some(entry) = cached(a, id, "models") else {
        return Some(vec![]);
    };
    let path = Path::new(s(&entry, "path"));
    if !path.join("metadata.pbtxt").exists() && !path.join("model.config").exists() {
        return Some(vec![]);
    }
    let meta = metadata(path)?;
    let mut deps = vec![];
    if let Some(items) = meta["dependencies"].as_array() {
        for item in items {
            let url = item.as_str().unwrap_or_else(|| s(item, "uri"));
            if let Some(parsed) = parse_url(url, &a["config"], "models", false) {
                deps.push(parsed["id"].clone());
            }
        }
    }
    Some(deps)
}
fn dep_closure(a: &Value, ids: Vec<Value>, recursive: bool) -> Value {
    let mut queue: VecDeque<Value> = ids.into();
    let mut seen = HashSet::new();
    let mut out = vec![];
    while let Some(id) = queue.pop_front() {
        if !seen.insert(resource_key(&id)) {
            continue;
        }
        let Some(deps) = dependencies(a, &id) else {
            return result(7);
        };
        for dep in deps {
            if !out.contains(&dep) {
                out.push(dep.clone());
                if recursive {
                    queue.push_back(dep);
                }
            }
        }
    }
    json!({"result":4,"dependencies":out})
}
fn download(a: &Value) -> Value {
    let mut scoped = a.clone();
    if scoped["header_origin"].is_null() {
        scoped["header_origin"] = origin(s(&a["id"]["server"], "url"));
    }
    let a = &scoped;
    let output = download_one(a);
    if output["result"] != 4 || s(a, "kind") != "models" {
        return output;
    }
    let Some(deps) = dependencies(a, &output["id"]) else {
        return result(7);
    };
    let mut output = output;
    output["dependencies"] = json!(deps);
    if a["recursive"].as_bool().unwrap_or(true) {
        let mut queue: VecDeque<Value> = deps.into();
        let mut seen = HashSet::from([resource_key(&a["id"]), resource_key(&output["id"])]);
        while let Some(id) = queue.pop_front() {
            if !seen.insert(resource_key(&id)) {
                continue;
            }
            if let Some(entry) = cached(a, &id, "models") {
                let Some(next) = dependencies(a, &entry["id"]) else {
                    return result(7);
                };
                queue.extend(next);
                continue;
            }
            let mut sub = a.clone();
            sub["id"] = id;
            let res = download_one(&sub);
            if res["result"] != 4 {
                return res;
            }
            let Some(next) = dependencies(&sub, &res["id"]) else {
                return result(7);
            };
            queue.extend(next);
        }
    }
    output
}
fn upload(a: &Value, patch: bool) -> Value {
    let id = &a["id"];
    let path = Path::new(s(a, "path"));
    let mut form = vec![];
    if !patch || !s(a, "path").is_empty() {
        let Some(meta) = metadata(path) else {
            return result(10);
        };
        form.push(json!(["name", meta["name"].as_str().unwrap_or("")]));
        form.push(json!([
            "description",
            meta["description"].as_str().unwrap_or("")
        ]));
        if !s(a, "owner").is_empty() {
            form.push(json!(["owner", s(a, "owner")]));
        }
        let license = s(&meta, "license");
        let license_id = if license.is_empty() {
            1
        } else {
            let resp = request(a, id, 0, "licenses", json!([]), json!([]));
            if resp.status != 200 {
                return result(10);
            }
            let licenses = core("json_licenses", json!({"text":body(&resp)}));
            let Some(n) = licenses["licenses"][license].as_u64() else {
                return result(10);
            };
            n
        };
        form.push(json!(["license", license_id.to_string()]));
        for field in ["tags", "categories"] {
            let values = meta[field]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_default();
            if !values.is_empty() {
                form.push(json!([field, values]));
            }
        }
        if let Some(annotations) = meta["annotations"].as_object() {
            for (k, v) in annotations {
                form.push(json!(["metadata", json!({"key":k,"value":v}).to_string()]));
            }
        }
        fn files(dir: &Path, root: &Path, out: &mut Vec<Value>) -> std::io::Result<()> {
            for entry in fs::read_dir(dir)? {
                let e = entry?;
                let ty = e.file_type()?;
                if ty.is_symlink() {
                    return Err(std::io::Error::other("Symlink in model upload"));
                }
                if ty.is_dir() {
                    files(&e.path(), root, out)?;
                } else if ty.is_file() {
                    out.push(json!(["file", {"file":e.path(),"filename":e.path().strip_prefix(root).unwrap()}]));
                }
            }
            Ok(())
        }
        if files(path, path, &mut form).is_err() {
            return result(10);
        }
    }
    form.push(json!([
        "private",
        if a["private"].as_bool().unwrap_or(false) {
            "1"
        } else {
            "0"
        }
    ]));
    if !configured(id) && patch {
        return result(11);
    }
    let response = request(
        a,
        id,
        if patch { 6 } else { 5 },
        &if patch {
            route(id, "models")
        } else {
            "models".into()
        },
        json!([]),
        json!(form),
    );
    result(if response.status == 200 {
        if patch { 12 } else { 8 }
    } else if patch {
        11
    } else {
        10
    })
}
fn cached_file(a: &Value) -> Value {
    let Some(parsed) = parse_url(s(a, "url"), &a["config"], s(a, "kind"), true) else {
        return result(7);
    };
    let Some(entry) = cached(a, &parsed["id"], s(a, "kind")) else {
        return result(7);
    };
    let base = Path::new(s(&entry, "path"));
    let Ok(file) = percent_encoding::percent_decode_str(s(&parsed, "file")).decode_utf8() else {
        return result(7);
    };
    if file.contains(['\\', '\0'])
        || Path::new(file.as_ref())
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return result(7);
    }
    let path = base.join(file.as_ref());
    let (Ok(root), Ok(real)) = (base.canonicalize(), path.canonicalize()) else {
        return result(7);
    };
    if !real.starts_with(root) {
        return result(7);
    }
    json!({"result":5,"path":path.to_string_lossy()})
}
fn update(a: &Value) -> Value {
    let kind = s(a, "kind");
    let entries = io("io.cache_list", json!({"config":a["config"],"kind":kind}));
    let mut latest: BTreeMap<String, Value> = BTreeMap::new();
    for entry in entries.as_array().unwrap_or(&vec![]) {
        let id = &entry["id"];
        let key = route(id, kind) + s(&id["server"], "url");
        if latest
            .get(&key)
            .is_none_or(|old| old["version"].as_u64() < id["version"].as_u64())
        {
            latest.insert(key, id.clone());
        }
    }
    let mut ok = true;
    for id in latest.into_values() {
        let mut args = a.clone();
        args["id"] = id.clone();
        let cloud = details(&args);
        if cloud["result"] != 4 {
            ok = false;
        } else if cloud["id"]["version"].as_u64() > id["version"].as_u64() {
            args["id"] = cloud["id"].clone();
            ok &= download(&args)["result"] == 4;
        }
    }
    json!(ok)
}
fn batch(a: &Value) -> Value {
    let jobs = a["jobs"].as_u64().unwrap_or(2).clamp(1, 64) as usize;
    let mut pending = a["ids"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|id| (id.clone(), origin(s(&id["server"], "url"))))
        .collect::<Vec<_>>();
    let mut seen = HashSet::new();
    let mut results = vec![];
    while !pending.is_empty() {
        pending.retain(|(id, _)| seen.insert(resource_key(id)));
        let mut next = vec![];
        for chunk in pending.chunks(jobs) {
            let wave = std::thread::scope(|scope| {
                let handles = chunk
                    .iter()
                    .map(|(id, header_origin)| {
                        scope.spawn(move || {
                            let mut args = a.clone();
                            args["id"] = id.clone();
                            args["header_origin"] = header_origin.clone();
                            args["recursive"] = json!(false);
                            let mut out = download(&args);
                            out["requested_id"] = id.clone();
                            if out.get("id").is_none() {
                                out["id"] = id.clone();
                            }
                            out
                        })
                    })
                    .collect::<Vec<_>>();
                handles
                    .into_iter()
                    .map(|h| h.join().unwrap_or_else(|_| result(7)))
                    .collect::<Vec<_>>()
            });
            for (out, (_, header_origin)) in wave.into_iter().zip(chunk) {
                if s(a, "kind") == "models"
                    && let Some(deps) = out["dependencies"].as_array()
                {
                    next.extend(deps.iter().cloned().map(|id| (id, header_origin.clone())));
                }
                results.push(out);
            }
        }
        pending = next;
    }
    if s(a, "kind") == "worlds" {
        json!({"ok":results.iter().all(|r|r["result"]==4),"results":results})
    } else {
        json!(results)
    }
}
/// Internal compatibility dispatch. Native Rust callers may use `FuelClient`.
pub fn dispatch(op: &str, a: &Value) -> Option<Value> {
    Some(match op {
        "client.batch" => batch(a),
        "client.headers" => headers(a, &a["id"]),
        "client.licenses" => {
            let resp = request(a, &a["id"], 0, "licenses", json!([]), json!([]));
            core("json_licenses", json!({"text":body(&resp)}))
        }
        "client.list_path" => {
            let id = &a["id"];
            let kind = s(a, "kind");
            json!(if a["collection"] == true {
                format!("{}/collections/{}/{}", s(id, "owner"), s(id, "name"), kind)
            } else if !s(id, "owner").is_empty() {
                if !s(id, "name").is_empty() {
                    route(id, kind)
                } else {
                    format!("{}/{}", s(id, "owner"), kind)
                }
            } else if kind == "worlds" {
                kind.to_owned()
            } else {
                String::new()
            })
        }
        "client.parse" => parse_url(
            s(a, "url"),
            &a["config"],
            s(a, "kind"),
            a["file"].as_bool().unwrap_or(false),
        )
        .unwrap_or(Value::Null),
        "client.details" => details(a),
        "client.download" => download(a),
        "client.dependencies" => dep_closure(
            a,
            a["ids"]
                .as_array()
                .cloned()
                .unwrap_or_else(|| vec![a["id"].clone()]),
            a["recursive"].as_bool().unwrap_or(false),
        ),
        "client.cached_file" => cached_file(a),
        "client.upload" => upload(a, false),
        "client.patch" => upload(a, true),
        "client.delete" => {
            let id = &a["id"];
            if !configured(id) {
                result(3)
            } else {
                let resp = request(a, id, 2, &route(id, s(a, "kind")), json!([]), json!([]));
                result(match resp.status {
                    200 | 204 => 1,
                    404 => 2,
                    _ => 3,
                })
            }
        }
        "client.update" => update(a),
        "client.fetch" => fetch(a),
        _ => return None,
    })
}
fn fetch(a: &Value) -> Value {
    for kind in ["models", "worlds"] {
        for file in [false, true] {
            if let Some(parsed) = parse_url(s(a, "url"), &a["config"], kind, file) {
                let mut args = a.clone();
                args["kind"] = json!(kind);
                args["id"] = parsed["id"].clone();
                if file {
                    let found = cached_file(&args);
                    if found["result"] == 5 {
                        return found["path"].clone();
                    }
                } else if let Some(entry) = cached(&args, &args["id"], kind) {
                    return entry["path"].clone();
                }
                let res = download(&args);
                if res["result"] != 4 {
                    return json!("");
                }
                if file {
                    let found = cached_file(&args);
                    return found["path"].as_str().map_or(json!(""), |p| json!(p));
                }
                return res["path"].clone();
            }
        }
    }
    json!("")
}

#[cfg(test)]
#[path = "client_tests.rs"]
pub(crate) mod tests;
