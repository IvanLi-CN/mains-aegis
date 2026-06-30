#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import argparse
import tempfile
import unittest
from pathlib import Path


def load_module(filename: str, module_name: str):
    path = Path(__file__).with_name(filename)
    spec = importlib.util.spec_from_file_location(module_name, path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


class FormalHilCliSuiteTests(unittest.TestCase):
    def setUp(self) -> None:
        self.suite = load_module("formal_hil_cli_suite.py", "formal_hil_cli_suite")

    def args(self) -> argparse.Namespace:
        return argparse.Namespace(
            ups_cli="mains-aegis",
            ups_ipc="/tmp/mains-aegis.sock",
            ups_device_id="serial-04f3bb3f5367",
            artifact_manifest_12v="/tmp/12v.manifest.json",
            artifact_manifest_19v="/tmp/19v.manifest.json",
            load_cli="loadlynx",
            load_ipc="/tmp/loadlynx.sock",
            load_device="loadlynx-d68638",
            isolapurr_cli="isolapurr",
            isolapurr_device_id="856a141cdbd4",
            load_min_v_mv=3000,
            load_max_i_ma_total=4000,
            load_max_p_mw=80000,
        )

    def test_jsonl_collector_accepts_pretty_summary_after_samples(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            script = Path(tmp) / "stream.py"
            script.write_text(
                "\n".join(
                    [
                        "import json",
                        "print(json.dumps({'seq': 1, 'received_at_ms': 1000}))",
                        "print(json.dumps({'seq': 2, 'received_at_ms': 1250}))",
                        "print('{')",
                        "print('  \"ok\": true,')",
                        "print('  \"samples\": 2')",
                        "print('}')",
                    ]
                ),
                encoding="utf-8",
            )
            collector = self.suite.JsonlCollector(
                "load",
                ["python3", str(script)],
                Path(tmp),
            )
            collector.start()
            assert collector.proc is not None
            collector.proc.wait(timeout=5)
            collector.stop()

            self.assertEqual([row["seq"] for row in collector.rows], [1, 2])
            self.assertEqual(collector.summary, {"ok": True, "samples": 2})
            self.assertEqual(collector.errors, [])

    def test_isolapurr_profile_config_keeps_output_disconnected(self) -> None:
        args = self.args()
        profile = {"source_voltage_mv": 19000, "source_current_limit_ma": 3000}

        result = self.suite.configure_isolapurr_manual(args, profile, dry_run=True)

        self.assertEqual(
            result["cmd"],
            [
                "isolapurr",
                "--json",
                "power",
                "config",
                "set",
                "--device-id",
                "856a141cdbd4",
                "--tps-mode",
                "manual",
                "--voltage-mv",
                "19000",
                "--current-limit-ma",
                "3000",
                "--usb-c-path",
                "disconnected",
            ],
        )

    def test_isolapurr_profile_config_forces_output_off_after_config(self) -> None:
        args = self.args()
        profile = {"source_voltage_mv": 19000, "source_current_limit_ma": 3000}
        calls: list[tuple[bool, bool]] = []

        def fake_run_json(cmd, **_kwargs):
            return {"ok": True, "cmd": cmd}

        def fake_set_output(fake_args, enabled: bool, *, dry_run: bool):
            self.assertIs(fake_args, args)
            calls.append((enabled, dry_run))
            return {"cmd": ["isolapurr", "--json", "power", "output", "auto"], "result": {"ok": True}}

        self.suite.run_json = fake_run_json
        self.suite.set_isolapurr_output = fake_set_output
        self.suite.wait_isolapurr_output_state = lambda _args, enabled: {"ok": not enabled}

        result = self.suite.configure_isolapurr_manual(args, profile, dry_run=False)

        self.assertEqual(calls, [(False, False)])
        self.assertEqual(result["source_off"]["result"], {"ok": True})

    def test_isolapurr_output_off_preserves_known_manual_target(self) -> None:
        args = self.args()
        args._active_source_voltage_mv = 12000
        args._active_source_current_limit_ma = 3000

        result = self.suite.set_isolapurr_output(args, False, dry_run=True)

        self.assertEqual(
            result["cmd"],
            [
                "isolapurr",
                "--json",
                "power",
                "output",
                "auto",
                "--device-id",
                "856a141cdbd4",
            ],
        )

    def test_isolapurr_output_on_uses_forced_on_with_profile_target(self) -> None:
        args = self.args()
        args._active_source_voltage_mv = 19000
        args._active_source_current_limit_ma = 3000

        result = self.suite.set_isolapurr_output(args, True, dry_run=True)

        self.assertEqual(result["cmd"][0:5], ["isolapurr", "--json", "power", "output", "manual"])
        self.assertEqual(result["cmd"][result["cmd"].index("--usb-c-path") + 1], "forced-on")
        self.assertEqual(result["cmd"][result["cmd"].index("--voltage-mv") + 1], "19000")
        self.assertEqual(result["cmd"][result["cmd"].index("--current-limit-ma") + 1], "3000")

    def test_load_commands_use_usb_cli_and_protection_rails(self) -> None:
        args = self.args()

        disable = self.suite.ensure_load_disabled(args, dry_run=True)
        cc = self.suite.set_load_cc(args, 3900, dry_run=True)

        self.assertEqual(
            disable["cmd"],
            [
                "loadlynx",
                "--ipc",
                "/tmp/loadlynx.sock",
                "control",
                "set",
                "--device",
                "loadlynx-d68638",
                "--disable",
                "--json",
            ],
        )
        self.assertEqual(cc["cmd"][0:5], ["loadlynx", "--ipc", "/tmp/loadlynx.sock", "--json", "cc"])
        self.assertEqual(cc["cmd"][cc["cmd"].index("--min-v-mv") + 1], "3000")
        self.assertEqual(cc["cmd"][cc["cmd"].index("--max-i-ma-total") + 1], "4000")
        self.assertEqual(cc["cmd"][cc["cmd"].index("--max-p-mw") + 1], "80000")

    def test_ups_watch_row_unwraps_sample_payload(self) -> None:
        row = {
            "sample_received_at_ms": 1000,
            "sample": {
                "mode": "standby",
                "input": {"vin_vbus_mv": 19000},
            },
            "meta": {"transport": "usb"},
        }

        sample = self.suite.unwrap_ups_sample(row)

        self.assertEqual(sample["mode"], "standby")
        self.assertEqual(sample["input"]["vin_vbus_mv"], 19000)

    def test_start_collectors_uses_ups_watch_commands(self) -> None:
        args = self.args()
        args.sample_interval_s = 0.2

        collectors = self.suite.start_collectors.__globals__
        status_cmd = self.suite.JsonlCollector(
            "ups_status",
            self.suite.ups_cmd(
                args,
                "status",
                "--watch",
                "--interval-ms",
                "200",
                "--watch-freshness-ms",
                str(self.suite.UPS_WATCH_FRESHNESS_MS),
                "--include-meta",
            ),
            Path("."),
        ).cmd

        self.assertIn("--watch", status_cmd)
        self.assertIn("--include-meta", status_cmd)
        self.assertNotIn("--cache-only", status_cmd)
        self.assertIsInstance(collectors, dict)

    def test_ups_commands_use_cli_ipc_device_target(self) -> None:
        args = self.args()

        identity, settings = self.suite.read_ups_identity_settings(args, dry_run=True)

        self.assertEqual(identity["cmd"], ["mains-aegis", "--ipc", "/tmp/mains-aegis.sock", "device", "serial-04f3bb3f5367", "identity"])
        self.assertEqual(settings["cmd"], ["mains-aegis", "--ipc", "/tmp/mains-aegis.sock", "device", "serial-04f3bb3f5367", "settings"])

    def test_artifact_select_and_flash_use_devd_cli(self) -> None:
        args = self.args()

        select = self.suite.select_ups_artifact(args, "/tmp/19v.manifest.json", dry_run=True)
        dry_flash = self.suite.flash_ups_artifact(args, real=False, dry_run=True)
        real_flash = self.suite.flash_ups_artifact(args, real=True, dry_run=True)

        self.assertEqual(
            select["cmd"],
            [
                "mains-aegis",
                "--ipc",
                "/tmp/mains-aegis.sock",
                "device",
                "serial-04f3bb3f5367",
                "artifact",
                "select",
                "--manifest-path",
                "/tmp/19v.manifest.json",
            ],
        )
        self.assertEqual(dry_flash["cmd"], ["mains-aegis", "--ipc", "/tmp/mains-aegis.sock", "device", "serial-04f3bb3f5367", "flash", "--dry-run"])
        self.assertEqual(real_flash["cmd"], ["mains-aegis", "--ipc", "/tmp/mains-aegis.sock", "device", "serial-04f3bb3f5367", "flash", "--real"])

    def test_usb_5v_input_does_not_fail_dcin_cut_gate(self) -> None:
        verdict = self.suite.validate_ups_input_cut(
            {
                "mode": "standby",
                "input": {
                    "source": "usbc",
                    "mains_present": True,
                    "vin_vbus_mv": 5100,
                },
            }
        )

        self.assertEqual(verdict["failures"], [])
        self.assertTrue(verdict["ok"])

    def test_live_dcin_voltage_fails_cut_gate(self) -> None:
        verdict = self.suite.validate_ups_input_cut(
            {
                "mode": "standby",
                "input": {
                    "source": "dcin",
                    "mains_present": True,
                    "vin_vbus_mv": 12000,
                },
            }
        )

        self.assertIn("ups_dcin_still_powered", verdict["failures"])
        self.assertFalse(verdict["ok"])


if __name__ == "__main__":
    unittest.main()
