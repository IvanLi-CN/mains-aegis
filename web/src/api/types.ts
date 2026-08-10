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
  hardware_capabilities?: {
    output_profile: "12v" | "19v" | string;
    rated_vout_mv: number;
  };
};

export type ChannelState = {
  state: "ok" | "off" | "hold" | "fault" | "unknown" | string;
  enabled: boolean;
  vbus_mv: number | null;
  iout_ma: number | null;
};

export type ChargePowerPath = "auto" | "dcin" | "usbc" | string;

export type ChargeCapabilities = {
  target_voltage_mv: number;
  normal_current_ma: number;
  dc_derated_current_ma: number;
  dcin_input_limit_ma: number;
  max_output_current_ma: number;
  usb_pd_high_power_min_voltage_mv: number;
  usb_pd_high_power_max_voltage_mv: number;
  usb_pd_high_power_min_power_mw: number;
  loop_start_max_power_without_confirm_w10: number;
  loop_stop_power_latched_w10: number;
  loop_telemetry_miss_limit: number;
  supported_power_paths: ChargePowerPath[];
  auto_path_priority: ChargePowerPath[];
};

export type ChargeControlSummary = {
  mode: "auto" | "manual" | string;
  manual_active: boolean;
  takeover: boolean;
  stop_inhibit: boolean;
  last_stop_reason: string | null;
  requested_power_path: ChargePowerPath;
  bound_power_path: ChargePowerPath | null;
  start_state: string;
  output_power_w10: number | null;
  power_telemetry_fresh: boolean;
};

export type ChargeControlBlock = {
  code:
    | "battery_full"
    | "target_reached"
    | "path_unavailable"
    | "path_not_qualified"
    | "no_input"
    | "temperature_blocked"
    | "battery_telemetry_unready"
    | "output_overload"
    | "charger_runtime_unavailable"
    | "loop_confirmation_required"
    | "blocked_unknown"
    | string;
  message: string;
};

export type ChargeControlDetailSummary = {
  mode: "auto" | "manual" | string;
  manual_active: boolean;
  takeover: boolean;
  stop_inhibit: boolean;
  last_stop_reason: string | null;
  remaining_minutes: number | null;
  loop_override_active: boolean;
};

export type ChargeControlReadiness = {
  state: "ready" | "blocked" | "confirm_required" | "running" | string;
  action: "start" | "stop" | "confirm_loop" | "none" | string;
  planned_path: {
    requested: ChargePowerPath;
    bound: ChargePowerPath | null;
    binding_reason: string | null;
  };
  block: ChargeControlBlock | null;
  loop_override: {
    required: boolean;
    active: boolean;
    allowed_guards: string[];
  };
};

export type ChargeControlTelemetry = {
  input_source: string;
  policy_target_ichg_ma: number | null;
  ibat_actual_ma: number | null;
  target_voltage_mv: number;
  iindpm_ma: number | null;
  vindpm_mv: number | null;
  output_power_w10: number | null;
  power_telemetry_fresh: boolean;
  input_limit_summary: string | null;
  output_limit_summary: string | null;
};

export type ChargeControlEvidenceEntry = {
  source: string;
  code: string;
  label: string;
  value: boolean | number | string | null;
};

export type ChargeControlDetail = {
  summary: ChargeControlDetailSummary;
  readiness: ChargeControlReadiness;
  telemetry: ChargeControlTelemetry;
  evidence: ChargeControlEvidenceEntry[];
};

export type UpsStatus = {
  mode: "standby" | "assist" | "backup" | "off" | "fault" | string;
  input: {
    source?: "dcin" | "usbc" | "auto" | "unknown" | string;
    mains_present: boolean;
    input_vbus_mv: number | null;
    input_ibus_ma: number | null;
    pre_tps_vin_mv?: number | null;
    vin_vbus_mv: number | null;
    input_gate_state?: "enabled" | "cutoff" | string | null;
    input_gate_reason?: "none" | "pre_tps_undervoltage" | string | null;
    input_power_good?: boolean | null;
    vin_iin_ma: number | null;
    tps_total_iout_ma?: number | null;
    tps_limit_threshold_ma?: number | null;
    pressure_state?: "inactive" | "headroom" | "watch" | "limited" | "cooldown" | string;
    pressure_score_pct?: number | null;
    pressure_reason?: string | null;
    vin_baseline_mv?: number | null;
    vin_drop_mv?: number | null;
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
    policy_target_ichg_ma?: number | null;
    limit_active?: boolean | null;
    limit_reason?: string | null;
    limit_detail?: string | null;
    limit_threshold_ma?: number | null;
    detail_status?: string | null;
  };
  charge_control?: ChargeControlSummary;
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
  temporary?: boolean;
  transport?: "http" | "serial" | "devd";
  preferredTransport?: "http" | "serial" | "devd";
  rememberedChannels?: {
    http?: {
      baseUrl: string;
      seenAt: string;
      source?: "manual" | "devd_discovery";
      mdnsHost?: string;
      fallbackBaseUrl?: string;
    };
    devd?: {
      baseUrl: string;
      devdDeviceId?: string | null;
      seenAt: string;
      transport?: "usb" | "lan" | "mock";
    };
    serial?: {
      seenAt: string;
    };
  };
  serialProtocol?: string;
  bridgeAuth?: boolean;
  mock?: boolean;
};

export type LanCompanionCandidate = {
  mdns_host: string;
  ip: string;
  port: number;
  detected_at: string;
  verified_at: string;
  source: string;
};

export type LanCompanionBinding = {
  mdns_host: string;
  ip: string;
  port: number;
  confirmed_at: string;
  last_verified_at: string;
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
    power_path?: ChargePowerPath;
  };
  charge_capabilities?: ChargeCapabilities;
  advanced_power: {
    standby_drop_mv: number;
    input_uvlo_cutoff_mv: number;
    input_uvlo_recover_mv: number;
    input_uvlo_required_samples: number;
    source_limited_enter_delta_ma: number;
  };
  advanced_power_capabilities: {
    rated_vout_mv: number;
    standby_drop_mv: {
      default: number;
      min: number;
      max: number;
      step: number;
    };
    input_uvlo_cutoff_mv: {
      default: number;
      min: number;
      max: number;
      step: number;
    };
    input_uvlo_recover_mv: {
      default: number;
      min: number;
      max: number;
      step: number;
    };
    input_uvlo_required_samples: {
      default: number;
      min: number;
      max: number;
      step: number;
    };
    source_limited_enter_delta_ma: {
      default: number;
      min: number;
      max: number;
      step: number;
    };
  };
};

export type AdvancedPowerSettings = DeviceSettings["advanced_power"];
export type AdvancedPowerCapabilities = DeviceSettings["advanced_power_capabilities"];

export type DeviceRecord = {
  target: DeviceTarget;
  identity: Identity | null;
  network: NetworkSummary | null;
  settings: DeviceSettings | null;
  status: UpsStatus | null;
  chargeControlDetail?: ChargeControlDetail | null;
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

export type ActiveAlert = {
  alert_id:
    | "mains_absent_dc"
    | "high_stress"
    | "battery_low_no_mains"
    | "battery_low_with_mains"
    | "shutdown_protection"
    | "io_over_voltage"
    | "io_over_current"
    | "module_fault"
    | "battery_protection";
  instance_id: number;
  severity: "warning" | "critical";
  sound_state: "audible" | "muted" | "system_silent" | "policy_silent";
  summary?: string;
};

export type ActiveAlertsSnapshot = { alerts: ActiveAlert[] };

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
  devd_manifest_path?: string;
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
  binding: {
    alias?: string | null;
    stable_id: string;
    port_path?: string | null;
    created_at: string;
    logical_device_id?: string | null;
    lan_companion?: LanCompanionBinding | null;
  } | null;
  companion_lan_candidate?: LanCompanionCandidate | null;
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

export type TpsEnableInterlock = {
  therm_kill_n_low: boolean;
  mcu_drive_low: boolean;
  tps_en_effective_inhibit: boolean;
  source: "mcu_i2c_retry_exhausted" | "external_or_unknown" | "released" | string;
  asserted_at_ms: number | null;
  last_release_at_ms: number | null;
  failure_channel: string | null;
  failure_stage: string | null;
  failure_code: string | null;
};

export type DevdDiagSnapshot = {
  schema_version: number;
  packages: {
    "mcu.runtime"?: {
      payload?: {
        tps_enable_interlock?: TpsEnableInterlock;
      };
    };
  };
  errors?: Record<string, unknown>;
};

export type TpsEnableReleaseResponse = {
  ok: true;
  accepted: true;
  result: "released" | "already_released" | string;
  mcu_drive_low: false;
  therm_kill_n_low: boolean;
  warning: "therm_kill_n_still_low" | null;
  output_gate_reason: string;
};

export type AppRuntimeMode =
  | "hosted"
  | "http_service_api_only"
  | "public_static"
  | "unknown";
