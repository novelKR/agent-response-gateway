#!/usr/bin/env python3
"""Generate site inputs from reviewed public documents, never from a directory copy."""
from __future__ import annotations

import json
from pathlib import Path
import posixpath
import re
import shutil
import subprocess
import sys
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts'))
import check_docs as docs


def plain(text):
    text = re.sub(r'\[([^]]+)\]\([^)]*\)', r'\1', text)
    return re.sub(r'\s+', ' ', re.sub(r'<[^>]+>|[*`#]', '', text)).strip()


def description(text):
    _, prose = docs.parts(text)
    for paragraph in re.split(r'\n\s*\n', prose):
        if not paragraph.strip() or paragraph.lstrip().startswith(('#', '<', '[English]', '|')):
            continue
        return plain(paragraph)
    raise docs.DocumentationError('Document summary is missing')


def catalogue(root, manifest, navigation):
    groups = {group['id'] for group in navigation['groups']}
    result = []
    for entry in manifest['documents']:
        docs.require(entry['section'] in groups, 'Site navigation group is missing')
        for locale, settings in navigation['locales'].items():
            source = entry[settings['sourceField']]
            text = docs.source_path(root, source).read_text(encoding='utf-8')
            result.append(dict(id=entry['id'], locale=locale, source=source,
                section=entry['section'], order=entry['order'],
                route=settings['prefix'] + entry['route'],
                title=plain(docs.headings(text)[0][1]), description=description(text)))
    return result


def rewrite(text, source, routes, repository, commit):
    _, prose = docs.parts(text)
    docs.require(not re.search(r'<!--\s*@include:|<script\b|<style\b', prose, re.I),
                 'Source includes and page scripts/styles are outside the publication list')
    def link(match):
        target = match[2]
        parsed = urlsplit(target)
        if parsed.scheme or parsed.netloc or not parsed.path:
            return match[0]
        relative = posixpath.normpath(posixpath.join(posixpath.dirname(source), unquote(parsed.path)))
        suffix = ('?' + parsed.query if parsed.query else '') + ('#' + parsed.fragment if parsed.fragment else '')
        # VitePress validates root-relative routes and adds its configured base.
        url = routes[relative] if relative in routes else f'{repository}/blob/{commit}/{relative}'
        return f'{match[1]}({url}{suffix}{match[3] or ""})'
    output, marker = [], None
    for line in text.splitlines(keepends=True):
        fence = re.match(r'^\s{0,3}(`{3,}|~{3,})', line)
        if fence:
            if marker is None:
                marker = fence[1]
            elif re.match(r'^\s{0,3}' + re.escape(marker[0]) + '{' + str(len(marker)) + r',}\s*$', line):
                marker = None
            output.append(line)
        elif marker is None:
            if line.startswith('[English]'):
                continue  # The generated page toolbar retains the same-page language pair.
            segments = re.split(r'(`+[^`\n]*`+)', line)
            output.append(''.join(segment if index % 2 else
                re.sub(r'(\[[^]\n]*\])\(([^\s)]+)(\s+"[^"]*")?\)', link, segment)
                for index, segment in enumerate(segments)))
        else:
            output.append(line)
    return ''.join(output)


def prepare(root=ROOT):
    docs.check(root)
    manifest = docs.load(root)
    navigation = json.loads((root / 'docs-site/navigation.json').read_text())
    pages = catalogue(root, manifest, navigation)
    routes = {page['source']: page['route'] for page in pages}
    commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
    dirty = bool(subprocess.check_output(['git', 'status', '--porcelain', '--untracked-files=normal'], cwd=root, text=True))
    build = root / '.local/docs-site'
    source_dir = build / 'source'
    if source_dir.exists():
        shutil.rmtree(source_dir)
    source_dir.mkdir(parents=True)
    for page in pages:
        relative = page['route'].lstrip('/')
        relative = relative + 'index' if page['route'].endswith('/') else relative
        path = source_dir / (relative + '.md')
        path.parent.mkdir(parents=True, exist_ok=True)
        text = docs.source_path(root, page['source']).read_text(encoding='utf-8')
        text = rewrite(text, page['source'], routes, navigation['repository'], commit)
        frontmatter = {'title': page['title'], 'description': page['description'],
            'docId': page['id'], 'sourcePath': page['source'], 'docLocale': page['locale']}
        if page['id'] == 'overview':
            frontmatter.update(layout='page', pageClass='product-page')
        path.write_text('---\n' + '\n'.join(f'{k}: {json.dumps(v, ensure_ascii=False)}' for k, v in frontmatter.items())
            + '\n---\n\n' + text, encoding='utf-8')
    for locale, settings in navigation['locales'].items():
        for group in navigation['groups']:
            path = source_dir / (settings['prefix'].strip('/') + '/guide/sections/' + group['id'] + '.md').lstrip('/')
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(f'---\ndocLocale: {locale}\ngroupId: {group["id"]}\n---\n\n# {group[locale]}\n\n<CardGrid section="{group["id"]}" />\n', encoding='utf-8')
    info = {'commit': commit, 'dirty': dirty, 'pages': pages, 'navigation': navigation}
    (build / 'catalogue.json').write_text(json.dumps(info, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(f'Prepared {len(pages)} reviewed pages and {len(navigation["groups"]) * len(navigation["locales"])} group hubs')


if __name__ == '__main__':
    prepare()
