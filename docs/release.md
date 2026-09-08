# 배포 준비와 버전 고정

현재는 로컬 기반 구현이며 공개 GitHub 저장소, 바이너리 릴리스,
attestation과 상용 계약이 준비되었다고 주장하지 않는다.

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
cargo build --release --locked
```

Linux·macOS CI workflow는 공개 가능한 fixture만 사용하며 공급자 secret을
요구하지 않는다. 호스팅된 CI가 실제 실행되기 전에는 성공으로 표시하지 않는다.
현재 checkout action은 2026-09-08 확인한 v4 태그의 커밋
`11d5960a326750d5838078e36cf38b85af677262`로 고정한다.

## 소스와 고지

AGPL의 Corresponding Source에는 실제 배포 버전을 빌드·설치·수정하는 데
필요한 소스와 관련 스크립트 등을 검토해 포함한다. [공식 AGPLv3](https://www.gnu.org/licenses/agpl-3.0.html)의
조건과 실제 배포 형태에 따라 확인한다. 실행 파일만 공개하거나 최신 브랜치
URL만 표시해 충분하다고 가정하지 않는다.

배포할 소스의 HTTPS 위치가 정해지면 설정의 `source_url`에 해당 버전을
제공하는 검증된 위치를 넣는다. `GET /`의 링크 안내가 네트워크 사용자에게
필요한 소스 접근 기회를 실제 제공하는지 확인한다. 링크의 표시나
`source_status`는 라이선스 이행 여부를 판정하는 기능이 아니다.

의존성 고지는 실제 배포 대상과 잠금 파일에 맞춰 준비한다. 프로젝트 내부의
무시된 디렉터리에 메타데이터를 내보낼 수 있다.

```sh
mkdir -p .local/release
cargo metadata --locked --format-version 1 > .local/release/cargo-metadata.json
```

이 메타데이터의 의존성·license·license_file과 패키지의 원문 고지를 조사한다.
배포물에 포함하는 제3자 코드에 필요한 저작권·허가 전문을 수집하고 누락을
검토한다. 메타데이터 생성은 완성된 SBOM, 고지 묶음 또는 법률 검토가 아니다.

권리자·기여 조건·별도 계약 권한을 확정한 뒤 외부 코드 기여 정책을 연다.
상용 계약에서도 제3자 구성요소의 기존 조건을 보존한다.

## 소비자의 채택과 복구

소비자는 특정 릴리스·해시를 선택하고 기동 전에 확인한다. 실행 시 최신
브랜치나 미검증 바이너리를 자동으로 가져오지 않는다. 준비 JSON과 HTTP
계약을 통해 실행하며 Codex·tenant 등 소비자별 내부 상태를 게이트웨이로
옮기지 않는다.

에이전트 호스트는 런타임과 게이트웨이의 검증된 조합을 선택한다. 백엔드
서비스는 기존 워크플로·외부 호출·배포 경계에서 채택한다. 운영 전환은 각 소비자의 승인을
거치며 이전 실행 파일과 설정 조합을 복구 가능하게 보존한다.

## 공개 소스 배포물

로컬 작업 디렉터리 전체를 압축하지 않는다. 검토한 공개 커밋의 추적 파일로
소스 archive를 만들고, [문서 관리](documentation.md)의 검사로 내부 경로와
지원하지 않는 링크·파일 유형이 없는지 확인한다. Cargo 패키지도
`cargo package --list --allow-dirty`로 실제 포함 목록을 확인한다.
`.gitignore`와 Cargo의 제외 규칙은 검사나 실제 배포 목록 확인을 대체하지 않는다.
