"""Black-box adversarial checks for the independently distributable runner."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[2]
TOOL = ROOT / 'tools/plugin-conformance/conformance.py'
spec = importlib.util.spec_from_file_location('plugin_conformance', TOOL)
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class ConformanceTests(unittest.TestCase):
    def setUp(self):
        local = ROOT / '.local'
        local.mkdir(exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix='conformance-test-', dir=local)
        self.root = Path(self.temporary.name).resolve()
        self.package = self.root / 'package'
        self.package.mkdir(mode=0o700)
        self.addCleanup(self.temporary.cleanup)

    def package_source(self, source, protocol='gateway-observer/v1', target=None):
        contents = ('#!' + str(Path(sys.executable).resolve()) + '\n' + source).encode()
        (self.package / 'extension').write_bytes(contents)
        (self.package / 'LICENSE.txt').write_text('Synthetic local test notice\n')
        permissions, state = runner.ROLES[protocol]
        value = {'schema': 'gateway-extension-package/v1', 'id': 'independent-observer', 'version': '0.1.0',
                 'target': target or runner.host_target(), 'protocol': protocol,
                 'permissions': permissions, 'state_schema': state,
                 'files': {name: runner.digest((self.package / name).read_bytes())
                           for name in ('extension', 'LICENSE.txt')}}
        return self.write_manifest(value)

    def write_manifest(self, value):
        raw = runner.canonical(value)
        (self.package / 'extension.json').write_bytes(raw)
        return runner.digest(raw)

    def run_package(self, source, **kwargs):
        checksum = self.package_source(source, **kwargs)
        return runner.run(self.package, checksum, execute=True, state_root=self.root)

    def assert_failure(self, report, code):
        self.assertEqual(report['status'], 'fail', report)
        self.assertEqual(report['checks'][-1]['code'], code, report)

    def test_external_project_build_and_execution_without_repo(self):
        copied = self.root / 'independent-project'
        shutil.copytree(ROOT / 'tools/plugin-conformance/examples/observer', copied)
        binary = self.root / 'observer'
        subprocess.run([sys.executable, '-B', str(copied / 'build.py'), '--output', str(binary)],
                       check=True, cwd=self.root, capture_output=True)
        checksum = self.package_source('')
        (self.package / 'extension').write_bytes(binary.read_bytes())
        manifest = json.loads((self.package / 'extension.json').read_bytes())
        manifest['files']['extension'] = runner.digest(binary.read_bytes())
        checksum = self.write_manifest(manifest)
        standalone = self.root / 'check.py'
        shutil.copyfile(TOOL, standalone)
        completed = subprocess.run([sys.executable, '-B', str(standalone), '--package', str(self.package),
                                    '--expected-sha256', checksum, '--execute', '--state-root', str(self.root)],
                                   cwd=self.root, capture_output=True, check=True)
        report = json.loads(completed.stdout)
        self.assertEqual(report['status'], 'pass')
        self.assertEqual(report['package_sha256'], checksum)
        self.assertEqual(report['contract'], 'gateway-observer/v1')
        self.assertEqual(report['tool_version'], '1.1.0')
        self.assertEqual(report['checks'][-1]['id'], 'role.execution')
        self.assertFalse(list(self.root.glob('observer-*')))

    def test_declared_sibling_resource_is_available(self):
        source = ("from pathlib import Path\n"
                  "assert Path(__file__).with_name('settings.json').read_text() == 'synthetic'\n"
                  "assert Path(__file__).with_name('extension.json').is_file()\n"
                  + (ROOT / 'tools/plugin-conformance/examples/observer/observer.py').read_text())
        self.package_source(source)
        (self.package / 'settings.json').write_text('synthetic')
        manifest = json.loads((self.package / 'extension.json').read_bytes())
        manifest['files']['settings.json'] = runner.digest(b'synthetic')
        checksum = self.write_manifest(manifest)
        self.assertEqual(runner.run(self.package, checksum, True, self.root)['status'], 'pass')

    def test_static_inspection_never_executes(self):
        marker = self.root / 'must-not-exist'
        checksum = self.package_source(f'open({str(marker)!r}, "w").close()\n')
        report = runner.run(self.package, checksum)
        self.assertEqual(report['status'], 'not-run')
        self.assertEqual(report['checks'][0]['status'], 'pass')
        self.assertFalse(marker.exists())

    def test_unknown_role_runner_is_not_a_pass(self):
        checksum = self.package_source('raise SystemExit(42)', protocol='gateway-api-codec/v2')
        report = runner.run(self.package, checksum, True, self.root)
        self.assertEqual(report['status'], 'not-run')
        self.assertEqual(report['checks'][-1]['code'], 'role_runner_unavailable')

    def test_cross_target_static_acceptance_execution_rejection(self):
        target = next(value for value in sorted(runner.TARGETS) if value != runner.host_target())
        checksum = self.package_source('raise SystemExit(42)', target=target)
        self.assertEqual(runner.run(self.package, checksum)['checks'][0]['status'], 'pass')
        self.assert_failure(runner.run(self.package, checksum, True, self.root), 'host_target_mismatch')

    def test_wrong_digest_inventory_and_link_rejected(self):
        checksum = self.package_source('')
        self.assert_failure(runner.run(self.package, '0' * 64), 'manifest_digest')
        (self.package / 'unlisted').write_text('x')
        self.assert_failure(runner.run(self.package, checksum), 'file_inventory')
        (self.package / 'unlisted').unlink()
        (self.package / 'extension').unlink()
        (self.package / 'extension').symlink_to(self.package / 'LICENSE.txt')
        self.assert_failure(runner.run(self.package, checksum), 'linked_path')

    def test_noncanonical_duplicate_and_wrong_type_manifest(self):
        checksum = self.package_source('')
        value = json.loads((self.package / 'extension.json').read_bytes())
        raw = json.dumps(value).encode()
        (self.package / 'extension.json').write_bytes(raw)
        self.assert_failure(runner.run(self.package, runner.digest(raw)), 'noncanonical_manifest')
        value['protocol'] = []
        self.assert_failure(runner.run(self.package, self.write_manifest(value)), 'unsupported_role')
        raw = b'{"schema":1,"schema":2}'
        (self.package / 'extension.json').write_bytes(raw)
        self.assert_failure(runner.run(self.package, runner.digest(raw)), 'duplicate_field')

    def test_invalid_ready_and_unknown_fields(self):
        for ready in ({'type': 'ready', 'protocol': 'other'},
                      {'type': 'ready', 'protocol': 'gateway-observer/v1', 'callback': True}):
            with self.subTest(ready=ready):
                self.assert_failure(self.run_package(f'print({json.dumps(ready)!r}, flush=True)'), 'invalid_ready')

    def test_wrong_ack_bool_sequence_and_duplicate_key(self):
        ready = 'print(\'{"type":"ready","protocol":"gateway-observer/v1"}\', flush=True)\n'
        for ack, code in [(' {"type":"ack","sequence":2}', 'invalid_ack'),
                          ('{"type":"ack","sequence":true}', 'invalid_ack'),
                          ('{"type":"ack","sequence":1,"sequence":1}', 'duplicate_field')]:
            with self.subTest(ack=ack):
                self.assert_failure(self.run_package(ready + f'input()\nprint({ack!r}, flush=True)\n'), code)

    def test_partial_frame_timeout_reaps_child(self):
        started = time.monotonic()
        report = self.run_package('import os,time\nos.write(1,b"{")\ntime.sleep(20)\n')
        self.assert_failure(report, 'deadline')
        self.assertLess(time.monotonic() - started, 5)
        self.assertFalse(list(self.root.glob('observer-*')))

    def test_ack_stall_early_exit_and_oversized_frame(self):
        ready = 'print(\'{"type":"ready","protocol":"gateway-observer/v1"}\', flush=True)\n'
        self.assert_failure(self.run_package(ready + 'import time\ntime.sleep(20)'), 'deadline')
        self.assert_failure(self.run_package('raise SystemExit(0)'), 'unexpected_eof')
        self.assert_failure(self.run_package('import os\nos.write(1,b"x"*4097)'), 'frame_limit')

    def test_empty_environment_and_no_output_leakage(self):
        source = ('import os\nassert not os.environ.get("CONFORMANCE_SECRET")\n'
                  'print("sensitive synthetic text", flush=True)\n')
        previous = os.environ.get('CONFORMANCE_SECRET')
        os.environ['CONFORMANCE_SECRET'] = 'must-not-inherit'
        try:
            report = self.run_package(source)
        finally:
            if previous is None:
                del os.environ['CONFORMANCE_SECRET']
            else:
                os.environ['CONFORMANCE_SECRET'] = previous
        self.assert_failure(report, 'invalid_json')
        serialized = json.dumps(report)
        self.assertNotIn('sensitive synthetic text', serialized)
        self.assertNotIn(str(self.root), serialized)

    def test_capability_package_vectors_match_standalone_inspection(self):
        corpus = json.loads((ROOT / 'schemas/plugin-capabilities-vectors.json').read_text())
        legacy = json.loads((ROOT / 'schemas/plugin-vectors.json').read_text())['canonical_package']
        for name, data in legacy['files_utf8'].items():
            (self.package / name).write_text(data)
        for case in corpus['cases']:
            if not case['schema'].startswith('gateway-extension-package-'):
                continue
            with self.subTest(case=case['id']):
                checksum = self.write_manifest(case['value'])
                report = runner.run(self.package, checksum)
                self.assertEqual(report['checks'][0]['status'] == 'pass', case['valid'], report)

    def test_provider_declaration_execution_is_explicitly_unavailable(self):
        corpus = json.loads((ROOT / 'schemas/plugin-capabilities-vectors.json').read_text())
        manifest = next(case['value'] for case in corpus['cases'] if case['id'] == 'provider-declaration-only')
        legacy = json.loads((ROOT / 'schemas/plugin-vectors.json').read_text())['canonical_package']
        for name, data in legacy['files_utf8'].items():
            (self.package / name).write_text(data)
        checksum = self.write_manifest(manifest)
        report = runner.run(self.package, checksum, True, self.root)
        self.assertEqual(report['status'], 'not-run')
        self.assertEqual(report['checks'][-1]['code'], 'provider_runtime_unavailable')
        self.assertFalse(list(self.root.glob('observer-*')))

    def test_legacy_capability_and_new_protocol_reinterpretation_rejected(self):
        checksum = self.package_source('')
        manifest = json.loads((self.package / 'extension.json').read_bytes())
        manifest['capabilities'] = None
        self.assert_failure(runner.run(self.package, self.write_manifest(manifest)), 'manifest_fields')
        del manifest['capabilities']
        manifest['protocol'] = 'gateway-api-codec/v3'
        self.assert_failure(runner.run(self.package, self.write_manifest(manifest)), 'unsupported_role')

    def test_capability_schema_references_resolve_inside_distribution(self):
        from urllib.parse import unquote, urlsplit
        schema_root = (ROOT / 'schemas').resolve()
        visited = set()

        def load(path):
            self.assertTrue(path.is_relative_to(schema_root))
            value = json.loads(path.read_text())
            if path not in visited:
                visited.add(path)
                self.assertEqual(value['$schema'], 'https://json-schema.org/draft/2020-12/schema')
                walk(value, path)
            return value

        def walk(value, path):
            if isinstance(value, list):
                for item in value:
                    walk(item, path)
            elif isinstance(value, dict):
                if '$ref' in value:
                    ref = urlsplit(value['$ref'])
                    self.assertFalse(ref.scheme or ref.netloc or ref.query)
                    target = load((path.parent / ref.path).resolve() if ref.path else path)
                    if ref.fragment:
                        self.assertTrue(ref.fragment.startswith('/'))
                        for segment in unquote(ref.fragment[1:]).split('/'):
                            target = target[segment.replace('~1', '/').replace('~0', '~')]
                for item in value.values():
                    walk(item, path)

        load(schema_root / 'gateway-extension-package-v2.schema.json')
        load(schema_root / 'gateway-api-codec-v3.schema.json')

    def test_strict_json_and_canonical_unicode(self):
        self.assertEqual(runner.canonical({'z': '\u00e9', 'a': 1}), b'{"a":1,"z":"\\u00e9"}\n')
        for raw in (b'NaN', b'Infinity', b'{"x":1,"x":2}', b'\xff'):
            with self.subTest(raw=raw), self.assertRaises(runner.Failure):
                runner.decode(raw)


if __name__ == '__main__':
    unittest.main()
