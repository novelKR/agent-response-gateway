<a id="authentication-and-host-boundary"></a>
<a id="browser-read-sessions"></a>
<a id="cli-use-and-verification"></a>
<a id="http-surface"></a>
<a id="management-http-and-cli-contracts"></a>
<a id="preflight-admission-and-operation-evidence"></a>

# 관리 HTTP 및 CLI 계약

[English](../management-api.md) | [한국어](management-api.md)

선택형 `gateway-management-api` 패키지는 loopback HTTP router, 로컬 인증 adapter,
`gateway-management-cli` client를 제공합니다. 신뢰된 호스트가 초기화한 journal·reader·dispatcher를
전달합니다. 이 패키지가 서버를 자동 시작하거나 runtime·패키지·사용량·팀 서비스를 자체 조립하지는
않습니다. 기본 Gateway는 이 패키지에 의존하지 않습니다.

## 인증과 호스트 경계

`Service::new`는 실제 bind된 numeric loopback 주소와 등록 대상 하나를 요구합니다.
HTTP Host는 정규화된 주소·포트와 일치해야 하고, Origin이 있으면 같은 HTTP origin이어야 합니다.
브라우저 세션 생성과 로그아웃에는 해당 Origin이 명시적으로 필요합니다. wildcard CORS 허용·
redirect·환경 proxy는 없습니다. 이 router를 내장한 호스트가 bind와 인증 책임을 유지하며,
loopback 주소를 선언하는 것만으로 별도 공개 listener가 보호되지는 않습니다.

`Authenticator`는 현재의 불변 actor, credential 종류, 권한 버전을 반환합니다. 요청 본문이나
identity header에서 role·subject를 가져오지 않습니다. 대기 중인 요청을 포함해 writer를 얻은 뒤
변경 권한을 다시 확인합니다. 각 작업에는 정확한 대상/작업 권한과 호스트 지원이 필요합니다.
호스트가 지원하지 않는 작업은 시작 기록 전에 거절합니다.

`LocalAuthenticator`는 명시적으로 준비한 고엔트로피 credential을 받고 메모리에 hash를 보관합니다.
관리용과 조회 전용을 구분하며 조회 전용에는 변경·상태 조정 권한을 포함할 수 없습니다. 이 adapter는
비밀번호·가입·메일·MFA 시스템이 아닙니다. Credential 준비와 호스트 인증은 transport 패키지 밖의
책임입니다.

## HTTP 인터페이스

Envelope schema는 `gateway-management-http/v1`이며 Responses와 manifest/readiness v7에서
독립되어 있습니다. 성공 응답에는 schema와 관측 시각이 포함됩니다. 오류는 header·credential 값·제출
데이터를 반사하지 않는 고정 코드를 사용합니다. Dispatcher가 권한 내 module view와 제공 기능을
선언하며 transport는 누락·미관측 값을 0으로 바꾸지 않습니다.
상태는 `gateway-management-state/v1`을 사용하고 이름 있는 module view를 최대 16개 포함합니다.
각 view는 자체 계약과 `observed`, `unobserved`, `unsupported` 관측을 선언합니다. 관측된
데이터에는 자체 시각과 일치하는 schema가 있습니다. 사용자 호스트는 관리 패키지에 업무 타입을
넣지 않고도 범용 module metadata를 제공할 수 있습니다.

| `/management/v1` 아래 경로 | 메서드 | 필요한 권한 |
|---|---|---|
| `/capabilities` | GET | 상태 조회; 호스트 지원 작업과 actor 허용 작업 보고 |
| `/state` | GET | 상태 조회와 호스트 지원 |
| `/usage` | GET | 사용량 조회와 호스트 지원 |
| `/continuations/{id}` | GET | 상태 조회와 호스트 제공 continuation 조회 |
| `/operations` | GET | 대상 작업 이력 조회 |
| `/operations/{id}` | GET | 대상 작업 이력 조회 |
| `/preflight` | POST | 관리 credential과 요청 작업 권한 |
| `/operations` | POST | 관리 credential과 요청 작업 권한 |
| `/operations/{id}/reconcile` | POST | 관리 credential·상태 조정 권한·호스트 지원 |
| `/session` | POST, DELETE | 아래의 선택형 조회 세션 경계 |

조회 요청에는 등록 대상이 포함됩니다. 작업 목록은 로컬 행 cursor를 사용하며 기본 페이지 크기는
20, 최대는 100입니다. 사용량은 `from_ms`, `to_ms`, `timezone`을 받고 요청 범위는 최대
366일입니다. Module adapter의 자체 조회 검증과 관측 의미도 유지됩니다. 요청 본문은 64 KiB,
응답은 2 MiB로 제한합니다. Blocking 요청/작업은 최대 64개를 수용하며 용량 초과는 변경 없이
명시적인 busy 응답으로 반환합니다.

Wire 명령은 runtime 시작/종료/재시작, 구성 후보 저장/선택, 패키지 설치/활성화/비활성화/버전 선택,
호스트 continuation 전환을 위한 닫힌 등록-ID 필드를 사용합니다. 서버 실행 파일·shell·임의 서버
경로를 지정할 수 없습니다. 패키지 종류와 정확한 선택을 명시해야 하며 알 수 없는 필드와 미지원 제거
명령은 거절합니다. 호스트는 continuation 업무 조건을 독립적으로 확인해야 합니다. Client가 보낸
pending flag가 도구 승인이나 업무 상태 소유권을 Gateway로 이전하지 않습니다.

## 사전 검증·시작 기록·작업 증거

사전 검증은 schema·대상·중복 방지 키·형식화된 명령을 받습니다. 현재 권한과 호스트 지원을 확인하고
현재 snapshot을 관측한 뒤 해당 snapshot으로 명령을 검증합니다. 작업 생성이나 변경 없이 완전한
제출 후보를 반환하며, 미래의 실행 권한을 뜻하지 않습니다.

제출에는 해당 예상 snapshot을 포함합니다. Dispatcher는 정확한 canonical wire 의도를 prepared
operation과 보존 증거에 결합해야 합니다. Module 명령으로 변환할 때 관계없는 매개변수 digest로
조용히 바꾸면 안 됩니다. Journal은 준비된 상태를 다시 확인하고 권한·의도를 저장한 뒤 적용 전에
시작 기록을 저장합니다. 반환된 작업 ID는 영속 시작 의도 또는 이전에 기록된 동일 요청을 의미하며
적용 성공을 뜻하지 않습니다. HTTP router와 CLI는 요청을 자동으로 재시도하지 않습니다.

작업 조회는 변경하지 않은 영속 기록인 `operation`, `observed_state`, 선택형 `uncertainty`
사유를 반환합니다. 시작 기록 후 결과를 남기지 못하고 worker가 종료하면 현재 프로세스는 계속 실행
중이라고 주장하지 않고 미확정을 보고합니다. 이 관측에는 ID만 포함하고 감사 이력을 다시 쓰지 않습니다.
재시작 후에는 journal의 기존 복구 규칙이 불완전한 기록을 표시합니다. 상태 조정에는 별도 권한이 필요하고
adapter 증거만 읽으며, 원래 명령을 반복하거나 모델 완료를 추정하지 않습니다.

작업 조회는 별도 데이터베이스 reader를 사용하므로 변경 작업이 writer를 점유해도 가능합니다.
Native package와 구성 adapter는 자체 동기화·원본 검사를 유지해야 합니다. Callback fixture가
HTTP 검사를 통과했다고 실제 adapter 조립을 완료한 것은 아닙니다.

## 브라우저 조회 세션

호스트가 조회 세션을 명시적으로 켜야 하며, 그렇지 않으면 해당 경로가 없습니다. 전용 조회 credential로만
생성할 수 있고 관리 credential은 이 흐름에서 거절합니다. 브라우저는 관리 경로로 범위가 제한된
HttpOnly·SameSite=Strict opaque cookie를 받습니다. Cookie 이름은 origin별로 다르고 수명은
15분입니다. Live session은 최대 256개를 유지하며 background session worker는 없습니다.
Cookie 값은 hash로 저장하고 조회 API에서 반환하지 않습니다.

세션을 사용할 때마다 credential identity와 권한 버전을 확인합니다. 폐기·버전 변경·만료·identity
불일치는 세션을 거절합니다. Cookie는 사전 검증·변경·상태 조정 권한으로 사용할 수 없습니다.
로그아웃은 해당 조회 세션만 제거합니다. 제공 Dashboard는 실제 조회 API를 사용해야 하며 변경용
관리 credential을 받거나 보존·전송해서는 안 됩니다.

## CLI 사용과 검증

CLI는 명시적으로 선택한 numeric loopback HTTP endpoint를 받습니다. 보호된 호스트 credential
원본에서 `GATEWAY_MANAGEMENT_TOKEN`을 제공하며 토큰 값 자체를 명령행 인수로 전달하지 않습니다.
Redirect·상속 proxy·자동 재시도는 비활성화되어 있습니다. 출력은 크기가 제한된 버전 있는 응답
metadata이며 요청 header나 credential 값이 아닙니다. Client HTTP 응답은 모델 완료 증거가 아닙니다.

```sh
gateway-management-cli --endpoint http://127.0.0.1:47100 capabilities --target gateway
gateway-management-cli --endpoint http://127.0.0.1:47100 state --target gateway
gateway-management-cli --endpoint http://127.0.0.1:47100 preflight --file preflight.json
gateway-management-cli --endpoint http://127.0.0.1:47100 submit --file submission.json
cargo test -p gateway-management-api --locked
```

위 endpoint·대상·입력 파일명은 예시이며 자동 생성되는 서비스 상태가 아닙니다. 사전 검증 응답의
제출 내용을 검토한 뒤 전송해야 합니다. 합성 테스트는 권한, Host/Origin 경계, 조회 세션,
대기 중 권한 변경, 오래된 상태, 시작/결과 기록 실패, 중복 방지와 실제 CLI/loopback HTTP 교환을
다룹니다. 배포 archive·제공 Web asset·팀 접근·실제 adapter 조립은 별도 검증 대상입니다.
