<a id="troubleshooting"></a>

# 문제 해결

[English](../troubleshooting.md) | [한국어](troubleshooting.md)

키, 모델 설정이나 재시도 정책을 바꾸기 전에 실패한 단계를 확인한다.
HTTP 상태, 게이트웨이 오류 코드와 요청 ID를 함께 사용한다.
[HTTP 계약](protocol.md#gateway-error-responses)은 오류 형식과 한도를,
[호출 예제](usage.md)는 올바른 요청 형식을 설명한다.

<a id="the-gateway-does-not-start"></a>

## 게이트웨이가 시작되지 않음

| 증상 | 확인할 값 | 원인 | 조치 |
|---|---|---|---|
| 설정 거부 | 선택한 TOML, 공급자·모델 참조, API와 프로필 연결 | 알 수 없는 필드, 없는 참조나 일치하지 않는 선언 | 아래 오프라인 검사 두 개를 실행하고 선택한 설정 예시와 비교 |
| 키 환경변수 누락 | 게이트웨이 자식 환경과 모든 공급자 항목 | 사용하지 않는 항목을 포함해 등록된 모든 공급자 키를 시작 시 읽음 | 필요한 변수를 제공하거나 설정하지 않을 항목을 제거; 키의 실제 값은 TOML에 넣지 않음 |
| 로컬 토큰 오류 | `ARG_LOCAL_TOKEN` 또는 `local_token_env`로 지정한 이름 | 토큰 누락, 짧은 길이, 공백 포함 또는 공급자 키와 동일 | 별도의 32~4096자 출력 가능한 공백 없는 ASCII 토큰을 쓰고 소비자에도 같은 로컬 토큰 전달 |
| 리스너 바인딩 실패 | 설정한 숫자 루프백 주소와 포트 소유자 | 다른 프로세스가 포트를 사용하거나 주소를 사용할 수 없음 | `127.0.0.1:0`과 준비 정보를 우선 사용하고 다른 프로세스를 임의 종료하지 않음 |
| 준비 메시지가 오지 않음 | 자식 종료 상태, stderr와 호스트의 시작 기한 | 시작 실패 또는 호스트의 자식 파이프 읽기 문제 | 시간 초과 시 자신이 시작한 자식을 종료하고 로컬 오류와 프로세스 수명 계약 확인 |

```sh
cargo run --locked -- check-config --config config.local.toml
cargo run --locked -- manifest --config config.local.toml
```

이 명령은 인증정보를 읽거나 공급자에 접속하지 않고 설정을 검사하고 정규화된
내용을 보여준다. 검사가 성공해도 `serve`에 필요한 환경변수가 있다는 뜻은 아니다.
환경이나 TOML 변경은 실행 중인 인스턴스가 아닌 다음 프로세스 시작에 적용된다.
[시작과 종료](embedded-design.md#process-lifecycle)를 참조한다.

<a id="the-consumer-cannot-connect-or-authenticate"></a>

## 소비자가 접속하거나 인증할 수 없음

| 증상 | 확인할 값 | 원인 | 조치 |
|---|---|---|---|
| 연결 거부 | 준비 정보의 주소와 게이트웨이 프로세스 | 잘못되거나 오래된 포트, 종료된 자식 또는 다른 네트워크 네임스페이스 | 실제 준비 URL과 지원하는 로컬 배치 사용 |
| 잘못된 경로나 404 | 소비자 URL과 `error.code` | 공급자 엔드포인트로 소비자가 호출하거나 `/v1` 중복 또는 별칭 누락 | 준비 기본 주소에 `/responses`를 한 번 붙이고 `not_found`와 `model_not_found` 구분 |
| `401 unauthorized` | 로컬 토큰을 넣은 `Authorization: Bearer ...` 헤더 한 개 | 전송 전 게이트웨이 인증 실패 | 로컬 토큰을 바로잡고 공급자 키로 대체하지 않음 |
| `401 upstream_error` 또는 `403 upstream_error` | 선택한 별칭, 공급자 키 참조와 해당 키의 공급자 권한 | 선택한 공급자가 인증정보나 접근을 거부 | 공급자 인증정보·권한을 수정하고 키가 바뀌면 재시작 |
| 브라우저 CORS 오류 또는 다른 컨테이너에서 연결 실패 | 브라우저 출처와 네트워크 네임스페이스 | 브라우저 CORS, 공개 서비스·별도 컨테이너 배포 미지원 | 지원하는 같은 호스트 또는 같은 네트워크 네임스페이스의 백엔드 통합 사용 |

게이트웨이는 소비자의 공급자 인증 헤더를 교체한다. `x-api-key`를 보내도 다른
키가 선택되지 않는다. [여러 키 사용](route-design.md#multiple-api-keys-for-one-provider)에
설명한 등록 별칭을 사용한다. 로컬 토큰은 해당 인스턴스의 모든 별칭을 허용하며
사용자별 권한 경계가 아니다.

<a id="a-request-is-rejected-before-reaching-the-model"></a>

## 모델에 도달하기 전에 요청이 거부됨

| 증상 | 확인할 값 | 원인 | 조치 |
|---|---|---|---|
| `invalid_json`, `invalid_request`, `invalid_model`인 400 | JSON 문법, 최상위 객체, `model`과 불리언 필드 | 공통 요청 형식 오류 | 최소 호출 예제로 시작한 뒤 필드를 의도적으로 추가 |
| `415 unsupported_media_type` | `Content-Type` | 요청이 JSON으로 선언되지 않음 | `application/json` 전송 |
| `413 request_too_large` | 인코딩한 요청 바이트와 `max_request_bytes` | 전체 문맥이 바이트 한도를 초과 | 전달 가능한 문맥을 줄이거나 적절한 한도를 명시; 토큰과 바이트 한도는 다름 |
| `400 unsupported_feature` | `store`, `background`, `previous_response_id`, 대화·저장 항목 참조 | 상태를 저장하지 않는 게이트웨이에 저장·복구를 요구 | `store:false`로 현재 문맥 전체를 보내고 이력은 호스트에서 유지 |
| `400 unsupported_request` | API·프로필, 출력 한도, 클라이언트가 추가한 필드와 완전한 도구 이력 | 변환·지원 기능·프로필 한도 검사 실패 | 지원표와 비교하여 의도하지 않은 옵션을 제거하거나 검증한 호환 경로 선택 |
| `501 unsupported_endpoint` | 응답 조회·삭제 또는 압축 URL | 저장 응답 작업이 구현되지 않음 | 호스트의 이력·연속성 흐름 사용 |

프로필에 플래그를 추가해도 미지원 변환이 활성화되지 않는다. 예를 들어 추론
요약, `text.verbosity`, 미완료 도구 이력이나 미지원 스키마는 실제 공급자가 비슷한
이름의 기능을 제공하더라도 전송 전에 실패할 수 있다. 애플리케이션이 요구하는
옵션을 조용히 삭제하지 않는다. [요청 옵션](usage.md#request-options-and-response-values)과
[경로 설정](route-design.md#configuration)을 참조한다.

<a id="requests-return-429"></a>

## 요청에 429가 반환됨

| 증상 | 확인할 값 | 원인 | 조치 |
|---|---|---|---|
| `429 capacity_exceeded` | 활성 요청, `max_in_flight`와 열린 스트림 | 인스턴스 공통 용량을 사용 중이며 새 요청은 전송되지 않음 | 소비자의 동시 작업 수를 제한하고 완료·취소한 응답을 닫음 |
| `429 upstream_error` | 선택한 공급자·키와 공급자 한도 | 업스트림이 호출을 거부 | 실패를 확인한 뒤 공급자에 맞는 명시적 정책을 적용; 게이트웨이는 키를 바꾸지 않음 |

한 게이트웨이의 모든 별칭과 공급자는 같은 용량을 공유하며 대기열이 없다.
JSON 요청은 게이트웨이가 업스트림 본문을 모아 처리한 뒤 슬롯을 반환한다.
스트리밍 응답은 소비를 끝내거나 닫을 때까지 슬롯을 유지하며, 소비자가 느리거나
읽기를 멈춘 경우도 포함한다. 별칭을 바꿔도 별도의 용량이 생기지 않는다.
한도 증가는 애플리케이션 동시 실행 수와 공급자 한도를 함께 고려한 뒤 결정한다.

공급자의 `Retry-After`와 호출 한도 헤더는 전달하지 않으며 게이트웨이가 대신
재시도하지 않는다. 소비자 측 동시 실행·재시도 예산을 두고, 네트워크 호출 실패를
공급자가 아무 작업도 수행하지 않았다는 증거로 해석하지 않는다.

<a id="a-request-stalls-times-out-or-ends-early"></a>

## 요청 지연·시간 초과·조기 종료

[시간과 크기 한도 표](protocol.md#resources-and-failures)로 적용 구간을 확인한다.
요청 본문 제한은 로컬 입력 수신, 연결·응답 헤더 제한은 업스트림 전송에 적용한다.
본문 유휴 제한은 헤더 이후 JSON과 SSE 모두에 적용하며 전체 요청 시간 한도가 아니다.

| 증상 | 확인할 값 | 원인 | 조치 |
|---|---|---|---|
| `408 request_timeout` | 소비자 업로드와 요청 본문 기한 | 본문을 제때 수신하지 못함 | 설정한 시간 안에 완전한 JSON 전송 |
| 본문 전달 전 `504 upstream_timeout` | 연결·헤더 대기 시간과 JSON 본문 데이터 사이의 간격 | 업스트림 대기 한도 초과 | 오류 메시지와 로그로 구간을 확인하고 전송 가능성이 있으면 결과 불명 상태 보존 |
| SSE가 시작된 뒤 끊김 | 종료 이벤트, 읽기 오류와 `upstream_body_closed` 결과 | 유휴 한도, 원본 단절, 잘못된 변환이나 잘린 스트림 | 부분 출력을 보존하고 유효한 종료 결과를 받지 못했다면 중단으로 처리 |
| heartbeat는 오지만 유효한 출력이 오지 않음 | 애플리케이션의 전체 기한 | 수신 바이트가 본문 유휴 타이머 만료를 막음 | 호출자 기한을 적용하고 취소 시 응답을 닫음 |
| 출력 상태가 `incomplete` | `incomplete_details`와 요청·프로필 출력 토큰 | 모델이 선언된 출력 경계에 도달 | 부분 결과를 보존하고 이어갈지 다음 요청을 조정할지 결정 |

HTTP 200이나 정상 연결 종료만으로 모델 완료가 확인되지 않는다. 네이티브 SSE는
의미상 완료를 검사하지 않고 전달한다. 변환 스트림은 헤더 이후 대체 JSON 오류 없이
닫힐 수 있다. 기본 클라이언트는 바이트를 출력하므로 애플리케이션에는
[완료 판정 규칙](usage.md#read-a-stream-and-handle-cancellation)을 적용한다.
취소나 시간 초과가 공급자의 작업 또는 과금 취소를 보장하지 않는다.

<a id="the-upstream-returns-502-or-an-unusable-response"></a>

## 업스트림의 502 또는 사용할 수 없는 응답

| 증상 | 확인할 값 | 원인 | 조치 |
|---|---|---|---|
| `502 upstream_unavailable` | 설정한 주소와 게이트웨이 환경의 DNS·TLS·연결 | 업스트림 전송 실패 | 주소와 지원하는 HTTPS 연결을 확인; 환경의 HTTP 프록시는 사용하지 않음 |
| 리디렉션 뒤 `502 upstream_error` | 공급자 기본 URL | 게이트웨이가 따르지 않는 리디렉션을 업스트림이 반환 | 검증한 최종 API 접두사를 설정하고 API 엔드포인트를 두 번 붙이지 않음 |
| `502 upstream_content_type` 또는 `upstream_content_encoding` | 선택한 API와 공급자 응답 형식 | 예상과 다른 미디어 타입이나 압축 | JSON·SSE 구분과 identity 인코딩을 확인하고 HTML·로그인 페이지를 모델 응답으로 취급하지 않음 |
| `502 upstream_response_too_large` | 비스트리밍 JSON 바이트와 응답 한도 | 버퍼에 모으는 본문이 바이트 예산을 초과 | 예상 출력을 줄이거나 적절한 한도를 명시 |
| `502 upstream_invalid_json`, `upstream_invalid_response`, `upstream_read_error` | 네이티브 JSON 유효성, 변환 응답 계약과 전송 완료 | 잘못되었거나 미지원이거나 중단된 공급자 출력 | 실패를 보존하고 합성 입력으로 공급자·API 조합을 검증 |

같은 바이트 설정이라도 스트림 적용 범위가 다르다. 네이티브 SSE에는 이 설정의
전체 응답 바이트 상한이 없고, 변환 스트림은 이벤트별 크기와 누적 출력을 제한한다.
값을 바꾸기 전에 HTTP 계약을 확인한다. 지원 기능 프로필이 있다는 것만으로
공급자가 요구한 응답 형식을 보낸다는 사실이 검증되지는 않는다.

<a id="trace-a-failure-without-exposing-content"></a>

## 내용을 노출하지 않고 오류 추적

로컬 진단에는 의도적으로 등록하지 않은 별칭을 사용할 수 있다. 공급자 호출 없이
로컬 오류를 생성한다. `GATEWAY_BASE_URL`은 호출 가이드의 준비 기본 주소다.

```sh
curl --noproxy '*' -i "${GATEWAY_BASE_URL}/responses" \
  -H "Authorization: Bearer ${ARG_LOCAL_TOKEN}" \
  -H 'Content-Type: application/json' \
  -d '{"model":"not-registered","input":"Synthetic diagnostic request.","store":false}'
```

HTTP 상태, `error.code`와 응답의 `x-request-id`를 기록한다. 이 ID를 게이트웨이
로그의 `request_id`와 연결한다. 게이트웨이가 생성한 값이며 소비자가 보낸 요청 ID는
교체한다. 오류 객체의 `type`은 `gateway_error`다. 활성 업스트림 본문에는
`upstream_body_closed` 로그로 경로, 경과 시간과 결과도 기록한다. 초기 로컬
거절에서는 이 본문 로그가 없을 수 있다.

공급자의 오류 본문과 공급자 요청 ID 헤더를 포함한 임의의 업스트림 헤더는 전달하지 않는다.
[오류 레퍼런스](protocol.md#gateway-error-responses)로 전송 가능성을 확인한다.
프롬프트, 응답 본문, 키와 헤더를 진단 로그나 공개 보고에 넣지 않는다.
상태, 로컬 요청 ID, 경로 별칭, 구간과 시간 정보로 조사를 시작할 수 있다.
