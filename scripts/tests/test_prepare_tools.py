"""Restored tools must be verified; only a missing tool is installed."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
ROOT=Path(__file__).resolve().parents[2]
spec=importlib.util.spec_from_file_location('prepare',ROOT/'scripts/prepare_tools.py');p=importlib.util.module_from_spec(spec);spec.loader.exec_module(p)


class PreparationTests(unittest.TestCase):
    def test_valid_cache_avoids_install_and_wrong_cache_is_not_replaced(self):
        state=ROOT/'.local/test-state';state.mkdir(parents=True,exist_ok=True)
        with tempfile.TemporaryDirectory(dir=state) as d:
            root=Path(d);(root/'licensing').mkdir();(root/'licensing/policy.json').write_text(json.dumps({'cargo_deny_version':'0.20.2'}))
            binary=root/'.local/tools/bin'/('cargo-deny.exe' if p.sys.platform=='win32' else 'cargo-deny');binary.parent.mkdir(parents=True);binary.write_bytes(b'synthetic')
            with patch.object(p,'ROOT',root),patch.object(p.subprocess,'check_output',return_value='cargo-deny 0.20.2\n'),patch.object(p.subprocess,'run') as run:
                p.prepare();self.assertEqual(run.call_count,1);self.assertIn('fetch',run.call_args.args[0])
            with patch.object(p,'ROOT',root),patch.object(p.subprocess,'check_output',return_value='cargo-deny wrong\n'),patch.object(p.subprocess,'run') as run:
                with self.assertRaises(ValueError):p.prepare()
                run.assert_not_called()
