"""Regress the reviewed JavaScript-action migration without querying the network.

These source-contract tests reject the replaced Node 20 pins and warning
suppression. They do not infer a future action's runtime from its version: review
upstream action.yml and bundled dependencies when updating a pin, then inspect
hosted logs. Artifact and deployment authorization contracts remain separate.
"""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
LEGACY_PINS = {
    'actions/setup-node': '49933ea5288caeca8642d1e84afbd3f7d6820020',
    'actions/upload-artifact': 'ea165f8d65b6e75b540449e92b4886f43607fa02',
    'actions/download-artifact': 'd3f86a106a0bac45b974a628896c90dbdf5c8093',
    'actions/upload-pages-artifact': '7b1f4a764d45c48632c6b24a0339c27f5614fb0b',
}


class ActionsRuntimeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.workflows = {
            path.name: path.read_text(encoding='utf-8')
            for path in (ROOT / '.github/workflows').glob('*.yml')
        }

    def test_replaced_node20_actions_cannot_return_or_float(self):
        found = set()
        for name, text in self.workflows.items():
            for action, pin in re.findall(r'uses:\s+(actions/[\w-]+)@([^\s]+)', text):
                if action not in LEGACY_PINS:
                    continue
                with self.subTest(workflow=name, action=action):
                    self.assertRegex(pin, r'^[a-f0-9]{40}$')
                    self.assertNotEqual(pin, LEGACY_PINS[action])
                    found.add(action)
        self.assertEqual(found, set(LEGACY_PINS))

    def test_runtime_migration_does_not_suppress_warnings_or_allow_node20(self):
        for name, text in self.workflows.items():
            with self.subTest(workflow=name):
                for forbidden in ('ACTIONS_ALLOW_USE_UNSECURE_NODE_VERSION',
                                  'ACTIONS_RUNNER_FORCE_ACTIONS_NODE_VERSION',
                                  'FORCE_JAVASCRIPT_ACTIONS_TO_NODE24',
                                  'NODE_NO_WARNINGS', '--no-warnings', '--no-deprecation'):
                    self.assertNotIn(forbidden, text)

    def test_existing_artifacts_remain_archived_and_digest_checked(self):
        # Bound each match to its step, not another action's settings.
        for name, text in self.workflows.items():
            for block in re.split(r'(?m)^      - ', text)[1:]:
                with self.subTest(workflow=name, step=block.splitlines()[0]):
                    if 'uses: actions/upload-artifact@' in block:
                        self.assertIn('archive: true', block)
                        self.assertIn('if-no-files-found: error', block)
                    if 'uses: actions/download-artifact@' in block:
                        self.assertIn('skip-decompress: false', block)
                        self.assertIn('digest-mismatch: error', block)

    def test_documentation_build_node_version_is_not_changed_by_action_upgrade(self):
        ci = self.workflows['ci.yml']
        self.assertIn("node-version: '24.21.0'", ci)
        self.assertIn('cache-dependency-path: docs-site/package-lock.json', ci)
        self.assertIn('cache: npm', ci)


if __name__ == '__main__':
    unittest.main()
