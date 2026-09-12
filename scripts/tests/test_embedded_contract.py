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
    def test_codec_identity_versions_permissions_and_readiness_are_bound(self):
        value=manifest();value['schema']='gateway-embedded-manifest/v6'
        codec={'id':'reference-codec','version':'1.0.0','package_sha256':'a'*64,'executable_sha256':'b'*64,'protocol':'gateway-api-codec/v1','replay_versions':[1],'permissions':['read_model_payload','transform_model_protocol']}
        value['configuration']['routes'][0]['api_codec']=codec
        def stamp(v):v['configuration_sha256']=hashlib.sha256(json.dumps(v['configuration'],ensure_ascii=False,sort_keys=True,separators=(',',':')).encode()).hexdigest()
        stamp(value);contract.validate_manifest(value)
        r=ready(value);r['schema']='gateway-ready/v6';contract.parse_ready_line(json.dumps(r)+'\n',value)
        for field,wrong in [('protocol','gateway-api-codec/v2'),('replay_versions',[2]),('permissions',['read_credentials']),('package_sha256','wrong')]:
            changed=copy.deepcopy(value);changed['configuration']['routes'][0]['api_codec'][field]=wrong;stamp(changed)
            with self.subTest(field=field),self.assertRaises(ValueError):contract.validate_manifest(changed)
        changed=copy.deepcopy(value);changed['configuration']['routes'][0]['api_codec']['package_sha256']='c'*64
        with self.assertRaisesRegex(ValueError,'digest'):contract.validate_manifest(changed)
        changed=copy.deepcopy(value);del changed['configuration']['routes'][0]['api_codec'];stamp(changed)
        with self.assertRaises(ValueError):contract.validate_manifest(changed)

    def test_profile_pack_manifest_binds_package_imports_and_optional_extensions(self):
        value = manifest()
        value['schema'] = 'gateway-embedded-manifest/v5'
        package = {'schema':'gateway-profile-pack/v1','id':'synthetic','version':'1.0.0','capabilities':{'functions':{}},'policies':{},'evidence':[],'notices':{'LICENSE':'synthetic'}}
        sha = hashlib.sha256((json.dumps(package,ensure_ascii=False,sort_keys=True,separators=(',',':'))+'\n').encode()).hexdigest()
        entry = {'id':'synthetic','version':'1.0.0','package_sha256':sha}
        value['configuration']['profile_packs'] = {'schema':'gateway-profile-pack-configuration/v1','activation':{'schema':'gateway-profile-pack-lock/v1','generation':1,'packs':[entry]},'packages':{'synthetic':package},'evidence_status':'publisher_claims_not_attestation','capability_imports':{'local':{'pack':'synthetic','export':'functions','provider':'host','upstream_model':'model'}},'policy_imports':{}}
        value['configuration']['routes'][0].update(capability_profile={'id':'local'},profile_packs={'capability':{'pack':'synthetic','version':'1.0.0','package_sha256':sha,'export':'functions'},'policy':None})
        def stamp(v):
            v['configuration_sha256'] = hashlib.sha256(json.dumps(v['configuration'],ensure_ascii=False,sort_keys=True,separators=(',',':')).encode()).hexdigest()
        stamp(value)
        contract.validate_manifest(value)
        for mutation in ('package', 'export', 'route', 'claims', 'unknown', 'duplicate'):
            changed = copy.deepcopy(value)
            packs = changed['configuration']['profile_packs']
            if mutation == 'package': packs['packages']['synthetic']['version'] = '2.0.0'
            elif mutation == 'export': packs['capability_imports']['local']['export'] = 'unknown'
            elif mutation == 'route': changed['configuration']['routes'][0]['profile_packs']['capability']['package_sha256'] = '0'*64
            elif mutation == 'claims': packs['evidence_status'] = 'attested'
            elif mutation == 'unknown': packs['auto_download'] = True
            else: packs['activation']['packs'].append(copy.deepcopy(entry))
            stamp(changed)
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                contract.validate_manifest(changed)
        for managed in (False,True):
            current = copy.deepcopy(value)
            if managed: current['configuration'].update(continuation={},replay_versions={'read':[1,2],'write':2})
            stamp(current)
            for recorder in (False, True):
                config = {'gateway':current,'extensions':{}}
                if recorder: config.update(usage_contract='gateway-usage-event/v1',usage_profiles=['responses/v1','chat/v1','messages/v1']+(['gemini_interactions/v1','deepseek/v1'] if managed else []))
                extended = {'schema':'gateway-extended-manifest/v5','configuration':config,'execution_sha256':hashlib.sha256(json.dumps(config,ensure_ascii=False,sort_keys=True,separators=(',',':')).encode()).hexdigest()}
                frame = {**ready(current),'schema':'gateway-extended-ready/v5','manifest_schema':extended['schema'],'execution_sha256':extended['execution_sha256']}
                self.assertEqual(contract.parse_extended_ready_line(json.dumps(frame)+'\n',extended),frame)

    def test_checked_policy_manifest_and_readiness_bind_selection_and_replay(self):
        def digest(value):
            value["configuration_sha256"] = hashlib.sha256(json.dumps(value["configuration"], ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        for managed in (False, True):
            value = manifest()
            value["schema"] = "gateway-embedded-manifest/v4"
            binding = {"id":"tools","policy":{"version":1,"tools":{"custom_input":"function_json","namespaces":"flatten","grammar":"registered_output_validation"}},"provider_support":{"function_tools":"native"},"admission":"checked","on_unsupported":"reject","contract":"gateway-tool-compatibility/v1"}
            value["configuration"]["routes"][0]["compatibility"] = binding
            if managed:
                value["configuration"].update(continuation={"store_id":"synthetic"},replay_versions={"read":[1,2],"write":2})
            digest(value)
            self.assertEqual(contract.validate_manifest(value), value)
            current = ready(value)
            current["schema"] = "gateway-ready/v4"
            self.assertEqual(contract.parse_ready_line(json.dumps(current)+"\n", value), current)
            changed = copy.deepcopy(value)
            changed["configuration"]["routes"][0]["compatibility"]["id"] = "other"
            with self.assertRaisesRegex(ValueError,"digest"):
                contract.validate_manifest(changed)
            for mutation in ("version","rule","contract","missing"):
                changed = copy.deepcopy(value)
                b = changed["configuration"]["routes"][0]["compatibility"]
                if mutation == "version": b["policy"]["version"] = 2
                elif mutation == "rule": b["policy"]["tools"]["grammar"] = "arbitrary"
                elif mutation == "contract": b["on_unsupported"] = "drop"
                else: del changed["configuration"]["routes"][0]["compatibility"]
                digest(changed)
                with self.assertRaises(ValueError):
                    contract.validate_manifest(changed)
            for recorder in (False,True):
                config = {"gateway":value,"extensions":{}}
                if recorder:
                    config.update(usage_contract="gateway-usage-event/v1",usage_profiles=["responses/v1","chat/v1","messages/v1"]+(["gemini_interactions/v1","deepseek/v1"] if managed else []))
                outer = {"schema":"gateway-extended-manifest/v4","configuration":config,"execution_sha256":hashlib.sha256(json.dumps(config,ensure_ascii=False,sort_keys=True,separators=(",",":")).encode()).hexdigest()}
                contract.validate_extended_manifest(outer)
                r = {**current,"schema":"gateway-extended-ready/v4","manifest_schema":outer["schema"],"execution_sha256":outer["execution_sha256"]}
                self.assertEqual(contract.parse_extended_ready_line(json.dumps(r)+"\n",outer),r)

    def test_managed_manifest_requires_exact_replay_versions_and_matching_readiness(self):
        value = manifest()
        value["schema"] = "gateway-embedded-manifest/v3"
        value["configuration"].update(continuation={"store_id": "synthetic"}, replay_versions={"read": [1, 2], "write": 2})
        value["configuration_sha256"] = hashlib.sha256(json.dumps(value["configuration"], ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        self.assertEqual(contract.validate_manifest(value), value)
        current = ready(value)
        current["schema"] = "gateway-ready/v3"
        self.assertEqual(contract.parse_ready_line(json.dumps(current) + "\n", value), current)
        for versions in ({"read": [2], "write": 2}, {"read": [1, 2], "write": 1}, {"read": [1, 2, 3], "write": 3}):
            changed = copy.deepcopy(value)
            changed["configuration"]["replay_versions"] = versions
            with self.assertRaisesRegex(ValueError, "replay"):
                contract.validate_manifest(changed)
        current["schema"] = "gateway-ready/v2"
        with self.assertRaises(ValueError):
            contract.parse_ready_line(json.dumps(current) + "\n", value)

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
