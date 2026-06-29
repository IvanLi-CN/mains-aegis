#!/usr/bin/env python3
from __future__ import annotations

import argparse
import concurrent.futures
import json
import os
import select
import signal
import socket
import subprocess
import sys
import threading
import time
import urllib.parse
import urllib.request
from collections.abc import Callable
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


DEFAULT_LOAD_DEVICE = "loadlynx-d68638"
DEFAULT_LOAD_USB_DEVICE_ID = "digital-2bdfc170893f"
DEFAULT_LOAD_USB_PORT = "/dev/cu.usbmodem212101"
DEFAULT_UPS_USB_DEVICE_ID = "serial-04f3bb3f5367"
DEFAULT_LOAD_BRIDGE_DEVICE = ""
DEFAULT_LOAD_IPC = ""
DEFAULT_LOAD_DEVD_BASE_URL = "http://127.0.0.1:20641"
DEFAULT_LOAD_DEVD_SOCKET = "/var/folders/nl/qbk0flf9607bv21rd_7d042c0000gn/T/loadlynx-devd.sock"
DEFAULT_LOAD_CLI = str(Path.home() / ".local" / "bin" / "loadlynx")
DEFAULT_LOAD_BRIDGE_URL = "http://127.0.0.1:30180"
DEFAULT_UPS_OBSERVE_DEVICE_ID = (
    os.environ.get("MAINS_AEGIS_OBSERVE_DEVICE_ID") or DEFAULT_UPS_USB_DEVICE_ID
)


def default_mains_aegis_devd_base_url() -> str:
    return (
        os.environ.get("MAINS_AEGIS_DEVD_URL")
        or os.environ.get("VITE_DEFAULT_DEVD_URL")
        or os.environ.get("VITE_DEVD_API_BASE")
        or "http://127.0.0.1:30080"
    )


DEFAULT_UPS_STATUS_URL = (
    f"{default_mains_aegis_devd_base_url().rstrip('/')}/api/v1/devices/{DEFAULT_UPS_OBSERVE_DEVICE_ID}/status"
    "?include_meta=true&watch_freshness_ms=333"
)
DEFAULT_UPS_SETTINGS_URL = (
    f"{default_mains_aegis_devd_base_url().rstrip('/')}/api/v1/devices/{DEFAULT_UPS_OBSERVE_DEVICE_ID}/settings"
)
DEFAULT_DEVD_POWER_DIAG_URL = (
    f"{default_mains_aegis_devd_base_url().rstrip('/')}/api/v1/devices/{DEFAULT_UPS_OBSERVE_DEVICE_ID}/power-diag"
    "?include_meta=true&watch_freshness_ms=333"
)
DEFAULT_DEVD_MONITOR_START_URL = (
    f"{default_mains_aegis_devd_base_url().rstrip('/')}/api/v1/devices/{DEFAULT_UPS_OBSERVE_DEVICE_ID}/monitor/start"
)
DEFAULT_DEVD_DEVICE_TRACE_URL = (
    f"{default_mains_aegis_devd_base_url().rstrip('/')}/api/v1/devices/{DEFAULT_UPS_OBSERVE_DEVICE_ID}/trace?trace_limit=1"
)
DEFAULT_DEVD_SCAN_URL = f"{default_mains_aegis_devd_base_url().rstrip('/')}/api/v1/devices/scan"
DEFAULT_MAINS_AEGIS_CLI = str(
    Path(__file__).resolve().parent.parent / "mains-aegis-host" / "target" / "debug" / "mains-aegis"
)
DEFAULT_ISOLAPURR_URL = "http://192.168.31.122"
DEFAULT_ISOLAPURR_DEVICE_ID = "856a141cdbd4"
DEFAULT_ISOLAPURR_CLI = "isolapurr"
DEFAULT_COMMAND_TIMEOUT_SECONDS = 45.0
DEFAULT_STATUS_TIMEOUT_SECONDS = 20.0
DEFAULT_VERIFY_TIMEOUT_SECONDS = 45.0
DEFAULT_LOAD_STATUS_POLL_TIMEOUT_SECONDS = 3.0
DEFAULT_SOURCE_VOLTAGE_MV = 12000
DEFAULT_SOURCE_CURRENT_LIMIT_MA = 3000
DEFAULT_LOAD_MIN_V_MV = 3000
DEFAULT_MAX_I_MA_TOTAL = 4000
DEFAULT_MAX_P_MW = 80000
DEFAULT_SAMPLE_INTERVAL_SECONDS = 0.25
DEFAULT_LOAD_STREAM_INTERVAL_SECONDS = 0.1
DEFAULT_LOAD_STATUS_READY_TIMEOUT_SECONDS = 20.0
DEFAULT_SAMPLE_READ_RETRIES = 1
DEFAULT_SAMPLE_READ_RETRY_DELAY_SECONDS = 0.05
DEFAULT_LOAD_STATE_VERIFY_POLL_SECONDS = 0.2
DEFAULT_LOAD_TELEMETRY_PROBE = "tools/hil/probe_loadlynx_released_telemetry.py"
DEFAULT_LOAD_IPC_STATUS_HELPER = "tools/hil/loadlynx_ipc_status_helper.py"
FORMAL_TARGET_SAMPLE_RATE_HZ = 3.0
FORMAL_MIN_EFFECTIVE_SAMPLE_RATE_HZ = 2.0
FORMAL_MAX_SAMPLE_GAP_SECONDS = 0.5
FORMAL_MAX_REALTIME_SAMPLE_AGE_SECONDS = 0.5
FORMAL_MAX_CONFIGURED_SAMPLE_INTERVAL_SECONDS = 1.0 / FORMAL_MIN_EFFECTIVE_SAMPLE_RATE_HZ
FORMAL_MIN_SOURCE_CUT_VIN_DELTA_MV = 500
UPS_INPUT_CUT_MAX_VIN_MV = 2999
TRANSIENT_HTTP_STATUS_CODES = {502, 503, 504}
PORT_C_POWER_PATH = "/api/v1/ports/port_c/power"
PORT_C_STATUS_PATH = "/api/v1/ports/port_c"
PORTS_PATH = "/api/v1/ports"
NO_IN_WINDOW_LOAD_REFRESH_SECONDS = 1_000_000_000.0
SCHEDULED_ACTION_TIMEOUT_MARGIN_SECONDS = 5.0
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


class LoadStatusPoller:
    def __init__(
        self,
        args: argparse.Namespace,
        load_device: str,
        *,
        timeout_sec: float,
        poll_interval_sec: float,
        stream_interval_sec: float,
        use_status_stream: bool,
    ) -> None:
        self._args = args
        self._load_device = load_device
        self._bridge_device = (
            resolve_load_bridge_device_id(args, timeout_sec=timeout_sec)
            if effective_load_bridge_url(args)
            else load_device
        )
        self._timeout_sec = timeout_sec
        self._poll_interval_sec = max(0.1, poll_interval_sec)
        self._stream_interval_sec = max(0.1, stream_interval_sec)
        self._use_status_stream = use_status_stream
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
        self._stream_process: subprocess.Popen[str] | None = None
        self._last_stream_line_monotonic: float | None = None
        self._status_source: str | None = None
        self._status_stream_supported = use_status_stream
        self._bridge_lease_id: str | None = None
        self._bridge_lease_acquired_at_monotonic: float | None = None
        self._bridge_lease_ttl_ms: int | None = None
        self._bridge_lease_heartbeat_interval_ms: int | None = None
        self._load_devd_lease_id: str | None = None
        self._load_devd_lease_acquired_at_monotonic: float | None = None
        self._load_devd_lease_ttl_ms: int | None = None
        self._load_devd_http_retry_not_before_monotonic: float | None = None

    def effective_status_source_mode(self) -> str:
        if self._use_status_stream:
            return "status-stream"
        load_devd_socket = resolve_load_devd_socket(self._args)
        if load_devd_socket:
            return "ipc-helper-poll"
        return "poll"

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
                self._generation += 1
                self._error = None
                self._status_source = "replace"
            elif isinstance(normalized, dict):
                self._fetched_at_monotonic = time.monotonic()
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
        if not effective_load_bridge_url(self._args):
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

    def load_devd_lease_snapshot(self) -> dict[str, Any] | None:
        lease_id = self._load_devd_lease_id
        if not isinstance(lease_id, str) or not lease_id:
            return None
        return {
            "lease_id": lease_id,
            "device_id": self._args.load_usb_device_id,
            "lease_ttl_ms": self._load_devd_lease_ttl_ms,
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
                "payload": self._latest_status,
                "status": self._latest_status,
                "generation": self._generation,
                "age_s": age_s,
                "sample_age_s": sample_age_s,
                "device_generation": self._device_generation,
                "device_sampled_at_ms": self._device_sampled_at_ms,
                "error": self._error,
                "poller_paused": self._pause_event.is_set(),
                "poller_idle": self._idle_event.is_set(),
                "source": self._status_source,
            }

    def generation(self) -> int:
        with self._state_lock:
            return self._generation

    def stop(self, *, timeout_sec: float = 5.0) -> None:
        self._stop_event.set()
        self._pause_event.clear()
        if self._thread is not None:
            self._thread.join(timeout=timeout_sec)
        self._stop_stream_process(timeout_sec=timeout_sec)
        self.release_bridge_lease(timeout_sec=min(timeout_sec, self._timeout_sec))
        self.release_load_devd_lease(timeout_sec=min(timeout_sec, self._timeout_sec))

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

    def release_load_devd_lease(self, *, timeout_sec: float | None = None) -> None:
        lease_id = self._load_devd_lease_id
        self._load_devd_lease_id = None
        self._load_devd_lease_acquired_at_monotonic = None
        self._load_devd_lease_ttl_ms = None
        if not isinstance(lease_id, str) or not lease_id:
            return
        release_load_devd_lease_quietly(
            self._args,
            {"lease_id": lease_id},
            timeout_sec=min(timeout_sec or self._timeout_sec, self._timeout_sec),
        )

    def _run(self) -> None:
        if (
            self._use_status_stream
            and not effective_load_bridge_url(self._args)
        ):
            self._run_status_stream()
            if self._status_stream_supported:
                return
        while not self._stop_event.is_set():
            if self._pause_event.is_set():
                time.sleep(0.05)
                continue
            cycle_started_at = time.monotonic()
            self._idle_event.clear()
            try:
                bridge_lease = self._ensure_bridge_lease()
                if effective_load_bridge_url(self._args):
                    status = get_load_status_via_bridge(
                        self._args,
                        self._load_device,
                        timeout_sec=min(self._timeout_sec, 5.0),
                        bridge_lease=bridge_lease,
                        retries=1,
                        retry_delay_sec=0.0,
                    )
                else:
                    load_devd_socket = resolve_load_devd_socket(self._args)
                    if load_devd_socket:
                        try:
                            status = get_load_status_via_ipc_without_lease(
                                self._args,
                                timeout_sec=min(self._timeout_sec, 2.0),
                                retries=1,
                                retry_delay_sec=0.0,
                            )
                            status = ensure_valid_load_status_payload(
                                status,
                                source="ipc_status_no_lease",
                            )
                        except Exception:
                            load_devd_lease = self._ensure_load_devd_lease()
                            if load_devd_lease is not None:
                                try:
                                    status = get_load_status_via_ipc(
                                        self._args,
                                        timeout_sec=min(self._timeout_sec, 2.0),
                                        load_devd_lease=load_devd_lease,
                                        retries=1,
                                        retry_delay_sec=0.0,
                                    )
                                    status = ensure_valid_load_status_payload(
                                        status,
                                        source="ipc_status",
                                    )
                                except Exception:
                                    status = get_load_status_via_ipc_helper(
                                        self._args,
                                        timeout_sec=min(self._timeout_sec, 2.0),
                                        load_devd_lease=load_devd_lease,
                                        retries=1,
                                        retry_delay_sec=0.0,
                                        scan_first=False,
                                        warmup=False,
                                    )
                                    status = ensure_valid_load_status_payload(
                                        status,
                                        source="ipc_helper_status",
                                    )
                            else:
                                status = get_load_status(
                                    self._args,
                                    self._load_device,
                                    timeout_sec=self._timeout_sec,
                                    bridge_lease=bridge_lease,
                                    load_devd_lease=load_devd_lease,
                                    prefer_bridge=False,
                                    prefer_devd_http=False,
                                )
                    else:
                        load_devd_lease = self._ensure_load_devd_lease()
                        status = get_load_status(
                            self._args,
                            self._load_device,
                            timeout_sec=self._timeout_sec,
                            bridge_lease=bridge_lease,
                            load_devd_lease=load_devd_lease,
                            prefer_devd_http=False,
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
                        self._generation += 1
                        self._error = None
                        payload_source = (
                            normalized.get("source")
                            if isinstance(normalized, dict)
                            and isinstance(normalized.get("source"), str)
                            else None
                        )
                        self._status_source = payload_source or (
                            "bridge-http" if effective_load_bridge_url(self._args) else "poll"
                        )
                    else:
                        self._error = "invalid_status_payload"
                if not valid_status:
                    self.release_bridge_lease(timeout_sec=min(self._timeout_sec, 5.0))
                    self.release_load_devd_lease(timeout_sec=min(self._timeout_sec, 5.0))
            except (
                subprocess.TimeoutExpired,
                subprocess.CalledProcessError,
                json.JSONDecodeError,
            ) as exc:
                with self._state_lock:
                    self._fetched_at_monotonic = time.monotonic()
                    self._error = repr(exc)
            except Exception as exc:  # noqa: BLE001
                self.release_bridge_lease(timeout_sec=min(self._timeout_sec, 5.0))
                self.release_load_devd_lease(timeout_sec=min(self._timeout_sec, 5.0))
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
        bridge_url = effective_load_bridge_url(self._args).rstrip("/")
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

    def _ensure_load_devd_lease(self) -> dict[str, Any] | None:
        if not (
            (self._args.load_devd_base_url or "").strip()
            or resolve_load_devd_socket(self._args)
        ):
            return None
        now = time.monotonic()
        retry_not_before = self._load_devd_http_retry_not_before_monotonic
        if isinstance(retry_not_before, (int, float)) and now < retry_not_before:
            return None
        if (
            self._load_devd_lease_id
            and self._load_devd_lease_acquired_at_monotonic is not None
            and self._load_devd_lease_ttl_ms is not None
        ):
            ttl_s = self._load_devd_lease_ttl_ms / 1000.0
            if now - self._load_devd_lease_acquired_at_monotonic < max(1.0, ttl_s / 2.0):
                return {"lease_id": self._load_devd_lease_id}
        try:
            lease = acquire_load_devd_lease(self._args, timeout_sec=self._timeout_sec)
        except Exception:  # noqa: BLE001
            self._load_devd_lease_id = None
            self._load_devd_lease_acquired_at_monotonic = None
            self._load_devd_lease_ttl_ms = None
            self._load_devd_http_retry_not_before_monotonic = now + max(
                1.0,
                self._poll_interval_sec,
            )
            return None
        lease_id = lease.get("lease_id")
        if isinstance(lease_id, str) and lease_id:
            self._load_devd_lease_id = lease_id
            self._load_devd_lease_acquired_at_monotonic = now
            ttl_ms = lease.get("lease_ttl_ms")
            self._load_devd_lease_ttl_ms = ttl_ms if isinstance(ttl_ms, int) else 8000
            self._load_devd_http_retry_not_before_monotonic = None
            if resolve_load_devd_socket(self._args):
                try:
                    warm_load_status_via_ipc(
                        self._args,
                        timeout_sec=max(self._timeout_sec, 6.0),
                        load_devd_lease={"lease_id": lease_id},
                    )
                except Exception:  # noqa: BLE001
                    try:
                        get_load_status_via_ipc_helper(
                            self._args,
                            timeout_sec=max(self._timeout_sec, 6.0),
                            load_devd_lease={"lease_id": lease_id},
                            retries=1,
                            retry_delay_sec=0.0,
                            scan_first=False,
                            warmup=True,
                        )
                    except Exception:
                        pass
            return {"lease_id": lease_id}
        return None

    def _start_stream_process(self) -> subprocess.Popen[str]:
        rate_hz = max(2, round(1.0 / self._stream_interval_sec))
        process = subprocess.Popen(
            loadlynx_cmd(
                self._args,
                "status-stream",
                "--device",
                self._load_device,
                "--rate-hz",
                str(rate_hz),
                "--jsonl",
            ),
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        self._last_stream_line_monotonic = time.monotonic()
        return process

    def _stop_stream_process(self, *, timeout_sec: float) -> None:
        process = self._stream_process
        self._stream_process = None
        if process is None:
            return
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=timeout_sec)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=timeout_sec)

    def _run_status_stream(self) -> None:
        while not self._stop_event.is_set():
            if not self._status_stream_supported:
                return
            if self._pause_event.is_set():
                self._stop_stream_process(timeout_sec=2.0)
                self._idle_event.set()
                time.sleep(0.05)
                continue
            if self._stream_process is None:
                try:
                    self._stream_process = self._start_stream_process()
                except Exception as exc:  # noqa: BLE001
                    with self._state_lock:
                        self._fetched_at_monotonic = time.monotonic()
                        self._error = repr(exc)
                    time.sleep(self._poll_interval_sec)
                    continue
            process = self._stream_process
            if process is None:
                continue
            self._idle_event.clear()
            try:
                stdout = process.stdout
                if stdout is None:
                    self._stop_stream_process(timeout_sec=1.0)
                    with self._state_lock:
                        self._fetched_at_monotonic = time.monotonic()
                        self._error = "status_stream_missing_stdout"
                    time.sleep(self._poll_interval_sec)
                    continue
                line = stdout.readline()
                if line == "":
                    stderr_text = ""
                    if process.stderr is not None:
                        stderr_text = process.stderr.read().strip()
                    exit_code = process.poll()
                    self._stop_stream_process(timeout_sec=1.0)
                    with self._state_lock:
                        self._fetched_at_monotonic = time.monotonic()
                        self._error = (
                            f"status_stream_exited code={exit_code} stderr={stderr_text!r}"
                        )
                        if "unrecognized subcommand 'status-stream'" in stderr_text:
                            self._status_stream_supported = False
                            self._status_source = "status-stream-fallback-to-poll"
                            return
                    time.sleep(self._poll_interval_sec)
                    continue
                payload = json.loads(line.strip())
                normalized = normalize_load_status_payload(payload.get("status"))
                valid_status = isinstance(normalized, dict) and isinstance(
                    normalized.get("status"), dict
                )
                self._last_stream_line_monotonic = time.monotonic()
                with self._state_lock:
                    self._latest_status = normalized
                    self._fetched_at_monotonic = time.monotonic()
                    if valid_status:
                        self._generation += 1
                        self._error = None
                        self._status_source = "status-stream"
                    else:
                        self._error = "invalid_status_stream_payload"
            except json.JSONDecodeError as exc:
                with self._state_lock:
                    self._fetched_at_monotonic = time.monotonic()
                    self._error = repr(exc)
            finally:
                self._idle_event.set()


class JsonPoller:
    def __init__(
        self,
        *,
        name: str,
        fetch_fn,
        poll_interval_sec: float,
    ) -> None:
        self._name = name
        self._fetch_fn = fetch_fn
        self._poll_interval_sec = max(0.05, poll_interval_sec)
        self._state_lock = threading.Lock()
        self._stop_event = threading.Event()
        self._thread: threading.Thread | None = None
        self._latest_payload: Any = None
        self._fetched_at_monotonic: float | None = None
        self._generation = 0
        self._error: str | None = None
        self._last_elapsed_ms: int | None = None

    def prime(self, payload: Any) -> None:
        with self._state_lock:
            self._latest_payload = payload
            self._fetched_at_monotonic = time.monotonic()
            self._generation += 1
            self._error = None
            self._last_elapsed_ms = None

    def start(self) -> None:
        if self._thread is not None:
            return
        self._thread = threading.Thread(
            target=self._run,
            name=f"json-poller:{self._name}",
            daemon=True,
        )
        self._thread.start()

    def stop(self, *, timeout_sec: float = 5.0) -> None:
        self._stop_event.set()
        if self._thread is not None:
            self._thread.join(timeout=timeout_sec)

    def snapshot(self, now_monotonic: float) -> dict[str, Any]:
        with self._state_lock:
            age_s = None
            if isinstance(self._fetched_at_monotonic, (int, float)):
                age_s = round(max(0.0, now_monotonic - self._fetched_at_monotonic), 3)
            return {
                "payload": self._latest_payload,
                "generation": self._generation,
                "age_s": age_s,
                "error": self._error,
                "elapsed_ms": self._last_elapsed_ms,
            }

    def _run(self) -> None:
        next_poll_at = time.monotonic()
        while not self._stop_event.is_set():
            started_at = time.monotonic()
            try:
                payload = self._fetch_fn()
                elapsed_ms = int((time.monotonic() - started_at) * 1000)
                with self._state_lock:
                    self._latest_payload = payload
                    self._fetched_at_monotonic = time.monotonic()
                    self._generation += 1
                    self._error = None
                    self._last_elapsed_ms = elapsed_ms
            except Exception as exc:  # noqa: BLE001
                elapsed_ms = int((time.monotonic() - started_at) * 1000)
                with self._state_lock:
                    self._error = repr(exc)
                    self._last_elapsed_ms = elapsed_ms
            next_poll_at += self._poll_interval_sec
            now = time.monotonic()
            if next_poll_at < now:
                skipped_intervals = int((now - next_poll_at) / self._poll_interval_sec) + 1
                next_poll_at += skipped_intervals * self._poll_interval_sec
            while not self._stop_event.is_set():
                remaining = next_poll_at - time.monotonic()
                if remaining <= 0:
                    break
                time.sleep(min(0.02, remaining))


class SseStatusPoller:
    def __init__(self, *, name: str, url: str, timeout_sec: float) -> None:
        self._name = name
        self._url = url
        self._timeout_sec = timeout_sec
        self._state_lock = threading.Lock()
        self._stop_event = threading.Event()
        self._thread: threading.Thread | None = None
        self._latest_payload: Any = None
        self._fetched_at_monotonic: float | None = None
        self._generation = 0
        self._error: str | None = None
        self._last_elapsed_ms: int | None = None

    def prime(self, payload: Any) -> None:
        with self._state_lock:
            self._latest_payload = payload
            self._fetched_at_monotonic = time.monotonic()
            self._generation += 1
            self._error = None
            self._last_elapsed_ms = None

    def start(self) -> None:
        if self._thread is not None:
            return
        self._thread = threading.Thread(
            target=self._run,
            name=f"sse-poller:{self._name}",
            daemon=True,
        )
        self._thread.start()

    def stop(self, *, timeout_sec: float = 5.0) -> None:
        self._stop_event.set()
        if self._thread is not None:
            self._thread.join(timeout=timeout_sec)

    def snapshot(self, now_monotonic: float) -> dict[str, Any]:
        with self._state_lock:
            age_s = None
            if isinstance(self._fetched_at_monotonic, (int, float)):
                age_s = round(max(0.0, now_monotonic - self._fetched_at_monotonic), 3)
            return {
                "payload": self._latest_payload,
                "generation": self._generation,
                "age_s": age_s,
                "error": self._error,
                "elapsed_ms": self._last_elapsed_ms,
                "source": "sse",
            }

    def _run(self) -> None:
        while not self._stop_event.is_set():
            started_at = time.monotonic()
            try:
                request = urllib.request.Request(
                    self._url,
                    headers={"Accept": "text/event-stream"},
                )
                with urllib.request.urlopen(request, timeout=self._timeout_sec) as response:
                    event_name: str | None = None
                    data_lines: list[str] = []
                    while not self._stop_event.is_set():
                        line = response.readline()
                        if line == b"":
                            break
                        text = line.decode("utf-8", errors="replace").strip()
                        if text == "":
                            if event_name == "status" and data_lines:
                                payload = json.loads("\n".join(data_lines))
                                elapsed_ms = int((time.monotonic() - started_at) * 1000)
                                with self._state_lock:
                                    self._latest_payload = payload
                                    self._fetched_at_monotonic = time.monotonic()
                                    self._generation += 1
                                    self._error = None
                                    self._last_elapsed_ms = elapsed_ms
                                started_at = time.monotonic()
                            event_name = None
                            data_lines = []
                        elif text.startswith("event:"):
                            event_name = text.split(":", 1)[1].strip()
                        elif text.startswith("data:"):
                            data_lines.append(text.split(":", 1)[1].strip())
            except Exception as exc:  # noqa: BLE001
                elapsed_ms = int((time.monotonic() - started_at) * 1000)
                with self._state_lock:
                    self._error = repr(exc)
                    self._last_elapsed_ms = elapsed_ms
                self._stop_event.wait(0.2)


class DerivedPowerDiagPoller:
    def __init__(self, source_poller: Any) -> None:
        self._source_poller = source_poller
        self._last_generation: int | None = None
        self._latest_payload: Any = None
        self._fetched_at_monotonic: float | None = None
        self._generation = 0
        self._error: str | None = None
        self._last_elapsed_ms: int | None = None

    def prime(self, payload: Any) -> None:
        self._latest_payload = payload
        self._fetched_at_monotonic = time.monotonic()
        self._generation += 1
        self._error = None
        self._last_elapsed_ms = None

    def start(self) -> None:
        return

    def stop(self, *, timeout_sec: float = 5.0) -> None:
        _ = timeout_sec
        return

    def snapshot(self, now_monotonic: float) -> dict[str, Any]:
        source = self._source_poller.snapshot(now_monotonic)
        source_generation = source.get("generation")
        if isinstance(source_generation, int) and source_generation != self._last_generation:
            self._last_generation = source_generation
            self._latest_payload = derive_power_diag_from_status(
                source.get("payload"),
                source="direct_lan_status_derived",
            )
            fetched_at_monotonic = now_monotonic
            age_s = source.get("age_s")
            if isinstance(age_s, (int, float)):
                fetched_at_monotonic = now_monotonic - float(age_s)
            self._fetched_at_monotonic = fetched_at_monotonic
            self._generation += 1
            self._error = source.get("error")
            self._last_elapsed_ms = source.get("elapsed_ms")
        age_s = None
        if isinstance(self._fetched_at_monotonic, (int, float)):
            age_s = round(max(0.0, now_monotonic - self._fetched_at_monotonic), 3)
        return {
            "payload": self._latest_payload,
            "generation": self._generation,
            "age_s": age_s,
            "error": self._error,
            "elapsed_ms": self._last_elapsed_ms,
        }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run stage-based Advanced Power HIL scenes with three-device evidence."
    )
    parser.add_argument("--profile-name", required=True)
    parser.add_argument(
        "--output-profile",
        choices=("12v", "19v"),
        default="12v",
        help="UPS output profile label recorded in report metadata.",
    )
    parser.add_argument(
        "--scene-type",
        choices=("assist_path", "backup_only"),
        default="assist_path",
        help="Formal scene contract label recorded in report metadata.",
    )
    parser.add_argument("--target-ma", type=int, required=True)
    parser.add_argument("--load-device", default=DEFAULT_LOAD_DEVICE)
    parser.add_argument("--load-usb-device-id", default=DEFAULT_LOAD_USB_DEVICE_ID)
    parser.add_argument("--load-usb-port", default=DEFAULT_LOAD_USB_PORT)
    parser.add_argument("--load-cli", default=DEFAULT_LOAD_CLI)
    parser.add_argument("--load-ipc", default=DEFAULT_LOAD_IPC)
    parser.add_argument("--load-bridge-device", default=DEFAULT_LOAD_BRIDGE_DEVICE)
    parser.add_argument("--load-bridge-url", default=DEFAULT_LOAD_BRIDGE_URL)
    parser.add_argument("--load-devd-base-url", default=DEFAULT_LOAD_DEVD_BASE_URL)
    parser.add_argument("--load-devd-socket", default=DEFAULT_LOAD_DEVD_SOCKET)
    parser.add_argument("--mains-aegis-cli", default=DEFAULT_MAINS_AEGIS_CLI)
    parser.add_argument("--mains-aegis-ipc", default=None)
    parser.add_argument("--ups-device-id", default=DEFAULT_UPS_USB_DEVICE_ID)
    parser.add_argument("--ups-status-url", default=DEFAULT_UPS_STATUS_URL)
    parser.add_argument("--ups-settings-url", default=DEFAULT_UPS_SETTINGS_URL)
    parser.add_argument("--devd-power-diag-url", default=DEFAULT_DEVD_POWER_DIAG_URL)
    parser.add_argument("--devd-monitor-start-url", default=DEFAULT_DEVD_MONITOR_START_URL)
    parser.add_argument("--devd-device-trace-url", default=DEFAULT_DEVD_DEVICE_TRACE_URL)
    parser.add_argument("--devd-scan-url", default=DEFAULT_DEVD_SCAN_URL)
    parser.add_argument("--isolapurr-cli", default=DEFAULT_ISOLAPURR_CLI)
    parser.add_argument("--isolapurr-url", default=DEFAULT_ISOLAPURR_URL)
    parser.add_argument("--isolapurr-device-id", default=DEFAULT_ISOLAPURR_DEVICE_ID)
    parser.add_argument("--source-voltage-mv", type=int, default=DEFAULT_SOURCE_VOLTAGE_MV)
    parser.add_argument(
        "--source-current-limit-ma",
        type=int,
        default=DEFAULT_SOURCE_CURRENT_LIMIT_MA,
    )
    parser.add_argument("--pre-seconds", type=float, default=12.0)
    parser.add_argument("--hold-seconds", type=float, default=18.0)
    parser.add_argument("--backup-hold-seconds", type=float, default=18.0)
    parser.add_argument("--restore-hold-seconds", type=float, default=18.0)
    parser.add_argument("--post-seconds", type=float, default=12.0)
    parser.add_argument("--sample-interval-seconds", type=float, default=DEFAULT_SAMPLE_INTERVAL_SECONDS)
    parser.add_argument(
        "--load-status-source",
        choices=("poll", "status-stream"),
        default="status-stream",
    )
    parser.add_argument(
        "--load-stream-interval-seconds",
        type=float,
        default=DEFAULT_LOAD_STREAM_INTERVAL_SECONDS,
    )
    parser.add_argument(
        "--load-status-ready-timeout-sec",
        type=float,
        default=DEFAULT_LOAD_STATUS_READY_TIMEOUT_SECONDS,
    )
    parser.add_argument("--include-backup", action="store_true")
    parser.add_argument("--command-timeout-sec", type=float, default=DEFAULT_COMMAND_TIMEOUT_SECONDS)
    parser.add_argument("--status-timeout-sec", type=float, default=DEFAULT_STATUS_TIMEOUT_SECONDS)
    parser.add_argument(
        "--load-status-poll-timeout-sec",
        type=float,
        default=DEFAULT_LOAD_STATUS_POLL_TIMEOUT_SECONDS,
    )
    parser.add_argument("--verify-timeout-sec", type=float, default=DEFAULT_VERIFY_TIMEOUT_SECONDS)
    parser.add_argument("--load-min-v-mv", type=int, default=DEFAULT_LOAD_MIN_V_MV)
    parser.add_argument("--max-i-ma-total", type=int, default=DEFAULT_MAX_I_MA_TOTAL)
    parser.add_argument("--max-p-mw", type=int, default=DEFAULT_MAX_P_MW)
    parser.add_argument("--report-root", default="tools/hil/reports")
    parser.add_argument("--load-telemetry-probe", default=DEFAULT_LOAD_TELEMETRY_PROBE)
    parser.add_argument("--skip-load-telemetry-probe", action="store_true")
    parser.add_argument("--print-full-results", action="store_true")
    args = parser.parse_args()
    validate_args(parser, args)
    normalize_load_transport_args(args)
    return args


def validate_args(parser: argparse.ArgumentParser, args: argparse.Namespace) -> None:
    if args.sample_interval_seconds <= 0:
        parser.error("--sample-interval-seconds must be > 0")
    if args.sample_interval_seconds > FORMAL_MAX_CONFIGURED_SAMPLE_INTERVAL_SECONDS:
        parser.error(
            "--sample-interval-seconds must be <= "
            f"{FORMAL_MAX_CONFIGURED_SAMPLE_INTERVAL_SECONDS:.3f}s to satisfy the "
            f"formal >= {FORMAL_MIN_EFFECTIVE_SAMPLE_RATE_HZ:.1f}Hz floor "
            f"(target {FORMAL_TARGET_SAMPLE_RATE_HZ:.1f}Hz)"
        )
    if args.load_stream_interval_seconds <= 0:
        parser.error("--load-stream-interval-seconds must be > 0")
    if (
        args.load_status_source == "status-stream"
        and args.load_stream_interval_seconds > FORMAL_MAX_SAMPLE_GAP_SECONDS
    ):
        parser.error(
            "--load-stream-interval-seconds must be <= "
            f"{FORMAL_MAX_SAMPLE_GAP_SECONDS:.3f}s when --load-status-source=status-stream "
            "to keep formal telemetry freshness within the 0.5s ceiling"
        )
    if args.source_voltage_mv <= 0:
        parser.error("--source-voltage-mv must be > 0")
    if args.source_current_limit_ma <= 0:
        parser.error("--source-current-limit-ma must be > 0")
    if args.load_min_v_mv <= 0:
        parser.error("--load-min-v-mv must be > 0")
    if args.max_i_ma_total <= 0:
        parser.error("--max-i-ma-total must be > 0")
    if args.max_p_mw <= 0:
        parser.error("--max-p-mw must be > 0")
    observe_urls = normalized_observe_urls(args)
    args.ups_status_url = observe_urls["ups_status_url"]
    args.ups_settings_url = observe_urls["ups_settings_url"]
    args.devd_power_diag_url = observe_urls["devd_power_diag_url"]
    args.devd_monitor_start_url = observe_urls["devd_monitor_start_url"]
    args.devd_device_trace_url = observe_urls["devd_device_trace_url"]
    if args.scene_type == "backup_only" and args.include_backup is not True:
        parser.error("--scene-type=backup_only requires --include-backup")


def load_transport_configured(args: argparse.Namespace) -> bool:
    return bool(
        (getattr(args, "load_ipc", "") or "").strip()
        or
        (getattr(args, "load_devd_socket", "") or "").strip()
        or (getattr(args, "load_devd_base_url", "") or "").strip()
    )


def load_devd_transport_configured(args: argparse.Namespace) -> bool:
    return bool(
        (getattr(args, "load_devd_socket", "") or "").strip()
        or (getattr(args, "load_devd_base_url", "") or "").strip()
    )


def effective_load_bridge_url(args: argparse.Namespace) -> str:
    bridge_url = (getattr(args, "load_bridge_url", "") or "").strip()
    if not bridge_url:
        return ""
    if load_transport_configured(args):
        return ""
    return bridge_url


def normalize_load_transport_args(args: argparse.Namespace) -> None:
    bridge_url = (getattr(args, "load_bridge_url", "") or "").strip()
    if bridge_url == DEFAULT_LOAD_BRIDGE_URL and load_transport_configured(args):
        args.load_bridge_url = ""
    if (getattr(args, "load_ipc", "") or "").strip():
        args.load_devd_socket = ""
        if (getattr(args, "load_devd_base_url", "") or "").strip() == DEFAULT_LOAD_DEVD_BASE_URL:
            args.load_devd_base_url = ""


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
    # Serialize all loadlynx commands inside one process so scene logic does not
    # race control/status calls and leave the hardware in a dirty state.
    with LOADLYNX_COMMAND_LOCK:
        return run(cmd, timeout_sec=timeout_sec)


def force_loadlynx_ipc_cmd(args: argparse.Namespace, *parts: str) -> list[str]:
    cmd = [args.load_cli]
    ipc_endpoint = resolve_load_devd_socket(args)
    if not ipc_endpoint:
        raise RuntimeError("load_devd_socket is empty")
    cmd.extend(["--ipc", ipc_endpoint])
    cmd.extend(parts)
    return cmd


def mains_aegis_cmd(args: argparse.Namespace, *parts: str) -> list[str]:
    cmd = [args.mains_aegis_cli]
    ipc_endpoint = (args.mains_aegis_ipc or "").strip()
    if ipc_endpoint:
        cmd.extend(["--ipc", ipc_endpoint])
    cmd.extend(parts)
    return cmd


def loadlynx_cmd(args: argparse.Namespace, *parts: str) -> list[str]:
    cmd = [args.load_cli]
    explicit_url = any(part == "--url" for part in parts)
    ipc_endpoint = (args.load_ipc or "").strip()
    bridge_mode = bool(effective_load_bridge_url(args))
    if ipc_endpoint and not explicit_url and not bridge_mode:
        cmd.extend(["--ipc", ipc_endpoint])
    cmd.extend(parts)
    return cmd


def loadlynx_direct_or_ipc_cmd(args: argparse.Namespace, *parts: str) -> list[str]:
    return loadlynx_cmd(args, *parts)


def http_json(url: str) -> Any:
    with urllib.request.urlopen(url, timeout=10) as response:
        return json.load(response)


def http_post_json(url: str) -> Any:
    request = urllib.request.Request(url, method="POST")
    with urllib.request.urlopen(request, timeout=10) as response:
        return json.load(response)


def http_post_json_with_retries(
    url: str,
    *,
    timeout_sec: float,
    retries: int = DEFAULT_SAMPLE_READ_RETRIES,
    retry_delay_sec: float = DEFAULT_SAMPLE_READ_RETRY_DELAY_SECONDS,
) -> Any:
    last_exc: Exception | None = None
    for attempt in range(max(1, retries)):
        try:
            request = urllib.request.Request(url, method="POST")
            with urllib.request.urlopen(request, timeout=timeout_sec) as response:
                return json.load(response)
        except Exception as exc:  # noqa: BLE001
            last_exc = exc
            if attempt + 1 < max(1, retries):
                time.sleep(retry_delay_sec)
    raise RuntimeError(f"http post failed after retries: url={url!r} error={last_exc!r}")


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
        except urllib.error.HTTPError as exc:
            last_exc = exc
            if exc.code in TRANSIENT_HTTP_STATUS_CODES and attempt + 1 < max(1, retries):
                time.sleep(retry_delay_sec)
                continue
            break
        except Exception as exc:  # noqa: BLE001
            last_exc = exc
            if attempt + 1 < max(1, retries):
                time.sleep(retry_delay_sec)
    raise RuntimeError(f"http fetch failed after retries: url={url!r} error={last_exc!r}")


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


def run_json(cmd: list[str], *, timeout_sec: float = DEFAULT_COMMAND_TIMEOUT_SECONDS) -> Any:
    stdout = run(cmd, timeout_sec=timeout_sec).stdout.strip()
    return json.loads(stdout) if stdout else {}


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


def ensure_valid_load_status_payload(payload: Any, *, source: str) -> dict[str, Any]:
    normalized = normalize_load_status_payload(payload)
    if is_valid_load_status_payload(normalized):
        if isinstance(normalized, dict) and isinstance(normalized.get("source"), str):
            return normalized
        result = dict(normalized) if isinstance(normalized, dict) else {}
        result.setdefault("source", source)
        return result
    raise RuntimeError(f"{source}_invalid_payload: {payload!r}")


def resolve_load_ipc_status_helper(args: argparse.Namespace) -> str:
    helper_path = getattr(args, "load_ipc_status_helper", DEFAULT_LOAD_IPC_STATUS_HELPER)
    if os.path.isabs(helper_path):
        return helper_path
    return str((Path.cwd() / helper_path).resolve())


def resolve_load_devd_socket(args: argparse.Namespace) -> str:
    explicit_socket = (getattr(args, "load_devd_socket", "") or "").strip()
    if explicit_socket:
        return explicit_socket
    return (getattr(args, "load_ipc", "") or "").strip()


def scan_load_devd_devices_via_ipc(
    args: argparse.Namespace,
    *,
    timeout_sec: float,
) -> dict[str, Any]:
    endpoint = resolve_load_devd_socket(args)
    if not endpoint:
        raise RuntimeError("load_devd_socket is empty")
    return ipc_call(
        endpoint,
        "devices.scan",
        {},
        timeout_sec=timeout_sec,
    )


def warm_load_status_via_ipc(
    args: argparse.Namespace,
    *,
    timeout_sec: float,
    load_devd_lease: dict[str, Any],
) -> Any:
    return get_load_status_via_ipc(
        args,
        timeout_sec=timeout_sec,
        load_devd_lease=load_devd_lease,
        retries=1,
        retry_delay_sec=0.0,
    )


def get_load_status_via_ipc_helper(
    args: argparse.Namespace,
    *,
    timeout_sec: float,
    load_devd_lease: dict[str, Any] | None = None,
    retries: int = DEFAULT_SAMPLE_READ_RETRIES,
    retry_delay_sec: float = DEFAULT_SAMPLE_READ_RETRY_DELAY_SECONDS,
    scan_first: bool = False,
    warmup: bool = False,
) -> Any:
    helper_path = resolve_load_ipc_status_helper(args)
    ipc_endpoint = resolve_load_devd_socket(args)
    if not ipc_endpoint:
        raise RuntimeError("load_devd_socket is empty")
    lease_id = dict_or_empty(load_devd_lease).get("lease_id")
    cmd = [
        sys.executable,
        helper_path,
        "--ipc-endpoint",
        ipc_endpoint,
        "--device-id",
        args.load_usb_device_id,
        "--timeout-sec",
        str(timeout_sec),
    ]
    if scan_first:
        cmd.append("--scan-first")
    if isinstance(lease_id, str) and lease_id:
        cmd.extend(["--lease-id", lease_id])
        if warmup:
            cmd.append("--warmup")
    payload = run_json_command_with_retries(
        cmd,
        timeout_sec=timeout_sec,
        retries=retries,
        retry_delay_sec=retry_delay_sec,
    )
    payload = dict_or_empty(payload)
    result = dict_or_empty(payload.get("result"))
    if result:
        return ensure_valid_load_status_payload(result, source="ipc_helper_status")
    raise RuntimeError(f"ipc_helper_status_missing_result: {payload!r}")


def acquire_load_devd_lease_via_ipc(
    args: argparse.Namespace,
    *,
    timeout_sec: float,
) -> dict[str, Any]:
    scan_load_devd_devices_via_ipc(args, timeout_sec=timeout_sec)
    endpoint = resolve_load_devd_socket(args)
    if not endpoint:
        raise RuntimeError("load_devd_socket is empty")
    payload = {"device_id": args.load_usb_device_id}
    expected_identity = getattr(args, "load_device", None)
    if isinstance(expected_identity, str) and expected_identity:
        payload["expected_identity_device_id"] = expected_identity
    response = ipc_call(
        endpoint,
        "serial.lease.create",
        payload,
        timeout_sec=timeout_sec,
    )
    result = dict_or_empty(dict_or_empty(response).get("result"))
    if not result:
        raise RuntimeError(f"load_devd_ipc_lease_create_failed: {response!r}")
    return result


def release_load_devd_lease_via_ipc(
    args: argparse.Namespace,
    lease_id: str,
    *,
    timeout_sec: float,
) -> dict[str, Any]:
    endpoint = resolve_load_devd_socket(args)
    if not endpoint:
        raise RuntimeError("load_devd_socket is empty")
    return ipc_call(
        endpoint,
        "serial.lease.release",
        {"lease_id": lease_id},
        timeout_sec=timeout_sec,
    )


def get_load_status_via_ipc(
    args: argparse.Namespace,
    *,
    timeout_sec: float,
    load_devd_lease: dict[str, Any] | None,
    retries: int = DEFAULT_SAMPLE_READ_RETRIES,
    retry_delay_sec: float = DEFAULT_SAMPLE_READ_RETRY_DELAY_SECONDS,
) -> Any:
    endpoint = resolve_load_devd_socket(args)
    if not endpoint:
        raise RuntimeError("load_devd_socket is empty")
    active_lease = dict_or_empty(load_devd_lease)
    lease_id = active_lease.get("lease_id")
    if not isinstance(lease_id, str) or not lease_id:
        raise RuntimeError("load_devd_lease_missing_lease_id")
    last_exc: Exception | None = None
    for attempt in range(max(1, retries)):
        try:
            payload = ipc_call(
                endpoint,
                "compat.status",
                {
                    "device_id": args.load_usb_device_id,
                    "lease_id": lease_id,
                },
                timeout_sec=timeout_sec,
            )
            result = dict_or_empty(dict_or_empty(payload).get("result"))
            if result:
                return ensure_valid_load_status_payload(result, source="ipc_status")
            raise RuntimeError(f"load_devd_ipc_status_missing_result: {payload!r}")
        except (
            socket.timeout,
            TimeoutError,
            ConnectionError,
            OSError,
            json.JSONDecodeError,
            RuntimeError,
        ) as exc:
            last_exc = exc
            if attempt + 1 < max(1, retries):
                time.sleep(retry_delay_sec)
    raise RuntimeError(f"load_devd_ipc_status_failed: {last_exc!r}")


def get_load_status_via_ipc_without_lease(
    args: argparse.Namespace,
    *,
    timeout_sec: float,
    retries: int = DEFAULT_SAMPLE_READ_RETRIES,
    retry_delay_sec: float = DEFAULT_SAMPLE_READ_RETRY_DELAY_SECONDS,
) -> Any:
    endpoint = resolve_load_devd_socket(args)
    if not endpoint:
        raise RuntimeError("load_devd_socket is empty")
    last_exc: Exception | None = None
    for attempt in range(max(1, retries)):
        try:
            payload = ipc_call(
                endpoint,
                "compat.status",
                {
                    "device_id": args.load_usb_device_id,
                },
                timeout_sec=timeout_sec,
            )
            result = dict_or_empty(dict_or_empty(payload).get("result"))
            if result:
                return ensure_valid_load_status_payload(result, source="ipc_status_no_lease")
            raise RuntimeError(f"load_devd_ipc_status_no_lease_missing_result: {payload!r}")
        except (
            socket.timeout,
            TimeoutError,
            ConnectionError,
            OSError,
            json.JSONDecodeError,
            RuntimeError,
        ) as exc:
            last_exc = exc
            if attempt + 1 < max(1, retries):
                time.sleep(retry_delay_sec)
    raise RuntimeError(f"load_devd_ipc_status_no_lease_failed: {last_exc!r}")


def run_load_telemetry_probe(args: argparse.Namespace) -> dict[str, Any]:
    if getattr(args, "skip_load_telemetry_probe", False):
        return {
            "skipped": True,
            "reason": "skip_load_telemetry_probe",
        }
    probe_path = getattr(args, "load_telemetry_probe", DEFAULT_LOAD_TELEMETRY_PROBE)
    if not os.path.isabs(probe_path):
        probe_path = str((Path.cwd() / probe_path).resolve())
    cmd = [
        sys.executable,
        probe_path,
        "--load-cli",
        args.load_cli,
        "--load-device",
        args.load_device,
        "--load-usb-device-id",
        args.load_usb_device_id,
        "--load-usb-port",
        args.load_usb_port,
        "--load-ipc",
        args.load_ipc,
        "--load-devd-base-url",
        args.load_devd_base_url,
        "--load-devd-socket",
        args.load_devd_socket,
        "--load-bridge-url",
        effective_load_bridge_url(args),
    ]
    if args.load_bridge_device:
        cmd.extend(["--load-bridge-device", args.load_bridge_device])
    started_at = time.monotonic()
    completed = subprocess.run(
        cmd,
        text=True,
        capture_output=True,
        timeout=max(args.command_timeout_sec, 60.0),
    )
    elapsed_s = round(time.monotonic() - started_at, 3)
    stdout = completed.stdout.strip()
    payload = json.loads(stdout) if stdout else {}
    payload["elapsed_s"] = elapsed_s
    payload["exit_code"] = completed.returncode
    payload["stderr"] = completed.stderr.strip() or None
    return payload


def probe_live_load_status_poller_capability(
    args: argparse.Namespace,
    *,
    runtime_sec: float = 1.5,
) -> dict[str, Any]:
    poller = LoadStatusPoller(
        args,
        args.load_device,
        timeout_sec=min(args.status_timeout_sec, 2.0),
        poll_interval_sec=getattr(args, "sample_interval_seconds", DEFAULT_SAMPLE_INTERVAL_SECONDS),
        stream_interval_sec=getattr(
            args,
            "load_stream_interval_seconds",
            DEFAULT_LOAD_STREAM_INTERVAL_SECONDS,
        ),
        use_status_stream=getattr(args, "load_status_source", "status-stream") == "status-stream",
    )
    snapshots: list[dict[str, Any]] = []
    started_at = time.monotonic()
    try:
        poller.start()
        effective_mode = poller.effective_status_source_mode()
        deadline = started_at + max(0.75, runtime_sec)
        next_sample_at = started_at
        while time.monotonic() < deadline:
            now = time.monotonic()
            if now < next_sample_at:
                time.sleep(min(0.05, next_sample_at - now))
                continue
            snapshot = poller.snapshot(now)
            snapshots.append(
                {
                    "t_s": round(now - started_at, 3),
                    "generation": snapshot.get("generation"),
                    "age_s": snapshot.get("age_s"),
                    "sample_age_s": snapshot.get("sample_age_s"),
                    "error": snapshot.get("error"),
                    "source": snapshot.get("source"),
                    "has_payload": isinstance(snapshot.get("payload"), dict),
                }
            )
            next_sample_at += 0.25
        valid = [
            sample
            for sample in snapshots
            if sample.get("has_payload")
            and sample.get("error") is None
            and isinstance(sample.get("sample_age_s"), (int, float))
            and float(sample["sample_age_s"]) <= FORMAL_MAX_REALTIME_SAMPLE_AGE_SECONDS
        ]
        started_values = [
            float(sample["t_s"])
            for sample in valid
            if isinstance(sample.get("t_s"), (int, float))
        ]
        gaps = [
            round(curr - prev, 3)
            for prev, curr in zip(started_values, started_values[1:])
            if curr > prev
        ]
        max_gap_s = max(gaps, default=None)
        effective_sample_rate_hz = (
            round(1.0 / max_gap_s, 3)
            if isinstance(max_gap_s, (int, float)) and max_gap_s > 0
            else None
        )
        failures: list[str] = []
        if not valid:
            failures.append("live_poller_no_fresh_samples")
        if effective_sample_rate_hz is None or effective_sample_rate_hz < FORMAL_MIN_EFFECTIVE_SAMPLE_RATE_HZ:
            failures.append("live_poller_sample_rate_below_formal_floor")
        if max_gap_s is None or max_gap_s > FORMAL_MAX_SAMPLE_GAP_SECONDS:
            failures.append("live_poller_sample_gap_above_formal_ceiling")
        return {
            "source": "live_load_status_poller",
            "effective_mode": effective_mode,
            "sample_count": len(snapshots),
            "fresh_sample_count": len(valid),
            "samples": snapshots,
            "effective_sample_rate_hz": effective_sample_rate_hz,
            "max_sample_gap_s": max_gap_s,
            "formal_capable": not failures,
            "failures": failures,
        }
    finally:
        poller.stop(timeout_sec=2.0)


def write_json(path: Path, payload: Any) -> None:
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def append_jsonl(path: Path, payload: Any) -> None:
    with path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(payload, ensure_ascii=False) + "\n")


def dict_or_empty(payload: Any) -> dict[str, Any]:
    return payload if isinstance(payload, dict) else {}


def devd_read_sample(payload: Any) -> Any:
    data = dict_or_empty(payload)
    if "sample" in data and isinstance(data.get("meta"), dict):
        return data.get("sample")
    return payload


def devd_read_meta(payload: Any) -> dict[str, Any]:
    data = dict_or_empty(payload)
    return dict_or_empty(data.get("meta"))


def devd_snapshot_sample_age_s(
    *,
    payload: Any,
    fetch_age_s: Any,
) -> float | None:
    meta = devd_read_meta(payload)
    cache_age_ms = meta.get("cache_age_ms")
    if isinstance(cache_age_ms, (int, float)):
        return round(max(0.0, float(cache_age_ms) / 1000.0), 3)
    if isinstance(fetch_age_s, (int, float)):
        return round(max(0.0, float(fetch_age_s)), 3)
    return None


def devd_snapshot_is_fresh(
    *,
    payload: Any,
    fetch_age_s: Any,
) -> bool:
    meta = devd_read_meta(payload)
    if meta:
        return meta.get("cache_fresh") is True
    age_s = devd_snapshot_sample_age_s(payload=payload, fetch_age_s=fetch_age_s)
    return isinstance(age_s, (int, float)) and age_s <= FORMAL_MAX_REALTIME_SAMPLE_AGE_SECONDS


def devd_device_id_from_endpoint(url: str) -> str | None:
    path_parts = [part for part in urllib.parse.urlparse(url).path.split("/") if part]
    try:
        devices_idx = path_parts.index("devices")
    except ValueError:
        return None
    if devices_idx + 1 >= len(path_parts):
        return None
    device_id = path_parts[devices_idx + 1]
    return urllib.parse.unquote(device_id) if device_id else None


def devd_devices_url_from_endpoint(url: str) -> str | None:
    parsed = urllib.parse.urlparse(url)
    path_parts = [part for part in parsed.path.split("/") if part]
    try:
        api_idx = path_parts.index("api")
        devices_idx = path_parts.index("devices")
    except ValueError:
        return None
    if devices_idx < api_idx:
        return None
    base_path = "/" + "/".join(path_parts[api_idx : devices_idx + 1])
    return urllib.parse.urlunparse((parsed.scheme, parsed.netloc, base_path, "", "", ""))


def devd_device_entry_from_listing(listing_payload: Any, *, device_id: str | None) -> dict[str, Any]:
    if not isinstance(device_id, str) or not device_id:
        return {}
    devices = dict_or_empty(listing_payload).get("devices")
    if not isinstance(devices, list):
        return {}
    for device in devices:
        payload = dict_or_empty(device)
        identity = dict_or_empty(payload.get("identity"))
        if payload.get("id") == device_id or identity.get("device_id") == device_id:
            return payload
    return {}


def devd_device_entry_from_scan(scan_payload: Any, *, device_id: str | None) -> dict[str, Any]:
    return devd_device_entry_from_listing(scan_payload, device_id=device_id)


def seeded_devd_device_is_capability_ready(device_payload: Any) -> bool:
    payload = dict_or_empty(device_payload)
    if payload.get("connection") != "connected":
        return False
    identity = payload.get("identity")
    settings = payload.get("settings")
    return (
        isinstance(identity, dict)
        and isinstance(settings, dict)
    )


def devd_devices_listing_url_from_endpoint(url: str) -> str | None:
    return devd_devices_url_from_endpoint(url)


def normalize_devd_device_endpoint(url: str, *, device_id: str | None) -> str:
    if not isinstance(url, str) or not url.strip():
        return url
    normalized_device_id = (
        str(device_id).strip()
        if isinstance(device_id, str) and device_id.strip()
        else None
    )
    if normalized_device_id is None:
        return url
    parsed = urllib.parse.urlparse(url)
    path_parts = [part for part in parsed.path.split("/") if part]
    try:
        devices_idx = path_parts.index("devices")
    except ValueError:
        return url
    target_idx = devices_idx + 1
    if target_idx >= len(path_parts):
        return url
    path_parts[target_idx] = urllib.parse.quote(normalized_device_id, safe="")
    new_path = "/" + "/".join(path_parts)
    return urllib.parse.urlunparse(
        (
            parsed.scheme,
            parsed.netloc,
            new_path,
            parsed.params,
            parsed.query,
            parsed.fragment,
        )
    )


def rewrite_devd_endpoint_base(url: str, *, base_url: str | None) -> str:
    if not isinstance(url, str) or not url.strip():
        return url
    if not isinstance(base_url, str) or not base_url.strip():
        return url
    parsed = urllib.parse.urlparse(url)
    base_parsed = urllib.parse.urlparse(base_url)
    if not parsed.scheme or not parsed.netloc or not base_parsed.scheme or not base_parsed.netloc:
        return url
    return urllib.parse.urlunparse(
        (
            base_parsed.scheme,
            base_parsed.netloc,
            parsed.path,
            parsed.params,
            parsed.query,
            parsed.fragment,
        )
    )


def devd_base_url_from_scan_url(scan_url: str) -> str | None:
    if not isinstance(scan_url, str) or not scan_url.strip():
        return None
    parsed = urllib.parse.urlparse(scan_url)
    if not parsed.scheme or not parsed.netloc:
        return None
    return urllib.parse.urlunparse((parsed.scheme, parsed.netloc, "", "", "", ""))


def normalized_observe_urls(args: argparse.Namespace) -> dict[str, str]:
    normalized_device_id = (
        str(args.ups_device_id).strip()
        if isinstance(args.ups_device_id, str) and args.ups_device_id.strip()
        else None
    )
    devd_base_url = devd_base_url_from_scan_url(getattr(args, "devd_scan_url", ""))
    return {
        "ups_status_url": normalize_devd_device_endpoint(
            rewrite_devd_endpoint_base(args.ups_status_url, base_url=devd_base_url),
            device_id=normalized_device_id,
        ),
        "ups_settings_url": normalize_devd_device_endpoint(
            rewrite_devd_endpoint_base(args.ups_settings_url, base_url=devd_base_url),
            device_id=normalized_device_id,
        ),
        "devd_power_diag_url": normalize_devd_device_endpoint(
            rewrite_devd_endpoint_base(args.devd_power_diag_url, base_url=devd_base_url),
            device_id=normalized_device_id,
        ),
        "devd_monitor_start_url": normalize_devd_device_endpoint(
            rewrite_devd_endpoint_base(args.devd_monitor_start_url, base_url=devd_base_url),
            device_id=normalized_device_id,
        ),
        "devd_device_trace_url": normalize_devd_device_endpoint(
            rewrite_devd_endpoint_base(args.devd_device_trace_url, base_url=devd_base_url),
            device_id=normalized_device_id,
        ),
    }


def validate_mains_aegis_devd_bootstrap(bootstrap_payload: Any) -> dict[str, Any]:
    payload = dict_or_empty(bootstrap_payload)
    app = dict_or_empty(payload.get("app"))
    failures: list[str] = []
    if app.get("name") != "mains-aegis-devd":
        failures.append("bootstrap_app_name_mismatch")
    if app.get("mode") not in {"http_service", "http_service_api_only"}:
        failures.append("bootstrap_app_mode_invalid")
    return {
        "ok": not failures,
        "failures": failures,
        "app": app,
    }


def ensure_valid_mains_aegis_devd_http_base(
    scan_url: str,
    *,
    timeout_sec: float,
) -> dict[str, Any]:
    base_url = devd_base_url_from_scan_url(scan_url)
    if not isinstance(base_url, str) or not base_url:
        return {
            "ok": False,
            "failures": ["devd_base_url_unresolved"],
            "scan_url": scan_url,
        }
    bootstrap_url = f"{base_url.rstrip('/')}/api/v1/bootstrap"
    try:
        payload = http_json_with_retries(bootstrap_url, timeout_sec=timeout_sec)
    except Exception as exc:  # noqa: BLE001
        return {
            "ok": False,
            "failures": ["devd_bootstrap_unreachable"],
            "base_url": base_url,
            "bootstrap_url": bootstrap_url,
            "error": repr(exc),
        }
    validation = validate_mains_aegis_devd_bootstrap(payload)
    return {
        "ok": validation["ok"],
        "failures": list(validation["failures"]),
        "base_url": base_url,
        "bootstrap_url": bootstrap_url,
        "bootstrap": payload,
        "validation": validation,
    }


def ups_identity_url_from_status_url(status_url: str) -> str:
    parsed = urllib.parse.urlparse(status_url)
    path = parsed.path
    if path.endswith("/status"):
        path = f"{path[:-len('/status')]}/identity"
    else:
        raise RuntimeError(f"cannot derive identity url from status url: {status_url}")
    return urllib.parse.urlunparse(
        (parsed.scheme, parsed.netloc, path, parsed.params, parsed.query, parsed.fragment)
    )


def mains_aegis_read_identity(args: argparse.Namespace) -> dict[str, Any]:
    cmd = mains_aegis_cmd(
        args,
        "device",
        args.ups_device_id,
        "identity",
    )
    return run_json(cmd)


def mains_aegis_connect_device(args: argparse.Namespace) -> dict[str, Any]:
    cmd = mains_aegis_cmd(
        args,
        "device",
        args.ups_device_id,
        "connect",
    )
    return run_json(cmd)


def mains_aegis_read_connection(args: argparse.Namespace) -> dict[str, Any]:
    cmd = mains_aegis_cmd(
        args,
        "device",
        args.ups_device_id,
        "connection",
    )
    return run_json(cmd)


def mains_aegis_read_settings(args: argparse.Namespace) -> dict[str, Any]:
    cmd = mains_aegis_cmd(
        args,
        "device",
        args.ups_device_id,
        "settings",
    )
    return run_json(cmd)


def expected_profile_rated_vout_mv(output_profile: str) -> int:
    if output_profile == "12v":
        return 12_000
    if output_profile == "19v":
        return 19_000
    raise RuntimeError(f"unsupported output_profile: {output_profile}")


def extract_identity_hardware_capabilities(identity_payload: Any) -> dict[str, Any]:
    return dict_or_empty(dict_or_empty(identity_payload).get("hardware_capabilities"))


def extract_settings_hardware_capabilities(settings_payload: Any) -> dict[str, Any]:
    advanced_power_capabilities = dict_or_empty(
        dict_or_empty(settings_payload).get("advanced_power_capabilities")
    )
    rated_vout_mv = advanced_power_capabilities.get("rated_vout_mv")
    hardware_capabilities: dict[str, Any] = {}
    if isinstance(rated_vout_mv, int):
        hardware_capabilities["rated_vout_mv"] = rated_vout_mv
        hardware_capabilities["output_profile"] = (
            "19v" if rated_vout_mv == 19_000 else "12v" if rated_vout_mv == 12_000 else "unknown"
        )
    return hardware_capabilities


def validate_ups_hardware_capabilities(
    *,
    expected_output_profile: str,
    expected_source_voltage_mv: int,
    identity_payload: Any,
    settings_payload: Any,
) -> dict[str, Any]:
    expected_rated_vout_mv = expected_profile_rated_vout_mv(expected_output_profile)
    failures: list[str] = []
    identity_caps = extract_identity_hardware_capabilities(identity_payload)
    settings_caps = extract_settings_hardware_capabilities(settings_payload)
    identity_profile = identity_caps.get("output_profile")
    identity_rated_vout_mv = identity_caps.get("rated_vout_mv")
    settings_profile = settings_caps.get("output_profile")
    settings_rated_vout_mv = settings_caps.get("rated_vout_mv")

    if expected_source_voltage_mv != expected_rated_vout_mv:
        failures.append("source_voltage_profile_mismatch")
    if identity_profile != expected_output_profile:
        failures.append("identity_output_profile_mismatch")
    if identity_rated_vout_mv != expected_rated_vout_mv:
        failures.append("identity_rated_vout_mismatch")
    if settings_profile != expected_output_profile:
        failures.append("settings_output_profile_mismatch")
    if settings_rated_vout_mv != expected_rated_vout_mv:
        failures.append("settings_rated_vout_mismatch")

    return {
        "ok": not failures,
        "failures": failures,
        "expected": {
            "output_profile": expected_output_profile,
            "rated_vout_mv": expected_rated_vout_mv,
            "source_voltage_mv": expected_source_voltage_mv,
        },
        "identity_hardware_capabilities": identity_caps,
        "settings_hardware_capabilities": settings_caps,
    }


def validate_dual_surface_hardware_capabilities(
    *,
    expected_output_profile: str,
    expected_source_voltage_mv: int,
    usb_identity_payload: Any,
    usb_settings_payload: Any,
    http_identity_payload: Any,
    http_settings_payload: Any,
) -> dict[str, Any]:
    usb_gate = validate_ups_hardware_capabilities(
        expected_output_profile=expected_output_profile,
        expected_source_voltage_mv=expected_source_voltage_mv,
        identity_payload=usb_identity_payload,
        settings_payload=usb_settings_payload,
    )
    http_gate = validate_ups_hardware_capabilities(
        expected_output_profile=expected_output_profile,
        expected_source_voltage_mv=expected_source_voltage_mv,
        identity_payload=http_identity_payload,
        settings_payload=http_settings_payload,
    )
    failures = list(usb_gate["failures"]) + [f"http:{item}" for item in http_gate["failures"]]
    if usb_gate["identity_hardware_capabilities"] != http_gate["identity_hardware_capabilities"]:
        failures.append("usb_http_identity_caps_mismatch")
    if usb_gate["settings_hardware_capabilities"] != http_gate["settings_hardware_capabilities"]:
        failures.append("usb_http_settings_caps_mismatch")
    return {
        "ok": not failures,
        "failures": failures,
        "expected": usb_gate["expected"],
        "usb": usb_gate,
        "http": http_gate,
    }


def validate_ups_external_input_cut(status_payload: Any) -> dict[str, Any]:
    status = dict_or_empty(status_payload)
    input_root = dict_or_empty(status.get("input"))
    vin_vbus_mv = input_root.get("vin_vbus_mv")
    mains_present = input_root.get("mains_present")
    assist_power_stage = input_root.get("assist_power_stage")
    mode = status.get("mode")
    failures: list[str] = []
    if not isinstance(vin_vbus_mv, int):
        failures.append("ups_vin_vbus_missing")
    elif vin_vbus_mv > UPS_INPUT_CUT_MAX_VIN_MV:
        failures.append("ups_vin_vbus_not_cut")
    if mains_present is not False:
        failures.append("ups_mains_present_not_false")
    if mode != "backup" and assist_power_stage != "backup":
        failures.append("ups_backup_semantics_not_observed")
    return {
        "ok": not failures,
        "failures": failures,
        "status": {
            "mode": mode,
            "mains_present": mains_present,
            "vin_vbus_mv": vin_vbus_mv,
            "assist_power_stage": assist_power_stage,
            "input_source": input_root.get("source"),
            "input_vbus_mv": input_root.get("input_vbus_mv"),
        },
        "expected": {
            "vin_vbus_mv_max": UPS_INPUT_CUT_MAX_VIN_MV,
            "mains_present": False,
            "backup_required": True,
        },
    }


def wait_for_ups_external_input_cut(
    status_url: str,
    *,
    timeout_sec: float,
) -> dict[str, Any]:
    deadline = time.monotonic() + max(0.1, timeout_sec)
    last_status: Any = None
    last_gate: dict[str, Any] | None = None
    while time.monotonic() < deadline:
        last_status = http_json_with_retries(
            status_url,
            timeout_sec=min(timeout_sec, 5.0),
        )
        last_gate = validate_ups_external_input_cut(last_status)
        if last_gate.get("ok") is True:
            return {
                "ok": True,
                "url": status_url,
                "validation": last_gate,
            }
        time.sleep(0.2)
    return {
        "ok": False,
        "url": status_url,
        "validation": last_gate
        or {
            "ok": False,
            "failures": ["ups_status_unavailable"],
            "status": last_status if isinstance(last_status, dict) else {},
        },
    }


def looks_like_ups_status_payload(payload: Any) -> bool:
    data = dict_or_empty(payload)
    input_root = data.get("input")
    return isinstance(input_root, dict) and (
        "mode" in data or "output" in data or "battery" in data
    )


def looks_like_power_diag_payload(payload: Any) -> bool:
    data = dict_or_empty(payload)
    input_root = data.get("input")
    return isinstance(input_root, dict) and "devices" not in data and "mode" not in data


def derive_power_diag_from_status(
    status_payload: Any,
    *,
    source: str = "status_derived",
) -> dict[str, Any]:
    status = dict_or_empty(status_payload)
    input_root = dict_or_empty(status.get("input"))
    if not isinstance(status.get("mode"), str):
        return {}
    if not isinstance(input_root.get("mains_present"), bool):
        return {}
    charger = dict_or_empty(status.get("charger"))
    battery = dict_or_empty(status.get("battery"))
    derived = {
        "input": {
            "source": input_root.get("source"),
            "mains_present": input_root.get("mains_present"),
            "input_vbus_mv": input_root.get("input_vbus_mv"),
            "input_ibus_ma": input_root.get("input_ibus_ma"),
            "vin_vbus_mv": input_root.get("vin_vbus_mv"),
            "vin_iin_ma": input_root.get("vin_iin_ma"),
            "vin_baseline_mv": input_root.get("vin_baseline_mv"),
            "vin_drop_mv": input_root.get("vin_drop_mv"),
            "assist_power_stage": input_root.get("assist_power_stage"),
            "assist_target_vout_mv": input_root.get("assist_target_vout_mv"),
            "tps_total_iout_ma": input_root.get("tps_total_iout_ma"),
            "tps_limit_threshold_ma": input_root.get("tps_limit_threshold_ma"),
            "pressure_state": input_root.get("pressure_state"),
            "pressure_score_pct": input_root.get("pressure_score_pct"),
            "pressure_reason": input_root.get("pressure_reason"),
        },
        "charger": {
            "allow_charge": charger.get("allow_charge"),
            "detail_status": charger.get("detail_status"),
            "input_present": charger.get("input_present"),
            "vbus_present": charger.get("vbus_present"),
            "vbus_stat": charger.get("vbus_stat"),
            "vbus_adc_mv": charger.get("vbus_adc_mv"),
            "ibus_adc_ma": charger.get("ibus_adc_ma"),
            "vac1_adc_mv": charger.get("vac1_adc_mv"),
            "vbat_adc_mv": charger.get("vbat_adc_mv"),
        },
        "bms": {
            "state": battery.get("state"),
            "pack_mv": battery.get("pack_mv"),
            "current_ma": battery.get("current_ma"),
            "soc_pct": battery.get("soc_pct"),
        },
        "source": source,
    }
    return derived if looks_like_power_diag_payload(derived) else {}


def trace_power_diag_with_status_fallback(trace_payload: Any) -> dict[str, Any]:
    trace_snapshot = {"payload": trace_payload}
    power_diag = power_diag_from_trace_snapshot(trace_snapshot)
    if looks_like_power_diag_payload(power_diag):
        return power_diag
    return derive_power_diag_from_status(
        status_from_trace_snapshot(trace_snapshot),
        source="trace_status_derived",
    )


def fetch_power_diag_with_trace_fallback(
    args: argparse.Namespace,
    *,
    timeout_sec: float,
    seeded_power_diag: Any | None = None,
) -> tuple[Any, str | None, str | None]:
    power_diag_error: str | None = None
    try:
        return (
            http_json_with_retries(
                args.devd_power_diag_url,
                timeout_sec=timeout_sec,
            ),
            "direct_http",
            None,
        )
    except Exception as exc:  # noqa: BLE001
        power_diag_error = repr(exc)
    trace_url = (getattr(args, "devd_device_trace_url", "") or "").strip()
    if trace_url:
        try:
            trace_payload = http_json_with_retries(
                trace_url,
                timeout_sec=timeout_sec,
            )
            derived_power_diag = trace_power_diag_with_status_fallback(trace_payload)
            if looks_like_power_diag_payload(derived_power_diag):
                return derived_power_diag, "devd_trace", None
        except Exception as exc:  # noqa: BLE001
            power_diag_error = f"{power_diag_error}; trace={exc!r}" if power_diag_error else repr(exc)
    if isinstance(seeded_power_diag, dict):
        return seeded_power_diag, "seeded_refresh_devd_devices", power_diag_error
    return None, None, power_diag_error


def lan_address_from_devd_listing_snapshot(
    snapshot: dict[str, Any],
    *,
    device_id: str | None,
) -> str | None:
    payload = dict_or_empty(snapshot.get("payload"))
    entry = devd_device_entry_from_listing(payload, device_id=device_id)
    value = entry.get("lan_address")
    return value if isinstance(value, str) and value else None


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


def http_post_empty_best_effort(url: str, *, timeout_sec: float) -> Any:
    try:
        return http_post_empty(url, timeout_sec=timeout_sec)
    except Exception as exc:  # noqa: BLE001
        return {"ok": False, "error": repr(exc), "url": url}


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
        if dict_or_empty(dict_or_empty(device.get("identity")).get("stable_identity")).get(
            "device_id"
        )
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
    _ = load_device
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


def acquire_load_devd_lease(
    args: argparse.Namespace,
    *,
    timeout_sec: float,
) -> dict[str, Any]:
    load_devd_socket = resolve_load_devd_socket(args)
    if load_devd_socket:
        return acquire_load_devd_lease_via_ipc(args, timeout_sec=timeout_sec)
    load_devd_base_url = (args.load_devd_base_url or "").strip()
    if not load_devd_base_url:
        raise RuntimeError("load_devd transport is unavailable")
    payload = http_post_json_body(
        f"{load_devd_base_url.rstrip('/')}/api/v1/serial/lease",
        {"device_id": args.load_usb_device_id},
        timeout_sec=timeout_sec,
    )
    return dict_or_empty(payload)


def release_load_devd_lease(
    args: argparse.Namespace,
    lease_id: str,
    *,
    timeout_sec: float,
) -> dict[str, Any]:
    load_devd_socket = resolve_load_devd_socket(args)
    if load_devd_socket:
        return release_load_devd_lease_via_ipc(args, lease_id, timeout_sec=timeout_sec)
    load_devd_base_url = (args.load_devd_base_url or "").strip()
    if not load_devd_base_url:
        raise RuntimeError("load_devd transport is unavailable")
    request = urllib.request.Request(
        f"{load_devd_base_url.rstrip('/')}/api/v1/serial/lease/{urllib.parse.quote(lease_id, safe='')}",
        method="DELETE",
    )
    with urllib.request.urlopen(request, timeout=timeout_sec) as response:
        return json.load(response)


def release_load_devd_lease_quietly(
    args: argparse.Namespace,
    lease: dict[str, Any] | None,
    *,
    timeout_sec: float,
) -> None:
    lease_id = dict_or_empty(lease).get("lease_id")
    if not isinstance(lease_id, str) or not lease_id:
        return
    try:
        release_load_devd_lease(args, lease_id, timeout_sec=timeout_sec)
    except Exception:  # noqa: BLE001
        return


def get_load_status_via_devd_http(
    args: argparse.Namespace,
    *,
    timeout_sec: float,
    load_devd_lease: dict[str, Any] | None,
    retries: int = DEFAULT_SAMPLE_READ_RETRIES,
    retry_delay_sec: float = DEFAULT_SAMPLE_READ_RETRY_DELAY_SECONDS,
) -> Any:
    active_lease = load_devd_lease
    owned_lease = False
    if active_lease is None:
        active_lease = acquire_load_devd_lease(args, timeout_sec=timeout_sec)
        owned_lease = True
    lease_id = dict_or_empty(active_lease).get("lease_id")
    if not isinstance(lease_id, str) or not lease_id:
        raise RuntimeError("load_devd_lease_missing_lease_id")
    try:
        payload = http_json_with_retries(
            (
                f"{args.load_devd_base_url.rstrip('/')}/api/v1/status"
                f"?device_id={urllib.parse.quote(args.load_usb_device_id, safe='')}"
                f"&lease_id={urllib.parse.quote(lease_id, safe='')}"
            ),
            timeout_sec=timeout_sec,
            retries=retries,
            retry_delay_sec=retry_delay_sec,
        )
        if isinstance(payload, dict):
            payload.setdefault("source", "devd_http_status")
        return payload
    finally:
        if owned_lease:
            release_load_devd_lease_quietly(
                args,
                active_lease,
                timeout_sec=min(timeout_sec, 5.0),
            )


def get_load_status(
    args: argparse.Namespace,
    load_device: str,
    *,
    timeout_sec: float,
    bridge_lease: dict[str, Any] | None = None,
    load_devd_lease: dict[str, Any] | None = None,
    retries: int = DEFAULT_SAMPLE_READ_RETRIES,
    retry_delay_sec: float = DEFAULT_SAMPLE_READ_RETRY_DELAY_SECONDS,
    prefer_bridge: bool = True,
    prefer_devd_http: bool = True,
) -> Any:
    if prefer_bridge and effective_load_bridge_url(args):
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
    load_devd_socket = resolve_load_devd_socket(args)
    load_devd_base_url = (args.load_devd_base_url or "").strip()
    if load_devd_socket and load_devd_lease is not None:
        if load_devd_socket:
            try:
                return get_load_status_via_ipc(
                    args,
                    timeout_sec=timeout_sec,
                    load_devd_lease=load_devd_lease,
                    retries=retries,
                    retry_delay_sec=retry_delay_sec,
                )
            except Exception:
                pass
        try:
            return get_load_status_via_ipc_helper(
                args,
                timeout_sec=timeout_sec,
                load_devd_lease=load_devd_lease,
                retries=retries,
                retry_delay_sec=retry_delay_sec,
                scan_first=False,
                warmup=False,
            )
        except Exception:
            pass
    if load_devd_socket and load_devd_lease is None:
        try:
            return get_load_status_via_ipc_without_lease(
                args,
                timeout_sec=timeout_sec,
                retries=retries,
                retry_delay_sec=retry_delay_sec,
            )
        except Exception:
            try:
                return get_load_status_via_ipc_helper(
                    args,
                    timeout_sec=timeout_sec,
                    load_devd_lease=None,
                    retries=retries,
                    retry_delay_sec=retry_delay_sec,
                    scan_first=True,
                    warmup=True,
                )
            except Exception:
                pass
    if prefer_devd_http and load_devd_base_url:
        try:
            return get_load_status_via_devd_http(
                args,
                timeout_sec=timeout_sec,
                load_devd_lease=load_devd_lease,
                retries=retries,
                retry_delay_sec=retry_delay_sec,
            )
        except Exception:
            if load_devd_socket:
                try:
                    return get_load_status_via_ipc_helper(
                        args,
                        timeout_sec=timeout_sec,
                        load_devd_lease=load_devd_lease,
                        retries=retries,
                        retry_delay_sec=retry_delay_sec,
                        scan_first=False,
                        warmup=False,
                    )
                except Exception:
                    pass
    if load_devd_socket:
        raise RuntimeError("load_status_devd_transport_exhausted_without_cli_fallback")
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
    load_devd_lease: dict[str, Any] | None = None,
    retries: int = DEFAULT_SAMPLE_READ_RETRIES,
    retry_delay_sec: float = DEFAULT_SAMPLE_READ_RETRY_DELAY_SECONDS,
    prefer_bridge: bool = True,
    prefer_devd_http: bool = True,
) -> Any:
    try:
        return get_load_status(
            args,
            load_device,
            timeout_sec=timeout_sec,
            bridge_lease=bridge_lease,
            load_devd_lease=load_devd_lease,
            retries=retries,
            retry_delay_sec=retry_delay_sec,
            prefer_bridge=prefer_bridge,
            prefer_devd_http=prefer_devd_http,
        )
    except (
        subprocess.TimeoutExpired,
        subprocess.CalledProcessError,
        json.JSONDecodeError,
        RuntimeError,
        OSError,
        TimeoutError,
    ) as exc:
        return {"ok": False, "error": repr(exc)}


def get_load_status_direct_cli(
    args: argparse.Namespace,
    load_device: str,
    *,
    timeout_sec: float,
) -> Any:
    cmd = (
        force_loadlynx_ipc_cmd(args, "status", "--device", load_device, "--json")
        if resolve_load_devd_socket(args)
        else loadlynx_cmd(args, "status", "--device", load_device, "--json")
    )
    completed = run_loadlynx(
        cmd,
        timeout_sec=timeout_sec,
    )
    payload = json.loads(completed.stdout)
    return ensure_valid_load_status_payload(payload, source="cli_status_direct")


def get_load_status_direct_cli_best_effort(
    args: argparse.Namespace,
    load_device: str,
    *,
    timeout_sec: float,
) -> Any:
    try:
        return get_load_status_direct_cli(
            args,
            load_device,
            timeout_sec=timeout_sec,
        )
    except (
        subprocess.TimeoutExpired,
        subprocess.CalledProcessError,
        json.JSONDecodeError,
        RuntimeError,
        OSError,
        TimeoutError,
    ) as exc:
        return {"ok": False, "error": repr(exc)}


def get_load_control(
    args: argparse.Namespace,
    load_device: str,
    *,
    timeout_sec: float,
    bridge_lease: dict[str, Any] | None = None,
    load_devd_lease: dict[str, Any] | None = None,
) -> Any:
    if effective_load_bridge_url(args):
        active_bridge_lease = bridge_lease
        if active_bridge_lease is None:
            active_bridge_lease = acquire_load_bridge_lease(
                args,
                timeout_sec=timeout_sec,
            )
        try:
            completed = run_loadlynx_bridge_command(
                args,
                active_bridge_lease,
                "control",
                "get",
                "--json",
                timeout_sec=timeout_sec,
            )
            return json.loads(completed.stdout)
        finally:
            if bridge_lease is None:
                release_load_bridge_lease_quietly(
                    args,
                    active_bridge_lease,
                    timeout_sec=min(timeout_sec, 5.0),
                )
    completed = run_loadlynx(
        loadlynx_cmd(args, "control", "get", "--device", load_device, "--json"),
        timeout_sec=timeout_sec,
    )
    payload = json.loads(completed.stdout)
    if isinstance(payload, dict):
        payload.setdefault("source", "cli_control")
    return payload


def get_load_control_direct_cli(
    args: argparse.Namespace,
    load_device: str,
    *,
    timeout_sec: float,
) -> Any:
    cmd = (
        force_loadlynx_ipc_cmd(args, "control", "get", "--device", load_device, "--json")
        if resolve_load_devd_socket(args)
        else loadlynx_cmd(args, "control", "get", "--device", load_device, "--json")
    )
    completed = run_loadlynx(
        cmd,
        timeout_sec=timeout_sec,
    )
    payload = json.loads(completed.stdout)
    if isinstance(payload, dict):
        payload.setdefault("source", "cli_control_direct")
    return payload


def get_load_control_via_ipc(
    args: argparse.Namespace,
    *,
    timeout_sec: float,
    load_devd_lease: dict[str, Any] | None,
) -> Any:
    status_payload = get_load_status_via_ipc(
        args,
        timeout_sec=timeout_sec,
        load_devd_lease=load_devd_lease,
        retries=1,
        retry_delay_sec=0.0,
    )
    control = dict_or_empty(dict_or_empty(status_payload).get("control"))
    if control:
        return {
            "control": control,
            "source": "ipc_control_from_status",
        }
    return status_payload


def get_load_control_via_ipc_helper(
    args: argparse.Namespace,
    *,
    timeout_sec: float,
    load_devd_lease: dict[str, Any] | None,
) -> Any:
    status_payload = get_load_status_via_ipc_helper(
        args,
        timeout_sec=timeout_sec,
        load_devd_lease=load_devd_lease,
        retries=1,
        retry_delay_sec=0.0,
        scan_first=load_devd_lease is None,
        warmup=load_devd_lease is None,
    )
    control = dict_or_empty(dict_or_empty(status_payload).get("control"))
    if control:
        return {
            "control": control,
            "source": "ipc_helper_control_from_status",
        }
    return status_payload


def get_load_control_best_effort(
    args: argparse.Namespace,
    load_device: str,
    *,
    timeout_sec: float,
    bridge_lease: dict[str, Any] | None = None,
    load_devd_lease: dict[str, Any] | None = None,
) -> Any:
    try:
        if load_devd_lease is not None and resolve_load_devd_socket(args):
            return get_load_control_via_ipc(
                args,
                timeout_sec=timeout_sec,
                load_devd_lease=load_devd_lease,
            )
        if load_devd_lease is None and resolve_load_devd_socket(args):
            return get_load_control_via_ipc_helper(
                args,
                timeout_sec=timeout_sec,
                load_devd_lease=None,
            )
        return get_load_control(
            args,
            load_device,
            timeout_sec=timeout_sec,
            bridge_lease=bridge_lease,
            load_devd_lease=load_devd_lease,
        )
    except (
        subprocess.TimeoutExpired,
        subprocess.CalledProcessError,
        json.JSONDecodeError,
        RuntimeError,
        OSError,
        TimeoutError,
    ) as exc:
        return {"ok": False, "error": repr(exc)}


def get_load_control_direct_cli_best_effort(
    args: argparse.Namespace,
    load_device: str,
    *,
    timeout_sec: float,
) -> Any:
    try:
        return get_load_control_direct_cli(
            args,
            load_device,
            timeout_sec=timeout_sec,
        )
    except (
        subprocess.TimeoutExpired,
        subprocess.CalledProcessError,
        json.JSONDecodeError,
        RuntimeError,
        OSError,
        TimeoutError,
    ) as exc:
        return {"ok": False, "error": repr(exc)}


def load_output_enabled(payload: Any) -> bool | None:
    if not isinstance(payload, dict):
        return None
    if isinstance(payload.get("output_enabled"), bool):
        return payload.get("output_enabled")
    if payload.get("ok") is False:
        return None
    control = payload.get("control")
    if isinstance(control, dict) and isinstance(control.get("output_enabled"), bool):
        return control.get("output_enabled")
    raw_status = payload.get("status")
    if isinstance(raw_status, dict) and isinstance(raw_status.get("enable"), bool):
        return raw_status.get("enable")
    return None


def load_target_i_ma(payload: Any) -> int | None:
    if not isinstance(payload, dict):
        return None
    if payload.get("ok") is False:
        return None
    if isinstance(payload.get("target_i_ma"), int):
        return payload.get("target_i_ma")
    preset = payload.get("preset")
    if isinstance(preset, dict) and isinstance(preset.get("target_i_ma"), int):
        return preset.get("target_i_ma")
    control = payload.get("control")
    if isinstance(control, dict) and isinstance(control.get("target_i_ma"), int):
        return control.get("target_i_ma")
    return None


def normalize_verified_load_payload(payload: Any) -> Any | None:
    if not isinstance(payload, dict):
        return None
    if payload.get("ok") is False:
        return None
    return payload


def load_command_response_payload(*outputs: str | None) -> Any | None:
    for output in outputs:
        if not isinstance(output, str):
            continue
        text = output.strip()
        if not text:
            continue
        candidates = [text]
        if "\n" in text:
            candidates.extend(
                line.strip()
                for line in reversed(text.splitlines())
                if line.strip()
            )
        for candidate in candidates:
            try:
                payload = json.loads(candidate)
            except json.JSONDecodeError:
                continue
            if isinstance(payload, dict):
                return payload
    return None


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


def bootstrap_load_status_seed(
    args: argparse.Namespace,
    load_device: str,
    *,
    disable_result: Any,
    timeout_sec: float,
) -> tuple[Any | None, dict[str, Any]]:
    disable_payload = dict_or_empty(disable_result)
    candidates = [
        ("verified_status", disable_payload.get("verified_status")),
        ("status", disable_payload.get("status")),
        ("control", disable_payload.get("control")),
    ]
    for source, payload in candidates:
        if is_valid_load_status_payload(payload):
            return payload, {
                "source": source,
                "verified": True,
            }

    direct_status = get_load_status_direct_cli_best_effort(
        args,
        load_device,
        timeout_sec=timeout_sec,
    )
    if is_valid_load_status_payload(direct_status):
        return direct_status, {
            "source": "direct_cli_status",
            "verified": True,
        }

    return None, {
        "source": "none",
        "verified": False,
        "direct_cli_status": direct_status,
    }


def probe_effective_load_state(load_telemetry_probe: Any) -> tuple[bool | None, int | None]:
    probe = dict_or_empty(load_telemetry_probe)
    cli_status_payload = dict_or_empty(
        dict_or_empty(dict_or_empty(probe.get("cli")).get("status")).get("payload")
    )
    cli_status_enabled = load_output_enabled(cli_status_payload)
    cli_status_target_i_ma = load_target_i_ma(cli_status_payload)
    if cli_status_enabled is not None or cli_status_target_i_ma is not None:
        return cli_status_enabled, cli_status_target_i_ma
    http_samples = dict_or_empty(probe.get("http_status")).get("samples")
    if isinstance(http_samples, list):
        for sample in http_samples:
            payload = dict_or_empty(sample)
            if payload.get("ok") is not True:
                continue
            http_enabled = payload.get("output_enabled")
            http_target_i_ma = payload.get("target_i_ma")
            if isinstance(http_enabled, bool) or isinstance(http_target_i_ma, int):
                return (
                    http_enabled if isinstance(http_enabled, bool) else None,
                    http_target_i_ma if isinstance(http_target_i_ma, int) else None,
                )
    return None, None


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
    raw_status = payload.get("status")
    if not isinstance(raw_status, dict):
        return False
    if raw_status.get("ok") is False:
        return False
    return (
        isinstance(raw_status.get("enable"), bool)
        or isinstance(raw_status.get("v_local_mv"), (int, float))
        or isinstance(raw_status.get("i_local_ma"), (int, float))
        or isinstance(raw_status.get("i_remote_ma"), (int, float))
    ) and (
        isinstance(payload.get("control"), dict)
        or isinstance(payload.get("analog_state"), str)
        or isinstance(payload.get("device_id"), str)
    )


def load_status_i_total_ma(payload: Any) -> int | None:
    if not isinstance(payload, dict):
        return None
    raw_status = payload.get("status")
    if not isinstance(raw_status, dict):
        return None
    parts: list[int] = []
    for key in ("i_local_ma", "i_remote_ma"):
        value = raw_status.get(key)
        if isinstance(value, int) and value >= 0:
            parts.append(value)
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


def wait_for_load_state(
    args: argparse.Namespace,
    load_device: str,
    *,
    expected_enabled: bool,
    expected_target_i_ma: int | None,
    status_timeout_sec: float,
    verify_timeout_sec: float,
    poll_interval_sec: float = DEFAULT_LOAD_STATE_VERIFY_POLL_SECONDS,
    status_observer: Callable[[Any], None] | None = None,
    live_status_poller: "LoadStatusPoller | None" = None,
    bridge_lease: dict[str, Any] | None = None,
) -> dict[str, Any]:
    deadline = time.monotonic() + verify_timeout_sec
    last_control: Any = None
    last_status: Any = None
    live_bridge_lease = None
    live_load_devd_lease = None
    if live_status_poller is not None:
        bridge_lease_snapshot = getattr(live_status_poller, "bridge_lease_snapshot", None)
        if callable(bridge_lease_snapshot):
            live_bridge_lease = bridge_lease_snapshot()
        load_devd_lease_snapshot = getattr(live_status_poller, "load_devd_lease_snapshot", None)
        if callable(load_devd_lease_snapshot):
            live_load_devd_lease = load_devd_lease_snapshot()
    while time.monotonic() < deadline:
        if live_status_poller is not None:
            live_snapshot = live_status_poller.snapshot(time.monotonic())
            live_payload = normalize_verified_load_payload(live_snapshot.get("payload"))
            live_age_s = live_snapshot.get("age_s")
            live_sample_age_s = live_snapshot.get("sample_age_s")
            live_error = live_snapshot.get("error")
            live_enabled = load_output_enabled(live_payload)
            live_target_i_ma = load_target_i_ma(live_payload)
            live_freshness_s = (
                live_sample_age_s
                if isinstance(live_sample_age_s, (int, float))
                else live_age_s
            )
            live_snapshot_usable = (
                live_payload is not None
                and live_error is None
                and isinstance(live_freshness_s, (int, float))
                and float(live_freshness_s) <= FORMAL_MAX_REALTIME_SAMPLE_AGE_SECONDS
            )
            live_target_ok = expected_target_i_ma is None or live_target_i_ma == expected_target_i_ma
            if (
                live_snapshot_usable
                and live_enabled is expected_enabled
                and live_target_ok
            ):
                if status_observer is not None:
                    status_observer(live_payload)
                return {
                    "control": None,
                    "status": live_payload,
                    "effective_enabled": live_enabled,
                    "effective_target_i_ma": live_target_i_ma,
                    "verified_from_live_poller": True,
                    "live_status_age_s": round(float(live_age_s), 3)
                    if isinstance(live_age_s, (int, float))
                    else None,
                    "live_status_sample_age_s": round(float(live_freshness_s), 3),
                }
            if live_snapshot_usable:
                if status_observer is not None:
                    status_observer(live_payload)
                time.sleep(max(0.05, poll_interval_sec))
                continue
        last_status = get_load_status_best_effort(
            args,
            load_device,
            timeout_sec=status_timeout_sec,
            bridge_lease=bridge_lease or live_bridge_lease,
            load_devd_lease=live_load_devd_lease,
            prefer_bridge=(bridge_lease or live_bridge_lease) is not None,
            prefer_devd_http=live_load_devd_lease is not None,
        )
        if status_observer is not None:
            status_observer(last_status)
        last_control = None
        enabled, target_i_ma = select_effective_load_state(last_control, last_status)
        if enabled is None or (expected_target_i_ma is not None and target_i_ma is None):
            last_control = get_load_control_best_effort(
                args,
                load_device,
                timeout_sec=status_timeout_sec,
                bridge_lease=bridge_lease or live_bridge_lease,
                load_devd_lease=live_load_devd_lease,
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
        time.sleep(max(0.05, poll_interval_sec))
    raise RuntimeError(
        "LoadLynx status did not reach expected state: "
        f"enabled={expected_enabled} target_i_ma={expected_target_i_ma} "
        f"last_control={last_control} last_status={last_status}"
    )


def confirm_load_state_with_direct_cli(
    args: argparse.Namespace,
    load_device: str,
    *,
    expected_enabled: bool,
    expected_target_i_ma: int | None,
    timeout_sec: float,
    status_observer: Callable[[Any], None] | None = None,
) -> dict[str, Any]:
    status = get_load_status_direct_cli_best_effort(
        args,
        load_device,
        timeout_sec=timeout_sec,
    )
    if status_observer is not None and isinstance(status, dict) and status.get("ok") is not False:
        status_observer(status)
    status_enabled = load_output_enabled(normalize_verified_load_payload(status))
    status_target_i_ma = load_target_i_ma(normalize_verified_load_payload(status))
    status_target_ok = (
        expected_target_i_ma is None
        or status_target_i_ma == expected_target_i_ma
    )
    if status_enabled is expected_enabled and status_target_ok:
        return {
            "control": None,
            "status": status,
            "effective_enabled": status_enabled,
            "effective_target_i_ma": status_target_i_ma,
            "degraded_verification": True,
            "degraded_from_direct_cli_confirmation": True,
        }

    control = get_load_control_direct_cli_best_effort(
        args,
        load_device,
        timeout_sec=timeout_sec,
    )
    control_enabled = load_output_enabled(normalize_verified_load_payload(control))
    control_target_i_ma = load_target_i_ma(normalize_verified_load_payload(control))
    control_target_ok = (
        expected_target_i_ma is None
        or control_target_i_ma == expected_target_i_ma
    )
    if control_enabled is expected_enabled and control_target_ok:
        return {
            "control": control,
            "status": status,
            "effective_enabled": control_enabled,
            "effective_target_i_ma": control_target_i_ma,
            "degraded_verification": True,
            "degraded_from_direct_cli_confirmation": True,
        }

    effective_enabled, effective_target_i_ma = select_effective_load_state(control, status)
    target_ok = expected_target_i_ma is None or effective_target_i_ma == expected_target_i_ma
    if effective_enabled is expected_enabled and target_ok:
        return {
            "control": control,
            "status": status,
            "effective_enabled": effective_enabled,
            "effective_target_i_ma": effective_target_i_ma,
            "degraded_verification": True,
            "degraded_from_direct_cli_confirmation": True,
        }
    raise RuntimeError(
        "direct_cli_load_state_mismatch "
        f"enabled={expected_enabled} target_i_ma={expected_target_i_ma} "
        f"status={status} control={control}"
    )


def command_response_proves_load_enabled(
    payload: Any,
    *,
    expected_target_i_ma: int,
) -> bool:
    response = dict_or_empty(payload)
    enabled = load_output_enabled(response)
    target_i_ma = load_target_i_ma(response)
    return enabled is True and target_i_ma == expected_target_i_ma


def load_status_snapshot_is_real_telemetry(snapshot: dict[str, Any]) -> bool:
    if snapshot.get("source") == "replace":
        return False
    payload = normalize_load_status_payload(snapshot.get("payload") or snapshot.get("status"))
    if dict_or_empty(payload).get("source") == "command_ack_synthetic_status":
        return False
    raw_status = dict_or_empty(dict_or_empty(payload).get("status"))
    return (
        isinstance(raw_status.get("v_local_mv"), (int, float))
        and isinstance(raw_status.get("i_local_ma"), (int, float))
    )


def wait_for_live_load_status(
    load_status_poller: LoadStatusPoller,
    *,
    sample_interval_seconds: float,
    timeout_sec: float,
    require_new_generation: bool = True,
    progress_hook: Callable[[float], None] | None = None,
) -> dict[str, Any]:
    started_at = time.monotonic()
    initial_snapshot = load_status_poller.snapshot(started_at)
    initial_generation = initial_snapshot.get("generation")
    freshness_limit = max(
        sample_interval_seconds * 2.0,
        FORMAL_MAX_REALTIME_SAMPLE_AGE_SECONDS,
    )
    deadline = started_at + max(0.1, timeout_sec)
    last_snapshot = initial_snapshot
    while time.monotonic() < deadline:
        if progress_hook is not None:
            progress_hook(time.monotonic())
        time.sleep(0.05)
        now = time.monotonic()
        last_snapshot = load_status_poller.snapshot(now)
        generation = last_snapshot.get("generation")
        age_s = last_snapshot.get("age_s")
        has_fresh_status = isinstance(age_s, (int, float)) and age_s <= freshness_limit
        has_real_telemetry = load_status_snapshot_is_real_telemetry(last_snapshot)
        if (
            not require_new_generation
            and isinstance(generation, int)
            and generation >= 1
            and has_fresh_status
            and has_real_telemetry
        ):
            return {
                "ready": True,
                "waited_s": round(now - started_at, 3),
                "initial_generation": initial_generation,
                "ready_generation": generation,
                "ready_age_s": round(float(age_s), 3),
                "freshness_limit_s": freshness_limit,
                "source": "fresh_existing_generation",
            }
        if (
            isinstance(initial_generation, int)
            and isinstance(generation, int)
            and generation > initial_generation
            and has_fresh_status
            and has_real_telemetry
        ):
            return {
                "ready": True,
                "waited_s": round(now - started_at, 3),
                "initial_generation": initial_generation,
                "ready_generation": generation,
                "ready_age_s": round(float(age_s), 3),
                "freshness_limit_s": freshness_limit,
                "source": "stream_new_generation",
            }
        if (
            (not isinstance(initial_generation, int) or initial_generation == 0)
            and isinstance(generation, int)
            and generation >= 1
            and has_fresh_status
            and has_real_telemetry
        ):
            return {
                "ready": True,
                "waited_s": round(now - started_at, 3),
                "initial_generation": initial_generation,
                "ready_generation": generation,
                "ready_age_s": round(float(age_s), 3),
                "freshness_limit_s": freshness_limit,
                "source": "stream_first_generation",
            }
    raise RuntimeError(
        "load_status_not_ready "
        f"timeout_sec={timeout_sec} "
        f"initial_snapshot={initial_snapshot!r} "
        f"last_snapshot={last_snapshot!r}"
    )


def wait_for_load_status_generation_advance(
    load_status_poller: LoadStatusPoller,
    *,
    baseline_generation: int,
    sample_interval_seconds: float,
    timeout_sec: float,
    progress_hook: Callable[[float], None] | None = None,
) -> dict[str, Any]:
    started_at = time.monotonic()
    freshness_limit = max(
        sample_interval_seconds * 2.0,
        FORMAL_MAX_REALTIME_SAMPLE_AGE_SECONDS,
    )
    deadline = started_at + max(0.1, timeout_sec)
    last_snapshot = load_status_poller.snapshot(started_at)
    while time.monotonic() < deadline:
        if progress_hook is not None:
            progress_hook(time.monotonic())
        time.sleep(0.05)
        now = time.monotonic()
        last_snapshot = load_status_poller.snapshot(now)
        generation = last_snapshot.get("generation")
        age_s = last_snapshot.get("age_s")
        if (
            isinstance(generation, int)
            and generation > baseline_generation
            and isinstance(age_s, (int, float))
            and age_s <= freshness_limit
        ):
            return {
                "ready": True,
                "waited_s": round(now - started_at, 3),
                "baseline_generation": baseline_generation,
                "ready_generation": generation,
                "ready_age_s": round(float(age_s), 3),
                "freshness_limit_s": freshness_limit,
            }
    raise RuntimeError(
        "load_status_generation_not_advanced "
        f"timeout_sec={timeout_sec} "
        f"baseline_generation={baseline_generation} "
        f"last_snapshot={last_snapshot!r}"
    )


def isolapurr_snapshot_has_expected_state(
    snapshot: dict[str, Any],
    *,
    expected_enabled: bool,
) -> bool:
    payload = dict_or_empty(snapshot.get("payload"))
    port_c = port_state(payload, port_id="port_c")
    telemetry = dict_or_empty(port_c.get("telemetry"))
    state = dict_or_empty(port_c.get("state"))
    if state.get("power_enabled") is not expected_enabled:
        return False
    if expected_enabled:
        return (
            isinstance(telemetry.get("voltage_mv"), (int, float))
            and isinstance(telemetry.get("current_ma"), (int, float))
        )
    return telemetry.get("status") == "not_inserted"


def wait_for_isolapurr_port_c_state(
    isolapurr_poller: "JsonPoller",
    *,
    expected_enabled: bool,
    sample_interval_seconds: float,
    timeout_sec: float,
    progress_hook: Callable[[float], None] | None = None,
) -> dict[str, Any]:
    started_at = time.monotonic()
    deadline = started_at + max(0.1, timeout_sec)
    initial_snapshot = isolapurr_poller.snapshot(started_at)
    initial_generation = initial_snapshot.get("generation")
    last_snapshot = initial_snapshot
    while time.monotonic() < deadline:
        if progress_hook is not None:
            progress_hook(time.monotonic())
        time.sleep(min(0.05, max(0.02, sample_interval_seconds / 2.0)))
        now = time.monotonic()
        last_snapshot = isolapurr_poller.snapshot(now)
        generation = last_snapshot.get("generation")
        age_s = last_snapshot.get("age_s")
        fresh = isinstance(age_s, (int, float)) and age_s <= FORMAL_MAX_REALTIME_SAMPLE_AGE_SECONDS
        if (
            isinstance(generation, int)
            and (not isinstance(initial_generation, int) or generation >= initial_generation)
            and fresh
            and isolapurr_snapshot_has_expected_state(
                last_snapshot,
                expected_enabled=expected_enabled,
            )
        ):
            return {
                "ready": True,
                "waited_s": round(now - started_at, 3),
                "initial_generation": initial_generation,
                "ready_generation": generation,
                "ready_age_s": round(float(age_s), 3),
                "expected_enabled": expected_enabled,
            }
    raise RuntimeError(
        "isolapurr_port_c_not_ready "
        f"timeout_sec={timeout_sec} "
        f"expected_enabled={expected_enabled} "
        f"initial_snapshot={initial_snapshot!r} "
        f"last_snapshot={last_snapshot!r}"
    )


def ups_snapshot_ready(snapshot: dict[str, Any]) -> bool:
    raw_payload = snapshot.get("payload")
    payload = dict_or_empty(devd_read_sample(raw_payload))
    input_payload = dict_or_empty(payload.get("input"))
    return (
        isinstance(snapshot.get("generation"), int)
        and snapshot.get("generation", 0) >= 1
        and devd_snapshot_is_fresh(
            payload=raw_payload,
            fetch_age_s=snapshot.get("age_s"),
        )
        and isinstance(payload.get("mode"), str)
        and isinstance(input_payload.get("mains_present"), bool)
    )


def power_diag_snapshot_ready(snapshot: dict[str, Any]) -> bool:
    raw_payload = snapshot.get("payload")
    payload = dict_or_empty(devd_read_sample(raw_payload))
    input_payload = dict_or_empty(payload.get("input"))
    root_source = payload.get("source")
    is_derived = isinstance(root_source, str) and root_source.endswith("_derived")
    return (
        isinstance(snapshot.get("generation"), int)
        and snapshot.get("generation", 0) >= 1
        and not is_derived
        and devd_snapshot_is_fresh(
            payload=raw_payload,
            fetch_age_s=snapshot.get("age_s"),
        )
        and isinstance(input_payload.get("assist_power_stage"), str)
        and isinstance(input_payload.get("vin_vbus_mv"), (int, float))
        and isinstance(input_payload.get("vin_iin_ma"), (int, float))
    )


def trace_snapshot_ready(snapshot: dict[str, Any]) -> bool:
    payload = dict_or_empty(snapshot.get("payload"))
    status_payload = dict_or_empty(payload.get("status"))
    status_input = dict_or_empty(status_payload.get("input"))
    power_diag_payload = dict_or_empty(payload.get("power_diag"))
    power_diag_input = dict_or_empty(power_diag_payload.get("input"))
    return (
        isinstance(snapshot.get("generation"), int)
        and snapshot.get("generation", 0) >= 1
        and isinstance(snapshot.get("age_s"), (int, float))
        and float(snapshot.get("age_s")) <= FORMAL_MAX_REALTIME_SAMPLE_AGE_SECONDS
        and isinstance(status_payload.get("mode"), str)
        and isinstance(status_input.get("mains_present"), bool)
        and isinstance(power_diag_input.get("assist_power_stage"), str)
        and isinstance(power_diag_input.get("vin_vbus_mv"), (int, float))
        and isinstance(power_diag_input.get("vin_iin_ma"), (int, float))
    )


def settings_snapshot_ready(snapshot: dict[str, Any]) -> bool:
    payload = dict_or_empty(snapshot.get("payload"))
    return (
        isinstance(snapshot.get("generation"), int)
        and snapshot.get("generation", 0) >= 1
        and snapshot.get("error") is None
        and isinstance(payload.get("advanced_power"), dict)
        and isinstance(payload.get("advanced_power_capabilities"), dict)
    )


def isolapurr_snapshot_ready(snapshot: dict[str, Any]) -> bool:
    payload = dict_or_empty(snapshot.get("payload"))
    ports_root = payload.get("ports")
    if isinstance(ports_root, list):
        ports = ports_root
    else:
        ports = dict_or_empty(ports_root).get("ports")
    return (
        isinstance(snapshot.get("generation"), int)
        and snapshot.get("generation", 0) >= 1
        and isinstance(snapshot.get("age_s"), (int, float))
        and float(snapshot.get("age_s")) <= FORMAL_MAX_REALTIME_SAMPLE_AGE_SECONDS
        and isinstance(ports, list)
        and any(dict_or_empty(port).get("portId") == "port_c" for port in ports)
    )


def wait_for_scene_pollers_ready(
    *,
    ups_status_poller: JsonPoller,
    power_diag_poller: JsonPoller,
    isolapurr_poller: JsonPoller,
    sample_interval_seconds: float,
    timeout_sec: float,
    ups_device_id: str | None = None,
) -> dict[str, Any]:
    _ = ups_device_id
    started_at = time.monotonic()
    deadline = started_at + max(0.1, timeout_sec)
    freshness_limit_s = max(sample_interval_seconds * 2.0, FORMAL_MAX_REALTIME_SAMPLE_AGE_SECONDS)
    last_snapshots: dict[str, Any] = {}
    while time.monotonic() < deadline:
        time.sleep(0.05)
        now = time.monotonic()
        snapshots = {
            "ups_status": ups_status_poller.snapshot(now),
            "power_diag": power_diag_poller.snapshot(now),
            "isolapurr": isolapurr_poller.snapshot(now),
        }
        last_snapshots = snapshots
        ups_ready_snapshot = snapshots["ups_status"]
        power_diag_ready_snapshot = snapshots["power_diag"]
        trace_snapshot = None
        if trace_snapshot_ready(snapshots["ups_status"]):
            trace_snapshot = snapshots["ups_status"]
        elif trace_snapshot_ready(snapshots["power_diag"]):
            trace_snapshot = snapshots["power_diag"]
        if trace_snapshot is not None:
            ups_ready_snapshot = {
                **snapshots["ups_status"],
                "payload": status_from_trace_snapshot(trace_snapshot),
            }
            power_diag_ready_snapshot = {
                **snapshots["power_diag"],
                "payload": power_diag_from_trace_snapshot(trace_snapshot),
            }
        else:
            listing_status_payload = ups_status_from_devd_listing_snapshot(
                snapshots["ups_status"],
                device_id=ups_device_id,
            )
            listing_power_diag_payload = power_diag_from_devd_listing_snapshot(
                snapshots["power_diag"],
                device_id=ups_device_id,
            )
            if listing_status_payload:
                ups_ready_snapshot = {
                    **snapshots["ups_status"],
                    "payload": listing_status_payload,
                }
            if listing_power_diag_payload:
                power_diag_ready_snapshot = {
                    **snapshots["power_diag"],
                    "payload": listing_power_diag_payload,
                }
        readiness = {
            "ups_status": ups_snapshot_ready(ups_ready_snapshot),
            "power_diag": power_diag_snapshot_ready(power_diag_ready_snapshot),
            "isolapurr": isolapurr_snapshot_ready(snapshots["isolapurr"]),
        }
        if all(readiness.values()):
            return {
                "ready": True,
                "waited_s": round(now - started_at, 3),
                "freshness_limit_s": freshness_limit_s,
                "snapshots": {
                    key: {
                        "generation": snapshot.get("generation"),
                        "age_s": snapshot.get("age_s"),
                        "elapsed_ms": snapshot.get("elapsed_ms"),
                        "error": snapshot.get("error"),
                    }
                    for key, snapshot in snapshots.items()
                },
            }
    raise RuntimeError(
        "scene_pollers_not_ready "
        f"timeout_sec={timeout_sec} "
        f"freshness_limit_s={freshness_limit_s} "
        f"last_snapshots={last_snapshots!r}"
    )


def load_cc(
    args: argparse.Namespace,
    load_device: str,
    current_ma: int,
    *,
    min_v_mv: int,
    max_i_ma_total: int,
    max_p_mw: int,
    timeout_sec: float,
    status_timeout_sec: float,
    verify_timeout_sec: float,
    status_observer: Callable[[Any], None] | None = None,
    live_status_poller: "LoadStatusPoller | None" = None,
    before_verify: Callable[[], None] | None = None,
    bridge_lease: dict[str, Any] | None = None,
    allow_command_ack_shortcut: bool = False,
) -> dict[str, Any]:
    owned_bridge_lease = False
    configure_cmd: list[str] | None = None
    if effective_load_bridge_url(args):
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
        configure_cmd = loadlynx_cmd(
            args,
            "cc",
            str(current_ma),
            "--min-v-mv",
            str(min_v_mv),
            "--max-i-ma-total",
            str(max_i_ma_total),
            "--max-p-mw",
            str(max_p_mw),
            "--json",
            "--url",
            build_load_bridge_cli_url(
                args,
                timeout_sec=min(status_timeout_sec, 5.0),
                bridge_lease=bridge_lease,
            ),
        )
    else:
        configure_cmd = loadlynx_direct_or_ipc_cmd(
            args,
            "cc",
            str(current_ma),
            "--device",
            load_device,
            "--min-v-mv",
            str(min_v_mv),
            "--max-i-ma-total",
            str(max_i_ma_total),
            "--max-p-mw",
            str(max_p_mw),
            "--json",
        )
    if bridge_lease is not None:
        configure_command_parts = (
            "cc",
            str(current_ma),
            "--min-v-mv",
            str(min_v_mv),
            "--max-i-ma-total",
            str(max_i_ma_total),
            "--max-p-mw",
            str(max_p_mw),
            "--json",
        )
    else:
        configure_command_parts = ()
    configure_completed: subprocess.CompletedProcess[str] | None = None
    timeout_error: subprocess.TimeoutExpired | None = None
    process_error: subprocess.CalledProcessError | None = None
    configure_attempt_errors: list[str] = []
    configure_attempt_count = 0
    for attempt in range(3):
        configure_attempt_count = attempt + 1
        timeout_error = None
        process_error = None
        try:
            if bridge_lease is not None:
                configure_completed = run_loadlynx_bridge_command(
                    args,
                    bridge_lease,
                    *configure_command_parts,
                    timeout_sec=timeout_sec,
                )
            else:
                configure_completed = run_loadlynx(configure_cmd, timeout_sec=timeout_sec)
            break
        except subprocess.TimeoutExpired as exc:
            timeout_error = exc
            configure_attempt_errors.append(repr(exc))
        except subprocess.CalledProcessError as exc:
            process_error = exc
            configure_attempt_errors.append(repr(exc))
        if attempt < 2:
            time.sleep(0.3)
    if before_verify is not None:
        before_verify()
    command_response = load_command_response_payload(
        configure_completed.stdout if configure_completed is not None else None,
        process_error.stdout if process_error is not None else None,
        timeout_error.stdout if timeout_error is not None else None,
    )
    command_enabled, command_target_i_ma = select_effective_load_state(
        command_response,
        command_response,
    )
    command_proved_enabled = (
        command_enabled is True
        and command_target_i_ma == current_ma
    )
    if (
        not command_proved_enabled
        and configure_completed is not None
        and process_error is None
        and timeout_error is None
    ):
        command_proved_enabled = True
        if not isinstance(command_response, dict):
            command_response = {
                "output_enabled": True,
                "target_i_ma": current_ma,
                "mode": "CC",
                "source": "released_cc_command_contract",
            }
    if allow_command_ack_shortcut and command_proved_enabled:
        synthetic_status = {
            "control": {
                "output_enabled": True,
                "target_i_ma": current_ma,
                "mode": "cc",
            },
            "status": {
                "enable": True,
            },
            "source": "command_ack_synthetic_status",
        }
        if status_observer is not None:
            status_observer(synthetic_status)
        verified_status = {
            "control": synthetic_status["control"],
            "status": synthetic_status,
            "effective_enabled": True,
            "effective_target_i_ma": current_ma,
            "degraded_verification": True,
            "degraded_from_command_ack": True,
            "command_response": command_response,
            "command_attempt_count": configure_attempt_count,
            "command_attempt_errors": configure_attempt_errors,
        }
        result = {
            "cmd": configure_cmd,
            "configure_cmd": configure_cmd,
            "enable_cmd": None,
            "configure_attempt_count": configure_attempt_count,
            "configure_attempt_errors": configure_attempt_errors,
            "verified_status": verified_status,
        }
        if configure_completed is not None:
            result["stdout"] = configure_completed.stdout
            result["stderr"] = configure_completed.stderr
            result["configure_stdout"] = configure_completed.stdout
            result["configure_stderr"] = configure_completed.stderr
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
        if owned_bridge_lease:
            release_load_bridge_lease_quietly(
                args,
                bridge_lease,
                timeout_sec=min(status_timeout_sec, 5.0),
            )
        return result
    try:
        verified_status = wait_for_load_state(
            args,
            load_device,
            expected_enabled=True,
            expected_target_i_ma=current_ma,
            status_timeout_sec=status_timeout_sec,
            verify_timeout_sec=verify_timeout_sec,
            status_observer=status_observer,
            live_status_poller=live_status_poller,
            bridge_lease=bridge_lease,
        )
    except RuntimeError as verify_exc:
        if command_proved_enabled:
            synthetic_status = {
                "control": {
                    "output_enabled": True,
                    "target_i_ma": current_ma,
                    "mode": "cc",
                },
                "status": {
                    "enable": True,
                },
                "source": "command_ack_synthetic_status",
            }
            verified_status = {
                "control": synthetic_status["control"],
                "status": synthetic_status,
                "effective_enabled": True,
                "effective_target_i_ma": current_ma,
                "degraded_verification": True,
                "degraded_from_command_ack": True,
                "command_response": command_response,
                "verify_error": repr(verify_exc),
                "command_attempt_count": configure_attempt_count,
                "command_attempt_errors": configure_attempt_errors,
            }
        else:
            try:
                verified_status = confirm_load_state_with_direct_cli(
                    args,
                    load_device,
                    expected_enabled=True,
                    expected_target_i_ma=current_ma,
                    timeout_sec=status_timeout_sec,
                    status_observer=status_observer,
                )
                verified_status["degraded_from_command_ack"] = False
                verified_status["command_response"] = command_response
                verified_status["verify_error"] = repr(verify_exc)
                verified_status["command_attempt_count"] = configure_attempt_count
                verified_status["command_attempt_errors"] = configure_attempt_errors
                verified_status["command_stdout"] = (
                    configure_completed.stdout if configure_completed is not None else None
                )
                verified_status["command_stderr"] = (
                    configure_completed.stderr if configure_completed is not None else None
                )
            except RuntimeError:
                if command_response_proves_load_enabled(
                    command_response,
                    expected_target_i_ma=current_ma,
                ):
                    verified_status = {
                        "control": command_response,
                        "status": None,
                        "effective_enabled": True,
                        "effective_target_i_ma": current_ma,
                        "degraded_verification": True,
                        "degraded_from_command_ack": True,
                        "command_response": command_response,
                        "verify_error": repr(verify_exc),
                        "command_attempt_count": configure_attempt_count,
                        "command_attempt_errors": configure_attempt_errors,
                        "command_stdout": (
                            configure_completed.stdout if configure_completed is not None else None
                        ),
                        "command_stderr": (
                            configure_completed.stderr if configure_completed is not None else None
                        ),
                    }
                else:
                    final_status = get_load_status_best_effort(
                        args,
                        load_device,
                        timeout_sec=status_timeout_sec,
                        bridge_lease=bridge_lease,
                    )
                    if status_observer is not None:
                        status_observer(final_status)
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
                    if final_enabled is not True or final_target_i_ma != current_ma:
                        raise
                    verified_status = {
                        "control": final_control,
                        "status": final_status,
                        "effective_enabled": final_enabled,
                        "effective_target_i_ma": final_target_i_ma,
                        "degraded_verification": True,
                        "degraded_from_command_ack": False,
                        "command_response": command_response,
                        "verify_error": repr(verify_exc),
                        "command_attempt_count": configure_attempt_count,
                        "command_attempt_errors": configure_attempt_errors,
                        "command_stdout": (
                            configure_completed.stdout if configure_completed is not None else None
                        ),
                        "command_stderr": (
                            configure_completed.stderr if configure_completed is not None else None
                        ),
                    }
    result = {
        "cmd": configure_cmd,
        "configure_cmd": configure_cmd,
        "enable_cmd": None,
        "configure_attempt_count": configure_attempt_count,
        "configure_attempt_errors": configure_attempt_errors,
        "verified_status": verified_status,
    }
    if configure_completed is not None:
        result["stdout"] = configure_completed.stdout
        result["stderr"] = configure_completed.stderr
        result["configure_stdout"] = (
            configure_completed.stdout if configure_completed is not None else None
        )
        result["configure_stderr"] = (
            configure_completed.stderr if configure_completed is not None else None
        )
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
    if owned_bridge_lease:
        release_load_bridge_lease_quietly(
            args,
            bridge_lease,
            timeout_sec=min(status_timeout_sec, 5.0),
        )
    return result


def disable_load(
    args: argparse.Namespace,
    load_device: str,
    *,
    timeout_sec: float,
    status_timeout_sec: float,
    verify_timeout_sec: float,
    status_observer: Callable[[Any], None] | None = None,
    assume_enabled: bool = False,
    allow_command_ack_shortcut: bool = False,
    live_status_poller: "LoadStatusPoller | None" = None,
    before_verify: Callable[[], None] | None = None,
    bridge_lease: dict[str, Any] | None = None,
) -> dict[str, Any]:
    owned_bridge_lease = False
    if effective_load_bridge_url(args):
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
        cmd = loadlynx_direct_or_ipc_cmd(
            args,
            "control",
            "set",
            "--device",
            load_device,
            "--disable",
        )
    status: Any = None
    control: Any = None
    effective_target_i_ma: int | None = None
    if not assume_enabled:
        status = get_load_status_best_effort(
            args,
            load_device,
            timeout_sec=status_timeout_sec,
            bridge_lease=bridge_lease,
        )
        if status_observer is not None:
            status_observer(status)
        enabled_from_status = load_output_enabled(normalize_verified_load_payload(status))
        effective_target_i_ma = load_target_i_ma(normalize_verified_load_payload(status))
        if enabled_from_status is False:
            return {
                "cmd": None,
                "skipped": True,
                "reason": "already_disabled",
                "control": control,
                "status": status,
                "effective_enabled": enabled_from_status,
                "effective_target_i_ma": effective_target_i_ma,
            }
        control = get_load_control_best_effort(
            args,
            load_device,
            timeout_sec=status_timeout_sec,
            bridge_lease=bridge_lease,
        )
        enabled, effective_target_i_ma = select_effective_load_state(control, status)
        if enabled is False:
            return {
                "cmd": None,
                "skipped": True,
                "reason": "already_disabled",
                "control": control,
                "status": status,
                "effective_enabled": enabled,
                "effective_target_i_ma": effective_target_i_ma,
            }
    completed: subprocess.CompletedProcess[str] | None = None
    timeout_error: subprocess.TimeoutExpired | None = None
    process_error: subprocess.CalledProcessError | None = None
    command_attempt_errors: list[str] = []
    command_attempt_count = 0
    for attempt in range(3):
        command_attempt_count = attempt + 1
        timeout_error = None
        process_error = None
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
            break
        except subprocess.TimeoutExpired as exc:
            timeout_error = exc
            command_attempt_errors.append(repr(exc))
        except subprocess.CalledProcessError as exc:
            process_error = exc
            command_attempt_errors.append(repr(exc))
        if attempt < 2:
            time.sleep(0.3)
    command_output_text = "\n".join(
        part
        for part in (
            completed.stdout if completed is not None else None,
            process_error.stdout if process_error is not None else None,
            timeout_error.stdout if timeout_error is not None else None,
        )
        if isinstance(part, str)
    )
    command_ack_disabled = "output=false" in command_output_text
    if allow_command_ack_shortcut and command_ack_disabled:
        result = {
            "cmd": cmd,
            "stdout": completed.stdout if completed is not None else (
                process_error.stdout if process_error is not None else (
                    timeout_error.stdout if timeout_error is not None else None
                )
            ),
            "stderr": completed.stderr if completed is not None else (
                process_error.stderr if process_error is not None else (
                    timeout_error.stderr if timeout_error is not None else None
                )
            ),
            "timed_out_but_verified": timeout_error is not None,
            "nonzero_but_verified": process_error is not None,
            "command_ack_disabled": True,
            "command_attempt_count": command_attempt_count,
            "command_attempt_errors": command_attempt_errors,
            "effective_target_i_ma": effective_target_i_ma,
        }
        if owned_bridge_lease:
            release_load_bridge_lease_quietly(
                args,
                bridge_lease,
                timeout_sec=min(status_timeout_sec, 5.0),
            )
        return result
    if before_verify is not None:
        before_verify()
    try:
        verified_status = wait_for_load_state(
            args,
            load_device,
            expected_enabled=False,
            expected_target_i_ma=None,
            status_timeout_sec=status_timeout_sec,
            verify_timeout_sec=verify_timeout_sec,
            status_observer=status_observer,
            live_status_poller=live_status_poller,
            bridge_lease=bridge_lease,
        )
    except RuntimeError as verify_exc:
        command_text = "\n".join(
            part for part in (
                completed.stdout if completed is not None else None,
                process_error.stdout if process_error is not None else None,
                timeout_error.stdout if timeout_error is not None else None,
            ) if isinstance(part, str)
        )
        command_proved_disabled = "output=false" in command_text
        if command_proved_disabled:
            verified_status = {
                "control": None,
                "status": None,
                "effective_enabled": False,
                "effective_target_i_ma": None,
                "degraded_verification": True,
                "degraded_from_command_ack": True,
                "verify_error": repr(verify_exc),
                "command_attempt_count": command_attempt_count,
                "command_attempt_errors": command_attempt_errors,
            }
        else:
            try:
                verified_status = confirm_load_state_with_direct_cli(
                    args,
                    load_device,
                    expected_enabled=False,
                    expected_target_i_ma=None,
                    timeout_sec=status_timeout_sec,
                    status_observer=status_observer,
                )
                verified_status["degraded_from_command_ack"] = False
                verified_status["verify_error"] = repr(verify_exc)
                verified_status["command_attempt_count"] = command_attempt_count
                verified_status["command_attempt_errors"] = command_attempt_errors
                verified_status["command_stdout"] = completed.stdout if completed is not None else None
                verified_status["command_stderr"] = completed.stderr if completed is not None else None
            except RuntimeError:
                final_status = get_load_status_best_effort(
                    args,
                    load_device,
                    timeout_sec=status_timeout_sec,
                    bridge_lease=bridge_lease,
                )
                if status_observer is not None:
                    status_observer(final_status)
                final_control = get_load_control_best_effort(
                    args,
                    load_device,
                    timeout_sec=status_timeout_sec,
                    bridge_lease=bridge_lease,
                )
                final_enabled, final_target_i_ma = select_effective_load_state(final_control, final_status)
                if final_enabled is not False:
                    raise
                verified_status = {
                    "control": final_control,
                    "status": final_status,
                    "effective_enabled": final_enabled,
                    "effective_target_i_ma": final_target_i_ma,
                    "degraded_verification": True,
                    "degraded_from_command_ack": False,
                    "verify_error": repr(verify_exc),
                    "command_attempt_count": command_attempt_count,
                    "command_attempt_errors": command_attempt_errors,
                    "command_stdout": completed.stdout if completed is not None else None,
                    "command_stderr": completed.stderr if completed is not None else None,
                }
    result = {
        "cmd": cmd,
        "stdout": completed.stdout if completed is not None else (
            process_error.stdout if process_error is not None else (
                timeout_error.stdout if timeout_error is not None else None
            )
        ),
        "stderr": completed.stderr if completed is not None else (
            process_error.stderr if process_error is not None else (
                timeout_error.stderr if timeout_error is not None else None
            )
        ),
            "timed_out_but_verified": timeout_error is not None,
            "nonzero_but_verified": process_error is not None,
            "command_attempt_count": command_attempt_count,
            "command_attempt_errors": command_attempt_errors,
            "verified_status": verified_status,
    }
    if owned_bridge_lease:
        release_load_bridge_lease_quietly(
            args,
            bridge_lease,
            timeout_sec=min(status_timeout_sec, 5.0),
        )
    return result


def set_port_c_power(isolapurr_url: str, enabled: bool) -> dict[str, Any]:
    query = urllib.parse.urlencode({"enabled": "1" if enabled else "0"})
    url = f"{isolapurr_url.rstrip('/')}{PORT_C_POWER_PATH}?{query}"
    return {
        "url": url,
        "enabled": enabled,
        "response": http_post_json(url),
    }


def probe_isolapurr_source_reachability(
    isolapurr_url: str,
    *,
    timeout_sec: float,
    isolapurr_cli: str,
    expected_device_id: str | None = None,
) -> dict[str, Any]:
    http_url = f"{isolapurr_url.rstrip('/')}/api/v1/ports"
    cli_cmd = [
        isolapurr_cli,
        "status",
        "--url",
        isolapurr_url,
        "--json",
    ]
    failures: list[str] = []
    http_ports: dict[str, Any] | None = None
    cli_status: dict[str, Any] | None = None
    http_error: str | None = None
    cli_error: str | None = None
    observed_cli_device_id: str | None = None
    observed_http_port_ids: list[str] = []
    try:
        payload = http_json_with_retries(http_url, timeout_sec=timeout_sec)
        http_ports = dict_or_empty(payload)
        for entry in http_ports.get("ports") or []:
            if not isinstance(entry, dict):
                continue
            port_id = entry.get("portId")
            if isinstance(port_id, str) and port_id.strip():
                observed_http_port_ids.append(port_id.strip())
        if "port_c" not in observed_http_port_ids:
            failures.append("http_port_c_missing")
    except Exception as exc:  # noqa: BLE001
        http_error = repr(exc)
        failures.append("http_ports_unreachable")
    try:
        cli_payload = run_json_command_with_retries(
            cli_cmd,
            timeout_sec=timeout_sec,
        )
        cli_status = dict_or_empty(cli_payload)
        cli_device = dict_or_empty(cli_status.get("device"))
        if isinstance(cli_device.get("device_id"), str):
            observed_cli_device_id = cli_device.get("device_id", "").strip() or None
        if observed_cli_device_id is None:
            cli_identity = dict_or_empty(cli_status.get("identity"))
            if isinstance(cli_identity.get("deviceId"), str):
                observed_cli_device_id = cli_identity.get("deviceId", "").strip() or None
    except Exception as exc:  # noqa: BLE001
        cli_error = repr(exc)
        failures.append("cli_status_unreachable")
    normalized_expected_device_id = (
        str(expected_device_id).strip()
        if isinstance(expected_device_id, str) and expected_device_id.strip()
        else None
    )
    if normalized_expected_device_id:
        if observed_cli_device_id != normalized_expected_device_id:
            failures.append("cli_status_device_id_mismatch")
    return {
        "ok": not failures,
        "failures": failures,
        "isolapurr_url": isolapurr_url,
        "expected_device_id": normalized_expected_device_id,
        "http_ports_url": http_url,
        "http_ports": http_ports,
        "http_port_ids": observed_http_port_ids,
        "http_error": http_error,
        "cli_cmd": cli_cmd,
        "cli_status": cli_status,
        "cli_status_device_id": observed_cli_device_id,
        "cli_error": cli_error,
    }


def set_isolapurr_manual_output(
    isolapurr_url: str,
    *,
    voltage_mv: int,
    current_limit_ma: int,
    isolapurr_cli: str,
) -> dict[str, Any]:
    payload = run_json_command_with_retries(
        [
            isolapurr_cli,
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


def fetch_isolapurr_power_show(
    isolapurr_url: str,
    *,
    timeout_sec: float,
    isolapurr_cli: str,
) -> dict[str, Any]:
    payload = dict_or_empty(
        run_json_command_with_retries(
            [
                isolapurr_cli,
                "power",
                "show",
                "--url",
                isolapurr_url,
                "--json",
            ],
            timeout_sec=timeout_sec,
        )
    )
    payload.setdefault("source", "cli_power_show")
    return payload


def fetch_isolapurr_power_show_best_effort(
    isolapurr_url: str,
    *,
    timeout_sec: float,
    isolapurr_cli: str,
) -> dict[str, Any]:
    try:
        return fetch_isolapurr_power_show(
            isolapurr_url,
            timeout_sec=timeout_sec,
            isolapurr_cli=isolapurr_cli,
        )
    except Exception as exc:  # noqa: BLE001
        return {
            "source": "cli_power_show_error",
            "ok": False,
            "error": repr(exc),
        }


def fetch_isolapurr_ports(
    isolapurr_url: str,
    *,
    timeout_sec: float,
    isolapurr_cli: str,
) -> dict[str, Any]:
    try:
        payload = dict_or_empty(
            http_json_with_retries(
                f"{isolapurr_url.rstrip('/')}{PORTS_PATH}",
                timeout_sec=timeout_sec,
            )
        )
        payload.setdefault("source", "http_ports")
        return payload
    except Exception:
        payload = dict_or_empty(
            run_json_command_with_retries(
                [
                    isolapurr_cli,
                    "power",
                    "show",
                    "--url",
                    isolapurr_url,
                    "--json",
                ],
                timeout_sec=timeout_sec,
            )
        )
        payload.setdefault("source", "cli_power_show")
        return payload


def validate_isolapurr_source_configuration(
    *,
    expected_voltage_mv: int,
    expected_current_limit_ma: int,
    manual_ack_payload: Any,
    power_show_payload: Any,
    ports_payload: Any,
) -> dict[str, Any]:
    failures: list[str] = []
    manual_ack_root = dict_or_empty(manual_ack_payload)
    manual_ack = dict_or_empty(manual_ack_root.get("manual"))
    if not manual_ack:
        for action in manual_ack_root.get("actions") or []:
            action_dict = dict_or_empty(action)
            action_result = dict_or_empty(action_dict.get("result"))
            nested_manual = dict_or_empty(action_result.get("manual"))
            if nested_manual:
                manual_ack = nested_manual
                manual_ack_root = action_result
                break
    power_show = dict_or_empty(power_show_payload)
    power_show_config = dict_or_empty(power_show.get("config"))
    readback_manual = dict_or_empty(
        power_show.get("manual") or power_show_config.get("manual")
    )
    capability_root = dict_or_empty(power_show.get("capability") or power_show_config.get("capability"))
    capability_pd = dict_or_empty(capability_root.get("pd"))
    fixed_voltages_mv = capability_pd.get("fixed_voltages_mv")
    if isinstance(fixed_voltages_mv, list) and fixed_voltages_mv:
        if expected_voltage_mv not in fixed_voltages_mv:
            failures.append("source_capability_voltage_unsupported")
    if manual_ack.get("voltage_mv") != expected_voltage_mv:
        failures.append("manual_ack_voltage_mismatch")
    if manual_ack.get("current_limit_ma") != expected_current_limit_ma:
        failures.append("manual_ack_current_limit_mismatch")
    has_manual_readback = bool(readback_manual)
    if has_manual_readback:
        if readback_manual.get("voltage_mv") != expected_voltage_mv:
            failures.append("manual_readback_voltage_mismatch")
        if readback_manual.get("current_limit_ma") != expected_current_limit_ma:
            failures.append("manual_readback_current_limit_mismatch")
        if readback_manual.get("path_policy") != "force_close":
            failures.append("manual_readback_path_policy_mismatch")
        if readback_manual.get("usb_c_path_mode") != "disconnect":
            failures.append("manual_readback_usb_c_path_mode_mismatch")
    readback_tps_mode = power_show.get("tps_mode") or power_show_config.get("tps_mode")
    if has_manual_readback and readback_tps_mode != "manual":
        failures.append("manual_readback_tps_mode_mismatch")
    port_c = port_state(dict_or_empty(ports_payload), port_id="port_c")
    port_c_state = dict_or_empty(port_c.get("state"))
    if port_c_state.get("power_enabled") is not False:
        failures.append("port_c_not_disabled_during_source_config")
    return {
        "ok": not failures,
        "failures": failures,
        "expected": {
            "voltage_mv": expected_voltage_mv,
            "current_limit_ma": expected_current_limit_ma,
            "path_policy": "force_close",
            "usb_c_path_mode": "disconnect",
            "tps_mode": "manual",
        },
        "manual_ack": manual_ack_root,
        "power_show": {
            "manual": readback_manual,
            "tps_mode": readback_tps_mode,
            "capability": capability_root,
            "source": power_show.get("source"),
        },
        "ports": {
            "source": dict_or_empty(ports_payload).get("source"),
            "port_c": port_c,
        },
    }


def status_from_trace_snapshot(snapshot: dict[str, Any]) -> dict[str, Any]:
    payload = dict_or_empty(devd_read_sample(snapshot.get("payload")))
    return dict_or_empty(payload.get("status"))


def power_diag_from_trace_snapshot(snapshot: dict[str, Any]) -> dict[str, Any]:
    payload = dict_or_empty(devd_read_sample(snapshot.get("payload")))
    return dict_or_empty(payload.get("power_diag"))


def ups_status_from_devd_listing_snapshot(
    snapshot: dict[str, Any],
    *,
    device_id: str | None,
) -> dict[str, Any]:
    payload = dict_or_empty(devd_read_sample(snapshot.get("payload")))
    return dict_or_empty(devd_device_entry_from_listing(payload, device_id=device_id).get("status"))


def power_diag_from_devd_listing_snapshot(
    snapshot: dict[str, Any],
    *,
    device_id: str | None,
) -> dict[str, Any]:
    payload = dict_or_empty(devd_read_sample(snapshot.get("payload")))
    return dict_or_empty(
        devd_device_entry_from_listing(payload, device_id=device_id).get("power_diag")
    )


def port_state(
    ports_payload: dict[str, Any],
    *,
    port_id: str,
) -> dict[str, Any]:
    ports_root = ports_payload.get("ports")
    if isinstance(ports_root, list):
        ports = ports_root
    else:
        ports = dict_or_empty(ports_root).get("ports", [])
    for port in ports:
        if port.get("portId") == port_id:
            if not isinstance(port, dict):
                return {}
            normalized = dict(port)
            if not isinstance(normalized.get("state"), dict):
                telemetry = dict_or_empty(normalized.get("telemetry"))
                diagnostics = dict_or_empty(ports_payload.get("diagnostics"))
                usb_c_power_enabled = diagnostics.get("usb_c_power_enabled")
                if port_id == "port_c" and isinstance(usb_c_power_enabled, bool):
                    normalized["state"] = {"power_enabled": usb_c_power_enabled}
                elif telemetry.get("status") == "not_inserted":
                    normalized["state"] = {"power_enabled": False}
            return normalized
    return {}


def maybe_promote_ups_status_url_to_direct_lan(
    requested_url: str,
    *,
    lan_address: str | None,
) -> str:
    if not isinstance(lan_address, str) or not lan_address.strip():
        return requested_url
    parsed = urllib.parse.urlparse(requested_url)
    if parsed.scheme not in {"http", "https"}:
        return requested_url
    if parsed.hostname not in {"127.0.0.1", "localhost"}:
        return requested_url
    target_path = parsed.path or "/api/v1/status"
    if not target_path.endswith("/status"):
        return requested_url
    return urllib.parse.urlunparse(
        (
            parsed.scheme,
            lan_address.strip(),
            "/api/v1/status",
            "",
            "",
            "",
        )
    )



def capture_three_device_sample(
    *,
    phase: str,
    t_s: float,
    load_device: str,
    ups_status_url: str,
    ups_settings_url: str,
    devd_power_diag_url: str,
    isolapurr_url: str,
    status_timeout_sec: float,
    load_status_snapshot: dict[str, Any],
    ups_status_snapshot: dict[str, Any],
    power_diag_snapshot: dict[str, Any],
    settings_snapshot: dict[str, Any],
    isolapurr_snapshot: dict[str, Any],
) -> dict[str, Any]:
    _ = (ups_status_url, ups_settings_url, devd_power_diag_url, isolapurr_url, status_timeout_sec)
    ups_device_id = devd_device_id_from_endpoint(devd_power_diag_url)
    raw_ups_status_payload = ups_status_snapshot.get("payload")
    raw_power_diag_payload = power_diag_snapshot.get("payload")
    raw_settings_payload = settings_snapshot.get("payload")
    raw_isolapurr_payload = isolapurr_snapshot.get("payload")
    direct_ups_status = dict_or_empty(devd_read_sample(raw_ups_status_payload))
    if not looks_like_ups_status_payload(direct_ups_status):
        direct_ups_status = {}
    trace_status = direct_ups_status
    if not trace_status:
        trace_status = status_from_trace_snapshot(ups_status_snapshot)
    if not trace_status:
        trace_status = status_from_trace_snapshot(power_diag_snapshot)
    if not trace_status:
        trace_status = ups_status_from_devd_listing_snapshot(
            ups_status_snapshot,
            device_id=ups_device_id,
        )
    if not trace_status:
        trace_status = ups_status_from_devd_listing_snapshot(
            power_diag_snapshot,
            device_id=ups_device_id,
        )
    direct_power_diag = dict_or_empty(devd_read_sample(raw_power_diag_payload))
    if not looks_like_power_diag_payload(direct_power_diag):
        direct_power_diag = {}
    trace_power_diag = direct_power_diag
    if not trace_power_diag:
        trace_power_diag = power_diag_from_trace_snapshot(power_diag_snapshot)
    if not trace_power_diag:
        trace_power_diag = power_diag_from_trace_snapshot(ups_status_snapshot)
    if not trace_power_diag:
        trace_power_diag = power_diag_from_devd_listing_snapshot(
            power_diag_snapshot,
            device_id=ups_device_id,
        )
    if not trace_power_diag:
        trace_power_diag = power_diag_from_devd_listing_snapshot(
            ups_status_snapshot,
            device_id=ups_device_id,
        )
    if not trace_power_diag:
        trace_power_diag = derive_power_diag_from_status(
            trace_status or dict_or_empty(ups_status_snapshot.get("payload")),
            source="ups_status_derived",
        )
    fetch_elapsed_ms = {
        "ups_status": ups_status_snapshot.get("elapsed_ms"),
        "power_diag": power_diag_snapshot.get("elapsed_ms"),
        "settings": settings_snapshot.get("elapsed_ms"),
        "isolapurr_power": isolapurr_snapshot.get("elapsed_ms"),
    }
    fetch_errors = {
        key: value
        for key, value in {
            "ups_status": ups_status_snapshot.get("error"),
            "power_diag": power_diag_snapshot.get("error"),
            "settings": settings_snapshot.get("error"),
            "isolapurr_power": isolapurr_snapshot.get("error"),
        }.items()
        if value
    }
    fetch_total_elapsed_ms = max(
        [
            int(value)
            for value in fetch_elapsed_ms.values()
            if isinstance(value, (int, float))
        ],
        default=0,
    )
    fetch_age_s = {
        "ups_status": ups_status_snapshot.get("age_s"),
        "power_diag": power_diag_snapshot.get("age_s"),
        "settings": settings_snapshot.get("age_s"),
        "isolapurr_power": isolapurr_snapshot.get("age_s"),
    }
    sample_age_s = {
        "ups_status": devd_snapshot_sample_age_s(
            payload=raw_ups_status_payload,
            fetch_age_s=ups_status_snapshot.get("age_s"),
        ),
        "power_diag": devd_snapshot_sample_age_s(
            payload=raw_power_diag_payload,
            fetch_age_s=power_diag_snapshot.get("age_s"),
        ),
        "settings": devd_snapshot_sample_age_s(
            payload=raw_settings_payload,
            fetch_age_s=settings_snapshot.get("age_s"),
        ),
        "isolapurr_power": devd_snapshot_sample_age_s(
            payload=raw_isolapurr_payload,
            fetch_age_s=isolapurr_snapshot.get("age_s"),
        ),
    }
    cache_fresh = {
        "ups_status": devd_read_meta(raw_ups_status_payload).get("cache_fresh"),
        "power_diag": devd_read_meta(raw_power_diag_payload).get("cache_fresh"),
        "settings": devd_read_meta(raw_settings_payload).get("cache_fresh"),
        "isolapurr_power": devd_read_meta(raw_isolapurr_payload).get("cache_fresh"),
    }
    ups_status = trace_status or dict_or_empty(devd_read_sample(raw_ups_status_payload))
    power_diag = trace_power_diag or dict_or_empty(devd_read_sample(raw_power_diag_payload))
    settings = dict_or_empty(devd_read_sample(raw_settings_payload))
    isolapurr_power = dict_or_empty(devd_read_sample(raw_isolapurr_payload))
    load_status = normalize_load_status_payload(load_status_snapshot.get("status"))
    load_status = dict_or_empty(load_status)
    load_control = {
        "derived_from_status": True,
        "control": load_status.get("control") if isinstance(load_status, dict) else None,
        "status": load_status.get("status") if isinstance(load_status, dict) else None,
    }
    ups_output = dict_or_empty(ups_status.get("output"))
    out_a = dict_or_empty(ups_output.get("out_a"))
    out_b = dict_or_empty(ups_output.get("out_b"))
    ups_input = dict_or_empty(ups_status.get("input"))
    ups_battery = dict_or_empty(ups_status.get("battery"))
    ups_charger = dict_or_empty(ups_status.get("charger"))
    diag_input = dict_or_empty(power_diag.get("input"))
    vin_vbus_mv = ups_input.get("vin_vbus_mv")
    if not isinstance(vin_vbus_mv, (int, float)):
        vin_vbus_mv = diag_input.get("vin_vbus_mv")
    vin_iin_ma = ups_input.get("vin_iin_ma")
    if not isinstance(vin_iin_ma, (int, float)):
        vin_iin_ma = diag_input.get("vin_iin_ma")
    input_vbus_mv = ups_input.get("input_vbus_mv")
    if not isinstance(input_vbus_mv, (int, float)):
        input_vbus_mv = diag_input.get("input_vbus_mv")
    input_ibus_ma = ups_input.get("input_ibus_ma")
    if not isinstance(input_ibus_ma, (int, float)):
        input_ibus_ma = diag_input.get("input_ibus_ma")
    mode = ups_status.get("mode")
    if not isinstance(mode, str):
        mode = power_diag.get("input", {}).get("assist_power_stage")
    stage = ups_input.get("assist_power_stage")
    if not isinstance(stage, str):
        stage = power_diag.get("input", {}).get("assist_power_stage")
    assist_target_vout_mv = ups_input.get("assist_target_vout_mv")
    if not isinstance(assist_target_vout_mv, (int, float)):
        assist_target_vout_mv = power_diag.get("input", {}).get("assist_target_vout_mv")
    mains_present = ups_input.get("mains_present")
    if not isinstance(mains_present, bool):
        mains_present = diag_input.get("mains_present")
    port_c_entry = port_state(isolapurr_power, port_id="port_c")
    port_c = dict_or_empty(port_c_entry.get("telemetry"))
    port_c_state = dict_or_empty(port_c_entry.get("state"))
    raw_load_status = dict_or_empty(load_status.get("status"))
    return {
        "captured_at_utc": datetime.now(timezone.utc).isoformat(),
        "phase": phase,
        "t_s": round(t_s, 3),
        "mode": mode,
        "mains_present": mains_present,
        "stage": stage,
        "assist_target_vout_mv": assist_target_vout_mv,
        "vin_vbus_mv": vin_vbus_mv,
        "vin_iin_ma": vin_iin_ma,
        "input_vbus_mv": input_vbus_mv,
        "input_ibus_ma": input_ibus_ma,
        "tps_total_iout_ma": ups_input.get("tps_total_iout_ma"),
        "battery_current_ma": ups_battery.get("current_ma"),
        "charger_allow_charge": ups_charger.get("allow_charge"),
        "charger_detail_status": ups_charger.get("detail_status"),
        "diag_stage": power_diag.get("input", {}).get("assist_power_stage"),
        "diag_assist_target_vout_mv": power_diag.get("input", {}).get("assist_target_vout_mv"),
        "diag_vin_vbus_mv": power_diag.get("input", {}).get("vin_vbus_mv"),
        "diag_vin_iin_ma": power_diag.get("input", {}).get("vin_iin_ma"),
        "diag_vin_baseline_mv": power_diag.get("input", {}).get("vin_baseline_mv"),
        "diag_vin_drop_mv": power_diag.get("input", {}).get("vin_drop_mv"),
        "diag_tps_total_iout_ma": power_diag.get("input", {}).get("tps_total_iout_ma"),
        "out_a_vbus_mv": out_a.get("vbus_mv"),
        "out_a_iout_ma": out_a.get("iout_ma"),
        "out_b_vbus_mv": out_b.get("vbus_mv"),
        "out_b_iout_ma": out_b.get("iout_ma"),
        "ups_vout_mv": preferred_ups_vout_mv(out_a.get("vbus_mv"), out_b.get("vbus_mv")),
        "port_c_enabled": port_c_state.get("power_enabled"),
        "isolapurr_port_c_mv": port_c.get("voltage_mv"),
        "isolapurr_port_c_ma": port_c.get("current_ma"),
        "load_output_enabled": load_output_enabled(load_control),
        "load_target_i_ma": load_target_i_ma(load_control),
        "load_v_local_mv": raw_load_status.get("v_local_mv"),
        "load_v_remote_mv": raw_load_status.get("v_remote_mv"),
        "load_i_local_ma": raw_load_status.get("i_local_ma"),
        "load_i_remote_ma": raw_load_status.get("i_remote_ma"),
        "load_i_total_ma": load_status_i_total_ma(load_status),
        "load_calc_p_mw": raw_load_status.get("calc_p_mw"),
        "load_status_generation": load_status_snapshot.get("generation"),
        "load_status_age_s": load_status_snapshot.get("age_s"),
        "load_status_sample_age_s": load_status_snapshot.get("sample_age_s"),
        "load_status_sampled_at_ms": load_status_snapshot.get("device_sampled_at_ms"),
        "load_status_error": load_status_snapshot.get("error"),
        "load_status_source": load_status_snapshot.get("source"),
        "fetch_elapsed_ms": fetch_elapsed_ms,
        "fetch_age_s": fetch_age_s,
        "sample_age_s": sample_age_s,
        "cache_fresh": cache_fresh,
        "fetch_total_elapsed_ms": fetch_total_elapsed_ms,
        "fetch_errors": fetch_errors or None,
        "raw": {
            "ups_status": ups_status,
            "power_diag": power_diag,
            "settings": settings,
            "isolapurr_power": isolapurr_power,
            "load_control": load_control,
            "load_status": load_status,
        },
    }


def capture_phase_series(
    jsonl_path: Path,
    *,
    phase: str,
    duration_seconds: float,
    sample_interval_seconds: float,
    started_at: float,
    load_device: str,
    ups_status_url: str,
    ups_settings_url: str,
    devd_power_diag_url: str,
    isolapurr_url: str,
    status_timeout_sec: float,
    load_status_poller: LoadStatusPoller,
) -> list[dict[str, Any]]:
    samples: list[dict[str, Any]] = []
    deadline = time.monotonic() + max(0.0, duration_seconds)
    freshness_grace_deadline = deadline + max(sample_interval_seconds * 2.0, 2.0)
    phase_initial_generation: int | None = None
    while True:
        now = time.monotonic()
        current_snapshot = load_status_poller.snapshot(now)
        if phase_initial_generation is None and isinstance(current_snapshot.get("generation"), int):
            phase_initial_generation = current_snapshot.get("generation")
        if now > deadline and samples:
            latest_generation = current_snapshot.get("generation")
            latest_age_s = current_snapshot.get("age_s")
            if (
                isinstance(phase_initial_generation, int)
                and isinstance(latest_generation, int)
                and latest_generation > phase_initial_generation
            ):
                break
            if isinstance(latest_age_s, (int, float)) and latest_age_s <= max(
                sample_interval_seconds * 2.0,
                2.0,
            ):
                break
            if now >= freshness_grace_deadline:
                break
        sample = capture_three_device_sample(
            phase=phase,
            t_s=now - started_at,
            load_device=load_device,
            ups_status_url=ups_status_url,
            ups_settings_url=ups_settings_url,
            devd_power_diag_url=devd_power_diag_url,
            isolapurr_url=isolapurr_url,
            status_timeout_sec=status_timeout_sec,
            load_status_snapshot=current_snapshot,
        )
        samples.append(sample)
        append_jsonl(jsonl_path, sample)
        now_after_sample = time.monotonic()
        latest_generation = current_snapshot.get("generation")
        latest_age_s = current_snapshot.get("age_s")
        if now_after_sample >= deadline:
            if (
                isinstance(phase_initial_generation, int)
                and isinstance(latest_generation, int)
                and latest_generation > phase_initial_generation
            ):
                break
            if isinstance(latest_age_s, (int, float)) and latest_age_s <= max(
                sample_interval_seconds * 2.0,
                2.0,
            ):
                break
            if now_after_sample >= freshness_grace_deadline:
                break
        time.sleep(sample_interval_seconds)
    return samples


def build_formal_sampling_metrics(samples: list[dict[str, Any]]) -> dict[str, Any]:
    if len(samples) < 2:
        return {
            "sample_count": len(samples),
            "span_s": 0.0,
            "effective_sample_rate_hz": None,
            "max_sample_gap_s": None,
            "formal_sampling_ok": False,
            "sampling_failures": ["too_few_samples"],
        }
    gaps = [
        round(float(curr["t_s"]) - float(prev["t_s"]), 3)
        for prev, curr in zip(samples, samples[1:])
    ]
    span_s = round(float(samples[-1]["t_s"]) - float(samples[0]["t_s"]), 3)
    effective_sample_rate_hz = round(((len(samples) - 1) / span_s), 3) if span_s > 0 else None
    max_sample_gap_s = max(gaps)
    failures: list[str] = []
    if effective_sample_rate_hz is None or effective_sample_rate_hz < FORMAL_MIN_EFFECTIVE_SAMPLE_RATE_HZ:
        failures.append("sample_rate_below_2hz")
    if max_sample_gap_s > FORMAL_MAX_SAMPLE_GAP_SECONDS:
        failures.append("sample_gap_exceeds_0.5s")
    return {
        "sample_count": len(samples),
        "span_s": span_s,
        "effective_sample_rate_hz": effective_sample_rate_hz,
        "max_sample_gap_s": max_sample_gap_s,
        "formal_sampling_ok": not failures,
        "sampling_failures": failures,
    }


def run_scheduled_action(
    *,
    args: argparse.Namespace,
    action_kind: str,
    status_observer: Callable[[Any], None] | None = None,
    live_status_poller: "LoadStatusPoller | None" = None,
    before_verify: Callable[[], None] | None = None,
    bridge_lease: dict[str, Any] | None = None,
) -> dict[str, Any]:
    if action_kind == "cc_target":
        return load_cc(
            args,
            args.load_device,
            args.target_ma,
            min_v_mv=args.load_min_v_mv,
            max_i_ma_total=args.max_i_ma_total,
            max_p_mw=args.max_p_mw,
            timeout_sec=args.command_timeout_sec,
            status_timeout_sec=args.status_timeout_sec,
            verify_timeout_sec=args.verify_timeout_sec,
            status_observer=status_observer,
            live_status_poller=live_status_poller,
            before_verify=before_verify,
            bridge_lease=bridge_lease,
            allow_command_ack_shortcut=True,
        )
    if action_kind == "port_c_disable_for_backup":
        return set_port_c_power(args.isolapurr_url, False)
    if action_kind == "port_c_enable_after_backup":
        return set_port_c_power(args.isolapurr_url, True)
    if action_kind == "disable_after_target":
        return disable_load(
            args,
            args.load_device,
            timeout_sec=args.command_timeout_sec,
            status_timeout_sec=args.status_timeout_sec,
            verify_timeout_sec=args.verify_timeout_sec,
            status_observer=status_observer,
            assume_enabled=True,
            allow_command_ack_shortcut=True,
            live_status_poller=live_status_poller,
            before_verify=before_verify,
            bridge_lease=bridge_lease,
        )
    raise RuntimeError(f"unknown scheduled action: {action_kind}")


def execute_continuous_scene(
    *,
    args: argparse.Namespace,
    jsonl_path: Path,
    metadata: dict[str, Any],
    actions: list[dict[str, Any]],
    load_status_poller: LoadStatusPoller,
    ups_status_poller: JsonPoller,
    power_diag_poller: JsonPoller,
    settings_snapshot: dict[str, Any],
    isolapurr_poller: JsonPoller,
    expected_phases: list[str],
    run_dir: Path,
) -> list[dict[str, Any]]:
    samples: list[dict[str, Any]] = []
    started_at = time.monotonic()
    next_sample_at = started_at
    state = "pre"
    state_started_at = started_at
    active_action: dict[str, Any] | None = None
    terminal = False

    def action_uses_loadlynx(action_kind: str) -> bool:
        return action_kind in {"cc_target", "disable_after_target"}

    def refresh_load_status_before_stable_phase(
        *,
        transition_phase: str,
        require_new_generation: bool,
    ) -> dict[str, Any]:
        return wait_for_live_load_status(
            load_status_poller,
            sample_interval_seconds=args.sample_interval_seconds,
            timeout_sec=max(args.load_status_poll_timeout_sec, 3.0),
            require_new_generation=require_new_generation,
            progress_hook=lambda current_now: (
                capture_sample_at(current_now, transition_phase)
                if current_now >= next_sample_at
                else None
            ),
        )

    def capture_sample_at(now_monotonic: float, phase_name: str) -> None:
        nonlocal next_sample_at
        sample = capture_three_device_sample(
            phase=phase_name,
            t_s=now_monotonic - started_at,
            load_device=args.load_device,
            ups_status_url=args.ups_status_url,
            ups_settings_url=args.ups_settings_url,
            devd_power_diag_url=args.devd_power_diag_url,
            isolapurr_url=args.isolapurr_url,
            status_timeout_sec=args.status_timeout_sec,
            load_status_snapshot=load_status_poller.snapshot(now_monotonic),
            ups_status_snapshot=ups_status_poller.snapshot(now_monotonic),
            power_diag_snapshot=power_diag_poller.snapshot(now_monotonic),
            settings_snapshot=settings_snapshot,
            isolapurr_snapshot=isolapurr_poller.snapshot(now_monotonic),
        )
        samples.append(sample)
        append_jsonl(jsonl_path, sample)
        next_sample_at += args.sample_interval_seconds
        now_after_sample = time.monotonic()
        if next_sample_at < now_after_sample:
            skipped_intervals = int((now_after_sample - next_sample_at) / args.sample_interval_seconds) + 1
            next_sample_at += skipped_intervals * args.sample_interval_seconds

    def capture_fresh_ups_sample_at(now_monotonic: float, phase_name: str) -> None:
        nonlocal next_sample_at
        ups_device_id = devd_device_id_from_endpoint(args.devd_power_diag_url)
        ups_snapshot = ups_status_poller.snapshot(now_monotonic)
        diag_snapshot = power_diag_poller.snapshot(now_monotonic)
        lan_address = None
        for snapshot in (ups_snapshot, diag_snapshot):
            lan_address = lan_address_from_devd_listing_snapshot(
                snapshot,
                device_id=ups_device_id,
            )
            if lan_address:
                break
        fresh_status = None
        if lan_address:
            try:
                fresh_status = http_json_with_retries(
                    f"http://{lan_address}/api/v1/status",
                    timeout_sec=min(args.status_timeout_sec, 2.0),
                    retries=1,
                    retry_delay_sec=0.0,
                )
            except Exception:
                fresh_status = None
        sample = capture_three_device_sample(
            phase=phase_name,
            t_s=now_monotonic - started_at,
            load_device=args.load_device,
            ups_status_url=args.ups_status_url,
            ups_settings_url=args.ups_settings_url,
            devd_power_diag_url=args.devd_power_diag_url,
            isolapurr_url=args.isolapurr_url,
            status_timeout_sec=args.status_timeout_sec,
            load_status_snapshot=load_status_poller.snapshot(now_monotonic),
            ups_status_snapshot={
                **ups_snapshot,
                "payload": fresh_status or ups_snapshot.get("payload"),
            },
            power_diag_snapshot=diag_snapshot,
            settings_snapshot=settings_snapshot,
            isolapurr_snapshot=isolapurr_poller.snapshot(now_monotonic),
        )
        samples.append(sample)
        append_jsonl(jsonl_path, sample)
        next_sample_at += args.sample_interval_seconds

    def start_action(action_kind: str, transition_phase: str, next_stable_phase: str) -> dict[str, Any]:
        holder: dict[str, Any] = {}
        action_started_at = time.monotonic()
        bridge_control_lease = None
        if effective_load_bridge_url(args) and action_uses_loadlynx(action_kind):
            bridge_control_lease = load_status_poller.wait_for_bridge_lease(
                timeout_sec=max(args.load_status_poll_timeout_sec, 3.0)
            )
        should_pause_load = (
            action_uses_loadlynx(action_kind)
            and not effective_load_bridge_url(args)
            and getattr(args, "load_status_source", "poll") != "status-stream"
        )

        def worker() -> None:
            if should_pause_load:
                load_status_poller.pause()
                load_status_poller.wait_until_idle(
                    timeout_sec=max(
                        args.load_status_poll_timeout_sec + 0.5,
                        3.5,
                    )
                )
                load_status_poller.release_bridge_lease(
                    timeout_sec=min(args.status_timeout_sec, 5.0)
                )
                load_status_poller.release_load_devd_lease(
                    timeout_sec=min(args.status_timeout_sec, 5.0)
                )
            try:
                holder["result"] = run_scheduled_action(
                    args=args,
                    action_kind=action_kind,
                    status_observer=load_status_poller.replace_status,
                    # Keep released USB control strictly serial while the background
                    # poller is paused; otherwise command verification can race the
                    # same owner path and turn a successful load command into a fake
                    # timeout.
                    live_status_poller=None if should_pause_load else load_status_poller,
                    before_verify=None,
                    bridge_lease=bridge_control_lease,
                )
            except Exception as exc:  # noqa: BLE001
                holder["error"] = repr(exc)
            finally:
                if should_pause_load:
                    load_status_poller.resume()
                holder["finished_at_monotonic"] = time.monotonic()

        thread = threading.Thread(
            target=worker,
            name=f"hil-scene-action:{action_kind}",
            daemon=True,
        )
        thread.start()
        return {
            "kind": action_kind,
            "transition_phase": transition_phase,
            "next_stable_phase": next_stable_phase,
            "started_at_monotonic": action_started_at,
            "pause_load_sampling": should_pause_load,
            "bridge_control_lease": bridge_control_lease,
            "timeout_sec": (
                max(
                    getattr(args, "command_timeout_sec", DEFAULT_COMMAND_TIMEOUT_SECONDS),
                    getattr(args, "verify_timeout_sec", DEFAULT_VERIFY_TIMEOUT_SECONDS),
                    getattr(args, "status_timeout_sec", DEFAULT_STATUS_TIMEOUT_SECONDS),
                    getattr(
                        args,
                        "load_status_poll_timeout_sec",
                        DEFAULT_LOAD_STATUS_POLL_TIMEOUT_SECONDS,
                    ),
                )
                + SCHEDULED_ACTION_TIMEOUT_MARGIN_SECONDS
            ),
            "thread": thread,
            "holder": holder,
        }

    def should_defer_transition_sample(
        now_monotonic: float,
        phase_name: str,
    ) -> bool:
        if active_action is None:
            return False
        if phase_name != active_action.get("transition_phase"):
            return False
        if active_action.get("pause_load_sampling") is not True:
            return False
        thread = active_action.get("thread")
        if thread is None or not thread.is_alive():
            return False
        load_snapshot = load_status_poller.snapshot(now_monotonic)
        load_age_s = load_snapshot.get("age_s")
        return not (
            isinstance(load_age_s, (int, float))
            and float(load_age_s) <= FORMAL_MAX_REALTIME_SAMPLE_AGE_SECONDS
        )

    while True:
        now = time.monotonic()
        elapsed_s = now - started_at

        if active_action is not None:
            thread = active_action["thread"]
            action_timeout_sec = active_action.get("timeout_sec")
            if (
                thread.is_alive()
                and isinstance(action_timeout_sec, (int, float))
                and now - active_action["started_at_monotonic"] > float(action_timeout_sec)
            ):
                if active_action.get("pause_load_sampling") is True:
                    load_status_poller.resume()
                raise RuntimeError(
                    "scheduled_action_timeout "
                    f"kind={active_action['kind']} timeout_sec={float(action_timeout_sec):.1f}"
                )
            if not thread.is_alive():
                holder = active_action["holder"]
                if "error" in holder:
                    raise RuntimeError(
                        f"scheduled_action_failed kind={active_action['kind']} error={holder['error']}"
                    )
                result = holder.get("result") or {}
                action_started_elapsed_s = active_action["started_at_monotonic"] - started_at
                action_finished_at = holder.get("finished_at_monotonic") or time.monotonic()
                action_finished_elapsed_s = action_finished_at - started_at
                verified_status = result.get("verified_status")
                verified_status_source = (
                    verified_status.get("source")
                    if isinstance(verified_status, dict)
                    and isinstance(verified_status.get("source"), str)
                    else None
                )
                if verified_status is not None and verified_status_source != "command_ack_synthetic_status":
                    load_status_poller.replace_status(verified_status)
                if action_uses_loadlynx(active_action["kind"]):
                    load_status_ready = refresh_load_status_before_stable_phase(
                        transition_phase=active_action["transition_phase"],
                        require_new_generation=verified_status_source
                        == "command_ack_synthetic_status",
                    )
                    result["load_status_ready"] = load_status_ready
                    action_finished_at = time.monotonic()
                    action_finished_elapsed_s = action_finished_at - started_at
                capture_sample_at(time.monotonic(), active_action["transition_phase"])
                if active_action["kind"] == "port_c_disable_for_backup":
                    isolapurr_ready = wait_for_isolapurr_port_c_state(
                        isolapurr_poller,
                        expected_enabled=False,
                        sample_interval_seconds=args.sample_interval_seconds,
                        timeout_sec=max(args.status_timeout_sec, 5.0),
                        progress_hook=lambda current_now: (
                            capture_sample_at(current_now, active_action["transition_phase"])
                            if current_now >= next_sample_at
                            else None
                        ),
                    )
                    result["isolapurr_ready"] = isolapurr_ready
                    result["load_status_ready"] = refresh_load_status_before_stable_phase(
                        transition_phase=active_action["transition_phase"],
                        require_new_generation=False,
                    )
                    capture_fresh_ups_sample_at(
                        time.monotonic(),
                        active_action["transition_phase"],
                    )
                    action_finished_at = time.monotonic()
                    action_finished_elapsed_s = action_finished_at - started_at
                elif active_action["kind"] == "port_c_enable_after_backup":
                    isolapurr_ready = wait_for_isolapurr_port_c_state(
                        isolapurr_poller,
                        expected_enabled=True,
                        sample_interval_seconds=args.sample_interval_seconds,
                        timeout_sec=max(args.status_timeout_sec, 5.0),
                        progress_hook=lambda current_now: (
                            capture_sample_at(current_now, active_action["transition_phase"])
                            if current_now >= next_sample_at
                            else None
                        ),
                    )
                    result["isolapurr_ready"] = isolapurr_ready
                    result["load_status_ready"] = refresh_load_status_before_stable_phase(
                        transition_phase=active_action["transition_phase"],
                        require_new_generation=False,
                    )
                    capture_fresh_ups_sample_at(
                        time.monotonic(),
                        active_action["transition_phase"],
                    )
                    action_finished_at = time.monotonic()
                    action_finished_elapsed_s = action_finished_at - started_at
                actions.append(
                    {
                        active_action["kind"]: result,
                        "event": {
                            "kind": active_action["kind"],
                            "started_at_s": round(action_started_elapsed_s, 3),
                            "finished_at_s": round(action_finished_elapsed_s, 3),
                            "phase_before": active_action["transition_phase"],
                            "phase_after": active_action["next_stable_phase"],
                        },
                    }
                )
                persist_progress(
                    run_dir,
                    metadata,
                    actions,
                    samples,
                    expected_phases=expected_phases,
                )
                state = active_action["next_stable_phase"]
                state_started_at = action_finished_at
                active_action = None
                now = time.monotonic()
                elapsed_s = now - started_at

        if active_action is None:
            stable_elapsed_s = now - state_started_at
            if state == "pre" and stable_elapsed_s >= args.pre_seconds:
                state = "transition_hold"
                active_action = start_action("cc_target", "transition_hold", "hold")
            elif state == "hold" and stable_elapsed_s >= args.hold_seconds:
                if args.include_backup:
                    state = "transition_backup"
                    active_action = start_action(
                        "port_c_disable_for_backup",
                        "transition_backup",
                        "backup",
                    )
                else:
                    state = "transition_post"
                    active_action = start_action(
                        "disable_after_target",
                        "transition_post",
                        "post",
                    )
            elif state == "backup" and stable_elapsed_s >= args.backup_hold_seconds:
                state = "transition_restore"
                active_action = start_action(
                    "port_c_enable_after_backup",
                    "transition_restore",
                    "restore",
                )
            elif state == "restore" and stable_elapsed_s >= args.restore_hold_seconds:
                state = "transition_post"
                active_action = start_action(
                    "disable_after_target",
                    "transition_post",
                    "post",
                )
            elif state == "post" and stable_elapsed_s >= args.post_seconds:
                terminal = True

        if terminal and active_action is None:
            break

        if should_defer_transition_sample(now, state):
            time.sleep(0.02)
            continue

        if now < next_sample_at:
            time.sleep(min(0.02, next_sample_at - now))
            continue

        capture_sample_at(now, state)
    return samples


def summarize_numeric(samples: list[dict[str, Any]], key: str) -> dict[str, Any] | None:
    keyed_samples = [sample for sample in samples if isinstance(sample.get(key), (int, float))]
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
            "phase": min_sample.get("phase"),
            "mode": min_sample.get("mode"),
            "stage": min_sample.get("stage"),
        },
        "max_sample": {
            "t_s": max_sample.get("t_s"),
            "captured_at_utc": max_sample.get("captured_at_utc"),
            "phase": max_sample.get("phase"),
            "mode": max_sample.get("mode"),
            "stage": max_sample.get("stage"),
        },
    }


def summarize_gap(samples: list[dict[str, Any]], lhs_key: str, rhs_key: str) -> dict[str, Any] | None:
    gaps: list[tuple[float | int, dict[str, Any]]] = []
    for sample in samples:
        lhs = sample.get(lhs_key)
        rhs = sample.get(rhs_key)
        if isinstance(lhs, (int, float)) and isinstance(rhs, (int, float)):
            gaps.append((abs(lhs - rhs), sample))
    if not gaps:
        return None
    maximum, max_sample = max(gaps, key=lambda item: item[0])
    minimum = min(gap for gap, _ in gaps)
    return {
        "min": minimum,
        "max": maximum,
        "span": maximum - minimum,
        "count": len(gaps),
        "max_sample": {
            "t_s": max_sample.get("t_s"),
            "captured_at_utc": max_sample.get("captured_at_utc"),
            "phase": max_sample.get("phase"),
            "mode": max_sample.get("mode"),
            "stage": max_sample.get("stage"),
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
        "phase": sample.get("phase"),
        "mode": sample.get("mode"),
        "stage": sample.get("stage"),
    }


def summarize_source_cut_semantics(
    samples: list[dict[str, Any]],
    *,
    expected_phases: list[str] | None = None,
) -> dict[str, Any]:
    backup_required = bool(expected_phases) and "backup" in (expected_phases or [])
    if not backup_required:
        return {
            "required": False,
            "cut_sample_count": 0,
            "mains_loss_observed": None,
            "backup_mode_observed": None,
            "backup_stage_observed": None,
            "vin_changed_with_source_cut": None,
            "vin_cut_delta_mv": None,
            "source_cut_state_observed": None,
            "source_cut_observed": None,
            "failures": [],
        }
    cut_phase_names = {"transition_backup", "backup", "transition_restore"}
    cut_samples = [
        sample
        for sample in samples
        if sample.get("phase") in cut_phase_names and sample.get("port_c_enabled") is False
    ]
    if not cut_samples:
        return {
            "required": True,
            "cut_sample_count": 0,
            "mains_loss_observed": False,
            "backup_mode_observed": False,
            "backup_stage_observed": False,
            "vin_changed_with_source_cut": False,
            "vin_cut_delta_mv": None,
            "source_cut_state_observed": False,
            "source_cut_observed": False,
            "failures": ["missing_source_cut_samples"],
        }
    mains_loss_observed = any(sample.get("mains_present") is False for sample in cut_samples)
    backup_mode_observed = any(sample.get("mode") == "backup" for sample in cut_samples)
    backup_stage_observed = any(
        sample.get("stage") == "backup" or sample.get("diag_stage") == "backup"
        for sample in cut_samples
    )
    source_cut_state_observed = (
        mains_loss_observed or backup_mode_observed or backup_stage_observed
    )
    online_vins = [
        float(sample.get("vin_vbus_mv"))
        for sample in samples
        if sample.get("port_c_enabled") is True
        and isinstance(sample.get("vin_vbus_mv"), (int, float))
    ]
    cut_vins = [
        float(sample.get("vin_vbus_mv"))
        for sample in cut_samples
        if isinstance(sample.get("vin_vbus_mv"), (int, float))
    ]
    vin_cut_delta_mv = None
    vin_changed_with_source_cut = False
    if online_vins and cut_vins:
        vin_cut_delta_mv = round(max(online_vins) - min(cut_vins), 3)
        vin_changed_with_source_cut = vin_cut_delta_mv >= FORMAL_MIN_SOURCE_CUT_VIN_DELTA_MV
    failures: list[str] = []
    if not source_cut_state_observed:
        failures.append("source_cut_not_observed_in_ups_state")
    if not vin_changed_with_source_cut:
        failures.append("vin_not_correlated_with_source_cut")
    return {
        "required": True,
        "cut_sample_count": len(cut_samples),
        "mains_loss_observed": mains_loss_observed,
        "backup_mode_observed": backup_mode_observed,
        "backup_stage_observed": backup_stage_observed,
        "vin_changed_with_source_cut": vin_changed_with_source_cut,
        "vin_cut_delta_mv": vin_cut_delta_mv,
        "source_cut_state_observed": source_cut_state_observed,
        "source_cut_observed": source_cut_state_observed and vin_changed_with_source_cut,
        "failures": failures,
    }


def all_samples_match(
    samples: list[dict[str, Any]],
    key: str,
    predicate,
) -> bool:
    return all(predicate(sample.get(key)) for sample in samples)


def any_samples_match(
    samples: list[dict[str, Any]],
    key: str,
    predicate,
) -> bool:
    return any(predicate(sample.get(key)) for sample in samples)


def predicate_matches_sample(predicate, sample: dict[str, Any], key: str) -> bool:
    value = sample.get(key)
    try:
        return bool(predicate(value, sample))
    except TypeError:
        return bool(predicate(value))


def isolapurr_sample_has_expected_cut_state(sample: dict[str, Any]) -> bool:
    if sample.get("port_c_enabled") is not False:
        return False
    raw = dict_or_empty((sample.get("raw") or {}).get("isolapurr_power"))
    port_c = port_state(raw, port_id="port_c")
    telemetry = dict_or_empty(port_c.get("telemetry"))
    return telemetry.get("status") == "not_inserted"


def isolapurr_sample_has_expected_busy_transition_state(sample: dict[str, Any]) -> bool:
    phase = sample.get("phase")
    if not isinstance(phase, str) or not phase.startswith("transition_"):
        return False
    raw = dict_or_empty((sample.get("raw") or {}).get("isolapurr_power"))
    port_c = port_state(raw, port_id="port_c")
    state = dict_or_empty(port_c.get("state"))
    return state.get("busy") is True


def isolapurr_voltage_present_or_expected_cut(sample: dict[str, Any]) -> bool:
    value = sample.get("isolapurr_port_c_mv")
    if isinstance(value, (int, float)):
        return True
    if isolapurr_sample_has_expected_busy_transition_state(sample):
        return True
    return isolapurr_sample_has_expected_cut_state(sample)


def isolapurr_current_present_or_expected_cut(sample: dict[str, Any]) -> bool:
    value = sample.get("isolapurr_port_c_ma")
    if isinstance(value, (int, float)):
        return True
    if isolapurr_sample_has_expected_busy_transition_state(sample):
        return True
    return isolapurr_sample_has_expected_cut_state(sample)


def evaluate_group_completeness(group_samples: list[dict[str, Any]]) -> dict[str, Any]:
    failures: list[str] = []
    surfaces = {
        "ups_status": all(isinstance((sample.get("raw") or {}).get("ups_status"), dict) for sample in group_samples),
        "power_diag": all(isinstance((sample.get("raw") or {}).get("power_diag"), dict) for sample in group_samples),
        "isolapurr_power": all(isinstance((sample.get("raw") or {}).get("isolapurr_power"), dict) for sample in group_samples),
        "load_control": all(isinstance((sample.get("raw") or {}).get("load_control"), dict) for sample in group_samples),
        "load_status": all(isinstance((sample.get("raw") or {}).get("load_status"), dict) for sample in group_samples),
    }
    if not surfaces["ups_status"]:
        failures.append("missing_ups_status")
    if not surfaces["power_diag"]:
        failures.append("missing_power_diag")
    if not surfaces["isolapurr_power"]:
        failures.append("missing_isolapurr_power")
    if not surfaces["load_control"]:
        failures.append("missing_load_control")
    if not surfaces["load_status"]:
        failures.append("missing_load_status")
    mains_online_samples = [sample for sample in group_samples if sample.get("mains_present") is True]
    online_source_samples = [
        sample
        for sample in mains_online_samples
        if sample.get("port_c_enabled") is not False
    ]
    field_checks = (
        ("port_c_enabled", lambda value: isinstance(value, bool), "missing_port_c_enabled", all_samples_match),
        (
            "isolapurr_port_c_mv",
            lambda _value, sample=None: True if sample is None else isolapurr_voltage_present_or_expected_cut(sample),
            "missing_isolapurr_voltage_series",
            all_samples_match,
        ),
        (
            "isolapurr_port_c_ma",
            lambda _value, sample=None: True if sample is None else isolapurr_current_present_or_expected_cut(sample),
            "missing_isolapurr_current_series",
            all_samples_match,
        ),
        ("mode", lambda value: isinstance(value, str) and value != "", "missing_mode_series", all_samples_match),
        (
            "mains_present",
            lambda value: isinstance(value, bool),
            "missing_mains_present_series",
            all_samples_match,
        ),
        ("stage", lambda value: isinstance(value, str) and value != "", "missing_assist_stage_series", all_samples_match),
        (
            "assist_target_vout_mv",
            lambda value: isinstance(value, (int, float)),
            "missing_assist_target_vout_series",
            all_samples_match,
        ),
        ("vin_vbus_mv", lambda value: isinstance(value, (int, float)), "missing_vin_voltage_series", all_samples_match),
        ("vin_iin_ma", lambda value: isinstance(value, (int, float)), "missing_vin_current_series", all_samples_match),
        (
            "tps_total_iout_ma",
            lambda value: isinstance(value, (int, float)),
            "missing_tps_total_iout_series",
            all_samples_match,
        ),
        (
            "battery_current_ma",
            lambda value: isinstance(value, (int, float)),
            "missing_battery_current_series",
            all_samples_match,
        ),
        (
            "charger_allow_charge",
            lambda value: isinstance(value, bool),
            "missing_charger_allow_charge_series",
            all_samples_match,
        ),
        (
            "charger_detail_status",
            lambda value: isinstance(value, str) and value != "",
            "missing_charger_detail_status_series",
            all_samples_match,
        ),
        ("diag_stage", lambda value: isinstance(value, str) and value != "", "missing_diag_stage_series", all_samples_match),
        (
            "diag_assist_target_vout_mv",
            lambda value: isinstance(value, (int, float)),
            "missing_diag_assist_target_vout_series",
            all_samples_match,
        ),
        (
            "diag_vin_baseline_mv",
            lambda value: isinstance(value, (int, float)),
            "missing_diag_vin_baseline_series",
            all_samples_match,
        ),
        (
            "diag_tps_total_iout_ma",
            lambda value: isinstance(value, (int, float)),
            "missing_diag_tps_total_iout_series",
            all_samples_match,
        ),
        (
            "ups_vout_mv",
            lambda value: isinstance(value, (int, float)),
            "missing_ups_output_voltage_series",
            all_samples_match,
        ),
        (
            "load_output_enabled",
            lambda value: isinstance(value, bool),
            "missing_load_output_enabled_series",
            all_samples_match,
        ),
        (
            "load_v_local_mv",
            lambda value: isinstance(value, (int, float)),
            "missing_load_v_local_series",
            all_samples_match,
        ),
        (
            "load_v_remote_mv",
            lambda value: isinstance(value, (int, float)),
            "missing_load_v_remote_series",
            all_samples_match,
        ),
        (
            "load_i_local_ma",
            lambda value: isinstance(value, (int, float)),
            "missing_load_i_local_series",
            all_samples_match,
        ),
        (
            "load_i_remote_ma",
            lambda value: isinstance(value, (int, float)),
            "missing_load_i_remote_series",
            all_samples_match,
        ),
        (
            "load_i_total_ma",
            lambda value: isinstance(value, (int, float)),
            "missing_load_i_total_series",
            all_samples_match,
        ),
        (
            "load_calc_p_mw",
            lambda value: isinstance(value, (int, float)),
            "missing_load_power_series",
            all_samples_match,
        ),
        (
            "load_v_local_mv",
            lambda value: isinstance(value, (int, float)),
            "missing_load_voltage_series",
            all_samples_match,
        ),
    )
    for key, predicate, failure, matcher in field_checks:
        if matcher is all_samples_match:
            ok = all(predicate_matches_sample(predicate, sample, key) for sample in group_samples)
        elif matcher is any_samples_match:
            ok = any(predicate_matches_sample(predicate, sample, key) for sample in group_samples)
        else:
            ok = matcher(group_samples, key, predicate)
        if not ok:
            failures.append(failure)
    if online_source_samples and not any(
        "vin_drop_mv" in dict_or_empty(dict_or_empty((sample.get("raw") or {}).get("power_diag")).get("input"))
        for sample in online_source_samples
    ):
        failures.append("missing_diag_vin_drop_series")
    generations = sorted(
        {
            int(sample.get("load_status_generation"))
            for sample in group_samples
            if isinstance(sample.get("load_status_generation"), int)
        }
    )
    freshness_threshold_s = FORMAL_MAX_REALTIME_SAMPLE_AGE_SECONDS
    ages = [
        float(sample.get("load_status_sample_age_s"))
        if isinstance(sample.get("load_status_sample_age_s"), (int, float))
        else float(sample.get("load_status_age_s"))
        for sample in group_samples
        if isinstance(sample.get("load_status_sample_age_s"), (int, float))
        or isinstance(sample.get("load_status_age_s"), (int, float))
    ]
    freshness_visible = bool(ages) and all(age <= freshness_threshold_s for age in ages)
    source_ages = [
        float(
            dict_or_empty(sample.get("sample_age_s")).get(
                "isolapurr_power",
                dict_or_empty(sample.get("fetch_age_s")).get("isolapurr_power"),
            )
        )
        for sample in group_samples
        if isinstance(
            dict_or_empty(sample.get("sample_age_s")).get(
                "isolapurr_power",
                dict_or_empty(sample.get("fetch_age_s")).get("isolapurr_power"),
            ),
            (int, float),
        )
    ]
    ups_status_ages = [
        float(
            dict_or_empty(sample.get("sample_age_s")).get(
                "ups_status",
                dict_or_empty(sample.get("fetch_age_s")).get("ups_status"),
            )
        )
        for sample in group_samples
        if isinstance(
            dict_or_empty(sample.get("sample_age_s")).get(
                "ups_status",
                dict_or_empty(sample.get("fetch_age_s")).get("ups_status"),
            ),
            (int, float),
        )
    ]
    power_diag_ages = [
        float(
            dict_or_empty(sample.get("sample_age_s")).get(
                "power_diag",
                dict_or_empty(sample.get("fetch_age_s")).get("power_diag"),
            )
        )
        for sample in group_samples
        if isinstance(
            dict_or_empty(sample.get("sample_age_s")).get(
                "power_diag",
                dict_or_empty(sample.get("fetch_age_s")).get("power_diag"),
            ),
            (int, float),
        )
    ]
    source_fresh = bool(source_ages) and all(age <= FORMAL_MAX_REALTIME_SAMPLE_AGE_SECONDS for age in source_ages)
    ups_status_fresh = bool(ups_status_ages) and all(
        age <= FORMAL_MAX_REALTIME_SAMPLE_AGE_SECONDS for age in ups_status_ages
    )
    power_diag_fresh = bool(power_diag_ages) and all(
        age <= FORMAL_MAX_REALTIME_SAMPLE_AGE_SECONDS for age in power_diag_ages
    )
    sampling_metrics = build_formal_sampling_metrics(group_samples)
    failures.extend(sampling_metrics.get("sampling_failures") or [])
    return {
        "scene_complete": not failures,
        "failures": failures,
        "load_status_generations": generations,
        "load_status_generation_count": len(generations),
        "load_freshness_visible": freshness_visible,
        "load_status_max_age_s": max(ages, default=None),
        "source_status_fresh": source_fresh,
        "source_status_max_age_s": max(source_ages, default=None),
        "ups_status_fresh": ups_status_fresh,
        "ups_status_max_age_s": max(ups_status_ages, default=None),
        "power_diag_fresh": power_diag_fresh,
        "power_diag_max_age_s": max(power_diag_ages, default=None),
        **sampling_metrics,
        **surfaces,
    }


def build_preflight(
    args: argparse.Namespace,
    identity_payload: Any,
    settings_payload: Any,
    *,
    known_load_disabled: bool = False,
    known_load_target_i_ma: int | None = None,
    load_telemetry_probe: dict[str, Any] | None = None,
    seeded_ups_status: Any | None = None,
    seeded_power_diag: Any | None = None,
) -> dict[str, Any]:
    isolapurr_ports = fetch_isolapurr_ports(
        args.isolapurr_url,
        timeout_sec=min(args.status_timeout_sec, 5.0),
        isolapurr_cli=args.isolapurr_cli,
    )
    port_c_entry = dict_or_empty(port_state(isolapurr_ports, port_id="port_c"))
    port_c_telemetry = dict_or_empty(port_c_entry.get("telemetry"))
    port_c_state = dict_or_empty(port_c_entry.get("state"))
    load_status = get_load_status_best_effort(
        args, args.load_device, timeout_sec=args.status_timeout_sec
    )
    load_control = get_load_control_best_effort(
        args, args.load_device, timeout_sec=args.status_timeout_sec
    )
    ups_status_error = None
    try:
        ups_status = http_json_with_retries(
            args.ups_status_url,
            timeout_sec=min(args.status_timeout_sec, 5.0),
        )
        ups_status_source = "direct_http"
    except Exception as exc:  # noqa: BLE001
        ups_status_error = repr(exc)
        ups_status = seeded_ups_status
        ups_status_source = "seeded_refresh_devd_devices" if isinstance(ups_status, dict) else None
    power_diag, power_diag_source, power_diag_error = fetch_power_diag_with_trace_fallback(
        args,
        timeout_sec=min(args.status_timeout_sec, 5.0),
        seeded_power_diag=seeded_power_diag,
    )
    effective_enabled = load_output_enabled(normalize_verified_load_payload(load_status))
    effective_target_i_ma = load_target_i_ma(normalize_verified_load_payload(load_status))
    if effective_enabled is None or effective_target_i_ma is None:
        effective_enabled, effective_target_i_ma = select_effective_load_state(load_control, load_status)
    probe_enabled, probe_target_i_ma = probe_effective_load_state(load_telemetry_probe)
    if effective_enabled is None and effective_target_i_ma is None:
        effective_enabled, effective_target_i_ma = probe_enabled, probe_target_i_ma
    elif effective_enabled is None and probe_enabled is not None:
        effective_enabled = probe_enabled
    elif effective_target_i_ma is None and probe_target_i_ma is not None:
        effective_target_i_ma = probe_target_i_ma
    if known_load_disabled:
        effective_enabled = False
        if effective_target_i_ma is None:
            effective_target_i_ma = known_load_target_i_ma
    gate_failures: list[str] = []
    if not isinstance(port_c_telemetry.get("voltage_mv"), int):
        gate_failures.append("isolapurr_port_c_voltage_missing")
    if port_c_state.get("power_enabled") is not True:
        gate_failures.append("isolapurr_port_c_not_enabled")
    if not isinstance(ups_status, dict):
        gate_failures.append("ups_status_unavailable")
    if not isinstance(power_diag, dict):
        gate_failures.append("power_diag_unavailable")
    if not isinstance(identity_payload, dict):
        gate_failures.append("ups_identity_unavailable")
    if not isinstance(settings_payload, dict):
        gate_failures.append("ups_settings_unavailable")
    hardware_validation = validate_ups_hardware_capabilities(
        expected_output_profile=args.output_profile,
        expected_source_voltage_mv=args.source_voltage_mv,
        identity_payload=identity_payload,
        settings_payload=settings_payload,
    )
    gate_failures.extend(hardware_validation.get("failures") or [])
    if effective_enabled is not False and not known_load_disabled:
        gate_failures.append("load_not_disabled_before_scene")
    load_live_poller_probe = None
    load_devd_socket = (getattr(args, "load_devd_socket", "") or "").strip()
    load_ipc = (getattr(args, "load_ipc", "") or "").strip()
    configured_load_socket = load_ipc or load_devd_socket
    if configured_load_socket and not Path(configured_load_socket).exists():
        gate_failures.append("load_ipc_socket_missing")
    if configured_load_socket:
        load_live_poller_probe = probe_live_load_status_poller_capability(args)
        if dict_or_empty(load_live_poller_probe).get("formal_capable") is not True:
            gate_failures.append("load_live_poller_not_formal_capable")
    elif isinstance(load_telemetry_probe, dict) and not load_telemetry_probe.get("skipped"):
        probe_verdict = dict_or_empty(load_telemetry_probe.get("verdict"))
        if probe_verdict.get("formal_capable") is not True:
            gate_failures.append("load_telemetry_not_formal_capable")
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
            "source": ups_status_source,
            "fetch_error": ups_status_error,
        },
        "identity": {
            "hardware_capabilities": extract_identity_hardware_capabilities(identity_payload),
        },
        "hardware_validation": hardware_validation,
        "power_diag": {
            "assist_power_stage": (
                (power_diag.get("input") or {}).get("assist_power_stage")
                if isinstance(power_diag, dict)
                else None
            ),
            "vin_vbus_mv": (
                (power_diag.get("input") or {}).get("vin_vbus_mv")
                if isinstance(power_diag, dict)
                else None
            ),
            "vin_iin_ma": (
                (power_diag.get("input") or {}).get("vin_iin_ma")
                if isinstance(power_diag, dict)
                else None
            ),
            "source": power_diag_source,
            "fetch_error": power_diag_error,
        },
        "load": {
            "output_enabled": effective_enabled,
            "target_i_ma": effective_target_i_ma,
            "status": load_status,
            "control": load_control,
        },
        "load_telemetry_probe": load_telemetry_probe,
        "load_live_poller_probe": load_live_poller_probe,
        "load_live_poller_mode": (
            dict_or_empty(load_live_poller_probe).get("effective_mode")
            if isinstance(load_live_poller_probe, dict)
            else None
        ),
    }


def summarize_samples(samples: list[dict[str, Any]], *, expected_phases: list[str] | None = None) -> dict[str, Any]:
    by_phase: dict[str, list[dict[str, Any]]] = {}
    for sample in samples:
        by_phase.setdefault(str(sample.get("phase")), []).append(sample)

    def summarize_group(group_samples: list[dict[str, Any]]) -> dict[str, Any]:
        stages = [sample.get("stage") for sample in group_samples if sample.get("stage") is not None]
        modes = [sample.get("mode") for sample in group_samples if sample.get("mode") is not None]
        out_a_summary = summarize_numeric(group_samples, "out_a_vbus_mv")
        out_b_summary = summarize_numeric(group_samples, "out_b_vbus_mv")
        load_v_summary = summarize_numeric(group_samples, "load_v_local_mv")
        return {
            "sample_count": len(group_samples),
            "t_start_s": group_samples[0].get("t_s") if group_samples else None,
            "t_end_s": group_samples[-1].get("t_s") if group_samples else None,
            "mode_set": sorted({mode for mode in modes if mode is not None}),
            "stage_set": sorted({stage for stage in stages if stage is not None}),
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
            "load_output_enabled_set": sorted(
                {
                    bool(sample.get("load_output_enabled"))
                    for sample in group_samples
                    if isinstance(sample.get("load_output_enabled"), bool)
                }
            ),
            "load_status_generations": sorted(
                {
                    int(sample.get("load_status_generation"))
                    for sample in group_samples
                    if isinstance(sample.get("load_status_generation"), int)
                }
            ),
            "load_status_generation_count": len(
                {
                    int(sample.get("load_status_generation"))
                    for sample in group_samples
                    if isinstance(sample.get("load_status_generation"), int)
                }
            ),
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
            "output_voltage_fluctuation": {
                "ups_vout_mv": summarize_numeric(group_samples, "ups_vout_mv"),
                "out_a_vbus_mv": out_a_summary,
                "out_b_vbus_mv": out_b_summary,
                "load_v_local_mv": load_v_summary,
                "out_a_out_b_gap_mv": summarize_gap(group_samples, "out_a_vbus_mv", "out_b_vbus_mv"),
                "minimum_observed_output_voltage": summarize_minimum_output_sample(group_samples),
            },
            "completeness": evaluate_group_completeness(group_samples),
        }

    overall = summarize_group(samples)
    by_phase_summary = {phase: summarize_group(group) for phase, group in by_phase.items()}
    if expected_phases:
        observed_phases = sorted(by_phase_summary.keys())
        expected_phase_summaries = [
            by_phase_summary[phase]
            for phase in expected_phases
            if phase in by_phase_summary
        ]

        def aggregate_all_bool(field: str) -> bool | None:
            values = [
                value
                for value in (
                    dict_or_empty(summary.get("completeness")).get(field)
                    for summary in expected_phase_summaries
                )
                if isinstance(value, bool)
            ]
            return all(values) if values else None

        def aggregate_max_number(field: str) -> float | int | None:
            values = [
                value
                for value in (
                    dict_or_empty(summary.get("completeness")).get(field)
                    for summary in expected_phase_summaries
                )
                if isinstance(value, (int, float))
            ]
            return max(values) if values else None

        def aggregate_min_number(field: str) -> float | int | None:
            values = [
                value
                for value in (
                    dict_or_empty(summary.get("completeness")).get(field)
                    for summary in expected_phase_summaries
                )
                if isinstance(value, (int, float))
            ]
            return min(values) if values else None

        failures: list[str] = []
        for phase in expected_phases:
            if phase not in by_phase_summary:
                failures.append(f"missing_phase_{phase}")
                continue
            phase_completeness = dict_or_empty(by_phase_summary[phase].get("completeness"))
            phase_failures = [
                failure
                for failure in phase_completeness.get("failures") or []
                if isinstance(failure, str)
            ]
            if phase_failures:
                failures.append(f"incomplete_phase_{phase}")
                failures.extend(phase_failures)
        source_cut_semantics = summarize_source_cut_semantics(
            samples,
            expected_phases=expected_phases,
        )
        failures.extend(source_cut_semantics.get("failures") or [])
        failures = sorted(dict.fromkeys(failures))
        completeness = {
            "expected_phases": expected_phases,
            "observed_phases": observed_phases,
            "failures": failures,
            "scene_complete": not failures,
            "load_freshness_visible": aggregate_all_bool("load_freshness_visible"),
            "load_status_max_age_s": aggregate_max_number("load_status_max_age_s"),
            "source_status_fresh": aggregate_all_bool("source_status_fresh"),
            "source_status_max_age_s": aggregate_max_number("source_status_max_age_s"),
            "ups_status_fresh": aggregate_all_bool("ups_status_fresh"),
            "ups_status_max_age_s": aggregate_max_number("ups_status_max_age_s"),
            "power_diag_fresh": aggregate_all_bool("power_diag_fresh"),
            "power_diag_max_age_s": aggregate_max_number("power_diag_max_age_s"),
            "effective_sample_rate_hz": aggregate_min_number("effective_sample_rate_hz"),
            "max_sample_gap_s": aggregate_max_number("max_sample_gap_s"),
            "ups_status": aggregate_all_bool("ups_status"),
            "power_diag": aggregate_all_bool("power_diag"),
            "isolapurr_power": aggregate_all_bool("isolapurr_power"),
            "load_control": aggregate_all_bool("load_control"),
            "load_status": aggregate_all_bool("load_status"),
            "source_cut_required": source_cut_semantics.get("required"),
            "source_cut_sample_count": source_cut_semantics.get("cut_sample_count"),
            "source_cut_state_observed": source_cut_semantics.get("source_cut_state_observed"),
            "source_cut_observed": source_cut_semantics.get("source_cut_observed"),
            "source_cut_mains_loss_observed": source_cut_semantics.get("mains_loss_observed"),
            "source_cut_backup_mode_observed": source_cut_semantics.get("backup_mode_observed"),
            "source_cut_backup_stage_observed": source_cut_semantics.get("backup_stage_observed"),
            "source_cut_vin_changed": source_cut_semantics.get("vin_changed_with_source_cut"),
            "source_cut_vin_delta_mv": source_cut_semantics.get("vin_cut_delta_mv"),
        }
        completeness["expected_phases"] = expected_phases
        overall["completeness"] = completeness
    overall["acceptance"] = build_signoff_acceptance(overall)
    return {
        "all": overall,
        "by_phase": by_phase_summary,
    }


def build_signoff_acceptance(overall: dict[str, Any]) -> dict[str, Any]:
    completeness = dict(overall.get("completeness") or {})
    failures = list(completeness.get("failures") or [])
    surface_keys = (
        "ups_status",
        "power_diag",
        "isolapurr_power",
        "load_control",
        "load_status",
    )
    surface_failures = [
        surface for surface in surface_keys if completeness.get(surface) is not True
    ]
    scene_structure_ok = not any(
        failure.startswith("missing_phase_") or failure.startswith("incomplete_phase_")
        for failure in failures
    )
    effective_sample_rate_hz = completeness.get("effective_sample_rate_hz")
    sample_rate_ok = (
        isinstance(effective_sample_rate_hz, (int, float))
        and effective_sample_rate_hz >= FORMAL_MIN_EFFECTIVE_SAMPLE_RATE_HZ
    )
    max_sample_gap_s = completeness.get("max_sample_gap_s")
    max_gap_ok = (
        isinstance(max_sample_gap_s, (int, float))
        and max_sample_gap_s <= FORMAL_MAX_SAMPLE_GAP_SECONDS
    )
    required_voltage_series = {
        "source_output_voltage": "missing_isolapurr_voltage_series" not in failures,
        "ups_dcin_voltage": "missing_vin_voltage_series" not in failures,
        "ups_output_voltage": "missing_ups_output_voltage_series" not in failures,
        "load_actual_voltage": (
            "missing_load_voltage_series" not in failures
            and "missing_load_v_local_series" not in failures
        ),
    }
    required_voltage_series_ok = all(required_voltage_series.values())
    load_status_fresh = bool(completeness.get("load_freshness_visible"))
    source_status_fresh = bool(completeness.get("source_status_fresh"))
    ups_status_fresh = bool(completeness.get("ups_status_fresh"))
    power_diag_fresh = bool(completeness.get("power_diag_fresh"))
    load_status_max_age_s = completeness.get("load_status_max_age_s")
    source_status_max_age_s = completeness.get("source_status_max_age_s")
    ups_status_max_age_s = completeness.get("ups_status_max_age_s")
    power_diag_max_age_s = completeness.get("power_diag_max_age_s")
    failed_acceptance_checks: list[str] = []
    if not bool(completeness.get("scene_complete")):
        failed_acceptance_checks.append("scene_incomplete")
    if not scene_structure_ok:
        failed_acceptance_checks.append("scene_structure_incomplete")
    if not sample_rate_ok:
        failed_acceptance_checks.append("sample_rate_below_2hz")
    if not max_gap_ok:
        failed_acceptance_checks.append("sample_gap_exceeds_0.5s")
    if not required_voltage_series.get("source_output_voltage"):
        failed_acceptance_checks.append("missing_source_output_voltage")
    if not required_voltage_series.get("ups_dcin_voltage"):
        failed_acceptance_checks.append("missing_ups_dcin_voltage")
    if not required_voltage_series.get("ups_output_voltage"):
        failed_acceptance_checks.append("missing_ups_output_voltage")
    if not required_voltage_series.get("load_actual_voltage"):
        failed_acceptance_checks.append("missing_load_actual_voltage")
    if surface_failures:
        failed_acceptance_checks.extend(
            [f"missing_surface:{surface}" for surface in surface_failures]
        )
    if not load_status_fresh:
        failed_acceptance_checks.append("load_status_stale")
    if not source_status_fresh:
        failed_acceptance_checks.append("source_status_stale")
    if not ups_status_fresh:
        failed_acceptance_checks.append("ups_status_stale")
    if not power_diag_fresh:
        failed_acceptance_checks.append("power_diag_stale")
    source_cut_required = bool(completeness.get("source_cut_required"))
    source_cut_state_observed = completeness.get("source_cut_state_observed")
    source_cut_vin_changed = completeness.get("source_cut_vin_changed")
    source_cut_observed = completeness.get("source_cut_observed")
    if source_cut_required and not source_cut_state_observed:
        failed_acceptance_checks.append("source_cut_not_observed_in_ups_state")
    if source_cut_required and not source_cut_vin_changed:
        failed_acceptance_checks.append("vin_not_correlated_with_source_cut")
    signoff_valid = (
        bool(completeness.get("scene_complete"))
        and scene_structure_ok
        and sample_rate_ok
        and max_gap_ok
        and required_voltage_series_ok
        and not surface_failures
        and load_status_fresh
        and source_status_fresh
        and ups_status_fresh
        and power_diag_fresh
        and (not source_cut_required or bool(source_cut_observed))
    )
    run_validity = "valid_for_signoff" if signoff_valid else "invalid_diagnostic_only"
    return {
        "run_validity": run_validity,
        "signoff_valid": signoff_valid,
        "required_sample_rate_hz": FORMAL_MIN_EFFECTIVE_SAMPLE_RATE_HZ,
        "target_sample_rate_hz": FORMAL_TARGET_SAMPLE_RATE_HZ,
        "required_max_sample_gap_s": FORMAL_MAX_SAMPLE_GAP_SECONDS,
        "required_max_realtime_age_s": FORMAL_MAX_REALTIME_SAMPLE_AGE_SECONDS,
        "scene_structure_complete": scene_structure_ok,
        "effective_sample_rate_hz": effective_sample_rate_hz,
        "effective_sample_rate_ok": sample_rate_ok,
        "max_sample_gap_s": max_sample_gap_s,
        "max_sample_gap_ok": max_gap_ok,
        "required_voltage_series": required_voltage_series,
        "required_voltage_series_ok": required_voltage_series_ok,
        "required_surfaces_complete": not surface_failures,
        "missing_surfaces": surface_failures,
        "load_status_fresh": load_status_fresh,
        "load_status_max_age_s": load_status_max_age_s,
        "source_status_fresh": source_status_fresh,
        "source_status_max_age_s": source_status_max_age_s,
        "ups_status_fresh": ups_status_fresh,
        "ups_status_max_age_s": ups_status_max_age_s,
        "power_diag_fresh": power_diag_fresh,
        "power_diag_max_age_s": power_diag_max_age_s,
        "source_cut_required": source_cut_required,
        "source_cut_state_observed": source_cut_state_observed,
        "source_cut_vin_changed": source_cut_vin_changed,
        "source_cut_observed": source_cut_observed,
        "source_cut_sample_count": completeness.get("source_cut_sample_count"),
        "source_cut_vin_delta_mv": completeness.get("source_cut_vin_delta_mv"),
        "failed_acceptance_checks": failed_acceptance_checks,
        "signoff_failures": failures,
    }


def build_console_summary(
    run_dir: Path,
    payload: dict[str, Any],
    *,
    success: bool,
) -> dict[str, Any]:
    summary = payload.get("summary") or {}
    overall = summary.get("all") or {}
    phases = summary.get("by_phase") or {}
    acceptance = overall.get("acceptance") or {}
    required_voltage_series = acceptance.get("required_voltage_series") or {}
    return {
        "success": success,
        "run_dir": str(run_dir),
        "profile_name": payload.get("metadata", {}).get("profile_name"),
        "target_ma": payload.get("metadata", {}).get("target_ma"),
        "run_validity": acceptance.get("run_validity"),
        "scene_complete": overall.get("completeness", {}).get("scene_complete"),
        "signoff_valid": acceptance.get("signoff_valid"),
        "failures": overall.get("completeness", {}).get("failures"),
        "failed_acceptance_checks": acceptance.get("failed_acceptance_checks"),
        "signoff_failures": acceptance.get("signoff_failures"),
        "required_thresholds": {
            "target_sample_rate_hz": acceptance.get("target_sample_rate_hz"),
            "minimum_effective_sample_rate_hz": acceptance.get("required_sample_rate_hz"),
            "maximum_sample_gap_s": acceptance.get("required_max_sample_gap_s"),
            "maximum_realtime_sample_age_s": acceptance.get("required_max_realtime_age_s"),
        },
        "scene_structure_complete": acceptance.get("scene_structure_complete"),
        "effective_sample_rate_hz": overall.get("completeness", {}).get("effective_sample_rate_hz"),
        "max_sample_gap_s": overall.get("completeness", {}).get("max_sample_gap_s"),
        "load_status_max_age_s": acceptance.get("load_status_max_age_s"),
        "source_status_max_age_s": acceptance.get("source_status_max_age_s"),
        "ups_status_max_age_s": acceptance.get("ups_status_max_age_s"),
        "power_diag_max_age_s": acceptance.get("power_diag_max_age_s"),
        "required_voltage_series": required_voltage_series,
        "phase_modes": {
            phase: info.get("mode_set")
            for phase, info in phases.items()
            if isinstance(info, dict)
        },
        "phase_stages": {
            phase: info.get("stage_set")
            for phase, info in phases.items()
            if isinstance(info, dict)
        },
        "minimum_observed_output_voltage": (
            overall.get("output_voltage_fluctuation", {}) or {}
        ).get("minimum_observed_output_voltage"),
        "error": payload.get("error"),
    }


def persist_progress(
    run_dir: Path,
    metadata: dict[str, Any],
    actions: list[dict[str, Any]],
    samples: list[dict[str, Any]],
    *,
    expected_phases: list[str] | None = None,
) -> None:
    payload = {
        "metadata": metadata,
        "actions": actions,
        "samples": samples,
        "summary": summarize_samples(samples, expected_phases=expected_phases) if samples else None,
    }
    write_json(run_dir / "progress.json", payload)


def main() -> int:
    args = parse_args()
    previous_signal_handlers: dict[int, Any] = {}

    def raise_keyboard_interrupt(signum: int, _frame: Any) -> None:
        raise KeyboardInterrupt(f"received signal {signum}")

    for signum in (signal.SIGINT, signal.SIGTERM):
        previous_signal_handlers[signum] = signal.getsignal(signum)
        signal.signal(signum, raise_keyboard_interrupt)

    report_root = Path(args.report_root)
    timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    run_dir = report_root / f"{timestamp}-{args.profile_name}"
    run_dir.mkdir(parents=True, exist_ok=False)
    jsonl_path = run_dir / "timeseries.jsonl"

    metadata = {
        "profile_name": args.profile_name,
        "output_profile": args.output_profile,
        "scene_type": args.scene_type,
        "started_at_utc": datetime.now(timezone.utc).isoformat(),
        "target_ma": args.target_ma,
        "load_min_v_mv": args.load_min_v_mv,
        "load_device": args.load_device,
        "load_usb_port": args.load_usb_port,
        "load_bridge_device": args.load_bridge_device,
        "load_bridge_url": effective_load_bridge_url(args),
        "load_cli": args.load_cli,
        "load_ipc": args.load_ipc,
        "load_status_source": args.load_status_source,
        "load_stream_interval_seconds": args.load_stream_interval_seconds,
        "load_status_ready_timeout_sec": args.load_status_ready_timeout_sec,
        "ups_status_url": args.ups_status_url,
        "ups_settings_url": args.ups_settings_url,
        "devd_power_diag_url": args.devd_power_diag_url,
        "devd_monitor_start_url": args.devd_monitor_start_url,
        "devd_device_trace_url": args.devd_device_trace_url,
        "isolapurr_url": args.isolapurr_url,
        "source_voltage_mv": args.source_voltage_mv,
        "source_current_limit_ma": args.source_current_limit_ma,
        "pre_seconds": args.pre_seconds,
        "hold_seconds": args.hold_seconds,
        "backup_hold_seconds": args.backup_hold_seconds,
        "restore_hold_seconds": args.restore_hold_seconds,
        "post_seconds": args.post_seconds,
        "sample_interval_seconds": args.sample_interval_seconds,
        "target_sample_rate_hz": FORMAL_TARGET_SAMPLE_RATE_HZ,
        "minimum_effective_sample_rate_hz": FORMAL_MIN_EFFECTIVE_SAMPLE_RATE_HZ,
        "include_backup": args.include_backup,
        "command_timeout_sec": args.command_timeout_sec,
        "status_timeout_sec": args.status_timeout_sec,
        "load_status_poll_timeout_sec": args.load_status_poll_timeout_sec,
        "verify_timeout_sec": args.verify_timeout_sec,
        "max_i_ma_total": args.max_i_ma_total,
        "max_p_mw": args.max_p_mw,
    }
    expected_phases = ["pre", "hold"]
    if args.include_backup:
        expected_phases.extend(["backup", "restore"])
    expected_phases.append("post")
    actions: list[dict[str, Any]] = []
    samples: list[dict[str, Any]] = []
    load_status_poller: LoadStatusPoller | None = None
    ups_status_poller: Any | None = None
    power_diag_poller: Any | None = None
    settings_poller: JsonPoller | None = None
    isolapurr_poller: JsonPoller | None = None
    settings_snapshot: dict[str, Any] | None = None
    load_telemetry_probe: dict[str, Any] | None = None
    caught_exc: Exception | None = None

    try:
        devd_bootstrap_gate = ensure_valid_mains_aegis_devd_http_base(
            args.devd_scan_url,
            timeout_sec=min(args.status_timeout_sec, 5.0),
        )
        actions.append({"devd_bootstrap_gate": devd_bootstrap_gate})
        if devd_bootstrap_gate.get("ok") is not True:
            raise RuntimeError(
                "devd_bootstrap_gate_failed: "
                f"{devd_bootstrap_gate.get('failures')}"
            )
        refresh_devd_devices = http_post_empty_best_effort(
            args.devd_scan_url,
            timeout_sec=min(args.status_timeout_sec, 10.0),
        )
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
        actions.append({"refresh_devd_devices": refresh_devd_devices})
        source_reachability_gate = probe_isolapurr_source_reachability(
            args.isolapurr_url,
            timeout_sec=min(args.status_timeout_sec, 5.0),
            isolapurr_cli=args.isolapurr_cli,
            expected_device_id=getattr(args, "isolapurr_device_id", DEFAULT_ISOLAPURR_DEVICE_ID),
        )
        actions.append({"source_reachability_gate": source_reachability_gate})
        if source_reachability_gate.get("ok") is not True:
            raise RuntimeError(
                "source_reachability_gate_failed: "
                f"{source_reachability_gate.get('failures')}"
            )
        seeded_devd_device = devd_device_entry_from_scan(
            refresh_devd_devices,
            device_id=devd_device_id_from_endpoint(args.devd_power_diag_url),
        )
        direct_lan_status_url = maybe_promote_ups_status_url_to_direct_lan(
            args.ups_status_url,
            lan_address=(
                seeded_devd_device.get("lan_address")
                if isinstance(seeded_devd_device.get("lan_address"), str)
                else None
            ),
        )
        actions.append({"port_c_disable_before_identify": set_port_c_power(args.isolapurr_url, False)})
        actions.append(
            {
                "ensure_ups_monitor": http_post_empty_best_effort(
                    args.devd_monitor_start_url,
                    timeout_sec=min(args.status_timeout_sec, 5.0),
                )
            }
        )
        input_cut_gate = wait_for_ups_external_input_cut(
            direct_lan_status_url,
            timeout_sec=min(args.status_timeout_sec, 10.0),
        )
        actions.append({"ups_input_cut_before_capability_gate": input_cut_gate})
        if input_cut_gate.get("ok") is not True:
            raise RuntimeError(
                f"ups_input_cut_gate_failed: {dict_or_empty(input_cut_gate.get('validation')).get('failures')}"
            )
        persist_progress(run_dir, metadata, actions, samples, expected_phases=expected_phases)

        if seeded_devd_device_is_capability_ready(seeded_devd_device):
            actions.append(
                {
                    "connect_ups_before_capability_gate": {
                        "skipped": True,
                        "reason": "already_connected_per_scan_snapshot_re_reading_usb_truth",
                    }
                }
            )
        else:
            connection_payload: dict[str, Any] | None = None
            if hasattr(args, "mains_aegis_cli"):
                connection_payload = mains_aegis_read_connection(args)
            if dict_or_empty(connection_payload).get("connection") == "connected":
                actions.append(
                    {
                        "connect_ups_before_capability_gate": {
                            "skipped": True,
                            "reason": "already_connected_with_live_connection_check",
                            "connection": connection_payload,
                        }
                    }
                )
            else:
                actions.append({"connect_ups_before_capability_gate": mains_aegis_connect_device(args)})
        usb_identity_payload = mains_aegis_read_identity(args)
        usb_settings_payload = mains_aegis_read_settings(args)
        identity_payload = http_json_with_retries(
            ups_identity_url_from_status_url(direct_lan_status_url),
            timeout_sec=min(args.status_timeout_sec, 5.0),
        )
        settings_payload = http_json_with_retries(
            direct_lan_status_url.rsplit("/status", 1)[0] + "/settings",
            timeout_sec=min(args.status_timeout_sec, 5.0),
        )
        capability_gate = validate_dual_surface_hardware_capabilities(
            expected_output_profile=args.output_profile,
            expected_source_voltage_mv=args.source_voltage_mv,
            usb_identity_payload=usb_identity_payload,
            usb_settings_payload=usb_settings_payload,
            http_identity_payload=identity_payload,
            http_settings_payload=settings_payload,
        )
        actions.append({"hardware_capability_gate": capability_gate})
        if capability_gate.get("ok") is not True:
            raise RuntimeError(f"hardware_capability_gate_failed: {capability_gate.get('failures')}")
        input_cut_gate_after_capability = wait_for_ups_external_input_cut(
            direct_lan_status_url,
            timeout_sec=min(args.status_timeout_sec, 10.0),
        )
        actions.append({"ups_input_cut_before_source_restore": input_cut_gate_after_capability})
        if input_cut_gate_after_capability.get("ok") is not True:
            raise RuntimeError(
                "ups_input_cut_gate_failed_before_source_restore: "
                f"{dict_or_empty(input_cut_gate_after_capability.get('validation')).get('failures')}"
            )
        actions.append(
            {
                "source_restore_before_start": set_isolapurr_manual_output(
                    args.isolapurr_url,
                    voltage_mv=args.source_voltage_mv,
                    current_limit_ma=args.source_current_limit_ma,
                    isolapurr_cli=args.isolapurr_cli,
                )
            }
        )
        source_restore_payload = dict_or_empty(
            dict_or_empty(actions[-1]).get("source_restore_before_start")
        )
        power_show_payload = fetch_isolapurr_power_show_best_effort(
            args.isolapurr_url,
            timeout_sec=min(args.status_timeout_sec, 5.0),
            isolapurr_cli=args.isolapurr_cli,
        )
        ports_payload = fetch_isolapurr_ports(
            args.isolapurr_url,
            timeout_sec=min(args.status_timeout_sec, 5.0),
            isolapurr_cli=args.isolapurr_cli,
        )
        source_readback_gate = validate_isolapurr_source_configuration(
            expected_voltage_mv=args.source_voltage_mv,
            expected_current_limit_ma=args.source_current_limit_ma,
            manual_ack_payload=source_restore_payload,
            power_show_payload=power_show_payload,
            ports_payload=ports_payload,
        )
        actions.append({"source_configuration_gate": source_readback_gate})
        if source_readback_gate.get("ok") is not True:
            raise RuntimeError(
                f"source_configuration_gate_failed: {source_readback_gate.get('failures')}"
            )
        actions.append({"port_c_enable_before_start": set_port_c_power(args.isolapurr_url, True)})
        settings_snapshot = {
            "advanced_power": settings_payload.get("advanced_power"),
            "advanced_power_capabilities": settings_payload.get("advanced_power_capabilities"),
        }
        seeded_ups_status = dict_or_empty(seeded_devd_device.get("status"))
        seeded_power_diag = dict_or_empty(seeded_devd_device.get("power_diag"))
        if direct_lan_status_url != args.ups_status_url:
            metadata["ups_status_url"] = direct_lan_status_url
            actions.append(
                {
                    "ups_status_transport_override": {
                        "requested": args.ups_status_url,
                        "effective": direct_lan_status_url,
                        "reason": "prefer_direct_lan_status_for_formal_truth",
                    }
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
        write_json(run_dir / "settings_snapshot.json", settings_snapshot)
        disable_before_start = actions[-1]["disable_before_start"]
        disable_verified = dict_or_empty(disable_before_start.get("verified_status"))
        load_telemetry_probe = run_load_telemetry_probe(args)
        actions.append({"load_telemetry_probe": load_telemetry_probe})
        preflight = build_preflight(
            argparse.Namespace(**{**vars(args), "ups_status_url": metadata["ups_status_url"]}),
            identity_payload,
            settings_payload,
            known_load_disabled=disable_verified.get("effective_enabled") is False,
            known_load_target_i_ma=disable_verified.get("effective_target_i_ma"),
            load_telemetry_probe=load_telemetry_probe,
            seeded_ups_status=seeded_ups_status,
            seeded_power_diag=seeded_power_diag,
        )
        write_json(run_dir / "preflight.json", preflight)
        if not preflight.get("scene_valid"):
            raise RuntimeError(f"preflight_failed: {preflight.get('failures')}")
        persist_progress(run_dir, metadata, actions, samples, expected_phases=expected_phases)
        initial_load_status, initial_load_status_seed = bootstrap_load_status_seed(
            args,
            args.load_device,
            disable_result=disable_before_start,
            timeout_sec=min(args.status_timeout_sec, args.load_status_poll_timeout_sec),
        )
        actions.append({"initial_load_status_seed": initial_load_status_seed})
        load_status_poller = LoadStatusPoller(
            args,
            args.load_device,
            timeout_sec=min(args.status_timeout_sec, args.load_status_poll_timeout_sec),
            poll_interval_sec=min(0.15, max(0.05, args.sample_interval_seconds / 2.0)),
            stream_interval_sec=args.load_stream_interval_seconds,
            use_status_stream=args.load_status_source == "status-stream",
        )
        load_status_poller.replace_status(initial_load_status)
        load_status_poller.start()
        ups_device_id = devd_device_id_from_endpoint(args.devd_power_diag_url)
        ups_status_poller = SseStatusPoller(
            name="ups-status",
            url=metadata["ups_status_url"],
            timeout_sec=min(args.status_timeout_sec, 5.0),
        )
        if seeded_ups_status and metadata["ups_status_url"] == args.ups_status_url:
            ups_status_poller.prime(seeded_ups_status)
        power_diag_poller = JsonPoller(
            name="ups-power-diag",
            fetch_fn=lambda: http_json_with_retries(
                metadata["devd_power_diag_url"],
                timeout_sec=min(args.status_timeout_sec, 5.0),
                retries=1,
                retry_delay_sec=0.05,
            ),
            poll_interval_sec=min(0.15, max(0.05, args.sample_interval_seconds / 2.0)),
        )
        if seeded_power_diag:
            power_diag_poller.prime(seeded_power_diag)
        isolapurr_poller = JsonPoller(
            name="isolapurr",
            fetch_fn=lambda: fetch_isolapurr_ports(
                args.isolapurr_url,
                timeout_sec=min(args.status_timeout_sec, 5.0),
                isolapurr_cli=args.isolapurr_cli,
            ),
            poll_interval_sec=min(0.2, args.sample_interval_seconds),
        )
        started_pollers: list[JsonPoller] = []
        for poller in (ups_status_poller, power_diag_poller, isolapurr_poller):
            if poller is None or poller in started_pollers:
                continue
            poller.start()
            started_pollers.append(poller)
        load_status_ready = wait_for_live_load_status(
            load_status_poller,
            sample_interval_seconds=args.sample_interval_seconds,
            timeout_sec=args.load_status_ready_timeout_sec,
            require_new_generation=False,
        )
        actions.append({"load_status_ready": load_status_ready})
        scene_pollers_ready = wait_for_scene_pollers_ready(
            ups_status_poller=ups_status_poller,
            power_diag_poller=power_diag_poller,
            isolapurr_poller=isolapurr_poller,
            sample_interval_seconds=args.sample_interval_seconds,
            timeout_sec=max(args.load_status_ready_timeout_sec, 10.0),
            ups_device_id=ups_device_id,
        )
        actions.append({"scene_pollers_ready": scene_pollers_ready})
        persist_progress(run_dir, metadata, actions, samples, expected_phases=expected_phases)
        samples.extend(
            execute_continuous_scene(
                args=args,
                jsonl_path=jsonl_path,
                metadata=metadata,
                actions=actions,
                load_status_poller=load_status_poller,
                ups_status_poller=ups_status_poller,
                power_diag_poller=power_diag_poller,
                settings_snapshot=settings_snapshot or {},
                isolapurr_poller=isolapurr_poller,
                expected_phases=expected_phases,
                run_dir=run_dir,
            )
        )
        persist_progress(run_dir, metadata, actions, samples)
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired, RuntimeError, KeyboardInterrupt, Exception) as exc:
        caught_exc = exc
    finally:
        cleanup_live_status_poller = load_status_poller
        try:
            if load_status_poller is not None:
                load_status_poller.pause()
                load_status_poller.wait_until_idle(
                    timeout_sec=max(
                        args.load_status_poll_timeout_sec + 0.5,
                        3.5,
                    )
                )
                load_status_poller.release_bridge_lease(
                    timeout_sec=min(args.status_timeout_sec, 5.0)
                )
                load_status_poller.release_load_devd_lease(
                    timeout_sec=min(args.status_timeout_sec, 5.0)
                )
                if (
                    args.load_status_source != "status-stream"
                    and not effective_load_bridge_url(args)
                ):
                    cleanup_live_status_poller = None
            cleanup = disable_load(
                args,
                args.load_device,
                timeout_sec=args.command_timeout_sec,
                status_timeout_sec=args.status_timeout_sec,
                verify_timeout_sec=args.verify_timeout_sec,
                live_status_poller=cleanup_live_status_poller,
                before_verify=None,
                allow_command_ack_shortcut=True,
            )
            actions.append({"cleanup_disable_finally": cleanup})
        except Exception as cleanup_exc:  # noqa: BLE001
            actions.append({"cleanup_disable_finally_failed": repr(cleanup_exc)})
        try:
            actions.append({"cleanup_port_c_disable_finally": set_port_c_power(args.isolapurr_url, False)})
        except Exception as cleanup_exc:  # noqa: BLE001
            actions.append({"cleanup_port_c_disable_finally_failed": repr(cleanup_exc)})
        if load_status_poller is not None:
            load_status_poller.stop(timeout_sec=args.status_timeout_sec + 5.0)
        stopped_pollers: list[JsonPoller] = []
        for poller in (
            ups_status_poller,
            power_diag_poller,
            isolapurr_poller,
        ):
            if poller is None or poller in stopped_pollers:
                continue
            poller.stop(timeout_sec=args.status_timeout_sec + 5.0)
            stopped_pollers.append(poller)
        for signum, previous_handler in previous_signal_handlers.items():
            signal.signal(signum, previous_handler)

    if caught_exc is not None:
        payload = {
            "metadata": metadata,
            "settings_snapshot": settings_snapshot,
            "actions": actions,
            "samples": samples,
            "summary": summarize_samples(samples, expected_phases=expected_phases) if samples else None,
            "error": repr(caught_exc),
        }
        write_json(run_dir / "failure.json", payload)
        print(
            json.dumps(
                build_console_summary(run_dir, payload, success=False),
                ensure_ascii=False,
                indent=2,
            )
        )
        return 1

    payload = {
        "metadata": metadata,
        "settings_snapshot": settings_snapshot,
        "actions": actions,
        "samples": samples,
        "summary": summarize_samples(samples, expected_phases=expected_phases),
    }
    write_json(run_dir / "results.json", payload)
    write_json(run_dir / "summary.json", payload["summary"])
    if args.print_full_results:
        print(json.dumps(payload, ensure_ascii=False, indent=2))
    else:
        print(
            json.dumps(
                build_console_summary(run_dir, payload, success=True),
                ensure_ascii=False,
                indent=2,
            )
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
