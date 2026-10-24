// SPDX-License-Identifier: Apache-2.0
//! Rust command-line argument adapter, shared by the standalone and gz frontends.
use serde_json::{Value, json};
use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
};
/// Gazebo-compatible CLI version spelling, including its prerelease suffix.
pub const VERSION: &str = "12.0.0~pre2";
const HELP: &str = "Gazebo Fuel resource tools\nUsage: gz-fuel [fuel] <action> [options]\nActions: list download upload delete edit update meta configure\nOptions: -u/--url URL -t/--type model|world -o/--owner OWNER -c/--config FILE\n         --header HEADER --raw -j/--jobs N -m/--model PATH --private --public\n         --force-version VERSION --versions -h/--help\nConfigure: --defaults permits overwrite; --console prints YAML; --cache DIR\n";
/// Parse and execute one invocation. Zero means success; failures are nonzero.
pub fn run(input: &[String]) -> (i32, String) {
    let mut args: Vec<String> = input
        .iter()
        .flat_map(|item| {
            if item.starts_with("--")
                && let Some((key, value)) = item.split_once('=')
            {
                return vec![key.into(), value.into()];
            }
            vec![item.clone()]
        })
        .collect();
    if args.first().is_some_and(|x| x == "fuel") {
        args.remove(0);
    }
    while let Some(index) = args.iter().position(|x| x == "--force-version") {
        if index + 1 >= args.len() {
            return (1, "Missing value for --force-version\n".into());
        }
        let requested = args.remove(index + 1);
        args.remove(index);
        if requested != VERSION {
            return (
                1,
                format!("Requested Fuel version {requested} is unavailable\n"),
            );
        }
    }
    if args.iter().any(|x| x == "--versions" || x == "--version") {
        return (0, format!("{VERSION}\n"));
    }
    if args.is_empty() || args.iter().any(|x| x == "--help" || x == "-h") {
        return (0, HELP.into());
    }
    let action = args.remove(0);
    let mut a = json!({"action":action,"jobs":2});
    let mut n = 0;
    while n < args.len() {
        let item = &args[n];
        let key = match item.as_str() {
            "-u" | "--url" => "url",
            "-t" | "--type" => "kind",
            "-o" | "--owner" => "owner",
            "-c" | "--config" => "config_file",
            "--cache" => "cache_location",
            "--header" => "header",
            "-j" | "--jobs" => "jobs",
            "-m" | "--model" | "--path" => "path",
            "--force-version" => "version",
            "--config2pbtxt" => "config2pbtxt",
            "--pbtxt2config" => "pbtxt2config",
            "-v" | "--verbose" => {
                let value = args.get(n + 1).filter(|value| !value.starts_with('-'));
                a["verbosity"] = json!(value.map(String::as_str).unwrap_or("3"));
                n += if value.is_some() { 2 } else { 1 };
                continue;
            }
            "-r" | "--raw" => {
                a["raw"] = json!("true");
                n += 1;
                continue;
            }
            "-p" | "--private" => {
                a["private"] = json!("1");
                n += 1;
                continue;
            }
            "-b" | "--public" => {
                a["private"] = json!("0");
                n += 1;
                continue;
            }
            "--onlymodels" | "--only-models" => {
                a["only_models"] = json!("1");
                n += 1;
                continue;
            }
            "--onlyworlds" | "--only-worlds" => {
                a["only_worlds"] = json!("1");
                n += 1;
                continue;
            }
            "--console" | "--defaults" => {
                a[item.trim_start_matches('-')] = json!(true);
                n += 1;
                continue;
            }
            _ => return (1, format!("Unknown option: {item}\n")),
        };
        n += 1;
        if key == "jobs" && (n == args.len() || args[n].starts_with('-')) {
            a["jobs"] = json!(1);
            continue;
        }
        if n == args.len() {
            return (1, format!("Missing value for {item}\n"));
        }
        if key == "jobs" {
            let Ok(jobs) = args[n].parse::<usize>() else {
                return (1, "Invalid job count\n".into());
            };
            if jobs == 0 || jobs > 64 {
                return (1, "Job count must be between 1 and 64\n".into());
            }
            a[key] = json!(jobs);
        } else {
            a[key] = json!(args[n]);
        }
        n += 1;
    }
    if action == "configure" {
        return configure(&a);
    }

    if action == "meta" {
        let (op, key) = if a.get("config2pbtxt").is_some() {
            ("command.config_to_pbtxt", "config2pbtxt")
        } else {
            ("command.pbtxt_to_config", "pbtxt2config")
        };
        let v = crate::execute(op, &json!({"path":a[key]}))
            .unwrap_or_else(|e| json!({"ok":false,"output":e}));
        return output(v);
    }
    if action == "list" {
        a["kind"] = match a["kind"].as_str() {
            None => return (1, "Missing resource type\n".into()),
            Some("model" | "models") => json!("models"),
            Some("world" | "worlds") => json!("worlds"),
            _ => return (1, "Invalid resource type\n".into()),
        };
    }
    if !["list", "download", "upload", "delete", "edit", "update"].contains(&action.as_str()) {
        return (1, format!("Unknown action: {action}\n{HELP}"));
    }
    output(crate::command::run(&a))
}
fn configure(a: &Value) -> (i32, String) {
    let text = |key: &str| a[key].as_str().unwrap_or("");
    let cache = if !text("cache_location").is_empty() {
        PathBuf::from(text("cache_location"))
    } else if let Some(cache) = env::var("GZ_FUEL_CACHE_PATH")
        .ok()
        .filter(|v| !v.is_empty())
    {
        PathBuf::from(cache)
    } else {
        let home = env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap_or_default();
        PathBuf::from(home).join(".gz/fuel")
    };
    let url = if text("url").is_empty() {
        "https://fuel.gazebosim.org"
    } else {
        text("url")
    };
    if url::Url::parse(url)
        .ok()
        .is_none_or(|u| !matches!(u.scheme(), "http" | "https") || u.host_str().is_none())
    {
        return (1, "Invalid Fuel server URL\n".into());
    }
    let token = text("header")
        .split_once(':')
        .filter(|(key, _)| key.trim().eq_ignore_ascii_case("private-token"))
        .map(|(_, token)| token.trim())
        .unwrap_or("");
    fn serialize_token<S: serde::Serializer>(
        token: &str,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        // The upstream console format spells the empty token as two single
        // quotes. Let the serializer escape every nonempty value normally.
        if token.is_empty() {
            serde::Serialize::serialize(&serde_saphyr::SingleQuoted(token), serializer)
        } else {
            serde::Serialize::serialize(token, serializer)
        }
    }
    #[derive(serde::Serialize)]
    struct SavedServer<'a> {
        name: &'a str,
        url: &'a str,
        #[serde(rename = "private-token", serialize_with = "serialize_token")]
        token: &'a str,
    }
    #[derive(serde::Serialize)]
    struct SavedCache<'a> {
        path: &'a Path,
    }
    #[derive(serde::Serialize)]
    struct SavedConfig<'a> {
        servers: [SavedServer<'a>; 1],
        cache: SavedCache<'a>,
    }
    let configuration = SavedConfig {
        servers: [SavedServer {
            name: "Fuel",
            url,
            token,
        }],
        cache: SavedCache { path: &cache },
    };
    let yaml = format!(
        "---\n{}",
        serde_saphyr::to_string(&configuration).expect("string configuration serializes to YAML")
    );
    if a["console"] == true {
        return (0, yaml);
    }
    let path = if text("config_file").is_empty() {
        cache.join("config.yaml")
    } else {
        PathBuf::from(text("config_file"))
    };
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let result = (|| -> std::io::Result<()> {
        fs::create_dir_all(parent)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(yaml.as_bytes())?;
        if a["defaults"] == true {
            temporary.persist(&path).map_err(|error| error.error)?;
        } else {
            temporary
                .persist_noclobber(&path)
                .map_err(|error| error.error)?;
        }
        Ok(())
    })();
    match result {
        Ok(()) => (0, format!("Settings saved to {}.\n", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (
            1,
            format!(
                "Configuration {} already exists; use --defaults to overwrite.\n",
                path.display()
            ),
        ),
        Err(error) => (
            1,
            format!("Unable to save configuration {}: {error}\n", path.display()),
        ),
    }
}

fn output(v: Value) -> (i32, String) {
    (
        if v["ok"] == true { 0 } else { 1 },
        v["output"].as_str().unwrap_or("").to_owned(),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    fn invoke(args: &[&str]) -> (i32, String) {
        run(&args.iter().map(|s| (*s).into()).collect::<Vec<_>>())
    }
    #[test]
    fn arguments_and_statuses() {
        for args in [
            vec![],
            vec!["fuel", "--help"],
            vec!["--versions"],
            vec!["--version"],
            vec!["configure", "--console", "--defaults"],
        ] {
            assert_eq!(invoke(&args).0, 0);
        }
        for args in [
            vec!["bad"],
            vec!["list"],
            vec!["list", "-t", "other"],
            vec!["list", "-u"],
            vec!["list", "--weird"],
            vec!["download", "-j", "0"],
            vec!["download", "-j", "invalid"],
            vec!["download", "--jobs", "65"],
            vec!["list", "-t", "model", "-u", "fake"],
            vec!["list", "-t", "world", "-u", "fake"],
            vec!["meta", "--config2pbtxt", "/absent"],
            vec!["meta", "--pbtxt2config", "/absent"],
            vec![
                "upload",
                "-m",
                "/absent",
                "--private",
                "--owner",
                "alice",
                "--header",
                "token",
                "--config",
                "/absent",
                "--force-version",
                VERSION,
                "-v",
                "4",
                "--raw",
            ],
            vec![
                "update",
                "--only-models",
                "--only-worlds",
                "--public",
                "-c",
                "/absent",
            ],
        ] {
            assert_ne!(invoke(&args).0, 0, "{args:?}");
        }
    }
    #[test]
    fn global_version_without_action_shows_help() {
        assert!(
            invoke(&["fuel", "--force-version", VERSION])
                .1
                .contains("list")
        );
        assert_eq!(invoke(&["--force-version"]).0, 1);
        assert_eq!(invoke(&["--force-version", "absent"]).0, 1);
        assert_eq!(invoke(&["--force-version=12.0.0~pre2", "--versions"]).0, 0);
        assert_eq!(
            invoke(&["configure", "--console", "--url=https://example.org"]).0,
            0
        );
        let jobs = invoke(&["download", "-j", "--url", "invalid"]);
        assert_eq!(jobs.0, 1);
        assert!(!jobs.1.contains("job count"));
    }

    #[test]
    fn configure_defaults_console_preserves_upstream_yaml_format() {
        let (status, output) = invoke(&[
            "configure",
            "--console",
            "--defaults",
            "--cache",
            "/tmp/fuel-defaults-cache",
        ]);
        assert_eq!(status, 0);
        assert_eq!(
            output,
            concat!(
                "---\n",
                "servers:\n",
                "- name: Fuel\n",
                "  url: https://fuel.gazebosim.org\n",
                "  private-token: ''\n",
                "cache:\n",
                "  path: /tmp/fuel-defaults-cache\n"
            )
        );
    }

    #[test]
    fn configure_files_console_and_legacy_aliases() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("custom/config.yaml");
        let cache = temp.path().join("resources");
        let args = [
            "configure",
            "--config",
            file.to_str().unwrap(),
            "--cache",
            cache.to_str().unwrap(),
            "--url",
            "https://example.com",
            "--header",
            "Private-Token: test-token",
        ];
        let out = invoke(&args);
        assert_eq!(out.0, 0, "{}", out.1);
        let content = fs::read_to_string(&file).unwrap();
        let parsed: Value = serde_saphyr::from_str(&content).unwrap();
        assert_eq!(parsed["cache"]["path"], cache.to_str().unwrap());
        assert_eq!(parsed["servers"][0]["url"], "https://example.com");
        assert_eq!(parsed["servers"][0]["private-token"], "test-token");
        assert_eq!(invoke(&args).0, 1);
        assert_eq!(fs::read_to_string(&file).unwrap(), content);
        let mut overwrite = args.to_vec();
        overwrite.push("--defaults");
        assert_eq!(invoke(&overwrite).0, 0);
        let mut console = args.to_vec();
        console.push("--console");
        assert_eq!(invoke(&console).1, content);
        let default_cache = temp.path().join("default-cache");
        assert_eq!(
            invoke(&[
                "configure",
                "--cache",
                default_cache.to_str().unwrap(),
                "--defaults"
            ])
            .0,
            0
        );
        assert!(default_cache.join("config.yaml").is_file());
        let blocked = temp.path().join("blocked");
        fs::write(&blocked, "file").unwrap();
        assert_eq!(
            invoke(&[
                "configure",
                "--config",
                blocked.join("config.yaml").to_str().unwrap()
            ])
            .0,
            1
        );
        assert_eq!(
            invoke(&["configure", "--console", "--url", "not-a-url"]).0,
            1
        );
        for args in [
            vec![
                "update",
                "--onlymodels",
                "--onlyworlds",
                "--config",
                "/absent",
            ],
            vec!["edit", "-p", "--config", "/absent"],
            vec!["edit", "-b", "--config", "/absent"],
            vec!["list", "-r", "-v", "-t", "invalid"],
        ] {
            let out = invoke(&args);
            assert_eq!(out.0, 1);
            assert!(!out.1.contains("Unknown option"));
        }
    }
}
