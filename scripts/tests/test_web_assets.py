"""Package consumers reject mismatched commit, notices, inventory and asset bytes."""
import json
from pathlib import Path
import sys
import tempfile
import unittest
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'scripts'))
import web_assets as web
import release_package as package


class WebAssetsTests(unittest.TestCase):
    def setUp(self):
        state=ROOT/'.local/test-state';state.mkdir(parents=True,exist_ok=True)
        self.temp=tempfile.TemporaryDirectory(dir=state);self.addCleanup(self.temp.cleanup)
        self.source=Path(self.temp.name);self.assets=self.source/'assets';self.assets.mkdir()
        (self.source/'management-web/licensing').mkdir(parents=True)
        (self.source/'LICENSE').write_bytes(b'license')
        dependencies=json.dumps([dict(name='synthetic-package',version='1',notices=[dict(file='LICENSE',sha256=package.sha(b'original notice'))])]).encode()
        (self.source/'management-web/licensing/dependencies.json').write_bytes(dependencies)
        notices=b'Web dependencies shipped by the optional management dashboard.\nOriginal license and notice bytes follow each package heading.\n\n===== synthetic-package@1 =====\n\n--- LICENSE ---\noriginal notice\n'
        files={'index.html':b'<html></html>','LICENSE.txt':b'license','web-notices.txt':notices,'web-dependencies.json':dependencies}
        for name,raw in files.items():(self.assets/name).write_bytes(raw)
        self.manifest=dict(schema='gateway-management-web/v1',api_contract='gateway-management-http/v1',state_contract='gateway-management-state/v1',read_only=True,source_commit='a'*40,source_dirty=False,files={n:package.sha(b) for n,b in files.items()})
        self.write_manifest()

    def write_manifest(self):
        (self.assets/'web-manifest.json').write_text(json.dumps(self.manifest))

    def test_exact_assets_can_be_used_without_node_or_cargo(self):
        self.assertEqual(len(web.verify(self.source,self.assets,'a'*40)),5)

    def test_other_commit_dirty_source_or_contract_fails(self):
        for key,value in [('source_commit','b'*40),('source_dirty',True),('api_contract','wrong'),('state_contract','wrong'),('read_only',False)]:
            original=self.manifest[key];self.manifest[key]=value;self.write_manifest()
            with self.assertRaises(package.PackageError):web.verify(self.source,self.assets,'a'*40)
            self.manifest[key]=original;self.write_manifest()

    def test_modified_and_extra_assets_fail(self):
        (self.assets/'extra.js').write_bytes(b'extra')
        with self.assertRaises(package.PackageError):web.verify(self.source,self.assets,'a'*40)
        (self.assets/'extra.js').unlink();(self.assets/'index.html').write_bytes(b'changed')
        with self.assertRaises(package.PackageError):web.verify(self.source,self.assets,'a'*40)

    def test_rehashed_notice_substitution_still_fails_source_comparison(self):
        (self.assets/'LICENSE.txt').write_bytes(b'other license')
        self.manifest['files']['LICENSE.txt']=package.sha(b'other license');self.write_manifest()
        with self.assertRaises(package.PackageError):web.verify(self.source,self.assets,'a'*40)

    def test_linked_output_is_rejected(self):
        try:(self.assets/'link').symlink_to(self.source/'LICENSE')
        except OSError:self.skipTest('Symlinks unavailable')
        with self.assertRaises(package.PackageError):web.verify(self.source,self.assets,'a'*40)

    def test_rehashed_third_party_notice_body_is_rejected(self):
        path=self.assets/'web-notices.txt';path.write_bytes(path.read_bytes().replace(b'original notice',b'substituted text'))
        self.manifest['files']['web-notices.txt']=package.sha(path.read_bytes());self.write_manifest()
        with self.assertRaises(package.PackageError):web.verify(self.source,self.assets,'a'*40)
