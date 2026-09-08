import importlib.util
from pathlib import Path
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("notice_archive", ROOT / "scripts/archive_notices.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class NoticeArchiveTests(unittest.TestCase):
    def setUp(self):
        state = ROOT / ".local/test-state"
        state.mkdir(parents=True, exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(dir=state)
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bundle = self.root / "bundle"
        self.bundle.mkdir()
        (self.bundle / "NOTICE.txt").write_bytes(b"Synthetic notice\r\n")

    def test_bytes_reproduce_without_owner_time_or_extended_metadata(self):
        first, second = self.root / "a.tar", self.root / "b.tar"
        module.archive_bundle(self.bundle, first)
        (self.bundle / "NOTICE.txt").chmod(0o600)
        module.archive_bundle(self.bundle, second)
        self.assertEqual(first.read_bytes(), second.read_bytes())
        with tarfile.open(first) as archive:
            member = archive.getmembers()[0]
            self.assertEqual((member.uid, member.gid, member.mtime, member.uname, member.gname, member.pax_headers), (0, 0, 0, "", "", {}))
            self.assertEqual(archive.extractfile(member).read(), b"Synthetic notice\r\n")

    def test_existing_archive_is_not_overwritten(self):
        output = self.root / "existing.tar"
        output.write_bytes(b"Existing artifact")
        with self.assertRaises(FileExistsError):
            module.archive_bundle(self.bundle, output)
        self.assertEqual(output.read_bytes(), b"Existing artifact")

    def test_symlink_and_output_inside_bundle_are_rejected(self):
        with self.assertRaises(ValueError):
            module.archive_bundle(self.bundle, self.bundle / "output.tar")
        (self.bundle / "link").symlink_to("NOTICE.txt")
        with self.assertRaises(ValueError):
            module.archive_bundle(self.bundle, self.root / "links.tar")


if __name__ == "__main__":
    unittest.main()
