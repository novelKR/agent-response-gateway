#!/usr/bin/env python3
"""Seal same-run synthetic-test inputs; not a release signature or reusable test result."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[1]
LOCKS = ['Cargo.lock','rust-toolchain.toml','tests/codex/runtime-lock.json','scripts/conformance-suites.json','.github/workflows/ci.yml','scripts/prepared_inputs.py']
BINS = {
    'native': ['target/debug/agent-response-gateway', 'target/debug/examples/embedded_editing',
               'target/legacy/debug/agent-response-gateway'],
    'codec': ['target/debug/agent-response-gateway', 'target/debug/gateway-usage-recorder',
              'target/debug/examples/api_codec', 'target/debug/examples/api_codec_editing'],
}


def require(value):
    if not value: raise ValueError('Prepared input identity or bytes differ')


def sha(data): return hashlib.sha256(data).hexdigest()


def expected(root, variant='native'):
    lock = json.loads((root / 'tests/codex/runtime-lock.json').read_text())
    names = set(BINS[variant]) | {'.local/codex-runtime/' + p for p in lock['files']}
    require(all(not n.startswith('/') and '\\' not in n and all(p not in {'', '.', '..'} for p in n.split('/')) for n in names))
    return names


def identity(root):
    return dict(schema='gateway-ci-prepared/v1',source_commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),
                run_id=os.environ['GITHUB_RUN_ID'],attempt=os.environ['GITHUB_RUN_ATTEMPT'],
                system=platform.system(),architecture=platform.machine(),
                rustc=subprocess.check_output(['rustc','--version'],text=True).strip(),
                locks={p:sha((root/p).read_bytes()) for p in LOCKS})


def pack(root, output, variant='native'):
    require(not output.exists())
    files={}
    for name in sorted(expected(root, variant)):
        path=root/(name.replace('target/debug/', 'target/conformance-native/debug/', 1) if variant=='native' and name.startswith('target/debug/') else name)
        require(path.is_file() and not any(p.is_symlink() for p in [path,*path.parents] if p!=root.parent))
        raw=path.read_bytes()
        files[name]=dict(sha256=sha(raw),size=len(raw),mode=path.stat().st_mode&0o777)
    receipt={**identity(root),'variant':variant,'files':files}
    output.parent.mkdir(parents=True,exist_ok=True)
    with tarfile.open(output,'w:gz') as archive:
        for name in ['prepared.json',*sorted(files)]:
            raw=(json.dumps(receipt,sort_keys=True)+'\n').encode() if name=='prepared.json' else (root/(name.replace('target/debug/', 'target/conformance-native/debug/', 1) if variant=='native' and name.startswith('target/debug/') else name)).read_bytes()
            if name!='prepared.json':require(sha(raw)==files[name]['sha256'])
            info=tarfile.TarInfo(name);info.size=len(raw);info.mode=0o644 if name=='prepared.json' else files[name]['mode']
            archive.addfile(info,io.BytesIO(raw))
    return sha(output.read_bytes())


def unpack(root, archive_path, expected_sha, variant='native'):
    require(sha(archive_path.read_bytes())==expected_sha)
    with tarfile.open(archive_path,'r:gz') as archive:
        members=archive.getmembers()
        require(len(members)==len(expected(root,variant))+1 and {m.name for m in members}==expected(root,variant)|{'prepared.json'})
        require(all(m.isfile() and not m.pax_headers and 0<=m.size<=1024**3 for m in members))
        receipt_member=archive.getmember('prepared.json');require(receipt_member.size<=1024**2)
        receipt=json.load(archive.extractfile(receipt_member))
        require({k:v for k,v in receipt.items() if k not in {'files','variant'}}==identity(root) and receipt['variant']==variant)
        require(set(receipt['files'])==expected(root,variant))
        # Verify every member before installing any executable bytes.
        for name,record in receipt['files'].items():
            member=archive.getmember(name)
            require(member.size==record['size'] and member.mode==record['mode'] and record['mode'] in {0o644,0o755})
            require(sha(archive.extractfile(member).read())==record['sha256'])
            target=root/name
            require(not target.exists() and not any(p.is_symlink() for p in [target,*target.parents]))
        for name,record in receipt['files'].items():
            target=root/name;target.parent.mkdir(parents=True,exist_ok=True)
            with target.open('xb') as stream:stream.write(archive.extractfile(name).read())
            target.chmod(record['mode'])


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command',choices=['pack','unpack']);parser.add_argument('archive',type=Path);parser.add_argument('--sha256');parser.add_argument('--variant',choices=['native','codec'],default='native')
    args=parser.parse_args()
    try:
        if args.command=='pack':
            digest=pack(ROOT,args.archive,args.variant)
            with open(os.environ['GITHUB_OUTPUT'],'a') as output:output.write('sha256='+digest+'\n')
        else:unpack(ROOT,args.archive,args.sha256,args.variant)
    except (OSError,ValueError,KeyError,TypeError,tarfile.TarError):
        raise SystemExit('Prepared input verification failed')
