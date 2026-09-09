<a id="고정-codex-실행-계약"></a>

# Pinned Codex contract

[English](codex-contract.md) | [한국어](ko/codex-contract.md)

The temporary test baseline is the official Codex **0.154.0-alpha.6** package for macOS ARM64.
`tests/codex/runtime-lock.json` records the official archive URL, byte size and
SHA-256, each executable/package member, and stable/experimental generated schema
bundle digests. These values were obtained from the official release and the
verified executable, not from a consumer's private configuration or generated logs.

<a id="준비와-검증"></a>

## Preparation and verification

```sh
python3.14 -B scripts/codex_runtime.py prepare
python3.14 -B scripts/codex_runtime.py verify
python3.14 -B scripts/codex_runtime.py schema --profile stable
python3.14 -B scripts/codex_runtime.py schema --profile experimental
```

Only `prepare` downloads anything. `prepare --archive <local-archive>` uses an
already-downloaded file and still verifies its size, digest and members. Existing
bundles are checked, not overwritten. Executables and schemas remain under ignored
`.local/` state and are not shipped with the gateway. Schema output must be a new
directory. Preparation does not log in, call a model or modify a personal Codex home.

The official archive digest is
`ae37c70e6c86f1f4248303e084cd03480d8628b74df57649c128730ce4159d50`.
The generated stable and experimental bundles have different digests; choose the
matching profile instead of treating experimental fields as stable protocol.

<a id="제어-인터페이스와-모델-인터페이스"></a>

## Control and model interfaces

Use stdio JSONL control: initialize once, send initialized, start a thread and
turn, handle notifications/server requests, and require turn/completed with an
explicit final status. Dynamic tools require experimental API opt-in. The exact
schema generated from the pinned executable is the version-specific reference.

Model traffic is a separate Responses HTTP/SSE connection to the gateway.
The conformance profile uses a dedicated CODEX_HOME, synthetic credentials and a
loopback custom provider with HTTP and stream retries disabled. It does not use
personal authentication, WebSockets or provider fallback. Tools and approvals
remain in Codex and the host, not the gateway.

<a id="필수-수락-기준"></a>

## Required acceptance

| Area | Required result |
|---|---|
| Text and streaming | Ordered output items and explicit successful completion |
| Function tools | Call identity, arguments, result and follow-up turn preserved |
| Custom tools | Original freeform input and required grammar preserved; no claim from JSON wrapping alone |
| Tool namespaces | Preserve actual declared grouping and identity; unsupported forms fail explicitly |
| Approval denial | Host declines an actual Codex approval request; proposed action does not execute |
| Cancellation | Interrupt reaches an explicit cancelled turn and closes upstream work |
| Transport failure | Missing completion is not success, and no stream is spliced or implicitly retried |
| Context | Model-specific window, output reservation and compaction threshold must agree |
| Continuity | Tool resume, compaction and restart require the separately approved state contract |

Synthetic model limits are test fixtures, not claims about a real model. Runtime
hosts own context selection and run/retry budgets; the gateway owns declared
transport routes and compatibility checks. The current gateway remains stateless.
The capability/route and persistence designs are separate approval work items.

The harness records which tests actually ran; the [current three-route suite](conformance.md)
extends the original G04 baseline. Binary integrity,
schema generation and synthetic archive tests alone are not Codex conformance,
provider qualification, consumer acceptance or a production release.

<a id="임시-기준과-안정판-교체"></a>

## Temporary baseline and stable replacement

The former stable baseline 0.153.4 failed the heartbeat cancellation scenario.
Paired synthetic tests and the pinned source indicate that a stream-lifetime
management defect in that stable release is the likely cause: its reader waits
for a parsed SSE event without also waiting for receiver closure. The official
0.154.0-alpha.6 contains that closure check and passed all eight identical local
conformance scenarios. The user approved this temporary test-baseline change.

Replace this prerelease with **0.154.0 or a later stable release** when available,
after verifying the official artifact, regenerating both schema profiles and
passing the complete conformance and repository gates. Keep the replacement as
a reviewed PR with exact version/digest evidence. Do not relabel alpha bytes as
0.154.0, fetch an unverified latest build, or treat a prerelease as stable.

This is a test-only baseline. It neither changes a consumer's installed runtime
nor authorizes a production rollout. The previous release and comparison evidence
remain documented for reproduction; no consumer history or state is migrated.

References: [official App Server documentation](https://learn.chatgpt.com/docs/app-server),
[official temporary release](https://github.com/openai/codex/releases/tag/rust-v0.154.0-alpha.6),
[previous stable SSE reader](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/codex-api/src/sse/responses.rs),
[candidate receiver-closure fix](https://github.com/openai/codex/blob/rust-v0.154.0-alpha.6/codex-rs/codex-api/src/sse/responses.rs).
