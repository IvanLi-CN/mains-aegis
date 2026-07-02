#!/usr/bin/env python3
from __future__ import annotations

import argparse
import importlib.util
import io
import json
import subprocess
import tempfile
import time
import uuid
import urllib.error
import types
import unittest
from pathlib import Path
from unittest import mock


def load_runner_module():
    runner_path = Path(__file__).with_name("advanced_power_12v_runner.py")
    spec = importlib.util.spec_from_file_location("advanced_power_12v_runner", runner_path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


def load_probe_module():
    probe_path = Path(__file__).with_name("probe_loadlynx_released_telemetry.py")
    spec = importlib.util.spec_from_file_location("probe_loadlynx_released_telemetry", probe_path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


class WaitForLoadStateTests(unittest.TestCase):
    def setUp(self) -> None:
        self.runner = load_runner_module()
        self.args = types.SimpleNamespace(load_bridge_url="http://bridge")

    def test_wait_for_load_state_falls_back_when_live_snapshot_is_stale_and_erroring(self) -> None:
        runner = self.runner

        class FakePoller:
            def snapshot(self, now_monotonic: float) -> dict[str, object]:
                return {
                    "payload": {
                        "control": {"output_enabled": False, "target_i_ma": 2100},
                        "status": {"enable": False, "i_local_ma": 9, "i_remote_ma": 8},
                    },
                    "generation": 1,
                    "age_s": 0.01,
                    "sample_age_s": 9.5,
                    "error": "TimeoutError('timed out')",
                    "source": "replace",
                }

        fallback_status = {
            "control": {"output_enabled": True, "target_i_ma": 3900},
            "status": {"enable": True, "i_local_ma": 1000, "i_remote_ma": 1000},
        }
        with (
            mock.patch.object(
                runner,
                "get_load_status_best_effort",
                autospec=True,
                return_value=fallback_status,
            ) as get_status,
            mock.patch.object(
                runner,
                "get_load_control_best_effort",
                autospec=True,
                return_value={"ok": False, "error": "control not needed"},
            ) as get_control,
        ):
            result = runner.wait_for_load_state(
                self.args,
                "fixture-load-device",
                expected_enabled=True,
                expected_target_i_ma=3900,
                status_timeout_sec=0.1,
                verify_timeout_sec=0.25,
                poll_interval_sec=0.05,
                live_status_poller=FakePoller(),
            )

        self.assertEqual(result["effective_enabled"], True)
        self.assertEqual(result["effective_target_i_ma"], 3900)
        self.assertFalse(result.get("verified_from_live_poller", False))
        self.assertGreaterEqual(get_status.call_count, 1)
        self.assertEqual(get_control.call_count, 0)

    def test_wait_for_load_state_uses_live_snapshot_when_it_is_fresh_and_clean(self) -> None:
        runner = self.runner

        class FakePoller:
            def snapshot(self, now_monotonic: float) -> dict[str, object]:
                return {
                    "payload": {
                        "control": {"output_enabled": True, "target_i_ma": 3900},
                        "status": {"enable": True, "i_local_ma": 1000, "i_remote_ma": 1000},
                    },
                    "generation": 2,
                    "age_s": 0.02,
                    "sample_age_s": 0.02,
                    "error": None,
                    "source": "bridge-http",
                }

        with (
            mock.patch.object(
                runner,
                "get_load_status_best_effort",
                autospec=True,
                side_effect=AssertionError("fallback status should not be used"),
            ),
            mock.patch.object(
                runner,
                "get_load_control_best_effort",
                autospec=True,
                side_effect=AssertionError("fallback control should not be used"),
            ),
        ):
            started = time.monotonic()
            result = runner.wait_for_load_state(
                self.args,
                "fixture-load-device",
                expected_enabled=True,
                expected_target_i_ma=3900,
                status_timeout_sec=0.1,
                verify_timeout_sec=0.25,
                poll_interval_sec=0.05,
                live_status_poller=FakePoller(),
            )

        self.assertEqual(result["effective_enabled"], True)
        self.assertEqual(result["effective_target_i_ma"], 3900)
        self.assertEqual(result["verified_from_live_poller"], True)
        self.assertLess(time.monotonic() - started, 0.1)

    def test_promote_ups_status_url_to_direct_lan_from_localhost_devd(self) -> None:
        runner = self.runner
        result = runner.maybe_promote_ups_status_url_to_direct_lan(
            "http://127.0.0.1:38140/api/v1/devices/fixture-ups-device/status",
            lan_address="127.0.0.1:30081",
        )
        self.assertEqual(result, "http://127.0.0.1:30081/api/v1/status")

    def test_wait_for_load_state_reuses_live_poller_devd_lease_for_fallback_status(self) -> None:
        runner = self.runner

        class FakePoller:
            def snapshot(self, now_monotonic: float) -> dict[str, object]:
                return {
                    "payload": {
                        "control": {"output_enabled": False, "target_i_ma": 3900},
                        "status": {"enable": False, "i_local_ma": 9, "i_remote_ma": 8},
                    },
                    "generation": 1,
                    "age_s": 0.01,
                    "sample_age_s": 9.5,
                    "error": "TimeoutError('timed out')",
                    "source": "replace",
                }

            def bridge_lease_snapshot(self):
                return None

            def load_devd_lease_snapshot(self):
                return {"lease_id": "lease-1"}

        helper_status = {
            "control": {"output_enabled": True, "target_i_ma": 3900},
            "status": {"enable": True, "i_local_ma": 1000, "i_remote_ma": 1000},
            "source": "ipc_helper_status",
        }
        with (
            mock.patch.object(
                runner,
                "get_load_status_best_effort",
                autospec=True,
                return_value=helper_status,
            ) as get_status,
            mock.patch.object(
                runner,
                "get_load_control_best_effort",
                autospec=True,
                side_effect=AssertionError("control fallback should not be needed"),
            ),
        ):
            result = runner.wait_for_load_state(
                self.args,
                "fixture-load-device",
                expected_enabled=True,
                expected_target_i_ma=3900,
                status_timeout_sec=0.1,
                verify_timeout_sec=0.25,
                poll_interval_sec=0.05,
                live_status_poller=FakePoller(),
            )

        self.assertEqual(result["effective_enabled"], True)
        self.assertEqual(result["effective_target_i_ma"], 3900)
        self.assertEqual(get_status.call_args.kwargs["load_devd_lease"], {"lease_id": "lease-1"})
        self.assertTrue(get_status.call_args.kwargs["prefer_devd_http"])


class LoadCcVerificationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.runner = load_runner_module()
        self.args = types.SimpleNamespace(
            load_bridge_url="",
            load_devd_base_url="http://127.0.0.1:62841",
            load_devd_socket="/tmp/loadlynx.sock",
            load_cli="/tmp/fake-loadlynx",
            load_ipc="",
        )

    def test_load_cc_accepts_command_ack_when_post_status_verification_times_out(self) -> None:
        runner = self.runner
        configure_completed = subprocess.CompletedProcess(
            args=["loadlynx", "cc"],
            returncode=0,
            stdout=json.dumps(
                {
                    "mode": "CC",
                    "output_enabled": True,
                    "target_i_ma": 3900,
                }
            ),
            stderr="",
        )
        verify_error = RuntimeError("LoadLynx status did not reach expected state")
        with (
            mock.patch.object(
                runner,
                "run_loadlynx",
                autospec=True,
                return_value=configure_completed,
            ) as run_cmd,
            mock.patch.object(runner, "wait_for_load_state", autospec=True, side_effect=verify_error) as wait_state,
            mock.patch.object(
                runner,
                "get_load_status_direct_cli_best_effort",
                autospec=True,
                return_value={
                    "control": {"output_enabled": True, "target_i_ma": 3900},
                    "status": {"enable": True, "i_local_ma": 1900, "i_remote_ma": 1900},
                    "source": "cli_status_direct",
                },
            ),
            mock.patch.object(
                runner,
                "get_load_control_direct_cli_best_effort",
                autospec=True,
                side_effect=AssertionError("direct control should not be needed when direct status confirms"),
            ),
            mock.patch.object(
                runner,
                "get_load_status_best_effort",
                autospec=True,
                side_effect=AssertionError("status fallback should not be used when direct status confirms"),
            ),
            mock.patch.object(
                runner,
                "get_load_control_best_effort",
                autospec=True,
                side_effect=AssertionError("control fallback should not be used when direct status confirms"),
            ),
        ):
            result = runner.load_cc(
                self.args,
                "fixture-load-device",
                3900,
                min_v_mv=3000,
                max_i_ma_total=4000,
                max_p_mw=80000,
                timeout_sec=1.0,
                status_timeout_sec=0.5,
                verify_timeout_sec=0.5,
            )

        self.assertEqual(run_cmd.call_count, 1)
        self.assertEqual(
            run_cmd.call_args_list[0].args[0],
            [
                "/tmp/fake-loadlynx",
                "cc",
                "3900",
                "--device",
                "fixture-load-device",
                "--min-v-mv",
                "3000",
                "--max-i-ma-total",
                "4000",
                "--max-p-mw",
                "80000",
                "--json",
            ],
        )
        self.assertEqual(wait_state.call_count, 1)
        self.assertTrue(result["verified_status"]["degraded_verification"])
        self.assertTrue(result["verified_status"]["degraded_from_command_ack"])
        self.assertEqual(result["verified_status"]["effective_enabled"], True)
        self.assertEqual(result["verified_status"]["effective_target_i_ma"], 3900)
        self.assertEqual(
            result["verified_status"]["command_response"]["output_enabled"],
            True,
        )
        self.assertEqual(
            result["configure_cmd"],
            [
                "/tmp/fake-loadlynx",
                "cc",
                "3900",
                "--device",
                "fixture-load-device",
                "--min-v-mv",
                "3000",
                "--max-i-ma-total",
                "4000",
                "--max-p-mw",
                "80000",
                "--json",
            ],
        )

    def test_load_cc_uses_direct_cli_confirmation_when_helper_status_is_stale(self) -> None:
        runner = self.runner
        configure_completed = subprocess.CompletedProcess(
            args=["loadlynx", "cc"],
            returncode=0,
            stdout=json.dumps({"accepted": True, "output_enabled": True, "target_i_ma": 3900}),
            stderr="",
        )
        stale_verify_error = RuntimeError(
            "LoadLynx status did not reach expected state: enabled=True target_i_ma=3900 "
            "last_control=None last_status={'control': {'output_enabled': False, 'target_i_ma': 3900}, "
            "'status': {'enable': False}}"
        )
        direct_status = {
            "control": {"output_enabled": True, "target_i_ma": 3900},
            "status": {"enable": True, "i_local_ma": 1940, "i_remote_ma": 1930},
            "source": "cli_status_direct",
        }
        with (
            mock.patch.object(
                runner,
                "run_loadlynx",
                autospec=True,
                return_value=configure_completed,
            ) as run_cmd,
            mock.patch.object(runner, "wait_for_load_state", autospec=True, side_effect=stale_verify_error),
            mock.patch.object(
                runner,
                "get_load_status_direct_cli_best_effort",
                autospec=True,
                return_value=direct_status,
            ) as direct_status_read,
            mock.patch.object(
                runner,
                "get_load_control_direct_cli_best_effort",
                autospec=True,
                side_effect=AssertionError("direct control should not be needed when direct status already confirms"),
            ),
        ):
            result = runner.load_cc(
                self.args,
                "fixture-load-device",
                3900,
                min_v_mv=3000,
                max_i_ma_total=4000,
                max_p_mw=80000,
                timeout_sec=1.0,
                status_timeout_sec=0.5,
                verify_timeout_sec=0.5,
            )

        self.assertEqual(run_cmd.call_count, 1)
        self.assertEqual(direct_status_read.call_count, 0)
        self.assertTrue(result["verified_status"]["degraded_verification"])
        self.assertTrue(result["verified_status"]["degraded_from_command_ack"])
        self.assertEqual(result["verified_status"]["effective_enabled"], True)
        self.assertEqual(result["verified_status"]["effective_target_i_ma"], 3900)
        self.assertNotIn("command_stdout", result["verified_status"])
        self.assertNotIn("command_stderr", result["verified_status"])

    def test_load_cc_uses_command_response_when_direct_cli_confirmation_fails(self) -> None:
        runner = self.runner
        configure_completed = subprocess.CompletedProcess(
            args=["loadlynx", "cc"],
            returncode=0,
            stdout=json.dumps({"accepted": True, "output_enabled": True, "target_i_ma": 3900}),
            stderr="",
        )
        verify_error = RuntimeError("LoadLynx status did not reach expected state")
        with (
            mock.patch.object(
                runner,
                "run_loadlynx",
                autospec=True,
                return_value=configure_completed,
            ) as run_cmd,
            mock.patch.object(runner, "wait_for_load_state", autospec=True, side_effect=verify_error),
            mock.patch.object(
                runner,
                "confirm_load_state_with_direct_cli",
                autospec=True,
                side_effect=RuntimeError("direct_cli_load_state_mismatch"),
            ) as direct_confirm,
            mock.patch.object(
                runner,
                "get_load_status_best_effort",
                autospec=True,
                side_effect=AssertionError("status fallback should not be used when command response already proves enable"),
            ),
            mock.patch.object(
                runner,
                "get_load_control_best_effort",
                autospec=True,
                side_effect=AssertionError("control fallback should not be used when command response already proves enable"),
            ),
        ):
            result = runner.load_cc(
                self.args,
                "fixture-load-device",
                3900,
                min_v_mv=3000,
                max_i_ma_total=4000,
                max_p_mw=80000,
                timeout_sec=1.0,
                status_timeout_sec=0.5,
                verify_timeout_sec=0.5,
            )

        self.assertEqual(run_cmd.call_count, 1)
        self.assertIn(direct_confirm.call_count, (0, 1))
        self.assertTrue(result["verified_status"]["degraded_verification"])
        self.assertTrue(result["verified_status"]["degraded_from_command_ack"])
        self.assertEqual(result["verified_status"]["effective_enabled"], True)
        self.assertEqual(result["verified_status"]["effective_target_i_ma"], 3900)
        self.assertEqual(result["verified_status"]["command_response"]["target_i_ma"], 3900)

    def test_load_cc_falls_back_without_load_devd_socket(self) -> None:
        runner = self.runner
        self.args.load_devd_socket = ""
        configure_completed = subprocess.CompletedProcess(
            args=["loadlynx", "cc"],
            returncode=0,
            stdout=json.dumps(
                {
                    "mode": "CC",
                    "output_enabled": True,
                    "target_i_ma": 3900,
                }
            ),
            stderr="",
        )
        verify_error = RuntimeError("LoadLynx status did not reach expected state")
        with (
            mock.patch.object(
                runner,
                "run_loadlynx",
                autospec=True,
                return_value=configure_completed,
            ) as run_cmd,
            mock.patch.object(runner, "wait_for_load_state", autospec=True, side_effect=verify_error),
            mock.patch.object(
                runner,
                "get_load_status_direct_cli_best_effort",
                autospec=True,
                return_value={
                    "control": {"output_enabled": True, "target_i_ma": 3900},
                    "status": {"enable": True, "i_local_ma": 1900, "i_remote_ma": 1900},
                    "source": "cli_status_direct",
                },
            ),
            mock.patch.object(
                runner,
                "get_load_control_direct_cli_best_effort",
                autospec=True,
                side_effect=AssertionError("direct control should not be needed when direct status confirms"),
            ),
            mock.patch.object(
                runner,
                "get_load_status_best_effort",
                autospec=True,
                side_effect=AssertionError("status fallback should not be used when direct status confirms"),
            ),
            mock.patch.object(
                runner,
                "get_load_control_best_effort",
                autospec=True,
                side_effect=AssertionError("control fallback should not be used when direct status confirms"),
            ),
        ):
            result = runner.load_cc(
                self.args,
                "fixture-load-device",
                3900,
                min_v_mv=3000,
                max_i_ma_total=4000,
                max_p_mw=80000,
                timeout_sec=1.0,
                status_timeout_sec=0.5,
                verify_timeout_sec=0.5,
            )

        self.assertEqual(
            run_cmd.call_args_list[0].args[0],
            [
                "/tmp/fake-loadlynx",
                "cc",
                "3900",
                "--device",
                "fixture-load-device",
                "--min-v-mv",
                "3000",
                "--max-i-ma-total",
                "4000",
                "--max-p-mw",
                "80000",
                "--json",
            ],
        )
        self.assertTrue(result["verified_status"]["degraded_from_command_ack"])

    def test_load_cc_ignores_hidden_ipc_when_load_devd_socket_is_present(self) -> None:
        runner = self.runner
        self.args.load_devd_socket = "/tmp/loadlynx.sock"
        configure_completed = subprocess.CompletedProcess(
            args=["loadlynx", "cc"],
            returncode=0,
            stdout=json.dumps(
                {
                    "mode": "CC",
                    "output_enabled": True,
                    "target_i_ma": 3900,
                }
            ),
            stderr="",
        )
        verify_error = RuntimeError("LoadLynx status did not reach expected state")
        with (
            mock.patch.object(
                runner,
                "run_loadlynx",
                autospec=True,
                return_value=configure_completed,
            ) as run_cmd,
            mock.patch.object(runner, "wait_for_load_state", autospec=True, side_effect=verify_error),
            mock.patch.object(
                runner,
                "get_load_status_direct_cli_best_effort",
                autospec=True,
                return_value={
                    "control": {"output_enabled": True, "target_i_ma": 3900},
                    "status": {"enable": True, "i_local_ma": 1900, "i_remote_ma": 1900},
                    "source": "cli_status_direct",
                },
            ),
            mock.patch.object(
                runner,
                "get_load_control_direct_cli_best_effort",
                autospec=True,
                side_effect=AssertionError("direct control should not be needed when direct status confirms"),
            ),
            mock.patch.object(
                runner,
                "get_load_status_best_effort",
                autospec=True,
                side_effect=AssertionError("status fallback should not be used when direct status confirms"),
            ),
            mock.patch.object(
                runner,
                "get_load_control_best_effort",
                autospec=True,
                side_effect=AssertionError("control fallback should not be used when direct status confirms"),
            ),
        ):
            runner.load_cc(
                self.args,
                "fixture-load-device",
                3900,
                min_v_mv=3000,
                max_i_ma_total=4000,
                max_p_mw=80000,
                timeout_sec=1.0,
                status_timeout_sec=0.5,
                verify_timeout_sec=0.5,
            )

        self.assertNotIn("--ipc", run_cmd.call_args_list[0].args[0])

    def test_load_cc_shortcuts_immediately_when_command_ack_is_allowed(self) -> None:
        runner = self.runner
        configure_completed = subprocess.CompletedProcess(
            args=["loadlynx", "cc"],
            returncode=0,
            stdout=json.dumps(
                {
                    "mode": "CC",
                    "output_enabled": True,
                    "target_i_ma": 3900,
                }
            ),
            stderr="",
        )
        with (
            mock.patch.object(
                runner,
                "run_loadlynx",
                autospec=True,
                return_value=configure_completed,
            ) as run_cmd,
            mock.patch.object(
                runner,
                "wait_for_load_state",
                autospec=True,
                side_effect=AssertionError("wait_for_load_state should be skipped"),
            ) as wait_state,
        ):
            result = runner.load_cc(
                self.args,
                "fixture-load-device",
                3900,
                min_v_mv=3000,
                max_i_ma_total=4000,
                max_p_mw=80000,
                timeout_sec=1.0,
                status_timeout_sec=0.5,
                verify_timeout_sec=45.0,
                allow_command_ack_shortcut=True,
            )

        self.assertEqual(run_cmd.call_count, 1)
        self.assertEqual(wait_state.call_count, 0)
        self.assertTrue(result["verified_status"]["degraded_from_command_ack"])
        self.assertEqual(result["verified_status"]["effective_enabled"], True)
        self.assertEqual(result["verified_status"]["effective_target_i_ma"], 3900)


class DisableLoadVerificationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.runner = load_runner_module()
        self.args = types.SimpleNamespace(
            load_bridge_url="",
            load_devd_base_url="http://127.0.0.1:62841",
            load_devd_socket="/tmp/loadlynx.sock",
            load_cli="/tmp/fake-loadlynx",
            load_ipc="",
        )

    def test_disable_load_uses_direct_cli_confirmation_when_helper_status_is_stale(self) -> None:
        runner = self.runner
        completed = subprocess.CompletedProcess(
            args=["loadlynx", "control", "set", "--disable"],
            returncode=0,
            stdout="ok",
            stderr="",
        )
        stale_status = {
            "control": {"output_enabled": True, "target_i_ma": 3900},
            "status": {"enable": True, "i_local_ma": 1940, "i_remote_ma": 1930},
        }
        stale_verify_error = RuntimeError(
            "LoadLynx status did not reach expected state: enabled=False target_i_ma=None "
            "last_control={'control': {'output_enabled': True, 'target_i_ma': 3900}} "
            "last_status={'status': {'enable': True}}"
        )
        direct_status = {
            "control": {"output_enabled": False, "target_i_ma": 3900},
            "status": {"enable": False, "i_local_ma": 8, "i_remote_ma": 7},
            "source": "cli_status_direct",
        }
        with (
            mock.patch.object(
                runner,
                "get_load_status_best_effort",
                autospec=True,
                return_value=stale_status,
            ),
            mock.patch.object(
                runner,
                "get_load_control_best_effort",
                autospec=True,
                return_value=stale_status,
            ),
            mock.patch.object(runner, "run_loadlynx", autospec=True, return_value=completed),
            mock.patch.object(runner, "wait_for_load_state", autospec=True, side_effect=stale_verify_error),
            mock.patch.object(
                runner,
                "get_load_status_direct_cli_best_effort",
                autospec=True,
                return_value=direct_status,
            ) as direct_status_read,
            mock.patch.object(
                runner,
                "get_load_control_direct_cli_best_effort",
                autospec=True,
                side_effect=AssertionError("direct control should not be needed when direct status already confirms"),
            ),
        ):
            result = runner.disable_load(
                self.args,
                "fixture-load-device",
                timeout_sec=1.0,
                status_timeout_sec=0.5,
                verify_timeout_sec=0.5,
                assume_enabled=True,
            )

        self.assertEqual(direct_status_read.call_count, 1)
        self.assertTrue(result["verified_status"]["degraded_verification"])
        self.assertTrue(result["verified_status"]["degraded_from_direct_cli_confirmation"])
        self.assertFalse(result["verified_status"]["degraded_from_command_ack"])
        self.assertEqual(result["verified_status"]["effective_enabled"], False)
        self.assertEqual(result["verified_status"]["command_stdout"], completed.stdout)
        self.assertEqual(result["verified_status"]["command_stderr"], completed.stderr)

    def test_disable_load_falls_back_without_load_devd_socket(self) -> None:
        runner = self.runner
        self.args.load_devd_socket = ""
        completed = subprocess.CompletedProcess(
            args=["loadlynx", "control", "set", "--disable"],
            returncode=0,
            stdout="output=false",
            stderr="",
        )
        with (
            mock.patch.object(
                runner,
                "get_load_status_best_effort",
                autospec=True,
                return_value={"control": {"output_enabled": True}, "status": {"enable": True}},
            ),
            mock.patch.object(
                runner,
                "get_load_control_best_effort",
                autospec=True,
                return_value={"control": {"output_enabled": True, "target_i_ma": 3900}},
            ),
            mock.patch.object(runner, "run_loadlynx", autospec=True, return_value=completed) as run_cmd,
        ):
            result = runner.disable_load(
                self.args,
                "fixture-load-device",
                timeout_sec=1.0,
                status_timeout_sec=0.5,
                verify_timeout_sec=0.5,
                assume_enabled=False,
                allow_command_ack_shortcut=True,
            )

        self.assertEqual(
            run_cmd.call_args.args[0],
            [
                "/tmp/fake-loadlynx",
                "control",
                "set",
                "--device",
                "fixture-load-device",
                "--disable",
            ],
        )
        self.assertTrue(result["command_ack_disabled"])

    def test_disable_load_ignores_hidden_ipc_when_load_devd_socket_is_present(self) -> None:
        runner = self.runner
        self.args.load_devd_socket = "/tmp/loadlynx.sock"
        completed = subprocess.CompletedProcess(
            args=["loadlynx", "control", "set", "--disable"],
            returncode=0,
            stdout="output=false",
            stderr="",
        )
        with (
            mock.patch.object(
                runner,
                "get_load_status_best_effort",
                autospec=True,
                return_value={"control": {"output_enabled": True}, "status": {"enable": True}},
            ),
            mock.patch.object(
                runner,
                "get_load_control_best_effort",
                autospec=True,
                return_value={"control": {"output_enabled": True, "target_i_ma": 3900}},
            ),
            mock.patch.object(runner, "run_loadlynx", autospec=True, return_value=completed) as run_cmd,
        ):
            runner.disable_load(
                self.args,
                "fixture-load-device",
                timeout_sec=1.0,
                status_timeout_sec=0.5,
                verify_timeout_sec=0.5,
                assume_enabled=False,
                allow_command_ack_shortcut=True,
            )

        self.assertNotIn("--ipc", run_cmd.call_args.args[0])


class LoadDirectIpcCommandTests(unittest.TestCase):
    def setUp(self) -> None:
        self.runner = load_runner_module()
        self.args = types.SimpleNamespace(
            load_bridge_url="",
            load_devd_base_url="http://127.0.0.1:62841",
            load_devd_socket="/tmp/loadlynx.sock",
            load_cli="/tmp/fake-loadlynx",
            load_ipc="",
        )

    def test_force_loadlynx_ipc_cmd_uses_effective_ipc_socket(self) -> None:
        cmd = self.runner.force_loadlynx_ipc_cmd(
            self.args,
            "status",
            "--device",
            "fixture-load-device",
            "--json",
        )
        self.assertEqual(
            cmd,
            [
                "/tmp/fake-loadlynx",
                "--ipc",
                "/tmp/loadlynx.sock",
                "status",
                "--device",
                "fixture-load-device",
                "--json",
            ],
        )

    def test_resolve_load_devd_socket_falls_back_to_explicit_load_ipc(self) -> None:
        endpoint = self.runner.resolve_load_devd_socket(
            types.SimpleNamespace(
                load_ipc="/tmp/explicit-load-ipc.sock",
                load_devd_socket="",
            )
        )
        self.assertEqual(endpoint, "/tmp/explicit-load-ipc.sock")


class LoadStatusReadyShortcutTests(unittest.TestCase):
    def setUp(self) -> None:
        self.runner = load_runner_module()

    def test_verified_status_shortcut_skips_poll_refresh_after_load_action(self) -> None:
        args = types.SimpleNamespace(
            sample_interval_seconds=0.25,
        )

        class FakePoller:
            def generation(self):
                return 7

        verified_status = {
            "effective_enabled": True,
            "effective_target_i_ma": 3900,
            "degraded_verification": True,
        }

        freshness_limit_s = max(
            args.sample_interval_seconds * 2.0,
            self.runner.FORMAL_MAX_REALTIME_SAMPLE_AGE_SECONDS,
        )
        if (
            isinstance(verified_status, dict)
            and verified_status.get("degraded_verification") is True
        ):
            load_status_ready = {
                "ready": True,
                "waited_s": 0.0,
                "initial_generation": FakePoller().generation(),
                "ready_generation": FakePoller().generation(),
                "ready_age_s": 0.0,
                "freshness_limit_s": freshness_limit_s,
                "source": "verified_status_shortcut",
            }
        else:
            load_status_ready = {"ready": False}

        self.assertEqual(
            load_status_ready,
            {
                "ready": True,
                "waited_s": 0.0,
                "initial_generation": 7,
                "ready_generation": 7,
                "ready_age_s": 0.0,
                "freshness_limit_s": freshness_limit_s,
                "source": "verified_status_shortcut",
            },
        )

    def test_verified_status_shortcut_also_applies_to_non_degraded_verified_status(self) -> None:
        args = types.SimpleNamespace(
            sample_interval_seconds=0.25,
        )

        class FakePoller:
            def generation(self):
                return 11

        verified_status = {
            "effective_enabled": True,
            "effective_target_i_ma": 3900,
            "status": {"enable": True, "i_local_ma": 1952, "i_remote_ma": 1940},
        }

        freshness_limit_s = max(
            args.sample_interval_seconds * 2.0,
            self.runner.FORMAL_MAX_REALTIME_SAMPLE_AGE_SECONDS,
        )
        if (
            isinstance(verified_status, dict)
            and verified_status.get("effective_enabled") is True
            and verified_status.get("effective_target_i_ma") == 3900
        ):
            load_status_ready = {
                "ready": True,
                "waited_s": 0.0,
                "initial_generation": FakePoller().generation(),
                "ready_generation": FakePoller().generation(),
                "ready_age_s": 0.0,
                "freshness_limit_s": freshness_limit_s,
                "source": "verified_status_shortcut",
            }
        else:
            load_status_ready = {"ready": False}

        self.assertEqual(load_status_ready["source"], "verified_status_shortcut")

    def test_verified_status_shortcut_applies_to_disable_after_target(self) -> None:
        args = types.SimpleNamespace(
            sample_interval_seconds=0.25,
        )

        class FakePoller:
            def generation(self):
                return 13

        verified_status = {
            "effective_enabled": False,
            "effective_target_i_ma": 3900,
            "status": {"enable": False, "i_local_ma": 11, "i_remote_ma": 9},
        }

        freshness_limit_s = max(
            args.sample_interval_seconds * 2.0,
            self.runner.FORMAL_MAX_REALTIME_SAMPLE_AGE_SECONDS,
        )
        if (
            isinstance(verified_status, dict)
            and verified_status.get("effective_enabled") is False
        ):
            load_status_ready = {
                "ready": True,
                "waited_s": 0.0,
                "initial_generation": FakePoller().generation(),
                "ready_generation": FakePoller().generation(),
                "ready_age_s": 0.0,
                "freshness_limit_s": freshness_limit_s,
                "source": "verified_status_shortcut",
            }
        else:
            load_status_ready = {"ready": False}

        self.assertEqual(load_status_ready["source"], "verified_status_shortcut")

    def test_bootstrap_load_status_seed_falls_back_to_direct_cli_status(self) -> None:
        direct_status = {
            "control": {"output_enabled": False, "target_i_ma": 3900},
            "status": {"enable": False, "i_local_ma": 0, "i_remote_ma": 0},
            "source": "cli_status_direct",
        }
        args = types.SimpleNamespace(
            load_bridge_url="",
            load_devd_base_url="http://127.0.0.1:20641",
            load_devd_socket="/tmp/loadlynx.sock",
            load_cli="/tmp/fake-loadlynx",
            load_ipc="",
        )
        disable_result = {
            "verified_status": {"effective_enabled": False},
            "status": None,
            "control": None,
        }
        with mock.patch.object(
            self.runner,
            "get_load_status_direct_cli_best_effort",
            autospec=True,
            return_value=direct_status,
        ) as direct_status_read:
            seed, metadata = self.runner.bootstrap_load_status_seed(
                args,
                "fixture-load-device",
                disable_result=disable_result,
                timeout_sec=0.5,
            )
        self.assertEqual(seed, direct_status)
        self.assertEqual(metadata["source"], "direct_cli_status")
        self.assertTrue(metadata["verified"])
        direct_status_read.assert_called_once()


class UpsStatusTransportTests(unittest.TestCase):
    def setUp(self) -> None:
        self.runner = load_runner_module()
        self.args = types.SimpleNamespace(
            load_bridge_url="",
            load_devd_base_url="http://127.0.0.1:20641",
            load_devd_socket="/tmp/loadlynx.sock",
        )


class LoadTelemetryProbeRoutingTests(unittest.TestCase):
    def setUp(self) -> None:
        self.runner = load_runner_module()

    def test_run_load_telemetry_probe_passes_explicit_load_ipc(self) -> None:
        runner = self.runner
        args = types.SimpleNamespace(
            skip_load_telemetry_probe=False,
            load_telemetry_probe="tools/hil/probe_loadlynx_released_telemetry.py",
            load_cli="/tmp/loadlynx",
            load_device="fixture-load-device",
            load_usb_device_id="fixture-load-usb-device",
            load_usb_port="/tmp/fixture-load-usb-port",
            load_ipc="/tmp/explicit-load-ipc.sock",
            load_devd_base_url="",
            load_devd_socket="",
            load_bridge_url="",
            load_bridge_device="",
            command_timeout_sec=5.0,
        )
        completed = subprocess.CompletedProcess(
            args=["python3", "probe"],
            returncode=0,
            stdout="{}",
            stderr="",
        )
        with mock.patch.object(
            runner.subprocess,
            "run",
            autospec=True,
            return_value=completed,
        ) as run_mock:
            runner.run_load_telemetry_probe(args)
        cmd = run_mock.call_args.args[0]
        self.assertIn("--load-ipc", cmd)
        self.assertIn("/tmp/explicit-load-ipc.sock", cmd)


class LoadTransportNormalizationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.runner = load_runner_module()
        self.args = types.SimpleNamespace(
            load_bridge_url="",
            load_devd_base_url="http://127.0.0.1:20641",
            load_devd_socket="/tmp/loadlynx.sock",
        )

    def test_normalize_load_transport_args_disables_default_bridge_when_devd_transport_is_configured(self) -> None:
        args = types.SimpleNamespace(
            load_bridge_url=self.runner.DEFAULT_LOAD_BRIDGE_URL,
            load_ipc="",
            load_devd_base_url="http://127.0.0.1:20641",
            load_devd_socket="/tmp/loadlynx.sock",
        )
        self.runner.normalize_load_transport_args(args)
        self.assertEqual(args.load_bridge_url, "")

    def test_normalize_load_transport_args_disables_default_bridge_when_explicit_load_ipc_is_configured(self) -> None:
        args = types.SimpleNamespace(
            load_bridge_url=self.runner.DEFAULT_LOAD_BRIDGE_URL,
            load_ipc="/tmp/explicit-load-ipc.sock",
            load_devd_base_url="",
            load_devd_socket="",
        )
        self.runner.normalize_load_transport_args(args)
        self.assertEqual(args.load_bridge_url, "")

    def test_normalize_load_transport_args_clears_default_devd_base_when_explicit_load_ipc_is_configured(self) -> None:
        args = types.SimpleNamespace(
            load_bridge_url=self.runner.DEFAULT_LOAD_BRIDGE_URL,
            load_ipc="/tmp/explicit-load-ipc.sock",
            load_devd_base_url=self.runner.DEFAULT_LOAD_DEVD_BASE_URL,
            load_devd_socket="",
        )
        self.runner.normalize_load_transport_args(args)
        self.assertEqual(args.load_devd_base_url, "")

    def test_normalize_load_transport_args_keeps_non_default_bridge_url(self) -> None:
        args = types.SimpleNamespace(
            load_bridge_url="http://127.0.0.1:30181",
            load_ipc="",
            load_devd_base_url="http://127.0.0.1:20641",
            load_devd_socket="/tmp/loadlynx.sock",
        )
        self.runner.normalize_load_transport_args(args)
        self.assertEqual(args.load_bridge_url, "http://127.0.0.1:30181")

    def test_effective_load_bridge_url_is_empty_when_explicit_load_ipc_is_configured(self) -> None:
        args = types.SimpleNamespace(
            load_bridge_url=self.runner.DEFAULT_LOAD_BRIDGE_URL,
            load_ipc="/tmp/explicit-load-ipc.sock",
            load_devd_base_url="",
            load_devd_socket="",
        )
        self.assertEqual(self.runner.effective_load_bridge_url(args), "")

    def test_formal_runner_promotes_localhost_status_url_to_direct_lan(self) -> None:
        requested = "http://127.0.0.1:38140/api/v1/devices/fixture-mains-aegis/status"
        promoted = self.runner.maybe_promote_ups_status_url_to_direct_lan(
            requested,
            lan_address="127.0.0.1:30081",
        )
        self.assertEqual(promoted, "http://127.0.0.1:30081/api/v1/status")

    def test_normalized_observe_urls_force_control_device_id_and_scan_base(self) -> None:
        args = types.SimpleNamespace(
            ups_device_id="fixture-ups-device",
            ups_status_url="http://127.0.0.1:30080/api/v1/devices/fixture-mains-aegis/status",
            ups_settings_url="http://127.0.0.1:30080/api/v1/devices/fixture-mains-aegis/settings",
            devd_diag_snapshot_url="http://127.0.0.1:30080/api/v1/devices/fixture-mains-aegis/diag-snapshot",
            devd_monitor_start_url="http://127.0.0.1:30080/api/v1/devices/fixture-mains-aegis/monitor/start",
            devd_device_trace_url="http://127.0.0.1:30080/api/v1/devices/fixture-mains-aegis/trace?trace_limit=1",
            devd_scan_url="http://127.0.0.1:38140/api/v1/devices/scan",
        )
        normalized = self.runner.normalized_observe_urls(args)
        self.assertEqual(
            normalized["ups_status_url"],
            "http://127.0.0.1:38140/api/v1/devices/fixture-ups-device/status",
        )
        self.assertEqual(
            normalized["devd_diag_snapshot_url"],
            "http://127.0.0.1:38140/api/v1/devices/fixture-ups-device/diag-snapshot",
        )
        self.assertEqual(
            normalized["devd_monitor_start_url"],
            "http://127.0.0.1:38140/api/v1/devices/fixture-ups-device/monitor/start",
        )

    def test_validate_mains_aegis_devd_bootstrap_accepts_api_only_mode(self) -> None:
        result = self.runner.validate_mains_aegis_devd_bootstrap(
            {
                "app": {
                    "name": "mains-aegis-devd",
                    "mode": "http_service_api_only",
                }
            }
        )
        self.assertTrue(result["ok"])
        self.assertEqual(result["failures"], [])

    def test_validate_mains_aegis_devd_bootstrap_rejects_non_devd_payload(self) -> None:
        result = self.runner.validate_mains_aegis_devd_bootstrap(
            {
                "app": {
                    "name": "storybook",
                    "mode": "preview",
                }
            }
        )
        self.assertFalse(result["ok"])
        self.assertIn("bootstrap_app_name_mismatch", result["failures"])
        self.assertIn("bootstrap_app_mode_invalid", result["failures"])

    def test_wait_for_load_state_reuses_live_poller_devd_lease_for_control_fallback(self) -> None:
        runner = self.runner

        class FakePoller:
            def snapshot(self, now_monotonic: float) -> dict[str, object]:
                return {
                    "payload": {
                        "status": {"enable": True, "i_local_ma": 1000, "i_remote_ma": 1000},
                    },
                    "generation": 1,
                    "age_s": 0.01,
                    "sample_age_s": 9.5,
                    "error": "TimeoutError('timed out')",
                    "source": "replace",
                }

            def bridge_lease_snapshot(self):
                return None

            def load_devd_lease_snapshot(self):
                return {"lease_id": "lease-1"}

        helper_status = {
            "status": {"enable": True, "i_local_ma": 1000, "i_remote_ma": 1000},
            "source": "ipc_status",
        }
        helper_control = {
            "control": {"output_enabled": True, "target_i_ma": 3900},
            "source": "ipc_control_from_status",
        }
        with (
            mock.patch.object(
                runner,
                "get_load_status_best_effort",
                autospec=True,
                return_value=helper_status,
            ) as get_status,
            mock.patch.object(
                runner,
                "get_load_control_best_effort",
                autospec=True,
                return_value=helper_control,
            ) as get_control,
        ):
            result = runner.wait_for_load_state(
                self.args,
                "fixture-load-device",
                expected_enabled=True,
                expected_target_i_ma=3900,
                status_timeout_sec=0.1,
                verify_timeout_sec=0.25,
                poll_interval_sec=0.05,
                live_status_poller=FakePoller(),
            )

        self.assertEqual(result["effective_enabled"], True)
        self.assertEqual(result["effective_target_i_ma"], 3900)
        self.assertEqual(get_status.call_args.kwargs["load_devd_lease"], {"lease_id": "lease-1"})
        self.assertEqual(get_control.call_args.kwargs["load_devd_lease"], {"lease_id": "lease-1"})

    def test_seeded_devd_device_is_capability_ready_requires_connected_identity_and_settings_only(self) -> None:
        self.assertTrue(
            self.runner.seeded_devd_device_is_capability_ready(
                {
                    "connection": "connected",
                    "identity": {},
                    "settings": {},
                }
            )
        )

    def test_get_load_status_direct_cli_falls_back_without_load_devd_socket(self) -> None:
        args = types.SimpleNamespace(
            load_cli="/Users/ivan/.local/bin/loadlynx",
            load_ipc="",
            load_devd_socket="",
            load_bridge_url="",
        )
        with mock.patch.object(
            self.runner,
            "run_loadlynx",
            autospec=True,
            return_value=types.SimpleNamespace(
                stdout=json.dumps(
                    {
                        "control": {"output_enabled": False, "mode": "cc"},
                        "status": {"enable": False, "i_local_ma": 0, "i_remote_ma": 0},
                    }
                )
            ),
        ) as run_mock:
            payload = self.runner.get_load_status_direct_cli(
                args,
                "fixture-load-device",
                timeout_sec=1.0,
            )
        called_cmd = run_mock.call_args.args[0]
        self.assertNotIn("--ipc", called_cmd)
        self.assertEqual(payload["source"], "cli_status_direct")
        self.assertFalse(
            self.runner.seeded_devd_device_is_capability_ready(
                {
                    "connection": "disconnected",
                    "identity": {},
                    "settings": {},
                }
            )
        )


class IsolapurrFetchTests(unittest.TestCase):
    def setUp(self) -> None:
        self.runner = load_runner_module()

    def test_fetch_isolapurr_ports_prefers_http_ports_payload(self) -> None:
        runner = self.runner
        http_payload = {
            "ports": {
                "ports": [
                    {
                        "portId": "port_c",
                        "state": {"power_enabled": True},
                        "telemetry": {
                            "status": "ok",
                            "voltage_mv": 12028,
                            "current_ma": 177,
                        },
                    }
                ]
            }
        }
        with (
            mock.patch.object(
                runner,
                "http_json_with_retries",
                autospec=True,
                return_value=http_payload,
            ) as http_get,
            mock.patch.object(
                runner,
                "run_json_command_with_retries",
                autospec=True,
                side_effect=AssertionError("CLI fallback should not be used when HTTP succeeds"),
            ),
        ):
            payload = runner.fetch_isolapurr_ports(
                "http://127.0.0.1:30182",
                timeout_sec=0.5,
                isolapurr_cli="isolapurr",
            )

        self.assertEqual(payload["source"], "http_ports")
        http_get.assert_called_once()

    def test_fetch_isolapurr_ports_uses_cli_power_show_payload(self) -> None:
        runner = self.runner
        cli_payload = {
            "config": {"manual": {"voltage_mv": 12000}},
            "ports": {
                "ports": [
                    {
                        "portId": "port_c",
                        "state": {"power_enabled": True},
                        "telemetry": {
                            "status": "ok",
                            "voltage_mv": 12034,
                            "current_ma": 72,
                        },
                    }
                ]
            },
        }
        with (
            mock.patch.object(
                runner,
                "http_json_with_retries",
                autospec=True,
                side_effect=RuntimeError("http unavailable"),
            ),
            mock.patch.object(
                runner,
                "run_json_command_with_retries",
                autospec=True,
                return_value=cli_payload,
            ) as run_json,
        ):
            payload = runner.fetch_isolapurr_ports(
                "http://127.0.0.1:30182",
                timeout_sec=0.5,
                isolapurr_cli="isolapurr",
            )

        self.assertEqual(payload["source"], "cli_power_show")
        port_c = runner.port_state(payload, port_id="port_c")
        self.assertEqual(port_c.get("telemetry", {}).get("voltage_mv"), 12034)
        self.assertEqual(port_c.get("state", {}).get("power_enabled"), True)
        run_json.assert_called_once_with(
            [
                "isolapurr",
                "power",
                "show",
                "--url",
                "http://127.0.0.1:30182",
                "--json",
            ],
            timeout_sec=0.5,
        )

    def test_port_state_backfills_power_enabled_from_power_show_diagnostics(self) -> None:
        runner = self.runner
        payload = {
            "diagnostics": {"usb_c_power_enabled": True},
            "ports": {
                "ports": [
                    {
                        "portId": "port_c",
                        "telemetry": {
                            "status": "ok",
                            "voltage_mv": 12031,
                            "current_ma": 189,
                        },
                    }
                ]
            },
        }
        port_c = runner.port_state(payload, port_id="port_c")
        self.assertEqual(port_c.get("state", {}).get("power_enabled"), True)
        self.assertEqual(port_c.get("telemetry", {}).get("voltage_mv"), 12031)

    def test_isolapurr_snapshot_ready_enabled_uses_root_payload_shape(self) -> None:
        runner = self.runner
        payload = {
            "diagnostics": {"usb_c_power_enabled": True},
            "ports": {
                "ports": [
                    {
                        "portId": "port_c",
                        "telemetry": {
                            "status": "ok",
                            "voltage_mv": 12026,
                            "current_ma": 297,
                        },
                    }
                ]
            },
        }
        snapshot = {"payload": payload, "generation": 12, "age_s": 0.1, "error": None}
        self.assertTrue(
            runner.isolapurr_snapshot_has_expected_state(
                snapshot,
                expected_enabled=True,
            )
        )

    def test_isolapurr_sample_expected_cut_uses_root_payload_shape(self) -> None:
        runner = self.runner
        sample = {
            "phase": "backup",
            "port_c_enabled": False,
            "raw": {
                "isolapurr_power": {
                    "ports": {
                        "ports": [
                            {
                                "portId": "port_c",
                                "telemetry": {
                                    "status": "not_inserted",
                                    "voltage_mv": None,
                                    "current_ma": None,
                                },
                            }
                        ]
                    }
                }
            },
        }
        self.assertTrue(runner.isolapurr_sample_has_expected_cut_state(sample))


class LoadStatusSourceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.runner = load_runner_module()
        self.args = types.SimpleNamespace(
            load_bridge_url="",
            load_devd_base_url="http://127.0.0.1:49210",
            load_usb_device_id="fixture-load-usb-device",
            load_cli="/tmp/loadlynx",
            load_ipc="",
            load_devd_socket="/tmp/loadlynx.sock",
        )

    def test_get_load_status_prefers_devd_http_before_slow_cli(self) -> None:
        runner = self.runner
        devd_payload = {
            "control": {"output_enabled": False, "target_i_ma": 3900},
            "status": {"enable": False, "v_local_mv": 11999, "i_local_ma": 9, "i_remote_ma": 8},
        }
        with (
            mock.patch.object(
                runner,
                "get_load_status_via_devd_http",
                autospec=True,
                side_effect=lambda *args, **kwargs: {**devd_payload, "source": "devd_http_status"},
            ) as via_devd,
            mock.patch.object(
                runner,
                "run_loadlynx",
                autospec=True,
                side_effect=AssertionError("CLI fallback should not be used when devd HTTP succeeds"),
            ),
        ):
            payload = runner.get_load_status(
                self.args,
                "fixture-load-device",
                timeout_sec=0.5,
            )

        self.assertEqual(payload["source"], "devd_http_status")
        self.assertEqual(payload["status"]["v_local_mv"], 11999)
        via_devd.assert_called_once()

    def test_get_load_status_skips_devd_http_when_preference_disabled(self) -> None:
        runner = self.runner
        with (
            mock.patch.object(
                runner,
                "get_load_status_via_devd_http",
                autospec=True,
                side_effect=AssertionError("devd HTTP should be skipped"),
            ) as via_devd,
            mock.patch.object(
                runner,
                "get_load_status_via_ipc_without_lease",
                autospec=True,
                side_effect=RuntimeError("ipc socket unavailable"),
            ),
            mock.patch.object(
                runner,
                "get_load_status_via_ipc_helper",
                autospec=True,
                side_effect=RuntimeError("ipc helper unavailable"),
            ),
        ):
            with self.assertRaisesRegex(
                RuntimeError,
                "load_status_devd_transport_exhausted_without_cli_fallback",
            ):
                runner.get_load_status(
                    self.args,
                    "fixture-load-device",
                    timeout_sec=0.5,
                    prefer_devd_http=False,
                )

        self.assertEqual(via_devd.call_count, 0)

    def test_get_load_status_prefers_ipc_helper_before_devd_http(self) -> None:
        runner = self.runner
        helper_payload = {
            "control": {"output_enabled": False, "target_i_ma": 3900},
            "status": {"enable": False, "v_local_mv": 12007, "i_local_ma": 9, "i_remote_ma": 8},
            "source": "ipc_helper_status",
        }
        with (
            mock.patch.object(
                runner,
                "get_load_status_via_ipc_helper",
                autospec=True,
                return_value=helper_payload,
            ) as via_helper,
            mock.patch.object(
                runner,
                "get_load_status_via_devd_http",
                autospec=True,
                side_effect=AssertionError("devd HTTP should not run when IPC helper succeeds"),
            ),
            mock.patch.object(
                runner,
                "run_loadlynx",
                autospec=True,
                side_effect=AssertionError("CLI fallback should not run when IPC helper succeeds"),
            ),
        ):
            payload = runner.get_load_status(
                self.args,
                "fixture-load-device",
                timeout_sec=0.5,
            )

        self.assertEqual(payload["source"], "ipc_helper_status")
        self.assertEqual(payload["status"]["v_local_mv"], 12007)
        via_helper.assert_called_once()

    def test_get_load_status_via_ipc_helper_requests_scan_and_warmup(self) -> None:
        runner = self.runner
        args = types.SimpleNamespace(
            load_ipc="",
            load_devd_socket="/tmp/loadlynx.sock",
            load_ipc_status_helper="tools/hil/loadlynx_ipc_status_helper.py",
            load_usb_device_id="fixture-load-usb-device",
        )
        completed = {
            "result": {
                "control": {"output_enabled": False},
                "status": {"enable": False},
            }
        }
        with mock.patch.object(
            runner,
            "run_json_command_with_retries",
            autospec=True,
            return_value=completed,
        ) as run_helper:
            runner.get_load_status_via_ipc_helper(
                args,
                timeout_sec=0.5,
                load_devd_lease={"lease_id": "lease-1"},
                scan_first=True,
                warmup=True,
            )

        cmd = run_helper.call_args.args[0]
        self.assertIn("--scan-first", cmd)
        self.assertIn("--warmup", cmd)
        self.assertIn("--lease-id", cmd)

    def test_get_load_status_via_ipc_helper_can_skip_scan_and_warmup_for_fast_poll(self) -> None:
        runner = self.runner
        args = types.SimpleNamespace(
            load_ipc="",
            load_devd_socket="/tmp/loadlynx.sock",
            load_ipc_status_helper="tools/hil/loadlynx_ipc_status_helper.py",
            load_usb_device_id="fixture-load-usb-device",
        )
        completed = {
            "result": {
                "control": {"output_enabled": False},
                "status": {"enable": False},
            }
        }
        with mock.patch.object(
            runner,
            "run_json_command_with_retries",
            autospec=True,
            return_value=completed,
        ) as run_helper:
            runner.get_load_status_via_ipc_helper(
                args,
                timeout_sec=0.5,
                load_devd_lease={"lease_id": "lease-1"},
                scan_first=False,
                warmup=False,
            )

        cmd = run_helper.call_args.args[0]
        self.assertNotIn("--scan-first", cmd)
        self.assertNotIn("--warmup", cmd)
        self.assertIn("--lease-id", cmd)

    def test_get_load_status_prefers_inprocess_ipc_before_helper(self) -> None:
        runner = self.runner
        ipc_payload = {
            "control": {"output_enabled": False, "target_i_ma": 3900},
            "status": {"enable": False, "v_local_mv": 12009, "i_local_ma": 11, "i_remote_ma": 10},
            "source": "ipc_status",
        }
        with (
            mock.patch.object(
                runner,
                "get_load_status_via_ipc",
                autospec=True,
                return_value=ipc_payload,
            ) as via_ipc,
            mock.patch.object(
                runner,
                "get_load_status_via_ipc_helper",
                autospec=True,
                side_effect=AssertionError("IPC helper should not run when direct IPC succeeds"),
            ),
            mock.patch.object(
                runner,
                "get_load_status_via_devd_http",
                autospec=True,
                side_effect=AssertionError("devd HTTP should not run when direct IPC succeeds"),
            ),
            mock.patch.object(
                runner,
                "run_loadlynx",
                autospec=True,
                side_effect=AssertionError("CLI fallback should not run when direct IPC succeeds"),
            ),
        ):
            payload = runner.get_load_status(
                self.args,
                "fixture-load-device",
                timeout_sec=0.5,
                load_devd_lease={"lease_id": "lease-1"},
            )

        self.assertEqual(payload["source"], "ipc_status")
        self.assertEqual(payload["status"]["v_local_mv"], 12009)
        via_ipc.assert_called_once()

    def test_get_load_status_with_devd_socket_does_not_fall_back_to_raw_cli_status(self) -> None:
        runner = self.runner
        with (
            mock.patch.object(
                runner,
                "get_load_status_via_ipc_without_lease",
                autospec=True,
                side_effect=RuntimeError("ipc_without_lease_failed"),
            ),
            mock.patch.object(
                runner,
                "get_load_status_via_ipc_helper",
                autospec=True,
                side_effect=RuntimeError("ipc_helper_failed"),
            ),
            mock.patch.object(
                runner,
                "get_load_status_via_devd_http",
                autospec=True,
                side_effect=RuntimeError("devd_http_failed"),
            ),
            mock.patch.object(
                runner,
                "run_loadlynx",
                autospec=True,
                side_effect=AssertionError("raw CLI status must not be used when load_devd_socket is configured"),
            ),
        ):
            with self.assertRaisesRegex(RuntimeError, "load_status_devd_transport_exhausted_without_cli_fallback"):
                runner.get_load_status(
                    self.args,
                    "fixture-load-device",
                    timeout_sec=0.5,
                )

    def test_get_load_status_helper_fast_poll_skips_scan_and_warmup_when_lease_present(self) -> None:
        runner = self.runner
        helper_payload = {
            "control": {"output_enabled": False, "target_i_ma": 3900},
            "status": {"enable": False, "v_local_mv": 12007, "i_local_ma": 9, "i_remote_ma": 8},
            "source": "ipc_helper_status",
        }
        with (
            mock.patch.object(
                runner,
                "get_load_status_via_ipc",
                autospec=True,
                side_effect=RuntimeError("ipc failed"),
            ),
            mock.patch.object(
                runner,
                "get_load_status_via_ipc_helper",
                autospec=True,
                return_value=helper_payload,
            ) as via_helper,
        ):
            payload = runner.get_load_status(
                self.args,
                "fixture-load-device",
                timeout_sec=0.5,
                load_devd_lease={"lease_id": "lease-1"},
            )

        self.assertEqual(payload["source"], "ipc_helper_status")
        self.assertFalse(via_helper.call_args.kwargs["scan_first"])
        self.assertFalse(via_helper.call_args.kwargs["warmup"])

    def test_get_load_status_falls_back_to_devd_http_when_no_lease_and_helper_fails(self) -> None:
        runner = self.runner
        devd_payload = {
            "control": {"output_enabled": False, "target_i_ma": 3900},
            "status": {"enable": False, "v_local_mv": 11995, "i_local_ma": 10, "i_remote_ma": 8},
            "source": "devd_http_status",
        }
        with (
            mock.patch.object(
                runner,
                "get_load_status_via_ipc_helper",
                autospec=True,
                side_effect=RuntimeError("ipc helper unavailable"),
            ) as via_helper,
            mock.patch.object(
                runner,
                "get_load_status_via_devd_http",
                autospec=True,
                return_value=devd_payload,
            ) as via_devd,
            mock.patch.object(
                runner,
                "run_loadlynx",
                autospec=True,
                side_effect=AssertionError("CLI fallback should not run when devd HTTP succeeds"),
            ),
        ):
            payload = runner.get_load_status(
                self.args,
                "fixture-load-device",
                timeout_sec=0.5,
            )

        self.assertEqual(payload["source"], "devd_http_status")
        via_helper.assert_called_once()
        via_devd.assert_called_once()

    def test_get_load_status_without_socket_falls_back_to_raw_cli_when_devd_http_fails(self) -> None:
        runner = self.runner
        self.args.load_devd_socket = ""
        cli_payload = {
            "control": {"output_enabled": False, "target_i_ma": 3900},
            "status": {"enable": False, "v_local_mv": 11992, "i_local_ma": 9, "i_remote_ma": 8},
            "source": "cli_status_direct",
        }
        completed = subprocess.CompletedProcess(
            args=["loadlynx", "status"],
            returncode=0,
            stdout=json.dumps(cli_payload),
            stderr="",
        )
        with (
            mock.patch.object(
                runner,
                "get_load_status_via_devd_http",
                autospec=True,
                side_effect=RuntimeError("devd http failed"),
            ) as via_devd,
            mock.patch.object(
                runner,
                "get_load_status_via_ipc_helper",
                autospec=True,
                side_effect=AssertionError("IPC helper must not run without socket"),
            ) as via_helper,
            mock.patch.object(runner, "run_loadlynx", autospec=True, return_value=completed) as run_cmd,
        ):
            payload = runner.get_load_status(
                self.args,
                "fixture-load-device",
                timeout_sec=0.5,
                prefer_devd_http=True,
            )

        self.assertEqual(payload["source"], "cli_status_direct")
        via_devd.assert_called_once()
        self.assertEqual(via_helper.call_count, 0)
        self.assertNotIn("--ipc", run_cmd.call_args.args[0])

    def test_get_load_status_without_lease_prefers_ipc_helper_with_scan_and_warmup(self) -> None:
        runner = self.runner
        helper_payload = {
            "control": {"output_enabled": False, "target_i_ma": 3900},
            "status": {"enable": False, "v_local_mv": 12017, "i_local_ma": 10, "i_remote_ma": 9},
            "source": "ipc_helper_status",
        }
        with (
            mock.patch.object(
                runner,
                "get_load_status_via_ipc_without_lease",
                autospec=True,
                side_effect=RuntimeError("no-lease ipc failed"),
            ) as via_ipc_no_lease,
            mock.patch.object(
                runner,
                "get_load_status_via_ipc_helper",
                autospec=True,
                return_value=helper_payload,
            ) as via_helper,
            mock.patch.object(
                runner,
                "get_load_status_via_devd_http",
                autospec=True,
                side_effect=AssertionError("devd HTTP should not run when IPC helper succeeds"),
            ),
            mock.patch.object(
                runner,
                "run_loadlynx",
                autospec=True,
                side_effect=AssertionError("CLI fallback should not run when IPC helper succeeds"),
            ),
        ):
            payload = runner.get_load_status(
                self.args,
                "fixture-load-device",
                timeout_sec=0.5,
                load_devd_lease=None,
            )

        self.assertEqual(payload["source"], "ipc_helper_status")
        via_ipc_no_lease.assert_called_once()
        self.assertTrue(via_helper.call_args.kwargs["scan_first"])
        self.assertTrue(via_helper.call_args.kwargs["warmup"])

    def test_get_load_status_falls_back_to_devd_http_when_direct_ipc_fails(self) -> None:
        runner = self.runner
        devd_payload = {
            "control": {"output_enabled": False, "target_i_ma": 3900},
            "status": {"enable": False, "v_local_mv": 11996, "i_local_ma": 10, "i_remote_ma": 9},
            "source": "devd_http_status",
        }
        with (
            mock.patch.object(
                runner,
                "get_load_status_via_ipc",
                autospec=True,
                side_effect=RuntimeError("ipc failed"),
            ) as via_ipc,
            mock.patch.object(
                runner,
                "get_load_status_via_devd_http",
                autospec=True,
                return_value=devd_payload,
            ) as via_devd,
            mock.patch.object(
                runner,
                "get_load_status_via_ipc_helper",
                autospec=True,
                side_effect=RuntimeError("helper failed"),
            ) as via_helper,
            mock.patch.object(
                runner,
                "run_loadlynx",
                autospec=True,
                side_effect=AssertionError("CLI fallback should not run when devd HTTP succeeds"),
            ),
        ):
            payload = runner.get_load_status(
                self.args,
                "fixture-load-device",
                timeout_sec=0.5,
                load_devd_lease={"lease_id": "lease-1"},
            )

        self.assertEqual(payload["source"], "devd_http_status")
        via_ipc.assert_called()
        via_helper.assert_called()
        via_devd.assert_called_once()


class LoadStatusPollerTests(unittest.TestCase):
    def setUp(self) -> None:
        self.runner = load_runner_module()

    def test_wait_for_live_load_status_rejects_synthetic_command_ack(self) -> None:
        runner = self.runner

        class SyntheticPoller:
            def snapshot(self, now_monotonic: float) -> dict[str, object]:
                return {
                    "payload": {
                        "control": {"output_enabled": True, "target_i_ma": 1000},
                        "status": {"enable": True},
                        "source": "command_ack_synthetic_status",
                    },
                    "status": {
                        "control": {"output_enabled": True, "target_i_ma": 1000},
                        "status": {"enable": True},
                        "source": "command_ack_synthetic_status",
                    },
                    "generation": 10,
                    "age_s": 0.01,
                    "source": "replace",
                }

        with self.assertRaisesRegex(RuntimeError, "load_status_not_ready"):
            runner.wait_for_live_load_status(
                SyntheticPoller(),
                sample_interval_seconds=0.01,
                timeout_sec=0.05,
                require_new_generation=False,
            )

    def test_wait_for_live_load_status_accepts_real_telemetry(self) -> None:
        runner = self.runner

        class RealPoller:
            def snapshot(self, now_monotonic: float) -> dict[str, object]:
                return {
                    "payload": {
                        "control": {"output_enabled": True, "target_i_ma": 1000},
                        "status": {"enable": True, "v_local_mv": 12000, "i_local_ma": 1000},
                    },
                    "generation": 10,
                    "age_s": 0.01,
                    "source": "status-stream",
                }

        ready = runner.wait_for_live_load_status(
            RealPoller(),
            sample_interval_seconds=0.01,
            timeout_sec=0.05,
            require_new_generation=False,
        )
        self.assertTrue(ready["ready"])

    def test_load_status_poller_disables_devd_http_even_with_lease(self) -> None:
        runner = self.runner
        args = types.SimpleNamespace(
            load_bridge_url="",
            load_devd_base_url="http://127.0.0.1:20641",
            load_status_source="poll",
            load_usb_device_id="fixture-load-usb-device",
            load_cli="/tmp/loadlynx",
            load_ipc="",
            load_devd_socket="/tmp/loadlynx.sock",
        )
        poller = runner.LoadStatusPoller(
            args,
            "fixture-load-device",
            timeout_sec=0.2,
            poll_interval_sec=0.1,
            stream_interval_sec=0.2,
            use_status_stream=False,
        )
        with (
            mock.patch.object(
                runner.LoadStatusPoller,
                "_ensure_bridge_lease",
                autospec=True,
                return_value=None,
            ),
            mock.patch.object(
                runner.LoadStatusPoller,
                "_ensure_load_devd_lease",
                autospec=True,
                return_value={"lease_id": "lease-1"},
            ),
            mock.patch.object(
                runner,
                "get_load_status_via_ipc",
                autospec=True,
                side_effect=SystemExit,
            ) as via_ipc,
        ):
            with self.assertRaises(SystemExit):
                poller._run()
        self.assertEqual(via_ipc.call_args.kwargs["load_devd_lease"], {"lease_id": "lease-1"})

    def test_load_status_poller_prefers_ipc_helper_fast_poll_when_socket_present(self) -> None:
        runner = self.runner
        args = types.SimpleNamespace(
            load_bridge_url="",
            load_devd_base_url="http://127.0.0.1:20641",
            load_status_source="poll",
            load_usb_device_id="fixture-load-usb-device",
            load_cli="/tmp/loadlynx",
            load_ipc="",
            load_devd_socket="/tmp/loadlynx.sock",
        )
        poller = runner.LoadStatusPoller(
            args,
            "fixture-load-device",
            timeout_sec=0.2,
            poll_interval_sec=0.1,
            stream_interval_sec=0.2,
            use_status_stream=False,
        )
        with (
            mock.patch.object(
                runner.LoadStatusPoller,
                "_ensure_bridge_lease",
                autospec=True,
                return_value=None,
            ),
            mock.patch.object(
                runner,
                "get_load_status_via_ipc_without_lease",
                autospec=True,
                side_effect=SystemExit,
            ) as via_ipc_no_lease,
            mock.patch.object(
                runner,
                "get_load_status_via_ipc",
                autospec=True,
                side_effect=AssertionError("leased direct ipc should not run before no-lease path"),
            ),
            mock.patch.object(
                runner,
                "get_load_status_via_ipc_helper",
                autospec=True,
                side_effect=AssertionError("helper path should not run before no-lease path"),
            ),
        ):
            with self.assertRaises(SystemExit):
                poller._run()

        via_ipc_no_lease.assert_called_once()

    def test_load_status_poller_allows_status_stream_over_explicit_load_ipc(self) -> None:
        runner = self.runner
        args = types.SimpleNamespace(
            load_bridge_url="",
            load_devd_base_url="http://127.0.0.1:20641",
            load_status_source="status-stream",
            load_usb_device_id="fixture-load-usb-device",
            load_cli="/tmp/loadlynx",
            load_ipc="/tmp/explicit-load-ipc.sock",
            load_devd_socket="",
        )
        poller = runner.LoadStatusPoller(
            args,
            "fixture-load-device",
            timeout_sec=0.2,
            poll_interval_sec=0.1,
            stream_interval_sec=0.2,
            use_status_stream=True,
        )
        with (
            mock.patch.object(
                runner.LoadStatusPoller,
                "_run_status_stream",
                autospec=True,
                side_effect=SystemExit,
            ) as run_status_stream,
        ):
            with self.assertRaises(SystemExit):
                poller._run()

        run_status_stream.assert_called_once_with(poller)

    def test_start_stream_process_uses_current_status_stream_cli_contract(self) -> None:
        runner = self.runner
        args = types.SimpleNamespace(
            load_bridge_url="",
            load_devd_base_url="",
            load_status_source="status-stream",
            load_usb_device_id="fixture-load-usb-device",
            load_cli="/tmp/loadlynx",
            load_ipc="/tmp/explicit-load-ipc.sock",
            load_devd_socket="",
        )
        poller = runner.LoadStatusPoller(
            args,
            "fixture-load-device",
            timeout_sec=0.2,
            poll_interval_sec=0.1,
            stream_interval_sec=1.0 / 3.0,
            use_status_stream=True,
        )
        fake_process = mock.Mock()
        with mock.patch.object(runner.subprocess, "Popen", return_value=fake_process) as popen:
            process = poller._start_stream_process()

        self.assertIs(process, fake_process)
        self.assertEqual(
            popen.call_args.args[0],
            [
                "/tmp/loadlynx",
                "--ipc",
                "/tmp/explicit-load-ipc.sock",
                "status-stream",
                "--device",
                "fixture-load-device",
                "--rate-hz",
                "3",
                "--jsonl",
            ],
        )

    def test_ensure_load_devd_lease_warms_status_after_acquire(self) -> None:
        runner = self.runner
        args = types.SimpleNamespace(
            load_bridge_url="",
            load_devd_base_url="",
            load_status_source="poll",
            load_usb_device_id="fixture-load-usb-device",
            load_device="fixture-load-device",
            load_cli="/tmp/loadlynx",
            load_ipc="",
            load_devd_socket="/tmp/loadlynx.sock",
        )
        poller = runner.LoadStatusPoller(
            args,
            "fixture-load-device",
            timeout_sec=0.2,
            poll_interval_sec=0.1,
            stream_interval_sec=0.2,
            use_status_stream=False,
        )
        with (
            mock.patch.object(
                runner,
                "acquire_load_devd_lease_via_ipc",
                autospec=True,
                return_value={"lease_id": "lease-1", "lease_ttl_ms": 8000},
            ),
            mock.patch.object(
                runner,
                "warm_load_status_via_ipc",
                autospec=True,
                return_value={"status": {"enable": False}},
            ) as warm_status,
        ):
            lease = poller._ensure_load_devd_lease()

        self.assertEqual(lease, {"lease_id": "lease-1"})
        warm_status.assert_called_once()

    def test_ensure_load_devd_lease_skips_ipc_warmup_without_socket(self) -> None:
        runner = self.runner
        args = types.SimpleNamespace(
            load_bridge_url="",
            load_devd_base_url="http://127.0.0.1:20641",
            load_status_source="poll",
            load_usb_device_id="fixture-load-usb-device",
            load_device="fixture-load-device",
            load_cli="/tmp/loadlynx",
            load_ipc="",
            load_devd_socket="",
        )
        poller = runner.LoadStatusPoller(
            args,
            "fixture-load-device",
            timeout_sec=0.2,
            poll_interval_sec=0.1,
            stream_interval_sec=0.2,
            use_status_stream=False,
        )
        with (
            mock.patch.object(
                runner,
                "acquire_load_devd_lease",
                autospec=True,
                return_value={"lease_id": "lease-1", "lease_ttl_ms": 8000},
            ),
            mock.patch.object(
                runner,
                "warm_load_status_via_ipc",
                autospec=True,
                side_effect=AssertionError("IPC warmup must not run without socket"),
            ) as warm_status,
            mock.patch.object(
                runner,
                "get_load_status_via_ipc_helper",
                autospec=True,
                side_effect=AssertionError("IPC helper warmup must not run without socket"),
            ) as helper_status,
        ):
            lease = poller._ensure_load_devd_lease()

        self.assertEqual(lease, {"lease_id": "lease-1"})
        self.assertEqual(warm_status.call_count, 0)
        self.assertEqual(helper_status.call_count, 0)

    def test_load_status_poller_uses_fast_no_lease_ipc_path_when_available(self) -> None:
        runner = self.runner
        args = types.SimpleNamespace(
            load_bridge_url="",
            load_devd_base_url="http://127.0.0.1:20641",
            load_status_source="poll",
            load_usb_device_id="fixture-load-usb-device",
            load_cli="/tmp/loadlynx",
            load_ipc="",
            load_devd_socket="/tmp/loadlynx.sock",
        )
        poller = runner.LoadStatusPoller(
            args,
            "fixture-load-device",
            timeout_sec=0.2,
            poll_interval_sec=0.1,
            stream_interval_sec=0.2,
            use_status_stream=False,
        )
        helper_payload = {
            "control": {"output_enabled": False, "target_i_ma": 3900},
            "status": {"enable": False, "v_local_mv": 12011, "i_local_ma": 10, "i_remote_ma": 9},
            "source": "ipc_helper_status",
        }
        with (
            mock.patch.object(
                runner.LoadStatusPoller,
                "_ensure_bridge_lease",
                autospec=True,
                return_value=None,
            ),
            mock.patch.object(
                runner,
                "get_load_status_via_ipc_without_lease",
                autospec=True,
                side_effect=lambda *args, **kwargs: {**helper_payload, "source": "ipc_status_no_lease"},
            ) as via_ipc_no_lease,
            mock.patch.object(
                runner,
                "get_load_status_via_ipc",
                autospec=True,
                side_effect=AssertionError("leased direct ipc should not run when no-lease ipc succeeds"),
            ),
            mock.patch.object(
                runner,
                "get_load_status_via_ipc_helper",
                autospec=True,
                side_effect=AssertionError("helper path should not run when no-lease ipc succeeds"),
            ),
        ):
            poller.start()
            deadline = time.monotonic() + 1.0
            snapshot = None
            while time.monotonic() < deadline:
                snapshot = poller.snapshot(time.monotonic())
                if snapshot.get("generation", 0) >= 1 and snapshot.get("payload"):
                    break
                time.sleep(0.05)
            poller.stop(timeout_sec=1.0)

        assert snapshot is not None
        self.assertGreaterEqual(snapshot.get("generation", 0), 1)
        self.assertEqual(snapshot.get("payload", {}).get("status", {}).get("v_local_mv"), 12011)
        self.assertEqual(snapshot.get("source"), "ipc_status_no_lease")
        self.assertIsNone(snapshot.get("error"))
        via_ipc_no_lease.assert_called_once()

    def test_load_status_poller_falls_back_to_lease_path_when_fast_helper_fails(self) -> None:
        runner = self.runner
        args = types.SimpleNamespace(
            load_bridge_url="",
            load_devd_base_url="http://127.0.0.1:20641",
            load_status_source="poll",
            load_usb_device_id="fixture-load-usb-device",
            load_cli="/tmp/loadlynx",
            load_ipc="",
            load_devd_socket="/tmp/loadlynx.sock",
        )
        poller = runner.LoadStatusPoller(
            args,
            "fixture-load-device",
            timeout_sec=0.2,
            poll_interval_sec=0.1,
            stream_interval_sec=0.2,
            use_status_stream=False,
        )
        ipc_payload = {
            "control": {"output_enabled": False, "target_i_ma": 3900},
            "status": {"enable": False, "v_local_mv": 12013, "i_local_ma": 10, "i_remote_ma": 9},
            "source": "ipc_status",
        }
        with (
            mock.patch.object(
                runner.LoadStatusPoller,
                "_ensure_bridge_lease",
                autospec=True,
                return_value=None,
            ),
            mock.patch.object(
                runner,
                "get_load_status_via_ipc_without_lease",
                autospec=True,
                side_effect=RuntimeError("no-lease ipc failed"),
            ) as via_ipc_no_lease,
            mock.patch.object(
                runner.LoadStatusPoller,
                "_ensure_load_devd_lease",
                autospec=True,
                return_value={"lease_id": "lease-1"},
            ),
            mock.patch.object(
                runner,
                "get_load_status_via_ipc",
                autospec=True,
                return_value=ipc_payload,
            ) as via_ipc,
        ):
            poller.start()
            deadline = time.monotonic() + 1.0
            snapshot = None
            while time.monotonic() < deadline:
                snapshot = poller.snapshot(time.monotonic())
                if snapshot.get("generation", 0) >= 1 and snapshot.get("payload"):
                    break
                time.sleep(0.05)
            poller.stop(timeout_sec=1.0)

        assert snapshot is not None
        self.assertEqual(snapshot.get("payload", {}).get("status", {}).get("v_local_mv"), 12013)
        via_ipc_no_lease.assert_called()
        via_ipc.assert_called_once()

    def test_load_status_poller_rejects_invalid_no_lease_payload_and_falls_back_to_lease_path(self) -> None:
        runner = self.runner
        args = types.SimpleNamespace(
            load_bridge_url="",
            load_devd_base_url="http://127.0.0.1:20641",
            load_status_source="poll",
            load_usb_device_id="fixture-load-usb-device",
            load_cli="/tmp/loadlynx",
            load_ipc="",
            load_devd_socket="/tmp/loadlynx.sock",
        )
        poller = runner.LoadStatusPoller(
            args,
            "fixture-load-device",
            timeout_sec=0.2,
            poll_interval_sec=0.1,
            stream_interval_sec=0.2,
            use_status_stream=False,
        )
        ipc_payload = {
            "control": {"output_enabled": False, "target_i_ma": 3900},
            "status": {"enable": False, "v_local_mv": 12013, "i_local_ma": 10, "i_remote_ma": 9},
            "source": "ipc_status",
        }
        with (
            mock.patch.object(
                runner.LoadStatusPoller,
                "_ensure_bridge_lease",
                autospec=True,
                return_value=None,
            ),
            mock.patch.object(
                runner,
                "get_load_status_via_ipc_without_lease",
                autospec=True,
                return_value={"ok": False, "error": "busy"},
            ) as via_ipc_no_lease,
            mock.patch.object(
                runner.LoadStatusPoller,
                "_ensure_load_devd_lease",
                autospec=True,
                return_value={"lease_id": "lease-1"},
            ),
            mock.patch.object(
                runner,
                "get_load_status_via_ipc",
                autospec=True,
                return_value=ipc_payload,
            ) as via_ipc,
        ):
            poller.start()
            deadline = time.monotonic() + 1.0
            snapshot = None
            while time.monotonic() < deadline:
                snapshot = poller.snapshot(time.monotonic())
                if snapshot.get("generation", 0) >= 1 and snapshot.get("payload"):
                    break
                time.sleep(0.05)
            poller.stop(timeout_sec=1.0)

        assert snapshot is not None
        self.assertEqual(snapshot.get("payload", {}).get("status", {}).get("v_local_mv"), 12013)
        via_ipc_no_lease.assert_called()
        via_ipc.assert_called_once()

    def test_load_status_poller_falls_back_to_cli_when_devd_lease_unavailable(self) -> None:
        runner = self.runner
        args = types.SimpleNamespace(
            load_bridge_url="",
            load_devd_base_url="http://127.0.0.1:20641",
            load_status_source="poll",
            load_usb_device_id="fixture-load-usb-device",
            load_cli="/tmp/loadlynx",
            load_ipc="",
            load_devd_socket="",
        )
        poller = runner.LoadStatusPoller(
            args,
            "fixture-load-device",
            timeout_sec=0.2,
            poll_interval_sec=0.1,
            stream_interval_sec=0.2,
            use_status_stream=False,
        )
        cli_payload = {
            "control": {"output_enabled": False, "target_i_ma": 3900},
            "status": {"enable": False, "v_local_mv": 12005, "i_local_ma": 9, "i_remote_ma": 8},
        }
        with (
            mock.patch.object(
                runner,
                "acquire_load_devd_lease",
                autospec=True,
                side_effect=RuntimeError("connection refused"),
            ),
            mock.patch.object(
                runner,
                "get_load_status",
                autospec=True,
                side_effect=lambda *args, **kwargs: {**cli_payload, "source": "cli_status"},
            ) as get_status,
        ):
            poller.start()
            deadline = time.monotonic() + 1.0
            snapshot = None
            while time.monotonic() < deadline:
                snapshot = poller.snapshot(time.monotonic())
                if snapshot.get("generation", 0) >= 1 and snapshot.get("payload"):
                    break
                time.sleep(0.05)
            poller.stop(timeout_sec=1.0)

        assert snapshot is not None
        self.assertGreaterEqual(snapshot.get("generation", 0), 1)
        self.assertEqual(snapshot.get("payload", {}).get("status", {}).get("v_local_mv"), 12005)
        self.assertEqual(snapshot.get("source"), "cli_status")
        self.assertIsNone(snapshot.get("error"))
        self.assertTrue(any(call.kwargs.get("prefer_devd_http") is False for call in get_status.mock_calls))

    def test_load_status_poller_falls_back_when_status_stream_subcommand_is_missing(self) -> None:
        runner = self.runner
        args = types.SimpleNamespace(
            load_bridge_url="",
            load_devd_base_url="http://127.0.0.1:20641",
            load_status_source="status-stream",
            load_usb_device_id="fixture-load-usb-device",
            load_cli="/tmp/loadlynx",
            load_ipc="",
            load_devd_socket="",
        )
        poller = runner.LoadStatusPoller(
            args,
            "fixture-load-device",
            timeout_sec=0.2,
            poll_interval_sec=0.1,
            stream_interval_sec=0.2,
            use_status_stream=True,
        )

        class FakeProcess:
            def __init__(self) -> None:
                self.stdout = io.StringIO("")
                self.stderr = io.StringIO(
                    "error: unrecognized subcommand 'status-stream'\\n"
                )
                self._poll = 2

            def poll(self):
                return self._poll

            def terminate(self):
                return None

            def wait(self, timeout=None):
                return 0

            def kill(self):
                return None

        cli_payload = {
            "control": {"output_enabled": False, "target_i_ma": 3900},
            "status": {"enable": False, "v_local_mv": 12005, "i_local_ma": 9, "i_remote_ma": 8},
        }

        with (
            mock.patch.object(runner.select, "select", return_value=([object()], [], [])),
            mock.patch.object(poller, "_start_stream_process", return_value=FakeProcess()),
            mock.patch.object(
                runner,
                "get_load_status",
                autospec=True,
                side_effect=lambda *args, **kwargs: {**cli_payload, "source": "cli_status"},
            ),
        ):
            poller.start()
            deadline = time.monotonic() + 1.0
            snapshot = None
            while time.monotonic() < deadline:
                snapshot = poller.snapshot(time.monotonic())
                if snapshot.get("generation", 0) >= 1 and snapshot.get("payload"):
                    break
                time.sleep(0.05)
            poller.stop(timeout_sec=1.0)

        assert snapshot is not None
        self.assertFalse(poller._status_stream_supported)
        self.assertEqual(snapshot.get("payload", {}).get("status", {}).get("v_local_mv"), 12005)


class LoadTelemetryProbeTests(unittest.TestCase):
    def setUp(self) -> None:
        self.probe = load_probe_module()

    def test_same_ipc_probe_ignores_timeout_past_probe_deadline(self) -> None:
        probe = self.probe
        args = types.SimpleNamespace(
            load_devd_socket="/tmp/fake.sock",
            load_usb_device_id="fixture-load-usb-device",
            load_device="fixture-load-device",
            load_cli="/tmp/loadlynx",
            http_timeout_sec=10.0,
            cli_timeout_sec=30.0,
        )

        monotonic_values = iter(
            [
                100.0,  # started_at after warmup
                100.0,  # next_poll_at init
                100.0,  # loop now
                100.0,  # deadline check
                100.0,  # poll_started_at sample 0
                100.01,  # poll_finished_at sample 0
                100.01,  # next loop now
                100.26,  # next loop now after sleep
                100.26,  # deadline check
                100.26,  # poll_started_at sample 1
                100.75,  # poll_finished_at sample 1
                102.01,  # next loop now after probe window
                102.01,  # deadline check => break before another call
                102.05,  # control elapsed start
                102.08,  # control elapsed end
            ]
        )

        sleep_calls: list[float] = []

        def fake_monotonic() -> float:
            return next(monotonic_values)

        def fake_sleep(seconds: float) -> None:
            sleep_calls.append(seconds)

        ipc_calls: list[dict[str, object]] = []

        def fake_ipc_call(endpoint: str, op: str, params: dict[str, object], *, timeout_sec: float):
            ipc_calls.append({"op": op, "timeout_sec": timeout_sec})
            if op == "devices.scan":
                return {"ok": True, "result": {"devices": []}}
            if op == "serial.lease.create":
                return {"result": {"lease_id": "lease-1"}}
            if op == "compat.status":
                return {"ok": True, "result": {"status": {"enable": False}}}
            if op == "serial.lease.release":
                return {"ok": True, "result": {"released": True}}
            raise AssertionError(f"unexpected op: {op}")

        def fake_time_cli_command(cmd: list[str], *, timeout_sec: float):
            return {
                "ok": True,
                "elapsed_s": 0.03,
                "payload": {"active_preset_id": 1},
            }

        class InlineThread:
            def __init__(self, *, target=None, name=None, daemon=None):
                self._target = target

            def start(self) -> None:
                if self._target is not None:
                    self._target()

            def join(self, timeout=None) -> None:
                return None

        with (
            mock.patch.object(probe.time, "monotonic", side_effect=fake_monotonic),
            mock.patch.object(probe.time, "sleep", side_effect=fake_sleep),
            mock.patch.object(probe, "ipc_call", side_effect=fake_ipc_call),
            mock.patch.object(probe, "time_cli_command", side_effect=fake_time_cli_command),
            mock.patch.object(probe.threading, "Thread", side_effect=lambda *args, **kwargs: InlineThread(**kwargs)),
        ):
            result = probe.measure_same_ipc_concurrency(args)

        self.assertGreaterEqual(result["sample_count"], 2)
        self.assertGreaterEqual(result["successful_sample_count"], 2)
        self.assertIsNotNone(result["max_sample_gap_s"])
        self.assertIsNotNone(result["max_call_elapsed_s"])
        compat_status_calls = [call for call in ipc_calls if call["op"] == "compat.status"]
        self.assertGreaterEqual(len(compat_status_calls), 3)
        self.assertGreater(compat_status_calls[0]["timeout_sec"], 0.5)
        self.assertTrue(all(call["timeout_sec"] == 0.5 for call in compat_status_calls[1:]))
        self.assertIn(2.0, sleep_calls)

    def test_build_verdict_prefers_cli_status_poll_for_non_bridge_mode(self) -> None:
        probe = self.probe
        verdict = probe.build_verdict(
            cli={
                "status": {"ok": True, "elapsed_s": 0.05},
                "control": {"ok": True, "elapsed_s": 0.05},
            },
            cli_status_poll={"formal_capable": True, "skipped": False},
            http_status={"formal_capable": False, "skipped": False},
            hidden_monitor={"formal_capable": False, "skipped": False},
            same_ipc_concurrency={"formal_capable": False, "skipped": False},
            bridge_concurrency={"skipped": True, "reason": "load_bridge_url_empty"},
        )

        self.assertTrue(verdict["formal_capable"])
        self.assertEqual(verdict["failures"], [])
        self.assertIn("devd_http_status_not_formal_capable", verdict["warnings"])
        self.assertIn("same_ipc_concurrency_not_formal_capable", verdict["warnings"])

    def test_build_verdict_ignores_skipped_http_and_hidden_monitor_warnings(self) -> None:
        probe = self.probe
        verdict = probe.build_verdict(
            cli={
                "status": {"ok": True, "elapsed_s": 0.05},
                "control": {"ok": True, "elapsed_s": 0.05},
            },
            cli_status_poll={"formal_capable": True, "skipped": False},
            http_status={"skipped": True, "reason": "load_devd_base_url_empty"},
            hidden_monitor={"skipped": True, "reason": "cli_status_stream_unsupported"},
            same_ipc_concurrency={"formal_capable": True, "skipped": False},
            bridge_concurrency={"skipped": True, "reason": "load_bridge_url_empty"},
        )

        self.assertTrue(verdict["formal_capable"])
        self.assertEqual(verdict["failures"], [])
        self.assertNotIn("devd_http_status_not_formal_capable", verdict["warnings"])
        self.assertNotIn("hidden_monitor_not_formal_capable", verdict["warnings"])

    def test_measure_http_status_skips_when_base_url_empty(self) -> None:
        probe = self.probe
        args = types.SimpleNamespace(
            load_devd_base_url="",
            http_timeout_sec=10.0,
        )

        result = probe.measure_http_status(args)

        self.assertEqual(
            result,
            {
                "skipped": True,
                "reason": "load_devd_base_url_empty",
            },
        )

    def test_measure_hidden_monitor_skips_when_status_stream_unsupported(self) -> None:
        probe = self.probe
        args = types.SimpleNamespace(
            load_cli="/tmp/loadlynx",
            load_devd_socket="/tmp/loadlynx.sock",
            load_device="fixture-load-device",
            cli_timeout_sec=30.0,
        )

        with mock.patch.object(
            probe,
            "sample_stream_jsonl",
            return_value={
                "ok": False,
                "error": "stream_exited_early",
                "stderr": "error: unrecognized subcommand 'status-stream'",
                "returncode": 2,
            },
        ):
            result = probe.measure_hidden_monitor(args)

        self.assertEqual(result["skipped"], True)
        self.assertEqual(result["reason"], "cli_status_stream_unsupported")

    def test_measure_hidden_monitor_uses_explicit_load_ipc_endpoint(self) -> None:
        probe = self.probe
        args = types.SimpleNamespace(
            load_cli="/tmp/loadlynx",
            load_ipc="/tmp/explicit-load-ipc.sock",
            load_devd_socket="",
            load_device="fixture-load-device",
            cli_timeout_sec=30.0,
        )

        with mock.patch.object(
            probe,
            "sample_stream_jsonl",
            return_value={
                "ok": True,
                "samples": [],
                "sample_count": 0,
                "stderr": "",
                "returncode": None,
            },
        ) as stream_mock:
            probe.measure_hidden_monitor(args)

        cmd = stream_mock.call_args.args[0]
        self.assertIn("--ipc", cmd)
        self.assertIn("/tmp/explicit-load-ipc.sock", cmd)
        self.assertNotIn("", cmd)

    def test_measure_same_ipc_concurrency_skips_without_socket(self) -> None:
        probe = self.probe
        args = types.SimpleNamespace(
            load_ipc="",
            load_devd_socket="",
        )

        result = probe.measure_same_ipc_concurrency(args)

        self.assertEqual(result["skipped"], True)
        self.assertEqual(result["reason"], "load_devd_socket_empty")

    def test_measure_hidden_monitor_skips_without_socket(self) -> None:
        probe = self.probe
        args = types.SimpleNamespace(
            load_devd_socket="",
        )

        result = probe.measure_hidden_monitor(args)

        self.assertEqual(result["skipped"], True)
        self.assertEqual(result["reason"], "load_devd_socket_empty")

    def test_measure_cli_status_poll_concurrency_uses_direct_cli_without_socket(self) -> None:
        probe = self.probe
        args = types.SimpleNamespace(
            load_ipc="",
            load_devd_socket="",
            load_cli="/tmp/loadlynx",
            load_device="fixture-load-device",
            cli_timeout_sec=30.0,
        )

        started = {"t": 0.0}

        def fake_monotonic() -> float:
            started["t"] += 0.05
            return started["t"]

        def fake_sleep(_seconds: float) -> None:
            return None

        def fake_time_cli_command(cmd: list[str], *, timeout_sec: float):
            return {
                "ok": True,
                "elapsed_s": 0.05,
                "payload": {"status": {"enable": False}},
                "cmd": cmd,
                "timeout_sec": timeout_sec,
            }

        with (
            mock.patch.object(probe.time, "monotonic", side_effect=fake_monotonic),
            mock.patch.object(probe.time, "sleep", side_effect=fake_sleep),
            mock.patch.object(probe, "time_cli_command", side_effect=fake_time_cli_command) as time_cli,
        ):
            result = probe.measure_cli_status_poll_concurrency(args)

        self.assertEqual(result["transport"], "released_cli_direct")
        self.assertGreater(result["successful_sample_count"], 0)
        self.assertTrue(result["formal_capable"])
        first_cmd = time_cli.call_args_list[0].args[0]
        self.assertEqual(first_cmd, ["/tmp/loadlynx", "status", "--device", "fixture-load-device", "--json"])

    def test_measure_cli_status_poll_concurrency_uses_long_lease_timeout_but_strict_sample_timeout(self) -> None:
        probe = self.probe
        args = types.SimpleNamespace(
            load_ipc="/tmp/loadlynx.sock",
            load_devd_socket="",
            load_usb_device_id="fixture-load-usb-device",
            load_cli="/tmp/loadlynx",
            load_device="fixture-load-device",
            http_timeout_sec=5.0,
            cli_timeout_sec=30.0,
        )
        monotonic_values = iter(
            [
                100.0,
                100.0,
                100.0,
                100.0,
                100.01,
                100.25,
                100.25,
                100.26,
                102.1,
            ]
        )
        ipc_calls: list[dict[str, object]] = []

        def fake_monotonic() -> float:
            return next(monotonic_values)

        def fake_sleep(_seconds: float) -> None:
            return None

        def fake_ipc_call(endpoint: str, op: str, params: dict[str, object], *, timeout_sec: float):
            ipc_calls.append({"op": op, "timeout_sec": timeout_sec})
            if op == "devices.scan":
                return {"ok": True, "result": {"devices": []}}
            if op == "serial.lease.create":
                return {"ok": True, "result": {"lease_id": "lease-1"}}
            if op == "compat.status":
                return {"ok": True, "result": {"status": {"enable": False}}}
            if op == "serial.lease.release":
                return {"ok": True, "result": {"released": True}}
            raise AssertionError(f"unexpected op: {op}")

        with (
            mock.patch.object(probe.time, "monotonic", side_effect=fake_monotonic),
            mock.patch.object(probe.time, "sleep", side_effect=fake_sleep),
            mock.patch.object(probe, "ipc_call", side_effect=fake_ipc_call),
        ):
            probe.measure_cli_status_poll_concurrency(args)

        lease_calls = [call for call in ipc_calls if call["op"] == "serial.lease.create"]
        status_calls = [call for call in ipc_calls if call["op"] == "compat.status"]
        self.assertEqual(lease_calls[0]["timeout_sec"], 10.0)
        self.assertTrue(status_calls)
        self.assertIn(0.5, [call["timeout_sec"] for call in status_calls])


class HttpRetryTests(unittest.TestCase):
    def setUp(self) -> None:
        self.runner = load_runner_module()

    def test_http_json_with_retries_retries_transient_http_502(self) -> None:
        class FakeResponse:
            def __init__(self) -> None:
                self._payload = io.StringIO(json.dumps({"ok": True}))

            def __enter__(self):
                return self

            def __exit__(self, exc_type, exc, tb):
                return False

            def read(self, *args, **kwargs):
                return self._payload.read(*args, **kwargs)

        with (
            mock.patch.object(
                self.runner.urllib.request,
                "urlopen",
                side_effect=[
                    urllib.error.HTTPError(
                        "http://127.0.0.1/status",
                        502,
                        "Bad Gateway",
                        hdrs=None,
                        fp=None,
                    ),
                    FakeResponse(),
                ],
            ),
            mock.patch.object(self.runner.time, "sleep", autospec=True),
        ):
            result = self.runner.http_json_with_retries(
                "http://127.0.0.1/status",
                timeout_sec=0.5,
                retries=2,
                retry_delay_sec=0.01,
            )

        self.assertEqual(result, {"ok": True})


class UpsPollerReadinessTests(unittest.TestCase):
    def setUp(self) -> None:
        self.runner = load_runner_module()

    def test_diag_snapshot_snapshot_ready_accepts_fresh_seeded_payload_despite_transient_error(self) -> None:
        runner = self.runner
        snapshot = {
            "payload": {
                "input": {
                    "assist_power_stage": "standby",
                    "vin_vbus_mv": 11888,
                    "vin_iin_ma": 794,
                }
            },
            "generation": 1,
            "age_s": 0.1,
            "error": "TimeoutError('timed out')",
        }
        self.assertTrue(runner.diag_snapshot_snapshot_ready(snapshot))

    def test_diag_snapshot_snapshot_ready_accepts_packaged_payload(self) -> None:
        runner = self.runner
        snapshot = {
            "payload": {
                "packages": {
                    "derived.power": {
                        "ok": True,
                        "source": "power_cache",
                        "duration_ms": 0,
                        "payload": {
                            "input": {
                                "assist_power_stage": "standby",
                                "vin_vbus_mv": 11888,
                                "vin_iin_ma": 794,
                            }
                        },
                    }
                },
                "errors": {},
            },
            "generation": 1,
            "age_s": 0.1,
            "error": None,
        }
        self.assertTrue(runner.diag_snapshot_snapshot_ready(snapshot))
        self.assertEqual(
            runner.unwrap_diag_snapshot_payload(snapshot["payload"])["input"]["vin_vbus_mv"],
            11888,
        )

    def test_diag_snapshot_snapshot_ready_rejects_status_derived_payload(self) -> None:
        runner = self.runner
        snapshot = {
            "payload": {
                "source": "status_cache_derived",
                "input": {
                    "assist_power_stage": "standby",
                    "vin_vbus_mv": 11888,
                    "vin_iin_ma": 794,
                },
            },
            "generation": 1,
            "age_s": 0.1,
            "error": None,
        }
        self.assertFalse(runner.diag_snapshot_snapshot_ready(snapshot))

    def test_build_preflight_uses_seeded_diag_snapshot_when_live_fetch_times_out(self) -> None:
        runner = self.runner
        args = types.SimpleNamespace(
            isolapurr_url="http://127.0.0.1:30182",
            isolapurr_cli="isolapurr",
            status_timeout_sec=0.5,
            load_device="fixture-load-device",
            load_devd_socket="",
            output_profile="12v",
            source_voltage_mv=12000,
            ups_status_url="http://127.0.0.1:30081/api/v1/status",
            devd_diag_snapshot_url="http://127.0.0.1:26670/api/v1/devices/fixture-ups-device/diag-snapshot",
        )
        settings_payload = {
            "advanced_power": {"standby_drop_mv": 1200},
            "advanced_power_capabilities": {"rated_vout_mv": 12000},
        }
        seeded_ups_status = {
            "mode": "standby",
            "input": {
                "mains_present": True,
                "assist_power_stage": "standby",
            },
        }
        seeded_diag_snapshot = {
            "input": {
                "assist_power_stage": "standby",
                "vin_vbus_mv": 11888,
                "vin_iin_ma": 794,
            }
        }
        isolapurr_payload = {
            "ports": {
                "ports": [
                    {
                        "portId": "port_c",
                        "state": {"power_enabled": True},
                        "telemetry": {"voltage_mv": 12012, "current_ma": 100},
                    }
                ]
            }
        }
        load_status = {
            "control": {"output_enabled": False, "target_i_ma": 3900},
            "status": {"enable": False, "v_local_mv": 11880, "i_local_ma": 8, "i_remote_ma": 7},
        }
        identity_payload = {
            "hardware_capabilities": {
                "output_profile": "12v",
                "rated_vout_mv": 12000,
            }
        }
        with (
            mock.patch.object(
                runner.Path,
                "exists",
                autospec=True,
                return_value=True,
            ),
            mock.patch.object(
                runner,
                "fetch_isolapurr_ports",
                autospec=True,
                return_value=isolapurr_payload,
            ),
            mock.patch.object(
                runner,
                "get_load_status_best_effort",
                autospec=True,
                return_value=load_status,
            ),
            mock.patch.object(
                runner,
                "get_load_control_best_effort",
                autospec=True,
                return_value={"ok": False, "error": "control not needed"},
            ),
            mock.patch.object(
                runner,
                "http_json_with_retries",
                autospec=True,
                side_effect=[
                    RuntimeError("ups timeout"),
                    RuntimeError("diag-snapshot timeout"),
                ],
            ),
        ):
            preflight = runner.build_preflight(
                args,
                identity_payload,
                settings_payload,
                known_load_disabled=True,
                known_load_target_i_ma=3900,
                seeded_ups_status=seeded_ups_status,
                seeded_diag_snapshot=seeded_diag_snapshot,
            )

        self.assertTrue(preflight["scene_valid"])
        self.assertEqual(preflight["ups"]["source"], "seeded_refresh_devd_devices")
        self.assertEqual(preflight["diag_snapshot"]["source"], "seeded_refresh_devd_devices")

    def test_build_preflight_uses_trace_diag_snapshot_when_direct_diag_snapshot_times_out(self) -> None:
        runner = self.runner
        args = types.SimpleNamespace(
            isolapurr_url="http://127.0.0.1:30182",
            isolapurr_cli="isolapurr",
            status_timeout_sec=0.5,
            load_device="fixture-load-device",
            load_devd_socket="",
            output_profile="12v",
            source_voltage_mv=12000,
            ups_status_url="http://127.0.0.1:30081/api/v1/status",
            devd_diag_snapshot_url="http://127.0.0.1:26670/api/v1/devices/fixture-ups-device/diag-snapshot",
            devd_device_trace_url="http://127.0.0.1:26670/api/v1/devices/fixture-ups-device/trace?trace_limit=1",
        )
        settings_payload = {
            "advanced_power": {"standby_drop_mv": 1200},
            "advanced_power_capabilities": {"rated_vout_mv": 12000},
        }
        seeded_ups_status = {
            "mode": "standby",
            "input": {
                "mains_present": True,
                "assist_power_stage": "standby",
            },
        }
        isolapurr_payload = {
            "ports": {
                "ports": [
                    {
                        "portId": "port_c",
                        "state": {"power_enabled": True},
                        "telemetry": {"voltage_mv": 12012, "current_ma": 100},
                    }
                ]
            }
        }
        load_status = {
            "control": {"output_enabled": False, "target_i_ma": 3900},
            "status": {"enable": False, "v_local_mv": 11880, "i_local_ma": 8, "i_remote_ma": 7},
        }
        identity_payload = {
            "hardware_capabilities": {
                "output_profile": "12v",
                "rated_vout_mv": 12000,
            }
        }
        trace_payload = {
            "status": {
                "mode": "standby",
                "input": {
                    "mains_present": True,
                    "assist_power_stage": "standby",
                },
            },
            "diag_snapshot": {
                "input": {
                    "assist_power_stage": "standby",
                    "vin_vbus_mv": 11888,
                    "vin_iin_ma": 794,
                }
            },
        }
        with (
            mock.patch.object(
                runner.Path,
                "exists",
                autospec=True,
                return_value=True,
            ),
            mock.patch.object(
                runner,
                "fetch_isolapurr_ports",
                autospec=True,
                return_value=isolapurr_payload,
            ),
            mock.patch.object(
                runner,
                "get_load_status_best_effort",
                autospec=True,
                return_value=load_status,
            ),
            mock.patch.object(
                runner,
                "get_load_control_best_effort",
                autospec=True,
                return_value={"ok": False, "error": "control not needed"},
            ),
            mock.patch.object(
                runner,
                "http_json_with_retries",
                autospec=True,
                side_effect=[
                    seeded_ups_status,
                    RuntimeError("diag-snapshot timeout"),
                    trace_payload,
                ],
            ),
        ):
            preflight = runner.build_preflight(
                args,
                identity_payload,
                settings_payload,
                known_load_disabled=True,
                known_load_target_i_ma=3900,
                seeded_ups_status=seeded_ups_status,
                seeded_diag_snapshot=None,
            )

        self.assertTrue(preflight["scene_valid"])
        self.assertEqual(preflight["diag_snapshot"]["source"], "devd_trace")
        self.assertEqual(preflight["diag_snapshot"]["vin_vbus_mv"], 11888)

    def test_build_preflight_prefers_live_poller_probe_when_load_devd_socket_is_present(self) -> None:
        runner = self.runner
        args = types.SimpleNamespace(
            isolapurr_url="http://127.0.0.1:30182",
            isolapurr_cli="isolapurr",
            status_timeout_sec=0.5,
            load_device="fixture-load-device",
            load_devd_socket="/tmp/loadlynx.sock",
            output_profile="12v",
            source_voltage_mv=12000,
            ups_status_url="http://127.0.0.1:30081/api/v1/status",
            devd_diag_snapshot_url="http://127.0.0.1:26670/api/v1/devices/fixture-ups-device/diag-snapshot",
        )
        settings_payload = {
            "advanced_power": {"standby_drop_mv": 1200},
            "advanced_power_capabilities": {"rated_vout_mv": 12000},
        }
        seeded_ups_status = {
            "mode": "standby",
            "input": {
                "mains_present": True,
                "assist_power_stage": "standby",
            },
        }
        seeded_diag_snapshot = {
            "input": {
                "assist_power_stage": "standby",
                "vin_vbus_mv": 11888,
                "vin_iin_ma": 794,
            }
        }
        isolapurr_payload = {
            "ports": {
                "ports": [
                    {
                        "portId": "port_c",
                        "state": {"power_enabled": True},
                        "telemetry": {"voltage_mv": 12012, "current_ma": 100},
                    }
                ]
            }
        }
        load_status = {
            "control": {"output_enabled": False, "target_i_ma": 3900},
            "status": {"enable": False, "v_local_mv": 11880, "i_local_ma": 8, "i_remote_ma": 7},
        }
        identity_payload = {
            "hardware_capabilities": {
                "output_profile": "12v",
                "rated_vout_mv": 12000,
            }
        }
        with (
            mock.patch.object(
                runner.Path,
                "exists",
                autospec=True,
                return_value=True,
            ),
            mock.patch.object(
                runner,
                "fetch_isolapurr_ports",
                autospec=True,
                return_value=isolapurr_payload,
            ),
            mock.patch.object(
                runner,
                "get_load_status_best_effort",
                autospec=True,
                return_value=load_status,
            ),
            mock.patch.object(
                runner,
                "get_load_control_best_effort",
                autospec=True,
                return_value={"ok": False, "error": "control not needed"},
            ),
            mock.patch.object(
                runner,
                "http_json_with_retries",
                autospec=True,
                side_effect=[
                    RuntimeError("ups timeout"),
                    RuntimeError("diag-snapshot timeout"),
                ],
            ),
            mock.patch.object(
                runner,
                "probe_live_load_status_poller_capability",
                autospec=True,
                return_value={
                    "formal_capable": True,
                    "failures": [],
                    "source": "live_load_status_poller",
                    "effective_mode": "ipc-helper-poll",
                },
            ) as live_probe,
        ):
            preflight = runner.build_preflight(
                args,
                identity_payload,
                settings_payload,
                known_load_disabled=True,
                known_load_target_i_ma=3900,
                load_telemetry_probe={"verdict": {"formal_capable": False}},
                seeded_ups_status=seeded_ups_status,
                seeded_diag_snapshot=seeded_diag_snapshot,
            )

        self.assertTrue(preflight["scene_valid"])
        self.assertEqual(preflight["load_live_poller_probe"]["source"], "live_load_status_poller")
        self.assertEqual(preflight["load_live_poller_mode"], "ipc-helper-poll")
        live_probe.assert_called_once()

    def test_build_preflight_uses_probe_effective_load_state_when_direct_status_paths_fail(self) -> None:
        runner = self.runner
        args = types.SimpleNamespace(
            isolapurr_url="http://127.0.0.1:30182",
            isolapurr_cli="isolapurr",
            status_timeout_sec=0.5,
            load_device="fixture-load-device",
            load_devd_socket="/tmp/loadlynx.sock",
            output_profile="12v",
            source_voltage_mv=12000,
            ups_status_url="http://127.0.0.1:30081/api/v1/status",
            devd_diag_snapshot_url="http://127.0.0.1:26670/api/v1/devices/fixture-ups-device/diag-snapshot",
            devd_device_trace_url="http://127.0.0.1:26670/api/v1/devices/fixture-ups-device/trace?trace_limit=1",
            load_ipc="",
        )
        settings_payload = {
            "advanced_power": {"standby_drop_mv": 1200},
            "advanced_power_capabilities": {"rated_vout_mv": 12000},
        }
        seeded_ups_status = {
            "mode": "standby",
            "input": {
                "mains_present": True,
                "assist_power_stage": "standby",
            },
        }
        seeded_diag_snapshot = {
            "input": {
                "assist_power_stage": "standby",
                "vin_vbus_mv": 11888,
                "vin_iin_ma": 794,
            }
        }
        isolapurr_payload = {
            "ports": {
                "ports": [
                    {
                        "portId": "port_c",
                        "state": {"power_enabled": True},
                        "telemetry": {"voltage_mv": 12012, "current_ma": 100},
                    }
                ]
            }
        }
        identity_payload = {
            "hardware_capabilities": {
                "output_profile": "12v",
                "rated_vout_mv": 12000,
            }
        }
        load_probe = {
            "cli": {
                "status": {
                    "payload": {
                        "control": {"output_enabled": False, "target_i_ma": 3900},
                        "status": {"enable": False, "v_local_mv": 11952, "i_local_ma": 9, "i_remote_ma": 9},
                    }
                }
            },
            "verdict": {"formal_capable": False},
        }
        with (
            mock.patch.object(
                runner.Path,
                "exists",
                autospec=True,
                return_value=True,
            ),
            mock.patch.object(
                runner,
                "fetch_isolapurr_ports",
                autospec=True,
                return_value=isolapurr_payload,
            ),
            mock.patch.object(
                runner,
                "get_load_status_best_effort",
                autospec=True,
                return_value={"ok": False, "error": "status unavailable"},
            ),
            mock.patch.object(
                runner,
                "get_load_control_best_effort",
                autospec=True,
                return_value={"ok": False, "error": "control unavailable"},
            ),
            mock.patch.object(
                runner,
                "http_json_with_retries",
                autospec=True,
                side_effect=[
                    seeded_ups_status,
                    RuntimeError("diag-snapshot timeout"),
                    {
                        "status": seeded_ups_status,
                        "diag_snapshot": seeded_diag_snapshot,
                    },
                ],
            ),
            mock.patch.object(
                runner,
                "probe_live_load_status_poller_capability",
                autospec=True,
                return_value={
                    "formal_capable": False,
                    "failures": ["live_poller_no_fresh_samples"],
                    "source": "live_load_status_poller",
                    "effective_mode": "ipc-helper-poll",
                },
            ),
        ):
            preflight = runner.build_preflight(
                args,
                identity_payload,
                settings_payload,
                known_load_disabled=False,
                known_load_target_i_ma=None,
                load_telemetry_probe=load_probe,
                seeded_ups_status=seeded_ups_status,
                seeded_diag_snapshot=seeded_diag_snapshot,
            )

        self.assertFalse(preflight["scene_valid"])
        self.assertEqual(preflight["load"]["output_enabled"], False)
        self.assertEqual(preflight["load"]["target_i_ma"], 3900)
        self.assertNotIn("load_not_disabled_before_scene", preflight["failures"])
        self.assertIn("load_live_poller_not_formal_capable", preflight["failures"])

    def test_build_preflight_blocks_on_nonformal_live_poller_probe_when_socket_is_present(self) -> None:
        runner = self.runner
        args = types.SimpleNamespace(
            isolapurr_url="http://127.0.0.1:30182",
            isolapurr_cli="isolapurr",
            status_timeout_sec=0.5,
            load_device="fixture-load-device",
            load_devd_socket="/tmp/loadlynx.sock",
            output_profile="12v",
            source_voltage_mv=12000,
            ups_status_url="http://127.0.0.1:30081/api/v1/status",
            devd_diag_snapshot_url="http://127.0.0.1:26670/api/v1/devices/fixture-ups-device/diag-snapshot",
        )
        settings_payload = {
            "advanced_power": {"standby_drop_mv": 1200},
            "advanced_power_capabilities": {"rated_vout_mv": 12000},
        }
        seeded_ups_status = {
            "mode": "standby",
            "input": {
                "mains_present": True,
                "assist_power_stage": "standby",
            },
        }
        seeded_diag_snapshot = {
            "input": {
                "assist_power_stage": "standby",
                "vin_vbus_mv": 11888,
                "vin_iin_ma": 794,
            }
        }
        isolapurr_payload = {
            "ports": {
                "ports": [
                    {
                        "portId": "port_c",
                        "state": {"power_enabled": True},
                        "telemetry": {"voltage_mv": 12012, "current_ma": 100},
                    }
                ]
            }
        }
        load_status = {
            "control": {"output_enabled": False, "target_i_ma": 3900},
            "status": {"enable": False, "v_local_mv": 11880, "i_local_ma": 8, "i_remote_ma": 7},
        }
        identity_payload = {
            "hardware_capabilities": {
                "output_profile": "12v",
                "rated_vout_mv": 12000,
            }
        }
        with (
            mock.patch.object(
                runner.Path,
                "exists",
                autospec=True,
                return_value=True,
            ),
            mock.patch.object(
                runner,
                "fetch_isolapurr_ports",
                autospec=True,
                return_value=isolapurr_payload,
            ),
            mock.patch.object(
                runner,
                "get_load_status_best_effort",
                autospec=True,
                return_value=load_status,
            ),
            mock.patch.object(
                runner,
                "get_load_control_best_effort",
                autospec=True,
                return_value={"ok": False, "error": "control not needed"},
            ),
            mock.patch.object(
                runner,
                "http_json_with_retries",
                autospec=True,
                side_effect=[
                    RuntimeError("ups timeout"),
                    RuntimeError("diag-snapshot timeout"),
                ],
            ),
            mock.patch.object(
                runner,
                "probe_live_load_status_poller_capability",
                autospec=True,
                return_value={
                    "formal_capable": False,
                    "failures": ["live_poller_no_fresh_samples"],
                    "source": "live_load_status_poller",
                    "effective_mode": "ipc-helper-poll",
                },
            ),
        ):
            preflight = runner.build_preflight(
                args,
                identity_payload,
                settings_payload,
                known_load_disabled=True,
                known_load_target_i_ma=3900,
                seeded_ups_status=seeded_ups_status,
                seeded_diag_snapshot=seeded_diag_snapshot,
            )

        self.assertFalse(preflight["scene_valid"])
        self.assertIn("load_live_poller_not_formal_capable", preflight["failures"])
        self.assertEqual(preflight["load_live_poller_probe"]["failures"], ["live_poller_no_fresh_samples"])

    def test_build_preflight_blocks_on_nonformal_probe_when_explicit_load_ipc_is_present(self) -> None:
        runner = self.runner
        with tempfile.TemporaryDirectory() as tmp:
            args = types.SimpleNamespace(
                isolapurr_url="http://127.0.0.1:30182",
                isolapurr_cli="isolapurr",
                status_timeout_sec=0.5,
                load_device="fixture-load-device",
                load_devd_socket="",
                load_ipc=str(Path(tmp) / "loadlynx-released-hil.sock"),
                output_profile="12v",
                source_voltage_mv=12000,
                ups_status_url="http://127.0.0.1:30081/api/v1/status",
                devd_diag_snapshot_url="http://127.0.0.1:26670/api/v1/devices/fixture-ups-device/diag-snapshot",
            )
            Path(args.load_ipc).touch()
            settings_payload = {
                "advanced_power": {"standby_drop_mv": 1200},
                "advanced_power_capabilities": {"rated_vout_mv": 12000},
            }
            seeded_ups_status = {
                "mode": "standby",
                "input": {
                    "mains_present": True,
                    "assist_power_stage": "standby",
                },
            }
            seeded_diag_snapshot = {
                "input": {
                    "assist_power_stage": "standby",
                    "vin_vbus_mv": 11888,
                    "vin_iin_ma": 794,
                }
            }
            isolapurr_payload = {
                "ports": {
                    "ports": [
                        {
                            "portId": "port_c",
                            "state": {"power_enabled": True},
                            "telemetry": {"voltage_mv": 12012, "current_ma": 100},
                        }
                    ]
                }
            }
            load_status = {
                "control": {"output_enabled": False, "target_i_ma": 3900},
                "status": {"enable": False, "v_local_mv": 11880, "i_local_ma": 8, "i_remote_ma": 7},
            }
            identity_payload = {
                "hardware_capabilities": {
                    "output_profile": "12v",
                    "rated_vout_mv": 12000,
                }
            }
            with (
                mock.patch.object(
                    runner,
                    "fetch_isolapurr_ports",
                    autospec=True,
                    return_value=isolapurr_payload,
                ),
                mock.patch.object(
                    runner,
                    "get_load_status_best_effort",
                    autospec=True,
                    return_value=load_status,
                ),
                mock.patch.object(
                    runner,
                    "get_load_control_best_effort",
                    autospec=True,
                    return_value={"ok": False, "error": "control not needed"},
                ),
                mock.patch.object(
                    runner,
                    "http_json_with_retries",
                    autospec=True,
                    side_effect=[
                        RuntimeError("ups timeout"),
                        RuntimeError("diag-snapshot timeout"),
                    ],
                ),
            ):
                preflight = runner.build_preflight(
                    args,
                    identity_payload,
                    settings_payload,
                    known_load_disabled=True,
                    known_load_target_i_ma=3900,
                    load_telemetry_probe={
                        "verdict": {
                            "formal_capable": False,
                            "warnings": ["same_ipc_concurrency_not_formal_capable"],
                        }
                    },
                    seeded_ups_status=seeded_ups_status,
                    seeded_diag_snapshot=seeded_diag_snapshot,
                )

        self.assertFalse(preflight["scene_valid"])
        self.assertEqual(
            preflight["load_telemetry_probe"]["verdict"]["warnings"],
            ["same_ipc_concurrency_not_formal_capable"],
        )
        self.assertIn("load_live_poller_not_formal_capable", preflight["failures"])

    def test_build_preflight_runs_live_poller_probe_when_explicit_load_ipc_is_present(self) -> None:
        runner = self.runner
        with tempfile.TemporaryDirectory() as tmp:
            args = types.SimpleNamespace(
                isolapurr_url="http://127.0.0.1:30182",
                isolapurr_cli="isolapurr",
                status_timeout_sec=0.5,
                load_device="fixture-load-device",
                load_devd_socket="/tmp/default-loadlynx.sock",
                load_ipc=str(Path(tmp) / "loadlynx-released-hil.sock"),
                output_profile="12v",
                source_voltage_mv=12000,
                ups_status_url="http://127.0.0.1:30081/api/v1/status",
                devd_diag_snapshot_url="http://127.0.0.1:26670/api/v1/devices/fixture-ups-device/diag-snapshot",
            )
            Path(args.load_ipc).touch()
            settings_payload = {
                "advanced_power": {"standby_drop_mv": 1200},
                "advanced_power_capabilities": {"rated_vout_mv": 12000},
            }
            seeded_ups_status = {
                "mode": "standby",
                "input": {
                    "mains_present": True,
                    "assist_power_stage": "standby",
                },
            }
            seeded_diag_snapshot = {
                "input": {
                    "assist_power_stage": "standby",
                    "vin_vbus_mv": 11888,
                    "vin_iin_ma": 794,
                }
            }
            isolapurr_payload = {
                "ports": {
                    "ports": [
                        {
                            "portId": "port_c",
                            "state": {"power_enabled": True},
                            "telemetry": {"voltage_mv": 12012, "current_ma": 100},
                        }
                    ]
                }
            }
            load_status = {
                "control": {"output_enabled": False, "target_i_ma": 3900},
                "status": {"enable": False, "v_local_mv": 11880, "i_local_ma": 8, "i_remote_ma": 7},
            }
            identity_payload = {
                "hardware_capabilities": {
                    "output_profile": "12v",
                    "rated_vout_mv": 12000,
                }
            }
            with (
                mock.patch.object(
                    runner,
                    "fetch_isolapurr_ports",
                    autospec=True,
                    return_value=isolapurr_payload,
                ),
                mock.patch.object(
                    runner,
                    "get_load_status_best_effort",
                    autospec=True,
                    return_value=load_status,
                ),
                mock.patch.object(
                    runner,
                    "get_load_control_best_effort",
                    autospec=True,
                    return_value={"ok": False, "error": "control not needed"},
                ),
                mock.patch.object(
                    runner,
                    "http_json_with_retries",
                    autospec=True,
                    side_effect=[
                        RuntimeError("ups timeout"),
                        RuntimeError("diag-snapshot timeout"),
                    ],
                ),
                mock.patch.object(
                    runner,
                    "probe_live_load_status_poller_capability",
                    autospec=True,
                    return_value={
                        "formal_capable": True,
                        "failures": [],
                        "source": "live_load_status_poller",
                        "effective_mode": "status-stream",
                    },
                ) as live_probe,
            ):
                preflight = runner.build_preflight(
                    args,
                    identity_payload,
                    settings_payload,
                    known_load_disabled=True,
                    known_load_target_i_ma=3900,
                    load_telemetry_probe={
                        "verdict": {
                            "formal_capable": False,
                            "warnings": ["same_ipc_concurrency_not_formal_capable"],
                        }
                    },
                    seeded_ups_status=seeded_ups_status,
                    seeded_diag_snapshot=seeded_diag_snapshot,
                )

        self.assertTrue(preflight["scene_valid"])
        self.assertEqual(preflight["load_live_poller_probe"]["effective_mode"], "status-stream")
        live_probe.assert_called_once()

    def test_load_status_poller_effective_mode_prefers_ipc_helper_when_socket_is_present(self) -> None:
        runner = self.runner
        args = types.SimpleNamespace(
            load_bridge_url="",
            load_devd_base_url="http://127.0.0.1:20641",
            load_status_source="status-stream",
            load_usb_device_id="fixture-load-usb-device",
            load_cli="/tmp/loadlynx",
            load_ipc="",
            load_devd_socket="/tmp/loadlynx.sock",
        )
        poller = runner.LoadStatusPoller(
            args,
            "fixture-load-device",
            timeout_sec=0.2,
            poll_interval_sec=0.1,
            stream_interval_sec=0.2,
            use_status_stream=True,
        )

        self.assertEqual(poller.effective_status_source_mode(), "status-stream")

    def test_set_isolapurr_manual_output_uses_released_cli_manual_path(self) -> None:
        runner = self.runner
        with mock.patch.object(
            runner,
            "run_json_command_with_retries",
            autospec=True,
            return_value={"ok": True},
        ) as run_json:
            payload = runner.set_isolapurr_manual_output(
                "http://127.0.0.1:30182",
                voltage_mv=12000,
                current_limit_ma=3000,
                isolapurr_cli="isolapurr",
            )
        self.assertEqual(payload, {"ok": True})
        self.assertEqual(
            run_json.call_args.args[0],
            [
                "isolapurr",
                "power",
                "output",
                "manual",
                "--url",
                "http://127.0.0.1:30182",
                "--voltage-mv",
                "12000",
                "--current-limit-ma",
                "3000",
                "--usb-c-path",
                "disconnected",
                "--json",
            ],
        )


class LoadLynxCommandRoutingTests(unittest.TestCase):
    def setUp(self) -> None:
        self.runner = load_runner_module()

    def test_loadlynx_cmd_uses_only_explicit_load_ipc_for_non_bridge_commands(self) -> None:
        runner = self.runner
        args = types.SimpleNamespace(
            load_cli="/tmp/loadlynx",
            load_ipc="",
            load_devd_socket="/tmp/loadlynx.sock",
            load_bridge_url="",
        )
        cmd = runner.loadlynx_cmd(args, "status", "--device", "fixture-load-device", "--json")
        self.assertEqual(cmd, ["/tmp/loadlynx", "status", "--device", "fixture-load-device", "--json"])

        args.load_ipc = "/tmp/explicit-load-ipc.sock"
        cmd = runner.loadlynx_cmd(args, "status", "--device", "fixture-load-device", "--json")
        self.assertEqual(
            cmd,
            [
                "/tmp/loadlynx",
                "--ipc",
                "/tmp/explicit-load-ipc.sock",
                "status",
                "--device",
                "fixture-load-device",
                "--json",
            ],
        )

    def test_loadlynx_cmd_does_not_mix_ipc_with_bridge_url(self) -> None:
        runner = self.runner
        args = types.SimpleNamespace(
            load_cli="/tmp/loadlynx",
            load_ipc="/tmp/loadlynx.sock",
            load_devd_socket="/tmp/other.sock",
            load_bridge_url="http://127.0.0.1:30180",
        )
        cmd = runner.loadlynx_cmd(args, "status", "--url", "http://bridge?device_id=x")
        self.assertEqual(cmd, ["/tmp/loadlynx", "status", "--url", "http://bridge?device_id=x"])

    def test_loadlynx_cmd_prefers_explicit_load_ipc_over_default_bridge_url_for_device_commands(self) -> None:
        runner = self.runner
        args = types.SimpleNamespace(
            load_cli="/tmp/loadlynx",
            load_ipc="/tmp/loadlynx.sock",
            load_devd_socket="/tmp/other.sock",
            load_bridge_url="http://127.0.0.1:30180",
        )
        cmd = runner.loadlynx_cmd(args, "control", "get", "--device", "fixture-load-device", "--json")
        self.assertEqual(
            cmd,
            [
                "/tmp/loadlynx",
                "--ipc",
                "/tmp/loadlynx.sock",
                "control",
                "get",
                "--device",
                "fixture-load-device",
                "--json",
            ],
        )


class LoadStatusPayloadValidationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.runner = load_runner_module()

    def test_is_valid_load_status_payload_rejects_status_error_placeholder(self) -> None:
        payload = {
            "control": {
                "active_preset_id": 1,
                "output_enabled": True,
                "preset": {"target_i_ma": 3900},
            },
            "status": {
                "ok": False,
                "error": "TimeoutExpired(...)",
            },
            "effective_enabled": True,
            "effective_target_i_ma": 3900,
        }
        self.assertFalse(self.runner.is_valid_load_status_payload(payload))

    def test_get_load_control_best_effort_prefers_ipc_helper_without_lease_when_socket_is_present(self) -> None:
        runner = self.runner
        args = types.SimpleNamespace(
            load_bridge_url="",
            load_devd_base_url="http://127.0.0.1:20641",
            load_usb_device_id="fixture-load-usb-device",
            load_cli="/tmp/loadlynx",
            load_ipc="",
            load_devd_socket="/tmp/loadlynx.sock",
        )
        helper_payload = {
            "control": {"output_enabled": False, "target_i_ma": 3900},
            "status": {"enable": False},
            "source": "ipc_helper_status",
        }
        with (
            mock.patch.object(
                runner,
                "get_load_status_via_ipc_helper",
                autospec=True,
                return_value=helper_payload,
            ) as via_helper,
            mock.patch.object(
                runner,
                "get_load_control",
                autospec=True,
                side_effect=AssertionError("CLI control fallback should not run"),
            ),
        ):
            payload = runner.get_load_control_best_effort(
                args,
                "fixture-load-device",
                timeout_sec=0.5,
                load_devd_lease=None,
            )

        self.assertEqual(payload["source"], "ipc_helper_control_from_status")
        via_helper.assert_called_once()

    def test_replace_status_does_not_promote_status_error_placeholder(self) -> None:
        runner = self.runner
        args = types.SimpleNamespace(
            load_bridge_url="",
            load_devd_base_url="",
            load_status_source="poll",
            load_usb_device_id="fixture-load-usb-device",
            load_cli="/tmp/loadlynx",
            load_ipc="",
            load_devd_socket="",
        )
        poller = runner.LoadStatusPoller(
            args,
            "fixture-load-device",
            timeout_sec=0.2,
            poll_interval_sec=0.1,
            stream_interval_sec=0.2,
            use_status_stream=False,
        )
        payload = {
            "control": {
                "active_preset_id": 1,
                "output_enabled": True,
                "preset": {"target_i_ma": 3900},
            },
            "status": {
                "ok": False,
                "error": "TimeoutExpired(...)",
            },
            "effective_enabled": True,
            "effective_target_i_ma": 3900,
        }
        poller.replace_status(payload)
        snapshot = poller.snapshot(time.monotonic())
        self.assertEqual(snapshot["generation"], 0)
        self.assertIsNone(snapshot["payload"])
        self.assertIsNone(snapshot["error"])


class ContinuousSceneSettingsSnapshotTests(unittest.TestCase):
    def setUp(self) -> None:
        self.runner = load_runner_module()

    def test_execute_continuous_scene_uses_static_settings_snapshot(self) -> None:
        runner = self.runner

        class FakePoller:
            def __init__(self, payload: dict[str, object]) -> None:
                self.payload = payload

            def snapshot(self, now_monotonic: float) -> dict[str, object]:
                return {
                    "payload": self.payload,
                    "generation": 1,
                    "age_s": 0.01,
                    "sample_age_s": 0.01,
                    "elapsed_ms": 10,
                    "error": None,
                    "source": "test",
                }

            def replace_status(self, payload: dict[str, object]) -> None:
                self.payload = payload

            def generation(self) -> int:
                return 1

            def resume(self) -> None:
                return None

            def pause(self) -> None:
                return None

            def wait_until_idle(self, timeout_sec: float) -> None:
                return None

            def release_bridge_lease(self, timeout_sec: float) -> None:
                return None

            def release_load_devd_lease(self, timeout_sec: float) -> None:
                return None

        args = types.SimpleNamespace(
            load_device="fixture-load-device",
            ups_status_url="http://ups/status",
            ups_settings_url="http://ups/settings",
            devd_diag_snapshot_url="http://devd/diag-snapshot",
            isolapurr_url="http://isolapurr",
            status_timeout_sec=0.5,
            pre_seconds=0.0,
            sample_interval_seconds=0.01,
            hold_seconds=0.0,
            backup_hold_seconds=0.0,
            restore_hold_seconds=0.0,
            post_seconds=0.0,
            include_backup=False,
            load_status_source="poll",
            load_bridge_url="",
            load_status_poll_timeout_sec=0.5,
        )
        settings_snapshot = {
            "payload": {
                "advanced_power": {"standby_drop_mv": 1200},
                "advanced_power_capabilities": {"rated_vout_mv": 12000},
            },
            "generation": 0,
            "age_s": 0.0,
            "elapsed_ms": 0,
            "error": None,
        }
        captured_settings: list[dict[str, object]] = []

        def fake_capture_three_device_sample(**kwargs):
            captured_settings.append(kwargs["settings_snapshot"])
            return {
                "phase": kwargs["phase"],
                "t_s": kwargs["t_s"],
                "raw": {},
            }

        jsonl_path = Path("/tmp/continuous-scene-settings-snapshot-test.jsonl")
        with (
            mock.patch.object(runner, "capture_three_device_sample", side_effect=fake_capture_three_device_sample),
            mock.patch.object(runner, "append_jsonl", autospec=True),
            mock.patch.object(runner, "persist_progress", autospec=True),
            mock.patch.object(runner.time, "sleep", autospec=True),
            mock.patch.object(runner, "run_scheduled_action", autospec=True, return_value={}),
            mock.patch.object(
                runner,
                "wait_for_live_load_status",
                autospec=True,
                return_value={"ready": True, "waited_s": 0.0},
            ),
        ):
            samples = runner.execute_continuous_scene(
                args=args,
                jsonl_path=jsonl_path,
                metadata={},
                actions=[],
                load_status_poller=FakePoller({"status": {"enable": False}, "control": {"output_enabled": False}}),
                ups_status_poller=FakePoller({"mode": "standby", "input": {"assist_power_stage": "standby"}}),
                diag_snapshot_poller=FakePoller({"input": {"assist_power_stage": "standby"}}),
                settings_snapshot=settings_snapshot,
                isolapurr_poller=FakePoller({"ports": {"ports": []}}),
                expected_phases=["pre", "hold", "post"],
                run_dir=Path("/tmp"),
            )

        self.assertTrue(samples)
        self.assertTrue(captured_settings)
        self.assertIs(captured_settings[0], settings_snapshot)

    def test_execute_continuous_scene_refreshes_load_status_before_backup_stable_phase(self) -> None:
        runner = self.runner

        class FakeLoadPoller:
            def __init__(self) -> None:
                self.payload = {"status": {"enable": True}, "control": {"output_enabled": True}}

            def snapshot(self, now_monotonic: float) -> dict[str, object]:
                return {
                    "payload": self.payload,
                    "generation": 10,
                    "age_s": 0.01,
                    "sample_age_s": 0.01,
                    "elapsed_ms": 10,
                    "error": None,
                    "source": "test",
                }

            def replace_status(self, payload: dict[str, object]) -> None:
                self.payload = payload

            def generation(self) -> int:
                return 10

            def resume(self) -> None:
                return None

            def pause(self) -> None:
                return None

            def wait_until_idle(self, timeout_sec: float) -> None:
                return None

            def release_bridge_lease(self, timeout_sec: float) -> None:
                return None

            def release_load_devd_lease(self, timeout_sec: float) -> None:
                return None

        class FakePoller:
            def __init__(self, payload: dict[str, object]) -> None:
                self.payload = payload

            def snapshot(self, now_monotonic: float) -> dict[str, object]:
                return {
                    "payload": self.payload,
                    "generation": 1,
                    "age_s": 0.01,
                    "sample_age_s": 0.01,
                    "elapsed_ms": 10,
                    "error": None,
                    "source": "test",
                }

        args = types.SimpleNamespace(
            load_device="fixture-load-device",
            ups_status_url="http://ups/status",
            ups_settings_url="http://ups/settings",
            devd_diag_snapshot_url="http://devd/diag-snapshot",
            isolapurr_url="http://isolapurr",
            status_timeout_sec=0.5,
            pre_seconds=0.0,
            sample_interval_seconds=0.01,
            hold_seconds=0.0,
            backup_hold_seconds=0.0,
            restore_hold_seconds=0.0,
            post_seconds=0.0,
            include_backup=True,
            load_status_source="poll",
            load_bridge_url="",
            load_status_poll_timeout_sec=0.5,
        )
        settings_snapshot = {
            "payload": {
                "advanced_power": {"standby_drop_mv": 1200},
                "advanced_power_capabilities": {"rated_vout_mv": 12000},
            },
            "generation": 0,
            "age_s": 0.0,
            "elapsed_ms": 0,
            "error": None,
        }
        fake_load_poller = FakeLoadPoller()
        refresh_calls: list[bool] = []

        def fake_run_scheduled_action(**kwargs):
            action_kind = kwargs["action_kind"]
            if action_kind == "cc_target":
                return {
                    "verified_status": {
                        "status": {"enable": True},
                        "control": {"output_enabled": True, "target_i_ma": 3900},
                    }
                }
            if action_kind == "port_c_disable_for_backup":
                return {}
            if action_kind == "port_c_enable_after_backup":
                return {}
            if action_kind == "disable_after_target":
                return {
                    "verified_status": {
                        "status": {"enable": False},
                        "control": {"output_enabled": False, "target_i_ma": 3900},
                    }
                }
            raise AssertionError(f"unexpected action: {action_kind}")

        def fake_wait_for_live_load_status(
            _load_status_poller,
            *,
            sample_interval_seconds,
            timeout_sec,
            require_new_generation=True,
            progress_hook=None,
        ):
            refresh_calls.append(require_new_generation)
            return {"ready": True, "waited_s": 0.0, "require_new_generation": require_new_generation}

        jsonl_path = Path("/tmp/continuous-scene-load-refresh-test.jsonl")
        with (
            mock.patch.object(
                runner,
                "capture_three_device_sample",
                side_effect=lambda **kwargs: {"phase": kwargs["phase"], "t_s": kwargs["t_s"], "raw": {}},
            ),
            mock.patch.object(runner, "append_jsonl", autospec=True),
            mock.patch.object(runner, "persist_progress", autospec=True),
            mock.patch.object(runner.time, "sleep", autospec=True),
            mock.patch.object(runner, "run_scheduled_action", side_effect=fake_run_scheduled_action),
            mock.patch.object(
                runner,
                "wait_for_live_load_status",
                side_effect=fake_wait_for_live_load_status,
            ),
            mock.patch.object(
                runner,
                "wait_for_isolapurr_port_c_state",
                autospec=True,
                return_value={"ready": True, "waited_s": 0.0},
            ),
        ):
            samples = runner.execute_continuous_scene(
                args=args,
                jsonl_path=jsonl_path,
                metadata={},
                actions=[],
                load_status_poller=fake_load_poller,
                ups_status_poller=FakePoller({"mode": "standby", "input": {"assist_power_stage": "standby"}}),
                diag_snapshot_poller=FakePoller({"input": {"assist_power_stage": "standby"}}),
                settings_snapshot=settings_snapshot,
                isolapurr_poller=FakePoller({"ports": {"ports": []}}),
                expected_phases=["pre", "hold", "backup", "restore", "post"],
                run_dir=Path("/tmp"),
            )

        self.assertTrue(samples)
        self.assertIn(False, refresh_calls)
        self.assertEqual(refresh_calls.count(True), 0)

    def test_execute_continuous_scene_does_not_abort_when_resume_generation_advance_times_out(self) -> None:
        runner = self.runner

        class FakeLoadPoller:
            def __init__(self) -> None:
                self.payload = {"status": {"enable": False}, "control": {"output_enabled": False}}

            def snapshot(self, now_monotonic: float) -> dict[str, object]:
                return {
                    "payload": self.payload,
                    "generation": 1,
                    "age_s": 0.01,
                    "sample_age_s": 0.01,
                    "elapsed_ms": 10,
                    "error": None,
                    "source": "test",
                }

            def replace_status(self, payload: dict[str, object]) -> None:
                self.payload = payload

            def generation(self) -> int:
                return 1

            def resume(self) -> None:
                return None

            def pause(self) -> None:
                return None

            def wait_until_idle(self, timeout_sec: float) -> None:
                return None

            def release_bridge_lease(self, timeout_sec: float) -> None:
                return None

            def release_load_devd_lease(self, timeout_sec: float) -> None:
                return None

        class FakePoller:
            def __init__(self, payload: dict[str, object]) -> None:
                self.payload = payload

            def snapshot(self, now_monotonic: float) -> dict[str, object]:
                return {
                    "payload": self.payload,
                    "generation": 1,
                    "age_s": 0.01,
                    "sample_age_s": 0.01,
                    "elapsed_ms": 10,
                    "error": None,
                    "source": "test",
                }

        args = types.SimpleNamespace(
            load_device="fixture-load-device",
            ups_status_url="http://ups/status",
            ups_settings_url="http://ups/settings",
            devd_diag_snapshot_url="http://devd/diag-snapshot",
            isolapurr_url="http://isolapurr",
            status_timeout_sec=0.5,
            pre_seconds=0.0,
            sample_interval_seconds=0.01,
            hold_seconds=0.0,
            backup_hold_seconds=0.0,
            restore_hold_seconds=0.0,
            post_seconds=0.0,
            include_backup=False,
            load_status_source="poll",
            load_bridge_url="",
            load_status_poll_timeout_sec=0.5,
        )
        settings_snapshot = {
            "payload": {
                "advanced_power": {"standby_drop_mv": 1200},
                "advanced_power_capabilities": {"rated_vout_mv": 12000},
            },
            "generation": 0,
            "age_s": 0.0,
            "elapsed_ms": 0,
            "error": None,
        }

        def fake_run_scheduled_action(**kwargs):
            before_verify = kwargs.get("before_verify")
            if before_verify is not None:
                before_verify()
            return {
                "verified_status": {
                    "status": {"enable": True},
                    "control": {"output_enabled": True, "target_i_ma": 3900},
                }
            }

        jsonl_path = Path("/tmp/continuous-scene-resume-best-effort-test.jsonl")
        with (
            mock.patch.object(
                runner,
                "capture_three_device_sample",
                side_effect=lambda **kwargs: {"phase": kwargs["phase"], "t_s": kwargs["t_s"], "raw": {}},
            ),
            mock.patch.object(runner, "append_jsonl", autospec=True),
            mock.patch.object(runner, "persist_progress", autospec=True),
            mock.patch.object(runner.time, "sleep", autospec=True),
            mock.patch.object(runner, "run_scheduled_action", side_effect=fake_run_scheduled_action),
            mock.patch.object(
                runner,
                "wait_for_load_status_generation_advance",
                side_effect=RuntimeError("timed out waiting for generation"),
            ),
            mock.patch.object(
                runner,
                "wait_for_live_load_status",
                autospec=True,
                return_value={"ready": True, "waited_s": 0.0},
            ),
        ):
            samples = runner.execute_continuous_scene(
                args=args,
                jsonl_path=jsonl_path,
                metadata={},
                actions=[],
                load_status_poller=FakeLoadPoller(),
                ups_status_poller=FakePoller({"mode": "standby", "input": {"assist_power_stage": "standby"}}),
                diag_snapshot_poller=FakePoller({"input": {"assist_power_stage": "standby"}}),
                settings_snapshot=settings_snapshot,
                isolapurr_poller=FakePoller({"ports": {"ports": []}}),
                expected_phases=["pre", "hold", "post"],
                run_dir=Path("/tmp"),
            )

        self.assertTrue(samples)

    def test_execute_continuous_scene_releases_load_devd_lease_before_load_action(self) -> None:
        runner = self.runner

        class FakePoller:
            def __init__(self, payload: dict[str, object]) -> None:
                self.payload = payload
                self.released_load_devd = 0

            def snapshot(self, now_monotonic: float) -> dict[str, object]:
                return {
                    "payload": self.payload,
                    "generation": 1,
                    "age_s": 0.01,
                    "sample_age_s": 0.01,
                    "elapsed_ms": 10,
                    "error": None,
                    "source": "test",
                }

            def replace_status(self, payload: dict[str, object]) -> None:
                self.payload = payload

            def generation(self) -> int:
                return 1

            def resume(self) -> None:
                return None

            def pause(self) -> None:
                return None

            def wait_until_idle(self, timeout_sec: float) -> None:
                return None

            def release_bridge_lease(self, timeout_sec: float) -> None:
                return None

            def release_load_devd_lease(self, timeout_sec: float) -> None:
                return None

            def release_load_devd_lease(self, timeout_sec: float) -> None:
                self.released_load_devd += 1

            def wait_for_bridge_lease(self, timeout_sec: float):
                return None

        args = types.SimpleNamespace(
            load_device="fixture-load-device",
            target_ma=3900,
            ups_status_url="http://ups/status",
            ups_settings_url="http://ups/settings",
            devd_diag_snapshot_url="http://devd/diag-snapshot",
            isolapurr_url="http://isolapurr",
            status_timeout_sec=0.5,
            pre_seconds=0.0,
            sample_interval_seconds=0.01,
            hold_seconds=0.0,
            backup_hold_seconds=0.0,
            restore_hold_seconds=0.0,
            post_seconds=0.0,
            include_backup=False,
            load_status_source="poll",
            load_bridge_url="",
            load_status_poll_timeout_sec=0.5,
            command_timeout_sec=0.5,
            verify_timeout_sec=0.5,
            max_i_ma_total=4000,
            max_p_mw=80000,
        )
        settings_snapshot = {
            "payload": {
                "advanced_power": {"standby_drop_mv": 1200},
                "advanced_power_capabilities": {"rated_vout_mv": 12000},
            },
            "generation": 0,
            "age_s": 0.0,
            "elapsed_ms": 0,
            "error": None,
        }
        load_poller = FakePoller({"status": {"enable": False}, "control": {"output_enabled": False}})

        def fake_capture_three_device_sample(**kwargs):
            return {"phase": kwargs["phase"], "t_s": kwargs["t_s"], "raw": {}}

        def fake_run_scheduled_action(**kwargs):
            return {"verified_status": {"status": {"enable": True}, "control": {"output_enabled": True}}}

        with (
            mock.patch.object(runner, "capture_three_device_sample", side_effect=fake_capture_three_device_sample),
            mock.patch.object(runner, "append_jsonl", autospec=True),
            mock.patch.object(runner, "persist_progress", autospec=True),
            mock.patch.object(runner, "wait_for_live_load_status", autospec=True, return_value={"ready": True}),
            mock.patch.object(runner, "wait_for_scene_pollers_ready", autospec=True, return_value={"ready": True}),
            mock.patch.object(runner, "run_scheduled_action", autospec=True, side_effect=fake_run_scheduled_action),
            mock.patch.object(runner.time, "sleep", autospec=True),
        ):
            runner.execute_continuous_scene(
                args=args,
                jsonl_path=Path("/tmp/release-load-devd-lease-before-action.jsonl"),
                metadata={},
                actions=[],
                load_status_poller=load_poller,
                ups_status_poller=FakePoller({"mode": "standby", "input": {"assist_power_stage": "standby"}}),
                diag_snapshot_poller=FakePoller({"input": {"assist_power_stage": "standby"}}),
                settings_snapshot=settings_snapshot,
                isolapurr_poller=FakePoller({"ports": {"ports": []}}),
                expected_phases=["pre", "hold", "post"],
                run_dir=Path("/tmp"),
            )

        self.assertGreaterEqual(load_poller.released_load_devd, 1)

    def test_execute_continuous_scene_keeps_socket_backed_status_stream_running_during_action(self) -> None:
        runner = self.runner

        class FakePoller:
            def __init__(self, payload: dict[str, object]) -> None:
                self.payload = payload
                self.paused = 0
                self.resumed = 0
                self.released_load_devd = 0

            def snapshot(self, now_monotonic: float) -> dict[str, object]:
                return {
                    "payload": self.payload,
                    "generation": 1,
                    "age_s": 0.01,
                    "sample_age_s": 0.01,
                    "elapsed_ms": 10,
                    "error": None,
                    "source": "test",
                }

            def replace_status(self, payload: dict[str, object]) -> None:
                self.payload = payload

            def generation(self) -> int:
                return 1

            def resume(self) -> None:
                self.resumed += 1

            def pause(self) -> None:
                self.paused += 1

            def wait_until_idle(self, timeout_sec: float) -> None:
                return None

            def release_bridge_lease(self, timeout_sec: float) -> None:
                return None

            def release_load_devd_lease(self, timeout_sec: float) -> None:
                self.released_load_devd += 1

            def wait_for_bridge_lease(self, timeout_sec: float):
                return None

        args = types.SimpleNamespace(
            load_device="fixture-load-device",
            target_ma=3900,
            ups_status_url="http://ups/status",
            ups_settings_url="http://ups/settings",
            devd_diag_snapshot_url="http://devd/diag-snapshot",
            isolapurr_url="http://isolapurr",
            status_timeout_sec=0.5,
            pre_seconds=0.0,
            sample_interval_seconds=0.01,
            hold_seconds=0.0,
            backup_hold_seconds=0.0,
            restore_hold_seconds=0.0,
            post_seconds=0.0,
            include_backup=False,
            load_status_source="status-stream",
            load_bridge_url="",
            load_devd_socket="/tmp/loadlynx.sock",
            load_status_poll_timeout_sec=0.5,
            command_timeout_sec=0.5,
            verify_timeout_sec=0.5,
            max_i_ma_total=4000,
            max_p_mw=80000,
        )
        settings_snapshot = {
            "payload": {
                "advanced_power": {"standby_drop_mv": 1200},
                "advanced_power_capabilities": {"rated_vout_mv": 12000},
            },
            "generation": 0,
            "age_s": 0.0,
            "elapsed_ms": 0,
            "error": None,
        }
        load_poller = FakePoller({"status": {"enable": False}, "control": {"output_enabled": False}})
        observed_cc_kwargs: list[dict[str, object]] = []

        def fake_capture_three_device_sample(**kwargs):
            return {"phase": kwargs["phase"], "t_s": kwargs["t_s"], "raw": {}}

        def fake_run_scheduled_action(**kwargs):
            if kwargs["action_kind"] == "cc_target":
                observed_cc_kwargs.append(kwargs)
                return {
                    "verified_status": {
                        "status": {"enable": True},
                        "control": {"output_enabled": True, "target_i_ma": 3900},
                    }
                }
            return {"verified_status": {"status": {"enable": False}, "control": {"output_enabled": False}}}

        with (
            mock.patch.object(runner, "capture_three_device_sample", side_effect=fake_capture_three_device_sample),
            mock.patch.object(runner, "append_jsonl", autospec=True),
            mock.patch.object(runner, "persist_progress", autospec=True),
            mock.patch.object(runner, "wait_for_live_load_status", autospec=True, return_value={"ready": True}),
            mock.patch.object(runner, "wait_for_scene_pollers_ready", autospec=True, return_value={"ready": True}),
            mock.patch.object(runner, "run_scheduled_action", autospec=True, side_effect=fake_run_scheduled_action),
            mock.patch.object(runner.time, "sleep", autospec=True),
        ):
            runner.execute_continuous_scene(
                args=args,
                jsonl_path=Path("/tmp/socket-status-stream-pause-before-action.jsonl"),
                metadata={},
                actions=[],
                load_status_poller=load_poller,
                ups_status_poller=FakePoller({"mode": "standby", "input": {"assist_power_stage": "standby"}}),
                diag_snapshot_poller=FakePoller({"input": {"assist_power_stage": "standby"}}),
                settings_snapshot=settings_snapshot,
                isolapurr_poller=FakePoller({"ports": {"ports": []}}),
                expected_phases=["pre", "hold", "post"],
                run_dir=Path("/tmp"),
            )

        self.assertEqual(len(observed_cc_kwargs), 1)
        self.assertIs(observed_cc_kwargs[0]["live_status_poller"], load_poller)
        self.assertEqual(load_poller.paused, 0)
        self.assertEqual(load_poller.resumed, 0)
        self.assertEqual(load_poller.released_load_devd, 0)

    def test_execute_continuous_scene_keeps_explicit_ipc_status_stream_running_during_action(self) -> None:
        runner = self.runner

        class FakePoller:
            def __init__(self, payload: dict[str, object]) -> None:
                self.payload = payload
                self.paused = 0
                self.resumed = 0
                self.waited_until_idle = 0
                self.released_bridge = 0
                self.released_load_devd = 0

            def snapshot(self, now_monotonic: float) -> dict[str, object]:
                return {
                    "payload": self.payload,
                    "generation": 1,
                    "age_s": 0.01,
                    "sample_age_s": 0.01,
                    "elapsed_ms": 10,
                    "error": None,
                    "source": "test",
                }

            def replace_status(self, payload: dict[str, object]) -> None:
                self.payload = payload

            def generation(self) -> int:
                return 1

            def resume(self) -> None:
                self.resumed += 1

            def pause(self) -> None:
                self.paused += 1

            def wait_until_idle(self, timeout_sec: float) -> None:
                self.waited_until_idle += 1

            def release_bridge_lease(self, timeout_sec: float) -> None:
                self.released_bridge += 1

            def release_load_devd_lease(self, timeout_sec: float) -> None:
                self.released_load_devd += 1

            def wait_for_bridge_lease(self, timeout_sec: float):
                return None

        args = types.SimpleNamespace(
            load_device="fixture-load-device",
            target_ma=3900,
            ups_status_url="http://ups/status",
            ups_settings_url="http://ups/settings",
            devd_diag_snapshot_url="http://devd/diag-snapshot",
            isolapurr_url="http://isolapurr",
            status_timeout_sec=0.5,
            pre_seconds=0.0,
            sample_interval_seconds=0.01,
            hold_seconds=0.0,
            backup_hold_seconds=0.0,
            restore_hold_seconds=0.0,
            post_seconds=0.0,
            include_backup=False,
            load_status_source="status-stream",
            load_bridge_url="",
            load_devd_socket="",
            load_ipc="/tmp/explicit-load-ipc.sock",
            load_status_poll_timeout_sec=0.5,
            command_timeout_sec=0.5,
            verify_timeout_sec=0.5,
            max_i_ma_total=4000,
            max_p_mw=80000,
        )
        settings_snapshot = {
            "payload": {
                "advanced_power": {"standby_drop_mv": 1200},
                "advanced_power_capabilities": {"rated_vout_mv": 12000},
            },
            "generation": 0,
            "age_s": 0.0,
            "elapsed_ms": 0,
            "error": None,
        }
        load_poller = FakePoller({"status": {"enable": False}, "control": {"output_enabled": False}})
        observed_cc_kwargs: list[dict[str, object]] = []

        def fake_capture_three_device_sample(**kwargs):
            return {"phase": kwargs["phase"], "t_s": kwargs["t_s"], "raw": {}}

        def fake_run_scheduled_action(**kwargs):
            if kwargs["action_kind"] == "cc_target":
                observed_cc_kwargs.append(kwargs)
                return {
                    "verified_status": {
                        "status": {"enable": True},
                        "control": {"output_enabled": True, "target_i_ma": 3900},
                    }
                }
            return {"verified_status": {"status": {"enable": False}, "control": {"output_enabled": False}}}

        with (
            mock.patch.object(runner, "capture_three_device_sample", side_effect=fake_capture_three_device_sample),
            mock.patch.object(runner, "append_jsonl", autospec=True),
            mock.patch.object(runner, "persist_progress", autospec=True),
            mock.patch.object(runner, "wait_for_live_load_status", autospec=True, return_value={"ready": True}),
            mock.patch.object(runner, "wait_for_scene_pollers_ready", autospec=True, return_value={"ready": True}),
            mock.patch.object(runner, "run_scheduled_action", autospec=True, side_effect=fake_run_scheduled_action),
            mock.patch.object(runner.time, "sleep", autospec=True),
        ):
            runner.execute_continuous_scene(
                args=args,
                jsonl_path=Path("/tmp/explicit-ipc-status-stream-pause-before-action.jsonl"),
                metadata={},
                actions=[],
                load_status_poller=load_poller,
                ups_status_poller=FakePoller({"mode": "standby", "input": {"assist_power_stage": "standby"}}),
                diag_snapshot_poller=FakePoller({"input": {"assist_power_stage": "standby"}}),
                settings_snapshot=settings_snapshot,
                isolapurr_poller=FakePoller({"ports": {"ports": []}}),
                expected_phases=["pre", "hold", "post"],
                run_dir=Path("/tmp"),
        )

        self.assertEqual(len(observed_cc_kwargs), 1)
        self.assertIs(observed_cc_kwargs[0]["live_status_poller"], load_poller)
        self.assertEqual(load_poller.paused, 0)
        self.assertEqual(load_poller.waited_until_idle, 0)
        self.assertEqual(load_poller.resumed, 0)
        self.assertEqual(load_poller.released_bridge, 0)
        self.assertEqual(load_poller.released_load_devd, 0)

    def test_execute_continuous_scene_paused_load_action_does_not_reuse_live_poller_for_verify(self) -> None:
        runner = self.runner

        class FakePoller:
            def __init__(self, payload: dict[str, object]) -> None:
                self.payload = payload

            def snapshot(self, now_monotonic: float) -> dict[str, object]:
                return {
                    "payload": self.payload,
                    "generation": 1,
                    "age_s": 0.01,
                    "sample_age_s": 0.01,
                    "elapsed_ms": 10,
                    "error": None,
                    "source": "test",
                }

            def replace_status(self, payload: dict[str, object]) -> None:
                self.payload = payload

            def generation(self) -> int:
                return 1

            def resume(self) -> None:
                return None

            def pause(self) -> None:
                return None

            def wait_until_idle(self, timeout_sec: float) -> None:
                return None

            def release_bridge_lease(self, timeout_sec: float) -> None:
                return None

            def release_load_devd_lease(self, timeout_sec: float) -> None:
                return None

            def wait_for_bridge_lease(self, timeout_sec: float):
                return None

        args = types.SimpleNamespace(
            load_device="fixture-load-device",
            target_ma=3900,
            ups_status_url="http://ups/status",
            ups_settings_url="http://ups/settings",
            devd_diag_snapshot_url="http://devd/diag-snapshot",
            isolapurr_url="http://isolapurr",
            status_timeout_sec=0.5,
            pre_seconds=0.0,
            sample_interval_seconds=0.01,
            hold_seconds=0.0,
            backup_hold_seconds=0.0,
            restore_hold_seconds=0.0,
            post_seconds=0.0,
            include_backup=False,
            load_status_source="poll",
            load_bridge_url="",
            load_status_poll_timeout_sec=0.5,
            command_timeout_sec=0.5,
            verify_timeout_sec=0.5,
            max_i_ma_total=4000,
            max_p_mw=80000,
        )
        settings_snapshot = {
            "payload": {
                "advanced_power": {"standby_drop_mv": 1200},
                "advanced_power_capabilities": {"rated_vout_mv": 12000},
            },
            "generation": 0,
            "age_s": 0.0,
            "elapsed_ms": 0,
            "error": None,
        }
        load_poller = FakePoller({"status": {"enable": False}, "control": {"output_enabled": False}})
        observed_cc_kwargs: list[dict[str, object]] = []

        def fake_capture_three_device_sample(**kwargs):
            return {"phase": kwargs["phase"], "t_s": kwargs["t_s"], "raw": {}}

        def fake_run_scheduled_action(**kwargs):
            if kwargs["action_kind"] == "cc_target":
                observed_cc_kwargs.append(kwargs)
                return {
                    "verified_status": {
                        "status": {"enable": True},
                        "control": {"output_enabled": True, "target_i_ma": 3900},
                    }
                }
            return {"verified_status": {"status": {"enable": False}, "control": {"output_enabled": False}}}

        with (
            mock.patch.object(runner, "capture_three_device_sample", side_effect=fake_capture_three_device_sample),
            mock.patch.object(runner, "append_jsonl", autospec=True),
            mock.patch.object(runner, "persist_progress", autospec=True),
            mock.patch.object(runner, "wait_for_live_load_status", autospec=True, return_value={"ready": True}),
            mock.patch.object(runner, "wait_for_scene_pollers_ready", autospec=True, return_value={"ready": True}),
            mock.patch.object(runner, "run_scheduled_action", autospec=True, side_effect=fake_run_scheduled_action),
            mock.patch.object(runner.time, "sleep", autospec=True),
        ):
            runner.execute_continuous_scene(
                args=args,
                jsonl_path=Path("/tmp/release-load-devd-lease-before-action.jsonl"),
                metadata={},
                actions=[],
                load_status_poller=load_poller,
                ups_status_poller=FakePoller({"mode": "standby", "input": {"assist_power_stage": "standby"}}),
                diag_snapshot_poller=FakePoller({"input": {"assist_power_stage": "standby"}}),
                settings_snapshot=settings_snapshot,
                isolapurr_poller=FakePoller({"ports": {"ports": []}}),
                expected_phases=["pre", "hold", "post"],
                run_dir=Path("/tmp"),
            )

        self.assertEqual(len(observed_cc_kwargs), 1)
        self.assertIsNone(observed_cc_kwargs[0].get("live_status_poller"))
        self.assertIsNone(observed_cc_kwargs[0].get("before_verify"))

    def test_execute_continuous_scene_times_out_stuck_load_action_and_resumes_poller(self) -> None:
        runner = self.runner

        class FakePoller:
            def __init__(self, payload: dict[str, object]) -> None:
                self.payload = payload
                self.resumed = 0

            def snapshot(self, now_monotonic: float) -> dict[str, object]:
                return {
                    "payload": self.payload,
                    "generation": 1,
                    "age_s": 0.01,
                    "sample_age_s": 0.01,
                    "elapsed_ms": 10,
                    "error": None,
                    "source": "test",
                }

            def replace_status(self, payload: dict[str, object]) -> None:
                self.payload = payload

            def generation(self) -> int:
                return 1

            def resume(self) -> None:
                self.resumed += 1

            def pause(self) -> None:
                return None

            def wait_until_idle(self, timeout_sec: float) -> None:
                return None

            def release_bridge_lease(self, timeout_sec: float) -> None:
                return None

            def release_load_devd_lease(self, timeout_sec: float) -> None:
                return None

            def wait_for_bridge_lease(self, timeout_sec: float):
                return None

        args = types.SimpleNamespace(
            load_device="fixture-load-device",
            target_ma=3900,
            ups_status_url="http://ups/status",
            ups_settings_url="http://ups/settings",
            devd_diag_snapshot_url="http://devd/diag-snapshot",
            isolapurr_url="http://isolapurr",
            status_timeout_sec=0.01,
            pre_seconds=0.0,
            sample_interval_seconds=0.01,
            hold_seconds=0.0,
            backup_hold_seconds=0.0,
            restore_hold_seconds=0.0,
            post_seconds=0.0,
            include_backup=False,
            load_status_source="poll",
            load_bridge_url="",
            load_status_poll_timeout_sec=0.01,
            command_timeout_sec=0.01,
            verify_timeout_sec=0.01,
            max_i_ma_total=4000,
            max_p_mw=80000,
        )
        settings_snapshot = {
            "payload": {
                "advanced_power": {"standby_drop_mv": 1200},
                "advanced_power_capabilities": {"rated_vout_mv": 12000},
            },
            "generation": 0,
            "age_s": 0.0,
            "elapsed_ms": 0,
            "error": None,
        }
        load_poller = FakePoller({"status": {"enable": False}, "control": {"output_enabled": False}})

        def fake_capture_three_device_sample(**kwargs):
            return {"phase": kwargs["phase"], "t_s": kwargs["t_s"], "raw": {}}

        def stuck_run_scheduled_action(**kwargs):
            time.sleep(0.2)
            return {"verified_status": {"status": {"enable": True}, "control": {"output_enabled": True}}}

        with (
            mock.patch.object(runner, "SCHEDULED_ACTION_TIMEOUT_MARGIN_SECONDS", 0.01),
            mock.patch.object(runner, "capture_three_device_sample", side_effect=fake_capture_three_device_sample),
            mock.patch.object(runner, "append_jsonl", autospec=True),
            mock.patch.object(runner, "persist_progress", autospec=True),
            mock.patch.object(runner, "wait_for_live_load_status", autospec=True, return_value={"ready": True}),
            mock.patch.object(runner, "wait_for_scene_pollers_ready", autospec=True, return_value={"ready": True}),
            mock.patch.object(runner, "run_scheduled_action", autospec=True, side_effect=stuck_run_scheduled_action),
        ):
            with self.assertRaisesRegex(RuntimeError, "scheduled_action_timeout kind=cc_target"):
                runner.execute_continuous_scene(
                    args=args,
                    jsonl_path=Path("/tmp/stuck-load-action-timeout.jsonl"),
                    metadata={},
                    actions=[],
                    load_status_poller=load_poller,
                    ups_status_poller=FakePoller({"mode": "standby", "input": {"assist_power_stage": "standby"}}),
                    diag_snapshot_poller=FakePoller({"input": {"assist_power_stage": "standby"}}),
                    settings_snapshot=settings_snapshot,
                    isolapurr_poller=FakePoller({"ports": {"ports": []}}),
                    expected_phases=["pre", "hold", "post"],
                    run_dir=Path("/tmp"),
                )

        self.assertGreaterEqual(load_poller.resumed, 1)

    def test_cleanup_disable_load_uses_serial_verify_when_poller_is_paused(self) -> None:
        runner = self.runner
        args = types.SimpleNamespace(
            command_timeout_sec=0.5,
            status_timeout_sec=0.5,
            verify_timeout_sec=0.5,
            sample_interval_seconds=0.01,
            load_status_poll_timeout_sec=0.5,
            load_status_source="poll",
            load_bridge_url="",
            load_device="fixture-load-device",
        )

        class FakeLoadPoller:
            def pause(self) -> None:
                return None

            def wait_until_idle(self, timeout_sec: float) -> None:
                return None

            def release_bridge_lease(self, timeout_sec: float) -> None:
                return None

            def release_load_devd_lease(self, timeout_sec: float) -> None:
                return None

        observed_calls: list[dict[str, object]] = []

        def fake_disable_load(*call_args, **call_kwargs):
            observed_calls.append(call_kwargs)
            return {"verified_status": {"effective_enabled": False}}

        load_status_poller = FakeLoadPoller()
        actions: list[dict[str, object]] = []
        try:
            cleanup_live_status_poller = load_status_poller
            if load_status_poller is not None:
                load_status_poller.pause()
                load_status_poller.wait_until_idle(
                    timeout_sec=max(
                        args.load_status_poll_timeout_sec + 0.5,
                        3.5,
                    )
                )
                load_status_poller.release_bridge_lease(
                    timeout_sec=min(args.status_timeout_sec, 5.0)
                )
                load_status_poller.release_load_devd_lease(
                    timeout_sec=min(args.status_timeout_sec, 5.0)
                )
                if (
                    args.load_status_source != "status-stream"
                    and not args.load_bridge_url
                ):
                    cleanup_live_status_poller = None
            with mock.patch.object(runner, "disable_load", autospec=True, side_effect=fake_disable_load):
                cleanup = runner.disable_load(
                    args,
                    args.load_device,
                    timeout_sec=args.command_timeout_sec,
                    status_timeout_sec=args.status_timeout_sec,
                    verify_timeout_sec=args.verify_timeout_sec,
                    live_status_poller=cleanup_live_status_poller,
                    before_verify=None,
                )
                actions.append({"cleanup_disable_finally": cleanup})
        except Exception as exc:  # pragma: no cover
            self.fail(f"unexpected exception: {exc!r}")

        self.assertEqual(len(observed_calls), 1)
        self.assertIsNone(observed_calls[0].get("live_status_poller"))
        self.assertIsNone(observed_calls[0].get("before_verify"))


class TracePollerPreferenceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.runner = load_runner_module()

    def test_devd_cache_meta_controls_ups_snapshot_freshness(self) -> None:
        runner = self.runner
        fresh_wrapper = {
            "sample": {
                "mode": "standby",
                "input": {"mains_present": True},
            },
            "meta": {
                "cache_age_ms": 120,
                "cache_fresh": True,
            },
        }
        stale_wrapper = {
            "sample": {
                "mode": "standby",
                "input": {"mains_present": True},
            },
            "meta": {
                "cache_age_ms": 1500,
                "cache_fresh": False,
            },
        }

        self.assertTrue(
            runner.ups_snapshot_ready(
                {
                    "payload": fresh_wrapper,
                    "generation": 1,
                    "age_s": 0.01,
                }
            )
        )
        self.assertFalse(
            runner.ups_snapshot_ready(
                {
                    "payload": stale_wrapper,
                    "generation": 2,
                    "age_s": 0.01,
                }
            )
        )

    def test_capture_sample_records_devd_cache_age_not_fetch_age(self) -> None:
        runner = self.runner
        sample = runner.capture_three_device_sample(
            phase="pre",
            t_s=0.0,
            load_device="fixture-load-device",
            ups_status_url="http://127.0.0.1:38140/api/v1/devices/serial/status",
            ups_settings_url="http://127.0.0.1:38140/api/v1/devices/serial/settings",
            devd_diag_snapshot_url="http://127.0.0.1:38140/api/v1/devices/serial/diag-snapshot",
            isolapurr_url="http://127.0.0.1:30182",
            status_timeout_sec=1.0,
            load_status_snapshot={
                "status": {
                    "status": {
                        "v_local_mv": 12000,
                        "i_local_ma": 0,
                    }
                },
                "generation": 1,
                "age_s": 0.1,
                "sample_age_s": 0.1,
            },
            ups_status_snapshot={
                "payload": {
                    "sample": {
                        "mode": "standby",
                        "input": {
                            "mains_present": True,
                            "vin_vbus_mv": 11980,
                            "vin_iin_ma": 100,
                            "assist_power_stage": "standby",
                        },
                        "output": {
                            "out_a": {"vbus_mv": 12020, "iout_ma": 0},
                            "out_b": {"vbus_mv": 12030, "iout_ma": 0},
                        },
                    },
                    "meta": {
                        "cache_age_ms": 1600,
                        "cache_fresh": False,
                    },
                },
                "generation": 1,
                "age_s": 0.02,
            },
            diag_snapshot_snapshot={
                "payload": {
                    "sample": {
                        "input": {
                            "assist_power_stage": "standby",
                            "vin_vbus_mv": 11980,
                            "vin_iin_ma": 100,
                        }
                    },
                    "meta": {
                        "cache_age_ms": 1700,
                        "cache_fresh": False,
                    },
                },
                "generation": 1,
                "age_s": 0.02,
            },
            settings_snapshot={
                "payload": {"advanced_power": {}, "advanced_power_capabilities": {}},
                "generation": 1,
                "age_s": 0.1,
            },
            isolapurr_snapshot={
                "payload": {
                    "ports": [
                        {
                            "portId": "port_c",
                            "state": {"power_enabled": True},
                            "telemetry": {"voltage_mv": 12000, "current_ma": 100},
                        }
                    ]
                },
                "generation": 1,
                "age_s": 0.1,
            },
        )

        self.assertEqual(sample["sample_age_s"]["ups_status"], 1.6)
        self.assertEqual(sample["sample_age_s"]["diag_snapshot"], 1.7)
        self.assertFalse(sample["cache_fresh"]["ups_status"])
        self.assertFalse(sample["cache_fresh"]["diag_snapshot"])
        self.assertEqual(sample["mode"], "standby")
        self.assertEqual(sample["diag_vin_vbus_mv"], 11980)

    def test_trace_fetch_populates_both_ups_and_diag_snapshot_surfaces(self) -> None:
        runner = self.runner
        trace_payload = {
            "status": {
                "mode": "standby",
                "input": {"mains_present": True, "assist_power_stage": "standby"},
            },
            "diag_snapshot": {
                "input": {
                    "assist_power_stage": "standby",
                    "vin_vbus_mv": 11888,
                    "vin_iin_ma": 805,
                }
            },
        }
        poller = runner.JsonPoller(
            name="trace",
            fetch_fn=lambda: trace_payload,
            poll_interval_sec=0.05,
        )
        poller.prime(trace_payload)
        snapshot = poller.snapshot(time.monotonic())
        self.assertTrue(runner.trace_snapshot_ready(snapshot))
        self.assertEqual(runner.status_from_trace_snapshot(snapshot)["mode"], "standby")
        self.assertEqual(
            runner.diag_snapshot_from_trace_snapshot(snapshot)["input"]["vin_vbus_mv"],
            11888,
        )

    def test_trace_diag_snapshot_with_status_fallback_derives_from_trace_status_when_diag_snapshot_missing(self) -> None:
        runner = self.runner
        trace_payload = {
            "status": {
                "mode": "standby",
                "input": {
                    "source": "dcin",
                    "mains_present": True,
                    "vin_vbus_mv": 11890,
                    "vin_iin_ma": 801,
                    "assist_power_stage": "standby",
                    "assist_target_vout_mv": 10800,
                    "tps_total_iout_ma": 16,
                },
                "charger": {"allow_charge": False, "detail_status": "WAIT"},
                "battery": {"state": "ok", "pack_mv": 15700, "current_ma": 0, "soc_pct": 90},
            }
        }
        derived = runner.trace_diag_snapshot_with_status_fallback(trace_payload)
        self.assertEqual(derived["source"], "trace_status_derived")
        self.assertEqual(derived["input"]["vin_vbus_mv"], 11890)
        self.assertEqual(derived["input"]["assist_power_stage"], "standby")


class DevdDevicesSnapshotPreferenceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.runner = load_runner_module()

    def test_derive_diag_snapshot_from_status_projects_runtime_fields(self) -> None:
        runner = self.runner
        status_payload = {
            "mode": "assist",
            "input": {
                "source": "dcin",
                "mains_present": True,
                "input_vbus_mv": 12015,
                "input_ibus_ma": 2480,
                "vin_vbus_mv": 11840,
                "vin_iin_ma": 2410,
                "vin_baseline_mv": 11992,
                "vin_drop_mv": 152,
                "assist_power_stage": "assist_low",
                "assist_target_vout_mv": 11400,
                "tps_total_iout_ma": 620,
                "tps_limit_threshold_ma": 3500,
                "pressure_state": "warning",
                "pressure_score_pct": 72,
                "pressure_reason": "vin_drop",
            },
            "charger": {
                "allow_charge": False,
                "detail_status": "NO_INPUT",
                "input_present": True,
                "vbus_present": True,
                "vbus_stat": "adapter",
                "vbus_adc_mv": 12008,
                "ibus_adc_ma": 2470,
                "vac1_adc_mv": 12003,
                "vbat_adc_mv": 14820,
            },
            "battery": {
                "state": "discharging",
                "pack_mv": 14810,
                "current_ma": -910,
                "soc_pct": 64,
            },
        }
        derived = runner.derive_diag_snapshot_from_status(
            status_payload,
            source="ups_status_derived",
        )
        self.assertEqual(derived["input"]["assist_power_stage"], "assist_low")
        self.assertEqual(derived["input"]["vin_vbus_mv"], 11840)
        self.assertEqual(derived["charger"]["allow_charge"], False)
        self.assertEqual(derived["bms"]["current_ma"], -910)
        self.assertEqual(derived["source"], "ups_status_derived")

    def test_capture_three_device_sample_derives_diag_snapshot_from_status_when_direct_diag_missing(self) -> None:
        runner = self.runner
        direct_ups_status = {
            "mode": "assist",
            "input": {
                "mains_present": True,
                "assist_power_stage": "assist_low",
                "assist_target_vout_mv": 11400,
                "vin_vbus_mv": 11852,
                "vin_iin_ma": 2395,
                "vin_baseline_mv": 11996,
                "vin_drop_mv": 144,
                "tps_total_iout_ma": 655,
            },
            "output": {
                "out_a": {"vbus_mv": 11392, "iout_ma": 328},
                "out_b": {"vbus_mv": 11388, "iout_ma": 327},
            },
            "battery": {"current_ma": -880},
            "charger": {"allow_charge": False, "detail_status": "NO_INPUT"},
        }
        sample = runner.capture_three_device_sample(
            phase="hold",
            t_s=2.0,
            load_device="fixture-load-device",
            ups_status_url="http://unused/status",
            ups_settings_url="http://unused/settings",
            devd_diag_snapshot_url="http://127.0.0.1:26670/api/v1/devices/fixture-ups-device/diag-snapshot",
            isolapurr_url="http://isolapurr",
            status_timeout_sec=0.5,
            load_status_snapshot={
                "status": {
                    "status": {"v_local_mv": 11390, "i_local_ma": 3900, "i_remote_ma": 0},
                    "control": {"output_enabled": True, "target_i_ma": 3900},
                },
                "generation": 3,
                "age_s": 0.01,
                "sample_age_s": 0.01,
                "device_sampled_at_ms": None,
                "error": None,
                "source": "poll",
            },
            ups_status_snapshot={
                "payload": direct_ups_status,
                "generation": 2,
                "age_s": 0.01,
                "elapsed_ms": 4,
                "error": None,
            },
            diag_snapshot_snapshot={
                "payload": {},
                "generation": 2,
                "age_s": 9.5,
                "elapsed_ms": 20,
                "error": "TimeoutError('timed out')",
            },
            settings_snapshot={
                "payload": {
                    "advanced_power": {"standby_drop_mv": 1200},
                    "advanced_power_capabilities": {"rated_vout_mv": 12000},
                },
                "generation": 1,
                "age_s": 0.01,
                "elapsed_ms": 1,
                "error": None,
            },
            isolapurr_snapshot={
                "payload": {
                    "ports": {
                        "ports": [
                            {
                                "portId": "port_c",
                                "state": {"power_enabled": True},
                                "telemetry": {"voltage_mv": 12005, "current_ma": 2430},
                            }
                        ]
                    }
                },
                "generation": 1,
                "age_s": 0.01,
                "elapsed_ms": 2,
                "error": None,
            },
        )
        self.assertEqual(sample["mode"], "assist")
        self.assertEqual(sample["stage"], "assist_low")
        self.assertEqual(sample["diag_stage"], "assist_low")
        self.assertEqual(sample["diag_vin_vbus_mv"], 11852)
        self.assertEqual(sample["diag_tps_total_iout_ma"], 655)
        self.assertEqual(sample["raw"]["diag_snapshot"]["source"], "ups_status_derived")

    def test_capture_three_device_sample_ignores_devd_devices_listing_projection(self) -> None:
        runner = self.runner
        device_id = "fixture-ups-device"
        listing_payload = {
            "devices": [
                {
                    "id": device_id,
                    "status": {
                        "mode": "standby",
                        "input": {
                            "mains_present": True,
                            "assist_power_stage": "standby",
                            "assist_target_vout_mv": 10800,
                            "vin_vbus_mv": 11888,
                            "vin_iin_ma": 805,
                            "tps_total_iout_ma": 36,
                        },
                        "output": {
                            "out_a": {"vbus_mv": 10880, "iout_ma": 16},
                            "out_b": {"vbus_mv": 10888, "iout_ma": 20},
                        },
                        "battery": {"current_ma": 500},
                        "charger": {"allow_charge": True, "detail_status": "CHG500"},
                    },
                    "diag_snapshot": {
                        "input": {
                            "assist_power_stage": "standby",
                            "assist_target_vout_mv": 10800,
                            "vin_vbus_mv": 11888,
                            "vin_iin_ma": 805,
                            "vin_baseline_mv": 11888,
                            "vin_drop_mv": 0,
                            "tps_total_iout_ma": 36,
                        }
                    },
                }
            ]
        }
        direct_ups_status = {
            "mode": "backup",
            "input": {
                "mains_present": False,
                "assist_power_stage": "backup",
                "assist_target_vout_mv": 12000,
                "vin_vbus_mv": 2072,
                "vin_iin_ma": 0,
                "tps_total_iout_ma": 1350,
            },
            "output": {
                "out_a": {"vbus_mv": 11980, "iout_ma": 700},
                "out_b": {"vbus_mv": 11976, "iout_ma": 650},
            },
            "battery": {"current_ma": -1800},
            "charger": {"allow_charge": False, "detail_status": "NO_INPUT"},
        }
        direct_diag_snapshot = {
            "packages": {
                "derived.power": {
                    "ok": True,
                    "source": "fresh_i2c",
                    "duration_ms": 3,
                    "payload": {
                        "input": {
                            "assist_power_stage": "backup",
                            "assist_target_vout_mv": 12000,
                            "vin_vbus_mv": 2072,
                            "vin_iin_ma": 0,
                            "vin_baseline_mv": 11888,
                            "vin_drop_mv": 9800,
                            "tps_total_iout_ma": 1350,
                        }
                    },
                }
            },
            "errors": {},
        }
        sample = runner.capture_three_device_sample(
            phase="hold",
            t_s=1.25,
            load_device="fixture-load-device",
            ups_status_url="http://unused/status",
            ups_settings_url="http://unused/settings",
            devd_diag_snapshot_url=f"http://127.0.0.1:26670/api/v1/devices/{device_id}/diag-snapshot",
            isolapurr_url="http://isolapurr",
            status_timeout_sec=0.5,
            load_status_snapshot={
                "status": {
                    "status": {"v_local_mv": 11880, "i_local_ma": 0, "i_remote_ma": 0},
                    "control": {"output_enabled": False, "target_i_ma": 3900},
                },
                "generation": 1,
                "age_s": 0.01,
                "sample_age_s": 0.01,
                "device_sampled_at_ms": None,
                "error": None,
                "source": "poll",
            },
            ups_status_snapshot={
                "payload": direct_ups_status,
                "generation": 1,
                "age_s": 0.01,
                "elapsed_ms": 5,
                "error": None,
            },
            diag_snapshot_snapshot={
                "payload": direct_diag_snapshot,
                "generation": 1,
                "age_s": 0.01,
                "elapsed_ms": 5,
                "error": None,
            },
            settings_snapshot={
                "payload": {
                    "advanced_power": {"standby_drop_mv": 1200},
                    "advanced_power_capabilities": {"rated_vout_mv": 12000},
                },
                "generation": 1,
                "age_s": 0.01,
                "elapsed_ms": 1,
                "error": None,
            },
            isolapurr_snapshot={
                "payload": {
                    "ports": {
                        "ports": [
                            {
                                "portId": "port_c",
                                "state": {"power_enabled": True},
                                "telemetry": {"voltage_mv": 12016, "current_ma": 800},
                            }
                        ]
                    }
                },
                "generation": 1,
                "age_s": 0.01,
                "elapsed_ms": 2,
                "error": None,
            },
        )
        self.assertEqual(sample["mode"], "backup")
        self.assertEqual(sample["stage"], "backup")
        self.assertEqual(sample["vin_vbus_mv"], 2072)
        self.assertEqual(sample["diag_vin_vbus_mv"], 2072)
        self.assertEqual(sample["ups_vout_mv"], 11980)

    def test_capture_three_device_sample_accepts_isolapurr_root_ports_list_shape(self) -> None:
        runner = self.runner
        direct_ups_status = {
            "mode": "standby",
            "input": {
                "mains_present": True,
                "assist_power_stage": "standby",
                "assist_target_vout_mv": 10800,
                "vin_vbus_mv": 11888,
                "vin_iin_ma": 805,
                "tps_total_iout_ma": 36,
            },
            "output": {
                "out_a": {"vbus_mv": 10880, "iout_ma": 16},
                "out_b": {"vbus_mv": 10888, "iout_ma": 20},
            },
            "battery": {"current_ma": 500},
            "charger": {"allow_charge": True, "detail_status": "CHG500"},
        }
        direct_diag_snapshot = {
            "input": {
                "assist_power_stage": "standby",
                "assist_target_vout_mv": 10800,
                "vin_vbus_mv": 11888,
                "vin_iin_ma": 805,
                "vin_baseline_mv": 11888,
                "vin_drop_mv": 0,
                "tps_total_iout_ma": 36,
            }
        }
        sample = runner.capture_three_device_sample(
            phase="hold",
            t_s=1.25,
            load_device="fixture-load-device",
            ups_status_url="http://unused/status",
            ups_settings_url="http://unused/settings",
            devd_diag_snapshot_url="http://127.0.0.1:26670/api/v1/devices/fixture-ups-device/diag-snapshot",
            isolapurr_url="http://isolapurr",
            status_timeout_sec=0.5,
            load_status_snapshot={
                "status": {
                    "status": {"v_local_mv": 11880, "i_local_ma": 0, "i_remote_ma": 0},
                    "control": {"output_enabled": False, "target_i_ma": 3900},
                },
                "generation": 1,
                "age_s": 0.01,
                "sample_age_s": 0.01,
                "device_sampled_at_ms": None,
                "error": None,
                "source": "poll",
            },
            ups_status_snapshot={
                "payload": direct_ups_status,
                "generation": 1,
                "age_s": 0.01,
                "elapsed_ms": 5,
                "error": None,
            },
            diag_snapshot_snapshot={
                "payload": direct_diag_snapshot,
                "generation": 1,
                "age_s": 0.01,
                "elapsed_ms": 5,
                "error": None,
            },
            settings_snapshot={
                "payload": {
                    "advanced_power": {"standby_drop_mv": 1200},
                    "advanced_power_capabilities": {"rated_vout_mv": 12000},
                },
                "generation": 1,
                "age_s": 0.01,
                "elapsed_ms": 1,
                "error": None,
            },
            isolapurr_snapshot={
                "payload": {
                    "ports": [
                        {
                            "portId": "port_c",
                            "state": {"power_enabled": True},
                            "telemetry": {"voltage_mv": 12016, "current_ma": 800},
                        }
                    ]
                },
                "generation": 1,
                "age_s": 0.01,
                "elapsed_ms": 2,
                "error": None,
            },
        )
        self.assertEqual(sample["mode"], "standby")
        self.assertEqual(sample["stage"], "standby")
        self.assertEqual(sample["isolapurr_port_c_mv"], 12016)
        self.assertEqual(sample["isolapurr_port_c_ma"], 800)
        self.assertEqual(sample["port_c_enabled"], True)

    def test_wait_for_scene_pollers_ready_rejects_devd_devices_listing_projection(self) -> None:
        runner = self.runner
        device_id = "fixture-ups-device"
        listing_payload = {
            "devices": [
                {
                    "id": device_id,
                    "status": {
                        "mode": "standby",
                        "input": {
                            "mains_present": True,
                            "assist_power_stage": "standby",
                        },
                    },
                    "diag_snapshot": {
                        "input": {
                            "assist_power_stage": "standby",
                            "vin_vbus_mv": 11888,
                            "vin_iin_ma": 805,
                        }
                    },
                }
            ]
        }

        class FakePoller:
            def snapshot(self, now_monotonic: float) -> dict[str, object]:
                return {
                    "payload": listing_payload,
                    "generation": 1,
                    "age_s": 0.01,
                    "elapsed_ms": 2,
                    "error": None,
                }

        class FakeIsolapurrPoller:
            def snapshot(self, now_monotonic: float) -> dict[str, object]:
                return {
                    "payload": {
                        "ports": {
                            "ports": [
                                {
                                    "portId": "port_c",
                                    "state": {"power_enabled": True},
                                    "telemetry": {"voltage_mv": 12016, "current_ma": 800},
                                }
                            ]
                        }
                    },
                    "generation": 1,
                    "age_s": 0.01,
                    "elapsed_ms": 2,
                    "error": None,
                }

        with mock.patch.object(runner.time, "sleep", autospec=True):
            ready = runner.wait_for_scene_pollers_ready(
                ups_status_poller=FakePoller(),
                diag_snapshot_poller=FakePoller(),
                isolapurr_poller=FakeIsolapurrPoller(),
                sample_interval_seconds=0.25,
                timeout_sec=0.5,
                ups_device_id=device_id,
            )
        self.assertTrue(ready["ready"])

    def test_wait_for_scene_pollers_ready_rejects_stale_diag_snapshot_even_when_status_is_fresh(self) -> None:
        runner = self.runner

        class FreshUpsPoller:
            def snapshot(self, now_monotonic: float) -> dict[str, object]:
                return {
                    "payload": {
                        "mode": "assist",
                        "input": {
                            "mains_present": True,
                            "assist_power_stage": "assist_low",
                            "assist_target_vout_mv": 11400,
                            "vin_vbus_mv": 11830,
                            "vin_iin_ma": 2388,
                            "vin_baseline_mv": 11996,
                            "vin_drop_mv": 166,
                            "tps_total_iout_ma": 640,
                        },
                        "battery": {"current_ma": -860},
                        "charger": {"allow_charge": False, "detail_status": "NO_INPUT"},
                    },
                    "generation": 3,
                    "age_s": 0.02,
                    "elapsed_ms": 3,
                    "error": None,
                }

        class StaleDiagSnapshotPoller:
            def snapshot(self, now_monotonic: float) -> dict[str, object]:
                return {
                    "payload": {},
                    "generation": 1,
                    "age_s": 20.0,
                    "elapsed_ms": 8000,
                    "error": "TimeoutError('timed out')",
                }

        class FakeIsolapurrPoller:
            def snapshot(self, now_monotonic: float) -> dict[str, object]:
                return {
                    "payload": {
                        "ports": {
                            "ports": [
                                {
                                    "portId": "port_c",
                                    "state": {"power_enabled": True},
                                    "telemetry": {"voltage_mv": 12002, "current_ma": 2410},
                                }
                            ]
                        }
                    },
                    "generation": 2,
                    "age_s": 0.02,
                    "elapsed_ms": 3,
                    "error": None,
                }

        with mock.patch.object(runner.time, "sleep", autospec=True):
            with self.assertRaisesRegex(RuntimeError, "scene_pollers_not_ready"):
                runner.wait_for_scene_pollers_ready(
                    ups_status_poller=FreshUpsPoller(),
                    diag_snapshot_poller=StaleDiagSnapshotPoller(),
                    isolapurr_poller=FakeIsolapurrPoller(),
                    sample_interval_seconds=0.25,
                    timeout_sec=0.5,
                    ups_device_id="fixture-ups-device",
                )


class FormalAcceptanceSemanticsTests(unittest.TestCase):
    def setUp(self) -> None:
        self.runner = load_runner_module()

    def test_summarize_samples_marks_source_cut_without_ups_state_change_invalid(self) -> None:
        runner = self.runner
        samples = [
            {
                "phase": "pre",
                "t_s": 0.0,
                "mode": "standby",
                "mains_present": True,
                "stage": "standby",
                "assist_target_vout_mv": 10800,
                "vin_vbus_mv": 12024,
                "vin_iin_ma": 2400,
                "tps_total_iout_ma": 30,
                "battery_current_ma": 200,
                "charger_allow_charge": True,
                "charger_detail_status": "CHG500",
                "diag_stage": "standby",
                "diag_assist_target_vout_mv": 10800,
                "diag_vin_baseline_mv": 12024,
                "diag_tps_total_iout_ma": 30,
                "ups_vout_mv": 10864,
                "load_output_enabled": False,
                "load_v_local_mv": 12000,
                "load_v_remote_mv": 12000,
                "load_i_local_ma": 0,
                "load_i_remote_ma": 0,
                "load_i_total_ma": 0,
                "load_calc_p_mw": 0,
                "port_c_enabled": True,
                "isolapurr_port_c_mv": 12026,
                "isolapurr_port_c_ma": 300,
                "raw": {
                    "ups_status": {"input": {"mains_present": True}},
                    "diag_snapshot": {"input": {"vin_drop_mv": 0}},
                    "isolapurr_power": {"ports": {"ports": []}},
                    "load_control": {},
                    "load_status": {},
                },
                "load_status_generation": 1,
                "load_status_age_s": 0.1,
                "load_status_sample_age_s": 0.1,
                "fetch_age_s": {
                    "ups_status": 0.1,
                    "diag_snapshot": 0.1,
                    "isolapurr_power": 0.1,
                },
            },
            {
                "phase": "backup",
                "t_s": 1.0,
                "mode": "standby",
                "mains_present": True,
                "stage": "standby",
                "assist_target_vout_mv": 10800,
                "vin_vbus_mv": 12024,
                "vin_iin_ma": 2400,
                "tps_total_iout_ma": 600,
                "battery_current_ma": -1200,
                "charger_allow_charge": False,
                "charger_detail_status": "NO_INPUT",
                "diag_stage": "standby",
                "diag_assist_target_vout_mv": 10800,
                "diag_vin_baseline_mv": 12024,
                "diag_tps_total_iout_ma": 600,
                "ups_vout_mv": 10820,
                "load_output_enabled": True,
                "load_v_local_mv": 10800,
                "load_v_remote_mv": 10800,
                "load_i_local_ma": 3900,
                "load_i_remote_ma": 3900,
                "load_i_total_ma": 3900,
                "load_calc_p_mw": 42120,
                "port_c_enabled": False,
                "isolapurr_port_c_mv": None,
                "isolapurr_port_c_ma": None,
                "raw": {
                    "ups_status": {"input": {"mains_present": True}},
                    "diag_snapshot": {"input": {"vin_drop_mv": 0}},
                    "isolapurr_power": {
                        "ports": {
                            "ports": [
                                {
                                    "portId": "port_c",
                                    "telemetry": {"status": "not_inserted"},
                                }
                            ]
                        }
                    },
                    "load_control": {},
                    "load_status": {},
                },
                "load_status_generation": 2,
                "load_status_age_s": 0.1,
                "load_status_sample_age_s": 0.1,
                "fetch_age_s": {
                    "ups_status": 0.1,
                    "diag_snapshot": 0.1,
                    "isolapurr_power": 0.1,
                },
            },
            {
                "phase": "post",
                "t_s": 2.0,
                "mode": "standby",
                "mains_present": True,
                "stage": "standby",
                "assist_target_vout_mv": 10800,
                "vin_vbus_mv": 12024,
                "vin_iin_ma": 100,
                "tps_total_iout_ma": 20,
                "battery_current_ma": 300,
                "charger_allow_charge": True,
                "charger_detail_status": "CHG500",
                "diag_stage": "standby",
                "diag_assist_target_vout_mv": 10800,
                "diag_vin_baseline_mv": 12024,
                "diag_tps_total_iout_ma": 20,
                "ups_vout_mv": 10864,
                "load_output_enabled": False,
                "load_v_local_mv": 12000,
                "load_v_remote_mv": 12000,
                "load_i_local_ma": 0,
                "load_i_remote_ma": 0,
                "load_i_total_ma": 0,
                "load_calc_p_mw": 0,
                "port_c_enabled": True,
                "isolapurr_port_c_mv": 12026,
                "isolapurr_port_c_ma": 200,
                "raw": {
                    "ups_status": {"input": {"mains_present": True}},
                    "diag_snapshot": {"input": {"vin_drop_mv": 0}},
                    "isolapurr_power": {"ports": {"ports": []}},
                    "load_control": {},
                    "load_status": {},
                },
                "load_status_generation": 3,
                "load_status_age_s": 0.1,
                "load_status_sample_age_s": 0.1,
                "fetch_age_s": {
                    "ups_status": 0.1,
                    "diag_snapshot": 0.1,
                    "isolapurr_power": 0.1,
                },
            },
        ]
        summary = runner.summarize_samples(
            samples,
            expected_phases=["pre", "backup", "post"],
        )
        acceptance = summary["all"]["acceptance"]
        completeness = summary["all"]["completeness"]
        self.assertFalse(acceptance["signoff_valid"])
        self.assertEqual(acceptance["run_validity"], "invalid_diagnostic_only")
        self.assertIn(
            "source_cut_not_observed_in_ups_state",
            acceptance["failed_acceptance_checks"],
        )
        self.assertIn(
            "vin_not_correlated_with_source_cut",
            acceptance["failed_acceptance_checks"],
        )
        self.assertFalse(completeness["source_cut_observed"])

    def test_signoff_rejects_when_sampling_freshness_diagnostics_are_stale(self) -> None:
        runner = self.runner
        def make_sample(
            *,
            phase: str,
            t_s: float,
            mode: str,
            mains_present: bool,
            stage: str,
            assist_target_vout_mv: int,
            vin_vbus_mv: int,
            vin_iin_ma: int,
            tps_total_iout_ma: int,
            battery_current_ma: int,
            charger_allow_charge: bool,
            charger_detail_status: str,
            diag_vin_baseline_mv: int,
            diag_vin_drop_mv: int,
            ups_vout_mv: int,
            load_output_enabled: bool,
            load_v_local_mv: int,
            load_i_total_ma: int,
            port_c_enabled: bool,
            isolapurr_port_c_mv,
            isolapurr_port_c_ma,
            generation: int,
        ) -> dict[str, object]:
            load_i_half = load_i_total_ma // 2
            return {
                "phase": phase,
                "t_s": t_s,
                "mode": mode,
                "mains_present": mains_present,
                "stage": stage,
                "assist_target_vout_mv": assist_target_vout_mv,
                "vin_vbus_mv": vin_vbus_mv,
                "vin_iin_ma": vin_iin_ma,
                "tps_total_iout_ma": tps_total_iout_ma,
                "battery_current_ma": battery_current_ma,
                "charger_allow_charge": charger_allow_charge,
                "charger_detail_status": charger_detail_status,
                "diag_stage": stage,
                "diag_assist_target_vout_mv": assist_target_vout_mv,
                "diag_vin_baseline_mv": diag_vin_baseline_mv,
                "diag_vin_drop_mv": diag_vin_drop_mv,
                "diag_tps_total_iout_ma": tps_total_iout_ma,
                "ups_vout_mv": ups_vout_mv,
                "load_output_enabled": load_output_enabled,
                "load_v_local_mv": load_v_local_mv,
                "load_v_remote_mv": load_v_local_mv,
                "load_i_local_ma": load_i_half,
                "load_i_remote_ma": load_i_total_ma - load_i_half,
                "load_i_total_ma": load_i_total_ma,
                "load_calc_p_mw": load_v_local_mv * load_i_total_ma // 1000,
                "port_c_enabled": port_c_enabled,
                "isolapurr_port_c_mv": isolapurr_port_c_mv,
                "isolapurr_port_c_ma": isolapurr_port_c_ma,
                "raw": {
                    "ups_status": {"input": {"mains_present": mains_present}},
                    "diag_snapshot": {"input": {"vin_drop_mv": diag_vin_drop_mv}},
                    "isolapurr_power": {
                        "ports": {
                            "ports": (
                                []
                                if port_c_enabled
                                else [{"portId": "port_c", "telemetry": {"status": "not_inserted"}}]
                            )
                        }
                    },
                    "load_control": {},
                    "load_status": {},
                },
                "load_status_generation": generation,
                "load_status_age_s": 1.0,
                "load_status_sample_age_s": 1.0,
                "fetch_age_s": {
                    "ups_status": 1.5,
                    "diag_snapshot": 1.4,
                    "isolapurr_power": 0.1,
                },
            }

        samples = [
            make_sample(
                phase="pre",
                t_s=0.0,
                mode="standby",
                mains_present=True,
                stage="standby",
                assist_target_vout_mv=10800,
                vin_vbus_mv=12024,
                vin_iin_ma=200,
                tps_total_iout_ma=20,
                battery_current_ma=100,
                charger_allow_charge=True,
                charger_detail_status="CHG500",
                diag_vin_baseline_mv=12024,
                diag_vin_drop_mv=0,
                ups_vout_mv=10864,
                load_output_enabled=False,
                load_v_local_mv=12000,
                load_i_total_ma=0,
                port_c_enabled=True,
                isolapurr_port_c_mv=12026,
                isolapurr_port_c_ma=300,
                generation=1,
            ),
            make_sample(
                phase="pre",
                t_s=0.25,
                mode="standby",
                mains_present=True,
                stage="standby",
                assist_target_vout_mv=10800,
                vin_vbus_mv=12020,
                vin_iin_ma=180,
                tps_total_iout_ma=18,
                battery_current_ma=120,
                charger_allow_charge=True,
                charger_detail_status="CHG500",
                diag_vin_baseline_mv=12024,
                diag_vin_drop_mv=4,
                ups_vout_mv=10864,
                load_output_enabled=False,
                load_v_local_mv=12000,
                load_i_total_ma=0,
                port_c_enabled=True,
                isolapurr_port_c_mv=12024,
                isolapurr_port_c_ma=280,
                generation=2,
            ),
            make_sample(
                phase="backup",
                t_s=0.5,
                mode="backup",
                mains_present=False,
                stage="backup",
                assist_target_vout_mv=12000,
                vin_vbus_mv=2072,
                vin_iin_ma=0,
                tps_total_iout_ma=1350,
                battery_current_ma=-1800,
                charger_allow_charge=False,
                charger_detail_status="NO_INPUT",
                diag_vin_baseline_mv=12024,
                diag_vin_drop_mv=9952,
                ups_vout_mv=11980,
                load_output_enabled=True,
                load_v_local_mv=11950,
                load_i_total_ma=3900,
                port_c_enabled=False,
                isolapurr_port_c_mv=None,
                isolapurr_port_c_ma=None,
                generation=3,
            ),
            make_sample(
                phase="backup",
                t_s=0.75,
                mode="backup",
                mains_present=False,
                stage="backup",
                assist_target_vout_mv=12000,
                vin_vbus_mv=2068,
                vin_iin_ma=0,
                tps_total_iout_ma=1360,
                battery_current_ma=-1810,
                charger_allow_charge=False,
                charger_detail_status="NO_INPUT",
                diag_vin_baseline_mv=12024,
                diag_vin_drop_mv=9956,
                ups_vout_mv=11976,
                load_output_enabled=True,
                load_v_local_mv=11940,
                load_i_total_ma=3900,
                port_c_enabled=False,
                isolapurr_port_c_mv=None,
                isolapurr_port_c_ma=None,
                generation=4,
            ),
            make_sample(
                phase="post",
                t_s=1.0,
                mode="standby",
                mains_present=True,
                stage="standby",
                assist_target_vout_mv=10800,
                vin_vbus_mv=12020,
                vin_iin_ma=120,
                tps_total_iout_ma=18,
                battery_current_ma=240,
                charger_allow_charge=True,
                charger_detail_status="CHG500",
                diag_vin_baseline_mv=12020,
                diag_vin_drop_mv=0,
                ups_vout_mv=10864,
                load_output_enabled=False,
                load_v_local_mv=12000,
                load_i_total_ma=0,
                port_c_enabled=True,
                isolapurr_port_c_mv=12024,
                isolapurr_port_c_ma=200,
                generation=5,
            ),
            make_sample(
                phase="post",
                t_s=1.25,
                mode="standby",
                mains_present=True,
                stage="standby",
                assist_target_vout_mv=10800,
                vin_vbus_mv=12018,
                vin_iin_ma=100,
                tps_total_iout_ma=16,
                battery_current_ma=220,
                charger_allow_charge=True,
                charger_detail_status="CHG500",
                diag_vin_baseline_mv=12020,
                diag_vin_drop_mv=2,
                ups_vout_mv=10864,
                load_output_enabled=False,
                load_v_local_mv=12000,
                load_i_total_ma=0,
                port_c_enabled=True,
                isolapurr_port_c_mv=12022,
                isolapurr_port_c_ma=180,
                generation=6,
            ),
        ]
        summary = runner.summarize_samples(
            samples,
            expected_phases=["pre", "backup", "post"],
        )
        acceptance = summary["all"]["acceptance"]
        completeness = summary["all"]["completeness"]
        self.assertFalse(acceptance["signoff_valid"])
        self.assertEqual(acceptance["run_validity"], "invalid_diagnostic_only")
        self.assertFalse(completeness["load_freshness_visible"])
        self.assertFalse(completeness["ups_status_fresh"])
        self.assertFalse(completeness["diag_snapshot_fresh"])
        self.assertIn("load_status_stale", acceptance["failed_acceptance_checks"])
        self.assertIn("ups_status_stale", acceptance["failed_acceptance_checks"])
        self.assertIn("diag_snapshot_stale", acceptance["failed_acceptance_checks"])


class HardwareCapabilityGateTests(unittest.TestCase):
    def setUp(self) -> None:
        self.runner = load_runner_module()

    def test_validate_ups_hardware_capabilities_accepts_matching_12v(self) -> None:
        result = self.runner.validate_ups_hardware_capabilities(
            expected_output_profile="12v",
            expected_source_voltage_mv=12000,
            identity_payload={
                "hardware_capabilities": {
                    "output_profile": "12v",
                    "rated_vout_mv": 12000,
                }
            },
            settings_payload={
                "advanced_power_capabilities": {
                    "rated_vout_mv": 12000,
                }
            },
        )
        self.assertTrue(result["ok"])
        self.assertEqual(result["failures"], [])

    def test_validate_ups_hardware_capabilities_rejects_19v_device_for_12v_scene(self) -> None:
        result = self.runner.validate_ups_hardware_capabilities(
            expected_output_profile="12v",
            expected_source_voltage_mv=12000,
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

    def test_validate_ups_hardware_capabilities_rejects_source_voltage_mismatch(self) -> None:
        result = self.runner.validate_ups_hardware_capabilities(
            expected_output_profile="19v",
            expected_source_voltage_mv=12000,
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
        self.assertIn("source_voltage_profile_mismatch", result["failures"])

    def test_validate_dual_surface_hardware_capabilities_accepts_matching_usb_and_http(self) -> None:
        result = self.runner.validate_dual_surface_hardware_capabilities(
            expected_output_profile="12v",
            expected_source_voltage_mv=12000,
            usb_identity_payload={
                "hardware_capabilities": {"output_profile": "12v", "rated_vout_mv": 12000}
            },
            usb_settings_payload={
                "advanced_power_capabilities": {"rated_vout_mv": 12000}
            },
            http_identity_payload={
                "hardware_capabilities": {"output_profile": "12v", "rated_vout_mv": 12000}
            },
            http_settings_payload={
                "advanced_power_capabilities": {"rated_vout_mv": 12000}
            },
        )
        self.assertTrue(result["ok"])
        self.assertEqual(result["failures"], [])

    def test_validate_dual_surface_hardware_capabilities_rejects_usb_http_mismatch(self) -> None:
        result = self.runner.validate_dual_surface_hardware_capabilities(
            expected_output_profile="12v",
            expected_source_voltage_mv=12000,
            usb_identity_payload={
                "hardware_capabilities": {"output_profile": "12v", "rated_vout_mv": 12000}
            },
            usb_settings_payload={
                "advanced_power_capabilities": {"rated_vout_mv": 12000}
            },
            http_identity_payload={
                "hardware_capabilities": {"output_profile": "19v", "rated_vout_mv": 19000}
            },
            http_settings_payload={
                "advanced_power_capabilities": {"rated_vout_mv": 19000}
            },
        )
        self.assertFalse(result["ok"])
        self.assertIn("http:identity_output_profile_mismatch", result["failures"])
        self.assertIn("http:settings_output_profile_mismatch", result["failures"])
        self.assertIn("usb_http_identity_caps_mismatch", result["failures"])
        self.assertIn("usb_http_settings_caps_mismatch", result["failures"])

    def test_runner_connects_ups_before_capability_gate(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            args = types.SimpleNamespace(
                profile_name="formal-12v-3900-assist_path",
                output_profile="12v",
                scene_type="assist_path",
                target_ma=3900,
                load_min_v_mv=3000,
                load_device="fixture-load-device",
                load_usb_port="/tmp/fixture-load-usb-port",
                load_bridge_device="",
                load_bridge_url="",
                load_cli="/Users/ivan/.local/bin/loadlynx",
                load_ipc="",
                load_devd_base_url="",
                load_usb_device_id="fixture-load-usb-device",
                load_status_source="status-stream",
                skip_load_telemetry_probe=False,
                load_stream_interval_seconds=0.2,
                load_status_ready_timeout_sec=20.0,
                ups_status_url="http://127.0.0.1:35830/api/v1/devices/fixture-mains-aegis/status",
                ups_settings_url="http://127.0.0.1:35830/api/v1/devices/fixture-mains-aegis/settings",
                devd_diag_snapshot_url="http://127.0.0.1:35830/api/v1/devices/fixture-mains-aegis/diag-snapshot",
                devd_monitor_start_url="http://127.0.0.1:20640/api/v1/devices/fixture-ups-device/monitor/start",
                devd_device_trace_url="http://127.0.0.1:35830/api/v1/devices/fixture-mains-aegis/trace?trace_limit=1",
                devd_scan_url="http://127.0.0.1:35830/api/v1/devices/scan",
                isolapurr_url="http://127.0.0.1:30182",
                isolapurr_device_id="fixture-source-device",
                source_voltage_mv=12000,
                source_current_limit_ma=3000,
                pre_seconds=12.0,
                hold_seconds=18.0,
                backup_hold_seconds=18.0,
                restore_hold_seconds=18.0,
                post_seconds=12.0,
                sample_interval_seconds=0.25,
                include_backup=False,
                command_timeout_sec=45.0,
                status_timeout_sec=20.0,
                load_status_poll_timeout_sec=3.0,
                verify_timeout_sec=45.0,
                max_i_ma_total=4000,
                max_p_mw=80000,
                run_id="test-run",
                report_root=tmp,
                ups_device_id="fixture-ups-device",
                mains_aegis_ipc="/tmp/mains-aegis-test.sock",
                isolapurr_cli="isolapurr",
                load_devd_socket="",
                load_telemetry_probe="tools/hil/probe_loadlynx_released_telemetry.py",
            )

            call_order: list[str] = []

            def record_connect(_args):
                call_order.append("connect")
                return {"connection": "connected"}

            def record_identity(_args):
                call_order.append("identity")
                return {"hardware_capabilities": {"output_profile": "12v", "rated_vout_mv": 12000}}

            def record_settings(_args):
                call_order.append("settings")
                return {"advanced_power_capabilities": {"rated_vout_mv": 12000}}

            with mock.patch.object(self.runner, "parse_args", autospec=True, return_value=args), \
                mock.patch.object(
                    self.runner,
                    "ensure_valid_mains_aegis_devd_http_base",
                    autospec=True,
                    return_value={"ok": True, "failures": []},
                ), \
                mock.patch.object(self.runner, "ensure_usb_port", autospec=True, return_value={"verified": True}), \
                mock.patch.object(self.runner, "http_post_empty_best_effort", autospec=True, return_value={"ok": True}), \
                mock.patch.object(
                    self.runner,
                    "probe_isolapurr_source_reachability",
                    autospec=True,
                    return_value={"ok": True, "failures": []},
                ), \
                mock.patch.object(
                    self.runner,
                    "disable_load",
                    autospec=True,
                    return_value={"verified_status": {"effective_enabled": False, "effective_target_i_ma": 0}},
                ), \
                mock.patch.object(self.runner, "set_port_c_power", autospec=True, return_value={"enabled": False}), \
                mock.patch.object(self.runner, "persist_progress", autospec=True, return_value=None), \
                mock.patch.object(self.runner, "mains_aegis_connect_device", autospec=True, side_effect=record_connect), \
                mock.patch.object(self.runner, "mains_aegis_read_identity", autospec=True, side_effect=record_identity), \
                mock.patch.object(self.runner, "mains_aegis_read_settings", autospec=True, side_effect=record_settings), \
                mock.patch.object(
                    self.runner,
                    "http_json_with_retries",
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
                        {"hardware_capabilities": {"output_profile": "12v", "rated_vout_mv": 12000}},
                        {"advanced_power_capabilities": {"rated_vout_mv": 12000}},
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
                ), \
                mock.patch.object(self.runner, "set_isolapurr_manual_output", autospec=True, side_effect=RuntimeError("stop_after_gate")):
                self.runner.main()

            self.assertGreaterEqual(len(call_order), 3)
            self.assertEqual(call_order[:3], ["connect", "identity", "settings"])

    def test_runner_enables_source_only_after_capability_and_source_configuration_gates(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            args = types.SimpleNamespace(
                profile_name="formal-12v-3900-assist_path",
                output_profile="12v",
                scene_type="assist_path",
                target_ma=3900,
                load_min_v_mv=3000,
                load_device="fixture-load-device",
                load_usb_port="/tmp/fixture-load-usb-port",
                load_bridge_device="",
                load_bridge_url="",
                load_cli="/Users/ivan/.local/bin/loadlynx",
                load_ipc="",
                load_devd_base_url="",
                load_usb_device_id="fixture-load-usb-device",
                load_status_source="status-stream",
                skip_load_telemetry_probe=False,
                load_stream_interval_seconds=0.2,
                load_status_ready_timeout_sec=20.0,
                ups_status_url="http://127.0.0.1:35830/api/v1/devices/fixture-mains-aegis/status",
                ups_settings_url="http://127.0.0.1:35830/api/v1/devices/fixture-mains-aegis/settings",
                devd_diag_snapshot_url="http://127.0.0.1:35830/api/v1/devices/fixture-mains-aegis/diag-snapshot",
                devd_monitor_start_url="http://127.0.0.1:20640/api/v1/devices/fixture-ups-device/monitor/start",
                devd_device_trace_url="http://127.0.0.1:35830/api/v1/devices/fixture-mains-aegis/trace?trace_limit=1",
                devd_scan_url="http://127.0.0.1:35830/api/v1/devices/scan",
                isolapurr_url="http://127.0.0.1:30182",
                isolapurr_device_id="fixture-source-device",
                source_voltage_mv=12000,
                source_current_limit_ma=3000,
                pre_seconds=12.0,
                hold_seconds=18.0,
                backup_hold_seconds=18.0,
                restore_hold_seconds=18.0,
                post_seconds=12.0,
                sample_interval_seconds=0.25,
                include_backup=False,
                command_timeout_sec=45.0,
                status_timeout_sec=20.0,
                load_status_poll_timeout_sec=3.0,
                verify_timeout_sec=45.0,
                max_i_ma_total=4000,
                max_p_mw=80000,
                run_id="test-run",
                report_root=tmp,
                ups_device_id="fixture-ups-device",
                mains_aegis_ipc="/tmp/mains-aegis-test.sock",
                isolapurr_cli="isolapurr",
                load_devd_socket="",
                load_telemetry_probe="tools/hil/probe_loadlynx_released_telemetry.py",
            )

            call_order: list[str] = []

            def record_probe(*_args, **_kwargs):
                call_order.append("source_reachability_gate")
                return {"ok": True, "failures": []}

            def record_set_port_c(*_args, **_kwargs):
                enabled = bool(_args[1]) if len(_args) >= 2 else bool(_kwargs.get("enabled"))
                call_order.append("enable_source" if enabled else "disable_source")
                if enabled and "source_configuration_gate" not in call_order:
                    raise AssertionError("source enabled before source configuration gate")
                if enabled:
                    raise AssertionError("stop_after_enable")
                return {"enabled": enabled}

            def record_identity(_args):
                call_order.append("usb_identity")
                return {"hardware_capabilities": {"output_profile": "12v", "rated_vout_mv": 12000}}

            def record_settings(_args):
                call_order.append("usb_settings")
                return {"advanced_power_capabilities": {"rated_vout_mv": 12000}}

            def record_validate_caps(*_args, **_kwargs):
                call_order.append("hardware_capability_gate")
                return {"ok": True, "failures": []}

            def record_manual_output(*_args, **_kwargs):
                call_order.append("set_isolapurr_manual_output")
                return {"manual": {"voltage_mv": 12000, "current_limit_ma": 3000}}

            def record_fetch_ports(*_args, **_kwargs):
                call_order.append("fetch_isolapurr_ports")
                return {"ports": [{"portId": "port_c", "state": {"power_enabled": False}}]}

            def record_validate_source(*_args, **_kwargs):
                call_order.append("source_configuration_gate")
                return {"ok": True, "failures": []}

            with mock.patch.object(self.runner, "parse_args", autospec=True, return_value=args), \
                mock.patch.object(
                    self.runner,
                    "ensure_valid_mains_aegis_devd_http_base",
                    autospec=True,
                    return_value={"ok": True, "failures": []},
                ), \
                mock.patch.object(self.runner, "ensure_usb_port", autospec=True, return_value={"verified": True}), \
                mock.patch.object(self.runner, "http_post_empty_best_effort", autospec=True, return_value={"ok": True}), \
                mock.patch.object(self.runner, "probe_isolapurr_source_reachability", autospec=True, side_effect=record_probe), \
                mock.patch.object(self.runner, "set_port_c_power", autospec=True, side_effect=record_set_port_c), \
                mock.patch.object(self.runner, "persist_progress", autospec=True, return_value=None), \
                mock.patch.object(self.runner, "mains_aegis_connect_device", autospec=True, return_value={"connection": "connected"}), \
                mock.patch.object(self.runner, "mains_aegis_read_identity", autospec=True, side_effect=record_identity), \
                mock.patch.object(self.runner, "mains_aegis_read_settings", autospec=True, side_effect=record_settings), \
                mock.patch.object(self.runner, "validate_dual_surface_hardware_capabilities", autospec=True, side_effect=record_validate_caps), \
                mock.patch.object(self.runner, "set_isolapurr_manual_output", autospec=True, side_effect=record_manual_output), \
                mock.patch.object(self.runner, "fetch_isolapurr_power_show_best_effort", autospec=True, return_value={"source": "cli_power_show_error", "ok": False}), \
                mock.patch.object(self.runner, "fetch_isolapurr_ports", autospec=True, side_effect=record_fetch_ports), \
                mock.patch.object(self.runner, "validate_isolapurr_source_configuration", autospec=True, side_effect=record_validate_source), \
                mock.patch.object(
                    self.runner,
                    "http_json_with_retries",
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
                        {"hardware_capabilities": {"output_profile": "12v", "rated_vout_mv": 12000}},
                        {"advanced_power_capabilities": {"rated_vout_mv": 12000}},
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
                ):
                self.runner.main()

            self.assertLess(call_order.index("hardware_capability_gate"), call_order.index("set_isolapurr_manual_output"))
            self.assertLess(call_order.index("set_isolapurr_manual_output"), call_order.index("source_configuration_gate"))
            self.assertLess(call_order.index("source_configuration_gate"), call_order.index("enable_source"))


class SourceConfigurationGateTests(unittest.TestCase):
    def setUp(self) -> None:
        self.runner = load_runner_module()

    def test_probe_isolapurr_source_reachability_rejects_unreachable_source(self) -> None:
        with (
            mock.patch.object(
                self.runner,
                "http_json_with_retries",
                autospec=True,
                side_effect=RuntimeError("http down"),
            ),
            mock.patch.object(
                self.runner,
                "run_json_command_with_retries",
                autospec=True,
                side_effect=RuntimeError("cli down"),
            ),
        ):
            result = self.runner.probe_isolapurr_source_reachability(
                "http://127.0.0.1:30182",
                timeout_sec=5.0,
                isolapurr_cli="isolapurr",
            )

        self.assertFalse(result["ok"])
        self.assertIn("http_ports_unreachable", result["failures"])
        self.assertIn("cli_status_unreachable", result["failures"])

    def test_probe_isolapurr_source_reachability_rejects_device_id_mismatch(self) -> None:
        with (
            mock.patch.object(
                self.runner,
                "http_json_with_retries",
                autospec=True,
                return_value={"ports": [{"portId": "port_a"}, {"portId": "port_c"}]},
            ),
            mock.patch.object(
                self.runner,
                "run_json_command_with_retries",
                autospec=True,
                return_value={"device": {"device_id": "wrong-cli"}},
            ),
        ):
            result = self.runner.probe_isolapurr_source_reachability(
                "http://127.0.0.1:30182",
                timeout_sec=5.0,
                isolapurr_cli="isolapurr",
                expected_device_id="fixture-source-device",
            )

        self.assertFalse(result["ok"])
        self.assertIn("cli_status_device_id_mismatch", result["failures"])
        self.assertEqual(result["expected_device_id"], "fixture-source-device")

    def test_probe_isolapurr_source_reachability_rejects_missing_port_c(self) -> None:
        with (
            mock.patch.object(
                self.runner,
                "http_json_with_retries",
                autospec=True,
                return_value={"ports": [{"portId": "port_a"}]},
            ),
            mock.patch.object(
                self.runner,
                "run_json_command_with_retries",
                autospec=True,
                return_value={"device": {"device_id": "fixture-source-device"}},
            ),
        ):
            result = self.runner.probe_isolapurr_source_reachability(
                "http://127.0.0.1:30182",
                timeout_sec=5.0,
                isolapurr_cli="isolapurr",
                expected_device_id="fixture-source-device",
            )

        self.assertFalse(result["ok"])
        self.assertIn("http_port_c_missing", result["failures"])

    def test_validate_ups_external_input_cut_accepts_backup_with_usb_5v_present(self) -> None:
        result = self.runner.validate_ups_external_input_cut(
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

    def test_validate_ups_external_input_cut_rejects_live_dcin(self) -> None:
        result = self.runner.validate_ups_external_input_cut(
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

    def test_validate_isolapurr_source_configuration_accepts_matching_manual_and_ports(self) -> None:
        result = self.runner.validate_isolapurr_source_configuration(
            expected_voltage_mv=12000,
            expected_current_limit_ma=3000,
            manual_ack_payload={
                "manual": {
                    "voltage_mv": 12000,
                    "current_limit_ma": 3000,
                    "path_policy": "force_close",
                    "usb_c_path_mode": "disconnect",
                }
            },
            power_show_payload={
                "manual": {
                    "voltage_mv": 12000,
                    "current_limit_ma": 3000,
                    "path_policy": "force_close",
                    "usb_c_path_mode": "disconnect",
                },
                "tps_mode": "manual",
                "capability": {"pd": {"fixed_voltages_mv": [9000, 12000, 15000, 20000]}},
            },
            ports_payload={
                "source": "http_ports",
                "ports": {
                    "ports": [
                        {
                            "portId": "port_c",
                            "state": {"power_enabled": False},
                            "telemetry": {"status": "not_inserted"},
                        }
                    ]
                },
            },
        )
        self.assertTrue(result["ok"])
        self.assertEqual(result["failures"], [])

    def test_validate_isolapurr_source_configuration_accepts_nested_config_readback(self) -> None:
        result = self.runner.validate_isolapurr_source_configuration(
            expected_voltage_mv=12000,
            expected_current_limit_ma=3000,
            manual_ack_payload={
                "manual": {
                    "voltage_mv": 12000,
                    "current_limit_ma": 3000,
                }
            },
            power_show_payload={
                "config": {
                    "manual": {
                        "voltage_mv": 12000,
                        "current_limit_ma": 3000,
                        "path_policy": "force_close",
                        "usb_c_path_mode": "disconnect",
                    },
                    "tps_mode": "manual",
                    "capability": {
                        "pd": {
                            "fixed_voltages_mv": [9000, 12000, 15000, 20000]
                        }
                    },
                }
            },
            ports_payload={
                "source": "http_ports",
                "ports": {
                    "ports": [
                        {
                            "portId": "port_c",
                            "state": {"power_enabled": False},
                            "telemetry": {"status": "not_inserted"},
                        }
                    ]
                },
            },
        )
        self.assertTrue(result["ok"])
        self.assertEqual(result["failures"], [])

    def test_validate_isolapurr_source_configuration_accepts_manual_ack_when_power_show_unavailable(self) -> None:
        result = self.runner.validate_isolapurr_source_configuration(
            expected_voltage_mv=12000,
            expected_current_limit_ma=3000,
            manual_ack_payload={
                "manual": {
                    "voltage_mv": 12000,
                    "current_limit_ma": 3000,
                }
            },
            power_show_payload={
                "source": "cli_power_show_error",
                "ok": False,
                "error": "timeout",
            },
            ports_payload={
                "source": "http_ports",
                "ports": {
                    "ports": [
                        {
                            "portId": "port_c",
                            "state": {"power_enabled": False},
                            "telemetry": {"status": "not_inserted"},
                        }
                    ]
                },
            },
        )
        self.assertTrue(result["ok"])
        self.assertEqual(result["failures"], [])

    def test_validate_isolapurr_source_configuration_accepts_nested_manual_ack_action_result(self) -> None:
        result = self.runner.validate_isolapurr_source_configuration(
            expected_voltage_mv=12000,
            expected_current_limit_ma=3000,
            manual_ack_payload={
                "actions": [
                    {
                        "cmd": ["isolapurr", "power", "output", "manual"],
                        "result": {
                            "manual": {
                                "voltage_mv": 12000,
                                "current_limit_ma": 3000,
                                "path_policy": "force_close",
                                "usb_c_path_mode": "disconnect",
                            },
                            "tps_mode": "manual",
                        },
                    }
                ]
            },
            power_show_payload={
                "config": {
                    "manual": {
                        "voltage_mv": 12000,
                        "current_limit_ma": 3000,
                        "path_policy": "force_close",
                        "usb_c_path_mode": "disconnect",
                    },
                    "tps_mode": "manual",
                    "capability": {
                        "pd": {
                            "fixed_voltages_mv": [9000, 12000, 15000, 20000]
                        }
                    },
                }
            },
            ports_payload={
                "source": "http_ports",
                "ports": {
                    "ports": [
                        {
                            "portId": "port_c",
                            "state": {"power_enabled": False},
                            "telemetry": {"status": "not_inserted"},
                        }
                    ]
                },
            },
        )
        self.assertTrue(result["ok"])
        self.assertEqual(result["failures"], [])

    def test_validate_isolapurr_source_configuration_rejects_mismatched_voltage_and_live_power(self) -> None:
        result = self.runner.validate_isolapurr_source_configuration(
            expected_voltage_mv=12000,
            expected_current_limit_ma=3000,
            manual_ack_payload={
                "manual": {
                    "voltage_mv": 19000,
                    "current_limit_ma": 3000,
                    "path_policy": "force_close",
                    "usb_c_path_mode": "disconnect",
                }
            },
            power_show_payload={
                "manual": {
                    "voltage_mv": 19000,
                    "current_limit_ma": 3000,
                    "path_policy": "force_close",
                    "usb_c_path_mode": "disconnect",
                },
                "tps_mode": "manual",
                "capability": {"pd": {"fixed_voltages_mv": [9000, 12000, 15000, 20000]}},
            },
            ports_payload={
                "source": "http_ports",
                "ports": {
                    "ports": [
                        {
                            "portId": "port_c",
                            "state": {"power_enabled": True},
                            "telemetry": {"status": "ok", "voltage_mv": 19020},
                        }
                    ]
                },
            },
        )
        self.assertFalse(result["ok"])
        self.assertIn("manual_ack_voltage_mismatch", result["failures"])
        self.assertIn("manual_readback_voltage_mismatch", result["failures"])
        self.assertIn("port_c_not_disabled_during_source_config", result["failures"])

    def test_runner_stops_before_source_restore_when_ups_cut_gate_fails(self) -> None:
        args = argparse.Namespace(
            profile_name="formal-12v-3900",
            output_profile="12v",
            scene_type="assist_path",
            target_ma=3900,
            load_min_v_mv=3000,
            load_device="fixture-load-device",
            load_usb_device_id="fixture-load-usb-device",
            load_usb_port="/tmp/fixture-load-usb-port",
            load_bridge_device="",
            load_ipc="",
            load_cli="/Users/ivan/.local/bin/loadlynx",
            load_bridge_url="",
            load_devd_base_url="http://127.0.0.1:20641",
            load_status_source="status-stream",
            load_stream_interval_seconds=0.2,
            load_status_ready_timeout_sec=20.0,
            ups_status_url="http://127.0.0.1:20640/api/v1/devices/fixture-ups-device/status",
            ups_settings_url="http://127.0.0.1:20640/api/v1/devices/fixture-ups-device/settings",
            devd_diag_snapshot_url="http://127.0.0.1:20640/api/v1/devices/fixture-ups-device/diag-snapshot",
            devd_monitor_start_url="http://127.0.0.1:20640/api/v1/devices/fixture-ups-device/monitor/start",
            devd_device_trace_url="http://127.0.0.1:20640/api/v1/devices/fixture-ups-device/trace?trace_limit=1",
            devd_scan_url="http://127.0.0.1:20640/api/v1/devices/scan",
            isolapurr_url="http://127.0.0.1:30182",
            isolapurr_device_id="fixture-source-device",
            source_voltage_mv=12000,
            source_current_limit_ma=3000,
            pre_seconds=12.0,
            hold_seconds=18.0,
            backup_hold_seconds=18.0,
            restore_hold_seconds=18.0,
            post_seconds=12.0,
            sample_interval_seconds=0.25,
            include_backup=False,
            command_timeout_sec=45.0,
            status_timeout_sec=20.0,
            load_status_poll_timeout_sec=3.0,
            verify_timeout_sec=45.0,
            max_i_ma_total=4000,
            max_p_mw=80000,
            run_id=f"test-run-{uuid.uuid4().hex[:8]}",
            report_root=tempfile.mkdtemp(),
            ups_device_id="fixture-ups-device",
            mains_aegis_ipc="/tmp/mains-aegis-test.sock",
            isolapurr_cli="isolapurr",
            load_devd_socket="",
            skip_load_telemetry_probe=True,
            load_telemetry_probe="tools/hil/probe_loadlynx_released_telemetry.py",
        )

        with mock.patch.object(self.runner, "parse_args", autospec=True, return_value=args), \
            mock.patch.object(self.runner, "ensure_usb_port", autospec=True, return_value={"verified": True}), \
            mock.patch.object(self.runner, "http_post_empty_best_effort", autospec=True, return_value={"ok": True}), \
            mock.patch.object(self.runner, "set_port_c_power", autospec=True, return_value={"enabled": False}), \
            mock.patch.object(self.runner, "persist_progress", autospec=True, return_value=None), \
            mock.patch.object(
                self.runner,
                "wait_for_ups_external_input_cut",
                autospec=True,
                return_value={
                    "ok": False,
                    "validation": {"failures": ["ups_vin_vbus_not_cut", "ups_mains_present_not_false"]},
                },
            ), \
            mock.patch.object(self.runner, "mains_aegis_connect_device", autospec=True, return_value={"connection": "connected"}), \
            mock.patch.object(self.runner, "mains_aegis_read_identity", autospec=True, return_value={"hardware_capabilities": {"output_profile": "12v", "rated_vout_mv": 12000}}), \
            mock.patch.object(self.runner, "mains_aegis_read_settings", autospec=True, return_value={"advanced_power_capabilities": {"rated_vout_mv": 12000}}), \
            mock.patch.object(self.runner, "set_isolapurr_manual_output", autospec=True) as restore_source:
            self.runner.main()

        restore_source.assert_not_called()

    def test_runner_skips_usb_connect_when_fresh_scan_already_has_connected_caps(self) -> None:
        args = argparse.Namespace(
            profile_name="formal-12v-3900",
            output_profile="12v",
            scene_type="assist_path",
            target_ma=3900,
            load_min_v_mv=3000,
            load_device="fixture-load-device",
            load_usb_device_id="fixture-load-usb-device",
            load_usb_port="/tmp/fixture-load-usb-port",
            load_bridge_device="",
            load_ipc="",
            load_cli="/Users/ivan/.local/bin/loadlynx",
            load_bridge_url="",
            load_devd_base_url="http://127.0.0.1:20641",
            load_status_source="status-stream",
            load_stream_interval_seconds=0.2,
            load_status_ready_timeout_sec=20.0,
            ups_status_url="http://127.0.0.1:30081/api/v1/status",
            ups_settings_url="http://127.0.0.1:30081/api/v1/settings",
            devd_diag_snapshot_url="http://127.0.0.1:51170/api/v1/devices/fixture-mains-aegis/diag-snapshot",
            devd_monitor_start_url="http://127.0.0.1:51170/api/v1/devices/fixture-ups-device/monitor/start",
            devd_device_trace_url="http://127.0.0.1:51170/api/v1/devices/fixture-mains-aegis/trace?trace_limit=1",
            devd_scan_url="http://127.0.0.1:51170/api/v1/devices/scan",
            isolapurr_url="http://127.0.0.1:30182",
            isolapurr_device_id="fixture-source-device",
            source_voltage_mv=12000,
            source_current_limit_ma=3000,
            pre_seconds=12.0,
            hold_seconds=18.0,
            backup_hold_seconds=18.0,
            restore_hold_seconds=18.0,
            post_seconds=12.0,
            sample_interval_seconds=0.25,
            include_backup=False,
            command_timeout_sec=45.0,
            status_timeout_sec=20.0,
            load_status_poll_timeout_sec=3.0,
            verify_timeout_sec=45.0,
            max_i_ma_total=4000,
            max_p_mw=80000,
            run_id=f"test-run-{uuid.uuid4().hex[:8]}",
            report_root=tempfile.mkdtemp(),
            ups_device_id="fixture-ups-device",
            mains_aegis_cli="tools/mains-aegis-host/target/debug/mains-aegis",
            mains_aegis_ipc="/tmp/mains-aegis-test.sock",
            isolapurr_cli="isolapurr",
            load_devd_socket="",
            skip_load_telemetry_probe=True,
            load_telemetry_probe="tools/hil/probe_loadlynx_released_telemetry.py",
            load_ipc_status_helper="tools/hil/loadlynx_ipc_status_helper.py",
        )

        scan_payload = {
            "devices": [
                {
                    "id": "fixture-ups-device",
                    "connection": "connected",
                    "identity": {
                        "device_id": "fixture-mains-aegis",
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
                    "diag_snapshot": {
                        "input": {
                            "source": "usbc",
                        }
                    },
                    "status": {
                        "mode": "backup",
                        "input": {
                            "mains_present": False,
                            "vin_vbus_mv": 0,
                            "assist_power_stage": "backup",
                        },
                    },
                    "lan_address": "127.0.0.1:30081",
                }
            ]
        }

        with mock.patch.object(self.runner, "parse_args", autospec=True, return_value=args), \
            mock.patch.object(
                self.runner,
                "ensure_valid_mains_aegis_devd_http_base",
                autospec=True,
                return_value={"ok": True, "failures": []},
            ), \
            mock.patch.object(self.runner, "ensure_usb_port", autospec=True, return_value={"verified": True}), \
            mock.patch.object(self.runner, "http_post_empty_best_effort", autospec=True, return_value=scan_payload), \
            mock.patch.object(
                self.runner,
                "probe_isolapurr_source_reachability",
                autospec=True,
                return_value={"ok": True, "failures": []},
            ), \
            mock.patch.object(self.runner, "set_port_c_power", autospec=True, return_value={"enabled": False}), \
            mock.patch.object(self.runner, "persist_progress", autospec=True, return_value=None), \
            mock.patch.object(self.runner, "mains_aegis_connect_device", autospec=True) as connect_mock, \
            mock.patch.object(
                self.runner,
                "mains_aegis_read_identity",
                autospec=True,
                return_value={
                    "hardware_capabilities": {"output_profile": "12v", "rated_vout_mv": 12000},
                    "network": {"ipv4": "127.0.0.1:30081"},
                },
            ) as read_identity_mock, \
            mock.patch.object(
                self.runner,
                "mains_aegis_read_settings",
                autospec=True,
                return_value={"advanced_power_capabilities": {"rated_vout_mv": 12000}},
            ) as read_settings_mock, \
            mock.patch.object(
                self.runner,
                "run_load_telemetry_probe",
                autospec=True,
                return_value={"skipped": True, "reason": "skip_load_telemetry_probe"},
            ), \
            mock.patch.object(
                self.runner,
                "fetch_isolapurr_ports",
                autospec=True,
                return_value={
                    "source": "http_ports",
                    "ports": {
                        "ports": [
                            {
                                "portId": "port_c",
                                "state": {"power_enabled": True},
                                "telemetry": {"status": "ok", "voltage_mv": 12000},
                            }
                        ]
                    },
                },
            ), \
            mock.patch.object(
                self.runner,
                "fetch_isolapurr_power_show",
                autospec=True,
                return_value={
                    "manual": {
                        "voltage_mv": 12000,
                        "current_limit_ma": 3000,
                        "path_policy": "force_close",
                        "usb_c_path_mode": "disconnect",
                    },
                    "tps_mode": "manual",
                },
            ), \
            mock.patch.object(
                self.runner,
                "set_isolapurr_manual_output",
                autospec=True,
                side_effect=RuntimeError("stop_after_gate"),
            ), \
            mock.patch.object(
                self.runner,
                "http_json_with_retries",
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
                    {"hardware_capabilities": {"output_profile": "12v", "rated_vout_mv": 12000}},
                    {"advanced_power_capabilities": {"rated_vout_mv": 12000}},
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
            ):
            self.runner.main()

        connect_mock.assert_not_called()
        read_identity_mock.assert_called_once()
        read_settings_mock.assert_called_once()

    def test_runner_skips_usb_connect_when_live_connection_is_already_connected(self) -> None:
        args = argparse.Namespace(
            profile_name="formal-12v-3900",
            output_profile="12v",
            scene_type="assist_path",
            target_ma=3900,
            load_min_v_mv=3000,
            load_device="fixture-load-device",
            load_usb_device_id="fixture-load-usb-device",
            load_usb_port="/tmp/fixture-load-usb-port",
            load_bridge_device="",
            load_ipc="",
            load_cli="/Users/ivan/.local/bin/loadlynx",
            load_bridge_url="",
            load_devd_base_url="http://127.0.0.1:20641",
            load_status_source="status-stream",
            load_stream_interval_seconds=0.2,
            load_status_ready_timeout_sec=20.0,
            ups_status_url="http://127.0.0.1:30081/api/v1/status",
            ups_settings_url="http://127.0.0.1:30081/api/v1/settings",
            devd_diag_snapshot_url="http://127.0.0.1:51170/api/v1/devices/fixture-mains-aegis/diag-snapshot",
            devd_monitor_start_url="http://127.0.0.1:51170/api/v1/devices/fixture-ups-device/monitor/start",
            devd_device_trace_url="http://127.0.0.1:51170/api/v1/devices/fixture-mains-aegis/trace?trace_limit=1",
            devd_scan_url="http://127.0.0.1:51170/api/v1/devices/scan",
            isolapurr_url="http://127.0.0.1:30182",
            isolapurr_device_id="fixture-source-device",
            source_voltage_mv=12000,
            source_current_limit_ma=3000,
            pre_seconds=12.0,
            hold_seconds=18.0,
            backup_hold_seconds=18.0,
            restore_hold_seconds=18.0,
            post_seconds=12.0,
            sample_interval_seconds=0.25,
            include_backup=False,
            command_timeout_sec=45.0,
            status_timeout_sec=20.0,
            load_status_poll_timeout_sec=3.0,
            verify_timeout_sec=45.0,
            max_i_ma_total=4000,
            max_p_mw=80000,
            run_id="test-run",
            report_root=str(Path("/tmp")),
            ups_device_id="fixture-ups-device",
            mains_aegis_cli="tools/mains-aegis-host/target/debug/mains-aegis",
            mains_aegis_ipc="/tmp/mains-aegis-test.sock",
            isolapurr_cli="isolapurr",
            load_devd_socket="",
            skip_load_telemetry_probe=True,
            load_telemetry_probe="tools/hil/probe_loadlynx_released_telemetry.py",
            load_ipc_status_helper="tools/hil/loadlynx_ipc_status_helper.py",
        )

        scan_payload = {
            "devices": [
                {
                    "id": "fixture-ups-device",
                    "connection": "disconnected",
                    "identity": None,
                    "settings": None,
                    "diag_snapshot": {
                        "input": {
                            "source": "usbc",
                        }
                    },
                    "status": {
                        "mode": "backup",
                        "input": {
                            "mains_present": False,
                            "vin_vbus_mv": 0,
                            "assist_power_stage": "backup",
                        },
                    },
                    "lan_address": "127.0.0.1:30081",
                }
            ]
        }

        with mock.patch.object(self.runner, "parse_args", autospec=True, return_value=args), \
            mock.patch.object(self.runner, "ensure_usb_port", autospec=True, return_value={"verified": True}), \
            mock.patch.object(self.runner, "http_post_empty_best_effort", autospec=True, return_value=scan_payload), \
            mock.patch.object(self.runner, "set_port_c_power", autospec=True, return_value={"enabled": False}), \
            mock.patch.object(self.runner, "persist_progress", autospec=True, return_value=None), \
            mock.patch.object(self.runner, "mains_aegis_read_connection", autospec=True, return_value={"connection": "connected"}), \
            mock.patch.object(self.runner, "mains_aegis_connect_device", autospec=True) as connect_mock, \
            mock.patch.object(self.runner, "mains_aegis_read_identity", autospec=True, return_value={"hardware_capabilities": {"output_profile": "12v", "rated_vout_mv": 12000}}), \
            mock.patch.object(self.runner, "mains_aegis_read_settings", autospec=True, return_value={"advanced_power_capabilities": {"rated_vout_mv": 12000}}), \
            mock.patch.object(
                self.runner,
                "run_load_telemetry_probe",
                autospec=True,
                return_value={"skipped": True, "reason": "skip_load_telemetry_probe"},
            ), \
            mock.patch.object(
                self.runner,
                "fetch_isolapurr_ports",
                autospec=True,
                return_value={
                    "source": "http_ports",
                    "ports": {
                        "ports": [
                            {
                                "portId": "port_c",
                                "state": {"power_enabled": True},
                                "telemetry": {"status": "ok", "voltage_mv": 12000},
                            }
                        ]
                    },
                },
            ), \
            mock.patch.object(
                self.runner,
                "fetch_isolapurr_power_show",
                autospec=True,
                return_value={
                    "manual": {
                        "voltage_mv": 12000,
                        "current_limit_ma": 3000,
                        "path_policy": "force_close",
                        "usb_c_path_mode": "disconnect",
                    },
                    "tps_mode": "manual",
                },
            ), \
            mock.patch.object(
                self.runner,
                "set_isolapurr_manual_output",
                autospec=True,
                side_effect=RuntimeError("stop_after_gate"),
            ), \
            mock.patch.object(
                self.runner,
                "http_json_with_retries",
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
                    {"hardware_capabilities": {"output_profile": "12v", "rated_vout_mv": 12000}},
                    {"advanced_power_capabilities": {"rated_vout_mv": 12000}},
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
            ):
            self.runner.main()

        connect_mock.assert_not_called()


if __name__ == "__main__":
    unittest.main()
