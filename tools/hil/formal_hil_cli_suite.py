#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import signal
import subprocess
import sys
import threading
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parent
DEFAULT_REPORT_ROOT = ROOT / "reports"
DEFAULT_UPS_CLI = str(ROOT.parent / "mains-aegis-host" / "target" / "debug" / "mains-aegis")
DEFAULT_UPS_IPC = str(ROOT.parent.parent / ".tmp" / "mains-aegis-devd-hil.sock")
DEFAULT_UPS_DEVICE_ID = os.environ.get("MAINS_AEGIS_UPS_DEVICE_ID")
DEFAULT_LOAD_CLI = os.environ.get("LOADLYNX_CLI")
DEFAULT_LOAD_IPC = str(ROOT.parent.parent / ".tmp" / "loadlynx-devd-hil.sock")
DEFAULT_LOAD_DEVICE = os.environ.get("MAINS_AEGIS_LOAD_DEVICE_ID")
DEFAULT_ISOLAPURR_CLI = "isolapurr"
DEFAULT_ISOLAPURR_DEVICE_ID = os.environ.get("MAINS_AEGIS_POWER_DEVICE_ID")
MIN_SAMPLE_RATE_HZ = 3.0
MAX_SAMPLE_GAP_S = 0.5
UPS_WATCH_FRESHNESS_MS = 750
SOURCE_VOLTAGE_TOLERANCE_MV = 800
UPS_VIN_TOLERANCE_MV = 1500

PROFILES = {
    "12v": {"source_voltage_mv": 12000, "source_current_limit_ma": 3000, "rated_vout_mv": 12000},
    "19v": {"source_voltage_mv": 19000, "source_current_limit_ma": 3000, "rated_vout_mv": 19000},
}
SCENES = {
    "assist_path": {"target_ma": 3900, "include_backup": True},
    "backup_only": {"target_ma": 1000, "include_backup": True},
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="CLI/IPC formal HIL suite for UPS + LoadLynx.")
    parser.add_argument("--report-root", default=str(DEFAULT_REPORT_ROOT))
    parser.add_argument("--suite-id", default=None)
    parser.add_argument("--profiles", nargs="+", choices=sorted(PROFILES), default=["12v", "19v"])
    parser.add_argument("--scenes", nargs="+", choices=sorted(SCENES), default=["assist_path", "backup_only"])
    parser.add_argument("--ups-cli", default=DEFAULT_UPS_CLI)
    parser.add_argument("--ups-ipc", default=DEFAULT_UPS_IPC)
    parser.add_argument("--ups-device-id", default=os.environ.get("MAINS_AEGIS_UPS_DEVICE_ID"))
    parser.add_argument("--artifact-manifest-12v", default=None)
    parser.add_argument("--artifact-manifest-19v", default=None)
    parser.add_argument("--load-cli", default=DEFAULT_LOAD_CLI)
    parser.add_argument("--load-ipc", default=DEFAULT_LOAD_IPC)
    parser.add_argument("--load-device", default=os.environ.get("MAINS_AEGIS_LOAD_DEVICE_ID"))
    parser.add_argument("--isolapurr-cli", default=DEFAULT_ISOLAPURR_CLI)
    parser.add_argument("--isolapurr-device-id", default=os.environ.get("MAINS_AEGIS_POWER_DEVICE_ID"))
    parser.add_argument("--sample-interval-s", type=float, default=0.2)
    parser.add_argument("--pre-s", type=float, default=8.0)
    parser.add_argument("--hold-s", type=float, default=16.0)
    parser.add_argument("--backup-s", type=float, default=12.0)
    parser.add_argument("--restore-s", type=float, default=12.0)
    parser.add_argument("--post-s", type=float, default=8.0)
    parser.add_argument("--load-min-v-mv", type=int, default=3000)
    parser.add_argument("--load-max-i-ma-total", type=int, default=4000)
    parser.add_argument("--load-max-p-mw", type=int, default=80000)
    parser.add_argument("--render-chart", default=str(ROOT / "render_voltage_chart_html.py"))
    parser.add_argument("--render-overview", default=str(ROOT / "render_formal_suite_html.py"))
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    if not args.load_cli:
        parser.error("formal HIL requires --load-cli or LOADLYNX_CLI; do not rely on a released PATH default")
    for name, option in (
        ("ups_device_id", "--ups-device-id or MAINS_AEGIS_UPS_DEVICE_ID"),
        ("load_device", "--load-device or MAINS_AEGIS_LOAD_DEVICE_ID"),
        ("isolapurr_device_id", "--isolapurr-device-id or MAINS_AEGIS_POWER_DEVICE_ID"),
    ):
        if not (getattr(args, name, None) or "").strip():
            parser.error(f"formal HIL requires {option}; no hardware device id is built in")
    return args


def run(cmd: list[str], *, timeout: float = 20.0, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, check=True, text=True, capture_output=True, timeout=timeout, env=env)


def command_error(exc: subprocess.CalledProcessError) -> dict[str, Any]:
    return {
        "returncode": exc.returncode,
        "stdout": (exc.stdout or "").strip(),
        "stderr": (exc.stderr or "").strip(),
    }


def run_json(cmd: list[str], *, timeout: float = 20.0, env: dict[str, str] | None = None) -> Any:
    text = run(cmd, timeout=timeout, env=env).stdout.strip()
    return json.loads(text) if text else {}


def write_json(path: Path, payload: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def ups_env(args: argparse.Namespace) -> dict[str, str]:
    env = os.environ.copy()
    env["MAINS_AEGIS_DEVD_IPC"] = args.ups_ipc
    return env


def ups_cmd(args: argparse.Namespace, *parts: str) -> list[str]:
    return [args.ups_cli, "--ipc", args.ups_ipc, "device", args.ups_device_id, *parts]


def load_cmd(args: argparse.Namespace, *parts: str) -> list[str]:
    return [args.load_cli, "--ipc", args.load_ipc, *parts]


def isolapurr_cmd(args: argparse.Namespace, *parts: str) -> list[str]:
    return [args.isolapurr_cli, "--json", *parts]


def manifest_for_profile(args: argparse.Namespace, profile_key: str) -> str | None:
    if profile_key == "12v":
        return args.artifact_manifest_12v
    if profile_key == "19v":
        return args.artifact_manifest_19v
    return None


def ensure_load_disabled(args: argparse.Namespace, *, dry_run: bool) -> dict[str, Any]:
    cmd = load_cmd(args, "control", "set", "--device", args.load_device, "--disable", "--json")
    return {"cmd": cmd, "dry_run": True} if dry_run else {"cmd": cmd, "result": run_json(cmd)}


def set_load_cc(args: argparse.Namespace, target_ma: int, *, dry_run: bool) -> dict[str, Any]:
    cmd = load_cmd(
        args,
        "--json",
        "cc",
        str(target_ma),
        "--device",
        args.load_device,
        "--min-v-mv",
        str(args.load_min_v_mv),
        "--max-i-ma-total",
        str(args.load_max_i_ma_total),
        "--max-p-mw",
        str(args.load_max_p_mw),
    )
    return {"cmd": cmd, "dry_run": True} if dry_run else {"cmd": cmd, "result": run_json(cmd)}


def validate_loadlynx_cli(args: argparse.Namespace, *, dry_run: bool) -> dict[str, Any]:
    cmd = [args.load_cli, "--help"]
    if dry_run:
        return {"cmd": cmd, "dry_run": True}
    proc = run(cmd, timeout=5.0)
    supports_stream = "status-stream" in proc.stdout
    return {"cmd": cmd, "ok": supports_stream, "supports_status_stream": supports_stream}


def set_isolapurr_output(args: argparse.Namespace, enabled: bool, *, dry_run: bool) -> dict[str, Any]:
    if enabled:
        cmd = isolapurr_cmd(
            args,
            "power",
            "output",
            "manual",
            "--device-id",
            args.isolapurr_device_id,
            "--voltage-mv",
            str(args._active_source_voltage_mv),
            "--current-limit-ma",
            str(args._active_source_current_limit_ma),
            "--usb-c-path",
            "forced-on",
        )
    else:
        cmd = isolapurr_cmd(args, "power", "output", "auto", "--device-id", args.isolapurr_device_id)
    if dry_run:
        return {"cmd": cmd, "dry_run": True}
    result = run_json(cmd)
    verification = wait_isolapurr_output_state(args, enabled)
    if not verification["ok"]:
        raise RuntimeError(f"IsolaPurr output {enabled} command returned but readback failed: {verification}")
    return {"cmd": cmd, "result": result, "verified": verification}


def configure_isolapurr_manual(args: argparse.Namespace, profile: dict[str, int], *, dry_run: bool) -> dict[str, Any]:
    args._active_source_voltage_mv = profile["source_voltage_mv"]
    args._active_source_current_limit_ma = profile["source_current_limit_ma"]
    cmd = isolapurr_cmd(
        args,
        "power",
        "config",
        "set",
        "--device-id",
        args.isolapurr_device_id,
        "--tps-mode",
        "manual",
        "--voltage-mv",
        str(profile["source_voltage_mv"]),
        "--current-limit-ma",
        str(profile["source_current_limit_ma"]),
        "--usb-c-path",
        "disconnected",
    )
    if dry_run:
        return {"cmd": cmd, "dry_run": True}
    result = run_json(cmd)
    # Some IsolaPurr firmware revisions keep the previous live PD output while
    # accepting a manual target/path config. The HIL safety contract requires
    # source-off to be an explicit action, not an inferred side effect of config.
    off = set_isolapurr_output(args, False, dry_run=False)
    verification = wait_isolapurr_output_state(args, False)
    if not verification["ok"]:
        raise RuntimeError(
            f"IsolaPurr manual config command returned but output-off readback failed: {verification}"
        )
    return {"cmd": cmd, "result": result, "source_off": off, "verified": verification}


def read_isolapurr_ports(args: argparse.Namespace, *, dry_run: bool) -> dict[str, Any]:
    cmd = isolapurr_cmd(args, "ports", "--device-id", args.isolapurr_device_id)
    return {"cmd": cmd, "dry_run": True} if dry_run else {"cmd": cmd, "result": run_json(cmd)}


def read_isolapurr_power(args: argparse.Namespace, *, dry_run: bool) -> dict[str, Any]:
    cmd = isolapurr_cmd(args, "power", "show", "--device-id", args.isolapurr_device_id)
    return {"cmd": cmd, "dry_run": True} if dry_run else {"cmd": cmd, "result": run_json(cmd)}


def port_c_from_ports(payload: Any) -> dict[str, Any]:
    for port in (payload or {}).get("ports") or []:
        if isinstance(port, dict) and port.get("portId") == "port_c":
            return port
    return {}


def isolapurr_live_voltage_mv(power_payload: Any, ports_payload: Any) -> int | None:
    diagnostics = dict((power_payload or {}).get("diagnostics") or {})
    sw2303_vbus_mv = diagnostics.get("sw2303_vbus_mv")
    if isinstance(sw2303_vbus_mv, int) and sw2303_vbus_mv > 0:
        return sw2303_vbus_mv
    port_c = port_c_from_ports(ports_payload)
    telemetry = dict(port_c.get("telemetry") or {})
    port_voltage_mv = telemetry.get("voltage_mv")
    if isinstance(port_voltage_mv, int) and port_voltage_mv > 0:
        return port_voltage_mv
    actual = dict(diagnostics.get("usb_c_actual") or {})
    actual_voltage_mv = actual.get("voltage_mv")
    if isinstance(actual_voltage_mv, int) and actual_voltage_mv > 0:
        return actual_voltage_mv
    return None


def verify_isolapurr_output_state(args: argparse.Namespace, enabled: bool) -> dict[str, Any]:
    power = read_isolapurr_power(args, dry_run=False)
    ports = read_isolapurr_ports(args, dry_run=False)
    power_payload = power.get("result")
    ports_payload = ports.get("result")
    port = port_c_from_ports(ports.get("result"))
    live_voltage_mv = isolapurr_live_voltage_mv(power_payload, ports_payload)
    if enabled:
        expected = getattr(args, "_active_source_voltage_mv", None)
        ok = isinstance(live_voltage_mv, int) and isinstance(expected, int) and abs(live_voltage_mv - expected) <= SOURCE_VOLTAGE_TOLERANCE_MV
    else:
        ok = live_voltage_mv is None
    return {"ok": ok, "expected_enabled": enabled, "live_voltage_mv": live_voltage_mv, "port_c": port, "ports": ports, "power": power}


def wait_isolapurr_output_state(args: argparse.Namespace, enabled: bool, *, timeout_s: float = 5.0) -> dict[str, Any]:
    deadline = time.monotonic() + timeout_s
    last: dict[str, Any] | None = None
    while time.monotonic() < deadline:
        last = verify_isolapurr_output_state(args, enabled)
        if last["ok"]:
            return last
        time.sleep(0.1)
    return last or {"ok": False, "expected_enabled": enabled}


def validate_source_config(profile: dict[str, int], power_payload: Any, ports_payload: Any) -> dict[str, Any]:
    config = dict((power_payload or {}).get("config") or {})
    manual = dict(config.get("manual") or {})
    port_c = port_c_from_ports(ports_payload)
    live_voltage_mv = isolapurr_live_voltage_mv(power_payload, ports_payload)
    failures: list[str] = []
    if manual.get("voltage_mv") != profile["source_voltage_mv"]:
        failures.append("isolapurr_manual_voltage_mismatch")
    if manual.get("current_limit_ma") != profile["source_current_limit_ma"]:
        failures.append("isolapurr_manual_current_limit_mismatch")
    if live_voltage_mv is not None:
        failures.append("isolapurr_source_not_cut")
    return {"ok": not failures, "failures": failures, "manual": manual, "port_c": port_c, "live_voltage_mv": live_voltage_mv}


def validate_source_online(profile: dict[str, int], ports_payload: Any) -> dict[str, Any]:
    port_c = port_c_from_ports(ports_payload)
    telemetry = dict(port_c.get("telemetry") or {})
    voltage_mv = telemetry.get("voltage_mv")
    failures: list[str] = []
    if not isinstance(voltage_mv, int) or abs(voltage_mv - profile["source_voltage_mv"]) > SOURCE_VOLTAGE_TOLERANCE_MV:
        failures.append("isolapurr_source_voltage_not_in_profile_window")
    return {"ok": not failures, "failures": failures, "port_c": port_c, "expected_voltage_mv": profile["source_voltage_mv"]}


def read_ups_identity_settings(args: argparse.Namespace, *, dry_run: bool) -> tuple[dict[str, Any], dict[str, Any]]:
    identity_cmd = ups_cmd(args, "identity")
    settings_cmd = ups_cmd(args, "settings")
    if dry_run:
        return {"cmd": identity_cmd, "dry_run": True}, {"cmd": settings_cmd, "dry_run": True}
    return {"cmd": identity_cmd, "result": run_json(identity_cmd, env=ups_env(args))}, {
        "cmd": settings_cmd,
        "result": run_json(settings_cmd, env=ups_env(args)),
    }


def select_ups_artifact(args: argparse.Namespace, manifest_path: str, *, dry_run: bool) -> dict[str, Any]:
    cmd = ups_cmd(args, "artifact", "select", "--manifest-path", manifest_path)
    return {"cmd": cmd, "dry_run": True} if dry_run else {"cmd": cmd, "result": run_json(cmd, timeout=30.0, env=ups_env(args))}


def flash_ups_artifact(args: argparse.Namespace, *, real: bool, dry_run: bool) -> dict[str, Any]:
    flag = "--real" if real else "--dry-run"
    cmd = ups_cmd(args, "flash", flag)
    return {"cmd": cmd, "dry_run": True} if dry_run else {"cmd": cmd, "result": run_json(cmd, timeout=180.0, env=ups_env(args))}


def read_ups_status(args: argparse.Namespace, *, dry_run: bool) -> dict[str, Any]:
    cmd = ups_cmd(args, "status", "--include-meta")
    if dry_run:
        return {"cmd": cmd, "dry_run": True}
    try:
        return {"cmd": cmd, "result": run_json(cmd, env=ups_env(args))}
    except subprocess.CalledProcessError as exc:
        return {"cmd": cmd, "error": command_error(exc)}


def read_ups_status_cache(args: argparse.Namespace, *, dry_run: bool) -> dict[str, Any]:
    cmd = ups_cmd(args, "status", "--cache-only", "--allow-stale-cache", "--include-meta")
    if dry_run:
        return {"cmd": cmd, "dry_run": True}
    return {"cmd": cmd, "result": run_json(cmd, env=ups_env(args))}


def read_ups_diag_snapshot_cache(args: argparse.Namespace, *, dry_run: bool) -> dict[str, Any]:
    cmd = ups_cmd(args, "diag-snapshot", "--cache-only", "--allow-stale-cache", "--include-meta")
    if dry_run:
        return {"cmd": cmd, "dry_run": True}
    return {"cmd": cmd, "result": run_json(cmd, env=ups_env(args))}


def start_ups_monitor(args: argparse.Namespace, *, dry_run: bool) -> dict[str, Any]:
    cmd = ups_cmd(args, "monitor", "start")
    if dry_run:
        return {"cmd": cmd, "dry_run": True}
    try:
        return {"cmd": cmd, "result": run_json(cmd, env=ups_env(args))}
    except subprocess.CalledProcessError as exc:
        return {"cmd": cmd, "ok": False, "error": command_error(exc)}


def stop_ups_monitor(args: argparse.Namespace, *, dry_run: bool) -> dict[str, Any]:
    cmd = ups_cmd(args, "monitor", "stop")
    if dry_run:
        return {"cmd": cmd, "dry_run": True}
    return {"cmd": cmd, "result": run_json(cmd, env=ups_env(args))}


def restart_ups_monitor(args: argparse.Namespace, *, dry_run: bool) -> dict[str, Any]:
    actions: list[dict[str, Any]] = []
    try:
        actions.append({"stop": stop_ups_monitor(args, dry_run=dry_run)})
    except Exception as exc:
        actions.append({"stop_error": str(exc)})
    if not dry_run:
        time.sleep(1.0)
    actions.append({"start": start_ups_monitor(args, dry_run=dry_run)})
    return {"ok": True, "actions": actions}


def wait_ups_status_cache(args: argparse.Namespace, *, timeout_s: float = 8.0, dry_run: bool) -> dict[str, Any]:
    if dry_run:
        return {"ok": True, "dry_run": True}
    fresh = read_ups_status_fresh(args, dry_run=False)
    sample = unwrap_ups_sample(fresh.get("result"))
    if sample:
        return {"ok": True, "sample": sample, "fresh": fresh}
    return wait_for_gate(
        "ups_status_cache",
        lambda: unwrap_ups_sample(read_ups_status(args, dry_run=False).get("result")),
        lambda sample: {"ok": bool(sample), "sample": sample},
        timeout_s=timeout_s,
    )


def read_ups_status_fresh(args: argparse.Namespace, *, dry_run: bool) -> dict[str, Any]:
    cmd = ups_cmd(args, "status", "--fresh", "--include-meta")
    if dry_run:
        return {"cmd": cmd, "dry_run": True}
    try:
        return {"cmd": cmd, "result": run_json(cmd, env=ups_env(args))}
    except subprocess.CalledProcessError as exc:
        return {"cmd": cmd, "error": command_error(exc)}


def read_ups_watch_sample(args: argparse.Namespace, kind: str = "status", *, dry_run: bool) -> dict[str, Any]:
    cmd = ups_cmd(
        args,
        kind,
        "--watch",
        "--interval-ms",
        "200",
        "--watch-freshness-ms",
        str(UPS_WATCH_FRESHNESS_MS),
        "--include-meta",
        "--samples",
        "1",
    )
    if dry_run:
        return {"cmd": cmd, "dry_run": True}
    try:
        return {"cmd": cmd, "result": run_json(cmd, timeout=5.0, env=ups_env(args))}
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as exc:
        return {"cmd": cmd, "error": str(exc)}


def validate_ups_profile(profile_key: str, identity: Any, settings: Any) -> dict[str, Any]:
    expected = PROFILES[profile_key]["rated_vout_mv"]
    caps = dict((identity or {}).get("hardware_capabilities") or {})
    adv_caps = dict((settings or {}).get("advanced_power_capabilities") or {})
    failures: list[str] = []
    if caps.get("rated_vout_mv") != expected:
        failures.append("ups_identity_rated_vout_mismatch")
    if caps.get("output_profile") != profile_key:
        failures.append("ups_identity_profile_mismatch")
    if adv_caps.get("rated_vout_mv") != expected:
        failures.append("ups_settings_rated_vout_mismatch")
    return {"ok": not failures, "failures": failures, "identity_caps": caps, "settings_caps": adv_caps}


def current_ups_profile(identity: Any, settings: Any) -> str | None:
    identity_caps = dict((identity or {}).get("hardware_capabilities") or {})
    settings_caps = dict((settings or {}).get("advanced_power_capabilities") or {})
    profile = identity_caps.get("output_profile")
    rated_vout_mv = identity_caps.get("rated_vout_mv")
    settings_rated_vout_mv = settings_caps.get("rated_vout_mv")
    for key, info in PROFILES.items():
        if profile == key and rated_vout_mv == info["rated_vout_mv"] and settings_rated_vout_mv == info["rated_vout_mv"]:
            return key
    return None


def validate_ups_input_cut(status_sample: Any) -> dict[str, Any]:
    status = dict(status_sample or {})
    input_root = dict(status.get("input") or {})
    failures = []
    source = input_root.get("source")
    vin = input_root.get("vin_vbus_mv")
    if source == "dcin" and isinstance(vin, int) and vin > 2999:
        failures.append("ups_dcin_still_powered")
    if source == "dcin" and input_root.get("mains_present") is not False:
        failures.append("ups_dcin_mains_still_present")
    if source == "dcin" and not isinstance(vin, int):
        failures.append("ups_dcin_vin_missing")
    return {"ok": not failures, "failures": failures, "status": status}


def validate_ups_input_restored(profile_key: str, status_sample: Any) -> dict[str, Any]:
    expected = PROFILES[profile_key]["source_voltage_mv"]
    status = dict(status_sample or {})
    input_root = dict(status.get("input") or {})
    vin = input_root.get("vin_vbus_mv")
    failures = []
    if input_root.get("mains_present") is not True:
        failures.append("ups_mains_present_not_true")
    if not isinstance(vin, int) or abs(vin - expected) > UPS_VIN_TOLERANCE_MV:
        failures.append("ups_vin_not_in_profile_window")
    if status.get("mode") == "backup" or input_root.get("assist_power_stage") == "backup":
        failures.append("ups_still_backup_after_restore")
    return {"ok": not failures, "failures": failures, "status": status, "expected_vin_mv": expected}


def switch_profile_if_needed(args: argparse.Namespace, profile_key: str, suite_dir: Path) -> dict[str, Any]:
    actions: list[dict[str, Any]] = []
    manifest_path = manifest_for_profile(args, profile_key)
    if not manifest_path:
        raise RuntimeError(f"missing artifact manifest for {profile_key}; pass --artifact-manifest-{profile_key}")
    actions.append({"load_disable_before_profile_switch": ensure_load_disabled(args, dry_run=args.dry_run)})
    actions.append({"isolapurr_off_before_profile_switch": set_isolapurr_output(args, False, dry_run=args.dry_run)})
    actions.append({"ups_monitor_restart_profile_switch": restart_ups_monitor(args, dry_run=args.dry_run)})
    actions.append({"ups_status_cache_profile_switch": wait_ups_status_cache(args, timeout_s=20.0, dry_run=args.dry_run)})
    source_cut_gate = (
        wait_for_gate(
            "ups_profile_switch_source_cut",
            lambda: unwrap_ups_sample(read_ups_status(args, dry_run=False).get("result")),
            validate_ups_input_cut,
            timeout_s=20.0,
        )
        if not args.dry_run
        else {"ok": True}
    )
    actions.append({"ups_profile_switch_source_cut_gate": source_cut_gate})
    if not args.dry_run and not source_cut_gate["ok"]:
        raise RuntimeError(f"profile switch source cut gate failed: {source_cut_gate}")

    identity, settings = read_ups_identity_settings(args, dry_run=args.dry_run)
    actions.extend([{"ups_identity_before_profile_switch": identity}, {"ups_settings_before_profile_switch": settings}])
    current_profile = None if args.dry_run else current_ups_profile(identity.get("result"), settings.get("result"))
    actions.append({"current_profile_before_switch": {"profile": current_profile, "target": profile_key}})
    if current_profile != profile_key:
        actions.append({"artifact_select": select_ups_artifact(args, manifest_path, dry_run=args.dry_run)})
        actions.append({"flash_dry_run": flash_ups_artifact(args, real=False, dry_run=args.dry_run)})
        actions.append({"flash_real": flash_ups_artifact(args, real=True, dry_run=args.dry_run)})
        if not args.dry_run:
            time.sleep(6.0)
            actions.append({"ups_monitor_restart_after_flash": restart_ups_monitor(args, dry_run=False)})
            actions.append({"ups_status_cache_after_flash": wait_ups_status_cache(args, timeout_s=45.0, dry_run=False)})
    identity_after, settings_after = read_ups_identity_settings(args, dry_run=args.dry_run)
    actions.extend([{"ups_identity_after_profile_switch": identity_after}, {"ups_settings_after_profile_switch": settings_after}])
    profile_gate = (
        validate_ups_profile(profile_key, identity_after.get("result"), settings_after.get("result"))
        if not args.dry_run
        else {"ok": True}
    )
    actions.append({"ups_profile_gate_after_switch": profile_gate})
    if not args.dry_run and not profile_gate["ok"]:
        raise RuntimeError(f"profile switch verification failed: {profile_gate}")
    write_json(suite_dir / f"profile-switch-{profile_key}.json", {"target_profile": profile_key, "manifest_path": manifest_path, "actions": actions})
    return {"target_profile": profile_key, "manifest_path": manifest_path, "actions": actions}


def wait_for_gate(name: str, supplier, validator, *, timeout_s: float = 8.0, interval_s: float = 0.25, tick=None) -> dict[str, Any]:
    deadline = time.monotonic() + timeout_s
    last: dict[str, Any] | None = None
    while time.monotonic() < deadline:
        if tick is not None:
            tick()
        try:
            sample = supplier()
            verdict = validator(sample)
        except Exception as exc:
            sample = None
            verdict = {"ok": False, "failures": ["gate_supplier_error"], "error": str(exc)}
        last = {"sample": sample, "verdict": verdict}
        if verdict.get("ok"):
            return {"ok": True, "name": name, **last}
        time.sleep(interval_s)
    return {"ok": False, "name": name, **(last or {"sample": None, "verdict": {"ok": False, "failures": ["no_sample"]}})}


class JsonlCollector:
    def __init__(self, name: str, cmd: list[str], cwd: Path):
        self.name = name
        self.cmd = cmd
        self.cwd = cwd
        self.proc: subprocess.Popen[str] | None = None
        self.rows: list[dict[str, Any]] = []
        self.errors: list[str] = []
        self.summary: dict[str, Any] | None = None
        self._thread: threading.Thread | None = None

    def start(self) -> None:
        self.proc = subprocess.Popen(
            self.cmd,
            cwd=self.cwd,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        self._thread = threading.Thread(target=self._read_stdout, daemon=True)
        self._thread.start()

    def _read_stdout(self) -> None:
        assert self.proc is not None and self.proc.stdout is not None
        pending_summary: list[str] = []
        for line in self.proc.stdout:
            line = line.strip()
            if not line:
                continue
            if pending_summary:
                pending_summary.append(line)
                if line == "}":
                    try:
                        self.summary = json.loads("\n".join(pending_summary))
                    except json.JSONDecodeError:
                        self.errors.extend(pending_summary)
                    pending_summary = []
                continue
            try:
                payload = json.loads(line)
            except json.JSONDecodeError:
                if line == "{":
                    pending_summary = [line]
                    continue
                self.errors.append(line)
                continue
            if isinstance(payload, dict) and {"ok", "samples"}.issubset(payload.keys()):
                self.summary = payload
            else:
                self.rows.append(payload)
        if pending_summary:
            self.errors.extend(pending_summary)

    def stop(self) -> None:
        if self.proc is None:
            return
        if self.proc.poll() is None:
            self.proc.send_signal(signal.SIGINT)
            try:
                self.proc.wait(timeout=3)
            except subprocess.TimeoutExpired:
                self.proc.kill()
        if self._thread is not None:
            self._thread.join(timeout=2)
        if self.proc.stderr is not None:
            stderr = self.proc.stderr.read().strip()
            if stderr:
                self.errors.append(stderr)

    def latest_before(self, unix_ms: int) -> dict[str, Any] | None:
        best = None
        for row in self.rows:
            t = row_time_ms(row)
            if isinstance(t, int) and t <= unix_ms:
                best = row
        return best


class PollCollector:
    def __init__(self, name: str, interval_s: float, poll):
        self.name = name
        self.interval_s = interval_s
        self.poll = poll
        self.rows: list[dict[str, Any]] = []
        self.errors: list[str] = []
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None

    @property
    def cmd(self) -> list[str]:
        return [f"poll:{self.name}", f"interval_s={self.interval_s}"]

    def start(self) -> None:
        self._thread = threading.Thread(target=self._run, daemon=True)
        self._thread.start()

    def _run(self) -> None:
        while not self._stop.is_set():
            started_ms = int(time.time() * 1000)
            try:
                payload = self.poll()
                self.rows.append({"sample_received_at_ms": int(time.time() * 1000), "requested_at_ms": started_ms, "payload": payload})
            except Exception as exc:
                self.errors.append(str(exc))
            self._stop.wait(self.interval_s)

    def stop(self) -> None:
        self._stop.set()
        if self._thread is not None:
            self._thread.join(timeout=2)

    def latest_before(self, unix_ms: int) -> dict[str, Any] | None:
        best = None
        for row in self.rows:
            t = row_time_ms(row)
            if isinstance(t, int) and t <= unix_ms:
                best = row
        return best


def row_time_ms(row: dict[str, Any]) -> int | None:
    for key in ("sample_received_at_ms", "received_at_ms", "requested_at_ms"):
        value = row.get(key)
        if isinstance(value, int):
            return value
    return None


def unwrap_ups_sample(row: dict[str, Any] | None) -> dict[str, Any]:
    if not isinstance(row, dict):
        return {}
    if "input" in row or "mode" in row:
        return row
    sample = row.get("sample")
    if isinstance(sample, dict):
        return sample
    result = row.get("result")
    if isinstance(result, dict):
        if "input" in result or "mode" in result:
            return result
        sample = result.get("sample")
        if isinstance(sample, dict):
            return sample
    return {}


def unwrap_diag_snapshot(row: dict[str, Any] | None) -> dict[str, Any]:
    sample = unwrap_ups_sample(row)
    packages = sample.get("packages")
    if isinstance(packages, dict):
        derived = packages.get("derived.power")
        if isinstance(derived, dict):
            payload = derived.get("payload")
            if isinstance(payload, dict):
                return payload
    return sample


def unwrap_load(row: dict[str, Any] | None) -> dict[str, Any]:
    if not isinstance(row, dict):
        return {}
    payload = row.get("payload")
    return payload if isinstance(payload, dict) else row


def unwrap_isolapurr(row: dict[str, Any] | None) -> dict[str, Any]:
    if not isinstance(row, dict):
        return {}
    payload = row.get("payload")
    return payload if isinstance(payload, dict) else row


def collect_sample(args: argparse.Namespace, start: float, phase: str, target_ma: int, collectors: dict[str, JsonlCollector]) -> dict[str, Any]:
    now = time.time()
    unix_ms = int(now * 1000)
    status = unwrap_ups_sample(collectors["ups_status"].latest_before(unix_ms))
    diag_snapshot = unwrap_diag_snapshot(collectors["ups_diag_snapshot"].latest_before(unix_ms))
    load = unwrap_load(collectors["load"].latest_before(unix_ms))
    ports = unwrap_isolapurr(collectors["isolapurr"].latest_before(unix_ms))
    port_c = port_c_from_ports(ports)
    input_root = dict(status.get("input") or {})
    output = dict(status.get("output") or {})
    out_a = dict(output.get("out_a") or {})
    out_b = dict(output.get("out_b") or {})
    diag_input = dict(diag_snapshot.get("input") or {})
    load_status = dict(load.get("status") or {})
    load_control = dict(load.get("control") or {})
    telemetry = dict(port_c.get("telemetry") or {})
    state = dict(port_c.get("state") or {})
    return {
        "t_s": round(now - start, 3),
        "unix_ms": unix_ms,
        "phase": phase,
        "stage": input_root.get("assist_power_stage"),
        "mode": status.get("mode"),
        "load_target_i_ma": target_ma,
        "port_c_enabled": state.get("power_enabled"),
        "isolapurr_port_c_mv": telemetry.get("voltage_mv"),
        "isolapurr_port_c_ma": telemetry.get("current_ma"),
        "mains_present": input_root.get("mains_present"),
        "assist_target_vout_mv": input_root.get("assist_target_vout_mv"),
        "vin_vbus_mv": input_root.get("vin_vbus_mv"),
        "vin_iin_ma": input_root.get("vin_iin_ma"),
        "tps_total_iout_ma": input_root.get("tps_total_iout_ma"),
        "battery_current_ma": dict(status.get("battery") or {}).get("current_ma"),
        "out_a_vbus_mv": out_a.get("vbus_mv"),
        "out_b_vbus_mv": out_b.get("vbus_mv"),
        "out_a_iout_ma": out_a.get("iout_ma"),
        "out_b_iout_ma": out_b.get("iout_ma"),
        "diag_stage": diag_input.get("assist_power_stage"),
        "diag_assist_target_vout_mv": diag_input.get("assist_target_vout_mv"),
        "diag_vin_baseline_mv": diag_input.get("vin_baseline_mv"),
        "diag_vin_drop_mv": diag_input.get("vin_drop_mv"),
        "diag_tps_total_iout_ma": diag_input.get("tps_total_iout_ma"),
        "load_output_enabled": load_control.get("output_enabled"),
        "load_v_local_mv": load_status.get("v_local_mv"),
        "load_i_local_ma": load_status.get("i_local_ma"),
        "load_i_remote_ma": load_status.get("i_remote_ma"),
        "load_i_total_ma": sum(v for v in [load_status.get("i_local_ma"), load_status.get("i_remote_ma")] if isinstance(v, int)),
    }


def sleep_collect(args: argparse.Namespace, start: float, phase: str, target_ma: int, collectors: dict[str, JsonlCollector], samples: list[dict[str, Any]], duration_s: float) -> None:
    deadline = time.monotonic() + duration_s
    while time.monotonic() < deadline:
        samples.append(collect_sample(args, start, phase, target_ma, collectors))
        time.sleep(args.sample_interval_s)


def run_action_collecting(args: argparse.Namespace, start: float, phase: str, target_ma: int, collectors: dict[str, JsonlCollector], samples: list[dict[str, Any]], action):
    stop = threading.Event()

    def sample_loop() -> None:
        while not stop.is_set():
            samples.append(collect_sample(args, start, phase, target_ma, collectors))
            stop.wait(args.sample_interval_s)

    thread = threading.Thread(target=sample_loop, daemon=True)
    thread.start()
    try:
        return action()
    finally:
        stop.set()
        thread.join(timeout=1)
        samples.append(collect_sample(args, start, phase, target_ma, collectors))


def completeness(samples: list[dict[str, Any]]) -> dict[str, Any]:
    failures: list[str] = []
    if len(samples) < 2:
        return {"scene_complete": False, "failures": ["too_few_samples"], "effective_sample_rate_hz": None, "max_sample_gap_s": None}
    times = [float(s["t_s"]) for s in samples]
    gaps = [b - a for a, b in zip(times, times[1:])]
    span = times[-1] - times[0]
    rate = (len(times) - 1) / span if span > 0 else 0.0
    max_gap = max(gaps) if gaps else 0.0
    if rate < MIN_SAMPLE_RATE_HZ:
        failures.append("sample_rate_below_3hz")
    if max_gap > MAX_SAMPLE_GAP_S:
        failures.append("sample_gap_above_0_5s")
    required = {
        "source_v": any(isinstance(s.get("isolapurr_port_c_mv"), (int, float)) for s in samples),
        "ups_vin": any(isinstance(s.get("vin_vbus_mv"), (int, float)) for s in samples),
        "ups_vout": any(isinstance(s.get("out_a_vbus_mv"), (int, float)) or isinstance(s.get("out_b_vbus_mv"), (int, float)) for s in samples),
        "load_v": any(isinstance(s.get("load_v_local_mv"), (int, float)) for s in samples),
    }
    for key, ok in required.items():
        if not ok:
            failures.append(f"missing_{key}")
    return {
        "scene_complete": not failures,
        "failures": failures,
        "effective_sample_rate_hz": round(rate, 3),
        "max_sample_gap_s": round(max_gap, 3),
        "required_voltage_series": required,
    }


def gate_failures(actions: list[dict[str, Any]]) -> list[str]:
    failures: list[str] = []
    for action in actions:
        for name, value in action.items():
            if isinstance(value, dict):
                if value.get("ok") is False:
                    failures.append(name)
                verdict = value.get("verdict")
                if isinstance(verdict, dict) and verdict.get("ok") is False:
                    failures.append(name)
                nested_failures = value.get("failures")
                if isinstance(nested_failures, list) and nested_failures:
                    failures.extend(f"{name}:{failure}" for failure in nested_failures)
    return failures


def acceptance(summary: dict[str, Any]) -> dict[str, Any]:
    checks = list(summary.get("failures") or [])
    return {
        "signoff_valid": not checks,
        "run_validity": "valid_for_signoff" if not checks else "invalid_diagnostic_only",
        "failed_acceptance_checks": checks,
        "required_sample_rate_hz": MIN_SAMPLE_RATE_HZ,
    }


def start_collectors(args: argparse.Namespace) -> dict[str, Any]:
    collectors = {
        "ups_status": JsonlCollector(
            "ups_status",
            ups_cmd(
                args,
                "status",
                "--watch",
                "--interval-ms",
                str(int(args.sample_interval_s * 1000)),
                "--watch-freshness-ms",
                str(UPS_WATCH_FRESHNESS_MS),
                "--include-meta",
            ),
            ROOT.parent.parent,
        ),
        "ups_diag_snapshot": JsonlCollector(
            "ups_diag_snapshot",
            ups_cmd(
                args,
                "diag-snapshot",
                "--watch",
                "--interval-ms",
                str(int(args.sample_interval_s * 1000)),
                "--watch-freshness-ms",
                str(UPS_WATCH_FRESHNESS_MS),
                "--include-meta",
            ),
            ROOT.parent.parent,
        ),
        "load": JsonlCollector(
            "load",
            load_cmd(args, "--json", "status-stream", "--device", args.load_device, "--interval-ms", "200"),
            ROOT.parent.parent,
        ),
        "isolapurr": PollCollector(
            "isolapurr",
            args.sample_interval_s,
            lambda: read_isolapurr_ports(args, dry_run=False).get("result", {}),
        ),
    }
    for collector in collectors.values():
        collector.start()
    time.sleep(1.0)
    return collectors


def stop_collectors(collectors: dict[str, JsonlCollector]) -> None:
    for collector in collectors.values():
        collector.stop()


def run_scene(args: argparse.Namespace, suite_dir: Path, profile_key: str, scene_key: str) -> dict[str, Any]:
    profile = PROFILES[profile_key]
    scene = SCENES[scene_key]
    target_ma = scene["target_ma"]
    run_id = f"{profile_key}-{scene_key}-{target_ma}ma"
    run_dir = suite_dir / run_id
    run_dir.mkdir(parents=True, exist_ok=True)
    actions: list[dict[str, Any]] = []
    samples: list[dict[str, Any]] = []
    collectors: dict[str, JsonlCollector] = {}
    try:
        actions.append({"loadlynx_cli_gate": validate_loadlynx_cli(args, dry_run=args.dry_run)})
        if not args.dry_run and not actions[-1]["loadlynx_cli_gate"]["ok"]:
            raise RuntimeError(f"LoadLynx CLI does not support status-stream: {actions[-1]['loadlynx_cli_gate']}")
        actions.append({"load_disable_before_scene": ensure_load_disabled(args, dry_run=args.dry_run)})
        actions.append({"isolapurr_off_before_scene": set_isolapurr_output(args, False, dry_run=args.dry_run)})
        actions.append({"ups_monitor_restart_after_source_off": restart_ups_monitor(args, dry_run=args.dry_run)})
        actions.append({"ups_status_cache_after_monitor_restart": wait_ups_status_cache(args, timeout_s=20.0, dry_run=args.dry_run)})
        if not args.dry_run and not actions[-1]["ups_status_cache_after_monitor_restart"]["ok"]:
            raise RuntimeError(f"UPS status cache preheat failed: {actions[-1]['ups_status_cache_after_monitor_restart']}")
        pre_source_cut_gate = (
            wait_for_gate(
                "ups_pre_scene_source_cut",
                lambda: unwrap_ups_sample(read_ups_status(args, dry_run=False).get("result")),
                validate_ups_input_cut,
                timeout_s=20.0,
            )
            if not args.dry_run
            else {"ok": True}
        )
        actions.append({"ups_pre_scene_source_cut_gate": pre_source_cut_gate})
        if not args.dry_run and not pre_source_cut_gate["ok"]:
            raise RuntimeError(f"pre-scene UPS source cut gate failed: {pre_source_cut_gate}")
        identity, settings = read_ups_identity_settings(args, dry_run=args.dry_run)
        actions.extend([{"ups_identity": identity}, {"ups_settings": settings}])
        profile_gate = validate_ups_profile(profile_key, identity.get("result"), settings.get("result")) if not args.dry_run else {"ok": True}
        actions.append({"ups_profile_gate": profile_gate})
        if not args.dry_run and not profile_gate["ok"]:
            raise RuntimeError(f"UPS profile gate failed: {profile_gate}")
        actions.append({"isolapurr_manual_config": configure_isolapurr_manual(args, profile, dry_run=args.dry_run)})
        ports = read_isolapurr_ports(args, dry_run=args.dry_run)
        power = read_isolapurr_power(args, dry_run=args.dry_run)
        source_gate = validate_source_config(profile, power.get("result"), ports.get("result")) if not args.dry_run else {"ok": True}
        actions.extend([{"isolapurr_ports_before_enable": ports}, {"isolapurr_power_before_enable": power}, {"source_gate_before_enable": source_gate}])
        if not args.dry_run and not source_gate["ok"]:
            raise RuntimeError(f"source gate failed: {source_gate}")
        actions.append({"isolapurr_on": set_isolapurr_output(args, True, dry_run=args.dry_run)})
        source_online_gate = (
            wait_for_gate(
                "isolapurr_source_online",
                lambda: read_isolapurr_ports(args, dry_run=False).get("result"),
                lambda sample: validate_source_online(profile, sample),
                timeout_s=20.0,
            )
            if not args.dry_run
            else {"ok": True}
        )
        actions.append({"source_online_gate_before_scene": source_online_gate})
        if not args.dry_run and not source_online_gate["ok"]:
            raise RuntimeError(f"source online gate failed: {source_online_gate}")
        actions.append({"ups_monitor_start": start_ups_monitor(args, dry_run=args.dry_run)})
        ups_watch_preheat_gate = (
            wait_for_gate(
                "ups_watch_preheat",
                lambda: unwrap_ups_sample(read_ups_status(args, dry_run=False).get("result")),
                lambda sample: {"ok": bool(sample), "sample": sample},
                timeout_s=8.0,
            )
            if not args.dry_run
            else {"ok": True}
        )
        actions.append({"ups_watch_preheat_gate": ups_watch_preheat_gate})
        if not args.dry_run and not ups_watch_preheat_gate["ok"]:
            raise RuntimeError(f"UPS watch preheat gate failed: {ups_watch_preheat_gate}")
        ups_input_online_gate = (
            wait_for_gate(
                "ups_input_online",
                lambda: unwrap_ups_sample(read_ups_status(args, dry_run=False).get("result")),
                lambda sample: validate_ups_input_restored(profile_key, sample),
                timeout_s=20.0,
            )
            if not args.dry_run
            else {"ok": True}
        )
        actions.append({"ups_input_online_gate_before_scene": ups_input_online_gate})
        if not args.dry_run and not ups_input_online_gate["ok"]:
            raise RuntimeError(f"UPS input online gate failed: {ups_input_online_gate}")
        if args.dry_run:
            return {
                "output_profile": profile_key,
                "scene_type": scene_key,
                "target_ma": target_ma,
                "include_backup": scene["include_backup"],
                "load_min_v_mv": args.load_min_v_mv,
                "load_max_i_ma_total": args.load_max_i_ma_total,
                "load_max_p_mw": args.load_max_p_mw,
                "source_voltage_mv": profile["source_voltage_mv"],
                "source_current_limit_ma": profile["source_current_limit_ma"],
                "report_dir": str(run_dir.relative_to(suite_dir)),
                "scene_complete": False,
                "run_validity": "dry_run",
                "signoff_valid": False,
                "failures": ["dry_run"],
                "failed_acceptance_checks": ["dry_run"],
                "effective_sample_rate_hz": None,
                "max_sample_gap_s": None,
                "advanced_power": {},
            }
        collectors = start_collectors(args)
        start = time.time()
        sleep_collect(args, start, "pre", target_ma, collectors, samples, args.pre_s)
        actions.append({
            "load_cc": run_action_collecting(
                args,
                start,
                "transition_load",
                target_ma,
                collectors,
                samples,
                lambda: set_load_cc(args, target_ma, dry_run=False),
            )
        })
        sleep_collect(args, start, "hold", target_ma, collectors, samples, args.hold_s)
        actions.append({
            "isolapurr_cut_for_backup": run_action_collecting(
                args,
                start,
                "transition_backup",
                target_ma,
                collectors,
                samples,
                lambda: set_isolapurr_output(args, False, dry_run=False),
            )
        })
        sleep_collect(args, start, "backup", target_ma, collectors, samples, args.backup_s)
        source_cut_gate = wait_for_gate(
            "isolapurr_source_cut",
            lambda: {
                "power": read_isolapurr_power(args, dry_run=False).get("result"),
                "ports": read_isolapurr_ports(args, dry_run=False).get("result"),
            },
            lambda sample: {
                "ok": isolapurr_live_voltage_mv(sample.get("power"), sample.get("ports")) is None,
                "live_voltage_mv": isolapurr_live_voltage_mv(sample.get("power"), sample.get("ports")),
                "port_c": port_c_from_ports(sample.get("ports")),
            },
            timeout_s=3.0,
            tick=lambda: samples.append(collect_sample(args, start, "transition_backup", target_ma, collectors)),
        )
        actions.append({"source_cut_gate": source_cut_gate})
        cut_gate = validate_ups_input_cut(unwrap_ups_sample(collectors["ups_status"].latest_before(int(time.time() * 1000))))
        actions.append({"ups_input_cut_gate": cut_gate})
        actions.append({
            "isolapurr_restore": run_action_collecting(
                args,
                start,
                "transition_restore",
                target_ma,
                collectors,
                samples,
                lambda: set_isolapurr_output(args, True, dry_run=False),
            )
        })
        sleep_collect(args, start, "restore", target_ma, collectors, samples, args.restore_s)
        source_restore_gate = wait_for_gate(
            "isolapurr_source_restore",
            lambda: read_isolapurr_ports(args, dry_run=False).get("result"),
            lambda sample: validate_source_online(profile, sample),
            timeout_s=8.0,
            tick=lambda: samples.append(collect_sample(args, start, "transition_restore", target_ma, collectors)),
        )
        restore_gate = wait_for_gate(
            "ups_input_restore",
            lambda: unwrap_ups_sample(read_ups_status(args, dry_run=False).get("result")),
            lambda sample: validate_ups_input_restored(profile_key, sample),
            timeout_s=20.0,
            tick=lambda: samples.append(collect_sample(args, start, "transition_restore", target_ma, collectors)),
        )
        actions.extend([{"source_restore_gate": source_restore_gate}, {"ups_input_restore_gate": restore_gate}])
        actions.append({
            "load_disable_after_scene": run_action_collecting(
                args,
                start,
                "transition_unload",
                target_ma,
                collectors,
                samples,
                lambda: ensure_load_disabled(args, dry_run=False),
            )
        })
        sleep_collect(args, start, "post", target_ma, collectors, samples, args.post_s)
    finally:
        if collectors:
            stop_collectors(collectors)
        if not args.dry_run:
            try:
                ensure_load_disabled(args, dry_run=False)
            except Exception:
                pass
            try:
                set_isolapurr_output(args, False, dry_run=False)
            except Exception:
                pass
    for name, collector in collectors.items():
        write_json(run_dir / f"{name}_raw.json", {"cmd": collector.cmd, "rows": collector.rows, "errors": collector.errors})
    comp = completeness(samples)
    collector_failure_list = [f"{name}_collector_error" for name, collector in collectors.items() if collector.errors]
    gate_failure_list = gate_failures(actions)
    if gate_failure_list or collector_failure_list:
        comp["failures"] = [*comp["failures"], *gate_failure_list, *collector_failure_list]
        comp["scene_complete"] = False
    acc = acceptance(comp)
    metadata = {
        "output_profile": profile_key,
        "scene_type": scene_key,
        "target_ma": target_ma,
        "include_backup": scene["include_backup"],
        "load_min_v_mv": args.load_min_v_mv,
        "max_i_ma_total": args.load_max_i_ma_total,
        "max_p_mw": args.load_max_p_mw,
        "source_voltage_mv": profile["source_voltage_mv"],
        "source_current_limit_ma": profile["source_current_limit_ma"],
        "ups_transport": "cli+ipc+usb",
        "load_transport": "cli+ipc+usb",
        "isolapurr_transport": "cli+default-ipc",
    }
    settings_payload = settings.get("result") if isinstance(settings, dict) else {}
    results = {
        "metadata": metadata,
        "actions": actions,
        "settings_snapshot": settings_payload,
        "samples": samples,
        "summary": {"all": {"completeness": comp, "acceptance": acc}},
    }
    write_json(run_dir / "results.json", results)
    with (run_dir / "timeseries.jsonl").open("w", encoding="utf-8") as fh:
        for sample in samples:
            fh.write(json.dumps(sample, ensure_ascii=False) + "\n")
    run([sys.executable, args.render_chart, "--input", str(run_dir / "timeseries.jsonl"), "--output", str(run_dir / "voltage-chart.html"), "--title", f"{profile_key.upper()} {scene_key}"], timeout=20)
    return {
        "output_profile": profile_key,
        "scene_type": scene_key,
        "target_ma": target_ma,
        "include_backup": scene["include_backup"],
        "load_min_v_mv": args.load_min_v_mv,
        "load_max_i_ma_total": args.load_max_i_ma_total,
        "load_max_p_mw": args.load_max_p_mw,
        "source_voltage_mv": profile["source_voltage_mv"],
        "source_current_limit_ma": profile["source_current_limit_ma"],
        "report_dir": str(run_dir.relative_to(suite_dir)),
        "scene_complete": comp["scene_complete"],
        "run_validity": acc["run_validity"],
        "signoff_valid": acc["signoff_valid"],
        "failures": comp["failures"],
        "failed_acceptance_checks": acc["failed_acceptance_checks"],
        "effective_sample_rate_hz": comp["effective_sample_rate_hz"],
        "max_sample_gap_s": comp["max_sample_gap_s"],
        "advanced_power": dict((settings_payload or {}).get("advanced_power") or {}),
    }


def main() -> int:
    args = parse_args()
    suite_id = args.suite_id or datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ-cli-formal-suite")
    suite_dir = Path(args.report_root) / suite_id
    suite_dir.mkdir(parents=True, exist_ok=True)
    reports: list[dict[str, Any]] = []
    profile_switches: list[dict[str, Any]] = []
    for profile_key in args.profiles:
        profile_switches.append(switch_profile_if_needed(args, profile_key, suite_dir))
        for scene_key in args.scenes:
            reports.append(run_scene(args, suite_dir, profile_key, scene_key))
    summary = {
        "suite_id": suite_id,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "transport": {
            "ups": "CLI + native IPC + USB",
            "loadlynx": "CLI + native IPC + USB",
            "isolapurr": "CLI + default IPC",
        },
        "load_protection": {"min_v_mv": args.load_min_v_mv, "max_i_ma_total": args.load_max_i_ma_total, "max_p_mw": args.load_max_p_mw},
        "profiles": PROFILES,
        "profile_switches": profile_switches,
        "reports": reports,
    }
    summary_path = suite_dir / "suite-summary.json"
    write_json(summary_path, summary)
    run([sys.executable, args.render_overview, "--summary", str(summary_path), "--output", str(suite_dir / "suite-overview.html")], timeout=20)
    print(json.dumps({"suite_dir": str(suite_dir), "summary": str(summary_path), "overview": str(suite_dir / "suite-overview.html")}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
