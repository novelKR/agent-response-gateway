<a id="gemini-interactions-host-integration"></a>
<a id="configuration"></a>
<a id="support-and-usage"></a>
<a id="host-control-and-resume"></a>
<a id="backup-and-failure-recovery"></a>

# Gemini Interactions 호스트 연동

[English](../interactions.md) | [한국어](interactions.md)

검증된 조합은 호스트 관리 Codex 0.154.0과 모의 Gemini Interactions이다.
Responses 요청은 검증된 continuation과 공통 IR을 거친다. 실제 Gemini 모델의
호환성과 품질은 별도 qualification이 필요하며 일반 CLI/Desktop 직접 연결은
검증 범위가 아니다. 게이트웨이는 도구를 실행하지 않는다.

## 설정

모델·프로필과 Google API 키 인증을 명시한다. API prefix는
https://generativelanguage.googleapis.com/v1이며 요청 경로에 /interactions를 붙인다.
고정된 [v1 계약](../../tests/interactions/wire-lock.json)에 v1beta를 혼용하지 않는다.
다음 예제는 실행 전에 호스트가 모델과 저장소 정체성을 지정해야 한다.

```toml
listen = "127.0.0.1:0"
[providers.google]
base_url = "https://generativelanguage.googleapis.com/v1"
api_key_env = "ARG_GOOGLE_KEY"
[models."example/gemini"]
provider = "google"
upstream_model = "CHOOSE_MODEL"
api = "gemini_interactions"
auth = "google_api_key"
capability_profile = "gemini"
[capability_profiles.gemini]
version = "1"
provider = "google"
upstream_model = "CHOOSE_MODEL"
api = "gemini_interactions"
context_window = 32768
max_output_tokens = 1024
tested_codex_version = "0.154.0"
[capability_profiles.gemini.support]
instructions = "native"
instruction_hierarchy = "bridged_gemini_instruction_envelope"
function_tools = "native"
custom_tools = "bridged_custom_tool_json"
custom_grammar = "bridged_codex_patch_grammar"
namespaced_tools = "bridged_tool_namespace"
tool_choice = "native"
parallel_tool_control = "native"
max_output_tokens = "native"
reasoning_effort = "native"
structured_output = "native"
strict_structured_output = "native"
[continuation]
directory = "/ABSOLUTE/PRIVATE/DIRECTORY"
store_id = "COPY_INITIALIZED_STORE_ID"
realm = "host-realm"
generation = "1"
key_id = "stable-key-1"
key_env = "ARG_CONTINUATION_KEY"
control_token_env = "ARG_CONTROL_TOKEN"
max_store_bytes = 1073741824
```

호스트가 비공개 디렉터리를 준비한다. Unix에서는 0700 권한, Windows에서는
소유자 전용 ACL을 사용한다. symlink 구성 요소가 없는 네이티브 절대 경로를
사용하며 초기화는 한 번만 실행한다.

```sh
agent-response-gateway init-continuation --directory /ABSOLUTE/PRIVATE/DIRECTORY
```

반환된 store_id를 설정에 넣는다. 호스트는 독립적으로 생성한 안정적인 256-bit
키를 소문자 hex 64자로 key_env에 공급한다. key_id와 키 값은 보호된 호스트
저장소에서 재시작 후에도 유지하며 실행할 때마다 다시 생성하지 않는다.
별도의 32–4096자 printable ASCII 제어 토큰, Codex용 로컬 토큰, Google 키를
공급한다. 비밀 값은 모두 달라야 한다. 기존 api_key 인증은 계속 x-api-key를
뜻하며 google_api_key는 x-goog-api-key를 뜻한다. Codex에는 로컬 토큰과 세션
헤더만 전달한다.

활성화 시 manifest/readiness schema는 gateway-embedded-manifest/v3와
 gateway-ready/v3이다. Codex 실행 전 오프라인 설정 digest와 비교한다.
구버전 호스트는 모르는 계약을 거부해야 한다. manifest에는 설정·저장소 정체성,
wire digest, 프로필·어댑터 버전이 포함되지만 실제 제공자 호출을 증명하지 않는다.
현재 observer/continuation manifest를 함께 사용하는 모드는 거부한다.

## 지원 범위와 사용량

| 기능 | 계약 |
|---|---|
| 텍스트 JSON/SSE | 검증된 출력 변환; 원본 provider steps는 별도 보존 |
| 함수와 병렬 호출 | 이름·call ID·개수·스키마·결과 연결 검사 |
| Custom 텍스트·namespace·patch grammar | 기존 JSON/이름 bridge와 등록된 grammar |
| 지시 | 선행 system/developer 역할·순서를 명시적인 지시 envelope에 보존; 중간 지시 거부 |
| 출력 스키마 | object/array/scalar, enum, required, properties, items, anyOf, additionalProperties의 보수적 부분집합; 출력 검증 |
| Thinking | minimal, low, medium, high; 알 수 없는 effort 거부 |
| 병렬 비활성화 | 호출 가능한 도구가 있으면 거부; 도구가 없으면 수락 |
| 엄격한 함수 인자 | 고정 함수 계약에서 strict:true를 표현하지 못하므로 거부 |
| Hosted tools와 멀티모달 입출력 | 이미지·음성·영상·검색·코드 실행을 포함해 거부 |
| 제공자 저장/background | store:false와 background:false 명시 |
| Requires action | 완료된 Responses 도구 항목; 세션에는 도구 대기 상태 유지 |
| 불완전하거나 중단된 출력 | 성공 확정·자동 재시도 없음; 호스트 정리 필요 |

종료 이벤트의 usage를 우선한다. 없으면 step delta metadata나 step stop의 마지막
누적 usage를 사용하며 누적 카운터와 단계별 카운터를 합산하지 않는다.
usage의 input_tokens는 total_input_tokens이다. output_tokens는
 total_output_tokens와 total_thought_tokens의 합이며 둘 중 하나라도 없으면
알 수 없는 값으로 둔다. total_tokens는 보존하고 모든 카운터가 있으면 합과
비교한다. total_cached_tokens는 입력의 부분집합이며 다시 더하지 않는다.
reasoning 토큰도 별도 표시한다. 누락 카운터는 null이며 0이 아닌 hosted-tool
사용량은 거부한다. 같은 토큰·effort 이름이 같은 모델 비용을 뜻하지는 않는다.

## 호스트 제어와 재개

제어 경로는 제어 토큰의 Bearer 인증을 사용하며 모델 API와 분리된다.
세션 누락은 오류이며 세션을 새로 만들라는 지시가 아니다.

| 메서드와 경로 | 본문 또는 결과 |
|---|---|
| POST /__continuation/sessions | route, realm, generation을 포함한 origin; id, epoch, revision, status 반환 |
| GET /__continuation/sessions/{id} | 현재 메타데이터와 pending_tools; provider payload 없음 |
| POST /__continuation/sessions/{id}/transitions | revision, kind, portable_sha256, decision_reference, pending_tools:false, pending_approvals:false |

선택한 manifest 경로에서 api_key_env를 제외해 origin.route를 만든다.
credential realm/generation은 환경변수 이름이나 키 값이 아닌 호스트 정체성이다.
전용 Codex provider의 고정 http_headers에 x-gateway-session으로 세션 ID를 넣는다.
Responses wire_api를 사용하며 transport retry를 비활성화한다.
제어 토큰을 Codex 환경이나 설정에 넣지 않는다.

일반 재시작에는 같은 세션·보호 키·저장소·경로·Codex 이력이 필요하다.
각 응답은 새로 생성한 provider steps만 암호화해 reasoning.encrypted_content에
넣는다. 다음 요청에서 원래 입력 prefix와 대응하는 공개 출력을 인증한다.
공개 텍스트·도구 항목으로 누락된 provider signature를 대체하지 않는다.

로컬 압축 전에 호스트가 도구·승인 대기가 없음을 확인하고 compact_begin 전이를
보낸다. 이어서 thread/compact/start를 호출하고 완료·요약·완료 도구 결과를
검증한다. 기존 작업을 보존한다. 검증된 이동 가능 맥락을 새 Codex 작업의 사용자
메시지 하나에 넣고 그 digest로 compact_commit하여 같은 세션의 새 epoch를
활성화한다. 이 명시적 이동은 Codex의 중간 developer 재삽입을 피하며 제거된
signature를 보존하지 않는다. 관측하지 못한 자동 압축이나 설명되지 않는 이력
변경은 검증에 실패한다.

portable_sha256는 해당 사용자 메시지 하나를 담은 JSON 배열의 hash이다.
객체 키를 정렬한 compact UTF-8 JSON을 사용한다. 생략된 메시지 type은 message로
맞추고, 항목의 id/status, null인 phase/internal_chat_message_metadata_passthrough,
비어 있는 출력 텍스트 annotations를 제거한다. 텍스트나 도구 내용은 정규화하지
않는다. 새 맥락에는 Codex의 선행 지시·환경 메시지가 포함될 수 있다. 결합한
사용자 메시지는 정확히 한 번 있어야 하며 과거 assistant/tool/provider 항목을
직접 가져올 수 없다. [실행 가능한 합성 호스트](../../tests/codex/interactions_continuity.py)를 참고한다.

## 백업과 실패 복구

SQLite는 WAL, synchronous=FULL, foreign keys, revision 검사와 독점 소유권을
사용한다. blocking worker에서 DB·암호화 작업을 처리한다. 초기화는 명시적으로
수행하며 serve는 누락·손상 DB를 빈 DB로 대체하지 않는다. 자동 migration·만료·
퇴출·fallback·추론 재시도는 없다. 각 attempt마다 max_response_bytes를 예약하며
max_store_bytes가 예약 합계와 DB page 수를 제한한다. WAL과 호스트 백업에는
추가 파일시스템 공간이 필요하다. 암호화 전 payload는 2 MiB로 제한하며 입출력
byte 제한은 envelope와 변환 SSE에도 적용한다. 한도 오류로 상태를 조용히 자르지
않는다.

일관된 백업을 위해 Codex 작업과 게이트웨이를 중지한 뒤 DB, 존재하는 WAL/SHM
sidecar, 저장소 메타데이터, 설정, 안정적인 키/key ID와 Codex 이력을 함께 보존한다.
모든 사본을 비공개로 유지한다. 실행 중인 main DB 파일 하나만 복사하지 않는다.
서버를 중지한 상태에서 상호 호환되는 조합을 복원한다. lock 파일은 실행 증거가
아니다. Windows 디렉터리 ACL은 호스트가 보장하며 Unix 비공개 권한은 로컬에서
검사한다. 키·본문·signature·envelope는 로그에 남기지 않는다.

finalized 실행 기록이 있고 암호화 payload만 없으면 일치하는 Codex envelope로
모델 호출 없이 복원할 수 있다. 기록이 없거나 attempt가 pending/unknown이면
자동 사용을 차단한다. 호스트가 이전 실행을 조사하고 도구·승인 대기가 없음을
확인한 뒤 새 이동 가능 메시지 digest와 recover 결정을 기록하고 새 epoch/작업을
시작한다. 이는 이전 attempt의 완료를 만들어 내지 않는다. 키 유실·변경이나
비호환 schema는 명시적으로 실패한다. rollback에는 호환되는 바이너리·DB·키·
Codex 이력 조합이 함께 필요하다.

ContinuationStore 계약은 backend 독립적이지만 SQLite만 구현했다.
PostgreSQL, Redis, 공개 Responses 저장·조회·삭제와 previous_response_id는 지원
범위 밖이다. 게이트웨이/provider 연결 종료가 제공자 작업 취소나 무과금을
증명하지는 않는다.

공통 managed 실행 계층은 타입이 구분된 원본 payload와 완료 상태를 가진
gateway-continuation/v2 기록을 쓴다. gateway-continuation/v1 기록은 원래
finalized digest로 인증·검증한 뒤 변환한다. 기존 Gemini 경로 binding, DB 테이블,
키와 이력은 유효하며 자동 migration이나 암호문 재작성은 하지 않는다. manifest의
replay_versions는 read [1, 2]와 write 2를 선언한다. 구버전 호스트는 새 manifest를
거부하며 구버전 바이너리가 v2 기록을 읽는다고 보장하지 않는다. rollback에는
호환되는 바이너리·DB·키·Codex 이력 조합이 필요하다. 공개 reasoning summary가
있으면 원본 제공자 상태와 구분해 인증된 이력 digest에 포함한다. 관리형 Messages는 명시적 Claude reasoning 계약을 사용한다. Chat reasoning은
명시적 계약 구현 전까지 활성화되지 않는다.
