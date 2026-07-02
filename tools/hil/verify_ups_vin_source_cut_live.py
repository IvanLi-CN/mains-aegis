#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import time
import urllib.parse
import urllib.request
from typing import Any


DEFAULT_UPS_STATUS_URL = None
DEFAULT_ISOLAPURR_URL = None
PORTS_PATH = "/api/v1/ports"
PORT_C_POWER_PATH = "/api/v1/ports/port_c/power"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Live-check that UPS VIN follows a real IsolaPurr source cut."
    )
    parser.add_argument("--ups-status-url", default=DEFAULT_UPS_STATUS_URL)
    parser.add_argument("--isolapurr-url", default=DEFAULT_ISOLAPURR_URL)
    parser.add_argument("--cut-settle-seconds", type=float, default=1.0)
    parser.add_argument("--backup-settle-seconds", type=float, default=2.0)
    parser.add_argument("--restore-settle-seconds", type=float, default=2.0)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()
    if not (args.ups_status_url or "").strip():
        parser.error("--ups-status-url is required; no UPS URL is built in")
    if not (args.isolapurr_url or "").strip():
        parser.error("--isolapurr-url is required; no source URL is built in")
    return args


def http_json(url: str, *, method: str = "GET") -> Any:
    request = urllib.request.Request(url, method=method)
    with urllib.request.urlopen(request, timeout=10) as response:
        return json.load(response)


def set_port_c_power(isolapurr_url: str, enabled: bool) -> dict[str, Any]:
    query = urllib.parse.urlencode({"enabled": "1" if enabled else "0"})
    payload = http_json(
        f"{isolapurr_url.rstrip('/')}{PORT_C_POWER_PATH}?{query}",
        method="POST",
    )
    return {"enabled": enabled, "response": payload}


def read_ups_status(ups_status_url: str) -> dict[str, Any]:
    return dict(http_json(ups_status_url))


def read_isolapurr_ports(isolapurr_url: str) -> dict[str, Any]:
    return dict(http_json(f"{isolapurr_url.rstrip('/')}{PORTS_PATH}"))


def port_c_snapshot(payload: dict[str, Any]) -> dict[str, Any]:
    ports = payload.get("ports")
    if isinstance(ports, dict):
        ports = ports.get("ports")
    if not isinstance(ports, list):
        ports = []
    port_c = next((port for port in ports if port.get("portId") == "port_c"), {})
    telemetry = port_c.get("telemetry") or {}
    state = port_c.get("state") or {}
    return {
        "power_enabled": state.get("power_enabled"),
        "busy": state.get("busy"),
        "status": telemetry.get("status"),
        "voltage_mv": telemetry.get("voltage_mv"),
        "current_ma": telemetry.get("current_ma"),
    }


def ups_snapshot(payload: dict[str, Any]) -> dict[str, Any]:
    input_root = payload.get("input") or {}
    output_root = payload.get("output") or {}
    out_a = output_root.get("out_a") or {}
    out_b = output_root.get("out_b") or {}
    return {
        "mode": payload.get("mode"),
        "mains_present": input_root.get("mains_present"),
        "assist_power_stage": input_root.get("assist_power_stage"),
        "vin_vbus_mv": input_root.get("vin_vbus_mv"),
        "input_vbus_mv": input_root.get("input_vbus_mv"),
        "vin_iin_ma": input_root.get("vin_iin_ma"),
        "tps_total_iout_ma": input_root.get("tps_total_iout_ma"),
        "out_a_vbus_mv": out_a.get("vbus_mv"),
        "out_b_vbus_mv": out_b.get("vbus_mv"),
    }


def build_report(
    *,
    pre_ports: dict[str, Any],
    pre_ups: dict[str, Any],
    cut_action: dict[str, Any],
    cut_ports_1: dict[str, Any],
    cut_ups_1: dict[str, Any],
    cut_ports_2: dict[str, Any],
    cut_ups_2: dict[str, Any],
    restore_action: dict[str, Any],
    restore_ports: dict[str, Any],
    restore_ups: dict[str, Any],
) -> dict[str, Any]:
    pre_vin = pre_ups["vin_vbus_mv"]
    cut_vin = cut_ups_2["vin_vbus_mv"]
    restore_vin = restore_ups["vin_vbus_mv"]
    return {
        "pre_ports": pre_ports,
        "pre_ups": pre_ups,
        "cut_action": cut_action,
        "cut_ports_1": cut_ports_1,
        "cut_ups_1": cut_ups_1,
        "cut_ports_2": cut_ports_2,
        "cut_ups_2": cut_ups_2,
        "restore_action": restore_action,
        "restore_ports": restore_ports,
        "restore_ups": restore_ups,
        "verdict": {
            "port_cut_acknowledged": bool(cut_action.get("response", {}).get("accepted")),
            "port_cut_visible": cut_ports_2.get("power_enabled") is False,
            "ups_detected_cut": (
                cut_ups_2.get("mains_present") is False
                or cut_ups_2.get("mode") == "backup"
                or cut_ups_2.get("assist_power_stage") == "backup"
            ),
            "vin_drop_mv": (
                pre_vin - cut_vin
                if isinstance(pre_vin, (int, float)) and isinstance(cut_vin, (int, float))
                else None
            ),
            "vin_restored_mv": (
                restore_vin
                if isinstance(restore_vin, (int, float))
                else None
            ),
        },
    }


def main() -> int:
    args = parse_args()
    pre_ports = port_c_snapshot(read_isolapurr_ports(args.isolapurr_url))
    pre_ups = ups_snapshot(read_ups_status(args.ups_status_url))
    cut_action: dict[str, Any] | None = None
    restore_action: dict[str, Any] | None = None
    try:
        cut_action = set_port_c_power(args.isolapurr_url, False)
        time.sleep(max(0.0, args.cut_settle_seconds))
        cut_ports_1 = port_c_snapshot(read_isolapurr_ports(args.isolapurr_url))
        cut_ups_1 = ups_snapshot(read_ups_status(args.ups_status_url))
        time.sleep(max(0.0, args.backup_settle_seconds))
        cut_ports_2 = port_c_snapshot(read_isolapurr_ports(args.isolapurr_url))
        cut_ups_2 = ups_snapshot(read_ups_status(args.ups_status_url))
    finally:
        restore_action = set_port_c_power(args.isolapurr_url, True)
    time.sleep(max(0.0, args.restore_settle_seconds))
    restore_ports = port_c_snapshot(read_isolapurr_ports(args.isolapurr_url))
    restore_ups = ups_snapshot(read_ups_status(args.ups_status_url))
    report = build_report(
        pre_ports=pre_ports,
        pre_ups=pre_ups,
        cut_action=cut_action or {},
        cut_ports_1=cut_ports_1,
        cut_ups_1=cut_ups_1,
        cut_ports_2=cut_ports_2,
        cut_ups_2=cut_ups_2,
        restore_action=restore_action or {},
        restore_ports=restore_ports,
        restore_ups=restore_ups,
    )
    if args.json:
        print(json.dumps(report, ensure_ascii=False, indent=2))
    else:
        print(json.dumps(report, ensure_ascii=False))
    verdict = report["verdict"]
    if (
        verdict["port_cut_acknowledged"]
        and verdict["port_cut_visible"]
        and verdict["ups_detected_cut"]
        and isinstance(verdict["vin_drop_mv"], (int, float))
        and verdict["vin_drop_mv"] > 0
    ):
        return 0
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
