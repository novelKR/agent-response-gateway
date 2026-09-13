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

This design defines opt-in editing compatibility. The runtime does not yet expose
the editing policies described here. Existing custom string bridging remains the
default. The synthetic fixture verifies direct patch execution and Code Mode helper
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

Reserve `gateway-editing-policy/v1` for policy meaning and
`gateway-embedded-manifest/v7`, `gateway-ready/v7`,
`gateway-extended-manifest/v7`, `gateway-extended-ready/v7` for opted-in execution.
Reserve `gateway-api-codec/v2`, `gateway-profile-pack/v2` and
`gateway-continuation/v3` for new codec, pack and authenticated mapping contracts.
These are planned versions, not currently supported schemas. Old configurations
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
