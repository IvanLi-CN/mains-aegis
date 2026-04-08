use crate::front_panel_scene::{
    is_bq40_activation_needed, BmsRecoveryUiAction, BmsResultKind, DashboardInputSource,
    ManualChargeSpeed, ManualChargeStopReason, SelfCheckCommState, SelfCheckUiSnapshot, UpsMode,
};
use esp_firmware::bq40z50;
use esp_firmware::fan;
use esp_firmware::output_state::{self as output_state_logic, OutputGateReason};
use esp_firmware::time::Duration;
use esp_firmware::usb_pd;

use super::channel::OutputChannel;
use super::{
    discharge_authorization_input_ready, mains_present_edge, mains_present_from_vin,
    mark_vin_telemetry_unavailable, normalize_charger_input_power_sample,
    record_vin_sample_failure, stable_mains_present, stable_mains_state, ups_mode_from_mains,
    AudioBatteryLowState, AudioChargePhase, AudioMainsSource, Bq40z50Snapshot,
    ChargerInputPowerSample, ChargerInputSampleIssue, OutputRuntimeState, StableMainsState,
    CHARGER_INPUT_POWER_ANOMALY_W10,
};

const BMS_SELF_CHECK_AUTO_RECOVERY_ENABLED: bool = false;
const CHARGE_POLICY_NORMAL_ICHG_MA: u16 = 500;
const CHARGE_POLICY_DC_DERATED_ICHG_MA: u16 = 100;
const CHARGE_POLICY_START_RSOC_PCT: u16 = 80;
const CHARGE_POLICY_START_CELL_MIN_MV: u16 = 3_700;
const CHARGE_POLICY_DC_DERATE_ENTER_IBUS_MA: i32 = 3_000;
const CHARGE_POLICY_DC_DERATE_EXIT_IBUS_MA: i32 = 2_700;
const CHARGE_POLICY_DC_DERATE_ENTER_HOLD: Duration = Duration::from_secs(1);
const CHARGE_POLICY_DC_DERATE_EXIT_HOLD: Duration = Duration::from_secs(5);
const CHARGE_POLICY_OUTPUT_POWER_LIMIT_W10: u32 = 50;
const CHARGE_POLICY_OUTPUT_POWER_RESUME_W10: u32 = 45;
const CHARGE_POLICY_OUTPUT_BLOCK_ENTER_POLLS: u8 = 2;
const CHARGE_POLICY_OUTPUT_BLOCK_EXIT_POLLS: u8 = 3;
const FAN_RPM_SAMPLE_WINDOW_MS: u64 = 1_200;
const FAN_RPM_MAX_SAMPLE_WINDOW_MS: u64 = 2_000;
const FAN_RPM_MIN_SAMPLE_REVS: u32 = 2;
const VIN_MAINS_PRESENT_THRESHOLD_MV: u16 = 3_000;
const VIN_MAINS_LATCH_FAILURE_LIMIT: u8 = 2;

pub(super) fn bq40_op_bit(op_status: Option<u32>, mask: u32) -> Option<bool> {
    op_status.map(|raw| (raw & mask) != 0)
}

#[derive(Clone, Copy)]
pub struct AppliedFanState {
    pub command: fan::FanLevel,
    pub pwm_pct: u8,
    pub vset_duty_pct: u8,
    pub degraded: bool,
    pub disabled_by_feature: bool,
}

pub(super) fn detail_input_source(
    vbus_present: bool,
    ac1_present: bool,
    ac2_present: bool,
) -> Option<DashboardInputSource> {
    if ac1_present && !ac2_present {
        Some(DashboardInputSource::UsbC)
    } else if ac2_present && !ac1_present {
        Some(DashboardInputSource::DcIn)
    } else if ac1_present || ac2_present || vbus_present {
        Some(DashboardInputSource::Auto)
    } else {
        None
    }
}

pub(super) fn dashboard_input_source_name(source: Option<DashboardInputSource>) -> &'static str {
    match source {
        Some(DashboardInputSource::DcIn) => "dcin",
        Some(DashboardInputSource::UsbC) => "usbc",
        Some(DashboardInputSource::Auto) => "auto",
        None => "none",
    }
}

pub(super) const fn manual_charge_stop_hold_blocks_charge(
    stop_inhibit: bool,
    activation_pending: bool,
    activation_force_charge: bool,
) -> bool {
    stop_inhibit && !activation_pending && !activation_force_charge
}

pub(super) fn manual_charge_speed_derated(speed: ManualChargeSpeed, dc_derated: bool) -> bool {
    speed != ManualChargeSpeed::Ma100 && dc_derated
}

pub(super) const fn manual_charge_safety_notice_active(
    last_stop_reason: ManualChargeStopReason,
    active: bool,
    stop_inhibit: bool,
    blocked: bool,
) -> bool {
    matches!(last_stop_reason, ManualChargeStopReason::SafetyBlocked)
        && !active
        && !stop_inhibit
        && blocked
}

pub(super) fn usb_pd_restore_vindpm_mv(measured_input_voltage_mv: Option<u16>) -> u16 {
    match measured_input_voltage_mv {
        Some(voltage_mv) if voltage_mv >= 7_000 => {
            voltage_mv.saturating_sub(1_400).clamp(3_600, 22_000)
        }
        Some(voltage_mv) => voltage_mv.saturating_sub(700).clamp(3_600, 22_000),
        None => 3_600,
    }
}

pub(super) fn usb_pd_measured_input_voltage_mv(
    usb_c_vbus_present: Option<bool>,
    vac1_adc_mv: Option<u16>,
) -> Option<u16> {
    matches!(usb_c_vbus_present, Some(true))
        .then_some(vac1_adc_mv)
        .flatten()
}


fn charge_policy_channel_enabled(
    snapshot_enabled: Option<bool>,
    active_outputs: EnabledOutputs,
    channel: OutputChannel,
) -> bool {
    active_outputs.is_enabled(channel) || snapshot_enabled == Some(true)
}

pub(super) fn tps_channel_output_power_w10(
    enabled: bool,
    vbus_mv: Option<u16>,
    current_ma: Option<i32>,
) -> Option<u32> {
    if !enabled {
        return Some(0);
    }
    Some((u32::from(vbus_mv?) * current_ma?.max(0) as u32) / 100_000)
}

pub(super) fn charge_policy_output_power_w10(
    snapshot: &SelfCheckUiSnapshot,
    active_outputs: EnabledOutputs,
) -> Option<u32> {
    let out_a_enabled =
        charge_policy_channel_enabled(snapshot.tps_a_enabled, active_outputs, OutputChannel::OutA);
    let out_b_enabled =
        charge_policy_channel_enabled(snapshot.tps_b_enabled, active_outputs, OutputChannel::OutB);
    let out_a = tps_channel_output_power_w10(
        out_a_enabled,
        snapshot.out_a_vbus_mv,
        snapshot.tps_a_iout_ma,
    );
    let out_b = tps_channel_output_power_w10(
        out_b_enabled,
        snapshot.out_b_vbus_mv,
        snapshot.tps_b_iout_ma,
    );

    match (out_a, out_b) {
        (Some(a), Some(b)) => Some(a + b),
        (Some(a), None) if !out_b_enabled => Some(a),
        (None, Some(b)) if !out_a_enabled => Some(b),
        _ => None,
    }
}

pub(super) fn charge_policy_output_enabled(
    snapshot: &SelfCheckUiSnapshot,
    active_outputs: EnabledOutputs,
) -> bool {
    charge_policy_channel_enabled(snapshot.tps_a_enabled, active_outputs, OutputChannel::OutA)
        || charge_policy_channel_enabled(
            snapshot.tps_b_enabled,
            active_outputs,
            OutputChannel::OutB,
        )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChargePolicyState {
    BlockedNoInput,
    BlockedTemp,
    BlockedOutputOverload,
    BlockedNoBms,
    IdleWaitThreshold,
    Charging500mA,
    Charging100mADcDerated,
    FullLatched,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChargePolicyOutputBlockReason {
    OverLimit,
    PowerUnknown,
}

impl ChargePolicyOutputBlockReason {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::OverLimit => "blocked_output_over_limit",
            Self::PowerUnknown => "blocked_output_power_unknown",
        }
    }
}

impl ChargePolicyState {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::BlockedNoInput => "blocked_no_input",
            Self::BlockedTemp => "blocked_temp",
            Self::BlockedOutputOverload => "blocked_output_over_limit",
            Self::BlockedNoBms => "blocked_no_bms",
            Self::IdleWaitThreshold => "idle_wait_threshold",
            Self::Charging500mA => "charging_500ma",
            Self::Charging100mADcDerated => "charging_100ma_dc_derated",
            Self::FullLatched => "full_latched",
        }
    }

    pub(super) const fn ui_status(self) -> &'static str {
        match self {
            Self::BlockedNoInput => "NOAC",
            Self::BlockedTemp => "TEMP",
            Self::BlockedOutputOverload => "LOAD",
            Self::BlockedNoBms => "LOCK",
            Self::IdleWaitThreshold => "WAIT",
            Self::Charging500mA => "CHG500",
            Self::Charging100mADcDerated => "CHG100",
            Self::FullLatched => "FULL",
        }
    }

    pub(super) const fn charger_active(self) -> bool {
        matches!(self, Self::Charging500mA | Self::Charging100mADcDerated)
    }
}

pub(super) fn detail_charger_status_text(state: ChargePolicyState) -> &'static str {
    state.ui_status()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChargeStartReason {
    RsocLow,
    CellLow,
    RsocAndCellLow,
}

impl ChargeStartReason {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::RsocLow => "rsoc_low",
            Self::CellLow => "cell_low",
            Self::RsocAndCellLow => "rsoc_and_cell_low",
        }
    }
}

pub(super) fn charge_policy_start_reason(
    rsoc_pct: u16,
    cell_min_mv: u16,
) -> Option<ChargeStartReason> {
    match (
        rsoc_pct < CHARGE_POLICY_START_RSOC_PCT,
        cell_min_mv < CHARGE_POLICY_START_CELL_MIN_MV,
    ) {
        (true, true) => Some(ChargeStartReason::RsocAndCellLow),
        (true, false) => Some(ChargeStartReason::RsocLow),
        (false, true) => Some(ChargeStartReason::CellLow),
        (false, false) => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChargeFullReason {
    BmsFc,
    ChargerTermination,
    BmsFcAndChargerTermination,
}

impl ChargeFullReason {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::BmsFc => "bq40_fc",
            Self::ChargerTermination => "termination_done",
            Self::BmsFcAndChargerTermination => "bq40_fc_and_termination_done",
        }
    }
}

pub(super) fn charge_policy_full_reason(
    bms_full: bool,
    charger_done: bool,
) -> Option<ChargeFullReason> {
    match (bms_full, charger_done) {
        (true, true) => Some(ChargeFullReason::BmsFcAndChargerTermination),
        (true, false) => Some(ChargeFullReason::BmsFc),
        (false, true) => Some(ChargeFullReason::ChargerTermination),
        (false, false) => None,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ChargePolicyMemory {
    pub(super) charge_latched: bool,
    pub(super) full_latched: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ChargePolicyOutputLoadTracker {
    pub(super) blocked: bool,
    pub(super) enter_streak: u8,
    pub(super) exit_streak: u8,
}

impl ChargePolicyOutputLoadTracker {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn note_unknown_sample(&mut self) {
        self.blocked = true;
        self.enter_streak = 0;
        self.exit_streak = 0;
    }

    pub(super) fn observe(&mut self, output_enabled: bool, output_power_w10: Option<u32>) -> bool {
        let Some(output_power_w10) = output_power_w10 else {
            self.reset();
            return false;
        };

        if !output_enabled {
            self.reset();
            return false;
        }

        if self.blocked {
            self.enter_streak = 0;
            if output_power_w10 < CHARGE_POLICY_OUTPUT_POWER_RESUME_W10 {
                self.exit_streak = self.exit_streak.saturating_add(1);
                if self.exit_streak >= CHARGE_POLICY_OUTPUT_BLOCK_EXIT_POLLS {
                    self.reset();
                    return false;
                }
            } else {
                self.exit_streak = 0;
            }
            true
        } else {
            self.exit_streak = 0;
            if output_power_w10 > CHARGE_POLICY_OUTPUT_POWER_LIMIT_W10 {
                self.enter_streak = self.enter_streak.saturating_add(1);
                if self.enter_streak >= CHARGE_POLICY_OUTPUT_BLOCK_ENTER_POLLS {
                    self.blocked = true;
                    self.enter_streak = 0;
                    return true;
                }
            } else {
                self.enter_streak = 0;
            }
            false
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ChargePolicyDerateTracker {
    pub(super) derated: bool,
    pub(super) over_limit_since_ms: Option<u64>,
    pub(super) recover_since_ms: Option<u64>,
}

impl ChargePolicyDerateTracker {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn observe(&mut self, now_ms: u64, dc_input_only: bool, ibus_ma: Option<i32>) {
        if !dc_input_only {
            self.reset();
            return;
        }

        let Some(ibus_ma) = ibus_ma else {
            self.over_limit_since_ms = None;
            self.recover_since_ms = None;
            return;
        };

        if !self.derated {
            self.recover_since_ms = None;
            if ibus_ma > CHARGE_POLICY_DC_DERATE_ENTER_IBUS_MA {
                let since = self.over_limit_since_ms.get_or_insert(now_ms);
                if now_ms.saturating_sub(*since)
                    >= CHARGE_POLICY_DC_DERATE_ENTER_HOLD.as_millis() as u64
                {
                    self.derated = true;
                    self.over_limit_since_ms = None;
                }
            } else {
                self.over_limit_since_ms = None;
            }
        } else {
            self.over_limit_since_ms = None;
            if ibus_ma < CHARGE_POLICY_DC_DERATE_EXIT_IBUS_MA {
                let since = self.recover_since_ms.get_or_insert(now_ms);
                if now_ms.saturating_sub(*since)
                    >= CHARGE_POLICY_DC_DERATE_EXIT_HOLD.as_millis() as u64
                {
                    self.derated = false;
                    self.recover_since_ms = None;
                }
            } else {
                self.recover_since_ms = None;
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ChargePolicyTelemetry {
    pub(super) rsoc_pct: u16,
    pub(super) cell_min_mv: u16,
    pub(super) charge_ready: bool,
    pub(super) bms_full: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ChargePolicyInput {
    pub(super) input_present: bool,
    pub(super) vbat_present: bool,
    pub(super) ts_cold: bool,
    pub(super) ts_hot: bool,
    pub(super) input_source: Option<DashboardInputSource>,
    pub(super) ibus_ma: Option<i32>,
    pub(super) output_enabled: bool,
    pub(super) output_power_w10: Option<u32>,
    pub(super) telemetry: Option<ChargePolicyTelemetry>,
    pub(super) charger_done: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ChargePolicyDecision {
    pub(super) state: ChargePolicyState,
    pub(super) allow_charge: bool,
    pub(super) target_ichg_ma: Option<u16>,
    pub(super) start_reason: Option<ChargeStartReason>,
    pub(super) full_reason: Option<ChargeFullReason>,
    pub(super) output_block_reason: Option<ChargePolicyOutputBlockReason>,
}

pub(super) fn charge_policy_step(
    memory: &mut ChargePolicyMemory,
    derate: &mut ChargePolicyDerateTracker,
    output_load: &mut ChargePolicyOutputLoadTracker,
    now_ms: u64,
    input: ChargePolicyInput,
) -> ChargePolicyDecision {
    let start_reason = input.telemetry.and_then(|telemetry| {
        charge_policy_start_reason(telemetry.rsoc_pct, telemetry.cell_min_mv)
    });

    if !input.input_present {
        memory.charge_latched = false;
        derate.reset();
        output_load.reset();
        return ChargePolicyDecision {
            state: ChargePolicyState::BlockedNoInput,
            allow_charge: false,
            target_ichg_ma: None,
            start_reason,
            full_reason: None,
            output_block_reason: None,
        };
    }

    if input.ts_cold || input.ts_hot {
        memory.charge_latched = false;
        derate.reset();
        output_load.reset();
        return ChargePolicyDecision {
            state: ChargePolicyState::BlockedTemp,
            allow_charge: false,
            target_ichg_ma: None,
            start_reason,
            full_reason: None,
            output_block_reason: None,
        };
    }

    if input.output_enabled && input.output_power_w10.is_none() {
        memory.charge_latched = false;
        derate.reset();
        output_load.note_unknown_sample();
        return ChargePolicyDecision {
            state: ChargePolicyState::BlockedOutputOverload,
            allow_charge: false,
            target_ichg_ma: None,
            start_reason,
            full_reason: None,
            output_block_reason: Some(ChargePolicyOutputBlockReason::PowerUnknown),
        };
    }

    if output_load.observe(input.output_enabled, input.output_power_w10) {
        memory.charge_latched = false;
        derate.reset();
        return ChargePolicyDecision {
            state: ChargePolicyState::BlockedOutputOverload,
            allow_charge: false,
            target_ichg_ma: None,
            start_reason,
            full_reason: None,
            output_block_reason: Some(ChargePolicyOutputBlockReason::OverLimit),
        };
    }

    let Some(telemetry) = input.telemetry else {
        memory.charge_latched = false;
        derate.reset();
        output_load.reset();
        return ChargePolicyDecision {
            state: ChargePolicyState::BlockedNoBms,
            allow_charge: false,
            target_ichg_ma: None,
            start_reason,
            full_reason: None,
            output_block_reason: None,
        };
    };

    if !input.vbat_present || !telemetry.charge_ready {
        memory.charge_latched = false;
        derate.reset();
        output_load.reset();
        return ChargePolicyDecision {
            state: ChargePolicyState::BlockedNoBms,
            allow_charge: false,
            target_ichg_ma: None,
            start_reason,
            full_reason: None,
            output_block_reason: None,
        };
    }

    if memory.full_latched && start_reason.is_some() {
        memory.full_latched = false;
    }

    let full_reason = if memory.charge_latched || memory.full_latched {
        charge_policy_full_reason(telemetry.bms_full, input.charger_done)
    } else {
        None
    };

    if let Some(full_reason) = full_reason {
        memory.charge_latched = false;
        memory.full_latched = true;
        derate.reset();
        return ChargePolicyDecision {
            state: ChargePolicyState::FullLatched,
            allow_charge: false,
            target_ichg_ma: None,
            start_reason,
            full_reason: Some(full_reason),
            output_block_reason: None,
        };
    }

    if memory.full_latched {
        memory.charge_latched = false;
        derate.reset();
        return ChargePolicyDecision {
            state: ChargePolicyState::FullLatched,
            allow_charge: false,
            target_ichg_ma: None,
            start_reason,
            full_reason: None,
            output_block_reason: None,
        };
    }

    if !memory.charge_latched {
        if start_reason.is_some() {
            memory.charge_latched = true;
        } else {
            derate.reset();
            return ChargePolicyDecision {
                state: ChargePolicyState::IdleWaitThreshold,
                allow_charge: false,
                target_ichg_ma: None,
                start_reason: None,
                full_reason: None,
                output_block_reason: None,
            };
        }
    }

    derate.observe(
        now_ms,
        matches!(input.input_source, Some(DashboardInputSource::DcIn)),
        input.ibus_ma,
    );

    let state = if derate.derated {
        ChargePolicyState::Charging100mADcDerated
    } else {
        ChargePolicyState::Charging500mA
    };
    let target_ichg_ma = Some(if derate.derated {
        CHARGE_POLICY_DC_DERATED_ICHG_MA
    } else {
        CHARGE_POLICY_NORMAL_ICHG_MA
    });

    ChargePolicyDecision {
        state,
        allow_charge: true,
        target_ichg_ma,
        start_reason,
        full_reason: None,
        output_block_reason: None,
    }
}

pub(super) fn detail_fan_status_text(applied: AppliedFanState, tach_fault: bool) -> &'static str {
    if tach_fault {
        "FAULT"
    } else {
        match applied.command {
            fan::FanLevel::Off => "OFF",
            fan::FanLevel::Low => "LOW",
            fan::FanLevel::Mid => "MID",
            fan::FanLevel::High => "HIGH",
        }
    }
}

pub(super) fn charger_audio_thermal_stress(ts_cool: bool, treg: bool) -> bool {
    ts_cool || treg
}

pub(super) fn charger_detail_status_text(
    charger_fault: bool,
    ts_warm: bool,
    policy_status_text: &'static str,
) -> &'static str {
    if charger_fault {
        "FAULT"
    } else if ts_warm {
        "WARM"
    } else {
        policy_status_text
    }
}

pub(super) fn charger_home_status_text(
    charger_fault: bool,
    ts_cold: bool,
    ts_hot: bool,
    ts_warm: bool,
    policy_status_text: &'static str,
) -> &'static str {
    if ts_cold || ts_hot {
        "TEMP"
    } else if charger_fault {
        "LOCK"
    } else if ts_warm {
        "WARM"
    } else {
        policy_status_text
    }
}

pub(super) fn charger_detail_notice_text(
    charger_fault: bool,
    ts_warm: bool,
    policy_notice_text: &'static str,
) -> &'static str {
    if charger_fault {
        "CHARGER PROTECTION ACTIVE"
    } else if ts_warm {
        "BQ25792 TS WARM - FAN FORCED HIGH"
    } else {
        policy_notice_text
    }
}

pub(super) fn thermal_notice_text(
    therm_kill_asserted: bool,
    tmp_hw_protect_test_mode: bool,
) -> &'static str {
    if therm_kill_asserted {
        "THERM KILL ASSERTED"
    } else if tmp_hw_protect_test_mode {
        "TMP HW PROTECT TEST MODE"
    } else {
        "LIVE DATA"
    }
}

pub(super) fn temp_c_to_x16(temp_c: Option<i16>) -> Option<i16> {
    temp_c.map(|value| value.saturating_mul(16))
}

pub(super) fn accumulate_max_temp_c_x16(
    max_temp_c_x16: Option<i16>,
    temp_c_x16: Option<i16>,
) -> Option<i16> {
    match (max_temp_c_x16, temp_c_x16) {
        (Some(current), Some(sample)) => Some(current.max(sample)),
        (None, Some(sample)) => Some(sample),
        (current, None) => current,
    }
}

pub(super) fn bms_thermal_max_c_x16(snapshot: &SelfCheckUiSnapshot) -> Option<i16> {
    let detail = &snapshot.dashboard_detail;
    let mut max_temp_c_x16 = None;

    max_temp_c_x16 = accumulate_max_temp_c_x16(max_temp_c_x16, temp_c_to_x16(detail.board_temp_c));
    max_temp_c_x16 =
        accumulate_max_temp_c_x16(max_temp_c_x16, temp_c_to_x16(detail.battery_temp_c));
    for sample in detail.cell_temp_c {
        max_temp_c_x16 = accumulate_max_temp_c_x16(max_temp_c_x16, temp_c_to_x16(sample));
    }

    max_temp_c_x16
}

pub(super) fn max_optional_temp(a: Option<i16>, b: Option<i16>) -> Option<i16> {
    accumulate_max_temp_c_x16(a, b)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct FanRpmTracker {
    pub(super) window_started_ms: Option<u64>,
    pub(super) window_pulses: u32,
    pub(super) raw_rpm: Option<u16>,
    pub(super) display_rpm: Option<u16>,
}

impl FanRpmTracker {
    pub(super) const fn new() -> Self {
        Self {
            window_started_ms: None,
            window_pulses: 0,
            raw_rpm: None,
            display_rpm: None,
        }
    }

    pub(super) fn reset(&mut self) {
        self.window_started_ms = None;
        self.window_pulses = 0;
        self.raw_rpm = None;
        self.display_rpm = None;
    }

    pub(super) const fn raw_rpm(&self) -> Option<u16> {
        self.raw_rpm
    }

    pub(super) const fn display_rpm(&self) -> Option<u16> {
        self.display_rpm
    }

    pub(super) fn observe(
        &mut self,
        now_ms: u64,
        pulse_delta: u32,
        status: fan::Status,
        cfg: fan::Config,
    ) -> Option<u16> {
        if !status.command.enabled() || status.tach_fault || cfg.tach_pulses_per_rev == 0 {
            self.reset();
            return None;
        }

        let started_ms = self.window_started_ms.get_or_insert(now_ms);
        self.window_pulses = self.window_pulses.saturating_add(pulse_delta);

        let elapsed_ms = now_ms.saturating_sub(*started_ms);
        let enough_pulses = self.window_pulses
            >= u32::from(cfg.tach_pulses_per_rev).saturating_mul(FAN_RPM_MIN_SAMPLE_REVS);
        let should_refresh = elapsed_ms >= FAN_RPM_MAX_SAMPLE_WINDOW_MS
            || (elapsed_ms >= FAN_RPM_SAMPLE_WINDOW_MS && enough_pulses);

        if should_refresh {
            self.raw_rpm =
                fan_rpm_from_sample(self.window_pulses, elapsed_ms, cfg.tach_pulses_per_rev);
            if let Some(raw_rpm) = self.raw_rpm {
                self.display_rpm = smooth_fan_rpm(self.display_rpm, raw_rpm);
            }
            self.window_started_ms = Some(now_ms);
            self.window_pulses = 0;
        }

        self.display_rpm
    }
}

pub(super) fn fan_rpm_from_sample(
    pulse_count: u32,
    elapsed_ms: u64,
    pulses_per_rev: u8,
) -> Option<u16> {
    if pulse_count == 0 || elapsed_ms == 0 || pulses_per_rev == 0 {
        return None;
    }

    let rpm = u64::from(pulse_count)
        .saturating_mul(60_000)
        .checked_div(elapsed_ms.saturating_mul(u64::from(pulses_per_rev)))?;
    Some(rpm.min(u64::from(u16::MAX)) as u16)
}

pub(super) fn smooth_fan_rpm(previous_rpm: Option<u16>, raw_rpm: u16) -> Option<u16> {
    match previous_rpm {
        None => Some(raw_rpm),
        Some(previous_rpm) => Some(
            (((u32::from(previous_rpm) * 2) + u32::from(raw_rpm) + 1) / 3).min(u32::from(u16::MAX))
                as u16,
        ),
    }
}

pub(super) fn boot_diag_auto_recovery_enabled(auto_validate: bool) -> bool {
    BMS_SELF_CHECK_AUTO_RECOVERY_ENABLED && auto_validate
}

pub(super) fn detail_battery_temp_c(snapshot: &Bq40z50Snapshot) -> Option<i16> {
    if let Some(da_status2) = snapshot.da_status2 {
        let temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(da_status2.cell_temp_k_x10);
        if (-400..=1250).contains(&temp_c_x10) {
            return Some((temp_c_x10 / 10) as i16);
        }
    }

    let temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(snapshot.temp_k_x10);
    if (-400..=1250).contains(&temp_c_x10) {
        Some((temp_c_x10 / 10) as i16)
    } else {
        None
    }
}

pub(super) fn bq40_cell_min_mv(snapshot: &Bq40z50Snapshot) -> u16 {
    snapshot.cell_mv.into_iter().min().unwrap_or_default()
}

pub(super) fn detail_da_status2_temp_c(temp_k_x10: u16) -> Option<i16> {
    let temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(temp_k_x10);
    (-400..=1250)
        .contains(&temp_c_x10)
        .then_some((temp_c_x10 / 10) as i16)
}

pub(super) fn detail_bms_cell_sensor_temps(snapshot: &Bq40z50Snapshot) -> [Option<i16>; 4] {
    snapshot
        .da_status2
        .map_or([None, None, None, None], |da_status2| {
            da_status2.ts_temp_k_x10.map(detail_da_status2_temp_c)
        })
}

pub(super) fn detail_bms_board_temp_c(snapshot: &Bq40z50Snapshot) -> Option<i16> {
    snapshot
        .da_status2
        .and_then(|da_status2| detail_da_status2_temp_c(da_status2.ts_temp_k_x10[0]))
}

pub(super) fn filter_energy_mwh(cwh: u16) -> Option<u32> {
    (cwh != u16::MAX).then_some(cwh as u32 * 10)
}

pub(super) fn approximate_energy_mwh(capacity_mah: u16, vpack_mv: u16) -> Option<u32> {
    (capacity_mah != 0 && vpack_mv != 0).then_some(capacity_mah as u32 * vpack_mv as u32 / 1000)
}

pub(super) fn detail_bms_energy_mwh(snapshot: &Bq40z50Snapshot) -> Option<u32> {
    if (snapshot.battery_mode & bq40z50::battery_mode::CAPM) != 0 {
        Some(snapshot.remcap as u32 * 10)
    } else {
        snapshot
            .filter_capacity
            .and_then(|filter| filter_energy_mwh(filter.remaining_energy_cwh))
            .or_else(|| approximate_energy_mwh(snapshot.remcap, snapshot.vpack_mv))
    }
}

pub(super) fn detail_bms_full_capacity_mwh(snapshot: &Bq40z50Snapshot) -> Option<u32> {
    if (snapshot.battery_mode & bq40z50::battery_mode::CAPM) != 0 {
        Some(snapshot.fcc as u32 * 10)
    } else {
        snapshot
            .filter_capacity
            .and_then(|filter| filter_energy_mwh(filter.full_charge_energy_cwh))
            .or_else(|| approximate_energy_mwh(snapshot.fcc, snapshot.vpack_mv))
    }
}

pub(super) fn detail_bms_balance_mask(snapshot: &Bq40z50Snapshot) -> Option<u8> {
    match bq40_op_bit(snapshot.op_status, bq40z50::operation_status::CB) {
        Some(false) => Some(0),
        Some(true) => snapshot.afe_register.and_then(|afe| {
            let mask = afe.cell_balance_status & 0x0F;
            if mask == 0 {
                None
            } else {
                Some(mask)
            }
        }),
        None => None,
    }
}

pub(super) fn detail_bms_single_balance_cell(balance_mask: Option<u8>) -> Option<u8> {
    let mask = balance_mask?;
    if mask.count_ones() != 1 {
        return None;
    }

    Some(mask.trailing_zeros() as u8 + 1)
}

pub(super) fn bq40_primary_reason(
    batt_status: u16,
    op_status: Option<u32>,
    charge_reason: &'static str,
    discharge_reason: &'static str,
) -> &'static str {
    if bq40z50::battery_status::error_code(batt_status) != 0 {
        return "sbs_error_code";
    }
    if (batt_status & bq40z50::battery_status::RCA) != 0 {
        return "remaining_capacity_alarm";
    }
    if bq40_op_bit(op_status, bq40z50::operation_status::PF) == Some(true) {
        return "permanent_failure";
    }
    if discharge_reason != "ready" && discharge_reason != "op_status_unavailable" {
        return discharge_reason;
    }
    if charge_reason != "ready" && charge_reason != "op_status_unavailable" {
        return charge_reason;
    }
    if bq40_op_bit(op_status, bq40z50::operation_status::SLEEP) == Some(true) {
        return "sleep_mode";
    }
    if op_status.is_none() {
        return "op_status_unavailable";
    }
    "nominal"
}

pub(super) fn bq40_protection_active(batt_status: u16, op_status: Option<u32>) -> bool {
    // BatteryStatus alarm bits like TCA/OTA/OCA/TDA are advisory thresholds and
    // should not drive the hard "battery protection" UI/audio state on their own.
    bq40_op_bit(op_status, bq40z50::operation_status::PF) == Some(true)
        || bq40z50::battery_status::error_code(batt_status) != 0
}

pub(super) fn bq40_cell_min_max_delta(cell_mv: &[u16; 4]) -> (u16, u16, u16) {
    let mut min_mv = cell_mv[0];
    let mut max_mv = cell_mv[0];

    for mv in cell_mv.iter().skip(1).copied() {
        if mv < min_mv {
            min_mv = mv;
        }
        if mv > max_mv {
            max_mv = mv;
        }
    }

    (min_mv, max_mv, max_mv.saturating_sub(min_mv))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnabledOutputs {
    None,
    Only(OutputChannel),
    Both,
}

impl EnabledOutputs {
    pub fn is_enabled(self, ch: OutputChannel) -> bool {
        match self {
            EnabledOutputs::None => false,
            EnabledOutputs::Only(only) => only == ch,
            EnabledOutputs::Both => true,
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            EnabledOutputs::None => "none",
            EnabledOutputs::Only(OutputChannel::OutA) => "out_a",
            EnabledOutputs::Only(OutputChannel::OutB) => "out_b",
            EnabledOutputs::Both => "out_a+out_b",
        }
    }
}

pub(super) const fn enabled_outputs_from_flags(out_a: bool, out_b: bool) -> EnabledOutputs {
    match (out_a, out_b) {
        (true, true) => EnabledOutputs::Both,
        (true, false) => EnabledOutputs::Only(OutputChannel::OutA),
        (false, true) => EnabledOutputs::Only(OutputChannel::OutB),
        (false, false) => EnabledOutputs::None,
    }
}

pub(super) const fn logic_outputs_from_enabled(
    outputs: EnabledOutputs,
) -> output_state_logic::EnabledOutputs {
    match outputs {
        EnabledOutputs::None => output_state_logic::EnabledOutputs::None,
        EnabledOutputs::Only(OutputChannel::OutA) => {
            output_state_logic::EnabledOutputs::Only(output_state_logic::OutputSelector::OutA)
        }
        EnabledOutputs::Only(OutputChannel::OutB) => {
            output_state_logic::EnabledOutputs::Only(output_state_logic::OutputSelector::OutB)
        }
        EnabledOutputs::Both => output_state_logic::EnabledOutputs::Both,
    }
}

pub(super) const fn enabled_outputs_from_logic(
    outputs: output_state_logic::EnabledOutputs,
) -> EnabledOutputs {
    match outputs {
        output_state_logic::EnabledOutputs::None => EnabledOutputs::None,
        output_state_logic::EnabledOutputs::Only(output_state_logic::OutputSelector::OutA) => {
            EnabledOutputs::Only(OutputChannel::OutA)
        }
        output_state_logic::EnabledOutputs::Only(output_state_logic::OutputSelector::OutB) => {
            EnabledOutputs::Only(OutputChannel::OutB)
        }
        output_state_logic::EnabledOutputs::Both => EnabledOutputs::Both,
    }
}

pub(super) const fn output_state_to_logic(
    state: OutputRuntimeState,
) -> output_state_logic::OutputRuntimeState {
    output_state_logic::OutputRuntimeState::new(
        logic_outputs_from_enabled(state.requested_outputs),
        logic_outputs_from_enabled(state.active_outputs),
        logic_outputs_from_enabled(state.recoverable_outputs),
        state.gate_reason,
    )
}

pub(super) const fn output_state_from_logic(
    state: output_state_logic::OutputRuntimeState,
) -> OutputRuntimeState {
    OutputRuntimeState::new(
        enabled_outputs_from_logic(state.requested_outputs),
        enabled_outputs_from_logic(state.active_outputs),
        enabled_outputs_from_logic(state.recoverable_outputs),
        state.gate_reason,
    )
}

#[derive(Clone, Copy)]
pub enum TelemetryValue {
    Value(i32),
    Err(&'static str),
}

impl defmt::Format for TelemetryValue {
    fn format(&self, fmt: defmt::Formatter) {
        match self {
            TelemetryValue::Value(v) => defmt::write!(fmt, "{}", v),
            TelemetryValue::Err(kind) => defmt::write!(fmt, "err({})", kind),
        }
    }
}

#[derive(Clone, Copy)]
pub enum TelemetryTempC {
    Value(i32), // temp_c_x16
    Err(&'static str),
}

impl defmt::Format for TelemetryTempC {
    fn format(&self, fmt: defmt::Formatter) {
        match self {
            TelemetryTempC::Value(temp_c_x16) => {
                let neg = *temp_c_x16 < 0;
                let abs = temp_c_x16.wrapping_abs() as u32;
                let int = abs / 16;
                let frac_4 = (abs % 16) * 625; // 1/16°C = 0.0625°C => 6250e-4

                if neg {
                    defmt::write!(fmt, "-{=u32}.{=u32:04}", int, frac_4);
                } else {
                    defmt::write!(fmt, "{=u32}.{=u32:04}", int, frac_4);
                }
            }
            TelemetryTempC::Err(kind) => defmt::write!(fmt, "err({})", kind),
        }
    }
}

#[derive(Clone, Copy)]
pub enum TelemetryU8 {
    Value(u8),
    Err(&'static str),
}

impl defmt::Format for TelemetryU8 {
    fn format(&self, fmt: defmt::Formatter) {
        match self {
            TelemetryU8::Value(v) => defmt::write!(fmt, "0x{=u8:x}", v),
            TelemetryU8::Err(kind) => defmt::write!(fmt, "err({})", kind),
        }
    }
}

#[derive(Clone, Copy)]
pub enum TelemetryU16 {
    Value(u16),
    Err(&'static str),
}

impl defmt::Format for TelemetryU16 {
    fn format(&self, fmt: defmt::Formatter) {
        match self {
            TelemetryU16::Value(v) => defmt::write!(fmt, "0x{=u16:x}", v),
            TelemetryU16::Err(kind) => defmt::write!(fmt, "err({})", kind),
        }
    }
}

#[derive(Clone, Copy)]
pub enum TelemetryBool {
    Value(bool),
    Err(&'static str),
}

impl defmt::Format for TelemetryBool {
    fn format(&self, fmt: defmt::Formatter) {
        match self {
            TelemetryBool::Value(v) => defmt::write!(fmt, "{}", v),
            TelemetryBool::Err(kind) => defmt::write!(fmt, "err({})", kind),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_input_sample_accepts_stable_positive_input() {
        let sample = normalize_charger_input_power_sample(true, true, Some(20_000), Some(1_500));

        assert_eq!(sample.issue, None);
        assert_eq!(sample.ui_vbus_mv, Some(20_000));
        assert_eq!(sample.ui_ibus_ma, Some(1_500));
        assert_eq!(sample.raw_power_w10, Some(300));
    }

    #[test]
    fn normalize_input_sample_returns_na_when_input_missing() {
        let sample = normalize_charger_input_power_sample(false, true, Some(20_000), Some(1_500));

        assert_eq!(sample.issue, None);
        assert_eq!(sample.ui_vbus_mv, None);
        assert_eq!(sample.ui_ibus_ma, None);
    }

    #[test]
    fn normalize_input_sample_clamps_reverse_current_to_zero() {
        let sample = normalize_charger_input_power_sample(true, true, Some(20_000), Some(-1_500));

        assert_eq!(sample.issue, None);
        assert_eq!(sample.ui_vbus_mv, Some(20_000));
        assert_eq!(sample.ui_ibus_ma, Some(0));
        assert_eq!(sample.raw_power_w10, Some(300));
    }

    #[test]
    fn normalize_input_sample_rejects_out_of_range_current() {
        let sample = normalize_charger_input_power_sample(true, true, Some(20_000), Some(i16::MIN));

        assert_eq!(sample.issue, Some(ChargerInputSampleIssue::IbusOutOfRange));
        assert_eq!(sample.ui_vbus_mv, None);
        assert_eq!(sample.ui_ibus_ma, None);
        assert!(sample.raw_power_w10.unwrap_or(0) > CHARGER_INPUT_POWER_ANOMALY_W10);
    }

    #[test]
    fn normalize_input_sample_rejects_not_ready_adc() {
        let sample = normalize_charger_input_power_sample(true, false, Some(20_000), Some(1_500));

        assert_eq!(sample.issue, Some(ChargerInputSampleIssue::AdcNotReady));
        assert_eq!(sample.ui_vbus_mv, None);
        assert_eq!(sample.ui_ibus_ma, None);
    }

    #[test]
    fn detail_input_source_prefers_explicit_usb_and_dc_routes() {
        assert_eq!(
            detail_input_source(true, true, false),
            Some(DashboardInputSource::UsbC)
        );
        assert_eq!(
            detail_input_source(true, false, true),
            Some(DashboardInputSource::DcIn)
        );
        assert_eq!(
            detail_input_source(true, true, true),
            Some(DashboardInputSource::Auto)
        );
        assert_eq!(detail_input_source(false, false, false), None);
    }

    #[test]
    fn manual_stop_hold_blocks_only_plain_charge_policy() {
        assert!(manual_charge_stop_hold_blocks_charge(true, false, false));
    }

    #[test]
    fn manual_stop_hold_does_not_block_activation_sequences() {
        assert!(!manual_charge_stop_hold_blocks_charge(true, true, false));
    }

    #[test]
    fn manual_stop_hold_does_not_block_explicit_activation_force_charge() {
        assert!(!manual_charge_stop_hold_blocks_charge(true, false, true));
    }

    #[test]
    fn manual_stop_hold_still_blocks_boot_auto_force_charge() {
        assert!(manual_charge_stop_hold_blocks_charge(true, false, false));
    }

    #[test]
    fn manual_charge_derate_only_applies_to_above_100ma_profiles() {
        assert!(!manual_charge_speed_derated(ManualChargeSpeed::Ma100, true));
        assert!(!manual_charge_speed_derated(
            ManualChargeSpeed::Ma500,
            false
        ));
        assert!(!manual_charge_speed_derated(
            ManualChargeSpeed::Ma1000,
            false
        ));
        assert!(manual_charge_speed_derated(ManualChargeSpeed::Ma500, true));
        assert!(manual_charge_speed_derated(ManualChargeSpeed::Ma1000, true));
    }

    #[test]
    fn manual_charge_safety_notice_persists_only_while_blocked() {
        assert!(manual_charge_safety_notice_active(
            ManualChargeStopReason::SafetyBlocked,
            false,
            false,
            true
        ));
        assert!(!manual_charge_safety_notice_active(
            ManualChargeStopReason::SafetyBlocked,
            true,
            false,
            true
        ));
        assert!(!manual_charge_safety_notice_active(
            ManualChargeStopReason::SafetyBlocked,
            false,
            true,
            true
        ));
        assert!(!manual_charge_safety_notice_active(
            ManualChargeStopReason::SafetyBlocked,
            false,
            false,
            false
        ));
        assert!(!manual_charge_safety_notice_active(
            ManualChargeStopReason::UserStop,
            false,
            false,
            true
        ));
    }

    #[test]
    fn usb_pd_restore_vindpm_tracks_bq25792_por_detection_margin() {
        assert_eq!(usb_pd_restore_vindpm_mv(Some(5_000)), 4_300);
        assert_eq!(usb_pd_restore_vindpm_mv(Some(20_000)), 18_600);
    }

    #[test]
    fn usb_pd_restore_vindpm_defaults_to_bq25792_minimum_without_sample() {
        assert_eq!(usb_pd_restore_vindpm_mv(None), 3_600);
    }

    #[test]
    fn usb_pd_measured_input_voltage_only_tracks_usbc_vac1_path() {
        assert_eq!(
            usb_pd_measured_input_voltage_mv(Some(true), Some(20_100)),
            Some(20_100)
        );
        assert_eq!(
            usb_pd_measured_input_voltage_mv(Some(false), Some(24_000)),
            None
        );
        assert_eq!(usb_pd_measured_input_voltage_mv(None, Some(24_000)), None);
    }

    #[test]
    fn usb_pd_vbus_present_stays_scoped_to_usbc_path() {
        assert_eq!(usb_pd_vbus_present(None, false), Some(false));
        assert_eq!(usb_pd_vbus_present(None, true), Some(true));
        assert_eq!(usb_pd_vbus_present(Some(true), false), Some(true));
    }

    #[test]
    fn usb_pd_charging_enabled_prefers_runtime_allow_charge() {
        assert!(!usb_pd_charging_enabled(Some(false), true, true));
        assert!(usb_pd_charging_enabled(Some(true), false, false));
        assert!(usb_pd_charging_enabled(None, true, true));
        assert!(!usb_pd_charging_enabled(None, true, false));
    }

    #[test]
    fn usb_pd_charge_gate_only_blocks_live_usbc_transients() {
        assert!(!usb_pd_charge_gate_ready(true, true, false));
        assert!(usb_pd_charge_gate_ready(true, true, true));
        assert!(usb_pd_charge_gate_ready(true, false, false));
        assert!(usb_pd_charge_gate_ready(false, true, false));
    }

    #[test]
    fn usb_pd_runtime_unsafe_source_latch_uses_live_usbc_vac1_sample() {
        assert!(usb_pd_runtime_unsafe_source_latched(
            false,
            true,
            Some(20_600)
        ));
        assert!(!usb_pd_runtime_unsafe_source_latched(
            false,
            false,
            Some(24_000)
        ));
        assert!(usb_pd_runtime_unsafe_source_latched(true, false, None));
    }

    #[test]
    fn usb_pd_input_limit_update_keeps_contract_limits_in_activation_paths() {
        assert_eq!(
            usb_pd_input_limit_update(true, false, true, false, true),
            UsbPdInputLimitUpdate::ApplyContract
        );
        assert_eq!(
            usb_pd_input_limit_update(true, false, false, true, false),
            UsbPdInputLimitUpdate::ApplyContract
        );
    }

    #[test]
    fn usb_pd_input_limit_update_defers_restore_until_activation_exits() {
        assert_eq!(
            usb_pd_input_limit_update(false, true, true, false, true),
            UsbPdInputLimitUpdate::None
        );
        assert_eq!(
            usb_pd_input_limit_update(false, true, false, false, false),
            UsbPdInputLimitUpdate::RestorePrevious
        );
    }

    #[test]
    fn usb_pd_restore_tracking_arms_restore_when_contract_drops_on_detach() {
        assert_eq!(
            usb_pd_restore_tracking_update(true, false, false, true),
            UsbPdRestoreTrackingUpdate::ArmRestore
        );
        assert_eq!(
            usb_pd_restore_tracking_update(false, false, false, false),
            UsbPdRestoreTrackingUpdate::None
        );
    }

    #[test]
    fn usb_pd_restore_tracking_only_arms_restore_while_attached() {
        assert_eq!(
            usb_pd_restore_tracking_update(true, false, true, true),
            UsbPdRestoreTrackingUpdate::ArmRestore
        );
        assert_eq!(
            usb_pd_restore_tracking_update(false, true, true, false),
            UsbPdRestoreTrackingUpdate::ClearRestorePending
        );
        assert_eq!(
            usb_pd_restore_tracking_update(false, false, true, false),
            UsbPdRestoreTrackingUpdate::None
        );
    }

    #[test]
    fn usb_pd_effective_input_current_limit_preserves_activation_throttle() {
        assert_eq!(
            usb_pd_effective_input_current_limit_ma(Some(2_000), Some(500)),
            Some(500)
        );
        assert_eq!(
            usb_pd_effective_input_current_limit_ma(Some(300), Some(500)),
            Some(300)
        );
        assert_eq!(
            usb_pd_effective_input_current_limit_ma(Some(2_000), None),
            Some(2_000)
        );
    }

    #[test]
    fn charge_policy_output_enabled_prefers_runtime_active_outputs() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.tps_a_enabled = Some(false);
        snapshot.tps_b_enabled = Some(false);

        assert!(charge_policy_output_enabled(
            &snapshot,
            EnabledOutputs::Only(OutputChannel::OutA)
        ));
        assert!(!charge_policy_output_enabled(
            &snapshot,
            EnabledOutputs::None
        ));

        snapshot.tps_b_enabled = Some(true);
        assert!(charge_policy_output_enabled(
            &snapshot,
            EnabledOutputs::None
        ));
    }

    #[test]
    fn charge_policy_output_power_uses_runtime_enabled_source() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.tps_a_enabled = Some(false);
        snapshot.tps_b_enabled = Some(false);

        assert_eq!(
            charge_policy_output_power_w10(&snapshot, EnabledOutputs::Only(OutputChannel::OutA)),
            None
        );
        assert_eq!(
            charge_policy_output_power_w10(&snapshot, EnabledOutputs::None),
            Some(0)
        );

        snapshot.out_a_vbus_mv = Some(20_000);
        snapshot.tps_a_iout_ma = Some(420);
        assert_eq!(
            charge_policy_output_power_w10(&snapshot, EnabledOutputs::Only(OutputChannel::OutA)),
            Some(84)
        );
    }

    #[test]
    fn detail_charger_status_maps_runtime_states_to_short_tokens() {
        assert_eq!(
            detail_charger_status_text(ChargePolicyState::BlockedNoInput),
            "NOAC"
        );
        assert_eq!(
            detail_charger_status_text(ChargePolicyState::BlockedTemp),
            "TEMP"
        );
        assert_eq!(
            detail_charger_status_text(ChargePolicyState::BlockedOutputOverload),
            "LOAD"
        );
        assert_eq!(
            detail_charger_status_text(ChargePolicyState::BlockedNoBms),
            "LOCK"
        );
        assert_eq!(
            detail_charger_status_text(ChargePolicyState::IdleWaitThreshold),
            "WAIT"
        );
        assert_eq!(
            detail_charger_status_text(ChargePolicyState::Charging500mA),
            "CHG500"
        );
        assert_eq!(
            detail_charger_status_text(ChargePolicyState::Charging100mADcDerated),
            "CHG100"
        );
        assert_eq!(
            detail_charger_status_text(ChargePolicyState::FullLatched),
            "FULL"
        );
    }

    #[test]
    fn charger_detail_status_preserves_fault_chip() {
        assert_eq!(charger_detail_status_text(true, false, "TEMP"), "FAULT");
        assert_eq!(charger_detail_status_text(false, true, "CHG500"), "WARM");
    }

    #[test]
    fn charger_home_status_keeps_runtime_temp_token_under_warn() {
        assert_eq!(
            charger_home_status_text(false, false, false, false, "TEMP"),
            "TEMP"
        );
        assert_eq!(
            charger_home_status_text(true, true, false, false, "CHG500"),
            "TEMP"
        );
        assert_eq!(
            charger_home_status_text(true, false, false, false, "CHG500"),
            "LOCK"
        );
        assert_eq!(
            charger_home_status_text(false, false, false, true, "CHG500"),
            "WARM"
        );
    }

    fn policy_input(
        telemetry: Option<ChargePolicyTelemetry>,
        input_source: Option<DashboardInputSource>,
        ibus_ma: Option<i32>,
    ) -> ChargePolicyInput {
        ChargePolicyInput {
            input_present: true,
            vbat_present: true,
            ts_cold: false,
            ts_hot: false,
            input_source,
            ibus_ma,
            output_enabled: false,
            output_power_w10: Some(0),
            telemetry,
            charger_done: false,
        }
    }

    fn policy_telemetry(rsoc_pct: u16, cell_min_mv: u16) -> ChargePolicyTelemetry {
        ChargePolicyTelemetry {
            rsoc_pct,
            cell_min_mv,
            charge_ready: true,
            bms_full: false,
        }
    }

    #[test]
    fn charge_policy_starts_when_rsoc_is_below_threshold() {
        let mut memory = ChargePolicyMemory::default();
        let mut derate = ChargePolicyDerateTracker::default();
        let mut output_load = ChargePolicyOutputLoadTracker::default();

        let decision = charge_policy_step(
            &mut memory,
            &mut derate,
            &mut output_load,
            0,
            policy_input(
                Some(policy_telemetry(79, CHARGE_POLICY_START_CELL_MIN_MV)),
                Some(DashboardInputSource::UsbC),
                Some(1_000),
            ),
        );

        assert_eq!(decision.state, ChargePolicyState::Charging500mA);
        assert!(decision.allow_charge);
        assert_eq!(decision.target_ichg_ma, Some(CHARGE_POLICY_NORMAL_ICHG_MA));
        assert_eq!(decision.start_reason, Some(ChargeStartReason::RsocLow));
        assert!(memory.charge_latched);
        assert!(!memory.full_latched);
    }

    #[test]
    fn charge_policy_starts_when_cell_voltage_is_below_threshold() {
        let mut memory = ChargePolicyMemory::default();
        let mut derate = ChargePolicyDerateTracker::default();
        let mut output_load = ChargePolicyOutputLoadTracker::default();

        let decision = charge_policy_step(
            &mut memory,
            &mut derate,
            &mut output_load,
            0,
            policy_input(
                Some(policy_telemetry(90, 3_650)),
                Some(DashboardInputSource::UsbC),
                Some(1_000),
            ),
        );

        assert_eq!(decision.state, ChargePolicyState::Charging500mA);
        assert_eq!(decision.start_reason, Some(ChargeStartReason::CellLow));
        assert!(memory.charge_latched);
    }

    #[test]
    fn charge_policy_waits_when_thresholds_are_not_crossed() {
        let mut memory = ChargePolicyMemory::default();
        let mut derate = ChargePolicyDerateTracker::default();
        let mut output_load = ChargePolicyOutputLoadTracker::default();

        let decision = charge_policy_step(
            &mut memory,
            &mut derate,
            &mut output_load,
            0,
            policy_input(
                Some(policy_telemetry(95, 3_900)),
                Some(DashboardInputSource::UsbC),
                Some(0),
            ),
        );

        assert_eq!(decision.state, ChargePolicyState::IdleWaitThreshold);
        assert!(!decision.allow_charge);
        assert!(!memory.charge_latched);
    }

    #[test]
    fn charge_policy_full_latches_until_threshold_drop() {
        let mut memory = ChargePolicyMemory {
            charge_latched: true,
            full_latched: false,
        };
        let mut derate = ChargePolicyDerateTracker::default();
        let mut output_load = ChargePolicyOutputLoadTracker::default();

        let first = charge_policy_step(
            &mut memory,
            &mut derate,
            &mut output_load,
            0,
            policy_input(
                Some(policy_telemetry(95, 4_050)),
                Some(DashboardInputSource::UsbC),
                Some(200),
            ),
        );
        assert_eq!(first.state, ChargePolicyState::Charging500mA);

        let hold = charge_policy_step(
            &mut memory,
            &mut derate,
            &mut output_load,
            100,
            policy_input(
                Some(policy_telemetry(95, 4_050)),
                Some(DashboardInputSource::UsbC),
                Some(200),
            ),
        );
        assert_eq!(hold.state, ChargePolicyState::Charging500mA);

        let full = charge_policy_step(
            &mut memory,
            &mut derate,
            &mut output_load,
            200,
            ChargePolicyInput {
                charger_done: true,
                ..policy_input(
                    Some(policy_telemetry(95, 4_050)),
                    Some(DashboardInputSource::UsbC),
                    Some(200),
                )
            },
        );
        assert_eq!(full.state, ChargePolicyState::FullLatched);
        assert_eq!(full.full_reason, Some(ChargeFullReason::ChargerTermination));
        assert!(!memory.charge_latched);
        assert!(memory.full_latched);
    }

    #[test]
    fn charge_policy_full_latch_requires_threshold_drop_to_restart() {
        let mut memory = ChargePolicyMemory {
            charge_latched: false,
            full_latched: true,
        };
        let mut derate = ChargePolicyDerateTracker::default();
        let mut output_load = ChargePolicyOutputLoadTracker::default();

        let still_full = charge_policy_step(
            &mut memory,
            &mut derate,
            &mut output_load,
            0,
            policy_input(
                Some(policy_telemetry(90, 3_950)),
                Some(DashboardInputSource::UsbC),
                Some(0),
            ),
        );
        assert_eq!(still_full.state, ChargePolicyState::FullLatched);

        let restart = charge_policy_step(
            &mut memory,
            &mut derate,
            &mut output_load,
            100,
            policy_input(
                Some(policy_telemetry(79, 3_950)),
                Some(DashboardInputSource::UsbC),
                Some(900),
            ),
        );
        assert_eq!(restart.state, ChargePolicyState::Charging500mA);
        assert!(memory.charge_latched);
        assert!(!memory.full_latched);
    }

    #[test]
    fn charge_policy_derates_only_for_dc_source_after_hold() {
        let mut memory = ChargePolicyMemory {
            charge_latched: true,
            full_latched: false,
        };
        let mut derate = ChargePolicyDerateTracker::default();
        let mut output_load = ChargePolicyOutputLoadTracker::default();
        let input = policy_input(
            Some(policy_telemetry(79, 3_850)),
            Some(DashboardInputSource::DcIn),
            Some(3_200),
        );

        let before_hold =
            charge_policy_step(&mut memory, &mut derate, &mut output_load, 900, input);
        assert_eq!(before_hold.state, ChargePolicyState::Charging500mA);

        let after_hold =
            charge_policy_step(&mut memory, &mut derate, &mut output_load, 1_900, input);
        assert_eq!(after_hold.state, ChargePolicyState::Charging100mADcDerated);
        assert_eq!(
            after_hold.target_ichg_ma,
            Some(CHARGE_POLICY_DC_DERATED_ICHG_MA)
        );
    }

    #[test]
    fn charge_policy_recovers_from_dc_derate_after_hold() {
        let mut memory = ChargePolicyMemory {
            charge_latched: true,
            full_latched: false,
        };
        let mut derate = ChargePolicyDerateTracker {
            derated: true,
            over_limit_since_ms: None,
            recover_since_ms: None,
        };
        let mut output_load = ChargePolicyOutputLoadTracker::default();
        let input = policy_input(
            Some(policy_telemetry(79, 3_850)),
            Some(DashboardInputSource::DcIn),
            Some(2_600),
        );

        let before_recover =
            charge_policy_step(&mut memory, &mut derate, &mut output_load, 4_900, input);
        assert_eq!(
            before_recover.state,
            ChargePolicyState::Charging100mADcDerated
        );

        let after_recover =
            charge_policy_step(&mut memory, &mut derate, &mut output_load, 9_950, input);
        assert_eq!(after_recover.state, ChargePolicyState::Charging500mA);
        assert!(!derate.derated);
    }

    #[test]
    fn charge_policy_does_not_derate_when_input_source_is_auto() {
        let mut memory = ChargePolicyMemory {
            charge_latched: true,
            full_latched: false,
        };
        let mut derate = ChargePolicyDerateTracker::default();
        let mut output_load = ChargePolicyOutputLoadTracker::default();

        let decision = charge_policy_step(
            &mut memory,
            &mut derate,
            &mut output_load,
            5_000,
            policy_input(
                Some(policy_telemetry(79, 3_850)),
                Some(DashboardInputSource::Auto),
                Some(3_500),
            ),
        );

        assert_eq!(decision.state, ChargePolicyState::Charging500mA);
        assert!(!derate.derated);
    }

    #[test]
    fn charge_policy_blocks_when_bms_telemetry_is_missing() {
        let mut memory = ChargePolicyMemory {
            charge_latched: true,
            full_latched: false,
        };
        let mut derate = ChargePolicyDerateTracker {
            derated: true,
            over_limit_since_ms: Some(0),
            recover_since_ms: None,
        };
        let mut output_load = ChargePolicyOutputLoadTracker {
            blocked: true,
            enter_streak: 0,
            exit_streak: 1,
        };

        let decision = charge_policy_step(
            &mut memory,
            &mut derate,
            &mut output_load,
            0,
            policy_input(None, Some(DashboardInputSource::UsbC), Some(1_000)),
        );

        assert_eq!(decision.state, ChargePolicyState::BlockedNoBms);
        assert!(!memory.charge_latched);
        assert!(!derate.derated);
        assert_eq!(output_load, ChargePolicyOutputLoadTracker::default());
    }

    #[test]
    fn charge_policy_requires_two_high_samples_before_blocking_output_power() {
        let mut memory = ChargePolicyMemory {
            charge_latched: true,
            full_latched: false,
        };
        let mut derate = ChargePolicyDerateTracker::default();
        let mut output_load = ChargePolicyOutputLoadTracker::default();

        let first = charge_policy_step(
            &mut memory,
            &mut derate,
            &mut output_load,
            0,
            ChargePolicyInput {
                output_enabled: true,
                output_power_w10: Some(CHARGE_POLICY_OUTPUT_POWER_LIMIT_W10 + 1),
                ..policy_input(
                    Some(policy_telemetry(79, 3_850)),
                    Some(DashboardInputSource::DcIn),
                    Some(1_000),
                )
            },
        );

        assert_eq!(first.state, ChargePolicyState::Charging500mA);
        assert!(memory.charge_latched);

        let second = charge_policy_step(
            &mut memory,
            &mut derate,
            &mut output_load,
            1_000,
            ChargePolicyInput {
                output_enabled: true,
                output_power_w10: Some(CHARGE_POLICY_OUTPUT_POWER_LIMIT_W10 + 1),
                ..policy_input(
                    Some(policy_telemetry(79, 3_850)),
                    Some(DashboardInputSource::DcIn),
                    Some(1_000),
                )
            },
        );

        assert_eq!(second.state, ChargePolicyState::BlockedOutputOverload);
        assert_eq!(
            second.output_block_reason,
            Some(ChargePolicyOutputBlockReason::OverLimit)
        );
        assert!(!memory.charge_latched);
    }

    #[test]
    fn charge_policy_recovers_output_block_after_three_low_samples() {
        let mut memory = ChargePolicyMemory {
            charge_latched: true,
            full_latched: false,
        };
        let mut derate = ChargePolicyDerateTracker::default();
        let mut output_load = ChargePolicyOutputLoadTracker::default();
        let mut high_input = policy_input(
            Some(policy_telemetry(79, 3_850)),
            Some(DashboardInputSource::DcIn),
            Some(1_000),
        );
        high_input.output_enabled = true;
        high_input.output_power_w10 = Some(CHARGE_POLICY_OUTPUT_POWER_LIMIT_W10 + 1);

        let _ = charge_policy_step(&mut memory, &mut derate, &mut output_load, 0, high_input);
        let blocked = charge_policy_step(
            &mut memory,
            &mut derate,
            &mut output_load,
            1_000,
            high_input,
        );
        assert_eq!(blocked.state, ChargePolicyState::BlockedOutputOverload);

        let mut low_input = high_input;
        low_input.output_power_w10 = Some(CHARGE_POLICY_OUTPUT_POWER_RESUME_W10 - 1);
        let low_1 =
            charge_policy_step(&mut memory, &mut derate, &mut output_load, 2_000, low_input);
        let low_2 =
            charge_policy_step(&mut memory, &mut derate, &mut output_load, 3_000, low_input);
        let low_3 =
            charge_policy_step(&mut memory, &mut derate, &mut output_load, 4_000, low_input);

        assert_eq!(low_1.state, ChargePolicyState::BlockedOutputOverload);
        assert_eq!(low_2.state, ChargePolicyState::BlockedOutputOverload);
        assert_eq!(low_3.state, ChargePolicyState::Charging500mA);
    }

    #[test]
    fn charge_policy_unknown_output_power_preserves_existing_load_block() {
        let mut memory = ChargePolicyMemory {
            charge_latched: true,
            full_latched: false,
        };
        let mut derate = ChargePolicyDerateTracker::default();
        let mut output_load = ChargePolicyOutputLoadTracker {
            blocked: true,
            enter_streak: 0,
            exit_streak: 2,
        };

        let unknown = charge_policy_step(
            &mut memory,
            &mut derate,
            &mut output_load,
            0,
            ChargePolicyInput {
                output_enabled: true,
                output_power_w10: None,
                ..policy_input(
                    Some(policy_telemetry(79, 3_850)),
                    Some(DashboardInputSource::UsbC),
                    Some(1_000),
                )
            },
        );
        assert_eq!(unknown.state, ChargePolicyState::BlockedOutputOverload);
        assert_eq!(
            unknown.output_block_reason,
            Some(ChargePolicyOutputBlockReason::PowerUnknown)
        );
        assert!(output_load.blocked);
        assert_eq!(output_load.exit_streak, 0);

        let mut low_input = policy_input(
            Some(policy_telemetry(79, 3_850)),
            Some(DashboardInputSource::UsbC),
            Some(1_000),
        );
        low_input.output_enabled = true;
        low_input.output_power_w10 = Some(CHARGE_POLICY_OUTPUT_POWER_RESUME_W10 - 1);

        let low_1 =
            charge_policy_step(&mut memory, &mut derate, &mut output_load, 1_000, low_input);
        let low_2 =
            charge_policy_step(&mut memory, &mut derate, &mut output_load, 2_000, low_input);
        let low_3 =
            charge_policy_step(&mut memory, &mut derate, &mut output_load, 3_000, low_input);

        assert_eq!(low_1.state, ChargePolicyState::BlockedOutputOverload);
        assert_eq!(low_2.state, ChargePolicyState::BlockedOutputOverload);
        assert_eq!(low_3.state, ChargePolicyState::Charging500mA);
    }

    #[test]
    fn charge_policy_blocks_conservatively_when_output_power_is_unknown() {
        let mut memory = ChargePolicyMemory {
            charge_latched: true,
            full_latched: false,
        };
        let mut derate = ChargePolicyDerateTracker::default();
        let mut output_load = ChargePolicyOutputLoadTracker::default();

        let decision = charge_policy_step(
            &mut memory,
            &mut derate,
            &mut output_load,
            0,
            ChargePolicyInput {
                output_enabled: true,
                output_power_w10: None,
                ..policy_input(
                    Some(policy_telemetry(79, 3_850)),
                    Some(DashboardInputSource::UsbC),
                    Some(1_000),
                )
            },
        );

        assert_eq!(decision.state, ChargePolicyState::BlockedOutputOverload);
        assert_eq!(
            decision.output_block_reason,
            Some(ChargePolicyOutputBlockReason::PowerUnknown)
        );
        assert!(!memory.charge_latched);
        assert!(output_load.blocked);
        assert_eq!(output_load.exit_streak, 0);
    }

    #[test]
    fn charge_policy_ignores_unknown_output_power_when_outputs_are_disabled() {
        let mut memory = ChargePolicyMemory {
            charge_latched: true,
            full_latched: false,
        };
        let mut derate = ChargePolicyDerateTracker::default();
        let mut output_load = ChargePolicyOutputLoadTracker::default();

        let decision = charge_policy_step(
            &mut memory,
            &mut derate,
            &mut output_load,
            0,
            ChargePolicyInput {
                output_enabled: false,
                output_power_w10: None,
                ..policy_input(
                    Some(policy_telemetry(79, 3_850)),
                    Some(DashboardInputSource::UsbC),
                    Some(1_000),
                )
            },
        );

        assert_eq!(decision.state, ChargePolicyState::Charging500mA);
    }

    #[test]
    fn charge_policy_resets_output_load_when_no_input_or_temp_blocks() {
        let mut memory = ChargePolicyMemory {
            charge_latched: true,
            full_latched: false,
        };
        let mut derate = ChargePolicyDerateTracker::default();
        let mut output_load = ChargePolicyOutputLoadTracker {
            blocked: true,
            enter_streak: 1,
            exit_streak: 2,
        };

        let no_input = charge_policy_step(
            &mut memory,
            &mut derate,
            &mut output_load,
            0,
            ChargePolicyInput {
                input_present: false,
                ..policy_input(
                    Some(policy_telemetry(79, 3_850)),
                    Some(DashboardInputSource::UsbC),
                    Some(1_000),
                )
            },
        );
        assert_eq!(no_input.state, ChargePolicyState::BlockedNoInput);
        assert_eq!(output_load, ChargePolicyOutputLoadTracker::default());

        output_load = ChargePolicyOutputLoadTracker {
            blocked: true,
            enter_streak: 1,
            exit_streak: 2,
        };
        let temp_block = charge_policy_step(
            &mut memory,
            &mut derate,
            &mut output_load,
            100,
            ChargePolicyInput {
                ts_hot: true,
                ..policy_input(
                    Some(policy_telemetry(79, 3_850)),
                    Some(DashboardInputSource::UsbC),
                    Some(1_000),
                )
            },
        );
        assert_eq!(temp_block.state, ChargePolicyState::BlockedTemp);
        assert_eq!(output_load, ChargePolicyOutputLoadTracker::default());
    }

    #[test]
    fn detail_bms_balance_mask_requires_active_cb_flag() {
        let base = Bq40z50Snapshot {
            battery_mode: 0,
            temp_k_x10: 2981,
            vpack_mv: 15_200,
            current_ma: 1200,
            rsoc_pct: 67,
            remcap: 0,
            fcc: 0,
            batt_status: 0,
            op_status: Some(0),
            da_status2: None,
            filter_capacity: None,
            balance_config: None,
            afe_register: None,
            cell_mv: [4100, 4098, 4102, 4099],
        };

        assert_eq!(detail_bms_balance_mask(&base), Some(0));

        let active = Bq40z50Snapshot {
            op_status: Some(bq40z50::operation_status::CB),
            ..base
        };
        assert_eq!(detail_bms_balance_mask(&active), None);
    }

    #[test]
    fn bq40_protection_active_ignores_alarm_only_bits() {
        assert!(!bq40_protection_active(
            bq40z50::battery_status::TCA,
            Some(0),
        ));
        assert!(!bq40_protection_active(
            bq40z50::battery_status::OCA | bq40z50::battery_status::OTA,
            Some(0),
        ));
    }

    #[test]
    fn bq40_protection_active_requires_pf_or_error_code() {
        assert!(bq40_protection_active(0x0001, Some(0)));
        assert!(bq40_protection_active(
            0,
            Some(bq40z50::operation_status::PF),
        ));
    }

    #[test]
    fn detail_bms_single_balance_cell_only_accepts_one_hot_masks() {
        assert_eq!(detail_bms_single_balance_cell(Some(0b0001)), Some(1));
        assert_eq!(detail_bms_single_balance_cell(Some(0b0100)), Some(3));
        assert_eq!(detail_bms_single_balance_cell(Some(0b0110)), None);
        assert_eq!(detail_bms_single_balance_cell(Some(0)), None);
        assert_eq!(detail_bms_single_balance_cell(None), None);
    }

    #[test]
    fn detail_bms_balance_mask_does_not_guess_live_cell_from_historical_timer_data() {
        let snapshot = Bq40z50Snapshot {
            battery_mode: 0,
            temp_k_x10: 2981,
            vpack_mv: 15_200,
            current_ma: 1200,
            rsoc_pct: 67,
            remcap: 0,
            fcc: 0,
            batt_status: 0,
            op_status: Some(bq40z50::operation_status::CB),
            da_status2: None,
            filter_capacity: None,
            balance_config: None,
            afe_register: None,
            cell_mv: [4100, 4098, 4102, 4099],
        };

        assert_eq!(detail_bms_balance_mask(&snapshot), None);
        assert_eq!(
            detail_bms_single_balance_cell(detail_bms_balance_mask(&snapshot)),
            None
        );
    }

    #[test]
    fn detail_bms_temps_use_da_status2_sensor_mapping() {
        let snapshot = Bq40z50Snapshot {
            battery_mode: 0,
            temp_k_x10: 3331,
            vpack_mv: 15_200,
            current_ma: 1200,
            rsoc_pct: 67,
            remcap: 0,
            fcc: 0,
            batt_status: 0,
            op_status: Some(0),
            da_status2: Some(bq40z50::DaStatus2 {
                int_temp_k_x10: 3051,
                ts_temp_k_x10: [3081, 3091, 3101, 3111],
                cell_temp_k_x10: 3121,
                fet_temp_k_x10: 3131,
                gauging_temp_k_x10: 3141,
            }),
            filter_capacity: None,
            balance_config: None,
            afe_register: None,
            cell_mv: [4100, 4098, 4102, 4099],
        };

        assert_eq!(
            detail_bms_cell_sensor_temps(&snapshot),
            [Some(35), Some(36), Some(37), Some(38)]
        );
        assert_eq!(detail_bms_board_temp_c(&snapshot), Some(35));
        assert_eq!(detail_battery_temp_c(&snapshot), Some(39));
    }

    #[test]
    fn detail_battery_temp_falls_back_to_temperature_word_without_da_status2() {
        let snapshot = Bq40z50Snapshot {
            battery_mode: 0,
            temp_k_x10: 3061,
            vpack_mv: 15_200,
            current_ma: 1200,
            rsoc_pct: 67,
            remcap: 0,
            fcc: 0,
            batt_status: 0,
            op_status: Some(0),
            da_status2: None,
            filter_capacity: None,
            balance_config: None,
            afe_register: None,
            cell_mv: [4100, 4098, 4102, 4099],
        };

        assert_eq!(detail_battery_temp_c(&snapshot), Some(33));
    }

    #[test]
    fn detail_bms_energy_prefers_filter_capacity_energy_when_capm_is_clear() {
        let snapshot = Bq40z50Snapshot {
            battery_mode: 0,
            temp_k_x10: 3061,
            vpack_mv: 15_200,
            current_ma: 1200,
            rsoc_pct: 67,
            remcap: 4321,
            fcc: 8765,
            batt_status: 0,
            op_status: Some(0),
            da_status2: None,
            filter_capacity: Some(bq40z50::FilterCapacity {
                remaining_capacity_mah: 4000,
                remaining_energy_cwh: 4685,
                full_charge_capacity_mah: 5000,
                full_charge_energy_cwh: 6320,
            }),
            balance_config: None,
            afe_register: None,
            cell_mv: [4100, 4098, 4102, 4099],
        };

        assert_eq!(detail_bms_energy_mwh(&snapshot), Some(46_850));
        assert_eq!(detail_bms_full_capacity_mwh(&snapshot), Some(63_200));
    }

    #[test]
    fn detail_bms_energy_uses_sbs_energy_units_when_capm_is_set() {
        let snapshot = Bq40z50Snapshot {
            battery_mode: bq40z50::battery_mode::CAPM,
            temp_k_x10: 3061,
            vpack_mv: 15_200,
            current_ma: 1200,
            rsoc_pct: 67,
            remcap: 4685,
            fcc: 6320,
            batt_status: 0,
            op_status: Some(0),
            da_status2: None,
            filter_capacity: Some(bq40z50::FilterCapacity {
                remaining_capacity_mah: 0,
                remaining_energy_cwh: 1,
                full_charge_capacity_mah: 0,
                full_charge_energy_cwh: 1,
            }),
            balance_config: None,
            afe_register: None,
            cell_mv: [4100, 4098, 4102, 4099],
        };

        assert_eq!(detail_bms_energy_mwh(&snapshot), Some(46_850));
        assert_eq!(detail_bms_full_capacity_mwh(&snapshot), Some(63_200));
    }

    #[test]
    fn detail_bms_energy_falls_back_when_filter_capacity_reports_invalid_sentinel() {
        let snapshot = Bq40z50Snapshot {
            battery_mode: 0,
            temp_k_x10: 3061,
            vpack_mv: 16_727,
            current_ma: 0,
            rsoc_pct: 100,
            remcap: 3917,
            fcc: 3917,
            batt_status: 0,
            op_status: Some(0),
            da_status2: None,
            filter_capacity: Some(bq40z50::FilterCapacity {
                remaining_capacity_mah: 3917,
                remaining_energy_cwh: u16::MAX,
                full_charge_capacity_mah: 3917,
                full_charge_energy_cwh: u16::MAX,
            }),
            balance_config: None,
            afe_register: None,
            cell_mv: [4184, 4188, 4149, 4157],
        };

        assert_eq!(detail_bms_energy_mwh(&snapshot), Some(65_519));
        assert_eq!(detail_bms_full_capacity_mwh(&snapshot), Some(65_519));
    }

    #[test]
    fn detail_bms_balance_mask_prefers_afe_cell_balance_status() {
        let snapshot = Bq40z50Snapshot {
            battery_mode: 0,
            temp_k_x10: 2981,
            vpack_mv: 15_200,
            current_ma: 1200,
            rsoc_pct: 67,
            remcap: 0,
            fcc: 0,
            batt_status: 0,
            op_status: Some(bq40z50::operation_status::CB),
            da_status2: None,
            filter_capacity: None,
            balance_config: None,
            afe_register: Some(bq40z50::AfeRegister {
                cell_balance_status: 0b0101,
            }),
            cell_mv: [4100, 4098, 4102, 4099],
        };

        assert_eq!(detail_bms_balance_mask(&snapshot), Some(0b0101));
        assert_eq!(
            detail_bms_single_balance_cell(detail_bms_balance_mask(&snapshot)),
            None
        );
    }

    #[test]
    fn fan_rpm_tracker_uses_two_pulses_per_rev() {
        let mut tracker = FanRpmTracker::new();
        let cfg = fan::Config {
            stop_temp_c_x16: 37 * 16,
            target_temp_c_x16: 40 * 16,
            min_run_pwm_pct: 10,
            step_down_pwm_pct: 5,
            step_up_small_delta_c_x16: 1 * 16,
            step_up_medium_delta_c_x16: 3 * 16,
            step_up_small_pwm_pct: 5,
            step_up_medium_pwm_pct: 10,
            step_up_large_pwm_pct: 15,
            control_interval_ms: 500,
            tach_timeout_ms: 2_000,
            tach_pulses_per_rev: 2,
            tach_watchdog_enabled: true,
        };
        let status = fan::Status {
            requested_command: fan::FanLevel::High,
            requested_pwm_pct: 100,
            command: fan::FanLevel::High,
            pwm_pct: 100,
            temp_source: fan::TempSource::Max,
            control_temp_c_x16: Some(55 * 16),
            tach_fault: false,
            tach_pulse_seen_recently: true,
        };

        assert_eq!(tracker.observe(0, 0, status, cfg), None);
        assert_eq!(tracker.observe(1_200, 40, status, cfg), Some(1000));
        assert_eq!(tracker.raw_rpm(), Some(1000));
    }

    #[test]
    fn fan_rpm_tracker_clears_when_fan_turns_off() {
        let mut tracker = FanRpmTracker::new();
        let cfg = fan::Config {
            stop_temp_c_x16: 37 * 16,
            target_temp_c_x16: 40 * 16,
            min_run_pwm_pct: 10,
            step_down_pwm_pct: 5,
            step_up_small_delta_c_x16: 1 * 16,
            step_up_medium_delta_c_x16: 3 * 16,
            step_up_small_pwm_pct: 5,
            step_up_medium_pwm_pct: 10,
            step_up_large_pwm_pct: 15,
            control_interval_ms: 500,
            tach_timeout_ms: 2_000,
            tach_pulses_per_rev: 2,
            tach_watchdog_enabled: true,
        };
        let running = fan::Status {
            requested_command: fan::FanLevel::Mid,
            requested_pwm_pct: 52,
            command: fan::FanLevel::Mid,
            pwm_pct: 52,
            temp_source: fan::TempSource::Max,
            control_temp_c_x16: Some(45 * 16),
            tach_fault: false,
            tach_pulse_seen_recently: true,
        };
        let off = fan::Status {
            requested_command: fan::FanLevel::Off,
            requested_pwm_pct: 0,
            command: fan::FanLevel::Off,
            pwm_pct: 0,
            temp_source: fan::TempSource::Max,
            control_temp_c_x16: Some(35 * 16),
            tach_fault: false,
            tach_pulse_seen_recently: false,
        };

        assert_eq!(tracker.observe(0, 0, running, cfg), None);
        assert_eq!(tracker.observe(1_200, 20, running, cfg), Some(500));
        assert_eq!(tracker.observe(1_500, 0, off, cfg), None);
        assert_eq!(tracker.display_rpm(), None);
        assert_eq!(tracker.raw_rpm(), None);
    }

    #[test]
    fn detail_fan_status_uses_applied_state_bands() {
        let low = AppliedFanState {
            command: fan::FanLevel::Low,
            pwm_pct: 25,
            vset_duty_pct: 75,
            degraded: false,
            disabled_by_feature: false,
        };
        let off = AppliedFanState {
            command: fan::FanLevel::Off,
            pwm_pct: 0,
            vset_duty_pct: 0,
            degraded: false,
            disabled_by_feature: true,
        };

        assert_eq!(detail_fan_status_text(low, false), "LOW");
        assert_eq!(detail_fan_status_text(off, false), "OFF");
        assert_eq!(detail_fan_status_text(low, true), "FAULT");
    }

    #[test]
    fn thermal_notice_prefers_therm_kill_over_test_mode() {
        assert_eq!(thermal_notice_text(false, false), "LIVE DATA");
        assert_eq!(thermal_notice_text(false, true), "TMP HW PROTECT TEST MODE");
        assert_eq!(thermal_notice_text(true, true), "THERM KILL ASSERTED");
    }

    #[test]
    fn charger_warm_status_overrides_policy_without_escalating_to_fault() {
        assert_eq!(charger_detail_status_text(false, true, "CHG500"), "WARM");
        assert_eq!(
            charger_detail_notice_text(false, true, "charging_500ma"),
            "BQ25792 TS WARM - FAN FORCED HIGH"
        );
    }

    #[test]
    fn charger_audio_thermal_stress_ignores_ts_warm_only() {
        assert!(!charger_audio_thermal_stress(false, false));
        assert!(charger_audio_thermal_stress(true, false));
        assert!(charger_audio_thermal_stress(false, true));
    }

    #[test]
    fn accumulate_protection_temp_disables_thermal_branch_in_test_mode() {
        assert_eq!(max_optional_temp(None, Some(45 * 16)), Some(45 * 16));
        assert_eq!(max_optional_temp(Some(41 * 16), None), Some(41 * 16));
        assert_eq!(
            max_optional_temp(Some(41 * 16), Some(45 * 16)),
            Some(45 * 16)
        );
    }

    #[test]
    fn bms_thermal_max_uses_highest_available_detail_sensor() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Backup);
        snapshot.dashboard_detail.board_temp_c = Some(35);
        snapshot.dashboard_detail.battery_temp_c = Some(41);
        snapshot.dashboard_detail.cell_temp_c = [Some(39), Some(44), None, Some(42)];

        assert_eq!(bms_thermal_max_c_x16(&snapshot), Some(44 * 16));
    }

    #[test]
    fn fan_rpm_tracker_uses_longer_window_and_smoothing() {
        let mut tracker = FanRpmTracker::new();
        let cfg = fan::Config {
            stop_temp_c_x16: 37 * 16,
            target_temp_c_x16: 40 * 16,
            min_run_pwm_pct: 10,
            step_down_pwm_pct: 5,
            step_up_small_delta_c_x16: 1 * 16,
            step_up_medium_delta_c_x16: 3 * 16,
            step_up_small_pwm_pct: 5,
            step_up_medium_pwm_pct: 10,
            step_up_large_pwm_pct: 15,
            control_interval_ms: 500,
            tach_timeout_ms: 2_000,
            tach_pulses_per_rev: 2,
            tach_watchdog_enabled: true,
        };
        let status = fan::Status {
            requested_command: fan::FanLevel::High,
            requested_pwm_pct: 100,
            command: fan::FanLevel::High,
            pwm_pct: 100,
            temp_source: fan::TempSource::Max,
            control_temp_c_x16: Some(55 * 16),
            tach_fault: false,
            tach_pulse_seen_recently: true,
        };

        assert_eq!(tracker.observe(0, 0, status, cfg), None);
        assert_eq!(tracker.observe(800, 40, status, cfg), None);
        assert_eq!(tracker.observe(1_200, 20, status, cfg), Some(1_500));
        assert_eq!(tracker.raw_rpm(), Some(1_500));
        assert_eq!(tracker.observe(2_400, 100, status, cfg), Some(1_833));
        assert_eq!(tracker.raw_rpm(), Some(2_500));
    }

    #[test]
    fn mains_present_from_vin_uses_dc5025_threshold_only() {
        assert_eq!(mains_present_from_vin(None), None);
        assert_eq!(mains_present_from_vin(Some(2_999)), Some(false));
        assert_eq!(mains_present_from_vin(Some(3_000)), Some(true));
    }

    #[test]
    fn stable_mains_present_prefers_fresh_vin_and_keeps_last_known_good() {
        assert_eq!(stable_mains_present(None, None, None), None);
        assert_eq!(stable_mains_present(None, None, Some(true)), Some(true));
        assert_eq!(
            stable_mains_present(Some(true), None, Some(false)),
            Some(true)
        );
        assert_eq!(
            stable_mains_present(Some(false), None, Some(true)),
            Some(false)
        );
        assert_eq!(
            stable_mains_present(Some(true), Some(2_900), Some(true)),
            Some(false)
        );
        assert_eq!(
            stable_mains_present(Some(false), Some(19_200), Some(false)),
            Some(true)
        );
    }

    #[test]
    fn stable_mains_state_tracks_when_audio_is_using_charger_fallback() {
        assert_eq!(
            stable_mains_state(None, None, Some(false)),
            StableMainsState {
                present: Some(false),
                source: AudioMainsSource::ChargerFallback,
            }
        );
        assert_eq!(
            stable_mains_state(Some(true), None, Some(false)),
            StableMainsState {
                present: Some(true),
                source: AudioMainsSource::Vin,
            }
        );
        assert_eq!(
            stable_mains_state(Some(false), Some(19_200), Some(false)),
            StableMainsState {
                present: Some(true),
                source: AudioMainsSource::Vin,
            }
        );
    }

    #[test]
    fn discharge_authorization_input_ready_accepts_charger_presence_fallback() {
        assert!(!discharge_authorization_input_ready(None, None));
        assert!(!discharge_authorization_input_ready(
            Some(false),
            Some(false)
        ));
        assert!(discharge_authorization_input_ready(None, Some(true)));
        assert!(discharge_authorization_input_ready(Some(true), Some(false)));
        assert!(discharge_authorization_input_ready(Some(false), Some(true)));
    }

    #[test]
    fn mains_present_edge_only_silences_source_switches_without_state_change() {
        let vin_true = StableMainsState {
            present: Some(true),
            source: AudioMainsSource::Vin,
        };
        let vin_false = StableMainsState {
            present: Some(false),
            source: AudioMainsSource::Vin,
        };
        let charger_false = StableMainsState {
            present: Some(false),
            source: AudioMainsSource::ChargerFallback,
        };
        let charger_true = StableMainsState {
            present: Some(true),
            source: AudioMainsSource::ChargerFallback,
        };

        assert_eq!(mains_present_edge(vin_true, vin_false), Some(false));
        assert_eq!(mains_present_edge(charger_false, charger_true), Some(true));
        assert_eq!(mains_present_edge(vin_true, charger_false), Some(false));
        assert_eq!(mains_present_edge(charger_false, vin_true), Some(true));
        assert_eq!(
            mains_present_edge(
                StableMainsState {
                    present: Some(true),
                    source: AudioMainsSource::Vin,
                },
                StableMainsState {
                    present: Some(true),
                    source: AudioMainsSource::ChargerFallback,
                }
            ),
            None
        );
    }

    #[test]
    fn record_vin_sample_failure_expires_stale_latch_after_repeated_misses() {
        let mut mains_present = Some(true);
        let mut missing_streak = 0;

        record_vin_sample_failure(&mut mains_present, &mut missing_streak);
        assert_eq!(mains_present, Some(true));
        assert_eq!(missing_streak, 1);

        record_vin_sample_failure(&mut mains_present, &mut missing_streak);
        assert_eq!(mains_present, None);
        assert_eq!(missing_streak, VIN_MAINS_LATCH_FAILURE_LIMIT);
    }

    #[test]
    fn mark_vin_telemetry_unavailable_expires_stale_latch_after_repeated_skips() {
        let mut vin_vbus_mv = Some(19_200);
        let mut vin_iin_ma = Some(850);
        let mut mains_present = Some(true);
        let mut missing_streak = 0;

        mark_vin_telemetry_unavailable(
            true,
            &mut vin_vbus_mv,
            &mut vin_iin_ma,
            &mut mains_present,
            &mut missing_streak,
        );
        assert_eq!(vin_vbus_mv, None);
        assert_eq!(vin_iin_ma, None);
        assert_eq!(mains_present, Some(true));
        assert_eq!(missing_streak, 1);

        mark_vin_telemetry_unavailable(
            true,
            &mut vin_vbus_mv,
            &mut vin_iin_ma,
            &mut mains_present,
            &mut missing_streak,
        );
        assert_eq!(mains_present, None);
        assert_eq!(missing_streak, VIN_MAINS_LATCH_FAILURE_LIMIT);
    }

    #[test]
    fn mark_vin_telemetry_unavailable_clears_state_when_vin_channel_disabled() {
        let mut vin_vbus_mv = Some(19_200);
        let mut vin_iin_ma = Some(850);
        let mut mains_present = Some(true);
        let mut missing_streak = 1;

        mark_vin_telemetry_unavailable(
            false,
            &mut vin_vbus_mv,
            &mut vin_iin_ma,
            &mut mains_present,
            &mut missing_streak,
        );
        assert_eq!(vin_vbus_mv, None);
        assert_eq!(vin_iin_ma, None);
        assert_eq!(mains_present, None);
        assert_eq!(missing_streak, 0);
    }

    #[test]
    fn ups_mode_from_mains_prefers_vin_truth_source() {
        assert_eq!(ups_mode_from_mains(Some(true), false), UpsMode::Standby);
        assert_eq!(ups_mode_from_mains(Some(true), true), UpsMode::Supplement);
        assert_eq!(ups_mode_from_mains(Some(false), true), UpsMode::Backup);
        assert_eq!(ups_mode_from_mains(None, false), UpsMode::Standby);
        assert_eq!(ups_mode_from_mains(None, true), UpsMode::Backup);
    }
}
