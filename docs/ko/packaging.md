<a id="unpublished-release-candidates"></a>

# 미공개 배포 후보

[English](../packaging.md) | [한국어](packaging.md)

패키징 도구는 Ubuntu 24.04의 x86_64-unknown-linux-gnu와 macOS 15의
aarch64-apple-darwin 후보를 생성·검증한다. CI에서는 해당 플랫폼의 Rust 1.98.0과
Python 3.14를 사용한다. 릴리스를 게시하지 않고 로컬 후보 파일을 만든다.

빌더는 Git HEAD를 격리한 소스 디렉터리로 내보낸다. Ignored·untracked·수정된
작업 파일은 소스 빌드에 들어가지 않는다. 잠금 crate 원본과 고정 cargo-deny는
미리 준비해야 한다. Cargo는 명시적 release target과 정리된 환경으로 오프라인
실행하고 외부 Cargo 설정 override를 거부하며 소스·캐시·도구 경로를 재매핑한다.
빌드 결과는 target, 임시 소스와 비공개 빌드 로그는 .local에 둔다. 검증 명령은
패키지·smoke 도구가 동일 hash로 소스 압축파일에 포함됐는지도 확인한다.
검증 후보를 만들기 전에 도구 변경을 커밋해야 한다.

```sh
python3 -B scripts/release_package.py build \
  --target aarch64-apple-darwin --output .local/candidate
python3 -B scripts/release_package.py verify .local/candidate \
  --commit VERIFIED_COMMIT_SHA --target aarch64-apple-darwin
```

출력 경로는 존재하지 않아야 하며 기존 후보를 덮어쓰지 않는다. 캐시와
cargo-deny는 [라이선스 안내](../../licensing/README.ko.md)에 따라 준비한다. Linux
native 빌더에서는 x86_64-unknown-linux-gnu를 사용한다. 이 명령은 게시나
attestation을 수행하지 않는다. 체크섬은 내부 일관성을 확인하며,
[서명·배포 절차](release-promotion.md)에서 빌드 출처 검증과 배포 승인을 수행한다.

| 후보 파일 | 증거 |
|---|---|
| 대상 바이너리 tar.gz | 실행 파일, 설정 예제, 제품·라이선스 문서, 커밋된 전체 Cargo 고지와 제공된 Rust 도구 고지 |
| 소스 tar.gz | Commit 검증 기록을 가진 Git 추적 소스, 정규화된 gzip 메타데이터 |
| 대상 cdx.json | 바이너리 hash에 연결한 CycloneDX 1.6 빌드 의존성 목록 |
| candidate.json | 소스 commit, Cargo.lock hash, compiler, target, 도구·자산·구성원 hash와 mode, 패키지 목록, linkage와 검증 단계 |
| SHA256SUMS | 정확한 파일명의 모든 후보 자산과 명세 |

바이너리 압축파일은 일반 파일과 검토된 0644/0755 mode만 사용한다. 이름·바이트·
mode·hash가 명세와 정확히 같아야 한다. 추가·누락, 링크, 예약 경로,
체크섬 변경과 잘못된 소스·target은 거부한다. 소스·바이너리 모두 공개 경계
검사를 적용하고 Cargo 패키지 목록도 별도로 검사한다. 소스 압축파일에는
대응 소스를 재빌드할 스크립트가 포함된다.

<a id="inventory-scope-and-notices"></a>

## 목록 범위와 고지

Target으로 Cargo metadata를 필터링한 후 실제 성공한 compiler-artifact 기록과
대조한다. 빌드하지 않은 platform/dev 의존성은 제외하고 정상 런타임 소스와
build/procedural-macro 입력을 구분한다. Feature는 실제 빌드 기록에서 얻는다.
Crate hash는 잠금 압축파일의 hash이며 컴파일 object의 hash가 아니다. 이름,
버전, 출처와 선택 라이선스는 검토된 기록을 사용한다. Metadata의 파일시스템
경로는 내보내지 않는다.

SBOM은 Rust 표준 라이브러리 묶음과 관측된 OS 동적 라이브러리도 기록한다.
완전한 구성 목록이라고 주장하지 않는다. 빌드 입력은 정확한 링크 바이트 목록이
아니며 표준 라이브러리·OS의 모든 구성원을 확장하지 않는다. Mach-O 라이브러리·
최소 macOS load command, ELF NEEDED/GLIBC symbol 요구를 관측한 플랫폼 조건으로
보존한다. 현재 runner의 smoke 성공으로 과거 OS 호환성을 추론하지 않는다.
시스템 라이브러리는 외부에 남고 패키지에 재배포하지 않는다.

Build/dev 기록을 포함한 커밋된 전체 플랫폼 Cargo 고지 묶음을 보존한다.
설치된 Rust COPYRIGHT-library, 사용 가능한 copyright/license 원문과 target
rlib hash도 바이트 수정 없이 기록한다. 이는 제공된 도구 기록이며 법률 검토
완료, 완전한 바이너리 구성이나 대체 라이선스 허가를 증명하지 않는다. 배포자는
정식 승격 전에 정확한 도구·target과 추가 의무를 검토한다. 로컬 배포판과
업스트림 CI compiler 설명은 구분한다. 게이트웨이 패키지에는 Codex 실행 파일이 없다.

주요 목록 계약: [Cargo metadata](https://doc.rust-lang.org/cargo/commands/cargo-metadata.html),
[CycloneDX 1.6](https://cyclonedx.org/docs/1.6/json/),
[Rust 저작권 목록 설명](https://github.com/rust-lang/rust/blob/main/COPYRIGHT).

<a id="validation-and-promotion-boundary"></a>

## 검증과 승격 경계

Native 후보 실행 파일로 manifest/readiness binding, 미인증 거부, 합성
업스트림의 세 JSON 경로, 자격 헤더 분리와 제한된 정상 종료를 검사한다.
실제 모델 공급자는 호출하지 않는다. Archive 결정성은 같은 입력 바이트를
검사하며 다른 머신·도구 배포판 사이의 바이너리 재현성을 주장하지 않는다.
기존 Rust·Python·라이선스·공개 경계·고정 Codex 전체 시험은 계속 필수다.

PR package-smoke 행렬은 검증된 공개 후보만 보존하고 compiler 로그나 로컬
상태는 올리지 않는다. 결과는 ci-required에 포함된다. PR 산출물은 검토용이며
정식 승격 대상이 될 수 없다. 릴리스 후보는 검증된 main commit과 인증된
출처에서 생성해야 한다. [배포 워크플로](release-promotion.md)는 출처를 검증하고
승인 후 보관한 바이트를 그대로 게시한다. 운영 전에는 사용할 애플리케이션과
실제 모델을 별도로 시험해야 한다.
