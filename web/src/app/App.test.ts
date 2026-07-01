import { describe, expect, test } from "bun:test";
import type { DeviceRecord } from "../api/types";
import {
  deviceSettingsAvailable,
  resolveManualHttpRememberedChannel,
  resolveDevdTarget,
  resolveOwnerFacingDevdTarget,
  resolveStartupDevdTarget,
} from "./App";

function makeRecord(overrides: Partial<DeviceRecord>): DeviceRecord {
  return {
    target: {
      deviceId: "mains-aegis-legacy-usb",
      baseUrl: "serial:mains-aegis-legacy-usb",
      alias: "Legacy USB UPS",
      location: "Bench",
      addedAt: "2026-06-07T00:00:00.000Z",
      transport: "serial",
      preferredTransport: "serial",
      rememberedChannels: {
        serial: {
          seenAt: "2026-06-07T00:00:00.000Z",
        },
      },
    },
    identity: null,
    network: null,
    settings: null,
    status: null,
    connectionState: "online",
    streamState: "streaming",
    error: null,
    lastUpdated: "2026-06-07T00:00:00.000Z",
    serial: {
      connected: true,
      source: "web_serial",
      protocol: "mains-aegis.cdc.v1",
      logs: [],
      trace: [],
    },
    ...overrides,
  };
}

describe("deviceSettingsAvailable", () => {
  test("returns false for USB records without real settings support", () => {
    expect(deviceSettingsAvailable(makeRecord({ settings: null }))).toBe(false);
  });

  test("returns true for USB records with real settings", () => {
    expect(
      deviceSettingsAvailable(
        makeRecord({
          settings: {
            wifi: {
              configured: false,
              ssid: null,
            },
            log_level: "info",
            manual_charge: {
              target: "full_100",
              speed: "ma_500",
              timer_h: 2,
            },
            advanced_power: {
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
            },
            advanced_power_capabilities: {
              rated_vout_mv: 12000,
              standby_drop_mv: { default: 1200, min: 0, max: 3000, step: 20 },
              assist_low_drop_mv: { default: 600, min: 0, max: 3000, step: 20 },
              assist_enter_delta_ma: {
                default: 0,
                min: -100,
                max: 1000,
                step: 50,
              },
              assist_exit_delta_ma: {
                default: 0,
                min: -50,
                max: 1000,
                step: 50,
              },
              assist_required_samples: { default: 2, min: 1, max: 5, step: 1 },
              assist_ramp_step_mv: { default: 100, min: 20, max: 1000, step: 20 },
              assist_ramp_interval_ms: {
                default: 200,
                min: 100,
                max: 3000,
                step: 100,
              },
              rated_enter_delta_ma: {
                default: 0,
                min: -100,
                max: 1000,
                step: 50,
              },
              rated_exit_delta_ma: {
                default: 0,
                min: -50,
                max: 1000,
                step: 50,
              },
              vin_drop_threshold_pct: { default: 4, min: 1, max: 12, step: 1 },
              required_samples: { default: 2, min: 1, max: 5, step: 1 },
            },
          },
        }),
      ),
    ).toBe(true);
  });
});

describe("resolveOwnerFacingDevdTarget", () => {
  test("accepts explicit devd target values", () => {
    expect(resolveOwnerFacingDevdTarget(" ipc://devd.sock ", false)).toBe(
      "ipc://devd.sock",
    );
  });

  test("allows legacy mock target values in demo mode", () => {
    expect(resolveOwnerFacingDevdTarget("mock:devd", true)).toBe("mock:devd");
  });

  test("rejects mock target values outside demo mode", () => {
    expect(resolveOwnerFacingDevdTarget("mock:devd", false)).toBeUndefined();
  });
});

describe("resolveStartupDevdTarget", () => {
  test("prefers devd_target over legacy mock_devd_target", () => {
    const params = new URLSearchParams({
      devd_target: "ipc://preferred.sock",
      mock_devd_target: "ipc://legacy.sock",
    });
    expect(resolveStartupDevdTarget(params, false)).toBe(
      "ipc://preferred.sock",
    );
  });

  test("falls back to legacy mock_devd_target when devd_target is absent", () => {
    const params = new URLSearchParams({
      mock_devd_target: "ipc://legacy.sock",
    });
    expect(resolveStartupDevdTarget(params, false)).toBe("ipc://legacy.sock");
  });
});

describe("resolveDevdTarget", () => {
  test("keeps seeded demos mock-only without an explicit devd target", () => {
    expect(resolveDevdTarget(undefined, false, true)).toBeNull();
  });
});

describe("resolveManualHttpRememberedChannel", () => {
  test("keeps verified hostnames as the primary remembered URL", () => {
    expect(
      resolveManualHttpRememberedChannel("mains-aegis-a1b2c3.local"),
    ).toEqual({
      rememberedHttpBaseUrl: "http://mains-aegis-a1b2c3.local",
    });
  });

  test("stores manual IPv4 targets as fallback URLs", () => {
    expect(resolveManualHttpRememberedChannel("192.168.31.42")).toEqual({
      rememberedHttpFallbackBaseUrl: "http://192.168.31.42",
    });
    expect(resolveManualHttpRememberedChannel("192.168.31.42:8080")).toEqual({
      rememberedHttpFallbackBaseUrl: "http://192.168.31.42:8080",
    });
  });
});
