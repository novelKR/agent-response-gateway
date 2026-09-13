#!/usr/bin/env python3
"""Synthetic host-only API and explicit stdin shutdown; no model/provider or deployment."""
import argparse
import json
import os
from pathlib import Path
import queue
import subprocess
import tempfile
import threading
from urllib.error import HTTPError
from urllib.request import Request, ProxyHandler, build_opener

ROOT = Path(__file__).resolve().parents[1]
TOKEN = 'synthetic-embedded-read-key-01234567890123456789'


def verify(binary):
    parent = ROOT / '.local' / 'embedded-host-smoke'
    parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=parent) as directory:
        path = Path(directory).resolve()
        path.chmod(0o700)
        environment = {}
        if os.name == 'nt':
            environment['SYSTEMROOT'] = os.environ['SYSTEMROOT']
        process = subprocess.Popen([str(binary), str(path)], stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                   env=environment)
        messages = queue.Queue()
        reader = threading.Thread(target=lambda: messages.put(process.stdout.readline()), daemon=True)
        reader.start()
        try:
            line = messages.get(timeout=10).decode('utf-8').strip()
            prefix = 'Synthetic host API: '
            assert line.startswith(prefix)
            base = line[len(prefix):]
            assert base.startswith('http://127.0.0.1:') and base.endswith('/management/v1')
            client = build_opener(ProxyHandler({}))

            def call(endpoint, token=TOKEN, data=None):
                headers = {'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json'}
                request = Request(base + endpoint, headers=headers,
                                  data=None if data is None else json.dumps(data).encode())
                try:
                    with client.open(request, timeout=5) as response:
                        return response.status, json.load(response)
                except HTTPError as failure:
                    return failure.code, json.load(failure)

            assert call('/capabilities?target=gateway', 'untrusted-role:admin')[0] == 401
            code, result = call('/capabilities?target=gateway')
            assert code == 200 and result['schema'] == 'gateway-management-http/v1'
            assert set(result['data']['supported_operations']) == {'read_state', 'read_operations'}
            assert 'runtime_start' not in result['data']['allowed_operations']
            code, result = call('/state?target=gateway')
            assert code == 200
            assert result['data']['modules'][0]['observation']['data']['lifecycle_owner'] == 'host'
            assert call('/preflight', data={'schema': 'gateway-management-http/v1',
                                           'target': 'gateway', 'idempotency_key': 'forbidden',
                                           'command': {'kind': 'runtime_start'}})[0] == 403
            process.stdin.close()
            assert process.wait(timeout=5) == 0
            assert not (path / 'team.sqlite3').exists()
            assert not (path / 'team-requests.sqlite3').exists()
            print('Synthetic embedded host: scoped reads, denied lifecycle and bounded stdin shutdown passed')
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)
            process.stdout.close()
            reader.join(timeout=1)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    args = parser.parse_args()
    verify(args.binary.resolve())
