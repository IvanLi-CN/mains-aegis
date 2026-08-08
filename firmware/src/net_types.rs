use core::fmt::Write as _;

use heapless::String;

pub const API_VERSION: &str = "v1";
pub const SERVICE_ROLE: &str = "ups";
pub const SERVICE_TYPE: &str = "_mains-aegis-ups._tcp.local";
pub const HOSTNAME_PREFIX: &str = "mains-aegis-";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiConnectionState {
    Disabled,
    Idle,
    Connecting,
    Connected,
    Error,
}

impl WifiConnectionState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Idle => "idle",
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::Error => "error",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiErrorKind {
    BadStaticConfig,
    ConnectFailed,
    DhcpTimeout,
    LinkLost,
}

impl WifiErrorKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BadStaticConfig => "bad_static_config",
            Self::ConnectFailed => "connect_failed",
            Self::DhcpTimeout => "dhcp_timeout",
            Self::LinkLost => "link_lost",
        }
    }

    pub const fn ui_hint(self) -> &'static str {
        match self {
            Self::BadStaticConfig => "STATIC CFG",
            Self::ConnectFailed => "JOIN FAIL",
            Self::DhcpTimeout => "DHCP WAIT",
            Self::LinkLost => "LINK LOST",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WifiSnapshot {
    pub state: WifiConnectionState,
    pub ipv4: Option<[u8; 4]>,
    pub gateway: Option<[u8; 4]>,
    pub dns: Option<[u8; 4]>,
    pub is_static: bool,
    pub last_error: Option<WifiErrorKind>,
    pub rssi_dbm: Option<i8>,
    pub mac: Option<[u8; 6]>,
}

impl WifiSnapshot {
    pub const fn disabled() -> Self {
        Self {
            state: WifiConnectionState::Disabled,
            ipv4: None,
            gateway: None,
            dns: None,
            is_static: false,
            last_error: None,
            rssi_dbm: None,
            mac: None,
        }
    }

    pub const fn connecting() -> Self {
        Self {
            state: WifiConnectionState::Connecting,
            ..Self::disabled()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetworkUiSummary {
    pub state: WifiConnectionState,
    pub ipv4: Option<[u8; 4]>,
    pub last_error: Option<WifiErrorKind>,
}

impl NetworkUiSummary {
    pub const fn disabled() -> Self {
        Self {
            state: WifiConnectionState::Disabled,
            ipv4: None,
            last_error: None,
        }
    }

    pub fn from_wifi(snapshot: WifiSnapshot) -> Self {
        Self {
            state: snapshot.state,
            ipv4: snapshot.ipv4,
            last_error: snapshot.last_error,
        }
    }

    pub fn subtitle(self) -> String<32> {
        let mut out = String::<32>::new();
        match self.state {
            WifiConnectionState::Disabled | WifiConnectionState::Idle => {
                let _ = out.push_str("WIFI OFF");
            }
            WifiConnectionState::Connecting => {
                let _ = out.push_str("WIFI CONNECTING");
            }
            WifiConnectionState::Connected => {
                if let Some(ipv4) = self.ipv4 {
                    let _ = write!(out, "IP {}.{}.{}.{}", ipv4[0], ipv4[1], ipv4[2], ipv4[3]);
                } else {
                    let _ = out.push_str("WIFI READY");
                }
            }
            WifiConnectionState::Error => {
                let _ = out.push_str("WIFI RETRY");
                if let Some(kind) = self.last_error {
                    let _ = out.push(' ');
                    let _ = out.push_str(kind.ui_hint());
                }
            }
        }
        out
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrontPanelRuntimeSnapshot {
    pub init_state: &'static str,
    pub display_power_mode: &'static str,
    pub ui_variant: &'static str,
    pub frame_no: u32,
    pub ready: bool,
    pub needs_redraw: bool,
    pub attention_hold: bool,
}

impl FrontPanelRuntimeSnapshot {
    pub const fn unavailable() -> Self {
        Self {
            init_state: "unknown",
            display_power_mode: "unknown",
            ui_variant: "unknown",
            frame_no: 0,
            ready: false,
            needs_redraw: false,
            attention_hold: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WifiSettingsSnapshot {
    pub configured: bool,
    pub ssid: Option<String<32>>,
}

impl WifiSettingsSnapshot {
    pub fn unconfigured() -> Self {
        Self {
            configured: false,
            ssid: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ManualChargeSettingsSnapshot {
    pub target: &'static str,
    pub speed: &'static str,
    pub timer_h: u8,
    pub power_path: &'static str,
}

impl ManualChargeSettingsSnapshot {
    pub const fn defaults() -> Self {
        Self {
            target: "full_100",
            speed: "ma_500",
            timer_h: 2,
            power_path: "auto",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChargeCapabilitiesSnapshot {
    pub target_voltage_mv: u16,
    pub normal_current_ma: u16,
    pub dc_derated_current_ma: u16,
    pub dcin_input_limit_ma: u16,
    pub max_output_current_ma: u16,
    pub usb_pd_high_power_min_voltage_mv: u16,
    pub usb_pd_high_power_max_voltage_mv: u16,
    pub usb_pd_high_power_min_power_mw: u32,
    pub loop_start_max_power_without_confirm_w10: u32,
    pub loop_stop_power_latched_w10: u32,
    pub loop_telemetry_miss_limit: u8,
    pub supported_power_paths: [&'static str; 3],
    pub auto_path_priority: [&'static str; 3],
}

impl ChargeCapabilitiesSnapshot {
    pub const fn defaults() -> Self {
        Self {
            target_voltage_mv: 16_800,
            normal_current_ma: 500,
            dc_derated_current_ma: 100,
            dcin_input_limit_ma: 1_000,
            max_output_current_ma: 3_500,
            usb_pd_high_power_min_voltage_mv: 9_000,
            usb_pd_high_power_max_voltage_mv: 20_000,
            usb_pd_high_power_min_power_mw: 20_000,
            loop_start_max_power_without_confirm_w10: 20,
            loop_stop_power_latched_w10: 30,
            loop_telemetry_miss_limit: 2,
            supported_power_paths: ["auto", "dcin", "usbc"],
            auto_path_priority: ["usb_pd_high_power", "dcin", "usbc"],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChargeControlSnapshot {
    pub mode: &'static str,
    pub manual_active: bool,
    pub takeover: bool,
    pub stop_inhibit: bool,
    pub last_stop_reason: &'static str,
    pub remaining_minutes: Option<u16>,
    pub requested_power_path: &'static str,
    pub bound_power_path: Option<&'static str>,
    pub binding_reason: Option<&'static str>,
    pub start_state: &'static str,
    pub start_block_reason: Option<&'static str>,
    pub loop_confirmation_required: bool,
    pub loop_override_active: bool,
    pub output_power_w10: Option<u32>,
    pub power_telemetry_fresh: bool,
}

impl ChargeControlSnapshot {
    pub const fn empty() -> Self {
        Self {
            mode: "auto",
            manual_active: false,
            takeover: false,
            stop_inhibit: false,
            last_stop_reason: "none",
            remaining_minutes: None,
            requested_power_path: "auto",
            bound_power_path: None,
            binding_reason: None,
            start_state: "blocked",
            start_block_reason: Some("manual_charge_path_unavailable"),
            loop_confirmation_required: false,
            loop_override_active: false,
            output_power_w10: None,
            power_telemetry_fresh: false,
        }
    }
}

pub const CHARGE_CONTROL_EVIDENCE_CAP: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChargeControlPlannedPathSnapshot {
    pub requested: &'static str,
    pub bound: Option<&'static str>,
    pub binding_reason: Option<&'static str>,
}

impl ChargeControlPlannedPathSnapshot {
    pub const fn empty() -> Self {
        Self {
            requested: "auto",
            bound: None,
            binding_reason: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChargeControlBlockSnapshot {
    pub code: &'static str,
    pub message: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChargeControlLoopOverrideSnapshot {
    pub required: bool,
    pub active: bool,
    pub allowed_guards: [&'static str; 3],
}

impl ChargeControlLoopOverrideSnapshot {
    pub const fn defaults() -> Self {
        Self {
            required: false,
            active: false,
            allowed_guards: [
                "loop_start_low_output_gate",
                "loop_telemetry_miss_latch",
                "loop_stop_high_output_latch",
            ],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChargeControlReadinessSnapshot {
    pub state: &'static str,
    pub action: &'static str,
    pub planned_path: ChargeControlPlannedPathSnapshot,
    pub block: Option<ChargeControlBlockSnapshot>,
    pub loop_override: ChargeControlLoopOverrideSnapshot,
}

impl ChargeControlReadinessSnapshot {
    pub const fn empty() -> Self {
        Self {
            state: "blocked",
            action: "none",
            planned_path: ChargeControlPlannedPathSnapshot::empty(),
            block: None,
            loop_override: ChargeControlLoopOverrideSnapshot::defaults(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChargeControlTelemetrySnapshot {
    pub input_source: &'static str,
    pub policy_target_ichg_ma: Option<u16>,
    pub ibat_actual_ma: Option<i16>,
    pub target_voltage_mv: u16,
    pub iindpm_ma: Option<u16>,
    pub vindpm_mv: Option<u16>,
    pub output_power_w10: Option<u32>,
    pub power_telemetry_fresh: bool,
    pub input_limit_summary: Option<&'static str>,
    pub output_limit_summary: Option<&'static str>,
}

impl ChargeControlTelemetrySnapshot {
    pub const fn empty() -> Self {
        Self {
            input_source: "unknown",
            policy_target_ichg_ma: None,
            ibat_actual_ma: None,
            target_voltage_mv: 16_800,
            iindpm_ma: None,
            vindpm_mv: None,
            output_power_w10: None,
            power_telemetry_fresh: false,
            input_limit_summary: None,
            output_limit_summary: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChargeControlEvidenceValueSnapshot {
    None,
    Bool(bool),
    I32(i32),
    U16(u16),
    U32(u32),
    Str(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChargeControlEvidenceEntrySnapshot {
    pub source: &'static str,
    pub code: &'static str,
    pub label: &'static str,
    pub value: ChargeControlEvidenceValueSnapshot,
}

impl ChargeControlEvidenceEntrySnapshot {
    pub const fn empty() -> Self {
        Self {
            source: "none",
            code: "none",
            label: "none",
            value: ChargeControlEvidenceValueSnapshot::None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChargeControlDetailSnapshot {
    pub summary: ChargeControlSnapshot,
    pub readiness: ChargeControlReadinessSnapshot,
    pub telemetry: ChargeControlTelemetrySnapshot,
    pub evidence: [ChargeControlEvidenceEntrySnapshot; CHARGE_CONTROL_EVIDENCE_CAP],
    pub evidence_len: u8,
}

impl ChargeControlDetailSnapshot {
    pub const fn empty() -> Self {
        Self {
            summary: ChargeControlSnapshot::empty(),
            readiness: ChargeControlReadinessSnapshot::empty(),
            telemetry: ChargeControlTelemetrySnapshot::empty(),
            evidence: [ChargeControlEvidenceEntrySnapshot::empty(); CHARGE_CONTROL_EVIDENCE_CAP],
            evidence_len: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdvancedPowerU16CapabilityBounds {
    pub min: u16,
    pub max: u16,
    pub step: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdvancedPowerI16CapabilityBounds {
    pub min: i16,
    pub max: i16,
    pub step: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdvancedPowerU8CapabilityBounds {
    pub min: u8,
    pub max: u8,
    pub step: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdvancedPowerCapabilityBounds {
    pub standby_drop_mv: AdvancedPowerU16CapabilityBounds,
    pub input_uvlo_threshold_mv: AdvancedPowerU16CapabilityBounds,
    pub input_uvlo_required_samples: AdvancedPowerU8CapabilityBounds,
    pub source_limited_enter_delta_ma: AdvancedPowerI16CapabilityBounds,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeModeFixedPolicy {
    pub assist_low_drop_mv: u16,
    pub assist_enter_iout_ma: i32,
    pub assist_exit_iout_ma: i32,
    pub assist_required_samples: u8,
    pub assist_ramp_step_mv: u16,
    pub assist_ramp_interval_ms: u16,
    pub rated_enter_iout_ma: i32,
    pub rated_exit_iout_ma: i32,
    pub vin_drop_threshold_pct: u16,
    pub required_samples: u8,
    pub source_limited_vin_drop_pct: u16,
    pub source_limited_exit_iout_ma: i32,
    pub source_limited_required_samples: u8,
    pub source_limited_recover_margin_mv: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeModeProfileDefinition {
    pub id: &'static str,
    pub max_rated_vout_mv: u16,
    pub defaults: AdvancedPowerSettingsSnapshot,
    pub fixed_policy: RuntimeModeFixedPolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdvancedPowerSettingsSnapshot {
    pub standby_drop_mv: u16,
    pub input_uvlo_cutoff_mv: u16,
    pub input_uvlo_recover_mv: u16,
    pub input_uvlo_required_samples: u8,
    pub source_limited_enter_delta_ma: i16,
}

include!(concat!(
    env!("OUT_DIR"),
    "/runtime_mode_profiles_generated.rs"
));

pub fn runtime_mode_profile_for_rated_vout(rated_vout_mv: u16) -> RuntimeModeProfileDefinition {
    RUNTIME_MODE_PROFILES
        .iter()
        .copied()
        .find(|profile| rated_vout_mv <= profile.max_rated_vout_mv)
        .unwrap_or_else(|| *RUNTIME_MODE_PROFILES.last().expect("runtime mode profiles"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeModeTuningSnapshot {
    pub standby_vout_mv: u16,
    pub input_uvlo_cutoff_mv: u16,
    pub input_uvlo_recover_mv: u16,
    pub input_uvlo_required_samples: u8,
    pub source_limited_enter_iout_ma: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeModePolicySnapshot {
    pub rated_vout_mv: u16,
    pub tuning: RuntimeModeTuningSnapshot,
    pub fixed: RuntimeModeFixedPolicy,
}

impl RuntimeModePolicySnapshot {
    pub const fn standby_vout_mv(self) -> u16 {
        self.tuning.standby_vout_mv
    }

    pub const fn assist_low_vout_mv(self) -> u16 {
        self.rated_vout_mv
            .saturating_sub(self.fixed.assist_low_drop_mv)
    }
}

impl AdvancedPowerSettingsSnapshot {
    pub fn defaults() -> Self {
        Self::defaults_for_rated_vout(12_000)
    }

    pub fn defaults_for_rated_vout(rated_vout_mv: u16) -> Self {
        runtime_mode_profile_for_rated_vout(rated_vout_mv).defaults
    }

    pub fn resolve_runtime_policy(
        self,
        rated_vout_mv: u16,
    ) -> Result<RuntimeModePolicySnapshot, AdvancedPowerValidationError> {
        validate_advanced_power_settings(self)?;
        let profile = runtime_mode_profile_for_rated_vout(rated_vout_mv);
        if self.standby_drop_mv < profile.fixed_policy.assist_low_drop_mv {
            return Err(AdvancedPowerValidationError::VoltageOrderInvalid);
        }
        let source_limited_enter_iout_ma = profile.fixed_policy.rated_enter_iout_ma
            + i32::from(self.source_limited_enter_delta_ma);
        if profile.fixed_policy.source_limited_exit_iout_ma > source_limited_enter_iout_ma {
            return Err(AdvancedPowerValidationError::SourceLimitedCurrentOrderInvalid);
        }
        Ok(RuntimeModePolicySnapshot {
            rated_vout_mv,
            tuning: RuntimeModeTuningSnapshot {
                standby_vout_mv: rated_vout_mv.saturating_sub(self.standby_drop_mv),
                input_uvlo_cutoff_mv: self.input_uvlo_cutoff_mv,
                input_uvlo_recover_mv: self.input_uvlo_recover_mv,
                input_uvlo_required_samples: self.input_uvlo_required_samples,
                source_limited_enter_iout_ma,
            },
            fixed: profile.fixed_policy,
        })
    }

    pub fn capabilities_for_rated_vout(rated_vout_mv: u16) -> AdvancedPowerCapabilitiesSnapshot {
        AdvancedPowerCapabilitiesSnapshot::for_rated_vout(rated_vout_mv)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdvancedPowerU16CapabilitySnapshot {
    pub default: u16,
    pub min: u16,
    pub max: u16,
    pub step: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdvancedPowerI16CapabilitySnapshot {
    pub default: i16,
    pub min: i16,
    pub max: i16,
    pub step: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdvancedPowerU8CapabilitySnapshot {
    pub default: u8,
    pub min: u8,
    pub max: u8,
    pub step: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdvancedPowerCapabilitiesSnapshot {
    pub rated_vout_mv: u16,
    pub standby_drop_mv: AdvancedPowerU16CapabilitySnapshot,
    pub input_uvlo_cutoff_mv: AdvancedPowerU16CapabilitySnapshot,
    pub input_uvlo_recover_mv: AdvancedPowerU16CapabilitySnapshot,
    pub input_uvlo_required_samples: AdvancedPowerU8CapabilitySnapshot,
    pub source_limited_enter_delta_ma: AdvancedPowerI16CapabilitySnapshot,
}

impl AdvancedPowerCapabilitiesSnapshot {
    pub fn for_rated_vout(rated_vout_mv: u16) -> Self {
        let profile = runtime_mode_profile_for_rated_vout(rated_vout_mv);
        let bounds = ADVANCED_POWER_CAPABILITY_BOUNDS;
        Self {
            rated_vout_mv,
            standby_drop_mv: AdvancedPowerU16CapabilitySnapshot {
                default: profile.defaults.standby_drop_mv,
                min: bounds.standby_drop_mv.min,
                max: bounds.standby_drop_mv.max,
                step: bounds.standby_drop_mv.step,
            },
            input_uvlo_cutoff_mv: AdvancedPowerU16CapabilitySnapshot {
                default: profile.defaults.input_uvlo_cutoff_mv,
                min: bounds.input_uvlo_threshold_mv.min,
                max: bounds.input_uvlo_threshold_mv.max,
                step: bounds.input_uvlo_threshold_mv.step,
            },
            input_uvlo_recover_mv: AdvancedPowerU16CapabilitySnapshot {
                default: profile.defaults.input_uvlo_recover_mv,
                min: bounds.input_uvlo_threshold_mv.min,
                max: bounds.input_uvlo_threshold_mv.max,
                step: bounds.input_uvlo_threshold_mv.step,
            },
            input_uvlo_required_samples: AdvancedPowerU8CapabilitySnapshot {
                default: profile.defaults.input_uvlo_required_samples,
                min: bounds.input_uvlo_required_samples.min,
                max: bounds.input_uvlo_required_samples.max,
                step: bounds.input_uvlo_required_samples.step,
            },
            source_limited_enter_delta_ma: AdvancedPowerI16CapabilitySnapshot {
                default: profile.defaults.source_limited_enter_delta_ma,
                min: bounds.source_limited_enter_delta_ma.min,
                max: bounds.source_limited_enter_delta_ma.max,
                step: bounds.source_limited_enter_delta_ma.step,
            },
        }
    }
}

#[derive(defmt::Format, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdvancedPowerValidationError {
    StandbyDropOutOfRange,
    InputUvloCutoffOutOfRange,
    InputUvloRecoverOutOfRange,
    InputUvloRequiredSamplesOutOfRange,
    SourceLimitedEnterDeltaOutOfRange,
    VoltageOrderInvalid,
    InputUvloThresholdOrderInvalid,
    SourceLimitedCurrentOrderInvalid,
}

impl AdvancedPowerValidationError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::StandbyDropOutOfRange => "advanced_power_standby_drop_out_of_range",
            Self::InputUvloCutoffOutOfRange => "advanced_power_input_uvlo_cutoff_out_of_range",
            Self::InputUvloRecoverOutOfRange => "advanced_power_input_uvlo_recover_out_of_range",
            Self::InputUvloRequiredSamplesOutOfRange => {
                "advanced_power_input_uvlo_required_samples_out_of_range"
            }
            Self::SourceLimitedEnterDeltaOutOfRange => {
                "advanced_power_source_limited_enter_delta_out_of_range"
            }
            Self::VoltageOrderInvalid => "advanced_power_voltage_order_invalid",
            Self::InputUvloThresholdOrderInvalid => {
                "advanced_power_input_uvlo_threshold_order_invalid"
            }
            Self::SourceLimitedCurrentOrderInvalid => {
                "advanced_power_source_limited_current_order_invalid"
            }
        }
    }

    pub const fn message(self) -> &'static str {
        match self {
            Self::StandbyDropOutOfRange => {
                "standby_drop_mv must be within 0..3000 mV in 20 mV steps"
            }
            Self::InputUvloCutoffOutOfRange => {
                "input_uvlo_cutoff_mv must be within 5000..20000 mV in 20 mV steps"
            }
            Self::InputUvloRecoverOutOfRange => {
                "input_uvlo_recover_mv must be within 5000..20000 mV in 20 mV steps"
            }
            Self::InputUvloRequiredSamplesOutOfRange => {
                "input_uvlo_required_samples must be within 1..5 in step 1"
            }
            Self::SourceLimitedEnterDeltaOutOfRange => {
                "source_limited_enter_delta_ma must be within -100..3000 mA in 50 mA steps"
            }
            Self::VoltageOrderInvalid => {
                "standby_drop_mv must be greater than or equal to assist_low_drop_mv"
            }
            Self::InputUvloThresholdOrderInvalid => {
                "input_uvlo_recover_mv must be greater than or equal to input_uvlo_cutoff_mv"
            }
            Self::SourceLimitedCurrentOrderInvalid => {
                "expanded source_limited_exit_threshold_ma must not exceed source_limited_enter_threshold_ma"
            }
        }
    }
}

pub fn validate_advanced_power_settings(
    settings: AdvancedPowerSettingsSnapshot,
) -> Result<(), AdvancedPowerValidationError> {
    let bounds = ADVANCED_POWER_CAPABILITY_BOUNDS;
    if !value_in_u16_range(
        settings.standby_drop_mv,
        bounds.standby_drop_mv.min,
        bounds.standby_drop_mv.max,
        bounds.standby_drop_mv.step,
    ) {
        return Err(AdvancedPowerValidationError::StandbyDropOutOfRange);
    }
    if !value_in_u16_range(
        settings.input_uvlo_cutoff_mv,
        bounds.input_uvlo_threshold_mv.min,
        bounds.input_uvlo_threshold_mv.max,
        bounds.input_uvlo_threshold_mv.step,
    ) {
        return Err(AdvancedPowerValidationError::InputUvloCutoffOutOfRange);
    }
    if !value_in_u16_range(
        settings.input_uvlo_recover_mv,
        bounds.input_uvlo_threshold_mv.min,
        bounds.input_uvlo_threshold_mv.max,
        bounds.input_uvlo_threshold_mv.step,
    ) {
        return Err(AdvancedPowerValidationError::InputUvloRecoverOutOfRange);
    }
    if !value_in_u8_range(
        settings.input_uvlo_required_samples,
        bounds.input_uvlo_required_samples.min,
        bounds.input_uvlo_required_samples.max,
        bounds.input_uvlo_required_samples.step,
    ) {
        return Err(AdvancedPowerValidationError::InputUvloRequiredSamplesOutOfRange);
    }
    if !value_in_i16_range(
        settings.source_limited_enter_delta_ma,
        bounds.source_limited_enter_delta_ma.min,
        bounds.source_limited_enter_delta_ma.max,
        bounds.source_limited_enter_delta_ma.step,
    ) {
        return Err(AdvancedPowerValidationError::SourceLimitedEnterDeltaOutOfRange);
    }
    if settings.input_uvlo_recover_mv < settings.input_uvlo_cutoff_mv {
        return Err(AdvancedPowerValidationError::InputUvloThresholdOrderInvalid);
    }
    Ok(())
}

const fn value_in_u16_range(value: u16, min: u16, max: u16, step: u16) -> bool {
    value >= min && value <= max && value.wrapping_sub(min) % step == 0
}

const fn value_in_u8_range(value: u8, min: u8, max: u8, step: u8) -> bool {
    value >= min && value <= max && value.wrapping_sub(min) % step == 0
}

fn value_in_i16_range(value: i16, min: i16, max: i16, step: i16) -> bool {
    value >= min && value <= max && i32::from(value - min).rem_euclid(i32::from(step)) == 0
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceSettingsSnapshot {
    pub wifi: WifiSettingsSnapshot,
    pub log_level: &'static str,
    pub manual_charge: ManualChargeSettingsSnapshot,
    pub charge_capabilities: ChargeCapabilitiesSnapshot,
    pub advanced_power: AdvancedPowerSettingsSnapshot,
    pub advanced_power_capabilities: AdvancedPowerCapabilitiesSnapshot,
}

impl DeviceSettingsSnapshot {
    pub fn defaults() -> Self {
        Self::defaults_for_rated_vout(12_000)
    }

    pub fn defaults_for_rated_vout(rated_vout_mv: u16) -> Self {
        Self {
            wifi: WifiSettingsSnapshot::unconfigured(),
            log_level: "info",
            manual_charge: ManualChargeSettingsSnapshot::defaults(),
            charge_capabilities: ChargeCapabilitiesSnapshot::defaults(),
            advanced_power: AdvancedPowerSettingsSnapshot::defaults_for_rated_vout(rated_vout_mv),
            advanced_power_capabilities: AdvancedPowerCapabilitiesSnapshot::for_rated_vout(
                rated_vout_mv,
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UpsStatusSnapshot {
    pub mode: &'static str,
    pub requested_outputs: &'static str,
    pub active_outputs: &'static str,
    pub recoverable_outputs: &'static str,
    pub output_gate_reason: &'static str,
    pub input_source: &'static str,
    pub input_vbus_mv: Option<u16>,
    pub input_ibus_ma: Option<i32>,
    pub mains_present: Option<bool>,
    pub pre_tps_vin_mv: Option<u16>,
    pub vin_vbus_mv: Option<u16>,
    pub input_gate_state: Option<&'static str>,
    pub input_gate_reason: Option<&'static str>,
    pub input_power_good: Option<bool>,
    pub vin_iin_ma: Option<i32>,
    pub tps_total_iout_ma: Option<i32>,
    pub tps_limit_threshold_ma: Option<i32>,
    pub input_pressure_state: &'static str,
    pub input_pressure_score_pct: Option<u8>,
    pub input_pressure_reason: Option<&'static str>,
    pub input_vin_baseline_mv: Option<u16>,
    pub input_vin_drop_mv: Option<u16>,
    pub assist_power_stage: Option<&'static str>,
    pub assist_target_vout_mv: Option<u16>,
    pub backup_reason: Option<&'static str>,
    pub charger_state: &'static str,
    pub charger_allow_charge: Option<bool>,
    pub charger_ichg_ma: Option<u16>,
    pub charger_ibat_ma: Option<i16>,
    pub charger_vbat_present: Option<bool>,
    pub charger_policy_target_ichg_ma: Option<u16>,
    pub charger_limit_active: Option<bool>,
    pub charger_limit_reason: Option<&'static str>,
    pub charger_limit_detail: Option<&'static str>,
    pub charger_limit_threshold_ma: Option<i32>,
    pub charger_detail_status: Option<&'static str>,
    pub battery_state: &'static str,
    pub battery_pack_mv: Option<u16>,
    pub battery_current_ma: Option<i16>,
    pub battery_soc_pct: Option<u16>,
    pub battery_cell_mv: [Option<u16>; 4],
    pub battery_cell_delta_mv: Option<u16>,
    pub battery_balance_enabled: Option<bool>,
    pub battery_balance_cfg_match: Option<bool>,
    pub battery_balance_active: Option<bool>,
    pub battery_balance_mask: Option<u8>,
    pub battery_balance_cell: Option<u8>,
    pub battery_balance_min_start_delta_mv: Option<u8>,
    pub battery_no_battery: Option<bool>,
    pub battery_discharge_ready: Option<bool>,
    pub battery_charge_fet_on: Option<bool>,
    pub battery_discharge_fet_on: Option<bool>,
    pub battery_precharge_fet_on: Option<bool>,
    pub battery_issue_detail: Option<&'static str>,
    pub battery_recovery_pending: bool,
    pub battery_last_result: Option<&'static str>,
    pub out_a_state: &'static str,
    pub out_a_enabled: Option<bool>,
    pub out_a_vbus_mv: Option<u16>,
    pub out_a_iout_ma: Option<i32>,
    pub out_b_state: &'static str,
    pub out_b_enabled: Option<bool>,
    pub out_b_vbus_mv: Option<u16>,
    pub out_b_iout_ma: Option<i32>,
    pub tmp_a_state: &'static str,
    pub tmp_a_c: Option<i16>,
    pub tmp_b_state: &'static str,
    pub tmp_b_c: Option<i16>,
    pub charge_control: ChargeControlSnapshot,
    pub front_panel: FrontPanelRuntimeSnapshot,
    pub network: NetworkUiSummary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DerivedPowerSnapshot {
    pub input: DerivedPowerInputSnapshot,
    pub charger: DerivedPowerChargerSnapshot,
    pub policy: DerivedPowerPolicySnapshot,
    pub bms: DerivedPowerBmsSnapshot,
    pub hardware: HardwareDiagSnapshot,
}

impl DerivedPowerSnapshot {
    pub const fn empty() -> Self {
        Self {
            input: DerivedPowerInputSnapshot::empty(),
            charger: DerivedPowerChargerSnapshot::empty(),
            policy: DerivedPowerPolicySnapshot::empty(),
            bms: DerivedPowerBmsSnapshot::empty(),
            hardware: HardwareDiagSnapshot::empty(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiagReadU8 {
    pub raw: Option<u8>,
    pub error: Option<&'static str>,
}

impl DiagReadU8 {
    pub const fn empty() -> Self {
        Self {
            raw: None,
            error: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiagReadU16 {
    pub raw: Option<u16>,
    pub error: Option<&'static str>,
}

impl DiagReadU16 {
    pub const fn empty() -> Self {
        Self {
            raw: None,
            error: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tps55288DiagSnapshot {
    pub captured_at_ms: u64,
    pub duration_ms: u16,
    pub address: u8,
    pub vref: DiagReadU16,
    pub mode: DiagReadU8,
    pub status: DiagReadU8,
    pub vout_sr: DiagReadU8,
    pub vout_fs: DiagReadU8,
    pub cdc: DiagReadU8,
    pub iout_limit: DiagReadU8,
    pub vbus_mv: Option<u16>,
    pub iout_ma: Option<i32>,
    pub temp_c_x16: Option<i16>,
}

impl Tps55288DiagSnapshot {
    pub const fn empty(address: u8) -> Self {
        Self {
            captured_at_ms: 0,
            duration_ms: 0,
            address,
            vref: DiagReadU16::empty(),
            mode: DiagReadU8::empty(),
            status: DiagReadU8::empty(),
            vout_sr: DiagReadU8::empty(),
            vout_fs: DiagReadU8::empty(),
            cdc: DiagReadU8::empty(),
            iout_limit: DiagReadU8::empty(),
            vbus_mv: None,
            iout_ma: None,
            temp_c_x16: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ina3221DiagSnapshot {
    pub captured_at_ms: u64,
    pub duration_ms: u16,
    pub config: DiagReadU16,
    pub manufacturer_id: DiagReadU16,
    pub die_id: DiagReadU16,
    pub bus_mv: [Option<i32>; 3],
    pub shunt_uv: [Option<i32>; 3],
    pub channel_errors: [Option<&'static str>; 3],
    pub irq_mask_enable_raw: Option<u16>,
    pub irq_latched_bits: u16,
    pub irq_pv_count: u32,
    pub irq_warning_count: u32,
    pub irq_critical_count: u32,
    pub irq_last_at_ms: Option<u64>,
    pub irq_read_error: Option<&'static str>,
}

impl Ina3221DiagSnapshot {
    pub const fn empty() -> Self {
        Self {
            captured_at_ms: 0,
            duration_ms: 0,
            config: DiagReadU16::empty(),
            manufacturer_id: DiagReadU16::empty(),
            die_id: DiagReadU16::empty(),
            bus_mv: [None; 3],
            shunt_uv: [None; 3],
            channel_errors: [None; 3],
            irq_mask_enable_raw: None,
            irq_latched_bits: 0,
            irq_pv_count: 0,
            irq_warning_count: 0,
            irq_critical_count: 0,
            irq_last_at_ms: None,
            irq_read_error: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tmp112DiagSnapshot {
    pub captured_at_ms: u64,
    pub duration_ms: u16,
    pub address: u8,
    pub temperature: DiagReadU16,
    pub config: DiagReadU16,
    pub tlow: DiagReadU16,
    pub thigh: DiagReadU16,
}

impl Tmp112DiagSnapshot {
    pub const fn empty(address: u8) -> Self {
        Self {
            captured_at_ms: 0,
            duration_ms: 0,
            address,
            temperature: DiagReadU16::empty(),
            config: DiagReadU16::empty(),
            tlow: DiagReadU16::empty(),
            thigh: DiagReadU16::empty(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HardwareDiagSnapshot {
    pub tps_a: Tps55288DiagSnapshot,
    pub tps_b: Tps55288DiagSnapshot,
    pub ina3221: Ina3221DiagSnapshot,
    pub tmp_a: Tmp112DiagSnapshot,
    pub tmp_b: Tmp112DiagSnapshot,
    pub last_fresh_capture_ms: Option<u64>,
    pub capture_error: Option<&'static str>,
    pub retry_after_ms: Option<u16>,
}

impl HardwareDiagSnapshot {
    pub const fn empty() -> Self {
        Self {
            tps_a: Tps55288DiagSnapshot::empty(0x74),
            tps_b: Tps55288DiagSnapshot::empty(0x75),
            ina3221: Ina3221DiagSnapshot::empty(),
            tmp_a: Tmp112DiagSnapshot::empty(0x48),
            tmp_b: Tmp112DiagSnapshot::empty(0x49),
            last_fresh_capture_ms: None,
            capture_error: None,
            retry_after_ms: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DerivedPowerInputSnapshot {
    pub source: &'static str,
    pub mains_present: Option<bool>,
    pub input_vbus_mv: Option<u16>,
    pub input_ibus_ma: Option<i32>,
    pub pre_tps_vin_mv: Option<u16>,
    pub vin_vbus_mv: Option<u16>,
    pub input_gate_state: Option<&'static str>,
    pub input_gate_reason: Option<&'static str>,
    pub input_power_good: Option<bool>,
    pub vin_iin_ma: Option<i32>,
    pub tps_total_iout_ma: Option<i32>,
    pub tps_limit_threshold_ma: Option<i32>,
    pub pressure_state: &'static str,
    pub pressure_score_pct: Option<u8>,
    pub pressure_reason: Option<&'static str>,
    pub vin_baseline_mv: Option<u16>,
    pub vin_drop_mv: Option<u16>,
    pub assist_power_stage: Option<&'static str>,
    pub assist_target_vout_mv: Option<u16>,
    pub backup_reason: Option<&'static str>,
    pub usb_pd_attached: bool,
    pub usb_pd_charge_ready: bool,
    pub usb_pd_vbus_present: Option<bool>,
    pub usb_pd_unsafe_source_latched: bool,
    pub usb_pd_contract_kind: Option<&'static str>,
    pub usb_pd_contract_mv: Option<u16>,
    pub usb_pd_contract_ma: Option<u16>,
    pub usb_pd_vac1_mv: Option<u16>,
    pub usb_pd_vsys_mv: Option<u16>,
}

impl DerivedPowerInputSnapshot {
    pub const fn empty() -> Self {
        Self {
            source: "unknown",
            mains_present: None,
            input_vbus_mv: None,
            input_ibus_ma: None,
            pre_tps_vin_mv: None,
            vin_vbus_mv: None,
            input_gate_state: None,
            input_gate_reason: None,
            input_power_good: None,
            vin_iin_ma: None,
            tps_total_iout_ma: None,
            tps_limit_threshold_ma: None,
            pressure_state: "inactive",
            pressure_score_pct: None,
            pressure_reason: None,
            vin_baseline_mv: None,
            vin_drop_mv: None,
            assist_power_stage: None,
            assist_target_vout_mv: None,
            backup_reason: None,
            usb_pd_attached: false,
            usb_pd_charge_ready: false,
            usb_pd_vbus_present: None,
            usb_pd_unsafe_source_latched: false,
            usb_pd_contract_kind: None,
            usb_pd_contract_mv: None,
            usb_pd_contract_ma: None,
            usb_pd_vac1_mv: None,
            usb_pd_vsys_mv: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DerivedPowerChargerSnapshot {
    pub poll_valid: bool,
    pub enabled: bool,
    pub ce_low: bool,
    pub ilim_hiz_brk_low: bool,
    pub allow_charge: bool,
    pub normal_allow_charge: bool,
    pub force_allow_charge: bool,
    pub can_enable: bool,
    pub usb_pd_charge_gate_ready: bool,
    pub input_present: bool,
    pub vbus_present: bool,
    pub ac1_present: bool,
    pub ac2_present: bool,
    pub pg: bool,
    pub vbat_present: bool,
    pub adc_enabled: bool,
    pub adc_done: bool,
    pub adc_ready: bool,
    pub ibus_adc_ma: Option<i16>,
    pub ibat_adc_ma: Option<i16>,
    pub vbus_adc_mv: Option<u16>,
    pub vbat_adc_mv: Option<u16>,
    pub vsys_adc_mv: Option<u16>,
    pub vac1_adc_mv: Option<u16>,
    pub vac2_adc_mv: Option<u16>,
    pub vreg_mv: Option<u16>,
    pub ichg_ma: Option<u16>,
    pub vindpm_mv: Option<u16>,
    pub iindpm_ma: Option<u16>,
    pub vbat_lowv_pct_x10: Option<u16>,
    pub iprechg_ma: Option<u16>,
    pub iterm_ma: Option<u16>,
    pub chg_stat: &'static str,
    pub vbus_stat: &'static str,
    pub ico_stat: &'static str,
    pub treg: bool,
    pub dpdm: bool,
    pub wd: bool,
    pub poorsrc: bool,
    pub vindpm: bool,
    pub iindpm: bool,
    pub ts_cold: bool,
    pub ts_hot: bool,
    pub st0: Option<u8>,
    pub st1: Option<u8>,
    pub st2: Option<u8>,
    pub st3: Option<u8>,
    pub st4: Option<u8>,
    pub fault0: Option<u8>,
    pub fault1: Option<u8>,
    pub ctrl0: Option<u8>,
    pub ctrl2: Option<u8>,
    pub ctrl3: Option<u8>,
    pub ctrl4: Option<u8>,
    pub ctrl5: Option<u8>,
    pub sfet_present: Option<bool>,
    pub sdrv_ctrl: Option<u8>,
    pub acdrv_path: &'static str,
    pub term_ctrl: Option<u16>,
}

impl DerivedPowerChargerSnapshot {
    pub const fn empty() -> Self {
        Self {
            poll_valid: false,
            enabled: false,
            ce_low: false,
            ilim_hiz_brk_low: false,
            allow_charge: false,
            normal_allow_charge: false,
            force_allow_charge: false,
            can_enable: false,
            usb_pd_charge_gate_ready: false,
            input_present: false,
            vbus_present: false,
            ac1_present: false,
            ac2_present: false,
            pg: false,
            vbat_present: false,
            adc_enabled: false,
            adc_done: false,
            adc_ready: false,
            ibus_adc_ma: None,
            ibat_adc_ma: None,
            vbus_adc_mv: None,
            vbat_adc_mv: None,
            vsys_adc_mv: None,
            vac1_adc_mv: None,
            vac2_adc_mv: None,
            vreg_mv: None,
            ichg_ma: None,
            vindpm_mv: None,
            iindpm_ma: None,
            vbat_lowv_pct_x10: None,
            iprechg_ma: None,
            iterm_ma: None,
            chg_stat: "unknown",
            vbus_stat: "unknown",
            ico_stat: "unknown",
            treg: false,
            dpdm: false,
            wd: false,
            poorsrc: false,
            vindpm: false,
            iindpm: false,
            ts_cold: false,
            ts_hot: false,
            st0: None,
            st1: None,
            st2: None,
            st3: None,
            st4: None,
            fault0: None,
            fault1: None,
            ctrl0: None,
            ctrl2: None,
            ctrl3: None,
            ctrl4: None,
            ctrl5: None,
            sfet_present: None,
            sdrv_ctrl: None,
            acdrv_path: "unknown",
            term_ctrl: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DerivedPowerPolicySnapshot {
    pub state: Option<&'static str>,
    pub status: &'static str,
    pub notice: &'static str,
    pub input_source: &'static str,
    pub start_reason: Option<&'static str>,
    pub full_reason: Option<&'static str>,
    pub output_block_reason: Option<&'static str>,
    pub recovery_stage: Option<&'static str>,
    pub target_ichg_ma: Option<u16>,
    pub adaptive_cap_ichg_ma: Option<u16>,
    pub effective_target_ichg_ma: Option<u16>,
    pub limit_active: bool,
    pub limit_reason: Option<&'static str>,
    pub limit_detail: Option<&'static str>,
    pub detail_status: Option<&'static str>,
    pub pressure_state: &'static str,
    pub pressure_reason: Option<&'static str>,
    pub pressure_score_pct: Option<u8>,
    pub vin_baseline_mv: Option<u16>,
    pub vin_drop_mv: Option<u16>,
    pub tps_total_iout_ma: Option<i32>,
    pub tps_limit_threshold_ma: Option<i32>,
    pub output_power_w10: Option<u32>,
    pub charge_latched: bool,
    pub full_latched: bool,
    pub dc_derated: bool,
    pub output_blocked: bool,
    pub manual_active: bool,
    pub manual_stop_inhibit: bool,
}

impl DerivedPowerPolicySnapshot {
    pub const fn empty() -> Self {
        Self {
            state: None,
            status: "unknown",
            notice: "unavailable",
            input_source: "unknown",
            start_reason: None,
            full_reason: None,
            output_block_reason: None,
            recovery_stage: None,
            target_ichg_ma: None,
            adaptive_cap_ichg_ma: None,
            effective_target_ichg_ma: None,
            limit_active: false,
            limit_reason: None,
            limit_detail: None,
            detail_status: None,
            pressure_state: "inactive",
            pressure_reason: None,
            pressure_score_pct: None,
            vin_baseline_mv: None,
            vin_drop_mv: None,
            tps_total_iout_ma: None,
            tps_limit_threshold_ma: None,
            output_power_w10: None,
            charge_latched: false,
            full_latched: false,
            dc_derated: false,
            output_blocked: false,
            manual_active: false,
            manual_stop_inhibit: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DerivedPowerBmsSnapshot {
    pub addr: Option<u8>,
    pub state: &'static str,
    pub pack_mv: Option<u16>,
    pub current_ma: Option<i16>,
    pub soc_pct: Option<u16>,
    pub cell_min_mv: Option<u16>,
    pub cell_max_mv: Option<u16>,
    pub no_battery: Option<bool>,
    pub discharge_ready: Option<bool>,
    pub charge_ready: Option<bool>,
    pub full: Option<bool>,
    pub issue_detail: Option<&'static str>,
    pub rca_alarm: Option<bool>,
    pub safety_alert: Option<u32>,
    pub safety_status: Option<u32>,
    pub pf_status: Option<u32>,
    pub manufacturing_status: Option<u32>,
    pub gauging_status: Option<u32>,
    pub charging_status: Option<u32>,
    pub op_status: Option<u32>,
    pub op_status_raw_len: Option<u8>,
    pub op_status_raw_bytes: Option<[u8; 4]>,
    pub afe_fet_status: Option<u8>,
    pub afe_fet_control: Option<u8>,
    pub afe_latch_status: Option<u8>,
    pub afe_cell_balance_status: Option<u8>,
    pub afe_chg_fet: Option<bool>,
    pub afe_dsg_fet: Option<bool>,
    pub emshut: Option<bool>,
    pub pres: Option<bool>,
    pub xchg: Option<bool>,
    pub xdsg: Option<bool>,
    pub op_chg_fet: Option<bool>,
    pub op_dsg_fet: Option<bool>,
    pub op_pchg_fet: Option<bool>,
    pub chg_fet: Option<bool>,
    pub dsg_fet: Option<bool>,
    pub pchg_fet: Option<bool>,
    pub discharge_path_contradiction: Option<bool>,
    pub discharge_path_contradiction_reason: Option<&'static str>,
    pub cuv: Option<bool>,
    pub cuvc: Option<bool>,
    pub cov: Option<bool>,
    pub occ1: Option<bool>,
    pub occ2: Option<bool>,
    pub oc: Option<bool>,
    pub safety_alert_oc: Option<bool>,
    pub cuv_recovery_mv: Option<u16>,
    pub cuv_recov_chg: Option<bool>,
    pub fet_en: Option<bool>,
    pub chg_en: Option<bool>,
    pub dsg_en: Option<bool>,
    pub charging_inhibit: Option<bool>,
    pub charging_suspend: Option<bool>,
    pub charging_hv: Option<bool>,
    pub current_at_eoc_ma: Option<u16>,
    pub da_configuration: Option<u16>,
    pub power_config: Option<u16>,
    pub emshut_en: Option<bool>,
    pub emshut_pexit_dis: Option<bool>,
    pub emshut_exit_comm: Option<bool>,
    pub emshut_exit_vpack: Option<bool>,
}

impl DerivedPowerBmsSnapshot {
    pub const fn empty() -> Self {
        Self {
            addr: None,
            state: "pending",
            pack_mv: None,
            current_ma: None,
            soc_pct: None,
            cell_min_mv: None,
            cell_max_mv: None,
            no_battery: None,
            discharge_ready: None,
            charge_ready: None,
            full: None,
            issue_detail: None,
            rca_alarm: None,
            safety_alert: None,
            safety_status: None,
            pf_status: None,
            manufacturing_status: None,
            gauging_status: None,
            charging_status: None,
            op_status: None,
            op_status_raw_len: None,
            op_status_raw_bytes: None,
            afe_fet_status: None,
            afe_fet_control: None,
            afe_latch_status: None,
            afe_cell_balance_status: None,
            afe_chg_fet: None,
            afe_dsg_fet: None,
            emshut: None,
            pres: None,
            xchg: None,
            xdsg: None,
            op_chg_fet: None,
            op_dsg_fet: None,
            op_pchg_fet: None,
            chg_fet: None,
            dsg_fet: None,
            pchg_fet: None,
            discharge_path_contradiction: None,
            discharge_path_contradiction_reason: None,
            cuv: None,
            cuvc: None,
            cov: None,
            occ1: None,
            occ2: None,
            oc: None,
            safety_alert_oc: None,
            cuv_recovery_mv: None,
            cuv_recov_chg: None,
            fet_en: None,
            chg_en: None,
            dsg_en: None,
            charging_inhibit: None,
            charging_suspend: None,
            charging_hv: None,
            current_at_eoc_ma: None,
            da_configuration: None,
            power_config: None,
            emshut_en: None,
            emshut_pexit_dis: None,
            emshut_exit_comm: None,
            emshut_exit_vpack: None,
        }
    }
}

impl UpsStatusSnapshot {
    pub const fn empty() -> Self {
        Self {
            mode: "standby",
            requested_outputs: "none",
            active_outputs: "none",
            recoverable_outputs: "none",
            output_gate_reason: "none",
            input_source: "unknown",
            input_vbus_mv: None,
            input_ibus_ma: None,
            mains_present: None,
            pre_tps_vin_mv: None,
            vin_vbus_mv: None,
            input_gate_state: None,
            input_gate_reason: None,
            input_power_good: None,
            vin_iin_ma: None,
            tps_total_iout_ma: None,
            tps_limit_threshold_ma: None,
            input_pressure_state: "inactive",
            input_pressure_score_pct: None,
            input_pressure_reason: None,
            input_vin_baseline_mv: None,
            input_vin_drop_mv: None,
            assist_power_stage: None,
            assist_target_vout_mv: None,
            backup_reason: None,
            charger_state: "pending",
            charger_allow_charge: None,
            charger_ichg_ma: None,
            charger_ibat_ma: None,
            charger_vbat_present: None,
            charger_policy_target_ichg_ma: None,
            charger_limit_active: None,
            charger_limit_reason: None,
            charger_limit_detail: None,
            charger_limit_threshold_ma: None,
            charger_detail_status: None,
            battery_state: "pending",
            battery_pack_mv: None,
            battery_current_ma: None,
            battery_soc_pct: None,
            battery_cell_mv: [None, None, None, None],
            battery_cell_delta_mv: None,
            battery_balance_enabled: None,
            battery_balance_cfg_match: None,
            battery_balance_active: None,
            battery_balance_mask: None,
            battery_balance_cell: None,
            battery_balance_min_start_delta_mv: None,
            battery_no_battery: None,
            battery_discharge_ready: None,
            battery_charge_fet_on: None,
            battery_discharge_fet_on: None,
            battery_precharge_fet_on: None,
            battery_issue_detail: None,
            battery_recovery_pending: false,
            battery_last_result: None,
            out_a_state: "pending",
            out_a_enabled: None,
            out_a_vbus_mv: None,
            out_a_iout_ma: None,
            out_b_state: "pending",
            out_b_enabled: None,
            out_b_vbus_mv: None,
            out_b_iout_ma: None,
            tmp_a_state: "pending",
            tmp_a_c: None,
            tmp_b_state: "pending",
            tmp_b_c: None,
            charge_control: ChargeControlSnapshot::empty(),
            front_panel: FrontPanelRuntimeSnapshot::unavailable(),
            network: NetworkUiSummary::disabled(),
        }
    }
}

pub fn format_ipv4(buf: &mut String<16>, ipv4: [u8; 4]) {
    let _ = write!(buf, "{}.{}.{}.{}", ipv4[0], ipv4[1], ipv4[2], ipv4[3]);
}

#[cfg(test)]
mod tests {
    use super::{
        validate_advanced_power_settings, AdvancedPowerSettingsSnapshot,
        AdvancedPowerValidationError, DeviceSettingsSnapshot, NetworkUiSummary,
        RuntimeModePolicySnapshot, WifiConnectionState, WifiErrorKind, WifiSnapshot,
    };

    #[test]
    fn connected_ui_summary_prefers_ip_text() {
        let summary = NetworkUiSummary::from_wifi(WifiSnapshot {
            state: WifiConnectionState::Connected,
            ipv4: Some([192, 168, 31, 15]),
            ..WifiSnapshot::disabled()
        });
        assert_eq!(summary.subtitle().as_str(), "IP 192.168.31.15");
    }

    #[test]
    fn error_ui_summary_includes_short_hint() {
        let summary = NetworkUiSummary::from_wifi(WifiSnapshot {
            state: WifiConnectionState::Error,
            last_error: Some(WifiErrorKind::DhcpTimeout),
            ..WifiSnapshot::disabled()
        });
        assert_eq!(summary.subtitle().as_str(), "WIFI RETRY DHCP WAIT");
    }

    fn resolve_policy(
        settings: AdvancedPowerSettingsSnapshot,
        rated_vout_mv: u16,
    ) -> RuntimeModePolicySnapshot {
        settings.resolve_runtime_policy(rated_vout_mv).unwrap()
    }

    #[test]
    fn advanced_power_defaults_resolve_against_rated_vout() {
        let policy = resolve_policy(
            AdvancedPowerSettingsSnapshot::defaults_for_rated_vout(19_000),
            19_000,
        );
        assert_eq!(policy.rated_vout_mv, 19_000);
        assert_eq!(policy.standby_vout_mv(), 18_100);
        assert_eq!(policy.assist_low_vout_mv(), 18_400);
        assert_eq!(policy.fixed.assist_enter_iout_ma, 100);
        assert_eq!(policy.fixed.assist_exit_iout_ma, 50);
        assert_eq!(policy.fixed.assist_required_samples, 2);
        assert_eq!(policy.fixed.assist_ramp_step_mv, 100);
        assert_eq!(policy.fixed.assist_ramp_interval_ms, 200);
        assert_eq!(policy.fixed.rated_enter_iout_ma, 100);
        assert_eq!(policy.fixed.rated_exit_iout_ma, 50);
        assert_eq!(policy.fixed.vin_drop_threshold_pct, 4);
        assert_eq!(policy.fixed.required_samples, 2);
        assert_eq!(policy.tuning.input_uvlo_cutoff_mv, 18_200);
        assert_eq!(policy.tuning.input_uvlo_recover_mv, 18_400);
        assert_eq!(policy.tuning.input_uvlo_required_samples, 3);
        assert_eq!(policy.fixed.source_limited_vin_drop_pct, 1);
        assert_eq!(policy.tuning.source_limited_enter_iout_ma, 1_100);
        assert_eq!(policy.fixed.source_limited_exit_iout_ma, 50);
        assert_eq!(policy.fixed.source_limited_required_samples, 2);
        assert_eq!(policy.fixed.source_limited_recover_margin_mv, 400);
    }

    #[test]
    fn advanced_power_resolve_rejects_voltage_order_inversion() {
        let err = AdvancedPowerSettingsSnapshot {
            standby_drop_mv: 580,
            ..AdvancedPowerSettingsSnapshot::defaults()
        }
        .resolve_runtime_policy(12_000)
        .unwrap_err();
        assert_eq!(err, AdvancedPowerValidationError::VoltageOrderInvalid);
        assert_eq!(err.code(), "advanced_power_voltage_order_invalid");
    }

    #[test]
    fn advanced_power_resolve_rejects_invalid_source_limited_current_order() {
        let err = AdvancedPowerSettingsSnapshot {
            source_limited_enter_delta_ma: -100,
            ..AdvancedPowerSettingsSnapshot::defaults()
        }
        .resolve_runtime_policy(12_000)
        .unwrap_err();
        assert_eq!(
            err,
            AdvancedPowerValidationError::SourceLimitedCurrentOrderInvalid
        );
        assert_eq!(
            err.message(),
            "expanded source_limited_exit_threshold_ma must not exceed source_limited_enter_threshold_ma"
        );
    }

    #[test]
    fn advanced_power_rejects_invalid_input_uvlo_order() {
        let err = validate_advanced_power_settings(AdvancedPowerSettingsSnapshot {
            input_uvlo_cutoff_mv: 11_500,
            input_uvlo_recover_mv: 11_300,
            ..AdvancedPowerSettingsSnapshot::defaults()
        })
        .unwrap_err();
        assert_eq!(
            err,
            AdvancedPowerValidationError::InputUvloThresholdOrderInvalid
        );
    }

    #[test]
    fn settings_defaults_can_follow_variant_rated_vout() {
        let settings = DeviceSettingsSnapshot::defaults_for_rated_vout(19_000);
        assert_eq!(settings.advanced_power_capabilities.rated_vout_mv, 19_000);
        assert_eq!(settings.advanced_power.standby_drop_mv, 900);
        assert_eq!(settings.advanced_power.input_uvlo_cutoff_mv, 18_200);
        assert_eq!(settings.advanced_power.input_uvlo_recover_mv, 18_400);
        assert_eq!(settings.advanced_power.source_limited_enter_delta_ma, 1_000);
    }

    #[test]
    fn settings_defaults_use_11v3_standby_for_12v_profile() {
        let settings = DeviceSettingsSnapshot::defaults_for_rated_vout(12_000);
        let policy = resolve_policy(settings.advanced_power, 12_000);
        assert_eq!(settings.advanced_power.standby_drop_mv, 700);
        assert_eq!(policy.standby_vout_mv(), 11_300);
        assert_eq!(settings.advanced_power.input_uvlo_cutoff_mv, 11_300);
        assert_eq!(settings.advanced_power.input_uvlo_recover_mv, 11_500);
        assert_eq!(
            settings.advanced_power_capabilities.standby_drop_mv.default,
            700
        );
        assert_eq!(
            settings
                .advanced_power_capabilities
                .input_uvlo_cutoff_mv
                .default,
            11_300
        );
    }
}
