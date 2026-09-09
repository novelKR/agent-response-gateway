<a id="g15--호스트-이력에-기반한-연속성"></a>

# G15 — Continuity with host-owned history

[English](continuity-design.md) | [한국어](ko/continuity-design.md)

Status: **explicitly approved on 2026-09-08; implementation and acceptance tracked by G16/G17**.

<a id="근거와-권고"></a>

## Evidence and recommendation

The gateway currently rejects stored-response IDs and remote compact endpoints.
That does not by itself require a gateway database for long-running Codex work.
The pinned Codex selects local compaction when the configured provider declares
remote compaction unsupported. A synthetic control-plane probe completed an
initial turn, `thread/compact/start`, and a following turn through three ordinary
`/v1/responses` calls; no gateway compact endpoint was needed.

The same three-call local-compaction probe also passed with the separately
approved temporary 0.154.0-alpha.6 test baseline.

Source: [pinned compaction task selection](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/core/src/tasks/compact.rs).
This is evidence for the tested custom-provider profile, not every model/profile
or a proof that a real model produces a sufficient summary.

**Strongly recommended, high confidence for ownership:** retain Codex/host-owned
history and the gateway's stateless transport. First qualify local compaction and
complete-history replay. A new gateway history database, provider-ID emulation or
encrypted summary envelope is not required by this observed path and is excluded
from the initial implementation.

<a id="책임과-인터페이스"></a>

## Responsibilities and interfaces

| Owner | Responsibility |
|---|---|
| Codex | Thread history, local compaction, tool result context, resume control protocol |
| Host runtime | Verified executable/settings, run binding, cancellation, recovery and explicit model switching |
| Gateway | Frozen request route, provider credential selection, protocol conversion and stateless errors |
| Consumer | Workflow checkpoints, approvals, domain meaning and operational acceptance |

Use existing `thread/start`, `thread/resume`, `thread/compact/start`, `turn/start`
and `turn/interrupt` control interfaces. No gateway storage, response lookup or
compact HTTP endpoint is enabled by this proposal. Unsupported stateful requests
continue to fail rather than being forwarded to a nonexistent provider ID.

<a id="실행-binding과-복구-기록"></a>

## Run binding and recovery record

The host stores a private, versioned `gateway-run-binding/v1` record alongside its
existing execution record, containing:

- Codex version, executable digest and declared state compatibility.
- Gateway version/digest and the exact effective route configuration digest.
- Resolved provider, actual model, API, capability profile and adapter versions.
- Credential realm and generation reference; never the raw key in the record.
- Thread identity, private history reference, last completed turn and recovery state.
- Context/output limits, compaction policy and the run's retry/request budget.

Write a new record atomically after a completed transition. Keep the previous
record and compatible history backup for recovery. Do not rewrite completed
execution evidence or infer workflow approval from a model response.

Before resume, verify the executable, state compatibility, route/profile and
credential generation. The host must obtain the credential generation from its
authoritative credential owner; reusing an environment-variable name alone is
insufficient. If that binding cannot be established, reject same-context resume.
Do not silently resolve an old alias using changed configuration.

<a id="지원하는-연속-실행-경로"></a>

## Supported continuation paths

1. **Same process and route:** Codex sends the complete current context and tool
   results. The gateway translates only the declared supported stateless subset.
2. **Local compaction:** the host uses a verified provider profile whose resolved
   remote-compaction capability is Unsupported. Codex owns the summarization
   request and replacement history. The gateway handles the ordinary Responses
   call without pretending the summary is native encrypted state.
3. **Process restart:** restore a compatible Codex home/history and the matching
   run binding, resume the recorded thread, and verify the resumed model/provider
   before starting another turn. Do not replay already-completed tools.
4. **Explicit model change:** require a host transition, select a newly verified
   route, and create a fresh context from portable messages and completed tool
   results. Record omitted opaque state and the new binding. Preserve approvals
   and workflow checkpoints in their existing owner.

A provider profile that requests remote compaction is not eligible for this local
compaction contract. It remains unsupported until a separate adapter is designed
and approved. A new Codex version must demonstrate which path it actually uses;
do not rely on a provider's display name or on old observed behavior.

<a id="불투명-상태취소불확실성"></a>

## Opaque state, cancellation and uncertainty

Preserve native opaque state only within a verified identical origin binding.
Cross-protocol opaque replay remains an error. Reasoning summaries are ordinary
portable content only when explicitly represented as such; they are not a
substitute for signed or encrypted provider state. No new cryptographic envelope
or persistent key-management mechanism is introduced here.

Cancellation must pass the G04 idle/heartbeat case as well as active-event streams.
An interrupted control turn alone is insufficient. The observed 0.153.4 defect
is addressed through a separately reviewed runtime baseline, not fabricated
events or a hidden gateway timeout workaround.

After a send-before-response disconnect, record outcome Unknown. Do not replay a
request automatically merely because no final event was observed. The host owns
the retry budget and recovery decision; the gateway continues to make one
upstream attempt. Compaction is not triggered while tool results or approvals are
pending in the host's workflow.

The [host contract implementation](continuity.md) defines executable record
validation and synthetic conformance. Consumer persistence and operational
acceptance remain separate.

<a id="g16g17-수락과-마이그레이션"></a>

## G16/G17 acceptance and migration

- Add reusable, synthetic continuation tests for tool result replay, explicit
  compaction, resume after restart and exact route/profile mismatch rejection.
- Verify a required sentinel and completed tool result survive compaction and
  restart; this does not certify real-model summarization or literary quality.
- Verify that a changed credential generation, binary/state compatibility or
  route configuration blocks resume before any provider request.
- Verify explicit switching records state loss, starts a new context and does
  not execute completed tools or inherit unapproved operations.
- Verify cancellation and unknown-outcome behavior across process restart.
- Consumer operational acceptance and live model qualification remain separate
  from mock tests and GitHub checks.

Existing gateway requests and configuration remain compatible. The new private
host record is opt-in for gateway-backed runs; existing records without a binding
are not silently upgraded into verified resume records. Their owner may establish
a binding through a reviewed migration, or start a fresh context. Rollback restores
the previous verified executable/settings/history combination; never apply older
code blindly to a newer incompatible state directory.

Expected benefit: reuse the existing history owner and avoid duplicate state
stores and response-ID namespaces. Cost: moderate host integration and recovery
tests, with small generic gateway contract changes only if G05 requires them.
Risks: summary quality, credential-generation availability and runtime state
compatibility. These are explicit acceptance gates, not defaults assumed true.

Alternative: gateway-owned state and translated native compaction could support
additional clients, but would require an approved storage format, encryption/key
lifecycle, authenticated origin binding and migrations. Defer that larger change
until a demonstrated client requirement cannot use this host-owned path.

Approval accepts the ownership and recovery contract above. It does not approve
consumer activation, commercial rights, a runtime prerelease promotion, a new
database or production deployment.
