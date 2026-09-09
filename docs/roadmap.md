<a id="구현-마일스톤과-우선순위"></a>

# Implementation milestones and priorities

[English](roadmap.md) | [한국어](ko/roadmap.md)

This document defines implementation dependencies, completion criteria and
remaining acceptance gates. A plan is not implementation, structural approval,
real-provider qualification, a public release or consumer operational acceptance.
Current support follows the [HTTP](protocol.md) and [IR](ir.md) contracts.
[Work items](work-items.md) and the [PR/CI procedure](github-workflow.md) track progress.

The public-source baseline reviewed on 2026-09-09 is `9916c4f`. Approved G00–G20
design/implementation, local and hosted checks, and PR merges are complete.
M0–M6 tracking milestones are closed; M7 retains the [stable Codex baseline replacement](https://github.com/novelKR/agent-response-gateway/issues/33).
This describes implementation through synthetic validation. Real-provider
qualification, consumer production activation/acceptance and formal release
remain separate. The sections below preserve each stage's purpose and acceptance
criteria; they do not describe every item as unimplemented.

P0 covers prerequisites, P1 the multi-API and embedded-use core, P2 distribution
of a verified product and P3 separate extensions. Dependencies still apply within
the same priority. Estimate effort and dates after fixing the execution contract.

<a id="출발점-m0--responses-전달과-ir-기반"></a>

## Starting point: M0 — Responses transport and IR foundation

The foundation implemented an independent Rust executable, loopback authentication,
model aliases/provider-key selection, Responses JSON/SSE forwarding, readiness
and shutdown, and IR v1 request codecs, capabilities, custom JSON bridging,
event validation and origin binding.

At initial M0, IR and HTTP conversion were separate. M2–M4 connected the IR to
Messages and Chat Completions HTTP routes and added actual-Codex/synthetic-upstream
checks for all three routes. Follow the [common matrix](conformance.md) for current
support. M0 completion alone is not evidence for those later checks or real-provider
acceptance.

<a id="전체-순서"></a>

## Overall sequence

| Priority | Milestone | Prerequisite | Claim supported by completion |
|---|---|---|---|
| P0 | M1. Validation foundation and pinned Codex contract | M0 | Exact runtime contract and reproducible baseline tests |
| P0 | M2. Model routes and capability profiles | M1 contract | Route selection and pre-request capability admission with a stated verification scope |
| P1 | M3. First converted API's complete tool round trip | M1, M2 | Codex compatibility for the first API's declared features |
| P1 | M4. Second converted API and common regression | M3 | Three upstream API types through Responses input |
| P1 | M5. Consumer startup and bounded embedded acceptance | M1, M2, M3 | Automatic startup/shutdown and bounded embedding of verified routes |
| P1 | M6. Long-running work, compaction and resume | M5, applicable M3/M4 route | Long work and continuity after restart for that route |
| P2 | M7. Verified distribution and pinned consumer adoption | Feature validation, distribution/rights preparation | Independent distribution and consumer adoption within the verified scope |
| P3 | X. More input APIs and service capabilities | Stable core, separate approval | Separately defined extensions |

M4 and M5 can proceed after M3 in parallel. M5 startup scaffolding for a verified
Responses route can begin in M1, while converted-route acceptance waits for M3.
Distribution/rights preparation can start in M1 and blocks unready external
contribution, contracting and distribution. These are planning dependencies, not
a requirement for any execution method or agent delegation. Follow-up work can
start from preceding contracts, implementation and mock tests. Other adapters can
be developed while live qualification is pending, without marking that earlier
milestone's full acceptance complete or operating an unverified route.

<a id="p0--m1--검증-기반과-codex-실행-계약-고정"></a>

## P0 / M1 — Validation foundation and pinned Codex contract

Observe actual consumer requirements before implementing provider adapters and
make the baseline checks repeatable.

- Define supported/rejected local refs beyond commit/tag, their inspection scope
  and synthetic repository regressions. Do not delete or silently omit refs to pass.
- Pin the Codex version, executable hash, control schema and final settings.
  Distinguish control-schema integrity from model HTTP compatibility.
- Connect actual Codex to a synthetic Responses upstream and check tools, custom
  formats, namespaces, tool-result re-entry and completion.
- Classify text, function/custom tools, images, strict output, cancellation,
  resume and compaction as required, optional or unsupported. Design long-running
  state ownership and resume.
- Fix Messages as the first converted API and Chat Completions as the second.
  Select actual qualification models and cost limits separately for each API.
- Define model input/output limits, context accounting, compaction reserve,
  retry ownership and execution budgets using verified provider-specific evidence.

Completion: repository and publication checks pass, and the pinned Codex baseline
reproduces tool round trips, cancellation and approval denial. Mock success remains
distinct from provider qualification. Missing executables or an unresolved contract
leave M1 incomplete.

<a id="p0--m2--모델-경로와-기능-프로필의-실행-연결"></a>

## P0 / M2 — Connect model routes and capability profiles

Connect the pure IR contract to real request handling without unintentionally
narrowing original native forwarding.

- Connect API, authentication, profile version and limits to each model route.
- Resolve aliases and freeze RouteSnapshot per request. Bind execution across
  requests in the consumer and verify it under M5/M6.
- Distinguish verified native transport from conversion. Do not force every native
  request through a narrower IR codec.
- Derive converted requirements once and reject missing features before sending.
  Document native qualification and handling of unknown features as well.
- Apply output limits and connect M1 context-fit requirements with consumer settings.
  Do not present byte counts or unsupported token estimates as exact context guarantees.
- Establish configuration compatibility, migration and the authority behind
  capability declarations.

Completion: a synthetic upstream receives zero requests for capability/limit
violations; admitted requests use only the frozen route. Existing native forwarding
regressions pass. An API without an adapter is not declared supported or silently
substituted with another API.

<a id="p1--m3--첫-변환-api의-전체-도구-왕복"></a>

## P1 / M3 — First converted API's complete tool round trip

Complete Messages first and add Chat Completions in M4. Request conversion alone
is not completion while streaming and custom tools remain unfinished.

- Connect request encoding, authentication, JSON responses and errors to HTTP.
- Connect SSE/UTF-8 framing, provider parsing, Event IR validation and Responses encoding.
- Preserve function/custom names, namespaces, item/call IDs and result association.
- Restore partial JSON wrapper strings and parallel calls. Do not claim full tool
  compatibility if required M1 grammar cannot be represented faithfully.
- Implement required images, strict output and reasoning controls; explicitly
  describe the remaining support levels.
- Exercise disconnection, output limits, errors, cancellation, slow consumers and
  bounded buffers.
- Measure provider headers, first model event, first text and completion. Distinguish
  these from actual consumer screen-rendering latency.

Completion: actual pinned Codex performs text → tool call → result re-entry →
follow-up, including custom tools, approval denial, cancellation and transport
loss. Record synthetic wire and actual-provider results separately. Without live
provider testing, claim implementation/mock compatibility only and leave
qualification incomplete.

<a id="p1--m4--두-번째-변환-api와-공통-회귀"></a>

## P1 / M4 — Second converted API and common regression

- Implement the second API's request, response, stream and tool-round-trip scope.
- Reuse common IR capability and event invariants; keep wire differences in adapters.
- Extract only common behavior observed in the two actual adapters. Do not require
  a hypothetical provider framework or crate split before implementation.
- Compare equivalent fixtures and failures across native and both converted routes.

Completion: all three upstream API paths provide their declared features through
Responses input, with shared regression coverage and per-model qualification
status recorded. One passing model does not qualify every model at its provider.

<a id="p1--m5--소비자-내장-기동과-제한된-사용-수락"></a>

## P1 / M5 — Consumer startup and bounded embedded acceptance

The consumer runtime manager owns the main implementation. The gateway contains
reusable integration contracts only. Keep consumer names, paths, configuration
and operational records out of public source.

- Verify startup scaffolding with a pinned local-test binary and hash manifest.
  External distribution follows M7's source, notice and rights conditions.
- Connect dedicated configuration, a new local token per launch, readiness checks
  and subsequent Codex startup.
- Define/implement provider-key inheritance and the token's model/project/usage
  scope. Treat necessary generic HTTP authorization expansion as a separate design.
- Bind Codex model selection to the gateway route and final context settings.
- Freeze route/policy during execution and manage explicit changes without fallback.
- Test startup failure, shutdown order, orphan cleanup, cancellation, budget exhaustion
  and recovery.

Completion: a verified route becomes ready without separate manual installation or
startup; tools, approval and cancellation work without changing personal settings
or authentication. This is bounded task acceptance. Long use requiring compaction
and resume also needs M6.

<a id="p1--m6--장기-실행압축재개-수락"></a>

## P1 / M6 — Long-running work, compaction and resume

The approved [ownership design](continuity-design.md) retains stateless gateway
HTTP transport. The [continuity contract](continuity.md) validates host-owned Codex
history and private journals; consumers own persistence, approval and recovery.

- Bind runtime, gateway, route, profile, credential generation, history and request budget.
- Use Codex local compaction and host-history resume. The gateway continues rejecting
  remote compact, stored-response lookup/deletion and `previous_response_id`.
- Preserve opaque state only at the same origin. Explicit model switching starts a
  new thread, binds portable text/completed tool results and records omitted state.
- Persist in-flight state before dispatch and recheck actual files, routes and tool
  association after restart. Reject changed bindings and duplicate completed execution.
- Keep uncertain outcomes uncertain. Require host review and reconciliation before
  resume; do not retry automatically.
- Distinguish host-control budgets from internal HTTP attempts and billing. Primary
  transport retries remain zero; real-provider spending limits are selected separately.

Completion: each applicable route tests resume after tools/compaction, process
restart, model switching, state mismatch and approval denial, and the consumer
accepts long-running operation. Do not extend acceptance to an unfinished M4 route.
Storage-format changes require checks of previous-version readability and recovery
of compatible state, executable and configuration.

<a id="병행-준비와-p2--m7--공개-배포권리소비자-채택"></a>

## Parallel preparation and P2 / M7 — Distribution, rights and consumer adoption

Preparation can start in M1. A completed commercial agreement is not a prerequisite
for purely local development; distinguish the rights and distribution conditions
of each activity.

- Establish the rights holder/contact, provenance, relicensing permissions and
  consent records for external contributions. Keep their merges on hold until ready.
- Prepare commercial agreements independently; a policy is not an alternative grant.
- Record verified commit, toolchain, lockfile, target and hashes together.
- Generate and verify corresponding source, actual notices, SBOM and provenance.
- Verify actual public-source/CI/release results and connect consumer hash/provenance checks.
- Restore the previous verified combination after failed updates, including state compatibility.

Completion: tests for the distributed scope, public boundary/archive/package
membership and source/notice/provenance checks pass, and a consumer verifies and
adopts the pinned version. Distinguish AGPL distribution from separately negotiated
permission and verify the actual distribution conditions.

A limited preview can be released separately after its M3/M4 feature checks and
distribution conditions. Do not require full long-running acceptance for every
preview or claim compaction/resume acceptance without it. A long-running embedded
release requires the route's M4/M5/M6 acceptance and distribution conditions.
Follow [release procedures](release.md) and [integration responsibilities](integration.md).

<a id="p3--x--별도-확장"></a>

## P3 / X — Separate extensions

Other client input APIs, WebSocket, OAuth/account pools, an administration UI,
public-service/tenant mode and in-process embedding require separate requirements
and approval. They are not initial-core completion criteria. Preserve loopback-only
access and the default no-retry/no-fallback policy.

<a id="각-마일스톤의-변경검증-규칙"></a>

## Change and verification rules for every milestone

- Track implementation, mock checks, actual Codex checks, real-provider qualification,
  consumer acceptance and distribution verification separately; explain inapplicable stages.
- Before structural, public-configuration, state, authentication, runtime-dependency
  or deployment changes, present impact, alternatives, compatibility, cost, risk,
  recovery and verification, and obtain required approval. This roadmap is not a
  blanket authorization to apply them.
- Rust changes require formatting, Clippy and locked tests plus updated support
  documentation. Follow repository instructions for publication regressions and
  pre-commit checks.
- Default tests use synthetic fixtures and mock upstreams. Live calls, installation,
  deployment, publication and consumer activation stay within their applicable authorization.
- Retain a recoverable prior binary/configuration and support contract at each
  milestone. Never silently substitute another provider or weaker feature after failure.
