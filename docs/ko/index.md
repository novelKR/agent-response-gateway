<ProductIntro>

<a id="a-clear-connection-between-agents-and-models"></a>

# 에이전트와 모델 사이의 명확한 연결.

[English](../index.md) | [한국어](index.md)

에이전트 런타임과 백엔드 서비스를 위한 로컬 Responses 게이트웨이입니다.
경로를 선언하고 자격 증명을 분리하여, 하나의 인터페이스로 JSON과 스트리밍
응답을 전달합니다.

[시작하기](../../README.ko.md) · [프로토콜 살펴보기](protocol.md)

</ProductIntro>

<a id="one-entry-point-explicit-routes"></a>

## 하나의 진입점. 명시적인 경로.

클라이언트는 Responses를 유지하면서 선언한 업스트림 API를 선택합니다.
네이티브 전달은 응답 본문을 보존하고, 변환 경로는 문서화한 호환성 프로파일을
적용합니다.

<CardGrid kind="api" />

<a id="integrate-with-clear-responsibilities"></a>

## 책임이 분명한 통합

게이트웨이는 전송, 라우팅과 업스트림 자격 증명을 담당합니다. 애플리케이션은
도구, 승인과 대화 연속성을 담당합니다. 실제 공급자를 검증하기 전에 합성 입력과
모의 업스트림으로 시작하세요.

로컬에서 빌드하고 프로세스를 시작하기 전에 경로 설정을 검증합니다.
이 검사는 설정을 읽으며 공급자를 호출하지 않습니다.

```sh
cargo build --locked
cp config.example.toml config.local.toml
cargo run --locked -- check-config --config config.local.toml
```

<DiagramFigure kind="ownership" />

<CardGrid :ids="['integration', 'continuity', 'codex-contract']" />

<a id="know-the-supported-boundaries"></a>

## 현재 지원 범위

<SupportTable>

| 기능 | 현재 계약 |
|---|---|
| JSON과 SSE | 네이티브 전달과 명시적인 프로파일에 따른 API 변환 |
| 자격 증명 | 로컬 Bearer 인증과 별도의 공급자 자격 증명 |
| 도구와 승인 | 소비자가 실행하고 승인 |
| 연속성 | 호스트가 소유하는 이력, 로컬 압축과 복구 |
| 원격 상태 | 게이트웨이 저장소, `previous_response_id`와 원격 compact API는 미지원 |

</SupportTable>

<Callout variant="note">

합성 테스트와 고정 Codex 적합성 검증은 반복 가능한 근거를 제공합니다.
실제 공급자 qualification, 운영 활성화와 정식 릴리스는 별도의 수락 단계입니다.

</Callout>

<a id="find-your-next-step"></a>

## 다음 단계

<CardGrid :ids="['getting-started', 'packaging', 'contributing']" />
