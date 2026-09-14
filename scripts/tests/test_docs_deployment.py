"""Documentation selection and served-byte verification without deploying or HTTP calls."""
import hashlib
import json
from pathlib import Path
import sys
import unittest
from unittest.mock import patch
ROOT=Path(__file__).resolve().parents[2];sys.path.insert(0,str(ROOT/'scripts'))
import docs_deployment as docs


class DocsDeploymentTests(unittest.TestCase):
    def test_only_documentation_impact_or_manual_main_run_selects_deployment(self):
        env=dict(GITHUB_REF='refs/heads/main',GITHUB_EVENT_NAME='push',GITHUB_SHA='a'*40)
        for path in ['src/lib.rs','scripts/usage_smoke.py','scripts/release_provenance.py']:
            with patch.object(docs.validation,'changes',return_value=([path],'b'*40,'a'*40)):
                self.assertFalse(docs.needed(ROOT,{'before':'b'*40},env))
        for path in ['docs/github-workflow.md','CONTRIBUTING.md','licensing/README.md','docs-site/package-lock.json','scripts/check_public_boundary.py','.github/workflows/docs-deploy.yml']:
            with patch.object(docs.validation,'changes',return_value=([path],'b'*40,'a'*40)):
                self.assertTrue(docs.needed(ROOT,{'before':'b'*40},env))
        with patch.object(docs.validation,'changes',side_effect=docs.validation.ValidationError('missing')):
            self.assertTrue(docs.needed(ROOT,{'before':'b'*40},env))
        env['GITHUB_EVENT_NAME']='workflow_dispatch';self.assertTrue(docs.needed(ROOT,{},env))
        env['GITHUB_REF']='refs/pull/1/merge'
        with self.assertRaises(ValueError):docs.needed(ROOT,{},env)

    def test_served_manifest_must_match_built_bytes_and_source(self):
        value=dict(schema='gateway-docs-build/v1',source_commit='a'*40,working_tree=False,files={'index.html':'b'*64})
        raw=json.dumps(value).encode();digest=hashlib.sha256(raw).hexdigest()
        docs.verify_bytes(raw,'a'*40,digest)
        with self.assertRaises(ValueError):docs.verify_bytes(raw,'c'*40,digest)
        with self.assertRaises(ValueError):docs.verify_bytes(raw+b' ','a'*40,digest)
        value['working_tree']=True;raw=json.dumps(value).encode()
        with self.assertRaises(ValueError):docs.verify_bytes(raw,'a'*40,hashlib.sha256(raw).hexdigest())
