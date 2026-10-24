#!/usr/bin/env python3
"""Measure all production C++ facade lines, including inline bridge helpers."""
from pathlib import Path
import json
import subprocess
import shutil
import sys

root = Path(__file__).resolve().parents[1]
build = root / (sys.argv[1] if len(sys.argv) > 1 else "build-cpp-coverage")
records = {}
compiler = (build / "CMakeFiles").glob("*/CMakeCXXCompiler.cmake")
import re
compiler_text = next(compiler).read_text()
major = re.search(r'set\(CMAKE_CXX_COMPILER_VERSION "(\d+)', compiler_text).group(1)
gcov = shutil.which(f"gcov-{major}") or "gcov"
notes = list((build / "CMakeFiles/gz-fuel_tools.dir/cpp").glob("*.gcno"))
compiled = {path.name.removesuffix(".gcno") for path in notes}
expected = {path.name for path in (root / "cpp").glob("*.cc")}
if compiled != expected:
    raise SystemExit(f"C++ coverage source mismatch: missing={expected-compiled}, unexpected={compiled-expected}")
for path in notes:
    report = subprocess.run([gcov, "--json-format", "--stdout", str(path)], check=True, capture_output=True, text=True)
    for item in json.loads(report.stdout)["files"]:
        source = Path(item["file"]).resolve()
        if not source.is_relative_to(root / "cpp"):
            continue
        data = records.setdefault(source, {})
        for line in item["lines"]:
            data[line["line_number"]] = data.get(line["line_number"], 0) + line["count"]
covered = total = 0
for source, lines in sorted(records.items()):
    if not lines:
        continue
    hits = sum(n > 0 for n in lines.values())
    covered += hits
    total += len(lines)
    print(f"{source.relative_to(root)}: {hits}/{len(lines)} ({100*hits/len(lines):.2f}%)")
    zero = [str(n) for n, hit in lines.items() if not hit]
    if zero:
        print("  Uncovered: " + ", ".join(zero))
if not total:
    raise SystemExit("No C++ coverage found")
print(f"Production C++ facade coverage: {covered}/{total} ({100*covered/total:.4f}%)")
if covered / total <= 0.95:
    raise SystemExit("FAIL: C++ facade line coverage must be greater than 95%")
