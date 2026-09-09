#!/usr/bin/env python3
"""Check the exact HTML inventory, generated assets and local navigation in a site."""
from __future__ import annotations

import argparse
import hashlib
from html.parser import HTMLParser
import json
from pathlib import Path
import re
import sys
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[2]


class Page(HTMLParser):
    def __init__(self, text):
        super().__init__(convert_charrefs=True)
        self.links, self.ids, self.resources = [], set(), []
        self.feed(text)

    def handle_starttag(self, tag, attributes):
        attributes = dict(attributes)
        if 'id' in attributes:
            self.ids.add(attributes['id'])
        if tag == 'a' and 'href' in attributes:
            self.links.append(attributes['href'])
        if tag in {'script', 'img', 'source', 'iframe'} and 'src' in attributes:
            self.resources.append(attributes['src'])
        if tag == 'link' and set(attributes.get('rel', '').split()) & {'stylesheet', 'modulepreload', 'preload', 'icon'}:
            self.resources.append(attributes.get('href', ''))


def require(ok, message):
    if not ok:
        raise ValueError(message)


def html_path(route):
    return route.lstrip('/') + ('index.html' if route.endswith('/') else '.html')


def check(directory, catalogue):
    navigation = catalogue['navigation']
    base = navigation['base']
    expected = {html_path(page['route']) for page in catalogue['pages']} | {'404.html'}
    for locale in navigation['locales'].values():
        expected |= {html_path(locale['prefix'] + '/guide/sections/' + group['id']) for group in navigation['groups']}
    allowed = expected | {'hashmap.json', 'vp-icons.css', 'LICENSE.txt', 'web-notices.txt', 'web-dependencies.json', 'build-manifest.json'}
    files, pages, digests = set(), {}, {}
    for file in directory.rglob('*'):
        require(not file.is_symlink(), 'Site output cannot contain symlinks')
        if file.is_dir():
            continue
        require(file.is_file(), 'Site output must contain regular files')
        relative = file.relative_to(directory).as_posix()
        require(relative in allowed or re.fullmatch(r'assets/(?:chunks/)?[@A-Za-z0-9_.-]+\.(?:js|css)', relative),
                'Unexpected file in publication output')
        raw = file.read_bytes()
        require(len(raw) <= 16 * 1024 * 1024, 'Unexpected site file size')
        # Reject the actual machine's build root, not legitimate example paths in docs.
        require(str(ROOT).encode() not in raw and str(ROOT.resolve()).encode() not in raw,
                'Build path leaked into publication output')
        files.add(relative)
        if relative != 'build-manifest.json':
            digests[relative] = hashlib.sha256(raw).hexdigest()
        if relative.endswith('.html'):
            pages[relative] = Page(raw.decode('utf-8'))
    require(set(pages) == expected, 'Published HTML inventory differs from reviewed sources')
    for name, page in pages.items():
        for resource in page.resources:
            require(resource.startswith(base), 'Page resource must use the local project base')
            require(unquote(urlsplit(resource).path[len(base):]) in files, 'Missing local page resource')
        for link in page.links:
            parsed = urlsplit(link)
            if parsed.scheme or parsed.netloc:
                require(parsed.scheme in {'https', 'http', 'mailto'}, 'Unsupported output link scheme')
                continue
            if parsed.path:
                require(parsed.path.startswith(base), 'Local link escaped project base')
                path = unquote(parsed.path[len(base):])
                target = path + ('index.html' if path.endswith('/') or not path else '')
                if target not in files and '.' not in Path(target).name:
                    target += '.html'
            else:
                target = name
            require(target in files, 'Missing internal output link target')
            if parsed.fragment and target in pages:
                require(unquote(parsed.fragment) in pages[target].ids, 'Missing output anchor')
    return dict(sorted(digests.items()))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--directory', type=Path, default=ROOT / '.local/docs-site/dist')
    parser.add_argument('--record', action='store_true')
    args = parser.parse_args()
    try:
        catalogue = json.loads((ROOT / '.local/docs-site/catalogue.json').read_text())
        digests = check(args.directory, catalogue)
        manifest = {'schema': 'gateway-docs-build/v1', 'source_commit': catalogue['commit'],
            'working_tree': catalogue['dirty'], 'files': digests}
        path = args.directory / 'build-manifest.json'
        if args.record:
            path.write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8')
        else:
            require(json.loads(path.read_text()) == manifest, 'Build manifest does not match artifact bytes')
        print(f'Site publication check passed: files={len(digests)}, pages={sum(p.endswith(".html") for p in digests)}')
    except (ValueError, OSError) as error:
        print('Site publication check failed: ' + str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
