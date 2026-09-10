<a id="messages-instruction-role-lowering--approval-addendum"></a>
<a id="messages-지시-메시지-변환"></a>
<a id="messages-지시-역할-변환--승인-부록"></a>

# Messages instruction mapping

[English](messages-instruction-design.md) | [한국어](ko/messages-instruction-design.md)

The Messages adapter can convert a leading sequence of Responses instructions
into system text when its profile explicitly enables `bridged_instruction_envelope`
for `instruction_hierarchy`. This preserves instruction text and ordering but
cannot reproduce separate system/developer priority in the target model.

<a id="observed-boundary"></a>
<a id="관측된-경계"></a>
<a id="역할-차이"></a>

## Role differences

Responses represents system and developer messages as distinct input roles.
Messages accepts user/assistant conversation roles and a separate system prompt,
without a developer role. The adapter therefore reports this feature as Bridged,
not Native. See the [Messages reference](https://platform.claude.com/docs/en/api/messages/create)
and [Responses reference](https://developers.openai.com/api/reference/typescript/resources/responses/methods/create).

<a id="recommendation-and-exact-proposed-behavior"></a>
<a id="권고와-정확한-동작"></a>
<a id="변환-규칙"></a>

## Conversion rules

1. Collect only instructions before the conversation content.
2. Encode each instruction's original role, source position and exact text as
   JSON data, preserving order and distinguishing protocol-default, system and
   developer sources.
3. Prepend a fixed adapter instruction explaining that this data contains
   application instructions with priority above user content.
4. Keep user and tool content outside that instruction envelope.
5. Reject system/developer messages after conversation content instead of moving
   them to the beginning. Reject a missing bridge declaration before dispatch.

The original Responses path and intermediate representation retain their roles
and ordering. The conversion applies only to an explicitly configured Messages
route. Use a Codex profile that disables unsupported hosted search and reasoning
options; the gateway does not silently remove required features.

<a id="alternatives-and-affected-boundaries"></a>
<a id="대안과-영향-경계"></a>
<a id="한계와-검증"></a>

## Limits and validation

The bridge preserves the source roles as data; it cannot enforce the target
model's interpretation of their relative priority. Tool permissions and user
approval remain host responsibilities.

Tests check exact text, role and order preservation, exclusion of user/tool
content, late-instruction rejection and zero upstream requests when the bridge
is missing. The pinned Codex suite exercises the path with a mock provider.
Validate instruction adherence with the actual model before operational use.

If native system/developer separation is required, select a route that supports
it. To disable this conversion, remove the bridge declaration; requests requiring
it will then fail explicitly. See [Messages support](messages.md) for the complete
profile and conversion limits.
