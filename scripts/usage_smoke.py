#!/usr/bin/env python3
"""Synthetic three-protocol accounting, actual recorder IPC and HTTP outbox smoke."""
import argparse
import contextlib
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import threading
from urllib.request import Request, ProxyHandler, build_opener

import extension_manager as manager
from extension_smoke import read_ready, terminate, wait_for
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tests/codex'))
import conformance


def encode(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode()


class Upstream(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        streaming = body.get('stream', False)
        usage = {'input_tokens': 17, 'output_tokens': 3, 'total_tokens': 20,
                 'input_tokens_details': {'cached_tokens': 5, 'cache_write_tokens': 2}}
        if self.path.endswith('/messages'):
            usage = {'input_tokens': 10, 'output_tokens': 3, 'cache_read_input_tokens': 5, 'cache_creation_input_tokens': 2}
            value = {'id': 'synthetic', 'type': 'message', 'role': 'assistant', 'model': 'synthetic-model',
                     'content': [{'type': 'text', 'text': 'synthetic-output'}], 'stop_reason': 'end_turn', 'usage': usage}
            if streaming:
                frames = conformance.messages_frames(value['content'], 1)
                start = json.loads(frames[0].decode().split('data: ', 1)[1])
                start['message']['usage'].update(usage)
                start['message']['usage']['output_tokens'] = 1
                frames[0] = conformance.event('message_start', message=start['message'])
                raw = b''.join(frames)
        elif self.path.endswith('/chat/completions'):
            usage = {'prompt_tokens': 17, 'completion_tokens': 3, 'total_tokens': 20,
                     'prompt_tokens_details': {'cached_tokens': 5}}
            value = {'id': 'chat_fixture_1', 'object': 'chat.completion', 'created': 0, 'model': 'synthetic-model',
                     'choices': [{'index': 0, 'message': {'role': 'assistant', 'content': 'synthetic-output'}, 'finish_reason': 'stop'}], 'usage': usage}
            if streaming:
                frames = [conformance.chat_chunk({'role': 'assistant', 'content': 'synthetic-output'}, 1), conformance.chat_chunk({}, 1, 'stop')]
                frames += [b'data: ' + encode({'id': 'chat_fixture_1', 'object': 'chat.completion.chunk', 'created': 0, 'model': 'synthetic-model', 'choices': [], 'usage': usage}) + b'\n\n', b'data: [DONE]\n\n']
                raw = b''.join(frames)
        else:
            value = {'id': 'synthetic', 'object': 'response', 'status': 'completed', 'output': [], 'usage': usage}
            if streaming:
                raw = conformance.event('response.completed', response=value)
                if body.get('input') == 'synthetic-malformed-observation':
                    raw = b'data: not-json\n\n' + raw
        if not streaming:
            raw = encode(value)
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream' if streaming else 'application/json')
        self.send_header('Content-Length', str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)


class Collector(BaseHTTPRequestHandler):
    events = {}
    fail = True

    def log_message(self, *_):
        pass

    def do_POST(self):
        assert self.headers.get('Authorization') == 'Bearer synthetic-export-token'
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        assert body['schema'] == 'gateway-usage-batch/v1'
        receipts = []
        for event in body['events']:
            key = (event['producer_id'], event['event_id'])
            sha = hashlib.sha256(encode(event)).hexdigest()
            previous = self.events.get(key)
            self.events[key] = sha
            receipts.append({'producer_id': key[0], 'event_id': key[1], 'sha256': sha,
                             'status': 'committed' if previous is None else 'duplicate' if previous == sha else 'conflict'})
        # Simulate committed remote events whose ACK was lost/unusable.
        self.send_response(503 if self.fail else 200)
        self.end_headers()
        if not self.fail:
            self.wfile.write(encode({'schema': 'gateway-usage-batch-receipt/v1', 'receipts': receipts}))


def run(binary, recorder):
    parent = ROOT / '.local/usage-smoke'
    parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=parent) as temporary, contextlib.ExitStack() as cleanup:
        root = Path(temporary).resolve()
        root.chmod(0o700)
        servers = []
        for handler in (Upstream, Collector):
            server = ThreadingHTTPServer(('127.0.0.1', 0), handler)
            threading.Thread(target=server.serve_forever, daemon=True).start()
            cleanup.callback(server.server_close)
            cleanup.callback(server.shutdown)
            servers.append(server)
        upstream, collector = servers
        extension_store = manager.open_store(root / 'extensions')
        usage_parent = manager.private_dir(extension_store / 'usage', create=True)
        ledger = manager.private_dir(usage_parent / 'primary', create=True)
        secret = ledger / 'export-token'
        manager.write_new(secret, b'synthetic-export-token')
        configuration = {'schema': 'gateway-usage-recorder-config/v1', 'destinations': [{'kind': 'http', 'id': 'collector', 'url': f'http://127.0.0.1:{collector.server_port}/events', 'bearer_file': str(secret)}]}
        raw = encode(configuration)
        manager.write_new(ledger / 'recorder.json', raw)
        subprocess.run([str(recorder), 'init', '--store', str(ledger)], check=True, capture_output=True)
        binding = {'store_id': 'primary', 'mode': 'durable_local', 'queue_capacity': 256, 'ack_timeout_ms': 5000, 'config_sha256': hashlib.sha256(raw).hexdigest()}
        package = root / 'package'
        sha = manager.package_binary(recorder, ROOT / 'LICENSE', package, 'usage-recorder', '0.1.0', 'usage_recorder')
        manager.install(extension_store, package, sha)
        manager.enable(extension_store, 'usage-recorder', '0.1.0', sha, manager.RECORDER_PERMISSIONS, binding)
        text = f'[providers.mock]\nbase_url="http://127.0.0.1:{upstream.server_port}/v1"\napi_key_env="SYNTHETIC_KEY"\n'
        for api in ('responses', 'messages', 'chat_completions'):
            text += f'[models.{api}]\nprovider="mock"\nupstream_model="synthetic-model"\napi="{api}"\n'
            if api != 'responses':
                text += f'capability_profile="{api}"\n'
            if api == 'chat_completions':
                text += 'auth="bearer"\n'
            if api == 'messages':
                text += 'auth="api_key"\nmessages_version="2023-06-01"\n'
        for api in ('messages', 'chat_completions'):
            text += f'[capability_profiles.{api}]\nversion="1"\nprovider="mock"\nupstream_model="synthetic-model"\napi="{api}"\ncontext_window=4096\nmax_output_tokens=128\ntested_codex_version="0.154.0"\n'
        config_file = root / 'gateway.toml'
        config_file.write_text(text)
        env = {**os.environ, 'ARG_LOCAL_TOKEN': 'L' * 40, 'SYNTHETIC_KEY': 'synthetic-upstream-key'}
        args = [str(binary), 'serve', '--config', str(config_file), '--extensions-lock', str(extension_store / 'active.json')]
        process = subprocess.Popen(args, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        cleanup.callback(terminate, process)
        try:
            ready = read_ready(process)
        except Exception:
            if process.poll() is not None:
                print(process.stderr.read().decode(), file=sys.stderr)
            raise
        assert ready['schema'] == 'gateway-extended-ready/v2'
        client = build_opener(ProxyHandler({}))
        for api in ('responses', 'messages', 'chat_completions'):
            for streaming in (False, True):
                request = Request(ready['base_url'] + '/responses', data=encode({'model': api, 'input': 'synthetic-input', 'stream': streaming}), headers={'Authorization': 'Bearer ' + 'L' * 40, 'Content-Type': 'application/json'})
                with client.open(request, timeout=10) as response:
                    assert response.status == 200
                    raw = response.read()
                    assert b'input_tokens' in raw
        request = Request(ready['base_url'] + '/responses', data=encode({'model': 'responses', 'input': 'synthetic-malformed-observation', 'stream': True}), headers={'Authorization': 'Bearer ' + 'L' * 40, 'Content-Type': 'application/json'})
        with client.open(request, timeout=10) as response:
            assert response.read().startswith(b'data: not-json\n\n')
        with sqlite3.connect(ledger / 'usage.sqlite3') as db:
            rows = [json.loads(r[0]) for r in db.execute('SELECT payload FROM usage_current')]
            assert len(rows) == 7 and all(r['kind'] == 'attempt_finished' for r in rows)
            assert sum(r['observation_incomplete'] for r in rows) == 1
            for row in rows:
                counters = row['usage']['counters']
                assert counters['input_tokens']['value'] == 17
                assert counters['output_tokens']['value'] == 3
                assert counters['cache_read_input_tokens']['value'] == 5
                assert counters['cache_write_input_tokens']['value'] == (None if row['profile'] == 'chat_v1' else 2)
            producer = rows[0]['producer_id']
        wait_for(lambda: bool(Collector.events))
        Collector.fail = False
        with sqlite3.connect(ledger / 'usage.sqlite3') as db:
            db.execute('UPDATE usage_outbox SET next_at_ms=0')
        def delivered():
            with sqlite3.connect(ledger / 'usage.sqlite3') as db:
                return db.execute("SELECT COUNT(*) FROM usage_outbox WHERE state!='committed'").fetchone()[0] == 0
        wait_for(delivered, seconds=10)
        terminate(process)
        # Package version and producer/store identity evolve independently.
        package2 = root / 'package2'
        sha2 = manager.package_binary(recorder, ROOT / 'LICENSE', package2, 'usage-recorder', '0.1.1', 'usage_recorder')
        manager.install(extension_store, package2, sha2)
        manager.enable(extension_store, 'usage-recorder', '0.1.1', sha2, manager.RECORDER_PERMISSIONS, binding)
        process2 = subprocess.Popen(args, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        cleanup.callback(terminate, process2)
        read_ready(process2)
        with sqlite3.connect(ledger / 'usage.sqlite3') as db:
            assert db.execute("SELECT value FROM metadata WHERE key='producer_id'").fetchone()[0] == producer
            assert db.execute('SELECT COUNT(*) FROM usage_current').fetchone()[0] == 7
        terminate(process2)
        report = subprocess.run([str(recorder), 'aggregate', '--store', str(ledger), '--timezone', 'Asia/Seoul'], check=True, capture_output=True)
        assert sum(g['calls'] for g in json.loads(report.stdout)['groups']) == 7
        # A slow best-effort recorder must not extend shutdown by every queued ACK.
        fixture = root / 'slow-recorder'
        fixture.write_text('#!' + sys.executable + '\n' + "import sys,json,time,hashlib\nprint(json.dumps({'type':'ready','protocol':'gateway-usage-recorder/v1','producer_id':'fixture'}),flush=True)\nfor line in sys.stdin:\n raw=line.rstrip('\\n').encode(); event=json.loads(raw); time.sleep(0.1)\n print(json.dumps({'type':'committed','event_id':event['event_id'],'sha256':hashlib.sha256(raw).hexdigest()}),flush=True)\n")
        fixture.chmod(0o500)
        slow_package = root / 'slow-package'
        slow_sha = manager.package_binary(fixture, ROOT / 'LICENSE', slow_package, 'usage-recorder', '0.1.2', 'usage_recorder')
        manager.install(extension_store, slow_package, slow_sha)
        slow_binding = {**binding, 'mode': 'best_effort'}
        manager.enable(extension_store, 'usage-recorder', '0.1.2', slow_sha, manager.RECORDER_PERMISSIONS, slow_binding)
        slow = subprocess.Popen(args, env=env, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        cleanup.callback(terminate, slow)
        slow_ready = read_ready(slow)
        for _ in range(40):
            request = Request(slow_ready['base_url'] + '/responses', data=encode({'model':'responses','input':'synthetic-input'}), headers={'Authorization':'Bearer ' + 'L' * 40,'Content-Type':'application/json'})
            with client.open(request, timeout=5) as response:
                response.read()
        terminate(slow)
    print('usage-smoke: PASS; three APIs, JSON/SSE, incomplete observation, durable IPC, HTTP ACK replay, upgrade, CLI')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--recorder', type=Path, required=True)
    args = parser.parse_args()
    run(args.binary.resolve(), args.recorder.resolve())
