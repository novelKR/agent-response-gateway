#!/usr/bin/env python3
"""Verify signed candidate distributions and promote identical bytes as a preview."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tarfile
import tempfile

import release_package as package

ROOT = Path(__file__).resolve().parents[1]
REPOSITORY = "novelKR/agent-response-gateway"
WORKFLOW = ".github/workflows/release-candidate.yml"
SCHEMA = "gateway-distribution/v1"
PROMOTION = "gateway-preview-promotion/v1"
require = package.require


def commit_sha(value):
    require(isinstance(value, str) and re.fullmatch(r"[0-9a-f]{40}", value), "invalid source commit")
    return value


def run_number(value):
    require(re.fullmatch(r"[1-9][0-9]*", str(value)), "invalid workflow run identity")
    return int(value)


class Github:
    def api(self, endpoint, method="GET", data=None, missing=False):
        args = ["gh", "api", "--include", "--method", method, "-H", "X-GitHub-Api-Version: 2026-03-10", endpoint]
        if data is not None:
            args += ["--input", "-"]
        result = subprocess.run(args, input=package.encoded(data) if data is not None else None, capture_output=True, timeout=60)
        # --include keeps the HTTP status distinct from permission/transport failures.
        parts = result.stdout.split(b"\r\n\r\n", 1)
        if len(parts) != 2:
            parts = result.stdout.split(b"\n\n", 1)
        require(len(parts) == 2, "GitHub did not return an HTTP response")
        status = re.match(rb"HTTP/\S+ (\d{3})", parts[0])
        require(status is not None, "GitHub status missing")
        code = int(status[1])
        if code == 404 and missing:
            return None
        require(result.returncode == 0 and 200 <= code < 300, "GitHub operation failed")
        return package.json_value(parts[1]) if parts[1].strip() else None

    def upload(self, tag, path):
        package.run(["gh", "release", "upload", tag, path, "--repo", REPOSITORY], ROOT, timeout=120)

    def verify(self, path, proof, commit, run_id, attempt):
        raw = package.run(["gh", "attestation", "verify", path, "--bundle", proof,
            "--repo", REPOSITORY, "--signer-workflow", REPOSITORY + "/" + WORKFLOW,
            "--source-ref", "refs/heads/main", "--source-digest", commit, "--signer-digest", commit,
            "--deny-self-hosted-runners", "--format", "json"], ROOT, timeout=120)
        values = package.json_value(raw)
        expected = f"https://github.com/{REPOSITORY}/actions/runs/{run_number(run_id)}/attempts/{run_number(attempt)}"
        require(isinstance(values, list) and any(v["verificationResult"]["statement"]["predicate"]["runDetails"]["metadata"]["invocationId"] == expected for v in values), "attestation invocation differs from candidate run")


def validate_run(value, commit, workflow=WORKFLOW, event="workflow_dispatch"):
    require(value["head_sha"] == commit_sha(commit) and value["head_branch"] == "main", "workflow source is not the selected main commit")
    require(value["path"] == workflow and value["event"] == event and value["status"] == "completed" and value["conclusion"] == "success", "workflow did not complete the required contract")
    require(value["repository"]["full_name"] == REPOSITORY and value["head_repository"]["full_name"] == REPOSITORY, "workflow repository differs")
    run_number(value["id"]); run_number(value["run_attempt"])
    return value


def require_main_ci(github, commit):
    commit_sha(commit)
    require(os.environ.get("GITHUB_REPOSITORY") == REPOSITORY and os.environ.get("GITHUB_REF") == "refs/heads/main" and os.environ.get("GITHUB_SHA") == commit and os.environ.get("GITHUB_RUN_ATTEMPT") == "1", "candidate must use a fresh run of its selected main commit")
    values = github.api(f"repos/{REPOSITORY}/actions/workflows/ci.yml/runs?head_sha={commit}&event=push&branch=main&per_page=10")["workflow_runs"]
    require(values, "main CI has not run for the selected commit")
    selected = max(values, key=lambda v: v["id"])
    return validate_run(selected, commit, ".github/workflows/ci.yml", "push")


def pack_distribution(candidate, output):
    manifest = package.verify_candidate(candidate)
    require(not output.exists(), "distribution destination must not exist")
    target, version = manifest["target"], manifest["version"]
    filename = f"agent-response-gateway-{version}-{target}-distribution.tar.gz"
    files = {"candidate/" + p.name:(package.read(p),0o644) for p in candidate.iterdir()}
    output.mkdir(parents=True)
    archive = package.tar_bytes(files)
    package.write_new(output/filename, archive)
    descriptor = {"schema":SCHEMA, "source_commit":manifest["source_commit"], "target":target, "version":version,
        "filename":filename, "sha256":package.sha(archive), "candidate_sha256":package.sha(package.read(candidate/"candidate.json")), "cargo_lock_sha256":manifest["cargo_lock_sha256"]}
    package.write_new(output/f"{target}.manifest.json", package.encoded(descriptor))
    return descriptor


def inspect_distribution(directory, commit, target, state_dir):
    commit_sha(commit)
    require(target in package.TARGETS, "unsupported distribution target")
    descriptor_path = directory/f"{target}.manifest.json"
    descriptor = package.json_value(package.read(descriptor_path))
    require(set(descriptor) == {"schema","source_commit","target","version","filename","sha256","candidate_sha256","cargo_lock_sha256"}, "invalid distribution descriptor")
    require(descriptor["schema"] == SCHEMA and descriptor["source_commit"] == commit and descriptor["target"] == target, "distribution binding differs")
    name = descriptor["filename"]
    require(package.relative_name(name).name == name and name == f"agent-response-gateway-{descriptor['version']}-{target}-distribution.tar.gz", "invalid distribution filename")
    require(package.sha(package.read(directory/name)) == descriptor["sha256"], "distribution digest differs")
    files = package.archive_files(directory/name)
    require(files and all(n.startswith("candidate/") and len(n.split("/")) == 2 and mode == 0o644 for n,(_,mode) in files.items()), "invalid distribution membership")
    state_dir.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="inspect-", dir=state_dir) as temporary:
        temporary = Path(temporary)
        for name,(data,_) in files.items():
            package.write_new(temporary/name, data)
        candidate = temporary/"candidate"
        manifest = package.verify_candidate(candidate, commit, target)
        require(package.sha(package.read(candidate/"candidate.json")) == descriptor["candidate_sha256"] and manifest["version"] == descriptor["version"] and manifest["cargo_lock_sha256"] == descriptor["cargo_lock_sha256"], "inner candidate binding differs")
    return descriptor


def verify_distribution(github, directory, commit, target, run_id, attempt, state_dir):
    descriptor = inspect_distribution(directory, commit, target, state_dir)
    proof = directory/f"{target}.sigstore.jsonl"
    names = {descriptor["filename"], f"{target}.manifest.json", proof.name}
    require({p.name for p in directory.iterdir()} == names, "signed distribution includes unexpected files")
    package.read(proof)
    for name in [descriptor["filename"], f"{target}.manifest.json"]:
        github.verify(directory/name, proof, commit, run_id, attempt)
    return descriptor


def preview_tag(version, tag, track):
    require(track == "preview", "operational promotion requires the future verified G17 acceptance contract")
    require(re.fullmatch(re.escape("v" + version) + r"-preview\.[1-9][0-9]*", tag), "preview tag must match the candidate version")


def prepare_promotion(github, directory, commit, run_id, tag, track, output, state_dir):
    require(track == "preview", "operational promotion requires the future verified G17 acceptance contract")
    run_id = run_number(run_id)
    run = validate_run(github.api(f"repos/{REPOSITORY}/actions/runs/{run_id}"), commit)
    require(run["id"] == run_id, "candidate run identity differs")
    require({p.name for p in directory.iterdir()} == {"release-candidate-" + t for t in package.TARGETS}, "candidate target set differs")
    descriptors, assets = [], {}
    for target in sorted(package.TARGETS):
        folder = directory/("release-candidate-" + target)
        descriptor = verify_distribution(github, folder, commit, target, run_id, run["run_attempt"], state_dir)
        descriptors.append(descriptor)
        for path in folder.iterdir():
            require(path.name not in assets, "duplicate promotion asset name")
            assets[path.name] = package.sha(package.read(path))
    require(len({d["version"] for d in descriptors}) == len({d["cargo_lock_sha256"] for d in descriptors}) == 1, "candidate targets have different source versions")
    preview_tag(descriptors[0]["version"], tag, track)
    require(not output.exists(), "promotion destination must not exist")
    output.mkdir(parents=True)
    for path in directory.glob("*/*"):
        package.write_new(output/path.name, package.read(path))
    receipt = {"schema":PROMOTION, "repository":REPOSITORY, "source_commit":commit, "candidate_run_id":run_id, "candidate_run_attempt":run["run_attempt"],
        "version":descriptors[0]["version"], "tag":tag, "track":track, "assets":dict(sorted(assets.items()))}
    package.write_new(output/"promotion.json", package.encoded(receipt))
    return receipt


def verify_promotion(github, directory, state_dir):
    receipt = package.json_value(package.read(directory/"promotion.json"))
    require(set(receipt) == {"schema","repository","source_commit","candidate_run_id","candidate_run_attempt","version","tag","track","assets"}, "invalid promotion receipt")
    require(receipt["schema"] == PROMOTION and receipt["repository"] == REPOSITORY, "invalid promotion scope")
    preview_tag(receipt["version"], receipt["tag"], receipt["track"])
    require({p.name for p in directory.iterdir()} == {*receipt["assets"], "promotion.json"}, "promotion asset set differs")
    for name,digest in receipt["assets"].items():
        require(package.relative_name(name).name == name and package.sha(package.read(directory/name)) == digest, "promotion bytes differ")
    run = validate_run(github.api(f"repos/{REPOSITORY}/actions/runs/{run_number(receipt['candidate_run_id'])}"), receipt["source_commit"])
    require(run["run_attempt"] == receipt["candidate_run_attempt"], "candidate run was replaced by a later attempt")
    # Isolate each already signed target's exact three files for the same strict verifier.
    state_dir.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="promote-", dir=state_dir) as temporary:
        for target in sorted(package.TARGETS):
            folder = Path(temporary)/target; folder.mkdir()
            descriptor = package.json_value(package.read(directory/f"{target}.manifest.json"))
            for name in [descriptor["filename"], f"{target}.manifest.json", f"{target}.sigstore.jsonl"]:
                require(package.relative_name(name).name == name and name in receipt["assets"], "missing signed promotion asset")
                package.write_new(folder/name, package.read(directory/name))
            verified = verify_distribution(github, folder, receipt["source_commit"], target, receipt["candidate_run_id"], receipt["candidate_run_attempt"], state_dir)
            require(verified["version"] == receipt["version"], "promotion package version differs")
    return receipt


def release_body(receipt):
    return ("Limited preview of the verified gateway implementation. No live provider or consumer operational qualification is claimed.\n\n"
        + "Source commit: `" + receipt["source_commit"] + "`.\n"
        + "Candidate run: " + f"https://github.com/{REPOSITORY}/actions/runs/{receipt['candidate_run_id']}" + ".\n"
        + "Promotion receipt SHA-256: `" + package.sha(package.encoded(receipt)) + "`.\n\n"
        + "The retained distribution bundles contain the exact verified binaries, corresponding source, notices and scoped SBOMs. Verify the accompanying Sigstore proofs before adopting them. No artifact was rebuilt during promotion.\n")


def validate_release(value, receipt):
    require(value["tag_name"] == receipt["tag"] and value["target_commitish"] == receipt["source_commit"] and value["prerelease"] is True and value["name"] == receipt["tag"] and value["body"] == release_body(receipt), "existing release belongs to a different promotion")
    found = {}
    for asset in value["assets"]:
        name = asset["name"]
        require(name not in found and name in receipt["assets"] and asset["state"] == "uploaded" and asset.get("digest") == "sha256:" + receipt["assets"][name], "existing release asset differs; no overwrite is permitted")
        found[name] = asset["digest"]
    if not value["draft"]:
        require(set(found) == set(receipt["assets"]), "published release is incomplete")
    return set(found)


def validate_tag(github, tag, commit, required=False):
    ref = github.api(f"repos/{REPOSITORY}/git/ref/tags/{tag}", missing=True)
    if ref is None:
        require(not required, "published release tag is missing")
        return
    obj = ref["object"]
    seen = set()
    while obj["type"] == "tag":
        require(obj["sha"] not in seen and len(seen) < 8, "invalid release tag chain")
        seen.add(obj["sha"])
        obj = github.api(f"repos/{REPOSITORY}/git/tags/{commit_sha(obj['sha'])}")["object"]
    require(obj["type"] == "commit" and obj["sha"] == commit, "release tag points to a different source")


def publish_preview(github, directory, receipt):
    # This function is called only after protected environment approval and re-verification.
    preview_tag(receipt["version"], receipt["tag"], receipt["track"])
    main = github.api(f"repos/{REPOSITORY}/git/ref/heads/main")
    require(main["object"]["type"] == "commit" and main["object"]["sha"] == receipt["source_commit"], "main changed after candidate approval; prepare a new candidate without rebuilding this one")
    validate_tag(github, receipt["tag"], receipt["source_commit"])
    endpoint = f"repos/{REPOSITORY}/releases/tags/{receipt['tag']}"
    release = github.api(endpoint, missing=True)
    if release is None:
        release = github.api(f"repos/{REPOSITORY}/releases", "POST", {"tag_name":receipt["tag"], "target_commitish":receipt["source_commit"], "name":receipt["tag"], "body":release_body(receipt), "draft":True, "prerelease":True, "make_latest":"false"})
    present = validate_release(release, receipt)
    if not release["draft"]:
        validate_tag(github, receipt["tag"], receipt["source_commit"], required=True)
        return "already_published"
    for name in sorted(set(receipt["assets"]) - present):
        require(package.sha(package.read(directory/name)) == receipt["assets"][name], "promotion bytes changed before upload")
        github.upload(receipt["tag"], directory/name)
    release = github.api(endpoint)
    require(validate_release(release, receipt) == set(receipt["assets"]), "uploaded release asset set is incomplete")
    validate_tag(github, receipt["tag"], receipt["source_commit"])
    github.api(f"repos/{REPOSITORY}/releases/{run_number(release['id'])}", "PATCH", {"draft":False, "make_latest":"false"})
    final = github.api(endpoint)
    require(final["draft"] is False and validate_release(final, receipt) == set(receipt["assets"]), "published release readback differs")
    validate_tag(github, receipt["tag"], receipt["source_commit"], required=True)
    return "published"


def require_release_environment(github):
    value = github.api(f"repos/{REPOSITORY}/environments/release")
    reviewers = [r for r in value["protection_rules"] if r["type"] == "required_reviewers"]
    require(len(reviewers) == 1 and len(reviewers[0]["reviewers"]) == 1, "release environment must require the designated user")
    reviewer = reviewers[0]["reviewers"][0]
    require(reviewer["type"] == "User" and reviewer["reviewer"]["login"] == "novelKR", "release environment reviewer differs")
    require(value["deployment_branch_policy"] == {"protected_branches":True,"custom_branch_policies":False}, "release environment protection differs")
    # GitHub enforces the environment gate. Its actual review history must also bind this run.
    reviews = github.api(f"repos/{REPOSITORY}/actions/runs/{run_number(os.environ.get('GITHUB_RUN_ID'))}/approvals")
    require(any(r["state"] == "approved" and r["user"]["login"] == "novelKR" and any(e["id"] == value["id"] and e["name"] == "release" for e in r["environments"]) for r in reviews), "explicit user approval of this release run is missing")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["gate", "pack", "inspect", "verify", "prepare-promotion", "publish"])
    parser.add_argument("--directory", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--commit")
    parser.add_argument("--target", choices=sorted(package.TARGETS))
    parser.add_argument("--run-id")
    parser.add_argument("--attempt")
    parser.add_argument("--tag")
    parser.add_argument("--track", choices=["preview", "operational"], default="preview")
    args = parser.parse_args()
    github, state = Github(), ROOT/".local/provenance-state"
    try:
        if args.command == "gate":
            require_main_ci(github, args.commit)
        elif args.command == "pack":
            pack_distribution(args.directory, args.output)
        elif args.command == "inspect":
            inspect_distribution(args.directory, args.commit, args.target, state)
        elif args.command == "verify":
            verify_distribution(github, args.directory, args.commit, args.target, args.run_id, args.attempt, state)
        elif args.command == "prepare-promotion":
            prepare_promotion(github, args.directory, args.commit, args.run_id, args.tag, args.track, args.output, state)
        else:
            require(os.environ.get("GITHUB_REPOSITORY") == REPOSITORY and os.environ.get("GITHUB_REF") == "refs/heads/main" and os.environ.get("ARG_RELEASE_APPROVED_ENVIRONMENT") == "release" and os.environ.get("GITHUB_RUN_ATTEMPT") == "1", "publish must use a fresh protected release run")
            require_release_environment(github)
            receipt = verify_promotion(github, args.directory, state)
            publish_preview(github, args.directory, receipt)
    except (OSError, ValueError, KeyError, TypeError, AttributeError, package.PackageError, package.license_audit.AuditError, package.check_public_boundary.BoundaryError, tarfile.TarError, subprocess.TimeoutExpired):
        print("release-provenance: failed; no implicit retry, overwrite or qualification", file=sys.stderr)
        return 1
    print(json.dumps({"command":args.command,"status":"passed"}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
