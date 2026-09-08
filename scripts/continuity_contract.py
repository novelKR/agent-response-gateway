"""Pure host-owned continuity records for the approved gateway-run-binding/v1 contract.

This module performs no I/O or model calls. The host verifies current executables,
manifest, credential-owner generation and history, then writes each returned
record atomically to its own private journal before dispatching another request.
The gateway HTTP process remains stateless. Records contain metadata, never keys,
prompts, tool results, or approval decisions.
"""

import copy
import hashlib
import json
import re

SCHEMA = "gateway-run-binding/v1"
HEX = re.compile(r"[0-9a-f]{64}\Z")


class ContinuityError(ValueError):
    pass


def require(condition, reason):
    if not condition:
        raise ContinuityError(reason)


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False).encode()


def sha256(value):
    return hashlib.sha256(canonical(value)).hexdigest()


def fields(value, names):
    require(isinstance(value, dict) and set(value) == set(names.split()), "invalid continuity object fields")
    return value


def label(value):
    require(isinstance(value, str) and 0 < len(value) <= 256 and value.strip() == value
            and all(ord(c) >= 32 and ord(c) != 127 for c in value), "invalid continuity identifier")
    return value


def hexadecimal(value):
    require(isinstance(value, str) and HEX.fullmatch(value) is not None, "invalid continuity digest")
    return value


def positive(value):
    require(type(value) is int and value > 0, "invalid positive continuity limit")
    return value


def validate_identity(value):
    fields(value, "runtime gateway route credential_owner policy")
    runtime = fields(value["runtime"], "version binary_sha256 state_compatibility")
    label(runtime["version"])
    label(runtime["state_compatibility"])
    hexadecimal(runtime["binary_sha256"])
    gateway = fields(value["gateway"], "version binary_sha256 configuration_sha256")
    label(gateway["version"])
    for name in ("binary_sha256", "configuration_sha256"):
        hexadecimal(gateway[name])
    route = fields(value["route"], "alias codex_provider provider_id upstream_model api adapter_version profile_id profile_version profile_sha256 context_window max_output_tokens")
    for name in ("alias", "codex_provider", "provider_id", "upstream_model", "adapter_version", "profile_id", "profile_version"):
        label(route[name])
    require(route["api"] in {"responses", "messages", "chat_completions"}, "unsupported continuity API")
    hexadecimal(route["profile_sha256"])
    require(positive(route["max_output_tokens"]) < positive(route["context_window"]), "invalid continuity context limits")
    owner = fields(value["credential_owner"], "realm generation")
    label(owner["realm"])
    label(owner["generation"])
    policy = fields(value["policy"], "compaction remote_compaction max_host_requests primary_transport_retries")
    require(policy["compaction"] == "local-only" and policy["remote_compaction"] == "unsupported",
            "remote compaction is outside this continuity contract")
    require(type(policy["primary_transport_retries"]) is int and policy["primary_transport_retries"] == 0,
            "uncertain requests cannot be automatically retried")
    positive(policy["max_host_requests"])
    return copy.deepcopy(value)


def validate_thread(value):
    fields(value, "id provider history_reference history_sha256")
    label(value["id"])
    label(value["provider"])
    reference = value["history_reference"]
    require(isinstance(reference, str) and reference and not reference.startswith("/")
            and "\\" not in reference and all(part not in {"", ".", ".."} for part in reference.split("/"))
            and all(ord(c) >= 32 and ord(c) != 127 for c in reference), "invalid private history reference")
    # A newly created Codex thread declares its path before materializing the
    # rollout. Null is explicitly unmaterialized, never an empty-file digest.
    if value["history_sha256"] is not None:
        hexadecimal(value["history_sha256"])
    return copy.deepcopy(value)


def validate_record(record):
    fields(record, "schema identity thread revision previous_sha256 requests_issued state transition")
    require(record["schema"] == SCHEMA, "unsupported continuity record")
    identity = validate_identity(record["identity"])
    thread = validate_thread(record["thread"])
    require(thread["provider"] == identity["route"]["codex_provider"], "thread provider differs from binding")
    positive(record["revision"])
    if record["previous_sha256"] is not None:
        hexadecimal(record["previous_sha256"])
    require((record["revision"] == 1) == (record["previous_sha256"] is None), "invalid continuity revision chain")
    require(type(record["requests_issued"]) is int and 0 <= record["requests_issued"] <= identity["policy"]["max_host_requests"],
            "host request budget exceeded")
    state = fields(record["state"], "status pending_kind last_completed_turn completed_tool_ids")
    require(state["status"] in {"ready", "in_flight", "unknown", "cancelled"}, "invalid recovery state")
    require(state["pending_kind"] in {None, "turn", "compact"}, "invalid pending request kind")
    require((state["status"] in {"in_flight", "unknown"}) == (state["pending_kind"] is not None), "inconsistent pending recovery state")
    if state["last_completed_turn"] is not None:
        label(state["last_completed_turn"])
        require(thread["history_sha256"] is not None, "completed turn requires materialized history")
    tools = state["completed_tool_ids"]
    require(isinstance(tools, list) and len(tools) <= 4096 and len(set(map(label, tools))) == len(tools), "invalid completed tool identity set")
    transition = fields(record["transition"], "kind source_sha256 portable_context_sha256 omitted_state recovery_reference")
    require(transition["kind"] in {"created", "started", "completed", "compacted", "unknown", "cancelled", "switched", "recovered"}, "invalid continuity transition")
    for name in ("source_sha256", "portable_context_sha256"):
        if transition[name] is not None:
            hexadecimal(transition[name])
    require(isinstance(transition["omitted_state"], list)
            and all(item in {"opaque_reasoning", "provider_response_ids", "pending_operations"} for item in transition["omitted_state"])
            and len(set(transition["omitted_state"])) == len(transition["omitted_state"]), "invalid omitted state record")
    if transition["kind"] == "recovered":
        label(transition["recovery_reference"])
    else:
        require(transition["recovery_reference"] is None, "unexpected recovery decision reference")
    if transition["kind"] == "switched":
        require(transition["source_sha256"] is not None and transition["portable_context_sha256"] is not None,
                "model switch requires source and portable context bindings")
    else:
        require(transition["source_sha256"] is None and transition["portable_context_sha256"] is None
                and transition["omitted_state"] == [], "unexpected model switch metadata")
    return copy.deepcopy(record)


def create(identity, thread):
    return validate_record({"schema": SCHEMA, "identity": identity, "thread": thread, "revision": 1,
        "previous_sha256": None, "requests_issued": 0,
        "state": {"status": "ready", "pending_kind": None, "last_completed_turn": None, "completed_tool_ids": []},
        "transition": {"kind": "created", "source_sha256": None, "portable_context_sha256": None, "omitted_state": [], "recovery_reference": None}})


def _next(record, kind):
    value = validate_record(record)
    value.update(revision=value["revision"] + 1, previous_sha256=sha256(record))
    value["transition"] = {"kind": kind, "source_sha256": None, "portable_context_sha256": None, "omitted_state": [], "recovery_reference": None}
    return value


def resume(record, identity, thread):
    """Verify before thread/resume; also compare the resumed model/provider before turn/start."""
    record = validate_record(record)
    require(record["identity"] == validate_identity(identity), "continuity origin binding changed")
    require(record["thread"] == validate_thread(thread), "continuity history binding changed")
    require(thread["history_sha256"] is not None, "unmaterialized history cannot resume")
    require(record["state"]["status"] == "ready", "continuation requires an explicit recovery decision")
    return {"threadId": record["thread"]["id"], "model": identity["route"]["alias"],
            "modelProvider": identity["route"]["codex_provider"]}


def begin(record, kind, *, pending_tools=False, pending_approvals=False):
    require(kind in {"turn", "compact"}, "unsupported host request kind")
    require(pending_tools is False and pending_approvals is False, "host has unfinished tools or approvals")
    value = _next(record, "started")
    require(value["state"]["status"] == "ready", "request outcome requires an explicit recovery decision")
    value["requests_issued"] += 1
    value["state"].update(status="in_flight", pending_kind=kind)
    return validate_record(value)


def complete(record, thread, completed_turn, *, completed_tool_ids=()):
    record = validate_record(record)
    require(record["state"]["status"] == "in_flight", "no pending request to complete")
    value = _next(record, "compacted" if record["state"]["pending_kind"] == "compact" else "completed")
    current = validate_thread(thread)
    require(current["history_sha256"] is not None, "completed request requires materialized history")
    require(all(current[k] == record["thread"][k] for k in ("id", "provider", "history_reference")),
            "completion changed thread identity")
    value["thread"] = current
    ids = list(completed_tool_ids)
    require(record["state"]["pending_kind"] != "compact" or not ids, "compaction cannot complete new tools")
    require(not set(ids).intersection(value["state"]["completed_tool_ids"]), "completed tool cannot execute twice")
    value["state"]["completed_tool_ids"].extend(ids)
    value["state"].update(status="ready", pending_kind=None, last_completed_turn=label(completed_turn))
    return validate_record(value)


def admit_tool(record, call_id):
    """For hosts that dispatch tools: check before execution, never only afterward."""
    record = validate_record(record)
    require(record["state"]["status"] == "in_flight" and record["state"]["pending_kind"] == "turn",
            "tool dispatch requires an active ordinary turn")
    require(label(call_id) not in record["state"]["completed_tool_ids"], "completed tool cannot be dispatched again")


def interrupted(record, *, upstream_closed):
    value = _next(record, "cancelled" if upstream_closed is True else "unknown")
    require(value["state"]["status"] == "in_flight", "no pending request to interrupt")
    require(type(upstream_closed) is bool, "upstream closure must be observed")
    value["state"]["status"] = "cancelled" if upstream_closed else "unknown"
    if upstream_closed:
        value["state"]["pending_kind"] = None
    return validate_record(value)


def switch(record, identity, thread, portable_context, *, omitted_state):
    """An explicit host transition; portable context is text and completed tool results only."""
    previous = validate_record(record)
    require(previous["state"]["status"] == "ready", "unknown or cancelled work needs separate recovery before switching")
    require(isinstance(portable_context, list) and bool(portable_context), "portable context is required")
    known = set(previous["state"]["completed_tool_ids"])
    for item in portable_context:
        require(isinstance(item, dict), "invalid portable context item")
        if item.get("type") == "message":
            fields(item, "type role text")
            require(item["role"] in {"user", "assistant"} and isinstance(item["text"], str), "invalid portable message")
        else:
            fields(item, "type call_id result")
            require(item["type"] == "completed_tool_result" and item["call_id"] in known
                    and isinstance(item["result"], str), "pending tools and opaque state are not portable")
    value = create(identity, thread)
    require(value["thread"]["id"] != previous["thread"]["id"], "model switching requires a fresh thread")
    value["state"]["completed_tool_ids"] = list(previous["state"]["completed_tool_ids"])
    value["transition"] = {"kind": "switched", "source_sha256": sha256(previous),
                           "portable_context_sha256": sha256(portable_context), "omitted_state": list(omitted_state), "recovery_reference": None}
    return validate_record(value)


def recover(record, identity, thread, completed_turn, *, decision_reference, completed_tool_ids=(),
            pending_tools, pending_approvals):
    """Bind a host-approved reconciliation to independently verified compatible history.

    A reference is a host audit identity, not proof of user approval. The caller
    owns approval, checks unresolved tool outcomes, verifies the history bytes,
    and supplies the complete tool identity set before using this transition.
    """
    previous = validate_record(record)
    require(previous["state"]["status"] in {"unknown", "cancelled"}, "recovery requires an interrupted request")
    require(pending_tools is False and pending_approvals is False, "recovery has unfinished tools or approvals")
    require(previous["identity"] == validate_identity(identity), "recovery cannot change the origin binding")
    current = validate_thread(thread)
    require(current["history_sha256"] is not None, "recovery requires materialized history")
    require(all(current[k] == previous["thread"][k] for k in ("id", "provider", "history_reference")),
            "recovery cannot change the thread identity")
    ids = list(completed_tool_ids)
    require(set(previous["state"]["completed_tool_ids"]).issubset(ids), "recovery cannot forget completed tools")
    value = _next(previous, "recovered")
    value["thread"] = current
    value["state"].update(status="ready", pending_kind=None, last_completed_turn=label(completed_turn), completed_tool_ids=ids)
    value["transition"]["recovery_reference"] = label(decision_reference)
    return validate_record(value)
