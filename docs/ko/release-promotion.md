<a id="signed-candidates-and-preview-releases"></a>
<a id="signed-candidates-and-protected-preview-promotion"></a>
<a id="서명-후보와-보호된-preview-승격"></a>
<a id="서명-후보의-미리보기-배포"></a>
<a id="tagged-builds-and-release-promotion"></a>

# 태그 빌드와 릴리스 승급

[English](../release-promotion.md) | [한국어](release-promotion.md)

기존 소스 커밋에 버전 태그를 올리면 네이티브 [패키지](packaging.md) 4종을
빌드·검증·서명한다. 후보가 성공하면 GitHub Pre-release를 자동 공개한다.
별도 수동 워크플로에서 승인을 받은 뒤 같은 태그, Release ID와 다운로드 파일을
정식 Release로 승급한다.

<a id="build-and-signing-boundaries"></a>

## 빌드와 서명 경계

태그는 v 뒤에 Cargo.toml의 패키지 버전을 붙인 값과 정확히 같아야 한다.
vX.Y.Z와 vX.Y.Z-rc.1 같은 사전 버전을 모두 받는다. 빌드 메타데이터가 있는
태그는 지원하지 않는다. 소스 커밋은 main에 포함되어야 하며, 그 정확한 커밋의
최신 main push CI가 성공해야 한다. 이후 main이 진행되어도 재빌드할 필요가
없다. 버전 태그를 다른 커밋으로 옮기는 것은 거부한다.

Release candidate는 버전 태그 push로 시작한다. 후보가 실패하면 원인을 확인한
뒤 같은 기존 태그를 대상으로 새 수동 실행을 시작한다. 브랜치에서의 dispatch와
기존 실행의 재시도는 거부한다. 후보 산출물은 30일 보관한다. 이미 공개한 후보를
새 빌드로 교체하지 않는다.

네이티브 빌드 4종 모두 Rust 1.98.0, 잠금 의존성과 준비된 라이선스 검사 도구를
사용하며 PR 캐시를 사용하지 않는다. 각 배포물에는 실행 압축파일, 대응 소스,
고지, 범위를 명시한 SBOM, 후보 명세와 체크섬이 들어간다. 대상별 명세는 배포물과
내부 후보 해시, 소스 커밋, 버전, 대상과 Cargo.lock 해시를 연결한다.

빌드 작업 권한은 contents:read다. 별도 서명 작업에는 contents:read,
actions:read, id-token:write와 attestations:write가 있다. 다운로드한 파일을
실행하지 않고 검사한 뒤 배포 압축파일과 명세를 서명한다. Sigstore 증명을 해당
파일과 함께 보존한다. 후보 작업에는 릴리스 쓰기 권한이 없다. GitHub CLI는
저장소, 서명 워크플로, refs/tags 소스 참조, 소스·서명 커밋, hosted runner와
정확한 SLSA 실행 ID·attempt를 확인한다. PR 산출물은 이 계약을 충족할 수 없다.

신뢰하는 체크아웃에서 다운로드 대상을 검증하려면 선택한 디렉터리에 해당
배포 압축파일, target.manifest.json과 target.sigstore.jsonl만 둔다.
명령에는 attestation을 지원하는 GitHub CLI와 Python 3.11+가 필요하다.
릴리스 명세의 소스 커밋과 후보 실행 ID를 넣고 사용할 대상·태그를 명시적으로
선택한다. 아래 Windows 태그는 예시다.

```sh
python3 -B scripts/release_provenance.py verify \
  --directory .local/downloaded-target \
  --commit SOURCE_COMMIT --target x86_64-pc-windows-msvc \
  --run-id CANDIDATE_RUN_ID --attempt 1 --tag v0.1.0
```

<a id="review-and-promotion"></a>

## 검토와 승격

Publish prerelease는 Release candidate 성공 후 실행된다. 읽기 권한을 가진
검증 작업이 신뢰하는 main 워크플로의 검증기를 사용한다. 같은 커밋·버전·실행에서
생성된 서명 대상 4종이 모두 필요하다. 불변 release-manifest.json에 이 연결과
대상 파일 12개의 해시를 기록한다. 명세 자체가 열세 번째 릴리스 파일이며
변할 수 있는 릴리스 상태는 명세에 넣지 않는다.

게시 작업만 contents:write를 가진다. 준비된 파일을 다시 검증하고 draft를 만든
뒤 누락 파일을 덮어쓰기 없이 올린다. 전체 파일 목록과 GitHub 해시를 확인하고
make_latest=false인 prerelease로 공개한다. 다운로드한 실행 파일이나 빌드
스크립트는 실행하지 않는다. 일부 플랫폼을 빼고 부분 릴리스를 공개하지 않는다.

Promote release는 main에서 release_tag를 입력해 수동 실행한다. 태그가 일반
버전 vX.Y.Z 형식인 공개 릴리스만 받는다. 파일을 다운로드해 서명과 명세를
검증하고, 명세를 보여준 뒤 보호된 release 환경의 승인을 기다린다. 승인 후
공개 파일을 다시 다운로드·검증하고 명세 해시가 이전과 같은지 확인한다.
같은 Release에서 prerelease=false와 make_latest=legacy만 변경한다. 재빌드,
태그 이름 변경이나 파일 교체는 하지 않는다. -rc.1로 끝나는 태그는 시험판으로
유지한다. 예를 들어 v0.1.0은 먼저 Pre-release로 공개하고 나중에 같은 v0.1.0
태그와 파일로 정식 승급할 수 있다. 이 예시는 실제 버전 공개를 알리는 내용이 아니다.

Release 환경은 지정한 저장소 소유자의 검토와 보호 브랜치를 요구한다. 검증기는
해당 정책과 이 실행·환경에 대한 실제 승인 이력을 확인한다. 환경 이름 변수만으로는
부족하다. 소유자는 자기 수동 실행을 승인할 수 있다. 이는 배포 권한 부여이며
독립 코드 검토가 아니다. 관리자 우회를 끈 상태로 유지하고 GitHub 설정에서
확인한다. 환경 REST 응답에는 그 설정이 없다. 워크플로 병합이 환경을 구성하지는
않는다. 승인은 배포 채널을 변경하며 실제 공급자 검증과 소비자 운영 수락은 별도다.

<a id="failure-and-recovery"></a>

## 실패와 복구

게시 실패 시 draft를 확인하고 원래 candidate_run_id로 main의 Publish prerelease를
수동 실행한다. 성공한 후보를 따로 보관하므로 게시 복구 과정에서 재빌드하지 않는다.
기존 이름과 해시가 일치하는 draft의 누락 파일만 이어 올린다. 완성된 릴리스가
일치하면 재업로드 없이 성공으로 인식하며, 이미 정식 승급한 릴리스는 정식 상태를
유지한다. 예상 밖 파일, 바뀐 해시와 충돌 태그는 교체·삭제 없이 복구를 중단한다.
태그별 게시·승급 작업은 쓰기 작업을 직렬화한다. 불확실한 쓰기를 자동 재시도하지 않는다.

정식 승급은 GitHub Release 파일을 읽으므로 Actions 산출물의 30일 보관 기한에
의존하지 않는다. 미공개 후보가 만료됐다면 원래 서명 파일을 복구한 뒤 게시해야 한다.
일부 업로드된 후보를 새 빌드로 대체할 수 없다. 정식 승급 실패 후에는 새 환경
승인을 받아 다시 시작할 수 있으며, 이미 완료된 동일 상태는 파일 수정 없이 인식한다.

자동 공개를 멈추려면 Publish prerelease를 비활성화하고, 정식 승급을 보류하려면
release 환경 승인을 보류한다. 후보 증거와 공개 소스 커밋을 보존한다. 소비자는
자신의 채택 절차를 통해 이전에 검증한 실행 파일과 설정으로 복구한다.

GitHub의 [출처 증명 검증](https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations/verifying-the-provenance-of-artifacts),
[환경 보호](https://docs.github.com/en/actions/reference/workflows-and-actions/deployments-and-environments),
[release API](https://docs.github.com/en/rest/releases/releases)가 플랫폼 동작을 설명한다.
워크플로, 검증기와 보관한 명세가 이 저장소의 배포 계약을 정의한다. 모의 시험은
거부 조건과 상태 전환을 검증한다. 실제 태그 빌드, 서명 공개와 승인 승급은 별도
실행 증거이며 설정을 병합했다고 수행된 것으로 처리하지 않는다.
