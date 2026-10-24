#!/usr/bin/env python3
"""Compare consumer declarations against the pinned upstream source tree."""
import os
from pathlib import Path
import sys
root = Path(__file__).resolve().parents[1]
upstream = Path(os.environ.get("FUEL_UPSTREAM_SOURCE", root.parent / "gz-fuel-tools"))
headers = sorted((upstream / "include/gz/fuel_tools").glob("*.hh"))
if not headers:
    raise SystemExit("Upstream public headers were not found")
for source in headers:
    target = root / "include/gz/fuel_tools" / source.name
    if source.read_bytes() != target.read_bytes():
        raise SystemExit(f"Public C++ header differs from pinned upstream: {source.name}")
print(f"All {len(headers)} upstream public C++ headers match exactly.")
