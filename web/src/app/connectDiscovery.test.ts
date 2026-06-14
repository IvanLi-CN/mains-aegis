import { describe, expect, test } from "bun:test";

import type { DeviceRecord, DevdDevice } from "../api/types";
import {
  buildDiscoveredLogicalDevices,
  buildFleetEntries,
  resolveSelectedRecord,
} from "./App";

function savedRecord(deviceId: string): DeviceRecord {
  return {
    target: {
      deviceId,
      baseUrl: "http://mains-aegis-a1b2c3.local",
      alias: "Lab rack A",
      location: "Bench 1",
      addedAt: "2026-06-07T00:00:00.000Z",
      transport: "http",
      preferredTransport: "http",
      rememberedChannels: {
        http: {
          baseUrl: "http://mains-aegis-a1b2c3.local",
          seenAt: "2026-06-07T00:00:00.000Z",
          source: "devd_discovery",
        },
      },
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
}

function usbPendingDevice(logicalDeviceId: string): DevdDevice {
  return {
    id: "usb-stable-a",
    display_name: "/dev/cu.usbmodem1",
    port_path: "/dev/cu.usbmodem1",
    lan_address: null,
    lan_conflict_addresses: [],
    transport: "native_serial",
    binding: {
      alias: "Bench USB",
      stable_id: "usb-stable-a",
      port_path: "/dev/cu.usbmodem1",
      created_at: "2026-06-07T00:00:00.000Z",
      logical_device_id: logicalDeviceId,
    },
    connection: "disconnected",
    identity: null,
    selected_artifact_id: null,
    log_decode: {
      status: "unverified",
      reason: null,
      artifact_id: null,
    },
  };
}

function usbConfirmedCompanionDevice(
  logicalDeviceId: string,
  lanAddress: string | null = null,
): DevdDevice {
  const device = usbPendingDevice(logicalDeviceId);
  return {
    ...device,
    lan_address: lanAddress,
    binding: {
      ...device.binding!,
      lan_companion: {
        mdns_host: "mains-aegis-a1b2c3.local",
        ip: "192.168.31.42",
        port: 80,
        confirmed_at: "2026-06-07T00:00:00.000Z",
        last_verified_at: "2026-06-07T00:00:00.000Z",
      },
    },
  };
}

function lanDevice(deviceId: string): DevdDevice {
  return {
    id: deviceId,
    display_name: "mains-aegis-a1b2c3",
    port_path: null,
    lan_address: "192.168.31.42",
    lan_conflict_addresses: [],
    transport: "lan",
    binding: null,
    connection: "connected",
    identity: {
      role: "ups",
      api_version: "v1",
      device_id: deviceId,
      short_id: "a1b2c3",
      hostname: "mains-aegis-a1b2c3",
      hostname_fqdn: "mains-aegis-a1b2c3.local",
      capabilities: {
        sse: true,
        mdns: true,
        dns_sd: true,
        write_controls: true,
      },
      firmware: {
        protocol: "mains-aegis.cdc.v1",
        package_version: "0.1.0",
        build_profile: "release",
        build_id: "build-1",
        git_sha: "git-1",
        src_hash: "src-1",
        git_dirty: "false",
        features: ["web_serial"],
      },
      network: {
        device_id: deviceId,
        hostname: "mains-aegis-a1b2c3",
        hostname_fqdn: "mains-aegis-a1b2c3.local",
        state: "connected",
        ipv4: "192.168.31.42",
        gateway: "192.168.31.1",
        dns: "192.168.31.1",
        is_static: false,
        rssi_dbm: -42,
        last_error: null,
      },
    },
    selected_artifact_id: null,
    log_decode: {
      status: "unverified",
      reason: null,
      artifact_id: null,
    },
  };
}

describe("buildDiscoveredLogicalDevices", () => {
  test("groups a pending USB binding back into the saved logical device", () => {
    const logicalDevices = buildDiscoveredLogicalDevices(
      [usbPendingDevice("mains-aegis-a1b2c3"), lanDevice("mains-aegis-a1b2c3")],
      [savedRecord("mains-aegis-a1b2c3")],
    );

    expect(logicalDevices).toHaveLength(1);
    expect(logicalDevices[0]?.key).toBe("mains-aegis-a1b2c3");
    expect(logicalDevices[0]?.displayName).toBe("Lab rack A");
    expect(logicalDevices[0]?.existingRecord?.target.deviceId).toBe(
      "mains-aegis-a1b2c3",
    );
    expect(logicalDevices[0]?.channels.devd?.id).toBe("usb-stable-a");
    expect(logicalDevices[0]?.channels.http?.id).toBe("mains-aegis-a1b2c3");
    expect(logicalDevices[0]?.availableTransports).toEqual(["http", "devd"]);
  });

  test("derives a WiFi channel from a merged USB record that already carries lan_address", () => {
    const mergedUsbRecord = {
      ...usbPendingDevice("mains-aegis-a1b2c3"),
      lan_address: "192.168.31.42",
    } satisfies DevdDevice;

    const logicalDevices = buildDiscoveredLogicalDevices(
      [mergedUsbRecord],
      [savedRecord("mains-aegis-a1b2c3")],
    );

    expect(logicalDevices).toHaveLength(1);
    expect(logicalDevices[0]?.channels.devd?.id).toBe("usb-stable-a");
    expect(logicalDevices[0]?.channels.http?.lan_address).toBe("192.168.31.42");
    expect(logicalDevices[0]?.availableTransports).toEqual(["http", "devd"]);
  });

  test("does not fabricate a live WiFi channel from a saved companion alone", () => {
    const logicalDevices = buildDiscoveredLogicalDevices(
      [usbConfirmedCompanionDevice("mains-aegis-a1b2c3")],
      [savedRecord("mains-aegis-a1b2c3")],
    );

    expect(logicalDevices).toHaveLength(1);
    expect(logicalDevices[0]?.channels.devd?.id).toBe("usb-stable-a");
    expect(logicalDevices[0]?.channels.http).toBeUndefined();
    expect(logicalDevices[0]?.availableTransports).toEqual(["devd"]);
  });
});

describe("buildFleetEntries", () => {
  test("keeps browser-saved devices when devd records are empty", () => {
    const entries = buildFleetEntries(
      [savedRecord("mains-aegis-a1b2c3")],
      [],
      "same-origin",
    );

    expect(entries).toHaveLength(1);
    expect(entries[0]?.saved).toBe(true);
    expect(entries[0]?.record.target.deviceId).toBe("mains-aegis-a1b2c3");
    expect(entries[0]?.record.target.alias).toBe("Lab rack A");
  });

  test("surfaces devd-backed device records even when the browser has not saved the device", () => {
    const entries = buildFleetEntries(
      [],
      [lanDevice("mains-aegis-a1b2c3")],
      "same-origin",
    );

    expect(entries).toHaveLength(1);
    expect(entries[0]?.saved).toBe(false);
    expect(entries[0]?.record.target.deviceId).toBe("mains-aegis-a1b2c3");
    expect(entries[0]?.record.target.location).toBe("devd records");
    expect(entries[0]?.record.target.rememberedChannels?.http?.source).toBe(
      "devd_discovery",
    );
    expect(entries[0]?.record.connectionState).toBe("online");
  });

  test("merges saved browser data with current devd record channels", () => {
    const entries = buildFleetEntries(
      [savedRecord("mains-aegis-a1b2c3")],
      [usbPendingDevice("mains-aegis-a1b2c3"), lanDevice("mains-aegis-a1b2c3")],
      "same-origin",
    );

    expect(entries).toHaveLength(1);
    expect(entries[0]?.saved).toBe(true);
    expect(entries[0]?.record.target.alias).toBe("Lab rack A");
    expect(entries[0]?.record.target.rememberedChannels?.http?.source).toBe(
      "devd_discovery",
    );
    expect(entries[0]?.record.target.rememberedChannels?.devd?.devdDeviceId).toBe(
      "usb-stable-a",
    );
  });

  test("refreshes confirmed companion fallback IP from current LAN discovery", () => {
    const entries = buildFleetEntries(
      [savedRecord("mains-aegis-a1b2c3")],
      [usbConfirmedCompanionDevice("mains-aegis-a1b2c3", "192.168.31.99")],
      "same-origin",
    );

    expect(entries).toHaveLength(1);
    expect(entries[0]?.record.target.rememberedChannels?.http?.baseUrl).toBe(
      "http://mains-aegis-a1b2c3.local",
    );
    expect(
      entries[0]?.record.target.rememberedChannels?.http?.fallbackBaseUrl,
    ).toBe("http://192.168.31.99:80");
  });

  test("keeps staged fleet records unsaved when they are temporary", () => {
    const temporaryRecord = {
      ...savedRecord("mains-aegis-a1b2c3"),
      target: {
        ...savedRecord("mains-aegis-a1b2c3").target,
        temporary: true,
      },
    } satisfies DeviceRecord;

    const entries = buildFleetEntries(
      [temporaryRecord],
      [lanDevice("mains-aegis-a1b2c3")],
      "same-origin",
    );

    expect(entries).toHaveLength(1);
    expect(entries[0]?.saved).toBe(false);
  });
});

describe("resolveSelectedRecord", () => {
  test("falls back to fleet entries when the device is not saved in the local registry", () => {
    const fleetEntries = buildFleetEntries(
      [],
      [lanDevice("mains-aegis-a1b2c3")],
      "same-origin",
    );

    const selected = resolveSelectedRecord(
      "mains-aegis-a1b2c3",
      [],
      fleetEntries,
    );

    expect(selected?.target.deviceId).toBe("mains-aegis-a1b2c3");
    expect(selected?.target.location).toBe("devd records");
  });
});

describe("direct device route discovery", () => {
  test("keeps unresolved route empty until discovery has produced fleet entries", () => {
    const selected = resolveSelectedRecord("mains-aegis-a1b2c3", [], []);
    expect(selected).toBeNull();

    const fleetEntries = buildFleetEntries(
      [],
      [lanDevice("mains-aegis-a1b2c3")],
      "same-origin",
    );
    const hydrated = resolveSelectedRecord(
      "mains-aegis-a1b2c3",
      [],
      fleetEntries,
    );
    expect(hydrated?.target.deviceId).toBe("mains-aegis-a1b2c3");
  });
});
