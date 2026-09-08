"""Exercise the required merge gate with Actions job conclusions."""

import importlib.util
import json
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("ci_results", Path(__file__).resolve().parents[1] / "check_ci_results.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class CiResultsTests(unittest.TestCase):
    def test_successful_prerequisites(self):
        self.assertTrue(module.succeeded(json.dumps({"format": {"result": "success"}, "test": {"result": "success", "outputs": {}}})))

    def test_failed_skipped_cancelled_and_missing_prerequisites_block_merge(self):
        for result in ("failure", "cancelled", "skipped", "neutral", "", None):
            with self.subTest(result=result):
                self.assertFalse(module.succeeded(json.dumps({"format": {"result": "success"}, "test": {"result": result}})))

    def test_empty_malformed_or_wrong_shape_is_not_success(self):
        for raw in ("", "{", "{}", "[]", "null", '{"test":null}', '{"test":{}}', '{"test":"success"}'):
            with self.subTest(raw=raw):
                self.assertFalse(module.succeeded(raw))


if __name__ == "__main__":
    unittest.main()
