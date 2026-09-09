#!/usr/bin/env python3
"""Verify tagged builds, publish their exact bytes, and approve release state promotion."""
from __future__ import annotations

import argparse
import base64
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import zipfile

import release_package as package

ROOT = Path(__file__).resolve().parents[1]
REPOSITORY = "novelKR/agent-response-gateway"
WORKFLOW = ".github/workflows/release-candidate.yml"
SCHEMA = "gateway-distribution/v1"
PROMOTION = "gateway-release-manifest/v1"
MANIFEST = "release-manifest.json"
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

    def download(self, asset_id, path):
        result = subprocess.run(["gh", "api", f"repos/{REPOSITORY}/releases/assets/{run_number(asset_id)}", "-H", "Accept: application/octet-stream"], capture_output=True, timeout=120)
        require(result.returncode == 0 and len(result.stdout) <= package.MAX_FILE, "release download failed or exceeded size limit")
        package.write_new(path, result.stdout)

    def verify(self, path, proof, commit, run_id, attempt, tag):
        raw = package.run(["gh", "attestation", "verify", path, "--bundle", proof,
            "--repo", REPOSITORY, "--signer-workflow", REPOSITORY + "/" + WORKFLOW,
            "--source-ref", "refs/tags/" + version_tag(tag), "--source-digest", commit, "--signer-digest", commit,
            "--deny-self-hosted-runners", "--format", "json"], ROOT, timeout=120)
        values = package.json_value(raw)
        expected = f"https://github.com/{REPOSITORY}/actions/runs/{run_number(run_id)}/attempts/{run_number(attempt)}"
        require(isinstance(values, list) and any(v["verificationResult"]["statement"]["predicate"]["runDetails"]["metadata"]["invocationId"] == expected for v in values), "attestation invocation differs from candidate run")


def version_tag(tag, version=None, stable=False):
    number = r"(?:0|[1-9][0-9]*)"
    identifier = r"(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)"
    suffix = r"(?:-" + identifier + r"(?:\." + identifier + r")*)?"
    require(isinstance(tag, str) and re.fullmatch("v" + number + r"\." + number + r"\." + number + ("" if stable else suffix), tag), "invalid release version tag")
    require(version is None or tag == "v" + version, "tag differs from Cargo package version")
    return tag


def validate_run(value, commit, tag=None):
    require(value["head_sha"] == commit_sha(commit), "workflow source differs")
    require(value["status"] == "completed" and value["conclusion"] == "success", "workflow did not succeed")
    require(value["repository"]["full_name"] == REPOSITORY and value["head_repository"]["full_name"] == REPOSITORY, "workflow repository differs")
    if tag is None:
        require(value["path"] == ".github/workflows/ci.yml" and value["event"] == "push" and value["head_branch"] == "main", "selected source lacks main push CI")
    else:
        require(value["path"] == WORKFLOW and value["event"] in {"push", "workflow_dispatch"} and value["head_branch"] == version_tag(tag), "candidate is not a version tag run")
        require(value["run_attempt"] == 1, "candidate rebuild attempts cannot replace signed inputs")
    run_number(value["id"]); run_number(value["run_attempt"])
    return value


def require_main_ci(github, commit):
    commit_sha(commit)
    comparison = github.api(f"repos/{REPOSITORY}/compare/main...{commit}")
    require(comparison["merge_base_commit"]["sha"] == commit and comparison["status"] in {"identical", "behind"}, "source commit is not contained in main")
    values = github.api(f"repos/{REPOSITORY}/actions/workflows/ci.yml/runs?head_sha={commit}&event=push&branch=main&per_page=100")["workflow_runs"]
    require(values, "main CI has not run for the selected commit")
    return validate_run(max(values, key=lambda v: v["id"]), commit)


def require_source(github, tag, commit, version=None):
    version_tag(tag, version)
    validate_tag(github, tag, commit)
    require_main_ci(github, commit)
    cargo = github.api(f"repos/{REPOSITORY}/contents/Cargo.toml?ref={commit}")
    require(cargo["encoding"] == "base64", "source package manifest encoding differs")
    source_version = tomllib.loads(base64.b64decode(cargo["content"]).decode())["package"]["version"]
    version_tag(tag, source_version)
    return source_version


def candidate_gate(github):
    tag = os.environ.get("GITHUB_REF", "").removeprefix("refs/tags/")
    commit = commit_sha(os.environ.get("GITHUB_SHA"))
    require(os.environ.get("GITHUB_REPOSITORY") == REPOSITORY and os.environ.get("GITHUB_REF") == "refs/tags/" + version_tag(tag) and os.environ.get("GITHUB_RUN_ATTEMPT") == "1" and os.environ.get("GITHUB_EVENT_NAME") in {"push", "workflow_dispatch"}, "candidate requires a fresh existing tag run")
    version = require_source(github, tag, commit)
    return {"tag":tag, "commit":commit, "version":version}


def pack_distribution(candidate, output):
    manifest = package.verify_candidate(candidate)
    require(not output.exists(), "distribution destination must not exist")
    target, version = manifest["target"], manifest["version"]
    filename = f"agent-response-gateway-{version}-{target}-distribution.{package.TARGETS[target]['archive']}"
    files = {"candidate/" + p.name:(package.read(p),0o644) for p in candidate.iterdir()}
    output.mkdir(parents=True)
    archive = package.archive_bytes(files, target)
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
    require(package.relative_name(name).name == name and name == f"agent-response-gateway-{descriptor['version']}-{target}-distribution.{package.TARGETS[target]['archive']}", "invalid distribution filename")
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


def verify_distribution(github, directory, commit, target, run_id, attempt, state_dir, tag):
    descriptor = inspect_distribution(directory, commit, target, state_dir)
    proof = directory/f"{target}.sigstore.jsonl"
    names = {descriptor["filename"], f"{target}.manifest.json", proof.name}
    require({p.name for p in directory.iterdir()} == names, "signed distribution includes unexpected files")
    package.read(proof)
    for name in [descriptor["filename"], f"{target}.manifest.json"]:
        github.verify(directory/name, proof, commit, run_id, attempt, tag)
    return descriptor


def prepare_publication(github, directory, run_id, output, state_dir):
    run_id = run_number(run_id)
    run = github.api(f"repos/{REPOSITORY}/actions/runs/{run_id}")
    tag, commit = version_tag(run["head_branch"]), commit_sha(run["head_sha"])
    validate_run(run, commit, tag)
    require(run["id"] == run_id, "candidate run identity differs")
    version = require_source(github, tag, commit)
    require({p.name for p in directory.iterdir()} == {"release-candidate-" + t for t in package.TARGETS}, "candidate target set differs")
    descriptors, assets = [], {}
    for target in sorted(package.TARGETS):
        folder = directory/("release-candidate-" + target)
        descriptor = verify_distribution(github, folder, commit, target, run_id, run["run_attempt"], state_dir, tag)
        require(descriptor["version"] == version, "candidate target version differs from source")
        descriptors.append(descriptor)
        for path in folder.iterdir():
            require(path.name not in assets, "duplicate publication asset name")
            assets[path.name] = package.sha(package.read(path))
    require(len({d["cargo_lock_sha256"] for d in descriptors}) == 1, "candidate targets have different source locks")
    require(not output.exists(), "publication destination must not exist")
    output.mkdir(parents=True)
    for path in directory.glob("*/*"):
        package.write_new(output/path.name, package.read(path))
    receipt = {"schema":PROMOTION, "repository":REPOSITORY, "source_commit":commit, "candidate_run_id":run_id, "candidate_run_attempt":run["run_attempt"],
        "version":version, "tag":tag, "targets":sorted(package.TARGETS), "assets":dict(sorted(assets.items()))}
    package.write_new(output/MANIFEST, package.encoded(receipt))
    return receipt


def verify_publication(github, directory, state_dir):
    receipt = package.json_value(package.read(directory/MANIFEST))
    require(set(receipt) == {"schema","repository","source_commit","candidate_run_id","candidate_run_attempt","version","tag","targets","assets"}, "invalid release manifest")
    require(receipt["schema"] == PROMOTION and receipt["repository"] == REPOSITORY and receipt["targets"] == sorted(package.TARGETS), "invalid publication scope")
    version_tag(receipt["tag"], receipt["version"])
    require({p.name for p in directory.iterdir()} == {*receipt["assets"], MANIFEST}, "publication asset set differs")
    for name,digest in receipt["assets"].items():
        require(package.relative_name(name).name == name and package.sha(package.read(directory/name)) == digest, "publication bytes differ")
    run = validate_run(github.api(f"repos/{REPOSITORY}/actions/runs/{run_number(receipt['candidate_run_id'])}"), receipt["source_commit"], receipt["tag"])
    require(run["id"] == receipt["candidate_run_id"] and run["run_attempt"] == receipt["candidate_run_attempt"], "candidate run identity or attempt differs")
    require_source(github, receipt["tag"], receipt["source_commit"], receipt["version"])
    state_dir.mkdir(parents=True, exist_ok=True)
    assets, locks = set(), set()
    with tempfile.TemporaryDirectory(prefix="publish-", dir=state_dir) as temporary:
        for target in sorted(package.TARGETS):
            folder = Path(temporary)/target; folder.mkdir()
            descriptor = package.json_value(package.read(directory/f"{target}.manifest.json"))
            for name in [descriptor["filename"], f"{target}.manifest.json", f"{target}.sigstore.jsonl"]:
                require(package.relative_name(name).name == name and name in receipt["assets"], "missing signed publication asset")
                assets.add(name)
                package.write_new(folder/name, package.read(directory/name))
            verified = verify_distribution(github, folder, receipt["source_commit"], target, receipt["candidate_run_id"], receipt["candidate_run_attempt"], state_dir, receipt["tag"])
            require(verified["version"] == receipt["version"], "release package version differs")
            locks.add(verified["cargo_lock_sha256"])
    require(assets == set(receipt["assets"]) and len(locks) == 1, "release contains mixed or unsigned extra inputs")
    return receipt


def release_assets(receipt):
    return {**receipt["assets"], MANIFEST:package.sha(package.encoded(receipt))}


def release_body(receipt):
    return ("Verified native gateway packages. Live provider and consumer operational qualification are separate.\n\n"
        + "Source commit: `" + receipt["source_commit"] + "`.\n"
        + "Candidate run: " + f"https://github.com/{REPOSITORY}/actions/runs/{receipt['candidate_run_id']}" + ".\n"
        + "Release manifest SHA-256: `" + package.sha(package.encoded(receipt)) + "`.\n\n"
        + "Distribution bundles contain the verified binaries, corresponding source, notices and scoped SBOMs. Verify the accompanying Sigstore proofs before use. Approval changes the release state while preserving the tag and every download byte.\n")


def validate_release(value, receipt):
    require(value["tag_name"] == receipt["tag"] and value["target_commitish"] == receipt["source_commit"] and isinstance(value["prerelease"], bool) and value["name"] == receipt["tag"] and value["body"] == release_body(receipt), "existing release belongs to a different publication")
    require(not value["draft"] or value["prerelease"], "draft release must remain prerelease")
    expected, found = release_assets(receipt), {}
    for asset in value["assets"]:
        name = asset["name"]
        require(name not in found and name in expected and asset["state"] == "uploaded" and asset.get("digest") == "sha256:" + expected[name], "existing release asset differs; no overwrite is permitted")
        found[name] = asset["digest"]
    if not value["draft"]:
        require(set(found) == set(expected), "published release is incomplete")
    return set(found)


def validate_tag(github, tag, commit):
    version_tag(tag)
    commit_sha(commit)
    ref = github.api(f"repos/{REPOSITORY}/git/ref/tags/{tag}", missing=True)
    require(ref is not None, "release requires an existing version tag")
    obj, seen = ref["object"], set()
    while obj["type"] == "tag":
        require(obj["sha"] not in seen and len(seen) < 8, "invalid release tag chain")
        seen.add(obj["sha"])
        obj = github.api(f"repos/{REPOSITORY}/git/tags/{commit_sha(obj['sha'])}")["object"]
    require(obj["type"] == "commit" and obj["sha"] == commit, "release tag points to a different source")


def publish_prerelease(github, directory, receipt):
    require_source(github, receipt["tag"], receipt["source_commit"], receipt["version"])
    endpoint = f"repos/{REPOSITORY}/releases/tags/{receipt['tag']}"
    release = github.api(endpoint, missing=True)
    if release is None:
        release = github.api(f"repos/{REPOSITORY}/releases", "POST", {"tag_name":receipt["tag"], "target_commitish":receipt["source_commit"], "name":receipt["tag"], "body":release_body(receipt), "draft":True, "prerelease":True, "make_latest":"false"})
    present = validate_release(release, receipt)
    if not release["draft"]:
        return "already_published"
    expected = release_assets(receipt)
    for name in sorted(set(expected) - present):
        require(package.sha(package.read(directory/name)) == expected[name], "publication bytes changed before upload")
        github.upload(receipt["tag"], directory/name)
    release = github.api(endpoint)
    require(validate_release(release, receipt) == set(expected), "uploaded release asset set is incomplete")
    validate_tag(github, receipt["tag"], receipt["source_commit"])
    github.api(f"repos/{REPOSITORY}/releases/{run_number(release['id'])}", "PATCH", {"draft":False, "prerelease":True, "make_latest":"false"})
    final = github.api(endpoint)
    require(final["draft"] is False and final["prerelease"] is True and validate_release(final, receipt) == set(expected), "published release readback differs")
    validate_tag(github, receipt["tag"], receipt["source_commit"])
    return "published"


def download_release(github, tag, output, state_dir):
    version_tag(tag, stable=True)
    release = github.api(f"repos/{REPOSITORY}/releases/tags/{tag}")
    require(not release["draft"] and release["tag_name"] == tag, "promotion needs an existing public release")
    require(not output.exists(), "release download destination must not exist")
    output.mkdir(parents=True)
    require(len(release["assets"]) == 3 * len(package.TARGETS) + 1, "release asset count differs")
    for asset in release["assets"]:
        name = asset["name"]
        require(package.relative_name(name).name == name and asset["state"] == "uploaded" and 0 <= asset["size"] <= package.MAX_FILE, "invalid release asset")
        github.download(asset["id"], output/name)
        require(package.sha(package.read(output/name)) == asset["digest"].removeprefix("sha256:"), "downloaded release asset differs")
    receipt = verify_publication(github, output, state_dir)
    require(receipt["tag"] == tag, "downloaded release manifest tag differs")
    validate_release(release, receipt)
    return receipt


def promote_release(github, directory, state_dir, expected_receipt_sha):
    require(os.environ.get("GITHUB_REPOSITORY") == REPOSITORY and os.environ.get("GITHUB_REF") == "refs/heads/main" and os.environ.get("GITHUB_EVENT_NAME") == "workflow_dispatch" and os.environ.get("GITHUB_RUN_ATTEMPT") == "1" and os.environ.get("ARG_RELEASE_APPROVED_ENVIRONMENT") == "release", "promotion must use a fresh protected manual main workflow")
    require_release_environment(github)
    require(package.sha(package.read(directory/MANIFEST)) == expected_receipt_sha, "release manifest changed after approval preparation")
    receipt = verify_publication(github, directory, state_dir)
    version_tag(receipt["tag"], receipt["version"], stable=True)
    endpoint = f"repos/{REPOSITORY}/releases/tags/{receipt['tag']}"
    release = github.api(endpoint)
    require(not release["draft"], "draft cannot be promoted")
    validate_release(release, receipt)
    if not release["prerelease"]:
        return "already_promoted"
    github.api(f"repos/{REPOSITORY}/releases/{run_number(release['id'])}", "PATCH", {"prerelease":False, "make_latest":"legacy"})
    final = github.api(endpoint)
    require(final["id"] == release["id"] and not final["draft"] and not final["prerelease"], "promotion state readback differs")
    validate_release(final, receipt)
    validate_tag(github, receipt["tag"], receipt["source_commit"])
    return "promoted"


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
    parser.add_argument("command", choices=["gate", "pack", "inspect", "verify", "prepare-publication", "publish", "download-release", "promote"])
    parser.add_argument("--directory", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--commit")
    parser.add_argument("--target", choices=sorted(package.TARGETS))
    parser.add_argument("--run-id")
    parser.add_argument("--attempt")
    parser.add_argument("--tag")
    parser.add_argument("--receipt-sha")
    args = parser.parse_args()
    github, state = Github(), ROOT/".local/provenance-state"
    try:
        if args.command == "gate":
            value = candidate_gate(github)
            with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
                for key, val in value.items():
                    output.write(f"{key}={val}\n")
        elif args.command == "pack":
            pack_distribution(args.directory, args.output)
        elif args.command == "inspect":
            inspect_distribution(args.directory, args.commit, args.target, state)
        elif args.command == "verify":
            verify_distribution(github, args.directory, args.commit, args.target, args.run_id, args.attempt, state, args.tag)
        elif args.command == "prepare-publication":
            prepare_publication(github, args.directory, args.run_id, args.output, state)
        elif args.command == "download-release":
            download_release(github, args.tag, args.output, state)
        elif args.command == "promote":
            promote_release(github, args.directory, state, args.receipt_sha)
        else:
            require(os.environ.get("GITHUB_REPOSITORY") == REPOSITORY and os.environ.get("GITHUB_REF") == "refs/heads/main" and os.environ.get("GITHUB_EVENT_NAME") in {"workflow_run", "workflow_dispatch"}, "publication must use the trusted main workflow")
            receipt = verify_publication(github, args.directory, state)
            publish_prerelease(github, args.directory, receipt)
    except (OSError, ValueError, KeyError, TypeError, AttributeError, package.PackageError, package.license_audit.AuditError, package.check_public_boundary.BoundaryError, tarfile.TarError, zipfile.BadZipFile, subprocess.TimeoutExpired):
        print("release-provenance: failed; no overwrite or fallback performed", file=sys.stderr)
        return 1
    print(json.dumps({"status":"passed", "command":args.command}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
