# 배포 준비와 버전 고정

공개 소스와 PR CI는 운영 중이며 G18은 미공개 배포 후보 생성·검증을 제공한다.
G19는 서명된 후보와 사용자 승인 후 동일 바이트를 공개하는 preview 승격을 제공한다.
공개된 [후보 workflow 실행](https://github.com/novelKR/agent-response-gateway/actions/workflows/release-candidate.yml)에서
선택한 commit·run·attempt의 결과와 출처 증명을 확인한다. 서명된 후보가 있다는
사실은 최신 main의 릴리스나 바이너리 공개를 뜻하지 않는다. 소비자 생산 운영
수락과 상용 계약 체결도 별도 단계다.
현재 후보 생성과 정확한 검증 범위는 [패키징 계약](packaging.md)을 따른다.
[서명·승격 계약](release-promotion.md)은 최소 권한, 환경 승인, 검증과 복구 절차를 정한다.

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
[라이선스 관리 안내](../licensing/README.md)에 따라 먼저 준비한다.

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

의존성 고지는 Git에 보존한 [패키지별 기록과 원문](../licensing/README.md)을
기준으로 생성한다. 공개판과 별도 계약 배포 모두에 제3자 고지를 유지한다.

```sh
mkdir -p .local/release
python3 -B scripts/license_audit.py check
python3 -B scripts/license_audit.py bundle --output .local/release/licenses
python3 -B scripts/archive_notices.py .local/release/licenses .local/release/license-notices.tar
python3 -B scripts/check_public_boundary.py --archive .local/release/license-notices.tar
```

출력 디렉터리는 새 경로 또는 빈 경로여야 한다. 묶음의 manifest는 잠금 파일·
정책·패키지 기록·고지 파일의 해시를 포함한다. 동일한 입력으로 다른 빈 경로에
생성한 묶음이 일치하는지 확인한다. 공개 소스 archive와 함께 배포 자료에 포함한다.
고지 archive는 위 전용 도구로 호스트 소유자·시간·확장 메타데이터를 제거하며,
기존 파일을 덮어쓰지 않는다. 고지 원문의 바이트는 변경하지 않는다.

이 목록은 Cargo.lock 전체를 보수적으로 포함한다. 실제 바이너리에 포함된
항목만을 증명하는 SBOM으로 표시하지 않는다. 시스템 라이브러리의 정적·동적
링크, 컨테이너 패키지, 번들 실행 파일과 그 의존성은 실제 배포 산출물별로
추가 조사한다. 이 단계와 전체 법률 검토가 끝났다고 자동으로 표시하지 않는다.

권리자·기여 조건·별도 계약 권한을 확정한 뒤 외부 코드 기여 정책을 연다.
상용 계약에서도 제3자 구성요소의 기존 조건을 보존한다.

현재 `0.1.0`은 미출시 개발 버전이다. 대상별 package-smoke CI와 후보 파일은
정식 배포 승인이나 실제 공급자 qualification을 뜻하지 않는다. 공개·대체 조건 및 미확정 상태는
[버전별 라이선스 정책](../COMMERCIAL-LICENSING.md)에 기록한다. 검사 통과와
별도 계약 체결·권리 확보·상용 배포 가능 판정은 구분한다.

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
