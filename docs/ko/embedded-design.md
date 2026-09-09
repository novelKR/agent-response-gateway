<a id="g13--host-owned-embedded-process-contract"></a>

# G13 — 호스트가 소유하는 내장 프로세스 계약

[English](../embedded-design.md) | [한국어](embedded-design.md)

상태: **2026-09-08 명시적으로 승인; 구현과 합성 계약 테스트 완료**.
이는 범용 gateway 계약이다. 소비자 활성화, 런타임 업그레이드 선택,
서비스 모드나 릴리스를 실행하지 않는다.

<a id="evidence-root-cause-and-recommendation"></a>

## 근거·원인·권고

G13 이전 gateway는 설정 파일을 읽고 숫자 루프백에 바인딩하며, signal handler
등록 후 readiness를 출력하고 제한된 종료를 지원했다. `check-config`와
라이브러리 router는 내장에 유용했지만, 해석된 경로·기본값·프로필·한도를
버전이 있는 형식으로 설명하는 CLI는 없었다. 원래 version/address 필드만으로는
호스트의 기대 설정과 child를 연결할 수 없었다. 아래 구현은 기존 필드를
유지하면서 이 binding을 추가한다.

**Strongly Recommended, 신뢰도 높음:** 작은 버전형 오프라인 manifest 명령과
readiness의 설정 digest를 추가한다. 프로세스 감독, Codex 설정, 자격 증명,
이력과 워크플로 결정은 호스트에 둔다. 기존 Config/ResolvedRoute 경계에서
해석을 한 번 수행해 호스트가 라우팅·기본값 규칙을 재구현하지 않게 한다.

보수적인 완전한 대안도 있다. 호스트가 원문 TOML과 실행 파일을 고정·해시하고
원래 readiness 필드를 사용하면 된다. 그러나 의미가 같은 재서식도 거부하고
표준 경로 해석 결과가 없다. 선택한 M5 계약은 재사용 가능한 보고서를 제공한다.
Gateway 프로세스 관리자나 소비자 전용 오케스트레이션 계층은 필요하지 않다.

<a id="proposed-cli-and-manifest"></a>

## CLI와 manifest 제안

`agent-response-gateway manifest --config <path>`:

- serve/check-config와 같은 코드로 설정을 파싱·검증한다.
- stdout에 JSON 객체 하나를 출력하고 종료한다. Listener, 자격 증명 읽기,
  다운로드, 공급자 probe나 모델 호출은 수행하지 않는다.
- `gateway-embedded-manifest/v1`, 패키지 이름·버전, Responses 클라이언트 API와
  `host-supervised-process/v1` 수명 계약을 보고한다.
- Listener 설정, 선택적 소스 URL, 로컬 토큰 환경 변수 참조, 한도와 별칭으로
  정렬한 경로 목록을 정규화한다. 기존 serve는 사용하지 않는 provider의 키도
  확인하므로 설정된 모든 공급자 자격 환경 변수 참조를 포함한다.
- 각 경로는 별칭·provider ID·해석한 endpoint·실제 model/API·auth·자격 참조,
  선택적 Messages 버전, 어댑터 버전, 기능 profile ID/version/support,
  context/output 한도와 시험한 Codex 버전을 포함한다. 선택값 누락은 null이며
  미검증 native 전달도 명시한다.
- 정규화한 설정만으로 `configuration_sha256`를 계산한다. 재귀적 키 정렬,
  UTF-8, 짧은 JSON 구분자, 끝 줄바꿈 없음이 규칙이다. 값은 정수·불리언·
  문자열·객체·배열·null이며 부동소수점은 없다. 별칭·경로 순서는 결정적이다.
- 비밀 값, 인증 헤더, 자격 증명, prompt, 이력, 비공개 호스트 경로,
  소비자 정체성과 운영 기록은 넣지 않는다. 환경 변수 이름은 참조이지
  자격 세대나 비밀 검증 결과가 아니다.

Manifest는 로컬 설정 데이터다. 호스트는 비공개로 보관할 수 있으나 실제
구조·식별자를 공개 시험·릴리스 증거로 올리지 않는다. 공개 예제·CI는 합성
설정을 사용한다. Digest는 일관성 확인이며 서명이나 모델 qualification이 아니다.
실행 파일 무결성은 별도의 릴리스·채택 계약으로 확인한다.

`check-config` 호환성을 유지한다. 다른 설정 형식을 추가하거나 조회 중 설정을
쓰지 않는다. Manifest 입력으로 endpoint를 바꾸거나 정상 설정 검증을 우회하지 않는다.

<a id="readiness-and-lifecycle"></a>

## 준비 통지와 수명

기존 ready 필드 `event`, `address`, `base_url`, `version`을 유지하고
`schema: gateway-ready/v1`, `manifest_schema: gateway-embedded-manifest/v1`,
`configuration_sha256`를 추가한다. Listener/router와 같은 Config에서 도출하므로
오프라인 검사와 serve 사이에 유효 설정이 바뀌면 감지한다. 원래 필드만 읽는
호스트도 계속 동작한다.

내장 호스트는 다음 순서를 따른다.

1. 고정 실행 파일과 대응 소스·고지·호환성 기록을 검증한다. 비공개 불변 설정
   snapshot을 준비하고 오프라인 manifest의 schema와 digest를 실행 설정에 연결한다.
2. 인스턴스 전용 로컬 토큰을 만든다. 권위 있는 자격 소유자에서 upstream 키를
   선택해 명시적 child 환경을 구성한다. Gateway에는 필요한 키만,
   Codex에는 로컬 토큰만 전달하고 upstream 키는 전달하지 않는다.
3. 정확한 gateway를 자식으로 기동해 제한된 stdout 준비 한 줄을 읽는다.
   Stderr는 별도로 제한·배출한다. 기동 제한 시간, schema, digest, version과
   숫자 루프백 주소를 확인한다. 무관한 프로세스에 연결하거나 고정 포트가
   비었다고 가정하지 않는다. Child 종료나 readiness 불일치 시 기동을 중단한다.
4. 준비 확인 후에만 Codex를 시작한다. 전용 비공개 HOME, 지원되는 명시적
   모델·catalog·provider, 준비 base URL, Responses wire API, 비활성
   fallback/retry와 로컬 토큰 참조를 사용한다. Turn 전에 실제 모델·공급자를 확인한다.
5. 정상 종료는 새 작업 중단, 필요한 turn 해결·interrupt, Codex 모델 연결 종료,
   gateway 종료 순서다. SIGTERM/SIGINT는 설정된 유예를 사용하며 호스트는
   외부 제한 시간 안에 자신이 소유한 child만 회수한다.
6. 기동·초기화 실패 시 시작한 모든 child와 임시 비밀 자료를 정리한다.
   예상치 못한 종료는 관련 실행을 실패 또는 Unknown으로 기록한다.
   조용한 재시작·재라우팅·모델 작업 재실행은 하지 않는다.

Readiness는 로컬 준비만 증명한다. 공급자 키 유효성, 모델 가용성, 의미 검증,
도구 실행 성공, 워크플로 승인이나 소비자 운영 수락이 아니다. HTTP readiness와
모델 목록의 의미는 유지한다. HTTP shutdown/admin/manifest endpoint는 추가하지
않으며 프로세스 감독은 부모에게 남는다.

<a id="authentication-and-access-scope"></a>

## 인증과 접근 범위

로컬 Bearer 토큰 하나가 해당 인스턴스의 모든 모델 별칭에 대한 기존 보호
endpoint를 허가한다. 파일·도구·승인 권한, 임의 URL·키·헤더 선택, tenant
정체성이나 외부 서비스 접근을 허가하지 않는다. 별칭을 줄여야 하면 호스트가
좁은 설정을 제공한다. 요청별 actor나 tenant 규칙은 추가하지 않는다.

숫자 루프백, 모의 루프백을 제외한 HTTPS, 명시적 Bearer/API-key 인증,
ambient proxy·redirect 없음, 요청당 한 번 시도를 유지한다. Gateway 경로
선택으로 호스트의 도구 sandbox, 사용자 승인 정책이나 업무 권위를 바꾸지
않는다. 재개에 필요한 credential realm/generation은 호스트가 소유한다.
Manifest나 환경 변수 이름의 일치는 세대를 대신하지 않는다.

<a id="compatibility-and-continuity"></a>

## 호환성과 연속성

Manifest는 선택적 조회 인터페이스이며 readiness 확장은 기존 필드에 추가된다.
새 호스트는 정확한 지원 schema를 요구한다. 이후 의미 변경에는 버전형 계약과
마이그레이션이 필요하다. 기존 독립 serve/check-config는 유효하며 데이터베이스나
gateway 영속 상태는 도입하지 않는다.

승인된 [연속성 설계](continuity-design.md)가 호스트 binding·이력의 기준이다.
호스트는 gateway/Codex digest, manifest 설정 digest, 경로·프로필, 권위 있는
자격 세대와 기존 thread·이력 checkpoint를 기록한다. G16은 승인된 범용 검증·
호스트 계약만 구현하고 소비자 기록은 소비자 저장소에 둔다. Digest가 바뀌면
명시적 전이 또는 새 시작 전까지 같은 맥락을 무조건 재사용할 수 없다.

이 계약은 소비자의 고정 Codex를 업그레이드하지 않는다. 임시 시험 기준과 소비자
자체 bundle·고지·상태 호환성은 구분한다. G14의 구체적 통합과 런타임·인증
모드 변경은 별도 승인을 받아야 한다. 모델·공급자 지출이나 생산 활성화를 승인하지 않는다.

<a id="alternatives-cost-and-risks"></a>

## 대안·비용·위험

| 대안 | 결과 |
|---|---|
| 권고 CLI manifest + ready digest | 작은 범용 변경, 하나의 정규 해석과 명시적 호스트 감독 |
| 기존 CLI + 호스트 원문 해시 | 즉시 변경은 최소이나 바이트에 민감하고 경로 해석 보고서가 없음 |
| Rust router를 같은 프로세스에 연결 | 일부 호스트에 적합하지만 배포·수명 결합이 바뀌며 자격·상태 계약은 여전히 필요 |
| Gateway가 Codex·워크플로를 감독 | 전송 책임 경계를 넘어 제외 |

구현은 manifest projection, 정규 digest, CLI/readiness 연결과 합성 계약 시험으로
구성한다. 기존 의존성을 사용하며 프로세스 관리 프레임워크가 필요하지 않다.
호스트 통합·운영 시험은 더 큰 별도 G14/G17 작업이다. Digest 불일치, 실제 설정의
공개 기록 유출, 준비 검증 전 Codex 기동, 자격 참조와 세대의 혼동이 주요 위험이다.
명시적 schema, 합성 자료만의 공개 검사와 호스트 수명·복구 규칙으로 관리한다.

<a id="validation-and-rollback"></a>

## 검증과 롤백

- TOML 순서·기본값 표기가 같으면 같은 유효 digest, endpoint/auth/model/profile/
  limits가 바뀌면 다른 digest여야 한다.
- Manifest는 자격·네트워크 없이 성공하고 비밀을 출력하지 않으며 serve와 같은
  경로 오류를 사용한다. 미지원 API는 명시적으로 실패한다.
- 실제 합성 child가 일치하는 version/schema/digest와 루프백 주소를 출력한다.
  기존 ready 필드가 유지되고 설정 변경이 readiness digest에 반영된다.
- Bind 실패, 시작 제한 시간, ready 직후 종료, 활성 응답 중 종료와 잘못되거나
  불일치하는 readiness를 시험한다.
- 내장 fixture에서 키 분리와 개인 설정 비상속을 확인한다. 기존 인증·취소·
  원형 Responses·Messages conformance는 계속 필수다.
- Rust·Python·라이선스·공개 소스·이력·archive·hosted CI를 실행한다.
  모의 호스트 검사는 소비자 또는 실제 공급자 수락이 아니다.

롤백은 gateway revert PR과 이전 검증 호스트·실행 파일·설정 조합을 사용한다.
기존 호스트는 원래 ready 필드를 읽는다. Manifest를 요구하는 새 호스트는
과거 바이너리와 연결될 때 검증을 건너뛰지 않고 실패한다. 상태 마이그레이션이나
파괴적 정리는 필요하지 않다.

<a id="implemented-interface"></a>

## 구현된 인터페이스

`Config::manifest()`는 불변 EmbeddedManifest projection을 반환한다.
configuration_sha256 getter는 listener/router와 같은 Config에서 readiness를
만든다. CLI는 Secrets를 읽지 않고 직렬화한다. 알려진 unsupported 항목은 생략
기본값으로 정규화한다. 명시한 기존 auth/API 기본값과 동일하게 해석되는 URL도
같은 digest를 만든다. 공통 SHA-256은 기존 grammar API를 유지하고 의존성을 추가하지 않는다.

Rust 테스트는 기본값·URL 정규화, profile/auth/key-reference/limit 변경, 정렬,
오프라인 조회, bind 실패와 활성 응답 중 제한 종료를 검증한다. 합성 호스트는
정확한 manifest/ready schema, Python의 독립 digest 계산, 숫자 루프백과 child
자격·HOME 분리를 확인한다. 실제 Codex 시험은 native/Messages 시나리오마다
이를 먼저 수행한다. 잘못되거나 다른 readiness와 준비되지 않은 child의 제한
시간은 별도 테스트다. 이는 범용 계약 검사이며 소비자 활성화·운영 수락은 G14/G17에 남는다.
