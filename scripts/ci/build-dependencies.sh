#!/usr/bin/env bash
# Copyright (C) 2026 Open Source Robotics Foundation
# SPDX-License-Identifier: Apache-2.0
# Build only the pinned Gazebo libraries required by unchanged C++ parity tests.
set -euo pipefail
cd "$(dirname "$0")/../.."
ci_root="${FUEL_CI_ROOT:-$PWD/.ci}"
ci_prefix="$ci_root/install"
mkdir -p "$ci_root/src" "$ci_root/build" "$ci_prefix"
export CMAKE_PREFIX_PATH="$ci_prefix${CMAKE_PREFIX_PATH:+:$CMAKE_PREFIX_PATH}"
export PKG_CONFIG_PATH="$ci_prefix/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export LD_LIBRARY_PATH="$ci_prefix/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
while read -r repository revision; do
  [[ -z "$repository" || "$repository" == \#* ]] && continue
  source_dir="$ci_root/src/$repository"
  mkdir -p "$source_dir"
  if [[ -n "${FUEL_CI_SOURCE_CACHE:-}" && -d "$FUEL_CI_SOURCE_CACHE/$repository/.git" ]]; then
    # Local validation can reuse existing Git objects; CI fetches the same SHA.
    git -C "$FUEL_CI_SOURCE_CACHE/$repository" archive "$revision" | tar -xf - -C "$source_dir"
  else
    if [[ ! -d "$source_dir/.git" ]]; then
      git init -q "$source_dir"
      git -C "$source_dir" remote add origin "https://github.com/gazebosim/$repository.git"
    fi
    git -C "$source_dir" fetch --depth 1 origin "$revision"
    git -C "$source_dir" checkout --detach FETCH_HEAD
  fi
  options=(-DBUILD_TESTING=OFF -DCMAKE_BUILD_TYPE=Release
    "-DCMAKE_INSTALL_PREFIX=$ci_prefix" -DCMAKE_INSTALL_LIBDIR=lib)
  case "$repository" in
    gz-utils) options+=(-DGZ_UTILS_VENDOR_CLI11=ON) ;;
    gz-math) options+=(-DSKIP_SWIG=ON -DSKIP_PYBIND11=ON) ;;
    gz-common) options+=(-DSKIP_av=ON -DSKIP_events=ON -DSKIP_graphics=ON
      -DSKIP_geospatial=ON -DSKIP_io=ON -DSKIP_profiler=ON) ;;
  esac
  cmake -S "$source_dir" -B "$ci_root/build/$repository" "${options[@]}"
  cmake --build "$ci_root/build/$repository" --parallel "${FUEL_BUILD_JOBS:-2}"
  cmake --install "$ci_root/build/$repository"
done < scripts/ci/dependencies.lock
