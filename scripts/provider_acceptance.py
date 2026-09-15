#!/usr/bin/env python3
"""Installed native provider/Recorder acceptance using disposable synthetic fixtures.

Requires a prebuilt normal host whose public provider activation is enabled by the
integrated implementation. No test-only configuration loader, lock rewrite or gate
bypass is used. Execution evidence is reported only for checks completed by this invocation.
All endpoints/state/credentials are generated synthetic fixtures. No core imports.
"""
import argparse
import contextlib
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import platform
import secrets
import signal
import selectors
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import threading
import time
from urllib.error import HTTPError
from urllib.parse import urlsplit
from urllib.request import HTTPRedirectHandler, ProxyHandler, Request, build_opener

TOOL_VERSION = '1.0.0'
MAX_BODY = 2 * 1024 * 1024
TOKEN = 'L' * 40
UPSTREAM_KEY = 'synthetic-upstream-key'
CASES = ['builtin', 'json', 'sse', 'tool_call', 'tool_result', 'numeric_invalid', 'numeric_missing', 'managed_first', 'managed_resume']
REMAINING = [
    'unsupported host capabilities and manifest/Ready mismatch',
    'provider timeout/cancellation/partial frame/unexpected exit and child reap',
    'recording failure and publication barrier',
    'decreasing numeric evidence and incompatible Recorder v1',
    'offline Linux container and cross-target/native-platform evidence',
    'shipped SQLite management/team/Web positive consumer qualification',
]


class Failure(ValueError):
    pass


def require(condition, code):
    if not condition:
        raise Failure(code)


def encode(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False, allow_nan=False).encode()


def decode(raw):
    def pairs(items):
        result = {}
        for name, value in items:
            require(name not in result, 'duplicate_json_field')
            result[name] = value
        return result
    return json.loads(raw, object_pairs_hook=pairs,
                      parse_constant=lambda _: (_ for _ in ()).throw(Failure('invalid_json_number')))


def digest(path):
    before = path.stat()
    hasher = hashlib.sha256()
    with path.open('rb') as source:
        for chunk in iter(lambda: source.read(65536), b''):
            hasher.update(chunk)
    after = path.stat()
    require((before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns)
            == (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns), 'changed_artifact')
    return hasher.hexdigest()


def command(arguments, cwd, env, *, success=True, timeout=30):
    # A dedicated group contains builder subprocesses as well as their parent.
    # On failure only this group is killed. No command stderr/body enters reports.
    child = subprocess.Popen([str(a) for a in arguments], cwd=cwd, env=env,
                             stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                             stderr=subprocess.DEVNULL, start_new_session=True)
    raw = bytearray()
    deadline = time.monotonic() + timeout
    failed = True
    try:
        with selectors.DefaultSelector() as selector:
            selector.register(child.stdout, selectors.EVENT_READ)
            while True:
                remaining = deadline - time.monotonic()
                require(remaining > 0 and bool(selector.select(remaining)), 'command_deadline')
                part = os.read(child.stdout.fileno(), 65536)
                if not part:
                    break
                raw.extend(part)
                require(len(raw) <= MAX_BODY, 'command_output_limit')
        try:
            child.wait(timeout=max(0.001, deadline - time.monotonic()))
        except subprocess.TimeoutExpired as error:
            raise Failure('command_deadline') from error
        require((child.returncode == 0) == success, 'command_status')
        failed = False
        return bytes(raw)
    finally:
        try:
            if failed:
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
        finally:
            try:
                child.wait(timeout=3)
            except subprocess.TimeoutExpired as error:
                raise Failure('command_cleanup_deadline') from error
            finally:
                child.stdout.close()


def target():
    system = {'darwin': 'macos', 'linux': 'linux'}.get(sys.platform)
    arch = {'arm64': 'arm64', 'aarch64': 'arm64', 'x86_64': 'x64', 'amd64': 'x64'}.get(platform.machine().lower())
    require(system is not None and arch is not None, 'unsupported_native_host')
    return f'{system}-{arch}'


def copy_project(source, destination):
    require(source.is_dir() and not source.is_symlink(), 'example_directory')
    require(not (source == destination or source in destination.parents), 'example_copy_location')
    destination.mkdir(mode=0o700)
    files = {}
    for path in sorted(source.iterdir()):
        if path.name == '__pycache__':
            continue
        require(path.is_file() and not path.is_symlink() and path.stat().st_nlink == 1, 'example_file_type')
        require(path.stat().st_size <= 256 * 1024 and len(files) < 16, 'example_copy_limit')
        files[path.name] = digest(path)
        shutil.copyfile(path, destination / path.name)
        require(digest(destination / path.name) == files[path.name] == digest(path), 'example_copy_changed')
        (destination / path.name).chmod(0o600)
    require({'package.py', 'build.py', 'LICENSE.txt'} <= files.keys(), 'example_source_incomplete')
    return hashlib.sha256(encode(files)).hexdigest()


def stop(child):
    if child.__dict__.get('_acceptance_cleaned', False):
        return
    running = child.poll() is None
    try:
        if running:
            child.terminate()
            try:
                child.wait(timeout=6)
            except subprocess.TimeoutExpired as error:
                child.kill()
                try:
                    child.wait(timeout=2)
                except subprocess.TimeoutExpired as cleanup_error:
                    raise Failure('gateway_cleanup_deadline') from cleanup_error
                raise Failure('gateway_shutdown_deadline') from error
    finally:
        try:
            # The owned process group can survive an already exited host.
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        finally:
            if child.stdout:
                child.stdout.close()
            child._acceptance_cleaned = True


def read_ready(child):
    raw = bytearray()
    deadline = time.monotonic() + 6
    with selectors.DefaultSelector() as selector:
        selector.register(child.stdout, selectors.EVENT_READ)
        while not raw.endswith(b'\n'):
            remaining = deadline - time.monotonic()
            require(remaining > 0 and bool(selector.select(remaining)), 'gateway_ready_deadline')
            part = os.read(child.stdout.fileno(), min(4096, 65537 - len(raw)))
            require(bool(part), 'gateway_exited_before_ready')
            raw.extend(part)
            require(len(raw) <= 65536, 'gateway_ready_limit')
    require(raw.count(b'\n') == 1, 'gateway_extra_ready_output')
    return decode(bytes(raw))


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, *_args, **_kwargs):
        return None


def request(client, url, body, *, token=TOKEN, session=None):
    parsed = urlsplit(url)
    require(parsed.scheme == 'http' and parsed.hostname == '127.0.0.1', 'non_synthetic_endpoint')
    headers = {'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json'}
    if session is not None:
        headers['x-gateway-session'] = session
    try:
        return client.open(Request(url, data=None if body is None else encode(body), headers=headers), timeout=10)
    except HTTPError as error:
        error.close()
        raise Failure('unexpected_http_status') from error


def rejected_request(client, url, body, *, status, code, token=TOKEN, session=None):
    parsed = urlsplit(url)
    require(parsed.scheme == 'http' and parsed.hostname == '127.0.0.1', 'non_synthetic_endpoint')
    headers = {'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json'}
    if session is not None:
        headers['x-gateway-session'] = session
    try:
        with client.open(Request(url, data=encode(body), headers=headers), timeout=10):
            raise Failure('expected_http_rejection')
    except HTTPError as error:
        with error:
            raw = error.read(MAX_BODY + 1)
            require(len(raw) <= MAX_BODY and error.code == status, 'rejection_status')
            require(decode(raw).get('error', {}).get('code') == code, 'rejection_code')


def json_request(client, url, body, **kwargs):
    with request(client, url, body, **kwargs) as response:
        require(response.status == 200, 'unexpected_http_status')
        raw = response.read(MAX_BODY + 1)
        require(len(raw) <= MAX_BODY, 'response_limit')
        return decode(raw)


class Upstream(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        self.connection.settimeout(5)
        try:
            if self.server.hold_request:
                size=int(self.headers.get('Content-Length','-1'))
                require(0 <= size <= MAX_BODY,'upstream_request_limit')
                body=decode(self.rfile.read(size))
                require(self.path=='/vendor/generate' and self.headers.get('Authorization')=='Bearer '+UPSTREAM_KEY
                        and body.get('cursor')==0,'interrupted_request_shape')
                self.server.request_started.set()
                require(self.server.release_request.wait(10),'interrupted_request_deadline')
                return
            with self.server.guard:
                index = len(self.server.observed)
                require(index < len(CASES), 'unexpected_upstream_call')
                case = CASES[index]
                self.server.observed.append({'case': case})
            size = int(self.headers.get('Content-Length', '-1'))
            require(0 <= size <= MAX_BODY, 'upstream_request_limit')
            body = decode(self.rfile.read(size))
            require(self.headers.get('Authorization') == 'Bearer ' + UPSTREAM_KEY, 'host_auth_mismatch')
            if case == 'builtin':
                require(self.path == '/vendor/responses' and body['model'] == 'synthetic-model', 'builtin_route')
                raw = encode({'id': 'synthetic-builtin', 'object': 'response', 'created_at': 0,
                              'model': 'synthetic-model', 'status': 'completed', 'output': [],
                              'usage': {'input_tokens': 7, 'output_tokens': 2, 'total_tokens': 9}})
                media = 'application/json'
            else:
                require(self.path == '/vendor/generate' and isinstance(body, dict) and 'model' not in body, 'provider_transport')
                require(body['query']['model'] == 'synthetic-model', 'provider_model')
                require('arg-continuation' not in json.dumps(body), 'protected_envelope_forwarded')
                expected_cursor = 1 if case == 'managed_resume' else 0
                require(type(body['cursor']) is int and body['cursor'] == expected_cursor, 'provider_state_cursor')
                with self.server.guard:
                    self.server.observed[index]['cursor'] = body['cursor']
                if case == 'tool_call':
                    require(any(tool.get('name') == 'lookup' for tool in body['query']['tools']), 'admitted_tool_missing')
                    answer = [{'call': 'call_1', 'name': 'lookup', 'arguments': '{}'}]
                else:
                    if case == 'tool_result':
                        require(any(item.get('type') == 'function_call_output' and item.get('call_id') == 'call_1'
                                    and item.get('output') == 'synthetic-result' for item in body['query']['input']), 'tool_result_missing')
                    answer = [{'text': 'managed 1' if case == 'managed_resume' else 'managed 0' if case == 'managed_first' else 'hello'}]
                native = {'answer': answer, 'meter': {'input_tokens': 7, 'output_tokens': 2, 'total_tokens': 9}}
                if case == 'numeric_invalid':
                    native['meter']['input_tokens'] = -1
                elif case == 'numeric_missing':
                    native.pop('meter')
                raw = encode(native)
                media = 'application/json'
                if case == 'sse':
                    require(body['query'].get('stream') is True, 'stream_not_requested')
                    prefix = b'event: piece\ndata: {"text":"hel"}\n\nevent: piece\ndata: {"text":"lo"}\n\n'
                    end = b'event: end\ndata: ' + raw + b'\n\n'
                    self.send_response(200)
                    self.send_header('Content-Type', 'text/event-stream')
                    self.send_header('Content-Length', str(len(prefix) + len(end)))
                    self.end_headers()
                    self.wfile.write(prefix);self.wfile.flush()
                    require(self.server.release_terminal.wait(7), 'incremental_stream_deadline')
                    self.wfile.write(end);self.wfile.flush()
                    return
            self.send_response(200)
            self.send_header('Content-Type', media)
            self.send_header('Content-Length', str(len(raw)))
            self.end_headers();self.wfile.write(raw)
        except Exception as error:
            with self.server.guard:
                self.server.failure = str(error) if isinstance(error, Failure) else 'synthetic_upstream_failure'
            try:
                self.send_response(500);self.end_headers()
            except OSError:
                pass


def config_text(upstream, continuation, store_id):
    value = f'listen="127.0.0.1:0"\n[providers.synthetic]\nbase_url="{upstream}/vendor"\napi_key_env="SYNTHETIC_KEY"\n'
    value += '[models.builtin]\nprovider="synthetic"\nupstream_model="synthetic-model"\napi="responses"\n'
    for alias in ('stateless', 'managed'):
        value += f'''[models.{alias}]
provider="synthetic"
upstream_model="synthetic-model"
api="plugin"
auth="bearer"
provider_plugin="synthetic-provider"
provider_protocol="synthetic-provider/v1"
provider_path="generate"
capability_profile="synthetic"
continuation_mode="{alias}"
'''
    value += '''[capability_profiles.synthetic]
version="1"
provider="synthetic"
upstream_model="synthetic-model"
api="plugin"
context_window=32768
max_output_tokens=1024
tested_codex_version="0.154.0"
[capability_profiles.synthetic.support]
function_tools="native"
tool_choice="native"
max_output_tokens="native"
'''
    value += f'''[continuation]
directory={json.dumps(str(continuation))}
store_id={json.dumps(store_id)}
realm="synthetic"
generation="1"
key_id="synthetic-key"
key_env="SYNTHETIC_CONTINUATION_KEY"
control_token_env="SYNTHETIC_CONTROL_TOKEN"
max_store_bytes=67108864
'''
    return value


def history_message(text):
    return {'type': 'message', 'role': 'user', 'content': [{'type': 'input_text', 'text': text}]}


def stream_request(client, base, upstream):
    with request(client, base + '/responses', {'model': 'stateless', 'input': 'hello', 'stream': True}) as response:
        require(response.status == 200, 'stream_status')
        events, raw, size = [], bytearray(), 0
        deadline = time.monotonic() + 15
        try:
            while True:
                require(time.monotonic() < deadline, 'stream_deadline')
                line = response.readline(65537)
                if not line:
                    break
                size += len(line);require(size <= MAX_BODY, 'stream_limit')
                raw.extend(line)
                if line in (b'\n', b'\r\n'):
                    for part in bytes(raw).splitlines():
                        if part.startswith(b'data: '):
                            event = decode(part[6:]);events.append(event)
                            if event.get('type') == 'response.output_text.delta':
                                require(not any(item.get('type') in ('response.completed','response.incomplete') for item in events), 'premature_terminal')
                                upstream.release_terminal.set()
                    raw.clear()
        finally:
            upstream.release_terminal.set()
        require(not raw and any(e.get('type') == 'response.output_text.delta' for e in events), 'missing_stream_progress')
        terminals = [e for e in events if e.get('type') == 'response.completed']
        require(len(terminals) == 1, 'missing_stream_terminal')
        require([e.get('sequence_number') for e in events] == list(range(len(events))), 'stream_sequence')
        require(all(type(e.get('sequence_number')) is int for e in events), 'stream_sequence')
        return terminals[0]['response']


def ledger_rows(directory):
    path = directory / 'events.sqlite3'
    with sqlite3.connect(path.as_uri() + '?mode=ro', uri=True) as db:
        return db.execute('SELECT event_id,payload,sha256 FROM events ORDER BY event_id').fetchall()


def inspect_usage(directory, producer, provider_manifest, provider_sha):
    rows = ledger_rows(directory)
    expected = {'kind': 'trusted_provider_plugin', 'protocol': 'gateway-provider/v1',
                'provider_protocol': 'synthetic-provider/v1', 'package_id': provider_manifest['id'],
                'package_version': provider_manifest['version'], 'package_sha256': provider_sha,
                'executable_sha256': provider_manifest['files']['extension']}
    finished, versions, sources = 0, set(), set()
    attempts, terminal_ids, version_counts = {}, set(), {}
    for _event_id, raw, checksum in rows:
        require(hashlib.sha256(raw).hexdigest() == checksum, 'stored_event_hash')
        value = decode(raw)
        require(encode(value) == raw and value['producer_id'] == producer, 'stored_event_identity')
        versions.add(value['schema'])
        attempt=(value['producer_id'],value['attempt_id'])
        kinds=attempts.setdefault(attempt,[])
        kinds.append(value['kind'])
        if value['schema'] == 'gateway-usage-event/v2':
            require(value.get('interpretation') == expected and 'profile' not in value, 'stored_plugin_provenance')
            require(value['usage']['reported'] == {} and value['usage']['cache_write_details'] == [], 'provider_usage_shape')
        else:
            require(value['schema'] == 'gateway-usage-event/v1' and 'interpretation' not in value, 'legacy_event_version')
        if value['kind'] == 'attempt_finished':
            require(attempt not in terminal_ids,'duplicate_terminal_attempt')
            terminal_ids.add(attempt)
            version_counts[value['schema']]=version_counts.get(value['schema'],0)+1
            finished += 1
            counters = value['usage']['counters']
            sources.add(counters['input_tokens']['source'])
            if counters['input_tokens']['source'] == 'invalid':
                require(counters['input_tokens']['value'] is None and counters['output_tokens']['value'] == 2
                        and value['gateway'] == 'conversion_failed' and value['observation_incomplete'], 'invalid_counter_evidence')
            elif counters['input_tokens']['source'] == 'not_reported':
                require(value['gateway']=='completed' and value['upstream']=='completed','missing_usage_outcome')
                require(all(c['value'] is None and c['source'] == 'not_reported' for c in counters.values()), 'missing_counter_evidence')
            else:
                require(value['gateway']=='completed' and value['upstream']=='completed','successful_usage_outcome')
                require(counters['input_tokens']['value'] == 7 and counters['output_tokens']['value'] == 2, 'stored_counter')
            require(value['usage']['counters']['cache_read_input_tokens']['value'] is None, 'unknown_counter_became_zero')
    require(versions == {'gateway-usage-event/v1','gateway-usage-event/v2'} and finished == len(CASES), 'usage_attempt_coverage')
    require(sources == {'reported', 'invalid', 'not_reported'}, 'numeric_failure_coverage')
    require(version_counts=={'gateway-usage-event/v1':1,'gateway-usage-event/v2':len(CASES)-1},'terminal_version_coverage')
    require(set(attempts)==terminal_ids and all(kinds.count('attempt_started')==1 and kinds.count('attempt_finished')==1
            for kinds in attempts.values()),'attempt_lifecycle_coverage')
    return len(rows)


def run(args):
    report = {'schema': 'gateway-plugin-acceptance-report/v1', 'tool_version': TOOL_VERSION, 'status': 'not-run',
              'contracts': ['gateway-extension-package/v2', 'gateway-provider/v1', 'gateway-usage-recorder/v2',
                            'gateway-usage-event/v1', 'gateway-usage-event/v2', 'gateway-extended-manifest/v11'],
              'platform': None, 'binary_sha256': None, 'checks': [],
              'remaining_acceptance': [{'id': name, 'status': 'not-run'} for name in REMAINING]}
    check = lambda name: report['checks'].append({'id': name, 'status': 'pass'})
    phase = 'independent_build_install'
    try:
        for key in ('binary','manager','provider_example','recorder_example','python','query_recorder','work_root'):
            if getattr(args,key) is not None:
                setattr(args,key,getattr(args,key).resolve())
        report.update(platform=target(), binary_sha256=digest(args.binary))
        with tempfile.TemporaryDirectory(prefix='provider-acceptance-', dir=args.work_root) as temporary, contextlib.ExitStack() as cleanup:
            root = Path(temporary).resolve();root.chmod(0o700)
            require(not any((p / '.git').exists() for p in root.parents), 'acceptance_workspace_inside_checkout')
            projects = root / 'projects';projects.mkdir(mode=0o700)
            tools = root / 'tools';tools.mkdir(mode=0o700)
            manager = tools / 'extension_manager.py';shutil.copyfile(args.manager, manager)
            report['manager_sha256'] = digest(manager)
            provider_source = projects / 'provider';recorder_source = projects / 'recorder'
            report['provider_source_sha256'] = copy_project(args.provider_example, provider_source)
            report['recorder_source_sha256'] = copy_project(args.recorder_example, recorder_source)
            env = {'ARG_LOCAL_TOKEN': TOKEN, 'SYNTHETIC_KEY': UPSTREAM_KEY,
                   'SYNTHETIC_CONTINUATION_KEY': secrets.token_hex(32), 'SYNTHETIC_CONTROL_TOKEN': secrets.token_urlsafe(32)}
            packages = {}
            for role, project in [('provider',provider_source),('recorder',recorder_source)]:
                package = root / (role + '-package')
                checksum = command([args.python,'-B',project/'package.py','--output',package,'--target',report['platform']], root, env).decode().strip()
                manifest = decode((package/'extension.json').read_bytes())
                require(digest(package/'extension.json') == checksum, 'package_digest')
                packages[role] = (package, checksum, manifest)
            store = root / 'extensions'
            for package, checksum, _manifest in packages.values():
                command([args.python,'-I','-B',manager,'install','--store',store,'--package',package,'--expected-sha256',checksum],root,env)
            usage_parent=store/'usage';usage_parent.mkdir(mode=0o700)
            ledger=usage_parent/'independent';ledger.mkdir(mode=0o700)
            recorder_package,recorder_sha,recorder_manifest=packages['recorder']
            installed_recorder=store/'packages'/recorder_manifest['id']/recorder_manifest['version']/recorder_sha/'extension'
            producer=command([installed_recorder,'init'],ledger,env).decode().strip()
            config_raw=encode({'schema':'gateway-usage-recorder-config/v1','destinations':[]})
            (ledger/'recorder.json').write_bytes(config_raw);(ledger/'recorder.json').chmod(0o600)
            binding={'store_id':'independent','mode':'durable_local','queue_capacity':256,'ack_timeout_ms':5000,
                     'config_sha256':hashlib.sha256(config_raw).hexdigest()}
            binding_file=root/'recorder-binding.json';binding_file.write_bytes(encode(binding));binding_file.chmod(0o600)
            command([args.python,'-I','-B',manager,'enable','--store',store,'--id',recorder_manifest['id'],'--version',recorder_manifest['version'],
                     '--package-sha256',recorder_sha,'--grant','export_usage','--grant','observe_usage','--grant','write_usage_store',
                     '--recorder-binding',binding_file],root,env)
            provider_package,provider_sha,provider_manifest=packages['provider']
            # Intentionally uses ordinary public enable; a gated predecessor fails here.
            command([args.python,'-I','-B',manager,'enable','--store',store,'--id',provider_manifest['id'],'--version',provider_manifest['version'],
                     '--package-sha256',provider_sha,'--grant','read_model_payload','--grant','transform_model_protocol'],root,env)
            require(digest(args.binary)==report['binary_sha256'],'gateway_binary_changed')
            report.update(provider_package_sha256=provider_sha,recorder_package_sha256=recorder_sha)
            check(phase)
            upstream=ThreadingHTTPServer(('127.0.0.1',0),Upstream)
            upstream.daemon_threads=True;upstream.guard=threading.Lock();upstream.observed=[];upstream.failure=None
            upstream.release_terminal=threading.Event()
            upstream.hold_request=False;upstream.request_started=threading.Event();upstream.release_request=threading.Event()
            thread=threading.Thread(target=upstream.serve_forever,kwargs={'poll_interval':0.05},daemon=True);thread.start()
            def stop_upstream():
                upstream.release_terminal.set();upstream.release_request.set();upstream.shutdown();upstream.server_close();thread.join(timeout=3)
                require(not thread.is_alive(),'upstream_shutdown_deadline')
            cleanup.callback(stop_upstream)
            continuation=root/'continuation';continuation.mkdir(mode=0o700)
            initialized=decode(command([args.binary,'init-continuation','--directory',continuation,'--max-store-bytes','67108864'],root,env))
            config=root/'gateway.toml';config.write_text(config_text(f'http://127.0.0.1:{upstream.server_port}',continuation,initialized['store_id']))
            config.chmod(0o600)
            invocation=['--config',config,'--extensions-lock',store/'active.json']
            manifest=decode(command([args.binary,'manifest',*invocation],root,env))
            require(manifest['schema']=='gateway-extended-manifest/v11','host_manifest_version')
            configuration=manifest['configuration'];base=configuration['gateway']
            require(configuration['usage_event_schemas']==['gateway-usage-event/v1','gateway-usage-event/v2'],'host_event_contract')
            require(base['schema']=='gateway-embedded-manifest/v10' and base['configuration']['replay_versions']=={'read':[1,2,3],'write_builtin':2,'write_provider':3},'host_replay_contract')
            require(hashlib.sha256(encode(configuration)).hexdigest()==manifest['execution_sha256'],'host_execution_digest')
            client=build_opener(ProxyHandler({}),NoRedirect())
            children=[]
            def launch():
                child=subprocess.Popen([str(args.binary),'serve',*[str(a) for a in invocation]],cwd=root,env=env,stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,start_new_session=True)
                children.append(child);cleanup.callback(stop,child)
                ready=read_ready(child)
                require(ready['schema']=='gateway-extended-ready/v11' and ready['manifest_schema']==manifest['schema']
                        and ready['execution_sha256']==manifest['execution_sha256']
                        and ready['configuration_sha256']==base['configuration_sha256'],'ready_binding')
                return child,ready['base_url']
            child,base_url=launch();check('installed_host_ready')
            phase='json_sse_tools'
            json_request(client,base_url+'/responses',{'model':'builtin','input':'synthetic'})
            plain=json_request(client,base_url+'/responses',{'model':'stateless','input':'hello'})
            require(plain['output'][0]['content'][0]['text']=='hello','json_output')
            streamed=stream_request(client,base_url,upstream)
            require(streamed['output'][0]['content'][0]['text']=='hello','stream_output')
            tools=[{'type':'function','name':'lookup','parameters':{'type':'object','properties':{},'required':[],'additionalProperties':False}}]
            first_input=[history_message('lookup')]
            call=json_request(client,base_url+'/responses',{'model':'stateless','input':first_input,'tools':tools})
            require(any(item.get('type')=='function_call' and item.get('call_id')=='call_1' for item in call['output']),'tool_call_output')
            tool_input=[*first_input,*call['output'],{'type':'function_call_output','call_id':'call_1','output':'synthetic-result'}]
            json_request(client,base_url+'/responses',{'model':'stateless','input':tool_input,'tools':tools})
            check(phase)
            phase='numeric_failure_and_missing'
            rejected_request(client,base_url+'/responses',{'model':'stateless','input':'hello'},status=502,code='upstream_invalid_response')
            missing=json_request(client,base_url+'/responses',{'model':'stateless','input':'hello'})
            require('usage' not in missing or all(missing['usage'].get(k) is None for k in ('input_tokens','output_tokens','total_tokens')), 'missing_usage_became_zero')
            check(phase)
            phase='managed_process_restart'
            route=next(dict(r) for r in base['configuration']['routes'] if r['alias']=='managed');route.pop('api_key_env',None)
            control_base=base_url.removesuffix('/v1')
            session=json_request(client,control_base+'/__continuation/sessions',{'origin':{'route':route,'realm':'synthetic','generation':'1'}},token=env['SYNTHETIC_CONTROL_TOKEN'])
            first_history=[history_message('managed-first')]
            first=json_request(client,base_url+'/responses',{'model':'managed','input':first_history},session=session['id'])
            require(any(str(item.get('encrypted_content','')).startswith('arg-continuation-v3.') for item in first['output']),'managed_envelope_missing')
            before_rows=ledger_rows(ledger)
            first_pid=child.pid;stop(child)
            child,restarted_url=launch();require(child.pid!=first_pid,'gateway_not_restarted')
            resume=[*first_history,*first['output'],history_message('managed-second')]
            second=json_request(client,restarted_url+'/responses',{'model':'managed','input':resume},session=session['id'])
            require(any(item.get('type')=='message' and item['content'][0].get('text')=='managed 1' for item in second['output']),'managed_resume_output')
            check(phase)
            phase='continuation_rejections'
            control_base=restarted_url.removesuffix('/v1')
            status_url=control_base+'/__continuation/sessions/'+session['id']
            current=json_request(client,status_url,None,token=env['SYNTHETIC_CONTROL_TOKEN'])
            rejected_request(client,status_url+'/transitions',{'revision':session['revision'],'kind':'recover','portable_sha256':hashlib.sha256(encode([history_message('portable')])).hexdigest(),
                             'decision_reference':'synthetic-stale','pending_tools':False,'pending_approvals':False},
                             status=409,code='continuation_rejected',token=env['SYNTHETIC_CONTROL_TOKEN'])
            other=json_request(client,control_base+'/__continuation/sessions',{'origin':{'route':route,'realm':'synthetic','generation':'1'}},token=env['SYNTHETIC_CONTROL_TOKEN'])
            rejected_request(client,restarted_url+'/responses',{'model':'managed','input':resume},session=other['id'],status=409,code='continuation_rejected')
            corrupt=decode(encode([*resume,*second['output'],history_message('managed-third')]))
            for item in corrupt:
                if 'encrypted_content' in item:
                    token=item['encrypted_content'];item['encrypted_content']=token[:-1]+('A' if token[-1]!='A' else 'B');break
            rejected_request(client,restarted_url+'/responses',{'model':'managed','input':corrupt},session=session['id'],status=409,code='continuation_rejected')
            require(json_request(client,status_url,None,token=env['SYNTHETIC_CONTROL_TOKEN']) == current,'rejection_changed_session')
            stop(child);check(phase)
            phase='recorded_provenance'
            rows=ledger_rows(ledger);row_map={row[0]:row[1:] for row in rows}
            require(all(row_map[row[0]]==row[1:] for row in before_rows),'historical_usage_rewritten')
            report['stored_event_count']=inspect_usage(ledger,producer,provider_manifest,provider_sha)
            require(upstream.failure is None and [v['case'] for v in upstream.observed]==CASES,'upstream_call_coverage')
            require(upstream.observed[-1]['cursor']==1,'state_not_resumed')
            check(phase)
            phase='changed_package_session_refused'
            changed_project=projects/'provider-changed'
            copy_project(provider_source,changed_project)
            with (changed_project/'provider.py').open('ab') as changed:
                changed.write(b'\n# Synthetic alternate package identity.\n')
            changed_package=root/'provider-changed-package'
            changed_sha=command([args.python,'-B',changed_project/'package.py','--output',changed_package,'--target',report['platform']],root,env).decode().strip()
            require(changed_sha != provider_sha and digest(changed_package/'extension.json') == changed_sha,'changed_package_identity')
            command([args.python,'-I','-B',manager,'install','--store',store,'--package',changed_package,'--expected-sha256',changed_sha],root,env)
            def select_provider(checksum):
                command([args.python,'-I','-B',manager,'enable','--store',store,'--id',provider_manifest['id'],'--version',provider_manifest['version'],
                         '--package-sha256',checksum,'--grant','read_model_payload','--grant','transform_model_protocol'],root,env)
            select_provider(changed_sha)
            manifest=decode(command([args.binary,'manifest',*invocation],root,env))
            base=manifest['configuration']['gateway']
            changed_child,changed_url=launch()
            rejected_request(client,changed_url+'/responses',{'model':'managed','input':[*resume,*second['output'],history_message('managed-third')]},
                             session=session['id'],status=409,code='continuation_rejected')
            require(len(upstream.observed)==len(CASES),'changed_package_disclosed_history')
            stop(changed_child)
            select_provider(provider_sha)
            manifest=decode(command([args.binary,'manifest',*invocation],root,env));base=manifest['configuration']['gateway']
            restored_child,restored_url=launch()
            restored=json_request(client,restored_url.removesuffix('/v1')+'/__continuation/sessions/'+session['id'],None,token=env['SYNTHETIC_CONTROL_TOKEN'])
            require(restored==current,'package_restore_changed_session')
            stop(restored_child);check(phase)
            phase='damaged_checkpoint_refused'
            db_path=continuation/'continuation.sqlite3'
            with sqlite3.connect(db_path) as db:
                saved=db.execute('SELECT digest,envelope FROM records WHERE id=?',(current['head'],)).fetchone()
                require(saved is not None and isinstance(saved[1],str),'checkpoint_fixture_missing')
                damaged=saved[1]+'x'
                db.execute('UPDATE records SET envelope=? WHERE id=?',(damaged,current['head']))
            damaged_child,damaged_url=launch()
            rejected_request(client,damaged_url+'/responses',{'model':'managed','input':[*resume,*second['output'],history_message('managed-third')]},
                             session=session['id'],status=409,code='continuation_rejected')
            require(len(upstream.observed)==len(CASES),'damaged_checkpoint_disclosed_history')
            stop(damaged_child)
            with sqlite3.connect(db_path) as db:
                require(db.execute('SELECT digest,envelope FROM records WHERE id=?',(current['head'],)).fetchone()==(saved[0],damaged),'damaged_checkpoint_silently_repaired')
                db.execute('UPDATE records SET envelope=? WHERE id=?',(saved[1],current['head']))
            check(phase)
            if args.query_recorder is not None:
                phase='foreign_query_backend_rejected'
                before=digest(ledger/'events.sqlite3')
                command([args.query_recorder,'query','--store',ledger],root,env,success=False)
                require(not (ledger/'usage.sqlite3').exists() and digest(ledger/'events.sqlite3')==before,'query_backend_mutated_foreign_state')
                check(phase)
            else:
                report['checks'].append({'id':'foreign_query_backend_rejected','status':'not-run','code':'query_binary_not_supplied'})
            phase='interrupted_attempt_recovery'
            interrupted_child,interrupted_url=launch()
            control=interrupted_url.removesuffix('/v1')
            interrupted_session=json_request(client,control+'/__continuation/sessions',{'origin':{'route':route,'realm':'synthetic','generation':'1'}},token=env['SYNTHETIC_CONTROL_TOKEN'])
            upstream.hold_request=True
            interrupted_result=[]
            def interrupted_call():
                try:
                    json_request(client,interrupted_url+'/responses',{'model':'managed','input':[history_message('interrupted')]},session=interrupted_session['id'])
                    interrupted_result.append('unexpected_success')
                except Exception:
                    interrupted_result.append('interrupted')
            interrupted_thread=threading.Thread(target=interrupted_call,daemon=True);interrupted_thread.start()
            require(upstream.request_started.wait(6),'interrupted_request_not_started')
            os.killpg(interrupted_child.pid,signal.SIGKILL);interrupted_child.wait(timeout=3)
            upstream.release_request.set();interrupted_thread.join(timeout=5)
            require(not interrupted_thread.is_alive() and interrupted_result==['interrupted'],'interrupted_request_cleanup')
            upstream.hold_request=False
            recovered_child,recovered_url=launch()
            control=recovered_url.removesuffix('/v1')
            status_url=control+'/__continuation/sessions/'+interrupted_session['id']
            uncertain=json_request(client,status_url,None,token=env['SYNTHETIC_CONTROL_TOKEN'])
            require(uncertain['status']=='unknown' and uncertain['head'] is None,'unfinished_attempt_not_uncertain')
            rejected_request(client,recovered_url+'/responses',{'model':'managed','input':[history_message('interrupted')]},session=interrupted_session['id'],status=409,code='continuation_rejected')
            portable=hashlib.sha256(encode([history_message('portable')])).hexdigest()
            recovered=json_request(client,status_url+'/transitions',{'revision':uncertain['revision'],'kind':'recover','portable_sha256':portable,
                                   'decision_reference':'synthetic-recovery','pending_tools':False,'pending_approvals':False},token=env['SYNTHETIC_CONTROL_TOKEN'])
            require(recovered['status']=='ready' and recovered['head'] is None and recovered['epoch']==uncertain['epoch']+1
                    and recovered['portable_sha256']==portable,'explicit_recovery_failed')
            stop(recovered_child);check(phase)
            require(digest(args.binary)==report['binary_sha256'],'gateway_binary_changed')
            check('binary_unchanged')
            phase='harness_cleanup'
        check('harness_cleanup')
        report['status']='pass_positive_scope'
    except Failure as error:
        unsupported=str(error)=='unsupported_native_host'
        report['status']='not-run' if unsupported else 'fail'
        report['checks'].append({'id':phase,'status':'not-run' if unsupported else 'fail',
                                 'code':'platform_unsupported' if unsupported else str(error)})
    except (OSError,ValueError,TypeError,KeyError,StopIteration,sqlite3.Error,subprocess.SubprocessError):
        report['status']='fail';report['checks'].append({'id':phase,'status':'fail','code':'fixture_or_io_failure'})
    return report


def write_report(path, report):
    # Write only the requested artifact, atomically, with private temporary bytes.
    destination=path.absolute()
    require(destination.parent.is_dir() and not destination.is_symlink()
            and all(not parent.is_symlink() for parent in destination.parents),'report_output_path')
    temporary=None
    try:
        with tempfile.NamedTemporaryFile(prefix='.acceptance-report-',dir=destination.parent,delete=False) as output:
            temporary=Path(output.name)
            output.write(encode(report)+b'\n');output.flush();os.fsync(output.fileno())
        os.replace(temporary,destination)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ('binary','manager','provider-example','recorder-example'):
        parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--python',type=Path,default=Path(sys.executable))
    parser.add_argument('--query-recorder',type=Path)
    parser.add_argument('--work-root',type=Path,help='Disposable parent outside any checkout; defaults to OS temporary directory')
    parser.add_argument('--output',type=Path,help='Write the same redacted report atomically to this file')
    args=parser.parse_args()
    report=run(args)
    if args.output is not None:
        try:
            write_report(args.output,report)
        except (OSError,Failure):
            report['status']='fail';report['checks'].append({'id':'report_output','status':'fail','code':'report_write_failed'})
    print(encode(report).decode())
    return 1 if report['status']=='fail' else 2 if report['status']=='not-run' else 0


if __name__=='__main__':
    raise SystemExit(main())
