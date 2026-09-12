<a id="호스트가-소유하는-연속성-계약"></a>

# Host-owned continuity contract

[English](continuity.md) | [한국어](ko/continuity.md)

This is the current host-owned history configuration, not a permanent ban on
optional continuity services in the [product](index.md). Provider continuation,
public Response storage, application workflows and usage ledgers have different
purposes. Future protocol-state modules need explicit origin, access, retention
and recovery contracts; the module described here does not implement them.
The optional [Usage Recorder](usage-accounting.md) persists usage, not conversation
history. References below to a stateless gateway concern model-session state.

The optional [Python continuity module](../scripts/continuity_contract.py) validates
host-owned run records and returns new revisions. It uses the standard library
and performs no I/O or model calls. The host owns persistence, authentication,
approval and recovery; the Rust gateway remains stateless.
See the [history and resume design](continuity-design.md) for the overall flow.

<a id="검증된-입력과-비공개-저널"></a>

## Verified inputs and private journal

`gateway-run-binding/v1` contains the exact Codex binary/state compatibility,
gateway binary/configuration, resolved route/profile/adapter/limits, credential
owner realm/generation, thread/history identity, completed tool IDs, recovery
state and host request budget. It contains no prompts, tool results, keys, key
fingerprints or approval decisions.

The host derives current identity from its verified executables, embedded
manifest, authoritative credential owner and private history. A record alone
cannot establish those facts. Before `thread/resume`, compare all current inputs
with `resume(record, identity, thread)`. Then compare the control response's actual
model/provider with the returned expectation before `turn/start`. Changing an
alias mapping, binary, profile, state compatibility, credential generation or
history digest fails the comparison.

The host must serialize transitions and atomically write each returned revision
into its private journal. Preserve previous revisions and compatible history
backups. Each subsequent revision binds the previous record digest. An atomic
current pointer must be written only after its revision has been durably saved;
a stale pointer must never allow a pending request to be replayed. The caller
owns filesystem containment, private file modes, locking, crash reconciliation
and backup restoration. The synthetic harness demonstrates immutable revisions
and an atomic current pointer, while production persistence belongs to the host.

A newly created Codex thread can declare a history path before its first rollout
file exists. `history_sha256: null` explicitly means unmaterialized history.
It cannot qualify a completed turn or resume. Bind the actual bytes after the
first completed request. The pinned alpha supports `thread/read` metadata with
`includeTurns: false`; requesting its turn listing reports an unsupported
operation. No unsupported turn listing is needed to hash the private history file.

<a id="전이와-복구"></a>

## Transitions and recovery

- `create`: bind a newly created thread and the verified current origin.
- `begin`: save an in-flight revision before sending a turn or compaction request.
  Reject pending tools/approvals and exhausted host request budgets.
- `complete`: bind observed completed history and turn identity; retain completed
  tool IDs and reject duplicate completed IDs. Hosts that dispatch tools use
  `admit_tool` before execution as well. Codex-owned tool execution remains under
  Codex's permissions; this metadata module does not execute or approve tools.
- `interrupted`: record `cancelled` only when upstream closure was observed;
  otherwise retain `unknown`. A pending record found after a crash is uncertain.
  Neither state can resume or retry automatically.
- `recover`: require a separate host reconciliation reference, unchanged origin,
  independently verified compatible history and the complete tool identity set.
  Unresolved tools/approvals block recovery. The reference is an audit identity,
  not evidence of user approval; approval remains the caller's responsibility.
  If the first request was interrupted before any completed turn, preserve
  `last_completed_turn: null` instead of inventing a completion identity.
- `switch`: require an explicit host transition and a fresh thread. Bind portable
  user/assistant text and completed tool results by digest, retain completed tool
  IDs, and record omitted opaque state and the source record digest. Pending
  operations and encrypted reasoning cannot enter portable context.

The budget counts **host control requests**, including explicit compaction. It
does not claim to count internal reviewer calls, all HTTP attempts or actual
billing. Primary transport retries remain zero. Provider/reviewer attempt bounds,
model-specific qualification and live cost limits need their own acceptance.

Only verified local compaction is eligible: `remote_compaction` must be
`unsupported` and `compaction` must be `local-only`. The pinned Codex performs
the summary through an ordinary Responses request and owns replacement history.
This policy allows explicit host compaction and Codex-triggered local compaction;
it does not claim every internal compaction is a separate host control request.
The host records the final resulting history digest after the control turn.
Validate automatic compaction thresholds with the application and selected model.
The gateway still rejects remote compact and stored-response endpoints. A new
runtime/profile needs a fresh demonstration of its resolved compaction behavior.

<a id="재현-가능한-검증"></a>

## Reproducible validation

```sh
python3 -B -m unittest discover -s scripts/tests -p 'test_continuity_contract.py' -v
python3 -B scripts/codex_runtime.py prepare
cargo build --locked
python3 -B tests/codex/continuity.py
```

The actual pinned Codex probe uses only synthetic loopback upstreams. It checks a
function tool result across a following turn, explicit local compaction, another
turn, gateway/Codex restart, and explicit model switching into a fresh thread.
A required sentinel and completed tool result must be present in each applicable
upstream request. The completed tool executes once; six changed origin bindings
are rejected before any new request. No remote compact endpoint is called.
The public result contains only versions, statuses and request counters.

Connect the module to the application's private journal, permissions, approvals
and recovery controls. Test real-model summaries and crash recovery with the
intended workload before production use.
