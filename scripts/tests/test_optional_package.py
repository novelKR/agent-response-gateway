import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest

SCRIPTS=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(SCRIPTS))
import optional_package as optional
import release_package as package


class OptionalPackageTests(unittest.TestCase):
    def files(self,kind='management'):
        target='aarch64-apple-darwin';commit='a'*40
        files={'gateway-management/scripts/extension_manager.py':(b'synthetic driver, never run',0o644),'gateway-management/LICENSE':(b'synthetic license',0o644),'gateway-management/license-notices/THIRD-PARTY-NOTICES.md':(b'synthetic original notice',0o644)}
        for binary in ['gateway-manager','gateway-managed-child','gateway-management-cli']:
            files['gateway-management/bin/'+binary]=(b'synthetic test executable, never run',0o755)
        manifest=optional.module(kind,commit,target,files)
        files[f'gateway-management/modules/{kind}.json']=(package.encoded(manifest),0o644)
        return target,commit,files

    def test_native_module_requires_exact_compatible_files_and_original_notices(self):
        target,commit,files=self.files()
        optional.verify_module('management',commit,target,files)
        with self.assertRaises(package.PackageError):
            optional.verify_module('management','b'*40,target,files)
        changed=dict(files);changed['gateway-management/bin/gateway-manager']=(b'changed executable',0o755)
        with self.assertRaises(package.PackageError):
            optional.verify_module('management',commit,target,changed)
        changed=dict(files);del changed['gateway-management/license-notices/THIRD-PARTY-NOTICES.md']
        value=optional.module('management',commit,target,{n:v for n,v in changed.items() if '/modules/' not in n})
        changed['gateway-management/modules/management.json']=(package.encoded(value),0o644)
        with self.assertRaises(package.PackageError):
            optional.verify_module('management',commit,target,changed)

    def test_module_cannot_claim_unknown_contract_or_automatic_activation(self):
        target,commit,files=self.files()
        path='gateway-management/modules/management.json'
        for field,value in [('contract','gateway-management-http/v99'),('enabled_automatically',True),('requires',{'unverified-host':commit})]:
            changed=dict(files);manifest=json.loads(files[path][0]);manifest[field]=value;changed[path]=(package.encoded(manifest),0o644)
            with self.assertRaises(package.PackageError):
                optional.verify_module('management',commit,target,changed)

    def test_extraction_never_overwrites_an_existing_candidate_file(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            optional.extract({'candidate/file':(b'first',0o644)},root)
            with self.assertRaises(FileExistsError):
                optional.extract({'candidate/file':(b'second',0o644)},root)
            self.assertEqual((root/'candidate/file').read_bytes(),b'first')


if __name__=='__main__':
    unittest.main()
