<a id="token-usage-accounting"></a>
<a id="usage-accounting"></a>

# 토큰 사용량 계측

[English](../usage-accounting.md) | [한국어](usage-accounting.md)

게이트웨이는 프로토콜 변환 전에 공급자가 보고한 사용량을 추출한다. 선택형
네이티브 Usage Recorder는 SQLite 운영 원장을 커밋하고 저장된 이벤트를
PostgreSQL 또는 HTTP 수집기로 전송한다. 토큰 추정, 청구 계산, 도구 실행,
대화 저장은 수행하지 않는다. 공급자 청구서는 별도의 근거다.

<a id="token-meaning"></a>

## 토큰 의미

`usage.counters`의 각 카운터는 nullable `value`와 `source`를 포함한다. 출처는
`reported`, `derived`, `not_reported`, `not_applicable`, `invalid`다. 누락은
0이 아니다. 전송 스키마는 [UsageEvent](../../schemas/gateway-usage-event-v1.schema.json)를 참조한다.

| 카운터 | 의미 |
|---|---|
| `input_tokens` | 캐시 읽기와 쓰기를 포함한 전체 입력 |
| `output_tokens` | 보고된 추론을 포함한 전체 생성 출력 |
| `total_tokens` | 오버플로와 불일치를 검사한 입력·출력 합계 |
| `input_regular_tokens` | 알려진 캐시 읽기·쓰기 구분에 속하지 않는 입력 |
| `cache_read_input_tokens` | 캐시에서 읽은 입력 부분집합 |
| `cache_write_input_tokens` | 캐시에 기록한 입력 부분집합 |
| `reasoning_output_tokens` | 출력의 추론 부분집합이며 중복 가산하지 않음 |

`cache_write_details`는 제공된 TTL별 내역을 보존한다. `reported`는 허용된
숫자형 사용량 경로와 유효성만 보존하며 공급자의 임의 JSON은 저장하지 않는다.
음수·소수·범위 초과·불일치 카운터는 invalid로 남기고 독립적으로 유효한
필드는 유지한다. Recorder는 정확한 정수 연산을 사용한다.

`responses_v1`, `chat_v1`, `messages_v1` 프로필은 API별 의미를 정의한다.
경로는 일치하는 `usage_profile`을 명시적으로 선택할 수 있으며 불일치는
거절한다. 기본값은 경로 API와 일치한다. 프로필과 이벤트 계약은 Recorder
실행 명세에 결합된다.

Responses는 캐시 읽기·쓰기 상세와 추론을 직접 읽는다. Chat은 캐시 읽기와
추론을 보존하되 미보고 캐시 쓰기는 알 수 없는 값으로 둔다. Messages는 일반
입력·캐시 읽기·캐시 생성·제공된 TTL 상세를 각각 보존한다. Messages 프로필은
캐시 구성값이 생략되면 정규화된 전체 입력을 보수적으로 미확정 상태로 둔다.
기존 클라이언트 변환은 생략된 선택형 캐시 구성값을 기존 전송 합계에 기여하지
않는 값으로 취급한다. 클라이언트 Responses usage에는 지원되는 캐시 상세를
넣으며 원장 전용 필드는 넣지 않는다.

Messages의 일반 입력 10, 캐시 읽기 5, 캐시 쓰기 2는 전체 입력 17이다.
입력에서 캐시 읽기를 빼면 12이며 이는 **캐시 읽기 제외 입력**이다. 엄밀한
캐시 미스로 단정하지 않는다. Chat 입력 10과 캐시 읽기 4에서 읽기 제외 입력은
6이지만, 쓰기 누락만으로 일반 입력을 확정하지 않는다. 출력에서 추론을 뺀
값도 반드시 가시적 텍스트는 아니다. 누적 스트리밍 카운터는 교체하며 1, 3, 8의
최종값은 8이다.

<a id="delivery-guarantees"></a>

## 전달 보장

| 모드 | 동작 |
|---|---|
| `off` | 바인딩의 기본 모드이며 Recorder 프로세스·원장 전달 없음 |
| `best_effort` | 제한된 비차단 큐를 사용하며 이벤트 유실 가능 |
| `durable_local` | 업스트림 전송 전 시작 커밋, 인식한 최종 응답은 로컬 커밋 대기 |

확장 잠금 파일이 없으면 계측은 비활성이다. 명시적인 Recorder 바인딩에는
모드가 필수이며 아래 예시는 `durable_local`을 선택한다.

호출은 producer·request·attempt·event·revision 식별자를 구분한다. 이벤트는
업스트림 결과, 게이트웨이 결과, 사용량 완결성, 관찰 완전성을 각각 보존한다.
사용량 미관찰은 `unobserved`이며 0토큰 성공이 아니다. UTC 시각은 Unix
밀리초다. 인식한 JSON/SSE 최종 응답을 공개하기 전에 종료 스냅샷을 커밋한다.
이는 클라이언트 수신 증거가 아니다. 시작 커밋 실패는 공급자 호출을 막는다.
최종 커밋 실패는 로컬 JSON 오류 또는 이미 시작한 스트림 중단으로 처리한다.
기록 실패 때문에 모델 요청을 재시도하지 않는다.

네이티브 SSE 관찰은 제한된 메모리를 사용하고 원본 바이트를 유지한다.
잘못되거나 지나치게 큰 관찰 프레임만으로 지원되는 네이티브 전달을 중단하지
않고 수집 불완전을 표시한다. 종료를 인식하지 못하면 정상 EOF에서 종료 기록
ACK를 기다리지만 클라이언트 최종 이벤트 이전 커밋은 보장할 수 없다. 전송
제한과 프로토콜 변환 오류 정책은 계속 적용된다. 취소 후 usage 확보를 위해
업스트림을 계속 읽지 않는다. 강제 종료 시 미커밋 관측은 유실될 수 있으며
종료 기록 없이 시작만 남은 호출은 재시작 후에도 결과 불명으로 표시된다.

로컬 커밋은 SQLite 트랜잭션 완료이며 원격 전달이 아니다. SQLite는 WAL,
`synchronous=FULL`, 단일 원장 쓰기 작업자와 busy timeout을 사용한다.
전원 장애의 실제 동작은 파일시스템과 저장 장치에도 달려 있다. 커밋된 outbox
이벤트는 수신 측 중복 제거와 함께 재전송하며 공급자 청구량의 완전한 exactly-once
기록을 주장하지 않는다. 네트워크 대기는 로컬 IPC 쓰기 경로 밖에서 수행한다.

<a id="recorder-installation"></a>

## Recorder 설치

네이티브 기록은 Linux와 macOS에서 지원한다. 일반 게이트웨이는 네이티브
확장 없이도 사용할 수 있다. 별도 실행 파일을 명시적으로 빌드한다.

```sh
cargo build --locked --release -p gateway-usage-recorder
python3 -B scripts/extension_manager.py package \
  --binary "$PWD/target/release/gateway-usage-recorder" \
  --license-file "$PWD/LICENSE" --output "$PWD/.local/recorder-package" \
  --id usage-recorder --version 0.1.0 --role usage_recorder
```

[확장 설치 안내](extensions.md)에 따라 정확한 패키지 다이제스트를 설치한다.
확장 저장소 안의 `usage/<store_id>`에 사용자 소유의 비공개 원장 디렉터리를
준비한다. Recorder의 `init --store` 명령으로 초기화한다. 확장 관리자는
패키지 코드를 실행하지 않는다.

원장 디렉터리에 비공개 `recorder.json`을 만든다.

```json
{"schema":"gateway-usage-recorder-config/v1","destinations":[]}
```

실제 설정 파일 SHA-256으로 비공개 Recorder 바인딩 파일을 만든다.

```json
{"store_id":"primary","mode":"durable_local","queue_capacity":256,"ack_timeout_ms":5000,"config_sha256":"<configuration-file-sha256>"}
```

`enable --recorder-binding`과 세 가지 명시적 권한 `export_usage`,
`observe_usage`, `write_usage_store`로 활성화한다. 패키지 프로토콜은
`gateway-usage-recorder/v1`이며 `--role usage_recorder`로 역할을 선택한다.
Recorder는 하나만 지원한다. 큐는 2–4096칸, ACK 제한은 1–60000밀리초다.
계측이 허용된 호출마다 종료용 슬롯 하나를 예약한다. 이벤트 프레임은 줄바꿈을
제외하고 65536바이트, ACK는 4096바이트로 제한한다.

Recorder 활성화는 `gateway-extension-lock/v2`,
`gateway-extended-manifest/v2`, `gateway-extended-ready/v2`를 사용한다.
소비자는 대응하는 실행 다이제스트를 검사하고 미지원 버전을 거절해야 한다.
Observer 전용 활성화는 기존 계약과 권한을 유지한다. 원장 정체성은 패키지
버전과 독립적이므로 업그레이드해도 저장된 사용량을 유지한다. 이전 실행 파일은
미지원 DB 스키마를 거절하며 마이그레이션은 명시적으로 수행한다.

네이티브 실행 파일은 신뢰된 코드이며 OS 샌드박스가 아니다. 이벤트에 프롬프트,
응답 텍스트, 도구 데이터, 자격증명, 임의 헤더를 넣지 않는다. 게이트웨이는
클라이언트의 테넌트·과금 주체 정보를 신뢰하지 않는다. 소비 호스트가 검증한
실행 문맥을 요청 식별자에 독립적으로 연결한다.

<a id="external-export"></a>

## 외부 전송

목적지는 최대 8개다. PostgreSQL은 `initialize-postgres --destination`으로
명시적으로 초기화한 전용 `gateway_usage` 스키마를 사용한다. 다른 애플리케이션의
내부 테이블에 쓰지 않는다. 목적지 설정에는 `kind`, `id`와 함께 HTTP의 `url`,
`bearer_file` 또는 PostgreSQL의 `connection_file`, 선택형 `tls_ca_file`을 쓴다.
비밀 파일은 Recorder가 직접 읽는 비공개 일반 파일이다. 상속 환경은 비운다.
PostgreSQL은 검증된 TLS를 요구하며 선택형 비공개 CA 파일로 명시적으로 신뢰한
인증서를 지원한다.

HTTP는 숫자형 loopback 테스트 주소를 제외하고 HTTPS를 요구한다. 리디렉션,
상속 프록시, URL 자격증명·쿼리·fragment는 거절한다. 설정 URL은 수집기의 전체
주소이며 비공개 호스트 전용 경로를 가정하지 않는다.

요청은 `gateway-usage-batch/v1`, Bearer 인증을 사용하며 배치당 최대 100개
이벤트 또는 1 MiB다. 200 응답은 `gateway-usage-batch-receipt/v1`과 이벤트별
정확히 하나의 receipt를 포함해야 한다. 필드는 `producer_id`, `event_id`,
`sha256`, `status`다. 상태는 `committed`, `duplicate`, `conflict`, `rejected`다.
receipt는 메모리 큐 접수가 아닌 영속 저장을 확인한다. 202 응답은 커밋 ACK가
아니다. [배치 스키마](../../schemas/gateway-usage-batch-v1.schema.json)를 참조한다.

이벤트 다이제스트는 객체 키를 정렬하고 공백 구분자·마지막 줄바꿈을 제거한
UTF-8 JSON을 대상으로 한다. 전체 정수 범위를 보존해야 한다. 같은 식별자와
내용은 중복이며 내용이 다르면 충돌이다. 늦은 이전 revision은 최신 상태를
덮어쓸 수 없다. PostgreSQL도 같은 규칙을 트랜잭션으로 적용한다.

일시 장애·불확실한 ACK는 제한된 지수 백오프로 동일 이벤트를 재전송한다.
대부분의 HTTP 클라이언트 오류는 전달을 보류하며 408·429는 재시도한다.
충돌은 표시 상태로 유지한다. 목적지 문제를 수정한 뒤 `retry-blocked --destination`을
사용한다. 목적지 ID는 설정에 결합되며 설정 변경은 거절한다. 대기 이벤트가
있는 목적지 제거도 거절한다. 새 ID에는 이후 이벤트만 전달한다. 원격 진단
원문과 비밀은 로그에 기록하지 않는다.

<a id="queries-and-maintenance"></a>

## 조회와 유지보수

Recorder CLI는 `status`, `query`, `export`, `aggregate`, `backup`, `migrate`,
`prune`, `flush`, `retry-blocked`를 제공한다. 활성화된 원장 디렉터리에서 실행하는
`serve`를 제외하면 `--store`를 받는다. 읽기 전용 조회는 실행 중에도 가능하며
유지보수·수동 전송은 쓰기 작업자를 중지한 뒤 수행한다.

`query --attempt`는 호출을 선택하고 `--limit`은 1000으로 제한한다. `export`는
`--after-rowid`용 커서와 불변 이벤트를 반환한다. `aggregate --from-ms --to-ms
--timezone`은 호출 시작 시각에 귀속된 반개구간을 사용하며 기본 UTC와 IANA
시간대를 지원한다. 정확한 합계와 함께 필드 관측 수, 최종·부분·미관측 호출,
미종료 호출, 캐시 비율의 관측 범위를 반환한다. 캐시 읽기 비율에는 입력과
읽기가 모두 알려진 호출만 포함한다. 분모가 없거나 0이면 null이다. 비용
계산은 구현하지 않는다.

`backup --output`은 덮어쓰기를 거절한다. `migrate --backup`은 현재 스키마를
검증하고 백업하며 첫 스키마에는 이전 마이그레이션이 없다. `prune --before-ms`는
외부 전달 대기가 없는 종료 호출만 제거한다. 자동 만료는 비활성이다.
유지보수와 패키지 롤백 전에 백업을 보존한다.

합성 검사는 `scripts/usage_smoke.py`, `scripts/usage_postgres_test.py`를 사용한다.
후자는 격리된 TLS PostgreSQL을 소유하고 검사 후 종료한다. 합성 conformance나
로컬 검사 통과는 실제 공급자 청구 정확성 또는 외부 호스트의 운영 수락을
증명하지 않는다.
