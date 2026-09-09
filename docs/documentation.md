<a id="공개-문서와-로컬-기록-관리"></a>

# Public documentation and local records

[English](documentation.md) | [한국어](ko/documentation.md)

The public repository maintains reusable API, execution and distribution contracts.
Do not include consumer names, repository URLs, local paths, internal service
topology or operational records in public documentation, examples, commit messages,
tags or CI logs. Public builds and tests must work without separate local records.

<a id="독립적인-로컬-이력"></a>

## Independent local history

When local documents need to live in the same working directory, `.private/` can
contain an independent Git repository. The parent `.gitignore` excludes the entire
directory. Do not register it as a submodule or connect it with `.gitmodules`, a
gitlink or a public link. Parent commits and pushes do not include its history or
backups. The public project must also work when this directory does not exist.

Before generalizing content, retain its original path, bytes and hash privately.
Public documentation contains the reusable requirement or contract. Internal
records may reference public commits; public documents must not reference internal
identifiers. Exclusion rules do not configure access or backups. Manage the private
remote and retention policy separately.

<a id="공개-경계-검사"></a>

## Publication checks

The standard-library Python checker accepts regular files only. It rejects
reserved private/build paths, gitlinks, symlinks and `.gitmodules`. The default
check reads actual Git index blobs and commit history reachable from every local
ref. Removing a worktree file does not remove it from the index or history.

```sh
python3 -B scripts/check_public_boundary.py
python3 -B -m unittest discover -s scripts/tests -v
```

Before an initial commit or staging, use the following to include non-ignored
untracked files. The default check does not accept an empty index.

```sh
python3 -B scripts/check_public_boundary.py --worktree
```

An optional external JSON array of strings can check private identifiers. The
filename below is an example; never put its actual dictionary or private
identifiers in public source or CI.

```sh
python3 -B scripts/check_public_boundary.py --worktree --private-patterns .private/patterns.json
```

Dictionary checks match exact UTF-8 strings. They do not infer contextual
relationships or detect every variant, so review the final public diff as well.
Failure output includes neither matched content nor private paths. CI checks a
full-history checkout without requiring a private dictionary.

<a id="소스-압축파일"></a>

## Source archives

Create public artifacts from the tracked files of a reviewed public commit.
Do not archive the whole working directory. Run the following only after a
public HEAD has been committed.

```sh
mkdir -p .local/release
git archive --format=tar.gz --prefix=agent-response-gateway/ --output=.local/release/source.tar.gz HEAD
python3 -B scripts/check_public_boundary.py --archive .local/release/source.tar.gz
```

Archive checks also reject reserved paths, links and special objects. An optional
dictionary applies to member contents as well. The checker does not extract the
archive. Passing this check does not prove corresponding-source completeness or
binary reproducibility; follow the [release procedure](release.md).

The checker does not modify Git objects, the index or refs. Inputs over its bounds
fail. Fetch full history before checking a shallow clone.

<a id="라이선스-자료"></a>

## License evidence

Policy, package records and original notices under `licensing/` are versioned in
public Git. Each original has public provenance and a hash. Do not copy local
cache paths or private contracting records into them. The [dedicated audit](../licensing/README.md)
checks originals and selections; the publication checker checks what may enter
files, history and archives. Neither replaces the other.

Create a tar for `license_audit.py bundle` output with `scripts/archive_notices.py`
to remove host metadata, then apply the archive check above. License checking and
notice generation must work without `.private/` in the public source.

<a id="ref와-로컬-설정의-검사-범위"></a>

## Ref and local-configuration scope

Check all history reachable from commit refs and detached HEAD. For tree refs,
apply the same path, regular-file and content rules. Verify nested annotated tags
and their target object types before checking the final commit or tree. Refs or
tags that point directly to blobs are unsupported and fail explicitly. Do not
ignore a tree ref or delete refs to obtain a passing result.

`.codex/` is reserved for personal execution configuration. Its presence in the
index, history, a tree ref or an archive is rejected. A valid tree object does
not make its contents publishable. Local snapshots containing private configuration
can keep the full local check failing; distinguish that outcome from checks of
the actual refs and artifacts intended for publication.

<a id="번역-관리"></a>

## Translation maintenance

English files are the editorial source. Root guides use corresponding `.ko.md`
files; guides under `docs/` use `docs/ko/`. Legal originals and generated notices
remain canonical evidence, and `AGENTS.md` has one English instruction source.
Keep reciprocal language links and the explicit compatibility anchors when
editing a heading. Commands, code blocks, identifiers and support conditions must
remain consistent across each pair.

The [document registry](translations.json) records stable IDs, navigation groups,
preserved anchors, source/translation paths and both reviewed file hashes. The hashes detect drift;
they do not prove semantic equivalence or human approval. Review both complete
documents before recording the pair. Missing or stale translations fail the
checker explicitly; CI never updates the review records automatically.

```sh
python3.14 -B scripts/check_docs.py
python3.14 -B scripts/check_docs.py record --id documentation
```

The second command is for an editor after reviewing that document pair; select
each reviewed ID explicitly. A new maintained Markdown document requires its
Korean edition and registry entry. The checker validates inventory, sections,
technical literals, code blocks, table structure, shared anchors and relative
links. Its synthetic regressions exercise missing/stale translations and broken
contracts; it does not replace editorial review.

<a id="문서-사이트-개발"></a>

## Documentation site development

The site uses VitePress 1.6.4, Node 24.21.0 and npm 11.19.0 with the committed
[npm lock](../docs-site/package-lock.json). Use Python 3.11+; CI selects Python
3.14. Set the DOCS_PYTHON environment variable if the Python executable has a
different name. From the repository root:

```sh
npm ci --prefix docs-site --ignore-scripts
npm test --prefix docs-site
npm run build --prefix docs-site
python3 -B docs-site/scripts/check-output.py
npm run preview --prefix docs-site
```

The preview listens at `http://127.0.0.1:43140/agent-response-gateway/`. Rebuild
and reload after edits; the preview has no hot module replacement. The project
base is `/agent-response-gateway/`, with English at the root and Korean under
`/ko/`. Maintained pages live under `/guide/` and `/ko/guide/`. The toolbar links
to the same document in the other language and retains compatibility anchors.
Local search keeps queries in the browser. Search indexes contain only selected
public document content; no provider, analytics or remote search request is needed.

The document registry owns stable IDs, source pairs, groups, order and site routes.
[Navigation metadata](../docs-site/navigation.json) supplies localized group labels,
locale prefixes and the API route illustration. Page titles and card summaries
come from the Markdown heading and first prose paragraph. Add a paired document
and registry route, then its group label if needed; existing routes remain stable.
Both languages share the same theme and components. Adding another language also
requires extending the editorial checker and translated UI labels explicitly.

Edit [central tokens](../docs-site/theme/tokens.css) for color, type, spacing and
width changes. Components consume semantic tokens; VitePress variables map to
the same values. Responsive rules extend the official theme at its existing
breakpoints. The site uses system fonts and locally bundled theme icons, with
original SVG connectors. Content slots and typed props keep page text separate
from layout. Code, sidebar, outline, theme selection and search use VitePress's
official extension points.

The input generator copies only registry-selected Markdown to an ignored build
directory. It rejects page scripts, page styles and transitive include directives.
Unpublished source links point to the public source commit instead of copying
the checkout. Output checks verify the complete page inventory, local links,
anchors, resource origins and generated assets. The build manifest records each
published file's SHA-256 and whether it came from a dirty working tree. It is
an integrity record, not an attestation. Installation directories, caches and
generated pages are excluded from Git and source packages.

<a id="웹-고지와-개발-서버-제약"></a>

## Web notices and development-server constraints

Web dependencies are development dependencies of the static site, separate from
the Rust gateway and its license audit. The build reads Rollup's client module
inventory, matches package versions and integrity records to the npm lock, and
preserves original notice bytes. The [reviewed records](../docs-site/licensing/dependencies.json)
also include Lucide/Feather icons embedded in VitePress. Their source file and
license hashes are pinned separately. The DocSearch CSS tarball omits its license;
its original MIT notice is retained from the exact official source tag's commit.
No font files or social-icon assets are shipped.

The generated `web-dependencies.json`, `web-notices.txt` and the unchanged project
license in `LICENSE.txt` accompany the site.
After reviewing changed packages, their license terms, original notices and any
embedded assets, record the revised inventory explicitly:

```sh
npm run build --prefix docs-site -- --record-notices
git diff -- docs-site/licensing
npm run build --prefix docs-site
```

Recording hashes does not approve commercial rights. Ordinary builds and CI fail
on stale records. An update to VitePress also requires reviewing its bundled
theme assets; npm package boundaries alone do not describe every copied asset.

The selected stable release resolves Vite 5.4.21 and esbuild 0.21.5. The npm audit
reports three affected package entries (two moderate, one high), covering four
development-server advisories: [esbuild CORS](https://github.com/advisories/GHSA-67mh-4wv8-2f99),
[optimized-dependency paths](https://github.com/advisories/GHSA-4w7w-66w2-5vf9),
[Windows filesystem paths](https://github.com/advisories/GHSA-fx2h-pf6j-xcff) and
[Windows editor launch](https://github.com/advisories/GHSA-v6wh-96g9-6wx3).
These findings are not fixed by this site implementation. The supported commands
perform production builds and use a separate static HTTP preview bound to
loopback. They never start Vite's development server, esbuild's server or an
editor-launch endpoint. Do not substitute `vitepress dev` or the built-in preview
for this workflow. Dependency upgrades require a fresh compatibility and notice
review; do not force an incompatible Vite major through an npm override.

<a id="사이트-검증과-공개"></a>

## Site verification and publication

Before review, check both languages at desktop, tablet and 320-pixel mobile
widths; inspect long titles, tables, keyboard focus, theme changes and direct
page reloads. Search for `authentication`, `인증`, `previous_response_id` and
`압축`. Confirm same-page language switching, preserved anchors and the 404 page.
Test a temporary palette and spacing change across navigation, cards, badges,
tables, code and diagrams, then restore the tokens. Synthetic tests also exercise
new groups and translations, forbidden output and preview traversal.

The read-only `docs` CI job is part of `ci-required` and retains the verified
static artifact for review. Pages activation, deployment permissions and the
protected deployment environment require a separate publication decision.
Supply the source commit, build manifest and web notices for that decision.
Deploy the approved artifact without rebuilding it and verify the actual URL.
Restore an earlier verified artifact for a site rollback; use a reverting PR
for source changes. A successful local build or retained artifact is not a live
site, a formal gateway release or consumer operational acceptance.
