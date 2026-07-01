#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import json
import subprocess
import tempfile
import unittest
import urllib.error
from unittest import mock
from pathlib import Path
from types import SimpleNamespace


def load_module(filename: str, module_name: str):
    path = Path(__file__).with_name(filename)
    spec = importlib.util.spec_from_file_location(module_name, path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


class FormalHilSuiteTests(unittest.TestCase):
    def setUp(self) -> None:
        self.suite = load_module("formal_hil_suite.py", "formal_hil_suite")
        self.verify = load_module("verify_formal_suite.py", "verify_formal_suite")
        self.overview = load_module("render_formal_suite_html.py", "render_formal_suite_html")

    def test_build_report_entry_uses_actual_source_window_and_relative_report_dir(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            run_dir = root / "run-a"
            run_dir.mkdir()
            payload = {
                "metadata": {
                    "target_ma": 3900,
                    "include_backup": True,
                    "load_min_v_mv": 3000,
                    "max_i_ma_total": 4000,
                    "max_p_mw": 80000,
                    "source_voltage_mv": 12000,
                    "source_current_limit_ma": 3000,
                },
                "settings_snapshot": {
                    "advanced_power": {
                        "standby_drop_mv": 1200,
                        "assist_low_drop_mv": 600,
                    }
                },
                "summary": {
                    "all": {
                        "completeness": {
                            "scene_complete": True,
                            "failures": [],
                            "effective_sample_rate_hz": 4.0,
                            "max_sample_gap_s": 0.25,
                        },
                        "acceptance": {
                            "run_validity": "valid_for_signoff",
                            "signoff_valid": True,
                            "failed_acceptance_checks": [],
                        },
                    }
                },
                "samples": [
                    {"port_c_enabled": True, "isolapurr_port_c_mv": 11982},
                    {"port_c_enabled": True, "isolapurr_port_c_mv": 12021},
                    {"port_c_enabled": False, "isolapurr_port_c_mv": None},
                ],
            }
            (run_dir / "results.json").write_text(json.dumps(payload), encoding="utf-8")
            entry, _raw = self.suite.build_report_entry(
                report_root=root,
                run_dir=run_dir,
                profile_key="12v",
                scene_key="assist_path",
                artifact_identity={"build_id": "abc"},
            )
            self.assertEqual(entry["report_dir"], "run-a")
            self.assertEqual(entry["source_online_mv_min"], 11982)
            self.assertEqual(entry["source_online_mv_max"], 12021)
            self.assertEqual(entry["run_validity"], "valid_for_signoff")
            self.assertEqual(entry["advanced_power"]["standby_drop_mv"], 1200)

    def test_resolve_report_dir_keeps_relative_path_structure(self) -> None:
        summary_root = Path("/tmp/suite-root")
        resolved = self.verify.resolve_report_dir(summary_root, "nested/report-a")
        self.assertEqual(resolved, (summary_root / "nested/report-a").resolve())

    def test_verify_formal_suite_accepts_cli_suite_schema(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            report_dir = root / "scene-a"
            report_dir.mkdir()
            advanced_power = {"standby_drop_mv": 1200, "assist_low_drop_mv": 600}
            results = {
                "metadata": {
                    "target_ma": 3900,
                    "include_backup": True,
                    "output_profile": "12v",
                    "scene_type": "assist_path",
                    "source_voltage_mv": 12000,
                    "source_current_limit_ma": 3000,
                    "load_min_v_mv": 3000,
                    "max_i_ma_total": 4000,
                    "max_p_mw": 80000,
                },
                "settings_snapshot": {"advanced_power": advanced_power},
                "summary": {
                    "all": {
                        "completeness": {
                            "scene_complete": True,
                            "failures": [],
                            "required_voltage_series": {
                                "source_v": True,
                                "ups_vin": True,
                                "ups_vout": True,
                                "load_v": True,
                            },
                        },
                        "acceptance": {
                            "run_validity": "valid_for_signoff",
                            "signoff_valid": True,
                            "failed_acceptance_checks": [],
                        },
                    }
                },
            }
            (report_dir / "results.json").write_text(json.dumps(results), encoding="utf-8")
            rows = [
                {
                    "t_s": 0.0,
                    "phase": "hold",
                    "port_c_enabled": True,
                    "isolapurr_port_c_mv": 11990,
                    "vin_vbus_mv": 11980,
                    "out_a_vbus_mv": 10800,
                    "load_v_local_mv": 10750,
                },
                {
                    "t_s": 0.25,
                    "phase": "hold",
                    "port_c_enabled": True,
                    "isolapurr_port_c_mv": 12010,
                    "vin_vbus_mv": 11990,
                    "out_a_vbus_mv": 10810,
                    "load_v_local_mv": 10760,
                },
            ]
            (report_dir / "timeseries.jsonl").write_text(
                "".join(json.dumps(row) + "\n" for row in rows),
                encoding="utf-8",
            )
            summary = {
                "suite_id": "suite-cli",
                "transport": {
                    "ups": "CLI + native IPC + USB",
                    "loadlynx": "CLI + native IPC + USB",
                    "isolapurr": "CLI + default IPC",
                },
                "reports": [
                    {
                        "report_dir": "scene-a",
                        "output_profile": "12v",
                        "scene_type": "assist_path",
                        "target_ma": 3900,
                        "include_backup": True,
                        "load_min_v_mv": 3000,
                        "load_max_i_ma_total": 4000,
                        "load_max_p_mw": 80000,
                        "source_voltage_mv": 12000,
                        "source_current_limit_ma": 3000,
                        "failures": [],
                        "advanced_power": advanced_power,
                    }
                ],
            }
            summary_path = root / "suite-summary.json"
            summary_path.write_text(json.dumps(summary), encoding="utf-8")
            with mock.patch.object(self.verify, "parse_args", return_value=SimpleNamespace(summary=str(summary_path))):
                self.assertEqual(self.verify.main(), 0)

    def test_overview_uses_relative_href_from_output_path(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp)
            summary_path = base / "reports" / "suite-summary.json"
            output_path = base / "reports" / "suite-overview.html"
            report_dir = base / "reports" / "runs" / "scene-a"
            report_dir.mkdir(parents=True)
            payload = {
                "suite_id": "suite-1",
                "load_protection": {"min_v_mv": 3000, "max_i_ma_total": 4000, "max_p_mw": 80000},
                "transport": {},
                "profiles": {},
                "reports": [
                    {
                        "report_dir": "runs/scene-a",
                        "output_profile": "12v",
                        "scene_type": "assist_path",
                        "target_ma": 3900,
                        "source_voltage_mv": 12000,
                        "source_current_limit_ma": 3000,
                        "scene_complete": True,
                        "signoff_valid": True,
                        "run_validity": "valid_for_signoff",
                        "effective_sample_rate_hz": 4.0,
                        "max_sample_gap_s": 0.25,
                        "failures": [],
                        "failed_acceptance_checks": [],
                        "advanced_power": {},
                    }
                ],
            }
            html = self.overview.render_html(summary_path, output_path, payload)
            self.assertIn('src="runs/scene-a/voltage-chart.html?embed=1"', html)

    def test_resolve_manifest_from_bundle_matches_profile_feature_set(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            bundle = Path(tmp)
            artifact_12 = {
                "artifact_id": "artifact-12",
                "features": ["net_http", "web_serial"],
            }
            artifact_19 = {
                "artifact_id": "artifact-19",
                "features": ["net_http", "web_serial", "main-vout-19v"],
            }
            (bundle / "firmware-catalog.json").write_text(
                json.dumps({"schema_version": 1, "artifacts": [artifact_12, artifact_19]}),
                encoding="utf-8",
            )
            (bundle / "artifact-12.manifest.json").write_text("{}", encoding="utf-8")
            (bundle / "artifact-19.manifest.json").write_text("{}", encoding="utf-8")
            resolved_12 = self.suite.resolve_manifest_from_bundle(
                bundle_root=bundle,
                profile_key="12v",
            )
            resolved_19 = self.suite.resolve_manifest_from_bundle(
                bundle_root=bundle,
                profile_key="19v",
            )
            self.assertEqual(resolved_12, str((bundle / "artifact-12.manifest.json")))
            self.assertEqual(resolved_19, str((bundle / "artifact-19.manifest.json")))

    def test_connect_device_with_retry_recovers_after_transient_failure(self) -> None:
        args = SimpleNamespace(
            mains_aegis_cli="mains-aegis",
            mains_aegis_ipc="/tmp/mains-aegis-test.sock",
            ups_device_id="serial-04f3bb3f5367",
        )
        transient = subprocess.CalledProcessError(
            1,
            ["mains-aegis", "device", "serial-04f3bb3f5367", "connect"],
            output="",
            stderr="failed to open /dev/cu.usbmodem212201: No such file or directory",
        )
        with mock.patch.object(
            self.suite,
            "connect_device",
            side_effect=[transient, {"cmd": ["ok"], "result": {"connection": "connected"}}],
        ) as connect_mock, mock.patch.object(self.suite.time, "sleep", autospec=True):
            result = self.suite.connect_device_with_retry(
                args,
                dry_run=False,
                timeout_sec=2.0,
                retry_interval_sec=0.01,
            )
        self.assertEqual(result["result"]["connection"], "connected")
        self.assertEqual(connect_mock.call_count, 2)
        self.assertEqual(len(result["retry_attempts"]), 1)

    def test_validate_ups_external_input_restored_accepts_matching_12v_window(self) -> None:
        result = self.suite.validate_ups_external_input_restored(
            profile_key="12v",
            status_payload={
                "mode": "standby",
                "input": {
                    "mains_present": True,
                    "vin_vbus_mv": 12016,
                    "assist_power_stage": "standby",
                    "source": "dcin",
                    "input_vbus_mv": 12083,
                },
            },
        )
        self.assertTrue(result["ok"])
        self.assertEqual(result["failures"], [])

    def test_validate_ups_external_input_restored_rejects_backup_or_out_of_window(self) -> None:
        result = self.suite.validate_ups_external_input_restored(
            profile_key="12v",
            status_payload={
                "mode": "backup",
                "input": {
                    "mains_present": False,
                    "vin_vbus_mv": 1600,
                    "assist_power_stage": "backup",
                    "source": "usbc",
                    "input_vbus_mv": 5106,
                },
            },
        )
        self.assertFalse(result["ok"])
        self.assertIn("ups_mains_present_not_true", result["failures"])
        self.assertIn("ups_vin_vbus_out_of_profile_window", result["failures"])
        self.assertIn("ups_backup_semantics_still_active", result["failures"])

    def test_devd_device_entry_from_listing_matches_logical_device_id(self) -> None:
        payload = {
            "devices": [
                {
                    "id": "serial-04f3bb3f5367",
                    "identity": {
                        "device_id": "mains-aegis-198840",
                    },
                }
            ]
        }
        result = self.suite.devd_device_entry_from_listing(
            payload,
            device_id="mains-aegis-198840",
        )
        self.assertEqual(result["id"], "serial-04f3bb3f5367")

    def test_observe_device_id_prefers_device_id_derived_from_observe_urls(self) -> None:
        args = SimpleNamespace(
            ups_observe_device_id=None,
            devd_diag_snapshot_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/diag-snapshot",
            ups_status_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/status",
            ups_settings_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/settings",
            ups_device_id="serial-04f3bb3f5367",
        )
        self.assertEqual(
            self.suite.observe_device_id_from_args(args),
            "mains-aegis-198840",
        )

    def test_build_runner_cmd_uses_observe_device_id_for_monitor_url_and_control_id_for_cli(self) -> None:
        args = SimpleNamespace(
            runner="runner.py",
            output_profiles=["12v"],
            scenes=["assist_path"],
            load_device="loadlynx-d68638",
            load_usb_port="/dev/cu.usbmodem212101",
            load_cli="/Users/ivan/.local/bin/loadlynx",
            load_bridge_url="http://127.0.0.1:30180",
            load_bridge_device="",
            load_devd_socket="/tmp/loadlynx.sock",
            mains_aegis_cli="mains-aegis",
            ups_device_id="serial-04f3bb3f5367",
            load_min_v_mv=3000,
            max_i_ma_total=4000,
            max_p_mw=80000,
            isolapurr_cli="isolapurr",
            isolapurr_url="http://192.168.31.122",
            isolapurr_device_id="856a141cdbd4",
            ups_status_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/status",
            ups_settings_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/settings",
            devd_diag_snapshot_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/diag-snapshot",
            devd_device_trace_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/trace?trace_limit=1",
            devd_scan_url="http://127.0.0.1:38140/api/v1/devices/scan",
            pre_seconds=12.0,
            hold_seconds=18.0,
            backup_hold_seconds=18.0,
            restore_hold_seconds=18.0,
            post_seconds=12.0,
            sample_interval_seconds=0.25,
            load_stream_interval_seconds=0.2,
            load_status_ready_timeout_sec=20.0,
            command_timeout_sec=45.0,
            status_timeout_sec=20.0,
            verify_timeout_sec=45.0,
            report_root="/tmp/reports",
            mains_aegis_ipc=None,
            load_ipc="",
            load_devd_base_url="",
            ups_observe_device_id=None,
        )
        cmd = self.suite.build_runner_cmd(args, profile_key="12v", scene_key="assist_path")
        self.assertIn("--ups-device-id", cmd)
        self.assertEqual(cmd[cmd.index("--ups-device-id") + 1], "serial-04f3bb3f5367")
        self.assertIn("--isolapurr-device-id", cmd)
        self.assertEqual(cmd[cmd.index("--isolapurr-device-id") + 1], "856a141cdbd4")
        self.assertIn("--load-bridge-url", cmd)
        self.assertEqual(cmd[cmd.index("--load-bridge-url") + 1], "")
        self.assertIn("--load-devd-socket", cmd)
        self.assertEqual(cmd[cmd.index("--load-devd-socket") + 1], "/tmp/loadlynx.sock")
        self.assertIn("--devd-monitor-start-url", cmd)
        self.assertEqual(
            cmd[cmd.index("--devd-monitor-start-url") + 1],
            "http://127.0.0.1:38140/api/v1/devices/serial-04f3bb3f5367/monitor/start",
        )
        self.assertEqual(
            cmd[cmd.index("--ups-status-url") + 1],
            "http://127.0.0.1:38140/api/v1/devices/serial-04f3bb3f5367/status",
        )
        self.assertEqual(
            cmd[cmd.index("--ups-settings-url") + 1],
            "http://127.0.0.1:38140/api/v1/devices/serial-04f3bb3f5367/settings",
        )
        self.assertEqual(
            cmd[cmd.index("--devd-diag-snapshot-url") + 1],
            "http://127.0.0.1:38140/api/v1/devices/serial-04f3bb3f5367/diag-snapshot",
        )
        self.assertEqual(
            cmd[cmd.index("--devd-device-trace-url") + 1],
            "http://127.0.0.1:38140/api/v1/devices/serial-04f3bb3f5367/trace?trace_limit=1",
        )

    def test_build_runner_cmd_rewrites_devd_observe_urls_to_scan_base(self) -> None:
        args = SimpleNamespace(
            runner="runner.py",
            output_profiles=["12v"],
            scenes=["assist_path"],
            load_device="loadlynx-d68638",
            load_usb_port="/dev/cu.usbmodem212101",
            load_cli="/Users/ivan/.local/bin/loadlynx",
            load_bridge_url="",
            load_bridge_device="",
            load_devd_socket="/tmp/loadlynx.sock",
            mains_aegis_cli="mains-aegis",
            ups_device_id="serial-04f3bb3f5367",
            load_min_v_mv=3000,
            max_i_ma_total=4000,
            max_p_mw=80000,
            isolapurr_cli="isolapurr",
            isolapurr_url="http://192.168.31.122",
            isolapurr_device_id="856a141cdbd4",
            ups_status_url="http://192.168.31.232/api/v1/status",
            ups_settings_url="http://192.168.31.232/api/v1/settings",
            devd_diag_snapshot_url="http://127.0.0.1:30080/api/v1/devices/mains-aegis-198840/diag-snapshot",
            devd_device_trace_url="http://127.0.0.1:30080/api/v1/devices/mains-aegis-198840/trace?trace_limit=1",
            devd_scan_url="http://127.0.0.1:38140/api/v1/devices/scan",
            pre_seconds=12.0,
            hold_seconds=18.0,
            backup_hold_seconds=18.0,
            restore_hold_seconds=18.0,
            post_seconds=12.0,
            sample_interval_seconds=0.25,
            load_stream_interval_seconds=0.2,
            load_status_ready_timeout_sec=20.0,
            command_timeout_sec=45.0,
            status_timeout_sec=20.0,
            verify_timeout_sec=45.0,
            report_root="/tmp/reports",
            mains_aegis_ipc=None,
            load_ipc="",
            load_devd_base_url="",
            ups_observe_device_id=None,
        )
        cmd = self.suite.build_runner_cmd(args, profile_key="12v", scene_key="assist_path")
        self.assertEqual(
            cmd[cmd.index("--devd-diag-snapshot-url") + 1],
            "http://127.0.0.1:38140/api/v1/devices/serial-04f3bb3f5367/diag-snapshot",
        )
        self.assertEqual(
            cmd[cmd.index("--devd-device-trace-url") + 1],
            "http://127.0.0.1:38140/api/v1/devices/serial-04f3bb3f5367/trace?trace_limit=1",
        )

    def test_build_runner_cmd_rewrites_ups_status_and_settings_urls_to_scan_base(self) -> None:
        args = SimpleNamespace(
            runner="runner.py",
            output_profiles=["12v"],
            scenes=["assist_path"],
            load_device="loadlynx-d68638",
            load_usb_port="/dev/cu.usbmodem212101",
            load_cli="/Users/ivan/.local/bin/loadlynx",
            load_bridge_url="",
            load_bridge_device="",
            load_devd_socket="/tmp/loadlynx.sock",
            mains_aegis_cli="mains-aegis",
            ups_device_id="serial-04f3bb3f5367",
            load_min_v_mv=3000,
            max_i_ma_total=4000,
            max_p_mw=80000,
            isolapurr_cli="isolapurr",
            isolapurr_url="http://192.168.31.122",
            isolapurr_device_id="856a141cdbd4",
            ups_status_url="http://127.0.0.1:30080/api/v1/devices/mains-aegis-198840/status",
            ups_settings_url="http://127.0.0.1:30080/api/v1/devices/mains-aegis-198840/settings",
            devd_diag_snapshot_url="http://127.0.0.1:30080/api/v1/devices/mains-aegis-198840/diag-snapshot",
            devd_device_trace_url="http://127.0.0.1:30080/api/v1/devices/mains-aegis-198840/trace?trace_limit=1",
            devd_scan_url="http://127.0.0.1:38140/api/v1/devices/scan",
            pre_seconds=12.0,
            hold_seconds=18.0,
            backup_hold_seconds=18.0,
            restore_hold_seconds=18.0,
            post_seconds=12.0,
            sample_interval_seconds=0.25,
            load_stream_interval_seconds=0.2,
            load_status_ready_timeout_sec=20.0,
            command_timeout_sec=45.0,
            status_timeout_sec=20.0,
            verify_timeout_sec=45.0,
            report_root="/tmp/reports",
            mains_aegis_ipc=None,
            load_ipc="",
            load_devd_base_url="",
            ups_observe_device_id=None,
        )
        cmd = self.suite.build_runner_cmd(args, profile_key="12v", scene_key="assist_path")
        self.assertEqual(
            cmd[cmd.index("--ups-status-url") + 1],
            "http://127.0.0.1:38140/api/v1/devices/serial-04f3bb3f5367/status",
        )
        self.assertEqual(
            cmd[cmd.index("--ups-settings-url") + 1],
            "http://127.0.0.1:38140/api/v1/devices/serial-04f3bb3f5367/settings",
        )

    def test_build_runner_cmd_keeps_bridge_when_no_load_devd_transport_is_configured(self) -> None:
        args = SimpleNamespace(
            runner="runner.py",
            output_profiles=["12v"],
            scenes=["assist_path"],
            load_device="loadlynx-d68638",
            load_usb_port="/dev/cu.usbmodem212101",
            load_cli="/Users/ivan/.local/bin/loadlynx",
            load_bridge_url="http://127.0.0.1:30180",
            load_bridge_device="",
            load_devd_socket="",
            mains_aegis_cli="mains-aegis",
            ups_device_id="serial-04f3bb3f5367",
            load_min_v_mv=3000,
            max_i_ma_total=4000,
            max_p_mw=80000,
            isolapurr_cli="isolapurr",
            isolapurr_url="http://192.168.31.122",
            isolapurr_device_id="856a141cdbd4",
            ups_status_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/status",
            ups_settings_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/settings",
            devd_diag_snapshot_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/diag-snapshot",
            devd_device_trace_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/trace?trace_limit=1",
            devd_scan_url="http://127.0.0.1:38140/api/v1/devices/scan",
            pre_seconds=12.0,
            hold_seconds=18.0,
            backup_hold_seconds=18.0,
            restore_hold_seconds=18.0,
            post_seconds=12.0,
            sample_interval_seconds=0.25,
            load_stream_interval_seconds=0.2,
            load_status_ready_timeout_sec=20.0,
            command_timeout_sec=45.0,
            status_timeout_sec=20.0,
            verify_timeout_sec=45.0,
            report_root="/tmp/reports",
            mains_aegis_ipc=None,
            load_ipc="",
            load_devd_base_url="",
            ups_observe_device_id=None,
        )
        cmd = self.suite.build_runner_cmd(args, profile_key="12v", scene_key="assist_path")
        self.assertEqual(cmd[cmd.index("--load-bridge-url") + 1], "http://127.0.0.1:30180")

    def test_build_runner_cmd_disables_default_bridge_when_explicit_load_ipc_is_configured(self) -> None:
        args = SimpleNamespace(
            runner="runner.py",
            output_profiles=["12v"],
            scenes=["assist_path"],
            load_device="loadlynx-d68638",
            load_usb_port="/dev/cu.usbmodem212101",
            load_cli="/Users/ivan/.local/bin/loadlynx",
            load_bridge_url="http://127.0.0.1:30180",
            load_bridge_device="",
            load_devd_socket="",
            mains_aegis_cli="mains-aegis",
            ups_device_id="serial-04f3bb3f5367",
            load_min_v_mv=3000,
            max_i_ma_total=4000,
            max_p_mw=80000,
            isolapurr_cli="isolapurr",
            isolapurr_url="http://192.168.31.122",
            isolapurr_device_id="856a141cdbd4",
            ups_status_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/status",
            ups_settings_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/settings",
            devd_diag_snapshot_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/diag-snapshot",
            devd_device_trace_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/trace?trace_limit=1",
            devd_scan_url="http://127.0.0.1:38140/api/v1/devices/scan",
            pre_seconds=12.0,
            hold_seconds=18.0,
            backup_hold_seconds=18.0,
            restore_hold_seconds=18.0,
            post_seconds=12.0,
            sample_interval_seconds=0.25,
            load_stream_interval_seconds=0.2,
            load_status_ready_timeout_sec=20.0,
            command_timeout_sec=45.0,
            status_timeout_sec=20.0,
            verify_timeout_sec=45.0,
            report_root="/tmp/reports",
            mains_aegis_ipc=None,
            load_ipc="/tmp/explicit-load-ipc.sock",
            load_devd_base_url="",
            ups_observe_device_id=None,
        )
        cmd = self.suite.build_runner_cmd(args, profile_key="12v", scene_key="assist_path")
        self.assertEqual(cmd[cmd.index("--load-bridge-url") + 1], "")

    def test_build_runner_cmd_keeps_default_devd_base_when_no_explicit_devd_transport_is_configured(self) -> None:
        args = SimpleNamespace(
            runner="runner.py",
            output_profiles=["12v"],
            scenes=["assist_path"],
            load_device="loadlynx-d68638",
            load_usb_port="/dev/cu.usbmodem212101",
            load_cli="/Users/ivan/.local/bin/loadlynx",
            load_bridge_url="http://127.0.0.1:30180",
            load_bridge_device="",
            load_devd_socket="",
            mains_aegis_cli="mains-aegis",
            ups_device_id="serial-04f3bb3f5367",
            load_min_v_mv=3000,
            max_i_ma_total=4000,
            max_p_mw=80000,
            isolapurr_cli="isolapurr",
            isolapurr_url="http://192.168.31.122",
            isolapurr_device_id="856a141cdbd4",
            ups_status_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/status",
            ups_settings_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/settings",
            devd_diag_snapshot_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/diag-snapshot",
            devd_device_trace_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/trace?trace_limit=1",
            devd_scan_url="http://127.0.0.1:38140/api/v1/devices/scan",
            pre_seconds=12.0,
            hold_seconds=18.0,
            backup_hold_seconds=18.0,
            restore_hold_seconds=18.0,
            post_seconds=12.0,
            sample_interval_seconds=0.25,
            load_stream_interval_seconds=0.2,
            load_status_ready_timeout_sec=20.0,
            command_timeout_sec=45.0,
            status_timeout_sec=20.0,
            verify_timeout_sec=45.0,
            report_root="/tmp/reports",
            mains_aegis_ipc=None,
            load_ipc="/tmp/explicit-load-ipc.sock",
            load_devd_base_url="http://127.0.0.1:20641",
            ups_observe_device_id=None,
        )
        cmd = self.suite.build_runner_cmd(args, profile_key="12v", scene_key="assist_path")
        self.assertIn("--load-devd-base-url", cmd)

    def test_run_formal_scene_dry_run_includes_scene_gate(self) -> None:
        args = SimpleNamespace(
            runner="runner.py",
            output_profiles=["12v"],
            scenes=["assist_path"],
            load_device="loadlynx-d68638",
            load_usb_port="/dev/cu.usbmodem212101",
            load_cli="/Users/ivan/.local/bin/loadlynx",
            load_bridge_url="",
            load_bridge_device="",
            load_devd_socket="/tmp/loadlynx.sock",
            mains_aegis_cli="mains-aegis",
            ups_device_id="serial-04f3bb3f5367",
            load_min_v_mv=3000,
            max_i_ma_total=4000,
            max_p_mw=80000,
            isolapurr_cli="isolapurr",
            isolapurr_url="http://192.168.31.122",
            isolapurr_device_id="856a141cdbd4",
            ups_status_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/status",
            ups_settings_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/settings",
            devd_diag_snapshot_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/diag-snapshot",
            devd_device_trace_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/trace?trace_limit=1",
            devd_scan_url="http://127.0.0.1:38140/api/v1/devices/scan",
            pre_seconds=12.0,
            hold_seconds=18.0,
            backup_hold_seconds=18.0,
            restore_hold_seconds=18.0,
            post_seconds=12.0,
            sample_interval_seconds=0.25,
            load_stream_interval_seconds=0.2,
            load_status_ready_timeout_sec=20.0,
            command_timeout_sec=45.0,
            status_timeout_sec=20.0,
            verify_timeout_sec=45.0,
            report_root="/tmp/reports",
            mains_aegis_ipc=None,
            load_ipc="",
            load_devd_base_url="",
            ups_observe_device_id=None,
        )
        fake_gate = {
            "ok": True,
            "profile": "12v",
            "expected_source_voltage_mv": 12000,
            "actions": [{"disable_load_before_scene": {"dry_run": True}}],
        }
        with mock.patch.object(
            self.suite,
            "prepare_scene_source_and_capability_gate",
            autospec=True,
            return_value=fake_gate,
        ) as gate_mock:
            payload = self.suite.run_formal_scene(
                args,
                profile_key="12v",
                scene_key="assist_path",
                dry_run=True,
            )
        self.assertEqual(gate_mock.call_count, 1)
        self.assertEqual(payload["scene_gate"], fake_gate)
        self.assertTrue(payload["dry_run"])

    def test_seeded_devd_device_is_capability_ready_requires_connected_identity_and_settings(self) -> None:
        self.assertTrue(
            self.suite.seeded_devd_device_is_capability_ready(
                {
                    "connection": "connected",
                    "identity": {},
                    "settings": {},
                }
            )
        )
        self.assertFalse(
            self.suite.seeded_devd_device_is_capability_ready(
                {
                    "connection": "disconnected",
                    "identity": {},
                    "settings": {},
                }
            )
        )
        self.assertFalse(
            self.suite.seeded_devd_device_is_capability_ready(
                {
                    "connection": "connected",
                    "identity": {},
                }
            )
        )

    def test_validate_profile_hardware_capabilities_accepts_matching_19v_identity_and_settings(self) -> None:
        result = self.suite.validate_profile_hardware_capabilities(
            profile_key="19v",
            identity_payload={
                "hardware_capabilities": {
                    "output_profile": "19v",
                    "rated_vout_mv": 19000,
                }
            },
            settings_payload={
                "advanced_power_capabilities": {
                    "rated_vout_mv": 19000,
                }
            },
        )
        self.assertTrue(result["ok"])
        self.assertEqual(result["failures"], [])

    def test_validate_profile_hardware_capabilities_rejects_profile_mismatch(self) -> None:
        result = self.suite.validate_profile_hardware_capabilities(
            profile_key="12v",
            identity_payload={
                "hardware_capabilities": {
                    "output_profile": "19v",
                    "rated_vout_mv": 19000,
                }
            },
            settings_payload={
                "advanced_power_capabilities": {
                    "rated_vout_mv": 19000,
                }
            },
        )
        self.assertFalse(result["ok"])
        self.assertIn("identity_output_profile_mismatch", result["failures"])
        self.assertIn("settings_output_profile_mismatch", result["failures"])

    def test_validate_dual_surface_hardware_capabilities_rejects_usb_http_mismatch(self) -> None:
        result = self.suite.validate_dual_surface_hardware_capabilities(
            profile_key="12v",
            usb_identity_payload={
                "hardware_capabilities": {
                    "output_profile": "12v",
                    "rated_vout_mv": 12000,
                }
            },
            usb_settings_payload={
                "advanced_power_capabilities": {
                    "rated_vout_mv": 12000,
                }
            },
            http_identity_payload={
                "hardware_capabilities": {
                    "output_profile": "19v",
                    "rated_vout_mv": 19000,
                }
            },
            http_settings_payload={
                "advanced_power_capabilities": {
                    "rated_vout_mv": 19000,
                }
            },
        )
        self.assertFalse(result["ok"])
        self.assertIn("http:identity_output_profile_mismatch", result["failures"])
        self.assertIn("http:settings_output_profile_mismatch", result["failures"])
        self.assertIn("usb_http_identity_caps_mismatch", result["failures"])
        self.assertIn("usb_http_settings_caps_mismatch", result["failures"])

    def test_validate_ups_external_input_cut_accepts_backup_with_usb_5v_still_present(self) -> None:
        result = self.suite.validate_ups_external_input_cut(
            {
                "mode": "backup",
                "input": {
                    "source": "usbc",
                    "mains_present": False,
                    "input_vbus_mv": 5103,
                    "vin_vbus_mv": 0,
                    "assist_power_stage": "backup",
                },
            }
        )
        self.assertTrue(result["ok"])
        self.assertEqual(result["failures"], [])

    def test_validate_ups_external_input_cut_rejects_when_vin_still_present(self) -> None:
        result = self.suite.validate_ups_external_input_cut(
            {
                "mode": "standby",
                "input": {
                    "source": "dcin",
                    "mains_present": True,
                    "input_vbus_mv": 5103,
                    "vin_vbus_mv": 12024,
                    "assist_power_stage": "standby",
                },
            }
        )
        self.assertFalse(result["ok"])
        self.assertIn("ups_vin_vbus_not_cut", result["failures"])
        self.assertIn("ups_mains_present_not_false", result["failures"])

    def test_wait_for_ups_external_input_cut_retries_transient_http_error(self) -> None:
        with mock.patch.object(
            self.suite,
            "http_get_json",
            autospec=True,
            side_effect=[
                urllib.error.HTTPError(
                    "http://127.0.0.1/status",
                    502,
                    "Bad Gateway",
                    hdrs=None,
                    fp=None,
                ),
                {
                    "mode": "backup",
                    "input": {
                        "source": "usbc",
                        "mains_present": False,
                        "input_vbus_mv": 5103,
                        "vin_vbus_mv": 0,
                        "assist_power_stage": "backup",
                    },
                },
            ],
        ), mock.patch.object(self.suite.time, "sleep", autospec=True):
            result = self.suite.wait_for_ups_external_input_cut(
                status_url="http://127.0.0.1/status",
                timeout_sec=1.0,
                dry_run=False,
            )
        self.assertTrue(result["ok"])

    def test_validate_source_configuration_accepts_matching_manual_ack(self) -> None:
        result = self.suite.validate_source_configuration(
            expected_voltage_mv=19000,
            expected_current_limit_ma=3000,
            set_source_payload={
                "enabled": True,
                "actions": [
                    {
                        "cmd": ["isolapurr", "power", "output", "manual"],
                        "result": {
                            "manual": {
                                "voltage_mv": 19000,
                                "current_limit_ma": 3000,
                                "path_policy": "force_close",
                                "usb_c_path_mode": "disconnect",
                            },
                            "tps_mode": "manual",
                        },
                    },
                    {
                        "method": "POST",
                        "settle": {
                            "ok": True,
                            "target_power_enabled": True,
                            "port": {
                                "portId": "port_c",
                                "state": {"power_enabled": True},
                            },
                        },
                    },
                ],
            },
            ports_payload={
                "ports": [
                    {
                        "portId": "port_c",
                        "state": {"power_enabled": False},
                    }
                ]
            },
        )
        self.assertTrue(result["ok"])
        self.assertEqual(result["failures"], [])

    def test_cut_source_power_only_does_not_require_voltage_or_current(self) -> None:
        with mock.patch.object(
            self.suite,
            "http_post_json",
            autospec=True,
            return_value={"accepted": True, "power_enabled": False},
        ), mock.patch.object(
            self.suite,
            "http_get_json",
            autospec=True,
            return_value={
                "ports": [
                    {
                        "portId": "port_c",
                        "state": {"power_enabled": False},
                    }
                ]
            },
        ):
            payload = self.suite.cut_source_power_only(
                isolapurr_url="http://example",
                dry_run=False,
            )
        self.assertEqual(payload["enabled"], False)
        settle = payload["actions"][0]["settle"]
        self.assertTrue(settle["ok"])
        self.assertEqual(settle["target_power_enabled"], False)

    def test_suite_stops_before_source_configuration_when_capability_gate_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            report_root = Path(tmp) / "reports"
            report_root.mkdir()
            bundle_root = Path(tmp) / "bundle"
            bundle_root.mkdir()
            (bundle_root / "firmware-catalog.json").write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "artifacts": [
                            {
                                "artifact_id": "artifact-12",
                                "features": ["net_http", "web_serial"],
                            },
                            {
                                "artifact_id": "artifact-19",
                                "features": ["net_http", "web_serial", "main-vout-19v"],
                            },
                        ]
                    }
                ),
                encoding="utf-8",
            )
            (bundle_root / "artifact-12.manifest.json").write_text("{}", encoding="utf-8")
            (bundle_root / "artifact-19.manifest.json").write_text("{}", encoding="utf-8")

            argv = [
                "formal_hil_suite.py",
                "--report-root",
                str(report_root),
                "--firmware-bundle-root",
                str(bundle_root),
                "--load-device",
                "loadlynx-d68638",
                "--load-usb-port",
                "/dev/cu.usbmodem212101",
                "--load-cli",
                "/Users/ivan/.local/bin/loadlynx",
                "--load-devd-socket",
                "/tmp/loadlynx-test.sock",
                "--mains-aegis-ipc",
                "/tmp/mains-aegis-test.sock",
                "--isolapurr-url",
                "http://192.168.31.122",
                "--ups-device-id",
                "serial-04f3bb3f5367",
                "--ups-status-url",
                "http://127.0.0.1:35830/api/v1/devices/mains-aegis-198840/status",
                "--ups-settings-url",
                "http://127.0.0.1:35830/api/v1/devices/mains-aegis-198840/settings",
                "--devd-diag-snapshot-url",
                "http://127.0.0.1:35830/api/v1/devices/mains-aegis-198840/diag-snapshot",
                "--output-profiles",
                "12v",
                "--scenes",
                "assist_path",
                "--skip-flash",
            ]

            with mock.patch.object(self.suite.sys, "argv", argv), \
                mock.patch.object(
                    self.suite,
                    "probe_isolapurr_source_reachability",
                    autospec=True,
                    return_value={"ok": True, "failures": []},
                ), \
                mock.patch.object(
                    self.suite,
                    "disable_load",
                    autospec=True,
                    return_value={"ok": True},
                ), \
                mock.patch.object(
                    self.suite,
                    "cut_source_power_only",
                    autospec=True,
                    return_value={"enabled": False, "actions": []},
                ), \
                mock.patch.object(
                    self.suite,
                    "read_selected_artifact",
                    autospec=True,
                    return_value={"result": {"artifact_id": "artifact-12"}},
                ), \
                mock.patch.object(
                    self.suite,
                    "refresh_control_devices",
                    autospec=True,
                    return_value={"result": {"devices": []}},
                ) as refresh_control_devices, \
                mock.patch.object(
                    self.suite,
                    "connect_device",
                    autospec=True,
                    return_value={"result": {"connection": "connected"}},
                ), \
                mock.patch.object(
                    self.suite,
                    "read_device_identity",
                    autospec=True,
                    return_value={"result": {"hardware_capabilities": {"output_profile": "19v", "rated_vout_mv": 19000}}},
                ), \
                mock.patch.object(
                    self.suite,
                    "read_device_settings",
                    autospec=True,
                    return_value={"result": {"advanced_power_capabilities": {"rated_vout_mv": 19000}}},
                ), \
                mock.patch.object(
                    self.suite,
                    "http_get_json",
                    autospec=True,
                    side_effect=[
                        {
                            "mode": "backup",
                            "input": {
                                "source": "usbc",
                                "mains_present": False,
                                "input_vbus_mv": 5103,
                                "vin_vbus_mv": 0,
                                "assist_power_stage": "backup",
                            },
                        },
                        {"hardware_capabilities": {"output_profile": "19v", "rated_vout_mv": 19000}},
                        {"advanced_power_capabilities": {"rated_vout_mv": 19000}},
                    ],
                ), \
                mock.patch.object(
                    self.suite,
                    "http_post_json",
                    autospec=True,
                    return_value={
                        "devices": [
                            {
                                "id": "serial-04f3bb3f5367",
                                "connection": "connected",
                                "identity": {
                                    "device_id": "mains-aegis-198840",
                                    "hardware_capabilities": {
                                        "output_profile": "19v",
                                        "rated_vout_mv": 19000,
                                    },
                                },
                                "settings": {
                                    "advanced_power_capabilities": {
                                        "rated_vout_mv": 19000,
                                    }
                                },
                                "diag_snapshot": {},
                            }
                        ]
                    },
                ), \
                mock.patch.object(
                    self.suite,
                    "configure_source_manual_output",
                    autospec=True,
                ) as configure_source:
                with self.assertRaises(SystemExit) as exc:
                    self.suite.main()

            self.assertIn("hardware capability gate failed", str(exc.exception))
            refresh_control_devices.assert_called()
            configure_source.assert_not_called()

    def test_suite_stops_before_source_cut_when_source_reachability_gate_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            report_root = Path(tmp) / "reports"
            report_root.mkdir()
            bundle_root = Path(tmp) / "bundle"
            bundle_root.mkdir()
            (bundle_root / "firmware-catalog.json").write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "artifacts": [
                            {
                                "artifact_id": "artifact-12",
                                "features": ["net_http", "web_serial"],
                            }
                        ]
                    }
                ),
                encoding="utf-8",
            )
            (bundle_root / "artifact-12.manifest.json").write_text("{}", encoding="utf-8")

            argv = [
                "formal_hil_suite.py",
                "--report-root",
                str(report_root),
                "--firmware-bundle-root",
                str(bundle_root),
                "--output-profiles",
                "12v",
                "--scenes",
                "assist_path",
                "--skip-flash",
            ]

            with mock.patch.object(self.suite.sys, "argv", argv), \
                mock.patch.object(
                    self.suite,
                    "disable_load",
                    autospec=True,
                    return_value={"ok": True},
                ), \
                mock.patch.object(
                    self.suite,
                    "cut_source_power_only",
                    autospec=True,
                ) as cut_source, \
                mock.patch.object(
                    self.suite,
                    "probe_isolapurr_source_reachability",
                    autospec=True,
                    return_value={
                        "ok": False,
                        "failures": ["http_ports_unreachable", "cli_status_unreachable"],
                    },
                ):
                with self.assertRaises(SystemExit) as exc:
                    self.suite.main()

            self.assertIn("source reachability gate failed", str(exc.exception))
            cut_source.assert_not_called()

    def test_prepare_scene_source_and_capability_gate_stops_before_source_cut_when_reachability_fails(self) -> None:
        args = self.suite.argparse.Namespace(
            load_cli="/Users/ivan/.local/bin/loadlynx",
            load_device="loadlynx-d68638",
            isolapurr_cli="isolapurr",
            isolapurr_url="http://192.168.31.122",
            isolapurr_device_id="856a141cdbd4",
            status_timeout_sec=20.0,
            dry_run=False,
            devd_scan_url="http://127.0.0.1:38140/api/v1/devices/scan",
            ups_status_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/status",
            ups_settings_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/settings",
            devd_diag_snapshot_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/diag-snapshot",
            ups_device_id="serial-04f3bb3f5367",
            mains_aegis_cli="mains-aegis",
            mains_aegis_ipc=None,
            ups_observe_device_id=None,
        )
        observe_urls = {
            "ups_status_url": args.ups_status_url,
            "ups_settings_url": args.ups_settings_url,
            "devd_diag_snapshot_url": args.devd_diag_snapshot_url,
            "devd_device_trace_url": "http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/trace?trace_limit=1",
        }
        with (
            mock.patch.object(
                self.suite,
                "probe_isolapurr_source_reachability",
                autospec=True,
                return_value={"ok": False, "failures": ["http_ports_unreachable"]},
            ),
            mock.patch.object(self.suite, "cut_source_power_only", autospec=True) as cut_source,
            mock.patch.object(self.suite, "disable_load", autospec=True) as disable_load,
        ):
            result = self.suite.prepare_scene_source_and_capability_gate(
                args,
                profile_key="12v",
                observe_urls=observe_urls,
                dry_run=False,
            )
        self.assertFalse(result["ok"])
        self.assertIn("source_reachability_gate_before_scene_failed", result["failures"])
        cut_source.assert_not_called()
        disable_load.assert_not_called()

    def test_prepare_scene_source_and_capability_gate_keeps_source_disabled_for_runner_owned_enable(self) -> None:
        args = self.suite.argparse.Namespace(
            load_cli="/Users/ivan/.local/bin/loadlynx",
            load_device="loadlynx-d68638",
            isolapurr_cli="isolapurr",
            isolapurr_url="http://192.168.31.122",
            isolapurr_device_id="856a141cdbd4",
            status_timeout_sec=20.0,
            dry_run=False,
            devd_scan_url="http://127.0.0.1:38140/api/v1/devices/scan",
            ups_status_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/status",
            ups_settings_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/settings",
            devd_diag_snapshot_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/diag-snapshot",
            ups_device_id="serial-04f3bb3f5367",
            mains_aegis_cli="mains-aegis",
            mains_aegis_ipc=None,
            ups_observe_device_id=None,
        )
        observe_urls = {
            "ups_status_url": args.ups_status_url,
            "ups_settings_url": args.ups_settings_url,
            "devd_diag_snapshot_url": args.devd_diag_snapshot_url,
            "devd_device_trace_url": "http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/trace?trace_limit=1",
        }
        with (
            mock.patch.object(
                self.suite,
                "probe_isolapurr_source_reachability",
                autospec=True,
                return_value={"ok": True, "failures": []},
            ),
            mock.patch.object(self.suite, "disable_load", autospec=True, return_value={"ok": True}),
            mock.patch.object(self.suite, "cut_source_power_only", autospec=True, return_value={"ok": True}),
            mock.patch.object(self.suite, "wait_for_ups_external_input_cut", autospec=True, return_value={"ok": True, "validation": {"failures": []}}),
            mock.patch.object(self.suite, "refresh_control_devices", autospec=True, return_value={"ok": True}),
            mock.patch.object(self.suite, "http_post_json", autospec=True, return_value={"devices": []}),
            mock.patch.object(self.suite, "connect_device_with_retry", autospec=True, return_value={"ok": True}),
            mock.patch.object(self.suite, "read_device_identity", autospec=True, return_value={"result": {"hardware_capabilities": {"output_profile": "12v", "rated_vout_mv": 12000}, "network": {"ipv4": "192.168.31.232"}}}),
            mock.patch.object(self.suite, "read_device_settings", autospec=True, return_value={"result": {"advanced_power_capabilities": {"rated_vout_mv": 12000}}}),
            mock.patch.object(self.suite, "http_get_json", autospec=True, side_effect=[
                {"hardware_capabilities": {"output_profile": "12v", "rated_vout_mv": 12000}},
                {"advanced_power_capabilities": {"rated_vout_mv": 12000}},
                {"ports": [{"portId": "port_c", "state": {"power_enabled": False}, "telemetry": {"status": "not_inserted"}}]},
            ]),
            mock.patch.object(self.suite, "validate_dual_surface_hardware_capabilities", autospec=True, return_value={"ok": True, "failures": []}),
            mock.patch.object(self.suite, "configure_source_manual_output", autospec=True, return_value={"ok": True}),
            mock.patch.object(
                self.suite.runner,
                "fetch_isolapurr_power_show_best_effort",
                autospec=True,
                return_value={"source": "cli_power_show", "manual": {"voltage_mv": 12000, "current_limit_ma": 3000}},
            ),
            mock.patch.object(
                self.suite.runner,
                "validate_isolapurr_source_configuration",
                autospec=True,
                return_value={"ok": True, "failures": []},
            ),
            mock.patch.object(self.suite, "set_port_c_power_state", autospec=True, side_effect=AssertionError("suite must not enable source before runner")),
            mock.patch.object(self.suite, "wait_for_ups_external_input_restored", autospec=True, side_effect=AssertionError("suite must not wait for source restore before runner")),
        ):
            result = self.suite.prepare_scene_source_and_capability_gate(
                args,
                profile_key="12v",
                observe_urls=observe_urls,
                dry_run=False,
            )

        self.assertTrue(result["ok"])

    def test_prepare_scene_source_and_capability_gate_re_reads_usb_truth_even_when_scan_has_connected_caps(self) -> None:
        args = self.suite.argparse.Namespace(
            load_cli="/Users/ivan/.local/bin/loadlynx",
            load_device="loadlynx-d68638",
            isolapurr_cli="isolapurr",
            isolapurr_url="http://192.168.31.122",
            isolapurr_device_id="856a141cdbd4",
            status_timeout_sec=20.0,
            dry_run=False,
            devd_scan_url="http://127.0.0.1:38140/api/v1/devices/scan",
            ups_status_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/status",
            ups_settings_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/settings",
            devd_diag_snapshot_url="http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/diag-snapshot",
            ups_device_id="serial-04f3bb3f5367",
            mains_aegis_cli="mains-aegis",
            mains_aegis_ipc=None,
            ups_observe_device_id=None,
        )
        observe_urls = {
            "ups_status_url": args.ups_status_url,
            "ups_settings_url": args.ups_settings_url,
            "devd_diag_snapshot_url": args.devd_diag_snapshot_url,
            "devd_device_trace_url": "http://127.0.0.1:38140/api/v1/devices/mains-aegis-198840/trace?trace_limit=1",
        }
        scan_payload = {
            "devices": [
                {
                    "id": "serial-04f3bb3f5367",
                    "connection": "connected",
                    "identity": {
                        "device_id": "mains-aegis-198840",
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
            ]
        }
        with (
            mock.patch.object(
                self.suite,
                "probe_isolapurr_source_reachability",
                autospec=True,
                return_value={"ok": True, "failures": []},
            ),
            mock.patch.object(self.suite, "disable_load", autospec=True, return_value={"ok": True}),
            mock.patch.object(self.suite, "cut_source_power_only", autospec=True, return_value={"ok": True}),
            mock.patch.object(self.suite, "wait_for_ups_external_input_cut", autospec=True, return_value={"ok": True, "validation": {"failures": []}}),
            mock.patch.object(self.suite, "refresh_control_devices", autospec=True, return_value={"ok": True}),
            mock.patch.object(self.suite, "http_post_json", autospec=True, return_value=scan_payload),
            mock.patch.object(self.suite, "connect_device_with_retry", autospec=True, return_value={"ok": True}),
            mock.patch.object(
                self.suite,
                "read_device_identity",
                autospec=True,
                return_value={"result": {"hardware_capabilities": {"output_profile": "12v", "rated_vout_mv": 12000}, "network": {"ipv4": "192.168.31.232"}}},
            ) as read_identity_mock,
            mock.patch.object(
                self.suite,
                "read_device_settings",
                autospec=True,
                return_value={"result": {"advanced_power_capabilities": {"rated_vout_mv": 12000}}},
            ) as read_settings_mock,
            mock.patch.object(self.suite, "http_get_json", autospec=True, side_effect=[
                {"hardware_capabilities": {"output_profile": "12v", "rated_vout_mv": 12000}},
                {"advanced_power_capabilities": {"rated_vout_mv": 12000}},
                {"ports": [{"portId": "port_c", "state": {"power_enabled": False}, "telemetry": {"status": "not_inserted"}}]},
            ]),
            mock.patch.object(self.suite, "validate_dual_surface_hardware_capabilities", autospec=True, return_value={"ok": True, "failures": []}),
            mock.patch.object(self.suite, "configure_source_manual_output", autospec=True, return_value={"ok": True}),
            mock.patch.object(
                self.suite.runner,
                "fetch_isolapurr_power_show_best_effort",
                autospec=True,
                return_value={"source": "cli_power_show", "manual": {"voltage_mv": 12000, "current_limit_ma": 3000}},
            ),
            mock.patch.object(
                self.suite.runner,
                "validate_isolapurr_source_configuration",
                autospec=True,
                return_value={"ok": True, "failures": []},
            ),
        ):
            result = self.suite.prepare_scene_source_and_capability_gate(
                args,
                profile_key="12v",
                observe_urls=observe_urls,
                dry_run=False,
            )

        self.assertTrue(result["ok"])
        read_identity_mock.assert_called_once()
        read_settings_mock.assert_called_once()

    def test_probe_isolapurr_source_reachability_rejects_device_id_mismatch(self) -> None:
        with (
            mock.patch.object(
                self.suite,
                "http_get_json",
                autospec=True,
                return_value={"ports": [{"portId": "port_a"}, {"portId": "port_c"}]},
            ),
            mock.patch.object(
                self.suite,
                "run_json",
                autospec=True,
                return_value={"device": {"device_id": "wrong-cli"}},
            ),
        ):
            result = self.suite.probe_isolapurr_source_reachability(
                isolapurr_cli="isolapurr",
                isolapurr_url="http://192.168.31.122",
                timeout_sec=5.0,
                dry_run=False,
                expected_device_id="856a141cdbd4",
            )

        self.assertFalse(result["ok"])
        self.assertIn("cli_status_device_id_mismatch", result["failures"])
        self.assertEqual(result["expected_device_id"], "856a141cdbd4")

    def test_probe_isolapurr_source_reachability_rejects_missing_port_c(self) -> None:
        with (
            mock.patch.object(
                self.suite,
                "http_get_json",
                autospec=True,
                return_value={"ports": [{"portId": "port_a"}]},
            ),
            mock.patch.object(
                self.suite,
                "run_json",
                autospec=True,
                return_value={"device": {"device_id": "856a141cdbd4"}},
            ),
        ):
            result = self.suite.probe_isolapurr_source_reachability(
                isolapurr_cli="isolapurr",
                isolapurr_url="http://192.168.31.122",
                timeout_sec=5.0,
                dry_run=False,
                expected_device_id="856a141cdbd4",
            )

        self.assertFalse(result["ok"])
        self.assertIn("http_port_c_missing", result["failures"])

    def test_suite_stops_before_flash_when_ups_cut_gate_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            report_root = Path(tmp) / "reports"
            report_root.mkdir()
            bundle_root = Path(tmp) / "bundle"
            bundle_root.mkdir()
            (bundle_root / "firmware-catalog.json").write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "artifacts": [
                            {
                                "artifact_id": "artifact-12",
                                "features": ["net_http", "web_serial"],
                            }
                        ]
                    }
                ),
                encoding="utf-8",
            )
            (bundle_root / "artifact-12.manifest.json").write_text("{}", encoding="utf-8")

            argv = [
                "formal_hil_suite.py",
                "--report-root",
                str(report_root),
                "--firmware-bundle-root",
                str(bundle_root),
                "--output-profiles",
                "12v",
                "--scenes",
                "backup_only",
                "--skip-flash",
            ]

            with mock.patch.object(self.suite.sys, "argv", argv), \
                mock.patch.object(
                    self.suite,
                    "probe_isolapurr_source_reachability",
                    autospec=True,
                    return_value={"ok": True, "failures": []},
                ), \
                mock.patch.object(self.suite, "disable_load", autospec=True, return_value={"ok": True}), \
                mock.patch.object(self.suite, "cut_source_power_only", autospec=True, return_value={"enabled": False, "actions": []}), \
                mock.patch.object(
                    self.suite,
                    "refresh_control_devices",
                    autospec=True,
                    return_value={"dry_run": False, "devices": []},
                ), \
                mock.patch.object(
                    self.suite,
                    "connect_device_with_retry",
                    autospec=True,
                    return_value={"ok": True},
                ), \
                mock.patch.object(
                    self.suite,
                    "read_selected_artifact",
                    autospec=True,
                    return_value={"result": {"artifact_id": "artifact-12"}},
                ), \
                mock.patch.object(
                    self.suite,
                    "read_device_identity",
                    autospec=True,
                    return_value={
                        "result": {
                            "network": {"ipv4": "192.168.31.232"},
                            "hardware_capabilities": {
                                "output_profile": "12v",
                                "rated_vout_mv": 12000,
                            },
                        }
                    },
                ), \
                mock.patch.object(
                    self.suite,
                    "read_device_settings",
                    autospec=True,
                    return_value={
                        "result": {
                            "advanced_power_capabilities": {
                                "rated_vout_mv": 12000,
                            }
                        }
                    },
                ), \
                mock.patch.object(
                    self.suite,
                    "http_post_json",
                    autospec=True,
                    return_value={
                        "devices": [
                            {
                                "id": "serial-04f3bb3f5367",
                                "connection": "connected",
                                "identity": {
                                    "network": {"ipv4": "192.168.31.232"},
                                    "hardware_capabilities": {
                                        "output_profile": "12v",
                                        "rated_vout_mv": 12000,
                                    },
                                },
                                "settings": {
                                    "advanced_power_capabilities": {
                                        "rated_vout_mv": 12000,
                                    }
                                },
                            }
                        ]
                    },
                ), \
                mock.patch.object(
                    self.suite,
                    "http_get_json",
                    autospec=True,
                    return_value={
                        "mode": "standby",
                        "input": {
                            "source": "dcin",
                            "mains_present": True,
                            "input_vbus_mv": 5103,
                            "vin_vbus_mv": 12016,
                            "assist_power_stage": "standby",
                        },
                    },
                ), \
                mock.patch.object(self.suite, "select_and_flash_artifact", autospec=True) as flash_artifact:
                with self.assertRaises(SystemExit) as exc:
                    self.suite.main()

            self.assertIn("UPS input-cut gate failed", str(exc.exception))
            flash_artifact.assert_not_called()

    def test_suite_dry_run_expands_four_required_scenes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            report_root = Path(tmp) / "reports"
            report_root.mkdir()
            manifest_12 = Path(tmp) / "artifact-12.manifest.json"
            manifest_19 = Path(tmp) / "artifact-19.manifest.json"
            manifest_12.write_text("{}", encoding="utf-8")
            manifest_19.write_text("{}", encoding="utf-8")

            argv = [
                "formal_hil_suite.py",
                "--suite-id",
                "dry-run-suite",
                "--report-root",
                str(report_root),
                "--artifact-manifest-12v",
                str(manifest_12),
                "--artifact-manifest-19v",
                str(manifest_19),
                "--output-profiles",
                "12v",
                "19v",
                "--scenes",
                "assist_path",
                "backup_only",
                "--dry-run",
                "--skip-verify",
                "--skip-overview",
            ]

            with mock.patch.object(self.suite.sys, "argv", argv):
                rc = self.suite.main()

            self.assertEqual(rc, 0)
            summary = json.loads(
                (report_root / "dry-run-suite-summary.json").read_text(encoding="utf-8")
            )
            self.assertEqual(summary["suite_id"], "dry-run-suite")
            self.assertEqual(set(summary["profiles"].keys()), {"12v", "19v"})
            run_actions = [
                action["run_scene"]
                for action in summary["actions"]
                if "run_scene" in action
            ]
            observed_pairs = {
                (
                    action["profile"],
                    action["scene"],
                )
                for action in run_actions
            }
            self.assertEqual(
                observed_pairs,
                {
                    ("12v", "assist_path"),
                    ("12v", "backup_only"),
                    ("19v", "assist_path"),
                    ("19v", "backup_only"),
                },
            )
            self.assertTrue(all(action.get("dry_run") is True for action in run_actions))
            self.assertEqual(summary["reports"], [])


if __name__ == "__main__":
    unittest.main()
