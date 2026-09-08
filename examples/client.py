#!/usr/bin/env python3
"""A stdlib-only client for the local gateway; no provider-specific headers."""

import argparse
import ipaddress
import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-url", required=True, help="Ready JSON's loopback /v1 URL")
    parser.add_argument("--token-env", default="ARG_LOCAL_TOKEN")
    parser.add_argument("--model")
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument("--list-models", action="store_true")
    action.add_argument("--input", help="Synthetic prompt; sending it invokes the configured upstream")
    parser.add_argument("--stream", action="store_true")
    args = parser.parse_args()

    try:
        parsed = urllib.parse.urlsplit(args.base_url)
        valid = (
            parsed.scheme == "http"
            and parsed.hostname is not None
            and ipaddress.ip_address(parsed.hostname).is_loopback
            and parsed.port is not None
            and parsed.path.rstrip("/") == "/v1"
            and not parsed.username
            and not parsed.password
            and not parsed.query
            and not parsed.fragment
        )
    except ValueError:
        valid = False
    if not valid:
        parser.error("--base-url must be the gateway's numeric loopback HTTP /v1 URL with a port")
    token = os.environ.get(args.token_env, "")
    if not token or any(ord(c) < 33 or ord(c) > 126 for c in token):
        parser.error("the local token environment variable is missing or invalid")
    if args.input is not None and not args.model:
        parser.error("--model is required with --input")
    if args.list_models and args.stream:
        parser.error("--stream is only supported with --input")

    headers = {"Authorization": f"Bearer {token}"}
    if args.list_models:
        path, data = "/models", None
    else:
        path = "/responses"
        headers["Content-Type"] = "application/json"
        data = json.dumps(
            {"model": args.model, "input": args.input, "stream": args.stream, "store": False},
            ensure_ascii=False,
        ).encode("utf-8")
    request = urllib.request.Request(args.base_url.rstrip("/") + path, data=data, headers=headers)
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
    try:
        with opener.open(request, timeout=75) as response:
            while chunk := response.read1(8192):
                sys.stdout.buffer.write(chunk)
                sys.stdout.buffer.flush()
        return 0
    except urllib.error.HTTPError as error:
        print(f"Gateway returned HTTP {error.code}; response body omitted from diagnostics.", file=sys.stderr)
        return 1
    except (urllib.error.URLError, TimeoutError, OSError):
        print("Gateway transport failed; no automatic retry was attempted.", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    raise SystemExit(main())
