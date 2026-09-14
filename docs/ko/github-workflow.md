<a id="github-delivery-workflow"></a>

# GitHub 개발·검증·배포 절차

[English](../github-workflow.md) | [한국어](github-workflow.md)

이슈로 변경 범위를 정하고 PR로 구현을 검토한다. 이 문서는 브랜치 관리,
필수 검사와 릴리스 승인 절차를 설명한다.

<a id="branches-commits-and-reviews"></a>

## 브랜치·커밋·검토

범위가 정해진 PR마다 별도 worktree와 `codex/<purpose>` 브랜치를 사용한다.
사용자의 기존 변경과 독립적인 로컬 저장소를 보존한다. PR은 한 가지 검토 가능한
목적과 논리적 커밋 1–3개로 구성하고 구현·회귀 테스트를 같은 커밋에 둔다.
Merge commit으로 각 커밋의 이력을 보존한다.

작업 Issue에 연결한 draft PR로 시작한다. 문제, 결과 동작, 검증, 호환성과
복구 방법을 기록한다. 자동 병합 전에 정확한 head SHA를 검토한다. Head가
바뀌면 새로 검토하고, 갱신 전에 기존 자동 병합을 해제한다. 한 번에 하나씩
병합하며 다음 브랜치에 새 main을 반영한다. 공유 이력을 force-push하거나
필수 검사를 우회하지 않는다.

승인된 설계 범위의 변경은 코드 검토와 필수 CI 후 자동 병합할 수 있다.
공개 계약, 인증, 영속 상태, CI 쓰기 권한과 릴리스 변경은 명시적 승인이 필요하다.
설계 전용 작업을 구현보다 먼저 진행한다. 라벨은 추적 수단이며 승인 자체가 아니다.
에이전트 검토는 독립적인 사람의 GitHub 승인과 다르다. 작성자 계정으로 자기
PR을 승인하지 않는다.

<a id="required-checks"></a>

## 필수 검사

로컬 반복과 제출은 명시적인 영향 계획을 사용한다. 계획기는 선택 검사, 도구,
패키지 소비자와 범위 확대 이유를 보고한다. Cargo 의존성을 해석하지 않고 Git과
manifest를 읽는다. 알 수 없는 경로, 확인할 수 없는 revision, 의존성 및 검증
정책 변경은 전체 범위를 선택한다. `--base BASE --head HEAD`는 커밋 범위를
나타내며 이름 변경과 삭제는 영향을 받는 양쪽 경로를 포함한다. 범위 실행은
정확한 head의 깨끗한 checkout에서만 가능하며, 파일 경계 검사는 선택한 커밋의
blob을 읽는다. staged 및 범위 실행은 검증 도중 checkout이 바뀌면 실패한다.
전체 로컬 프로필은 릴리스 자격이 아니다.

```sh
python3.14 -B scripts/validation.py plan --worktree
python3.14 -B scripts/validation.py run --worktree
python3.14 -B scripts/validation.py run --staged
python3.14 -B scripts/validation.py plan --profile full --base BASE --head HEAD
python3.14 -B scripts/validation.py run --base BASE --head HEAD
```

실행 전 보고된 도구와 잠긴 npm 의존성을 준비한다. 웹 표현만 검사할 때에는 Cargo나
crate 라이선스 도구가 필요하지 않다. 로컬 결과는 `.local/validation/`에 기록한다.
전체 Python discovery에는 분리된 실제 SPDX 통합 테스트가 포함된다. PR과 main push
CI는 선택한 영향 계획을 실행한다. 매일 실행, 수동 전체 검사와 릴리스 자격 검증은
전체 실행을 유지한다. 영향 계획 자체는 테스트 실행 증거가 아니므로 실행 집합과 필수 검사 결과를
확인한다. 검증 정책의 `force_full`은 범위를 확대하는 방향으로만 작동한다.

전체 검증은 매일 18:17 UTC(한국 시간 다음 날 03:17)에 실행한다. 수동 CI도 전체
범위를 선택한다. 이 실행들과 릴리스 자격 검증은 오래된 PR/main 통합 검사와 별도
동시 실행 그룹을 사용한다. 정기 실패는 Actions 실패이며 자동 재시도나 릴리스가
아니다. 영향 분류 문제를 복구하려면 버전 관리 정책의 `force_full`을 true로 설정한다.
실행 범위를 확대하는 것이며 실패한 검사를 우회하지 않는다.

등록된 작업 계열은 `validation-plan`, `conformance-prepare`, `web-windows`, `targets`, `format`, `rust`,
`publication`, `licenses`, `codex-conformance`, `api-codecs`, `usage-recorder`,
`management-web`, `docs`, `package-smoke`다. 두 네이티브 행렬은
[공통 대상 정의](../../scripts/release_targets.py)의 플랫폼 4종을 모두 사용한다.
`ci-required`는 계획기와 선택한 작업 전체의 성공을 요구한다. 계획에서 명시적으로
제외한 작업만 건너뛸 수 있다. 누락·추가·예상 밖 건너뜀·취소·실패 결과는 집계를
차단한다. 필수 workflow 자체를 경로 필터로 생략하지 않는다. 첫 성공 실행 이후 해당
검사를 브랜치 보호에 등록한다.

네이티브 Rust 검사는 기존 지원 플랫폼 집합에서 변경 패키지와 소비자를 선택한다.
전체 검사는 원래 패키지·feature 행렬을 유지한다. Recorder는 별도 데이터베이스와
업그레이드 검사를 유지한다. Web 스타일과 독립적인 화면 컴포넌트에는 Rust fixture가
필요하지 않으며 API client와 인증을 소유한 애플리케이션 컨테이너는 실제 API fixture를
계속 사용한다.

job-seconds를 청구량으로 해석하지 않고 실제 Actions 시간을 확인할 수 있다.

```sh
python3.14 -B scripts/validation_metrics.py RUN_ID --output .local/validation/run.json
```

보고서는 필수 gate 대기 시간, 완료한 job 시간, 미완료 작업과 배포 대기를 구분한다.
단계별 관측을 보존하며 queue·cache·동시 실행 조건이 다른 결과를 통제된 성능 비교로
해석하지 않는다.

네이티브 적합성 검사는 editing, protocol/continuity, managed reasoning,
legacy migration의 독립 그룹 네 개로 실행한다. 외부 codec은 protocol과
editing/accounting 그룹을 사용한다. 기존 시나리오 명령은 각각 정확히 한 번
등록된다. 준비 작업이 같은 기본 feature 네이티브 입력을 빌드하고 고정 runtime을
검증한다. 소비자는 공유 실행 파일을 설치하기 전에 소스, run/attempt, 플랫폼,
도구 체계, 입력 lock과 모든 파일 해시를 검증한다. 준비 archive는 3일,
그룹 결과는 14일 보관한다. 준비 입력은 캐시된 테스트 성공이나 릴리스 서명이 아니다.

공유 gateway archive에는 대응 소스와 원래 의존성·Rust 도구 체계 고지를 포함한다.
각 그룹은 upstream 캐시에서 고정 Codex 묶음을 복원하고 검증하며, 공유 archive에
그 묶음을 다시 배포하지 않는다. 불변 legacy writer는 이행 그룹에서만 빌드한다.

Web 생산 작업은 정확한 소스 내보내기를 한 번 빌드한다. 네이티브 패키지 작업은
검사한 자산을 소비하며 별도 Windows 작업이 Web 빌드 호환성을 유지한다.
라이선스 도구 캐시는 의존성·빌드 캐시와 분리한다. 복원한 도구 버전을 사용 전에
검증하며 불일치하면 조용히 재설치하지 않고 실패한다.

라이선스 작업은 잠금 증거와 모든 스크립트 테스트를 검사하고 고정 개발 도구로
고지 묶음을 재현한다. Codex conformance는 실제 고정 실행 파일, 게이트웨이와
합성 업스트림을 사용한다. 모든 시나리오를 보고하며 하나라도 실패하면 집계를
차단한다. 패키지 검사는 커밋된 소스에서 후보를 만들고 압축파일, 대응 소스,
고지와 대상 SBOM을 검증한다. 후보 서명과 승인된 릴리스 승격은 별도 workflow다.
워크플로는 Ubuntu 24.04 x64·ARM64, macOS 15 ARM64와 Windows 2025 x64에서
Rust 1.98.0과 Python 3.14를 사용한다. 각 네이티브 CLI 시험은 설정, 준비 정보,
로컬 인증, 합성 경로 3종과 정상 종료도 검사한다.
공개 경계 검사는 전체 이력을 받으며 Actions는 SHA로 고정한다. 캐시는 OS,
아키텍처, 도구 체계와 관련 lockfile에 따라 구분한다.

문서 작업은 Node 24.21.0과 npm 11.19.0으로 검토한 언어 쌍, 정적 산출물,
로컬 미리보기 경계와 웹 의존성 고지를 검사한다. 검증한 사이트를 검토용으로
14일간 보관한다. Pages 배포 권한은 없으며 산출물 보관은 공개 승인이 아니다.

문서 배포는 별도의 main 전용 워크플로를 사용한다. 문서 입력이나 배포 정책 변경을
선택하고 명시적인 수동 배포도 지원한다. 검토한 문서, 공개 경계, Web 고지와 정적
산출물을 검사한 뒤 같은 산출물을 Pages용으로 포장한다. 제품 적합성이나 네이티브
패키지 작업에는 의존하지 않는다. 배포 후 제공되는 build manifest가 빌드의 소스
커밋과 정확한 manifest 바이트에 일치해야 한다.

배포는 [소비자 lock](../../.github/docs-pages-deploy.lock.json)에 기록한 전체 commit의
공개 [docs-actions workflow](https://github.com/novelKR/docs-actions)를 호출한다.
배포 작업에만 pages: write와 id-token: write를 부여한다. Pages를 GitHub Actions로
설정하고 github-pages 환경을 main으로 제한한다. 필수 검토자는 계속 공개를 제어하며
PR에서는 배포하지 않는다. 배포 실행은 중단 없이 직렬화하고 오래된 통합 검사 정리와
분리한다.

중앙 변경은 중앙 contracts CI 성공 후 workflow SHA, lock, 실행되지 않는 시험
사본을 함께 바꾸는 검토된 PR로 채택한다. 빌드 도구와 문서 검증은 이 저장소에
유지한다. 배포 후 실제 사이트 URL과 제공되는 build-manifest.json을 확인한다.
중앙 CI 성공만으로 실제 사이트가 공개됐다고 판단하지 않는다.

PR은 공급자 secret 없이 저장소 읽기 권한으로 실행한다. 신뢰하지 않는 PR
코드를 쓰기 자격 증명이나 소비자 호스트에서 실행하지 않는다. 런타임 운영 로그,
비공개 내용과 공급자 payload는 공개 산출물에 넣지 않는다. 실제 Codex를
실행하는 시험도 합성 입력과 모의 업스트림을 사용한다.

<a id="release-and-dependency-changes"></a>

## 릴리스와 의존성 변경

의존성과 Action 갱신은 별도 Dependabot PR로 받는다. 해결된 lockfile과
필수 라이선스 증거를 검토한다. 자동 갱신은 새 의존성이나 라이선스 조건의
승인이 아니다.

태그 후보가 성공하면 완성된 Pre-release를 자동 공개한다. 이후 보호된 배포
승인으로 같은 Release와 검증 파일을 재빌드 없이 승급한다. 후보, 공개와
정식 승급은 별도 워크플로를 사용한다. 채택 전에 소스·고지·출처를 검증하고 이전
검증 바이너리·설정·호환 상태를 보존한다. [배포 절차](release.md)를 따른다.

소비자 통합은 소비자 저장소에서 수행한다. 공개 Issue는 범용 계약을 추적할 수
있지만 비공개 구현이나 운영 기록을 연결하지 않는다. 게이트웨이 PR 병합만으로
소비자 수락이 완료되지는 않는다.
