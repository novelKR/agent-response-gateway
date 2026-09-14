"""Prepared test inputs must bind source, platform, run and all bytes before extraction."""
import hashlib
import io
import json
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'scripts'))
import prepared_inputs as p


class PreparedTests(unittest.TestCase):
    def setUp(self):
        state=ROOT/'.local/test-state';state.mkdir(parents=True,exist_ok=True)
        self.temp=tempfile.TemporaryDirectory(dir=state);self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name);self.source=self.root/'source';self.destination=self.root/'destination'
        self.source.mkdir();self.destination.mkdir()
        for name in ('one','two'):
            path=self.source/'target'/name;path.parent.mkdir(exist_ok=True);path.write_bytes(name.encode());path.chmod(0o755)
        self.expected={'target/one','target/two'}
        self.identity=dict(schema='gateway-ci-prepared/v1',source_commit='a'*40,run_id='123',attempt='1',system='Darwin',architecture='arm64',rustc='1.98.0',locks={})
        self.addCleanup(patch.stopall)
        patch.object(p,'expected',return_value=self.expected).start()
        patch.object(p,'identity',side_effect=lambda root:dict(self.identity)).start()
        self.archive=self.root/'prepared.tar.gz';self.digest=p.pack(self.source,self.archive)

    def test_exact_inputs_restore_executable_modes(self):
        p.unpack(self.destination,self.archive,self.digest)
        self.assertEqual((self.destination/'target/one').read_bytes(),b'one')
        self.assertEqual((self.destination/'target/one').stat().st_mode&0o777,0o755)

    def test_wrong_source_run_attempt_platform_and_lock_are_rejected(self):
        for key,value in [('source_commit','b'*40),('run_id','456'),('attempt','2'),('architecture','x86_64'),('system','Linux'),('locks',{'Cargo.lock':'different'})]:
            with self.subTest(key=key):
                original=self.identity[key];self.identity[key]=value
                with self.assertRaises(ValueError):p.unpack(self.destination,self.archive,self.digest)
                self.identity[key]=original
                self.assertFalse((self.destination/'target').exists())

    def test_feature_variant_cannot_be_substituted(self):
        with self.assertRaises(ValueError):p.unpack(self.destination,self.archive,self.digest,'codec')
        self.assertFalse((self.destination/'target').exists())

    def test_transport_digest_and_existing_files_are_rejected(self):
        with self.assertRaises(ValueError):p.unpack(self.destination,self.archive,'0'*64)
        (self.destination/'target').mkdir();(self.destination/'target/one').write_bytes(b'existing')
        with self.assertRaises(ValueError):p.unpack(self.destination,self.archive,self.digest)
        self.assertEqual((self.destination/'target/one').read_bytes(),b'existing')

    def rewrite(self, mutate):
        with tarfile.open(self.archive,'r:gz') as src:
            members=[(m,src.extractfile(m).read()) for m in src.getmembers()]
        replacement=self.root/'changed.tar.gz'
        with tarfile.open(replacement,'w:gz') as dst:
            for m,raw in members:
                m,raw=mutate(m,raw);m.size=len(raw);dst.addfile(m,io.BytesIO(raw))
        return replacement,p.sha(replacement.read_bytes())

    def test_changed_last_member_leaves_no_partially_installed_binary(self):
        archive,digest=self.rewrite(lambda m,b:(m,b'changed') if m.name=='target/two' else (m,b))
        with self.assertRaises(ValueError):p.unpack(self.destination,archive,digest)
        self.assertFalse((self.destination/'target').exists())

    def test_extra_or_traversing_member_cannot_be_extracted(self):
        def rename(m,b):
            if m.name=='target/two':m.name='../outside'
            return m,b
        archive,digest=self.rewrite(rename)
        with self.assertRaises(ValueError):p.unpack(self.destination,archive,digest)
        self.assertFalse((self.root/'outside').exists())
