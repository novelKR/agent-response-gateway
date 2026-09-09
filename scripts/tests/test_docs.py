"""Synthetic bilingual drift, link and editorial-record regressions."""
from __future__ import annotations

import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('docs_check', ROOT / 'scripts/check_docs.py')
docs = importlib.util.module_from_spec(spec)
spec.loader.exec_module(docs)


class DocumentationTests(unittest.TestCase):
    def setUp(self):
        state = ROOT / '.local/test-state'
        state.mkdir(parents=True, exist_ok=True)
        temporary = tempfile.TemporaryDirectory(prefix='docs-', dir=state)
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        (self.root / 'docs').mkdir()
        en = '<a id="안내"></a>\n\n# Guide\n\n[English](README.md) | [한국어](README.ko.md)\n\nUse model_alias.\n\n<a id="시작"></a>\n\n## Start\n\n```json\n{"model":"fixture"}\n```\n\n[Start](#start)\n'
        ko = '<a id="guide"></a>\n\n# 안내\n\n[English](README.md) | [한국어](README.ko.md)\n\nmodel_alias를 사용한다.\n\n<a id="start"></a>\n\n## 시작\n\n```json\n{"model":"fixture"}\n```\n\n[시작](#start)\n'
        (self.root / 'README.md').write_text(en, encoding='utf-8')
        (self.root / 'README.ko.md').write_text(ko, encoding='utf-8')
        self.document = {'schema': 'gateway-documentation/v1',
            'review_method': 'paired-editorial-review; hashes detect drift, not semantic equivalence',
            'documents': [self.entry('start', 'README.md', 'README.ko.md')]}
        self.save()

    def entry(self, ident, source, translation):
        return {'id': ident, 'section': 'start', 'order': 0, 'source': source, 'translation': translation,
            'anchors': sorted(docs.anchors((self.root / source).read_text(encoding='utf-8'))),
            'source_sha256': docs.digest((self.root / source).read_bytes()),
            'translation_sha256': docs.digest((self.root / translation).read_bytes())}

    def save(self):
        (self.root / docs.MANIFEST).write_text(json.dumps(self.document), encoding='utf-8')

    def change(self, path, old, new):
        target = self.root / path
        target.write_text(target.read_text(encoding='utf-8').replace(old, new), encoding='utf-8')

    def test_reviewed_pair_and_korean_identifier_suffix(self):
        before = {p: p.read_bytes() for p in self.root.rglob('*') if p.is_file()}
        self.assertEqual(docs.check(self.root), 1)
        self.assertEqual(before, {p: p.read_bytes() for p in before})
        self.assertEqual(docs.literals('model_alias를 읽는다.'), docs.literals('Read model_alias.'))

    def test_missing_or_stale_translation_is_an_explicit_failure(self):
        self.change('README.ko.md', '사용한다.', '읽는다.')
        with self.assertRaisesRegex(docs.DocumentationError, 'missing or stale'):
            docs.check(self.root)
        (self.root / 'README.ko.md').unlink()
        with self.assertRaisesRegex(docs.DocumentationError, 'missing'):
            docs.check(self.root)

    def test_source_change_needs_a_new_editorial_record(self):
        self.change('README.md', 'Use model_alias.', 'Read model_alias.')
        with self.assertRaisesRegex(docs.DocumentationError, 'missing or stale'):
            docs.check(self.root)
        command = [sys.executable, '-B', str(ROOT / 'scripts/check_docs.py'), 'record', '--root', str(self.root)]
        self.assertNotEqual(subprocess.run(command, capture_output=True).returncode, 0)
        completed = subprocess.run(command + ['--id', 'start'], capture_output=True)
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertEqual(docs.check(self.root), 1)

    def test_changed_command_or_identifier_is_not_fixed_by_stamping(self):
        self.change('README.ko.md', '{"model":"fixture"}', '{"model":"different"}')
        with self.assertRaisesRegex(docs.DocumentationError, 'Code blocks differ'):
            docs.check_pair(self.root, self.document['documents'][0], hashes=False)
        self.change('README.ko.md', '{"model":"different"}', '{"model":"fixture"}')
        self.change('README.ko.md', 'model_alias', 'model_aliases')
        with self.assertRaisesRegex(docs.DocumentationError, 'Technical literals differ'):
            docs.check_pair(self.root, self.document['documents'][0], hashes=False)

    def test_missing_section_and_compatibility_anchor_are_detected(self):
        self.change('README.ko.md', '<a id="start"></a>', '')
        with self.assertRaisesRegex(docs.DocumentationError, 'anchors differ'):
            docs.check(self.root)
        self.change('README.ko.md', '## 시작', '시작')
        with self.assertRaisesRegex(docs.DocumentationError, 'Sections differ'):
            docs.check(self.root)

    def test_local_links_and_anchor_targets_are_verified(self):
        for target in ('missing.md', '#missing'):
            with self.subTest(target=target), self.assertRaises(docs.DocumentationError):
                docs.local_links(self.root, 'README.md', f'[Link]({target})')
        docs.local_links(self.root, 'README.md', '[한글](README.ko.md#시작)')

    def test_removing_a_legacy_anchor_from_both_languages_still_fails(self):
        self.change('README.md', '<a id="안내"></a>', '')
        self.change('README.ko.md', '# 안내', '# Guide')
        self.change('README.ko.md', '<a id="guide"></a>', '')
        with self.assertRaisesRegex(docs.DocumentationError, 'Preserved anchors'):
            docs.check_pair(self.root, self.document['documents'][0], hashes=False)

    def test_source_link_and_image_cannot_escape_or_follow_a_symlink(self):
        assets = self.root / 'assets'
        assets.mkdir()
        (assets / 'data.json').write_text('{}')
        (assets / 'link.json').symlink_to('data.json')
        for target in ('../outside.md', '.local/private.md', 'assets/link.json', 'javascript:alert'):
            with self.subTest(target=target), self.assertRaises(docs.DocumentationError):
                docs.local_links(self.root, 'README.md', f'![Image]({target})')
        (self.root / 'README.ko.md').unlink()
        (self.root / 'README.ko.md').symlink_to('README.md')
        with self.assertRaisesRegex(docs.DocumentationError, 'symlink'):
            docs.check(self.root)

    def test_new_document_needs_a_pair_and_explicit_inventory_entry(self):
        source, translation = 'docs/extra.md', 'docs/ko/extra.md'
        (self.root / 'docs/ko').mkdir()
        (self.root / source).write_text('# Extra\n\n[한국어](ko/extra.md)\n')
        (self.root / translation).write_text('# Extra\n\n[English](../extra.md)\n')
        with self.assertRaisesRegex(docs.DocumentationError, 'inventory'):
            docs.check(self.root)
        entry = self.entry('extra', source, translation)
        entry['section'] = 'reference'
        self.document['documents'].append(entry)
        self.save()
        self.assertEqual(docs.check(self.root), 2)

    def test_duplicate_navigation_identity_or_unknown_entry_fields_reject(self):
        self.document['documents'].append(copy.deepcopy(self.document['documents'][0]))
        self.save()
        with self.assertRaisesRegex(docs.DocumentationError, 'Duplicate'):
            docs.check(self.root)
        self.document['documents'].pop()
        self.document['documents'][0]['unreviewed'] = True
        self.save()
        with self.assertRaisesRegex(docs.DocumentationError, 'Invalid document entry'):
            docs.check(self.root)

    def test_current_repository_has_complete_reviewed_documentation(self):
        self.assertGreaterEqual(docs.check(ROOT), 25)


if __name__ == '__main__':
    unittest.main()
