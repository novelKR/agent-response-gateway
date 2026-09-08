# Messages instruction-role lowering — approval addendum

Status: proposal; no instruction lowering is applied.

## Observed boundary

The pinned 0.154.0-alpha.6 synthetic Codex text turn sends top-level instructions,
then a developer message and user messages. The gateway IR retains each original
role and position. G05 permits only declared feature support and custom-tool JSON
bridging; it does not authorize merging instruction roles silently.

Messages accepts user/assistant conversation roles and a separate system prompt,
with no distinct developer-message role. See the [Messages reference](https://platform.claude.com/docs/en/api/messages/create).
Responses retains [explicit input instruction roles](https://developers.openai.com/api/reference/typescript/resources/responses/methods/create).
The current route cannot truthfully declare native instruction-hierarchy support.
A successful mock response would not establish equivalent model behavior.

## Recommendation and exact proposed behavior

**Required for the observed Codex-to-Messages path; high confidence in the wire
mismatch, moderate confidence in model instruction adherence:** add an opt-in
`bridged_instruction_envelope` support rule for `instruction_hierarchy`.

Keep the canonical IR and native Responses path unchanged. Only a Messages route
explicitly declaring this bridge may lower a leading instruction prefix into
system text blocks. Encode the original role, source position and exact text as
JSON data, preceded by a fixed adapter instruction identifying the fields as
higher-priority application instructions. Preserve prefix order and distinguish
protocol-default, system and developer provenance. User/tool text never enters
that envelope. Reject system/developer messages after conversation content;
promoting a late message into the system prompt would change its position.

This preserves content/provenance and their priority above user text, but cannot
provide native enforcement of separate system/developer priority. Mark support
Bridged, never Native, and state that limitation in the enabled profile. No model
output substitutes for host-side tool permissions or approvals.

The profile must explicitly declare the bridge. Existing profiles/configs retain
current behavior. A missing rule rejects the request before dispatch. The target
Codex profile must also disable unsupported hosted tool search and unsupported
reasoning options; do not strip these fields at the gateway.

## Alternatives and affected boundaries

1. Keep strict native-role equivalence: ship the narrow Messages codec and reject
   this Codex profile. This is the complete safe local behavior without approval,
   but G09 cannot qualify the currently observed default profile.
2. Use the explicit bridge above: enable the intended path with the documented
   loss of native role separation and retain independent model/consumer acceptance.
3. Modify the host/Codex prompt-generation path to produce an API-specific prompt:
   larger runtime/source maintenance and qualification scope; not proposed here.

Affected components: capability support enum/validation, Messages request encoder,
profile documentation and Codex qualification fixtures. No database, authentication
mechanism, production dependency or native wire behavior changes. Implementation
cost is small-to-moderate (one lowering rule and focused regression cases);
maintenance must follow future changes to Codex instruction placement.

Risks: a target model may interpret labelled instructions differently; a later
instruction prefix layout may become unsupported; claiming exact native hierarchy
would conceal the limitation. Mitigate with explicit opt-in, role/position/content
regressions, rejection tests and separate consumer/live-model acceptance.

Validation requires exact content and order retention, user/tool exclusion,
late-instruction rejection, missing-bridge zero-dispatch, real pinned Codex with
synthetic upstream, and unchanged native passthrough tests. Mock tests verify wire
behavior only. Rollback removes the profile declaration and reverts the bridge;
no state migration is needed.

Approval authorizes only this explicit Messages bridge and its disclosed limits.
It does not approve live provider calls, production activation or role merging in
other APIs. Until approved, G07/G08 may implement the independent narrow codec and
tool/stream machinery while G09's default-profile qualification remains blocked.
