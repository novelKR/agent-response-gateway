<a id="unpublished-release-candidates"></a>

# 미공개 배포 후보

[English](../packaging.md) | [한국어](packaging.md)

패키징 도구는 CI에서 네이티브 Rust 1.98.0과 Python 3.14로 후보를 빌드하고
실행한다. 도구 실행은 로컬 파일을 생성하며 공개는 별도의
[태그 배포 워크플로](release-promotion.md)를 따른다.

| 플랫폼 | Rust 대상 | 네이티브 러너 | 실행·배포 압축 형식 |
|---|---|---|---|
| Linux x64 | x86_64-unknown-linux-gnu | ubuntu-24.04 | tar.gz |
| Linux ARM64 | aarch64-unknown-linux-gnu | ubuntu-24.04-arm | tar.gz |
| macOS ARM64 | aarch64-apple-darwin | macos-15 | tar.gz |
| Windows x64 | x86_64-pc-windows-msvc | windows-2025 | zip; 실행 파일 확장자는 .exe |

[공통 대상 정의](../../scripts/release_targets.py)를 CI, 후보 빌드, 서명과 검증이
함께 사용한다. Rust·패키지 행렬의 모든 항목이 ci-required 통과에 필요하다.
이 러너 이미지는 시험한 빌드 기준이며 이전 OS 호환성은 별도 증거가 필요하다.

빌더는 Git HEAD를 격리한 소스 디렉터리로 내보낸다. Ignored·untracked·수정된
작업 파일은 소스 빌드에 들어가지 않는다. 잠금 crate 원본과 고정 cargo-deny는
미리 준비해야 한다. Cargo는 명시적 release target과 정리된 환경으로 오프라인
실행하고 외부 Cargo 설정 override를 거부하며 소스·캐시·도구 경로를 재매핑한다.
빌드 결과는 target, 임시 소스와 비공개 빌드 로그는 .local에 둔다. Windows에서는
설치된 vcvars64.bat로 네이티브 x64 MSVC 환경을 초기화한다. 필요한 빌드 변수만
유지하고 PATH에서 선택한 MSVC 도구를 Git 도구보다 앞에 두며 DUMPBIN도 같은
설정의 도구 모음을 사용한다. [Microsoft 명령줄 빌드 안내](https://learn.microsoft.com/en-us/cpp/build/building-on-the-command-line)를 참고한다.
검증 명령은
패키지·smoke 도구가 동일 hash로 소스 압축파일에 포함됐는지도 확인한다.
검증 후보를 만들기 전에 도구 변경을 커밋해야 한다.

```sh
python3 -B scripts/release_package.py build \
  --target aarch64-apple-darwin --output .local/candidate
python3 -B scripts/release_package.py verify .local/candidate \
  --commit VERIFIED_COMMIT_SHA --target aarch64-apple-darwin
```

출력 경로는 존재하지 않아야 하며 기존 후보를 덮어쓰지 않는다. 캐시와
cargo-deny는 [라이선스 안내](../../licensing/README.ko.md)에 따라 준비한다. 표에서
네이티브 빌더에 맞는 대상을 선택한다. 이 명령은 게시나
attestation을 수행하지 않는다. 체크섬은 내부 일관성을 확인하며,
[서명·배포 절차](release-promotion.md)에서 빌드 출처 검증과 배포 승인을 수행한다.

| 후보 파일 | 증거 |
|---|---|
| 대상 바이너리 tar.gz 또는 zip | 실행 파일, 설정 예제, 제품·라이선스 문서, 커밋된 전체 Cargo 고지와 제공된 Rust 도구 고지 |
| 소스 tar.gz | Commit 검증 기록을 가진 Git 추적 소스, 정규화된 gzip 메타데이터 |
| 대상 cdx.json | 바이너리 hash에 연결한 CycloneDX 1.6 빌드 의존성 목록 |
| candidate.json | 소스 commit, Cargo.lock hash, compiler, target, 도구·자산·구성원 hash와 mode, 패키지 목록, linkage와 검증 단계 |
| SHA256SUMS | 정확한 파일명의 모든 후보 자산과 명세 |

바이너리 압축파일은 일반 파일과 검토된 0644/0755 mode만 사용한다. 이름·바이트·
mode·hash가 명세와 정확히 같아야 한다. 추가·누락, 링크, 예약 경로,
체크섬 변경과 잘못된 소스·target은 거부한다. 소스·바이너리 모두 공개 경계
검사를 적용하고 Cargo 패키지 목록도 별도로 검사한다. 소스 압축파일에는
대응 소스를 재빌드할 스크립트가 포함된다. Windows ZIP은 고정 시간과 일반 파일
mode를 사용한다. 위험하거나 중복된 이름, 대소문자 충돌, 링크, 부모 경로 충돌,
추가 메타데이터와 크기 초과 항목은 거부한다. Windows 체크아웃은 자동 줄바꿈
변환을 끈다. CRLF를 포함한 소스와 라이선스 고지 원문의 바이트를 보존한다.

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
Windows에서는 설치된 MSVC DUMPBIN으로 x64 PE 헤더와 DLL 의존성을 확인하고
도구 버전을 기록한다. 패키지 명세에는 러너 이미지와 대상 정의도 남긴다.
Windows·MSVC 런타임 DLL은 외부에 남으며 시스템 라이브러리를 재배포하지 않는다.
GitHub 출처 증명은 별도로 제공한다. 이 빌드 절차에는 Apple·Authenticode 코드서명,
설치프로그램이나 Windows 서비스 등록이 없다.

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

Native 후보 실행 파일과 압축 해제한 실행 파일 모두에서 설정 검사,
manifest/readiness binding, 미인증 거부, 합성
업스트림의 네 JSON 경로와 Interactions 영속 재시작, 자격 헤더 분리와 제한된 정상 종료를 검사한다.
실제 모델 공급자는 호출하지 않는다. Windows는 CREATE_NEW_PROCESS_GROUP으로
생성한 자식에게만 CTRL_BREAK_EVENT를 보낸다. 강제 종료는 실패 후 정리 절차이며
정상 종료 통과로 처리하지 않는다. Archive 결정성은 같은 입력 바이트를
검사하며 다른 머신·도구 배포판 사이의 바이너리 재현성을 주장하지 않는다.
기존 Rust·Python·라이선스·공개 경계·고정 Codex 전체 시험은 계속 필수다.

PR package-smoke 행렬은 검증된 공개 후보만 보존하고 compiler 로그나 로컬
상태는 올리지 않는다. 결과는 ci-required에 포함된다. PR 산출물은 검토용이며
정식 승격 대상이 될 수 없다. 릴리스 후보는 검증된 main commit과 인증된
출처에서 생성해야 한다. [배포 워크플로](release-promotion.md)는 출처를 검증하고
보관한 바이트를 그대로 시험판으로 공개한다. 승인 후 동일한 공개 파일을 정식으로 승급한다. 운영 전에는 사용할 애플리케이션과
실제 모델을 별도로 시험해야 한다.
