"""Publication input selection, extension and built navigation regressions."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, ROOT / path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


prepare = module('site_prepare', 'docs-site/scripts/prepare.py')
output = module('site_output', 'docs-site/scripts/check-output.py')


class DocsSiteTests(unittest.TestCase):
    def setUp(self):
        state = ROOT / '.local/test-state'
        state.mkdir(parents=True, exist_ok=True)
        temporary = tempfile.TemporaryDirectory(prefix='site-', dir=state)
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)

    def test_routes_keep_existing_urls_when_new_group_and_translation_are_added(self):
        navigation = json.loads((ROOT / 'docs-site/navigation.json').read_text())
        navigation['groups'].append({'id': 'new-guides', 'en': 'New guides', 'ko': '새 안내'})
        (self.root / 'README.md').write_text('# Existing\n\nExisting public summary.\n')
        (self.root / 'README.ko.md').write_text('# 기존 문서\n\n기존 문서 요약.\n')
        (self.root / 'extra.md').write_text('# New\n\nNew public summary.\n')
        (self.root / 'extra.ko.md').write_text('# 새 문서\n\n긴 한국어 제목과 요약을 추가합니다.\n')
        original = {'id': 'existing', 'source': 'README.md', 'translation': 'README.ko.md', 'section': 'start', 'order': 0, 'route': '/guide/existing'}
        manifest = {'documents': [original]}
        before = prepare.catalogue(self.root, manifest, navigation)
        manifest['documents'].append({'id': 'extra', 'source': 'extra.md', 'translation': 'extra.ko.md', 'section': 'new-guides', 'order': 0, 'route': '/guide/extra'})
        after = prepare.catalogue(self.root, manifest, navigation)
        self.assertEqual(after[:2], before)
        self.assertEqual([page['route'] for page in after[2:]], ['/guide/extra', '/ko/guide/extra'])
        self.assertEqual(after[3]['title'], '새 문서')
        self.assertEqual(after[3]['section'], 'new-guides')

    def test_only_reviewed_sources_become_local_pages_and_examples_keep_bytes(self):
        text = '[Guide](../README.md#start) [Code](../src/main.rs) `[Inline](../README.md)`\n\n```md\n[Example](../README.md)\n```\n'
        rewritten = prepare.rewrite(text, 'docs/page.md', {'README.md': '/guide/start'}, 'https://github.com/example/repository', 'a' * 40)
        self.assertIn('[Guide](/guide/start#start)', rewritten)
        self.assertIn('/blob/' + 'a' * 40 + '/src/main.rs', rewritten)
        self.assertIn('```md\n[Example](../README.md)\n```', rewritten)
        self.assertIn('`[Inline](../README.md)`', rewritten)
        for directive in ('<!--@include: ../../.private/record.md-->', '<script>export default {}</script>', '<style>body{}</style>'):
            with self.assertRaises(ValueError):
                prepare.rewrite(directive, 'docs/page.md', {}, 'https://example.com', 'a' * 40)

    def fixture(self):
        catalogue = {'navigation': {'base': '/project/', 'locales': {'en': {'prefix': ''}}, 'groups': []}, 'pages': [{'route': '/'}]}
        (self.root / 'index.html').write_text('<div id="start"><a href="/project/#start">Start</a></div>')
        (self.root / '404.html').write_text('<a href="/project/">Home</a>')
        return catalogue

    def test_extra_pages_and_private_canaries_cannot_enter_the_artifact(self):
        catalogue = self.fixture()
        self.assertEqual(len(output.check(self.root, catalogue)), 2)
        extra = self.root / 'private-record.md'
        extra.write_text('synthetic-publication-canary')
        with self.assertRaisesRegex(ValueError, 'Unexpected file'):
            output.check(self.root, catalogue)
        extra.unlink()
        (self.root / 'unreviewed.html').write_text('<h1>Unreviewed</h1>')
        with self.assertRaises(ValueError):
            output.check(self.root, catalogue)

    def test_resource_base_missing_anchor_and_symlink_fail(self):
        catalogue = self.fixture()
        for text in ('<a href="/guide">Wrong base</a>', '<a href="/project/#missing">Missing anchor</a>',
                     '<script src="https://example.com/tracker.js"></script>'):
            (self.root / 'index.html').write_text(text)
            with self.assertRaises(ValueError):
                output.check(self.root, catalogue)
        (self.root / 'index.html').unlink()
        (self.root / 'index.html').symlink_to('404.html')
        with self.assertRaisesRegex(ValueError, 'symlink'):
            output.check(self.root, catalogue)

    def test_reviewed_web_records_preserve_exact_originals_and_lock_integrities(self):
        records = json.loads((ROOT / 'docs-site/licensing/dependencies.json').read_text())
        lock = json.loads((ROOT / 'docs-site/package-lock.json').read_text())
        for record in records:
            if 'integrity' in record:
                matches = [entry for name, entry in lock['packages'].items()
                    if name.endswith('node_modules/' + record['name']) and entry.get('version') == record['version']]
                self.assertTrue(matches)
                self.assertEqual(matches[0]['integrity'], record['integrity'])
        for record in json.loads((ROOT / 'docs-site/licensing/supplements.json').read_text()).values():
            self.assertEqual(hashlib.sha256((ROOT / 'docs-site/licensing/originals' / record['file']).read_bytes()).hexdigest(), record['sha256'])
        for asset in json.loads((ROOT / 'docs-site/licensing/embedded-assets.json').read_text()):
            for notice in asset['notices']:
                self.assertEqual(hashlib.sha256((ROOT / 'docs-site/licensing/originals' / notice['file']).read_bytes()).hexdigest(), notice['sha256'])


if __name__ == '__main__':
    unittest.main()
