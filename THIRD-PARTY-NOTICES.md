<a id="제3자-구성요소와-출처"></a>

# Third-party components and provenance

[English](THIRD-PARTY-NOTICES.md) | [한국어](THIRD-PARTY-NOTICES.ko.md)

New project source is AGPL-3.0-only. Rust dependencies have their own licenses;
the project license does not replace their original permissions or notices.

`Cargo.toml` records direct dependencies and `Cargo.lock` records resolved versions.
This document is a guide to the evidence. The [dependency record](licensing/dependencies.json)
contains version-specific declarations and selections. The [generated notices](licensing/THIRD-PARTY-NOTICES.md)
link each package to its originals. The [license guide](licensing/README.md) covers
verification and updates.

Original notice bytes and hashes are preserved under `licensing/texts/`. The
inventory includes dependencies for every platform, build and development scope;
it does not claim all entries are linked into the final binary. The project's
commercial agreement does not replace third-party notice or permission requirements.

Before distribution, inspect locked dependency metadata and the code actually
included in the target. Verify each package's required `LICENSE`, `COPYING`,
`NOTICE` and other originals. `license_audit.py check` compares records with the
sources; `bundle` creates a notice collection. An SPDX metadata identifier alone
does not establish that required copyright notices are present. System libraries,
containers and bundled executables need separate inspection under the
[release procedure](docs/release.md).

<a id="라이선스-본문-출처"></a>

## Source of the license text

`LICENSE` is the complete AGPLv3 text obtained from the GNU original at
<https://www.gnu.org/licenses/agpl-3.0.txt>. Do not edit its body. Cargo metadata
and the documentation specify the project's version selection as `AGPL-3.0-only`.
