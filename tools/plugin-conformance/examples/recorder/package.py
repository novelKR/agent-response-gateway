"""Build a flat native package without a gateway checkout. No runtime downloads."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
from recorder import CAPABILITIES, PROTOCOL

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--output', type=Path, required=True)
parser.add_argument('--target', required=True, choices=['linux-x64', 'linux-arm64', 'macos-x64', 'macos-arm64'])
args = parser.parse_args()
source = Path(__file__).resolve().parent
args.output.mkdir(mode=0o700)
subprocess.run([sys.executable, '-B', str(source / 'build.py'), '--output', str(args.output / 'extension')], check=True)
for name in ['LICENSE.txt', 'recorder.py', 'build.py', 'package.py', 'README.md', 'event-v1.schema.json', 'event-v2.schema.json']:
    (args.output / name).write_bytes((source / name).read_bytes())
manifest = {'schema': 'gateway-extension-package/v2', 'id': 'synthetic-recorder', 'version': '1.0.0',
            'target': args.target, 'protocol': PROTOCOL,
            'permissions': ['export_usage', 'observe_usage', 'write_usage_store'],
            'state_schema': 'usage-store/v2', 'capabilities': CAPABILITIES,
            'files': {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in args.output.iterdir()}}
raw = (json.dumps(manifest, sort_keys=True, separators=(',', ':'), ensure_ascii=True) + '\n').encode()
(args.output / 'extension.json').write_bytes(raw)
print(hashlib.sha256(raw).hexdigest())
