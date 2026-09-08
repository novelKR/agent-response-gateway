"""Synthetic host state transitions; no provider calls or private history."""

import copy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("continuity_contract", Path(__file__).resolve().parents[1] / "continuity_contract.py")
c = importlib.util.module_from_spec(spec)
spec.loader.exec_module(c)


def identity():
    return {"runtime": {"version": "test", "binary_sha256": "1" * 64, "state_compatibility": "synthetic/v1"},
            "gateway": {"version": "test", "binary_sha256": "2" * 64, "configuration_sha256": "3" * 64},
            "route": {"alias": "synthetic", "codex_provider": "gateway", "provider_id": "mock",
                      "upstream_model": "synthetic-upstream", "api": "responses", "adapter_version": "responses/v1",
                      "profile_id": "synthetic", "profile_version": "1", "profile_sha256": "4" * 64,
                      "context_window": 32768, "max_output_tokens": 1024},
            "credential_owner": {"realm": "mock", "generation": "1"},
            "policy": {"compaction": "local-only", "remote_compaction": "unsupported",
                       "max_host_requests": 8, "primary_transport_retries": 0}}


def thread():
    return {"id": "synthetic-thread", "provider": "gateway", "history_reference": "private/history.jsonl", "history_sha256": "5" * 64}


class ContinuityTests(unittest.TestCase):
    def test_declared_but_unmaterialized_history_cannot_resume(self):
        declared = {**thread(), "history_sha256": None}
        created = c.create(identity(), declared)
        with self.assertRaises(c.ContinuityError):
            c.resume(created, identity(), declared)
        pending = c.begin(created, "turn")
        with self.assertRaises(c.ContinuityError):
            c.complete(pending, declared, "turn-1")
        completed = c.complete(pending, thread(), "turn-1")
        self.assertEqual(c.resume(completed, identity(), thread())["threadId"], thread()["id"])

    def test_completed_turn_and_compaction_preserve_tool_identity_and_prior_records(self):
        original = c.create(identity(), thread())
        saved = copy.deepcopy(original)
        pending = c.begin(original, "turn")
        changed = {**thread(), "history_sha256": "6" * 64}
        completed = c.complete(pending, changed, "turn-1", completed_tool_ids=["tool-1"])
        compact = c.begin(completed, "compact")
        compacted = c.complete(compact, {**changed, "history_sha256": "7" * 64}, "compact-1")
        self.assertEqual(original, saved)
        self.assertEqual(compacted["state"]["completed_tool_ids"], ["tool-1"])
        self.assertEqual(compacted["transition"]["kind"], "compacted")
        self.assertEqual(compacted["requests_issued"], 2)
        self.assertEqual(compacted["previous_sha256"], c.sha256(compact))
        self.assertEqual(c.resume(compacted, identity(), compacted["thread"])["threadId"], "synthetic-thread")

    def test_resume_checks_every_origin_field_and_history_before_requests(self):
        record = c.create(identity(), thread())
        for group, values in identity().items():
            for key, value in values.items():
                with self.subTest(group=group, key=key):
                    changed = identity()
                    if type(value) is int:
                        changed[group][key] = value + 1
                    elif key.endswith("sha256"):
                        changed[group][key] = "a" * 64
                    else:
                        changed[group][key] = "changed"
                    with self.assertRaises(c.ContinuityError):
                        c.resume(record, changed, thread())
        for key in thread():
            changed = thread()
            changed[key] = "a" * 64 if key.endswith("sha256") else "changed"
            with self.subTest(history=key), self.assertRaises(c.ContinuityError):
                c.resume(record, identity(), changed)

    def test_pending_unknown_and_cancelled_requests_never_implicitly_replay(self):
        record = c.begin(c.create(identity(), thread()), "turn")
        for pending in (record, c.interrupted(record, upstream_closed=False), c.interrupted(record, upstream_closed=True)):
            with self.subTest(state=pending["state"]["status"]):
                with self.assertRaises(c.ContinuityError):
                    c.resume(pending, identity(), thread())
                with self.assertRaises(c.ContinuityError):
                    c.begin(pending, "turn")
                self.assertEqual(pending["requests_issued"], 1)

    def test_no_compaction_with_pending_host_operations_or_remote_profile(self):
        record = c.create(identity(), thread())
        for kwargs in ({"pending_tools": True}, {"pending_approvals": True}, {"pending_tools": 0}):
            with self.assertRaises(c.ContinuityError):
                c.begin(record, "compact", **kwargs)
        changed = identity()
        changed["policy"]["remote_compaction"] = "native"
        with self.assertRaises(c.ContinuityError):
            c.create(changed, thread())

    def test_recovery_requires_reconciliation_and_preserves_budget_and_tool_evidence(self):
        first = c.interrupted(c.begin(c.create(identity(), thread()), "turn"), upstream_closed=False)
        first_recovered = c.recover(first, identity(), thread(), None,
            decision_reference="synthetic-first-turn-cancel", pending_tools=False, pending_approvals=False)
        self.assertIsNone(first_recovered["state"]["last_completed_turn"])
        self.assertEqual(first_recovered["requests_issued"], 1)
        c.resume(first_recovered, identity(), thread())
        old = c.complete(c.begin(c.create(identity(), thread()), "turn"), thread(), "turn-1", completed_tool_ids=["tool-1"])
        unknown = c.interrupted(c.begin(old, "turn"), upstream_closed=False)
        kwargs = {"decision_reference": "synthetic-host-decision", "completed_tool_ids": ["tool-1", "tool-2"],
                  "pending_tools": False, "pending_approvals": False}
        restored = {**thread(), "history_sha256": "8" * 64}
        recovered = c.recover(unknown, identity(), restored, "verified-turn-2", **kwargs)
        self.assertEqual(recovered["requests_issued"], 2)
        self.assertEqual(recovered["transition"]["recovery_reference"], "synthetic-host-decision")
        self.assertEqual(recovered["previous_sha256"], c.sha256(unknown))
        self.assertEqual(recovered["state"]["completed_tool_ids"], ["tool-1", "tool-2"])
        c.resume(recovered, identity(), restored)
        for changes in ({"pending_tools": True}, {"pending_approvals": True}, {"completed_tool_ids": ["tool-2"]},
                        {"decision_reference": ""}):
            with self.subTest(changes=changes), self.assertRaises(c.ContinuityError):
                c.recover(unknown, identity(), restored, "verified-turn-2", **{**kwargs, **changes})

    def test_completed_tools_are_not_reexecuted_and_request_budget_is_bounded(self):
        current = identity()
        current["policy"]["max_host_requests"] = 2
        record = c.complete(c.begin(c.create(current, thread()), "turn"), thread(), "turn-1", completed_tool_ids=["tool-1"])
        pending = c.begin(record, "turn")
        with self.assertRaises(c.ContinuityError):
            c.admit_tool(pending, "tool-1")
        c.admit_tool(pending, "tool-2")
        with self.assertRaises(c.ContinuityError):
            c.complete(pending, thread(), "turn-2", completed_tool_ids=["tool-1"])
        record = c.complete(pending, thread(), "turn-2")
        with self.assertRaises(c.ContinuityError):
            c.begin(record, "compact")

    def test_explicit_switch_binds_portable_context_and_records_opaque_state_loss(self):
        old = c.complete(c.begin(c.create(identity(), thread()), "turn"), thread(), "turn-1", completed_tool_ids=["tool-1"])
        new_identity = identity()
        new_identity["route"].update(alias="new-alias", api="messages", upstream_model="new-model")
        new_thread = {**thread(), "id": "fresh-thread", "history_reference": "private/new-history.jsonl"}
        context = [{"type": "message", "role": "user", "text": "synthetic sentinel"},
                   {"type": "completed_tool_result", "call_id": "tool-1", "result": "synthetic completed result"}]
        new = c.switch(old, new_identity, new_thread, context, omitted_state=["opaque_reasoning", "provider_response_ids"])
        self.assertEqual(new["revision"], 1)
        self.assertEqual(new["transition"]["source_sha256"], c.sha256(old))
        self.assertEqual(new["transition"]["portable_context_sha256"], c.sha256(context))
        self.assertEqual(new["state"]["completed_tool_ids"], ["tool-1"])
        self.assertNotIn("synthetic completed result", c.canonical(new).decode())
        self.assertEqual(c.resume(new, new_identity, new_thread)["model"], "new-alias")
        for invalid in ([{"type": "reasoning", "encrypted_content": "opaque"}],
                        [{"type": "completed_tool_result", "call_id": "unknown", "result": "x"}]):
            with self.assertRaises(c.ContinuityError):
                c.switch(old, new_identity, new_thread, invalid, omitted_state=[])
        with self.assertRaises(c.ContinuityError):
            c.switch(old, new_identity, thread(), context, omitted_state=[])

    def test_raw_credentials_extra_fields_and_unsafe_history_are_rejected(self):
        current = identity()
        current["credential_owner"]["api_key"] = "synthetic-secret"
        with self.assertRaises(c.ContinuityError):
            c.create(current, thread())
        for reference in ("/absolute", "../outside", "private/../../outside", "private//history", "private\\history", "private/history\n"):
            with self.subTest(path=reference), self.assertRaises(c.ContinuityError):
                c.create(identity(), {**thread(), "history_reference": reference})
        invalid = c.create(identity(), thread())
        invalid["state"]["status"] = "unknown"
        with self.assertRaises(c.ContinuityError):
            c.validate_record(invalid)


if __name__ == "__main__":
    unittest.main()
