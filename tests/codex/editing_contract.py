#!/usr/bin/env python3
"""Observe pinned direct/helper tool contracts with synthetic loopback requests.

Only contract hashes and outcomes are reported. Runtime prompts and tool
descriptions stay in temporary private test state and are never published.
"""
import argparse
import contextlib
import hashlib
import json
import os
from pathlib import Path
import secrets
import subprocess
import tempfile
import threading
import time

import conformance as c

PATCH = "*** Begin Patch\n*** Add File: fixture.txt\n+synthetic-content\n*** End Patch"


class Scenario(c.Scenario):
    def __init__(self, workspace, mode):
        super().__init__("editing_contract", workspace)
        self.mode = mode
        self.contracts = []
        self.result_type = None

    def response(self, body):
        self.requests += 1
        c.require(self.requests <= 2, "extra request")
        if self.requests == 2:
            outputs = [v for v in body["input"] if v.get("call_id") == "call_fixture" and v.get("type", "").endswith("_output")]
            c.require(len(outputs) == 1, "missing result linkage")
            self.result_type = outputs[0]["type"]
            self.result_seen = True
            return c.wire_response(c.text_item(), 2)
        tools = body.get("tools", [])
        self.observed_tools = tools
        self.contracts = [{"type": t.get("type"), "name": t.get("name"),
                           "sha256": hashlib.sha256(json.dumps(t, sort_keys=True, separators=(",", ":")).encode()).hexdigest(),
                           "format": {k: v for k, v in t.get("format", {}).items() if k != "definition"},
                           "grammar_sha256": hashlib.sha256(t.get("format", {}).get("definition", "").encode()).hexdigest()}
                          for t in tools if t.get("name") == ("apply_patch" if self.mode == "direct" else "exec")]
        name = "apply_patch" if self.mode == "direct" else "exec"
        candidates = [t for t in tools if t.get("type") == "custom" and t.get("name") == name]
        c.require(len(candidates) == 1, "expected custom tool missing")
        expected = json.loads((Path(__file__).with_name("editing-contract-lock.json")).read_text())["contracts"][self.mode]
        c.require(self.contracts == [expected], "pinned tool contract changed")
        value = PATCH if self.mode == "direct" else "const result = await tools.apply_patch(" + json.dumps(PATCH) + ");\ntext(result);"
        item = {"id": "ct_fixture", "type": "custom_tool_call", "name": name,
                "call_id": "call_fixture", "input": value, "status": "completed"}
        return c.wire_response(item, 1)


def run(binary, gateway, mode, deny=False, capture=None):
    local = c.ROOT / ".local"
    local.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="editing-contract-", dir=local) as tmp, contextlib.ExitStack() as cleanup:
        root = Path(tmp)
        workspace, home = root / "workspace", root / "codex-home"
        workspace.mkdir()
        home.mkdir()
        state = Scenario(workspace, mode)
        server = c.MockServer(("127.0.0.1", 0), c.UpstreamHandler)
        server.scenario = state
        threading.Thread(target=server.serve_forever, daemon=True).start()
        cleanup.callback(server.server_close)
        cleanup.callback(server.shutdown)
        cleanup.callback(state.stop.set)
        token = secrets.token_urlsafe(32)
        env = {k: v for k, v in os.environ.items() if k in {"HOME", "PATH", "TMPDIR", "LANG"}}
        config = root / "gateway.toml"
        config.write_text(f'[providers.mock]\nbase_url="http://127.0.0.1:{server.server_port}/v1"\napi_key_env="ARG_MOCK_KEY"\n[models."gpt-5.4"]\nprovider="mock"\nupstream_model="synthetic-model"\n')
        gateway_env = {**env, "ARG_LOCAL_TOKEN": token, "ARG_MOCK_KEY": "synthetic-upstream-key"}
        manifest = c.embedded_contract.inspect_manifest(gateway, config, gateway_env)
        process = subprocess.Popen([str(gateway), "serve", "--config", str(config)], env=gateway_env, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        cleanup.callback(c.stop_process, process)
        ready = c.embedded_contract.read_ready(process, manifest)
        (home / "config.toml").write_text(f'model="gpt-5.4"\nmodel_provider="gateway"\nweb_search="disabled"\nmodel_context_window=32768\nmodel_auto_compact_token_limit=24576\n[features]\napps=false\nmulti_agent=false\ncode_mode={str(mode == "code_mode").lower()}\ncode_mode_only={str(mode == "code_mode").lower()}\n[model_providers.gateway]\nname="Synthetic gateway"\nbase_url="{ready["base_url"]}"\nwire_api="responses"\nenv_key="ARG_CODEX_TEST_TOKEN"\nrequires_openai_auth=false\nsupports_websockets=false\nrequest_max_retries=0\nstream_max_retries=0\n')
        child = subprocess.Popen([str(binary), "app-server"], cwd=workspace,
                                 env={**env, "HOME": str(home), "CODEX_HOME": str(home), "ARG_CODEX_TEST_TOKEN": token},
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, bufsize=1)
        cleanup.callback(c.stop_process, child)
        rpc = c.RpcClient(child)
        rpc.call("initialize", {"clientInfo": {"name": "arg_editing_contract", "version": "0.1.0"}, "capabilities": {"experimentalApi": True}})
        rpc.send({"method": "initialized", "params": {}})
        thread = rpc.call("thread/start", {"model": "gpt-5.4", "modelProvider": "gateway", "cwd": str(workspace), "sandbox": "read-only" if deny else "workspace-write", "approvalPolicy": "on-request", "approvalsReviewer": "user", "ephemeral": True, "allowProviderModelFallback": False})
        rpc.call("turn/start", {"threadId": thread["thread"]["id"], "input": [{"type": "text", "text": "Exercise the synthetic editing fixture."}]})
        final, approvals = None, 0
        deadline = time.monotonic() + 45
        while time.monotonic() < deadline:
            message = rpc.pending.pop(0) if rpc.pending else rpc.next(max(0.01, deadline - time.monotonic()))
            method = message.get("method", "")
            if method.endswith("requestApproval"):
                c.require(deny, "unexpected approval")
                approvals += 1
                rpc.send({"id": message["id"], "result": {"decision": "decline"}})
            elif method == "turn/completed":
                final = message["params"]["turn"]["status"]
                break
            elif "id" in message and method:
                raise AssertionError("unexpected server request")
        applied = (workspace / "fixture.txt").exists()
        if applied:
            c.require((workspace / "fixture.txt").read_bytes() == b"synthetic-content\n", "file content changed")
        result = {"mode": mode, "deny": deny, "turn_status": final, "requests": state.requests,
                  "applied": applied, "approvals": approvals, "result_type": state.result_type,
                  "contracts": state.contracts}
        if state.errors or final != "completed" or not state.result_seen:
            print(json.dumps({"failed_contract": result}), flush=True)
        c.require(not state.errors and final == "completed" and state.result_seen, "contract round trip failed")
        c.require((deny and approvals == 1 and not applied) or (not deny and applied), "execution contract failed")
        if capture is not None:
            capture.extend(t["format"] for t in state.observed_tools if t.get("type")=="custom" and t.get("name")=="apply_patch")
        return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--gateway-bin", type=Path, required=True)
    parser.add_argument("--runtime-dir", type=Path, default=c.runtime.BUNDLE)
    parser.add_argument("--mode", choices=("direct", "code_mode"), action="append")
    args = parser.parse_args()
    lock = json.loads(c.runtime.LOCK.read_text())
    binary = c.runtime.verify_bundle(args.runtime_dir.resolve(), lock)
    for mode in args.mode or ["direct", "code_mode"]:
        for deny in (False, True):
            print(json.dumps(run(binary, args.gateway_bin.resolve(), mode, deny), sort_keys=True), flush=True)


if __name__ == "__main__":
    main()
