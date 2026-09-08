#!/usr/bin/env python3
"""Package a verified notice bundle without host metadata (Python stdlib only)."""

import argparse
from pathlib import Path
import tarfile


def archive_bundle(source: Path, output: Path) -> int:
    if source.is_symlink() or not source.is_dir():
        raise ValueError("bundle must be a regular directory")
    files = []
    for path in sorted(source.rglob("*")):
        if path.is_symlink() or not (path.is_file() or path.is_dir()):
            raise ValueError("bundle contains a nonregular entry")
        if path.is_file():
            files.append(path)
    if not files or source.resolve() == output.resolve() or source.resolve() in output.resolve().parents:
        raise ValueError("invalid archive destination or empty bundle")
    with tarfile.open(output, "x", format=tarfile.USTAR_FORMAT) as archive:
        for path in files:
            relative = path.relative_to(source).as_posix()
            info = tarfile.TarInfo("license-notices/" + relative)
            info.size = path.stat().st_size
            info.mode = 0o644
            # TarInfo defaults keep uid, gid, mtime at zero and names empty.
            with path.open("rb") as stream:
                archive.addfile(info, stream)
    return len(files)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    try:
        count = archive_bundle(args.source, args.output)
    except (OSError, ValueError, tarfile.TarError):
        parser.exit(1, "Cannot create notice archive; inspect local inputs privately\n")
    print(f"Notice archive created: files={count}")
