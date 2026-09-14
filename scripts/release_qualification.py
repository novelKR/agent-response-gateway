#!/usr/bin/env python3
"""Exact-source full validation evidence, independent of ordinary main integration."""
import base64
import hashlib
import json
import os
from pathlib import Path
import re

import check_ci_results
from release_targets import TARGETS

SCHEMA='gateway-release-qualification/v1'
ROOT=Path(__file__).resolve().parents[1]
MINIMUM={'targets','format','rust','publication','licenses','codex-conformance','package-smoke','docs'}


def require(value):
    if not value:raise ValueError('Full release qualification is missing or differs')


def record(plan, results, env):
    require(plan['profile']=='full' and plan['stage']=='release')
    require(check_ci_results.succeeded(json.dumps(results),json.dumps(plan),env['GITHUB_SHA'],env['GITHUB_SHA'],env['GITHUB_SHA']))
    value = dict(schema=SCHEMA,source_commit=env['GITHUB_SHA'],policy_sha256=plan['policy_sha256'],
                run_id=int(env['GITHUB_RUN_ID']),attempt=int(env['GITHUB_RUN_ATTEMPT']),
                targets=sorted(TARGETS),checks={n:results[n]['result'] for n in sorted(results)})
    validate(value, env['GITHUB_SHA'], (ROOT/'scripts/validation-policy.json').read_bytes())
    return value


def validate(value, commit, policy_bytes, targets=TARGETS, run_id=None, attempt=None):
    require(isinstance(value,dict) and set(value)=={'schema','source_commit','policy_sha256','run_id','attempt','targets','checks'})
    require(isinstance(commit,str) and re.fullmatch('[a-f0-9]{40}',commit))
    policy=json.loads(policy_bytes)
    require(policy['schema']=='gateway-validation-policy/v1' and isinstance(policy['jobs'],list) and len(policy['jobs'])==len(set(policy['jobs'])))
    expected=set(policy['jobs'])|{'validation-plan'}
    require(MINIMUM<=expected and value['schema']==SCHEMA and value['source_commit']==commit
            and value['policy_sha256']==hashlib.sha256(policy_bytes).hexdigest()
            and value['targets']==sorted(targets) and isinstance(value['checks'],dict)
            and set(value['checks'])==expected and all(s=='success' for s in value['checks'].values()))
    require(type(value['run_id']) is int and value['run_id']>0 and type(value['attempt']) is int and value['attempt']==1)
    if run_id is not None:require(value['run_id']==int(run_id))
    if attempt is not None:require(value['attempt']==int(attempt))


def source_bytes(github, repository, commit, path, optional=False):
    value=github.api(f'repos/{repository}/contents/{path}?ref={commit}',missing=optional)
    if value is None:return None
    require(value['encoding']=='base64')
    return base64.b64decode(value['content'],validate=False)


def job_names(jobs, suites=None):
    names={'ci-required'}
    for job in jobs:
        if job in {'rust','package-smoke'}:
            names.update(f'{job} ({target})' for target in TARGETS)
        elif suites and job in {'codex-conformance','api-codecs'}:
            groups=[g for g in suites['groups'] if g.startswith('codec-')==(job=='api-codecs')]
            require(bool(groups));names.update(f'{job} ({group})' for group in groups)
        else:names.add(job)
    return names


def require_jobs(github, repository, run_id, attempt, names, prefix=''):
    seen={};page=1
    while True:
        value=github.api(f'repos/{repository}/actions/runs/{int(run_id)}/attempts/{int(attempt)}/jobs?per_page=100&page={page}')
        require(isinstance(value['jobs'],list))
        for job in value['jobs']:
            if job['name'] in seen:raise ValueError('Duplicate qualification job identity')
            seen[job['name']]=job
        if len(seen)>=value['total_count']:break
        require(bool(value['jobs']) and page<20);page+=1
    for name in names:
        job=seen.get(prefix+name)
        require(job is not None and job['status']=='completed' and job['conclusion']=='success')


def verify_live(github, repository, value, commit, run_id, attempt):
    policy=source_bytes(github,repository,commit,'scripts/validation-policy.json')
    validate(value,commit,policy,run_id=run_id,attempt=attempt)
    suites=json.loads(source_bytes(github,repository,commit,'scripts/conformance-suites.json'))
    require_jobs(github,repository,run_id,attempt,job_names(value['checks'],suites),'qualification / ')


def verify_legacy_main(github, repository, commit, run):
    workflow=source_bytes(github,repository,commit,'.github/workflows/ci.yml').decode()
    match=re.search(r'^  ci-required:\n.*?^    needs: \[([^\]]+)\]',workflow,re.M|re.S)
    require(match is not None)
    jobs={j.strip() for j in match[1].split(',')}
    require(MINIMUM<=jobs and all(re.fullmatch('[a-z][a-z-]*',j) for j in jobs))
    raw=source_bytes(github,repository,commit,'scripts/conformance-suites.json',optional=True)
    suites=json.loads(raw) if raw is not None else None
    require_jobs(github,repository,run['id'],run['run_attempt'],job_names(jobs,suites))


if __name__=='__main__':
    try:
        result=record(json.loads(os.environ['VALIDATION_PLAN']),json.loads(os.environ['CI_RESULTS']),os.environ)
        output=ROOT/'.local/validation/qualification.json';output.parent.mkdir(parents=True,exist_ok=True)
        output.write_text(json.dumps(result,sort_keys=True,indent=2)+'\n')
        print('Full release qualification recorded for the exact source and execution')
    except (OSError,ValueError,KeyError,TypeError):raise SystemExit('Full release qualification failed')
