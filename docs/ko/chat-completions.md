<a id="chat-completions-adapter-support"></a>

# Chat Completions 어댑터 지원

[English](../chat-completions.md) | [한국어](chat-completions.md)

G10–G12는 명시적 프로필 아래 요청·JSON 응답·스트림·HTTP 변환을 제공한다.
Function-wire 프로필은 실제 고정 Codex·모의 upstream 및 HTTP 시험을 통과했다.
이 fixture는 실제 공급자 모델을 qualification하지 않는다.

초기 프로필은 표준 function-tool wire 형식을 사용한다. Custom 자유 형식
도구는 명시적 CustomToolJson bridge를 사용하고 Messages와 동일한 요청 단위
정체성·선택·결과 검증을 따른다. 현재 Chat Completions API에 별도 native custom
도구 형식이 있어도 이 어댑터의 구현·검증 부분집합에는 포함되지 않는다.
Custom을 Native로 선언하면 JSON bridge로 몰래 바꾸지 않고 명시적으로 실패한다.

| 계약 | 구현 동작 |
|---|---|
| Top-level instructions | 선행 system 메시지 |
| 명시적 system/developer/user/assistant | 뒤늦은 지시를 포함해 native 역할과 원래 순서 유지 |
| 순서 있는 텍스트 | Text/part 순서 보존; assistant 서문과 뒤따르는 병렬 도구를 같은 메시지에 둘 수 있음 |
| User의 HTTPS 이미지 참조 | 선택적 auto/low/high detail을 가진 image_url, 다운로드 없음; 다른 역할의 이미지는 거부 |
| 함수 schema·호출·결과 | Schema JSON 값, 원래 인자 문자열, call ID와 결과 연결 보존 |
| 인자 없는 함수 | additionalProperties false인 명시적 빈 객체 schema |
| Custom/namespace/등록 문법 | 대응 승인 bridge 필요; 정의·선택·이력·출력에 하나의 매핑 사용 |
| 도구 선택·병렬 제어 | Native 필드 매핑과 반환 정체성·횟수 검증 |
| max_output_tokens | 선언 한도로 제한한 max_completion_tokens; 과거 max_tokens로 fallback하지 않음 |
| Temperature/top_p | Chat의 0–2 / 0–1 범위 안에서 값 보존 |
| 스트리밍 | 증분 텍스트, 제한된 도구 ID·이름·인자 조각, 텍스트 후 도구의 정규 출력과 명시적 finish + [DONE] |
| Strict 함수 schema | 지원을 선언했을 때 native strict flag 보존 |
| JSON/text 출력 형식 | response_format으로 매핑; schema 이름·규칙·선택 strict flag 보존 |
| Reasoning effort | none/minimal/low/medium/high/xhigh/max를 reasoning_effort에 이름 변경·fallback 없이 매핑 |
| Reasoning 요약·상태·verbosity | 이 codec 프로필에서 명시적 미지원 |
| 비스트리밍 출력 | 정확히 하나의 choice; 텍스트·function/custom 출력을 EventIR로 검증 |
| stop/tool_calls 종료 | 도구 횟수·선택이 일치할 때만 완료 |
| length 종료 | Incomplete/max_output_tokens; 성공 완료로 처리하지 않음 |
| Refusal/filter/과거 function_call/미지 의미 출력 | 명시적 변환 오류 |
| Usage | 선택적 prompt/completion/total 산술 검사; cache/reasoning 세부값 보존, 누락은 null |

Chat created는 Responses created_at으로 보존한다. 변환 response/item ID는
공급자 응답 ID에서 도출하고, tool call ID와 namespace/name은 유지한다.
Gateway가 응답 조회·저장이나 이력을 소유하지 않는다. 미지 확장과 cross-protocol
opaque 입력은 계속 오류다. 공급자별 모델·역할 동작에는 별도 검증 프로필이 필요하다.

공통 PreparedTools는 Messages와 Chat의 반환 도구 선택·횟수와 정체성 복원을
소유하고, 각 어댑터는 wire/finish 의미를 유지한다. 공급자 바이트는 Value로
변환하기 전에 중복 JSON key를 거부한다. Custom wrapper는 원래 인자 문자열의
중복·추가 필드와 등록 문법도 검증한다.

Codec·스트림 테스트는 역할·순서·옵션, namespace/custom 이름 선택, 병렬 이력,
필수 effort·strict schema, 원래 숫자 인자, cache/reasoning 수치, usage 누락,
토큰 잘림, 미지원 입력과 잘못된 출력을 검사한다. 공통 helper를 바꿔도 Messages
회귀는 필수다. Native Responses의 원형 JSON/SSE 계약은 유지한다. 공통 dispatch는
각 변환 요청의 승인된 계획을 한 번 사용하며 완료·drop까지 stream/permit을 소유한다.

주요 계약: [Chat Completions create](https://developers.openai.com/api/reference/typescript/resources/chat/subresources/completions/methods/create),
[custom tools](https://developers.openai.com/api/docs/guides/function-calling#custom-tools).
Fixture는 새로 작성한 합성 데이터이며 참고 구현의 코드·테스트·prompt를 복사하지 않았다.

매 turn 제어값을 지정하는 호스트에는 명시적 effort와 출력 schema가 필요하다.
Codec은 해당 기능을 프로필이 지원할 때만 native Chat 필드로 전달한다. Effort를
낮추거나 schema 규칙을 바꾸거나 strict 생성을 prompt로 대체하지 않는다.
Schema dialect, 결과 준수와 effort 동작은 공급자의 native 계약이며 gateway에
두 번째 범용 JSON Schema 검증기를 넣지 않는다. 호스트는 결과 데이터를 검증해야
한다. G12는 실제 고정 Codex·합성 upstream에서 high effort와 strict schema를
검증하며 소비자 운영 수락은 별도다.

<a id="streaming-contract"></a>

## 스트리밍 계약

SSE framer는 임의 byte·UTF-8 경계를 처리한다. 한 stream은 response ID,
model, created와 choice index 하나를 고정한다. 도구 조각은 공급자 index별로
모으며 쪼개진 ID·이름도 처리한다. 최종 도구 index는 연속이어야 한다.
고정 메타데이터로 이미 연결한 응답 정체성을 바꿀 수 없다.

Chat 응답은 assistant content와 순서 있는 tool-call 배열을 가진다. 텍스트는
즉시 보내고 도구는 finish_reason까지 모은다. 그래야 초기 도구 조각 뒤에 온
텍스트가 이미 내보낸 출력 index나 정체성을 바꾸지 않는다. 전체 응답 codec과
EventIR이 모든 도구·선택·문법을 검증한 뒤 도구 완료를 내보낸다. 텍스트와 도구
정체성·인자 바이트는 누적 출력 한도에 포함되며 인자는 IR의 공통 8 MiB 제한도
따른다. 변환 텍스트 delta는 1 MiB 이하이고 누적 텍스트를 최종 JSON으로 검증할
때는 UTF-8 경계에서 나눈다.

유효한 finish_reason과 마지막 data: [DONE] 후에만 response.completed/incomplete를
내보낸다. 마지막 usage 전용 chunk를 지원한다. Usage 누락은 null, 불일치·감소는
거부한다. 종료 표식 누락, 공급자 오류, 미지 의미 delta, 정체성 변경과 finish
이후 데이터는 상태를 실패로 고정한다. 잘린 잘못된 도구 JSON은 finish_reason이
length여도 변환에 실패하며, 고친 인자로 완료·실행하지 않는다.

시험은 뒤늦은 텍스트·병렬 도구 stream의 모든 byte 분할, ID·이름·인자의 독립
조각, 비스트리밍/스트리밍 동등성, 중단 stream, 잘못된 wrapper, 정체성·순서,
usage 누락, 큰 누적 텍스트와 인자·출력 한도를 포함한다. HTTP 시험은 두
어댑터의 자격 분리, 요청 전 오류, JSON, 분할 stream, 정제 오류, EOF,
누적 한도, 취소와 동시 요청 슬롯 반환을 검사한다.

[공통 행렬](conformance.md)은 실제 Codex의 Chat 시나리오 13개를 기록한다.
도구·namespace·custom·병렬 왕복, 혼합 출력 재입력, 후속 텍스트, effort/schema,
승인 거절, 문법 실패, 전송 단절과 두 취소 모드를 포함한다. 시험 호스트는
[Messages](messages.md)의 제한된 catalog를 사용하고 선택적 기본값은 끄되
명시적 turn 제어는 유지한다. [Chat Completions 설정 예제](../../config.chat.example.toml)를 참조한다.
