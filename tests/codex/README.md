# Real Codex, synthetic upstream

This explicit conformance command starts the pinned Codex executable, the actual
gateway binary and a synthetic loopback HTTP upstream. The upstream supplies
newly authored fixtures; no reference implementation's tests or consumer content
are copied. All generated homes, workspaces and files remain under `.local/`.

```sh
python3.14 -B scripts/codex_runtime.py prepare
cargo build --locked
python3.14 -B tests/codex/conformance.py
```

Only runtime preparation downloads the pinned official artifact. Conformance
uses no provider account or personal authentication. Each scenario creates a
separate local token and Codex home. Upstream keys are not in the Codex child
environment. The gateway continues to own transport only; Codex executes the
synthetic patch and the host replies to dynamic tools and approval requests.

The default command runs both native Responses and Messages. `--api responses` or
`--api messages` selects one route. The program emits one payload-free JSON result per scenario and exits nonzero
if any scenario fails. It still collects later results after an earlier failure.
Its `gpt-5.4` model identifier selects Codex's tool profile; all model traffic goes
to the synthetic provider and is routed to `synthetic-model`. It is not a live
model test. The test's 32,768-token context and 24,576-token compaction threshold
are synthetic settings, not assertions about any provider model.

| Scenario | Contract |
|---|---|
| text | One request and explicit completed turn |
| function_tool | Dynamic function call and result/follow-up round trip |
| namespace_tool | Namespace identity and arguments survive the round trip |
| custom_patch | Codex applies a synthetic patch; its result returns to the model |
| approval_denial | An actual file-change approval is declined and no file is written |
| cancellation | Client-observed output precedes interrupt; an eventful stream closes |
| cancellation_heartbeat | The same interrupt must close a stream sending only SSE comments |
| transport_failure | EOF without completion fails the turn without retry |

Runtime processes, upstream threads and temporary workspaces are cleaned up after
each scenario. Standard Rust tests still exercise the gateway without requiring
Codex. See the [pinned contract](../../docs/codex-contract.md).

## Previous failure and temporary baseline

On stable 0.153.4 the heartbeat cancellation scenario failed: the control turn became
`interrupted`, but the upstream socket remains open beyond the test's five-second
closure bound. Sending another model event instead of an SSE comment makes the
connection close. The fixture waits for client-observed partial output before
interrupting; this is not a race with request startup.

The pinned implementation spawns an SSE reader task and waits for the next parsed
event or an idle timeout. It observes a dropped receiver when sending a parsed
event, without selecting receiver closure while waiting. SSE comments do not
produce such an event. This source path is consistent with the paired synthetic
reproduction. See [pinned SSE implementation](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/codex-api/src/sse/responses.rs).

The gateway cannot infer a control-plane interrupt while its HTTP client still
holds the connection open. Do not inject artificial Responses events, shorten
timeouts to conceal this gap or remove the regression case. The suspected stable-
release stream-lifetime defect is addressed by the explicitly approved temporary
**0.154.0-alpha.6** test baseline, whose receiver-closure fix passed all eight local
scenarios. Replace it with **0.154.0 or a later stable version** after official
artifact/schema verification and all checks pass. The actual alpha version and
digest stay visible; this is not a consumer-runtime or production upgrade.

The same heartbeat case remains mandatory in CI. A single passing run on the old
version does not erase the repeated local reproduction or the source-level gap.
This fixture does not guarantee a provider will stop already-processed work or
reverse charges.

## Messages profile and extended checks

Messages runs the eight common scenarios plus parallel_tools, grammar_failure
and text_followup. The host derives a minimal compatible catalog from the pinned
binary's `debug models --bundled` output, retaining all original prompts and
changing only the declared optional reasoning/verbosity/search capability fields.
It disables host search and multi-agent tools in this test profile. The gateway
never strips required semantic fields to pass the fixture. Catalog digest and
timing fields appear in the result; no catalog, prompt or body is uploaded.

The custom test validates the forwarded grammar fingerprint, applies a real
synthetic patch and returns its result. Grammar failure must produce a failed
turn with zero execution and no retry. Both cancellation variants require socket
closure within 5000 ms of the interrupt, measured from the interrupt itself.
The text-followup test starts a second actual turn and checks retained assistant
text. Parallel tools preserve both call IDs and results.

Timing fields describe this synthetic end-to-end path. first_client_text_ms may
follow a tool round trip; turn_elapsed_ms excludes setup; cancellation reports
interrupt_to_upstream_close_ms. These are not provider latency or isolated gateway
overhead. See [Messages support](../../docs/messages.md) for the exact profile,
limitations and one local measurement. Consumer activation and live-model tests
remain separate from this suite.

## Embedded child contract

Before each scenario the harness inspects the offline manifest without credentials,
recomputes its canonical configuration SHA-256 in Python, and validates the child's
bounded readiness line against that schema/version/digest and configured numeric
loopback address. The Codex child receives a dedicated HOME/CODEX_HOME and local
token only; upstream values remain confined to the gateway child environment.
The host fixture raises on mismatched readiness or startup timeout and its existing
cleanup stack reaps started children. Separate script/Rust tests cover malformed
frames, deadline cleanup, bind failure and graceful shutdown with an active body.
Executable/release provenance and consumer operational acceptance remain separate.

## Continuity

Run `python3 -B tests/codex/continuity.py` for the actual pinned Codex tool, local
compaction, process restart and explicit model-switch probe. It uses synthetic
loopback traffic and the [host-owned continuity contract](../../docs/continuity.md).
The result does not qualify a provider or consumer workflow.
