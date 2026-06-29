#!/usr/bin/env python3
from __future__ import annotations

import argparse
import importlib.util
import json
import time
import urllib.parse
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent
SUITE_PATH = ROOT / "formal_hil_suite.py"
SUITE_SPEC = importlib.util.spec_from_file_location("formal_hil_suite", SUITE_PATH)
if SUITE_SPEC is None or SUITE_SPEC.loader is None:
    raise RuntimeError(f"failed to load formal_hil_suite module from {SUITE_PATH}")
suite = importlib.util.module_from_spec(SUITE_SPEC)
SUITE_SPEC.loader.exec_module(suite)
RUNNER_PATH = ROOT / "advanced_power_12v_runner.py"
RUNNER_SPEC = importlib.util.spec_from_file_location("advanced_power_12v_runner", RUNNER_PATH)
if RUNNER_SPEC is None or RUNNER_SPEC.loader is None:
    raise RuntimeError(f"failed to load advanced_power_12v_runner module from {RUNNER_PATH}")
runner = importlib.util.module_from_spec(RUNNER_SPEC)
RUNNER_SPEC.loader.exec_module(runner)


DEFAULT_REPORT_ROOT = ROOT / "reports"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Prepare the formal HIL bench in a safe pre-connection state and "
            "verify capability/manifests before the operator connects source/load wiring."
        )
    )
    parser.add_argument("--report-root", default=str(DEFAULT_REPORT_ROOT))
    parser.add_argument("--load-device", default=suite.DEFAULT_LOAD_DEVICE)
    parser.add_argument("--load-cli", default=suite.DEFAULT_LOAD_CLI)
    parser.add_argument("--load-bridge-url", default="")
    parser.add_argument("--load-ipc", default=suite.DEFAULT_LOAD_IPC)
    parser.add_argument("--load-devd-base-url", default=suite.DEFAULT_LOAD_DEVD_BASE_URL)
    parser.add_argument("--load-devd-socket", default=suite.DEFAULT_LOAD_DEVD_SOCKET)
    parser.add_argument("--load-usb-device-id", default=runner.DEFAULT_LOAD_USB_DEVICE_ID)
    parser.add_argument("--isolapurr-cli", default=suite.DEFAULT_ISOLAPURR_CLI)
    parser.add_argument("--isolapurr-url", default=suite.DEFAULT_ISOLAPURR_URL)
    parser.add_argument("--isolapurr-device-id", default=suite.DEFAULT_ISOLAPURR_DEVICE_ID)
    parser.add_argument(
        "--mains-aegis-cli",
        default=str(ROOT.parent / "mains-aegis-host" / "target" / "debug" / "mains-aegis"),
    )
    parser.add_argument("--mains-aegis-ipc", default=None)
    parser.add_argument("--ups-device-id", default=suite.DEFAULT_UPS_DEVICE_ID)
    parser.add_argument("--ups-status-url", default=suite.DEFAULT_UPS_STATUS_URL)
    parser.add_argument("--ups-settings-url", default=suite.DEFAULT_UPS_SETTINGS_URL)
    parser.add_argument("--devd-scan-url", default=suite.DEFAULT_UPS_SCAN_URL)
    parser.add_argument(
        "--devd-power-diag-url",
        default=suite.DEFAULT_UPS_POWER_DIAG_URL,
    )
    parser.add_argument(
        "--devd-device-trace-url",
        default=suite.DEFAULT_UPS_TRACE_URL,
    )
    parser.add_argument("--artifact-manifest-12v", default=None)
    parser.add_argument("--artifact-manifest-19v", default=None)
    parser.add_argument("--firmware-bundle-root", default=str(suite.DEFAULT_FIRMWARE_BUNDLE_ROOT))
    parser.add_argument("--status-timeout-sec", type=float, default=20.0)
    parser.add_argument("--telemetry-probe-samples", type=int, default=12)
    parser.add_argument(
        "--telemetry-probe-interval-sec",
        type=float,
        default=1.0 / runner.FORMAL_TARGET_SAMPLE_RATE_HZ,
    )
    parser.add_argument(
        "--skip-safe-prepare",
        action="store_true",
        help="Do not actively disable the load or cut IsolaPurr source power before readback checks.",
    )
    parser.add_argument("--dry-run", action="store_true")
    return parser.parse_args()


def load_status_payload(args: argparse.Namespace, *, load_cli: str, load_device: str, dry_run: bool) -> Any:
    if dry_run:
        return {
            "dry_run": True,
            "control": {
                "output_enabled": False,
                "mode": "cc",
            },
        }
    timeout_sec = min(getattr(args, "status_timeout_sec", 20.0), 5.0)
    try:
        status = runner.get_load_status_via_ipc_helper(
            args,
            timeout_sec=timeout_sec,
            load_devd_lease=None,
        )
        if isinstance(status, dict) and isinstance(status.get("control"), dict):
            return status
    except Exception as exc:
        ipc_error = repr(exc)
    else:
        ipc_error = None
    status = runner.get_load_status_best_effort(
        args,
        load_device,
        timeout_sec=timeout_sec,
    )
    if isinstance(status, dict) and isinstance(status.get("control"), dict):
        return status
    if isinstance(status, dict):
        payload = dict(status)
        if ipc_error and "ipc_helper_error" not in payload:
            payload["ipc_helper_error"] = ipc_error
        return payload
    return {
        "ok": False,
        "error": "load_status_unavailable",
        "ipc_helper_error": ipc_error,
        "load_cli": load_cli,
        "load_device": load_device,
    }


def disable_load_output_acknowledged(actions: list[dict[str, Any]]) -> bool:
    for action in actions:
        payload = action.get("disable_load") if isinstance(action, dict) else None
        if not isinstance(payload, dict):
            continue
        result = payload.get("result")
        if isinstance(result, str) and "output=false" in result:
            return True
    return False


def resolve_devd_scan_url(args: argparse.Namespace) -> str:
    if isinstance(args.devd_scan_url, str) and args.devd_scan_url.strip():
        return args.devd_scan_url.strip()
    for candidate in (args.ups_status_url, args.ups_settings_url):
        device_id = suite.devd_device_id_from_endpoint(candidate)
        if not device_id:
            continue
        parsed = suite.urllib.parse.urlparse(candidate)
        return suite.urllib.parse.urlunparse(
            (parsed.scheme, parsed.netloc, "/api/v1/devices/scan", "", "", "")
        )
    return suite.DEFAULT_UPS_SCAN_URL


def resolve_devd_devices_url(args: argparse.Namespace) -> str:
    scan_url = getattr(args, "devd_scan_url", None)
    if isinstance(scan_url, str) and scan_url.strip():
        parsed = suite.urllib.parse.urlparse(scan_url)
        if parsed.scheme and parsed.netloc:
            return suite.urllib.parse.urlunparse(
                (parsed.scheme, parsed.netloc, "/api/v1/devices", "", "", "")
            )
    return suite.default_mains_aegis_devd_base_url().rstrip("/") + "/api/v1/devices"


def resolve_observe_device_id(args: argparse.Namespace) -> str | None:
    return suite.observe_device_id_from_args(args)


def read_ups_cli_observation_surfaces(
    args: argparse.Namespace,
    *,
    dry_run: bool,
) -> dict[str, Any]:
    device_id = resolve_observe_device_id(args) or args.ups_device_id
    surfaces = {
        "status": ["status"],
        "power_diag": ["power-diag"],
    }
    result: dict[str, Any] = {
        "ok": True,
        "device_id": device_id,
        "surfaces": {},
        "failures": [],
    }
    for surface, command_parts in surfaces.items():
        cmd = suite.mains_aegis_base_cmd(args) + [
            "device",
            device_id,
            *command_parts,
            "--include-meta",
            "--cache-only",
        ]
        if dry_run:
            payload = {"dry_run": True, "cmd": cmd}
        else:
            try:
                payload = {"cmd": cmd, "result": suite.run_json(cmd)}
            except Exception as exc:  # noqa: BLE001
                payload = {
                    "cmd": cmd,
                    "error": repr(exc),
                }
                result["ok"] = False
                result["failures"].append(f"{surface}_cli_unavailable")
        if not dry_run and "result" in payload and not isinstance(payload["result"], dict):
            result["ok"] = False
            result["failures"].append(f"{surface}_cli_non_object")
        result["surfaces"][surface] = payload
    return result


def normalized_observe_urls(args: argparse.Namespace) -> dict[str, str]:
    return suite.normalized_observe_urls(args)


def direct_http_identity_settings(
    args: argparse.Namespace,
    *,
    usb_identity_payload: Any,
    dry_run: bool,
) -> tuple[dict[str, Any], dict[str, Any]]:
    direct_lan_base_url = suite.direct_lan_base_url_from_identity(usb_identity_payload)
    identity_url = (
        f"{direct_lan_base_url}/api/v1/identity"
        if direct_lan_base_url
        else suite.direct_lan_url_from_status_url(args.ups_status_url, "/api/v1/identity")
    )
    settings_url = (
        f"{direct_lan_base_url}/api/v1/settings"
        if direct_lan_base_url
        else suite.direct_lan_url_from_status_url(args.ups_status_url, "/api/v1/settings")
    )
    if dry_run:
        return (
            {"dry_run": True, "url": identity_url},
            {"dry_run": True, "url": settings_url},
        )
    identity_snapshot: dict[str, Any] = {"url": identity_url}
    settings_snapshot: dict[str, Any] = {"url": settings_url}
    try:
        identity_snapshot["result"] = suite.http_get_json(identity_url)
    except Exception as exc:  # noqa: BLE001
        identity_snapshot["error"] = repr(exc)
    try:
        settings_snapshot["result"] = suite.http_get_json(settings_url)
    except Exception as exc:  # noqa: BLE001
        settings_snapshot["error"] = repr(exc)
    return (
        identity_snapshot,
        settings_snapshot,
    )


def resolve_active_profile(*, usb_identity_payload: Any, usb_settings_payload: Any) -> str | None:
    identity_caps = suite.extract_identity_hardware_capabilities(usb_identity_payload)
    settings_caps = suite.extract_settings_hardware_capabilities(usb_settings_payload)
    identity_profile = identity_caps.get("output_profile")
    settings_profile = settings_caps.get("output_profile")
    if identity_profile == settings_profile and identity_profile in suite.PROFILES:
        return str(identity_profile)
    return None


def evaluate_readiness(
    *,
    load_status_payload: Any,
    ports_payload: Any,
    ups_cut_gate: dict[str, Any],
    ups_cli_observation_gate: dict[str, Any],
    source_reachability_gate: dict[str, Any],
    safe_prepare_failures: list[str] | None,
    active_profile: str | None,
    dual_surface_gate: dict[str, Any],
    manifest_by_profile: dict[str, str | None],
    http_identity_payload: dict[str, Any] | None = None,
    http_settings_payload: dict[str, Any] | None = None,
) -> dict[str, Any]:
    failures: list[str] = []
    load_control = load_status_payload.get("control") if isinstance(load_status_payload, dict) else {}
    load_output_enabled = load_control.get("output_enabled") if isinstance(load_control, dict) else None
    disable_acknowledged = load_status_payload.get("_disable_acknowledged") is True if isinstance(load_status_payload, dict) else False
    if load_output_enabled is not False and not disable_acknowledged:
        failures.append("load_output_not_disabled")
    port_c = suite.port_state(ports_payload if isinstance(ports_payload, dict) else {}, port_id="port_c")
    port_c_state = port_c.get("state") if isinstance(port_c.get("state"), dict) else {}
    if port_c_state.get("power_enabled") is True:
        failures.append("source_port_c_not_disabled")
    if ups_cut_gate.get("ok") is not True:
        failures.extend(list((ups_cut_gate.get("validation") or {}).get("failures") or []))
    if ups_cli_observation_gate.get("ok") is not True:
        failures.extend([f"ups_cli:{item}" for item in ups_cli_observation_gate.get("failures") or []])
    if source_reachability_gate.get("ok") is not True:
        failures.extend([f"source:{item}" for item in source_reachability_gate.get("failures") or []])
    for item in safe_prepare_failures or []:
        failures.append(f"safe_prepare:{item}")
    if active_profile not in suite.PROFILES:
        failures.append("active_profile_unknown")
    if dual_surface_gate.get("ok") is not True:
        failures.extend([f"capability:{item}" for item in dual_surface_gate.get("failures") or []])
    if isinstance(http_identity_payload, dict) and http_identity_payload.get("error"):
        failures.append("http_identity_unavailable")
    if isinstance(http_settings_payload, dict) and http_settings_payload.get("error"):
        failures.append("http_settings_unavailable")
    for profile_key in ("12v", "19v"):
        if not manifest_by_profile.get(profile_key):
            failures.append(f"manifest_missing:{profile_key}")
    return {
        "ready_for_operator_connect": not failures,
        "failures": failures,
        "safe_state": {
            "load_output_enabled": load_output_enabled,
            "port_c_power_enabled": port_c_state.get("power_enabled"),
            "port_c_status": (port_c.get("telemetry") or {}).get("status")
            if isinstance(port_c.get("telemetry"), dict)
            else None,
            "ups_input_cut_gate_ok": ups_cut_gate.get("ok") is True,
            "ups_cli_observation_gate_ok": ups_cli_observation_gate.get("ok") is True,
        },
        "active_profile": active_profile,
    }


def rate_gate_failures(
    name: str,
    payload: Any,
    *,
    require_fresh: bool = True,
) -> list[str]:
    data = payload if isinstance(payload, dict) else {}
    failures = list(data.get("failures") or [])
    effective_hz = data.get("effective_sample_rate_hz")
    max_gap_s = data.get("max_sample_gap_s")
    if not isinstance(effective_hz, (int, float)) or effective_hz < runner.FORMAL_MIN_EFFECTIVE_SAMPLE_RATE_HZ:
        failures.append(f"{name}_sample_rate_below_2hz")
    if not isinstance(max_gap_s, (int, float)) or max_gap_s > runner.FORMAL_MAX_SAMPLE_GAP_SECONDS:
        failures.append(f"{name}_sample_gap_above_0_5s")
    if require_fresh and data.get("fresh") is False:
        failures.append(f"{name}_stale_samples")
    return sorted(dict.fromkeys(failures))


def evaluate_telemetry_gate(
    *,
    ups_status_probe: Any,
    ups_power_diag_probe: Any,
    source_probe: Any,
    load_probe: Any,
) -> dict[str, Any]:
    failures: list[str] = []
    for name, payload in (
        ("ups_status", ups_status_probe),
        ("ups_power_diag", ups_power_diag_probe),
        ("source", source_probe),
        ("load", load_probe),
    ):
        failures.extend(rate_gate_failures(name, payload))
    failures = sorted(dict.fromkeys(failures))
    return {
        "ok": not failures,
        "failures": failures,
        "required": {
            "min_effective_sample_rate_hz": runner.FORMAL_MIN_EFFECTIVE_SAMPLE_RATE_HZ,
            "target_sample_rate_hz": runner.FORMAL_TARGET_SAMPLE_RATE_HZ,
            "max_sample_gap_s": runner.FORMAL_MAX_SAMPLE_GAP_SECONDS,
            "fresh_samples_required": True,
        },
        "probes": {
            "ups_status": ups_status_probe,
            "ups_power_diag": ups_power_diag_probe,
            "source": source_probe,
            "load": load_probe,
        },
    }


def append_query_params(url: str, params: dict[str, str]) -> str:
    parsed = urllib.parse.urlparse(url)
    query = dict(urllib.parse.parse_qsl(parsed.query, keep_blank_values=True))
    query.update(params)
    return urllib.parse.urlunparse(
        parsed._replace(query=urllib.parse.urlencode(query))
    )


def freshness_from_devd_payload(payload: Any, *, fetch_age_s: float) -> tuple[bool, float | None]:
    meta = runner.devd_read_meta(payload)
    cache_age_ms = meta.get("cache_age_ms")
    sample_age_s = (
        round(max(0.0, float(cache_age_ms) / 1000.0), 3)
        if isinstance(cache_age_ms, (int, float))
        else round(max(0.0, fetch_age_s), 3)
    )
    if meta.get("cache_fresh") is True:
        return sample_age_s <= runner.FORMAL_MAX_REALTIME_SAMPLE_AGE_SECONDS, sample_age_s
    if meta.get("cache_fresh") is False:
        return False, sample_age_s
    return fetch_age_s <= runner.FORMAL_MAX_REALTIME_SAMPLE_AGE_SECONDS, sample_age_s


def build_rate_probe(
    *,
    name: str,
    samples: list[dict[str, Any]],
    freshness_required: bool = True,
) -> dict[str, Any]:
    fresh_samples = [
        sample
        for sample in samples
        if sample.get("ok") is True
        and (
            not freshness_required
            or sample.get("fresh") is True
        )
    ]
    times = [
        float(sample["t_s"])
        for sample in fresh_samples
        if isinstance(sample.get("t_s"), (int, float))
    ]
    gaps = [
        round(curr - prev, 3)
        for prev, curr in zip(times, times[1:])
        if curr > prev
    ]
    max_gap_s = max(gaps, default=None)
    effective_hz = (
        round(1.0 / max_gap_s, 3)
        if isinstance(max_gap_s, (int, float)) and max_gap_s > 0
        else None
    )
    fresh = bool(fresh_samples) and len(fresh_samples) == len(samples)
    failures: list[str] = []
    if len(fresh_samples) < 2:
        failures.append(f"{name}_too_few_fresh_samples")
    if any(sample.get("ok") is not True for sample in samples):
        failures.append(f"{name}_read_errors")
    if freshness_required and any(sample.get("fresh") is False for sample in samples if sample.get("ok") is True):
        failures.append(f"{name}_stale_samples")
    return {
        "sample_count": len(samples),
        "fresh_sample_count": len(fresh_samples),
        "effective_sample_rate_hz": effective_hz,
        "max_sample_gap_s": max_gap_s,
        "fresh": fresh,
        "failures": failures,
        "samples": samples,
    }


def probe_http_json_rate(
    *,
    name: str,
    url: str,
    samples: int,
    interval_sec: float,
    timeout_sec: float,
    retries: int,
    devd_meta_required: bool,
    dry_run: bool,
) -> dict[str, Any]:
    if dry_run:
        return {
            "dry_run": True,
            "sample_count": samples,
            "effective_sample_rate_hz": runner.FORMAL_TARGET_SAMPLE_RATE_HZ,
            "max_sample_gap_s": round(interval_sec, 3),
            "fresh": True,
            "failures": [],
            "url": url,
        }
    started_at = time.monotonic()
    next_sample_at = started_at
    rows: list[dict[str, Any]] = []
    for idx in range(max(1, samples)):
        now = time.monotonic()
        if now < next_sample_at:
            time.sleep(next_sample_at - now)
        read_started = time.monotonic()
        try:
            payload = suite.http_request_json(
                url,
                method="GET",
                timeout_sec=timeout_sec,
                retries=retries,
            )
            read_finished = time.monotonic()
            fetch_age_s = read_finished - read_started
            if devd_meta_required:
                fresh, sample_age_s = freshness_from_devd_payload(
                    payload,
                    fetch_age_s=fetch_age_s,
                )
            else:
                sample_age_s = round(fetch_age_s, 3)
                fresh = fetch_age_s <= runner.FORMAL_MAX_REALTIME_SAMPLE_AGE_SECONDS
            rows.append(
                {
                    "index": idx,
                    "t_s": round(read_finished - started_at, 3),
                    "ok": True,
                    "fresh": fresh,
                    "sample_age_s": sample_age_s,
                    "fetch_age_s": round(fetch_age_s, 3),
                }
            )
        except Exception as exc:  # noqa: BLE001
            rows.append(
                {
                    "index": idx,
                    "t_s": round(time.monotonic() - started_at, 3),
                    "ok": False,
                    "fresh": False,
                    "error": repr(exc),
                }
            )
        next_sample_at += interval_sec
    result = build_rate_probe(name=name, samples=rows)
    result["url"] = url
    return result


def probe_load_rate(args: argparse.Namespace, *, dry_run: bool) -> dict[str, Any]:
    interval_sec = float(getattr(args, "telemetry_probe_interval_sec", 1.0 / runner.FORMAL_TARGET_SAMPLE_RATE_HZ))
    if dry_run:
        return {
            "dry_run": True,
            "effective_sample_rate_hz": runner.FORMAL_TARGET_SAMPLE_RATE_HZ,
            "max_sample_gap_s": round(interval_sec, 3),
            "fresh": True,
            "failures": [],
        }
    setattr(args, "load_status_source", getattr(args, "load_status_source", "status-stream"))
    setattr(args, "sample_interval_seconds", interval_sec)
    setattr(args, "load_stream_interval_seconds", float(getattr(args, "load_stream_interval_seconds", 0.2)))
    probe = runner.probe_live_load_status_poller_capability(
        args,
        runtime_sec=max(
            1.5,
            float(getattr(args, "telemetry_probe_samples", 12))
            * float(getattr(args, "telemetry_probe_interval_sec", 1.0 / runner.FORMAL_TARGET_SAMPLE_RATE_HZ)),
        ),
    )
    return {
        **probe,
        "fresh": bool(probe.get("formal_capable")),
    }


def run_telemetry_gate(args: argparse.Namespace, *, dry_run: bool) -> dict[str, Any]:
    samples = max(3, int(getattr(args, "telemetry_probe_samples", 12)))
    interval_sec = max(0.05, float(getattr(args, "telemetry_probe_interval_sec", 1.0 / runner.FORMAL_TARGET_SAMPLE_RATE_HZ)))
    timeout_sec = min(float(args.status_timeout_sec), runner.FORMAL_MAX_SAMPLE_GAP_SECONDS)
    freshness_ms = str(int(runner.FORMAL_MAX_REALTIME_SAMPLE_AGE_SECONDS * 1000))
    devd_cache_query = {
        "include_meta": "true",
        "watch_freshness_ms": freshness_ms,
        "cache_only": "true",
        "allow_stale_cache": "true",
    }
    ups_status_url = append_query_params(
        args.ups_status_url,
        devd_cache_query,
    )
    ups_power_diag_url = append_query_params(
        args.devd_power_diag_url,
        devd_cache_query,
    )
    source_ports_url = f"{args.isolapurr_url.rstrip('/')}/api/v1/ports"
    ups_status_probe = probe_http_json_rate(
        name="ups_status",
        url=ups_status_url,
        samples=samples,
        interval_sec=interval_sec,
        timeout_sec=timeout_sec,
        retries=1,
        devd_meta_required=True,
        dry_run=dry_run,
    )
    ups_power_diag_probe = probe_http_json_rate(
        name="ups_power_diag",
        url=ups_power_diag_url,
        samples=samples,
        interval_sec=interval_sec,
        timeout_sec=timeout_sec,
        retries=1,
        devd_meta_required=True,
        dry_run=dry_run,
    )
    source_probe = probe_http_json_rate(
        name="source",
        url=source_ports_url,
        samples=samples,
        interval_sec=interval_sec,
        timeout_sec=timeout_sec,
        retries=0,
        devd_meta_required=False,
        dry_run=dry_run,
    )
    load_probe = probe_load_rate(args, dry_run=dry_run)
    return evaluate_telemetry_gate(
        ups_status_probe=ups_status_probe,
        ups_power_diag_probe=ups_power_diag_probe,
        source_probe=source_probe,
        load_probe=load_probe,
    )


def evaluate_non_blocking_warnings(*, selected_artifact_payload: Any) -> list[str]:
    warnings: list[str] = []
    if not isinstance(selected_artifact_payload, dict):
        return warnings
    log_decode = selected_artifact_payload.get("log_decode")
    if not isinstance(log_decode, dict):
        return warnings
    if log_decode.get("status") == "unverified":
        warnings.append("selected_artifact_differs_from_current_device_firmware")
    return warnings


def write_summary(report_root: Path, payload: dict[str, Any]) -> Path:
    report_root.mkdir(parents=True, exist_ok=True)
    summary_path = report_root / f"{suite.suite_timestamp()}-formal-readiness.json"
    summary_path.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return summary_path


def main() -> int:
    args = parse_args()
    runner.normalize_load_transport_args(args)
    observe_urls = normalized_observe_urls(args)
    args.ups_status_url = observe_urls["ups_status_url"]
    args.ups_settings_url = observe_urls["ups_settings_url"]
    args.devd_power_diag_url = observe_urls["devd_power_diag_url"]
    actions: list[dict[str, Any]] = []
    safe_prepare_failures: list[str] = []
    devd_bootstrap_gate = runner.ensure_valid_mains_aegis_devd_http_base(
        args.devd_scan_url,
        timeout_sec=min(args.status_timeout_sec, 5.0),
    )
    actions.append({"devd_bootstrap_gate": devd_bootstrap_gate})
    if devd_bootstrap_gate.get("ok") is not True and not args.dry_run:
        summary = {
            "ready_for_operator_connect": False,
            "failures": [f"devd:{item}" for item in devd_bootstrap_gate.get("failures") or []],
            "warnings": [],
            "active_profile": None,
            "next_operator_action": "fix mains-aegis-devd HTTP base before any source or load wiring",
            "actions": actions,
        }
        summary_path = write_summary(Path(args.report_root).resolve(), summary)
        print(json.dumps({"summary_path": str(summary_path), **summary}, ensure_ascii=False, indent=2))
        return 1
    if not args.skip_safe_prepare:
        try:
            disable_load_payload = suite.disable_load(
                load_cli=args.load_cli,
                load_device=args.load_device,
                dry_run=args.dry_run,
            )
        except Exception as exc:  # noqa: BLE001
            disable_load_payload = {
                "ok": False,
                "error": repr(exc),
            }
            safe_prepare_failures.append("disable_load_failed")
        actions.append({"disable_load": disable_load_payload})

    source_reachability_gate = suite.probe_isolapurr_source_reachability(
        isolapurr_cli=args.isolapurr_cli,
        isolapurr_url=args.isolapurr_url,
        timeout_sec=min(args.status_timeout_sec, 5.0),
        dry_run=args.dry_run,
        expected_device_id=getattr(args, "isolapurr_device_id", suite.DEFAULT_ISOLAPURR_DEVICE_ID),
    )
    actions.append({"source_reachability_gate": source_reachability_gate})
    if not args.skip_safe_prepare:
        if source_reachability_gate.get("ok") is True:
            try:
                cut_source_payload = suite.cut_source_power_only(
                    isolapurr_url=args.isolapurr_url,
                    dry_run=args.dry_run,
                )
            except Exception as exc:  # noqa: BLE001
                cut_source_payload = {
                    "ok": False,
                    "error": repr(exc),
                }
                safe_prepare_failures.append("cut_source_failed")
        else:
            cut_source_payload = {
                "skipped": True,
                "reason": "source_reachability_gate_failed_before_cut_source",
            }
        actions.append({"cut_source": cut_source_payload})

    devd_scan_url = resolve_devd_scan_url(args)
    refresh_snapshot = (
        {"dry_run": True, "url": devd_scan_url}
        if args.dry_run
        else {"url": devd_scan_url, "result": suite.http_post_json(devd_scan_url)}
    )
    actions.append({"refresh_devd_devices_before_capability_gate": refresh_snapshot})
    devd_devices_url = resolve_devd_devices_url(args)
    if args.dry_run:
        devices_snapshot = {"dry_run": True, "url": devd_devices_url}
    else:
        devices_snapshot = {"url": devd_devices_url, "result": suite.http_get_json(devd_devices_url)}
    actions.append({"read_devd_devices_after_refresh": devices_snapshot})
    seeded_devd_device = suite.devd_device_entry_from_listing(
        devices_snapshot.get("result"),
        device_id=resolve_observe_device_id(args),
    )
    if source_reachability_gate.get("ok") is not True:
        connect_snapshot = {
            "skipped": True,
            "reason": "source_reachability_gate_failed_before_device_connect",
        }
        usb_identity_snapshot = {
            "skipped": True,
            "reason": "source_reachability_gate_failed_before_device_connect",
        }
        usb_settings_snapshot = {
            "skipped": True,
            "reason": "source_reachability_gate_failed_before_device_connect",
        }
    else:
        if suite.seeded_devd_device_is_capability_ready(seeded_devd_device):
            connect_snapshot = {
                "skipped": True,
                "reason": "already_connected_per_scan_snapshot_re_reading_usb_truth",
            }
        else:
            connect_snapshot = suite.connect_device(args, dry_run=args.dry_run)
        usb_identity_snapshot = suite.read_device_identity(args, dry_run=args.dry_run)
        usb_settings_snapshot = suite.read_device_settings(args, dry_run=args.dry_run)
    actions.append({"connect_device": connect_snapshot})
    actions.append({"usb_identity": usb_identity_snapshot})
    actions.append({"usb_settings": usb_settings_snapshot})

    usb_identity_result = usb_identity_snapshot.get("result")
    usb_settings_result = usb_settings_snapshot.get("result")
    direct_lan_base_url = suite.direct_lan_base_url_from_identity(usb_identity_result)
    input_cut_status_url = (
        f"{direct_lan_base_url}/api/v1/status"
        if direct_lan_base_url
        else args.ups_status_url
    )
    ups_cut_gate = suite.wait_for_ups_external_input_cut(
        status_url=input_cut_status_url,
        timeout_sec=min(args.status_timeout_sec, 10.0),
        dry_run=args.dry_run,
    )
    actions.append({"ups_input_cut_gate": ups_cut_gate})
    ups_cli_observation_gate = read_ups_cli_observation_surfaces(
        args,
        dry_run=args.dry_run,
    )
    actions.append({"ups_cli_observation_gate": ups_cli_observation_gate})
    http_identity_snapshot, http_settings_snapshot = direct_http_identity_settings(
        args,
        usb_identity_payload=usb_identity_result,
        dry_run=args.dry_run,
    )
    actions.append({"http_identity": http_identity_snapshot})
    actions.append({"http_settings": http_settings_snapshot})

    active_profile = resolve_active_profile(
        usb_identity_payload=usb_identity_result,
        usb_settings_payload=usb_settings_result,
    )
    dual_surface_gate = (
        suite.validate_dual_surface_hardware_capabilities(
            profile_key=active_profile,
            usb_identity_payload=usb_identity_result,
            usb_settings_payload=usb_settings_result,
            http_identity_payload=http_identity_snapshot.get("result"),
            http_settings_payload=http_settings_snapshot.get("result"),
        )
        if active_profile is not None
        else {
            "ok": False,
            "failures": ["active_profile_unknown"],
            "usb": {},
            "http": {},
            "expected": {},
        }
    )
    actions.append({"dual_surface_capability_gate": dual_surface_gate})

    selected_artifact = suite.read_selected_artifact(args, dry_run=args.dry_run)
    actions.append({"selected_artifact": selected_artifact})
    non_blocking_warnings = evaluate_non_blocking_warnings(
        selected_artifact_payload=selected_artifact.get("result")
    )

    manifest_by_profile = {
        "12v": suite.artifact_manifest_for_profile(args, "12v"),
        "19v": suite.artifact_manifest_for_profile(args, "19v"),
    }
    if args.dry_run:
        ports_payload = {"ports": [{"portId": "port_c", "telemetry": {"status": "not_inserted"}, "state": {"power_enabled": False}}]}
    elif source_reachability_gate.get("ok") is not True:
        ports_payload = {
            "ports": [],
            "skipped": True,
            "reason": "source_reachability_gate_failed",
        }
    else:
        ports_payload = suite.http_get_json(f"{args.isolapurr_url.rstrip('/')}/api/v1/ports")
    load_status = load_status_payload(
        args,
        load_cli=args.load_cli,
        load_device=args.load_device,
        dry_run=args.dry_run,
    )
    if (
        isinstance(load_status, dict)
        and not isinstance(load_status.get("control"), dict)
        and disable_load_output_acknowledged(actions)
    ):
        load_status = dict(load_status)
        load_status["_disable_acknowledged"] = True

    telemetry_gate = run_telemetry_gate(args, dry_run=args.dry_run)
    actions.append({"telemetry_gate": telemetry_gate})

    readiness = evaluate_readiness(
        load_status_payload=load_status,
        ports_payload=ports_payload,
        ups_cut_gate=ups_cut_gate,
        ups_cli_observation_gate=ups_cli_observation_gate,
        source_reachability_gate=source_reachability_gate,
        safe_prepare_failures=safe_prepare_failures,
        active_profile=active_profile,
        dual_surface_gate=dual_surface_gate,
        manifest_by_profile=manifest_by_profile,
        http_identity_payload=http_identity_snapshot,
        http_settings_payload=http_settings_snapshot,
    )
    failures = list(readiness["failures"])
    if telemetry_gate.get("ok") is not True:
        failures.extend([f"telemetry:{item}" for item in telemetry_gate.get("failures") or []])
    ready = not failures
    summary = {
        "success": ready,
        "ready_for_operator_connect": ready,
        "next_operator_action": (
            "connect source and load wiring; keep bench source disabled until the formal suite re-runs the per-scene capability gate"
            if ready
            else "do not connect source/load wiring yet; inspect failures"
        ),
        "active_profile": readiness["active_profile"],
        "failures": sorted(dict.fromkeys(failures)),
        "warnings": non_blocking_warnings,
        "safe_state": readiness["safe_state"],
        "telemetry_gate": telemetry_gate,
        "manifest_by_profile": manifest_by_profile,
        "selected_artifact": selected_artifact.get("result"),
        "usb_identity": usb_identity_result,
        "usb_settings": usb_settings_result,
        "http_identity": http_identity_snapshot.get("result"),
        "http_settings": http_settings_snapshot.get("result"),
        "ups_control_device_id": args.ups_device_id,
        "ups_observe_device_id": resolve_observe_device_id(args),
        "ups_input_cut_gate": ups_cut_gate,
        "ups_cli_observation_gate": ups_cli_observation_gate,
        "source_reachability_gate": source_reachability_gate,
        "dual_surface_capability_gate": dual_surface_gate,
        "load_status": load_status,
        "isolapurr_ports": ports_payload,
        "actions": actions,
    }
    summary_path = write_summary(Path(args.report_root).resolve(), summary)
    print(
        json.dumps(
            {
                "summary_path": str(summary_path),
                "ready_for_operator_connect": summary["ready_for_operator_connect"],
                "active_profile": summary["active_profile"],
                "failures": summary["failures"],
                "telemetry_gate": telemetry_gate,
                "warnings": summary["warnings"],
                "next_operator_action": summary["next_operator_action"],
            },
            ensure_ascii=False,
            indent=2,
        )
    )
    return 0 if ready else 1


if __name__ == "__main__":
    raise SystemExit(main())
