#!/usr/bin/env python3
"""Probe opaque Responses replay with the pinned Codex and synthetic providers only.

This is a transport prerequisite, not encryption or Gemini qualification.
"""
import argparse
import json
from pathlib import Path
from unittest.mock import patch

import continuity
import conformance as base


class OpaqueState(continuity.State):
    def __init__(self):
        super().__init__()
        self.expected = None
        self.checks = []

    def respond(self, body):
        if self.expected is not None and self.phase not in {"after_compact", "switch"}:
            carried = [item.get("encrypted_content") for item in body.get("input", [])
                       if item.get("type") == "reasoning"]
            base.require(self.expected in carried, "opaque continuation was not replayed")
            self.checks.append(self.phase)
        frames = super().respond(body)
        payload = json.loads(frames[-1].decode().split("data: ", 1)[1])
        response = payload["response"]
        self.expected = "arg-probe-v1.synthetic-" + str(self.requests)
        opaque = {"type": "reasoning", "id": "rs_probe_" + str(self.requests),
                  "summary": [], "encrypted_content": self.expected}
        response["output"].insert(0, opaque)
        return [base.event("response.created", response={**response, "status": "in_progress", "output": []}),
                base.event("response.output_item.done", output_index=0, item=opaque),
                base.event("response.output_item.done", output_index=1, item=response["output"][1]),
                base.event("response.completed", response=response)]


def run(binary, gateway):
    state = OpaqueState()
    with patch.object(continuity, "State", lambda: state):
        result = continuity.run(binary, gateway)
    base.require(all(phase in state.checks for phase in ("tool", "followup", "compact", "restart")),
                 "opaque replay scenario coverage incomplete")
    return {**result, "schema": "gateway-opaque-conformance/v1", "opaque_checks": state.checks}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--gateway-bin", type=Path, default=base.ROOT / "target/debug/agent-response-gateway")
    parser.add_argument("--codex-bundle", type=Path, default=base.runtime.BUNDLE)
    args = parser.parse_args()
    lock = json.loads(base.runtime.LOCK.read_text())
    binary = base.runtime.verify_bundle(args.codex_bundle, lock)
    try:
        print(json.dumps(run(binary, args.gateway_bin.resolve()), sort_keys=True))
    except Exception:
        print(json.dumps({"schema": "gateway-opaque-conformance/v1", "status": "failed",
                          "phase": "opaque_replay", "provider_qualification": False}))
        raise SystemExit(1) from None


if __name__ == "__main__":
    main()
