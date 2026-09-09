<a id="messages-adapter-support"></a>

# Messages 어댑터 지원

[English](../messages.md) | [한국어](messages.md)

G07–G12는 명시적 Messages 경로 아래 요청·JSON 응답·증분 스트림 변환을
제공한다. 아래 프로필은 실제 고정 Codex·모의 upstream과 HTTP 취소 시험을
통과했다. 시험은 합성 데이터만 사용하며 실제 공급자 모델은 qualification하지 않았다.

| 계약 | 구현된 codec 동작 |
|---|---|
| Top-level instructions | Messages system 텍스트 |
| 선행 system/developer 메시지 | 승인된 `bridged_instruction_envelope`를 명시한 경우만 허용 |
| 뒤늦은 system/developer 메시지 | 거부; 대화 내용 앞으로 옮기지 않음 |
| 순서 있는 user/assistant 텍스트 | 블록 순서 보존; 인접한 같은 역할은 Messages 규칙대로 결합 |
| Detail 없는 HTTPS 이미지 참조 | URL source로 매핑, 다운로드 없음; 다른 이미지 입력은 거부 |
| 평면 함수 정의·호출·결과 | Schema, 파싱한 JSON 값, call ID와 결과 연결 보존 |
| 함수 인자 schema 누락 | 인자 없는 함수에 additionalProperties false인 빈 객체 schema |
| 도구 선택·병렬 제한 | Messages tool_choice/disable_parallel_tool_use 매핑과 반환 호출 검증 |
| Custom 텍스트 도구 | 명시적 `bridged_custom_tool_json`, 문자열 하나의 wrapper와 정확한 복원 |
| 순서 있는 namespace 그룹 | 명시적 `bridged_tool_namespace`; 정의·선택·이력·결과·출력이 같은 평면 별칭을 사용하고 설명 보존 |
| 등록 patch 문법 | 명시적 `bridged_codex_patch_grammar`, 정확한 SHA-256/version 선택과 생성 후 구문 검사 |
| Strict 함수 도구 | strict_tool_arguments 지원 선언 시 native strict flag |
| Strict JSON schema 출력 | 규칙을 보존한 native output_config.format; strict_structured_output·structured_output 필수 |
| Reasoning effort | low/medium/high/xhigh/max의 native output_config.effort; 다른 값 거부 |
| Loose JSON schema/json_object, reasoning 요약·상태, verbosity | 명시적 미지원 |
| 비스트리밍 텍스트·도구 | EventIR 검증 후 Responses JSON 생성; 공급자 바이트의 중복 JSON key 거부 |
| 텍스트·함수 인자 스트림 | 고정 ID/index와 순서 있는 sequence number의 증분 Responses 이벤트 |
| Custom 입력 스트림 | 도구별 envelope를 모아 wrapper·문법 검증 후 원래 입력 출력 |
| SSE 경계 | 임의 UTF-8/byte 분할, LF/CRLF/CR, BOM, comment와 여러 줄 data |
| Stream 오류·잘림·미지 의미 이벤트 | 실패; 완료 생성이나 재시도 없음 |
| max_tokens 종료 | 불완전 응답; 토큰 잘림을 완료로 표시하지 않음 |
| 미지 블록·인용·상태·종료 의미 | 의미 출력을 버리지 않고 명시적 오류 |
| Usage | 입력 및 cache-read/cache-created 입력, 출력과 검증된 합계 |
| 공급자 오류 | 기존 HTTP 상태 정제; codec 오류에 공급자 본문 미포함 |

지시 bridge는 원문, 역할 출처와 선행 순서를 보존하지만 Responses의 native
역할 우선순위를 보장하지 못한다. Messages에서만 명시적으로 선택한다.
User/tool 내용은 envelope 밖에 두고 도구 권한·승인은 기존 호스트가 소유한다.
승인된 [지시 bridge 설계](messages-instruction-design.md)를 따른다.

프로필 선언으로 미구현 encoder 기능을 활성화할 수 없다. 순수 capability
admission 이후 codec이 실제 부분집합을 다시 검사한다. 설정·프로필 검증은
공급자나 실제 모델 출력 품질의 인증이 아니다.

반환 모델은 고정한 upstream model과 일치해야 하므로 검증된 구체적 식별자를
설정한다. Response/item ID는 변환 응답을 식별하고 tool call ID는 유지한다.
ID는 조회·영속 응답 의미를 제공하지 않는다. created_at은 변환 시각이며
Messages는 대응 생성 시각을 제공하지 않는다. Cache 토큰은 기록하지만
서로 다른 모델의 토큰 수가 동등하다고 주장하지 않는다.

주요 wire 계약: [Messages](https://platform.claude.com/docs/en/api/messages/create),
[stop reasons](https://platform.claude.com/docs/en/build-with-claude/handling-stop-reasons),
[Responses](https://developers.openai.com/api/reference/typescript/resources/responses/methods/create).
Codec·테스트에 upstream 구현이나 소비자 데이터를 복사하지 않았다.

<a id="tool-and-stream-limits"></a>

## 도구·스트림 한도

Custom/namespace bridge에는 native 함수 지원이 필요하다. Native namespace
의미나 constrained sampling을 주장하지 않는다. Namespace와 원래 이름이
정규 정체성이다. 평면·그룹 도구가 같은 leaf 이름을 가질 수 있지만 중복
유효 정체성과 중첩 그룹은 거부한다. 미지 별칭, 선택·결과 불일치, 중복,
문자열이 아닌 입력과 추가 wrapper 필드는 거부한다.

초기 문법 registry는 `codex-patch/1`이며 선언한 Lark 문법의 hash가
`d6367f4826ed608c424b0a308f3d6163527df63c22513d089b91863552f8bfeb`일 때만 선택한다.
이 선언은 고정 합성 Codex 런타임에서 관측했다. 공개 registry에는 지문과
독립적으로 작성한 구문 검사기만 있고 원래 문법 소스는 없다. SHA-256은 별도
승인된 기존 잠금 패키지 `ring` 0.17.14를 사용하며 새 패키지·feature를 켜지
않는다. Patch 구문만 검사하고 파일 권한·존재·적용 가능성·실행은 호스트가
판정한다. 미지 문법은 거부한다.

SSE framer는 같은 네트워크 chunk의 다음 이벤트를 소비하기 전에 downstream에
내보낼 수 있도록 한 번에 이벤트 하나를 반환한다. 호출자는 양수의 framing·
누적 byte 한도를 제공한다. Event 검증은 delta 1 MiB, 누적 인자 8 MiB,
출력 항목 4096개도 제한한다. 텍스트와 원래 도구 조각은 누적 예산을 공유한다.
message_delta 뒤 message_stop 없이는 완료하지 않는다. EOF나 상태 drop은
성공 종료가 아니다. 실제 HTTP 단절·취소는 전송 계층이 소유한다. G09는 SSE
comment만 보내는 경우를 포함해 소켓 종료와 permit 반환을 검사한다.
max_response_bytes는 각 변환 SSE 이벤트와 누적 출력에도 적용하며 native
SSE는 기존의 비수집 전달을 유지한다.

회귀는 텍스트·병렬 함수·custom stream의 모든 byte 분할, 원문 복원,
namespace 충돌과 선택·이력 매핑, 중복 JSON, 잘못된 wrapper, 조기 EOF,
오류·미지 이벤트·순서와 누적 한도를 검사한다. 이는 아래 실제 Codex 시험을
보완하며 공급자 qualification은 아니다. 스트림 계약은 공식
[Messages streaming](https://platform.claude.com/docs/en/build-with-claude/streaming)을 따른다.

<a id="qualified-synthetic-codex-profile"></a>

## 검증한 합성 Codex 프로필

시험 artifact는 [런타임 lock](../../tests/codex/runtime-lock.json)의 임시
`0.154.0-alpha.6` / macOS ARM64다. 호스트는 내장 `gpt-5.4` catalog로 도구
계약을 선택하고 모든 모델 요청은 합성 upstream model로 전달한다. 실제
GPT-5.4 공급자 호출이 아니다. 원래 prompt와 다른 모델 필드는 보존하면서
다음 기능만 명시적으로 바꾼다.

| Catalog 필드 | 시험 설정 |
|---|---|
| support_verbosity | false |
| default_verbosity | null |
| default_reasoning_level | null |
| supported_reasoning_levels | 빈 배열 |
| supports_search_tool | false |

격리 설정은 model reasoning metadata, tool_search, search_tool, multi_agent와
web search를 끈다. 함수, 등록된 자유 형식 patch, dynamic namespace와 승인
처리는 유지한다. 일반 설정 flag만으로 미지원 필드가 모두 없어지지는 않았기
때문에 최종 wire 요청을 검사한다. 고정 프로필의 정규 catalog digest는
`5730ed50d14b2432b960cfb821c6de91edcdc70665e650f71f0dff032ab14b8a`다.
Catalog는 실행 시 도출하고 공개 fixture에 복사하지 않는다.

메시지·도구 status와 output-text annotations는 typed IR 필드다. Native
왕복은 이를 유지한다. 변환 이력은 생략/completed status와 생략/빈 annotations만
받고 미완료·의미 있는 주석은 거부한다. Assistant의 tool-use/text/tool-use 블록
순서는 결과 앞에서 유지한다. 일부 결과 뒤 assistant가 이어지는 경우는 거부한다.
미지 필수 필드, reasoning 요약, verbosity, opaque state와 hosted search는 미지원이다.

실제 Codex의 13개 시나리오는 텍스트, 함수, namespace, patch 적용·재입력,
승인 거절, 병렬 호출 두 개, 후속 텍스트, 이벤트·heartbeat 취소, 전송 단절,
실행 전 문법 실패, high effort·strict schema, 도구·텍스트 혼합 재입력을
검사한다. 기본 CI는 세 경로 전체 35개를 실행한다. [공통 행렬](conformance.md)을
참조한다. 로컬 HTTP 시험은 비스트리밍 JSON, auth/version 헤더, 전송 전
거부, 증분 바이트, 정제 오류, EOF와 누적 한도도 확인한다.

결과는 payload 없이 turn_elapsed_ms, 텍스트 관측 시 first_client_text_ms,
취소 시 interrupt_to_upstream_close_ms를 기록한다. 호스트 처리를 포함한 합성
제어·HTTP 경로의 측정이며 함수 시나리오의 첫 텍스트는 도구 왕복 뒤에 온다.
실제 모델 지연, 분리된 gateway overhead나 생산 SLA가 아니다. 2026-09-08의
한 로컬 실행은 텍스트 첫 출력 27.520 ms, 이벤트·heartbeat 취소의 upstream
종료 109.056 ms / 104.698 ms를 관측했다. 기준은 interrupt부터 5000 ms이며
CI가 다시 실행해 새 시간을 기록한다.

공급자별 컨텍스트 계수, 실제 모델 동작, 소비자 통합, 장기 연속성과 릴리스
수락은 별도 단계다. 프로필 설정이나 합성 성공이 운영자의 모델을 인증하지
않는다. [Messages 설정 예제](../../config.messages.example.toml)를 참조한다.

Messages와 Chat Completions는 반환된 도구 선택·호출 수·원래 정체성의 검증을
공유한다. API별 finish 처리와 지시 계층 변환 범위는 각각의 계약을 유지한다.

<a id="native-output-controls"></a>

## Native 출력 제어

Strict 도구, reasoning effort와 strict 구조화 출력은 명시적 프로필 지원이
필요하다. Messages는 이름을 바꾸지 않은 output_config.effort와 원래 schema의
output_config.format을 사용한다. Strict=true인 source json_schema만 지원한다.
json_object, loose/strict 미지정 schema와 미지원 effort는 전송 전에 거부한다.
원래 format 이름은 Messages wire 필드가 없는 descriptor이며 정규 IR과 응답의
Responses text.format에 보존한다. Schema 규칙이나 prompt에 넣지 않는다.
Schema·strict 도구 제약은 공급자·호스트 계약이며 gateway가 두 번째 JSON Schema
구현을 제공하지 않는다.

같은 effort 이름이 공급자 간 같은 reasoning·연산·비용을 뜻하지 않는다.
Schema dialect, 모델별 기능과 출력 준수에는 공급자 qualification과 호스트
검증이 필요하다. 합성 시나리오는 제어값의 wire 도달과 알려진 유효 JSON의
Codex 도달을 확인하며 모델 준수를 인증하지 않는다. 주요 계약은
[structured outputs](https://platform.claude.com/docs/en/build-with-claude/structured-outputs)와
[effort](https://platform.claude.com/docs/en/build-with-claude/effort)다.
