#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import socket
import sys
import time
from dataclasses import dataclass
from typing import Any


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Read LoadLynx status through a single loadlynx-devd IPC session."
    )
    parser.add_argument("--ipc-endpoint", required=True)
    parser.add_argument("--device-id", required=True)
    parser.add_argument("--expected-identity-device-id")
    parser.add_argument("--lease-id")
    parser.add_argument("--timeout-sec", type=float, default=20.0)
    parser.add_argument("--scan-first", action="store_true")
    parser.add_argument("--warmup", action="store_true")
    parser.add_argument("--heartbeat", action="store_true")
    parser.add_argument("--release", action="store_true")
    return parser.parse_args()


def ipc_call(endpoint: str, op: str, params: dict[str, Any], timeout_sec: float) -> dict[str, Any]:
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.settimeout(timeout_sec)
    try:
        sock.connect(endpoint)
        sock.sendall(json.dumps({"op": op, "params": params}).encode("utf-8") + b"\n")
        chunks = bytearray()
        while not chunks.endswith(b"\n"):
            chunk = sock.recv(65536)
            if not chunk:
                break
            chunks.extend(chunk)
    finally:
        sock.close()
    return json.loads(chunks.decode("utf-8").strip())


@dataclass
class Lease:
    lease_id: str
    device_id: str
    identity_device_id: str | None
    lease_ttl_ms: int | None
    heartbeat_interval_ms: int | None

    @classmethod
    def from_result(cls, result: dict[str, Any]) -> "Lease":
        return cls(
            lease_id=str(result["lease_id"]),
            device_id=str(result["device_id"]),
            identity_device_id=(
                str(result["identity_device_id"])
                if isinstance(result.get("identity_device_id"), str)
                else None
            ),
            lease_ttl_ms=int(result["lease_ttl_ms"]) if isinstance(result.get("lease_ttl_ms"), int) else None,
            heartbeat_interval_ms=(
                int(result["heartbeat_interval_ms"])
                if isinstance(result.get("heartbeat_interval_ms"), int)
                else None
            ),
        )


def create_lease(args: argparse.Namespace) -> Lease:
    payload = {"device_id": args.device_id}
    if args.expected_identity_device_id:
        payload["expected_identity_device_id"] = args.expected_identity_device_id
    response = ipc_call(
        args.ipc_endpoint,
        "serial.lease.create",
        payload,
        args.timeout_sec,
    )
    if response.get("ok") is not True or not isinstance(response.get("result"), dict):
        raise RuntimeError(f"lease_create_failed: {response}")
    return Lease.from_result(response["result"])


def heartbeat_lease(args: argparse.Namespace, lease_id: str) -> dict[str, Any]:
    return ipc_call(
        args.ipc_endpoint,
        "serial.lease.heartbeat",
        {"lease_id": lease_id},
        args.timeout_sec,
    )


def release_lease(args: argparse.Namespace, lease_id: str) -> dict[str, Any]:
    return ipc_call(
        args.ipc_endpoint,
        "serial.lease.release",
        {"lease_id": lease_id},
        args.timeout_sec,
    )


def scan_devices(args: argparse.Namespace) -> dict[str, Any]:
    return ipc_call(
        args.ipc_endpoint,
        "devices.scan",
        {},
        args.timeout_sec,
    )


def compat_status(args: argparse.Namespace, lease_id: str) -> dict[str, Any]:
    response = ipc_call(
        args.ipc_endpoint,
        "compat.status",
        {
            "device_id": args.device_id,
            "lease_id": lease_id,
        },
        args.timeout_sec,
    )
    return response


def main() -> int:
    args = parse_args()
    created_lease = False
    lease_id = args.lease_id
    try:
        if args.scan_first:
            scan_devices(args)
        if not lease_id:
            lease = create_lease(args)
            lease_id = lease.lease_id
            created_lease = True
        if args.heartbeat:
            payload = heartbeat_lease(args, lease_id)
        elif args.release:
            payload = release_lease(args, lease_id)
        else:
            if args.warmup:
                compat_status(args, lease_id)
            payload = compat_status(args, lease_id)
        if isinstance(payload, dict) and created_lease and not args.release:
            payload.setdefault("_hil_lease", {"lease_id": lease_id, "created_here": True})
        print(json.dumps(payload, ensure_ascii=False))
        return 0
    finally:
        if created_lease and args.release is False:
            try:
                release_lease(args, lease_id)
            except Exception:
                pass


if __name__ == "__main__":
    raise SystemExit(main())
