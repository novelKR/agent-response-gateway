"""Preserve the independent baseline's complete scenario recipes while splitting execution."""
from collections import Counter
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
ROOT=Path(__file__).resolve().parents[2]
spec=importlib.util.spec_from_file_location('suite',ROOT/'scripts/conformance_suite.py')
suite=importlib.util.module_from_spec(spec);spec.loader.exec_module(suite)


class SuiteTests(unittest.TestCase):
    def test_original_scenarios_are_present_exactly_once(self):
        baseline=json.loads((ROOT/'scripts/tests/fixtures/conformance-baseline.json').read_text())
        groups=json.loads((ROOT/'scripts/conformance-suites.json').read_text())['groups']
        self.assertEqual(set(groups),{'editing','protocol-continuity','managed-reasoning','legacy-migration','codec-protocol','codec-editing-accounting'})
        original=Counter((s['name'],s['run']) for s in baseline['steps'])
        actual=Counter((s['name'],s['run']) for steps in groups.values() for s in steps)
        self.assertEqual(actual,original)
        self.assertTrue(all(count==1 for count in actual.values()))
        self.assertRegex(baseline['source_commit'],r'^[a-f0-9]{40}$')

    def test_failure_stops_group_and_is_recorded(self):
        state=ROOT/'.local/test-state';state.mkdir(parents=True,exist_ok=True)
        with tempfile.TemporaryDirectory(dir=state) as d:
            root=Path(d);(root/'scripts').mkdir()
            (root/'scripts/conformance-suites.json').write_text(json.dumps({'groups':{'editing':[{'name':'fails','run':'exit 7'},{'name':'must not run','run':'exit 0'}]}}))
            with patch.object(suite,'ROOT',root),patch.dict('os.environ',{},clear=True):
                self.assertEqual(suite.run('editing'),7)
            result=json.loads((root/'.local/conformance-groups/editing/results.json').read_text())
            self.assertEqual(len(result['results']),1)
            self.assertEqual(result['results'][0]['returncode'],7)
