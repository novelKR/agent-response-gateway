"""Synthetic source-integrity and real cargo-deny SPDX regression tests."""

from __future__ import annotations

import copy
import io
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
import license_audit as audit


class LicenseFixtures(unittest.TestCase):
    def setUp(self):
        state = ROOT / ".local/test-state"
        state.mkdir(parents=True, exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(prefix="licenses-", dir=state)
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "licensing/texts").mkdir(parents=True)
        (self.root / "LICENSE").write_bytes((ROOT / "LICENSE").read_bytes())
        (self.root / "Cargo.toml").write_text('[package]\nname="agent-response-gateway"\nversion="0.1.0"\nlicense="AGPL-3.0-only"\nedition="2024"\n')
        self.policy = copy.deepcopy(audit.load_policy(ROOT))
        self.policy["supplemental_notices"] = []
        self.save_policy()
        self.cargo_home = self.root / ".local/cargo-home"
        self.packages = []
        self.metadata = {"packages": []}

    def save_policy(self):
        (self.root / "licensing/policy.json").write_bytes(audit.json_bytes(self.policy))

    def add_package(self, name="synthetic-library", license="MIT", files=None):
        data = {"Cargo.toml": f'[package]\nname="{name}"\nversion="1.0.0"\nlicense={json.dumps(license)}\n'.encode(), "src/lib.rs": b"// Synthetic fixture only.\n"}
        data.update(files if files is not None else {"LICENSE": b"Synthetic notice\r\nCopyright fixture authors\r\n"})
        buffer = io.BytesIO()
        with tarfile.open(fileobj=buffer, mode="w:gz") as archive:
            for path, content in data.items():
                info = tarfile.TarInfo(f"{name}-1.0.0/{path}")
                info.size = len(content)
                archive.addfile(info, io.BytesIO(content))
        blob = buffer.getvalue()
        package = {"name": name, "version": "1.0.0", "source": audit.REGISTRY, "checksum": audit.digest(blob)}
        cache = self.cargo_home / "registry/cache/synthetic"
        cache.mkdir(parents=True, exist_ok=True)
        archive_path = cache / f"{name}-1.0.0.crate"
        archive_path.write_bytes(blob)
        source = self.cargo_home / "registry/src/synthetic" / f"{name}-1.0.0"
        for path, content in data.items():
            target = source / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(content)
        self.packages.append(package)
        self.metadata["packages"].append({"name": name, "version": "1.0.0", "source": audit.REGISTRY, "license": license, "manifest_path": str(source / "Cargo.toml")})
        self.save_lock()
        return package, archive_path, source

    def save_lock(self):
        text = 'version = 4\n\n[[package]]\nname="agent-response-gateway"\nversion="0.1.0"\n'
        for p in self.packages:
            text += '\n[[package]]\n' + ''.join(f'{k}={json.dumps(v)}\n' for k, v in p.items())
        (self.root / "Cargo.lock").write_text(text)

    def collect(self):
        return audit.collect(self.root, self.policy, self.metadata, self.cargo_home)

    def stored(self):
        records, blobs = self.collect()
        choices = {audit.package_key(r): ["MIT"] for r in records}
        document = audit.record_document(self.root, self.policy, records, choices)
        for sha, data in blobs.items():
            (self.root / "licensing/texts" / f"{sha}.txt").write_bytes(data)
        (self.root / "licensing/dependencies.json").write_bytes(audit.json_bytes(document))
        (self.root / "licensing/THIRD-PARTY-NOTICES.md").write_bytes(audit.render_notices(document))
        return document, blobs

    def test_root_agpl_is_not_a_global_exception(self):
        self.policy["allowed_licenses"].append("AGPL-3.0-only")
        self.save_policy()
        with self.assertRaisesRegex(audit.AuditError, "globally"):
            audit.load_policy(self.root)

    def test_english_notice_labels_preserve_the_original_notice_bytes(self):
        original = b"Synthetic notice\r\nCopyright fixture authors\r\n"
        self.add_package(files={"LICENSE": original})
        document, blobs = self.stored()
        rendered = audit.render_notices(document)
        self.assertTrue(rendered.startswith(b"# Third-party licenses and notices\n"))
        self.assertIn(b"Declared: `MIT`", rendered)
        self.assertIn(b"Selected: `MIT`", rendered)
        self.assertEqual(list(blobs.values()), [original])
        self.assertEqual((self.root / "licensing/texts" / (audit.digest(original) + ".txt")).read_bytes(), original)

    def test_wrong_root_or_license_text_is_rejected(self):
        (self.root / "LICENSE").write_bytes(b"Synthetic modified license")
        with self.assertRaisesRegex(audit.AuditError, "license text"):
            audit.load_policy(self.root)

    def test_unknown_policy_field_cannot_add_a_hidden_exception(self):
        self.policy["ignore_private"] = True
        self.save_policy()
        with self.assertRaisesRegex(audit.AuditError, "record fields"):
            audit.load_policy(self.root)

    def test_duplicate_json_fields_are_rejected(self):
        (self.root / "licensing/policy.json").write_text('{"schema_version":1,"schema_version":2}')
        with self.assertRaisesRegex(audit.AuditError, "duplicate"):
            audit.load_policy(self.root)

    def test_every_conditional_locked_package_requires_metadata(self):
        self.add_package()
        self.add_package("synthetic-other-target")
        self.metadata["packages"].pop()
        with self.assertRaisesRegex(audit.AuditError, "full lock inventory"):
            self.collect()

    def test_archive_checksum_mismatch_fails(self):
        _, archive, _ = self.add_package()
        archive.write_bytes(archive.read_bytes() + b"changed")
        with self.assertRaisesRegex(audit.AuditError, "archive checksum"):
            self.collect()

    def test_unpacked_source_cannot_change_what_deny_reads(self):
        _, _, source = self.add_package()
        (source / "Cargo.toml").write_text('[package]\nlicense="AGPL-3.0-only"\n')
        with self.assertRaisesRegex(audit.AuditError, "unpacked crate"):
            self.collect()

    def test_missing_archive_fails_without_network_fallback(self):
        _, archive, _ = self.add_package()
        archive.unlink()
        with self.assertRaisesRegex(audit.AuditError, "archive is missing"):
            self.collect()

    def test_missing_notice_is_not_replaced_with_generic_license(self):
        self.add_package(files={})
        with self.assertRaisesRegex(audit.AuditError, "no verified notice"):
            self.collect()

    def test_nested_and_multiple_notices_and_raw_line_endings(self):
        originals = {"LICENSE": b"MIT fixture\r\n", "LICENSE.httprouter": b"BSD fixture\n", "src/once_cell/LICENSE-MIT": b"Nested fixture\n", "AUTHORS": b"Named authors\n"}
        self.add_package(files=originals)
        records, blobs = self.collect()
        self.assertEqual({n["path"] for n in records[0]["notices"]}, set(originals))
        self.assertTrue(all(blobs[audit.digest(data)] == data for data in originals.values()))

    def test_source_module_named_target_is_not_a_build_artifact(self):
        self.add_package(files={"LICENSE": b"Fixture\n", "src/target/parser.rs": b"// public source fixture\n"})
        self.assertEqual(len(self.collect()[0]), 1)

    def test_no_new_foreign_registry_or_local_dependency(self):
        self.add_package()
        self.packages[0]["source"] = "git+https://example.invalid/unreviewed"
        self.save_lock()
        with self.assertRaisesRegex(audit.AuditError, "provenance adapter"):
            self.collect()

    def test_supplement_is_bound_to_archive_vcs_commit_and_bytes(self):
        commit = "a" * 40
        package, _, _ = self.add_package(files={".cargo_vcs_info.json": audit.json_bytes({"git": {"sha1": commit}})})
        data = b"Synthetic upstream notice\n"
        sha = audit.digest(data)
        extra = {"name": package["name"], "version": package["version"], "package_checksum": package["checksum"], "path": "upstream/LICENSE", "url": f"https://example.invalid/source/{commit}/LICENSE", "vcs_commit": commit, "sha256": sha}
        self.policy["supplemental_notices"] = [extra]
        (self.root / "licensing/texts" / f"{sha}.txt").write_bytes(data)
        self.assertEqual(self.collect()[0][0]["notices"][0]["origin"], "pinned_upstream")
        extra["vcs_commit"] = "b" * 40
        with self.assertRaisesRegex(audit.AuditError, "source commit"):
            self.collect()
        extra["vcs_commit"] = commit
        (self.root / "licensing/texts" / f"{sha}.txt").write_bytes(b"different notice")
        with self.assertRaisesRegex(audit.AuditError, "notice hash"):
            self.collect()

    def test_unpinned_supplement_is_rejected(self):
        self.policy["supplemental_notices"] = copy.deepcopy(audit.load_policy(ROOT)["supplemental_notices"])
        self.policy["supplemental_notices"][0]["url"] = "https://example.invalid/main/LICENSE"
        self.save_policy()
        with self.assertRaisesRegex(audit.AuditError, "commit pinned"):
            audit.load_policy(self.root)

    def test_lockfile_change_invalidates_records(self):
        self.add_package()
        document, blobs = self.stored()
        (self.root / "Cargo.lock").write_text((self.root / "Cargo.lock").read_text() + "\n# changed lock\n")
        changed = {**document, "cargo_lock_sha256": audit.digest((self.root / "Cargo.lock").read_bytes())}
        with self.assertRaisesRegex(audit.AuditError, "stale"):
            audit.verify_stored(self.root, changed, blobs)

    def test_notice_alteration_missing_and_generated_drift(self):
        self.add_package()
        document, blobs = self.stored()
        path = self.root / "licensing/texts" / f"{next(iter(blobs))}.txt"
        path.write_bytes(b"changed")
        with self.assertRaises(audit.AuditError):
            audit.verify_stored(self.root, document, blobs)
        path.unlink()
        with self.assertRaises(audit.AuditError):
            audit.verify_stored(self.root, document, blobs)
        path.write_bytes(next(iter(blobs.values())))
        (self.root / "licensing/THIRD-PARTY-NOTICES.md").write_text("changed")
        with self.assertRaisesRegex(audit.AuditError, "generated"):
            audit.verify_stored(self.root, document, blobs)

    def test_invalid_selected_license_does_not_enter_records(self):
        self.add_package()
        records, _ = self.collect()
        with self.assertRaisesRegex(audit.AuditError, "outside policy"):
            audit.record_document(self.root, self.policy, records, {audit.package_key(records[0]): ["AGPL-3.0-only"]})

    def test_bundle_reproduces_without_private_directory_or_local_paths(self):
        self.add_package()
        document, blobs = self.stored()
        first = audit.bundle_files(self.root, self.policy, document, blobs)
        private = self.root / ".private"
        private.mkdir()
        (private / "contract").write_text("synthetic-private-marker")
        second = audit.bundle_files(self.root, self.policy, document, blobs)
        self.assertEqual(first, second)
        contents = b"\n".join(first.values())
        self.assertNotIn(str(self.root).encode(), contents)
        self.assertNotIn(b"synthetic-private-marker", contents)
        self.assertNotIn(b".private", contents)
        manifest = json.loads(first["manifest.json"])
        self.assertTrue(all(audit.digest(first[name]) == sha for name, sha in manifest["files"].items()))

    def test_historical_content_addressed_texts_are_also_checked(self):
        (self.root / "licensing/texts" / ("a" * 64 + ".txt")).write_text("altered historical text")
        with self.assertRaisesRegex(audit.AuditError, "altered"):
            audit.verify_text_store(self.root)

    def test_diagnostics_do_not_expose_input_or_local_path(self):
        (self.root / "licensing/policy.json").write_text('{"synthetic-private-marker": true}')
        result = subprocess.run([sys.executable, "-B", str(ROOT / "scripts/license_audit.py"), "check", "--root", str(self.root)], capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn(str(self.root).encode(), result.stdout + result.stderr)
        self.assertNotIn(b"synthetic-private-marker", result.stdout + result.stderr)


class RealSpdxTests(unittest.TestCase):
    """Exercise the pinned SPDX engine; a missing tool is an unmet prerequisite."""

    @classmethod
    def setUpClass(cls):
        cls.executable = ROOT / ".local/tools/bin/cargo-deny"
        if not cls.executable.is_file():
            raise AssertionError("Prepare cargo-deny 0.20.2 as documented in licensing/README.md")
        state = ROOT / ".local/test-state"
        state.mkdir(parents=True, exist_ok=True)
        cls.temp = tempfile.TemporaryDirectory(prefix="spdx-fixtures-", dir=state)
        cls.addClassCleanup(cls.temp.cleanup)
        cls.root = Path(cls.temp.name)
        (cls.root / "src").mkdir()
        (cls.root / "src/lib.rs").write_text("// Synthetic fixture only.\n")
        cls.policy = audit.load_policy(ROOT)
        cls.expressions = {"choice": "MIT OR Apache-2.0", "weak_choice": "MIT OR LGPL-2.1-or-later", "strong_gpl": "GPL-3.0-only", "strong_agpl": "AGPL-3.0-only", "combined": "MIT AND ISC", "unknown": "MadeUp-License-Example", "with_choice": "Apache-2.0 WITH LLVM-exception OR MIT"}
        manifest = '[package]\nname="agent-response-gateway"\nversion="0.1.0"\nedition="2024"\nlicense="AGPL-3.0-only"\npublish=false\n[workspace]\n[dependencies]\n'
        for name, expression in cls.expressions.items():
            crate = cls.root / name
            crate.mkdir()
            (crate / "src").mkdir()
            (crate / "src/lib.rs").write_text("// Synthetic fixture only.\n")
            (crate / "Cargo.toml").write_text(f'[package]\nname="{name}"\nversion="1.0.0"\nedition="2024"\nlicense={json.dumps(expression)}\npublish=false\n')
            manifest += f'{name} = {{ path = "{name}" }}\n'
        (cls.root / "Cargo.toml").write_text(manifest)
        result = audit.run_process(["cargo", "generate-lockfile", "--offline"], cls.root)
        if result.returncode:
            raise AssertionError("Cannot prepare synthetic Cargo workspace")
        result = audit.run_process(["cargo", "metadata", "--format-version", "1", "--locked", "--offline"], cls.root)
        if result.returncode:
            raise AssertionError("Cannot inspect synthetic Cargo workspace")
        cls.metadata = json.loads(result.stdout)

    def evaluate(self, licenses):
        with tempfile.TemporaryDirectory(dir=self.root) as temp:
            evaluator = audit.Deny(self.root, self.executable, self.metadata, self.policy, Path(temp))
            return evaluator.evaluate({(name, "1.0.0"): licenses.get(name, ["MIT"]) for name in self.expressions})

    def test_real_or_and_with_unknown_and_copyleft_decisions(self):
        results = self.evaluate({"combined": ["MIT", "ISC"]})
        for name in ("choice", "weak_choice", "combined", "with_choice"):
            self.assertTrue(results[(name, "1.0.0")], name)
        for name in ("strong_gpl", "strong_agpl", "unknown"):
            self.assertFalse(results[(name, "1.0.0")], name)

    def test_real_and_rejects_a_missing_condition(self):
        self.assertFalse(self.evaluate({})[("combined", "1.0.0")])

    def test_real_choice_cannot_select_unrelated_license(self):
        self.assertFalse(self.evaluate({"choice": ["BSD-3-Clause"]})[("choice", "1.0.0")])

    def test_implicit_exception_file_is_rejected(self):
        path = self.root / "deny.exceptions.toml"
        path.write_text("exceptions=[]\n")
        try:
            with self.assertRaisesRegex(audit.AuditError, "implicit"):
                self.evaluate({})
        finally:
            path.unlink()


if __name__ == "__main__":
    unittest.main()
