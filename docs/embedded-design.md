<a id="alternatives-cost-and-risks"></a>
<a id="evidence-root-cause-and-recommendation"></a>
<a id="g13--host-owned-embedded-process-contract"></a>
<a id="g13--호스트가-소유하는-내장-프로세스-계약"></a>
<a id="근거원인권고"></a>
<a id="대안비용위험"></a>
<a id="호스트-애플리케이션에-게이트웨이-내장하기"></a>

# Embedding the gateway in a host application

[English](embedded-design.md) | [한국어](ko/embedded-design.md)

The host application starts and supervises the gateway as a child process. It
checks the executable and configuration before starting its agent, supplies
credentials separately and controls shutdown and recovery.

<a id="cli와-manifest-제안"></a>
<a id="proposed-cli-and-manifest"></a>
<a id="설정-명세"></a>

## Configuration manifest

`agent-response-gateway manifest --config <path>` validates the configuration and
writes one JSON object to stdout. It does not start a listener, read credential
values or contact a provider. The report uses `gateway-embedded-manifest/v1` and
identifies the `host-supervised-process/v1` lifecycle contract.

The manifest contains:

- Package name/version and the Responses client API.
- Listen address, optional source URL, local-token environment reference and limits.
- All configured provider credential environment references, including unused
  providers whose credentials are checked when the server starts.
- Routes ordered by alias: provider, resolved endpoint, upstream model/API, auth
  scheme, credential reference, optional Messages version, adapter/profile
  versions, feature support, context/output limits and tested Codex version.
- `configuration_sha256`, the SHA-256 of the normalized configuration.

Missing optional values are null. The digest uses UTF-8 JSON with recursively
sorted keys, compact separators and no trailing newline. Route ordering is stable;
values are integers, booleans, strings, objects, arrays or null, without floating
point. Equivalent defaults and URL spellings normalize to the same digest.

The report contains configuration references, not secret values, prompts or
history. Keep real deployment addresses and identifiers private. The digest
checks configuration consistency; verify executable integrity separately.
`check-config` uses the same validation rules without producing this report.

<a id="readiness-and-lifecycle"></a>
<a id="준비-통지"></a>
<a id="준비-통지와-수명"></a>

## Readiness

The first `serve` stdout line describes the running child:

| Field | Meaning |
|---|---|
| `event` | ready |
| `address` | Bound numeric loopback address |
| `base_url` | Responses client base URL |
| `version` | Gateway version |
| `schema` | `gateway-ready/v1` |
| `manifest_schema` | `gateway-embedded-manifest/v1` |
| `configuration_sha256` | Digest of the configuration used by the listener and router |

Compare these values with the inspected executable and manifest. A different
configuration digest or an unexpected address must stop startup. Readiness
confirms local startup, not provider credentials or model availability.

<a id="프로세스-수명"></a>

## Process lifecycle

1. Verify the pinned executable, source, notices and compatibility record.
   Create a private configuration snapshot and inspect its manifest.
2. Generate an instance-specific local token. Give the gateway its required
   provider credentials; give the agent only the gateway token. Use explicit
   child environments and keep personal agent settings separate.
3. Start the exact gateway executable. Read one bounded readiness line within a
   startup deadline, drain stderr separately and verify the reported values.
   Do not attach to an unrelated process or assume a fixed port is available.
4. Start the agent after verified readiness. For Codex, use a dedicated private
   home, the supported model/catalog profile, Responses API and reported base URL.
   Disable fallback/retries and confirm the effective model and provider.
5. On shutdown, stop new work, interrupt or resolve active turns, close agent model
   connections and terminate the gateway. Unix SIGTERM/SIGINT and Windows
   CTRL_C_EVENT/CTRL_BREAK_EVENT use the configured grace window. On Windows,
   create the child with CREATE_NEW_PROCESS_GROUP and send CTRL_BREAK_EVENT to
   that group only. The host enforces an outer deadline and reaps its own child;
   TerminateProcess is forced cleanup, not graceful shutdown.
6. On startup failure, clean up started children and temporary credentials.
   Unexpected exit marks affected work failed or uncertain. Do not silently
   restart the model request or select another route.

There is no HTTP shutdown, administration or manifest endpoint. Process control
belongs to the parent application.

<a id="authentication-and-access-scope"></a>
<a id="인증과-접근"></a>
<a id="인증과-접근-범위"></a>

## Authentication and access

One local Bearer token covers every configured model alias in that gateway
instance. Restrict the instance configuration when a host needs fewer aliases.
The token grants no file access, tool execution, approval or tenant identity.

Connections use numeric loopback addresses. Upstream URLs require HTTPS except
for numeric-loopback mocks. Provider authentication is explicit, and each request
makes one upstream attempt without inherited proxies, redirects or fallback.
The application's tool permissions and approval rules remain in force.

<a id="compatibility-and-continuity"></a>
<a id="재개와-호환성"></a>
<a id="호환성과-연속성"></a>

## Resume and compatibility

Hosts using the manifest must require its supported schema. Configuration,
route/profile, executable and credential changes are checked before reusing
history under the [continuity contract](continuity.md).

An environment-variable name does not identify the current credential generation.
The host must obtain that identity from its credential manager. If a required
binding differs, perform an explicit transition or start a fresh context.

The gateway stores no durable session state. Agent runtime selection and the
compatibility of its saved history remain host responsibilities.

<a id="validation-and-rollback"></a>
<a id="검증과-롤백"></a>
<a id="검증과-복구"></a>

## Validation and recovery

Tests cover configuration normalization, offline inspection, matching readiness,
bind failure, startup timeout and bounded shutdown with an active response.
Host fixtures independently recompute the digest and verify child credential and
settings isolation. Use the [Codex test guide](../tests/codex/README.md) for setup.

For recovery, restore a verified host/executable/configuration combination and
compatible history. A host requiring this manifest must reject a binary that
cannot provide it. Actual provider and application behavior still require tests
in the intended deployment.

<a id="implemented-interface"></a>
<a id="rust-인터페이스"></a>
<a id="구현된-인터페이스"></a>

## Rust interface

`Config::manifest()` returns an immutable EmbeddedManifest. Its
configuration_sha256 is also used for readiness from the same Config that creates
the router and listener. Serialization does not read Secrets. Known unsupported
features normalize to their omitted defaults.
