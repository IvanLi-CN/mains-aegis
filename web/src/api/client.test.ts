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
      input_uvlo_cutoff_mv: 18300,
      input_uvlo_recover_mv: 18500,
      input_uvlo_required_samples: 3,
      source_limited_enter_delta_ma: 1200,
    });

    await resetDeviceAdvancedPower(baseUrl);

    const after = await getSettings(baseUrl);
    expect(after.advanced_power).toEqual({
      standby_drop_mv: 900,
      input_uvlo_cutoff_mv: 18200,
      input_uvlo_recover_mv: 18400,
      input_uvlo_required_samples: 3,
      source_limited_enter_delta_ma: 1000,
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
      input_uvlo_cutoff_mv: 11400,
      input_uvlo_recover_mv: 11600,
      input_uvlo_required_samples: 4,
      source_limited_enter_delta_ma: 2600,
    });

    const updated = await getSettings(baseUrl);
    expect(updated.advanced_power).toEqual({
      standby_drop_mv: 1550,
      input_uvlo_cutoff_mv: 11400,
      input_uvlo_recover_mv: 11600,
      input_uvlo_required_samples: 4,
      source_limited_enter_delta_ma: 2600,
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
