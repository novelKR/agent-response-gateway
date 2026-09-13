<a id="initialize-and-operate-explicitly"></a>
<a id="optional-standalone-operations"></a>
<a id="register-one-owned-target"></a>
<a id="select-components"></a>
<a id="team-credentials-and-model-access"></a>
<a id="verification-artifacts-and-recovery"></a>
<a id="web-and-usage-views"></a>

# 선택형 Standalone 운영

[English](../standalone-management.md) | [한국어](standalone-management.md)

`gateway-management-app` 패키지는 실제 구성·소유 실행·확장·감사·사용량 adapter를
[관리 API](management-api.md)에 연결합니다. `gateway-manager`, 선택형 정적 대시보드,
별도로 선택하는 Team 빌드를 제공합니다. 기본 `agent-response-gateway` 실행 파일과
의존 경로에는 관리·Web·Team이 포함되지 않습니다. 기존 CLI와 Embedded 사용은 이
구성 요소 없이도 가능합니다.

## 구성 요소 선택

| 구성 요소 | 실행 동작 |
| --- | --- |
| Gateway archive | 기존 모델 전송과 선언된 route |
| 관리 archive | `gateway-manager`, `gateway-managed-child`, 관리 CLI, 로컬 패키지 driver와 고지 |
| Web archive | 관리자가 제공하는 검증된 Vue 정적 앱. Node는 빌드에만 필요 |
| Team archive | `gateway-team-manager`. 등록에서 Team을 선택해야 저장소·인증·listener를 생성 |
| Embedded 라이브러리 | 호스트 인증과 명시적 위임. 업무 상태와 실행 수명은 호스트 소유 |

동일 소스 commit의 archive를 사용합니다. Web·Team module manifest는 정확한 관리 소스
commit을 선행 조건으로 선언합니다. 다른 패키지 버전을 선택하는 것은 DB downgrade,
ciphertext 재작성, 모델 요청 재실행을 뜻하지 않습니다. 어느 프로그램도 다운로드한
확장을 자동 설치·활성화하지 않습니다.

일반 관리 빌드에는 Team 의존성이 없습니다. Team 빌드도 `team: null`로 실행할 수 있으며,
이 경우 Team DB·listener·worker를 만들지 않습니다. Native 확장 관리와 내장 SQLite
Recorder reader는 Linux·macOS를 지원합니다. Windows에서는 profile pack 관리,
실행·구성 제어, Team 모델 전송과 요청 증거를 제공하며 native 확장·Recorder 기능은
미지원으로 유지합니다. Native 패키지 관리에는 등록한 Python 3.11+ interpreter가
명시적으로 필요합니다. Interpreter 자체는 배포물에 포함하지 않습니다.

## 소유할 대상 하나 등록

비공개 JSON 등록 파일은 `gateway-management-registration/v1`을 사용합니다. 등록은
HTTP 명령 본문과 별개인 신뢰된 로컬 설정입니다. Symbolic link가 없는 절대 경로,
정확한 managed-child 실행 파일 digest, source ID, 환경 참조 연결, 로컬 credential
정체성을 제공합니다. 구성 후보는 원문 바이트를 보존하고 시작 전에 현재 활성화 파일과
검증합니다. 관리자는 부모 프로토콜로 자신이 시작한 child만 제어하며 PID나 사용 중인
포트를 인수하지 않습니다.

다음은 등록 형식입니다. 꺾쇠 placeholder를 실제 절대 경로와 선택한 managed-child의
SHA-256으로 바꿉니다. Gateway 구성은 명시적으로 전달한 환경 변수 이름을 참조해야
합니다. 예제는 관리만 선택하며 자체 모델 route를 노출하지 않습니다.

```json
{
  "schema": "gateway-management-registration/v1",
  "target": "gateway",
  "listen": "127.0.0.1:48080",
  "journal": "<absolute-private-journal-directory>",
  "runtime": {
    "directory": "<absolute-private-runtime-directory>",
    "executable": "<absolute-path-to-gateway-managed-child>",
    "executable_sha256": "<64-lowercase-hex-sha256>",
    "credential_generation": "local-generation-1",
    "sources": {
      "local-config": {
        "configuration": "<absolute-path-to-gateway-toml>",
        "extensions_lock": null,
        "profile_packs_lock": null
      }
    },
    "environment": {
      "ARG_LOCAL_TOKEN": "GATEWAY_MODEL_TOKEN",
      "PROVIDER_KEY": "SELECTED_PROVIDER_KEY"
    }
  },
  "credentials": [
    {
      "subject": "local:operator",
      "credential": "local:management-key",
      "token_env": "GATEWAY_MANAGEMENT_TOKEN",
      "read_only": false,
      "grants": [
        {"target": "gateway", "action": "read_state"},
        {"target": "gateway", "action": "read_operations"},
        {"target": "gateway", "action": "configuration_stage"},
        {"target": "gateway", "action": "configuration_select"},
        {"target": "gateway", "action": "runtime_start"},
        {"target": "gateway", "action": "runtime_stop"}
      ]
    }
  ],
  "native": null,
  "profile_packs": null,
  "usage": null,
  "continuation": null,
  "web": null,
  "team": null
}
```

32자 이상의 출력 가능한 ASCII 문자로 된 독립적인 고엔트로피 로컬 credential을
사용합니다. 값은 지정한 환경 변수에서 읽으며 등록과 HTTP API에는 값 대신 참조를
넣습니다. Child 환경에는 Gateway 구성의 참조만 포함하고 Windows에서는 명시적으로
등록한 `SYSTEMROOT`도 전달합니다. 관리·조회 credential 값이나 `gwt1_` Team key를
Gateway·provider 환경에 연결하면 거절합니다. Provider와 Gateway 모델 credential의
기존 Core 분리 규칙도 유지합니다.

로컬 subject·credential ID는 `local:`로 시작해야 합니다. Standalone Team adapter는
이 namespace를 예약하고 충돌하는 기존 Team 저장소를 거절합니다. 검사하지 않은 문자열
prefix만으로 정체성 출처를 판단할 수 없으므로, 관리자는 인증을 합성하기 전에 로컬 등록과
Team 상태를 검증합니다. Embedded 호스트의 정체성 namespace는 해당 호스트 책임입니다.

## 명시적 초기화와 운영

먼저 비공개 부모 디렉터리를 만듭니다. Windows 디렉터리 ACL 보호는 호스트·운영자 책임이며 POSIX 모드 검사가 Windows ACL을 보장하지 않습니다. 선택한 새 저장소를 각각 초기화하며 기존 저장소의
스키마를 자동 변환하거나 삭제로 복구하지 않습니다. 반복 초기화는 실패하고 데이터를
덮어쓰지 않습니다.

```sh
gateway-manager --registration registration.json init --component management
gateway-manager --registration registration.json init --component runtime
gateway-manager --registration registration.json serve
```

추가 초기화 요소는 `native`, `profile-pack`, `continuation`, `team`, `team-requests`이며
해당 등록이 있어야 합니다. 패키지 저장소와 Recorder·continuation 데이터 저장소는 기존
도구로 초기화합니다. 관리 초기화는 기존 데이터 형식을 초기화·변환하지 않습니다.
`serve`는 초기화된 adapter를 열지만 감사된 시작 작업을 제출하기 전에는 Gateway를
시작하지 않습니다.

관리자는 실제 numeric loopback endpoint를 `gateway-management-listeners/v1` 정보로
출력합니다. 포트 0은 새 로컬 포트를 요청합니다. 전달 header, 공개 listener, 원격 정체성
제품을 암묵적으로 활성화하지 않습니다. Supervisor는 `serve --parent-stdin`을 사용하며
stdin을 닫으면 관리자와 소유 Gateway가 종료됩니다. 대화형 실행은 Ctrl-C를 처리합니다.
종료 시 관리 변경 접수를 먼저 닫고 이미 실행 중인 작업을 소유 잠금 안에서 마친 뒤,
감사 저장소 상태와 별개로 소유 child를 정리합니다. 대기하던 변경이 정리 시작 후 Gateway를
다시 시작하지 못합니다. 모델 요청은 자동 재제출하지 않습니다.

CLI 사전 검증 결과의 `data.submission`을 그대로 `submit` 입력으로 사용합니다. 사전
검증은 권한을 부여하거나 변경을 적용하지 않습니다. 작업 ID를 저장·확인하고 상태를
조회합니다. `202` 응답은 접수만 증명하며 실행 성공을 뜻하지 않습니다. 저장·선택·시작은
각각 별도 명령입니다.

```json
{"kind":"configuration_stage","source":"local-config","candidate":"candidate-1","source_sha256":"<exact-config-file-sha256>"}
{"kind":"configuration_select","candidate":"candidate-1"}
{"kind":"runtime_start"}
```

각 명령을 등록 대상과 별도의 idempotency key가 포함된
[버전이 있는 사전 검증 envelope](management-api.md)에 넣습니다. 결과 submission은
예상 상태 revision·digest를 포함합니다. 미확정 재시도에는 원래 submission을 사용하며,
같은 key의 예상 상태나 의도를 바꾸면 충돌입니다. Timeout을 자동 재시작이나 새 모델
요청으로 바꾸지 않습니다.

로컬 패키지 등록에는 `directory`, `store`, 등록한 `sources`(`path`와 정확한
`package_sha256`), `recorder_bindings`, `driver`를 넣습니다. Native driver는 Python과
기존 관리자 스크립트의 절대 경로 및 각 파일 hash를 고정합니다. Profile pack은
`driver: null`을 사용하며 Recorder 연결을 넣지 않습니다. Source·grant 검증은 기존
관리자의 변경 경계 안에서 재사용합니다. 설치·활성화·비활성화·정확한 버전 선택은
실행 시작·재시작과 별개입니다. Profile 버전 교체는 먼저 명시적으로 비활성화해야 합니다.
제거·데이터 삭제·원격 검색·다운로드·자동 업데이트는 지원하지 않습니다.

설치됨, 다음 시작에 선택됨, 실제 실행 manifest에 반영됨은 서로 다른 관측입니다.
Effective codec 항목은 실행 구성을 나타내며 상주 codec 프로세스의 증거가 아닙니다.
비활성화한 패키지, 사용량 기록과 연속성 데이터는 보존합니다.

## Web과 사용량 조회

`web`에는 검증된 정적 디렉터리의 절대 경로와 `web-manifest.json`의 정확한 SHA-256을
등록합니다. 관리자는 clean 소스 정보, API·상태 계약 호환성, 허용한 각 asset digest를
검증한 뒤 해당 불변 바이트를 같은 numeric loopback Origin에서 제공합니다. 알 수 없는
파일이 asset route로 추가되지 않습니다. 제품 실행에는 Vite 개발 서버나 Node가 없습니다.

필요한 조회 grant를 가진 별도의 `read_only: true` credential을 등록합니다. 브라우저는
이 값을 제한된 HttpOnly/SameSite 조회 세션을 만들 때만 사용합니다. 관리 credential로는
조회 세션을 만들 수 없습니다. Host·Origin을 검사하고 정적 콘텐츠에는 CSP, no-store,
nosniff, no-referrer 정책을 적용합니다. 대시보드에는 설정·확장·credential 변경 제어가
없습니다. 권한 축소나 credential 교체는 해당 Team 조회 세션을 무효화합니다.

`usage`에는 기존 Recorder 저장소의 절대 경로를 등록합니다. `read_usage` 권한이 있는
로컬 reader는 시도 시작 시각으로 묶은 기존 집계를 봅니다. Team 관리·조회 credential은
정확한 ID로 연결한 자신의 요청 이력을 조회하며, 전체 팀 조회에는 `read_all_usage`가
필요합니다. Team 범위는 접수 시각 기준이고 원래 canonical attempt를 보존하며
`after`·`next_after`로 다음 페이지를 조회합니다. 대시보드는 해당 범위와 시간 기준을
표시합니다. 누락 counter는 null로 유지하고 전송 EOF와 모델 완료를 구분하며,
producer·request 증거가 없으면 미귀속으로 남깁니다. 비용·quota·청구 결과를 추론하지
않습니다.

## Team credential과 모델 접근

`gateway-team-manager`를 사용하고 비공개 `directory`·`requests` 디렉터리와 별도
numeric loopback `listen`으로 `team`을 선택합니다. `team`·`team-requests`를 명시적으로
초기화합니다. 운영자에게 필요한 Team 관리 grant만 추가합니다. 클라이언트가 보낸 role은
권한을 부여하지 않습니다. 주체 등록·권한 변경·credential 폐기는 같은 감사 dispatcher를
사용하며 Team grant는 이 등록 대상을 지정해야 합니다.

`team_credential_issue`·`team_credential_rotate`는 보호된 동기
`POST /management/v1/credential-delivery` endpoint를 사용합니다. 일반 비동기 제출은
이 명령을 거절하며 브라우저 Origin·cookie를 이용한 전달도 거절합니다. CLI로 새 key를
표준 출력 대신 새 비공개 파일에 받습니다.

```sh
gateway-management-cli --endpoint http://127.0.0.1:48080 \
  --token-env GATEWAY_MANAGEMENT_TOKEN deliver-credential \
  --file issued-submission.json --output new-private-credential-file
```

기존 부모 디렉터리는 비공개이고 link가 없어야 하며 대상 파일은 없어야 합니다. 출력 경로는
서버에 전달하지 않습니다. 원문 credential은 결과가 영속적으로 성공한 뒤에만 전달합니다.
HTTP 클라이언트가 연결을 끊거나 결과 기록이 실패해도 일시적 출력은 소비합니다.
조회·상태 조정·중복 재시도로 다시 표시할 수 없습니다. 전달을 받지 못하면 명시적으로
대체 credential을 선택해야 할 수 있으며 자동 재발급하지 않습니다. 실패 시 로컬 출력
파일이 비어 있을 수 있으므로 다음 행동 전에 작업 증거를 확인합니다.

Team 모델 endpoint는 현재 HTTP Responses·모델 목록 계약을 지원합니다. 모델 key로
관리를 호출하거나 관리·조회 key로 모델을 호출할 수 없습니다. 허용 route로 모델 호출과
목록을 모두 제한합니다. 관리자는 실제 소유 실행을 관측하고 별도의 Gateway 모델·control
credential로 연결합니다. Producer를 귀속하려면 활성 Recorder 저장소가 관측한 확장
연결과 일치해야 합니다. 시간이나 모델 이름으로 사용자를 추측하지 않습니다.

관리형 route의 공개 Team session ID는 주체·route 소유권을 유지하고 내부 Gateway
header·control token 접근을 차단합니다. 선택형 관리 `continuation` adapter는 알려진
내부 session ID에 대해 권한 있는 호스트 정보 조회와 명시적인 `compact_begin`,
`compact_commit`, `recover` 전환을 제공합니다. 별도의 비공개 증거 디렉터리, 현재
revision, 정확한 의도와 Core의 pending 상태 검사가 필요합니다. 도구를 실행하거나
portable 이력을 만들어 내거나 추론을 다시 보내거나 미확정 전환을 재실행하여 복구하지
않습니다.

## 검증·배포물·복구

독립 Web은 고정한 Node 24.21.0/npm 11.19.0으로 빌드합니다. 내보낸 소스 빌드는 검증된
소스 파일 receipt를 사용하며 일반 로컬 빌드는 Git의 dirty 표시를 유지합니다. 자체
일관성이 있는 receipt·hash는 attestation이나 외부 정체성 증명이 아닙니다. 선택형 패키징
절차는 검증된 기본 candidate의 정확한 source archive를 사용하고 해당 소스와 원래
고지를 보존하며 관리·Team feature를 별도로 빌드하고 추출한 배포물을 합성 loopback
provider로 실행합니다.

```sh
python3 -B scripts/optional_package.py build --base .local/base-candidate --output .local/optional-candidate
python3 -B scripts/optional_package.py verify .local/optional-candidate --commit SOURCE_COMMIT --target NATIVE_TARGET
```

기존 기본 [Release 절차](release.md)는 별도로 유지합니다. 선택형 module archive는
정확한 파일, 소스·lock hash, 지원 계약과 의존 조건을 기록합니다. 빌드·fixture 검증은
hosted CI, provider 적합성, 소비자 운영, attestation, 정식 Release와 구분합니다. 이 절차는
서비스를 배포하거나 tag·Release를 발행하지 않습니다.

명시적인 일관된 SQLite 백업 전에는 관리자를 종료합니다.

```sh
gateway-manager --registration registration.json backup --component management --destination new-management-backup.sqlite3
gateway-team-manager --registration registration.json backup --component team --destination new-team-backup.sqlite3
gateway-team-manager --registration registration.json backup --component team-requests --destination new-request-backup.sqlite3
```

연결된 검증 구성 후보, runtime·extension·control 증거 디렉터리와 호환 패키지 바이트도
보존합니다. 실행 중인 SQLite·WAL 파일을 각각 복사하거나, 증거를 덮어쓰거나, 오류를
지우기 위해 데이터를 삭제하거나, 스키마를 암묵적으로 downgrade하지 않습니다. 다시
열 때 완료하지 못한 감사 작업은 명시적 증거 조정 전까지 미확정입니다. 호환되는 검증
실행 파일과 일관된 상태를 복구에 사용하며 완료된 모델 호출을 되돌림 수단으로 사용하지
않습니다.
