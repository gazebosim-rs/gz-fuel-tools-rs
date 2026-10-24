#!/usr/bin/env python3
# Copyright (C) 2026 Open Source Robotics Foundation
# SPDX-License-Identifier: Apache-2.0
"""Validate the native Rust distribution's file list and generated crate archive."""

import argparse
from pathlib import Path, PurePosixPath
import tarfile
import tomllib


ROOT_FILES = frozenset({
    ".cargo_vcs_info.json", "Cargo.toml", "Cargo.toml.orig", "Cargo.lock",
    "LICENSE", "NOTICE", "README.md", "CHANGELOG.md", "RELEASING.md", "CONTRIBUTING.md",
    "SECURITY.md",
})
REQUIRED_FILES = ROOT_FILES | {
    "src/lib.rs", "src/bin/gz-fuel.rs", "tests/cli.rs", "docs/VERIFICATION.md",
}


def check_paths(paths: list[str]) -> None:
    seen = set()
    for name in paths:
        path = PurePosixPath(name)
        if (not name or "\\" in name or path.is_absolute()
                or path.as_posix() != name or ".." in path.parts):
            raise ValueError(f"Noncanonical package path: {name!r}")
        if name in seen:
            raise ValueError(f"Duplicate package path: {name!r}")
        seen.add(name)
        if not (name in ROOT_FILES or name == "tests/cli.rs"
                or (len(path.parts) > 1 and path.parts[0] in {"src", "docs"})):
            raise ValueError(f"Unexpected package path: {name!r}")
    missing = sorted(REQUIRED_FILES - seen)
    if missing:
        raise ValueError(f"Required package files missing: {missing!r}")


def check_crate(crate: Path, prefix: str) -> list[str]:
    paths = []
    with tarfile.open(crate, "r:gz") as archive:
        for member in archive.getmembers():
            if not member.isfile():
                raise ValueError(f"Crate member is not a regular file: {member.name!r}")
            if not member.name.startswith(prefix + "/"):
                raise ValueError(f"Unexpected crate prefix: {member.name!r}")
            paths.append(member.name[len(prefix) + 1:])
    check_paths(paths)
    return paths


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    inputs = parser.add_mutually_exclusive_group(required=True)
    inputs.add_argument("--list", type=Path, help="Output of cargo package --list --locked")
    inputs.add_argument("--crate", type=Path, help="Generated .crate file")
    parser.add_argument("--expected-list", type=Path, help="Require archive contents to match this Cargo file list")
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    arguments = parser.parse_args()
    if arguments.expected_list is not None and arguments.crate is None:
        parser.error("--expected-list requires --crate")
    try:
        if arguments.list is not None:
            files = arguments.list.read_text().splitlines()
            check_paths(files)
        else:
            manifest = tomllib.loads((arguments.root / "Cargo.toml").read_text())
            package = manifest["package"]
            files = check_crate(arguments.crate, f"{package['name']}-{package['version']}")
            if arguments.expected_list is not None:
                expected = arguments.expected_list.read_text().splitlines()
                check_paths(expected)
                if set(files) != set(expected):
                    raise ValueError("Built crate contents do not match cargo package --list")
    except (OSError, ValueError, KeyError, tarfile.TarError) as error:
        parser.exit(1, f"Package contents check failed: {error}\n")
    print(f"Package contents verified: {len(files)} native Rust distribution files")
