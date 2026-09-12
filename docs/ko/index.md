<ProductIntro>

<a id="a-clear-connection-between-agents-and-models"></a>

# 에이전트와 모델 사이의 명확한 연결.

[English](../index.md) | [한국어](index.md)

Non-Responses 모델 API를 Responses 애플리케이션에서 사용할 수 있게 하는
확장형 프록시 소프트웨어입니다. 경량 Rust 코어는 명시적 변환 계약을 통해 선언한
요청·응답·도구·스트리밍 의미를 보존합니다. 선택형 확장과 애플리케이션 구성을
조합하여 독립 실행, 내장과 백엔드 연동을 지원하는 방향으로 발전합니다.
현재 런타임은 로컬 연결을 사용합니다.

[시작하기](../../README.ko.md) · [프로토콜 살펴보기](protocol.md)

</ProductIntro>

제품은 재사용 가능한 Proxy Core, 선택형 확장, 이를 선택하고 운영하는
애플리케이션의 세 책임 계층으로 구성됩니다. 계층마다 별도 저장소·crate·프로세스를
요구하지 않습니다. 경량성이란 선택하지 않은 기능의 런타임 의존성과 운영 부담을
기본 구성에 강제하지 않는다는 의미입니다.

| 계층 | 책임 |
|---|---|
| Proxy Core | 공통 의미, 기능 허용 판정, API 변환, 전송, 취소와 공통 검증 |
| 선택형 확장 | 선택한 공급자 접근, 자격 증명, 계정 풀, 연속성, 사용량과 운영 기능 |
| 실행 애플리케이션 | 설정, 기동, 패키징, 관리 경험과 호스트·백엔드 통합 |

이는 제품의 발전 방향입니다. 현재 확장은 메타데이터 관측과 사용량 기록을
제공하며, 계정 풀과 공급자 연속성 서비스에는 새로운 계약과 구현이 필요합니다.
확장은 공통 기능·정체성·종료 상태 검증을 우회하지 않고 구현을 제공합니다.
새로운 의미를 지원하려면 코어나 프로토콜의 버전을 변경해야 할 수 있습니다.

독립 실행 구성은 필요한 어댑터와 확장을 선택합니다. 내장 구성은 호스트가 실행을
감독합니다. 백엔드 구성은 범용 인증·정책·저장 계약에 연결합니다. 이는 조합 방식이며
제안된 모든 구성이 현재 제공된다는 증거는 아닙니다. [통합](integration.md),
[확장 설계](extensions-design.md), [개발 방향](roadmap.md)을 참조하세요.

지원한다고 선언한 의미는 보존하고 보존할 수 없는 차이는 드러냅니다. 브리지라는
사실만으로 의미 동등성이 증명되지는 않으며, 프로토콜 호환성이 모델의 지시 준수나
출력 품질을 보장하지는 않습니다. [IR 계약](ir.md)은 표현·변환·보장을 구분합니다.

<a id="one-entry-point-explicit-routes"></a>

## 하나의 진입점. 명시적인 경로.

클라이언트는 Responses를 유지하면서 선언한 업스트림 API를 선택합니다.
Responses 원형 전달은 응답 본문을 보존하고, 변환 경로는 문서화한 지원 기능
프로필을 적용합니다.

<CardGrid kind="api" />

<a id="integrate-with-clear-responsibilities"></a>

## 책임이 분명한 통합

현재 기본 구성에서 게이트웨이는 전송, 라우팅과 업스트림 자격 증명 사용을
담당합니다. 애플리케이션은 도구, 승인과 이력을 담당합니다.
실제 공급자를 검증하기 전에 합성 입력과
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
| JSON과 SSE | Responses 원형 전달과 명시한 프로필에 따른 API 변환 |
| 자격 증명 | 로컬 Bearer 인증과 별도의 공급자 자격 증명 |
| 도구와 승인 | 애플리케이션이 실행하고 승인 |
| 연속성 | 호스트가 소유하는 이력, 로컬 압축과 복구 |
| 원격 상태 | 게이트웨이 저장소, `previous_response_id`와 원격 compact API는 미지원 |

</SupportTable>

<Callout variant="note">

프로토콜 시험은 모의 공급자를 사용합니다. 운영 전에는 선택한 실제 모델과
애플리케이션의 권한·복구를 검증하세요.

</Callout>

<a id="find-your-next-step"></a>

## 다음 단계

<CardGrid :ids="['getting-started', 'packaging', 'contributing']" />
