<a id="g05--model-routes-and-capability-admission"></a>
<a id="g05--모델-경로와-기능-admission"></a>
<a id="model-routing-and-capability-checks"></a>

# 모델 경로와 지원 기능 검사

[English](../route-design.md) | [한국어](route-design.md)

모델 별칭은 설정된 공급자, 실제 업스트림 모델과 API를 선택한다. 소비자가
Responses 엔드포인트에 별칭으로 요청하면, 게이트웨이가 공급자 키를 선택하고
해당 경로의 지원 기능을 검사한 뒤 공급자를 호출한다.

<a id="consumer-model-names"></a>

## 소비자용 모델 이름

소비자는 요청의 `model` 필드에 등록된 별칭을 보낸다. 별칭의 `provider` 설정은
`providers`의 항목을 선택하고, `upstream_model`은 공급자에 전달할 실제 모델 ID다.
공급자 항목 하나에는 `base_url` 하나와 `api_key_env` 참조 하나가 연결된다.
공급자 항목은 접속 주소와 인증정보를 묶은 설정이므로, 여러 항목이 같은 공급자
서비스를 가리킬 수 있다.

예를 들어 `A-Provider`가 `Model-L`, `Model-M`, `Model-S`를 제공한다면
소비자에게 다음 두 방식 중 어느 쪽으로도 이름을 제공할 수 있다.

| 실제 모델 | 공급자 이름을 포함한 별칭 | 크기에 따른 별칭 |
|---|---|---|
| `Model-L` | `A-Provider/Model-L` | `Large-Model` |
| `Model-M` | `A-Provider/Model-M` | `Medium-Model` |
| `Model-S` | `A-Provider/Model-S` | `Small-Model` |

다음은 두 이름 체계를 함께 등록하는 완전한 설정이다. 제공할 별칭만 남겨도 된다.
공급자, URL과 모델 이름은 설명용 예시이므로 검증한 공급자 주소와 모델 ID로
바꾼다. 이 예시들은 기본값인 Responses API와 업스트림 Bearer 인증을 사용한다.

```toml
listen = "127.0.0.1:0"
local_token_env = "ARG_LOCAL_TOKEN"

[providers.A-Provider]
base_url = "https://api.a-provider.example/v1"
api_key_env = "A_PROVIDER_API_KEY"

[models."A-Provider/Model-L"]
provider = "A-Provider"
upstream_model = "Model-L"

[models."A-Provider/Model-M"]
provider = "A-Provider"
upstream_model = "Model-M"

[models."A-Provider/Model-S"]
provider = "A-Provider"
upstream_model = "Model-S"

[models.Large-Model]
provider = "A-Provider"
upstream_model = "Model-L"

[models.Medium-Model]
provider = "A-Provider"
upstream_model = "Model-M"

[models.Small-Model]
provider = "A-Provider"
upstream_model = "Model-S"
```

게이트웨이 프로세스의 환경변수에 `A_PROVIDER_API_KEY`와 별도의 `ARG_LOCAL_TOKEN`을
설정한다. 설정을 `config.local.toml`로 저장한 뒤 [시작 절차](../../README.ko.md#getting-started)를
따른다. 키의 실제 값은 TOML 파일에 넣지 않는다.

별칭은 대소문자를 구분하여 정확히 일치하는 이름을 찾는다. 게이트웨이가
`A-Provider/Model-L`을 슬래시로 나누거나 접두사에서 공급자를 추론하지 않는다.
`Large-Model`이라는 이름이 사용 가능한 가장 큰 모델을 자동 선택하지도 않는다.
각 이름은 명시한 매핑을 따른다. 여러 별칭을 같은 공급자와 모델에 연결할 수 있으며,
등록되지 않은 별칭은 업스트림 호출 없이 `404 model_not_found`를 반환한다.

<a id="call-a-model"></a>

### 모델 호출

게이트웨이의 로컬 토큰과 준비 완료 주소를 사용한다. 아래 예시 포트는 실행 중인
게이트웨이가 알린 포트로 바꾼다.

```sh
curl --noproxy '*' http://127.0.0.1:43127/v1/responses \
  -H "Authorization: Bearer ${ARG_LOCAL_TOKEN}" \
  -H 'Content-Type: application/json' \
  -d '{"model":"Large-Model","input":"Reply with hello.","store":false,"stream":false}'
```

이 설정에서 게이트웨이는 `A_PROVIDER_API_KEY`가 참조하는 키로 공급자의
`/v1/responses`에 `model: Model-L`을 보낸다. 요청 모델을 `A-Provider/Model-L`로
바꿔도 같은 대상과 키를 선택한다. `Medium-Model`과 `Small-Model`은 나머지
등록 모델을 선택한다. 로컬 토큰은 선택한 업스트림 키로 교체하며, 소비자가 보낸
공급자 인증 헤더는 전달하지 않는다.

소비자가 사용할 수 있는 이름은 다음 요청으로 확인한다.

```sh
curl --noproxy '*' http://127.0.0.1:43127/v1/models \
  -H "Authorization: Bearer ${ARG_LOCAL_TOKEN}"
```

예시에서는 별칭 여섯 개를 반환하며 공급자 URL이나 키 참조는 포함하지 않는다.
공급자의 모델 목록을 조회하는 API는 아니다. 네이티브 Responses 경로에서는
응답 본문의 `model`을 별칭으로 바꾸지 않으므로, `Large-Model` 요청에 `Model-L`이
반환될 수 있다. 소비자가 화면 표시나 요청 기록에 별칭을 사용하려면 요청한 이름을
따로 보관한다.

<a id="multiple-api-keys-for-one-provider"></a>

## 같은 공급자의 여러 API Key

같은 공급자의 같은 모델을 서로 다른 키로 호출하려면 키마다 공급자 항목을
등록하고 별도의 모델 별칭을 연결한다. 각 항목에는 `api_key_env` 하나만 있으며,
모델이 이 참조를 덮어쓰거나 키 풀에서 하나를 선택할 수는 없다.

다음은 동일한 주소와 `Model-L`을 두 키로 호출하는 별개의 완전한 설정이다.

```toml
listen = "127.0.0.1:0"
local_token_env = "ARG_LOCAL_TOKEN"

[providers.A-Provider-key-a]
base_url = "https://api.a-provider.example/v1"
api_key_env = "A_PROVIDER_KEY_A"

[providers.A-Provider-key-b]
base_url = "https://api.a-provider.example/v1"
api_key_env = "A_PROVIDER_KEY_B"

[models.Large-Model-key-a]
provider = "A-Provider-key-a"
upstream_model = "Model-L"

[models.Large-Model-key-b]
provider = "A-Provider-key-b"
upstream_model = "Model-L"
```

이 설정을 시작하기 전에 로컬 토큰과 함께 `A_PROVIDER_KEY_A`와 `A_PROVIDER_KEY_B`를
모두 설정한다. 소비자는 별칭으로 사용할 키를 선택한다.

| 요청 모델 | 공급자 항목 | 실제 모델 | 키 참조 |
|---|---|---|---|
| `Large-Model-key-a` | `A-Provider-key-a` | `Model-L` | `A_PROVIDER_KEY_A` |
| `Large-Model-key-b` | `A-Provider-key-b` | `Model-L` | `A_PROVIDER_KEY_B` |

예를 들어 다음 요청은 두 번째 키를 선택한다.

```sh
curl --noproxy '*' http://127.0.0.1:43127/v1/responses \
  -H "Authorization: Bearer ${ARG_LOCAL_TOKEN}" \
  -H 'Content-Type: application/json' \
  -d '{"model":"Large-Model-key-b","input":"Reply with hello.","store":false,"stream":false}'
```

키 값은 게이트웨이를 시작할 때 읽는다. 환경변수를 바꿔도 실행 중인 인스턴스가
다시 읽지는 않는다. 요청에서 `provider`나 `credential_id`를 별도로 보내 선택하는
기능은 없으며, 등록된 `model` 별칭으로 경로를 선택한다. `auth`는 사용할 키가 아닌
업스트림 인증 헤더 형식을 지정한다. 인증 실패나 호출 한도 초과 시 다른 등록 키로
재시도하지 않는다.

로컬 Bearer 토큰으로 해당 인스턴스의 모든 등록 별칭에 접근할 수 있다. 별칭으로
키를 구분하는 것만으로 사용자·테넌트별 권한을 제한하지는 않는다.
[접근 범위](embedded-design.md#authentication-and-access)와
[통합 계약](integration.md#calling-from-a-backend-service)을 따른다.

Messages와 Chat Completions 경로도 같은 방식으로 별칭과 키를 선택한다.
이 경로에는 아래에서 설명하는 API, 인증 방식과 지원 기능 설정을 추가로 명시해야
한다. 같은 서비스의 키를 공급자 항목 여러 개로 나눈 경우에도 각 프로필은
선택한 공급자 항목과 실제 모델에 일치해야 한다.

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
