#!/usr/bin/env python3
"""Build and verify an unpublished gateway candidate from committed source only."""
from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import stat
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import zipfile
from urllib.parse import quote

import check_public_boundary
import license_audit
from release_targets import TARGETS

ROOT = Path(__file__).resolve().parents[1]
SCHEMA = "gateway-release-candidate/v1"
MAX_FILE = 64 * 1024 * 1024
MAX_TOTAL = 256 * 1024 * 1024


class PackageError(Exception):
    """Static diagnostics only; build output stays in ignored local state."""


def require(condition, message):
    if not condition:
        raise PackageError(message)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def read(path):
    require(path.is_file() and not path.is_symlink(), "required regular file missing")
    with path.open("rb") as stream:
        data = stream.read(MAX_FILE + 1)
    require(len(data) <= MAX_FILE, "file exceeds package limit")
    return data


def encoded(value):
    return (json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2, allow_nan=False) + "\n").encode()


def json_value(data):
    return json.loads(data, object_pairs_hook=license_audit.no_duplicate_keys)


def relative_name(value):
    require(isinstance(value, str) and value and not value.startswith("/") and "\\" not in value, "invalid package path")
    require(all(p not in {"", ".", ".."} and not p.endswith((".", " ")) and not re.search(r'[\x00-\x1f:<>"|?*]', p) and not re.fullmatch(r"(?i:CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\..*)?", p) for p in value.split("/")), "invalid package path")
    return PurePosixPath(value)


def write_new(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("xb") as stream:
        stream.write(data)


def run(args, cwd, env=None, log=None, timeout=1800):
    result = subprocess.run([str(v) for v in args], cwd=cwd, env=env, capture_output=True, timeout=timeout)
    if log is not None:
        write_new(log, result.stdout + b"\n" + result.stderr)
    if result.returncode and log is not None and log.name == "cargo.log":
        # Only compiler error summaries from the credential-free source build.
        # Omit rendered source, quoted command arguments and host paths.
        diagnostics = result.stderr.decode("utf-8", errors="replace").splitlines()
        for line in result.stdout.splitlines():
            item = json_value(line)
            if item.get("reason") == "compiler-message" and item["message"].get("level") == "error":
                diagnostics.append("error: " + item["message"]["message"].splitlines()[0])
                for child in item["message"].get("children", []):
                    diagnostics.extend(child["message"].splitlines())
        for line in diagnostics:
            if line.strip().startswith(("error:", "error[", "Caused by:")) or "(os error " in line or re.search(r"\bLNK[0-9]{4}\b", line):
                summary = re.sub(r"`[^`]*`|\"[^\"]*\"|'[^']*'", lambda m: m[0] if re.fullmatch(r"[`\"'][A-Za-z0-9_.-]+[`\"']", m[0]) else "<detail>", line.strip())
                summary = re.sub(r"\S*[\\/]\S*", "<path>", summary)
                print("release-package: compiler: " + summary[:500], file=sys.stderr)
    require(result.returncode == 0, "build or verification command failed; inspect ignored local log")
    return result.stdout


def clean_environment(target_dir, source):
    # Do not inherit provider/GitHub credentials, Cargo feature overrides or compiler wrappers.
    keys = {"PATH", "HOME", "TMPDIR", "LANG", "LC_ALL", "SYSTEMROOT", "RUSTUP_HOME", "CARGO_HOME", "SDKROOT", "DEVELOPER_DIR", "LD_LIBRARY_PATH", "DYLD_LIBRARY_PATH"}
    if os.name == "nt":
        keys.update({"USERPROFILE", "APPDATA", "LOCALAPPDATA", "TEMP", "TMP", "PROGRAMFILES", "PROGRAMFILES(X86)", "PROGRAMW6432", "COMSPEC", "PATHEXT", "INCLUDE", "LIB", "LIBPATH", "VCINSTALLDIR", "VCTOOLSINSTALLDIR", "WINDOWSSDKDIR", "WINDOWSSDKVERSION", "UNIVERSALCRTSDKDIR", "UCRTVERSION"})
    env = {k: v for k, v in os.environ.items() if k.upper() in keys}
    env["CARGO_TARGET_DIR"] = str(target_dir)
    env["CARGO_INCREMENTAL"] = "0"
    sysroot = run(["rustc", "--print", "sysroot"], source, env).decode().strip()
    cargo_home = str(Path(env.get("CARGO_HOME", str(Path.home() / ".cargo"))).resolve())
    # Cargo config overrides are external build inputs; candidates use only this recipe.
    for ancestor in [source, *source.parents]:
        require(not any((ancestor / ".cargo" / name).exists() for name in ["config", "config.toml"]), "external Cargo configuration is not supported by the candidate recipe")
    require(not any((Path(cargo_home) / name).exists() for name in ["config", "config.toml"]), "Cargo-home configuration is not supported by the candidate recipe")
    env["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join([
        f"--remap-path-prefix={source}=/source",
        f"--remap-path-prefix={cargo_home}=/cargo",
        f"--remap-path-prefix={sysroot}=/rust",
    ])
    return env, Path(sysroot)


def tar_bytes(files):
    """files maps relative paths to (exact bytes, mode); no links or host metadata."""
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w", format=tarfile.USTAR_FORMAT) as archive:
        for name, (data, mode) in sorted(files.items()):
            relative_name(name)
            require(mode in {0o644, 0o755} and len(data) <= MAX_FILE, "invalid archive member")
            info = tarfile.TarInfo(name)
            info.mode, info.size = mode, len(data)
            archive.addfile(info, io.BytesIO(data))
    require(raw.tell() <= MAX_TOTAL, "archive exceeds package limit")
    compressed = io.BytesIO()
    with gzip.GzipFile(fileobj=compressed, mode="wb", filename="", mtime=0) as stream:
        stream.write(raw.getvalue())
    return compressed.getvalue()


def zip_bytes(files):
    """Deterministic ZIP with regular Unix mode metadata understood on every host."""
    raw = io.BytesIO()
    total, seen = 0, set()
    with zipfile.ZipFile(raw, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for name, (data, mode) in sorted(files.items()):
            relative_name(name)
            require(name.casefold() not in seen and mode in {0o644, 0o755} and len(data) <= MAX_FILE, "invalid ZIP member")
            seen.add(name.casefold())
            total += len(data)
            require(total <= MAX_TOTAL and len(seen) <= 4096, "ZIP exceeds package limit")
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.create_system = 3
            info.external_attr = (stat.S_IFREG | mode) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(info, data, compresslevel=9)
    return raw.getvalue()


def zip_files(path):
    files, total, seen = {}, 0, set()
    require(path.stat().st_size <= MAX_FILE, "ZIP exceeds package limit")
    with zipfile.ZipFile(path) as archive:
        require(not archive.comment and len(archive.infolist()) <= 4096, "invalid ZIP metadata")
        for item in archive.infolist():
            relative_name(item.filename)
            require(item.orig_filename == item.filename, "ZIP filename was truncated")
            mode = item.external_attr >> 16
            require(item.filename.casefold() not in seen and stat.S_ISREG(mode) and stat.S_IMODE(mode) in {0o644, 0o755}, "unsafe or duplicate ZIP member")
            seen.add(item.filename.casefold())
            require(not item.extra and not item.comment and item.create_system == 3 and item.date_time == (1980, 1, 1, 0, 0, 0) and not item.flag_bits & 1 and item.compress_type == zipfile.ZIP_DEFLATED, "invalid ZIP metadata")
            total += item.file_size
            require(item.file_size <= MAX_FILE and total <= MAX_TOTAL, "ZIP exceeds package limit")
            with archive.open(item) as stream:
                data = stream.read(MAX_FILE + 1)
            require(len(data) == item.file_size, "ZIP member size differs")
            files[item.filename] = (data, stat.S_IMODE(mode))
    require(all(not any(str(p).casefold() in seen for p in PurePosixPath(name).parents if str(p) != ".") for name in files), "ZIP path conflicts with a file")
    return files


def archive_bytes(files, target):
    return zip_bytes(files) if TARGETS[target]["archive"] == "zip" else tar_bytes(files)


def archive_files(path):
    if path.suffix == ".zip":
        return zip_files(path)
    files, total = {}, 0
    with tarfile.open(path, "r:*", tarinfo=check_public_boundary.BoundedTarInfo) as archive:
        for item in archive:
            relative_name(item.name.rstrip("/"))
            require(item.name not in files and item.isfile() and item.size <= MAX_FILE, "unsafe or duplicate archive member")
            require(item.mode in {0o644, 0o755}, "invalid archive mode")
            total += item.size
            require(total <= MAX_TOTAL and len(files) < 4096, "archive exceeds package limit")
            files[item.name] = (archive.extractfile(item).read(), item.mode)
    return files


def compiled_packages(raw):
    result = {}
    finished = False
    for line in raw.splitlines():
        item = json_value(line)
        if item.get("reason") == "compiler-artifact":
            package = result.setdefault(item["package_id"], {"features": set(), "kinds": set()})
            package["features"].update(item["features"])
            package["kinds"].update(item["target"]["kind"])
        elif item.get("reason") == "build-finished":
            require(item.get("success") is True, "build did not succeed")
            finished = True
    require(finished and result, "missing successful Cargo artifact evidence")
    return result


def make_sbom(metadata, built, records, target, version, commit, binary_digest, rustc, linkage):
    require(target in TARGETS and metadata.get("version") == 1, "unsupported target or Cargo metadata")
    packages = {p["id"]: p for p in metadata["packages"]}
    nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
    root = metadata["resolve"]["root"]
    require(root in built and packages[root]["version"] == version, "root build identity differs")
    evidence = {(r["name"], r["version"], r["source"]): r for r in records["packages"]}
    reached, runtime, pending = set(), set(), [(root, True)]
    visited = set()
    while pending:
        ident, active = pending.pop()
        if (ident, active) in visited:
            continue
        visited.add((ident, active))
        require(ident in nodes and ident in packages, "incomplete Cargo graph")
        if ident not in built:
            continue
        reached.add(ident)
        active = active and "proc-macro" not in built[ident]["kinds"]
        if active:
            runtime.add(ident)
        for dep in nodes[ident]["deps"]:
            for kind in dep["dep_kinds"]:
                require(kind["kind"] in {None, "build", "dev"}, "unknown Cargo dependency kind")
                if kind["kind"] != "dev":
                    pending.append((dep["pkg"], active and kind["kind"] is None))
    require(reached == set(built), "compiled packages differ from target build graph")
    refs = {}
    for ident in reached:
        package = packages[ident]
        refs[ident] = f"pkg:cargo/{quote(package['name'], safe='')}@{quote(package['version'], safe='')}"
    require(len(set(refs.values())) == len(refs), "ambiguous package source identity")
    components = []
    for ident in sorted(reached - {root}, key=refs.get):
        package = packages[ident]
        record = evidence.get((package["name"], package["version"], package["source"]))
        require(record is not None and package["source"] == license_audit.REGISTRY, "compiled package lacks reviewed source/license evidence")
        components.append({"type":"library", "bom-ref":refs[ident], "purl":refs[ident], "name":package["name"], "version":package["version"],
            "scope":"required" if ident in runtime else "excluded",
            "hashes":[{"alg":"SHA-256", "content":record["checksum"]}],
            "licenses":[{"expression":" AND ".join(record["selected_licenses"])}],
            "properties":[{"name":"gateway:cargo-source", "value":record["source"]},
                {"name":"gateway:hash-scope", "value":"crate archive checksum from Cargo.lock"},
                {"name":"gateway:compiled-features", "value":json.dumps(sorted(built[ident]["features"]))},
                {"name":"gateway:compiled-target-kinds", "value":json.dumps(sorted(built[ident]["kinds"]))},
                {"name":"gateway:scope", "value":"runtime-source-dependency" if ident in runtime else "build-only-or-procedural-macro"}]})
    root_component = {"type":"application", "bom-ref":refs[root], "name":"agent-response-gateway", "version":version,
        "licenses":[{"expression":"AGPL-3.0-only"}], "hashes":[{"alg":"SHA-256", "content":binary_digest}]}
    std_ref = "rust-standard-library"
    components.append({"type":"library", "bom-ref":std_ref, "name":std_ref, "version":rustc["release"],
        "properties":[{"name":"gateway:scope", "value":"installed target rlibs; component detail retained in supplied toolchain notices"}]})
    system_refs = []
    for index, name in enumerate(linkage["libraries"]):
        ref = f"system-library-{index}"
        system_refs.append(ref)
        components.append({"type":"library", "bom-ref":ref, "name":name, "properties":[{"name":"gateway:scope", "value":"external OS dependency; not redistributed"}]})
    dependencies = []
    for ident in sorted(reached, key=refs.get):
        deps = {refs[d["pkg"]] for d in nodes[ident]["deps"] if d["pkg"] in reached and any(k["kind"] != "dev" for k in d["dep_kinds"])}
        if ident == root:
            deps.update([std_ref, *system_refs])
        dependencies.append({"ref":refs[ident], "dependsOn":sorted(deps)})
    return {"bomFormat":"CycloneDX", "specVersion":"1.6", "version":1,
        "metadata":{"component":root_component, "properties":[{"name":"gateway:target", "value":target}, {"name":"gateway:source-commit", "value":commit},
            {"name":"gateway:inventory-scope", "value":"target-filtered actual Cargo build inputs, Rust std aggregate and observed dynamic OS libraries; not exact linked-byte composition"}]},
        "components":components, "dependencies":dependencies,
        "compositions":[{"aggregate":"incomplete", "assemblies":[refs[root]]}]}


def macos_versions(load):
    versions, command = [], None
    for line in load.splitlines():
        if line.strip().startswith("cmd "):
            command = line.strip().split()[1]
        if command in {"LC_BUILD_VERSION", "LC_VERSION_MIN_MACOSX"}:
            field = "minos" if command == "LC_BUILD_VERSION" else "version"
            match = re.fullmatch(r"\s*" + field + r" (\d+\.\d+(?:\.\d+)?)", line)
            if match:
                versions.append(match[1])
    require(versions, "Mach-O minimum macOS version missing")
    return sorted(set(versions))


def dynamic_linkage(binary, target, env):
    if TARGETS[target]["format"] == "PE":
        program_files = next((v for k, v in env.items() if k.upper() == "PROGRAMFILES(X86)"), None)
        require(program_files is not None, "MSVC installer location missing")
        vswhere = Path(program_files) / "Microsoft Visual Studio/Installer/vswhere.exe"
        found = run([vswhere, "-latest", "-products", "*", "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-find", "VC/Tools/MSVC/*/bin/Hostx64/x64/dumpbin.exe"], ROOT, env).decode().splitlines()
        require(found, "installed MSVC DUMPBIN missing")
        dumpbin = Path(sorted(found)[-1])
        headers = run([dumpbin, "/HEADERS", binary], ROOT, env).decode()
        dependencies = run([dumpbin, "/DEPENDENTS", binary], ROOT, env).decode()
        require(re.search(r"8664 machine \(x64\)", headers, re.IGNORECASE) and re.search(r"20B magic # \(PE32\+\)", headers, re.IGNORECASE), "candidate is not an x64 PE executable")
        libraries = re.findall(r"^\s+([A-Za-z0-9_.+-]+\.dll)\s*$", dependencies, re.MULTILINE | re.IGNORECASE)
        tool = re.search(r"Microsoft .*? Version ([0-9.]+)", headers)
        require(libraries and tool, "PE library or inspection tool evidence missing")
        return {"format":"PE", "machine":"x64", "libraries":sorted(set(v.lower() for v in libraries)), "inspection_tool":{"name":"MSVC DUMPBIN", "version":tool[1]}, "runtime":"external Windows and MSVC runtime DLLs; not bundled"}
    if target == "aarch64-apple-darwin":
        raw = run(["otool", "-L", binary], ROOT, env).decode()
        libraries = []
        for line in raw.splitlines()[1:]:
            name = line.strip().split(" (", 1)[0]
            require(name.startswith(("/usr/lib/", "/System/Library/")), "non-system dynamic library in candidate")
            libraries.append(name)
        load = run(["otool", "-l", binary], ROOT, env).decode()
        versions = macos_versions(load)
        return {"format":"Mach-O", "libraries":sorted(set(libraries)), "declared_platform_versions":sorted(set(versions))}
    raw = run(["readelf", "-d", binary], ROOT, env).decode()
    libraries = re.findall(r"\(NEEDED\).*\[([^\]]+)\]", raw)
    require(libraries and all(re.fullmatch(r"[A-Za-z0-9_.+-]+", v) for v in libraries), "invalid ELF library reference")
    versions = run(["readelf", "--version-info", binary], ROOT, env).decode()
    return {"format":"ELF", "libraries":sorted(set(libraries)), "required_glibc_versions":sorted(set(re.findall(r"Name: (GLIBC_[0-9.]+)", versions)))}


def toolchain_evidence(sysroot, target, rustc):
    doc_roots = [sysroot / "share/doc/rust", sysroot / "share/doc/rustc"]
    doc = next((p for p in doc_roots if (p / "COPYRIGHT-library.html").is_file()), None)
    require(doc is not None, "installed Rust standard-library notices missing")
    notices = {}
    for path in [doc / "COPYRIGHT-library.html", doc / "COPYRIGHT.html"]:
        if path.is_file():
            notices[path.name] = read(path)
    for directory in [doc, sysroot]:
        for name in ["LICENSE-APACHE", "LICENSE-MIT", "COPYRIGHT"]:
            path = directory / name
            if path.is_file():
                value = read(path)
                require(name not in notices or notices[name] == value, "toolchain notice sources disagree")
                notices[name] = value
    license_dir = doc / "licenses"
    if license_dir.is_dir():
        for path in sorted(license_dir.iterdir()):
            notices["licenses/" + path.name] = read(path)
    require(any(k.endswith(("LICENSE-MIT", "MIT.txt")) for k in notices) and any(k.endswith(("LICENSE-APACHE", "Apache-2.0.txt")) for k in notices), "toolchain license texts missing")
    rlibs = {p.name:sha(read(p)) for p in sorted((sysroot / "lib/rustlib" / target / "lib").glob("*.rlib"))}
    require(rlibs and any(k.startswith("libstd-") for k in rlibs), "installed target standard library missing")
    receipt = {"rustc":rustc, "target":target, "installed_rlibs":rlibs, "notice_files":{n:sha(v) for n,v in sorted(notices.items())},
        "scope":"supplied toolchain copyright/license records and installed rlibs; conservative inventory, not a legal clearance or exact link map"}
    notices["manifest.json"] = encoded(receipt)
    return notices, receipt


def verify_candidate(directory, expected_commit=None, expected_target=None):
    manifest = json_value(read(directory / "candidate.json"))
    require(manifest["schema"] == SCHEMA and manifest["target"] in TARGETS and re.fullmatch(r"[0-9a-f]{40}", manifest["source_commit"]), "invalid candidate identity")
    require(expected_commit in {None, manifest["source_commit"]} and expected_target in {None, manifest["target"]}, "candidate binding differs")
    assets = manifest["assets"]
    require(set(p.name for p in directory.iterdir()) == {*assets, "candidate.json", "SHA256SUMS"}, "unexpected or missing candidate file")
    for name, digest in assets.items():
        require(relative_name(name).name == name and re.fullmatch(r"[0-9a-f]{64}", digest), "invalid asset manifest")
        require(sha(read(directory / name)) == digest, "candidate asset digest differs")
    checksums = {**assets, "candidate.json":sha(read(directory / "candidate.json"))}
    require(read(directory / "SHA256SUMS") == "".join(f"{v}  {k}\n" for k,v in sorted(checksums.items())).encode(), "candidate checksum list differs")
    files = archive_files(directory / manifest["binary_archive"])
    require({n:{"sha256":sha(v), "mode":m} for n,(v,m) in files.items()} == manifest["package_files"], "binary archive membership differs")
    executable = "agent-response-gateway/bin/" + TARGETS[manifest["target"]]["executable"]
    require(files[executable][1] == 0o755 and sha(files[executable][0]) == manifest["binary_sha256"], "packaged executable differs")
    checker = check_public_boundary.Checker(ROOT, [])
    for name, (data, mode) in files.items():
        checker.path(name.encode(), f"100{mode:o}".encode())
        checker.size(len(data))
        checker.content(data)
    check_public_boundary.Checker(ROOT, []).archive(directory / manifest["source_archive"])
    with tarfile.open(directory / manifest["source_archive"], tarinfo=check_public_boundary.BoundedTarInfo) as source:
        require(source.pax_headers.get("comment") == manifest["source_commit"], "source archive commit receipt differs")
        tools = manifest["packaging_tools"]
        require(set(tools) == {"release_package.py", "package_smoke.py", "release_targets.py"}, "incomplete packaging source binding")
        for name, digest in tools.items():
            member = source.getmember("agent-response-gateway/scripts/" + name)
            require(member.isfile() and member.size <= MAX_FILE and sha(source.extractfile(member).read()) == digest, "packaging tools differ from committed source")
    sbom = json_value(read(directory / manifest["sbom"])); components = [sbom["metadata"]["component"], *sbom["components"]]
    refs = {c["bom-ref"] for c in components}
    require(sbom["bomFormat"] == "CycloneDX" and sbom["specVersion"] == "1.6" and len(refs) == len(components), "invalid SBOM identity")
    require(all(d["ref"] in refs and set(d["dependsOn"]) <= refs for d in sbom["dependencies"]), "unbound SBOM dependency")
    require(sbom["metadata"]["component"]["hashes"] == [{"alg":"SHA-256", "content":manifest["binary_sha256"]}], "SBOM binary binding differs")
    return manifest


def build_candidate(root, target, output, cargo_deny):
    require(target in TARGETS, "unsupported release target")
    require(not output.exists(), "candidate destination must not exist")
    # The checkout can contain unrelated work; none of it enters the exported source build.
    commit = run(["git", "rev-parse", "HEAD"], root).decode().strip()
    require(re.fullmatch(r"[0-9a-f]{40}", commit), "invalid source commit")
    local = root / ".local/release-build"
    local.mkdir(parents=True, exist_ok=True)
    logs = Path(tempfile.mkdtemp(prefix=target + "-logs-", dir=local))
    with tempfile.TemporaryDirectory(prefix="candidate-", dir=local) as temporary:
        temporary = Path(temporary)
        print("release-package: export committed source", flush=True)
        source_tar = run(["git", "archive", "--format=tar", "--prefix=agent-response-gateway/", commit], root)
        source_path = temporary / "source.tar"
        write_new(source_path, source_tar)
        check_public_boundary.Checker(root, []).archive(source_path)
        with tarfile.open(source_path) as archive:
            archive.extractall(temporary, filter="data")
        source = temporary / "agent-response-gateway"
        env, sysroot = clean_environment(root / "target/release-candidate" / target, source)
        compiler_raw = run(["rustc", "-vV"], source, env).decode().strip()
        compiler = dict(line.split(": ", 1) for line in compiler_raw.splitlines()[1:] if ": " in line)
        compiler["description"] = compiler_raw.splitlines()[0]
        require(compiler.get("release") == "1.98.0" and compiler.get("host") == target, "candidate requires pinned native Rust target")
        version = tomllib.loads(read(source / "Cargo.toml").decode())["package"]["version"]
        require(re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[a-zA-Z0-9.-]+)?", version), "invalid package version")
        # Run the committed audit implementation against the exported records and locked cache.
        print("release-package: verify dependency notices", flush=True)
        run([sys.executable, "-B", source / "scripts/license_audit.py", "bundle", "--cargo-deny", cargo_deny, "--output", temporary / "notices"], source, env, logs / "licenses.log")
        metadata = json_value(run(["cargo", "metadata", "--format-version=1", "--locked", "--offline", "--filter-platform", target], source, env))
        print("release-package: build native executable", flush=True)
        raw = run(["cargo", "build", "--release", "--locked", "--offline", "--target", target, "--message-format=json"], source, env, logs / "cargo.log")
        built = compiled_packages(raw)
        binary = Path(env["CARGO_TARGET_DIR"]) / target / "release" / TARGETS[target]["executable"]
        binary_bytes = read(binary)
        print("release-package: execute native smoke", flush=True)
        run([sys.executable, "-B", root / "scripts/package_smoke.py", "--binary", binary, "--state-dir", temporary / "smoke"], source, env, logs / "smoke.log", timeout=60)
        print("release-package: inspect native linkage", flush=True)
        linkage = dynamic_linkage(binary, target, env)
        print("release-package: collect toolchain notices", flush=True)
        tool_notices, tool_receipt = toolchain_evidence(sysroot, target, compiler)
        records = json_value(read(source / "licensing/dependencies.json"))
        print("release-package: bind target inventory", flush=True)
        sbom = make_sbom(metadata, built, records, target, version, commit, sha(binary_bytes), compiler, linkage)
        package_list = run(["cargo", "package", "--list", "--locked", "--offline"], source, env).decode().splitlines()
        require(package_list and all(not set(PurePosixPath(p).parts) & {".private", ".codex", ".local", "target"} for p in package_list), "Cargo package includes reserved files")
        files = {"agent-response-gateway/bin/" + TARGETS[target]["executable"]:(binary_bytes, 0o755)}
        for name in ["LICENSE", "COMMERCIAL-LICENSING.md", "README.md", "config.example.toml", "config.messages.example.toml", "config.chat.example.toml", "docs/protocol.md", "docs/conformance.md"]:
            files["agent-response-gateway/" + name] = (read(source / name), 0o644)
        for path in sorted((temporary / "notices").rglob("*")):
            if path.is_file():
                files["agent-response-gateway/license-notices/" + path.relative_to(temporary / "notices").as_posix()] = (read(path), 0o644)
        for name, value in tool_notices.items():
            files["agent-response-gateway/rust-notices/" + name] = (value, 0o644)
        output.mkdir(parents=True)
        binary_name = f"agent-response-gateway-{version}-{target}.{TARGETS[target]['archive']}"
        source_name = f"agent-response-gateway-{version}-source.tar.gz"
        sbom_name = f"agent-response-gateway-{version}-{target}.cdx.json"
        print("release-package: create binary archive", flush=True)
        write_new(output / binary_name, archive_bytes(files, target))
        # Run the actual extracted package, including Windows ZIP path/mode handling.
        extracted = temporary / "extracted package"
        for name, (data, mode) in archive_files(output / binary_name).items():
            path = extracted / name
            write_new(path, data)
            path.chmod(mode)
        print("release-package: execute extracted archive smoke", flush=True)
        run([sys.executable, "-B", root / "scripts/package_smoke.py", "--binary", extracted / "agent-response-gateway/bin" / TARGETS[target]["executable"], "--state-dir", temporary / "extracted-smoke"], source, env, logs / "extracted-smoke.log", timeout=60)
        # Normalize gzip metadata around Git's committed tar bytes, including its commit receipt.
        zipped = io.BytesIO()
        with gzip.GzipFile(fileobj=zipped, mode="wb", filename="", mtime=0) as stream:
            stream.write(source_tar)
        write_new(output / source_name, zipped.getvalue())
        write_new(output / sbom_name, encoded(sbom))
        manifest = {"schema":SCHEMA, "package":"agent-response-gateway", "version":version, "source_commit":commit,
            "source_url":f"https://github.com/novelKR/agent-response-gateway/archive/{commit}.tar.gz", "target":target,
            "cargo_lock_sha256":sha(read(source / "Cargo.lock")), "rustc":compiler, "binary_sha256":sha(binary_bytes),
            "binary_archive":binary_name, "source_archive":source_name, "sbom":sbom_name,
            "assets":{p.name:sha(read(p)) for p in sorted(output.iterdir())}, "package_files":{n:{"sha256":sha(v), "mode":m} for n,(v,m) in sorted(files.items())},
            "cargo_package_files":sorted(package_list), "dynamic_linkage":linkage, "rust_toolchain_evidence":tool_receipt,
            "build_recipe":{"profile":"release", "locked":True, "offline":True, "native_target":True, "path_remapping":["/source", "/cargo", "/rust"]},
            "packaging_tools":{n:sha(read(root / "scripts" / n)) for n in ["release_package.py", "package_smoke.py", "release_targets.py"]},
            "build_platform":{"runner":os.environ.get("ImageOS", "local"), "image_version":os.environ.get("ImageVersion", "not_recorded"), "target_spec":TARGETS[target]},
            "validation":{"package_smoke":"passed", "provider_qualification":"not_performed", "consumer_acceptance":"not_performed", "attestation":"not_created", "formal_release":"not_approved"}}
        write_new(output / "candidate.json", encoded(manifest))
        checksums = {p.name:sha(read(p)) for p in sorted(output.iterdir())}
        write_new(output / "SHA256SUMS", "".join(f"{v}  {k}\n" for k,v in sorted(checksums.items())).encode())
        print("release-package: verify completed candidate", flush=True)
        return verify_candidate(output, commit, target)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    build = commands.add_parser("build")
    build.add_argument("--target", choices=sorted(TARGETS), required=True)
    build.add_argument("--output", type=Path, required=True)
    build.add_argument("--cargo-deny", type=Path, default=ROOT / ".local/tools/bin" / ("cargo-deny.exe" if os.name == "nt" else "cargo-deny"))
    verify = commands.add_parser("verify")
    verify.add_argument("directory", type=Path)
    verify.add_argument("--commit")
    verify.add_argument("--target", choices=sorted(TARGETS))
    args = parser.parse_args()
    try:
        result = build_candidate(ROOT, args.target, args.output.resolve(), args.cargo_deny.resolve()) if args.command == "build" else verify_candidate(args.directory, args.commit, args.target)
    except PackageError as error:
        # PackageError messages are static recipe diagnostics, never subprocess output.
        print("release-package: " + str(error), file=sys.stderr)
        return 1
    except (OSError, ValueError, KeyError, TypeError, license_audit.AuditError, check_public_boundary.BoundaryError, tarfile.TarError, zipfile.BadZipFile, subprocess.TimeoutExpired) as error:
        print("release-package: error type " + type(error).__name__, file=sys.stderr)
        print("release-package: failed; inspect ignored local inputs/logs", file=sys.stderr)
        return 1
    print(json.dumps({"status":"passed", "command":args.command, "source_commit":result["source_commit"], "target":result["target"], "binary_sha256":result["binary_sha256"], "formal_release":"not_approved"}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
