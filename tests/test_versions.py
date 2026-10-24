# Copyright (C) 2026 Open Source Robotics Foundation
# SPDX-License-Identifier: Apache-2.0
"""Regressions for stale versions that could produce inconsistent releases."""

import importlib.util
from pathlib import Path
import shutil
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("check_versions", ROOT / "scripts/check_versions.py")
CHECKER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECKER)
VERSION_FILES = (
    "Cargo.toml", "Cargo.lock", "CMakeLists.txt", "cmake/install-gz.cmake.in",
    "src/cli.rs", "src/core.rs", "src/command.rs", "cpp/Command.cc",
)


class VersionAgreementTests(unittest.TestCase):
    def test_current_versions_agree(self):
        CHECKER.check(ROOT)

    def test_stale_versions_are_rejected(self):
        mutations = (
            ("Cargo.lock", 'name = "gz-fuel-tools"\nversion = "', 'name = "gz-fuel-tools"\nversion = "999'),
            ("CMakeLists.txt", "SOVERSION ", "SOVERSION 999"),
            ("CMakeLists.txt", 'set(PROJECT_VERSION_FULL "', 'set(PROJECT_VERSION_FULL "999'),
            ("cmake/install-gz.cmake.in", "cmdfuel", "cmdfuel999"),
            ("src/cli.rs", 'pub const VERSION: &str = "', 'pub const VERSION: &str = "999'),
            ("src/core.rs", "GazeboFuelTools-", "GazeboFuelTools-999"),
            ("src/command.rs", "FuelTools ", "FuelTools 999"),
            ("cpp/Command.cc", 'strdup("', 'strdup("999'),
        )
        for filename, original, replacement in mutations:
            with self.subTest(filename=filename, original=original), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                for source in VERSION_FILES:
                    destination = root / source
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copyfile(ROOT / source, destination)
                target = root / filename
                text = target.read_text()
                self.assertIn(original, text)
                target.write_text(text.replace(original, replacement, 1))
                with self.assertRaisesRegex(ValueError, filename):
                    CHECKER.check(root)


if __name__ == "__main__":
    unittest.main()
