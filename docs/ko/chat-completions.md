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
| 추론 요약과 원본 상태 | 명시적인 managed DeepSeek/OpenRouter 계약에서만 지원; [관리형 reasoning](#managed-reasoning) 참고 |
| verbosity | 미지원 |
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

<a id="managed-reasoning"></a>

## 관리형 reasoning

표준 OpenAI Chat은 stateless를 유지하며 제공자별 reasoning 필드를 허용하지 않는다.
reasoning dialect를 사용하려면 Chat 모델에 continuation_mode="managed"를 설정하고
reasoning_contract를 명시한다. 먼저 [저장소·보호 키·호스트 세션](interactions.md#host-control-and-resume)을
설정한다. DB나 알려진 endpoint/model 이름에서 dialect를 추론하지 않는다.
capability profile에는 reasoning_summary="native"와 reasoning_items="native"가
필요하며 effort를 요청하면 reasoning_effort="native"도 선언한다. 기존의
provider/model/API 및 맥락·출력 한도 선언도 필요하다.

DeepSeek 계약 설정 일부는 다음과 같다.

```toml
[capability_profiles.deepseek.reasoning_contract]
kind = "deep_seek"
version = 1
efforts = ["low", "high", "max"]
default_effort = "high"
```

thinking.type="enabled"와 native reasoning_effort를 사용한다. medium, xhigh,
minimal과 같은 별칭은 제공자 재매핑을 수락하지 않고 거부한다. 출력 한도는
max_tokens를 사용하며 프로필 한도와 393216을 넘지 않는다. temperature, 강제 도구,
strict tool beta와 JSON-schema 출력은 이 계약에서 제외한다. top_p는 제공자의
자동 상향을 피하도록 0.95 이상이어야 한다. text/json_object도 프로필의 명시적
지원 범위를 따라야 한다.

선행 system/developer 지시에는 instruction_hierarchy="bridged_chat_instruction_envelope"를
사용한다. 명시적인 system 메시지 bridge가 지시의 역할·순서·텍스트를 기록하며,
원래 지시 우선순위와 완전히 같다고 주장하지 않는다. 이 프로필은 대화 중간 지시를
거부한다. parallel_tool_control="bridged_parallel_permission"은 미지원 필드인
parallel_tool_calls를 보내지 않고 병렬 호출을 허용한다. false는 호스트 압축이나
tool_choice="none"처럼 도구를 호출할 수 없는 경우에만 허용하며, 그 외에는 전송
전에 거부한다. 호출 개수·정체성·인자·결과 연결은 계속 검사한다.

OpenRouter 계약 설정 일부는 다음과 같다.

```toml
[capability_profiles.router.reasoning_contract]
kind = "open_router"
version = 1
provider_endpoint = "YOUR_PROVIDER/YOUR_EXACT_ENDPOINT"
efforts = ["low", "high"]
default_effort = "high"
formats = ["anthropic-claude-v1"]
```

provider/endpoint 부분이 있는, 별도로 검증한 정확한 endpoint slug를 고정한다.
요청은 해당 값 하나를 가진 provider.only와 allow_fallbacks=false,
require_parameters=true를 사용한다. OpenRouter 자동 모델명·라우팅 변형·다중 모델
fallback은 제외한다. 고정한 model/endpoint에 맞는 efforts와 formats를 선택하며,
게이트웨이가 제공자의 capability를 자동 탐색하지 않는다.

고정 reasoning 토큰 예산을 사용하려면 reasoning_contract 표의 efforts/default_effort를
max_tokens=2048로 교체한다. effort 정책과 토큰 예산을 한 계약에 혼용할 수 없으며,
예산 프로필의 요청에 effort를 추가할 수도 없다. 예산은 모델 출력 한도보다 작아야 한다.
OpenRouter에는 reasoning.enabled=true와 reasoning.exclude=false 및 한 가지 제어
형식만 전달한다.

DeepSeek는 후속 요청에 도구가 선언되면, 도구를 사용하지 않았던 과거 턴을 포함해
assistant의 원래 reasoning_content를 재생한다. OpenRouter는 reasoning과 순서가
있는 reasoning_details의 id·index·format·signature·암호화 데이터를 보존한다.
등록된 detail type과 프로필이 선택한 format만 허용한다. details가 있으면 공개
text/summary detail을 표시 기준으로 삼으며 평문 reasoning을 중복 표시하지 않는다.
평문만 있는 스트림은 terminal에서 details가 없음을 확인할 때까지 표시를 보류한다.
암호화 상태만 있으면 공개 텍스트를 만들어 내지 않는다. 두 dialect 모두
reasoning.summary="auto"를 지원하며 concise/detailed는 거부한다.

공개 summary와 원본 replay는 별개다. 응답의 첫 reasoning 항목에 인증된 gateway
envelope 하나를 결합하며, 공개 텍스트가 없는 상태는 빈 항목으로 전달한다. 도구 완료와
성공 terminal은 finalize 이후에만 공개한다. pending tool 턴에서는 reasoning 제어를
유지해야 한다. 재시작·누락 payload 복원·호스트 관리 압축은 공통 continuation 계약을
사용한다. dialect/model/route 변경에는 새 세션이 필요하며 제공자 상태를 자동 변환하지 않는다.

usage는 보고된 수치 카운터를 공개 텍스트와 별도로 보존한다. reasoning 토큰은 이미
completion 토큰에 포함되므로 다시 더하지 않는다. DeepSeek cache-hit는 prompt 토큰의
일부이며 추가 입력이 아니다. 누락 카운터는 unknown으로 유지한다. 필수 합계가
불완전하면 Codex용 usage 객체는 null이며 어댑터 수치 메타데이터에는 확인된 카운터를
유지한다. 알 수 없는 선택적 상세값은 생략하고 텍스트 길이로 토큰 수를 추산하지 않는다.

두 계약은 Codex 0.154.0과 합성 제공자로 reasoning 알림·도구·재시작·payload 복원·압축·
명시 복구를 검사한다. [reasoning wire lock](../../tests/reasoning/wire-lock.json)과
[DeepSeek API 보충 계약](../../tests/reasoning/deepseek-api-lock.json)을 참고한다. 실제 제공자
모델 호환성·품질·비용 qualification은 별도이다.
