import { describe, expect, test } from "bun:test";

import {
  getSettings,
  resetDeviceAdvancedPower,
  setDeviceAdvancedPower,
} from "./client";

describe("mock advanced power reset", () => {
  test("preserves 19V mock capabilities when resetting advanced power", async () => {
    const baseUrl = "mock:lab-standby";

    const before = await getSettings(baseUrl);
    expect(before.advanced_power_capabilities.rated_vout_mv).toBe(19000);

    await setDeviceAdvancedPower(baseUrl, {
      standby_drop_mv: 1400,
      assist_low_drop_mv: 800,
      assist_enter_delta_ma: 50,
      assist_exit_delta_ma: 0,
      assist_required_samples: 3,
      assist_ramp_step_mv: 120,
      assist_ramp_interval_ms: 300,
      rated_enter_delta_ma: 100,
      rated_exit_delta_ma: 50,
      vin_drop_threshold_pct: 5,
      required_samples: 3,
    });

    await resetDeviceAdvancedPower(baseUrl);

    const after = await getSettings(baseUrl);
    expect(after.advanced_power).toEqual({
      standby_drop_mv: 1200,
      assist_low_drop_mv: 600,
      assist_enter_delta_ma: 0,
      assist_exit_delta_ma: 0,
      assist_required_samples: 2,
      assist_ramp_step_mv: 100,
      assist_ramp_interval_ms: 200,
      rated_enter_delta_ma: 0,
      rated_exit_delta_ma: 0,
      vin_drop_threshold_pct: 4,
      required_samples: 2,
    });
    expect(after.advanced_power_capabilities.rated_vout_mv).toBe(19000);
  });

  test("preserves POST bodies for demo HTTP mock targets", async () => {
    const originalWindow = globalThis.window;
    Object.defineProperty(globalThis, "window", {
      configurable: true,
      value: {
        location: new URL("http://localhost/?demo=true"),
      },
    });
    const baseUrl = "http://mains-aegis-a1b2c3.local";

    await setDeviceAdvancedPower(baseUrl, {
      standby_drop_mv: 1550,
      assist_low_drop_mv: 700,
      assist_enter_delta_ma: 25,
      assist_exit_delta_ma: 10,
      assist_required_samples: 4,
      assist_ramp_step_mv: 130,
      assist_ramp_interval_ms: 310,
      rated_enter_delta_ma: 110,
      rated_exit_delta_ma: 60,
      vin_drop_threshold_pct: 6,
      required_samples: 5,
    });

    const updated = await getSettings(baseUrl);
    expect(updated.advanced_power).toEqual({
      standby_drop_mv: 1550,
      assist_low_drop_mv: 700,
      assist_enter_delta_ma: 25,
      assist_exit_delta_ma: 10,
      assist_required_samples: 4,
      assist_ramp_step_mv: 130,
      assist_ramp_interval_ms: 310,
      rated_enter_delta_ma: 110,
      rated_exit_delta_ma: 60,
      vin_drop_threshold_pct: 6,
      required_samples: 5,
    });

    if (originalWindow === undefined) {
      delete (globalThis as typeof globalThis & { window?: Window }).window;
    } else {
      Object.defineProperty(globalThis, "window", {
        configurable: true,
        value: originalWindow,
      });
    }
  });
});
