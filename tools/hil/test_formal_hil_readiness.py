#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from unittest import mock
from pathlib import Path


def load_module(filename: str, module_name: str):
    path = Path(__file__).with_name(filename)
    spec = importlib.util.spec_from_file_location(module_name, path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


class FormalHilReadinessTests(unittest.TestCase):
    def setUp(self) -> None:
        self.readiness = load_module("formal_hil_readiness.py", "formal_hil_readiness")

    def test_main_normalizes_load_transport_args_before_status_reads(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            report_root = Path(tmp)
            args = self.readiness.argparse.Namespace(
                report_root=str(report_root),
                load_device="loadlynx-d68638",
                load_cli="/tmp/loadlynx",
                load_bridge_url="",
                load_ipc="/tmp/loadlynx-formal-ipc.a31f.sock",
                load_devd_base_url="",
                load_devd_socket=self.readiness.suite.DEFAULT_LOAD_DEVD_SOCKET,
                load_usb_device_id="digital-2bdfc170893f",
                isolapurr_cli="isolapurr",
                isolapurr_url="http://192.168.31.122",
                isolapurr_device_id="856a141cdbd4",
                mains_aegis_cli="/tmp/mains-aegis",
                mains_aegis_ipc=None,
                ups_device_id="serial-04f3bb3f5367",
                ups_status_url="http://127.0.0.1:38140/api/v1/devices/serial-04f3bb3f5367/status",
                ups_settings_url="http://127.0.0.1:38140/api/v1/devices/serial-04f3bb3f5367/settings",
                devd_scan_url="http://127.0.0.1:38140/api/v1/devices/scan",
                devd_diag_snapshot_url="http://127.0.0.1:38140/api/v1/devices/serial-04f3bb3f5367/diag-snapshot",
                devd_device_trace_url="http://127.0.0.1:38140/api/v1/devices/serial-04f3bb3f5367/trace?trace_limit=1",
                artifact_manifest_12v=None,
                artifact_manifest_19v=None,
                firmware_bundle_root=str(report_root),
                status_timeout_sec=5.0,
                skip_safe_prepare=True,
                dry_run=True,
            )
            with (
                mock.patch.object(self.readiness, "parse_args", return_value=args),
                mock.patch.object(self.readiness.runner, "normalize_load_transport_args", wraps=self.readiness.runner.normalize_load_transport_args) as normalize_mock,
                mock.patch.object(self.readiness.runner, "ensure_valid_mains_aegis_devd_http_base", return_value={"ok": True, "failures": []}),
                mock.patch.object(self.readiness.suite, "probe_isolapurr_source_reachability", return_value={"ok": True, "failures": []}),
                mock.patch.object(self.readiness.suite, "wait_for_ups_external_input_cut", return_value={"ok": True, "validation": {"failures": []}}) as cut_gate,
                mock.patch.object(self.readiness.suite, "http_post_json", return_value={"devices": []}),
                mock.patch.object(self.readiness.suite, "refresh_control_devices", return_value={"result": {"devices": []}}),
                mock.patch.object(self.readiness.suite, "devd_device_entry_from_scan", return_value={}),
                mock.patch.object(self.readiness.suite, "connect_device_with_retry", return_value={"dry_run": True}),
                mock.patch.object(self.readiness.suite, "read_device_identity", return_value={"result": {"network": {"ipv4": "192.168.31.232"}, "hardware_capabilities": {"output_profile": "12v", "rated_vout_mv": 12000}}}),
                mock.patch.object(self.readiness.suite, "read_device_settings", return_value={"result": {"advanced_power_capabilities": {"rated_vout_mv": 12000}}}),
                mock.patch.object(self.readiness, "direct_http_identity_settings", return_value=(
                    {"result": {"hardware_capabilities": {"output_profile": "12v", "rated_vout_mv": 12000}}},
                    {"result": {"advanced_power_capabilities": {"rated_vout_mv": 12000}}},
                )),
                mock.patch.object(self.readiness.suite, "validate_dual_surface_hardware_capabilities", return_value={"ok": True, "failures": []}),
                mock.patch.object(self.readiness, "load_status_payload", return_value={"control": {"output_enabled": False}}),
                mock.patch.object(self.readiness, "run_telemetry_gate", return_value={"ok": True, "failures": []}),
                mock.patch.object(self.readiness.suite, "http_get_json", return_value={"ports": [{"portId": "port_c", "state": {"power_enabled": False}, "telemetry": {"status": "not_inserted"}}]}),
                mock.patch.object(self.readiness.suite, "read_selected_artifact", return_value={"result": {"log_decode": {"status": "verified"}}}),
                mock.patch.object(self.readiness.suite, "artifact_manifest_for_profile", side_effect=lambda _args, profile: f"/tmp/{profile}.manifest.json"),
            ):
                rc = self.readiness.main()
            self.assertEqual(rc, 0)
            normalize_mock.assert_called_once()
            self.assertEqual(args.load_devd_socket, "")
            self.assertEqual(
                cut_gate.call_args.kwargs["status_url"],
                "http://192.168.31.232/api/v1/status",
            )

    def test_main_fails_when_telemetry_gate_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            report_root = Path(tmp)
            args = self.readiness.argparse.Namespace(
                report_root=str(report_root),
                load_device="loadlynx-d68638",
                load_cli="/tmp/loadlynx",
                load_bridge_url="",
                load_ipc="/tmp/loadlynx-formal-ipc.a31f.sock",
                load_devd_base_url="",
                load_devd_socket=self.readiness.suite.DEFAULT_LOAD_DEVD_SOCKET,
                load_usb_device_id="digital-2bdfc170893f",
                isolapurr_cli="isolapurr",
                isolapurr_url="http://192.168.31.122",
                isolapurr_device_id="856a141cdbd4",
                mains_aegis_cli="/tmp/mains-aegis",
                mains_aegis_ipc=None,
                ups_device_id="serial-04f3bb3f5367",
                ups_status_url="http://127.0.0.1:38140/api/v1/devices/serial-04f3bb3f5367/status",
                ups_settings_url="http://127.0.0.1:38140/api/v1/devices/serial-04f3bb3f5367/settings",
                devd_scan_url="http://127.0.0.1:38140/api/v1/devices/scan",
                devd_diag_snapshot_url="http://127.0.0.1:38140/api/v1/devices/serial-04f3bb3f5367/diag-snapshot",
                devd_device_trace_url="http://127.0.0.1:38140/api/v1/devices/serial-04f3bb3f5367/trace?trace_limit=1",
                artifact_manifest_12v="/tmp/12v.manifest.json",
                artifact_manifest_19v="/tmp/19v.manifest.json",
                firmware_bundle_root=str(report_root),
                telemetry_probe_samples=3,
                telemetry_probe_interval_sec=0.333,
                sample_interval_seconds=0.333,
                load_stream_interval_seconds=0.2,
                command_timeout_sec=5.0,
                status_timeout_sec=5.0,
                skip_safe_prepare=True,
                dry_run=True,
            )
            with (
                mock.patch.object(self.readiness, "parse_args", return_value=args),
                mock.patch.object(self.readiness.runner, "normalize_load_transport_args", wraps=self.readiness.runner.normalize_load_transport_args),
                mock.patch.object(self.readiness.runner, "ensure_valid_mains_aegis_devd_http_base", return_value={"ok": True, "failures": []}),
                mock.patch.object(self.readiness.suite, "probe_isolapurr_source_reachability", return_value={"ok": True, "failures": []}),
                mock.patch.object(self.readiness.suite, "wait_for_ups_external_input_cut", return_value={"ok": True, "validation": {"failures": []}}),
                mock.patch.object(self.readiness.suite, "http_post_json", return_value={"devices": []}),
                mock.patch.object(self.readiness.suite, "devd_device_entry_from_listing", return_value={}),
                mock.patch.object(self.readiness.suite, "connect_device", return_value={"dry_run": True}),
                mock.patch.object(self.readiness.suite, "read_device_identity", return_value={"result": {"network": {"ipv4": "192.168.31.232"}, "hardware_capabilities": {"output_profile": "12v", "rated_vout_mv": 12000}}}),
                mock.patch.object(self.readiness.suite, "read_device_settings", return_value={"result": {"advanced_power_capabilities": {"rated_vout_mv": 12000}}}),
                mock.patch.object(self.readiness, "direct_http_identity_settings", return_value=(
                    {"result": {"hardware_capabilities": {"output_profile": "12v", "rated_vout_mv": 12000}}},
                    {"result": {"advanced_power_capabilities": {"rated_vout_mv": 12000}}},
                )),
                mock.patch.object(self.readiness.suite, "validate_dual_surface_hardware_capabilities", return_value={"ok": True, "failures": []}),
                mock.patch.object(self.readiness, "load_status_payload", return_value={"control": {"output_enabled": False}}),
                mock.patch.object(self.readiness.suite, "http_get_json", return_value={"ports": [{"portId": "port_c", "state": {"power_enabled": False}, "telemetry": {"status": "not_inserted"}}]}),
                mock.patch.object(self.readiness.suite, "read_selected_artifact", return_value={"result": {"log_decode": {"status": "verified"}}}),
                mock.patch.object(self.readiness, "run_telemetry_gate", return_value={"ok": False, "failures": ["ups_status_sample_rate_below_2hz"]}),
            ):
                rc = self.readiness.main()

            self.assertEqual(rc, 1)
            summaries = list(report_root.glob("*-formal-readiness.json"))
            self.assertEqual(len(summaries), 1)
            payload = json.loads(summaries[0].read_text(encoding="utf-8"))
            self.assertFalse(payload["ready_for_operator_connect"])
            self.assertIn("telemetry:ups_status_sample_rate_below_2hz", payload["failures"])

    def test_resolve_active_profile_prefers_matching_usb_identity_and_settings(self) -> None:
        profile = self.readiness.resolve_active_profile(
            usb_identity_payload={
                "hardware_capabilities": {
                    "output_profile": "19v",
                    "rated_vout_mv": 19000,
                }
            },
            usb_settings_payload={
                "advanced_power_capabilities": {
                    "rated_vout_mv": 19000,
                }
            },
        )
        self.assertEqual(profile, "19v")

    def test_resolve_active_profile_returns_none_for_mismatch(self) -> None:
        profile = self.readiness.resolve_active_profile(
            usb_identity_payload={
                "hardware_capabilities": {
                    "output_profile": "12v",
                    "rated_vout_mv": 12000,
                }
            },
            usb_settings_payload={
                "advanced_power_capabilities": {
                    "rated_vout_mv": 19000,
                }
            },
        )
        self.assertIsNone(profile)

    def test_evaluate_readiness_accepts_safe_waiting_state(self) -> None:
        result = self.readiness.evaluate_readiness(
            load_status_payload={
                "control": {
                    "output_enabled": False,
                    "mode": "cc",
                }
            },
            ports_payload={
                "ports": [
                    {
                        "portId": "port_c",
                        "telemetry": {"status": "not_inserted"},
                        "state": {"power_enabled": False},
                    }
                ]
            },
            ups_cut_gate={
                "ok": True,
                "validation": {
                    "ok": True,
                    "failures": [],
                },
            },
            ups_cli_observation_gate={
                "ok": True,
                "failures": [],
            },
            source_reachability_gate={
                "ok": True,
                "failures": [],
            },
            safe_prepare_failures=[],
            active_profile="19v",
            dual_surface_gate={
                "ok": True,
                "failures": [],
            },
            manifest_by_profile={
                "12v": "/tmp/12v.manifest.json",
                "19v": "/tmp/19v.manifest.json",
            },
        )
        self.assertTrue(result["ready_for_operator_connect"])
        self.assertEqual(result["failures"], [])

    def test_evaluate_readiness_rejects_missing_safe_conditions(self) -> None:
        result = self.readiness.evaluate_readiness(
            load_status_payload={
                "control": {
                    "output_enabled": True,
                    "mode": "cc",
                }
            },
            ports_payload={
                "ports": [
                    {
                        "portId": "port_c",
                        "telemetry": {"status": "ok"},
                        "state": {"power_enabled": True},
                    }
                ]
            },
            ups_cut_gate={
                "ok": False,
                "validation": {
                    "ok": False,
                    "failures": ["ups_vin_vbus_not_cut"],
                },
            },
            ups_cli_observation_gate={
                "ok": False,
                "failures": ["status_cli_unavailable"],
            },
            source_reachability_gate={
                "ok": False,
                "failures": ["http_ports_unreachable"],
            },
            safe_prepare_failures=["cut_source_failed"],
            active_profile=None,
            dual_surface_gate={
                "ok": False,
                "failures": ["active_profile_unknown"],
            },
            manifest_by_profile={
                "12v": None,
                "19v": "/tmp/19v.manifest.json",
            },
        )
        self.assertFalse(result["ready_for_operator_connect"])
        self.assertIn("load_output_not_disabled", result["failures"])
        self.assertIn("source_port_c_not_disabled", result["failures"])
        self.assertIn("ups_vin_vbus_not_cut", result["failures"])
        self.assertIn("ups_cli:status_cli_unavailable", result["failures"])
        self.assertIn("source:http_ports_unreachable", result["failures"])
        self.assertIn("safe_prepare:cut_source_failed", result["failures"])
        self.assertIn("active_profile_unknown", result["failures"])
        self.assertIn("capability:active_profile_unknown", result["failures"])
        self.assertIn("manifest_missing:12v", result["failures"])

    def test_evaluate_readiness_rejects_unavailable_http_capability_surface(self) -> None:
        result = self.readiness.evaluate_readiness(
            load_status_payload={"control": {"output_enabled": False}},
            ports_payload={
                "ports": [
                    {
                        "portId": "port_c",
                        "telemetry": {"status": "not_inserted"},
                        "state": {"power_enabled": False},
                    }
                ]
            },
            ups_cut_gate={"ok": True, "validation": {"failures": []}},
            ups_cli_observation_gate={"ok": True, "failures": []},
            source_reachability_gate={"ok": True, "failures": []},
            safe_prepare_failures=[],
            active_profile="12v",
            dual_surface_gate={"ok": True, "failures": []},
            manifest_by_profile={
                "12v": "/tmp/12v.manifest.json",
                "19v": "/tmp/19v.manifest.json",
            },
            http_identity_payload={"url": "http://device/api/v1/identity", "error": "HTTP 400"},
            http_settings_payload={"url": "http://device/api/v1/settings", "error": "HTTP 400"},
        )

        self.assertFalse(result["ready_for_operator_connect"])
        self.assertIn("http_identity_unavailable", result["failures"])
        self.assertIn("http_settings_unavailable", result["failures"])

    def test_evaluate_telemetry_gate_requires_all_devices_above_2hz(self) -> None:
        result = self.readiness.evaluate_telemetry_gate(
            ups_status_probe={
                "effective_sample_rate_hz": 3.0,
                "max_sample_gap_s": 0.333,
                "fresh": True,
            },
            ups_diag_snapshot_probe={
                "effective_sample_rate_hz": 1.2,
                "max_sample_gap_s": 0.8,
                "fresh": True,
            },
            source_probe={
                "effective_sample_rate_hz": 2.5,
                "max_sample_gap_s": 0.4,
                "fresh": True,
            },
            load_probe={
                "effective_sample_rate_hz": 3.0,
                "max_sample_gap_s": 0.333,
                "fresh": False,
            },
        )

        self.assertFalse(result["ok"])
        self.assertIn("ups_diag_snapshot_sample_rate_below_2hz", result["failures"])
        self.assertIn("ups_diag_snapshot_sample_gap_above_0_5s", result["failures"])
        self.assertIn("load_stale_samples", result["failures"])

    def test_evaluate_telemetry_gate_accepts_all_fresh_formal_rates(self) -> None:
        fresh_probe = {
            "effective_sample_rate_hz": 3.0,
            "max_sample_gap_s": 0.333,
            "fresh": True,
        }
        result = self.readiness.evaluate_telemetry_gate(
            ups_status_probe=fresh_probe,
            ups_diag_snapshot_probe=fresh_probe,
            source_probe=fresh_probe,
            load_probe=fresh_probe,
        )

        self.assertTrue(result["ok"])
        self.assertEqual(result["failures"], [])

    def test_run_telemetry_gate_uses_transient_retry_only_for_ups_probes(self) -> None:
        args = self.readiness.argparse.Namespace(
            ups_status_url="http://127.0.0.1:41490/api/v1/devices/serial-04f3bb3f5367/status",
            devd_diag_snapshot_url="http://127.0.0.1:41490/api/v1/devices/serial-04f3bb3f5367/diag-snapshot",
            isolapurr_url="http://192.168.31.122",
            status_timeout_sec=5.0,
            telemetry_probe_samples=3,
            telemetry_probe_interval_sec=0.5,
            dry_run=False,
        )
        calls: list[dict[str, object]] = []

        def fake_probe_http_json_rate(**kwargs):
            calls.append(kwargs)
            return {
                "effective_sample_rate_hz": 3.0,
                "max_sample_gap_s": 0.333,
                "fresh": True,
                "failures": [],
            }

        with (
            mock.patch.object(self.readiness, "probe_http_json_rate", side_effect=fake_probe_http_json_rate),
            mock.patch.object(self.readiness, "probe_load_rate", return_value={
                "effective_sample_rate_hz": 3.0,
                "max_sample_gap_s": 0.333,
                "fresh": True,
                "failures": [],
            }),
        ):
            result = self.readiness.run_telemetry_gate(args, dry_run=False)

        self.assertTrue(result["ok"])
        self.assertEqual(
            [call["retries"] for call in calls],
            [1, 1, 0],
        )
        self.assertEqual([call["name"] for call in calls], ["ups_status", "ups_diag_snapshot", "source"])

    def test_read_ups_cli_observation_surfaces_uses_status_and_diag_snapshot_cli(self) -> None:
        args = self.readiness.argparse.Namespace(
            mains_aegis_cli="/tmp/mains-aegis",
            mains_aegis_ipc="/tmp/mains-aegis.sock",
            ups_device_id="serial-04f3bb3f5367",
            ups_observe_device_id=None,
            devd_diag_snapshot_url="http://127.0.0.1:38140/api/v1/devices/serial-04f3bb3f5367/diag-snapshot",
            ups_status_url="http://127.0.0.1:38140/api/v1/devices/serial-04f3bb3f5367/status",
            ups_settings_url="http://127.0.0.1:38140/api/v1/devices/serial-04f3bb3f5367/settings",
        )
        calls: list[list[str]] = []

        def fake_run_json(cmd: list[str]):
            calls.append(cmd)
            return {"meta": {"cache_fresh": True}}

        with mock.patch.object(self.readiness.suite, "run_json", side_effect=fake_run_json):
            result = self.readiness.read_ups_cli_observation_surfaces(args, dry_run=False)

        self.assertTrue(result["ok"])
        self.assertEqual(
            calls,
            [
                [
                    "/tmp/mains-aegis",
                    "--ipc",
                    "/tmp/mains-aegis.sock",
                    "device",
                    "serial-04f3bb3f5367",
                    "status",
                    "--include-meta",
                    "--cache-only",
                ],
                [
                    "/tmp/mains-aegis",
                    "--ipc",
                    "/tmp/mains-aegis.sock",
                    "device",
                    "serial-04f3bb3f5367",
                    "diag-snapshot",
                    "--include-meta",
                    "--cache-only",
                ],
            ],
        )

    def test_read_ups_cli_observation_surfaces_fails_on_missing_diag_snapshot_cli(self) -> None:
        args = self.readiness.argparse.Namespace(
            mains_aegis_cli="/tmp/mains-aegis",
            mains_aegis_ipc=None,
            ups_device_id="serial-04f3bb3f5367",
            ups_observe_device_id=None,
            devd_diag_snapshot_url="http://127.0.0.1:38140/api/v1/devices/serial-04f3bb3f5367/diag-snapshot",
            ups_status_url="http://127.0.0.1:38140/api/v1/devices/serial-04f3bb3f5367/status",
            ups_settings_url="http://127.0.0.1:38140/api/v1/devices/serial-04f3bb3f5367/settings",
        )

        def fake_run_json(cmd: list[str]):
            if "diag-snapshot" in cmd:
                raise RuntimeError("missing subcommand")
            return {"meta": {"cache_fresh": True}}

        with mock.patch.object(self.readiness.suite, "run_json", side_effect=fake_run_json):
            result = self.readiness.read_ups_cli_observation_surfaces(args, dry_run=False)

        self.assertFalse(result["ok"])
        self.assertIn("diag_snapshot_cli_unavailable", result["failures"])

    def test_write_summary_persists_json_payload(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            summary_path = self.readiness.write_summary(
                root,
                {
                    "ready_for_operator_connect": True,
                    "failures": [],
                },
            )
            self.assertTrue(summary_path.is_file())
            payload = json.loads(summary_path.read_text(encoding="utf-8"))
            self.assertTrue(payload["ready_for_operator_connect"])

    def test_evaluate_non_blocking_warnings_marks_unverified_selected_artifact(self) -> None:
        warnings = self.readiness.evaluate_non_blocking_warnings(
            selected_artifact_payload={
                "log_decode": {
                    "status": "unverified",
                    "reason": "device firmware identity does not match selected artifact",
                }
            }
        )
        self.assertEqual(
            warnings,
            ["selected_artifact_differs_from_current_device_firmware"],
        )

    def test_resolve_devd_scan_url_prefers_explicit_arg(self) -> None:
        args = self.readiness.argparse.Namespace(
            devd_scan_url="http://127.0.0.1:51170/api/v1/devices/scan",
            ups_status_url="http://192.168.31.232/api/v1/status",
            ups_settings_url="http://192.168.31.232/api/v1/settings",
        )
        self.assertEqual(
            self.readiness.resolve_devd_scan_url(args),
            "http://127.0.0.1:51170/api/v1/devices/scan",
        )

    def test_resolve_devd_devices_url_uses_same_origin_as_status_url(self) -> None:
        args = self.readiness.argparse.Namespace(
            devd_scan_url="http://127.0.0.1:51170/api/v1/devices/scan",
            ups_status_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/status",
            ups_settings_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/settings",
        )
        self.assertEqual(
            self.readiness.resolve_devd_devices_url(args),
            "http://127.0.0.1:51170/api/v1/devices",
        )

    def test_resolve_devd_devices_url_falls_back_to_default_when_scan_missing(self) -> None:
        args = self.readiness.argparse.Namespace(
            devd_scan_url="",
            ups_status_url="http://192.168.31.232/api/v1/status",
            ups_settings_url="http://192.168.31.232/api/v1/settings",
        )
        self.assertEqual(
            self.readiness.resolve_devd_devices_url(args),
            self.readiness.suite.default_mains_aegis_devd_base_url().rstrip("/") + "/api/v1/devices",
        )

    def test_resolve_observe_device_id_prefers_observe_url_device_id(self) -> None:
        args = self.readiness.argparse.Namespace(
            ups_observe_device_id=None,
            devd_diag_snapshot_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/diag-snapshot",
            ups_status_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/status",
            ups_settings_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/settings",
            ups_device_id="serial-04f3bb3f5367",
        )
        self.assertEqual(
            self.readiness.resolve_observe_device_id(args),
            "mains-aegis-198840",
        )

    def test_normalized_observe_urls_rewrite_to_control_device_id(self) -> None:
        args = self.readiness.argparse.Namespace(
            ups_device_id="serial-04f3bb3f5367",
            ups_status_url="http://127.0.0.1:30080/api/v1/devices/mains-aegis-198840/status",
            ups_settings_url="http://127.0.0.1:30080/api/v1/devices/mains-aegis-198840/settings",
            devd_diag_snapshot_url="http://127.0.0.1:30080/api/v1/devices/mains-aegis-198840/diag-snapshot",
            devd_device_trace_url="http://127.0.0.1:30080/api/v1/devices/mains-aegis-198840/trace?trace_limit=1",
            devd_scan_url="http://127.0.0.1:38140/api/v1/devices/scan",
        )
        normalized = self.readiness.normalized_observe_urls(args)
        self.assertEqual(
            normalized["ups_status_url"],
            "http://127.0.0.1:38140/api/v1/devices/serial-04f3bb3f5367/status",
        )
        self.assertEqual(
            normalized["ups_settings_url"],
            "http://127.0.0.1:38140/api/v1/devices/serial-04f3bb3f5367/settings",
        )
        self.assertEqual(
            normalized["devd_diag_snapshot_url"],
            "http://127.0.0.1:38140/api/v1/devices/serial-04f3bb3f5367/diag-snapshot",
        )

    def test_main_records_source_reachability_gate(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            args = self.readiness.argparse.Namespace(
                report_root=tmp,
                load_device="loadlynx-d68638",
                load_cli="/Users/ivan/.local/bin/loadlynx",
                load_bridge_url="",
                load_ipc="",
                load_devd_base_url="",
                load_devd_socket="/tmp/loadlynx.sock",
                load_usb_device_id="digital-2bdfc170893f",
                isolapurr_cli="isolapurr",
                isolapurr_url="http://192.168.31.122",
                isolapurr_device_id="856a141cdbd4",
                mains_aegis_cli="mains-aegis",
                mains_aegis_ipc=None,
                ups_device_id="serial-04f3bb3f5367",
                ups_status_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/status",
                ups_settings_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/settings",
                devd_scan_url="http://127.0.0.1:38140/api/v1/devices/scan",
                artifact_manifest_12v="/tmp/12v.manifest.json",
                artifact_manifest_19v="/tmp/19v.manifest.json",
                firmware_bundle_root="/tmp",
                status_timeout_sec=20.0,
                skip_safe_prepare=True,
                dry_run=True,
                devd_diag_snapshot_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/diag-snapshot",
                devd_device_trace_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/trace?trace_limit=1",
                ups_observe_device_id=None,
            )
            with (
                mock.patch.object(self.readiness, "parse_args", return_value=args),
                mock.patch.object(
                    self.readiness.runner,
                    "ensure_valid_mains_aegis_devd_http_base",
                    return_value={"ok": True, "failures": []},
                ),
                mock.patch.object(
                    self.readiness.suite,
                    "probe_isolapurr_source_reachability",
                    return_value={"ok": True, "failures": [], "expected_device_id": "856a141cdbd4"},
                ),
                mock.patch.object(
                    self.readiness.suite,
                    "wait_for_ups_external_input_cut",
                    return_value={"ok": True, "validation": {"ok": True, "failures": []}},
                ),
                mock.patch.object(self.readiness.suite, "http_post_json", return_value={"devices": []}),
                mock.patch.object(
                    self.readiness.suite,
                    "http_get_json",
                    side_effect=[
                        {"devices": [{"id": "mains-aegis-198840", "connection": "connected", "identity": {}, "settings": {}}]},
                    ],
                ),
                mock.patch.object(self.readiness.suite, "devd_device_entry_from_listing", return_value={"id": "mains-aegis-198840", "connection": "connected", "identity": {}, "settings": {}}),
                mock.patch.object(self.readiness.suite, "connect_device", return_value={"ok": True}),
                mock.patch.object(
                    self.readiness.suite,
                    "read_device_identity",
                    return_value={"result": {"hardware_capabilities": {"output_profile": "12v", "rated_vout_mv": 12000}}},
                ),
                mock.patch.object(
                    self.readiness.suite,
                    "read_device_settings",
                    return_value={"result": {"advanced_power_capabilities": {"rated_vout_mv": 12000}}},
                ),
                mock.patch.object(
                    self.readiness,
                    "direct_http_identity_settings",
                    return_value=(
                        {"result": {"hardware_capabilities": {"output_profile": "12v", "rated_vout_mv": 12000}}},
                        {"result": {"advanced_power_capabilities": {"rated_vout_mv": 12000}}},
                    ),
                ),
                mock.patch.object(
                    self.readiness.suite,
                    "validate_dual_surface_hardware_capabilities",
                    return_value={"ok": True, "failures": []},
                ),
                mock.patch.object(self.readiness.suite, "read_selected_artifact", return_value={"result": {}}),
                mock.patch.object(
                    self.readiness.suite,
                    "artifact_manifest_for_profile",
                    side_effect=lambda _args, profile: f"/tmp/{profile}.manifest.json",
                ),
                mock.patch.object(
                    self.readiness,
                    "load_status_payload",
                    return_value={"control": {"output_enabled": False, "mode": "cc"}},
                ),
                mock.patch.object(
                    self.readiness,
                    "read_ups_cli_observation_surfaces",
                    return_value={"ok": True, "failures": []},
                ),
                mock.patch.object(self.readiness, "run_telemetry_gate", return_value={"ok": True, "failures": []}),
            ):
                rc = self.readiness.main()
            self.assertEqual(rc, 0)

    def test_main_re_reads_usb_truth_even_when_scan_has_connected_caps(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            args = self.readiness.argparse.Namespace(
                report_root=tmp,
                load_device="loadlynx-d68638",
                load_cli="/Users/ivan/.local/bin/loadlynx",
                load_bridge_url="",
                load_ipc="",
                load_devd_base_url="",
                load_devd_socket="/tmp/loadlynx.sock",
                load_usb_device_id="digital-2bdfc170893f",
                isolapurr_cli="isolapurr",
                isolapurr_url="http://192.168.31.122",
                isolapurr_device_id="856a141cdbd4",
                mains_aegis_cli="mains-aegis",
                mains_aegis_ipc=None,
                ups_device_id="serial-04f3bb3f5367",
                ups_status_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/status",
                ups_settings_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/settings",
                devd_scan_url="http://127.0.0.1:38140/api/v1/devices/scan",
                artifact_manifest_12v="/tmp/12v.manifest.json",
                artifact_manifest_19v="/tmp/19v.manifest.json",
                firmware_bundle_root="/tmp",
                status_timeout_sec=20.0,
                skip_safe_prepare=True,
                dry_run=False,
                devd_diag_snapshot_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/diag-snapshot",
                devd_device_trace_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/trace?trace_limit=1",
                ups_observe_device_id=None,
            )
            seeded_device = {
                "id": "mains-aegis-198840",
                "connection": "connected",
                "identity": {
                    "hardware_capabilities": {
                        "output_profile": "12v",
                        "rated_vout_mv": 12000,
                    }
                },
                "settings": {
                    "advanced_power_capabilities": {
                        "rated_vout_mv": 12000,
                    }
                },
            }
            with (
                mock.patch.object(self.readiness, "parse_args", return_value=args),
                mock.patch.object(
                    self.readiness.runner,
                    "ensure_valid_mains_aegis_devd_http_base",
                    return_value={"ok": True, "failures": []},
                ),
                mock.patch.object(
                    self.readiness.suite,
                    "probe_isolapurr_source_reachability",
                    return_value={"ok": True, "failures": [], "expected_device_id": "856a141cdbd4"},
                ),
                mock.patch.object(
                    self.readiness.suite,
                    "wait_for_ups_external_input_cut",
                    return_value={"ok": True, "validation": {"ok": True, "failures": []}},
                ),
                mock.patch.object(self.readiness.suite, "http_post_json", return_value={"devices": []}),
                mock.patch.object(
                    self.readiness.suite,
                    "http_get_json",
                    side_effect=[
                        {"devices": [seeded_device]},
                        {"ports": [{"portId": "port_c", "telemetry": {"status": "not_inserted"}, "state": {"power_enabled": False}}]},
                    ],
                ),
                mock.patch.object(self.readiness.suite, "devd_device_entry_from_listing", return_value=seeded_device),
                mock.patch.object(self.readiness.suite, "connect_device", return_value={"ok": True}) as connect_mock,
                mock.patch.object(
                    self.readiness.suite,
                    "read_device_identity",
                    return_value={"result": {"hardware_capabilities": {"output_profile": "12v", "rated_vout_mv": 12000}}},
                ) as read_identity_mock,
                mock.patch.object(
                    self.readiness.suite,
                    "read_device_settings",
                    return_value={"result": {"advanced_power_capabilities": {"rated_vout_mv": 12000}}},
                ) as read_settings_mock,
                mock.patch.object(
                    self.readiness,
                    "direct_http_identity_settings",
                    return_value=(
                        {"result": {"hardware_capabilities": {"output_profile": "12v", "rated_vout_mv": 12000}}},
                        {"result": {"advanced_power_capabilities": {"rated_vout_mv": 12000}}},
                    ),
                ),
                mock.patch.object(
                    self.readiness.suite,
                    "validate_dual_surface_hardware_capabilities",
                    return_value={"ok": True, "failures": []},
                ),
                mock.patch.object(self.readiness.suite, "read_selected_artifact", return_value={"result": {}}),
                mock.patch.object(
                    self.readiness.suite,
                    "artifact_manifest_for_profile",
                    side_effect=lambda _args, profile: f"/tmp/{profile}.manifest.json",
                ),
                mock.patch.object(
                    self.readiness,
                    "load_status_payload",
                    return_value={"control": {"output_enabled": False, "mode": "cc"}},
                ),
                mock.patch.object(
                    self.readiness,
                    "read_ups_cli_observation_surfaces",
                    return_value={"ok": True, "failures": []},
                ),
                mock.patch.object(self.readiness, "run_telemetry_gate", return_value={"ok": True, "failures": []}),
            ):
                rc = self.readiness.main()
            self.assertEqual(rc, 0)
            connect_mock.assert_not_called()
            read_identity_mock.assert_called_once()
            read_settings_mock.assert_called_once()

    def test_main_skips_ports_fetch_when_source_reachability_gate_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            args = self.readiness.argparse.Namespace(
                report_root=tmp,
                load_device="loadlynx-d68638",
                load_cli="/Users/ivan/.local/bin/loadlynx",
                load_bridge_url="",
                load_ipc="",
                load_devd_base_url="",
                load_devd_socket="/tmp/loadlynx.sock",
                load_usb_device_id="digital-2bdfc170893f",
                isolapurr_cli="isolapurr",
                isolapurr_url="http://192.168.31.122",
                isolapurr_device_id="856a141cdbd4",
                mains_aegis_cli="mains-aegis",
                mains_aegis_ipc=None,
                ups_device_id="serial-04f3bb3f5367",
                ups_status_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/status",
                ups_settings_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/settings",
                devd_scan_url="http://127.0.0.1:38140/api/v1/devices/scan",
                artifact_manifest_12v="/tmp/12v.manifest.json",
                artifact_manifest_19v="/tmp/19v.manifest.json",
                firmware_bundle_root="/tmp",
                status_timeout_sec=20.0,
                skip_safe_prepare=True,
                dry_run=False,
                devd_diag_snapshot_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/diag-snapshot",
                devd_device_trace_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/trace?trace_limit=1",
                ups_observe_device_id=None,
            )
            with (
                mock.patch.object(self.readiness, "parse_args", return_value=args),
                mock.patch.object(
                    self.readiness.runner,
                    "ensure_valid_mains_aegis_devd_http_base",
                    return_value={"ok": True, "failures": []},
                ),
                mock.patch.object(
                    self.readiness.suite,
                    "probe_isolapurr_source_reachability",
                    return_value={"ok": False, "failures": ["http_ports_unreachable"]},
                ),
                mock.patch.object(
                    self.readiness.suite,
                    "wait_for_ups_external_input_cut",
                    return_value={"ok": True, "validation": {"ok": True, "failures": []}},
                ),
                mock.patch.object(self.readiness.suite, "http_post_json", return_value={"devices": []}),
                mock.patch.object(
                    self.readiness.suite,
                    "http_get_json",
                    return_value={"devices": [{"id": "mains-aegis-198840", "connection": "connected", "identity": {}, "settings": {}}]},
                ),
                mock.patch.object(self.readiness.suite, "devd_device_entry_from_listing", return_value={}) as entry_mock,
                mock.patch.object(self.readiness.suite, "connect_device", return_value={"ok": True}) as connect_mock,
                mock.patch.object(
                    self.readiness.suite,
                    "read_device_identity",
                    return_value={"result": {"hardware_capabilities": {"output_profile": "12v", "rated_vout_mv": 12000}}},
                ),
                mock.patch.object(
                    self.readiness.suite,
                    "read_device_settings",
                    return_value={"result": {"advanced_power_capabilities": {"rated_vout_mv": 12000}}},
                ),
                mock.patch.object(
                    self.readiness,
                    "direct_http_identity_settings",
                    return_value=(
                        {"result": {"hardware_capabilities": {"output_profile": "12v", "rated_vout_mv": 12000}}},
                        {"result": {"advanced_power_capabilities": {"rated_vout_mv": 12000}}},
                    ),
                ),
                mock.patch.object(
                    self.readiness.suite,
                    "validate_dual_surface_hardware_capabilities",
                    return_value={"ok": True, "failures": []},
                ),
                mock.patch.object(self.readiness.suite, "read_selected_artifact", return_value={"result": {}}),
                mock.patch.object(
                    self.readiness.suite,
                    "artifact_manifest_for_profile",
                    side_effect=lambda _args, profile: f"/tmp/{profile}.manifest.json",
                ),
                mock.patch.object(
                    self.readiness,
                    "load_status_payload",
                    return_value={"control": {"output_enabled": False, "mode": "cc"}},
                ),
                mock.patch.object(
                    self.readiness,
                    "read_ups_cli_observation_surfaces",
                    return_value={"ok": True, "failures": []},
                ),
                mock.patch.object(self.readiness, "run_telemetry_gate", return_value={"ok": True, "failures": []}),
            ):
                rc = self.readiness.main()
            self.assertEqual(rc, 1)
            self.assertEqual(entry_mock.call_count, 1)
            connect_mock.assert_not_called()

    def test_main_safe_prepare_skips_cut_source_when_source_reachability_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            args = self.readiness.argparse.Namespace(
                report_root=tmp,
                load_device="loadlynx-d68638",
                load_cli="/Users/ivan/.local/bin/loadlynx",
                load_bridge_url="",
                load_ipc="",
                load_devd_base_url="",
                load_devd_socket="/tmp/loadlynx.sock",
                load_usb_device_id="digital-2bdfc170893f",
                isolapurr_cli="isolapurr",
                isolapurr_url="http://192.168.31.122",
                isolapurr_device_id="856a141cdbd4",
                mains_aegis_cli="mains-aegis",
                mains_aegis_ipc=None,
                ups_device_id="serial-04f3bb3f5367",
                ups_status_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/status",
                ups_settings_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/settings",
                devd_scan_url="http://127.0.0.1:38140/api/v1/devices/scan",
                artifact_manifest_12v="/tmp/12v.manifest.json",
                artifact_manifest_19v="/tmp/19v.manifest.json",
                firmware_bundle_root="/tmp",
                status_timeout_sec=20.0,
                skip_safe_prepare=False,
                dry_run=False,
                devd_diag_snapshot_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/diag-snapshot",
                devd_device_trace_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/trace?trace_limit=1",
                ups_observe_device_id=None,
            )
            with (
                mock.patch.object(self.readiness, "parse_args", return_value=args),
                mock.patch.object(self.readiness.suite, "disable_load", return_value={"ok": True}),
                mock.patch.object(
                    self.readiness.suite,
                    "probe_isolapurr_source_reachability",
                    return_value={"ok": False, "failures": ["http_ports_unreachable"]},
                ),
                mock.patch.object(
                    self.readiness.suite,
                    "wait_for_ups_external_input_cut",
                    return_value={"ok": True, "validation": {"ok": True, "failures": []}},
                ),
                mock.patch.object(self.readiness.suite, "cut_source_power_only") as cut_source_mock,
                mock.patch.object(self.readiness.suite, "http_post_json", return_value={"devices": []}),
                mock.patch.object(
                    self.readiness.suite,
                    "http_get_json",
                    return_value={"devices": [{"id": "mains-aegis-198840", "connection": "connected", "identity": {}, "settings": {}}]},
                ),
                mock.patch.object(self.readiness.suite, "devd_device_entry_from_listing", return_value={}),
                mock.patch.object(self.readiness.suite, "connect_device", return_value={"ok": True}) as connect_mock,
                mock.patch.object(
                    self.readiness,
                    "direct_http_identity_settings",
                    return_value=(
                        {"result": {"hardware_capabilities": {"output_profile": "12v", "rated_vout_mv": 12000}}},
                        {"result": {"advanced_power_capabilities": {"rated_vout_mv": 12000}}},
                    ),
                ),
                mock.patch.object(
                    self.readiness.suite,
                    "validate_dual_surface_hardware_capabilities",
                    return_value={"ok": False, "failures": ["active_profile_unknown"]},
                ),
                mock.patch.object(self.readiness.suite, "read_selected_artifact", return_value={"result": {}}),
                mock.patch.object(
                    self.readiness.suite,
                    "artifact_manifest_for_profile",
                    side_effect=lambda _args, profile: f"/tmp/{profile}.manifest.json",
                ),
                mock.patch.object(
                    self.readiness,
                    "load_status_payload",
                    return_value={"control": {"output_enabled": False, "mode": "cc"}},
                ),
                mock.patch.object(
                    self.readiness,
                    "read_ups_cli_observation_surfaces",
                    return_value={"ok": True, "failures": []},
                ),
                mock.patch.object(self.readiness, "run_telemetry_gate", return_value={"ok": True, "failures": []}),
            ):
                rc = self.readiness.main()
            self.assertEqual(rc, 1)
            cut_source_mock.assert_not_called()
            connect_mock.assert_not_called()

    def test_main_fails_early_when_devd_bootstrap_gate_is_invalid(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            args = self.readiness.argparse.Namespace(
                report_root=tmp,
                load_device="loadlynx-d68638",
                load_cli="/Users/ivan/.local/bin/loadlynx",
                load_bridge_url="",
                load_ipc="",
                load_devd_base_url="",
                load_devd_socket="/tmp/loadlynx.sock",
                load_usb_device_id="digital-2bdfc170893f",
                isolapurr_cli="isolapurr",
                isolapurr_url="http://192.168.31.122",
                isolapurr_device_id="856a141cdbd4",
                mains_aegis_cli="mains-aegis",
                mains_aegis_ipc=None,
                ups_device_id="serial-04f3bb3f5367",
                ups_status_url="http://127.0.0.1:30080/api/v1/devices/mains-aegis-198840/status",
                ups_settings_url="http://127.0.0.1:30080/api/v1/devices/mains-aegis-198840/settings",
                devd_scan_url="http://127.0.0.1:30080/api/v1/devices/scan",
                artifact_manifest_12v="/tmp/12v.manifest.json",
                artifact_manifest_19v="/tmp/19v.manifest.json",
                firmware_bundle_root="/tmp",
                status_timeout_sec=20.0,
                skip_safe_prepare=True,
                dry_run=False,
                devd_diag_snapshot_url="http://127.0.0.1:30080/api/v1/devices/mains-aegis-198840/diag-snapshot",
                devd_device_trace_url="http://127.0.0.1:30080/api/v1/devices/mains-aegis-198840/trace?trace_limit=1",
                ups_observe_device_id=None,
            )
            with (
                mock.patch.object(self.readiness, "parse_args", return_value=args),
                mock.patch.object(
                    self.readiness.runner,
                    "ensure_valid_mains_aegis_devd_http_base",
                    return_value={"ok": False, "failures": ["bootstrap_app_name_mismatch"]},
                ),
                mock.patch.object(self.readiness.suite, "probe_isolapurr_source_reachability") as source_gate_mock,
            ):
                rc = self.readiness.main()
            self.assertEqual(rc, 1)
            source_gate_mock.assert_not_called()

    def test_load_status_payload_prefers_best_effort_status_path(self) -> None:
        args = self.readiness.argparse.Namespace(
            load_bridge_url="",
            load_ipc="/tmp/loadlynx-koha-formal-2.sock",
            load_devd_base_url="",
            load_devd_socket="/tmp/loadlynx-koha-formal-2.sock",
            load_usb_device_id="digital-2bdfc170893f",
            status_timeout_sec=20.0,
        )
        with mock.patch.object(
            self.readiness.runner,
            "get_load_status_via_ipc_helper",
            return_value={"control": {"output_enabled": False, "mode": "cc"}, "source": "ipc_helper_status"},
        ) as ipc_mock, mock.patch.object(
            self.readiness.runner,
            "get_load_status_best_effort",
            return_value={"control": {"output_enabled": False, "mode": "cc"}, "source": "ipc"},
        ) as status_mock:
            payload = self.readiness.load_status_payload(
                args,
                load_cli="/Users/ivan/.local/bin/loadlynx",
                load_device="loadlynx-d68638",
                dry_run=False,
            )
        self.assertEqual(payload["source"], "ipc_helper_status")
        ipc_mock.assert_called_once()
        status_mock.assert_not_called()

    def test_load_status_payload_falls_back_to_best_effort_when_ipc_helper_fails(self) -> None:
        args = self.readiness.argparse.Namespace(
            load_bridge_url="",
            load_ipc="/tmp/loadlynx-koha-formal-2.sock",
            load_devd_base_url="",
            load_devd_socket="/tmp/loadlynx-koha-formal-2.sock",
            load_usb_device_id="digital-2bdfc170893f",
            status_timeout_sec=20.0,
        )
        with mock.patch.object(
            self.readiness.runner,
            "get_load_status_via_ipc_helper",
            side_effect=RuntimeError("ipc helper failed"),
        ) as ipc_mock, mock.patch.object(
            self.readiness.runner,
            "get_load_status_best_effort",
            return_value={"control": {"output_enabled": False, "mode": "cc"}, "source": "ipc"},
        ) as status_mock:
            payload = self.readiness.load_status_payload(
                args,
                load_cli="/Users/ivan/.local/bin/loadlynx",
                load_device="loadlynx-d68638",
                dry_run=False,
            )
        self.assertEqual(payload["source"], "ipc")
        ipc_mock.assert_called_once()
        status_mock.assert_called_once()

    def test_load_status_payload_returns_structured_error_without_cli_fallback(self) -> None:
        args = self.readiness.argparse.Namespace(
            load_bridge_url="",
            load_ipc="/tmp/loadlynx-koha-formal-2.sock",
            load_devd_base_url="",
            load_devd_socket="/tmp/loadlynx-koha-formal-2.sock",
            load_usb_device_id="digital-2bdfc170893f",
            status_timeout_sec=20.0,
        )
        with mock.patch.object(
            self.readiness.runner,
            "get_load_status_via_ipc_helper",
            side_effect=RuntimeError("ipc helper failed"),
        ) as ipc_mock, mock.patch.object(
            self.readiness.runner,
            "get_load_status_best_effort",
            return_value={"ok": False, "error": "best_effort_failed"},
        ) as status_mock:
            payload = self.readiness.load_status_payload(
                args,
                load_cli="/Users/ivan/.local/bin/loadlynx",
                load_device="loadlynx-d68638",
                dry_run=False,
            )
        self.assertEqual(payload["error"], "best_effort_failed")
        self.assertIn("ipc_helper_error", payload)
        ipc_mock.assert_called_once()
        status_mock.assert_called_once()


if __name__ == "__main__":
    unittest.main()
