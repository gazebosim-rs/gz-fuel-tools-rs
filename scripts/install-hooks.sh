#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ "$(git rev-parse --show-toplevel)" != "$PWD" ]]; then
  echo 'Install hooks from the gz-fuel-tools-rs repository root.' >&2
  exit 1
fi
git config core.hooksPath .githooks
