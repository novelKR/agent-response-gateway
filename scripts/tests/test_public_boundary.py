"""Synthetic publication-boundary regression tests; no private identifiers."""

from __future__ import annotations

import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "check_public_boundary.py"
MARKER = "synthetic-private-marker-74"


class PublicBoundaryTests(unittest.TestCase):
    def setUp(self):
        state = ROOT / ".local" / "test-state"
        state.mkdir(parents=True, exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(prefix="publication-", dir=state)
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.repo = self.base / "repo"
        self.repo.mkdir()
        self.env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        self.env.update({"GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull})
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.name", "Synthetic Test")
        self.git("config", "user.email", "test@example.invalid")
        self.patterns = self.base / "patterns.json"
        self.patterns.write_text(json.dumps([MARKER]), encoding="utf-8")
        self.write(".gitignore", "/.private/\n/.local/\n")
        self.write("README.md", "Public fixture\n")

    def git(self, *args):
        return subprocess.run(["git", "-C", str(self.repo), *args], check=True, capture_output=True, env=self.env).stdout

    def write(self, relative, content):
        path = self.repo / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
        return path

    def check(self, *args, ok=True):
        result = subprocess.run(
            [sys.executable, "-B", str(SCRIPT), "--root", str(self.repo), "--private-patterns", str(self.patterns), *map(str, args)],
            capture_output=True, text=True, env=self.env,
        )
        self.assertEqual(result.returncode, 0 if ok else 1, result.stderr)
        self.assertNotIn(MARKER, result.stdout + result.stderr)
        self.assertNotIn(str(self.base), result.stdout + result.stderr)
        return result

    def stage(self):
        self.git("add", ".")

    def commit(self):
        self.git("commit", "-q", "-m", "Synthetic fixture")
        return self.git("rev-parse", "HEAD").strip().decode()

    def test_empty_index_fails_but_worktree_supports_initial_files(self):
        self.check(ok=False)
        self.check("--worktree")

    def test_clean_index_and_history(self):
        self.stage()
        self.check()
        self.commit()
        self.check()

    def test_nested_private_history_is_excluded(self):
        original = self.write(".private/archive/original.md", MARKER)
        nested = self.repo / ".private"
        for args in [("init", "-q", "-b", "main"), ("add", "."), ("-c", "user.name=Synthetic Test", "-c", "user.email=test@example.invalid", "commit", "-q", "-m", "Private fixture")]:
            subprocess.run(["git", "-C", str(nested), *args], check=True, capture_output=True, env=self.env)
        self.stage()
        self.commit()
        self.check("--worktree")
        self.assertEqual(original.read_text(), MARKER)
        self.assertNotIn(b".private", self.git("ls-files"))

    def test_forced_private_path_in_index_is_rejected(self):
        self.write(".private/hidden.md", "Opaque fixture")
        self.stage()
        self.git("add", "-f", ".private/hidden.md")
        self.check(ok=False)
        self.check("--worktree", ok=False)

    def test_gitlink_is_rejected(self):
        self.stage()
        commit = self.commit()
        self.git("update-index", "--add", "--cacheinfo", "160000", commit, "nested")
        self.check(ok=False)

    def test_staged_content_is_inspected_even_if_worktree_is_clean(self):
        self.write("README.md", MARKER)
        self.stage()
        self.write("README.md", "Public fixture\n")
        self.check(ok=False)
        self.check("--worktree", ok=False)

    def test_removal_does_not_hide_prior_history(self):
        self.write("README.md", MARKER)
        self.stage()
        self.commit()
        self.write("README.md", "Public fixture\n")
        self.stage()
        self.commit()
        self.check(ok=False)

    def test_replacement_cannot_hide_original_private_history(self):
        self.write("README.md", MARKER)
        self.stage()
        original = self.commit()
        self.write("README.md", "Public fixture\n")
        self.stage()
        tree = self.git("write-tree").strip().decode()
        replacement = self.git("commit-tree", tree, "-m", "Sanitized fixture").strip().decode()
        self.git("replace", original, replacement)
        self.assertEqual(self.git("show", "HEAD:README.md"), b"Public fixture\n")
        self.check(ok=False)

    def test_removed_private_path_remains_rejected_in_history(self):
        self.write(".private/hidden.md", "Opaque fixture")
        self.stage()
        self.git("add", "-f", ".private/hidden.md")
        self.commit()
        self.git("rm", "-q", ".private/hidden.md")
        self.commit()
        self.check(ok=False)

    def test_ref_names_are_checked(self):
        self.stage()
        self.commit()
        self.git("branch", MARKER)
        self.check(ok=False)

    def test_annotated_tag_messages_are_checked(self):
        self.stage()
        self.commit()
        self.git("tag", "-a", "v0.0.1", "-m", MARKER)
        self.check(ok=False)

    def test_shallow_history_is_rejected(self):
        self.stage()
        self.commit()
        shallow = self.base / "shallow"
        self.git("clone", "--quiet", "--depth", "1", self.repo.as_uri(), str(shallow))
        self.repo = shallow
        self.check(ok=False)

    def test_untracked_marker_rejected_only_in_worktree_mode(self):
        self.stage()
        self.write("notes.txt", MARKER)
        self.check()
        self.check("--worktree", ok=False)

    def test_symlink_and_gitmodules_are_rejected(self):
        self.stage()
        link = self.repo / "link"
        link.symlink_to("README.md")
        self.git("add", "link")
        self.check(ok=False)
        self.git("rm", "--cached", "link")
        link.unlink()
        self.write(".gitmodules", "Synthetic metadata")
        self.git("add", ".gitmodules")
        self.check(ok=False)

    def archive(self, name, data=b"Public fixture", kind=tarfile.REGTYPE):
        archive = self.base / "source.tar.gz"
        with tarfile.open(archive, "w:gz") as output:
            entry = tarfile.TarInfo(name)
            entry.type = kind
            if kind == tarfile.REGTYPE:
                entry.size = len(data)
                output.addfile(entry, io.BytesIO(data))
            else:
                entry.linkname = "README.md"
                output.addfile(entry)
        return archive

    def test_clean_archive(self):
        self.stage()
        self.check("--archive", self.archive("source/README.md"))

    def test_archive_reserved_and_unsafe_paths_are_rejected(self):
        self.stage()
        for name in ["source/.private/hidden.md", "source/.PRIVATE/hidden.md", "source/.git/config", "target/program", ".local/output", ".build/output", ".gitmodules", "../escape", "/absolute"]:
            with self.subTest(name=name):
                self.check("--archive", self.archive(name), ok=False)

    def test_archive_links_and_private_content_are_rejected(self):
        self.stage()
        self.check("--archive", self.archive("link", kind=tarfile.SYMTYPE), ok=False)
        self.check("--archive", self.archive("source/README.md", MARKER.encode()), ok=False)

    def test_invalid_marker_file_has_no_details(self):
        self.stage()
        self.patterns.write_text("not valid json", encoding="utf-8")
        self.check(ok=False)

    def test_oversized_archive_member_is_rejected_without_reading_it(self):
        self.stage()
        archive = self.base / "oversized.tar"
        entry = tarfile.TarInfo("large.bin")
        entry.size = 64 * 1024 * 1024 + 1
        # Deliberately omit the body. Python 3.14's addfile requires fileobj
        # for non-empty regular files, so write only the synthetic header.
        archive.write_bytes(entry.tobuf())
        self.check("--archive", archive, ok=False)


if __name__ == "__main__":
    unittest.main()
