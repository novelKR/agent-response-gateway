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

Local iteration and submission use an explicit impact plan. The planner reports
selected checks, tools, package consumers and the reason for broader coverage.
It uses Git and manifest reads without resolving Cargo dependencies. Unknown
paths, unavailable revisions, dependency and validation-policy changes select
full coverage. `--base BASE --head HEAD` describes a commit range; rename and
deletion inputs include both affected paths. Range plans are not executed against
an unrelated local worktree. The local full profile is not a release qualification.

```sh
python3.14 -B scripts/validation.py plan --worktree
python3.14 -B scripts/validation.py run --worktree
python3.14 -B scripts/validation.py run --staged
python3.14 -B scripts/validation.py plan --profile full --base BASE --head HEAD
```

Prepare reported tools and locked npm dependencies before execution. Web-only
presentation checks require no Cargo or crate-license tooling. Local results are
written under `.local/validation/`. Full Python discovery includes the separate
real SPDX integration tests. PR CI executes the selected impact plan; main, manual
full checks and release qualification retain full execution. An impact plan is not
evidence that a test ran: inspect its execution set and the required-check result.
`force_full` in the validation policy only expands coverage.

The registered job families are `validation-plan`, `conformance-prepare`, `web-windows`, `targets`, `format`, `rust`,
`publication`, `licenses`, `codex-conformance`, `api-codecs`, `usage-recorder`,
`management-web`, `docs` and `package-smoke`. Both native matrices use all four
platforms from the [common target definition](../scripts/release_targets.py).
`ci-required` requires the planner and every selected job to succeed. Only jobs
explicitly outside the plan may be skipped. Missing, extra, unexpectedly skipped,
cancelled or failed results block it. The required workflow itself is never omitted
by a path filter.
Register a required check in branch protection after its first successful run.

Native Rust checks select changed packages and their consumers within the existing
supported platform set. Full checks preserve the original package/feature matrix.
The recorder retains its separate database and upgrade checks. Web styles and
independent presentation components need no Rust fixture; the API client and the
application container that owns authentication keep the real API fixture.

Inspect observed Actions timing without equating job-seconds with billing:

```sh
python3.14 -B scripts/validation_metrics.py RUN_ID --output .local/validation/run.json
```

The report distinguishes required-gate latency, completed job durations, pending
work and deployment waits. It preserves per-step observations; differing queue,
cache and concurrency conditions are not a controlled performance comparison.

Native conformance runs as four isolated groups: editing, protocol/continuity,
managed reasoning and legacy migration. External codecs use protocol and
editing/accounting groups. Every original scenario recipe remains registered
exactly once. A preparation job builds the same default-feature native inputs
and verifies the pinned runtime; consumers verify source, run/attempt, platform,
toolchain, input locks and every file hash before installing shared executables.
The prepared archive is retained for three days, group results for 14 days.
Prepared inputs are not cached test success or release signatures.

Shared gateway archives include corresponding source and the original dependency
and Rust toolchain notices. Each group restores and verifies the pinned Codex
bundle from its upstream cache; it is not republished inside the shared archive.
The immutable legacy writer is built only in its migration group.

The Web producer builds the exact source export once. Native package jobs consume
its checked assets, while a separate Windows job retains Web build compatibility.
License-tool caches are separate from dependency/build caches. Restored tool
versions are verified before use; a mismatch fails instead of reinstalling silently.

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

Documentation deployment has its own main-only workflow. It selects changes to
documentation inputs or deployment policy, and supports explicit manual deployment.
The workflow checks reviewed pages, publication boundaries, Web notices and static
output before packaging the same artifact for Pages. Product conformance and
native package jobs are not its dependencies. After deployment, the served build
manifest must match the source commit and exact manifest bytes from the build.

The deployment calls the public [docs-actions workflow](https://github.com/novelKR/docs-actions)
at the full commit recorded in the [consumer lock](../.github/docs-pages-deploy.lock.json).
Only its deployment job receives pages: write and id-token: write. Configure Pages
for GitHub Actions and restrict the github-pages environment to main. Required
reviewers still control publication; PRs do not deploy. Deployment runs serialize
without interruption, separately from superseded integration checks.

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
