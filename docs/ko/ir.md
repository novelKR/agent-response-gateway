<a id="ir-v1--request-semantics-and-output-state-contract"></a>

# IR v1 — 요청 의미와 출력 상태 계약

[English](../ir.md) | [한국어](ir.md)

이 문서는 `ir::VERSION = 1`인 내부 Rust 라이브러리 계약을 정의한다.
현재 HTTP 서비스의 Responses 전달 경로는 그대로 유지한다. IR은 요청 codec,
기능 판정, 도구 bridge와 이벤트 상태 검증을 제공하고 Messages·Chat Completions
변환 어댑터가 이를 사용한다. IR 라이브러리 자체는 네트워크 요청을 실행하거나
새 HTTP 엔드포인트와 영속 저장 형식을 정의하지 않는다.

<a id="design-goals-and-boundaries"></a>

## 설계 목적과 경계

서로 다른 API의 공통 의미를 한곳에서 표현하되 메시지 문자열의 교집합으로
축소하지 않는다. 원래 순서·지시 역할·도구 정체성·불투명 상태의 사용 범위를
보존한다. 소비자 업무 타입, 사용자 승인, 도구 실행과 게시 상태는 포함하지 않는다.

| 설계 요소 | 구현 |
|---|---|
| CanonicalRequest | `request::RequestIR` |
| Instructions | top-level 지시와 순서 있는 메시지에서 도출하는 `instructions()` |
| OrderedItems | `Input`, `Item`, `Content`, `Part` |
| ToolDefinitions | `ToolDefinition`, `ToolIdentity`, `ToolKind`, `ToolInput` |
| GenerationOptions | 출력 한도·샘플링·도구 선택·구조화 출력·추론 옵션 |
| RequiredCapabilities | `capability::requirements()`가 검증한 요청에서 도출 |
| RouteSnapshot | 실제 경로와 전체 기능 프로필 선언을 고정한 값 |
| OpaqueProviderState | `continuity::OpaqueState`와 `ContinuityBinding` |
| 출력 상태 | `event::EventIR`, `EventValidator` |

기존 HTTP admission과 새 codec은 동일한 stateless 검사 함수를 사용한다.
HTTP 전달은 IR의 지원 부분집합이나 추가 구조 검사를 강제로 적용받지 않는다.
따라서 HTTP에서 원형 전달 가능한 요청이 IR codec에서는 거부될 수 있다.

<a id="request-representation-and-responses-codec"></a>

## 요청 표현과 Responses codec

`responses::decode(value, binding)`는 Responses 요청을 `RequestIR`로 해석한다.
`responses::encode(request, binding)`는 같은 프로토콜의 JSON 값으로 복원한다.
기본값 정리인 `store:false`를 제외하고 지원 부분집합의 JSON 값과 배열 순서를
왕복 보존한다. 공백·객체 키 순서 등의 직렬화 바이트 동일성은 약속하지 않는다.
함수 인자는 JSON 문자열 원문을 유지하며 큰 숫자의 정밀도도 보존한다.

```rust
use agent_response_gateway::ir::{IrError, responses};
use serde_json::json;

fn main() -> Result<(), IrError> {
    let request = responses::decode(
        json!({"model":"example/writer", "input":"Synthetic text"}),
        None,
    )?;
    let wire = responses::encode(&request, None)?;
    assert_eq!(wire["store"], false);
    Ok(())
}
```

지원하는 입력은 문자열 또는 순서 있는 항목 배열이다. 항목은 system·developer·
user·assistant 메시지, 함수/custom 도구 호출과 결과, reasoning 요약 및
불투명 상태다. 콘텐츠는 문자열 또는 text·`image_url` 이미지 참조 등을 담는
배열이다. 이미지 다운로드·업로드·파일 ID 해석은 수행하지 않는다.

top-level 지시는 `ProtocolDefault`, 메시지 지시는 원래 System/Developer 역할과
입력 항목 위치를 갖는다. 이 정보는 원본 항목에서 도출하는 읽기 전용 view다.
지시를 다른 역할의 문자열에 합치거나 도구 호출과 결과를 별도 배열로 재정렬하지 않는다.
대상 프로토콜이 이 계층을 보존할 수 있는지는 기능 판정과 후속 어댑터의 책임이다.

`ItemId`, `CallId`, `ResponseId`, `ToolIdentity`를 구분한다. 도구 정체성은
namespace와 이름으로 구성하며 함수와 custom 입력도 별도 종류로 보존한다.
입력 항목 ID와 호출 ID의 중복, 앞선 호출 없는 결과, 호출 종류 불일치와
중복 결과를 거부한다. 이는 독립 요청에 완전한 호출 문맥을 제공하는 v1 범위다.

도구 정의는 평면 function/custom 선언과 순서·설명을 가진 namespace 그룹을
지원한다. 그룹의 자식은 namespace/name 정체성을 사용하며 중첩 그룹과 중복
정체성을 거부한다. 호스팅 도구 선언은 아직 해석하지 않는다. JSON Schema는 값으로
보존하며 스키마 전체의 타당성이나 실제 모델의 준수 여부를 검증하지 않는다.

<a id="extension-fields"></a>

### 확장 필드

알 수 없는 필드는 출처 프로토콜이 있는 `Extensions`에 보존한다. 알려진 타입의
typed 필드를 확장 필드로 덮어쓰는 것은 거부한다. 선택 필드의 명시적 null도
동일 프로토콜 왕복에서 유지한다. null이 남아 있는 필드에 typed 값을 새로
넣으면 기존 표현을 명시적으로 정리해야 하며 encoder는 충돌을 거부한다.

알 수 없는 입력 항목·콘텐츠·출력 format·tool choice는 출처가 있는 확장으로
남길 수 있다. 같은 API로 보존한다는 사실은 해당 공급자의 기능 적합성을
검증했다는 뜻이 아니다. 현재 cross-protocol 변환 계획은 확장을 발견하면
명시적 변환 규칙이 없으므로 거부한다. 클라이언트 JSON에서 만든 source나
기능 지원 주장을 신뢰하는 인증 경계로 사용해서는 안 된다.

기존과 마찬가지로 store, background, response ID 이력·conversation·압축 요청은
stateless 정책으로 제한한다. 실제 저장·압축·재개 기능은 추가되지 않는다.

<a id="capability-admission-and-execution-routes"></a>

## 기능 판정과 실행 경로

`requirements(request)`는 검증한 요청에서 필요한 기능을 한 번 도출한다.
`plan_translation(request, target_binding)`는 대상 `CapabilityProfile`과 대조해
고정된 `RouteSnapshot`, 요구 기능, 적용할 bridge 종류를 반환한다.
필수 기능을 caller가 임의로 빼서 계획에 넣는 인터페이스는 제공하지 않는다.

기능에는 지시 계층, 이미지, 함수/custom 도구, strict 도구 인자, 문법,
namespace, 도구 선택·병렬 제어, 구조화·strict 출력, 출력 한도, temperature·
top_p, 추론 옵션·항목과 불투명 연속성이 포함된다. `parallel_tool_calls:false`
처럼 동작을 제한하는 명시적 옵션도 지원 요구로 취급한다.

판정은 `Native`, `Bridged`, `Unsupported`다. 선언이 없으면 Unsupported다.
유효한 bridge는 `CustomToolJson`, `ToolNamespace`, `CodexPatchGrammar`와
Messages 전용 `MessagesInstructionEnvelope`다. 각 bridge는 대응 기능에만
선언할 수 있다. 도구 bridge에는 함수 도구의 Native 지원이 필요하고 문법
bridge에는 custom JSON bridge도 필요하다. 미지 bridge나 strict 출력을
프롬프트로 대체하는 묵시적 완화는 허용하지 않는다.

경로에는 공급자 ID, 실제 모델, API 종류, 자격 증명 바인딩 참조, 어댑터 버전,
기능 프로필의 ID·버전·전체 선언과 모델 한도를 담는다. 선언된 출력 한도는
검사하지만 입력 토큰 계산이나 컨텍스트 적합성 측정은 수행하지 않는다.
모델 별칭을 실제 경로로 해석하는 라우터, live qualification, 네트워크 요청과
재시도는 이 순수 계획 함수의 범위 밖이다.

## Custom tool JSON bridge

`CustomToolBridge::new()`는 요청의 도구 선언에서 결정적인 이름 매핑을 만든다.
기존 이름과 충돌하지 않는 함수 이름을 선택하고 입력을 다음 스키마로 감싼다.

```json
{"type":"object","properties":{"input":{"type":"string"}},"required":["input"],"additionalProperties":false}
```

`lower_call`과 `restore_call`은 원래 이름·namespace, 항목 ID·호출 ID와 문자열을
복원한다. `lower_choice`는 명시적으로 선택한 custom·namespace 함수도 같은 매핑으로 바꾼다.
결과 변환은 원래 호출을 함께 받아 ID와 종류의 연결을 검사한다. 이름 충돌,
알 수 없는 wrapper 이름, 중복·추가 필드나 잘못된 입력, 잘못된 호출 매핑을 거부한다.

단일 요청 registry가 namespace 그룹을 평면 이름으로 변환하며 그룹·자식 설명을
유지한다. 이름 복원은 실제 모델의 namespace 의미 준수를 증명하지 않는다.
custom format은 생략, 정확한 text format, 등록된 Codex patch 문법만 허용한다.
문법은 SHA-256와 버전으로 선택하여 이력과 출력의 구문을 검사하며, 미지 문법과
확장은 거부한다. 도구 실행이나 파일 적용 가능성은 검사하지 않는다. Messages
스트림은 부분 wrapper를 모은 뒤 검사하고 원래 자유 형식 입력을 전달한다.

<a id="opaque-state-and-continuity"></a>

## 불투명 상태와 연속성

`OpaqueState`는 바이트, 형식과 origin binding을 함께 가진다. binding은 전체
경로 snapshot과 caller가 관리하는 principal/auth scope 참조로 구성한다.
실제 API 키·세션 토큰은 넣지 않는다. `replay()`는 대상 바인딩과 형식이 정확히
일치할 때만 바이트를 제공한다. 모델·공급자·API·인증 범위·어댑터·기능 프로필
변경은 불일치로 거부한다. ID·버전만 같고 프로필 내용이 바뀐 경우도 구분한다.

Responses의 `encrypted_content`를 decode하려면 origin binding을 명시해야 한다.
이를 encode할 때도 같은 바인딩과 `responses.encrypted_content/v1` 형식이 필요하다.
일반 메시지나 reasoning 요약에 불투명 바이트를 합치지 않는다.

이 컨테이너는 암호화·서명·소유권 인증을 새로 수행하지 않는다. 기본 Debug와
Serialize도 제공하지 않는다. 현재 source-bound 자료를 같은 경로로 복원하는
내부 계약이며, 프록시 소유 암호화 envelope나 클라이언트에게 제공할 재개 토큰은
후속 설계다. 타입 검사 통과는 공급자가 상태를 실제 수락한다는 증거가 아니다.

<a id="event-ir-and-state-transitions"></a>

## Event IR과 상태 전이

`EventValidator`는 응답 하나의 이벤트를 순서대로 검증한다. 이벤트는 시작,
항목 시작, 콘텐츠 시작·증분·종료, 도구 인자 증분, 항목 종료, 사용량 갱신과
최종 상태로 구성한다. 입력 문자열은 이미 wire UTF-8 처리가 끝났다고 가정한다.
SSE 프레이밍, 공급자별 이벤트 파서와 클라이언트 이벤트 encoder는 각 어댑터의
책임이며 이 순수 검증기 안에 포함되지 않는다.

| 상태·입력 | 규칙 |
|---|---|
| 시작 전 | Started만 허용 |
| 항목 시작 | 항목 ID·출력 index·도구 호출 ID 중복 거부 |
| 콘텐츠 증분 | 열린 항목의 열린 콘텐츠에만 허용 |
| 도구 인자 증분 | 열린 도구 항목별로 누적; 부분 JSON 자체는 허용 |
| 항목 종료 | 열린 콘텐츠가 없어야 하며 함수 인자는 완성된 JSON이어야 함 |
| 정상 완료 | 열린 항목이 없어야 함 |
| 불완전·실패·취소·연결 유실 | 열린 항목이 남아 있어도 실패 상태로 종료 |
| 종료 후 | 추가 이벤트와 중복 종료 거부 |

최종 상태와 선택적 사유를 보존한다. HTTP EOF를 모델의 Completed로 바꾸는
규칙은 없다. 잘못된 이벤트는 이전 상태를 변경하지 않고 오류를 반환한다.
텍스트는 누적하지 않으며 기본 한도는 항목 4096개, 콘텐츠 16384개, 단일 증분
1 MiB, 전체 도구 인자 버퍼 8 MiB다. 사용량은 누적 수치로 받아 감소를 거부하고,
생략된 항목은 기존 값을 유지한다. 토큰 단위의 공급자 간 동등성이나 비용은 계산하지 않는다.

<a id="validation-and-further-scope"></a>

## 검증과 후속 범위

합성 요청의 왕복, 지시·도구 연결, 숫자·문자열 보존, 기능 누락·확장 거부,
binding 변경, wrapper 복원, 교차 이벤트와 모든 문자열 분할 위치를 시험한다.
기존 HTTP 전달 테스트와 함께 수행하며 private 기록이 없는 소스에서도 확인한다.

현재 Messages와 Chat Completions codec·SSE 변환·HTTP 연결은 G12의 실제
Codex 합성 시험을 통과했다. 호스트가 소유하는 이력·로컬 압축·복구는 별도의
[연속성 계약](continuity.md)을 따른다. Gateway의 상태 저장과 tenant 인증은
미지원이며 실제 공급자 모델 qualification과 소비자 생산 운영 수락은 별도 단계다.
라이브러리 계약을 통과한 선언을 운영 수락으로 간주하지 않는다.

참고: [OpenAI custom tools](https://developers.openai.com/api/docs/guides/function-calling#custom-tools),
[Anthropic streaming](https://platform.claude.com/docs/en/build-with-claude/streaming).

<a id="http-route-integration"></a>

## HTTP 경로 선언 연결

`Config::resolve_route`는 선언된 API·모델 프로필을 기존 RouteSnapshot으로
고정한다. `ResolvedRoute::admit`는 HTTP의 공통 stateless 검사 이후 호출한다.
Native Responses는 JSON과 SSE의 기존 passthrough를 유지하며 변환 경로는
검증된 RequestIR에서 기능 요구와 TranslationPlan을 도출한다. 선언 프로필은
실제 모델 qualification이나 자격 증명 세대의 증명이 아니다. Namespace와
grammar bridge를 Messages·Chat Completions HTTP 경로에 연결한다.

Messages adapter는 별도 순수 codec으로 제공한다. 승인된
`MessagesInstructionEnvelope` bridge는 선행 지시의 원문·역할·위치를 유지해
system 영역에 표시하되 native 역할 우선순위와 동일하다고 주장하지 않는다.
이 bridge는 Messages 프로필에서 instruction_hierarchy에만 선언할 수 있다.
현재 요청·일반 응답·스트림 지원 범위는 [Messages 지원표](messages.md)를 따른다.

도구 호출의 선택적 status는 ToolCallStatus로 보존한다. Messages 이력은 생략
또는 completed만 받으며 in_progress·incomplete를 완료된 호출로 바꾸지 않는다.
HTTP 변환은 route admission에서 만든 TranslationPlan을 재사용해 요구 기능을
한 번 도출하고, 어댑터의 실제 구현 범위를 추가로 확인한다.

Chat Completions의 요청·일반 응답·스트림은 별도 순수 codec으로 제공한다. 도구 정체성·
선택·호출 수·출력 복원 검증은 Messages와 공유하고, API별 role·finish 의미는
각 어댑터에 둔다. HTTP 지원 범위는 [Chat 지원표](chat-completions.md)를 따른다.

완료 메시지 status와 output_text의 annotations도 typed 필드로 보존한다.
변환 이력은 완료/생략 status와 비어 있는/생략 annotations만 허용한다.
의미 있는 주석, 미완료 메시지와 알 수 없는 확장은 명시적으로 거부한다.
메시지 도구 호출 뒤 같은 assistant 구간의 텍스트는 Messages 블록 순서를
유지하며, 도구 결과 일부가 나온 뒤 assistant 내용을 끼워 넣지 않는다.
