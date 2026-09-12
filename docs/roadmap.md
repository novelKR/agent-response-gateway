<a id="implementation-milestones-and-priorities"></a>
<a id="구현-마일스톤과-우선순위"></a>
<a id="지원-범위와-개발-방향"></a>

# Support status and development direction

[English](roadmap.md) | [한국어](ko/roadmap.md)

The gateway provides a local Responses interface for three upstream API types.
This page summarizes the available capabilities and the checks required before
production use. Detailed behavior is defined in the [HTTP contract](protocol.md)
and [support matrix](conformance.md).

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
<a id="제공하는-기능"></a>
<a id="출발점-m0--responses-전달과-ir-기반"></a>

## Available capabilities

| Area | Scope |
|---|---|
| Transport | Loopback authentication, configured model aliases, separate provider keys, JSON/SSE and bounded shutdown |
| API conversion | Responses forwarding and declared Messages / Chat Completions profiles |
| Tools and output | Functions, custom text, namespaces, registered patch grammar and declared output controls |
| Embedding | Offline configuration manifest, readiness verification and host-supervised process lifecycle |
| Continuity | Host-owned history, local compaction, verified resume and explicit model switching |
| Distribution tooling | Four native targets, source/notices, dependency inventory, signed tag builds, automatic prereleases and approved formal promotion |

The automated suites use mock providers, including when running the actual pinned
Codex executable. They validate protocol and host contracts, not real-model output
quality or a particular application's production operation.

<a id="p0--m1--validation-foundation-and-pinned-codex-contract"></a>
<a id="p0--m1--검증-기반과-codex-실행-계약-고정"></a>
<a id="parallel-preparation-and-p2--m7--distribution-rights-and-consumer-adoption"></a>
<a id="병행-준비와-p2--m7--공개-배포권리소비자-채택"></a>
<a id="운영-전-검증"></a>

## Before production use

1. **Select and test the model.** Confirm the required tools, instruction behavior,
   output formats and context limits with the actual provider/model. Set a test
   cost limit and record the verified profile.
2. **Validate the application integration.** Exercise startup, credential isolation,
   tool approval, cancellation, compaction, restart and uncertain-request recovery
   using the application's own persistence and permissions.
3. **Verify the distribution.** Bind the version, source, executable, configuration,
   notices and provenance. Test adoption and recovery of that exact combination
   under the [release procedure](release.md).
4. **Maintain the Codex test runtime.** Replace the pinned prerelease with a stable
   runtime only after artifact/schema checks and the full [conformance suite](codex-contract.md).

A limited preview is assessed against its declared features. Long-running
embedding also requires the [continuity checks](continuity.md).

<a id="p3--x--separate-extensions"></a>
<a id="p3--x--별도-확장"></a>
<a id="현재-범위-밖의-기능"></a>

## Outside the current scope

Other client input APIs, WebSocket, OAuth/account pools, tenant-aware public
service mode and an administration UI are not supported. Expanding these areas
requires requirements and review of the affected API, authentication, persistence
and deployment contracts.

Current exclusions are implementation boundaries, not permanent product non-goals.
Optional credential brokers, account pooling, provider continuity and response-state
services may extend the [product configurations](index.md). They require explicit
contracts and implementation; this direction promises no release or fixed order.
The current observer protocol does not enable them.

Persistent design principles are shared semantic validation, explicit differences,
no silent feature removal, and no core dependency on a consumer's private domain.
Choose a user scenario first, then the needed core contract, optional modules and
application integration. Evaluate consumer version, provider/model, active extension
configuration, scenario and guarantee level together. Tool round trips, cancellation,
recovery and usage consistency must work across that combination; feature names
alone do not establish compatibility.

<a id="change-and-verification-rules-for-every-milestone"></a>
<a id="각-마일스톤의-변경검증-규칙"></a>
<a id="변경과-검증"></a>

## Changes and verification

Keep each change focused, describe its resulting behavior and preserve existing
compatibility. Use the [development workflow](github-workflow.md) for review and
required checks, and the [contribution policy](../CONTRIBUTING.md) for licensing
permissions. Current work and proposals are tracked in
[Issues](https://github.com/novelKR/agent-response-gateway/issues).
