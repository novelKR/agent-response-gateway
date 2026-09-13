<a id="helper와-정규화-계약"></a>
<a id="검증"></a>
<a id="독립-작업-묶음"></a>
<a id="스트리밍재생버전"></a>
<a id="직접-문맥-편집"></a>
<a id="책임과-활성화"></a>
<a id="편집-호환-계약"></a>
<a id="editing-compatibility-contract"></a>

# Editing compatibility contract

[English](editing-design.md) | [한국어](ko/editing-design.md)

This contract includes implemented direct context editing and separately planned
normalization and bundle representations. Explicit helper execution is supported. Existing custom string bridging
remains the default. The synthetic fixture verifies direct patch execution and Code Mode helper
execution with pinned Codex 0.154.0; it does not qualify any real provider.

## Ownership and activation

The host selects a verified client contract. A model explicitly selects
`editing_policy` from `editing_policies`; policy definition alone never activates
it. CLI and Rust router construction use the same validated configuration.
The gateway converts and validates representations. Codex owns permission,
approval, file access and actual application. No editing process, filesystem
executor, automatic retry or provider-name inference is introduced.

The policy names `version`, `client_contract`, `representation`, `patch_dialect`
and `normalization`. Missing policy preserves existing behavior. Definitions and
the actual request must agree. A request without an editing tool gains none.
Unknown contracts fail before dispatch. Explicit named tool choices do not gain
synthetic alternatives. The original tool remains available for existing history.

## Direct context editing

`codex-direct-custom/v1` identifies the pinned custom patch contract;
`context-lines/v1` proposes one file and one context change. Its required fields
are `path`, `before_context`, `old_lines`, `new_lines`, `after_context`. Line arrays
contain strings without embedded newlines. Unknown or duplicate keys, no-op edits,
empty edits, delimiter injection and newline-only changes reject. Whitespace and
Unicode are preserved. The compiler emits a deterministic `codex-patch/1` patch
and verifies its registered grammar. File matching remains the host's decision.

One synthetic function call maps to one original custom call. The request registry
owns collision-free names, namespaces, choices and call/result linkage. Canonical
patches round-trip to their context input; unrepresentable old patches retain the
original tool path. Managed replay must authenticate the provider-original call
and the publicly restored call, including any new mapping metadata.

## Helper and normalization contracts

`codex-code-mode/v1` requires the pinned custom exec declaration and host-selected
helper contract. It emits one fixed helper wrapper with a JSON-serialized patch
string. It never executes or generally parses JavaScript. Only generated wrappers
are inverted; arbitrary programs, comments and embedded patch literals remain
unchanged. Exec output is the whole program result, not invented helper success.
Undeclared top-level calls are not repaired.

`normalization=none` is the default. A later explicit envelope rule may remove only
the extra trailing delimiter on the outer begin/end lines of one complete patch.
It must not change body lines, paths or environment selection. Revalidate grammar
after normalization. Do not recover fences, prose, partial patches or other tools.

## Independent operation bundles

The later `operations/v1` representation supports ordered create, delete, move
and context update operations. A bundle becomes one patch and one execution result.
Repeated paths, move dependencies and detectable lexical aliases reject. No inode
identity, atomicity, rollback or per-file success is inferred without host evidence.
Sequential edits dependent on earlier changes remain outside this contract.

## Streaming, replay and versions

Buffer editing arguments within existing limits until conversion and validation
finish. Preserve preceding text progress and item order. Do not expose executable
completion before continuation finalization and any durable recorder final ACK.
EOF, cancellation and failures never synthesize completion or repeat inference.

Use `gateway-editing-policy/v1` for policy meaning and
`gateway-embedded-manifest/v7`, `gateway-ready/v7`,
`gateway-extended-manifest/v7`, `gateway-extended-ready/v7` for opted-in execution.
Use `gateway-api-codec/v2` and `gateway-profile-pack/v2` for editing-aware codecs
and packs. `gateway-continuation/v3` remains reserved for future mapping metadata;
current reversible edits use existing replay v2. Old configurations
and v1/v2 replay retain their original byte contracts. Do not migrate database
tables or rewrite ciphertext automatically. New route origins bind every selected
policy and implementation version. Rollback requires a compatible binary, policy,
package selection, database, key and host history.

## Validation

The source test `tests/codex/editing_contract.py` accepts `--gateway-bin` and
`--runtime-dir`. It verifies runtime bytes, uses a loopback synthetic provider and
checks both application and approval denial for direct and helper calls. Published
fixtures contain contract hashes and synthetic metadata, never copied runtime
prompts. Native and external codecs, inline and imported policies, stateless and
managed history must preserve the same public contract when implemented.

<a id="using-direct-context-editing"></a>
<a id="직접-문맥-편집-사용"></a>

## Using direct context editing

The following model fragment requires a separately defined provider and capability
profile with native function support plus the existing custom and grammar bridges.
Values are synthetic, not a qualified provider configuration.

```toml
[models.writer]
provider="mock"
upstream_model="synthetic-model"
api="messages"
auth="api_key"
messages_version="2023-06-01"
capability_profile="verified-functions"
editing_policy="line-edit"

[editing_policies.line-edit]
version=1
client_contract="codex-direct-custom/v1"
representation="context-lines/v1"
patch_dialect="codex-patch/1"
normalization="none"
```

`old_lines` must contain at least one line. This version replaces or removes an
existing line block; insertion-only edits use the original patch tool. Limits are
16384 total lines and 8 MiB of compiled patch. No trimming or line-ending repair
occurs. Normalization and operation bundles remain planned representations.

Synthetic names are deterministic over original tool identity and policy; a name
collision rejects instead of reassigning a historical provider name. Canonical
patches invert exactly. Old noncanonical patches remain on the original tool path.
Managed provider-original content and restored public output already fit replay v2;
this representation adds no new replay fields and does not claim v3 support.
The editing policy is bound into the route origin. Changes require a new session.

CLI and library callers configure the same Config and router path. Selecting this
policy yields the reserved manifest/readiness v7 contract. Pure compiler output is
not permission to bypass router admission, output validation or host approval.

<a id="editing-pack-and-codec-integration"></a>
<a id="편집-팩과-codec-통합"></a>

## Editing pack and codec integration

`gateway-profile-pack/v2` adds named `editing_policies` exports. The host selects
an export through `editing_policy_imports`, then selects that alias on its model.
Use the existing explicit pack installation and activation flow. Version v1 packs
reject editing fields and retain their original bytes. Mixed old and new packs
are allowed; aliases never override inline policies.

```toml
[editing_policy_imports.line-edit]
pack="synthetic-fixture"
export="editing-0"
```

The corresponding configuration projection is `gateway-profile-pack-configuration/v2`.
Pack bytes, export identity and policy bind the route origin. Changing them does
not authorize reuse of an existing session. Evidence remains publisher claims;
the supplied integration fixtures are synthetic only.

Build the reference editing codec with `cargo build --locked --example api_codec_editing`.
The existing package command accepts `--role api_codec --codec-protocol gateway-api-codec/v2`.
Its two existing payload/transform permissions are unchanged. Select the package
explicitly with the model and extension lock. Codec v1 remains available through
the existing reference executable and cannot accept editing policies, even null
editing fields in its prepare request. Codec v2 carries the selected policy in
its prepare contract and uses the shared pure compiler; core output validation,
usage observation and durable completion remain authoritative.

Combined editing executions retain manifest/readiness v7. Unknown versions and
mismatched package protocols reject before inference. Tests cover exact restored
calls, approval denial, invalid edits, restart with original native history, stale
package origins, recorder admission failure and final ACK failure. Neither helper
program execution nor a new database is introduced by this integration.

The compiled host example [embedded_editing](../examples/embedded_editing.rs) uses
the ordinary router with a host-owned runtime, listener and shutdown signal. It
does not initialize library-global logging or modify process environment.

<a id="explicit-code-mode-editing"></a>
<a id="명시적-code-mode-편집"></a>

## Explicit Code Mode editing

The host selects `codex-code-mode/v1` and supplies `client_descriptor_sha256`, the
SHA-256 of the exact UTF-8 exec tool description for its verified runtime/tool set.
It must obtain this from its own qualified configuration, not trust a hash claimed
by the incoming request. Dynamic helpers change the description, so one global
hash cannot identify every supported tool set. The runtime fixture records separate
builtin and synthetic dynamic-tool configurations without copying their prompts.

```toml
[editing_policies.helper-edit]
version=1
client_contract="codex-code-mode/v1"
representation="context-lines/v1"
patch_dialect="codex-patch/1"
normalization="none"
client_descriptor_sha256="<host-verified-64-lowercase-hex-digest>"
```

The request must contain the matching bare custom exec and pinned source grammar.
Changed descriptions, wrong types and mismatched contracts reject before inference.
The shared compiler emits one `tools.apply_patch` helper invocation with JSON data
and one result display statement. Only that exact wrapper inverts to a patch.
Other programs retain their original source. The source grammar accepts nonempty
Unicode; JavaScript parsing and execution remain the host's responsibility.

Observed exec results are ordered arrays of input-text parts. The selected policy
adds the effective `CodeModeTextParts` bridge for `StructuredToolOutput` without
claiming native provider support. Parts become a canonical JSON text record with
`schema=codex-exec-text-parts/v1` and `parts`; text, order and boundaries are kept.
No status inside text is interpreted as helper success. Images, unknown fields,
nontext parts and unrelated structured function results reject. Direct patch
contracts retain their existing string result rules.

Descriptor and policy changes alter route origin. Existing authenticated replay v2
stores provider-original calls and public wrapper output without new fields.
Synthetic tests cover execution, denial, invalid edits, wrong descriptors, whole
program results, same-session restart and recorder failure barriers. This is not
arbitrary Code Mode recovery or real-provider qualification.
