# HTTP 지원 계약

이 계약은 native Responses 전달과 선언된 Messages 변환의 범위를 설명한다. 모델 출력의 의미,
Codex 도구 실행, 압축·재개나 소비자의 승인 절차를 인증하는 계약은 아니다.
별도의 [IR v1](ir.md)은 내부 라이브러리 계약이며 이 HTTP 지원 범위를
자동으로 확장하지 않는다. stateless admission 규칙은 두 경로가 공유한다.

## 설정과 인증

프로세스는 `config.example.toml` 구조의 TOML을 읽는다. 알 수 없는 설정 키,
등록되지 않은 공급자 참조, 잘못된 제한값은 시작 전에 거부한다. 설정을
자동 재로딩하지 않으며 변경 적용에는 재시작이 필요하다.

리스너는 숫자 루프백 주소만 허용한다. 기본값은 `127.0.0.1:0`이다.
공급자 URL은 HTTPS를 사용하고, 모의 서버를 위한 숫자 루프백 호스트에만
HTTP를 허용한다. URL의 사용자정보·query·fragment는 허용하지 않는다.
주소와 모델 경로는 운영자 설정에서만 선택하며 요청 본문으로 새로운
공급자를 지정할 수 없다. ambient HTTP proxy와 redirect를 사용하지 않는다.

로컬 토큰은 32~4096자의 공백 없는 ASCII이고 공급자 키와 달라야 한다.
소비자 `Authorization`을 upstream에 전달하지 않고 해당 공급자의 키로
선언한 `auth`에 따라 Bearer 또는 `x-api-key` 헤더를 구성한다. 기본값은 Bearer다.
소비자 쿠키·임의 헤더도 전달하지 않는다.
브라우저 애플리케이션용 CORS나 공개 서비스 인증은 제공하지 않는다.

## 엔드포인트

| 요청 | 인증 | 의미 |
|---|---|---|
| `GET /` | 없음 | 이름·버전·라이선스와 선택적 `source_url` |
| `GET /healthz` | 없음 | 프로세스가 응답함 |
| `GET /readyz` | 없음 | 시작에 필요한 로컬 구성이 준비됨; `provider_probe:false` |
| `GET /v1/models` | 로컬 Bearer | 설정에 등록된 모델명 목록 |
| `POST /v1/responses` | 로컬 Bearer | 등록 경로로 한 번 전달 |

readiness는 공급자 키의 실제 유효성, 외부 네트워크 상태, 모델 가용성 또는
qualification을 확인하지 않는다. 루트의 소스 URL도 존재 여부를 온라인으로
검증한 결과가 아니다.

## 요청·응답

JSON 객체와 등록된 문자열 `model`을 받는다. native Responses 경로는 모델명을
공급자 모델명으로 치환하며 나머지 필드를 아래 제한 외에는 JSON 값으로 보존한다.
Messages 경로는 등록된 기능 부분집합을 변환하며 미지원 필드는 전송 전에 거부한다.
아래 표의 원형 전달 규칙은 native 경로에 적용한다.
`stream:true`이면 SSE, 생략하거나 false이면 JSON 응답을 사용한다.

| 입력 | 처리 |
|---|---|
| `store` 생략 또는 false | upstream에 `store:false` 전달 |
| `store:true` | 미지원 오류 |
| null이 아닌 `previous_response_id`, `conversation`, `context_management` | 상태·압축 기능 미지원 오류 |
| `input`의 `item_reference`(생략/null type의 ID 참조 포함), `compaction`, `compaction_trigger` | 저장 항목 참조·압축 기능 미지원 오류 |
| `background:true` | 미지원 오류 |
| 함수·custom tool·구조화 출력·reasoning 필드 | JSON 값 보존; 공급자 기능 보장은 별도 검증 필요 |

native 경로는 response ID를 저장·변환하지 않고 정상 응답의 model 필드를
다시 쓰지 않는다. Messages는 공급자 ID를 기반으로 Responses 응답·항목 ID를
만들며 원래 도구 호출 ID를 유지한다. 두 경로 모두 response ID 조회·저장은 없다. 공급자의 오류 본문은 prompt나 자격 증명을 되돌려줄 수 있으므로
전달하지 않고 로컬 오류와 HTTP 상태를 반환한다. redirect는 따라가지 않고
502로 처리한다. 정상 응답과 SSE 이벤트 본문은 전달되므로 공유 서비스나
tenant 격리 용도로 이 계약을 확장 해석하지 않는다.

native SSE 본문은 전체 수집이나 JSON 재해석 없이 전달한다. Messages는 SSE를
증분 해석하고 검증된 Responses 이벤트를 출력한다. 청크 경계는 이벤트·문자·
JSON 경계가 아닐 수 있다. 이미 전송을 시작한 스트림에서 실패하면 연결을
종료하고 다른 공급자 응답을 이어 붙이거나 성공 종료 이벤트를 만들어내지 않는다.
상위 소비자는 최종 완료 이벤트 없이 끝난 스트림을 성공으로 간주해서는 안 된다.
공급자에는 `Accept-Encoding: identity`를 요청하며 압축된 본문은 502로 거부한다.
JSON 숫자는 임의 정밀도로 파싱하여 큰 정수와 소수의 값을 보존한다.

## 자원과 실패

기본 제한은 요청 8 MiB, 비스트리밍 응답 16 MiB, 동시 요청 32개,
요청 본문 대기 30초, 연결 10초, 응답 헤더 60초, 스트림 유휴 대기 60초,
종료 유예 5초다. 정확한 설정값은 TOML의 `[limits]`에 둔다.
native 스트림은 전체 수집 없이 전달한다. Messages는 최종 Responses output을
구성하기 위해 텍스트와 도구 입력을 제한된 버퍼에 유지하며, max_response_bytes가
개별 SSE 이벤트와 누적 출력에도 적용된다. custom wrapper는 검증 후 복원한다.

소비자가 스트림을 끊으면 upstream 읽기를 종료한다. 서버의 정상 종료는
진행 요청에 제한된 유예 시간을 부여한다. 이미 공급자가 처리한 요청의
취소나 비용 회수를 보장하지 않는다.

게이트웨이는 자동 재시도와 fallback을 하지 않는다. 인증 실패, 입력 오류,
미등록 모델은 공급자 요청 전에 거부한다. 연결·timeout·본문 크기 초과와
공급자 HTTP 오류를 성공으로 바꾸지 않는다. 처리 여부가 불명확한 실패는
소비자가 재시도 정책과 실행 기록을 통해 다뤄야 한다.

로그는 요청·경로 식별자, 상태와 소요 시간 같은 운영 메타데이터로 제한한다.
본문, prompt, 인증정보와 헤더를 로그에 넣지 않는다.

## 선언된 경로와 기능 프로필

모델에 `api`, `auth`, `capability_profile`, `messages_version`을 선언할 수 있다.
기존 설정의 API는 `responses`이며 Bearer 인증을 유지한다. Messages는 G09의
고정 Codex·합성 upstream과 HTTP 검증 후 명시적 프로필로 활성화할 수 있다.
Chat Completions는 G12 검증 전까지 서버 시작 시 거부한다.

프로필은 공급자·실제 모델·API를 모델 매핑과 일치시켜야 한다. 키가 프로필 ID이며
`version`, `tested_codex_version`, `context_window`, `max_output_tokens`을 명시한다.
이 값은 운영자의 선언이며 런타임·공급자 qualification을 자동 증명하지 않는다.
`support`는 기능 이름을 `native`, `unsupported` 또는 구현된 bridge 선언에
매핑하며 누락된 기능은 Unsupported다. namespaced_tools에는
`bridged_tool_namespace`, custom_grammar에는 `bridged_codex_patch_grammar`를
선언할 수 있고 두 도구 bridge에는 native 함수 지원이 필요하다. custom_tools에는 JSON bridge를,
Messages의 instruction_hierarchy에는 승인된 bridged_instruction_envelope를
명시할 수 있다. 설정 예시는 `config.example.toml`의 선택적 프로필을 참조한다.

프로필이 있는 경로는 요청한 `max_output_tokens`가 양의 정수인지와 선언 한도
이하인지를 전송 전에 검사한다. 입력 토큰 계수는 구현·검증되지 않았으며,
`context_window`는 호스트 설정용 계약이다. 프로필 없는 native Responses는
기존의 미검증 passthrough를 유지한다. 프로필이 있는 native 경로도 원형 JSON과
SSE를 보존하며, 기능별 semantic admission은 변환 경로에서 적용한다.

각 HTTP 요청은 해석한 공급자·모델·API·인증 참조·프로필·한도를 한 번 고정한다.
여기서 환경 변수 이름은 요청 시 자격 증명을 선택하는 참조일 뿐, 이력 재개를
증명하는 자격 증명 세대가 아니다. 영속 재개는 별도 연속성 계약을 따른다.
변환 admission은 명시한 client_metadata·prompt_cache_key·선택적 encrypted
reasoning 출력 요청만 전송 힌트로 제외한다. 알 수 없는 확장과 opaque 입력은
거부한다. Messages의 namespace·grammar bridge와 실제 Codex 합성 시험 범위는
[Messages 지원표](messages.md)에 기록한다. 실제 공급자 모델 검증은 별도다.

## 오프라인 내장 계약

`manifest --config`는 서버와 같은 설정 검증 및 경로 해석으로 정규화된 JSON과
설정 digest를 만든다. listener·환경 변수의 키 값·공급자에 접근하지 않는다.
`serve`의 첫 준비 JSON에는 기존 필드와 함께 `schema`, `manifest_schema`,
`configuration_sha256`가 포함된다. 새 HTTP 관리 엔드포인트는 없다.
상세 schema와 호스트 책임은 [내장 계약](embedded-design.md)에 명시한다.

Chat Completions는 요청·일반 응답·스트림 codec을 제공하며, 서버 설정은
실제 Codex G12 검증 전까지 거부한다. [초기 Chat 프로필](chat-completions.md)은
function wire와 명시적 custom JSON bridge의 지원 범위를 구분한다.
