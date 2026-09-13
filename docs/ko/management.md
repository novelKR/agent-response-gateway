<a id="audited-management-operations"></a>
<a id="ownership-and-access"></a>
<a id="validation"></a>
<a id="audited-management"></a>

# 관리 작업의 영속 감사

[English](../management.md) | [한국어](management.md)

선택형 Rust 라이브러리 `gateway-management`는 타입이 정의된 관리 계약과
영속 작업 증거를 제공합니다. 기본 게이트웨이는 이 라이브러리에 의존하지
않습니다. 이 기반은 listener나 프로세스를 시작하지 않고 credential도
탐색하지 않습니다. 독립형 관리 서버, 대시보드, 패키지 adapter, 팀 서비스는
별도 통합이며 이 라이브러리를 추가해도 자동으로 제공되지 않습니다.

<a id="management-ownership"></a>

## 소유권과 접근

신뢰된 호스트는 현재 인증과 권한 판정으로 불변 `Actor`를 구성합니다.
HTTP 본문에서 actor를 역직렬화할 수 없습니다. 권한은 정확한 `Action`과
대상을 결합합니다. 기능 조회는 호스트 지원과 해당 권한의 교집합을 반환합니다.
대기열 처리나 credential 변경 후를 포함해 실행 시점에 새로운 actor를 얻어야
합니다. 오래된 Rust 값을 보유하는 것은 인증이나 외부 폐기 상태 확인이 아닙니다.

`Request`는 중복 방지 키, 대상, 작업, 예상 `Snapshot`,
`parameters_sha256`를 포함합니다. adapter는 검증된 실제 비공개 명령에서
매개변수 digest를 계산해야 합니다. 감사용 요청은 임의 경로나 프로그램을
받는 공개 인터페이스가 아닙니다. 식별자에 payload나 비밀값을 넣으면 안 됩니다.

`Backend::prepare`는 대상에 의도한 변경을 일으키면 안 됩니다.
`PreparedOperation`은 journal이 snapshot을 확인하고 권한과 의도를
영속 저장한 뒤 실행을 시작하는 동안 대상 잠금 또는 동등한 호스트 lease를
유지합니다. lease로 보호되지 않는 조건은 적용 시 다시 확인해야 합니다.
라이브러리는 파일시스템, 프로세스, 패키지, credential 권한을 추가하지 않습니다.
도구 승인과 업무 흐름은 호스트의 책임으로 유지됩니다.

<a id="operation-evidence"></a>

## 작업 증거

계약은 `gateway-management/v1`입니다. 각 작업은 시작한 주체와 credential ID,
부여된 작업 권한, 요청 fingerprint와 순서 있는 이벤트를 유지합니다.
journal은 명령 본문, 설정 원문, credential 값을 저장하지 않습니다.

| 상태 | 의미 |
|---|---|
| queued | 권한과 의도가 저장됐고 적용은 시작되지 않음 |
| running | 시작 기록이 저장됐으며 효과가 발생했을 수 있음 |
| succeeded | adapter가 적용된 효과와 사후 상태 및 증거 digest를 보고함 |
| failed | adapter 또는 복구가 효과가 발생하지 않았음을 입증할 수 있음 |
| uncertain | 현재 증거로 실제 효과를 확정할 수 없음 |

권한 검사는 중복 조회보다 먼저 수행합니다. 같은 주체와 중복 방지 키가 같은
의도를 가지면 효과를 다시 실행하지 않고 기록된 작업을 반환합니다.
충돌하는 재사용은 거절합니다. 교체된 credential은 새로 권한이 확인된 actor로만
동일 의도를 조회할 수 있습니다. 주체별 키 영역은 분리됩니다.

adapter는 `Applied`, `NotApplied`, `Uncertain`을 반환합니다.
적용 결과에는 관측한 사후 상태와 증거 digest가 필요합니다. digest만으로
adapter가 실제 상태를 관측했음이 입증되지는 않습니다. `NotApplied`에는
의도한 효과가 발생하지 않았다는 증거가 필요합니다. 부분 효과는 미확정으로 유지합니다.

journal은 효과를 호출하기 전에 의도와 시작을 영속 저장합니다. 두 쓰기 중
하나라도 실패하면 적용하지 않습니다. 결과 기록 실패는 작업 ID를 포함한
미확정 오류를 반환합니다. 파일, 프로세스, SQLite는 하나의 원자적 트랜잭션이
아닙니다. 원래 작업을 자동으로 재실행하지 않습니다.

<a id="recovery-and-storage"></a>

## 복구와 저장

`Journal::initialize`는 기존 비공개 디렉터리가 필요하고 DB 교체를 거절합니다.
`Journal::open`은 `gateway-management-store/v1`이 필요하며,
지원하지 않는 저장소를 복구하거나 마이그레이션하지 않습니다.
저장소는 SQLite WAL/FULL, 배타적 writer 잠금, 설정한 페이지 한도를 사용합니다.
읽기 전용 `Reader` 접근에는 명시적인 대상 읽기 권한이 필요합니다.
조회는 크기가 제한된 로컬 cursor를 사용하며 시간으로 이벤트 순서를 판정하지 않습니다.
조회 페이지는 최대 100개 작업, 각 작업은 최대 256개 이벤트를 포함합니다.
증거 한도에 도달하면 이력을 덮어쓰지 않고 추가 기록을 거절합니다.
페이지 한도는 DB 페이지를 제한하며 전체 WAL이나 파일시스템 자원 소비를
제한하지는 않습니다.

다시 열거나 실행 중인 작업이 없는 writer가 권한이 확인된 재요청 또는 조정을
처리할 때 queued 작업은 적용이 시작되지 않았으므로 실패가 됩니다.
running 작업은 미확정이 됩니다. 자동 복구 이벤트에는 actor가 없으며,
최초 주체는 작업에 그대로 남습니다. 별도로 권한을 받은 조정자는
읽기 전용 backend 검사로 증거를 추가합니다. 조정은 원래 명령을 실행하거나
이전 이벤트를 덮어쓰지 않습니다.

DB trigger는 일반적인 작업·이벤트 행 수정과 삭제를 막습니다.
이는 API 무결성 검사이며 관리자나 악의적인 동일 사용자 코드의 DB 변경을
방지하지 않습니다. 호스트 권한과 백업을 보호해야 합니다.
Unix 저장소는 소유자가 일치하는 비공개 일반 파일만 허용하고 추가 hard link를
거절합니다. link와 Windows reparse 경로도 거절합니다.
Windows 디렉터리 접근은 POSIX 모드 보장이 아닌 호스트 ACL 책임입니다.

백업은 새 파일을 만들고 저장된 WAL 데이터를 포함합니다. 초기화와 백업은
신뢰된 호스트의 유지보수 메서드이며 미인증 endpoint가 아닙니다.
초기화나 백업 실패 시 불완전한 파일이 남을 수 있습니다. 검사 후 새 대상을
선택해야 하며 조용히 덮어쓰거나 지우면 안 됩니다. 자동 보존 정책, 패키지 제거,
데이터 삭제, 추론 재시도는 제공하지 않습니다.

<a id="management-validation"></a>

## 검증

라이브러리 집중 검사와 저장소 검증을 실행합니다.

```sh
cargo test -p gateway-management --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

합성 테스트는 영속 admission 실패, 시작·결과 기록 중단, 충돌하는 재요청,
권한 거절, 대상별 읽기, 배타적 소유권, 조정, 백업, 잘못된 저장소를 확인합니다.
외부 adapter의 정확성, 브라우저 보안, 실제 provider 동작, 소비자 운영 수용을
입증하지는 않습니다.
