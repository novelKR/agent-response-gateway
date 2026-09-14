"""CI event identity and baseline-policy expansion without network access."""
import copy
from pathlib import Path
import sys
import unittest
from unittest.mock import patch
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts'))
import ci_plan
import validation


class CiPlanningTests(unittest.TestCase):
    def setUp(self):
        self.env = dict(GITHUB_SHA='a'*40, GITHUB_EVENT_NAME='pull_request')
        self.event = {'pull_request': {'base': {'sha':'b'*40}, 'head':{'sha':'c'*40}}}
        self.prospect = dict(schema=validation.SCHEMA, stage='pr', profile='affected', jobs=['management-web','publication'], reasons=[])

    def git(self, root, *args):
        if args[0] == 'rev-parse': return b'a'*40+b'\n'
        return (ROOT / args[-1].split(':',1)[1]).read_bytes()

    def prepare(self, changed=False):
        policy, digest = validation.read_policy(ROOT)
        policy.update(activation='affected', affected_stages=['pr'])
        def plan(root, profile, scope, base, head, stage):
            p = copy.deepcopy(self.prospect)
            p.update(stage=stage, profile=profile, base_sha=base, head_sha=head)
            if profile == 'full': p['jobs'] = sorted(policy['jobs'])
            return p
        def git(root,*args):
            if changed and args[0]=='show': return b'prior policy'
            return self.git(root,*args)
        with patch.object(validation, 'git', side_effect=git), patch.object(validation, 'make_plan', side_effect=plan), patch.object(validation, 'read_policy', return_value=(policy,digest)):
            return ci_plan.prepare(ROOT,self.event,self.env)

    def test_identical_policy_can_select_web_coverage(self):
        p = self.prepare()
        self.assertEqual(p['execution_jobs'], ['management-web','publication'])
        self.assertEqual(p['source_sha'], self.env['GITHUB_SHA'])
        self.assertFalse(p['web_fixture'])

    def test_policy_change_runs_full_even_if_new_rules_claim_web_only(self):
        p = self.prepare(changed=True)
        self.assertEqual(p['profile'], 'full')
        self.assertIn('codex-conformance', p['execution_jobs'])
        self.assertIn('baseline-policy-differs', p['reasons'])
        self.assertTrue(p['web_fixture'])

    def test_checkout_mismatch_is_rejected(self):
        self.env['GITHUB_SHA'] = 'd'*40
        with self.assertRaises(validation.ValidationError): self.prepare()

    def test_schedule_and_manual_runs_require_full(self):
        for name in ('schedule','workflow_dispatch'):
            self.env['GITHUB_EVENT_NAME'] = name
            p = self.prepare()
            self.assertEqual(p['profile'],'full')
            self.assertEqual(p['mode'],'shadow')

    def test_release_caller_qualifies_exact_tag_source_even_on_push(self):
        self.env.update(GITHUB_EVENT_NAME='push', VALIDATION_RELEASE='true')
        self.event = {'before':'0'*40}
        p = self.prepare()
        self.assertEqual(p['stage'], 'release')
        self.assertEqual(p['profile'], 'full')
        self.assertEqual(p['base_sha'], self.env['GITHUB_SHA'])
        self.assertEqual(p['head_sha'], self.env['GITHUB_SHA'])

    def test_push_uses_before_and_actual_commit(self):
        self.env['GITHUB_EVENT_NAME'] = 'push'
        self.event = {'before':'b'*40}
        p = self.prepare()
        self.assertEqual(p['head_sha'], self.env['GITHUB_SHA'])
        self.assertEqual(p['base_sha'], self.event['before'])

    def test_malformed_revision_is_rejected(self):
        self.event['pull_request']['head']['sha'] = 'not-a-sha'
        with self.assertRaises(validation.ValidationError): self.prepare()
