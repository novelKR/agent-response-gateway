import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("embedded_contract", ROOT / "tests/codex/embedded_contract.py")
contract = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(contract)


def manifest():
    config = {"listen": "127.0.0.1:0", "source_url": None, "local_token_env": "LOCAL", "upstream_credential_references": {"mock": "UPSTREAM"}, "limits": {"n": 9007199254740993123}, "routes": [{"upstream_model": "합성 모델"}]}
    digest = hashlib.sha256(json.dumps(config, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
    return {"schema": contract.MANIFEST_SCHEMA, "package": {"name": "agent-response-gateway", "version": "test"}, "client_api": "responses", "lifecycle": "host-supervised-process/v1", "configuration": config, "configuration_sha256": digest}


def ready(value):
    return {"schema": contract.READY_SCHEMA, "manifest_schema": value["schema"], "configuration_sha256": value["configuration_sha256"], "event": "ready", "version": "test", "address": "127.0.0.1:12345", "base_url": "http://127.0.0.1:12345/v1"}


class EmbeddedContractTests(unittest.TestCase):
    def test_manifest_digest_preserves_unicode_integers_and_rejects_changed_configuration(self):
        value = manifest()
        self.assertEqual(contract.validate_manifest(value), value)
        wrong = copy.deepcopy(value)
        wrong["configuration"]["limits"]["n"] += 1
        with self.assertRaisesRegex(ValueError, "digest"):
            contract.validate_manifest(wrong)
        wrong = {**value, "schema": "unknown"}
        with self.assertRaisesRegex(ValueError, "Unsupported"):
            contract.validate_manifest(wrong)

    def test_readiness_requires_matching_schema_digest_version_and_numeric_loopback(self):
        value = manifest()
        valid = ready(value)
        self.assertEqual(contract.parse_ready_line(json.dumps(valid) + "\n", value), valid)
        for change in [{"configuration_sha256": "0" * 64}, {"version": "other"}, {"schema": "unknown"}, {"address": "localhost:12345", "base_url": "http://localhost:12345/v1"}, {"address": "127.0.0.2:12345", "base_url": "http://127.0.0.2:12345/v1"}, {"address": "0.0.0.0:12345", "base_url": "http://0.0.0.0:12345/v1"}, {"address": "127.0.0.1:0", "base_url": "http://127.0.0.1:0/v1"}, {"base_url": "http://127.0.0.1:12345/other"}]:
            with self.subTest(change=change), self.assertRaises(ValueError):
                contract.parse_ready_line(json.dumps({**valid, **change}) + "\n", value)
        for raw in ["", "{}", "{" + "x" * contract.MAX_READY_BYTES + "}\n", '{"schema":"one","schema":"two"}\n']:
            with self.assertRaises(ValueError):
                contract.parse_ready_line(raw, value)

    def test_credential_split_and_private_home_are_explicit_host_requirements(self):
        value = manifest()
        gateway = {"LOCAL": "local-token", "UPSTREAM": "upstream-value"}
        child = {"TOKEN": "local-token", "CODEX_HOME": "private-home", "HOME": "private-home"}
        contract.validate_credential_split(value, gateway, child, "TOKEN", "private-home")
        for changed in [{**child, "LEAK": "upstream-value"}, {**child, "TOKEN": "other"}, {**child, "HOME": "personal-home"}]:
            with self.assertRaises(ValueError):
                contract.validate_credential_split(value, gateway, changed, "TOKEN", "private-home")

    def test_startup_deadline_allows_owner_to_reap_its_unready_child(self):
        # Retain interpreter loader support on hosted Python distributions, not credentials.
        env = {key: os.environ[key] for key in ("PATH", "LD_LIBRARY_PATH", "DYLD_LIBRARY_PATH", "SYSTEMROOT") if key in os.environ}
        child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"], env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        try:
            with self.assertRaisesRegex(ValueError, "deadline"):
                contract.read_ready(child, manifest(), timeout=0.05)
        finally:
            child.kill()
            child.wait(timeout=5)
            child.stdout.close()
        self.assertIsNotNone(child.returncode)


if __name__ == "__main__":
    unittest.main()
