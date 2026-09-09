<a id="호출통합-예제"></a>

# Calling and integrating the gateway

[English](usage.md) | [한국어](ko/usage.md)

Call a registered model, read its response and continue the conversation or tool
workflow from your application. These examples use synthetic text and a local
gateway. Use a mock provider for verification; a configured real provider receives
actual model requests.

<a id="경로-설정과-첫-호출"></a>

## Configure the route and make a first call

Follow [Getting started](../README.md#getting-started) to configure and start the
gateway. Replace the provider endpoint and model placeholders in the selected
configuration, set its key variables in the gateway process and retain the separate
local token for the consumer.

| Upstream API | Configuration example | Registered example alias |
|---|---|---|
| Responses | [Responses configuration](../config.example.toml) | `example/writer` |
| Messages | [Messages configuration](../config.messages.example.toml) | `example/messages` |
| Chat Completions | [Chat configuration](../config.chat.example.toml) | `example/chat` |

These are separate configurations. Use the alias registered in the configuration
you actually started. All three receive Responses-format JSON at the gateway's
`POST /v1/responses`; `/v1/messages` and `/v1/chat/completions` are not consumer
endpoints. [Model names and keys](route-design.md#consumer-model-names) explains how
to publish other names or select different keys.

| Address | Example | Use |
|---|---|---|
| Provider `base_url` | `https://provider.example/v1` | Gateway appends the selected upstream API path |
| Readiness `base_url` | `http://127.0.0.1:43127/v1` | Consumer base; already includes `/v1` |
| Consumer request URL | `http://127.0.0.1:43127/v1/responses` | Append `/responses` once to the readiness base |

Use the port from the readiness JSON, not the illustrative port below. In the
calling shell, set `ARG_LOCAL_TOKEN` to the same local token used by the gateway.
The provider key stays in the gateway process.

```sh
export GATEWAY_BASE_URL='http://127.0.0.1:43127/v1'
curl --noproxy '*' -i "${GATEWAY_BASE_URL}/responses" \
  -H "Authorization: Bearer ${ARG_LOCAL_TOKEN}" \
  -H 'Content-Type: application/json' \
  -d '{"model":"example/writer","input":"Reply with hello.","max_output_tokens":256,"store":false,"stream":false}'
```

The command prints response headers and the JSON body. A successful health check
or model listing establishes local connectivity only; this request exercises the
selected upstream route. For Messages or Chat Completions, replace `example/writer`
with the corresponding registered alias while keeping the Responses request shape.

<a id="요청-옵션과-응답-값"></a>

## Request options and response values

Choose an explicit `max_output_tokens` within the verified model limit. On converted
routes, omitting it uses the profile's output limit; on a native Responses route,
omission is left to the upstream API. A profile limit is not an input-token counter
or a monetary budget.

Converted routes require profile declarations for the features the request uses.
For example, `function_tools`, `reasoning_effort` and `strict_structured_output`
must be declared before using those features. A declaration does not implement a
missing conversion or prove that a real model supports it. Client-added defaults
such as `text.verbosity` or reasoning summaries can make a converted request fail.
Use the [support comparison](conformance.md#protocol-coverage) and the selected
[Messages](messages.md) or [Chat Completions](chat-completions.md) reference.

Check the HTTP status before decoding a successful response. For a decoded
Responses object named `response`, collect text and calls by type rather than
assuming the first output item is the answer:

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

This excerpt selects text and tool calls; inspect refusals and other output types
according to the selected route. It is not a complete response-schema validator.
Validate structured model output against the application's required schema before
using it. Missing or null `usage` means unreported usage, not zero tokens or zero
cost. Provider token counts alone do not establish a bill.

| Response state | Application action |
|---|---|
| `completed` with text and no pending tools | Accept the validated model response |
| `completed` with tool calls | Process permitted calls and send their results; the workflow is not finished |
| `incomplete` | Inspect `incomplete_details`, retain partial output and choose an explicit recovery action |
| `failed` or missing/unexpected final status | Treat as failure or an invalid response; do not execute its tools |

<a id="대화-이어가기"></a>

## Continue a conversation

Each request supplies its complete current context. The gateway does not retrieve
earlier messages from a response ID. This second-turn example includes the earlier
user message and an illustrative assistant answer:

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

In your application, insert the actual completed messages, not the illustrative
answer above. Preserve their order and append the new user message. Keep
`store:false`; `previous_response_id`, stored-item references and conversation
storage are unsupported. Plain text is portable; opaque reasoning and incomplete
provider state are not automatically portable between routes. Follow the
[continuity contract](continuity.md) for restart or model changes.

<a id="함수-도구-호출-왕복"></a>

## Complete a function-tool round trip

The gateway returns tool calls to the host. It does not run a tool. The following
first request defines an `echo` function whose only effect is returning its input
text. A converted route needs `function_tools` support for this example.

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

After a completed model response, an item in its `output` can have this form:

```json
{
  "type": "function_call",
  "call_id": "call_echo",
  "name": "echo",
  "arguments": "{\"text\":\"hello\"}"
}
```

Use the actual returned call, including its `call_id` and arguments. Assuming the
first request and decoded response are named `first_request` and `first_response`,
the host can execute this explicitly permitted example and construct the follow-up:

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

POST `second_request` to the same gateway endpoint. It contains the original
context, the model's messages and calls, and a `function_call_output` for every
call. Each result uses the call's `call_id`; a response ID or item ID is not a
substitute. This example uses an input array so it can append the earlier items.

For parallel calls, retain all calls in order, collect all required results and
then continue with the model. Converted routes reject duplicate/mismatched results
and assistant continuation while results are still pending. Do not reconstruct a
call from its name alone or execute partial arguments from a stream. Real tools
need the application's authorization, argument validation and completion records;
a retry or reconnect must not repeat an already completed side effect.

<a id="스트리밍과-취소"></a>

## Read a stream and handle cancellation

Use `stream:true` and consume SSE incrementally. `-N` disables curl output buffering;
`--max-time 120` is an example consumer deadline, not a gateway default. Choose the
deadline for the application and keep its timeout outcome distinct from completion.

```sh
curl --noproxy '*' -N --max-time 120 "${GATEWAY_BASE_URL}/responses" \
  -H "Authorization: Bearer ${ARG_LOCAL_TOKEN}" \
  -H 'Content-Type: application/json' \
  -d '{"model":"example/writer","input":"Reply with hello.","max_output_tokens":256,"store":false,"stream":true}'
```

A text response can follow this event order; tools introduce their own item and
argument events. Parse complete SSE events before parsing each JSON payload.
TCP chunks can split UTF-8 characters, event lines or JSON values.

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

| Observation | Meaning |
|---|---|
| HTTP 200 or `response.created` | A response has started; generation is not yet complete |
| `response.completed` | Validate the final response and process any tool calls |
| `response.incomplete` | Generation ended incompletely; inspect `incomplete_details` |
| `response.failed` | The provider reported failure; retain the failure state |
| EOF, transport error or deadline without a valid terminal event | Interrupted or uncertain result, not successful completion |

The native route forwards provider events. Converted routes emit validated
completion/incomplete events; a provider or conversion failure can instead close
the stream after headers are sent. Do not expect a new JSON error body in that case.
The consumer's state handling should follow this outline:

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

The [basic Python client](../examples/client.py) prints raw response bytes. Its
successful exit at EOF is not semantic completion validation. Keep provisional
text separate from committed output and execute tools only after their complete
arguments and final response have been validated.

Close the active response when cancelling. The gateway stops upstream reading,
but work already accepted by the provider may continue or be billed. Heartbeat
bytes can keep an idle stream alive, so enforce an overall application deadline.
The [timeout contract](protocol.md#resources-and-failures) defines the gateway's
individual waiting limits.

<a id="애플리케이션에-연결"></a>

## Connect the gateway to an application

1. Select the configuration and required model features. Inspect `check-config`
   and the offline `manifest`; neither calls the provider.
2. Start the gateway with an explicit child environment containing all configured
   provider keys and an instance-specific local token. Give the consumer only the
   local token and registered aliases.
3. Read and verify the bounded readiness message, configuration digest and actual
   address. Drain stderr separately; a live `/readyz` is not provider qualification.
4. Start model requests only after readiness. Track active calls, request IDs,
   deadlines and tool completion. Apply settings or key changes by restarting the
   instance and checking its new readiness; there is no automatic reload.
5. On shutdown, stop new work, cancel or resolve active requests, close their
   responses and terminate the owned gateway process within a host deadline.
   Treat interrupted work as uncertain rather than automatically replaying it.

See [process supervision](embedded-design.md#process-lifecycle) for the full
startup/shutdown contract, [continuity](continuity.md) for history recovery, and
[troubleshooting](troubleshooting.md) for observable failures.
