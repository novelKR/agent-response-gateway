# Independent synthetic provider

This stdlib Python 3.11+ example implements `gateway-provider/v1` and
`synthetic-provider/v1`. Copy this whole directory outside the gateway checkout.
It imports no gateway crate, engine or SDK. Its license and corresponding source
are included. Explicitly provision the selected Python interpreter on the native
installation host; the build pins its absolute path and downloads nothing.

```sh
python3.14 -B package.py --output /absolute/new-package --target macos-arm64
```

Use the actual execution target. A target label does not cross-compile Python or
provision the pinned interpreter elsewhere. The script prints the exact manifest
SHA-256 for independent static inspection/install. The package must be explicitly
trusted: the process has the invoking user's native privileges and is not sandboxed.
Explicit manager activation and a configured model route select the provider.
Managed routes additionally require host continuity configuration; usage recording
requires a compatible Recorder contract. Follow the [verification procedure](../../../../docs/plugin-verification.md)
for standalone profiles and ordinary installed-host acceptance. Successful example
wire tests alone do not establish host persistence, consumer compatibility or
actual-provider operational qualification.

The executable uses big-endian four-byte length-prefixed JSON on stdin/stdout,
with a one-MiB example frame limit. Ready repeats the exact packaged capabilities.
Each process handles one prepare followed by JSON or SSE then exits. EOF, duplicate
JSON keys, wrong sequences and invalid state transitions fail; no network requests,
credential reads, host state handles or cancellation callbacks are implemented.

The host's admitted request becomes `{"query": <request>, "cursor": 0}` without a
root `model` key. The synthetic upstream replies with `{"answer":[{"text":"hello"}]}`.
Function calls use `{"call":"call_1","name":"lookup","arguments":"{}"}` in
`answer`. A subsequent admitted request can contain a `function_call_output` item;
it passes through `query.input` unchanged. No tool is executed by this example.

SSE uses event name `piece` and data `{"text":"hello"}`, followed by event `end`
whose data is the same full object used for JSON. Pieces produce nonterminal
Responses text progress. `end` marks semantic completion and `finish` returns the
full verified-output candidate; function calls are never released in progress.
Optional `meter` names the seven public counters. Missing meter is unobserved;
missing counters are not_reported, explicit zero is reported zero, and malformed
numbers are invalid. Only the host can validate arithmetic and assign provenance.

The managed wire example restores and increments an opaque `synthetic-counter`
version-one base64 JSON counter. It has no persistence/encryption implementation.
Wire tests exercise that value across process restarts; protected host persistence,
authorization, package binding and recovery require separate host integration tests.
This example does not support editing or reasoning progress and makes no actual
provider qualification claim. Adversarial process fixtures belong in tests, not
runtime flags or hidden input commands.
