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

VERSION = '2.0.0'
FRAME_LIMIT = 4096
ROLES = {
    'gateway-observer/v1': (['observe_http_metadata', 'write_private_state'], 'observer-state/v1'),
    'gateway-usage-recorder/v1': (['export_usage', 'observe_usage', 'write_usage_store'], 'usage-store/v1'),
    'gateway-api-codec/v1': (['read_model_payload', 'transform_model_protocol'], 'request-memory/v1'),
    'gateway-api-codec/v2': (['read_model_payload', 'transform_model_protocol'], 'request-memory/v1'),
}
V2_ROLES = {
    'gateway-usage-recorder/v2': (['export_usage', 'observe_usage', 'write_usage_store'], 'usage-store/v2'),
    'gateway-api-codec/v3': (['read_model_payload', 'transform_model_protocol'], 'request-memory/v1'),
    'gateway-provider/v1': (['read_model_payload', 'transform_model_protocol'], 'provider-request-memory/v1'),
}
APIS = {'chat_completions', 'gemini_interactions', 'messages', 'responses'}
FEATURES = {'editing', 'json', 'managed_continuation', 'streaming'}
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


def validate_capabilities(value, protocol):
    require(isinstance(value, dict) and set(value) == {'schema', 'apis', 'features', 'requires'}, 'capability_fields')
    require(value['schema'] == 'gateway-plugin-capabilities/v1', 'capability_schema')
    for name in ('apis', 'features', 'requires'):
        entries = value[name]
        require(isinstance(entries, list) and all(isinstance(entry, str) for entry in entries), 'capability_list')
        require(entries == sorted(set(entries)), 'capability_order')
    if protocol == 'gateway-usage-recorder/v2':
        require(value['apis'] == [] and value['features'] == ['usage_event_v1', 'usage_event_v2']
                and value['requires'] == ['usage_recorder_ipc_v2'], 'recorder_capabilities')
        return
    require(bool(value['features']) and 'json' in value['features']
            and set(value['features']) <= FEATURES, 'capability_features')
    if protocol == 'gateway-api-codec/v3':
        require(bool(value['apis']) and set(value['apis']) <= APIS, 'capability_apis')
        required = ['codec_ipc_v3', 'responses_output_validation']
    else:
        require(value['apis'] == [], 'capability_apis')
        required = ['provider_ipc_v1', 'responses_output_validation']
    require(value['requires'] == required, 'host_contract')


def inspect_package(directory, expected):
    require(matches(r'[a-f0-9]{64}', expected), 'invalid_expected_digest')
    raw = read_regular(directory / 'extension.json', 65536)
    require(digest(raw) == expected, 'manifest_digest')
    value = decode(raw)
    require(isinstance(value, dict), 'manifest_fields')
    require(value.get('schema') in ('gateway-extension-package/v1', 'gateway-extension-package/v2'), 'package_schema')
    fields = {'schema', 'id', 'version', 'target', 'protocol', 'permissions', 'state_schema', 'files'}
    roles = ROLES
    if value['schema'] == 'gateway-extension-package/v2':
        roles = V2_ROLES
        fields.add('capabilities')
        if value.get('protocol') == 'gateway-provider/v1':
            fields.add('provider_protocol')
    require(set(value) == fields, 'manifest_fields')
    require(isinstance(value['protocol'], str) and value['protocol'] in roles, 'unsupported_role')
    if roles is V2_ROLES:
        validate_capabilities(value['capabilities'], value['protocol'])
        if value['protocol'] == 'gateway-provider/v1':
            require(matches(r'[a-z][a-z0-9._-]{0,63}/v[1-9][0-9]{0,5}', value['provider_protocol']), 'provider_protocol')
    require(matches(r'[a-z][a-z0-9-]{0,63}', value['id']) and matches(
        r'(0|[1-9][0-9]{0,5})\.(0|[1-9][0-9]{0,5})\.(0|[1-9][0-9]{0,5})', value['version']), 'package_identity')
    require(isinstance(value['target'], str) and value['target'] in TARGETS, 'package_target')
    permissions, state_schema = roles[value['protocol']]
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


def observer(executable, state, checks):
    process = None
    try:
        process = Native(executable, state)
        frames = Frames(process.socket)
        mark(checks, 'observer.ready', 'not-run', 'in_progress')
        require(frames.read(time.monotonic() + 3) == {'type': 'ready', 'protocol': 'gateway-observer/v1'}, 'invalid_ready')
        mark(checks, 'observer.ready', 'pass')
        mark(checks, 'observer.ack_sequence', 'not-run', 'in_progress')
        for sequence, status, elapsed in [(1, 200, 0), (2, 503, 2**64 - 1), (3, 100, 17)]:
            deadline = time.monotonic() + 1
            frames.write({'type': 'http', 'sequence': sequence, 'status': status, 'headers_ms': elapsed}, deadline)
            reply = frames.read(deadline)
            require(isinstance(reply, dict) and set(reply) == {'type', 'sequence'}
                    and reply['type'] == 'ack' and type(reply['sequence']) is int
                    and reply['sequence'] == sequence, 'invalid_ack')
        mark(checks, 'observer.ack_sequence', 'pass')
    finally:
        if process is not None:
            process.close()


def check(identifier, status, code=None):
    result = {'id': identifier, 'status': status}
    if code is not None:
        result['code'] = code
    return result



# The v1 Observer runner and package inspector above retain their contracts.
import copy
import struct

PROFILES = ('wire', 'synthetic-provider/v1', 'recorder-events/v2')
PROVIDER_FRAME = 1024 * 1024
STATE_LIMIT = 32 * 1024 * 1024
EVENT_SCHEMAS = ['gateway-usage-event/v1', 'gateway-usage-event/v2']
RECORDER_CAPS = {'schema': 'gateway-plugin-capabilities/v1', 'apis': [],
                 'features': ['usage_event_v1', 'usage_event_v2'], 'requires': ['usage_recorder_ipc_v2']}


def bytes_json(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False, allow_nan=False).encode()


def mark(checks, identifier, status, code=None):
    value = check(identifier, status, code)
    for index, previous in enumerate(checks):
        if previous['id'] == identifier:
            checks[index] = value
            return
    checks.append(value)


class Native:
    def __init__(self, executable, cwd, arguments=()):
        self.socket, child = socket.socketpair()
        self.child = None
        self.failed = False
        try:
            self.child = subprocess.Popen([str(executable), *arguments], stdin=child, stdout=child,
                                          stderr=subprocess.DEVNULL, env={}, cwd=cwd)
        except BaseException:
            self.socket.close()
            raise
        finally:
            child.close()

    def exact(self, size, deadline):
        raw = bytearray()
        while len(raw) < size:
            remaining = deadline - time.monotonic()
            require(remaining > 0, 'deadline')
            self.socket.settimeout(remaining)
            chunk = self.socket.recv(size - len(raw))
            require(bool(chunk), 'unexpected_eof')
            raw.extend(chunk)
        return bytes(raw)

    def frame(self, deadline):
        length = struct.unpack('>I', self.exact(4, deadline))[0]
        require(0 < length <= PROVIDER_FRAME, 'frame_limit')
        return decode(self.exact(length, deadline))

    def write(self, raw, deadline):
        remaining = deadline - time.monotonic()
        require(remaining > 0 and not self.failed, 'deadline')
        self.socket.settimeout(remaining)
        self.socket.sendall(raw)

    def exchange(self, value):
        deadline = time.monotonic() + 3
        raw = bytes_json(value)
        require(0 < len(raw) <= PROVIDER_FRAME, 'frame_limit')
        try:
            self.write(struct.pack('>I', len(raw)) + raw, deadline)
            return self.frame(deadline)
        except BaseException:
            self.failed = True
            raise

    def close(self):
        failure = None
        try:
            self.socket.close()
        except OSError as error:
            failure = error
        if self.child is not None:
            try:
                if self.child.poll() is None:
                    self.child.kill()
                self.child.wait(timeout=3)
            except (OSError, subprocess.TimeoutExpired) as error:
                failure = error
        if failure is not None:
            raise Failure('cleanup_failed') from failure


def copy_package(contents, directory):
    directory.mkdir(mode=0o700)
    for name, raw in contents.items():
        path = directory / name
        path.write_bytes(raw)
        path.chmod(0o700 if name == 'extension' else 0o600)
    return directory / 'extension'


def state_fixture(source, package, scratch):
    require(source is not None and source.is_absolute() and '..' not in source.parts and source.is_dir(), 'state_fixture_required')
    require(all(not p.is_symlink() for p in [source, *source.parents]), 'linked_state_fixture')
    require(source.stat().st_mode & 0o077 == 0, 'private_state_fixture_required')
    for other in (package, scratch):
        require(not (source == other or source in other.parents or other in source.parents), 'state_fixture_overlap')
    entries = sorted(source.iterdir())
    require(len(entries) <= 64, 'state_fixture_limit')
    contents, inventory, total = {}, {}, 0
    for path in entries:
        require(path.is_file() and matches(r'[A-Za-z0-9_.-]{1,128}', path.name), 'unsupported_state_layout')
        raw = read_regular(path, STATE_LIMIT)
        total += len(raw)
        require(total <= STATE_LIMIT, 'state_fixture_limit')
        contents[path.name] = raw
        inventory[path.name] = digest(raw)
    require([p.name for p in sorted(source.iterdir())] == list(contents), 'changed_state_fixture')
    for name, raw in contents.items():
        require(read_regular(source / name, STATE_LIMIT) == raw, 'changed_state_fixture')
    return contents, digest(bytes_json(inventory))


def provider_ready(process, manifest):
    expected = {'protocol': 'gateway-provider/v1', 'sequence': 0, 'value': {'result': 'ready',
                'provider_protocol': manifest['provider_protocol'], 'capabilities': manifest['capabilities']}}
    reply = process.frame(time.monotonic() + 3)
    require(type(reply.get('sequence')) is int and reply == expected, 'invalid_ready')


def provider_call(process, sequence, operation, result):
    if 'response_id' in operation:
        operation = dict(operation, response_id=process.response_id)
    reply = process.exchange({'protocol': 'gateway-provider/v1', 'sequence': sequence, 'operation': operation})
    require(isinstance(reply, dict) and set(reply) == {'protocol', 'sequence', 'value'}
            and reply['protocol'] == 'gateway-provider/v1' and type(reply['sequence']) is int
            and reply['sequence'] == sequence and isinstance(reply['value'], dict), 'reply_envelope')
    require(reply['value'].get('result') == result, 'unexpected_result')
    return reply['value']


def prepare_value(continuation=None, request=None):
    return {'operation': 'prepare', 'value': {'request': request or {'model': 'synthetic-model', 'input': 'hello'},
            'route': {'provider_protocol': 'synthetic-provider/v1', 'model': 'synthetic-model',
                      'profile_id': 'synthetic', 'profile_version': '1', 'support': {}, 'editing': {'kind': 'none'}},
            'continuation': continuation or {'mode': 'stateless'}, 'max_request_bytes': 65536, 'max_output_bytes': 65536}}


def provider_completed(reply, response_id, *, tools=False, text='hello', managed=False):
    require(set(reply) == {'result', 'value'} and isinstance(reply['value'], dict)
            and set(reply['value']) == {'response', 'outcome', 'usage', 'state'}, 'completed_shape')
    value = reply['value']
    actual_output = value['response'].get('output')
    require(isinstance(actual_output, list) and len(actual_output) == 1 and isinstance(actual_output[0], dict), 'completed_items')
    item_id = actual_output[0].get('id')
    require(matches(r'[A-Za-z0-9_/.:\-]{1,200}', item_id), 'completed_item_identity')
    output = ([{'id': item_id, 'type': 'function_call', 'status': 'completed', 'call_id': 'call_1',
               'name': 'lookup', 'arguments': '{}'}] if tools else [{'id': item_id, 'type': 'message',
               'role': 'assistant', 'status': 'completed', 'content': [{'type': 'output_text', 'text': text, 'annotations': []}]}])
    expected = {'id': response_id, 'object': 'response', 'created_at': 0,
                'model': 'synthetic-model', 'status': 'completed', 'output': output}
    require(bytes_json(value['response']) == bytes_json(expected) and value['outcome'] == ('awaiting_tools' if tools else 'completed')
            and value['usage'] == {'kind': 'unobserved'}, 'completed_output')
    if not managed:
        require(value['state'] == {'kind': 'none'}, 'unexpected_state')
    else:
        state = value['state']
        require(isinstance(state, dict) and set(state) == {'kind', 'value'} and state['kind'] == 'opaque'
                and isinstance(state['value'], dict) and set(state['value']) == {'format', 'version', 'data_base64'}
                and state['value']['format'] == 'synthetic-counter' and type(state['value']['version']) is int
                and state['value']['version'] == 1, 'state_shape')
    return value


def provider_suite(executable, cwd, manifest, checks, profile):
    active = []
    response_number = 0
    completed_item_ids = set()
    def completed(reply, **kwargs):
        value = provider_completed(reply, process.response_id, **kwargs)
        ids = {item['id'] for item in value['response']['output']}
        require(not (ids & completed_item_ids), 'reused_item_identity')
        completed_item_ids.update(ids)
        return value
    def fresh():
        nonlocal response_number
        process = Native(executable, cwd)
        active.append(process)
        response_number += 1
        process.response_id = f'conformance_response_{response_number}'
        mark(checks, 'provider.ready', 'not-run', 'in_progress')
        provider_ready(process, manifest)
        mark(checks, 'provider.ready', 'pass')
        return process
    try:
        if profile == 'wire':
            fresh()
            for name in ('prepare', 'json', 'sse', 'tools', 'state_wire', 'usage_wire'):
                mark(checks, 'provider.' + name, 'not-run', 'semantic_fixture_required')
            return
        require(manifest['provider_protocol'] == 'synthetic-provider/v1', 'profile_provider_mismatch')
        process = fresh()
        mark(checks, 'provider.prepare', 'not-run', 'in_progress')
        prepared = provider_call(process, 1, prepare_value(), 'prepared')
        require(set(prepared) == {'result', 'payload'} and bytes_json(prepared['payload']) == bytes_json({'query': {'model': 'synthetic-model', 'input': 'hello'}, 'cursor': 0}), 'prepared_payload')
        mark(checks, 'provider.prepare', 'pass')
        mark(checks, 'provider.json', 'not-run', 'in_progress')
        result = provider_call(process, 2, {'operation': 'json', 'body': '{"answer":[{"text":"hello"}]}', 'response_id': 'conformance_response'}, 'completed')
        completed(result)
        mark(checks, 'provider.json', 'pass')
        process.close();active.remove(process)
        if 'streaming' in manifest['capabilities']['features']:
            mark(checks, 'provider.sse', 'not-run', 'in_progress')
            process = fresh()
            provider_call(process, 1, prepare_value(), 'prepared')
            started = provider_call(process, 2, {'operation': 'stream', 'response_id': 'conformance_response'}, 'progress')
            require(bytes_json(started) == bytes_json({'result': 'progress', 'events': [], 'complete': False, 'usage': {'kind': 'unobserved'}}), 'stream_initialization')
            observed = []
            for sequence, piece in [(3, 'hel'), (4, 'lo')]:
                progress = provider_call(process, sequence, {'operation': 'event', 'event': 'piece', 'data': json.dumps({'text': piece})}, 'progress')
                require(set(progress) == {'result', 'events', 'complete', 'usage'} and progress['complete'] is False
                        and progress['usage'] == {'kind': 'unobserved'} and isinstance(progress['events'], list), 'progress_shape')
                observed.extend(progress['events'])
            require(len(observed) == 5 and isinstance(observed[1].get('item'), dict), 'progress_shape')
            item_id = observed[1]['item'].get('id')
            require(matches(r'[A-Za-z0-9_/.:\-]{1,200}', item_id), 'progress_identity')
            expected_progress = [
                {'type': 'response.created', 'response': {'id': process.response_id, 'object': 'response', 'created_at': 0, 'model': 'synthetic-model', 'status': 'in_progress', 'output': []}},
                {'type': 'response.output_item.added', 'output_index': 0, 'item': {'id': item_id, 'type': 'message', 'role': 'assistant', 'status': 'in_progress', 'content': []}},
                {'type': 'response.content_part.added', 'output_index': 0, 'item_id': item_id, 'content_index': 0, 'part': {'type': 'output_text', 'text': '', 'annotations': []}},
                *[{'type': 'response.output_text.delta', 'output_index': 0, 'item_id': item_id, 'content_index': 0, 'delta': piece} for piece in ('hel', 'lo')],
            ]
            require(bytes_json(observed) == bytes_json(expected_progress), 'progress_consistency')
            end = provider_call(process, 5, {'operation': 'event', 'event': 'end', 'data': '{"answer":[{"text":"hello"}]}'}, 'progress')
            require(bytes_json(end) == bytes_json({'result': 'progress', 'events': [], 'complete': True, 'usage': {'kind': 'unobserved'}}), 'semantic_end')
            finalized = completed(provider_call(process, 6, {'operation': 'finish'}, 'completed'))
            require(finalized['response']['output'][0]['id'] == item_id, 'progress_final_identity')
            mark(checks, 'provider.sse', 'pass')
            process.close();active.remove(process)
        else:
            mark(checks, 'provider.sse', 'not-run', 'feature_not_declared')
        mark(checks, 'provider.tools', 'not-run', 'in_progress')
        request = {'model': 'synthetic-model', 'input': 'hello', 'tools': [{'type': 'function', 'name': 'lookup', 'parameters': {'type': 'object', 'properties': {}}}]}
        process = fresh()
        provider_call(process, 1, prepare_value(request=request), 'prepared')
        completed(provider_call(process, 2, {'operation': 'json', 'body': '{"answer":[{"call":"call_1","name":"lookup","arguments":"{}"}]}', 'response_id': 'conformance_response'}, 'completed'), tools=True)
        process.close();active.remove(process)
        process = fresh()
        result_item = {'type': 'function_call_output', 'call_id': 'call_1', 'output': 'synthetic-result'}
        request = {'model': 'synthetic-model', 'input': [{'type': 'function_call', 'call_id': 'call_1', 'name': 'lookup', 'arguments': '{}'}, result_item]}
        prepared = provider_call(process, 1, prepare_value(request=request), 'prepared')
        require(prepared['payload']['query']['input'][-1] == result_item, 'tool_result_mapping')
        completed(provider_call(process, 2, {'operation': 'json', 'body': '{"answer":[{"text":"hello"}]}', 'response_id': 'conformance_response'}, 'completed'))
        mark(checks, 'provider.tools', 'pass')
        process.close();active.remove(process)
        mark(checks, 'provider.usage_wire', 'not-run', 'in_progress')
        for number in (0, 2**53+1, 2**64-1, -1):
            process = fresh()
            provider_call(process, 1, prepare_value(), 'prepared')
            reply = provider_call(process, 2, {'operation': 'json', 'body': json.dumps({'answer': [{'text': 'hello'}], 'meter': {'input_tokens': number, 'output_tokens': 0}}), 'response_id': 'conformance_response'}, 'completed')
            require(set(reply) == {'result', 'value'} and isinstance(reply['value'], dict), 'usage_result')
            without_usage = copy.deepcopy(reply)
            without_usage['value']['usage'] = {'kind': 'unobserved'}
            without_usage['value']['response'].pop('usage', None)
            completed(without_usage)
            counters = {name: {'source': 'not_reported'} for name in EVENT_V2['usage']['counters']}
            counters['input_tokens'] = {'source': 'invalid'} if number < 0 else {'source': 'reported', 'value': number}
            counters['output_tokens'] = {'source': 'reported', 'value': 0}
            require(reply['value']['usage'] == {'kind': 'observed', 'counters': counters}, 'usage_interpretation')
            for counter in reply['value']['usage']['counters'].values():
                if counter['source'] == 'reported':require(type(counter['value']) is int, 'usage_number')
            expected = {'input_tokens': None if number < 0 else number, 'output_tokens': 0, 'total_tokens': None if number < 0 else number}
            require(bytes_json(reply['value']['response']['usage']) == bytes_json(expected), 'usage_projection')
            process.close();active.remove(process)
        mark(checks, 'provider.usage_wire', 'pass')
        if 'managed_continuation' in manifest['capabilities']['features']:
            mark(checks, 'provider.state_wire', 'not-run', 'in_progress')
            import base64
            process = fresh()
            provider_call(process, 1, prepare_value({'mode': 'managed', 'pending_tools': False, 'history': []}), 'prepared')
            value = completed(provider_call(process, 2, {'operation': 'json', 'body': '{"answer":[{"text":"hello"}]}', 'response_id': 'conformance_response'}, 'completed'), managed=True)
            state = value['state']['value']
            require(state['data_base64'] == base64.b64encode(b'{"counter":1}').decode(), 'state_counter')
            process.close();active.remove(process)
            process = fresh()
            prepared = provider_call(process, 1, prepare_value({'mode': 'managed', 'pending_tools': False, 'history': [{'start': 0, 'end': 1, 'state': state}]}), 'prepared')
            require(type(prepared['payload']['cursor']) is int and prepared['payload']['cursor'] == 1, 'state_restore')
            value = completed(provider_call(process, 2, {'operation': 'json', 'body': '{"answer":[{"text":"hello"}]}', 'response_id': 'conformance_response'}, 'completed'), managed=True)
            require(value['state']['value']['data_base64'] == base64.b64encode(b'{"counter":2}').decode(), 'state_counter')
            mark(checks, 'provider.state_wire', 'pass')
        else:
            mark(checks, 'provider.state_wire', 'not-run', 'feature_not_declared')
    finally:
        for process in active:
            process.close()


def recorder_ready(process, manifest):
    reader = Frames(process.socket)
    ready = reader.read(time.monotonic() + 3)
    require(isinstance(ready, dict) and set(ready) == {'type', 'protocol', 'producer_id', 'capabilities'}
            and ready['type'] == 'ready' and ready['protocol'] == 'gateway-usage-recorder/v2'
            and ready['capabilities'] == manifest['capabilities']
            and matches(r'[A-Za-z0-9_/.:\-]{1,200}', ready['producer_id']), 'invalid_ready')
    return reader, ready['producer_id']


def recorder_send(process, reader, event):
    raw = bytes_json(event)
    require(len(raw) <= 65536, 'event_limit')
    deadline = time.monotonic() + 3
    process.write(raw + b'\n', deadline)
    ack = reader.read(deadline)
    require(ack == {'type': 'committed', 'event_id': event['event_id'], 'sha256': digest(raw)}, 'invalid_ack')


def recorder_suite(executable, cwd, manifest, checks, profile):
    process = None
    try:
        process = Native(executable, cwd, ('serve-v2',))
        mark(checks, 'recorder.ready', 'not-run', 'in_progress')
        reader, producer = recorder_ready(process, manifest)
        mark(checks, 'recorder.ready', 'pass')
        if profile == 'wire':
            for name in ('v1_ack','v2_ack','invalid_observation','duplicate','restart'):
                mark(checks, 'recorder.'+name, 'not-run', 'event_profile_required')
            return
        events = []
        for version, template in [(1, EVENT_V1), (2, EVENT_V2)]:
            event = copy.deepcopy(template)
            event.update(producer_id=producer, request_id=f'conformance-request-{version}',
                         attempt_id=f'conformance-attempt-{version}', event_id=f'conformance-event-{version}')
            mark(checks, f'recorder.v{version}_ack', 'not-run', 'in_progress')
            recorder_send(process, reader, event)
            events.append(event)
            mark(checks, f'recorder.v{version}_ack', 'pass')
        invalid_event = copy.deepcopy(events[1])
        invalid_event.update(attempt_id='conformance-invalid-attempt', event_id='conformance-invalid-event', request_id='conformance-invalid-request',
                             finality='partial', upstream='unknown', gateway='conversion_failed', observation_incomplete=True)
        invalid_event['usage']['counters'] = {name: {'source': 'not_reported', 'value': None} for name in EVENT_V2['usage']['counters']}
        invalid_event['usage']['counters']['input_tokens'] = {'source': 'invalid', 'value': None}
        invalid_event['usage']['counters']['output_tokens'] = {'source': 'reported', 'value': 0}
        invalid_event['usage']['violations'] = ['invalid_counter']
        mark(checks, 'recorder.invalid_observation', 'not-run', 'in_progress')
        recorder_send(process, reader, invalid_event);events.append(invalid_event)
        mark(checks, 'recorder.invalid_observation', 'pass')
        mark(checks, 'recorder.duplicate', 'not-run', 'in_progress')
        for event in events:
            recorder_send(process, reader, event)
        mark(checks, 'recorder.duplicate', 'pass')
        process.close();process = None
        process = Native(executable, cwd, ('serve-v2',))
        mark(checks, 'recorder.restart', 'not-run', 'in_progress')
        reader, restarted = recorder_ready(process, manifest)
        require(restarted == producer, 'producer_changed')
        for event in events:
            recorder_send(process, reader, event)
        mark(checks, 'recorder.restart', 'pass')
    finally:
        if process is not None:
            process.close()


def run(directory, expected, execute=False, state_root=None, profile='wire', recorder_state=None):
    report = {'schema': 'gateway-plugin-conformance-report/v2', 'tool_version': VERSION,
              'package_sha256': expected if matches(r'[a-f0-9]{64}', expected) else None,
              'package_schema': None, 'contract': None, 'target': None, 'host_target': host_target(),
              'profile': profile, 'tool_sha256': digest(Path(__file__).read_bytes()), 'fixture_sha256': None, 'checks': [check('package.static','not-run','not_started')]}
    checks = report['checks'];phase = 'package.static'
    try:
        require(profile in PROFILES, 'unsupported_profile')
        manifest, contents = inspect_package(directory, expected)
        report.update(package_schema=manifest['schema'], contract=manifest['protocol'], target=manifest['target'])
        mark(checks, phase, 'pass')
        role = manifest['protocol']
        ids = (['provider.ready','provider.prepare','provider.json','provider.sse','provider.tools','provider.state_wire','provider.usage_wire'] if role == 'gateway-provider/v1'
               else ['recorder.ready','recorder.v1_ack','recorder.v2_ack','recorder.invalid_observation','recorder.duplicate','recorder.restart'] if role == 'gateway-usage-recorder/v2'
               else ['observer.ready','observer.ack_sequence'] if role == 'gateway-observer/v1' else [])
        for identifier in ids:mark(checks, identifier, 'not-run','execution_not_requested' if not execute else 'not_reached')
        mark(checks, 'role.execution', 'not-run', 'execution_not_requested')
        mark(checks, 'host.integration', 'not-run', 'separate_host_qualification')
        if not execute:
            return finish_report(report, execute)
        phase = 'role.execution'
        require(profile == 'wire' or (profile == 'synthetic-provider/v1' and role == 'gateway-provider/v1')
                or (profile == 'recorder-events/v2' and role == 'gateway-usage-recorder/v2'), 'profile_role_mismatch')
        if role not in ('gateway-observer/v1','gateway-provider/v1','gateway-usage-recorder/v2'):
            mark(checks, phase,'not-run','role_runner_unavailable');return finish_report(report,execute)
        require(host_target() == manifest['target'], 'host_target_mismatch')
        require(state_root is not None and state_root.is_absolute() and '..' not in state_root.parts and state_root.is_dir()
                and all(not p.is_symlink() for p in [state_root,*state_root.parents])
                and state_root.stat().st_mode & 0o077 == 0, 'private_state_root_required')
        state_contents = {}
        if role == 'gateway-usage-recorder/v2':
            if recorder_state is None:
                mark(checks,phase,'not-run','state_fixture_required');return finish_report(report,execute)
            state_contents,state_digest = state_fixture(recorder_state,directory,state_root)
            report['fixture_sha256'] = digest(bytes_json({'state':state_digest,'events':[EVENT_V1,EVENT_V2]}))
        elif profile == 'synthetic-provider/v1':
            report['fixture_sha256'] = digest(bytes_json({'suite':profile,'version':1,'tool_sha256':report['tool_sha256']}))
        mark(checks,'harness.teardown','not-run','not_reached')
        body_error = None
        try:
            with tempfile.TemporaryDirectory(prefix='conformance-v2-',dir=state_root) as temporary:
                try:
                    root = Path(temporary)
                    executable=copy_package(contents,root/'package')
                    cwd=root/'state';cwd.mkdir(mode=0o700)
                    for name,raw in state_contents.items():
                        destination=cwd/name;destination.write_bytes(raw);destination.chmod(0o600)
                    if role == 'gateway-provider/v1':provider_suite(executable,cwd,manifest,checks,profile)
                    elif role == 'gateway-usage-recorder/v2':recorder_suite(executable,cwd,manifest,checks,profile)
                    else:observer(executable,cwd,checks)
                    mark(checks,phase,'pass')
                except BaseException as error:
                    # Delay re-raising until the scratch context has actually cleaned up.
                    body_error = error
        except OSError as error:
            mark(checks,'harness.teardown','fail','cleanup_failed')
            raise Failure('cleanup_failed') from error
        if isinstance(body_error, Failure) and str(body_error) == 'cleanup_failed':
            mark(checks,'harness.teardown','fail','cleanup_failed')
        else:
            mark(checks,'harness.teardown','pass')
        if body_error is not None:
            raise body_error

    except Failure as error:
        mark(checks,phase,'fail',str(error))
        if str(error)=='cleanup_failed':mark(checks,'harness.teardown','fail','cleanup_failed')
    except (TimeoutError,socket.timeout):mark(checks,phase,'fail','deadline')
    except (OSError,ValueError,TypeError,KeyError,AttributeError,RecursionError):mark(checks,phase,'fail','io_or_structure_error')
    return finish_report(report,execute)


def finish_report(report, execute):
    checks=report['checks']
    failure=next((item.get('code','check_failed') for item in checks if item['status']=='fail'),None)
    if failure:
        for item in checks:
            if item.get('code')=='in_progress':item.update(status='fail',code=failure)
    for item in checks:item['required']=item['id']!='host.integration'
    requested=[item for item in checks if item['required']]
    report['status']='fail' if any(item['status']=='fail' for item in checks) else 'not-run' if any(item['status']=='not-run' for item in requested) else 'pass'
    report['exit_code']=1 if report['status']=='fail' else 2 if execute and report['status']=='not-run' else 0
    return report


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--package',required=True,type=Path)
    parser.add_argument('--expected-sha256',required=True)
    parser.add_argument('--execute',action='store_true')
    parser.add_argument('--state-root',type=Path)
    parser.add_argument('--profile',choices=PROFILES,default='wire')
    parser.add_argument('--recorder-state-fixture',type=Path)
    args=parser.parse_args()
    report=run(args.package,args.expected_sha256,args.execute,args.state_root,args.profile,args.recorder_state_fixture)
    print(canonical(report).decode(),end='')
    return report['exit_code']

EVENT_V1 = {'schema': 'gateway-usage-event/v1', 'producer_id': 'synthetic-producer', 'request_id': 'synthetic-request', 'attempt_id': 'synthetic-attempt', 'event_id': 'synthetic-event', 'revision': 0, 'kind': 'attempt_started', 'started_at_ms': 1, 'observed_at_ms': 1, 'provider': 'synthetic-provider', 'model_alias': 'synthetic', 'upstream_model': 'synthetic', 'reported_model': None, 'provider_request_id': None, 'provider_response_id': None, 'profile': 'responses_v1', 'configuration_sha256': '0000000000000000000000000000000000000000000000000000000000000000', 'upstream': 'in_progress', 'gateway': 'in_progress', 'finality': 'unobserved', 'observation_incomplete': False, 'usage': {'counters': {'input_tokens': {'value': None, 'source': 'not_reported'}, 'output_tokens': {'value': None, 'source': 'not_reported'}, 'total_tokens': {'value': None, 'source': 'not_reported'}, 'input_regular_tokens': {'value': None, 'source': 'not_reported'}, 'cache_read_input_tokens': {'value': None, 'source': 'not_reported'}, 'cache_write_input_tokens': {'value': None, 'source': 'not_reported'}, 'reasoning_output_tokens': {'value': None, 'source': 'not_reported'}}, 'cache_write_details': [], 'reported': {}, 'violations': []}}
EVENT_V2 = {'schema': 'gateway-usage-event/v2', 'producer_id': 'synthetic-producer', 'request_id': 'synthetic-request', 'attempt_id': 'synthetic-attempt', 'event_id': 'synthetic-event', 'revision': 0, 'kind': 'attempt_started', 'started_at_ms': 1, 'observed_at_ms': 1, 'provider': 'synthetic-provider', 'model_alias': 'synthetic', 'upstream_model': 'synthetic', 'reported_model': None, 'provider_request_id': None, 'provider_response_id': None, 'configuration_sha256': '0000000000000000000000000000000000000000000000000000000000000000', 'upstream': 'in_progress', 'gateway': 'in_progress', 'finality': 'unobserved', 'observation_incomplete': False, 'usage': {'counters': {'input_tokens': {'value': None, 'source': 'not_reported'}, 'output_tokens': {'value': None, 'source': 'not_reported'}, 'total_tokens': {'value': None, 'source': 'not_reported'}, 'input_regular_tokens': {'value': None, 'source': 'not_reported'}, 'cache_read_input_tokens': {'value': None, 'source': 'not_reported'}, 'cache_write_input_tokens': {'value': None, 'source': 'not_reported'}, 'reasoning_output_tokens': {'value': None, 'source': 'not_reported'}}, 'cache_write_details': [], 'reported': {}, 'violations': []}, 'interpretation': {'kind': 'trusted_provider_plugin', 'protocol': 'gateway-provider/v1', 'provider_protocol': 'synthetic-provider/v1', 'package_id': 'synthetic-provider', 'package_version': '1.0.0', 'package_sha256': 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'executable_sha256': 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'}}

if __name__ == "__main__":
    raise SystemExit(main())
