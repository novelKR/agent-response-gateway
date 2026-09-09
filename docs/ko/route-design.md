<a id="g05--model-routes-and-capability-admission"></a>
<a id="g05--모델-경로와-기능-admission"></a>
<a id="model-routing-and-capability-checks"></a>

# 모델 경로와 지원 기능 검사

[English](../route-design.md) | [한국어](route-design.md)

모델 별칭은 설정된 공급자, 업스트림 API와 지원 기능 프로필을 선택한다.
게이트웨이는 요청이 끝날 때까지 이 선택을 고정하고, 변환할 수 없는 기능은
공급자에 요청하기 전에 거부한다.

<a id="evidence-and-recommendation"></a>
<a id="routing-rules"></a>
<a id="근거와-권고"></a>

## 경로 처리 규칙

Responses 경로는 [HTTP 계약](protocol.md)에 따라 원래 JSON 값과 SSE 스트림을
보존한다. Messages와 Chat Completions 경로는 [중간 표현](ir.md)으로 기능을
검사하고 프로토콜을 변환한다. 경로를 구분하므로 Responses는 변환 API에서
표현할 수 없는 필드도 원형 전달할 수 있다.

지원 기능 프로필은 직접 지원, 변환 규칙을 통한 지원, 미지원을 구분한다.
프로필에 선언하는 것만으로 구현되지 않은 변환을 활성화할 수는 없다.

<a id="configuration"></a>
<a id="public-configuration-additions"></a>
<a id="공개-설정-추가"></a>

## 설정

| 필드 | 의미 |
|---|---|
| `api` | 기본값 `responses`, 또는 `messages`, `chat_completions` |
| `auth` | `bearer` 또는 `api_key`; Responses 기본값은 bearer이며 변환 경로는 명시 필요 |
| `capability_profile` | `capability_profiles` 참조; 변환 경로에 필수 |
| `messages_version` | Messages에 필요한 업스트림 버전 헤더 |

프로필은 버전, 공급자, 모델, API, 지원 기능, 컨텍스트 크기, 출력 한도와
시험한 Codex 버전을 선언한다. 생략한 기능은 미지원이다. 프로필과 모델 매핑이
일치해야 하며, 알 수 없는 필드와 잘못된 참조는 설정 검사에서 거부한다.

운영자가 공급자 기본 URL과 `api_key_env`를 지정한다. 선택한 API에 따라
`/responses`, `/messages`, `/chat/completions`를 붙인다. `bearer` 인증은
Authorization, `api_key` 인증은 `x-api-key`를 사용한다. Messages에는 선언한
버전 헤더도 보낸다. 클라이언트가 새 업스트림 URL, 키나 임의 헤더를 지정할 수는 없다.

`/v1/models`는 설정된 별칭 목록이며 모델의 가용성이나 기능을 검사하지 않는다.
접속은 루프백으로 제한하고, 암묵적 프록시·리디렉션·재시도·대체 경로를 사용하지 않는다.

<a id="request-processing"></a>
<a id="runtime-flow-and-ir-additions"></a>
<a id="실행-흐름과-ir-추가"></a>

## 요청 처리

1. 로컬 인증, 요청 크기와 상태 저장을 허용하지 않는 요청 조건을 검사한다.
2. 별칭을 한 번 해석하여 공급자, 모델, API, 자격 증명 참조,
   어댑터·프로필 버전과 한도를 요청 단위로 고정한다.
3. Responses 요청은 원래 JSON/SSE 계약에 따라 전달한다.
   프로필이 없는 경로는 모델 기능을 검증하지 않은 원형 전달을 제공한다.
4. 변환 경로는 요청을 해석하고 필요한 기능을 구한 뒤 선언된 변환 규칙만
   적용한다. 지원하지 않는 기능은 전송 전에 거부한다.
5. 업스트림 요청 한 개를 생성하고 응답 이벤트를 검사한다.
   스트리밍 중 실패하면 완료를 만들어 내지 않고 연결을 닫는다.

도구 그룹의 네임스페이스, 구성원 이름, 설명과 순서를 보존한다. 변환된 이름은
정의·선택·호출·결과가 공유하는 하나의 복원 가능한 매핑을 사용한다.
실질적으로 같은 도구 식별자가 중복되거나 네임스페이스가 중첩되면 거부한다.

사용자 정의 텍스트 도구에는 명시적인 JSON 포장을 사용한다. 필수 패치 문법은
도구 이름이 아니라 정확한 해시와 버전으로 선택한다. 지원 문법과 한도는
[Messages](messages.md)에 정리되어 있다. 생성된 구문만 검사하며 파일 접근,
승인과 실행은 호스트가 담당한다. 알 수 없는 문법이나 잘못된 인자는 도구의
성공 완료를 알리기 전에 응답을 실패시킨다.

<a id="codex-transport-fields-and-unsupported-semantics"></a>
<a id="codex-전송-필드와-미지원-의미"></a>
<a id="protocol-fields"></a>

## 프로토콜 필드

| 입력 | 변환 경로의 처리 |
|---|---|
| model, stream, store | 경로·전송 선택; `store:false` |
| instructions, ordered input, tools, choices, limits | 타입에 따른 변환과 지원 기능 검사 |
| client_metadata, prompt_cache_key | 전송·캐시 힌트; 다른 API에서는 생략하며 지시로 해석하지 않음 |
| include reasoning.encrypted_content | 선택적인 불투명 출력 요청; 변환 경로는 불투명 출력을 제공하지 않음 |
| 불투명 추론 입력 | 다른 프로토콜로 재사용하는 요청은 거부 |
| 공급자 실행 웹·도구 검색 | 미지원 |
| 알 수 없는 확장이나 include 값 | 거부 |

필수 출력 형식, 이미지, 추론 옵션과 병렬 도구 제약을 삭제해 요청을 통과시키지
않는다. 선택한 경로의 지원 기능에 맞는 호스트 프로필을 사용해야 한다.

<a id="model-limits"></a>
<a id="model-limits-and-ownership"></a>
<a id="모델-한도와-책임"></a>

## 모델 한도

게이트웨이는 전송 전에 요청의 최대 출력 토큰을 설정된 모델 한도와 비교한다.
바이트 한도와 토큰 한도는 별개다.

`context_window`는 호스트의 컨텍스트·압축 설정에 사용한다. 게이트웨이는
입력 토큰을 세지 않는다. 호스트가 모델별 계수기나 추정기를 사용하고
출력과 압축에 필요한 컨텍스트를 확보해야 한다.

<a id="validation-and-recovery"></a>
<a id="validation-compatibility-and-rollback"></a>
<a id="검증호환성복구"></a>

## 검증과 복구

테스트는 Responses 원형 전달, JSON 값의 정밀도, SSE 바이트, 네임스페이스의
도구 식별과 변환 실패를 검사한다. 기능 누락, 잘못된 도구 관계, 출력 한도 초과와
호환되지 않는 불투명 상태는 업스트림 요청 전에 거부해야 한다.

[Codex 적합성 시험](conformance.md)은 선언된 프로필을 모의 공급자로 검사한다.
실제 모델의 동작과 호스트 운영은 별도로 시험해야 한다. 통합을 되돌릴 때는
검증된 실행 파일·설정 조합을 복원한다. 게이트웨이의 영속 상태를 이전할 필요는 없다.
