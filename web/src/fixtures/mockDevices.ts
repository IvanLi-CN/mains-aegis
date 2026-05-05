import type { DeviceRecord, DeviceTarget, Identity, NetworkSummary, SafeSettingsState, SerialLogEntry, SerialTraceEntry, UpsStatus } from "../api/types";

type MockDefinition = {
  target: DeviceTarget;
  identity: Identity;
  network: NetworkSummary;
  status: UpsStatus;
  connectionState: DeviceRecord["connectionState"];
};

export type DemoSeed = "default" | "dual" | "empty" | "offline" | "large" | "usb";

const demoSeedIds: DemoSeed[] = ["default", "dual", "empty", "offline", "large", "usb"];
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
  if (seed === "usb") {
    return [
      makeMockUsbSerialRecord(),
      makeMockDevdRecord({ baseUrl: "mock:usb", bound: false }),
      makeMockDevdRecord({ baseUrl: "mock:devd", bound: true }),
    ];
  }
  if (seed === "dual") {
    const records = mockDefinitions.map((definition) => recordFromDefinition(definition));
    const first = records[0];
    const usb = makeMockUsbSerialRecord({ deviceId: first.target.deviceId });
    return [
      {
        ...first,
        target: {
          ...first.target,
          serialProtocol: usb.target.serialProtocol,
        },
        streamState: "streaming",
        serial: usb.serial,
      },
      ...records.slice(1),
    ];
  }
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
  if (target.transport === "serial") return makeMockUsbSerialRecord(target);
  const mock = findMock(target.baseUrl);
  return recordFromDefinition({ ...mock, target });
}

export function makeMockUsbSerialRecord(targetOverride?: Partial<DeviceTarget>): DeviceRecord {
  const base = mockDefinitions[0];
  const deviceId = targetOverride?.deviceId ?? base.identity.device_id;
  const hostname = targetOverride?.deviceId ?? base.identity.hostname;
  const identity: Identity = {
    ...base.identity,
    device_id: deviceId,
    hostname,
    hostname_fqdn: `${hostname}.local`,
    short_id: targetOverride?.deviceId ? "usb001" : base.identity.short_id,
    network: {
      ...base.network,
      device_id: deviceId,
      hostname,
      hostname_fqdn: `${hostname}.local`,
      state: "idle",
      ipv4: null,
      gateway: null,
      dns: null,
      rssi_dbm: null,
    },
    capabilities: {
      sse: false,
      mdns: false,
      dns_sd: false,
      write_controls: true,
    },
  };
  const target: DeviceTarget = {
    deviceId: identity.device_id,
    baseUrl: "serial:mock-usb-cdc",
    alias: "USB demo CDC",
    location: "Bench USB",
    addedAt: now,
    transport: "serial",
    serialProtocol: "mains-aegis.cdc.v1",
    mock: true,
    ...targetOverride,
  };
  const serialBase = Date.now() - 240_000;
  const serialTimestamp = (seconds: number) => new Date(serialBase + seconds * 1000).toISOString();
  const serialLogs: SerialLogEntry[] = [
    {
      id: "mock-usb-log-1",
      timestamp: serialTimestamp(3),
      level: "info",
      target: "usb_cdc",
      message: "mock USB CDC session ready",
    },
    {
      id: "mock-usb-log-2",
      timestamp: serialTimestamp(8),
      level: "debug",
      target: "protocol",
      message: "hello capabilities negotiated",
    },
    {
      id: "mock-usb-log-3",
      timestamp: serialTimestamp(14),
      level: "trace",
      target: "status",
      message: "status snapshot published over serial",
    },
    {
      id: "mock-usb-log-4",
      timestamp: serialTimestamp(22),
      level: "debug",
      target: "telemetry",
      message: "output A sample vbus=12064mV current=20mA",
    },
    {
      id: "mock-usb-log-5",
      timestamp: serialTimestamp(29),
      level: "info",
      target: "settings",
      message: "safe setting manual_charge target=rsoc_80 queued",
    },
    {
      id: "mock-usb-log-6",
      timestamp: serialTimestamp(36),
      level: "warn",
      target: "wifi",
      message: "wifi credentials pending reboot to apply",
    },
    {
      id: "mock-usb-log-7",
      timestamp: serialTimestamp(45),
      level: "debug",
      target: "eeprom",
      message: "wifi config crc updated",
    },
    {
      id: "mock-usb-log-8",
      timestamp: serialTimestamp(52),
      level: "info",
      target: "network",
      message: "station state idle until reboot",
    },
    {
      id: "mock-usb-log-9",
      timestamp: serialTimestamp(63),
      level: "error",
      target: "protocol",
      message: "rejected unsafe output_control request in demo session",
    },
    {
      id: "mock-usb-log-10",
      timestamp: serialTimestamp(76),
      level: "trace",
      target: "serial_rx",
      message: "ignored legacy console line",
    },
  ];
  serialLogs.push(
    ...Array.from({ length: 72 }, (_, index): SerialLogEntry => {
      const level = (["trace", "debug", "info", "debug", "warn", "trace"] as const)[index % 6];
      const target = (["status", "telemetry", "charger", "network", "wifi", "serial_rx"] as const)[index % 6];
      const sample = index + 1;
      return {
        id: `mock-usb-log-generated-${sample}`,
        timestamp: serialTimestamp(90 + index * 2),
        level,
        target,
        message:
          target === "telemetry"
            ? `stream sample ${sample} out_a=${12040 + (index % 18)}mV current=${18 + (index % 9)}mA`
            : target === "wifi"
              ? `wifi station check ${sample} state=idle`
              : target === "serial_rx"
                ? `legacy console line ${sample} ignored`
                : `${target} loop sample ${sample} ok`,
      };
    }),
  );
  const serialTrace: SerialTraceEntry[] = [
    {
      id: "mock-usb-trace-1",
      timestamp: serialTimestamp(1),
      direction: "tx",
      kind: "frame",
      frameType: "hello",
      requestId: "web-demo-hello",
      target: null,
      summary: "protocol handshake",
      payload: "{\"type\":\"hello\",\"request_id\":\"web-demo-hello\"}",
    },
    {
      id: "mock-usb-trace-2",
      timestamp: serialTimestamp(3),
      direction: "rx",
      kind: "frame",
      frameType: "hello",
      requestId: "web-demo-hello",
      target: null,
      summary: "capabilities negotiated",
      payload: "{\"type\":\"hello\",\"request_id\":\"web-demo-hello\",\"protocol\":\"mains-aegis.cdc.v1\",\"capabilities\":{\"status_stream\":true,\"structured_logs\":true,\"wifi_config\":true}}",
    },
    {
      id: "mock-usb-trace-3",
      timestamp: serialTimestamp(8),
      direction: "rx",
      kind: "frame",
      frameType: "log",
      requestId: null,
      target: "protocol",
      summary: "hello capabilities negotiated",
      payload: "{\"type\":\"log\",\"level\":\"debug\",\"target\":\"protocol\",\"message\":\"hello capabilities negotiated\"}",
    },
    {
      id: "mock-usb-trace-4",
      timestamp: serialTimestamp(12),
      direction: "tx",
      kind: "frame",
      frameType: "request",
      requestId: "web-status-001",
      target: "status",
      summary: "request current status",
      payload: "{\"type\":\"request\",\"request_id\":\"web-status-001\",\"target\":\"status.get\"}",
    },
    {
      id: "mock-usb-trace-5",
      timestamp: serialTimestamp(14),
      direction: "rx",
      kind: "frame",
      frameType: "status",
      requestId: "web-status-001",
      target: null,
      summary: "status snapshot standby battery 67%",
      payload: "{\"type\":\"status\",\"request_id\":\"web-status-001\",\"mode\":\"standby\",\"battery\":{\"soc_pct\":67},\"output\":{\"active\":\"both\"}}",
    },
    {
      id: "mock-usb-trace-6",
      timestamp: serialTimestamp(22),
      direction: "rx",
      kind: "raw",
      frameType: "defmt",
      requestId: null,
      target: "defmt",
      summary: "telemetry ch=out_a addr=0x74 vset_mv=12000 vbus_mv=12064 current_ma=20 dv_mv=64 vbus_reg=0x2f20 shunt_uv=200 oe=true fpwm=false status=0x1 scp=false ocp=false ovp=false vout_sr=0x30 cdc=0x0 iout_limit=0xc6 tmp_addr=0x48 temp_c_x16=499 therm_kill_n=1 temp_c=31.1875",
      payload: "telemetry ch=out_a addr=0x74 vset_mv=12000 vbus_mv=12064 current_ma=20 dv_mv=64 vbus_reg=0x2f20 shunt_uv=200 oe=true fpwm=false status=0x1 scp=false ocp=false ovp=false vout_sr=0x30 cdc=0x0 iout_limit=0xc6 tmp_addr=0x48 temp_c_x16=499 therm_kill_n=1 temp_c=31.1875",
    },
    {
      id: "mock-usb-trace-7",
      timestamp: serialTimestamp(29),
      direction: "tx",
      kind: "frame",
      frameType: "request",
      requestId: "web-settings-002",
      target: "settings",
      summary: "apply manual charge preference",
      payload: "{\"type\":\"request\",\"request_id\":\"web-settings-002\",\"target\":\"settings.safe.update\",\"manual_charge\":{\"target\":\"rsoc_80\",\"speed\":\"ma_500\",\"timer\":\"h_2\"}}",
    },
    {
      id: "mock-usb-trace-8",
      timestamp: serialTimestamp(31),
      direction: "rx",
      kind: "frame",
      frameType: "response",
      requestId: "web-settings-002",
      target: "settings",
      summary: "manual charge preference accepted",
      payload: "{\"type\":\"response\",\"request_id\":\"web-settings-002\",\"ok\":true,\"target\":\"settings.safe.update\"}",
    },
    {
      id: "mock-usb-trace-9",
      timestamp: serialTimestamp(36),
      direction: "tx",
      kind: "frame",
      frameType: "wifi_config",
      requestId: "web-wifi-003",
      target: "wifi",
      summary: "write WiFi credentials",
      payload: "{\"type\":\"wifi_config\",\"request_id\":\"web-wifi-003\",\"ssid\":\"Lab-UPS\",\"psk\":\"***redacted***\"}",
    },
    {
      id: "mock-usb-trace-10",
      timestamp: serialTimestamp(39),
      direction: "rx",
      kind: "frame",
      frameType: "response",
      requestId: "web-wifi-003",
      target: "wifi",
      summary: "wifi config stored without PSK echo",
      payload: "{\"type\":\"response\",\"request_id\":\"web-wifi-003\",\"ok\":true,\"target\":\"wifi_config\",\"ssid\":\"Lab-UPS\"}",
    },
    {
      id: "mock-usb-trace-11",
      timestamp: serialTimestamp(45),
      direction: "rx",
      kind: "frame",
      frameType: "log",
      requestId: null,
      target: "eeprom",
      summary: "wifi config crc updated",
      payload: "{\"type\":\"log\",\"level\":\"debug\",\"target\":\"eeprom\",\"message\":\"wifi config crc updated\"}",
    },
    {
      id: "mock-usb-trace-12",
      timestamp: serialTimestamp(52),
      direction: "rx",
      kind: "frame",
      frameType: "log",
      requestId: null,
      target: "network",
      summary: "station state idle until reboot",
      payload: "{\"type\":\"log\",\"level\":\"info\",\"target\":\"network\",\"message\":\"station state idle until reboot\"}",
    },
    {
      id: "mock-usb-trace-13",
      timestamp: serialTimestamp(63),
      direction: "tx",
      kind: "frame",
      frameType: "request",
      requestId: "web-unsafe-004",
      target: "output",
      summary: "unsafe output control blocked",
      payload: "{\"type\":\"request\",\"request_id\":\"web-unsafe-004\",\"target\":\"output_control\",\"action\":\"enable\"}",
    },
    {
      id: "mock-usb-trace-14",
      timestamp: serialTimestamp(64),
      direction: "rx",
      kind: "frame",
      frameType: "error",
      requestId: "web-unsafe-004",
      target: "output",
      summary: "unsafe request rejected",
      payload: "{\"type\":\"error\",\"request_id\":\"web-unsafe-004\",\"code\":\"unsafe_operation\",\"message\":\"output control is not available over Web Serial\"}",
    },
    {
      id: "mock-usb-trace-15",
      timestamp: serialTimestamp(76),
      direction: "rx",
      kind: "ignored",
      frameType: null,
      requestId: null,
      target: null,
      summary: "ignored legacy console line",
      payload: "ets Jul 29 2019 12:21:46 rst:0x1 boot:0x13",
    },
    {
      id: "mock-usb-trace-16",
      timestamp: serialTimestamp(84),
      direction: "rx",
      kind: "raw",
      frameType: null,
      requestId: null,
      target: null,
      summary: "raw CDC line",
      payload: "[DEBUG] heap_free=183224 loop_ms=4 serial_rx=ok",
    },
  ];
  serialTrace.push(
    ...Array.from({ length: 120 }, (_, index): SerialTraceEntry => {
      const sample = index + 1;
      const sequence = sample + 16;
      const timestamp = serialTimestamp(90 + index);
      const requestId = `web-stream-${String(sample).padStart(3, "0")}`;
      switch (index % 8) {
        case 0:
          return {
            id: `mock-usb-trace-generated-${sequence}`,
            timestamp,
            direction: "rx",
            kind: "frame",
            frameType: "status",
            requestId,
            target: null,
            summary: `status stream sample ${sample}`,
            payload: JSON.stringify({
              type: "status",
              request_id: requestId,
              mode: "standby",
              battery: { soc_pct: 67 - (index % 4) },
              output: { active: "both", out_a_mv: 12040 + (index % 18), out_a_ma: 18 + (index % 9) },
            }),
          };
        case 1:
          return {
            id: `mock-usb-trace-generated-${sequence}`,
            timestamp,
            direction: "rx",
            kind: "frame",
            frameType: "log",
            requestId: null,
            target: "telemetry",
            summary: `telemetry log sample ${sample}`,
            payload: JSON.stringify({
              type: "log",
              level: index % 16 === 1 ? "warn" : "debug",
              target: "telemetry",
              message: `out_a=${12040 + (index % 18)}mV current=${18 + (index % 9)}mA`,
            }),
          };
        case 2:
          return {
            id: `mock-usb-trace-generated-${sequence}`,
            timestamp,
            direction: "rx",
            kind: "raw",
            frameType: null,
            requestId: null,
            target: null,
            summary: "raw CDC telemetry line",
            payload: `[INFO ] telemetry sample=${sample} ch=out_a vbus_mv=${12040 + (index % 18)} current_ma=${18 + (index % 9)}`,
          };
        case 3:
          return {
            id: `mock-usb-trace-generated-${sequence}`,
            timestamp,
            direction: "tx",
            kind: "frame",
            frameType: "request",
            requestId,
            target: "status",
            summary: `poll status request ${sample}`,
            payload: JSON.stringify({ type: "request", request_id: requestId, target: "status.get" }),
          };
        case 4:
          return {
            id: `mock-usb-trace-generated-${sequence}`,
            timestamp,
            direction: "rx",
            kind: "frame",
            frameType: "response",
            requestId,
            target: "status",
            summary: `poll status response ${sample}`,
            payload: JSON.stringify({ type: "response", request_id: requestId, ok: true, target: "status.get" }),
          };
        case 5:
          return {
            id: `mock-usb-trace-generated-${sequence}`,
            timestamp,
            direction: "rx",
            kind: "ignored",
            frameType: null,
            requestId: null,
            target: null,
            summary: "ignored legacy console line",
            payload: `rst:0x1 boot:0x13 legacy diagnostic sample=${sample}`,
          };
        case 6:
          return {
            id: `mock-usb-trace-generated-${sequence}`,
            timestamp,
            direction: "tx",
            kind: "frame",
            frameType: "request",
            requestId,
            target: "settings",
            summary: `safe settings dry-run ${sample}`,
            payload: JSON.stringify({
              type: "request",
              request_id: requestId,
              target: "settings.safe.update",
              display: { log_level: sample % 3 === 0 ? "trace" : "debug" },
            }),
          };
        default:
          return {
            id: `mock-usb-trace-generated-${sequence}`,
            timestamp,
            direction: "rx",
            kind: "frame",
            frameType: index % 24 === 7 ? "error" : "response",
            requestId,
            target: "settings",
            summary: index % 24 === 7 ? `settings validation warning ${sample}` : `safe settings accepted ${sample}`,
            payload: JSON.stringify(
              index % 24 === 7
                ? { type: "error", request_id: requestId, code: "demo_validation", message: "demo warning sample" }
                : { type: "response", request_id: requestId, ok: true, target: "settings.safe.update" },
            ),
          };
      }
    }),
  );
  return {
    target,
    identity,
    network: identity.network,
    status: {
      ...base.status,
      network: {
        state: "idle",
        ipv4: null,
        last_error: null,
      },
    },
    connectionState: "online",
    streamState: "streaming",
    error: null,
    lastUpdated: new Date().toISOString(),
    serial: {
      connected: true,
      source: "mock",
      protocol: "mains-aegis.cdc.v1",
      logs: serialLogs,
      trace: serialTrace,
      safeSettings: defaultMockSafeSettings(),
    },
  };
}

export function makeMockDevdRecord(options: { baseUrl?: string; bound?: boolean } = {}): DeviceRecord {
  const baseUrl = options.baseUrl ?? "mock:devd";
  const bound = options.bound ?? true;
  const target: DeviceTarget = {
    deviceId: bound ? "mains-aegis-devd-bridge" : "mains-aegis-devd-unbound",
    baseUrl,
    alias: bound ? "USB devd bridge" : "USB devd pending bind",
    location: "Bench USB",
    addedAt: now,
    transport: "adapter",
    mock: false,
  };
  const identity = {
    ...mockDefinitions[0].identity,
      device_id: target.deviceId,
      hostname: bound ? "mains-aegis-devd-bridge" : "mains-aegis-devd-unbound",
      hostname_fqdn: `${bound ? "mains-aegis-devd-bridge" : "mains-aegis-devd-unbound"}.local`,
      short_id: bound ? "devd01" : "devd00",
      firmware: {
        ...mockDefinitions[0].identity.firmware,
        package_version: "0.1.0",
        build_profile: "release",
      build_id: `${bound ? "devd01" : "devd00"}-clean-demo`,
      git_sha: "fea0b19",
      src_hash: bound ? "devd01" : "devd00",
      git_dirty: "clean",
      features: ["web_serial", "devd"],
    },
    capabilities: {
      sse: true,
      mdns: true,
      dns_sd: true,
      write_controls: true,
    },
  } satisfies Identity;
  const network: NetworkSummary = {
    ...identity.network,
    device_id: target.deviceId,
    hostname: bound ? "mains-aegis-devd-bridge" : "mains-aegis-devd-unbound",
    hostname_fqdn: `${bound ? "mains-aegis-devd-bridge" : "mains-aegis-devd-unbound"}.local`,
    state: "connected",
    ipv4: "192.168.31.63",
    gateway: "192.168.31.1",
    dns: "1.1.1.1",
    is_static: false,
    last_error: null,
    rssi_dbm: -48,
  };
  const deviceStatus: UpsStatus = {
    ...status("standby", 74),
    network: {
      state: "connected",
      ipv4: "192.168.31.63",
      last_error: null,
    },
  };

  return {
    target,
    identity,
    network,
    status: deviceStatus,
    connectionState: "online",
    streamState: "polling",
    error: null,
    lastUpdated: new Date().toISOString(),
  };
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

function defaultMockSafeSettings(): SafeSettingsState {
  return {
    wifi_configured: false,
    wifi_ssid: null,
    log_level: "info",
    manual_charge: {
      target: "full_100",
      speed: "ma_500",
      timer_h: 2,
    },
  };
}

function findMock(baseUrl: string): MockDefinition {
  const match = [...mockDefinitions, ...largeMockDefinitions].find((definition) => definition.target.baseUrl === baseUrl);
  if (!match) throw new Error(`unknown mock device: ${baseUrl}`);
  return match;
}
