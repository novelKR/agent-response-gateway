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
    require(value["schema"] in {MANIFEST_SCHEMA,"gateway-embedded-manifest/v2","gateway-embedded-manifest/v3"} and value["client_api"] == "responses" and value["lifecycle"] == "host-supervised-process/v1", "Unsupported manifest contract")
    package = value["package"]
    require(isinstance(package, dict) and set(package) == {"name", "version"} and package["name"] == "agent-response-gateway" and isinstance(package["version"], str) and bool(package["version"]), "Invalid package identity")
    configuration = value["configuration"]
    require(isinstance(configuration, dict) and set(configuration) == ({"listen", "source_url", "local_token_env", "upstream_credential_references", "limits", "routes"} | ({"continuation", "replay_versions"} if value["schema"].endswith("/v3") else {"continuation"} if value["schema"].endswith("/v2") else set())), "Invalid configuration projection")
    if value["schema"].endswith("/v3"):
        require(configuration["replay_versions"] == {"read": [1, 2], "write": 2}, "Unsupported replay contract")
    # Integers remain arbitrary precision in both Python and the Rust projection.
    raw = json.dumps(configuration, ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()
    require(hashlib.sha256(raw).hexdigest() == value["configuration_sha256"], "Configuration digest mismatch")
    return value


def inspect_manifest(binary, config, env):
    result = subprocess.run([str(binary), "manifest", "--config", str(config)], env=env, capture_output=True, timeout=10, check=False)
    require(result.returncode == 0 and not result.stderr and len(result.stdout.splitlines()) == 1, "Manifest inspection failed")
    return validate_manifest(load_json(result.stdout))


def parse_ready_line(line, manifest):
    require(isinstance(line, str) and line.endswith("\n") and len(line.encode()) <= MAX_READY_BYTES, "Invalid readiness frame")
    ready = load_json(line)
    require(isinstance(ready, dict) and set(ready) == {"schema", "manifest_schema", "configuration_sha256", "event", "address", "base_url", "version"}, "Invalid readiness shape")
    require(ready["schema"] == ("gateway-ready/v3" if manifest["schema"].endswith("/v3") else "gateway-ready/v2" if manifest["schema"].endswith("/v2") else READY_SCHEMA) and ready["manifest_schema"] == manifest["schema"] and ready["event"] == "ready", "Unsupported readiness contract")
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
    return parse_ready_line(line, manifest)


def validate_credential_split(manifest, gateway_env, codex_env, local_key_name, home):
    configuration = manifest["configuration"]
    references = configuration["upstream_credential_references"].values()
    require(all(key in gateway_env for key in references), "Gateway credential reference missing")
    keys = {gateway_env[key] for key in references}
    require(not any(value in keys for value in codex_env.values()), "Upstream credential reached Codex environment")
    require(codex_env.get(local_key_name) == gateway_env.get(configuration["local_token_env"]) and codex_env.get(local_key_name) not in keys, "Local token separation failed")
    require(codex_env.get("CODEX_HOME") == str(home) and codex_env.get("HOME") == str(home), "Codex home is not isolated")


def validate_extended_manifest(value):
    require(isinstance(value,dict) and set(value)=={'schema','configuration','execution_sha256'}, 'Invalid extended manifest')
    require(value['schema']=='gateway-extended-manifest/v3', 'Unsupported extended managed contract')
    configuration=value['configuration']
    require(set(configuration) in ({'gateway','extensions'}, {'gateway','extensions','usage_contract','usage_profiles'}), 'Invalid extended configuration')
    base=validate_manifest(configuration['gateway'])
    require(base['schema']=='gateway-embedded-manifest/v3', 'Managed base version mismatch')
    if 'usage_contract' in configuration:
        require(configuration['usage_contract']=='gateway-usage-event/v1' and configuration['usage_profiles']==['responses/v1','chat/v1','messages/v1','gemini_interactions/v1','deepseek/v1'], 'Unsupported usage profiles')
    raw=json.dumps(configuration,ensure_ascii=False,sort_keys=True,separators=(',',':'),allow_nan=False).encode()
    require(hashlib.sha256(raw).hexdigest()==value['execution_sha256'], 'Execution digest mismatch')
    return value


def parse_extended_ready_line(line, manifest):
    validate_extended_manifest(manifest)
    ready=load_json(line)
    require(ready.get('schema')=='gateway-extended-ready/v3' and ready.get('manifest_schema')==manifest['schema'] and ready.get('execution_sha256')==manifest['execution_sha256'], 'Extended readiness binding mismatch')
    base=dict(ready);base.pop('execution_sha256')
    base['schema']='gateway-ready/v3';base['manifest_schema']='gateway-embedded-manifest/v3'
    parse_ready_line(json.dumps(base)+'\n',manifest['configuration']['gateway'])
    return ready
