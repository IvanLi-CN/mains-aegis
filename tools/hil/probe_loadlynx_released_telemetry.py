#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import select
import subprocess
import threading
import time
import socket
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any


DEFAULT_LOAD_DEVICE = "loadlynx-d68638"
DEFAULT_LOAD_USB_DEVICE_ID = "digital-2bdfc170893f"
DEFAULT_LOAD_USB_PORT = "/dev/cu.usbmodem212101"
DEFAULT_LOAD_DEVD_BASE_URL = "http://127.0.0.1:20641"
DEFAULT_LOAD_DEVD_SOCKET = "/var/folders/nl/qbk0flf9607bv21rd_7d042c0000gn/T/loadlynx-devd.sock"
DEFAULT_LOAD_CLI = str(Path.home() / ".local" / "bin" / "loadlynx")
DEFAULT_LOAD_BRIDGE_DEVICE = ""
DEFAULT_LOAD_BRIDGE_URL = "http://127.0.0.1:30180"
FORMAL_MIN_SAMPLE_RATE_HZ = 2.0
FORMAL_MAX_SAMPLE_GAP_S = 0.5
SAME_IPC_PROBE_RUNTIME_SEC = 2.0
CLI_STATUS_POLL_RUNTIME_SEC = 2.0


def summarize_timed_rows(
    rows: list[dict[str, Any]],
) -> tuple[list[dict[str, Any]], float | None, float | None, float | None]:
    successful = [row for row in rows if row.get("ok") is True]
    started_values = [
        float(row["started_s"])
        for row in rows
        if isinstance(row.get("started_s"), (int, float))
    ]
    gaps = [
        round(curr - prev, 3)
        for prev, curr in zip(started_values, started_values[1:])
        if curr > prev
    ]
    max_gap_s = max(gaps, default=None)
    elapsed_values = [
        float(row["elapsed_s"])
        for row in rows
        if isinstance(row.get("elapsed_s"), (int, float))
    ]
    max_call_elapsed_s = max(elapsed_values, default=None)
    effective_sample_rate_hz = (
        round(1.0 / max_gap_s, 3)
        if isinstance(max_gap_s, (int, float)) and max_gap_s > 0
        else None
    )
    return successful, effective_sample_rate_hz, max_gap_s, max_call_elapsed_s


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Probe released LoadLynx host telemetry capability against the formal HIL sampling gate."
    )
    parser.add_argument("--load-device", default=DEFAULT_LOAD_DEVICE)
    parser.add_argument("--load-usb-device-id", default=DEFAULT_LOAD_USB_DEVICE_ID)
    parser.add_argument("--load-usb-port", default=DEFAULT_LOAD_USB_PORT)
    parser.add_argument("--load-ipc", default="")
    parser.add_argument("--load-devd-base-url", default=DEFAULT_LOAD_DEVD_BASE_URL)
    parser.add_argument("--load-devd-socket", default=DEFAULT_LOAD_DEVD_SOCKET)
    parser.add_argument("--load-cli", default=DEFAULT_LOAD_CLI)
    parser.add_argument("--load-bridge-device", default=DEFAULT_LOAD_BRIDGE_DEVICE)
    parser.add_argument("--load-bridge-url", default=DEFAULT_LOAD_BRIDGE_URL)
    parser.add_argument("--cli-timeout-sec", type=float, default=30.0)
    parser.add_argument("--http-timeout-sec", type=float, default=10.0)
    parser.add_argument("--http-samples", type=int, default=3)
    parser.add_argument("--http-sleep-sec", type=float, default=0.25)
    return parser.parse_args()


def load_devd_transport_configured(args: argparse.Namespace) -> bool:
    return bool(
        (getattr(args, "load_ipc", "") or "").strip()
        or
        (getattr(args, "load_devd_socket", "") or "").strip()
        or (getattr(args, "load_devd_base_url", "") or "").strip()
    )


def effective_load_bridge_url(args: argparse.Namespace) -> str:
    bridge_url = (getattr(args, "load_bridge_url", "") or "").strip()
    if not bridge_url:
        return ""
    if load_devd_transport_configured(args):
        return ""
    return bridge_url


def resolve_ipc_endpoint(args: argparse.Namespace) -> str:
    return (getattr(args, "load_ipc", "") or "").strip() or (args.load_devd_socket or "").strip()


def has_ipc_endpoint(args: argparse.Namespace) -> bool:
    return bool(resolve_ipc_endpoint(args))


def run(cmd: list[str], timeout_sec: float) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        cmd,
        check=True,
        text=True,
        capture_output=True,
        timeout=timeout_sec,
    )


def sample_stream_jsonl(
    cmd: list[str],
    *,
    runtime_sec: float,
    startup_timeout_sec: float,
) -> dict[str, Any]:
    started_at = time.monotonic()
    process = subprocess.Popen(
        cmd,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    lines: list[dict[str, Any]] = []
    try:
        stdout = process.stdout
        if stdout is None:
            return {
                "ok": False,
                "elapsed_s": round(time.monotonic() - started_at, 3),
                "error": "stream_missing_stdout",
            }
        first_deadline = time.monotonic() + startup_timeout_sec
        end_at = time.monotonic() + runtime_sec
        saw_first_line = False
        while time.monotonic() < end_at:
            wait_timeout = 0.2
            if not saw_first_line:
                wait_timeout = max(0.0, min(wait_timeout, first_deadline - time.monotonic()))
            ready, _, _ = select.select([stdout], [], [], wait_timeout)
            if not ready:
                if not saw_first_line and time.monotonic() >= first_deadline:
                    stderr_text = ""
                    if process.stderr is not None:
                        stderr_text = process.stderr.read().strip()
                    return {
                        "ok": False,
                        "elapsed_s": round(time.monotonic() - started_at, 3),
                        "error": "stream_start_timeout",
                        "stderr": stderr_text or None,
                    }
                continue
            line = stdout.readline()
            if line == "":
                stderr_text = ""
                if process.stderr is not None:
                    stderr_text = process.stderr.read().strip()
                return {
                    "ok": False,
                    "elapsed_s": round(time.monotonic() - started_at, 3),
                    "error": "stream_exited_early",
                    "returncode": process.poll(),
                    "stderr": stderr_text or None,
                    "lines": lines,
                }
            saw_first_line = True
            ts = round(time.monotonic() - started_at, 3)
            raw = line.strip()
            try:
                payload = json.loads(raw)
            except json.JSONDecodeError:
                payload = {"_raw": raw}
            lines.append({"t_s": ts, "payload": payload})
        return {
            "ok": True,
            "elapsed_s": round(time.monotonic() - started_at, 3),
            "lines": lines,
        }
    finally:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=2.0)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=2.0)


def http_json(url: str, *, timeout_sec: float) -> Any:
    with urllib.request.urlopen(url, timeout=timeout_sec) as response:
        return json.load(response)


def http_post_json(url: str, *, timeout_sec: float, payload: dict[str, Any] | None = None) -> Any:
    data = None if payload is None else json.dumps(payload).encode()
    request = urllib.request.Request(
        url,
        data=data,
        method="POST",
        headers={"Content-Type": "application/json"} if data is not None else {},
    )
    with urllib.request.urlopen(request, timeout=timeout_sec) as response:
        return json.load(response)


def http_delete_json(url: str, *, timeout_sec: float) -> Any:
    request = urllib.request.Request(url, method="DELETE")
    with urllib.request.urlopen(request, timeout=timeout_sec) as response:
        return json.load(response)


def time_cli_command(cmd: list[str], *, timeout_sec: float) -> dict[str, Any]:
    started_at = time.monotonic()
    try:
        completed = run(cmd, timeout_sec=timeout_sec)
        elapsed_s = round(time.monotonic() - started_at, 3)
        payload = json.loads(completed.stdout)
        return {
            "ok": True,
            "elapsed_s": elapsed_s,
            "payload": payload,
        }
    except Exception as exc:  # noqa: BLE001
        elapsed_s = round(time.monotonic() - started_at, 3)
        return {
            "ok": False,
            "elapsed_s": elapsed_s,
            "error": repr(exc),
        }


def ipc_call(
    endpoint: str,
    op: str,
    params: dict[str, Any],
    *,
    timeout_sec: float,
) -> dict[str, Any]:
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


def scan_ipc_devices(args: argparse.Namespace) -> dict[str, Any]:
    return ipc_call(
        resolve_ipc_endpoint(args),
        "devices.scan",
        {},
        timeout_sec=max(args.http_timeout_sec, 10.0),
    )


def warm_ipc_status(args: argparse.Namespace, lease_id: str) -> dict[str, Any]:
    return ipc_call(
        resolve_ipc_endpoint(args),
        "compat.status",
        {
            "device_id": args.load_usb_device_id,
            "lease_id": lease_id,
        },
        timeout_sec=max(args.cli_timeout_sec, 20.0),
    )


def measure_cli(args: argparse.Namespace) -> dict[str, Any]:
    ipc_endpoint = resolve_ipc_endpoint(args)
    status_cmd = [args.load_cli]
    control_cmd = [args.load_cli]
    if ipc_endpoint:
        status_cmd.extend(["--ipc", ipc_endpoint])
        control_cmd.extend(["--ipc", ipc_endpoint])
    status = time_cli_command(
        status_cmd + ["status", "--device", args.load_device, "--json"],
        timeout_sec=args.cli_timeout_sec,
    )
    control = time_cli_command(
        control_cmd + ["control", "get", "--device", args.load_device, "--json"],
        timeout_sec=args.cli_timeout_sec,
    )
    return {"status": status, "control": control}


def measure_same_ipc_concurrency(args: argparse.Namespace) -> dict[str, Any]:
    if not has_ipc_endpoint(args):
        return {
            "skipped": True,
            "reason": "load_devd_socket_empty",
        }
    lease_response: dict[str, Any] | None = None
    lease_id: str | None = None
    rows: list[dict[str, Any]] = []
    stop_event = threading.Event()
    probe_window: dict[str, float] = {
        "started_at": 0.0,
        "probe_deadline": 0.0,
    }
    status_timeout_sec = min(FORMAL_MAX_SAMPLE_GAP_S, args.http_timeout_sec)

    def poller() -> None:
        next_poll_at = probe_window["started_at"]
        sample_index = 0
        while not stop_event.is_set():
            now = time.monotonic()
            if now < next_poll_at:
                time.sleep(max(0.0, next_poll_at - now))
            if stop_event.is_set() or time.monotonic() >= probe_window["probe_deadline"]:
                break
            poll_started_at = time.monotonic()
            try:
                payload = ipc_call(
                    resolve_ipc_endpoint(args),
                    "compat.status",
                    {
                        "device_id": args.load_usb_device_id,
                        "lease_id": lease_id,
                    },
                    timeout_sec=status_timeout_sec,
                )
                ok = payload.get("ok") is True and isinstance(payload.get("result"), dict)
                error = None if ok else payload
            except Exception as exc:  # noqa: BLE001
                ok = False
                error = repr(exc)
            poll_finished_at = time.monotonic()
            rows.append(
                {
                    "sample_index": sample_index,
                    "started_s": round(poll_started_at - probe_window["started_at"], 3),
                    "elapsed_s": round(poll_finished_at - poll_started_at, 3),
                    "ok": ok,
                    "error": error,
                }
            )
            sample_index += 1
            next_poll_at += 0.25

    try:
        scan_ipc_devices(args)
        lease_response = ipc_call(
            resolve_ipc_endpoint(args),
            "serial.lease.create",
            {
                "device_id": args.load_usb_device_id,
                "expected_identity_device_id": args.load_device,
            },
            timeout_sec=max(args.cli_timeout_sec, 20.0),
        )
        lease_result = lease_response.get("result") if isinstance(lease_response, dict) else None
        if not isinstance(lease_result, dict) or not isinstance(lease_result.get("lease_id"), str):
            return {
                "lease": lease_response,
                "control": None,
                "samples": [],
                "effective_sample_rate_hz": None,
                "max_sample_gap_s": None,
                "max_call_elapsed_s": None,
                "formal_capable": False,
                "failures": ["same_ipc_lease_create_failed"],
            }
        lease_id = lease_result["lease_id"]
        warm_ipc_status(args, lease_id)
        probe_window["started_at"] = time.monotonic()
        probe_window["probe_deadline"] = (
            probe_window["started_at"] + SAME_IPC_PROBE_RUNTIME_SEC
        )
        poller_thread = threading.Thread(target=poller, name="same-ipc-concurrency-probe", daemon=True)
        poller_thread.start()
        time.sleep(SAME_IPC_PROBE_RUNTIME_SEC)
        stop_event.set()
        poller_thread.join(timeout=5.0)
        control = time_cli_command(
            [
                args.load_cli,
            ]
            + (["--ipc", resolve_ipc_endpoint(args)] if resolve_ipc_endpoint(args) else [])
            + ["control", "get", "--device", args.load_device, "--json"],
            timeout_sec=max(args.cli_timeout_sec, 15.0),
        )
    finally:
        if lease_id:
            try:
                release = ipc_call(
                    resolve_ipc_endpoint(args),
                    "serial.lease.release",
                    {"lease_id": lease_id},
                    timeout_sec=args.http_timeout_sec,
                )
            except Exception as exc:  # noqa: BLE001
                release = {"ok": False, "error": repr(exc)}
        else:
            release = None

    successful, effective_sample_rate_hz, max_gap_s, max_call_elapsed_s = summarize_timed_rows(rows)
    failures: list[str] = []
    if not successful:
        failures.append("same_ipc_no_successful_status_samples")
    if control.get("ok") is not True:
        failures.append("same_ipc_control_get_failed")
    if effective_sample_rate_hz is None or effective_sample_rate_hz < FORMAL_MIN_SAMPLE_RATE_HZ:
        failures.append("same_ipc_sample_rate_below_formal_floor")
    if max_gap_s is None or max_gap_s > FORMAL_MAX_SAMPLE_GAP_S:
        failures.append("same_ipc_sample_gap_above_formal_ceiling")
    if max_call_elapsed_s is None or max_call_elapsed_s > FORMAL_MAX_SAMPLE_GAP_S:
        failures.append("same_ipc_call_duration_above_formal_ceiling")
    return {
        "lease": lease_response,
        "release": release,
        "control": control,
        "sample_count": len(rows),
        "successful_sample_count": len(successful),
        "samples": rows[:40],
        "effective_sample_rate_hz": effective_sample_rate_hz,
        "max_sample_gap_s": max_gap_s,
        "max_call_elapsed_s": max_call_elapsed_s,
        "formal_capable": not failures,
        "failures": failures,
    }


def measure_cli_status_poll_concurrency(args: argparse.Namespace) -> dict[str, Any]:
    if not has_ipc_endpoint(args):
        rows: list[dict[str, Any]] = []
        sample_index = 0
        started_at = time.monotonic()
        deadline = started_at + CLI_STATUS_POLL_RUNTIME_SEC
        next_poll_at = started_at
        while time.monotonic() < deadline:
            now = time.monotonic()
            if now < next_poll_at:
                time.sleep(max(0.0, next_poll_at - now))
            if time.monotonic() >= deadline:
                break
            poll_started_at = time.monotonic()
            result = time_cli_command(
                [args.load_cli, "status", "--device", args.load_device, "--json"],
                timeout_sec=min(FORMAL_MAX_SAMPLE_GAP_S, args.cli_timeout_sec),
            )
            rows.append(
                {
                    "sample_index": sample_index,
                    "started_s": round(poll_started_at - started_at, 3),
                    "elapsed_s": float(result.get("elapsed_s") or 0.0),
                    "ok": result.get("ok") is True,
                    "error": None if result.get("ok") is True else result.get("error"),
                }
            )
            sample_index += 1
            next_poll_at += 0.25
        successful, effective_sample_rate_hz, max_gap_s, max_call_elapsed_s = summarize_timed_rows(rows)
        failures: list[str] = []
        if not successful:
            failures.append("cli_status_poll_no_successful_samples")
        if effective_sample_rate_hz is None or effective_sample_rate_hz < FORMAL_MIN_SAMPLE_RATE_HZ:
            failures.append("cli_status_poll_sample_rate_below_formal_floor")
        if max_gap_s is None or max_gap_s > FORMAL_MAX_SAMPLE_GAP_S:
            failures.append("cli_status_poll_sample_gap_above_formal_ceiling")
        if max_call_elapsed_s is None or max_call_elapsed_s > FORMAL_MAX_SAMPLE_GAP_S:
            failures.append("cli_status_poll_call_duration_above_formal_ceiling")
        return {
            "skipped": False,
            "transport": "released_cli_direct",
            "sample_count": len(rows),
            "successful_sample_count": len(successful),
            "samples": rows[:40],
            "effective_sample_rate_hz": effective_sample_rate_hz,
            "max_sample_gap_s": max_gap_s,
            "max_call_elapsed_s": max_call_elapsed_s,
            "formal_capable": not failures,
            "failures": failures,
        }
    rows: list[dict[str, Any]] = []
    sample_index = 0
    lease_response = None
    release = None
    lease_id: str | None = None
    try:
        scan_ipc_devices(args)
        lease_response = ipc_call(
            resolve_ipc_endpoint(args),
            "serial.lease.create",
            {"device_id": args.load_usb_device_id},
            timeout_sec=max(args.http_timeout_sec, 10.0),
        )
        lease_result = (
            lease_response.get("result")
            if isinstance(lease_response, dict)
            else None
        )
        if not isinstance(lease_result, dict) or not isinstance(lease_result.get("lease_id"), str):
            return {
                "lease": lease_response,
                "release": None,
                "sample_count": 0,
                "successful_sample_count": 0,
                "samples": [],
                "effective_sample_rate_hz": None,
                "max_sample_gap_s": None,
                "max_call_elapsed_s": None,
                "formal_capable": False,
                "failures": ["cli_status_poll_lease_create_failed"],
            }
        lease_id = lease_result["lease_id"]
        warm_ipc_status(args, lease_id)
        started_at = time.monotonic()
        deadline = started_at + CLI_STATUS_POLL_RUNTIME_SEC
        next_poll_at = started_at
        while time.monotonic() < deadline:
            now = time.monotonic()
            if now < next_poll_at:
                time.sleep(max(0.0, next_poll_at - now))
            if time.monotonic() >= deadline:
                break
            poll_started_at = time.monotonic()
            try:
                status = ipc_call(
                    resolve_ipc_endpoint(args),
                    "compat.status",
                    {
                        "device_id": args.load_usb_device_id,
                        "lease_id": lease_id,
                    },
                    timeout_sec=min(FORMAL_MAX_SAMPLE_GAP_S, args.cli_timeout_sec),
                )
                elapsed_s = round(time.monotonic() - poll_started_at, 3)
                payload = status.get("result") if isinstance(status, dict) else None
                ok = status.get("ok") is True and isinstance(payload, dict)
                error = None if ok else repr(status)
            except Exception as exc:  # noqa: BLE001
                elapsed_s = round(time.monotonic() - poll_started_at, 3)
                ok = False
                error = repr(exc)
            rows.append(
                {
                    "sample_index": sample_index,
                    "started_s": round(poll_started_at - started_at, 3),
                    "elapsed_s": elapsed_s,
                    "ok": ok,
                    "error": error,
                }
            )
            sample_index += 1
            next_poll_at += 0.25
    finally:
        if lease_id:
            try:
                release = ipc_call(
                    resolve_ipc_endpoint(args),
                    "serial.lease.release",
                    {"lease_id": lease_id},
                    timeout_sec=args.http_timeout_sec,
                )
            except Exception as exc:  # noqa: BLE001
                release = {"ok": False, "error": repr(exc)}

    successful, effective_sample_rate_hz, max_gap_s, max_call_elapsed_s = summarize_timed_rows(rows)
    failures: list[str] = []
    if not successful:
        failures.append("cli_status_poll_no_successful_samples")
    if effective_sample_rate_hz is None or effective_sample_rate_hz < FORMAL_MIN_SAMPLE_RATE_HZ:
        failures.append("cli_status_poll_sample_rate_below_formal_floor")
    if max_gap_s is None or max_gap_s > FORMAL_MAX_SAMPLE_GAP_S:
        failures.append("cli_status_poll_sample_gap_above_formal_ceiling")
    if max_call_elapsed_s is None or max_call_elapsed_s > FORMAL_MAX_SAMPLE_GAP_S:
        failures.append("cli_status_poll_call_duration_above_formal_ceiling")
    return {
        "lease": lease_response,
        "release": release,
        "sample_count": len(rows),
        "successful_sample_count": len(successful),
        "samples": rows[:40],
        "effective_sample_rate_hz": effective_sample_rate_hz,
        "max_sample_gap_s": max_gap_s,
        "max_call_elapsed_s": max_call_elapsed_s,
        "formal_capable": not failures,
        "failures": failures,
    }


def http_post_json_body(url: str, payload: Any, *, timeout_sec: float) -> Any:
    data = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(
        url,
        data=data,
        method="POST",
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(request, timeout=timeout_sec) as response:
        return json.load(response)


def scan_load_bridge_devices(bridge_url: str, *, timeout_sec: float) -> list[dict[str, Any]]:
    payload = http_post_json(
        f"{bridge_url.rstrip('/')}/api/v1/devices/scan",
        timeout_sec=timeout_sec,
    )
    devices = payload.get("devices")
    if not isinstance(devices, list):
        raise RuntimeError("load bridge scan did not return devices")
    return [device for device in devices if isinstance(device, dict)]


def resolve_load_bridge_device_id(args: argparse.Namespace) -> str:
    bridge_url = (args.load_bridge_url or "").rstrip("/")
    if not bridge_url:
        raise RuntimeError("load_bridge_url is empty")
    cached = getattr(args, "_resolved_load_bridge_device_id", None)
    if isinstance(cached, str) and cached:
        return cached
    explicit = (args.load_bridge_device or "").strip()
    devices = scan_load_bridge_devices(bridge_url, timeout_sec=args.http_timeout_sec)
    if explicit:
        if any(device.get("id") == explicit for device in devices):
            setattr(args, "_resolved_load_bridge_device_id", explicit)
            return explicit
        raise RuntimeError(f"load bridge device not found after scan: {explicit}")
    port_matches = [
        device
        for device in devices
        if ((device.get("digital_target") or {}).get("port_path") == args.load_usb_port)
    ]
    if len(port_matches) == 1:
        resolved = str(port_matches[0].get("id"))
        setattr(args, "_resolved_load_bridge_device_id", resolved)
        return resolved
    identity_matches = [
        device
        for device in devices
        if ((device.get("identity") or {}).get("device_id") == args.load_device)
    ]
    if len(identity_matches) == 1:
        resolved = str(identity_matches[0].get("id"))
        setattr(args, "_resolved_load_bridge_device_id", resolved)
        return resolved
    stable_identity_matches = [
        device
        for device in devices
        if (((device.get("identity") or {}).get("stable_identity") or {}).get("device_id") == args.load_device)
    ]
    if len(stable_identity_matches) == 1:
        resolved = str(stable_identity_matches[0].get("id"))
        setattr(args, "_resolved_load_bridge_device_id", resolved)
        return resolved
    if len(devices) == 1 and isinstance(devices[0].get("id"), str):
        resolved = str(devices[0]["id"])
        setattr(args, "_resolved_load_bridge_device_id", resolved)
        return resolved
    raise RuntimeError(
        "could not resolve load bridge device id from scan; "
        f"load_device={args.load_device} load_usb_port={args.load_usb_port}"
    )


def ensure_load_bridge_device_ready(args: argparse.Namespace, bridge_device: str) -> None:
    bridge_url = (args.load_bridge_url or "").rstrip("/")
    http_post_json(
        f"{bridge_url}/api/v1/devices/{bridge_device}/connect",
        timeout_sec=args.http_timeout_sec,
    )


def acquire_load_bridge_lease(args: argparse.Namespace, bridge_device: str) -> dict[str, Any]:
    bridge_url = (args.load_bridge_url or "").rstrip("/")
    lease = http_post_json_body(
        f"{bridge_url}/api/v1/serial/lease",
        {"device_id": bridge_device},
        timeout_sec=args.http_timeout_sec,
    )
    if isinstance(lease, dict):
        lease.setdefault("bridge_device_id", bridge_device)
    return lease


def release_load_bridge_lease(args: argparse.Namespace, lease_id: str) -> Any:
    bridge_url = (args.load_bridge_url or "").rstrip("/")
    return http_delete_json(
        f"{bridge_url}/api/v1/serial/lease/{lease_id}",
        timeout_sec=args.http_timeout_sec,
    )


def build_load_bridge_cli_url(args: argparse.Namespace, *, lease_id: str, bridge_device: str) -> str:
    bridge_url = (args.load_bridge_url or "").rstrip("/")
    query = urllib.parse.urlencode(
        {
            "device_id": bridge_device,
            "lease_id": lease_id,
        }
    )
    return f"{bridge_url}?{query}"


class BridgeLeaseHeartbeat:
    def __init__(
        self,
        *,
        args: argparse.Namespace,
        lease_id: str,
    ) -> None:
        self._args = args
        self._lease_id = lease_id
        self._stop_event = threading.Event()
        self._thread: threading.Thread | None = None

    def start(self) -> None:
        self._thread = threading.Thread(
            target=self._run,
            name=f"bridge-lease-heartbeat:{self._lease_id}",
            daemon=True,
        )
        self._thread.start()

    def stop(self, *, timeout_sec: float = 2.0) -> None:
        self._stop_event.set()
        if self._thread is not None:
            self._thread.join(timeout=timeout_sec)

    def _run(self) -> None:
        while not self._stop_event.wait(1.0):
            try:
                http_post_json(
                    f"{self._args.load_bridge_url.rstrip('/')}/api/v1/serial/lease/{self._lease_id}",
                    timeout_sec=self._args.http_timeout_sec,
                )
            except Exception:  # noqa: BLE001
                break


def measure_bridge_concurrency(args: argparse.Namespace) -> dict[str, Any]:
    if not effective_load_bridge_url(args):
        return {
            "skipped": True,
            "reason": "load_bridge_url_empty",
        }
    bridge_device = resolve_load_bridge_device_id(args)
    ensure_load_bridge_device_ready(args, bridge_device)
    lease = acquire_load_bridge_lease(args, bridge_device)
    lease_id = lease.get("lease_id") if isinstance(lease, dict) else None
    if not isinstance(lease_id, str) or not lease_id:
        return {
            "lease": lease,
            "control": None,
            "samples": [],
            "effective_sample_rate_hz": None,
            "max_sample_gap_s": None,
            "max_call_elapsed_s": None,
            "formal_capable": False,
            "failures": ["bridge_lease_create_failed"],
        }
    rows: list[dict[str, Any]] = []
    stop_event = threading.Event()
    started_at = time.monotonic()
    status_url = (
        f"{effective_load_bridge_url(args).rstrip('/')}/api/v1/status"
        f"?device_id={urllib.parse.quote(bridge_device)}&lease_id={urllib.parse.quote(lease_id)}"
    )

    def poller() -> None:
        next_poll_at = time.monotonic()
        sample_index = 0
        while not stop_event.is_set():
            now = time.monotonic()
            if now < next_poll_at:
                time.sleep(max(0.0, next_poll_at - now))
            poll_started_at = time.monotonic()
            try:
                payload = http_json(status_url, timeout_sec=min(3.0, args.http_timeout_sec))
                ok = isinstance(payload, dict) and isinstance(payload.get("status"), dict)
                error = None if ok else payload
            except Exception as exc:  # noqa: BLE001
                ok = False
                error = repr(exc)
            poll_finished_at = time.monotonic()
            rows.append(
                {
                    "sample_index": sample_index,
                    "started_s": round(poll_started_at - started_at, 3),
                    "elapsed_s": round(poll_finished_at - poll_started_at, 3),
                    "ok": ok,
                    "error": error,
                }
            )
            sample_index += 1
            next_poll_at += 0.25

    bridge_cli_url = build_load_bridge_cli_url(
        args,
        lease_id=lease_id,
        bridge_device=bridge_device,
    )
    heartbeat = BridgeLeaseHeartbeat(args=args, lease_id=lease_id)
    try:
        heartbeat.start()
        poller_thread = threading.Thread(target=poller, name="bridge-concurrency-probe", daemon=True)
        poller_thread.start()
        time.sleep(1.0)
        control = time_cli_command(
            [
                args.load_cli,
                "--ipc",
                args.load_devd_socket,
                "control",
                "get",
                "--url",
                bridge_cli_url,
                "--json",
            ],
            timeout_sec=max(args.cli_timeout_sec, 15.0),
        )
        time.sleep(1.0)
        stop_event.set()
        poller_thread.join(timeout=5.0)
    finally:
        heartbeat.stop()
        try:
            release = release_load_bridge_lease(args, lease_id)
        except Exception as exc:  # noqa: BLE001
            release = {"ok": False, "error": repr(exc)}

    successful = [row for row in rows if row.get("ok") is True]
    started_values = [
        float(row["started_s"])
        for row in rows
        if isinstance(row.get("started_s"), (int, float))
    ]
    gaps = [
        round(curr - prev, 3)
        for prev, curr in zip(started_values, started_values[1:])
        if curr > prev
    ]
    max_gap_s = max(gaps, default=None)
    elapsed_values = [
        float(row["elapsed_s"])
        for row in rows
        if isinstance(row.get("elapsed_s"), (int, float))
    ]
    max_call_elapsed_s = max(elapsed_values, default=None)
    effective_sample_rate_hz = (
        round(1.0 / max_gap_s, 3)
        if isinstance(max_gap_s, (int, float)) and max_gap_s > 0
        else None
    )
    failures: list[str] = []
    if not successful:
        failures.append("bridge_no_successful_status_samples")
    if control.get("ok") is not True:
        failures.append("bridge_control_get_failed")
    if effective_sample_rate_hz is None or effective_sample_rate_hz < FORMAL_MIN_SAMPLE_RATE_HZ:
        failures.append("bridge_sample_rate_below_formal_floor")
    if max_gap_s is None or max_gap_s > FORMAL_MAX_SAMPLE_GAP_S:
        failures.append("bridge_sample_gap_above_formal_ceiling")
    if max_call_elapsed_s is None or max_call_elapsed_s > FORMAL_MAX_SAMPLE_GAP_S:
        failures.append("bridge_call_duration_above_formal_ceiling")
    return {
        "lease": lease,
        "release": release,
        "control": control,
        "sample_count": len(rows),
        "successful_sample_count": len(successful),
        "samples": rows[:40],
        "effective_sample_rate_hz": effective_sample_rate_hz,
        "max_sample_gap_s": max_gap_s,
        "max_call_elapsed_s": max_call_elapsed_s,
        "formal_capable": not failures,
        "failures": failures,
    }


def ensure_device_connected(args: argparse.Namespace) -> None:
    http_post_json(
        f"{args.load_devd_base_url.rstrip('/')}/api/v1/devices/scan",
        timeout_sec=args.http_timeout_sec,
    )
    http_post_json(
        f"{args.load_devd_base_url.rstrip('/')}/api/v1/devices/{args.load_usb_device_id}/connect",
        timeout_sec=args.http_timeout_sec,
    )


def probe_session_like_endpoint(
    url: str,
    *,
    timeout_sec: float,
) -> dict[str, Any]:
    started_at = time.monotonic()
    try:
        with urllib.request.urlopen(url, timeout=timeout_sec) as response:
            first_chunk = response.read(1)
            return {
                "ok": True,
                "elapsed_s": round(time.monotonic() - started_at, 3),
                "content_type": response.headers.get("content-type"),
                "first_chunk_len": len(first_chunk),
                "first_chunk_preview": first_chunk.decode(errors="replace"),
            }
    except urllib.error.HTTPError as exc:
        body = exc.read().decode(errors="replace")
        return {
            "ok": False,
            "elapsed_s": round(time.monotonic() - started_at, 3),
            "error": f"HTTPError({exc.code})",
            "body": body,
        }
    except socket.timeout as exc:
        return {
            "ok": False,
            "elapsed_s": round(time.monotonic() - started_at, 3),
            "error": repr(exc),
            "kind": "timeout_without_payload",
        }
    except Exception as exc:  # noqa: BLE001
        return {
            "ok": False,
            "elapsed_s": round(time.monotonic() - started_at, 3),
            "error": repr(exc),
        }


def measure_http_status(args: argparse.Namespace) -> dict[str, Any]:
    if not (args.load_devd_base_url or "").strip():
        return {
            "skipped": True,
            "reason": "load_devd_base_url_empty",
        }
    try:
        ensure_device_connected(args)
        connect_result = {"ok": True}
    except Exception as exc:  # noqa: BLE001
        return {
            "connect_result": {"ok": False, "error": repr(exc)},
            "samples": [],
            "release": None,
            "effective_sample_rate_hz": None,
            "max_sample_gap_s": None,
            "formal_capable": False,
            "failures": ["device_connect_failed"],
        }
    try:
        lease = http_post_json(
            f"{args.load_devd_base_url.rstrip('/')}/api/v1/serial/lease",
            timeout_sec=args.http_timeout_sec,
            payload={"device_id": args.load_usb_device_id},
        )
        lease_id = lease["lease_id"]
    except Exception as exc:  # noqa: BLE001
        return {
            "connect_result": connect_result,
            "lease": None,
            "samples": [],
            "release": None,
            "session_probe": None,
            "events_probe": None,
            "effective_sample_rate_hz": None,
            "max_sample_gap_s": None,
            "formal_capable": False,
            "failures": ["lease_acquire_failed"],
            "lease_error": repr(exc),
        }
    rows: list[dict[str, Any]] = []
    try:
        for i in range(args.http_samples):
            started_at = time.monotonic()
            url = (
                f"{args.load_devd_base_url.rstrip('/')}/api/v1/status"
                f"?device_id={args.load_usb_device_id}&lease_id={lease_id}"
            )
            try:
                payload = http_json(url, timeout_sec=args.http_timeout_sec)
                rows.append(
                    {
                        "sample_index": i,
                        "ok": True,
                        "elapsed_s": round(time.monotonic() - started_at, 3),
                        "hello_seen": payload.get("hello_seen"),
                        "link_up": payload.get("link_up"),
                        "v_local_mv": ((payload.get("status") or {}).get("v_local_mv")),
                        "i_remote_ma": ((payload.get("status") or {}).get("i_remote_ma")),
                        "uptime_ms": payload.get("uptime_ms"),
                    }
                )
            except Exception as exc:  # noqa: BLE001
                rows.append(
                    {
                        "sample_index": i,
                        "ok": False,
                        "elapsed_s": round(time.monotonic() - started_at, 3),
                        "error": repr(exc),
                    }
                )
                break
            time.sleep(max(0.0, args.http_sleep_sec))
        session_probe = probe_session_like_endpoint(
            (
                f"{args.load_devd_base_url.rstrip('/')}/api/v1/serial/session"
                f"?device_id={args.load_usb_device_id}&lease_id={lease_id}"
            ),
            timeout_sec=min(3.0, args.http_timeout_sec),
        )
        events_probe = probe_session_like_endpoint(
            (
                f"{args.load_devd_base_url.rstrip('/')}/api/v1/serial/events"
                f"?device_id={args.load_usb_device_id}&lease_id={lease_id}"
            ),
            timeout_sec=min(3.0, args.http_timeout_sec),
        )
    finally:
        try:
            released = http_delete_json(
                f"{args.load_devd_base_url.rstrip('/')}/api/v1/serial/lease/{lease_id}",
                timeout_sec=args.http_timeout_sec,
            )
        except Exception as exc:  # noqa: BLE001
            released = {"ok": False, "error": repr(exc)}
    successful = [row for row in rows if row.get("ok") is True]
    elapsed_samples = [float(row["elapsed_s"]) for row in successful if isinstance(row.get("elapsed_s"), (int, float))]
    effective_sample_rate_hz = None
    max_sample_gap_s = None
    failures: list[str] = []
    if elapsed_samples:
        # Conservative capability metric: one synchronous status call occupies one sample window.
        max_sample_gap_s = round(max(elapsed_samples), 3)
        effective_sample_rate_hz = round(1.0 / max_sample_gap_s, 3) if max_sample_gap_s > 0 else None
    else:
        failures.append("no_successful_http_status_samples")
    if effective_sample_rate_hz is None or effective_sample_rate_hz < FORMAL_MIN_SAMPLE_RATE_HZ:
        failures.append("http_status_sample_rate_below_formal_floor")
    if max_sample_gap_s is None or max_sample_gap_s > FORMAL_MAX_SAMPLE_GAP_S:
        failures.append("http_status_sample_gap_above_formal_ceiling")
    if not session_probe.get("ok"):
        failures.append("serial_session_endpoint_not_live")
    if not events_probe.get("ok"):
        failures.append("serial_events_endpoint_not_live")
    formal_capable = (
        effective_sample_rate_hz is not None
        and effective_sample_rate_hz >= FORMAL_MIN_SAMPLE_RATE_HZ
        and max_sample_gap_s is not None
        and max_sample_gap_s <= FORMAL_MAX_SAMPLE_GAP_S
    )
    return {
        "connect_result": connect_result,
        "lease": lease,
        "samples": rows,
        "release": released,
        "session_probe": session_probe,
        "events_probe": events_probe,
        "effective_sample_rate_hz": effective_sample_rate_hz,
        "max_sample_gap_s": max_sample_gap_s,
        "formal_capable": formal_capable,
        "failures": failures,
    }


def measure_hidden_monitor(args: argparse.Namespace) -> dict[str, Any]:
    if not has_ipc_endpoint(args):
        return {
            "skipped": True,
            "reason": "load_devd_socket_empty",
        }
    cmd = [
        args.load_cli,
        "--ipc",
        resolve_ipc_endpoint(args),
        "status-stream",
        "--device",
        args.load_device,
        "--jsonl",
        "--rate-hz",
        "3",
    ]
    sampled = sample_stream_jsonl(
        cmd,
        runtime_sec=min(3.0, max(1.5, args.cli_timeout_sec)),
        startup_timeout_sec=min(2.0, args.cli_timeout_sec),
    )
    if not sampled.get("ok"):
        stderr_text = sampled.get("stderr")
        if sampled.get("error") == "stream_exited_early" and isinstance(stderr_text, str):
            if "unrecognized subcommand 'status-stream'" in stderr_text:
                return {
                    "skipped": True,
                    "reason": "cli_status_stream_unsupported",
                    "stderr": stderr_text,
                }
        return {
            "ok": False,
            "elapsed_s": sampled.get("elapsed_s"),
            "error": sampled.get("error"),
            "stderr": sampled.get("stderr"),
            "returncode": sampled.get("returncode"),
            "preview": sampled.get("lines"),
        }
    lines = list(sampled.get("lines") or [])
    decoded = [item.get("payload") for item in lines if isinstance(item.get("payload"), dict)]
    status_like = []
    status_line_times: list[float] = []
    for item, line in zip(decoded, lines):
        if item.get("kind") == "status":
            payload = item.get("status")
        elif item.get("source") == "usb_status_stream":
            payload = item.get("status")
        else:
            payload = item.get("item")
        if isinstance(payload, dict) and isinstance(payload.get("status"), dict):
            status_like.append(payload)
            status_line_times.append(float(line.get("t_s")))
    elapsed_between_status_samples = [
        round(curr - prev, 3)
        for prev, curr in zip(status_line_times, status_line_times[1:])
        if curr > prev
    ]
    max_gap_s = max(elapsed_between_status_samples, default=None)
    effective_sample_rate_hz = round(1.0 / max_gap_s, 3) if isinstance(max_gap_s, (int, float)) and max_gap_s > 0 else None
    formal_capable = (
        effective_sample_rate_hz is not None
        and effective_sample_rate_hz >= FORMAL_MIN_SAMPLE_RATE_HZ
        and max_gap_s is not None
        and max_gap_s <= FORMAL_MAX_SAMPLE_GAP_S
    )
    failures: list[str] = []
    if not status_like:
        failures.append("hidden_monitor_no_status_samples")
    if effective_sample_rate_hz is None or effective_sample_rate_hz < FORMAL_MIN_SAMPLE_RATE_HZ:
        failures.append("hidden_monitor_sample_rate_below_formal_floor")
    if max_gap_s is None or max_gap_s > FORMAL_MAX_SAMPLE_GAP_S:
        failures.append("hidden_monitor_sample_gap_above_formal_ceiling")
    return {
        "ok": True,
        "elapsed_s": sampled.get("elapsed_s"),
        "line_count": len(lines),
        "status_sample_count": len(status_like),
        "status_sample_gaps_s": elapsed_between_status_samples,
        "effective_sample_rate_hz": effective_sample_rate_hz,
        "max_sample_gap_s": max_gap_s,
        "formal_capable": formal_capable,
        "failures": failures,
        "preview": decoded[:8],
    }


def build_verdict(
    cli: dict[str, Any],
    cli_status_poll: dict[str, Any],
    http_status: dict[str, Any],
    hidden_monitor: dict[str, Any],
    same_ipc_concurrency: dict[str, Any],
    bridge_concurrency: dict[str, Any],
) -> dict[str, Any]:
    failures: list[str] = []
    warnings: list[str] = []
    cli_status_elapsed = cli["status"].get("elapsed_s")
    if not cli["status"].get("ok"):
        warnings.append("cli_status_failed")
    elif not isinstance(cli_status_elapsed, (int, float)) or cli_status_elapsed > FORMAL_MAX_SAMPLE_GAP_S:
        warnings.append("cli_status_too_slow")
    cli_control_elapsed = cli["control"].get("elapsed_s")
    if not cli["control"].get("ok"):
        warnings.append("cli_control_failed")
    elif not isinstance(cli_control_elapsed, (int, float)) or cli_control_elapsed > FORMAL_MAX_SAMPLE_GAP_S:
        warnings.append("cli_control_too_slow")
    if not bridge_concurrency.get("skipped"):
        primary_probe = bridge_concurrency
        primary_failure = "bridge_concurrency_not_formal_capable"
    elif not cli_status_poll.get("skipped"):
        primary_probe = cli_status_poll
        primary_failure = "cli_status_poll_not_formal_capable"
    elif not same_ipc_concurrency.get("skipped"):
        primary_probe = same_ipc_concurrency
        primary_failure = "same_ipc_concurrency_not_formal_capable"
    else:
        primary_probe = http_status
        primary_failure = "devd_http_status_not_formal_capable"
    if not primary_probe.get("formal_capable"):
        failures.append(primary_failure)
    if primary_probe is not http_status and not http_status.get("formal_capable"):
        if not http_status.get("skipped"):
            warnings.append("devd_http_status_not_formal_capable")
    if not cli_status_poll.get("skipped") and not cli_status_poll.get("formal_capable"):
        warnings.append("cli_status_poll_not_formal_capable")
    if not same_ipc_concurrency.get("skipped") and not same_ipc_concurrency.get("formal_capable"):
        warnings.append("same_ipc_concurrency_not_formal_capable")
    if not hidden_monitor.get("skipped") and not hidden_monitor.get("formal_capable"):
        warnings.append("hidden_monitor_not_formal_capable")
    return {
        "formal_capable": not failures,
        "failures": failures,
        "warnings": warnings,
        "required_min_sample_rate_hz": FORMAL_MIN_SAMPLE_RATE_HZ,
        "required_max_sample_gap_s": FORMAL_MAX_SAMPLE_GAP_S,
    }


def main() -> int:
    args = parse_args()
    bridge_mode = bool(effective_load_bridge_url(args))
    cli = measure_cli(args)
    if bridge_mode:
        http_status = {
            "skipped": True,
            "reason": "bridge_mode_prefers_bridge_concurrency",
        }
        hidden_monitor = {
            "skipped": True,
            "reason": "bridge_mode_prefers_bridge_concurrency",
        }
        same_ipc_concurrency = {
            "skipped": True,
            "reason": "bridge_mode_prefers_bridge_concurrency",
        }
        cli_status_poll = {
            "skipped": True,
            "reason": "bridge_mode_prefers_bridge_concurrency",
        }
        bridge_concurrency = measure_bridge_concurrency(args)
    else:
        http_status = measure_http_status(args)
        hidden_monitor = measure_hidden_monitor(args)
        cli_status_poll = measure_cli_status_poll_concurrency(args)
        same_ipc_concurrency = measure_same_ipc_concurrency(args)
        bridge_concurrency = {
            "skipped": True,
            "reason": "load_bridge_url_empty",
        }
    verdict = build_verdict(
        cli,
        cli_status_poll,
        http_status,
        hidden_monitor,
        same_ipc_concurrency,
        bridge_concurrency,
    )
    payload = {
        "load_device": args.load_device,
        "load_usb_device_id": args.load_usb_device_id,
        "load_devd_base_url": args.load_devd_base_url,
        "load_devd_socket": args.load_devd_socket,
        "cli": cli,
        "cli_status_poll": cli_status_poll,
        "http_status": http_status,
        "hidden_monitor": hidden_monitor,
        "same_ipc_concurrency": same_ipc_concurrency,
        "bridge_concurrency": bridge_concurrency,
        "verdict": verdict,
    }
    print(json.dumps(payload, ensure_ascii=False, indent=2))
    return 0 if verdict["formal_capable"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
