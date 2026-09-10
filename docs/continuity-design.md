<a id="g15--continuity-with-host-owned-history"></a>
<a id="g15--호스트-이력에-기반한-연속성"></a>
<a id="이력압축재개"></a>

# History, compaction and resume

[English](continuity-design.md) | [한국어](ko/continuity-design.md)

Codex and its host keep conversation history and recovery records. The gateway
handles stateless model requests; it does not store responses or provide a remote
compaction endpoint.

<a id="evidence-and-recommendation"></a>
<a id="근거와-권고"></a>
<a id="로컬-압축"></a>

## Local compaction

With a provider profile that declares remote compaction unsupported, the pinned
Codex uses local compaction. It sends a summarization request through the ordinary
`/v1/responses` endpoint and updates its own history. The host invokes
`thread/compact/start` and decides when another turn can begin.

This path is covered by mock-provider tests. The actual model's summary quality
and the application's ability to continue from that summary require validation
with the intended workload.

<a id="책임과-인터페이스"></a>

## Responsibilities and interfaces

| Owner | Responsibility |
|---|---|
| Codex | Conversation history, local compaction, tool-result context and resume control |
| Host runtime | Verified executables/settings, run binding, cancellation, recovery and explicit model changes |
| Gateway | Fixed request route, provider credentials, protocol conversion and stateless errors |
| Application workflow | Checkpoints, user approval and domain decisions |

Use `thread/start`, `thread/resume`, `thread/compact/start`, `turn/start` and
`turn/interrupt`. Stored-response references and remote compaction requests remain
unsupported at the gateway.

<a id="실행-binding과-복구-기록"></a>
<a id="실행-연결-정보와-복구-기록"></a>

## Run binding and recovery record

A run binding is the record of the exact runtime, route, credentials and history
that may be used together. The host stores a private `gateway-run-binding/v1`
record containing:

- Codex version, executable digest and state compatibility.
- Gateway version/digest and effective configuration digest.
- Provider, actual model, API, capability profile and adapter versions.
- Credential realm and generation reference, without the raw key.
- Thread identity, private history reference, last completed turn and recovery state.
- Context/output limits, compaction policy and request/retry budget.

After a completed transition, save the new record atomically and retain the
previous compatible record/history for recovery. Before resume, verify these
bindings against the current executables, configuration and credential manager.
Reusing the same environment-variable name is not proof that the credential is
unchanged. Reject an unverified binding instead of silently resolving an old alias
with new settings.

<a id="지원하는-연속-실행-경로"></a>

## Supported continuation paths

1. **Same process and route:** send the complete current context and tool results
   through the route's supported stateless request format.
2. **Local compaction:** let Codex generate the summary and replace its history.
   The summary is ordinary content, not provider-encrypted state.
3. **Process restart:** restore compatible Codex settings/history and the recorded
   run binding. Verify the resumed model/provider before starting another turn.
   Do not rerun completed tools.
4. **Explicit model change:** select a verified route and start a fresh context
   from portable messages and completed tool results. Record omitted opaque state
   and the new binding; retain workflow checkpoints and approvals separately.

A profile requiring remote compaction cannot use this local-compaction contract.
Verify the selected Codex version's effective behavior when changing runtimes.

<a id="opaque-state-cancellation-and-uncertainty"></a>
<a id="불투명-상태와-결과-불명"></a>
<a id="불투명-상태취소불확실성"></a>

## Opaque state and uncertain outcomes

Opaque provider state is data the gateway does not interpret, such as encrypted
reasoning. Preserve it only within its verified origin binding. Cross-protocol
reuse is rejected; a text summary cannot substitute for signed or encrypted state.

Cancellation tests must cover both active-event streams and streams sending only
heartbeat comments. Stopping the control turn must also close its upstream model
connection within the tested bound.

If a request was sent but its response is unknown, record the outcome as Unknown.
The host decides recovery and any retry within its budget; the gateway makes one
upstream attempt. Do not compact while tool results or approvals are pending.
The executable checks and journal format are defined in the [continuity contract](continuity.md).

<a id="g16g17-acceptance-and-migration"></a>
<a id="g16g17-수락과-마이그레이션"></a>
<a id="검증과-복구"></a>

## Validation and recovery

Test tool-result replay, compaction, restart, route/profile mismatches, credential
changes, model switching, cancellation and uncertain outcomes. Confirm that required
context and completed tool results survive compaction and restart without duplicate
execution or inherited unapproved actions.

A record without a verified binding cannot be resumed as trusted history. Establish
its binding through a checked migration or start fresh. Recovery restores a verified
executable/settings/history combination; do not run older code against incompatible
newer state. Validate model summaries and host recovery with the intended workload
before production use.
