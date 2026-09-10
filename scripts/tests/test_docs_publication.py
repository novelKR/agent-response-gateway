"""Regress provenance validation and the narrow, caller-owned Pages contract."""
from __future__ import annotations

import importlib.util
import json
import hashlib
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/check_docs_publication.py"
spec = importlib.util.spec_from_file_location("docs_publication", SCRIPT)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
COMMIT = "a" * 40


class DocsPublicationTests(unittest.TestCase):
    def manifest(self):
        return {"schema": "gateway-docs-build/v1", "source_commit": COMMIT,
                "working_tree": False, "files": {"index.html": "b" * 64}}

    def test_clean_matching_manifest(self):
        manifest = self.manifest()
        original = json.dumps(manifest)
        module.validate_manifest(manifest, COMMIT)
        self.assertEqual(json.dumps(manifest), original)

    def test_expected_commit_must_be_full_sha(self):
        for value in ("", "main", "a" * 7, "a" * 39, "g" * 40, "a" * 41, None):
            with self.subTest(value=value), self.assertRaises(ValueError):
                module.validate_manifest(self.manifest(), value)

    def test_wrong_source_commit_is_rejected(self):
        for value in (None, "main", "b" * 40):
            manifest = self.manifest()
            manifest["source_commit"] = value
            with self.subTest(value=value), self.assertRaises(ValueError):
                module.validate_manifest(manifest, COMMIT)

    def test_dirty_missing_and_non_boolean_clean_values_are_rejected(self):
        for value in (True, None, 0, "false", "clean", []):
            manifest = self.manifest()
            manifest["working_tree"] = value
            with self.subTest(value=value), self.assertRaises(ValueError):
                module.validate_manifest(manifest, COMMIT)
        manifest = self.manifest()
        del manifest["working_tree"]
        with self.assertRaises(ValueError):
            module.validate_manifest(manifest, COMMIT)

    def test_wrong_schema_and_inventory_fail_closed(self):
        for manifest in (None, [], {}, {**self.manifest(), "schema": "other/v1"},
                         {**self.manifest(), "files": {}}, {**self.manifest(), "files": []}):
            with self.subTest(manifest=manifest), self.assertRaises(ValueError):
                module.validate_manifest(manifest, COMMIT)

    def invoke(self, path, commit=COMMIT):
        return subprocess.run([sys.executable, "-B", str(SCRIPT), "--manifest", str(path),
                               "--commit", commit], capture_output=True, text=True, check=False)

    def test_cli_success_does_not_rewrite_manifest(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "manifest.json"
            raw = json.dumps(self.manifest()).encode()
            path.write_bytes(raw)
            result = self.invoke(path)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(path.read_bytes(), raw)
            self.assertIn("provenance passed", result.stdout)

    def test_cli_rejects_missing_malformed_and_dirty_manifests(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "manifest.json"
            self.assertNotEqual(self.invoke(path).returncode, 0)
            for raw in (b"{", b"\xff", json.dumps({**self.manifest(), "working_tree": True}).encode()):
                path.write_bytes(raw)
                result = self.invoke(path)
                self.assertNotEqual(result.returncode, 0)
                self.assertNotIn("provenance passed", result.stdout)
                self.assertEqual(path.read_bytes(), raw)


class DocsPagesWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.ci = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        cls.lock = json.loads((ROOT / ".github/docs-pages-deploy.lock.json").read_text(encoding="utf-8"))
        cls.reusable = (ROOT / "scripts/tests/fixtures/docs-actions-reusable.yml").read_text(encoding="utf-8")
        if (cls.lock.get("repository") != "novelKR/docs-actions" or
                cls.lock.get("workflow") != ".github/workflows/reusable-pages-deploy.yml" or
                not re.fullmatch(r"[0-9a-f]{40}", cls.lock.get("commit", "")) or
                hashlib.sha256(cls.reusable.encode()).hexdigest() != cls.lock.get("workflow_sha256")):
            raise ValueError("Central workflow lock or verified snapshot has drifted")

    def job(self, name):
        # This is a source-contract regression for the repository's YAML layout,
        # not a general YAML parser or a substitute for GitHub's workflow validation.
        match = re.search(r"^  " + re.escape(name) + r":\n(.*?)(?=^  [\w-]+:|\Z)", self.ci, re.M | re.S)
        self.assertIsNotNone(match, name)
        return match[1]

    def test_pr_checks_are_read_only_and_do_not_use_privileged_events(self):
        header = self.ci.split("jobs:\n", 1)[0]
        self.assertIn("permissions:\n  contents: read\n", header)
        self.assertNotIn("write", header)
        self.assertIn("permissions:\n      contents: read\n", self.job("docs"))
        self.assertNotIn("pages: write", self.job("docs"))
        self.assertNotIn("id-token: write", self.job("docs"))
        self.assertNotIn("pull_request_target:", self.ci)
        self.assertNotIn("workflow_run:", self.ci)
        self.assertNotIn("paths:", header)
        self.assertNotIn("paths-ignore:", header)

    def test_publication_is_after_the_complete_merge_gate_and_main_only(self):
        self.assertIn("docs", self.job("ci-required").split("needs:", 1)[1].split("\n", 1)[0])
        deploy = self.job("docs-pages")
        self.assertIn("needs: ci-required\n", deploy)
        self.assertIn("github.ref == 'refs/heads/main'", deploy)
        self.assertIn("github.event_name == 'push' || github.event_name == 'workflow_dispatch'", deploy)
        self.assertNotIn("always()", deploy)
        self.assertIn("uses: " + self.lock["repository"] + "/" + self.lock["workflow"] + "@" + self.lock["commit"], deploy)
        self.assertIn("artifact-name: github-pages", deploy)
        self.assertIn("publication-branch: main", deploy)
        self.assertIn("pages: write", deploy)
        self.assertIn("id-token: write", deploy)
        self.assertNotIn("contents: write", deploy)

    def test_upload_reuses_the_checked_output_and_is_main_only(self):
        docs = self.job("docs")
        self.assertEqual(docs.count("npm run build --prefix docs-site"), 1)
        check = docs.index("docs-site/scripts/check-output.py")
        provenance = docs.index('scripts/check_docs_publication.py --commit "$GITHUB_SHA"')
        upload = docs.index("uses: actions/upload-pages-artifact@")
        self.assertLess(check, provenance)
        self.assertLess(provenance, upload)
        self.assertEqual(docs.count("github.ref == 'refs/heads/main'"), 2)
        self.assertEqual(docs.count("github.event_name == 'push' || github.event_name == 'workflow_dispatch'"), 2)
        self.assertEqual(docs.count("path: .local/docs-site/dist/"), 2)
        self.assertIn("name: github-pages", docs[upload:])
        self.assertIn("retention-days: 14", docs[upload:])

    def test_reusable_deployment_has_no_checkout_build_or_cross_run_input(self):
        self.assertIn("workflow_call:", self.reusable)
        self.assertIn("name: github-pages", self.reusable)
        self.assertIn("github.ref == format('refs/heads/{0}', inputs.publication-branch)", self.reusable)
        self.assertIn("github.event_name == 'push' || github.event_name == 'workflow_dispatch'", self.reusable)
        self.assertIn("artifact_name: ${{ inputs.artifact-name }}", self.reusable)
        self.assertIn("default: main", self.reusable)
        for forbidden in ("actions/checkout@", "secrets: inherit", "contents: write", "workflow_run:",
                          "run-id:", "repository:", "      - run:"):
            self.assertNotIn(forbidden, self.reusable)
        actions = re.findall(r"uses: ([^\s]+)", self.reusable)
        self.assertEqual(len(actions), 1)
        self.assertRegex(actions[0], r"^actions/deploy-pages@[0-9a-f]{40}$")
        self.assertRegex(self.job("docs"), r"uses: actions/upload-pages-artifact@[0-9a-f]{40}")

    def test_main_deployments_are_not_cancelled_by_prs_or_new_pushes(self):
        self.assertIn("group: ci-${{ github.event.pull_request.number || github.ref }}", self.ci)
        self.assertIn("cancel-in-progress: ${{ github.event_name == 'pull_request' }}", self.ci)
        self.assertIn("group: github-pages\n      cancel-in-progress: false", self.reusable)


if __name__ == "__main__":
    unittest.main()
