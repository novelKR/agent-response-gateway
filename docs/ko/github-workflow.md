<a id="github-delivery-workflow"></a>

# GitHub 개발·검증·배포 절차

[English](../github-workflow.md) | [한국어](github-workflow.md)

[로드맵](roadmap.md)은 M0–M7을 정의한다. GitHub 마일스톤과 작업 Issue는
구현 상태를 검증, qualification, 채택과 구분해 추적한다.

<a id="branches-commits-and-reviews"></a>

## 브랜치·커밋·검토

범위가 정해진 PR마다 별도 worktree와 `codex/m<N>-<purpose>` 브랜치를 사용한다.
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

필수 작업은 `format`, `rust-linux`, `rust-macos`, `publication`, `licenses`,
`codex-conformance`, `docs`와 Linux/macOS의 `package-smoke` 행렬이다.
`ci-required`는 명시한 필수 선행 작업 전체가 성공해야 통과한다.
누락·추가·건너뜀·취소·실패 작업은 집계를 차단한다.
경로 필터로 필수 검사를 조용히 생략하지 않는다. 첫 성공 실행 이후 해당
검사를 브랜치 보호에 등록한다.

라이선스 작업은 잠금 증거와 모든 스크립트 테스트를 검사하고 고정 개발 도구로
고지 묶음을 재현한다. Codex conformance는 실제 고정 실행 파일, gateway와
합성 upstream을 사용한다. 모든 시나리오를 보고하며 하나라도 실패하면 집계를
차단한다. 패키지 검사는 커밋된 소스에서 후보를 만들고 archive, 대응 소스,
고지와 대상 SBOM을 검증한다. 후보 서명과 승인된 릴리스 승격은 별도 workflow다.
기본 환경은 Rust 1.98.0, Python 3.14, Ubuntu 24.04 x64와 macOS 15 ARM64다.
공개 경계 검사는 전체 이력을 받으며 Actions는 SHA로 고정한다. 캐시는 OS,
아키텍처, 도구 체계와 관련 lockfile에 따라 구분한다.

문서 작업은 Node 24.21.0과 npm 11.19.0으로 검토한 언어 쌍, 정적 산출물,
로컬 미리보기 경계와 웹 의존성 고지를 검사한다. 검증한 사이트를 검토용으로
14일간 보관한다. Pages 배포 권한은 없으며 산출물 보관은 공개 승인이 아니다.

PR은 공급자 secret 없이 저장소 읽기 권한으로 실행한다. 신뢰하지 않는 PR
코드를 쓰기 자격 증명이나 소비자 호스트에서 실행하지 않는다. 런타임 운영 로그,
비공개 내용과 공급자 payload는 공개 artifact에 넣지 않는다. 실제 Codex를
실행하는 시험도 합성 입력과 모의 upstream을 사용한다.

<a id="release-and-dependency-changes"></a>

## 릴리스와 의존성 변경

의존성과 Action 갱신은 별도 Dependabot PR로 받는다. 해결된 lockfile과
필수 라이선스 증거를 검토한다. 자동 갱신은 새 의존성이나 라이선스 조건의
승인이 아니다.

후보 빌드와 정식 릴리스는 구분한다. 보호된 배포 승인은 검증한 동일 바이너리
digest를 재빌드 없이 승격한다. 채택 전에 소스·고지·출처를 검증하고 이전
검증 바이너리·설정·호환 상태를 보존한다. [배포 절차](release.md)를 따른다.

소비자 통합은 소비자 저장소에서 수행한다. 공개 Issue는 범용 계약을 추적할 수
있지만 비공개 구현이나 운영 기록을 연결하지 않는다. Gateway PR 병합만으로
소비자 수락이 완료되지는 않는다.
