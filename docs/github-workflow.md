<a id="github-개발검증배포-절차"></a>

# GitHub delivery workflow

[English](github-workflow.md) | [한국어](ko/github-workflow.md)

Use issues to define scoped changes and pull requests to review them. This guide
describes branch management, required checks and release approvals.

<a id="브랜치커밋검토"></a>

## Branches, commits and reviews

Use a separate worktree and a `codex/<purpose>` branch for each bounded PR.
Keep existing user changes and independent local repositories intact. A PR has
one reviewable purpose and one to three logical commits; implementation and its
regression tests belong together. Merge commits preserve the individual history.

Start with a draft PR linked to its work-item issue. Record the problem, resulting
behavior, relevant checks, compatibility and recovery. Review the exact head SHA
before enabling auto-merge. A new head requires a fresh review; disable any prior
auto-merge before updating it. Merge one PR at a time and update the next branch
against the new main. Do not force-push shared history or bypass required checks.

Changes within an approved design may auto-merge after code review and required
checks. Public contract, authentication, persisted state, CI write permission
and release changes require explicit approval. Design-only work items precede
their implementation. A label is a tracking aid, not the source of approval.
An agent review is not an independent human GitHub approval. Do not self-approve
a PR using the author's identity.

<a id="필수-검사"></a>

## Required checks

The required jobs are `targets`, `format`, `rust`, `publication`, `licenses`,
`codex-conformance`, `docs` and `package-smoke`. Both native matrices use all four
platforms from the [common target definition](../scripts/release_targets.py).
`ci-required` succeeds only when the complete, explicitly named prerequisite set succeeds.
Missing, extra, skipped, cancelled and failed jobs block it. No path filter silently omits a required check.
Register a required check in branch protection after its first successful run.

The license job verifies locked evidence, runs all script tests and reproduces
notice bundles with the pinned development tool. Codex conformance runs the actual
pinned executable with the gateway and a synthetic upstream. Every scenario is
reported; any failure blocks the aggregate. Package checks build a candidate from
the committed source and verify archives, corresponding source, notices and the
target SBOM. Candidate signing and approved release promotion use separate workflows.
The workflow uses Rust 1.98.0 and Python 3.14 on Ubuntu 24.04 x64/ARM64, macOS 15
ARM64 and Windows 2025 x64. Each native CLI smoke also checks configuration,
readiness, local authentication, three synthetic routes and graceful shutdown. Full history is fetched for publication checks. Actions are pinned by SHA.
Caches are partitioned by OS, architecture, toolchain and relevant lock files.

The documentation job uses Node 24.21.0 and npm 11.19.0 to check the reviewed
language pairs, static output, local preview boundary and web dependency notices.
It retains the verified site for review for 14 days. It has no Pages deployment
permission; artifact retention is not publication approval.

On main push or manual main CI, the same checked site is packaged for Pages.
After ci-required succeeds, the deployment job calls the public
[docs-actions workflow](https://github.com/novelKR/docs-actions) at the full commit
recorded in the [consumer lock](../.github/docs-pages-deploy.lock.json).
Only that job receives pages: write and id-token: write; the caller owns its
Pages site and github-pages environment. Configure Pages to use GitHub Actions
and restrict that environment to main before publication. Add required reviewers
there when a separate publication approval is needed. PRs do not deploy.

Adopt central updates through a reviewed PR that changes the workflow SHA, lock
and non-executing test snapshot together after central contracts CI succeeds.
Build tools and document validation remain in this repository. Verify the actual
site URL and served build-manifest.json after deployment; a central CI success
alone does not establish a live site.

PRs run without provider secrets and with read-only repository permissions.
Do not execute untrusted PR code with write credentials or on a consumer host.
Keep runtime execution logs, private content and provider payloads out of public
artifacts. Tests use synthetic inputs and mock upstreams, even when exercising
the real pinned Codex executable.

<a id="릴리스와-의존성-변경"></a>

## Release and dependency changes

Dependency and action updates arrive as separate Dependabot PRs. Review the
resolved lockfile and the required license evidence; an automated update is not
approval of new dependencies or licensing terms.

Tag candidate success automatically publishes a complete Pre-release. A protected
release approval then promotes the same Release and verified files without a
rebuild. Candidate, publication and formal promotion use separate workflows. Verify source,
notices and provenance before adoption, and retain the preceding verified binary,
configuration and compatible state. See [release procedures](release.md).

Consumer integration work belongs in the consumer's repository. Public issues
may track the generic contract but must not link to private implementation or
operational records. A merged gateway PR does not close consumer acceptance.
