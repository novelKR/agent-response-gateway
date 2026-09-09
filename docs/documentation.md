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
