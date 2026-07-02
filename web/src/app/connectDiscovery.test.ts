import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import type { DeviceRecord, DevdDevice } from "../api/types";
import {
  buildDiscoveredLogicalDevices,
  buildFleetEntries,
  detectBrowserLanCapability,
  expandIpv4Cidr,
  isLanIdentityCandidate,
  resolveConnectRuntimeMode,
  resolveOwnerFacingDevdTarget,
  resolveSelectedRecord,
  resolveUpsHardwareCapability,
  ScanActionRow,
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
    display_name: "/tmp/fixture-usb-a",
    port_path: "/tmp/fixture-usb-a",
    lan_address: null,
    lan_conflict_addresses: [],
    transport: "native_serial",
    binding: {
      alias: "Bench USB",
      stable_id: "usb-stable-a",
      port_path: "/tmp/fixture-usb-a",
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

  test("keeps same-origin devd-backed saved entries online without any mock query target", () => {
    const entries = buildFleetEntries(
      [savedRecord("mains-aegis-a1b2c3")],
      [lanDevice("mains-aegis-a1b2c3")],
      "same-origin",
    );

    expect(entries).toHaveLength(1);
    expect(entries[0]?.record.connectionState).toBe("online");
    expect(entries[0]?.record.target.transport).toBe("http");
    expect(entries[0]?.record.target.baseUrl).toBe("http://192.168.31.42");
    expect(entries[0]?.record.target.rememberedChannels?.http?.baseUrl).toBe(
      "http://192.168.31.42",
    );
    expect(entries[0]?.record.target.rememberedChannels?.devd).toBeUndefined();
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

  test("prefers a hydrated fleet record over a temporary registry shell without status", () => {
    const registryShell: DeviceRecord = {
      ...savedRecord("mains-aegis-a1b2c3"),
      target: {
        ...savedRecord("mains-aegis-a1b2c3").target,
        temporary: true,
      },
      status: null,
    };
    const fleetRecord: DeviceRecord = {
      ...registryShell,
      status: {
        mode: "backup",
        input: {
          source: "usbc",
          mains_present: false,
          input_vbus_mv: 5100,
          input_ibus_ma: 10,
          vin_vbus_mv: 1600,
          vin_iin_ma: 5,
          tps_total_iout_ma: 40,
          tps_limit_threshold_ma: 100,
          pressure_state: "inactive",
          pressure_score_pct: 0,
          pressure_reason: "none",
          vin_baseline_mv: null,
          vin_drop_mv: null,
          assist_power_stage: "backup",
          assist_target_vout_mv: 12000,
        },
        output: {
          requested: "both",
          active: "both",
          recoverable: "both",
          gate_reason: "none",
          out_a: { state: "ok", enabled: true, vbus_mv: 12064, iout_ma: 20 },
          out_b: { state: "ok", enabled: true, vbus_mv: 12072, iout_ma: 20 },
        },
        charger: {
          state: "ok",
          allow_charge: false,
          ichg_ma: null,
          ibat_ma: 0,
          vbat_present: true,
          policy_target_ichg_ma: null,
          limit_active: false,
          limit_reason: "none",
          limit_detail: "none",
          limit_threshold_ma: null,
          detail_status: "NOAC",
        },
        battery: {
          state: "ok",
          pack_mv: 15669,
          current_ma: -38,
          soc_pct: 89,
          cell_mv: [3924, 3925, 3905, 3914],
          cell_delta_mv: 20,
          balance_enabled: true,
          balance_cfg_match: true,
          balance_active: false,
          balance_mask: 0,
          balance_cell: null,
          balance_min_start_delta_mv: 3,
          no_battery: false,
          discharge_ready: true,
          charge_fet_on: true,
          discharge_fet_on: true,
          precharge_fet_on: false,
          issue_detail: null,
          recovery_pending: false,
        },
        thermal: {
          tmp_a_state: "ok",
          tmp_a_c: 37,
          tmp_b_state: "ok",
          tmp_b_c: 38,
        },
        network: {
          state: "connected",
          ipv4: "192.168.31.42",
          last_error: null,
        },
      },
    };
    const fleetEntries = [
      { key: "mains-aegis-a1b2c3", record: fleetRecord, saved: false },
    ];

    const selected = resolveSelectedRecord(
      "mains-aegis-a1b2c3",
      [registryShell],
      fleetEntries,
    );

    expect(selected).toBe(fleetRecord);
  });
});

describe("resolveOwnerFacingDevdTarget", () => {
  test("ignores mock query targets outside demo mode", () => {
    expect(resolveOwnerFacingDevdTarget("mock:devd", false)).toBeUndefined();
    expect(resolveOwnerFacingDevdTarget("same-origin", false)).toBe(
      "same-origin",
    );
  });

  test("keeps mock query targets in demo mode", () => {
    expect(resolveOwnerFacingDevdTarget("mock:devd", true)).toBe("mock:devd");
  });
});

describe("resolveUpsHardwareCapability", () => {
  test("prefers hardware identity capabilities over settings fallback", () => {
    const capability = resolveUpsHardwareCapability({
      identity: {
        ...lanDevice("mains-aegis-a1b2c3").identity!,
        hardware_capabilities: {
          output_profile: "19v",
          rated_vout_mv: 19000,
        },
      },
      settings: {
        wifi: { configured: false, ssid: null },
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
          assist_enter_delta_ma: { default: 0, min: -100, max: 1000, step: 50 },
          assist_exit_delta_ma: { default: 0, min: -50, max: 1000, step: 50 },
          assist_required_samples: { default: 2, min: 1, max: 5, step: 1 },
          assist_ramp_step_mv: { default: 100, min: 20, max: 1000, step: 20 },
          assist_ramp_interval_ms: { default: 200, min: 100, max: 3000, step: 100 },
          rated_enter_delta_ma: { default: 0, min: -100, max: 1000, step: 50 },
          rated_exit_delta_ma: { default: 0, min: -50, max: 1000, step: 50 },
          vin_drop_threshold_pct: { default: 4, min: 1, max: 12, step: 1 },
          required_samples: { default: 2, min: 1, max: 5, step: 1 },
        },
      },
    });

    expect(capability).toEqual({
      outputProfile: "19v",
      ratedVoutMv: 19000,
      source: "identity",
    });
  });

  test("falls back to the advanced-power baseline when identity is missing", () => {
    const capability = resolveUpsHardwareCapability({
      identity: null,
      settings: {
        wifi: { configured: false, ssid: null },
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
          assist_enter_delta_ma: { default: 0, min: -100, max: 1000, step: 50 },
          assist_exit_delta_ma: { default: 0, min: -50, max: 1000, step: 50 },
          assist_required_samples: { default: 2, min: 1, max: 5, step: 1 },
          assist_ramp_step_mv: { default: 100, min: 20, max: 1000, step: 20 },
          assist_ramp_interval_ms: { default: 200, min: 100, max: 3000, step: 100 },
          rated_enter_delta_ma: { default: 0, min: -100, max: 1000, step: 50 },
          rated_exit_delta_ma: { default: 0, min: -50, max: 1000, step: 50 },
          vin_drop_threshold_pct: { default: 4, min: 1, max: 12, step: 1 },
          required_samples: { default: 2, min: 1, max: 5, step: 1 },
        },
      },
    });

    expect(capability).toEqual({
      outputProfile: "12v",
      ratedVoutMv: 12000,
      source: "settings",
    });
  });

  test("prefers active firmware output profile over settings fallback when capability fields are missing", () => {
    const capability = resolveUpsHardwareCapability({
      identity: {
        ...lanDevice("mains-aegis-a1b2c3").identity!,
        hardware_capabilities: undefined,
        firmware: {
          ...lanDevice("mains-aegis-a1b2c3").identity!.firmware,
          features: ["net_http", "web_serial", "main-vout-19v"],
        },
      },
      settings: {
        wifi: { configured: false, ssid: null },
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
          assist_enter_delta_ma: { default: 0, min: -100, max: 1000, step: 50 },
          assist_exit_delta_ma: { default: 0, min: -50, max: 1000, step: 50 },
          assist_required_samples: { default: 2, min: 1, max: 5, step: 1 },
          assist_ramp_step_mv: { default: 100, min: 20, max: 1000, step: 20 },
          assist_ramp_interval_ms: { default: 200, min: 100, max: 3000, step: 100 },
          rated_enter_delta_ma: { default: 0, min: -100, max: 1000, step: 50 },
          rated_exit_delta_ma: { default: 0, min: -50, max: 1000, step: 50 },
          vin_drop_threshold_pct: { default: 4, min: 1, max: 12, step: 1 },
          required_samples: { default: 2, min: 1, max: 5, step: 1 },
        },
      },
    });

    expect(capability).toEqual({
      outputProfile: "19v",
      ratedVoutMv: 19000,
      source: "firmware",
    });
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

describe("public static connect runtime", () => {
  test("treats a Pages build without devd as public static", () => {
    expect(
      resolveConnectRuntimeMode({
        hostedHttpServiceApp: false,
        devdTarget: null,
        publicStaticBuild: true,
      }),
    ).toBe("public_static");
  });

  test("keeps explicit devd discovery semantics when a Pages build has a devd target", () => {
    expect(
      resolveConnectRuntimeMode({
        hostedHttpServiceApp: false,
        devdTarget: "http://127.0.0.1:30080",
        publicStaticBuild: true,
      }),
    ).toBe("standalone_with_devd");
  });
});

describe("browser direct LAN capability", () => {
  test("requires secure context and Chrome 142+", () => {
    expect(
      detectBrowserLanCapability({
        isSecureContext: true,
        userAgent:
          "Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36",
      }).supported,
    ).toBe(true);

    expect(
      detectBrowserLanCapability({
        isSecureContext: false,
        userAgent:
          "Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36",
      }).reason,
    ).toContain("Secure context");

    expect(
      detectBrowserLanCapability({
        isSecureContext: true,
        userAgent:
          "Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36",
      }).reason,
    ).toContain("Chrome 142+");
  });
});

describe("CIDR scan contract", () => {
  test("expands host addresses inside the allowed range", () => {
    const expanded = expandIpv4Cidr("192.168.31.40/29");
    expect(expanded.normalized).toBe("192.168.31.40/29");
    expect(expanded.hosts).toEqual([
      "192.168.31.41",
      "192.168.31.42",
      "192.168.31.43",
      "192.168.31.44",
      "192.168.31.45",
      "192.168.31.46",
    ]);
  });

  test("rejects CIDR ranges outside the 2..256 host contract", () => {
    expect(() => expandIpv4Cidr("192.168.31.0/31")).toThrow(
      "CIDR scan must expand to between 2 and 256 hosts.",
    );
    expect(() => expandIpv4Cidr("192.168.31.0/23")).toThrow(
      "CIDR scan must expand to between 2 and 256 hosts.",
    );
  });

  test("only accepts identities that satisfy the device contract", () => {
    expect(
      isLanIdentityCandidate({
        device_id: "mains-aegis-a1b2c3",
        hostname: "mains-aegis-a1b2c3",
        hostname_fqdn: "mains-aegis-a1b2c3.local",
        short_id: "a1b2c3",
        role: "ups",
        api_version: "v1",
        firmware: {
          package_version: "0.1.0",
          build_profile: "release",
          build_id: "build",
          git_sha: "abc123",
          src_hash: "src",
          git_dirty: "false",
          protocol: "mains-aegis.cdc.v1",
        },
        network: {
          device_id: "mains-aegis-a1b2c3",
          hostname: "mains-aegis-a1b2c3",
          hostname_fqdn: "mains-aegis-a1b2c3.local",
          state: "connected",
          ipv4: "192.168.31.42",
          gateway: null,
          dns: null,
          is_static: false,
          last_error: null,
          rssi_dbm: null,
        },
        capabilities: {
          sse: true,
          mdns: true,
          dns_sd: true,
          write_controls: true,
        },
      }),
    ).toBe(true);

    expect(
      isLanIdentityCandidate({
        device_id: "",
        hostname: "stale-service",
        hostname_fqdn: "stale-service.local",
        short_id: "stale",
        role: "service",
        api_version: "v2",
        firmware: {
          package_version: "0.1.0",
          build_profile: "release",
          build_id: "build",
          git_sha: "abc123",
          src_hash: "src",
          git_dirty: "false",
          protocol: "mains-aegis.cdc.v1",
        },
        network: {
          device_id: "",
          hostname: "stale-service",
          hostname_fqdn: "stale-service.local",
          state: "connected",
          ipv4: "192.168.31.99",
          gateway: null,
          dns: null,
          is_static: false,
          last_error: null,
          rssi_dbm: null,
        },
        capabilities: {
          sse: true,
          mdns: false,
          dns_sd: false,
          write_controls: false,
        },
      }),
    ).toBe(false);
  });

  test("keeps the scan summary in a reserved inline status slot", () => {
    const idleMarkup = renderToStaticMarkup(
      ScanActionRow({
        busy: false,
        disabled: false,
        buttonText: "Scan LAN",
        busyText: "Scanning",
        successFeedback: null,
        errorMessage: null,
      }),
    );
    expect(idleMarkup).toContain('data-slot="scan-inline-status"');
    expect(idleMarkup).toContain('aria-live="polite"');

    const successMarkup = renderToStaticMarkup(
      ScanActionRow({
        busy: false,
        disabled: false,
        buttonText: "Scan LAN",
        busyText: "Scanning",
        successFeedback: {
          tone: "success",
          message: "Found 2 devices in 192.168.31.40/29",
        },
        errorMessage: null,
      }),
    );

    expect(successMarkup).toContain('data-slot="scan-inline-status"');
    expect(successMarkup).toContain("Found 2 devices in 192.168.31.40/29");
    expect(successMarkup.indexOf("Scan LAN")).toBeLessThan(
      successMarkup.indexOf("Found 2 devices in 192.168.31.40/29"),
    );
  });

  test("keeps unsupported scan buttons disabled without showing a busy label", () => {
    const disabledMarkup = renderToStaticMarkup(
      ScanActionRow({
        busy: false,
        disabled: true,
        buttonText: "Scan LAN",
        busyText: "Scanning",
        successFeedback: null,
        errorMessage: null,
      }),
    );

    expect(disabledMarkup).toContain("Scan LAN");
    expect(disabledMarkup).not.toContain("Scanning");
    expect(disabledMarkup).toContain("disabled");
  });
});
