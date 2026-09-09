<a id="g05--model-routes-and-capability-admission"></a>

# G05 — 모델 경로와 기능 admission

[English](../route-design.md) | [한국어](route-design.md)

상태: **2026-09-08 명시적으로 승인; G06과 G09/G12 어댑터에서 구현**.
이 문서는 G06 구현과 G09/G12 어댑터 활성화 계약이다. 기존 루프백·stateless
전송 경계를 보존한다.

<a id="evidence-and-recommendation"></a>

## 근거와 권고

G05 설계 당시 HTTP 요청은 공급자·모델을 선택해 Bearer 인증으로 항상
`/responses`를 호출했다. 그 경로는 IR 기능 계획을 사용하지 않았다. 실제
고정 Codex 시험에서는 평면 function/custom 도구와 namespace container를
관측했다. 초기 요청에는 cache/metadata/include 필드와 hosted 도구 선언도
포함된다. 모든 원형 요청을 더 좁은 IR 부분집합으로 처리하면 native 전달이
깨지고, 변환 요청의 기능 검사를 우회하면 미지원 의미가 숨겨진다.

**Required, 신뢰도 높음:** 별도의 native 전달 경로를 유지하면서 하나의
변환 경계에서 선언 경로와 기능을 연결한다. 호출마다 공급자 특수 조건을 쌓는
방식은 유지보수 가능한 완전한 대안이 아니다. 기존 native 전용 서비스를
유지하는 것은 범위가 제한된 유효한 대안이었다.

<a id="public-configuration-additions"></a>

## 공개 설정 추가

기존 provider/model 필드를 유지하면서 모델에 다음 선택 필드를 추가한다.

| 필드 | 계약 |
|---|---|
| `api` | `responses`(기존 기본값), `messages`, `chat_completions` |
| `auth` | `bearer` 또는 `api_key`; 기존 Responses 기본값은 bearer, 변환 경로는 명시 필요 |
| `capability_profile` | `capability_profiles` 참조; 변환 경로에 필수 |
| `messages_version` | Messages 경로의 명시적 upstream 버전 헤더에 필수 |

각 프로필은 ID·version·API·기능 지원·context window·최대 출력·시험한 Codex
버전을 선언한다. 기능은 기존 Native/Bridged/Unsupported를 사용하고 누락은
Unsupported다. 모델의 provider·API·profile이 일치해야 하며, 미지 필드와
잘못된 참조는 시작 전에 거부한다. 구현되고 선언 범위의 검증을 마친 어댑터만
전송 설정에 사용할 수 있다. 미완성 어댑터는 라이브러리·테스트 코드에 둔다.

Provider base URL과 `api_key_env`는 운영자가 관리한다. API에 따라 정확히
`/responses`, `/messages`, `/chat/completions`를 붙인다. `api_key`는
`x-api-key`로 공급자 키를 전달하고 `bearer`는 Authorization을 사용한다.
Messages는 선언한 버전 헤더를 보낸다. 소비자는 새 URL, 원문 upstream 키나
임의 헤더를 지정할 수 없다. 기존 URL·로컬 인증·무proxy·무redirect·무retry
규칙을 유지한다.

`/v1/models`는 활성 별칭 목록이며 기능 인증서가 아니다. 공개 형태나 실제
공급자·모델을 조용히 바꾸지 않는다.

<a id="runtime-flow-and-ir-additions"></a>

## 실행 흐름과 IR 추가

1. 로컬 인증, 요청 크기와 공통 stateless admission을 검사한다.
2. 별칭을 한 번 해석해 해당 HTTP 요청의 공급자, 실제 모델, API, 자격 참조,
   어댑터·프로필 버전과 한도를 고정한다.
3. Native Responses는 기존 JSON/SSE 동작을 유지한다. 프로필 부재는 미검증
   전달이며 모든 기능 지원을 묵시적으로 증명하지 않는다.
4. 변환 경로는 검증된 요청을 decode하고 요구를 한 번 도출한다. 지원 bridge를
   계획하고 누락된 기능은 HTTP 전송 전에 거부한다.
5. 선택한 어댑터가 한 요청을 encode하고 공통 출력 이벤트 상태를 구동한다.
   스트리밍 도중 오류가 나면 완료를 만들거나 다른 공급자 출력을 붙이지 않고 닫는다.

IR 도구 정의 enum에 설명과 function/custom 자식을 가진 순서 있는 namespace
그룹을 추가한다. 유효 정체성은 namespace/name이다. Responses 왕복에서
그룹·자식 순서를 유지하고 중첩 그룹은 거부한다. 어댑터의 평면 이름은 정의·
선택·호출·결과가 공유하는 요청 단위 가역 매핑에 둔다. 기존 평면 도구 표현과
테스트는 유지한다. Rust 타입 추가이며 영속 형식이나 소비자 업무 타입은 추가하지 않는다.

Custom 텍스트 JSON 포장은 bridge다. 필수 문법은 도구 이름 추정이 아니라
선언한 문법의 정확한 hash·version으로 검증기를 선택한다. 초기 범위는 검증된
Codex patch 문법과 text format이다. 미지 문법은 Unsupported다. Patch 검증기는
구문만 읽으며 파일을 읽거나 변경하지 않는다. 생성 후 bridge 검증과 native
constrained decoding은 구분한다. 인자와 필수 문법 검사 전에는 도구 완료를
내보내지 않는다. 문법 실패는 응답 실패다.

<a id="codex-transport-fields-and-unsupported-semantics"></a>

## Codex 전송 필드와 미지원 의미

| 입력 | 변환 경로 정책 |
|---|---|
| model, stream, store | 경로·전송 선택; stateless `store:false` 유지 |
| instructions, ordered input, tools, choices, limits | 명시적 typed 매핑과 기능 검사 |
| client_metadata, prompt_cache_key | 선언된 전송·cache 힌트; 모델 지시로 해석하거나 다른 API의 미지 필드로 전달하지 않으며 생략을 프로필에 기록 |
| include reasoning.encrypted_content | 선택적 opaque 출력 요청이며 생성 권한이 아님; stateless 변환 프로필은 opaque 출력 없음으로 선언 |
| 기존 opaque reasoning 입력 | 승인된 연속성 어댑터가 생길 때까지 cross-protocol 재사용 거부 |
| hosted web/tool search | 구체적 의미 매핑 구현·선언 전까지 미지원; 호스트는 검증 전에 호환 Codex 프로필을 선택 |
| 미지 확장 또는 include 값 | 명시적 규칙이 생길 때까지 거부 |

Strict 출력, 이미지, reasoning 옵션과 병렬 도구 제약을 성공 응답을 얻기 위해
삭제하지 않는다. 고정 Codex가 대상에서 표현할 수 없는 필수 기능을 보내면
해당 경로는 미검증으로 남는다. 위 전송 힌트가 초기 예외의 전부다.

<a id="model-limits-and-ownership"></a>

## 모델 한도와 책임

전송 전에 요청 최대 출력을 선택 모델의 한도와 비교한다. 바이트와 토큰 한도를
구분한다. `context_window`는 호스트의 모델별 컨텍스트·압축 설정 계약이며,
정확한 입력 토큰 계수기가 아니다. 근거 없는 범용 tokenizer를 추가하지 않는다.

G06은 컨텍스트 설정과 출력 한도 검사를 구현하지만, 정확한 입력 admission은
선택 모델에 검증된 계수 방식이 생길 때까지 미검증으로 둔다. 생산 컨텍스트
수락에는 M5/M6에서 실제 호스트 설정과 계수·추정 근거도 필요하다. 합성 토큰
fixture는 실제 모델 증거가 아니다. 이 단계 구분은 설계 승인에 포함되며
전체 컨텍스트 목표를 완료로 표시하지 않는다.

<a id="validation-compatibility-and-rollback"></a>

## 검증·호환성·복구

- 기존 설정, native 확장 전달, 숫자 정밀도와 SSE 바이트 검사를 유지한다.
  설정된 미지원 API는 명시적으로 실패한다.
- 프로필·기능 누락, 잘못된 도구 관계, 출력 한도 초과와 cross-origin opaque
  입력은 모의 upstream 요청 0회로 거부한다.
- Namespace 왕복, 충돌 없는 bridge 이름, 문법 거부와 교차 도구 호출이 같은
  정체성 불변식을 따른다.
- G09/G12는 실제 고정 Codex 시험과 지원 프로필 기록 후 어댑터를 활성화한다.
  Hosted mock 성공은 실제 공급자 qualification이 아니다.
- 구현과 함께 protocol·IR·integration 문서를 갱신한다. 이 설계는 새 생산
  의존성을 승인하지 않으므로 필요하면 별도로 제안한다.

기대 효과는 공급자마다 조건을 중복하는 대신 하나의 검증 가능한 admission·
정규화 경계를 만드는 것이다. 라우팅·IR과 각 어댑터 wire 구현 비용은 중간 수준이다.
기본 Codex 도구의 미지원, bridge 문법 동작과 모델별 토큰 계수가 주요 위험이며
각각 명시적 qualification 조건으로 관리한다. 구현 PR을 되돌리고 이전 Responses
전용 설정을 복원하는 방식으로 롤백하며 데이터 마이그레이션은 없다.

승인은 위 추가 항목과 힌트·컨텍스트 정책을 포함한다. 상태 저장, 서비스 노출,
실제 공급자 호출, 소비자 활성화나 릴리스를 승인하지 않는다. G04가 별도 승인된
임시 0.154.0-alpha.6 기준을 검증했으며 안정판 교체는 별도 artifact·conformance
검증으로 진행한다.
