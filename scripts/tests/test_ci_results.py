"""Exercise the required merge gate with Actions job conclusions."""

import copy
import hashlib
from unittest.mock import patch
import importlib.util
import json
import tempfile
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
        needs = re.search(r'^    needs: \[([^]]+)\]', workflow.split('  ci-required:\n', 1)[1], re.M)
        self.assertEqual({name.strip() for name in needs[1].split(',')}, module.REQUIRED_JOBS)

    def test_empty_malformed_or_wrong_shape_is_not_success(self):
        for raw in ("", "{", "{}", "[]", "null", '{"test":null}', '{"test":{}}', '{"test":"success"}'):
            with self.subTest(raw=raw):
                self.assertFalse(module.succeeded(raw))


class PlannedCiTests(unittest.TestCase):
    def setUp(self):
        self.source = 'a' * 40
        self.policy = json.loads((Path(__file__).resolve().parents[1] / 'validation-policy.json').read_text())
        self.policy.update(activation='affected', affected_stages=['pr'])
        self.plan = dict(schema='gateway-validation-plan/v1', source_sha=self.source,
                         profile='affected', mode='affected', stage='pr', base_sha='b'*40, head_sha='c'*40,
                         jobs=['management-web', 'publication'], execution_jobs=['management-web', 'publication'])
        self.jobs = {j: {'result': 'success' if j in {'validation-plan', 'management-web', 'publication'} else 'skipped'} for j in module.REQUIRED_JOBS}

    def check(self):
        raw = json.dumps(self.policy).encode()
        self.plan['policy_sha256'] = hashlib.sha256(raw).hexdigest()
        with patch.object(Path, 'read_bytes', return_value=raw):
            return module.succeeded(json.dumps(self.jobs), json.dumps(self.plan), self.source, 'b'*40, 'c'*40)

    def test_only_explicit_non_applicable_skips_pass(self):
        self.assertTrue(self.check())
        for name in ('validation-plan', 'management-web', 'publication'):
            for result in ('skipped', 'failure', 'cancelled', ''):
                original = self.jobs[name]['result']
                self.jobs[name]['result'] = result
                self.assertFalse(self.check())
                self.jobs[name]['result'] = original

    def test_non_applicable_failure_is_not_hidden(self):
        self.jobs['rust']['result'] = 'failure'
        self.assertFalse(self.check())

    def test_missing_and_unknown_jobs_fail(self):
        del self.jobs['rust']
        self.assertFalse(self.check())
        self.jobs['unknown'] = {'result':'success'}
        self.assertFalse(self.check())

    def test_source_and_range_mismatch_fail(self):
        for key in ('source_sha', 'base_sha', 'head_sha'):
            original = self.plan[key]
            self.plan[key] = 'd' * 40
            self.assertFalse(self.check())
            self.plan[key] = original

    def test_missing_revision_requires_full_coverage(self):
        self.plan['base_sha'] = None
        self.assertFalse(self.check())

    def test_shadow_requires_all_jobs_even_for_web_plan(self):
        self.policy['activation'] = 'shadow'
        self.plan['mode'] = 'shadow'
        self.assertFalse(self.check())
        self.plan['execution_jobs'] = sorted(module.REQUIRED_JOBS - {'validation-plan'})
        self.assertFalse(self.check())
        for job in self.jobs.values(): job['result'] = 'success'
        self.assertTrue(self.check())

    def test_full_cannot_claim_a_subset(self):
        self.plan['profile'] = 'full'
        self.assertFalse(self.check())

    def test_duplicate_jobs_and_disagreeing_execution_fail(self):
        self.plan['jobs'].append('publication')
        self.assertFalse(self.check())
        self.plan['jobs'].pop()
        self.plan['execution_jobs'] = ['publication']
        self.assertFalse(self.check())

    def test_activation_is_stage_specific(self):
        self.plan['stage'] = 'main'
        self.assertFalse(self.check())

    def test_malformed_or_missing_plan_never_reduces_coverage(self):
        for raw in ('', '{}', 'null', '[]'):
            self.assertFalse(module.succeeded(json.dumps(self.jobs), raw, self.source))
        self.assertFalse(module.succeeded(json.dumps(self.jobs)))


class ResultReportTests(unittest.TestCase):
    def test_failure_and_declared_skip_are_recorded_without_job_outputs(self):
        state=Path(__file__).resolve().parents[2]/'.local/test-state';state.mkdir(parents=True,exist_ok=True)
        with tempfile.TemporaryDirectory(dir=state) as directory:
            root=Path(directory);summary=root/'summary.md'
            raw=json.dumps({'publication':{'result':'success','outputs':{'private':'must-not-copy'}},'management-web':{'result':'failure'}})
            plan=json.dumps({'execution_jobs':['publication','management-web']})
            module.write_report(raw,plan,False,{'GITHUB_SHA':'a'*40,'GITHUB_RUN_ID':'123','GITHUB_RUN_ATTEMPT':'2','GITHUB_STEP_SUMMARY':str(summary)},root)
            data=(root/'.local/validation/result.json').read_text();doc=json.loads(data)
            self.assertFalse(doc['passed']);self.assertEqual(doc['attempt'],'2')
            self.assertNotIn('must-not-copy',data+summary.read_text())
            self.assertEqual(next(j for j in doc['jobs'] if j['name']=='management-web')['result'],'failure')
            self.assertFalse(next(j for j in doc['jobs'] if j['name']=='rust')['selected'])
            module.write_report(raw,'malformed-plan',False,{},root)
            invalid=json.loads((root/'.local/validation/result.json').read_text())
            publication=next(j for j in invalid['jobs'] if j['name']=='publication')
            self.assertIsNone(publication['selected']);self.assertEqual(publication['result'],'success')
            module.write_report('null','null',False,{},root)
            self.assertTrue(all(j['selected'] is None for j in json.loads((root/'.local/validation/result.json').read_text())['jobs']))


if __name__ == "__main__":
    unittest.main()
