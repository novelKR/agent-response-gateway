import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'build_plugin_tools.py'
spec = importlib.util.spec_from_file_location('build_plugin_tools', SCRIPT)
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)
BASE = 'python:3.14.0-slim-bookworm@sha256:' + 'a' * 64  # Synthetic; never pulled.


class PluginToolsTests(unittest.TestCase):
    def test_standalone_archive_is_deterministic_and_runs_outside_checkout(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            first, second = root / 'first.tar', root / 'second.tar'
            self.assertEqual(m.assemble(first, BASE), m.assemble(second, BASE))
            extracted = root / 'extracted'
            extracted.mkdir()
            with tarfile.open(first) as archive:
                self.assertEqual(set(archive.getnames()), {
                    'extension_manager.py', 'build_plugin_tools.py', 'LICENSE',
                    'README.md', 'Dockerfile', 'SHA256SUMS',
                })
                archive.extractall(extracted, filter='data')
            for line in (extracted / 'SHA256SUMS').read_text().splitlines():
                sha, name = line.split('  ')
                self.assertEqual(hashlib.sha256((extracted / name).read_bytes()).hexdigest(), sha)
            binary, notice = root / 'binary', root / 'notice'
            binary.write_bytes(b'synthetic non-executable bytes')
            notice.write_text('Synthetic notice\n')
            command = [sys.executable, '-I', '-B', str(extracted / 'extension_manager.py')]
            result = subprocess.run(command + [
                'package', '--binary', str(binary), '--license-file', str(notice),
                '--output', str(root / 'package'), '--id', 'sample', '--version', '1.0.0',
                '--target', 'macos-arm64',
            ], cwd=root, check=True, capture_output=True, text=True)
            sha = json.loads(result.stdout)['package_sha256']
            subprocess.run(command + ['inspect', '--package', str(root / 'package'),
                '--expected-sha256', sha, '--target', 'macos-arm64'],
                cwd=root, check=True, capture_output=True)

    def test_relocated_archive_conformance_reports_public_v2(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            archive_path = root / 'tools.tar'
            source = SCRIPT.parents[1] / 'tools/plugin-conformance/conformance.py'
            m.assemble(archive_path, BASE, source)
            extracted = root / 'tools'; extracted.mkdir()
            with tarfile.open(archive_path) as archive:
                archive.extractall(extracted, filter='data')
            package = root / 'package'; package.mkdir()
            data = {'extension': b'inert synthetic fixture', 'LICENSE.txt': b'Synthetic notice'}
            for name, raw in data.items():
                (package / name).write_bytes(raw)
            manifest = {'schema':'gateway-extension-package/v1','id':'sample','version':'1.0.0',
                        'target':'linux-x64','protocol':'gateway-observer/v1',
                        'permissions':['observe_http_metadata','write_private_state'],
                        'state_schema':'observer-state/v1',
                        'files':{name:hashlib.sha256(raw).hexdigest() for name,raw in data.items()}}
            raw = json.dumps(manifest,sort_keys=True,separators=(',',':')).encode()+b'\n'
            (package/'extension.json').write_bytes(raw)
            process = subprocess.run([sys.executable,'-I','-B',str(extracted/'plugin_conformance.py'),
                '--package',str(package),'--expected-sha256',hashlib.sha256(raw).hexdigest()],
                cwd=root,check=True,capture_output=True,timeout=15)
            report=json.loads(process.stdout)
            self.assertEqual(report['schema'],'gateway-plugin-conformance-report/v2')
            self.assertEqual(report['tool_version'],'2.0.0')
            self.assertEqual(report['tool_sha256'],hashlib.sha256(source.read_bytes()).hexdigest())
            self.assertEqual(report['status'],'not-run')
            self.assertEqual(report['exit_code'],0)
            self.assertNotIn(str(root),process.stdout.decode())

    def test_base_must_be_immutable_and_output_is_not_overwritten(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / 'tools.tar'
            with self.assertRaises(ValueError):
                m.assemble(output, 'python:3.14-slim')
            self.assertFalse(output.exists())
            output.write_bytes(b'preserve')
            with self.assertRaises(FileExistsError):
                m.assemble(output, BASE)
            self.assertEqual(output.read_bytes(), b'preserve')

    def test_optional_conformance_is_copied_explicitly(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            script = root / 'conformance.py'
            script.write_bytes(b'print("synthetic")\n')
            output = root / 'tools.tar'
            m.assemble(output, BASE, script)
            with tarfile.open(output) as archive:
                self.assertEqual(archive.extractfile('plugin_conformance.py').read(), script.read_bytes())
                self.assertIn(b'plugin_conformance.py', archive.extractfile('Dockerfile').read())


if __name__ == '__main__':
    unittest.main()
