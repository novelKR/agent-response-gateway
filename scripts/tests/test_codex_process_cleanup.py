"""Owned conformance subprocess cleanup; no gateway or Codex executable required."""
import importlib.util
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
with patch.object(sys, 'path', [str(ROOT / 'tests/codex'), *sys.path]):
    spec = importlib.util.spec_from_file_location('cleanup_conformance', ROOT / 'tests/codex/conformance.py')
    harness = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(harness)


SAFE_STOP = harness.stop_process


@unittest.skipUnless(os.name == 'posix', 'Owned process groups require POSIX')
class CleanupTests(unittest.TestCase):
    def setUp(self):
        local = ROOT / '.local/process-cleanup-tests'
        local.mkdir(parents=True, exist_ok=True)
        self.directory = tempfile.TemporaryDirectory(dir=local)
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)

    def launch(self, source):
        child = harness.start_process([sys.executable, '-I', '-c', source],
                                      stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                      stderr=subprocess.DEVNULL)
        self.addCleanup(harness.stop_process, child)
        return child

    def wait_file(self, path):
        deadline = time.monotonic() + 5
        while not path.exists():
            self.assertLess(time.monotonic(), deadline, 'synthetic child did not become ready')
            time.sleep(0.01)

    def test_exited_parent_writer_is_stopped_before_directory_cleanup(self):
        sentinel = self.launch('import time;time.sleep(60)')
        with tempfile.TemporaryDirectory(dir=self.root) as temporary:
            scratch = Path(temporary)
            clone = scratch / 'plugins-clone' / '.git'
            clone.mkdir(parents=True)
            marker = clone / 'writer'
            writer = ('import pathlib,time; p=pathlib.Path(' + repr(str(marker)) + ');'
                      '\nwhile True:\n p.write_text(str(time.monotonic()));time.sleep(0.01)')
            parent = self.launch('import subprocess,sys;subprocess.Popen([sys.executable,"-I","-c",' + repr(writer) + '])')
            parent.wait(timeout=5)
            self.wait_file(marker)
            try:
                harness.stop_process(parent)
                # A Linux zombie-only group is quiescent; a live writer is not.
                if sys.platform == 'linux':
                    self.assertFalse(harness.linux_group_has_live_members(parent.pid, time.monotonic() + 5))
                elif sys.platform == 'darwin':
                    self.assertFalse(harness.darwin_group_has_live_members(parent.pid, time.monotonic() + 5))
                else:
                    with self.assertRaises(ProcessLookupError):
                        os.killpg(parent.pid, 0)
            finally:
                # Preserve the owned-group safety net even during the old-helper RED probe.
                SAFE_STOP(parent)
            snapshot = marker.read_bytes()
            time.sleep(0.05)
            self.assertEqual(marker.read_bytes(), snapshot)
            self.assertIsNone(sentinel.poll())
            with patch.object(harness.os, 'killpg', side_effect=AssertionError('repeated cleanup signaled a group')):
                harness.stop_process(parent)
        self.assertFalse(scratch.exists())

    def test_graceful_parent_exits_and_pipes_close(self):
        ready, stopped = self.root / 'ready', self.root / 'stopped'
        source = ('import pathlib,signal,sys,time\n'
                  'def stop(*_):\n pathlib.Path(' + repr(str(stopped)) + ').write_text("stopped");sys.exit(0)\n'
                  'signal.signal(signal.SIGTERM,stop)\npathlib.Path(' + repr(str(ready)) + ').touch()\ntime.sleep(60)')
        child = self.launch(source)
        self.wait_file(ready)
        harness.stop_process(child)
        self.assertEqual(child.returncode, 0)
        self.assertEqual(stopped.read_text(), 'stopped')
        self.assertTrue(child.stdin.closed)
        self.assertTrue(child.stdout.closed)

    def test_term_ignoring_parent_is_forcibly_reaped(self):
        ready = self.root / 'ready'
        child = self.launch('import pathlib,signal,time;signal.signal(signal.SIGTERM,signal.SIG_IGN);'
                            'pathlib.Path(' + repr(str(ready)) + ').touch();time.sleep(60)')
        self.wait_file(ready)
        harness.stop_process(child)
        self.assertEqual(child.returncode, -signal.SIGKILL)
        with self.assertRaises(ProcessLookupError):
            os.kill(child.pid, 0)

    def test_unowned_child_never_signals_inherited_group(self):
        child = subprocess.Popen([sys.executable, '-I', '-c', 'import time;time.sleep(60)'],
                                 stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.addCleanup(harness.stop_process, child)
        with patch.object(harness.os, 'killpg', side_effect=AssertionError('unowned group')):
            harness.stop_process(child)
        self.assertIsNotNone(child.returncode)


class LinuxGroupMetadataTests(unittest.TestCase):
    def check_group(self, records, kill_error=None):
        class Entry:
            def __init__(self, name, raw): self.name, self.raw = name, raw
            def __truediv__(self, _): return self
            def read_text(self):
                if isinstance(self.raw, BaseException): raise self.raw
                return self.raw
        entries = [Entry(str(i + 1), value) for i, value in enumerate(records)]
        with patch.object(harness.Path, 'iterdir', return_value=entries), patch.object(harness.os, 'killpg', side_effect=kill_error, create=True):
            return harness.linux_group_has_live_members(123, time.monotonic() + 5)

    def test_zombies_are_quiescent_but_live_or_unknown_states_are_not(self):
        self.assertFalse(self.check_group(['1 (writer name) Z 1 123 0', '2 (other) S 1 999 0']))
        self.assertTrue(self.check_group(['1 (writer) S 1 123 0']))
        self.assertTrue(self.check_group(['1 (writer) D 1 123 0']))
        self.assertTrue(self.check_group(['1 (writer) ? 1 123 0']))

    def test_unreadable_or_malformed_metadata_fails_closed(self):
        with self.assertRaises(PermissionError): self.check_group([PermissionError('synthetic')])
        for malformed in ('incomplete', 'Z 1 123', '999 (writer) Z 1 123'):
            with self.assertRaises(AssertionError): self.check_group([malformed])
        with self.assertRaises(AssertionError): self.check_group([])

    def test_exit_race_requires_group_disappearance_when_no_member_was_seen(self):
        self.assertFalse(self.check_group([FileNotFoundError()], ProcessLookupError()))
        with self.assertRaises(AssertionError): self.check_group([FileNotFoundError()])


class DarwinGroupMetadataTests(unittest.TestCase):
    def test_zombie_modifiers_and_absence_are_quiescent_live_and_unknown_are_not(self):
        for raw, live in [(b'123 Z\n999 S\n', False), (b'123 Z+\n', False),
                          (b'999 S\n', False), (b'123 S\n', True), (b'123 ?\n', True)]:
            with self.subTest(raw=raw), patch.object(harness, 'darwin_group_snapshot', return_value=raw):
                self.assertEqual(harness.darwin_group_has_live_members(123, time.monotonic()+5), live)
        for raw in (b'', b'malformed\n', b'123 Z extra\n', b'123 Z\ninvalid\n'):
            with self.subTest(raw=raw), patch.object(harness, 'darwin_group_snapshot', return_value=raw):
                with self.assertRaises(AssertionError):
                    harness.darwin_group_has_live_members(123, time.monotonic()+5)

    def test_initial_and_probe_eperm_require_verified_quiescence(self):
        class Exited:
            stdin = stdout = None
            def __init__(self): self._conformance_group = 123
            def poll(self): return 0
        for initial in (False, True):
            for raw in (b'123 Z\n', b'999 S\n', b'123 S\n', b'123 ?\n', b'invalid\n', OSError('unreadable')):
                child = Exited()
                calls = [PermissionError()] if initial else [None, PermissionError()]
                with self.subTest(initial=initial, raw=raw), patch.object(harness.sys, 'platform', 'darwin'), \
                     patch.object(harness.os, 'killpg', side_effect=calls, create=True), \
                     patch.object(harness, 'darwin_group_snapshot', **({'side_effect': raw} if isinstance(raw, Exception) else {'return_value': raw})):
                    if raw in (b'123 Z\n', b'999 S\n'):
                        harness.stop_process(child)
                        self.assertTrue(child._conformance_cleaned)
                    else:
                        with self.assertRaises((PermissionError, AssertionError, OSError)):
                            harness.stop_process(child)
                        self.assertFalse(child.__dict__.get('_conformance_cleaned', False))

    def test_snapshot_limits_deadline_and_read_errors_reap_inspector(self):
        class Pipe:
            closed = False
            def fileno(self): return 42
            def close(self): self.closed = True
        class Inspector:
            def __init__(self): self.stdout, self.killed, self.waited = Pipe(), False, False
            def poll(self): return None
            def kill(self): self.killed = True
            def wait(self, timeout): self.waited = True; return 0
        class Selector:
            def __enter__(self): return self
            def __exit__(self, *_): pass
            def register(self, *_): pass
            def select(self, _): return [True]
        for scenario in ('deadline', 'limit', 'read_error'):
            child = Inspector()
            reads = OSError('read failure') if scenario == 'read_error' else [b'x'*65536]*17
            deadline = 9 if scenario == 'deadline' else 11
            with self.subTest(scenario=scenario), patch.object(harness.subprocess, 'Popen', return_value=child) as launch, \
                 patch.object(harness.selectors, 'DefaultSelector', return_value=Selector()), \
                 patch.object(harness.time, 'monotonic', return_value=10), \
                 patch.object(harness.os, 'read', side_effect=reads):
                with self.assertRaises((AssertionError, OSError)):
                    harness.darwin_group_snapshot(deadline)
                self.assertEqual(launch.call_args.args[0], ['/bin/ps', '-A', '-o', 'pgid=,state='])
                self.assertTrue(child.killed and child.waited and child.stdout.closed)


if __name__ == '__main__':
    unittest.main()
