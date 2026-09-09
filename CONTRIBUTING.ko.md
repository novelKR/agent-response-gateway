<a id="contribution-policy"></a>

# 기여 정책

[English](CONTRIBUTING.md) | [한국어](CONTRIBUTING.ko.md)

이 프로젝트는 **AGPL-3.0-only와 별도 상용 라이선스**를 사용한다.
모든 기여는 [라이선스 정책](COMMERCIAL-LICENSING.ko.md)에 명시한
양쪽 배포 방식을 지원할 수 있어야 한다.

<a id="rights-required-for-code-contributions"></a>

## 코드 기여에 필요한 권한

다음 조건을 확인한 코드만 병합한다.

1. 기여자가 필요한 권리를 보유하거나 권리자의 허가를 받아야 한다.
   고용주가 권리를 가진 경우 고용주의 허가도 필요하다.
2. 서면 기여 계약으로 프로젝트가 해당 기여를 AGPL-3.0-only와 상용 조건
   양쪽으로 사용·수정·배포할 수 있는 권한을 확보해야 한다.
3. 복사하거나 수정해 가져온 자료는 원본 위치, 정확한 버전, 라이선스와
   필요한 고지를 명시해야 한다. 해당 조건이 제안한 재사용을 허용해야 한다.
4. 관리자가 출처를 검토하고 필요한 동의 기록을 확인해야 한다.
   PR 제출이나 DCO 서명만으로 상용 재라이선스 권한이 생기지는 않는다.

비공개 계약과 동의 기록은 공개 이슈나 소스 파일에 넣지 않는다.

<a id="submitting-a-change"></a>

## 변경 제출

[Issues](https://github.com/novelKR/agent-response-gateway/issues)에서 버그 제보,
재현 절차와 API 의견을 받는다. 큰 변경은 PR을 열기 전에 논의한다.
각 변경은 목적을 좁히고 관련 테스트와 문서를 함께 제출한다.

저장소 지침을 읽고 `cargo fmt --check`,
`cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`를 실행한다.
기본 테스트는 실제 API 키 없이 합성 입력과 모의 공급자를 사용한다.

의존성 변경에는 [라이선스 관리 절차](licensing/README.ko.md)에 따른 잠금 파일과
필수 라이선스 기록을 포함한다. 스크립트 검사는 Python 3.11 이상을 사용한다.

인증정보, 비공개 설정, 사내 데이터와 다른 애플리케이션의 코드를 허가 없이
포함하지 않는다. 허용된 재사용도 원래 고지를 보존한다. 공개 내용은
[문서 관리](docs/ko/documentation.md), API 변경은 [지원 계약](docs/ko/protocol.md),
호스트 책임은 [통합 경계](docs/ko/integration.md)를 따른다.
