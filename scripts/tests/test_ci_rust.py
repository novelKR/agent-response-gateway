"""Keep the existing full native matrix and narrow only explicit affected consumers."""
import json
from pathlib import Path
import sys
import unittest
ROOT=Path(__file__).resolve().parents[2];sys.path.insert(0,str(ROOT/'scripts'))
import ci_rust
import validation


class NativeSelectionTests(unittest.TestCase):
    def plan(self,packages,profile='affected'):
        return dict(source_sha='a'*40,policy_sha256=validation.read_policy(ROOT)[1],packages=packages,profile=profile,scope='range')

    def test_selected_management_consumers_keep_team_features(self):
        commands=ci_rust.selected_commands(ROOT,self.plan(['gateway-management-app','gateway-management-embedded']),'a'*40)
        self.assertEqual(len(commands),2)
        for command in commands:
            self.assertIn('gateway-management-app/team,gateway-management-embedded/team', ','.join(sorted(command[command.index('--features')+1].split(','))))
            self.assertNotIn('gateway-usage-recorder',command)
            self.assertIn('--locked',command)
            self.assertEqual(command[1],'+1.98.0')

    def test_full_matrix_preserves_native_packages_and_excludes_recorder_platform_expansion(self):
        command=ci_rust.selected_commands(ROOT,self.plan(['gateway-usage-recorder'],profile='full'),'a'*40)[1]
        self.assertIn('agent-response-gateway',command)
        self.assertIn('gateway-management-runtime',command)
        self.assertNotIn('gateway-usage-recorder',command)

    def test_unresolved_script_package_inputs_remain_conservative(self):
        command=ci_rust.selected_commands(ROOT,self.plan([]),'a'*40)[1]
        self.assertIn('agent-response-gateway',command)

    def test_wrong_source_or_policy_is_rejected(self):
        plan=self.plan(['agent-response-gateway'])
        with self.assertRaises(validation.ValidationError):ci_rust.selected_commands(ROOT,plan,'b'*40)
        plan['policy_sha256']='0'*64
        with self.assertRaises(validation.ValidationError):ci_rust.selected_commands(ROOT,plan,'a'*40)


class NativeFixtureOrderingTests(unittest.TestCase):
    def test_gateway_fixture_is_built_before_scoped_tests_without_cached_binaries(self):
        workflow=(ROOT/'.github/workflows/ci.yml').read_text()
        build=workflow.index('      - run: cargo +1.98.0 build -p agent-response-gateway --locked')
        tests=workflow.index('      - name: Check selected native packages and consumers')
        self.assertLess(build,tests)
