#!/usr/bin/env python3
from __future__ import annotations

import argparse
import contextlib
import importlib.util
import io
import tempfile
import unittest
from pathlib import Path


def load_module():
    path = Path(__file__).with_name("diag_snapshot_readonly.py")
    spec = importlib.util.spec_from_file_location("diag_snapshot_readonly", path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


class DiagSnapshotReadonlyTests(unittest.TestCase):
    def setUp(self) -> None:
        self.mod = load_module()

    def test_build_url_is_get_diag_snapshot_only(self) -> None:
        url = self.mod.build_diag_snapshot_url(
            "http://127.0.0.1:38140/",
            "fixture ups/device",
            ["bq40.manufacturing", "derived.power"],
        )

        self.assertTrue(url.startswith("http://127.0.0.1:38140/api/v1/devices/fixture%20ups%2Fdevice/diag-snapshot?"))
        self.assertIn("fresh=true", url)
        self.assertIn("include_meta=true", url)
        self.assertIn("package=bq40.manufacturing", url)
        self.assertIn("package=derived.power", url)
        for forbidden in self.mod.FORBIDDEN_PATH_PARTS:
            self.assertNotIn(forbidden, url)

    def test_rejects_non_diag_snapshot_paths(self) -> None:
        with self.assertRaises(ValueError):
            self.mod.assert_read_only_url("http://127.0.0.1:38140/api/v1/devices/fixture/flash")
        with self.assertRaises(ValueError):
            self.mod.assert_read_only_url("http://127.0.0.1:38140/api/v1/devices/fixture/monitor/start")

    def test_validate_accepts_required_packages(self) -> None:
        response = {
            "sample": {
                "packages": {
                    "bq40.manufacturing": {
                        "payload": {
                            "manufacturing_status": 0,
                            "fet_en": True,
                            "chg_en": True,
                            "dsg_en": True,
                            "safety_status": 0,
                            "pf_status": 0,
                            "charging_status": 0,
                            "gauging_status": 0,
                            "op_status_raw_len": 4,
                            "op_status_raw_bytes": [1, 2, 3, 4],
                        }
                    },
                    "bq25792.regs": {"payload": {"reg08": 0}},
                    "derived.power": {
                        "payload": {
                            "charger": {},
                            "bms": {},
                            "policy": {},
                        }
                    },
                }
            }
        }

        self.assertEqual(
            self.mod.validate_response(
                response,
                ["bq40.manufacturing", "bq25792.regs", "derived.power"],
            ),
            [],
        )

    def test_validate_reports_missing_bq40_raw_fields(self) -> None:
        response = {
            "sample": {
                "packages": {
                    "bq40.manufacturing": {
                        "payload": {
                            "manufacturing_status": 0,
                            "fet_en": True,
                            "chg_en": True,
                            "dsg_en": True,
                        }
                    }
                }
            }
        }

        failures = self.mod.validate_response(response, ["bq40.manufacturing"])

        self.assertIn("bq40.manufacturing.op_status_raw_bytes missing", failures)
        self.assertIn("bq40.manufacturing.safety_status missing", failures)

    def test_run_writes_failure_without_state_changing_urls(self) -> None:
        args = argparse.Namespace(
            devd_url="http://127.0.0.1:38140",
            device_id="fixture-ups-device",
            packages=["bq40.manufacturing"],
            timeout_sec=0.01,
            retries=1,
            out=None,
        )
        captured_urls: list[str] = []

        def fake_fetch(url: str, _timeout: float):
            captured_urls.append(url)
            return {"sample": {"packages": {}}}

        self.mod.fetch_json = fake_fetch

        with contextlib.redirect_stdout(io.StringIO()):
            rc = self.mod.run(args)

        self.assertEqual(rc, 1)
        self.assertEqual(len(captured_urls), 1)
        self.assertIn("/diag-snapshot?", captured_urls[0])
        for forbidden in self.mod.FORBIDDEN_PATH_PARTS:
            self.assertNotIn(forbidden, captured_urls[0])

    def test_run_writes_success_report(self) -> None:
        response = {
            "sample": {
                "packages": {
                    "derived.power": {
                        "payload": {
                            "charger": {},
                            "bms": {},
                            "policy": {},
                        }
                    }
                }
            }
        }
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "diag.json"
            args = argparse.Namespace(
                devd_url="http://127.0.0.1:38140",
                device_id="fixture-ups-device",
                packages=["derived.power"],
                timeout_sec=1,
                retries=1,
                out=str(out),
            )
            self.mod.fetch_json = lambda _url, _timeout: response

            with contextlib.redirect_stdout(io.StringIO()):
                rc = self.mod.run(args)

            self.assertEqual(rc, 0)
            self.assertIn('"ok": true', out.read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()
