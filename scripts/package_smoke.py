#!/usr/bin/env python3
"""Execute a packaged gateway against synthetic loopback JSON upstreams only."""
import argparse
import contextlib
import json
import os
from pathlib import Path
import secrets
import subprocess
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.error import HTTPError
from urllib.request import Request, ProxyHandler, build_opener

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tests/codex"))
import embedded_contract


class Upstream(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        try:
            size = int(self.headers["Content-Length"])
            assert 0 < size < 65536
            body = json.loads(self.rfile.read(size))
            assert body["model"] == "synthetic-model"
            if self.path == "/v1/messages":
                assert self.headers.get("x-api-key") == "synthetic-upstream-key"
                assert self.headers.get("Authorization") is None
                assert self.headers.get("anthropic-version") == "2023-06-01"
                assert body["messages"][0]["content"][0]["text"] == "synthetic-input"
                value = {"type":"message", "id":"synthetic", "role":"assistant", "model":"synthetic-model", "content":[{"type":"text", "text":"synthetic-output"}], "stop_reason":"end_turn", "usage":{"input_tokens":1,"output_tokens":1}}
            elif self.path == "/v1/chat/completions":
                assert self.headers.get("Authorization") == "Bearer synthetic-upstream-key"
                assert self.headers.get("x-api-key") is None and self.headers.get("anthropic-version") is None
                assert body["messages"][0]["content"] == "synthetic-input"
                value = {"id":"synthetic", "object":"chat.completion", "created":0, "model":"synthetic-model", "choices":[{"index":0,"message":{"role":"assistant","content":"synthetic-output"},"finish_reason":"stop"}]}
            else:
                assert self.path == "/v1/responses" and body["store"] is False and body["input"] == "synthetic-input"
                assert self.headers.get("Authorization") == "Bearer synthetic-upstream-key"
                value = {"object":"response", "id":"synthetic", "status":"completed", "output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"synthetic-output"}]}]}
            self.server.requests.append(self.path)
            raw = json.dumps(value).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(raw)))
            self.end_headers()
            self.wfile.write(raw)
        except (AssertionError, KeyError, TypeError, ValueError):
            self.server.failed = True
            self.send_error(500)


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
    if process.stdout:
        process.stdout.close()


def smoke(binary, state_dir):
    state_dir.mkdir(parents=True, exist_ok=False)
    state_dir.chmod(0o700)
    env = {k:v for k,v in os.environ.items() if k in {"PATH", "LANG", "SYSTEMROOT", "LD_LIBRARY_PATH", "DYLD_LIBRARY_PATH"}}
    env["HOME"] = str(state_dir)
    with contextlib.ExitStack() as cleanup:
        server = ThreadingHTTPServer(("127.0.0.1", 0), Upstream)
        server.requests, server.failed = [], False
        threading.Thread(target=server.serve_forever, daemon=True).start()
        cleanup.callback(server.server_close)
        cleanup.callback(server.shutdown)
        config = state_dir / "gateway.toml"
        text = f'listen="127.0.0.1:0"\n[providers.mock]\nbase_url="http://127.0.0.1:{server.server_port}/v1"\napi_key_env="ARG_MOCK_KEY"\n'
        for api in ["responses", "messages", "chat_completions"]:
            text += f'[models.{api}]\nprovider="mock"\nupstream_model="synthetic-model"\napi="{api}"\n'
            if api != "responses":
                text += f'capability_profile="{api}"\nauth="{"api_key" if api == "messages" else "bearer"}"\n'
            if api == "messages":
                text += 'messages_version="2023-06-01"\n'
        for api in ["messages", "chat_completions"]:
            text += f'[capability_profiles.{api}]\nversion="1"\nprovider="mock"\nupstream_model="synthetic-model"\napi="{api}"\ncontext_window=4096\nmax_output_tokens=128\ntested_codex_version="0.154.0-alpha.6"\n'
        config.write_text(text)
        manifest = embedded_contract.inspect_manifest(binary, config, env)
        token = secrets.token_urlsafe(32)
        process = subprocess.Popen([str(binary), "serve", "--config", str(config)], env={**env,"ARG_LOCAL_TOKEN":token,"ARG_MOCK_KEY":"synthetic-upstream-key"}, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        cleanup.callback(stop, process)
        ready = embedded_contract.read_ready(process, manifest)
        client = build_opener(ProxyHandler({}))
        try:
            client.open(ready["base_url"] + "/models", timeout=5)
        except HTTPError as error:
            assert error.code == 401
        else:
            raise AssertionError("unauthenticated models access succeeded")
        assert not server.requests
        for api in ["responses", "messages", "chat_completions"]:
            request = Request(ready["base_url"] + "/responses", data=json.dumps({"model":api,"input":"synthetic-input"}).encode(), headers={"Authorization":"Bearer " + token,"Content-Type":"application/json"})
            with client.open(request, timeout=5) as response:
                output = json.load(response)
            assert output["status"] == "completed" and output["output"][0]["content"][0]["text"] == "synthetic-output"
        assert not server.failed and len(server.requests) == 3
        process.terminate()
        assert process.wait(timeout=5) == 0
    return {"status":"passed", "routes":3, "authentication":"passed", "manifest_ready_binding":"passed", "shutdown":"passed", "provider_probe":False}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--state-dir", type=Path, required=True)
    args = parser.parse_args()
    try:
        result = smoke(args.binary.resolve(), args.state_dir.resolve())
    except (OSError, ValueError, AssertionError, KeyError, TypeError, subprocess.TimeoutExpired):
        parser.exit(1, "package-smoke: failed; no provider qualification\n")
    print(json.dumps(result))
