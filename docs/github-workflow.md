# GitHub delivery workflow

The [roadmap](roadmap.md) defines M0–M7. GitHub milestones and work-item issues
track implementation separately from validation, qualification and adoption.

## Branches, commits and reviews

Use a separate worktree and a `codex/m<N>-<purpose>` branch for each bounded PR.
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

## Required checks

The required jobs are `format`, `rust-linux`, `rust-macos`, `publication`, `licenses`
and `codex-conformance`, plus the Linux/macOS `package-smoke` matrix.
`ci-required` succeeds only when every required prerequisite succeeds, including
after cancellation or failure. No path filter silently omits a required check.
Register a required check in branch protection after its first successful run.

The license job verifies locked evidence, runs all script tests and reproduces
notice bundles with the pinned development tool. Codex conformance runs the actual
pinned executable with the gateway and a synthetic upstream. Every scenario is
reported; any failure blocks the aggregate. Package checks build a candidate from
the committed source and verify archives, corresponding source, notices and the
target SBOM. Candidate signing and approved release promotion use separate workflows.
The default workflow uses Rust 1.98.0, Python 3.14, Ubuntu 24.04 x64 and macOS 15
ARM64. Full history is fetched for publication checks. Actions are pinned by SHA.
Caches are partitioned by OS, architecture, toolchain and relevant lock files.

PRs run without provider secrets and with read-only repository permissions.
Do not execute untrusted PR code with write credentials or on a consumer host.
Keep runtime execution logs, private content and provider payloads out of public
artifacts. Tests use synthetic inputs and mock upstreams, even when exercising
the real pinned Codex executable.

## Release and dependency changes

Dependency and action updates arrive as separate Dependabot PRs. Review the
resolved lockfile and the required license evidence; an automated update is not
approval of new dependencies or licensing terms.

Candidate builds and final release are distinct. A protected release approval
promotes the already verified binary digest without rebuilding it. Verify source,
notices and provenance before adoption, and retain the preceding verified binary,
configuration and compatible state. See [release procedures](release.md).

Consumer integration work belongs in the consumer's repository. Public issues
may track the generic contract but must not link to private implementation or
operational records. A merged gateway PR does not close consumer acceptance.
