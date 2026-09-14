from contextlib import contextmanager, redirect_stderr, redirect_stdout
import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / 'extension_manager.py'
spec = importlib.util.spec_from_file_location('extension_manager', SCRIPT)
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


@unittest.skipUnless(os.name == 'posix', 'Private extension stores require POSIX')
class ExtensionManagerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.store = self.root / 'store'
        self.binary = self.root / 'binary'
        self.binary.write_bytes(b'not executable: installation must never run package code\n')
        self.license = self.root / 'license'
        self.license.write_bytes(b'Synthetic test license\n')
        self.package = self.root / 'package'
        self.sha = m.package_binary(self.binary, self.license, self.package, 'observer', '0.1.0')

    def install(self):
        return m.install(self.store, self.package, self.sha)

    def enable(self):
        return m.enable(self.store, 'observer', '0.1.0', self.sha, m.PERMISSIONS)

    def rewrite_manifest(self, change):
        path = self.package / 'extension.json'
        value = json.loads(path.read_bytes())
        change(value)
        path.chmod(0o600)
        raw = m.canonical(value)
        path.write_bytes(raw)
        self.sha = m.digest(raw)

    def test_install_is_offline_and_does_not_activate_or_execute(self):
        self.install()
        self.assertFalse((self.store / 'active.json').exists())
        self.assertEqual(m.read_lock(self.store)['extensions'], [])
        self.assertEqual(list((self.store / 'state').iterdir()), [])

    def condition(self):
        report = m.inventory(self.store)
        return {key: report[key] for key in ('generation', 'inventory_sha256')}

    def test_inventory_distinguishes_installation_selection_and_damaged_bytes(self):
        self.install()
        installed = m.inventory(self.store)['inventory']
        self.assertEqual(installed['activation']['extensions'], [])
        self.assertTrue(installed['installed'][0]['verified'])
        self.assertFalse(installed['runtime_checked'])
        self.assertFalse(installed['removal_supported'])
        self.enable()
        self.assertEqual(len(m.inventory(self.store)['inventory']['activation']['extensions']), 1)
        binary = m.installed_dir(self.store, 'observer', '0.1.0', self.sha) / 'extension'
        binary.chmod(0o700)
        binary.write_bytes(b'damaged')
        damaged = m.inventory(self.store)['inventory']
        self.assertFalse(damaged['installed'][0]['verified'])
        self.assertIsNone(damaged['installed'][0]['package'])
        m.disable(self.store, 'observer', condition=self.condition())
        self.assertEqual(binary.read_bytes(), b'damaged')

    def test_precondition_is_checked_inside_the_legacy_mutation_lock(self):
        self.install()
        expected = self.condition()
        original = m.mutation_lock

        @contextmanager
        def intervening_writer(root):
            with original(root):
                m.commit_lock(root, m.read_lock(root))
            with original(root):
                yield

        with patch.object(m, 'mutation_lock', intervening_writer), self.assertRaises(m.ExtensionConflict):
            m.enable(self.store, 'observer', '0.1.0', self.sha, m.PERMISSIONS, condition=expected)
        self.assertEqual(m.read_lock(self.store)['extensions'], [])
        self.assertFalse((self.store / 'state' / 'observer').exists())

    def test_installation_inventory_changes_even_without_activation_generation(self):
        m.open_store(self.store)
        before = self.condition()
        self.install()
        self.assertEqual(m.read_lock(self.store)['generation'], before['generation'])
        with self.assertRaises(m.ExtensionConflict):
            m.enable(self.store, 'observer', '0.1.0', self.sha, m.PERMISSIONS, condition=before)
        self.assertEqual(list((self.store / 'state').iterdir()), [])

    def test_guarded_install_does_not_implicitly_initialize_a_store(self):
        with self.assertRaises(OSError):
            m.install(self.store, self.package, self.sha, condition={'generation': 0, 'inventory_sha256': '0' * 64})
        self.assertFalse(self.store.exists())

    def test_inventory_does_not_hide_interrupted_install_artifacts(self):
        self.install()
        (self.store / 'packages' / 'observer' / '0.1.0' / '.install-interrupted').mkdir(mode=0o700)
        with self.assertRaises(m.ExtensionError):
            m.inventory(self.store)

    def test_guarded_result_binds_the_completion_inventory(self):
        self.install()
        result = m.enable(self.store, 'observer', '0.1.0', self.sha, m.PERMISSIONS, condition=self.condition())
        self.assertEqual(result['result'], result['after']['inventory']['activation'])
        self.assertEqual(result['after'], m.inventory(self.store))

    def test_replacing_a_recorder_with_an_observer_preserves_data_and_clears_binding(self):
        recorder = self.root / 'recorder'
        sha = m.package_binary(self.binary, self.license, recorder, 'observer', '0.0.1', 'usage_recorder')
        m.install(self.store, recorder, sha)
        usage = self.store / 'usage'
        usage.mkdir(mode=0o700)
        data = usage / 'synthetic-store'
        data.mkdir(mode=0o700)
        m.write_new(data / 'recorder.json', b'{}\n')
        m.write_new(data / 'preserved', b'synthetic retained usage')
        binding = {'store_id': 'synthetic-store', 'mode': 'best_effort', 'queue_capacity': 2,
                   'ack_timeout_ms': 100, 'config_sha256': m.digest(b'{}\n')}
        m.enable(self.store, 'observer', '0.0.1', sha, m.RECORDER_PERMISSIONS, binding)
        self.install()
        result = m.enable(self.store, 'observer', '0.1.0', self.sha, m.PERMISSIONS, condition=self.condition())
        self.assertNotIn('recorder', m.read_lock(self.store))
        self.assertEqual(result['after']['inventory']['activation']['schema'], m.LOCK_SCHEMA)
        self.assertEqual((data / 'preserved').read_bytes(), b'synthetic retained usage')

    def test_old_python_rejects_before_store_creation(self):
        with patch.object(m.sys, 'version_info', (3, 10)), self.assertRaises(m.ExtensionError):
            self.install()
        self.assertFalse(self.store.exists())

    def test_enable_disable_and_reinstall_preserve_immutable_bytes(self):
        self.install()
        active = self.enable()
        self.assertEqual(active['generation'], 1)
        location = m.installed_dir(self.store, 'observer', '0.1.0', self.sha)
        original = (location / 'extension').read_bytes()
        self.assertEqual((location / 'extension').stat().st_mode & 0o777, 0o500)
        self.install()
        self.assertEqual(m.read_lock(self.store), active)
        disabled = m.disable(self.store, 'observer')
        self.assertEqual(disabled['generation'], 2)
        self.assertEqual(disabled['extensions'], [])
        self.assertEqual((location / 'extension').read_bytes(), original)
        self.assertTrue((self.store / 'state' / 'observer' / self.sha).is_dir())

    def test_permission_approval_is_exact_and_explicit(self):
        self.install()
        for grants in ([], ['observe_http_metadata'], [*m.PERMISSIONS, 'read_credentials'], [*m.PERMISSIONS, m.PERMISSIONS[0]]):
            with self.subTest(grants=grants), self.assertRaises(m.ExtensionError):
                m.enable(self.store, 'observer', '0.1.0', self.sha, grants)
        self.assertFalse((self.store / 'active.json').exists())

    def test_digest_mismatch_is_rejected(self):
        with self.assertRaises(m.ExtensionError):
            m.install(self.store, self.package, '0' * 64)
        file = self.package / 'extension'
        file.chmod(0o700)
        file.write_bytes(b'changed')
        with self.assertRaises(m.ExtensionError):
            self.install()

    def test_unlisted_file_is_rejected(self):
        (self.package / 'extra').write_text('extra')
        with self.assertRaises(m.ExtensionError):
            self.install()

    def test_path_traversal_and_unknown_fields_are_rejected(self):
        self.rewrite_manifest(lambda p: p['files'].update({'../escape': 'a' * 64}))
        with self.assertRaises(m.ExtensionError):
            self.install()

    def test_unknown_protocol_and_permissions_are_rejected(self):
        for key, value in (('protocol', 'gateway-credentials/v1'), ('permissions', ['read_credentials']), ('target', 'wrong-target'), ('state_schema', 'new-schema')):
            raw = json.loads((self.package / 'extension.json').read_bytes())
            raw[key] = value
            with self.subTest(key=key), self.assertRaises(m.ExtensionError):
                m.validate_package(m.canonical(raw))

    def test_noncanonical_and_duplicate_json_are_rejected(self):
        raw = (self.package / 'extension.json').read_bytes()
        with self.assertRaises(m.ExtensionError):
            m.validate_package(b' ' + raw)
        with self.assertRaises(m.ExtensionError):
            m.decode_json(b'{"a":1,"a":2}')
        with self.assertRaises(m.ExtensionError):
            m.decode_json(b'{"a":NaN}')

    def test_links_and_hardlinks_are_rejected(self):
        path = self.package / 'extension'
        path.unlink()
        path.symlink_to(self.binary)
        with self.assertRaises(m.ExtensionError):
            self.install()
        path.unlink()
        os.link(self.binary, path)
        with self.assertRaises(m.ExtensionError):
            self.install()

    def test_ancestor_link_and_nonprivate_store_are_rejected(self):
        alias = self.root / 'alias'
        alias.symlink_to(self.root, target_is_directory=True)
        with self.assertRaises(m.ExtensionError):
            m.install(alias / 'store', self.package, self.sha)
        self.store.mkdir(mode=0o755)
        self.store.chmod(0o755)
        with self.assertRaises(m.ExtensionError):
            self.install()

    def test_corrupted_installed_package_cannot_be_enabled(self):
        self.install()
        executable = m.installed_dir(self.store, 'observer', '0.1.0', self.sha) / 'extension'
        executable.chmod(0o700)
        executable.write_bytes(b'corrupt')
        with self.assertRaises(m.ExtensionError):
            self.enable()

    def test_atomic_activation_failure_preserves_prior_lock(self):
        self.install()
        self.enable()
        original = (self.store / 'active.json').read_bytes()
        with patch.object(m.os, 'replace', side_effect=OSError('synthetic failure')):
            with self.assertRaises(OSError):
                m.disable(self.store, 'observer')
        self.assertEqual((self.store / 'active.json').read_bytes(), original)
        self.assertEqual(list(self.store.glob('.active-*')), [])

    def test_second_mutator_fails_without_waiting(self):
        self.install()
        with m.mutation_lock(self.store):
            with self.assertRaises(m.ExtensionError):
                self.enable()

    def test_invalid_activation_never_silently_resets(self):
        self.install()
        self.enable()
        path = self.store / 'active.json'
        for raw in (b'broken', b'{"schema":"old"}', m.canonical({'schema':m.LOCK_SCHEMA, 'generation':True, 'extensions':[]})):
            path.write_bytes(raw)
            with self.assertRaises((ValueError, m.ExtensionError)):
                self.enable()
            self.assertEqual(path.read_bytes(), raw)

    def test_activation_overflow_does_not_modify_lock(self):
        self.install()
        self.enable()
        path = self.store / 'active.json'
        value = m.read_lock(self.store)
        value['generation'] = 2**64 - 1
        path.write_bytes(m.canonical(value))
        before = path.read_bytes()
        with self.assertRaises(m.ExtensionError):
            m.disable(self.store, 'observer')
        self.assertEqual(path.read_bytes(), before)

    def test_package_upgrade_is_separate_and_requires_fresh_grants(self):
        self.install()
        self.enable()
        second = self.root / 'second'
        new_sha = m.package_binary(self.binary, self.license, second, 'observer', '0.2.0')
        m.install(self.store, second, new_sha)
        self.assertEqual(m.read_lock(self.store)['extensions'][0]['package_sha256'], self.sha)
        m.enable(self.store, 'observer', '0.2.0', new_sha, m.PERMISSIONS)
        self.assertTrue(m.installed_dir(self.store, 'observer', '0.1.0', self.sha).exists())
        self.assertTrue((self.store / 'state' / 'observer' / self.sha).exists())
        self.assertTrue((self.store / 'state' / 'observer' / new_sha).exists())

    def test_bounds(self):
        with patch.object(m, 'MAX_BINARY', 4), self.assertRaises(m.ExtensionError):
            self.install()
        for value in ('../../a', 'x/y', '', 'Uppercase', 'a' * 65):
            self.assertFalse(m.identifier(value))
        for value in ('1.2', '01.2.3', '1.2.3-alpha', '1000000.0.0'):
            self.assertFalse(m.version(value))

    def test_status_does_not_create_or_execute(self):
        self.install()
        self.enable()
        out = io.StringIO()
        with redirect_stdout(out):
            self.assertEqual(m.main(['status', '--store', str(self.store)]), 0)
        self.assertFalse(json.loads(out.getvalue())['runtime_checked'])
        self.assertEqual(list((self.store / 'state' / 'observer' / self.sha).iterdir()), [])

    def test_cli_errors_do_not_echo_supplied_paths(self):
        out = io.StringIO()
        err = io.StringIO()
        with redirect_stdout(out), redirect_stderr(err):
            self.assertEqual(m.main(['inspect', '--package', str(self.package), '--expected-sha256', 'synthetic-private-value']), 1)
        self.assertNotIn('synthetic-private-value', err.getvalue())
        self.assertNotIn(str(self.root), err.getvalue())
        self.assertEqual(out.getvalue(), '')

    @staticmethod
    def codec_capabilities():
        return {'schema': m.CAPABILITIES_SCHEMA, 'apis': ['messages'],
                'features': ['editing', 'json', 'managed_continuation', 'streaming'],
                'requires': ['codec_ipc_v3', 'responses_output_validation']}

    @staticmethod
    def provider_capabilities():
        return {'schema': m.CAPABILITIES_SCHEMA, 'apis': [], 'features': ['json'],
                'requires': ['provider_ipc_v1', 'responses_output_validation']}

    def test_codec_v3_package_install_enable_and_inventory_bind_declaration(self):
        package = self.root / 'codec'
        capabilities = self.codec_capabilities()
        sha = m.package_binary(self.binary, self.license, package, 'codec', '1.0.0',
                               'api_codec', m.SUBSET_CODEC_PROTOCOL, capabilities=capabilities)
        value, _ = m.inspect_package(package, sha)
        self.assertEqual(value['schema'], m.CAPABILITY_PACKAGE_SCHEMA)
        self.assertEqual(value['capabilities'], capabilities)
        self.assertNotIn('provider_protocol', value)
        m.install(self.store, package, sha)
        before = m.inventory(self.store)['inventory']
        self.assertEqual(before['installed'][0]['package'], value)
        self.assertEqual(before['activation']['extensions'], [])
        m.enable(self.store, 'codec', '1.0.0', sha, m.CODEC_PERMISSIONS)
        after = m.inventory(self.store)['inventory']
        self.assertFalse(after['runtime_checked'])
        self.assertEqual(after['activation']['extensions'][0]['package_sha256'], sha)
        self.assertEqual(after['installed'][0]['package']['capabilities'], capabilities)
        path = package / 'extension.json'
        path.chmod(0o600)
        value['capabilities']['apis'] = ['responses']
        path.write_bytes(m.canonical(value))
        with self.assertRaises(m.ExtensionError):
            m.inspect_package(package, sha)

    def test_provider_static_install_preserves_identity_but_activation_is_unavailable(self):
        package = self.root / 'provider'
        sha = m.package_binary(self.binary, self.license, package, 'provider', '1.0.0',
                               'provider', capabilities=self.provider_capabilities(),
                               provider_protocol='synthetic.vendor/v1')
        value, _ = m.inspect_package(package, sha)
        self.assertEqual(value['protocol'], m.PROVIDER_PROTOCOL)
        self.assertEqual(value['provider_protocol'], 'synthetic.vendor/v1')
        self.assertEqual(value['state_schema'], 'provider-request-memory/v1')
        self.assertEqual(value['permissions'], m.CODEC_PERMISSIONS)
        m.install(self.store, package, sha)
        self.assertEqual(m.inventory(self.store)['inventory']['installed'][0]['package'], value)
        with self.assertRaises(m.ExtensionError):
            m.enable(self.store, 'provider', '1.0.0', sha, m.CODEC_PERMISSIONS)
        self.assertEqual(m.read_lock(self.store)['extensions'], [])
        self.assertFalse((self.store / 'state' / 'provider').exists())

    def test_capability_schema_and_arrays_reject_noncanonical_or_unsupported_values(self):
        for field, replacement in [
            ('schema', 'unknown/v1'), ('apis', []), ('apis', ['messages', 'messages']),
            ('apis', ['responses', 'messages']), ('apis', ['unknown']), ('apis', [True]),
            ('features', []), ('features', ['streaming']), ('features', ['json', 'unknown']),
            ('features', ['json', 'json']), ('requires', ['codec_ipc_v3']),
            ('requires', ['codec_ipc_v3', 'network', 'responses_output_validation']),
            ('requires', ['responses_output_validation', 'codec_ipc_v3']),
        ]:
            with self.subTest(field=field, replacement=replacement):
                capabilities = self.codec_capabilities()
                capabilities[field] = replacement
                with self.assertRaises(m.ExtensionError):
                    m.validate_capabilities(capabilities, m.SUBSET_CODEC_PROTOCOL)
        for field, replacement in [('apis', ['responses']), ('requires', ['codec_ipc_v3', 'responses_output_validation'])]:
            capabilities = self.provider_capabilities()
            capabilities[field] = replacement
            with self.assertRaises(m.ExtensionError):
                m.validate_capabilities(capabilities, m.PROVIDER_PROTOCOL)
        capabilities = self.codec_capabilities()
        capabilities['unknown'] = True
        with self.assertRaises(m.ExtensionError):
            m.validate_capabilities(capabilities, m.SUBSET_CODEC_PROTOCOL)

    def test_legacy_roles_do_not_gain_capabilities_or_new_protocols(self):
        original = json.loads((self.package / 'extension.json').read_bytes())
        for change in [
            {'capabilities': self.codec_capabilities()},
            {'protocol': m.SUBSET_CODEC_PROTOCOL},
            {'protocol': m.PROVIDER_PROTOCOL},
            {'schema': m.CAPABILITY_PACKAGE_SCHEMA, 'capabilities': self.codec_capabilities()},
        ]:
            with self.subTest(change=change), self.assertRaises(m.ExtensionError):
                m.validate_package(m.canonical({**original, **change}))
        for protocol in m.LEGACY_CODEC_PROTOCOLS:
            output = self.root / protocol.rsplit('/', 1)[1]
            with self.assertRaises(m.ExtensionError):
                m.package_binary(self.binary, self.license, output, 'codec', '1.0.0',
                                 'api_codec', protocol, capabilities=self.codec_capabilities())
            self.assertFalse(output.exists())

    def test_v2_required_fields_provider_identity_and_canonical_digest_are_strict(self):
        package = self.root / 'codec'
        sha = m.package_binary(self.binary, self.license, package, 'codec', '1.0.0',
                               'api_codec', m.SUBSET_CODEC_PROTOCOL, capabilities=self.codec_capabilities())
        value, _ = m.inspect_package(package, sha)
        invalid = [dict(value, capabilities=None), dict(value, provider_protocol=None),
                   {k: v for k, v in value.items() if k != 'capabilities'}]
        for manifest in invalid:
            with self.assertRaises(m.ExtensionError):
                m.validate_package(m.canonical(manifest))
        raw = m.canonical(value)
        with self.assertRaises(m.ExtensionError):
            m.validate_package(b' ' + raw)
        with self.assertRaises(m.ExtensionError):
            m.validate_package(raw.replace(b'"schema":', b'"schema":"duplicate","schema":', 1))
        for identity in [None, '', 'synthetic', 'synthetic/v0', 'synthetic/v01', 'synthetic/v1000000',
                         'https://synthetic/v1', 'Synthetic/v1', 'synthetic/v1\n']:
            with self.subTest(identity=identity), self.assertRaises(m.ExtensionError):
                m.package_binary(self.binary, self.license, self.root / 'bad-provider', 'provider', '1.0.0',
                                 'provider', capabilities=self.provider_capabilities(), provider_protocol=identity)
        self.assertFalse((self.root / 'bad-provider').exists())

    def test_cli_codec_v3_requires_explicit_capabilities_file(self):
        path = self.root / 'capabilities.json'
        path.write_bytes(m.canonical(self.codec_capabilities()))
        output = self.root / 'cli-codec'
        command = ['package', '--binary', str(self.binary), '--license-file', str(self.license),
                   '--output', str(output), '--id', 'codec', '--version', '1.0.0',
                   '--role', 'api_codec', '--codec-protocol', m.SUBSET_CODEC_PROTOCOL]
        with redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
            self.assertEqual(m.main(command), 1)
        self.assertFalse(output.exists())
        out = io.StringIO()
        with redirect_stdout(out):
            self.assertEqual(m.main(command + ['--capabilities', str(path)]), 0)
        m.inspect_package(output, json.loads(out.getvalue())['package_sha256'])

    def test_cross_target_static_package_does_not_relax_install(self):
        other = next(value for value in m.TARGETS if value != m.target())
        output = self.root / 'cross-package'
        sha = m.package_binary(self.binary, self.license, output, 'observer', '0.1.0', package_target=other)
        package, _ = m.inspect_package(output, sha, expected_target=other)
        self.assertEqual(package['target'], other)
        with self.assertRaises(m.ExtensionError):
            m.inspect_package(output, sha)
        with self.assertRaises(m.ExtensionError):
            m.install(self.store, output, sha)
        self.assertEqual(list((self.store / 'packages').iterdir()), [])
        out = io.StringIO()
        with redirect_stdout(out):
            self.assertEqual(m.main(['inspect', '--package', str(output), '--expected-sha256', sha, '--target', other]), 0)
        self.assertFalse(json.loads(out.getvalue())['executed'])

    def test_cross_target_still_checks_bytes_and_rejects_unknown_targets(self):
        with self.assertRaises(m.ExtensionError):
            m.package_binary(self.binary, self.license, self.root / 'bad', 'observer', '0.1.0', package_target='windows-x64')
        self.assertFalse((self.root / 'bad').exists())
        binary = self.package / 'extension'
        binary.chmod(0o600)
        binary.write_bytes(b'tampered')
        with self.assertRaises(m.ExtensionError):
            m.inspect_package(self.package, self.sha, expected_target=m.target())

    def test_unsupported_platform_fails_before_mutation(self):
        with patch.object(m.sys, 'platform', 'win32'), self.assertRaises(m.ExtensionError):
            self.install()
        self.assertFalse(self.store.exists())


if __name__ == '__main__':
    unittest.main()
