<a id="messages-instruction-mapping"></a>
<a id="messages-instruction-role-lowering--approval-addendum"></a>
<a id="messages-지시-역할-변환--승인-부록"></a>

# Messages 지시 메시지 변환

[English](../messages-instruction-design.md) | [한국어](messages-instruction-design.md)

Messages 어댑터는 프로필의 `instruction_hierarchy`에
`bridged_instruction_envelope`를 명시하면, 대화 앞에 있는 Responses 지시를
system 텍스트로 변환한다. 지시 원문과 순서는 보존하지만 대상 모델에서
system과 developer의 우선순위를 각각 재현할 수는 없다.

<a id="observed-boundary"></a>
<a id="role-differences"></a>
<a id="관측된-경계"></a>

## 역할 차이

Responses는 system과 developer를 서로 다른 입력 역할로 표현한다.
Messages는 user/assistant 대화 역할과 별도의 system 지시를 사용하며
개별 developer 역할이 없다. 따라서 이 기능은 Native가 아닌 Bridged로 표시한다.
[Messages 참조](https://platform.claude.com/docs/en/api/messages/create)와
[Responses 참조](https://developers.openai.com/api/reference/typescript/resources/responses/methods/create)를 따른다.

<a id="conversion-rules"></a>
<a id="recommendation-and-exact-proposed-behavior"></a>
<a id="권고와-정확한-동작"></a>

## 변환 규칙

1. 대화 내용 앞에 있는 지시만 모은다.
2. 각 지시의 원래 역할, 위치와 정확한 텍스트를 JSON 데이터로 표현한다.
   순서를 보존하며 protocol-default, system, developer의 출처를 구분한다.
3. 이 데이터가 사용자 내용보다 우선하는 애플리케이션 지시임을 설명하는
   고정 어댑터 지시를 앞에 붙인다.
4. 사용자 메시지와 도구 내용은 이 지시 묶음에 넣지 않는다.
5. 대화 뒤에 나온 system/developer 메시지는 앞으로 옮기지 않고 거부한다.
   변환 규칙 선언이 없으면 전송 전에 거부한다.

원형 Responses 경로와 중간 표현은 원래 역할과 순서를 유지한다.
변환은 명시적으로 설정한 Messages 경로에만 적용한다. 미지원 공급자 검색과
추론 옵션을 끈 Codex 프로필을 사용해야 하며, 게이트웨이는 필수 기능을 임의로
제거하지 않는다.

<a id="alternatives-and-affected-boundaries"></a>
<a id="limits-and-validation"></a>
<a id="대안과-영향-경계"></a>

## 한계와 검증

변환 규칙은 원래 역할을 데이터로 보존하지만, 대상 모델이 그 우선순위를
해석하는 방식을 강제하지는 못한다. 도구 권한과 사용자 승인은 호스트가 담당한다.

테스트는 원문·역할·순서 보존, 사용자·도구 내용의 제외, 뒤늦은 지시의 거부와
변환 규칙 누락 시 업스트림 요청이 발생하지 않는지를 검사한다.
고정 Codex 시험은 모의 공급자로 이 경로를 확인한다. 운영에 사용하기 전에는
실제 모델의 지시 준수를 검증해야 한다.

system과 developer의 고유한 우선순위 구분이 필요하면 이를 지원하는 경로를
선택한다. 이 변환을 끄려면 규칙 선언을 제거한다. 이후 해당 기능이 필요한
요청은 명시적으로 실패한다. 전체 프로필과 변환 한도는 [Messages 지원](messages.md)을 참조한다.
