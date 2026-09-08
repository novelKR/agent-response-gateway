# G05 — Model routes and capability admission

Status: **proposal awaiting explicit approval; no runtime/configuration change applied**.
This document is the implementation contract for G06 and adapter activation in
G09/G12. It preserves the existing loopback and stateless transport boundary.

## Evidence and recommendation

Current HTTP requests select a provider/model and always call `/responses` with
Bearer authentication. IR capability plans are not used by that path. Actual
pinned-Codex tests exercise flat function/custom tools and a namespace container.
The initial request also carries cache/metadata/include fields and hosted-tool
declarations. Treating every raw request as the current narrower IR subset would
break native passthrough; bypassing capability checks for translated requests
would hide unsupported semantics.

**Required, high confidence:** connect declared routes and capabilities at one
translation boundary, while retaining the distinct native passthrough path.
A per-call collection of provider special cases is not a maintainable complete
alternative. The current native-only service remains a valid limited alternative.

## Public configuration additions

Keep existing provider and model fields valid. Add optional model fields:

| Field | Contract |
|---|---|
| `api` | `responses` (legacy default), `messages`, or `chat_completions` |
| `auth` | `bearer` or `api_key`; legacy Responses defaults to bearer; converted routes must declare it |
| `capability_profile` | Reference into `capability_profiles`; required for converted routes |
| `messages_version` | Required explicit upstream version header for Messages routes |

Each capability profile declares an ID/version, API, feature support, context
window, maximum output tokens and tested Codex version. Feature support uses the
existing Native/Bridged/Unsupported distinction; missing support is Unsupported.
The model's provider, API and profile must agree. Unknown fields and invalid
references fail startup. Only implemented and qualified-for-the-declared-scope
adapters may be configured for dispatch; incomplete adapters are library/test code.

Provider base URLs and `api_key_env` remain operator-controlled. The selected API
appends exactly `/responses`, `/messages` or `/chat/completions`. `api_key` sends
the provider key in `x-api-key`; `bearer` uses Authorization. Messages sends its
declared version header. Consumers cannot provide a new URL, raw upstream key or
arbitrary upstream headers. Existing URL, local authentication and no-proxy/no-
redirect/no-retry rules continue to apply.

`/v1/models` remains the enabled alias list, not a capability certificate. Do not
silently change its public shape or substitute a different provider/model.

## Runtime flow and IR additions

1. Validate local authentication, request size and shared stateless admission.
2. Resolve the configured alias once and freeze provider, actual model, API,
   credential reference, adapter/profile version and limits for that HTTP request.
3. Native Responses dispatch preserves its present JSON/SSE behavior. An absent
   profile means unqualified passthrough, not implicit proof of all capabilities.
4. Converted dispatch decodes a validated request, derives requirements once,
   plans supported bridges and rejects missing capabilities before HTTP dispatch.
5. The selected adapter encodes one request and drives the shared output-event
   state machine. Errors after streaming starts close the failed stream without
   manufacturing completion or appending another provider's output.

Extend the IR tool-definition enum with an ordered namespace group containing
its description and function/custom definitions. Effective tool identity remains
the namespace/name pair. Preserve group and child order in Responses round trips;
reject nested namespace groups. Adapter-specific flattened names belong to one
request-scoped reversible mapping shared by definitions, choices, calls and results.
Existing flat tool requests retain their representation and tests. This is a
Rust library type addition; no persistence format or consumer workflow type is added.

Custom text JSON wrapping remains a bridge. For required grammar, use a validator
selected by the exact declared grammar's content hash and version, not by a tool
name heuristic. Initially support only the verified Codex patch grammar and text
format. Unknown grammars remain Unsupported. The patch validator parses syntax
only, never reads or changes files. Bridged post-generation validation is explicitly
different from native constrained decoding. Do not emit successful tool completion
until arguments and required grammar pass. A grammar failure is a failed response.

## Codex transport fields and unsupported semantics

| Input | Converted-route policy |
|---|---|
| model, stream, store | Route/transport selection; retain stateless `store:false` policy |
| instructions, ordered input, tools, choices, limits | Explicit typed mapping and capability checks |
| client_metadata, prompt_cache_key | Declared transport/cache hints; not interpreted as model instructions or forwarded to a different API as unknown fields; their omission is documented in the route profile |
| include reasoning.encrypted_content | A request for optional opaque output, not permission to invent it; a stateless converted profile declares no opaque output |
| existing opaque reasoning input | Reject cross-protocol replay until an approved continuity adapter exists |
| hosted web/tool search | Unsupported unless a specific semantic mapping is implemented and declared; the host must select a compatible Codex profile before route qualification |
| unknown extensions or include values | Reject until an explicit rule exists |

Strict output, image, reasoning options and parallel-tool restrictions are never
deleted to obtain a successful response. If the pinned Codex profile sends a
required feature the target cannot represent, that route remains unqualified.
The transport-hint exceptions above are the complete initial exception list.

## Model limits and ownership

Enforce the requested maximum output against the selected model limit before
dispatch. Keep byte limits distinct from token limits. `context_window` supplies
the host's model-specific context/compaction configuration contract; it is not an
exact input-token counter. Do not add an invented universal tokenizer.

For G06, context configuration and output-limit checks are implemented, while
exact input-token admission remains explicitly unqualified until the selected
model has a verified counting method. Production context acceptance additionally
requires the host's final settings and counter/estimator evidence in M5/M6. A
synthetic token-count fixture is not real-model evidence. This staging must be
approved as part of this proposal; it does not mark the full context goal complete.

## Validation, compatibility and rollback

- Existing configs, native extension passthrough, numeric precision and raw SSE
  tests must pass unchanged. Unsupported configured APIs fail explicitly.
- A mock upstream receives zero requests for missing profiles/features, invalid
  tool relationships, excessive output limits and cross-origin opaque state.
- Namespace round trips, collision-free bridge names, grammar rejection and
  interleaved tool calls share the same identity invariants.
- G09/G12 must pass the actual pinned-Codex suite and record supported profiles
  before enabling the corresponding adapter. Hosted mock success is not live qualification.
- Update protocol, IR and integration docs with implementation. No new production
  dependency is authorized by this design; propose one separately if required.

Expected benefit: one testable admission and normalization boundary instead of
duplicated conditions in each provider. Cost: moderate routing/IR work plus each
adapter's wire implementation. Main risks are unsupported default Codex tools,
bridged grammar behavior and model-specific context counting; each remains an
explicit qualification gate. Roll back by reverting the implementation PR and
restoring the prior Responses-only configuration; no data migration is required.

Approval authorizes the additions and declared hint/context policies above. It
does not authorize state storage, service-mode exposure, live provider calls,
consumer activation or release. G04's pinned-runtime cancellation issue remains
a separate runtime-version approval and acceptance gate.
