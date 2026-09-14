<a id="writing-native-plugins"></a>

# 네이티브 플러그인 작성

[English](../plugin-authoring.md) | [한국어](plugin-authoring.md)

이 명세는 현재 설치 가능한 네이티브 역할을 설명합니다. 아래 프로세스·바이트 계약을 구현할 수 있는 언어라면 독립 저장소에서 플러그인을 제작할 수 있습니다. 호환 실행 파일 설치에는 게이트웨이 재빌드가 필요하지 않습니다. Rust ABI나 게이트웨이 라이브러리 의존성이 필요하지 않습니다. 이 문서는 새로운 런타임 역할, 공급자 API, 권한 또는 호환성 협상을 추가하지 않습니다.

이 명세와 함께 [설치 안내](extensions.md), [신뢰·수명 계약](extensions-design.md), [codec 동작](api-codecs.md), [사용량 의미](usage-accounting.md)를 읽으십시오. 네이티브 플러그인은 호스트 사용자 권한으로 실행하는 신뢰된 프로그램입니다. 제한된 IPC 인터페이스와 비워진 상속 환경은 OS 샌드박스가 아닙니다.

<a id="choose-a-supported-role"></a>

## 지원 역할 선택

패키지 정체성, 패키지 버전, 프로토콜 버전은 별개입니다. `protocol`은 기존 역할 하나를 선택하며, `permissions`와 `state_schema`는 권한 순서까지 아래 행과 일치해야 합니다. 패키지 버전은 각 여섯 자리 이하인 십진수 세 부분이며, 숫자 영을 제외한 선행 영은 허용하지 않습니다. 버전 번호는 호스트 버전 범위를 선언하지 않습니다.

```json
[
  {
    "protocol": "gateway-observer/v1",
    "permissions": [
      "observe_http_metadata",
      "write_private_state"
    ],
    "state_schema": "observer-state/v1"
  },
  {
    "protocol": "gateway-usage-recorder/v1",
    "permissions": [
      "export_usage",
      "observe_usage",
      "write_usage_store"
    ],
    "state_schema": "usage-store/v1"
  },
  {
    "protocol": "gateway-api-codec/v1",
    "permissions": [
      "read_model_payload",
      "transform_model_protocol"
    ],
    "state_schema": "request-memory/v1"
  },
  {
    "protocol": "gateway-api-codec/v2",
    "permissions": [
      "read_model_payload",
      "transform_model_protocol"
    ],
    "state_schema": "request-memory/v1"
  }
]
```

Observer는 숫자 HTTP 메타데이터만 봅니다. Recorder는 별도로 설정한 전달 모드에 따라 사용량 이벤트를 봅니다. Codec은 이미 승인된 요청과 공급자 응답을 기존 Responses, Messages, Chat Completions, Gemini Interactions API에 맞게 변환합니다. Codec 선택은 API enum, 경로, 자격증명 출처 또는 전송 방식을 추가하지 않습니다. 단일 API만 구현한 codec은 현행 시작 선언을 충족할 수 없습니다.

[패키지 스키마](../../schemas/gateway-extension-package-v1.schema.json)는 현행 역할 조합을 나열합니다. 알 수 없는 역할이나 필드는 오류입니다. 기능 협상이나 선택적 권한 축소는 없습니다. 정확히 지원하는 호스트 릴리스와 시험한 OS·아키텍처를 산출물과 함께 게시하십시오. 이 정보는 릴리스 문서에 기록하며 추가 manifest 필드로 넣지 않습니다. 향후 비호환 wire 변경에는 별도로 지원되는 프로토콜 식별자가 필요합니다.

<a id="package-bytes-and-execution"></a>

## 패키지 바이트와 실행

`extension.json`, 실행 파일 `extension`, `LICENSE.txt`, 그 밖에 선언한 고지·런타임 파일을 포함한 평면 디렉터리로 배포하십시오. `files` 맵은 manifest 외 모든 파일과 소문자 SHA-256 값을 포함합니다. 미등록 파일과 하위 디렉터리는 거절합니다. 등록 파일 수는 2–8개입니다. 이름은 ASCII `[A-Za-z0-9][A-Za-z0-9_.-]{0,63}`에 맞고 `extension.json`과 달라야 합니다. manifest의 `id`는 `[a-z][a-z0-9-]{0,63}`에 맞아야 합니다.

Manifest 바이트는 BOM 없는 UTF-8이며, 객체 키를 오름차순 정렬하고 불필요한 공백 없이 십진수 정수를 사용하며 끝에 LF 하나를 붙입니다. 허용하는 패키지 문자열은 모두 ASCII이므로 Unicode 이스케이프 변형은 관여하지 않습니다. LF를 포함한 이 정확한 바이트의 SHA-256이 패키지 다이제스트입니다. 이는 패키지 canonical 형식이며 RFC 8785 준수를 주장하지 않습니다. JSON Schema만으로 canonical 바이트, 중복 키, 파일 목록, 파일시스템 정체성 또는 신뢰된 다이제스트를 검사할 수 없습니다.

Manifest와 활성화 JSON은 각각 65,536바이트, 실행 파일은 128 MiB, 그 밖의 각 파일은 256 KiB로 제한됩니다. 대상은 `linux-x64`, `linux-arm64`, `macos-x64`, `macos-arm64`입니다. 설치·실행에는 실제 호스트 대상이 일치해야 합니다. 컨테이너는 다른 호스트 OS의 시험을 대체하지 않습니다. 패키징 도구는 하드 링크인 빌드 산출물을 복사할 수 있지만, 배포·설치 패키지 파일은 링크 하나인 일반 파일이어야 합니다. 경로의 심볼릭 링크는 거절합니다. 설치 저장소는 실행 사용자 소유의 비공개 저장소입니다.

실행 파일은 절대 경로로 시작하며, 소켓 기반 stdin/stdout, 비워진 상속 환경, 비공개 작업 디렉터리, 버려지는 stderr를 사용합니다. Observer와 codec에는 인수가 없고 Recorder에는 `serve`가 전달됩니다. 셸이나 언어 런타임은 제공하지 않습니다. 네이티브·자체 포함 바이너리 또는 명시적으로 준비한 절대 경로 인터프리터를 사용하십시오. `/usr/bin/env`를 쓰는 스크립트는 상속된 `PATH`에 의존할 수 없습니다. 번들 런타임에도 평면 파일·크기 제한이 적용됩니다. 데몬화하거나 자손 프로세스를 생성하지 마십시오.

설치는 바이트를 검사·복사하기만 합니다. 명시적 활성화는 정확한 패키지 다이제스트와 승인 권한을 기록하며 코드를 실행하지 않습니다. 활성화는 다음 시작에 사용할 고정 선택이며, 역할 전체를 합쳐 최대 네 패키지를 선택합니다. Recorder 바인딩은 활성화 v2를 사용하고 다른 선택은 v1을 사용합니다. 호스트가 관리하는 활성화 파일을 직접 수정하지 마십시오. 변경에는 재시작이 필요하며 패키지 교체는 실행 정체성을 바꿉니다. 롤백은 보존한 바이트를 선택하는 작업이며 데이터 마이그레이션이나 옛 자격증명 복원이 아닙니다. [관리 수명 주기](managed-extensions.md)를 참고하십시오.

<a id="observer-protocol"></a>

## Observer 프로토콜

[Observer 스키마](../../schemas/gateway-observer-v1.schema.json)는 각 메시지를 정의합니다. UTF-8 JSON 객체 하나와 LF를 쓰고 flush하십시오. 자식이 ready를 먼저 보내고, 호스트가 관측을 하나씩 보내며, 자식은 정확한 순서 번호를 확인 응답합니다. 순서 번호는 프로세스별 하나부터 시작해 재사용 없이 하나씩 증가합니다. 정수 필드는 소수점이나 지수 없이 십진수 숫자 토큰으로 출력하십시오. JSON 정수는 정확한 부호 없는 64비트 값이어야 합니다. 부동소수점 JSON 숫자만 제공하는 구현에는 손실 없는 정수 처리가 필요합니다. 응답 필드 누락·추가·중복·타입 불일치는 유효하지 않습니다.

```json
{"type":"ready","protocol":"gateway-observer/v1"}
{"type":"http","sequence":1,"status":200,"headers_ms":8}
{"type":"ack","sequence":1}
```

Ready는 3초 이내에 도착해야 합니다. 관측 쓰기와 확인 응답은 부분 I/O를 포함하여 1초를 공유합니다. LF 포함 응답 프레임은 최대 4,096바이트입니다. 상태는 100–599이며 시간은 전체 모델 완료 시간이 아닌 헤더까지의 밀리초입니다. Observer별 큐 용량은 64이며, 가득 차거나 연결이 끊긴 큐는 관측을 버립니다. 시작 시 EOF, 잘못된 순서 번호, 잘못된 응답 또는 시간 초과는 실행을 거절하며, 준비 완료 후에는 해당 Observer를 중지합니다. 재시작·이벤트 재전송은 없습니다. 확인 응답은 수신을 뜻하며 영구 저장의 증거는 아닙니다.

<a id="recorder-protocol"></a>

## Recorder 프로토콜

[Recorder 스키마](../../schemas/gateway-usage-recorder-v1.schema.json)는 전체 [사용량 이벤트 스키마](../../schemas/gateway-usage-event-v1.schema.json)를 참조합니다. Recorder는 영속적인 `producer_id`를 포함한 ready를 보냅니다. 게이트웨이는 envelope 없이 사용량 이벤트 객체를 직접 보냅니다. Recorder는 로컬 커밋 후에만 동일한 `event_id`와 LF를 제외한 정확한 이벤트 JSON 바이트의 SHA-256으로 응답합니다. 모든 응답을 flush하십시오.

```json
{"type":"ready","protocol":"gateway-usage-recorder/v1","producer_id":"synthetic-producer"}
{"type":"committed","event_id":"synthetic-event","sha256":"0000000000000000000000000000000000000000000000000000000000000000"}
```

위 영 다이제스트는 형식 예시이며 실제 이벤트의 확인 응답이 아닙니다. 레이블은 문자·숫자·`-_/.:`로 구성한 1–200 ASCII 문자입니다. 이벤트는 LF 전 최대 65,536바이트, 응답은 LF 포함 최대 4,096바이트입니다. `ack_timeout_ms`는 1–60,000이며 시작과 각 전체 쓰기·ACK 교환을 제한합니다. `queue_capacity`는 2–4,096입니다. 둘 다 ready 필드가 아닌 호스트 바인딩 필드입니다.

호스트 이벤트는 정렬된 객체 키와 압축 UTF-8 JSON을 사용합니다. 정확한 바이트와 정수를 보존하십시오. 보기 좋게 다시 쓰거나 숫자를 반올림하여 재구성한 바이트를 해시하지 마십시오. 스키마, producer 정체성, 이벤트·revision 정체성, 시각 순서, [정규화 규칙](usage-accounting.md)을 검사하십시오. `attempt_started`의 revision은 영이며 나머지 종류는 양수입니다. 같은 정체성과 바이트의 반복은 멱등적이어야 하며, 충돌하는 바이트에 committed를 응답하면 안 됩니다. 누락된 사용량은 알 수 없는 값이지 영이 아닙니다. 원격 전달은 로컬 ACK와 별개입니다.

모드 `off`는 recorder를 시작하지 않습니다. `best_effort`는 전달 손실을 허용합니다. `durable_local`은 설정된 시작·종료 경계를 로컬 커밋 확인 뒤에 통과시키며 recorder 실패 때문에 모델 추론을 재시도하지 않습니다. Ready 실패는 선택된 시작을 거절합니다. 이후 종료·잘못된 ACK·시간 초과는 recorder를 사용 불가로 만듭니다. ACK로 클라이언트 수신, 원격 영속성 또는 공급자 과금의 정확히 한 번 처리를 주장하지 마십시오.

작업 디렉터리는 명시적으로 바인딩한 사용량 저장소이며, 호스트가 정확한 다이제스트로 선택한 비공개 `recorder.json`을 포함합니다. 활성화 전에 선택한 recorder로 저장소를 준비하십시오. 설치는 초기화 코드를 실행하지 않습니다. [참조 recorder 설정·저장 절차](usage-accounting.md)는 로컬 원장과 외부 전달 대상을 설명합니다. 독립 구현은 선언한 로컬 커밋 의미와 명시적인 저장소 준비·복구 안내를 제공해야 합니다. 비호환 형식으로 기존 참조 원장을 열거나 상태 스키마를 마이그레이션 권한으로 재해석하지 마십시오.

<a id="codec-messages-and-state-machine"></a>

## Codec 메시지와 상태 기계

[Codec v1 스키마](../../schemas/gateway-api-codec-v1.schema.json)와 [codec v2 스키마](../../schemas/gateway-api-codec-v2.schema.json)는 전체 envelope와 중첩 계약 필드를 설명합니다. 각 프레임은 부호 없는 4바이트 big-endian 길이와 정확히 그 길이의 UTF-8 JSON 바이트로 구성하며 LF framing을 사용하지 않습니다. 길이는 1–134,217,728바이트여야 합니다. 소켓이 나누어 전달하더라도 접두부와 payload를 정확히 읽으십시오. 각 시작과 요청·응답 교환은 3초의 공통 제한을 가집니다.

```json
{"protocol":"gateway-api-codec/v1","sequence":0,"value":{"result":"ready","apis":["responses","messages","chat_completions","gemini_interactions"],"replay_versions":[1]}}
```

Ready는 정확히 위 API 배열 순서와 replay 버전 배열을 선언해야 합니다. 모든 envelope에서 선택된 프로토콜을 사용하십시오. 요청은 순서 번호 하나부터 하나씩 증가하고, 각 응답은 해당 요청의 번호를 그대로 반환합니다. Codec framing에는 JSON 필드 순서·공백의 canonical 형식을 요구하지 않습니다. 중복 키와 알 수 없는 필드는 거절합니다. 부호 없는 정수는 정확한 64비트 값을 사용하며 호스트가 보낸 크기·색인 필드는 설정한 크기 제한에 맞아야 합니다. 문자열에는 암묵적인 식별자나 callback 주소가 없습니다.

```text
ready(sequence=0)
  -> prepare(sequence=1) -> prepared
  -> json(sequence=2) -> json [stateless] / managed [managed]
  OR
  -> stream(sequence=2) -> progress
  -> event(sequence=3...) -> progress
  -> finish(next sequence) -> finished [stateless] / managed [managed]
```

첫 operation은 `prepare`입니다. `request`는 승인된 Responses JSON 객체이며 `route`에는 `api`, `model`, `profile_id`, `profile_version`, `reasoning_contract`, `support`, `context_window`, `max_output_tokens`가 있습니다. 지원 항목 누락은 미지원을 뜻합니다. `managed`, `pending_tools`, `history`, `max_output_bytes`는 replay 모드, 대기 중인 제어 상태, 인증된 이력 구간, 출력 제한(1–67,108,864)을 지정합니다. `prepared.payload`로 공급자 JSON 객체를 반환하며, 모델은 승인된 모델과 일치해야 합니다. HTTP 목적지·메서드·자격증명·헤더를 선택할 수 없습니다.

`json` operation은 공급자 본문 문자열과 호스트 `response_id`를 전달하고 `stream`은 그 정체성으로 스트림을 시작합니다. 각 `event`에는 호스트가 파싱한 SSE `event` 이름과 `data` 문자열이 있습니다. `progress.events`는 Responses 이벤트 객체를 포함하며, `complete`는 전송·recorder 성공이 아닌 파서 완료를 보고합니다. 완료 보고 후에도 `finish`는 입력 끝을 검증합니다. Stateless 종료는 `finished`, managed 종료는 최종 managed 객체를 반환합니다. `rejected`, 잘못된 operation, 순서 위반, 이른 EOF, 시간 초과 또는 잘못된 출력은 대체 경로나 재시도 없이 해당 요청을 실패시킵니다.

<a id="codec-nested-types-and-invariants"></a>

## Codec 중첩 타입과 불변 조건

선택적 nullable 필드는 각 스키마에 따라 누락 또는 `null`을 허용합니다. 호스트 직렬화는 명시적으로 생략하는 확장 필드 외에는 보통 null을 표시합니다. Codec v1은 `null`을 포함하여 `editing` 필드 자체를 금지합니다. Codec v2는 이를 허용하며, null이 아닌 정책은 [편집 계약](editing-design.md)을 충족해야 합니다. 해당 `version`은 하나로 IPC 버전 둘과 별개입니다. Codec은 도구 이름으로 편집 정책을 추론하면 안 됩니다.

스키마는 지원 기능과 bridge 이름을 나열합니다. Bridge는 해당 기능과 일치해야 합니다. Instruction envelope는 명령 계층과 해당 API, custom-tool JSON은 custom tools, tool namespaces는 namespaced tools, code-mode text parts는 structured tool output, patch·registered grammar bridge는 custom grammar에 연결됩니다. Registered grammar는 Responses의 native custom tools를 요구합니다. Provider parallel permission은 명시적 DeepSeek reasoning 계약의 Chat Completions를 요구합니다. 미지원 조합은 승인 과정에서 실패하며 플러그인이 이를 넓힐 수 없습니다.

`reasoning_contract`는 공급자 탐색이 아닌 선택된 타입 선언입니다. DeepSeek·OpenRouter 계약은 Chat Completions를, Claude adaptive·manual 계약은 Messages를 요구합니다. 버전은 하나입니다. 기본 effort는 선언한 지원 집합에 포함되어야 하며, OpenRouter budget·effort 모드는 서로 배타적이고 수동 Claude budget은 최소 1,024입니다. Reasoning 계약은 native reasoning summary·items를 요구합니다. [Reasoning 안내](chat-completions.md)는 허용 effort·format 조합과 wire 동작을 명시합니다.

각 이력 구간에는 `start`, `end`, `native`가 있습니다. 이는 오름차순이며 겹치지 않는 반개구간 입력 항목 색인이고, 시작은 끝보다 작고 끝은 요청 입력 배열 범위 안에 있어야 합니다. 비어 있지 않은 이력은 managed 모드를 요구하며 stateless 모드는 대기 중인 도구를 금지합니다. Native replay 버전은 하나입니다. `gemini_steps`는 비어 있지 않은 `steps`, `chat_assistant`는 `dialect`(`deep_seek` 또는 `open_router`)와 객체 `assistant`·`controls`, `messages_content`는 비어 있지 않은 `blocks`와 객체 `controls`를 포함합니다. 이 공급자 고유 값은 공개 Responses 출력이 아닌 비공개 상태입니다. 선택한 API·dialect와 인증된 구간 정체성을 보존하십시오. 연속성을 승인·보호·저장하는 주체는 호스트뿐입니다. [연속성](continuity.md)을 참고하십시오.

Managed 결과에는 `response`, `native`, `outcome`, `accounting`이 있습니다. Managed outcome은 `completed` 또는 `awaiting_tools`이며 실제 도구 출력과 맞아야 합니다. Accounting에는 `usage`, nullable `model`, nullable `response_id`, `upstream`이 있으며 사용량은 공통 usage-event 스키마와 정규화 규칙을 따릅니다. 호스트는 실제 공급자 바이트로 사용량을 구하고 codec 주장과 비교합니다. 종료 상태, 도구 정체성·인수·문법, 이벤트 순서를 독립 검증합니다. Managed 진행 출력은 text·reasoning만 공개하며 최종 텍스트와 일치해야 합니다. Native 상태와 도구 실행 인수는 종료 검증 전 공개하지 않습니다.

<a id="validate-and-release-independently"></a>

## 독립 검증과 배포

Observer부터 시작하십시오. Ready와 정확한 확인 응답을 구현하고, 직접 제작한 실행 파일을 패키징하며, 신뢰된 다이제스트를 검사하고 명시적으로 활성화한 뒤 선택한 잠금으로 이미 빌드된 게이트웨이를 다시 시작합니다. 합성 loopback 트래픽과 실패 사례를 시험하십시오. 이후 전체 메시지·의미 계약에 맞춰 더 큰 역할을 구현하십시오. 기본 테스트에는 공급자 호출과 운영 자격증명을 넣지 마십시오.

[계약 벡터](../../schemas/plugin-vectors.json)는 스키마 유효·무효 객체, 정확한 canonical 패키지 바이트·다이제스트, Observer 프레임을 포함합니다. 이는 합성 예제이며 실행 가능한 패키지나 호스트 수용 증거가 아닙니다. `cases` 목록은 로컬 `schema`, `value`, 예상 구조 검사 결과 `valid`를 지정합니다. `canonical_package`에는 `utf8`, `sha256`, `files_utf8`가 있고, `canonical_usage_event`는 LF를 제외한 이벤트 다이제스트와 일치하는 `ack`를 포함하며, `observer_frames` 목록에는 LF로 끝나는 바이트를 JSON 문자열로 담습니다. 스키마 유효성은 메시지 순서, 바이트 정체성, 네이티브 동작의 안전성 또는 공급자 정확성을 입증하지 않습니다.

배포 전에 산출물 다이제스트, 프로토콜, 정확히 시험한 호스트 릴리스, 플랫폼, 검사 도구 버전, 통과·실패·미실행 시나리오, 런타임 전제조건과 출처·고지를 기록하십시오. 시작 실패, 잘못된 필드, 분할·크기 초과 프레임, 순서 불일치, EOF, 시간 제한, 취소를 시험하십시오. Recorder에는 중복·충돌 이벤트와 커밋 실패가, codec에는 JSON·SSE 일치, 도구 검증, replay, 사용량 불일치 시험이 추가로 필요합니다. 전송만 또는 Observer만 검사한 결과는 다른 역할을 인증하지 않습니다.

릴리스마다 프로토콜 변경 기록과 실행 가능한 예제를 유지하십시오. 패키지 해시 시험, 스키마 시험, 상태 기계 시험, 실제 호스트 통합을 구분하십시오. 새 스키마나 예제는 문서 해시 기록 전에 두 언어판과 함께 검토해야 합니다. 스키마 변경은 실행 중인 호스트를 조용히 바꾸지 않습니다. 이 계약은 마켓플레이스, 배포자 서명 검증, 자동 업데이트, hot reload 또는 악의적 코드 격리를 약속하지 않습니다.
