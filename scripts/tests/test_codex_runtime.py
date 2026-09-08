"""Synthetic pinned-runtime integrity tests; never download or execute Codex."""

import hashlib
import importlib.util
import io
from pathlib import Path
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("codex_runtime", ROOT / "scripts/codex_runtime.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class RuntimeIntegrityTests(unittest.TestCase):
    def setUp(self):
        state = ROOT / ".local/test-state"
        state.mkdir(parents=True, exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(dir=state)
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.data = b"Synthetic executable fixture\n"
        self.archive = self.root / "runtime.tar.gz"
        self.lock = {"entrypoint": "bin/codex", "files": {"bin/codex": {"size": len(self.data), "sha256": hashlib.sha256(self.data).hexdigest(), "executable": True}}}
        self.make_archive()

    def make_archive(self, extra=None):
        with tarfile.open(self.archive, "w:gz") as archive:
            item = tarfile.TarInfo("bin/codex")
            item.size = len(self.data)
            archive.addfile(item, io.BytesIO(self.data))
            if extra:
                item = tarfile.TarInfo(extra)
                item.size = 1
                archive.addfile(item, io.BytesIO(b"x"))
        self.lock["archive"] = {"size": self.archive.stat().st_size, "sha256": hashlib.sha256(self.archive.read_bytes()).hexdigest()}

    def test_verified_install_and_recheck(self):
        destination = self.root / "installed"
        binary = module.install_archive(self.archive, destination, self.lock)
        self.assertEqual(binary.read_bytes(), self.data)
        self.assertEqual(binary, module.verify_bundle(destination, self.lock))
        self.assertEqual(binary, module.install_archive(self.archive, destination, self.lock))

    def test_wrong_archive_and_binary_are_rejected(self):
        destination = self.root / "installed"
        self.archive.write_bytes(self.archive.read_bytes() + b"x")
        with self.assertRaises(ValueError):
            module.install_archive(self.archive, destination, self.lock)
        self.assertFalse(destination.exists())
        self.make_archive()
        binary = module.install_archive(self.archive, destination, self.lock)
        binary.write_bytes(b"Tampered executable\n")
        with self.assertRaises(ValueError):
            module.verify_bundle(destination, self.lock)

    def test_unsafe_and_unexpected_members_do_not_install(self):
        for name in ("../escape", "/absolute", "unexpected.txt"):
            with self.subTest(name=name):
                self.make_archive(name)
                with self.assertRaises(ValueError):
                    module.install_archive(self.archive, self.root / "installed", self.lock)
                self.assertFalse((self.root / "installed").exists())

    def test_symlink_and_mode_changes_are_rejected(self):
        destination = self.root / "installed"
        binary = module.install_archive(self.archive, destination, self.lock)
        binary.chmod(0o644)
        with self.assertRaises(ValueError):
            module.verify_bundle(destination, self.lock)
        binary.unlink()
        binary.symlink_to(self.archive)
        with self.assertRaises(ValueError):
            module.verify_bundle(destination, self.lock)


if __name__ == "__main__":
    unittest.main()
