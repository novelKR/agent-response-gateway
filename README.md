# agent-response-gateway

Rust로 작성한 독립 Responses 프록시의 초기 기반이다. 등록한 공개 모델명을
공급자의 실제 모델명으로 바꾸고, 별도 공급자 인증으로 JSON 또는 SSE를
전달한다. 에이전트 런타임과 백엔드 서비스가 같은 HTTP 인터페이스를
소비할 수 있도록 구성한다.

현재는 **루프백 전용 Responses → Responses 전달**을 제공한다. Messages와
Chat Completions 변환, Codex 실제 호환성, 백엔드의 tenant·egress 통합과
생산 운영 수락은 아직 완료되지 않았다.

## 시작하기

Rust 1.98.0과 Cargo가 필요하다. 고정 도구 체계는 `rust-toolchain.toml`,
의존성 버전은 `Cargo.lock`에서 관리한다.

```sh
cargo build --locked
cp config.example.toml config.local.toml
```

`config.local.toml`의 공급자 `base_url`과 `upstream_model`을 사용할 경로로
수정한다. `base_url`은 `/v1` 등의 API 접두사이며 게이트웨이가 `/responses`를
붙인다. 최초 검증에는 모의 공급자 주소와 합성 입력을 사용한다.
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
cargo run --locked -- serve --config config.local.toml
```

기본 바인딩 `127.0.0.1:0`은 가용 포트를 선택한다. 준비 완료 시 stdout에
다음 형태의 JSON 한 줄을 출력한다. 로그는 stderr로 출력한다.

```json
{"event":"ready","address":"127.0.0.1:43127","base_url":"http://127.0.0.1:43127/v1","version":"0.1.0"}
```

별도 셸에서도 같은 `ARG_LOCAL_TOKEN`을 사용한다. 아래 주소의 포트는 실제
준비 JSON의 값으로 바꾼다. Python 예제는 표준 라이브러리만 사용한다.

```sh
python3 examples/client.py --base-url http://127.0.0.1:43127/v1 --list-models
python3 examples/client.py --base-url http://127.0.0.1:43127/v1 --model example/writer --input 'Reply with hello.'
python3 examples/client.py --base-url http://127.0.0.1:43127/v1 --model example/writer --input 'Reply with hello.' --stream
```

## 지원 범위

| 인터페이스 | 동작 |
|---|---|
| `GET /` | 버전·라이선스·설정된 소스 위치 안내 |
| `GET /healthz` | 로컬 프로세스 생존 정보 |
| `GET /readyz` | 로컬 준비 상태; 실제 공급자 probe는 하지 않음 |
| `GET /v1/models` | Bearer 인증 후 설정된 모델명 목록 |
| `POST /v1/responses` | Bearer 인증 후 모델 치환·공급자 인증 교체·JSON/SSE 전달 |

도구·구조화 출력·추론 항목은 재구성하지 않는다. 원래 JSON 필드는 모델
치환과 `store:false` 정규화를 제외하고 보존되지만 JSON 직렬화 바이트가
동일하다는 뜻은 아니다. SSE 응답 본문은 바이트 그대로 전달한다.
공급자 모델을 `/v1/models`에 등록했다는 사실은 기능 호환성 검증을 의미하지 않는다.

저장 요청, 이전 response ID와 conversation 기반 상태, 압축, background 실행,
응답 조회·삭제, 자동 재시도·fallback, WebSocket, OAuth·계정 풀은 미지원이다.
상세 계약과 제한은 [프로토콜 문서](docs/protocol.md)를 참조한다.

## 내부 IR v1

라이브러리에는 요청 의미, 출력 이벤트 상태, 기능 판정과 origin-bound 불투명
상태를 표현하는 IR v1을 제공한다. 순수 Responses 요청 왕복 codec과 custom
tool JSON bridge, 이벤트 상태 검증을 포함한다. 기존 HTTP 전달 경로에
공급자 변환을 활성화하지는 않는다. 지원 부분집합과 후속 어댑터 경계는
[IR 계약](docs/ir.md)에 정리했다.

후속 구현의 우선순위·의존성과 완료 기준은 [구현 마일스톤](docs/roadmap.md),
PR·커밋·CI 운영 방식은 [GitHub 작업 절차](docs/github-workflow.md)에 정리했다.
계획과 현재 지원 범위는 구분한다.

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
[도구 준비·고지 갱신 안내](licensing/README.md)를 먼저 따른다.

테스트는 모의 upstream과 합성 데이터를 사용하며 실제 모델 API 키를 요구하지
않는다. CI는 Linux와 macOS에서 Rust 1.98.0으로 같은 검사를 실행하도록
구성했다. 이 구성 파일의 존재는 GitHub에서 CI가 실행되었다는 증거가 아니다.

## 통합과 라이선스

호스트 애플리케이션은 검증된 게이트웨이 릴리스를 버전·해시로 고정하고
런타임 관리자가 에이전트와 함께 관리할 수 있다. 백엔드 서비스는 기존
워크플로·자격 증명·외부 호출의 책임을 유지하면서 HTTP 경로를 연결한다.
[통합 경계](docs/integration.md)와 [배포 절차](docs/release.md)에
소비자별 후속 검증과 책임을 정리했다.

공개 라이선스는 [AGPL-3.0-only](LICENSE)다. 별도 상용 계약을 제공할 정책은
[COMMERCIAL-LICENSING.md](COMMERCIAL-LICENSING.md)에 있으며 실제 대체 허가나
체결된 계약은 아니다. [기여 정책](CONTRIBUTING.md)과
[제3자 고지 현황](THIRD-PARTY-NOTICES.md)도 함께 확인한다.

`source_url`이 없으면 루트 응답은 `source_status:"not_configured"`를 표시한다.
실제 배포 시에는 해당 버전의 Corresponding Source를 제공하고 검증된 HTTPS
위치를 설정한다. URL 표시만으로 모든 라이선스 의무를 이행했다고 간주하지 않는다.

공개 문서는 범용 제품 계약만 다룬다. 로컬 메모를 별도 저장소로 관리하는
경우 부모 Git과 배포물의 추적 대상에서 제외한다. 공개 경계 검사와
소스 배포 절차는 [문서 관리](docs/documentation.md)에 설명한다.
