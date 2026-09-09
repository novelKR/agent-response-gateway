<a id="three-route-conformance"></a>

# 세 API 경로 적합성 검증

[English](../conformance.md) | [한국어](conformance.md)

G12는 실제 고정 Codex 실행 파일과 합성 루프백 upstream으로 구현을 검증한다.
기본 CI는 native Responses 9개, Messages 13개, Chat Completions 13개로
총 35개 시나리오를 실행한다. 커밋된 런타임 lock의 임시 0.154.0-alpha.6 /
macOS ARM64 artifact를 사용한다. 이 결과가 실제 공급자 모델, 소비자 워크플로
또는 릴리스를 qualification하는 것은 아니다.

| 기능 | Native Responses | Messages | Chat Completions |
|---|---|---|---|
| JSON/SSE | 원래 JSON 값과 SSE 바이트 | 명시적 codec과 증분 블록 | 명시적 codec, 증분 텍스트와 제한된 도구 조각 |
| 지시 역할 | 원래 필드 | 승인된 선행 지시 envelope; 뒤늦은 지시는 거부 | Native 역할 필드·순서 |
| 함수 도구와 결과 | 원래 wire 필드 | Native tool_use/tool_result | Function tool_calls/tool messages |
| Namespace/custom/등록 patch 문법 | 원래 wire 필드 | 명시적 namespace·JSON wrapper·문법 bridge | Function-wire 프로필의 동일한 명시적 bridge |
| Strict 도구 | 전달 | 선언된 native strict flag | 선언된 native strict flag |
| Strict JSON schema | 원래 descriptor·규칙 | 선언된 native schema 규칙; 원본 이름은 응답 descriptor에 보존 | 선언된 native descriptor·규칙·strict flag |
| Loose schema 또는 json_object | 전달 | 거부 | 선언된 native format |
| 명시적 effort | 원래 값 | low/medium/high/xhigh/max만 지원 | none/minimal/low/medium/high/xhigh/max만 지원 |
| Reasoning 요약·opaque state·verbosity | Stateless 정책 안에서 전달 | 거부 | 거부 |
| 토큰·컨텍스트 한도 | 프로필이 있으면 선언 출력 한도 | 선언 출력 한도·기본값 | 선언 출력 한도·기본값 |
| 입력 토큰 계수 | 미검증 | 미검증 | 미검증 |
| 완료 | 공급자의 종료 의미 | 일관된 stop reason 뒤 유효한 message_stop | 유효한 finish_reason과 마지막 [DONE] |
| 재시도·fallback·저장 | 없음 | 없음 | 없음 |

전달 성공이나 native 프로필 선언은 실제 모델의 기능 지원을 증명하지 않는다.
같은 effort 이름이나 토큰 수가 공급자 간 동일 연산·비용·품질을 뜻하지 않는다.
구조화 출력은 공급자의 native 계약이며 호스트가 결과 데이터를 검증한다.
Gateway는 별도 JSON Schema 구현이나 프롬프트 대체 없이 원래 schema 규칙을 보존한다.

모든 경로에서 텍스트, 함수·namespace 왕복, custom patch 적용과 결과 재입력,
호스트 승인 거절, 이벤트·heartbeat 취소, 전송 단절, 명시적 high effort와
strict JSON schema를 시험한다. 변환 경로는 병렬 호출 두 개, 실행 전 잘못된
patch 문법, 후속 텍스트 turn, 도구·텍스트 출력과 결과 재입력도 검사한다.
Endpoint, 모델, 인증, native 필드, 원래 도구 정체성·인자, 정확한 schema 값,
결과 JSON, 재시도 부재와 interrupt부터 upstream 종료까지의 5000 ms 한도를 검증한다.

변환 시험 호스트는 [Messages](messages.md)에 명시한 catalog와 설정을 사용한다.
기본 선택 기능인 reasoning·verbosity·search를 끄고, 출력 제어 시나리오는
turn마다 필요한 effort와 schema를 명시한다. 기본 도구 프로필과 매 turn
출력 제어가 필요한 호스트를 구분한다. 원본 catalog prompt는 고정 런타임에서
도출해 그대로 유지하며 공개 fixture에 복사하지 않는다.

공통 HTTP 회귀는 두 어댑터의 JSON 변환, 인증 헤더 분리, upstream 접근 전
admission, 임의 byte 분할, 증분 출력, 정제 오류, 완료 누락, 누적 한도,
heartbeat 단절과 동시 요청 슬롯 반환을 검사한다. 순수 codec은 잘못된 JSON·
wrapper, 의미 확장, 출력 순서·정체성, UTF-8, 부분 인자, 문법과 메시지 이력도 검사한다.

고정 런타임과 현재 gateway를 준비한 뒤 `python3 -B tests/codex/conformance.py`로
전체 행렬을 실행한다. `--api`와 `--scenario`는 범위를 좁힌 진단용이며 전체
기본 CI 실행을 대체하지 않는다. 출력은 합성 횟수, 결과, 호스트 프로필 digest와
시간 측정값만 담고 요청·응답 본문이나 자격 증명을 포함하지 않는다.

실제 공급자 qualification에는 대상 모델·자격 증명·비용 한도의 명시적 결정이
필요하다. 소비자 기동·승인·취소와 장기 연속성도 별도 수락 경로를 거친다.
[내장](embedded-design.md) 및 [연속성](continuity-design.md) 계약은 이 경계를 유지한다.
