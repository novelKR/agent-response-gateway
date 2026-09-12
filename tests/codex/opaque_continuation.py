#!/usr/bin/env python3
"""Probe opaque Responses replay with the pinned Codex and synthetic providers only.

This is a transport prerequisite, not encryption or Gemini qualification.
"""
import argparse
import json

PUBLIC_REASONING = "SYNTHETIC_REASONING_DISPLAY"
from pathlib import Path
from unittest.mock import patch

import continuity
import conformance as base


class OpaqueState(continuity.State):
    def __init__(self):
        super().__init__()
        self.expected = None
        self.checks = []
        self.summary_checks = []

    def respond(self, body):
        if self.expected is not None and self.phase not in {"after_compact", "switch"}:
            carried = [item.get("encrypted_content") for item in body.get("input", [])
                       if item.get("type") == "reasoning"]
            base.require(self.expected in carried, "opaque continuation was not replayed")
            self.checks.append(self.phase)
            matching = [item for item in body.get("input", [])
                        if item.get("encrypted_content") == self.expected]
            base.require(len(matching) == 1 and matching[0].get("summary") == [
                {"type": "summary_text", "text": PUBLIC_REASONING}],
                "public reasoning and opaque state did not survive together")
            self.summary_checks.append(self.phase)
        frames = super().respond(body)
        payload = json.loads(frames[-1].decode().split("data: ", 1)[1])
        response = payload["response"]
        self.expected = "arg-probe-v1.synthetic-" + str(self.requests)
        opaque = {"type": "reasoning", "id": "rs_probe_" + str(self.requests),
                  "summary": [{"type": "summary_text", "text": PUBLIC_REASONING}],
                  "encrypted_content": self.expected}
        response["output"].insert(0, opaque)
        return [base.event("response.created", response={**response, "status": "in_progress", "output": []}),
                base.event("response.output_item.added", output_index=0,
                           item={**opaque, "summary": []}),
                base.event("response.reasoning_summary_part.added", output_index=0,
                           item_id=opaque["id"], summary_index=0,
                           part={"type": "summary_text", "text": ""}),
                base.event("response.reasoning_summary_text.delta", output_index=0,
                           item_id=opaque["id"], summary_index=0, delta=PUBLIC_REASONING),
                base.event("response.reasoning_summary_text.done", output_index=0,
                           item_id=opaque["id"], summary_index=0, text=PUBLIC_REASONING),
                base.event("response.reasoning_summary_part.done", output_index=0,
                           item_id=opaque["id"], summary_index=0, part=opaque["summary"][0]),
                base.event("response.output_item.done", output_index=0, item=opaque),
                base.event("response.output_item.done", output_index=1, item=response["output"][1]),
                base.event("response.completed", response=response)]


def run(binary, gateway):
    state = OpaqueState()
    notifications = set()
    original_next = base.RpcClient.next

    def observe(rpc, *args, **kwargs):
        message = original_next(rpc, *args, **kwargs)
        method = message.get("method", "")
        if method in {"item/reasoning/summaryPartAdded", "item/reasoning/summaryTextDelta"}:
            notifications.add(method)
        return message

    with patch.object(continuity, "State", lambda: state), patch.object(base.RpcClient, "next", observe):
        result = continuity.run(binary, gateway)
    base.require(all(phase in state.checks for phase in ("tool", "followup", "compact", "restart")),
                 "opaque replay scenario coverage incomplete")
    base.require(state.summary_checks == state.checks, "reasoning summary coverage incomplete")
    base.require(notifications == {"item/reasoning/summaryPartAdded", "item/reasoning/summaryTextDelta"},
                 "Codex reasoning display notifications missing")
    return {**result, "schema": "gateway-opaque-conformance/v1", "opaque_checks": state.checks,
            "public_summary_checks": state.summary_checks,
            "reasoning_notifications": sorted(notifications)}


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
