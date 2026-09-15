"""Bounded harness checks; these do not execute or qualify a gateway binary."""
import argparse
import copy
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import shutil
import sys
import os
import tempfile
import unittest
from unittest.mock import Mock, patch
from urllib.error import HTTPError

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('provider_acceptance', ROOT / 'scripts/provider_acceptance.py')
acceptance = importlib.util.module_from_spec(spec)
spec.loader.exec_module(acceptance)


class AcceptanceTests(unittest.TestCase):
    def test_copy_project_retains_exact_bytes_and_digest(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / 'source'; source.mkdir()
            files = {'package.py': b'package\n', 'build.py': b'build\n', 'LICENSE.txt': b'notice\n'}
            for name, raw in files.items():
                (source / name).write_bytes(raw)
            result = acceptance.copy_project(source, root / 'copy')
            expected = hashlib.sha256(acceptance.encode({name: hashlib.sha256(raw).hexdigest() for name, raw in files.items()})).hexdigest()
            self.assertEqual(result, expected)
            self.assertEqual({p.name: p.read_bytes() for p in (root / 'copy').iterdir()}, files)

    def test_changed_copy_fails_instead_of_relabeling(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); source = root / 'source'; source.mkdir()
            for name in ('package.py', 'build.py', 'LICENSE.txt'):
                (source / name).write_bytes(b'original')
            def changed_copy(_source, destination):
                destination.write_bytes(b'changed')
            with patch.object(shutil, 'copyfile', side_effect=changed_copy):
                with self.assertRaisesRegex(acceptance.Failure, '^example_copy_changed$'):
                    acceptance.copy_project(source, root / 'copy')

    def test_remote_endpoint_rejected_without_open(self):
        class Client:
            def open(self, *_args, **_kwargs):
                raise AssertionError('must not connect')
        for operation in (acceptance.request, acceptance.rejected_request):
            kwargs = {} if operation is acceptance.request else {'status': 409, 'code': 'continuation_rejected'}
            with self.assertRaisesRegex(acceptance.Failure, '^non_synthetic_endpoint$'):
                operation(Client(), 'https://example.invalid/responses', {}, **kwargs)

    def test_negative_request_requires_exact_status_and_error(self):
        class Client:
            def __init__(self, status, code): self.status, self.code = status, code
            def open(self, *_args, **_kwargs):
                raise HTTPError('http://127.0.0.1/', self.status, 'fixed', {}, io.BytesIO(acceptance.encode({'error': {'code': self.code}})))
        acceptance.rejected_request(Client(409, 'continuation_rejected'), 'http://127.0.0.1/', {}, status=409, code='continuation_rejected')
        for status, code in ((400, 'continuation_rejected'), (409, 'other')):
            with self.assertRaises(acceptance.Failure):
                acceptance.rejected_request(Client(status, code), 'http://127.0.0.1/', {}, status=409, code='continuation_rejected')

    def test_unsupported_platform_is_explicitly_not_run(self):
        args=argparse.Namespace(binary=Path('/unused'),manager=None,provider_example=None,recorder_example=None,python=None,query_recorder=None,work_root=None)
        with patch.object(acceptance,'target',side_effect=acceptance.Failure('unsupported_native_host')):
            report=acceptance.run(args)
        self.assertEqual(report['status'],'not-run')
        self.assertEqual(report['checks'][-1]['code'],'platform_unsupported')
        self.assertEqual(report['tool_version'],acceptance.TOOL_VERSION)
        self.assertIn('gateway-provider/v1',report['contracts'])

    def test_report_output_is_private_and_exact(self):
        with tempfile.TemporaryDirectory() as temporary:
            path=Path(temporary).resolve()/'report.json'
            report={'status':'not-run','checks':[]}
            acceptance.write_report(path,report)
            self.assertEqual(path.read_bytes(),acceptance.encode(report)+b'\n')
            if os.name != 'nt':
                self.assertEqual(path.stat().st_mode & 0o077,0)
            self.assertEqual(list(path.parent.iterdir()),[path])

    def test_failure_report_contains_no_paths_or_exception_payload(self):
        args = argparse.Namespace(binary=Path('/synthetic/private/binary'), manager=None, provider_example=None,
                                  recorder_example=None, python=None, query_recorder=None, work_root=None)
        with patch.object(acceptance, 'target', return_value='linux-x64'), patch.object(acceptance, 'digest', side_effect=OSError('secret body')):
            report = acceptance.run(args)
        serialized = json.dumps(report)
        self.assertNotIn('secret body', serialized)
        self.assertNotIn('/synthetic', serialized)
        self.assertEqual(report['status'], 'fail')
        self.assertEqual(report['checks'][-1]['code'], 'fixture_or_io_failure')
        self.assertTrue(all(item['status'] == 'not-run' for item in report['remaining_acceptance']))

    def test_ledger_rejects_duplicate_attempts_failed_success_and_changed_hash(self):
        manifest={'id':'synthetic-provider','version':'1.0.0','files':{'extension':'b'*64}}
        interpretation={'kind':'trusted_provider_plugin','protocol':'gateway-provider/v1','provider_protocol':'synthetic-provider/v1',
                        'package_id':manifest['id'],'package_version':manifest['version'],'package_sha256':'a'*64,'executable_sha256':'b'*64}
        events=[]
        for index, case in enumerate(acceptance.CASES):
            counters={'input_tokens':{'value':7,'source':'reported'},'output_tokens':{'value':2,'source':'reported'},
                      'cache_read_input_tokens':{'value':None,'source':'not_reported'}}
            if case=='numeric_invalid': counters['input_tokens']={'value':None,'source':'invalid'}
            if case=='numeric_missing': counters={name:{'value':None,'source':'not_reported'} for name in counters}
            event={'schema':'gateway-usage-event/v1' if index==0 else 'gateway-usage-event/v2','producer_id':'recorder','attempt_id':str(index),
                   'gateway':'conversion_failed' if case=='numeric_invalid' else 'completed','upstream':'completed',
                   'observation_incomplete':case=='numeric_invalid','usage':{'counters':counters,'reported':{},'cache_write_details':[]}}
            if index: event['interpretation']=interpretation
            for kind in ('attempt_started','attempt_finished'):
                events.append(dict(event,kind=kind))
        def rows(values):
            return [(str(index),acceptance.encode(event),hashlib.sha256(acceptance.encode(event)).hexdigest()) for index,event in enumerate(values)]
        with patch.object(acceptance,'ledger_rows',return_value=rows(events)):
            self.assertEqual(acceptance.inspect_usage(Path('/unused'),'recorder',manifest,'a'*64),len(events))
        bad=copy.deepcopy(events);bad[-1]['attempt_id']=bad[-3]['attempt_id']
        with patch.object(acceptance,'ledger_rows',return_value=rows(bad)):
            with self.assertRaisesRegex(acceptance.Failure,'duplicate_terminal_attempt'):
                acceptance.inspect_usage(Path('/unused'),'recorder',manifest,'a'*64)
        bad=copy.deepcopy(events);bad[1]['gateway']='cancelled'
        with patch.object(acceptance,'ledger_rows',return_value=rows(bad)):
            with self.assertRaisesRegex(acceptance.Failure,'successful_usage_outcome'):
                acceptance.inspect_usage(Path('/unused'),'recorder',manifest,'a'*64)
        badrows=rows(events);badrows[0]=(badrows[0][0],badrows[0][1],'0'*64)
        with patch.object(acceptance,'ledger_rows',return_value=badrows):
            with self.assertRaisesRegex(acceptance.Failure,'stored_event_hash'):
                acceptance.inspect_usage(Path('/unused'),'recorder',manifest,'a'*64)

    @unittest.skipUnless(hasattr(os, 'killpg'), 'Native process groups require Linux or macOS')
    def test_cleanup_kills_owned_group_after_parent_already_exited(self):
        child = Mock(pid=12345, stdout=None)
        child.poll.return_value = 0
        with patch.object(acceptance.os, 'killpg') as kill:
            acceptance.stop(child)
            acceptance.stop(child)
        kill.assert_called_once_with(child.pid, acceptance.signal.SIGKILL)
        child.terminate.assert_not_called()

    @unittest.skipUnless(hasattr(os, 'killpg'), 'Native process groups require Linux or macOS')
    def test_command_deadline_reaps_child_and_redacts_output(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            marker = root / 'pid'
            source = 'import os,time;open(' + repr(str(marker)) + ',"w").write(str(os.getpid()));time.sleep(30)'
            with self.assertRaisesRegex(acceptance.Failure, '^command_deadline$'):
                acceptance.command([sys.executable, '-c', source], root, {}, timeout=0.5)
            with self.assertRaises(ProcessLookupError):
                os.kill(int(marker.read_text()), 0)

    def test_decode_preserves_integer_precision_and_rejects_duplicates(self):
        self.assertEqual(acceptance.decode(b'{"n":18446744073709551615}')['n'], 2**64 - 1)
        with self.assertRaisesRegex(acceptance.Failure, 'duplicate_json_field'):
            acceptance.decode(b'{"n":0,"n":1}')


if __name__ == '__main__':
    unittest.main()
