<a id="direct-context-editing"></a>
<a id="helper-and-normalization-contracts"></a>
<a id="independent-operation-bundles"></a>
<a id="ownership-and-activation"></a>
<a id="streaming-replay-and-versions"></a>
<a id="validation"></a>
<a id="editing-compatibility-contract"></a>

# 편집 호환 계약

[English](../editing-design.md) | [한국어](editing-design.md)

이 계약은 구현된 직접 문맥 편집·명시적 helper 실행·선택형 정규화·독립 작업 묶음을
구분한다. 기존 custom 문자열 브리지가 기본값이다. 합성 fixture는
고정 Codex 0.154.0의 직접 패치와 Code Mode helper 실행을 검증하며 실제 공급자를
검증하지 않는다.

## 책임과 활성화

호스트가 검증된 클라이언트 계약을 선택한다. 모델은 `editing_policies`에서
`editing_policy`를 명시적으로 선택하며 정의만으로 활성화되지 않는다. CLI와
Rust 라우터 구성은 동일하게 검증된 설정을 사용한다. 게이트웨이는 표현 변환과
검증을 담당한다. Codex가 권한·승인·파일 접근·실제 적용을 소유한다. 새 편집
프로세스·파일 실행기·자동 재시도·공급자 이름 추론은 추가하지 않는다.

정책은 `version`, `client_contract`, `representation`, `patch_dialect`,
`normalization`을 지정한다. 정책이 없으면 기존 동작을 유지한다. 선언과 실제
요청은 일치해야 한다. 편집 도구가 없는 요청에 도구를 추가하지 않는다. 미지 계약은
전송 전에 거부한다. 이름을 지정한 강제 선택에는 합성 대안을 추가하지 않는다.
기존 이력을 위해 원래 도구도 유지한다.

## 직접 문맥 편집

`codex-direct-custom/v1`은 고정 custom 패치 계약이며 `context-lines/v1`은
한 파일·한 문맥 변경이다. 필수 필드는 `path`, `before_context`, `old_lines`,
`new_lines`, `after_context`다. 줄 배열은 줄바꿈 없는 문자열을 담는다. 미지·중복
키, 무변경·빈 편집, 구분자 주입과 개행만의 변경은 거부한다. 공백과 Unicode는
보존한다. 컴파일러는 결정적인 `codex-patch/1` 패치를 생성하고 등록 문법으로
검증한다. 실제 파일 문맥 일치는 호스트가 판단한다.

합성 함수 호출 하나는 원래 custom 호출 하나에 대응한다. 요청 registry는 충돌 없는
이름·namespace·선택·호출과 결과 연결을 소유한다. 정규 패치는 문맥 입력으로
왕복하며 표현할 수 없는 기존 패치는 원래 도구 경로로 유지한다. 관리형 replay는
새 매핑 메타데이터를 포함하여 공급자 원본 호출과 공개 복원 호출을 인증해야 한다.

## Helper와 정규화 계약

`codex-code-mode/v1`은 고정 custom exec 선언과 호스트가 선택한 helper 계약을
요구한다. 패치를 JSON 문자열로 직렬화해 고정 helper wrapper 하나를 만든다.
JavaScript를 실행하거나 일반적으로 해석하지 않는다. 생성한 wrapper만 역변환하며
임의 프로그램·주석·내부 패치 문자열은 유지한다. Exec 출력은 전체 프로그램의
결과이며 helper 성공을 만들어내지 않는다. 미등록 최상위 호출은 보정하지 않는다.

기본값은 `normalization=none`이다. 명시적 envelope 규칙은 완성된 패치
하나의 바깥 시작·종료 줄에 붙은 추가 구분자만 제거할 수 있다. 본문·경로·환경
선택을 바꾸지 않고 정규화 후 문법을 다시 검사한다. 코드 fence·설명·미완성
패치·다른 도구는 복구하지 않는다.

## 독립 작업 묶음

`operations/v1` 표현은 순서 있는 생성·삭제·이동·문맥 수정이다. 묶음은
패치 하나와 실행 결과 하나가 된다. 반복 경로·이동 의존·확인 가능한 어휘적 경로
별칭은 거부한다. 호스트 근거 없이 inode 동일성·원자성·rollback·파일별 성공을
추론하지 않는다. 앞선 변경에 의존하는 순차 편집은 범위 밖이다.

## 스트리밍·재생·버전

편집 인자는 기존 한도 안에서 변환·검증 완료까지 보관한다. 선행 텍스트 진행과
항목 순서는 유지한다. continuation 확정과 durable recorder 최종 ACK 전에
실행 가능한 완료를 공개하지 않는다. EOF·취소·실패는 완료나 추론 재실행을 만들지 않는다.

정책 의미에는 `gateway-editing-policy/v1`, 선택형 실행에는
`gateway-embedded-manifest/v7`, `gateway-ready/v7`,
`gateway-extended-manifest/v7`, `gateway-extended-ready/v7`을 예약한다.
편집을 지원하는 codec·팩에는 `gateway-api-codec/v2`와 `gateway-profile-pack/v2`를
사용한다. `gateway-continuation/v3`은 향후 매핑 메타데이터를 위해 예약하며 현재
가역 편집은 기존 replay v2를 사용한다.
기존 설정과 v1/v2 replay는 원래 바이트 계약을 유지한다. DB 테이블 이행이나 암호문
재작성은 자동 수행하지 않는다. 새 경로 origin에는 선택한 정책·구현 버전을 모두
연결한다. rollback에는 호환되는 바이너리·정책·패키지 선택·DB·키·호스트 이력이 필요하다.

## 검증

소스 시험 `tests/codex/editing_contract.py`는 `--gateway-bin`과 `--runtime-dir`을
받는다. 런타임 바이트를 검증하고 loopback 합성 공급자로 직접·helper 호출의
적용과 승인 거부를 확인한다. 공개 fixture에는 계약 hash와 합성 메타데이터만
있으며 런타임 프롬프트는 복사하지 않는다. 구현 시 내장·외부 codec, 인라인·가져온
정책, stateless·managed 이력은 같은 공개 계약을 보존해야 한다.

<a id="using-direct-context-editing"></a>
<a id="직접-문맥-편집-사용"></a>

## 직접 문맥 편집 사용

다음 모델 설정 일부에는 별도로 정의한 공급자와 기능 프로필이 필요하다. 프로필은
native 함수 지원과 기존 custom·문법 브리지를 선언한다. 값은 합성이며 검증된
실제 공급자 설정이 아니다.

```toml
[models.writer]
provider="mock"
upstream_model="synthetic-model"
api="messages"
auth="api_key"
messages_version="2023-06-01"
capability_profile="verified-functions"
editing_policy="line-edit"

[editing_policies.line-edit]
version=1
client_contract="codex-direct-custom/v1"
representation="context-lines/v1"
patch_dialect="codex-patch/1"
normalization="none"
```

`old_lines`는 한 줄 이상이어야 한다. 이 버전은 기존 줄 블록을 교체하거나 제거하며
삽입만 하는 편집은 원래 패치 도구를 사용한다. 전체 16384줄과 컴파일된 패치
8 MiB로 제한한다. 공백 제거나 개행 보정은 하지 않는다. 독립 작업 묶음은 별도 표현을 선택한다.

합성 이름은 원래 도구 정체성과 정책으로 결정한다. 이름 충돌 시 과거 공급자
이름을 재배정하지 않고 거부한다. 정규 패치는 정확히 역변환하며 기존 비정규
패치는 원래 도구 경로로 유지한다. 공급자 원본과 복원된 공개 출력은 기존 replay v2에
표현되므로 이 표현은 새 replay 필드를 추가하지 않고 v3 지원을 주장하지 않는다.
편집 정책은 경로 origin에 결합하며 변경 시 새 세션이 필요하다.

CLI와 라이브러리 호출자는 같은 Config·라우터 경로를 구성한다. 정책 선택 시
예약된 manifest/readiness v7 계약을 사용한다. 순수 컴파일러 출력은 라우터의
허용 판정·출력 검증·호스트 승인을 우회할 권한이 아니다.

<a id="editing-pack-and-codec-integration"></a>
<a id="편집-팩과-codec-통합"></a>

## 편집 팩과 codec 통합

`gateway-profile-pack/v2`는 이름 있는 `editing_policies` export를 추가한다.
호스트는 `editing_policy_imports`로 export를 가져온 뒤 모델에서 그 별칭을
선택한다. 기존의 명시적 팩 설치·활성화 절차를 사용한다. v1 팩은 편집 필드를
거부하고 원래 바이트를 유지한다. 구형·신형 팩은 공존할 수 있지만 가져온 별칭이
인라인 정책을 덮어쓰지는 못한다.

```toml
[editing_policy_imports.line-edit]
pack="synthetic-fixture"
export="editing-0"
```

대응 설정 projection은 `gateway-profile-pack-configuration/v2`다. 팩 바이트·
export 정체성·정책은 경로 origin에 결합한다. 변경이 기존 세션 재사용을 허가하지
않는다. 근거는 여전히 게시자의 주장이며 제공하는 통합 fixture는 합성 전용이다.

참조 편집 codec은 `cargo build --locked --example api_codec_editing`으로 빌드한다.
기존 패키지 명령은 `--role api_codec --codec-protocol gateway-api-codec/v2`를
받는다. 기존 본문 읽기·변환 권한 두 개는 유지한다. 모델과 확장 잠금으로 패키지를
명시적으로 선택한다. 기존 참조 실행 파일의 codec v1도 유지하며 편집 정책은
받을 수 없다. 준비 요청의 null 편집 필드도 거부한다. Codec v2는 준비 계약에
선택한 정책을 담고 공통 순수 컴파일러를 사용한다. 코어 출력 검증·사용량 관찰·
영속 완료의 권위는 유지한다.

편집 결합 실행은 manifest/readiness v7을 유지한다. 미지 버전·불일치 패키지
프로토콜은 추론 전에 거부한다. 시험은 정확한 호출 복원·승인 거부·잘못된 편집·
원본 native 이력을 가진 재시작·낡은 팩 origin·기록기 시작 실패·최종 ACK 실패를
다룬다. 이 통합은 helper 프로그램 실행이나 새 DB를 추가하지 않는다.

컴파일되는 호스트 예제 [embedded_editing](../../examples/embedded_editing.rs)은
호스트 소유 런타임·listener·종료 신호와 일반 라우터를 사용한다. 라이브러리의
전역 로깅을 초기화하거나 프로세스 환경을 변경하지 않는다.

<a id="explicit-code-mode-editing"></a>
<a id="명시적-code-mode-편집"></a>

## 명시적 Code Mode 편집

호스트는 `codex-code-mode/v1`을 선택하고 검증된 런타임·도구 구성의 exec 설명
UTF-8 바이트 SHA-256을 `client_descriptor_sha256`으로 제공한다. 호스트가
검증한 자체 구성에서 얻어야 하며 요청이 주장하는 hash를 신뢰해서는 안 된다.
동적 helper에 따라 설명이 달라지므로 전역 hash 하나가 모든 도구 구성을 식별하지
않는다. 런타임 fixture는 프롬프트 복사 없이 builtin·합성 동적 도구 구성을 구분한다.

```toml
[editing_policies.helper-edit]
version=1
client_contract="codex-code-mode/v1"
representation="context-lines/v1"
patch_dialect="codex-patch/1"
normalization="none"
client_descriptor_sha256="<host-verified-64-lowercase-hex-digest>"
```

실제 요청은 일치하는 bare custom exec와 고정 source 문법을 포함해야 한다.
설명 변경·잘못된 타입·계약 불일치는 추론 전에 거부한다. 공통 컴파일러는
JSON 데이터로 `tools.apply_patch` helper를 한 번 호출하고 결과 표시문 하나를
생성한다. 이 정확한 wrapper만 패치로 역변환한다. 다른 프로그램은 원문을 유지한다.
Source 문법은 비어 있지 않은 Unicode를 허용하며 JavaScript 해석·실행은 호스트 책임이다.

관측된 exec 결과는 순서 있는 input-text 파트 배열이다. 선택한 정책은 공급자의
native 지원을 주장하지 않고 `StructuredToolOutput`에 유효 `CodeModeTextParts`
브리지를 추가한다. 파트는 `schema=codex-exec-text-parts/v1`과 `parts`를 가진 정규
JSON 텍스트 기록이 되며 원문·순서·경계를 유지한다. 텍스트 안의 상태를 helper
성공으로 해석하지 않는다. 이미지·미지 필드·비텍스트 파트·관련 없는 구조화 함수
결과는 거부한다. 직접 패치 계약은 기존 문자열 결과 규칙을 유지한다.

Descriptor·정책 변경은 경로 origin을 바꾼다. 기존 인증 replay v2는 새 필드 없이
공급자 원본 호출과 공개 wrapper 출력을 저장한다. 합성 시험은 실행·거부·잘못된
편집·틀린 descriptor·전체 프로그램 결과·동일 세션 재시작·기록기 실패 장벽을
다룬다. 임의 Code Mode 복구나 실제 공급자 적합성을 의미하지 않는다.

<a id="optional-envelope-normalization"></a>
<a id="선택형-envelope-정규화"></a>

## 선택형 envelope 정규화

직접 패치 정책에서 `normalization="patch-envelope/v1"`을 선택한다.
`representation="patch-text/v1"`은 원래 패치 도구만 유지하고 구조화 선택지를
추가하지 않는다. 같은 규칙을 문맥 편집과 함께 선택할 수 있으며 등록된 원래
패치 출력에만 적용한다. Code Mode에서는 이 정규화를 거부한다.

완성된 바깥 줄이 정확히 `*** Begin Patch ***`와 `*** End Patch ***`일 때만
후행 구분자를 제거한다. 선택적인 마지막 LF 하나는 보존한다. 본문·경로·Unicode·
공백·클라이언트 이력·다른 도구는 바꾸지 않는다. 정상 입력은 바이트 단위로
유지한다. 미완성 입력·설명·fence·다른 마커는 복구하지 않고 일반 문법 검사에서
잘못된 출력을 거부한다. 정규화 출력도 공개 전에 같은 등록 패치 문법을 통과해야 한다.

순수 함수는 텍스트와 적용 규칙을 함께 반환한다. 요청 registry는
`normalization_evidence`로 적용 규칙과 정규화한 서로 다른 호출 ID 수를 제공하며
반복 출력 검증을 중복 집계하지 않는다. 메타데이터에 패치 본문은 없으며 실제
파일 실행·성공을 입증하지 않는다. 전역 로거나 새 HTTP/replay 필드를 추가하지
않는다. Native·wrapped custom 인자는 최종 정규화 값 검증까지 버퍼링한다.

<a id="using-independent-operations"></a>
<a id="독립-작업-사용"></a>

## 독립 작업 사용

검증된 직접 또는 Code Mode 계약에서 `representation="operations/v1"`을
선택한다. 합성 함수는 순서 있는 `operations` 배열 하나를 요구한다.

```json
{"operations":[
  {"operation":"create","path":"new.txt","lines":["content"]},
  {"operation":"delete","path":"obsolete.txt"},
  {"operation":"move","source":"source.txt","destination":"destination.txt","context":["unchanged line"]},
  {"operation":"update","edit":{"path":"existing.txt","before_context":[],"old_lines":["old"],"new_lines":["new"],"after_context":[]}}
]}
```

각 작업의 모든 필드는 필수이며 미지·중복 키를 거부한다. 고정 Codex가 hunk 없는
이동을 거부하므로 이동에는 비어 있지 않은 불변 문맥이 필요하다. 게이트웨이는
문맥을 만들기 위해 원본 내용을 읽지 않는다. 생성은 내용 줄 하나 이상을 요구하며
빈 파일 생성과 삽입 전용 수정은 구조화 부분집합 밖이다. 원래 패치 도구는 유지한다.

묶음은 최대 64개 작업·전체 내용/문맥 16384줄·컴파일 패치 8 MiB로 제한한다.
컴파일러는 작업 순서·원문·경로를 유지한다. 충돌 검사에만 사용하는 이식 가능한
문자열 키는 구분자·점 구성요소를 합치고 이름을 소문자로 비교한다. 부모/자식 경로·
반복 원본/대상 경로·해소하지 못한 상위 경로 이동·후행 점/공백 구성요소는 보수적으로
거부한다. 파일시스템·symlink·현재 디렉터리·inode를 조회하지 않으므로 서로 다른
절대/상대 문자열이 같은 파일을 가리킬 수 있다.

정규 패치는 정확히 같은 순서의 묶음으로 역변환한다. 비정규 기존 패치는 원래
도구를 유지한다. 전체 패치 또는 helper 프로그램은 Codex의 실제 결과 하나를
반환한다. 게이트웨이는 파일별 상태를 나누거나 전체 원자성·rollback을 보장하지
않는다. 파일 문맥 충돌은 실행 결과이며 컴파일 거부·승인 거부와 구분한다.

합성 fixture는 네 작업·다음 턴 입력 동일성·승인 거부·문자열 경로 의존·파일 문맥
충돌·managed replay·기록기 재시작을 검증한다. 순수 컴파일러 시험은 호스트 권한이나
실제 공급자 적합성을 의미하지 않는다. 정책 선택은 DB migration 없이 경로 origin을 바꾼다.
