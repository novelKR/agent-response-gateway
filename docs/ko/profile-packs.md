<a id="change-versions-and-recover"></a>
<a id="compatibility-profile-packs"></a>
<a id="import-declarations-and-start"></a>
<a id="package-contract"></a>
<a id="prepare-and-activate-a-local-fixture"></a>

# 호환성 프로파일 팩

[English](../profile-packs.md) | [한국어](profile-packs.md)

명시적으로 설치한 비실행 데이터 패키지로 기능 선언과 도구 호환 정책을
재사용한다. 공급자 URL, 모델명, 인증, 라우팅, 연속 대화 저장소와 사용량 기록은
호스트 설정에 남는다. 프로파일 팩은 게이트웨이가 지원하는 Linux, macOS,
Windows 플랫폼에서 동작한다.

## 패키지 계약

`gateway-profile-pack/v1` 패키지는 UTF-8 JSON 파일 하나다. `id`, 세 부분의 숫자로
구성된 `version`, 이름이 있는 `capabilities`와 `policies` export, `evidence`,
그리고 `notices` 안의 원본 `LICENSE`와 선택형 `NOTICE` 본문을 포함한다.
package 명령은 원본 JSON을 키 정렬·공백 최소화·마지막 줄바꿈을 적용한 JSON으로
정규화한다. SHA-256은 이 바이트 전체를 대상으로 한다. 중복 키, 알 수 없는 필드,
지원하지 않는 계약은 검증에 실패한다. 실행 파일, 진입점, 권한 부여, 자격 증명
참조, 공급자 바인딩이나 파일 경로를 패키지에 넣을 수 없다.

기능 export는 `api`, 선택형 `reasoning_contract`, `context_window`,
`max_output_tokens`, `tested_codex_version`과 기존 `support` 맵을 포함한다.
프로파일 버전은 패키지 버전이다. 정책은 기존
[도구 정책 계약](protocol.md#checked-responses-tools)을 사용한다. 생략한 기능은
미지원이며 패키지로 어댑터나 문법 규칙을 추가할 수 없다. 기존 경로 검증은
호환되지 않는 API·모델·정책 바인딩을 계속 거절한다.

근거 항목은 길이가 제한된 `description`, 선택형 HTTPS `source_url`, 선택형
`artifact_sha256`을 포함한다. 시험한 Codex 버전을 포함하여 모두 게시자의 주장이다.
게이트웨이는 근거를 가져오거나 적합성 시험을 실행하거나 공급자 동작을 증명하지
않는다. 다른 사람의 패키지를 선택할 때는 신뢰할 수 있는 경로로 예상 digest를
확보해야 한다. 설치와 바이트 검증은 게시자 신원 확인이나 재배포 권리 승인이 아니다.

패키지당 256 KiB, export 합계 64개, 근거 16개, 활성 팩 16개로 제한한다.
export 이름과 패키지 ID는 영문 소문자·숫자·하이픈을 사용하고 영문자로 시작하며
Windows 장치 이름을 제외한다. 고지 본문은 내장 데이터이며 경로로 해석하지 않는다.
운영자가 관리하고 일반적인 파일 접근 제어를 적용한 로컬 저장소를 사용한다.
팩에는 비밀을 넣지 않는다. 정적 심볼릭 링크, Windows reparse point와 일반 파일이
아닌 패키지를 거절한다. 같은 저장소를 동시에 변경할 수 있는 악의적인 사용자를
격리하는 기능은 아니다.

## 로컬 합성 패키지 준비와 활성화

소스 체크아웃에서 게이트웨이를 빌드하고 합성 패키지 원본을 준비한다.
다음 POSIX 셸 예제는 데이터 준비에만 Python을 사용한다. 게이트웨이의
`profile-pack` 명령 자체에는 Python 설치가 필요 없다. Windows에서도 같은 JSON과
CLI 인자를 사용하되 실행 파일 경로를 해당 플랫폼에 맞춘다.

```sh
cargo build --locked
mkdir -p .local/profile-demo
python3 - <<'PY'
import json
from pathlib import Path
source = {
    "schema": "gateway-profile-pack/v1", "id": "local-fixture", "version": "1.0.0",
    "capabilities": {"functions": {
        "api": "responses", "context_window": 8192, "max_output_tokens": 2048,
        "tested_codex_version": "synthetic-not-qualified",
        "support": {"function_tools": "native", "tool_choice": "native"}}},
    "policies": {"tools": {"version": 1, "tools": {
        "custom_input": "function_json", "namespaces": "flatten"}}},
    "evidence": [], "notices": {"LICENSE": Path("LICENSE").read_text(encoding="utf-8")}}
Path(".local/profile-demo/source.json").write_text(json.dumps(source), encoding="utf-8")
PY
target/debug/agent-response-gateway profile-pack package \
  --source .local/profile-demo/source.json --output .local/profile-demo/pack.json
target/debug/agent-response-gateway profile-pack inspect \
  --package .local/profile-demo/pack.json
target/debug/agent-response-gateway profile-pack install \
  --package .local/profile-demo/pack.json --store .local/profile-demo/store
```

패키지 생성은 기존 출력 파일을 거절한다. 설치는
`packages/<id>/<version>/<sha256>.json`에 불변 바이트를 저장하고 비활성 상태로
남긴다. 같은 검증된 바이트를 다시 설치해도 무방하다. 이 로컬 합성 패키지에는
package 명령이 출력한 정확한 digest를 선택한다.

```sh
target/debug/agent-response-gateway profile-pack enable \
  --store .local/profile-demo/store --id local-fixture --version 1.0.0 \
  --sha256 <exact-package-sha256>
target/debug/agent-response-gateway profile-pack status --store .local/profile-demo/store
```

enable은 `gateway-profile-pack-lock/v1`, 단조 증가하는 `generation`, 정렬된
정확한 ID·버전·digest 항목으로 `active.json`을 기록한다. 패키지 ID마다 하나의
바인딩만 활성화할 수 있다. 레지스트리 조회, 자동 다운로드, 버전 범위, 암묵적
활성화나 hot reload는 없다. status는 저장된 활성 스냅샷을 검증하며 실행 중인
프로세스를 검사하거나 비활성 패키지를 나열하지 않는다.

## 선언 가져오기와 시작

명시적인 import 별칭으로 호스트 설정을 저장한다. 아래의 비활성 루프백 주소는
오프라인 검사에 적합하며 추론 서비스가 없다.

```toml
[providers.demo]
base_url = "http://127.0.0.1:9/v1"
api_key_env = "ARG_DEMO_KEY"
[models.writer]
provider = "demo"
upstream_model = "host-selected-model"
auth = "bearer"
capability_profile = "local-functions"
compatibility_policy = "local-tools"
[capability_profile_imports.local-functions]
pack = "local-fixture"
export = "functions"
provider = "demo"
upstream_model = "host-selected-model"
[compatibility_policy_imports.local-tools]
pack = "local-fixture"
export = "tools"
```

`check-config`, `manifest`, `serve`에 일반 `--config` 인자와 함께
`--profile-packs-lock .local/profile-demo/store/active.json`을 전달한다.
플래그가 없으면 해석되지 않은 import 때문에 실패한다. 인라인 선언과 가져온
선언은 서로 다른 별칭으로 공존할 수 있다. 별칭이 겹치면 덮어쓰거나 deep merge하지
않고 실패한다. 비활성 패키지, 없는 export, 잘못된 종류의 export도 거절한다.

명시적인 lock을 선택하면 활성 팩 목록이 비어 있어도
`gateway-embedded-manifest/v5`와 `gateway-ready/v5`를 사용한다. 설정은 로컬
패키지 경로 없이 고정된 활성 상태, 패키지 전체, 근거의 지위와 호스트 import를
바인딩한다. 영향을 받는 각 경로는 선택한 export와 패키지 digest를 어댑터 식별에
포함한다. 선택한 패키지 바이트를 바꾸면 해당 경로의 기존 연속 대화 origin은
유효하지 않게 된다. generation만 바꾸면 설정 digest는 바뀌지만 경로 origin은
바뀌지 않는다. 이미 실행 중인 프로세스는 처음 검증한 스냅샷을 유지한다.

`--extensions-lock`도 선택하면 외부 스키마는 `gateway-extended-manifest/v5`와
`gateway-extended-ready/v5`다. 기존 observer와 사용량 recorder의 권한·프로토콜·
공개 전 기록 장벽은 유지된다. 팩은 네이티브 프로세스를 추가하거나 managed replay
형식을 바꾸지 않는다. 호스트는 v5를 지원하고 검사 결과와 준비 응답의 digest를
비교한 뒤 에이전트를 시작해야 한다. 프로파일 팩 lock을 선택하지 않은 설정은
기존 manifest와 준비 응답 버전을 유지한다.

## 버전 변경과 복구

선택한 ID를 비활성화하고 명시적으로 고른 새 버전·digest를 활성화한 다음,
호스트 manifest를 검사하고 게이트웨이를 재시작한다. 기존 활성 바인딩을 암묵적으로
교체하는 명령은 없다. 비활성화는 설치된 바이트를 유지하며 손상된 패키지 바인딩도
제거할 수 있다. 남아 있는 활성 패키지는 여전히 검증을 통과해야 한다. 롤백하려면
보관된 이전 바이트를 명시적으로 다시 선택하고 재시작한다. 이전 대화를 재개해도
되는지 판단하는 책임은 호스트에 있다.

```sh
target/debug/agent-response-gateway profile-pack disable \
  --store .local/profile-demo/store --id local-fixture
```

`activation.writer`로 활성화 쓰기를 직렬화한다. 새 lock을 `activation.next`에
기록하고 동기화한 후 `active.json`으로 원자적으로 이름을 바꾼다. Unix에서는
디렉터리도 동기화한다. 모든 플랫폼의 전원 장애 내구성을 보장하는 것은 아니다.
쓰기가 중단되면 표식이나 임시 파일이 남으며 후속 쓰기는 명시적으로 실패한다.
관리 명령을 멈추고 정규형 활성 lock과 보관된 패키지 해시를 검사하며 중단된
작업의 근거를 보존한 뒤, 남은 표식·임시 파일을 제거하고 다시 시도한다. 시작할 때
패키지 바이트를 복구하거나 대체하지 않는다. 잘못된 lock은 운영자가 명시적으로
복구해야 한다.
