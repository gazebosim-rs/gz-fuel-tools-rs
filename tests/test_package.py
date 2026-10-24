# Copyright (C) 2026 Open Source Robotics Foundation
# SPDX-License-Identifier: Apache-2.0
"""Guard the crate against ignored checkout files and unsafe archive contents."""

import importlib.util
import io
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("check_package", ROOT / "scripts/check_package.py")
CHECKER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECKER)


class PackageContentsTests(unittest.TestCase):
    def test_unintended_paths_are_rejected(self):
        for extra in (
            "upstream-tests/README.md", "upstream-tests/LICENSE",
            "upstream-tests/example/README.md", ".ci/install/LICENSE",
            "src/../upstream-tests/LICENSE", "/LICENSE", "src//unexpected.rs",
        ):
            with self.subTest(extra=extra), self.assertRaises(ValueError):
                CHECKER.check_paths([*CHECKER.REQUIRED_FILES, extra])

    def test_missing_and_duplicate_files_are_rejected(self):
        files = sorted(CHECKER.REQUIRED_FILES)
        for required in ("LICENSE", "NOTICE"):
            with self.subTest(required=required), self.assertRaisesRegex(ValueError, "missing"):
                CHECKER.check_paths([name for name in files if name != required])
        with self.assertRaisesRegex(ValueError, "Duplicate"):
            CHECKER.check_paths([*files, "LICENSE"])

    def test_crate_links_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            crate = Path(temporary) / "test.crate"
            with tarfile.open(crate, "w:gz") as archive:
                for name in sorted(CHECKER.REQUIRED_FILES):
                    member = tarfile.TarInfo("test/" + name)
                    archive.addfile(member, io.BytesIO(b""))
                link = tarfile.TarInfo("test/src/link.rs")
                link.type = tarfile.SYMTYPE
                link.linkname = "../../upstream-tests/LICENSE"
                archive.addfile(link)
            with self.assertRaisesRegex(ValueError, "not a regular file"):
                CHECKER.check_crate(crate, "test")

    def test_cargo_ignores_nested_upstream_checkout(self):
        def run(arguments, directory):
            result = subprocess.run(arguments, cwd=directory, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            return result.stdout

        def commit(directory):
            run(["git", "init", "--quiet"], directory)
            run(["git", "add", "."], directory)
            run(["git", "-c", "user.name=Package Regression", "-c", "user.email=test@localhost",
                 "-c", "core.hooksPath=/dev/null", "commit", "--quiet", "-m", "Fixture"], directory)

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for name in sorted(CHECKER.ROOT_FILES - {".cargo_vcs_info.json", "Cargo.toml.orig"}):
                shutil.copyfile(ROOT / name, root / name)
            for directory in ("src", "docs"):
                shutil.copytree(ROOT / directory, root / directory)
            (root / "tests").mkdir()
            shutil.copyfile(ROOT / "tests/cli.rs", root / "tests/cli.rs")
            (root / ".gitignore").write_text("/upstream-tests/\n")
            commit(root)
            upstream = root / "upstream-tests"
            for name in ("README.md", "CONTRIBUTING.md", "LICENSE", "example/README.md",
                         "test/gtest_vendor/LICENSE", "test/gtest_vendor/include/gtest/internal/custom/README.md"):
                target = upstream / name
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text("Ignored upstream checkout fixture\n")
            commit(upstream)
            self.assertEqual(run(["git", "status", "--porcelain"], root), "")
            listing = run(["cargo", "package", "--list", "--locked", "--offline"], root)
            CHECKER.check_paths(listing.splitlines())
            self.assertNotIn("upstream-tests/", listing)


if __name__ == "__main__":
    unittest.main()
