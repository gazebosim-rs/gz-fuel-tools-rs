#!/usr/bin/env bash
# GitHub's required gate is code too; lint it with a checksum-pinned tool.
set -euo pipefail
cd "$(dirname "$0")/../.."
if [[ "$(uname -s)-$(uname -m)" != Linux-x86_64 ]]; then
  echo 'This tool bootstrap supports the validated Linux x86_64 CI host.' >&2
  exit 1
fi
tool_dir="$(mktemp -d)"
trap 'rm -rf "$tool_dir"' EXIT
curl --fail --silent --show-error --location --retry 3 \
  https://github.com/rhysd/actionlint/releases/download/v1.7.12/actionlint_1.7.12_linux_amd64.tar.gz \
  --output "$tool_dir/actionlint.tar.gz"
printf '%s  %s\n' \
  8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8 \
  "$tool_dir/actionlint.tar.gz" | sha256sum --check --status
tar -xzf "$tool_dir/actionlint.tar.gz" -C "$tool_dir" actionlint
"$tool_dir/actionlint" -color
