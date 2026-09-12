<a id="three-route-conformance"></a>

<a id="세-api-경로-적합성-검증"></a>
<a id="protocol-conformance"></a>

# 프로토콜 적합성 검증

[English](../conformance.md) | [한국어](conformance.md)

적합성 시험은 버전이 고정된 실제 Codex, 게이트웨이와 모의 HTTP 공급자를
실행한다. Responses 9개, Messages와 Chat Completions 각 13개로 총 35개
기존 시나리오를 검사한다. Interactions는 14개 시나리오와 별도의 호스트
continuation/실패 시험을 추가한다. 시험 런타임은 macOS ARM64의 0.154.0이며
[런타임 잠금 파일](../../tests/codex/runtime-lock.json)에 고정되어 있다.

<a id="protocol-coverage"></a>

## 프로토콜 지원 범위

Responses는 상태를 저장하지 않는 HTTP 계약 안에서 원래 필드를 전달한다.
변환 경로는 사용하는 기능을 프로필에 명시해야 한다. 아래 표는 기존 세 경로를
설명하며, 지시·저장·스키마 경계가 다른 [Interactions 지원 표](interactions.md)는
별도로 확인한다.

| 기능 | Responses | Messages | Chat Completions |
|---|---|---|---|
| JSON/SSE | 원래 JSON 값과 SSE 바이트 | 변환된 JSON과 증분 이벤트 | 변환된 JSON과 증분 이벤트 |
| 지시 역할 | 원래 필드 | 대화 앞의 지시 묶음; 뒤늦은 지시는 거부 | 원래 역할 필드와 순서 |
| 함수 도구·결과 | 원래 필드 | tool_use/tool_result | tool_calls/tool 메시지 |
| 네임스페이스·사용자 정의 텍스트·등록 패치 문법 | 원래 필드 | 선언된 이름 매핑·JSON 포장·문법 검사 | 함수 도구 필드로 같은 변환 규칙 적용 |
| 엄격한 도구 인자 | 전달 | 선언된 strict 필드 | 선언된 strict 필드 |
| 엄격한 JSON 스키마 | 원래 형식과 규칙 | 선언된 스키마 규칙; 이름은 Responses 설명 필드에 보존 | 선언된 스키마 이름·규칙·strict 필드 |
| 느슨한 스키마 또는 json_object | 전달 | 거부 | 선언된 형식 |
| 추론 강도 | 원래 값 | low/medium/high/xhigh/max | none/minimal/low/medium/high/xhigh/max |
| 추론 요약·불투명 상태·verbosity | 상태 저장 제한 안에서 전달 | 거부 | 거부 |
| 컨텍스트·출력 한도 | 프로필이 있으면 선언된 출력 한도 | 선언된 출력 한도·기본값 | 선언된 출력 한도·기본값 |
| 입력 토큰 계수 | 미구현 | 미구현 | 미구현 |
| 완료 | 공급자의 종료 이벤트 | 일관된 종료 사유 뒤 유효한 message_stop | 유효한 finish_reason과 마지막 [DONE] |
| 재시도·대체 경로·저장 | 없음 | 없음 | 없음 |

공급자 지원과 출력 품질은 선택한 모델로 시험해야 한다. 추론 강도 이름이나
토큰 수가 같아도 연산량·비용·품질이 같다는 뜻은 아니다. 구조화 출력은 호스트가
검증하며, 게이트웨이는 스키마 규칙을 보존하고 프롬프트나 별도 범용 스키마
검증기로 대체하지 않는다.

<a id="test-coverage"></a>

## 시험 항목

모든 경로에서 텍스트, 함수·네임스페이스 왕복, 사용자 정의 패치 적용,
승인 거절, 명시적 추론 강도·엄격한 출력, 전송 실패와 취소를 검사한다.
취소는 출력 중인 스트림과 heartbeat만 보내는 스트림을 모두 포함하며,
중단 요청부터 5000 ms 안에 업스트림 연결이 닫혀야 한다.

변환 경로는 병렬 도구, 실행 전 문법 실패, 후속 텍스트와 도구·텍스트 혼합
결과도 검사한다. 경로·모델 선택, 자격 증명 분리, 도구 식별자·인자,
응답 값과 재시도 부재를 확인한다. HTTP·변환기 시험은 잘못된 입력,
임의 바이트·UTF-8 분할, 잘린 응답, 출력 한도와 동시 요청 슬롯 반환도 검사한다.

시험 호스트는 [Messages](messages.md)의 제한된 모델 목록을 사용한다.
선택적인 추론·verbosity·검색 기본값은 끄고, 요청별로 명시한 출력 제어는 별도로 시험한다.

<a id="running-the-suite"></a>

## 시험 실행

[시험 안내](../../tests/codex/README.ko.md)에 따라 고정 런타임과 게이트웨이를
준비한 뒤 `python3 -B tests/codex/conformance.py`를 실행한다.
`--api`와 `--scenario`로 로컬 검사 범위를 좁힐 수 있으며 CI는 전체를 실행한다.
결과에는 횟수·상태·프로필 해시·시간만 담고 요청·응답 본문이나 자격 증명은 넣지 않는다.

Interactions는 같은 명령에 `--api gemini_interactions`를 지정하고,
`python3 -B tests/codex/interactions_http.py`와
`python3 -B tests/codex/interactions_continuity.py`도 실행한다. required CI는 opaque
probe, 재시작, payload 유실 복원, 실행 기록 유실·pending 차단, 명시적 복구,
새 Codex 작업으로의 호스트 관리 압축과 이후 재시작도 검사한다.

이 시험은 모의 공급자를 사용한 프로토콜 호환성을 확인한다. 운영 전에는
[내장](embedded-design.md)과 [연속성](continuity-design.md) 계약에 따라
실제 모델, 애플리케이션 권한과 복구를 시험해야 한다.

별도의 checked Responses 모드는
`python3 -B tests/codex/conformance.py --api responses_checked`로 실행하는
합성 시나리오 13개를 추가한다. 고정된 Codex를 통해 custom/function/namespace
이력 복원, 병렬 도구, 등록된 grammar 거절, 혼합 텍스트·도구 출력, 승인 거절과
취소를 검사한다. `--responses-native-custom`을 추가하면 native custom 입력을
유지하며 등록된 grammar를 로컬에서 검증한다. 기존 경로의 기본 실행은 이 정책을
활성화하지 않는다. 코덱 테스트는 공개 reasoning 요약의 생명주기, 임의 UTF-8 분할,
불일치하는 done·최종 값과 durable 사용량 커밋 대기도 검증한다. 이 근거는
[도구 정책](protocol.md#checked-responses-tools)의 checked 부분집합에 관한 것이며
실제 공급자의 적합성 입증이 아니다.
