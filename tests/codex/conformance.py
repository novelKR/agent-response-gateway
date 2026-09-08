#!/usr/bin/env python3
"""Pinned real-Codex tests against the gateway and a synthetic loopback upstream."""

import argparse
import contextlib
import importlib.util
import json
import os
from pathlib import Path
import queue
import secrets
import subprocess
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("codex_runtime", ROOT / "scripts/codex_runtime.py")
runtime = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runtime)


def require(condition, reason):
    if not condition:
        raise AssertionError(reason)


def event(kind, **fields):
    return ("event: " + kind + "\ndata: " + json.dumps({"type": kind, **fields}) + "\n\n").encode()


def wire_response(item, number):
    response = {"id": f"resp_fixture_{number}", "object": "response", "created_at": 0, "status": "completed", "output": [item], "usage": {"input_tokens": 10, "output_tokens": 3, "total_tokens": 13}}
    return [
        event("response.created", response={**response, "status": "in_progress", "output": []}),
        event("response.output_item.done", output_index=0, item=item),
        event("response.completed", response=response),
    ]


def text_item():
    return {"id": "msg_fixture", "type": "message", "role": "assistant", "status": "completed", "content": [{"type": "output_text", "text": "Synthetic complete.", "annotations": []}]}


class Scenario:
    def __init__(self, name, workspace):
        self.name, self.workspace = name, workspace
        self.requests = 0
        self.result_seen = False
        self.started = threading.Event()
        self.disconnected = threading.Event()
        self.stop = threading.Event()
        self.errors = []
        self.tool_contracts = set()

    def response(self, body):
        self.requests += 1
        require(self.requests <= 2, "unexpected retry or extra model request")
        require(body.get("store") is False and body.get("model") == "synthetic-model", "gateway admission or routing differs")
        require(body.get("stream") is True, "Codex did not request SSE")
        for tool in body.get("tools", []):
            self.tool_contracts.add((tool.get("type", ""), tool.get("name", "")))
        if self.requests == 2:
            expected_type = "custom_tool_call_output" if self.name in {"custom_patch", "approval_denial"} else "function_call_output"
            outputs = [item for item in body.get("input", []) if item.get("type") == expected_type and item.get("call_id") == "call_fixture"]
            require(len(outputs) == 1, "tool result did not return with its original call ID")
            if self.name in {"function_tool", "namespace_tool"}:
                require("synthetic-result" in json.dumps(outputs[0]), "dynamic tool result changed")
            self.result_seen = True
            return wire_response(text_item(), self.requests)
        if self.name == "function_tool":
            require(("function", "gateway_echo") in self.tool_contracts, "dynamic function declaration missing")
            item = {"id": "fc_fixture", "type": "function_call", "call_id": "call_fixture", "name": "gateway_echo", "arguments": json.dumps({"text": "synthetic"}), "status": "completed"}
        elif self.name == "namespace_tool":
            groups = [tool for tool in body.get("tools", []) if tool.get("type") == "namespace" and tool.get("name") == "fixture"]
            require(len(groups) == 1 and any(tool.get("name") == "echo" for tool in groups[0]["tools"]), "namespace tool declaration missing")
            item = {"id": "fc_fixture", "type": "function_call", "call_id": "call_fixture", "namespace": "fixture", "name": "echo", "arguments": json.dumps({"text": "synthetic"}), "status": "completed"}
        elif self.name in {"custom_patch", "approval_denial"}:
            require(("custom", "apply_patch") in self.tool_contracts, "custom patch declaration missing")
            patch = "*** Begin Patch\n*** Add File: fixture.txt\n+synthetic-content\n*** End Patch"
            item = {"id": "ct_fixture", "type": "custom_tool_call", "call_id": "call_fixture", "name": "apply_patch", "input": patch, "status": "completed"}
        else:
            item = text_item()
        return wire_response(item, self.requests)


class MockServer(ThreadingHTTPServer):
    daemon_threads = True

    def handle_error(self, request, client_address):
        self.scenario.errors.append("synthetic upstream handler failed")


class UpstreamHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def do_POST(self):
        state = self.server.scenario
        try:
            require(self.path == "/v1/responses", "unexpected upstream endpoint")
            require(self.headers.get("Authorization") == "Bearer synthetic-upstream-key", "upstream credential selection differs")
            length = int(self.headers["Content-Length"])
            require(0 < length <= 8 * 1024 * 1024, "unexpected request size")
            body = json.loads(self.rfile.read(length))
            frames = state.response(body)
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Connection", "close")
            self.end_headers()
            self.close_connection = True
            self.wfile.write(frames[0])
            self.wfile.flush()
            state.started.set()
            if state.name.startswith("cancellation"):
                # Synchronize cancellation with client-observed output, not server writes.
                partial = {"id": "msg_fixture", "type": "message", "role": "assistant", "status": "in_progress", "content": []}
                for frame in [
                    event("response.output_item.added", output_index=0, item=partial),
                    event("response.content_part.added", item_id="msg_fixture", output_index=0, content_index=0, part={"type": "output_text", "text": "", "annotations": []}),
                    event("response.output_text.delta", item_id="msg_fixture", output_index=0, content_index=0, delta="Synthetic partial."),
                ]:
                    self.wfile.write(frame)
                self.wfile.flush()
                while not state.stop.wait(0.05):
                    frame = b": synthetic keepalive\n\n" if state.name == "cancellation_heartbeat" else event("response.output_text.delta", item_id="msg_fixture", output_index=0, content_index=0, delta=" synthetic")
                    self.wfile.write(frame)
                    self.wfile.flush()
            elif state.name == "transport_failure":
                return  # Deliberately omit the final completion event.
            else:
                for frame in frames[1:]:
                    self.wfile.write(frame)
                    self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            state.disconnected.set()
        except Exception:
            state.errors.append("synthetic upstream contract failed")
            self.close_connection = True


class RpcClient:
    def __init__(self, process):
        self.process = process
        self.messages = queue.Queue()
        self.pending = []
        self.sequence = 0
        threading.Thread(target=self.read, daemon=True).start()

    def read(self):
        try:
            for line in self.process.stdout:
                self.messages.put(json.loads(line))
        except ValueError:
            self.messages.put({"harness_error": "invalid JSONL"})
        finally:
            self.messages.put({"harness_error": "control connection closed"})

    def send(self, message):
        self.process.stdin.write(json.dumps(message) + "\n")
        self.process.stdin.flush()

    def next(self, timeout=20):
        message = self.messages.get(timeout=timeout)
        require("harness_error" not in message and "error" not in message, "Codex control request failed")
        return message

    def call(self, method, params):
        self.sequence += 1
        ident = self.sequence
        self.send({"id": ident, "method": method, "params": params})
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            message = self.next(max(0.01, deadline - time.monotonic()))
            if message.get("id") == ident and "method" not in message:
                return message["result"]
            self.pending.append(message)
        raise AssertionError("Codex control request timed out")


def stop_process(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
    for stream in (process.stdin, process.stdout):
        if stream:
            stream.close()


def run_scenario(name, binary, gateway_binary):
    local = ROOT / ".local/conformance"
    local.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=name + "-", dir=local) as temporary, contextlib.ExitStack() as cleanup:
        root = Path(temporary)
        workspace = root / "workspace"
        workspace.mkdir()
        home = root / "codex-home"
        home.mkdir()
        state = Scenario(name, workspace)
        server = MockServer(("127.0.0.1", 0), UpstreamHandler)
        server.scenario = state
        threading.Thread(target=server.serve_forever, daemon=True).start()
        cleanup.callback(server.server_close)
        cleanup.callback(server.shutdown)
        cleanup.callback(state.stop.set)
        token = secrets.token_urlsafe(32)
        env = {k: v for k, v in os.environ.items() if k in {"HOME", "PATH", "TMPDIR", "LANG"}}
        gateway_env = {**env, "ARG_LOCAL_TOKEN": token, "ARG_MOCK_KEY": "synthetic-upstream-key"}
        config = root / "gateway.toml"
        config.write_text(f'listen="127.0.0.1:0"\n[providers.mock]\nbase_url="http://127.0.0.1:{server.server_port}/v1"\napi_key_env="ARG_MOCK_KEY"\n[models."gpt-5.4"]\nprovider="mock"\nupstream_model="synthetic-model"\n')
        gateway = subprocess.Popen([str(gateway_binary), "serve", "--config", str(config)], env=gateway_env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        cleanup.callback(stop_process, gateway)
        ready_queue = queue.Queue()
        threading.Thread(target=lambda: ready_queue.put(gateway.stdout.readline()), daemon=True).start()
        ready = json.loads(ready_queue.get(timeout=10))
        require(ready.get("event") == "ready", "gateway did not announce readiness")
        (home / "config.toml").write_text(f'model="gpt-5.4"\nmodel_provider="gateway"\nweb_search="disabled"\nmodel_context_window=32768\nmodel_auto_compact_token_limit=24576\n[model_providers.gateway]\nname="Synthetic gateway"\nbase_url="{ready["base_url"]}"\nwire_api="responses"\nenv_key="ARG_CODEX_TEST_TOKEN"\nrequires_openai_auth=false\nsupports_websockets=false\nrequest_max_retries=0\nstream_max_retries=0\n')
        codex_env = {**env, "CODEX_HOME": str(home), "ARG_CODEX_TEST_TOKEN": token}
        codex = subprocess.Popen([str(binary), "app-server"], cwd=workspace, env=codex_env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, bufsize=1)
        cleanup.callback(stop_process, codex)
        rpc = RpcClient(codex)
        rpc.call("initialize", {"clientInfo": {"name": "arg_conformance", "version": "0.1.0"}, "capabilities": {"experimentalApi": True}})
        rpc.send({"method": "initialized", "params": {}})
        echo = {"type": "function", "name": "echo" if name == "namespace_tool" else "gateway_echo", "description": "Return a synthetic fixture value.", "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"], "additionalProperties": False}}
        dynamic = {"type": "namespace", "name": "fixture", "description": "Synthetic tool group.", "tools": [echo]} if name == "namespace_tool" else echo
        thread = rpc.call("thread/start", {"model": "gpt-5.4", "modelProvider": "gateway", "cwd": str(workspace), "sandbox": "workspace-write" if name == "custom_patch" else "read-only", "approvalPolicy": "on-request", "approvalsReviewer": "user", "ephemeral": True, "allowProviderModelFallback": False, "experimentalRawEvents": False, "dynamicTools": [dynamic]})
        thread_id = thread["thread"]["id"]
        turn = rpc.call("turn/start", {"threadId": thread_id, "input": [{"type": "text", "text": "Exercise the synthetic fixture."}]})
        if name.startswith("cancellation"):
            require(state.started.wait(10), "upstream did not start")
            deadline = time.monotonic() + 10
            observed = False
            while time.monotonic() < deadline:
                message = rpc.pending.pop(0) if rpc.pending else rpc.next(max(0.01, deadline - time.monotonic()))
                if message.get("method") == "item/agentMessage/delta":
                    observed = True
                    break
                require(message.get("method") != "turn/completed", "turn ended before cancellation")
            require(observed, "client did not receive partial output")
            rpc.call("turn/interrupt", {"threadId": thread_id, "turnId": turn["turn"]["id"]})
        calls, approvals, final = 0, 0, None
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            message = rpc.pending.pop(0) if rpc.pending else rpc.next(max(0.01, deadline - time.monotonic()))
            method = message.get("method")
            if method == "item/tool/call":
                require(name in {"function_tool", "namespace_tool"}, "unexpected dynamic tool invocation")
                require(message["params"]["tool"] == echo["name"] and message["params"]["arguments"] == {"text": "synthetic"}, "dynamic tool identity or arguments changed")
                require(message["params"].get("namespace") == ("fixture" if name == "namespace_tool" else None), "dynamic tool namespace changed")
                calls += 1
                rpc.send({"id": message["id"], "result": {"contentItems": [{"type": "inputText", "text": "synthetic-result"}], "success": True}})
            elif method in {"item/fileChange/requestApproval", "item/commandExecution/requestApproval"}:
                require(name == "approval_denial", "unexpected approval request")
                approvals += 1
                rpc.send({"id": message["id"], "result": {"decision": "decline"}})
            elif method == "turn/completed":
                final = message["params"]["turn"]["status"]
                break
            elif "id" in message and method:
                raise AssertionError("unexpected server request")
        expected = "interrupted" if name.startswith("cancellation") else "failed" if name == "transport_failure" else "completed"
        require(final == expected, "unexpected final turn status")
        require(not state.errors, "mock upstream validation failed")
        if name in {"function_tool", "namespace_tool"}:
            require(calls == 1 and state.result_seen and state.requests == 2, "function tool round trip incomplete")
        elif name == "custom_patch":
            require((workspace / "fixture.txt").read_text() == "synthetic-content\n", "custom patch was not applied correctly")
            require(state.result_seen and state.requests == 2, "custom tool result did not return")
        elif name == "approval_denial":
            require(approvals == 1 and not (workspace / "fixture.txt").exists(), "approval denial did not prevent the action")
            require(state.result_seen and state.requests == 2, "declined tool result did not return")
        elif name.startswith("cancellation"):
            require(state.disconnected.wait(5), "cancellation did not close upstream within 5 seconds")
            require(state.requests == 1, "cancelled request was retried")
        else:
            require(state.requests == 1, "unexpected extra model request")
        return {"scenario": name, "status": "passed", "turn_status": final, "upstream_requests": state.requests, "dynamic_calls": calls, "denied_approvals": approvals}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--gateway-bin", type=Path, default=ROOT / "target/debug/agent-response-gateway")
    parser.add_argument("--scenario", choices=("text", "function_tool", "namespace_tool", "custom_patch", "approval_denial", "cancellation", "cancellation_heartbeat", "transport_failure"), action="append")
    args = parser.parse_args()
    lock = json.loads(runtime.LOCK.read_text())
    binary = runtime.verify_bundle(runtime.BUNDLE, lock)
    scenarios = args.scenario or ["text", "function_tool", "namespace_tool", "custom_patch", "approval_denial", "cancellation", "cancellation_heartbeat", "transport_failure"]
    failures = 0
    for name in scenarios:
        try:
            result = run_scenario(name, binary, args.gateway_bin.resolve())
        except Exception as error:
            failures += 1
            result = {"scenario": name, "status": "failed", "error_class": type(error).__name__}
            if isinstance(error, AssertionError):
                result["check"] = str(error)  # Only static harness assertions, never payloads.
        print(json.dumps(result), flush=True)
    raise SystemExit(1 if failures else 0)


if __name__ == "__main__":
    main()
