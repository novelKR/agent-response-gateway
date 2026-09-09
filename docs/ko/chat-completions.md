<a id="chat-completions-adapter-support"></a>

# Chat Completions 어댑터 지원

[English](../chat-completions.md) | [한국어](chat-completions.md)

Chat Completions 어댑터는 선언된 지원 기능 프로필에 따라 Responses 요청,
JSON 응답과 SSE 스트림을 변환한다. [설정 예제](../../config.chat.example.toml)로
경로와 사용할 기능을 선택한다.

도구는 표준 함수 도구 형식을 사용한다. 사용자 정의 자유 형식 입력은 Messages와
같은 명시적 JSON 변환 규칙이 필요하며, 요청 단위로 이름·선택·결과를 검사한다.
Chat Completions의 별도 고유 사용자 정의 도구 형식은 지원하지 않는다.
해당 기능을 Native로 선언하면 임의로 변환하지 않고 거부한다.

| 기능 | 동작 |
|---|---|
| 최상위 지시 | 대화 앞의 system 메시지 |
| system/developer/user/assistant 메시지 | 뒤늦은 지시를 포함해 원래 역할 필드와 순서 유지 |
| 순서 있는 텍스트 | 내용 순서 보존; assistant 텍스트와 뒤따르는 병렬 호출을 같은 메시지에 배치 가능 |
| 사용자 HTTPS 이미지 | 선택적 auto/low/high detail을 가진 image_url; 다운로드 없음; 다른 역할의 이미지는 거부 |
| 함수 스키마·호출·결과 | JSON 스키마, 원래 인자 문자열, 호출 ID와 결과 연결 보존 |
| 인자 없는 함수 | additionalProperties false인 빈 객체 스키마 |
| 사용자 정의 텍스트·네임스페이스·등록 문법 | 변환 규칙 선언 필요; 정의·선택·이력·출력에서 하나의 매핑 공유 |
| 도구 선택·병렬 제어 | 필드 매핑과 반환 식별자·횟수 검사 |
| max_output_tokens | 한도를 적용한 max_completion_tokens; max_tokens로 대체하지 않음 |
| Temperature/top_p | 0–2 / 0–1 범위에서 값 보존 |
| 스트리밍 | 증분 텍스트, 크기가 제한된 도구 조각, 텍스트 다음 도구 출력, 명시적 finish + [DONE] |
| 엄격한 함수 스키마 | 지원을 선언한 경우 strict 필드 보존 |
| JSON/text 출력 형식 | 원래 스키마 이름·규칙·선택적 strict 필드를 담은 response_format |
| 추론 강도 | reasoning_effort의 none/minimal/low/medium/high/xhigh/max를 이름 변경·대체 없이 사용 |
| 추론 요약·상태와 verbosity | 미지원 |
| 비스트리밍 출력 | 정확히 하나의 choice; 텍스트와 도구 출력 검사 |
| stop/tool_calls 종료 | 도구 횟수와 선택이 일치할 때만 완료 |
| length 종료 | Incomplete/max_output_tokens |
| 거절·필터·function_call·알 수 없는 의미 출력 | 명시적 변환 오류 |
| 토큰 사용량 | 선택적 prompt/completion/total 검사; 캐시·추론 세부값 보존; 누락은 null |

공급자의 created 시각은 created_at으로 보존한다. 응답·항목 ID는 공급자 응답
ID에서 만들고, 원래 도구 호출 ID와 네임스페이스·이름 쌍은 유지한다.
게이트웨이는 응답을 저장하거나 ID 조회를 제공하지 않는다. 알 수 없는 확장
필드와 다른 프로토콜의 불투명 입력은 거부한다.

반환된 도구 이름, 선택과 횟수는 요청과 일치해야 한다. 공급자 JSON의 중복
키를 검사하며, 사용자 정의 입력의 중복·추가 포장 필드와 잘못된 등록 문법도 거부한다.

엄격한 출력과 추론 강도는 프로필에 지원을 명시해야 한다. 어댑터는 스키마
규칙과 추론 강도를 약화하거나 프롬프트로 대체하지 않고 보존한다.
호스트가 반환 데이터를 검사하며, 실제 공급자·모델의 스키마 지원, 지시 처리와
출력 품질을 시험해야 한다.

주요 계약: [Chat Completions](https://developers.openai.com/api/reference/typescript/resources/chat/subresources/completions/methods/create),
[사용자 정의 도구](https://developers.openai.com/api/docs/guides/function-calling#custom-tools).

<a id="streaming-contract"></a>

## 스트리밍 계약

디코더는 임의 바이트·UTF-8 경계를 처리한다. 스트림은 응답 ID, 모델,
created 값과 choice 인덱스 하나를 고정한다. 도구 조각은 공급자의 인덱스별로
모으며 분할된 ID·이름도 처리한다. 최종 도구 인덱스는 연속이어야 하고,
이 식별 정보는 응답 안에서 바뀔 수 없다.

텍스트는 즉시 전송한다. 도구는 finish_reason까지 모아 뒤늦은 텍스트가 이미
내보낸 출력 인덱스를 바꾸지 못하게 한다. 도구 완료 전에 전체 인자, 선택과
필수 문법을 검사한다. 텍스트와 도구 바이트는 누적 출력 한도에 포함한다.
누적 인자는 8 MiB, 개별 텍스트 조각은 1 MiB로 제한하며 큰 최종 텍스트는
UTF-8 경계에서 나눈다.

유효한 finish_reason과 마지막 data: [DONE] 후에만 response.completed/incomplete를
내보낸다. 마지막 사용량 전용 조각을 지원한다. 사용량 누락은 null로 남기고,
불일치하거나 감소하는 수치는 거부한다.

종료 표식 누락, 공급자 오류, 알 수 없는 의미 조각, 응답 식별자 변경과 종료
이후 데이터는 스트림을 실패시킨다. 잘린 도구 JSON이 잘못되었다면 finish_reason이
length여도 실패하며, 실행 가능한 인자 객체로 임의 복구하지 않는다.

테스트는 바이트 분할, 병렬 호출, 스트리밍·비스트리밍 값의 일치, 잘린 응답,
잘못된 사용자 정의 입력, 식별자·순서 오류, 사용량과 출력 한도를 검사한다.
HTTP 시험은 자격 증명, 전송 전 거부, 취소와 동시 요청 슬롯 반환을 확인한다.

[공통 시험](conformance.md)은 실제 Codex와 모의 공급자로 Chat Completions
시나리오 13개를 실행한다. 제한된 [Messages 시험 프로필](messages.md)을 사용해
명시적 추론 강도·스키마 제어, 문법 거부, 승인 거절과 두 취소 방식을 검사한다.
이 결과는 프로토콜 경로를 검증하며, 운영에는 선택한 실제 모델과 애플리케이션의
시험이 필요하다.
