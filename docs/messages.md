# Messages adapter support

G07/G08 provide pure Messages request, JSON response and incremental stream codecs.
G09 actual pinned-Codex qualification and HTTP cancellation tests remain required
before route activation. Tests use synthetic data only.

| Contract | Implemented codec behavior |
|---|---|
| Top-level instructions | Messages system text |
| Leading system/developer messages | Explicit approved `bridged_instruction_envelope` only |
| Late system/developer messages | Reject; never move them before conversation content |
| Ordered user/assistant text | Preserve ordered content blocks; adjacent equal roles combine as Messages specifies |
| HTTPS image references without detail | Map to URL source; no downloads; other image inputs reject |
| Flat function definitions/calls/results | Schema, parsed JSON values, call IDs and result association retained |
| Function argument schema absent | Empty object schema for a parameterless function |
| Tool choices and parallel restrictions | Map to Messages tool_choice/disable_parallel_tool_use and validate returned calls |
| Custom text tools | Explicit `bridged_custom_tool_json`, one-string JSON wrapper and exact restoration |
| Ordered namespace groups | Explicit `bridged_tool_namespace`, flat aliases shared by definitions, choices, history, results and output; group/member descriptions retained |
| Registered patch grammar | Explicit `bridged_codex_patch_grammar`, exact SHA-256/version selection and post-generation syntax validation |
| Strict tools, structured output, reasoning controls/state | Explicitly unsupported by this codec |
| Non-streaming text/tool output | Validate through EventIR and produce Responses JSON; untrusted bytes reject duplicate JSON keys |
| Streaming text and function arguments | Incremental Responses events, stable IDs/indices and ordered sequence numbers |
| Streaming custom input | Buffer the envelope per tool; emit restored input only after wrapper and grammar validation |
| SSE boundaries | Arbitrary UTF-8/byte splits, LF/CRLF/CR, BOM, comments and multiline data |
| Stream error, truncation or unknown semantic event | Fail; never synthesize response completion or retry |
| max_tokens finish | Incomplete response; never report token truncation as completion |
| Unknown blocks, citations, state or finish behavior | Explicit error; never drop semantic output to claim success |
| Usage | Input plus cache-read/cache-created input tokens, output, and checked total |
| Provider errors | Existing HTTP status sanitization; codec errors never contain provider bodies |

The instruction bridge preserves text, role provenance and prefix order, but
cannot guarantee the native role priority behavior of Responses. It is opt-in
only on Messages. User/tool content stays outside the envelope. Host tool
permissions and approvals keep their existing owner. See the approved
[instruction bridge design](messages-instruction-design.md).

A profile declaration cannot activate unsupported encoder features. The codec
rechecks its implemented subset after pure capability admission. Configuration
and profile validation do not certify a provider or a real model's output quality.

A returned model must match the frozen upstream model; configure a concrete
qualified model identifier. Response/item IDs identify the translated response and retain original call IDs
for tool results. They provide no lookup or persisted response semantics. The
created_at field is the gateway's conversion time; the Messages response does not
provide a corresponding creation timestamp. Cache-token accounting is retained
without claiming that provider token counts are comparable across models.

Primary wire contracts: [Messages](https://platform.claude.com/docs/en/api/messages/create),
[stop reasons](https://platform.claude.com/docs/en/build-with-claude/handling-stop-reasons),
and [Responses](https://developers.openai.com/api/reference/typescript/resources/responses/methods/create).
No upstream implementation code or consumer data was copied into the codec/tests.

## Tool and stream limits

The custom/namespace bridges require native function-tool support in the declared
profile. These bridges do not claim native namespace semantics or constrained
sampling. A member's namespace and original name remain its canonical identity;
plain and grouped tools may share a leaf name, but duplicate effective identities
and nested groups reject. Unknown aliases, mismatched choices/results, duplicate
wrapper fields and non-string/extra wrapper fields reject.

The initial grammar registry is `codex-patch/1`, selected only when the declared
Lark grammar hashes to
`d6367f4826ed608c424b0a308f3d6163527df63c22513d089b91863552f8bfeb`.
This declaration was observed from the pinned synthetic Codex runtime. The public
registry contains its fingerprint and an independently written syntax checker;
no upstream grammar source is vendored. SHA-256 uses the explicitly approved
existing locked `ring` 0.17.14 package without new packages or feature activation.
Only patch syntax is checked: file permissions, existence, content applicability
and execution stay with the host. Unknown grammar definitions reject.

The SSE framer returns one event at a time so its caller can yield downstream
before consuming another event from the same network chunk. The caller supplies
positive framing and aggregate output byte limits. Event validation additionally
bounds deltas to 1 MiB, accumulated arguments to 8 MiB and output items to 4096.
Text and raw tool fragments share the aggregate output budget. No response is
completed without message_delta followed by message_stop; EOF or dropping the
state is not a successful terminal event. Actual HTTP disconnect/cancellation is
owned by the transport and is an additional G09 acceptance gate.

The regression suite checks every byte split of a text/parallel-function/custom
stream, exact custom text restoration, namespace collisions and choice/history
mapping, duplicate JSON, malformed wrappers, early EOF, error/unknown/order
failures and aggregate limits. Pure codec success does not establish actual Codex
or provider qualification. The streaming contract follows the primary
[Messages streaming documentation](https://platform.claude.com/docs/en/build-with-claude/streaming).
