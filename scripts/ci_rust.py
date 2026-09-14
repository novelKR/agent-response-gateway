#!/usr/bin/env python3
"""Select native-matrix package tests without expanding unsupported platform packages."""
import json
import os
import re
import subprocess
import validation


def selected_commands(root, plan, source):
    policy,digest=validation.read_policy(root)
    if plan['source_sha']!=source or not re.fullmatch('[a-f0-9]{40}',source) or plan['policy_sha256']!=digest:
        raise validation.ValidationError('Rust check source or policy differs')
    native=set(policy['ci_rust_packages'])
    chosen=set(plan['packages']) & native
    # Script integration inputs have no Cargo path; retain the existing native set.
    if not chosen or plan['profile']=='full':chosen=native
    scoped={**plan,'checks':['rust'],'packages':sorted(chosen)}
    return [['cargo','+1.98.0',*command[1:]] for name,command in validation.commands(root,scoped,policy)
            if name=='rust' and command[1]!='fmt']


if __name__=='__main__':
    try:
        commands=selected_commands(validation.ROOT,json.loads(os.environ['VALIDATION_PLAN']),os.environ['GITHUB_SHA'])
        for command in commands:
            print('Native validation: '+' '.join(command),flush=True)
            result=subprocess.run(command,cwd=validation.ROOT,check=False)
            if result.returncode:raise SystemExit(result.returncode)
    except (OSError,ValueError,KeyError,TypeError,validation.ValidationError):raise SystemExit('Selected native validation failed')
