<a id="signed-candidates-and-protected-preview-promotion"></a>
<a id="서명-후보와-보호된-preview-승격"></a>
<a id="서명-후보의-미리보기-배포"></a>
<a id="signed-candidates-and-preview-releases"></a>
<a id="태그-빌드와-릴리스-승급"></a>

# Tagged builds and release promotion

[English](release-promotion.md) | [한국어](ko/release-promotion.md)

Push an existing-source version tag to build, verify and sign all four native
[packages](packaging.md). A successful candidate automatically publishes a GitHub
Pre-release. A separate manual workflow waits for approval before promoting the
same tag, Release ID and download files to a formal Release.

<a id="빌드와-서명-경계"></a>

## Build and signing boundaries

The tag must exactly equal v followed by the Cargo.toml package version. Both
vX.Y.Z and prerelease versions such as vX.Y.Z-rc.1 are accepted. Tags with build
metadata are not supported. The source commit must be contained in main, and the
latest main push CI run for that exact commit must have succeeded. Main can move
forward later without requiring a rebuild; moving the version tag is rejected.

Release candidate starts on a version-tag push. To recover a failed candidate,
start a new manual run against the same existing tag after inspecting the failure.
The workflow rejects branch dispatches and rerun attempts. Candidate artifacts
are retained for 30 days; do not replace a published candidate with a new build.

All four native build jobs use Rust 1.98.0, locked dependencies and the prepared
license audit tool, without PR caches. Each distribution includes the binary
archive, corresponding source, notices, scoped SBOM, candidate manifest and
checksums. A target descriptor binds the distribution and inner candidate hashes,
source commit, version, target and Cargo.lock hash.

Build jobs have contents:read. Separate signing jobs have contents:read,
actions:read, id-token:write and attestations:write. They inspect the downloaded
files without executing them, then attest the distribution archive and descriptor.
The saved Sigstore bundle accompanies those exact files. Candidate jobs have no
release-write permission. GitHub CLI verifies the repository, signing workflow,
refs/tags source ref, source and signer commit, hosted runner, and exact SLSA run
ID and attempt. PR artifacts cannot satisfy this contract.

To verify a downloaded target with a trusted checkout, put only its distribution
archive, target.manifest.json and target.sigstore.jsonl in the selected directory.
The command requires GitHub CLI with attestation support and Python 3.11+.
Substitute the source commit and candidate run ID from the release manifest, and
select the intended target/tag explicitly; the Windows tag below is illustrative.

```sh
python3 -B scripts/release_provenance.py verify \
  --directory .local/downloaded-target \
  --commit SOURCE_COMMIT --target x86_64-pc-windows-msvc \
  --run-id CANDIDATE_RUN_ID --attempt 1 --tag v0.1.0
```

<a id="검토와-승격"></a>

## Review and promotion

Publish prerelease runs after a successful Release candidate. Its verification job
has read permissions and uses the trusted main workflow's verifier. It requires
all four signed targets from the same commit, version and run. The immutable
release-manifest.json records these bindings and all 12 target-file hashes.
The manifest is the thirteenth release asset. It contains no mutable release state.

Only the publish job has contents:write. It verifies the prepared files again,
creates a draft, uploads missing files without overwrite, checks the complete asset
set and GitHub digests, then publishes as prerelease with make_latest=false.
It executes no downloaded binaries or build scripts. No platform is omitted to
make a partial release succeed.

Promote release runs manually on main with release_tag. It accepts only a public
release whose tag has the stable vX.Y.Z form. It downloads the release files,
verifies their signatures and manifest, and shows the manifest before waiting for
the protected release environment. After approval, it downloads and verifies the
public files again and requires the same manifest digest. It changes only
prerelease=false and make_latest=legacy on the same Release. It never rebuilds,
renames the tag or replaces an asset. A tag ending in -rc.1 remains a prerelease.
For example, v0.1.0 initially publishes as Pre-release and can later be promoted
with the same v0.1.0 tag and files; this example does not announce a published version.

The release environment requires the designated repository owner and protected
branches. The helper checks that policy and actual approval history for this run
and environment; an environment-name variable alone is insufficient. The owner
may approve their own dispatch. This is release authorization, not independent
code review. Keep administrator bypass disabled and inspect that setting in
GitHub because the environment REST response does not expose it. Merging workflows
does not configure the environment. Approval changes the distribution channel;
real-provider qualification and consumer operational acceptance remain separate.

<a id="실패와-복구"></a>

## Failure and recovery

If publication fails, inspect the draft and start Publish prerelease manually on
main with the original candidate_run_id. The successful candidate is retained
separately, so publication recovery does not rebuild. A draft resumes only missing
files whose existing names and digests match. A matching complete release returns
success without another upload; an already promoted release remains formal.
Unexpected files, changed digests or conflicting tags stop recovery without
replacement or deletion. Per-tag publication and promotion jobs serialize writes.
There is no automatic retry of an uncertain write.

Formal promotion reads GitHub Release assets, so it does not depend on the
30-day Actions artifact retention. If an unpublished candidate has expired, the
original signed files must be recovered before publication; a new build cannot
stand in for a partially uploaded candidate. A failed formal promotion can be
started again with a fresh environment approval; the same completed state is
recognized without modifying files.

Disable Publish prerelease to pause automatic publication; withhold the release
environment approval to pause formal promotion. Retain candidate evidence and
published source commits. Consumer rollback selects an earlier verified executable
and configuration through the consumer's own adoption procedure.

GitHub's [artifact attestation verification](https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations/verifying-the-provenance-of-artifacts),
[environment protection](https://docs.github.com/en/actions/reference/workflows-and-actions/deployments-and-environments),
and [release API](https://docs.github.com/en/rest/releases/releases) describe the
platform mechanisms. The workflow, verifier and retained manifest define this
repository's release contract. Mock tests verify rejection and state transitions;
a real tag build, signed publication and approved promotion are separate execution
evidence, not consequences of merging the configuration.
