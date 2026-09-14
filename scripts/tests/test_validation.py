"""Synthetic impact and execution contracts; no live providers or network."""
import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('validation', ROOT / 'scripts/validation.py')
v = importlib.util.module_from_spec(spec)
spec.loader.exec_module(v)


class ImpactTests(unittest.TestCase):
    def plan(self, paths, **kwargs):
        with patch.object(v, 'changes', return_value=(paths, 'a'*40, 'b'*40)), patch.object(v, 'input_digest', return_value='c'*64):
            return v.make_plan(ROOT, **kwargs)

    def test_web_presentation_requires_no_rust_or_license_tools(self):
        p = self.plan(['management-web/src/style.css'])
        self.assertEqual(p['jobs'], ['management-web', 'publication'])
        self.assertEqual(p['checks'], ['boundary', 'web'])
        self.assertNotIn('cargo', p['tools'])
        self.assertNotIn('cargo-deny', p['tools'])

    def test_web_parser_includes_actual_api_fixture(self):
        self.assertIn('web-api', self.plan(['management-web/src/api.mjs'])['checks'])

    def test_authentication_container_keeps_api_fixture_but_pure_view_does_not(self):
        self.assertIn('web-api', self.plan(['management-web/src/App.vue'])['checks'])
        self.assertNotIn('cargo', self.plan(['management-web/src/JsonView.vue'])['tools'])

    def test_docs_do_not_select_product_packaging(self):
        self.assertEqual(self.plan(['docs/management.md'])['jobs'], ['docs', 'publication'])

    def test_shared_notice_tool_selects_both_web_consumers_and_packages(self):
        p = self.plan(['docs-site/scripts/web-notices.mjs'])
        self.assertTrue({'management-web', 'docs', 'package-smoke', 'targets'} <= set(p['jobs']))

    def test_unknown_and_dependency_changes_expand_to_full(self):
        for path in ['new-area/input', 'Cargo.lock', 'crates/management/Cargo.toml', 'rust-toolchain.toml']:
            with self.subTest(path=path):
                self.assertEqual(self.plan([path])['profile'], 'full')

    def test_policy_cannot_remove_its_own_expansion(self):
        policy, digest = v.read_policy(ROOT)
        policy['rules'] = [dict(name='everything', patterns=['*'], ci=[], local=[])]
        with patch.object(v, 'read_policy', return_value=(policy, digest)):
            for path in ['scripts/validation.py', 'scripts/validation-policy.json', '.github/workflows/ci.yml']:
                self.assertEqual(self.plan([path])['profile'], 'full')

    def test_force_full_can_only_expand(self):
        policy, digest = v.read_policy(ROOT)
        policy['force_full'] = True
        with patch.object(v, 'read_policy', return_value=(policy, digest)):
            self.assertEqual(self.plan(['management-web/src/style.css'])['profile'], 'full')

    def test_unresolved_git_input_reports_full_without_claiming_sha(self):
        with patch.object(v, 'changes', side_effect=v.ValidationError('missing')):
            p = v.make_plan(ROOT)
        self.assertEqual(p['profile'], 'full')
        self.assertIsNone(p['head_sha'])
        self.assertIn('unresolved-git-input', p['reasons'])

    def test_management_selects_reverse_consumers(self):
        p = self.plan(['crates/management/src/lib.rs'])
        self.assertTrue({'gateway-management', 'gateway-management-api', 'gateway-management-app', 'gateway-management-embedded'} <= set(p['packages']))
        self.assertNotIn('agent-response-gateway', p['packages'])

    def test_shared_usage_contract_expands_to_gateway_and_recorder(self):
        p = self.plan(['crates/usage-contract/src/lib.rs'])
        self.assertTrue({'agent-response-gateway', 'gateway-usage-recorder'} <= set(p['packages']))
        self.assertTrue({'codex-conformance', 'usage-recorder'} <= set(p['jobs']))

    def test_recorder_keeps_database_and_upgrade_job(self):
        p = self.plan(['extensions/usage-recorder/src/storage.rs'])
        self.assertIn('usage-recorder', p['jobs'])
        self.assertIn('gateway-usage-recorder', p['packages'])

    def test_mixed_changes_take_union(self):
        p = self.plan(['management-web/src/style.css', 'docs/management.md'])
        self.assertEqual(p['jobs'], ['docs', 'management-web', 'publication'])

    def test_empty_change_has_explicit_publication_check(self):
        self.assertEqual(self.plan([])['jobs'], ['publication'])

    def test_all_tracked_paths_have_a_rule(self):
        policy, _ = v.read_policy(ROOT)
        paths = v.split_paths(v.git(ROOT, 'ls-files', '-z'))
        unknown = [p for p in paths if v.rule_for(policy, p) is None]
        self.assertEqual(unknown, [])

    def test_rust_commands_select_packages_and_preserve_team_features(self):
        p = self.plan(['crates/management/src/lib.rs'])
        policy, _ = v.read_policy(ROOT)
        cmds = [c for name, c in v.commands(ROOT, p, policy) if name == 'rust']
        self.assertIn('gateway-management-app/team', ' '.join(cmds[-1]))
        self.assertIn('-p', cmds[-1])

    def test_range_plan_is_not_executed_against_unrelated_worktree(self):
        with self.assertRaises(v.ValidationError):
            v.run_plan(ROOT, self.plan([], scope='range'))


class GitSelectionTests(unittest.TestCase):
    def setUp(self):
        state = ROOT / '.local/test-state'
        state.mkdir(parents=True, exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(dir=state)
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        v.git(self.root, 'init', '-q')
        v.git(self.root, 'config', 'user.name', 'Synthetic')
        v.git(self.root, 'config', 'user.email', 'test@example.invalid')
        (self.root / 'old.txt').write_text('before')
        v.git(self.root, 'add', '.')
        v.git(self.root, 'commit', '-qm', 'Synthetic input')
        self.base = v.git(self.root, 'rev-parse', 'HEAD').decode().strip()

    def test_staged_rename_includes_both_paths(self):
        v.git(self.root, 'mv', 'old.txt', 'new.txt')
        paths, _, _ = v.changes(self.root, 'staged')
        self.assertEqual(paths, ['new.txt', 'old.txt'])

    def test_untracked_and_unstaged_are_not_in_staged_plan(self):
        (self.root / 'old.txt').write_text('changed')
        (self.root / 'new.txt').write_text('new')
        self.assertEqual(v.changes(self.root, 'staged')[0], [])
        self.assertEqual(v.changes(self.root, 'worktree')[0], ['new.txt', 'old.txt'])

    def test_input_digest_detects_content_and_index_changes(self):
        paths, base, head = v.changes(self.root, 'worktree')
        before = v.input_digest(self.root, 'worktree', base, head, paths)
        (self.root / 'new.txt').write_text('one')
        first = v.input_digest(self.root, 'worktree', base, head, ['new.txt'])
        (self.root / 'new.txt').write_text('two')
        second = v.input_digest(self.root, 'worktree', base, head, ['new.txt'])
        self.assertEqual(len({before, first, second}), 3)
        v.git(self.root, 'add', 'new.txt')
        staged = v.input_digest(self.root, 'staged', base, head, ['new.txt'])
        (self.root / 'new.txt').write_text('three')
        self.assertEqual(v.input_digest(self.root, 'staged', base, head, ['new.txt']), staged)

    def test_range_resolves_exact_commits_and_deletion(self):
        v.git(self.root, 'rm', 'old.txt')
        v.git(self.root, 'commit', '-qm', 'Synthetic deletion')
        paths, base, head = v.changes(self.root, 'range', self.base, 'HEAD')
        self.assertEqual(paths, ['old.txt'])
        self.assertEqual(base, self.base)
        self.assertNotEqual(base, head)


if __name__ == '__main__':
    unittest.main()
