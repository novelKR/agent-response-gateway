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


if __name__ == '__main__':
    unittest.main()
