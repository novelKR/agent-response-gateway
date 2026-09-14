#!/usr/bin/env python3
"""Require successful execution of the exact plan; distinguish non-applicable jobs."""
import json
import os
from pathlib import Path
import re

REQUIRED_JOBS = {'validation-plan', 'conformance-prepare', 'web-windows', 'targets', 'format', 'rust', 'publication', 'licenses',
                 'codex-conformance', 'package-smoke', 'docs', 'usage-recorder', 'api-codecs', 'management-web'}


def succeeded(raw, plan_raw=None, source=None, base=None, head=None):
    try:
        results = json.loads(raw)
        if not isinstance(results, dict) or set(results) != REQUIRED_JOBS:
            return False
        if plan_raw is None:  # Legacy callers retain the full-set-only contract.
            return all(isinstance(j, dict) and j.get('result') == 'success' for j in results.values())
        plan = json.loads(plan_raw)
        policy_raw = (Path(__file__).resolve().parent / 'validation-policy.json').read_bytes()
        import hashlib
        policy = json.loads(policy_raw)
        expected = REQUIRED_JOBS - {'validation-plan'}
        if (plan['schema'] != 'gateway-validation-plan/v1'
                or plan['source_sha'] != source or not re.fullmatch('[a-f0-9]{40}', source or '')
                or plan['policy_sha256'] != hashlib.sha256(policy_raw).hexdigest()
                or set(policy['jobs']) != expected or plan['profile'] not in {'full', 'affected'}
                or plan['mode'] not in {'shadow', 'affected'}
                or len(plan['jobs']) != len(set(plan['jobs']))
                or len(plan['execution_jobs']) != len(set(plan['execution_jobs']))
                or not set(plan['jobs']) <= expected
                or 'publication' not in plan['jobs']):
            return False
        if not isinstance(plan['jobs'], list) or not isinstance(plan['execution_jobs'], list):
            return False
        if base is not None and plan['base_sha'] not in {base, None} or head is not None and plan['head_sha'] not in {head, None}:
            return False
        if (plan['base_sha'] is None or plan['head_sha'] is None) and plan['profile'] != 'full':
            return False
        enabled = policy['activation'] == 'affected' and plan['stage'] in policy.get('affected_stages', [])
        if plan['mode'] != ('affected' if enabled else 'shadow'):
            return False
        if plan['profile'] == 'full' and set(plan['jobs']) != expected:
            return False
        execution = set(plan['jobs']) if enabled else expected
        if set(plan['execution_jobs']) != execution:
            return False
        for name, job in results.items():
            if not isinstance(job, dict):
                return False
            required = name == 'validation-plan' or name in execution
            if job.get('result') != ('success' if required else 'skipped'):
                return False
        return True
    except (OSError, ValueError, KeyError, TypeError):
        return False


if __name__ == '__main__':
    passed = succeeded(os.environ.get('CI_RESULTS', ''), os.environ.get('VALIDATION_PLAN'), os.environ.get('GITHUB_SHA'), os.environ.get('VALIDATION_BASE'), os.environ.get('VALIDATION_HEAD'))
    print('Required CI prerequisites passed' if passed else 'Required CI prerequisites did not all succeed')
    raise SystemExit(0 if passed else 1)
