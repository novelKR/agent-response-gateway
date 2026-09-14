#!/usr/bin/env python3
"""Assemble an offline, source-inclusive native plugin tooling distribution."""
import argparse
import hashlib
import io
from pathlib import Path
import re
import tarfile

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_BASE = 'python:3.14.0-slim-bookworm@sha256:d13fa0424035d290decef3d575cea23d1b7d5952cdf429df8f5542c71e961576'


def assemble(output, base_image, conformance=None):
    # No registry resolution or floating base fallback during assembly/build.
    if not re.fullmatch(r'python:3\.(?:11|12|13|14)\.[0-9]+-slim-bookworm@sha256:[a-f0-9]{64}', base_image):
        raise ValueError('Use an exact supported Python slim-bookworm tag and registry digest')
    members = {
        'extension_manager.py': (ROOT / 'scripts/extension_manager.py').read_bytes(),
        'LICENSE': (ROOT / 'LICENSE').read_bytes(),
        'README.md': (ROOT / 'tooling/plugin-tools/README.md').read_bytes(),
        'build_plugin_tools.py': Path(__file__).read_bytes(),
    }
    if conformance is not None:
        members['plugin_conformance.py'] = conformance.read_bytes()
    copies = ' '.join(name for name in members if name.endswith('.py'))
    members['Dockerfile'] = (
        f'FROM {base_image}\n'
        'ENV PYTHONDONTWRITEBYTECODE=1 PYTHONUNBUFFERED=1\n'
        'WORKDIR /opt/plugin-tools\n'
        f'COPY {copies} /opt/plugin-tools/\n'
        'COPY LICENSE README.md /opt/plugin-tools/\n'
        'USER 65532:65532\n'
        'ENTRYPOINT ["python3", "-I", "-B", "/opt/plugin-tools/extension_manager.py"]\n'
    ).encode()
    members['SHA256SUMS'] = ''.join(
        f'{hashlib.sha256(raw).hexdigest()}  {name}\n'
        for name, raw in sorted(members.items())
    ).encode()
    # Flat allowlist only: no checkout, private records, build cache or credentials.
    with output.open('xb') as stream:
        with tarfile.open(fileobj=stream, mode='w', format=tarfile.USTAR_FORMAT) as archive:
            for name, raw in sorted(members.items()):
                info = tarfile.TarInfo(name)
                info.size = len(raw)
                info.mode = 0o644
                info.mtime = 0
                archive.addfile(info, io.BytesIO(raw))
    return hashlib.sha256(output.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--base-image', default=DEFAULT_BASE)
    parser.add_argument('--conformance', type=Path, help='Explicit standalone standard-library conformance script')
    args = parser.parse_args()
    try:
        print(assemble(args.output, args.base_image, args.conformance))
    except (OSError, ValueError):
        parser.exit(1, 'Tool distribution assembly failed; check inputs and output destination\n')


if __name__ == '__main__':
    main()
