#!/usr/bin/env python3
"""Prepare or verify the pinned, test-only Codex executable; no model calls."""

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import subprocess
import tarfile
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
LOCK = ROOT / "tests/codex/runtime-lock.json"
BUNDLE = ROOT / ".local/codex-runtime"


def require(ok, reason):
    if not ok:
        raise ValueError(reason)


def check_file(path, expected):
    require(path.is_file() and not path.is_symlink(), "required regular file missing")
    require(path.stat().st_size == expected["size"], "pinned file size mismatch")
    with path.open("rb") as stream:
        actual = hashlib.file_digest(stream, "sha256").hexdigest()
    require(actual == expected["sha256"], "pinned file digest mismatch")


def safe_member(name):
    path = PurePosixPath(name)
    require(name and not path.is_absolute() and "\\" not in name, "invalid member path")
    require(all(part not in {"", ".", ".."} for part in name.split("/")), "invalid member path")
    return path


def verify_bundle(destination, lock):
    require(not destination.is_symlink() and destination.is_dir(), "runtime bundle missing")
    for name, expected in lock["files"].items():
        safe_member(name)
        path = destination / name
        for parent in path.parents:
            if parent == destination:
                break
            require(not parent.is_symlink(), "runtime path contains symlink")
        check_file(path, expected)
        require(bool(path.stat().st_mode & 0o111) == expected["executable"], "runtime file mode mismatch")
    return destination / lock["entrypoint"]


def install_archive(archive_path, destination, lock):
    check_file(archive_path, lock["archive"])
    if destination.exists():
        return verify_bundle(destination, lock)
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="codex-install-", dir=destination.parent) as temporary:
        stage = Path(temporary) / "bundle"
        stage.mkdir()
        seen = set()
        with tarfile.open(archive_path, "r:gz") as archive:
            for member in archive:
                safe_member(member.name)
                require(member.name not in seen, "duplicate archive member")
                seen.add(member.name)
                if member.isdir():
                    require(any(name.startswith(member.name + "/") for name in lock["files"]), "unexpected archive directory")
                    continue
                require(member.isfile() and member.name in lock["files"], "unexpected archive member")
                expected = lock["files"][member.name]
                require(member.size == expected["size"], "archive member size mismatch")
                output = stage / member.name
                output.parent.mkdir(parents=True, exist_ok=True)
                source = archive.extractfile(member)
                require(source is not None, "archive member unavailable")
                with source, output.open("xb") as target:
                    remaining = expected["size"]
                    while remaining:
                        chunk = source.read(min(1024 * 1024, remaining))
                        require(bool(chunk), "truncated archive member")
                        target.write(chunk)
                        remaining -= len(chunk)
                output.chmod(0o755 if expected["executable"] else 0o644)
        verify_bundle(stage, lock)
        require(not destination.exists(), "runtime destination changed")
        stage.rename(destination)
    return verify_bundle(destination, lock)


def download_archive(lock, output):
    url = lock["archive"]["url"]
    require(url.startswith("https://github.com/openai/codex/releases/download/"), "unreviewed download origin")
    # Explicit preparation only. Ignore ambient proxy credentials and never log bodies.
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    with opener.open(url, timeout=60) as response, output.open("xb") as stream:
        total = 0
        while chunk := response.read(1024 * 1024):
            total += len(chunk)
            require(total <= lock["archive"]["size"], "download exceeds pinned size")
            stream.write(chunk)
    check_file(output, lock["archive"])


def generate_schemas(binary, destination, lock, profile):
    require(platform.system() == "Darwin" and platform.machine() == "arm64", "schema execution requires the pinned macOS ARM64 target")
    require(not destination.exists(), "schema destination already exists")
    state = ROOT / ".local"
    state.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="schema-home-", dir=state) as home:
        env = {k: v for k, v in os.environ.items() if k in {"HOME", "PATH", "TMPDIR", "LANG"}}
        env["CODEX_HOME"] = home
        command = [str(binary), "app-server", "generate-json-schema", "--out", str(destination)]
        if profile == "experimental":
            command.append("--experimental")
        result = subprocess.run(command, cwd=home, env=env, capture_output=True, timeout=60)
        require(result.returncode == 0, "schema generation failed")
    check_file(destination / lock["schemas"][profile]["file"], lock["schemas"][profile])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("prepare", "verify", "schema"))
    parser.add_argument("--archive", type=Path, help="use an existing pinned archive instead of downloading")
    parser.add_argument("--profile", choices=("stable", "experimental"), default="experimental")
    parser.add_argument("--output", type=Path, help="new schema output directory under local test state")
    args = parser.parse_args()
    try:
        require(args.command == "prepare" or args.archive is None, "archive is only valid for preparation")
        require(args.command == "schema" or args.output is None, "output is only valid for schema generation")
        lock = json.loads(LOCK.read_text())
        require(lock["schema_version"] == 1, "unsupported runtime lock schema")
        if args.command == "prepare":
            if args.archive:
                install_archive(args.archive, BUNDLE, lock)
            elif BUNDLE.exists():
                verify_bundle(BUNDLE, lock)
            else:
                local = ROOT / ".local"
                local.mkdir(exist_ok=True)
                with tempfile.TemporaryDirectory(prefix="codex-download-", dir=local) as temporary:
                    archive = Path(temporary) / "runtime.tar.gz"
                    download_archive(lock, archive)
                    install_archive(archive, BUNDLE, lock)
        else:
            binary = verify_bundle(BUNDLE, lock)
            if args.command == "schema":
                output = args.output or ROOT / ".local" / ("codex-schema-" + args.profile)
                generate_schemas(binary, output, lock, args.profile)
        print(f"Pinned Codex {args.command} passed: version={lock['version']} target={lock['target']}")
    except (OSError, ValueError, KeyError, tarfile.TarError, subprocess.TimeoutExpired):
        parser.exit(1, "Pinned Codex validation failed; inspect local inputs privately\n")


if __name__ == "__main__":
    main()
