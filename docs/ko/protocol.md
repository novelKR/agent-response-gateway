<a id="http-support-contract"></a>

# HTTP 지원 계약

[English](../protocol.md) | [한국어](protocol.md)

게이트웨이는 Responses 요청을 Responses, Messages, Chat Completions 또는 명시적으로
활성화한 [Gemini Interactions 경로](interactions.md)로 전달한다. 이 문서는 인증,
요청 처리, 한도와 오류를 정의한다. 기존 세 경로는 상태를 저장하지 않으며
Interactions는 영속 provider continuation을 추가한다. 도구·승인·Codex 이력은
호스트 애플리케이션이 담당한다.
Rust 라이브러리 타입은 [IR 참조](ir.md)에 설명되어 있다.

<a id="configuration-and-authentication"></a>

## 설정과 인증

프로세스는 `config.example.toml` 구조의 TOML을 읽는다. 알 수 없는 설정 키,
등록되지 않은 공급자 참조, 잘못된 제한값은 시작 전에 거부한다. 설정을
자동 재로딩하지 않으며 변경 적용에는 재시작이 필요하다.

리스너는 숫자 루프백 주소만 허용한다. 기본값은 `127.0.0.1:0`이다.
공급자 URL은 HTTPS를 사용하고, 모의 서버를 위한 숫자 루프백 호스트에만
HTTP를 허용한다. URL의 사용자정보·query·fragment는 허용하지 않는다.
주소와 모델 경로는 운영자 설정에서만 선택하며 요청 본문으로 새로운
공급자를 지정할 수 없다. 환경의 HTTP 프록시와 리디렉션를 사용하지 않는다.

로컬 토큰은 32~4096자의 공백 없는 ASCII이고 공급자 키와 달라야 한다.
소비자 `Authorization`을 업스트림에 전달하지 않고 해당 공급자의 키로
선언한 `auth`에 따라 Bearer, `x-api-key` 또는 `x-goog-api-key` 헤더를 구성한다. 기본값은 Bearer다.
소비자 쿠키·임의 헤더도 전달하지 않는다.
브라우저 애플리케이션용 CORS나 공개 서비스 인증은 제공하지 않는다.

<a id="endpoints"></a>

## 엔드포인트

| 요청 | 인증 | 의미 |
|---|---|---|
| `GET /` | 없음 | 이름·버전·라이선스와 선택적 `source_url` |
| `GET /healthz` | 없음 | 프로세스가 응답함 |
| `GET /readyz` | 없음 | 시작에 필요한 로컬 구성이 준비됨; `provider_probe:false` |
| `GET /v1/models` | 로컬 Bearer | 설정에 등록된 모델명 목록 |
| `POST /v1/responses` | 로컬 Bearer | 등록 경로로 한 번 전달 |

준비 상태는 공급자 키의 유효성, 외부 연결이나 실제 모델의 기능을 검사하지
않는다. 루트에 표시되는 소스 URL도 온라인으로 접속을 확인한 결과가 아니다.

<a id="requests-and-responses"></a>

## 요청·응답

JSON 객체와 등록된 문자열 `model`을 받는다. 호환성 정책을 선택하지 않은 Responses 경로는 모델명을
공급자 모델명으로 치환하며 나머지 필드를 아래 제한 외에는 JSON 값으로 보존한다.
Messages·Chat Completions 경로는 등록된 기능 부분집합을 변환하며 미지원 필드는 전송 전에 거부한다.
아래 표의 원형 전달 규칙은 Responses 경로에 적용한다.
`stream:true`이면 SSE, 생략하거나 false이면 JSON 응답을 사용한다.

| 입력 | 처리 |
|---|---|
| `store` 생략 또는 false | 업스트림에 `store:false` 전달 |
| `store:true` | 미지원 오류 |
| null이 아닌 `previous_response_id`, `conversation`, `context_management` | 상태·압축 기능 미지원 오류 |
| `input`의 `item_reference`(생략/null type의 ID 참조 포함), `compaction`, `compaction_trigger` | 저장 항목 참조·압축 기능 미지원 오류 |
| `background:true` | 미지원 오류 |
| 함수·사용자 정의 도구·구조화 출력·추론 필드 | JSON 값 보존; 공급자 기능 보장은 별도 검증 필요 |

Responses 경로는 response ID를 저장·변환하지 않고 정상 응답의 model 필드를
다시 쓰지 않는다. 변환 경로는 공급자 ID를 기반으로 Responses 응답·항목 ID를
만들며 원래 도구 호출 ID를 유지한다. Interactions는 영속 로컬 attempt ID를
사용하고 암호화된 재생 상태를 저장한다. 공개 response 조회나 previous_response_id
해석은 어느 경로에서도 제공하지 않는다. 공급자의 오류 본문은 프롬프트나 자격 증명을 되돌려줄 수 있으므로
전달하지 않고 로컬 오류와 HTTP 상태를 반환한다. redirect는 따라가지 않고
502로 처리한다. 정상 응답과 SSE 이벤트 본문은 전달되므로 공유 서비스나
테넌트 격리 용도로 이 계약을 확장 해석하지 않는다.

Responses SSE 본문은 전체 수집이나 JSON 재해석 없이 전달한다. 변환 경로는 SSE를
증분 해석하고 검증된 Responses 이벤트를 출력한다. 청크 경계는 이벤트·문자·
JSON 경계가 아닐 수 있다. 이미 전송을 시작한 스트림에서 실패하면 연결을
종료하고 다른 공급자 응답을 이어 붙이거나 성공 종료 이벤트를 만들어내지 않는다.
상위 소비자는 최종 완료 이벤트 없이 끝난 스트림을 성공으로 간주해서는 안 된다.
공급자에는 `Accept-Encoding: identity`를 요청하며 압축된 본문은 502로 거부한다.
JSON 숫자는 임의 정밀도로 파싱하여 큰 정수와 소수의 값을 보존한다.

<a id="checked-responses-tools"></a>
<a id="explicit-tool-compatibility-policies"></a>

## 명시적 도구 호환성 정책

모델은 `compatibility_policy`로 버전이 지정된 정책을 선택한다.
`[compatibility_policies.NAME]` 정의만으로는 활성화되지 않는다.
명시적인 `auth`와 `capability_profile`이 필요하다. 공급자 프로필은 지원을
선언하고, 정책은 게이트웨이가 적용할 변환을 선택한다. 이 선언은 실제 공급자의
적합성 입증이 아니다. [설정 예제](../../config.checked-responses.example.toml)를 참고한다.

| 정책 필드 | 값 | 효과 |
|---|---|---|
| `version` | `1` | 알 수 없는 정책 버전 거절 |
| `tools.custom_input` | `preserve`, `function_json` | custom 입력을 보존하거나 정확한 문자열을 function JSON 객체로 감싸기 |
| `tools.namespaces` | `preserve`, `flatten` | namespace 그룹을 보존하거나 구성원을 충돌 없는 함수 이름으로 매핑 |
| `tools.grammar` | `preserve`, `registered_output_validation` | 선언된 형식을 보존하거나 등록된 grammar의 출력을 로컬에서 검증 |

선택을 생략하면 기존 브리지 선언을 따른다. 명시적 선택은 기존 브리지와 일치해야
하며, 선언된 native 지원과 충돌하는 변환은 거절한다. 정책은 지원되지 않는
보존을 native 지원으로 바꾸지 않는다. 함수 래핑과 namespace 평탄화에는 native
function 도구 지원이 필요하다. grammar 검증에는 custom JSON 래퍼 또는 native
Responses custom 입력이 필요하다. 래핑은 native custom grammar 생성을 유지할 수 없다.

Responses 경로에서 정책을 선택하면 checked 입력 검사와 변환이 활성화된다.
선택한 정책이 없으면 기존 Responses 값·바이트 전달 계약이 적용된다.
Messages, Chat Completions, Interactions는 기존 프로토콜·연속성 계약 안에서
같은 선택 도구 규칙을 사용한다. 요청별 레지스트리는 정의, 설명, 이름을 지정한
선택, 이력의 호출·결과, 반환 식별성을 함께 처리하며 도구를 실행하지 않는다.

| Checked Responses 영역 | 지원과 제한 |
|---|---|
| 요청·이력 | 선언된 텍스트·이미지, 지시문, function/custom 도구와 대응하는 문자열 결과; 알 수 없는 의미 필드와 opaque 이력 거절 |
| 출력 제어 | 선언된 출력 한도, 샘플링, reasoning 옵션과 구조화 형식의 값 보존; 스키마 검증은 호스트 책임 |
| Custom 형식 | 형식 생략, 정확한 text 형식 또는 등록된 Codex patch grammar; 알 수 없는 grammar는 전송 전 거절 |
| JSON 출력 | 전달 전에 응답 식별성, 모델, 항목·호출 고유성, 도구 선택, 병렬 개수, 래퍼 형태와 사용량 카운터 검증 |
| SSE 출력 | 이벤트 순서, 항목·부분의 생명주기, delta, done 값과 최종 출력 검증; 텍스트와 공개 reasoning 요약은 점진적으로 전달 |
| 도구 완료 | 전체 최종 응답 검증까지 도구 생명주기 이벤트 보류; durable 사용량 기록 시 최종 로컬 커밋까지 대기 |
| 미지원 출력 | Opaque reasoning, 비어 있지 않은 annotations/logprobs, 알 수 없는 항목·이벤트, 불일치하거나 닫히지 않은 최종 항목을 명시적으로 거절 |

checked 경로는 무상태다. 인식된 metadata/cache 힌트와 선택적인
`include:["reasoning.encrypted_content"]` 요청 힌트를 보존하지만 암호화된
reasoning 출력이나 재생을 허용하지 않는다. opaque 연속성이 필요하면 별도로
지원되는 managed 경로를 선택한다. 공개 reasoning 요약에는 해당 기능 선언이
필요하다. 메시지의 `phase`는 출력과 이력에서 `commentary`, `final_answer` 또는
null을 보존한다. 스트림 실패 시 가상의 성공을 만들지 않고 연결을 닫는다.
checked 이벤트 부분집합은 [Responses 스트리밍 참조](https://developers.openai.com/api/reference/resources/responses/streaming-events)를 따른다.

grammar 규칙은 생성 후 구문을 검증하며, 제한 디코딩이나 승인 또는 파일 적용 가능성을
제공하지 않는다. 변환된 설명에 grammar를 보존하고 응답의 설정 반환값에서 원래
도구 정의를 복원한다. checked 출력에는 버퍼·이벤트 한도가 적용되므로 큰 도구 호출은
완료 노출 전에 실패할 수 있다. 정책은 재시도, 대체 경로 선택, 규칙 완화를 하지 않는다.

<a id="resources-and-failures"></a>

## 자원과 실패

TOML의 `[limits]`는 다음 한도를 각각 정의한다. 시간 설정은 밀리초,
본문 설정은 바이트 단위다.

| 설정 | 기본값 | 적용 범위 |
|---|---|---|
| `max_request_bytes` | 8388608 (8 MiB) | 수신하는 요청 본문 전체 |
| `max_response_bytes` | 16777216 (16 MiB) | 버퍼에 모으는 업스트림 JSON; 변환 SSE의 개별 이벤트와 누적 출력; 네이티브 SSE 전체 바이트 상한은 아님 |
| `max_in_flight` | 32 | 인스턴스의 모든 별칭과 공급자에 걸친 활성 모델 요청 전체 |
| `request_body_timeout_ms` | 30000 | 로컬 요청 본문 수신에 쓰는 전체 시간이며 청크별 유휴 타이머가 아님 |
| `connect_timeout_ms` | 10000 | 업스트림 연결 수립 |
| `response_header_timeout_ms` | 60000 | 연결과 요청 전송을 포함해 업스트림 전송 시작부터 응답 헤더 수신까지 |
| `stream_idle_timeout_ms` | 60000 | 헤더 이후 다음 업스트림 본문 청크를 기다리는 시간; JSON과 SSE 모두 적용 |
| `shutdown_grace_ms` | 5000 | 서버 작업을 중단하기 전 정상 HTTP 종료 유예 |

모델 요청 본문을 읽기 전에 슬롯을 얻는다. 비스트리밍 요청은 업스트림 본문을
모아 처리한 뒤 반환하고, 스트리밍 응답은 소비를 끝내거나 닫을 때까지 유지한다.
대기열은 없으며 초과 요청은 업스트림 호출 없이 `429 capacity_exceeded`를 받는다.
열린 스트림은 슬롯을 유지한다. 소비자가 느리게 읽으면 업스트림 읽기도 느려진다.

heartbeat를 포함한 본문 데이터가 계속 오면 유휴 타이머가 만료되지 않을 수 있다.
이 설정들은 모델 호출 전체에 하나의 최종 기한을 두지 않는다. 전체 기한과 취소는
호스트가 관리한다. 네이티브 스트림은 전체 수집 없이 전달한다. 변환 경로는 최종
출력을 위해 텍스트와 도구 입력을 제한된 버퍼에 유지하고 검증 후 사용자 정의
입력의 포장 구조를 복원한다.

소비자가 스트림을 끊으면 업스트림 읽기를 종료한다. 서버의 정상 종료는
진행 요청에 제한된 유예 시간을 부여한다. 이미 공급자가 처리한 요청의
취소나 비용 회수를 보장하지 않는다.

게이트웨이는 자동 재시도와 대체 경로를 사용하지 않는다. 로컬 인증, 공통 요청
검증과 경로 허용 검사 실패는 공급자 요청 전에 발생한다. 연결·시간 초과·본문
크기 초과와 공급자 HTTP 오류를 성공으로 바꾸지 않는다. 결과가 불확실한 요청의
재시도 정책과 실행 기록은 소비자가 관리한다.

로그는 요청·경로 식별자, 상태와 소요 시간 같은 운영 메타데이터로 제한한다.
본문, 프롬프트, 인증정보와 전체 HTTP 헤더 덤프를 로그에 넣지 않는다.

<a id="gateway-error-responses"></a>

## 게이트웨이 오류 응답

게이트웨이가 생성하는 API 오류는 다음 JSON 형식을 사용한다. HTTP 상태는
별도로 전달하며 같은 상태의 실패를 `error.code`로 구분한다. HTTP 계층이나
클라이언트 전송 오류에는 이 형식이 없을 수 있다.

```json
{
  "error": {
    "code": "unauthorized",
    "message": "A valid local bearer token is required",
    "type": "gateway_error"
  }
}
```

아래 표의 업스트림 호출은 게이트웨이가 공급자와 HTTP 통신을 시도했다는 뜻이며,
공급자가 모델 작업을 실행하거나 과금했다는 뜻은 아니다.

| HTTP 상태 | `error.code` | 발생 단계 | 업스트림 호출 | 조치 |
|---|---|---|---|---|
| 401 | `unauthorized` | 로컬 인증 | 없음 | 로컬 Bearer 토큰 수정 |
| 404 | `not_found` | 엔드포인트 선택 | 없음 | 지원하는 게이트웨이 경로 사용 |
| 404 | `model_not_found` | 별칭 조회 | 없음 | 등록된 모델 별칭 선택 |
| 400 | `invalid_body`, `invalid_json`, `invalid_request`, `invalid_model` | 요청 본문·공통 형식 | 없음 | JSON 객체, 모델과 필드 타입 수정 |
| 400 | `unsupported_feature` | 상태 저장 금지 정책 | 없음 | 미지원 저장·상태 요청을 제거하고 현재 문맥 전송 |
| 400 | `unsupported_request` | 경로 한도·프로필·변환 허용 검사 | 없음 | 필수 기능, 한도와 완전한 도구 이력 확인 |
| 408 | `request_timeout` | 로컬 본문 수신 | 없음 | 요청 본문 기한 안에 업로드 완료 |
| 413 | `request_too_large` | 로컬 본문 수신 | 없음 | 요청 바이트를 줄이거나 적절한 한도 명시 |
| 415 | `unsupported_media_type` | 로컬 콘텐츠 타입 | 없음 | application/json 전송 |
| 429 | `capacity_exceeded` | 로컬 허용 검사 | 없음 | 소비자 동시 호출 수를 제한하고 끝난 응답을 닫음 |
| 501 | `unsupported_endpoint` | 저장 응답 엔드포인트 | 없음 | 조회·삭제·원격 압축 대신 호스트 이력 사용 |
| 공급자 상태; 리디렉션은 502 | `upstream_error` | 공급자 응답 헤더 | 있음 | 자동 재실행 없이 선택한 공급자·키와 상태 확인 |
| 502 | `upstream_unavailable` | 업스트림 전송 | 가능 | 연결을 확인하고 결과 불명 시도 보존 |
| 504 | `upstream_timeout` | 업스트림 전송·버퍼 본문 읽기 | 가능 또는 이미 헤더 수신 | 재시도 전에 대기 구간을 확인하고 결과 상태 보존 |
| 502 | `upstream_content_encoding`, `upstream_content_type` | 공급자 응답 헤더 | 있음 | identity 인코딩과 예상 JSON·SSE 콘텐츠 타입 확인 |
| 502 | `upstream_read_error`, `upstream_response_too_large` | 버퍼 응답 본문 | 있음 | 전송 완료나 응답 바이트 예산 확인 |
| 502 | `upstream_invalid_json`, `upstream_invalid_response` | 네이티브 JSON·변환 응답 검증 | 있음 | 선택한 공급자의 응답 계약 확인 |

예를 들어 로컬 `401 unauthorized`는 `401 upstream_error`와 다르고,
로컬 `429 capacity_exceeded`는 `429 upstream_error`와 다르다. 전송 후 오류를
받았다는 사실만으로 공급자가 작업했는지 확정할 수 없다. 게이트웨이는 공급자
오류 본문과 `Retry-After`, 공급자 요청 식별자 등 임의의 응답 헤더를 전달하지 않는다.

게이트웨이는 새 `x-request-id`를 생성해 소비자에게 반환하고 같은 ID를 업스트림에
보낸다. 로컬 로그의 `request_id`와 연결하며 소비자가 보낸 요청 ID는 재사용하지
않는다. 이 ID가 중복 제거, 응답 조회나 안전한 재시도를 제공하지는 않는다.

SSE 헤더 전송 후 전송·변환 오류가 발생하면 상태를 위 JSON 오류로 교체하지 않고
스트림을 닫는다. 종료 이벤트와 로컬 본문 종료 결과를 확인한다.
진단은 [문제 해결](troubleshooting.md), 애플리케이션 처리는
[스트림 소비](usage.md#read-a-stream-and-handle-cancellation)를 참조한다.

<a id="declared-routes-and-capability-profiles"></a>

## 선언된 경로와 기능 프로필

모델에 `api`, `auth`, `capability_profile`, `messages_version`을 선언할 수 있다.
기본 API는 Bearer 인증의 `responses`다. Messages와 Chat Completions에는
지원 기능 프로필을 명시해야 한다. [세 API 지원표](conformance.md)는
변환 기능과 모의 공급자 시험 범위를 정리한다.

프로필은 공급자·실제 모델·API를 모델 매핑과 일치시켜야 한다. 키가 프로필 ID이며
`version`, `tested_codex_version`, `context_window`, `max_output_tokens`을 명시한다.
이 값은 운영자의 선언이며 런타임·공급자의 호환성을 검사한 결과가 아니다.
`support`는 기능 이름을 `native`, `unsupported` 또는 구현된 변환 규칙 선언에
매핑하며 누락된 기능은 Unsupported다. namespaced_tools에는
`bridged_tool_namespace`, custom_grammar에는 `bridged_codex_patch_grammar`를
선언할 수 있고 두 도구 변환 규칙에는 공급자의 직접 함수 지원이 필요하다. custom_tools에는 JSON 변환 규칙을,
Messages의 instruction_hierarchy에는 승인된 bridged_instruction_envelope를
명시할 수 있다. 설정 예시는 `config.example.toml`의 선택적 프로필을 참조한다.

프로필이 있는 경로는 요청한 `max_output_tokens`가 양의 정수인지와 선언 한도
이하인지를 전송 전에 검사한다. 입력 토큰 계수는 구현·검증되지 않았으며,
`context_window`는 호스트 설정용 계약이다. 프로필 없는 Responses 원형 전달은
모델 기능을 검증하지 않고 요청을 전달한다. 프로필이 있는 Responses 경로는 호환성 정책을 선택하지 않으면 원형 JSON과
SSE를 보존한다. 정책을 선택하면 기능별 의미 검사도 적용한다.

각 HTTP 요청은 해석한 공급자·모델·API·인증 참조·프로필·한도를 한 번 고정한다.
여기서 환경 변수 이름은 요청 시 자격 증명을 선택하는 참조일 뿐, 이력 재개를
증명하는 자격 증명 세대가 아니다. 영속 재개는 별도 연속성 계약을 따른다.
변환 지원 검사은 명시한 client_metadata·prompt_cache_key·선택적 encrypted
reasoning 출력 요청만 전송 힌트로 제외한다. 알 수 없는 확장과 opaque 입력은
거부한다. 네임스페이스·grammar 변환 규칙과 실제 Codex 합성 시험 범위는
[Messages 지원표](messages.md)와 [Chat 지원표](chat-completions.md)에 기록한다. 실제 공급자 모델 검증은 별도다.

<a id="offline-embedded-contract"></a>

## 오프라인 내장 계약

`manifest --config`는 서버와 같은 설정 검증 및 경로 해석으로 정규화된 JSON과
설정 해시를 만든다. listener·환경 변수의 키 값·공급자에 접근하지 않는다.
`serve`의 첫 준비 JSON에는 주소·버전과 함께 `schema`, `manifest_schema`,
`configuration_sha256`가 포함된다. HTTP 관리 엔드포인트는 없다.
상세 스키마와 호스트 책임은 [내장 계약](embedded-design.md)에 명시한다.

함수 도구 형식과 명시적인 사용자 정의 텍스트 변환은
[Chat Completions 지원](chat-completions.md)을 참조한다.
