<a id="codex-시험-런타임"></a>
<a id="pinned-codex-contract"></a>
<a id="고정-codex-실행-계약"></a>

# Codex test runtime

[English](codex-contract.md) | [한국어](ko/codex-contract.md)

Tests use the official Codex **0.154.0** package for macOS ARM64.
`tests/codex/runtime-lock.json` pins its archive URL, size, SHA-256, package members
and generated stable/experimental schema digests. This stable release is used as the test runtime.

<a id="준비와-검증"></a>

## Preparation and verification

```sh
python3.14 -B scripts/codex_runtime.py prepare
python3.14 -B scripts/codex_runtime.py verify
python3.14 -B scripts/codex_runtime.py schema --profile stable
python3.14 -B scripts/codex_runtime.py schema --profile experimental
```

Only `prepare` downloads files. `prepare --archive <local-archive>` verifies an
existing download by size, digest and members. Existing bundles are checked rather
than overwritten. Executables and schemas stay under ignored `.local/` state and
are not bundled with the gateway. Schema output requires a new directory.
Preparation does not log in, call a model or change personal Codex settings.

The archive SHA-256 is
`427ca74c027049e0cd1a330d611e7f8d1fe0f1eb6a6d85ac16f61bcf2cb4a485`.
Select the required schema profile; stable and experimental schemas have different
digests and supported fields.

<a id="제어-인터페이스와-모델-인터페이스"></a>

## Control and model interfaces

Control uses stdio JSONL. Initialize once, send initialized, start a thread/turn
and handle notifications and server requests. Require turn/completed with an
explicit final status. Dynamic tools require experimental API opt-in. Use the
schema generated from the pinned executable as the version-specific reference.

Model requests use a separate Responses HTTP/SSE connection to the gateway.
Tests use a dedicated CODEX_HOME, synthetic credentials and a loopback provider
with HTTP/stream retries disabled. Codex and the host own tools and approval;
the gateway owns model transport.

<a id="required-acceptance"></a>
<a id="필수-검사"></a>
<a id="필수-수락-기준"></a>

## Required checks

| Area | Required result |
|---|---|
| Text and streaming | Ordered output and explicit successful completion |
| Function tools | Preserve call identity, arguments, results and follow-up |
| Custom tools | Preserve freeform input and validate required grammar |
| Tool namespaces | Preserve groups and identities; reject unsupported forms |
| Approval denial | Decline an actual approval request without executing the action |
| Cancellation | Cancel the control turn and close the upstream connection |
| Transport failure | Fail missing completion without splicing streams or retrying |
| Context | Align model window, output reserve and compaction threshold |
| Continuity | Verify tool history, compaction and restart under the host state contract |

Run the [three-route suite](conformance.md) after runtime verification. Mock-provider
results validate the protocol path. Hosts must test real-model behavior and their
own context limits, permissions and recovery before operational use.

<a id="temporary-baseline-and-stable-replacement"></a>
<a id="시험-런타임-갱신"></a>
<a id="임시-기준과-안정판-교체"></a>

## Updating the test runtime

A replacement stable runtime must be **0.154.0 or later**. Verify its official
artifact, regenerate both schema profiles and pass the full conformance and
repository checks before changing the lock. Keep the actual version and digests
in the reviewed change; do not select an unverified latest build automatically.

Heartbeat-only cancellation remains a required regression test. Runtime changes
must preserve the complete tool, approval and cancellation contracts. Updating
this test lock does not select or install a runtime for another application.

References: [App Server documentation](https://learn.chatgpt.com/docs/app-server),
[pinned release](https://github.com/openai/codex/releases/tag/rust-v0.154.0).
