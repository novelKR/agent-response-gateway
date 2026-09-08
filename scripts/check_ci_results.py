#!/usr/bin/env python3
"""Fail the required CI aggregate unless every prerequisite succeeded."""

import json
import os


def succeeded(raw: str) -> bool:
    try:
        results = json.loads(raw)
    except (ValueError, TypeError):
        return False
    return (
        isinstance(results, dict)
        and bool(results)
        and all(isinstance(job, dict) and job.get("result") == "success" for job in results.values())
    )


if __name__ == "__main__":
    passed = succeeded(os.environ.get("CI_RESULTS", ""))
    print("Required CI prerequisites passed" if passed else "Required CI prerequisites did not all succeed")
    raise SystemExit(0 if passed else 1)
