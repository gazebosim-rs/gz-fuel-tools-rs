#!/usr/bin/env python3
"""Strict production line coverage; exclude only explicit Rust test modules/files.

All production Rust files, including the FFI and error paths, are measured.
Test code is excluded by its cfg(test) declaration, not by handwritten paths to
production implementation. Coverage must be strictly greater than the threshold.
"""
from pathlib import Path
import re
import sys


def masked_rust(text):
    """Mask literals/comments while retaining positions and newlines."""
    output = list(text)
    index = 0
    while index < len(text):
        end = index
        if text.startswith("//", index):
            end = text.find("\n", index)
            if end < 0:
                end = len(text)
        elif text.startswith("/*", index):
            end, depth = index + 2, 1
            while end < len(text) and depth:
                if text.startswith("/*", end):
                    depth += 1
                    end += 2
                elif text.startswith("*/", end):
                    depth -= 1
                    end += 2
                else:
                    end += 1
        else:
            raw = re.match(r'(?:br|r)(\#*)"', text[index:])
            char = re.match(r"'(?:\\.|[^'\\\n])'", text[index:])
            if raw:
                delimiter = '"' + raw.group(1)
                close = text.find(delimiter, index + raw.end())
                end = len(text) if close < 0 else close + len(delimiter)
            elif char:
                end = index + char.end()
            elif text[index] == '"':
                end = index + 1
                while end < len(text):
                    if text[end] == "\\":
                        end += 2
                    elif text[end] == '"':
                        end += 1
                        break
                    else:
                        end += 1
        if end > index:
            for position in range(index, min(end, len(text))):
                if text[position] != "\n":
                    output[position] = " "
            index = end
        else:
            index += 1
    return "".join(output)


def test_modules(source):
    """Return exact cfg(test) line ranges and external test source paths."""
    text = source.read_text()
    masked = masked_rust(text)
    excluded, external = set(), set()
    pattern = r"#\[cfg\(test\)\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*([;{])"
    for match in re.finditer(pattern, masked):
        start = match.start()
        end = match.end()
        if match.group(2) == "{":
            depth = 1
            while end < len(masked) and depth:
                depth += (masked[end] == "{") - (masked[end] == "}")
                end += 1
            if depth:
                raise ValueError(f"Unbalanced test module in {source}")
        else:
            path = re.search(r'#\[path\s*=\s*"([^"]+)"\]', text[start:end])
            target = path.group(1) if path else match.group(1) + ".rs"
            external.add((source.parent / target).resolve())
        excluded.update(range(text.count("\n", 0, start) + 1, text.count("\n", 0, end) + 2))
    return excluded, external


def main():
    records = {}
    current = None
    root = Path(__file__).resolve().parents[1]
    for line in Path(sys.argv[1]).read_text().splitlines():
        if line.startswith("SF:"):
            current = Path(line[3:]).resolve()
        elif line.startswith("DA:") and current is not None:
            number, hits, *_ = line[3:].split(",")
            records.setdefault(current, {})[int(number)] = int(hits)
    covered = total = 0
    absent = False
    missing = []
    sources = sorted((root / "src").rglob("*.rs"))
    module_info = {source: test_modules(source) for source in sources}
    test_sources = set().union(*(info[1] for info in module_info.values()))
    for source in sources:
        if source.resolve() in test_sources:
            continue
        excluded = module_info[source][0]
        data = records.get(source.resolve(), {})
        data = {n: hits for n, hits in data.items() if n not in excluded}
        if not data:
            print(f"ERROR: no production coverage for {source.relative_to(root)}")
            absent = True
            continue
        hit = sum(n > 0 for n in data.values())
        covered += hit
        total += len(data)
        percent = 100 * hit / len(data)
        print(f"{source.relative_to(root)}: {hit}/{len(data)} ({percent:.2f}%)")
        zero = [str(n) for n, hits in data.items() if hits == 0]
        if zero:
            missing.append(f"{source.relative_to(root)}: {', '.join(zero)}")
    percent = 100 * covered / total if total else 0
    threshold = float(sys.argv[2])
    print(f"Production Rust line coverage: {covered}/{total} ({percent:.4f}%)")
    if missing:
        print("Uncovered production lines:\n" + "\n".join(missing))
    if absent or percent <= threshold:
        print(f"FAIL: coverage must be greater than {threshold}%")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
