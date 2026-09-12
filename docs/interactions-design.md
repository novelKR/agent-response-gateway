<a id="gemini-interactions-구현-계약"></a>
<a id="데이터와-실행-소유권"></a>
<a id="복구와-압축"></a>
<a id="호환성과-수락"></a>

<a id="gemini-interactions-design"></a>

# Gemini Interactions implementation contract

[English](interactions-design.md) | [한국어](ko/interactions-design.md)

The runtime has an opt-in Interactions route and durable SQLite
continuation. It requires host-created sessions and separate protection/control
credentials. After local compaction, the host transfers checked portable context
to a new Codex thread and explicitly activates a new continuation epoch.
The provider contract is pinned in the
[wire lock](../tests/interactions/wire-lock.json). The [opaque probe](../tests/codex/opaque_continuation.py)
tests the pinned Codex using synthetic Responses providers, not Gemini or encryption.

## Data and execution ownership

Responses enters through the common IR and an internal Rust Interactions adapter.
Foreground calls explicitly use store:false. Client tools remain with the host.
Provider steps and signatures are preserved independently from their public output
projection. Schema constraints and thinking levels must not be silently weakened.

A backend-neutral continuation contract owns session, epoch, attempt, finalized
record and encrypted payload. SQLite is the first backend; PostgreSQL is future work.
The host supplies a stable protection key separately from model and control tokens.
Codex carries an authenticated encrypted copy of each response's new provider steps.
The server normally loads its own copy and verifies the client's output binding.

## Recovery and compaction

Commit an attempt before dispatch. Persist finalized output and its replay payload
before publishing executable tool completions, recovery data or terminal success.
A finalized record permits repair of a missing payload from authenticated client
history. A missing execution record, pending attempt or unknown outcome does not.
The host must reconcile uncertain work explicitly; never silently resend inference.

The host creates sessions and registers local compaction through a separately
authenticated loopback control interface. Epoch transitions require checked portable
history and completed tool results; they must not claim preservation of lost provider
state. Codex 0.154.0 reinserts developer instructions after the compacted history.
The instruction bridge rejects such mid-conversation instructions. The host retains
the original thread, verifies the summary and completed tool results, and places them
in one portable user message in a fresh thread. Its canonical message digest is
committed to the new epoch. The gateway requires this message exactly once, accepts
the fresh thread's leading instructions, and authenticates the complete input prefix
on subsequent turns. New sessions are never inferred from missing database records.

## Compatibility and acceptance

Existing routes remain independent. Database schema, provider wire, protected
payload and host manifest identities are versioned. Recovery needs compatible
Codex history, database and protection key. Public response storage/lookup/deletion,
previous_response_id, provider storage, background execution, remote compaction,
managed agents and native hosted tools remain separate capabilities.

Implementation acceptance requires pinned-Codex tool/namespace/patch round trips,
restart, compaction, finalized-payload repair, uncertain-outcome rejection, and the
existing repository gates. Live-model qualification and consumer acceptance are
separate. The opaque probe is a prerequisite, not evidence of a finished adapter.
