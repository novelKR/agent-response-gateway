<a id="pinned-codex-contract"></a>

# 고정 Codex 실행 계약

[English](../codex-contract.md) | [한국어](codex-contract.md)

임시 시험 기준은 공식 Codex **0.154.0-alpha.6** macOS ARM64 패키지다.
`tests/codex/runtime-lock.json`은 공식 archive URL·바이트 크기·SHA-256,
실행 파일과 패키지 구성원, stable/experimental 생성 schema 묶음 digest를 기록한다.
이 값은 공식 배포물과 검증된 실행 파일에서 얻었으며, 소비자의 비공개 설정이나
생성된 운영 로그에서 가져온 값이 아니다.

<a id="preparation-and-verification"></a>

## 준비와 검증

```sh
python3.14 -B scripts/codex_runtime.py prepare
python3.14 -B scripts/codex_runtime.py verify
python3.14 -B scripts/codex_runtime.py schema --profile stable
python3.14 -B scripts/codex_runtime.py schema --profile experimental
```

다운로드는 `prepare`만 수행한다. `prepare --archive <local-archive>`는 이미 받은
파일을 사용하면서도 크기·digest·구성원을 검증한다. 기존 묶음은 덮어쓰지 않고
검사한다. 실행 파일과 schema는 ignored `.local/`에 두며 gateway에 포함해
배포하지 않는다. Schema 출력에는 새 디렉터리가 필요하다. 준비는 로그인,
모델 호출이나 개인 Codex HOME 변경을 수행하지 않는다.

공식 archive digest는
`ae37c70e6c86f1f4248303e084cd03480d8628b74df57649c128730ce4159d50`이다.
생성된 stable과 experimental 묶음의 digest는 서로 다르다. Experimental 필드를
stable 프로토콜로 취급하지 말고 사용할 프로필을 명시한다.

<a id="control-and-model-interfaces"></a>

## 제어 인터페이스와 모델 인터페이스

제어는 stdio JSONL을 사용한다. 한 번 초기화하고 initialized를 보낸 뒤 thread와
turn을 시작한다. 알림·서버 요청을 처리하고 명시적 최종 상태를 가진 turn/completed를
요구한다. Dynamic tool은 experimental API 선택이 필요하다. 고정 실행 파일이
생성한 정확한 schema가 해당 버전의 기준이다.

모델 트래픽은 gateway로 연결하는 별도의 Responses HTTP/SSE 통신이다.
Conformance 프로필은 전용 CODEX_HOME, 합성 자격 증명, HTTP·stream 재시도를
끈 루프백 custom provider를 사용한다. 개인 인증, WebSocket, 공급자 fallback은
사용하지 않는다. 도구와 승인은 gateway가 아닌 Codex와 호스트의 책임이다.

<a id="required-acceptance"></a>

## 필수 수락 기준

| 영역 | 요구 결과 |
|---|---|
| 텍스트와 스트리밍 | 출력 항목 순서와 명시적 성공 종료 |
| 함수 도구 | 호출 정체성·인자·결과·후속 turn 보존 |
| Custom 도구 | 원래 자유 형식 입력과 필수 문법 보존; JSON 포장만으로 성공을 주장하지 않음 |
| 도구 namespace | 실제 선언 그룹과 정체성 보존; 미지원 형태는 명시적으로 실패 |
| 승인 거절 | 호스트가 실제 Codex 승인 요청을 거절하고 제안한 동작이 실행되지 않음 |
| 취소 | 명시적 취소 상태에 도달하고 upstream 작업 연결이 닫힘 |
| 전송 실패 | 완료 누락을 성공으로 처리하지 않으며 스트림 결합·묵시적 재시도 없음 |
| 컨텍스트 | 모델별 window, 출력 예약과 압축 임계값의 일치 |
| 연속성 | 도구 후 재개·압축·재시작은 별도로 승인된 상태 계약을 따름 |

합성 모델 한도는 시험 fixture이며 실제 모델 사양이 아니다. 런타임 호스트는
컨텍스트 선택과 실행·재시도 예산을, gateway는 선언된 전송 경로와 호환성 검사를
소유한다. Gateway는 stateless 상태를 유지한다. 기능·경로 설계와 영속화 설계는
별도 승인 작업이다.

시험 장치는 실제로 실행한 검사를 기록한다. [현재 세 경로 시험](conformance.md)은
기존 G04 기준을 확장한다. 바이너리 무결성, schema 생성과 합성 archive 검사만으로
Codex conformance, 공급자 qualification, 소비자 수락이나 정식 배포를 주장하지 않는다.

<a id="temporary-baseline-and-stable-replacement"></a>

## 임시 기준과 안정판 교체

이전 안정판 0.153.4는 heartbeat 취소 시험에 실패했다. 짝을 맞춘 합성 시험과
고정 소스는 해당 안정판의 스트림 수명 관리 결함을 유력한 원인으로 가리킨다.
리더가 SSE 이벤트를 기다리는 동안 수신 측 종료를 함께 기다리지 않기 때문이다.
공식 0.154.0-alpha.6에는 이 종료 검사가 있으며 같은 로컬 conformance 시험 8개를
통과했다. 사용자가 이 임시 시험 기준 변경을 승인했다.

**0.154.0 또는 이후 안정판**이 제공되면 공식 artifact 검증, 두 schema 프로필
재생성과 전체 conformance·저장소 gate를 거쳐 교체한다. 정확한 버전·digest
증거를 가진 검토 PR로 진행한다. Alpha 바이트를 0.154.0으로 다시 표시하거나,
미검증 최신 빌드를 받거나, prerelease를 안정판으로 취급하지 않는다.

이는 시험 기준만의 변경이다. 소비자의 설치된 런타임을 바꾸거나 생산 배포를
승인하지 않는다. 재현을 위해 이전 릴리스와 비교 증거를 유지하며 소비자의
이력이나 상태를 마이그레이션하지 않는다.

참고: [공식 App Server 문서](https://learn.chatgpt.com/docs/app-server),
[공식 임시 릴리스](https://github.com/openai/codex/releases/tag/rust-v0.154.0-alpha.6),
[이전 안정판 SSE 리더](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/codex-api/src/sse/responses.rs),
[수신 종료 수정](https://github.com/openai/codex/blob/rust-v0.154.0-alpha.6/codex-rs/codex-api/src/sse/responses.rs).
