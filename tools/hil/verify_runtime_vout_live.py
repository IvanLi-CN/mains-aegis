#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import sys
import time
import urllib.error
import urllib.request
from dataclasses import dataclass
from typing import Any


DEFAULT_UPS_BASE_URL = None
DEFAULT_TIMEOUT_SECONDS = 8.0
DEFAULT_STATUS_POLL_SECONDS = 0.5
DEFAULT_STATUS_DEADLINE_SECONDS = 10.0
DEFAULT_STANDBY_DROP_STEP_MV = 100


class CheckError(RuntimeError):
    pass


@dataclass
class Snapshot:
    standby_drop_mv: int
    assist_target_vout_mv: int | None
    out_a_vbus_mv: int | None
    out_b_vbus_mv: int | None
    mode: str | None
    stage: str | None

    @classmethod
    def from_payload(cls, *, settings: dict[str, Any], status: dict[str, Any]) -> "Snapshot":
        advanced_power = dict_or_empty(settings.get("advanced_power"))
        input_section = dict_or_empty(status.get("input"))
        output_section = dict_or_empty(status.get("output"))
        out_a = dict_or_empty(output_section.get("out_a"))
        out_b = dict_or_empty(output_section.get("out_b"))
        return cls(
            standby_drop_mv=int(advanced_power["standby_drop_mv"]),
            assist_target_vout_mv=optional_int(input_section.get("assist_target_vout_mv")),
            out_a_vbus_mv=optional_int(out_a.get("vbus_mv")),
            out_b_vbus_mv=optional_int(out_b.get("vbus_mv")),
            mode=optional_str(status.get("mode")),
            stage=optional_str(input_section.get("assist_power_stage")),
        )

    def to_json(self) -> dict[str, Any]:
        return {
            "standby_drop_mv": self.standby_drop_mv,
            "assist_target_vout_mv": self.assist_target_vout_mv,
            "out_a_vbus_mv": self.out_a_vbus_mv,
            "out_b_vbus_mv": self.out_b_vbus_mv,
            "mode": self.mode,
            "stage": self.stage,
        }


def dict_or_empty(value: Any) -> dict[str, Any]:
    return value if isinstance(value, dict) else {}


def optional_int(value: Any) -> int | None:
    return value if isinstance(value, int) else None


def optional_str(value: Any) -> str | None:
    return value if isinstance(value, str) else None


def http_json(
    url: str,
    *,
    method: str = "GET",
    body: dict[str, Any] | None = None,
    timeout_seconds: float,
) -> dict[str, Any]:
    data = None if body is None else json.dumps(body).encode("utf-8")
    request = urllib.request.Request(
        url,
        data=data,
        method=method,
        headers={"content-type": "application/json"},
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout_seconds) as response:
            return json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as exc:
        payload = exc.read().decode("utf-8", errors="replace")
        raise CheckError(f"{method} {url} failed: http {exc.code}: {payload}") from exc
    except urllib.error.URLError as exc:
        raise CheckError(f"{method} {url} failed: {exc}") from exc


def build_advanced_power_payload(settings: dict[str, Any], standby_drop_mv: int) -> dict[str, Any]:
    advanced_power = dict_or_empty(settings.get("advanced_power"))
    payload = {
        "standby_drop_mv": standby_drop_mv,
        "assist_low_drop_mv": int(advanced_power["assist_low_drop_mv"]),
        "assist_enter_delta_ma": int(advanced_power["assist_enter_delta_ma"]),
        "assist_exit_delta_ma": int(advanced_power["assist_exit_delta_ma"]),
        "assist_required_samples": int(advanced_power["assist_required_samples"]),
        "assist_ramp_step_mv": int(advanced_power["assist_ramp_step_mv"]),
        "assist_ramp_interval_ms": int(advanced_power["assist_ramp_interval_ms"]),
        "rated_enter_delta_ma": int(advanced_power["rated_enter_delta_ma"]),
        "rated_exit_delta_ma": int(advanced_power["rated_exit_delta_ma"]),
        "vin_drop_threshold_pct": int(advanced_power["vin_drop_threshold_pct"]),
        "required_samples": int(advanced_power["required_samples"]),
    }
    return payload


def read_snapshot(ups_base_url: str, *, timeout_seconds: float) -> tuple[dict[str, Any], dict[str, Any], Snapshot]:
    settings = http_json(f"{ups_base_url}/api/v1/settings", timeout_seconds=timeout_seconds)
    status = http_json(f"{ups_base_url}/api/v1/status", timeout_seconds=timeout_seconds)
    return settings, status, Snapshot.from_payload(settings=settings, status=status)


def wait_for_standby_drop(
    ups_base_url: str,
    *,
    expected_standby_drop_mv: int,
    timeout_seconds: float,
    poll_seconds: float,
    deadline_seconds: float,
) -> tuple[dict[str, Any], dict[str, Any], Snapshot]:
    deadline = time.monotonic() + deadline_seconds
    last: tuple[dict[str, Any], dict[str, Any], Snapshot] | None = None
    while time.monotonic() < deadline:
        last = read_snapshot(ups_base_url, timeout_seconds=timeout_seconds)
        if last[2].standby_drop_mv == expected_standby_drop_mv:
            return last
        time.sleep(poll_seconds)
    if last is None:
        raise CheckError("no snapshot captured while waiting for standby_drop_mv change")
    raise CheckError(
        f"timed out waiting for standby_drop_mv={expected_standby_drop_mv}; "
        f"last_seen={last[2].standby_drop_mv}"
    )


def verify_snapshot_shift(
    before: Snapshot,
    after: Snapshot,
    *,
    expected_target_delta_mv: int,
) -> None:
    if before.mode != "standby" or before.stage != "standby":
        raise CheckError(
            f"expected standby before mutation, got mode={before.mode} stage={before.stage}"
        )
    if after.mode != "standby" or after.stage != "standby":
        raise CheckError(
            f"expected standby after mutation, got mode={after.mode} stage={after.stage}"
        )
    if before.assist_target_vout_mv is None or after.assist_target_vout_mv is None:
        raise CheckError("assist_target_vout_mv missing in status snapshot")
    actual_target_delta_mv = after.assist_target_vout_mv - before.assist_target_vout_mv
    if actual_target_delta_mv != expected_target_delta_mv:
        raise CheckError(
            f"assist_target_vout_mv delta mismatch: expected {expected_target_delta_mv}, "
            f"got {actual_target_delta_mv}"
        )
    if before.out_a_vbus_mv is None or after.out_a_vbus_mv is None:
        raise CheckError("out_a_vbus_mv missing in status snapshot")
    if before.out_b_vbus_mv is None or after.out_b_vbus_mv is None:
        raise CheckError("out_b_vbus_mv missing in status snapshot")
    required_vbus_shift_mv = max(abs(expected_target_delta_mv) // 2, 20)
    out_a_delta_mv = after.out_a_vbus_mv - before.out_a_vbus_mv
    out_b_delta_mv = after.out_b_vbus_mv - before.out_b_vbus_mv
    if out_a_delta_mv > -required_vbus_shift_mv:
        raise CheckError(
            "expected out_a_vbus_mv to move downward by a visible amount, "
            f"before={before.out_a_vbus_mv}, after={after.out_a_vbus_mv}, "
            f"delta={out_a_delta_mv}, required<={-required_vbus_shift_mv}"
        )
    if out_b_delta_mv > -required_vbus_shift_mv:
        raise CheckError(
            "expected out_b_vbus_mv to move downward by a visible amount, "
            f"before={before.out_b_vbus_mv}, after={after.out_b_vbus_mv}, "
            f"delta={out_b_delta_mv}, required<={-required_vbus_shift_mv}"
        )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Live-check that runtime standby target changes apply directly on the UPS"
    )
    parser.add_argument("--ups-base-url", default=DEFAULT_UPS_BASE_URL)
    parser.add_argument("--timeout-seconds", type=float, default=DEFAULT_TIMEOUT_SECONDS)
    parser.add_argument("--status-poll-seconds", type=float, default=DEFAULT_STATUS_POLL_SECONDS)
    parser.add_argument(
        "--status-deadline-seconds",
        type=float,
        default=DEFAULT_STATUS_DEADLINE_SECONDS,
    )
    parser.add_argument(
        "--standby-drop-step-mv",
        type=int,
        default=DEFAULT_STANDBY_DROP_STEP_MV,
        help="positive step applied to standby_drop_mv during the live check",
    )
    args = parser.parse_args()
    if not (args.ups_base_url or "").strip():
        parser.error("--ups-base-url is required; no UPS URL is built in")
    return args


def main() -> int:
    args = parse_args()
    before_settings, before_status, before = read_snapshot(
        args.ups_base_url,
        timeout_seconds=args.timeout_seconds,
    )
    mutated_standby_drop_mv = before.standby_drop_mv + args.standby_drop_step_mv
    mutated_payload = build_advanced_power_payload(before_settings, mutated_standby_drop_mv)
    restored_payload = build_advanced_power_payload(before_settings, before.standby_drop_mv)
    mutated_response: dict[str, Any] | None = None
    restored_response: dict[str, Any] | None = None
    mutated_snapshot: Snapshot | None = None
    restored_snapshot: Snapshot | None = None
    try:
        mutated_response = http_json(
            f"{args.ups_base_url}/api/v1/settings/advanced-power",
            method="POST",
            body=mutated_payload,
            timeout_seconds=args.timeout_seconds,
        )
        _, _, mutated_snapshot = wait_for_standby_drop(
            args.ups_base_url,
            expected_standby_drop_mv=mutated_standby_drop_mv,
            timeout_seconds=args.timeout_seconds,
            poll_seconds=args.status_poll_seconds,
            deadline_seconds=args.status_deadline_seconds,
        )
        verify_snapshot_shift(
            before,
            mutated_snapshot,
            expected_target_delta_mv=-args.standby_drop_step_mv,
        )
    finally:
        restored_response = http_json(
            f"{args.ups_base_url}/api/v1/settings/advanced-power",
            method="POST",
            body=restored_payload,
            timeout_seconds=args.timeout_seconds,
        )
        _, _, restored_snapshot = wait_for_standby_drop(
            args.ups_base_url,
            expected_standby_drop_mv=before.standby_drop_mv,
            timeout_seconds=args.timeout_seconds,
            poll_seconds=args.status_poll_seconds,
            deadline_seconds=args.status_deadline_seconds,
        )

    if mutated_snapshot is None or restored_snapshot is None:
        raise CheckError("runtime-vout live check did not capture mutated/restored snapshots")

    result = {
        "ok": True,
        "ups_base_url": args.ups_base_url,
        "step_mv": args.standby_drop_step_mv,
        "mutated_response": mutated_response,
        "restored_response": restored_response,
        "before": before.to_json(),
        "mutated": mutated_snapshot.to_json(),
        "restored": restored_snapshot.to_json(),
    }
    print(json.dumps(result, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except CheckError as exc:
        print(json.dumps({"ok": False, "error": str(exc)}, ensure_ascii=False, indent=2), file=sys.stderr)
        raise SystemExit(1) from exc
