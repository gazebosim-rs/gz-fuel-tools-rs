// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
//! Shared typed resource identity and URL component handling.
use crate::{ClientConfig, Error, ErrorKind, ResourceId, ResourceKind, ServerConfig};
use std::path::PathBuf;

pub(crate) struct ParsedResource {
    pub id: ResourceId,
    pub file: Option<PathBuf>,
}
pub(crate) fn component(value: &str) -> bool {
    !value.is_empty() && value != "." && value != ".." && !value.contains(['/', '\\', '\0'])
}
pub(crate) fn normalize(value: &str, kind: ResourceKind) -> String {
    if kind == ResourceKind::Collection {
        value.into()
    } else {
        value.to_ascii_lowercase()
    }
}
fn invalid(message: &str) -> Error {
    Error::new(ErrorKind::InvalidInput, message)
}
pub(crate) fn canonicalize(id: &ResourceId, kind: ResourceKind) -> Result<ResourceId, Error> {
    if !component(&id.owner) || !component(&id.name) {
        return Err(invalid("Invalid Fuel resource owner or name"));
    }
    let url = url::Url::parse(&id.server.url).map_err(|_| invalid("Invalid Fuel server URL"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid("Invalid Fuel server URL"));
    }
    if !id.server.version.is_empty() && !component(&id.server.version) {
        return Err(invalid("Invalid Fuel API version"));
    }
    let mut id = id.clone();
    id.owner = normalize(&id.owner, kind);
    id.name = normalize(&id.name, kind);
    Ok(id)
}
pub(crate) fn endpoint(server: &ServerConfig, segments: &[&str]) -> Result<url::Url, Error> {
    let mut url = url::Url::parse(&server.url).map_err(|_| invalid("Invalid Fuel server URL"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid("Invalid Fuel server URL"));
    }
    let mut parts = url
        .path_segments_mut()
        .map_err(|_| invalid("Invalid Fuel server URL"))?;
    parts.pop_if_empty();
    if !server.version.is_empty() {
        parts.push(&server.version);
    }
    for segment in segments {
        parts.push(segment);
    }
    drop(parts);
    Ok(url)
}
pub(crate) fn resource_url(
    id: &ResourceId,
    kind: ResourceKind,
    include_api: bool,
    include_version: bool,
) -> Result<url::Url, Error> {
    let id = canonicalize(id, kind)?;
    let mut server = id.server.clone();
    if !include_api {
        server.version.clear();
    }
    let mut url = endpoint(&server, &[&id.owner, kind.route(), &id.name])?;
    if include_version && kind != ResourceKind::Collection {
        url.path_segments_mut()
            .map_err(|_| invalid("Invalid resource URL"))?
            .push(&if id.version == 0 {
                "tip".into()
            } else {
                id.version.to_string()
            });
    }
    Ok(url)
}
pub(crate) fn parse(
    input: &str,
    config: &ClientConfig,
    kind: ResourceKind,
    file: bool,
) -> Result<ParsedResource, Error> {
    let bad = || invalid("Invalid Fuel resource URL");
    let (scheme, tail) = input.split_once("://").ok_or_else(bad)?;
    if !matches!(scheme, "http" | "https") || input.contains(['?', '#', '\\', '\0']) {
        return Err(bad());
    }
    let (authority, path) = tail.split_once('/').ok_or_else(bad)?;
    let base = format!("{scheme}://{authority}");
    let valid = url::Url::parse(&base).map_err(|_| bad())?;
    if valid.host_str().is_none() || authority.chars().any(char::is_whitespace) {
        return Err(bad());
    }
    let mut parts = path
        .split('/')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    // The longest configured base path owns this URL. Compare decoded prefix
    // components, retaining raw resource components for traversal validation.
    let configured = config
        .servers
        .iter()
        .filter_map(|server| {
            let server_url = url::Url::parse(&server.url).ok()?;
            if server_url.origin() != valid.origin() {
                return None;
            }
            let prefix = server_url
                .path_segments()?
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>();
            if prefix.len() > parts.len()
                || !prefix.iter().zip(&parts).all(|(expected, actual)| {
                    percent_encoding::percent_decode_str(expected)
                        .decode_utf8()
                        .ok()
                        == percent_encoding::percent_decode_str(actual)
                            .decode_utf8()
                            .ok()
                })
            {
                return None;
            }
            Some((prefix.len(), server))
        })
        .max_by_key(|(length, _)| *length);
    if let Some((length, _)) = configured {
        parts.drain(..length);
    }
    let api = if parts.first().is_some_and(|v| {
        v.split_once('.').is_some_and(|(a, b)| {
            !a.is_empty() && !b.is_empty() && a.bytes().chain(b.bytes()).all(|n| n.is_ascii_digit())
        })
    }) {
        parts.remove(0)
    } else {
        ""
    };
    if parts.len() < 3 || parts[1] != kind.route() {
        return Err(bad());
    }
    let decode = |s: &str| {
        percent_encoding::percent_decode_str(s)
            .decode_utf8()
            .map(|s| s.into_owned())
            .map_err(|_| bad())
    };
    let owner = decode(parts[0])?;
    let name = decode(parts[2])?;
    if !component(&owner) || !component(&name) {
        return Err(bad());
    }
    let server = configured
        .map(|(_, server)| server.clone())
        .unwrap_or_else(|| ServerConfig {
            url: base,
            version: api.into(),
            ..ServerConfig::default()
        });
    let version = match parts.get(3).copied().unwrap_or("") {
        "" | "tip" => 0,
        value => value.parse().map_err(|_| bad())?,
    };
    let file = if file {
        if kind == ResourceKind::Collection || parts.len() < 6 || parts[4] != "files" {
            return Err(bad());
        }
        let decoded = decode(&parts[5..].join("/"))?;
        if decoded.split('/').any(|part| !component(part)) {
            return Err(bad());
        }
        Some(PathBuf::from(decoded))
    } else {
        if parts.len()
            > if kind == ResourceKind::Collection {
                3
            } else {
                4
            }
        {
            return Err(bad());
        }
        None
    };
    Ok(ParsedResource {
        id: ResourceId {
            owner: normalize(&owner, kind),
            name: normalize(&name, kind),
            server,
            version,
            ..ResourceId::default()
        },
        file,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configured_base_paths_round_trip_and_choose_the_longest_prefix() {
        let server = ServerConfig {
            url: "https://example.com/fuel/prefix".into(),
            version: "2.0".into(),
            api_key: "secret".into(),
            ..ServerConfig::default()
        };
        let config = ClientConfig {
            servers: vec![
                ServerConfig {
                    url: "https://example.com/fuel".into(),
                    ..Default::default()
                },
                server.clone(),
            ],
            ..ClientConfig::default()
        };
        let id = ResourceId {
            name: "My Model".into(),
            owner: "OpenRobotics".into(),
            version: 3,
            server,
            ..ResourceId::default()
        };
        let url = resource_url(&id, ResourceKind::Model, true, true).unwrap();
        let parsed = parse(url.as_str(), &config, ResourceKind::Model, false).unwrap();
        assert_eq!(parsed.id.name, "my model");
        assert_eq!(parsed.id.server.api_key, "secret");
        assert_eq!(parsed.id.server.url, id.server.url);
        assert_eq!(parsed.id.version, 3);
        assert!(
            parse(
                "https://example.com/fuel/prefix/../models/name",
                &config,
                ResourceKind::Model,
                false
            )
            .is_err()
        );
    }
    #[test]
    fn direct_identifiers_reject_non_http_servers_and_unsafe_api_versions() {
        let mut id = ResourceId {
            name: "Model".into(),
            owner: "Owner".into(),
            ..ResourceId::default()
        };
        id.server.url = "ftp://example.com".into();
        assert!(canonicalize(&id, ResourceKind::Model).is_err());
        id.server.url = "https://example.com".into();
        id.server.version = "../1.0".into();
        assert!(canonicalize(&id, ResourceKind::Model).is_err());
        assert!(
            parse(
                "http://host /a/models/b",
                &ClientConfig::default(),
                ResourceKind::Model,
                false
            )
            .is_err()
        );
        assert_eq!(normalize("ÉCOLE", ResourceKind::Model), "École");
        assert_eq!(normalize("ÉCOLE", ResourceKind::Collection), "ÉCOLE");
    }
}
