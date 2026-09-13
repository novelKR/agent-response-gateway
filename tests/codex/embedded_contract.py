"""Synthetic host-side checks for the versioned gateway child contract.

This fixture is not a consumer runtime, credential manager or executable verifier.
"""
import hashlib
import ipaddress
import json
import queue
import subprocess
import threading
from urllib.parse import urlsplit

MANIFEST_SCHEMA = "gateway-embedded-manifest/v1"
READY_SCHEMA = "gateway-ready/v1"
MAX_READY_BYTES = 65536


def require(condition, message):
    if not condition:
        raise ValueError(message)


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "Duplicate contract key")
        result[key] = value
    return result


def load_json(raw):
    try:
        return json.loads(raw, object_pairs_hook=unique_object)
    except (ValueError, TypeError):
        raise ValueError("Invalid contract JSON") from None


def validate_manifest(value):
    require(isinstance(value, dict) and set(value) == {"schema", "package", "client_api", "lifecycle", "configuration", "configuration_sha256"}, "Invalid manifest shape")
    require(value["schema"] in {MANIFEST_SCHEMA,"gateway-embedded-manifest/v2","gateway-embedded-manifest/v3","gateway-embedded-manifest/v4","gateway-embedded-manifest/v5","gateway-embedded-manifest/v6","gateway-embedded-manifest/v7"} and value["client_api"] == "responses" and value["lifecycle"] == "host-supervised-process/v1", "Unsupported manifest contract")
    package = value["package"]
    require(isinstance(package, dict) and set(package) == {"name", "version"} and package["name"] == "agent-response-gateway" and isinstance(package["version"], str) and bool(package["version"]), "Invalid package identity")
    configuration = value["configuration"]
    version = value["schema"].rsplit("/", 1)[1]
    managed = version == "v3" or (version in {"v4","v5","v6","v7"} and "continuation" in configuration)
    require(isinstance(configuration, dict) and set(configuration) == ({"listen", "source_url", "local_token_env", "upstream_credential_references", "limits", "routes"} | ({"continuation", "replay_versions"} if managed else {"continuation"} if value["schema"].endswith("/v2") else set()) | ({"profile_packs"} if version == "v5" or (version == "v6" and "profile_packs" in configuration) else set())), "Invalid configuration projection")
    if managed:
        require(configuration["replay_versions"] == {"read": [1, 2], "write": 2}, "Unsupported replay contract")
    if version in {"v5","v6"} and "profile_packs" in configuration:
        validate_profile_packs(configuration)
    if version in {"v4", "v5", "v6", "v7"}:
        selected = [r["compatibility"] for r in configuration["routes"] if "compatibility" in r]
        require(bool(selected) or version in {"v5","v6","v7"}, "Compatibility selection missing")
        for binding in selected:
            require(set(binding) == {"id","policy","admission","on_unsupported","provider_support","contract"}, "Invalid compatibility projection")
            require(binding["admission"] == "checked" and binding["on_unsupported"] == "reject" and binding["contract"] == "gateway-tool-compatibility/v1", "Unsupported compatibility contract")
            policy = binding["policy"]
            require(set(policy) == {"version","tools"} and type(policy["version"]) is int and policy["version"] == 1, "Unsupported policy version")
            require(set(policy["tools"]) == {"custom_input","namespaces","grammar"}, "Invalid tool policy")
            for key, options in {"custom_input":{None,"preserve","function_json"},"namespaces":{None,"preserve","flatten"},"grammar":{None,"preserve","registered_output_validation"}}.items():
                require(policy["tools"][key] in options, "Unsupported tool policy")
    if version == "v7":
        bindings=[r["editing"] for r in configuration["routes"] if "editing" in r]
        require(bool(bindings), "Editing selection missing")
        for b in bindings:
            require(set(b)=={"id","contract","policy"} and isinstance(b["id"], str) and b["contract"]=="gateway-editing-policy/v1", "Invalid editing projection")
            require(b["policy"]=={"version":1,"client_contract":"codex-direct-custom/v1","representation":"context-lines/v1","patch_dialect":"codex-patch/1","normalization":"none"}, "Unsupported editing policy")
    if version == "v6":
        selected=[route['api_codec'] for route in configuration['routes'] if 'api_codec' in route]
        require(bool(selected), 'Codec selection missing')
        for codec in selected:
            require(set(codec)=={'id','version','package_sha256','executable_sha256','protocol','replay_versions','permissions'}, 'Invalid codec binding')
            require(codec['protocol']=='gateway-api-codec/v1' and codec['replay_versions']==[1] and codec['permissions']==['read_model_payload','transform_model_protocol'], 'Unsupported codec contract')
            require(all(isinstance(codec[k],str) and len(codec[k])==64 and all(c in '0123456789abcdef' for c in codec[k]) for k in ('package_sha256','executable_sha256')), 'Invalid codec digest')
    # Integers remain arbitrary precision in both Python and the Rust projection.
    raw = json.dumps(configuration, ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()
    require(hashlib.sha256(raw).hexdigest() == value["configuration_sha256"], "Configuration digest mismatch")
    return value


def validate_profile_packs(configuration):
    packs = configuration["profile_packs"]
    require(isinstance(packs, dict) and set(packs) == {"schema", "activation", "packages", "evidence_status", "capability_imports", "policy_imports"}, "Invalid profile pack projection")
    require(packs["schema"] == "gateway-profile-pack-configuration/v1" and packs["evidence_status"] == "publisher_claims_not_attestation", "Unsupported profile pack contract")
    activation = packs["activation"]
    require(set(activation) == {"schema", "generation", "packs"} and activation["schema"] == "gateway-profile-pack-lock/v1" and type(activation["generation"]) is int and 0 <= activation["generation"] < 2**64, "Invalid profile pack activation")
    entries = activation["packs"]
    require(isinstance(entries, list) and len(entries) <= 16 and all(isinstance(e, dict) and set(e) == {"id", "version", "package_sha256"} for e in entries), "Invalid pack entries")
    require([e["id"] for e in entries] == sorted(set(e["id"] for e in entries)) and set(packs["packages"]) == {e["id"] for e in entries}, "Profile pack inventory mismatch")
    entries = {e["id"]: e for e in entries}
    for name, package in packs["packages"].items():
        entry = entries[name]
        require(set(package) == {"schema", "id", "version", "capabilities", "policies", "evidence", "notices"} and package["schema"] == "gateway-profile-pack/v1" and package["id"] == name and package["version"] == entry["version"], "Invalid profile pack package")
        raw = json.dumps(package, ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False).encode() + b"\n"
        require(hashlib.sha256(raw).hexdigest() == entry["package_sha256"], "Profile pack bytes mismatch")
    for kind, exports in (("capability_imports", "capabilities"), ("policy_imports", "policies")):
        for binding in packs[kind].values():
            require(set(binding) == ({"pack", "export", "provider", "upstream_model"} if kind == "capability_imports" else {"pack", "export"}), "Invalid profile pack import")
            require(binding["pack"] in entries and binding["export"] in packs["packages"][binding["pack"]][exports], "Unresolved profile pack import")
    for route in configuration["routes"]:
        expected = {}
        for kind, imports, name in (("capability", "capability_imports", route["capability_profile"]["id"]), ("policy", "policy_imports", route.get("compatibility", {}).get("id"))):
            binding = packs[imports].get(name)
            expected[kind] = None if binding is None else {"pack": binding["pack"], "export": binding["export"], "version": entries[binding["pack"]]["version"], "package_sha256": entries[binding["pack"]]["package_sha256"]}
        require(route.get("profile_packs") == (expected if any(expected.values()) else None), "Route pack binding mismatch")


def inspect_manifest(binary, config, env, extra_args=()):
    result = subprocess.run([str(binary), "manifest", "--config", str(config), *extra_args], env=env, capture_output=True, timeout=10, check=False)
    require(result.returncode == 0 and not result.stderr and len(result.stdout.splitlines()) == 1, "Manifest inspection failed")
    value=load_json(result.stdout)
    return validate_extended_manifest(value) if value["schema"].startswith("gateway-extended") else validate_manifest(value)


def parse_ready_line(line, manifest):
    require(isinstance(line, str) and line.endswith("\n") and len(line.encode()) <= MAX_READY_BYTES, "Invalid readiness frame")
    ready = load_json(line)
    require(isinstance(ready, dict) and set(ready) == {"schema", "manifest_schema", "configuration_sha256", "event", "address", "base_url", "version"}, "Invalid readiness shape")
    require(ready["schema"] == ("gateway-ready/v7" if manifest["schema"].endswith("/v7") else "gateway-ready/v6" if manifest["schema"].endswith("/v6") else "gateway-ready/v5" if manifest["schema"].endswith("/v5") else "gateway-ready/v4" if manifest["schema"].endswith("/v4") else "gateway-ready/v3" if manifest["schema"].endswith("/v3") else "gateway-ready/v2" if manifest["schema"].endswith("/v2") else READY_SCHEMA) and ready["manifest_schema"] == manifest["schema"] and ready["event"] == "ready", "Unsupported readiness contract")
    require(ready["configuration_sha256"] == manifest["configuration_sha256"] and ready["version"] == manifest["package"]["version"], "Readiness binding mismatch")
    require(isinstance(ready["address"], str) and ready["base_url"] == "http://" + ready["address"] + "/v1", "Readiness endpoint mismatch")
    try:
        url = urlsplit(ready["base_url"])
        configured = urlsplit("http://" + manifest["configuration"]["listen"])
        valid = ipaddress.ip_address(url.hostname).is_loopback and 0 < url.port <= 65535 and url.username is None and url.password is None and not url.query and not url.fragment
        valid = valid and ipaddress.ip_address(configured.hostname) == ipaddress.ip_address(url.hostname) and configured.port in {0, url.port}
    except (ValueError, TypeError):
        valid = False
    require(valid, "Readiness address is not numeric loopback")
    return ready


def read_ready(process, manifest, timeout=10):
    result = queue.Queue()

    def read():
        try:
            result.put(process.stdout.readline(MAX_READY_BYTES + 1))
        except (OSError, ValueError):
            result.put("")

    threading.Thread(target=read, daemon=True).start()
    try:
        line = result.get(timeout=timeout)
    except queue.Empty:
        raise ValueError("Readiness deadline expired") from None
    require(process.poll() is None, "Gateway exited before host initialization")
    return parse_extended_ready_line(line, manifest) if manifest["schema"].startswith("gateway-extended") else parse_ready_line(line, manifest)


def validate_credential_split(manifest, gateway_env, codex_env, local_key_name, home):
    if manifest["schema"].startswith("gateway-extended"): manifest=manifest["configuration"]["gateway"]
    configuration = manifest["configuration"]
    references = configuration["upstream_credential_references"].values()
    require(all(key in gateway_env for key in references), "Gateway credential reference missing")
    keys = {gateway_env[key] for key in references}
    require(not any(value in keys for value in codex_env.values()), "Upstream credential reached Codex environment")
    require(codex_env.get(local_key_name) == gateway_env.get(configuration["local_token_env"]) and codex_env.get(local_key_name) not in keys, "Local token separation failed")
    require(codex_env.get("CODEX_HOME") == str(home) and codex_env.get("HOME") == str(home), "Codex home is not isolated")


def validate_extended_manifest(value):
    require(isinstance(value,dict) and set(value)=={'schema','configuration','execution_sha256'}, 'Invalid extended manifest')
    require(value['schema'] in {'gateway-extended-manifest/v3','gateway-extended-manifest/v4','gateway-extended-manifest/v5','gateway-extended-manifest/v6','gateway-extended-manifest/v7'}, 'Unsupported extended managed contract')
    configuration=value['configuration']
    require(set(configuration) in ({'gateway','extensions'}, {'gateway','extensions','usage_contract','usage_profiles'}), 'Invalid extended configuration')
    base=validate_manifest(configuration['gateway'])
    require(base['schema']==value['schema'].replace('extended','embedded') or value['schema'].endswith('/v6'), 'Managed base version mismatch')
    if 'usage_contract' in configuration:
        require(configuration['usage_contract']=='gateway-usage-event/v1' and configuration['usage_profiles']==(['responses/v1','chat/v1','messages/v1','gemini_interactions/v1','deepseek/v1'] if 'continuation' in base['configuration'] else ['responses/v1','chat/v1','messages/v1']), 'Unsupported usage profiles')
    raw=json.dumps(configuration,ensure_ascii=False,sort_keys=True,separators=(',',':'),allow_nan=False).encode()
    require(hashlib.sha256(raw).hexdigest()==value['execution_sha256'], 'Execution digest mismatch')
    return value


def parse_extended_ready_line(line, manifest):
    validate_extended_manifest(manifest)
    ready=load_json(line)
    require(ready.get('schema')==manifest['schema'].replace('-manifest/','-ready/') and ready.get('manifest_schema')==manifest['schema'] and ready.get('execution_sha256')==manifest['execution_sha256'], 'Extended readiness binding mismatch')
    base=dict(ready);base.pop('execution_sha256')
    base['manifest_schema']=manifest['configuration']['gateway']['schema'];base['schema']=base['manifest_schema'].replace('embedded-manifest','ready')
    parse_ready_line(json.dumps(base)+'\n',manifest['configuration']['gateway'])
    return ready
