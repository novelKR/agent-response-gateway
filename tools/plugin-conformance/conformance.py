#!/usr/bin/env python3
"""Offline package and bounded executable checks. Python 3.11+, Linux/macOS.

This standalone tool intentionally imports no gateway implementation. Execution is
explicit, runs trusted native code with an empty environment, and is not a sandbox.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import socket
import stat
import subprocess
import tempfile
import time

VERSION = '1.0.0'
FRAME_LIMIT = 4096
ROLES = {
    'gateway-observer/v1': (['observe_http_metadata', 'write_private_state'], 'observer-state/v1'),
    'gateway-usage-recorder/v1': (['export_usage', 'observe_usage', 'write_usage_store'], 'usage-store/v1'),
    'gateway-api-codec/v1': (['read_model_payload', 'transform_model_protocol'], 'request-memory/v1'),
    'gateway-api-codec/v2': (['read_model_payload', 'transform_model_protocol'], 'request-memory/v1'),
}
TARGETS = {'linux-x64', 'linux-arm64', 'macos-x64', 'macos-arm64'}


class Failure(ValueError):
    """Only fixed diagnostic codes may cross the report boundary."""


def require(condition, code):
    if not condition:
        raise Failure(code)


def canonical(value):
    return (json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=True, allow_nan=False) + '\n').encode()


def decode(raw):
    def pairs(items):
        value = {}
        for key, item in items:
            require(key not in value, 'duplicate_field')
            value[key] = item
        return value
    try:
        return json.loads(raw.decode('utf-8'), object_pairs_hook=pairs,
                          parse_constant=lambda _: (_ for _ in ()).throw(Failure('invalid_number')))
    except (UnicodeError, json.JSONDecodeError, RecursionError) as error:
        raise Failure('invalid_json') from error


def matches(pattern, value):
    return isinstance(value, str) and re.fullmatch(pattern, value) is not None


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def host_target():
    arch = {'x86_64': 'x64', 'amd64': 'x64', 'arm64': 'arm64', 'aarch64': 'arm64'}.get(platform.machine().lower())
    system = {'Linux': 'linux', 'Darwin': 'macos'}.get(platform.system())
    return f'{system}-{arch}' if system and arch else 'unsupported'


def read_regular(path, limit):
    # Validate ancestors before opening. Package directories must remain immutable
    # during inspection; the execution copy below avoids executing reopened bytes.
    require(path.is_absolute() and '..' not in path.parts, 'invalid_path')
    for parent in [*path.parents, path]:
        require(not parent.is_symlink(), 'linked_path')
    before = path.lstat()
    require(stat.S_ISREG(before.st_mode) and before.st_nlink == 1, 'invalid_file')
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, 'rb') as stream:
        current = os.fstat(stream.fileno())
        require((before.st_dev, before.st_ino) == (current.st_dev, current.st_ino)
                and stat.S_ISREG(current.st_mode) and current.st_nlink == 1, 'changed_file')
        require(current.st_size <= limit, 'file_limit')
        raw = stream.read(limit + 1)
        require(len(raw) <= limit, 'file_limit')
        return raw


def inspect_package(directory, expected):
    require(matches(r'[a-f0-9]{64}', expected), 'invalid_expected_digest')
    raw = read_regular(directory / 'extension.json', 65536)
    require(digest(raw) == expected, 'manifest_digest')
    value = decode(raw)
    require(isinstance(value, dict) and set(value) == {
        'schema', 'id', 'version', 'target', 'protocol', 'permissions', 'state_schema', 'files'
    }, 'manifest_fields')
    require(value['schema'] == 'gateway-extension-package/v1', 'package_schema')
    require(isinstance(value['protocol'], str) and value['protocol'] in ROLES, 'unsupported_role')
    require(matches(r'[a-z][a-z0-9-]{0,63}', value['id']) and matches(
        r'(0|[1-9][0-9]{0,5})\.(0|[1-9][0-9]{0,5})\.(0|[1-9][0-9]{0,5})', value['version']), 'package_identity')
    require(isinstance(value['target'], str) and value['target'] in TARGETS, 'package_target')
    permissions, state_schema = ROLES[value['protocol']]
    require(value['permissions'] == permissions and value['state_schema'] == state_schema, 'role_contract')
    files = value['files']
    require(isinstance(files, dict) and 2 <= len(files) <= 8 and {'extension', 'LICENSE.txt'} <= files.keys(), 'package_files')
    for name, checksum in files.items():
        require(matches(r'[A-Za-z0-9][A-Za-z0-9_.-]{0,63}', name)
                and name != 'extension.json' and matches(r'[a-f0-9]{64}', checksum), 'file_record')
    require(canonical(value) == raw, 'noncanonical_manifest')
    require({p.name for p in directory.iterdir()} == {'extension.json', *files}, 'file_inventory')
    contents = {'extension.json': raw}
    for name, checksum in files.items():
        contents[name] = read_regular(directory / name, 128 * 1024 * 1024 if name == 'extension' else 256 * 1024)
        require(digest(contents[name]) == checksum, 'file_digest')
    return value, contents


class Frames:
    def __init__(self, stream):
        self.stream = stream
        self.pending = b''

    def read(self, deadline):
        while b'\n' not in self.pending:
            require(len(self.pending) < FRAME_LIMIT, 'frame_limit')
            remaining = deadline - time.monotonic()
            require(remaining > 0, 'deadline')
            self.stream.settimeout(remaining)
            chunk = self.stream.recv(FRAME_LIMIT + 1 - len(self.pending))
            require(bool(chunk), 'unexpected_eof')
            self.pending += chunk
        frame, self.pending = self.pending.split(b'\n', 1)
        require(len(frame) + 1 <= FRAME_LIMIT, 'frame_limit')
        return decode(frame)

    def write(self, value, deadline):
        remaining = deadline - time.monotonic()
        require(remaining > 0, 'deadline')
        self.stream.settimeout(remaining)
        self.stream.sendall(canonical(value))


def observer(contents, root, checks):
    with tempfile.TemporaryDirectory(prefix='observer-', dir=root) as temporary:
        directory = Path(temporary)
        executable = directory / 'extension'
        for name, raw in contents.items():
            destination = directory / name
            destination.write_bytes(raw)
            destination.chmod(0o700 if name == 'extension' else 0o600)
        state = directory / 'state'
        state.mkdir(mode=0o700)
        parent, child = socket.socketpair()
        process = None
        try:
            process = subprocess.Popen([str(executable)], stdin=child, stdout=child,
                                       stderr=subprocess.DEVNULL, env={}, cwd=state)
            child.close()
            frames = Frames(parent)
            require(frames.read(time.monotonic() + 3) == {'type': 'ready', 'protocol': 'gateway-observer/v1'}, 'invalid_ready')
            checks.append(check('observer.ready', 'pass'))
            for sequence, status, elapsed in [(1, 200, 0), (2, 503, 2**64 - 1), (3, 100, 17)]:
                deadline = time.monotonic() + 1
                frames.write({'type': 'http', 'sequence': sequence, 'status': status, 'headers_ms': elapsed}, deadline)
                reply = frames.read(deadline)
                require(isinstance(reply, dict) and set(reply) == {'type', 'sequence'}
                        and reply['type'] == 'ack' and type(reply['sequence']) is int
                        and reply['sequence'] == sequence, 'invalid_ack')
            checks.append(check('observer.ack_sequence', 'pass'))
            # Shutdown is host-owned in v1, which has no graceful-EOF contract.
        finally:
            child.close()
            parent.close()
            if process is not None:
                if process.poll() is None:
                    process.kill()
                process.wait()


RUNNERS = {'gateway-observer/v1': observer}


def check(identifier, status, code=None):
    result = {'id': identifier, 'status': status}
    if code is not None:
        result['code'] = code
    return result


def run(directory, expected, execute=False, state_root=None):
    report = {'schema': 'gateway-plugin-conformance-report/v1', 'tool_version': VERSION,
              'package_sha256': expected if matches(r'[a-f0-9]{64}', expected) else None,
              'package_schema': None, 'contract': None, 'target': None,
              'host_target': host_target(), 'checks': []}
    checks = report['checks']
    phase = 'package.static'
    try:
        value, contents = inspect_package(directory, expected)
        report.update(package_schema=value['schema'], contract=value['protocol'], target=value['target'])
        checks.append(check(phase, 'pass'))
        phase = 'role.execution'
        if not execute:
            checks.append(check(phase, 'not-run', 'execution_not_requested'))
        elif value['protocol'] not in RUNNERS:
            checks.append(check(phase, 'not-run', 'role_runner_unavailable'))
        else:
            require(host_target() == value['target'], 'host_target_mismatch')
            require(state_root is not None and state_root.is_absolute() and state_root.is_dir()
                    and all(not path.is_symlink() for path in [state_root, *state_root.parents]), 'state_root_required')
            RUNNERS[value['protocol']](contents, state_root, checks)
            checks.append(check(phase, 'pass'))
    except Failure as error:
        checks.append(check(phase, 'fail', str(error)))
    except (TimeoutError, socket.timeout):
        checks.append(check(phase, 'fail', 'deadline'))
    except (OSError, ValueError, TypeError, RecursionError):
        checks.append(check(phase, 'fail', 'io_or_structure_error'))
    report['status'] = ('fail' if any(item['status'] == 'fail' for item in checks)
                        else 'not-run' if any(item['status'] == 'not-run' for item in checks) else 'pass')
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--package', required=True, type=Path)
    parser.add_argument('--expected-sha256', required=True)
    parser.add_argument('--execute', action='store_true', help='Execute explicitly trusted package code (not sandboxed)')
    parser.add_argument('--state-root', type=Path, help='Existing scratch directory for isolated temporary state')
    args = parser.parse_args()
    report = run(args.package, args.expected_sha256, args.execute, args.state_root)
    print(canonical(report).decode(), end='')
    return 1 if report['status'] == 'fail' else 2 if args.execute and report['status'] == 'not-run' else 0


if __name__ == '__main__':
    raise SystemExit(main())
