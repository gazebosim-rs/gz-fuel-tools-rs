// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
//! One bounded pagination state machine for native, command and C++ clients.
use crate::{ClientConfig, Error, ErrorKind, ResourceId, ResourceKind, ServerConfig};
use std::collections::{BTreeMap, HashSet};
pub(crate) struct ListingPage {
    pub items: Vec<ResourceId>,
    pub total: Option<usize>,
    pub has_next: Option<bool>,
    pub response_bytes: usize,
}
pub(crate) fn headers(
    values: &BTreeMap<String, String>,
) -> Result<(Option<usize>, Option<bool>), Error> {
    let total = values
        .get("x-total-count")
        .map(|v| {
            v.parse::<usize>()
                .map_err(|_| Error::new(ErrorKind::Metadata, "Invalid Fuel listing count"))
        })
        .transpose()?;
    let next = values
        .get("link")
        .map(|v| link_has_next(v).map_err(|e| Error::new(ErrorKind::Metadata, e)))
        .transpose()?;
    Ok((total, next))
}
type Key = (String, String, String, u32);
pub(crate) struct Pager {
    config: ClientConfig,
    kind: ResourceKind,
    server: ServerConfig,
    path: String,
    headers: Vec<String>,
    page: u32,
    done: bool,
    received: usize,
    bytes: u64,
    total: Option<usize>,
    seen: HashSet<Vec<Key>>,
}
impl Pager {
    pub(crate) fn new(
        config: ClientConfig,
        kind: ResourceKind,
        server: ServerConfig,
        path: String,
        headers: Vec<String>,
    ) -> Self {
        Self {
            config,
            kind,
            server,
            path,
            headers,
            page: 0,
            done: false,
            received: 0,
            bytes: 0,
            total: None,
            seen: HashSet::new(),
        }
    }
    pub(crate) fn next_page(&mut self) -> Result<Option<Vec<ResourceId>>, Error> {
        if self.done {
            return Ok(None);
        }
        // A failed page permanently fuses the iterator, including transport errors.
        self.done = true;
        if self.page as usize >= self.config.limits.max_pages {
            return Err(Error::new(
                ErrorKind::Limit,
                "Fuel listing page limit exceeded",
            ));
        }
        self.page = self
            .page
            .checked_add(1)
            .ok_or_else(|| Error::new(ErrorKind::Limit, "Fuel listing page counter overflow"))?;
        let page = crate::client::ClientCore::new(&self.config).list_page(
            self.kind,
            &self.server,
            &self.path,
            self.page,
            &self.headers,
        )?;
        self.accept(page)
    }
    fn accept(&mut self, page: ListingPage) -> Result<Option<Vec<ResourceId>>, Error> {
        let invalid = |message| Error::new(ErrorKind::Metadata, message);
        self.received = self.received.checked_add(page.items.len()).ok_or_else(|| {
            Error::new(ErrorKind::Limit, "Fuel listing resource counter overflow")
        })?;
        self.bytes = self
            .bytes
            .checked_add(page.response_bytes as u64)
            .ok_or_else(|| Error::new(ErrorKind::Limit, "Fuel listing byte counter overflow"))?;
        if self.received > self.config.limits.max_listing_resources
            || self.bytes > self.config.limits.max_listing_bytes
        {
            return Err(Error::new(ErrorKind::Limit, "Fuel listing budget exceeded"));
        }
        if let (Some(old), Some(new)) = (self.total, page.total)
            && old != new
        {
            return Err(invalid("Fuel listing changed its total count"));
        }
        self.total = page.total.or(self.total);
        let empty = page.items.is_empty();
        if self.total.is_some_and(|t| {
            self.received > t
                || ((empty || page.has_next == Some(false)) && self.received < t)
                || (page.has_next == Some(true) && self.received == t)
        }) {
            return Err(invalid(
                "Fuel listing disagrees with its advertised total count",
            ));
        }
        if empty && page.has_next == Some(true) {
            return Err(invalid(
                "Fuel listing returned an empty page with a next link",
            ));
        }
        if !empty {
            let keys = page
                .items
                .iter()
                .map(|id| {
                    (
                        id.server.url.clone(),
                        id.owner.clone(),
                        id.name.clone(),
                        id.version,
                    )
                })
                .collect();
            if !self.seen.insert(keys) {
                return Err(invalid("Fuel server repeated a listing page"));
            }
        }
        self.done = empty || page.has_next == Some(false) || self.total == Some(self.received);
        Ok(if empty { None } else { Some(page.items) })
    }
}
fn link_parts(value: &str, separator: char) -> Result<Vec<&str>, String> {
    let (mut angle, mut quoted, mut escaped) = (false, false, false);
    let mut parts = vec![];
    let mut start = 0;
    for (index, character) in value.char_indices() {
        if escaped {
            escaped = false;
        } else if quoted {
            match character {
                '\\' => escaped = true,
                '"' => quoted = false,
                _ => {}
            }
        } else if angle {
            if character == '>' {
                angle = false;
            } else if character == '<' {
                return Err("Invalid nested URI in Fuel Link header".into());
            }
        } else {
            match character {
                '<' => angle = true,
                '>' => return Err("Invalid URI in Fuel Link header".into()),
                '"' => quoted = true,
                delimiter if delimiter == separator => {
                    parts.push(&value[start..index]);
                    start = index + character.len_utf8();
                }
                _ => {}
            }
        }
    }
    if angle || quoted || escaped {
        return Err("Unterminated URI or quoted string in Fuel Link header".into());
    }
    parts.push(&value[start..]);
    Ok(parts)
}
pub(crate) fn link_has_next(value: &str) -> Result<bool, String> {
    let mut next = false;
    for link in link_parts(value, ',')? {
        let parts = link_parts(link, ';')?;
        let target = parts[0].trim();
        if !target.starts_with('<') || !target.ends_with('>') || target.len() <= 2 {
            return Err("Invalid target in Fuel Link header".into());
        }
        let mut relation = None;
        for parameter in &parts[1..] {
            let Some((name, value)) = parameter.split_once('=') else {
                if parameter.trim().eq_ignore_ascii_case("rel") {
                    return Err("Missing Fuel Link relation value".into());
                }
                // RFC Link extensions may be bare parameters without a value.
                continue;
            };
            if !name.trim().eq_ignore_ascii_case("rel") {
                continue;
            }
            if relation.is_some() {
                return Err("Repeated relation in Fuel Link header".into());
            }
            let value = value.trim();
            let value = if value.starts_with('"') {
                let quoted = value
                    .strip_prefix('"')
                    .and_then(|v| v.strip_suffix('"'))
                    .ok_or("Invalid quoted Fuel Link relation")?;
                let mut decoded = String::new();
                let mut characters = quoted.chars();
                while let Some(character) = characters.next() {
                    decoded.push(if character == '\\' {
                        characters
                            .next()
                            .ok_or("Invalid escape in Fuel Link relation")?
                    } else {
                        character
                    });
                }
                decoded
            } else {
                value.to_owned()
            };
            if value.is_empty() {
                return Err("Empty relation in Fuel Link header".into());
            }
            relation = Some(
                value
                    .split_whitespace()
                    .any(|item| item.eq_ignore_ascii_case("next")),
            );
        }
        next |= relation.ok_or("Missing relation in Fuel Link header")?;
    }
    Ok(next)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn pager() -> Pager {
        Pager::new(
            ClientConfig::default(),
            ResourceKind::Model,
            ServerConfig::default(),
            "models".into(),
            vec![],
        )
    }
    fn page(name: &str, total: Option<usize>, next: Option<bool>) -> ListingPage {
        ListingPage {
            items: if name.is_empty() {
                vec![]
            } else {
                vec![ResourceId {
                    name: name.into(),
                    owner: "owner".into(),
                    ..ResourceId::default()
                }]
            },
            total,
            has_next: next,
            response_bytes: 12,
        }
    }
    #[test]
    fn detects_non_adjacent_cycles_without_serializing_metadata() {
        let mut p = pager();
        p.accept(page("a", None, None)).unwrap();
        p.accept(page("b", None, None)).unwrap();
        let mut repeated = page("a", None, None);
        repeated.items[0].description = "Changed metadata must not hide the cycle".into();
        assert!(p.accept(repeated).is_err());
    }
    #[test]
    fn pagination_limits_counts_and_end_conditions() {
        for (first, second) in [
            (page("a", Some(3), None), page("b", Some(2), None)),
            (page("a", Some(2), None), page("", None, None)),
            (page("a", None, None), page("", None, Some(true))),
        ] {
            let mut p = pager();
            p.accept(first).unwrap();
            assert!(p.accept(second).is_err());
        }
        for page in [
            page("a", Some(0), None),
            page("a", Some(1), Some(true)),
            page("a", Some(2), Some(false)),
        ] {
            assert!(pager().accept(page).is_err());
        }
        let mut p = pager();
        assert!(p.accept(page("a", Some(1), None)).unwrap().is_some());
        assert!(p.next_page().unwrap().is_none());
        let mut p = pager();
        assert!(p.accept(page("", None, None)).unwrap().is_none());
        assert!(p.next_page().unwrap().is_none());
        let mut p = pager();
        p.config.limits.max_listing_resources = 0;
        assert_eq!(
            p.accept(page("a", None, None)).unwrap_err().kind,
            ErrorKind::Limit
        );
        let mut p = pager();
        p.config.limits.max_listing_bytes = 0;
        assert_eq!(
            p.accept(page("a", None, None)).unwrap_err().kind,
            ErrorKind::Limit
        );
        let mut p = pager();
        p.config.limits.max_pages = 0;
        assert_eq!(p.next_page().unwrap_err().kind, ErrorKind::Limit);
        assert!(p.next_page().unwrap().is_none());
        let mut p = pager();
        p.page = u32::MAX;
        p.config.limits.max_pages = usize::MAX;
        assert_eq!(p.next_page().unwrap_err().kind, ErrorKind::Limit);
        let mut p = pager();
        p.received = usize::MAX;
        assert!(p.accept(page("a", None, None)).is_err());
        let mut p = pager();
        p.bytes = u64::MAX;
        assert!(p.accept(page("a", None, None)).is_err());
        assert!(headers(&BTreeMap::from([("x-total-count".into(), "wrong".into())])).is_err());
        assert_eq!(headers(&BTreeMap::new()).unwrap(), (None, None));
    }
    #[test]
    fn native_iterator_stops_a_b_a_and_preserves_collection_case() {
        use crate::client::tests::Server;
        let server = Server::new(|request| {
            assert!(request.contains("Owner/collections/MyCollection/models"));
            let name = if request.contains("page=2") { "b" } else { "a" };
            (
                200,
                vec![],
                format!(r#"[{{"name":"{name}","owner":"Owner"}}]"#).into_bytes(),
            )
        });
        let client = crate::FuelClient::default();
        let id = ResourceId {
            owner: "Owner".into(),
            name: "MyCollection".into(),
            server: ServerConfig {
                url: server.url.clone(),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut iter = client.list_collection(&id, ResourceKind::Model);
        assert_eq!(iter.next().unwrap().unwrap().name, "a");
        assert_eq!(iter.next().unwrap().unwrap().name, "b");
        assert!(iter.next().unwrap().is_err());
        assert!(iter.next().is_none());
        assert!(iter.next().is_none());
    }
}
