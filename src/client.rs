// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
//! Legacy JSON compatibility adaptation around the typed resource domain.
pub(crate) use crate::client_native::ClientCore;
use crate::{ClientConfig, Downloaded, Error, ResourceId, ResourceKind, identity};
use serde_json::{Value, json};
#[cfg(test)]
use std::{fs, path::Path};
fn s<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or("")
}
#[cfg(test)]
fn io(op: &str, args: Value) -> Value {
    crate::io::dispatch(op, &args).unwrap_or(Value::Null)
}
fn kind(value: &str) -> Option<ResourceKind> {
    match value {
        "models" => Some(ResourceKind::Model),
        "worlds" => Some(ResourceKind::World),
        "collections" => Some(ResourceKind::Collection),
        _ => None,
    }
}
fn config(value: &Value) -> Result<ClientConfig, Error> {
    crate::core::typed_config(value)
}
#[cfg(test)]
fn api_version(value: &str) -> bool {
    value.split_once('.').is_some_and(|(a, b)| {
        !a.is_empty() && !b.is_empty() && a.bytes().chain(b.bytes()).all(|n| n.is_ascii_digit())
    })
}
pub(crate) fn parse_url(url: &str, settings: &Value, route: &str, file: bool) -> Option<Value> {
    let parsed = identity::parse(url, &config(settings).ok()?, kind(route)?, file).ok()?;
    Some(json!({"id":parsed.id,"file":parsed.file.unwrap_or_default()}))
}
fn origin(value: &str) -> Value {
    url::Url::parse(value)
        .map(|url| json!([url.scheme(), url.host_str(), url.port_or_known_default()]))
        .unwrap_or_else(|_| json!(value))
}
fn headers(args: &Value, id: &Value) -> Value {
    let mut headers = if args["header_origin"].is_null()
        || args["header_origin"] == origin(s(&id["server"], "url"))
    {
        args["headers"].as_array().cloned().unwrap_or_default()
    } else {
        vec![]
    };
    if !headers.iter().any(|line| {
        line.as_str()
            .and_then(|line| line.split_once(':'))
            .is_some_and(|(name, _)| name.trim().eq_ignore_ascii_case("private-token"))
    }) && !s(&id["server"], "api_key").is_empty()
    {
        headers.push(json!(format!(
            "Private-token: {}",
            s(&id["server"], "api_key")
        )));
    }
    json!(headers)
}
fn downloaded(value: Result<Downloaded, Error>) -> Value {
    match value {
        Ok(value) => {
            json!({"result":4,"id":value.id,"path":value.path,"dependencies":value.dependencies})
        }
        Err(error) => json!({"result":7,"error":error.to_string()}),
    }
}
fn result(value: Result<(), Error>, success: u8, failure: u8) -> Value {
    match value {
        Ok(()) => json!({"result":success}),
        Err(error) => json!({"result":failure,"error":error.to_string()}),
    }
}
fn cached_file(args: &Value, config: &ClientConfig, kind: ResourceKind) -> Value {
    let found = (|| {
        let parsed = identity::parse(s(args, "url"), config, kind, true)?;
        let entry = crate::io::lookup_resource(config, kind, &parsed.id)?
            .ok_or_else(|| Error::from("Fuel resource is not cached"))?;
        let path = entry.path.join(parsed.file.unwrap());
        let root = entry.path.canonicalize()?;
        let real = path.canonicalize()?;
        if !real.starts_with(root) || !real.is_file() {
            return Err(Error::from("Invalid cached Fuel file"));
        }
        Ok::<_, Error>(path)
    })();
    match found {
        Ok(path) => json!({"result":5,"path":path}),
        Err(error) => json!({"result":7,"error":error.to_string()}),
    }
}
pub(crate) fn dispatch(op: &str, args: &Value) -> Option<Value> {
    if !op.starts_with("client.") {
        return None;
    }
    if op == "client.headers" {
        return Some(headers(args, &args["id"]));
    }
    if op == "client.parse" {
        return Some(
            parse_url(
                s(args, "url"),
                &args["config"],
                s(args, "kind"),
                args["file"].as_bool().unwrap_or(false),
            )
            .unwrap_or(Value::Null),
        );
    }
    let settings = match config(&args["config"]) {
        Ok(config) => config,
        Err(error) => return Some(json!({"result":7,"error":error.to_string()})),
    };
    let core = ClientCore::new(&settings);
    let id = serde_json::from_value::<ResourceId>(args["id"].clone()).unwrap_or_default();
    let resource_kind = kind(s(args, "kind")).unwrap_or(ResourceKind::Model);
    let request_headers = args["headers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    Some(match op {
        "client.parse" => parse_url(
            s(args, "url"),
            &args["config"],
            s(args, "kind"),
            args["file"].as_bool().unwrap_or(false),
        )
        .unwrap_or(Value::Null),
        "client.headers" => headers(args, &args["id"]),
        "client.list_path" => json!(if args["collection"] == true {
            format!(
                "{}/collections/{}/{}",
                id.owner,
                id.name,
                resource_kind.route()
            )
        } else if !id.owner.is_empty() {
            if !id.name.is_empty() {
                format!("{}/{}/{}", id.owner, resource_kind.route(), id.name)
            } else {
                format!("{}/{}", id.owner, resource_kind.route())
            }
        } else if resource_kind == ResourceKind::World {
            "worlds".into()
        } else {
            String::new()
        }),
        "client.details" => match core.details(resource_kind, &id, &request_headers) {
            Ok(id) => json!({"result":4,"id":id}),
            Err(error) => json!({"result":7,"error":error.to_string()}),
        },
        "client.download" => downloaded(if args["recursive"] == false {
            core.download_shallow(resource_kind, &id, &request_headers)
        } else {
            core.download(resource_kind, &id, &request_headers)
        }),
        "client.dependencies" => {
            let ids = if let Some(ids) = args["ids"].as_array() {
                ids.iter()
                    .filter_map(|value| serde_json::from_value(value.clone()).ok())
                    .collect()
            } else {
                vec![id.clone()]
            };
            let mut dependencies = vec![];
            let mut failure = None;
            for id in ids {
                match core.dependencies(&id, args["recursive"].as_bool().unwrap_or(false)) {
                    Ok(items) => {
                        for item in items {
                            if !dependencies.contains(&item) {
                                dependencies.push(item);
                            }
                        }
                    }
                    Err(error) => {
                        failure = Some(error);
                        break;
                    }
                }
            }
            if let Some(error) = failure {
                json!({"result":7,"error":error.to_string()})
            } else {
                json!({"result":4,"dependencies":dependencies})
            }
        }
        "client.batch" => {
            let ids =
                serde_json::from_value::<Vec<ResourceId>>(args["ids"].clone()).unwrap_or_default();
            let jobs = args["jobs"].as_u64().unwrap_or(2).clamp(1, 64) as usize;
            let results = core
                .download_many_with_headers(resource_kind, &ids, jobs, &request_headers)
                .into_iter()
                .map(downloaded)
                .collect::<Vec<_>>();
            if resource_kind == ResourceKind::World {
                json!({"ok":results.iter().all(|item|item["result"]==4),"results":results})
            } else {
                json!(results)
            }
        }
        "client.cached_file" => cached_file(args, &settings, resource_kind),
        "client.fetch" => json!(core.fetch(s(args, "url")).ok().unwrap_or_default()),
        "client.upload" => result(
            core.upload(
                std::path::Path::new(s(args, "path")),
                &id,
                &request_headers,
                args["private"].as_bool().unwrap_or(false),
                s(args, "owner"),
            ),
            8,
            10,
        ),
        "client.patch" => {
            let mut id = id;
            id.private = args["private"].as_bool().unwrap_or(id.private);
            let path = (!s(args, "path").is_empty()).then(|| std::path::Path::new(s(args, "path")));
            match core.patch(&id, &request_headers, path) {
                Ok(()) => json!({"result":12}),
                Err(error) => {
                    let upload_error = path.is_some()
                        && (matches!(
                            error.kind,
                            crate::ErrorKind::Io
                                | crate::ErrorKind::Metadata
                                | crate::ErrorKind::Limit
                        ) || (error.kind == crate::ErrorKind::InvalidInput
                            && identity::canonicalize(&id, ResourceKind::Model).is_ok()));
                    json!({"result":if upload_error{10}else{11},"error":error.to_string()})
                }
            }
        }
        "client.delete" => match core.delete(resource_kind, &id, &request_headers) {
            Ok(()) => json!({"result":1}),
            Err(error) => {
                json!({"result":if error.status==Some(404){2}else{3},"error":error.to_string()})
            }
        },
        "client.update" => json!(core.update(resource_kind, &request_headers).is_ok()),
        "client.licenses" => {
            let response = crate::transport::transport(
                &json!({"method":0,"url":id.server.url,"version":id.server.version,"path":"licenses","headers":headers(args,&args["id"]),"user_agent":settings.user_agent}),
            );
            crate::core::dispatch(
                "json_licenses",
                &json!({"text":String::from_utf8_lossy(&response.data)}),
            )
            .unwrap_or(Value::Null)
        }
        _ => return None,
    })
}
#[cfg(test)]
#[path = "client_tests.rs"]
pub(crate) mod tests;
