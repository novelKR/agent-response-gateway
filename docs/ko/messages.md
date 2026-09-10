<a id="messages-adapter-support"></a>

# Messages 어댑터 지원

[English](../messages.md) | [한국어](messages.md)

Messages 어댑터는 명시적으로 설정한 Messages 경로에서 Responses 요청,
JSON 응답과 SSE 스트림을 변환한다. [설정 예제](../../config.messages.example.toml)를
기준으로 호스트에 필요한 기능을 선언한다. 프로토콜 시험은 모의 공급자를
사용하므로, 운영 전에는 선택한 실제 모델을 시험해야 한다.

| 기능 | 동작 |
|---|---|
| 최상위 지시 | Messages system 텍스트 |
| 대화 앞의 system/developer 메시지 | `bridged_instruction_envelope` 필요 |
| 대화 뒤의 system/developer 메시지 | 거부; 대화 앞으로 옮기지 않음 |
| 순서 있는 user/assistant 텍스트 | 내용 순서 보존; 인접한 같은 역할은 Messages 규칙대로 결합 |
| 상세 수준 지정이 없는 HTTPS 이미지 참조 | 다운로드 없이 URL source로 전달; 다른 이미지 입력은 거부 |
| 함수 정의·호출·결과 | 스키마, 해석한 JSON 인자, 호출 ID와 결과 연결 보존 |
| 함수 인자 스키마 누락 | additionalProperties false인 빈 객체 스키마 |
| 도구 선택·병렬 제한 | tool_choice/disable_parallel_tool_use 매핑과 반환 호출 검사 |
| 사용자 정의 텍스트 도구 | `bridged_custom_tool_json` 필요; 문자열 하나를 JSON으로 포장하고 정확히 복원 |
| 네임스페이스 그룹 | `bridged_tool_namespace` 필요; 정의·선택·이력·결과·출력에서 별칭 공유 |
| 등록 패치 문법 | `bridged_codex_patch_grammar` 필요; SHA-256·버전으로 선택하고 생성된 구문 검사 |
| 엄격한 함수 인자 | strict_tool_arguments 선언 시 공급자의 strict 필드 설정 |
| 엄격한 JSON 스키마 출력 | output_config.format 규칙 보존; strict_structured_output·structured_output 필수 |
| 추론 강도 | output_config.effort의 low/medium/high/xhigh/max 지원; 다른 값은 거부 |
| 느슨한 JSON 스키마/json_object, 추론 요약·상태, verbosity | 미지원 |
| 비스트리밍 텍스트·도구 출력 | 출력 이벤트 검사 후 Responses JSON 생성; 중복 JSON 키 거부 |
| 텍스트·함수 인자 스트림 | 고정 ID·인덱스·일련번호를 가진 증분 Responses 이벤트 |
| 사용자 정의 입력 스트림 | 도구별로 모은 뒤 JSON 포장과 문법을 검사하고 원래 입력 복원 |
| SSE 구분 처리 | 임의 UTF-8·바이트 분할, LF/CRLF/CR, BOM, 주석과 여러 줄 data |
| 스트림 오류·잘림·알 수 없는 의미 이벤트 | 완료 생성이나 재시도 없이 실패 |
| max_tokens 종료 | 불완전 응답 |
| 알 수 없는 블록·인용·상태·종료 방식 | 명시적 오류 |
| 토큰 사용량 | 입력, 캐시 조회·생성 입력, 출력과 검사된 합계 |
| 공급자 오류 | 정제한 HTTP 상태·오류; 공급자 응답 본문은 포함하지 않음 |

[지시 변환](messages-instruction-design.md)은 원문, 원래 역할과 순서를
보존하지만 Messages에서 system과 developer의 우선순위를 각각 강제하지는 못한다.
사용자·도구 내용은 지시 묶음 밖에 두며 도구 권한과 승인은 호스트가 담당한다.

반환 모델 이름은 설정한 업스트림 모델과 일치해야 한다. 변환된 응답·항목 ID는
변환 결과를 식별하고 원래 도구 호출 ID는 보존한다. 저장된 응답을 조회하는
기능은 제공하지 않는다. Messages에 대응 생성 시각이 없으므로 created_at에는
게이트웨이의 변환 시각을 넣는다.

주요 계약: [Messages](https://platform.claude.com/docs/en/api/messages/create),
[종료 사유](https://platform.claude.com/docs/en/build-with-claude/handling-stop-reasons),
[Responses](https://developers.openai.com/api/reference/typescript/resources/responses/methods/create).

<a id="tool-and-stream-limits"></a>

## 도구·스트림 한도

사용자 정의 도구와 네임스페이스 변환에는 공급자의 함수 도구 지원이 필요하다.
공급자 고유의 네임스페이스 처리나 생성 중 문법 강제를 제공하는 것은 아니다.
도구는 네임스페이스·이름 쌍으로 식별한다. 평면 도구와 그룹 도구가 같은 이름을
쓸 수 있지만 식별자 중복과 중첩 그룹은 거부한다. 알 수 없는 별칭, 결과
불일치, JSON 포장 필드의 중복·추가와 문자열이 아닌 사용자 정의 입력도 거부한다.

지원하는 패치 문법은 `codex-patch/1`이며, 선언한 Lark 문법의 해시가
`d6367f4826ed608c424b0a308f3d6163527df63c22513d089b91863552f8bfeb`일 때 선택한다.
검사기는 구문만 확인한다. 파일 권한, 존재 여부, 적용 가능성과 실행은
호스트가 담당한다. 알 수 없는 문법 정의는 거부한다.

이벤트 검사는 조각 크기 1 MiB, 누적 인자 8 MiB, 출력 항목 4096개로 제한한다.
텍스트와 도구 조각은 누적 출력 예산을 공유한다. max_response_bytes는 각 변환
SSE 이벤트와 보관한 출력에도 적용한다. 디코더는 이벤트를 하나씩 내보내므로
네트워크 조각 안의 모든 이벤트를 처리할 때까지 클라이언트 출력이 기다릴 필요는 없다.

완료에는 message_delta 뒤의 message_stop이 필요하다. EOF나 스트림 폐기는
성공이 아니다. HTTP 취소는 업스트림 연결을 닫고 동시 요청 슬롯을 반환하며,
SSE 주석만 보내는 스트림에도 적용된다. Responses 원형 전달은 전체 스트림을
모으거나 다시 해석하지 않는다.

테스트는 모든 바이트 분할, 병렬·사용자 정의 도구, 원문 복원, 네임스페이스
충돌, 중복 JSON, 잘못된 JSON 포장, 이벤트 순서, 잘린 응답과 한도를 검사한다.
공급자 이벤트 형식은 [Messages 스트리밍](https://platform.claude.com/docs/en/build-with-claude/streaming)을 따른다.

<a id="codex-test-profile"></a>
<a id="qualified-synthetic-codex-profile"></a>
<a id="검증한-합성-codex-프로필"></a>

## Codex 시험 프로필

시험 프로필은 [런타임 잠금 파일](../../tests/codex/runtime-lock.json)의 macOS ARM64
`0.154.0`을 사용한다. Codex에 내장된 `gpt-5.4` 모델 목록 항목으로
도구 설정을 선택하며 모델 요청은 모의 공급자로 보낸다.
목록의 프롬프트와 다른 필드는 유지하고 다음 값만 바꾼다.

| 모델 목록 필드 | 시험 설정 |
|---|---|
| support_verbosity | false |
| default_verbosity | null |
| default_reasoning_level | null |
| supported_reasoning_levels | 빈 배열 |
| supports_search_tool | false |

격리된 호스트는 추론 메타데이터, tool_search, search_tool, multi_agent와
웹 검색을 끈다. 함수 도구, 패치 입력, 동적 네임스페이스와 승인 처리는 유지한다.
시험은 실제 전송된 HTTP 요청을 검사한다. 모델 목록의 해시는
`5730ed50d14b2432b960cfb821c6de91edcdc70665e650f71f0dff032ab14b8a`다.

변환 이력은 메시지·도구 호출 상태가 생략되었거나 completed이고, 출력 텍스트의
주석이 생략되었거나 비어 있을 때 허용한다. 미완료 호출, 의미 있는 주석,
추론 요약, verbosity, 불투명 상태와 공급자 실행 검색은 거부한다.
도구·텍스트·도구 순서의 assistant 내용은 결과 앞에서 유지하며,
일부 도구 결과만 받은 뒤 assistant가 이어지는 이력은 거부한다.

[공통 지원표](conformance.md)는 Messages 시나리오 13개를 다룬다.
결과 필드 turn_elapsed_ms, first_client_text_ms, interrupt_to_upstream_close_ms는
모의 공급자 경로를 측정하며 실제 모델 지연이나 게이트웨이만의 처리 시간을 뜻하지 않는다.
취소는 중단 요청부터 5000 ms 안에 업스트림 연결을 닫아야 한다.
준비 방법과 명령은 [시험 안내](../../tests/codex/README.ko.md)를 따른다.

<a id="native-output-controls"></a>
<a id="native-출력-제어"></a>
<a id="output-controls"></a>

## 출력 제어

엄격한 도구 인자, 추론 강도와 엄격한 구조화 출력은 프로필에 지원을 명시해야 한다.
Messages는 단계 이름을 바꾸지 않은 output_config.effort와 원래 JSON 스키마
규칙을 담은 output_config.format을 사용한다. 원본 json_schema의 strict=true인
경우만 지원한다. json_object, 느슨하거나 strict를 생략한 스키마,
지원하지 않는 추론 강도는 전송 전에 거부한다.

원본 형식의 이름에 대응하는 Messages 필드는 없다. 이름은 중간 표현과
Responses text.format에 보존하고 스키마 규칙이나 프롬프트에 넣지 않는다.
호스트가 요청한 스키마에 따라 반환 데이터를 검사한다. 추론 강도 이름이
같아도 연산량·비용·모델 동작이 같다는 뜻은 아니다.
[구조화 출력](https://platform.claude.com/docs/en/build-with-claude/structured-outputs)과
[추론 강도](https://platform.claude.com/docs/en/build-with-claude/effort)를 참조한다.
