#!/usr/bin/env python3
"""Bind prospective and executable coverage to the current workflow source."""
import json
import os
from pathlib import Path
import re
import sys

import validation


def prepare(root, event, env):
    name = env['GITHUB_EVENT_NAME']
    source = validation.git(root, 'rev-parse', 'HEAD').decode().strip()
    if source != env['GITHUB_SHA']:
        raise validation.ValidationError('Workflow checkout differs from the selected source')
    if env.get('VALIDATION_RELEASE') == 'true':
        base = head = source
        stage, profile = 'release', 'full'
    elif name == 'pull_request':
        base, head = event['pull_request']['base']['sha'], event['pull_request']['head']['sha']
        stage, profile = 'pr', 'affected'
    elif name == 'push':
        base, head = event['before'], env['GITHUB_SHA']
        stage, profile = 'main', 'affected'
    elif name in {'workflow_dispatch', 'schedule', 'workflow_call'}:
        base = head = source
        stage, profile = 'release' if env.get('VALIDATION_RELEASE') == 'true' else 'main', 'full'
    else:
        raise validation.ValidationError('Unsupported CI planning event')
    if not all(isinstance(x, str) and re.fullmatch('[a-f0-9]{40}', x) for x in (base, head)):
        raise validation.ValidationError('Invalid CI revision identity')
    plan = validation.make_plan(root, profile, 'range', base, head, stage)
    policy, digest = validation.read_policy(root)
    # Activation cannot be introduced by the same unvalidated change. Compare
    # the execution policy AND planner to the event's trusted base snapshot.
    changed_policy = False
    for path in ('scripts/validation.py', validation.POLICY, 'scripts/ci_plan.py', 'scripts/check_ci_results.py'):
        try:
            original = validation.git(root, 'show', base + ':' + path)
            changed_policy |= original != (root / path).read_bytes()
        except (OSError, validation.ValidationError):
            changed_policy = True
    if changed_policy:
        plan = validation.make_plan(root, 'full', 'range', base, head, stage)
        plan['reasons'] = sorted(set(plan['reasons']) | {'baseline-policy-differs'})
    enabled = policy['activation'] == 'affected' and stage in policy.get('affected_stages', [])
    execution = plan['jobs'] if enabled else sorted(policy['jobs'])
    plan.update(source_sha=source, execution_jobs=execution, mode='affected' if enabled else 'shadow',
                policy_sha256=digest, event=name)
    return plan


def main():
    try:
        root = validation.ROOT
        event = json.loads(Path(os.environ['GITHUB_EVENT_PATH']).read_text())
        plan = prepare(root, event, os.environ)
        raw = json.dumps(plan, separators=(',', ':'))
        state = root / '.local/validation'
        state.mkdir(parents=True, exist_ok=True)
        (state / 'plan.json').write_text(json.dumps(plan, indent=2) + '\n')
        with open(os.environ['GITHUB_OUTPUT'], 'a') as output:
            output.write('plan=' + raw + '\n')
            for job in validation.read_policy(root)[0]['jobs']:
                output.write(job + '=' + str(job in plan['execution_jobs']).lower() + '\n')
        with open(os.environ['GITHUB_STEP_SUMMARY'], 'a') as summary:
            summary.write('```json\n' + json.dumps(plan, indent=2) + '\n```\n')
        print('Validation plan recorded: mode=' + plan['mode'] + ' profile=' + plan['profile'])
    except (OSError, ValueError, KeyError, TypeError, validation.ValidationError):
        raise SystemExit('CI validation planning failed; no reduced coverage is authorized')


if __name__ == '__main__':
    main()
