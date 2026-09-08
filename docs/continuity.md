# Host-owned continuity contract

The approved [G15 design](continuity-design.md) keeps gateway HTTP transport
stateless. The reusable [Python contract module](../scripts/continuity_contract.py)
validates private host records and returns new revisions. It performs no I/O,
model calls, authentication, workflow approval, or automatic recovery. Python
hosts may use this optional stdlib module; the Rust gateway has no new runtime
dependency. Consumer integration and operational acceptance are tracked separately
by G17.

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
Automatic pressure thresholds need consumer acceptance in G17.
The gateway still rejects remote compact and stored-response endpoints. A new
runtime/profile needs a fresh demonstration of its resolved compaction behavior.

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

These tests do not establish real-model summary quality, provider qualification,
consumer workflow acceptance, production activation or release readiness. G17
must connect this contract to the consumer's private persistence and control
paths, including its own permissions, approvals and recovery checks.
