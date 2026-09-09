#!/usr/bin/env python3
"""Canonical native build targets shared by CI, packaging and release verification."""
import json

TARGETS = {
    "x86_64-unknown-linux-gnu": {"os": "ubuntu-24.04", "executable": "agent-response-gateway", "archive": "tar.gz", "format": "ELF"},
    "aarch64-unknown-linux-gnu": {"os": "ubuntu-24.04-arm", "executable": "agent-response-gateway", "archive": "tar.gz", "format": "ELF"},
    "aarch64-apple-darwin": {"os": "macos-15", "executable": "agent-response-gateway", "archive": "tar.gz", "format": "Mach-O"},
    "x86_64-pc-windows-msvc": {"os": "windows-2025", "executable": "agent-response-gateway.exe", "archive": "zip", "format": "PE"},
}


def matrix():
    return {"include": [{"target": target, **spec} for target, spec in TARGETS.items()]}


if __name__ == "__main__":
    print(json.dumps(matrix(), separators=(",", ":")))
