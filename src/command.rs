// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
//! Command operations and resource metadata conversion, shared by all frontends.
use serde_json::{Map, Value, json};
use std::{collections::BTreeSet, fs};
fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
fn call(op: &str, a: &Value) -> Value {
    crate::execute(op, a).unwrap_or(Value::Null)
}

#[derive(Debug, PartialEq)]
enum Token {
    Word(String),
    Text(String),
    Open,
    Close,
    Colon,
}
fn tokens(input: &str) -> Result<Vec<Token>, String> {
    let mut out = Vec::new();
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() || c == ',' || c == ';' => {}
            '#' => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'/') => {
                chars.next();
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut closed = false;
                while let Some(c) = chars.next() {
                    if c == '*' && chars.peek() == Some(&'/') {
                        chars.next();
                        closed = true;
                        break;
                    }
                }
                if !closed {
                    return Err("Unterminated block comment".into());
                }
            }
            '{' | '<' => out.push(Token::Open),
            '}' | '>' => out.push(Token::Close),
            ':' => out.push(Token::Colon),
            '"' | '\'' => {
                let mut text = Vec::new();
                let mut closed = false;
                while let Some(x) = chars.next() {
                    if x == c {
                        closed = true;
                        break;
                    }
                    if x == '\\' {
                        let n = chars.next().ok_or("Unterminated escape")?;
                        match n {
                            'n' => text.push(b'\n'),
                            'r' => text.push(b'\r'),
                            't' => text.push(b'\t'),
                            'a' => text.push(7),
                            'b' => text.push(8),
                            'f' => text.push(12),
                            'v' => text.push(11),
                            '\\' => text.push(b'\\'),
                            '\'' => text.push(b'\''),
                            '"' => text.push(b'"'),
                            '?' => text.push(b'?'),
                            '0'..='7' => {
                                let mut number = n.to_digit(8).unwrap();
                                for _ in 0..2 {
                                    if let Some(d) = chars.peek().and_then(|c| c.to_digit(8)) {
                                        number = number * 8 + d;
                                        chars.next();
                                    } else {
                                        break;
                                    }
                                }
                                text.push(
                                    u8::try_from(number)
                                        .map_err(|_| "Octal escape exceeds a byte")?,
                                );
                            }
                            'x' | 'X' => {
                                let first = chars
                                    .next()
                                    .and_then(|c| c.to_digit(16))
                                    .ok_or("Invalid hex escape")?;
                                let mut number = first;
                                if let Some(d) = chars.peek().and_then(|c| c.to_digit(16)) {
                                    number = number * 16 + d;
                                    chars.next();
                                }
                                text.push(number as u8);
                            }
                            'u' | 'U' => {
                                let mut number = 0;
                                for _ in 0..if n == 'u' { 4 } else { 8 } {
                                    number = number * 16
                                        + chars
                                            .next()
                                            .and_then(|c| c.to_digit(16))
                                            .ok_or("Invalid Unicode escape")?;
                                }
                                let ch = char::from_u32(number).ok_or("Invalid Unicode scalar")?;
                                text.extend_from_slice(ch.encode_utf8(&mut [0; 4]).as_bytes());
                            }
                            _ => return Err("Unsupported text escape".into()),
                        }
                    } else {
                        text.extend_from_slice(x.encode_utf8(&mut [0; 4]).as_bytes());
                    }
                }
                if !closed {
                    return Err("Unterminated string".into());
                }
                out.push(Token::Text(
                    String::from_utf8(text).map_err(|_| "Metadata string is not UTF-8")?,
                ));
            }
            c if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' => {
                let mut word = String::from(c);
                while chars.peek().is_some_and(|c| {
                    c.is_ascii_alphanumeric() || *c == '_' || *c == '-' || *c == '.'
                }) {
                    word.push(chars.next().unwrap());
                }
                out.push(Token::Word(word));
            }
            _ => return Err(format!("Unexpected metadata character {c}")),
        }
    }
    Ok(out)
}
fn message(ts: &[Token], index: &mut usize, depth: usize) -> Result<Map<String, Value>, String> {
    if depth > 64 {
        return Err("Metadata nesting exceeds 64 levels".into());
    }
    let mut out = Map::new();
    while *index < ts.len() {
        if ts[*index] == Token::Close {
            if depth == 0 {
                return Err("Unexpected closing message".into());
            }
            *index += 1;
            return Ok(out);
        }
        let Token::Word(key) = &ts[*index] else {
            return Err("Expected field name".into());
        };
        let key = key.clone();
        *index += 1;
        if ts.get(*index) == Some(&Token::Colon) {
            *index += 1;
        }
        let value = match ts.get(*index) {
            Some(Token::Open) => {
                *index += 1;
                Value::Object(message(ts, index, depth + 1)?)
            }
            Some(Token::Text(text)) => {
                let mut text = text.clone();
                *index += 1;
                while let Some(Token::Text(next)) = ts.get(*index) {
                    text.push_str(next);
                    *index += 1;
                }
                json!(text)
            }
            Some(Token::Word(word)) => {
                *index += 1;
                serde_json::from_str(word).unwrap_or_else(|_| json!(word))
            }
            _ => return Err("Expected field value".into()),
        };
        if let Some(previous) = out.get_mut(&key) {
            if !previous.is_array() {
                *previous = json!([previous.clone()]);
            }
            previous.as_array_mut().unwrap().push(value);
        } else {
            out.insert(key, value);
        }
    }
    if depth > 0 {
        Err("Unterminated message".into())
    } else {
        Ok(out)
    }
}
fn repeated(v: &Value) -> Vec<Value> {
    match v {
        Value::Null => vec![],
        Value::Array(a) => a.clone(),
        _ => vec![v.clone()],
    }
}
pub fn parse_pbtxt(input: &str) -> Result<Value, String> {
    let ts = tokens(input)?;
    let raw = Value::Object(message(&ts, &mut 0, 0)?);
    let allowed = [
        "model",
        "world",
        "name",
        "description",
        "version",
        "authors",
        "legal",
        "tags",
        "annotations",
        "dependencies",
        "compatibilities",
        "categories",
    ];
    if raw
        .as_object()
        .unwrap()
        .keys()
        .any(|k| !allowed.contains(&k.as_str()))
    {
        return Err("Unknown Fuel metadata field".into());
    }
    if raw.get("model").is_some() && raw.get("world").is_some() {
        return Err("Multiple resource types".into());
    }
    for key in ["name", "description"] {
        if raw.get(key).is_some_and(|v| !v.is_string()) {
            return Err(format!("Expected string for {key}"));
        }
    }
    if raw.get("version").is_some_and(|v| v.as_i64().is_none()) {
        return Err("Expected integer version".into());
    }
    for (key, fields) in [
        ("model", vec!["file", "file_format"]),
        ("world", vec!["file", "file_format"]),
        ("legal", vec!["copyright", "license"]),
        ("categories", vec!["first", "second"]),
    ] {
        if let Some(value) = raw.get(key) {
            let obj = value
                .as_object()
                .ok_or_else(|| format!("Expected message for {key}"))?;
            if obj.keys().any(|k| !fields.contains(&k.as_str())) {
                return Err(format!("Unknown {key} field"));
            }
            for (name, value) in obj {
                if name != "file_format" && !value.is_string() {
                    return Err(format!("Expected string for {key}.{name}"));
                }
                if name == "file_format" && !value.is_object() {
                    return Err("Expected file format message".into());
                }
            }
        }
    }
    for (key, fields) in [
        ("authors", vec!["name", "email"]),
        ("dependencies", vec!["uri"]),
        ("annotations", vec!["key", "value"]),
    ] {
        for value in repeated(&raw[key]) {
            let obj = value
                .as_object()
                .ok_or_else(|| format!("Expected {key} message"))?;
            if obj
                .iter()
                .any(|(k, v)| !fields.contains(&k.as_str()) || !v.is_string())
            {
                return Err(format!("Invalid {key} field"));
            }
        }
    }
    if repeated(&raw["tags"]).iter().any(|v| !v.is_string()) {
        return Err("Expected string tags".into());
    }
    let mut result = raw.clone();
    result["license"] = json!(s(&raw["legal"], "license"));
    for key in ["tags", "authors", "dependencies", "compatibilities"] {
        result[key] = json!(repeated(&raw[key]));
    }
    let mut annotations = Map::new();
    for item in repeated(&raw["annotations"]) {
        let key = s(&item, "key");
        if !key.is_empty() {
            annotations.insert(key.into(), json!(s(&item, "value")));
        }
    }
    result["annotations"] = Value::Object(annotations);
    result["categories"] = json!(
        ["first", "second"]
            .iter()
            .filter_map(|k| raw["categories"][k].as_str())
            .collect::<Vec<_>>()
    );
    Ok(result)
}
pub fn parse_config(input: &str) -> Result<Value, String> {
    let doc = roxmltree::Document::parse(input.trim_start()).map_err(|e| e.to_string())?;
    let root = doc.root_element();
    if root.tag_name().name() != "model" && root.tag_name().name() != "world" {
        return Err("Expected model/world configuration".into());
    }
    let text = |name: &str| {
        root.children()
            .find(|n| n.has_tag_name(name))
            .and_then(|n| n.text())
            .unwrap_or("")
            .trim()
            .to_owned()
    };
    if text("name").is_empty() {
        return Err("Missing resource name".into());
    }
    let kind = root.tag_name().name();
    let mut result = json!({"name":text("name"),"description":text("description"),"license":text("license"),"version":text("version").chars().take_while(|c|c.is_ascii_digit()||*c=='-'||*c=='+').collect::<String>().parse::<i32>().unwrap_or(0),"tags":[],"categories":[],"annotations":{},"dependencies":[],"authors":[]});
    let sdf = root
        .children()
        .filter(|n| n.has_tag_name("sdf"))
        .max_by_key(|n| {
            n.attribute("version")
                .unwrap_or("")
                .split('.')
                .map(|v| v.parse::<u32>().unwrap_or(0))
                .collect::<Vec<_>>()
        });
    if let Some(sdf) = sdf {
        let numbers = sdf
            .attribute("version")
            .unwrap_or("")
            .split('.')
            .map(|n| n.parse::<u32>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "Invalid SDF version")?;
        if numbers.len() < 2 || sdf.text().unwrap_or("").trim().is_empty() {
            return Err("Invalid SDF declaration".into());
        }
        result[kind] = json!({"file":sdf.text().unwrap_or("").trim(),"file_format":{"name":"sdf","version":{"major":numbers[0],"minor":numbers[1],"patch":numbers.get(2).copied().unwrap_or(0)}}});
    } else {
        return Err("Missing SDF declaration".into());
    }
    result["dependencies"] = json!(
        root.children()
            .filter(|n| n.has_tag_name("depend"))
            .flat_map(|n| n.descendants().filter(|c| c.has_tag_name("uri")))
            .filter_map(|n| n.text())
            .map(|uri| json!({"uri":uri.trim()}))
            .collect::<Vec<_>>()
    );
    result["authors"]=json!(root.children().filter(|n|n.has_tag_name("author")).map(|n|json!({"name":n.children().find(|c|c.has_tag_name("name")).and_then(|c|c.text()).unwrap_or(""),"email":n.children().find(|c|c.has_tag_name("email")).and_then(|c|c.text()).unwrap_or("")})).collect::<Vec<_>>());
    Ok(result)
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
fn pbtxt_text(v: &Value) -> String {
    let q = |s: &str| serde_json::to_string(s).unwrap();
    let kind = if v.get("world").is_some() {
        "world"
    } else {
        "model"
    };
    let mut out = format!(
        "{kind} {{\n  file: {}\n  file_format {{ name: \"sdf\" version {{ major: {} minor: {} patch: {} }} }}\n}}\nname: {}\ndescription: {}\nversion: {}\n",
        q(s(&v[kind], "file")),
        v[kind]["file_format"]["version"]["major"]
            .as_u64()
            .unwrap_or(1),
        v[kind]["file_format"]["version"]["minor"]
            .as_u64()
            .unwrap_or(6),
        v[kind]["file_format"]["version"]["patch"]
            .as_u64()
            .unwrap_or(0),
        q(s(v, "name")),
        q(s(v, "description")),
        v["version"].as_i64().unwrap_or(0)
    );
    for a in v["authors"].as_array().unwrap_or(&vec![]) {
        out.push_str(&format!(
            "authors {{ name: {} email: {} }}\n",
            q(s(a, "name")),
            q(s(a, "email"))
        ));
    }
    for dep in v["dependencies"].as_array().unwrap_or(&vec![]) {
        out.push_str(&format!("dependencies {{ uri: {} }}\n", q(s(dep, "uri"))));
    }
    out
}
/// Fetch a listing page while preserving transport, encoding and schema errors.
pub(crate) struct ListingPage {
    pub items: Value,
    pub total: Option<usize>,
    pub has_next: Option<bool>,
}
impl ListingPage {
    pub(crate) fn complete(&self, received: usize, total: Option<usize>) -> Result<bool, String> {
        let empty = self.items.as_array().is_some_and(Vec::is_empty);
        if total.is_some_and(|total| {
            received > total
                || ((empty || self.has_next == Some(false)) && received < total)
                || (self.has_next == Some(true) && received == total)
        }) {
            return Err("Fuel listing disagrees with its advertised total count".into());
        }
        if empty && self.has_next == Some(true) {
            return Err("Fuel listing returned an empty page with a next link".into());
        }
        Ok(empty || self.has_next == Some(false) || total == Some(received))
    }
}
pub fn list_page(a: &Value) -> Result<Value, String> {
    list_page_with_metadata(a).map(|page| page.items)
}
// Split Link field lists/parameters only outside URI references and quoted
// strings. A quoted title can legally contain commas, semicolons and escapes.
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
fn link_has_next(value: &str) -> Result<bool, String> {
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
pub(crate) fn list_page_with_metadata(a: &Value) -> Result<ListingPage, String> {
    let mut headers = a["headers"].as_array().cloned().unwrap_or_default();
    headers.push(json!("Accept: application/json"));
    let headers = call(
        "client.headers",
        &json!({"headers":headers,"id":{"server":a["server"]}}),
    );
    let response = crate::io::transport(
        &json!({"method":0,"url":a["server"]["url"],"version":a["server"]["version"],"path":s(a,"path").replace('\\',"/"),"path_encoded":a["path_encoded"],"query":[format!("page={}",a["page"].as_u64().unwrap_or(1))],"headers":headers,"data":[],"form":[],"user_agent":a["user_agent"]}),
    );
    if let Some(error) = response.error {
        return Err(format!("Fuel listing request failed: {error}"));
    }
    if response.status != 200 {
        return Err(format!("Fuel listing returned HTTP {}", response.status));
    }
    let text =
        std::str::from_utf8(&response.data).map_err(|e| format!("Invalid UTF-8 listing: {e}"))?;
    let mut records: Value =
        serde_json::from_str(text).map_err(|e| format!("Invalid JSON listing: {e}"))?;
    let parts: Vec<_> = s(a, "path").split('/').filter(|p| !p.is_empty()).collect();
    let detail = parts.len() == 3 && matches!(parts[1], "models" | "worlds");
    if records.is_object() && detail {
        records = json!([records]);
    }
    let items = records.as_array().ok_or("Fuel listing is not an array")?;
    if items
        .iter()
        .any(|item| !item.is_object() || item["name"].as_str().is_none())
    {
        return Err("Fuel listing contains an invalid resource record".into());
    }
    let total = response
        .headers
        .get("x-total-count")
        .and_then(|value| value.parse().ok());
    let has_next = response
        .headers
        .get("link")
        .map(|links| link_has_next(links))
        .transpose()?;
    let items = call(
        "json_parse",
        &json!({"text":records.to_string(),"kind":a["kind"],"server":a["server"],"many":true}),
    );
    Ok(ListingPage {
        items,
        total,
        has_next,
    })
}
fn page(a: &Value) -> Value {
    list_page(a).unwrap_or_else(|_| json!([]))
}

fn listing(a: &Value) -> Result<Value, String> {
    let mut out = vec![];
    let mut seen = BTreeSet::new();
    let mut args = a.clone();
    let mut total = None;
    for n in 1..=10000 {
        args["page"] = json!(n);
        let page = list_page_with_metadata(&args)?;
        let ids = page
            .items
            .as_array()
            .ok_or("Fuel listing is not an array")?;
        total = page.total.or(total);
        let fingerprint = serde_json::to_string(ids).unwrap_or_default();
        if !seen.insert(fingerprint) {
            return Err("Fuel server repeated a listing page".into());
        }
        out.extend(ids.iter().cloned());
        // Fuel responds with 404 beyond its final page. Honor its advertised
        // boundary instead of probing past it or treating arbitrary errors as EOF.
        if page.complete(out.len(), total)? {
            return Ok(json!(out));
        }
    }
    Err("Fuel listing exceeded the page limit".into())
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
            let mut srv = call("server_default", &json!({}));
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
                &json!({"server":server,"path":path,"kind":kind,"user_agent":config["user_agent"],"headers":args["headers"]}),
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
                            &json!({"server":parsed["id"]["server"],"path":path,"kind":sub,"user_agent":config["user_agent"],"headers":args["headers"]}),
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
            .and_then(|s| parse_config(&s))
        {
            Ok(v) => json!({"ok":true,"output":pbtxt_text(&v)}),
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
        let pb = pbtxt_text(&v);
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
        fs::write(model.join("model.sdf"), "<sdf/>").unwrap();
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
        writer.write_all(b"<sdf/>").unwrap();
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
            (
                "// comment\nname: 'literal é' /* block\ncomment */ # trailing\n",
                "literal é",
            ),
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
        assert!(parse_pbtxt(&nested).unwrap_err().contains("64"));
        let ts = tokens(&format!("{}{}", "nested {".repeat(64), "}".repeat(64))).unwrap();
        assert!(message(&ts, &mut 0, 0).is_ok());
    }
}
