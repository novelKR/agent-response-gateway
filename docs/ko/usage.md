<a id="calling-and-integrating-the-gateway"></a>

# 호출·통합 예제

[English](../usage.md) | [한국어](usage.md)

등록된 모델을 호출하고 응답을 읽은 뒤 애플리케이션에서 대화나 도구 작업을
이어가는 방법을 설명한다. 예제는 합성 텍스트와 로컬 게이트웨이를 사용한다.
검증에는 모의 공급자를 사용한다. 실제 공급자를 설정하면 실제 모델 요청이 전달된다.

<a id="run-a-downloaded-package"></a>

## 배포 파일에서 실행

[GitHub Releases](https://github.com/novelKR/agent-response-gateway/releases)에
버전이 공개되면 특정 태그와 [네이티브 대상](packaging.md)을 선택한다. Linux
x64·ARM64와 macOS ARM64는 tar.gz, Windows x64는 zip과 agent-response-gateway.exe를
사용한다. Pre-release와 정식 Release의 파일은 같으며 승인 후 표시가 바뀐다.

해당 대상의 배포 압축파일, manifest.json과 sigstore.jsonl을 다운로드하고
release-manifest.json도 보관한다. 실행하기 전에 [출처 검증 절차](release-promotion.md)로
소스 커밋, 태그, 실행과 파일 해시를 확인한다. 체크섬 일치만으로 다운로드의
출처를 인증할 수는 없다.

배포 압축파일을 새 디렉터리에 푼다. 내부 candidate 디렉터리에 실행 압축파일,
소스 압축파일, SBOM, candidate.json과 SHA256SUMS가 있다. 실행 압축파일을
한 번 더 풀면 agent-response-gateway/bin과 설정 예제가 나온다. 대응 소스와
고지를 함께 보관한다. 기존 설치에 덮어쓰지 않고 이전에 검증한 실행 파일과
설정을 복구용으로 유지한다.

아래 명령은 실행 압축파일을 현재 디렉터리에 풀고 config.local.toml을 준비한
상태를 가정한다. 시작 안내의 cargo run 명령 대신 패키지 실행 파일을 사용한다.
공급자 키를 게이트웨이 환경에 설정하고 별도 로컬 토큰을 사용하는 원칙은 같다.

```sh
./agent-response-gateway/bin/agent-response-gateway check-config --config config.local.toml
./agent-response-gateway/bin/agent-response-gateway serve --config config.local.toml
```

```powershell
$env:ARG_LOCAL_TOKEN = [guid]::NewGuid().ToString("N")
.\agent-response-gateway\bin\agent-response-gateway.exe check-config --config config.local.toml
.\agent-response-gateway\bin\agent-response-gateway.exe serve --config config.local.toml
```


<a id="configure-the-route-and-make-a-first-call"></a>

## 경로 설정과 첫 호출

[시작하기](../../README.ko.md#getting-started)에 따라 게이트웨이를 설정하고 실행한다.
선택한 설정의 공급자 주소와 모델 자리표시자를 바꾸고, 게이트웨이 프로세스에
키 환경변수를 설정한다. 소비자는 별도의 로컬 토큰을 사용한다.

| 업스트림 API | 설정 예시 | 예시에 등록된 별칭 |
|---|---|---|
| Responses | [Responses 설정](../../config.example.toml) | `example/writer` |
| Messages | [Messages 설정](../../config.messages.example.toml) | `example/messages` |
| Chat Completions | [Chat 설정](../../config.chat.example.toml) | `example/chat` |

각 예시는 별개의 설정이다. 실제로 실행한 설정에 등록된 별칭을 사용한다.
세 경로 모두 게이트웨이의 `POST /v1/responses`에서 Responses 형식 JSON을 받는다.
`/v1/messages`와 `/v1/chat/completions`는 소비자용 엔드포인트가 아니다.
다른 이름을 제공하거나 키를 선택하는 방법은 [모델 이름과 키](route-design.md#consumer-model-names)를 참조한다.

| 주소 | 예시 | 용도 |
|---|---|---|
| 공급자 `base_url` | `https://provider.example/v1` | 게이트웨이가 선택한 업스트림 API 경로를 붙임 |
| 준비 정보의 `base_url` | `http://127.0.0.1:43127/v1` | 소비자 기본 주소이며 이미 `/v1`을 포함함 |
| 소비자 요청 URL | `http://127.0.0.1:43127/v1/responses` | 준비 정보의 기본 주소에 `/responses`를 한 번 붙임 |

아래 예시 포트 대신 준비 JSON에서 받은 포트를 사용한다. 호출하는 셸의
`ARG_LOCAL_TOKEN`에는 게이트웨이와 같은 로컬 토큰을 설정한다.
공급자 키는 게이트웨이 프로세스에 둔다.

```sh
export GATEWAY_BASE_URL='http://127.0.0.1:43127/v1'
curl --noproxy '*' -i "${GATEWAY_BASE_URL}/responses" \
  -H "Authorization: Bearer ${ARG_LOCAL_TOKEN}" \
  -H 'Content-Type: application/json' \
  -d '{"model":"example/writer","input":"Reply with hello.","max_output_tokens":256,"store":false,"stream":false}'
```

명령은 응답 헤더와 JSON 본문을 출력한다. 상태 확인이나 모델 목록 조회 성공은
로컬 연결만 확인하며, 이 요청은 선택한 업스트림 경로를 실제로 호출한다.
Messages나 Chat Completions를 사용하려면 Responses 요청 형식을 유지하면서
`example/writer`를 해당 경로에 등록된 별칭으로 바꾼다.

<a id="request-options-and-response-values"></a>

## 요청 옵션과 응답 값

검증한 모델 한도 안에서 `max_output_tokens`를 명시한다. 변환 경로에서 생략하면
프로필의 출력 한도를 사용하고, 네이티브 Responses 경로에서는 생략한 상태를
업스트림 API에 전달한다. 프로필 한도는 입력 토큰 계수나 금액 예산이 아니다.

변환 경로에는 요청이 사용하는 기능의 프로필 선언이 필요하다. 예를 들어
`function_tools`, `reasoning_effort`, `strict_structured_output`을 사용하기 전에
해당 기능을 선언해야 한다. 선언만으로 없는 변환 기능이 구현되거나 실제 모델의
지원 여부가 검증되지는 않는다. 클라이언트가 추가한 `text.verbosity`나 추론 요약
같은 기본 옵션으로 변환 요청이 실패할 수도 있다.
[지원 비교표](conformance.md#protocol-coverage)와 선택한 [Messages](messages.md)
또는 [Chat Completions](chat-completions.md) 레퍼런스를 따른다.

성공 응답을 해석하기 전에 HTTP 상태를 확인한다. 해석한 Responses 객체가
`response`라면 첫 출력 항목을 답변으로 가정하지 말고 타입에 따라 텍스트와
도구 호출을 모은다.

```python
if response.get("status") not in {"completed", "incomplete", "failed"}:
    raise ValueError("Expected a final Responses status")
output = response["output"]
text_parts = [
    part["text"]
    for item in output if item["type"] == "message"
    for part in item["content"] if part["type"] == "output_text"
]
tool_calls = [
    item for item in output
    if item["type"] in {"function_call", "custom_tool_call"}
]
status = response["status"]
usage = response.get("usage")
```

이 코드는 텍스트와 도구 호출을 고르는 예시다. 거절과 다른 출력 타입은 선택한
경로의 규칙에 따라 확인하며, 완전한 응답 스키마 검증기를 대신하지 않는다.
구조화된 모델 출력도 사용 전에 애플리케이션이 요구하는 스키마로 검사한다.
`usage`가 없거나 null이면 사용량이 보고되지 않은 것이며 토큰이나 비용이
0이라는 뜻이 아니다. 공급자 토큰 수치만으로 청구 금액이 확정되지 않는다.

| 응답 상태 | 애플리케이션의 처리 |
|---|---|
| 텍스트가 있고 미처리 도구가 없는 `completed` | 검증한 모델 응답을 사용 |
| 도구 호출이 있는 `completed` | 허용된 호출을 처리하고 결과를 전송; 전체 작업은 아직 끝나지 않음 |
| `incomplete` | `incomplete_details`를 확인하고 부분 출력을 보존한 뒤 복구 방법을 명시적으로 선택 |
| `failed` 또는 없거나 예상과 다른 최종 상태 | 실패나 잘못된 응답으로 처리하고 도구를 실행하지 않음 |

<a id="continue-a-conversation"></a>

## 대화 이어가기

매 요청에 현재 대화 문맥 전체를 제공한다. 게이트웨이는 응답 ID로 이전 메시지를
조회하지 않는다. 다음 두 번째 턴 예시는 이전 사용자 메시지와 설명용 모델 답변을
함께 보낸다.

```json
{
  "model": "example/writer",
  "input": [
    {"role": "user", "content": "Remember the word blue."},
    {"role": "assistant", "content": "The word is blue."},
    {"role": "user", "content": "What word did I ask you to remember?"}
  ],
  "max_output_tokens": 256,
  "store": false,
  "stream": false
}
```

애플리케이션에서는 설명용 답변 대신 실제로 완료된 메시지를 넣는다. 순서를
보존하고 새 사용자 메시지를 끝에 추가한다. `store:false`를 유지한다.
`previous_response_id`, 저장 항목 참조와 대화 저장은 지원하지 않는다.
일반 텍스트는 옮길 수 있지만 불투명한 추론 정보와 미완료 공급자 상태를 다른
경로에서 자동 재사용할 수는 없다. 재시작이나 모델 변경은 [연속성 계약](continuity.md)을 따른다.

<a id="complete-a-function-tool-round-trip"></a>

## 함수 도구 호출 왕복

게이트웨이는 도구 호출을 호스트에 반환하며 도구를 실행하지 않는다.
다음 첫 요청은 입력 텍스트를 그대로 돌려주는 것만 수행하는 `echo` 함수를 정의한다.
변환 경로에서 이 예시를 사용하려면 `function_tools` 지원이 필요하다.

```json
{
  "model": "example/writer",
  "input": [{"role": "user", "content": "Use echo to return hello."}],
  "tools": [{
    "type": "function",
    "name": "echo",
    "description": "Return the supplied text unchanged.",
    "parameters": {
      "type": "object",
      "properties": {"text": {"type": "string"}},
      "required": ["text"],
      "additionalProperties": false
    }
  }],
  "max_output_tokens": 256,
  "store": false,
  "stream": false
}
```

모델 응답이 완료되면 `output`에 다음 형태의 항목이 포함될 수 있다.

```json
{
  "type": "function_call",
  "call_id": "call_echo",
  "name": "echo",
  "arguments": "{\"text\":\"hello\"}"
}
```

실제 반환된 호출의 `call_id`와 인자를 사용한다. 첫 요청과 해석한 응답을
각각 `first_request`, `first_response`라고 할 때, 호스트는 이 예시에서 명시적으로
허용한 함수를 실행하고 후속 요청을 구성할 수 있다.

```python
import json

def run_echo(call):
    if call["type"] != "function_call" or call["name"] != "echo":
        raise ValueError("Only the example echo function is allowed")
    arguments = json.loads(call["arguments"])
    if not isinstance(arguments, dict) or set(arguments) != {"text"}:
        raise ValueError("Expected only the text argument")
    if not isinstance(arguments["text"], str):
        raise ValueError("Expected a string")
    return {
        "type": "function_call_output",
        "call_id": call["call_id"],
        "output": arguments["text"],
    }

if first_response.get("status") != "completed":
    raise ValueError("Do not execute tools from an incomplete or failed response")
items = first_response["output"]
if any(item["type"] not in {"message", "function_call"} for item in items):
    raise ValueError("This example accepts only portable messages and function calls")
calls = [item for item in items if item["type"] == "function_call"]
if not calls:
    raise ValueError("The model returned no function call")
results = [run_echo(call) for call in calls]
second_request = {
    **first_request,
    "input": [*first_request["input"], *items, *results],
}
```

`second_request`를 같은 게이트웨이 엔드포인트에 POST한다. 원래 문맥, 모델의
메시지와 호출, 각 호출에 대한 `function_call_output`이 포함된다. 결과에는 해당
호출의 `call_id`를 사용한다. 응답 ID나 항목 ID로 대체하지 않는다.
이 예시는 이전 항목들을 추가할 수 있도록 입력 배열을 사용한다.

병렬 호출은 모든 호출의 순서를 유지하고 필요한 결과를 모두 모은 뒤 모델을
다시 호출한다. 변환 경로는 중복되거나 연결이 맞지 않는 결과와, 아직 결과가
오지 않았는데 assistant가 대화를 이어가는 요청을 거부한다. 이름만 보고 호출을
재구성하거나 스트림의 부분 인자를 실행하지 않는다. 실제 도구에는 애플리케이션의
권한 확인, 인자 검증과 완료 기록이 필요하다. 재시도나 재연결로 이미 완료한
외부 변경을 반복해서는 안 된다.

<a id="read-a-stream-and-handle-cancellation"></a>

## 스트리밍과 취소

`stream:true`로 SSE를 조금씩 읽는다. `-N`은 curl 출력 버퍼링을 끄며,
`--max-time 120`은 소비자 기한의 예시이고 게이트웨이 기본값은 아니다.
애플리케이션에 맞게 기한을 정하고 시간 초과를 정상 완료와 구분한다.

```sh
curl --noproxy '*' -N --max-time 120 "${GATEWAY_BASE_URL}/responses" \
  -H "Authorization: Bearer ${ARG_LOCAL_TOKEN}" \
  -H 'Content-Type: application/json' \
  -d '{"model":"example/writer","input":"Reply with hello.","max_output_tokens":256,"store":false,"stream":true}'
```

텍스트 응답은 다음 순서로 이벤트를 보낼 수 있다. 도구는 별도의 항목·인자
이벤트를 사용한다. 완전한 SSE 이벤트를 해석한 뒤 각각의 JSON을 해석한다.
TCP 청크는 UTF-8 문자, 이벤트 줄이나 JSON 중간에서 나뉠 수 있다.

```text
response.created
response.output_item.added
response.content_part.added
response.output_text.delta  (zero or more)
response.output_text.done
response.content_part.done
response.output_item.done
response.completed
```

| 관찰한 값 | 의미 |
|---|---|
| HTTP 200 또는 `response.created` | 응답을 시작했으며 생성이 아직 완료된 것은 아님 |
| `response.completed` | 최종 응답을 검증하고 도구 호출이 있으면 처리 |
| `response.incomplete` | 불완전한 생성 종료이며 `incomplete_details`를 확인 |
| `response.failed` | 공급자가 실패를 보고했으므로 실패 상태를 보존 |
| 유효한 종료 이벤트 전에 EOF·전송 오류·기한 도달 | 중단되었거나 결과가 불확실하며 성공 완료가 아님 |

네이티브 경로는 공급자 이벤트를 전달한다. 변환 경로는 검증한 완료·불완전
이벤트를 생성하지만, 공급자나 변환 오류로 헤더 전송 후 스트림을 닫을 수도 있다.
이 경우 새로운 JSON 오류 본문이 온다고 가정하지 않는다. 소비자는 다음 개요에
따라 상태를 처리한다.

```text
state = awaiting_terminal
for each complete, decoded SSE event within the application deadline:
    display text deltas as provisional output
    collect tool arguments without executing partial arguments
    response.completed  -> validate the final response; state = completed
    response.incomplete -> retain incomplete_details; state = incomplete
    response.failed     -> retain failure information; state = failed
on deadline, cancellation, transport error or EOF before a valid terminal event:
    state = interrupted
continue the workflow only from a validated completed response
```

[기본 Python 클라이언트](../../examples/client.py)는 응답 바이트를 그대로 출력한다.
EOF에서 정상 종료했다고 모델의 정상 완료가 검증된 것은 아니다. 임시 표시한
텍스트와 확정한 출력을 구분하고, 완전한 인자와 최종 응답을 검증한 뒤 도구를 실행한다.

취소할 때는 활성 응답을 닫는다. 게이트웨이는 업스트림 읽기를 멈추지만 공급자가
이미 받은 작업은 계속되거나 과금될 수 있다. heartbeat 바이트로 유휴 스트림이
계속 유지될 수 있으므로 애플리케이션의 전체 호출 기한을 적용한다.
게이트웨이의 구간별 대기 한도는 [타임아웃 계약](protocol.md#resources-and-failures)을 참조한다.

<a id="connect-the-gateway-to-an-application"></a>

## 애플리케이션에 연결

1. 설정과 필요한 모델 기능을 선택한다. `check-config`와 오프라인 `manifest`를
   확인한다. 둘 다 공급자를 호출하지 않는다.
2. 모든 등록 공급자 키와 인스턴스 전용 로컬 토큰을 포함하는 명시적인 자식
   환경으로 게이트웨이를 시작한다. 소비자에는 로컬 토큰과 등록 별칭만 제공한다.
3. 크기와 대기 시간을 제한하여 준비 메시지를 읽고 설정 해시와 실제 주소를
   검증한다. stderr는 별도로 읽는다. `/readyz` 응답이 공급자 검증을 뜻하지는 않는다.
4. 준비를 확인한 뒤 모델 요청을 시작한다. 활성 호출, 요청 ID, 기한과 도구 완료를
   추적한다. 설정이나 키 변경은 인스턴스를 재시작하고 새 준비 정보를 확인하여
   적용한다. 자동 재로딩은 없다.
5. 종료 시 새 작업을 막고 활성 요청을 취소하거나 마무리한 뒤 응답을 닫는다.
   호스트의 기한 안에서 자신이 시작한 게이트웨이를 종료한다. 중단된 작업은
   자동 재실행하지 않고 결과가 불확실한 상태로 처리한다.

전체 시작·종료 계약은 [프로세스 감독](embedded-design.md#process-lifecycle), 이력 복구는
[연속성](continuity.md), 관찰되는 오류의 해결 방법은 [문제 해결](troubleshooting.md)을 참조한다.
