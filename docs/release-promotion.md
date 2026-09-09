<a id="signed-candidates-and-protected-preview-promotion"></a>
<a id="서명-후보와-보호된-preview-승격"></a>
<a id="서명-후보의-미리보기-배포"></a>

# Signed candidates and preview releases

[English](release-promotion.md) | [한국어](ko/release-promotion.md)

Two manual workflows build candidates and publish verified previews. The candidate
workflow builds and signs a selected main commit after its push CI succeeds.
The promotion workflow verifies the retained files and waits for approval in the
protected release environment before publishing the same bytes as a prerelease.


<a id="빌드와-서명-경계"></a>

## Build and signing boundaries

Release candidate accepts a full expected_commit SHA and runs only on main in
this repository. The gate requires that SHA to equal the workflow source and the
latest push CI for that commit to have succeeded. A failed candidate needs a new
dispatch; rerunning an existing run is rejected. Artifact names are immutable
within a run and retained for 30 days.

Separate native Ubuntu 24.04 and macOS 15 jobs run the [candidate builder](packaging.md)
without PR caches. Each distribution archive contains the binary bundle,
corresponding source, notices, scoped SBOM, candidate manifest and checksums.
A target descriptor binds the distribution SHA-256, inner candidate SHA-256,
source commit, version, target and Cargo.lock hash.

Build jobs have contents:read. Separate signing jobs have contents:read,
actions:read, id-token:write and attestations:write. They inspect the downloaded
distribution without executing its binary or build scripts, then use the pinned
actions/attest action to attest both the archive and descriptor. The saved
Sigstore bundle accompanies those exact files. No release-write permission is
available to candidate jobs.

Verification uses GitHub CLI's authenticated attestation verifier with the
expected repository, signing workflow, main source ref, source and signer commit
digests, and hosted runners only. It additionally binds the verified SLSA
invocation to the exact candidate run ID and attempt. A checksum-only or PR
candidate cannot pass this contract. The local synthetic tests do not establish
that an actual signature was generated or verified; a successful signed workflow
and independent verification of its downloaded bytes provide that evidence.


<a id="검토와-승격"></a>

## Review and promotion

Preview promotion accepts candidate_run_id, expected_commit and release_tag.
The tag must match vVERSION-preview.N for the candidate's package version. The
operational track is an explicit error until a separately verified consumer
acceptance contract exists.

The first job has only contents:read and actions:read. It requires a successful
main candidate workflow, verifies both signed targets and emits promotion.json
with all six asset hashes to the job summary and an immutable workflow artifact.
Review that receipt and the candidate's inventory/qualification limits before
approving the publish job. The publish job alone has contents:write, runs through
the release environment and downloads the verified receipt and original assets.
It repeats provenance and byte verification after approval. It never rebuilds.

The release environment requires the designated repository owner, protected
branches only, and disabled administrator bypass. The sole owner may review
their own manual dispatch; this is explicit release authorization, not an
independent code review. The helper checks the configured reviewer/branch policy
and GitHub's actual approval history for that environment and promotion run.
An environment-name variable alone is insufficient. Administrator bypass must
also be inspected in repository settings because the environment REST response
does not expose that setting. Configure the environment before dispatching a
promotion; merely merging these workflows does not configure it.

The candidate source must still equal current main when publishing starts.
Create a fresh candidate if main has moved. Publication creates a draft, uploads
only missing matching assets without overwrite, verifies GitHub's asset digests,
publishes as prerelease with make_latest=false, then reads back the release,
complete asset set and exact tag commit. An existing tag or release with a
different source, metadata or asset digest is rejected.


<a id="실패와-복구"></a>

## Failure and recovery

A lost upload acknowledgement leaves a draft and an unknown workflow outcome.
There is no automatic retry. Inspect the draft and start a new promotion dispatch
with fresh environment approval; an exact matching draft can resume only its
missing assets. A matching complete release is recognized without uploading
again. Unexpected assets, changed bytes or conflicting tags stop recovery and
are never overwritten or deleted automatically. A failure after publication
requires release/tag readback before deciding the next action.

To pause publication, disable the manual promotion workflow or withhold the
release environment approval. Retain existing candidates and source commits for
investigation; do not rewrite a published version. Consumer rollback selects a
previous verified executable/configuration combination through its own adoption
process. Gateway automation does not restart or migrate a consumer.

GitHub's [artifact attestation verification](https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations/verifying-the-provenance-of-artifacts),
[deployment environment protection](https://docs.github.com/en/actions/reference/workflows-and-actions/deployments-and-environments),
and [release API](https://docs.github.com/en/rest/releases/releases) describe the
platform mechanisms. The exact workflow, helper and retained receipts define
this repository's promotion contract.
