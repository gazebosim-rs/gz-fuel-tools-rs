# Copyright (C) 2026 Open Source Robotics Foundation
# SPDX-License-Identifier: Apache-2.0
"""Require a reproducible Fuel schema and independently specified protobuf corpus."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("metadata_schema", Path(__file__).resolve().parents[1] / "scripts/check_metadata_schema.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class MetadataSchema(unittest.TestCase):
    def test_embedded_descriptor_and_upstream_text_corpus(self):
        module.verify()
