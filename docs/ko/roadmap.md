<a id="implementation-milestones-and-priorities"></a>
<a id="support-status-and-development-direction"></a>
<a id="구현-마일스톤과-우선순위"></a>

# 지원 범위와 개발 방향

[English](../roadmap.md) | [한국어](roadmap.md)

게이트웨이는 세 종류의 업스트림 API를 로컬 Responses 인터페이스로 제공한다.
이 문서는 사용할 수 있는 기능과 운영 전에 필요한 검증을 정리한다.
세부 동작은 [HTTP 계약](protocol.md)과 [지원표](conformance.md)를 따른다.

<a id="available-capabilities"></a>
<a id="overall-sequence"></a>
<a id="p0--m2--connect-model-routes-and-capability-profiles"></a>
<a id="p0--m2--모델-경로와-기능-프로필의-실행-연결"></a>
<a id="p1--m3--first-converted-apis-complete-tool-round-trip"></a>
<a id="p1--m3--첫-변환-api의-전체-도구-왕복"></a>
<a id="p1--m4--second-converted-api-and-common-regression"></a>
<a id="p1--m4--두-번째-변환-api와-공통-회귀"></a>
<a id="p1--m5--consumer-startup-and-bounded-embedded-acceptance"></a>
<a id="p1--m5--소비자-내장-기동과-제한된-사용-수락"></a>
<a id="p1--m6--long-running-work-compaction-and-resume"></a>
<a id="p1--m6--장기-실행압축재개-수락"></a>
<a id="starting-point-m0--responses-transport-and-ir-foundation"></a>
<a id="전체-순서"></a>
<a id="출발점-m0--responses-전달과-ir-기반"></a>

## 제공하는 기능

| 영역 | 범위 |
|---|---|
| 전송 | 루프백 인증, 설정된 모델 별칭, 공급자 키 분리, JSON/SSE와 제한된 종료 |
| API 변환 | Responses 원형 전달과 명시적으로 설정한 Messages / Chat Completions 프로필 |
| 도구와 출력 | 함수, 사용자 정의 텍스트, 네임스페이스, 등록된 패치 문법과 선언된 출력 제어 |
| 내장 | 오프라인 설정 명세, 준비 통지 검사와 호스트가 관리하는 프로세스 수명 |
| 연속성 | 호스트가 보관하는 이력, 로컬 압축, 검증된 재개와 명시적 모델 전환 |
| 배포 도구 | 후보 압축파일, 소스·고지, 의존성 목록, 서명된 출처와 승인 후 미리보기 배포 |

자동 시험은 실제 고정 Codex를 실행할 때도 모의 공급자를 사용한다.
프로토콜과 호스트 계약을 검사하며, 실제 모델의 출력 품질이나 특정
애플리케이션의 운영 동작을 보장하지는 않는다.

<a id="before-production-use"></a>
<a id="p0--m1--validation-foundation-and-pinned-codex-contract"></a>
<a id="p0--m1--검증-기반과-codex-실행-계약-고정"></a>
<a id="parallel-preparation-and-p2--m7--distribution-rights-and-consumer-adoption"></a>
<a id="병행-준비와-p2--m7--공개-배포권리소비자-채택"></a>

## 운영 전 검증

1. **모델 선택과 시험:** 실제 공급자·모델에서 필요한 도구, 지시 처리,
   출력 형식과 컨텍스트 한도를 확인한다. 시험 비용 한도를 정하고 검증한
   프로필을 기록한다.
2. **애플리케이션 통합:** 애플리케이션의 저장 방식과 권한 체계로 기동,
   자격 증명 분리, 도구 승인, 취소, 압축, 재시작과 결과 불명 요청의 복구를 시험한다.
3. **배포물 확인:** 버전, 소스, 실행 파일, 설정, 고지와 출처를 연결한다.
   [배포 절차](release.md)에 따라 그 정확한 조합의 도입과 복구를 시험한다.
4. **Codex 시험 런타임 관리:** 고정된 시험판을 안정판으로 바꿀 때는
   산출물·스키마 검사와 전체 [적합성 시험](codex-contract.md)을 먼저 통과해야 한다.

제한된 미리보기 배포는 선언한 기능에 맞춰 검증한다.
장기 실행을 위한 내장은 [연속성 검사](continuity.md)도 필요하다.

<a id="outside-the-current-scope"></a>
<a id="p3--x--separate-extensions"></a>
<a id="p3--x--별도-확장"></a>

## 현재 범위 밖의 기능

다른 클라이언트 입력 API, WebSocket, OAuth·계정 풀, 테넌트를 구분하는 공개
서비스 모드와 관리 UI는 지원하지 않는다. 해당 영역을 확장할 때는 요구를
정의하고 API, 인증, 영속 상태와 배포 계약의 영향을 검토해야 한다.

<a id="change-and-verification-rules-for-every-milestone"></a>
<a id="changes-and-verification"></a>
<a id="각-마일스톤의-변경검증-규칙"></a>

## 변경과 검증

각 변경은 목적을 좁히고 결과 동작을 설명하며 기존 호환성을 보존한다.
검토와 필수 검사는 [개발 절차](github-workflow.md), 라이선스 권한은
[기여 정책](../../CONTRIBUTING.ko.md)을 따른다. 진행 중인 작업과 제안은
[Issues](https://github.com/novelKR/agent-response-gateway/issues)에서 관리한다.
