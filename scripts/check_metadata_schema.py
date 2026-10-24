#!/usr/bin/env python3
# Copyright (C) 2026 Open Source Robotics Foundation
# SPDX-License-Identifier: Apache-2.0
"""Verify the embedded schema and differential protobuf text metadata corpus."""
import argparse
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
SCHEMA = ROOT / "src/metadata"
BASE = 'model { file: "model.sdf" file_format {name:"sdf" version {major:1 minor:9}} } name:"example" '
CORPUS = [
    (BASE + 'tags:["one","two"] authors:[{name:"a"},{name:"b"}]', True),
    (BASE + 'annotations {key:"a" value:"v"} compatibilities {name:"gz" version_range {min {major:1} max {major:9}}}', True),
    (BASE + 'version:0x7fffffff', True),
    (BASE + 'description:"\\u00e9\\U0001F600"', True),
    ('model {file:"x" file_format {version {major:"broken"}}}', False),
    ('model {file_format {unknown:"x"}}', False),
    ('compatibilities {name:"x" version {} version_range {}}', False),
    ('name:"a" name:"b"', False),
    ('version:2147483648', False),
    ('// unsupported comment\n' + BASE, False),
]


def protoc(text: str) -> subprocess.CompletedProcess:
    return subprocess.run(["protoc", "-I", str(SCHEMA), "--encode=gz.msgs.FuelMetadata", str(SCHEMA / "gz/msgs/fuel_metadata.proto")], input=text.encode(), capture_output=True, check=False)


def verify(binary: Path | None = None) -> None:
    with tempfile.TemporaryDirectory(prefix="fuel-metadata-schema.") as temporary:
        folder = Path(temporary)
        descriptor = folder / "descriptor.pb"
        subprocess.run(["protoc", "-I", str(SCHEMA), "--include_imports", "--descriptor_set_out=" + str(descriptor), str(SCHEMA / "gz/msgs/fuel_metadata.proto")], check=True)
        if descriptor.read_bytes() != (SCHEMA / "descriptor.pb").read_bytes():
            raise ValueError("Embedded Fuel descriptor differs from the retained schemas")
        for index, (text, accepted) in enumerate(CORPUS):
            upstream = protoc(text)
            if (upstream.returncode == 0) != accepted:
                raise ValueError(f"Corpus {index} disagrees with protoc: {upstream.stderr.decode()}")
            if binary is not None:
                file = folder / f"case-{index}.pbtxt"
                file.write_text(text)
                actual = subprocess.run([str(binary), "meta", "--pbtxt2config", str(file)], capture_output=True, check=False)
                # Conversion additionally requires a named resource/file; use the
                # complete fixture for successful parser cases only.
                if accepted and actual.returncode != 0:
                    raise ValueError(f"Rust rejected valid corpus {index}: {actual.stdout.decode()}")
                if not accepted and actual.returncode == 0:
                    raise ValueError(f"Rust accepted invalid corpus {index}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path)
    args = parser.parse_args()
    verify(args.binary.resolve() if args.binary else None)
    print(f"Fuel metadata descriptor and {len(CORPUS)} protobuf text cases verified")
