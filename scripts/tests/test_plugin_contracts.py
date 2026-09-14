"""Check public contract artifact links and byte vectors without optional packages."""
import hashlib
import importlib.util
import json
import re
from pathlib import Path
import unittest
from unittest.mock import patch
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[2]
SCHEMAS = ROOT / 'schemas'


def read_json(path):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError('Duplicate JSON key')
            result[key] = value
        return result
    return json.loads(path.read_bytes(), object_pairs_hook=pairs)


class PublicPluginContracts(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.vectors = read_json(SCHEMAS / 'plugin-vectors.json')
        spec = importlib.util.spec_from_file_location('contract_manager', ROOT / 'scripts/extension_manager.py')
        cls.manager = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.manager)

    def test_package_byte_identity_and_role_cases_match_installer(self):
        vector = self.vectors['canonical_package']
        raw = vector['utf8'].encode('utf-8')
        self.assertEqual(self.manager.canonical(vector['value']), raw)
        self.assertEqual(hashlib.sha256(raw).hexdigest(), vector['sha256'])
        self.assertEqual(set(vector['files_utf8']), set(vector['value']['files']))
        for name, data in vector['files_utf8'].items():
            self.assertEqual(hashlib.sha256(data.encode()).hexdigest(), vector['value']['files'][name])
        # This is static package validation. It does not assert host-target acceptance.
        with patch.object(self.manager, 'target', return_value=vector['value']['target']):
            for case in self.vectors['cases']:
                if case['schema'] != 'gateway-extension-package-v1.schema.json':
                    continue
                with self.subTest(case=case['id']):
                    encoded = self.manager.canonical(case['value'])
                    if case['valid']:
                        self.assertEqual(self.manager.validate_package(encoded), case['value'])
                    else:
                        with self.assertRaises((ValueError, TypeError, KeyError)):
                            self.manager.validate_package(encoded)

    def test_usage_ack_binds_exact_event_bytes_without_newline(self):
        vector = self.vectors['canonical_usage_event']
        raw = vector['utf8'].encode('utf-8')
        self.assertFalse(raw.endswith(b'\n'))
        self.assertEqual(json.loads(raw), vector['value'])
        self.assertEqual(hashlib.sha256(raw).hexdigest(), vector['sha256'])
        self.assertNotEqual(hashlib.sha256(raw + b'\n').hexdigest(), vector['sha256'])
        self.assertEqual(vector['ack'], {'type': 'committed',
                                       'event_id': vector['value']['event_id'],
                                       'sha256': vector['sha256']})

    def test_published_patterns_reject_terminal_newlines(self):
        package = read_json(SCHEMAS / 'gateway-extension-package-v1.schema.json')
        recorder = read_json(SCHEMAS / 'gateway-usage-recorder-v1.schema.json')
        usage = read_json(SCHEMAS / 'gateway-usage-event-v1.schema.json')
        patterns = [
            (usage['properties']['producer_id']['pattern'], 'synthetic-producer'),
            (usage['properties']['configuration_sha256']['pattern'], '0' * 64),
            (package['properties']['id']['pattern'], 'example-observer'),
            (package['properties']['version']['pattern'], '1.0.0'),
            (package['properties']['files']['propertyNames']['pattern'], 'LICENSE.txt'),
            (package['properties']['files']['additionalProperties']['pattern'], '0' * 64),
            (recorder['oneOf'][0]['properties']['producer_id']['pattern'], 'synthetic-producer'),
        ]
        for pattern, value in patterns:
            with self.subTest(pattern=pattern):
                self.assertIsNotNone(re.search(pattern, value))
                self.assertIsNone(re.search(pattern, value + '\n'))

    def test_schema_references_are_local_and_resolvable(self):
        names = {case['schema'] for case in self.vectors['cases']}
        self.assertEqual(len(names), 5)
        visited = set()

        def walk(value, document):
            if isinstance(value, list):
                for item in value:
                    walk(item, document)
            elif isinstance(value, dict):
                if '$ref' in value:
                    link = urlsplit(value['$ref'])
                    self.assertFalse(link.scheme or link.netloc or link.query)
                    destination = (document.parent / link.path).resolve() if link.path else document
                    self.assertTrue(destination.is_relative_to(SCHEMAS))
                    target = load(destination)
                    if link.fragment:
                        self.assertTrue(link.fragment.startswith('/'))
                        for part in unquote(link.fragment[1:]).split('/'):
                            target = target[part.replace('~1', '/').replace('~0', '~')]
                for item in value.values():
                    walk(item, document)

        def load(path):
            value = read_json(path)
            if path not in visited:
                visited.add(path)
                self.assertEqual(value['$schema'], 'https://json-schema.org/draft/2020-12/schema')
                walk(value, path)
            return value

        for name in names:
            load(SCHEMAS / name)

    def test_vector_inventory_and_observer_framing_are_consistent(self):
        cases = self.vectors['cases']
        self.assertEqual(self.vectors['schema'], 'gateway-plugin-contract-vectors/v1')
        self.assertEqual(len({case['id'] for case in cases}), len(cases))
        by_id = {case['id']: case for case in cases}
        for case in cases:
            self.assertEqual(set(case), {'id', 'schema', 'value', 'valid'})
            self.assertIs(type(case['valid']), bool)
        for frame in self.vectors['observer_frames']:
            case = by_id[frame['id']]
            raw = frame['utf8'].encode()
            self.assertTrue(raw.endswith(b'\n'))
            self.assertEqual(raw.count(b'\n'), 1)
            self.assertLessEqual(len(raw), 4096)
            self.assertEqual(json.loads(raw), case['value'])
            self.assertEqual(frame['valid'], case['valid'])


if __name__ == '__main__':
    unittest.main()
