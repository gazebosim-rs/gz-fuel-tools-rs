// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
//! Cache publication and hostile archive regression tests.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

fn setup(path: &Path) -> (ClientConfig, ResourceId) {
    let server = ServerConfig {
        url: "https://example.test".into(),
        ..ServerConfig::default()
    };
    (
        ClientConfig {
            cache_location: path.into(),
            servers: vec![server.clone()],
            ..ClientConfig::default()
        },
        ResourceId {
            owner: "alice".into(),
            name: "box".into(),
            version: 1,
            server,
            ..ResourceId::default()
        },
    )
}
fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, contents) in entries {
        archive
            .start_file(
                *name,
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
        archive.write_all(contents).unwrap();
    }
    archive.finish().unwrap().into_inner()
}
fn model(generation: &str) -> Vec<u8> {
    zip(&[
        (
            "model.config",
            b"<model><name>box</name><sdf version='1.10'>model.sdf</sdf></model>",
        ),
        (
            "model.sdf",
            b"<sdf version='1.10'><model name='box'/></sdf>",
        ),
        ("generation-a", generation.as_bytes()),
        ("generation-b", generation.as_bytes()),
    ])
}

#[test]
fn invalid_metadata_never_replaces_a_usable_cache_entry() {
    let root = tempfile::tempdir().unwrap();
    let (config, id) = setup(root.path());
    let saved = save_resource(
        &config,
        ResourceKind::Model,
        &id,
        Cursor::new(model("original")),
        false,
    )
    .unwrap();
    for metadata in [
        "<model><sdf version='1.10'>model.sdf</sdf></model>",
        "<model><name>box</name><sdf>model.sdf</sdf></model>",
        "<world><name>box</name><sdf version='1.10'>model.sdf</sdf></world>",
    ] {
        let archive = zip(&[
            ("model.config", metadata.as_bytes()),
            ("model.sdf", b"<sdf><model name='box'/></sdf>"),
        ]);
        assert!(
            save_resource(
                &config,
                ResourceKind::Model,
                &id,
                Cursor::new(archive),
                true
            )
            .is_err()
        );
        assert_eq!(
            fs::read_to_string(saved.path.join("generation-a")).unwrap(),
            "original"
        );
    }
    let archive = zip(&[
        (
            "model.config",
            b"<model><name>box</name><sdf version='1.10'>model.sdf</sdf></model>",
        ),
        ("model.sdf", b"<unrelated/>"),
    ]);
    assert!(
        save_resource(
            &config,
            ResourceKind::Model,
            &id,
            Cursor::new(archive),
            true
        )
        .is_err()
    );
    assert_eq!(
        fs::read_to_string(saved.path.join("generation-a")).unwrap(),
        "original"
    );
    assert!(
        !fs::read_dir(saved.path.parent().unwrap())
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".fuel-backup"))
    );
}

#[test]
fn canonical_cache_identity_and_generated_urls_preserve_escaped_names() {
    let root = tempfile::tempdir().unwrap();
    let (config, mut id) = setup(root.path());
    id.owner = "Alice".into();
    id.name = "É?%2f#".into();
    let archive = zip(&[("model.config", b"<model><name>box</name><sdf version='1.10'>model.sdf</sdf></model>"),
        ("model.sdf", b"<sdf><model name='box'><link><visual><geometry><mesh><uri>model://box/mesh%20file.dae</uri></mesh></geometry></visual></link></model></sdf>")]);
    let saved = save_resource(
        &config,
        ResourceKind::Model,
        &id,
        Cursor::new(archive),
        false,
    )
    .unwrap();
    assert_eq!(saved.id.owner, "alice");
    assert_eq!(saved.id.name, "É?%2f#");
    let found = lookup_resource(&config, ResourceKind::Model, &id)
        .unwrap()
        .unwrap();
    assert_eq!(found.path, saved.path);
    let text = fs::read_to_string(saved.path.join("model.sdf")).unwrap();
    let document = roxmltree::Document::parse(&text).unwrap();
    let uri = document
        .descendants()
        .find(|node| node.has_tag_name("uri"))
        .unwrap()
        .text()
        .unwrap();
    let parsed = crate::identity::parse(uri, &config, ResourceKind::Model, true).unwrap();
    assert_eq!(parsed.id.name, saved.id.name);
    assert_eq!(parsed.file.unwrap(), PathBuf::from("mesh file.dae"));
    let enumerated = cached_resources(&config, ResourceKind::Model, None).unwrap();
    assert_eq!(enumerated.len(), 1);
    assert_eq!(enumerated[0].id.name, id.name);
    for name in ["../escape", "a/b", "a\\b", "", ".", ".."] {
        id.name = name.into();
        assert!(lookup_resource(&config, ResourceKind::Model, &id).is_err());
    }
}

#[test]
fn archive_budgets_reject_count_depth_and_expansion_before_publication() {
    let root = tempfile::tempdir().unwrap();
    let (mut config, id) = setup(root.path());
    let payload = model("original");
    let saved = save_resource(
        &config,
        ResourceKind::Model,
        &id,
        Cursor::new(&payload),
        false,
    )
    .unwrap();
    for limits in [
        ResourceLimits {
            max_download_bytes: 1,
            ..ResourceLimits::default()
        },
        ResourceLimits {
            max_archive_entries: 1,
            ..ResourceLimits::default()
        },
        ResourceLimits {
            max_archive_entry_bytes: 1,
            ..ResourceLimits::default()
        },
        ResourceLimits {
            max_archive_total_bytes: 1,
            ..ResourceLimits::default()
        },
        ResourceLimits {
            max_metadata_bytes: 1,
            ..ResourceLimits::default()
        },
    ] {
        config.limits = limits;
        let error = save_resource(
            &config,
            ResourceKind::Model,
            &id,
            Cursor::new(&payload),
            true,
        )
        .unwrap_err();
        assert_eq!(error.kind, crate::ErrorKind::Limit, "{error:?}");
        assert_eq!(
            fs::read_to_string(saved.path.join("generation-a")).unwrap(),
            "original"
        );
    }
    config.limits = ResourceLimits {
        max_archive_depth: 2,
        ..ResourceLimits::default()
    };
    let payload = zip(&[("a/b/c", b"deep")]);
    let destination = root.path().join("deep");
    assert!(extract_with_limits(Cursor::new(payload), &destination, &config.limits).is_err());
    assert!(!destination.exists());
    config.limits = ResourceLimits {
        max_archive_entry_bytes: 1024,
        ..ResourceLimits::default()
    };
    let payload = zip(&[("large", &vec![0; 2 * 1024 * 1024])]);
    assert!(payload.len() < 4096);
    assert!(extract_with_limits(Cursor::new(payload), &destination, &config.limits).is_err());
    assert!(!destination.exists());
}

#[cfg(unix)]
#[test]
fn compression_propagates_unreadable_directory_errors() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let nested = source.join("unreadable");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("important"), "retained").unwrap();
    fs::set_permissions(&nested, fs::Permissions::from_mode(0o0)).unwrap();
    let result = compress(&source, &root.path().join("out.zip"));
    let was_unreadable = fs::read_dir(&nested).is_err();
    fs::set_permissions(&nested, fs::Permissions::from_mode(0o700)).unwrap();
    // Elevated/root test runners can read mode-000 directories. On ordinary CI
    // users the actual I/O error must propagate; no data is silently omitted.
    if was_unreadable {
        assert!(result.is_err());
        assert!(!root.path().join("out.zip").exists());
    }
    assert!(read_children(&root.path().join("missing")).is_err());
}

#[test]
fn explicit_cache_lookup_does_not_enumerate_unrelated_resources() {
    let root = tempfile::tempdir().unwrap();
    let (config, id) = setup(root.path());
    let saved = save_resource(
        &config,
        ResourceKind::Model,
        &id,
        Cursor::new(model("known")),
        false,
    )
    .unwrap();
    let owner_root = saved.path.ancestors().nth(3).unwrap();
    // An unreadable/unrelated subtree cannot affect an exact path lookup.
    fs::write(owner_root.join("unrelated"), "not a directory").unwrap();
    assert_eq!(
        lookup_resource(&config, ResourceKind::Model, &id)
            .unwrap()
            .unwrap()
            .path,
        saved.path
    );
    let mut tip = id.clone();
    tip.version = 0;
    assert_eq!(
        lookup_resource(&config, ResourceKind::Model, &tip)
            .unwrap()
            .unwrap()
            .id
            .version,
        1
    );
    tip.version = 2;
    assert!(
        lookup_resource(&config, ResourceKind::Model, &tip)
            .unwrap()
            .is_none()
    );
}

#[cfg(target_os = "linux")]
#[test]
fn cache_writer_process() {
    let Some(path) = std::env::var_os("FUEL_TEST_WRITER_CACHE") else {
        return;
    };
    let (config, id) = setup(Path::new(&path));
    for generation in 0..32 {
        let marker = format!("{}-{generation}", std::process::id());
        save_resource(
            &config,
            ResourceKind::Model,
            &id,
            Cursor::new(model(&marker)),
            true,
        )
        .unwrap();
    }
}

#[cfg(target_os = "linux")]
#[test]
fn cross_process_publication_keeps_resources_available_and_reads_consistent() {
    let root = tempfile::tempdir().unwrap();
    let (config, id) = setup(root.path());
    let initial = save_resource(
        &config,
        ResourceKind::Model,
        &id,
        Cursor::new(model("initial")),
        false,
    )
    .unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let reader_stop = stop.clone();
    let path = initial.path.clone();
    let read_config = config.clone();
    let read_id = id.clone();
    let reader = std::thread::spawn(move || {
        let mut reads = 0;
        while !reader_stop.load(Ordering::Acquire) {
            assert!(
                fs::read(path.join("model.sdf")).is_ok(),
                "published resource disappeared"
            );
            with_cached_resource(&read_config, ResourceKind::Model, &read_id, |entry| {
                let first = fs::read(entry.path.join("generation-a"))?;
                std::thread::yield_now();
                let second = fs::read(entry.path.join("generation-b"))?;
                assert_eq!(
                    first, second,
                    "metadata read crossed publication generations"
                );
                Ok(())
            })
            .unwrap()
            .unwrap();
            reads += 1;
        }
        reads
    });
    let mut children = (0..4)
        .map(|_| {
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "io::cache_tests::cache_writer_process",
                    "--nocapture",
                ])
                .env("FUEL_TEST_WRITER_CACHE", root.path())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect::<Vec<_>>();
    let results = children
        .drain(..)
        .map(|child| child.wait_with_output().unwrap())
        .collect::<Vec<_>>();
    stop.store(true, Ordering::Release);
    let reads = reader.join().unwrap();
    assert!(reads > 0);
    for result in results {
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    assert!(
        !fs::read_dir(initial.path.parent().unwrap())
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".fuel-backup"))
    );
}

#[test]
fn footer_counts_are_checked_before_zip_metadata_allocation() {
    let limits = ResourceLimits {
        max_archive_entries: 1,
        ..ResourceLimits::default()
    };
    let mut bytes = zip(&[("a", b"x")]);
    let footer = bytes.len() - 22;
    bytes[footer + 8..footer + 12].copy_from_slice(&[2, 0, 2, 0]);
    let error = archive_directory_budget(&mut Cursor::new(&bytes), bytes.len() as u64, &limits)
        .unwrap_err();
    assert!(error.get_ref().unwrap().is::<LimitExceeded>());
    // A ZIP64 footer can advertise huge raw entry counts in only a few bytes.
    let mut zip64 = vec![0; 56];
    zip64[..4].copy_from_slice(b"PK\x06\x06");
    zip64[4..12].copy_from_slice(&44u64.to_le_bytes());
    zip64[32..40].copy_from_slice(&1000u64.to_le_bytes());
    zip64.extend_from_slice(b"PK\x06\x07");
    zip64.extend_from_slice(&0u32.to_le_bytes());
    zip64.extend_from_slice(&0u64.to_le_bytes());
    zip64.extend_from_slice(&1u32.to_le_bytes());
    let mut footer = [0; 22];
    footer[..4].copy_from_slice(b"PK\x05\x06");
    zip64.extend_from_slice(&footer);
    let error = archive_directory_budget(&mut Cursor::new(&zip64), zip64.len() as u64, &limits)
        .unwrap_err();
    assert!(error.get_ref().unwrap().is::<LimitExceeded>());
    let permissive = ResourceLimits {
        max_archive_entries: 1000,
        ..ResourceLimits::default()
    };
    archive_directory_budget(&mut Cursor::new(&zip64), zip64.len() as u64, &permissive).unwrap();
    zip64[0] = 0;
    assert!(
        archive_directory_budget(&mut Cursor::new(&zip64), zip64.len() as u64, &permissive)
            .is_err()
    );
    assert!(archive_directory_budget(&mut Cursor::new(b"not a ZIP"), 9, &limits).is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn atomic_install_failures_preserve_existing_directories() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("previous"), "good").unwrap();
    assert!(install_stage(&root.path().join("missing"), &destination).is_err());
    assert_eq!(
        fs::read_to_string(destination.join("previous")).unwrap(),
        "good"
    );
    // Exchanging an ancestor with its descendant is invalid; no rename-away
    // fallback may run after the operating system rejects the atomic operation.
    assert_eq!(
        install_stage(root.path(), &destination).unwrap_err().kind(),
        io::ErrorKind::Unsupported
    );
    assert_eq!(
        fs::read_to_string(destination.join("previous")).unwrap(),
        "good"
    );
    let file = root.path().join("file");
    fs::write(&file, "regular").unwrap();
    assert!(install_stage(root.path(), &file).is_err());
    assert_eq!(fs::read_to_string(file).unwrap(), "regular");
}

#[test]
fn invalid_worlds_and_asset_uris_fail_without_replacing_cache() {
    let root = tempfile::tempdir().unwrap();
    let (mut config, mut id) = setup(root.path());
    let original = save_resource(
        &config,
        ResourceKind::World,
        &id,
        Cursor::new(zip(&[("world.sdf", b"<sdf/>")])),
        false,
    )
    .unwrap();
    for contents in [b"<sdf>".as_slice(), b"<model/>", &[255]] {
        assert!(
            save_resource(
                &config,
                ResourceKind::World,
                &id,
                Cursor::new(zip(&[("world.sdf", contents)])),
                true
            )
            .is_err()
        );
        assert_eq!(
            fs::read(original.path.join("world.sdf")).unwrap(),
            b"<sdf/>"
        );
    }
    assert!(lookup_resource(&config, ResourceKind::Collection, &id).is_err());
    assert!(
        cached_resources(&config, ResourceKind::Collection, None)
            .unwrap()
            .is_empty()
    );
    id.name = "unsafe:name".into();
    assert!(lookup_resource(&config, ResourceKind::Model, &id).is_err());
    id.name = "box".into();
    assert_eq!(
        cache_list(&json!({"config":false,"kind":"models"})),
        json!([])
    );
    for suffix in ["%2e%2e/asset", "bad%FF", "bad%5cpath"] {
        let sdf = format!("<sdf><model name='box'><uri>model://box/{suffix}</uri></model></sdf>");
        let archive = zip(&[
            (
                "model.config",
                b"<model><name>box</name><sdf version='1.10'>model.sdf</sdf></model>",
            ),
            ("model.sdf", sdf.as_bytes()),
        ]);
        assert!(
            save_resource(
                &config,
                ResourceKind::Model,
                &id,
                Cursor::new(archive),
                false
            )
            .is_err()
        );
    }
    config.limits.max_metadata_bytes = 90;
    let archive = zip(&[
        (
            "model.config",
            b"<model><name>box</name><sdf version='1.10'>model.sdf</sdf></model>",
        ),
        (
            "model.sdf",
            b"<sdf><model name='box'><uri>model://box/a</uri></model></sdf>",
        ),
    ]);
    let error = save_resource(
        &config,
        ResourceKind::Model,
        &id,
        Cursor::new(archive),
        false,
    )
    .unwrap_err();
    assert_eq!(error.kind, crate::ErrorKind::Limit);
    let metadata = root.path().join("oversize");
    fs::write(&metadata, "too long").unwrap();
    assert!(read_text_limited(&metadata, 1).is_err());
}

#[cfg(unix)]
#[test]
fn malformed_cache_entries_and_non_utf8_directories_are_omitted() {
    use std::os::unix::{
        ffi::OsStringExt,
        fs::{PermissionsExt, symlink},
    };
    let root = tempfile::tempdir().unwrap();
    let (config, mut id) = setup(root.path());
    let parent = resource_parent(&config, ResourceKind::Model, &id).unwrap();
    fs::create_dir_all(&parent).unwrap();
    fs::write(parent.join("1"), "not a directory").unwrap();
    assert!(
        lookup_resource(&config, ResourceKind::Model, &id)
            .unwrap()
            .is_none()
    );
    id.version = 0;
    assert!(
        lookup_resource(&config, ResourceKind::Model, &id)
            .unwrap()
            .is_none()
    );
    id.name = "missing".into();
    assert!(
        lookup_resource(&config, ResourceKind::Model, &id)
            .unwrap()
            .is_none()
    );
    id.name = "box".into();
    let owner_root = config.cache_location.join("example.test");
    fs::create_dir(owner_root.join(std::ffi::OsString::from_vec(vec![255]))).unwrap();
    let models = owner_root.join("alice/models");
    fs::create_dir(models.join(std::ffi::OsString::from_vec(vec![254]))).unwrap();
    fs::create_dir_all(models.join("bad:name/1")).unwrap();
    symlink(&parent, models.join("linked")).unwrap();
    assert!(
        cached_resources(&config, ResourceKind::Model, None)
            .unwrap()
            .is_empty()
    );
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o0)).unwrap();
    if fs::read_dir(&parent).is_err() {
        assert!(lookup_resource(&config, ResourceKind::Model, &id).is_err());
    }
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn special_files_are_not_archived_and_legacy_lock_sidecars_are_ignored() {
    use std::os::unix::ffi::OsStrExt;
    let root = tempfile::tempdir().unwrap();
    let pipe = root.path().join("pipe");
    let name = std::ffi::CString::new(pipe.as_os_str().as_bytes()).unwrap();
    // SAFETY: live NUL-terminated pathname, creating only a private temporary FIFO.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    assert!(compress(&pipe, &root.path().join("out.zip")).is_err());
    fs::rename(&pipe, root.path().join(".fuel-1.lock")).unwrap();
    assert!(resource_lock(root.path(), 1, true).is_ok());
}

#[test]
fn exact_cache_lookup_accepts_legacy_directory_case_without_scanning_resources() {
    let root = tempfile::tempdir().unwrap();
    let (config, mut id) = setup(root.path());
    id.name = "My Model".into();
    let parent = config
        .cache_location
        .join("example.test/Alice/models/My Model");
    fs::create_dir_all(parent.join("2")).unwrap();
    fs::write(parent.join("2/model.config"), "legacy metadata").unwrap();
    id.version = 2;
    let found = lookup_resource(&config, ResourceKind::Model, &id)
        .unwrap()
        .unwrap();
    assert_eq!(found.id.name, "my model");
    assert_eq!(found.id.owner, "alice");
    assert_eq!(found.path, parent.join("2"));
    id.version = 0;
    assert_eq!(
        lookup_resource(&config, ResourceKind::Model, &id)
            .unwrap()
            .unwrap()
            .id
            .version,
        2
    );
    id.version = 3;
    assert!(
        lookup_resource(&config, ResourceKind::Model, &id)
            .unwrap()
            .is_none()
    );
}

#[test]
fn tip_compares_all_legacy_case_spellings_but_explicit_lookup_keeps_direct_path() {
    let root = tempfile::tempdir().unwrap();
    let (config, mut id) = setup(root.path());
    let canonical = save_resource(
        &config,
        ResourceKind::Model,
        &id,
        Cursor::new(model("one")),
        false,
    )
    .unwrap();
    let legacy = config
        .cache_location
        .join("example.test/Alice/models/Box/9");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join("model.config"), "legacy marker").unwrap();
    assert_eq!(
        lookup_resource(&config, ResourceKind::Model, &id)
            .unwrap()
            .unwrap()
            .path,
        canonical.path
    );
    id.version = 0;
    let tip = lookup_resource(&config, ResourceKind::Model, &id)
        .unwrap()
        .unwrap();
    assert_eq!(tip.id.version, 9);
    assert_eq!(tip.path, legacy);
}

#[cfg(target_os = "linux")]
#[test]
fn readonly_legacy_cache_metadata_reads_need_no_sidecar_or_write_permission() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let (config, id) = setup(root.path());
    let saved = save_resource(
        &config,
        ResourceKind::Model,
        &id,
        Cursor::new(model("readable")),
        false,
    )
    .unwrap();
    let parent = saved.path.parent().unwrap();
    fs::set_permissions(parent, fs::Permissions::from_mode(0o555)).unwrap();
    let result = with_cached_resource(&config, ResourceKind::Model, &id, |entry| {
        crate::metadata::read_with_limits(&entry.path, &config.limits).map(|metadata| metadata.name)
    });
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(result.unwrap().unwrap(), "box");
    assert!(!fs::read_dir(parent).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".lock")
    }));
}
