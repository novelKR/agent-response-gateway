#!/usr/bin/env python3
"""Offline, explicit installation and activation of trusted native observers.

No package code, shell, login flow, network request or post-install hook is run.
The digest must come from a trusted distribution channel; it is not a signature.
Python 3.11+, Linux/macOS. Other platforms fail closed rather than skip ACL checks.
"""
from __future__ import annotations

import argparse
from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import stat
import sys
import uuid

PACKAGE_SCHEMA = 'gateway-extension-package/v1'
LOCK_SCHEMA = 'gateway-extension-lock/v1'
PROTOCOL = 'gateway-observer/v1'
PERMISSIONS = ['observe_http_metadata', 'write_private_state']
MAX_JSON = 65536
MAX_BINARY = 128 * 1024 * 1024
MAX_NOTICE = 256 * 1024
MAX_EXTENSIONS = 4


class ExtensionError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise ExtensionError(message)


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def canonical(value):
    return (json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=True) + '\n').encode()


def hex_digest(value):
    return isinstance(value, str) and re.fullmatch(r'[a-f0-9]{64}', value) is not None


def identifier(value):
    return isinstance(value, str) and re.fullmatch(r'[a-z][a-z0-9-]{0,63}', value) is not None


def version(value):
    return isinstance(value, str) and re.fullmatch(r'(0|[1-9][0-9]{0,5})\.(0|[1-9][0-9]{0,5})\.(0|[1-9][0-9]{0,5})', value) is not None


def target():
    require(sys.platform in ('linux', 'darwin'), 'Native extensions require Linux or macOS')
    arch = {'x86_64': 'x64', 'amd64': 'x64', 'aarch64': 'arm64', 'arm64': 'arm64'}.get(platform.machine().lower())
    require(arch is not None, 'Unsupported extension architecture')
    return ('macos' if sys.platform == 'darwin' else 'linux') + '-' + arch


def no_links(path):
    """Reject lexical escapes and links, including links in ancestor directories."""
    path = Path(path)
    require(path.is_absolute() and '..' not in path.parts, 'Use an absolute path without traversal')
    for part in [*reversed(path.parents), path]:
        require(not part.is_symlink(), 'Extension paths cannot contain symlinks')
    return path


def private_dir(path, *, create=False):
    path = no_links(path)
    if create:
        path.mkdir(mode=0o700, exist_ok=True)
    info = path.stat()
    require(stat.S_ISDIR(info.st_mode) and info.st_uid == os.getuid()
            and info.st_mode & 0o077 == 0, 'Extension directories must be private and user-owned')
    return path


def read_file(path, maximum, *, private=False):
    path = no_links(path)
    before = path.lstat()
    require(stat.S_ISREG(before.st_mode) and before.st_nlink == 1, 'Expected an unlinked regular file')
    flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK
    fd = os.open(path, flags)
    with os.fdopen(fd, 'rb') as stream:
        info = os.fstat(stream.fileno())
        require((info.st_dev, info.st_ino) == (before.st_dev, before.st_ino), 'File changed during inspection')
        if private:
            require(info.st_uid == os.getuid() and info.st_mode & 0o077 == 0, 'Extension files must be private and user-owned')
        require(info.st_size <= maximum, 'Extension file exceeds its size limit')
        raw = stream.read(maximum + 1)
        require(len(raw) <= maximum, 'Extension file exceeds its size limit')
        return raw


def decode_json(raw):
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, 'Duplicate JSON field')
            result[key] = value
        return result
    return json.loads(raw, object_pairs_hook=pairs, parse_constant=lambda _: (_ for _ in ()).throw(ExtensionError('Invalid JSON number')))


def validate_package(raw):
    package = decode_json(raw)
    require(isinstance(package, dict) and set(package) == {
        'schema', 'id', 'version', 'target', 'protocol', 'permissions', 'state_schema', 'files'
    }, 'Invalid package manifest fields')
    require(package['schema'] == PACKAGE_SCHEMA and package['protocol'] == PROTOCOL, 'Unsupported package protocol')
    require(identifier(package['id']) and version(package['version']), 'Invalid package identity')
    require(package['target'] == target(), 'Package target does not match this host')
    require(package['permissions'] == PERMISSIONS, 'Unsupported package permissions')
    require(package['state_schema'] == 'observer-state/v1', 'Unsupported observer state schema')
    entries = package['files']
    require(isinstance(entries, dict) and 2 <= len(entries) <= 8
            and {'extension', 'LICENSE.txt'} <= entries.keys(), 'Executable and license evidence are required')
    for name, value in entries.items():
        require(isinstance(name, str) and re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_.-]{0,63}', name)
                and name not in ('extension.json', '.', '..') and hex_digest(value), 'Invalid package file record')
    require(canonical(package) == raw, 'Package JSON must use canonical sorted keys and a terminal newline')
    return package


def inspect_package(directory, expected, *, private=False):
    require(hex_digest(expected), 'An exact trusted manifest SHA-256 is required')
    directory = no_links(directory)
    if private:
        private_dir(directory)
    raw = read_file(directory / 'extension.json', MAX_JSON, private=private)
    require(digest(raw) == expected, 'Package manifest digest mismatch')
    package = validate_package(raw)
    require({p.name for p in directory.iterdir()} == {'extension.json', *package['files']}, 'Package contains missing or unlisted files')
    contents = {'extension.json': raw}
    for name, expected_file in package['files'].items():
        content = read_file(directory / name, MAX_BINARY if name == 'extension' else MAX_NOTICE, private=private)
        require(digest(content) == expected_file, 'Package file digest mismatch')
        contents[name] = content
    return package, contents


def write_new(path, raw, mode=0o600):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, mode)
    with os.fdopen(fd, 'wb') as stream:
        stream.write(raw)
        stream.flush()
        os.fsync(stream.fileno())


def sync_dir(path):
    fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def open_store(path):
    target()  # Must happen before any filesystem mutation on an unsupported platform.
    root = private_dir(path, create=True)
    for name in ('packages', 'state'):
        private_dir(root / name, create=True)
    return root


@contextmanager
def mutation_lock(root):
    import fcntl
    path = root / '.manager.lock'
    fd = os.open(path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_NONBLOCK, 0o600)
    try:
        info = os.fstat(fd)
        require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and info.st_uid == os.getuid()
                and info.st_mode & 0o077 == 0, 'Unsafe management lock')
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise ExtensionError('Another extension mutation is active') from error
        yield
    finally:
        os.close(fd)


def read_lock(root):
    path = root / 'active.json'
    if not path.exists() and not path.is_symlink():
        return {'schema': LOCK_SCHEMA, 'generation': 0, 'extensions': []}
    raw = read_file(path, MAX_JSON, private=True)
    lock = decode_json(raw)
    require(isinstance(lock, dict) and set(lock) == {'schema', 'generation', 'extensions'}
            and lock['schema'] == LOCK_SCHEMA and type(lock['generation']) is int
            and 0 <= lock['generation'] < 2**64, 'Invalid activation lock')
    require(isinstance(lock['extensions'], list) and len(lock['extensions']) <= MAX_EXTENSIONS, 'Too many extensions')
    ids = set()
    for entry in lock['extensions']:
        require(isinstance(entry, dict) and set(entry) == {'id', 'version', 'package_sha256', 'grants'}
                and identifier(entry['id']) and version(entry['version'])
                and hex_digest(entry['package_sha256']) and entry['grants'] == PERMISSIONS
                and entry['id'] not in ids, 'Invalid activation entry')
        ids.add(entry['id'])
    require(lock['extensions'] == sorted(lock['extensions'], key=lambda e: e['id']), 'Activation entries must be ordered')
    require(canonical(lock) == raw, 'Activation JSON must be canonical')
    return lock


def commit_lock(root, lock):
    require(lock['generation'] < 2**64 - 1, 'Activation generation is exhausted')
    lock = {**lock, 'generation': lock['generation'] + 1,
            'extensions': sorted(lock['extensions'], key=lambda e: e['id'])}
    temporary = root / ('.active-' + uuid.uuid4().hex)
    try:
        write_new(temporary, canonical(lock))
        os.replace(temporary, root / 'active.json')
        sync_dir(root)
    finally:
        temporary.unlink(missing_ok=True)
    return lock


def installed_dir(root, package_id, package_version, sha):
    require(identifier(package_id) and version(package_version) and hex_digest(sha), 'Invalid installed package selector')
    return root / 'packages' / package_id / package_version / sha


def install(root, directory, expected):
    root = open_store(root)
    package, contents = inspect_package(directory, expected)
    with mutation_lock(root):
        identity = private_dir(root / 'packages' / package['id'], create=True)
        versions = private_dir(identity / package['version'], create=True)
        destination = versions / expected
        if destination.exists() or destination.is_symlink():
            inspect_package(destination, expected, private=True)
            return package
        temporary = private_dir(versions / ('.install-' + uuid.uuid4().hex), create=True)
        try:
            for name, content in contents.items():
                write_new(temporary / name, content, 0o500 if name == 'extension' else 0o400)
            sync_dir(temporary)
            os.rename(temporary, destination)
            sync_dir(versions)
        finally:
            if temporary.exists():
                shutil.rmtree(temporary)
        # Installation never activates and never reads or rotates account credentials.
        return package


def enable(root, package_id, package_version, sha, grants):
    root = open_store(root)
    require(sorted(grants) == PERMISSIONS, 'Explicit permission approval is required')
    with mutation_lock(root):
        directory = installed_dir(root, package_id, package_version, sha)
        package, _ = inspect_package(directory, sha, private=True)
        require((package['id'], package['version']) == (package_id, package_version), 'Installed identity mismatch')
        lock = read_lock(root)
        entries = [entry for entry in lock['extensions'] if entry['id'] != package_id]
        require(len(entries) < MAX_EXTENSIONS, 'Too many active extensions')
        state_parent = private_dir(root / 'state' / package_id, create=True)
        private_dir(state_parent / sha, create=True)
        entries.append({'id': package_id, 'version': package_version, 'package_sha256': sha, 'grants': PERMISSIONS})
        lock['extensions'] = entries
        return commit_lock(root, lock)


def disable(root, package_id):
    root = open_store(root)
    require(identifier(package_id), 'Invalid package identity')
    with mutation_lock(root):
        lock = read_lock(root)
        require(any(e['id'] == package_id for e in lock['extensions']), 'Extension is not enabled')
        lock['extensions'] = [e for e in lock['extensions'] if e['id'] != package_id]
        return commit_lock(root, lock)


def package_binary(binary, license_file, output, package_id, package_version):
    """Build a flat local package from explicitly supplied bytes; never execute them."""
    host = target()
    require(identifier(package_id) and version(package_version), 'Invalid package identity')
    binary_bytes = read_file(binary, MAX_BINARY)
    license_bytes = read_file(license_file, MAX_NOTICE)
    output = no_links(output)
    require(not output.exists(), 'Package output already exists')
    manifest = {'schema': PACKAGE_SCHEMA, 'id': package_id, 'version': package_version,
                'target': host, 'protocol': PROTOCOL, 'permissions': PERMISSIONS,
                'state_schema': 'observer-state/v1',
                'files': {'extension': digest(binary_bytes), 'LICENSE.txt': digest(license_bytes)}}
    raw = canonical(manifest)
    output.mkdir(mode=0o700)
    write_new(output / 'extension', binary_bytes, 0o500)
    write_new(output / 'LICENSE.txt', license_bytes, 0o400)
    write_new(output / 'extension.json', raw, 0o400)
    sync_dir(output)
    return digest(raw)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    build = commands.add_parser('package')
    for name in ('binary', 'license-file', 'output'):
        build.add_argument('--' + name, type=Path, required=True)
    build.add_argument('--id', required=True)
    build.add_argument('--version', required=True)
    inspect = commands.add_parser('inspect')
    inspect.add_argument('--package', type=Path, required=True)
    inspect.add_argument('--expected-sha256', required=True)
    for command in ('install', 'enable', 'disable', 'status'):
        sub = commands.add_parser(command)
        sub.add_argument('--store', type=Path, required=True)
        if command == 'install':
            sub.add_argument('--package', type=Path, required=True)
            sub.add_argument('--expected-sha256', required=True)
        if command in ('enable', 'disable'):
            sub.add_argument('--id', required=True)
        if command == 'enable':
            sub.add_argument('--version', required=True)
            sub.add_argument('--package-sha256', required=True)
            sub.add_argument('--grant', action='append', default=[])
    args = parser.parse_args(argv)
    try:
        target()
        if args.command == 'package':
            result = {'package_sha256': package_binary(args.binary, args.license_file, args.output, args.id, args.version)}
        elif args.command == 'inspect':
            package, _ = inspect_package(args.package, args.expected_sha256)
            result = {'package': package, 'executed': False}
        elif args.command == 'install':
            package = install(args.store, args.package, args.expected_sha256)
            result = {'id': package['id'], 'version': package['version'], 'installed': True, 'activated': False}
        elif args.command == 'enable':
            result = enable(args.store, args.id, args.version, args.package_sha256, args.grant)
        elif args.command == 'disable':
            result = disable(args.store, args.id)
        else:
            root = private_dir(args.store)
            result = {'activation': read_lock(root), 'runtime_checked': False}
        print(canonical(result).decode(), end='')
        return 0
    except (OSError, ValueError, TypeError, KeyError):
        # Even a parser/filesystem error must not echo a path or supplied document.
        print('Extension operation failed; verify package, permissions, target and store', file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
