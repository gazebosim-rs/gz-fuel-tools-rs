// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
//! Schema-complete Fuel protobuf metadata and bounded resource validation.
use crate::{Error, ErrorKind, ResourceKind, ResourceLimits};
use prost_reflect::{DescriptorPool, DynamicMessage};
use std::{
    collections::HashMap,
    fs,
    io::Read,
    path::{Component, Path},
    sync::OnceLock,
};

#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct Version {
    #[prost(int32, tag = "1")]
    pub major: i32,
    #[prost(int32, tag = "2")]
    pub minor: i32,
    #[prost(int32, tag = "3")]
    pub patch: i32,
    #[prost(string, tag = "4")]
    pub prerelease: String,
    #[prost(string, tag = "5")]
    pub build: String,
}
#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct VersionRange {
    #[prost(message, optional, tag = "1")]
    pub min: Option<Version>,
    #[prost(message, optional, tag = "2")]
    pub max: Option<Version>,
}
#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct FileFormat {
    #[prost(message, optional, tag = "1")]
    pub version: Option<Version>,
    #[prost(string, tag = "2")]
    pub name: String,
}
#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct ResourceFile {
    #[prost(string, tag = "1")]
    pub file: String,
    #[prost(message, optional, tag = "2")]
    pub file_format: Option<FileFormat>,
}
#[derive(Clone, PartialEq, prost::Oneof)]
pub(crate) enum ResourceType {
    #[prost(message, tag = "1")]
    Model(ResourceFile),
    #[prost(message, tag = "2")]
    World(ResourceFile),
}
#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct Contact {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(string, tag = "2")]
    pub email: String,
}
#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct Legal {
    #[prost(string, tag = "1")]
    pub copyright: String,
    #[prost(string, tag = "2")]
    pub license: String,
}
#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct Dependency {
    #[prost(string, tag = "1")]
    pub uri: String,
}
#[derive(Clone, PartialEq, prost::Oneof)]
pub(crate) enum CompatibilityVersion {
    #[prost(message, tag = "2")]
    Version(Version),
    #[prost(message, tag = "3")]
    Range(VersionRange),
}
#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct Compatibility {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(oneof = "CompatibilityVersion", tags = "2,3")]
    pub version: Option<CompatibilityVersion>,
}
#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct Categories {
    #[prost(string, tag = "1")]
    pub first: String,
    #[prost(string, tag = "2")]
    pub second: String,
}
#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct Metadata {
    #[prost(oneof = "ResourceType", tags = "1,2")]
    pub resource: Option<ResourceType>,
    #[prost(string, tag = "3")]
    pub name: String,
    #[prost(string, tag = "4")]
    pub description: String,
    #[prost(int32, tag = "5")]
    pub version: i32,
    #[prost(message, repeated, tag = "6")]
    pub authors: Vec<Contact>,
    #[prost(message, optional, tag = "7")]
    pub legal: Option<Legal>,
    #[prost(string, repeated, tag = "8")]
    pub tags: Vec<String>,
    #[prost(map = "string,string", tag = "9")]
    pub annotations: HashMap<String, String>,
    #[prost(message, repeated, tag = "10")]
    pub dependencies: Vec<Dependency>,
    #[prost(message, repeated, tag = "11")]
    pub compatibilities: Vec<Compatibility>,
    #[prost(message, optional, tag = "12")]
    pub categories: Option<Categories>,
}
fn descriptor() -> prost_reflect::MessageDescriptor {
    static POOL: OnceLock<DescriptorPool> = OnceLock::new();
    POOL.get_or_init(|| {
        DescriptorPool::decode(include_bytes!("metadata/descriptor.pb").as_slice())
            .expect("checked-in Fuel schema descriptor is valid")
    })
    .get_message_by_name("gz.msgs.FuelMetadata")
    .expect("Fuel metadata descriptor exists")
}
fn invalid(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::Metadata, message)
}
pub(crate) fn parse_pbtxt(text: &str) -> Result<Metadata, Error> {
    parse_pbtxt_with_limit(text, ResourceLimits::default().max_metadata_bytes)
}
fn parse_pbtxt_with_limit(text: &str, limit: u64) -> Result<Metadata, Error> {
    if text.len() as u64 > limit {
        return Err(Error::new(
            ErrorKind::Limit,
            "Fuel metadata exceeds byte limit",
        ));
    }
    DynamicMessage::parse_text_format(descriptor(), text)
        .map_err(|e| invalid(format!("Invalid Fuel protobuf metadata: {e}")))?
        .transcode_to()
        .map_err(|e| Error::with_source(ErrorKind::Metadata, "Unable to decode Fuel metadata", e))
}
impl Metadata {
    pub(crate) fn resource_file(&self) -> Option<&ResourceFile> {
        match self.resource.as_ref()? {
            ResourceType::Model(file) | ResourceType::World(file) => Some(file),
        }
    }
    pub(crate) fn kind(&self) -> Option<ResourceKind> {
        match self.resource.as_ref()? {
            ResourceType::Model(_) => Some(ResourceKind::Model),
            ResourceType::World(_) => Some(ResourceKind::World),
        }
    }
    pub(crate) fn license(&self) -> &str {
        self.legal
            .as_ref()
            .map_or("", |legal| legal.license.as_str())
    }
    pub(crate) fn pbtxt(&self) -> String {
        {
            let mut message = DynamicMessage::new(descriptor());
            message
                .transcode_from(self)
                .expect("typed metadata matches checked-in descriptor");
            message.to_text_format_with_options(
                &prost_reflect::text_format::FormatOptions::new().pretty(true),
            )
        }
    }
}
pub(crate) fn read_text(path: &Path, limit: u64) -> Result<String, Error> {
    let file = fs::File::open(path)?;
    if file.metadata()?.len() > limit {
        return Err(Error::new(
            ErrorKind::Limit,
            "Resource metadata exceeds byte limit",
        ));
    }
    let mut text = String::new();
    file.take(limit.saturating_add(1))
        .read_to_string(&mut text)?;
    if text.len() as u64 > limit {
        return Err(Error::new(
            ErrorKind::Limit,
            "Resource metadata exceeds byte limit",
        ));
    }
    Ok(text)
}
pub(crate) fn read_with_limits(path: &Path, limits: &ResourceLimits) -> Result<Metadata, Error> {
    if path.join("metadata.pbtxt").is_file() {
        parse_pbtxt_with_limit(
            &read_text(&path.join("metadata.pbtxt"), limits.max_metadata_bytes)?,
            limits.max_metadata_bytes,
        )
    } else {
        parse_config(&read_text(
            &path.join("model.config"),
            limits.max_metadata_bytes,
        )?)
    }
}
pub(crate) fn parse_config(text: &str) -> Result<Metadata, Error> {
    let doc = roxmltree::Document::parse(text.trim_start())
        .map_err(|e| invalid(format!("Invalid model configuration XML: {e}")))?;
    let root = doc.root_element();
    let kind = match root.tag_name().name() {
        "model" => ResourceKind::Model,
        "world" => ResourceKind::World,
        _ => return Err(invalid("Expected model/world configuration")),
    };
    let text = |name: &str| {
        root.children()
            .find(|n| n.has_tag_name(name))
            .and_then(|n| n.text())
            .unwrap_or("")
            .trim()
            .to_owned()
    };
    let name = text("name");
    if name.is_empty() {
        return Err(invalid("Missing resource name"));
    }
    let mut files = root
        .children()
        .filter(|n| n.has_tag_name("sdf"))
        .map(|node| {
            let value = node.attribute("version").unwrap_or("");
            let (numbers, build) = value.split_once('+').unwrap_or((value, ""));
            let (numbers, prerelease) = numbers.split_once('-').unwrap_or((numbers, ""));
            let parts = numbers
                .split('.')
                .map(str::parse::<i32>)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| invalid("Invalid SDF version"))?;
            if !(2..=3).contains(&parts.len())
                || parts.iter().any(|n| *n < 0)
                || node.text().unwrap_or("").trim().is_empty()
            {
                return Err(invalid("Invalid SDF declaration"));
            }
            Ok(ResourceFile {
                file: node.text().unwrap_or("").trim().into(),
                file_format: Some(FileFormat {
                    name: "sdf".into(),
                    version: Some(Version {
                        major: parts[0],
                        minor: parts[1],
                        patch: parts.get(2).copied().unwrap_or(0),
                        prerelease: prerelease.into(),
                        build: build.into(),
                    }),
                }),
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    files.sort_by_key(|file| {
        let v = file.file_format.as_ref().unwrap().version.as_ref().unwrap();
        (v.major, v.minor, v.patch, v.prerelease.is_empty())
    });
    let file = files
        .pop()
        .ok_or_else(|| invalid("Missing SDF declaration"))?;
    let version = text("version")
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-' || *c == '+')
        .collect::<String>();
    let mut metadata = Metadata {
        name,
        description: text("description"),
        version: if version.is_empty() {
            0
        } else {
            version
                .parse()
                .map_err(|_| invalid("Invalid resource version"))?
        },
        resource: Some(if kind == ResourceKind::Model {
            ResourceType::Model(file)
        } else {
            ResourceType::World(file)
        }),
        ..Metadata::default()
    };
    let license = text("license");
    if !license.is_empty() {
        metadata.legal = Some(Legal {
            license,
            ..Legal::default()
        });
    }
    metadata.dependencies = root
        .children()
        .filter(|n| n.has_tag_name("depend"))
        .flat_map(|n| n.descendants().filter(|n| n.has_tag_name("uri")))
        .filter_map(|n| n.text())
        .map(|uri| Dependency {
            uri: uri.trim().into(),
        })
        .collect();
    metadata.authors = root
        .children()
        .filter(|n| n.has_tag_name("author"))
        .map(|node| Contact {
            name: node
                .children()
                .find(|n| n.has_tag_name("name"))
                .and_then(|n| n.text())
                .unwrap_or("")
                .trim()
                .into(),
            email: node
                .children()
                .find(|n| n.has_tag_name("email"))
                .and_then(|n| n.text())
                .unwrap_or("")
                .trim()
                .into(),
        })
        .collect();
    Ok(metadata)
}
pub(crate) fn validate(
    path: &Path,
    kind: ResourceKind,
    limits: &ResourceLimits,
) -> Result<Metadata, Error> {
    let metadata = read_with_limits(path, limits)?;
    if metadata.name.trim().is_empty() || metadata.kind() != Some(kind) {
        return Err(invalid(
            "Missing resource name or mismatched metadata resource kind",
        ));
    }
    let file = metadata
        .resource_file()
        .ok_or_else(|| invalid("Missing resource file"))?;
    let relative = Path::new(&file.file);
    if file.file.is_empty()
        || file.file.contains(['\\', '\0'])
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(invalid("Invalid main SDF path"));
    }
    let mut current = path.to_path_buf();
    for part in relative.components() {
        current.push(part);
        if fs::symlink_metadata(&current)?.file_type().is_symlink() {
            return Err(invalid("Symbolic link in main SDF path"));
        }
    }
    let text = read_text(&current, limits.max_metadata_bytes)?;
    let xml = roxmltree::Document::parse(text.trim_start())
        .map_err(|e| invalid(format!("Invalid main SDF XML: {e}")))?;
    let root = xml.root_element();
    let expected = if kind == ResourceKind::Model {
        "model"
    } else {
        "world"
    };
    if root.tag_name().name() != "sdf"
        || !root.children().any(|node| {
            (node.has_tag_name(expected)
                || (kind == ResourceKind::Model && node.has_tag_name("actor")))
                && node.attribute("name").is_some_and(|name| !name.is_empty())
        })
    {
        return Err(invalid("Main SDF lacks its declared model/world resource"));
    }
    Ok(metadata)
}
/// Convert only at the legacy JSON adapter boundary.
pub(crate) fn compatibility_value(metadata: &Metadata) -> serde_json::Value {
    use serde_json::json;
    let mut value = json!({"name":metadata.name,"description":metadata.description,"version":metadata.version,"license":metadata.license(),"tags":metadata.tags,"annotations":metadata.annotations,"authors":metadata.authors.iter().map(|a|json!({"name":a.name,"email":a.email})).collect::<Vec<_>>(),"dependencies":metadata.dependencies.iter().map(|a|json!({"uri":a.uri})).collect::<Vec<_>>(),"compatibilities":[],"categories":metadata.categories.iter().flat_map(|c|[&c.first,&c.second]).filter(|s|!s.is_empty()).collect::<Vec<_>>()});
    if let Some(file) = metadata.resource_file() {
        let mut data = json!({"file":file.file});
        if let Some(format) = &file.file_format {
            data["file_format"] = json!({"name":format.name});
            if let Some(version) = &format.version {
                data["file_format"]["version"] = json!({"major":version.major,"minor":version.minor,"patch":version.patch,"prerelease":version.prerelease,"build":version.build});
            }
        }
        value[if metadata.kind() == Some(ResourceKind::World) {
            "world"
        } else {
            "model"
        }] = data;
    }
    if let Some(legal) = &metadata.legal {
        value["legal"] = json!({"license":legal.license,"copyright":legal.copyright});
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn custom_metadata_allowance_can_exceed_the_default() {
        let folder = tempfile::tempdir().unwrap();
        let text = format!(
            "#{}\nname:\"large\"",
            "x".repeat(ResourceLimits::default().max_metadata_bytes as usize)
        );
        fs::write(folder.path().join("metadata.pbtxt"), &text).unwrap();
        assert_eq!(parse_pbtxt(&text).unwrap_err().kind, ErrorKind::Limit);
        let limits = ResourceLimits {
            max_metadata_bytes: text.len() as u64,
            ..ResourceLimits::default()
        };
        assert_eq!(
            read_with_limits(folder.path(), &limits).unwrap().name,
            "large"
        );
    }
    #[test]
    fn full_schema_lists_maps_oneofs_and_integer_bounds_round_trip() {
        let source = r#"model { file:"model.sdf" file_format { name:"sdf" version {major:1 minor:9 patch:2 prerelease:"rc1" build:"local"} } }
name:"example" description:"é😀" version:2147483647
legal {copyright:"OSRF" license:"Apache-2.0"}
authors:[{name:"A" email:"a@example.com"},{name:"B"}]
tags:["robot","simulation"] annotations {key:"arbitrary" value:"data"}
dependencies:[{uri:"https://host/1.0/a/models/b"},{uri:"model://local"}]
compatibilities:[{name:"gazebo" version {major:11}},{name:"bullet" version_range {min {major:3} max {major:4}}}]
categories {first:"robot" second:"industrial"}"#;
        let metadata = parse_pbtxt(source).unwrap();
        assert_eq!(metadata.version, i32::MAX);
        assert_eq!(metadata.authors.len(), 2);
        assert_eq!(metadata.compatibilities.len(), 2);
        assert_eq!(metadata.annotations["arbitrary"], "data");
        assert_eq!(parse_pbtxt(&metadata.pbtxt()).unwrap(), metadata);
        for source in [
            "version:2147483648",
            "model {file_format {version {major:\"broken\"}}}",
            "model {file_format {unknown:1}}",
            "compatibilities{version{} version_range{}}",
            "name:\"a\" name:\"b\"",
            "model{} world{}",
            "tags:[1]",
        ] {
            assert!(parse_pbtxt(source).is_err(), "{source}");
        }
        assert_eq!(
            parse_pbtxt("version:-2147483648").unwrap().version,
            i32::MIN
        );
    }
    #[test]
    fn configuration_versions_legal_dependencies_and_world_descriptor() {
        let parsed=parse_config("<world><name>world</name><version>-2.0</version><sdf version='1.9.2-rc1+build'>world.sdf</sdf><license>MIT</license><depend><model><uri>model://local</uri></model></depend><author><name> A </name><email>a@b</email></author></world>").unwrap();
        assert_eq!(parsed.version, -2);
        assert_eq!(parsed.kind(), Some(ResourceKind::World));
        assert_eq!(parsed.license(), "MIT");
        assert_eq!(parsed.dependencies[0].uri, "model://local");
        assert_eq!(parsed.authors[0].name, "A");
        assert_eq!(
            parsed
                .resource_file()
                .unwrap()
                .file_format
                .as_ref()
                .unwrap()
                .version
                .as_ref()
                .unwrap()
                .prerelease,
            "rc1"
        );
        for declaration in ["1", "1.2.3.4", "1.-1", "1.x"] {
            assert!(
                parse_config(&format!(
                    "<model><name>x</name><sdf version='{declaration}'>x.sdf</sdf></model>"
                ))
                .is_err()
            );
        }
        assert!(parse_config("<model><name>x</name><version>999999999999999999999</version><sdf version='1.9'>x.sdf</sdf></model>").is_err());
    }
    #[test]
    fn validation_rejects_unsafe_main_files_and_accepts_actor_and_world() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let limits = ResourceLimits::default();
        for (resource, kind, content) in [
            (
                "model",
                ResourceKind::Model,
                "<sdf><actor name='actor'/></sdf>",
            ),
            (
                "world",
                ResourceKind::World,
                "<sdf><world name='world'/></sdf>",
            ),
        ] {
            fs::write(
                root.join("metadata.pbtxt"),
                format!("name:\"x\" {resource} {{file:\"main.sdf\"}} "),
            )
            .unwrap();
            fs::write(root.join("main.sdf"), content).unwrap();
            assert!(validate(root, kind, &limits).is_ok());
        }
        for filename in ["", "../outside.sdf", "/absolute.sdf", "a\\\\b.sdf"] {
            fs::write(
                root.join("metadata.pbtxt"),
                format!("name:\"x\" model{{file:{filename:?}}}"),
            )
            .unwrap();
            assert!(validate(root, ResourceKind::Model, &limits).is_err());
        }
        fs::write(
            root.join("metadata.pbtxt"),
            "name:\"x\" model{file:\"main.sdf\"}",
        )
        .unwrap();
        fs::write(root.join("main.sdf"), "<sdf/>").unwrap();
        assert!(validate(root, ResourceKind::Model, &limits).is_err());
        #[cfg(unix)]
        {
            fs::remove_file(root.join("main.sdf")).unwrap();
            std::os::unix::fs::symlink("outside.sdf", root.join("main.sdf")).unwrap();
            assert!(validate(root, ResourceKind::Model, &limits).is_err());
        }
        assert_eq!(
            read_text(&root.join("metadata.pbtxt"), 1).unwrap_err().kind,
            ErrorKind::Limit
        );
    }
}
