import { describe, expect, test } from "bun:test";

import { loadUsbProbeSettings } from "./DeviceRegistry";

describe("loadUsbProbeSettings", () => {
  test("skips get_settings for hello frames that do not advertise settings support", async () => {
    let called = false;
    const settings = await loadUsbProbeSettings(
      {
        capabilities: {
          status: true,
          structured_logs: true,
          settings: false,
          wifi_config: true,
          psk_echo: false,
        },
      },
      {
        async requestSettings() {
          called = true;
          throw new Error("should not be called");
        },
      },
    );

    expect(called).toBe(false);
    expect(settings).toBeNull();
  });

  test("falls back to defaults when an older firmware rejects get_settings", async () => {
    const settings = await loadUsbProbeSettings(
      {
        capabilities: {
          status: true,
          structured_logs: true,
          settings: true,
          wifi_config: true,
          psk_echo: false,
        },
      },
      {
        async requestSettings() {
          throw Object.assign(new Error("unsupported"), {
            envelope: {
              code: "unsupported_operation",
              message: "unsupported",
              retryable: false,
              details: null,
            },
          });
        },
      },
    );

    expect(settings).toBeNull();
  });
});
