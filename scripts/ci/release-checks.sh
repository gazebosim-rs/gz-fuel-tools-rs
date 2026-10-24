#!/usr/bin/env bash
# Copyright (C) 2026 Open Source Robotics Foundation
# SPDX-License-Identifier: Apache-2.0
# Validate candidate artifacts; never create tags, GitHub releases or publications.
set -euo pipefail
cd "$(dirname "$0")/../.."
root="$PWD"
if [[ -n "$(git status --porcelain --untracked-files=normal)" ]]; then
  echo "Release checks require a clean checkout so artifacts describe exactly HEAD." >&2
  exit 1
fi
python3 scripts/check_versions.py
work="$(mktemp -d "${TMPDIR:-/tmp}/gz-fuel-release.XXXXXX")"
trap 'rm -rf -- "$work"' EXIT
output="${FUEL_RELEASE_OUTPUT_DIR:-$root/build/release-checks}"
jobs="${FUEL_BUILD_JOBS:-2}"
libdir="${FUEL_RELEASE_LIBDIR:-lib}"
version="$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["package"]["version"])')"
major="${version%%.*}"
name="gz-fuel-tools-$version"
check_dispatcher() {
  local prefix="$1"
  local actual
  actual="$(ruby -ryaml -e '
    manifest = YAML.load_file(ARGV.fetch(0))
    plugin = manifest.fetch("library_path")
    abort "Dispatcher manifest has an incorrect library_path: #{plugin}" unless plugin == ARGV.fetch(1)
    require plugin
    Cmd.new.execute(["--versions"])
  ' "$prefix/share/gz/fuel${major}.yaml" "$prefix/$libdir/ruby/gz/cmdfuel${major}")"
  test "$actual" = "${version/-pre./~pre}"
}
mkdir -p "$work/source" "$output"
git archive --format=tar --prefix="$name/" HEAD | gzip -n > "$work/$name-source.tar.gz"
tar -xzf "$work/$name-source.tar.gz" -C "$work/source" --strip-components=1
python3 "$work/source/scripts/check_versions.py"

# Cargo verifies a freshly extracted crate and records the clean commit in its metadata.
cargo package --list --locked > "$work/package-files.txt"
python3 scripts/check_package.py --list "$work/package-files.txt"
cargo package --locked --target-dir "$work/cargo"
python3 scripts/check_package.py --crate "$work/cargo/package/$name.crate" --expected-list "$work/package-files.txt"
cmake -S "$work/source" -B "$work/build" \
  -DBUILD_TESTING=OFF -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_INSTALL_PREFIX="$work/install" -DCMAKE_INSTALL_LIBDIR="$libdir"
cmake --build "$work/build" --parallel "$jobs"
cmake --install "$work/build"
for document in LICENSE NOTICE; do
  cmp "$work/source/$document" "$work/install/share/licenses/gz-fuel_tools/$document"
done
check_dispatcher "$work/install"
mv "$work/install" "$work/relocated"
export PKG_CONFIG_PATH="$work/relocated/$libdir/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export LD_LIBRARY_PATH="$work/relocated/$libdir${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
test "$("$work/relocated/bin/gz-fuel" --versions)" = "${version/-pre./~pre}"
"$work/relocated/bin/gz-fuel" --help > /dev/null
cp -R tests/install_consumer "$work/consumer"
cmake -S "$work/consumer" -B "$work/consumer-build" \
  -DCMAKE_PREFIX_PATH="$work/relocated" -DEXPECTED_MAJOR="$major" -DEXPECTED_PREFIX="$work/relocated"
cmake --build "$work/consumer-build" --parallel "$jobs"
ctest --test-dir "$work/consumer-build" --output-on-failure
python3 - "$work/consumer/main.cc" "$work/consumer-build/consumer_pkgconfig" "$major" <<'PY'
import os
import shlex
import subprocess
import sys
flags = subprocess.check_output(["pkg-config", "--cflags", "--libs", "gz-fuel_tools"], text=True)
compiler = shlex.split(os.environ.get("CXX", "c++"))
subprocess.run([*compiler, "-std=c++17", f"-DEXPECTED_MAJOR={sys.argv[3]}", sys.argv[1], "-o", sys.argv[2], *shlex.split(flags)], check=True)
PY
"$work/consumer-build/consumer_pkgconfig"
# Canonicalize pkg-config paths to catch accidental references to the old prefix.
python3 - "$work/relocated" "$libdir" <<'PY'
from pathlib import Path
import subprocess
import sys
prefix = Path(sys.argv[1]).resolve()
for field, expected in (("prefix", prefix), ("libdir", prefix / sys.argv[2]), ("includedir", prefix / "include")):
    value = subprocess.check_output(["pkg-config", f"--variable={field}", "gz-fuel_tools"], text=True).strip()
    if Path(value).resolve() != expected.resolve():
        raise SystemExit(f"Relocated pkg-config {field}: {value!r}, expected {str(expected)!r}")
PY
# gz-tools requires an absolute library_path. Refresh its manifest after moving
# the prefix, after proving library/CLI/pkg-config relocation without reinstalling.
cmake --install "$work/build" --prefix "$work/relocated"
check_dispatcher "$work/relocated"
cp "$work/$name-source.tar.gz" "$work/cargo/package/$name.crate" "$output/"
git rev-parse HEAD > "$output/COMMIT"
(
  cd "$output"
  sha256sum "$name-source.tar.gz" "$name.crate" > SHA256SUMS
)
echo "Release preparation passed for $(git rev-parse HEAD). Candidate artifacts: $output"
