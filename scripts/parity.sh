#!/usr/bin/env bash
# Copyright (C) 2026 Open Source Robotics Foundation
# SPDX-License-Identifier: Apache-2.0
set -euo pipefail
cd "$(dirname "$0")/.."
python3 scripts/check_headers.py
cmake -S . -B build -DCMAKE_BUILD_TYPE=RelWithDebInfo -DFUEL_CPP_COVERAGE=ON -DUPSTREAM_SOURCE="${FUEL_UPSTREAM_SOURCE:-$PWD/../gz-fuel-tools}"
cmake --build build -j "${FUEL_BUILD_JOBS:-4}"
find build -name '*.gcda' -delete
ctest --test-dir build --output-on-failure
mkdir -p coverage
python3 scripts/cpp_coverage.py build | tee coverage/cpp-summary.txt
