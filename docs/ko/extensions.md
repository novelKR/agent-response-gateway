<a id="installing-native-observers"></a>

# 네이티브 관측 확장 설치

[English](../extensions.md) | [한국어](extensions.md)

 gateway 코어를 다시 빌드하지 않고 선택형 메타데이터 관측 확장을 설치하고 활성화한다. 현재 패키지 관리는 소스 배포에 포함된 별도의 Python 명령을 사용하며, 실행은 gateway 바이너리가 담당한다. 네이티브 실행을 허가하기 전에 [구조와 신뢰 모델](extensions-design.md)을 읽는다.

<a id="requirements-and-supported-scope"></a>

## 요구사항과 지원 범위

Linux 또는 macOS, Python 3.11 이상과 정확히 호환되는 gateway 빌드를 사용한다. 소스 예제에는 저장소가 고정한 Rust 1.98.0 도구 체인이 필요하다. 패키지는 호스트 OS·아키텍처와 일치하는 `linux-x64`, `linux-arm64`, `macos-x64`, `macos-arm64` 중 하나여야 한다. CI는 Linux x64·ARM64와 macOS ARM64를 확인하며, macOS x64 식별 지원이 별도의 호스팅 검증을 의미하지는 않는다. Windows 확장 실행은 미지원이며 일반 gateway 동작은 유지된다.

이 안내는 `gateway-observer/v1`을 다룬다. 이 관측 확장은 HTTP 헤더 상태·시간 메타데이터를 받으며 프롬프트, 토큰, 응답 본문, 계정 한도나 도구 결과를 받지 않는다. Codex Pool, 동적 공급자 어댑터, hot reload, 원격 레지스트리, 자동 다운로드와 자격증명 접근은 이 패키지 역할에서 지원하지 않는다. 소스 관리자와 참조 관측 확장은 일반 gateway 바이너리 패키지에 바로 설치할 확장 바이너리로 포함되어 있지 않다.

다음 예제는 검토한 소스 체크아웃에서 절대 경로와 심볼릭 링크 없는 경로로 실행한다. 고유한 비공개 디렉터리와 의도적으로 비활성인 loopback 공급자를 사용한다. 실제 자격증명이나 유료 요청은 필요 없다. 패키지 설치·검사는 실행 파일을 시작하지 않는다.

<a id="prepare-a-local-example-package"></a>

## 로컬 예제 패키지 준비

```sh
ROOT="$(pwd -P)"
mkdir -p "$ROOT/.local"
DEMO="$(mktemp -d "$ROOT/.local/extension-demo.XXXXXX")"
CARGO_TARGET_DIR="$ROOT/target" cargo build --locked
CARGO_TARGET_DIR="$ROOT/target" cargo build --locked --example metadata_observer
python3 -B scripts/extension_manager.py package \
  --binary "$ROOT/target/debug/examples/metadata_observer" \
  --license-file "$ROOT/LICENSE" --output "$DEMO/package" \
  --id metadata-counter --version 0.1.0 > "$DEMO/package-result.json"
SHA="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["package_sha256"])' "$DEMO/package-result.json")"
```

도우미는 컴파일 결과를 독립된 파일로 복사하고 마지막 줄바꿈까지 포함한 정규 `extension.json`의 SHA-256을 출력한다. 여기서는 명시적으로 선택한 로컬 빌드에서 다이제스트를 얻는다. 타인의 패키지라면 신뢰하는 독립 경로에서 기대 다이제스트를 받아야 하며, 불신 바이트를 직접 해시하고 그 결과를 믿는 것은 배포자 검증이 아니다.

이 로컬 예제는 비공개 합성 시험을 위해 프로젝트 라이선스를 포함한다. 도우미는 릴리즈·라이선스 감사기가 아니다. 바이너리를 재배포하기 전에 [릴리즈 절차](release.md)에 따라 적용되는 제3자 근거와 해당 소스를 제공한다. 직접 준비한 단일 계층 패키지는 명시된 제한 안에서 해시된 고지 파일을 추가할 수 있다. 패키지 크기를 맞추기 위해 필수 고지를 누락해서는 안 된다. 도우미는 새 출력 디렉터리를 만들며 기존 패키지를 덮어쓰지 않는다.

<a id="install-approve-and-enable"></a>

## 설치와 승인 및 활성화

```sh
python3 -B scripts/extension_manager.py inspect \
  --package "$DEMO/package" --expected-sha256 "$SHA"
python3 -B scripts/extension_manager.py install \
  --store "$DEMO/store" --package "$DEMO/package" --expected-sha256 "$SHA"
python3 -B scripts/extension_manager.py enable \
  --store "$DEMO/store" --id metadata-counter --version 0.1.0 \
  --package-sha256 "$SHA" \
  --grant observe_http_metadata --grant write_private_state
python3 -B scripts/extension_manager.py status --store "$DEMO/store"
```

설치는 선언된 모든 파일을 검증하고 패키지를 비활성 상태로 둔다. 활성화는 두 권한의 명시적 승인을 요구하며 설치 바이트를 다시 확인하고 패키지별 상태 디렉터리를 만든 뒤 `active.json`을 쓴다. 코드는 실행하지 않는다. 이 파일은 `gateway-extension-lock/v1`을 사용하고 단조 증가하는 `generation`과 정확한 패키지 선택을 담는다.

상태 명령은 `runtime_checked:false`와 저장된 활성화 구성을 보고한다. 전체 설치 패키지 목록, 프로세스 상태 확인이나 실행 중 gateway가 최신 잠금을 사용한다는 증거가 아니다. 여러 버전을 설치할 수 있지만 패키지 ID마다 버전·다이제스트 하나만 활성화한다. 알 수 없는 규격 필드와 권한 확대는 조용히 무시하지 않고 거부한다.

<a id="inspect-and-start-the-opted-in-gateway"></a>

## 선택형 gateway 검사와 시작

```sh
cat > "$DEMO/gateway.toml" <<'TOML'
listen = "127.0.0.1:0"
local_token_env = "ARG_LOCAL_TOKEN"
[providers.demo]
base_url = "http://127.0.0.1:9"
api_key_env = "ARG_DEMO_KEY"
[models.demo]
provider = "demo"
upstream_model = "synthetic"
TOML
GATEWAY="$ROOT/target/debug/agent-response-gateway"
"$GATEWAY" check-config --config "$DEMO/gateway.toml" \
  --extensions-lock "$DEMO/store/active.json"
"$GATEWAY" manifest --config "$DEMO/gateway.toml" \
  --extensions-lock "$DEMO/store/active.json" > "$DEMO/execution-manifest.json"
ARG_LOCAL_TOKEN="$(python3 -c 'import secrets; print(secrets.token_urlsafe(32))')" \
ARG_DEMO_KEY="synthetic-unused-upstream-key" \
  "$GATEWAY" serve --config "$DEMO/gateway.toml" \
  --extensions-lock "$DEMO/store/active.json"
```

이 구성에는 모델 서버가 없으며 gateway 시작과 로컬 상태 확인만을 위한 예제다. 모델 요청은 실제 서비스로 우회하지 않고 실패해야 한다. 시작 시 해당 공급자를 검사하지 않는다. 첫 출력 줄은 선택한 포트와 확장 실행 다이제스트를 알려준다. 다른 셸에서 보고된 loopback 주소의 상태 엔드포인트에 요청하면 숫자형 관측이 생긴다. 나머지 명령을 계속하기 전에 일반 종료 신호로 gateway를 중지한다.

내장 호스트는 에이전트를 시작하기 전에 오프라인·준비 통지의 `execution_sha256`을 비교하고 지원되는 확장 스키마인지 확인하며 코어 실행 파일도 별도로 검증한다. 실제 manifest 경로나 구성 참조를 공개하지 않는다. `--extensions-lock`이 없으면 기존 manifest·readiness 형식과 확장 없는 경로 동작을 유지하며 설치 패키지를 자동 탐색하지 않는다.

예제는 첫 관측 이후 선택한 비공개 상태 디렉터리에 `counts.json`을 쓴다. 내용은 `schema`, `process_id`, `observed`, `status_counts`이며 본문과 토큰을 담지 않는다. 헤더 상태 카운터는 손실 가능한 운영 예제이지 성공한 추론의 합계가 아니다. gateway에 공급자 키가 있어도 상속 환경은 비운다.

<a id="disable-change-versions-and-recover"></a>

## 비활성화와 버전 변경 및 복구

```sh
python3 -B scripts/extension_manager.py disable \
  --store "$DEMO/store" --id metadata-counter
python3 -B scripts/extension_manager.py status --store "$DEMO/store"
```

비활성화는 다음 시작 스냅샷을 바꾸며 이미 실행 중인 확장을 종료할 수 없다. 적용하려면 gateway를 중지하고 다시 시작한다. 빈 활성화 잠금도 확장 실행 계약을 선택하며, 기존 계약을 사용하려면 `--extensions-lock`을 생략한다. 관리자는 패키지 버전이나 비공개 상태를 삭제하지 않는다.

업그레이드는 다른 정확한 패키지를 준비·설치하고 권한을 승인하여 해당 버전·다이제스트를 활성화한 뒤 gateway를 중지·시작한다. 롤백은 보관한 이전 패키지를 명시적으로 활성화하고 다시 시작한다. 상태는 패키지 다이제스트별로 분리되므로 이전 패키지로 돌아가면 그 패키지의 보관 상태를 사용하며 신버전 카운터와 합치지 않는다. 자동 이행, 제거, 정리나 디스크 보존 정책은 없다. 활성 패키지를 그 자리에서 수정해서는 안 된다.

손상된 활성화 파일은 오류이며 빈 구성으로 초기화할 허가가 아니다. 소유자를 중지한 후에만 검토한 호환 잠금을 복원한다. 실행 중 런타임을 무시하려고 잠금 파일을 삭제하지 않는다. 별도의 자격증명 이행 설계 없이 이 관측 카운터 복구 규칙을 후속 OAuth 토큰에 재사용해서는 안 된다.

<a id="validation-and-troubleshooting"></a>

## 검증과 문제 해결

```sh
python3 -B -m unittest discover -s scripts/tests -p 'test_extension*.py' -v
cargo test --locked extensions::
cargo build --locked --example metadata_observer
python3 -B scripts/extension_smoke.py \
  --binary target/debug/agent-response-gateway \
  --observer target/debug/examples/metadata_observer
```

통합 검사는 실제 관측 실행 파일을 패키징하여 실제 gateway와 합성 loopback 공급자에 연결한다. 오프라인 설치·검사, 기존 기본 manifest 유지, 정확한 네이티브 JSON·SSE 전달, 인증 분리, 고정 활성화, 독점 런타임 소유권, 관측 종료·멈춤의 영향 격리와 직접 자식 회수를 확인한다. 실제 OAuth, 한도나 모델 서비스는 사용하지 않는다. 저장소의 일반 형식, Clippy, Rust, Python, 라이선스와 공개 경계 검사도 계속 필수다.

패키지 오류는 잘못된 기대 다이제스트, 미지원 플랫폼·규격, 고지 누락, 목록 밖 파일, 비공개가 아닌 권한 또는 경로의 링크를 뜻할 수 있다. 실행시키기 위해 검사를 우회하지 않는다. 시작 규격 오류는 해당 선택형 gateway 실행을 거부한다. 실행 중 규격 실패는 고정 진단으로 기록하고 모델 경로가 아니라 해당 관측 확장을 비활성화한다. 통합 검사는 고정된 실패 단계만 밝히며 입력 경로, 자격증명이나 합성 본문을 출력하지 않는다. 이 기반을 범용 플러그인 SDK로 취급하기 전에 [구현 제한과 후속 역할](extensions-design.md)을 읽는다.

사용량 계측과 선택형 Recorder는 [토큰 사용량 계측 안내](usage-accounting.md)를
참조한다. Recorder 설치·로컬 커밋 보장·외부 전달은 HTTP 메타데이터 관찰과
별개의 계약이다.
