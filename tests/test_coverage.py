"""Regression tests for production/test coverage separation."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("coverage_gate", Path(__file__).resolve().parents[1] / "scripts/check_coverage.py")
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)


class CoverageSources(unittest.TestCase):
    def test_literals_do_not_hide_later_production(self):
        text = '''fn before() {}\n#[cfg(test)]\nmod tests {\nlet x = r#"}}}}}"#;\nlet y = "{";\nlet c = '}';\n/* } /* { */ } */\n}\nfn after() {}\n'''
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "lib.rs"
            source.write_text(text)
            excluded, external = gate.test_modules(source)
        self.assertEqual(excluded, set(range(2, 9)))
        self.assertFalse(external)

    def test_external_sources_come_only_from_cfg_test(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "lib.rs"
            source.write_text('#[cfg(test)]\n#[path = "special.rs"]\npub(crate) mod tests;\nfn production() {}\n')
            excluded, external = gate.test_modules(source)
            self.assertEqual(excluded, {1, 2, 3})
            self.assertEqual(external, {Path(directory) / "special.rs"})


if __name__ == "__main__":
    unittest.main()
