#!/usr/bin/env python3
"""Real pinned Codex continuity with synthetic loopback traffic and private host records."""

import argparse
import contextlib
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import secrets
import subprocess
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler

import conformance as base

spec = importlib.util.spec_from_file_location("continuity_contract", base.ROOT / "scripts/continuity_contract.py")
contract = importlib.util.module_from_spec(spec)
spec.loader.exec_module(contract)
SENTINEL = "CONTINUITY_SYNTHETIC_SENTINEL"
TOOL_RESULT = "CONTINUITY_COMPLETED_TOOL_RESULT"


def file_sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


class State:
    def __init__(self):
        self.phase = "tool"
        self.requests = 0
        self.errors = []
        self.phase_counts = {}

    def respond(self, body):
        self.requests += 1
        self.phase_counts[self.phase] = self.phase_counts.get(self.phase, 0) + 1
        count = self.phase_counts[self.phase]
        base.require(self.requests <= 9 and count <= 2, "unexpected extra continuity request")
        encoded = json.dumps(body.get("input", []))
        base.require(SENTINEL in encoded, "required sentinel disappeared from current context")
        base.require(body.get("model") == ("synthetic-alternate" if self.phase == "switch" else "synthetic-model"), "continuity model binding changed")
        if self.phase != "tool" or count == 2:
            base.require(TOOL_RESULT in encoded, "completed tool result disappeared from current context")
        if self.phase == "tool" and count == 1:
            item = {"type": "function_call", "id": "tool-item", "call_id": "continuity-tool-1",
                    "name": "gateway_echo", "arguments": json.dumps({"text": SENTINEL}), "status": "completed"}
        else:
            item = {"id": "continuity-message", "type": "message", "role": "assistant", "status": "completed",
                    "content": [{"type": "output_text", "text": SENTINEL + " " + TOOL_RESULT, "annotations": []}]}
        return base.wire_response(item, self.requests)


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        try:
            base.require(self.path == "/v1/responses", "unexpected remote storage or compaction endpoint")
            base.require(self.headers.get("Authorization") == "Bearer synthetic-key", "wrong upstream credential")
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            frames = self.server.state.respond(body)
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Connection", "close")
            self.end_headers()
            for frame in frames:
                self.wfile.write(frame)
                self.wfile.flush()
            self.close_connection = True
        except Exception:
            self.server.state.errors.append("continuity upstream assertion failed")
            self.close_connection = True


def finish(rpc, *, allow_tool=False, record=None):
    calls = []
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        message = rpc.pending.pop(0) if rpc.pending else rpc.next(max(0.01, deadline - time.monotonic()))
        method = message.get("method")
        if method == "item/tool/call":
            base.require(allow_tool and not calls and message["params"]["tool"] == "gateway_echo", "completed tool was reexecuted")
            base.require(message["params"]["arguments"] == {"text": SENTINEL}, "dynamic tool arguments changed")
            contract.admit_tool(record, "continuity-tool-1")
            calls.append("continuity-tool-1")
            rpc.send({"id": message["id"], "result": {"contentItems": [{"type": "inputText", "text": TOOL_RESULT}], "success": True}})
        elif method == "turn/completed":
            turn = message["params"]["turn"]
            base.require(turn["status"] == "completed", "continuity turn failed")
            return turn["id"], calls
        elif "id" in message and method:
            raise AssertionError("unexpected request during continuity turn")
    raise AssertionError("continuity turn did not finish")


def history(rpc, thread_id, home, *, allow_unmaterialized=False):
    # The pinned alpha reports list_turns as unsupported. The host needs the
    # actual private history path/digest, not an unimplemented turn listing.
    result = rpc.call("thread/read", {"threadId": thread_id, "includeTurns": False})
    original = Path(result["thread"]["path"])
    path = original.resolve(strict=not allow_unmaterialized)
    base.require(path.is_relative_to(home.resolve()) and not path.is_symlink(), "history is outside its private home")
    base.require(not any(p.is_symlink() for p in (original, *original.parents)), "history path contains a symlink")
    return {"id": thread_id, "provider": "gateway", "history_reference": path.relative_to(home).as_posix(),
            "history_sha256": file_sha(path) if path.exists() else None}


def identity(manifest, route, binary, gateway_binary):
    lock = json.loads(base.runtime.LOCK.read_text())
    return {"runtime": {"version": lock["version"], "binary_sha256": file_sha(binary),
                        "state_compatibility": "codex-" + lock["version"] + "/exact-bundle"},
            "gateway": {"version": manifest["package"]["version"], "binary_sha256": file_sha(gateway_binary),
                        "configuration_sha256": manifest["configuration_sha256"]},
            "route": {"alias": route["alias"], "codex_provider": "gateway", "provider_id": route["provider_id"],
                      "upstream_model": route["upstream_model"], "api": route["api"], "adapter_version": route["adapter_version"],
                      "profile_id": route["capability_profile"]["id"], "profile_version": route["capability_profile"]["version"],
                      "profile_sha256": contract.sha256(route["capability_profile"]),
                      "context_window": route["context_window"], "max_output_tokens": route["max_output_tokens"]},
            "credential_owner": {"realm": "synthetic", "generation": "1"},
            "policy": {"compaction": "local-only", "remote_compaction": "unsupported",
                       "max_host_requests": 8, "primary_transport_retries": 0}}


def write_record(root, record):
    # A fresh immutable revision plus atomic current pointer; records contain metadata only.
    directory = root / record["thread"]["id"]
    directory.mkdir(mode=0o700, exist_ok=True)
    path = directory / (str(record["revision"]) + ".json")
    with path.open("xb") as stream:
        os.chmod(path, 0o600)
        stream.write(contract.canonical(record))
        stream.flush()
        os.fsync(stream.fileno())
    pointer = directory / "current.new"
    pointer.write_text(json.dumps({"revision": record["revision"], "sha256": contract.sha256(record)}))
    pointer.chmod(0o600)
    pointer.replace(directory / "current.json")
    return record


def run(binary, gateway_binary):
    local = base.ROOT / ".local/continuity"
    local.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="continuity-", dir=local) as temporary, contextlib.ExitStack() as cleanup:
        root = Path(temporary)
        home, workspace, journal = root / "home", root / "workspace", root / "journal"
        for path in (home, workspace, journal):
            path.mkdir(mode=0o700)
        state = State()
        http = base.MockServer(("127.0.0.1", 0), Handler)
        http.state = state
        threading.Thread(target=http.serve_forever, daemon=True).start()
        cleanup.callback(http.server_close)
        cleanup.callback(http.shutdown)
        env = {key: os.environ[key] for key in ("PATH", "LANG", "TMPDIR") if key in os.environ}
        env["HOME"] = str(home)
        token = secrets.token_urlsafe(32)
        gateway_env = {**env, "ARG_LOCAL_TOKEN": token, "ARG_MOCK_KEY": "synthetic-key"}
        codex_env = {**env, "CODEX_HOME": str(home), "ARG_CODEX_TEST_TOKEN": token}
        lock = json.loads(base.runtime.LOCK.read_text())
        config = root / "gateway.toml"
        text = f'listen="127.0.0.1:0"\n[providers.mock]\nbase_url="http://127.0.0.1:{http.server_port}/v1"\napi_key_env="ARG_MOCK_KEY"\n'
        for alias, upstream, profile in (("gpt-5.4", "synthetic-model", "primary"), ("gpt-5.4-mini", "synthetic-alternate", "alternate")):
            text += f'[models."{alias}"]\nprovider="mock"\nupstream_model="{upstream}"\ncapability_profile="{profile}"\n'
            text += f'[capability_profiles.{profile}]\nversion="1"\nprovider="mock"\nupstream_model="{upstream}"\napi="responses"\ncontext_window=32768\nmax_output_tokens=8192\ntested_codex_version="{lock["version"]}"\n[capability_profiles.{profile}.support]\n'
            text += "".join(key + '=\"native\"\n' for key in ("instructions", "instruction_hierarchy", "function_tools", "custom_tools", "custom_grammar", "namespaced_tools", "tool_choice", "parallel_tool_control", "max_output_tokens", "reasoning_effort", "structured_output", "strict_structured_output", "strict_tool_arguments"))
        config.write_text(text)
        manifest = base.embedded_contract.inspect_manifest(gateway_binary, config, env)
        routes = {route["alias"]: route for route in manifest["configuration"]["routes"]}
        origin = identity(manifest, routes["gpt-5.4"], binary, gateway_binary)
        gateway, codex = None, None
        def close_children():
            if codex is not None:
                base.stop_process(codex)
            if gateway is not None:
                base.stop_process(gateway)
        cleanup.callback(close_children)
        def start_children():
            nonlocal gateway, codex
            current = base.embedded_contract.inspect_manifest(gateway_binary, config, env)
            base.require(current == manifest, "gateway configuration changed before restart")
            gateway = subprocess.Popen([str(gateway_binary), "serve", "--config", str(config)], env=gateway_env,
                                       stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
            ready = base.embedded_contract.read_ready(gateway, manifest)
            (home / "config.toml").write_text(f'model="gpt-5.4"\nmodel_provider="gateway"\nweb_search="disabled"\nmodel_context_window=32768\nmodel_auto_compact_token_limit=24576\n[model_providers.gateway]\nname="Synthetic continuity"\nbase_url="{ready["base_url"]}"\nwire_api="responses"\nenv_key="ARG_CODEX_TEST_TOKEN"\nrequires_openai_auth=false\nsupports_websockets=false\nrequest_max_retries=0\nstream_max_retries=0\n')
            base.embedded_contract.validate_credential_split(manifest, gateway_env, codex_env, "ARG_CODEX_TEST_TOKEN", home)
            codex = subprocess.Popen([str(binary), "app-server"], cwd=workspace, env=codex_env,
                                     stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, bufsize=1)
            rpc = base.RpcClient(codex)
            rpc.call("initialize", {"clientInfo": {"name": "arg_continuity", "version": "0.1.0"}, "capabilities": {"experimentalApi": True}})
            rpc.send({"method": "initialized", "params": {}})
            return rpc
        rpc = start_children()
        tool = {"type": "function", "name": "gateway_echo", "description": "Return a synthetic result once.",
                "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"], "additionalProperties": False}}
        settings = {"modelProvider": "gateway", "cwd": str(workspace), "sandbox": "read-only", "approvalPolicy": "on-request",
                    "approvalsReviewer": "user", "allowProviderModelFallback": False, "dynamicTools": [tool]}
        result = rpc.call("thread/start", {**settings, "model": "gpt-5.4", "ephemeral": False})
        tid = result["thread"]["id"]
        base.require(result["model"] == "gpt-5.4" and result["modelProvider"] == "gateway", "thread origin mismatch")
        record = write_record(journal, contract.create(origin, history(rpc, tid, home, allow_unmaterialized=True)))
        record = write_record(journal, contract.begin(record, "turn"))
        rpc.call("turn/start", {"threadId": tid, "input": [{"type": "text", "text": SENTINEL}]})
        turn, calls = finish(rpc, allow_tool=True, record=record)
        base.require(calls == ["continuity-tool-1"] and state.requests == 2, "tool round trip did not complete")
        record = write_record(journal, contract.complete(record, history(rpc, tid, home), turn, completed_tool_ids=calls))
        for phase, method, params in (("followup", "turn/start", {"threadId": tid, "input": [{"type": "text", "text": "Continue with the saved context."}]}),
                                      ("compact", "thread/compact/start", {"threadId": tid}),
                                      ("after_compact", "turn/start", {"threadId": tid, "input": [{"type": "text", "text": "Use the compacted context."}]})):
            state.phase = phase
            record = write_record(journal, contract.begin(record, "compact" if phase == "compact" else "turn"))
            rpc.call(method, params)
            turn, calls = finish(rpc)
            base.require(not calls, "completed tool executed during continuation")
            record = write_record(journal, contract.complete(record, history(rpc, tid, home), turn))
        close_children()
        before = state.requests
        # Verify the actual closed history and all current origin fields before restart/resume.
        persisted_thread = {**record["thread"], "history_sha256": file_sha(home / record["thread"]["history_reference"])}
        contract.resume(record, origin, persisted_thread)
        rejected = 0
        for group, key, changed_value in (("credential_owner", "generation", "2"), ("gateway", "configuration_sha256", "a" * 64),
                                          ("runtime", "binary_sha256", "a" * 64), ("runtime", "state_compatibility", "other/v2"),
                                          ("route", "profile_sha256", "a" * 64), ("route", "upstream_model", "other-model")):
            changed = copy.deepcopy(origin)
            changed[group][key] = changed_value
            try:
                contract.resume(record, changed, persisted_thread)
            except contract.ContinuityError:
                rejected += 1
        base.require(rejected == 6 and state.requests == before, "changed binding made an upstream request")
        rpc = start_children()
        expected = contract.resume(record, origin, persisted_thread)
        result = rpc.call("thread/resume", {**settings, **expected})
        base.require(result["model"] == expected["model"] and result["modelProvider"] == expected["modelProvider"], "resumed provider/model changed")
        state.phase = "restart"
        record = write_record(journal, contract.begin(record, "turn"))
        rpc.call("turn/start", {"threadId": tid, "input": [{"type": "text", "text": "Continue after process restart."}]})
        turn, calls = finish(rpc)
        base.require(not calls, "completed tool executed after process restart")
        record = write_record(journal, contract.complete(record, history(rpc, tid, home), turn))
        portable = [{"type": "message", "role": "user", "text": SENTINEL},
                    {"type": "completed_tool_result", "call_id": "continuity-tool-1", "result": TOOL_RESULT}]
        result = rpc.call("thread/start", {**settings, "model": "gpt-5.4-mini", "ephemeral": False})
        base.require(result["model"] == "gpt-5.4-mini" and result["modelProvider"] == "gateway", "switched provider/model changed")
        alternate = identity(manifest, routes["gpt-5.4-mini"], binary, gateway_binary)
        switched = contract.switch(record, alternate, history(rpc, result["thread"]["id"], home, allow_unmaterialized=True), portable,
                                   omitted_state=["opaque_reasoning", "provider_response_ids"])
        switched = write_record(journal, switched)
        state.phase = "switch"
        switched = write_record(journal, contract.begin(switched, "turn"))
        rpc.call("turn/start", {"threadId": switched["thread"]["id"], "input": [{"type": "text", "text": json.dumps(portable)}]})
        turn, calls = finish(rpc)
        base.require(not calls and not state.errors, "switch or upstream continuity failed")
        write_record(journal, contract.complete(switched, history(rpc, switched["thread"]["id"], home), turn))
        base.require(state.requests == 7, "continuity HTTP request count differs")
        return {"schema": "gateway-continuity-conformance/v1", "status": "passed", "codex_version": lock["version"],
                "upstream_requests": state.requests, "phase_requests": state.phase_counts, "completed_tool_executions": 1,
                "rejected_bindings_before_request": rejected, "remote_compaction_requests": 0,
                "provider_qualification": False, "consumer_operational_acceptance": False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--gateway-bin", type=Path, default=base.ROOT / "target/debug/agent-response-gateway")
    args = parser.parse_args()
    lock = json.loads(base.runtime.LOCK.read_text())
    binary = base.runtime.verify_bundle(base.runtime.BUNDLE, lock)
    print(json.dumps(run(binary, args.gateway_bin.resolve()), sort_keys=True))


if __name__ == "__main__":
    main()
