"""Regression for packaging hard-linked compiler outputs without trusting installed links."""
import os
from pathlib import Path
import tempfile
import unittest

from test_extension_manager import m


@unittest.skipUnless(os.name == 'posix', 'Private extension stores require POSIX')
class ExtensionBuildOutputTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.store = self.root / 'store'
        self.binary = self.root / 'binary'
        self.binary.write_bytes(b'synthetic compiler output')
        self.license = self.root / 'license'
        self.license.write_bytes(b'Synthetic license\n')

    def test_hardlinked_build_output_is_copied_into_an_independent_package(self):
        alias = self.root / 'compiler-output-alias'
        os.link(self.binary, alias)
        original = self.binary.read_bytes()
        output = self.root / 'from-build'
        sha = m.package_binary(alias, self.license, output, 'observer', '0.2.0')
        self.assertEqual((output / 'extension').stat().st_nlink, 1)
        self.assertNotEqual((output / 'extension').stat().st_ino, alias.stat().st_ino)
        self.binary.write_bytes(b'new compiler output')
        self.assertEqual((output / 'extension').read_bytes(), original)
        m.install(self.store, output, sha)
        m.enable(self.store, 'observer', '0.2.0', sha, m.PERMISSIONS)
        installed = m.installed_dir(self.store, 'observer', '0.2.0', sha) / 'extension'
        os.link(installed, self.root / 'installed-alias')
        with self.assertRaises(m.ExtensionError):
            m.enable(self.store, 'observer', '0.2.0', sha, m.PERMISSIONS)

    def test_package_helper_still_rejects_symlink_build_output(self):
        alias = self.root / 'symbolic-build-output'
        alias.symlink_to(self.binary)
        with self.assertRaises(m.ExtensionError):
            m.package_binary(alias, self.license, self.root / 'rejected', 'observer', '0.2.0')
        self.assertFalse((self.root / 'rejected').exists())


if __name__ == '__main__':
    unittest.main()
