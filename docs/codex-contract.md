# Pinned Codex contract

The test baseline is the official Codex **0.153.4** package for macOS ARM64.
`tests/codex/runtime-lock.json` records the official archive URL, byte size and
SHA-256, each executable/package member, and stable/experimental generated schema
bundle digests. These values were obtained from the official release and the
verified executable, not from a consumer's private configuration or generated logs.

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
`35438da1fbf7a6db7ddb3bcec84448fa6015ba188461472a97d9d1da7d9c4353`.
The generated stable and experimental bundles have different digests; choose the
matching profile instead of treating experimental fields as stable protocol.

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

The G04 harness will record which of these tests actually ran. Binary integrity,
schema generation and synthetic archive tests alone are not Codex conformance,
provider qualification, consumer acceptance or a production release.

References: [official App Server documentation](https://learn.chatgpt.com/docs/app-server),
[official pinned release](https://github.com/openai/codex/releases/tag/rust-v0.153.4).
