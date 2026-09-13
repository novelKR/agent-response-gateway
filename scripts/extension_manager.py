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

CODEC_PROTOCOL = 'gateway-api-codec/v1'
EDITING_CODEC_PROTOCOL = 'gateway-api-codec/v2'
CODEC_PROTOCOLS = (CODEC_PROTOCOL, EDITING_CODEC_PROTOCOL)
CODEC_PERMISSIONS = ['read_model_payload', 'transform_model_protocol']
PACKAGE_SCHEMA = 'gateway-extension-package/v1'
LOCK_SCHEMA = 'gateway-extension-lock/v1'
PROTOCOL = 'gateway-observer/v1'
PERMISSIONS = ['observe_http_metadata', 'write_private_state']
RECORDER_PROTOCOL = 'gateway-usage-recorder/v1'
RECORDER_PERMISSIONS = ['export_usage', 'observe_usage', 'write_usage_store']
MAX_JSON = 65536
MAX_BINARY = 128 * 1024 * 1024
MAX_NOTICE = 256 * 1024
MAX_EXTENSIONS = 4


class ExtensionError(ValueError):
    pass


class ExtensionConflict(ExtensionError):
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
    require(sys.version_info >= (3, 11), 'Native extension management requires Python 3.11+')
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


def read_file(path, maximum, *, private=False, allow_build_hardlinks=False):
    path = no_links(path)
    before = path.lstat()
    require(stat.S_ISREG(before.st_mode) and (allow_build_hardlinks or before.st_nlink == 1),
            'Expected a regular file with an allowed link count')
    flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK
    fd = os.open(path, flags)
    with os.fdopen(fd, 'rb') as stream:
        info = os.fstat(stream.fileno())
        require((info.st_dev, info.st_ino) == (before.st_dev, before.st_ino), 'File changed during inspection')
        require(stat.S_ISREG(info.st_mode) and (allow_build_hardlinks or info.st_nlink == 1),
                'Opened file has an invalid type or link count')
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
    require(package['schema'] == PACKAGE_SCHEMA and package['protocol'] in (PROTOCOL, RECORDER_PROTOCOL, *CODEC_PROTOCOLS), 'Unsupported package protocol')
    require(identifier(package['id']) and version(package['version']), 'Invalid package identity')
    require(package['target'] == target(), 'Package target does not match this host')
    require(package['permissions'] == (PERMISSIONS if package['protocol'] == PROTOCOL else CODEC_PERMISSIONS if package['protocol'] in CODEC_PROTOCOLS else RECORDER_PERMISSIONS), 'Unsupported package permissions')
    require(package['state_schema'] == ('observer-state/v1' if package['protocol'] == PROTOCOL else 'request-memory/v1' if package['protocol'] in CODEC_PROTOCOLS else 'usage-store/v1'), 'Unsupported observer state schema')
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


def validate_binding(binding):
    require(isinstance(binding, dict) and set(binding) == {'store_id', 'mode', 'queue_capacity', 'ack_timeout_ms', 'config_sha256'}, 'Invalid recorder binding')
    require(identifier(binding['store_id']) and binding['mode'] in ('off', 'best_effort', 'durable_local')
            and type(binding['queue_capacity']) is int and 2 <= binding['queue_capacity'] <= 4096
            and type(binding['ack_timeout_ms']) is int and 1 <= binding['ack_timeout_ms'] <= 60000
            and hex_digest(binding['config_sha256']), 'Invalid recorder limits or identity')


def read_lock(root):
    path = root / 'active.json'
    if not path.exists() and not path.is_symlink():
        return {'schema': LOCK_SCHEMA, 'generation': 0, 'extensions': []}
    raw = read_file(path, MAX_JSON, private=True)
    lock = decode_json(raw)
    require(isinstance(lock, dict) and set(lock) == ({'schema', 'generation', 'extensions', 'recorder'} if 'recorder' in lock else {'schema', 'generation', 'extensions'})
            and lock['schema'] == ('gateway-extension-lock/v2' if 'recorder' in lock else LOCK_SCHEMA) and type(lock['generation']) is int
            and 0 <= lock['generation'] < 2**64, 'Invalid activation lock')
    require(isinstance(lock['extensions'], list) and len(lock['extensions']) <= MAX_EXTENSIONS, 'Too many extensions')
    ids = set()
    for entry in lock['extensions']:
        require(isinstance(entry, dict) and set(entry) == {'id', 'version', 'package_sha256', 'grants'}
                and identifier(entry['id']) and version(entry['version'])
                and hex_digest(entry['package_sha256']) and entry['grants'] in (PERMISSIONS, RECORDER_PERMISSIONS, CODEC_PERMISSIONS)
                and entry['id'] not in ids, 'Invalid activation entry')
        ids.add(entry['id'])
    require(lock['extensions'] == sorted(lock['extensions'], key=lambda e: e['id']), 'Activation entries must be ordered')
    if 'recorder' in lock:
        validate_binding(lock['recorder'])
    require(sum(e['grants'] == RECORDER_PERMISSIONS for e in lock['extensions']) == int('recorder' in lock), 'Invalid recorder binding')
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


def existing_store(path):
    target()
    root = private_dir(path)
    for name in ('packages', 'state'):
        private_dir(root / name)
    return root


def inventory_unlocked(root):
    """Bounded installed/selected inventory; never infer a running process."""
    installed = []
    budget = 1024

    def entries(directory):
        nonlocal budget
        found = []
        for item in directory.iterdir():
            budget -= 1
            require(budget >= 0, 'Extension inventory exceeds its bound')
            found.append(item)
        return sorted(found, key=lambda p: p.name)

    activation = read_lock(root)
    for identity in entries(root / 'packages'):
        require(identifier(identity.name), 'Unrecognized package artifact; inspect the store')
        private_dir(identity)
        for release in entries(identity):
            require(version(release.name), 'Unrecognized package artifact; inspect the store')
            private_dir(release)
            for location in entries(release):
                require(hex_digest(location.name), 'Unrecognized package artifact; inspect the store')
                require(len(installed) < 128, 'Extension inventory exceeds its bound')
                package = None
                try:
                    package, _ = inspect_package(location, location.name, private=True)
                    require((package['id'], package['version']) == (identity.name, release.name), 'Installed identity mismatch')
                except (OSError, ValueError, TypeError, KeyError):
                    package = None
                installed.append({'id': identity.name, 'version': release.name,
                                  'package_sha256': location.name, 'verified': package is not None,
                                  'package': package})
    return {'schema': 'gateway-native-inventory/v1', 'activation': activation,
            'installed': installed, 'runtime_checked': False, 'removal_supported': False}


def inventory(root):
    root = existing_store(root)
    with mutation_lock(root):
        value = inventory_unlocked(root)
        return {'inventory': value, 'generation': value['activation']['generation'],
                'inventory_sha256': digest(canonical(value))}


def check_expected(root, expected):
    """Only call while holding the same mutation lock as legacy CLI writers."""
    if expected is None:
        return
    require(isinstance(expected, dict) and set(expected) == {'generation', 'inventory_sha256'}
            and type(expected['generation']) is int and 0 <= expected['generation'] < 2**64
            and hex_digest(expected['inventory_sha256']), 'Invalid inventory precondition')
    value = inventory_unlocked(root)
    if (value['activation']['generation'] != expected['generation']
            or digest(canonical(value)) != expected['inventory_sha256']):
        raise ExtensionConflict('Extension inventory changed')


def guarded_result(root, result, condition):
    if condition is None:
        return result
    value = inventory_unlocked(root)
    return {'result': result, 'after': {'inventory': value, 'generation': value['activation']['generation'],
                                      'inventory_sha256': digest(canonical(value))}}


def install(root, directory, expected, *, condition=None):
    root = existing_store(root) if condition is not None else open_store(root)
    package, contents = inspect_package(directory, expected)
    with mutation_lock(root):
        check_expected(root, condition)
        identity = private_dir(root / 'packages' / package['id'], create=True)
        versions = private_dir(identity / package['version'], create=True)
        destination = versions / expected
        if destination.exists() or destination.is_symlink():
            inspect_package(destination, expected, private=True)
            return guarded_result(root, package, condition)
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
        return guarded_result(root, package, condition)


def enable(root, package_id, package_version, sha, grants, recorder=None, *, condition=None):
    root = existing_store(root) if condition is not None else open_store(root)
    require(sorted(grants) in (PERMISSIONS, RECORDER_PERMISSIONS, CODEC_PERMISSIONS), 'Explicit permission approval is required')
    with mutation_lock(root):
        check_expected(root, condition)
        directory = installed_dir(root, package_id, package_version, sha)
        package, _ = inspect_package(directory, sha, private=True)
        require((package['id'], package['version']) == (package_id, package_version), 'Installed identity mismatch')
        require(sorted(grants) == package['permissions'], 'Package grants mismatch')
        lock = read_lock(root)
        entries = [entry for entry in lock['extensions'] if entry['id'] != package_id]
        require(len(entries) < MAX_EXTENSIONS, 'Too many active extensions')
        state_parent = private_dir(root / 'state' / package_id, create=True)
        private_dir(state_parent / sha, create=True)
        entries.append({'id': package_id, 'version': package_version, 'package_sha256': sha, 'grants': sorted(grants)})
        if package['protocol'] == RECORDER_PROTOCOL:
            require(recorder is not None, 'Explicit recorder binding is required')
            validate_binding(recorder)
            require(not any(e['grants'] == RECORDER_PERMISSIONS for e in entries if e['id'] != package_id), 'Only one recorder is supported')
            usage_root = private_dir(root / 'usage', create=True)
            state = private_dir(usage_root / recorder['store_id'])
            require(digest(read_file(state / 'recorder.json', MAX_JSON, private=True)) == recorder['config_sha256'], 'Recorder configuration digest mismatch')
            lock['recorder'] = recorder
            lock['schema'] = 'gateway-extension-lock/v2'
        else:
            require(recorder is None, 'Observer cannot use recorder binding')
        if not any(entry['grants'] == RECORDER_PERMISSIONS for entry in entries):
            lock.pop('recorder', None)
            lock['schema'] = LOCK_SCHEMA
        lock['extensions'] = entries
        return guarded_result(root, commit_lock(root, lock), condition)


def disable(root, package_id, *, condition=None):
    root = existing_store(root) if condition is not None else open_store(root)
    require(identifier(package_id), 'Invalid package identity')
    with mutation_lock(root):
        check_expected(root, condition)
        lock = read_lock(root)
        require(any(e['id'] == package_id for e in lock['extensions']), 'Extension is not enabled')
        lock['extensions'] = [e for e in lock['extensions'] if e['id'] != package_id]
        if not any(e['grants'] == RECORDER_PERMISSIONS for e in lock['extensions']):
            lock.pop('recorder', None)
            lock['schema'] = LOCK_SCHEMA
        return guarded_result(root, commit_lock(root, lock), condition)


def package_binary(binary, license_file, output, package_id, package_version, role="http_metadata_observer", codec_protocol=CODEC_PROTOCOL):
    """Build a flat local package from explicitly supplied bytes; never execute them."""
    host = target()
    require(identifier(package_id) and version(package_version), 'Invalid package identity')
    # Cargo may hard-link its explicit build output. Copy those bytes, never that inode.
    # Package inspection, installation and runtime verification still reject hard links.
    binary_bytes = read_file(binary, MAX_BINARY, allow_build_hardlinks=True)
    license_bytes = read_file(license_file, MAX_NOTICE)
    output = no_links(output)
    require(not output.exists(), 'Package output already exists')
    require(role in ('http_metadata_observer', 'usage_recorder', 'api_codec'), 'Unsupported role')
    require(codec_protocol in CODEC_PROTOCOLS and (role == 'api_codec' or codec_protocol == CODEC_PROTOCOL), 'Unsupported codec protocol selection')
    recorder = role == 'usage_recorder'
    codec = role == 'api_codec'
    manifest = {'schema': PACKAGE_SCHEMA, 'id': package_id, 'version': package_version,
                'target': host, 'protocol': codec_protocol if codec else RECORDER_PROTOCOL if recorder else PROTOCOL, 'permissions': CODEC_PERMISSIONS if codec else RECORDER_PERMISSIONS if recorder else PERMISSIONS,
                'state_schema': 'request-memory/v1' if codec else 'usage-store/v1' if recorder else 'observer-state/v1',
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
    build.add_argument('--role', choices=['http_metadata_observer', 'usage_recorder', 'api_codec'], default='http_metadata_observer')
    build.add_argument('--codec-protocol', choices=CODEC_PROTOCOLS, default=CODEC_PROTOCOL)
    inspect = commands.add_parser('inspect')
    inspect.add_argument('--package', type=Path, required=True)
    inspect.add_argument('--expected-sha256', required=True)
    for command in ('install', 'enable', 'disable', 'status', 'inventory'):
        sub = commands.add_parser(command)
        sub.add_argument('--store', type=Path, required=True)
        if command in ('install', 'enable', 'disable'):
            sub.add_argument('--expected-generation', type=int)
            sub.add_argument('--expected-inventory-sha256')
        if command == 'install':
            sub.add_argument('--package', type=Path, required=True)
            sub.add_argument('--expected-sha256', required=True)
        if command in ('enable', 'disable'):
            sub.add_argument('--id', required=True)
        if command == 'enable':
            sub.add_argument('--version', required=True)
            sub.add_argument('--package-sha256', required=True)
            sub.add_argument('--grant', action='append', default=[])
            binding = sub.add_mutually_exclusive_group()
            binding.add_argument('--recorder-binding', type=Path)
            binding.add_argument('--recorder-binding-json')
    args = parser.parse_args(argv)
    try:
        target()
        condition = None
        if args.command in ('install', 'enable', 'disable'):
            if args.expected_generation is not None or args.expected_inventory_sha256 is not None:
                require(args.expected_generation is not None and args.expected_inventory_sha256 is not None,
                        'Both inventory preconditions are required')
                condition = {'generation': args.expected_generation, 'inventory_sha256': args.expected_inventory_sha256}
        if args.command == 'package':
            result = {'package_sha256': package_binary(args.binary, args.license_file, args.output, args.id, args.version, args.role, args.codec_protocol)}
        elif args.command == 'inspect':
            package, _ = inspect_package(args.package, args.expected_sha256)
            result = {'package': package, 'executed': False}
        elif args.command == 'install':
            package = install(args.store, args.package, args.expected_sha256, condition=condition)
            result = package if condition is not None else {'id': package['id'], 'version': package['version'], 'installed': True, 'activated': False}
        elif args.command == 'enable':
            binding = decode_json(read_file(args.recorder_binding, MAX_JSON, private=True)) if args.recorder_binding else None
            if args.recorder_binding_json is not None:
                require(len(args.recorder_binding_json) <= MAX_JSON, 'Recorder binding exceeds its bound')
                binding = decode_json(args.recorder_binding_json)
            result = enable(args.store, args.id, args.version, args.package_sha256, args.grant, binding, condition=condition)
        elif args.command == 'disable':
            result = disable(args.store, args.id, condition=condition)
        elif args.command == 'inventory':
            result = inventory(args.store)
        else:
            root = private_dir(args.store)
            result = {'activation': read_lock(root), 'runtime_checked': False}
        print(canonical(result).decode(), end='')
        return 0
    except ExtensionConflict:
        print('Extension inventory changed; inspect before submitting a new operation', file=sys.stderr)
        return 3
    except (OSError, ValueError, TypeError, KeyError):
        # Even a parser/filesystem error must not echo a path or supplied document.
        print('Extension operation failed; verify package, permissions, target and store', file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
