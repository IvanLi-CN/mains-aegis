#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
import urllib.request
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


DEFAULT_LOAD_CLI = "loadlynx"
DEFAULT_UPS_STATUS_URL = os.environ.get("MAINS_AEGIS_UPS_STATUS_URL")
DEFAULT_DEVD_DIAG_SNAPSHOT_URL = os.environ.get("MAINS_AEGIS_DEVD_DIAG_SNAPSHOT_URL")
DEFAULT_ISOLAPURR_URL = os.environ.get("MAINS_AEGIS_ISOLAPURR_URL")
DEFAULT_LOADS = "3000,3025,3050,3900"
DEFAULT_HOLD_SECONDS = 8.0
DEFAULT_BASELINE_SECONDS = 2.5
DEFAULT_COMMAND_TIMEOUT_SECONDS = 45.0
DEFAULT_STATUS_TIMEOUT_SECONDS = 20.0
DEFAULT_VERIFY_TIMEOUT_SECONDS = 45.0
DEFAULT_MAX_I_MA_TOTAL = 3900
DEFAULT_MAX_P_MW = 45000


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Capture IsolaPurr source telemetry and UPS VIN/VOUT keypoints for 12V DCIN tests."
    )
    parser.add_argument("--profile-name", required=True)
    parser.add_argument("--loads-ma", default=DEFAULT_LOADS)
    parser.add_argument("--load-device", default=os.environ.get("MAINS_AEGIS_LOAD_DEVICE_ID"))
    parser.add_argument("--load-usb-port", default=os.environ.get("MAINS_AEGIS_LOAD_USB_PORT"))
    parser.add_argument("--load-cli", default=DEFAULT_LOAD_CLI)
    parser.add_argument("--ups-status-url", default=DEFAULT_UPS_STATUS_URL)
    parser.add_argument("--devd-diag-snapshot-url", default=DEFAULT_DEVD_DIAG_SNAPSHOT_URL)
    parser.add_argument("--isolapurr-url", default=DEFAULT_ISOLAPURR_URL)
    parser.add_argument("--hold-seconds", type=float, default=DEFAULT_HOLD_SECONDS)
    parser.add_argument("--baseline-seconds", type=float, default=DEFAULT_BASELINE_SECONDS)
    parser.add_argument("--command-timeout-sec", type=float, default=DEFAULT_COMMAND_TIMEOUT_SECONDS)
    parser.add_argument("--status-timeout-sec", type=float, default=DEFAULT_STATUS_TIMEOUT_SECONDS)
    parser.add_argument("--verify-timeout-sec", type=float, default=DEFAULT_VERIFY_TIMEOUT_SECONDS)
    parser.add_argument("--max-i-ma-total", type=int, default=DEFAULT_MAX_I_MA_TOTAL)
    parser.add_argument("--max-p-mw", type=int, default=DEFAULT_MAX_P_MW)
    parser.add_argument("--report-root", default="tools/hil/reports")
    args = parser.parse_args()
    for name, option in (
        ("load_device", "--load-device or MAINS_AEGIS_LOAD_DEVICE_ID"),
        ("load_usb_port", "--load-usb-port or MAINS_AEGIS_LOAD_USB_PORT"),
        ("ups_status_url", "--ups-status-url or MAINS_AEGIS_UPS_STATUS_URL"),
        ("devd_diag_snapshot_url", "--devd-diag-snapshot-url or MAINS_AEGIS_DEVD_DIAG_SNAPSHOT_URL"),
        ("isolapurr_url", "--isolapurr-url or MAINS_AEGIS_ISOLAPURR_URL"),
    ):
        if not (getattr(args, name, None) or "").strip():
            parser.error(f"capture requires {option}; no hardware target is built in")
    return args


def parse_loads(csv: str) -> list[int]:
    loads = [int(value.strip()) for value in csv.split(",") if value.strip()]
    if not loads:
        raise SystemExit("at least one load value is required")
    return loads


def run(cmd: list[str], *, timeout_sec: float | None = None) -> subprocess.CompletedProcess[str]:
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


def http_json(url: str) -> Any:
    with urllib.request.urlopen(url, timeout=10) as response:
        return json.load(response)


def ensure_usb_port(args: argparse.Namespace, load_device: str, load_usb_port: str) -> dict[str, Any]:
    completed = run([args.load_cli, "devices", "--json"])
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
        "device_id": load_device,
        "usb_port": actual_port,
        "last_transport": matched.get("last_transport"),
    }


def get_load_status(args: argparse.Namespace, load_device: str, *, timeout_sec: float) -> Any:
    completed = run([args.load_cli, "status", "--device", load_device, "--json"], timeout_sec=timeout_sec)
    return json.loads(completed.stdout)


def get_load_status_best_effort(
    args: argparse.Namespace, load_device: str, *, timeout_sec: float
) -> Any:
    try:
        return get_load_status(args, load_device, timeout_sec=timeout_sec)
    except (subprocess.TimeoutExpired, subprocess.CalledProcessError, json.JSONDecodeError) as exc:
        return {
            "ok": False,
            "error": repr(exc),
        }


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
            "ok": False,
            "error": repr(exc),
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
        last_status = get_load_status_best_effort(args, load_device, timeout_sec=status_timeout_sec)
        if isinstance(last_control, dict) and last_control.get("ok") is False:
            time.sleep(1.0)
            continue
        enabled = load_output_enabled(last_control)
        target_i_ma = load_target_i_ma(last_control)
        if enabled is None:
            enabled = load_output_enabled(last_status)
        if target_i_ma is None:
            target_i_ma = load_target_i_ma(last_status)
        if enabled is None:
            time.sleep(1.0)
            continue
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
    status = get_load_status_best_effort(args, load_device, timeout_sec=status_timeout_sec)
    if load_output_enabled(control) is False:
        return {
            "cmd": None,
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


def capture_point(
    *,
    args: argparse.Namespace,
    tag: str,
    load_ma: int,
    hold_seconds: float,
    load_device: str,
    ups_status_url: str,
    devd_diag_snapshot_url: str,
    isolapurr_url: str,
    status_timeout_sec: float,
) -> dict[str, Any]:
    time.sleep(hold_seconds)
    return {
        "tag": tag,
        "load_ma": load_ma,
        "hold_seconds": hold_seconds,
        "timestamp_utc": datetime.now(timezone.utc).isoformat(),
        "ups_status": http_json(ups_status_url),
        "diag_snapshot": http_json(devd_diag_snapshot_url),
        "isolapurr_power": json.loads(
            run(
                ["isolapurr", "power", "show", "--url", isolapurr_url, "--json"],
                timeout_sec=status_timeout_sec,
            ).stdout
        ),
        "load_control": get_load_control_best_effort(
            args,
            load_device, timeout_sec=status_timeout_sec
        ),
        "load_status": get_load_status_best_effort(args, load_device, timeout_sec=status_timeout_sec),
    }


def summarize_point(point: dict[str, Any]) -> dict[str, Any]:
    status = point["ups_status"]
    diag = unwrap_diag_snapshot_payload(point["diag_snapshot"])
    load_status = point.get("load_status") or {}
    port_c = None
    for port in point["isolapurr_power"].get("ports", {}).get("ports", []):
        if port.get("portId") == "port_c":
            port_c = port.get("telemetry")
            break
    return {
        "tag": point["tag"],
        "load_ma": point["load_ma"],
        "mode": status["mode"],
        "assist_power_stage": status["input"].get("assist_power_stage"),
        "isolapurr_port_c_mv": (port_c or {}).get("voltage_mv"),
        "isolapurr_port_c_ma": (port_c or {}).get("current_ma"),
        "vin_vbus_mv": status["input"].get("vin_vbus_mv"),
        "vin_iin_ma": status["input"].get("vin_iin_ma"),
        "assist_target_vout_mv": status["input"].get("assist_target_vout_mv"),
        "tps_total_iout_ma": status["input"].get("tps_total_iout_ma"),
        "out_a_vbus_mv": status["output"]["out_a"].get("vbus_mv"),
        "out_a_iout_ma": status["output"]["out_a"].get("iout_ma"),
        "out_b_vbus_mv": (status.get("output") or {}).get("out_b", {}).get("vbus_mv"),
        "out_b_iout_ma": (status.get("output") or {}).get("out_b", {}).get("iout_ma"),
        "ups_vout_mv": status["output"]["out_a"].get("vbus_mv")
        if isinstance(status["output"]["out_a"].get("vbus_mv"), (int, float))
        else (status.get("output") or {}).get("out_b", {}).get("vbus_mv"),
        "battery_current_ma": status["battery"].get("current_ma"),
        "diag_vin_vbus_mv": diag["input"].get("vin_vbus_mv"),
        "diag_vin_iin_ma": diag["input"].get("vin_iin_ma"),
        "diag_assist_target_vout_mv": diag["input"].get("assist_target_vout_mv"),
        "diag_tps_total_iout_ma": diag["input"].get("tps_total_iout_ma"),
        "load_output_enabled": load_output_enabled(load_status),
        "load_target_i_ma": load_target_i_ma(load_status),
        "load_v_local_mv": ((load_status.get("status") or {}).get("v_local_mv")),
        "load_i_local_ma": ((load_status.get("status") or {}).get("i_local_ma")),
        "load_i_remote_ma": ((load_status.get("status") or {}).get("i_remote_ma")),
        "load_i_total_ma": load_status_i_total_ma(load_status),
        "load_calc_p_mw": ((load_status.get("status") or {}).get("calc_p_mw")),
    }


def write_json(path: Path, payload: Any) -> None:
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


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


def main() -> int:
    args = parse_args()
    loads = parse_loads(args.loads_ma)
    report_root = Path(args.report_root)
    timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    run_dir = report_root / f"{timestamp}-{args.profile_name}"
    run_dir.mkdir(parents=True, exist_ok=False)

    metadata = {
        "profile_name": args.profile_name,
        "started_at_utc": datetime.now(timezone.utc).isoformat(),
        "loads_ma": loads,
        "load_device": args.load_device,
        "load_usb_port": args.load_usb_port,
        "load_cli": args.load_cli,
        "ups_status_url": args.ups_status_url,
        "devd_diag_snapshot_url": args.devd_diag_snapshot_url,
        "isolapurr_url": args.isolapurr_url,
        "hold_seconds": args.hold_seconds,
        "baseline_seconds": args.baseline_seconds,
        "command_timeout_sec": args.command_timeout_sec,
        "status_timeout_sec": args.status_timeout_sec,
        "verify_timeout_sec": args.verify_timeout_sec,
        "max_i_ma_total": args.max_i_ma_total,
        "max_p_mw": args.max_p_mw,
    }
    actions: list[dict[str, Any]] = []
    points: list[dict[str, Any]] = []

    def persist_progress(filename: str = "progress.json") -> None:
        payload = {
            "metadata": metadata,
            "actions": actions,
            "points": points,
            "summary": [summarize_point(point) for point in points],
        }
        write_json(run_dir / filename, payload)

    try:
        actions.append({"ensure_usb_port": ensure_usb_port(args, args.load_device, args.load_usb_port)})
        persist_progress()

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
        persist_progress()
        time.sleep(args.baseline_seconds)

        points.append(
            capture_point(
                tag="baseline",
                args=args,
                load_ma=0,
                hold_seconds=0.0,
                load_device=args.load_device,
                ups_status_url=args.ups_status_url,
                devd_diag_snapshot_url=args.devd_diag_snapshot_url,
                isolapurr_url=args.isolapurr_url,
                status_timeout_sec=args.status_timeout_sec,
            )
        )
        persist_progress()

        for load_ma in loads:
            actions.append(
                {
                    f"cc_{load_ma}": load_cc(
                        args,
                        args.load_device,
                        load_ma,
                        max_i_ma_total=args.max_i_ma_total,
                        max_p_mw=args.max_p_mw,
                        timeout_sec=args.command_timeout_sec,
                        status_timeout_sec=args.status_timeout_sec,
                        verify_timeout_sec=args.verify_timeout_sec,
                    )
                }
            )
            persist_progress()
            points.append(
                capture_point(
                    tag=f"cc_{load_ma}",
                    args=args,
                    load_ma=load_ma,
                    hold_seconds=args.hold_seconds,
                    load_device=args.load_device,
                    ups_status_url=args.ups_status_url,
                    devd_diag_snapshot_url=args.devd_diag_snapshot_url,
                    isolapurr_url=args.isolapurr_url,
                    status_timeout_sec=args.status_timeout_sec,
                )
            )
            persist_progress()
            actions.append(
                {
                    f"disable_after_{load_ma}": disable_load(
                        args,
                        args.load_device,
                        timeout_sec=args.command_timeout_sec,
                        status_timeout_sec=args.status_timeout_sec,
                        verify_timeout_sec=args.verify_timeout_sec,
                    )
                }
            )
            persist_progress()
            time.sleep(args.baseline_seconds)
            points.append(
                capture_point(
                    tag=f"recovered_{load_ma}",
                    args=args,
                    load_ma=0,
                    hold_seconds=0.0,
                    load_device=args.load_device,
                    ups_status_url=args.ups_status_url,
                    devd_diag_snapshot_url=args.devd_diag_snapshot_url,
                    isolapurr_url=args.isolapurr_url,
                    status_timeout_sec=args.status_timeout_sec,
                )
            )
            persist_progress()
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired, RuntimeError, KeyboardInterrupt) as exc:
        try:
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
        payload = {
            "metadata": metadata,
            "actions": actions,
            "points": points,
            "summary": [summarize_point(point) for point in points],
            "error": repr(exc),
        }
        write_json(run_dir / "failure.json", payload)
        print(json.dumps(payload, ensure_ascii=False, indent=2))
        return 1

    payload = {
        "metadata": metadata,
        "actions": actions,
        "points": points,
        "summary": [summarize_point(point) for point in points],
    }
    write_json(run_dir / "results.json", payload)
    write_json(run_dir / "summary.json", payload["summary"])
    print(json.dumps(payload, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
