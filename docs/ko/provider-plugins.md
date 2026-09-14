<a id="provider-plugin-contract"></a>

# Provider 플러그인 계약

[English](../provider-plugins.md) | [한국어](provider-plugins.md)

`gateway-provider/v1` 계약은 공급자의 JSON·SSE 의미를 별도로 구현한 파서를 설명합니다.
패키지 선언을 검사·설치하고 독립 프로토콜 예제를 시험할 수 있습니다.
**운영 provider 경로 활성화는 아직 사용할 수 없습니다.** 보호된 연속성과 사용량 출처를
결합한 뒤에 provider 경로를 지원 기능으로 공개해야 합니다. Codec v3는 기존 내장 API
의미를 유지하며 이 역할을 구현하지 않습니다.

[Wire 스키마](../../schemas/gateway-provider-v1.schema.json)와
[이식 가능한 타입](../../crates/plugin-contract/src/provider.rs)은 동일한 언어 독립 메시지를
설명합니다. 스키마 유효성만으로 메시지 순서, 숫자 유효성, 호스트 호환성, 출력 유효성이나
실행 신뢰를 입증할 수 없습니다. 패키지 v2 선언은 [작성 명세](plugin-authoring.md)를 따릅니다.

<a id="process-and-message-lifecycle"></a>

## 프로세스와 메시지 수명 주기

호스트는 명시적으로 신뢰한 네이티브 실행 파일을 전용 작업 디렉터리, 비워진 상속 환경,
stdin/stdout IPC, 폐기하는 stderr로 시작합니다. 네이티브 실행은 OS 샌드박스가 아닙니다.
취소는 호스트가 소유하며 요청 종료 시 직접 자식 프로세스를 종료·회수합니다.
Wire에는 취소 ACK나 재시도가 없습니다. 프레임은 부호 없는 4바이트 big-endian 바이트
길이와 UTF-8 JSON으로 구성됩니다. 호스트의 절대 프레임 한도는 128 MiB이며 설정된
payload 한도도 적용합니다. 시작과 각 전체 교환에는 각각 3초 제한이 있습니다.
길이 prefix, 본문, 쓰기와 읽기는 같은 교환 deadline에 포함됩니다.

자발적인 `ready` 응답은 sequence 0을 사용하며 패키지의 `provider_protocol`과
`capabilities`를 정확히 반복합니다. 모든 envelope는 `gateway-provider/v1`을 사용합니다.
호스트 요청은 sequence 1부터 overflow 없이 증가하며 응답은 정확히 같은 번호를 돌려줍니다.
중복 키, 미지 필드, 잘못된 버전, 불일치, 예상하지 않은 결과 종류나 framing 오류는 세션을 실패시킵니다.

1. `prepare`는 승인된 Responses 요청, 경로 snapshot, 명시적 `continuation`,
   `max_request_bytes`/`max_output_bytes`를 전달합니다. `prepared`는 공급자 고유 키를
   사용하는 JSON 객체를 반환하며 루트 `model` 키는 필수가 아닙니다.
2. JSON 처리는 원본 공급자 본문 문자열과 호스트 발급 `response_id`를 담은 `json` 이후
   하나의 `completed` 결과로 끝납니다.
3. SSE 처리는 `stream` 이후 SSE event/data 문자열을 담은 `event` 호출을 사용합니다.
   각 결과는 `progress`이며 `complete: true`는 의미상 종료를 나타냅니다.
   호스트는 정확히 한 번 `finish`를 보내고 `completed`를 받습니다.
4. 조기 finish, 종료 후 event, 중복 완료, 의미상 종료 없는 EOF는 오류입니다.
   대체 경로나 암묵적 스트리밍 기능 축소는 없습니다.

`progress.events`는 지원되는 비종료 Responses 텍스트·추론 진행만 포함합니다.
최종 응답, 항목·도구 완료와 함수 인자는 최종 출력 검증 및 호스트 기록·연속성 장벽을
통과한 뒤에만 공개합니다. 최종 응답은 이미 보낸 진행 이벤트, 승인된 모델·도구 계약과
일치해야 합니다. `outcome`은 `completed`, `awaiting_tools`, `incomplete` 중 하나입니다.

경로에는 공급자 프로토콜 식별, 모델, 프로파일 식별·버전, 지원 매핑과 명시적 편집 선택
(`none` 또는 policy가 있는 `enabled`)을 포함합니다. endpoint, HTTP header, 자격증명,
전송 callback, 저장소 handle이나 암호화 키는 포함하지 않습니다. 고정 HTTP 목적지와
인증은 호스트가 선택합니다. 반환 payload 안에서 전송 설정처럼 보이는 이름은 일반
JSON 데이터로 남습니다. 고정 거절 코드는 `unsupported_request`, `invalid_upstream`,
`unsupported_state`, `resource_limit`이며 임의 오류·본문 텍스트를 반환하지 않습니다.

초기 호스트 경로는 패키지가 편집을 선언해도 편집과 compatibility-policy 선택을 거절합니다. 편집 wire 선택지는 별도로 지원할 호스트 통합을 위해 예약되어 있으며 이 예제는 편집 지원을 선언하지 않습니다.

<a id="usage-and-state-values"></a>

## 사용량과 상태 값

사용량은 명시적으로 `unobserved`이거나 7개 필수 counter가 있는 `observed`입니다.
Counter 이름은 `input_tokens`, `output_tokens`, `total_tokens`, `input_regular_tokens`,
`cache_read_input_tokens`, `cache_write_input_tokens`, `reasoning_output_tokens`입니다.
각 counter는 정확한 부호 없는 64비트 정수가 있는 `reported`, `not_reported`,
`not_applicable`, `invalid` 중 하나입니다. 0은 보고된 값이며 누락이 0을 뜻하지 않습니다.
소수, 음수, overflow를 보고된 정수로 바꿀 수 없습니다. 스키마 검증기는 `1.0`을 정수로
취급할 수 있으므로 wire 검증은 호스트가 요구하는 정확한 JSON 숫자 표현도 보존해야 합니다.

Counter 파생과 패키지·파서 출처 및 요청·시도·commit 식별 부착은 호스트만 담당합니다.
성공 출력을 내기 전에 부분집합·합계 연산과 누적 snapshot을 검증합니다. 미지의 공급자를
내장 사용량 파서로 다시 표시하지 않습니다. Cache TTL 세부 bucket은 이 wire 버전 범위 밖입니다.

연속성은 `stateless` 또는 `pending_tools`와 history span이 있는 `managed`입니다.
Span은 시작·끝 index와 불투명 상태를 담습니다. 상태 결과는 `none` 또는 `opaque`를
명시적으로 선택하며 이를 대신해 필드를 생략할 수 없습니다. 불투명 상태는 format label,
양수인 부호 없는 32비트 version, canonical padding을 적용한 표준 base64를 포함합니다.
호스트가 decoded 1 MiB 한도를 검사합니다. 스키마 정규식만으로 canonical pad bit나 decoded
크기를 증명하지 못합니다. 보호·영속화·정확한 경로·세션·패키지 결합은 호스트가 소유합니다.
이 계약은 세션 승인이나 패키지 저장 상태를 이행할 권한을 제공하지 않습니다.
현재 호스트는 관리형 provider 요청을 거절하며 보호된 영속화와 재개는 활성화되지 않았습니다. 선언과 독립 예제만으로 보호된 재시작 지원이 입증되지는 않습니다.

<a id="independent-example-and-verification"></a>

## 독립 예제와 검증

[합성 provider 예제](../../tools/plugin-conformance/examples/provider/README.md) 디렉터리
전체를 다른 곳으로 복사하십시오. Builder, 소스, 라이선스와 패키지 제작 도구에는
명시적으로 준비한 Python 3.11+ 인터프리터만 필요합니다. 설치 시 인터프리터나 의존성을
다운로드하지 않습니다. 실행 파일은 빌드 인터프리터의 절대 경로를 고정하므로 대상 머신에도
같은 경로에 해당 인터프리터가 있어야 합니다.

예제는 고유 `query`/`answer` 형식, SSE 텍스트 조각, 함수 호출 결과, 명시적 unknown/0
사용량, 버전이 있는 불투명 counter를 사용합니다. Wire 재시작 시험은 호스트 암호화
영속화와 별개입니다. 시험은 합성 입력만 사용하며 실제 공급자, 네이티브 샌드박스나 운영
경로의 자격 검증이 아닙니다. 독립 적합성 runner는 현재 provider 실행을 사용 불가로
보고하며 정적 패키지 검증 결과는 provider 실행 인증서가 아닙니다.
