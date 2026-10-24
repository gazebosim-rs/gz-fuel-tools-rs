// SPDX-License-Identifier: Apache-2.0
use std::process::Command;
#[test]
fn standalone_cli_status_and_output() {
    for args in [
        vec!["--versions"],
        vec!["fuel", "--help"],
        vec!["fuel", "configure", "--console", "--defaults"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_gz-fuel"))
            .args(&args)
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}");
        assert!(!out.stdout.is_empty());
    }
    let out = Command::new(env!("CARGO_BIN_EXE_gz-fuel"))
        .args(["download", "-u", "invalid"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8(out.stdout).unwrap().contains("URL"));
}

#[test]
fn configure_honors_cache_environment_and_explicit_relative_file() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join("cache");
    let output = Command::new(env!("CARGO_BIN_EXE_gz-fuel"))
        .args(["configure", "--defaults"])
        .env("GZ_FUEL_CACHE_PATH", &cache)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let config: serde_json::Value =
        serde_yaml::from_str(&std::fs::read_to_string(cache.join("config.yaml")).unwrap()).unwrap();
    assert_eq!(config["cache"]["path"], cache.to_str().unwrap());
    let relative = Command::new(env!("CARGO_BIN_EXE_gz-fuel"))
        .args([
            "configure",
            "--config=settings.yaml",
            "--url=https://example.org",
        ])
        .env("GZ_FUEL_CACHE_PATH", &cache)
        .current_dir(temp.path())
        .output()
        .unwrap();
    assert!(relative.status.success());
    assert!(temp.path().join("settings.yaml").is_file());
    let repeat = Command::new(env!("CARGO_BIN_EXE_gz-fuel"))
        .args(["configure", "--config=settings.yaml"])
        .env("GZ_FUEL_CACHE_PATH", &cache)
        .current_dir(temp.path())
        .output()
        .unwrap();
    assert!(!repeat.status.success());
    assert!(String::from_utf8_lossy(&repeat.stdout).contains("--defaults"));
}
