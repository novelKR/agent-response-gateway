<a id="배포-준비와-버전-고정"></a>

# Distribution preparation and version pinning

[English](release.md) | [한국어](ko/release.md)

Public source and PR CI are available. G18 builds and verifies unpublished
candidates; G19 provides signed candidates and preview promotion of the same
bytes after user approval. Check the selected commit, run, attempt and provenance
in the public [candidate workflow](https://github.com/novelKR/agent-response-gateway/actions/workflows/release-candidate.yml).
A signed candidate does not establish a release of current main or public binary
distribution. Consumer production acceptance and commercial agreements are also
separate stages. The [packaging contract](packaging.md) defines candidate contents
and verification. The [signing/promotion contract](release-promotion.md) defines
least privilege, environment approval, verification and recovery.

<a id="검증할-단위"></a>

## Unit of verification

Record the source commit, Cargo lockfile, Rust 1.98.0, target platform and binary
hash together. Basic checks are formatting, Clippy and mock-provider tests.
Gateway release testing differs from agent/backend consumer acceptance. Tests
requiring model API calls are separate from default PR CI.

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
python3 -B -m unittest discover -s scripts/tests -v
python3 -B scripts/license_audit.py check
cargo build --release --locked
```

Use Python 3.11 or later and prepare the pinned license tool and exact sources
under the [license guide](../licensing/README.md).

Linux/macOS workflows use publishable fixtures without provider secrets. Do not
report hosted success before the actual run. The checkout action is pinned to
`11d5960a326750d5838078e36cf38b85af677262`, the v4 tag commit verified on 2026-09-08.

<a id="소스와-고지"></a>

## Source and notices

Review and include the source and scripts needed to build, install and modify the
actual distributed version as AGPL Corresponding Source, according to the
[official AGPLv3](https://www.gnu.org/licenses/agpl-3.0.html) and the distribution's
actual form. Publishing only a binary or linking only the latest branch is not
assumed sufficient.

Once the HTTPS source location is established, set `source_url` to a verified
location for that version. Check that the `GET /` link actually gives network
users the required source access. A displayed link or `source_status` does not
judge license compliance.

Generate dependency notices from the committed [package records and originals](../licensing/README.md).
Retain third-party notices in both public and separately contracted distributions.

```sh
mkdir -p .local/release
python3 -B scripts/license_audit.py check
python3 -B scripts/license_audit.py bundle --output .local/release/licenses
python3 -B scripts/archive_notices.py .local/release/licenses .local/release/license-notices.tar
python3 -B scripts/check_public_boundary.py --archive .local/release/license-notices.tar
```

Output must use a new or empty directory. The bundle manifest binds the lockfile,
policy, package records and notice hashes. Generate into different empty paths
and compare identical-input bundles. Include the evidence alongside the public
source archive. The dedicated archive tool removes host ownership, timestamps
and extension metadata without overwriting existing files. Preserve original
notice bytes.

This list conservatively covers all of Cargo.lock. Do not present it as an SBOM
proving only the components linked into the binary. Inspect static/dynamic system
libraries, containers, bundled executables and their dependencies for each actual
distribution. Do not automatically mark that inspection or a full legal review
as complete.

Open external code contributions only after establishing the rights holder,
contribution terms and alternative-contract permissions. Preserve existing
third-party conditions in commercial agreements.

The current `0.1.0` is an unreleased development version. Target package-smoke CI
and candidates are not formal distribution approval or live-provider qualification.
The [version-specific license policy](../COMMERCIAL-LICENSING.md) records public,
alternative and unresolved terms. Passing checks, executed agreements, established
rights and commercial distribution clearance are distinct.

<a id="소비자의-채택과-복구"></a>

## Consumer adoption and recovery

Consumers select and verify a specific release/hash before startup. Do not fetch
the latest branch or an unverified binary automatically at runtime. Use readiness
JSON and HTTP contracts without moving consumer-owned Codex/tenant state into
the gateway.

Agent hosts select verified runtime/gateway combinations. Backend services adopt
within their existing workflow, external-call and deployment boundaries. Operational
switches require consumer approval and retain a recoverable previous binary and
configuration combination.

<a id="공개-소스-배포물"></a>

## Public source artifacts

Do not archive the whole local working directory. Build source archives from the
tracked files of a reviewed public commit and apply [documentation checks](documentation.md)
for private paths and unsupported links/object types. Inspect Cargo contents with
`cargo package --list --allow-dirty`. Neither `.gitignore` nor Cargo exclusion rules
replace verification of the actual distribution contents.
