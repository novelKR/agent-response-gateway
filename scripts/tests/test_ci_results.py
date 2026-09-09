"""Exercise the required merge gate with Actions job conclusions."""

import importlib.util
import json
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("ci_results", Path(__file__).resolve().parents[1] / "check_ci_results.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class CiResultsTests(unittest.TestCase):
    def success(self):
        return {name: {"result": "success", "outputs": {}} for name in module.REQUIRED_JOBS}

    def test_successful_prerequisites(self):
        self.assertTrue(module.succeeded(json.dumps(self.success())))

    def test_failed_skipped_cancelled_and_missing_prerequisites_block_merge(self):
        for name in module.REQUIRED_JOBS:
            for result in ("failure", "cancelled", "skipped", "neutral", "", None):
                with self.subTest(name=name, result=result):
                    jobs = self.success()
                    jobs[name]['result'] = result
                    self.assertFalse(module.succeeded(json.dumps(jobs)))
            jobs = self.success()
            del jobs[name]
            self.assertFalse(module.succeeded(json.dumps(jobs)))

    def test_unknown_jobs_and_workflow_prerequisite_drift_fail(self):
        jobs = self.success()
        jobs['unregistered'] = {'result': 'success'}
        self.assertFalse(module.succeeded(json.dumps(jobs)))
        workflow = (Path(__file__).resolve().parents[2] / '.github/workflows/ci.yml').read_text()
        import re
        needs = re.search(r'^    needs: \[([^]]+)\]', workflow, re.M)
        self.assertEqual({name.strip() for name in needs[1].split(',')}, module.REQUIRED_JOBS)

    def test_empty_malformed_or_wrong_shape_is_not_success(self):
        for raw in ("", "{", "{}", "[]", "null", '{"test":null}', '{"test":{}}', '{"test":"success"}'):
            with self.subTest(raw=raw):
                self.assertFalse(module.succeeded(raw))


if __name__ == "__main__":
    unittest.main()
