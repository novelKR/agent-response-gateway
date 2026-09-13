<a id="authentication-target-and-transport"></a>
<a id="initialization-backup-and-verification"></a>
<a id="managed-session-ownership"></a>
<a id="scoped-team-model-entry-point"></a>
<a id="usage-evidence-and-failure-semantics"></a>

# 팀 주체별 모델 진입점

[English](../team-model-access.md) | [한국어](team-model-access.md)

`gateway-team-http`는 [팀 credential](team-access.md), 모델 admission,
관리형 세션 소유권, 정확한 사용량 연결을 위한 선택형 HTTP router입니다. 신뢰된 호스트가
인증 adapter, 등록 Gateway peer, 별도 요청 ledger와 선택형 Recorder reader를
명시적으로 제공합니다. 기본 Gateway는 이 패키지에 의존하지 않습니다. Router 생성은
listener·provider·Gateway 프로세스·반복 작업을 시작하지 않습니다. Standalone 프로세스
소유권과 배포물 조립에는 별도 호스트 구성이 필요합니다.

## 인증·대상·전송

Credential 저장소, 요청 ledger와 매번 선택한 peer는 같은 등록 대상을 지정해야 합니다.
Peer에는 검증된 실행·구성 정체성, 관측된 선택형 Recorder producer ID, 정확한 route
alias와 numeric loopback HTTP endpoint가 있습니다. Gateway 모델 credential과
continuation 제어 credential은 구분된 비공개 zeroizing 값이며 Team token prefix를
거절합니다. 모델 요청의 URL·본문·role·주체 필드에서 이 값을 가져오지 않습니다.

호스트가 `PeerSource`를 구현하거나 고정된 검증 peer를 제공합니다. 소유 실행이 바뀌면
peer를 철회하거나 교체합니다. PID·포트로 프로세스를 인수하거나 서비스를 탐색하거나
Gateway를 재시작하지 않습니다. 이미 허용한 요청은 해당 peer 정체성을 유지하고 새 호출은
현재 등록을 검사합니다. 중지·불일치·미제공 peer는 명시적 오류입니다.

Model 용도 credential만 이 인터페이스를 인증합니다. Management·ReadOnly key,
브라우저 관리 cookie와 주입한 `x-gateway-*` 헤더로 권한을 얻을 수 없습니다.
Admission writer를 얻은 뒤 변경 전에 권한 버전을 다시 확인합니다. 정확한 route 권한을
호출과 실제 Gateway 모델 목록에 같이 적용합니다. 호스트가 선택한 bind는 numeric
loopback이며 Host는 주소·포트와 일치해야 하고 Origin이 있으면 같은 HTTP Origin이어야
합니다. 외부 HTTPS·identity 시스템은 별도 호스트 경계입니다.

| 경로 | 메서드 | 동작 |
|---|---|---|
| `/v1/models` | GET | 실제 Gateway 모델을 등록 범위·주체 권한으로 필터링 |
| `/v1/responses` | POST | 허용한 JSON 요청 한 번을 전달하고 모델 재시도 없이 응답 stream 전달 |
| `/team/v1/usage` | GET | 본인 요청·사용량 증거; 전체 팀은 별도 권한 필요 |
| `/team/v1/sessions` | POST | 모델·중복 방지 key로 소유 관리형 세션 생성 |
| `/team/v1/sessions/{id}` | GET | 해당 주체에게 허용된 세션 상태만 조회 |

Responses 입력·프로토콜 호환성은 Core가 계속 소유합니다. 기존 stateless 정책이 저장·
백그라운드 호출, upstream conversation·history 참조와 compaction을 거절합니다.
팀 전달 계층은 해당 의미를 몰래 제거하거나 별도 native history 추정 규칙을 추가하지 않습니다.
다른 endpoint, WebSocket, 이미지·음성 진입점, 내부 제어 경로와 세션 전환은 제공하지 않습니다.

요청에는 등록된 로컬 Gateway credential과 제어된 헤더만 사용합니다. Redirect·환경
proxy·재시도는 꺼져 있습니다. Gateway 호출 전에 팀 요청 ID를 영속 기록합니다. 응답
헤더가 성공했거나 Gateway 헤더 이전에 실패하면 `x-team-request-id`를 제공합니다.
유효한 Gateway `x-request-id`는 별도로 반환할 수 있습니다. HTTP 성공이나 ID 수신은
모델 완료가 아닙니다. 누락된 ID를 모델명·시간 근접성으로 복원하지 않습니다.

기본 동시 요청은 32개, blocking job은 최대 64개입니다. 요청 2 MiB·응답 16 MiB,
요청 본문 30초, 헤더·stream idle 120초 제한을 적용합니다. 호스트는 `Limits`로 제한된
대안을 선택할 수 있습니다. Stream 해제 시 결과 기록 전에 upstream body를 취소합니다.
EOF, 클라이언트 단절, 연결 실패, body 손실, idle·헤더 timeout, 응답 크기 초과는 서로
다른 전송 관측입니다. 어느 것도 모델 결과·사용량·청구 가능한 완료를 만들어 내지 않습니다.

## 사용량 증거와 실패 의미

별도로 초기화하는 `team-requests.sqlite3`는 기존 SQLite 버전, WAL/FULL과 단일
writer를 사용합니다. 불변 기록에는 admission 정체성·권한, route, peer 정체성, 시각,
헤더 연결과 전송 종료를 보존합니다. 모델 본문·프롬프트·도구 내용·credential 값·추정한
토큰 수는 저장하지 않습니다. Credential·관리 감사·continuation·Recorder 저장소와 구분합니다.

Producer/request 쌍에는 하나의 팀 요청 소유자만 있습니다. `SqliteUsage`는 기존
Recorder의 명시적 조회 전용 열기와 현재 이벤트 조회를 재사용합니다. 정확한 producer와
Gateway가 반환한 request ID로 조회하고 attempt ID를 유지하며 route·구성 정체성을
검사합니다. 중복 attempt·불일치 증거는 중복 집계·재귀속하지 않고 미관측으로 둡니다.
Recorder schema 변환·전달 worker는 추가하지 않습니다. 내장 `SqliteUsage` adapter와
Recorder 의존성은 Linux/macOS에서만 제공하며 다른 플랫폼에서는 저장소를 열지 않고
미지원으로 표시합니다. 호스트가 별도 `UsageReader`를 명시적으로 제공할 수 있습니다.
모델 전송·일반 연결 계약은 독립적으로 제공하며 Windows Native Recorder 지원은
확대하지 않습니다.

사용량 조회는 `from_ms`, `to_ms`, 선택형 `after`·`all`을 받습니다. 최대 366일 범위에서
cursor 오름차순으로 요청 기록 최대 100개를 반환합니다. 조회 구간은 명시적으로
**팀 admission 시각**이며 Recorder의 attempt 시작 집계 시각과 다릅니다. 요청별 원본
canonical attempt 관측에는 null 카운터, 출처·finality, 불완전 관측과 upstream/Gateway
결과가 유지됩니다. 사용자별 청구·quota는 계산하지 않습니다.

기본 범위는 인증된 주체입니다. `all=true`는 명시적 `read_all_usage` 권한을 요구하고
보존된 팀 요청만 포함합니다. 다른 Gateway 사용량의 소유권을 주장하지 않습니다. 주체
필터를 직접 지정하면 거절합니다. 응답은 2 MiB, 요청별 조회는 attempt 16개로 제한합니다.
Recorder가 실패하면 같은 조회의 추가 lookup을 멈추고 미관측 사유를 보존합니다.

Producer/request 연결이 없으면 `unattributed`입니다. 연결되지 않은 Recorder,
없는 관측·잘못된 연결은 명시적 `unobserved`입니다. Admission 기록 실패는 Gateway
호출을 막습니다. 이후 연결·종료 기록 실패는 실행 중인 모델 응답을 재실행하거나 버리지
않으며 누락 증거를 미확정으로 남깁니다. Blocking admission 결과 자체가 활성 요청 guard를
소유하므로 취소된 호출자가 실제로 없는 요청을 활성 상태로 남기지 못합니다. Ledger를 다시
열어도 미완료 요청을 재실행하거나 완료로 간주하지 않습니다.

## 관리형 세션 소유권

`{ "model": "writer", "idempotency_key": "new-session-1" }`로 세션을 만듭니다.
진입점이 의도를 기록한 뒤 설정된 route origin과 별도 호스트 제어 token을 사용합니다.
공개 opaque 세션 ID는 Core 세션 UUID·주체·정확한 route·origin digest에 연결됩니다.
반환된 제어 관측을 검사한 뒤 연결하고 권한 범위의 상태 정보만 제공합니다.

관리형 모델 호출에서 공개 ID를 `x-team-session`에 넣습니다. 다른 주체의 ID·다른 route·
변경된 origin은 거절합니다. 모델 클라이언트는 내부 `x-gateway-session`이나 제어 token을
제공하지 않습니다. Core가 암호화 replay의 세션·origin과 pending tool·continuation
상태를 계속 검증합니다. 이 진입점은 도구 승인·세션 전환을 수행하지 않습니다.

생성 결과가 실패하거나 유실되면 `unconfirmed`로 남깁니다. 같은 중복 방지 key를 다시
보내도 Core 세션을 자동으로 추가 생성하지 않습니다. 연결된 재시도는 같은 공개 ID를
반환합니다. 상태 조회는 unknown·pending, epoch·revision을 유지하고 제어 token·
내부 ID·전체 origin 구성을 노출하지 않습니다. 호스트 상태 조정·명시적 새 세션 결정은
별도 작업입니다.

## 초기화·백업·검증

기존 비공개 디렉터리를 `Ledger::initialize`로 초기화합니다. 기존 ledger는 정확한 대상과
지원 schema로만 열립니다. 초기 제한은 보존 요청 1,000,000개, 세션 의도 16,384개와
설정된 저장소 크기입니다. 자동 삭제·schema 복구·보존 정책·기존 저장소 변환은 없습니다.
`Ledger::backup`은 기존 파일을 덮어쓰지 않는 일관된 비공개 SQLite 백업을 만듭니다.
호환되는 검증 코드와 일관된 관련 저장소로 복구하며 이를 모델 재실행으로 해석하지 않습니다.

실제 프로세스 fixture 전에 Gateway 실행 파일을 빌드합니다.

```sh
cargo build -p agent-response-gateway --locked
cargo test -p gateway-team-http --locked
cargo clippy -p gateway-team-http --all-targets --locked -- -D warnings
```

테스트는 실제 Core router·관리형 Gateway subprocess, 합성 credential·mock provider,
기존 Recorder 저장 구현을 사용합니다. 주체·route 분리, 암호화 세션 replay 거절, 정확한
사용량 소유권, null·부분 관측, stream 취소·실패, 중단된 admission, 제공되지 않는 증거,
명시적 백업·재개방을 확인합니다. Fixture 검증은 제공 Standalone 서비스·외부 배포·소비자
운영 승인과 구분합니다. 모듈 설치·활성화는 호스트의 명시적 선택이며 일반 Gateway는
이 리소스를 시작하지 않습니다.
