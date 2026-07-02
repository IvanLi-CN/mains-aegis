#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import sys
import time
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any


DEFAULT_PACKAGES = [
    "bq40.manufacturing",
    "bq25792.regs",
    "derived.power",
]

FORBIDDEN_PATH_PARTS = (
    "/bind",
    "/unbind",
    "/artifact",
    "/flash",
    "/reset",
    "/monitor",
    "/settings",
    "/host-power",
)


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Read-only HIL gate for the diag-snapshot contract. "
            "This script only performs GET /diag-snapshot reads."
        )
    )
    parser.add_argument("--devd-url", required=True, help="mains-aegis-devd HTTP base URL.")
    parser.add_argument("--device-id", required=True, help="Explicit devd device id.")
    parser.add_argument(
        "--package",
        dest="packages",
        action="append",
        default=[],
        help="diag-snapshot package id. Defaults to bq40.manufacturing,bq25792.regs,derived.power.",
    )
    parser.add_argument("--timeout-sec", type=float, default=10.0)
    parser.add_argument("--retries", type=int, default=1)
    parser.add_argument("--out", help="Write JSON result to this path.")
    return parser.parse_args(argv)


def build_diag_snapshot_url(devd_url: str, device_id: str, packages: list[str]) -> str:
    base = devd_url.rstrip("/")
    device = urllib.parse.quote(device_id, safe="")
    query: list[tuple[str, str]] = [
        ("fresh", "true"),
        ("include_meta", "true"),
    ]
    for package in packages:
        query.append(("package", package))
    url = f"{base}/api/v1/devices/{device}/diag-snapshot?{urllib.parse.urlencode(query)}"
    assert_read_only_url(url)
    return url


def assert_read_only_url(url: str) -> None:
    parsed = urllib.parse.urlparse(url)
    for forbidden in FORBIDDEN_PATH_PARTS:
        if forbidden in parsed.path:
            raise ValueError(f"refusing non-read-only devd path: {parsed.path}")
    if not parsed.path.endswith("/diag-snapshot"):
        raise ValueError(f"expected /diag-snapshot path, got: {parsed.path}")


def fetch_json(url: str, timeout_sec: float) -> dict[str, Any]:
    request = urllib.request.Request(url, method="GET")
    with urllib.request.urlopen(request, timeout=timeout_sec) as response:
        body = response.read()
    payload = json.loads(body.decode("utf-8"))
    if not isinstance(payload, dict):
        raise ValueError("diag-snapshot response must be a JSON object")
    return payload


def package_payload(response: dict[str, Any], package_id: str) -> dict[str, Any]:
    sample = response.get("sample")
    if not isinstance(sample, dict):
        sample = response
    packages = sample.get("packages")
    if not isinstance(packages, dict):
        raise ValueError("diag-snapshot response missing packages object")
    package = packages.get(package_id)
    if not isinstance(package, dict):
        raise ValueError(f"diag-snapshot missing package {package_id}")
    payload = package.get("payload")
    if not isinstance(payload, dict):
        raise ValueError(f"diag-snapshot package {package_id} missing object payload")
    return payload


def validate_response(response: dict[str, Any], packages: list[str]) -> list[str]:
    failures: list[str] = []
    for package in packages:
        try:
            payload = package_payload(response, package)
        except ValueError as exc:
            failures.append(str(exc))
            continue
        if package == "bq40.manufacturing":
            for field in (
                "manufacturing_status",
                "fet_en",
                "chg_en",
                "dsg_en",
                "safety_status",
                "pf_status",
                "charging_status",
                "gauging_status",
                "op_status_raw_len",
                "op_status_raw_bytes",
            ):
                if field not in payload:
                    failures.append(f"{package}.{field} missing")
        elif package == "bq25792.regs":
            if not payload:
                failures.append(f"{package} payload is empty")
        elif package == "derived.power":
            for field in ("charger", "bms", "policy"):
                if not isinstance(payload.get(field), dict):
                    failures.append(f"{package}.{field} missing")
    return failures


def run(args: argparse.Namespace) -> int:
    packages = args.packages or DEFAULT_PACKAGES
    url = build_diag_snapshot_url(args.devd_url, args.device_id, packages)
    attempts = max(1, args.retries)
    last_error: str | None = None
    response: dict[str, Any] | None = None
    failures: list[str] = []

    for attempt in range(1, attempts + 1):
        try:
            response = fetch_json(url, args.timeout_sec)
            failures = validate_response(response, packages)
            if not failures:
                break
        except Exception as exc:  # noqa: BLE001 - report transport and schema failures uniformly.
            last_error = str(exc)
        if attempt != attempts:
            time.sleep(0.5)

    ok = response is not None and not failures and last_error is None
    result = {
        "ok": ok,
        "test": "diag_snapshot_readonly",
        "read_only": True,
        "method": "GET",
        "url": url,
        "packages": packages,
        "failures": failures,
        "error": last_error,
        "response": response,
    }
    text = json.dumps(result, indent=2, sort_keys=True) + "\n"
    if args.out:
        Path(args.out).write_text(text, encoding="utf-8")
    print(text, end="")
    return 0 if ok else 1


def main(argv: list[str] | None = None) -> int:
    return run(parse_args(argv))


if __name__ == "__main__":
    raise SystemExit(main())
