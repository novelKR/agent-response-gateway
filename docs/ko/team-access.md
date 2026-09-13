<a id="audited-changes-and-one-time-delivery"></a>
<a id="identity-and-purpose"></a>
<a id="optional-named-credentials-and-team-authority"></a>
<a id="store-ownership-and-recovery"></a>
<a id="validation-and-integration-limits"></a>

# 선택형 주체 credential과 팀 권한

[English](../team-access.md) | [한국어](team-access.md)

선택형 workspace 패키지 `gateway-team-access`는 등록 주체, 고엔트로피 credential,
정확한 route·작업 권한을 제공합니다. 선택형 관리 계약과 인증 인터페이스에 의존하며 기본
Gateway는 이 패키지에 의존하지 않습니다. 라이브러리는 listener·인증 세션·백그라운드
작업자를 열지 않습니다. 호스트가 Team Access 활성화 시 별도 비공개 저장소를 명시적으로
초기화하고 adapter를 연결합니다. 이 패키지만으로 모델 전달, 사용량 귀속, credential
전달 HTTP endpoint나 Standalone 서비스를 제공하지 않습니다.

제공 [Standalone 관리 애플리케이션](standalone-management.md)은 명시적 저장소, 조회 Web, 선택형 Team 접근으로 이 adapter를 조립합니다.

## 정체성과 용도

주체에는 활성화 여부, revision, 정확한 허용 route alias, 명시적 관리 대상·작업 grant,
전체 주체 사용량 조회의 별도 권한이 있습니다. role 단축 규칙, route wildcard, tenant별
provider key는 없습니다. 모델 admission adapter가 정확한 route를 확인하고 모델 목록에도
같은 범위를 적용해야 합니다. 사용량 adapter는 주체 범위를 검증해야 하며 `read_all_usage`는
quota·가격·청구를 계산하지 않습니다. 이 필드들은 소비 adapter의 계약이며 전달·회계 조립이
완료되었다는 증거가 아닙니다.

| Credential 용도 | 모델 admission | 관리 인증 |
|---|---|---|
| Model | 명시적 `authenticate_model`; route 권한은 별도로 필요 | 거절 |
| Management | 거절 | 주체에게 명시적으로 부여한 관리 grant만 허용 |
| ReadOnly | 거절 | 조회 grant만 허용; 호스트가 활성화한 조회 세션 생성 가능 |

`Authenticator`는 기존 관리 인증·refresh trait을 구현하고 별도의 모델 인증·refresh를
제공합니다. 클라이언트가 지정한 role·주체가 아닌 credential 값을 받습니다. Principal에는
서버가 검증한 정체성·용도·권한·권한 버전 digest가 있습니다. 주체 권한 변경은 기존 권한
버전을, credential 교체·폐기는 해당 credential을 무효화합니다. 다른 주체의 독립적인
credential 변경은 변경되지 않은 주체를 무효화하지 않습니다. 호스트는 실제 작업·모델
admission 직전에 권한을 갱신해야 합니다. 인증은 영구 권한이 아닙니다.

조회 세션 cookie는 관리 transport가 계속 소유합니다. 기존 refresh 검사가 Team Access
권한 변경·폐기를 반영합니다. 모델 key로 이 세션을 만들 수 없으며 ReadOnly key로 관리
변경을 수행할 수 없습니다. 비밀번호 로그인·가입·메일·MFA·외부 identity는 제공하지 않습니다.

## 감사 변경과 일회성 전달

`Manager::execute`는 신뢰된 actor, 관리 요청, 닫힌 domain command를 받습니다.
Command는 주체 등록·권한 변경·credential 발급·폐기·새 등록 credential ID로의 교체를
제공합니다. 요청에는 정확한 domain command digest, 대상, 예상 snapshot, 중복 방지
key를 연결합니다. SQLite IMMEDIATE transaction이 journal 시작 의도·시작 기록 동안
현재 generation·digest를 고정합니다. 모든 의도된 팀 변경보다 actor 권한과 영속 시작
기록을 먼저 확인합니다.

Credential token은 시작 기록 후 effect 안에서 256비트 난수와 용도 prefix로 생성합니다.
SHA-256 verifier만 저장합니다. 주체·credential 정보, 권한, 작업에 연결된 receipt는 관리
journal과 별도로 보존합니다. Receipt는 요청, 변경 전·후 snapshot, 정확한 발급 credential
정보·verifier를 연결합니다. 불변 journal은 key 값 없이 digest와 결과만 기록합니다.
Token은 비밀번호 hash나 ciphertext가 아니며 복호화·과거 token 조회 경로가 없습니다.

`Secret`은 journal이 성공을 확인한 뒤에만 반환합니다. Debug·Serialize를 구현하지 않고,
해제 시 zeroize하며 명시적 전달 메서드로만 값을 제공합니다. 신뢰된 호스트가 보호된 일회성
출력 경로를 사용해야 합니다. 같은 중복 방지 요청의 재시도에는 기존 작업만 반환하고 secret은
없습니다. 연결 단절·응답 손실은 token 재전달의 근거가 되지 않습니다. Credential ID를
조회하여 명시적으로 폐기하거나 교체합니다.

인증에는 일치하는 관리 journal의 발급·교체 성공 증거와 정확한 receipt 연결이 필요합니다.
결과 기록이 없는 발급은 DB 행이 있다는 이유만으로 사용할 수 없습니다. 명시적 상태 조정은
새 secret 발급·전달 없이 보존된 변경 증거를 확인할 수 있습니다. Receipt가 없거나 일치하지
않으면 미검증으로 남깁니다. 인증 adapter가 불변 발급 성공 증거를 메모리에 cache하여 이미
확인한 credential의 모델 admission마다 새 관리 기록이나 반복 감사 조회를 요구하지 않습니다.
현재 credential·주체 상태는 매 사용 시 확인합니다.

폐기·권한 축소는 팀 transaction이 commit되면 효력이 있으며 이후 감사 결과 기록이 실패해도
기존 권한을 복구하지 않습니다. 교체는 기존 key 폐기와 새 key 삽입을 함께 commit합니다.
중간 transaction 실패는 둘 다 rollback하며 commit을 확인할 수 없으면 미확정으로 남깁니다.
중단된 작업을 자동 재실행하지 않습니다.

## 저장소 소유권과 복구

기존 비공개 디렉터리를 `Manager::initialize`로 명시적으로 초기화합니다.
`Manager::open`은 지원 schema와 정확한 대상을 요구합니다. 저장소의 고정 Rust·SQLite
버전을 재사용하며 WAL/FULL, 배타적 writer lease, 기록 제한과 명시적 크기 제한을 적용합니다.
자동 schema 복구·정리·continuation/usage 변환·credential 환경 탐색은 없습니다.

초기 제한은 주체 128개, 보존 credential 1,024개, 주체별 route alias 128개와 관리 grant
256개이며 직렬화한 권한은 최대 48 KiB입니다. 폐기된 기록도 제한에 포함하며 증거로 보존합니다. 용량 충돌은 명시적으로
운영 검토해야 합니다. 제거 endpoint·자동 보존 정책은 없습니다. 식별자·receipt hash·권한은
조회 가능한 정보이며 공개 inventory에는 key verifier·값이 없습니다. 호스트가 별도로
inventory 조회 권한과 디렉터리·ACL·전달 경로를 보호해야 합니다.

`Manager::backup`은 선택한 비공개 위치에 기존 파일을 덮어쓰지 않는 일관된 SQLite 백업을
만듭니다. 일치하는 관리 journal도 보존해야 합니다. 호환되는 검증 프로그램과 일관된 저장소로
복구하며 폐기된 credential을 되살리려고 과거 팀 저장소를 복원해서는 안 됩니다. 증거 손실은
이전 key 추정이 아닌 명시적 복구·재발급으로 처리합니다. Ciphertext 재작성, 데이터 역변환,
완료된 모델 요청 재실행은 하지 않습니다.

## 검증과 연동 제한

```sh
cargo test -p gateway-team-access --locked
cargo clippy -p gateway-team-access --all-targets --locked -- -D warnings
```

합성 테스트는 용도·route·주체 분리, 일회성 출력, SQLite·WAL·journal 정보에 평문이 없음,
actor 거절, 낡은 snapshot, 시작 의도·결과 기록 실패, 명시적 상태 조정, 교체 중 rollback,
폐기, credential 증거 변조, 일관된 백업과 잘못된 schema·대상을 검증합니다. 실제 관리
router fixture로 교체·권한 변경 후 조회 세션 무효화를 확인합니다. 이는 모델 전송,
Standalone 실제 조립, 외부 배포, 소비자 승인과 별도의 검증입니다. 모듈을 끄면 기존
CLI·설정 흐름을 유지하고 팀 리소스를 만들지 않습니다.
