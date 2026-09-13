<a id="audit-evidence-and-external-changes"></a>
<a id="host-registration-and-configuration"></a>
<a id="owned-lifecycle-and-readiness"></a>
<a id="owned-runtime-and-configuration-adapter"></a>
<a id="validation-and-platform-limits"></a>

# 소유 실행 및 구성 adapter

[English](../managed-runtime.md) | [한국어](managed-runtime.md)

선택형 `gateway-management-runtime` 라이브러리는 검증된 구성 snapshot을 저장·선택하고
직접 시작한 `gateway-managed-child` 프로세스를 감독합니다. Gateway와 관리 계약에
의존하며, 기본 Gateway에는 역방향 의존성이 없습니다. 기존 CLI·loopback listener·배포
archive도 유지합니다. 관리 HTTP 서버·명령 인터페이스·대시보드는 별도 조립이 필요하며,
이 adapter를 설치하는 것만으로 제공되지 않습니다.

## 호스트 등록과 구성

신뢰된 호스트는 대상 하나, 비공개 상태 디렉터리, SHA-256으로 지정한 절대 경로의 자식
실행 파일, 로컬 구성 source ID, 선택형 확장·profile-pack 잠금 경로를 등록합니다.
명시적인 credential 환경과 credential-generation ID도 제공합니다. 등록은 HTTP 요청
스키마가 아닙니다. 신뢰되지 않은 명령은 프로그램·shell·PID·임의 서버 경로를 선택할 수 없습니다.

`Runtime::initialize`는 비공개 저장소를 새로 만들며 기존 상태 산출물을 덮어쓰지 않습니다.
`Runtime::open`은 변환·복구 없이 `gateway-runtime-state/v1`을 요구합니다.
관리자 하나만 디렉터리 lease를 소유합니다. Windows에서는 호스트 ACL로 비공개 접근을
보장해야 하며 Unix에서는 권한과 소유자가 일치하는 일반 파일을 검사합니다. symlink·reparse
경로·상위 경로 이동은 거절합니다. 다른 호스트 관리자가 등록 실행 파일과 원본을 동시에
교체하지 못하도록 보호해야 합니다.

`Command::Stage`는 정확한 원본 바이트 digest를 검사하고 기존 시작 parser로 검증해
불변 비공개 후보를 보존합니다. 후보 ID는 해당 바이트를 선택하며 파일명으로 직접 사용하지
않습니다. 구성 원문은 1 MiB로 제한하고 후보는 최대 128개 보존합니다. `Command::Select`는
후보를 다시 검증하고 저장 revision을 증가시킵니다. 이전 후보를 선택하는 것도 명시적인 새
작업이며 데이터베이스 역변환이나 요청 재실행이 아닙니다.

Native extension과 profile-pack 잠금은 기존 시작 의미를 유지합니다. 선택한 구성 원문은
고정되지만 등록 activation 잠금의 변경은 다음 검증과 실행에 반영됩니다. 적용 대상의 구성·실행
digest와 현재 소유 프로세스가 보고한 digest를 구분합니다. 선택만으로 프로세스를 시작·종료·
재시작하지 않습니다.

## 소유 수명과 readiness

`Command::Start`, `Command::Stop`, `Command::Restart`는 실제 소유 자식 handle을
대상으로 동작합니다. 점유 포트나 실행 lease는 소유권 증거가 아닙니다. 다시 연 controller는
기존 PID나 endpoint를 인수하지 않습니다. 상태는 `owned`, `stopped`, `unowned`를
구분하며 readiness 부재는 알 수 없는 상태이지 0 digest가 아닙니다.

비공개 stdin 규약 `gateway-managed-launch/v1`은 등록된 실행과 예상 구성·실행 digest를
전달합니다. 자식은 credential 조회와 bind 전에 이를 검증하고 일반 Gateway router·provider
transport·확장 runtime을 사용합니다. 부모는 endpoint 공개 전에
`gateway-managed-process/v1` envelope 전체와 Gateway readiness의 instance ID,
numeric loopback 주소, 고정·임시 포트, 스키마, 실제 digest를 검사합니다. 중복되거나 알 수
없는 readiness 필드는 거절합니다. 기존 v7까지의 manifest/readiness는 별도 계약으로 유지합니다.

선택된 구성에 필요한 로컬·provider·continuation credential 참조만 전달합니다. Windows에서는
운영체제 socket 지원을 위한 명시적인 `SYSTEMROOT` 바인딩도 필요합니다. 자식은
관리자 환경·proxy 변수·관계없는 관리/팀 key를 상속하지 않습니다. 신뢰된 호스트는 credential
영역을 구분하고 바인딩이 바뀌면 credential-generation ID를 변경해야 합니다. 비밀값은 실행
frame이나 상태에 나타나지 않습니다.

부모 채널 EOF나 stop frame은 설정된 Gateway 종료 유예를 시작합니다. 부모는 별도로 시작과
종료 시간을 제한하고 자신의 자식만 kill/reap할 수 있습니다. 강제 정리는 성공적인 정상 종료로
보고하지 않습니다. 정리를 확인할 수 없으면 미확정입니다. `Runtime::stop_owned`와 객체 정리는
감사 데이터베이스 장애 중에도 가능합니다. 종료나 HTTP 응답은 모델 완료를 증명하지 않습니다.
시작 실패와 중단된 요청에 자동 재시도나 대체 구성 적용은 없습니다.

## 감사 증거와 외부 변경

`Runtime::bind`는 형식화된 명령을 [관리 journal](management.md)에 연결합니다.
관리자 독점 lease 안에서 예상 snapshot과 명령 digest를 검사하고 적용 시 원본과 실행 정체성을
다시 확인합니다. 승인된 작업과 자식 종료 후 epoch가 바뀌므로 겉보기에 같은 나중 상태가
이전 요청을 유효하게 만들지 않습니다.

journal은 변경 전에 권한과 시작을 기록합니다. adapter는 작업 ID·요청 digest·감사용 관측
상태를 포함하는 별도 불변 완료 증거를 저장합니다. 정규화 manifest는 비공개 구성 산출물로
보존하고 작업 이력에 복사하지 않습니다. 구성 원문·credential 값·모델 내용은 완료 증거와
journal 이벤트에 들어가지 않습니다.

상태 파일 수정은 행위자를 추정하지 않고 `external_change`로 표시합니다. 호스트가 검사한 뒤
유효한 상태를 의도적으로 다시 열기 전까지 일반 변경을 거절합니다. 변조 후보나 잘못된 적용
대상 activation은 무효 상태로 표시합니다. 비상 소유 프로세스 종료는 계속 가능합니다.
파일·프로세스 제어·SQLite는 단일 원자적 트랜잭션이 아닙니다. 완료 증거 없이 중단된 변경은
미확정으로 남으며 상태 조정은 정확한 보존 증거만 읽고 작업을 재실행하지 않습니다. 과거 적용
증거는 endpoint가 현재도 살아 있다는 주장이 아닙니다.

자동 정리·후보 삭제·패키지 제거·continuation 재작성·사용량 데이터 삭제는 없습니다.
비공개 구성 상태는 관리 journal과 별도로 백업합니다. 호환되는 실행 파일·구성·패키지·보존
상태로 복구하며, 일부만 초기화된 경우 덮어쓰지 않고 검사한 뒤 새 위치를 사용해야 합니다.

## 검증과 플랫폼 제한

```sh
cargo test -p gateway-management -p gateway-management-runtime --locked
cargo test -p agent-response-gateway --test cli --locked
```

합성 실제 프로세스 테스트는 모델 전달·진행 중 SSE 종료·부모 연결 상실·bind 실패·선택 적용
대기·명시적 재시작·외부 변경·변조·시작 기록 실패·결과 기록 누락을 다룹니다. Unix fixture는
무응답/잘못된 자식·환경 분리·강제 정리도 검사합니다. CI는 기존 네 대상에서 native 수명 검사를
수행합니다. Native extension 실행은 Linux/macOS로 제한되며 이 adapter가 Windows 지원을
확대하지 않습니다. 통과 결과는 최종 제품 조립·live-provider 검증·외부 소비자 운영 승인이 아닙니다.
