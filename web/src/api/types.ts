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
    no_battery: boolean;
    discharge_ready: boolean;
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
  mock?: boolean;
};

export type ConnectionState = "online" | "connecting" | "offline" | "error";

export type DeviceRecord = {
  target: DeviceTarget;
  identity: Identity | null;
  network: NetworkSummary | null;
  status: UpsStatus | null;
  connectionState: ConnectionState;
  streamState: "idle" | "streaming" | "polling" | "error";
  error: ApiErrorEnvelope["error"] | null;
  lastUpdated: string | null;
};

export type ProbeResult = {
  identity: Identity;
  network: NetworkSummary;
  status: UpsStatus;
};
