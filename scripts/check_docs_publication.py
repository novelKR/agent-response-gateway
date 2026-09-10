#!/usr/bin/env python3
"""Require clean, commit-bound provenance before packaging a verified Docs site.

Run docs-site/scripts/check-output.py first: it validates the actual file inventory,
links and hashes. This additional gate checks provenance only; it never records
reviews, rewrites a manifest, builds a site or approves a publication.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]


def validate_manifest(manifest: object, expected_commit: str) -> None:
    if not isinstance(expected_commit, str) or not re.fullmatch(r"[0-9a-f]{40}", expected_commit):
        raise ValueError("Expected a full source commit SHA")
    if not isinstance(manifest, dict) or manifest.get("schema") != "gateway-docs-build/v1":
        raise ValueError("Unsupported Docs build manifest")
    if manifest.get("source_commit") != expected_commit:
        raise ValueError("Docs artifact source does not match the workflow commit")
    # Reject missing/null/0/string values as well as an explicitly dirty build.
    if manifest.get("working_tree") is not False:
        raise ValueError("Docs publication requires an explicitly clean working tree")
    if not isinstance(manifest.get("files"), dict) or not manifest["files"]:
        raise ValueError("Docs publication requires a nonempty verified file inventory")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--commit", required=True, help="Full commit SHA from the publishing workflow")
    parser.add_argument("--manifest", type=Path, default=ROOT / ".local/docs-site/dist/build-manifest.json")
    args = parser.parse_args(argv)
    try:
        manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError):
        print("Docs publication check failed: cannot read a valid build manifest", file=sys.stderr)
        return 1
    try:
        validate_manifest(manifest, args.commit)
    except ValueError as error:
        print("Docs publication check failed: " + str(error), file=sys.stderr)
        return 1
    print("Docs publication provenance passed: clean source matches workflow commit")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
