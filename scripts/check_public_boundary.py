#!/usr/bin/env python3
"""Check a selected public Git snapshot, reachable history, and optional tar.

Only regular source files are supported. Private markers are exact UTF-8 byte
matches loaded explicitly from a local JSON array; they are never printed.
This is a publication guard, not a general secret detector or access control.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tarfile


RESERVED = {b".private", b".codex", b".git", b".build", b".local", b"target", b".gitmodules"}
MAX_FILE = 64 * 1024 * 1024
MAX_TOTAL = 1024 * 1024 * 1024
MAX_ENTRIES = 100_000


class BoundaryError(Exception):
    """An error whose input details must never be displayed."""


class BoundedTarInfo(tarfile.TarInfo):
    @classmethod
    def frombuf(cls, buf, encoding, errors):
        info = super().frombuf(buf, encoding, errors)
        # Bound extended-name/PAX records before tarfile reads their payloads.
        if info.size < 0 or info.size > MAX_FILE:
            raise BoundaryError
        return info


class Checker:
    def __init__(self, root: Path, patterns: list[bytes]):
        self.root = root
        self.patterns = patterns
        self.total = 0
        self.entries = 0
        self.blobs: set[bytes] = set()

    def git(self, *args: str) -> bytes:
        result = subprocess.run(
            ["git", "-C", str(self.root), *args],
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            check=False,
            env={**os.environ, "GIT_NO_REPLACE_OBJECTS": "1"},
        )
        if result.returncode:
            raise BoundaryError
        return result.stdout

    def content(self, data: bytes) -> None:
        if any(pattern in data for pattern in self.patterns):
            raise BoundaryError

    def path(self, path: bytes, mode: bytes = b"100644") -> None:
        self.entries += 1
        if self.entries > MAX_ENTRIES or mode not in {b"100644", b"100755", b"040000"}:
            raise BoundaryError
        parts = path.split(b"/")
        if (
            not path
            or b"\\" in path
            or b":" in parts[0]
            or any(part.lower() in RESERVED or part in {b"", b".", b".."} for part in parts)
        ):
            raise BoundaryError
        self.content(path)

    def size(self, length: int) -> None:
        if length < 0 or length > MAX_FILE:
            raise BoundaryError
        self.total += length
        if self.total > MAX_TOTAL:
            raise BoundaryError

    def blob(self, oid: bytes) -> None:
        if oid in self.blobs:
            return
        self.blobs.add(oid)
        name = oid.decode("ascii")
        length = int(self.git("cat-file", "-s", name))
        self.size(length)
        data = self.git("cat-file", "blob", name)
        if len(data) != length:
            raise BoundaryError
        self.content(data)

    def index(self) -> list[bytes]:
        paths = []
        for entry in self.git("ls-files", "--stage", "-z").split(b"\0"):
            if not entry:
                continue
            metadata, path = entry.split(b"\t", 1)
            mode, oid, stage = metadata.split()
            self.path(path, mode)
            if stage != b"0":
                raise BoundaryError
            # Read the indexed object, not the potentially different worktree.
            self.blob(oid)
            paths.append(path)
        return paths

    def worktree(self) -> list[bytes]:
        paths = sorted(set(self.git("ls-files", "--cached", "--others", "--exclude-standard", "-z").split(b"\0")) - {b""})
        for path in paths:
            self.path(path)
            target = self.root / os.fsdecode(path)
            # Do not follow a symlink in the file or any parent directory.
            parent = target
            while parent != self.root:
                if parent.is_symlink():
                    raise BoundaryError
                parent = parent.parent
            info = target.stat(follow_symlinks=False)
            if not stat.S_ISREG(info.st_mode):
                raise BoundaryError
            self.size(info.st_size)
            with target.open("rb") as stream:
                data = stream.read(MAX_FILE + 1)
            if len(data) != info.st_size:
                raise BoundaryError
            self.content(data)
        return paths

    def tree(self, oid: bytes) -> None:
        for entry in self.git("ls-tree", "-r", "-t", "-z", oid.decode("ascii")).split(b"\0"):
            if not entry:
                continue
            metadata, path = entry.split(b"\t", 1)
            mode, kind, child = metadata.split()
            self.path(path, mode)
            if kind == b"tree" and mode == b"040000":
                continue
            if kind != b"blob":
                raise BoundaryError
            self.blob(child)

    def history(self) -> int:
        if self.git("rev-parse", "--is-shallow-repository").strip() != b"false":
            raise BoundaryError
        refs = self.git("for-each-ref", "--format=%(refname)%00%(objecttype)%00%(objectname)").splitlines()
        if len(refs) > MAX_ENTRIES:
            raise BoundaryError
        tags: dict[bytes, tuple[bytes, bytes]] = {}
        trees: set[bytes] = set()
        for ref in refs:
            label, kind, oid = ref.split(b"\0")
            self.content(label)
            visited: set[bytes] = set()
            while kind == b"tag":
                if oid in visited:
                    raise BoundaryError
                visited.add(oid)
                if oid not in tags:
                    if len(tags) >= MAX_ENTRIES:
                        raise BoundaryError
                    name = oid.decode("ascii")
                    length = int(self.git("cat-file", "-s", name))
                    self.size(length)
                    data = self.git("cat-file", "tag", name)
                    if len(data) != length:
                        raise BoundaryError
                    self.content(data)
                    headers = dict(line.split(b" ", 1) for line in data.split(b"\n\n", 1)[0].splitlines())
                    tags[oid] = headers[b"object"], headers[b"type"]
                oid, kind = tags[oid]
                if self.git("cat-file", "-t", oid.decode("ascii")).strip() != kind:
                    raise BoundaryError
            if kind == b"tree":
                if oid not in trees:
                    trees.add(oid)
                    self.tree(oid)
            elif kind != b"commit":
                raise BoundaryError
        # --all does not include a detached HEAD. An unborn HEAD yields no revision.
        head = self.git("rev-parse", "--revs-only", "HEAD").splitlines()
        commits = self.git("rev-list", "--all", *(oid.decode("ascii") for oid in head)).splitlines()
        if len(commits) > MAX_ENTRIES:
            raise BoundaryError
        for commit in commits:
            name = commit.decode("ascii")
            # Messages and identities can also disclose configured markers.
            self.content(self.git("cat-file", "commit", name))
            self.tree(commit)
        return len(commits)

    def archive(self, archive: Path) -> int:
        count = 0
        names: set[bytes] = set()
        with tarfile.open(archive, mode="r|*", tarinfo=BoundedTarInfo) as source:
            for member in source:
                self.content(json.dumps(member.pax_headers, ensure_ascii=False).encode("utf-8"))
                self.content(member.uname.encode("utf-8"))
                self.content(member.gname.encode("utf-8"))
                path = member.name.rstrip("/").encode("utf-8")
                if path in names:
                    raise BoundaryError
                names.add(path)
                if member.isdir():
                    self.path(path, b"040000")
                    if member.size:
                        raise BoundaryError
                    continue
                self.path(path)
                if member.type not in {tarfile.REGTYPE, tarfile.AREGTYPE} or member.sparse is not None:
                    raise BoundaryError
                self.size(member.size)
                stream = source.extractfile(member)
                if stream is None:
                    raise BoundaryError
                with stream:
                    data = stream.read(MAX_FILE + 1)
                if len(data) != member.size:
                    raise BoundaryError
                self.content(data)
                count += 1
        if not count:
            raise BoundaryError
        return count


def load_patterns(path: Path | None) -> list[bytes]:
    if path is None:
        return []
    if path.stat().st_size > 1024 * 1024:
        raise BoundaryError
    entries = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(entries, list) or not entries or any(not isinstance(item, str) or not item for item in entries):
        raise BoundaryError
    return [item.encode("utf-8") for item in entries]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--worktree", action="store_true", help="also inspect untracked, non-ignored working files")
    parser.add_argument("--private-patterns", type=Path, help="explicit local JSON array of exact private markers")
    parser.add_argument("--archive", type=Path, help="also inspect a tar or tar.gz source artifact")
    args = parser.parse_args()
    try:
        checker = Checker(args.root.resolve(), load_patterns(args.private_patterns))
        if Path(os.fsdecode(checker.git("rev-parse", "--show-toplevel").strip())).resolve() != checker.root:
            raise BoundaryError
        indexed = checker.index()
        selected = checker.worktree() if args.worktree else indexed
        if not selected:
            raise BoundaryError
        commits = checker.history()
        archive_files = checker.archive(args.archive) if args.archive else 0
    except (BoundaryError, OSError, EOFError, ValueError, KeyError, UnicodeError, tarfile.TarError):
        print("Public boundary check failed; inspect selected files and local policy privately.", file=sys.stderr)
        return 1
    print(f"Public boundary check passed: files={len(selected)} commits={commits} archive_files={archive_files}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
