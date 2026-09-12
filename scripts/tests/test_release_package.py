import copy
import io
import json
from pathlib import Path
import sys
import stat
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import release_package as package


COMMIT = "1" * 40
TARGET = "aarch64-apple-darwin"


def metadata():
    names = ["root", "normal", "build", "macro", "macro_dep", "dev", "other_target"]
    def dep(name, kind=None):
        return {"pkg":name, "name":name, "dep_kinds":[{"kind":kind,"target":None}]}
    packages = [{"id":name,"name":"agent-response-gateway" if name == "root" else name,"version":"0.1.0","source":None if name == "root" else package.license_audit.REGISTRY} for name in names]
    nodes = [{"id":name,"deps":[]} for name in names]
    nodes[0]["deps"] = [dep("normal"), dep("build", "build"), dep("macro"), dep("dev", "dev"), dep("other_target")]
    nodes[3]["deps"] = [dep("macro_dep")]
    return {"version":1,"packages":packages,"resolve":{"root":"root","nodes":nodes}}


def built():
    return {name:{"features":{"enabled"},"kinds":{"proc-macro" if name == "macro" else "lib"}} for name in ["root","normal","build","macro","macro_dep"]}


def records():
    return {"packages":[{"name":p["name"],"version":"0.1.0","source":p["source"],"checksum":"a"*64,"selected_licenses":["MIT"]} for p in metadata()["packages"] if p["source"]]}


def sbom(meta=None, artifacts=None, licenses=None):
    return package.make_sbom(meta or metadata(), artifacts or built(), licenses or records(), TARGET, "0.1.0", COMMIT, "b"*64, {"release":"1.98.0"}, {"libraries":["/usr/lib/libSystem.B.dylib"]})


def source_archive():
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w:gz", format=tarfile.PAX_FORMAT, pax_headers={"comment":COMMIT}) as archive:
        for name, data in {"LICENSE":b"synthetic source license", "scripts/release_package.py":b"synthetic builder", "scripts/package_smoke.py":b"synthetic smoke", "scripts/release_targets.py":b"synthetic targets"}.items():
            item = tarfile.TarInfo("agent-response-gateway/" + name)
            item.mode, item.size = 0o644, len(data)
            archive.addfile(item, io.BytesIO(data))
    return raw.getvalue()


def fixture(directory, files=None, target=TARGET):
    executable = "agent-response-gateway/bin/" + package.TARGETS[target]["executable"]
    files = files or {executable:(b"synthetic executable",0o755), "agent-response-gateway/LICENSE":(b"synthetic license",0o644)}
    binary = files[executable][0]
    bom = sbom()
    bom["metadata"]["component"]["hashes"] = [{"alg":"SHA-256","content":package.sha(binary)}]
    binary_name = "binary." + package.TARGETS[target]["archive"]
    assets = {binary_name:package.archive_bytes(files, target),"source.tar.gz":source_archive(),"sbom.json":package.encoded(bom)}
    for name,data in assets.items():
        package.write_new(directory/name, data)
    manifest = {"schema":package.SCHEMA,"target":target,"source_commit":COMMIT,"assets":{n:package.sha(v) for n,v in assets.items()},"binary_archive":binary_name,"source_archive":"source.tar.gz","sbom":"sbom.json","binary_sha256":package.sha(binary),"package_files":{n:{"sha256":package.sha(v),"mode":m} for n,(v,m) in files.items()}, "packaging_tools":{"release_package.py":package.sha(b"synthetic builder"),"package_smoke.py":package.sha(b"synthetic smoke"),"release_targets.py":package.sha(b"synthetic targets")}}
    package.write_new(directory/"candidate.json", package.encoded(manifest))
    hashes = {p.name:package.sha(package.read(p)) for p in directory.iterdir()}
    package.write_new(directory/"SHA256SUMS", "".join(f"{v}  {n}\n" for n,v in sorted(hashes.items())).encode())


class ReleasePackageTests(unittest.TestCase):
    def test_target_sbom_uses_actual_build_inputs_and_distinguishes_build_scope(self):
        value = sbom()
        components = {c["name"]:c for c in value["components"]}
        self.assertNotIn("dev", components)
        self.assertNotIn("other_target", components)
        self.assertEqual(components["normal"]["scope"], "required")
        for name in ["macro","macro_dep","build"]:
            self.assertEqual(components[name]["scope"], "excluded")
        self.assertEqual(value["compositions"][0]["aggregate"], "incomplete")
        self.assertFalse(any("file://" in json.dumps(c) for c in value["components"]))
        alternate = built(); alternate["other_target"] = {"features":set(),"kinds":{"lib"}}
        self.assertIn("other_target", {c["name"] for c in sbom(artifacts=alternate)["components"]})

    def test_sbom_rejects_missing_license_source_and_unreachable_build_evidence(self):
        licenses = records(); licenses["packages"] = []
        with self.assertRaises(package.PackageError):
            sbom(licenses=licenses)
        wrong = metadata(); wrong["packages"][1]["source"] = "git+https://example.test/unreviewed"
        with self.assertRaises(package.PackageError):
            sbom(meta=wrong)
        artifacts = built(); artifacts["dev"] = {"features":set(),"kinds":{"lib"}}
        with self.assertRaises(package.PackageError):
            sbom(artifacts=artifacts)

    def test_local_usage_contract_is_bound_to_corresponding_source_not_a_registry_checksum(self):
        meta = metadata()
        source = Path('/synthetic-source').resolve()
        meta['workspace_root'] = str(source)
        meta['workspace_members'] = ['root', 'usage']
        meta['packages'].append({'id':'usage', 'name':'gateway-usage-contract', 'version':'0.1.0',
                                 'license':'AGPL-3.0-only', 'source':None,
                                 'manifest_path':str(source / 'crates/usage-contract/Cargo.toml')})
        meta['resolve']['nodes'].append({'id':'usage', 'deps':[]})
        meta['resolve']['nodes'][0]['deps'].append({'pkg':'usage', 'name':'usage', 'dep_kinds':[{'kind':None, 'target':None}]})
        artifacts = built()
        artifacts['usage'] = {'features':set(), 'kinds':{'lib'}}
        value = sbom(meta=meta, artifacts=artifacts)
        component = next(c for c in value['components'] if c['name'] == 'gateway-usage-contract')
        self.assertEqual(component['licenses'], [{'expression':'AGPL-3.0-only'}])
        self.assertNotIn('hashes', component)
        for field, invalid in [('name', 'unreviewed-local'), ('license', 'MIT'), ('manifest_path', str(source / 'other/Cargo.toml'))]:
            wrong = copy.deepcopy(meta)
            wrong['packages'][-1][field] = invalid
            with self.assertRaises(package.PackageError):
                sbom(meta=wrong, artifacts=artifacts)

    def test_cargo_artifact_evidence_requires_success_and_collects_compiled_features(self):
        raw = b'{"reason":"compiler-artifact","package_id":"root","features":["a"],"target":{"kind":["lib"]}}\n'
        with self.assertRaises(package.PackageError):
            package.compiled_packages(raw)
        value = package.compiled_packages(raw + b'{"reason":"build-finished","success":true}\n')
        self.assertEqual(value["root"]["features"], {"a"})
        with self.assertRaises(package.PackageError):
            package.compiled_packages(raw + b'{"reason":"build-finished","success":false}\n')

    def test_archives_are_deterministic_and_preserve_only_reviewed_bytes_and_modes(self):
        files = {"a/text":("합성".encode(),0o644),"a/bin":(b"executable",0o755)}
        self.assertEqual(package.tar_bytes(files), package.tar_bytes(dict(reversed(list(files.items())))))
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary)/"archive.tar.gz"; path.write_bytes(package.tar_bytes(files))
            self.assertEqual(package.archive_files(path), files)
            with tarfile.open(path) as archive:
                self.assertTrue(all(x.uid == x.gid == x.mtime == 0 and x.uname == x.gname == "" for x in archive))
        for name in ["../outside", "/absolute", "a//b", "a/./b", "a\\b"]:
            with self.assertRaises(package.PackageError):
                package.tar_bytes({name:(b"x",0o644)})
        with self.assertRaises(package.PackageError):
            package.tar_bytes({"a":(b"x",0o777)})

    def test_candidate_verification_binds_assets_membership_commit_target_and_executable(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary); fixture(directory)
            package.verify_candidate(directory, COMMIT, TARGET)
            for commit,target in [("2"*40,TARGET),(COMMIT,"x86_64-unknown-linux-gnu")]:
                with self.assertRaises(package.PackageError):
                    package.verify_candidate(directory, commit, target)
            (directory/"extra").write_text("unexpected")
            with self.assertRaises(package.PackageError):
                package.verify_candidate(directory)
            (directory/"extra").unlink()
            with (directory/"binary.tar.gz").open("ab") as stream:
                stream.write(b"tampered")
            with self.assertRaises(package.PackageError):
                package.verify_candidate(directory)

    def test_all_native_targets_bind_the_expected_packaged_executable(self):
        self.assertEqual(len(package.TARGETS), 4)
        for target in package.TARGETS:
            with self.subTest(target=target), tempfile.TemporaryDirectory() as temporary:
                directory = Path(temporary)
                fixture(directory, target=target)
                package.verify_candidate(directory, COMMIT, target)

    def test_zip_determinism_and_safe_paths_preserve_exact_notice_bytes(self):
        files = {"a/notice.txt":(b"original\r\nnotice\r\n",0o644), "a/bin.exe":(b"synthetic executable",0o755)}
        raw = package.zip_bytes(files)
        self.assertEqual(raw, package.zip_bytes(dict(reversed(list(files.items())))))
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary)/"package.zip"; path.write_bytes(raw)
            self.assertEqual(package.archive_files(path), files)
        for name in ["../escape", "C:/absolute", "a/stream:secret", "a/NUL.txt", "a/trailing.", "a/trailing ", "a\x00hidden", "a\\b"]:
            with self.subTest(name=name), self.assertRaises(package.PackageError):
                package.zip_bytes({name:(b"x",0o644)})

    def test_zip_rejects_duplicates_links_path_conflicts_metadata_and_size_limits(self):
        for change in ["case", "link", "parent", "extra", "size"]:
            with self.subTest(change=change), tempfile.TemporaryDirectory() as temporary:
                path = Path(temporary)/"bad.zip"
                with zipfile.ZipFile(path, "w") as archive:
                    for name in (["a", "A"] if change == "case" else ["a", "a/b"] if change == "parent" else ["a"]):
                        item = zipfile.ZipInfo(name, (1980,1,1,0,0,0)); item.create_system = 3
                        item.external_attr = ((stat.S_IFLNK if change == "link" else stat.S_IFREG) | 0o644) << 16
                        item.compress_type = zipfile.ZIP_DEFLATED
                        if change == "extra":
                            item.extra = b"\x01\x00\x00\x00"
                        archive.writestr(item, b"synthetic")
                with patch.object(package, "MAX_FILE", 1 if change == "size" else package.MAX_FILE), self.assertRaises(package.PackageError):
                    package.archive_files(path)

    def test_windows_build_selects_native_msvc_before_git_link_and_filters_setup_output(self):
        env = {"PATH":"synthetic-git-bin", "COMSPEC":"cmd.exe", "PROGRAMFILES(X86)":"C:/Program Files (x86)"}
        configured = "Path=synthetic-sdk-path\r\nINCLUDE=synthetic-headers\r\nLIB=synthetic-libraries\r\nVCToolsInstallDir=C:/VS/VC/Tools/MSVC/14.50/\r\nVSCMD_ARG_TGT_ARCH=x64\r\nVSCMD_ARG_HOST_ARCH=x64\r\nGH_TOKEN=do-not-inherit\r\nRUSTFLAGS=do-not-inherit\r\n"
        with tempfile.TemporaryDirectory(prefix="source with spaces ") as temporary:
            for arch in ["x64", "x86"]:
                with self.subTest(arch=arch), patch.object(package, "run", side_effect=[b"C:/Program Files/VS\r\n", configured.replace("TGT_ARCH=x64", "TGT_ARCH=" + arch).encode("utf-16-le")]) as run, patch.object(Path, "is_file", return_value=True):
                    if arch != "x64":
                        with self.assertRaises(package.PackageError):
                            package.windows_environment(env, Path(temporary)/"source")
                        continue
                    result = package.windows_environment(env, Path(temporary)/"source")
                    self.assertTrue(result["PATH"].startswith(str(Path("C:/VS/VC/Tools/MSVC/14.50/bin/Hostx64/x64")) + package.os.pathsep))
                    self.assertEqual(result["LIB"], "synthetic-libraries")
                    self.assertNotIn("GH_TOKEN", result)
                    self.assertNotIn("RUSTFLAGS", result)
                    self.assertEqual(run.call_args_list[1].args[0], ["cmd.exe", "/d", "/u", "/c", "environment.cmd"])
                    self.assertEqual(run.call_args_list[1].args[2]["ARG_MSVC_SETUP"], str(Path("C:/Program Files/VS/VC/Auxiliary/Build/vcvars64.bat")))

    def test_pe_inspection_uses_installed_dumpbin_and_records_external_dlls(self):
        raw = [b"Microsoft (R) COFF/PE Dumper Version 14.50.1\n8664 machine (x64)\n20B magic # (PE32+)\n", b"  KERNEL32.dll\n  VCRUNTIME140.dll\n"]
        with patch.object(package, "run", side_effect=raw) as run:
            value = package.dynamic_linkage(Path("gateway.exe"), "x86_64-pc-windows-msvc", {"VCTOOLSINSTALLDIR":"C:/VS/VC/Tools/MSVC/14.50"})
            self.assertEqual(value["libraries"], ["kernel32.dll", "vcruntime140.dll"])
            self.assertEqual(value["inspection_tool"]["version"], "14.50.1")
            self.assertIn("/HEADERS", run.call_args_list[0].args[0])
            self.assertIn("/DEPENDENTS", run.call_args_list[1].args[0])
            self.assertEqual(run.call_args_list[0].args[0][0], Path("C:/VS/VC/Tools/MSVC/14.50/bin/Hostx64/x64/dumpbin.exe"))

    def test_macos_minimum_version_excludes_dylib_versions(self):
        raw = "cmd LC_BUILD_VERSION\nminos 11.0\nsdk 15.5\ntools 1\ntool LD\nversion 1267.0\ncmd LC_SOURCE_VERSION\nversion 0.0\ncmd LC_ID_DYLIB\nversion 1267.0"
        self.assertEqual(package.macos_versions(raw), ["11.0"])
        with self.assertRaises(package.PackageError):
            package.macos_versions("cmd LC_ID_DYLIB\nversion 1267.0")

    def test_manifest_cannot_substitute_uncommitted_packaging_tool_bytes(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary); fixture(directory)
            value = json.loads((directory/"candidate.json").read_text())
            value["packaging_tools"]["release_package.py"] = "f"*64
            (directory/"candidate.json").write_bytes(package.encoded(value))
            hashes = {p.name:package.sha(package.read(p)) for p in directory.iterdir() if p.name != "SHA256SUMS"}
            (directory/"SHA256SUMS").write_bytes("".join(f"{v}  {n}\n" for n,v in sorted(hashes.items())).encode())
            with self.assertRaises(package.PackageError):
                package.verify_candidate(directory)

    def test_candidate_rejects_reserved_members_even_with_matching_checksums(self):
        with tempfile.TemporaryDirectory() as temporary:
            files = {"agent-response-gateway/bin/agent-response-gateway":(b"synthetic executable",0o755),"agent-response-gateway/.private/record":(b"synthetic",0o644)}
            fixture(Path(temporary), files)
            with self.assertRaises(package.check_public_boundary.BoundaryError):
                package.verify_candidate(Path(temporary))


if __name__ == "__main__":
    unittest.main()
