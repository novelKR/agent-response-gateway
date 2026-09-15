<a id="versioned-usage-provenance"></a>

# 버전이 있는 사용량 출처

[English](../usage-provenance.md) | [한국어](usage-provenance.md)

사용량 이벤트 버전은 공급자 의미를 해석한 주체를 구분합니다. 기존 내장 어댑터는
`gateway-usage-event/v1`을 계속 생성하며 원래 정규화, canonical 바이트, 이벤트 hash와
ACK 의미를 보존합니다. `gateway-usage-event/v2`는 명시적으로 신뢰한 provider 플러그인의
숫자 해석을 기록하며 내장 파서 검증으로 표시하지 않습니다. 명시적으로 선택한 provider
경로는 연속성과 기록 장벽을 통과하며 이 정체성을 보존합니다. Recorder v2 패키지 설치가
provider 경로를 활성화하지는 않습니다.

[이벤트 스키마](../../schemas/gateway-usage-event-v2.schema.json)와
[Recorder 스키마](../../schemas/gateway-usage-recorder-v2.schema.json)는 공개 wire를 설명합니다.
[독립 Recorder 예제](../../tools/plugin-conformance/examples/recorder/README.md)는 게이트웨이를
import하지 않는 패키지 제작과 네이티브 IPC를 보여 줍니다.

<a id="evidence-and-event-identity"></a>

## 근거와 이벤트 식별

V2는 호스트가 작성하는 공통 producer, request, attempt, event, revision, provider/model,
configuration, 시간, outcome, finality 필드를 유지합니다. V1의 `profile`을 필수
`interpretation`으로 대체하며 `kind: trusted_provider_plugin`, `protocol: gateway-provider/v1`,
`provider_protocol`, `package_id`, `package_version`, `package_sha256`, `executable_sha256`을
포함합니다. 호스트는 정확히 검사한 선택에서 정체성을 복사합니다. 플러그인 응답은
producer ID, 패키지 식별, 자격증명, commit이나 전송 결과를 지정할 수 없습니다.

Canonical 숫자 counter는 `reported`, `derived`, `not_reported`, `not_applicable`, `invalid`
출처를 유지합니다. 잘못된 공급자 숫자나 산술 위반은 타입이 있는 invalid 근거가 되며,
거절한 원본 값이나 오류 텍스트를 저장하지 않습니다. 정상인 다른 숫자는 유지합니다.
Unknown은 0이 아니며 invalid 관측으로 성공 종료나 도구 출력을 승인할 수 없습니다.
V2 `reported`는 빈 객체이고 `cache_write_details`는 빈 배열입니다. 공급자별 원본 사용량
경로나 cache TTL bucket은 provider v1 해석 범위 밖입니다.

모든 이벤트는 바이트 식별에 끝 LF를 포함하지 않는 canonical UTF-8 JSON입니다.
IPC는 정확히 하나의 LF를 추가합니다. Commit ACK는 `type`, `event_id`, `sha256`을 포함하며
LF를 제외한 원래 canonical 이벤트 바이트를 hash합니다. V1·V2는 공유 ledger에서도 별도
버전으로 유지됩니다. 기존 행 재직렬화, hash 교체나 기존 attempt의 암묵적 해석 변경은 없습니다.

<a id="recorder-package-and-startup-compatibility"></a>

## Recorder 패키지와 시작 호환성

Recorder v2는 패키지 v2, `gateway-usage-recorder/v2`, `usage-store/v2`와 기존
`export_usage`, `observe_usage`, `write_usage_store` 권한을 사용합니다. 필수 기능 객체는
정확히 다음과 같습니다.

```json
{"schema":"gateway-plugin-capabilities/v1","apis":[],"features":["usage_event_v1","usage_event_v2"],"requires":["usage_recorder_ipc_v2"]}
```

이 기능 이름은 Recorder 역할에만 속하며 배열은 정렬되고 중복되지 않습니다.
Ready는 `type: ready`, `protocol`, `producer_id`와 함께 정확한 선언을 반복합니다.
고정 실행 명령은 `serve-v2`이며 기존 `serve`는 Recorder v1을 유지합니다.
Manifest 수정만으로 실행 파일을 업그레이드할 수 없습니다.

```sh
python3 -B scripts/extension_manager.py package \
  --binary /absolute/path/recorder --license-file /absolute/path/LICENSE \
  --output /absolute/new-package --id recorder --version 1.0.0 \
  --role usage_recorder --recorder-protocol gateway-usage-recorder/v2 \
  --capabilities /absolute/path/recorder-capabilities.json
```

정적 inspect/install은 코드를 실행하지 않습니다. 시작 시 정확한 Ready 기능 호환성을
검사합니다. 활성 Recorder v1은 선택한 provider 경로와 호환되지 않으므로 출처를 버리는
대신 모델 작업 전에 해당 조합을 거절합니다. 네이티브 실행은 같은 사용자 권한의 신뢰
실행이며 OS 샌드박스가 아닙니다. 설치는 언어 runtime이나 의존성을 다운로드하지 않습니다.

Recorder v2 선택은 `gateway-extension-configuration/v5`와 외부
`gateway-extended-manifest/v11` / `gateway-extended-ready/v11`을 사용합니다.
외부 구성은 두 이벤트 버전을 포함하는 `usage_event_schemas`를 광고하며 모든 이벤트를
V2로 표시하지 않습니다. `usage_profiles`는 계속 내장 V1 프로파일 목록입니다. 기존 선택은
이전 스키마와 단일 `usage_contract`를 유지합니다. 설치됨·선택됨·관측된 실제 적용 상태는 구분합니다.

<a id="storage-export-and-recovery-boundaries"></a>

## 저장·내보내기·복구 경계

Ledger는 정확한 이벤트 payload와 hash를 저장합니다. 기본 제공 구현은 살아 있는 중복 행에 ACK를 보내거나 이벤트를 내보내기·조회로 반환하기 전에 원래 저장 hash와 canonical 바이트를 확인합니다. 불일치하면 행을 다시 쓰거나 대체 hash를 부여하지 않고 실패합니다. 보존 기간 정리로 남긴 tombstone은 기존 hash 전용 중복 의미를 유지합니다. V2 이벤트 해석에 새 이벤트 열이나
V1 행의 일괄 변환은 필요하지 않습니다. 저장 호환성은 명시적으로 검사해야 하며 새 이벤트
계약이 구버전 바이너리를 위한 데이터베이스 marker 하향이나 기존 payload 재작성 권한을
뜻하지 않습니다. 문서화된 호환성 업그레이드 전에 원래 데이터베이스와 backup을 보존하십시오.

준비한 전용 디렉터리에서 새 V2 ledger를 초기화하거나, 기존 writer를 중지한 뒤 새 backup 파일을 지정하여 명시적으로 업그레이드합니다.

```sh
gateway-usage-recorder init-v2 --store /absolute/new-private-ledger
# For an existing ledger, stop its writer first and choose a new backup file:
gateway-usage-recorder upgrade-v2 --store /absolute/existing-private-ledger --backup /absolute/private-backups/before-v2.sqlite3
```

업그레이드는 기존 이벤트 행을 바꾸지 않고 SQLite `user_version`을 2로 설정하는 호환성 guard입니다. 구버전 writer는 이를 거절합니다. 복구할 때 새 경로를 비활성화하고 호환 바이너리·패키지를 선택하거나 업그레이드 전 backup을 별도로 복원하십시오. V2 데이터가 있는 저장소의 marker를 1로 낮추면 안 됩니다.

Recorder 설정 스키마는 `gateway-usage-recorder-config/v1`을 유지합니다. V2 HTTP 목적지는 `kind: http_v2`를 명시하고 기존 `id`, `url`, `bearer_file` 필드를 사용합니다. Wire envelope는 `gateway-usage-batch/v2`와 `gateway-usage-batch-receipt/v2`이며 이벤트별 hash는 원래 이벤트 바이트를 계속 식별합니다.

V1 HTTP·PostgreSQL 내보내기는 기존 계약을 유지합니다. Provider V2 내보내기에는 명시적으로
선택한 V2 HTTP 목적지와 V2 batch/receipt 지원이 필요합니다. 기존 HTTP·PostgreSQL v1 목적지는
미지원 V2 전송을 전송 전에 거절하거나 차단하며 outbox와 출처를 보존합니다. 자동 PostgreSQL
DDL migration이나 V2 PostgreSQL 지원을 뜻하지 않습니다. 잘못되거나 미지원인 receipt를
성공 내보내기로 처리할 수 없으며, 원격 receipt와 로컬 durable commit ACK는 별개입니다.

관리·team 소비자는 이벤트 버전과 해석 주체를 이해해야 합니다. 같은 provider/model 이름으로
집계하면서 이 차이를 삭제하면 안 됩니다. 보고서는 invalid/missing 관측, 미완료 attempt,
로컬 commit과 export 상태를 구분해야 합니다. 네이티브·컨테이너 검사, 합성 provider fixture,
실제 공급자 자격 검증은 서로 다른 근거입니다.

Team 조회는 `gateway-team-http/v1` envelope를 유지합니다. V2 attempt가 하나라도 있으면
`usage_event_schemas`는 정확히 `["gateway-usage-event/v1","gateway-usage-event/v2"]`여야 합니다.
V1만 있는 응답에는 해당 marker를 추가하지 않습니다. Web 소비자는 이벤트 종류·정체성·숫자
형식과 marker를 검증하고, 페이지를 합칠 때 어느 페이지에 V2가 있든 marker를 유지합니다.
원래 요청 record와 attempt 객체는 변경하지 않습니다.

HTTP receiver 작성자는 [V2 batch 스키마](../../schemas/gateway-usage-batch-v2.schema.json)와
[V2 receipt 스키마](../../schemas/gateway-usage-batch-receipt-v2.schema.json)를 함께 사용하십시오.
스키마만으로 원래 이벤트 hash와 정확히 대응하는 전체 receipt 집합을 검증할 수는 없습니다.

<a id="external-recorder-state-and-query-support"></a>

## 외부 Recorder 상태와 조회 지원

외부 Recorder는 이벤트/ACK 계약을 구현하면서 자체 비공개 저장소를 사용할 수 있습니다.
게이트웨이는 Ready에서 producer 식별을 얻으며 Recorder 시작 경로에서 플러그인 DB를
열지 않습니다. 관리·team 조회 backend는 기본 제공 Recorder의 SQLite 구조만 읽는 별도
구현입니다. Recorder IPC 호환성과 `usage-store/v2`는 해당 조회 구조의 호환성을 선언하거나
일반 조회 프로토콜을 제공하지 않습니다.

독립 예제에서는 먼저 전체 패키지를 복사·빌드하고 출력 digest를 검사합니다. 아래에는
절대 경로 `EXTENSION_STORE`, `PACKAGE_DIR`와 검토한 `PACKAGE_SHA256`을 설정합니다.
설치 후 초기화 전에 store의 usage 상위 디렉터리와 usage/independent를 mode 0700으로
만듭니다. usage/independent 안에 다음 빈 export 설정의 recorder.json을 mode 0600으로 둡니다.

```json
{"schema":"gateway-usage-recorder-config/v1","destinations":[]}
```

다음 객체를 비공개 binding 파일로 작성합니다. Hash placeholder는 recorder.json의 정확한
바이트 digest로 바꾸며 끝 newline이 있다면 포함합니다. 선택 중에는 설정을 변경하면 안 됩니다.

```json
{"store_id":"independent","mode":"durable_local","queue_capacity":256,"ack_timeout_ms":5000,"config_sha256":"<SHA-256 of the exact recorder.json bytes>"}
```

```sh
python3 -B /absolute/tools/extension_manager.py install --store "$EXTENSION_STORE" --package "$PACKAGE_DIR" --expected-sha256 "$PACKAGE_SHA256"
(cd "$EXTENSION_STORE/usage/independent" && "$EXTENSION_STORE/packages/synthetic-recorder/1.0.0/$PACKAGE_SHA256/extension" init)
python3 -B /absolute/tools/extension_manager.py enable --store "$EXTENSION_STORE" --id synthetic-recorder --version 1.0.0 --package-sha256 "$PACKAGE_SHA256" --grant export_usage --grant observe_usage --grant write_usage_store --recorder-binding /absolute/private-recorder-binding.json
```

예제의 고정 init 명령은 패키지 디렉터리나 무관한 scratch 디렉터리가 아닌 실제로 선택한
usage 디렉터리에서 실행합니다. 초기화는 명시적 준비 작업이며 install·enable이 자동 실행하지
않습니다. 이후 호스트는 같은 usage 디렉터리에서 설치된 실행 파일을 serve-v2로 실행합니다.
패키지에 포함된 스키마·소스 resource를 모두 유지해야 합니다. 이 명령은 소스로 확인한 준비
절차이며 호스트 수용 실행이 이미 완료됐다는 주장은 아닙니다.

이 예제에서는 관리 애플리케이션의 선택적 usage 디렉터리 설정을 생략하십시오.
예제의 events.sqlite3와 metadata producer 열은 기본 제공 usage.sqlite3 스키마와 의도적으로
다릅니다. 이를 기본 SQLite 조회 backend로 선택하면 명시적으로 실패해야 하며 count 생성,
대체 DB 초기화, 저장 이벤트 재작성이나 모델명에 따른 귀속 추론을 해서는 안 됩니다.
호환 조회 backend를 설정하지 않으면 관리 사용량은 사용할 수 없고 team 귀속은 상황에 따라
unobserved 또는 unattributed로 남습니다. 이벤트 기록은 독립적으로 동작할 수 있습니다.
외부 구현이 문서화된 기본 SQLite 구조를 별도로 구현할 수는 있지만 별도 조회 호환성 검증이 필요합니다.
