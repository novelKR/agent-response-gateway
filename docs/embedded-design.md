<a id="g13--호스트가-소유하는-내장-프로세스-계약"></a>

# G13 — Host-owned embedded process contract

[English](embedded-design.md) | [한국어](ko/embedded-design.md)

Status: **explicitly approved on 2026-09-08; implemented and covered by synthetic contract tests**.
This is a generic gateway contract. It does not activate a consumer, select a
consumer runtime upgrade, enable service mode or create a release.

<a id="근거원인권고"></a>

## Evidence, root cause and recommendation

Before G13, the gateway accepted a configuration file, bound a numeric loopback
address, emitted readiness after registering signal handlers, and supported bounded
shutdown. `check-config` and the library router were useful embedding primitives,
but the CLI lacked a versioned description of resolved routes, defaults, profiles
and limits. The original version/address readiness fields could not bind the host's
expected effective configuration to its child. The implemented interface below
adds that binding while preserving those existing fields.

**Strongly recommended; high confidence:** add a small versioned, offline manifest
command and include its configuration digest in readiness. Keep process supervision,
Codex configuration, credentials, run history and workflow decisions in the host.
This centralizes interpretation in the gateway's existing Config/ResolvedRoute
boundary and avoids a second host implementation of routing/default rules.

A complete conservative local alternative exists: the host can freeze and hash the
raw TOML bytes plus the executable and use the original readiness fields. That rejects
semantically identical reformattings and leaves the host without a standardized
resolved-route report. The selected M5 contract instead makes that reusable report
explicit. No gateway process manager or consumer-specific orchestration layer is
needed.

<a id="cli와-manifest-제안"></a>

## Proposed CLI and manifest

`agent-response-gateway manifest --config <path>`:

- Parse and validate configuration with the same code as serve/check-config.
- Produce one JSON object on stdout and exit successfully; no listener, credential
  read, download, provider probe or model call occurs.
- Report `gateway-embedded-manifest/v1`, package name/version, the Responses client
  API and the `host-supervised-process/v1` lifecycle contract.
- Include a normalized configuration object containing configured listen address,
  optional source URL, local token environment-variable reference, limits and a
  stable alias-ordered route list. The projection also lists every configured
  provider credential environment reference, since existing serve validates keys
  for unused configured providers as well.
- Each route contains alias, provider ID, resolved endpoint, upstream model/API,
  auth scheme, credential environment-variable reference, optional Messages version,
  adapter version, capability-profile ID/version/support, context/output limits and
  declared tested Codex version. Missing optional values are explicit nulls; native
  unqualified passthrough remains explicitly described as such.
- Include `configuration_sha256`, computed from the normalized configuration only:
  recursively key-sorted JSON, UTF-8, compact separators, no trailing newline.
  This representation contains integers, booleans, strings, objects, arrays and
  nulls, but no floating-point fields. Aliases/routes use deterministic ordering.
- Secret values, auth headers, credentials, prompts, history, private host paths,
  consumer identities and operational records never enter the manifest. Environment
  variable names are references, not credential generations or secret verification.

The manifest is local configuration data: a host may store it privately but must
not upload its real topology or identifiers as public test/release evidence. Public
examples and CI use synthetic configuration. A digest is a consistency check, not
a signature or proof of provider/model qualification. Hosts verify executable
integrity separately under the release/consumer adoption contract.

Keep `check-config` behavior compatible. Do not introduce another configuration
file format, write configuration during inspection, or allow manifest input to
choose a different endpoint or bypass normal configuration validation.

<a id="준비-통지와-수명"></a>

## Readiness and lifecycle

Retain the existing ready fields `event`, `address`, `base_url` and `version`.
Add `schema: gateway-ready/v1`, `manifest_schema: gateway-embedded-manifest/v1`
and `configuration_sha256`. They come from the same parsed Config instance used
to build the listener/router, so a changed file between offline inspection and
serve is detectable. Existing hosts that read the original fields continue to work.

The embedding host performs these steps:

1. Verify a pinned executable and its matching source/notices/compatibility record.
   Prepare a private immutable configuration snapshot and obtain the offline
   manifest. Validate its schema and bind its digest to the selected run settings.
2. Generate a distinct local token for this managed instance. Select upstream keys
   from the host's authoritative credential owner and construct an explicit child
   environment. The gateway receives only its required credentials; the Codex
   child receives only the local gateway token, never upstream keys.
3. Launch the exact gateway executable as a managed child and read one bounded
   readiness line from its stdout pipe. Keep stderr bounded/drained separately.
   Enforce a startup deadline and verify schema, digest, version and numeric
   loopback address. Do not attach to an unrelated existing process or assume a
   fixed port is free. Abort startup if the child exits or readiness differs.
4. Start Codex only after gateway readiness. Use a private dedicated Codex home,
   an explicit supported model/catalog/provider configuration, the readiness base
   URL, Responses wire API, disabled fallback/retries and the local token reference.
   Verify the actual resolved Codex model/provider before running a model turn.
5. On normal stop, the host first stops new work, resolves/interrupts active control
   turns as its workflow requires, closes Codex model connections, then terminates
   the managed gateway. SIGTERM/SIGINT retain the configured graceful shutdown
   window; the host enforces an outer deadline and reaps only its own child.
6. If startup/initialization fails, clean up every started child and temporary secret
   material. Unexpected child exit marks affected host runs failed or outcome
   Unknown as appropriate. Do not silently restart, reroute or replay model work.

Readiness establishes local preparation only. It does not establish provider key
validity, model availability, semantic qualification, successful tool execution,
workflow approval or consumer operational acceptance. Existing HTTP readiness and
model-list meanings remain unchanged. No HTTP shutdown/admin/manifest endpoint is
added; process supervision stays with the parent.

<a id="인증과-접근-범위"></a>

## Authentication and access scope

One local bearer token authorizes the existing protected endpoints for every model
alias configured in that gateway instance. It grants no file/tool/approval authority,
no arbitrary URL/key/header selection, no tenant identity and no public-service
access. A host that needs fewer model aliases supplies a narrower configuration;
this proposal does not add per-request actor or tenant rules.

Retain numeric loopback binding, HTTPS upstreams except numeric-loopback mocks,
explicit Bearer or API-key upstream authentication, no ambient proxy/redirects,
and one attempt per gateway request. Do not change the host's tool sandbox,
user-approval policy or domain authority when selecting a gateway model route.
The host owns the credential realm/generation required for resume. A matching
manifest or environment-variable name cannot substitute for that generation.

<a id="호환성과-연속성"></a>

## Compatibility and continuity

The manifest is an opt-in inspection interface and readiness additions are additive.
Hosts adopting this contract require its exact supported schema; future semantic
changes require a versioned contract/migration. Existing standalone serve and
check-config usage stays valid. No database or durable gateway state is introduced.

The approved [continuity design](continuity-design.md) remains authoritative for
host-owned binding/history. The host records gateway/Codex executable digests,
manifest configuration digest, selected route/profile, authoritative credential
generation and its existing thread/history checkpoints. G16 implements only the
approved generic validation/host contract; consumer records belong in consumer
repositories. A changed digest blocks blind same-context reuse until the host
performs the approved explicit transition or starts fresh.

This contract does not upgrade a consumer's pinned Codex. The temporary gateway test
baseline and a consumer runtime's own bundle/notice/state compatibility are distinct.
G14 must present its concrete consumer integration and any runtime or authentication
mode change for separate approval. No model/provider spend or production activation
is authorized here.

<a id="대안비용위험"></a>

## Alternatives, cost and risks

| Alternative | Consequence |
|---|---|
| Recommended CLI manifest + ready digest | Small reusable gateway change; one canonical interpretation; explicit host supervision |
| Existing CLI + host raw-file digest | Lowest immediate gateway cost; safe but byte-sensitive and no resolved-route report |
| Rust router linked in-process | Valid for some hosts, but changes deployment/lifetime coupling and still needs explicit host credential/state contracts |
| Gateway supervises Codex or workflows | Crosses the transport responsibility boundary; excluded |

Implementation cost is a manifest projection, canonical digest, CLI/readiness
connection and synthetic contract tests. It uses existing dependencies; it does
not need a process-management framework. Host integration and operational testing
are separate, larger G14/G17 work. The principal risks are digest drift, leaking
real configuration into shared records, launching Codex before validated readiness,
and confusing credential references with generations. The explicit schemas,
synthetic-only publication tests and host-owned launch/recovery rules address them.

<a id="검증과-롤백"></a>

## Validation and rollback

- Equivalent TOML ordering/default spelling produces the same effective digest;
  changes to endpoint/auth/model/profile/limits produce a different digest.
- Manifest inspection succeeds without credentials/network, emits no secret value
  and uses the same route errors as serve. Unsupported APIs remain explicit errors.
- A real synthetic child emits matching version/schema/digest and loopback address;
  existing ready fields remain present. A changed config changes readiness digest.
- Exercise bind failure, startup deadline, immediate shutdown after ready, shutdown
  with an active response and malformed/mismatched readiness in a host fixture.
- Assert key separation and no personal configuration inheritance in the embedding
  fixture. Existing authentication, cancellation, raw Responses and translated
  Messages conformance remain required.
- Run the repository's Rust, Python, license, public-source/history/archive and
  hosted CI checks. Mock host tests are not a consumer or live-provider acceptance.

Rollback uses a revert PR for the gateway change and the previous verified host /
executable/configuration combination. Older hosts continue reading existing ready
fields; new hosts that require the manifest contract fail explicitly when paired
with an older binary instead of silently bypassing verification. No state migration
or destructive cleanup is required.

<a id="구현된-인터페이스"></a>

## Implemented interface

`Config::manifest()` returns an immutable EmbeddedManifest projection. Its
configuration_sha256 getter supplies readiness from the same Config used for
router/listener setup. The CLI serializes the report without reading Secrets.
Known unsupported capability entries are normalized to their omitted default;
explicit legacy auth/API defaults and equivalent resolved URLs produce the same
digest. The shared SHA-256 primitive retains the existing grammar API and adds
no dependency.

The standalone Rust tests cover default/URL normalization, profile/auth/key-reference/
limit changes, stable route ordering, offline inspection, bind failure and bounded
shutdown with an active response. The synthetic host fixture validates the exact
manifest/ready schema, independently recomputes the digest in Python, checks the
configured numeric loopback address and verifies child credential/home separation.
The actual Codex suite performs these checks before every native/Messages scenario.
Malformed/mismatched readiness and an unready child deadline are separate tests.
These are generic contract checks; consumer activation and operational acceptance
still belong to G14/G17.
