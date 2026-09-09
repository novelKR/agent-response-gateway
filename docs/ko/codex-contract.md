<a id="codex-test-runtime"></a>
<a id="pinned-codex-contract"></a>
<a id="고정-codex-실행-계약"></a>

# Codex 시험 런타임

[English](../codex-contract.md) | [한국어](codex-contract.md)

시험은 공식 Codex **0.154.0-alpha.6** macOS ARM64 패키지를 사용한다.
`tests/codex/runtime-lock.json`에 압축파일 URL, 크기, SHA-256, 패키지 구성원과
생성된 stable/experimental 스키마의 해시를 고정한다. 이 런타임은 시험판이다.

<a id="preparation-and-verification"></a>

## 준비와 검증

```sh
python3.14 -B scripts/codex_runtime.py prepare
python3.14 -B scripts/codex_runtime.py verify
python3.14 -B scripts/codex_runtime.py schema --profile stable
python3.14 -B scripts/codex_runtime.py schema --profile experimental
```

`prepare`만 파일을 다운로드한다. `prepare --archive <local-archive>`는 받은
파일의 크기·해시·구성원을 검사한다. 기존 묶음은 덮어쓰지 않고 확인한다.
실행 파일과 스키마는 Git에서 제외한 `.local/`에 두며 게이트웨이와 묶어
배포하지 않는다. 스키마 출력에는 새 디렉터리를 사용해야 한다.
준비 과정은 로그인, 모델 호출이나 개인 Codex 설정 변경을 수행하지 않는다.

압축파일의 SHA-256은
`ae37c70e6c86f1f4248303e084cd03480d8628b74df57649c128730ce4159d50`이다.
필요한 스키마 프로필을 선택한다. stable과 experimental은 해시와 지원 필드가 다르다.

<a id="control-and-model-interfaces"></a>

## 제어 인터페이스와 모델 인터페이스

제어는 stdio JSONL을 사용한다. 한 번 초기화하고 initialized를 보낸 뒤 대화와
요청을 시작하며 알림·서버 요청을 처리한다. 명시적 최종 상태를 가진
turn/completed를 확인해야 한다. 동적 도구에는 experimental API 선택이 필요하다.
해당 버전의 기준은 고정 실행 파일에서 생성한 스키마다.

모델 요청은 게이트웨이와 별도의 Responses HTTP/SSE 연결로 통신한다.
시험은 전용 CODEX_HOME, 합성 자격 증명, HTTP·스트림 재시도를 끈 루프백
공급자를 사용한다. 도구와 승인은 Codex·호스트가, 모델 전송은 게이트웨이가 담당한다.

<a id="required-acceptance"></a>
<a id="required-checks"></a>
<a id="필수-수락-기준"></a>

## 필수 검사

| 영역 | 요구 결과 |
|---|---|
| 텍스트와 스트리밍 | 출력 순서와 명시적 성공 종료 |
| 함수 도구 | 호출 식별자·인자·결과·후속 요청 보존 |
| 사용자 정의 도구 | 자유 형식 입력 보존과 필수 문법 검사 |
| 도구 네임스페이스 | 그룹·식별자 보존과 미지원 형태 거부 |
| 승인 거절 | 실제 승인 요청을 거절하고 동작을 실행하지 않음 |
| 취소 | 제어 요청 취소와 업스트림 연결 종료 |
| 전송 실패 | 완료 누락 시 스트림 결합이나 재시도 없이 실패 |
| 컨텍스트 | 모델별 크기, 출력 여유와 압축 임계값 일치 |
| 연속성 | 호스트 상태 계약에 따른 도구 이력·압축·재시작 검사 |

런타임을 확인한 뒤 [세 경로 시험](conformance.md)을 실행한다.
모의 공급자 결과는 프로토콜 경로를 검증한다. 호스트는 운영 전에 실제 모델의
동작과 자신의 컨텍스트 한도, 권한, 복구를 시험해야 한다.

<a id="temporary-baseline-and-stable-replacement"></a>
<a id="updating-the-test-runtime"></a>
<a id="임시-기준과-안정판-교체"></a>

## 시험 런타임 갱신

대체 안정판은 **0.154.0 이상**이어야 한다. 잠금 파일을 바꾸기 전에 공식
산출물을 검증하고 두 스키마 프로필을 다시 생성하며 전체 적합성 시험과
저장소 검사를 통과해야 한다. 검토할 변경에 실제 버전과 해시를 기록하고
검증하지 않은 최신 빌드를 자동으로 선택하지 않는다.

Heartbeat만 전송하는 스트림의 취소는 필수 회귀 시험이다. 런타임을 바꿔도
도구·승인·취소 계약 전체를 보존해야 한다. 이 시험 잠금 파일을 갱신하는 작업이
다른 애플리케이션의 런타임을 선택하거나 설치하지는 않는다.

참고: [App Server 문서](https://learn.chatgpt.com/docs/app-server),
[고정 릴리스](https://github.com/openai/codex/releases/tag/rust-v0.154.0-alpha.6).
