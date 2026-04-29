import type { DeviceRecord, DeviceTarget, Identity, NetworkSummary, UpsStatus } from "../api/types";

type MockDefinition = {
  target: DeviceTarget;
  identity: Identity;
  network: NetworkSummary;
  status: UpsStatus;
  connectionState: DeviceRecord["connectionState"];
};

export type DemoSeed = "default" | "empty" | "offline" | "large";

const demoSeedIds: DemoSeed[] = ["default", "empty", "offline", "large"];
const now = "2026-04-28T00:00:00.000Z";

function identity(deviceId: string, shortId: string, state: NetworkSummary["state"], ipv4: string | null): Identity {
  const network: NetworkSummary = {
    device_id: deviceId,
    hostname: deviceId,
    hostname_fqdn: `${deviceId}.local`,
    state,
    ipv4,
    gateway: ipv4 ? "192.168.31.1" : null,
    dns: ipv4 ? "1.1.1.1" : null,
    is_static: false,
    last_error: state === "error" ? "link_lost" : null,
    rssi_dbm: ipv4 ? -54 : null,
  };

  return {
    device_id: deviceId,
    hostname: deviceId,
    hostname_fqdn: `${deviceId}.local`,
    short_id: shortId,
    role: "ups",
    api_version: "v1",
    firmware: {
      package_version: "0.1.0",
      build_profile: "dev",
      build_id: `${shortId}-clean-demo`,
      git_sha: "fea0b19",
      src_hash: shortId,
      git_dirty: "clean",
    },
    network,
    capabilities: {
      sse: true,
      mdns: true,
      dns_sd: true,
      write_controls: false,
    },
  };
}

function status(
  mode: UpsStatus["mode"],
  soc: number | null,
  overrides: Partial<UpsStatus> = {},
): UpsStatus {
  return {
    mode,
    input: {
      mains_present: mode !== "backup",
      input_vbus_mv: mode === "backup" ? 0 : 19240,
      input_ibus_ma: mode === "backup" ? 0 : 1180,
      vin_vbus_mv: mode === "backup" ? 0 : 19240,
      vin_iin_ma: mode === "backup" ? 0 : 1180,
    },
    output: {
      requested: "both",
      active: mode === "fault" ? "none" : "both",
      recoverable: mode === "fault" ? "none" : "both",
      gate_reason: mode === "fault" ? "battery_protection" : "none",
      out_a: {
        state: mode === "fault" ? "fault" : "ok",
        enabled: mode !== "fault",
        vbus_mv: mode === "fault" ? 0 : 19020,
        iout_ma: mode === "fault" ? 0 : 430,
      },
      out_b: {
        state: mode === "fault" ? "hold" : "ok",
        enabled: mode !== "fault",
        vbus_mv: mode === "fault" ? 0 : 19010,
        iout_ma: mode === "fault" ? 0 : 220,
      },
    },
    charger: {
      state: mode === "backup" ? "idle" : "ok",
      allow_charge: mode !== "backup" && mode !== "fault",
      ichg_ma: mode === "backup" ? 0 : 520,
      ibat_ma: mode === "backup" ? -260 : 510,
      vbat_present: true,
    },
    battery: {
      state: soc !== null && soc < 25 ? "warning" : "ok",
      pack_mv: soc === null ? null : 15260,
      current_ma: mode === "backup" ? -380 : 180,
      soc_pct: soc,
      no_battery: false,
      discharge_ready: mode !== "fault",
      issue_detail: null,
      recovery_pending: false,
      last_result: null,
    },
    thermal: {
      tmp_a_state: "ok",
      tmp_a_c: 39,
      tmp_b_state: "ok",
      tmp_b_c: 37,
    },
    network: {
      state: "connected",
      ipv4: "192.168.31.42",
      last_error: null,
    },
    ...overrides,
  };
}

export const mockDefinitions: MockDefinition[] = [
  {
    target: { deviceId: "mains-aegis-a1b2c3", baseUrl: "mock:lab-standby", alias: "Lab rack A", location: "Bench 1", addedAt: now, mock: true },
    identity: identity("mains-aegis-a1b2c3", "a1b2c3", "connected", "192.168.31.42"),
    network: identity("mains-aegis-a1b2c3", "a1b2c3", "connected", "192.168.31.42").network,
    status: status("standby", 67),
    connectionState: "online",
  },
  {
    target: { deviceId: "mains-aegis-b4c5d6", baseUrl: "mock:lab-assist", alias: "NAS shelf UPS", location: "Rack 2U", addedAt: now, mock: true },
    identity: identity("mains-aegis-b4c5d6", "b4c5d6", "connected", "192.168.31.43"),
    network: identity("mains-aegis-b4c5d6", "b4c5d6", "connected", "192.168.31.43").network,
    status: status("assist", 54, { charger: { state: "ok", allow_charge: true, ichg_ma: 480, ibat_ma: 120, vbat_present: true } }),
    connectionState: "online",
  },
  {
    target: { deviceId: "mains-aegis-c7d8e9", baseUrl: "mock:backup", alias: "Router backup", location: "Network closet", addedAt: now, mock: true },
    identity: identity("mains-aegis-c7d8e9", "c7d8e9", "connected", "192.168.31.44"),
    network: identity("mains-aegis-c7d8e9", "c7d8e9", "connected", "192.168.31.44").network,
    status: status("backup", 41),
    connectionState: "online",
  },
  {
    target: { deviceId: "mains-aegis-d1e2f3", baseUrl: "mock:warning", alias: "Printer shelf", location: "Studio", addedAt: now, mock: true },
    identity: identity("mains-aegis-d1e2f3", "d1e2f3", "connected", "192.168.31.45"),
    network: identity("mains-aegis-d1e2f3", "d1e2f3", "connected", "192.168.31.45").network,
    status: status("standby", 18, {
      battery: {
        state: "warning",
        pack_mv: 13880,
        current_ma: 90,
        soc_pct: 18,
        no_battery: false,
        discharge_ready: true,
        issue_detail: "low_soc",
        recovery_pending: false,
        last_result: null,
      },
    }),
    connectionState: "online",
  },
  {
    target: { deviceId: "mains-aegis-e4f5a6", baseUrl: "mock:critical", alias: "Storage bay", location: "Bench 3", addedAt: now, mock: true },
    identity: identity("mains-aegis-e4f5a6", "e4f5a6", "connected", "192.168.31.46"),
    network: identity("mains-aegis-e4f5a6", "e4f5a6", "connected", "192.168.31.46").network,
    status: status("fault", 33, {
      battery: {
        state: "fault",
        pack_mv: 14220,
        current_ma: 0,
        soc_pct: 33,
        no_battery: false,
        discharge_ready: false,
        issue_detail: "battery_protection",
        recovery_pending: true,
        last_result: "bms_discharge_blocked",
      },
      thermal: {
        tmp_a_state: "hot",
        tmp_a_c: 71,
        tmp_b_state: "warm",
        tmp_b_c: 58,
      },
    }),
    connectionState: "error",
  },
  {
    target: { deviceId: "mains-aegis-f7a8b9", baseUrl: "mock:offline", alias: "Spare pack", location: "Shelf", addedAt: now, mock: true },
    identity: identity("mains-aegis-f7a8b9", "f7a8b9", "error", null),
    network: identity("mains-aegis-f7a8b9", "f7a8b9", "error", null).network,
    status: status("off", null, {
      network: { state: "error", ipv4: null, last_error: "link_lost" },
    }),
    connectionState: "offline",
  },
];

const largeLocations = ["Bench 1", "Bench 2", "Rack 2U", "Network closet", "Storage bay", "Studio", "Shelf", "Lab cart"];
const largeAliases = [
  "Lab rack A",
  "NAS shelf UPS",
  "Router backup",
  "Printer shelf",
  "Storage bay",
  "Spare pack",
  "Camera bridge",
  "Switch stack",
  "Build bench",
  "Instrument rail",
  "Server shelf",
  "Door controller",
  "Fiber closet",
  "Studio lights",
  "QA cart",
  "Reception AP",
  "Cold aisle",
  "Long hostname UPS for wrapping validation",
];

const largeModes: Array<UpsStatus["mode"]> = ["standby", "assist", "backup", "standby", "fault", "off"];

const largeMockDefinitions: MockDefinition[] = largeAliases.map((alias, index) => {
  const n = index + 1;
  const shortId = `l${String(n).padStart(5, "0")}`;
  const deviceId = `mains-aegis-${shortId}`;
  const mode = largeModes[index % largeModes.length];
  const isOffline = mode === "off" || index % 11 === 8;
  const isFault = mode === "fault";
  const soc = isOffline ? null : Math.max(9, 82 - index * 4);
  const network = identity(deviceId, shortId, isOffline ? "error" : "connected", isOffline ? null : `192.168.31.${60 + n}`).network;
  return {
    target: {
      deviceId,
      baseUrl: `mock:large-${String(n).padStart(2, "0")}`,
      alias,
      location: largeLocations[index % largeLocations.length],
      addedAt: now,
      mock: true,
    },
    identity: identity(deviceId, shortId, isOffline ? "error" : "connected", isOffline ? null : `192.168.31.${60 + n}`),
    network,
    status: status(mode, soc, {
      network: { state: isOffline ? "error" : "connected", ipv4: network.ipv4, last_error: isOffline ? "link_lost" : null },
      battery: {
        state: isFault ? "fault" : soc !== null && soc < 25 ? "warning" : isOffline ? "missing" : "ok",
        pack_mv: soc === null ? null : 15100 - index * 32,
        current_ma: mode === "backup" ? -320 : isOffline ? null : 160,
        soc_pct: soc,
        no_battery: isOffline,
        discharge_ready: !isFault && !isOffline,
        issue_detail: isFault ? "battery_protection" : soc !== null && soc < 25 ? "low_soc" : null,
        recovery_pending: isFault,
        last_result: isFault ? "bms_discharge_blocked" : null,
      },
    }),
    connectionState: isOffline ? "offline" : isFault ? "error" : "online",
  };
});

export const mockTargets = mockDefinitions.map((definition) => definition.target);

export function isDemoSeed(value: string | null | undefined): value is DemoSeed {
  return demoSeedIds.includes(value as DemoSeed);
}

export function makeMockRecords(seed: DemoSeed = "default"): DeviceRecord[] {
  if (seed === "empty") return [];
  if (seed === "large") return largeMockDefinitions.map((definition) => recordFromDefinition(definition));
  if (seed === "offline") {
    return mockDefinitions.map((definition) => ({
      ...recordFromDefinition(definition),
      connectionState: "offline",
      streamState: "polling",
      error: { code: "link_lost", message: "device is offline", retryable: true, details: null },
      status: definition.status
        ? {
            ...definition.status,
            network: { state: "error", ipv4: null, last_error: "link_lost" },
          }
        : null,
    }));
  }
  return mockDefinitions.map((definition) => recordFromDefinition(definition));
}

export function getMockIdentity(baseUrl: string): Identity {
  return findMock(baseUrl).identity;
}

export function getMockNetwork(baseUrl: string): NetworkSummary {
  return findMock(baseUrl).network;
}

export function getMockStatus(baseUrl: string): UpsStatus {
  return findMock(baseUrl).status;
}

export function makeMockRecord(target: DeviceTarget): DeviceRecord {
  const mock = findMock(target.baseUrl);
  return recordFromDefinition({ ...mock, target });
}

function recordFromDefinition(mock: MockDefinition): DeviceRecord {
  return {
    target: mock.target,
    identity: mock.identity,
    network: mock.network,
    status: mock.status,
    connectionState: mock.connectionState,
    streamState: mock.connectionState === "online" ? "streaming" : "polling",
    error: mock.connectionState === "offline" ? { code: "link_lost", message: "device is offline", retryable: true, details: null } : null,
    lastUpdated: new Date().toISOString(),
  };
}

function findMock(baseUrl: string): MockDefinition {
  const match = [...mockDefinitions, ...largeMockDefinitions].find((definition) => definition.target.baseUrl === baseUrl);
  if (!match) throw new Error(`unknown mock device: ${baseUrl}`);
  return match;
}
