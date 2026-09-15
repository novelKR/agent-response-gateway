"""Independent Recorder v2 packaging and actual newline IPC; synthetic data only."""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import platform
import shutil
import socket
import sqlite3
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
EXAMPLE = ROOT / 'tools/plugin-conformance/examples/recorder'


class RecorderExampleTests(unittest.TestCase):
    def setUp(self):
        parent = ROOT / '.local/recorder-example-tests'
        parent.mkdir(parents=True, exist_ok=True)
        self.tmp = tempfile.TemporaryDirectory(dir=parent)
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name).resolve()
        self.root.chmod(0o700)
        project = self.root / 'copied'
        shutil.copytree(EXAMPLE, project)
        self.package = self.root / 'package'
        target = ('macos' if sys.platform == 'darwin' else 'linux') + '-' + ('arm64' if platform.machine().lower() in ('aarch64', 'arm64') else 'x64')
        built = subprocess.run([sys.executable, '-B', str(project / 'package.py'), '--output', str(self.package), '--target', target],
                               check=True, capture_output=True, cwd=self.root)
        self.digest = built.stdout.decode().strip()
        initialized = subprocess.run([str(self.package / 'extension'), 'init'], check=True, capture_output=True, cwd=self.root, env={})
        self.producer = initialized.stdout.decode().strip()
        self.start()

    def start(self):
        self.socket, child = socket.socketpair()
        self.socket.settimeout(3)
        self.process = subprocess.Popen([str(self.package / 'extension'), 'serve-v2'], stdin=child, stdout=child,
                                        stderr=subprocess.DEVNULL, cwd=self.root, env={})
        child.close()
        self.addCleanup(self.stop)
        self.ready = self.read()
        self.assertEqual(self.ready['protocol'], 'gateway-usage-recorder/v2')
        self.assertEqual(self.ready['producer_id'], self.producer)

    def stop(self):
        if self.process.poll() is None:
            self.process.kill()
        self.process.wait(timeout=3)
        self.socket.close()

    def read(self):
        raw = b''
        while not raw.endswith(b'\n'):
            part = self.socket.recv(1)
            self.assertTrue(part)
            raw += part
            self.assertLessEqual(len(raw), 4096)
        return json.loads(raw)

    def event(self, version):
        value = json.loads((ROOT / 'schemas/plugin-vectors.json').read_text())['canonical_usage_event']['value']
        value.update(producer_id=self.producer, attempt_id=f'attempt-{version}', event_id=f'event-{version}')
        if version == 2:
            value['schema'] = 'gateway-usage-event/v2'
            del value['profile']
            value['interpretation'] = {'kind': 'trusted_provider_plugin', 'protocol': 'gateway-provider/v1',
                'provider_protocol': 'synthetic-provider/v1', 'package_id': 'synthetic-provider', 'package_version': '1.0.0',
                'package_sha256': 'a'*64, 'executable_sha256': 'b'*64}
            value['usage']['reported'] = {}
            value['usage']['cache_write_details'] = []
        return value

    def send(self, value):
        raw = json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode()
        self.socket.sendall(raw+b'\n')
        expected = {'type': 'committed', 'event_id': value['event_id'], 'sha256': hashlib.sha256(raw).hexdigest()}
        self.assertEqual(self.read(), expected)
        return raw

    def test_independent_package_and_schema_copies(self):
        package = json.loads((self.package / 'extension.json').read_bytes())
        self.assertEqual(package['capabilities'], self.ready['capabilities'])
        for version in (1, 2):
            self.assertEqual((EXAMPLE / f'event-v{version}.schema.json').read_bytes(),
                             (ROOT / f'schemas/gateway-usage-event-v{version}.schema.json').read_bytes())
        checker = self.root / 'check.py'
        shutil.copyfile(ROOT / 'tools/plugin-conformance/conformance.py', checker)
        checked = subprocess.run([sys.executable, '-B', str(checker), '--package', str(self.package), '--expected-sha256', self.digest],
                                 check=True, capture_output=True, cwd=self.root)
        self.assertEqual(json.loads(checked.stdout)['checks'][0]['status'], 'pass')

    def test_public_usage_vectors_and_original_v1_hash(self):
        source = self.package / 'recorder.py'
        spec = importlib.util.spec_from_file_location('copied_recorder', source)
        recorder = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(recorder)
        vectors = json.loads((ROOT / 'schemas/plugin-usage-vectors.json').read_text())
        original = json.loads((ROOT / 'schemas/plugin-vectors.json').read_text())['canonical_usage_event']
        self.assertEqual(vectors['canonical_v1'], original)
        for version in ('canonical_v1', 'canonical_v2'):
            item = vectors[version]
            self.assertEqual(recorder.encode(item['value']), item['utf8'].encode())
            self.assertEqual(hashlib.sha256(item['utf8'].encode()).hexdigest(), item['sha256'])
            recorder.validate(item['utf8'].encode())
        for case in vectors['cases']:
            with self.subTest(case=case['id']):
                raw = recorder.encode(case['value'])
                if case['valid']:
                    recorder.validate(raw)
                else:
                    with self.assertRaises(ValueError):
                        recorder.validate(raw)

    def test_v1_v2_exact_bytes_dedup_and_restart(self):
        expected = [self.send(self.event(version)) for version in (1, 2)]
        self.send(self.event(1))
        self.stop()
        self.start()
        self.send(self.event(2))
        with sqlite3.connect(self.root / 'events.sqlite3') as db:
            rows = db.execute('SELECT payload,sha256 FROM events ORDER BY event_id').fetchall()
        self.assertEqual([row[0] for row in rows], expected)
        self.assertEqual([row[1] for row in rows], [hashlib.sha256(raw).hexdigest() for raw in expected])

    def test_invalid_evidence_preserves_unknown_and_zero(self):
        value = self.event(2)
        value["request_id"] = "synthetic-request-1"
        counters = value['usage']['counters']
        counters['input_tokens'] = {'source': 'invalid', 'value': None}
        counters['output_tokens'] = {'source': 'reported', 'value': 0}
        counters['total_tokens'] = {'source': 'not_reported', 'value': None}
        value['usage']['violations'] = ['invalid_counter']
        raw = self.send(value)
        with sqlite3.connect(self.root / 'events.sqlite3') as db:
            payload, checksum = db.execute('SELECT payload,sha256 FROM events WHERE event_id=?',
                                           (value['event_id'],)).fetchone()
        self.assertEqual(payload, raw)
        self.assertEqual(checksum, hashlib.sha256(payload).hexdigest())
        stored = json.loads(payload)
        self.assertEqual(stored['request_id'], 'synthetic-request-1')
        self.assertEqual(stored['usage']['counters'], counters)
        self.assertEqual(stored['usage']['violations'], ['invalid_counter'])

    def test_malformed_negative_value_is_not_recorded(self):
        value = self.event(2)
        value['usage']['counters']['input_tokens'] = {'source': 'reported', 'value': -1}
        self.socket.sendall(json.dumps(value, sort_keys=True, separators=(',', ':')).encode()+b'\n')
        self.assertNotEqual(self.process.wait(timeout=3), 0)
        with sqlite3.connect(self.root / 'events.sqlite3') as db:
            self.assertEqual(db.execute('SELECT COUNT(*) FROM events').fetchone()[0], 0)

    def test_interpretation_conflict_is_rejected_without_rewrite(self):
        value = self.event(2)
        original = self.send(value)
        changed = copy.deepcopy(value)
        changed['interpretation']['package_sha256'] = 'c'*64
        self.socket.sendall(json.dumps(changed, sort_keys=True, separators=(',', ':')).encode()+b'\n')
        self.assertNotEqual(self.process.wait(timeout=3), 0)
        with sqlite3.connect(self.root / 'events.sqlite3') as db:
            self.assertEqual(db.execute('SELECT payload FROM events').fetchone()[0], original)

    def test_truncated_frame_and_legacy_command_fail(self):
        self.socket.sendall(b'{')
        self.socket.shutdown(socket.SHUT_WR)
        self.assertNotEqual(self.process.wait(timeout=3), 0)
        legacy = subprocess.run([str(self.package / 'extension'), 'serve'], env={}, cwd=self.root, capture_output=True)
        self.assertNotEqual(legacy.returncode, 0)
        self.assertEqual(legacy.stdout, b'')


if __name__ == '__main__':
    unittest.main()
