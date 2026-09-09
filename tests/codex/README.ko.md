<a id="real-codex-synthetic-upstream"></a>
<a id="testing-with-codex-and-mock-providers"></a>
<a id="실제-codex와-합성-upstream"></a>

# Codex와 모의 공급자로 시험하기

[English](README.md) | [한국어](README.ko.md)

적합성 시험은 고정 Codex 실행 파일, 게이트웨이 바이너리와 모의 루프백 HTTP
공급자를 시작한다. 공급자 계정이나 개인 인증 없이 도구 실행과 프로토콜을
검사한다. 생성한 홈, 작업 공간과 파일은 `.local/`에 두고 시나리오마다 정리한다.

```sh
python3.14 -B scripts/codex_runtime.py prepare
cargo build --locked
python3.14 -B tests/codex/conformance.py
```

런타임 준비는 고정된 공식 패키지를 다운로드한다. 시나리오마다 별도 로컬
토큰과 Codex 홈을 사용하며 공급자 키는 게이트웨이에만 전달한다.
Codex가 시험용 패치를 적용하고 호스트가 동적 도구와 승인 요청을 처리한다.
[런타임 안내](../../docs/ko/codex-contract.md)를 참조한다.

기본 시험은 Responses 9개, Messages와 Chat Completions 각 13개로 총 35개다.
`--api responses`, `--api messages`, `--api chat_completions`로 경로 하나를 선택한다.
시나리오별 JSON 결과를 출력하며 하나라도 실패하면 0이 아닌 종료 코드를 반환한다.
앞선 시나리오가 실패해도 뒤의 시험은 계속 실행한다.

`gpt-5.4`는 Codex 도구 설정을 선택하는 식별자다. 모든 모델 요청은 모의 공급자의
`synthetic-model`로 보낸다. 컨텍스트 32,768토큰과 압축 임계값 24,576토큰은
시험 설정이며 실제 모델 사양이 아니다.

| 시나리오 | 검사 내용 | 경로 |
|---|---|---|
| text | 요청 한 번과 명시적 완료 | 전체 |
| function_tool | 동적 함수 호출, 결과와 후속 요청 | 전체 |
| namespace_tool | 네임스페이스 식별자와 인자 | 전체 |
| custom_patch | 시험용 패치 적용과 결과 반환 | 전체 |
| approval_denial | 파일 변경 승인을 거절하고 파일을 쓰지 않음 | 전체 |
| cancellation | 텍스트 관측 후 중단하고 출력 중인 스트림을 닫음 | 전체 |
| cancellation_heartbeat | SSE 주석만 보내는 스트림 종료 | 전체 |
| transport_failure | 완료 없는 EOF를 재시도 없이 실패 처리 | 전체 |
| output_controls | 명시적 추론 강도와 엄격한 출력 스키마 | 전체 |
| parallel_tools | 도구 호출 두 개와 결과 보존 | 변환 경로 |
| grammar_failure | 도구 실행 전에 잘못된 문법 거부 | 변환 경로 |
| text_followup | 후속 요청에서 이전 assistant 텍스트 보존 | 변환 경로 |
| mixed_tool_text | 혼합 출력과 도구 결과 재입력 보존 | 변환 경로 |

<a id="cancellation-requirements"></a>
<a id="previous-failure-and-temporary-baseline"></a>
<a id="이전-실패와-임시-기준"></a>

## 취소 조건

두 취소 시나리오 모두 클라이언트 출력을 관측한 뒤 중단한다. 공급자가
heartbeat 주석만 보낼 때도 중단 요청부터 5000 ms 안에 제어 요청이 끝나고
업스트림 소켓이 닫혀야 한다. 제어 상태만 바뀌는 것으로는 부족하다.
두 사례를 시험에 유지하며 이벤트 내용을 바꾸거나 게이트웨이 시간 제한을
줄여 취소 실패를 감추지 않는다. 소켓 종료가 공급자에서 이미 처리한 작업의
취소를 보장하지는 않는다.

<a id="converted-route-profile"></a>
<a id="messages-profile-and-extended-checks"></a>
<a id="messages-프로필과-확장-검사"></a>

## 변환 경로 프로필

호스트는 `debug models --bundled`에서 호환 모델 목록을 만들고 프롬프트를
유지하면서 선언된 선택적 추론·verbosity·검색 필드만 바꾼다.
이 프로필에서는 호스트 검색과 다중 에이전트 도구를 끈다.
게이트웨이는 요청을 통과시키기 위해 필수 의미 필드를 제거하지 않는다.

패치 시험은 문법 지문을 확인하고 실제 시험용 패치를 적용해 결과를 반환한다.
문법 오류가 나면 도구 실행과 재시도가 없어야 한다.
정확한 목록 설정은 [Messages 지원](../../docs/ko/messages.md)에 정리되어 있다.

결과에는 목록·프롬프트·본문 없이 프로필 해시와 시간을 기록한다.
first_client_text_ms는 도구 왕복 뒤일 수 있고, turn_elapsed_ms는 준비 시간을
제외하며, interrupt_to_upstream_close_ms는 취소 시간을 측정한다.
모의 공급자 전체 경로의 시간이며 실제 모델 지연이나 게이트웨이만의 처리 시간이 아니다.

<a id="embedded-child-contract"></a>
<a id="embedded-process-checks"></a>
<a id="내장-child-계약"></a>

## 내장 프로세스 검사

각 시나리오 전에 자격 증명 없이 오프라인 명세를 읽고 Python으로 설정의
SHA-256을 다시 계산한다. 자식의 크기가 제한된 준비 통지를 스키마, 버전,
해시와 숫자 루프백 주소에 대조한다. Codex에는 전용 HOME/CODEX_HOME과
로컬 토큰만 전달하고 공급자 키는 게이트웨이 자식 환경에 둔다.
준비 통지가 다르거나 시작 시간이 초과되면 시험을 실패시키고 자식을 정리한다.

별도 Rust·스크립트 시험은 잘못된 준비 통지, 시간 초과 후 정리, 바인딩 실패와
활성 응답이 있는 상태의 정상 종료를 검사한다. 일반 Rust 테스트에는 Codex가 필요하지 않다.

<a id="continuity"></a>

## 연속성

`python3 -B tests/codex/continuity.py`로 [호스트 연속성 계약](../../docs/ko/continuity.md)에
따른 도구 이력, 로컬 압축, 프로세스 재시작과 명시적 모델 변경을 시험한다.
운영 전에는 실제 모델의 요약 품질과 애플리케이션의 저장·복구를 별도로 검증한다.
