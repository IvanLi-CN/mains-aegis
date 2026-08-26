import { describe, expect, test } from "bun:test";

import {
  loadUsbProbeSettings,
  recoverReadRecord,
  resolveManualHttpChannelPersistence,
} from "./DeviceRegistry";
import type { DeviceRecord } from "../api/types";

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

describe("resolveManualHttpChannelPersistence", () => {
  test("keeps a successfully probed IPv4 target as the saved base URL", () => {
    expect(
      resolveManualHttpChannelPersistence({
        baseUrl: "http://192.168.31.42",
        identityHostnameFqdn: "mains-aegis-a1b2c3.local",
        networkIpv4: "192.168.31.42",
      }),
    ).toEqual({
      savedBaseUrl: "http://192.168.31.42",
      rememberedHttpBaseUrl: "http://mains-aegis-a1b2c3.local",
      rememberedHttpMdnsHost: "mains-aegis-a1b2c3.local",
      rememberedHttpFallbackBaseUrl: "http://192.168.31.42",
    });
  });

  test("keeps a hostname target as both saved and remembered primary URL", () => {
    expect(
      resolveManualHttpChannelPersistence({
        baseUrl: "http://mains-aegis-a1b2c3.local",
        identityHostnameFqdn: "mains-aegis-a1b2c3.local",
        networkIpv4: "192.168.31.42",
      }),
    ).toEqual({
      savedBaseUrl: "http://mains-aegis-a1b2c3.local",
      rememberedHttpBaseUrl: "http://mains-aegis-a1b2c3.local",
      rememberedHttpMdnsHost: "mains-aegis-a1b2c3.local",
      rememberedHttpFallbackBaseUrl: "http://192.168.31.42",
    });
  });
});

describe("recoverReadRecord", () => {
  test("restores a devd record after a transient read failure", () => {
    const record: DeviceRecord = {
      target: {
        deviceId: "mains-aegis-a1b2c3",
        baseUrl: "http://127.0.0.1:8765",
        alias: "Bench A",
        location: "Lab",
        addedAt: "2026-06-07T00:00:00.000Z",
        transport: "devd",
        preferredTransport: "devd",
      },
      identity: null,
      network: null,
      settings: null,
      status: null,
      connectionState: "error",
      streamState: "error",
      error: {
        code: "http_503",
        message: "charge control unavailable",
        retryable: true,
        details: null,
      },
      lastUpdated: "2026-06-07T00:00:00.000Z",
      serial: {
        connected: false,
        source: "devd",
        baseUrl: "http://127.0.0.1:8765",
        protocol: "mains-aegis.cdc.v1",
        logs: [],
        trace: [],
      },
    };

    const recovered = recoverReadRecord(record, "devd");

    expect(recovered.connectionState).toBe("online");
    expect(recovered.streamState).toBe("polling");
    expect(recovered.error).toBeNull();
    expect(recovered.serial?.connected).toBe(true);
  });

  test("keeps an active HTTP stream streaming after a read succeeds", () => {
    const record: DeviceRecord = {
      target: {
        deviceId: "mains-aegis-a1b2c3",
        baseUrl: "http://mains-aegis-a1b2c3.local",
        alias: "Bench A",
        location: "Lab",
        addedAt: "2026-06-07T00:00:00.000Z",
        transport: "http",
      },
      identity: null,
      network: null,
      settings: null,
      status: null,
      connectionState: "error",
      streamState: "error",
      error: {
        code: "http_400",
        message: "invalid preview",
        retryable: false,
        details: null,
      },
      lastUpdated: "2026-06-07T00:00:00.000Z",
    };

    const recovered = recoverReadRecord(record, "http", true);

    expect(recovered.connectionState).toBe("online");
    expect(recovered.streamState).toBe("streaming");
    expect(recovered.error).toBeNull();
  });
});
