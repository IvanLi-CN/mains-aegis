import { describe, expect, test } from "bun:test";

import {
  canApplyDeviceRead,
  isDevdLeaseInvalidError,
  isDevdWriteAvailable,
  loadUsbProbeSettings,
  markClosedRuntimeUnavailableRecord,
  recoverReadRecord,
  resolveManualHttpChannelPersistence,
  sameDeviceRuntime,
  waitForPendingDevdLeaseRelease,
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
      errorSource: "read",
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
    expect(recovered.serial?.connected).toBe(false);
  });

  test("restores devd ownership when a live web lease remains attached", () => {
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
      errorSource: "read",
      lastUpdated: "2026-06-07T00:00:00.000Z",
      serial: {
        connected: false,
        source: "devd",
        baseUrl: "http://127.0.0.1:8765",
        leaseId: "lease-1",
        protocol: "mains-aegis.cdc.v1",
        logs: [],
        trace: [],
      },
    };

    const recovered = recoverReadRecord(record, "devd");

    expect(recovered.serial?.connected).toBe(true);
  });

  test("does not resurrect a devd record after its lease is invalidated", () => {
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
        code: "web_session_expired",
        message: "lease expired",
        retryable: false,
        details: null,
      },
      errorSource: "transport",
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

    expect(recoverReadRecord(record, "devd")).toBe(record);
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
      errorSource: "read",
      lastUpdated: "2026-06-07T00:00:00.000Z",
    };

    const recovered = recoverReadRecord(record, "http", true);

    expect(recovered.connectionState).toBe("online");
    expect(recovered.streamState).toBe("streaming");
    expect(recovered.error).toBeNull();
  });

  test("preserves an online command error while recovering transport state", () => {
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
      connectionState: "online",
      streamState: "polling",
      error: {
        code: "manual_charge_failed",
        message: "charge command rejected",
        retryable: false,
        details: null,
      },
      errorSource: "command",
      lastUpdated: "2026-06-07T00:00:00.000Z",
    };

    const recovered = recoverReadRecord(record, "http");

    expect(recovered.connectionState).toBe("online");
    expect(recovered.streamState).toBe("polling");
    expect(recovered.error).toEqual(record.error);
    expect(recovered.errorSource).toBe("command");
  });

  test("restores a preserved command error after a read error recovers", () => {
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
        code: "http_503",
        message: "charge read unavailable",
        retryable: true,
        details: null,
      },
      errorSource: "read",
      commandError: {
        code: "manual_charge_failed",
        message: "charge command rejected",
        retryable: false,
        details: null,
      },
      lastUpdated: "2026-06-07T00:00:00.000Z",
    };

    const recovered = recoverReadRecord(record, "http");

    expect(recovered.error).toEqual(record.commandError);
    expect(recovered.errorSource).toBe("command");
  });
});

describe("canApplyDeviceRead", () => {
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
    connectionState: "online",
    streamState: "idle",
    error: null,
    lastUpdated: "2026-06-07T00:00:00.000Z",
  };

  test("rejects an older generation or a response from another channel", () => {
    const request = {
      deviceId: record.target.deviceId,
      transport: "http" as const,
      generation: 2,
    };

    expect(canApplyDeviceRead(record, request, 2)).toBe(true);
    expect(
      canApplyDeviceRead(record, { ...request, baseUrl: record.target.baseUrl }, 2),
    ).toBe(true);
    expect(
      canApplyDeviceRead(record, { ...request, baseUrl: "http://192.168.31.42" }, 2),
    ).toBe(false);
    expect(canApplyDeviceRead(record, request, 3)).toBe(false);
    expect(
      canApplyDeviceRead(record, { ...request, transport: "devd" }, 2),
    ).toBe(false);
  });
});

describe("isDevdWriteAvailable", () => {
  const baseRecord: DeviceRecord = {
    target: {
      deviceId: "mains-aegis-a1b2c3",
      baseUrl: "http://127.0.0.1:8765",
      alias: "Bench A",
      location: "Lab",
      addedAt: "2026-06-07T00:00:00.000Z",
      transport: "devd",
      preferredTransport: "devd",
      rememberedChannels: {
        devd: {
          baseUrl: "http://127.0.0.1:8765",
          seenAt: "2026-06-07T00:00:00.000Z",
          transport: "usb",
        },
      },
    },
    identity: null,
    network: null,
    settings: null,
    status: null,
    connectionState: "online",
    streamState: "polling",
    error: null,
    lastUpdated: "2026-06-07T00:00:00.000Z",
  };

  test("requires an active lease for USB-backed devd writes", () => {
    expect(
      isDevdWriteAvailable({
        ...baseRecord,
        serial: {
          source: "devd",
          baseUrl: "http://127.0.0.1:8765",
          connected: false,
          logs: [],
          trace: [],
        },
      }),
    ).toBe(false);
    expect(
      isDevdWriteAvailable({
        ...baseRecord,
        serial: {
          source: "devd",
          baseUrl: "http://127.0.0.1:8765",
          connected: true,
          leaseId: "lease-1",
          logs: [],
          trace: [],
        },
      }),
    ).toBe(true);
  });

  test("allows lease-less writes for devd LAN channels", () => {
    expect(
      isDevdWriteAvailable({
        ...baseRecord,
        target: {
          ...baseRecord.target,
          rememberedChannels: {
            devd: {
              ...baseRecord.target.rememberedChannels!.devd!,
              transport: "lan",
            },
          },
        },
      }),
    ).toBe(true);
  });
});

test("does not recover an unleased USB devd record", () => {
  const record: DeviceRecord = {
    target: {
      deviceId: "mains-aegis-a1b2c3",
      baseUrl: "http://127.0.0.1:8765",
      alias: "Bench A",
      location: "Lab",
      addedAt: "2026-06-07T00:00:00.000Z",
      transport: "devd",
      rememberedChannels: {
        devd: {
          baseUrl: "http://127.0.0.1:8765",
          seenAt: "2026-06-07T00:00:00.000Z",
          transport: "usb",
        },
      },
    },
    identity: null,
    network: null,
    settings: null,
    status: null,
    connectionState: "error",
    streamState: "error",
    error: null,
    errorSource: "read",
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

  expect(recoverReadRecord(record, "devd")).toBe(record);
});

describe("sameDeviceRuntime", () => {
  const baseRecord: DeviceRecord = {
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
    connectionState: "online",
    streamState: "idle",
    error: null,
    lastUpdated: "2026-06-07T00:00:00.000Z",
  };

  test("rejects a successor record before its stream effect installs", () => {
    expect(
      sameDeviceRuntime(
        { ...baseRecord, runtimeId: "successor" },
        { ...baseRecord, runtimeId: "previous" },
      ),
    ).toBe(false);
  });

  test("accepts the same runtime after state-only updates", () => {
    expect(
      sameDeviceRuntime(
        { ...baseRecord, runtimeId: "runtime-1", connectionState: "offline" },
        { ...baseRecord, runtimeId: "runtime-1" },
      ),
    ).toBe(true);
  });

  test("matches legacy devd records by lease identity", () => {
    const previous: DeviceRecord = {
      ...baseRecord,
      target: { ...baseRecord.target, transport: "devd" },
      serial: {
        connected: true,
        source: "devd",
        baseUrl: "http://127.0.0.1:8765",
        leaseId: "lease-1",
        protocol: "mains-aegis.cdc.v1",
        logs: [],
        trace: [],
      },
    };
    expect(
      sameDeviceRuntime(
        { ...previous, serial: { ...previous.serial!, leaseId: "lease-2" } },
        previous,
      ),
    ).toBe(false);
  });
});

describe("markClosedRuntimeUnavailableRecord", () => {
  const previous: DeviceRecord = {
    target: {
      deviceId: "mains-aegis-a1b2c3",
      baseUrl: "http://mains-aegis-a1b2c3.local",
      alias: "Bench A",
      location: "Lab",
      addedAt: "2026-06-07T00:00:00.000Z",
      transport: "http",
    },
    runtimeId: "previous-runtime",
    identity: null,
    network: null,
    settings: null,
    status: null,
    connectionState: "online",
    streamState: "streaming",
    error: null,
    lastUpdated: "2026-06-07T00:00:00.000Z",
  };

  test("keeps a successor runtime untouched", () => {
    const successor = {
      ...previous,
      runtimeId: "successor-runtime",
      connectionState: "online" as const,
      streamState: "streaming" as const,
    };
    expect(markClosedRuntimeUnavailableRecord(successor, previous)).toBe(
      successor,
    );
  });

  test("marks the closed runtime offline and disconnects devd leases", () => {
    const closed = {
      ...previous,
      runtimeId: "previous-runtime",
      target: { ...previous.target, transport: "devd" as const },
      serial: {
        connected: true,
        source: "devd" as const,
        baseUrl: "http://127.0.0.1:8765",
        leaseId: "lease-1",
        leaseExpiresAt: "2026-06-07T00:00:10.000Z",
        protocol: "mains-aegis.cdc.v1",
        logs: [],
        trace: [],
      },
    };
    const unavailable = markClosedRuntimeUnavailableRecord(closed, previous);
    expect(unavailable.connectionState).toBe("offline");
    expect(unavailable.streamState).toBe("error");
    expect(unavailable.error).toBeNull();
    expect(unavailable.serial?.connected).toBe(false);
    expect(unavailable.serial?.leaseId).toBeUndefined();
    expect(unavailable.serial?.leaseExpiresAt).toBeUndefined();
  });
});

describe("isDevdLeaseInvalidError", () => {
  test("identifies server responses that invalidate a cached web lease", () => {
    expect(
      isDevdLeaseInvalidError({
        code: "web_session_expired",
        message: "expired",
        retryable: false,
        details: null,
      }),
    ).toBe(true);
    expect(
      isDevdLeaseInvalidError({
        code: "web_session_required",
        message: "required",
        retryable: false,
        details: null,
      }),
    ).toBe(true);
    expect(
      isDevdLeaseInvalidError({
        code: "transport_error",
        message: "temporary failure",
        retryable: true,
        details: null,
      }),
    ).toBe(false);
  });
});

describe("waitForPendingDevdLeaseRelease", () => {
  test("waits for release before clearing the pending lease", async () => {
    let resolveRelease!: () => void;
    let releaseCalls = 0;
    const pendingLeases = new Map([
      [
        "mains-aegis-a1b2c3",
        {
          release: () => {
            releaseCalls += 1;
            return new Promise<void>((resolve) => {
              resolveRelease = resolve;
            });
          },
        },
      ],
    ]);

    const waiting = waitForPendingDevdLeaseRelease(
      pendingLeases,
      "mains-aegis-a1b2c3",
    );
    await Promise.resolve();

    expect(releaseCalls).toBe(1);
    expect(pendingLeases.has("mains-aegis-a1b2c3")).toBe(true);

    resolveRelease();
    await waiting;

    expect(pendingLeases.has("mains-aegis-a1b2c3")).toBe(false);
  });

  test("keeps the pending lease when release fails", async () => {
    const pendingLeases = new Map([
      [
        "mains-aegis-a1b2c3",
        {
          release: () => Promise.reject(new Error("release failed")),
        },
      ],
    ]);

    await expect(
      waitForPendingDevdLeaseRelease(pendingLeases, "mains-aegis-a1b2c3"),
    ).rejects.toThrow("release failed");
    expect(pendingLeases.has("mains-aegis-a1b2c3")).toBe(true);
  });
});
