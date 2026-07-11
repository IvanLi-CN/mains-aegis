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


if __name__ == "__main__":
    unittest.main()
