#!/usr/bin/env python3
"""Reject drift between Rust package and Gazebo compatibility version strings."""

import argparse
from pathlib import Path
import re
import tomllib


def check(root: Path) -> None:
    manifest = tomllib.loads((root / "Cargo.toml").read_text())
    version = manifest["package"]["version"]
    match = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)(?:-pre\.(\d+))?", version)
    if match is None:
        raise ValueError("Cargo version must be MAJOR.MINOR.PATCH or MAJOR.MINOR.PATCH-pre.N")
    major, minor, patch, prerelease = match.groups()
    base = f"{major}.{minor}.{patch}"
    full = base + (f"~pre{prerelease}" if prerelease is not None else "")

    def expect(path: str, pattern: str, expected: str, count: int = 1) -> None:
        values = re.findall(pattern, (root / path).read_text())
        if len(values) != count or any(value != expected for value in values):
            raise ValueError(f"{path}: expected {count} occurrence(s) of {expected!r}; found {values!r}")

    expect("CMakeLists.txt", r"project\(gz-fuel_tools VERSION ([^ ]+) LANGUAGES", base)
    expect("CMakeLists.txt", r'set\(PROJECT_VERSION_FULL "([^"]+)"\)', full)
    expect("CMakeLists.txt", r"SOVERSION ([^ ]+) VERSION", major)
    expect("CMakeLists.txt", r"SOVERSION [^ ]+ VERSION ([^ ]+) LIBRARY_OUTPUT_DIRECTORY", base)
    expect("CMakeLists.txt", r'cmdfuel(\d+)\.rb', major, count=2)
    expect("cmake/install-gz.cmake.in", r"fuel(\d+)\.yaml", major)
    expect("cmake/install-gz.cmake.in", r"cmdfuel(\d+)", major)
    expect("src/cli.rs", r'pub const VERSION: &str = "([^"]+)";', full)
    expect("src/cli.rs", r'--force-version=([^"\s]+)', full)
    expect("src/core.rs", r'GazeboFuelTools-([^"\s]+)', full, count=2)
    expect("src/command.rs", r'FuelTools ([^"\s]+)', full)
    expect("cpp/Command.cc", r'gzVersion\(\)\s*\{\s*return strdup\("([^"]+)"\);\s*\}', full)
    lock = tomllib.loads((root / "Cargo.lock").read_text())
    locked = [item["version"] for item in lock["package"] if item["name"] == manifest["package"]["name"]]
    if locked != [version]:
        raise ValueError(f"Cargo.lock: expected package version {version!r}; found {locked!r}")
    print(f"Version agreement: crate {version}; C++/CLI {full}; ABI major {major}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    arguments = parser.parse_args()
    try:
        check(arguments.root)
    except (OSError, ValueError, KeyError) as error:
        parser.exit(1, f"Version check failed: {error}\n")
