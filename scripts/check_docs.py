#!/usr/bin/env python3
"""Check paired documentation, reviewed bytes, technical literals and local links.

`record --id NAME` records an editor's reviewed pair after structural checks.
It does not translate text or prove semantic equivalence. CI only runs `check`.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import posixpath
import re
import sys
import unicodedata
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = 'docs/translations.json'
RESERVED = {'.git', '.private', '.codex', '.local', 'node_modules', 'target'}
FIELDS = {'id', 'section', 'order', 'route', 'source', 'translation', 'anchors', 'source_sha256', 'translation_sha256'}


class DocumentationError(ValueError):
    pass


def require(ok, message):
    if not ok:
        raise DocumentationError(message)


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def regular_file(root: Path, relative: str) -> Path:
    require(isinstance(relative, str), 'Invalid document path')
    parts = PurePosixPath(relative).parts
    require(parts and not relative.startswith('/') and '\\' not in relative
            and not any(p in RESERVED or p in {'.', '..'} for p in parts), 'Unsafe document path')
    path = root
    for part in parts:
        path /= part
        require(not path.is_symlink(), 'Document paths cannot contain symlinks')
    require(path.is_file(), 'Document source or linked file is missing')
    return path


def source_path(root: Path, relative: str) -> Path:
    path = regular_file(root, relative)
    require(path.suffix == '.md', 'Document sources must be Markdown')
    return path


def slug(text: str) -> str:
    text = re.sub(r'\[([^]]+)\]\([^)]*\)', r'\1', text)
    text = re.sub(r'<[^>]+>', '', text).replace('`', '').lower()
    text = ''.join(c for c in text if c in '-_' or not unicodedata.category(c).startswith(('P', 'S')))
    return re.sub(r'\s', '-', text)


def parts(text):
    """Separate fences so examples are never interpreted as links or headings."""
    blocks, prose, block = [], [], []
    marker = None
    for line in text.splitlines(keepends=True):
        fence = re.match(r'^\s{0,3}(`{3,}|~{3,})(.*)$', line.rstrip('\n'))
        if marker is None and fence:
            marker = fence[1]
            block = [line]
        elif marker is not None:
            block.append(line)
            if re.match(r'^\s{0,3}' + re.escape(marker[0]) + '{' + str(len(marker)) + r',}\s*$', line):
                blocks.append(''.join(block))
                marker = None
        else:
            prose.append(line)
    require(marker is None, 'Unclosed Markdown fence')
    return blocks, ''.join(prose)


def headings(text):
    _, prose = parts(text)
    result, used = [], set()
    for match in re.finditer(r'^(#{1,6})\s+(.+?)\s*#*$', prose, re.M):
        name = slug(match[2])
        base, number = name, 0
        while name in used:
            number += 1
            name = f'{base}-{number}'
        used.add(name)
        result.append((len(match[1]), match[2], name))
    return result


def anchors(text):
    _, prose = parts(text)
    return {item[2] for item in headings(text)} | set(re.findall(r'<a\s+id="([^"]+)"\s*>', prose))


def literals(text):
    _, prose = parts(text)
    inline = set(re.findall(r'(?<!`)`([^`\n]+)`(?!`)', prose))
    fields = set(re.findall(r'(?<![A-Za-z0-9_])[a-zA-Z][a-zA-Z0-9]*_[a-zA-Z0-9_]+(?![A-Za-z0-9_])', prose))
    return inline, fields


def table_shape(text):
    _, prose = parts(text)
    return [len(re.split(r'(?<!\\)\|', line.strip())) for line in prose.splitlines()
            if line.lstrip().startswith('|')]


def link_targets(text):
    _, prose = parts(text)
    # Inline examples are not document navigation.
    prose = re.sub(r'`+[^`\n]*`+', '', prose)
    return re.findall(r'\[[^]\n]*\]\(([^\s)]+)(?:\s+"[^"]*")?\)', prose)


def local_links(root, relative, text):
    for target in link_targets(text):
        parsed = urlsplit(target)
        if parsed.scheme or parsed.netloc:
            require(parsed.scheme in {'https', 'http', 'mailto'}, 'Unsupported document link scheme')
            continue
        raw_path = unquote(parsed.path)
        require(not raw_path.startswith('/'), 'Source Markdown must use relative file links')
        rel = posixpath.normpath(str(PurePosixPath(relative).parent / raw_path)) if raw_path else relative
        candidate = regular_file(root, rel)
        if parsed.fragment and candidate.suffix == '.md':
            require(unquote(parsed.fragment) in anchors(candidate.read_text(encoding='utf-8')),
                    f'Missing heading target in {relative}')


def inventory(root):
    sources = {str(p.relative_to(root)) for p in root.glob('*.md') if p.name != 'AGENTS.md'}
    sources |= {str(p.relative_to(root)) for p in (root / 'docs').glob('*.md')}
    for name in ('licensing/README.md', 'tests/codex/README.md'):
        if (root / name).exists():
            sources.add(name)
    return {p for p in sources if not p.endswith('.ko.md')}


def load(root):
    path = root / MANIFEST
    require(not path.is_symlink(), 'Translation manifest cannot be a symlink')
    document = json.loads(path.read_text(encoding='utf-8'))
    require(set(document) == {'schema', 'review_method', 'documents'}
            and document['schema'] == 'gateway-documentation/v1'
            and document['review_method'] == 'paired-editorial-review; hashes detect drift, not semantic equivalence'
            and isinstance(document['documents'], list), 'Invalid translation manifest')
    seen_ids, seen_paths, seen_routes = set(), set(), set()
    for entry in document['documents']:
        require(set(entry) == FIELDS, 'Invalid document entry')
        require(re.fullmatch(r'[a-z][a-z0-9-]*', entry['id']) is not None
                and entry['id'] not in seen_ids, 'Duplicate or invalid document ID')
        require(re.fullmatch(r'[a-z][a-z0-9-]*', entry['section']) is not None
                and type(entry['order']) is int and entry['order'] >= 0,
                'Invalid navigation metadata')
        require(isinstance(entry['route'], str)
                and (entry['route'] == '/' or re.fullmatch(r'/guide/[a-z][a-z0-9-]*', entry['route']))
                and entry['route'] not in seen_routes, 'Duplicate or invalid site route')
        seen_routes.add(entry['route'])
        require(isinstance(entry['anchors'], list) and bool(entry['anchors'])
                and all(isinstance(a, str) and bool(a) for a in entry['anchors'])
                and len(set(entry['anchors'])) == len(entry['anchors']), 'Invalid preserved anchors')
        for name in ('source', 'translation'):
            require(entry[name] not in seen_paths, 'Duplicate document path')
            source_path(root, entry[name])
            seen_paths.add(entry[name])
        seen_ids.add(entry['id'])
    require({e['source'] for e in document['documents']} == inventory(root), 'Maintained document inventory differs')
    known_translations = {str(p.relative_to(root)) for p in root.glob('*.ko.md')}
    known_translations |= {str(p.relative_to(root)) for p in (root / 'docs/ko').glob('*.md')}
    for path in ('licensing/README.ko.md', 'tests/codex/README.ko.md'):
        if (root / path).exists():
            known_translations.add(path)
    require({e['translation'] for e in document['documents']} == known_translations, 'Translation inventory differs')
    return document


def check_pair(root, entry, *, hashes=True):
    source = source_path(root, entry['source']).read_bytes()
    translation = source_path(root, entry['translation']).read_bytes()
    en, ko = source.decode('utf-8'), translation.decode('utf-8')
    require(source.endswith(b'\n') and translation.endswith(b'\n'), 'Documents need terminal newlines')
    require(parts(en)[0] == parts(ko)[0], f'Code blocks differ: {entry["id"]}')
    require([h[0] for h in headings(en)] == [h[0] for h in headings(ko)], f'Sections differ: {entry["id"]}')
    require(literals(en) == literals(ko), f'Technical literals differ: {entry["id"]}')
    require(table_shape(en) == table_shape(ko), f'Table structure differs: {entry["id"]}')
    require(anchors(en) == anchors(ko), f'Language anchors differ: {entry["id"]}')
    require(set(entry['anchors']) <= anchors(en), f'Preserved anchors are missing: {entry["id"]}')
    for relative, text, other in ((entry['source'], en, entry['translation']), (entry['translation'], ko, entry['source'])):
        local_links(root, relative, text)
        require(any(not urlsplit(link).scheme and ((root / relative).parent / unquote(urlsplit(link).path)).resolve()
                    == (root / other).resolve() for link in link_targets(text)), 'Missing reciprocal language link')
    if hashes:
        require(entry['source_sha256'] == digest(source) and entry['translation_sha256'] == digest(translation),
                f'Translation review is missing or stale: {entry["id"]}')


def check(root):
    document = load(root)
    for entry in document['documents']:
        check_pair(root, entry)
    return len(document['documents'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=('check', 'record'), nargs='?', default='check')
    parser.add_argument('--root', type=Path, default=ROOT)
    parser.add_argument('--id', action='append', default=[])
    args = parser.parse_args()
    try:
        root = args.root.resolve()
        if args.command == 'record':
            require(bool(args.id), 'Select explicitly reviewed document IDs with --id')
            document = load(root)
            require(set(args.id) <= {e['id'] for e in document['documents']}, 'Unknown document ID')
            for entry in document['documents']:
                if entry['id'] in args.id:
                    check_pair(root, entry, hashes=False)
                    entry['source_sha256'] = digest(source_path(root, entry['source']).read_bytes())
                    entry['translation_sha256'] = digest(source_path(root, entry['translation']).read_bytes())
                    entry['anchors'] = sorted(anchors(source_path(root, entry['source']).read_text(encoding='utf-8')))
            (root / MANIFEST).write_text(json.dumps(document, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
        count = check(root)
        print(f'Documentation check passed: reviewed_pairs={count}')
        return 0
    except (OSError, UnicodeError, ValueError, KeyError, TypeError) as error:
        message = str(error) if isinstance(error, DocumentationError) else 'Invalid documentation input'
        print('Documentation check failed: ' + message, file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
