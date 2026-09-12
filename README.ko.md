# agent-response-gateway

[English](README.md) | [한국어](README.ko.md)

Non-Responses 모델 API를 Responses 인터페이스의 선언된 의미로 변환하는
경량 Rust 코어 기반 확장형 프록시 소프트웨어다. 코어·확장·애플리케이션의 책임은
[제품 개요](docs/ko/index.md)를 참조한다.
설정된 모델 별칭을 공급자 모델에 연결하고, 별도 업스트림 자격 증명으로
Responses, Messages와 Chat Completions를 사용한다.

게이트웨이는 JSON/SSE를 전달하고 선언된 변환 기능을 검사한다.
도구 실행, 승인과 대화 이력은 애플리케이션이 담당한다.
접속 주소는 루프백으로 제한한다.

<a id="getting-started"></a>

## 시작하기

패키지 실행은 [다운로드·실행 안내](docs/ko/usage.md#run-a-downloaded-package)에
따라 특정 버전과 플랫폼을 선택한다. 아래 소스 빌드에는 Rust 1.98.0이 필요하다.

Rust 1.98.0과 Cargo가 필요하다. 고정 도구 체계는 `rust-toolchain.toml`,
의존성 버전은 `Cargo.lock`에서 관리한다.

```sh
cargo build --locked
cp config.example.toml config.local.toml
```

`config.local.toml`의 공급자 `base_url`과 `upstream_model`을 사용할 경로로
수정한다. `base_url`은 `/v1` 등의 API 접두사이며 API 선언에 따라
`/responses`, `/messages` 또는 `/chat/completions`를 붙인다. Messages 설정은
[전용 예제](config.messages.example.toml)와 [지원표](docs/ko/messages.md)를 따른다. 최초 검증에는 모의 공급자 주소와 합성 입력을 사용한다.
실제 공급자에 아래 클라이언트로 요청하면 공급자 정책에 따라 비용이 발생할 수 있다.

환경변수 `ARG_LOCAL_TOKEN`에는 32~4096자의 공백 없는 ASCII 토큰을,
`ARG_EXAMPLE_API_KEY`에는 이와 다른 공급자 키를 설정한다. 키를 TOML이나 Git에
넣지 않는다. 로컬 개발 토큰은 예를 들어 다음처럼 생성할 수 있다.

```sh
export ARG_LOCAL_TOKEN="$(python3 -c 'import secrets; print(secrets.token_urlsafe(32))')"
```

`check-config`는 TOML 구조와 경로·제한값을 확인하며 환경변수의 자격 증명과
실제 공급자를 검사하지 않는다. `serve`를 시작할 때 공급자 키 환경변수도
설정해야 하며 자격 증명의 로컬 형식 검사가 추가로 수행된다.

```sh
cargo run --locked -- check-config --config config.local.toml
cargo run --locked -- manifest --config config.local.toml
cargo run --locked -- serve --config config.local.toml
```

기본 바인딩 `127.0.0.1:0`은 가용 포트를 선택한다. 준비 완료 시 stdout에
준비 JSON 한 줄을 출력한다. 로그는 stderr로 출력한다. 스키마·manifest_schema·configuration_sha256는 프로토콜과 실제 적용 설정을
식별한다. 에이전트를 시작하기 전에 오프라인 명세의 해시와 대조한다.

```json
{"event":"ready","address":"127.0.0.1:43127","base_url":"http://127.0.0.1:43127/v1","version":"0.1.0","schema":"gateway-ready/v1","manifest_schema":"gateway-embedded-manifest/v1","configuration_sha256":"<64 lowercase hex characters>"}
```

별도 셸에서도 같은 `ARG_LOCAL_TOKEN`을 사용한다. 아래 주소의 포트는 실제
준비 JSON의 값으로 바꾼다. Python 예제는 표준 라이브러리만 사용한다.

```sh
python3 examples/client.py --base-url http://127.0.0.1:43127/v1 --list-models
python3 examples/client.py --base-url http://127.0.0.1:43127/v1 --model example/writer --input 'Reply with hello.'
python3 examples/client.py --base-url http://127.0.0.1:43127/v1 --model example/writer --input 'Reply with hello.' --stream
```

공급자 이름을 포함한 모델명, `Large-Model` 같은 별칭과 여러 API Key를 구분하는
별칭 설정은 [모델 이름과 호출 경로 예시](docs/ko/route-design.md#consumer-model-names)를 참조한다.
대화·도구·스트림 처리는 [호출·통합 예제](docs/ko/usage.md), 실패 원인 확인은
[문제 해결](docs/ko/troubleshooting.md)로 이어서 확인할 수 있다.

<a id="supported-behavior"></a>

## 지원 범위

<SupportTable>

| 인터페이스 | 동작 |
|---|---|
| `GET /` | 버전·라이선스·설정된 소스 위치 안내 |
| `GET /healthz` | 로컬 프로세스 생존 정보 |
| `GET /readyz` | 로컬 준비 상태; 실제 공급자 probe는 하지 않음 |
| `GET /v1/models` | Bearer 인증 후 설정된 모델명 목록 |
| `POST /v1/responses` | Bearer 인증 후 모델 치환·공급자 인증 교체·JSON/SSE 전달 |

</SupportTable>

Responses 원형 전달은 도구·구조화 출력·추론 항목을 재구성하지 않는다. JSON 필드는 모델
치환과 `store:false` 정규화를 제외하고 보존되지만 JSON 직렬화 바이트가
동일하다는 뜻은 아니다. SSE 응답 본문은 바이트 그대로 전달한다.
변환 경로는 선언된 함수·custom·네임스페이스 도구와 텍스트를 변환하며 미지원
필수 기능을 전송 전에 거부한다. `/v1/models` 등록은 모델 호환성 검증이 아니다.

게이트웨이의 저장 요청, `previous_response_id`와 conversation 기반 서버 상태,
원격 compact API, background 실행, 응답 조회·삭제, 자동 재시도·fallback,
WebSocket, OAuth·계정 풀은 미지원이다. 호스트는 자신의 Codex 이력과 저널로
로컬 압축·재개·복구를 관리할 수 있다. [연속성 계약](docs/ko/continuity.md)은
검증된 binding과 불확실한 실행의 복구 책임을 설명한다.
상세 HTTP 계약과 제한은 [프로토콜 문서](docs/ko/protocol.md)를 참조한다.

<a id="internal-ir-v1"></a>

## 내부 IR v1

라이브러리에는 요청 의미, 출력 이벤트 상태, 기능 판정과 origin-bound 불투명
상태를 표현하는 IR v1을 제공한다. 순수 Responses 요청 왕복 변환기와 custom
tool JSON·네임스페이스·patch 문법 변환 규칙, 이벤트 상태 검증을 포함한다.
변환 HTTP 경로는 이 계약을 사용하고 Responses 경로는 원형 전달을 유지한다. 지원 부분집합과 후속 어댑터 경계는
[IR 계약](docs/ko/ir.md)에 정리했다.

<a id="validation"></a>

## 검증

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
python3 -B -m unittest discover -s scripts/tests -v
python3 -B scripts/license_audit.py check
```

Python 검사에는 3.11 이상을 사용한다. 라이선스 검사는 별도로 준비한
cargo-deny 0.20.2와 잠금 파일의 소스를 오프라인으로 읽는다.
[도구 준비·고지 갱신 안내](licensing/README.ko.md)를 먼저 따른다.

테스트는 모의 업스트림과 합성 데이터를 사용하며 실제 모델 API 키를 요구하지
않는다. CI는 Linux x64·ARM64, macOS ARM64와 Windows x64에서 Rust 1.98.0
검사와 네이티브 패키지 실행을 수행하도록 구성했다. 실제 결과는 [공개 CI 실행](https://github.com/novelKR/agent-response-gateway/actions/workflows/ci.yml)에서
해당 commit의 성공 여부를 확인한다. CI 성공은 실제 공급자 검증이나 운영 수락이 아니다.

[Codex 적합성 시험](tests/codex/README.ko.md)은 실제 `0.154.0` 시험
런타임과 모의 공급자를 사용한다. 준비 방법은 [런타임 안내](docs/ko/codex-contract.md),
시나리오는 [적합성 검증](docs/ko/conformance.md)을 참조한다.
운영 전에는 선택한 실제 모델과 애플리케이션 통합을 시험해야 한다.

<a id="integration-and-licensing"></a>

## 통합과 라이선스

호스트 애플리케이션은 검증된 게이트웨이 릴리스를 버전·해시로 고정하고
런타임 관리자가 에이전트와 함께 관리할 수 있다. 백엔드 서비스는 기존
워크플로·자격 증명·외부 호출의 책임을 유지하면서 HTTP 경로를 연결한다.
[통합 경계](docs/ko/integration.md), [내장 계약](docs/ko/embedded-design.md)과 [배포 절차](docs/ko/release.md)에
소비자별 후속 검증과 책임을 정리했다.

[AGPL-3.0-only](LICENSE) 또는 [상용 라이선스](COMMERCIAL-LICENSING.ko.md)를
선택할 수 있다. 상용 계약을 통해 계약 대상 프로젝트 코드의 AGPL 소스 공개
의무 없이 비공개로 이용할 수 있다. 기여는 [권리 요건](CONTRIBUTING.ko.md)을
따르며 의존성의 [제3자 라이선스와 고지](THIRD-PARTY-NOTICES.ko.md)는 유지한다.

`source_url`이 없으면 루트 응답은 `source_status:"not_configured"`를 표시한다.
AGPL로 배포할 때는 [배포 절차](docs/ko/release.md)에 따라 해당 버전의
대응 소스를 제공하고 HTTPS 위치를 설정한다.

범위는 [개발 방향](docs/ko/roadmap.md), 기여 검사는
[개발 절차](docs/ko/github-workflow.md)를 참조한다.

Linux/macOS의 선택형 [네이티브 확장](docs/ko/extensions.md)은 HTTP 메타데이터
관측기와 [Usage Recorder](docs/ko/usage-accounting.md)를 제공한다. 기록기의 저장·전달
보장은 최선형 관측과 다르다. 자격 증명 브로커·계정 풀·연속성 서비스는 새 계약이
필요한 확장 방향이며 현재 관측 프로토콜로 활성화되지 않는다.
[확장 구조](docs/ko/extensions-design.md)를 참조한다.
