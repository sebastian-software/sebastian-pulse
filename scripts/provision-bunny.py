#!/usr/bin/env python3
"""Provision the initial Pulse application without rendering secrets to disk."""

import argparse
import json
import os
from pathlib import Path
import re
import sys
import urllib.error
import urllib.parse
import urllib.request


MANIFEST = Path(__file__).resolve().parent.parent / "ops/bunny/application.workos.template.json"
RUNTIME_NAMES = {
    "APP_ENV", "PORT", "DATABASE_TLS", "DATABASE_URL", "AUTH_PUBLIC_URL",
    "WORKOS_API_KEY", "WORKOS_CLIENT_ID", "WORKOS_AUTHKIT_URL", "WORKOS_COOKIE_PASSWORD",
}
REFERENCE = re.compile(r"^\$env:([A-Z][A-Z0-9_]*)$")


def validate(manifest, rendered=False):
    """Reject a different target, credential boundary, or unpinned release."""
    if manifest.get("name") != "sebastian-pulse" or manifest.get("runtimeType") != "shared":
        raise ValueError("Expected the shared Sebastian Pulse application")
    if manifest.get("autoScaling") != {"min": 1, "max": 1}:
        raise ValueError("The initial deployment requires exactly one instance")
    if manifest.get("regionSettings") != {
        "allowedRegionIds": ["DE"], "requiredRegionIds": ["DE"], "maxAllowedRegions": 1
    }:
        raise ValueError("The initial deployment must stay in Frankfurt")
    if manifest.get("terminationGracePeriodSeconds") != 30 or manifest.get("volumes"):
        raise ValueError("Expected a stateless app with a 30-second shutdown grace")
    containers = manifest.get("containerTemplates", [])
    if len(containers) != 1:
        raise ValueError("Expected one Pulse container")
    container = containers[0]
    if (container.get("imageNamespace"), container.get("imageName")) != (
        "sebastian-software", "sebastian-pulse"
    ):
        raise ValueError("Expected the Pulse image")
    tag, digest = container.get("imageTag", ""), container.get("imageDigest", "")
    if rendered:
        if not re.fullmatch(r"[0-9a-f]{40}", tag):
            raise ValueError("A full tested release commit is required")
        if not re.fullmatch(r"sha256:[0-9a-f]{64}", digest):
            raise ValueError("A tested image digest is required")
    elif (tag, digest) != ("$env:PULSE_IMAGE_TAG", "$env:PULSE_IMAGE_DIGEST"):
        raise ValueError("Expected explicit release inputs")
    if container.get("entryPoint") != {"argumentsArray": ["serve"]} or container.get("volumeMounts"):
        raise ValueError("Only the stateless serve command belongs in the runtime")
    variables = container.get("environmentVariables", [])
    names = [item["name"] for item in variables]
    if len(names) != len(set(names)) or set(names) != RUNTIME_NAMES:
        raise ValueError("Runtime variables must exclude operator and migration credentials")
    values = {item["name"]: item["value"] for item in variables}
    for name, expected in {
        "APP_ENV": "production", "PORT": "3000", "DATABASE_TLS": "verify-full",
        "AUTH_PUBLIC_URL": "https://pulse.sebastian-software.de",
    }.items():
        if values.get(name) != expected:
            raise ValueError("The production origin, port, or TLS profile was changed")
    if container.get("endpoints") != [{
        "displayName": "https", "cdn": {
            "isSslEnabled": False, "portMappings": [{"containerPort": 3000}]
        }
    }]:
        raise ValueError("Expected only the Pulse HTTP endpoint")
    for probe, path in {"startup": "/health/ready", "readiness": "/health/ready", "liveness": "/health/live"}.items():
        http_get = container.get("probes", {}).get(probe, {}).get("httpGet", {})
        if http_get != {"request": {"path": path, "portNumber": 3000}, "response": {"expectedStatusCode": "ok"}}:
            raise ValueError("The configured health probes are required")
    if rendered:
        if not re.fullmatch(r"[0-9a-fA-F]{64}", values["WORKOS_COOKIE_PASSWORD"]):
            raise ValueError("A separate 32-byte session-sealing key is required")
        if not values["WORKOS_API_KEY"].startswith("sk_live_"):
            raise ValueError("A production WorkOS environment key is required")
        if not values["WORKOS_CLIENT_ID"].startswith("client_"):
            raise ValueError("An AuthKit environment client ID is required")
        issuer = parse_url(values["WORKOS_AUTHKIT_URL"])
        if issuer.scheme != "https" or not (issuer.hostname or "").endswith(".authkit.app") or issuer.path not in ("", "/") or issuer.query or issuer.fragment or issuer.username or issuer.password or issuer.port:
            raise ValueError("Expected the default HTTPS AuthKit origin")
        database = parse_url(values["DATABASE_URL"])
        if database.scheme not in ("postgres", "postgresql") or not database.hostname or not database.username or not database.password:
            raise ValueError("A restricted PostgreSQL runtime connection is required")


def parse_url(value):
    try:
        parsed = urllib.parse.urlsplit(value)
        _ = parsed.port
        return parsed
    except ValueError:
        raise ValueError("Invalid deployment connection URL") from None


def render(value, environment):
    if isinstance(value, dict):
        return {key: render(item, environment) for key, item in value.items()}
    if isinstance(value, list):
        return [render(item, environment) for item in value]
    if isinstance(value, str) and value.startswith("$env:"):
        match = REFERENCE.fullmatch(value)
        if not match or not environment.get(match.group(1)):
            raise ValueError("Missing or invalid deployment input")
        return environment[match.group(1)]
    return value


class NoRedirects(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


def api(method, path, key, payload=None):
    request = urllib.request.Request(
        "https://api.bunny.net/mc" + path,
        data=None if payload is None else json.dumps(payload).encode(), method=method,
        headers={"AccessKey": key, "Content-Type": "application/json", "Accept": "application/json"},
    )
    try:
        with urllib.request.build_opener(NoRedirects).open(request, timeout=30) as response:
            raw = response.read(8 * 1024 * 1024 + 1)
            if len(raw) > 8 * 1024 * 1024:
                raise RuntimeError("Bunny response exceeded the size limit")
            return json.loads(raw) if raw else {}
    except urllib.error.HTTPError as error:
        # Provider error bodies can echo submitted environment values.
        raise RuntimeError("Bunny API returned HTTP " + str(error.code)) from None
    except (OSError, ValueError):
        raise RuntimeError("Bunny request failed; response details suppressed") from None


def refuse_existing(key, name):
    cursor = None
    for _ in range(100):
        query = "?limit=1000" + ("&nextCursor=" + urllib.parse.quote(cursor, safe="") if cursor else "")
        apps = api("GET", "/apps" + query, key)
        if not isinstance(apps, dict) or not isinstance(apps.get("items"), list):
            raise RuntimeError("Unexpected application inventory; creation refused")
        if any(app.get("name") == name for app in apps["items"]):
            raise ValueError("Pulse already exists; inspect it before updating")
        cursor = apps.get("cursor")
        if not cursor:
            return
        if not isinstance(cursor, str):
            raise RuntimeError("Unexpected inventory cursor; creation refused")
    raise RuntimeError("Application inventory is incomplete; creation refused")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--validate-template", action="store_true")
    mode.add_argument("--check", action="store_true", help="Validate in-memory inputs without network calls")
    mode.add_argument("--create", action="store_true", help="Create the initial application; deploy separately")
    args = parser.parse_args()
    manifest = json.loads(MANIFEST.read_text())
    validate(manifest)
    if args.validate_template:
        print("Pulse template passed: Frankfurt, one instance, health probes, and separate runtime credentials.")
        return
    payload = render(manifest, os.environ)
    validate(payload, rendered=True)
    if args.check:
        print("Pulse deployment inputs passed; no network calls were made.")
        return
    key = os.environ.get("BUNNY_API_KEY")
    if not key:
        raise ValueError("Missing BUNNY_API_KEY operator credential")
    refuse_existing(key, payload["name"])
    app = api("POST", "/apps", key, payload)
    print(json.dumps({"id": app["id"], "name": payload["name"], "deployed": False}))


if __name__ == "__main__":
    try:
        main()
    except (KeyError, TypeError, ValueError, RuntimeError) as error:
        # ValueError messages above contain no credential values.
        print(str(error) if isinstance(error, RuntimeError) or type(error) is ValueError else "Invalid deployment input", file=sys.stderr)
        sys.exit(1)
