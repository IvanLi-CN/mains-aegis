#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import math
import os
import subprocess
import sys
import threading
import time
import urllib.parse
import urllib.request
import urllib.error
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


DEFAULT_LOAD_BRIDGE_DEVICE = ""
DEFAULT_LOAD_IPC = ""
DEFAULT_LOAD_BRIDGE_URL = ""
DEFAULT_UPS_STATUS_URL = os.environ.get("MAINS_AEGIS_UPS_STATUS_URL")
DEFAULT_DEVD_DIAG_SNAPSHOT_URL = os.environ.get("MAINS_AEGIS_DEVD_DIAG_SNAPSHOT_URL")
DEFAULT_UPS_SETTINGS_URL = os.environ.get("MAINS_AEGIS_UPS_SETTINGS_URL")
DEFAULT_ISOLAPURR_URL = os.environ.get("MAINS_AEGIS_ISOLAPURR_URL")
DEFAULT_PRE_SECONDS = 12.0
DEFAULT_HOLD_SECONDS = 18.0
DEFAULT_POST_SECONDS = 12.0
DEFAULT_INTERVAL_SECONDS = 0.5
DEFAULT_COMMAND_TIMEOUT_SECONDS = 45.0
DEFAULT_STATUS_TIMEOUT_SECONDS = 20.0
DEFAULT_VERIFY_TIMEOUT_SECONDS = 45.0
DEFAULT_MAX_I_MA_TOTAL = 3900
DEFAULT_MAX_P_MW = 45000
DEFAULT_LOAD_REFRESH_SECONDS = 2.0
DEFAULT_LOAD_POLLER_TIMEOUT_SECONDS = 25.0
DEFAULT_LOAD_STATUS_MAX_AGE_SECONDS = 8.0
DEFAULT_BACKUP_HOLD_SECONDS = 18.0
DEFAULT_RESTORE_HOLD_SECONDS = 18.0
DEFAULT_SAMPLE_READ_RETRIES = 3
DEFAULT_SAMPLE_READ_RETRY_DELAY_SECONDS = 1.0
DEFAULT_BACKUP_STIMULUS = "power_off"
DEFAULT_BACKUP_LOW_VOLTAGE_MV = 3000
DEFAULT_BASELINE_SOURCE_VOLTAGE_MV = 12000
DEFAULT_BASELINE_SOURCE_CURRENT_LIMIT_MA = 3000
PORT_C_POWER_PATH = "/api/v1/ports/port_c/power"
PORTS_PATH = "/api/v1/ports"
NO_IN_WINDOW_LOAD_REFRESH_SECONDS = 1_000_000_000.0
LOADLYNX_COMMAND_LOCK = threading.Lock()


class BridgeLeaseHeartbeat:
    def __init__(
        self,
        *,
        bridge_url: str,
        bridge_lease: dict[str, Any] | None,
        timeout_sec: float,
    ) -> None:
        self._bridge_url = bridge_url.rstrip("/")
        self._bridge_lease = dict_or_empty(bridge_lease)
        self._timeout_sec = timeout_sec
        self._stop_event = threading.Event()
        self._thread: threading.Thread | None = None

    def start(self) -> None:
        lease_id = self._bridge_lease.get("lease_id")
        if not isinstance(lease_id, str) or not lease_id:
            return
        interval_sec = 2.0
        heartbeat_interval_ms = self._bridge_lease.get("heartbeat_interval_ms")
        if isinstance(heartbeat_interval_ms, int) and heartbeat_interval_ms > 0:
            interval_sec = max(0.5, heartbeat_interval_ms / 1000.0)
        self._thread = threading.Thread(
            target=self._run,
            args=(lease_id, interval_sec),
            name=f"loadlynx-bridge-heartbeat:{lease_id}",
            daemon=True,
        )
        self._thread.start()

    def stop(self, *, timeout_sec: float = 2.0) -> None:
        self._stop_event.set()
        if self._thread is not None:
            self._thread.join(timeout=timeout_sec)

    def _run(self, lease_id: str, interval_sec: float) -> None:
        while not self._stop_event.wait(interval_sec):
            try:
                heartbeat_load_bridge_lease(
                    self._bridge_url,
                    lease_id,
                    timeout_sec=self._timeout_sec,
                )
            except Exception:  # noqa: BLE001
                break


class ContinuousSampler:
    def __init__(
        self,
        *,
        jsonl_path: Path,
        started_at: float,
        ups_status_url: str,
        devd_diag_snapshot_url: str,
        isolapurr_url: str,
        status_timeout_sec: float,
        load_status_poller: "LoadStatusPoller",
        interval_seconds: float,
        initial_tag: str,
    ) -> None:
        self._jsonl_path = jsonl_path
        self._started_at = started_at
        self._ups_status_url = ups_status_url
        self._devd_diag_snapshot_url = devd_diag_snapshot_url
        self._isolapurr_url = isolapurr_url
        self._status_timeout_sec = status_timeout_sec
        self._load_status_poller = load_status_poller
        self._interval_seconds = max(0.1, interval_seconds)
        self._tag_lock = threading.Lock()
        self._current_tag = initial_tag
        self._samples: list[dict[str, Any]] = []
        self._samples_lock = threading.Lock()
        self._stop_event = threading.Event()
        self._pause_event = threading.Event()
        self._thread: threading.Thread | None = None
        self._error: Exception | None = None
    def start(self) -> None:
        if self._thread is not None:
            return
        self._thread = threading.Thread(
            target=self._run,
            name="hil-continuous-sampler",
            daemon=True,
        )
        self._thread.start()

    def set_tag(self, tag: str) -> None:
        with self._tag_lock:
            self._current_tag = tag

    def pause(self) -> None:
        self._pause_event.set()

    def resume(self) -> None:
        self._pause_event.clear()

    def wait_for_tag_sample_count(
        self,
        tag: str,
        *,
        min_count: int = 1,
        timeout_sec: float = 5.0,
    ) -> bool:
        deadline = time.monotonic() + max(0.1, timeout_sec)
        while time.monotonic() < deadline:
            with self._samples_lock:
                count = sum(1 for sample in self._samples if sample.get("tag") == tag)
            if count >= min_count:
                return True
            time.sleep(0.05)
        return False

    def stop(self, *, timeout_sec: float = 10.0) -> None:
        self._stop_event.set()
        if self._thread is not None:
            self._thread.join(timeout=timeout_sec)
        if self._error is not None:
            raise self._error

    def snapshot_samples(self) -> list[dict[str, Any]]:
        with self._samples_lock:
            return list(self._samples)

    def _run(self) -> None:
        try:
            while not self._stop_event.is_set():
                if self._pause_event.is_set():
                    time.sleep(0.02)
                    continue
                cycle_started_at = time.monotonic()
                with self._tag_lock:
                    tag = self._current_tag
                load_status_snapshot = self._load_status_poller.snapshot(cycle_started_at)
                sample = sample_point(
                    t_s=cycle_started_at - self._started_at,
                    tag=tag,
                    captured_at_utc=datetime.now(timezone.utc).isoformat(),
                    ups_status_url=self._ups_status_url,
                    devd_diag_snapshot_url=self._devd_diag_snapshot_url,
                    isolapurr_url=self._isolapurr_url,
                    status_timeout_sec=self._status_timeout_sec,
                    load_status_snapshot=load_status_snapshot,
                )
                with self._samples_lock:
                    self._samples.append(sample)
                append_jsonl(self._jsonl_path, sample)
                deadline = cycle_started_at + self._interval_seconds
                while not self._stop_event.is_set() and time.monotonic() < deadline:
                    time.sleep(0.02)
        except Exception as exc:  # noqa: BLE001
            self._error = exc


class LoadStatusPoller:
    def __init__(
        self,
        args: argparse.Namespace,
        load_device: str,
        *,
        timeout_sec: float,
        poll_interval_sec: float,
    ) -> None:
        self._args = args
        self._load_device = load_device
        self._bridge_device = (
            resolve_load_bridge_device_id(args, timeout_sec=timeout_sec)
            if args.load_bridge_url
            else load_device
        )
        self._timeout_sec = timeout_sec
        self._poll_interval_sec = max(0.1, poll_interval_sec)
        self._state_lock = threading.Lock()
        self._stop_event = threading.Event()
        self._pause_event = threading.Event()
        self._idle_event = threading.Event()
        self._idle_event.set()
        self._thread: threading.Thread | None = None
        self._latest_status: Any = None
        self._fetched_at_monotonic: float | None = None
        self._generation = 0
        self._device_generation: int | None = None
        self._device_sampled_at_ms: int | None = None
        self._error: str | None = None
        self._bridge_lease_id: str | None = None
        self._bridge_lease_acquired_at_monotonic: float | None = None
        self._bridge_lease_ttl_ms: int | None = None
        self._bridge_lease_heartbeat_interval_ms: int | None = None

    def replace_status(self, payload: Any) -> None:
        normalized = normalize_load_status_payload(payload)
        valid_status = is_valid_load_status_payload(normalized)
        device_generation = extract_load_status_device_generation(normalized)
        device_sampled_at_ms = extract_load_status_sampled_at_ms(normalized)
        with self._state_lock:
            if valid_status:
                self._latest_status = normalized
                self._fetched_at_monotonic = time.monotonic()
                if isinstance(device_generation, int):
                    self._device_generation = device_generation
                if isinstance(device_sampled_at_ms, int):
                    self._device_sampled_at_ms = device_sampled_at_ms
                self._generation = effective_load_generation(
                    self._generation,
                    self._device_generation,
                )
                self._error = None
            elif isinstance(normalized, dict):
                self._error = normalized.get("error")

    def start(self) -> None:
        if self._thread is not None:
            return
        self._thread = threading.Thread(
            target=self._run,
            name=f"loadlynx-status-poller:{self._load_device}",
            daemon=True,
        )
        self._thread.start()

    def pause(self) -> None:
        self._pause_event.set()

    def resume(self) -> None:
        self._pause_event.clear()

    def wait_until_idle(self, timeout_sec: float) -> bool:
        return self._idle_event.wait(timeout=timeout_sec)

    def bridge_lease_snapshot(self) -> dict[str, Any] | None:
        if not self._args.load_bridge_url:
            return None
        lease_id = self._bridge_lease_id
        if not isinstance(lease_id, str) or not lease_id:
            return None
        return {
            "lease_id": lease_id,
            "bridge_device_id": self._bridge_device,
            "lease_ttl_ms": self._bridge_lease_ttl_ms,
            "heartbeat_interval_ms": self._bridge_lease_heartbeat_interval_ms,
        }

    def wait_for_bridge_lease(self, timeout_sec: float) -> dict[str, Any] | None:
        deadline = time.monotonic() + max(0.1, timeout_sec)
        while time.monotonic() < deadline:
            lease = self.bridge_lease_snapshot()
            if lease is not None:
                return lease
            time.sleep(0.05)
        return self.bridge_lease_snapshot()

    def snapshot(self, now_monotonic: float) -> dict[str, Any]:
        with self._state_lock:
            fetched_at_monotonic = self._fetched_at_monotonic
            age_s = None
            if isinstance(fetched_at_monotonic, (int, float)):
                age_s = round(max(0.0, now_monotonic - fetched_at_monotonic), 3)
            sample_age_s = age_s
            if isinstance(self._device_sampled_at_ms, int):
                sample_age_s = round(
                    max(0.0, time.time() - (self._device_sampled_at_ms / 1000.0)),
                    3,
                )
            return {
                "status": self._latest_status,
                "generation": self._generation,
                "age_s": age_s,
                "sample_age_s": sample_age_s,
                "device_generation": self._device_generation,
                "device_sampled_at_ms": self._device_sampled_at_ms,
                "error": self._error,
                "poller_paused": self._pause_event.is_set(),
                "poller_idle": self._idle_event.is_set(),
            }

    def stop(self, *, timeout_sec: float = 5.0) -> None:
        self._stop_event.set()
        self._pause_event.clear()
        if self._thread is not None:
            self._thread.join(timeout=timeout_sec)
        self.release_bridge_lease(timeout_sec=min(timeout_sec, self._timeout_sec))

    def release_bridge_lease(self, *, timeout_sec: float | None = None) -> None:
        lease_id = self._bridge_lease_id
        self._bridge_lease_id = None
        self._bridge_lease_acquired_at_monotonic = None
        self._bridge_lease_ttl_ms = None
        self._bridge_lease_heartbeat_interval_ms = None
        if not isinstance(lease_id, str) or not lease_id:
            return
        release_load_bridge_lease_quietly(
            self._args,
            {"lease_id": lease_id},
            timeout_sec=min(timeout_sec or self._timeout_sec, self._timeout_sec),
        )

    def _run(self) -> None:
        while not self._stop_event.is_set():
            if self._pause_event.is_set():
                time.sleep(0.05)
                continue
            cycle_started_at = time.monotonic()
            self._idle_event.clear()
            try:
                bridge_lease = self._ensure_bridge_lease()
                if self._args.load_bridge_url:
                    status = get_load_status_via_bridge(
                        self._args,
                        self._load_device,
                        timeout_sec=min(self._timeout_sec, 5.0),
                        bridge_lease=bridge_lease,
                        retries=1,
                        retry_delay_sec=0.0,
                    )
                else:
                    status = get_load_status(
                        self._args,
                        self._load_device,
                        timeout_sec=self._timeout_sec,
                        bridge_lease=bridge_lease,
                    )
                normalized = normalize_load_status_payload(status)
                valid_status = is_valid_load_status_payload(normalized)
                device_generation = extract_load_status_device_generation(normalized)
                device_sampled_at_ms = extract_load_status_sampled_at_ms(normalized)
                with self._state_lock:
                    if valid_status:
                        self._latest_status = normalized
                        self._fetched_at_monotonic = time.monotonic()
                        if isinstance(device_generation, int):
                            self._device_generation = device_generation
                        if isinstance(device_sampled_at_ms, int):
                            self._device_sampled_at_ms = device_sampled_at_ms
                        self._generation = effective_load_generation(
                            self._generation,
                            self._device_generation,
                        )
                        self._error = None
                    else:
                        self._error = "invalid_status_payload"
                if not valid_status:
                    self.release_bridge_lease(timeout_sec=min(self._timeout_sec, 5.0))
            except (
                subprocess.TimeoutExpired,
                subprocess.CalledProcessError,
                json.JSONDecodeError,
            ) as exc:
                with self._state_lock:
                    self._error = repr(exc)
            except Exception as exc:  # noqa: BLE001
                self.release_bridge_lease(timeout_sec=min(self._timeout_sec, 5.0))
                with self._state_lock:
                    self._error = repr(exc)
            finally:
                self._idle_event.set()
            deadline = cycle_started_at + self._poll_interval_sec
            while not self._stop_event.is_set() and time.monotonic() < deadline:
                if self._pause_event.is_set():
                    break
                time.sleep(0.05)

    def _ensure_bridge_lease(self) -> dict[str, Any] | None:
        bridge_url = (self._args.load_bridge_url or "").rstrip("/")
        if not bridge_url:
            return None
        now = time.monotonic()
        if (
            self._bridge_lease_id
            and self._bridge_lease_acquired_at_monotonic is not None
            and self._bridge_lease_ttl_ms is not None
        ):
            ttl_s = self._bridge_lease_ttl_ms / 1000.0
            heartbeat_s = ttl_s / 4.0
            if isinstance(self._bridge_lease_heartbeat_interval_ms, int):
                heartbeat_s = max(
                    0.5,
                    self._bridge_lease_heartbeat_interval_ms / 1000.0,
                )
            lease_age_s = now - self._bridge_lease_acquired_at_monotonic
            if lease_age_s >= heartbeat_s:
                try:
                    heartbeat_load_bridge_lease(
                        bridge_url,
                        self._bridge_lease_id,
                        timeout_sec=self._timeout_sec,
                    )
                    self._bridge_lease_acquired_at_monotonic = now
                    return {"lease_id": self._bridge_lease_id}
                except Exception:  # noqa: BLE001
                    self._bridge_lease_id = None
                    self._bridge_lease_acquired_at_monotonic = None
                    self._bridge_lease_ttl_ms = None
                    self._bridge_lease_heartbeat_interval_ms = None
            if (
                self._bridge_lease_acquired_at_monotonic is not None
                and now - self._bridge_lease_acquired_at_monotonic < max(1.0, ttl_s / 2.0)
            ):
                return {"lease_id": self._bridge_lease_id}
        ensure_load_bridge_device_ready(
            bridge_url,
            self._bridge_device,
            timeout_sec=self._timeout_sec,
        )
        lease = acquire_load_bridge_lease(
            self._args,
            timeout_sec=self._timeout_sec,
            bridge_device=self._bridge_device,
        )
        lease_id = lease.get("lease_id")
        if isinstance(lease_id, str) and lease_id:
            self._bridge_lease_id = lease_id
            self._bridge_lease_acquired_at_monotonic = now
            ttl_ms = lease.get("lease_ttl_ms")
            self._bridge_lease_ttl_ms = ttl_ms if isinstance(ttl_ms, int) else 8000
            heartbeat_ms = lease.get("heartbeat_interval_ms")
            self._bridge_lease_heartbeat_interval_ms = (
                heartbeat_ms if isinstance(heartbeat_ms, int) else None
            )
            return {"lease_id": lease_id}
        return None


def verified_load_payload_to_status_payload(payload: Any) -> Any:
    if not isinstance(payload, dict):
        return payload
    if is_valid_load_status_payload(normalize_load_status_payload(payload)):
        return payload
    status_payload = payload.get("status")
    if isinstance(status_payload, dict):
        return status_payload
    return payload


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Capture a time series around one LoadLynx CC transition for 12V DCIN HIL."
    )
    parser.add_argument("--profile-name", required=True)
    parser.add_argument("--target-ma", type=int, required=True)
    parser.add_argument("--load-device", default=os.environ.get("MAINS_AEGIS_LOAD_DEVICE_ID"))
    parser.add_argument("--load-bridge-device", default=DEFAULT_LOAD_BRIDGE_DEVICE)
    parser.add_argument("--load-usb-port", default=os.environ.get("MAINS_AEGIS_LOAD_USB_PORT"))
    parser.add_argument("--load-ipc", default=DEFAULT_LOAD_IPC)
    parser.add_argument("--load-bridge-url", default=DEFAULT_LOAD_BRIDGE_URL)
    parser.add_argument("--ups-status-url", default=DEFAULT_UPS_STATUS_URL)
    parser.add_argument("--ups-settings-url", default=DEFAULT_UPS_SETTINGS_URL)
    parser.add_argument("--devd-diag-snapshot-url", default=DEFAULT_DEVD_DIAG_SNAPSHOT_URL)
    parser.add_argument("--isolapurr-url", default=DEFAULT_ISOLAPURR_URL)
    parser.add_argument("--pre-seconds", type=float, default=DEFAULT_PRE_SECONDS)
    parser.add_argument("--hold-seconds", type=float, default=DEFAULT_HOLD_SECONDS)
    parser.add_argument("--post-seconds", type=float, default=DEFAULT_POST_SECONDS)
    parser.add_argument("--interval-seconds", type=float, default=DEFAULT_INTERVAL_SECONDS)
    parser.add_argument(
        "--command-timeout-sec", type=float, default=DEFAULT_COMMAND_TIMEOUT_SECONDS
    )
    parser.add_argument(
        "--status-timeout-sec", type=float, default=DEFAULT_STATUS_TIMEOUT_SECONDS
    )
    parser.add_argument(
        "--verify-timeout-sec", type=float, default=DEFAULT_VERIFY_TIMEOUT_SECONDS
    )
    parser.add_argument("--max-i-ma-total", type=int, default=DEFAULT_MAX_I_MA_TOTAL)
    parser.add_argument("--max-p-mw", type=int, default=DEFAULT_MAX_P_MW)
    parser.add_argument("--load-refresh-seconds", type=float, default=DEFAULT_LOAD_REFRESH_SECONDS)
    parser.add_argument(
        "--load-poller-timeout-sec",
        type=float,
        default=DEFAULT_LOAD_POLLER_TIMEOUT_SECONDS,
    )
    parser.add_argument(
        "--load-status-max-age-seconds",
        type=float,
        default=DEFAULT_LOAD_STATUS_MAX_AGE_SECONDS,
    )
    parser.add_argument("--include-backup", action="store_true")
    parser.add_argument("--backup-hold-seconds", type=float, default=DEFAULT_BACKUP_HOLD_SECONDS)
    parser.add_argument("--restore-hold-seconds", type=float, default=DEFAULT_RESTORE_HOLD_SECONDS)
    parser.add_argument(
        "--backup-stimulus",
        choices=("power_off", "low_voltage"),
        default=DEFAULT_BACKUP_STIMULUS,
    )
    parser.add_argument(
        "--backup-low-voltage-mv",
        type=int,
        default=DEFAULT_BACKUP_LOW_VOLTAGE_MV,
    )
    parser.add_argument(
        "--baseline-source-voltage-mv",
        type=int,
        default=DEFAULT_BASELINE_SOURCE_VOLTAGE_MV,
    )
    parser.add_argument(
        "--baseline-source-current-limit-ma",
        type=int,
        default=DEFAULT_BASELINE_SOURCE_CURRENT_LIMIT_MA,
    )
    parser.add_argument("--report-root", default="tools/hil/reports")
    args = parser.parse_args()
    for name, option in (
        ("load_device", "--load-device or MAINS_AEGIS_LOAD_DEVICE_ID"),
        ("load_usb_port", "--load-usb-port or MAINS_AEGIS_LOAD_USB_PORT"),
        ("ups_status_url", "--ups-status-url or MAINS_AEGIS_UPS_STATUS_URL"),
        ("ups_settings_url", "--ups-settings-url or MAINS_AEGIS_UPS_SETTINGS_URL"),
        ("devd_diag_snapshot_url", "--devd-diag-snapshot-url or MAINS_AEGIS_DEVD_DIAG_SNAPSHOT_URL"),
        ("isolapurr_url", "--isolapurr-url or MAINS_AEGIS_ISOLAPURR_URL"),
    ):
        if not (getattr(args, name, None) or "").strip():
            parser.error(f"capture requires {option}; no hardware target is built in")
    return args


def run(cmd: list[str], *, timeout_sec: float | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        cmd,
        check=True,
        text=True,
        capture_output=True,
        timeout=timeout_sec,
    )


def run_loadlynx(
    cmd: list[str], *, timeout_sec: float | None = None
) -> subprocess.CompletedProcess[str]:
    # The released loadlynx CLI uses a single USB owner path.
    # Keep all loadlynx invocations serialized inside this process so the scene
    # logic does not race `cc` / `disable` / `control get` and poison HIL runs.
    with LOADLYNX_COMMAND_LOCK:
        return run(cmd, timeout_sec=timeout_sec)


def loadlynx_cmd(args: argparse.Namespace, *parts: str) -> list[str]:
    cmd = ["loadlynx"]
    if args.load_ipc:
        cmd.extend(["--ipc", args.load_ipc])
    cmd.extend(parts)
    return cmd


def http_json(url: str) -> Any:
    with urllib.request.urlopen(url, timeout=10) as response:
        return json.load(response)


def http_post_json(url: str) -> Any:
    request = urllib.request.Request(url, method="POST")
    with urllib.request.urlopen(request, timeout=10) as response:
        return json.load(response)


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


def http_post_empty(url: str, *, timeout_sec: float) -> Any:
    request = urllib.request.Request(url, method="POST")
    with urllib.request.urlopen(request, timeout=timeout_sec) as response:
        return json.load(response)


def scan_load_bridge_devices(bridge_url: str, *, timeout_sec: float) -> list[dict[str, Any]]:
    payload = http_post_empty(
        f"{bridge_url.rstrip('/')}/api/v1/devices/scan",
        timeout_sec=timeout_sec,
    )
    devices = dict_or_empty(payload).get("devices")
    if not isinstance(devices, list):
        raise RuntimeError("load bridge scan did not return devices")
    return [dict_or_empty(device) for device in devices]


def resolve_load_bridge_device_id(args: argparse.Namespace, *, timeout_sec: float) -> str:
    cached = getattr(args, "_resolved_load_bridge_device_id", None)
    if isinstance(cached, str) and cached:
        return cached
    explicit = (args.load_bridge_device or "").strip()
    bridge_url = (args.load_bridge_url or "").rstrip("/")
    if not bridge_url:
        raise RuntimeError("load_bridge_url is empty")
    devices = scan_load_bridge_devices(bridge_url, timeout_sec=timeout_sec)
    if explicit:
        if any(device.get("id") == explicit for device in devices):
            setattr(args, "_resolved_load_bridge_device_id", explicit)
            return explicit
        raise RuntimeError(f"load bridge device not found after scan: {explicit}")
    port_matches = [
        device
        for device in devices
        if dict_or_empty(device.get("digital_target")).get("port_path") == args.load_usb_port
    ]
    if len(port_matches) == 1:
        resolved = str(port_matches[0].get("id"))
        setattr(args, "_resolved_load_bridge_device_id", resolved)
        return resolved
    identity_matches = [
        device
        for device in devices
        if dict_or_empty(device.get("identity")).get("device_id") == args.load_device
    ]
    if len(identity_matches) == 1:
        resolved = str(identity_matches[0].get("id"))
        setattr(args, "_resolved_load_bridge_device_id", resolved)
        return resolved
    stable_identity_matches = [
        device
        for device in devices
        if dict_or_empty(
            dict_or_empty(device.get("identity")).get("stable_identity")
        ).get("device_id")
        == args.load_device
    ]
    if len(stable_identity_matches) == 1:
        resolved = str(stable_identity_matches[0].get("id"))
        setattr(args, "_resolved_load_bridge_device_id", resolved)
        return resolved
    raise RuntimeError(
        "could not resolve load bridge device id from scan; "
        f"load_device={args.load_device} load_usb_port={args.load_usb_port}"
    )


def build_load_bridge_cli_url(
    args: argparse.Namespace,
    *,
    timeout_sec: float,
    bridge_lease: dict[str, Any] | None = None,
    bridge_device: str | None = None,
) -> str:
    bridge_url = (args.load_bridge_url or "").rstrip("/")
    if not bridge_url:
        raise RuntimeError("load_bridge_url is empty")
    resolved_device = (
        bridge_device
        or dict_or_empty(bridge_lease).get("bridge_device_id")
        or resolve_load_bridge_device_id(args, timeout_sec=timeout_sec)
    )
    query_pairs: list[tuple[str, str]] = [("device_id", str(resolved_device))]
    lease_id = dict_or_empty(bridge_lease).get("lease_id")
    if isinstance(lease_id, str) and lease_id:
        query_pairs.append(("lease_id", lease_id))
    return f"{bridge_url}?{urllib.parse.urlencode(query_pairs)}"


def build_load_bridge_status_url(
    args: argparse.Namespace,
    *,
    timeout_sec: float,
    bridge_lease: dict[str, Any] | None = None,
    bridge_device: str | None = None,
) -> str:
    cli_url = build_load_bridge_cli_url(
        args,
        timeout_sec=timeout_sec,
        bridge_lease=bridge_lease,
        bridge_device=bridge_device,
    )
    parsed = urllib.parse.urlparse(cli_url)
    bridge_url = (args.load_bridge_url or "").rstrip("/")
    query = parsed.query
    if query:
        return f"{bridge_url}/api/v1/status?{query}"
    return f"{bridge_url}/api/v1/status"


def run_loadlynx_bridge_command(
    args: argparse.Namespace,
    bridge_lease: dict[str, Any] | None,
    *parts: str,
    timeout_sec: float | None = None,
) -> subprocess.CompletedProcess[str]:
    bridge_url = (args.load_bridge_url or "").rstrip("/")
    heartbeat = BridgeLeaseHeartbeat(
        bridge_url=bridge_url,
        bridge_lease=bridge_lease,
        timeout_sec=min(timeout_sec or DEFAULT_COMMAND_TIMEOUT_SECONDS, 5.0),
    )
    heartbeat.start()
    try:
        cmd = loadlynx_cmd(
            args,
            *parts,
            "--url",
            build_load_bridge_cli_url(
                args,
                timeout_sec=min(timeout_sec or DEFAULT_COMMAND_TIMEOUT_SECONDS, 5.0),
                bridge_lease=bridge_lease,
            ),
        )
        return run_loadlynx(cmd, timeout_sec=timeout_sec)
    finally:
        heartbeat.stop()


def acquire_load_bridge_lease(
    args: argparse.Namespace,
    *,
    timeout_sec: float,
    bridge_device: str | None = None,
) -> dict[str, Any]:
    bridge_url = (args.load_bridge_url or "").rstrip("/")
    if not bridge_url:
        raise RuntimeError("load_bridge_url is empty")
    target_device = bridge_device or resolve_load_bridge_device_id(args, timeout_sec=timeout_sec)
    ensure_load_bridge_device_ready(
        bridge_url,
        target_device,
        timeout_sec=timeout_sec,
    )
    lease = http_post_json_body(
        f"{bridge_url}/api/v1/serial/lease",
        {"device_id": target_device},
        timeout_sec=timeout_sec,
    )
    payload = dict_or_empty(lease)
    payload.setdefault("bridge_device_id", target_device)
    return payload


def heartbeat_load_bridge_lease(
    bridge_url: str,
    lease_id: str,
    *,
    timeout_sec: float,
) -> dict[str, Any]:
    payload = http_post_empty(
        f"{bridge_url}/api/v1/serial/lease/{urllib.parse.quote(lease_id, safe='')}",
        timeout_sec=timeout_sec,
    )
    return dict_or_empty(payload)


def release_load_bridge_lease(
    bridge_url: str,
    lease_id: str,
    *,
    timeout_sec: float,
) -> dict[str, Any]:
    request = urllib.request.Request(
        f"{bridge_url.rstrip('/')}/api/v1/serial/lease/{urllib.parse.quote(lease_id, safe='')}",
        method="DELETE",
    )
    with urllib.request.urlopen(request, timeout=timeout_sec) as response:
        return json.load(response)


def release_load_bridge_lease_quietly(
    args: argparse.Namespace,
    bridge_lease: dict[str, Any] | None,
    *,
    timeout_sec: float,
) -> None:
    bridge_url = (args.load_bridge_url or "").rstrip("/")
    lease_id = dict_or_empty(bridge_lease).get("lease_id")
    if not bridge_url or not isinstance(lease_id, str) or not lease_id:
        return
    try:
        release_load_bridge_lease(
            bridge_url,
            lease_id,
            timeout_sec=timeout_sec,
        )
    except Exception:  # noqa: BLE001
        return


def ensure_load_bridge_device_ready(
    bridge_url: str,
    bridge_device: str,
    *,
    timeout_sec: float,
) -> None:
    devices = scan_load_bridge_devices(bridge_url, timeout_sec=timeout_sec)
    if not any(device.get("id") == bridge_device for device in devices):
        raise RuntimeError(f"load bridge device not found after scan: {bridge_device}")
    http_post_empty(
        f"{bridge_url}/api/v1/devices/{urllib.parse.quote(bridge_device, safe='')}/connect",
        timeout_sec=timeout_sec,
    )


def http_json_with_retries(
    url: str,
    *,
    timeout_sec: float,
    retries: int = DEFAULT_SAMPLE_READ_RETRIES,
    retry_delay_sec: float = DEFAULT_SAMPLE_READ_RETRY_DELAY_SECONDS,
) -> Any:
    last_exc: Exception | None = None
    for attempt in range(max(1, retries)):
        try:
            with urllib.request.urlopen(url, timeout=timeout_sec) as response:
                return json.load(response)
        except Exception as exc:  # noqa: BLE001
            last_exc = exc
            if attempt + 1 < max(1, retries):
                time.sleep(retry_delay_sec)
    raise RuntimeError(f"http fetch failed after retries: url={url!r} error={last_exc!r}")


def derive_diag_snapshot_from_status(status: Any, *, source: str = "lan_derived") -> dict[str, Any]:
    status_dict = dict_or_empty(status)
    input_section = dict_or_empty(status_dict.get("input"))
    charger_section = dict_or_empty(status_dict.get("charger"))
    return {
        "source": source,
        "input": {
            "assist_power_stage": input_section.get("assist_power_stage"),
            "assist_target_vout_mv": input_section.get("assist_target_vout_mv"),
            "input_ibus_ma": input_section.get("input_ibus_ma"),
            "input_vbus_mv": input_section.get("input_vbus_mv"),
            "mains_present": input_section.get("mains_present"),
            "pressure_reason": input_section.get("pressure_reason"),
            "pressure_score_pct": input_section.get("pressure_score_pct"),
            "pressure_state": input_section.get("pressure_state"),
            "source": input_section.get("source"),
            "tps_limit_threshold_ma": input_section.get("tps_limit_threshold_ma"),
            "tps_total_iout_ma": input_section.get("tps_total_iout_ma"),
            "vin_baseline_mv": input_section.get("vin_baseline_mv"),
            "vin_drop_mv": input_section.get("vin_drop_mv"),
            "vin_iin_ma": input_section.get("vin_iin_ma"),
            "vin_vbus_mv": input_section.get("vin_vbus_mv"),
        },
        "charger": {
            "allow_charge": charger_section.get("allow_charge"),
        },
        "policy": {
            "detail_status": charger_section.get("detail_status"),
            "input_source": input_section.get("source"),
            "pressure_state": input_section.get("pressure_state"),
        },
    }


def fetch_diag_snapshot_with_fallback(
    *,
    devd_diag_snapshot_url: str,
    ups_status: dict[str, Any],
    timeout_sec: float,
) -> dict[str, Any]:
    try:
        diag_snapshot = http_json_with_retries(
            devd_diag_snapshot_url,
            timeout_sec=timeout_sec,
        )
        if isinstance(diag_snapshot, dict):
            diag_snapshot.setdefault("source", "devd")
        return dict_or_empty(diag_snapshot)
    except Exception as exc:  # noqa: BLE001
        derived = derive_diag_snapshot_from_status(ups_status)
        derived["fallback_error"] = repr(exc)
        return derived


def run_json_command_with_retries(
    cmd: list[str],
    *,
    timeout_sec: float,
    retries: int = DEFAULT_SAMPLE_READ_RETRIES,
    retry_delay_sec: float = DEFAULT_SAMPLE_READ_RETRY_DELAY_SECONDS,
) -> Any:
    last_exc: Exception | None = None
    for attempt in range(max(1, retries)):
        try:
            return json.loads(run(cmd, timeout_sec=timeout_sec).stdout)
        except (
            subprocess.CalledProcessError,
            subprocess.TimeoutExpired,
            json.JSONDecodeError,
        ) as exc:
            last_exc = exc
            if attempt + 1 < max(1, retries):
                time.sleep(retry_delay_sec)
    raise RuntimeError(f"command failed after retries: cmd={cmd!r} error={last_exc!r}")


def fetch_isolapurr_power_show(
    isolapurr_url: str,
    *,
    timeout_sec: float,
) -> dict[str, Any]:
    return dict_or_empty(
        run_json_command_with_retries(
            ["isolapurr", "power", "show", "--url", isolapurr_url, "--json"],
            timeout_sec=timeout_sec,
        )
    )


def fetch_isolapurr_power_show_best_effort(
    isolapurr_url: str,
    *,
    timeout_sec: float,
) -> dict[str, Any]:
    try:
        payload = fetch_isolapurr_power_show(
            isolapurr_url,
            timeout_sec=timeout_sec,
        )
        payload.setdefault("source", "cli_power_show")
        return payload
    except Exception as exc:  # noqa: BLE001
        return {
            "source": "cli_power_show_error",
            "error": repr(exc),
        }


def write_json(path: Path, payload: Any) -> None:
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def append_jsonl(path: Path, payload: Any) -> None:
    with path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(payload, ensure_ascii=False) + "\n")


def dict_or_empty(payload: Any) -> dict[str, Any]:
    return payload if isinstance(payload, dict) else {}


def unwrap_diag_snapshot_payload(payload: Any) -> dict[str, Any]:
    data = dict_or_empty(payload)
    packages = dict_or_empty(data.get("packages"))
    derived = dict_or_empty(packages.get("derived.power"))
    derived_payload = dict_or_empty(derived.get("payload"))
    if derived_payload:
        return derived_payload
    return data


def first_numeric(*values: Any) -> int | float | None:
    for value in values:
        if isinstance(value, (int, float)):
            return value
    return None


def first_bool(*values: Any) -> bool | None:
    for value in values:
        if isinstance(value, bool):
            return value
    return None


def first_non_empty_string(*values: Any) -> str | None:
    for value in values:
        if isinstance(value, str) and value != "":
            return value
    return None


def ensure_usb_port(
    args: argparse.Namespace,
    load_device: str,
    load_usb_port: str,
    *,
    timeout_sec: float,
) -> dict[str, Any]:
    try:
        completed = run_loadlynx(
            loadlynx_cmd(args, "devices", "--json"),
            timeout_sec=timeout_sec,
        )
        payload = json.loads(completed.stdout)
    except (subprocess.TimeoutExpired, subprocess.CalledProcessError, json.JSONDecodeError) as exc:
        return {
            "device_id": load_device,
            "expected_usb_port": load_usb_port,
            "verified": False,
            "degraded_verification": True,
            "error": repr(exc),
        }
    devices = payload.get("devices", [])
    matched = next((device for device in devices if device.get("id") == load_device), None)
    if matched is None:
        raise RuntimeError(f"saved LoadLynx device not found: {load_device}")
    usb_transport = (matched.get("transports") or {}).get("usb")
    if not usb_transport:
        raise RuntimeError(f"saved LoadLynx device has no usb transport: {load_device}")
    actual_port = usb_transport.get("port_path")
    if actual_port != load_usb_port:
        raise RuntimeError(
            f"LoadLynx usb port mismatch for {load_device}: expected {load_usb_port}, got {actual_port}"
        )
    if matched.get("last_transport") != "usb":
        raise RuntimeError(
            f"LoadLynx last transport is not usb for {load_device}: {matched.get('last_transport')}"
        )
    return {
        "device_id": load_device,
        "usb_port": actual_port,
        "last_transport": matched.get("last_transport"),
        "verified": True,
    }


def get_load_status_via_bridge(
    args: argparse.Namespace,
    load_device: str,
    *,
    timeout_sec: float,
    bridge_lease: dict[str, Any] | None,
    retries: int = DEFAULT_SAMPLE_READ_RETRIES,
    retry_delay_sec: float = DEFAULT_SAMPLE_READ_RETRY_DELAY_SECONDS,
) -> Any:
    status_url = build_load_bridge_status_url(
        args,
        timeout_sec=timeout_sec,
        bridge_lease=bridge_lease,
    )
    payload = http_json_with_retries(
        status_url,
        timeout_sec=timeout_sec,
        retries=retries,
        retry_delay_sec=retry_delay_sec,
    )
    if isinstance(payload, dict):
        payload.setdefault("source", "bridge_http_status")
    return payload


def get_load_status(
    args: argparse.Namespace,
    load_device: str,
    *,
    timeout_sec: float,
    bridge_lease: dict[str, Any] | None = None,
    retries: int = DEFAULT_SAMPLE_READ_RETRIES,
    retry_delay_sec: float = DEFAULT_SAMPLE_READ_RETRY_DELAY_SECONDS,
) -> Any:
    if args.load_bridge_url:
        active_bridge_lease = bridge_lease
        if active_bridge_lease is None:
            active_bridge_lease = acquire_load_bridge_lease(
                args,
                timeout_sec=timeout_sec,
            )
        try:
            return get_load_status_via_bridge(
                args,
                load_device,
                timeout_sec=timeout_sec,
                bridge_lease=active_bridge_lease,
                retries=retries,
                retry_delay_sec=retry_delay_sec,
            )
        finally:
            if bridge_lease is None:
                release_load_bridge_lease_quietly(
                    args,
                    active_bridge_lease,
                    timeout_sec=min(timeout_sec, 5.0),
                )
    completed = run_loadlynx(
        loadlynx_cmd(args, "status", "--device", load_device, "--json"),
        timeout_sec=timeout_sec,
    )
    return json.loads(completed.stdout)


def get_load_status_best_effort(
    args: argparse.Namespace,
    load_device: str,
    *,
    timeout_sec: float,
    bridge_lease: dict[str, Any] | None = None,
    retries: int = DEFAULT_SAMPLE_READ_RETRIES,
    retry_delay_sec: float = DEFAULT_SAMPLE_READ_RETRY_DELAY_SECONDS,
) -> Any:
    try:
        return get_load_status(
            args,
            load_device,
            timeout_sec=timeout_sec,
            bridge_lease=bridge_lease,
            retries=retries,
            retry_delay_sec=retry_delay_sec,
        )
    except (subprocess.TimeoutExpired, subprocess.CalledProcessError, json.JSONDecodeError) as exc:
        return {
            "ok": False,
            "error": repr(exc),
        }
    except Exception as exc:  # noqa: BLE001
        return {
            "ok": False,
            "error": repr(exc),
        }


def get_load_control(
    args: argparse.Namespace,
    load_device: str,
    *,
    timeout_sec: float,
    bridge_lease: dict[str, Any] | None = None,
) -> Any:
    if args.load_bridge_url:
        return get_load_status(
            args,
            load_device,
            timeout_sec=timeout_sec,
            bridge_lease=bridge_lease,
        )
    completed = run_loadlynx(
        loadlynx_cmd(args, "control", "get", "--device", load_device, "--json"),
        timeout_sec=timeout_sec,
    )
    return json.loads(completed.stdout)


def get_load_control_best_effort(
    args: argparse.Namespace,
    load_device: str,
    *,
    timeout_sec: float,
    bridge_lease: dict[str, Any] | None = None,
) -> Any:
    try:
        return get_load_control(
            args,
            load_device,
            timeout_sec=timeout_sec,
            bridge_lease=bridge_lease,
        )
    except (subprocess.TimeoutExpired, subprocess.CalledProcessError, json.JSONDecodeError) as exc:
        return {
            "ok": False,
            "error": repr(exc),
        }
    except Exception as exc:  # noqa: BLE001
        return {
            "ok": False,
            "error": repr(exc),
        }


def load_output_enabled(status: Any) -> bool | None:
    if not isinstance(status, dict):
        return None
    if isinstance(status.get("output_enabled"), bool):
        return status.get("output_enabled")
    if status.get("ok") is False:
        return None
    control = status.get("control")
    if isinstance(control, dict) and isinstance(control.get("output_enabled"), bool):
        return control.get("output_enabled")
    raw_status = status.get("status")
    if isinstance(raw_status, dict) and isinstance(raw_status.get("enable"), bool):
        return raw_status.get("enable")
    return None


def load_target_i_ma(status: Any) -> int | None:
    if not isinstance(status, dict):
        return None
    if status.get("ok") is False:
        return None
    preset = status.get("preset")
    if isinstance(preset, dict) and isinstance(preset.get("target_i_ma"), int):
        return preset.get("target_i_ma")
    control = status.get("control")
    if isinstance(control, dict) and isinstance(control.get("target_i_ma"), int):
        return control.get("target_i_ma")
    return None


def normalize_load_status_payload(payload: Any) -> Any:
    if not isinstance(payload, dict):
        return payload
    nested_status = payload.get("status")
    if isinstance(nested_status, dict) and (
        "device_id" in nested_status or "analog_state" in nested_status
    ):
        return nested_status
    return payload


def extract_load_status_device_generation(payload: Any) -> int | None:
    if not isinstance(payload, dict):
        return None
    value = payload.get("status_sample_generation")
    if isinstance(value, int):
        return value
    return None


def extract_load_status_sampled_at_ms(payload: Any) -> int | None:
    if not isinstance(payload, dict):
        return None
    value = payload.get("status_sampled_at_ms")
    if isinstance(value, int):
        return value
    return None


def effective_load_generation(current_generation: int, device_generation: int | None) -> int:
    if isinstance(device_generation, int):
        return device_generation
    return current_generation + 1


def is_valid_load_status_payload(payload: Any) -> bool:
    if not isinstance(payload, dict):
        return False
    return isinstance(payload.get("status"), dict) and (
        isinstance(payload.get("control"), dict)
        or isinstance(payload.get("analog_state"), str)
        or isinstance(payload.get("device_id"), str)
    )


def normalize_verified_load_payload(payload: Any) -> Any | None:
    if not isinstance(payload, dict):
        return None
    if payload.get("ok") is False:
        return None
    return payload


def select_effective_load_state(control_payload: Any, status_payload: Any) -> tuple[bool | None, int | None]:
    normalized_status = normalize_verified_load_payload(status_payload)
    normalized_control = normalize_verified_load_payload(control_payload)
    effective_enabled = load_output_enabled(normalized_status)
    if effective_enabled is None:
        effective_enabled = load_output_enabled(normalized_control)
    effective_target_i_ma = load_target_i_ma(normalized_status)
    if effective_target_i_ma is None:
        effective_target_i_ma = load_target_i_ma(normalized_control)
    return effective_enabled, effective_target_i_ma


def wait_for_load_state(
    args: argparse.Namespace,
    load_device: str,
    *,
    expected_enabled: bool,
    expected_target_i_ma: int | None,
    status_timeout_sec: float,
    verify_timeout_sec: float,
    bridge_lease: dict[str, Any] | None = None,
) -> Any:
    active_bridge_lease = bridge_lease
    if args.load_bridge_url and active_bridge_lease is None:
        bridge_lease = acquire_load_bridge_lease(
            args,
            timeout_sec=status_timeout_sec,
            bridge_device=resolve_load_bridge_device_id(args, timeout_sec=status_timeout_sec),
        )
        active_bridge_lease = bridge_lease
    deadline = time.monotonic() + verify_timeout_sec
    last_control: Any = None
    last_status: Any = None
    while time.monotonic() < deadline:
        if args.load_bridge_url and active_bridge_lease is not None:
            lease_id = dict_or_empty(active_bridge_lease).get("lease_id")
            if isinstance(lease_id, str) and lease_id:
                try:
                    heartbeat_load_bridge_lease(
                        (args.load_bridge_url or "").rstrip("/"),
                        lease_id,
                        timeout_sec=min(status_timeout_sec, 5.0),
                    )
                except Exception:  # noqa: BLE001
                    active_bridge_lease = acquire_load_bridge_lease(
                        args,
                        timeout_sec=status_timeout_sec,
                    )
        last_status = get_load_status_best_effort(
            args,
            load_device,
            timeout_sec=status_timeout_sec,
            bridge_lease=active_bridge_lease,
        )
        last_control = None
        enabled, target_i_ma = select_effective_load_state(last_control, last_status)
        if enabled is None or (expected_target_i_ma is not None and target_i_ma is None):
            last_control = get_load_control_best_effort(
                args,
                load_device,
                timeout_sec=status_timeout_sec,
                bridge_lease=active_bridge_lease,
            )
            enabled, target_i_ma = select_effective_load_state(last_control, last_status)
        target_ok = expected_target_i_ma is None or target_i_ma == expected_target_i_ma
        if enabled is expected_enabled and target_ok:
            return {
                "control": last_control,
                "status": last_status,
                "effective_enabled": enabled,
                "effective_target_i_ma": target_i_ma,
            }
        time.sleep(1.0)
    raise RuntimeError(
        "LoadLynx status did not reach expected state: "
        f"enabled={expected_enabled} target_i_ma={expected_target_i_ma} "
        f"last_control={last_control} last_status={last_status}"
    )


def wait_for_poller_state(
    load_status_poller: LoadStatusPoller,
    *,
    min_generation: int,
    expected_enabled: bool,
    expected_target_i_ma: int | None,
    timeout_sec: float,
) -> dict[str, Any]:
    deadline = time.monotonic() + max(0.1, timeout_sec)
    last_snapshot: dict[str, Any] | None = None
    while time.monotonic() < deadline:
        now = time.monotonic()
        snapshot = load_status_poller.snapshot(now)
        last_snapshot = snapshot
        generation = snapshot.get("generation")
        status_payload = normalize_load_status_payload(snapshot.get("status"))
        enabled = load_output_enabled(status_payload)
        target_i_ma = load_target_i_ma(status_payload)
        target_ok = expected_target_i_ma is None or target_i_ma == expected_target_i_ma
        generation_ok = isinstance(generation, int) and generation > min_generation
        if generation_ok and enabled is expected_enabled and target_ok:
            return snapshot
        time.sleep(0.1)
    raise RuntimeError(
        "LoadLynx poller did not observe a fresh expected state: "
        f"min_generation={min_generation} enabled={expected_enabled} "
        f"target_i_ma={expected_target_i_ma} last_snapshot={last_snapshot}"
    )


def load_cc(
    args: argparse.Namespace,
    load_device: str,
    current_ma: int,
    *,
    max_i_ma_total: int,
    max_p_mw: int,
    timeout_sec: float,
    status_timeout_sec: float,
    verify_timeout_sec: float,
    bridge_lease: dict[str, Any] | None = None,
) -> dict[str, Any]:
    owned_bridge_lease = False
    completed: subprocess.CompletedProcess[str] | None = None
    timeout_error: subprocess.TimeoutExpired | None = None
    process_error: subprocess.CalledProcessError | None = None
    try:
        if args.load_bridge_url:
            if bridge_lease is None:
                bridge_lease = acquire_load_bridge_lease(
                    args,
                    timeout_sec=status_timeout_sec,
                    bridge_device=resolve_load_bridge_device_id(
                        args,
                        timeout_sec=min(status_timeout_sec, 5.0),
                    ),
                )
                owned_bridge_lease = True
            cmd = loadlynx_cmd(
                args,
                "cc",
                str(current_ma),
                "--max-i-ma-total",
                str(max_i_ma_total),
                "--max-p-mw",
                str(max_p_mw),
                "--url",
                build_load_bridge_cli_url(
                    args,
                    timeout_sec=min(status_timeout_sec, 5.0),
                    bridge_lease=bridge_lease,
                ),
            )
        else:
            cmd = loadlynx_cmd(
                args,
                "cc",
                str(current_ma),
                "--device",
                load_device,
                "--max-i-ma-total",
                str(max_i_ma_total),
                "--max-p-mw",
                str(max_p_mw),
            )

        try:
            if bridge_lease is not None:
                completed = run_loadlynx_bridge_command(
                    args,
                    bridge_lease,
                    "cc",
                    str(current_ma),
                    "--max-i-ma-total",
                    str(max_i_ma_total),
                    "--max-p-mw",
                    str(max_p_mw),
                    timeout_sec=timeout_sec,
                )
            else:
                completed = run_loadlynx(cmd, timeout_sec=timeout_sec)
        except subprocess.TimeoutExpired as exc:
            timeout_error = exc
        except subprocess.CalledProcessError as exc:
            process_error = exc

        verified_status = wait_for_load_state(
            args,
            load_device,
            expected_enabled=True,
            expected_target_i_ma=current_ma,
            status_timeout_sec=status_timeout_sec,
            verify_timeout_sec=verify_timeout_sec,
            bridge_lease=bridge_lease,
        )
        result = {
            "cmd": cmd,
            "verified_status": verified_status,
        }
        if completed is not None:
            result["stdout"] = completed.stdout
            result["stderr"] = completed.stderr
        elif process_error is not None:
            result["stdout"] = process_error.stdout
            result["stderr"] = process_error.stderr
            result["nonzero_but_verified"] = True
            result["process_error"] = repr(process_error)
        else:
            result["stdout"] = timeout_error.stdout if timeout_error else None
            result["stderr"] = timeout_error.stderr if timeout_error else None
            result["timed_out_but_verified"] = True
            result["timeout_error"] = repr(timeout_error)
        return result
    finally:
        if owned_bridge_lease:
            release_load_bridge_lease_quietly(
                args,
                bridge_lease,
                timeout_sec=min(status_timeout_sec, 5.0),
            )


def disable_load(
    args: argparse.Namespace,
    load_device: str,
    *,
    timeout_sec: float,
    status_timeout_sec: float,
    verify_timeout_sec: float,
    bridge_lease: dict[str, Any] | None = None,
) -> dict[str, Any]:
    owned_bridge_lease = False
    completed: subprocess.CompletedProcess[str] | None = None
    timeout_error: subprocess.TimeoutExpired | None = None
    process_error: subprocess.CalledProcessError | None = None
    try:
        if args.load_bridge_url:
            if bridge_lease is None:
                bridge_lease = acquire_load_bridge_lease(
                    args,
                    timeout_sec=status_timeout_sec,
                    bridge_device=resolve_load_bridge_device_id(
                        args,
                        timeout_sec=min(status_timeout_sec, 5.0),
                    ),
                )
                owned_bridge_lease = True
        status = get_load_status_best_effort(
            args,
            load_device,
            timeout_sec=status_timeout_sec,
            bridge_lease=bridge_lease,
        )
        enabled = load_output_enabled(normalize_verified_load_payload(status))
        status_enabled = load_output_enabled(normalize_verified_load_payload(status))
        if enabled is False or status_enabled is False:
            return {
                "cmd": None,
                "skipped": True,
                "reason": "already_disabled",
                "control": None,
                "status": status,
                "effective_enabled": enabled,
            }

        if bridge_lease is not None:
            cmd = loadlynx_cmd(
                args,
                "control",
                "set",
                "--disable",
                "--url",
                build_load_bridge_cli_url(
                    args,
                    timeout_sec=min(status_timeout_sec, 5.0),
                    bridge_lease=bridge_lease,
                ),
            )
        else:
            cmd = loadlynx_cmd(args, "control", "set", "--device", load_device, "--disable")

        command_ack_disabled = False
        try:
            if bridge_lease is not None:
                completed = run_loadlynx_bridge_command(
                    args,
                    bridge_lease,
                    "control",
                    "set",
                    "--disable",
                    timeout_sec=timeout_sec,
                )
            else:
                completed = run_loadlynx(cmd, timeout_sec=timeout_sec)
            command_ack_disabled = "output=false" in (completed.stdout or "")
        except subprocess.TimeoutExpired as exc:
            timeout_error = exc
            command_ack_disabled = "output=false" in (
                (exc.stdout or "") if isinstance(exc.stdout, str) else ""
            )
        except subprocess.CalledProcessError as exc:
            process_error = exc
            command_ack_disabled = "output=false" in (
                (exc.stdout or "") if isinstance(exc.stdout, str) else ""
            )

        if command_ack_disabled:
            actual_status = get_load_status_best_effort(
                args,
                load_device,
                timeout_sec=status_timeout_sec,
                bridge_lease=bridge_lease,
            )
            actual_control = get_load_control_best_effort(
                args,
                load_device,
                timeout_sec=status_timeout_sec,
                bridge_lease=bridge_lease,
            )
            actual_enabled, actual_target_i_ma = select_effective_load_state(
                actual_control,
                actual_status,
            )
            verified_status = {
                "control": actual_control if normalize_verified_load_payload(actual_control) else None,
                "status": actual_status if normalize_verified_load_payload(actual_status) else None,
                "effective_enabled": False if actual_enabled is None else actual_enabled,
                "effective_target_i_ma": 0 if actual_target_i_ma is None else actual_target_i_ma,
                "degraded_verification": True,
                "degraded_from_command_ack": True,
            }
            result = {
                "cmd": cmd,
                "verified_status": verified_status,
            }
            if completed is not None:
                result["stdout"] = completed.stdout
                result["stderr"] = completed.stderr
            elif process_error is not None:
                result["stdout"] = process_error.stdout
                result["stderr"] = process_error.stderr
                result["nonzero_but_verified"] = True
                result["process_error"] = repr(process_error)
            else:
                result["stdout"] = timeout_error.stdout if timeout_error else None
                result["stderr"] = timeout_error.stderr if timeout_error else None
                result["timed_out_but_verified"] = True
                result["timeout_error"] = repr(timeout_error)
            return result

        try:
            verified_status = wait_for_load_state(
                args,
                load_device,
                expected_enabled=False,
                expected_target_i_ma=None,
                status_timeout_sec=status_timeout_sec,
                verify_timeout_sec=verify_timeout_sec,
                bridge_lease=bridge_lease,
            )
        except RuntimeError as verify_exc:
            final_status = get_load_status_best_effort(
                args,
                load_device,
                timeout_sec=status_timeout_sec,
                bridge_lease=bridge_lease,
            )
            final_control = get_load_control_best_effort(
                args,
                load_device,
                timeout_sec=status_timeout_sec,
                bridge_lease=bridge_lease,
            )
            final_enabled, final_target_i_ma = select_effective_load_state(
                final_control,
                final_status,
            )
            command_text = "\n".join(
                part
                for part in (
                    completed.stdout if completed is not None else None,
                    process_error.stdout if process_error is not None else None,
                    timeout_error.stdout if timeout_error is not None else None,
                )
                if isinstance(part, str)
            )
            command_proved_disabled = "output=false" in command_text
            if final_enabled is not False and not command_proved_disabled:
                raise
            verified_status = {
                "control": final_control,
                "status": final_status,
                "effective_enabled": False if command_proved_disabled else final_enabled,
                "effective_target_i_ma": final_target_i_ma,
                "degraded_verification": True,
                "degraded_from_command_ack": command_proved_disabled,
                "verify_error": repr(verify_exc),
            }

        result = {
            "cmd": cmd,
            "verified_status": verified_status,
        }
        if completed is not None:
            result["stdout"] = completed.stdout
            result["stderr"] = completed.stderr
        elif process_error is not None:
            result["stdout"] = process_error.stdout
            result["stderr"] = process_error.stderr
            result["nonzero_but_verified"] = True
            result["process_error"] = repr(process_error)
        else:
            result["stdout"] = timeout_error.stdout if timeout_error else None
            result["stderr"] = timeout_error.stderr if timeout_error else None
            result["timed_out_but_verified"] = True
            result["timeout_error"] = repr(timeout_error)
        return result
    finally:
        if owned_bridge_lease:
            release_load_bridge_lease_quietly(
                args,
                bridge_lease,
                timeout_sec=min(status_timeout_sec, 5.0),
            )


def load_status_i_total_ma(status: Any) -> int | None:
    if not isinstance(status, dict):
        return None
    raw_status = status.get("status")
    if not isinstance(raw_status, dict):
        return None
    local_ma = raw_status.get("i_local_ma")
    remote_ma = raw_status.get("i_remote_ma")
    parts: list[int] = []
    if isinstance(local_ma, int) and local_ma >= 0:
        parts.append(local_ma)
    if isinstance(remote_ma, int) and remote_ma >= 0:
        parts.append(remote_ma)
    if not parts:
        return None
    return sum(parts)


def preferred_ups_vout_mv(
    out_a_vbus_mv: Any,
    out_b_vbus_mv: Any,
) -> int | float | None:
    if isinstance(out_a_vbus_mv, (int, float)):
        return out_a_vbus_mv
    if isinstance(out_b_vbus_mv, (int, float)):
        return out_b_vbus_mv
    return None


def set_port_c_power(isolapurr_url: str, enabled: bool) -> dict[str, Any]:
    query = urllib.parse.urlencode({"enabled": "1" if enabled else "0"})
    url = f"{isolapurr_url.rstrip('/')}{PORT_C_POWER_PATH}?{query}"
    return {
        "url": url,
        "enabled": enabled,
        "response": http_post_json(url),
    }


def set_isolapurr_manual_output(
    isolapurr_url: str,
    *,
    voltage_mv: int,
    current_limit_ma: int,
) -> dict[str, Any]:
    payload = run_json_command_with_retries(
        [
            "isolapurr",
            "power",
            "output",
            "manual",
            "--url",
            isolapurr_url,
            "--voltage-mv",
            str(voltage_mv),
            "--current-limit-ma",
            str(current_limit_ma),
            "--usb-c-path",
            "disconnected",
            "--json",
        ],
        timeout_sec=20.0,
    )
    return dict_or_empty(payload)


def fetch_isolapurr_ports(isolapurr_url: str, *, timeout_sec: float) -> dict[str, Any]:
    url = f"{isolapurr_url.rstrip('/')}{PORTS_PATH}"
    ports_payload = http_json_with_retries(url, timeout_sec=timeout_sec)
    return {
        "source": "http_ports",
        "ports": ports_payload,
    }


def port_state(
    ports_payload: dict[str, Any],
    *,
    port_id: str,
) -> dict[str, Any]:
    ports = (ports_payload.get("ports") or {}).get("ports", [])
    for port in ports:
        if port.get("portId") == port_id:
            return port if isinstance(port, dict) else {}
    return {}


def sample_point(
    *,
    t_s: float,
    tag: str,
    captured_at_utc: str,
    ups_status_url: str,
    devd_diag_snapshot_url: str,
    isolapurr_url: str,
    status_timeout_sec: float,
    load_status_snapshot: dict[str, Any],
) -> dict[str, Any]:
    ups_status: dict[str, Any] = {}
    diag_snapshot: dict[str, Any] = {}
    ups_input: dict[str, Any] = {}
    diag_input: dict[str, Any] = {}
    for attempt in range(3):
        ups_status = dict_or_empty(http_json_with_retries(
            ups_status_url,
            timeout_sec=min(status_timeout_sec, 5.0),
        ))
        diag_snapshot = unwrap_diag_snapshot_payload(
            fetch_diag_snapshot_with_fallback(
                devd_diag_snapshot_url=devd_diag_snapshot_url,
                ups_status=ups_status,
                timeout_sec=min(status_timeout_sec, 5.0),
            )
        )
        ups_input = dict_or_empty(ups_status.get("input"))
        diag_input = dict_or_empty(diag_snapshot.get("input"))
        if (
            isinstance(
                first_numeric(ups_input.get("vin_vbus_mv"), diag_input.get("vin_vbus_mv")),
                (int, float),
            )
            and isinstance(
                first_numeric(ups_input.get("vin_iin_ma"), diag_input.get("vin_iin_ma")),
                (int, float),
            )
        ):
            break
        if attempt < 2:
            time.sleep(0.1)
    isolapurr_ports = dict_or_empty(fetch_isolapurr_ports(
        isolapurr_url,
        timeout_sec=min(status_timeout_sec, 5.0),
    ))
    isolapurr_power_show: dict[str, Any] = {
        "source": "ports_primary",
    }
    load_status = normalize_load_status_payload(load_status_snapshot.get("status"))
    load_status = dict_or_empty(load_status)
    port_c_ports = next(
        (
            (dict_or_empty(port).get("telemetry") or {})
            for port in dict_or_empty(isolapurr_ports.get("ports")).get("ports", [])
            if port.get("portId") == "port_c"
        ),
        {},
    )
    port_c_ports = dict_or_empty(port_c_ports)
    port_c_power_show = next(
        (
            (dict_or_empty(port).get("telemetry") or {})
            for port in dict_or_empty(
                dict_or_empty(isolapurr_power_show.get("ports")).get("ports")
            )
            if port.get("portId") == "port_c"
        ),
        {},
    )
    port_c_power_show = dict_or_empty(port_c_power_show)
    diagnostics = dict_or_empty(isolapurr_power_show.get("diagnostics"))
    usb_c_actual = dict_or_empty(diagnostics.get("usb_c_actual"))
    port_c_state = dict_or_empty(port_state(isolapurr_ports, port_id="port_c").get("state"))
    source_voltage_mv = first_numeric(
        port_c_ports.get("voltage_mv"),
        port_c_power_show.get("voltage_mv"),
        usb_c_actual.get("voltage_mv"),
    )
    source_current_ma = first_numeric(
        port_c_ports.get("current_ma"),
        port_c_power_show.get("current_ma"),
        usb_c_actual.get("current_ma"),
    )
    source_status = first_non_empty_string(
        port_c_ports.get("status"),
        port_c_power_show.get("status"),
        usb_c_actual.get("status"),
    )
    source_enabled = first_bool(
        port_c_state.get("power_enabled"),
        diagnostics.get("usb_c_power_enabled"),
    )
    if (
        source_voltage_mv is None
        or source_current_ma is None
        or source_status is None
        or source_enabled is None
    ):
        isolapurr_power_show = dict_or_empty(fetch_isolapurr_power_show_best_effort(
            isolapurr_url,
            timeout_sec=min(status_timeout_sec, 5.0),
        ))
        port_c_power_show = next(
            (
                (dict_or_empty(port).get("telemetry") or {})
                for port in dict_or_empty(
                    dict_or_empty(isolapurr_power_show.get("ports")).get("ports")
                )
                if port.get("portId") == "port_c"
            ),
            {},
        )
        port_c_power_show = dict_or_empty(port_c_power_show)
        diagnostics = dict_or_empty(isolapurr_power_show.get("diagnostics"))
        usb_c_actual = dict_or_empty(diagnostics.get("usb_c_actual"))
        source_voltage_mv = first_numeric(
            source_voltage_mv,
            port_c_power_show.get("voltage_mv"),
            usb_c_actual.get("voltage_mv"),
        )
        source_current_ma = first_numeric(
            source_current_ma,
            port_c_power_show.get("current_ma"),
            usb_c_actual.get("current_ma"),
        )
        source_status = first_non_empty_string(
            source_status,
            port_c_power_show.get("status"),
            usb_c_actual.get("status"),
        )
        source_enabled = first_bool(
            source_enabled,
            diagnostics.get("usb_c_power_enabled"),
        )
    raw_status = dict_or_empty(load_status.get("status"))
    ups_output = dict_or_empty(ups_status.get("output"))
    ups_battery = dict_or_empty(ups_status.get("battery"))
    ups_charger = dict_or_empty(ups_status.get("charger"))
    out_a = dict_or_empty(ups_output.get("out_a"))
    out_b = dict_or_empty(ups_output.get("out_b"))
    return {
        "captured_at_utc": captured_at_utc,
        "t_s": round(t_s, 3),
        "tag": tag,
        "mode": ups_status.get("mode"),
        "mains_present": ups_input.get("mains_present"),
        "stage": ups_input.get("assist_power_stage"),
        "assist_target_vout_mv": ups_input.get("assist_target_vout_mv"),
        "vin_vbus_mv": first_numeric(ups_input.get("vin_vbus_mv"), diag_input.get("vin_vbus_mv")),
        "vin_iin_ma": first_numeric(ups_input.get("vin_iin_ma"), diag_input.get("vin_iin_ma")),
        "input_vbus_mv": first_numeric(
            ups_input.get("input_vbus_mv"),
            diag_input.get("input_vbus_mv"),
        ),
        "input_ibus_ma": first_numeric(
            ups_input.get("input_ibus_ma"),
            diag_input.get("input_ibus_ma"),
        ),
        "tps_total_iout_ma": first_numeric(
            ups_input.get("tps_total_iout_ma"),
            diag_input.get("tps_total_iout_ma"),
        ),
        "battery_current_ma": ups_battery.get("current_ma"),
        "charger_allow_charge": ups_charger.get("allow_charge"),
        "charger_detail_status": ups_charger.get("detail_status"),
        "diag_stage": diag_input.get("assist_power_stage"),
        "diag_assist_target_vout_mv": diag_input.get("assist_target_vout_mv"),
        "diag_vin_vbus_mv": diag_input.get("vin_vbus_mv"),
        "diag_vin_iin_ma": diag_input.get("vin_iin_ma"),
        "diag_vin_baseline_mv": diag_input.get("vin_baseline_mv"),
        "diag_vin_drop_mv": diag_input.get("vin_drop_mv"),
        "diag_tps_total_iout_ma": diag_input.get("tps_total_iout_ma"),
        "diag_source": diag_snapshot.get("source"),
        "out_a_vbus_mv": out_a.get("vbus_mv"),
        "out_a_iout_ma": out_a.get("iout_ma"),
        "out_b_vbus_mv": out_b.get("vbus_mv"),
        "out_b_iout_ma": out_b.get("iout_ma"),
        "ups_vout_mv": preferred_ups_vout_mv(out_a.get("vbus_mv"), out_b.get("vbus_mv")),
        "port_c_enabled": source_enabled,
        "isolapurr_port_c_mv": source_voltage_mv,
        "isolapurr_port_c_ma": source_current_ma,
        "isolapurr_port_c_status": source_status,
        "load_output_enabled": load_output_enabled(load_status),
        "load_target_i_ma": load_target_i_ma(load_status),
        "load_v_local_mv": raw_status.get("v_local_mv"),
        "load_v_remote_mv": raw_status.get("v_remote_mv"),
        "load_i_local_ma": raw_status.get("i_local_ma"),
        "load_i_remote_ma": raw_status.get("i_remote_ma"),
        "load_i_total_ma": load_status_i_total_ma(load_status),
        "load_calc_p_mw": raw_status.get("calc_p_mw"),
        "load_status_generation": load_status_snapshot.get("generation"),
        "load_status_age_s": load_status_snapshot.get("age_s"),
        "load_status_sample_age_s": load_status_snapshot.get("sample_age_s"),
        "load_status_device_generation": load_status_snapshot.get("device_generation"),
        "load_status_sampled_at_ms": load_status_snapshot.get("device_sampled_at_ms"),
        "load_status_error": load_status_snapshot.get("error"),
        "load_status_source": first_non_empty_string(load_status.get("source")),
        "load_poll_paused": load_status_snapshot.get("poller_paused"),
        "load_poll_idle": load_status_snapshot.get("poller_idle"),
        "raw": {
            "ups_status": ups_status,
            "diag_snapshot": diag_snapshot,
            "isolapurr_ports": isolapurr_ports,
            "isolapurr_power_show": isolapurr_power_show,
            "load_status": load_status,
        },
    }


def capture_window(
    jsonl_path: Path,
    *,
    tag: str,
    seconds: float,
    interval_seconds: float,
    started_at: float,
    ups_status_url: str,
    devd_diag_snapshot_url: str,
    isolapurr_url: str,
    status_timeout_sec: float,
    load_status_poller: LoadStatusPoller,
) -> list[dict[str, Any]]:
    samples: list[dict[str, Any]] = []
    deadline = time.monotonic() + max(0.0, seconds)
    freshness_grace_deadline = deadline + max(status_timeout_sec + 5.0, 15.0)
    phase_initial_generation: int | None = None
    while True:
        now = time.monotonic()
        if now > deadline and samples:
            latest_generation = current_snapshot.get("generation") if "current_snapshot" in locals() else None
            if (
                isinstance(phase_initial_generation, int)
                and isinstance(latest_generation, int)
                and latest_generation > phase_initial_generation
            ):
                break
            if now >= freshness_grace_deadline:
                break
        current_snapshot = load_status_poller.snapshot(now)
        if phase_initial_generation is None and isinstance(current_snapshot.get("generation"), int):
            phase_initial_generation = current_snapshot.get("generation")
        sample = sample_point(
            t_s=now - started_at,
            tag=tag,
            captured_at_utc=datetime.now(timezone.utc).isoformat(),
            ups_status_url=ups_status_url,
            devd_diag_snapshot_url=devd_diag_snapshot_url,
            isolapurr_url=isolapurr_url,
            status_timeout_sec=status_timeout_sec,
            load_status_snapshot=current_snapshot,
        )
        samples.append(sample)
        append_jsonl(jsonl_path, sample)
        now_after_sample = time.monotonic()
        latest_generation = current_snapshot.get("generation")
        if now_after_sample >= deadline:
            if (
                isinstance(phase_initial_generation, int)
                and isinstance(latest_generation, int)
                and latest_generation > phase_initial_generation
            ):
                break
            if now_after_sample >= freshness_grace_deadline:
                break
        time.sleep(interval_seconds)
    return samples


def sleep_with_sampler(
    sampler: ContinuousSampler | None,
    *,
    seconds: float,
    collect_into: list[dict[str, Any]] | None = None,
) -> None:
    deadline = time.monotonic() + max(0.0, seconds)
    while time.monotonic() < deadline:
        time.sleep(min(0.1, max(0.01, deadline - time.monotonic())))
    if sampler is not None and collect_into is not None:
        collect_into[:] = sampler.snapshot_samples()


def pause_load_status_poller_for_control(
    load_status_poller: LoadStatusPoller,
    *,
    idle_timeout_sec: float,
    release_bridge_lease: bool = True,
) -> bool:
    load_status_poller.pause()
    idle = load_status_poller.wait_until_idle(timeout_sec=idle_timeout_sec)
    if idle and release_bridge_lease:
        load_status_poller.release_bridge_lease(timeout_sec=min(idle_timeout_sec, 5.0))
    return idle


def summarize_numeric(samples: list[dict[str, Any]], key: str) -> dict[str, Any] | None:
    keyed_samples = [
        sample for sample in samples if isinstance(sample.get(key), (int, float))
    ]
    if not keyed_samples:
        return None
    values = [sample[key] for sample in keyed_samples]
    minimum = min(values)
    maximum = max(values)
    min_sample = next(sample for sample in keyed_samples if sample[key] == minimum)
    max_sample = next(sample for sample in keyed_samples if sample[key] == maximum)
    return {
        "min": minimum,
        "max": maximum,
        "span": maximum - minimum,
        "count": len(values),
        "min_sample": {
            "t_s": min_sample.get("t_s"),
            "captured_at_utc": min_sample.get("captured_at_utc"),
            "tag": min_sample.get("tag"),
            "mode": min_sample.get("mode"),
            "stage": min_sample.get("stage"),
        },
        "max_sample": {
            "t_s": max_sample.get("t_s"),
            "captured_at_utc": max_sample.get("captured_at_utc"),
            "tag": max_sample.get("tag"),
            "mode": max_sample.get("mode"),
            "stage": max_sample.get("stage"),
        },
    }


def summarize_sample_spacing(samples: list[dict[str, Any]]) -> dict[str, Any] | None:
    sample_times = [
        float(sample["t_s"])
        for sample in samples
        if isinstance(sample.get("t_s"), (int, float))
    ]
    if len(sample_times) < 2:
        return None
    gaps = [
        round(sample_times[index] - sample_times[index - 1], 3)
        for index in range(1, len(sample_times))
    ]
    return {
        "min_gap_s": min(gaps),
        "max_gap_s": max(gaps),
        "avg_gap_s": round(sum(gaps) / len(gaps), 3),
        "gap_count": len(gaps),
    }


def summarize_gap(samples: list[dict[str, Any]], lhs_key: str, rhs_key: str) -> dict[str, Any] | None:
    gap_samples: list[dict[str, Any]] = []
    for sample in samples:
        lhs = sample.get(lhs_key)
        rhs = sample.get(rhs_key)
        if isinstance(lhs, (int, float)) and isinstance(rhs, (int, float)):
            gap_samples.append(
                {
                    "gap_mv": abs(lhs - rhs),
                    "sample": sample,
                }
            )
    if not gap_samples:
        return None
    values = [entry["gap_mv"] for entry in gap_samples]
    maximum = max(values)
    max_entry = next(entry for entry in gap_samples if entry["gap_mv"] == maximum)
    non_zero_count = sum(1 for value in values if value > 0)
    return {
        "min": min(values),
        "max": maximum,
        "span": maximum - min(values),
        "count": len(values),
        "non_zero_count": non_zero_count,
        "max_sample": {
            "t_s": max_entry["sample"].get("t_s"),
            "captured_at_utc": max_entry["sample"].get("captured_at_utc"),
            "tag": max_entry["sample"].get("tag"),
            "mode": max_entry["sample"].get("mode"),
            "stage": max_entry["sample"].get("stage"),
        },
    }


def summarize_minimum_output_sample(samples: list[dict[str, Any]]) -> dict[str, Any] | None:
    candidates: list[tuple[str, int | float, dict[str, Any]]] = []
    for key in ("ups_vout_mv", "out_a_vbus_mv", "out_b_vbus_mv", "load_v_local_mv"):
        for sample in samples:
            value = sample.get(key)
            if isinstance(value, (int, float)):
                candidates.append((key, value, sample))
    if not candidates:
        return None
    channel, value, sample = min(candidates, key=lambda item: item[1])
    return {
        "channel": channel,
        "value": value,
        "t_s": sample.get("t_s"),
        "captured_at_utc": sample.get("captured_at_utc"),
        "tag": sample.get("tag"),
        "mode": sample.get("mode"),
        "stage": sample.get("stage"),
    }


def all_samples_match(
    samples: list[dict[str, Any]],
    key: str,
    predicate,
) -> bool:
    return all(predicate(sample.get(key)) for sample in samples)


def group_tag_name(group_samples: list[dict[str, Any]]) -> str:
    if not group_samples:
        return "unknown"
    return str(group_samples[0].get("tag") or "unknown")


def evaluate_group_completeness(
    group_samples: list[dict[str, Any]],
    *,
    load_status_max_age_seconds: float,
    expected_duration_seconds: float | None = None,
    nominal_interval_seconds: float | None = None,
) -> dict[str, Any]:
    failures: list[str] = []
    tag = group_tag_name(group_samples)
    ups_status_present = all(
        isinstance((sample.get("raw") or {}).get("ups_status"), dict)
        for sample in group_samples
    )
    diag_snapshot_present = all(
        isinstance((sample.get("raw") or {}).get("diag_snapshot"), dict)
        for sample in group_samples
    )
    isolapurr_present = all(
        isinstance((sample.get("raw") or {}).get("isolapurr_ports"), dict)
        and isinstance((sample.get("raw") or {}).get("isolapurr_power_show"), dict)
        for sample in group_samples
    )
    load_status_present = all(
        isinstance((sample.get("raw") or {}).get("load_status"), dict)
        for sample in group_samples
    )
    port_c_state_present = all(
        isinstance(sample.get("port_c_enabled"), bool)
        for sample in group_samples
    )
    output_voltage_present = all(
        summarize_numeric(group_samples, key) is not None
        for key in ("ups_vout_mv", "load_v_local_mv")
    )
    generations = sorted(
        {
            int(sample.get("load_status_generation"))
            for sample in group_samples
            if isinstance(sample.get("load_status_generation"), int)
        }
    )
    load_age_samples = [
        sample.get("load_status_sample_age_s")
        if isinstance(sample.get("load_status_sample_age_s"), (int, float))
        else sample.get("load_status_age_s")
        for sample in group_samples
        if isinstance(sample.get("load_status_sample_age_s"), (int, float))
        or isinstance(sample.get("load_status_age_s"), (int, float))
    ]
    max_load_status_age_s = max(load_age_samples, default=None)
    spacing = summarize_sample_spacing(group_samples)
    fresh_capture_max_age_s = min(load_status_max_age_seconds, 2.0)
    fresh_sample_count = sum(
        1 for age_s in load_age_samples if age_s <= fresh_capture_max_age_s
    )
    freshness_visible = fresh_sample_count >= 1
    steady_state_freshness_visible = fresh_sample_count >= 2
    freshness_ok = freshness_visible
    if not ups_status_present:
        failures.append("missing_ups_status")
    if not diag_snapshot_present:
        failures.append("missing_diag_snapshot")
    if not isolapurr_present:
        failures.append("missing_isolapurr")
    if not load_status_present:
        failures.append("missing_load_status")
    def series_complete(key: str, predicate) -> bool:
        for sample in group_samples:
            value = sample.get(key)
            if predicate(value):
                continue
            if (
                key == "diag_vin_baseline_mv"
                and sample.get("port_c_enabled") is False
                and sample.get("mains_present") is False
            ):
                continue
            return False
        return True

    field_checks = (
        ("port_c_enabled", lambda value: isinstance(value, bool), "missing_port_c_enabled"),
        (
            "isolapurr_port_c_status",
            lambda value: isinstance(value, str) and value != "",
            "missing_isolapurr_status_series",
        ),
        ("isolapurr_port_c_mv", lambda value: isinstance(value, (int, float)), "missing_isolapurr_voltage_series"),
        ("isolapurr_port_c_ma", lambda value: isinstance(value, (int, float)), "missing_isolapurr_current_series"),
        ("mode", lambda value: isinstance(value, str) and value != "", "missing_mode_series"),
        ("mains_present", lambda value: isinstance(value, bool), "missing_mains_present_series"),
        ("stage", lambda value: isinstance(value, str) and value != "", "missing_assist_stage_series"),
        (
            "assist_target_vout_mv",
            lambda value: isinstance(value, (int, float)),
            "missing_assist_target_vout_series",
        ),
        ("vin_vbus_mv", lambda value: isinstance(value, (int, float)), "missing_vin_voltage_series"),
        ("vin_iin_ma", lambda value: isinstance(value, (int, float)), "missing_vin_current_series"),
        (
            "tps_total_iout_ma",
            lambda value: isinstance(value, (int, float)),
            "missing_tps_total_iout_series",
        ),
        (
            "battery_current_ma",
            lambda value: isinstance(value, (int, float)),
            "missing_battery_current_series",
        ),
        (
            "charger_allow_charge",
            lambda value: isinstance(value, bool),
            "missing_charger_allow_charge_series",
        ),
        (
            "charger_detail_status",
            lambda value: isinstance(value, str) and value != "",
            "missing_charger_detail_status_series",
        ),
        ("diag_stage", lambda value: isinstance(value, str) and value != "", "missing_diag_stage_series"),
        (
            "diag_assist_target_vout_mv",
            lambda value: isinstance(value, (int, float)),
            "missing_diag_assist_target_vout_series",
        ),
        (
            "diag_vin_baseline_mv",
            lambda value: isinstance(value, (int, float)),
            "missing_diag_vin_baseline_series",
        ),
        (
            "diag_vin_drop_mv",
            lambda value: value is None or isinstance(value, (int, float)),
            "missing_diag_vin_drop_series",
        ),
        (
            "diag_tps_total_iout_ma",
            lambda value: isinstance(value, (int, float)),
            "missing_diag_tps_total_iout_series",
        ),
        (
            "ups_vout_mv",
            lambda value: isinstance(value, (int, float)),
            "missing_ups_output_voltage_series",
        ),
        (
            "load_output_enabled",
            lambda value: isinstance(value, bool),
            "missing_load_output_enabled_series",
        ),
        (
            "load_v_local_mv",
            lambda value: isinstance(value, (int, float)),
            "missing_load_v_local_series",
        ),
        (
            "load_v_remote_mv",
            lambda value: isinstance(value, (int, float)),
            "missing_load_v_remote_series",
        ),
        (
            "load_i_local_ma",
            lambda value: isinstance(value, (int, float)),
            "missing_load_i_local_series",
        ),
        (
            "load_i_remote_ma",
            lambda value: isinstance(value, (int, float)),
            "missing_load_i_remote_series",
        ),
        (
            "load_i_total_ma",
            lambda value: isinstance(value, (int, float)),
            "missing_load_i_total_series",
        ),
        (
            "load_calc_p_mw",
            lambda value: isinstance(value, (int, float)),
            "missing_load_power_series",
        ),
    )
    for key, predicate, failure in field_checks:
        if not series_complete(key, predicate):
            failures.append(failure)
    if not port_c_state_present:
        failures.append("missing_port_c_enabled")
    if not output_voltage_present:
        failures.append("missing_output_voltage_series")
    if not freshness_ok:
        failures.append("missing_scene_local_load_freshness")
    if (
        isinstance(max_load_status_age_s, (int, float))
        and max_load_status_age_s > load_status_max_age_seconds
    ):
        failures.append("stale_scene_local_load_samples")
    if isinstance(expected_duration_seconds, (int, float)) and expected_duration_seconds > 0:
        min_samples = max(2, math.ceil(expected_duration_seconds / 2.0))
        if len(group_samples) < min_samples:
            failures.append("insufficient_scene_sample_density")
        if nominal_interval_seconds and nominal_interval_seconds > 0 and spacing is not None:
            max_allowed_gap_s = max(3.0, nominal_interval_seconds * 6.0)
            if spacing.get("max_gap_s") is not None and spacing["max_gap_s"] > max_allowed_gap_s:
                failures.append("scene_sampling_gap_too_large")
    return {
        "scene_complete": not failures,
        "failures": failures,
        "ups_status_present": ups_status_present,
        "diag_snapshot_present": diag_snapshot_present,
        "isolapurr_present": isolapurr_present,
        "load_status_present": load_status_present,
        "tag": tag,
        "port_c_state_present": port_c_state_present,
        "output_voltage_present": output_voltage_present,
        "load_status_generations": generations,
        "load_status_generation_count": len(generations),
        "load_status_max_age_s": max_load_status_age_s,
        "sample_spacing": spacing,
        "load_freshness_visible": freshness_ok,
        "load_generation_change_visible": freshness_visible,
        "load_steady_state_freshness_visible": steady_state_freshness_visible,
    }


def summarize_samples(
    samples: list[dict[str, Any]],
    *,
    expected_tags: list[str] | None = None,
    load_status_max_age_seconds: float,
    expected_tag_durations: dict[str, float] | None = None,
    nominal_interval_seconds: float | None = None,
) -> dict[str, Any]:
    by_tag: dict[str, list[dict[str, Any]]] = {}
    for sample in samples:
        by_tag.setdefault(str(sample.get("tag")), []).append(sample)

    def summarize_group(group_samples: list[dict[str, Any]]) -> dict[str, Any]:
        stages = [sample.get("stage") for sample in group_samples if sample.get("stage") is not None]
        modes = [sample.get("mode") for sample in group_samples if sample.get("mode") is not None]
        generations = sorted(
            {
                int(sample.get("load_status_generation"))
                for sample in group_samples
                if isinstance(sample.get("load_status_generation"), int)
            }
        )
        out_a_summary = summarize_numeric(group_samples, "out_a_vbus_mv")
        out_b_summary = summarize_numeric(group_samples, "out_b_vbus_mv")
        load_v_summary = summarize_numeric(group_samples, "load_v_local_mv")
        return {
            "sample_count": len(group_samples),
            "t_start_s": group_samples[0].get("t_s") if group_samples else None,
            "t_end_s": group_samples[-1].get("t_s") if group_samples else None,
            "first_mode": modes[0] if modes else None,
            "last_mode": modes[-1] if modes else None,
            "first_stage": stages[0] if stages else None,
            "last_stage": stages[-1] if stages else None,
            "stage_set": sorted({stage for stage in stages if stage is not None}),
            "mode_set": sorted({mode for mode in modes if mode is not None}),
            "mains_present_set": sorted(
                {
                    bool(sample.get("mains_present"))
                    for sample in group_samples
                    if isinstance(sample.get("mains_present"), bool)
                }
            ),
            "charger_allow_charge_set": sorted(
                {
                    bool(sample.get("charger_allow_charge"))
                    for sample in group_samples
                    if isinstance(sample.get("charger_allow_charge"), bool)
                }
            ),
            "charger_detail_status_set": sorted(
                {
                    str(sample.get("charger_detail_status"))
                    for sample in group_samples
                    if isinstance(sample.get("charger_detail_status"), str)
                }
            ),
            "port_c_enabled_set": sorted(
                {
                    bool(sample.get("port_c_enabled"))
                    for sample in group_samples
                    if isinstance(sample.get("port_c_enabled"), bool)
                }
            ),
            "port_c_status_set": sorted(
                {
                    str(sample.get("isolapurr_port_c_status"))
                    for sample in group_samples
                    if isinstance(sample.get("isolapurr_port_c_status"), str)
                    and sample.get("isolapurr_port_c_status") != ""
                }
            ),
            "load_output_enabled_set": sorted(
                {
                    bool(sample.get("load_output_enabled"))
                    for sample in group_samples
                    if isinstance(sample.get("load_output_enabled"), bool)
                }
            ),
            "load_status_generations": generations,
            "load_status_generation_count": len(generations),
            "load_status_max_age_s": max(
                [
                    sample.get("load_status_age_s")
                    for sample in group_samples
                    if isinstance(sample.get("load_status_age_s"), (int, float))
                ],
                default=None,
            ),
            "isolapurr_port_c_mv": summarize_numeric(group_samples, "isolapurr_port_c_mv"),
            "isolapurr_port_c_ma": summarize_numeric(group_samples, "isolapurr_port_c_ma"),
            "assist_target_vout_mv": summarize_numeric(group_samples, "assist_target_vout_mv"),
            "vin_vbus_mv": summarize_numeric(group_samples, "vin_vbus_mv"),
            "vin_iin_ma": summarize_numeric(group_samples, "vin_iin_ma"),
            "tps_total_iout_ma": summarize_numeric(group_samples, "tps_total_iout_ma"),
            "battery_current_ma": summarize_numeric(group_samples, "battery_current_ma"),
            "diag_assist_target_vout_mv": summarize_numeric(
                group_samples,
                "diag_assist_target_vout_mv",
            ),
            "diag_vin_baseline_mv": summarize_numeric(group_samples, "diag_vin_baseline_mv"),
            "diag_vin_drop_mv": summarize_numeric(group_samples, "diag_vin_drop_mv"),
            "diag_tps_total_iout_ma": summarize_numeric(
                group_samples,
                "diag_tps_total_iout_ma",
            ),
            "ups_vout_mv": summarize_numeric(group_samples, "ups_vout_mv"),
            "out_a_vbus_mv": out_a_summary,
            "out_b_vbus_mv": out_b_summary,
            "load_v_local_mv": load_v_summary,
            "load_i_total_ma": summarize_numeric(group_samples, "load_i_total_ma"),
            "sample_spacing": summarize_sample_spacing(group_samples),
            "output_voltage_fluctuation": {
                "ups_vout_mv": summarize_numeric(group_samples, "ups_vout_mv"),
                "out_a_vbus_mv": out_a_summary,
                "out_b_vbus_mv": out_b_summary,
                "load_v_local_mv": load_v_summary,
                "out_a_out_b_gap_mv": summarize_gap(group_samples, "out_a_vbus_mv", "out_b_vbus_mv"),
                "minimum_observed_output_voltage": summarize_minimum_output_sample(group_samples),
            },
            "completeness": evaluate_group_completeness(
                group_samples,
                load_status_max_age_seconds=load_status_max_age_seconds,
                expected_duration_seconds=(expected_tag_durations or {}).get(
                    str(group_samples[0].get("tag") or "")
                ),
                nominal_interval_seconds=nominal_interval_seconds,
            ),
        }

    overall = summarize_group(samples)
    by_tag_summary = {tag: summarize_group(group_samples) for tag, group_samples in by_tag.items()}
    if expected_tags:
        completeness = dict(overall.get("completeness") or {})
        failures = list(completeness.get("failures") or [])
        observed_tags = sorted(by_tag_summary.keys())
        for tag in expected_tags:
            if tag not in by_tag_summary:
                failures.append(f"missing_tag_{tag}")
            elif not by_tag_summary[tag].get("completeness", {}).get("scene_complete"):
                failures.append(f"incomplete_tag_{tag}")
        completeness["expected_tags"] = expected_tags
        completeness["observed_tags"] = observed_tags
        completeness["failures"] = failures
        completeness["scene_complete"] = not failures
        overall["completeness"] = completeness
    return {
        "all": overall,
        "by_tag": by_tag_summary,
    }


def build_console_summary(
    run_dir: Path,
    payload: dict[str, Any],
    *,
    success: bool,
) -> dict[str, Any]:
    summary = payload.get("summary") or {}
    overall = summary.get("all") or {}
    tags = summary.get("by_tag") or {}
    return {
        "success": success,
        "run_dir": str(run_dir),
        "profile_name": payload.get("metadata", {}).get("profile_name"),
        "target_ma": payload.get("metadata", {}).get("target_ma"),
        "scene_complete": overall.get("completeness", {}).get("scene_complete"),
        "failures": overall.get("completeness", {}).get("failures"),
        "tag_modes": {
            tag: info.get("mode_set")
            for tag, info in tags.items()
            if isinstance(info, dict)
        },
        "tag_stages": {
            tag: info.get("stage_set")
            for tag, info in tags.items()
            if isinstance(info, dict)
        },
        "minimum_observed_output_voltage": (
            overall.get("output_voltage_fluctuation", {}) or {}
        ).get("minimum_observed_output_voltage"),
        "error": payload.get("error"),
    }


def finalize_run_artifacts(
    *,
    run_dir: Path,
    metadata: dict[str, Any],
    settings_snapshot: Any,
    actions: list[dict[str, Any]],
    samples: list[dict[str, Any]],
    expected_tags: list[str],
    load_status_max_age_seconds: float,
    error: str | None = None,
) -> dict[str, Any]:
    summary = (
        summarize_samples(
            samples,
            expected_tags=expected_tags,
            load_status_max_age_seconds=load_status_max_age_seconds,
            expected_tag_durations=build_expected_tag_durations(metadata),
            nominal_interval_seconds=metadata.get("interval_seconds"),
        )
        if samples
        else None
    )
    payload = {
        "metadata": metadata,
        "settings_snapshot": settings_snapshot,
        "actions": actions,
        "samples": samples,
        "summary": summary,
    }
    if summary is not None:
        write_json(run_dir / "summary.json", summary)
    if error is not None:
        payload["error"] = error
        write_json(run_dir / "failure.json", payload)
    else:
        write_json(run_dir / "results.json", payload)
    return payload


def build_expected_tag_durations(metadata: dict[str, Any]) -> dict[str, float]:
    durations: dict[str, float] = {
        "pre": float(metadata.get("pre_seconds") or 0.0),
        "hold": float(metadata.get("hold_seconds") or 0.0),
        "post": float(metadata.get("post_seconds") or 0.0),
    }
    if metadata.get("include_backup"):
        durations["backup"] = float(metadata.get("backup_hold_seconds") or 0.0)
        durations["restore"] = float(metadata.get("restore_hold_seconds") or 0.0)
    return durations


def build_preflight(
    args: argparse.Namespace,
    settings_payload: Any,
    *,
    known_load_disabled: bool = False,
    initial_load_status: Any | None = None,
) -> dict[str, Any]:
    isolapurr_ports = fetch_isolapurr_ports(
        args.isolapurr_url,
        timeout_sec=min(args.status_timeout_sec, 5.0),
    )
    port_c_entry = dict_or_empty(port_state(isolapurr_ports, port_id="port_c"))
    port_c_telemetry = dict_or_empty(port_c_entry.get("telemetry"))
    port_c_state = dict_or_empty(port_c_entry.get("state"))
    load_status = initial_load_status
    if not normalize_verified_load_payload(load_status):
        load_status = get_load_status_best_effort(
            args,
            args.load_device,
            timeout_sec=args.status_timeout_sec,
            bridge_lease=None,
        )
    load_control = get_load_control_best_effort(
        args,
        args.load_device,
        timeout_sec=args.status_timeout_sec,
        bridge_lease=None,
    )
    ups_status = http_json_with_retries(
        args.ups_status_url,
        timeout_sec=min(args.status_timeout_sec, 5.0),
    )
    diag_snapshot = unwrap_diag_snapshot_payload(
        fetch_diag_snapshot_with_fallback(
            devd_diag_snapshot_url=args.devd_diag_snapshot_url,
            ups_status=dict_or_empty(ups_status),
            timeout_sec=min(args.status_timeout_sec, 5.0),
        )
    )
    effective_enabled = load_output_enabled(normalize_verified_load_payload(load_status))
    effective_target_i_ma = load_target_i_ma(normalize_verified_load_payload(load_status))
    if effective_enabled is None or effective_target_i_ma is None:
        effective_enabled, effective_target_i_ma = select_effective_load_state(load_control, load_status)
    ups_input = dict_or_empty(ups_status.get("input")) if isinstance(ups_status, dict) else {}
    ups_charger = dict_or_empty(ups_status.get("charger")) if isinstance(ups_status, dict) else {}
    diag_input = dict_or_empty(diag_snapshot.get("input"))
    vin_recovered_candidates = (
        ("status_vin_vbus_mv", ups_input.get("vin_vbus_mv")),
        ("diag_vin_vbus_mv", diag_input.get("vin_vbus_mv")),
        ("status_input_vbus_mv", ups_input.get("input_vbus_mv")),
        ("diag_input_vbus_mv", diag_input.get("input_vbus_mv")),
        ("isolapurr_port_c_voltage_mv", port_c_telemetry.get("voltage_mv")),
    )
    vin_recovered_mv = None
    vin_recovered_source = None
    for source_name, value in vin_recovered_candidates:
        if isinstance(value, (int, float)):
            vin_recovered_mv = int(value)
            vin_recovered_source = source_name
            break
    charger_detail_status = ups_charger.get("detail_status")
    charger_allow_charge = ups_charger.get("allow_charge")
    mains_present = ups_input.get("mains_present")
    input_source = ups_input.get("source")
    gate_failures: list[str] = []
    if not isinstance(port_c_telemetry.get("voltage_mv"), int):
        gate_failures.append("isolapurr_port_c_voltage_missing")
    if port_c_state.get("power_enabled") is not True:
        gate_failures.append("isolapurr_port_c_not_enabled")
    if not isinstance(ups_status, dict):
        gate_failures.append("ups_status_unavailable")
    if not isinstance(diag_snapshot, dict):
        gate_failures.append("diag_snapshot_unavailable")
    if not isinstance(settings_payload, dict):
        gate_failures.append("ups_settings_unavailable")
    if effective_enabled is not False and not known_load_disabled:
        gate_failures.append("load_not_disabled_before_scene")
    if effective_enabled is None and known_load_disabled:
        gate_failures = [
            failure for failure in gate_failures if failure != "load_not_disabled_before_scene"
        ]
    if charger_allow_charge is True:
        gate_failures.append("charger_not_idle_before_scene")
    if charger_detail_status not in {"WAIT", "LOAD", "NOAC", "LOCK"}:
        gate_failures.append("charger_detail_status_not_idle_before_scene")
    if (
        not isinstance(vin_recovered_mv, int)
        or vin_recovered_mv < 11_950
        or mains_present is not True
        or input_source != "dcin"
    ):
        gate_failures.append("vin_not_recovered_before_scene")
    return {
        "scene_valid": not gate_failures,
        "failures": gate_failures,
        "isolapurr": {
            "port_c_enabled": port_c_state.get("power_enabled"),
            "port_c_voltage_mv": port_c_telemetry.get("voltage_mv"),
            "port_c_current_ma": port_c_telemetry.get("current_ma"),
        },
        "ups": {
            "mode": ups_status.get("mode") if isinstance(ups_status, dict) else None,
            "mains_present": (
                (ups_status.get("input") or {}).get("mains_present")
                if isinstance(ups_status, dict)
                else None
            ),
            "assist_power_stage": (
                (ups_status.get("input") or {}).get("assist_power_stage")
                if isinstance(ups_status, dict)
                else None
            ),
            "vin_vbus_mv": ups_input.get("vin_vbus_mv"),
            "vin_recovered_mv": vin_recovered_mv,
            "vin_recovered_source": vin_recovered_source,
            "source": input_source,
        },
        "charger": {
            "allow_charge": charger_allow_charge,
            "detail_status": charger_detail_status,
        },
        "diag_snapshot": {
            "source": dict_or_empty(diag_snapshot).get("source"),
        },
        "load": {
            "output_enabled": effective_enabled,
            "target_i_ma": effective_target_i_ma,
            "status": load_status,
            "control": load_control,
        },
    }


def main() -> int:
    args = parse_args()
    report_root = Path(args.report_root)
    timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    run_dir = report_root / f"{timestamp}-{args.profile_name}"
    run_dir.mkdir(parents=True, exist_ok=False)
    jsonl_path = run_dir / "timeseries.jsonl"

    metadata = {
        "profile_name": args.profile_name,
        "started_at_utc": datetime.now(timezone.utc).isoformat(),
        "target_ma": args.target_ma,
        "load_device": args.load_device,
        "load_usb_port": args.load_usb_port,
        "load_ipc": args.load_ipc,
        "ups_status_url": args.ups_status_url,
        "ups_settings_url": args.ups_settings_url,
        "devd_diag_snapshot_url": args.devd_diag_snapshot_url,
        "isolapurr_url": args.isolapurr_url,
        "pre_seconds": args.pre_seconds,
        "hold_seconds": args.hold_seconds,
        "post_seconds": args.post_seconds,
        "interval_seconds": args.interval_seconds,
        "command_timeout_sec": args.command_timeout_sec,
        "status_timeout_sec": args.status_timeout_sec,
        "verify_timeout_sec": args.verify_timeout_sec,
        "max_i_ma_total": args.max_i_ma_total,
        "max_p_mw": args.max_p_mw,
        "load_refresh_seconds": args.load_refresh_seconds,
        "include_backup": args.include_backup,
        "backup_hold_seconds": args.backup_hold_seconds,
        "restore_hold_seconds": args.restore_hold_seconds,
        "backup_stimulus": args.backup_stimulus,
        "backup_low_voltage_mv": args.backup_low_voltage_mv,
        "baseline_source_voltage_mv": args.baseline_source_voltage_mv,
        "baseline_source_current_limit_ma": args.baseline_source_current_limit_ma,
    }
    actions: list[dict[str, Any]] = []
    samples: list[dict[str, Any]] = []
    load_status_poller: LoadStatusPoller | None = None
    sampler: ContinuousSampler | None = None
    settings_snapshot: Any = None
    expected_tags = ["pre", "hold"]
    if args.include_backup:
        expected_tags.extend(["backup", "restore"])
    expected_tags.append("post")

    try:
        settings_payload = http_json_with_retries(
            args.ups_settings_url,
            timeout_sec=min(args.status_timeout_sec, 5.0),
        )
        settings_snapshot = {
            "advanced_power": settings_payload.get("advanced_power"),
            "advanced_power_capabilities": settings_payload.get("advanced_power_capabilities"),
        }
        write_json(run_dir / "settings_snapshot.json", settings_snapshot)
        actions.append(
            {
                "ensure_usb_port": ensure_usb_port(
                    args,
                    args.load_device,
                    args.load_usb_port,
                    timeout_sec=args.status_timeout_sec,
                )
            }
        )
        actions.append(
            {
                "disable_before_start": disable_load(
                    args,
                    args.load_device,
                    timeout_sec=args.command_timeout_sec,
                    status_timeout_sec=args.status_timeout_sec,
                    verify_timeout_sec=args.verify_timeout_sec,
                )
            }
        )
        disable_before_start = actions[-1]["disable_before_start"]
        disable_before_start_verified = dict_or_empty(disable_before_start.get("verified_status"))
        preflight = build_preflight(
            args,
            settings_payload,
            known_load_disabled=disable_before_start_verified.get("effective_enabled") is False,
            initial_load_status=actions[-1]["disable_before_start"].get("status"),
        )
        write_json(run_dir / "preflight.json", preflight)
        if not preflight.get("scene_valid"):
            raise RuntimeError(f"preflight_failed: {preflight.get('failures')}")
        initial_load_status = (
            actions[-1]["disable_before_start"].get("verified_status")
            or actions[-1]["disable_before_start"].get("status")
        )
        load_status_poller = LoadStatusPoller(
            args,
            args.load_device,
            timeout_sec=args.status_timeout_sec,
            poll_interval_sec=args.load_refresh_seconds,
        )
        load_status_poller.replace_status(initial_load_status)
        load_status_poller.start()
        started_at = time.monotonic()
        sampler = ContinuousSampler(
            jsonl_path=jsonl_path,
            started_at=started_at,
            ups_status_url=args.ups_status_url,
            devd_diag_snapshot_url=args.devd_diag_snapshot_url,
            isolapurr_url=args.isolapurr_url,
            status_timeout_sec=args.status_timeout_sec,
            load_status_poller=load_status_poller,
            interval_seconds=args.interval_seconds,
            initial_tag="pre",
        )
        sampler.start()
        sleep_with_sampler(sampler, seconds=args.pre_seconds, collect_into=samples)
        hold_generation_start = load_status_poller.snapshot(time.monotonic()).get("generation")
        if not isinstance(hold_generation_start, int):
            hold_generation_start = 0
        bridge_control_lease = None
        if args.load_bridge_url:
            bridge_control_lease = load_status_poller.wait_for_bridge_lease(
                timeout_sec=max(2.0, min(args.status_timeout_sec, 5.0)),
            )
        hold_pause_idle = True
        if bridge_control_lease is None:
            hold_pause_idle = pause_load_status_poller_for_control(
                load_status_poller,
                idle_timeout_sec=max(
                    args.load_poller_timeout_sec,
                    args.status_timeout_sec + 5.0,
                ),
                release_bridge_lease=True,
            )
        actions.append({"hold_pause_idle": hold_pause_idle})
        if not hold_pause_idle:
            raise RuntimeError("load_status_poller_not_idle_before_hold_control")
        actions.append(
            {
                "cc_target": load_cc(
                    args,
                    args.load_device,
                    args.target_ma,
                    max_i_ma_total=args.max_i_ma_total,
                    max_p_mw=args.max_p_mw,
                    timeout_sec=args.command_timeout_sec,
                    status_timeout_sec=args.status_timeout_sec,
                    verify_timeout_sec=args.verify_timeout_sec,
                    bridge_lease=bridge_control_lease,
                )
            }
        )
        if bridge_control_lease is None:
            load_status_poller.resume()
        load_status_poller.replace_status(
            verified_load_payload_to_status_payload(
                actions[-1]["cc_target"].get("verified_status")
            )
        )
        actions.append(
            {
                "hold_state_observed": wait_for_poller_state(
                    load_status_poller,
                    min_generation=hold_generation_start,
                    expected_enabled=True,
                    expected_target_i_ma=args.target_ma,
                    timeout_sec=args.status_timeout_sec + args.verify_timeout_sec,
                )
            }
        )
        sampler.set_tag("hold")
        sleep_with_sampler(sampler, seconds=args.hold_seconds, collect_into=samples)
        if args.include_backup:
            if args.backup_stimulus == "low_voltage":
                actions.append(
                    {
                        "source_low_voltage_for_backup": set_isolapurr_manual_output(
                            args.isolapurr_url,
                            voltage_mv=args.backup_low_voltage_mv,
                            current_limit_ma=args.baseline_source_current_limit_ma,
                        )
                    }
                )
            else:
                actions.append({"port_c_disable_for_backup": set_port_c_power(args.isolapurr_url, False)})
            sampler.set_tag("backup")
            sleep_with_sampler(sampler, seconds=args.backup_hold_seconds, collect_into=samples)
            if args.backup_stimulus == "low_voltage":
                actions.append(
                    {
                        "source_restore_after_backup": set_isolapurr_manual_output(
                            args.isolapurr_url,
                            voltage_mv=args.baseline_source_voltage_mv,
                            current_limit_ma=args.baseline_source_current_limit_ma,
                        )
                    }
                )
            actions.append({"port_c_enable_after_backup": set_port_c_power(args.isolapurr_url, True)})
            sampler.set_tag("restore")
            sleep_with_sampler(sampler, seconds=args.restore_hold_seconds, collect_into=samples)
        final_load_status: Any = None
        try:
            post_generation_start = load_status_poller.snapshot(time.monotonic()).get("generation")
            if not isinstance(post_generation_start, int):
                post_generation_start = 0
            bridge_control_lease = None
            if args.load_bridge_url:
                bridge_control_lease = load_status_poller.wait_for_bridge_lease(
                    timeout_sec=max(2.0, min(args.status_timeout_sec, 5.0)),
                )
            post_pause_idle = True
            if bridge_control_lease is None:
                post_pause_idle = pause_load_status_poller_for_control(
                    load_status_poller,
                    idle_timeout_sec=max(
                        args.load_poller_timeout_sec,
                        args.status_timeout_sec + 5.0,
                    ),
                    release_bridge_lease=True,
                )
            actions.append({"post_pause_idle": post_pause_idle})
            if not post_pause_idle:
                raise RuntimeError("load_status_poller_not_idle_before_post_control")
            disable_after_target = disable_load(
                args,
                args.load_device,
                timeout_sec=args.command_timeout_sec,
                status_timeout_sec=args.status_timeout_sec,
                verify_timeout_sec=args.verify_timeout_sec,
                bridge_lease=bridge_control_lease,
            )
            actions.append({"disable_after_target": disable_after_target})
            final_load_status = (
                disable_after_target.get("verified_status")
                or disable_after_target.get("status")
            )
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired, RuntimeError) as exc:
            fallback_control = get_load_control_best_effort(
                args,
                args.load_device,
                timeout_sec=args.status_timeout_sec,
            )
            bridge_lease = None
            if args.load_bridge_url:
                bridge_lease = acquire_load_bridge_lease(
                    args,
                    timeout_sec=args.status_timeout_sec,
                    bridge_device=resolve_load_bridge_device_id(
                        args,
                        timeout_sec=min(args.status_timeout_sec, 5.0),
                    ),
                )
            fallback_status = get_load_status_best_effort(
                args,
                args.load_device,
                timeout_sec=args.status_timeout_sec,
                bridge_lease=bridge_lease,
            )
            actions.append(
                {
                    "disable_after_target_failed": {
                        "error": repr(exc),
                        "control": fallback_control,
                        "status": fallback_status,
                    }
                }
            )
            final_load_status = fallback_status
        finally:
            if bridge_control_lease is None:
                load_status_poller.resume()
        load_status_poller.replace_status(verified_load_payload_to_status_payload(final_load_status))
        try:
            actions.append(
                {
                    "post_state_observed": wait_for_poller_state(
                        load_status_poller,
                        min_generation=post_generation_start,
                        expected_enabled=False,
                        expected_target_i_ma=None,
                        timeout_sec=args.status_timeout_sec + args.verify_timeout_sec,
                    )
                }
            )
        except RuntimeError as post_state_exc:
            actions.append({"post_state_observed_failed": repr(post_state_exc)})
        sampler.set_tag("post")
        sampler.wait_for_tag_sample_count(
            "post",
            min_count=1,
            timeout_sec=max(2.0, args.interval_seconds * 6.0),
        )
        sleep_with_sampler(sampler, seconds=args.post_seconds, collect_into=samples)
        sampler.stop(timeout_sec=args.status_timeout_sec + 5.0)
        samples = sampler.snapshot_samples()
        sampler = None
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired, RuntimeError, KeyboardInterrupt, Exception) as exc:
        try:
            if sampler is not None:
                try:
                    sampler.stop(timeout_sec=3.0)
                except Exception as sampler_exc:  # noqa: BLE001
                    actions.append({"sampler_stop_after_error_failed": repr(sampler_exc)})
                samples = sampler.snapshot_samples()
            actions.append(
                {
                    "cleanup_disable_after_error": disable_load(
                        args,
                        args.load_device,
                        timeout_sec=args.command_timeout_sec,
                        status_timeout_sec=args.status_timeout_sec,
                        verify_timeout_sec=args.verify_timeout_sec,
                    )
                }
            )
        except Exception as cleanup_exc:  # noqa: BLE001
            actions.append({"cleanup_disable_after_error_failed": repr(cleanup_exc)})
        payload = finalize_run_artifacts(
            run_dir=run_dir,
            metadata=metadata,
            settings_snapshot=settings_snapshot,
            actions=actions,
            samples=samples,
            expected_tags=expected_tags,
            load_status_max_age_seconds=args.load_status_max_age_seconds,
            error=repr(exc),
        )
        print(
            json.dumps(
                build_console_summary(run_dir, payload, success=False),
                ensure_ascii=False,
                indent=2,
            )
        )
        if load_status_poller is not None:
            load_status_poller.stop(timeout_sec=args.status_timeout_sec + 5.0)
        return 1

    payload = finalize_run_artifacts(
        run_dir=run_dir,
        metadata=metadata,
        settings_snapshot=settings_snapshot,
        actions=actions,
        samples=samples,
        expected_tags=expected_tags,
        load_status_max_age_seconds=args.load_status_max_age_seconds,
    )
    print(
        json.dumps(
            build_console_summary(run_dir, payload, success=True),
            ensure_ascii=False,
            indent=2,
        )
    )
    if load_status_poller is not None:
        load_status_poller.stop(timeout_sec=args.status_timeout_sec + 5.0)
    return 0


if __name__ == "__main__":
    sys.exit(main())
