#!/usr/bin/env python3
"""Keep corresponding source and original notices alongside shared gateway test binaries."""
from pathlib import Path
import subprocess
import sys
import tempfile
import release_package as package
import check_public_boundary

ROOT=Path(__file__).resolve().parents[1]


def collect(root):
    package.require(not package.run(['git','status','--porcelain','--untracked-files=normal'],root),'Prepared inputs require committed source bytes')
    source=package.run(['git','archive','--format=tar.gz','--prefix=agent-response-gateway/','HEAD'],root)
    sysroot=Path(package.run(['rustc','--print','sysroot'],root).decode().strip())
    raw=package.run(['rustc','-vV'],root).decode().strip()
    compiler=dict(line.split(': ',1) for line in raw.splitlines()[1:] if ': ' in line)
    notices,_=package.toolchain_evidence(sysroot,'aarch64-apple-darwin',compiler)
    state=root/'.local/prepared';state.mkdir(parents=True,exist_ok=True)
    with tempfile.TemporaryDirectory(dir=state) as d:
        archived=Path(d)/'source.tar.gz';archived.write_bytes(source)
        check_public_boundary.Checker(root,[]).archive(archived)
        destination=Path(d)/'notices'
        package.run([sys.executable,'-B','scripts/license_audit.py','bundle','--output',destination],root)
        files={'source.tar.gz':(source,0o644),'LICENSE':(package.read(root/'LICENSE'),0o644)}
        for path in destination.rglob('*'):
            if path.is_file():files['license-notices/'+path.relative_to(destination).as_posix()]=(package.read(path),0o644)
        files.update({'rust-notices/'+name:(raw,0o644) for name,raw in notices.items()})
    return package.tar_bytes(files)


if __name__=='__main__':
    try:package.write_new(Path(sys.argv[1]),collect(ROOT))
    except (OSError,ValueError,KeyError,IndexError,package.PackageError,check_public_boundary.BoundaryError):raise SystemExit('Prepared source and notice evidence failed')
