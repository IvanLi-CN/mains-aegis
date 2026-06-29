#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
import urllib.error
import urllib.request
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


DEFAULT_LOAD_DEVICE = "loadlynx-d68638"
DEFAULT_LOAD_USB_PORT = "/dev/cu.usbmodem212101"
DEFAULT_LOAD_CLI = "/Users/ivan/.codex/worktrees/a31f/loadlynx-host-src/tools/loadlynx-devd/target/debug/loadlynx"
DEFAULT_UPS_STATUS_URL = "http://192.168.31.232/api/v1/status"
DEFAULT_DEVD_URL = "http://127.0.0.1:30080"
DEFAULT_DEVICE_ID = "mains-aegis-198840"
DEFAULT_DEVICE_SERIAL = "serial-04f3bb3f5367"
DEFAULT_DEVD_TARGET_ID = "mains-aegis-198840"
DEFAULT_ISOLAPURR_BASE_URL = "http://192.168.31.122"
DEFAULT_LOADS = "1000,2000,2900,2950,2975,3000,3050,3100,3200,3300"
DEFAULT_NEIGHBOR_LOADS = "2925,2950,2975,3000,3025,3050"
DEFAULT_STANDBY_DROP_MV = 1200
DEFAULT_ASSIST_LOW_DROP_MV = 600
DEFAULT_ASSIST_ENTER_DELTA_MA = 0
DEFAULT_ASSIST_EXIT_DELTA_MA = 0
DEFAULT_ASSIST_REQUIRED_SAMPLES = 2
DEFAULT_ASSIST_RAMP_STEP_MV = 100
DEFAULT_ASSIST_RAMP_INTERVAL_MS = 200
DEFAULT_RATED_ENTER_DELTA_MA = 0
DEFAULT_RATED_EXIT_DELTA_MA = 0
DEFAULT_VIN_DROP_THRESHOLD_PCT = 4
DEFAULT_REQUIRED_SAMPLES = 2
DEFAULT_MAX_I_MA_TOTAL = 3600
DEFAULT_MAX_P_MW = 45000
DEFAULT_LOAD_STATUS_TIMEOUT_SEC = 20.0
DEFAULT_LOAD_COMMAND_TIMEOUT_SEC = 45.0
DEFAULT_LOAD_VERIFY_TIMEOUT_SEC = 45.0
PORT_C_POWER_PATH = "/api/v1/ports/port_c/power"
TRACE_LIMIT = 40


@dataclass
class AdvancedPower:
    standby_drop_mv: int
    assist_low_drop_mv: int
    assist_enter_delta_ma: int
    assist_exit_delta_ma: int
    assist_required_samples: int
    assist_ramp_step_mv: int
    assist_ramp_interval_ms: int
    rated_enter_delta_ma: int
    rated_exit_delta_ma: int
    vin_drop_threshold_pct: int
    required_samples: int


@dataclass
class SampleContext:
    tag: str
    load_ma: int
    hold_seconds: float


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run 12V HIL sweeps for Advanced Power staged-assist tuning."
    )
    parser.add_argument(
        "--load-device",
        default=DEFAULT_LOAD_DEVICE,
        help=f"LoadLynx saved device id for released CLI USB control (default: {DEFAULT_LOAD_DEVICE})",
    )
    parser.add_argument(
        "--load-usb-port",
        default=DEFAULT_LOAD_USB_PORT,
        help=f"Approved USB port for released LoadLynx CLI (default: {DEFAULT_LOAD_USB_PORT})",
    )
    parser.add_argument(
        "--load-cli",
        default=DEFAULT_LOAD_CLI,
        help=f"LoadLynx CLI path for HIL control/telemetry (default: {DEFAULT_LOAD_CLI})",
    )
    parser.add_argument(
        "--ups-status-url",
        default=DEFAULT_UPS_STATUS_URL,
        help=f"UPS status URL (default: {DEFAULT_UPS_STATUS_URL})",
    )
    parser.add_argument(
        "--devd-url",
        default=DEFAULT_DEVD_URL,
        help=f"mains-aegis-devd HTTP base URL (default: {DEFAULT_DEVD_URL})",
    )
    parser.add_argument(
        "--device-id",
        default=DEFAULT_DEVICE_ID,
        help=f"Target device id for settings writes (default: {DEFAULT_DEVICE_ID})",
    )
    parser.add_argument(
        "--device-serial",
        default=DEFAULT_DEVICE_SERIAL,
        help=f"Approved serial binding for reference/audit trails (default: {DEFAULT_DEVICE_SERIAL})",
    )
    parser.add_argument(
        "--devd-target-id",
        default=DEFAULT_DEVD_TARGET_ID,
        help=f"Connected devd device id used for power-diag/trace/reset paths (default: {DEFAULT_DEVD_TARGET_ID})",
    )
    parser.add_argument(
        "--isolapurr-url",
        default=DEFAULT_ISOLAPURR_BASE_URL,
        help=f"IsolaPurr HTTP base URL for 12V source control (default: {DEFAULT_ISOLAPURR_BASE_URL})",
    )
    parser.add_argument(
        "--profile-name",
        required=True,
        help="Human-readable profile label for this sweep.",
    )
    parser.add_argument(
        "--loads-ma",
        default=DEFAULT_LOADS,
        help=f"Comma-separated CC load points in mA (default: {DEFAULT_LOADS})",
    )
    parser.add_argument(
        "--neighbor-loads-ma",
        default=DEFAULT_NEIGHBOR_LOADS,
        help=f"Comma-separated neighbor loads for post-candidate verification (default: {DEFAULT_NEIGHBOR_LOADS})",
    )
    parser.add_argument(
        "--settle-seconds",
        type=float,
        default=4.0,
        help="Settle time after each load change (default: 4.0).",
    )
    parser.add_argument(
        "--baseline-seconds",
        type=float,
        default=2.5,
        help="Settle time before/after sweep at 0 mA (default: 2.5).",
    )
    parser.add_argument(
        "--backup-hold-seconds",
        type=float,
        default=3.0,
        help="Hold time after cutting DCIN during backup check (default: 3.0).",
    )
    parser.add_argument(
        "--long-hold-seconds",
        type=float,
        default=8.0,
        help="Long hold time for boundary re-check points (default: 8.0).",
    )
    parser.add_argument(
        "--reboot-wait-seconds",
        type=float,
        default=10.0,
        help="Wait time after UPS reset when persistence checks are enabled (default: 10.0).",
    )
    parser.add_argument(
        "--report-root",
        default="tools/hil/reports",
        help="Directory root for artifacts (default: tools/hil/reports).",
    )
    parser.add_argument(
        "--max-i-ma-total",
        type=int,
        default=DEFAULT_MAX_I_MA_TOTAL,
        help=f"LoadLynx protection rail (default: {DEFAULT_MAX_I_MA_TOTAL})",
    )
    parser.add_argument(
        "--max-p-mw",
        type=int,
        default=DEFAULT_MAX_P_MW,
        help=f"LoadLynx power protection rail (default: {DEFAULT_MAX_P_MW})",
    )
    parser.add_argument(
        "--load-status-timeout-sec",
        type=float,
        default=DEFAULT_LOAD_STATUS_TIMEOUT_SEC,
        help=f"Timeout for auxiliary LoadLynx status sampling (default: {DEFAULT_LOAD_STATUS_TIMEOUT_SEC})",
    )
    parser.add_argument(
        "--load-command-timeout-sec",
        type=float,
        default=DEFAULT_LOAD_COMMAND_TIMEOUT_SEC,
        help=f"Timeout for LoadLynx cc/disable commands (default: {DEFAULT_LOAD_COMMAND_TIMEOUT_SEC})",
    )
    parser.add_argument(
        "--load-verify-timeout-sec",
        type=float,
        default=DEFAULT_LOAD_VERIFY_TIMEOUT_SEC,
        help=f"Timeout for waiting until LoadLynx status reflects the requested enable state (default: {DEFAULT_LOAD_VERIFY_TIMEOUT_SEC})",
    )
    parser.add_argument(
        "--apply-settings",
        action="store_true",
        help="Write advanced_power settings before the sweep.",
    )
    parser.add_argument(
        "--reset-advanced-power",
        action="store_true",
        help="Reset advanced_power to device defaults before the sweep.",
    )
    parser.add_argument(
        "--reboot-after-apply",
        action="store_true",
        help="Reset the UPS after applying settings, then read back settings again.",
    )
    parser.add_argument(
        "--include-backup",
        action="store_true",
        help="Cut DCIN via IsolaPurr port_c during the highest load point and verify backup stage.",
    )
    parser.add_argument(
        "--include-trace",
        action="store_true",
        help="Capture mains-aegis trace output after each point when available.",
    )
    parser.add_argument(
        "--verify-web-readback",
        action="store_true",
        help="Read settings from LAN /settings in addition to devd readback for parity evidence.",
    )
    parser.add_argument(
        "--neighbor-verify",
        action="store_true",
        help="Run the neighbor-load verification sequence after the primary sweep.",
    )
    parser.add_argument(
        "--persistence-check",
        action="store_true",
        help="Require reboot + readback persistence checks after applying settings.",
    )
    parser.add_argument(
        "--standby-drop-mv",
        type=int,
        default=DEFAULT_STANDBY_DROP_MV,
    )
    parser.add_argument(
        "--assist-low-drop-mv",
        type=int,
        default=DEFAULT_ASSIST_LOW_DROP_MV,
    )
    parser.add_argument(
        "--assist-enter-delta-ma",
        type=int,
        default=DEFAULT_ASSIST_ENTER_DELTA_MA,
    )
    parser.add_argument(
        "--assist-exit-delta-ma",
        type=int,
        default=DEFAULT_ASSIST_EXIT_DELTA_MA,
    )
    parser.add_argument(
        "--assist-required-samples",
        type=int,
        default=DEFAULT_ASSIST_REQUIRED_SAMPLES,
    )
    parser.add_argument(
        "--assist-ramp-step-mv",
        type=int,
        default=DEFAULT_ASSIST_RAMP_STEP_MV,
    )
    parser.add_argument(
        "--assist-ramp-interval-ms",
        type=int,
        default=DEFAULT_ASSIST_RAMP_INTERVAL_MS,
    )
    parser.add_argument(
        "--rated-enter-delta-ma",
        type=int,
        default=DEFAULT_RATED_ENTER_DELTA_MA,
    )
    parser.add_argument(
        "--rated-exit-delta-ma",
        type=int,
        default=DEFAULT_RATED_EXIT_DELTA_MA,
    )
    parser.add_argument(
        "--vin-drop-threshold-pct",
        type=int,
        default=DEFAULT_VIN_DROP_THRESHOLD_PCT,
    )
    parser.add_argument(
        "--required-samples",
        type=int,
        default=DEFAULT_REQUIRED_SAMPLES,
    )
    return parser.parse_args()


def parse_loads(csv: str) -> list[int]:
    values = []
    for value in csv.split(","):
        value = value.strip()
        if not value:
            continue
        values.append(int(value))
    if not values:
        raise SystemExit("at least one load value is required")
    return values


def run(
    cmd: list[str], *, timeout_sec: float | None = None
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        cmd,
        check=True,
        text=True,
        capture_output=True,
        timeout=timeout_sec,
    )


def timeout_stream_text(value: str | bytes | None) -> str:
    if value is None:
        return ""
    if isinstance(value, bytes):
        return value.decode("utf-8", errors="replace")
    return value


def http_json(url: str, method: str = "GET", body: dict[str, Any] | None = None) -> Any:
    data = None
    headers = {}
    if body is not None:
        data = json.dumps(body).encode("utf-8")
        headers["content-type"] = "application/json"
    request = urllib.request.Request(url, data=data, method=method, headers=headers)
    with urllib.request.urlopen(request, timeout=10) as response:
        return json.load(response)


def http_post_void(url: str) -> dict[str, Any]:
    request = urllib.request.Request(url, method="POST")
    with urllib.request.urlopen(request, timeout=10) as response:
        body = response.read()
        text = body.decode("utf-8") if body else ""
        return {
            "status": response.status,
            "body": text,
        }


def isolapurr_power_show(isolapurr_url: str, *, timeout_sec: float = 10.0) -> Any:
    completed = run(
        [
            "isolapurr",
            "power",
            "show",
            "--url",
            isolapurr_url,
            "--json",
        ],
        timeout_sec=timeout_sec,
    )
    return json.loads(completed.stdout)


def ups_base_url(ups_status_url: str) -> str:
    return ups_status_url.rsplit("/", 1)[0]


def unique_devd_target_ids(*values: str) -> list[str]:
    ordered: list[str] = []
    for value in values:
        candidate = value.strip()
        if candidate and candidate not in ordered:
            ordered.append(candidate)
    return ordered


def devd_device_json(
    devd_url: str,
    path_suffix: str,
    *,
    target_ids: list[str],
    method: str = "GET",
    body: dict[str, Any] | None = None,
) -> Any:
    last_error: urllib.error.URLError | None = None
    for target_id in target_ids:
        url = f"{devd_url}/api/v1/devices/{target_id}{path_suffix}"
        try:
            return http_json(url, method=method, body=body)
        except urllib.error.HTTPError as exc:
            if exc.code != 404:
                raise
            last_error = exc
        except urllib.error.URLError as exc:
            last_error = exc
    if last_error is not None:
        raise last_error
    raise RuntimeError("devd target id list must not be empty")


def ensure_usb_port(args: argparse.Namespace, load_device: str, load_usb_port: str) -> dict[str, Any]:
    cmd = [args.load_cli, "devices", "--json"]
    completed = run(cmd)
    payload = json.loads(completed.stdout)
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
        "cmd": cmd,
        "device_id": load_device,
        "usb_port": actual_port,
        "last_transport": matched.get("last_transport"),
        "stdout": completed.stdout,
        "stderr": completed.stderr,
    }


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
) -> dict[str, Any]:
    cmd = [
        args.load_cli,
        "cc",
        str(current_ma),
        "--device",
        load_device,
        "--max-i-ma-total",
        str(max_i_ma_total),
        "--max-p-mw",
        str(max_p_mw),
    ]
    completed: subprocess.CompletedProcess[str] | None = None
    timeout_error: subprocess.TimeoutExpired | None = None
    try:
        completed = run(cmd, timeout_sec=timeout_sec)
    except subprocess.TimeoutExpired as exc:
        timeout_error = exc
    verified_status = wait_for_load_state(
        args,
        load_device,
        expected_enabled=True,
        expected_target_i_ma=current_ma,
        status_timeout_sec=status_timeout_sec,
        verify_timeout_sec=verify_timeout_sec,
    )
    result = {
        "cmd": cmd,
        "verified_status": verified_status,
    }
    if completed is not None:
        result["stdout"] = completed.stdout
        result["stderr"] = completed.stderr
    else:
        result["stdout"] = timeout_stream_text(timeout_error.stdout if timeout_error else None)
        result["stderr"] = timeout_stream_text(timeout_error.stderr if timeout_error else None)
        result["timed_out_but_verified"] = True
        result["timeout_error"] = repr(timeout_error)
    return result


def disable_load(
    args: argparse.Namespace,
    load_device: str,
    *,
    timeout_sec: float,
    status_timeout_sec: float,
    verify_timeout_sec: float,
) -> dict[str, Any]:
    control = get_load_control_best_effort(args, load_device, timeout_sec=status_timeout_sec)
    status = get_load_status(args, load_device, timeout_sec=status_timeout_sec)
    if load_output_enabled(control) is False:
        return {
            "cmd": None,
            "stdout": "",
            "stderr": "",
            "skipped": True,
            "reason": "already_disabled",
            "control": control,
            "status": status,
        }
    cmd = [args.load_cli, "control", "set", "--device", load_device, "--disable"]
    completed: subprocess.CompletedProcess[str] | None = None
    timeout_error: subprocess.TimeoutExpired | None = None
    try:
        completed = run(cmd, timeout_sec=timeout_sec)
    except subprocess.TimeoutExpired as exc:
        timeout_error = exc
    verified_status = wait_for_load_state(
        args,
        load_device,
        expected_enabled=False,
        expected_target_i_ma=None,
        status_timeout_sec=status_timeout_sec,
        verify_timeout_sec=verify_timeout_sec,
    )
    result = {
        "cmd": cmd,
        "verified_status": verified_status,
    }
    if completed is not None:
        result["stdout"] = completed.stdout
        result["stderr"] = completed.stderr
    else:
        result["stdout"] = timeout_stream_text(timeout_error.stdout if timeout_error else None)
        result["stderr"] = timeout_stream_text(timeout_error.stderr if timeout_error else None)
        result["timed_out_but_verified"] = True
        result["timeout_error"] = repr(timeout_error)
    return result


def get_load_control(args: argparse.Namespace, load_device: str, *, timeout_sec: float) -> Any:
    completed = run(
        [args.load_cli, "control", "get", "--device", load_device, "--json"],
        timeout_sec=timeout_sec,
    )
    return json.loads(completed.stdout)


def get_load_control_best_effort(
    args: argparse.Namespace, load_device: str, *, timeout_sec: float
) -> Any:
    try:
        return get_load_control(args, load_device, timeout_sec=timeout_sec)
    except (subprocess.TimeoutExpired, subprocess.CalledProcessError, json.JSONDecodeError) as exc:
        return {
            "error": repr(exc),
            "ok": False,
        }


def get_load_status(args: argparse.Namespace, load_device: str, *, timeout_sec: float) -> Any:
    try:
        completed = run(
            [args.load_cli, "status", "--device", load_device, "--json"],
            timeout_sec=timeout_sec,
        )
        return json.loads(completed.stdout)
    except (subprocess.TimeoutExpired, subprocess.CalledProcessError, json.JSONDecodeError) as exc:
        return {
            "error": repr(exc),
            "ok": False,
        }


def load_output_enabled(status: Any) -> bool | None:
    if not isinstance(status, dict):
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
    control = status.get("control")
    if isinstance(control, dict) and isinstance(control.get("target_i_ma"), int):
        return control.get("target_i_ma")
    return None


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


def wait_for_load_state(
    args: argparse.Namespace,
    load_device: str,
    *,
    expected_enabled: bool,
    expected_target_i_ma: int | None,
    status_timeout_sec: float,
    verify_timeout_sec: float,
) -> Any:
    deadline = time.monotonic() + verify_timeout_sec
    last_control: Any = None
    last_status: Any = None
    while time.monotonic() < deadline:
        last_control = get_load_control_best_effort(
            args,
            load_device, timeout_sec=status_timeout_sec
        )
        last_status = get_load_status(args, load_device, timeout_sec=status_timeout_sec)
        enabled = load_output_enabled(last_control)
        target_i_ma = load_target_i_ma(last_control)
        if enabled is None:
            enabled = load_output_enabled(last_status)
        if target_i_ma is None:
            target_i_ma = load_target_i_ma(last_status)
        target_ok = expected_target_i_ma is None or target_i_ma == expected_target_i_ma
        if enabled is expected_enabled and target_ok:
            return {
                "control": last_control,
                "status": last_status,
            }
        time.sleep(1.0)
    raise RuntimeError(
        "LoadLynx status did not reach expected state: "
        f"enabled={expected_enabled} target_i_ma={expected_target_i_ma} "
        f"last_control={last_control} last_status={last_status}"
    )


def reset_advanced_power(
    devd_url: str, device_id: str, ups_status_url: str
) -> dict[str, Any]:
    devd_error = None
    try:
        response = http_json(
            f"{devd_url}/api/v1/settings/advanced-power/reset?device_id={device_id}",
            method="POST",
        )
        return {"path": "devd", "response": response}
    except urllib.error.URLError as exc:
        devd_error = repr(exc)

    response = http_json(
        f"{ups_base_url(ups_status_url)}/settings/advanced-power/reset",
        method="POST",
        body={},
    )
    return {"path": "lan", "response": response, "devd_error": devd_error}


def set_advanced_power(
    devd_url: str, device_id: str, ups_status_url: str, settings: AdvancedPower
) -> dict[str, Any]:
    body = asdict(settings)
    body["device_id"] = device_id
    devd_error = None
    try:
        response = http_json(
            f"{devd_url}/api/v1/settings/advanced-power",
            method="POST",
            body=body,
        )
        return {"path": "devd", "response": response}
    except urllib.error.URLError as exc:
        devd_error = repr(exc)

    response = http_json(
        f"{ups_base_url(ups_status_url)}/settings/advanced-power",
        method="POST",
        body=asdict(settings),
    )
    return {"path": "lan", "response": response, "devd_error": devd_error}


def reset_ups(devd_url: str, devd_target_ids: list[str]) -> Any:
    return devd_device_json(
        devd_url,
        "/reset",
        target_ids=devd_target_ids,
        method="POST",
        body={"reason": "advanced_power_12v_sweep"},
    )


def get_settings(ups_status_url: str) -> Any:
    return http_json(f"{ups_base_url(ups_status_url)}/settings")


def get_power_diag(devd_url: str, devd_target_ids: list[str]) -> Any:
    return devd_device_json(
        devd_url,
        "/power-diag",
        target_ids=devd_target_ids,
    )


def get_trace(devd_url: str, devd_target_ids: list[str]) -> Any:
    return devd_device_json(
        devd_url,
        f"/trace?kind=event&target=power&limit={TRACE_LIMIT}",
        target_ids=devd_target_ids,
    )


def power_path_url(isolapurr_url: str, enabled: bool) -> str:
    value = "1" if enabled else "0"
    return f"{isolapurr_url}{PORT_C_POWER_PATH}?enabled={value}"


def set_port_c_power(isolapurr_url: str, enabled: bool) -> dict[str, Any]:
    return http_post_void(power_path_url(isolapurr_url, enabled))


def collect_point(
    context: SampleContext,
    *,
    args: argparse.Namespace,
    ups_status_url: str,
    devd_url: str,
    devd_target_ids: list[str],
    isolapurr_url: str,
    load_device: str,
    include_trace: bool,
    load_status_timeout_sec: float,
) -> dict[str, Any]:
    point = {
        "tag": context.tag,
        "load_ma": context.load_ma,
        "hold_seconds": context.hold_seconds,
        "timestamp_utc": datetime.now(timezone.utc).isoformat(),
        "status": http_json(ups_status_url),
        "power_diag": get_power_diag(devd_url, devd_target_ids),
        "isolapurr_power": isolapurr_power_show(isolapurr_url),
        "load_control": get_load_control_best_effort(
            args,
            load_device, timeout_sec=load_status_timeout_sec
        ),
        "load_status": get_load_status(
            args,
            load_device, timeout_sec=load_status_timeout_sec
        ),
    }
    if include_trace:
        try:
            point["trace"] = get_trace(devd_url, devd_target_ids)
        except urllib.error.URLError as exc:
            point["trace_error"] = repr(exc)
    return point


def summarize_point(point: dict[str, Any]) -> dict[str, Any]:
    status = point["status"]
    diag = point["power_diag"]
    input_status = status["input"]
    battery = status["battery"]
    charger = status["charger"]
    policy = diag["policy"]
    load_status = point.get("load_status") or {}
    return {
        "tag": point["tag"],
        "load_ma": point["load_ma"],
        "mode": status["mode"],
        "assist_power_stage": input_status.get("assist_power_stage"),
        "assist_target_vout_mv": input_status.get("assist_target_vout_mv"),
        "vin_vbus_mv": input_status.get("vin_vbus_mv"),
        "vin_iin_ma": input_status.get("vin_iin_ma"),
        "vin_baseline_mv": input_status.get("vin_baseline_mv"),
        "vin_drop_mv": input_status.get("vin_drop_mv"),
        "tps_total_iout_ma": input_status.get("tps_total_iout_ma"),
        "ups_vout_mv": status["output"]["out_a"].get("vbus_mv")
        if isinstance(status["output"]["out_a"].get("vbus_mv"), (int, float))
        else (status.get("output") or {}).get("out_b", {}).get("vbus_mv"),
        "battery_current_ma": battery.get("current_ma"),
        "battery_pack_mv": battery.get("pack_mv"),
        "allow_charge": charger.get("allow_charge"),
        "charger_status": charger.get("detail_status"),
        "policy_status": policy.get("status"),
        "policy_state": policy.get("state"),
        "policy_output_blocked": policy.get("output_blocked"),
        "policy_output_block_reason": policy.get("output_block_reason"),
        "load_output_enabled": load_output_enabled(load_status),
        "load_target_i_ma": load_target_i_ma(load_status),
        "load_v_local_mv": ((load_status.get("status") or {}).get("v_local_mv")),
        "load_i_local_ma": ((load_status.get("status") or {}).get("i_local_ma")),
        "load_i_remote_ma": ((load_status.get("status") or {}).get("i_remote_ma")),
        "load_i_total_ma": load_status_i_total_ma(load_status),
        "load_calc_p_mw": ((load_status.get("status") or {}).get("calc_p_mw")),
    }


def build_settings(args: argparse.Namespace) -> AdvancedPower:
    return AdvancedPower(
        standby_drop_mv=args.standby_drop_mv,
        assist_low_drop_mv=args.assist_low_drop_mv,
        assist_enter_delta_ma=args.assist_enter_delta_ma,
        assist_exit_delta_ma=args.assist_exit_delta_ma,
        assist_required_samples=args.assist_required_samples,
        assist_ramp_step_mv=args.assist_ramp_step_mv,
        assist_ramp_interval_ms=args.assist_ramp_interval_ms,
        rated_enter_delta_ma=args.rated_enter_delta_ma,
        rated_exit_delta_ma=args.rated_exit_delta_ma,
        vin_drop_threshold_pct=args.vin_drop_threshold_pct,
        required_samples=args.required_samples,
    )


def sleep_and_collect(
    points: list[dict[str, Any]],
    context: SampleContext,
    *,
    args: argparse.Namespace,
    ups_status_url: str,
    devd_url: str,
    devd_target_ids: list[str],
    isolapurr_url: str,
    load_device: str,
    include_trace: bool,
    load_status_timeout_sec: float,
) -> None:
    time.sleep(context.hold_seconds)
    points.append(
        collect_point(
            context,
            args=args,
            ups_status_url=ups_status_url,
            devd_url=devd_url,
            devd_target_ids=devd_target_ids,
            isolapurr_url=isolapurr_url,
            load_device=load_device,
            include_trace=include_trace,
            load_status_timeout_sec=load_status_timeout_sec,
        )
    )


def apply_load_and_sample(
    *,
    args: argparse.Namespace,
    load_ma: int,
    hold_seconds: float,
    load_device: str,
    max_i_ma_total: int,
    max_p_mw: int,
    points: list[dict[str, Any]],
    actions: list[dict[str, Any]],
    ups_status_url: str,
    devd_url: str,
    devd_target_ids: list[str],
    isolapurr_url: str,
    include_trace: bool,
    load_status_timeout_sec: float,
    load_command_timeout_sec: float,
    load_verify_timeout_sec: float,
) -> None:
    actions.append(
        {
            f"cc_{load_ma}": load_cc(
                args,
                load_device,
                load_ma,
                max_i_ma_total=max_i_ma_total,
                max_p_mw=max_p_mw,
                timeout_sec=load_command_timeout_sec,
                status_timeout_sec=load_status_timeout_sec,
                verify_timeout_sec=load_verify_timeout_sec,
            )
        }
    )
    sleep_and_collect(
        points,
        SampleContext(tag=f"cc_{load_ma}", load_ma=load_ma, hold_seconds=hold_seconds),
        args=args,
        ups_status_url=ups_status_url,
        devd_url=devd_url,
        devd_target_ids=devd_target_ids,
        isolapurr_url=isolapurr_url,
        load_device=load_device,
        include_trace=include_trace,
        load_status_timeout_sec=load_status_timeout_sec,
    )


def main() -> int:
    args = parse_args()
    report_root = Path(args.report_root)
    timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    run_dir = report_root / f"{timestamp}-{args.profile_name}"
    run_dir.mkdir(parents=True, exist_ok=False)

    primary_loads = parse_loads(args.loads_ma)
    neighbor_loads = parse_loads(args.neighbor_loads_ma)
    settings = build_settings(args)
    devd_target_ids = unique_devd_target_ids(
        args.devd_target_id,
        args.device_id,
        args.device_serial,
    )

    metadata: dict[str, Any] = {
        "profile_name": args.profile_name,
        "started_at_utc": datetime.now(timezone.utc).isoformat(),
        "load_device": args.load_device,
        "load_usb_port": args.load_usb_port,
        "load_cli": args.load_cli,
        "ups_status_url": args.ups_status_url,
        "devd_url": args.devd_url,
        "device_id": args.device_id,
        "device_serial": args.device_serial,
        "devd_target_id": args.devd_target_id,
        "devd_target_ids": devd_target_ids,
        "isolapurr_url": args.isolapurr_url,
        "loads_ma": primary_loads,
        "neighbor_loads_ma": neighbor_loads,
        "settle_seconds": args.settle_seconds,
        "baseline_seconds": args.baseline_seconds,
        "backup_hold_seconds": args.backup_hold_seconds,
        "long_hold_seconds": args.long_hold_seconds,
        "load_command_timeout_sec": args.load_command_timeout_sec,
        "load_verify_timeout_sec": args.load_verify_timeout_sec,
        "apply_settings": args.apply_settings,
        "reset_advanced_power": args.reset_advanced_power,
        "reboot_after_apply": args.reboot_after_apply,
        "persistence_check": args.persistence_check,
        "include_backup": args.include_backup,
        "include_trace": args.include_trace,
        "verify_web_readback": args.verify_web_readback,
        "neighbor_verify": args.neighbor_verify,
        "requested_advanced_power": asdict(settings),
    }

    actions: list[dict[str, Any]] = []
    points: list[dict[str, Any]] = []

    try:
        actions.append(
            {
                "ensure_usb_port": ensure_usb_port(
                    args,
                    args.load_device,
                    args.load_usb_port,
                )
            }
        )
        actions.append({"port_c_enable_before_start": set_port_c_power(args.isolapurr_url, True)})
        actions.append(
            {
                "disable_before_start": disable_load(
                    args,
                    args.load_device,
                    timeout_sec=args.load_command_timeout_sec,
                    status_timeout_sec=args.load_status_timeout_sec,
                    verify_timeout_sec=args.load_verify_timeout_sec,
                )
            }
        )
        time.sleep(args.baseline_seconds)

        if args.reset_advanced_power:
            actions.append(
                {
                    "reset_advanced_power": reset_advanced_power(
                        args.devd_url, args.device_id, args.ups_status_url
                    )
                }
            )
            time.sleep(1.0)

        if args.apply_settings:
            actions.append(
                {
                    "set_advanced_power": set_advanced_power(
                        args.devd_url, args.device_id, args.ups_status_url, settings
                    )
                }
            )
            actions.append({"readback_after_set": get_settings(args.ups_status_url)})
            if args.verify_web_readback:
                actions.append({"web_readback_after_set": get_settings(args.ups_status_url)})
            time.sleep(1.0)

        if args.reboot_after_apply or args.persistence_check:
            actions.append({"reset_ups": reset_ups(args.devd_url, devd_target_ids)})
            time.sleep(args.reboot_wait_seconds)
            actions.append({"readback_after_reboot": get_settings(args.ups_status_url)})
            if args.verify_web_readback:
                actions.append({"web_readback_after_reboot": get_settings(args.ups_status_url)})

        sleep_and_collect(
            points,
            SampleContext(tag="baseline", load_ma=0, hold_seconds=0.0),
            args=args,
            ups_status_url=args.ups_status_url,
            devd_url=args.devd_url,
            devd_target_ids=devd_target_ids,
            isolapurr_url=args.isolapurr_url,
            load_device=args.load_device,
            include_trace=args.include_trace,
            load_status_timeout_sec=args.load_status_timeout_sec,
        )

        for load_ma in primary_loads:
            hold = args.long_hold_seconds if load_ma in {2950, 2975, 3000, 3050, 3200, 3300} else args.settle_seconds
            apply_load_and_sample(
                args=args,
                load_ma=load_ma,
                hold_seconds=hold,
                load_device=args.load_device,
                max_i_ma_total=args.max_i_ma_total,
                max_p_mw=args.max_p_mw,
                points=points,
                actions=actions,
                ups_status_url=args.ups_status_url,
                devd_url=args.devd_url,
                devd_target_ids=devd_target_ids,
                isolapurr_url=args.isolapurr_url,
                include_trace=args.include_trace,
                load_status_timeout_sec=args.load_status_timeout_sec,
                load_command_timeout_sec=args.load_command_timeout_sec,
                load_verify_timeout_sec=args.load_verify_timeout_sec,
            )

        if args.include_backup:
            backup_load = primary_loads[-1]
            actions.append({"port_c_disable_for_backup": set_port_c_power(args.isolapurr_url, False)})
            sleep_and_collect(
                points,
                SampleContext(
                    tag=f"backup_{backup_load}",
                    load_ma=backup_load,
                    hold_seconds=args.backup_hold_seconds,
                ),
                args=args,
                ups_status_url=args.ups_status_url,
                devd_url=args.devd_url,
                devd_target_ids=devd_target_ids,
                isolapurr_url=args.isolapurr_url,
                load_device=args.load_device,
                include_trace=args.include_trace,
                load_status_timeout_sec=args.load_status_timeout_sec,
            )
            actions.append({"port_c_enable_after_backup": set_port_c_power(args.isolapurr_url, True)})
            time.sleep(args.baseline_seconds)
            sleep_and_collect(
                points,
                SampleContext(
                    tag=f"restored_after_backup_{backup_load}",
                    load_ma=backup_load,
                    hold_seconds=0.0,
                ),
                args=args,
                ups_status_url=args.ups_status_url,
                devd_url=args.devd_url,
                devd_target_ids=devd_target_ids,
                isolapurr_url=args.isolapurr_url,
                load_device=args.load_device,
                include_trace=args.include_trace,
                load_status_timeout_sec=args.load_status_timeout_sec,
            )

        if args.neighbor_verify:
            actions.append(
                {
                        "disable_before_neighbor_verify": disable_load(
                            args,
                            args.load_device,
                            timeout_sec=args.load_command_timeout_sec,
                            status_timeout_sec=args.load_status_timeout_sec,
                            verify_timeout_sec=args.load_verify_timeout_sec,
                        )
                    }
                )
            time.sleep(args.baseline_seconds)
            sleep_and_collect(
                points,
                SampleContext(tag="neighbor_baseline", load_ma=0, hold_seconds=0.0),
                args=args,
                ups_status_url=args.ups_status_url,
                devd_url=args.devd_url,
                devd_target_ids=devd_target_ids,
                isolapurr_url=args.isolapurr_url,
                load_device=args.load_device,
                include_trace=args.include_trace,
                load_status_timeout_sec=args.load_status_timeout_sec,
            )
            for load_ma in neighbor_loads:
                apply_load_and_sample(
                    args=args,
                    load_ma=load_ma,
                    hold_seconds=args.long_hold_seconds,
                    load_device=args.load_device,
                    max_i_ma_total=args.max_i_ma_total,
                    max_p_mw=args.max_p_mw,
                    points=points,
                    actions=actions,
                    ups_status_url=args.ups_status_url,
                    devd_url=args.devd_url,
                    devd_target_ids=devd_target_ids,
                    isolapurr_url=args.isolapurr_url,
                    include_trace=args.include_trace,
                    load_status_timeout_sec=args.load_status_timeout_sec,
                    load_command_timeout_sec=args.load_command_timeout_sec,
                    load_verify_timeout_sec=args.load_verify_timeout_sec,
                )
                actions.append(
                    {
                        "disable_after_neighbor_point": disable_load(
                            args,
                            args.load_device,
                            timeout_sec=args.load_command_timeout_sec,
                            status_timeout_sec=args.load_status_timeout_sec,
                            verify_timeout_sec=args.load_verify_timeout_sec,
                        )
                    }
                )
                time.sleep(args.baseline_seconds)
                sleep_and_collect(
                    points,
                    SampleContext(tag=f"neighbor_recovered_{load_ma}", load_ma=0, hold_seconds=0.0),
                    args=args,
                    ups_status_url=args.ups_status_url,
                    devd_url=args.devd_url,
                    devd_target_ids=devd_target_ids,
                    isolapurr_url=args.isolapurr_url,
                    load_device=args.load_device,
                    include_trace=args.include_trace,
                    load_status_timeout_sec=args.load_status_timeout_sec,
                )

        actions.append(
            {
                "disable_after_sweep": disable_load(
                    args,
                    args.load_device,
                    timeout_sec=args.load_command_timeout_sec,
                    status_timeout_sec=args.load_status_timeout_sec,
                    verify_timeout_sec=args.load_verify_timeout_sec,
                )
            }
        )
        time.sleep(args.baseline_seconds)
        sleep_and_collect(
            points,
            SampleContext(tag="recovered", load_ma=0, hold_seconds=0.0),
            args=args,
            ups_status_url=args.ups_status_url,
            devd_url=args.devd_url,
            devd_target_ids=devd_target_ids,
            isolapurr_url=args.isolapurr_url,
            load_device=args.load_device,
            include_trace=args.include_trace,
            load_status_timeout_sec=args.load_status_timeout_sec,
        )
    except (
        subprocess.CalledProcessError,
        subprocess.TimeoutExpired,
        urllib.error.URLError,
        RuntimeError,
    ) as exc:
        failure = {
            "metadata": metadata,
            "actions": actions,
            "points": points,
            "error": repr(exc),
        }
        (run_dir / "failure.json").write_text(
            json.dumps(failure, ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
        )
        print(json.dumps(failure, ensure_ascii=False, indent=2))
        return 1

    summary = [summarize_point(point) for point in points]
    payload = {
        "metadata": metadata,
        "actions": actions,
        "points": points,
        "summary": summary,
    }
    (run_dir / "results.json").write_text(
        json.dumps(payload, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    (run_dir / "summary.json").write_text(
        json.dumps(summary, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(payload, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
