"""Synthetic host-side checks for the versioned gateway child contract.

This fixture is not a consumer runtime, credential manager or executable verifier.
"""
import hashlib
import ipaddress
import json
import queue
import re
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
    require(value["schema"] in {MANIFEST_SCHEMA,"gateway-embedded-manifest/v2","gateway-embedded-manifest/v3","gateway-embedded-manifest/v4","gateway-embedded-manifest/v5","gateway-embedded-manifest/v6","gateway-embedded-manifest/v7","gateway-embedded-manifest/v8","gateway-embedded-manifest/v9","gateway-embedded-manifest/v10"} and value["client_api"] == "responses" and value["lifecycle"] == "host-supervised-process/v1", "Unsupported manifest contract")
    package = value["package"]
    require(isinstance(package, dict) and set(package) == {"name", "version"} and package["name"] == "agent-response-gateway" and isinstance(package["version"], str) and bool(package["version"]), "Invalid package identity")
    configuration = value["configuration"]
    version = value["schema"].rsplit("/", 1)[1]
    managed = version == "v3" or (version in {"v4","v5","v6","v7","v8","v9","v10"} and "continuation" in configuration)
    require(isinstance(configuration, dict) and set(configuration) == ({"listen", "source_url", "local_token_env", "upstream_credential_references", "limits", "routes"} | ({"continuation", "replay_versions"} if managed else {"continuation"} if value["schema"].endswith("/v2") else set()) | ({"profile_packs"} if version == "v5" or (version in {"v6","v7","v8","v9","v10"} and "profile_packs" in configuration) else set())), "Invalid configuration projection")
    if managed:
        expected_replay = {"read": [1, 2, 3], "write_builtin": 2, "write_provider": 3} if version == "v10" else {"read": [1, 2], "write": 2}
        require(configuration["replay_versions"] == expected_replay and all(type(v) is int for v in configuration["replay_versions"]["read"]) and all(type(v) is int for k,v in configuration["replay_versions"].items() if k != "read"), "Unsupported replay contract")
    if version in {"v5","v6","v7","v8","v9","v10"} and "profile_packs" in configuration:
        validate_profile_packs(configuration)
    if version in {"v4", "v5", "v6", "v7", "v8", "v9","v10"}:
        selected = [r["compatibility"] for r in configuration["routes"] if "compatibility" in r]
        require(bool(selected) or version in {"v5","v6","v7","v8","v9","v10"}, "Compatibility selection missing")
        for binding in selected:
            require(set(binding) == {"id","policy","admission","on_unsupported","provider_support","contract"}, "Invalid compatibility projection")
            require(binding["admission"] == "checked" and binding["on_unsupported"] == "reject" and binding["contract"] == "gateway-tool-compatibility/v1", "Unsupported compatibility contract")
            policy = binding["policy"]
            require(set(policy) == {"version","tools"} and type(policy["version"]) is int and policy["version"] == 1, "Unsupported policy version")
            require(set(policy["tools"]) == {"custom_input","namespaces","grammar"}, "Invalid tool policy")
            for key, options in {"custom_input":{None,"preserve","function_json"},"namespaces":{None,"preserve","flatten"},"grammar":{None,"preserve","registered_output_validation"}}.items():
                require(policy["tools"][key] in options, "Unsupported tool policy")
    if version in {"v7", "v8", "v9","v10"}:
        bindings=[r["editing"] for r in configuration["routes"] if "editing" in r]
        require(version in {"v8", "v9","v10"} or bool(bindings) or configuration.get("profile_packs",{}).get("schema")=="gateway-profile-pack-configuration/v2", "Editing selection missing")
        for b in bindings:
            require(set(b)=={"id","contract","policy"} and isinstance(b["id"], str) and b["contract"]=="gateway-editing-policy/v1", "Invalid editing projection")
            policy=b["policy"].copy()
            if policy.get("client_contract")=="codex-code-mode/v1":
                descriptor=policy.pop("client_descriptor_sha256", None)
                require(isinstance(descriptor,str) and len(descriptor)==64 and all(c in '0123456789abcdef' for c in descriptor), "Missing host Code Mode descriptor")
                policy["client_contract"]="codex-direct-custom/v1"
            normalization=policy.get('normalization')
            require(normalization in {'none','patch-envelope/v1'} and not (b['policy']['client_contract']=='codex-code-mode/v1' and normalization!='none'), 'Unsupported editing normalization')
            policy['normalization']='none'
            require(policy.get('representation') in {'context-lines/v1','patch-text/v1','operations/v1'}, 'Unsupported editing representation')
            policy['representation']='context-lines/v1'
            require(policy=={"version":1,"client_contract":"codex-direct-custom/v1","representation":"context-lines/v1","patch_dialect":"codex-patch/1","normalization":"none"}, "Unsupported editing policy")
    if version in {"v6", "v8"} or (version in {"v7", "v9","v10"} and any("api_codec" in r for r in configuration["routes"])):
        selected=[route['api_codec'] for route in configuration['routes'] if 'api_codec' in route]
        require(bool(selected), 'Codec selection missing')
        if version == 'v8':
            require(any(c.get('protocol') == 'gateway-api-codec/v3' for c in selected), 'Capability codec selection missing')
        for codec in selected:
            versioned = version in {'v8', 'v9', 'v10'} and codec.get('protocol') == 'gateway-api-codec/v3'
            require(set(codec)=={'id','version','package_sha256','executable_sha256','protocol','replay_versions','permissions'} | ({'capabilities'} if versioned else set()), 'Invalid codec binding')
            require((versioned or codec['protocol'] in {'gateway-api-codec/v1','gateway-api-codec/v2'}) and codec['replay_versions']==[1] and codec['permissions']==['read_model_payload','transform_model_protocol'], 'Unsupported codec contract')
            if versioned:
                validate_codec_capabilities(codec['capabilities'])
            require(all(isinstance(codec[k],str) and len(codec[k])==64 and all(c in '0123456789abcdef' for c in codec[k]) for k in ('package_sha256','executable_sha256')), 'Invalid codec digest')
    provider_routes = [r for r in configuration['routes'] if 'provider_plugin' in r or r.get('api') == 'plugin']
    require((version in {'v9','v10'}) == bool(provider_routes), 'Provider manifest version mismatch')
    managed_providers = [r for r in provider_routes if r.get('continuation_mode') == 'managed']
    require((version == 'v10') == bool(managed_providers), 'Provider replay version mismatch')
    if version == 'v10':
        require(managed, 'Managed provider storage missing')
    for route in managed_providers:
        require(route.get('provider_replay_schema') == 'gateway-continuation/v3' and type(route.get('provider_state_limit_bytes')) is int and route['provider_state_limit_bytes'] == 1048576 and 'managed_continuation' in route['provider_plugin']['capabilities']['features'] and 'wire_contract_sha256' not in route, 'Unsupported provider state contract')
    for route in provider_routes:
        require(route.get('api') == 'plugin' and 'api_codec' not in route and 'usage_profile' not in route, 'Invalid provider route')
        validate_provider_binding(route.get('provider_plugin'))
        path = route.get('provider_path')
        require(isinstance(path, str) and 0 < len(path) <= 1024 and all(re.fullmatch(r'[A-Za-z0-9._-]+', segment) and segment not in {'.', '..'} for segment in path.split('/')), 'Invalid provider path')
    # Integers remain arbitrary precision in both Python and the Rust projection.
    raw = json.dumps(configuration, ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()
    require(hashlib.sha256(raw).hexdigest() == value["configuration_sha256"], "Configuration digest mismatch")
    return value



def validate_codec_capabilities(value):
    require(isinstance(value, dict) and set(value) == {'schema', 'apis', 'features', 'requires'}, 'Invalid codec capabilities')
    require(value['schema'] == 'gateway-plugin-capabilities/v1', 'Unsupported capability schema')
    for name in ('apis', 'features', 'requires'):
        entries = value[name]
        require(isinstance(entries, list) and all(isinstance(v, str) for v in entries), 'Invalid capability array')
        require(entries == sorted(set(entries)), 'Capabilities must be sorted and unique')
    require(bool(value['apis']) and set(value['apis']) <= {'chat_completions', 'gemini_interactions', 'messages', 'responses'}, 'Unsupported codec API')
    require('json' in value['features'] and set(value['features']) <= {'editing', 'json', 'managed_continuation', 'streaming'}, 'Unsupported codec feature')
    require(value['requires'] == ['codec_ipc_v3', 'responses_output_validation'], 'Unsupported required host contracts')


def validate_provider_binding(value):
    require(isinstance(value, dict) and set(value) == {'protocol', 'provider_protocol', 'id', 'version', 'package_sha256', 'executable_sha256', 'capabilities', 'permissions'}, 'Invalid provider binding')
    require(value['protocol'] == 'gateway-provider/v1' and value['permissions'] == ['read_model_payload', 'transform_model_protocol'], 'Unsupported provider contract')
    require(isinstance(value['provider_protocol'], str) and re.fullmatch(r'[a-z][a-z0-9._-]{0,63}/v[1-9][0-9]{0,5}', value['provider_protocol']), 'Invalid provider identity')
    require(all(isinstance(value[k], str) and re.fullmatch(r'[0-9a-f]{64}', value[k]) for k in ('package_sha256', 'executable_sha256')), 'Invalid provider digest')
    validate_provider_capabilities(value['capabilities'])


def validate_provider_capabilities(capabilities):
    require(isinstance(capabilities, dict) and capabilities.get('apis') == [] and capabilities.get('requires') == ['provider_ipc_v1', 'responses_output_validation'], 'Invalid provider capabilities')
    codec = {**capabilities, 'apis': ['responses'], 'requires': ['codec_ipc_v3', 'responses_output_validation']}
    validate_codec_capabilities(codec)


def validate_profile_packs(configuration):
    packs = configuration["profile_packs"]
    require(isinstance(packs, dict) and set(packs) == ({"schema", "activation", "packages", "evidence_status", "capability_imports", "policy_imports"} | ({"editing_imports"} if packs.get("schema")=="gateway-profile-pack-configuration/v2" else set())), "Invalid profile pack projection")
    require(packs["schema"] in {"gateway-profile-pack-configuration/v1","gateway-profile-pack-configuration/v2"} and packs["evidence_status"] == "publisher_claims_not_attestation", "Unsupported profile pack contract")
    activation = packs["activation"]
    require(set(activation) == {"schema", "generation", "packs"} and activation["schema"] == "gateway-profile-pack-lock/v1" and type(activation["generation"]) is int and 0 <= activation["generation"] < 2**64, "Invalid profile pack activation")
    entries = activation["packs"]
    require(isinstance(entries, list) and len(entries) <= 16 and all(isinstance(e, dict) and set(e) == {"id", "version", "package_sha256"} for e in entries), "Invalid pack entries")
    require([e["id"] for e in entries] == sorted(set(e["id"] for e in entries)) and set(packs["packages"]) == {e["id"] for e in entries}, "Profile pack inventory mismatch")
    entries = {e["id"]: e for e in entries}
    for name, package in packs["packages"].items():
        entry = entries[name]
        require(set(package) == ({"schema", "id", "version", "capabilities", "policies", "evidence", "notices"} | ({"editing_policies"} if package.get("editing_policies") else set())) and package["schema"] in {"gateway-profile-pack/v1","gateway-profile-pack/v2"} and package["id"] == name and package["version"] == entry["version"], "Invalid profile pack package")
        raw = json.dumps(package, ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False).encode() + b"\n"
        require(hashlib.sha256(raw).hexdigest() == entry["package_sha256"], "Profile pack bytes mismatch")
    for kind, exports in (("capability_imports", "capabilities"), ("policy_imports", "policies"), ("editing_imports","editing_policies")):
        for binding in packs.get(kind,{}).values():
            require(set(binding) == ({"pack", "export", "provider", "upstream_model"} if kind == "capability_imports" else {"pack", "export"}), "Invalid profile pack import")
            require(binding["pack"] in entries and binding["export"] in packs["packages"][binding["pack"]][exports], "Unresolved profile pack import")
    for route in configuration["routes"]:
        expected = {}
        for kind, imports, name in (("capability", "capability_imports", route["capability_profile"]["id"]), ("policy", "policy_imports", route.get("compatibility", {}).get("id"))):
            binding = packs[imports].get(name)
            expected[kind] = None if binding is None else {"pack": binding["pack"], "export": binding["export"], "version": entries[binding["pack"]]["version"], "package_sha256": entries[binding["pack"]]["package_sha256"]}
        binding=packs.get("editing_imports",{}).get(route.get("editing",{}).get("id"))
        if binding is not None:
            expected['editing']={"pack":binding['pack'],"export":binding['export'],"version":entries[binding['pack']]['version'],"package_sha256":entries[binding['pack']]['package_sha256']}
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
    require(ready["schema"] == ("gateway-ready/v10" if manifest["schema"].endswith("/v10") else "gateway-ready/v9" if manifest["schema"].endswith("/v9") else "gateway-ready/v8" if manifest["schema"].endswith("/v8") else "gateway-ready/v7" if manifest["schema"].endswith("/v7") else "gateway-ready/v6" if manifest["schema"].endswith("/v6") else "gateway-ready/v5" if manifest["schema"].endswith("/v5") else "gateway-ready/v4" if manifest["schema"].endswith("/v4") else "gateway-ready/v3" if manifest["schema"].endswith("/v3") else "gateway-ready/v2" if manifest["schema"].endswith("/v2") else READY_SCHEMA) and ready["manifest_schema"] == manifest["schema"] and ready["event"] == "ready", "Unsupported readiness contract")
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
    require(value['schema'] in {'gateway-extended-manifest/v3','gateway-extended-manifest/v4','gateway-extended-manifest/v5','gateway-extended-manifest/v6','gateway-extended-manifest/v7','gateway-extended-manifest/v8','gateway-extended-manifest/v9','gateway-extended-manifest/v10','gateway-extended-manifest/v11'}, 'Unsupported extended managed contract')
    configuration=value['configuration']
    require(set(configuration) in (({'gateway','extensions','usage_event_schemas','usage_profiles'},) if value['schema'].endswith('/v11') else ({'gateway','extensions'}, {'gateway','extensions','usage_contract','usage_profiles'})), 'Invalid extended configuration')
    base=validate_manifest(configuration['gateway'])
    require(base['schema']==value['schema'].replace('extended','embedded') or value['schema'].endswith(('/v6', '/v8', '/v9', '/v11')), 'Managed base version mismatch')
    if value['schema'].endswith(('/v8', '/v11')):
        extensions = configuration['extensions']
        require(isinstance(extensions, dict) and extensions.get('schema') == ('gateway-extension-configuration/v5' if value['schema'].endswith('/v11') else 'gateway-extension-configuration/v3'), 'Unsupported extension capabilities configuration')
        packages = extensions.get('packages')
        require(isinstance(packages, list), 'Invalid extension packages')
        selected = [p for p in packages if isinstance(p, dict) and p.get('protocol') == 'gateway-api-codec/v3']
        require(bool(selected) or value['schema'].endswith('/v11'), 'Capability package selection missing')
        for package in selected:
            require(package.get('schema') == 'gateway-extension-package/v2', 'Invalid capability package version')
            validate_codec_capabilities(package.get('capabilities'))
        for route in base['configuration']['routes']:
            codec = route.get('api_codec', {})
            if codec.get('protocol') == 'gateway-api-codec/v3':
                matching = [p for p in selected if p.get('id') == codec['id'] and p.get('version') == codec['version']]
                require(len(matching) == 1 and matching[0]['capabilities'] == codec['capabilities'], 'Codec package capabilities mismatch')
    if value['schema'].endswith(('/v9', '/v10', '/v11')):
        extensions = configuration['extensions']
        require((value['schema'].endswith('/v11') and extensions.get('schema') == 'gateway-extension-configuration/v5') or (set(configuration) == {'gateway', 'extensions'} and extensions.get('schema') == 'gateway-extension-configuration/v4'), 'Unsupported provider extension configuration')
        packages = extensions.get('packages')
        require(isinstance(packages, list), 'Invalid provider packages')
        providers = [p for p in packages if p.get('protocol') == 'gateway-provider/v1']
        require(bool(providers) or value['schema'].endswith('/v11'), 'Provider package selection missing')
        for package in providers:
            require(package.get('schema') == 'gateway-extension-package/v2' and isinstance(package.get('provider_protocol'), str) and re.fullmatch(r'[a-z][a-z0-9._-]{0,63}/v[1-9][0-9]{0,5}', package['provider_protocol']), 'Invalid provider package identity')
            validate_provider_capabilities(package.get('capabilities'))
        for route in base['configuration']['routes']:
            binding = route.get('provider_plugin')
            if binding is not None:
                matching = [p for p in providers if p.get('id') == binding['id'] and p.get('version') == binding['version']]
                require(len(matching) == 1 and matching[0].get('capabilities') == binding['capabilities'] and matching[0].get('provider_protocol') == binding['provider_protocol'], 'Provider package binding mismatch')
    if value['schema'].endswith('/v11'):
        extensions = configuration['extensions']
        require(isinstance(extensions, dict) and extensions.get('schema') == 'gateway-extension-configuration/v5', 'Unsupported recorder extension configuration')
        packages = extensions.get('packages')
        require(isinstance(packages, list), 'Invalid recorder packages')
        recorders = [p for p in packages if p.get('protocol') in ('gateway-usage-recorder/v1', 'gateway-usage-recorder/v2')]
        expected = {'schema': 'gateway-plugin-capabilities/v1', 'apis': [], 'features': ['usage_event_v1', 'usage_event_v2'], 'requires': ['usage_recorder_ipc_v2']}
        require(len(recorders) == 1 and recorders[0].get('schema') == 'gateway-extension-package/v2'
                and recorders[0].get('protocol') == 'gateway-usage-recorder/v2'
                and recorders[0].get('state_schema') == 'usage-store/v2'
                and recorders[0].get('permissions') == ['export_usage', 'observe_usage', 'write_usage_store']
                and recorders[0].get('capabilities') == expected, 'Recorder v2 selection missing or incompatible')
        require(configuration['usage_event_schemas'] == ['gateway-usage-event/v1', 'gateway-usage-event/v2']
                and configuration['usage_profiles'] == (['responses/v1','chat/v1','messages/v1','gemini_interactions/v1','deepseek/v1'] if 'continuation' in base['configuration'] else ['responses/v1','chat/v1','messages/v1']), 'Unsupported recorder event schemas')
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
