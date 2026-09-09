"""Synthetic provenance/promotion contracts; never invokes real GitHub writes."""
import base64
import copy
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import release_provenance as provenance
import release_package as package
import test_release_package as fixtures

COMMIT = fixtures.COMMIT


def successful_run():
    return {"id":123,"run_attempt":1,"head_sha":COMMIT,"head_branch":"v0.1.0","path":provenance.WORKFLOW,"event":"push","status":"completed","conclusion":"success","repository":{"full_name":provenance.REPOSITORY},"head_repository":{"full_name":provenance.REPOSITORY}}


def candidate(directory, target):
    directory.mkdir(parents=True)
    fixtures.fixture(directory, target=target)
    manifest = json.loads((directory/"candidate.json").read_text())
    manifest.update(target=target, version="0.1.0", cargo_lock_sha256="c"*64)
    (directory/"candidate.json").write_bytes(package.encoded(manifest))
    hashes = {p.name:package.sha(package.read(p)) for p in directory.iterdir() if p.name != "SHA256SUMS"}
    (directory/"SHA256SUMS").write_bytes("".join(f"{v}  {n}\n" for n,v in sorted(hashes.items())).encode())


class FakeGithub:
    def __init__(self):
        self.run = successful_run()
        self.release = None
        self.tag = COMMIT
        self.main = COMMIT
        self.writes, self.uploaded, self.verified = [], [], []
        self.fail_after_upload = False
        self.downloads = {}
        self.version = "0.1.0"
        self.ci_conclusion = "success"
        self.ancestor = True
        self.environment = {"id":1,"protection_rules":[{"type":"required_reviewers","reviewers":[{"type":"User","reviewer":{"login":"novelKR"}}]}],"deployment_branch_policy":{"protected_branches":True,"custom_branch_policies":False}}
        self.reviews = [{"state":"approved","user":{"login":"novelKR"},"environments":[{"id":1,"name":"release"}]}]

    def api(self, endpoint, method="GET", data=None, missing=False):
        if method != "GET":
            self.writes.append((method, endpoint, copy.deepcopy(data)))
        if "/compare/main..." in endpoint:
            return {"merge_base_commit":{"sha":COMMIT if self.ancestor else "2"*40}, "status":"identical" if self.main == COMMIT else "behind"}
        if "/contents/Cargo.toml?ref=" in endpoint:
            return {"encoding":"base64", "content":base64.b64encode(('[package]\nversion="'+self.version+'"\n').encode()).decode()}
        if "/actions/workflows/ci.yml/runs?" in endpoint:
            return {"workflow_runs":[{**successful_run(), "head_branch":"main", "path":".github/workflows/ci.yml", "conclusion":self.ci_conclusion}]}
        if endpoint.endswith("/git/ref/heads/main"):
            return {"object":{"type":"commit","sha":self.main}}
        if "/git/ref/tags/" in endpoint:
            return None if self.tag is None else {"object":{"type":"commit","sha":self.tag}}
        if endpoint.endswith("/environments/release"):
            return self.environment
        if endpoint.endswith("/approvals"):
            return self.reviews
        if "/actions/runs/" in endpoint:
            return copy.deepcopy(self.run)
        if method == "POST" and endpoint.endswith("/releases"):
            self.release = {**copy.deepcopy(data), "id":99,"assets":[]}
        elif method == "PATCH" and endpoint.endswith("/releases/99"):
            self.release.update(data); self.tag = self.release["target_commitish"]
        if "/releases" in endpoint:
            return copy.deepcopy(self.release)
        raise AssertionError("unexpected synthetic GitHub call")

    def verify(self, path, proof, commit, run_id, attempt, tag):
        package.require(package.read(proof) == b"synthetic-proof", "synthetic proof rejected")
        self.verified.append((path.name, commit, run_id, attempt, tag))

    def upload(self, tag, path):
        self.uploaded.append(path.name)
        ident = len(self.release["assets"]) + 1
        self.downloads[ident] = package.read(path)
        self.release["assets"].append({"id":ident,"name":path.name,"size":len(package.read(path)),"state":"uploaded","digest":"sha256:"+package.sha(package.read(path))})
        if self.fail_after_upload:
            self.fail_after_upload = False
            raise package.PackageError("synthetic lost upload acknowledgement")

    def download(self, asset_id, path):
        package.write_new(path, self.downloads[asset_id])


def prepared(root, github):
    candidates = root/"signed"; candidates.mkdir()
    for target in package.TARGETS:
        source = root/("source-"+target); candidate(source, target)
        output = candidates/("release-candidate-"+target)
        provenance.pack_distribution(source, output)
        (output/f"{target}.sigstore.jsonl").write_bytes(b"synthetic-proof")
    output = root/"promotion"
    receipt = provenance.prepare_publication(github, candidates, 123, output, root/"state")
    return output, receipt


class ReleaseProvenanceTests(unittest.TestCase):
    def test_candidate_requires_existing_tag_main_ancestry_exact_version_and_successful_ci(self):
        env = {"GITHUB_REPOSITORY":provenance.REPOSITORY,"GITHUB_REF":"refs/tags/v0.1.0","GITHUB_SHA":COMMIT,"GITHUB_RUN_ATTEMPT":"1","GITHUB_EVENT_NAME":"push"}
        github = FakeGithub()
        with patch.dict(os.environ,env):
            self.assertEqual(provenance.candidate_gate(github)["commit"],COMMIT)
            for field,replacement in [("GITHUB_SHA","2"*40),("GITHUB_REF","refs/heads/main"),("GITHUB_RUN_ATTEMPT","2"),("GITHUB_EVENT_NAME","pull_request")]:
                with patch.dict(os.environ,{field:replacement}), self.assertRaises(package.PackageError):
                    provenance.candidate_gate(github)
            for field,replacement in [("ci_conclusion","failure"),("ancestor",False),("version","0.2.0"),("tag",None)]:
                with patch.object(github,field,replacement), self.assertRaises(package.PackageError):
                    provenance.candidate_gate(github)
            github.main = "2"*40
            provenance.candidate_gate(github)

    def test_only_successful_selected_tag_workflow_is_eligible(self):
        provenance.validate_run(successful_run(), COMMIT, "v0.1.0")
        for change in [{"event":"pull_request"},{"head_branch":"main"},{"head_sha":"2"*40},{"path":".github/workflows/other.yml"},{"status":"in_progress"},{"conclusion":"failure"},{"head_repository":{"full_name":"other/repo"}},{"run_attempt":2}]:
            value = successful_run(); value.update(change)
            with self.assertRaises(package.PackageError):
                provenance.validate_run(value, COMMIT, "v0.1.0")

    def test_unsigned_or_modified_distribution_is_rejected_before_promotion(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); github = FakeGithub()
            source=root/"candidate"; candidate(source, fixtures.TARGET)
            descriptor = provenance.pack_distribution(source, root/"distribution")
            provenance.inspect_distribution(root/"distribution", COMMIT, fixtures.TARGET, root/"state")
            with self.assertRaises(package.PackageError):
                provenance.verify_distribution(github, root/"distribution", COMMIT, fixtures.TARGET, 123, 1, root/"state", "v0.1.0")
            (root/"distribution"/descriptor["filename"]).write_bytes(b"tampered")
            with self.assertRaises(package.PackageError):
                provenance.inspect_distribution(root/"distribution", COMMIT, fixtures.TARGET, root/"state")
            self.assertFalse(github.writes)

    def test_strict_signature_command_binds_repository_workflow_source_runner_and_attempt(self):
        value = [{"verificationResult":{"statement":{"predicate":{"runDetails":{"metadata":{"invocationId":f"https://github.com/{provenance.REPOSITORY}/actions/runs/123/attempts/1"}}}}}}]
        with patch.object(package,"run",return_value=package.encoded(value)) as run:
            provenance.Github().verify(Path("asset"),Path("proof"),COMMIT,123,1,"v0.1.0")
            args = run.call_args.args[0]
            for flag in ["--repo","--signer-workflow","--source-ref","--source-digest","--signer-digest","--deny-self-hosted-runners"]:
                self.assertIn(flag,args)
            self.assertEqual(args[args.index("--source-digest")+1],COMMIT)
            self.assertEqual(args[args.index("--source-ref")+1],"refs/tags/v0.1.0")
            with self.assertRaises(package.PackageError):
                provenance.Github().verify(Path("asset"),Path("proof"),COMMIT,124,1,"v0.1.0")

    def test_four_targets_are_verified_before_preparing_and_again_before_publishing(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); github = FakeGithub(); output,receipt = prepared(root,github)
            self.assertEqual(len(github.verified),8)
            self.assertEqual(provenance.verify_publication(github,output,root/"state"),receipt)
            self.assertEqual(len(github.verified),16)
            self.assertFalse(github.writes)
            github.run["run_attempt"] = 2
            with self.assertRaises(package.PackageError):
                provenance.verify_publication(github,output,root/"state")

    def test_preview_publish_preserves_bytes_and_repeated_completion_does_not_upload_again(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); github = FakeGithub(); output,receipt = prepared(root,github)
            self.assertEqual(provenance.publish_prerelease(github,output,receipt),"published")
            self.assertEqual(set(github.uploaded),set(provenance.release_assets(receipt)))
            count = len(github.uploaded)
            self.assertEqual(provenance.publish_prerelease(github,output,receipt),"already_published")
            self.assertEqual(len(github.uploaded),count)
            self.assertFalse(github.release["draft"])
            self.assertTrue(github.release["prerelease"])
            self.assertEqual(github.release["make_latest"],"false")

    def test_unknown_upload_outcome_keeps_draft_and_resumes_only_missing_matching_assets(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); github = FakeGithub(); output,receipt = prepared(root,github)
            github.fail_after_upload = True
            with self.assertRaises(package.PackageError):
                provenance.publish_prerelease(github,output,receipt)
            self.assertTrue(github.release["draft"])
            first = github.uploaded[0]
            self.assertEqual(provenance.publish_prerelease(github,output,receipt),"published")
            self.assertEqual(github.uploaded.count(first),1)
            self.assertEqual(len([w for w in github.writes if w[0] == "POST"]),1)

    def test_wrong_main_tag_or_existing_asset_never_overwrites_a_release(self):
        for field in ["tag","asset"]:
            with self.subTest(field=field), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary); github = FakeGithub(); output,receipt = prepared(root,github)
                if field == "asset":
                    github.fail_after_upload = True
                    with self.assertRaises(package.PackageError):
                        provenance.publish_prerelease(github,output,receipt)
                    github.release["assets"][0]["digest"] = "sha256:"+"0"*64
                else:
                    setattr(github,field,"2"*40)
                count = len(github.uploaded)
                with self.assertRaises(package.PackageError):
                    provenance.publish_prerelease(github,output,receipt)
                self.assertEqual(len(github.uploaded),count)

    def test_tags_bind_exact_semver_and_only_stable_versions_can_be_promoted(self):
        for tag,version in [("v0.1.0","0.1.0"),("v1.2.3-rc.1","1.2.3-rc.1"),("v1.2.3-preview.1","1.2.3-preview.1")]:
            self.assertEqual(provenance.version_tag(tag,version),tag)
        for tag,version in [("v0.1.0-preview.1","0.1.0"),("v01.1.0","01.1.0"),("v1.2.3-01","1.2.3-01"),("v1.2.3+build","1.2.3+build"),("../escape",None)]:
            with self.assertRaises(package.PackageError):
                provenance.version_tag(tag,version)
        with self.assertRaises(package.PackageError):
            provenance.version_tag("v1.2.3-rc.1",stable=True)

    def test_missing_target_and_mixed_run_proofs_cannot_prepare_publication(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); github = FakeGithub(); output,receipt = prepared(root,github)
            signed = root/"signed"
            folder = signed/("release-candidate-"+next(iter(package.TARGETS)))
            folder.rename(root/"removed-target")
            with self.assertRaises(package.PackageError):
                provenance.prepare_publication(github,signed,123,root/"other",root/"state")
            (output/(next(iter(package.TARGETS))+".sigstore.jsonl")).write_bytes(b"other-run-proof")
            with self.assertRaises(package.PackageError):
                provenance.verify_publication(github,output,root/"state")
            self.assertFalse(github.writes)

    def test_formal_promotion_requires_approval_and_preserves_release_id_tag_and_every_asset(self):
        env = {"GITHUB_REPOSITORY":provenance.REPOSITORY,"GITHUB_REF":"refs/heads/main","GITHUB_EVENT_NAME":"workflow_dispatch","ARG_RELEASE_APPROVED_ENVIRONMENT":"release","GITHUB_RUN_ID":"123","GITHUB_RUN_ATTEMPT":"1"}
        with tempfile.TemporaryDirectory() as temporary, patch.dict(os.environ,env):
            root = Path(temporary); github = FakeGithub(); output,receipt = prepared(root,github)
            provenance.publish_prerelease(github,output,receipt)
            before = copy.deepcopy(github.release)
            downloaded = root/"public-assets"
            provenance.download_release(github,"v0.1.0",downloaded,root/"state")
            digest = package.sha(package.encoded(receipt))
            github.reviews = []
            with self.assertRaises(package.PackageError):
                provenance.promote_release(github,downloaded,root/"state",digest)
            self.assertEqual(github.release,before)
            github.reviews = [{"state":"approved","user":{"login":"novelKR"},"environments":[{"id":1,"name":"release"}]}]
            with patch.dict(os.environ,{"GITHUB_RUN_ATTEMPT":"2"}), self.assertRaises(package.PackageError):
                provenance.promote_release(github,downloaded,root/"state",digest)
            with self.assertRaises(package.PackageError):
                provenance.promote_release(github,downloaded,root/"state","0"*64)
            github.main = "2"*40
            self.assertEqual(provenance.promote_release(github,downloaded,root/"state",digest),"promoted")
            self.assertEqual(github.release,{**before,"prerelease":False,"make_latest":"legacy"})
            count = len(github.writes)
            self.assertEqual(provenance.promote_release(github,downloaded,root/"state",digest),"already_promoted")
            self.assertEqual(len(github.writes),count)
            self.assertEqual(provenance.publish_prerelease(github,output,receipt),"already_published")
            self.assertFalse(github.release["prerelease"])

    def test_public_release_download_rejects_changed_bytes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); github = FakeGithub(); output,receipt = prepared(root,github)
            provenance.publish_prerelease(github,output,receipt)
            github.downloads[1] = b"tampered"
            with self.assertRaises(package.PackageError):
                provenance.download_release(github,"v0.1.0",root/"downloads",root/"state")

    def test_publish_requires_configured_reviewer_and_actual_approval_for_this_run(self):
        github = FakeGithub()
        with patch.dict(os.environ,{"GITHUB_RUN_ID":"123"}):
            provenance.require_release_environment(github)
            github.reviews = []
            with self.assertRaises(package.PackageError):
                provenance.require_release_environment(github)
            github.reviews = [{"state":"approved","user":{"login":"different"},"environments":[{"id":1,"name":"release"}]}]
            with self.assertRaises(package.PackageError):
                provenance.require_release_environment(github)
        self.assertFalse(github.writes)


if __name__ == "__main__":
    unittest.main()
