#!/usr/bin/env python3
"""Plan and run explicit local checks; report prospective CI coverage without skipping CI."""
from __future__ import annotations

import argparse
from fnmatch import fnmatchcase
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import time
import tomllib

ROOT = Path(__file__).resolve().parents[1]
POLICY = 'scripts/validation-policy.json'
SCHEMA = 'gateway-validation-plan/v1'


class ValidationError(Exception):
    """Fixed diagnostics; never include inspected file contents."""


def git(root, *args):
    result = subprocess.run(['git', '-C', str(root), *args], capture_output=True,
                            env={**os.environ, 'GIT_NO_REPLACE_OBJECTS': '1'})
    if result.returncode:
        raise ValidationError('Cannot resolve the selected Git input')
    return result.stdout


def read_policy(root):
    raw = (root / POLICY).read_bytes()
    value = json.loads(raw)
    if (value['schema'] != 'gateway-validation-policy/v1'
            or type(value['force_full']) is not bool or value['activation'] not in {'shadow', 'affected'}
            or len(set(value['jobs'])) != len(value['jobs'])):
        raise ValidationError('Invalid validation policy')
    for rule in value['rules']:
        if not set(rule['ci']) <= set(value['jobs']) or not set(rule['local']) <= set(value['checks']):
            raise ValidationError('Unknown check in validation policy')
    return value, hashlib.sha256(raw).hexdigest()


def split_paths(raw):
    return {os.fsdecode(p) for p in raw.split(b'\0') if p}


def changes(root, scope, base=None, head=None):
    """--no-renames reports both sides as add/delete, including old sensitive paths."""
    if scope == 'range':
        if not base or not head:
            raise ValidationError('Both base and head are required')
        base = git(root, 'rev-parse', '--verify', base + '^{commit}').decode().strip()
        head = git(root, 'rev-parse', '--verify', head + '^{commit}').decode().strip()
        raw = git(root, 'diff', '--name-only', '--no-renames', '-z', base, head, '--')
    else:
        head = git(root, 'rev-parse', '--verify', 'HEAD').decode().strip()
        base = head
        raw = git(root, 'diff', '--cached', '--name-only', '--no-renames', '-z', '--')
        if scope == 'worktree':
            raw += git(root, 'diff', '--name-only', '--no-renames', '-z', '--')
            raw += git(root, 'ls-files', '--others', '--exclude-standard', '-z')
    return sorted(split_paths(raw)), base, head


def input_digest(root, scope, base, head, paths):
    digest = hashlib.sha256()
    if scope == 'range':
        digest.update(git(root, 'diff', '--binary', '--no-ext-diff', base, head, '--'))
    elif scope == 'staged':
        digest.update(git(root, 'diff', '--cached', '--binary', '--no-ext-diff', '--'))
    else:
        digest.update(git(root, 'diff', '--cached', '--binary', '--no-ext-diff', '--'))
        digest.update(git(root, 'diff', '--binary', '--no-ext-diff', '--'))
        for path in sorted(split_paths(git(root, 'ls-files', '--others', '--exclude-standard', '-z'))):
            target = root / path
            info = target.lstat()
            digest.update(os.fsencode(path) + b'\0' + str(info.st_mode).encode() + b'\0')
            if stat.S_ISLNK(info.st_mode):
                digest.update(os.fsencode(os.readlink(target)))
            elif stat.S_ISREG(info.st_mode):
                with target.open('rb') as stream:
                    for chunk in iter(lambda: stream.read(1024 * 1024), b''):
                        digest.update(chunk)
            else:
                raise ValidationError('Unsupported changed file type')
    return digest.hexdigest()


def rule_for(policy, path):
    return next((r for r in policy['rules'] if any(fnmatchcase(path, p) for p in r['patterns'])), None)


def packages(root, paths, full):
    """Read local manifests without resolving third-party dependencies or invoking Cargo."""
    workspace = tomllib.loads((root / 'Cargo.toml').read_text())
    manifests = {'.': workspace}
    for member in workspace['workspace']['members']:
        manifests[member] = tomllib.loads((root / member / 'Cargo.toml').read_text())
    selected = {m['package']['name'] for directory, m in manifests.items()
                if full or any(p.startswith(directory + '/') for p in paths if directory != '.')}
    if any(p.startswith(('src/', 'tests/', 'examples/')) for p in paths):
        selected.add(workspace['package']['name'])
    edges = {}
    for directory, manifest in manifests.items():
        tables = [manifest] + list(manifest.get('target', {}).values())
        dependencies = set()
        for table in tables:
            for kind in ('dependencies', 'dev-dependencies', 'build-dependencies'):
                for alias, spec in table.get(kind, {}).items():
                    if isinstance(spec, dict) and 'path' in spec:
                        dependencies.add(spec.get('package', alias))
        edges[manifest['package']['name']] = dependencies
    while True:
        expanded = selected | {name for name, deps in edges.items() if deps & selected}
        if expanded == selected:
            return sorted(selected)
        selected = expanded


def make_plan(root, profile='affected', scope='worktree', base=None, head=None, stage='local'):
    policy, digest = read_policy(root)
    fallback = None
    try:
        paths, base, head = changes(root, scope, base, head)
    except ValidationError:
        paths, base, head = [], None, None
        fallback = 'unresolved-git-input'
    matched = [rule_for(policy, p) for p in paths]
    reasons = sorted({r['name'] if r else 'unclassified-path' for r in matched})
    full = profile == 'full' or policy['force_full'] or fallback or None in matched or any(r and r.get('full') for r in matched)
    # Policy changes never decide their own reduced coverage, even when a rule was removed.
    protected = [POLICY, 'scripts/validation.py', 'scripts/check_ci_results.py', 'scripts/ci_plan.py']
    if any(p in protected or p.startswith('.github/') for p in paths):
        full = True
        reasons.append('validation-policy-change')
    if fallback:
        reasons.append(fallback)
    jobs = set(policy['jobs']) if full else {'publication'} | {j for r in matched for j in r['ci']}
    local = set(policy['checks']) if full else {'boundary'} | {c for r in matched for c in r['local']}
    if 'package-smoke' in jobs:
        jobs |= {'targets', 'management-web'}
    if {'codex-conformance', 'api-codecs'} & jobs:
        jobs.add('conformance-prepare')
    if 'rust' in jobs:
        jobs.add('targets')
    if profile == 'full' or policy['force_full']:
        reasons.append('explicit-full-profile')
    return dict(schema=SCHEMA, stage=stage, profile='full' if full else 'affected', scope=scope,
                base_sha=base, head_sha=head, policy_sha256=digest, activation=policy['activation'],
                changed_count=len(paths), reasons=sorted(set(reasons)), jobs=sorted(jobs),
                checks=sorted(local), packages=packages(root, paths, bool(full)),
                input_sha256=input_digest(root, scope, base, head, paths) if not fallback else None,
                tools=sorted({t for c in local for t in policy['checks'][c]['tools']}))


def commands(root, plan, policy):
    rust_packages = [x for p in plan['packages'] for x in ('-p', p)]
    features = [p + '/team' for p in ('gateway-management-embedded', 'gateway-management-app') if p in plan['packages']]
    values = {'{python}': [sys.executable], '{scope}': ['--staged' if plan['scope'] == 'staged' else '--worktree'],
              '{packages}': rust_packages or ['--workspace'],
              '{features}': ['--features', ','.join(features)] if features else []}
    suffix = '.exe' if os.name == 'nt' else ''
    values['{gateway-bin}'] = ['target/debug/agent-response-gateway' + suffix]
    values['{observer-bin}'] = ['target/debug/examples/metadata_observer' + suffix]
    values['{recorder-bin}'] = ['target/debug/gateway-usage-recorder' + suffix]
    if plan['scope'] == 'range' and 'boundary' in plan['checks']:
        values['{scope}'] = ['--base', plan['base_sha'], '--head', plan['head_sha']]
    # The fixture consumes built assets; run it only after the production Web build.
    for name in policy['checks']:
        if name not in plan['checks']:
            continue
        for command in policy['checks'][name]['commands']:
            yield name, [part for arg in command for part in values.get(arg, [arg])]


def require_execution_checkout(root, plan):
    if git(root, 'rev-parse', 'HEAD').decode().strip() != plan['head_sha']:
        raise ValidationError('Execution checkout must match the selected head')
    if plan['scope'] == 'range' and git(root, 'status', '--porcelain', '--untracked-files=normal'):
        raise ValidationError('Commit-range execution requires a clean checkout of the selected head')
    if plan['scope'] == 'staged' and (git(root, 'diff', '--name-only', '-z', '--') or git(root, 'ls-files', '--others', '--exclude-standard', '-z')):
        raise ValidationError('Staged execution requires worktree bytes to match the index; stage changes or use --worktree')


def run_plan(root, plan):
    if plan['head_sha'] is None:
        raise ValidationError('Resolve the Git input before executing checks')
    require_execution_checkout(root, plan)
    policy, digest = read_policy(root)
    if digest != plan['policy_sha256']:
        raise ValidationError('Validation policy changed after planning')
    missing = []
    for tool in plan['tools']:
        if tool == 'cargo-deny':
            present = (root / '.local/tools/bin' / ('cargo-deny.exe' if os.name == 'nt' else 'cargo-deny')).is_file()
        else:
            present = tool == 'python3' or shutil.which(tool)
        if not present:
            missing.append(tool)
    if missing:
        affected = sorted(c for c in plan['checks'] if set(policy['checks'][c]['tools']) & set(missing))
        preparation = []
        if 'cargo' in missing:
            preparation.append('rustup toolchain install 1.98.0 --profile minimal --component clippy --component rustfmt')
        if 'cargo-deny' in missing:
            preparation.append(sys.executable + ' -B scripts/prepare_tools.py')
        if {'node', 'npm'} & set(missing):
            preparation.append('Install Node 24.21.0 and npm 11.19.0 using your Node manager')
            for check, directory in [('web', 'management-web'), ('web-api', 'management-web'), ('docs', 'docs-site')]:
                command = 'npm ci --prefix ' + directory + ' --ignore-scripts'
                if check in plan['checks'] and command not in preparation:
                    preparation.append(command)
        if 'git' in missing:
            preparation.append('Install Git using your platform package manager')
        raise ValidationError('Missing tools: ' + ', '.join(missing) + '; affected checks: ' + ', '.join(affected)
                              + '; preparation (not executed): ' + '; '.join(preparation))
    state = root / '.local/validation'
    state.mkdir(parents=True, exist_ok=True)
    result = dict(schema='gateway-validation-result/v1', plan=plan, checks=[], success=False)
    try:
        for name, command in commands(root, plan, policy):
            env = dict(os.environ)
            if policy['checks'][name].get('fixture') and command[0] == 'npm':
                env['WEB_FIXTURE_BIN'] = str(root / 'target/debug/examples' / ('web_fixture.exe' if os.name == 'nt' else 'web_fixture'))
            if command[0] == 'npm':
                from web_assets import npm_command
                from release_package import PackageError
                try:
                    command = npm_command(env) + command[1:]
                except PackageError as error:
                    raise ValidationError('Pinned npm CLI is missing; repair the selected Node/npm installation') from error
            started = time.monotonic()
            print('validation: ' + name + ': ' + ' '.join(command), flush=True)
            completed = subprocess.run(command, cwd=root, env=env, check=False)
            result['checks'].append(dict(check=name, command=command, returncode=completed.returncode,
                                         duration_seconds=round(time.monotonic() - started, 3)))
            if completed.returncode:
                raise ValidationError('Selected check failed: ' + name)
        require_execution_checkout(root, plan)
        current = make_plan(root, plan['profile'], plan['scope'], plan['base_sha'], plan['head_sha'], stage=plan['stage'])
        if current['input_sha256'] != plan['input_sha256'] or current['head_sha'] != plan['head_sha'] or current['policy_sha256'] != digest:
            raise ValidationError('Validation inputs changed during execution; rerun selected checks')
        result['success'] = True
    finally:
        (state / 'result.json').write_text(json.dumps(result, indent=2) + '\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['plan', 'run'])
    parser.add_argument('--root', type=Path, default=ROOT)
    parser.add_argument('--profile', choices=['affected', 'full'], default='affected')
    parser.add_argument('--stage', choices=['local', 'pr', 'main', 'release'], default='local')
    group = parser.add_mutually_exclusive_group()
    group.add_argument('--worktree', action='store_true')
    group.add_argument('--staged', action='store_true')
    group.add_argument('--base')
    parser.add_argument('--head')
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    try:
        if args.base == '' or args.head == '':
            raise ValidationError('Non-empty base and head are required')
        if bool(args.base) != bool(args.head):
            raise ValidationError('Both base and head are required')
        scope = 'range' if args.base else 'staged' if args.staged else 'worktree'
        plan = make_plan(args.root.resolve(), args.profile, scope, args.base, args.head, args.stage)
        raw = json.dumps(plan, indent=2) + '\n'
        if args.output:
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(raw)
        print(raw, end='')
        if args.command == 'run':
            run_plan(args.root.resolve(), plan)
    except (ValidationError, OSError, ValueError, KeyError, TypeError) as error:
        message = str(error) if isinstance(error, ValidationError) else 'Invalid validation input or tool failure'
        parser.exit(1, 'validation: ' + message + '\n')


if __name__ == '__main__':
    main()
