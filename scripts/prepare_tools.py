#!/usr/bin/env python3
"""Explicit CI tool preparation; a restored wrong version is an error, not a fallback."""
import json
from pathlib import Path
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[1]


def prepare():
    version=json.loads((ROOT/'licensing/policy.json').read_text())['cargo_deny_version']
    binary=ROOT/'.local/tools/bin'/('cargo-deny.exe' if sys.platform=='win32' else 'cargo-deny')
    if not binary.exists():
        subprocess.run(['cargo','+1.98.0','install','cargo-deny','--version',version,'--locked','--root','.local/tools','--target-dir','target/license-tools'],cwd=ROOT,check=True)
    actual=subprocess.check_output([str(binary),'--version'],cwd=ROOT,text=True).strip()
    if actual!='cargo-deny '+version:raise ValueError('Pinned cargo-deny version differs')
    subprocess.run(['cargo','+1.98.0','fetch','--locked'],cwd=ROOT,check=True)


if __name__=='__main__':
    try:prepare()
    except (OSError,ValueError,subprocess.SubprocessError):raise SystemExit('Pinned license-tool preparation failed')
