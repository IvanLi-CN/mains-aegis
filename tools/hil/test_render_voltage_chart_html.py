import importlib.util
import unittest
from pathlib import Path


RENDERER_PATH = Path(__file__).with_name("render_voltage_chart_html.py")
SPEC = importlib.util.spec_from_file_location("render_voltage_chart_html", RENDERER_PATH)
assert SPEC and SPEC.loader
RENDERER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RENDERER)


class VoltageChartTooltipTests(unittest.TestCase):
    def test_tooltip_is_viewport_positioned_and_escapes_chart_clipping(self):
        html = RENDERER.render_html(
            "test chart",
            Path("/tmp/timeseries.jsonl"),
            [{"t_s": 0.0, "phase": "pre", "stage": "standby", "source_v": 12.0, "vin_v": 12.0,
              "ups_vout": 12.0, "load_v": 12.0}],
            0.5,
        )

        self.assertIn("position: fixed", html)
        self.assertIn("document.body.appendChild(tooltip)", html)
        self.assertIn("const viewportWidth = document.documentElement.clientWidth", html)
        self.assertIn("const viewportHeight = document.documentElement.clientHeight", html)
        self.assertIn("translate3d(${tipX}px, ${tipY}px, 0)", html)
        self.assertIn("target=${fmtMa(nearest.target_ma)} | actual=${fmtMa(nearest.load_i_total_ma)}", html)
        self.assertNotIn("load_i_local_ma", html)
        self.assertNotIn("load_i_remote_ma", html)
        self.assertIn("<strong>Chart Tags</strong>", html)
        self.assertNotIn("P* = phase window | S* = state marker", html)
        self.assertIn("Scene baseline state.", html)
        self.assertIn("const transitionWindowS = Math.max(0.35, Math.min(1.2, (state.end - state.start) * 0.03));", html)
        self.assertIn("const showTransition = transition && Math.abs(targetT - transition.t_s) <= transitionWindowS;", html)
        self.assertNotIn("No state marker was recorded before this sample.", html)

    def test_backup_transition_span_starts_on_first_live_cut_effect(self):
        rows = [
            {"t_s": 8.211, "phase": "hold", "stage": "standby", "mode": "standby", "backup_reason": None, "mains_present": True, "vin_vbus_mv": 18920, "target_ma": 1000},
            {"t_s": 24.212, "phase": "transition_backup", "stage": "standby", "mode": "standby", "backup_reason": None, "mains_present": True, "vin_vbus_mv": 18920, "target_ma": 1000},
            {"t_s": 24.914, "phase": "transition_backup", "stage": "standby", "mode": "standby", "backup_reason": None, "mains_present": True, "vin_vbus_mv": 18920, "target_ma": 1000},
            {"t_s": 25.014, "phase": "transition_backup", "stage": "backup", "mode": "backup", "backup_reason": "input_absent", "mains_present": True, "vin_vbus_mv": 3016, "target_ma": 1000},
            {"t_s": 25.113, "phase": "transition_backup", "stage": "backup", "mode": "backup", "backup_reason": "input_absent", "mains_present": True, "vin_vbus_mv": 3016, "target_ma": 1000},
            {"t_s": 26.054, "phase": "backup", "stage": "backup", "mode": "backup", "backup_reason": "input_absent", "mains_present": False, "vin_vbus_mv": 2024, "target_ma": 1000},
        ]

        spans = RENDERER.build_tag_spans(rows)

        self.assertEqual(
            spans,
            [
                {"phase": "hold", "start": 8.211, "end": 25.014, "label": "hold / 1000mA"},
                {"phase": "transition_backup", "start": 25.014, "end": 25.113, "label": "transition_backup"},
                {"phase": "backup", "start": 25.113, "end": 26.054, "label": "backup / input cut"},
            ],
        )

    def test_tooltip_includes_phase_and_state_marker_meanings(self):
        html = RENDERER.render_html(
            "test chart",
            Path("/tmp/timeseries.jsonl"),
            [
                {
                    "t_s": 0.0,
                    "phase": "pre",
                    "stage": "standby",
                    "mode": "standby",
                    "backup_reason": None,
                    "charger_state": "ok",
                    "charger_allow_charge": False,
                    "source_v": 12.0,
                    "vin_v": 12.0,
                    "ups_vout": 11.3,
                    "load_v": 12.0,
                },
                {
                    "t_s": 1.0,
                    "phase": "transition_unload",
                    "stage": "standby",
                    "mode": "standby",
                    "backup_reason": None,
                    "charger_state": "ok",
                    "charger_allow_charge": False,
                    "source_v": 12.0,
                    "vin_v": 12.0,
                    "ups_vout": 11.3,
                    "load_v": 12.0,
                },
            ],
            0.5,
        )

        self.assertIn("function chartTagDetails(targetT)", html)
        self.assertIn("Unload transient while the runner disables the electronic load and returns toward post-idle.", html)
        self.assertIn("<strong>S${transitionIndex + 1}</strong> ${previousTransition ? \"state change\" : \"initial state\"}", html)


if __name__ == "__main__":
    unittest.main()
