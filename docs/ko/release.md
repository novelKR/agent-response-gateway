<a id="distribution-preparation-and-version-pinning"></a>

# 배포 준비와 버전 고정

[English](../release.md) | [한국어](release.md)

배포물은 특정 소스 버전, 실행 파일, 설정, 고지와 검증된 빌드 출처를 연결한다.
[후보](packaging.md)를 만들고 [워크플로 결과](https://github.com/novelKR/agent-response-gateway/actions/workflows/release-candidate.yml)를
확인한 뒤, [서명·배포 절차](release-promotion.md)에 따라 승인된 파일을 게시한다.

<a id="unit-of-verification"></a>

## 검증할 단위

소스 커밋, Cargo 잠금 파일, Rust 1.98.0, 대상 플랫폼과 실행 파일 해시를
하나의 빌드 기록에 남긴다. 기본 검사는 fmt·Clippy·모의 공급자 테스트다.
게이트웨이 릴리스 시험과 에이전트·백엔드 소비자의 통합 수락을 구분한다.
모델 API 호출이 필요한 시험은 기본 PR CI와 분리한다.

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
python3 -B -m unittest discover -s scripts/tests -v
python3 -B scripts/license_audit.py check
cargo build --release --locked
```

Python은 3.11 이상을 사용하며, 라이선스 검사의 고정 도구와 원본 캐시는
[라이선스 관리 안내](../../licensing/README.ko.md)에 따라 먼저 준비한다.

Linux·macOS 워크플로는 공급자 자격 증명 없이 공개 가능한 시험 데이터를 사용한다.
Action 버전은 [CI 워크플로](../../.github/workflows/ci.yml)에 고정한다.

<a id="source-and-notices"></a>

## 소스와 고지

AGPL로 배포할 때는 해당 버전을 빌드·설치·수정하는 데 필요한 소스와
스크립트를 대응 소스(Corresponding Source)에 포함한다. [공식 AGPLv3](https://www.gnu.org/licenses/agpl-3.0.html)의
조건과 실제 배포 형태에 따라 확인한다. 실행 파일만 공개하거나 최신 브랜치
URL만 표시해 충분하다고 가정하지 않는다.

배포할 소스의 HTTPS 위치가 정해지면 설정의 `source_url`에 해당 버전을
제공하는 검증된 위치를 넣는다. `GET /`의 링크 안내가 네트워크 사용자에게
필요한 소스 접근 기회를 실제 제공하는지 확인한다. 링크의 표시나
`source_status`는 라이선스 이행 여부를 판정하는 기능이 아니다.

의존성 고지는 Git에 보존한 [패키지별 기록과 원문](../../licensing/README.ko.md)을
기준으로 생성한다. 공개판과 별도 계약 배포 모두에 제3자 고지를 유지한다.

```sh
mkdir -p .local/release
python3 -B scripts/license_audit.py check
python3 -B scripts/license_audit.py bundle --output .local/release/licenses
python3 -B scripts/archive_notices.py .local/release/licenses .local/release/license-notices.tar
python3 -B scripts/check_public_boundary.py --archive .local/release/license-notices.tar
```

출력 디렉터리는 새 경로 또는 빈 경로여야 한다. 묶음의 명세는 잠금 파일·
정책·패키지 기록·고지 파일의 해시를 포함한다. 동일한 입력으로 다른 빈 경로에
생성한 묶음이 일치하는지 확인한다. 공개 소스 압축파일과 함께 배포 자료에 포함한다.
고지 압축파일은 위 전용 도구로 호스트 소유자·시간·확장 메타데이터를 제거하며,
기존 파일을 덮어쓰지 않는다. 고지 원문의 바이트는 변경하지 않는다.

이 목록은 Cargo.lock 전체를 보수적으로 포함한다. 실제 바이너리에 포함된
항목만을 증명하는 SBOM으로 표시하지 않는다. 시스템 라이브러리의 정적·동적
링크, 컨테이너 패키지, 번들 실행 파일과 그 의존성은 실제 배포 산출물별로
추가 조사한다. 이 단계와 전체 법률 검토가 끝났다고 자동으로 표시하지 않는다.

[기여 정책](../../CONTRIBUTING.ko.md)에 따라 기여 권한을 확인한다.
공개 배포는 AGPL을, 계약 대상 상용 배포는 [별도 계약](../../COMMERCIAL-LICENSING.ko.md)을
따른다. 양쪽 모두 제3자 조건을 지켜야 한다.

`0.1.0`은 개발 버전이다. 미리보기 버전은 승인을 거친 배포 워크플로로 게시한다.

<a id="consumer-adoption-and-recovery"></a>

## 소비자의 채택과 복구

소비자는 특정 릴리스·해시를 선택하고 기동 전에 확인한다. 실행 시 최신
브랜치나 미검증 바이너리를 자동으로 가져오지 않는다. 준비 JSON과 HTTP
계약을 통해 실행하며 Codex·테넌트 등 소비자별 내부 상태를 게이트웨이로
옮기지 않는다.

에이전트 호스트는 런타임과 게이트웨이의 검증된 조합을 선택한다. 백엔드
서비스는 기존 워크플로·외부 호출·배포 경계에서 채택한다. 운영 전환은 각 소비자의 승인을
거치며 이전 실행 파일과 설정 조합을 복구 가능하게 보존한다.

<a id="public-source-artifacts"></a>

## 공개 소스 배포물

로컬 작업 디렉터리 전체를 압축하지 않는다. 검토한 공개 커밋의 추적 파일로
소스 압축파일을 만들고, [문서 관리](documentation.md)의 검사로 내부 경로와
지원하지 않는 링크·파일 유형이 없는지 확인한다. Cargo 패키지도
`cargo package --list --allow-dirty`로 실제 포함 목록을 확인한다.
`.gitignore`와 Cargo의 제외 규칙은 검사나 실제 배포 목록 확인을 대체하지 않는다.
