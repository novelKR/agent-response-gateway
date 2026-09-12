#!/usr/bin/env python3
"""Pinned real-Codex tests against the gateway and a synthetic loopback upstream."""

import argparse
import contextlib
import hashlib
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
import embedded_contract

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


def converted_route(api, native_custom=False):
    if api == "responses_checked":
        import responses_compatibility
        return responses_compatibility.route(native_custom)
    route = """api="messages"
auth="api_key"
messages_version="2023-06-01"
capability_profile="synthetic-messages"
[capability_profiles.synthetic-messages]
version="1"
provider="mock"
upstream_model="synthetic-model"
api="messages"
context_window=32768
max_output_tokens=1024
tested_codex_version="0.154.0"
[capability_profiles.synthetic-messages.support]
instructions="native"
instruction_hierarchy="bridged_instruction_envelope"
function_tools="native"
custom_tools="bridged_custom_tool_json"
custom_grammar="bridged_codex_patch_grammar"
namespaced_tools="bridged_tool_namespace"
tool_choice="native"
parallel_tool_control="native"
max_output_tokens="native"
reasoning_effort="native"
structured_output="native"
strict_structured_output="native"
strict_tool_arguments="native"
"""
    if api == "gemini_interactions":
        import interactions_harness
        return interactions_harness.route(json.loads(runtime.LOCK.read_text())["version"])
    if api == "chat_completions":
        route = route.replace('api="messages"', 'api="chat_completions"').replace('auth="api_key"', 'auth="bearer"').replace('messages_version="2023-06-01"\n', '').replace('instruction_hierarchy="bridged_instruction_envelope"', 'instruction_hierarchy="native"').replace("synthetic-messages", "synthetic-chat")
    return route


def prepare_converted_profile(binary, home, env):
    # Derive from the verified runtime without copying its prompts into repository fixtures.
    raw = subprocess.run([str(binary), "debug", "models", "--bundled"], env=env, capture_output=True, text=True, check=True).stdout
    model = next(m for m in json.loads(raw)["models"] if m["slug"] == "gpt-5.4")
    preserved = {k: v for k, v in model.items() if k not in {"support_verbosity", "default_verbosity", "default_reasoning_level", "supported_reasoning_levels", "supports_search_tool"}}
    model.update(support_verbosity=False, default_verbosity=None, default_reasoning_level=None, supported_reasoning_levels=[], supports_search_tool=False)
    require(all(model[k] == v for k, v in preserved.items()), "host profile changed an unrelated model field")
    catalog = home / "models.json"
    catalog.write_text(json.dumps({"models": [model]}))
    config = home / "config.toml"
    content = 'model_catalog_json=' + json.dumps(str(catalog)) + "\n" + config.read_text()
    content = content.replace("[model_providers.gateway]", "model_supports_reasoning_summaries=false\n[features]\ntool_search=false\nsearch_tool=false\nmulti_agent=false\n[model_providers.gateway]")
    config.write_text(content)
    return hashlib.sha256(json.dumps(model, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def messages_frames(blocks, number):
    frames = [event("message_start", message={"id": f"msg_fixture_{number}", "type":"message", "role":"assistant", "model":"synthetic-model", "content":[], "stop_reason":None, "usage":{"input_tokens":10,"output_tokens":1}})]
    for index, block in enumerate(blocks):
        tool = block["type"] == "tool_use"
        frames.append(event("content_block_start", index=index, content_block={**block, "input":{}} if tool else {"type":"text","text":""}))
        text = json.dumps(block["input"], ensure_ascii=False) if tool else block["text"]
        cut = len(text) // 2
        for fragment in (text[:cut], text[cut:]):
            delta = {"type":"input_json_delta","partial_json":fragment} if tool else {"type":"text_delta","text":fragment}
            frames.append(event("content_block_delta", index=index, delta=delta))
        frames.append(event("content_block_stop", index=index))
    frames.extend([event("message_delta", delta={"stop_reason":"tool_use" if any(b["type"] == "tool_use" for b in blocks) else "end_turn", "stop_sequence":None}, usage={"output_tokens":3}), event("message_stop")])
    return frames


def chat_chunk(delta, number, finish=None):
    value = {"id":f"chat_fixture_{number}","object":"chat.completion.chunk","created":0,"model":"synthetic-model","choices":[{"index":0,"delta":delta,"finish_reason":finish}]}
    return ("data: " + json.dumps(value) + "\n\n").encode()


def converted_frames(state, blocks):
    if state.api == "responses_checked":
        import responses_compatibility
        return responses_compatibility.frames(blocks, state.requests, state)
    if state.api == "messages":
        return messages_frames(blocks, state.requests)
    frames = [chat_chunk({"role":"assistant","content":""}, state.requests)]
    tool_index = 0
    for block in blocks:
        if block["type"] == "tool_use":
            arguments = json.dumps(block["input"], ensure_ascii=False)
            cut = len(arguments) // 2
            frames.append(chat_chunk({"tool_calls":[{"index":tool_index,"id":block["id"],"type":"function","function":{"name":block["name"],"arguments":arguments[:cut]}}]}, state.requests))
            frames.append(chat_chunk({"tool_calls":[{"index":tool_index,"function":{"arguments":arguments[cut:]}}]}, state.requests))
            tool_index += 1
        else:
            frames.append(chat_chunk({"content":block["text"]}, state.requests))
    frames.append(chat_chunk({}, state.requests, "tool_calls" if tool_index else "stop"))
    usage = {"id":f"chat_fixture_{state.requests}","object":"chat.completion.chunk","created":0,"model":"synthetic-model","choices":[],"usage":{"prompt_tokens":10,"completion_tokens":3,"total_tokens":13}}
    frames.extend([("data: " + json.dumps(usage) + "\n\n").encode(), b"data: [DONE]\n\n"])
    return frames


CONTROL_SCHEMA = {"type":"object","properties":{"answer":{"type":"string"}},"required":["answer"],"additionalProperties":False}
CONTROL_TEXT = '{"answer":"synthetic"}'


def check_controls(body, api):
    if api == "messages":
        require(body.get("output_config") == {"effort":"high","format":{"type":"json_schema","schema":CONTROL_SCHEMA}}, "Messages explicit output controls changed")
    elif api == "chat_completions":
        require(body.get("reasoning_effort") == "high", "Chat explicit effort changed")
        output = body.get("response_format", {}).get("json_schema", {})
        require(output.get("schema") == CONTROL_SCHEMA and output.get("strict") is True and output.get("name"), "Chat explicit schema changed")
    else:
        require(body.get("reasoning", {}).get("effort") == "high", "Responses explicit effort changed")
        output = body.get("text", {}).get("format", {})
        require(output.get("schema") == CONTROL_SCHEMA and output.get("strict") is True, "Responses explicit schema changed")


def chat_fixture_view(body, managed_contract=None):
    # Only the test assertion view uses Messages-shaped blocks; production adapters stay independent.
    if managed_contract:
        require(body.get("max_tokens") == 8192 and body.get("stream_options") == {"include_usage":True}, "managed Chat wire options changed")
        require(not any(k in body for k in ("store","n","max_completion_tokens","input","instructions","text","client_metadata","include")), "unmapped fields reached managed Chat")
        roles = [m["role"] for m in body["messages"]]
        require("system" in roles and ("developer" not in roles if managed_contract == "deep_seek" else "developer" in roles), "managed Chat instruction bridge differs")
    else:
        require(body.get("store") is False and body.get("n") == 1 and body.get("max_completion_tokens") == 1024 and body.get("stream_options") == {"include_usage":True}, "Chat wire options changed")
        require(not any(k in body for k in ("max_tokens", "input", "instructions", "reasoning", "text", "client_metadata", "include")), "unmapped Responses fields leaked into Chat")
        roles = [m["role"] for m in body["messages"]]
        require("system" in roles and "developer" in roles, "Chat instruction roles missing")
    tools = []
    for tool in body.get("tools", []):
        require(tool["type"] == "function", "Chat function-wire declaration changed")
        definition = tool["function"]
        tools.append({"name":definition["name"],"description":definition.get("description", ""),"input_schema":definition["parameters"]})
    messages = []
    for message in body["messages"]:
        role = message["role"]
        if role in {"system", "developer"}:
            continue
        if role == "tool":
            blocks = [{"type":"tool_result","tool_use_id":message["tool_call_id"],"content":message["content"]}]
        else:
            content = message.get("content")
            blocks = [{"type":"text","text":content}] if isinstance(content, str) else list(content or [])
            for call in message.get("tool_calls", []):
                blocks.append({"type":"tool_use","id":call["id"],"name":call["function"]["name"],"input":json.loads(call["function"]["arguments"])})
        messages.append({"role":role,"content":blocks})
    return {**body,"tools":tools,"messages":messages}


def converted_response(state, body):
    state.requests += 1
    require(state.requests <= 2, "unexpected retry or extra model request")
    require(body.get("model") == "synthetic-model" and body.get("stream") is True, "converted routing/stream differs")
    if state.name == "output_controls" and not getattr(state, "managed_contract", None):
        check_controls(body, state.api)
    if state.api == "responses_checked":
        import responses_compatibility
        body = responses_compatibility.fixture_view(body, state)
    elif state.api == "chat_completions":
        body = chat_fixture_view(body, getattr(state,"managed_contract",None))
    else:
        require(body.get("max_tokens") == getattr(state,"output_limit",1024), "Messages output limit differs")
        require(not any(k in body for k in ("store", "input", "instructions", "reasoning", "text", "client_metadata", "prompt_cache_key", "include")), "unmapped Responses fields leaked into Messages")
        require(body.get("system") and len(body["system"]) == 2, "approved instruction envelope missing")
        records = json.loads(body["system"][1]["text"])
        require(any(r.get("role") == "developer" for r in records) and all(r.get("role") in {"protocol_default", "developer", "system"} for r in records), "instruction provenance changed")
    if getattr(state, "editing", False) and not getattr(state,"normalization",False) and state.requests == 2 and state.name in {"custom_patch", "approval_denial"}:
        calls = [b for m in body["messages"] if m["role"] == "assistant" for b in m["content"] if b.get("type") == "tool_use" and b.get("id") == "call_fixture"]
        require(len(calls) == 1 and calls[0]["name"].startswith("arg_edit_") and calls[0]["input"] == __import__("editing_fixture").edit_input(getattr(state,"operations",False),file_conflict=getattr(state,"file_conflict",False)), "structured history changed")
    if state.requests == 2:
        if state.name == "text_followup":
            require(any(m.get("role") == "assistant" and any(b.get("text") == "Synthetic complete." for b in m["content"]) for m in body["messages"]), "prior assistant text is missing")
        else:
            outputs = [b for m in body["messages"] for b in m["content"] if b["type"] == "tool_result"]
            expected = {"call_fixture_a", "call_fixture_b"} if state.name == "parallel_tools" else {"call_fixture"}
            require({o["tool_use_id"] for o in outputs} == expected and len(outputs) == len(expected), "Messages tool result identity changed")
            if state.name in {"function_tool", "namespace_tool", "parallel_tools", "mixed_tool_text", "multi_tool_turns"}:
                require(all("synthetic-result" in o["content"] for o in outputs), "Messages tool results changed")
        if state.name == "mixed_tool_text":
            assistant = [m for m in body["messages"] if m["role"] == "assistant" and any(b.get("type") == "tool_use" for b in m["content"])]
            require(len(assistant) == 1 and any(b.get("text") == "Synthetic after tool." for b in assistant[0]["content"]), "mixed assistant text lost on replay")
            if state.api == "messages":
                require([b["type"] for b in assistant[0]["content"] if not (getattr(state,"managed_contract",None) and b["type"] in {"thinking","redacted_thinking"})] == ["tool_use", "text"], "Messages block order changed on replay")
        state.result_seen = True
        return converted_frames(state, [{"type":"text","text":"Synthetic complete."}])
    if state.name in {"function_tool", "namespace_tool", "parallel_tools", "mixed_tool_text", "multi_tool_turns"}:
        if state.name == "namespace_tool":
            candidates = []
            for tool in body["tools"]:
                try:
                    description = json.loads(tool.get("description", ""))
                except ValueError:
                    continue
                if isinstance(description, dict) and description.get("namespace") == "fixture":
                    candidates.append(tool)
        else:
            candidates = [t for t in body["tools"] if t["name"] == "gateway_echo"]
        require(len(candidates) == 1, "Messages dynamic declaration missing")
        ids = ["call_fixture_a", "call_fixture_b"] if state.name == "parallel_tools" else ["call_fixture"]
        blocks = [{"type":"tool_use","id":ident,"name":candidates[0]["name"],"input":{"text":"synthetic"}} for ident in ids]
    elif state.name in {"custom_patch", "approval_denial", "grammar_failure"}:
        candidates = [t for t in body["tools"] if list(t["input_schema"].get("properties", {})) == ["input"] and t["input_schema"]["properties"]["input"].get("type") == "string"]
        require(len(candidates) == 1, "Messages custom envelope missing")
        if not getattr(state,"code_mode",False):
            description = candidates[0]["input_schema"]["properties"]["input"].get("description", "")
            prefix = "The exact text must match this grammar: "
            require(description.startswith(prefix), "registered grammar declaration missing")
            grammar = json.loads(description[len(prefix):])
            require(hashlib.sha256(grammar["definition"].encode()).hexdigest() == "d6367f4826ed608c424b0a308f3d6163527df63c22513d089b91863552f8bfeb", "pinned grammar fingerprint changed")
        patch = "*** Begin Patch\n*** Add File: fixture.txt\n+synthetic-content\n*** End Patch"
        if state.name == "grammar_failure":
            patch = "*** Begin Patch\n*** End Patch"
        if getattr(state,"normalization",False) and state.name in {"custom_patch","approval_denial"}:
            patch=patch.replace("*** Begin Patch\n","*** Begin Patch ***\n")+" ***"
        blocks = [{"type":"tool_use","id":"call_fixture","name":candidates[0]["name"],"input":{"input":patch}}]
    else:
        blocks = [{"type":"text","text":"Synthetic complete."}]
    if getattr(state, "editing", False) and not getattr(state,"normalization",False) and state.name in {"custom_patch", "approval_denial", "grammar_failure"}:
        from editing_fixture import block
        blocks = [block(body["tools"], invalid=state.name == "grammar_failure", operations=getattr(state,"operations",False), file_conflict=getattr(state,"file_conflict",False))]
    if state.name == "mixed_tool_text":
        blocks.append({"type":"text","text":"Synthetic after tool."})
    if state.name == "output_controls":
        blocks = [{"type":"text","text":CONTROL_TEXT}]
    return converted_frames(state, blocks)


class Scenario:
    def __init__(self, name, workspace, api="responses"):
        self.name, self.workspace, self.api = name, workspace, api
        self.requests = 0
        self.result_seen = False
        self.started = threading.Event()
        self.disconnected = threading.Event()
        self.disconnected_at = None
        self.stop = threading.Event()
        self.errors = []
        self.tool_contracts = set()

    def response(self, body):
        if getattr(self,"managed_contract",None):
            import reasoning_conformance
            return reasoning_conformance.respond(self,body)
        if self.api == "gemini_interactions":
            import interactions_harness
            return interactions_harness.respond(self,body)
        if self.api != "responses":
            return converted_response(self, body)
        self.requests += 1
        require(self.requests <= 2, "unexpected retry or extra model request")
        require(body.get("store") is False and body.get("model") == "synthetic-model", "gateway admission or routing differs")
        require(body.get("stream") is True, "Codex did not request SSE")
        if self.name == "output_controls":
            check_controls(body, self.api)
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
        if self.name == "output_controls":
            item["content"][0]["text"] = CONTROL_TEXT
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
            require(self.path == {"messages":"/v1/messages", "chat_completions":"/v1/chat/completions", "responses":"/v1/responses", "responses_checked":"/v1/responses", "gemini_interactions":"/v1/interactions"}[state.api], "unexpected upstream endpoint")
            if state.api == "gemini_interactions":
                require(self.headers.get("x-goog-api-key") == "synthetic-upstream-key" and self.headers.get("Authorization") is None, "Interactions authentication differs")
            elif state.api == "messages":
                require(self.headers.get("x-api-key") == "synthetic-upstream-key" and self.headers.get("Authorization") is None, "Messages credential selection differs")
                require(self.headers.get("anthropic-version") == "2023-06-01", "Messages version header differs")
            else:
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
                if state.api == "gemini_interactions":
                    import interactions_harness as ih
                    partial_frames=[ih.event("step.start",index=0,step={"type":"model_output"}), ih.event("step.delta",index=0,delta={"type":"text","text":"Synthetic partial."})]
                elif state.api == "messages":
                    partial_frames = [
                        event("content_block_start", index=0, content_block={"type":"text","text":""}),
                        event("content_block_delta", index=0, delta={"type":"text_delta","text":"Synthetic partial."}),
                    ]
                elif state.api == "chat_completions":
                    partial_frames = [chat_chunk({"content":"Synthetic partial."}, state.requests)]
                else:
                    partial = {"id": "msg_fixture", "type": "message", "role": "assistant", "status": "in_progress", "content": []}
                    partial_frames = [
                        event("response.output_item.added", output_index=0, item=partial),
                        event("response.content_part.added", item_id="msg_fixture", output_index=0, content_index=0, part={"type": "output_text", "text": "", "annotations": []}),
                        event("response.output_text.delta", item_id="msg_fixture", output_index=0, content_index=0, delta="Synthetic partial."),
                    ]
                if getattr(state,"managed_contract",None) and state.name == "cancellation_heartbeat":
                    partial_frames=[]
                checked_sequence = 1
                if state.api == "responses_checked":
                    def checked_frame(frame):
                        nonlocal checked_sequence
                        value = json.loads(frame.split(b"data: ",1)[1])
                        value["sequence_number"] = checked_sequence
                        checked_sequence += 1
                        return event(value.pop("type"), **value)
                    partial_frames = [checked_frame(frame) for frame in partial_frames]
                for frame in partial_frames:
                    self.wfile.write(frame)
                self.wfile.flush()
                while not state.stop.wait(0.05):
                    if state.name == "cancellation_heartbeat":
                        frame = b": synthetic keepalive\n\n"
                    elif state.api == "gemini_interactions":
                        frame=ih.event("step.delta",index=0,delta={"type":"text","text":" synthetic"})
                    elif state.api == "messages":
                        frame = event("content_block_delta", index=0, delta={"type":"text_delta","text":" synthetic"})
                    else:
                        frame = event("response.output_text.delta", item_id="msg_fixture", output_index=0, content_index=0, delta=" synthetic")
                    if state.api == "responses_checked" and not frame.startswith(b":"):
                        frame = checked_frame(frame)
                    self.wfile.write(frame)
                    self.wfile.flush()
            elif state.name == "transport_failure":
                return  # Deliberately omit the final completion event.
            else:
                for frame in frames[1:]:
                    self.wfile.write(frame)
                    self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            state.disconnected_at = time.monotonic()
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


def run_scenario(name, binary, gateway_binary, api="responses", managed_contract=None, native_custom=False, profile_packs=False, codec_binary=None, editing=False, code_mode=False, normalization=False, operations=False, file_conflict=False):
    local = ROOT / ".local/conformance"
    local.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=name + "-", dir=local) as temporary, contextlib.ExitStack() as cleanup:
        root = Path(temporary)
        workspace = root / "workspace"
        workspace.mkdir()
        home = root / "codex-home"
        home.mkdir()
        state = Scenario(name, workspace, api)
        state.operations = operations
        state.file_conflict = file_conflict
        state.normalization = normalization
        state.code_mode = code_mode
        state.editing = editing
        if editing and not normalization and name in {"custom_patch", "approval_denial"}:
            (workspace / "fixture.txt").write_text("synthetic-old\n")
        if operations:
            for path, content in [('deleted.txt','synthetic-delete'),('source.txt','synthetic-moved')]:
                (workspace/path).write_text(content+'\n')
        state.native_custom=native_custom
        state.native_custom_names=set()
        state.managed_contract=managed_contract
        state.output_limit=8192 if managed_contract else 1024
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
        if managed_contract:
            import reasoning_conformance
            config.write_text(config.read_text() + reasoning_conformance.route(managed_contract))
        elif api != "responses":
            config.write_text(config.read_text() + converted_route(api, native_custom))
        if api == "gemini_interactions" or managed_contract:
            import interactions_harness as ih
            host_token=ih.setup(root,gateway_binary,config,gateway_env)
        if editing:
            from editing_fixture import configure
            config.write_text(configure(config.read_text(), code_mode=code_mode).replace('normalization="none"', 'normalization="patch-envelope/v1"').replace('representation="context-lines/v1"','representation="patch-text/v1"') if normalization else configure(config.read_text(), code_mode=code_mode))
            if operations: config.write_text(config.read_text().replace('representation="context-lines/v1"','representation="operations/v1"'))
            if name == "contract_failure":
                raw=config.read_text();lines=raw.splitlines();lines=[('client_descriptor_sha256="'+'0'*64+'"') if line.startswith('client_descriptor_sha256=') else line for line in lines];config.write_text('\n'.join(lines)+'\n')
        gateway_args = ["--config", str(config)]
        if profile_packs:
            from profile_pack_fixture import activate
            packed, pack_lock = activate(gateway_binary, root / "profile-packs", config.read_text())
            config.write_text(packed)
            gateway_args += ["--profile-packs-lock", str(pack_lock)]
        if codec_binary:
            from codec_fixture import activate as activate_codec
            encoded,codec_lock=activate_codec(codec_binary,root/'codec',config.read_text(), editing=editing)
            config.write_text(encoded)
            gateway_args += ['--extensions-lock',str(codec_lock)]
        manifest = embedded_contract.inspect_manifest(gateway_binary, config, env, gateway_args[2:])
        gateway = subprocess.Popen([str(gateway_binary), "serve", *gateway_args], env=gateway_env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        cleanup.callback(stop_process, gateway)
        ready = embedded_contract.read_ready(gateway, manifest)
        if codec_binary: manifest=manifest["configuration"]["gateway"]
        (home / "config.toml").write_text(f'model="gpt-5.4"\nmodel_provider="gateway"\nweb_search="disabled"\nmodel_context_window=32768\nmodel_auto_compact_token_limit=24576\n[model_providers.gateway]\nname="Synthetic gateway"\nbase_url="{ready["base_url"]}"\nwire_api="responses"\nenv_key="ARG_CODEX_TEST_TOKEN"\nrequires_openai_auth=false\nsupports_websockets=false\nrequest_max_retries=0\nstream_max_retries=0\n')
        if api == "gemini_interactions" or managed_contract:
            session=ih.create_session(ready["base_url"],host_token,manifest)
            with (home/"config.toml").open("a") as f:f.write("http_headers="+json.dumps({"x-gateway-session":session["id"]}).replace(": "," = ")+"\n")
        codex_env = {**env, "HOME": str(home), "CODEX_HOME": str(home), "ARG_CODEX_TEST_TOKEN": token}
        embedded_contract.validate_credential_split(manifest, gateway_env, codex_env, "ARG_CODEX_TEST_TOKEN", home)
        profile_digest = None
        if api != "responses":
            profile_digest = prepare_converted_profile(binary, home, codex_env)
        if code_mode:
            path=home/'config.toml';content=path.read_text()
            content=content.replace('[features]', '[features]\ncode_mode=true\ncode_mode_only=true\napps=false')
            path.write_text(content)
        codex = subprocess.Popen([str(binary), "app-server"], cwd=workspace, env=codex_env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, bufsize=1)
        cleanup.callback(stop_process, codex)
        rpc = RpcClient(codex)
        rpc.call("initialize", {"clientInfo": {"name": "arg_conformance", "version": "0.1.0"}, "capabilities": {"experimentalApi": True}})
        rpc.send({"method": "initialized", "params": {}})
        echo = {"type": "function", "name": "echo" if name == "namespace_tool" else "gateway_echo", "description": "Return a synthetic fixture value.", "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"], "additionalProperties": False}}
        dynamic = {"type": "namespace", "name": "fixture", "description": "Synthetic tool group.", "tools": [echo]} if name == "namespace_tool" else echo
        dynamic_tools=[dynamic]
        if managed_contract and name=="namespace_tool":
            dynamic_tools.append({"type":"namespace","name":"second","description":"Synthetic collision namespace","tools":[echo]})
        thread = rpc.call("thread/start", {"model": "gpt-5.4", "modelProvider": "gateway", "cwd": str(workspace), "sandbox": "workspace-write" if name == "custom_patch" else "read-only", "approvalPolicy": "on-request", "approvalsReviewer": "user", "ephemeral": True, "allowProviderModelFallback": False, "experimentalRawEvents": False, "dynamicTools": dynamic_tools})
        thread_id = thread["thread"]["id"]
        turn_started = time.monotonic()
        first_text_ms, interrupt_at = None, None
        params = {"threadId": thread_id, "input": [{"type": "text", "text": "Exercise the synthetic fixture."}]}
        if name == "output_controls":
            params.update(effort="high", outputSchema=CONTROL_SCHEMA)
        turn = rpc.call("turn/start", params)
        if name.startswith("cancellation"):
            require(state.started.wait(10), "upstream did not start")
            heartbeat_only=managed_contract and name=="cancellation_heartbeat"
            deadline = time.monotonic() + 10
            observed = bool(heartbeat_only)
            if heartbeat_only: time.sleep(0.1)
            while not heartbeat_only and time.monotonic() < deadline:
                message = rpc.pending.pop(0) if rpc.pending else rpc.next(max(0.01, deadline - time.monotonic()))
                if message.get("method") == "item/agentMessage/delta":
                    observed = True
                    first_text_ms = round((time.monotonic() - turn_started) * 1000, 3)
                    break
                require(message.get("method") != "turn/completed", "turn ended before cancellation")
            require(observed, "client did not receive partial output")
            interrupt_at = time.monotonic()
            rpc.call("turn/interrupt", {"threadId": thread_id, "turnId": turn["turn"]["id"]})
        calls, approvals, final = 0, 0, None
        reasoning_notifications=0
        final_text = None
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            message = rpc.pending.pop(0) if rpc.pending else rpc.next(max(0.01, deadline - time.monotonic()))
            method = message.get("method")
            if method == "item/reasoning/summaryTextDelta":
                reasoning_notifications+=1
                require("synthetic_signature_" not in json.dumps(message) and "synthetic_encrypted_" not in json.dumps(message), "private reasoning reached display")
            if method == "item/agentMessage/delta" and first_text_ms is None:
                first_text_ms = round((time.monotonic() - turn_started) * 1000, 3)
            if method == "item/completed" and message["params"]["item"].get("type") == "agentMessage":
                final_text = message["params"]["item"].get("text")
            if method == "item/tool/call":
                require(name in {"function_tool", "namespace_tool", "parallel_tools", "mixed_tool_text", "multi_tool_turns"}, "unexpected dynamic tool invocation")
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
                if name == "text_followup" and state.requests == 1:
                    require(final == "completed", f"first text turn did not complete (requests={state.requests})")
                    rpc.call("turn/start", {"threadId": thread_id, "input": [{"type":"text","text":"Continue the synthetic fixture."}]})
                    continue
                break
            elif "id" in message and method:
                raise AssertionError("unexpected server request")
        turn_elapsed_ms = round((time.monotonic() - turn_started) * 1000, 3)
        rejected_controls=managed_contract=="deep_seek" and name=="output_controls"
        expected = "interrupted" if name.startswith("cancellation") else "failed" if name in {"transport_failure", "grammar_failure", "contract_failure"} or rejected_controls else "completed"
        require(final == expected, f"unexpected final turn status (requests={state.requests}, calls={calls})")
        require(not state.errors, "mock upstream validation failed")
        if name == "contract_failure":
            require(state.requests == 0 and calls == 0 and approvals == 0, "client mismatch reached execution")
        elif rejected_controls:
            require(state.requests==0 and calls==0 and approvals==0, "unsupported controls reached inference")
        elif name == "output_controls":
            require(json.loads(final_text) == {"answer":"synthetic"} and state.requests == 1, "explicit schema output did not reach Codex")
        elif name in {"function_tool", "namespace_tool", "mixed_tool_text"}:
            require(calls == 1 and state.result_seen and state.requests == 2, "function tool round trip incomplete")
        elif name == "multi_tool_turns":
            require(calls == 2 and state.result_seen and state.requests == 3, "multiple tool turns incomplete")
        elif name == "parallel_tools":
            require(calls == 2 and state.result_seen and state.requests == 2, "parallel tool round trip incomplete")
        elif name == "text_followup":
            require(state.result_seen and state.requests == 2, "text history did not survive a new turn")
        elif name == "grammar_failure":
            require(calls == 0 and approvals == 0 and not (workspace / "fixture.txt").exists(), "invalid grammar reached tool execution")
            require(state.requests == 1, "failed grammar request was retried")
        elif name == "custom_patch":
            require((workspace / "fixture.txt").read_text() == ("synthetic-old\n" if file_conflict else "synthetic-content\n"), "custom patch result differs")
            if operations and not file_conflict:
                require((workspace/'created.txt').read_text()=='synthetic-created\n' and not (workspace/'deleted.txt').exists() and not (workspace/'source.txt').exists() and (workspace/'moved.txt').read_text()=='synthetic-moved\n','independent operations differ')
            require(state.result_seen and state.requests == 2, "custom tool result did not return")
        elif name == "approval_denial":
            require(approvals == 1 and ((workspace / "fixture.txt").read_text() == "synthetic-old\n" if editing and not normalization else not (workspace / "fixture.txt").exists()), "approval denial did not prevent the action")
            require(state.result_seen and state.requests == 2, "declined tool result did not return")
        elif name.startswith("cancellation"):
            require(state.disconnected.wait(max(0, interrupt_at + 5 - time.monotonic())), "cancellation did not close upstream within 5 seconds")
            require(state.disconnected_at - interrupt_at <= 5, "upstream closure exceeded the interrupt bound")
            require(state.requests == 1, "cancelled request was retried")
        else:
            require(state.requests == 1, "unexpected extra model request")
        result = {"api": api, "scenario": name, "status": "passed", "turn_status": final, "upstream_requests": state.requests, "dynamic_calls": calls, "denied_approvals": approvals}
        if operations:
            result['operations']=True
            result['file_conflict']=file_conflict
            if name=='approval_denial':
                require(not (workspace/'created.txt').exists() and (workspace/'deleted.txt').exists() and (workspace/'source.txt').exists() and not (workspace/'moved.txt').exists(),'denied bundle changed files')
        result["turn_elapsed_ms"] = turn_elapsed_ms
        if managed_contract:
            result["reasoning_contract"]=managed_contract
            if name=="namespace_tool":result["namespace_collision"]=True
            result["reasoning_notifications"]=reasoning_notifications
            if expected=="completed":require(reasoning_notifications>0,"managed reasoning display missing")
            if name=="cancellation_heartbeat":result["heartbeat_without_output"]=True
            if rejected_controls:result["request_rejected_before_upstream"]=True
        if first_text_ms is not None:
            result["first_client_text_ms"] = first_text_ms
        if interrupt_at is not None:
            result["interrupt_to_upstream_close_ms"] = round((state.disconnected_at - interrupt_at) * 1000, 3)
        if codec_binary:
            result["external_codec"] = True
        if profile_packs:
            result["profile_packs"] = True
            result["gateway_configuration_sha256"] = manifest["configuration_sha256"]
        if profile_digest:
            result["host_profile_sha256"] = profile_digest
        return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--gateway-bin", type=Path, default=ROOT / "target/debug/agent-response-gateway")
    parser.add_argument("--codex-bundle", type=Path, default=runtime.BUNDLE)
    parser.add_argument("--api", choices=("responses", "responses_checked", "messages", "chat_completions", "gemini_interactions"), action="append")
    parser.add_argument("--scenario", choices=("text", "function_tool", "namespace_tool", "custom_patch", "approval_denial", "cancellation", "cancellation_heartbeat", "transport_failure", "parallel_tools", "grammar_failure", "text_followup", "output_controls", "mixed_tool_text", "multi_tool_turns"), action="append")
    parser.add_argument("--responses-native-custom", action="store_true", help="Retain native custom input while checking the registered grammar output")
    parser.add_argument("--profile-packs", action="store_true", help="Import synthetic declarations from pinned non-executable packages")
    parser.add_argument("--codec-bin", type=Path)
    args = parser.parse_args()
    require(not args.profile_packs or (args.api and "responses" not in args.api), "profile pack fixture requires an explicitly profiled API")
    require(not args.responses_native_custom or args.api == ["responses_checked"], "native custom fixture requires checked Responses")
    lock = json.loads(runtime.LOCK.read_text())
    binary = runtime.verify_bundle(args.codex_bundle, lock)
    failures = 0
    for api in args.api or ["responses", "messages", "chat_completions"]:
        scenarios = args.scenario or ["text", "function_tool", "namespace_tool", "custom_patch", "approval_denial", "cancellation", "cancellation_heartbeat", "transport_failure", "output_controls"]
        if args.scenario is None and api != "responses":
            scenarios += ["parallel_tools", "grammar_failure", "text_followup", "mixed_tool_text"]
        if args.scenario is None and api == "gemini_interactions":
            scenarios += ["multi_tool_turns"]
        for name in scenarios:
            try:
                require(api != "responses" or name not in {"parallel_tools", "grammar_failure", "text_followup", "mixed_tool_text"}, "scenario requires converted API")
                result = run_scenario(name, binary, args.gateway_bin.resolve(), api, native_custom=args.responses_native_custom, profile_packs=args.profile_packs, codec_binary=args.codec_bin)
            except Exception as error:
                failures += 1
                result = {"api": api, "scenario": name, "status": "failed", "error_class": type(error).__name__}
                if isinstance(error, AssertionError):
                    result["check"] = str(error)  # Only static harness assertions, never payloads.
            print(json.dumps(result), flush=True)
    raise SystemExit(1 if failures else 0)


if __name__ == "__main__":
    main()
