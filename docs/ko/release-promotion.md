<a id="signed-candidates-and-protected-preview-promotion"></a>

# 서명 후보와 보호된 preview 승격

[English](../release-promotion.md) | [한국어](release-promotion.md)

G19는 수동 실행 workflow 두 개를 제공한다. 후보 workflow는 선택한 main
commit의 push CI 성공 후 그 commit을 빌드·서명한다. Preview 승격은 보존한
후보를 검증하고 receipt를 제시한 뒤 보호된 release 환경 승인을 기다려 같은
바이트를 prerelease로 게시한다. 실제 공급자나 소비자 운영을 qualification하지 않는다.

<a id="build-and-signing-boundaries"></a>

## 빌드와 서명 경계

후보는 전체 expected_commit SHA를 받고 이 저장소의 main에서만 실행한다.
SHA가 workflow 소스와 같고 해당 commit의 최신 push CI가 성공해야 한다.
후보가 실패하면 새 dispatch가 필요하며 기존 run 재실행은 거부한다. Run 안의
artifact 이름은 불변이며 30일 보존한다.

Ubuntu 24.04와 macOS 15 native 작업이 PR cache 없이 [G18 빌더](packaging.md)를
실행한다. 각 distribution archive는 바이너리 묶음, 대응 소스, 고지, 범위가
명시된 SBOM, 후보 manifest와 체크섬을 포함한다. Target descriptor는 distribution
SHA-256, 내부 후보 SHA-256, 소스 commit, version, target과 Cargo.lock hash를 연결한다.

빌드 작업 권한은 contents:read다. 별도 서명 작업에는 contents:read,
actions:read, id-token:write, attestations:write가 있다. 다운로드한 배포물을
실행하지 않고 검사한 뒤 고정 actions/attest로 archive와 descriptor를 모두
서명한다. 정확한 파일과 함께 Sigstore bundle을 보존한다. 후보 작업에는
릴리스 쓰기 권한이 없다.

검증은 GitHub CLI attestation verifier로 예상 저장소·서명 workflow·main
source ref·source/signer commit digest·hosted runner를 확인한다. SLSA
invocation도 정확한 run ID와 attempt에 연결한다. 체크섬만 있는 후보나 PR
후보는 통과할 수 없다. 로컬 합성 시험은 실제 서명 생성·검증을 증명하지
않는다. 성공한 서명 workflow와 다운로드 바이트의 독립 검증이 그 증거다.

<a id="review-and-promotion"></a>

## 검토와 승격

Preview 승격 입력은 candidate_run_id, expected_commit, release_tag다.
Tag는 후보 버전의 vVERSION-preview.N 형식이어야 한다. 별도 검증 소비자
수락 계약이 생기기 전까지 operational track은 명시적 오류다.

첫 작업은 contents:read와 actions:read만 가진다. 성공한 main 후보 workflow를
확인하고 서명된 두 target을 검증한 뒤, 자산 여섯 개의 hash를 가진 promotion.json을
요약과 불변 artifact로 내보낸다. 게시 작업 승인 전에 receipt와 후보 구성·
qualification 한계를 검토한다. Publish 작업만 contents:write를 가지며
release 환경을 통해 검증 receipt와 원래 자산을 받는다. 승인 후 출처·바이트를
다시 검증하고 재빌드하지 않는다.

Release 환경은 지정한 저장소 소유자의 검토, 보호 브랜치와 관리자 우회 해제를
요구한다. 단일 소유자가 자기 dispatch를 검토할 수 있지만 이는 명시적 배포
승인이며 독립 코드 검토가 아니다. Helper는 reviewer·branch 정책과 해당 환경·
승격 run의 실제 GitHub 승인 이력을 검사한다. 환경 이름 변수만으로는 부족하다.
REST 응답에 없는 관리자 우회 설정은 저장소 설정에서 확인한다. 승격 dispatch
전에 환경을 구성하며 workflow 병합 자체가 환경 설정을 수행하지 않는다.

게시 시작 시 후보 소스는 현재 main과 같아야 한다. Main이 바뀌면 새 후보가
필요하다. 게시 과정은 draft 생성, 동일한 누락 자산만 덮어쓰기 없이 업로드,
GitHub 자산 digest 확인, make_latest=false prerelease 게시, 릴리스·전체 자산·
정확한 tag commit 재조회 순서다. 다른 소스·메타데이터·digest의 tag나 release는 거부한다.

<a id="failure-and-recovery"></a>

## 실패와 복구

업로드 응답을 잃으면 draft와 불확실한 workflow 결과가 남는다. 자동 재시도는
없다. Draft를 확인하고 새 환경 승인을 가진 승격을 dispatch한다. 정확히
일치하는 draft는 누락 자산만 이어 올릴 수 있다. 완료 릴리스가 일치하면 다시
업로드하지 않는다. 예상 밖 자산, 바뀐 바이트와 충돌 tag는 복구를 중단하며
자동 덮어쓰기·삭제하지 않는다. 게시 후 실패는 release/tag를 재조회한 뒤 판단한다.

게시를 멈추려면 수동 승격 workflow를 끄거나 release 승인을 보류한다. 조사할
후보와 소스 commit을 유지하고 게시한 버전을 재작성하지 않는다. 소비자 롤백은
자신의 채택 절차로 이전 검증 실행 파일·설정을 선택한다. Gateway 자동화는
소비자를 재시작하거나 마이그레이션하지 않는다.

플랫폼 동작은 GitHub의 [출처 증명 검증](https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations/verifying-the-provenance-of-artifacts),
[배포 환경 보호](https://docs.github.com/en/actions/reference/workflows-and-actions/deployments-and-environments),
[release API](https://docs.github.com/en/rest/releases/releases)를 따른다. 이 저장소의
정확한 workflow·helper·receipt가 더 좁은 승격 계약을 정의한다. 최초 서명 후보,
preview 게시, 공급자 qualification과 운영 수락은 각각 별도 사건으로 보고한다.
