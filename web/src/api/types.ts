export type ApiErrorEnvelope = {
  error: {
    code: string;
    message: string;
    retryable: boolean;
    details: unknown | null;
  };
};

export type NetworkState = "disabled" | "idle" | "connecting" | "connected" | "error";

export type FirmwareInfo = {
  package_version: string;
  build_profile: string;
  build_id: string;
  git_sha: string;
  src_hash: string;
  git_dirty: string;
  features?: string[];
  protocol?: "mains-aegis.cdc.v1" | string;
  defmt?: {
    enabled: boolean;
    encoding: string;
    table_hash: string | null;
  };
};

export type NetworkSummary = {
  device_id: string;
  hostname: string;
  hostname_fqdn: string;
  state: NetworkState;
  ipv4: string | null;
  gateway: string | null;
  dns: string | null;
  is_static: boolean;
  last_error: "bad_static_config" | "connect_failed" | "dhcp_timeout" | "link_lost" | null;
  rssi_dbm: number | null;
};

export type WifiApplyNetwork = {
  state: NetworkState;
  ipv4: string | null;
  last_error: unknown | null;
};

export type Identity = {
  device_id: string;
  hostname: string;
  hostname_fqdn: string;
  short_id: string;
  role: "ups" | string;
  api_version: "v1" | string;
  firmware: FirmwareInfo;
  network: NetworkSummary;
  capabilities: {
    sse: boolean;
    mdns: boolean;
    dns_sd: boolean;
    write_controls: boolean;
  };
};

export type ChannelState = {
  state: "ok" | "off" | "hold" | "fault" | "unknown" | string;
  enabled: boolean;
  vbus_mv: number | null;
  iout_ma: number | null;
};

export type UpsStatus = {
  mode: "standby" | "assist" | "backup" | "off" | "fault" | string;
  input: {
    mains_present: boolean;
    input_vbus_mv: number | null;
    input_ibus_ma: number | null;
    vin_vbus_mv: number | null;
    vin_iin_ma: number | null;
  };
  output: {
    requested: "none" | "out_a" | "out_b" | "both" | string;
    active: "none" | "out_a" | "out_b" | "both" | string;
    recoverable: "none" | "out_a" | "out_b" | "both" | string;
    gate_reason: "none" | string | null;
    out_a: ChannelState;
    out_b: ChannelState;
  };
  charger: {
    state: "ok" | "charging" | "blocked" | "fault" | "unknown" | string;
    allow_charge: boolean;
    ichg_ma: number | null;
    ibat_ma: number | null;
    vbat_present: boolean;
  };
  battery: {
    state: "ok" | "warning" | "fault" | "missing" | string;
    pack_mv: number | null;
    current_ma: number | null;
    soc_pct: number | null;
    cell_mv?: Array<number | null>;
    cell_delta_mv?: number | null;
    balance_enabled?: boolean | null;
    balance_cfg_match?: boolean | null;
    balance_active?: boolean | null;
    balance_mask?: number | null;
    balance_cell?: number | null;
    balance_min_start_delta_mv?: number | null;
    no_battery: boolean;
    discharge_ready: boolean;
    charge_fet_on?: boolean | null;
    discharge_fet_on?: boolean | null;
    precharge_fet_on?: boolean | null;
    issue_detail: string | null;
    recovery_pending: boolean;
    last_result: string | null;
  };
  thermal: {
    tmp_a_state: "ok" | "warm" | "hot" | "fault" | string;
    tmp_a_c: number | null;
    tmp_b_state: "ok" | "warm" | "hot" | "fault" | string;
    tmp_b_c: number | null;
  };
  network: {
    state: NetworkState;
    ipv4: string | null;
    last_error: NetworkSummary["last_error"];
  };
};

export type DeviceTarget = {
  deviceId: string;
  baseUrl: string;
  alias: string;
  location: string;
  addedAt: string;
  transport?: "http" | "serial" | "devd";
  serialProtocol?: string;
  bridgeAuth?: boolean;
  mock?: boolean;
};

export type ConnectionState = "online" | "connecting" | "offline" | "error";

export type SerialLogEntry = {
  id: string;
  timestamp: string;
  level: "error" | "warn" | "info" | "debug" | "trace" | string;
  target: string;
  message: string;
};

export type DefmtDecodeResult = {
  level: string;
  target: string;
  message: string;
  index: number;
};

export type SerialTraceEntry = {
  id: string;
  timestamp: string;
  direction: "rx" | "tx" | "sse" | "error" | "info" | string;
  kind: "raw" | "frame" | "ignored" | "defmt" | "http" | string;
  frameType: string | null;
  requestId: string | null;
  target: string | null;
  summary: string;
  payload: string;
};

export type DeviceSettings = {
  wifi: {
    configured: boolean;
    ssid: string | null;
  };
  log_level: "error" | "warn" | "info" | "debug" | "trace" | string;
  manual_charge: {
    target: "pack_3v7" | "rsoc_80" | "full_100" | string;
    speed: "ma_100" | "ma_500" | "ma_1000" | string;
    timer_h: 1 | 2 | 6 | number;
  };
};

export type DeviceRecord = {
  target: DeviceTarget;
  identity: Identity | null;
  network: NetworkSummary | null;
  settings: DeviceSettings | null;
  status: UpsStatus | null;
  connectionState: ConnectionState;
  streamState: "idle" | "streaming" | "polling" | "error";
  error: ApiErrorEnvelope["error"] | null;
  lastUpdated: string | null;
  serial?: {
    connected: boolean;
    source: "web_serial" | "devd" | "mock";
    baseUrl?: string;
    leaseId?: string;
    leaseExpiresAt?: string;
    heartbeatIntervalMs?: number;
    leaseTtlMs?: number;
    protocol: string;
    status?: UpsStatus | null;
    logs: SerialLogEntry[];
    trace: SerialTraceEntry[];
  };
};

export type ProbeResult = {
  identity: Identity;
  network: NetworkSummary;
  status: UpsStatus;
  settings: DeviceSettings;
};

export type FirmwareArtifactFile = {
  kind: "elf" | "image" | "defmt_metadata";
  path: string;
  sha256: string;
  size: number;
  flash_address?: number;
};

export type FirmwareArtifact = {
  artifact_id: string;
  name: string;
  version: string;
  git_sha: string;
  build_id: string;
  target_chip: "esp32s3";
  profile: "debug" | "release" | "dev";
  features: string[];
  protocol: "mains-aegis.cdc.v1";
  defmt: {
    enabled: boolean;
    encoding: string;
    elf_sha256: string | null;
    metadata_sha256: string | null;
  };
  files: FirmwareArtifactFile[];
};

export type FirmwareCatalog = {
  schema_version: 1;
  artifacts: FirmwareArtifact[];
};

export type FirmwareArtifactMatch = {
  artifact: FirmwareArtifact;
  source: "github_release" | "bundled" | "bundled_overrides_release";
  catalog_url: string;
};

export type DevdDevice = {
  id: string;
  display_name: string;
  port_path: string | null;
  lan_address?: string | null;
  lan_conflict_addresses?: string[];
  transport: "native_serial" | "lan" | "mock";
  binding: unknown | null;
  connection: "disconnected" | "connected" | "busy" | "error";
  identity: Identity | null;
  status?: UpsStatus | null;
  selected_artifact_id: string | null;
  log_decode: {
    status: "verified" | "unverified" | string;
    reason: string | null;
    artifact_id: string | null;
  };
};

export type DevdScanTraceEntry = {
  id: string;
  timestamp: string;
  direction: string;
  kind: string;
  frameType?: string | null;
  requestId?: string | null;
  target?: string | null;
  summary: string;
  payload: string;
};

export type DevdWebLease = {
  lease_id: string;
  device_id: string;
  identity_device_id: string | null;
  expires_at: string;
  heartbeat_interval_ms: number;
  lease_ttl_ms: number;
  device: DevdDevice;
};
