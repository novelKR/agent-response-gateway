"""Independent framed provider example; synthetic data only, no host runtime claim."""
import base64
import hashlib
import json
from pathlib import Path
import shutil
import platform
import socket
import struct
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
EXAMPLE = ROOT / 'tools/plugin-conformance/examples/provider'


class ProviderExampleTests(unittest.TestCase):
    def setUp(self):
        parent = ROOT / '.local/provider-example-tests'
        parent.mkdir(parents=True, exist_ok=True)
        self.tmp = tempfile.TemporaryDirectory(dir=parent)
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name).resolve()
        project = self.root / 'external'
        shutil.copytree(EXAMPLE, project)
        self.package = self.root / 'package'
        subprocess.run([sys.executable, '-B', str(project / 'package.py'), '--output', str(self.package),
                        '--target', ('macos' if sys.platform == 'darwin' else 'linux') + '-' +
                        ('arm64' if platform.machine().lower() in ('arm64', 'aarch64') else 'x64')], check=True, capture_output=True, cwd=self.root)
        self.frames = []
        self.start()

    def start(self):
        self.socket, child = socket.socketpair()
        self.socket.settimeout(3)
        self.process = subprocess.Popen([str(self.package / 'extension')], stdin=child, stdout=child,
                                        stderr=subprocess.DEVNULL, env={}, cwd=self.root)
        child.close()
        self.addCleanup(self.stop)
        self.sequence = 0
        self.ready = self.read()
        self.assertEqual(self.ready['sequence'], 0)

    def stop(self):
        if self.process.poll() is None:
            self.process.kill()
        self.process.wait(timeout=3)
        self.socket.close()

    def exact(self, size):
        data = b''
        while len(data) < size:
            part = self.socket.recv(size - len(data))
            self.assertTrue(part)
            data += part
        return data

    def read(self):
        size = struct.unpack('>I', self.exact(4))[0]
        self.assertLessEqual(size, 1024 * 1024)
        value = json.loads(self.exact(size))
        self.frames.append(value)
        return value

    def call(self, operation):
        self.sequence += 1
        message = {'protocol': 'gateway-provider/v1', 'sequence': self.sequence, 'operation': operation}
        self.frames.append(message)
        raw = json.dumps(message).encode()
        self.socket.sendall(struct.pack('>I', len(raw)) + raw)
        response = self.read()
        self.assertEqual(response['sequence'], self.sequence)
        return response['value']

    def prepare(self, continuation=None, request=None, maximum=65536):
        return self.call({'operation': 'prepare', 'value': {
            'request': request or {'model': 'synthetic-model', 'input': 'hello'},
            'route': {'provider_protocol': 'synthetic-provider/v1', 'model': 'synthetic-model',
                      'profile_id': 'synthetic', 'profile_version': '1', 'support': {}, 'editing': {'kind': 'none'}},
            'continuation': continuation or {'mode': 'stateless'},
            'max_request_bytes': maximum, 'max_output_bytes': maximum}})

    def complete(self, native):
        return self.call({'operation': 'json', 'body': json.dumps(native), 'response_id': 'response_1'})['value']

    def test_independent_package_and_unknown_vendor_shape(self):
        manifest = json.loads((self.package / 'extension.json').read_bytes())
        self.assertEqual(manifest['capabilities'], self.ready['value']['capabilities'])
        checker = self.root / 'standalone-check.py'
        shutil.copyfile(ROOT / 'tools/plugin-conformance/conformance.py', checker)
        checked = subprocess.run([sys.executable, '-B', str(checker), '--package', str(self.package),
                                  '--expected-sha256', hashlib.sha256((self.package / 'extension.json').read_bytes()).hexdigest()],
                                 check=True, capture_output=True, cwd=self.root)
        report = json.loads(checked.stdout)
        self.assertEqual(report['checks'][0]['status'], 'pass')
        self.assertEqual(report['checks'][-1]['status'], 'not-run')
        payload = self.prepare()['payload']
        self.assertNotIn('model', payload)
        self.assertEqual(payload['query']['input'], 'hello')
        result = self.complete({'answer': [{'text': 'hello'}]})
        self.assertEqual(result['response']['model'], 'synthetic-model')
        self.assertEqual(result['usage'], {'kind': 'unobserved'})
        self.assertEqual(result['state'], {'kind': 'none'})
        self.assertEqual(self.process.wait(timeout=3), 0)

    def test_zero_unknown_and_large_exact_counter(self):
        self.prepare()
        result = self.complete({'answer': [], 'meter': {'input_tokens': 2**53+1, 'output_tokens': 0}})
        counters = result['usage']['counters']
        self.assertEqual(counters['input_tokens'], {'source': 'reported', 'value': 2**53+1})
        self.assertEqual(counters['output_tokens'], {'source': 'reported', 'value': 0})
        self.assertEqual(counters['total_tokens'], {'source': 'not_reported'})
        self.assertEqual(result['response']['usage']['total_tokens'], 2**53+1)

    def test_invalid_numbers_are_not_zero(self):
        self.prepare()
        result = self.complete({'answer': [], 'meter': {'input_tokens': -1, 'output_tokens': 1.5,
                                                      'total_tokens': 2**64, 'input_regular_tokens': True}})
        for name in ('input_tokens', 'output_tokens', 'total_tokens', 'input_regular_tokens'):
            self.assertEqual(result['usage']['counters'][name], {'source': 'invalid'})

    def test_sse_progress_and_semantic_completion(self):
        self.prepare()
        self.call({'operation': 'stream', 'response_id': 'response_1'})
        for piece in ['hel', 'lo']:
            progress = self.call({'operation': 'event', 'event': 'piece', 'data': json.dumps({'text': piece})})
            self.assertFalse(progress['complete'])
            self.assertEqual(progress['events'][-1]['delta'], piece)
        end = self.call({'operation': 'event', 'event': 'end', 'data': json.dumps({'answer': [{'text': 'hello'}]})})
        self.assertTrue(end['complete'])
        result = self.call({'operation': 'finish'})
        self.assertEqual(result['value']['response']['output'][0]['content'][0]['text'], 'hello')

    def test_function_result_roundtrip_and_opaque_process_restart(self):
        self.prepare({'mode': 'managed', 'pending_tools': False, 'history': []})
        first = self.complete({'answer': [{'call': 'call_1', 'name': 'lookup', 'arguments': '{}'}]})
        self.assertEqual(first['outcome'], 'awaiting_tools')
        state = first['state']['value']
        self.assertEqual(json.loads(base64.b64decode(state['data_base64'])), {'counter': 1})
        self.stop()
        self.start()
        tool_result = {'type': 'function_call_output', 'call_id': 'call_1', 'output': 'synthetic-result'}
        payload = self.prepare({'mode': 'managed', 'pending_tools': True, 'history': [{'start': 0, 'end': 1, 'state': state}]},
                               {'model': 'synthetic-model', 'input': [tool_result]})['payload']
        self.assertEqual(payload['query']['input'], [tool_result])
        self.assertEqual(payload['cursor'], 1)
        final = self.complete({'answer': [{'text': 'done'}]})
        self.assertEqual(json.loads(base64.b64decode(final['state']['value']['data_base64'])), {'counter': 2})

    def test_finish_before_terminal_rejected(self):
        self.prepare()
        self.call({'operation': 'stream', 'response_id': 'response_1'})
        self.assertEqual(self.call({'operation': 'finish'}), {'result': 'rejected', 'code': 'invalid_upstream'})
        self.assertNotEqual(self.process.wait(timeout=3), 0)

    def test_duplicate_fields_and_truncated_frame_fail(self):
        raw = b'{"protocol":"gateway-provider/v1","sequence":1,"sequence":2,"operation":{}}'
        self.socket.sendall(struct.pack('>I', len(raw)) + raw)
        self.assertEqual(self.read()['value']['result'], 'rejected')
        self.assertNotEqual(self.process.wait(timeout=3), 0)
        self.stop()
        self.start()
        self.socket.sendall(b'\x00\x00')
        self.socket.shutdown(socket.SHUT_WR)
        self.assertNotEqual(self.process.wait(timeout=3), 0)

    def test_unsupported_state_version_is_rejected(self):
        state = {'format': 'synthetic-counter', 'version': 2,
                 'data_base64': base64.b64encode(b'{"counter":1}').decode()}
        result = self.prepare({'mode': 'managed', 'pending_tools': False,
                               'history': [{'start': 0, 'end': 1, 'state': state}]})
        self.assertEqual(result, {'result': 'rejected', 'code': 'unsupported_state'})

    def test_declared_output_limit_is_enforced(self):
        self.prepare(maximum=300)
        result = self.call({'operation': 'json', 'response_id': 'response_1',
                            'body': json.dumps({'answer': [{'text': 'x' * 400}]})})
        self.assertEqual(result, {'result': 'rejected', 'code': 'resource_limit'})

    def test_stalled_input_is_killed_without_completion(self):
        self.prepare()
        self.socket.sendall(struct.pack('>I', 200) + b'{')
        self.assertIsNone(self.process.poll())
        self.stop()
        self.assertNotEqual(self.process.returncode, 0)


if __name__ == '__main__':
    unittest.main()
