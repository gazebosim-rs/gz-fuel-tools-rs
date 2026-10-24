#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cmake -S . -B build-cpp-coverage -DCMAKE_BUILD_TYPE=Debug -DFUEL_CPP_COVERAGE=ON
cmake --build build-cpp-coverage -j "${FUEL_BUILD_JOBS:-4}"
find build-cpp-coverage -name '*.gcda' -delete
ctest --test-dir build-cpp-coverage --output-on-failure
python3 scripts/cpp_coverage.py
