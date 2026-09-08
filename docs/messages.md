# Messages adapter support

G07 provides a pure Messages request/non-streaming response codec. It is not an
HTTP-enabled route yet: G08 stream/custom-tool work and G09 actual Codex
qualification are required before activation. Tests use synthetic data only.

| Contract | G07 behavior |
|---|---|
| Top-level instructions | Messages system text |
| Leading system/developer messages | Explicit approved `bridged_instruction_envelope` only |
| Late system/developer messages | Reject; never move them before conversation content |
| Ordered user/assistant text | Preserve ordered content blocks; adjacent equal roles combine as Messages specifies |
| HTTPS image references without detail | Map to URL source; no downloads; other image inputs reject |
| Flat function definitions/calls/results | Schema, parsed JSON values, call IDs and result association retained |
| Function argument schema absent | Empty object schema for a parameterless function |
| Tool choices and parallel restrictions | Map to Messages tool_choice/disable_parallel_tool_use and validate returned calls |
| Custom tools, namespace and grammar | G08; currently reject |
| Strict tools, structured output, reasoning controls/state | Explicitly unsupported by this codec |
| Non-streaming text/tool output | Validate through the shared EventIR state machine and produce Responses JSON |
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
