import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import math
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT=Path(__file__).resolve().parents[2]
HERE=ROOT/'.local/plugin-conformance-v2-tests'
HERE.mkdir(parents=True,exist_ok=True)
TOOL=ROOT/'tools/plugin-conformance/conformance.py'
spec=importlib.util.spec_from_file_location('public_conformance_v2',TOOL)
runner=importlib.util.module_from_spec(spec);spec.loader.exec_module(runner)


# Test-only evaluator for the vocabulary used by this report schema. This is not
# a general JSON Schema implementation. Preflight visits even unselected branches
# so new keywords can never silently weaken these contract checks.
REPORT_SCHEMA = json.loads((ROOT/'schemas/gateway-plugin-conformance-report-v2.schema.json').read_text())
SCHEMA_KEYWORDS = {
    '$schema', 'title', 'description', 'type', 'const', 'enum', 'pattern',
    'properties', 'required', 'additionalProperties', 'items', 'minItems',
    'allOf', 'if', 'then', 'else', 'not', 'contains',
}
JSON_TYPES = {'null', 'boolean', 'object', 'array', 'string', 'number', 'integer'}


def json_equal(left, right):
    if type(left) is bool or type(right) is bool:
        return type(left) is type(right) and left == right
    if isinstance(left, (int, float)) and isinstance(right, (int, float)):
        return left == right
    if type(left) is not type(right):
        return False
    if isinstance(left, list):
        return len(left) == len(right) and all(json_equal(a, b) for a, b in zip(left, right))
    if isinstance(left, dict):
        return left.keys() == right.keys() and all(json_equal(left[k], right[k]) for k in left)
    return left == right


def check_schema_vocabulary(schema):
    if type(schema) is bool:
        return
    if not isinstance(schema, dict):
        raise AssertionError('report schema must be an object or boolean')
    unknown = schema.keys() - SCHEMA_KEYWORDS
    if unknown:
        raise AssertionError('unsupported report schema keywords: ' + ', '.join(sorted(unknown)))
    if 'type' in schema:
        kinds = schema['type'] if isinstance(schema['type'], list) else [schema['type']]
        assert kinds and all(isinstance(k, str) and k in JSON_TYPES for k in kinds)
    if 'enum' in schema:
        assert isinstance(schema['enum'], list) and schema['enum']
    if 'pattern' in schema:
        assert isinstance(schema['pattern'], str)
        re.compile(schema['pattern'])
    if 'required' in schema:
        assert isinstance(schema['required'], list) and all(isinstance(k, str) for k in schema['required'])
    if 'minItems' in schema:
        assert type(schema['minItems']) is int and schema['minItems'] >= 0
    if 'properties' in schema:
        assert isinstance(schema['properties'], dict)
        for child in schema['properties'].values():
            check_schema_vocabulary(child)
    if 'allOf' in schema:
        assert isinstance(schema['allOf'], list) and schema['allOf']
        for child in schema['allOf']:
            check_schema_vocabulary(child)
    for key in ('additionalProperties', 'items', 'if', 'then', 'else', 'not', 'contains'):
        if key in schema:
            check_schema_vocabulary(schema[key])


def report_schema_accepts(value, schema=REPORT_SCHEMA):
    check_schema_vocabulary(schema)
    return _report_schema_accepts(value, schema)


def _report_schema_accepts(value, schema):
    if type(schema) is bool:
        return schema
    def accepts(child):
        return _report_schema_accepts(value, child)
    number = type(value) is int or (type(value) is float and math.isfinite(value))
    types = {'null': value is None, 'boolean': type(value) is bool,
             'object': isinstance(value, dict), 'array': isinstance(value, list),
             'string': isinstance(value, str), 'number': number,
             'integer': number and (type(value) is int or value.is_integer())}
    if 'type' in schema:
        kinds = schema['type'] if isinstance(schema['type'], list) else [schema['type']]
        if not any(types[k] for k in kinds):
            return False
    if 'const' in schema and not json_equal(value, schema['const']):
        return False
    if 'enum' in schema and not any(json_equal(value, item) for item in schema['enum']):
        return False
    if isinstance(value, str) and 'pattern' in schema and re.search(schema['pattern'], value) is None:
        return False
    if isinstance(value, dict):
        if any(key not in value for key in schema.get('required', [])):
            return False
        properties = schema.get('properties', {})
        for key, item in value.items():
            child = properties.get(key, schema.get('additionalProperties', True))
            if not _report_schema_accepts(item, child):
                return False
    if isinstance(value, list):
        if len(value) < schema.get('minItems', 0):
            return False
        if 'items' in schema and not all(_report_schema_accepts(item, schema['items']) for item in value):
            return False
        if 'contains' in schema and not any(_report_schema_accepts(item, schema['contains']) for item in value):
            return False
    if 'allOf' in schema and not all(accepts(child) for child in schema['allOf']):
        return False
    if 'not' in schema and accepts(schema['not']):
        return False
    if 'if' in schema:
        branch = 'then' if accepts(schema['if']) else 'else'
        if branch in schema and not accepts(schema[branch]):
            return False
    return True


def checked_run(*args, **kwargs):
    report = runner.run(*args, **kwargs)
    assert report_schema_accepts(report), 'emitted report violates its public schema'
    return report


class ReportContractTests(unittest.TestCase):
    def test_report_vectors_and_schema_have_closed_public_shape(self):
        schema=json.loads((ROOT/'schemas/gateway-plugin-conformance-report-v2.schema.json').read_text())
        corpus=json.loads((ROOT/'schemas/plugin-conformance-report-vectors.json').read_text())
        self.assertEqual(schema['properties']['schema']['const'],'gateway-plugin-conformance-report/v2')
        self.assertFalse(schema['additionalProperties'])
        self.assertEqual(set(schema['required']),set(schema['properties']))
        self.assertEqual(corpus['report_schema'], 'gateway-plugin-conformance-report-v2.schema.json')
        self.assertEqual(len(corpus['cases']), 7)
        for case in corpus['cases']:
            with self.subTest(vector=case['id']):
                self.assertEqual(report_schema_accepts(case['value'], schema), case['valid'])
        report=checked_run(Path('/nonexistent-synthetic-package'),'a'*64)
        self.assertEqual(set(report),set(schema['required']))
        self.assertEqual(report['status'],'fail')
        self.assertEqual(report['exit_code'],1)
        self.assertEqual(report['tool_version'],'2.0.0')
        self.assertEqual(len({c['id'] for c in report['checks']}),len(report['checks']))


    def test_closed_vocabulary_rejects_unknown_keywords_before_branch_selection(self):
        for schema in ({'unknown': True}, {'if': False, 'then': {'minimum': 1}},
                       {'properties': {'absent': {'oneOf': [True]}}}):
            with self.assertRaisesRegex(AssertionError, 'unsupported report schema keywords'):
                report_schema_accepts({}, schema)

    def test_boolean_numeric_equality_null_and_condition_semantics(self):
        for keyword in ('const', 'enum'):
            def rule(value):
                return {keyword: [value] if keyword == 'enum' else value}
            self.assertFalse(report_schema_accepts(True, rule(1)))
            self.assertFalse(report_schema_accepts(0, rule(False)))
            self.assertFalse(report_schema_accepts({'x': [True]}, rule({'x': [1]})))
            self.assertTrue(report_schema_accepts(1.0, rule(1)))
            self.assertTrue(report_schema_accepts({'x': [1.0]}, rule({'x': [1]})))
        self.assertFalse(report_schema_accepts(True, {'type': 'integer'}))
        self.assertTrue(report_schema_accepts(1.0, {'type': 'integer'}))
        self.assertFalse(report_schema_accepts(1.5, {'type': 'integer'}))
        self.assertTrue(report_schema_accepts(None, {'type': ['string', 'null'], 'pattern': '^a$'}))
        self.assertFalse(report_schema_accepts({}, {'required': ['x']}))
        self.assertTrue(report_schema_accepts({'x': None}, {'required': ['x']}))
        self.assertFalse(report_schema_accepts({'extra': 1}, {'additionalProperties': False}))
        conditional = {'if': {'properties': {'flag': {'const': True}}},
                       'then': {'required': ['yes']}, 'else': {'required': ['no']}}
        self.assertFalse(report_schema_accepts({}, conditional))
        self.assertTrue(report_schema_accepts({'yes': None}, conditional))
        self.assertTrue(report_schema_accepts({'flag': False, 'no': None}, conditional))
        self.assertTrue(report_schema_accepts({}, {'then': False, 'else': False}))
        self.assertTrue(report_schema_accepts(4, {'required': ['x'], 'items': False, 'pattern': '^a$'}))
        self.assertFalse(report_schema_accepts([], {'contains': True}))
        self.assertTrue(report_schema_accepts([1], {'allOf': [{'minItems': 1}, {'items': {'not': {'type': 'boolean'}}}], 'contains': {'const': 1.0}}))


@unittest.skipUnless(sys.platform in ('darwin', 'linux'), 'Native plugin execution requires Linux or macOS')
class ConformanceV2Tests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory(prefix='candidate-',dir=HERE)
        self.addCleanup(self.tmp.cleanup)
        self.root=Path(self.tmp.name).resolve();self.root.chmod(0o700)
        self.scratch=self.root/'runs';self.scratch.mkdir(mode=0o700)
        self.package=self.root/'package'

    def build(self,role):
        copied=self.root/'copied'
        source=ROOT/f'tools/plugin-conformance/examples/{role}'
        shutil.copytree(source,copied)
        target=runner.host_target()
        result=subprocess.run([sys.executable,'-B',str(copied/'package.py'),'--output',str(self.package),'--target',target],
                              check=True,capture_output=True,cwd=self.root)
        return result.stdout.decode().strip()

    def manifest(self,source,protocol='gateway-provider/v1',change=None):
        self.package.mkdir()
        (self.package/'extension').write_bytes(('#!'+str(Path(sys.executable).resolve())+'\n'+source).encode())
        (self.package/'LICENSE.txt').write_text('Synthetic fixture notice\n')
        caps={'schema':'gateway-plugin-capabilities/v1','apis':[],'features':['json'],'requires':['provider_ipc_v1','responses_output_validation']}
        value={'schema':'gateway-extension-package/v2','id':'synthetic','version':'1.0.0','target':runner.host_target(),
               'protocol':protocol,'permissions':['read_model_payload','transform_model_protocol'],'state_schema':'provider-request-memory/v1',
               'provider_protocol':'synthetic-provider/v1','capabilities':caps,
               'files':{n:runner.digest((self.package/n).read_bytes()) for n in ['extension','LICENSE.txt']}}
        if protocol=='gateway-observer/v1':
            value.pop('provider_protocol');value.pop('capabilities')
            value.update(schema='gateway-extension-package/v1',permissions=['observe_http_metadata','write_private_state'],state_schema='observer-state/v1')
        if protocol=='gateway-usage-recorder/v2':
            value.pop('provider_protocol');value.update(permissions=['export_usage','observe_usage','write_usage_store'],state_schema='usage-store/v2',capabilities=runner.RECORDER_CAPS)
        if change:change(value)
        raw=runner.canonical(value);(self.package/'extension.json').write_bytes(raw)
        return runner.digest(raw)

    def execute(self,digest,profile='wire',fixture=None):
        return checked_run(self.package,digest,True,self.scratch,profile,fixture)

    def test_complete_synthetic_provider_profile(self):
        checksum=self.build('provider');report=self.execute(checksum,'synthetic-provider/v1')
        self.assertEqual(report['status'],'pass',report)
        self.assertEqual(report['exit_code'],0)
        self.assertEqual(report['schema'],'gateway-plugin-conformance-report/v2')
        self.assertEqual(len(report['fixture_sha256']),64)
        self.assertEqual(report['checks'][-2]['id'],'host.integration')
        self.assertFalse(next(c for c in report['checks'] if c['id']=='host.integration')['required'])
        self.assertEqual(list(self.scratch.iterdir()),[])

    def test_generic_provider_ready_does_not_claim_semantics(self):
        checksum=self.build('provider');report=self.execute(checksum)
        self.assertEqual(report['status'],'not-run',report);self.assertEqual(report['exit_code'],2)
        self.assertEqual(next(c for c in report['checks'] if c['id']=='provider.ready')['status'],'pass')
        self.assertEqual(next(c for c in report['checks'] if c['id']=='provider.json')['code'],'semantic_fixture_required')

    def test_recorder_profile_uses_copy_and_preserves_fixture(self):
        checksum=self.build('recorder');fixture=self.root/'fixture';fixture.mkdir(mode=0o700)
        subprocess.run([str(self.package/'extension'),'init'],cwd=fixture,env={},check=True,capture_output=True)
        before={p.name:p.read_bytes() for p in fixture.iterdir()}
        report=self.execute(checksum,'recorder-events/v2',fixture)
        self.assertEqual(report['status'],'pass',report)
        self.assertEqual({p.name:p.read_bytes() for p in fixture.iterdir()},before)
        self.assertEqual(list(self.scratch.iterdir()),[])

    def test_missing_recorder_fixture_never_guesses_init(self):
        marker=self.root/'not-executed'
        checksum=self.manifest(f'open({str(marker)!r},"w").close()\n',protocol='gateway-usage-recorder/v2')
        report=self.execute(checksum,'recorder-events/v2')
        self.assertEqual(report['exit_code'],2);self.assertFalse(marker.exists())

    def test_target_and_profile_mismatch_fail_before_execution(self):
        marker=self.root/'not-executed'
        checksum=self.manifest(f'open({str(marker)!r},"w").close()\n',change=lambda v:v.update(target=next(t for t in runner.TARGETS if t!=runner.host_target())))
        self.assertEqual(self.execute(checksum)['status'],'fail');self.assertFalse(marker.exists())
        self.assertEqual(self.execute(checksum,'recorder-events/v2')['status'],'fail');self.assertFalse(marker.exists())

    def test_wrong_ready_and_partial_prefix_fail_with_cleanup(self):
        checksum=self.manifest('import sys,struct\nraw=b"{}"\nsys.stdout.buffer.write(struct.pack(">I",len(raw))+raw);sys.stdout.buffer.flush()\n')
        report=self.execute(checksum)
        self.assertEqual(report['status'],'fail');self.assertEqual(list(self.scratch.iterdir()),[])
        shutil.rmtree(self.package)
        checksum=self.manifest('import sys\nsys.stdout.buffer.write(b"\\0\\0");sys.stdout.buffer.flush()\n')
        report=self.execute(checksum)
        self.assertEqual(report['status'],'fail');self.assertEqual(list(self.scratch.iterdir()),[])

    def test_ready_timeout_kills_direct_child(self):
        marker=self.root/'pid'
        checksum=self.manifest(f'import os,time\nopen({str(marker)!r},"w").write(str(os.getpid()))\ntime.sleep(30)\n')
        report=self.execute(checksum)
        self.assertEqual(report['status'],'fail');self.assertEqual(report['checks'][-3]['code'],'deadline')
        with self.assertRaises(ProcessLookupError):os.kill(int(marker.read_text()),0)

    def test_state_links_are_rejected_before_recorder_execution(self):
        marker=self.root/'not-executed'
        checksum=self.manifest(f'open({str(marker)!r},"w").close()\n',protocol='gateway-usage-recorder/v2')
        fixture=self.root/'fixture';fixture.mkdir(mode=0o700);(fixture/'linked').symlink_to(self.package/'extension')
        report=self.execute(checksum,'recorder-events/v2',fixture)
        self.assertEqual(report['status'],'fail');self.assertFalse(marker.exists())

    def test_public_cli_is_standalone_and_report_has_no_paths_or_payloads(self):
        checksum=self.build('provider');tool=self.root/'standalone.py';shutil.copyfile(TOOL,tool)
        process=subprocess.run([sys.executable,'-I','-B',str(tool),'--package',str(self.package),'--expected-sha256',checksum,'--execute',
                                '--state-root',str(self.scratch),'--profile','synthetic-provider/v1'],cwd=self.root,check=True,capture_output=True)
        value=process.stdout.decode();self.assertNotIn(str(self.root),value);self.assertNotIn('arguments',value)
        self.assertTrue(report_schema_accepts(json.loads(value)))
        self.assertEqual(json.loads(value)['status'],'pass')


    def test_observer_contract_is_preserved(self):
        source=(ROOT/'tools/plugin-conformance/examples/observer/observer.py').read_text()
        checksum=self.manifest(source,protocol='gateway-observer/v1')
        report=self.execute(checksum)
        self.assertEqual(report['status'],'pass',report)
        self.assertEqual(next(c for c in report['checks'] if c['id']=='observer.ack_sequence')['status'],'pass')

    def test_wrong_recorder_ack_is_a_failed_attempted_check(self):
        ready={'type':'ready','protocol':'gateway-usage-recorder/v2','producer_id':'synthetic-producer','capabilities':runner.RECORDER_CAPS}
        source='import sys,json\nprint('+repr(json.dumps(ready))+',flush=True)\nfor line in sys.stdin: print(json.dumps({"type":"committed","event_id":"wrong","sha256":"'+'0'*64+'"}),flush=True)\n'
        checksum=self.manifest(source,protocol='gateway-usage-recorder/v2')
        fixture=self.root/'fixture';fixture.mkdir(mode=0o700)
        report=self.execute(checksum,'recorder-events/v2',fixture)
        self.assertEqual(report['status'],'fail',report)
        attempted=next(c for c in report['checks'] if c['id']=='recorder.v1_ack')
        self.assertEqual(attempted['status'],'fail');self.assertEqual(attempted['code'],'invalid_ack')
        self.assertEqual(list(self.scratch.iterdir()),[])

    def test_static_only_never_executes_and_reports_incomplete_coverage(self):
        marker=self.root/'not-executed'
        checksum=self.manifest(f'open({str(marker)!r},"w").close()\n')
        report=checked_run(self.package,checksum)
        self.assertEqual(report['status'],'not-run');self.assertEqual(report['exit_code'],0)
        self.assertFalse(marker.exists())


    def test_scratch_cleanup_failure_cannot_report_teardown_pass(self):
        checksum=self.build('provider')
        original=runner.tempfile.TemporaryDirectory.cleanup
        def failed_cleanup(temporary):
            original(temporary)
            raise OSError('synthetic cleanup failure')
        with patch.object(runner.tempfile.TemporaryDirectory,'cleanup',failed_cleanup):
            report=self.execute(checksum)
        teardown=next(c for c in report['checks'] if c['id']=='harness.teardown')
        self.assertEqual(report['status'],'fail',report)
        self.assertEqual(teardown['status'],'fail',report)
        self.assertEqual(teardown['code'],'cleanup_failed')
        self.assertEqual(list(self.scratch.iterdir()),[])


if __name__=='__main__':unittest.main()
