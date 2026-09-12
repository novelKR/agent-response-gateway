<a id="compatibility-and-acceptance"></a>
<a id="data-and-execution-ownership"></a>
<a id="gemini-interactions-implementation-contract"></a>
<a id="recovery-and-compaction"></a>

<a id="gemini-interactions-design"></a>

# Gemini Interactions 구현 계약

[English](../interactions-design.md) | [한국어](interactions-design.md)

런타임은 명시적으로 활성화하는 Interactions 경로와 SQLite 영속
continuation을 제공한다. 호스트가 생성한 세션과 별도의 보호·제어 자격 증명이
필요하다. 로컬 압축 후 호스트가 검증된 이동 가능 맥락을 새 Codex 작업으로
옮기고 새 continuation epoch를 명시적으로 활성화한다.
제공자 계약은 [wire lock](../../tests/interactions/wire-lock.json)에
고정한다. [opaque probe](../../tests/codex/opaque_continuation.py)는 합성 Responses
제공자와 고정 Codex를 시험하며 Gemini나 암호화 구현을 검증하지 않는다.

## 데이터와 실행 소유권

Responses 입력은 공통 IR과 내부 Rust Interactions 어댑터를 거친다.
foreground 호출은 store:false를 명시한다. 클라이언트 도구 실행은 호스트 책임이다.
제공자 steps와 signature는 공개 출력 표현과 별도로 보존한다.
스키마 제약이나 thinking 수준을 묵시적으로 약화하지 않는다.

backend 독립 continuation 계약이 session, epoch, attempt, finalized record와
암호화 payload를 소유한다. 첫 backend는 SQLite이며 PostgreSQL은 후속이다.
호스트는 모델·제어 토큰과 별도로 안정적인 보호 키를 제공한다.
Codex는 각 응답에서 새로 생성한 steps의 인증 암호화 사본을 운반한다.
서버는 정상 경로에서 자체 사본을 읽고 클라이언트 출력과의 연결을 검증한다.

## 복구와 압축

전송 전에 attempt를 commit한다. 실행 가능한 도구 완료, 복구 데이터와 최종
성공을 공개하기 전에 확정 출력과 재생 payload를 저장한다.
finalized 기록이 있으면 인증된 클라이언트 이력으로 누락 payload를 복구할 수 있다.
실행 기록 유실, pending attempt나 결과 불명은 자동 복구·재호출 근거가 아니다.
호스트가 불확실한 작업을 명시적으로 정리하며 모델 호출을 조용히 반복하지 않는다.

호스트는 별도 인증을 사용하는 loopback 제어 인터페이스에서 세션을 생성하고
로컬 압축을 등록한다. epoch 전이에는 검증된 이동 가능 이력과 완료 도구 결과가
필요하며, 잃어버린 제공자 상태가 보존됐다고 주장하지 않는다. Codex 0.154.0은
압축된 이력 뒤에 developer 지시를 다시 삽입한다. 지시 bridge는 대화 중간 지시를
거부한다. 호스트는 원래 작업을 보존하고 요약·완료 도구 결과를 검증한 뒤 새 작업의
이동 가능 사용자 메시지 하나에 담는다. 이 메시지의 정규 digest를 새 epoch에
기록한다. 게이트웨이는 해당 메시지가 정확히 한 번 있는지 확인하고 새 작업의
선행 지시를 받아들이며, 후속 턴부터 전체 입력 prefix를 인증한다.
데이터베이스 기록이 없다는 이유로 새 세션을 추정하지 않는다.

## 호환성과 수락

기존 경로는 독립적으로 유지한다. 데이터베이스 schema, 제공자 wire, 보호 payload와
호스트 manifest 정체성은 버전으로 구분한다. 복구에는 호환되는 Codex 이력,
데이터베이스와 보호 키가 필요하다. 공개 response 저장·조회·삭제,
previous_response_id, 제공자 저장, background 실행, 원격 압축,
managed agents와 내장 hosted tools는 별도 기능이다.

구현 수락에는 고정 Codex의 도구·namespace·patch 왕복, 재시작, 압축,
finalized payload 복구, 결과 불명 거부와 기존 저장소 gate가 필요하다.
실제 모델 qualification과 소비자 수락은 별도다.
opaque probe는 선행 조건이며 어댑터 완성의 증거가 아니다.
