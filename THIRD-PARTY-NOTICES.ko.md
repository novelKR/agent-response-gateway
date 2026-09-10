<a id="third-party-components-and-provenance"></a>

# 제3자 구성요소와 출처

[English](THIRD-PARTY-NOTICES.md) | [한국어](THIRD-PARTY-NOTICES.ko.md)

공개 프로젝트 소스에는 AGPL-3.0-only를 적용한다. Rust 의존성의 라이선스와
고지는 공개·상용 어느 라이선스를 선택해도 각각 유지된다.

`Cargo.toml`은 직접 의존성을, `Cargo.lock`은 실제 해결된 의존성 버전을
기록한다. 이 문서는 고지 자료의 안내다. 실제 버전별 선언·선택 기록은
[의존성 기록](licensing/dependencies.json), 패키지별 고지와 원문 링크는
[생성된 고지](licensing/THIRD-PARTY-NOTICES.md), 검사·갱신 절차는
[라이선스 관리 안내](licensing/README.ko.md)에서 관리한다.

고지 원문은 `licensing/texts/`에 바이트와 해시를 보존한다. 목록은 잠금 파일의
모든 플랫폼·빌드·개발 의존성을 포함하며 모두가 실제 바이너리에 링크됐다는
뜻은 아니다. 프로젝트의 별도 상용 계약은 제3자 고지나 허가를 대체하지 않는다.

배포 전에는 잠금 파일 기준으로 의존성 메타데이터와 배포 대상에 포함되는
코드를 조사하고, 각 패키지의 `LICENSE`, `COPYING`, `NOTICE` 등 요구되는
원문을 확인한다. `license_audit.py check`가 기록과 원본을 대조하고 `bundle`이
고지 묶음을 만든다. 메타데이터의 SPDX 식별자만으로 필요한 저작권 고지가
충족된다고 간주하지 않는다. 시스템 라이브러리·컨테이너·번들 실행 파일은
별도 검토 대상이며 절차는 [배포 문서](docs/ko/release.md)를 따른다.

<a id="source-of-the-license-text"></a>

## 라이선스 본문 출처

`LICENSE`는 GNU 공식 원문
<https://www.gnu.org/licenses/agpl-3.0.txt>에서 내려받은 AGPLv3 전문이다.
본문을 수정하지 않는다. 프로젝트의 버전 선택은 Cargo 메타데이터와
문서에서 `AGPL-3.0-only`로 지정한다.
