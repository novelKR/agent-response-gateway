<a id="codex와-모의-공급자로-시험하기"></a>
<a id="real-codex-synthetic-upstream"></a>
<a id="실제-codex와-합성-upstream"></a>

# Testing with Codex and mock providers

[English](README.md) | [한국어](README.ko.md)

The conformance suite starts the pinned Codex executable, gateway binary and mock
loopback HTTP providers. It tests tool execution and protocol behavior without
provider accounts or personal authentication. Generated homes, workspaces and
files stay under `.local/` and are cleaned up after each scenario.

```sh
python3.14 -B scripts/codex_runtime.py prepare
cargo build --locked
python3.14 -B tests/codex/conformance.py
```

Runtime preparation downloads the pinned official package. Each scenario gets a
separate local token and Codex home. Provider keys are passed only to the gateway.
Codex applies the test patch, while the host handles dynamic tools and approval
requests. See the [runtime guide](../../docs/codex-contract.md).

The default suite runs 35 scenarios: nine Responses and thirteen each for Messages
and Chat Completions. `--api responses`, `--api messages` or `--api chat_completions`
selects one route. One JSON result is emitted per scenario; any failure produces a
nonzero exit status, while later scenarios still run.

The `gpt-5.4` identifier selects Codex tool settings; every model request goes to
`synthetic-model` at the mock provider. The 32,768-token context and 24,576-token
compaction threshold are test settings, not specifications for a real model.

| Scenario | Check | Routes |
|---|---|---|
| text | One request and explicit completion | All |
| function_tool | Dynamic function call, result and follow-up | All |
| namespace_tool | Namespace identity and arguments | All |
| custom_patch | Apply a test patch and return its result | All |
| approval_denial | Reject file-change approval without writing the file | All |
| cancellation | Interrupt after observed text and close the active stream | All |
| cancellation_heartbeat | Close a stream sending only SSE comments | All |
| transport_failure | Fail EOF without completion or retry | All |
| output_controls | Explicit effort and strict output schema | All |
| parallel_tools | Preserve two tool calls and results | Converted |
| grammar_failure | Reject invalid grammar before tool execution | Converted |
| text_followup | Preserve prior assistant text in a following turn | Converted |
| mixed_tool_text | Preserve mixed output and tool-result replay | Converted |

<a id="previous-failure-and-temporary-baseline"></a>
<a id="이전-실패와-임시-기준"></a>
<a id="취소-조건"></a>

## Cancellation requirements

Both cancellation scenarios wait for client-observed output before interrupting.
They require the control turn to stop and the upstream socket to close within
5000 ms of the interrupt, including when the provider sends only heartbeat
comments. A control status alone is insufficient. Keep both cases in the suite;
changing event content or shortening gateway timeouts must not conceal a failed
cancellation contract. Socket closure cannot guarantee reversal of provider work
already processed.

<a id="messages-profile-and-extended-checks"></a>
<a id="messages-프로필과-확장-검사"></a>
<a id="변환-경로-프로필"></a>

## Converted-route profile

The host derives a compatible catalog from `debug models --bundled`, retains its
prompts and changes the declared optional reasoning/verbosity/search fields.
Host search and multi-agent tools are disabled in this profile. The gateway does
not drop required semantic fields to make a request succeed.

The patch test checks the grammar fingerprint, applies a real test patch and
returns its result. Grammar failure requires zero tool execution and no retry.
The exact catalog settings are listed in [Messages support](../../docs/messages.md).

Results record profile digest and timings without catalogs, prompts or bodies.
first_client_text_ms may follow a tool round trip; turn_elapsed_ms excludes setup;
interrupt_to_upstream_close_ms measures cancellation. These are mock-provider
end-to-end timings, not real-model latency or isolated gateway overhead.

<a id="embedded-child-contract"></a>
<a id="내장-child-계약"></a>
<a id="내장-프로세스-검사"></a>

## Embedded process checks

Before each scenario, inspect the offline manifest without credentials,
recompute its configuration SHA-256 in Python and compare the child's bounded
readiness line with the schema, version, digest and numeric loopback address.
Codex receives a dedicated HOME/CODEX_HOME and local token; provider keys stay in
the gateway child environment. Readiness mismatch or startup timeout fails the
scenario and triggers child cleanup.

Separate Rust/script tests cover malformed readiness, timeout cleanup, bind
failure and graceful shutdown with an active response. Standard Rust tests do
not require Codex.

<a id="연속성"></a>

## Continuity

Run `python3 -B tests/codex/continuity.py` to test tool history, local compaction,
process restart and explicit model changes using the
[host continuity contract](../../docs/continuity.md). Test real-model summary quality
and the application's persistence/recovery separately before production use.
