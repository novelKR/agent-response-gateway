# GitHub work items

Each work item tracks its own PR and evidence. Issue closure does not imply provider qualification, consumer acceptance or a release. Consumer-owned work is implemented and validated in its own repository.

| Item | Milestone | Issue | Depends on |
|---|---|---|---|
| G00 | M0 | [GitHub workflow foundation](https://github.com/novelKR/agent-response-gateway/issues/1) | Baseline |
| G01 | M0 | [License evidence integration](https://github.com/novelKR/agent-response-gateway/issues/2) | Baseline |
| G02 | M1 | [Publication ref handling](https://github.com/novelKR/agent-response-gateway/issues/3) | G00 |
| G03 | M1 | [Pinned Codex execution contract](https://github.com/novelKR/agent-response-gateway/issues/4) | G00 |
| G04 | M1 | [Codex baseline conformance harness](https://github.com/novelKR/agent-response-gateway/issues/5) | G03 |
| G05 | M2 | [Model route and capability design](https://github.com/novelKR/agent-response-gateway/issues/6) | G04 |
| G06 | M2 | [Route and capability admission](https://github.com/novelKR/agent-response-gateway/issues/7) | G05 |
| G07 | M3 | [Messages request and response codec](https://github.com/novelKR/agent-response-gateway/issues/8) | G06 |
| G08 | M3 | [Messages streaming and custom tools](https://github.com/novelKR/agent-response-gateway/issues/9) | G07 |
| G09 | M3 | [Messages Codex conformance](https://github.com/novelKR/agent-response-gateway/issues/10) | G08 |
| G10 | M4 | [Chat Completions request and response codec](https://github.com/novelKR/agent-response-gateway/issues/11) | G09 |
| G11 | M4 | [Chat Completions streaming and tools](https://github.com/novelKR/agent-response-gateway/issues/12) | G10 |
| G12 | M4 | [Three-protocol conformance](https://github.com/novelKR/agent-response-gateway/issues/13) | G11 |
| G13 | M5 | [Generic embedding and access contract](https://github.com/novelKR/agent-response-gateway/issues/14) | G09 |
| G14 | M5 | [Consumer runtime embedding](https://github.com/novelKR/agent-response-gateway/issues/15) | G13 |
| G15 | M6 | [Continuity compaction and resume design](https://github.com/novelKR/agent-response-gateway/issues/16) | G04 |
| G16 | M6 | [Approved continuity implementation](https://github.com/novelKR/agent-response-gateway/issues/17) | G15, G14 |
| G17 | M6 | [Consumer long-running acceptance](https://github.com/novelKR/agent-response-gateway/issues/18) | G16 |
| G18 | M7 | [Release packaging and verification](https://github.com/novelKR/agent-response-gateway/issues/19) | G01, G09 |
| G19 | M7 | [Build provenance and release promotion](https://github.com/novelKR/agent-response-gateway/issues/20) | G18 |
| G20 | M7 | [Consumer pinned release adoption](https://github.com/novelKR/agent-response-gateway/issues/21) | G19, G17 |

Branches use `codex/m<N>-<purpose>`. Preserve logical commits with merge commits. Design and operational approval gates are described in the [roadmap](roadmap.md) and [GitHub workflow](github-workflow.md).
