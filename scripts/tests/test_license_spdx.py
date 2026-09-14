"""Real pinned Cargo/SPDX integration; included in full discovery."""
import json
from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
import license_audit as audit

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
