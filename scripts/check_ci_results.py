#!/usr/bin/env python3
"""Fail the required CI aggregate unless every prerequisite succeeded."""

import json
import os

REQUIRED_JOBS = {"format", "targets", "rust", "publication", "licenses", "codex-conformance", "package-smoke", "docs", "usage-recorder", "api-codecs", "management-web"}


def succeeded(raw: str) -> bool:
    try:
        results = json.loads(raw)
    except (ValueError, TypeError):
        return False
    return (
        isinstance(results, dict)
        and set(results) == REQUIRED_JOBS
        and all(isinstance(job, dict) and job.get("result") == "success" for job in results.values())
    )


if __name__ == "__main__":
    passed = succeeded(os.environ.get("CI_RESULTS", ""))
    print("Required CI prerequisites passed" if passed else "Required CI prerequisites did not all succeed")
    raise SystemExit(0 if passed else 1)
