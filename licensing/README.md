<a id="라이선스-정책과-고지-기록"></a>

# License policy and notice records

[English](README.md) | [한국어](README.ko.md)

The project uses **AGPL-3.0-only and a separate commercial license** under the
[licensing policy](../COMMERCIAL-LICENSING.md). This guide explains how to verify
and package dependency licenses and original notices for either distribution.

<a id="관리-자료"></a>

## Managed evidence

| Record | Purpose |
|---|---|
| [policy.json](policy.json) | Public version policy, checker version, third-party allowlist and pinned supplemental originals |
| [dependencies.json](dependencies.json) | Declarations, selections, checksums and notice provenance for every Cargo.lock package |
| [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) | Package notice guide generated from the records |
| `texts/<sha256>.txt` | Actual notice originals with bytes and line endings preserved |

Content hashes deduplicate notice texts while preserving each package's version
and provenance. Review changes to policy, selections and originals together.
Keep existing originals needed to reproduce earlier notice bundles.

Do not apply Git line-ending normalization or whitespace reformatting to originals.
Verify their integrity with checksums instead of whitespace edits.

The inventory includes build, development and inactive conditional dependencies
on every platform. It does not claim every entry is linked into the final binary.
An original may contain other licensing options; read it together with the
package-specific selection record.

<a id="도구와-자료-준비"></a>

## Prepare tools and sources

Use Rust 1.98.0, Python 3.11 or later, and cargo-deny 0.20.2. Verify that `python3`
in these commands satisfies the version requirement. CI uses Python 3.14.
The checker is a development tool, not a product runtime dependency or distribution member.

```sh
python3 --version
export CARGO_HOME="$PWD/.local/cargo-home"
cargo install cargo-deny --version 0.20.2 --locked \
  --root .local/tools --target-dir target/license-tools
cargo fetch --locked
```

Only preparation uses the network. Checking, refreshing and bundling are offline.
If using another Cargo cache, use the same `CARGO_HOME` for preparation and checking.
Do not substitute a missing version or original with the latest release or a
generic MIT text.

<a id="검사갱신묶음-생성"></a>

## Check, refresh and bundle

```sh
python3 -B scripts/license_audit.py check
python3 -B scripts/license_audit.py refresh
python3 -B scripts/license_audit.py bundle --output .local/release/licenses
```

- `check` does not change tracked files. It creates temporary audit state under `.local/`.
- `refresh` produces a reviewable diff from verified originals and the allowed policy.
  Existing selections for unchanged originals remain; new entries use the policy's
  preference order, with MIT first. Disallowed terms or missing notices fail before
  writing. Success is not commercial permission or completed change review.
- `bundle` performs the same checks, then writes notices, originals, policy,
  dependency records and a hash manifest into a new or empty directory. It does
  not overwrite existing files. Use separate empty directories for comparison.

Mismatched lock/policy hashes, original SPDX expressions, selected terms, notice
provenance or hashes fail. Output contains no current timestamps or local absolute
paths, so identical inputs produce identical bundle bytes. Errors also omit local paths.

<a id="판정과-특수-원문"></a>

## Evaluation and special originals

cargo-deny evaluates SPDX expressions. Each package is allowed only its selected
permissions, which must satisfy the original expression. `OR` selects an option;
`AND` requires every applicable condition. There is no global AGPL allowance,
blanket OSI/FSF allowance or omission based on `publish = false`. Additional
`deny.exceptions.toml`-style files are not allowed.

The default cargo-deny graph excludes some inactive packages. The checker passes
a temporary **audit graph** rooted at every locked entry from offline Cargo
metadata and requires an evaluation for every entry. Original licenses, versions,
sources and dependency relationships are preserved; the actual build graph is
unchanged. Unpacked source read by the tool is also compared byte for byte with
its checksum-verified crate archive.

Provenance verification currently supports crates.io packages with checksums.
Git, path and other registries need separate provenance implementations. New
licenses or source channels are not implicitly allowed.

- `ring`: retain subordinate notices for BoringSSL and once_cell-derived portions.
- `matchit`: preserve both MIT and the BSD terms in `LICENSE.httprouter`.
- Unicode and certificate data: retain the applicable data licenses and notices.
- `r-efi`: preserve the `AUTHORS` original and record the MIT selection.
- `valuable 0.1.1`: supplement the missing crate LICENSE from the upstream commit
  identified by its VCS record. The policy pins the URL, commit, crate checksum
  and original-text hash; checks read only the original retained in Git.

Normal discovery collects original LICENSE, LICENCE, COPYING, NOTICE, COPYRIGHT,
AUTHORS and declared `license-file` records. If a special obligation is located
elsewhere, verify its provenance and add a supplemental record. Discovery is not
a legal guarantee of every copyright relationship or complete notices.

<a id="공개-경계와-배포"></a>

## Publication boundary and distribution

Public records contain public third-party provenance and reusable contracts only.
Keep consumer information, contracts and contribution consents in independent
private history. Public builds and CI must not require those records.

Include the notice bundle in distribution after the [publication check](../docs/documentation.md).
Inspect system libraries, container packages and bundled executables separately
for the actual artifact, beyond the Cargo list. See the [release guide](../docs/release.md).
