<a id="managed-reasoning-and-continuation"></a>
<a id="execution-and-display"></a>
<a id="host-configuration-and-compatibility"></a>
<a id="recovery-and-compaction"></a>
<a id="usage-recording-and-failures"></a>
<a id="validation-and-limits"></a>

# 관리형 reasoning과 재개

[English](../managed-continuation.md) | [한국어](managed-continuation.md)

관리형 실행 계층은 Gemini Interactions, Claude Messages의 Adaptive/Manual
thinking, 명시적 DeepSeek/OpenRouter Chat 계약을 지원한다. Messages와 Chat의
기본값은 stateless다. DB 설정만으로 reasoning이 활성화되지는 않는다. 모델에
continuation_mode="managed", capability profile에 reasoning_contract를 지정한다.
Gemini는 기존 managed 기본 동작을 유지한다.

## 실행과 표시

요청은 Responses 입력 검증, 세션·이력 검증, Request IR, 제공자 어댑터, 원본
응답 조립, 암호화와 영속 확정, Responses 완료 순서로 진행한다. 공통 실행기는
attempt, revision 충돌, 용량 예약, 복구와 출력 공개 시점을 관리한다. 어댑터는
wire 변환, 원본 재생, 공개 출력과 종료 판정을 담당한다.

공개 thinking 텍스트는 Codex reasoning summary로 표시한다. 서명된 원본,
redacted 데이터와 암호화 상세는 제공자 상태로 유지한다. 공개 summary로 원본
상태를 재구성하지 않는다. 인증된 envelope 하나에는 응답 하나에서 새로 생긴
원본 상태만 담는다. 공개 summary와 일반 출력은 Codex 이력에 대해 함께 인증하며,
원본을 재생할 때 대응하는 공개 이력 구간을 정확히 한 번 대체한다.

텍스트와 reasoning delta는 저장 완료 전에도 스트리밍할 수 있다. 실행 가능한
도구 완료, 복구 envelope와 성공 terminal은 저장 확정까지 기다린다. 제공자가
도구를 요청하면 모델 실행은 종료됐지만 호스트의 도구 작업은 아직 대기 상태다.
게이트웨이는 도구를 실행하거나 승인하지 않는다.

## 호스트 설정과 호환성

[저장소·키·세션 설정](interactions.md#configuration)과
[호스트 제어 API](interactions.md#host-control-and-resume)를 사용한다. 제공자 설정은
[Messages](messages.md#managed-thinking)와
[Chat](chat-completions.md#managed-reasoning) 안내를 따른다. 전용 Codex provider가
호스트에서 만든 x-gateway-session을 고정 HTTP 헤더로 보낸다. 미등록 세션은 거부한다.
Codex에는 로컬 게이트웨이 토큰과 세션 ID만 전달한다. 제어 토큰, 제공자 자격 증명과
독립된 안정적 256-bit 암호화 키는 호스트에 둔다.

단독 관리형 manifest는 gateway-embedded-manifest/v3, readiness는 gateway-ready/v3다.
replay_versions에 read [1, 2], write 2를 선언한다. 관리형 설정과 확장을 함께 켜면
gateway-extended-manifest/v3와 gateway-extended-ready/v3를 사용한다. 중첩된 gateway
manifest, 설정·실행 digest, 지원 replay 버전과 usage profile을 검증한다. 구버전
호스트는 모르는 버전을 거부해야 한다. manifest는 설정 계약이며 제공자 qualification의
증거가 아니다.

기존 Gemini v1 기록은 원래 직렬화 형식으로 인증하고 원래 finalized digest를
검증한 뒤 내부 형식으로 변환한다. 새 응답에는 ReplayV2를 쓰며 기존 암호문과
Codex 이력을 보존한다. Gemini 경로 binding과 SQLite 테이블은 바뀌지 않는다.
자동 migration은 수행하지 않는다. Messages/Chat 계약은 신규 경로 binding에
포함되므로 제공자·모델·계약을 바꾸려면 새 세션이 필요하다.

## 복구와 압축

같은 DB, 안정적 키와 key ID, Codex 이력, 호스트 binding으로 재시작한다. DB가
기준이다. finalized 기록이 있고 payload만 없을 때 같은 인증 binding과 digest를
가진 envelope로 자동 복원할 수 있다. 실행 기록 유실과 pending/unknown attempt는
모델 호출을 차단한다. 호스트가 도구 결과를 확인하고 명시적으로 새 epoch를 활성화해야
한다. 복구 전이는 이전 attempt의 완료를 만들어 내지 않는다.

호스트는 thread/compact/start 전에 압축 전이를 등록한다. 원래 Codex 작업을 보존하고,
완성된 이동 가능 summary와 완료 도구를 검증한 뒤 이동 가능한 맥락을 새 작업으로
옮기고 같은 세션의 새 epoch를 활성화한다. 대기 중인 도구·승인이 있으면 전이를
차단한다. 제거된 제공자 signature가 보존됐다고 주장하지 않는다. 관측되지 않은
자동 압축이나 설명되지 않는 이력 변경은 일반 재개로 받아들이지 않는다.

continuation DB와 SQLite 부속 파일을 복사하기 전에 게이트웨이를 종료한다. 대응하는
키와 key ID, 호스트 binding, Codex 이력을 호환되는 복구 묶음으로 백업하고 디렉터리
권한을 보존한다. serve는 유실·손상된 저장소를 빈 DB로 대체하면 안 된다. 키 유실,
origin 변경과 미지원 schema는 명시적으로 실패한다. rollback에는 호환되는 바이너리와
복구 묶음이 필요하며 구버전 바이너리가 v2 기록을 읽는다고 보장하지 않는다. 자동 TTL
삭제는 없으며 용량이 부족하면 제공자 전송 전에 새 작업을 거부한다.

## 사용량 기록과 실패

선택형 [Usage Recorder](usage-accounting.md)는 별도 원장에 수치 메타데이터를 저장한다.
제공자 steps, reasoning 텍스트, signature나 envelope는 전달받지 않는다. continuation은
회계 저장·내보내기·도구 회계를 소유하지 않는다. DeepSeek은 deepseek/v1, OpenRouter는
chat/v1, Messages는 messages/v1, Gemini는 gemini_interactions/v1을 사용한다.
미보고 카운터는 알 수 없는 값으로 유지한다. 이미 thinking을 포함한 카운터에 이를
다시 더하지 않는다.

durable_local에서는 recorder의 접수 확인이 continuation attempt 생성과 제공자
전송보다 먼저다. 제공자 응답을 검증한 뒤 continuation을 먼저 확정하고 recorder의
최종 로컬 저장 확인을 받은 후 실행 가능한 완료 출력을 공개한다. recorder 접수 실패의
제공자 호출은 0회다. recorder 최종 저장이 실패하면 continuation에는 finalized 기록이
있지만 클라이언트에는 완료가 없을 수 있다. 두 저장소를 보존하고 작업 상태를 확인한
뒤 계속한다. 이는 inference를 반복해도 된다는 허가가 아니다. recorder IPC 실패는
호스트 감독하의 재시작을 요구하며 게이트웨이가 묵시적으로 재연결하거나 요청을 재생하지
않는다.

제공자 결과, 게이트웨이 결과와 usage finality는 별개다. 기록된 완료는 모든 바이트의
Codex 도착, 도구 실행이나 무과금을 증명하지 않는다. 취소할 때 usage를 더 받으려고
응답을 끝까지 읽지 않고 upstream을 닫는다. 부분 관측은 부분 상태로 남긴다. recorder의
백업·보존·내보내기 절차는 continuation 복구 묶음과 별도로 관리한다.

## 검증과 한계

지원 표현은 호스트 관리 Codex 0.154.0과 모의 Claude/DeepSeek/OpenRouter에서 검증된
reasoning 표시·원본 보존·재개 지원으로 한정한다. 필수 검사는 기존 49개 시나리오와
새 reasoning 60개, signed/opaque-only 연속성, v1/v2 복원, 압축, recorder 실패 시
출력 차단, 네 플랫폼 package reasoning·재시작 smoke를 포함한다. 출력 중 취소와
heartbeat-only 취소 모두 기존 5초 upstream 연결 종료 기준을 적용한다.

실제 모델 품질·호환성·비용 qualification에는 별도 수락이 필요하다. 일반 CLI/Desktop
직접 설정, Native Responses managed 모드, previous_response_id, 공개 응답 저장 API,
PostgreSQL/Redis continuation backend는 이 계약 범위 밖이다. PostgreSQL usage
내보내기는 독립된 recorder 기능이다.
