#!/usr/bin/env bash
# Copyright (C) 2026 Open Source Robotics Foundation
# SPDX-License-Identifier: Apache-2.0
set -euo pipefail
cd "$(dirname "$0")/.."
python3 -m unittest discover -s tests -p 'test_*.py'
coverage_dir="$PWD/coverage"
mkdir -p "$coverage_dir/raw"
# A fresh run must never inherit coverage from earlier source revisions.
find "$coverage_dir/raw" -name '*.profraw' -delete
sysroot="$(rustc --print sysroot)"
host="$(rustc -vV | sed -n 's/^host: //p')"
llvm_tools="$sysroot/lib/rustlib/$host/bin"
if [[ ! -x "$llvm_tools/llvm-cov" ]]; then
  echo 'Install matching tools: rustup component add llvm-tools-preview' >&2
  exit 1
fi
export CARGO_TARGET_DIR="$PWD/target/coverage"
export RUSTFLAGS="${RUSTFLAGS:-} -C instrument-coverage"
export LLVM_PROFILE_FILE="$coverage_dir/raw/fuel-%p-%m.profraw"
cargo test --locked --all-targets --message-format=json > "$coverage_dir/cargo.json"
"$llvm_tools/llvm-profdata" merge -sparse "$coverage_dir"/raw/*.profraw -o "$coverage_dir/fuel.profdata"
mapfile -t objects < <(python3 - "$coverage_dir/cargo.json" <<'PY'
import json,sys
for line in open(sys.argv[1]):
 try: item=json.loads(line)
 except ValueError: continue
 if item.get('reason')=='compiler-artifact' and item.get('executable'):
  print(item['executable'])
PY
)
if [[ ${#objects[@]} -eq 0 ]]; then echo 'No instrumented test binaries found' >&2; exit 1; fi
arguments=()
for object in "${objects[@]:1}"; do arguments+=(-object "$object"); done
"$llvm_tools/llvm-cov" export "${objects[0]}" "${arguments[@]}" -instr-profile "$coverage_dir/fuel.profdata" -format=lcov -ignore-filename-regex='(/\.cargo/|/\.rustup/)' > "$coverage_dir/lcov.info"
python3 scripts/check_coverage.py "$coverage_dir/lcov.info" 95 | tee "$coverage_dir/summary.txt"
