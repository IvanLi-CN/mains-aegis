use crate::front_panel_scene::{
    is_bq40_activation_needed, BmsRecoveryUiAction, BmsResultKind, DashboardChargerProtocol,
    DashboardInputSource, ManualChargeSpeed, ManualChargeStopReason, SelfCheckCommState,
    SelfCheckUiSnapshot, UpsMode,
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
    record_vin_sample_failure, stable_mains_present, stable_mains_state, AudioBatteryLowState,
    AudioChargePhase, AudioMainsSource, Bq40z50Snapshot, ChargerInputPowerSample,
    ChargerInputSampleIssue, OutputRuntimeState, StableMainsState, CHARGER_INPUT_POWER_ANOMALY_W10,
};

const BMS_SELF_CHECK_AUTO_RECOVERY_ENABLED: bool = false;
const CHARGE_POLICY_NORMAL_ICHG_MA: u16 = 500;
const CHARGE_POLICY_TOPOFF_ICHG_MA: u16 = 200;
const CHARGE_POLICY_DC_DERATED_ICHG_MA: u16 = 100;
const CHARGE_POLICY_BMS_RECOVERY_ICHG_MA: u16 = 100;
const CHARGE_POLICY_START_RSOC_PCT: u16 = 80;
const CHARGE_POLICY_START_CELL_MIN_MV: u16 = 3_700;
const CHARGE_POLICY_TOPOFF_RSOC_PCT: u16 = 99;
const CHARGE_POLICY_TOPOFF_CELL_MAX_MV: u16 = 4140;
pub(super) const CHARGE_POLICY_LOW_VOLTAGE_RECOVERY_EXIT_CELL_MIN_MV: u16 = 3_000;
const CHARGE_POLICY_DC_DERATE_ENTER_IBUS_MA: i32 = 3_000;
const CHARGE_POLICY_DC_DERATE_EXIT_IBUS_MA: i32 = 2_700;
const CHARGE_POLICY_DC_DERATE_ENTER_HOLD: Duration = Duration::from_secs(1);
const CHARGE_POLICY_DC_DERATE_EXIT_HOLD: Duration = Duration::from_secs(5);
const CHARGE_POLICY_OUTPUT_POWER_LIMIT_W10: u32 = 50;
const CHARGE_POLICY_OUTPUT_POWER_RESUME_W10: u32 = 45;
const CHARGE_POLICY_OUTPUT_BLOCK_ENTER_POLLS: u8 = 2;
const CHARGE_POLICY_OUTPUT_BLOCK_EXIT_POLLS: u8 = 3;
pub(super) const BACKUP_USB_CHARGE_START_POWER_LIMIT_W10: u32 = 20;
pub(super) const BACKUP_USB_CHARGE_STOP_POWER_LIMIT_W10: u32 = 30;
pub(super) const BACKUP_USB_CHARGE_TELEMETRY_MISS_LIMIT: u8 = 2;
pub(super) const BACKUP_USB_AUTO_CHARGE_ICHG_MA: u16 = CHARGE_POLICY_NORMAL_ICHG_MA;
pub(super) const BACKUP_USB_AUTO_CHARGE_STATUS_TEXT: &str = "CHG500";
pub(super) const DCIN_ADAPTIVE_START_ICHG_MA: u16 = 100;
const DCIN_ADAPTIVE_STEP_UP_ICHG_MA: u16 = 100;
const DCIN_ADAPTIVE_STEP_DOWN_ICHG_MA: u16 = 200;
const DCIN_ADAPTIVE_RAMP_HOLD_MS: u64 = 3_000;
const DCIN_ADAPTIVE_RECOVERY_HOLD_MS: u64 = 10_000;
const DCIN_ADAPTIVE_COOLDOWN_MS: u64 = 30_000;
const DCIN_BASELINE_RESTORE_HOLD_MS: u64 = 20_000;
const DCIN_ADAPTIVE_VIN_DROP_STREAK_LIMIT: u8 = 2;
pub(super) const DCIN_TPS_OUTPUT_STOP_THRESHOLD_MA: i32 = 100;
pub(super) const ASSIST_RATED_VIN_DROP_RECOVER_DIVISOR: u16 = 2;
// Require DCIN current to be effectively at the 3A class source ceiling before
// allowing assist_low online takeover on 12V input.
const ASSIST_LOW_DCIN_ENTER_IIN_THRESHOLD_MA: i32 =
    CHARGE_POLICY_DC_DERATE_ENTER_IBUS_MA as i32 - 50;
pub(super) const ASSIST_LOW_STANDBY_ENTER_MARGIN_MV: u16 = 40;
pub(super) const ASSIST_LOW_STANDBY_EXIT_MARGIN_MV: u16 = 200;
const ASSIST_RATED_LOW_TARGET_ENTER_MARGIN_MV: u16 = 60;
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
    usb_pd_attached: bool,
    vbus_adc_mv: Option<u16>,
    vac1_adc_mv: Option<u16>,
    vac2_adc_mv: Option<u16>,
) -> Option<DashboardInputSource> {
    let dc_selected = ac2_present
        && (vac2_adc_mv.is_some_and(|vac2_mv| {
            vac2_mv >= 7_000
                && vac1_adc_mv
                    .map(|vac1_mv| vac2_mv > vac1_mv.saturating_add(1_000))
                    .unwrap_or(true)
        }) || vbus_adc_mv.is_some_and(|vbus_mv| {
            vbus_mv >= 7_000
                && vac1_adc_mv
                    .map(|vac1_mv| vbus_mv > vac1_mv.saturating_add(1_000))
                    .unwrap_or(true)
        }));

    if dc_selected || (ac2_present && !ac1_present && !usb_pd_attached) {
        Some(DashboardInputSource::DcIn)
    } else if usb_pd_attached || (ac1_present && !ac2_present) {
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

pub(super) fn charger_protocol_from_usb_pd(
    input_source: Option<DashboardInputSource>,
    state: usb_pd::UsbPdPortState,
) -> Option<DashboardChargerProtocol> {
    match input_source {
        Some(DashboardInputSource::DcIn) => Some(DashboardChargerProtocol::DcIn),
        Some(DashboardInputSource::UsbC) | Some(DashboardInputSource::Auto) => {
            if !state.attached {
                return Some(DashboardChargerProtocol::NoCc);
            }

            if let Some(contract) = state.contract {
                return Some(match contract.kind {
                    usb_pd::ContractKind::Pps => DashboardChargerProtocol::Pps,
                    usb_pd::ContractKind::Fixed if contract.voltage_mv <= 5_500 => {
                        DashboardChargerProtocol::Usb5V
                    }
                    usb_pd::ContractKind::Fixed => DashboardChargerProtocol::PdFixed,
                });
            }

            if matches!(state.vbus_present, Some(true)) {
                Some(DashboardChargerProtocol::SourceCapsUnknown)
            } else {
                Some(DashboardChargerProtocol::NoCc)
            }
        }
        None => None,
    }
}

pub(super) fn trusted_usb_pd_recovery_rsoc_pct(snapshot: &SelfCheckUiSnapshot) -> Option<u16> {
    if snapshot.bq40z50 == SelfCheckCommState::Ok
        && snapshot.bq40z50_no_battery == Some(false)
        && snapshot.bq40z50_discharge_ready == Some(true)
    {
        snapshot.bq40z50_soc_pct.filter(|pct| *pct <= 100)
    } else {
        None
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChargerDeliveryDiagKind {
    None,
    ChargeOverTarget,
    InputOverLimit,
    ChargeUnderTarget,
    ChargeUnderTargetInputDpm,
}

impl ChargerDeliveryDiagKind {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ChargeOverTarget => "charge_over_target",
            Self::InputOverLimit => "input_over_limit",
            Self::ChargeUnderTarget => "charge_under_target",
            Self::ChargeUnderTargetInputDpm => "charge_under_target_input_dpm",
        }
    }

    pub(super) const fn is_under_delivery(self) -> bool {
        matches!(
            self,
            Self::ChargeUnderTarget | Self::ChargeUnderTargetInputDpm
        )
    }
}

pub(super) fn charger_delivery_diag_kind(
    allow_charge: bool,
    under_delivery_watch: bool,
    target_ichg_ma: Option<u16>,
    applied_iindpm_ma: Option<u16>,
    actual_ibus_ma: Option<i16>,
    ibat_adc_ma: Option<i16>,
    bms_current_ma: Option<i16>,
    vindpm: bool,
    iindpm: bool,
    margin_ma: i16,
) -> ChargerDeliveryDiagKind {
    if !allow_charge {
        return ChargerDeliveryDiagKind::None;
    }

    let target_ichg_ma = match target_ichg_ma {
        Some(v) => i32::from(v),
        None => return ChargerDeliveryDiagKind::None,
    };
    let margin_ma = i32::from(margin_ma);
    let actual_charge_ma = ibat_adc_ma
        .map(|v| i32::from(v).max(0))
        .or_else(|| bms_current_ma.map(|v| i32::from(v).max(0)));
    let actual_ibus_ma = actual_ibus_ma.map(|v| i32::from(v.abs()));

    if actual_charge_ma
        .map(|v| v > target_ichg_ma + margin_ma)
        .unwrap_or(false)
    {
        return ChargerDeliveryDiagKind::ChargeOverTarget;
    }

    if matches!(
        (applied_iindpm_ma, actual_ibus_ma),
        (Some(limit_ma), Some(actual_ma)) if actual_ma > i32::from(limit_ma) + margin_ma
    ) {
        return ChargerDeliveryDiagKind::InputOverLimit;
    }

    if under_delivery_watch
        && actual_charge_ma
            .map(|v| target_ichg_ma > v + margin_ma)
            .unwrap_or(false)
    {
        if vindpm || iindpm {
            ChargerDeliveryDiagKind::ChargeUnderTargetInputDpm
        } else {
            ChargerDeliveryDiagKind::ChargeUnderTarget
        }
    } else {
        ChargerDeliveryDiagKind::None
    }
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

pub(super) fn dcin_target_vindpm_mv(
    measured_dcin_voltage_mv: Option<u16>,
    vin_baseline_mv: Option<u16>,
) -> u16 {
    let source_mv = measured_dcin_voltage_mv
        .or(vin_baseline_mv)
        .unwrap_or(12_000);
    let scaled = (u32::from(source_mv) * 96) / 100;
    scaled.clamp(3_600, 22_000) as u16
}

pub(super) fn usb_pd_measured_input_voltage_mv(
    usb_c_vbus_present: Option<bool>,
    vac1_adc_mv: Option<u16>,
) -> Option<u16> {
    matches!(usb_c_vbus_present, Some(true))
        .then_some(vac1_adc_mv)
        .flatten()
}

pub(super) fn requested_tps_total_iout_ma(
    requested_outputs: EnabledOutputs,
    out_a_iout_ma: Option<i32>,
    out_b_iout_ma: Option<i32>,
) -> Option<i32> {
    let out_a_iout_ma = requested_outputs
        .is_enabled(OutputChannel::OutA)
        .then_some(out_a_iout_ma)
        .flatten();
    let out_b_iout_ma = requested_outputs
        .is_enabled(OutputChannel::OutB)
        .then_some(out_b_iout_ma)
        .flatten();
    match (out_a_iout_ma, out_b_iout_ma) {
        (None, None) => None,
        (Some(a), Some(b)) => Some(a.saturating_add(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
    }
}

pub(super) fn usb_pd_vbus_present(
    pd_vbus_present: Option<bool>,
    usb_c_input_present: bool,
) -> Option<bool> {
    pd_vbus_present.or(Some(usb_c_input_present))
}

pub(super) const fn usb_pd_charging_enabled(
    runtime_allow_charge: Option<bool>,
    charger_enabled: bool,
    charger_allowed: bool,
) -> bool {
    if let Some(runtime_allow_charge) = runtime_allow_charge {
        runtime_allow_charge
    } else {
        charger_enabled && charger_allowed
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RuntimeChargeOverride {
    pub(super) allow_charge: bool,
    pub(super) policy_status_text: &'static str,
    pub(super) policy_notice_text: &'static str,
}

pub(super) fn runtime_charge_override(
    mode: UpsMode,
    backup_reason: Option<&'static str>,
) -> Option<RuntimeChargeOverride> {
    match mode {
        UpsMode::Supplement => Some(RuntimeChargeOverride {
            allow_charge: false,
            policy_status_text: "LOAD",
            policy_notice_text: "runtime_assist_no_charge",
        }),
        UpsMode::Backup if matches!(backup_reason, Some("source_limited")) => {
            Some(RuntimeChargeOverride {
                allow_charge: false,
                policy_status_text: "LOAD",
                policy_notice_text: "runtime_source_limited_backup_no_charge",
            })
        }
        UpsMode::Backup => Some(RuntimeChargeOverride {
            allow_charge: false,
            policy_status_text: "NOAC",
            policy_notice_text: "runtime_backup_no_charge",
        }),
        UpsMode::Blocked => Some(RuntimeChargeOverride {
            allow_charge: false,
            policy_status_text: "LOCK",
            policy_notice_text: "runtime_blocked_no_charge",
        }),
        UpsMode::Standby | UpsMode::Off => None,
    }
}

pub(super) fn runtime_charge_override_for_charger(
    mode: UpsMode,
    backup_reason: Option<&'static str>,
    force_allow_charge: bool,
    auto_force_charge: bool,
) -> Option<RuntimeChargeOverride> {
    if force_allow_charge || auto_force_charge {
        None
    } else {
        runtime_charge_override(mode, backup_reason)
    }
}

pub(super) fn runtime_charge_override_for_backup_usb_charger(
    mode: UpsMode,
    backup_reason: Option<&'static str>,
    force_allow_charge: bool,
    auto_force_charge: bool,
    backup_usb_charge_allowed: bool,
) -> Option<RuntimeChargeOverride> {
    if mode == UpsMode::Backup
        && matches!(backup_reason, Some("input_absent"))
        && backup_usb_charge_allowed
    {
        None
    } else {
        runtime_charge_override_for_charger(
            mode,
            backup_reason,
            force_allow_charge,
            auto_force_charge,
        )
    }
}

pub(super) const BMS_NO_BATTERY_VPACK_MAX_MV: u16 = 2_500;

pub(super) fn bq40_pack_indicates_no_battery(vpack_mv: u16) -> bool {
    vpack_mv < BMS_NO_BATTERY_VPACK_MAX_MV
}

pub(super) fn bq40_physical_discharge_path_absent(
    pack_mv: Option<u16>,
    discharge_ready: Option<bool>,
    charger_vbat_present: Option<bool>,
) -> bool {
    discharge_ready == Some(true)
        && charger_vbat_present == Some(false)
        && pack_mv
            .map(|mv| !bq40_pack_indicates_no_battery(mv))
            .unwrap_or(false)
}

pub(super) fn bq40_physical_discharge_path_issue(
    charge_ready: Option<bool>,
    charge_reason: Option<&'static str>,
    discharge_ready: Option<bool>,
) -> &'static str {
    if charge_ready == Some(false) {
        charge_reason.unwrap_or("charge_path_blocked")
    } else if discharge_ready == Some(true) {
        "pack_output_path_open"
    } else {
        "physical_vbat_absent"
    }
}

pub(super) fn bq25792_effective_vbat_present(
    status_vbat_present: Option<bool>,
    vbat_adc_mv: Option<u16>,
) -> Option<bool> {
    match (status_vbat_present, vbat_adc_mv) {
        (Some(true), _) => Some(true),
        (_, Some(mv)) if !bq40_pack_indicates_no_battery(mv) => Some(true),
        (Some(false), _) => Some(false),
        (None, _) => None,
    }
}

pub(super) const fn usb_pd_demand_charging_enabled(
    runtime_allow_charge: Option<bool>,
    charger_enabled: bool,
    charger_allowed: bool,
    bms_charge_ready: Option<bool>,
) -> bool {
    !matches!(bms_charge_ready, Some(false))
        && usb_pd_charging_enabled(runtime_allow_charge, charger_enabled, charger_allowed)
}

pub(super) const fn usb_pd_charge_gate_ready(
    usb_pd_enabled: bool,
    usb_pd_controller_ready: bool,
    usb_c_path_present: bool,
    usb_pd_charge_ready: bool,
) -> bool {
    !usb_pd_enabled || !usb_c_path_present || !usb_pd_controller_ready || usb_pd_charge_ready
}

pub(super) const fn usb_pd_charge_gate_path_present(
    input_source: Option<DashboardInputSource>,
    usb_c_path_present: bool,
) -> bool {
    usb_c_path_present
        && matches!(
            input_source,
            Some(DashboardInputSource::UsbC | DashboardInputSource::Auto)
        )
}

pub(super) const fn charger_vbus_stat_allows_activation_charge(vbus_stat: u8) -> bool {
    matches!(vbus_stat & 0x0F, 0x1 | 0x2 | 0x3 | 0x4 | 0x5 | 0x6)
}

pub(super) fn usb_pd_runtime_unsafe_source_latched(
    previously_latched: bool,
    usb_c_path_present: bool,
    vac1_adc_mv: Option<u16>,
) -> bool {
    previously_latched
        || usb_pd::sink_policy::is_input_voltage_unsafe(
            usb_c_path_present.then_some(vac1_adc_mv).flatten(),
        )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum UsbPdInputLimitUpdate {
    None,
    ApplyContract,
    RestorePrevious,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum UsbPdRestoreTrackingUpdate {
    None,
    ArmRestore,
    ClearRestorePending,
}

pub(super) const fn usb_pd_input_limit_update(
    pd_limits_present: bool,
    restore_pending: bool,
    force_allow_charge: bool,
    auto_force_charge: bool,
    activation_pending: bool,
) -> UsbPdInputLimitUpdate {
    if pd_limits_present {
        UsbPdInputLimitUpdate::ApplyContract
    } else if restore_pending {
        let _ = force_allow_charge;
        let _ = auto_force_charge;
        let _ = activation_pending;
        UsbPdInputLimitUpdate::RestorePrevious
    } else {
        UsbPdInputLimitUpdate::None
    }
}

pub(super) const fn usb_pd_restore_tracking_update(
    previous_pd_limits_present: bool,
    pd_limits_present: bool,
    attached: bool,
    backup_present: bool,
) -> UsbPdRestoreTrackingUpdate {
    if previous_pd_limits_present && !pd_limits_present && backup_present {
        UsbPdRestoreTrackingUpdate::ArmRestore
    } else if pd_limits_present {
        UsbPdRestoreTrackingUpdate::ClearRestorePending
    } else {
        let _ = attached;
        UsbPdRestoreTrackingUpdate::None
    }
}

pub(super) const fn usb_pd_effective_input_current_limit_ma(
    contract_iindpm_ma: Option<u16>,
    activation_iindpm_cap_ma: Option<u16>,
) -> Option<u16> {
    match (contract_iindpm_ma, activation_iindpm_cap_ma) {
        (Some(contract_iindpm_ma), Some(cap_ma)) => Some(if contract_iindpm_ma < cap_ma {
            contract_iindpm_ma
        } else {
            cap_ma
        }),
        (Some(contract_iindpm_ma), None) => Some(contract_iindpm_ma),
        (None, _) => None,
    }
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
    ChargingTopoff200mA,
    Charging100mADcDerated,
    RecoveringLowVoltage,
    FullLatched,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChargePolicyRecoveryStage {
    Bq40Pchg,
    Bq25792Precharge,
}

impl ChargePolicyRecoveryStage {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Bq40Pchg => "bq40_pchg",
            Self::Bq25792Precharge => "bq25792_precharge",
        }
    }
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
            Self::ChargingTopoff200mA => "charging_topoff_200ma",
            Self::Charging100mADcDerated => "charging_100ma_dc_derated",
            Self::RecoveringLowVoltage => "recovering_low_voltage",
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
            Self::ChargingTopoff200mA => "CHG500",
            Self::Charging100mADcDerated => "CHG100",
            Self::RecoveringLowVoltage => "RECOV",
            Self::FullLatched => "FULL",
        }
    }

    pub(super) const fn charger_active(self) -> bool {
        matches!(
            self,
            Self::Charging500mA
                | Self::ChargingTopoff200mA
                | Self::Charging100mADcDerated
                | Self::RecoveringLowVoltage
        )
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

fn bq40_optional_bit(raw: Option<u32>, mask: u32) -> Option<bool> {
    raw.map(|value| (value & mask) != 0)
}

pub(super) fn bms_recovery_charge_allowed_from_diag(
    no_battery: Option<bool>,
    op_status: Option<u32>,
    safety_status: Option<u32>,
    pf_status: Option<u32>,
    charging_status: Option<u32>,
    cell_min_mv: Option<u16>,
) -> bool {
    let low_voltage_evidence = bq40_optional_bit(safety_status, bq40z50::safety_status::CUV)
        == Some(true)
        || cell_min_mv.is_some_and(|mv| mv < CHARGE_POLICY_LOW_VOLTAGE_RECOVERY_EXIT_CELL_MIN_MV);

    no_battery == Some(false)
        && bq40_op_bit(op_status, bq40z50::operation_status::PCHG) == Some(true)
        && bq40_op_bit(op_status, bq40z50::operation_status::XCHG) != Some(true)
        && low_voltage_evidence
        && bq40_optional_bit(safety_status, bq40z50::safety_status::CUVC) != Some(true)
        && pf_status.unwrap_or(0) == 0
        && bq40_optional_bit(charging_status, bq40z50::charging_status::IN) != Some(true)
        && bq40_optional_bit(charging_status, bq40z50::charging_status::SU) != Some(true)
}

pub(super) fn bms_discharge_authorization_needs_pack_path_recovery(
    charger_vbat_present: Option<bool>,
    afe_dsg_fet: Option<bool>,
    op_dsg_fet: Option<bool>,
) -> bool {
    charger_vbat_present == Some(false)
        && match afe_dsg_fet {
            Some(actual) => actual,
            None => op_dsg_fet == Some(true),
        }
}

pub(super) fn bms_discharge_authorization_needs_bq40_fet_state_reset(
    charger_vbat_present: Option<bool>,
    afe_chg_fet: Option<bool>,
    afe_chg_control: Option<bool>,
    afe_dsg_fet: Option<bool>,
    op_chg_fet: Option<bool>,
    op_dsg_fet: Option<bool>,
    xchg: Option<bool>,
    xdsg: Option<bool>,
    safety_status: Option<u32>,
    pf_status: Option<u32>,
    mfg_fet_en: Option<bool>,
) -> bool {
    charger_vbat_present == Some(false)
        && mfg_fet_en == Some(true)
        && safety_status == Some(0)
        && pf_status == Some(0)
        && xchg == Some(false)
        && xdsg == Some(false)
        && op_chg_fet == Some(true)
        && op_dsg_fet == Some(true)
        && (afe_dsg_fet != Some(false))
        && (afe_chg_fet != Some(true) || afe_chg_control != Some(true))
}

pub(super) fn bms_discharge_authorization_next_after_emshut_exit(
    emshut: Option<bool>,
    charger_vbat_present: Option<bool>,
    afe_chg_fet: Option<bool>,
    afe_chg_control: Option<bool>,
    afe_dsg_fet: Option<bool>,
    op_chg_fet: Option<bool>,
    op_dsg_fet: Option<bool>,
    xchg: Option<bool>,
    xdsg: Option<bool>,
    safety_status: Option<u32>,
    pf_status: Option<u32>,
    mfg_fet_en: Option<bool>,
) -> Option<&'static str> {
    if emshut != Some(false) {
        return None;
    }

    if bms_discharge_authorization_needs_bq40_fet_state_reset(
        charger_vbat_present,
        afe_chg_fet,
        afe_chg_control,
        afe_dsg_fet,
        op_chg_fet,
        op_dsg_fet,
        xchg,
        xdsg,
        safety_status,
        pf_status,
        mfg_fet_en,
    ) {
        Some("pack_output_path_reset_requested")
    } else if bms_discharge_authorization_needs_pack_path_recovery(
        charger_vbat_present,
        afe_dsg_fet,
        op_dsg_fet,
    ) {
        Some("pack_output_path_recovery_requested")
    } else {
        Some("ordinary_recovery_requested")
    }
}

pub(super) fn bms_discharge_authorization_reason_uses_activation(reason: &'static str) -> bool {
    matches!(
        reason,
        "ordinary_recovery_requested"
            | "pack_output_path_recovery_requested"
            | "pack_output_path_reset_requested"
    )
}

pub(super) fn bms_discharge_authorization_recovery_action(reason: &'static str) -> &'static str {
    match reason {
        "emshut_exit_sent" => "exit_emshut",
        "pack_output_path_reset_requested" => "bq40_device_reset_then_activation",
        "pack_output_path_recovery_requested" => "activation_min_charge",
        "ordinary_recovery_requested" => "activation",
        _ => "none",
    }
}

pub(super) fn bms_discharge_authorization_success_reason(reason: &'static str) -> &'static str {
    match reason {
        "emshut_exit_sent" => "emshut_exit_recovered",
        "pack_output_path_recovery_requested" => "pack_output_path_recovered",
        "pack_output_path_reset_requested" => "pack_output_path_reset_recovered",
        _ => "discharge_authorization_recovered",
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BackupUsbChargeBlockReason {
    OutputHigh,
    TelemetryLost,
}

impl BackupUsbChargeBlockReason {
    pub(super) const fn policy_status_text(self) -> &'static str {
        match self {
            Self::OutputHigh => "LOAD",
            Self::TelemetryLost => "LOCK",
        }
    }

    pub(super) const fn notice(self) -> &'static str {
        match self {
            Self::OutputHigh => "backup_usb_output_high_latched",
            Self::TelemetryLost => "backup_usb_telemetry_lost_latched",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BackupUsbChargeGuardDecision {
    NotApplicable,
    Allow,
    WaitingForLowOutput,
    Blocked(BackupUsbChargeBlockReason),
}

impl BackupUsbChargeGuardDecision {
    pub(super) const fn allows_charge(self) -> bool {
        matches!(self, Self::Allow)
    }

    pub(super) const fn notice(self) -> Option<&'static str> {
        match self {
            Self::WaitingForLowOutput => Some("backup_usb_wait_low_output"),
            Self::Blocked(reason) => Some(reason.notice()),
            Self::NotApplicable | Self::Allow => None,
        }
    }

    pub(super) const fn policy_status_text(self) -> Option<&'static str> {
        match self {
            Self::WaitingForLowOutput => Some("NOAC"),
            Self::Blocked(reason) => Some(reason.policy_status_text()),
            Self::NotApplicable | Self::Allow => None,
        }
    }
}

pub(super) const fn defer_output_power_unknown_block_for_backup_usb(
    backup_usb_charge_context: bool,
    manual_loopback_override: bool,
) -> bool {
    backup_usb_charge_context && !manual_loopback_override
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct BackupUsbChargeGuard {
    pub(super) admitted: bool,
    pub(super) latched_reason: Option<BackupUsbChargeBlockReason>,
    pub(super) telemetry_miss_streak: u8,
    pub(super) last_attempt_seq: Option<u32>,
}

impl BackupUsbChargeGuard {
    pub(super) fn reset_for_new_session(&mut self, last_attempt_seq: Option<u32>) {
        *self = Self {
            last_attempt_seq,
            ..Self::default()
        };
    }

    pub(super) fn observe(
        &mut self,
        applicable: bool,
        normal_charge_allowed: bool,
        output_enabled: bool,
        output_power_w10: Option<u32>,
        telemetry_attempt_seq: Option<u32>,
        manual_override: bool,
    ) -> BackupUsbChargeGuardDecision {
        if !applicable {
            self.admitted = false;
            self.telemetry_miss_streak = 0;
            self.last_attempt_seq = telemetry_attempt_seq;
            return BackupUsbChargeGuardDecision::NotApplicable;
        }

        if manual_override {
            self.admitted = false;
            self.telemetry_miss_streak = 0;
            self.last_attempt_seq = telemetry_attempt_seq;
            return BackupUsbChargeGuardDecision::Allow;
        }

        if let Some(reason) = self.latched_reason {
            return BackupUsbChargeGuardDecision::Blocked(reason);
        }

        if !output_enabled {
            self.admitted = true;
            self.telemetry_miss_streak = 0;
            self.last_attempt_seq = telemetry_attempt_seq;
            return BackupUsbChargeGuardDecision::Allow;
        }

        if !self.admitted {
            if !normal_charge_allowed {
                self.telemetry_miss_streak = 0;
                self.last_attempt_seq = telemetry_attempt_seq;
                return BackupUsbChargeGuardDecision::NotApplicable;
            }
            let is_new_attempt =
                telemetry_attempt_seq.is_some() && telemetry_attempt_seq != self.last_attempt_seq;
            if !is_new_attempt {
                return BackupUsbChargeGuardDecision::WaitingForLowOutput;
            }
            let Some(output_power_w10) = output_power_w10 else {
                self.last_attempt_seq = telemetry_attempt_seq;
                return BackupUsbChargeGuardDecision::WaitingForLowOutput;
            };
            self.last_attempt_seq = telemetry_attempt_seq;
            if output_power_w10 < BACKUP_USB_CHARGE_START_POWER_LIMIT_W10 {
                self.admitted = true;
                self.telemetry_miss_streak = 0;
                BackupUsbChargeGuardDecision::Allow
            } else {
                BackupUsbChargeGuardDecision::WaitingForLowOutput
            }
        } else {
            let is_new_attempt =
                telemetry_attempt_seq.is_some() && telemetry_attempt_seq != self.last_attempt_seq;
            if is_new_attempt {
                self.last_attempt_seq = telemetry_attempt_seq;
                match output_power_w10 {
                    Some(power_w10) if power_w10 > BACKUP_USB_CHARGE_STOP_POWER_LIMIT_W10 => {
                        self.admitted = false;
                        self.latched_reason = Some(BackupUsbChargeBlockReason::OutputHigh);
                        return BackupUsbChargeGuardDecision::Blocked(
                            BackupUsbChargeBlockReason::OutputHigh,
                        );
                    }
                    Some(_) => self.telemetry_miss_streak = 0,
                    None => {
                        self.telemetry_miss_streak = self.telemetry_miss_streak.saturating_add(1);
                        if self.telemetry_miss_streak >= BACKUP_USB_CHARGE_TELEMETRY_MISS_LIMIT {
                            self.admitted = false;
                            self.latched_reason = Some(BackupUsbChargeBlockReason::TelemetryLost);
                            return BackupUsbChargeGuardDecision::Blocked(
                                BackupUsbChargeBlockReason::TelemetryLost,
                            );
                        }
                    }
                }
            }
            BackupUsbChargeGuardDecision::Allow
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
pub(super) enum DcinInputPressureState {
    Inactive,
    Headroom,
    Watch,
    Limited,
    Cooldown,
}

impl DcinInputPressureState {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Inactive => "inactive",
            Self::Headroom => "headroom",
            Self::Watch => "watch",
            Self::Limited => "limited",
            Self::Cooldown => "cooldown",
        }
    }
}

impl Default for DcinInputPressureState {
    fn default() -> Self {
        Self::Inactive
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DcinInputPressureReason {
    None,
    VinDropWatch,
    VinDrop,
    TpsOutputCurrent,
    Vindpm,
    Iindpm,
    Poorsrc,
    Cooldown,
}

impl DcinInputPressureReason {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::VinDropWatch => "vin_drop_watch",
            Self::VinDrop => "vin_drop",
            Self::TpsOutputCurrent => "tps_output_current",
            Self::Vindpm => "vindpm",
            Self::Iindpm => "iindpm",
            Self::Poorsrc => "poorsrc",
            Self::Cooldown => "cooldown",
        }
    }
}

impl Default for DcinInputPressureReason {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DcinChargeLimitReason {
    None,
    StartupRamp,
    RecoveryHold,
    PressureVinDrop,
    PressureTpsOutputCurrent,
    PressureVindpm,
    PressureIindpm,
    PressurePoorsrc,
    CooldownRetryWait,
}

impl DcinChargeLimitReason {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::StartupRamp => "startup_ramp",
            Self::RecoveryHold => "recovery_hold",
            Self::PressureVinDrop => "pressure_vin_drop",
            Self::PressureTpsOutputCurrent => "pressure_tps_output_current",
            Self::PressureVindpm => "pressure_vindpm",
            Self::PressureIindpm => "pressure_iindpm",
            Self::PressurePoorsrc => "pressure_poorsrc",
            Self::CooldownRetryWait => "cooldown_retry_wait",
        }
    }
}

impl Default for DcinChargeLimitReason {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct DcinInputPressureTracker {
    pub(super) state: DcinInputPressureState,
    pub(super) reason: DcinInputPressureReason,
    pub(super) trigger_reason: DcinInputPressureReason,
    pub(super) limit_reason: DcinChargeLimitReason,
    pub(super) adaptive_cap_ichg_ma: Option<u16>,
    pub(super) vin_baseline_mv: Option<u16>,
    pub(super) vin_drop_mv: Option<u16>,
    pub(super) pressure_score_pct: u8,
    pub(super) vin_drop_streak: u8,
    pub(super) last_pressure_at_ms: Option<u64>,
    pub(super) last_ramp_at_ms: Option<u64>,
    pub(super) cooldown_until_ms: Option<u64>,
    pub(super) last_tps_total_iout_sample_seq: Option<u32>,
    pub(super) last_tps_total_iout_over_limit: Option<bool>,
    pub(super) dcin_absent_since_ms: Option<u64>,
}

impl DcinInputPressureTracker {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn has_recent_dcin_loss_for_restore(&self) -> bool {
        self.dcin_absent_since_ms.is_some()
    }

    pub(super) fn should_preserve_for_ac2_restore(&self, runtime_mode: UpsMode) -> bool {
        self.has_recent_dcin_loss_for_restore()
            || matches!(runtime_mode, UpsMode::Backup | UpsMode::Blocked)
    }

    pub(super) fn reset_for_online_restore(&mut self) {
        self.state = DcinInputPressureState::Inactive;
        self.reason = DcinInputPressureReason::None;
        self.trigger_reason = DcinInputPressureReason::None;
        self.limit_reason = DcinChargeLimitReason::None;
        self.adaptive_cap_ichg_ma = None;
        self.pressure_score_pct = 0;
        self.vin_drop_streak = 0;
        self.last_pressure_at_ms = None;
        self.last_ramp_at_ms = None;
        self.cooldown_until_ms = None;
        self.last_tps_total_iout_sample_seq = None;
        self.last_tps_total_iout_over_limit = None;
        self.dcin_absent_since_ms = None;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct DcinInputPressureInput {
    pub(super) input_source: Option<DashboardInputSource>,
    pub(super) dcin_present: bool,
    pub(super) requested_target_ichg_ma: Option<u16>,
    pub(super) allow_charge: bool,
    pub(super) vin_vbus_mv: Option<u16>,
    pub(super) vin_iin_ma: Option<i32>,
    pub(super) input_vbus_mv: Option<u16>,
    pub(super) tps_total_iout_ma: Option<i32>,
    pub(super) tps_total_iout_fresh: bool,
    pub(super) tps_total_iout_sample_seq: Option<u32>,
    pub(super) poorsrc: bool,
    pub(super) vindpm: bool,
    pub(super) iindpm: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct DcinInputPressureDecision {
    pub(super) pressure_state: DcinInputPressureState,
    pub(super) pressure_reason: DcinInputPressureReason,
    pub(super) trigger_reason: DcinInputPressureReason,
    pub(super) pressure_score_pct: u8,
    pub(super) vin_baseline_mv: Option<u16>,
    pub(super) vin_drop_mv: Option<u16>,
    pub(super) tps_total_iout_ma: Option<i32>,
    pub(super) tps_limit_threshold_ma: Option<i32>,
    pub(super) adaptive_cap_ichg_ma: Option<u16>,
    pub(super) effective_target_ichg_ma: Option<u16>,
    pub(super) allow_charge: bool,
    pub(super) limit_active: bool,
    pub(super) limit_reason: DcinChargeLimitReason,
}

impl DcinInputPressureDecision {
    pub(super) fn inactive(requested_target_ichg_ma: Option<u16>, allow_charge: bool) -> Self {
        Self {
            pressure_state: DcinInputPressureState::Inactive,
            pressure_reason: DcinInputPressureReason::None,
            trigger_reason: DcinInputPressureReason::None,
            pressure_score_pct: 0,
            vin_baseline_mv: None,
            vin_drop_mv: None,
            tps_total_iout_ma: None,
            tps_limit_threshold_ma: None,
            adaptive_cap_ichg_ma: None,
            effective_target_ichg_ma: if allow_charge {
                requested_target_ichg_ma
            } else {
                None
            },
            allow_charge,
            limit_active: false,
            limit_reason: DcinChargeLimitReason::None,
        }
    }

    pub(super) fn inactive_with_tracker(
        requested_target_ichg_ma: Option<u16>,
        allow_charge: bool,
        tracker: &DcinInputPressureTracker,
    ) -> Self {
        Self {
            pressure_state: DcinInputPressureState::Inactive,
            pressure_reason: DcinInputPressureReason::None,
            trigger_reason: DcinInputPressureReason::None,
            pressure_score_pct: 0,
            vin_baseline_mv: tracker.vin_baseline_mv,
            vin_drop_mv: tracker.vin_drop_mv,
            tps_total_iout_ma: None,
            tps_limit_threshold_ma: None,
            adaptive_cap_ichg_ma: tracker.adaptive_cap_ichg_ma,
            effective_target_ichg_ma: if allow_charge {
                requested_target_ichg_ma
            } else {
                None
            },
            allow_charge,
            limit_active: false,
            limit_reason: DcinChargeLimitReason::None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum AssistPowerStage {
    #[default]
    Standby,
    AssistLow,
    AssistRated,
    Backup,
}

impl AssistPowerStage {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Standby => "standby",
            Self::AssistLow => "assist_low",
            Self::AssistRated => "assist_rated",
            Self::Backup => "backup",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BackupReason {
    InputAbsent,
    SourceLimited,
}

impl BackupReason {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::InputAbsent => "input_absent",
            Self::SourceLimited => "source_limited",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct AssistPowerStageTracker {
    pub(super) stage: AssistPowerStage,
    pub(super) backup_reason: Option<BackupReason>,
    assist_enter_streak: u8,
    assist_exit_streak: u8,
    promote_streak: u8,
    recover_streak: u8,
    source_limited_enter_streak: u8,
    source_limited_recover_streak: u8,
    last_tps_total_iout_sample_seq: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct AssistPowerStageInput {
    pub(super) mains_present: Option<bool>,
    pub(super) input_source: Option<DashboardInputSource>,
    pub(super) dcin_assist_allowed: bool,
    pub(super) rated_vout_mv: u16,
    pub(super) standby_target_vout_mv: u16,
    pub(super) current_assist_target_vout_mv: u16,
    pub(super) assist_low_target_vout_mv: u16,
    pub(super) vin_baseline_mv: Option<u16>,
    pub(super) vin_drop_mv: Option<u16>,
    pub(super) vin_vbus_mv: Option<u16>,
    pub(super) vin_iin_ma: Option<i32>,
    pub(super) tps_total_iout_ma: Option<i32>,
    pub(super) tps_total_iout_fresh: bool,
    pub(super) tps_total_iout_sample_seq: Option<u32>,
    pub(super) assist_enter_iout_ma: i32,
    pub(super) assist_exit_iout_ma: i32,
    pub(super) assist_required_samples: u8,
    pub(super) rated_enter_iout_ma: i32,
    pub(super) rated_exit_iout_ma: i32,
    pub(super) vin_drop_threshold_pct: u16,
    pub(super) required_samples: u8,
    pub(super) source_limited_vin_drop_pct: u16,
    pub(super) source_limited_enter_iout_ma: i32,
    pub(super) source_limited_exit_iout_ma: i32,
    pub(super) source_limited_required_samples: u8,
    pub(super) source_limited_recover_margin_mv: u16,
}

impl AssistPowerStageTracker {
    pub(super) fn with_stage(stage: AssistPowerStage) -> Self {
        let mut tracker = Self::default();
        tracker.stage = stage;
        tracker
    }

    pub(super) fn reset_for_online(&mut self) {
        self.assist_enter_streak = 0;
        self.assist_exit_streak = 0;
        self.promote_streak = 0;
        self.recover_streak = 0;
        self.source_limited_enter_streak = 0;
        self.source_limited_recover_streak = 0;
    }
}

pub(super) fn assist_power_stage_step(
    tracker: &mut AssistPowerStageTracker,
    input: AssistPowerStageInput,
) -> AssistPowerStage {
    match input.mains_present {
        Some(false) => {
            tracker.stage = AssistPowerStage::Backup;
            tracker.backup_reason = Some(BackupReason::InputAbsent);
            tracker.reset_for_online();
            if input.tps_total_iout_fresh {
                tracker.last_tps_total_iout_sample_seq = input.tps_total_iout_sample_seq;
            }
            return tracker.stage;
        }
        Some(true) => {}
        None => return tracker.stage,
    }

    if tracker.stage == AssistPowerStage::Backup
        && tracker.backup_reason != Some(BackupReason::SourceLimited)
    {
        tracker.stage = AssistPowerStage::Standby;
        tracker.backup_reason = None;
        tracker.reset_for_online();
    }

    if !input.dcin_assist_allowed {
        tracker.stage = AssistPowerStage::Standby;
        tracker.backup_reason = None;
        tracker.reset_for_online();
        if input.tps_total_iout_fresh {
            tracker.last_tps_total_iout_sample_seq = input.tps_total_iout_sample_seq;
        }
        return tracker.stage;
    }

    let sample_is_new = input.tps_total_iout_fresh
        && input
            .tps_total_iout_sample_seq
            .is_some_and(|sample_seq| tracker.last_tps_total_iout_sample_seq != Some(sample_seq));

    let assist_low_vin_enter_mv = input
        .standby_target_vout_mv
        .saturating_add(ASSIST_LOW_STANDBY_ENTER_MARGIN_MV);
    let assist_low_vin_exit_mv = input
        .standby_target_vout_mv
        .saturating_add(ASSIST_LOW_STANDBY_EXIT_MARGIN_MV);

    let assist_low_meaningful_tps_iout_ma = input
        .assist_enter_iout_ma
        .max(DCIN_TPS_OUTPUT_STOP_THRESHOLD_MA * 2);
    let assist_source_stressed = matches!(
        (input.vin_vbus_mv, input.vin_iin_ma),
        (Some(vin_vbus_mv), Some(vin_iin_ma))
            if vin_vbus_mv <= assist_low_vin_enter_mv
                || vin_iin_ma >= ASSIST_LOW_DCIN_ENTER_IIN_THRESHOLD_MA
    );
    let assist_gate_ready = matches!(input.tps_total_iout_ma, Some(tps_total_iout_ma)
        if assist_source_stressed
            && tps_total_iout_ma >= assist_low_meaningful_tps_iout_ma);
    let assist_gate_recovered = matches!(
        (input.vin_vbus_mv, input.tps_total_iout_ma),
        (Some(vin_vbus_mv), Some(tps_total_iout_ma))
            if vin_vbus_mv >= assist_low_vin_exit_mv
                && tps_total_iout_ma <= input.assist_exit_iout_ma
    );

    let vin_drop_threshold_mv = input
        .vin_baseline_mv
        .map(|baseline_mv| dcin_vin_drop_threshold_mv(baseline_mv, input.vin_drop_threshold_pct));
    let vin_drop_recover_mv =
        vin_drop_threshold_mv.map(|threshold| threshold / ASSIST_RATED_VIN_DROP_RECOVER_DIVISOR);
    let source_limited_vin_drop_threshold_mv = input.vin_baseline_mv.map(|baseline_mv| {
        dcin_vin_drop_threshold_mv(baseline_mv, input.source_limited_vin_drop_pct)
    });
    let source_limited_vin_drop_mv = if sample_is_new {
        input.vin_drop_mv
    } else {
        input
            .vin_baseline_mv
            .zip(input.vin_vbus_mv)
            .map(|(baseline_mv, vin_vbus_mv)| baseline_mv.saturating_sub(vin_vbus_mv))
            .or(input.vin_drop_mv)
    };
    let source_limited_vin_drop_recover_mv = source_limited_vin_drop_threshold_mv
        .map(|threshold| threshold / ASSIST_RATED_VIN_DROP_RECOVER_DIVISOR);
    let source_limited_low_vin_mv = input
        .rated_vout_mv
        .saturating_sub(input.source_limited_recover_margin_mv);
    let source_limited_fast_enter_ready = !sample_is_new
        && matches!(
            (
                input.vin_vbus_mv,
                source_limited_vin_drop_mv,
                source_limited_vin_drop_threshold_mv,
                input.vin_iin_ma
            ),
            (Some(vin_vbus_mv), Some(vin_drop_mv), Some(threshold_mv), Some(vin_iin_ma))
                if vin_iin_ma >= input.source_limited_enter_iout_ma
                    && (vin_vbus_mv <= source_limited_low_vin_mv
                        || vin_drop_mv >= threshold_mv)
        );
    let source_limited_enter_ready = source_limited_fast_enter_ready
        || matches!(
            (
                input.vin_vbus_mv,
                input.vin_drop_mv,
                source_limited_vin_drop_threshold_mv,
                input.vin_iin_ma,
                input.tps_total_iout_ma
            ),
            (
                Some(vin_vbus_mv),
                Some(vin_drop_mv),
                Some(threshold_mv),
                Some(vin_iin_ma),
                Some(tps_total_iout_ma)
            ) if tps_total_iout_ma >= input.source_limited_enter_iout_ma
                && (vin_vbus_mv <= source_limited_low_vin_mv
                    || (vin_drop_mv > threshold_mv
                        && vin_iin_ma >= ASSIST_LOW_DCIN_ENTER_IIN_THRESHOLD_MA))
        );
    let source_limited_recover_ready = matches!(
        (
            input.vin_vbus_mv,
            source_limited_vin_drop_mv,
            source_limited_vin_drop_recover_mv,
            input.tps_total_iout_ma
        ),
        (
            Some(vin_vbus_mv),
            Some(vin_drop_mv),
            Some(recover_mv),
            Some(tps_total_iout_ma)
        ) if vin_vbus_mv > source_limited_low_vin_mv
            && vin_drop_mv <= recover_mv
            && tps_total_iout_ma <= input.source_limited_exit_iout_ma
    );
    if !sample_is_new && !source_limited_enter_ready {
        return tracker.stage;
    }
    if sample_is_new {
        tracker.last_tps_total_iout_sample_seq = input.tps_total_iout_sample_seq;
    }
    let assist_low_ramp_ready =
        input.current_assist_target_vout_mv >= input.assist_low_target_vout_mv;
    let assist_low_takeover_pinned = matches!(
        input.vin_vbus_mv,
        Some(vin_vbus_mv)
            if vin_vbus_mv
                <= input
                    .assist_low_target_vout_mv
                    .saturating_add(ASSIST_RATED_LOW_TARGET_ENTER_MARGIN_MV)
    );
    let promote_ready = matches!(
        (
            input.vin_drop_mv,
            vin_drop_threshold_mv,
            input.tps_total_iout_ma
        ),
        (Some(vin_drop_mv), Some(threshold_mv), Some(tps_total_iout_ma))
            if vin_drop_mv > threshold_mv && tps_total_iout_ma >= input.rated_enter_iout_ma
    ) && assist_low_ramp_ready
        && assist_low_takeover_pinned;
    let recover_ready = matches!(
        (
            input.vin_drop_mv,
            vin_drop_recover_mv,
            input.tps_total_iout_ma
        ),
        (Some(vin_drop_mv), Some(recover_mv), Some(tps_total_iout_ma))
            if vin_drop_mv <= recover_mv && tps_total_iout_ma <= input.rated_exit_iout_ma
    );

    match tracker.stage {
        AssistPowerStage::Standby => {
            if source_limited_enter_ready {
                tracker.source_limited_enter_streak =
                    tracker.source_limited_enter_streak.saturating_add(1);
                tracker.assist_enter_streak = 0;
                if tracker.source_limited_enter_streak >= input.source_limited_required_samples {
                    tracker.stage = AssistPowerStage::Backup;
                    tracker.backup_reason = Some(BackupReason::SourceLimited);
                    tracker.reset_for_online();
                }
            } else if assist_gate_ready {
                tracker.assist_enter_streak = tracker.assist_enter_streak.saturating_add(1);
                tracker.assist_exit_streak = 0;
                if tracker.assist_enter_streak >= input.assist_required_samples {
                    tracker.stage = AssistPowerStage::AssistLow;
                    tracker.assist_enter_streak = 0;
                }
            } else {
                tracker.assist_enter_streak = 0;
                tracker.assist_exit_streak = 0;
                tracker.promote_streak = 0;
                tracker.recover_streak = 0;
                tracker.source_limited_enter_streak = 0;
            }
        }
        AssistPowerStage::AssistLow => {
            if source_limited_enter_ready {
                tracker.source_limited_enter_streak =
                    tracker.source_limited_enter_streak.saturating_add(1);
                tracker.assist_exit_streak = 0;
                tracker.promote_streak = 0;
                tracker.recover_streak = 0;
                if tracker.source_limited_enter_streak >= input.source_limited_required_samples {
                    tracker.stage = AssistPowerStage::Backup;
                    tracker.backup_reason = Some(BackupReason::SourceLimited);
                    tracker.reset_for_online();
                }
            } else if assist_gate_recovered {
                tracker.assist_exit_streak = tracker.assist_exit_streak.saturating_add(1);
                tracker.assist_enter_streak = 0;
                tracker.promote_streak = 0;
                tracker.recover_streak = 0;
                tracker.source_limited_enter_streak = 0;
                if tracker.assist_exit_streak >= input.assist_required_samples {
                    tracker.stage = AssistPowerStage::Standby;
                    tracker.backup_reason = None;
                    tracker.reset_for_online();
                }
            } else {
                tracker.assist_exit_streak = 0;
                tracker.source_limited_enter_streak = 0;
                if promote_ready {
                    tracker.promote_streak = tracker.promote_streak.saturating_add(1);
                    tracker.recover_streak = 0;
                    if tracker.promote_streak >= input.required_samples {
                        tracker.stage = AssistPowerStage::AssistRated;
                        tracker.promote_streak = 0;
                    }
                } else {
                    tracker.promote_streak = 0;
                    tracker.recover_streak = 0;
                }
            }
        }
        AssistPowerStage::AssistRated => {
            if source_limited_enter_ready {
                tracker.source_limited_enter_streak =
                    tracker.source_limited_enter_streak.saturating_add(1);
                tracker.recover_streak = 0;
                if tracker.source_limited_enter_streak >= input.source_limited_required_samples {
                    tracker.stage = AssistPowerStage::Backup;
                    tracker.backup_reason = Some(BackupReason::SourceLimited);
                    tracker.reset_for_online();
                }
            } else if recover_ready {
                tracker.recover_streak = tracker.recover_streak.saturating_add(1);
                tracker.promote_streak = 0;
                tracker.source_limited_enter_streak = 0;
                if tracker.recover_streak >= input.required_samples {
                    tracker.stage = AssistPowerStage::AssistLow;
                    tracker.reset_for_online();
                }
            } else {
                tracker.recover_streak = 0;
                tracker.source_limited_enter_streak = 0;
            }
        }
        AssistPowerStage::Backup => {
            if tracker.backup_reason == Some(BackupReason::SourceLimited) {
                if source_limited_recover_ready {
                    tracker.source_limited_recover_streak =
                        tracker.source_limited_recover_streak.saturating_add(1);
                    if tracker.source_limited_recover_streak
                        >= input.source_limited_required_samples
                    {
                        tracker.stage = AssistPowerStage::Standby;
                        tracker.backup_reason = None;
                        tracker.reset_for_online();
                    }
                } else {
                    tracker.source_limited_recover_streak = 0;
                }
            }
        }
    }

    tracker.stage
}

fn dcin_vin_drop_threshold_mv(vin_baseline_mv: u16, vin_drop_threshold_pct: u16) -> u16 {
    let threshold = u32::from(vin_baseline_mv) * u32::from(vin_drop_threshold_pct) / 100;
    threshold.max(1) as u16
}

fn dcin_pressure_score_pct(
    state: DcinInputPressureState,
    drop_mv: Option<u16>,
    vin_baseline_mv: Option<u16>,
    vin_drop_threshold_pct: u16,
) -> u8 {
    match state {
        DcinInputPressureState::Inactive => 0,
        DcinInputPressureState::Headroom => 0,
        DcinInputPressureState::Cooldown => 100,
        DcinInputPressureState::Watch | DcinInputPressureState::Limited => {
            let Some(vin_baseline_mv) = vin_baseline_mv else {
                return if matches!(state, DcinInputPressureState::Limited) {
                    90
                } else {
                    45
                };
            };
            let drop_mv = u32::from(drop_mv.unwrap_or_default());
            let threshold_mv = u32::from(dcin_vin_drop_threshold_mv(
                vin_baseline_mv,
                vin_drop_threshold_pct,
            ));
            let normalized = if threshold_mv == 0 {
                0
            } else {
                ((drop_mv * 100) / threshold_mv).min(100) as u8
            };
            if matches!(state, DcinInputPressureState::Limited) {
                normalized.max(80)
            } else {
                normalized.max(35).min(70)
            }
        }
    }
}

pub(super) fn dcin_charge_detail_status_text(
    base_status_text: &'static str,
    limit_active: bool,
    allow_charge: bool,
    effective_target_ichg_ma: Option<u16>,
    limit_reason: DcinChargeLimitReason,
) -> &'static str {
    if !allow_charge && matches!(limit_reason, DcinChargeLimitReason::CooldownRetryWait) {
        "WAIT"
    } else if limit_active {
        match effective_target_ichg_ma {
            Some(target_ichg_ma) if target_ichg_ma <= DCIN_ADAPTIVE_START_ICHG_MA => "CHG100",
            Some(_) => "LIMIT",
            None => "WAIT",
        }
    } else {
        base_status_text
    }
}

fn dcin_vindpm_is_actionable(
    input: DcinInputPressureInput,
    vin_baseline_mv: Option<u16>,
    vin_drop_threshold_pct: u16,
) -> bool {
    if !input.vindpm {
        return false;
    }

    if input.poorsrc || input.iindpm {
        return true;
    }

    if input
        .tps_total_iout_ma
        .is_some_and(|total_iout_ma| total_iout_ma > DCIN_TPS_OUTPUT_STOP_THRESHOLD_MA)
    {
        return true;
    }

    if input
        .vin_iin_ma
        .is_some_and(|vin_iin_ma| vin_iin_ma >= CHARGE_POLICY_DC_DERATE_ENTER_IBUS_MA)
    {
        return true;
    }

    if let (Some(vin_baseline_mv), Some(vin_vbus_mv)) = (vin_baseline_mv, input.vin_vbus_mv) {
        let vin_drop_mv = vin_baseline_mv.saturating_sub(vin_vbus_mv);
        let threshold_mv = dcin_vin_drop_threshold_mv(vin_baseline_mv, vin_drop_threshold_pct);
        if vin_drop_mv > threshold_mv {
            return true;
        }
    }

    // In the real dual-input coexistence case the charger-side VBUS can stay near 5V
    // while VAC2/DCIN is the actual 12V source. Treat bare VINDPM in that shape as
    // observational noise for DCIN pressure, not as a hard stop.
    if input
        .input_vbus_mv
        .is_some_and(|input_vbus_mv| input_vbus_mv < 7_000)
        && input
            .vin_vbus_mv
            .is_some_and(|vin_vbus_mv| vin_vbus_mv >= 7_000)
    {
        return false;
    }

    true
}

pub(super) fn dcin_input_pressure_step(
    tracker: &mut DcinInputPressureTracker,
    now_ms: u64,
    input: DcinInputPressureInput,
    vin_drop_threshold_pct: u16,
) -> DcinInputPressureDecision {
    if !input.dcin_present {
        tracker.dcin_absent_since_ms.get_or_insert(now_ms);
        if tracker.dcin_absent_since_ms.is_some_and(|absent_since_ms| {
            now_ms.saturating_sub(absent_since_ms) >= DCIN_BASELINE_RESTORE_HOLD_MS
        }) {
            tracker.reset();
        }
        return DcinInputPressureDecision::inactive_with_tracker(
            input.requested_target_ichg_ma,
            input.allow_charge,
            tracker,
        );
    }

    tracker.dcin_absent_since_ms = None;

    if let Some(vin_vbus_mv) = input.vin_vbus_mv {
        match tracker.vin_baseline_mv {
            None => tracker.vin_baseline_mv = Some(vin_vbus_mv),
            Some(vin_baseline_mv) if vin_vbus_mv >= vin_baseline_mv => {
                tracker.vin_baseline_mv = Some(vin_vbus_mv);
            }
            Some(_) => {}
        }
        tracker.vin_drop_mv = tracker
            .vin_baseline_mv
            .map(|vin_baseline_mv| vin_baseline_mv.saturating_sub(vin_vbus_mv));
    } else {
        tracker.vin_drop_mv = None;
    }

    if input.requested_target_ichg_ma.is_none() || !input.allow_charge {
        if input.requested_target_ichg_ma.is_none() {
            tracker.adaptive_cap_ichg_ma = None;
            tracker.last_ramp_at_ms = None;
        }
        tracker.pressure_score_pct = dcin_pressure_score_pct(
            DcinInputPressureState::Inactive,
            tracker.vin_drop_mv,
            tracker.vin_baseline_mv,
            vin_drop_threshold_pct,
        );
        return DcinInputPressureDecision {
            pressure_state: DcinInputPressureState::Inactive,
            pressure_reason: DcinInputPressureReason::None,
            trigger_reason: DcinInputPressureReason::None,
            pressure_score_pct: tracker.pressure_score_pct,
            vin_baseline_mv: tracker.vin_baseline_mv,
            vin_drop_mv: tracker.vin_drop_mv,
            tps_total_iout_ma: input.tps_total_iout_ma,
            tps_limit_threshold_ma: Some(DCIN_TPS_OUTPUT_STOP_THRESHOLD_MA),
            adaptive_cap_ichg_ma: tracker.adaptive_cap_ichg_ma,
            effective_target_ichg_ma: None,
            allow_charge: input.allow_charge,
            limit_active: false,
            limit_reason: DcinChargeLimitReason::None,
        };
    }

    let tps_sample_is_new = input.tps_total_iout_fresh
        && input
            .tps_total_iout_sample_seq
            .is_some_and(|sample_seq| tracker.last_tps_total_iout_sample_seq != Some(sample_seq));
    if tps_sample_is_new {
        tracker.last_tps_total_iout_sample_seq = input.tps_total_iout_sample_seq;
        tracker.last_tps_total_iout_over_limit = Some(
            input
                .tps_total_iout_ma
                .is_some_and(|total_iout_ma| total_iout_ma > DCIN_TPS_OUTPUT_STOP_THRESHOLD_MA),
        );
    }

    if let Some(cooldown_until_ms) = tracker.cooldown_until_ms {
        if now_ms < cooldown_until_ms {
            tracker.state = DcinInputPressureState::Cooldown;
            tracker.limit_reason = DcinChargeLimitReason::CooldownRetryWait;
            tracker.pressure_score_pct = 100;
            return DcinInputPressureDecision {
                pressure_state: tracker.state,
                pressure_reason: tracker.reason,
                trigger_reason: tracker.trigger_reason,
                pressure_score_pct: tracker.pressure_score_pct,
                vin_baseline_mv: tracker.vin_baseline_mv,
                vin_drop_mv: tracker.vin_drop_mv,
                tps_total_iout_ma: input.tps_total_iout_ma,
                tps_limit_threshold_ma: Some(DCIN_TPS_OUTPUT_STOP_THRESHOLD_MA),
                adaptive_cap_ichg_ma: tracker.adaptive_cap_ichg_ma,
                effective_target_ichg_ma: None,
                allow_charge: false,
                limit_active: input.requested_target_ichg_ma.is_some(),
                limit_reason: tracker.limit_reason,
            };
        }
        if tracker.last_tps_total_iout_over_limit == Some(true) {
            tracker.state = DcinInputPressureState::Cooldown;
            tracker.limit_reason = DcinChargeLimitReason::CooldownRetryWait;
            tracker.pressure_score_pct = 100;
            return DcinInputPressureDecision {
                pressure_state: tracker.state,
                pressure_reason: tracker.reason,
                trigger_reason: tracker.trigger_reason,
                pressure_score_pct: tracker.pressure_score_pct,
                vin_baseline_mv: tracker.vin_baseline_mv,
                vin_drop_mv: tracker.vin_drop_mv,
                tps_total_iout_ma: input.tps_total_iout_ma,
                tps_limit_threshold_ma: Some(DCIN_TPS_OUTPUT_STOP_THRESHOLD_MA),
                adaptive_cap_ichg_ma: tracker.adaptive_cap_ichg_ma,
                effective_target_ichg_ma: None,
                allow_charge: false,
                limit_active: input.requested_target_ichg_ma.is_some(),
                limit_reason: tracker.limit_reason,
            };
        }
        tracker.cooldown_until_ms = None;
        tracker.last_pressure_at_ms = Some(now_ms);
    }

    let vindpm_actionable =
        dcin_vindpm_is_actionable(input, tracker.vin_baseline_mv, vin_drop_threshold_pct);
    let mut hard_reason = if tps_sample_is_new
        && input
            .tps_total_iout_ma
            .is_some_and(|total_iout_ma| total_iout_ma > DCIN_TPS_OUTPUT_STOP_THRESHOLD_MA)
    {
        Some(DcinInputPressureReason::TpsOutputCurrent)
    } else if input.poorsrc {
        Some(DcinInputPressureReason::Poorsrc)
    } else if vindpm_actionable {
        Some(DcinInputPressureReason::Vindpm)
    } else if input.iindpm {
        Some(DcinInputPressureReason::Iindpm)
    } else {
        None
    };

    if hard_reason.is_none() {
        if let (Some(vin_baseline_mv), Some(vin_vbus_mv)) =
            (tracker.vin_baseline_mv, input.vin_vbus_mv)
        {
            let vin_drop_mv = vin_baseline_mv.saturating_sub(vin_vbus_mv);
            let threshold_mv = dcin_vin_drop_threshold_mv(vin_baseline_mv, vin_drop_threshold_pct);
            if vin_drop_mv > threshold_mv {
                tracker.vin_drop_streak = tracker.vin_drop_streak.saturating_add(1);
                tracker.vin_drop_mv = Some(vin_drop_mv);
                if tracker.vin_drop_streak >= DCIN_ADAPTIVE_VIN_DROP_STREAK_LIMIT {
                    hard_reason = Some(DcinInputPressureReason::VinDrop);
                }
            } else {
                tracker.vin_drop_streak = 0;
                tracker.vin_drop_mv = Some(vin_drop_mv);
                tracker.vin_baseline_mv = Some(vin_vbus_mv);
            }
        } else {
            tracker.vin_drop_streak = 0;
            tracker.vin_drop_mv = None;
        }
    } else {
        tracker.vin_drop_streak = 0;
    }

    if let Some(reason) = hard_reason {
        tracker.reason = reason;
        tracker.trigger_reason = reason;
        tracker.last_pressure_at_ms = Some(now_ms);
        tracker.last_ramp_at_ms = Some(now_ms);
        tracker.limit_reason = match reason {
            DcinInputPressureReason::VinDrop => DcinChargeLimitReason::PressureVinDrop,
            DcinInputPressureReason::TpsOutputCurrent => {
                DcinChargeLimitReason::PressureTpsOutputCurrent
            }
            DcinInputPressureReason::Vindpm => DcinChargeLimitReason::PressureVindpm,
            DcinInputPressureReason::Iindpm => DcinChargeLimitReason::PressureIindpm,
            DcinInputPressureReason::Poorsrc => DcinChargeLimitReason::PressurePoorsrc,
            DcinInputPressureReason::Cooldown => DcinChargeLimitReason::CooldownRetryWait,
            DcinInputPressureReason::None | DcinInputPressureReason::VinDropWatch => {
                DcinChargeLimitReason::RecoveryHold
            }
        };
        if matches!(reason, DcinInputPressureReason::TpsOutputCurrent) {
            tracker.adaptive_cap_ichg_ma = None;
            tracker.cooldown_until_ms = Some(now_ms.saturating_add(DCIN_ADAPTIVE_COOLDOWN_MS));
            tracker.state = DcinInputPressureState::Cooldown;
            tracker.limit_reason = DcinChargeLimitReason::CooldownRetryWait;
            tracker.pressure_score_pct = 100;
            return DcinInputPressureDecision {
                pressure_state: tracker.state,
                pressure_reason: tracker.reason,
                trigger_reason: tracker.trigger_reason,
                pressure_score_pct: tracker.pressure_score_pct,
                vin_baseline_mv: tracker.vin_baseline_mv,
                vin_drop_mv: tracker.vin_drop_mv,
                tps_total_iout_ma: input.tps_total_iout_ma,
                tps_limit_threshold_ma: Some(DCIN_TPS_OUTPUT_STOP_THRESHOLD_MA),
                adaptive_cap_ichg_ma: tracker.adaptive_cap_ichg_ma,
                effective_target_ichg_ma: None,
                allow_charge: false,
                limit_active: input.requested_target_ichg_ma.is_some(),
                limit_reason: tracker.limit_reason,
            };
        }
        match tracker
            .adaptive_cap_ichg_ma
            .unwrap_or(DCIN_ADAPTIVE_START_ICHG_MA)
        {
            cap_ichg_ma if cap_ichg_ma > DCIN_ADAPTIVE_START_ICHG_MA => {
                tracker.adaptive_cap_ichg_ma = Some(
                    cap_ichg_ma
                        .saturating_sub(DCIN_ADAPTIVE_STEP_DOWN_ICHG_MA)
                        .max(DCIN_ADAPTIVE_START_ICHG_MA),
                );
                tracker.state = DcinInputPressureState::Limited;
            }
            _ => {
                tracker.adaptive_cap_ichg_ma = None;
                tracker.cooldown_until_ms = Some(now_ms.saturating_add(DCIN_ADAPTIVE_COOLDOWN_MS));
                tracker.state = DcinInputPressureState::Cooldown;
                tracker.limit_reason = DcinChargeLimitReason::CooldownRetryWait;
                tracker.pressure_score_pct = 100;
                return DcinInputPressureDecision {
                    pressure_state: tracker.state,
                    pressure_reason: tracker.reason,
                    trigger_reason: tracker.trigger_reason,
                    pressure_score_pct: tracker.pressure_score_pct,
                    vin_baseline_mv: tracker.vin_baseline_mv,
                    vin_drop_mv: tracker.vin_drop_mv,
                    tps_total_iout_ma: input.tps_total_iout_ma,
                    tps_limit_threshold_ma: Some(DCIN_TPS_OUTPUT_STOP_THRESHOLD_MA),
                    adaptive_cap_ichg_ma: tracker.adaptive_cap_ichg_ma,
                    effective_target_ichg_ma: None,
                    allow_charge: false,
                    limit_active: input.requested_target_ichg_ma.is_some(),
                    limit_reason: tracker.limit_reason,
                };
            }
        }
    } else if tracker.vin_drop_streak > 0 {
        tracker.state = DcinInputPressureState::Watch;
        tracker.reason = DcinInputPressureReason::VinDropWatch;
    } else if tracker
        .last_pressure_at_ms
        .is_some_and(|last_pressure_at_ms| {
            now_ms.saturating_sub(last_pressure_at_ms) < DCIN_ADAPTIVE_RECOVERY_HOLD_MS
        })
    {
        tracker.state = DcinInputPressureState::Watch;
        if matches!(
            tracker.reason,
            DcinInputPressureReason::None | DcinInputPressureReason::Cooldown
        ) {
            tracker.reason = DcinInputPressureReason::VinDropWatch;
        }
    } else {
        tracker.state = DcinInputPressureState::Headroom;
        tracker.reason = DcinInputPressureReason::None;
        tracker.trigger_reason = DcinInputPressureReason::None;
    }

    let requested_target_ichg_ma = input.requested_target_ichg_ma.unwrap_or_default();
    let cap_was_unset = tracker.adaptive_cap_ichg_ma.is_none();
    let cap_ichg_ma = tracker
        .adaptive_cap_ichg_ma
        .get_or_insert(requested_target_ichg_ma.min(DCIN_ADAPTIVE_START_ICHG_MA));
    if cap_was_unset {
        tracker.last_ramp_at_ms = Some(now_ms);
    }
    if *cap_ichg_ma > requested_target_ichg_ma {
        *cap_ichg_ma = requested_target_ichg_ma;
    }

    let recovery_clear = tracker
        .last_pressure_at_ms
        .map_or(true, |last_pressure_at_ms| {
            now_ms.saturating_sub(last_pressure_at_ms) >= DCIN_ADAPTIVE_RECOVERY_HOLD_MS
        });
    let ramp_due = tracker.last_ramp_at_ms.map_or(true, |last_ramp_at_ms| {
        now_ms.saturating_sub(last_ramp_at_ms) >= DCIN_ADAPTIVE_RAMP_HOLD_MS
    });
    if !cap_was_unset
        && recovery_clear
        && ramp_due
        && *cap_ichg_ma < requested_target_ichg_ma
        && matches!(
            tracker.state,
            DcinInputPressureState::Headroom | DcinInputPressureState::Watch
        )
    {
        *cap_ichg_ma = cap_ichg_ma
            .saturating_add(DCIN_ADAPTIVE_STEP_UP_ICHG_MA)
            .min(requested_target_ichg_ma);
        tracker.last_ramp_at_ms = Some(now_ms);
    }

    let effective_target_ichg_ma = Some(requested_target_ichg_ma.min(*cap_ichg_ma));
    let limit_active = effective_target_ichg_ma != Some(requested_target_ichg_ma);
    if !limit_active && matches!(tracker.state, DcinInputPressureState::Limited) {
        tracker.state = DcinInputPressureState::Headroom;
        tracker.reason = DcinInputPressureReason::None;
    }
    tracker.limit_reason = if matches!(tracker.state, DcinInputPressureState::Limited) {
        tracker.limit_reason
    } else if !recovery_clear && limit_active {
        DcinChargeLimitReason::RecoveryHold
    } else if limit_active {
        DcinChargeLimitReason::StartupRamp
    } else {
        DcinChargeLimitReason::None
    };
    tracker.pressure_score_pct = dcin_pressure_score_pct(
        tracker.state,
        tracker.vin_drop_mv,
        tracker.vin_baseline_mv,
        vin_drop_threshold_pct,
    );

    let _ = input.vin_iin_ma;

    DcinInputPressureDecision {
        pressure_state: tracker.state,
        pressure_reason: tracker.reason,
        trigger_reason: tracker.trigger_reason,
        pressure_score_pct: tracker.pressure_score_pct,
        vin_baseline_mv: tracker.vin_baseline_mv,
        vin_drop_mv: tracker.vin_drop_mv,
        tps_total_iout_ma: input.tps_total_iout_ma,
        tps_limit_threshold_ma: Some(DCIN_TPS_OUTPUT_STOP_THRESHOLD_MA),
        adaptive_cap_ichg_ma: tracker.adaptive_cap_ichg_ma,
        effective_target_ichg_ma,
        allow_charge: true,
        limit_active,
        limit_reason: tracker.limit_reason,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ChargePolicyTelemetry {
    pub(super) rsoc_pct: u16,
    pub(super) cell_min_mv: u16,
    pub(super) cell_max_mv: u16,
    pub(super) charge_ready: bool,
    pub(super) bms_recovery_charge_allowed: bool,
    pub(super) bms_full: bool,
    pub(super) hv: bool,
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
    pub(super) defer_output_power_unknown_block: bool,
    pub(super) telemetry: Option<ChargePolicyTelemetry>,
    pub(super) charger_done: bool,
    pub(super) charger_taper_cv: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ChargePolicyDecision {
    pub(super) state: ChargePolicyState,
    pub(super) allow_charge: bool,
    pub(super) target_ichg_ma: Option<u16>,
    pub(super) start_reason: Option<ChargeStartReason>,
    pub(super) full_reason: Option<ChargeFullReason>,
    pub(super) output_block_reason: Option<ChargePolicyOutputBlockReason>,
    pub(super) recovery_stage: Option<ChargePolicyRecoveryStage>,
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
            recovery_stage: None,
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
            recovery_stage: None,
        };
    }

    if input.output_enabled
        && input.output_power_w10.is_none()
        && start_reason.is_none()
        && !input.defer_output_power_unknown_block
    {
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
            recovery_stage: None,
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
            recovery_stage: None,
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
            recovery_stage: None,
        };
    };

    let low_voltage_recovery_input = matches!(
        input.input_source,
        Some(DashboardInputSource::DcIn | DashboardInputSource::UsbC)
    );
    let low_voltage_recovery_stage = if low_voltage_recovery_input
        && telemetry.cell_min_mv < CHARGE_POLICY_LOW_VOLTAGE_RECOVERY_EXIT_CELL_MIN_MV
    {
        if !telemetry.charge_ready && telemetry.bms_recovery_charge_allowed {
            Some(ChargePolicyRecoveryStage::Bq40Pchg)
        } else if telemetry.charge_ready {
            Some(ChargePolicyRecoveryStage::Bq25792Precharge)
        } else {
            None
        }
    } else {
        None
    };
    let bms_recovery_active = matches!(
        low_voltage_recovery_stage,
        Some(ChargePolicyRecoveryStage::Bq40Pchg)
    ) && start_reason.is_some();
    let bq25792_precharge_active = matches!(
        low_voltage_recovery_stage,
        Some(ChargePolicyRecoveryStage::Bq25792Precharge)
    ) && start_reason.is_some();
    let low_voltage_recovery_active = bms_recovery_active || bq25792_precharge_active;

    if !telemetry.charge_ready && !bms_recovery_active {
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
            recovery_stage: None,
        };
    }

    if memory.full_latched && start_reason.is_some() {
        memory.full_latched = false;
    }

    let full_reason =
        if !low_voltage_recovery_active && (memory.charge_latched || memory.full_latched) {
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
            recovery_stage: None,
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
            recovery_stage: None,
        };
    }

    if !memory.charge_latched {
        if start_reason.is_some() {
            memory.charge_latched = true;
        } else {
            if !input.vbat_present {
                derate.reset();
                return ChargePolicyDecision {
                    state: ChargePolicyState::BlockedNoBms,
                    allow_charge: false,
                    target_ichg_ma: None,
                    start_reason: None,
                    full_reason: None,
                    output_block_reason: None,
                    recovery_stage: None,
                };
            }
            derate.reset();
            return ChargePolicyDecision {
                state: ChargePolicyState::IdleWaitThreshold,
                allow_charge: false,
                target_ichg_ma: None,
                start_reason: None,
                full_reason: None,
                output_block_reason: None,
                recovery_stage: None,
            };
        }
    }

    derate.observe(
        now_ms,
        matches!(input.input_source, Some(DashboardInputSource::DcIn)),
        input.ibus_ma,
    );

    let topoff_active = !low_voltage_recovery_active
        && !derate.derated
        && input.telemetry.is_some_and(|telemetry| {
            telemetry.rsoc_pct >= CHARGE_POLICY_TOPOFF_RSOC_PCT
                && telemetry.charge_ready
                && !telemetry.bms_full
                && (input.charger_taper_cv
                    || telemetry.hv
                    || telemetry.cell_max_mv >= CHARGE_POLICY_TOPOFF_CELL_MAX_MV)
        });
    let state = if low_voltage_recovery_active {
        ChargePolicyState::RecoveringLowVoltage
    } else if derate.derated {
        ChargePolicyState::Charging100mADcDerated
    } else if topoff_active {
        ChargePolicyState::ChargingTopoff200mA
    } else {
        ChargePolicyState::Charging500mA
    };
    let target_ichg_ma = Some(if low_voltage_recovery_active {
        CHARGE_POLICY_BMS_RECOVERY_ICHG_MA
    } else if derate.derated {
        CHARGE_POLICY_DC_DERATED_ICHG_MA
    } else if topoff_active {
        CHARGE_POLICY_TOPOFF_ICHG_MA
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
        recovery_stage: low_voltage_recovery_stage.filter(|_| low_voltage_recovery_active),
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

pub(super) fn bq40_cell_max_mv(snapshot: &Bq40z50Snapshot) -> u16 {
    snapshot.cell_mv.into_iter().max().unwrap_or_default()
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
    if bq40_op_bit(op_status, bq40z50::operation_status::PF) == Some(true) {
        return "permanent_failure";
    }
    if bq40z50::battery_status::error_code(batt_status) != 0 {
        return "sbs_error_code";
    }
    if (batt_status & bq40z50::battery_status::RCA) != 0 {
        return "remaining_capacity_alarm";
    }
    if bq40_op_bit(op_status, bq40z50::operation_status::SLEEP) == Some(true) {
        return "sleep_mode";
    }
    if discharge_reason != "ready" && discharge_reason != "op_status_unavailable" {
        return discharge_reason;
    }
    if charge_reason != "ready" && charge_reason != "op_status_unavailable" {
        return charge_reason;
    }
    if op_status.is_none() {
        return "op_status_unavailable";
    }
    "nominal"
}

pub(super) fn detail_bms_reason_label(primary_reason: &'static str) -> &'static str {
    match primary_reason {
        "nominal" => "SYSTEM READY",
        "xchg_blocked" => "CHG BLOCKED",
        "chg_fet_off" => "CHG FET OFF",
        "xdsg_blocked" => "DSG BLOCKED",
        "dsg_fet_off" => "DSG FET OFF",
        "remaining_capacity_alarm" => "RCA ALARM",
        "cell_undervoltage" => "CELL UV",
        "permanent_failure" => "PERM FAIL",
        "sleep_mode" => "SLEEP MODE",
        "op_status_unavailable" => "STATUS N/A",
        "sbs_error_code" => "SBS ERROR",
        _ => "CHECK STATUS",
    }
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

pub(super) fn confirmed_active_outputs_from_tps_readback(
    active_outputs: EnabledOutputs,
    out_a_enabled: Option<bool>,
    out_b_enabled: Option<bool>,
) -> EnabledOutputs {
    enabled_outputs_from_flags(
        active_outputs.is_enabled(OutputChannel::OutA) && out_a_enabled == Some(true),
        active_outputs.is_enabled(OutputChannel::OutB) && out_b_enabled == Some(true),
    )
}

pub(super) const fn bms_recovery_pending_for_ui(
    activation_pending: bool,
    bms_snapshot_available: bool,
    manual_recovery_pending: bool,
) -> bool {
    (activation_pending && bms_snapshot_available) || manual_recovery_pending
}

pub(super) fn active_tps_output_readback_missing(
    active_outputs: EnabledOutputs,
    out_a_enabled: Option<bool>,
    out_a_ready: bool,
    out_b_enabled: Option<bool>,
    out_b_ready: bool,
) -> bool {
    (active_outputs.is_enabled(OutputChannel::OutA) && out_a_ready && out_a_enabled == Some(false))
        || (active_outputs.is_enabled(OutputChannel::OutB)
            && out_b_ready
            && out_b_enabled == Some(false))
}

pub(super) fn output_restore_input_present(
    mains_present: Option<bool>,
    charger_input_present: Option<bool>,
) -> Option<bool> {
    if mains_present == Some(true) || charger_input_present == Some(true) {
        Some(true)
    } else if mains_present == Some(false) && charger_input_present == Some(false) {
        Some(false)
    } else {
        None
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

    const TEST_VIN_DROP_THRESHOLD_PCT: u16 = 4;

    fn dcin_input_pressure_step(
        tracker: &mut DcinInputPressureTracker,
        now_ms: u64,
        input: DcinInputPressureInput,
    ) -> DcinInputPressureDecision {
        super::dcin_input_pressure_step(tracker, now_ms, input, TEST_VIN_DROP_THRESHOLD_PCT)
    }

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
            detail_input_source(true, true, false, false, Some(5_000), Some(5_000), None),
            Some(DashboardInputSource::UsbC)
        );
        assert_eq!(
            detail_input_source(true, false, true, false, Some(12_000), None, Some(12_000)),
            Some(DashboardInputSource::DcIn)
        );
        assert_eq!(
            detail_input_source(
                true,
                true,
                true,
                false,
                Some(5_000),
                Some(5_000),
                Some(5_000)
            ),
            Some(DashboardInputSource::Auto)
        );
        assert_eq!(
            detail_input_source(false, false, false, false, None, None, None),
            None
        );
    }

    #[test]
    fn detail_input_source_keeps_usbc_route_while_pd_session_is_attached() {
        assert_eq!(
            detail_input_source(false, false, false, true, None, None, None),
            Some(DashboardInputSource::UsbC)
        );
    }

    #[test]
    fn detail_input_source_prefers_dc_when_bq_vbus_tracks_ac2_not_usb_vac1() {
        assert_eq!(
            detail_input_source(
                true,
                true,
                true,
                true,
                Some(12_240),
                Some(5_100),
                Some(12_250),
            ),
            Some(DashboardInputSource::DcIn)
        );
    }

    #[test]
    fn detail_input_source_prefers_dc_when_vac2_is_12v_and_usb_vbus_stays_at_5v() {
        assert_eq!(
            detail_input_source(
                true,
                true,
                true,
                true,
                Some(5_108),
                Some(5_099),
                Some(12_107),
            ),
            Some(DashboardInputSource::DcIn)
        );
    }

    #[test]
    fn trusted_usb_pd_recovery_rsoc_requires_safe_bms_state() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.bq40z50 = SelfCheckCommState::Ok;
        snapshot.bq40z50_soc_pct = Some(53);
        snapshot.bq40z50_no_battery = Some(false);
        snapshot.bq40z50_discharge_ready = Some(true);

        assert_eq!(trusted_usb_pd_recovery_rsoc_pct(&snapshot), Some(53));

        snapshot.bq40z50 = SelfCheckCommState::Warn;
        assert_eq!(trusted_usb_pd_recovery_rsoc_pct(&snapshot), None);

        snapshot.bq40z50 = SelfCheckCommState::Ok;
        snapshot.bq40z50_no_battery = Some(true);
        assert_eq!(trusted_usb_pd_recovery_rsoc_pct(&snapshot), None);

        snapshot.bq40z50_no_battery = Some(false);
        snapshot.bq40z50_discharge_ready = Some(false);
        assert_eq!(trusted_usb_pd_recovery_rsoc_pct(&snapshot), None);
    }

    #[test]
    fn trusted_usb_pd_recovery_rsoc_rejects_invalid_percent() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.bq40z50 = SelfCheckCommState::Ok;
        snapshot.bq40z50_soc_pct = Some(101);
        snapshot.bq40z50_no_battery = Some(false);
        snapshot.bq40z50_discharge_ready = Some(true);

        assert_eq!(trusted_usb_pd_recovery_rsoc_pct(&snapshot), None);
    }

    #[test]
    fn charger_protocol_reports_pps_contract() {
        let state = usb_pd::UsbPdPortState {
            attached: true,
            vbus_present: Some(true),
            contract: Some(usb_pd::ActiveContract {
                kind: usb_pd::ContractKind::Pps,
                object_position: 1,
                voltage_mv: 16_000,
                current_ma: 1_000,
                source_max_current_ma: 3_000,
                input_current_limit_ma: Some(2_000),
                vindpm_mv: Some(15_500),
            }),
            ..Default::default()
        };

        assert_eq!(
            charger_protocol_from_usb_pd(Some(DashboardInputSource::UsbC), state),
            Some(DashboardChargerProtocol::Pps)
        );
    }

    #[test]
    fn charger_protocol_reports_5v_fallback_when_fixed_contract_is_usb_level() {
        let state = usb_pd::UsbPdPortState {
            attached: true,
            vbus_present: Some(true),
            contract: Some(usb_pd::ActiveContract {
                kind: usb_pd::ContractKind::Fixed,
                object_position: 1,
                voltage_mv: 5_000,
                current_ma: 500,
                source_max_current_ma: 3_000,
                input_current_limit_ma: Some(500),
                vindpm_mv: Some(4_500),
            }),
            ..Default::default()
        };

        assert_eq!(
            charger_protocol_from_usb_pd(Some(DashboardInputSource::UsbC), state),
            Some(DashboardChargerProtocol::Usb5V)
        );
    }

    #[test]
    fn charger_protocol_reports_uncaptured_caps_when_attached_without_contract() {
        let state = usb_pd::UsbPdPortState {
            attached: true,
            vbus_present: Some(true),
            ..Default::default()
        };

        assert_eq!(
            charger_protocol_from_usb_pd(Some(DashboardInputSource::UsbC), state),
            Some(DashboardChargerProtocol::SourceCapsUnknown)
        );
    }

    #[test]
    fn manual_stop_hold_blocks_only_plain_charge_policy() {
        assert!(manual_charge_stop_hold_blocks_charge(true, false, false));
    }

    #[test]
    fn detail_bms_reason_label_names_charge_fet_off() {
        assert_eq!(detail_bms_reason_label("chg_fet_off"), "CHG FET OFF");
    }

    #[test]
    fn bq40_primary_reason_prioritizes_permanent_failure_over_other_pack_alarms() {
        let batt_status = bq40z50::battery_status::RCA | 0b1111;
        let op_status = Some(bq40z50::operation_status::PF);
        assert_eq!(
            bq40_primary_reason(batt_status, op_status, "ready", "ready"),
            "permanent_failure"
        );
    }

    #[test]
    fn bq40_primary_reason_prioritizes_sleep_before_path_off_states() {
        let op_status = Some(bq40z50::operation_status::SLEEP);
        assert_eq!(
            bq40_primary_reason(0, op_status, "chg_fet_off", "ready"),
            "sleep_mode"
        );
        assert_eq!(
            bq40_primary_reason(0, op_status, "ready", "dsg_fet_off"),
            "sleep_mode"
        );
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
    fn manual_charge_1a_profile_maps_to_1000ma() {
        assert_eq!(ManualChargeSpeed::Ma1000.ichg_ma(), 1_000);
    }

    #[test]
    fn charger_delivery_diag_detects_manual_1a_under_delivery_on_iindpm() {
        assert_eq!(
            charger_delivery_diag_kind(
                true,
                true,
                Some(1_000),
                Some(1_300),
                Some(420),
                Some(470),
                Some(465),
                false,
                true,
                200,
            ),
            ChargerDeliveryDiagKind::ChargeUnderTargetInputDpm
        );
    }

    #[test]
    fn charger_delivery_diag_does_not_warn_when_actual_current_matches_target() {
        assert_eq!(
            charger_delivery_diag_kind(
                true,
                true,
                Some(1_000),
                Some(1_300),
                Some(1_120),
                Some(940),
                Some(950),
                false,
                false,
                200,
            ),
            ChargerDeliveryDiagKind::None
        );
    }

    #[test]
    fn charger_delivery_diag_treats_negative_battery_current_as_zero_delivery() {
        assert_eq!(
            charger_delivery_diag_kind(
                true,
                true,
                Some(1_000),
                Some(1_300),
                Some(420),
                Some(-900),
                Some(-850),
                false,
                true,
                200,
            ),
            ChargerDeliveryDiagKind::ChargeUnderTargetInputDpm
        );
    }

    #[test]
    fn charger_delivery_diag_preserves_existing_over_limit_checks() {
        assert_eq!(
            charger_delivery_diag_kind(
                true,
                false,
                Some(500),
                Some(740),
                Some(421),
                Some(760),
                None,
                false,
                false,
                200,
            ),
            ChargerDeliveryDiagKind::ChargeOverTarget
        );
        assert_eq!(
            charger_delivery_diag_kind(
                true,
                false,
                Some(500),
                Some(740),
                Some(1_000),
                Some(500),
                None,
                false,
                false,
                200,
            ),
            ChargerDeliveryDiagKind::InputOverLimit
        );
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
    fn requested_tps_total_iout_ignores_unrequested_channels() {
        assert_eq!(
            requested_tps_total_iout_ma(EnabledOutputs::Both, Some(80), Some(60)),
            Some(140)
        );
        assert_eq!(
            requested_tps_total_iout_ma(
                EnabledOutputs::Only(OutputChannel::OutA),
                Some(80),
                Some(60)
            ),
            Some(80)
        );
        assert_eq!(
            requested_tps_total_iout_ma(
                EnabledOutputs::Only(OutputChannel::OutB),
                Some(80),
                Some(60)
            ),
            Some(60)
        );
        assert_eq!(
            requested_tps_total_iout_ma(EnabledOutputs::None, Some(80), Some(60)),
            None
        );
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
    fn runtime_charge_override_blocks_charging_in_output_and_blocked_modes() {
        assert_eq!(
            runtime_charge_override(UpsMode::Supplement, None),
            Some(RuntimeChargeOverride {
                allow_charge: false,
                policy_status_text: "LOAD",
                policy_notice_text: "runtime_assist_no_charge",
            })
        );
        assert_eq!(
            runtime_charge_override(UpsMode::Backup, Some("input_absent")),
            Some(RuntimeChargeOverride {
                allow_charge: false,
                policy_status_text: "NOAC",
                policy_notice_text: "runtime_backup_no_charge",
            })
        );
        assert_eq!(
            runtime_charge_override(UpsMode::Backup, Some("source_limited")),
            Some(RuntimeChargeOverride {
                allow_charge: false,
                policy_status_text: "LOAD",
                policy_notice_text: "runtime_source_limited_backup_no_charge",
            })
        );
        assert_eq!(
            runtime_charge_override(UpsMode::Blocked, None),
            Some(RuntimeChargeOverride {
                allow_charge: false,
                policy_status_text: "LOCK",
                policy_notice_text: "runtime_blocked_no_charge",
            })
        );
        assert_eq!(runtime_charge_override(UpsMode::Standby, None), None);
        assert_eq!(runtime_charge_override(UpsMode::Off, None), None);
    }

    #[test]
    fn runtime_charge_override_does_not_swallow_recovery_force_charge() {
        assert_eq!(
            runtime_charge_override_for_charger(UpsMode::Blocked, None, true, false),
            None
        );
        assert_eq!(
            runtime_charge_override_for_charger(UpsMode::Backup, Some("input_absent"), false, true),
            None
        );
        assert_eq!(
            runtime_charge_override_for_charger(UpsMode::Blocked, None, false, false),
            runtime_charge_override(UpsMode::Blocked, None)
        );
    }

    #[test]
    fn backup_usb_guard_requires_a_fresh_strictly_sub_2w_sample_to_start() {
        let mut guard = BackupUsbChargeGuard::default();

        assert_eq!(
            guard.observe(false, false, true, Some(19), Some(1), false),
            BackupUsbChargeGuardDecision::NotApplicable
        );
        assert_eq!(
            guard.observe(true, true, true, Some(19), Some(1), false),
            BackupUsbChargeGuardDecision::WaitingForLowOutput
        );
        assert_eq!(
            guard.observe(true, true, true, Some(19), Some(2), false),
            BackupUsbChargeGuardDecision::Allow
        );

        // A new USB session cannot reuse the pre-detach low-output observation.
        guard.reset_for_new_session(Some(2));
        assert_eq!(
            guard.observe(true, true, true, Some(19), Some(2), false),
            BackupUsbChargeGuardDecision::WaitingForLowOutput
        );
        assert_eq!(
            guard.observe(true, true, true, Some(19), Some(3), false),
            BackupUsbChargeGuardDecision::Allow
        );

        guard.reset_for_new_session(Some(3));
        assert_eq!(
            guard.observe(true, true, true, Some(20), Some(4), false),
            BackupUsbChargeGuardDecision::WaitingForLowOutput
        );

        guard.reset_for_new_session(Some(4));
        assert_eq!(
            guard.observe(true, false, false, None, None, false),
            BackupUsbChargeGuardDecision::Allow
        );
    }

    #[test]
    fn backup_usb_guard_keeps_charging_through_3w_then_latches_above_3w() {
        let mut guard = BackupUsbChargeGuard::default();
        let _ = guard.observe(false, false, true, Some(0), Some(10), false);
        assert_eq!(
            guard.observe(true, true, true, Some(19), Some(11), false),
            BackupUsbChargeGuardDecision::Allow
        );
        assert_eq!(
            guard.observe(true, true, true, Some(20), Some(12), false),
            BackupUsbChargeGuardDecision::Allow
        );
        assert_eq!(
            guard.observe(true, true, true, Some(29), Some(13), false),
            BackupUsbChargeGuardDecision::Allow
        );
        assert_eq!(
            guard.observe(true, false, true, Some(30), Some(14), false),
            BackupUsbChargeGuardDecision::Allow
        );
        assert_eq!(
            guard.observe(true, false, true, Some(31), Some(15), false),
            BackupUsbChargeGuardDecision::Blocked(BackupUsbChargeBlockReason::OutputHigh)
        );
        assert_eq!(
            guard.latched_reason,
            Some(BackupUsbChargeBlockReason::OutputHigh)
        );
    }

    #[test]
    fn backup_usb_guard_counts_only_distinct_missing_telemetry_attempts() {
        let mut guard = BackupUsbChargeGuard::default();
        let _ = guard.observe(false, false, true, Some(0), Some(20), false);
        assert_eq!(
            guard.observe(true, true, true, Some(19), Some(21), false),
            BackupUsbChargeGuardDecision::Allow
        );
        assert_eq!(
            guard.observe(true, true, true, None, Some(22), false),
            BackupUsbChargeGuardDecision::Allow
        );
        assert_eq!(guard.telemetry_miss_streak, 1);
        assert_eq!(
            guard.observe(true, true, true, None, Some(22), false),
            BackupUsbChargeGuardDecision::Allow
        );
        assert_eq!(guard.telemetry_miss_streak, 1);
        assert_eq!(
            guard.observe(true, true, true, Some(25), Some(23), false),
            BackupUsbChargeGuardDecision::Allow
        );
        assert_eq!(guard.telemetry_miss_streak, 0);
        assert_eq!(
            guard.observe(true, true, true, None, Some(24), false),
            BackupUsbChargeGuardDecision::Allow
        );
        assert_eq!(
            guard.observe(true, true, true, None, Some(25), false),
            BackupUsbChargeGuardDecision::Blocked(BackupUsbChargeBlockReason::TelemetryLost)
        );
    }

    #[test]
    fn backup_usb_guard_preserves_latches_until_manual_session_or_usb_replug_reset() {
        let mut guard = BackupUsbChargeGuard::default();
        let _ = guard.observe(false, false, true, Some(0), Some(30), false);
        let _ = guard.observe(true, true, true, Some(19), Some(31), false);
        assert_eq!(
            guard.observe(true, true, true, Some(31), Some(32), false),
            BackupUsbChargeGuardDecision::Blocked(BackupUsbChargeBlockReason::OutputHigh)
        );
        assert_eq!(
            guard.observe(false, false, true, Some(0), Some(33), false),
            BackupUsbChargeGuardDecision::NotApplicable
        );
        assert_eq!(
            guard.observe(true, true, true, Some(19), Some(34), false),
            BackupUsbChargeGuardDecision::Blocked(BackupUsbChargeBlockReason::OutputHigh)
        );
        assert_eq!(
            guard.observe(true, true, true, Some(31), Some(35), true),
            BackupUsbChargeGuardDecision::Allow
        );

        guard.reset_for_new_session(Some(35));
        let _ = guard.observe(false, false, true, Some(0), Some(36), false);
        assert_eq!(
            guard.observe(true, true, true, Some(19), Some(37), false),
            BackupUsbChargeGuardDecision::Allow
        );
    }

    #[test]
    fn backup_usb_guard_requires_a_new_tps_attempt_after_session_reset() {
        let mut guard = BackupUsbChargeGuard::default();

        guard.reset_for_new_session(Some(40));
        assert_eq!(
            guard.observe(true, true, true, Some(19), Some(40), false),
            BackupUsbChargeGuardDecision::WaitingForLowOutput
        );
        assert_eq!(
            guard.observe(true, true, true, Some(19), Some(41), false),
            BackupUsbChargeGuardDecision::Allow
        );
    }

    #[test]
    fn backup_usb_auto_charge_keeps_the_fixed_500ma_policy() {
        assert_eq!(BACKUP_USB_AUTO_CHARGE_ICHG_MA, 500);
        assert_eq!(BACKUP_USB_AUTO_CHARGE_STATUS_TEXT, "CHG500");
    }

    #[test]
    fn backup_usb_runtime_override_only_opens_backup_for_the_guard_allowance() {
        assert_eq!(
            runtime_charge_override_for_backup_usb_charger(
                UpsMode::Backup,
                Some("input_absent"),
                false,
                false,
                true,
            ),
            None
        );
        assert_eq!(
            runtime_charge_override_for_backup_usb_charger(
                UpsMode::Backup,
                Some("input_absent"),
                false,
                false,
                false,
            ),
            runtime_charge_override(UpsMode::Backup, Some("input_absent"))
        );
        assert_eq!(
            runtime_charge_override_for_backup_usb_charger(
                UpsMode::Backup,
                Some("source_limited"),
                false,
                false,
                true,
            ),
            runtime_charge_override(UpsMode::Backup, Some("source_limited"))
        );
        assert_eq!(
            runtime_charge_override_for_backup_usb_charger(
                UpsMode::Supplement,
                None,
                false,
                false,
                true,
            ),
            runtime_charge_override(UpsMode::Supplement, None)
        );
    }

    #[test]
    fn charger_vbat_adc_overrides_false_status_presence_bit() {
        assert_eq!(
            bq25792_effective_vbat_present(Some(false), Some(16_243)),
            Some(true)
        );
        assert!(!bq40_physical_discharge_path_absent(
            Some(16_243),
            Some(true),
            bq25792_effective_vbat_present(Some(false), Some(16_243))
        ));
    }

    #[test]
    fn charger_vbat_absent_remains_absent_without_valid_adc_voltage() {
        assert_eq!(
            bq25792_effective_vbat_present(Some(false), Some(1_280)),
            Some(false)
        );
        assert!(bq40_physical_discharge_path_absent(
            Some(16_243),
            Some(true),
            bq25792_effective_vbat_present(Some(false), Some(1_280))
        ));
    }

    #[test]
    fn usb_pd_demand_charging_enabled_respects_bms_charge_path() {
        assert!(!usb_pd_demand_charging_enabled(
            Some(true),
            true,
            true,
            Some(false)
        ));
        assert!(usb_pd_demand_charging_enabled(
            Some(true),
            true,
            true,
            Some(true)
        ));
        assert!(usb_pd_demand_charging_enabled(None, true, true, None));
    }

    #[test]
    fn usb_pd_charge_gate_only_blocks_live_usbc_transients() {
        assert!(!usb_pd_charge_gate_ready(true, true, true, false));
        assert!(usb_pd_charge_gate_ready(true, true, true, true));
        assert!(usb_pd_charge_gate_ready(true, false, true, false));
        assert!(usb_pd_charge_gate_ready(false, true, true, false));
        assert!(usb_pd_charge_gate_ready(true, true, false, false));
    }

    #[test]
    fn usb_pd_charge_gate_path_ignores_dc_input_source() {
        assert!(!usb_pd_charge_gate_path_present(
            Some(DashboardInputSource::DcIn),
            true
        ));
        assert!(usb_pd_charge_gate_path_present(
            Some(DashboardInputSource::UsbC),
            true
        ));
        assert!(usb_pd_charge_gate_path_present(
            Some(DashboardInputSource::Auto),
            true
        ));
        assert!(!usb_pd_charge_gate_path_present(None, true));
        assert!(!usb_pd_charge_gate_path_present(
            Some(DashboardInputSource::UsbC),
            false
        ));
    }

    #[test]
    fn charger_vbus_stat_allows_only_source_modes_for_activation_charge() {
        for code in 0x1..=0x6 {
            assert!(charger_vbus_stat_allows_activation_charge(code));
        }
        for code in [0x0, 0x7, 0x8, 0x9, 0xA, 0xB, 0xF] {
            assert!(!charger_vbus_stat_allows_activation_charge(code));
        }
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
    fn usb_pd_input_limit_update_restores_limits_even_in_force_charge_paths() {
        assert_eq!(
            usb_pd_input_limit_update(false, true, true, false, true),
            UsbPdInputLimitUpdate::RestorePrevious
        );
        assert_eq!(
            usb_pd_input_limit_update(false, true, false, true, false),
            UsbPdInputLimitUpdate::RestorePrevious
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
    fn usb_pd_restore_tracking_keeps_usb_pd_fallback_limits_active() {
        assert_eq!(
            usb_pd_restore_tracking_update(true, true, true, true),
            UsbPdRestoreTrackingUpdate::ClearRestorePending
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
    fn confirmed_active_outputs_require_tps_enabled_readback() {
        assert_eq!(
            confirmed_active_outputs_from_tps_readback(
                EnabledOutputs::Both,
                Some(false),
                Some(false)
            ),
            EnabledOutputs::None
        );
        assert_eq!(
            confirmed_active_outputs_from_tps_readback(
                EnabledOutputs::Both,
                Some(true),
                Some(false)
            ),
            EnabledOutputs::Only(OutputChannel::OutA)
        );
        assert_eq!(
            confirmed_active_outputs_from_tps_readback(
                EnabledOutputs::Only(OutputChannel::OutB),
                Some(true),
                Some(true)
            ),
            EnabledOutputs::Only(OutputChannel::OutB)
        );
    }

    #[test]
    fn bms_recovery_pending_for_ui_includes_manual_recovery_transaction() {
        assert!(bms_recovery_pending_for_ui(true, true, false));
        assert!(!bms_recovery_pending_for_ui(true, false, false));
        assert!(bms_recovery_pending_for_ui(false, false, true));
        assert!(bms_recovery_pending_for_ui(true, false, true));
        assert!(!bms_recovery_pending_for_ui(false, true, false));
    }

    #[test]
    fn ready_active_outputs_with_disabled_readback_are_missing() {
        assert!(active_tps_output_readback_missing(
            EnabledOutputs::Both,
            Some(false),
            true,
            Some(true),
            true,
        ));
        assert!(!active_tps_output_readback_missing(
            EnabledOutputs::Both,
            Some(false),
            false,
            Some(false),
            false,
        ));
        assert!(!active_tps_output_readback_missing(
            EnabledOutputs::Only(OutputChannel::OutA),
            Some(true),
            true,
            Some(false),
            true,
        ));
    }

    #[test]
    fn output_restore_input_present_accepts_charger_input_without_vin_mains() {
        assert_eq!(
            output_restore_input_present(Some(false), Some(true)),
            Some(true)
        );
        assert_eq!(output_restore_input_present(None, Some(true)), Some(true));
        assert_eq!(
            output_restore_input_present(Some(true), Some(false)),
            Some(true)
        );
        assert_eq!(
            output_restore_input_present(Some(false), Some(false)),
            Some(false)
        );
        assert_eq!(output_restore_input_present(None, Some(false)), None);
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
            detail_charger_status_text(ChargePolicyState::ChargingTopoff200mA),
            "CHG500"
        );
        assert_eq!(
            detail_charger_status_text(ChargePolicyState::Charging100mADcDerated),
            "CHG100"
        );
        assert_eq!(
            detail_charger_status_text(ChargePolicyState::RecoveringLowVoltage),
            "RECOV"
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
            defer_output_power_unknown_block: false,
            telemetry,
            charger_done: false,
            charger_taper_cv: false,
        }
    }

    fn policy_telemetry(
        rsoc_pct: u16,
        cell_min_mv: u16,
        cell_max_mv: u16,
    ) -> ChargePolicyTelemetry {
        ChargePolicyTelemetry {
            rsoc_pct,
            cell_min_mv,
            cell_max_mv,
            charge_ready: true,
            bms_recovery_charge_allowed: false,
            bms_full: false,
            hv: false,
        }
    }

    fn dcin_input(
        requested_target_ichg_ma: Option<u16>,
        vin_vbus_mv: Option<u16>,
    ) -> DcinInputPressureInput {
        DcinInputPressureInput {
            input_source: Some(DashboardInputSource::DcIn),
            dcin_present: true,
            requested_target_ichg_ma,
            allow_charge: true,
            vin_vbus_mv,
            vin_iin_ma: Some(1_200),
            input_vbus_mv: Some(12_000),
            tps_total_iout_ma: Some(0),
            tps_total_iout_fresh: true,
            tps_total_iout_sample_seq: Some(1),
            poorsrc: false,
            vindpm: false,
            iindpm: false,
        }
    }

    #[test]
    fn dcin_pressure_starts_at_100ma_and_ramps_after_holds() {
        let mut tracker = DcinInputPressureTracker::default();

        let initial =
            dcin_input_pressure_step(&mut tracker, 0, dcin_input(Some(500), Some(19_400)));
        assert_eq!(initial.effective_target_ichg_ma, Some(100));
        assert!(initial.limit_active);
        assert_eq!(initial.limit_reason, DcinChargeLimitReason::StartupRamp);
        assert_eq!(initial.pressure_state, DcinInputPressureState::Headroom);

        let early =
            dcin_input_pressure_step(&mut tracker, 2_999, dcin_input(Some(500), Some(19_400)));
        assert_eq!(early.effective_target_ichg_ma, Some(100));

        let ramped =
            dcin_input_pressure_step(&mut tracker, 3_000, dcin_input(Some(500), Some(19_400)));
        assert_eq!(ramped.effective_target_ichg_ma, Some(200));
        assert!(ramped.limit_active);
    }

    #[test]
    fn dcin_pressure_runs_when_dcin_present_even_if_input_source_is_usb() {
        let mut tracker = DcinInputPressureTracker::default();

        let initial = dcin_input_pressure_step(
            &mut tracker,
            0,
            DcinInputPressureInput {
                input_source: Some(DashboardInputSource::UsbC),
                ..dcin_input(Some(500), Some(19_400))
            },
        );
        assert_eq!(initial.effective_target_ichg_ma, Some(100));
        assert_eq!(initial.pressure_state, DcinInputPressureState::Headroom);

        let pressure = dcin_input_pressure_step(
            &mut tracker,
            3_100,
            DcinInputPressureInput {
                input_source: Some(DashboardInputSource::Auto),
                vindpm: true,
                ..dcin_input(Some(500), Some(18_600))
            },
        );
        assert_eq!(pressure.pressure_state, DcinInputPressureState::Cooldown);
        assert_eq!(pressure.pressure_reason, DcinInputPressureReason::Vindpm);
        assert_eq!(pressure.trigger_reason, DcinInputPressureReason::Vindpm);
        assert_eq!(
            pressure.limit_reason,
            DcinChargeLimitReason::CooldownRetryWait
        );
        assert_eq!(pressure.vin_baseline_mv, Some(19_400));
    }

    #[test]
    fn dcin_pressure_ignores_bare_vindpm_when_only_usb_vbus_is_low_and_dcin_is_idle() {
        let mut tracker = DcinInputPressureTracker::default();

        let _ = dcin_input_pressure_step(&mut tracker, 0, dcin_input(Some(500), Some(12_040)));
        let pressure = dcin_input_pressure_step(
            &mut tracker,
            100,
            DcinInputPressureInput {
                input_source: Some(DashboardInputSource::DcIn),
                vin_vbus_mv: Some(12_032),
                vin_iin_ma: Some(28),
                input_vbus_mv: Some(5_109),
                tps_total_iout_ma: Some(36),
                vindpm: true,
                ..dcin_input(Some(500), Some(12_032))
            },
        );

        assert_ne!(pressure.pressure_reason, DcinInputPressureReason::Vindpm);
        assert_ne!(
            pressure.limit_reason,
            DcinChargeLimitReason::CooldownRetryWait
        );
        assert!(pressure.allow_charge);
    }

    #[test]
    fn dcin_pressure_keeps_higher_baseline_when_charge_policy_is_idle() {
        let mut tracker = DcinInputPressureTracker::default();

        let seeded = dcin_input_pressure_step(&mut tracker, 0, dcin_input(Some(500), Some(12_000)));
        assert_eq!(seeded.vin_baseline_mv, Some(12_000));
        assert_eq!(seeded.vin_drop_mv, Some(0));

        let idle = dcin_input_pressure_step(
            &mut tracker,
            100,
            DcinInputPressureInput {
                requested_target_ichg_ma: None,
                allow_charge: false,
                ..dcin_input(None, Some(11_200))
            },
        );
        assert_eq!(idle.pressure_state, DcinInputPressureState::Inactive);
        assert_eq!(idle.vin_baseline_mv, Some(12_000));
        assert_eq!(idle.vin_drop_mv, Some(800));
    }

    #[test]
    fn dcin_pressure_preserves_recent_baseline_across_short_dcin_loss_for_restore() {
        let mut tracker = DcinInputPressureTracker::default();

        let seeded = dcin_input_pressure_step(&mut tracker, 0, dcin_input(Some(500), Some(12_000)));
        assert_eq!(seeded.vin_baseline_mv, Some(12_000));
        assert_eq!(seeded.vin_drop_mv, Some(0));

        let lost = dcin_input_pressure_step(
            &mut tracker,
            5_000,
            DcinInputPressureInput {
                dcin_present: false,
                requested_target_ichg_ma: None,
                allow_charge: false,
                vin_vbus_mv: Some(0),
                vin_iin_ma: Some(0),
                input_vbus_mv: Some(5_100),
                tps_total_iout_ma: Some(0),
                tps_total_iout_fresh: true,
                tps_total_iout_sample_seq: Some(2),
                poorsrc: false,
                vindpm: false,
                iindpm: false,
                input_source: Some(DashboardInputSource::DcIn),
            },
        );
        assert_eq!(lost.pressure_state, DcinInputPressureState::Inactive);
        assert_eq!(lost.vin_baseline_mv, Some(12_000));
        assert_eq!(lost.vin_drop_mv, Some(0));

        let restored = dcin_input_pressure_step(
            &mut tracker,
            8_000,
            DcinInputPressureInput {
                requested_target_ichg_ma: None,
                allow_charge: false,
                ..dcin_input(None, Some(10_900))
            },
        );
        assert_eq!(restored.pressure_state, DcinInputPressureState::Inactive);
        assert_eq!(restored.vin_baseline_mv, Some(12_000));
        assert_eq!(restored.vin_drop_mv, Some(1_100));
    }

    #[test]
    fn dcin_pressure_clears_stale_baseline_after_long_dcin_loss() {
        let mut tracker = DcinInputPressureTracker::default();

        let seeded = dcin_input_pressure_step(&mut tracker, 0, dcin_input(Some(500), Some(12_000)));
        assert_eq!(seeded.vin_baseline_mv, Some(12_000));

        let first_absent = dcin_input_pressure_step(
            &mut tracker,
            1_000,
            DcinInputPressureInput {
                dcin_present: false,
                requested_target_ichg_ma: None,
                allow_charge: false,
                vin_vbus_mv: Some(0),
                vin_iin_ma: Some(0),
                input_vbus_mv: Some(5_100),
                tps_total_iout_ma: Some(0),
                tps_total_iout_fresh: true,
                tps_total_iout_sample_seq: Some(2),
                poorsrc: false,
                vindpm: false,
                iindpm: false,
                input_source: Some(DashboardInputSource::DcIn),
            },
        );
        assert_eq!(first_absent.vin_baseline_mv, Some(12_000));

        let dropped = dcin_input_pressure_step(
            &mut tracker,
            1_000 + DCIN_BASELINE_RESTORE_HOLD_MS + 1,
            DcinInputPressureInput {
                dcin_present: false,
                requested_target_ichg_ma: None,
                allow_charge: false,
                vin_vbus_mv: Some(0),
                vin_iin_ma: Some(0),
                input_vbus_mv: Some(5_100),
                tps_total_iout_ma: Some(0),
                tps_total_iout_fresh: true,
                tps_total_iout_sample_seq: Some(3),
                poorsrc: false,
                vindpm: false,
                iindpm: false,
                input_source: Some(DashboardInputSource::DcIn),
            },
        );
        assert_eq!(dropped.pressure_state, DcinInputPressureState::Inactive);
        assert_eq!(dropped.vin_baseline_mv, None);
        assert_eq!(dropped.vin_drop_mv, None);

        let reappeared = dcin_input_pressure_step(
            &mut tracker,
            1_000 + DCIN_BASELINE_RESTORE_HOLD_MS + 2,
            dcin_input(Some(500), Some(10_900)),
        );
        assert_eq!(reappeared.vin_baseline_mv, Some(10_900));
        assert_eq!(reappeared.vin_drop_mv, Some(0));
    }

    #[test]
    fn dcin_pressure_online_restore_reset_preserves_baseline_and_drop() {
        let mut tracker = DcinInputPressureTracker {
            state: DcinInputPressureState::Cooldown,
            reason: DcinInputPressureReason::TpsOutputCurrent,
            trigger_reason: DcinInputPressureReason::TpsOutputCurrent,
            limit_reason: DcinChargeLimitReason::CooldownRetryWait,
            adaptive_cap_ichg_ma: None,
            vin_baseline_mv: Some(12_016),
            vin_drop_mv: Some(1_128),
            pressure_score_pct: 100,
            vin_drop_streak: 2,
            last_pressure_at_ms: Some(10_000),
            last_ramp_at_ms: Some(10_000),
            cooldown_until_ms: Some(40_000),
            last_tps_total_iout_sample_seq: Some(12),
            last_tps_total_iout_over_limit: Some(true),
            dcin_absent_since_ms: Some(9_000),
        };

        tracker.reset_for_online_restore();

        assert_eq!(tracker.state, DcinInputPressureState::Inactive);
        assert_eq!(tracker.reason, DcinInputPressureReason::None);
        assert_eq!(tracker.trigger_reason, DcinInputPressureReason::None);
        assert_eq!(tracker.limit_reason, DcinChargeLimitReason::None);
        assert_eq!(tracker.adaptive_cap_ichg_ma, None);
        assert_eq!(tracker.vin_baseline_mv, Some(12_016));
        assert_eq!(tracker.vin_drop_mv, Some(1_128));
        assert_eq!(tracker.pressure_score_pct, 0);
        assert_eq!(tracker.vin_drop_streak, 0);
        assert_eq!(tracker.last_pressure_at_ms, None);
        assert_eq!(tracker.last_ramp_at_ms, None);
        assert_eq!(tracker.cooldown_until_ms, None);
        assert_eq!(tracker.last_tps_total_iout_sample_seq, None);
        assert_eq!(tracker.last_tps_total_iout_over_limit, None);
        assert_eq!(tracker.dcin_absent_since_ms, None);
    }

    #[test]
    fn dcin_pressure_restore_detection_requires_recent_dcin_loss() {
        let tracker = DcinInputPressureTracker {
            dcin_absent_since_ms: Some(9_000),
            ..DcinInputPressureTracker::default()
        };
        assert!(tracker.has_recent_dcin_loss_for_restore());

        let tracker = DcinInputPressureTracker::default();
        assert!(!tracker.has_recent_dcin_loss_for_restore());
    }

    #[test]
    fn dcin_pressure_ac2_restore_preserves_baseline_after_backup_even_without_absent_flag() {
        let tracker = DcinInputPressureTracker::default();
        assert!(tracker.should_preserve_for_ac2_restore(UpsMode::Backup));
        assert!(tracker.should_preserve_for_ac2_restore(UpsMode::Blocked));
        assert!(!tracker.should_preserve_for_ac2_restore(UpsMode::Standby));
        assert!(!tracker.should_preserve_for_ac2_restore(UpsMode::Supplement));
    }

    #[test]
    fn dcin_pressure_ac2_restore_preserves_baseline_when_recent_loss_was_recorded() {
        let tracker = DcinInputPressureTracker {
            dcin_absent_since_ms: Some(9_000),
            ..DcinInputPressureTracker::default()
        };
        assert!(tracker.should_preserve_for_ac2_restore(UpsMode::Standby));
        assert!(tracker.should_preserve_for_ac2_restore(UpsMode::Supplement));
        assert!(tracker.should_preserve_for_ac2_restore(UpsMode::Backup));
    }

    #[test]
    fn dcin_pressure_vindpm_reduces_cap_and_enters_recovery_hold() {
        let mut tracker = DcinInputPressureTracker::default();

        let _ = dcin_input_pressure_step(&mut tracker, 0, dcin_input(Some(500), Some(19_400)));
        let _ = dcin_input_pressure_step(&mut tracker, 3_000, dcin_input(Some(500), Some(19_400)));
        let pressure = dcin_input_pressure_step(
            &mut tracker,
            3_100,
            DcinInputPressureInput {
                vindpm: true,
                ..dcin_input(Some(500), Some(18_600))
            },
        );

        assert_eq!(pressure.pressure_state, DcinInputPressureState::Limited);
        assert_eq!(pressure.pressure_reason, DcinInputPressureReason::Vindpm);
        assert_eq!(pressure.effective_target_ichg_ma, Some(100));
        assert!(pressure.limit_active);
        assert_eq!(pressure.limit_reason, DcinChargeLimitReason::PressureVindpm);

        let held =
            dcin_input_pressure_step(&mut tracker, 8_000, dcin_input(Some(500), Some(19_350)));
        assert_eq!(held.pressure_state, DcinInputPressureState::Watch);
        assert_eq!(held.limit_reason, DcinChargeLimitReason::RecoveryHold);
        assert_eq!(held.effective_target_ichg_ma, Some(100));
    }

    #[test]
    fn dcin_pressure_enters_cooldown_when_100ma_still_overstresses_input() {
        let mut tracker = DcinInputPressureTracker::default();

        let _ = dcin_input_pressure_step(&mut tracker, 0, dcin_input(Some(500), Some(19_400)));
        let cooldown = dcin_input_pressure_step(
            &mut tracker,
            100,
            DcinInputPressureInput {
                poorsrc: true,
                ..dcin_input(Some(500), Some(18_400))
            },
        );

        assert_eq!(cooldown.pressure_state, DcinInputPressureState::Cooldown);
        assert_eq!(cooldown.pressure_reason, DcinInputPressureReason::Poorsrc);
        assert_eq!(cooldown.trigger_reason, DcinInputPressureReason::Poorsrc);
        assert_eq!(cooldown.effective_target_ichg_ma, None);
        assert!(!cooldown.allow_charge);
        assert_eq!(
            cooldown.limit_reason,
            DcinChargeLimitReason::CooldownRetryWait
        );

        let waiting =
            dcin_input_pressure_step(&mut tracker, 20_000, dcin_input(Some(500), Some(19_200)));
        assert_eq!(waiting.pressure_state, DcinInputPressureState::Cooldown);
        assert_eq!(waiting.pressure_reason, DcinInputPressureReason::Poorsrc);
        assert_eq!(waiting.trigger_reason, DcinInputPressureReason::Poorsrc);
        assert_eq!(waiting.effective_target_ichg_ma, None);
        assert!(!waiting.allow_charge);
    }

    #[test]
    fn dcin_pressure_recovers_after_cooldown_and_resume_hold() {
        let mut tracker = DcinInputPressureTracker::default();

        let _ = dcin_input_pressure_step(&mut tracker, 0, dcin_input(Some(500), Some(19_400)));
        let _ = dcin_input_pressure_step(
            &mut tracker,
            100,
            DcinInputPressureInput {
                iindpm: true,
                ..dcin_input(Some(500), Some(18_500))
            },
        );

        let post_cooldown =
            dcin_input_pressure_step(&mut tracker, 30_100, dcin_input(Some(500), Some(19_420)));
        assert_eq!(post_cooldown.pressure_state, DcinInputPressureState::Watch);
        assert_eq!(post_cooldown.effective_target_ichg_ma, Some(100));
        assert_eq!(
            post_cooldown.limit_reason,
            DcinChargeLimitReason::RecoveryHold
        );

        let recovered =
            dcin_input_pressure_step(&mut tracker, 40_100, dcin_input(Some(500), Some(19_430)));
        assert_eq!(recovered.pressure_state, DcinInputPressureState::Headroom);
        assert_eq!(recovered.effective_target_ichg_ma, Some(200));
        assert!(recovered.limit_active);
    }

    #[test]
    fn dcin_pressure_tps_output_current_stops_charge_and_reports_reason() {
        let mut tracker = DcinInputPressureTracker::default();

        let stopped = dcin_input_pressure_step(
            &mut tracker,
            0,
            DcinInputPressureInput {
                tps_total_iout_ma: Some(128),
                tps_total_iout_fresh: true,
                tps_total_iout_sample_seq: Some(2),
                ..dcin_input(Some(500), Some(19_400))
            },
        );

        assert_eq!(stopped.pressure_state, DcinInputPressureState::Cooldown);
        assert_eq!(
            stopped.pressure_reason,
            DcinInputPressureReason::TpsOutputCurrent
        );
        assert_eq!(
            stopped.trigger_reason,
            DcinInputPressureReason::TpsOutputCurrent
        );
        assert_eq!(stopped.effective_target_ichg_ma, None);
        assert!(!stopped.allow_charge);
        assert_eq!(
            stopped.limit_reason,
            DcinChargeLimitReason::CooldownRetryWait
        );
        assert_eq!(stopped.tps_total_iout_ma, Some(128));
        assert_eq!(
            stopped.tps_limit_threshold_ma,
            Some(DCIN_TPS_OUTPUT_STOP_THRESHOLD_MA)
        );
    }

    #[test]
    fn dcin_pressure_does_not_seed_cooldown_before_charge_request() {
        let mut tracker = DcinInputPressureTracker::default();

        let idle_load = dcin_input_pressure_step(
            &mut tracker,
            0,
            DcinInputPressureInput {
                requested_target_ichg_ma: None,
                tps_total_iout_ma: Some(128),
                tps_total_iout_fresh: true,
                tps_total_iout_sample_seq: Some(2),
                ..dcin_input(None, Some(19_400))
            },
        );

        assert_ne!(idle_load.pressure_state, DcinInputPressureState::Cooldown);
        assert_eq!(idle_load.effective_target_ichg_ma, None);
        assert!(idle_load.allow_charge);
        assert_eq!(idle_load.limit_reason, DcinChargeLimitReason::None);

        let first_charge =
            dcin_input_pressure_step(&mut tracker, 100, dcin_input(Some(500), Some(19_400)));
        assert_eq!(first_charge.effective_target_ichg_ma, Some(100));
        assert!(first_charge.allow_charge);
        assert_eq!(
            first_charge.limit_reason,
            DcinChargeLimitReason::StartupRamp
        );
    }

    #[test]
    fn dcin_pressure_prefers_tps_output_reason_over_vin_drop() {
        let mut tracker = DcinInputPressureTracker::default();

        let _ = dcin_input_pressure_step(&mut tracker, 0, dcin_input(Some(500), Some(19_400)));
        let pressure = dcin_input_pressure_step(
            &mut tracker,
            3_100,
            DcinInputPressureInput {
                tps_total_iout_ma: Some(128),
                tps_total_iout_fresh: true,
                tps_total_iout_sample_seq: Some(2),
                ..dcin_input(Some(500), Some(18_200))
            },
        );

        assert_eq!(
            pressure.pressure_reason,
            DcinInputPressureReason::TpsOutputCurrent
        );
        assert_eq!(
            pressure.trigger_reason,
            DcinInputPressureReason::TpsOutputCurrent
        );
        assert_eq!(
            pressure.limit_reason,
            DcinChargeLimitReason::CooldownRetryWait
        );
        assert_eq!(pressure.tps_total_iout_ma, Some(128));
    }

    #[test]
    fn dcin_pressure_tps_output_current_stops_immediately_after_ramp() {
        let mut tracker = DcinInputPressureTracker::default();

        let _ = dcin_input_pressure_step(&mut tracker, 0, dcin_input(Some(500), Some(19_400)));
        let ramped =
            dcin_input_pressure_step(&mut tracker, 3_000, dcin_input(Some(500), Some(19_400)));
        assert_eq!(ramped.effective_target_ichg_ma, Some(200));
        assert!(ramped.allow_charge);

        let stopped = dcin_input_pressure_step(
            &mut tracker,
            3_100,
            DcinInputPressureInput {
                tps_total_iout_ma: Some(128),
                tps_total_iout_fresh: true,
                tps_total_iout_sample_seq: Some(2),
                ..dcin_input(Some(500), Some(19_350))
            },
        );

        assert_eq!(stopped.pressure_state, DcinInputPressureState::Cooldown);
        assert_eq!(
            stopped.pressure_reason,
            DcinInputPressureReason::TpsOutputCurrent
        );
        assert_eq!(stopped.effective_target_ichg_ma, None);
        assert!(!stopped.allow_charge);
        assert_eq!(
            stopped.limit_reason,
            DcinChargeLimitReason::CooldownRetryWait
        );
    }

    #[test]
    fn dcin_pressure_ignores_stale_tps_output_current_samples() {
        let mut tracker = DcinInputPressureTracker::default();

        let stale = dcin_input_pressure_step(
            &mut tracker,
            0,
            DcinInputPressureInput {
                tps_total_iout_ma: Some(128),
                tps_total_iout_fresh: false,
                tps_total_iout_sample_seq: Some(2),
                ..dcin_input(Some(500), Some(19_400))
            },
        );

        assert_eq!(stale.pressure_state, DcinInputPressureState::Headroom);
        assert_eq!(stale.pressure_reason, DcinInputPressureReason::None);
        assert_eq!(stale.effective_target_ichg_ma, Some(100));
        assert!(stale.allow_charge);
        assert_eq!(stale.limit_reason, DcinChargeLimitReason::StartupRamp);
    }

    #[test]
    fn dcin_pressure_consumes_each_tps_sample_only_once() {
        let mut tracker = DcinInputPressureTracker::default();

        let stopped = dcin_input_pressure_step(
            &mut tracker,
            0,
            DcinInputPressureInput {
                tps_total_iout_ma: Some(128),
                tps_total_iout_fresh: true,
                tps_total_iout_sample_seq: Some(2),
                ..dcin_input(Some(500), Some(19_400))
            },
        );
        assert_eq!(stopped.pressure_state, DcinInputPressureState::Cooldown);

        let recovered = dcin_input_pressure_step(
            &mut tracker,
            40_100,
            DcinInputPressureInput {
                tps_total_iout_ma: Some(128),
                tps_total_iout_fresh: false,
                tps_total_iout_sample_seq: Some(2),
                ..dcin_input(Some(500), Some(19_420))
            },
        );
        assert_eq!(recovered.pressure_state, DcinInputPressureState::Cooldown);
        assert_eq!(recovered.effective_target_ichg_ma, None);
        assert!(!recovered.allow_charge);

        let reused = dcin_input_pressure_step(
            &mut tracker,
            43_200,
            DcinInputPressureInput {
                tps_total_iout_ma: Some(128),
                tps_total_iout_fresh: true,
                tps_total_iout_sample_seq: Some(2),
                ..dcin_input(Some(500), Some(19_420))
            },
        );
        assert_eq!(reused.pressure_state, DcinInputPressureState::Cooldown);
        assert_eq!(reused.effective_target_ichg_ma, None);
        assert!(!reused.allow_charge);

        let resumed = dcin_input_pressure_step(
            &mut tracker,
            53_200,
            DcinInputPressureInput {
                tps_total_iout_ma: Some(80),
                tps_total_iout_fresh: true,
                tps_total_iout_sample_seq: Some(3),
                ..dcin_input(Some(500), Some(19_430))
            },
        );
        assert_eq!(resumed.pressure_state, DcinInputPressureState::Watch);
        assert_eq!(resumed.effective_target_ichg_ma, Some(100));
        assert!(resumed.allow_charge);
    }

    #[test]
    fn dcin_pressure_requires_fresh_safe_tps_sample_to_exit_cooldown() {
        let mut tracker = DcinInputPressureTracker::default();

        let _ = dcin_input_pressure_step(
            &mut tracker,
            0,
            DcinInputPressureInput {
                tps_total_iout_ma: Some(128),
                tps_total_iout_fresh: true,
                tps_total_iout_sample_seq: Some(2),
                ..dcin_input(Some(500), Some(19_400))
            },
        );

        let still_blocked = dcin_input_pressure_step(
            &mut tracker,
            30_100,
            DcinInputPressureInput {
                tps_total_iout_ma: Some(128),
                tps_total_iout_fresh: false,
                tps_total_iout_sample_seq: Some(2),
                ..dcin_input(Some(500), Some(19_420))
            },
        );
        assert_eq!(
            still_blocked.pressure_state,
            DcinInputPressureState::Cooldown
        );
        assert_eq!(still_blocked.effective_target_ichg_ma, None);
        assert!(!still_blocked.allow_charge);

        let fresh_safe = dcin_input_pressure_step(
            &mut tracker,
            30_200,
            DcinInputPressureInput {
                tps_total_iout_ma: Some(80),
                tps_total_iout_fresh: true,
                tps_total_iout_sample_seq: Some(3),
                ..dcin_input(Some(500), Some(19_420))
            },
        );
        assert_eq!(fresh_safe.pressure_state, DcinInputPressureState::Watch);
        assert_eq!(fresh_safe.effective_target_ichg_ma, Some(100));
        assert!(fresh_safe.allow_charge);
    }

    #[test]
    fn dcin_target_vindpm_tracks_96pct_of_input_voltage() {
        assert_eq!(dcin_target_vindpm_mv(Some(12_000), None), 11_520);
        assert_eq!(dcin_target_vindpm_mv(None, Some(19_400)), 18_624);
    }

    #[test]
    fn bms_recovery_charge_allowed_accepts_pchg_low_cell_when_safety_status_missing() {
        assert!(bms_recovery_charge_allowed_from_diag(
            Some(false),
            Some(bq40z50::operation_status::PCHG),
            None,
            Some(0),
            Some(0),
            Some(2_858),
        ));
    }

    #[test]
    fn bms_recovery_charge_allowed_accepts_pchg_low_cell_after_cuv_clears() {
        assert!(bms_recovery_charge_allowed_from_diag(
            Some(false),
            Some(bq40z50::operation_status::PCHG),
            Some(0),
            Some(0),
            Some(0),
            Some(2_790),
        ));
    }

    #[test]
    fn bms_recovery_charge_allowed_rejects_missing_safety_status_without_low_cell() {
        assert!(!bms_recovery_charge_allowed_from_diag(
            Some(false),
            Some(bq40z50::operation_status::PCHG),
            None,
            Some(0),
            Some(0),
            Some(CHARGE_POLICY_LOW_VOLTAGE_RECOVERY_EXIT_CELL_MIN_MV),
        ));
    }

    #[test]
    fn bms_recovery_charge_allowed_rejects_fault_or_charge_inhibit() {
        for (op_status, pf_status, charging_status, safety_status) in [
            (
                Some(bq40z50::operation_status::PCHG),
                Some(1),
                Some(0),
                None,
            ),
            (
                Some(bq40z50::operation_status::PCHG),
                Some(0),
                Some(bq40z50::charging_status::IN),
                None,
            ),
            (
                Some(bq40z50::operation_status::PCHG),
                Some(0),
                Some(bq40z50::charging_status::SU),
                None,
            ),
            (
                Some(bq40z50::operation_status::PCHG),
                Some(0),
                Some(0),
                Some(bq40z50::safety_status::CUV | bq40z50::safety_status::CUVC),
            ),
            (
                Some(bq40z50::operation_status::PCHG | bq40z50::operation_status::XCHG),
                Some(0),
                Some(0),
                Some(0),
            ),
        ] {
            assert!(!bms_recovery_charge_allowed_from_diag(
                Some(false),
                op_status,
                safety_status,
                pf_status,
                charging_status,
                Some(2_858),
            ));
        }
    }

    #[test]
    fn discharge_authorization_pack_path_recovery_detects_dsg_on_bat_absent() {
        assert!(bms_discharge_authorization_needs_pack_path_recovery(
            Some(false),
            Some(true),
            Some(false),
        ));
        assert!(bms_discharge_authorization_reason_uses_activation(
            "pack_output_path_recovery_requested"
        ));
        assert_eq!(
            bms_discharge_authorization_success_reason("pack_output_path_recovery_requested"),
            "pack_output_path_recovered"
        );
        assert_eq!(
            bms_discharge_authorization_recovery_action("pack_output_path_recovery_requested"),
            "activation_min_charge"
        );
    }

    #[test]
    fn discharge_authorization_pack_path_recovery_uses_op_dsg_when_afe_unknown() {
        assert!(bms_discharge_authorization_needs_pack_path_recovery(
            Some(false),
            None,
            Some(true),
        ));
    }

    #[test]
    fn discharge_authorization_pack_path_recovery_does_not_mask_dsg_off() {
        assert!(!bms_discharge_authorization_needs_pack_path_recovery(
            Some(false),
            Some(false),
            Some(true),
        ));
        assert!(!bms_discharge_authorization_needs_pack_path_recovery(
            Some(true),
            Some(true),
            Some(true),
        ));
    }

    #[test]
    fn discharge_authorization_fet_state_reset_matches_current_safe_fault() {
        assert!(bms_discharge_authorization_needs_bq40_fet_state_reset(
            Some(false),
            Some(false),
            Some(false),
            Some(true),
            Some(true),
            Some(true),
            Some(false),
            Some(false),
            Some(0),
            Some(0),
            Some(true),
        ));
        assert!(bms_discharge_authorization_reason_uses_activation(
            "pack_output_path_reset_requested"
        ));
        assert_eq!(
            bms_discharge_authorization_success_reason("pack_output_path_reset_requested"),
            "pack_output_path_reset_recovered"
        );
        assert_eq!(
            bms_discharge_authorization_recovery_action("pack_output_path_reset_requested"),
            "bq40_device_reset_then_activation"
        );
    }

    #[test]
    fn discharge_authorization_fet_state_reset_allows_missing_afe_timing_evidence() {
        assert!(bms_discharge_authorization_needs_bq40_fet_state_reset(
            Some(false),
            None,
            None,
            None,
            Some(true),
            Some(true),
            Some(false),
            Some(false),
            Some(0),
            Some(0),
            Some(true),
        ));
    }

    #[test]
    fn discharge_authorization_fet_state_reset_rejects_confirmed_dsg_off() {
        assert!(!bms_discharge_authorization_needs_bq40_fet_state_reset(
            Some(false),
            None,
            None,
            Some(false),
            Some(true),
            Some(true),
            Some(false),
            Some(false),
            Some(0),
            Some(0),
            Some(true),
        ));
    }

    #[test]
    fn discharge_authorization_fet_state_reset_rejects_unsafe_conditions() {
        let safe = (
            Some(false),
            Some(false),
            Some(false),
            Some(true),
            Some(true),
            Some(true),
            Some(false),
            Some(false),
            Some(0),
            Some(0),
            Some(true),
        );
        for override_case in [
            (
                safe.0,
                safe.1,
                safe.2,
                safe.3,
                safe.4,
                safe.5,
                safe.6,
                Some(true),
                safe.8,
                safe.9,
                safe.10,
            ),
            (
                safe.0,
                safe.1,
                safe.2,
                safe.3,
                safe.4,
                safe.5,
                Some(true),
                safe.7,
                safe.8,
                safe.9,
                safe.10,
            ),
            (
                safe.0,
                safe.1,
                safe.2,
                safe.3,
                safe.4,
                safe.5,
                safe.6,
                safe.7,
                Some(1),
                safe.9,
                safe.10,
            ),
            (
                safe.0,
                safe.1,
                safe.2,
                safe.3,
                safe.4,
                safe.5,
                safe.6,
                safe.7,
                safe.8,
                Some(1),
                safe.10,
            ),
            (
                safe.0,
                safe.1,
                safe.2,
                safe.3,
                safe.4,
                safe.5,
                safe.6,
                safe.7,
                safe.8,
                safe.9,
                Some(false),
            ),
            (
                Some(true),
                safe.1,
                safe.2,
                safe.3,
                safe.4,
                safe.5,
                safe.6,
                safe.7,
                safe.8,
                safe.9,
                safe.10,
            ),
        ] {
            assert!(!bms_discharge_authorization_needs_bq40_fet_state_reset(
                override_case.0,
                override_case.1,
                override_case.2,
                override_case.3,
                override_case.4,
                override_case.5,
                override_case.6,
                override_case.7,
                override_case.8,
                override_case.9,
                override_case.10,
            ));
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
                Some(policy_telemetry(79, CHARGE_POLICY_START_CELL_MIN_MV, 4000)),
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
                Some(policy_telemetry(90, 3_650, 3900)),
                Some(DashboardInputSource::UsbC),
                Some(1_000),
            ),
        );

        assert_eq!(decision.state, ChargePolicyState::Charging500mA);
        assert_eq!(decision.start_reason, Some(ChargeStartReason::CellLow));
        assert!(memory.charge_latched);
    }

    #[test]
    fn charge_policy_starts_low_battery_recovery_from_trusted_bms_even_when_charger_vbat_absent() {
        let mut memory = ChargePolicyMemory::default();
        let mut derate = ChargePolicyDerateTracker::default();
        let mut output_load = ChargePolicyOutputLoadTracker::default();
        let mut input = policy_input(
            Some(policy_telemetry(0, 2_912, 3_015)),
            Some(DashboardInputSource::UsbC),
            Some(112),
        );
        input.vbat_present = false;

        let decision = charge_policy_step(&mut memory, &mut derate, &mut output_load, 0, input);

        assert_eq!(decision.state, ChargePolicyState::RecoveringLowVoltage);
        assert!(decision.allow_charge);
        assert_eq!(
            decision.target_ichg_ma,
            Some(CHARGE_POLICY_BMS_RECOVERY_ICHG_MA)
        );
        assert_eq!(
            decision.recovery_stage,
            Some(ChargePolicyRecoveryStage::Bq25792Precharge)
        );
        assert_eq!(
            decision.start_reason,
            Some(ChargeStartReason::RsocAndCellLow)
        );
        assert!(memory.charge_latched);
    }

    #[test]
    fn charge_policy_allows_cuv_precharge_on_dc_recovery_path() {
        let mut memory = ChargePolicyMemory::default();
        let mut derate = ChargePolicyDerateTracker::default();
        let mut output_load = ChargePolicyOutputLoadTracker::default();
        let mut telemetry = policy_telemetry(0, 2_853, 2_974);
        telemetry.charge_ready = false;
        telemetry.bms_recovery_charge_allowed = true;

        let decision = charge_policy_step(
            &mut memory,
            &mut derate,
            &mut output_load,
            0,
            policy_input(Some(telemetry), Some(DashboardInputSource::DcIn), Some(70)),
        );

        assert_eq!(decision.state, ChargePolicyState::RecoveringLowVoltage);
        assert!(decision.allow_charge);
        assert_eq!(
            decision.target_ichg_ma,
            Some(CHARGE_POLICY_BMS_RECOVERY_ICHG_MA)
        );
        assert_eq!(
            decision.recovery_stage,
            Some(ChargePolicyRecoveryStage::Bq40Pchg)
        );
        assert_eq!(
            decision.start_reason,
            Some(ChargeStartReason::RsocAndCellLow)
        );
    }

    #[test]
    fn charge_policy_allows_cuv_precharge_on_usb_path() {
        let mut memory = ChargePolicyMemory::default();
        let mut derate = ChargePolicyDerateTracker::default();
        let mut output_load = ChargePolicyOutputLoadTracker::default();
        let mut telemetry = policy_telemetry(0, 2_853, 2_974);
        telemetry.charge_ready = false;
        telemetry.bms_recovery_charge_allowed = true;

        let decision = charge_policy_step(
            &mut memory,
            &mut derate,
            &mut output_load,
            0,
            policy_input(Some(telemetry), Some(DashboardInputSource::UsbC), Some(70)),
        );

        assert_eq!(decision.state, ChargePolicyState::RecoveringLowVoltage);
        assert!(decision.allow_charge);
        assert_eq!(
            decision.recovery_stage,
            Some(ChargePolicyRecoveryStage::Bq40Pchg)
        );
    }

    #[test]
    fn charge_policy_blocks_cuv_precharge_without_known_input_source() {
        for input_source in [None, Some(DashboardInputSource::Auto)] {
            let mut memory = ChargePolicyMemory::default();
            let mut derate = ChargePolicyDerateTracker::default();
            let mut output_load = ChargePolicyOutputLoadTracker::default();
            let mut telemetry = policy_telemetry(0, 2_853, 2_974);
            telemetry.charge_ready = false;
            telemetry.bms_recovery_charge_allowed = true;

            let decision = charge_policy_step(
                &mut memory,
                &mut derate,
                &mut output_load,
                0,
                policy_input(Some(telemetry), input_source, Some(70)),
            );

            assert_eq!(decision.state, ChargePolicyState::BlockedNoBms);
            assert!(!decision.allow_charge);
            assert_eq!(decision.target_ichg_ma, None);
            assert_eq!(decision.recovery_stage, None);
            assert!(!memory.charge_latched);
        }
    }

    #[test]
    fn charge_policy_cuv_precharge_ignores_charger_termination_done() {
        let mut memory = ChargePolicyMemory {
            charge_latched: true,
            full_latched: false,
        };
        let mut derate = ChargePolicyDerateTracker::default();
        let mut output_load = ChargePolicyOutputLoadTracker::default();
        let mut telemetry = policy_telemetry(0, 2_853, 2_974);
        telemetry.charge_ready = false;
        telemetry.bms_recovery_charge_allowed = true;
        let mut input = policy_input(Some(telemetry), Some(DashboardInputSource::DcIn), Some(74));
        input.charger_done = true;

        let decision = charge_policy_step(&mut memory, &mut derate, &mut output_load, 3_000, input);

        assert_eq!(decision.state, ChargePolicyState::RecoveringLowVoltage);
        assert!(decision.allow_charge);
        assert_eq!(decision.full_reason, None);
        assert!(!memory.full_latched);
    }

    #[test]
    fn charge_policy_starts_low_battery_recovery_when_output_power_is_unknown() {
        let mut memory = ChargePolicyMemory::default();
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
                    Some(policy_telemetry(0, 2_994, 3_077)),
                    Some(DashboardInputSource::UsbC),
                    Some(170),
                )
            },
        );

        assert_eq!(decision.state, ChargePolicyState::RecoveringLowVoltage);
        assert!(decision.allow_charge);
        assert_eq!(
            decision.recovery_stage,
            Some(ChargePolicyRecoveryStage::Bq25792Precharge)
        );
        assert_eq!(
            decision.start_reason,
            Some(ChargeStartReason::RsocAndCellLow)
        );
        assert_eq!(decision.output_block_reason, None);
        assert_eq!(output_load, ChargePolicyOutputLoadTracker::default());
        assert!(memory.charge_latched);
    }

    #[test]
    fn charge_policy_enters_topoff_current_when_taper_cv_and_rsoc_is_99() {
        let mut memory = ChargePolicyMemory::default();
        memory.charge_latched = true;
        let mut derate = ChargePolicyDerateTracker::default();
        let mut output_load = ChargePolicyOutputLoadTracker::default();

        let mut input = policy_input(
            Some(policy_telemetry(
                99,
                CHARGE_POLICY_START_CELL_MIN_MV,
                CHARGE_POLICY_TOPOFF_CELL_MAX_MV,
            )),
            Some(DashboardInputSource::UsbC),
            Some(1_000),
        );
        input.charger_taper_cv = true;

        let decision = charge_policy_step(&mut memory, &mut derate, &mut output_load, 0, input);

        assert_eq!(decision.state, ChargePolicyState::ChargingTopoff200mA);
        assert!(decision.allow_charge);
        assert_eq!(decision.target_ichg_ma, Some(CHARGE_POLICY_TOPOFF_ICHG_MA));
    }

    #[test]
    fn charge_policy_enters_topoff_before_taper_cv_when_rsoc_is_99_and_cell_max_is_high() {
        let mut memory = ChargePolicyMemory::default();
        memory.charge_latched = true;
        let mut derate = ChargePolicyDerateTracker::default();
        let mut output_load = ChargePolicyOutputLoadTracker::default();

        let input = policy_input(
            Some(policy_telemetry(
                99,
                CHARGE_POLICY_START_CELL_MIN_MV,
                CHARGE_POLICY_TOPOFF_CELL_MAX_MV,
            )),
            Some(DashboardInputSource::UsbC),
            Some(1_000),
        );

        let decision = charge_policy_step(&mut memory, &mut derate, &mut output_load, 0, input);

        assert_eq!(decision.state, ChargePolicyState::ChargingTopoff200mA);
        assert!(decision.allow_charge);
        assert_eq!(decision.target_ichg_ma, Some(CHARGE_POLICY_TOPOFF_ICHG_MA));
    }

    #[test]
    fn charge_policy_enters_topoff_when_rsoc_is_99_and_bms_reports_hv() {
        let mut memory = ChargePolicyMemory::default();
        memory.charge_latched = true;
        let mut derate = ChargePolicyDerateTracker::default();
        let mut output_load = ChargePolicyOutputLoadTracker::default();

        let mut telemetry = policy_telemetry(99, CHARGE_POLICY_START_CELL_MIN_MV, 4_105);
        telemetry.hv = true;
        let input = policy_input(
            Some(telemetry),
            Some(DashboardInputSource::UsbC),
            Some(1_000),
        );

        let decision = charge_policy_step(&mut memory, &mut derate, &mut output_load, 0, input);

        assert_eq!(decision.state, ChargePolicyState::ChargingTopoff200mA);
        assert!(decision.allow_charge);
        assert_eq!(decision.target_ichg_ma, Some(CHARGE_POLICY_TOPOFF_ICHG_MA));
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
                Some(policy_telemetry(95, 3_900, 4000)),
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
                Some(policy_telemetry(95, 4_050, 4_050)),
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
                Some(policy_telemetry(95, 4_050, 4_050)),
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
                    Some(policy_telemetry(95, 4_050, 4_050)),
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
                Some(policy_telemetry(90, 3_950, 3_950)),
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
                Some(policy_telemetry(79, 3_950, 3_950)),
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
            Some(policy_telemetry(79, 3_850, 3_850)),
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
            Some(policy_telemetry(79, 3_850, 3_850)),
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
                Some(policy_telemetry(79, 3_850, 3_850)),
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
                    Some(policy_telemetry(79, 3_850, 3_850)),
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
                    Some(policy_telemetry(79, 3_850, 3_850)),
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
            Some(policy_telemetry(79, 3_850, 3_850)),
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
                    Some(policy_telemetry(80, 3_850, 3_850)),
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
            Some(policy_telemetry(79, 3_850, 3_850)),
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
                    Some(policy_telemetry(80, 3_850, 3_850)),
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
    fn charge_policy_defers_unknown_output_power_to_the_backup_usb_guard() {
        let mut memory = ChargePolicyMemory::default();
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
                defer_output_power_unknown_block: true,
                ..policy_input(
                    Some(policy_telemetry(79, 3_850, 3_850)),
                    Some(DashboardInputSource::UsbC),
                    Some(1_000),
                )
            },
        );

        assert_eq!(decision.state, ChargePolicyState::Charging500mA);
        assert!(decision.allow_charge);
        assert_eq!(decision.output_block_reason, None);
    }

    #[test]
    fn backup_usb_unknown_output_power_deferral_excludes_confirmed_manual_override() {
        assert!(defer_output_power_unknown_block_for_backup_usb(true, false));
        assert!(!defer_output_power_unknown_block_for_backup_usb(true, true));
        assert!(!defer_output_power_unknown_block_for_backup_usb(
            false, false
        ));
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
                    Some(policy_telemetry(79, 3_850, 3_850)),
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
                    Some(policy_telemetry(79, 3_850, 3_850)),
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
                    Some(policy_telemetry(79, 3_850, 3_850)),
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
    fn discharge_authorization_emshut_exit_chains_to_pack_path_reset() {
        assert_eq!(
            bms_discharge_authorization_next_after_emshut_exit(
                Some(false),
                Some(false),
                Some(false),
                Some(false),
                Some(true),
                Some(true),
                Some(true),
                Some(false),
                Some(false),
                Some(0),
                Some(0),
                Some(true),
            ),
            Some("pack_output_path_reset_requested")
        );
    }

    #[test]
    fn discharge_authorization_emshut_still_active_does_not_chain() {
        assert_eq!(
            bms_discharge_authorization_next_after_emshut_exit(
                Some(true),
                Some(false),
                Some(false),
                Some(false),
                Some(true),
                Some(true),
                Some(true),
                Some(false),
                Some(false),
                Some(0),
                Some(0),
                Some(true),
            ),
            None
        );
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
                interrupt_status: 0,
                fet_status: 0,
                rxin: 0,
                latch_status: 0,
                interrupt_enable: 0,
                fet_control: 0,
                rxien: 0,
                rlout: 0,
                rhout: 0,
                rhint: 0,
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
    fn stable_mains_state_tracks_when_audio_is_using_aggregate_input_fallback() {
        assert_eq!(
            stable_mains_state(None, None, Some(false)),
            StableMainsState {
                present: Some(false),
                source: AudioMainsSource::AggregateInputPresent,
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
            source: AudioMainsSource::AggregateInputPresent,
        };
        let charger_true = StableMainsState {
            present: Some(true),
            source: AudioMainsSource::AggregateInputPresent,
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
                    source: AudioMainsSource::AggregateInputPresent,
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
        assert_eq!(vin_vbus_mv, Some(19_200));
        assert_eq!(vin_iin_ma, Some(850));
        assert_eq!(mains_present, Some(true));
        assert_eq!(missing_streak, 1);

        mark_vin_telemetry_unavailable(
            true,
            &mut vin_vbus_mv,
            &mut vin_iin_ma,
            &mut mains_present,
            &mut missing_streak,
        );
        assert_eq!(vin_vbus_mv, None);
        assert_eq!(vin_iin_ma, None);
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
    fn runtime_mode_tracker_requires_two_fresh_samples_and_holds_unknown_mains() {
        let mut tracker = super::super::RuntimeModeTracker::new(UpsMode::Standby);

        assert_eq!(
            tracker.update(Some(true), Some(120), true, Some(1), 100, 50, 2),
            UpsMode::Standby
        );
        assert_eq!(
            tracker.update(Some(true), Some(120), true, Some(2), 100, 50, 2),
            UpsMode::Supplement
        );
        assert_eq!(
            tracker.update(None, Some(0), false, Some(2), 100, 50, 2),
            UpsMode::Supplement
        );
        assert_eq!(
            tracker.update(Some(true), Some(40), true, Some(3), 100, 50, 2),
            UpsMode::Supplement
        );
        assert_eq!(
            tracker.update(Some(true), Some(40), true, Some(4), 100, 50, 2),
            UpsMode::Standby
        );
        assert_eq!(
            tracker.update(Some(false), Some(0), true, Some(5), 100, 50, 2),
            UpsMode::Backup
        );
        assert_eq!(
            tracker.update(None, Some(0), false, Some(5), 100, 50, 2),
            UpsMode::Backup
        );
        assert_eq!(
            tracker.update(Some(true), Some(0), true, Some(6), 100, 50, 2),
            UpsMode::Standby
        );
        assert_eq!(
            tracker.update(Some(true), Some(0), true, Some(7), 100, 50, 2),
            UpsMode::Standby
        );
    }

    #[test]
    fn runtime_mode_tracker_only_enters_backup_on_confirmed_no_input() {
        let mut tracker = super::super::RuntimeModeTracker::new(UpsMode::Standby);

        assert_eq!(
            tracker.update(Some(true), Some(120), true, Some(1), 100, 50, 2),
            UpsMode::Standby
        );
        assert_eq!(
            tracker.update(Some(true), Some(120), true, Some(2), 100, 50, 2),
            UpsMode::Supplement
        );
        assert_eq!(
            tracker.update(None, None, false, None, 100, 50, 2),
            UpsMode::Supplement
        );
        assert_eq!(
            tracker.update(Some(false), None, false, None, 100, 50, 2),
            UpsMode::Backup
        );
    }

    fn assist_stage_input(
        vin_vbus_mv: Option<u16>,
        vin_baseline_mv: Option<u16>,
        vin_drop_mv: Option<u16>,
        vin_iin_ma: Option<i32>,
        tps_total_iout_ma: Option<i32>,
        sample_seq: u32,
    ) -> AssistPowerStageInput {
        AssistPowerStageInput {
            mains_present: Some(true),
            input_source: Some(DashboardInputSource::DcIn),
            dcin_assist_allowed: true,
            rated_vout_mv: 12_000,
            standby_target_vout_mv: 10_800,
            current_assist_target_vout_mv: 11_400,
            assist_low_target_vout_mv: 11_400,
            vin_baseline_mv,
            vin_drop_mv,
            vin_vbus_mv,
            vin_iin_ma,
            tps_total_iout_ma,
            tps_total_iout_fresh: true,
            tps_total_iout_sample_seq: Some(sample_seq),
            assist_enter_iout_ma: 100,
            assist_exit_iout_ma: 50,
            assist_required_samples: 2,
            rated_enter_iout_ma: 100,
            rated_exit_iout_ma: 50,
            vin_drop_threshold_pct: TEST_VIN_DROP_THRESHOLD_PCT,
            required_samples: 2,
            source_limited_vin_drop_pct: TEST_VIN_DROP_THRESHOLD_PCT,
            source_limited_enter_iout_ma: 2_000,
            source_limited_exit_iout_ma: 50,
            source_limited_required_samples: 2,
            source_limited_recover_margin_mv: 400,
        }
    }

    #[test]
    fn assist_stage_requires_low_vin_and_tps_current_to_enter_low() {
        let mut tracker = AssistPowerStageTracker::default();

        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(11_900),
                    Some(12_000),
                    Some(600),
                    Some(2_980),
                    Some(160),
                    1
                )
            ),
            AssistPowerStage::Standby
        );
        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(10_888),
                    Some(12_000),
                    Some(600),
                    Some(2_980),
                    Some(80),
                    2
                )
            ),
            AssistPowerStage::Standby
        );
        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(10_840),
                    Some(12_000),
                    Some(600),
                    Some(2_980),
                    Some(220),
                    3
                )
            ),
            AssistPowerStage::Standby
        );
        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(10_840),
                    Some(12_000),
                    Some(600),
                    Some(2_980),
                    Some(220),
                    4
                )
            ),
            AssistPowerStage::AssistLow
        );
    }

    #[test]
    fn assist_stage_keeps_12v_3a_neighbor_in_standby_until_tps_current_reaches_enter_threshold() {
        let mut tracker = AssistPowerStageTracker::default();

        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(10_888),
                    Some(12_040),
                    Some(1_152),
                    Some(2_450),
                    Some(80),
                    1
                )
            ),
            AssistPowerStage::Standby
        );
        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(10_888),
                    Some(12_040),
                    Some(1_152),
                    Some(2_450),
                    Some(80),
                    2
                )
            ),
            AssistPowerStage::Standby
        );
        assert_eq!(tracker.stage, AssistPowerStage::Standby);

        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(10_888),
                    Some(12_040),
                    Some(1_200),
                    Some(2_450),
                    Some(1_040),
                    3
                )
            ),
            AssistPowerStage::Standby
        );
        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(10_888),
                    Some(12_040),
                    Some(1_200),
                    Some(2_450),
                    Some(1_040),
                    4
                )
            ),
            AssistPowerStage::Standby
        );
    }

    #[test]
    fn assist_stage_requires_dcin_input_current_near_limit_before_entering_low() {
        let mut tracker = AssistPowerStageTracker::default();

        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(10_900),
                    Some(12_000),
                    Some(1_160),
                    Some(2_700),
                    Some(1_040),
                    1
                )
            ),
            AssistPowerStage::Standby
        );
        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(10_900),
                    Some(12_000),
                    Some(1_160),
                    Some(2_700),
                    Some(1_040),
                    2
                )
            ),
            AssistPowerStage::Standby
        );
        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(10_900),
                    Some(12_000),
                    Some(1_160),
                    Some(2_960),
                    Some(1_040),
                    3
                )
            ),
            AssistPowerStage::Standby
        );
        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(10_900),
                    Some(12_000),
                    Some(1_160),
                    Some(2_960),
                    Some(1_040),
                    4
                )
            ),
            AssistPowerStage::AssistLow
        );
    }

    #[test]
    fn assist_stage_enters_low_once_dcin_is_near_limit_and_tps_iout_is_meaningful() {
        let mut tracker = AssistPowerStageTracker::default();

        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(10_896),
                    Some(12_040),
                    Some(1_144),
                    Some(3_051),
                    Some(148),
                    1
                )
            ),
            AssistPowerStage::Standby
        );
        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(10_896),
                    Some(12_040),
                    Some(1_144),
                    Some(3_051),
                    Some(148),
                    2
                )
            ),
            AssistPowerStage::Standby
        );
        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(10_896),
                    Some(12_040),
                    Some(1_144),
                    Some(3_051),
                    Some(276),
                    3
                )
            ),
            AssistPowerStage::Standby
        );
        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(10_896),
                    Some(12_040),
                    Some(1_144),
                    Some(3_051),
                    Some(276),
                    4
                )
            ),
            AssistPowerStage::AssistLow
        );
    }

    #[test]
    fn assist_stage_requires_dcin_assist_allowed_for_low_stage() {
        let mut tracker = AssistPowerStageTracker::default();

        let mut input = assist_stage_input(
            Some(11_050),
            Some(12_000),
            Some(600),
            Some(2_980),
            Some(180),
            1,
        );
        input.input_source = Some(DashboardInputSource::UsbC);
        assert_eq!(
            assist_power_stage_step(&mut tracker, input),
            AssistPowerStage::Standby
        );

        let mut input = assist_stage_input(
            Some(10_800),
            Some(12_000),
            Some(600),
            Some(2_980),
            Some(180),
            2,
        );
        input.dcin_assist_allowed = false;
        input.input_source = Some(DashboardInputSource::UsbC);
        assert_eq!(
            assist_power_stage_step(&mut tracker, input),
            AssistPowerStage::Standby
        );
        assert_eq!(tracker.stage, AssistPowerStage::Standby);

        let mut input = assist_stage_input(
            Some(10_800),
            Some(12_000),
            Some(600),
            Some(2_980),
            Some(220),
            3,
        );
        input.input_source = Some(DashboardInputSource::UsbC);
        assert_eq!(
            assist_power_stage_step(&mut tracker, input),
            AssistPowerStage::Standby
        );

        let mut input = assist_stage_input(
            Some(10_800),
            Some(12_000),
            Some(600),
            Some(2_980),
            Some(220),
            4,
        );
        input.input_source = Some(DashboardInputSource::UsbC);
        assert_eq!(
            assist_power_stage_step(&mut tracker, input),
            AssistPowerStage::AssistLow
        );

        let mut input = assist_stage_input(
            Some(10_800),
            Some(12_000),
            Some(600),
            Some(2_980),
            Some(220),
            5,
        );
        input.input_source = Some(DashboardInputSource::UsbC);
        assert_eq!(
            assist_power_stage_step(&mut tracker, input),
            AssistPowerStage::AssistLow
        );
    }

    #[test]
    fn assist_stage_marks_backup_reason_for_input_absent() {
        let mut tracker = AssistPowerStageTracker::default();
        let mut input = assist_stage_input(Some(0), Some(12_000), None, Some(0), Some(0), 1);
        input.mains_present = Some(false);

        assert_eq!(
            assist_power_stage_step(&mut tracker, input),
            AssistPowerStage::Backup
        );
        assert_eq!(tracker.backup_reason, Some(BackupReason::InputAbsent));
        assert_eq!(
            tracker.backup_reason.map(BackupReason::as_str),
            Some("input_absent")
        );
    }

    #[test]
    fn assist_stage_enters_source_limited_backup_after_consecutive_limited_samples() {
        let mut tracker = AssistPowerStageTracker::default();
        let mut input = assist_stage_input(
            Some(10_850),
            Some(12_000),
            Some(1_150),
            Some(3_050),
            Some(2_400),
            1,
        );

        assert_eq!(
            assist_power_stage_step(&mut tracker, input),
            AssistPowerStage::Standby
        );
        assert_eq!(tracker.backup_reason, None);

        input.tps_total_iout_sample_seq = Some(2);
        assert_eq!(
            assist_power_stage_step(&mut tracker, input),
            AssistPowerStage::Backup
        );
        assert_eq!(tracker.backup_reason, Some(BackupReason::SourceLimited));
        assert_eq!(
            tracker.backup_reason.map(BackupReason::as_str),
            Some("source_limited")
        );
    }

    #[test]
    fn assist_stage_source_limited_backup_recovers_with_hysteresis_and_samples() {
        let mut tracker = AssistPowerStageTracker::default();
        let mut limited = assist_stage_input(
            Some(10_850),
            Some(12_000),
            Some(1_150),
            Some(3_050),
            Some(2_400),
            1,
        );
        let _ = assist_power_stage_step(&mut tracker, limited);
        limited.tps_total_iout_sample_seq = Some(2);
        assert_eq!(
            assist_power_stage_step(&mut tracker, limited),
            AssistPowerStage::Backup
        );

        let mut recovered =
            assist_stage_input(Some(11_900), Some(12_000), Some(80), Some(900), Some(40), 3);
        assert_eq!(
            assist_power_stage_step(&mut tracker, recovered),
            AssistPowerStage::Backup
        );
        assert_eq!(tracker.backup_reason, Some(BackupReason::SourceLimited));

        recovered.tps_total_iout_sample_seq = Some(4);
        assert_eq!(
            assist_power_stage_step(&mut tracker, recovered),
            AssistPowerStage::Standby
        );
        assert_eq!(tracker.backup_reason, None);
    }

    #[test]
    fn assist_stage_enters_source_limited_backup_below_fixed_input_current_ceiling() {
        let mut tracker = AssistPowerStageTracker::default();
        tracker.last_tps_total_iout_sample_seq = Some(1);
        let input = AssistPowerStageInput {
            source_limited_enter_iout_ma: 1_100,
            ..assist_stage_input(
                Some(10_900),
                Some(12_000),
                Some(1_100),
                Some(2_400),
                Some(1_240),
                1,
            )
        };

        assert_eq!(
            assist_power_stage_step(&mut tracker, input),
            AssistPowerStage::Standby
        );
        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                AssistPowerStageInput {
                    tps_total_iout_sample_seq: Some(2),
                    ..input
                }
            ),
            AssistPowerStage::Backup
        );
        assert_eq!(tracker.backup_reason, Some(BackupReason::SourceLimited));
    }

    #[test]
    fn assist_stage_does_not_enter_source_limited_backup_without_tps_contribution() {
        let mut tracker = AssistPowerStageTracker::default();
        tracker.last_tps_total_iout_sample_seq = Some(1);
        let input = AssistPowerStageInput {
            source_limited_enter_iout_ma: 1_100,
            ..assist_stage_input(
                Some(10_900),
                Some(12_000),
                Some(1_100),
                Some(900),
                Some(1_000),
                1,
            )
        };

        assert_eq!(
            assist_power_stage_step(&mut tracker, input),
            AssistPowerStage::Standby
        );
        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                AssistPowerStageInput {
                    tps_total_iout_sample_seq: Some(2),
                    ..input
                }
            ),
            AssistPowerStage::Standby
        );
        assert_eq!(tracker.backup_reason, None);
    }

    #[test]
    fn assist_stage_requires_vin_drop_and_tps_current_to_promote_to_rated() {
        let mut tracker = AssistPowerStageTracker::default();

        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(10_840),
                    Some(12_000),
                    Some(200),
                    Some(2_980),
                    Some(220),
                    1
                )
            ),
            AssistPowerStage::Standby
        );
        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(10_840),
                    Some(12_000),
                    Some(200),
                    Some(2_980),
                    Some(220),
                    2
                )
            ),
            AssistPowerStage::AssistLow
        );
        assert_eq!(tracker.stage, AssistPowerStage::AssistLow);

        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(11_050),
                    Some(12_000),
                    Some(520),
                    Some(2_980),
                    Some(160),
                    3
                )
            ),
            AssistPowerStage::AssistLow
        );
        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(11_000),
                    Some(12_000),
                    Some(520),
                    Some(2_980),
                    Some(160),
                    4
                )
            ),
            AssistPowerStage::AssistRated
        );
        assert_eq!(tracker.stage, AssistPowerStage::AssistRated);
    }

    #[test]
    fn assist_stage_does_not_promote_when_vin_has_not_collapsed_to_low_target_yet() {
        let mut tracker = AssistPowerStageTracker::default();

        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(10_840),
                    Some(12_000),
                    Some(520),
                    Some(2_980),
                    Some(220),
                    1
                )
            ),
            AssistPowerStage::Standby
        );
        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(10_840),
                    Some(12_000),
                    Some(520),
                    Some(2_980),
                    Some(220),
                    2
                )
            ),
            AssistPowerStage::AssistLow
        );

        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(11_520),
                    Some(12_000),
                    Some(520),
                    Some(1_600),
                    Some(1_200),
                    3
                )
            ),
            AssistPowerStage::AssistLow
        );
        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(11_520),
                    Some(12_000),
                    Some(520),
                    Some(1_600),
                    Some(1_200),
                    4
                )
            ),
            AssistPowerStage::AssistLow
        );
    }

    #[test]
    fn assist_stage_promotes_once_vin_drop_and_tps_current_hold_with_low_target_pinned() {
        let mut tracker = AssistPowerStageTracker::default();

        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(10_840),
                    Some(12_000),
                    Some(520),
                    Some(2_980),
                    Some(220),
                    1
                )
            ),
            AssistPowerStage::Standby
        );
        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(10_840),
                    Some(12_000),
                    Some(520),
                    Some(2_980),
                    Some(220),
                    2
                )
            ),
            AssistPowerStage::AssistLow
        );

        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(11_448),
                    Some(12_000),
                    Some(520),
                    Some(2_600),
                    Some(1_200),
                    3
                )
            ),
            AssistPowerStage::AssistLow
        );
        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(11_456),
                    Some(12_000),
                    Some(520),
                    Some(2_580),
                    Some(1_200),
                    4
                )
            ),
            AssistPowerStage::AssistRated
        );
        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(11_456),
                    Some(12_000),
                    Some(520),
                    Some(2_580),
                    Some(1_200),
                    5
                )
            ),
            AssistPowerStage::AssistRated
        );
    }

    #[test]
    fn assist_stage_does_not_promote_on_tps_current_alone() {
        let mut tracker = AssistPowerStageTracker::default();

        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(10_840),
                    Some(12_000),
                    Some(120),
                    Some(2_980),
                    Some(220),
                    1
                )
            ),
            AssistPowerStage::Standby
        );
        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(10_840),
                    Some(12_000),
                    Some(120),
                    Some(2_980),
                    Some(220),
                    2
                )
            ),
            AssistPowerStage::AssistLow
        );
        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(11_100),
                    Some(12_000),
                    Some(120),
                    Some(2_980),
                    Some(180),
                    3
                )
            ),
            AssistPowerStage::AssistLow
        );
    }

    #[test]
    fn assist_stage_does_not_promote_before_low_ramp_reaches_target() {
        let mut tracker = AssistPowerStageTracker::default();

        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(10_840),
                    Some(12_000),
                    Some(520),
                    Some(2_980),
                    Some(220),
                    1
                )
            ),
            AssistPowerStage::Standby
        );
        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(10_840),
                    Some(12_000),
                    Some(520),
                    Some(2_980),
                    Some(220),
                    2
                )
            ),
            AssistPowerStage::AssistLow
        );

        let mut input = assist_stage_input(
            Some(11_000),
            Some(12_000),
            Some(520),
            Some(2_980),
            Some(220),
            3,
        );
        input.current_assist_target_vout_mv = 11_000;
        input.assist_low_target_vout_mv = 11_400;
        assert_eq!(
            assist_power_stage_step(&mut tracker, input),
            AssistPowerStage::AssistLow
        );

        let mut input = assist_stage_input(
            Some(10_980),
            Some(12_000),
            Some(520),
            Some(2_980),
            Some(220),
            4,
        );
        input.current_assist_target_vout_mv = 11_100;
        input.assist_low_target_vout_mv = 11_400;
        assert_eq!(
            assist_power_stage_step(&mut tracker, input),
            AssistPowerStage::AssistLow
        );
    }

    #[test]
    fn assist_stage_can_promote_even_if_vin_is_not_below_assist_low_target() {
        let mut tracker = AssistPowerStageTracker::default();

        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(10_840),
                    Some(12_000),
                    Some(520),
                    Some(2_980),
                    Some(220),
                    1
                )
            ),
            AssistPowerStage::Standby
        );
        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(10_840),
                    Some(12_000),
                    Some(520),
                    Some(2_980),
                    Some(220),
                    2
                )
            ),
            AssistPowerStage::AssistLow
        );

        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(11_432),
                    Some(12_048),
                    Some(616),
                    Some(2_980),
                    Some(1_260),
                    3
                )
            ),
            AssistPowerStage::AssistLow
        );
        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(11_432),
                    Some(12_048),
                    Some(616),
                    Some(2_980),
                    Some(1_272),
                    4
                )
            ),
            AssistPowerStage::AssistRated
        );
    }

    #[test]
    fn assist_stage_recovers_from_rated_to_low_and_backup_remains_distinct() {
        let mut tracker = AssistPowerStageTracker::default();

        let _ = assist_power_stage_step(
            &mut tracker,
            assist_stage_input(
                Some(10_840),
                Some(12_000),
                Some(520),
                Some(2_980),
                Some(220),
                1,
            ),
        );
        let _ = assist_power_stage_step(
            &mut tracker,
            assist_stage_input(
                Some(10_840),
                Some(12_000),
                Some(520),
                Some(2_980),
                Some(220),
                2,
            ),
        );
        let _ = assist_power_stage_step(
            &mut tracker,
            assist_stage_input(
                Some(10_840),
                Some(12_000),
                Some(520),
                Some(2_980),
                Some(220),
                3,
            ),
        );
        let _ = assist_power_stage_step(
            &mut tracker,
            assist_stage_input(
                Some(10_840),
                Some(12_000),
                Some(520),
                Some(2_980),
                Some(220),
                4,
            ),
        );
        assert_eq!(tracker.stage, AssistPowerStage::AssistRated);

        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(11_400),
                    Some(12_000),
                    Some(200),
                    Some(2_980),
                    Some(40),
                    5
                )
            ),
            AssistPowerStage::AssistRated
        );
        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(11_700),
                    Some(12_000),
                    Some(200),
                    Some(2_980),
                    Some(40),
                    6
                )
            ),
            AssistPowerStage::AssistLow
        );

        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(11_700),
                    Some(12_000),
                    Some(80),
                    Some(2_980),
                    Some(40),
                    7
                )
            ),
            AssistPowerStage::AssistLow
        );
        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                assist_stage_input(
                    Some(11_700),
                    Some(12_000),
                    Some(80),
                    Some(2_980),
                    Some(40),
                    8
                )
            ),
            AssistPowerStage::Standby
        );

        assert_eq!(
            assist_power_stage_step(
                &mut tracker,
                AssistPowerStageInput {
                    mains_present: Some(false),
                    input_source: Some(DashboardInputSource::DcIn),
                    dcin_assist_allowed: true,
                    rated_vout_mv: 12_000,
                    standby_target_vout_mv: 10_800,
                    current_assist_target_vout_mv: 11_400,
                    assist_low_target_vout_mv: 11_400,
                    vin_baseline_mv: Some(12_000),
                    vin_drop_mv: None,
                    vin_vbus_mv: Some(0),
                    vin_iin_ma: Some(0),
                    tps_total_iout_ma: Some(0),
                    tps_total_iout_fresh: true,
                    tps_total_iout_sample_seq: Some(5),
                    assist_enter_iout_ma: 100,
                    assist_exit_iout_ma: 50,
                    assist_required_samples: 2,
                    rated_enter_iout_ma: 100,
                    rated_exit_iout_ma: 50,
                    vin_drop_threshold_pct: TEST_VIN_DROP_THRESHOLD_PCT,
                    required_samples: 2,
                    source_limited_vin_drop_pct: TEST_VIN_DROP_THRESHOLD_PCT,
                    source_limited_enter_iout_ma: 2_000,
                    source_limited_exit_iout_ma: 50,
                    source_limited_required_samples: 2,
                    source_limited_recover_margin_mv: 400,
                }
            ),
            AssistPowerStage::Backup
        );
    }
}
