#!/usr/bin/env python3
"""Offline, locked-source license evidence and notice generation (Python 3.11+).

Prepare crates with `cargo fetch --locked` and install the pinned cargo-deny
separately. This script never fetches sources, runs build scripts, or grants
commercial/relicensing rights. SPDX evaluation belongs to cargo-deny.
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
import tarfile
import tempfile
from urllib.parse import urlsplit

if sys.version_info < (3, 11):
    sys.exit("license-audit: Python 3.11 or newer is required")
import tomllib


ROOT = Path(__file__).resolve().parents[1]
REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"
SCHEMA = 1
MAX_ARCHIVE = 64 * 1024 * 1024
MAX_MEMBER = 32 * 1024 * 1024
MAX_EXPANDED = 256 * 1024 * 1024
MAX_RECORD = 8 * 1024 * 1024
MAX_PACKAGES = 4096
SCOPE = "cargo-lock-all-platforms-including-build-and-dev; not a linked-binary inventory"


class AuditError(Exception):
    """Only static diagnostics: never expose local paths or input contents."""


class BoundedTarInfo(tarfile.TarInfo):
    @classmethod
    def frombuf(cls, buf, encoding, errors):
        result = super().frombuf(buf, encoding, errors)
        require(0 <= result.size <= MAX_MEMBER, "crate archive header exceeds inspection limit")
        return result


def require(condition, message):
    if not condition:
        raise AuditError(message)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def json_bytes(value):
    return (json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode()


def exact_keys(value, keys):
    require(isinstance(value, dict) and set(value) == set(keys), "invalid record fields")


def no_duplicate_keys(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate JSON field")
        result[key] = value
    return result


def read_bytes(path, limit=MAX_RECORD):
    require(not path.is_symlink() and path.is_file(), "required regular file is missing")
    with path.open("rb") as stream:
        data = stream.read(limit + 1)
    require(len(data) <= limit, "input exceeds inspection limit")
    return data


def read_json(path):
    return json.loads(read_bytes(path), object_pairs_hook=no_duplicate_keys)


def valid_hash(value, length=64):
    return isinstance(value, str) and re.fullmatch(f"[0-9a-f]{{{length}}}", value) is not None


def relative_path(value):
    require(isinstance(value, str) and value and "\\" not in value, "invalid evidence path")
    parts = value.split("/")
    require(all(p not in {"", ".", ".."} and not any(c in p for c in ":\r\n\0") for p in parts), "invalid evidence path")
    require(not any(p.lower() in {".private", ".git", ".local"} for p in parts), "reserved evidence path")
    return value


def public_url(value):
    require(isinstance(value, str), "invalid evidence URL")
    parsed = urlsplit(value)
    require(parsed.scheme == "https" and parsed.hostname and not parsed.username and not parsed.password and not parsed.query and not parsed.fragment, "invalid evidence URL")
    require(not any(c.isspace() or ord(c) < 32 for c in value), "invalid evidence URL")
    return value


def package_key(package):
    name, version = package.get("name"), package.get("version")
    require(isinstance(name, str) and re.fullmatch(r"[A-Za-z0-9_-]+", name), "invalid package name")
    require(isinstance(version, str) and re.fullmatch(r"[0-9A-Za-z.+-]+", version), "invalid package version")
    return name, version


def load_policy(root):
    policy = read_json(root / "licensing/policy.json")
    exact_keys(policy, {"schema_version", "cargo_deny_version", "root", "allowed_licenses", "releases", "supplemental_notices"})
    require(policy["schema_version"] == SCHEMA, "unsupported policy schema")
    require(policy["cargo_deny_version"] == "0.20.2", "unreviewed cargo-deny version")
    own = policy["root"]
    exact_keys(own, {"name", "license", "license_sha256"})
    require(own["name"] == "agent-response-gateway" and own["license"] == "AGPL-3.0-only", "invalid project license boundary")
    require(valid_hash(own["license_sha256"]) and digest(read_bytes(root / "LICENSE")) == own["license_sha256"], "project license text changed")
    allowed = policy["allowed_licenses"]
    require(isinstance(allowed, list) and allowed and all(isinstance(x, str) and x for x in allowed) and len(set(allowed)) == len(allowed), "invalid license allow list")
    require(own["license"] not in allowed, "project AGPL must not be globally allowed")
    # Every new license is a policy change, not an inferred OSI/FSF exemption.
    require(set(allowed) <= {"MIT", "Apache-2.0", "BSD-3-Clause", "ISC", "Unicode-3.0", "CDLA-Permissive-2.0", "Zlib"}, "unreviewed third-party license policy")
    manifest = tomllib.loads(read_bytes(root / "Cargo.toml").decode())
    package = manifest["package"]
    require(package["name"] == own["name"] and package.get("license") == own["license"], "root package license differs from policy")
    require(package.get("license-file") is None, "ambiguous root license declaration")
    releases = policy["releases"]
    require(isinstance(releases, list) and releases, "release policy is missing")
    versions = set()
    for release in releases:
        exact_keys(release, {"version", "status", "public_license", "alternative_terms", "rights_status"})
        require(release["version"] not in versions, "duplicate release policy")
        versions.add(release["version"])
        require(release["public_license"] == own["license"], "release license differs from policy")
        require(release["status"] == "unreleased" and release["alternative_terms"] == "separately_executed_agreement" and release["rights_status"] == "not_established", "release or commercial rights need separate review")
    require(package["version"] in versions, "package version lacks release policy")
    supplements = policy["supplemental_notices"]
    require(isinstance(supplements, list), "invalid supplemental notices")
    seen = set()
    for notice in supplements:
        exact_keys(notice, {"name", "version", "package_checksum", "path", "url", "sha256", "vcs_commit"})
        key = package_key(notice)
        require(key not in seen, "duplicate supplemental notice")
        seen.add(key)
        relative_path(notice["path"])
        public_url(notice["url"])
        require(valid_hash(notice["sha256"]) and valid_hash(notice["package_checksum"]) and valid_hash(notice["vcs_commit"], 40), "invalid supplemental evidence hash")
        require(notice["vcs_commit"] in urlsplit(notice["url"]).path.split("/"), "supplemental URL is not commit pinned")
    return policy


def run_process(args, cwd):
    env = {**os.environ, "CARGO_NET_OFFLINE": "true", "CARGO_TERM_COLOR": "never"}
    try:
        return subprocess.run(args, cwd=cwd, env=env, capture_output=True, timeout=180, check=False)
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise AuditError("inspection tool unavailable or timed out") from exc


def load_metadata(root, policy):
    result = run_process(["cargo", "metadata", "--format-version", "1", "--locked", "--offline"], root)
    require(result.returncode == 0, "locked offline Cargo metadata failed; prepare the exact dependencies first")
    metadata = json.loads(result.stdout)
    own_id = metadata["resolve"]["root"]
    require(metadata["workspace_members"] == [own_id], "unreviewed workspace member")
    own = next(p for p in metadata["packages"] if p["id"] == own_id)
    require(Path(own["manifest_path"]).resolve() == root / "Cargo.toml" and own["source"] is None and own["name"] == policy["root"]["name"], "root license exception is not bound to the project")
    require(all(p["id"] == own_id or p["name"] != own["name"] for p in metadata["packages"]), "third party collides with project exception")
    require(Path(metadata["workspace_root"]).resolve() == root, "unexpected workspace root")
    return metadata


def locked_packages(root):
    locked = tomllib.loads(read_bytes(root / "Cargo.lock").decode())
    packages = []
    seen = set()
    for package in locked["package"]:
        key = package_key(package)
        require(key not in seen, "ambiguous locked package identity")
        seen.add(key)
        if not package.get("source"):
            require(package["name"] == "agent-response-gateway", "unreviewed local dependency")
            continue
        require(package["source"] == REGISTRY, "dependency source needs a reviewed provenance adapter")
        require(valid_hash(package.get("checksum")), "locked source checksum is missing")
        packages.append({k: package[k] for k in ("name", "version", "source", "checksum")})
    require(0 < len(packages) <= MAX_PACKAGES, "invalid locked package count")
    return sorted(packages, key=package_key)


def archive_files(package, cargo_home):
    filename = f"{package['name']}-{package['version']}.crate"
    candidates = sorted((cargo_home / "registry/cache").glob(f"*/{filename}"))
    require(bool(candidates), "locked crate archive is missing; run cargo fetch --locked")
    # Multiple registry directories must agree; do not silently select different bytes.
    archives = [read_bytes(path, MAX_ARCHIVE) for path in candidates]
    require(all(digest(data) == package["checksum"] for data in archives), "crate archive checksum mismatch")
    files = {}
    total = 0
    with tarfile.open(fileobj=io.BytesIO(archives[0]), mode="r:gz", tarinfo=BoundedTarInfo) as archive:
        for member in archive:
            path = PurePosixPath(member.name)
            require(path.parts and path.parts[0] == f"{package['name']}-{package['version']}", "invalid crate archive prefix")
            if member.isdir():
                continue
            require(member.isfile() and not member.issym() and not member.islnk(), "unsupported crate archive entry")
            relative = relative_path("/".join(path.parts[1:]))
            require(relative not in files and 0 <= member.size <= MAX_MEMBER, "invalid crate archive member")
            total += member.size
            require(total <= MAX_EXPANDED and len(files) < 100_000, "crate archive exceeds inspection limit")
            stream = archive.extractfile(member)
            require(stream is not None, "unreadable crate archive member")
            files[relative] = stream.read(MAX_MEMBER + 1)
            require(len(files[relative]) == member.size, "truncated crate archive member")
    return files


def notice_path(path):
    name = PurePosixPath(path).name.upper()
    return name == "AUTHORS" or name.startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE", "COPYRIGHT"))


def collect(root, policy, metadata, cargo_home):
    locked = locked_packages(root)
    third_party = {package_key(p): p for p in metadata["packages"] if p["source"] is not None}
    require(set(third_party) == {package_key(p) for p in locked}, "Cargo metadata and full lock inventory differ")
    supplemental = {package_key(n): n for n in policy["supplemental_notices"]}
    require(set(supplemental) <= set(third_party), "stale supplemental notice")
    records, blobs = [], {}
    for package in locked:
        key = package_key(package)
        meta = third_party[key]
        require(meta["source"] == package["source"], "metadata source mismatch")
        files = archive_files(package, cargo_home)
        manifest = tomllib.loads(files["Cargo.toml"].decode())["package"]
        require(package_key(manifest) == key, "archive package identity mismatch")
        declared = manifest.get("license")
        require(isinstance(declared, str) and declared and declared == meta["license"], "license declaration missing or inconsistent")
        source_root = Path(meta["manifest_path"]).parent
        # cargo-deny reads these manifests and license files; bind those reads to
        # the checked archive rather than trusting arbitrary cache contents.
        require(all(read_bytes(source_root / path, MAX_MEMBER) == data for path, data in files.items()), "unpacked crate source differs from locked archive")
        paths = sorted(path for path in files if notice_path(path))
        if manifest.get("license-file"):
            declared_path = relative_path(manifest["license-file"])
            require(declared_path in files, "declared license file missing")
            paths = sorted(set(paths) | {declared_path})
        notices = []
        for path in paths:
            data = files[path]
            data.decode("utf-8")
            require(bool(data.strip()), "empty notice text")
            sha = digest(data)
            blobs[sha] = data
            notices.append({"path": path, "sha256": sha, "origin": "crate_archive"})
        if key in supplemental:
            extra = supplemental[key]
            require(extra["package_checksum"] == package["checksum"], "supplemental package checksum mismatch")
            vcs = json.loads(files[".cargo_vcs_info.json"])
            require(vcs["git"]["sha1"] == extra["vcs_commit"], "supplemental source commit mismatch")
            data = read_bytes(root / "licensing/texts" / f"{extra['sha256']}.txt")
            require(digest(data) == extra["sha256"], "supplemental notice hash mismatch")
            data.decode("utf-8")
            blobs[extra["sha256"]] = data
            notices.append({"path": extra["path"], "sha256": extra["sha256"], "origin": "pinned_upstream", "url": extra["url"], "vcs_commit": extra["vcs_commit"]})
        require(bool(notices), "package has no verified notice text")
        records.append({**package, "declared_license": declared, "notices": sorted(notices, key=lambda n: n["path"])})
    return records, blobs


class Deny:
    """Evaluate all lock entries, including normally inactive Cargo graph nodes."""

    def __init__(self, root, executable, metadata, policy, directory):
        self.root = root
        self.executable = executable
        self.policy = policy
        self.directory = directory
        version = run_process([str(executable), "--version"], root)
        require(version.returncode == 0 and version.stdout.decode().strip() == "cargo-deny " + policy["cargo_deny_version"], "pinned cargo-deny version required")
        for name in ("deny.exceptions.toml", ".deny.exceptions.toml", ".cargo/deny.exceptions.toml"):
            require(not (root / name).exists(), "implicit cargo-deny exception file is not permitted")
        self.metadata = copy.deepcopy(metadata)
        # Only root selection is overlaid; source, version, original license and
        # resolved edges remain the exact offline Cargo metadata. This is an
        # audit graph, NOT a change to Cargo.toml or the actual build graph.
        self.metadata["workspace_members"] = [p["id"] for p in metadata["packages"]]
        self.metadata["workspace_default_members"] = self.metadata["workspace_members"]
        self.metadata_path = directory / "metadata.json"
        self.metadata_path.write_bytes(json_bytes(self.metadata))
        self.expected = {package_key(p) for p in metadata["packages"]}
        self.own_key = package_key(next(p for p in metadata["packages"] if p["id"] == metadata["resolve"]["root"]))

    def evaluate(self, selections):
        require(set(selections) == self.expected - {self.own_key}, "SPDX selection coverage mismatch")
        config = ['[licenses]', 'allow = []', 'include-dev = true', 'include-build = true', 'unused-allowed-license = "deny"', 'unused-license-exception = "allow"', '[licenses.private]', 'ignore = false']
        values = {**selections, self.own_key: [self.policy["root"]["license"]]}
        for (name, version), licenses in sorted(values.items()):
            require(isinstance(licenses, list) and all(isinstance(x, str) for x in licenses), "invalid license selection")
            config.extend(['[[licenses.exceptions]]', 'crate = ' + json.dumps(name + '@=' + version), 'allow = ' + json.dumps(licenses)])
        config_path = self.directory / "deny.toml"
        config_path.write_text("\n".join(config) + "\n", encoding="utf-8")
        result = run_process([str(self.executable), "--format", "json", "--log-level", "debug", "--metadata-path", str(self.metadata_path), "--workspace", "--config", str(config_path), "--frozen", "check", "licenses"], self.root)
        answers = {}
        summaries = []
        for line in result.stderr.splitlines():
            item = json.loads(line)
            fields = item["fields"]
            if item["type"] == "log":
                require(fields.get("level") != "ERROR", "cargo-deny could not inspect prepared sources")
            if item["type"] == "summary":
                summaries.append(fields["licenses"])
            if item["type"] != "diagnostic":
                continue
            code = fields.get("code")
            if code in {"accepted", "rejected", "unlicensed"}:
                graphs = fields.get("graphs", [])
                require(graphs and "Krate" in graphs[0], "cargo-deny diagnostic lacks package identity")
                key = package_key(graphs[0]["Krate"])
                require(key in self.expected, "cargo-deny diagnostic coverage mismatch")
                if key in answers:
                    # 0.20.2 reports unlicensed both during gathering and final
                    # checking after an expression parse failure. Both deny.
                    require(code == "unlicensed" and answers[key] is False, "conflicting cargo-deny diagnostics")
                    continue
                answers[key] = code == "accepted"
            elif fields["severity"] in {"error", "bug"} and code not in {"parse-error", "empty-license-field", "no-license-field"}:
                raise AuditError("cargo-deny policy or evidence error")
        require(set(answers) == self.expected and len(summaries) == 1, "cargo-deny did not check every locked package")
        # cargo-deny 0.20.2 uses the bit value 4 for license-check failures.
        require(result.returncode in {0, 4} and answers[self.own_key], "cargo-deny failed or project license rejected")
        require((result.returncode == 0) == all(answers.values()), "cargo-deny outcome mismatch")
        return {key: accepted for key, accepted in answers.items() if key != self.own_key}


def select_licenses(records, policy, evaluator, previous):
    allowed = policy["allowed_licenses"]
    selections = {}
    new = set()
    for record in records:
        key = package_key(record)
        prior = previous.get(key)
        if prior and all(prior.get(k) == record[k] for k in ("source", "checksum", "declared_license")):
            selections[key] = prior["selected_licenses"]
        else:
            selections[key] = allowed.copy()
            new.add(key)
    require(all(evaluator.evaluate(selections).values()), "dependency license is not permitted or prior selection is invalid")
    # Greedy removal from least to most preferred license, evaluated by the
    # pinned SPDX engine. Never parse/split/rewrite the upstream expression.
    for license_name in reversed(allowed):
        trials = {key: ([x for x in value if x != license_name] if key in new else value) for key, value in selections.items()}
        results = evaluator.evaluate(trials)
        for key in new:
            if results[key]:
                selections[key] = trials[key]
    return selections


def record_document(root, policy, records, selections):
    result = []
    for record in records:
        selected = selections[package_key(record)]
        require(selected and len(selected) == len(set(selected)) and all(x in policy["allowed_licenses"] for x in selected), "license selection is outside policy")
        result.append({**record, "selected_licenses": selected})
    return {"schema_version": SCHEMA, "scope": SCOPE, "cargo_lock_sha256": digest(read_bytes(root / "Cargo.lock")), "policy_sha256": digest(json_bytes(policy)), "packages": result}


def render_notices(document):
    lines = ['# Third-party licenses and notices', '',
             'Generated from the license records. Do not edit this file directly.', '',
             'Scope: all of Cargo.lock, including every platform, build and development dependency.',
             'This list does not claim every entry is linked into the final executable.',
             'A separate project agreement does not replace third-party permissions or notices.',
             'Original texts may include alternative licenses; unselected alternatives are not all applied together.', '',
             f"Cargo.lock SHA-256: `{document['cargo_lock_sha256']}`", '',
             f"Policy SHA-256: `{document['policy_sha256']}`", '']
    for package in document['packages']:
        lines.extend([f"## {package['name']} {package['version']}", '', f"Declared: `{package['declared_license']}`", '', 'Selected: ' + ', '.join(f'`{x}`' for x in package['selected_licenses']), '', f"Original package SHA-256: `{package['checksum']}`", ''])
        for notice in package['notices']:
            lines.append(f"- [{notice['path']}](texts/{notice['sha256']}.txt) — `{notice['sha256']}`")
            if notice['origin'] == 'pinned_upstream':
                lines.append(f"  Source: [original at the pinned commit]({notice['url']})")
        lines.append('')
    return ('\n'.join(lines).rstrip('\n') + '\n').encode()


def bundle_files(root, policy, document, blobs):
    notice_data = render_notices(document)
    files = {"THIRD-PARTY-NOTICES.md": notice_data, "dependencies.json": json_bytes(document), "policy.json": json_bytes(policy)}
    files.update({f"texts/{sha}.txt": data for sha, data in blobs.items()})
    manifest = {"schema_version": SCHEMA, "scope": SCOPE, "cargo_lock_sha256": document["cargo_lock_sha256"], "policy_sha256": document["policy_sha256"], "dependencies_sha256": digest(files["dependencies.json"]), "files": {path: digest(data) for path, data in sorted(files.items())}}
    files["manifest.json"] = json_bytes(manifest)
    return files


def write_file(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    require(not path.is_symlink(), "refusing to overwrite a symlink")
    if path.exists() and read_bytes(path) == data:
        return
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as tmp:
        temp_path = Path(tmp.name)
        try:
            tmp.write(data)
            tmp.flush()
            os.fsync(tmp.fileno())
        except BaseException:
            temp_path.unlink(missing_ok=True)
            raise
    temp_path.chmod(0o644)
    temp_path.replace(path)


def verify_stored(root, document, blobs):
    require(read_bytes(root / "licensing/dependencies.json") == json_bytes(document), "license records are stale or changed; review refresh diff")
    require(read_bytes(root / "licensing/THIRD-PARTY-NOTICES.md") == render_notices(document), "generated notices differ from records")
    for sha, data in blobs.items():
        require(read_bytes(root / "licensing/texts" / f"{sha}.txt") == data, "stored notice missing or altered")


def verify_text_store(root):
    for path in (root / "licensing/texts").iterdir():
        require(path.suffix == ".txt" and valid_hash(path.stem), "unexpected notice store entry")
        data = read_bytes(path)
        require(digest(data) == path.stem, "content-addressed notice was altered")
        data.decode("utf-8")


def audit(root, mode, executable, output=None):
    root = root.resolve()
    require(not (root / "licensing").is_symlink() and not (root / "licensing/texts").is_symlink(), "license evidence directory must not be a symlink")
    policy = load_policy(root)
    verify_text_store(root)
    metadata = load_metadata(root, policy)
    cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo")).resolve()
    records, blobs = collect(root, policy, metadata, cargo_home)
    existing_path = root / "licensing/dependencies.json"
    previous = {}
    if existing_path.exists():
        old = read_json(existing_path)
        exact_keys(old, {"schema_version", "scope", "cargo_lock_sha256", "policy_sha256", "packages"})
        require(old["schema_version"] == SCHEMA and old["scope"] == SCOPE, "unsupported dependency record schema")
        require(isinstance(old["packages"], list), "invalid package records")
        for record in old["packages"]:
            exact_keys(record, {"name", "version", "source", "checksum", "declared_license", "notices", "selected_licenses"})
            key = package_key(record)
            require(key not in previous, "duplicate package record")
            previous[key] = record
    if mode != "refresh":
        require(set(previous) == {package_key(r) for r in records}, "locked dependency coverage changed; refresh review required")
    temporary = root / ".local/license-state"
    temporary.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="spdx-", dir=temporary) as temp:
        evaluator = Deny(root, executable, metadata, policy, Path(temp))
        if mode == "refresh":
            selections = select_licenses(records, policy, evaluator, previous)
        else:
            selections = {key: record["selected_licenses"] for key, record in previous.items()}
            require(all(evaluator.evaluate(selections).values()), "SPDX expression rejects the selected licenses")
        document = record_document(root, policy, records, selections)
        if mode == "refresh":
            # All validation precedes tracked writes. Old content-addressed texts
            # remain available; refresh does not delete history or approve rights.
            for sha, data in blobs.items():
                path = root / "licensing/texts" / f"{sha}.txt"
                require(not path.exists() or read_bytes(path) == data, "content-addressed notice was altered")
            for sha, data in blobs.items():
                write_file(root / "licensing/texts" / f"{sha}.txt", data)
            write_file(existing_path, json_bytes(document))
            write_file(root / "licensing/THIRD-PARTY-NOTICES.md", render_notices(document))
        else:
            verify_stored(root, document, blobs)
        if mode == "bundle":
            require(output is not None, "bundle output missing")
            output = output.resolve()
            require(output != root and output not in root.parents and output != root / "licensing", "unsafe bundle destination")
            require(not output.exists() or not any(output.iterdir()), "bundle destination must be new or empty")
            for path, data in bundle_files(root, policy, document, blobs).items():
                write_file(output / path, data)
    return len(records), len(blobs)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("check", "refresh", "bundle"))
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--cargo-deny", type=Path, help="pinned executable; default: project .local/tools/bin/cargo-deny")
    parser.add_argument("--output", type=Path, help="new/empty bundle directory; default: project .local/release/licenses")
    args = parser.parse_args()
    try:
        require(args.command == "bundle" or args.output is None, "output is only valid for bundle")
        executable = (args.cargo_deny or args.root / ".local/tools/bin" / ("cargo-deny.exe" if os.name == "nt" else "cargo-deny")).resolve()
        output = args.output or args.root / ".local/release/licenses"
        count, texts = audit(args.root, args.command, executable, output)
        suffix = "; review generated diff; no commercial rights granted" if args.command == "refresh" else ""
        print(f"license-audit: {args.command} PASS; packages={count}; notice_texts={texts}{suffix}")
        return 0
    except AuditError as error:
        print(f"license-audit: {error}", file=sys.stderr)
    except (OSError, ValueError, KeyError, TypeError, StopIteration, tarfile.TarError):
        print("license-audit: invalid or unavailable input; no validation result", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
