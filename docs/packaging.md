<a id="미공개-배포-후보"></a>

# Unpublished release candidates

[English](packaging.md) | [한국어](ko/packaging.md)

The packaging tool builds and verifies candidates for x86_64-unknown-linux-gnu
on Ubuntu 24.04 and aarch64-apple-darwin on macOS 15, using native Rust 1.98.0
and Python 3.14 in CI. It produces local candidate files without publishing a release.

The builder exports Git HEAD into an isolated source directory. Ignored,
untracked and modified checkout files cannot enter that source build. The locked
crate sources and pinned cargo-deny must already be prepared. Cargo runs offline
with an explicit release target and sanitized environment, rejects external Cargo
configuration overrides, and remaps source/cache/toolchain paths. Build artifacts
stay under target; temporary exports and private build logs stay under .local.
The verification command additionally checks that the packaging/smoke tool bytes
are present in the source archive with the same hashes. Commit tooling changes
before constructing a verified candidate.

```sh
python3 -B scripts/release_package.py build \
  --target aarch64-apple-darwin --output .local/candidate
python3 -B scripts/release_package.py verify .local/candidate \
  --commit VERIFIED_COMMIT_SHA --target aarch64-apple-darwin
```

The output directory must not exist. No existing candidate is overwritten.
Prepare the cache and cargo-deny as described in [licensing](../licensing/README.md).
Use x86_64-unknown-linux-gnu on the native Linux builder. These commands never
publish or create an attestation. A checksum establishes internal consistency;
[Signing and promotion](release-promotion.md) provides verified build provenance and release approval.

| Candidate file | Evidence |
|---|---|
| Target binary tar.gz | Executable, configuration examples, product/license documents, complete committed Cargo notice bundle and supplied Rust toolchain notices |
| Source tar.gz | Git's tracked source archive with commit receipt; normalized gzip metadata |
| Target cdx.json | CycloneDX 1.6 target build dependency inventory bound to the binary hash |
| candidate.json | Source commit, Cargo.lock hash, compiler identity, target, tool hashes, asset/member hashes and modes, package inclusion list, linkage and validation stages |
| SHA256SUMS | Every candidate asset and the candidate manifest, with exact filenames |

The binary archive uses regular files and reviewed 0644/0755 modes only. Names,
bytes, modes and hashes must match its manifest exactly; extra/missing files,
links, reserved paths, changed checksums and wrong source/target bindings reject.
Source and binary archives both run the existing public-boundary checker. Cargo's
package inclusion list is inspected separately. The source archive includes the
scripts required to rebuild the corresponding source.

<a id="목록-범위와-고지"></a>

## Inventory scope and notices

Cargo metadata is filtered to the target, then matched against actual successful
Cargo compiler-artifact records. Unbuilt platform/dev dependencies are excluded;
normal runtime source dependencies and build/procedural-macro inputs are
classified separately. Features come from the actual artifact records. Crate
hashes describe the locked crate archives, not compiled object bytes. Names,
versions, sources and selected license expressions come from the reviewed license
records. No filesystem paths from Cargo metadata are exported.

The SBOM also records a Rust standard-library aggregate and observed OS dynamic
libraries. Its composition is explicitly incomplete: build inputs are not an
exact linked-byte inventory, and the aggregate does not expand every standard-
library or OS component. Mach-O libraries/minimum macOS load commands and ELF
NEEDED/GLIBC symbol requirements are retained as observed platform requirements;
older OS compatibility is not inferred from a successful current-runner smoke.
System libraries remain external and are not redistributed by this package.

The package preserves the committed all-platform Cargo notice bundle, including
build/dev records. It also captures the installed Rust COPYRIGHT-library,
available copyright/license texts and target rlib hashes without editing their
bytes. These are supplied toolchain records; their presence is not a legal
clearance, proof of complete binary composition or permission under an alternative
license. A distributor reviews the exact toolchain/target and any additional
obligations before formal promotion. Local distro compiler descriptions and
upstream CI compiler descriptions are recorded distinctly. The gateway package
contains no Codex executable.

Primary inventory contracts are [Cargo metadata](https://doc.rust-lang.org/cargo/commands/cargo-metadata.html),
[CycloneDX 1.6](https://cyclonedx.org/docs/1.6/json/), and the Rust project's
[copyright inventory description](https://github.com/rust-lang/rust/blob/main/COPYRIGHT).

<a id="검증과-승격-경계"></a>

## Validation and promotion boundary

The native candidate executable runs manifest/readiness binding, unauthenticated
access rejection, all three JSON routes with synthetic upstreams, credential
header isolation, and bounded normal shutdown. No model provider is contacted.
Archive determinism tests cover identical input bytes; binary reproducibility
across machines/toolchain distributions is not claimed. The full existing
Rust/Python/license/publication and pinned-Codex suites remain required.

The PR package-smoke matrix builds and retains only verified public candidate
assets, not compiler logs or local state. Its result joins ci-required. A PR
artifact is for review and is never eligible for formal promotion. A release
candidate must come from a verified main commit with authenticated provenance.
The [promotion workflow](release-promotion.md) verifies that provenance and
publishes the exact retained bytes after approval. Test the intended application
and real model separately before operational use.
