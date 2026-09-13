<a id="declared-host-responsibilities"></a>
<a id="external-endpoint-and-runtime-verification"></a>
<a id="host-owned-management-and-access-contracts"></a>
<a id="optional-host-model-identity"></a>
<a id="synthetic-host-example-and-validation"></a>

# 호스트 소유 관리·접근 계약

[English](../embedded-management.md) | [한국어](embedded-management.md)

선택형 패키지 `gateway-management-embedded`는 호스트가 검증한 정체성, 명시한 권한,
구현 callback을 기존 [관리 API](management-api.md)에 연결합니다. 호스트가 인증·실행
수명·업무 상태를 소유합니다. 소비자 업무 타입을 요구하지 않으며 자체적으로 프로세스·
listener·로컬 사용자 저장소·백그라운드 작업을 시작하지 않습니다. 기본 Gateway는 이
패키지에 의존하지 않습니다.

## 명시적 호스트 책임

`HostContract`는 `gateway-embedded-management/v1`, 등록 대상 하나, 명시적 작업
집합과 `HostOwned` 또는 `Delegated` 수명을 사용합니다. `HostOwned`에는 시작·종료·
재시작 작업을 선언할 수 없습니다. 해당 작업 위임은 명시적 선언과 구현 backend가 필요하며
권한만 부여한다고 프로세스 제어 구현이 생기지 않습니다.

`HostDispatcher`는 호스트 `Dispatcher`를 감싸고 미구현 작업을 거절하며 기능·작업
목록을 선언 범위로 제한합니다. 준비 전에 정확한 대상·작업·wire intent digest를 검사합니다.
직접 조회에도 actor grant를 확인합니다. 상태 조정에는 별도 선언 권한이 필요하며 조회
전용 증거 검사만 위임하고 자동 재실행하지 않습니다. 호스트 prepared operation이 자체
동기화·예상 상태·실제 적용/미적용/미확정 증거의 책임을 계속 가집니다.

`service` factory는 같은 계약의 대상, bind된 numeric loopback 주소, 기존 journal·reader,
identity verifier를 받습니다. 저장소 초기화·socket bind는 하지 않습니다. HTTP 없이
라이브러리 인터페이스를 사용할 수 있고, 제공 HTTP router를 사용하면 Host·Origin·세션
경계를 유지해야 합니다. PID·포트가 같다는 이유로 다른 프로세스의 소유권을 얻지 않습니다.

`IdentityVerifier`는 호스트의 신뢰된 시스템으로 credential을 인증한 뒤
`VerifiedIdentity`를 만들어야 합니다. 이 증거는 `gateway-host-identity/v1`을 사용하며
Deserialize를 구현하지 않습니다. HTTP 요청의 role·주체 필드는 증거가 아닙니다.
`HostAuthenticator`는 알 수 없는 증거 버전을 거절하고 grant를 호스트 계약·credential
용도로 줄이며, refresh가 요청한 정체성·권한 버전을 유지하도록 요구합니다. 상위 principal이
넓은 grant를 주장해도 미지원 작업은 제공하지 않습니다.

Credential 제공·정체성 namespace·폐기·업무 상태 판단은 호스트 책임입니다. 인증 출처를
합칠 때 주체·credential 정체성이 모호하지 않아야 합니다. 이 adapter는 로그인·비밀번호·
MFA 제품을 제공하거나 미검증 principal을 감쌌다는 이유로 신뢰 가능한 것으로 만들지 않습니다.

## 선택형 호스트 모델 정체성

패키지의 `team` feature는 기본적으로 꺼져 있습니다. 선택하면 `HostModelAuthority`가
로컬 Team credential DB를 열지 않고 `ModelIdentityVerifier`를
[팀 모델 진입점](team-model-access.md)에 연결합니다. 호스트가 검증된 Model 용도 정체성,
버전 증거(`gateway-host-model-identity/v1`), 정확한 route와 사용량 범위를 제공합니다.
Adapter는 route를 교집합으로 제한하고 관리 grant를 제거하며 전체 사용량을 호스트의
명시적 허용 범위로 제한합니다. 알 수 없는 버전·잘못된 용도·변경된 refresh 정체성·버전은
거절합니다.

`gateway-team-http::ModelAuthority`는 공통 대상·인증·refresh 인터페이스입니다.
기존 로컬 Team authenticator가 이를 구현하여 credential 흐름을 유지합니다. 모델 서비스도
용도·활성 권한·정체성·버전·일치 대상을 독립적으로 확인합니다. 요청 ledger는 Gateway 전송
전에 admission을 기록하고 동일한 사용량·세션 소유권 규칙을 적용합니다. 호스트가 인스턴스를
만들지 않으면 이 인터페이스·선택 feature는 listener·저장소를 추가하지 않습니다.

## 외부 endpoint·런타임 검증

`ExternalAccess`는 `gateway-external-access/v1`로 등록 대상, `/v1`로 끝나는 HTTPS
공개 base, 명시적 Models/Responses 경로, 구분된 외부 접근·Gateway credential 참조를
제공합니다. 선언한 경로만 구성합니다. 참조 이름이 달라도 값이 같을 수 있으므로 실제 보호된
credential 값도 검사해야 합니다. 검증은 credential 값·verifier를 저장하거나 반환하지 않습니다.
Wildcard route proxy, WebSocket·이미지·음성 지원, identity provider 구현·배포 작업은
이 계약에 포함하지 않습니다.

`ExpectedRuntime`은 **이미 신뢰된 예상 manifest**를 받아 구성·실행 digest와 지원 schema를
검사하고, 관측 readiness의 주소·base URL·패키지 버전·manifest/readiness schema·digest를
비교합니다. 현재 embedded manifest/readiness 1 및 3–7, extended 1–7을 지원합니다.
알 수 없는 버전, loopback이 아닌 내부 bind, 잘못된 digest·endpoint는 거절합니다.
Gateway listener 경계를 넓히지 않습니다.

관리형 child에서는 `confirm_managed`가 `gateway-managed-process/v1` wrapper와
호스트가 선택한 실행 ID도 검사합니다. 인증된 부모 연결과 제한된 종료 정책은 여전히
호스트 책임이며 readiness 정보만으로 종료 동작을 증명할 수 없습니다. 기존
[소유 실행 adapter](managed-runtime.md)가 Standalone 부모 연결 상실·명시적 종료를
구현합니다. Embedded 호스트는 제어를 위임하는 대신 자체 수명 구현을 유지할 수 있습니다.

자체 일관성이 있는 manifest·digest·계약·fixture 통과는 정체성 검증·산출물 출처·
attestation이 아닙니다. 호스트의 신뢰된 구성·산출물 절차로 예상 manifest와 credential
연결을 얻어야 합니다. 외부 HTTPS·접근 인프라는 내부 loopback 서비스와 별개입니다.
Fixture는 재사용 가능한 endpoint·경로·정체성·readiness·종료·credential 경계를 준비하며
실제 외부 배포를 완료한 증거가 아닙니다.

## 합성 호스트 예제·검증

Rust `synthetic_host` 예제는 실제 관리 HTTP와 조회 전용 합성 module을 제공합니다.
명시적으로 초기화한 임시 journal과 테스트 전용 고정 조회 credential을 사용하고 수명 변경을
노출하지 않으며 stdin이 닫히면 종료합니다. 제공 Standalone 제품·소비자 연동은 아닙니다.

```sh
cargo test -p gateway-management-embedded --all-features --locked
cargo clippy -p gateway-management-embedded --all-features --all-targets --locked -- -D warnings
cargo build -p gateway-management-embedded --example synthetic_host --locked
python3 -B scripts/embedded_host_smoke.py --binary target/debug/examples/synthetic_host
```

수동 확인 시 비어 있는 비공개 디렉터리를 만들고
`cargo run -p gateway-management-embedded --example synthetic_host --locked -- DIRECTORY`에
전달합니다. 출력된 numeric loopback 주소, 대상 `gateway`, 합성 조회 credential
`synthetic-embedded-read-key-01234567890123456789`를 사용합니다. 예제는 Web 파일이나
실제 Gateway 프로세스를 제공하지 않으며 고정 key를 제품 구성에 사용해서는 안 됩니다.
호스트 UI 연결에는 실제 인증된 조회 API를 재사용합니다.

테스트는 호스트 작업·수명 제어 거절, 축소 grant, 정체성 갱신, 알 수 없는 계약, 본문 정체성
주입, 정확한 런타임·endpoint 연결과 구분된 credential 경계를 확인합니다. 선택형 `team`
feature에서는 실제 모델 router fixture가 로컬 credential DB 없이 호스트 정체성을
사용합니다. 예제 smoke test는 실제 조회 범위, 수명 변경 거절, 제한된 stdin 종료를
확인합니다. 이 검증은 provider 적합성·Standalone 조립·외부 배포·소비자 운영 승인과 구분합니다.

기존 구성·활성화·continuation·사용량·credential 형식은 변환하지 않습니다. 복구에는 호환되는
검증 호스트 코드와 일관된 상태를 사용합니다. 미지원 작업·버전은 조용히 축소하지 않고
명시적으로 거절합니다.
