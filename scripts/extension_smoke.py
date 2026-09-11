#!/usr/bin/env python3
"""Exercise the actual native gateway and observer using only synthetic loopback traffic."""
from __future__ import annotations

import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import selectors
import signal
import subprocess
import tempfile
import threading
import time
from urllib.error import HTTPError
from urllib.request import Request, urlopen

import extension_manager as manager


JSON_RESPONSE = b'{"id":"resp_synthetic","object":"response","status":"completed","output":[]}'
SSE_RESPONSE = b'data: {"type":"response.created","response":{"id":"resp_synthetic"}}\n\ndata: {"type":"response.completed","response":{"id":"resp_synthetic","status":"completed","output":[]}}\n\n'


class Upstream(BaseHTTPRequestHandler):
    calls = 0

    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        type(self).calls += 1
        raw = SSE_RESPONSE if request.get('stream') else JSON_RESPONSE
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream' if request.get('stream') else 'application/json')
        self.send_header('Content-Length', str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def log_message(self, *_):
        pass


def require(ok, message):
    if not ok:
        raise RuntimeError(message)


def wait_for(predicate, seconds=4):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.02)
    raise RuntimeError('Bounded extension observation timed out')


def terminate(child):
    if child.poll() is None:
        child.terminate()
        try:
            child.wait(timeout=6)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait(timeout=2)
            raise RuntimeError('Gateway did not stop within the test deadline')
    if child.stdout:
        child.stdout.close()


def read_ready(child):
    # Read the descriptor incrementally: buffered readline can outlive the readiness deadline.
    deadline = time.monotonic() + 6
    raw = bytearray()
    with selectors.DefaultSelector() as selector:
        selector.register(child.stdout, selectors.EVENT_READ)
        while not raw.endswith(b'\n'):
            remaining = deadline - time.monotonic()
            require(remaining > 0 and bool(selector.select(timeout=remaining)), 'Gateway readiness timed out')
            chunk = os.read(child.stdout.fileno(), min(4096, 65_537 - len(raw)))
            require(bool(chunk), 'Gateway exited before readiness')
            raw.extend(chunk)
            require(len(raw) <= 65_536, 'Invalid readiness frame')
        require(raw.count(b'\n') == 1, 'Unexpected additional readiness output')
        return json.loads(raw)


def read_counts(path):
    try:
        return json.loads(path.read_bytes())
    except FileNotFoundError:
        return {'observed': 0}


def request(base, path, *, payload=None, auth=False):
    headers = {'Authorization': 'Bearer ' + 'L' * 40} if auth else {}
    data = None if payload is None else json.dumps(payload).encode()
    if data is not None:
        headers['Content-Type'] = 'application/json'
    try:
        with urlopen(Request(base + path, data=data, headers=headers), timeout=4) as response:
            return response.status, response.read()
    except HTTPError as error:
        return error.code, error.read()


# Only fixed, source-defined phase labels enter failure reports; never exception messages.
PHASE = 'initialization'


def run(binary, observer):
    global PHASE
    manager.target()
    Upstream.calls = 0
    parent = Path(__file__).resolve().parents[1] / '.local'
    parent.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='extensions-smoke-', dir=parent) as temporary:
        root = Path(temporary).resolve()
        server = ThreadingHTTPServer(('127.0.0.1', 0), Upstream)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            store = root / 'store'
            package = root / 'package'
            # Project license is packaged as real evidence; no credential or private fixture is read.
            license_file = Path(__file__).resolve().parents[1] / 'LICENSE'
            PHASE = 'package-preparation'
            sha = manager.package_binary(observer, license_file, package, 'metadata-counter', '0.1.0')
            PHASE = 'installation-and-activation'
            manager.install(store, package, sha)
            manager.enable(store, 'metadata-counter', '0.1.0', sha, manager.PERMISSIONS)
            lock = store / 'active.json'
            counts = store / 'state' / 'metadata-counter' / sha / 'counts.json'
            config = root / 'gateway.toml'
            config.write_text('listen = "127.0.0.1:0"\nlocal_token_env = "ARG_LOCAL_TOKEN"\n'
                              f'[providers.mock]\nbase_url = "http://127.0.0.1:{server.server_port}"\n'
                              'api_key_env = "ARG_MOCK_KEY"\n[models.mock]\nprovider = "mock"\nupstream_model = "synthetic"\n')
            environment = {'ARG_LOCAL_TOKEN': 'L' * 40, 'ARG_MOCK_KEY': 'synthetic-upstream-key',
                           'UNRELATED_SECRET': 'synthetic-not-for-observers'}
            common = ['--config', str(config)]
            options = [*common, '--extensions-lock', str(lock)]

            def command(verb, args, expected=0):
                result = subprocess.run([str(binary), verb, *args], env=environment,
                                        capture_output=True, timeout=8, check=False)
                require(result.returncode == expected if expected == 0 else result.returncode != 0,
                        'Unexpected CLI outcome')
                return result

            PHASE = 'offline-inspection'
            plain = command('manifest', common).stdout
            extended = json.loads(command('manifest', options).stdout)
            require(extended['schema'] == 'gateway-extended-manifest/v1', 'Wrong extended manifest schema')
            checked = json.loads(command('check-config', options).stdout)
            require(checked['extensions_executed'] is False, 'Offline inspection executed code')
            require(not counts.exists(), 'Offline operations ran the observer')
            require(command('manifest', common).stdout == plain, 'Legacy manifest changed after installation')

            child = subprocess.Popen([str(binary), 'serve', *options], env=environment,
                                     stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
            PHASE = 'runtime-and-native-forwarding'
            observer_pid = None
            try:
                ready = read_ready(child)
                require(ready['schema'] == 'gateway-extended-ready/v1'
                        and ready['execution_sha256'] == extended['execution_sha256'], 'Readiness binding mismatch')
                base = ready['base_url'].removesuffix('/v1')
                require(request(base, '/healthz')[0] == 200, 'Health endpoint failed')
                require(request(base, '/v1/models')[0] == 401, 'Local authentication changed')
                require(request(base, '/v1/responses', payload={'model':'mock', 'input':'synthetic'}, auth=True)
                        == (200, JSON_RESPONSE), 'Native JSON forwarding changed')
                require(request(base, '/v1/responses', payload={'model':'mock', 'input':'synthetic', 'stream':True}, auth=True)
                        == (200, SSE_RESPONSE), 'Native SSE forwarding changed')
                require(Upstream.calls == 2, 'Unexpected inference attempt count')
                wait_for(lambda: read_counts(counts)['observed'] >= 4)
                observed = read_counts(counts)
                require(set(observed) == {'schema', 'process_id', 'observed', 'status_counts'}, 'Unexpected observer state fields')
                require(observed['status_counts']['401'] == 1, 'Observer did not receive numeric HTTP status')
                observer_pid = observed['process_id']
                PHASE = 'ownership-and-frozen-activation'
                command('serve', options, expected=1)  # One supervisor owns this store.
                manager.disable(store, 'metadata-counter')
                before = read_counts(counts)['observed']
                require(request(base, '/healthz')[0] == 200, 'Live snapshot was changed by disable')
                wait_for(lambda: read_counts(counts)['observed'] > before)
            finally:
                terminate(child)
            if observer_pid is not None:
                try:
                    os.kill(observer_pid, 0)
                except ProcessLookupError:
                    pass
                else:
                    raise RuntimeError('Direct observer process was not reaped')
            PHASE = 'disabled-runtime'
            before = counts.read_bytes()
            child = subprocess.Popen([str(binary), 'serve', *options], env=environment,
                                     stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
            try:
                ready = read_ready(child)
                require(request(ready['base_url'].removesuffix('/v1'), '/healthz')[0] == 200, 'Disabled gateway failed')
            finally:
                terminate(child)
            require(counts.read_bytes() == before, 'Disabled observer executed')
            manager.enable(store, 'metadata-counter', '0.1.0', sha, manager.PERMISSIONS)
            for fault in (signal.SIGTERM, signal.SIGSTOP):
                PHASE = 'observer-exit' if fault == signal.SIGTERM else 'observer-stall'
                old_count = read_counts(counts)['observed']
                child = subprocess.Popen([str(binary), 'serve', *options], env=environment,
                                         stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
                try:
                    ready = read_ready(child)
                    base = ready['base_url'].removesuffix('/v1')
                    require(request(base, '/healthz')[0] == 200, 'Observer restart failed')
                    wait_for(lambda: read_counts(counts)['observed'] > old_count)
                    fault_pid = read_counts(counts)['process_id']
                    os.kill(fault_pid, fault)
                    require(request(base, '/healthz')[0] == 200, 'Observer failure delayed HTTP')

                    def reaped():
                        try:
                            os.kill(fault_pid, 0)
                        except ProcessLookupError:
                            return True
                        return False

                    wait_for(reaped)
                    require(request(base, '/v1/models', auth=True)[0] == 200,
                            'Observer failure disabled ordinary routes')
                finally:
                    terminate(child)
            PHASE = 'tampered-package'
            executable = manager.installed_dir(store, 'metadata-counter', '0.1.0', sha) / 'extension'
            executable.chmod(0o700)
            executable.write_bytes(b'tampered-synthetic-executable')
            command('manifest', options, expected=1)
            command('serve', options, expected=1)
            require(Upstream.calls == 2, 'Invalid package reached inference transport')
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=2)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--observer', type=Path, required=True)
    args = parser.parse_args()
    try:
        run(args.binary.resolve(strict=True), args.observer.resolve(strict=True))
        print('Extension smoke passed: offline lifecycle, native forwarding, isolation and shutdown')
        return 0
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError, KeyError):
        print('Extension smoke failed in ' + PHASE + '; no fixture bodies or credentials are emitted')
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
