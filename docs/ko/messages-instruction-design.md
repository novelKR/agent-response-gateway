<a id="messages-instruction-role-lowering--approval-addendum"></a>

# Messages 지시 역할 변환 — 승인 부록

[English](../messages-instruction-design.md) | [한국어](messages-instruction-design.md)

상태: 의미적 한계를 상세 검토한 후 2026-09-08 명시적으로 승인했다.
G07/G08에서 bridge를 구현했다. 이 문서는 승인된 설계와 수락 경계를 보존하며,
[Messages 지원 계약](messages.md)이 현재 동작을 설명한다.

<a id="observed-boundary"></a>

## 관측된 경계

고정 0.154.0-alpha.6의 합성 Codex 텍스트 turn은 top-level instructions 뒤에
developer와 user 메시지를 보낸다. Gateway IR은 원래 역할과 위치를 유지한다.
G05는 선언된 기능과 custom-tool JSON bridge를 허용하지만 지시 역할을 조용히
합치는 동작까지 승인하지 않는다.

Messages는 user/assistant 대화 역할과 별도의 system prompt를 받으며,
별개의 developer 역할은 없다. [Messages 참조](https://platform.claude.com/docs/en/api/messages/create)를 따른다.
Responses는 [명시적 입력 역할](https://developers.openai.com/api/reference/typescript/resources/responses/methods/create)을
유지한다. 이 경로를 native 지시 계층 지원으로 선언할 수 없다. 모의 응답
성공도 실제 모델의 동등한 동작을 증명하지 않는다.

<a id="recommendation-and-exact-proposed-behavior"></a>

## 권고와 정확한 동작

**관측한 Codex → Messages 경로에 Required; wire 불일치 신뢰도 높음,
모델의 지시 준수 신뢰도 중간:** `instruction_hierarchy`에 선택적
`bridged_instruction_envelope` 규칙을 추가한다.

정규 IR과 native Responses 경로는 유지한다. 이 bridge를 명시적으로 선언한
Messages 경로만 선행 지시 묶음을 system 텍스트 블록으로 변환한다. 원래 역할,
위치와 정확한 텍스트를 JSON 데이터로 표현하고, 해당 필드가 우선순위 높은
애플리케이션 지시임을 설명하는 고정 어댑터 지시를 앞에 둔다. 원래 순서와
protocol-default·system·developer 출처를 구분한다. User/tool 텍스트는 이
envelope에 넣지 않는다. 대화 내용 뒤에 나온 system/developer 메시지는
system 앞으로 옮기면 위치가 달라지므로 거부한다.

이 방식은 내용·출처와 user 텍스트보다 높은 우선순위를 보존하지만,
system/developer 사이의 native 우선순위를 강제하지는 못한다. 지원은 Native가
아닌 Bridged로 표시하고 프로필에 한계를 기록한다. 모델 출력은 호스트의 도구
권한이나 승인을 대신하지 않는다.

프로필이 bridge를 명시해야 한다. 기존 프로필·설정은 기존 동작을 유지하며,
규칙이 없으면 요청 전에 거부한다. 대상 Codex 프로필은 미지원 hosted tool
search와 reasoning 옵션도 꺼야 한다. Gateway에서 필드를 제거해 통과시키지 않는다.

<a id="alternatives-and-affected-boundaries"></a>

## 대안과 영향 경계

1. 엄격한 native 역할 동등성을 유지하면 좁은 Messages codec만 제공하고 이
   Codex 프로필을 거부한다. 승인 없이도 완전하고 안전한 동작이지만 G09에서
   관측한 기본 프로필을 qualification할 수 없다.
2. 위 명시적 bridge를 사용하면 native 역할 구분 손실을 기록하며 원하는 경로를
   연결할 수 있다. 모델·소비자 수락은 별도로 유지한다.
3. 호스트/Codex prompt 생성 경로를 API별로 바꾸는 방식은 런타임 소스 유지보수와
   qualification 범위가 더 커서 여기서는 제안하지 않는다.

영향 범위는 기능 지원 enum·검증, Messages 요청 encoder, 프로필 문서와 Codex
시험 fixture다. 데이터베이스, 인증 방식, 생산 의존성과 native wire 동작은
바꾸지 않는다. 구현 비용은 작은~중간 수준의 변환 규칙 하나와 회귀 검사이며,
이후 Codex 지시 배치 변경을 따라야 한다.

위험은 대상 모델의 표기된 지시 해석 차이, 후속 지시 배치의 미지원 가능성과
native 계층이 동일하다는 과장이다. 명시적 선택, 역할·위치·내용 회귀, 거부
검사와 별도 소비자·실제 모델 수락으로 관리한다.

검증은 내용·순서 보존, user/tool 제외, 뒤늦은 지시 거부, bridge 누락 시
요청 0회, 실제 고정 Codex와 합성 upstream, 기존 native 전달 회귀를 포함한다.
모의 시험은 wire 동작을 검증한다. 복구는 프로필 선언 제거와 bridge revert로
수행하며 상태 마이그레이션은 없다.

승인은 이 명시적 Messages bridge와 설명한 한계만 포함한다. 실제 공급자 호출,
생산 활성화나 다른 API의 역할 병합은 승인하지 않는다. 승인 전에는 독립적인
좁은 codec과 도구·스트림 부분만 진행할 수 있었으며 관측한 Codex 프로필에는
이 명시적 bridge가 필요했다.
