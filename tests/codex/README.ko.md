<a id="real-codex-synthetic-upstream"></a>

# 실제 Codex와 합성 upstream

[English](README.md) | [한국어](README.ko.md)

이 명시적 conformance 명령은 고정 Codex, 실제 gateway와 합성 루프백 HTTP
upstream을 시작한다. Upstream fixture는 새로 작성했으며 참고 구현의 테스트나
소비자 내용을 복사하지 않는다. 생성한 HOME, workspace와 파일은 `.local/`에 둔다.

```sh
python3.14 -B scripts/codex_runtime.py prepare
cargo build --locked
python3.14 -B tests/codex/conformance.py
```

런타임 준비만 공식 artifact를 다운로드한다. Conformance는 공급자 계정이나
개인 인증을 사용하지 않는다. 시나리오마다 별도 로컬 토큰과 Codex HOME을
만든다. Upstream 키는 Codex child 환경에 넣지 않는다. Gateway는 전송만
소유하며 Codex가 합성 patch를 실행하고 호스트가 dynamic 도구·승인 요청에 답한다.

기본 명령은 native Responses, Messages, Chat Completions 세 경로 전체를
실행한다. `--api responses`, `--api messages`, `--api chat_completions`로
한 경로를 선택한다. 시나리오별로 payload 없는 JSON 결과를 출력하고 하나라도
실패하면 0이 아닌 값으로 종료한다. 앞선 실패 이후의 결과도 수집한다.
`gpt-5.4`는 Codex 도구 프로필 선택용이며 모든 모델 트래픽은 합성 공급자의
`synthetic-model`로 간다. 실제 모델 시험이 아니다. 32,768-token context와
24,576-token 압축 임계값은 합성 설정이며 실제 공급자 모델 사양이 아니다.

| 시나리오 | 계약 |
|---|---|
| text | 요청 한 번과 명시적 완료 turn |
| function_tool | Dynamic 함수 호출, 결과와 후속 왕복 |
| namespace_tool | Namespace 정체성과 인자 보존 |
| custom_patch | Codex가 합성 patch를 적용하고 결과가 모델로 돌아옴 |
| approval_denial | 실제 파일 변경 승인을 거절하고 파일을 쓰지 않음 |
| cancellation | 클라이언트 출력 관측 후 interrupt, 이벤트 stream 종료 |
| cancellation_heartbeat | SSE comment만 보내는 stream도 같은 interrupt로 종료 |
| transport_failure | 완료 없는 EOF는 재시도 없이 실패 |

시나리오 후 런타임 프로세스, upstream thread와 임시 workspace를 정리한다.
일반 Rust 테스트는 Codex 없이 gateway를 검사한다. [고정 계약](../../docs/ko/codex-contract.md)을 참조한다.

<a id="previous-failure-and-temporary-baseline"></a>

## 이전 실패와 임시 기준

안정판 0.153.4의 heartbeat 취소는 제어 turn이 `interrupted`가 돼도 upstream
소켓이 5초 한도를 넘어 열려 있어 실패했다. SSE comment 대신 다른 모델
이벤트를 보내면 닫힌다. Fixture는 클라이언트의 부분 출력을 관측한 뒤 취소하므로
기동과의 race가 아니다.

고정 구현은 SSE reader task를 시작해 다음 파싱 이벤트나 idle timeout을
기다린다. 파싱 이벤트를 전달할 때 수신 측 drop을 보지만 대기 중에는 종료를
함께 선택하지 않는다. SSE comment는 그런 이벤트를 생성하지 않는다. 이
소스 흐름은 짝지은 합성 재현과 일치한다. [고정 SSE 소스](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/codex-api/src/sse/responses.rs)를 따른다.

HTTP 클라이언트가 연결을 유지하면 gateway가 제어 interrupt를 추론할 수 없다.
가짜 Responses 이벤트를 넣거나 timeout을 줄여 숨기거나 회귀 사례를 제거하지
않는다. 의심되는 안정판 수명 결함은 별도로 승인한 임시 **0.154.0-alpha.6**
기준으로 대응하며 해당 수신 종료 수정은 같은 로컬 시험 8개를 통과했다.
공식 artifact/schema와 전체 검증 후 **0.154.0 또는 이후 안정판**으로 교체한다.
실제 alpha 버전·digest를 명시하고 소비자 런타임·생산 업그레이드로 취급하지 않는다.

Heartbeat 사례는 CI에서 계속 필수다. 이전 버전이 한 번 통과해도 반복 재현과
소스 결함 증거가 사라지지 않는다. 이 fixture는 공급자가 이미 처리한 작업의
중단이나 비용 반환을 보장하지 않는다.

<a id="messages-profile-and-extended-checks"></a>

## Messages 프로필과 확장 검사

현재 [공통 행렬](../../docs/ko/conformance.md)은 native Responses 9개와 Messages·
Chat Completions 각 13개로 총 35개다. 초기 8개 비교에 effort·strict 출력
제어와 변환 경로의 병렬 도구, 문법 실패, 후속 텍스트, 도구·텍스트 혼합 재입력을
추가했다. 호스트는 고정 바이너리의 `debug models --bundled`에서 최소 호환
catalog를 도출하고 원래 prompt는 유지하며 선언한 선택적 reasoning·verbosity·
search 기능만 바꾼다. 이 프로필은 호스트 search와 multi-agent 도구를 끈다.
Gateway에서 필수 의미 필드를 제거해 통과시키지 않는다. 결과에는 catalog
digest와 시간값만 있고 catalog·prompt·본문은 올리지 않는다.

Custom 시험은 전달된 문법 지문을 확인하고 실제 합성 patch를 적용해 결과를
반환한다. 잘못된 문법은 실행 0회·재시도 없음의 실패 turn이어야 한다. 두 취소
방식 모두 interrupt 시점부터 5000 ms 안에 소켓이 닫혀야 한다. 후속 텍스트
시험은 두 번째 실제 turn에서 이전 assistant 내용을 확인한다. 병렬 도구는
두 call ID와 결과를 보존한다.

시간은 합성 전체 경로를 설명한다. first_client_text_ms는 도구 왕복 뒤일 수
있고 turn_elapsed_ms는 준비를 제외하며 취소는 interrupt_to_upstream_close_ms를
기록한다. 공급자 지연이나 분리된 gateway overhead가 아니다. 정확한 프로필·
한계·로컬 측정은 [Messages 지원](../../docs/ko/messages.md)을 따른다. 소비자 활성화와
실제 모델 시험은 별도다.

<a id="embedded-child-contract"></a>

## 내장 child 계약

각 시나리오 전에 자격 없이 오프라인 manifest를 읽고 Python에서 설정 SHA-256을
다시 계산한다. Child의 제한된 준비 한 줄을 schema/version/digest와 숫자 루프백
설정에 대조한다. Codex는 전용 HOME/CODEX_HOME과 로컬 토큰만 받고 upstream
값은 gateway child에 한정한다. Readiness 불일치나 시작 timeout은 실패하고
기존 정리 경로가 시작한 child를 회수한다. 별도 스크립트·Rust 시험은 잘못된
frame, deadline 정리, bind 실패와 활성 응답 중 정상 종료를 검사한다.
실행 파일·배포 출처와 소비자 운영 수락은 별도다.

<a id="continuity"></a>

## 연속성

실제 고정 Codex의 도구, 로컬 압축, 재시작과 명시적 모델 전환은
`python3 -B tests/codex/continuity.py`로 시험한다. 합성 루프백과
[호스트 연속성 계약](../../docs/ko/continuity.md)을 사용하며 공급자나 소비자
워크플로를 qualification하는 결과가 아니다.
