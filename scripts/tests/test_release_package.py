import io
import json
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest

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
        for name, data in {"LICENSE":b"synthetic source license", "scripts/release_package.py":b"synthetic builder", "scripts/package_smoke.py":b"synthetic smoke"}.items():
            item = tarfile.TarInfo("agent-response-gateway/" + name)
            item.mode, item.size = 0o644, len(data)
            archive.addfile(item, io.BytesIO(data))
    return raw.getvalue()


def fixture(directory, files=None):
    files = files or {"agent-response-gateway/bin/agent-response-gateway":(b"synthetic executable",0o755), "agent-response-gateway/LICENSE":(b"synthetic license",0o644)}
    binary = files["agent-response-gateway/bin/agent-response-gateway"][0]
    bom = sbom()
    bom["metadata"]["component"]["hashes"] = [{"alg":"SHA-256","content":package.sha(binary)}]
    assets = {"binary.tar.gz":package.tar_bytes(files),"source.tar.gz":source_archive(),"sbom.json":package.encoded(bom)}
    for name,data in assets.items():
        package.write_new(directory/name, data)
    manifest = {"schema":package.SCHEMA,"target":TARGET,"source_commit":COMMIT,"assets":{n:package.sha(v) for n,v in assets.items()},"binary_archive":"binary.tar.gz","source_archive":"source.tar.gz","sbom":"sbom.json","binary_sha256":package.sha(binary),"package_files":{n:{"sha256":package.sha(v),"mode":m} for n,(v,m) in files.items()}, "packaging_tools":{"release_package.py":package.sha(b"synthetic builder"),"package_smoke.py":package.sha(b"synthetic smoke")}}
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

    def test_macos_minimum_version_excludes_dylib_versions(self):
        raw = "cmd LC_BUILD_VERSION\nminos 11.0\nsdk 15.5\ncmd LC_SOURCE_VERSION\nversion 0.0\ncmd LC_ID_DYLIB\nversion 1267.0"
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
