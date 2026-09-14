#!/usr/bin/env python3
"""Run one complete, isolated group of the existing synthetic conformance recipes."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]


def run(group):
    document = json.loads((ROOT / 'scripts/conformance-suites.json').read_text())
    steps = document.get('setup', {}).get(group, []) + document['groups'][group]
    state = ROOT / '.local/conformance-groups' / group
    state.mkdir(parents=True, exist_ok=True)
    results = []
    try:
        for step in steps:
            print(step['name'], flush=True)
            start = time.monotonic()
            result = subprocess.run(['bash', '-euo', 'pipefail', '-c', step['run']], cwd=ROOT, check=False)
            results.append(dict(name=step['name'], returncode=result.returncode,
                                duration_seconds=round(time.monotonic()-start, 3)))
            if result.returncode:
                return result.returncode
        return 0
    finally:
        (state / 'results.json').write_text(json.dumps(dict(group=group, results=results), indent=2)+'\n')
        summary = os.environ.get('GITHUB_STEP_SUMMARY')
        if summary:
            with open(summary, 'a') as stream:
                stream.write('```json\n'+json.dumps(results, indent=2)+'\n```\n')
                for name in ['conformance-results.jsonl','continuity-results.jsonl']:
                    path = ROOT / '.local' / name
                    if path.exists():stream.write(path.read_text())


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('group', choices=json.loads((ROOT / 'scripts/conformance-suites.json').read_text())['groups'])
    raise SystemExit(run(parser.parse_args().group))
