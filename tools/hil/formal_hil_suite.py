#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
import urllib.parse
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path
from typing import Any
import importlib.util


ROOT = Path(__file__).resolve().parent
RUNNER_PATH = ROOT / "advanced_power_12v_runner.py"
RUNNER_SPEC = importlib.util.spec_from_file_location("advanced_power_12v_runner", RUNNER_PATH)
if RUNNER_SPEC is None or RUNNER_SPEC.loader is None:
    raise RuntimeError(f"failed to load advanced_power_12v_runner module from {RUNNER_PATH}")
runner = importlib.util.module_from_spec(RUNNER_SPEC)
RUNNER_SPEC.loader.exec_module(runner)
DEFAULT_RUNNER = ROOT / "advanced_power_12v_runner.py"
DEFAULT_VERIFY = ROOT / "verify_formal_suite.py"
DEFAULT_OVERVIEW = ROOT / "render_formal_suite_html.py"
DEFAULT_CHART = ROOT / "render_voltage_chart_html.py"
DEFAULT_REPORT_ROOT = ROOT / "reports"
DEFAULT_FIRMWARE_BUNDLE_ROOT = ROOT.parent.parent / "web/public/firmware"

DEFAULT_UPS_DEVICE_ID = "serial-04f3bb3f5367"
DEFAULT_LOAD_DEVICE = "loadlynx-d68638"
DEFAULT_LOAD_USB_PORT = "/dev/cu.usbmodem212101"
DEFAULT_LOAD_CLI = str(Path.home() / ".local" / "bin" / "loadlynx")
DEFAULT_LOAD_IPC = ""
DEFAULT_LOAD_DEVD_SOCKET = "/tmp/loadlynx-koha-formal.sock"
DEFAULT_LOAD_DEVD_BASE_URL = ""
DEFAULT_LOAD_BRIDGE_URL = "http://127.0.0.1:30180"
DEFAULT_LOAD_BRIDGE_DEVICE = ""
DEFAULT_MAINS_AEGIS_CLI = str(
    ROOT.parent / "mains-aegis-host" / "target" / "debug" / "mains-aegis"
)
DEFAULT_ISOLAPURR_CLI = "isolapurr"
DEFAULT_ISOLAPURR_URL = "http://192.168.31.122"
DEFAULT_ISOLAPURR_DEVICE_ID = "856a141cdbd4"
DEFAULT_UPS_OBSERVE_DEVICE_ID = (
    os.environ.get("MAINS_AEGIS_OBSERVE_DEVICE_ID") or DEFAULT_UPS_DEVICE_ID
)


def default_mains_aegis_devd_base_url() -> str:
    return (
        os.environ.get("MAINS_AEGIS_DEVD_URL")
        or os.environ.get("VITE_DEFAULT_DEVD_URL")
        or os.environ.get("VITE_DEVD_API_BASE")
        or "http://127.0.0.1:30080"
    )


def default_ups_status_url(device_id: str = DEFAULT_UPS_OBSERVE_DEVICE_ID) -> str:
    return f"{default_mains_aegis_devd_base_url().rstrip('/')}/api/v1/devices/{device_id}/status"


def default_ups_settings_url(device_id: str = DEFAULT_UPS_OBSERVE_DEVICE_ID) -> str:
    return f"{default_mains_aegis_devd_base_url().rstrip('/')}/api/v1/devices/{device_id}/settings"


def default_ups_power_diag_url(device_id: str = DEFAULT_UPS_OBSERVE_DEVICE_ID) -> str:
    return f"{default_mains_aegis_devd_base_url().rstrip('/')}/api/v1/devices/{device_id}/power-diag"


def default_ups_trace_url(device_id: str = DEFAULT_UPS_OBSERVE_DEVICE_ID) -> str:
    return f"{default_mains_aegis_devd_base_url().rstrip('/')}/api/v1/devices/{device_id}/trace?trace_limit=1"


def default_ups_scan_url() -> str:
    return f"{default_mains_aegis_devd_base_url().rstrip('/')}/api/v1/devices/scan"


DEFAULT_UPS_STATUS_URL = default_ups_status_url()
DEFAULT_UPS_SETTINGS_URL = default_ups_settings_url()
DEFAULT_UPS_POWER_DIAG_URL = default_ups_power_diag_url()
DEFAULT_UPS_TRACE_URL = default_ups_trace_url()
DEFAULT_UPS_SCAN_URL = default_ups_scan_url()
UPS_INPUT_CUT_MAX_VIN_MV = 2999
TRANSIENT_HTTP_STATUS_CODES = {502, 503, 504}

PROFILES: dict[str, dict[str, Any]] = {
    "12v": {
        "source_voltage_mv": 12000,
        "source_current_limit_ma": 3000,
        "artifact_features": ["net_http", "web_serial"],
    },
    "19v": {
        "source_voltage_mv": 19000,
        "source_current_limit_ma": 3000,
        "artifact_features": ["net_http", "web_serial", "main-vout-19v"],
    },
}

SCENES: dict[str, dict[str, Any]] = {
    "assist_path": {
        "target_ma": 3900,
        "include_backup": True,
    },
    "backup_only": {
        "target_ma": 1000,
        "include_backup": True,
    },
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run the formal 12V/19V HIL suite and build one four-chart overview report."
    )
    parser.add_argument("--suite-id", default=None)
    parser.add_argument("--runner", default=str(DEFAULT_RUNNER))
    parser.add_argument("--verify-script", default=str(DEFAULT_VERIFY))
    parser.add_argument("--overview-script", default=str(DEFAULT_OVERVIEW))
    parser.add_argument("--chart-script", default=str(DEFAULT_CHART))
    parser.add_argument("--report-root", default=str(DEFAULT_REPORT_ROOT))
    parser.add_argument(
        "--output-profiles",
        nargs="+",
        choices=("12v", "19v"),
        default=["12v", "19v"],
    )
    parser.add_argument(
        "--scenes",
        nargs="+",
        choices=("assist_path", "backup_only"),
        default=["assist_path", "backup_only"],
    )
    parser.add_argument("--load-device", default=DEFAULT_LOAD_DEVICE)
    parser.add_argument("--load-usb-port", default=DEFAULT_LOAD_USB_PORT)
    parser.add_argument("--load-cli", default=DEFAULT_LOAD_CLI)
    parser.add_argument("--load-ipc", default=DEFAULT_LOAD_IPC)
    parser.add_argument("--load-devd-socket", default=DEFAULT_LOAD_DEVD_SOCKET)
    parser.add_argument("--load-devd-base-url", default=DEFAULT_LOAD_DEVD_BASE_URL)
    parser.add_argument("--load-bridge-url", default=DEFAULT_LOAD_BRIDGE_URL)
    parser.add_argument("--load-bridge-device", default=DEFAULT_LOAD_BRIDGE_DEVICE)
    parser.add_argument("--load-min-v-mv", type=int, default=3000)
    parser.add_argument("--max-i-ma-total", type=int, default=4000)
    parser.add_argument("--max-p-mw", type=int, default=80000)
    parser.add_argument("--isolapurr-cli", default=DEFAULT_ISOLAPURR_CLI)
    parser.add_argument("--isolapurr-url", default=DEFAULT_ISOLAPURR_URL)
    parser.add_argument("--isolapurr-device-id", default=DEFAULT_ISOLAPURR_DEVICE_ID)
    parser.add_argument("--mains-aegis-cli", default=DEFAULT_MAINS_AEGIS_CLI)
    parser.add_argument("--mains-aegis-ipc", default=None)
    parser.add_argument("--ups-device-id", default=DEFAULT_UPS_DEVICE_ID)
    parser.add_argument("--ups-status-url", default=DEFAULT_UPS_STATUS_URL)
    parser.add_argument("--ups-settings-url", default=DEFAULT_UPS_SETTINGS_URL)
    parser.add_argument("--devd-power-diag-url", default=DEFAULT_UPS_POWER_DIAG_URL)
    parser.add_argument("--devd-device-trace-url", default=DEFAULT_UPS_TRACE_URL)
    parser.add_argument("--devd-scan-url", default=DEFAULT_UPS_SCAN_URL)
    parser.add_argument("--artifact-manifest-12v", default=None)
    parser.add_argument("--artifact-manifest-19v", default=None)
    parser.add_argument(
        "--firmware-bundle-root",
        default=str(DEFAULT_FIRMWARE_BUNDLE_ROOT),
        help="Firmware bundle root used to auto-resolve profile manifests when explicit paths are omitted.",
    )
    parser.add_argument("--sample-interval-seconds", type=float, default=0.25)
    parser.add_argument("--load-stream-interval-seconds", type=float, default=0.2)
    parser.add_argument("--command-timeout-sec", type=float, default=45.0)
    parser.add_argument("--status-timeout-sec", type=float, default=20.0)
    parser.add_argument("--verify-timeout-sec", type=float, default=45.0)
    parser.add_argument("--load-status-ready-timeout-sec", type=float, default=20.0)
    parser.add_argument("--pre-seconds", type=float, default=12.0)
    parser.add_argument("--hold-seconds", type=float, default=18.0)
    parser.add_argument("--backup-hold-seconds", type=float, default=18.0)
    parser.add_argument("--restore-hold-seconds", type=float, default=18.0)
    parser.add_argument("--post-seconds", type=float, default=12.0)
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--skip-flash", action="store_true")
    parser.add_argument("--skip-verify", action="store_true")
    parser.add_argument("--skip-overview", action="store_true")
    return parser.parse_args()


def run(cmd: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, check=True, text=True, capture_output=True)


def run_json(cmd: list[str]) -> Any:
    completed = run(cmd)
    text = completed.stdout.strip()
    return json.loads(text) if text else {}


def http_request_json(
    url: str,
    *,
    method: str,
    timeout_sec: float = 20.0,
    retries: int = 3,
    retry_delay_sec: float = 0.2,
) -> Any:
    attempts = max(1, retries + 1)
    last_exc: Exception | None = None
    for attempt_idx in range(attempts):
        try:
            request = urllib.request.Request(url, method=method)
            with urllib.request.urlopen(request, timeout=timeout_sec) as response:
                payload = response.read().decode("utf-8").strip()
            return json.loads(payload) if payload else {}
        except urllib.error.HTTPError as exc:
            last_exc = exc
            if exc.code not in TRANSIENT_HTTP_STATUS_CODES or attempt_idx + 1 >= attempts:
                raise
        except (
            urllib.error.URLError,
            TimeoutError,
            ConnectionError,
            OSError,
            json.JSONDecodeError,
        ) as exc:
            last_exc = exc
            if attempt_idx + 1 >= attempts:
                raise
        time.sleep(retry_delay_sec)
    if last_exc is not None:
        raise last_exc
    return {}


def http_post_json(url: str) -> Any:
    return http_request_json(url, method="POST")


def http_get_json(url: str) -> Any:
    return http_request_json(url, method="GET")


def probe_isolapurr_source_reachability(
    *,
    isolapurr_cli: str,
    isolapurr_url: str,
    timeout_sec: float,
    dry_run: bool,
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
    if dry_run:
        return {
            "ok": True,
            "dry_run": True,
            "isolapurr_url": isolapurr_url,
            "expected_device_id": expected_device_id,
            "http_ports_url": http_url,
            "cli_cmd": cli_cmd,
        }
    failures: list[str] = []
    http_ports: dict[str, Any] | None = None
    cli_status: dict[str, Any] | None = None
    http_error: str | None = None
    cli_error: str | None = None
    observed_cli_device_id: str | None = None
    observed_http_port_ids: list[str] = []
    try:
        http_ports = dict(http_get_json(http_url) or {})
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
        cli_status = dict(run_json(cli_cmd) or {})
        cli_device = dict(cli_status.get("device") or {})
        if isinstance(cli_device.get("device_id"), str):
            observed_cli_device_id = cli_device.get("device_id", "").strip() or None
        if observed_cli_device_id is None:
            cli_identity = dict(cli_status.get("identity") or {})
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


def mains_aegis_base_cmd(args: argparse.Namespace) -> list[str]:
    cmd = [args.mains_aegis_cli]
    if args.mains_aegis_ipc:
        cmd.extend(["--ipc", args.mains_aegis_ipc])
    return cmd


def observe_device_id_from_args(args: argparse.Namespace) -> str | None:
    explicit = getattr(args, "ups_observe_device_id", None)
    if isinstance(explicit, str) and explicit.strip():
        return explicit.strip()
    for candidate in (
        getattr(args, "devd_power_diag_url", None),
        getattr(args, "ups_status_url", None),
        getattr(args, "ups_settings_url", None),
    ):
        if not isinstance(candidate, str) or not candidate.strip():
            continue
        device_id = devd_device_id_from_endpoint(candidate)
        if isinstance(device_id, str) and device_id:
            return device_id
    control_id = getattr(args, "ups_device_id", None)
    if isinstance(control_id, str) and control_id.strip():
        return control_id.strip()
    return None


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


def configure_source_manual_output(
    *,
    isolapurr_cli: str,
    isolapurr_url: str,
    voltage_mv: int,
    current_limit_ma: int,
    dry_run: bool,
) -> dict[str, Any]:
    manual_cmd = [
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
    ]
    if dry_run:
        return {
            "voltage_mv": voltage_mv,
            "current_limit_ma": current_limit_ma,
            "actions": [{"dry_run": True, "cmd": manual_cmd}],
        }
    return {
        "voltage_mv": voltage_mv,
        "current_limit_ma": current_limit_ma,
        "actions": [{"cmd": manual_cmd, "result": run_json(manual_cmd)}],
    }


def set_port_c_power_state(
    *,
    isolapurr_url: str,
    enabled: bool,
    dry_run: bool,
) -> dict[str, Any]:
    power_url = (
        f"{isolapurr_url.rstrip('/')}/api/v1/ports/port_c/power?"
        f"{urllib.parse.urlencode({'enabled': '1' if enabled else '0'})}"
    )
    if dry_run:
        return {
            "enabled": enabled,
            "actions": [{"dry_run": True, "url": power_url, "method": "POST"}],
        }
    try:
        power_result = http_post_json(power_url)
    except urllib.error.HTTPError as exc:
        response_text = exc.read().decode("utf-8", errors="replace")
        ports_snapshot = http_get_json(f"{isolapurr_url.rstrip('/')}/api/v1/ports")
        port_c = next(
            (
                port
                for port in ports_snapshot.get("ports", [])
                if port.get("portId") == "port_c"
            ),
            None,
        )
        port_c_state = port_c.get("state") if isinstance(port_c, dict) else None
        if (
            exc.code == 409
            and isinstance(port_c_state, dict)
            and bool(port_c_state.get("power_enabled")) is enabled
        ):
            power_result = {
                "accepted": True,
                "power_enabled": enabled,
                "source": "conflict-treated-as-target-state",
                "http_error": {
                    "status": exc.code,
                    "body": response_text,
                },
                "port": port_c,
            }
        else:
            raise
    deadline = time.monotonic() + 5.0
    last_snapshot: dict[str, Any] | None = None
    while time.monotonic() < deadline:
        ports_snapshot = http_get_json(f"{isolapurr_url.rstrip('/')}/api/v1/ports")
        port_c = next(
            (
                port
                for port in ports_snapshot.get("ports", [])
                if port.get("portId") == "port_c"
            ),
            None,
        )
        state = port_c.get("state") if isinstance(port_c, dict) else None
        if isinstance(state, dict):
            last_snapshot = {"port": port_c, "state": state}
            if bool(state.get("power_enabled")) is enabled and not bool(state.get("busy")):
                settle_result = {
                    "ok": True,
                    "target_power_enabled": enabled,
                    "port": port_c,
                }
                return {
                    "enabled": enabled,
                    "actions": [
                        {
                            "url": power_url,
                            "method": "POST",
                            "result": power_result,
                            "settle": settle_result,
                        }
                    ],
                }
        time.sleep(0.1)
    raise RuntimeError(
        "isolapurr_port_c_failed_to_settle: "
        f"{{'target_power_enabled': {enabled}, 'last_snapshot': {last_snapshot}}}"
    )


def set_source_state(
    *,
    isolapurr_cli: str,
    isolapurr_url: str,
    voltage_mv: int,
    current_limit_ma: int,
    enabled: bool,
    dry_run: bool,
) -> dict[str, Any]:
    manual_payload = configure_source_manual_output(
        isolapurr_cli=isolapurr_cli,
        isolapurr_url=isolapurr_url,
        voltage_mv=voltage_mv,
        current_limit_ma=current_limit_ma,
        dry_run=dry_run,
    )
    power_payload = set_port_c_power_state(
        isolapurr_url=isolapurr_url,
        enabled=enabled,
        dry_run=dry_run,
    )
    return {
        "voltage_mv": voltage_mv,
        "current_limit_ma": current_limit_ma,
        "enabled": enabled,
        "actions": list(manual_payload.get("actions") or []) + list(power_payload.get("actions") or []),
    }


def cut_source_power_only(
    *,
    isolapurr_url: str,
    dry_run: bool,
) -> dict[str, Any]:
    return set_port_c_power_state(
        isolapurr_url=isolapurr_url,
        enabled=False,
        dry_run=dry_run,
    )


def port_state(ports_payload: dict[str, Any], *, port_id: str) -> dict[str, Any]:
    ports_root = ports_payload.get("ports")
    if isinstance(ports_root, list):
        ports = ports_root
    else:
        ports = dict(ports_root or {}).get("ports", []) if isinstance(ports_root, dict) else []
    for port in ports:
        if isinstance(port, dict) and port.get("portId") == port_id:
            return dict(port)
    return {}


def validate_source_configuration(
    *,
    expected_voltage_mv: int,
    expected_current_limit_ma: int,
    set_source_payload: Any,
    ports_payload: Any,
) -> dict[str, Any]:
    payload = set_source_payload if isinstance(set_source_payload, dict) else {}
    actions = payload.get("actions")
    if not isinstance(actions, list):
        actions = []
    manual_ack = {}
    for action in actions:
        if not isinstance(action, dict):
            continue
        result = action.get("result")
        if isinstance(result, dict) and "manual" in result:
            manual_ack = dict(result)
    manual = dict(manual_ack.get("manual") or {})
    failures: list[str] = []
    if manual.get("voltage_mv") != expected_voltage_mv:
        failures.append("manual_ack_voltage_mismatch")
    if manual.get("current_limit_ma") != expected_current_limit_ma:
        failures.append("manual_ack_current_limit_mismatch")
    if manual.get("path_policy") != "force_close":
        failures.append("manual_ack_path_policy_mismatch")
    if manual.get("usb_c_path_mode") != "disconnect":
        failures.append("manual_ack_usb_c_path_mode_mismatch")
    if manual_ack.get("tps_mode") != "manual":
        failures.append("manual_ack_tps_mode_mismatch")
    port_c = port_state(ports_payload if isinstance(ports_payload, dict) else {}, port_id="port_c")
    port_c_state = dict(port_c.get("state") or {})
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
            "power_enabled": False,
        },
        "manual_ack": manual_ack,
        "port_c": port_c,
    }


def disable_load(*, load_cli: str, load_device: str, dry_run: bool) -> dict[str, Any]:
    cmd = [load_cli, "control", "set", "--device", load_device, "--disable"]
    if dry_run:
        return {"dry_run": True, "cmd": cmd}
    return {"cmd": cmd, "result": run(cmd).stdout.strip()}


def load_catalog(path: Path) -> dict[str, Any]:
    payload = json.loads(path.read_text())
    if payload.get("schema_version") != 1 or not isinstance(payload.get("artifacts"), list):
        raise SystemExit(f"unsupported firmware catalog schema: {path}")
    return payload


def resolve_manifest_from_bundle(
    *,
    bundle_root: Path,
    profile_key: str,
) -> str | None:
    catalog_path = bundle_root / "firmware-catalog.json"
    if not catalog_path.is_file():
        return None
    required_features = list(PROFILES[profile_key]["artifact_features"])
    catalog = load_catalog(catalog_path)
    for artifact in catalog["artifacts"]:
        features = artifact.get("features") or []
        if features == required_features:
            artifact_id = artifact.get("artifact_id")
            if isinstance(artifact_id, str) and artifact_id:
                manifest_path = bundle_root / f"{artifact_id}.manifest.json"
                if manifest_path.is_file():
                    return str(manifest_path)
    return None


def artifact_manifest_for_profile(args: argparse.Namespace, profile_key: str) -> str | None:
    if profile_key == "12v":
        return args.artifact_manifest_12v or resolve_manifest_from_bundle(
            bundle_root=Path(args.firmware_bundle_root).resolve(),
            profile_key=profile_key,
        )
    if profile_key == "19v":
        return args.artifact_manifest_19v or resolve_manifest_from_bundle(
            bundle_root=Path(args.firmware_bundle_root).resolve(),
            profile_key=profile_key,
        )
    return None


def select_and_flash_artifact(
    args: argparse.Namespace,
    *,
    profile_key: str,
    manifest_path: str,
    dry_run: bool,
) -> dict[str, Any]:
    base = mains_aegis_base_cmd(args)
    select_cmd = base + [
        "device",
        args.ups_device_id,
        "artifact",
        "select",
        "--manifest-path",
        manifest_path,
    ]
    flash_cmd = base + [
        "device",
        args.ups_device_id,
        "flash",
        "--real" if not dry_run else "--dry-run",
    ]
    result: dict[str, Any] = {
        "profile": profile_key,
        "manifest_path": manifest_path,
        "select_cmd": select_cmd,
        "flash_cmd": flash_cmd,
    }
    if dry_run:
        result["dry_run"] = True
        return result
    result["select_result"] = run_json(select_cmd)
    result["flash_result"] = run_json(flash_cmd)
    return result


def read_selected_artifact(args: argparse.Namespace, *, dry_run: bool) -> dict[str, Any]:
    cmd = mains_aegis_base_cmd(args) + [
        "device",
        args.ups_device_id,
        "artifact",
        "get",
    ]
    if dry_run:
        return {"dry_run": True, "cmd": cmd}
    return {"cmd": cmd, "result": run_json(cmd)}


def connect_device(args: argparse.Namespace, *, dry_run: bool) -> dict[str, Any]:
    cmd = mains_aegis_base_cmd(args) + [
        "device",
        args.ups_device_id,
        "connect",
    ]
    if dry_run:
        return {"dry_run": True, "cmd": cmd}
    return {"cmd": cmd, "result": run_json(cmd)}


def refresh_control_devices(args: argparse.Namespace, *, dry_run: bool) -> dict[str, Any]:
    cmd = mains_aegis_base_cmd(args) + [
        "devices",
        "scan",
    ]
    if dry_run:
        return {"dry_run": True, "cmd": cmd}
    return {"cmd": cmd, "result": run_json(cmd)}


def connect_device_with_retry(
    args: argparse.Namespace,
    *,
    dry_run: bool,
    timeout_sec: float,
    retry_interval_sec: float = 0.75,
) -> dict[str, Any]:
    if dry_run:
        return connect_device(args, dry_run=True)
    deadline = time.monotonic() + max(timeout_sec, 0.0)
    attempts: list[dict[str, Any]] = []
    last_error: str | None = None
    while True:
        try:
            result = connect_device(args, dry_run=False)
            if attempts:
                result = {
                    **result,
                    "retry_attempts": attempts,
                }
            return result
        except subprocess.CalledProcessError as error:
            stdout = (error.stdout or "").strip()
            stderr = (error.stderr or "").strip()
            last_error = stderr or stdout or str(error)
            attempts.append(
                {
                    "returncode": error.returncode,
                    "stdout": stdout,
                    "stderr": stderr,
                }
            )
            if time.monotonic() >= deadline:
                raise RuntimeError(
                    "connect_device_retry_exhausted: "
                    f"{{'attempts': {attempts}, 'last_error': {last_error!r}}}"
                ) from error
            time.sleep(retry_interval_sec)


def devd_device_entry_from_listing(listing_payload: Any, *, device_id: str | None) -> dict[str, Any]:
    if not isinstance(device_id, str) or not device_id:
        return {}
    devices = listing_payload.get("devices") if isinstance(listing_payload, dict) else None
    if not isinstance(devices, list):
        return {}
    for device in devices:
        payload = device if isinstance(device, dict) else {}
        identity = payload.get("identity") if isinstance(payload.get("identity"), dict) else {}
        if payload.get("id") == device_id or identity.get("device_id") == device_id:
            return payload
    return {}


def devd_device_entry_from_scan(scan_payload: Any, *, device_id: str | None) -> dict[str, Any]:
    return devd_device_entry_from_listing(scan_payload, device_id=device_id)


def seeded_devd_device_is_capability_ready(device_payload: Any) -> bool:
    payload = device_payload if isinstance(device_payload, dict) else {}
    if payload.get("connection") != "connected":
        return False
    return (
        isinstance(payload.get("identity"), dict)
        and isinstance(payload.get("settings"), dict)
    )


def devd_device_id_from_endpoint(url: str) -> str | None:
    parsed = urllib.parse.urlparse(url)
    path_parts = [part for part in parsed.path.split("/") if part]
    try:
        devices_idx = path_parts.index("devices")
    except ValueError:
        return None
    target_idx = devices_idx + 1
    if target_idx >= len(path_parts):
        return None
    device_id = urllib.parse.unquote(path_parts[target_idx]).strip()
    return device_id or None


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


def devd_base_url_from_scan_url(scan_url: str) -> str | None:
    if not isinstance(scan_url, str) or not scan_url.strip():
        return None
    parsed = urllib.parse.urlparse(scan_url)
    if not parsed.scheme or not parsed.netloc:
        return None
    return urllib.parse.urlunparse((parsed.scheme, parsed.netloc, "", "", "", ""))


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
        "devd_device_trace_url": normalize_devd_device_endpoint(
            rewrite_devd_endpoint_base(args.devd_device_trace_url, base_url=devd_base_url),
            device_id=normalized_device_id,
        ),
    }


def read_device_identity(args: argparse.Namespace, *, dry_run: bool) -> dict[str, Any]:
    cmd = mains_aegis_base_cmd(args) + [
        "device",
        args.ups_device_id,
        "identity",
    ]
    if dry_run:
        return {"dry_run": True, "cmd": cmd}
    return {"cmd": cmd, "result": run_json(cmd)}


def read_device_settings(args: argparse.Namespace, *, dry_run: bool) -> dict[str, Any]:
    cmd = mains_aegis_base_cmd(args) + [
        "device",
        args.ups_device_id,
        "settings",
    ]
    if dry_run:
        return {"dry_run": True, "cmd": cmd}
    return {"cmd": cmd, "result": run_json(cmd)}


def direct_lan_url_from_status_url(status_url: str, path_suffix: str) -> str:
    parsed = urllib.parse.urlparse(status_url)
    return urllib.parse.urlunparse(
        (parsed.scheme, parsed.netloc, path_suffix, "", "", "")
    )


def direct_lan_base_url_from_identity(identity_payload: Any) -> str | None:
    identity = identity_payload if isinstance(identity_payload, dict) else {}
    network = identity.get("network")
    if not isinstance(network, dict):
        return None
    ipv4 = network.get("ipv4")
    if isinstance(ipv4, str) and ipv4:
        return f"http://{ipv4}"
    hostname_fqdn = identity.get("hostname_fqdn")
    if isinstance(hostname_fqdn, str) and hostname_fqdn:
        return f"http://{hostname_fqdn}"
    hostname = identity.get("hostname")
    if isinstance(hostname, str) and hostname:
        return f"http://{hostname}"
    return None


def expected_profile_rated_vout_mv(profile_key: str) -> int:
    return 19_000 if profile_key == "19v" else 12_000


def validate_ups_external_input_cut(status_payload: Any) -> dict[str, Any]:
    status = status_payload if isinstance(status_payload, dict) else {}
    input_root = status.get("input") if isinstance(status.get("input"), dict) else {}
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


def validate_ups_external_input_restored(
    *,
    status_payload: Any,
    profile_key: str,
) -> dict[str, Any]:
    status = status_payload if isinstance(status_payload, dict) else {}
    input_root = status.get("input") if isinstance(status.get("input"), dict) else {}
    vin_vbus_mv = input_root.get("vin_vbus_mv")
    mains_present = input_root.get("mains_present")
    assist_power_stage = input_root.get("assist_power_stage")
    mode = status.get("mode")
    window = output_profile_guard(profile_key)
    failures: list[str] = []
    if mains_present is not True:
        failures.append("ups_mains_present_not_true")
    if not isinstance(vin_vbus_mv, int):
        failures.append("ups_vin_vbus_missing")
    elif not (window["min_mv"] <= vin_vbus_mv <= window["max_mv"]):
        failures.append("ups_vin_vbus_out_of_profile_window")
    if mode == "backup" or assist_power_stage == "backup":
        failures.append("ups_backup_semantics_still_active")
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
            "mains_present": True,
            "vin_vbus_mv_min": window["min_mv"],
            "vin_vbus_mv_max": window["max_mv"],
            "backup_required": False,
        },
    }


def wait_for_ups_external_input_cut(
    *,
    status_url: str,
    timeout_sec: float,
    dry_run: bool,
) -> dict[str, Any]:
    if dry_run:
        return {
            "dry_run": True,
            "url": status_url,
            "expected": {
                "vin_vbus_mv_max": UPS_INPUT_CUT_MAX_VIN_MV,
                "mains_present": False,
                "backup_required": True,
            },
        }
    deadline = time.monotonic() + max(0.1, timeout_sec)
    last_status: Any = None
    last_gate: dict[str, Any] | None = None
    last_error: str | None = None
    while time.monotonic() < deadline:
        try:
            last_status = http_get_json(status_url)
            last_error = None
        except Exception as exc:  # noqa: BLE001
            last_error = repr(exc)
            time.sleep(0.2)
            continue
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
            "failures": ["ups_status_unavailable"]
            + (["ups_status_transport_error"] if last_error else []),
            "status": last_status if isinstance(last_status, dict) else {},
            "error": last_error,
        },
    }


def wait_for_ups_external_input_restored(
    *,
    status_url: str,
    profile_key: str,
    timeout_sec: float,
    dry_run: bool,
) -> dict[str, Any]:
    window = output_profile_guard(profile_key)
    if dry_run:
        return {
            "dry_run": True,
            "url": status_url,
            "expected": {
                "mains_present": True,
                "vin_vbus_mv_min": window["min_mv"],
                "vin_vbus_mv_max": window["max_mv"],
                "backup_required": False,
            },
        }
    deadline = time.monotonic() + max(0.1, timeout_sec)
    last_status: Any = None
    last_gate: dict[str, Any] | None = None
    last_error: str | None = None
    while time.monotonic() < deadline:
        try:
            last_status = http_get_json(status_url)
            last_error = None
        except Exception as exc:  # noqa: BLE001
            last_error = repr(exc)
            time.sleep(0.2)
            continue
        last_gate = validate_ups_external_input_restored(
            status_payload=last_status,
            profile_key=profile_key,
        )
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
            "failures": ["ups_status_unavailable"]
            + (["ups_status_transport_error"] if last_error else []),
            "status": last_status if isinstance(last_status, dict) else {},
            "error": last_error,
        },
    }


def extract_identity_hardware_capabilities(identity_payload: Any) -> dict[str, Any]:
    identity = identity_payload if isinstance(identity_payload, dict) else {}
    hardware = identity.get("hardware_capabilities")
    return hardware if isinstance(hardware, dict) else {}


def extract_settings_hardware_capabilities(settings_payload: Any) -> dict[str, Any]:
    settings = settings_payload if isinstance(settings_payload, dict) else {}
    advanced = settings.get("advanced_power_capabilities")
    if not isinstance(advanced, dict):
        return {}
    rated_vout_mv = advanced.get("rated_vout_mv")
    if not isinstance(rated_vout_mv, int):
        return {}
    return {
        "rated_vout_mv": rated_vout_mv,
        "output_profile": "19v" if rated_vout_mv == 19_000 else "12v" if rated_vout_mv == 12_000 else "unknown",
    }


def validate_profile_hardware_capabilities(
    *,
    profile_key: str,
    identity_payload: Any,
    settings_payload: Any,
) -> dict[str, Any]:
    expected_rated_vout_mv = expected_profile_rated_vout_mv(profile_key)
    expected_output_profile = profile_key
    identity_caps = extract_identity_hardware_capabilities(identity_payload)
    settings_caps = extract_settings_hardware_capabilities(settings_payload)
    failures: list[str] = []
    if identity_caps.get("output_profile") != expected_output_profile:
        failures.append("identity_output_profile_mismatch")
    if identity_caps.get("rated_vout_mv") != expected_rated_vout_mv:
        failures.append("identity_rated_vout_mismatch")
    if settings_caps.get("output_profile") != expected_output_profile:
        failures.append("settings_output_profile_mismatch")
    if settings_caps.get("rated_vout_mv") != expected_rated_vout_mv:
        failures.append("settings_rated_vout_mismatch")
    return {
        "ok": not failures,
        "failures": failures,
        "expected": {
            "output_profile": expected_output_profile,
            "rated_vout_mv": expected_rated_vout_mv,
        },
        "identity_hardware_capabilities": identity_caps,
        "settings_hardware_capabilities": settings_caps,
    }


def validate_dual_surface_hardware_capabilities(
    *,
    profile_key: str,
    usb_identity_payload: Any,
    usb_settings_payload: Any,
    http_identity_payload: Any,
    http_settings_payload: Any,
) -> dict[str, Any]:
    usb_gate = validate_profile_hardware_capabilities(
        profile_key=profile_key,
        identity_payload=usb_identity_payload,
        settings_payload=usb_settings_payload,
    )
    http_gate = validate_profile_hardware_capabilities(
        profile_key=profile_key,
        identity_payload=http_identity_payload,
        settings_payload=http_settings_payload,
    )
    failures = list(usb_gate["failures"]) + [f"http:{item}" for item in http_gate["failures"]]
    usb_identity_caps = usb_gate["identity_hardware_capabilities"]
    http_identity_caps = http_gate["identity_hardware_capabilities"]
    usb_settings_caps = usb_gate["settings_hardware_capabilities"]
    http_settings_caps = http_gate["settings_hardware_capabilities"]
    if usb_identity_caps != http_identity_caps:
        failures.append("usb_http_identity_caps_mismatch")
    if usb_settings_caps != http_settings_caps:
        failures.append("usb_http_settings_caps_mismatch")
    return {
        "ok": not failures,
        "failures": failures,
        "expected": usb_gate["expected"],
        "usb": usb_gate,
        "http": http_gate,
    }


def chart_title_for(profile_key: str, scene_key: str) -> str:
    scene_label = "Assist Path" if scene_key == "assist_path" else "Backup Only"
    return f"{profile_key.upper()} Formal Scene / {scene_label}"


def build_runner_cmd(
    args: argparse.Namespace,
    *,
    profile_key: str,
    scene_key: str,
) -> list[str]:
    profile = PROFILES[profile_key]
    scene = SCENES[scene_key]
    profile_name = f"formal-{profile_key}-{scene['target_ma']}-{scene_key}"
    observe_device_id = args.ups_device_id
    observe_urls = normalized_observe_urls(args)
    bridge_url = effective_load_bridge_url(args)
    cmd = [
        sys.executable,
        args.runner,
        "--profile-name",
        profile_name,
        "--output-profile",
        profile_key,
        "--scene-type",
        scene_key,
        "--target-ma",
        str(scene["target_ma"]),
        "--load-device",
        args.load_device,
        "--load-usb-port",
        args.load_usb_port,
        "--load-cli",
        args.load_cli,
        "--load-bridge-url",
        bridge_url,
        "--load-devd-socket",
        args.load_devd_socket,
        "--mains-aegis-cli",
        args.mains_aegis_cli,
        "--ups-device-id",
        args.ups_device_id,
        "--load-min-v-mv",
        str(args.load_min_v_mv),
        "--max-i-ma-total",
        str(args.max_i_ma_total),
        "--max-p-mw",
        str(args.max_p_mw),
        "--isolapurr-cli",
        args.isolapurr_cli,
        "--isolapurr-url",
        args.isolapurr_url,
        "--isolapurr-device-id",
        getattr(args, "isolapurr_device_id", DEFAULT_ISOLAPURR_DEVICE_ID),
        "--ups-status-url",
        observe_urls["ups_status_url"],
        "--ups-settings-url",
        observe_urls["ups_settings_url"],
        "--devd-power-diag-url",
        observe_urls["devd_power_diag_url"],
        "--devd-monitor-start-url",
        derive_monitor_start_url(observe_urls["devd_power_diag_url"], observe_device_id),
        "--devd-device-trace-url",
        observe_urls["devd_device_trace_url"],
        "--devd-scan-url",
        args.devd_scan_url,
        "--source-voltage-mv",
        str(profile["source_voltage_mv"]),
        "--source-current-limit-ma",
        str(profile["source_current_limit_ma"]),
        "--pre-seconds",
        str(args.pre_seconds),
        "--hold-seconds",
        str(args.hold_seconds),
        "--backup-hold-seconds",
        str(args.backup_hold_seconds),
        "--restore-hold-seconds",
        str(args.restore_hold_seconds),
        "--post-seconds",
        str(args.post_seconds),
        "--sample-interval-seconds",
        str(args.sample_interval_seconds),
        "--load-stream-interval-seconds",
        str(args.load_stream_interval_seconds),
        "--load-status-ready-timeout-sec",
        str(args.load_status_ready_timeout_sec),
        "--command-timeout-sec",
        str(args.command_timeout_sec),
        "--status-timeout-sec",
        str(args.status_timeout_sec),
        "--verify-timeout-sec",
        str(args.verify_timeout_sec),
        "--report-root",
        args.report_root,
    ]
    if args.mains_aegis_ipc:
        cmd.extend(["--mains-aegis-ipc", args.mains_aegis_ipc])
    if args.load_ipc:
        cmd.extend(["--load-ipc", args.load_ipc])
    if args.load_devd_socket:
        cmd.extend(["--load-devd-socket", args.load_devd_socket])
    if args.load_bridge_device:
        cmd.extend(["--load-bridge-device", args.load_bridge_device])
    if args.load_devd_base_url:
        cmd.extend(["--load-devd-base-url", args.load_devd_base_url])
    if scene["include_backup"]:
        cmd.append("--include-backup")
    return cmd


def prepare_scene_source_and_capability_gate(
    args: argparse.Namespace,
    *,
    profile_key: str,
    observe_urls: dict[str, str],
    dry_run: bool,
) -> dict[str, Any]:
    profile = PROFILES[profile_key]
    observe_device_id = observe_device_id_from_args(args)
    scene_actions: list[dict[str, Any]] = []

    source_reachability_gate = probe_isolapurr_source_reachability(
        isolapurr_cli=args.isolapurr_cli,
        isolapurr_url=args.isolapurr_url,
        timeout_sec=min(args.status_timeout_sec, 5.0),
        dry_run=dry_run,
        expected_device_id=getattr(args, "isolapurr_device_id", DEFAULT_ISOLAPURR_DEVICE_ID),
    )
    scene_actions.append({"source_reachability_gate_before_scene": source_reachability_gate})
    if not dry_run and source_reachability_gate.get("ok") is not True:
        return {
            "ok": False,
            "failures": [
                "source_reachability_gate_before_scene_failed",
                *list(source_reachability_gate.get("failures") or []),
            ],
            "actions": scene_actions,
        }

    load_disable = disable_load(
        load_cli=args.load_cli,
        load_device=args.load_device,
        dry_run=dry_run,
    )
    scene_actions.append({"disable_load_before_scene": load_disable})

    source_cut = cut_source_power_only(
        isolapurr_url=args.isolapurr_url,
        dry_run=dry_run,
    )
    scene_actions.append({"cut_source_before_scene": source_cut})

    refresh_snapshot = {"dry_run": True, "url": args.devd_scan_url} if dry_run else {
        "url": args.devd_scan_url,
        "result": http_post_json(args.devd_scan_url),
    }
    scene_actions.append({"refresh_devd_devices_before_scene": refresh_snapshot})
    control_refresh_snapshot = refresh_control_devices(args, dry_run=dry_run)
    scene_actions.append({"refresh_control_devices_before_scene": control_refresh_snapshot})
    seeded_devd_device = devd_device_entry_from_scan(
        refresh_snapshot.get("result"),
        device_id=observe_device_id,
    )
    if seeded_devd_device_is_capability_ready(seeded_devd_device):
        connect_snapshot = {
            "skipped": True,
            "reason": "already_connected_per_scan_snapshot_re_reading_usb_truth",
        }
    else:
        connect_snapshot = connect_device_with_retry(
            args,
            dry_run=dry_run,
            timeout_sec=min(args.status_timeout_sec, 15.0),
        )
    identity_snapshot = read_device_identity(args, dry_run=dry_run)
    settings_snapshot = read_device_settings(args, dry_run=dry_run)
    scene_actions.append({"connect_device_before_scene": connect_snapshot})
    scene_actions.append({"device_identity_before_scene": identity_snapshot})
    scene_actions.append({"device_settings_before_scene": settings_snapshot})
    direct_lan_base_url = direct_lan_base_url_from_identity(identity_snapshot.get("result"))
    pre_scene_status_url = (
        f"{direct_lan_base_url}/api/v1/status"
        if direct_lan_base_url
        else observe_urls["ups_status_url"]
    )
    source_cut_gate = wait_for_ups_external_input_cut(
        status_url=pre_scene_status_url,
        timeout_sec=min(args.status_timeout_sec, 10.0),
        dry_run=dry_run,
    )
    scene_actions.append({"ups_input_cut_before_scene": source_cut_gate})
    if not dry_run and source_cut_gate.get("ok") is not True:
        return {
            "ok": False,
            "failures": [
                "ups_input_cut_before_scene_failed",
                *list(
                    (
                        dict(source_cut_gate.get("validation") or {}).get("failures")
                        or []
                    )
                ),
            ],
            "actions": scene_actions,
        }

    if dry_run:
        http_identity_snapshot = {
            "dry_run": True,
            "url": (
                f"{direct_lan_base_url}/api/v1/identity"
                if direct_lan_base_url
                else direct_lan_url_from_status_url(observe_urls["ups_status_url"], "/api/v1/identity")
            ),
        }
        http_settings_snapshot = {
            "dry_run": True,
            "url": (
                f"{direct_lan_base_url}/api/v1/settings"
                if direct_lan_base_url
                else direct_lan_url_from_status_url(observe_urls["ups_status_url"], "/api/v1/settings")
            ),
        }
    else:
        http_identity_url = (
            f"{direct_lan_base_url}/api/v1/identity"
            if direct_lan_base_url
            else direct_lan_url_from_status_url(observe_urls["ups_status_url"], "/api/v1/identity")
        )
        http_settings_url = (
            f"{direct_lan_base_url}/api/v1/settings"
            if direct_lan_base_url
            else direct_lan_url_from_status_url(observe_urls["ups_status_url"], "/api/v1/settings")
        )
        http_identity_snapshot = {
            "url": http_identity_url,
            "result": http_get_json(http_identity_url),
        }
        http_settings_snapshot = {
            "url": http_settings_url,
            "result": http_get_json(http_settings_url),
        }
    scene_actions.append({"http_identity_before_scene": http_identity_snapshot})
    scene_actions.append({"http_settings_before_scene": http_settings_snapshot})

    capability_gate = validate_dual_surface_hardware_capabilities(
        profile_key=profile_key,
        usb_identity_payload=identity_snapshot.get("result"),
        usb_settings_payload=settings_snapshot.get("result"),
        http_identity_payload=http_identity_snapshot.get("result"),
        http_settings_payload=http_settings_snapshot.get("result"),
    )
    scene_actions.append({"hardware_capability_gate_before_scene": capability_gate})
    if not dry_run and capability_gate.get("ok") is not True:
        return {
            "ok": False,
            "failures": [
                "hardware_capability_gate_before_scene_failed",
                *list(capability_gate.get("failures") or []),
            ],
            "actions": scene_actions,
        }

    configure_source = configure_source_manual_output(
        isolapurr_cli=args.isolapurr_cli,
        isolapurr_url=args.isolapurr_url,
        voltage_mv=profile["source_voltage_mv"],
        current_limit_ma=profile["source_current_limit_ma"],
        dry_run=dry_run,
    )
    scene_actions.append({"configure_source_before_scene": configure_source})
    ports_snapshot_before_enable = (
        {"dry_run": True, "url": f"{args.isolapurr_url.rstrip('/')}/api/v1/ports"}
        if dry_run
        else http_get_json(f"{args.isolapurr_url.rstrip('/')}/api/v1/ports")
    )
    power_show_snapshot_before_enable = (
        {"dry_run": True, "source": "cli_power_show"}
        if dry_run
        else runner.fetch_isolapurr_power_show_best_effort(
            args.isolapurr_url,
            timeout_sec=min(args.status_timeout_sec, 5.0),
            isolapurr_cli=args.isolapurr_cli,
        )
    )
    source_gate = (
        {
            "ok": True,
            "dry_run": True,
            "expected": {
                "voltage_mv": profile["source_voltage_mv"],
                "current_limit_ma": profile["source_current_limit_ma"],
                "power_enabled": False,
            },
        }
        if dry_run
        else runner.validate_isolapurr_source_configuration(
            expected_voltage_mv=profile["source_voltage_mv"],
            expected_current_limit_ma=profile["source_current_limit_ma"],
            manual_ack_payload=configure_source,
            power_show_payload=power_show_snapshot_before_enable,
            ports_payload=ports_snapshot_before_enable,
        )
    )
    scene_actions.append({"source_power_show_before_scene": power_show_snapshot_before_enable})
    scene_actions.append({"source_configuration_gate_before_scene": source_gate})
    if not dry_run and source_gate.get("ok") is not True:
        return {
            "ok": False,
            "failures": [
                "source_configuration_gate_before_scene_failed",
                *list(source_gate.get("failures") or []),
            ],
            "actions": scene_actions,
        }

    return {
        "ok": True,
        "profile": profile_key,
        "expected_source_voltage_mv": profile["source_voltage_mv"],
        "expected_source_current_limit_ma": profile["source_current_limit_ma"],
        "actions": scene_actions,
    }


def run_formal_scene(
    args: argparse.Namespace,
    *,
    profile_key: str,
    scene_key: str,
    dry_run: bool,
) -> dict[str, Any]:
    observe_urls = normalized_observe_urls(args)
    scene_gate = prepare_scene_source_and_capability_gate(
        args,
        profile_key=profile_key,
        observe_urls=observe_urls,
        dry_run=dry_run,
    )
    if not dry_run and scene_gate.get("ok") is not True:
        raise SystemExit(
            f"scene gate failed for {profile_key}/{scene_key}: {scene_gate.get('failures')}"
        )
    cmd = build_runner_cmd(args, profile_key=profile_key, scene_key=scene_key)
    if dry_run:
        return {
            "profile": profile_key,
            "scene": scene_key,
            "dry_run": True,
            "scene_gate": scene_gate,
            "cmd": cmd,
        }
    payload = run_json(cmd)
    return {
        "profile": profile_key,
        "scene": scene_key,
        "scene_gate": scene_gate,
        "cmd": cmd,
        "result": payload,
        "run_dir": payload.get("run_dir"),
    }


def derive_monitor_start_url(devd_power_diag_url: str, ups_device_id: str) -> str:
    parsed = urllib.parse.urlparse(devd_power_diag_url)
    path_parts = [part for part in parsed.path.split("/") if part]
    try:
        devices_idx = path_parts.index("devices")
    except ValueError:
        return devd_power_diag_url
    base_parts = path_parts[: devices_idx + 2]
    if len(base_parts) < devices_idx + 2:
        base_parts.append(urllib.parse.quote(ups_device_id, safe=""))
    base_parts[-1] = urllib.parse.quote(ups_device_id, safe="")
    new_path = "/" + "/".join(base_parts) + "/monitor/start"
    return urllib.parse.urlunparse((parsed.scheme, parsed.netloc, new_path, "", "", ""))


def output_profile_guard(profile_key: str) -> dict[str, int]:
    if profile_key == "12v":
        return {"min_mv": 11000, "max_mv": 12500}
    if profile_key == "19v":
        return {"min_mv": 18000, "max_mv": 19500}
    raise ValueError(profile_key)


def suite_timestamp() -> str:
    return datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")


def load_json(path: Path) -> Any:
    return json.loads(path.read_text())


def maybe_render_chart(
    args: argparse.Namespace,
    *,
    run_dir: Path,
    profile_key: str,
    scene_key: str,
    dry_run: bool,
) -> dict[str, Any]:
    timeseries_path = run_dir / "timeseries.jsonl"
    chart_path = run_dir / "voltage-chart.html"
    cmd = [
        sys.executable,
        args.chart_script,
        "--input",
        str(timeseries_path),
        "--output",
        str(chart_path),
        "--title",
        chart_title_for(profile_key, scene_key),
    ]
    if dry_run:
        return {"dry_run": True, "cmd": cmd, "chart_path": str(chart_path)}
    run(cmd)
    return {"cmd": cmd, "chart_path": str(chart_path)}


def build_report_entry(
    *,
    report_root: Path,
    run_dir: Path,
    profile_key: str,
    scene_key: str,
    artifact_identity: Any,
) -> tuple[dict[str, Any], dict[str, Any]]:
    results_path = run_dir / "results.json"
    payload = load_json(results_path)
    metadata = dict(payload.get("metadata") or {})
    settings_snapshot = dict(payload.get("settings_snapshot") or {})
    summary_all = dict((payload.get("summary") or {}).get("all") or {})
    completeness = dict(summary_all.get("completeness") or {})
    acceptance = dict(summary_all.get("acceptance") or {})
    samples = payload.get("samples") or []
    source_online = [
        sample.get("isolapurr_port_c_mv")
        for sample in samples
        if sample.get("port_c_enabled") is True
        and isinstance(sample.get("isolapurr_port_c_mv"), (int, float))
    ]
    report_dir_value: str
    try:
        report_dir_value = str(run_dir.relative_to(report_root))
    except ValueError:
        report_dir_value = str(run_dir)
    entry = {
        "output_profile": profile_key,
        "scene_type": scene_key,
        "target_ma": metadata.get("target_ma"),
        "include_backup": bool(metadata.get("include_backup")),
        "load_status_source": metadata.get("load_status_source"),
        "load_min_v_mv": metadata.get("load_min_v_mv"),
        "load_max_i_ma_total": metadata.get("max_i_ma_total"),
        "load_max_p_mw": metadata.get("max_p_mw"),
        "source_voltage_mv": metadata.get("source_voltage_mv"),
        "source_current_limit_ma": metadata.get("source_current_limit_ma"),
        "report_dir": report_dir_value,
        "scene_complete": bool(completeness.get("scene_complete")),
        "run_validity": acceptance.get("run_validity"),
        "signoff_valid": acceptance.get("signoff_valid"),
        "failures": list(completeness.get("failures") or []),
        "failed_acceptance_checks": list(acceptance.get("failed_acceptance_checks") or []),
        "effective_sample_rate_hz": completeness.get("effective_sample_rate_hz"),
        "max_sample_gap_s": completeness.get("max_sample_gap_s"),
        "source_online_mv_min": min(source_online) if source_online else None,
        "source_online_mv_max": max(source_online) if source_online else None,
        "artifact_identity": artifact_identity,
        "advanced_power": dict(settings_snapshot.get("advanced_power") or {}),
    }
    return entry, payload


def main() -> int:
    args = parse_args()
    runner.normalize_load_transport_args(args)
    suite_id = args.suite_id or f"{suite_timestamp()}-formal-dual-voltage-suite"
    report_root = Path(args.report_root).resolve()
    report_root.mkdir(parents=True, exist_ok=True)
    summary_path = report_root / f"{suite_id}-summary.json"
    verification_path = report_root / f"{suite_id}-verification.json"
    overview_path = report_root / f"{suite_id}-overview.html"

    suite_actions: list[dict[str, Any]] = []
    report_entries: list[dict[str, Any]] = []
    latest_advanced_power: dict[str, Any] | None = None
    artifact_selection_by_profile: dict[str, Any] = {}
    observe_device_id = observe_device_id_from_args(args)
    observe_urls = normalized_observe_urls(args)

    for profile_key in args.output_profiles:
        profile = PROFILES[profile_key]
        cut_load_before_flash = disable_load(
            load_cli=args.load_cli,
            load_device=args.load_device,
            dry_run=args.dry_run,
        )
        source_reachability_gate = probe_isolapurr_source_reachability(
            isolapurr_cli=args.isolapurr_cli,
            isolapurr_url=args.isolapurr_url,
            timeout_sec=min(args.status_timeout_sec, 5.0),
            dry_run=args.dry_run,
            expected_device_id=getattr(args, "isolapurr_device_id", DEFAULT_ISOLAPURR_DEVICE_ID),
        )
        suite_actions.append({"source_reachability_gate": source_reachability_gate})
        if not args.dry_run and source_reachability_gate.get("ok") is not True:
            raise SystemExit(
                f"source reachability gate failed before {profile_key} profile prepare: "
                f"{source_reachability_gate.get('failures')}"
            )
        cut_source_before_flash = cut_source_power_only(
            isolapurr_url=args.isolapurr_url,
            dry_run=args.dry_run,
        )
        suite_actions.append(
            {
                "profile_prepare": {
                    "profile": profile_key,
                    "power_off_gate": (
                        "disable load -> cut IsolaPurr port_c without reprogramming source -> artifact switch/flash"
                    ),
                    "cut_load_before_flash": cut_load_before_flash,
                    "cut_source_before_flash": cut_source_before_flash,
                }
            }
        )
        if not args.skip_flash:
            manifest_path = artifact_manifest_for_profile(args, profile_key)
            if manifest_path is None:
                raise SystemExit(
                    f"missing artifact manifest for {profile_key}; "
                    f"provide --artifact-manifest-{profile_key}"
                )
            flash_result = select_and_flash_artifact(
                args,
                profile_key=profile_key,
                manifest_path=manifest_path,
                dry_run=args.dry_run,
            )
            suite_actions.append({"flash_profile": flash_result})

        artifact_snapshot = read_selected_artifact(args, dry_run=args.dry_run)
        suite_actions.append({"selected_artifact": artifact_snapshot})
        artifact_selection_by_profile[profile_key] = artifact_snapshot.get("result")

        refresh_snapshot = {"dry_run": True, "url": args.devd_scan_url} if args.dry_run else {
            "url": args.devd_scan_url,
            "result": http_post_json(args.devd_scan_url),
        }
        suite_actions.append({"refresh_devd_devices_before_capability_check": refresh_snapshot})
        control_refresh_snapshot = refresh_control_devices(args, dry_run=args.dry_run)
        suite_actions.append({"refresh_control_devices_before_capability_check": control_refresh_snapshot})
        seeded_devd_device = devd_device_entry_from_scan(
            refresh_snapshot.get("result"),
            device_id=observe_device_id,
        )
        if seeded_devd_device_is_capability_ready(seeded_devd_device):
            connect_snapshot = {
                "skipped": True,
                "reason": "already_connected_per_scan_snapshot_re_reading_usb_truth",
            }
        else:
            connect_snapshot = connect_device_with_retry(
                args,
                dry_run=args.dry_run,
                timeout_sec=min(args.status_timeout_sec, 15.0),
            )
        identity_snapshot = read_device_identity(args, dry_run=args.dry_run)
        settings_snapshot = read_device_settings(args, dry_run=args.dry_run)
        suite_actions.append({"connect_device_before_capability_check": connect_snapshot})
        suite_actions.append({"device_identity_before_source_restore": identity_snapshot})
        suite_actions.append({"device_settings_before_source_restore": settings_snapshot})
        direct_lan_base_url = direct_lan_base_url_from_identity(identity_snapshot.get("result"))
        post_connect_status_url = (
            f"{direct_lan_base_url}/api/v1/status"
            if direct_lan_base_url
            else observe_urls["ups_status_url"]
        )
        source_cut_gate = wait_for_ups_external_input_cut(
            status_url=post_connect_status_url,
            timeout_sec=min(args.status_timeout_sec, 10.0),
            dry_run=args.dry_run,
        )
        suite_actions.append({"ups_input_cut_before_profile_switch": source_cut_gate})
        if not args.dry_run and source_cut_gate.get("ok") is not True:
            raise SystemExit(
                f"UPS input-cut gate failed before {profile_key} profile prepare: "
                f"{source_cut_gate.get('validation', {}).get('failures')}"
            )
        http_identity_snapshot: dict[str, Any]
        http_settings_snapshot: dict[str, Any]
        if args.dry_run:
            http_identity_snapshot = {
                "dry_run": True,
                "url": (
                    f"{direct_lan_base_url}/api/v1/identity"
                    if direct_lan_base_url
                    else direct_lan_url_from_status_url(observe_urls["ups_status_url"], "/api/v1/identity")
                ),
            }
            http_settings_snapshot = {
                "dry_run": True,
                "url": (
                    f"{direct_lan_base_url}/api/v1/settings"
                    if direct_lan_base_url
                    else direct_lan_url_from_status_url(observe_urls["ups_status_url"], "/api/v1/settings")
                ),
            }
        else:
            http_identity_url = (
                f"{direct_lan_base_url}/api/v1/identity"
                if direct_lan_base_url
                else direct_lan_url_from_status_url(observe_urls["ups_status_url"], "/api/v1/identity")
            )
            http_settings_url = (
                f"{direct_lan_base_url}/api/v1/settings"
                if direct_lan_base_url
                else direct_lan_url_from_status_url(observe_urls["ups_status_url"], "/api/v1/settings")
            )
            http_identity_snapshot = {
                "url": http_identity_url,
                "result": http_get_json(http_identity_url),
            }
            http_settings_snapshot = {
                "url": http_settings_url,
                "result": http_get_json(http_settings_url),
            }
        suite_actions.append({"http_identity_before_source_restore": http_identity_snapshot})
        suite_actions.append({"http_settings_before_source_restore": http_settings_snapshot})
        capability_gate = validate_dual_surface_hardware_capabilities(
            profile_key=profile_key,
            usb_identity_payload=identity_snapshot.get("result"),
            usb_settings_payload=settings_snapshot.get("result"),
            http_identity_payload=http_identity_snapshot.get("result"),
            http_settings_payload=http_settings_snapshot.get("result"),
        )
        suite_actions.append({"hardware_capability_gate": capability_gate})
        if not args.dry_run and capability_gate.get("ok") is not True:
            raise SystemExit(
                f"hardware capability gate failed for {profile_key}: {capability_gate.get('failures')}"
            )
        post_flash_status_url = (
            f"{direct_lan_base_url}/api/v1/status"
            if direct_lan_base_url
            else observe_urls["ups_status_url"]
        )
        source_cut_gate_after_flash = wait_for_ups_external_input_cut(
            status_url=post_flash_status_url,
            timeout_sec=min(args.status_timeout_sec, 10.0),
            dry_run=args.dry_run,
        )
        suite_actions.append({"ups_input_cut_before_source_restore": source_cut_gate_after_flash})
        if not args.dry_run and source_cut_gate_after_flash.get("ok") is not True:
            raise SystemExit(
                f"UPS input-cut gate failed before source restore for {profile_key}: "
                f"{source_cut_gate_after_flash.get('validation', {}).get('failures')}"
            )

        restore_source_result = configure_source_manual_output(
            isolapurr_cli=args.isolapurr_cli,
            isolapurr_url=args.isolapurr_url,
            voltage_mv=profile["source_voltage_mv"],
            current_limit_ma=profile["source_current_limit_ma"],
            dry_run=args.dry_run,
        )
        suite_actions.append({"configure_source_after_profile_check": restore_source_result})
        if not args.dry_run:
            ports_snapshot_before_enable = http_get_json(
                f"{args.isolapurr_url.rstrip('/')}/api/v1/ports"
            )
            source_gate = validate_source_configuration(
                expected_voltage_mv=profile["source_voltage_mv"],
                expected_current_limit_ma=profile["source_current_limit_ma"],
                set_source_payload=restore_source_result,
                ports_payload=ports_snapshot_before_enable,
            )
            suite_actions.append({"source_configuration_gate": source_gate})
            if source_gate.get("ok") is not True:
                raise SystemExit(
                    f"source configuration gate failed for {profile_key}: {source_gate.get('failures')}"
                )
        suite_actions.append(
            {
                "scene_start_gate": {
                    "profile": profile_key,
                    "reason": (
                        "keep source disabled here; each formal scene must begin from its own "
                        "source-off -> capability verified -> source configured/readback -> source-on path"
                    ),
                    "source_enable_delegated_to_runner": True,
                }
            }
        )

        for scene_key in args.scenes:
            run_payload = run_formal_scene(
                args,
                profile_key=profile_key,
                scene_key=scene_key,
                dry_run=args.dry_run,
            )
            suite_actions.append({"run_scene": run_payload})
            if args.dry_run:
                continue

            run_dir = Path(run_payload["run_dir"]).resolve()
            chart_result = maybe_render_chart(
                args,
                run_dir=run_dir,
                profile_key=profile_key,
                scene_key=scene_key,
                dry_run=False,
            )
            suite_actions.append({"render_scene_chart": chart_result})

            report_entry, results_payload = build_report_entry(
                report_root=report_root,
                run_dir=run_dir,
                profile_key=profile_key,
                scene_key=scene_key,
                artifact_identity=artifact_selection_by_profile.get(profile_key),
            )
            latest_advanced_power = dict(
                ((results_payload.get("settings_snapshot") or {}).get("advanced_power") or {})
            )
            report_entries.append(report_entry)

    suite_summary = {
        "suite_id": suite_id,
        "objective": (
            "Formal dual-voltage HIL suite with four required scenes and one overview HTML."
        ),
        "transport": {
            "load_cli": args.load_cli,
            "load_status_source": (
                report_entries[0].get("load_status_source") if report_entries else None
            ),
            "load_ipc": args.load_ipc,
            "load_devd_socket": args.load_devd_socket,
            "load_devd_base_url": args.load_devd_base_url,
            "load_bridge_url": effective_load_bridge_url(args),
            "load_usb_port": args.load_usb_port,
            "mains_aegis_ipc": args.mains_aegis_ipc,
            "ups_control_device_id": args.ups_device_id,
            "ups_observe_device_id": observe_device_id,
            "ups_status_url": args.ups_status_url,
            "ups_settings_url": args.ups_settings_url,
            "devd_power_diag_url": args.devd_power_diag_url,
            "normalized_ups_status_url": observe_urls["ups_status_url"],
            "normalized_ups_settings_url": observe_urls["ups_settings_url"],
            "normalized_devd_power_diag_url": observe_urls["devd_power_diag_url"],
            "isolapurr_url": args.isolapurr_url,
        },
        "load_protection": {
            "min_v_mv": args.load_min_v_mv,
            "max_i_ma_total": args.max_i_ma_total,
            "max_p_mw": args.max_p_mw,
        },
        "profiles": {
            profile_key: {
                "source_voltage_mv": PROFILES[profile_key]["source_voltage_mv"],
                "source_current_limit_ma": PROFILES[profile_key]["source_current_limit_ma"],
                "expected_source_window_mv": output_profile_guard(profile_key),
                "artifact_features": PROFILES[profile_key]["artifact_features"],
                "artifact_manifest": artifact_manifest_for_profile(args, profile_key),
                "selected_artifact": artifact_selection_by_profile.get(profile_key),
            }
            for profile_key in args.output_profiles
        },
        "advanced_power": latest_advanced_power or {},
        "reports": report_entries,
        "actions": suite_actions,
    }
    summary_path.write_text(
        json.dumps(suite_summary, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )

    if not args.dry_run and not args.skip_verify:
        verify_cmd = [sys.executable, args.verify_script, "--summary", str(summary_path)]
        verify_payload = run_json(verify_cmd)
        verification_path.write_text(
            json.dumps(verify_payload, ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
        )

    if not args.dry_run and not args.skip_overview:
        overview_cmd = [
            sys.executable,
            args.overview_script,
            "--summary",
            str(summary_path),
            "--output",
            str(overview_path),
        ]
        run(overview_cmd)

    print(
        json.dumps(
            {
                "suite_id": suite_id,
                "summary": str(summary_path),
                "verification": str(verification_path) if verification_path.exists() else None,
                "overview": str(overview_path) if overview_path.exists() else None,
                "report_count": len(report_entries),
                "dry_run": args.dry_run,
            },
            ensure_ascii=False,
            indent=2,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
