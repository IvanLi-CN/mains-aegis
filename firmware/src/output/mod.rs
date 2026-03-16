pub mod tps55288;

use crate::front_panel_scene::{
    is_bq40_activation_needed, BmsActivationState, BmsResultKind, DashboardInputSource,
    SelfCheckCommState, SelfCheckUiSnapshot, UpsMode,
};
use crate::irq::IrqSnapshot;
use esp_firmware::bq25792;
use esp_firmware::bq40z50;
use esp_firmware::fan;
use esp_firmware::ina3221;
use esp_firmware::output_protection;
use esp_firmware::output_state as output_state_logic;
use esp_firmware::tmp112;
use esp_hal::gpio::{Flex, Input};
use esp_hal::ram;
use esp_hal::time::{Duration, Instant};

pub use self::tps55288::OutputChannel;
pub use esp_firmware::output_state::OutputGateReason;

#[cfg(feature = "bms-dual-probe-diag")]
fn bms_probe_candidates() -> &'static [u8] {
    &bq40z50::I2C_ADDRESS_CANDIDATES
}

#[cfg(not(feature = "bms-dual-probe-diag"))]
fn bms_probe_candidates() -> &'static [u8] {
    &[bq40z50::I2C_ADDRESS_PRIMARY]
}

#[cfg(feature = "bms-dual-probe-diag")]
const BMS_ADDR_LOG: &str = "0x0b/0x16";

#[cfg(not(feature = "bms-dual-probe-diag"))]
const BMS_ADDR_LOG: &str = "0x0b";

const BMS_ACTIVATION_WINDOW: Duration = Duration::from_secs(40);
const BMS_ACTIVATION_FORCE_VREG_MV: u16 = 16_800;
const BMS_ACTIVATION_FORCE_ICHG_MA: u16 = 200;
const BMS_ACTIVATION_FORCE_IINDPM_MA: u16 = 500;
// Historical PASS samples needed a light-weight no-charge observe window before any wake bursts.
// Keep the first 12 s on the old 2 s cadence so activation can discover a naturally settling
// gauge before staged wake touches start perturbing the bus.
const BMS_ACTIVATION_PROBE_WITHOUT_CHARGE_WINDOW: Duration = Duration::from_secs(12);
const BMS_ACTIVATION_PROBE_WITHOUT_CHARGE_PERIOD: Duration = Duration::from_secs(2);
const BMS_ACTIVATION_REPOWER_OFF_WINDOW: Duration = Duration::from_secs(10);
const BMS_ACTIVATION_MIN_CHARGE_SETTLE: Duration = Duration::from_secs(4);
// After repower, keep the first recovery window on the tool's fast cadence so we do not miss
// the brief transition out of the bogus 49 mV pattern.
const BMS_ACTIVATION_MIN_CHARGE_PROBE_WINDOW: Duration = Duration::from_secs(12);
const BMS_ACTIVATION_DIAG_STAGE_DELAYS_MS: [u64; 3] = [0, 800, 1_600];
const BMS_ACTIVATION_DIAG_TOUCH_READ_GAPS_MS: [u64; 3] = [22, 40, 66];
const BMS_ACTIVATION_READ_GAPS_MS: [u64; 3] = [22, 40, 66];
const BMS_ACTIVATION_KEEPALIVE_GAP: Duration = Duration::from_millis(40);
const BMS_ACTIVATION_KEEPALIVE_ROUNDS: usize = 3;
const BMS_ACTIVATION_FOLLOWUP_INITIAL_DELAY: Duration = Duration::from_millis(0);
const BMS_ACTIVATION_FOLLOWUP_PERIOD: Duration = Duration::from_secs(2);
const BMS_ACTIVATION_EXIT_EXERCISE_WINDOW: Duration = Duration::from_secs(6);
const BMS_ACTIVATION_EXIT_EXERCISE_PERIOD: Duration = Duration::from_secs(2);
const BMS_ACTIVATION_CHARGER_POLL_PERIOD: Duration = Duration::from_secs(2);
const BMS_ACTIVATION_AUTO_POLL_RELEASE_DELAY: Duration = Duration::from_secs(34);
const BMS_ACTIVATION_AUTO_DELAY: Duration = Duration::from_secs(30);
const BMS_BOOT_DIAG_SHIP_RESET_DELAY: Duration = Duration::from_secs(20);
const BMS_BOOT_DIAG_SHIP_RESET_SETTLE: Duration = Duration::from_millis(800);
// Temporary boot-diag validation: keep the proven 16.8V/200mA/500mA wake bias active through
// the 30 s settle window so auto-validation matches the tool's working conditions.
const BMS_ACTIVATION_AUTO_BOOT_FORCE_CHARGE: bool = true;
// Boot auto-validation must exercise the real activation state machine so the
// captured logs reflect the same wake-window path as the manual self-check flow.
const BMS_BOOT_DIAG_TOOL_STYLE_PROBE_ONLY: bool = false;
const BMS_BOOT_DIAG_TOOL_STYLE_FORCE_HOLD: Duration = BMS_ACTIVATION_MIN_CHARGE_PROBE_WINDOW;
const BMS_ACTIVATION_ISOLATION_WINDOW: Duration = Duration::from_millis(40);
const BMS_ACTIVATION_MAC_WRITE_SETTLE: Duration = Duration::from_millis(66);
const BMS_ROM_MODE_SIGNATURE: u16 = 0x9002;
const BMS_DEVICE_TYPE_BQ40Z50: u16 = 0x4500;
const BMS_ACTIVATION_WORD_GAP: Duration = Duration::from_millis(2);
const BMS_DETAIL_MAC_REFRESH_PERIOD: Duration = Duration::from_secs(8);
const BMS_DETAIL_MAC_REFRESH_STAGGER: Duration = Duration::from_secs(2);
const BMS_SUSPICIOUS_VOLTAGE_MV: u16 = 5_911;
const BMS_SUSPICIOUS_CURRENT_MA: i16 = 5_911;
const BMS_SUSPICIOUS_STATUS: u16 = 0x1717;
const BMS_NO_BATTERY_VPACK_MAX_MV: u16 = 2_500;
const BQ40_CURRENT_IDLE_THRESHOLD_MA: i16 = 20;
const CHARGER_FAULT0_VBUS_OVP: u8 = 1 << 6;
const CHARGER_FAULT0_VBAT_OVP: u8 = 1 << 5;
const CHARGER_FAULT0_IBUS_OCP: u8 = 1 << 4;
const CHARGER_FAULT0_IBAT_OCP: u8 = 1 << 3;
const CHARGER_FAULT0_CONV_OCP: u8 = 1 << 2;
const CHARGER_FAULT0_VAC2_OVP: u8 = 1 << 1;
const CHARGER_FAULT0_VAC1_OVP: u8 = 1 << 0;
const CHARGER_FAULT1_VSYS_SHORT: u8 = 1 << 7;
const CHARGER_FAULT1_VSYS_OVP: u8 = 1 << 6;
const CHARGER_FAULT1_OTG_OVP: u8 = 1 << 5;
const CHARGER_FAULT1_TSHUT: u8 = 1 << 2;
const CHARGER_INPUT_IBUS_MAX_MA: i16 = 5_000;
const CHARGER_INPUT_VBUS_MAX_MV: u16 = 30_000;
const CHARGER_INPUT_POWER_ANOMALY_W10: u32 = 2_000;
const FAN_RPM_SAMPLE_WINDOW_MS: u64 = 1_000;
const FAN_RPM_MIN_SAMPLE_WINDOW_MS: u64 = 400;
const VIN_MAINS_PRESENT_THRESHOLD_MV: u16 = 3_000;
const VIN_MAINS_LATCH_FAILURE_LIMIT: u8 = 2;

const BMS_DIAG_BREADCRUMB_LEN: usize = 8;
const BMS_DIAG_BREADCRUMB_VERSION: u8 = 1;

#[ram(unstable(rtc_fast, persistent))]
static mut BMS_DIAG_BREADCRUMB_RTC: [u8; BMS_DIAG_BREADCRUMB_LEN] = [0; BMS_DIAG_BREADCRUMB_LEN];

fn bms_diag_breadcrumb_note(code: u8, detail: u8) {
    let buf = [
        b'B',
        b'D',
        b'B',
        b'G',
        BMS_DIAG_BREADCRUMB_VERSION,
        code,
        detail,
        0,
    ];
    unsafe {
        core::ptr::write_volatile(core::ptr::addr_of_mut!(BMS_DIAG_BREADCRUMB_RTC), buf);
    }
}

fn bms_diag_breadcrumb_take() -> Option<(u8, u8)> {
    let buf: [u8; BMS_DIAG_BREADCRUMB_LEN] =
        unsafe { core::ptr::read_volatile(core::ptr::addr_of!(BMS_DIAG_BREADCRUMB_RTC)) };
    if buf[0] != b'B'
        || buf[1] != b'D'
        || buf[2] != b'B'
        || buf[3] != b'G'
        || buf[4] != BMS_DIAG_BREADCRUMB_VERSION
    {
        return None;
    }
    unsafe {
        core::ptr::write_volatile(
            core::ptr::addr_of_mut!(BMS_DIAG_BREADCRUMB_RTC),
            [0; BMS_DIAG_BREADCRUMB_LEN],
        );
    }
    Some((buf[5], buf[6]))
}

fn self_check_comm_state_name(state: SelfCheckCommState) -> &'static str {
    match state {
        SelfCheckCommState::Pending => "pending",
        SelfCheckCommState::Ok => "ok",
        SelfCheckCommState::Warn => "warn",
        SelfCheckCommState::Err => "err",
        SelfCheckCommState::NotAvailable => "na",
    }
}

fn bms_result_name(result: BmsResultKind) -> &'static str {
    match result {
        BmsResultKind::Success => "success",
        BmsResultKind::NoBattery => "no_battery",
        BmsResultKind::RomMode => "rom_mode",
        BmsResultKind::Abnormal => "abnormal",
        BmsResultKind::NotDetected => "not_detected",
    }
}

fn bms_result_option_name(result: Option<BmsResultKind>) -> &'static str {
    result.map_or("none", bms_result_name)
}

fn audio_battery_low_state_name(state: AudioBatteryLowState) -> &'static str {
    match state {
        AudioBatteryLowState::Inactive => "inactive",
        AudioBatteryLowState::WithMains => "with_mains",
        AudioBatteryLowState::NoMains => "no_mains",
        AudioBatteryLowState::Unknown => "unknown",
    }
}

fn bq40_pack_indicates_no_battery(vpack_mv: u16) -> bool {
    vpack_mv < BMS_NO_BATTERY_VPACK_MAX_MV
}

fn bq40_low_pack_runtime_signature_matches(
    vpack_mv_a: u16,
    current_ma_a: i16,
    rsoc_pct_a: u16,
    batt_status_a: u16,
    vpack_mv_b: u16,
    current_ma_b: i16,
    rsoc_pct_b: u16,
    batt_status_b: u16,
) -> bool {
    vpack_mv_a == vpack_mv_b
        && current_ma_a == current_ma_b
        && rsoc_pct_a == rsoc_pct_b
        && batt_status_a == batt_status_b
}

fn bq40_self_test_no_battery_confirmed<I2C>(
    i2c: &mut I2C,
    addr: u8,
    temp_k_x10: u16,
    voltage_mv: u16,
    current_ma: i16,
    soc_pct: u16,
    status_raw: u16,
) -> bool
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    if !bq40_pack_indicates_no_battery(voltage_mv)
        || !(-400..=1250).contains(&bq40z50::temp_c_x10_from_k_x10(temp_k_x10))
        || soc_pct > 100
    {
        return false;
    }

    let confirm = (
        bq40z50::read_u16(i2c, addr, bq40z50::cmd::TEMPERATURE),
        bq40z50::read_u16(i2c, addr, bq40z50::cmd::VOLTAGE),
        bq40z50::read_i16(i2c, addr, bq40z50::cmd::CURRENT),
        bq40z50::read_u16(i2c, addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE),
        bq40z50::read_u16(i2c, addr, bq40z50::cmd::BATTERY_STATUS),
    );

    match confirm {
        (
            Ok(confirm_temp_k_x10),
            Ok(confirm_voltage_mv),
            Ok(confirm_current_ma),
            Ok(confirm_soc_pct),
            Ok(confirm_status_raw),
        ) => {
            (-400..=1250).contains(&bq40z50::temp_c_x10_from_k_x10(confirm_temp_k_x10))
                && confirm_soc_pct <= 100
                && bq40_pack_indicates_no_battery(confirm_voltage_mv)
                && bq40_low_pack_runtime_signature_matches(
                    voltage_mv,
                    current_ma,
                    soc_pct,
                    status_raw,
                    confirm_voltage_mv,
                    confirm_current_ma,
                    confirm_soc_pct,
                    confirm_status_raw,
                )
        }
        _ => false,
    }
}

fn bq40_op_bit(op_status: Option<u32>, mask: u32) -> Option<bool> {
    op_status.map(|raw| (raw & mask) != 0)
}

fn bq40_decode_charge_path(op_status: Option<u32>) -> (Option<bool>, &'static str) {
    let Some(raw) = op_status else {
        return (None, "op_status_unavailable");
    };

    let xchg = (raw & bq40z50::operation_status::XCHG) != 0;
    let chg_fet = (raw & bq40z50::operation_status::CHG) != 0;

    if xchg {
        (Some(false), "xchg_blocked")
    } else if chg_fet {
        (Some(true), "ready")
    } else {
        (Some(false), "chg_fet_off")
    }
}

fn bq40_decode_discharge_path(op_status: Option<u32>) -> (Option<bool>, &'static str) {
    let Some(raw) = op_status else {
        return (None, "op_status_unavailable");
    };

    let xdsg = (raw & bq40z50::operation_status::XDSG) != 0;
    let dsg_fet = (raw & bq40z50::operation_status::DSG) != 0;

    if xdsg {
        (Some(false), "xdsg_blocked")
    } else if dsg_fet {
        (Some(true), "ready")
    } else {
        (Some(false), "dsg_fet_off")
    }
}

fn bq40_decode_current_flow(current_ma: i16) -> &'static str {
    if current_ma > BQ40_CURRENT_IDLE_THRESHOLD_MA {
        "charging"
    } else if current_ma < -BQ40_CURRENT_IDLE_THRESHOLD_MA {
        "discharging"
    } else {
        "idle"
    }
}

fn audio_charge_phase_from_chg_stat(code: u8) -> AudioChargePhase {
    match code & 0x07 {
        0 => AudioChargePhase::NotCharging,
        1 | 2 | 3 | 4 | 6 => AudioChargePhase::Charging,
        7 => AudioChargePhase::Completed,
        _ => AudioChargePhase::Unknown,
    }
}

fn detail_input_source(
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

fn detail_charger_status_text(
    charger_fault: bool,
    input_present: bool,
    allow_charge: bool,
    chg_stat: u8,
) -> &'static str {
    if charger_fault {
        "FAULT"
    } else if !input_present {
        "NOAC"
    } else if !allow_charge {
        "LOCK"
    } else {
        match audio_charge_phase_from_chg_stat(chg_stat) {
            AudioChargePhase::Charging => "CHG",
            AudioChargePhase::Completed => "DONE",
            AudioChargePhase::NotCharging => "READY",
            AudioChargePhase::Unknown => "N/A",
        }
    }
}

fn detail_fan_status_text(status: fan::Status) -> &'static str {
    if status.tach_fault {
        "FAULT"
    } else {
        match status.command {
            fan::FanLevel::Off => "OFF",
            fan::FanLevel::Mid => "MID",
            fan::FanLevel::High => "HIGH",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct FanRpmTracker {
    window_started_ms: Option<u64>,
    window_pulses: u32,
    rpm: Option<u16>,
}

impl FanRpmTracker {
    const fn new() -> Self {
        Self {
            window_started_ms: None,
            window_pulses: 0,
            rpm: None,
        }
    }

    fn reset(&mut self) {
        self.window_started_ms = None;
        self.window_pulses = 0;
        self.rpm = None;
    }

    fn observe(
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
        let enough_pulses = self.window_pulses >= u32::from(cfg.tach_pulses_per_rev);
        let should_refresh = elapsed_ms >= FAN_RPM_SAMPLE_WINDOW_MS
            || (elapsed_ms >= FAN_RPM_MIN_SAMPLE_WINDOW_MS && enough_pulses);

        if should_refresh {
            self.rpm = fan_rpm_from_sample(self.window_pulses, elapsed_ms, cfg.tach_pulses_per_rev);
            self.window_started_ms = Some(now_ms);
            self.window_pulses = 0;
        }

        self.rpm
    }
}

fn fan_rpm_from_sample(pulse_count: u32, elapsed_ms: u64, pulses_per_rev: u8) -> Option<u16> {
    if pulse_count == 0 || elapsed_ms == 0 || pulses_per_rev == 0 {
        return None;
    }

    let rpm = u64::from(pulse_count)
        .saturating_mul(60_000)
        .checked_div(elapsed_ms.saturating_mul(u64::from(pulses_per_rev)))?;
    Some(rpm.min(u64::from(u16::MAX)) as u16)
}

fn detail_battery_temp_c(snapshot: &Bq40z50Snapshot) -> Option<i16> {
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

fn detail_da_status2_temp_c(temp_k_x10: u16) -> Option<i16> {
    let temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(temp_k_x10);
    (-400..=1250)
        .contains(&temp_c_x10)
        .then_some((temp_c_x10 / 10) as i16)
}

fn detail_bms_cell_sensor_temps(snapshot: &Bq40z50Snapshot) -> [Option<i16>; 4] {
    snapshot
        .da_status2
        .map_or([None, None, None, None], |da_status2| {
            da_status2.ts_temp_k_x10.map(detail_da_status2_temp_c)
        })
}

fn detail_bms_board_temp_c(snapshot: &Bq40z50Snapshot) -> Option<i16> {
    snapshot
        .da_status2
        .and_then(|da_status2| detail_da_status2_temp_c(da_status2.ts_temp_k_x10[0]))
}

fn filter_energy_mwh(cwh: u16) -> Option<u32> {
    (cwh != u16::MAX).then_some(cwh as u32 * 10)
}

fn approximate_energy_mwh(capacity_mah: u16, vpack_mv: u16) -> Option<u32> {
    (capacity_mah != 0 && vpack_mv != 0).then_some(capacity_mah as u32 * vpack_mv as u32 / 1000)
}

fn detail_bms_energy_mwh(snapshot: &Bq40z50Snapshot) -> Option<u32> {
    if (snapshot.battery_mode & bq40z50::battery_mode::CAPM) != 0 {
        Some(snapshot.remcap as u32 * 10)
    } else {
        snapshot
            .filter_capacity
            .and_then(|filter| filter_energy_mwh(filter.remaining_energy_cwh))
            .or_else(|| approximate_energy_mwh(snapshot.remcap, snapshot.vpack_mv))
    }
}

fn detail_bms_full_capacity_mwh(snapshot: &Bq40z50Snapshot) -> Option<u32> {
    if (snapshot.battery_mode & bq40z50::battery_mode::CAPM) != 0 {
        Some(snapshot.fcc as u32 * 10)
    } else {
        snapshot
            .filter_capacity
            .and_then(|filter| filter_energy_mwh(filter.full_charge_energy_cwh))
            .or_else(|| approximate_energy_mwh(snapshot.fcc, snapshot.vpack_mv))
    }
}

fn detail_bms_balance_mask(snapshot: &Bq40z50Snapshot) -> Option<u8> {
    match bq40_op_bit(snapshot.op_status, bq40z50::operation_status::CB) {
        Some(false) => Some(0),
        Some(true) => None,
        None => None,
    }
}

fn detail_bms_single_balance_cell(balance_mask: Option<u8>) -> Option<u8> {
    let mask = balance_mask?;
    if mask.count_ones() != 1 {
        return None;
    }

    Some(mask.trailing_zeros() as u8 + 1)
}

fn bq40_primary_reason(
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

fn bq40_cell_min_max_delta(cell_mv: &[u16; 4]) -> (u16, u16, u16) {
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

const fn enabled_outputs_from_flags(out_a: bool, out_b: bool) -> EnabledOutputs {
    match (out_a, out_b) {
        (true, true) => EnabledOutputs::Both,
        (true, false) => EnabledOutputs::Only(OutputChannel::OutA),
        (false, true) => EnabledOutputs::Only(OutputChannel::OutB),
        (false, false) => EnabledOutputs::None,
    }
}

const fn logic_outputs_from_enabled(outputs: EnabledOutputs) -> output_state_logic::EnabledOutputs {
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

const fn enabled_outputs_from_logic(outputs: output_state_logic::EnabledOutputs) -> EnabledOutputs {
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

const fn output_state_to_logic(
    state: OutputRuntimeState,
) -> output_state_logic::OutputRuntimeState {
    output_state_logic::OutputRuntimeState::new(
        logic_outputs_from_enabled(state.requested_outputs),
        logic_outputs_from_enabled(state.active_outputs),
        logic_outputs_from_enabled(state.recoverable_outputs),
        state.gate_reason,
    )
}

const fn output_state_from_logic(
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

pub(super) fn i2c_error_kind(err: esp_hal::i2c::master::Error) -> &'static str {
    use esp_hal::i2c::master::Error;
    match err {
        Error::Timeout => "i2c_timeout",
        Error::AcknowledgeCheckFailed(_) => "i2c_nack",
        Error::ArbitrationLost => "i2c_arbitration",
        _ => "i2c",
    }
}

fn spin_delay(wait: Duration) {
    let start = Instant::now();
    while start.elapsed() < wait {}
}

pub(super) fn tps_error_kind(err: ::tps55288::Error<esp_hal::i2c::master::Error>) -> &'static str {
    match err {
        ::tps55288::Error::I2c(e) => i2c_error_kind(e),
        ::tps55288::Error::OutOfRange => "out_of_range",
        ::tps55288::Error::InvalidConfig => "invalid_config",
    }
}

pub(super) fn ina_error_kind(err: ina3221::Error<esp_hal::i2c::master::Error>) -> &'static str {
    match err {
        ina3221::Error::I2c(e) => i2c_error_kind(e),
        ina3221::Error::OutOfRange => "out_of_range",
        ina3221::Error::InvalidConfig => "invalid_config",
    }
}

#[derive(Clone, Copy)]
pub struct BootSelfTestResult {
    pub ina_detected: bool,
    pub detected_tmp_outputs: EnabledOutputs,
    pub detected_tps_outputs: EnabledOutputs,
    pub requested_outputs: EnabledOutputs,
    pub active_outputs: EnabledOutputs,
    pub recoverable_outputs: EnabledOutputs,
    pub output_gate_reason: OutputGateReason,
    pub charger_probe_ok: bool,
    pub charger_enabled: bool,
    pub initial_audio_charge_phase: AudioChargePhase,
    pub initial_bms_protection_active: bool,
    pub initial_tps_a_over_voltage: bool,
    pub initial_tps_b_over_voltage: bool,
    pub initial_tps_a_over_current: bool,
    pub initial_tps_b_over_current: bool,
    pub bms_addr: Option<u8>,
    pub self_check_snapshot: SelfCheckUiSnapshot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelfCheckStage {
    Begin,
    Sensors,
    Screen,
    Bms,
    Charger,
    Tps,
    Done,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct OutputRuntimeState {
    requested_outputs: EnabledOutputs,
    active_outputs: EnabledOutputs,
    recoverable_outputs: EnabledOutputs,
    gate_reason: OutputGateReason,
}

impl OutputRuntimeState {
    const fn new(
        requested_outputs: EnabledOutputs,
        active_outputs: EnabledOutputs,
        recoverable_outputs: EnabledOutputs,
        gate_reason: OutputGateReason,
    ) -> Self {
        Self {
            requested_outputs,
            active_outputs,
            recoverable_outputs,
            gate_reason,
        }
    }
}

fn output_state_gate_transition(
    state: OutputRuntimeState,
    gate_reason: OutputGateReason,
) -> OutputRuntimeState {
    output_state_from_logic(output_state_logic::output_state_gate_transition(
        output_state_to_logic(state),
        gate_reason,
    ))
}

fn output_restore_pending_from_state(
    state: OutputRuntimeState,
    mains_present: Option<bool>,
) -> bool {
    output_state_logic::output_restore_pending_from_state(
        output_state_to_logic(state),
        mains_present,
    )
}

#[derive(Clone, Copy)]
pub struct PanelProbeResult {
    pub tca6408_present: bool,
    pub fusb302_present: bool,
}

impl PanelProbeResult {
    pub const fn screen_present(self) -> bool {
        // The front-panel screen path depends on the panel IO expander.
        self.tca6408_present
    }
}

fn mains_present_from_vin(vin_vbus_mv: Option<u16>) -> Option<bool> {
    vin_vbus_mv.map(|mv| mv >= VIN_MAINS_PRESENT_THRESHOLD_MV)
}

fn stable_mains_present(
    vin_mains_present: Option<bool>,
    vin_vbus_mv: Option<u16>,
    charger_present: Option<bool>,
) -> Option<bool> {
    stable_mains_state(vin_mains_present, vin_vbus_mv, charger_present).present
}

fn record_vin_sample_failure(vin_mains_present: &mut Option<bool>, missing_streak: &mut u8) {
    *missing_streak = missing_streak.saturating_add(1);
    if *missing_streak >= VIN_MAINS_LATCH_FAILURE_LIMIT {
        *vin_mains_present = None;
    }
}

fn mark_vin_telemetry_unavailable(
    telemetry_include_vin_ch3: bool,
    vin_vbus_mv: &mut Option<u16>,
    vin_iin_ma: &mut Option<i32>,
    vin_mains_present: &mut Option<bool>,
    missing_streak: &mut u8,
) {
    *vin_vbus_mv = None;
    *vin_iin_ma = None;
    if telemetry_include_vin_ch3 {
        record_vin_sample_failure(vin_mains_present, missing_streak);
    } else {
        *vin_mains_present = None;
        *missing_streak = 0;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AudioMainsSource {
    #[default]
    Unknown,
    Vin,
    ChargerFallback,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct StableMainsState {
    present: Option<bool>,
    source: AudioMainsSource,
}

fn stable_mains_state(
    vin_mains_present: Option<bool>,
    vin_vbus_mv: Option<u16>,
    charger_present: Option<bool>,
) -> StableMainsState {
    if let Some(present) = mains_present_from_vin(vin_vbus_mv) {
        return StableMainsState {
            present: Some(present),
            source: AudioMainsSource::Vin,
        };
    }
    if let Some(present) = vin_mains_present {
        return StableMainsState {
            present: Some(present),
            source: AudioMainsSource::Vin,
        };
    }
    if let Some(present) = charger_present {
        return StableMainsState {
            present: Some(present),
            source: AudioMainsSource::ChargerFallback,
        };
    }
    StableMainsState::default()
}

fn mains_present_edge(prev: StableMainsState, next: StableMainsState) -> Option<bool> {
    if prev.present.is_some() && next.present.is_some() && prev.present != next.present {
        next.present
    } else {
        None
    }
}

fn ups_mode_from_mains(mains_present: Option<bool>, has_output: bool) -> UpsMode {
    match mains_present {
        Some(true) => {
            if has_output {
                UpsMode::Supplement
            } else {
                UpsMode::Standby
            }
        }
        Some(false) => {
            let _ = has_output;
            UpsMode::Backup
        }
        None => {
            // Unknown VBUS is treated conservatively: avoid assuming mains-present.
            if has_output {
                UpsMode::Backup
            } else {
                UpsMode::Standby
            }
        }
    }
}

pub fn log_i2c2_presence<I2C>(i2c: &mut I2C) -> PanelProbeResult
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let mut tca6408_present = false;
    let mut fusb302_present = false;

    defmt::info!("self_test: i2c2 scan begin");
    for (addr, name) in [(0x21u8, "tca6408a"), (0x22u8, "fusb302b")] {
        let mut buf = [0u8; 1];
        match i2c.write_read(addr, &[0x00], &mut buf) {
            Ok(()) => {
                if addr == 0x21 {
                    tca6408_present = true;
                }
                if addr == 0x22 {
                    fusb302_present = true;
                }
                defmt::info!(
                    "self_test: i2c2 ok addr=0x{=u8:x} dev={} reg0=0x{=u8:x}",
                    addr,
                    name,
                    buf[0]
                );
            }
            Err(e) => defmt::warn!(
                "self_test: i2c2 miss addr=0x{=u8:x} dev={} err={}",
                addr,
                name,
                i2c_error_kind(e)
            ),
        }
    }

    defmt::info!(
        "self_test: i2c2 summary panel_io={=bool} fusb302={=bool}",
        tca6408_present,
        fusb302_present
    );

    PanelProbeResult {
        tca6408_present,
        fusb302_present,
    }
}

#[allow(dead_code)]
pub fn boot_self_test<I2C>(
    i2c: &mut I2C,
    desired_outputs: EnabledOutputs,
    vout_mv: u16,
    ilimit_ma: u16,
    include_vin_ch3: bool,
    tmp_out_a_ok: bool,
    tmp_out_b_ok: bool,
    sync_ok: bool,
    panel_probe: PanelProbeResult,
    therm_kill_asserted: bool,
    force_min_charge: bool,
    keep_charger_on_bms_missing: bool,
    defer_bms_probe: bool,
) -> BootSelfTestResult
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    boot_self_test_with_report(
        i2c,
        desired_outputs,
        vout_mv,
        ilimit_ma,
        include_vin_ch3,
        tmp_out_a_ok,
        tmp_out_b_ok,
        sync_ok,
        panel_probe,
        therm_kill_asserted,
        force_min_charge,
        keep_charger_on_bms_missing,
        defer_bms_probe,
        |_, _| {},
    )
}

pub fn boot_self_test_with_report<I2C, F>(
    i2c: &mut I2C,
    desired_outputs: EnabledOutputs,
    vout_mv: u16,
    ilimit_ma: u16,
    include_vin_ch3: bool,
    tmp_out_a_ok: bool,
    tmp_out_b_ok: bool,
    sync_ok: bool,
    panel_probe: PanelProbeResult,
    therm_kill_asserted: bool,
    force_min_charge: bool,
    keep_charger_on_bms_missing: bool,
    defer_bms_probe: bool,
    mut reporter: F,
) -> BootSelfTestResult
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
    F: FnMut(SelfCheckStage, SelfCheckUiSnapshot),
{
    defmt::info!(
        "self_test: begin vout_mv={=u16} ilimit_ma={=u16} tmp_a_ok={=bool} tmp_b_ok={=bool} sync_ok={=bool} screen_present={=bool} therm_kill_asserted={=bool} force_min_charge={=bool} keep_charger_on_bms_missing={=bool}",
        vout_mv,
        ilimit_ma,
        tmp_out_a_ok,
        tmp_out_b_ok,
        sync_ok,
        panel_probe.screen_present(),
        therm_kill_asserted,
        force_min_charge,
        keep_charger_on_bms_missing
    );

    let mut ui = SelfCheckUiSnapshot::pending(UpsMode::Standby);
    ui.gc9307 = if panel_probe.screen_present() {
        SelfCheckCommState::Ok
    } else {
        SelfCheckCommState::Err
    };
    ui.tca6408a = if panel_probe.tca6408_present {
        SelfCheckCommState::Ok
    } else {
        SelfCheckCommState::Err
    };
    ui.fusb302 = if panel_probe.fusb302_present {
        SelfCheckCommState::Ok
    } else {
        SelfCheckCommState::Err
    };
    reporter(SelfCheckStage::Begin, ui);

    // Stage 0: configure independent sensors.
    let ina_cfg = if include_vin_ch3 {
        ina3221::CONFIG_VALUE_CH123
    } else {
        ina3221::CONFIG_VALUE_CH12
    };
    let _ = ina3221::init_with_config(&mut *i2c, 0x8000).map_err(|e| {
        defmt::warn!("self_test: ina3221 reset err={}", ina_error_kind(e));
    });
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(2) {}
    let ina_ready = match ina3221::init_with_config(&mut *i2c, ina_cfg) {
        Ok(()) => {
            defmt::info!("self_test: ina3221 ready cfg=0x{=u16:x}", ina_cfg);
            true
        }
        Err(e) => {
            defmt::error!("self_test: ina3221 init err={}", ina_error_kind(e));
            false
        }
    };

    let tmp_a_read = tmp112::read_temp_c_x16(&mut *i2c, OutputChannel::OutA.tmp_addr());
    let tmp_b_read = tmp112::read_temp_c_x16(&mut *i2c, OutputChannel::OutB.tmp_addr());
    let tmp_a_present = tmp_a_read.is_ok();
    let tmp_b_present = tmp_b_read.is_ok();
    defmt::info!(
        "self_test: sensors ina_ready={=bool} tmp_a_present={=bool} tmp_b_present={=bool} tmp_a_cfg_ok={=bool} tmp_b_cfg_ok={=bool}",
        ina_ready,
        tmp_a_present,
        tmp_b_present,
        tmp_out_a_ok,
        tmp_out_b_ok
    );

    ui.ina3221 = if ina_ready {
        SelfCheckCommState::Ok
    } else {
        SelfCheckCommState::Err
    };
    if ina_ready && include_vin_ch3 {
        ui.vin_vbus_mv = ina3221::read_bus_mv(&mut *i2c, ina3221::Channel::Ch3)
            .ok()
            .and_then(|mv| u16::try_from(mv).ok());
        ui.vin_mains_present = mains_present_from_vin(ui.vin_vbus_mv);
        ui.vin_iin_ma = ina3221::read_shunt_uv(&mut *i2c, ina3221::Channel::Ch3)
            .ok()
            .map(|shunt_uv| ina3221::shunt_uv_to_current_ma(shunt_uv, 7));
    }
    ui.tmp_a = if tmp_a_present && tmp_out_a_ok {
        SelfCheckCommState::Ok
    } else if tmp_a_present {
        SelfCheckCommState::Warn
    } else {
        SelfCheckCommState::Err
    };
    ui.tmp_b = if tmp_b_present && tmp_out_b_ok {
        SelfCheckCommState::Ok
    } else if tmp_b_present {
        SelfCheckCommState::Warn
    } else {
        SelfCheckCommState::Err
    };
    ui.tmp_a_c_x16 = tmp_a_read.ok();
    ui.tmp_a_c = ui.tmp_a_c_x16.map(|v| v / 16);
    ui.tmp_b_c_x16 = tmp_b_read.ok();
    ui.tmp_b_c = ui.tmp_b_c_x16.map(|v| v / 16);
    reporter(SelfCheckStage::Sensors, ui);

    // Stage 1: screen module presence (already probed on I2C2 before entering this function).
    if panel_probe.screen_present() {
        defmt::info!("self_test: stage=screen result=present");
    } else {
        defmt::warn!("self_test: stage=screen result=missing action=disable_screen_module_only");
    }
    reporter(SelfCheckStage::Screen, ui);

    // Stage 2: BQ40Z50.
    let mut bms_addr: Option<u8> = None;
    let mut bms_voltage_mv: Option<u16> = None;
    let mut bms_current_ma: Option<i16> = None;
    let mut bms_soc_pct: Option<u16> = None;
    let mut bms_rca_alarm: Option<bool> = None;
    let mut bms_no_battery: Option<bool> = None;
    let mut bms_discharge_ready: Option<bool> = None;
    let mut bms_discharge_reason: Option<&'static str> = None;
    let mut bms_charge_ready: Option<bool> = None;
    let mut bms_charge_reason: Option<&'static str> = None;
    let mut bms_flow: Option<&'static str> = None;
    let mut bms_primary_reason: Option<&'static str> = None;
    let mut initial_bms_protection_active = false;
    if defer_bms_probe {
        defmt::info!(
            "self_test: bq40z50 probe deferred until activation auto_request settle_ms={=u64}",
            BMS_ACTIVATION_AUTO_DELAY.as_millis() as u64
        );
        ui.bq40z50 = SelfCheckCommState::Err;
    } else {
        for addr in bms_probe_candidates().iter().copied() {
            let temp = bq40z50::read_u16(&mut *i2c, addr, bq40z50::cmd::TEMPERATURE);
            let voltage = bq40z50::read_u16(&mut *i2c, addr, bq40z50::cmd::VOLTAGE);
            let current = bq40z50::read_i16(&mut *i2c, addr, bq40z50::cmd::CURRENT);
            let soc = bq40z50::read_u16(&mut *i2c, addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE);
            let status = bq40z50::read_u16(&mut *i2c, addr, bq40z50::cmd::BATTERY_STATUS);
            let op_status = bq40z50::read_operation_status(&mut *i2c, addr)
                .ok()
                .flatten();

            if let (Ok(temp_k_x10), Ok(voltage_mv), Ok(current_ma), Ok(soc_pct), Ok(status_raw)) =
                (temp, voltage, current, soc, status)
            {
                let temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(temp_k_x10);
                let err_code = bq40z50::battery_status::error_code(status_raw);
                let xchg = bq40_op_bit(op_status, bq40z50::operation_status::XCHG);
                let xdsg = bq40_op_bit(op_status, bq40z50::operation_status::XDSG);
                let chg_fet = bq40_op_bit(op_status, bq40z50::operation_status::CHG);
                let dsg_fet = bq40_op_bit(op_status, bq40z50::operation_status::DSG);
                let (charge_ready, charge_reason) = bq40_decode_charge_path(op_status);
                let (discharge_ready, discharge_reason) = bq40_decode_discharge_path(op_status);
                let flow = bq40_decode_current_flow(current_ma);
                let flow_abs_ma = current_ma.wrapping_abs() as u16;
                let primary_reason =
                    bq40_primary_reason(status_raw, op_status, charge_reason, discharge_reason);
                let protection_active = bq40_op_bit(op_status, bq40z50::operation_status::PF)
                    == Some(true)
                    || err_code != 0
                    || (status_raw
                        & (bq40z50::battery_status::OCA
                            | bq40z50::battery_status::TCA
                            | bq40z50::battery_status::OTA
                            | bq40z50::battery_status::TDA))
                        != 0;
                defmt::info!(
                    "self_test: bq40z50 ok addr=0x{=u8:x} temp_c_x10={=i32} voltage_mv={=u16} current_ma={=i16} flow={} flow_abs_ma={=u16} soc_pct={=u16} status=0x{=u16:x} op_status={=?} xchg={=?} xdsg={=?} chg_fet={=?} dsg_fet={=?} chg_ready={=?} dsg_ready={=?} chg_reason={} dsg_reason={} primary_reason={} err_code={} err_str={}",
                    addr,
                    temp_c_x10,
                    voltage_mv,
                    current_ma,
                    flow,
                    flow_abs_ma,
                    soc_pct,
                    status_raw,
                    op_status,
                    xchg,
                    xdsg,
                    chg_fet,
                    dsg_fet,
                    charge_ready,
                    discharge_ready,
                    charge_reason,
                    discharge_reason,
                    primary_reason,
                    err_code,
                    bq40z50::decode_error_code(err_code)
                );
                bms_addr = Some(addr);
                initial_bms_protection_active = protection_active;
                bms_voltage_mv = Some(voltage_mv);
                bms_current_ma = Some(current_ma);
                bms_soc_pct = Some(soc_pct);
                bms_rca_alarm = Some((status_raw & bq40z50::battery_status::RCA) != 0);
                let no_battery_confirmed = bq40_self_test_no_battery_confirmed(
                    &mut *i2c, addr, temp_k_x10, voltage_mv, current_ma, soc_pct, status_raw,
                );
                if bq40_pack_indicates_no_battery(voltage_mv) && !no_battery_confirmed {
                    defmt::info!(
                        "self_test: bq40z50 low_pack candidate rejected addr=0x{=u8:x} voltage_mv={=u16} current_ma={=i16} soc_pct={=u16} status=0x{=u16:x}",
                        addr,
                        voltage_mv,
                        current_ma,
                        soc_pct,
                        status_raw
                    );
                }
                bms_no_battery = Some(no_battery_confirmed);
                bms_charge_ready = charge_ready;
                bms_charge_reason = Some(charge_reason);
                bms_discharge_ready = discharge_ready;
                bms_discharge_reason = Some(discharge_reason);
                bms_flow = Some(flow);
                bms_primary_reason = Some(primary_reason);
                break;
            }

            defmt::warn!("self_test: bq40z50 miss addr=0x{=u8:x}", addr);
        }

        if bms_addr.is_none() {
            defmt::warn!("self_test: bq40z50 missing/err; battery module disabled");
            ui.bq40z50 = SelfCheckCommState::Err;
        } else if bms_no_battery == Some(true) {
            defmt::warn!(
                "self_test: bq40z50 no battery voltage_mv={=?} flow={=?} primary_reason={=?}",
                bms_voltage_mv,
                bms_flow,
                bms_primary_reason
            );
            ui.bq40z50 = SelfCheckCommState::Warn;
        } else if bms_discharge_ready != Some(true) {
            defmt::warn!(
                "self_test: bq40z50 discharge path not ready state={=?} reason={=?} charge_ready={=?} charge_reason={=?} flow={=?} primary_reason={=?}",
                bms_discharge_ready,
                bms_discharge_reason,
                bms_charge_ready,
                bms_charge_reason,
                bms_flow,
                bms_primary_reason
            );
            ui.bq40z50 = SelfCheckCommState::Warn;
        } else if bms_rca_alarm == Some(true) {
            defmt::warn!(
                "self_test: bq40z50 remaining capacity alarm flow={=?} primary_reason={=?}",
                bms_flow,
                bms_primary_reason
            );
            ui.bq40z50 = SelfCheckCommState::Warn;
        } else {
            ui.bq40z50 = SelfCheckCommState::Ok;
        }
    }
    ui.bq40z50_pack_mv = bms_voltage_mv;
    ui.bq40z50_current_ma = bms_current_ma;
    ui.bq40z50_soc_pct = bms_soc_pct;
    ui.bq40z50_rca_alarm = bms_rca_alarm;
    ui.bq40z50_no_battery = bms_no_battery;
    ui.bq40z50_discharge_ready = bms_discharge_ready;
    reporter(SelfCheckStage::Bms, ui);

    // Stage 3: BQ25792.
    let mut charger_ctrl0: Option<u8> = None;
    let mut charger_status0: Option<u8> = None;
    let mut charger_enabled = match bq25792::read_u8(&mut *i2c, bq25792::reg::CHARGER_CONTROL_0) {
        Ok(v) => {
            charger_ctrl0 = Some(v);
            defmt::info!("self_test: bq25792 ok ctrl0=0x{=u8:x}", v);
            true
        }
        Err(e) => {
            defmt::warn!(
                "self_test: bq25792 miss err={} action=disable_charger_module",
                i2c_error_kind(e)
            );
            false
        }
    };
    let mut charger_vbat_present: Option<bool> = None;
    let mut charger_vbus_adc_mv: Option<u16> = None;
    let mut charger_ibus_adc_ma: Option<i32> = None;
    let mut initial_audio_charge_phase = AudioChargePhase::Unknown;
    if charger_enabled {
        charger_status0 = bq25792::read_u8(&mut *i2c, bq25792::reg::CHARGER_STATUS_0).ok();
        let charger_status1 = bq25792::read_u8(&mut *i2c, bq25792::reg::CHARGER_STATUS_1).ok();
        let charger_status2 = bq25792::read_u8(&mut *i2c, bq25792::reg::CHARGER_STATUS_2).ok();
        let charger_status3 = bq25792::read_u8(&mut *i2c, bq25792::reg::CHARGER_STATUS_3).ok();
        let adc_state = bq25792::ensure_adc_power_path(&mut *i2c).ok();
        let charger_vbus_adc_mv_local = bq25792::read_u16(&mut *i2c, bq25792::reg::VBUS_ADC).ok();
        let charger_ibus_adc_ma_local = bq25792::read_i16(&mut *i2c, bq25792::reg::IBUS_ADC).ok();
        let charger_vbat_adc_mv = bq25792::read_u16(&mut *i2c, bq25792::reg::VBAT_ADC).ok();
        let charger_vsys_adc_mv = bq25792::read_u16(&mut *i2c, bq25792::reg::VSYS_ADC).ok();

        if let Some(status1) = charger_status1 {
            initial_audio_charge_phase =
                audio_charge_phase_from_chg_stat(bq25792::status1::chg_stat(status1));
        }
        let vbat_present = charger_status2.map(|v| (v & bq25792::status2::VBAT_PRESENT_STAT) != 0);
        charger_vbat_present = vbat_present;
        let vsys_min_reg = charger_status3.map(|v| (v & bq25792::status3::VSYS_STAT) != 0);
        let input_present = charger_status0
            .map(|status0| {
                (status0 & bq25792::status0::VBUS_PRESENT_STAT) != 0
                    || (status0 & bq25792::status0::AC1_PRESENT_STAT) != 0
                    || (status0 & bq25792::status0::AC2_PRESENT_STAT) != 0
                    || (status0 & bq25792::status0::PG_STAT) != 0
            })
            .unwrap_or(false);
        let adc_ready = match (adc_state, charger_status3) {
            (Some(adc_state), Some(status3)) => bq25792::power_path_adc_ready(adc_state, status3),
            _ => false,
        };
        let input_sample = normalize_charger_input_power_sample(
            input_present,
            adc_ready,
            charger_vbus_adc_mv_local,
            charger_ibus_adc_ma_local,
        );
        charger_vbus_adc_mv = input_sample.ui_vbus_mv;
        charger_ibus_adc_ma = input_sample.ui_ibus_ma;
        defmt::info!(
            "self_test: bq25792 ctrl0={=?} status0={=?} status1={=?} status2={=?} status3={=?} vbat_present={=?} phase={} vsys_min_reg={=?} vbus_adc_mv={=?} ibus_adc_ma={=?} ui_vbus_mv={=?} ui_ibus_ma={=?} adc_ready={=bool} vbat_adc_mv={=?} vsys_adc_mv={=?}",
            charger_ctrl0,
            charger_status0,
            charger_status1,
            charger_status2,
            charger_status3,
            vbat_present,
            bq25792::decode_chg_stat(
                charger_status1
                    .map(bq25792::status1::chg_stat)
                    .unwrap_or_default()
            ),
            vsys_min_reg,
            charger_vbus_adc_mv_local,
            charger_ibus_adc_ma_local,
            input_sample.ui_vbus_mv,
            input_sample.ui_ibus_ma,
            adc_ready,
            charger_vbat_adc_mv,
            charger_vsys_adc_mv
        );
    }
    ui.bq25792 = if charger_enabled {
        SelfCheckCommState::Ok
    } else {
        SelfCheckCommState::Err
    };
    ui.bq25792_ichg_ma = bq25792::read_u16(&mut *i2c, bq25792::reg::CHARGE_CURRENT_LIMIT)
        .ok()
        .map(|v| (v & 0x01ff) * 10);
    ui.bq25792_vbat_present = charger_vbat_present;
    ui.input_vbus_mv = charger_vbus_adc_mv;
    ui.input_ibus_ma = charger_ibus_adc_ma;
    if let Some(status0) = charger_status0 {
        let vbus_present = (status0 & bq25792::status0::VBUS_PRESENT_STAT) != 0
            || (status0 & bq25792::status0::AC1_PRESENT_STAT) != 0
            || (status0 & bq25792::status0::AC2_PRESENT_STAT) != 0
            || (status0 & bq25792::status0::PG_STAT) != 0;
        ui.fusb302_vbus_present = Some(vbus_present);
    }
    let charger_probe_ok = charger_enabled;
    reporter(SelfCheckStage::Charger, ui);

    // Stage 4: TPS55288.
    let tps_a_present = ::tps55288::Tps55288::with_address(&mut *i2c, OutputChannel::OutA.addr())
        .read_reg(::tps55288::registers::addr::MODE)
        .is_ok();
    let tps_b_present = ::tps55288::Tps55288::with_address(&mut *i2c, OutputChannel::OutB.addr())
        .read_reg(::tps55288::registers::addr::MODE)
        .is_ok();
    let status_a = if tps_a_present {
        ::tps55288::Tps55288::with_address(&mut *i2c, OutputChannel::OutA.addr())
            .read_reg(::tps55288::registers::addr::STATUS)
            .map_err(tps_error_kind)
    } else {
        Err("not_present")
    };
    let status_b = if tps_b_present {
        ::tps55288::Tps55288::with_address(&mut *i2c, OutputChannel::OutB.addr())
            .read_reg(::tps55288::registers::addr::STATUS)
            .map_err(tps_error_kind)
    } else {
        Err("not_present")
    };

    let tps_a_fault = matches!(
        &status_a,
        Ok(v)
            if ::tps55288::registers::StatusBits::from_bits_truncate(*v).intersects(
                ::tps55288::registers::StatusBits::SCP
                    | ::tps55288::registers::StatusBits::OCP
                    | ::tps55288::registers::StatusBits::OVP
            )
    );
    let initial_tps_a_over_voltage = matches!(
        &status_a,
        Ok(v)
            if ::tps55288::registers::StatusBits::from_bits_truncate(*v)
                .contains(::tps55288::registers::StatusBits::OVP)
    );
    let initial_tps_b_over_voltage = matches!(
        &status_b,
        Ok(v)
            if ::tps55288::registers::StatusBits::from_bits_truncate(*v)
                .contains(::tps55288::registers::StatusBits::OVP)
    );
    let initial_tps_a_over_current = matches!(
        &status_a,
        Ok(v)
            if ::tps55288::registers::StatusBits::from_bits_truncate(*v).intersects(
                ::tps55288::registers::StatusBits::OCP
                    | ::tps55288::registers::StatusBits::SCP
            )
    );
    let initial_tps_b_over_current = matches!(
        &status_b,
        Ok(v)
            if ::tps55288::registers::StatusBits::from_bits_truncate(*v).intersects(
                ::tps55288::registers::StatusBits::OCP
                    | ::tps55288::registers::StatusBits::SCP
            )
    );
    let detected_tmp_outputs = enabled_outputs_from_flags(tmp_a_present, tmp_b_present);
    let detected_tps_outputs = enabled_outputs_from_flags(tps_a_present, tps_b_present);
    let tps_b_fault = matches!(
        &status_b,
        Ok(v)
            if ::tps55288::registers::StatusBits::from_bits_truncate(*v).intersects(
                ::tps55288::registers::StatusBits::SCP
                    | ::tps55288::registers::StatusBits::OCP
                    | ::tps55288::registers::StatusBits::OVP
            )
    );

    let mut out_a_allowed = desired_outputs.is_enabled(OutputChannel::OutA)
        && sync_ok
        && ina_ready
        && tps_a_present
        && status_a.is_ok()
        && !tps_a_fault
        && tmp_a_present
        && tmp_out_a_ok;
    let mut out_b_allowed = desired_outputs.is_enabled(OutputChannel::OutB)
        && sync_ok
        && ina_ready
        && tps_b_present
        && status_b.is_ok()
        && !tps_b_fault
        && tmp_b_present
        && tmp_out_b_ok;
    let mut recoverable_outputs = enabled_outputs_from_flags(out_a_allowed, out_b_allowed);
    let mut output_gate_reason = OutputGateReason::None;

    if desired_outputs.is_enabled(OutputChannel::OutA) && !out_a_allowed {
        defmt::warn!(
            "self_test: tps out_a disabled sync_ok={=bool} ina_ready={=bool} tps_present={=bool} status={=?} fault={=bool} tmp_present={=bool} tmp_cfg_ok={=bool}",
            sync_ok,
            ina_ready,
            tps_a_present,
            status_a,
            tps_a_fault,
            tmp_a_present,
            tmp_out_a_ok
        );
    }
    if desired_outputs.is_enabled(OutputChannel::OutB) && !out_b_allowed {
        defmt::warn!(
            "self_test: tps out_b disabled sync_ok={=bool} ina_ready={=bool} tps_present={=bool} status={=?} fault={=bool} tmp_present={=bool} tmp_cfg_ok={=bool}",
            sync_ok,
            ina_ready,
            tps_b_present,
            status_b,
            tps_b_fault,
            tmp_b_present,
            tmp_out_b_ok
        );
    }

    if bms_addr.is_none() || bms_discharge_ready != Some(true) {
        // Policy: when BMS comm is missing or discharge path is not ready, keep TPS outputs off.
        if recoverable_outputs != EnabledOutputs::None {
            output_gate_reason = OutputGateReason::BmsNotReady;
        }
        out_a_allowed = false;
        out_b_allowed = false;

        if bms_addr.is_none() {
            if force_min_charge {
                defmt::warn!(
                    "self_test: bq40z50 missing; keep charger module for force_min_charge (charger_probe_ok={=bool})",
                    charger_enabled
                );
            } else if keep_charger_on_bms_missing {
                defmt::info!(
                    "self_test: bq40z50 missing; keep charger module for boot_diag_auto_validate (charger_probe_ok={=bool})",
                    charger_enabled
                );
            } else {
                if charger_enabled {
                    defmt::warn!("self_test: force disable charger because bq40z50 is missing");
                }
                charger_enabled = false;
            }
        } else {
            defmt::warn!(
                "self_test: bq40z50 discharge path not ready; block tps until activation/recovery"
            );
        }
    }

    // Emergency-stop path: only this path is allowed to change TPS output state in self-test.
    if therm_kill_asserted || tps_a_fault || tps_b_fault {
        defmt::error!(
            "self_test: emergency_stop therm_kill_asserted={=bool} tps_a_fault={=bool} tps_b_fault={=bool}",
            therm_kill_asserted,
            tps_a_fault,
            tps_b_fault
        );
        if tps_a_present {
            if let Err(e) =
                ::tps55288::Tps55288::with_address(&mut *i2c, OutputChannel::OutA.addr())
                    .disable_output()
            {
                defmt::warn!(
                    "self_test: emergency out_a disable err={}",
                    tps_error_kind(e)
                );
            }
        }
        if tps_b_present {
            if let Err(e) =
                ::tps55288::Tps55288::with_address(&mut *i2c, OutputChannel::OutB.addr())
                    .disable_output()
            {
                defmt::warn!(
                    "self_test: emergency out_b disable err={}",
                    tps_error_kind(e)
                );
            }
        }
        out_a_allowed = false;
        out_b_allowed = false;
        recoverable_outputs = EnabledOutputs::None;
        output_gate_reason = if therm_kill_asserted {
            OutputGateReason::ThermKill
        } else {
            OutputGateReason::TpsFault
        };
    }

    ui.tps_a = if tps_a_present {
        if tps_a_fault {
            SelfCheckCommState::Warn
        } else if status_a.is_ok() {
            SelfCheckCommState::Ok
        } else {
            SelfCheckCommState::Err
        }
    } else {
        SelfCheckCommState::Err
    };
    ui.tps_b = if tps_b_present {
        if tps_b_fault {
            SelfCheckCommState::Warn
        } else if status_b.is_ok() {
            SelfCheckCommState::Ok
        } else {
            SelfCheckCommState::Err
        }
    } else {
        SelfCheckCommState::Err
    };
    ui.tps_a_enabled = Some(out_a_allowed);
    ui.tps_b_enabled = Some(out_b_allowed);
    ui.bq25792_allow_charge = Some(charger_enabled);
    reporter(SelfCheckStage::Tps, ui);

    let enabled_outputs = enabled_outputs_from_flags(out_a_allowed, out_b_allowed);

    ui.mode = ups_mode_from_mains(
        stable_mains_present(
            ui.vin_mains_present,
            ui.vin_vbus_mv,
            ui.fusb302_vbus_present,
        ),
        out_a_allowed || out_b_allowed,
    );

    defmt::info!(
        "self_test: done requested_outputs={} active_outputs={} recoverable_outputs={} gate_reason={} charger_enabled={=bool} bms_present={=bool}",
        desired_outputs.describe(),
        enabled_outputs.describe(),
        recoverable_outputs.describe(),
        output_gate_reason.as_str(),
        charger_enabled,
        bms_addr.is_some()
    );

    reporter(SelfCheckStage::Done, ui);

    BootSelfTestResult {
        ina_detected: ina_ready,
        detected_tmp_outputs,
        detected_tps_outputs,
        requested_outputs: enabled_outputs_from_flags(
            enabled_outputs.is_enabled(OutputChannel::OutA)
                || recoverable_outputs.is_enabled(OutputChannel::OutA),
            enabled_outputs.is_enabled(OutputChannel::OutB)
                || recoverable_outputs.is_enabled(OutputChannel::OutB),
        ),
        active_outputs: enabled_outputs,
        recoverable_outputs,
        output_gate_reason,
        charger_probe_ok,
        charger_enabled,
        initial_audio_charge_phase,
        initial_bms_protection_active,
        initial_tps_a_over_voltage,
        initial_tps_b_over_voltage,
        initial_tps_a_over_current,
        initial_tps_b_over_current,
        bms_addr,
        self_check_snapshot: ui,
    }
}

pub struct PowerManager<'d, I2C> {
    i2c: I2C,
    i2c1_int: Input<'d>,
    bms_btp_int_h: Input<'d>,
    therm_kill: Flex<'d>,
    chg_ce: Flex<'d>,
    chg_ilim_hiz_brk: Flex<'d>,

    cfg: Config,

    next_telemetry_at: Instant,
    next_vin_telemetry_skip_at: Instant,
    next_fan_temp_refresh_at: Instant,
    last_fault_log_at: Option<Instant>,
    last_input_power_anomaly_log_at: Option<Instant>,
    last_therm_kill_hint_at: Option<Instant>,
    fan_started_at: Instant,
    fan_rpm_tracker: FanRpmTracker,

    ina_ready: bool,
    ina_next_retry_at: Option<Instant>,

    tps_a_ready: bool,
    tps_a_next_retry_at: Option<Instant>,
    tps_b_ready: bool,
    tps_b_next_retry_at: Option<Instant>,

    bms_addr: Option<u8>,
    bms_runtime_seen: bool,
    bms_next_poll_at: Instant,
    bms_next_retry_at: Option<Instant>,
    bms_last_int_poll_at: Option<Instant>,
    bms_poll_seq: u32,
    bms_ok_streak: u16,
    bms_err_streak: u16,
    bms_cached_da_status2: Option<bq40z50::DaStatus2>,
    bms_cached_filter_capacity: Option<bq40z50::FilterCapacity>,
    bms_next_da_status2_refresh_at: Instant,
    bms_next_filter_capacity_refresh_at: Instant,

    chg_next_poll_at: Instant,
    chg_next_retry_at: Option<Instant>,
    chg_enabled: bool,
    charger_allowed: bool,
    chg_last_int_poll_at: Option<Instant>,
    bms_activation_state: BmsActivationState,
    bms_activation_phase: BmsActivationPhase,
    bms_activation_started_at: Option<Instant>,
    bms_activation_deadline: Option<Instant>,
    bms_activation_diag_stage: usize,
    bms_activation_followup_next_at: Option<Instant>,
    bms_activation_followup_attempts: u16,
    bms_activation_exercise_next_at: Option<Instant>,
    bms_activation_pattern_tracker: Bq40ActivationPatternTracker,
    bms_activation_isolation_until: Option<Instant>,
    bms_activation_force_charge_requested: bool,
    bms_boot_diag_started_at: Instant,
    bms_boot_diag_ship_reset_attempted: bool,
    bms_activation_auto_due_at: Instant,
    bms_activation_auto_poll_release_at: Instant,
    bms_activation_auto_attempted: bool,
    bms_activation_current_is_auto: bool,
    bms_activation_auto_force_charge_until: Option<Instant>,
    bms_activation_auto_force_charge_programmed: bool,
    bms_activation_auto_defer_logged: bool,
    bms_activation_backup: Option<ChargerActivationBackup>,
    chg_watchdog_restore: Option<u8>,
    output_state: OutputRuntimeState,
    output_protection: output_protection::ProtectionRuntime,
    fan: fan::Controller,
    vin_sample_missing_streak: u8,

    ui_snapshot: SelfCheckUiSnapshot,
    audio_snapshot: AudioSignalSnapshot,
    audio_events: AudioSignalEvents,
    audio_signals_ready: bool,
    charger_audio: ChargerAudioState,
    bms_audio: BmsAudioState,
    tps_audio: TpsAudioState,
}

#[derive(Clone, Copy)]
struct ChargerActivationBackup {
    ctrl0: u8,
    vreg_reg: u16,
    ichg_reg: u16,
    iindpm_reg: u16,
    chg_enabled: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BmsActivationPhase {
    ProbeWithoutCharge,
    WaitChargeOff,
    WaitMinChargeSettle,
    MinChargeProbe,
    WakeProbe,
}

fn bms_activation_phase_name(phase: BmsActivationPhase) -> &'static str {
    match phase {
        BmsActivationPhase::ProbeWithoutCharge => "probe_without_charge",
        BmsActivationPhase::WaitChargeOff => "wait_charge_off",
        BmsActivationPhase::WaitMinChargeSettle => "wait_min_charge_settle",
        BmsActivationPhase::MinChargeProbe => "min_charge_probe",
        BmsActivationPhase::WakeProbe => "wake_probe",
    }
}

fn bms_activation_phase_allows_force_charge(phase: BmsActivationPhase) -> bool {
    matches!(
        phase,
        BmsActivationPhase::WaitMinChargeSettle
            | BmsActivationPhase::MinChargeProbe
            | BmsActivationPhase::WakeProbe
    )
}

fn bms_activation_phase_forces_charge_off(phase: BmsActivationPhase) -> bool {
    matches!(phase, BmsActivationPhase::WaitChargeOff)
}

#[derive(Clone, Copy)]
pub struct Config {
    pub ina_detected: bool,
    pub detected_tmp_outputs: EnabledOutputs,
    pub detected_tps_outputs: EnabledOutputs,
    pub requested_outputs: EnabledOutputs,
    pub active_outputs: EnabledOutputs,
    pub recoverable_outputs: EnabledOutputs,
    pub output_gate_reason: OutputGateReason,
    pub vout_mv: u16,
    pub ilimit_ma: u16,
    pub telemetry_period: Duration,
    pub retry_backoff: Duration,
    pub fault_log_min_interval: Duration,
    pub telemetry_include_vin_ch3: bool,
    pub tmp112_tlow_c_x16: i16,
    pub tmp112_thigh_c_x16: i16,
    pub protect_temp_derate_c_x16: i16,
    pub protect_temp_resume_c_x16: i16,
    pub protect_temp_hold: Duration,
    pub protect_current_derate_ma: i32,
    pub protect_current_resume_ma: i32,
    pub protect_current_hold: Duration,
    pub protect_ilim_step_ma: u16,
    pub protect_ilim_step_interval: Duration,
    pub protect_min_ilim_ma: u16,
    pub protect_shutdown_vout_mv: u16,
    pub protect_shutdown_hold: Duration,
    pub fan_config: fan::Config,
    pub charger_probe_ok: bool,
    pub charger_enabled: bool,
    pub initial_audio_charge_phase: AudioChargePhase,
    pub initial_bms_protection_active: bool,
    pub initial_tps_a_over_voltage: bool,
    pub initial_tps_b_over_voltage: bool,
    pub initial_tps_a_over_current: bool,
    pub initial_tps_b_over_current: bool,
    pub force_min_charge: bool,
    pub bms_boot_diag_auto_validate: bool,
    pub bms_addr: Option<u8>,
    pub self_check_snapshot: SelfCheckUiSnapshot,
}

#[derive(Clone, Copy)]
pub struct AppliedFanState {
    pub command: fan::FanLevel,
    pub pwm_pct: u8,
    pub degraded: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioChargePhase {
    Unknown,
    NotCharging,
    Charging,
    Completed,
}

impl Default for AudioChargePhase {
    fn default() -> Self {
        Self::Unknown
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioBatteryLowState {
    Unknown,
    Inactive,
    WithMains,
    NoMains,
}

impl Default for AudioBatteryLowState {
    fn default() -> Self {
        Self::Unknown
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioSignalSnapshot {
    pub mains_present: Option<bool>,
    pub mains_source: AudioMainsSource,
    pub charge_phase: AudioChargePhase,
    pub thermal_stress: bool,
    pub battery_low: AudioBatteryLowState,
    pub battery_protection: bool,
    pub module_fault: bool,
    pub io_over_voltage: bool,
    pub io_over_current: bool,
    pub shutdown_protection: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioSignalEvents {
    pub mains_present_changed: Option<bool>,
    pub charge_phase_changed: Option<AudioChargePhase>,
    pub thermal_stress_changed: Option<bool>,
    pub battery_low_changed: Option<AudioBatteryLowState>,
    pub battery_protection_changed: Option<bool>,
    pub module_fault_changed: Option<bool>,
    pub io_over_voltage_changed: Option<bool>,
    pub io_over_current_changed: Option<bool>,
    pub shutdown_protection_changed: Option<bool>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ChargerAudioState {
    input_present: Option<bool>,
    phase: AudioChargePhase,
    thermal_stress: bool,
    over_voltage: bool,
    over_current: bool,
    shutdown_protection: bool,
    module_fault: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChargerInputSampleIssue {
    AdcNotReady,
    VbusMissing,
    VbusOutOfRange,
    IbusMissing,
    IbusOutOfRange,
}

impl ChargerInputSampleIssue {
    const fn as_str(self) -> &'static str {
        match self {
            Self::AdcNotReady => "adc_not_ready",
            Self::VbusMissing => "vbus_missing",
            Self::VbusOutOfRange => "vbus_out_of_range",
            Self::IbusMissing => "ibus_missing",
            Self::IbusOutOfRange => "ibus_out_of_range",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ChargerInputPowerSample {
    raw_vbus_mv: Option<u16>,
    raw_ibus_ma: Option<i16>,
    ui_vbus_mv: Option<u16>,
    ui_ibus_ma: Option<i32>,
    raw_power_w10: Option<u32>,
    issue: Option<ChargerInputSampleIssue>,
}

fn normalize_charger_input_power_sample(
    input_present: bool,
    adc_ready: bool,
    raw_vbus_mv: Option<u16>,
    raw_ibus_ma: Option<i16>,
) -> ChargerInputPowerSample {
    let raw_power_w10 = match (raw_vbus_mv, raw_ibus_ma) {
        (Some(vbus_mv), Some(ibus_ma)) => {
            Some((vbus_mv as u32 * ibus_ma.unsigned_abs() as u32) / 100_000)
        }
        _ => None,
    };

    let mut sample = ChargerInputPowerSample {
        raw_vbus_mv,
        raw_ibus_ma,
        ui_vbus_mv: None,
        ui_ibus_ma: None,
        raw_power_w10,
        issue: None,
    };

    if !input_present {
        return sample;
    }

    if !adc_ready {
        sample.issue = Some(ChargerInputSampleIssue::AdcNotReady);
        return sample;
    }

    let vbus_mv = match raw_vbus_mv {
        Some(vbus_mv) if vbus_mv <= CHARGER_INPUT_VBUS_MAX_MV => vbus_mv,
        Some(_) => {
            sample.issue = Some(ChargerInputSampleIssue::VbusOutOfRange);
            return sample;
        }
        None => {
            sample.issue = Some(ChargerInputSampleIssue::VbusMissing);
            return sample;
        }
    };

    let ibus_ma = match raw_ibus_ma {
        Some(ibus_ma)
            if ibus_ma >= -CHARGER_INPUT_IBUS_MAX_MA && ibus_ma <= CHARGER_INPUT_IBUS_MAX_MA =>
        {
            ibus_ma
        }
        Some(_) => {
            sample.issue = Some(ChargerInputSampleIssue::IbusOutOfRange);
            return sample;
        }
        None => {
            sample.issue = Some(ChargerInputSampleIssue::IbusMissing);
            return sample;
        }
    };

    sample.ui_vbus_mv = Some(vbus_mv);
    sample.ui_ibus_ma = Some(if ibus_ma <= 0 { 0 } else { i32::from(ibus_ma) });
    sample
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct BmsAudioState {
    rca_alarm: Option<bool>,
    protection_active: bool,
    module_fault: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct TpsAudioState {
    out_a_over_voltage: bool,
    out_b_over_voltage: bool,
    out_a_over_current: bool,
    out_b_over_current: bool,
}

impl TpsAudioState {
    const fn any_over_voltage(self) -> bool {
        self.out_a_over_voltage || self.out_b_over_voltage
    }

    const fn any_over_current(self) -> bool {
        self.out_a_over_current || self.out_b_over_current
    }
}

impl<'d, I2C> PowerManager<'d, I2C>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    pub fn new(
        i2c: I2C,
        i2c1_int: Input<'d>,
        bms_btp_int_h: Input<'d>,
        therm_kill: Flex<'d>,
        mut chg_ce: Flex<'d>,
        mut chg_ilim_hiz_brk: Flex<'d>,
        cfg: Config,
    ) -> Self {
        let now = Instant::now();
        let output_state = OutputRuntimeState::new(
            cfg.requested_outputs,
            cfg.active_outputs,
            cfg.recoverable_outputs,
            cfg.output_gate_reason,
        );
        let outputs_allowed = output_state.requested_outputs != EnabledOutputs::None;
        let out_a_allowed = output_state.active_outputs.is_enabled(OutputChannel::OutA);
        let out_b_allowed = output_state.active_outputs.is_enabled(OutputChannel::OutB);
        let charger_allowed = cfg.charger_probe_ok;
        let bms_addr = cfg.bms_addr;
        let bms_runtime_seen = bms_addr.is_some()
            || output_state.gate_reason == OutputGateReason::BmsNotReady
            || (cfg.charger_probe_ok
                && matches!(cfg.self_check_snapshot.bq40z50, SelfCheckCommState::Err));

        // Fail-safe defaults.
        chg_ce.set_high();
        chg_ilim_hiz_brk.set_low();

        Self {
            i2c,
            i2c1_int,
            bms_btp_int_h,
            therm_kill,
            chg_ce,
            chg_ilim_hiz_brk,
            cfg,

            next_telemetry_at: now,
            next_vin_telemetry_skip_at: now,
            next_fan_temp_refresh_at: now,
            last_fault_log_at: None,
            last_input_power_anomaly_log_at: None,
            last_therm_kill_hint_at: None,
            fan_started_at: now,
            fan_rpm_tracker: FanRpmTracker::new(),

            ina_ready: false,
            ina_next_retry_at: if outputs_allowed { Some(now) } else { None },

            tps_a_ready: false,
            tps_a_next_retry_at: if out_a_allowed { Some(now) } else { None },
            tps_b_ready: false,
            tps_b_next_retry_at: if out_b_allowed { Some(now) } else { None },

            bms_addr,
            bms_runtime_seen,
            bms_next_poll_at: now,
            bms_next_retry_at: Some(now),
            bms_last_int_poll_at: None,
            bms_poll_seq: 0,
            bms_ok_streak: 0,
            bms_err_streak: 0,
            bms_cached_da_status2: None,
            bms_cached_filter_capacity: None,
            bms_next_da_status2_refresh_at: now,
            bms_next_filter_capacity_refresh_at: now + BMS_DETAIL_MAC_REFRESH_STAGGER,

            chg_next_poll_at: now,
            chg_next_retry_at: if charger_allowed { Some(now) } else { None },
            chg_enabled: false,
            charger_allowed,
            chg_last_int_poll_at: None,
            bms_activation_state: BmsActivationState::Idle,
            bms_activation_phase: BmsActivationPhase::WakeProbe,
            bms_activation_started_at: None,
            bms_activation_deadline: None,
            bms_activation_diag_stage: 0,
            bms_activation_followup_next_at: None,
            bms_activation_followup_attempts: 0,
            bms_activation_exercise_next_at: None,
            bms_activation_pattern_tracker: Bq40ActivationPatternTracker::new(),
            bms_activation_isolation_until: None,
            bms_activation_force_charge_requested: false,
            bms_boot_diag_started_at: now,
            bms_boot_diag_ship_reset_attempted: false,
            bms_activation_auto_due_at: now + BMS_ACTIVATION_AUTO_DELAY,
            bms_activation_auto_poll_release_at: now + BMS_ACTIVATION_AUTO_POLL_RELEASE_DELAY,
            bms_activation_auto_attempted: false,
            bms_activation_current_is_auto: false,
            bms_activation_auto_force_charge_until: if BMS_ACTIVATION_AUTO_BOOT_FORCE_CHARGE {
                Some(
                    now + BMS_ACTIVATION_AUTO_DELAY
                        + if BMS_BOOT_DIAG_TOOL_STYLE_PROBE_ONLY {
                            BMS_BOOT_DIAG_TOOL_STYLE_FORCE_HOLD
                        } else {
                            Duration::ZERO
                        },
                )
            } else {
                None
            },
            bms_activation_auto_force_charge_programmed: false,
            bms_activation_auto_defer_logged: false,
            bms_activation_backup: None,
            chg_watchdog_restore: None,
            output_state,
            output_protection: output_protection::ProtectionRuntime::new(cfg.ilimit_ma),
            fan: fan::Controller::new(cfg.fan_config),
            vin_sample_missing_streak: 0,
            ui_snapshot: cfg.self_check_snapshot,
            audio_snapshot: AudioSignalSnapshot::default(),
            audio_events: AudioSignalEvents::default(),
            audio_signals_ready: false,
            charger_audio: ChargerAudioState::default(),
            bms_audio: BmsAudioState::default(),
            tps_audio: TpsAudioState::default(),
        }
    }

    pub fn init_best_effort(&mut self) {
        let _ = bms_diag_breadcrumb_take();
        if self.output_state.requested_outputs != EnabledOutputs::None {
            self.try_init_ina();
            if self
                .output_state
                .active_outputs
                .is_enabled(OutputChannel::OutA)
            {
                self.try_configure_tps(OutputChannel::OutA);
            }
            if self
                .output_state
                .active_outputs
                .is_enabled(OutputChannel::OutB)
            {
                self.try_configure_tps(OutputChannel::OutB);
            }
        } else {
            defmt::warn!("power: outputs disabled (boot self-test)");
            self.force_disable_outputs();
        }

        if !self.charger_allowed {
            defmt::warn!("charger: bq25792 disabled (boot self-test)");
            self.chg_ce.set_high();
            self.chg_enabled = false;
        }

        if self.bms_addr.is_none() {
            defmt::warn!("bms: bq40z50 disabled (boot self-test)");
        }
        if self.output_state.gate_reason != OutputGateReason::None {
            defmt::warn!(
                "power: outputs gated reason={} (boot self-test)",
                self.output_state.gate_reason.as_str()
            );
        }

        if self.ui_snapshot.bq25792_allow_charge.is_none() {
            self.ui_snapshot.bq25792_allow_charge =
                Some(self.cfg.charger_enabled && self.charger_allowed);
        }
        if self.ui_snapshot.tps_a_enabled.is_none() {
            self.ui_snapshot.tps_a_enabled = Some(
                self.output_state
                    .active_outputs
                    .is_enabled(OutputChannel::OutA),
            );
        }
        if self.ui_snapshot.tps_b_enabled.is_none() {
            self.ui_snapshot.tps_b_enabled = Some(
                self.output_state
                    .active_outputs
                    .is_enabled(OutputChannel::OutB),
            );
        }
        self.charger_audio.input_present = self.ui_snapshot.fusb302_vbus_present;
        self.charger_audio.phase = self.cfg.initial_audio_charge_phase;
        self.charger_audio.module_fault =
            matches!(self.ui_snapshot.bq25792, SelfCheckCommState::Err);
        self.bms_audio.rca_alarm = self.ui_snapshot.bq40z50_rca_alarm;
        self.bms_audio.protection_active = self.cfg.initial_bms_protection_active;
        self.bms_audio.module_fault = matches!(self.ui_snapshot.bq40z50, SelfCheckCommState::Err);
        self.tps_audio.out_a_over_voltage = self.cfg.initial_tps_a_over_voltage;
        self.tps_audio.out_b_over_voltage = self.cfg.initial_tps_b_over_voltage;
        self.tps_audio.out_a_over_current = self.cfg.initial_tps_a_over_current;
        self.tps_audio.out_b_over_current = self.cfg.initial_tps_b_over_current;
        self.recompute_ui_mode();
        self.refresh_audio_signals();
    }

    fn force_disable_outputs(&mut self) {
        self.output_state.active_outputs = EnabledOutputs::None;
        self.tps_a_ready = false;
        self.tps_b_ready = false;
        self.tps_a_next_retry_at = None;
        self.tps_b_next_retry_at = None;
        self.ui_snapshot.tps_a_enabled = Some(false);
        self.ui_snapshot.tps_b_enabled = Some(false);
        self.ui_snapshot.out_a_vbus_mv = None;
        self.ui_snapshot.out_b_vbus_mv = None;
        self.ui_snapshot.tps_a_iout_ma = None;
        self.ui_snapshot.tps_b_iout_ma = None;

        let out_a = ::tps55288::Tps55288::with_address(&mut self.i2c, OutputChannel::OutA.addr())
            .disable_output()
            .map_err(tps_error_kind);
        let out_b = ::tps55288::Tps55288::with_address(&mut self.i2c, OutputChannel::OutB.addr())
            .disable_output()
            .map_err(tps_error_kind);

        defmt::info!(
            "power: force_disable_outputs out_a={=?} out_b={=?}",
            out_a,
            out_b
        );
        self.recompute_ui_mode();
    }

    fn maybe_log_charger_input_power_anomaly(
        &mut self,
        now: Instant,
        sample: ChargerInputPowerSample,
        adc_state: Option<bq25792::AdcState>,
        adc_ready: bool,
        status0: u8,
        status1: u8,
        status3: u8,
    ) {
        if sample.raw_power_w10.unwrap_or(0) <= CHARGER_INPUT_POWER_ANOMALY_W10 {
            return;
        }

        if !tps55288::should_log_fault(
            now,
            &mut self.last_input_power_anomaly_log_at,
            self.cfg.fault_log_min_interval,
        ) {
            return;
        }

        let adc_ctrl = adc_state.map(|state| state.ctrl).unwrap_or(0);
        defmt::warn!(
            "charger: input_power_anomaly raw_pin_w10={=u32} raw_ibus_adc_ma={=?} raw_vbus_adc_mv={=?} ui_ibus_ma={=?} ui_vbus_mv={=?} reason={} adc_ready={=bool} adc_ctrl=0x{=u8:x} adc_done={=bool} vbus_stat={} vbus_present={=bool} ac1_present={=bool} ac2_present={=bool} pg={=bool}",
            sample.raw_power_w10.unwrap_or(0),
            sample.raw_ibus_ma,
            sample.raw_vbus_mv,
            sample.ui_ibus_ma,
            sample.ui_vbus_mv,
            sample
                .issue
                .map(ChargerInputSampleIssue::as_str)
                .unwrap_or("none"),
            adc_ready,
            adc_ctrl,
            (status3 & bq25792::status3::ADC_DONE_STAT) != 0,
            bq25792::decode_vbus_stat(bq25792::status1::vbus_stat(status1)),
            (status0 & bq25792::status0::VBUS_PRESENT_STAT) != 0,
            (status0 & bq25792::status0::AC1_PRESENT_STAT) != 0,
            (status0 & bq25792::status0::AC2_PRESENT_STAT) != 0,
            (status0 & bq25792::status0::PG_STAT) != 0,
        );
    }

    pub fn tick(&mut self, irq: &IrqSnapshot) -> bool {
        if let Some(until) = self.bms_activation_isolation_until {
            if Instant::now() < until {
                self.note_skipped_vin_telemetry_if_due(Instant::now());
                self.update_fan_state(irq);
                self.refresh_audio_signals();
                return false;
            }
            self.bms_activation_isolation_until = None;
        }

        let activation_pending = self.bms_activation_state == BmsActivationState::Pending;
        if activation_pending {
            if matches!(
                self.bms_activation_phase,
                BmsActivationPhase::ProbeWithoutCharge
                    | BmsActivationPhase::WaitChargeOff
                    | BmsActivationPhase::WaitMinChargeSettle
                    | BmsActivationPhase::MinChargeProbe
                    | BmsActivationPhase::WakeProbe
            ) {
                self.maybe_poll_charger(irq);
            }
            let mut bms_i2c_active = false;
            if self.bms_activation_phase == BmsActivationPhase::MinChargeProbe {
                // Keep the regular strict snapshot poll alive during the min-charge observe
                // window. The historical passing path recovered here before wake touch logic ran.
                bms_i2c_active |= self.maybe_poll_bms(irq);
            }
            bms_i2c_active |= self.maybe_track_bms_activation();
            if bms_i2c_active {
                self.bms_activation_isolation_until =
                    Some(Instant::now() + BMS_ACTIVATION_ISOLATION_WINDOW);
                self.note_skipped_vin_telemetry_if_due(Instant::now());
                self.update_fan_state(irq);
                self.refresh_audio_signals();
                return false;
            }
            self.note_skipped_vin_telemetry_if_due(Instant::now());
            self.update_fan_state(irq);
            self.refresh_audio_signals();
            return false;
        }

        self.bms_activation_isolation_until = None;
        self.maybe_retry();
        self.maybe_handle_fault(irq);
        self.maybe_poll_charger(irq);
        self.maybe_auto_request_bms_activation();
        if self.bms_activation_state == BmsActivationState::Pending {
            let bms_i2c_active = self.maybe_track_bms_activation();
            if bms_i2c_active {
                self.bms_activation_isolation_until =
                    Some(Instant::now() + BMS_ACTIVATION_ISOLATION_WINDOW);
                self.note_skipped_vin_telemetry_if_due(Instant::now());
                self.update_fan_state(irq);
                self.refresh_audio_signals();
                return false;
            }
            self.note_skipped_vin_telemetry_if_due(Instant::now());
            self.update_fan_state(irq);
            self.refresh_audio_signals();
            return false;
        }
        let mut bms_i2c_active = self.maybe_poll_bms(irq);
        bms_i2c_active |= self.maybe_track_bms_activation();
        if bms_i2c_active {
            self.bms_activation_isolation_until =
                Some(Instant::now() + BMS_ACTIVATION_ISOLATION_WINDOW);
            self.note_skipped_vin_telemetry_if_due(Instant::now());
            self.update_fan_state(irq);
            self.refresh_audio_signals();
            return false;
        }
        if self.bms_activation_state == BmsActivationState::Pending {
            self.note_skipped_vin_telemetry_if_due(Instant::now());
            self.update_fan_state(irq);
            self.refresh_audio_signals();
            return false;
        }
        self.update_output_protection();
        self.reconcile_output_state();
        let telemetry_printed = self.maybe_print_telemetry();
        if telemetry_printed {
            self.update_output_protection();
            self.reconcile_output_state();
        }
        self.update_fan_state(irq);
        self.refresh_audio_signals();
        telemetry_printed
    }

    pub fn ui_snapshot(&self) -> SelfCheckUiSnapshot {
        let mut snapshot = self.ui_snapshot;
        let mut detail = snapshot.dashboard_detail;
        let fan_status = self.fan.status();

        detail.out_a_temp_c = snapshot.tmp_a_c;
        detail.out_b_temp_c = snapshot.tmp_b_c;
        detail.fan_rpm = self.fan_rpm_tracker.rpm;
        detail.fan_pwm_pct = Some(fan_status.pwm_pct(self.cfg.fan_config.mid_pwm_pct));
        detail.fan_status = Some(detail_fan_status_text(fan_status));
        detail.output_notice = Some("LIVE DATA");
        detail.thermal_notice = Some("LIVE DATA");

        snapshot.dashboard_detail = detail;
        snapshot
    }

    #[allow(dead_code)]
    pub fn output_restore_pending(&self) -> bool {
        self.can_request_output_restore()
    }

    #[allow(dead_code)]
    pub fn request_output_restore(&mut self) {
        if !self.can_request_output_restore() {
            defmt::warn!(
                "power: output restore ignored gate_reason={} active_outputs={} recoverable_outputs={} mains_present={=?}",
                self.output_state.gate_reason.as_str(),
                self.output_state.active_outputs.describe(),
                self.output_state.recoverable_outputs.describe(),
                self.current_mains_present()
            );
            return;
        }

        let restore = self.output_state.recoverable_outputs;
        self.output_state.active_outputs = restore;
        let now = Instant::now();
        if restore.is_enabled(OutputChannel::OutA) {
            self.tps_a_next_retry_at = Some(now);
        }
        if restore.is_enabled(OutputChannel::OutB) {
            self.tps_b_next_retry_at = Some(now);
        }
        if !self.ina_ready {
            self.ina_next_retry_at = Some(now);
        }
        self.ui_snapshot.tps_a_enabled = Some(restore.is_enabled(OutputChannel::OutA));
        self.ui_snapshot.tps_b_enabled = Some(restore.is_enabled(OutputChannel::OutB));
        self.recompute_ui_mode();
        defmt::info!(
            "power: output restore requested outputs={}",
            restore.describe()
        );
    }

    pub fn fan_command(&self) -> fan::Status {
        self.fan.status()
    }

    fn fan_now_ms(&self) -> u64 {
        self.fan_started_at.elapsed().as_millis()
    }

    fn fan_temps_ready(&self) -> bool {
        self.ui_snapshot.tmp_a != SelfCheckCommState::Pending
            || self.ui_snapshot.tmp_b != SelfCheckCommState::Pending
    }

    fn clear_bms_detail_snapshot(&mut self) {
        let detail = &mut self.ui_snapshot.dashboard_detail;
        detail.cell_mv = [None, None, None, None];
        detail.cell_temp_c = [None, None, None, None];
        detail.balance_active = None;
        detail.balance_mask = None;
        detail.balance_cell = None;
        detail.battery_energy_mwh = None;
        detail.battery_full_capacity_mwh = None;
        detail.charge_fet_on = None;
        detail.discharge_fet_on = None;
        detail.precharge_fet_on = None;
        detail.board_temp_c = None;
        detail.battery_temp_c = None;
        detail.cells_notice = None;
        detail.battery_notice = None;
    }

    fn reset_bms_detail_mac_cache(&mut self, now: Instant) {
        self.bms_cached_da_status2 = None;
        self.bms_cached_filter_capacity = None;
        self.bms_next_da_status2_refresh_at = now;
        self.bms_next_filter_capacity_refresh_at = now + BMS_DETAIL_MAC_REFRESH_STAGGER;
    }

    fn apply_bms_detail_snapshot(&mut self, snapshot: &Bq40z50Snapshot) {
        let detail = &mut self.ui_snapshot.dashboard_detail;
        let balance_mask = detail_bms_balance_mask(snapshot);
        detail.cell_mv = snapshot.cell_mv.map(Some);
        detail.cell_temp_c = detail_bms_cell_sensor_temps(snapshot);
        detail.balance_active = bq40_op_bit(snapshot.op_status, bq40z50::operation_status::CB);
        detail.balance_mask = balance_mask;
        detail.balance_cell = detail_bms_single_balance_cell(balance_mask);
        detail.battery_energy_mwh = detail_bms_energy_mwh(snapshot);
        detail.battery_full_capacity_mwh = detail_bms_full_capacity_mwh(snapshot);
        detail.charge_fet_on = bq40_op_bit(snapshot.op_status, bq40z50::operation_status::CHG);
        detail.discharge_fet_on = bq40_op_bit(snapshot.op_status, bq40z50::operation_status::DSG);
        detail.precharge_fet_on = bq40_op_bit(snapshot.op_status, bq40z50::operation_status::PCHG);
        detail.board_temp_c = detail_bms_board_temp_c(snapshot);
        detail.battery_temp_c = detail_battery_temp_c(snapshot);
        detail.cells_notice = Some("LIVE DATA");
        detail.battery_notice = Some("LIVE DATA");
    }

    fn clear_charger_detail_snapshot(&mut self) {
        let detail = &mut self.ui_snapshot.dashboard_detail;
        detail.input_source = None;
        detail.charger_active = None;
        detail.charger_status = None;
        detail.charger_notice = None;
    }

    fn refresh_tmp112_snapshot(&mut self, ch: OutputChannel) {
        let temp_c_x16 = tmp112::read_temp_c_x16(&mut self.i2c, ch.tmp_addr()).ok();
        match ch {
            OutputChannel::OutA => {
                self.ui_snapshot.tmp_a = if temp_c_x16.is_some() {
                    SelfCheckCommState::Ok
                } else {
                    SelfCheckCommState::Err
                };
                self.ui_snapshot.tmp_a_c_x16 = temp_c_x16;
                self.ui_snapshot.tmp_a_c = temp_c_x16.map(|v| v / 16);
            }
            OutputChannel::OutB => {
                self.ui_snapshot.tmp_b = if temp_c_x16.is_some() {
                    SelfCheckCommState::Ok
                } else {
                    SelfCheckCommState::Err
                };
                self.ui_snapshot.tmp_b_c_x16 = temp_c_x16;
                self.ui_snapshot.tmp_b_c = temp_c_x16.map(|v| v / 16);
            }
        }
    }

    fn refresh_fan_temp_snapshots_if_due(&mut self) {
        let now = Instant::now();
        if matches!(self.bms_activation_isolation_until, Some(until) if now < until) {
            return;
        }
        if now < self.next_fan_temp_refresh_at {
            return;
        }
        self.next_fan_temp_refresh_at = now + self.cfg.telemetry_period;
        self.refresh_tmp112_snapshot(OutputChannel::OutA);
        self.refresh_tmp112_snapshot(OutputChannel::OutB);
    }

    fn update_fan_state(&mut self, irq: &IrqSnapshot) {
        self.refresh_fan_temp_snapshots_if_due();
        let prev = self.fan.status();
        let now_ms = self.fan_now_ms();
        let (status, events) = self.fan.update(fan::Input {
            now_ms,
            temps_ready: self.fan_temps_ready(),
            temp_a_c_x16: self.ui_snapshot.tmp_a_c_x16,
            temp_b_c_x16: self.ui_snapshot.tmp_b_c_x16,
            tach_pulse_count: irq.fan_tach,
        });
        let rpm = self
            .fan_rpm_tracker
            .observe(now_ms, irq.fan_tach, status, self.cfg.fan_config);
        let pwm_pct = status.pwm_pct(self.cfg.fan_config.mid_pwm_pct);

        if events.temp_source_changed {
            match status.temp_source {
                fan::TempSource::Pending => {}
                fan::TempSource::Missing => {
                    defmt::warn!(
                        "fan: temp_source missing fallback=full_speed control_temp_c_x16={=?}",
                        status.control_temp_c_x16
                    );
                }
                fan::TempSource::TmpA | fan::TempSource::TmpB => {
                    defmt::warn!(
                        "fan: temp_source degraded source={} control_temp_c_x16={=?}",
                        status.temp_source.as_str(),
                        status.control_temp_c_x16
                    );
                }
                fan::TempSource::Max => {
                    if prev.temp_source.is_degraded() {
                        defmt::info!(
                            "fan: temp_source restored source={} control_temp_c_x16={=?}",
                            status.temp_source.as_str(),
                            status.control_temp_c_x16
                        );
                    }
                }
            }
        }

        if events.command_changed {
            defmt::info!(
                "fan: command mode={} pwm_pct={=u8} rpm={=?} temp_source={} control_temp_c_x16={=?} cooldown_active={=bool} tach_fault={=bool}",
                status.command.as_str(),
                pwm_pct,
                rpm,
                status.temp_source.as_str(),
                status.control_temp_c_x16,
                status.cooldown_active,
                status.tach_fault
            );
        }

        if events.tach_fault_changed {
            if status.tach_fault {
                defmt::warn!(
                    "fan: tach_timeout mode={} pwm_pct={=u8} rpm={=?} temp_source={} control_temp_c_x16={=?} timeout_ms={=u64}",
                    status.command.as_str(),
                    pwm_pct,
                    rpm,
                    status.temp_source.as_str(),
                    status.control_temp_c_x16,
                    self.cfg.fan_config.tach_timeout_ms
                );
            } else {
                defmt::info!(
                    "fan: tach_recovered mode={} pwm_pct={=u8} rpm={=?} temp_source={} control_temp_c_x16={=?}",
                    status.command.as_str(),
                    pwm_pct,
                    rpm,
                    status.temp_source.as_str(),
                    status.control_temp_c_x16
                );
            }
        }
    }

    pub fn log_fan_telemetry(&self, applied: AppliedFanState) {
        let status = self.fan.status();
        defmt::info!(
            "fan: telemetry requested_mode={} requested_pwm_pct={=u8} applied_mode={} applied_pwm_pct={=u8} rpm={=?} output_degraded={=bool} temp_source={} control_temp_c_x16={=?} tach_recent={=bool} tach_fault={=bool} cooldown_active={=bool}",
            status.requested_command.as_str(),
            status.requested_pwm_pct(self.cfg.fan_config.mid_pwm_pct),
            applied.command.as_str(),
            applied.pwm_pct,
            self.fan_rpm_tracker.rpm,
            applied.degraded,
            status.temp_source.as_str(),
            status.control_temp_c_x16,
            status.tach_pulse_seen_recently,
            status.tach_fault,
            status.cooldown_active
        );
    }

    pub fn bms_activation_state(&self) -> BmsActivationState {
        self.bms_activation_state
    }

    pub fn audio_signals(&self) -> AudioSignalSnapshot {
        self.audio_snapshot
    }

    pub fn take_audio_edges(&mut self) -> AudioSignalEvents {
        let events = self.audio_events;
        self.audio_events = AudioSignalEvents::default();
        events
    }

    pub fn clear_bms_activation_state(&mut self) {
        if self.bms_activation_state != BmsActivationState::Pending {
            defmt::info!(
                "bms: activation clear state={} keep_last_result={}",
                match self.bms_activation_state {
                    BmsActivationState::Idle => "idle",
                    BmsActivationState::Pending => "pending",
                    BmsActivationState::Result(result) => bms_result_name(result),
                },
                bms_result_option_name(self.ui_snapshot.bq40z50_last_result)
            );
            self.bms_activation_state = BmsActivationState::Idle;
        }
    }

    pub fn request_bms_activation(&mut self) {
        self.request_bms_activation_with_diag_override(false, false);
    }

    fn request_bms_activation_with_diag_override(
        &mut self,
        allow_diag_warn: bool,
        auto_request: bool,
    ) {
        if self.bms_activation_state == BmsActivationState::Pending {
            defmt::info!("bms: activation ignored reason=already_pending");
            return;
        }
        let activation_needed = if allow_diag_warn {
            self.ui_snapshot.bq40z50_last_result.is_none()
                && match self.ui_snapshot.bq40z50 {
                    SelfCheckCommState::Err => true,
                    SelfCheckCommState::Warn => !self.has_trusted_bq40_runtime_evidence(),
                    _ => false,
                }
        } else {
            is_bq40_activation_needed(&self.ui_snapshot)
        };
        if !activation_needed {
            defmt::info!(
                "bms: activation ignored reason=not_needed bq40_state={} trusted_evidence={=bool} dsg_ready={=?} last_result={} diag_override={=bool}",
                self_check_comm_state_name(self.ui_snapshot.bq40z50),
                self.has_trusted_bq40_runtime_evidence(),
                self.ui_snapshot.bq40z50_discharge_ready,
                bms_result_option_name(self.ui_snapshot.bq40z50_last_result),
                allow_diag_warn
            );
            return;
        }
        defmt::info!(
            "bms: activation requested bq40_state={} soc_pct={=?} rca_alarm={=?} dsg_ready={=?} charger_state={} charger_allowed={=bool} vbat_present={=?} input_present={=?} last_result={} diag_override={=bool}",
            self_check_comm_state_name(self.ui_snapshot.bq40z50),
            self.ui_snapshot.bq40z50_soc_pct,
            self.ui_snapshot.bq40z50_rca_alarm,
            self.ui_snapshot.bq40z50_discharge_ready,
            self_check_comm_state_name(self.ui_snapshot.bq25792),
            self.charger_allowed,
            self.ui_snapshot.bq25792_vbat_present,
            self.ui_snapshot.fusb302_vbus_present,
            bms_result_option_name(self.ui_snapshot.bq40z50_last_result),
            allow_diag_warn
        );
        bms_diag_breadcrumb_note(2, allow_diag_warn as u8);
        self.bms_activation_current_is_auto = auto_request;
        if !(allow_diag_warn && self.cfg.bms_boot_diag_auto_validate) {
            self.bms_activation_auto_force_charge_until = None;
            self.bms_activation_auto_force_charge_programmed = false;
        }
        self.bms_activation_force_charge_requested = true;
        self.ui_snapshot.bq40z50_last_result = None;
        if !self.charger_allowed {
            self.finish_bms_activation(BmsResultKind::NotDetected, "charger_not_allowed");
            return;
        }

        let status0 = match bq25792::read_u8(&mut self.i2c, bq25792::reg::CHARGER_STATUS_0) {
            Ok(v) => v,
            Err(_) => {
                self.finish_bms_activation(
                    BmsResultKind::NotDetected,
                    "read_charger_status0_failed",
                );
                return;
            }
        };
        defmt::info!(
            "bms: activation request_step=status0_read_ok status0=0x{=u8:x}",
            status0
        );
        let input_present = (status0 & bq25792::status0::VBUS_PRESENT_STAT) != 0
            || (status0 & bq25792::status0::AC1_PRESENT_STAT) != 0
            || (status0 & bq25792::status0::AC2_PRESENT_STAT) != 0
            || (status0 & bq25792::status0::PG_STAT) != 0;
        if !input_present {
            self.finish_bms_activation(BmsResultKind::NotDetected, "input_not_present");
            return;
        }

        let ctrl0 = match bq25792::read_u8(&mut self.i2c, bq25792::reg::CHARGER_CONTROL_0) {
            Ok(v) => v,
            Err(_) => {
                self.finish_bms_activation(BmsResultKind::NotDetected, "read_charger_ctrl0_failed");
                return;
            }
        };
        defmt::info!(
            "bms: activation request_step=ctrl0_read_ok ctrl0=0x{=u8:x}",
            ctrl0
        );
        let vreg_reg = match bq25792::read_u16(&mut self.i2c, bq25792::reg::CHARGE_VOLTAGE_LIMIT) {
            Ok(v) => v,
            Err(_) => {
                self.finish_bms_activation(
                    BmsResultKind::NotDetected,
                    "read_charge_voltage_limit_failed",
                );
                return;
            }
        };
        let ichg_reg = match bq25792::read_u16(&mut self.i2c, bq25792::reg::CHARGE_CURRENT_LIMIT) {
            Ok(v) => v,
            Err(_) => {
                self.finish_bms_activation(
                    BmsResultKind::NotDetected,
                    "read_charge_current_limit_failed",
                );
                return;
            }
        };
        let iindpm_reg = match bq25792::read_u16(&mut self.i2c, bq25792::reg::INPUT_CURRENT_LIMIT) {
            Ok(v) => v,
            Err(_) => {
                self.finish_bms_activation(
                    BmsResultKind::NotDetected,
                    "read_input_current_limit_failed",
                );
                return;
            }
        };
        defmt::info!(
            "bms: activation request_step=limits_read_ok vreg_reg=0x{=u16:x} ichg_reg=0x{=u16:x} iindpm_reg=0x{=u16:x}",
            vreg_reg,
            ichg_reg,
            iindpm_reg
        );
        self.capture_bms_activation_charger_backup(ctrl0, vreg_reg, ichg_reg, iindpm_reg);
        if self
            .maybe_disable_charger_watchdog_for_activation()
            .is_err()
        {
            self.finish_bms_activation(
                BmsResultKind::NotDetected,
                "disable_charger_watchdog_failed",
            );
            return;
        }
        defmt::info!("bms: activation request_step=watchdog_disable_ok");
        self.bms_activation_auto_force_charge_programmed = false;
        self.bms_activation_state = BmsActivationState::Pending;
        defmt::info!("bms: activation request_step=begin_probe_without_charge");
        if let Err(reason) = self.begin_bms_activation_probe_without_charge() {
            self.finish_bms_activation(BmsResultKind::NotDetected, reason);
            return;
        }

        let now = self.bms_activation_started_at.unwrap_or_else(Instant::now);
        let activation_window = if self.bms_activation_force_charge_requested {
            BMS_ACTIVATION_WINDOW
                + BMS_ACTIVATION_MIN_CHARGE_SETTLE
                + BMS_ACTIVATION_MIN_CHARGE_PROBE_WINDOW
        } else {
            BMS_ACTIVATION_WINDOW
        };
        self.bms_activation_deadline = Some(now + activation_window);
        bms_diag_breadcrumb_note(3, self.bms_activation_force_charge_requested as u8);
        defmt::info!(
            "bms: activation start window_ms={=u32} force_min_charge={=bool} phase={} probe_without_charge_window_ms={=u32} probe_without_charge_period_ms={=u32} repower_off_window_ms={=u32} settle_ms={=u32} min_charge_probe_window_ms={=u32} min_charge_direct={=bool} vreg_mv={=u16} ichg_ma={=u16} iindpm_ma={=u16} input_present={=bool}",
            activation_window.as_millis() as u32,
            self.bms_activation_force_charge_requested,
            bms_activation_phase_name(self.bms_activation_phase),
            BMS_ACTIVATION_PROBE_WITHOUT_CHARGE_WINDOW.as_millis() as u32,
            BMS_ACTIVATION_PROBE_WITHOUT_CHARGE_PERIOD.as_millis() as u32,
            BMS_ACTIVATION_REPOWER_OFF_WINDOW.as_millis() as u32,
            BMS_ACTIVATION_MIN_CHARGE_SETTLE.as_millis() as u32,
            BMS_ACTIVATION_MIN_CHARGE_PROBE_WINDOW.as_millis() as u32,
            self.bms_activation_force_charge_requested,
            BMS_ACTIVATION_FORCE_VREG_MV,
            BMS_ACTIVATION_FORCE_ICHG_MA,
            BMS_ACTIVATION_FORCE_IINDPM_MA,
            input_present
        );
    }

    fn maybe_disable_charger_watchdog_for_activation(&mut self) -> Result<(), ()> {
        if self.chg_watchdog_restore.is_some() {
            return Ok(());
        }

        match bq25792::ensure_watchdog_disabled(&mut self.i2c) {
            Ok(state) => {
                if state.watchdog_before != state.watchdog_after {
                    self.chg_watchdog_restore = Some(state.watchdog_before);
                }
                defmt::info!(
                    "bms: activation watchdog stage=disable before=0x{=u8:x} after=0x{=u8:x}",
                    state.watchdog_before,
                    state.watchdog_after
                );
                Ok(())
            }
            Err(_) => Err(()),
        }
    }

    fn maybe_restore_charger_watchdog_after_activation(&mut self) {
        let Some(bits) = self.chg_watchdog_restore else {
            return;
        };

        match bq25792::restore_watchdog(&mut self.i2c, bits) {
            Ok(state) => {
                self.chg_watchdog_restore = None;
                defmt::info!(
                    "bms: activation watchdog stage=restore before=0x{=u8:x} after=0x{=u8:x}",
                    state.watchdog_before,
                    state.watchdog_after
                );
            }
            Err(_) => {
                defmt::info!("bms: activation watchdog stage=restore err=watchdog_restore_failed");
            }
        }
    }

    fn capture_bms_activation_charger_backup(
        &mut self,
        ctrl0: u8,
        vreg_reg: u16,
        ichg_reg: u16,
        iindpm_reg: u16,
    ) {
        if self.bms_activation_backup.is_some() {
            return;
        }
        self.bms_activation_backup = Some(ChargerActivationBackup {
            ctrl0,
            vreg_reg,
            ichg_reg,
            iindpm_reg,
            chg_enabled: self.chg_enabled,
        });
        defmt::info!(
            "bms: activation backup_saved ctrl0=0x{=u8:x} vreg_reg=0x{=u16:x} ichg_reg=0x{=u16:x} iindpm_reg=0x{=u16:x} chg_enabled={=bool}",
            ctrl0,
            vreg_reg,
            ichg_reg,
            iindpm_reg,
            self.chg_enabled
        );
    }

    fn ensure_bms_activation_charger_backup(&mut self) -> Result<(), &'static str> {
        if self.bms_activation_backup.is_some() {
            return Ok(());
        }
        let ctrl0 = bq25792::read_u8(&mut self.i2c, bq25792::reg::CHARGER_CONTROL_0)
            .map_err(|_| "read_charger_ctrl0_backup_failed")?;
        let vreg_reg = bq25792::read_u16(&mut self.i2c, bq25792::reg::CHARGE_VOLTAGE_LIMIT)
            .map_err(|_| "read_charge_voltage_limit_backup_failed")?;
        let ichg_reg = bq25792::read_u16(&mut self.i2c, bq25792::reg::CHARGE_CURRENT_LIMIT)
            .map_err(|_| "read_charge_current_limit_backup_failed")?;
        let iindpm_reg = bq25792::read_u16(&mut self.i2c, bq25792::reg::INPUT_CURRENT_LIMIT)
            .map_err(|_| "read_input_current_limit_backup_failed")?;
        self.capture_bms_activation_charger_backup(ctrl0, vreg_reg, ichg_reg, iindpm_reg);
        Ok(())
    }

    fn restore_bms_activation_charger_backup(&mut self, reason: &'static str) -> Option<bool> {
        let backup = self.bms_activation_backup.take()?;
        let _ = bq25792::write_u16(
            &mut self.i2c,
            bq25792::reg::CHARGE_VOLTAGE_LIMIT,
            backup.vreg_reg,
        );
        let _ = bq25792::write_u16(
            &mut self.i2c,
            bq25792::reg::CHARGE_CURRENT_LIMIT,
            backup.ichg_reg,
        );
        let _ = bq25792::write_u16(
            &mut self.i2c,
            bq25792::reg::INPUT_CURRENT_LIMIT,
            backup.iindpm_reg,
        );
        let _ = bq25792::write_u8(&mut self.i2c, bq25792::reg::CHARGER_CONTROL_0, backup.ctrl0);
        defmt::info!(
            "bms: activation backup_restored reason={} ctrl0=0x{=u8:x} vreg_reg=0x{=u16:x} ichg_reg=0x{=u16:x} iindpm_reg=0x{=u16:x} restore_chg_enabled={=bool}",
            reason,
            backup.ctrl0,
            backup.vreg_reg,
            backup.ichg_reg,
            backup.iindpm_reg,
            backup.chg_enabled
        );
        Some(backup.chg_enabled)
    }

    fn update_bms_activation_auto_due(&mut self, due_at: Instant) {
        let release_margin =
            BMS_ACTIVATION_AUTO_POLL_RELEASE_DELAY.saturating_sub(BMS_ACTIVATION_AUTO_DELAY);
        self.bms_activation_auto_due_at = due_at;
        self.bms_activation_auto_poll_release_at = due_at + release_margin;
        self.bms_activation_auto_defer_logged = false;
        if BMS_ACTIVATION_AUTO_BOOT_FORCE_CHARGE {
            self.bms_activation_auto_force_charge_until = Some(
                due_at
                    + if BMS_BOOT_DIAG_TOOL_STYLE_PROBE_ONLY {
                        BMS_BOOT_DIAG_TOOL_STYLE_FORCE_HOLD
                    } else {
                        Duration::ZERO
                    },
            );
        }
    }

    fn maybe_run_bms_activation_wake_probe(&mut self) -> Option<Bq40ActivationProbeResult> {
        if self.bms_activation_phase != BmsActivationPhase::WakeProbe {
            return None;
        }
        let Some(started_at) = self.bms_activation_started_at else {
            return None;
        };
        let raw_diag = self.bms_activation_current_is_auto;

        while self.bms_activation_diag_stage < BMS_ACTIVATION_DIAG_STAGE_DELAYS_MS.len() {
            let step = self.bms_activation_diag_stage;
            let delay_ms = BMS_ACTIVATION_DIAG_STAGE_DELAYS_MS[step];
            if started_at.elapsed() < Duration::from_millis(delay_ms) {
                break;
            }

            defmt::info!(
                "bms: activation wake_stage step={=u8} delay_ms={=u64} addrs={}",
                step as u8,
                delay_ms,
                BMS_ADDR_LOG
            );
            bms_diag_breadcrumb_note(10, step as u8);

            for addr in bms_probe_candidates().iter().copied() {
                match self.run_bms_activation_wake_probe_step(addr, step as u8, delay_ms, raw_diag)
                {
                    Bq40ActivationProbeResult::Pending => {}
                    result => {
                        bms_diag_breadcrumb_note(11, step as u8);
                        self.bms_activation_diag_stage += 1;
                        return Some(result);
                    }
                }
            }

            defmt::info!(
                "bms: activation wake_stage step={=u8} delay_ms={=u64} result=miss",
                step as u8,
                delay_ms
            );
            bms_diag_breadcrumb_note(12, step as u8);
            self.bms_activation_diag_stage += 1;
        }

        None
    }

    fn maybe_run_bms_activation_probe_without_charge(
        &mut self,
    ) -> Option<Bq40ActivationProbeResult> {
        if self.bms_activation_phase != BmsActivationPhase::ProbeWithoutCharge {
            return None;
        }

        let Some(started_at) = self.bms_activation_started_at else {
            return None;
        };

        let now = Instant::now();
        let next_at = self.bms_activation_followup_next_at.get_or_insert(now);
        if now < *next_at {
            return None;
        }
        *next_at = now + BMS_ACTIVATION_PROBE_WITHOUT_CHARGE_PERIOD;
        self.bms_activation_followup_attempts =
            self.bms_activation_followup_attempts.saturating_add(1);

        let attempt = self.bms_activation_followup_attempts;
        let dwell_ms = started_at.elapsed().as_millis() as u64;
        let raw_diag = self.bms_activation_current_is_auto;
        defmt::info!(
            "bms: activation probe_without_charge attempt={=u16} dwell_ms={=u64} addrs={}",
            attempt,
            dwell_ms,
            BMS_ADDR_LOG
        );

        for addr in bms_probe_candidates().iter().copied() {
            if let Some(snapshot) =
                self.probe_bq40_activation_runtime(addr, attempt, dwell_ms, raw_diag, true)
            {
                return Some(Bq40ActivationProbeResult::Working { addr, snapshot });
            }
        }

        defmt::info!(
            "bms: activation probe_without_charge attempt={=u16} dwell_ms={=u64} result=miss",
            attempt,
            dwell_ms
        );
        None
    }

    fn maybe_run_bms_activation_followup_probe(&mut self) -> Option<Bq40ActivationProbeResult> {
        if self.bms_activation_phase != BmsActivationPhase::WakeProbe
            || self.bms_activation_diag_stage < BMS_ACTIVATION_DIAG_STAGE_DELAYS_MS.len()
        {
            return None;
        }

        let Some(started_at) = self.bms_activation_started_at else {
            return None;
        };

        let now = Instant::now();
        let next_at = self
            .bms_activation_followup_next_at
            .get_or_insert(now + BMS_ACTIVATION_FOLLOWUP_INITIAL_DELAY);
        if now < *next_at {
            return None;
        }
        *next_at = now + BMS_ACTIVATION_FOLLOWUP_PERIOD;
        self.bms_activation_followup_attempts =
            self.bms_activation_followup_attempts.saturating_add(1);

        let attempt = self.bms_activation_followup_attempts;
        let dwell_ms = started_at.elapsed().as_millis() as u64;
        let raw_diag = self.bms_activation_current_is_auto;
        defmt::info!(
            "bms: activation followup attempt={=u16} dwell_ms={=u64} addrs={}",
            attempt,
            dwell_ms,
            BMS_ADDR_LOG
        );

        let exercise_due = dwell_ms <= BMS_ACTIVATION_EXIT_EXERCISE_WINDOW.as_millis() as u64
            && self
                .bms_activation_exercise_next_at
                .map_or(true, |next| now >= next);
        if exercise_due {
            self.bms_activation_exercise_next_at = Some(now + BMS_ACTIVATION_EXIT_EXERCISE_PERIOD);
            for addr in bms_probe_candidates().iter().copied() {
                match self.run_bms_activation_wake_probe_step(
                    addr,
                    attempt.min(u8::MAX as u16) as u8,
                    dwell_ms,
                    raw_diag,
                ) {
                    Bq40ActivationProbeResult::Pending => {}
                    result => return Some(result),
                }
            }

            for addr in bms_probe_candidates().iter().copied() {
                if let Some(snapshot) =
                    self.probe_bq40_activation_runtime(addr, attempt, dwell_ms, raw_diag, false)
                {
                    return Some(Bq40ActivationProbeResult::Working { addr, snapshot });
                }
            }
        }

        for addr in bms_probe_candidates().iter().copied() {
            if let Some(snapshot) =
                self.probe_bq40_activation_runtime(addr, attempt, dwell_ms, raw_diag, false)
            {
                return Some(Bq40ActivationProbeResult::Working { addr, snapshot });
            }
        }

        defmt::info!(
            "bms: activation followup attempt={=u16} dwell_ms={=u64} result=miss",
            attempt,
            dwell_ms
        );
        None
    }

    fn maybe_run_bms_activation_min_charge_probe(&mut self) -> Option<Bq40ActivationProbeResult> {
        if self.bms_activation_phase != BmsActivationPhase::MinChargeProbe {
            return None;
        }

        let Some(started_at) = self.bms_activation_started_at else {
            return None;
        };

        let now = Instant::now();
        let next_at = self.bms_activation_followup_next_at.get_or_insert(now);
        if now < *next_at {
            return None;
        }
        *next_at = now + BMS_ACTIVATION_FOLLOWUP_PERIOD;
        self.bms_activation_followup_attempts =
            self.bms_activation_followup_attempts.saturating_add(1);

        let attempt = self.bms_activation_followup_attempts;
        let dwell_ms = started_at.elapsed().as_millis() as u64;
        defmt::info!(
            "bms: activation min_charge_probe observe attempt={=u16} dwell_ms={=u64} source=normal_poll",
            attempt,
            dwell_ms
        );
        bms_diag_breadcrumb_note(9, attempt.min(u16::from(u8::MAX)) as u8);

        let raw_diag = self.bms_activation_current_is_auto;
        for addr in bms_probe_candidates().iter().copied() {
            match self.read_bq40_activation_snapshot_lean(addr) {
                Ok(snapshot) => {
                    let mut tracker = self.bms_activation_pattern_tracker;
                    match self.read_bq40_activation_snapshot_strict(addr, &mut tracker) {
                        Ok(strict_snapshot) => {
                            self.bms_activation_pattern_tracker = tracker;
                            defmt::info!(
                                "bms: activation min_charge_probe strict addr=0x{=u8:x} attempt={=u16} dwell_ms={=u64} temp_c_x10={=i32} vpack_mv={=u16} current_ma={=i16} rsoc_pct={=u16} batt_status=0x{=u16:x} cell1_mv={=u16} cell2_mv={=u16} cell3_mv={=u16} cell4_mv={=u16}",
                                addr,
                                attempt,
                                dwell_ms,
                                bq40z50::temp_c_x10_from_k_x10(strict_snapshot.temp_k_x10),
                                strict_snapshot.vpack_mv,
                                strict_snapshot.current_ma,
                                strict_snapshot.rsoc_pct,
                                strict_snapshot.batt_status,
                                strict_snapshot.cell_mv[0],
                                strict_snapshot.cell_mv[1],
                                strict_snapshot.cell_mv[2],
                                strict_snapshot.cell_mv[3]
                            );
                            return Some(Bq40ActivationProbeResult::Working {
                                addr,
                                snapshot: strict_snapshot,
                            });
                        }
                        Err(err) => {
                            self.bms_activation_pattern_tracker = tracker;
                            defmt::info!(
                                "bms: activation min_charge_probe lean addr=0x{=u8:x} attempt={=u16} dwell_ms={=u64} temp_c_x10={=i32} vpack_mv={=u16} current_ma={=i16} rsoc_pct={=u16} batt_status=0x{=u16:x} strict_detail_err={} confirm=failed",
                                addr,
                                attempt,
                                dwell_ms,
                                bq40z50::temp_c_x10_from_k_x10(snapshot.temp_k_x10),
                                snapshot.vpack_mv,
                                snapshot.current_ma,
                                snapshot.rsoc_pct,
                                snapshot.batt_status,
                                bq40_activation_read_error_kind(err)
                            );
                            continue;
                        }
                    }
                }
                Err(err) => {
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage=min_charge_probe_lean attempt={=u16} dwell_ms={=u64} err={}",
                            addr,
                            attempt,
                            dwell_ms,
                            bq40_activation_read_error_kind(err)
                        );
                    }
                }
            }
        }
        None
    }

    fn probe_bq40_activation_runtime(
        &mut self,
        addr: u8,
        attempt: u16,
        dwell_ms: u64,
        raw_diag: bool,
        require_trusted_voltage_for_confirm: bool,
    ) -> Option<Bq40z50Snapshot> {
        let voltage_mv = match self.read_bq40_u16_direct(addr, bq40z50::cmd::VOLTAGE) {
            Ok(voltage_mv) if voltage_mv <= 20_000 => {
                defmt::info!(
                    "bms: activation runtime_probe_voltage addr=0x{=u8:x} attempt={=u16} dwell_ms={=u64} vpack_mv={=u16}",
                    addr,
                    attempt,
                    dwell_ms,
                    voltage_mv
                );
                Some(voltage_mv)
            }
            Ok(voltage_mv) => {
                if raw_diag {
                    defmt::info!(
                        "bms_diag: addr=0x{=u8:x} stage=runtime_probe_voltage attempt={=u16} dwell_ms={=u64} vpack_mv={=u16} err=bad_range keep_strict=true",
                        addr,
                        attempt,
                        dwell_ms,
                        voltage_mv
                    );
                }
                Some(voltage_mv)
            }
            Err(err) => {
                if raw_diag {
                    defmt::info!(
                        "bms_diag: addr=0x{=u8:x} stage=runtime_probe_voltage attempt={=u16} dwell_ms={=u64} err={} keep_strict=true",
                        addr,
                        attempt,
                        dwell_ms,
                        bq40_activation_read_error_kind(err)
                    );
                }
                None
            }
        };

        if require_trusted_voltage_for_confirm
            && !voltage_mv.is_some_and(|raw| (2_500..=20_000).contains(&raw))
        {
            if raw_diag {
                defmt::info!(
                    "bms_diag: addr=0x{=u8:x} stage=runtime_probe_confirm_skipped attempt={=u16} dwell_ms={=u64} reason=untrusted_voltage",
                    addr,
                    attempt,
                    dwell_ms
                );
            }
            return None;
        }

        let mut tracker = self.bms_activation_pattern_tracker;
        let confirmed = self.confirm_bq40_activation_snapshot(
            addr,
            attempt.min(u16::from(u8::MAX)) as u8,
            dwell_ms,
            "runtime_probe_confirm",
            &mut tracker,
            raw_diag,
        );
        self.bms_activation_pattern_tracker = tracker;

        if let Some(snapshot) = confirmed {
            let core_only_snapshot = snapshot.op_status.is_none() && snapshot.cell_mv == [0; 4];
            defmt::info!(
                "bms: activation runtime_probe addr=0x{=u8:x} attempt={=u16} dwell_ms={=u64} source={} temp_c_x10={=i32} vpack_mv={=u16} current_ma={=i16} rsoc_pct={=u16} batt_status=0x{=u16:x} cell1_mv={=u16} cell2_mv={=u16} cell3_mv={=u16} cell4_mv={=u16}",
                addr,
                attempt,
                dwell_ms,
                if core_only_snapshot { "core_5word" } else { "strict" },
                bq40z50::temp_c_x10_from_k_x10(snapshot.temp_k_x10),
                snapshot.vpack_mv,
                snapshot.current_ma,
                snapshot.rsoc_pct,
                snapshot.batt_status,
                snapshot.cell_mv[0],
                snapshot.cell_mv[1],
                snapshot.cell_mv[2],
                snapshot.cell_mv[3]
            );
            return Some(snapshot);
        }

        if raw_diag {
            defmt::info!(
                "bms_diag: addr=0x{=u8:x} stage=runtime_probe_confirm attempt={=u16} dwell_ms={=u64} vpack_mv={=?} result=miss",
                addr,
                attempt,
                dwell_ms,
                voltage_mv
            );
        }
        None
    }

    fn run_bms_activation_wake_probe_step(
        &mut self,
        addr: u8,
        step: u8,
        delay_ms: u64,
        raw_diag: bool,
    ) -> Bq40ActivationProbeResult {
        let mut touched = false;
        let mut rsoc_after_touch = None;

        match self.touch_bq40_command(addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE) {
            Ok(()) => {
                touched = true;
                if raw_diag {
                    defmt::info!(
                        "bms_diag: addr=0x{=u8:x} stage=wake_touch_rsoc step={=u8} delay_ms={=u64}",
                        addr,
                        step,
                        delay_ms
                    );
                }
            }
            Err(kind) => {
                if raw_diag {
                    defmt::info!(
                        "bms_diag: addr=0x{=u8:x} stage=wake_touch_rsoc step={=u8} delay_ms={=u64} err={}",
                        addr,
                        step,
                        delay_ms,
                        kind
                    );
                }
            }
        }

        if touched {
            if let Ok(raw) = self.read_bq40_u16_after_touch(
                addr,
                "wake_touch_rsoc_raw",
                step,
                delay_ms,
                raw_diag,
            ) {
                if raw == BMS_ROM_MODE_SIGNATURE {
                    defmt::info!(
                        "bms: activation wake_stage step={=u8} delay_ms={=u64} addr=0x{=u8:x} result=rom_mode",
                        step,
                        delay_ms,
                        addr
                    );
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage=wake_window_rom_signature step={=u8} delay_ms={=u64}",
                            addr,
                            step,
                            delay_ms
                        );
                    }
                    return Bq40ActivationProbeResult::Rom;
                }
                rsoc_after_touch = Some(raw);
            }
        }

        if rsoc_after_touch.is_none() {
            match self.touch_bq40_command(addr, bq40z50::cmd::TEMPERATURE) {
                Ok(()) => {
                    touched = true;
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage=wake_touch_temp step={=u8} delay_ms={=u64}",
                            addr,
                            step,
                            delay_ms
                        );
                    }
                    let _ = self.read_bq40_u16_after_touch(
                        addr,
                        "wake_touch_temp_raw",
                        step,
                        delay_ms,
                        raw_diag,
                    );
                }
                Err(kind) => {
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage=wake_touch_temp step={=u8} delay_ms={=u64} err={}",
                            addr,
                            step,
                            delay_ms,
                            kind
                        );
                    }
                }
            }
        }

        if let Some(rsoc) = rsoc_after_touch {
            match self.touch_bq40_command(addr, bq40z50::cmd::TEMPERATURE) {
                Ok(()) => {
                    touched = true;
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage=wake_touch_temp step={=u8} delay_ms={=u64}",
                            addr,
                            step,
                            delay_ms
                        );
                    }
                    if let Ok(temp_raw) = self.read_bq40_u16_after_touch(
                        addr,
                        "wake_touch_temp_raw",
                        step,
                        delay_ms,
                        raw_diag,
                    ) {
                        if raw_diag {
                            defmt::info!(
                                "bms_diag: addr=0x{=u8:x} stage=wake_window_candidate step={=u8} delay_ms={=u64} rsoc_pct={=u16} temp_raw=0x{=u16:x} temp_c_x10={=i32}",
                                addr,
                                step,
                                delay_ms,
                                rsoc,
                                temp_raw,
                                bq40z50::temp_c_x10_from_k_x10(temp_raw)
                            );
                        }

                        let mut tracker = self.bms_activation_pattern_tracker;
                        if rsoc <= 100 && (2_000..=4_300).contains(&temp_raw) {
                            if let Some(snapshot) = self.confirm_bq40_activation_snapshot(
                                addr,
                                step,
                                delay_ms,
                                "wake_snapshot_confirm_touch",
                                &mut tracker,
                                raw_diag,
                            ) {
                                self.bms_activation_pattern_tracker = tracker;
                                return Bq40ActivationProbeResult::Working { addr, snapshot };
                            }
                        }
                        self.bms_activation_pattern_tracker = tracker;
                    }
                }
                Err(kind) => {
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage=wake_touch_temp step={=u8} delay_ms={=u64} err={}",
                            addr,
                            step,
                            delay_ms,
                            kind
                        );
                    }
                }
            }
        }

        if !touched {
            return Bq40ActivationProbeResult::Pending;
        }

        let mut tracker = self.bms_activation_pattern_tracker;
        match self.touch_then_read_bq40_wake_probe(
            addr,
            bq40z50::cmd::RELATIVE_STATE_OF_CHARGE,
            "wake_touch_read_rsoc",
            "wake_touch_read_rsoc_raw",
            step,
            delay_ms,
            raw_diag,
        ) {
            Ok(rsoc) => {
                if rsoc == BMS_ROM_MODE_SIGNATURE {
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage=wake_window_rom_signature step={=u8} delay_ms={=u64}",
                            addr,
                            step,
                            delay_ms
                        );
                    }
                    return Bq40ActivationProbeResult::Rom;
                }
                if rsoc <= 100 {
                    match self.touch_then_read_bq40_wake_probe(
                        addr,
                        bq40z50::cmd::TEMPERATURE,
                        "wake_touch_read_temp",
                        "wake_touch_read_temp_raw",
                        step,
                        delay_ms,
                        raw_diag,
                    ) {
                        Ok(temp_raw) if (2_000..=4_300).contains(&temp_raw) => {
                            if let Some(snapshot) = self.confirm_bq40_activation_snapshot(
                                addr,
                                step,
                                delay_ms,
                                "wake_snapshot_confirm_split",
                                &mut tracker,
                                raw_diag,
                            ) {
                                self.bms_activation_pattern_tracker = tracker;
                                return Bq40ActivationProbeResult::Working { addr, snapshot };
                            }
                        }
                        Ok(_) | Err(_) => {}
                    }
                }
            }
            Err(_) => {}
        }

        for round in 0..BMS_ACTIVATION_KEEPALIVE_ROUNDS {
            if round > 0 {
                spin_delay(BMS_ACTIVATION_KEEPALIVE_GAP);
            }

            for (cmd, stage_name) in [
                (
                    bq40z50::cmd::RELATIVE_STATE_OF_CHARGE,
                    "wake_keepalive_rsoc",
                ),
                (bq40z50::cmd::TEMPERATURE, "wake_keepalive_temp"),
            ] {
                match self.touch_bq40_command(addr, cmd) {
                    Ok(()) => {
                        if raw_diag {
                            defmt::info!(
                                "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} round={=u8}",
                                addr,
                                stage_name,
                                step,
                                delay_ms,
                                round as u8
                            );
                        }
                    }
                    Err(kind) => {
                        if raw_diag {
                            defmt::info!(
                                "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} round={=u8} err={}",
                                addr,
                                stage_name,
                                step,
                                delay_ms,
                                round as u8,
                                kind
                            );
                        }
                    }
                }
            }

            let rsoc = self
                .read_bq40_u16_wake_probe(
                    addr,
                    bq40z50::cmd::RELATIVE_STATE_OF_CHARGE,
                    "wake_read_rsoc_split",
                    step,
                    delay_ms,
                    round as u8,
                    raw_diag,
                )
                .ok();
            if rsoc == Some(BMS_ROM_MODE_SIGNATURE) {
                defmt::info!(
                    "bms: activation wake_stage step={=u8} delay_ms={=u64} addr=0x{=u8:x} round={=u8} result=rom_mode",
                    step,
                    delay_ms,
                    addr,
                    round as u8
                );
                return Bq40ActivationProbeResult::Rom;
            }

            let temp = self
                .read_bq40_u16_wake_probe(
                    addr,
                    bq40z50::cmd::TEMPERATURE,
                    "wake_read_temp_split",
                    step,
                    delay_ms,
                    round as u8,
                    raw_diag,
                )
                .ok();
            if let (Some(rsoc), Some(temp_raw)) = (rsoc, temp) {
                if raw_diag {
                    defmt::info!(
                        "bms_diag: addr=0x{=u8:x} stage=wake_keepalive_candidate step={=u8} delay_ms={=u64} round={=u8} rsoc_pct={=u16} temp_raw=0x{=u16:x} temp_c_x10={=i32}",
                        addr,
                        step,
                        delay_ms,
                        round as u8,
                        rsoc,
                        temp_raw,
                        bq40z50::temp_c_x10_from_k_x10(temp_raw)
                    );
                }
                if rsoc <= 100 && (2_000..=4_300).contains(&temp_raw) {
                    if let Some(snapshot) = self.confirm_bq40_activation_snapshot(
                        addr,
                        step,
                        delay_ms,
                        "wake_snapshot_confirm_keepalive",
                        &mut tracker,
                        raw_diag,
                    ) {
                        self.bms_activation_pattern_tracker = tracker;
                        return Bq40ActivationProbeResult::Working { addr, snapshot };
                    }
                }
            }
        }

        self.bms_activation_pattern_tracker = tracker;
        Bq40ActivationProbeResult::Pending
    }

    fn touch_bq40_command(&mut self, addr: u8, cmd: u8) -> Result<(), &'static str> {
        self.i2c.write(addr, &[cmd]).map_err(i2c_error_kind)
    }

    fn read_bq40_block_raw_checked(
        &mut self,
        addr: u8,
        cmd: u8,
    ) -> Result<Bq40ActivationBlockReadRaw, &'static str> {
        let mut buf = [0u8; 33];
        self.i2c
            .write_read(addr, &[cmd], &mut buf)
            .map_err(i2c_error_kind)?;

        let declared_len = buf[0];
        if declared_len == 0 || declared_len > 32 {
            return Err("bad_len");
        }

        let payload_len = declared_len.min(32);
        let payload_len_usize = payload_len as usize;
        let mut payload = [0u8; 32];
        payload[..payload_len_usize].copy_from_slice(&buf[1..(1 + payload_len_usize)]);
        Ok(Bq40ActivationBlockReadRaw {
            declared_len,
            payload_len,
            payload,
        })
    }

    fn log_bq40_activation_mac_probe(&mut self, addr: u8, stage: &'static str) {
        const MANUFACTURER_ACCESS_CMD: u8 = 0x00;
        const MANUFACTURER_DATA_CMD: u8 = 0x23;
        const DEVICE_TYPE_CMD_MSB_FIRST: [u8; 2] = [0x00, 0x01];

        match self.i2c.write(
            addr,
            &[
                MANUFACTURER_ACCESS_CMD,
                DEVICE_TYPE_CMD_MSB_FIRST[0],
                DEVICE_TYPE_CMD_MSB_FIRST[1],
            ],
        ) {
            Ok(()) => {
                spin_delay(BMS_ACTIVATION_MAC_WRITE_SETTLE);
            }
            Err(err) => {
                defmt::info!(
                    "bms_diag: addr=0x{=u8:x} stage={} mac_probe err={}",
                    addr,
                    stage,
                    i2c_error_kind(err)
                );
                return;
            }
        }

        match self.read_bq40_block_raw_checked(addr, MANUFACTURER_DATA_CMD) {
            Ok(raw) => {
                let payload_len = raw.payload_len as usize;
                let b0 = if payload_len > 0 { raw.payload[0] } else { 0 };
                let b1 = if payload_len > 1 { raw.payload[1] } else { 0 };
                let b2 = if payload_len > 2 { raw.payload[2] } else { 0 };
                let b3 = if payload_len > 3 { raw.payload[3] } else { 0 };
                let mb44_ok = payload_len >= 4 && b0 == 0x01 && b1 == 0x00;
                let device_type = if payload_len >= 4 {
                    u16::from_le_bytes([b2, b3])
                } else {
                    0
                };
                let verdict = if mb44_ok && device_type == BMS_DEVICE_TYPE_BQ40Z50 {
                    "device_type_ok"
                } else if mb44_ok {
                    "device_type_mismatch"
                } else {
                    "reply_unconfirmed"
                };
                defmt::info!(
                    "bms_diag: addr=0x{=u8:x} stage={} mac_probe len={=u8} payload={=u8} b0=0x{=u8:x} b1=0x{=u8:x} b2=0x{=u8:x} b3=0x{=u8:x} device_type=0x{=u16:x} verdict={}",
                    addr,
                    stage,
                    raw.declared_len,
                    raw.payload_len,
                    b0,
                    b1,
                    b2,
                    b3,
                    device_type,
                    verdict
                );
            }
            Err(err) => {
                defmt::info!(
                    "bms_diag: addr=0x{=u8:x} stage={} mac_probe err={}",
                    addr,
                    stage,
                    err
                );
            }
        }
    }

    fn read_bq40_u16_after_touch(
        &mut self,
        addr: u8,
        stage: &'static str,
        step: u8,
        delay_ms: u64,
        raw_diag: bool,
    ) -> Result<u16, &'static str> {
        for gap_ms in BMS_ACTIVATION_DIAG_TOUCH_READ_GAPS_MS {
            spin_delay(Duration::from_millis(gap_ms));
            let mut buf = [0u8; 2];
            match self.i2c.read(addr, &mut buf) {
                Ok(()) => {
                    let raw = u16::from_le_bytes(buf);
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} gap_ms={=u64} raw=0x{=u16:x}",
                            addr,
                            stage,
                            step,
                            delay_ms,
                            gap_ms,
                            raw
                        );
                    }
                    return Ok(raw);
                }
                Err(err) => {
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} gap_ms={=u64} err={}",
                            addr,
                            stage,
                            step,
                            delay_ms,
                            gap_ms,
                            i2c_error_kind(err)
                        );
                    }
                }
            }
        }

        Err("i2c_nack")
    }

    fn touch_then_read_bq40_wake_probe(
        &mut self,
        addr: u8,
        cmd: u8,
        touch_stage: &'static str,
        read_stage: &'static str,
        step: u8,
        delay_ms: u64,
        raw_diag: bool,
    ) -> Result<u16, &'static str> {
        for gap_ms in BMS_ACTIVATION_READ_GAPS_MS {
            match self.touch_bq40_command(addr, cmd) {
                Ok(()) => {
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} gap_ms={=u64}",
                            addr,
                            touch_stage,
                            step,
                            delay_ms,
                            gap_ms
                        );
                    }
                }
                Err(kind) => {
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} gap_ms={=u64} err={}",
                            addr,
                            touch_stage,
                            step,
                            delay_ms,
                            gap_ms,
                            kind
                        );
                    }
                    continue;
                }
            }

            spin_delay(Duration::from_millis(gap_ms));
            let mut buf = [0u8; 2];
            match self.i2c.read(addr, &mut buf) {
                Ok(()) => {
                    let raw = u16::from_le_bytes(buf);
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} gap_ms={=u64} raw=0x{=u16:x}",
                            addr,
                            read_stage,
                            step,
                            delay_ms,
                            gap_ms,
                            raw
                        );
                    }
                    return Ok(raw);
                }
                Err(err) => {
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} gap_ms={=u64} err={}",
                            addr,
                            read_stage,
                            step,
                            delay_ms,
                            gap_ms,
                            i2c_error_kind(err)
                        );
                    }
                }
            }
        }

        Err("i2c_nack")
    }

    fn read_bq40_u16_wake_probe(
        &mut self,
        addr: u8,
        cmd: u8,
        stage: &'static str,
        step: u8,
        delay_ms: u64,
        round: u8,
        raw_diag: bool,
    ) -> Result<u16, Bq40ActivationReadError> {
        for gap_ms in BMS_ACTIVATION_READ_GAPS_MS {
            let gap = Duration::from_millis(gap_ms);
            match self.read_bq40_u16_split_with_gap(addr, cmd, gap) {
                Ok(raw) => {
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} round={=u8} gap_ms={=u64} raw=0x{=u16:x}",
                            addr,
                            stage,
                            step,
                            delay_ms,
                            round,
                            gap_ms,
                            raw
                        );
                    }
                    return Ok(raw);
                }
                Err(err) => {
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} round={=u8} gap_ms={=u64} err={}",
                            addr,
                            stage,
                            step,
                            delay_ms,
                            round,
                            gap_ms,
                            bq40_activation_read_error_kind(err)
                        );
                    }
                }
            }
        }

        self.read_bq40_u16_with_optional_pec(addr, cmd)
    }

    fn read_bq40_u16_with_pec(
        &mut self,
        addr: u8,
        cmd: u8,
    ) -> Result<u16, Bq40ActivationReadError> {
        let mut buf = [0u8; 3];
        self.i2c
            .write_read(addr, &[cmd], &mut buf)
            .map_err(|err| Bq40ActivationReadError::I2c(i2c_error_kind(err)))?;

        let addr_w = addr << 1;
        let addr_r = addr_w | 1;
        let expected = Self::crc8_smbus(&[addr_w, cmd, addr_r, buf[0], buf[1]]);
        if expected != buf[2] {
            return Err(Bq40ActivationReadError::InconsistentSample);
        }

        Ok(u16::from_le_bytes([buf[0], buf[1]]))
    }

    fn read_bq40_u16_split(&mut self, addr: u8, cmd: u8) -> Result<u16, Bq40ActivationReadError> {
        self.i2c
            .write(addr, &[cmd])
            .map_err(|err| Bq40ActivationReadError::I2c(i2c_error_kind(err)))?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let mut buf = [0u8; 2];
        self.i2c
            .read(addr, &mut buf)
            .map_err(|err| Bq40ActivationReadError::I2c(i2c_error_kind(err)))?;
        Ok(u16::from_le_bytes(buf))
    }

    fn read_bq40_u16_split_with_gap(
        &mut self,
        addr: u8,
        cmd: u8,
        gap: Duration,
    ) -> Result<u16, Bq40ActivationReadError> {
        self.i2c
            .write(addr, &[cmd])
            .map_err(|err| Bq40ActivationReadError::I2c(i2c_error_kind(err)))?;
        spin_delay(gap);
        let mut buf = [0u8; 2];
        self.i2c
            .read(addr, &mut buf)
            .map_err(|err| Bq40ActivationReadError::I2c(i2c_error_kind(err)))?;
        Ok(u16::from_le_bytes(buf))
    }

    fn read_bq40_u16_with_optional_pec(
        &mut self,
        addr: u8,
        cmd: u8,
    ) -> Result<u16, Bq40ActivationReadError> {
        const ATTEMPTS: u8 = 2;

        for attempt in 0..ATTEMPTS {
            if let Ok(v) = self.read_bq40_u16_with_pec(addr, cmd) {
                return Ok(v);
            }
            if let Ok(v) = self.read_bq40_u16_split(addr, cmd) {
                return Ok(v);
            }
            if let Ok(v) = bq40z50::read_u16(&mut self.i2c, addr, cmd) {
                return Ok(v);
            }

            if attempt + 1 < ATTEMPTS {
                spin_delay(BMS_ACTIVATION_WORD_GAP);
            }
        }

        Err(Bq40ActivationReadError::I2c("i2c_nack"))
    }

    fn read_bq40_i16_with_optional_pec(
        &mut self,
        addr: u8,
        cmd: u8,
    ) -> Result<i16, Bq40ActivationReadError> {
        self.read_bq40_u16_with_optional_pec(addr, cmd)
            .map(|raw| i16::from_le_bytes(raw.to_le_bytes()))
    }

    fn read_bq40_u16_direct(&mut self, addr: u8, cmd: u8) -> Result<u16, Bq40ActivationReadError> {
        bq40z50::read_u16(&mut self.i2c, addr, cmd)
            .map_err(|e| Bq40ActivationReadError::I2c(i2c_error_kind(e)))
    }

    fn read_bq40_i16_direct(&mut self, addr: u8, cmd: u8) -> Result<i16, Bq40ActivationReadError> {
        bq40z50::read_i16(&mut self.i2c, addr, cmd)
            .map_err(|e| Bq40ActivationReadError::I2c(i2c_error_kind(e)))
    }

    fn prime_bq40_command_window(&mut self, addr: u8) -> Result<(), Bq40ActivationReadError> {
        let _ = self.touch_bq40_command(addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE);
        Ok(())
    }

    fn read_bq40_u16_consistent(
        &mut self,
        addr: u8,
        cmd: u8,
        tolerance: u16,
    ) -> Result<u16, Bq40ActivationReadError> {
        let a = self.read_bq40_u16_with_optional_pec(addr, cmd)?;
        if a == BMS_SUSPICIOUS_STATUS || a == BMS_ROM_MODE_SIGNATURE {
            return Ok(a);
        }
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let b = self.read_bq40_u16_with_optional_pec(addr, cmd)?;
        let ab_diff = a.max(b) - a.min(b);
        if ab_diff <= tolerance {
            return Ok(b);
        }

        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let c = self.read_bq40_u16_with_optional_pec(addr, cmd)?;
        let ac_diff = a.max(c) - a.min(c);
        if ac_diff <= tolerance {
            return Ok(c);
        }
        let bc_diff = b.max(c) - b.min(c);
        if bc_diff <= tolerance {
            return Ok(c);
        }

        Err(Bq40ActivationReadError::InconsistentSample)
    }

    fn read_bq40_i16_consistent(
        &mut self,
        addr: u8,
        cmd: u8,
        tolerance: i16,
    ) -> Result<i16, Bq40ActivationReadError> {
        let a = self.read_bq40_i16_with_optional_pec(addr, cmd)?;
        if a == BMS_SUSPICIOUS_CURRENT_MA || a == BMS_ROM_MODE_SIGNATURE as i16 {
            return Ok(a);
        }
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let b = self.read_bq40_i16_with_optional_pec(addr, cmd)?;
        let ab_diff = (a as i32 - b as i32).abs();
        if ab_diff <= i32::from(tolerance) {
            return Ok(b);
        }

        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let c = self.read_bq40_i16_with_optional_pec(addr, cmd)?;
        let ac_diff = (a as i32 - c as i32).abs();
        if ac_diff <= i32::from(tolerance) {
            return Ok(c);
        }
        let bc_diff = (b as i32 - c as i32).abs();
        if bc_diff <= i32::from(tolerance) {
            return Ok(c);
        }

        Err(Bq40ActivationReadError::InconsistentSample)
    }

    fn read_bq40_activation_snapshot_strict(
        &mut self,
        addr: u8,
        tracker: &mut Bq40ActivationPatternTracker,
    ) -> Result<Bq40z50Snapshot, Bq40ActivationReadError> {
        self.prime_bq40_command_window(addr)?;
        let mut temp_k_x10 = self.read_bq40_u16_consistent(addr, bq40z50::cmd::TEMPERATURE, 5)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        self.prime_bq40_command_window(addr)?;
        let vpack_mv = self.read_bq40_u16_consistent(addr, bq40z50::cmd::VOLTAGE, 20)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        self.prime_bq40_command_window(addr)?;
        let current_ma = self.read_bq40_i16_consistent(addr, bq40z50::cmd::CURRENT, 100)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        self.prime_bq40_command_window(addr)?;
        let rsoc_pct =
            self.read_bq40_u16_consistent(addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE, 1)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        self.prime_bq40_command_window(addr)?;
        let batt_status = self.read_bq40_u16_consistent(addr, bq40z50::cmd::BATTERY_STATUS, 0)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        self.prime_bq40_command_window(addr)?;
        let cell1_mv = self.read_bq40_u16_consistent(addr, bq40z50::cmd::CELL_VOLTAGE_1, 20)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        self.prime_bq40_command_window(addr)?;
        let cell2_mv = self.read_bq40_u16_consistent(addr, bq40z50::cmd::CELL_VOLTAGE_2, 20)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        self.prime_bq40_command_window(addr)?;
        let cell3_mv = self.read_bq40_u16_consistent(addr, bq40z50::cmd::CELL_VOLTAGE_3, 20)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        self.prime_bq40_command_window(addr)?;
        let cell4_mv = self.read_bq40_u16_consistent(addr, bq40z50::cmd::CELL_VOLTAGE_4, 20)?;
        // Keep wake confirm aligned to the tool's mandatory snapshot only. Optional OP_STATUS
        // reads can perturb a fragile wake window without improving result classification.
        let op_status = None;

        let mut temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(temp_k_x10);
        if !(-400..=1250).contains(&temp_c_x10) {
            spin_delay(BMS_ACTIVATION_WORD_GAP);
            let retry_temp_k_x10 =
                self.read_bq40_u16_consistent(addr, bq40z50::cmd::TEMPERATURE, 5)?;
            let retry_temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(retry_temp_k_x10);
            if (-400..=1250).contains(&retry_temp_c_x10) {
                temp_k_x10 = retry_temp_k_x10;
                temp_c_x10 = retry_temp_c_x10;
            }
        }

        let repeat_count =
            observe_bq40_activation_signature(tracker, vpack_mv, current_ma, rsoc_pct, batt_status);
        if bq40_activation_signature_is_stale(vpack_mv, current_ma, batt_status, repeat_count) {
            defmt::info!(
                "bms_diag_raw: addr=0x{=u8:x} reason=stale_pattern temp_k_x10={=u16} temp_c_x10={=i32} vpack_mv={=u16} current_ma={=i16} rsoc_pct={=u16} batt_status=0x{=u16:x} cell1_mv={=u16} cell2_mv={=u16} cell3_mv={=u16} cell4_mv={=u16} repeats={=u8} op_status={=?}",
                addr,
                temp_k_x10,
                temp_c_x10,
                vpack_mv,
                current_ma,
                rsoc_pct,
                batt_status,
                cell1_mv,
                cell2_mv,
                cell3_mv,
                cell4_mv,
                repeat_count,
                op_status
            );
            return Err(Bq40ActivationReadError::StalePattern);
        }

        if !(-400..=1250).contains(&temp_c_x10) || vpack_mv > 20_000 || rsoc_pct > 100 {
            defmt::info!(
                "bms_diag_raw: addr=0x{=u8:x} reason=bad_range temp_k_x10={=u16} temp_c_x10={=i32} vpack_mv={=u16} current_ma={=i16} rsoc_pct={=u16} batt_status=0x{=u16:x} cell1_mv={=u16} cell2_mv={=u16} cell3_mv={=u16} cell4_mv={=u16} op_status={=?}",
                addr,
                temp_k_x10,
                temp_c_x10,
                vpack_mv,
                current_ma,
                rsoc_pct,
                batt_status,
                cell1_mv,
                cell2_mv,
                cell3_mv,
                cell4_mv,
                op_status
            );
            return Err(Bq40ActivationReadError::BadRange);
        }

        Ok(Bq40z50Snapshot {
            battery_mode: 0,
            temp_k_x10,
            vpack_mv,
            current_ma,
            rsoc_pct,
            remcap: 0,
            fcc: 0,
            batt_status,
            op_status,
            da_status2: None,
            filter_capacity: None,
            cell_mv: [cell1_mv, cell2_mv, cell3_mv, cell4_mv],
        })
    }

    fn read_bq40_activation_snapshot_core(
        &mut self,
        addr: u8,
    ) -> Result<Bq40z50Snapshot, Bq40ActivationReadError> {
        let temp_k_x10 = self.read_bq40_u16_direct(addr, bq40z50::cmd::TEMPERATURE)?;
        let vpack_mv = self.read_bq40_u16_direct(addr, bq40z50::cmd::VOLTAGE)?;
        let current_ma = self.read_bq40_i16_direct(addr, bq40z50::cmd::CURRENT)?;
        let rsoc_pct = self.read_bq40_u16_direct(addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE)?;
        let batt_status = self.read_bq40_u16_direct(addr, bq40z50::cmd::BATTERY_STATUS)?;
        let temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(temp_k_x10);

        if !(-400..=1250).contains(&temp_c_x10) || vpack_mv > 20_000 || rsoc_pct > 100 {
            return Err(Bq40ActivationReadError::BadRange);
        }

        Ok(Bq40z50Snapshot {
            battery_mode: 0,
            temp_k_x10,
            vpack_mv,
            current_ma,
            rsoc_pct,
            remcap: 0,
            fcc: 0,
            batt_status,
            op_status: None,
            da_status2: None,
            filter_capacity: None,
            cell_mv: [0; 4],
        })
    }

    fn read_bq40_activation_snapshot_lean(
        &mut self,
        addr: u8,
    ) -> Result<Bq40z50Snapshot, Bq40ActivationReadError> {
        let temp_k_x10 = self.read_bq40_u16_direct(addr, bq40z50::cmd::TEMPERATURE)?;
        let vpack_mv = self.read_bq40_u16_direct(addr, bq40z50::cmd::VOLTAGE)?;
        let current_ma = self.read_bq40_i16_direct(addr, bq40z50::cmd::CURRENT)?;
        let rsoc_pct = self.read_bq40_u16_direct(addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE)?;
        let batt_status = self.read_bq40_u16_direct(addr, bq40z50::cmd::BATTERY_STATUS)?;
        let temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(temp_k_x10);

        if !(-400..=1250).contains(&temp_c_x10) || vpack_mv > 20_000 || rsoc_pct > 100 {
            defmt::info!(
                "bms_diag_raw: addr=0x{=u8:x} reason=lean_bad_range temp_k_x10={=u16} temp_c_x10={=i32} vpack_mv={=u16} current_ma={=i16} rsoc_pct={=u16} batt_status=0x{=u16:x}",
                addr,
                temp_k_x10,
                temp_c_x10,
                vpack_mv,
                current_ma,
                rsoc_pct,
                batt_status
            );
            return Err(Bq40ActivationReadError::BadRange);
        }

        Ok(Bq40z50Snapshot {
            battery_mode: 0,
            temp_k_x10,
            vpack_mv,
            current_ma,
            rsoc_pct,
            remcap: 0,
            fcc: 0,
            batt_status,
            op_status: None,
            da_status2: None,
            filter_capacity: None,
            cell_mv: [0; 4],
        })
    }

    fn confirm_bq40_activation_snapshot(
        &mut self,
        addr: u8,
        step: u8,
        delay_ms: u64,
        stage: &'static str,
        tracker: &mut Bq40ActivationPatternTracker,
        raw_diag: bool,
    ) -> Option<Bq40z50Snapshot> {
        match self.read_bq40_activation_snapshot_core(addr) {
            Ok(snapshot) => {
                let repeat_count = observe_bq40_activation_signature(
                    tracker,
                    snapshot.vpack_mv,
                    snapshot.current_ma,
                    snapshot.rsoc_pct,
                    snapshot.batt_status,
                );
                if bq40_pack_indicates_no_battery(snapshot.vpack_mv) {
                    defmt::info!(
                        "bms: activation confirm_core low_pack_candidate addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} temp_c_x10={=i32} vpack_mv={=u16} current_ma={=i16} rsoc_pct={=u16} batt_status=0x{=u16:x}",
                        addr,
                        stage,
                        step,
                        delay_ms,
                        bq40z50::temp_c_x10_from_k_x10(snapshot.temp_k_x10),
                        snapshot.vpack_mv,
                        snapshot.current_ma,
                        snapshot.rsoc_pct,
                        snapshot.batt_status
                    );
                    return self.confirm_bq40_activation_no_battery(
                        addr, step, delay_ms, stage, tracker, raw_diag, "core", snapshot,
                    );
                }
                if bq40_activation_signature_is_stale(
                    snapshot.vpack_mv,
                    snapshot.current_ma,
                    snapshot.batt_status,
                    repeat_count,
                ) {
                    defmt::info!(
                        "bms: activation confirm_core stale addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} temp_c_x10={=i32} vpack_mv={=u16} current_ma={=i16} rsoc_pct={=u16} batt_status=0x{=u16:x} repeats={=u8}",
                        addr,
                        stage,
                        step,
                        delay_ms,
                        bq40z50::temp_c_x10_from_k_x10(snapshot.temp_k_x10),
                        snapshot.vpack_mv,
                        snapshot.current_ma,
                        snapshot.rsoc_pct,
                        snapshot.batt_status,
                        repeat_count
                    );
                    match self.read_bq40_activation_snapshot_strict(addr, tracker) {
                        Ok(snapshot) => {
                            if bq40_pack_indicates_no_battery(snapshot.vpack_mv) {
                                return self.confirm_bq40_activation_no_battery(
                                    addr,
                                    step,
                                    delay_ms,
                                    stage,
                                    tracker,
                                    raw_diag,
                                    "strict_after_stale",
                                    snapshot,
                                );
                            }
                            defmt::info!(
                                "bms: activation confirm addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} temp_c_x10={=i32} vpack_mv={=u16} current_ma={=i16} rsoc_pct={=u16} batt_status=0x{=u16:x} core_state=stale",
                                addr,
                                stage,
                                step,
                                delay_ms,
                                bq40z50::temp_c_x10_from_k_x10(snapshot.temp_k_x10),
                                snapshot.vpack_mv,
                                snapshot.current_ma,
                                snapshot.rsoc_pct,
                                snapshot.batt_status
                            );
                            return Some(snapshot);
                        }
                        Err(err) => {
                            if raw_diag {
                                defmt::info!(
                                    "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} core_state=stale strict_err={}",
                                    addr,
                                    stage,
                                    step,
                                    delay_ms,
                                    bq40_activation_read_error_kind(err)
                                );
                            }
                            return None;
                        }
                    }
                }
                defmt::info!(
                    "bms: activation confirm_core addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} temp_c_x10={=i32} vpack_mv={=u16} current_ma={=i16} rsoc_pct={=u16} batt_status=0x{=u16:x}",
                    addr,
                    stage,
                    step,
                    delay_ms,
                    bq40z50::temp_c_x10_from_k_x10(snapshot.temp_k_x10),
                    snapshot.vpack_mv,
                    snapshot.current_ma,
                    snapshot.rsoc_pct,
                    snapshot.batt_status
                );
                Some(snapshot)
            }
            Err(core_err) => match self.read_bq40_activation_snapshot_strict(addr, tracker) {
                Ok(snapshot) => {
                    if bq40_pack_indicates_no_battery(snapshot.vpack_mv) {
                        return self.confirm_bq40_activation_no_battery(
                            addr,
                            step,
                            delay_ms,
                            stage,
                            tracker,
                            raw_diag,
                            "strict_after_core_err",
                            snapshot,
                        );
                    }
                    defmt::info!(
                        "bms: activation confirm addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} temp_c_x10={=i32} vpack_mv={=u16} current_ma={=i16} rsoc_pct={=u16} batt_status=0x{=u16:x} op_status={=?} core_err={}",
                        addr,
                        stage,
                        step,
                        delay_ms,
                        bq40z50::temp_c_x10_from_k_x10(snapshot.temp_k_x10),
                        snapshot.vpack_mv,
                        snapshot.current_ma,
                        snapshot.rsoc_pct,
                        snapshot.batt_status,
                        snapshot.op_status,
                        bq40_activation_read_error_kind(core_err)
                    );
                    Some(snapshot)
                }
                Err(err) => {
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} core_err={} strict_err={}",
                            addr,
                            stage,
                            step,
                            delay_ms,
                            bq40_activation_read_error_kind(core_err),
                            bq40_activation_read_error_kind(err)
                        );
                    } else {
                        defmt::info!(
                            "bms: activation confirm miss addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} core_err={} strict_err={}",
                            addr,
                            stage,
                            step,
                            delay_ms,
                            bq40_activation_read_error_kind(core_err),
                            bq40_activation_read_error_kind(err)
                        );
                    }
                    None
                }
            },
        }
    }

    fn confirm_bq40_activation_no_battery(
        &mut self,
        addr: u8,
        step: u8,
        delay_ms: u64,
        stage: &'static str,
        tracker: &mut Bq40ActivationPatternTracker,
        raw_diag: bool,
        source: &'static str,
        snapshot: Bq40z50Snapshot,
    ) -> Option<Bq40z50Snapshot> {
        match self.read_bq40_activation_snapshot_strict(addr, tracker) {
            Ok(confirm) => {
                let confirmed = bq40_pack_indicates_no_battery(confirm.vpack_mv)
                    && bq40_low_pack_runtime_signature_matches(
                        snapshot.vpack_mv,
                        snapshot.current_ma,
                        snapshot.rsoc_pct,
                        snapshot.batt_status,
                        confirm.vpack_mv,
                        confirm.current_ma,
                        confirm.rsoc_pct,
                        confirm.batt_status,
                    );
                if confirmed {
                    defmt::info!(
                        "bms: activation confirm low_pack addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} source={} temp_c_x10={=i32} vpack_mv={=u16} current_ma={=i16} rsoc_pct={=u16} batt_status=0x{=u16:x} cell1_mv={=u16} cell2_mv={=u16} cell3_mv={=u16} cell4_mv={=u16}",
                        addr,
                        stage,
                        step,
                        delay_ms,
                        source,
                        bq40z50::temp_c_x10_from_k_x10(confirm.temp_k_x10),
                        confirm.vpack_mv,
                        confirm.current_ma,
                        confirm.rsoc_pct,
                        confirm.batt_status,
                        confirm.cell_mv[0],
                        confirm.cell_mv[1],
                        confirm.cell_mv[2],
                        confirm.cell_mv[3]
                    );
                    Some(confirm)
                } else {
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} low_pack_mismatch source={} first_vpack_mv={=u16} first_current_ma={=i16} first_rsoc_pct={=u16} first_batt_status=0x{=u16:x} confirm_vpack_mv={=u16} confirm_current_ma={=i16} confirm_rsoc_pct={=u16} confirm_batt_status=0x{=u16:x}",
                            addr,
                            stage,
                            step,
                            delay_ms,
                            source,
                            snapshot.vpack_mv,
                            snapshot.current_ma,
                            snapshot.rsoc_pct,
                            snapshot.batt_status,
                            confirm.vpack_mv,
                            confirm.current_ma,
                            confirm.rsoc_pct,
                            confirm.batt_status
                        );
                    } else {
                        defmt::info!(
                            "bms: activation confirm low_pack mismatch addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} source={} first_vpack_mv={=u16} confirm_vpack_mv={=u16}",
                            addr,
                            stage,
                            step,
                            delay_ms,
                            source,
                            snapshot.vpack_mv,
                            confirm.vpack_mv
                        );
                    }
                    None
                }
            }
            Err(err) => {
                if raw_diag {
                    defmt::info!(
                        "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} low_pack_confirm_miss source={} strict_err={}",
                        addr,
                        stage,
                        step,
                        delay_ms,
                        source,
                        bq40_activation_read_error_kind(err)
                    );
                } else {
                    defmt::info!(
                        "bms: activation confirm low_pack miss addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} source={} strict_err={}",
                        addr,
                        stage,
                        step,
                        delay_ms,
                        source,
                        bq40_activation_read_error_kind(err)
                    );
                }
                None
            }
        }
    }

    fn apply_bq40_activation_snapshot(
        &mut self,
        addr: u8,
        snapshot: &Bq40z50Snapshot,
    ) -> BmsResultKind {
        let rca_alarm = (snapshot.batt_status & bq40z50::battery_status::RCA) != 0;
        let core_only_snapshot = snapshot.op_status.is_none() && snapshot.cell_mv == [0; 4];
        let low_pack_runtime = bq40_pack_indicates_no_battery(snapshot.vpack_mv);
        let (charge_ready, charge_reason) = bq40_decode_charge_path(snapshot.op_status);
        let (discharge_ready, discharge_reason) = bq40_decode_discharge_path(snapshot.op_status);
        let protection_active = bq40_op_bit(snapshot.op_status, bq40z50::operation_status::PF)
            == Some(true)
            || bq40z50::battery_status::error_code(snapshot.batt_status) != 0
            || (snapshot.batt_status
                & (bq40z50::battery_status::OCA
                    | bq40z50::battery_status::TCA
                    | bq40z50::battery_status::OTA
                    | bq40z50::battery_status::TDA))
                != 0;
        let primary_reason = bq40_primary_reason(
            snapshot.batt_status,
            snapshot.op_status,
            charge_reason,
            discharge_reason,
        );
        let flow = bq40_decode_current_flow(snapshot.current_ma);
        let state = if core_only_snapshot {
            if rca_alarm || low_pack_runtime {
                SelfCheckCommState::Warn
            } else {
                SelfCheckCommState::Ok
            }
        } else if matches!(discharge_ready, Some(false)) || rca_alarm {
            SelfCheckCommState::Warn
        } else {
            SelfCheckCommState::Ok
        };

        self.bms_addr = Some(addr);
        self.bms_runtime_seen = true;
        self.bms_ok_streak = self.bms_ok_streak.saturating_add(1);
        self.bms_err_streak = 0;
        self.bms_next_retry_at = None;
        self.bms_next_poll_at = Instant::now();
        self.ui_snapshot.bq40z50 = state;
        self.ui_snapshot.bq40z50_pack_mv = Some(snapshot.vpack_mv);
        self.ui_snapshot.bq40z50_current_ma = Some(snapshot.current_ma);
        self.ui_snapshot.bq40z50_soc_pct = Some(snapshot.rsoc_pct);
        self.ui_snapshot.bq40z50_rca_alarm = Some(rca_alarm);
        self.ui_snapshot.bq40z50_no_battery = Some(low_pack_runtime);
        self.ui_snapshot.bq40z50_discharge_ready = discharge_ready;
        self.apply_bms_detail_snapshot(snapshot);
        self.bms_audio = BmsAudioState {
            rca_alarm: Some(rca_alarm),
            protection_active,
            module_fault: false,
        };

        defmt::info!(
            "bms: activation trusted_snapshot addr=0x{=u8:x} source={} state={} no_battery={=bool} temp_c_x10={=i32} vpack_mv={=u16} current_ma={=i16} flow={} rsoc_pct={=u16} batt_status=0x{=u16:x} cell1_mv={=u16} cell2_mv={=u16} cell3_mv={=u16} cell4_mv={=u16} rca_alarm={=bool} chg_ready={=?} dsg_ready={=?} primary_reason={}",
            addr,
            if core_only_snapshot { "core_5word" } else { "strict" },
            self_check_comm_state_name(state),
            low_pack_runtime,
            bq40z50::temp_c_x10_from_k_x10(snapshot.temp_k_x10),
            snapshot.vpack_mv,
            snapshot.current_ma,
            flow,
            snapshot.rsoc_pct,
            snapshot.batt_status,
            snapshot.cell_mv[0],
            snapshot.cell_mv[1],
            snapshot.cell_mv[2],
            snapshot.cell_mv[3],
            rca_alarm,
            charge_ready,
            discharge_ready,
            primary_reason
        );

        if low_pack_runtime {
            BmsResultKind::NoBattery
        } else if state == SelfCheckCommState::Ok {
            BmsResultKind::Success
        } else {
            BmsResultKind::Abnormal
        }
    }

    fn crc8_smbus(bytes: &[u8]) -> u8 {
        let mut crc = 0u8;
        for &byte in bytes {
            crc ^= byte;
            for _ in 0..8 {
                if (crc & 0x80) != 0 {
                    crc = (crc << 1) ^ 0x07;
                } else {
                    crc <<= 1;
                }
            }
        }
        crc
    }

    fn maybe_auto_request_bms_activation(&mut self) {
        if !self.cfg.bms_boot_diag_auto_validate {
            return;
        }

        if self.bms_activation_auto_attempted
            || self.bms_activation_state != BmsActivationState::Idle
        {
            return;
        }

        let now = Instant::now();
        if now < self.bms_activation_auto_due_at {
            return;
        }

        if self.ui_snapshot.bq25792 == SelfCheckCommState::Pending
            || self.ui_snapshot.fusb302 == SelfCheckCommState::Pending
        {
            let due_at = now + Duration::from_millis(500);
            self.update_bms_activation_auto_due(due_at);
            return;
        }

        let auto_activation_needed = self.ui_snapshot.bq40z50_last_result.is_none()
            && self.ui_snapshot.bq40z50 == SelfCheckCommState::Err;
        if !auto_activation_needed {
            self.bms_activation_auto_attempted = true;
            self.bms_activation_auto_force_charge_until = None;
            self.bms_activation_auto_force_charge_programmed = false;
            if let Some(restore_chg_enabled) =
                self.restore_bms_activation_charger_backup("auto_skip_not_needed")
            {
                if restore_chg_enabled {
                    self.chg_ce.set_low();
                    self.chg_enabled = true;
                } else {
                    self.chg_ce.set_high();
                    self.chg_enabled = false;
                }
            }
            defmt::info!(
                "bms: activation auto_skip reason=bq40_not_err state={} trusted_evidence={=bool} last_result={}",
                self_check_comm_state_name(self.ui_snapshot.bq40z50),
                self.has_trusted_bq40_runtime_evidence(),
                bms_result_option_name(self.ui_snapshot.bq40z50_last_result)
            );
            return;
        }

        if self.ui_snapshot.fusb302_vbus_present != Some(true) {
            self.bms_activation_auto_attempted = true;
            self.bms_activation_auto_force_charge_until = None;
            self.bms_activation_auto_force_charge_programmed = false;
            if let Some(restore_chg_enabled) =
                self.restore_bms_activation_charger_backup("auto_skip_no_input_power")
            {
                if restore_chg_enabled {
                    self.chg_ce.set_low();
                    self.chg_enabled = true;
                } else {
                    self.chg_ce.set_high();
                    self.chg_enabled = false;
                }
            }
            defmt::info!(
                "bms: activation auto_skip reason=no_input_power bq40_state={} charger_state={} input_present={=?}",
                self_check_comm_state_name(self.ui_snapshot.bq40z50),
                self_check_comm_state_name(self.ui_snapshot.bq25792),
                self.ui_snapshot.fusb302_vbus_present
            );
            return;
        }

        if BMS_BOOT_DIAG_TOOL_STYLE_PROBE_ONLY {
            self.bms_activation_auto_attempted = true;
            self.bms_activation_auto_force_charge_until =
                Some(now + BMS_BOOT_DIAG_TOOL_STYLE_FORCE_HOLD);
            self.bms_activation_auto_force_charge_programmed = true;
            self.bms_next_poll_at = now;
            self.bms_next_retry_at = None;
            self.chg_next_poll_at = now;
            self.chg_next_retry_at = None;
            defmt::info!(
                "bms: activation auto_skip reason=tool_style_probe_only hold_ms={=u64} poll_ms={=u32} retry_backoff_suppressed={=bool} bq40_state={} charger_state={} charger_allowed={=bool} input_present={=?} vbat_present={=?}",
                BMS_BOOT_DIAG_TOOL_STYLE_FORCE_HOLD.as_millis() as u64,
                2_000u32,
                true,
                self_check_comm_state_name(self.ui_snapshot.bq40z50),
                self_check_comm_state_name(self.ui_snapshot.bq25792),
                self.charger_allowed,
                self.ui_snapshot.fusb302_vbus_present,
                self.ui_snapshot.bq25792_vbat_present
            );
            return;
        }

        self.bms_activation_auto_attempted = true;
        self.bms_activation_auto_force_charge_until = None;
        bms_diag_breadcrumb_note(1, 0);
        defmt::info!(
            "bms: activation auto_request reason=boot_diag bq40_state={} charger_state={} charger_allowed={=bool} input_present={=?} vbat_present={=?}",
            self_check_comm_state_name(self.ui_snapshot.bq40z50),
            self_check_comm_state_name(self.ui_snapshot.bq25792),
            self.charger_allowed,
            self.ui_snapshot.fusb302_vbus_present,
            self.ui_snapshot.bq25792_vbat_present
        );
        self.request_bms_activation_with_diag_override(true, true);
    }

    fn maybe_track_bms_activation(&mut self) -> bool {
        if self.bms_activation_state != BmsActivationState::Pending {
            return false;
        }

        if !self.maybe_advance_bms_activation_phase() {
            return false;
        }

        let wake_stage_before = self.bms_activation_diag_stage;
        let probe_without_charge_before = self.bms_activation_followup_attempts;
        if let Some(result) = self.maybe_run_bms_activation_probe_without_charge() {
            match result {
                Bq40ActivationProbeResult::Pending => {}
                Bq40ActivationProbeResult::Rom => {
                    self.finish_bms_activation(BmsResultKind::RomMode, "rom_mode_detected");
                    return true;
                }
                Bq40ActivationProbeResult::Working { addr, snapshot } => {
                    let result = self.apply_bq40_activation_snapshot(addr, &snapshot);
                    let reason = match result {
                        BmsResultKind::Success => "bq40_probe_without_charge_ready",
                        BmsResultKind::Abnormal => "bq40_probe_without_charge_abnormal",
                        BmsResultKind::RomMode => "bq40_probe_without_charge_rom_mode",
                        BmsResultKind::NoBattery => "bq40_probe_without_charge_no_battery",
                        BmsResultKind::NotDetected => "bq40_probe_without_charge_not_detected",
                    };
                    self.finish_bms_activation(result, reason);
                    return true;
                }
            }
        }

        if self.bms_activation_phase == BmsActivationPhase::ProbeWithoutCharge {
            let Some(started_at) = self.bms_activation_started_at else {
                self.finish_bms_activation(BmsResultKind::NotDetected, "activation_phase_missing");
                return false;
            };
            if started_at.elapsed() >= BMS_ACTIVATION_PROBE_WITHOUT_CHARGE_WINDOW {
                if self.bms_activation_force_charge_requested {
                    if let Err(reason) = self.begin_bms_activation_repower_window() {
                        self.finish_bms_activation(BmsResultKind::NotDetected, reason);
                    }
                } else {
                    self.begin_bms_activation_wake_probe();
                }
                return true;
            }
        }

        if self.bms_activation_phase == BmsActivationPhase::WaitChargeOff {
            let Some(started_at) = self.bms_activation_started_at else {
                self.finish_bms_activation(BmsResultKind::NotDetected, "activation_phase_missing");
                return false;
            };
            if started_at.elapsed() >= BMS_ACTIVATION_REPOWER_OFF_WINDOW {
                if let Err(reason) = self.begin_bms_activation_min_charge_path() {
                    self.finish_bms_activation(BmsResultKind::NotDetected, reason);
                }
                return true;
            }
        }

        if let Some(result) = self.maybe_run_bms_activation_min_charge_probe() {
            match result {
                Bq40ActivationProbeResult::Pending => {}
                Bq40ActivationProbeResult::Rom => {
                    self.finish_bms_activation(BmsResultKind::RomMode, "rom_mode_detected");
                    return true;
                }
                Bq40ActivationProbeResult::Working { addr, snapshot } => {
                    let result = self.apply_bq40_activation_snapshot(addr, &snapshot);
                    let reason = match result {
                        BmsResultKind::Success => "bq40_min_charge_probe_ready",
                        BmsResultKind::Abnormal => "bq40_min_charge_probe_abnormal",
                        BmsResultKind::RomMode => "bq40_min_charge_probe_rom_mode",
                        BmsResultKind::NoBattery => "bq40_min_charge_probe_no_battery",
                        BmsResultKind::NotDetected => "bq40_min_charge_probe_not_detected",
                    };
                    self.finish_bms_activation(result, reason);
                    return true;
                }
            }
        }

        if self.bms_activation_phase == BmsActivationPhase::MinChargeProbe {
            let Some(started_at) = self.bms_activation_started_at else {
                self.finish_bms_activation(BmsResultKind::NotDetected, "activation_phase_missing");
                return false;
            };
            if started_at.elapsed() >= BMS_ACTIVATION_MIN_CHARGE_PROBE_WINDOW {
                self.begin_bms_activation_wake_probe();
                return true;
            }
        }

        if let Some(result) = self.maybe_run_bms_activation_wake_probe() {
            match result {
                Bq40ActivationProbeResult::Pending => {}
                Bq40ActivationProbeResult::Rom => {
                    self.finish_bms_activation(BmsResultKind::RomMode, "rom_mode_detected");
                    return true;
                }
                Bq40ActivationProbeResult::Working { addr, snapshot } => {
                    let result = self.apply_bq40_activation_snapshot(addr, &snapshot);
                    let reason = match result {
                        BmsResultKind::Success => "bq40_confirmed_ready",
                        BmsResultKind::Abnormal => "bq40_confirmed_abnormal",
                        BmsResultKind::RomMode => "bq40_confirmed_rom_mode",
                        BmsResultKind::NoBattery => "bq40_confirmed_no_battery",
                        BmsResultKind::NotDetected => "bq40_confirmed_not_detected",
                    };
                    self.finish_bms_activation(result, reason);
                    return true;
                }
            }
        }

        if self.bms_activation_phase == BmsActivationPhase::ProbeWithoutCharge
            && self.bms_activation_diag_stage >= BMS_ACTIVATION_DIAG_STAGE_DELAYS_MS.len()
        {
            if self.bms_activation_force_charge_requested {
                for addr in bms_probe_candidates().iter().copied() {
                    self.log_bq40_activation_mac_probe(addr, "probe_without_charge");
                }
                if let Err(reason) = self.begin_bms_activation_min_charge_path() {
                    self.finish_bms_activation(BmsResultKind::NotDetected, reason);
                }
                return true;
            }
            self.finish_bms_activation(BmsResultKind::NotDetected, "probe_without_charge_miss");
            return true;
        }

        let followup_before = self.bms_activation_followup_attempts;
        if let Some(result) = self.maybe_run_bms_activation_followup_probe() {
            match result {
                Bq40ActivationProbeResult::Pending => {}
                Bq40ActivationProbeResult::Rom => {
                    self.finish_bms_activation(BmsResultKind::RomMode, "rom_mode_detected");
                    return true;
                }
                Bq40ActivationProbeResult::Working { addr, snapshot } => {
                    let result = self.apply_bq40_activation_snapshot(addr, &snapshot);
                    let reason = match result {
                        BmsResultKind::Success => "bq40_followup_ready",
                        BmsResultKind::Abnormal => "bq40_followup_abnormal",
                        BmsResultKind::RomMode => "bq40_followup_rom_mode",
                        BmsResultKind::NoBattery => "bq40_followup_no_battery",
                        BmsResultKind::NotDetected => "bq40_followup_not_detected",
                    };
                    self.finish_bms_activation(result, reason);
                    return true;
                }
            }
        }

        let bms_i2c_active = self.bms_activation_diag_stage != wake_stage_before
            || self.bms_activation_followup_attempts != followup_before
            || self.bms_activation_followup_attempts != probe_without_charge_before;

        if self.is_bq40_rom_mode_detected() {
            self.finish_bms_activation(BmsResultKind::RomMode, "rom_mode_detected");
            return bms_i2c_active;
        }

        match self.ui_snapshot.bq40z50 {
            SelfCheckCommState::Ok => {
                self.finish_bms_activation(BmsResultKind::Success, "bq40_ready");
                return bms_i2c_active;
            }
            SelfCheckCommState::Warn if self.has_trusted_bq40_runtime_evidence() => {
                self.finish_bms_activation(BmsResultKind::Abnormal, "bq40_warn_after_activation");
                return bms_i2c_active;
            }
            _ => {}
        }

        let Some(deadline) = self.bms_activation_deadline else {
            self.finish_bms_activation(BmsResultKind::NotDetected, "activation_deadline_missing");
            return bms_i2c_active;
        };
        if Instant::now() >= deadline {
            let result = if self.is_bq40_rom_mode_detected() {
                BmsResultKind::RomMode
            } else {
                BmsResultKind::NotDetected
            };
            let reason = match result {
                BmsResultKind::RomMode => "deadline_elapsed_rom_mode",
                BmsResultKind::NotDetected => "deadline_elapsed_not_detected",
                BmsResultKind::Success => "deadline_elapsed_success",
                BmsResultKind::Abnormal => "deadline_elapsed_abnormal",
                BmsResultKind::NoBattery => "deadline_elapsed_no_battery",
            };
            self.finish_bms_activation(result, reason);
        }
        bms_i2c_active
    }

    fn maybe_advance_bms_activation_phase(&mut self) -> bool {
        let Some(started_at) = self.bms_activation_started_at else {
            self.finish_bms_activation(BmsResultKind::NotDetected, "activation_phase_missing");
            return false;
        };

        match self.bms_activation_phase {
            BmsActivationPhase::ProbeWithoutCharge => true,
            BmsActivationPhase::WaitChargeOff => true,
            BmsActivationPhase::WakeProbe => true,
            BmsActivationPhase::WaitMinChargeSettle => {
                if started_at.elapsed() < BMS_ACTIVATION_MIN_CHARGE_SETTLE {
                    return false;
                }
                let now = Instant::now();
                self.bms_activation_phase = BmsActivationPhase::MinChargeProbe;
                self.bms_activation_started_at = Some(now);
                self.bms_activation_diag_stage = 0;
                self.bms_activation_followup_next_at = None;
                self.bms_activation_followup_attempts = 0;
                self.bms_activation_exercise_next_at = None;
                self.bms_activation_pattern_tracker = Bq40ActivationPatternTracker::new();
                self.bms_activation_isolation_until = None;
                self.bms_next_poll_at = now;
                self.bms_next_retry_at = None;
                bms_diag_breadcrumb_note(7, 0);
                defmt::info!(
                    "bms: activation phase old={} new={} settle_ms={=u32} probe_window_ms={=u32}",
                    bms_activation_phase_name(BmsActivationPhase::WaitMinChargeSettle),
                    bms_activation_phase_name(self.bms_activation_phase),
                    BMS_ACTIVATION_MIN_CHARGE_SETTLE.as_millis() as u32,
                    BMS_ACTIVATION_MIN_CHARGE_PROBE_WINDOW.as_millis() as u32
                );
                true
            }
            BmsActivationPhase::MinChargeProbe => true,
        }
    }

    fn begin_bms_activation_repower_window(&mut self) -> Result<(), &'static str> {
        let old_phase = self.bms_activation_phase;
        let now = Instant::now();
        self.bms_activation_auto_force_charge_until = None;
        self.bms_activation_auto_force_charge_programmed = false;
        self.bms_activation_phase = BmsActivationPhase::WaitChargeOff;
        self.bms_activation_started_at = Some(now);
        self.bms_activation_diag_stage = 0;
        self.bms_activation_followup_next_at = None;
        self.bms_activation_followup_attempts = 0;
        self.bms_activation_exercise_next_at = None;
        self.bms_activation_pattern_tracker = Bq40ActivationPatternTracker::new();
        self.bms_activation_isolation_until = None;
        self.chg_next_poll_at = now;
        self.chg_next_retry_at = None;
        self.maybe_poll_charger(&IrqSnapshot::default());
        if self.bms_activation_state != BmsActivationState::Pending {
            return Err("charger_poll_failed");
        }
        if self.chg_enabled {
            return Err("disable_charger_for_repower_failed");
        }
        bms_diag_breadcrumb_note(5, 0);
        defmt::info!(
            "bms: activation phase old={} new={} off_ms={=u32}",
            bms_activation_phase_name(old_phase),
            bms_activation_phase_name(self.bms_activation_phase),
            BMS_ACTIVATION_REPOWER_OFF_WINDOW.as_millis() as u32
        );
        Ok(())
    }

    fn begin_bms_activation_min_charge_path(&mut self) -> Result<(), &'static str> {
        let now = Instant::now();
        let old_phase = self.bms_activation_phase;
        self.bms_activation_phase = BmsActivationPhase::WaitMinChargeSettle;
        if let Err(reason) = self.apply_bms_activation_min_charge_profile() {
            return Err(reason);
        }
        self.bms_activation_started_at = Some(now);
        self.bms_activation_diag_stage = 0;
        self.bms_activation_followup_next_at = None;
        self.bms_activation_followup_attempts = 0;
        self.bms_activation_exercise_next_at = None;
        self.bms_activation_pattern_tracker = Bq40ActivationPatternTracker::new();
        self.bms_activation_isolation_until = Some(now);
        self.bms_next_poll_at = now;
        self.bms_next_retry_at = None;
        self.chg_next_poll_at = now;
        self.chg_next_retry_at = None;
        bms_diag_breadcrumb_note(6, 0);
        defmt::info!(
            "bms: activation phase old={} new={} settle_ms={=u32}",
            bms_activation_phase_name(old_phase),
            bms_activation_phase_name(self.bms_activation_phase),
            BMS_ACTIVATION_MIN_CHARGE_SETTLE.as_millis() as u32
        );
        Ok(())
    }

    fn begin_bms_activation_wake_probe(&mut self) {
        let old_phase = self.bms_activation_phase;
        let now = Instant::now();
        self.bms_activation_phase = BmsActivationPhase::WakeProbe;
        self.bms_activation_started_at = Some(now);
        self.bms_activation_diag_stage = 0;
        self.bms_activation_followup_next_at = None;
        self.bms_activation_followup_attempts = 0;
        self.bms_activation_exercise_next_at = Some(now);
        self.bms_activation_pattern_tracker = Bq40ActivationPatternTracker::new();
        self.bms_activation_isolation_until = Some(now);
        bms_diag_breadcrumb_note(8, 0);
        defmt::info!(
            "bms: activation phase old={} new={} wake_stages={=u8}",
            bms_activation_phase_name(old_phase),
            bms_activation_phase_name(self.bms_activation_phase),
            BMS_ACTIVATION_DIAG_STAGE_DELAYS_MS.len() as u8
        );
    }

    fn begin_bms_activation_probe_without_charge(&mut self) -> Result<(), &'static str> {
        let old_phase = self.bms_activation_phase;
        let now = Instant::now();
        self.bms_activation_phase = BmsActivationPhase::ProbeWithoutCharge;
        self.bms_activation_started_at = Some(now);
        self.bms_activation_diag_stage = 0;
        self.bms_activation_followup_next_at = None;
        self.bms_activation_followup_attempts = 0;
        self.bms_activation_exercise_next_at = None;
        self.bms_activation_pattern_tracker = Bq40ActivationPatternTracker::new();
        self.bms_activation_isolation_until = None;
        self.bms_next_poll_at = now;
        self.bms_next_retry_at = None;
        self.chg_next_poll_at = now;
        self.chg_next_retry_at = None;
        self.maybe_poll_charger(&IrqSnapshot::default());
        if self.bms_activation_state != BmsActivationState::Pending {
            return Err("charger_poll_failed");
        }
        self.chg_next_poll_at = Instant::now() + BMS_ACTIVATION_CHARGER_POLL_PERIOD;
        bms_diag_breadcrumb_note(4, 0);
        defmt::info!(
            "bms: activation phase old={} new={} charger_mode=off",
            bms_activation_phase_name(old_phase),
            bms_activation_phase_name(self.bms_activation_phase)
        );
        Ok(())
    }

    fn has_trusted_bq40_runtime_evidence(&self) -> bool {
        self.ui_snapshot.bq40z50_soc_pct.is_some()
            || self.ui_snapshot.bq40z50_rca_alarm.is_some()
            || self.ui_snapshot.bq40z50_discharge_ready.is_some()
    }

    fn apply_bms_activation_min_charge_profile(&mut self) -> Result<(), &'static str> {
        self.chg_ilim_hiz_brk.set_low();
        defmt::info!(
            "bms: activation min_charge_step=program_profile_start vreg_mv={=u16} ichg_ma={=u16} iindpm_ma={=u16}",
            BMS_ACTIVATION_FORCE_VREG_MV,
            BMS_ACTIVATION_FORCE_ICHG_MA,
            BMS_ACTIVATION_FORCE_IINDPM_MA
        );
        if bq25792::set_charge_voltage_limit_mv(&mut self.i2c, BMS_ACTIVATION_FORCE_VREG_MV)
            .is_err()
        {
            return Err("program_activation_profile_failed");
        }
        defmt::info!("bms: activation min_charge_step=program_profile_vreg_ok");
        self.chg_next_poll_at = Instant::now();
        self.chg_next_retry_at = None;
        defmt::info!("bms: activation min_charge_step=poll_charger");
        self.maybe_poll_charger(&IrqSnapshot::default());
        if self.bms_activation_state != BmsActivationState::Pending {
            return Err("charger_poll_failed");
        }
        defmt::info!(
            "bms: activation min_charge_step=poll_charger_ok chg_enabled={=bool}",
            self.chg_enabled
        );
        if !self.chg_enabled {
            return Err("enable_charger_for_activation_failed");
        }
        Ok(())
    }

    fn finish_bms_activation(&mut self, result: BmsResultKind, reason: &'static str) {
        bms_diag_breadcrumb_note(
            13,
            match result {
                BmsResultKind::Success => 1,
                BmsResultKind::NoBattery => 2,
                BmsResultKind::RomMode => 3,
                BmsResultKind::Abnormal => 4,
                BmsResultKind::NotDetected => 5,
            },
        );
        let mut restore_chg_enabled = false;
        if let Some(chg_enabled) = self.restore_bms_activation_charger_backup(reason) {
            restore_chg_enabled = chg_enabled;
        }
        if restore_chg_enabled {
            self.chg_ce.set_low();
            self.chg_enabled = true;
        } else {
            self.chg_ce.set_high();
            self.chg_enabled = false;
        }
        self.bms_activation_deadline = None;
        self.bms_activation_phase = BmsActivationPhase::WakeProbe;
        self.bms_activation_started_at = None;
        self.bms_activation_diag_stage = 0;
        self.bms_activation_followup_next_at = None;
        self.bms_activation_followup_attempts = 0;
        self.bms_activation_exercise_next_at = None;
        self.bms_activation_pattern_tracker = Bq40ActivationPatternTracker::new();
        self.bms_activation_isolation_until = None;
        self.bms_activation_force_charge_requested = false;
        self.bms_activation_current_is_auto = false;
        self.bms_activation_state = BmsActivationState::Result(result);
        self.ui_snapshot.bq40z50_last_result = Some(result);
        self.chg_next_poll_at = Instant::now();
        self.maybe_restore_charger_watchdog_after_activation();
        match result {
            BmsResultKind::Success => defmt::info!(
                "bms: activation finish result={} reason={} bq40_state={} soc_pct={=?} rca_alarm={=?} dsg_ready={=?} charger_state={} allow_charge={=?} vbat_present={=?} input_present={=?} restore_chg_enabled={=bool}",
                bms_result_name(result),
                reason,
                self_check_comm_state_name(self.ui_snapshot.bq40z50),
                self.ui_snapshot.bq40z50_soc_pct,
                self.ui_snapshot.bq40z50_rca_alarm,
                self.ui_snapshot.bq40z50_discharge_ready,
                self_check_comm_state_name(self.ui_snapshot.bq25792),
                self.ui_snapshot.bq25792_allow_charge,
                self.ui_snapshot.bq25792_vbat_present,
                self.ui_snapshot.fusb302_vbus_present,
                restore_chg_enabled
            ),
            _ => defmt::warn!(
                "bms: activation finish result={} reason={} bq40_state={} soc_pct={=?} rca_alarm={=?} dsg_ready={=?} charger_state={} allow_charge={=?} vbat_present={=?} input_present={=?} restore_chg_enabled={=bool}",
                bms_result_name(result),
                reason,
                self_check_comm_state_name(self.ui_snapshot.bq40z50),
                self.ui_snapshot.bq40z50_soc_pct,
                self.ui_snapshot.bq40z50_rca_alarm,
                self.ui_snapshot.bq40z50_discharge_ready,
                self_check_comm_state_name(self.ui_snapshot.bq25792),
                self.ui_snapshot.bq25792_allow_charge,
                self.ui_snapshot.bq25792_vbat_present,
                self.ui_snapshot.fusb302_vbus_present,
                restore_chg_enabled
            ),
        }
    }

    fn is_bq40_rom_mode_detected(&mut self) -> bool {
        for addr in bms_probe_candidates().iter().copied() {
            match bq40z50::read_u16(&mut self.i2c, addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE) {
                Ok(sig) if sig == BMS_ROM_MODE_SIGNATURE => {
                    defmt::warn!("bms: bq40z50 rom_mode_detected addr=0x{=u8:x}", addr);
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    fn current_mains_present(&self) -> Option<bool> {
        stable_mains_present(
            self.ui_snapshot.vin_mains_present,
            self.ui_snapshot.vin_vbus_mv,
            self.ui_snapshot.fusb302_vbus_present,
        )
    }

    fn current_output_ilimit_ma(&self) -> u16 {
        self.output_protection.applied_ilim_ma
    }

    fn output_protection_config(&self) -> output_protection::ProtectionConfig {
        output_protection::ProtectionConfig {
            temp_enter_c_x16: self.cfg.protect_temp_derate_c_x16,
            temp_exit_c_x16: self.cfg.protect_temp_resume_c_x16,
            temp_hold_ms: self.cfg.protect_temp_hold.as_millis() as u64,
            current_enter_ma: self.cfg.protect_current_derate_ma,
            current_exit_ma: self.cfg.protect_current_resume_ma,
            current_hold_ms: self.cfg.protect_current_hold.as_millis() as u64,
            ilim_step_ma: self.cfg.protect_ilim_step_ma,
            ilim_step_interval_ms: self.cfg.protect_ilim_step_interval.as_millis() as u64,
            min_ilim_ma: self.cfg.protect_min_ilim_ma,
            shutdown_vout_mv: self.cfg.protect_shutdown_vout_mv,
            shutdown_hold_ms: self.cfg.protect_shutdown_hold.as_millis() as u64,
        }
    }

    fn output_gate_reason_now(&mut self) -> OutputGateReason {
        if self.therm_kill.is_low() {
            return OutputGateReason::ThermKill;
        }

        if self.bms_addr.is_none() || self.ui_snapshot.bq40z50_discharge_ready != Some(true) {
            return OutputGateReason::BmsNotReady;
        }

        for ch in [OutputChannel::OutA, OutputChannel::OutB] {
            if !self.output_state.requested_outputs.is_enabled(ch) {
                continue;
            }
            let status = ::tps55288::Tps55288::with_address(&mut self.i2c, ch.addr())
                .read_reg(::tps55288::registers::addr::STATUS)
                .ok()
                .map(::tps55288::registers::StatusBits::from_bits_truncate);
            if let Some(bits) = status {
                if bits.intersects(
                    ::tps55288::registers::StatusBits::SCP
                        | ::tps55288::registers::StatusBits::OCP
                        | ::tps55288::registers::StatusBits::OVP,
                ) {
                    return OutputGateReason::TpsFault;
                }
            }
        }

        if self.output_protection.phase == output_protection::ProtectionPhase::Shutdown {
            return OutputGateReason::ActiveProtection;
        }

        OutputGateReason::None
    }

    fn apply_output_gate(&mut self, gate_reason: OutputGateReason) {
        if gate_reason == OutputGateReason::None {
            return;
        }
        let next_state = output_state_gate_transition(self.output_state, gate_reason);
        if next_state == self.output_state {
            return;
        }
        self.output_state = next_state;
        self.force_disable_outputs();
        defmt::warn!(
            "power: outputs gated reason={} recoverable_outputs={} requested_outputs={}",
            gate_reason.as_str(),
            self.output_state.recoverable_outputs.describe(),
            self.output_state.requested_outputs.describe()
        );
    }

    fn reconcile_output_state(&mut self) {
        let gate_reason = self.output_gate_reason_now();
        if gate_reason != OutputGateReason::None {
            self.apply_output_gate(gate_reason);
            return;
        }

        if self.output_state.gate_reason != OutputGateReason::None {
            defmt::info!(
                "power: outputs gate cleared previous_reason={} recoverable_outputs={} mains_present={=?}",
                self.output_state.gate_reason.as_str(),
                self.output_state.recoverable_outputs.describe(),
                self.current_mains_present()
            );
            self.output_state =
                output_state_gate_transition(self.output_state, OutputGateReason::None);
        }
    }

    #[allow(dead_code)]
    fn can_request_output_restore(&self) -> bool {
        output_restore_pending_from_state(self.output_state, self.current_mains_present())
    }

    fn apply_output_current_limit(&mut self, limit_ma: u16) {
        for ch in [OutputChannel::OutA, OutputChannel::OutB] {
            if !self.output_state.active_outputs.is_enabled(ch) {
                continue;
            }

            let result = ::tps55288::Tps55288::with_address(&mut self.i2c, ch.addr())
                .set_ilim_ma(limit_ma, true)
                .map_err(tps_error_kind);
            match result {
                Ok(()) => defmt::warn!(
                    "power: active_protection set_ilim ch={} limit_ma={=u16}",
                    ch.name(),
                    limit_ma
                ),
                Err(kind) => defmt::warn!(
                    "power: active_protection set_ilim_failed ch={} limit_ma={=u16} err={}",
                    ch.name(),
                    limit_ma,
                    kind
                ),
            }
        }
    }

    fn update_output_protection(&mut self) {
        if self.output_state.requested_outputs == EnabledOutputs::None {
            return;
        }

        let mut max_temp_c_x16 = None;
        let mut max_current_ma = None;
        let mut min_vout_mv = None;

        for ch in [OutputChannel::OutA, OutputChannel::OutB] {
            if !self.output_state.requested_outputs.is_enabled(ch) {
                continue;
            }

            let temp = match ch {
                OutputChannel::OutA => self.ui_snapshot.tmp_a_c_x16,
                OutputChannel::OutB => self.ui_snapshot.tmp_b_c_x16,
            };
            if let Some(temp_c_x16) = temp {
                max_temp_c_x16 =
                    Some(max_temp_c_x16.map_or(temp_c_x16, |cur: i16| cur.max(temp_c_x16)));
            }

            let current = match ch {
                OutputChannel::OutA => self.ui_snapshot.tps_a_iout_ma,
                OutputChannel::OutB => self.ui_snapshot.tps_b_iout_ma,
            };
            if let Some(current_ma) = current {
                let current_ma = current_ma.max(0);
                max_current_ma =
                    Some(max_current_ma.map_or(current_ma, |cur: i32| cur.max(current_ma)));
            }

            let vout = match ch {
                OutputChannel::OutA => self.ui_snapshot.out_a_vbus_mv,
                OutputChannel::OutB => self.ui_snapshot.out_b_vbus_mv,
            };
            if let Some(vout_mv) = vout {
                min_vout_mv = Some(min_vout_mv.map_or(vout_mv, |cur: u16| cur.min(vout_mv)));
            }
        }

        let result = output_protection::step(
            self.fan_now_ms(),
            self.output_protection_config(),
            self.cfg.ilimit_ma,
            self.output_protection,
            output_protection::ProtectionInputs {
                max_temp_c_x16,
                max_current_ma,
                min_vout_mv,
            },
        );

        let prev = self.output_protection;
        self.output_protection = result.runtime;
        match result.action {
            output_protection::ProtectionAction::None => {}
            output_protection::ProtectionAction::ApplyIlim(limit_ma) => {
                self.apply_output_current_limit(limit_ma);
                defmt::warn!(
                    "power: active_protection derating reason={} ilim_ma={=u16} max_temp_c_x16={=?} max_current_ma={=?} min_vout_mv={=?}",
                    self.output_protection.status.reason().as_str(),
                    limit_ma,
                    max_temp_c_x16,
                    max_current_ma,
                    min_vout_mv
                );
            }
            output_protection::ProtectionAction::RestoreDefaultIlim(limit_ma) => {
                self.apply_output_current_limit(limit_ma);
                defmt::info!(
                    "power: active_protection cleared restore_ilim_ma={=u16}",
                    limit_ma
                );
            }
            output_protection::ProtectionAction::Shutdown(reason) => {
                defmt::error!(
                    "power: active_protection shutdown reason={} ilim_ma={=u16} min_vout_mv={=?} max_temp_c_x16={=?} max_current_ma={=?}",
                    reason.as_str(),
                    prev.applied_ilim_ma,
                    min_vout_mv,
                    max_temp_c_x16,
                    max_current_ma
                );
                self.apply_output_gate(OutputGateReason::ActiveProtection);
            }
        }
    }

    fn recompute_ui_mode(&mut self) {
        let has_output = self.ui_snapshot.tps_a_enabled == Some(true)
            || self.ui_snapshot.tps_b_enabled == Some(true);
        self.ui_snapshot.mode = ups_mode_from_mains(
            stable_mains_present(
                self.ui_snapshot.vin_mains_present,
                self.ui_snapshot.vin_vbus_mv,
                self.ui_snapshot.fusb302_vbus_present,
            ),
            has_output,
        );
    }

    fn refresh_audio_signals(&mut self) {
        let hold_no_battery_result = self.should_hold_no_battery_result();
        let mains_state = stable_mains_state(
            self.ui_snapshot.vin_mains_present,
            self.ui_snapshot.vin_vbus_mv,
            self.ui_snapshot.fusb302_vbus_present,
        );
        let mains_present = mains_state.present;
        let tmp_a_hot = self
            .cfg
            .detected_tmp_outputs
            .is_enabled(OutputChannel::OutA)
            && self
                .ui_snapshot
                .tmp_a_c
                .is_some_and(|temp_c| temp_c.saturating_mul(16) >= self.cfg.tmp112_tlow_c_x16);
        let tmp_b_hot = self
            .cfg
            .detected_tmp_outputs
            .is_enabled(OutputChannel::OutB)
            && self
                .ui_snapshot
                .tmp_b_c
                .is_some_and(|temp_c| temp_c.saturating_mul(16) >= self.cfg.tmp112_tlow_c_x16);
        let raw_battery_low = match self.bms_audio.rca_alarm {
            Some(true) => match mains_present {
                Some(true) => AudioBatteryLowState::WithMains,
                Some(false) => AudioBatteryLowState::NoMains,
                None => AudioBatteryLowState::Unknown,
            },
            Some(false) => AudioBatteryLowState::Inactive,
            None => AudioBatteryLowState::Unknown,
        };
        let module_fault = if hold_no_battery_result {
            false
        } else {
            (self.cfg.charger_probe_ok && self.charger_audio.module_fault)
                || (self.bms_runtime_seen && self.bms_audio.module_fault)
                || (self.cfg.ina_detected
                    && matches!(self.ui_snapshot.ina3221, SelfCheckCommState::Err))
                || (self
                    .cfg
                    .detected_tps_outputs
                    .is_enabled(OutputChannel::OutA)
                    && matches!(self.ui_snapshot.tps_a, SelfCheckCommState::Err))
                || (self
                    .cfg
                    .detected_tps_outputs
                    .is_enabled(OutputChannel::OutB)
                    && matches!(self.ui_snapshot.tps_b, SelfCheckCommState::Err))
                || (self
                    .cfg
                    .detected_tmp_outputs
                    .is_enabled(OutputChannel::OutA)
                    && matches!(self.ui_snapshot.tmp_a, SelfCheckCommState::Err))
                || (self
                    .cfg
                    .detected_tmp_outputs
                    .is_enabled(OutputChannel::OutB)
                    && matches!(self.ui_snapshot.tmp_b, SelfCheckCommState::Err))
        };
        let therm_kill_asserted = self.therm_kill.is_low();
        let battery_protection = if hold_no_battery_result {
            false
        } else {
            self.bms_audio.protection_active
        };
        let battery_low = if battery_protection {
            AudioBatteryLowState::Inactive
        } else {
            raw_battery_low
        };
        let snapshot = AudioSignalSnapshot {
            mains_present,
            mains_source: mains_state.source,
            charge_phase: self.charger_audio.phase,
            thermal_stress: self.charger_audio.thermal_stress || tmp_a_hot || tmp_b_hot,
            battery_low,
            battery_protection,
            module_fault,
            io_over_voltage: self.charger_audio.over_voltage || self.tps_audio.any_over_voltage(),
            io_over_current: self.charger_audio.over_current || self.tps_audio.any_over_current(),
            shutdown_protection: therm_kill_asserted
                || self.charger_audio.shutdown_protection
                || self.output_protection.phase == output_protection::ProtectionPhase::Shutdown,
        };

        if !self.audio_signals_ready {
            self.audio_snapshot = snapshot;
            self.audio_events = AudioSignalEvents::default();
            self.audio_signals_ready = true;
            return;
        }

        let prev = self.audio_snapshot;
        if let Some(edge) = mains_present_edge(
            StableMainsState {
                present: prev.mains_present,
                source: prev.mains_source,
            },
            StableMainsState {
                present: snapshot.mains_present,
                source: snapshot.mains_source,
            },
        ) {
            self.audio_events.mains_present_changed = Some(edge);
        }
        if prev.charge_phase != AudioChargePhase::Unknown
            && snapshot.charge_phase != AudioChargePhase::Unknown
            && prev.charge_phase != snapshot.charge_phase
        {
            self.audio_events.charge_phase_changed = Some(snapshot.charge_phase);
        }
        if prev.thermal_stress != snapshot.thermal_stress {
            self.audio_events.thermal_stress_changed = Some(snapshot.thermal_stress);
        }
        if prev.battery_low != snapshot.battery_low {
            self.audio_events.battery_low_changed = Some(snapshot.battery_low);
            defmt::info!(
                "audio: battery_low changed old={} new={} raw_new={} suppressed_by_battery_protection={=bool}",
                audio_battery_low_state_name(prev.battery_low),
                audio_battery_low_state_name(snapshot.battery_low),
                audio_battery_low_state_name(raw_battery_low),
                battery_protection && raw_battery_low != AudioBatteryLowState::Inactive
            );
        }
        if prev.battery_protection != snapshot.battery_protection {
            self.audio_events.battery_protection_changed = Some(snapshot.battery_protection);
        }
        if prev.module_fault != snapshot.module_fault {
            self.audio_events.module_fault_changed = Some(snapshot.module_fault);
            defmt::info!(
                "audio: module_fault changed old={=bool} new={=bool} hold_no_battery={=bool} bq40_last_result={} bq40_state={} tps_a={} tps_b={} bms_audio_fault={=bool}",
                prev.module_fault,
                snapshot.module_fault,
                hold_no_battery_result,
                bms_result_option_name(self.ui_snapshot.bq40z50_last_result),
                self_check_comm_state_name(self.ui_snapshot.bq40z50),
                self_check_comm_state_name(self.ui_snapshot.tps_a),
                self_check_comm_state_name(self.ui_snapshot.tps_b),
                self.bms_audio.module_fault
            );
        }
        if prev.io_over_voltage != snapshot.io_over_voltage {
            self.audio_events.io_over_voltage_changed = Some(snapshot.io_over_voltage);
        }
        if prev.io_over_current != snapshot.io_over_current {
            self.audio_events.io_over_current_changed = Some(snapshot.io_over_current);
        }
        if prev.shutdown_protection != snapshot.shutdown_protection {
            self.audio_events.shutdown_protection_changed = Some(snapshot.shutdown_protection);
        }
        self.audio_snapshot = snapshot;
    }

    fn should_hold_no_battery_result(&self) -> bool {
        self.ui_snapshot.bq40z50_last_result == Some(BmsResultKind::NoBattery)
            && self.bms_activation_state != BmsActivationState::Pending
    }

    fn hold_no_battery_result_audio_state(&mut self) {
        self.ui_snapshot.bq40z50 = SelfCheckCommState::Warn;
        self.ui_snapshot.bq40z50_pack_mv = None;
        self.ui_snapshot.bq40z50_current_ma = None;
        self.reset_bms_detail_mac_cache(Instant::now());
        self.clear_bms_detail_snapshot();
        if self.ui_snapshot.bq40z50_soc_pct.is_none() {
            self.ui_snapshot.bq40z50_soc_pct = Some(0);
        }
        self.ui_snapshot.bq40z50_rca_alarm = Some(true);
        self.ui_snapshot.bq40z50_no_battery = Some(true);
        self.ui_snapshot.bq40z50_discharge_ready = Some(false);
        self.bms_audio = BmsAudioState {
            rca_alarm: Some(true),
            protection_active: false,
            module_fault: false,
        };
    }

    fn refresh_tps_audio_state(&mut self) {
        for ch in [OutputChannel::OutA, OutputChannel::OutB] {
            if !self.cfg.detected_tps_outputs.is_enabled(ch) {
                continue;
            }
            let status = ::tps55288::Tps55288::with_address(&mut self.i2c, ch.addr())
                .read_reg(::tps55288::registers::addr::STATUS)
                .ok()
                .map(::tps55288::registers::StatusBits::from_bits_truncate);
            let Some(bits) = status else {
                continue;
            };
            let over_voltage = bits.contains(::tps55288::registers::StatusBits::OVP);
            let over_current = bits.contains(::tps55288::registers::StatusBits::OCP)
                || bits.contains(::tps55288::registers::StatusBits::SCP);
            match ch {
                OutputChannel::OutA => {
                    self.tps_audio.out_a_over_voltage = over_voltage;
                    self.tps_audio.out_a_over_current = over_current;
                }
                OutputChannel::OutB => {
                    self.tps_audio.out_b_over_voltage = over_voltage;
                    self.tps_audio.out_b_over_current = over_current;
                }
            }
        }
    }

    fn maybe_retry(&mut self) {
        let now = Instant::now();

        if !self.ina_ready {
            if let Some(t) = self.ina_next_retry_at {
                if now >= t {
                    self.ina_next_retry_at = None;
                    self.try_init_ina();
                }
            }
        }

        if !self.tps_a_ready
            && self
                .output_state
                .active_outputs
                .is_enabled(OutputChannel::OutA)
        {
            if let Some(t) = self.tps_a_next_retry_at {
                if now >= t {
                    self.tps_a_next_retry_at = None;
                    self.try_configure_tps(OutputChannel::OutA);
                }
            }
        }

        if !self.tps_b_ready
            && self
                .output_state
                .active_outputs
                .is_enabled(OutputChannel::OutB)
        {
            if let Some(t) = self.tps_b_next_retry_at {
                if now >= t {
                    self.tps_b_next_retry_at = None;
                    self.try_configure_tps(OutputChannel::OutB);
                }
            }
        }
    }

    fn try_init_ina(&mut self) {
        let cfg = if self.cfg.telemetry_include_vin_ch3 {
            ina3221::CONFIG_VALUE_CH123
        } else {
            ina3221::CONFIG_VALUE_CH12
        };

        // INA3221 has an IIR-style averaging filter (AVG bits). If we re-flash the MCU while the
        // board stays powered, stale register values can linger and take a long time to settle.
        // Force a device reset before applying our desired config.
        let _ = ina3221::init_with_config(&mut self.i2c, 0x8000).map_err(|e| {
            defmt::warn!("power: ina3221 reset err={}", ina_error_kind(e));
        });
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(2) {}

        match ina3221::init_with_config(&mut self.i2c, cfg) {
            Ok(()) => {
                self.ina_ready = true;
                self.ui_snapshot.ina3221 = SelfCheckCommState::Ok;
                let cfg_read = ina3221::read_config(&mut self.i2c).map_err(ina_error_kind);
                let man = ina3221::read_manufacturer_id(&mut self.i2c).map_err(ina_error_kind);
                let die = ina3221::read_die_id(&mut self.i2c).map_err(ina_error_kind);
                defmt::info!(
                    "power: ina3221 ok (addr=0x40 cfg_wr=0x{=u16:x} cfg_rd={=?} man_id={=?} die_id={=?})",
                    cfg,
                    cfg_read,
                    man,
                    die
                );
            }
            Err(e) => {
                self.ina_ready = false;
                self.ui_snapshot.ina3221 = SelfCheckCommState::Err;
                self.ina_next_retry_at = Some(Instant::now() + self.cfg.retry_backoff);
                defmt::error!("power: ina3221 err={}", ina_error_kind(e));
            }
        }
    }

    fn try_configure_tps(&mut self, ch: OutputChannel) {
        let enabled = self.output_state.active_outputs.is_enabled(ch);
        let addr = ch.addr();
        let ilimit_ma = self.current_output_ilimit_ma();

        match tps55288::configure_one(&mut self.i2c, ch, enabled, self.cfg.vout_mv, ilimit_ma) {
            Ok(()) => {
                tps55288::log_configured(&mut self.i2c, ch, enabled);
                self.mark_tps_ok(ch);
                match ch {
                    OutputChannel::OutA => {
                        self.ui_snapshot.tps_a = SelfCheckCommState::Ok;
                        self.ui_snapshot.tps_a_enabled = Some(enabled);
                    }
                    OutputChannel::OutB => {
                        self.ui_snapshot.tps_b = SelfCheckCommState::Ok;
                        self.ui_snapshot.tps_b_enabled = Some(enabled);
                    }
                }
            }
            Err((stage, e)) => {
                let kind = tps_error_kind(e);
                self.mark_tps_failed(ch, Instant::now() + self.cfg.retry_backoff);
                match ch {
                    OutputChannel::OutA => {
                        self.ui_snapshot.tps_a = SelfCheckCommState::Err;
                        self.ui_snapshot.tps_a_enabled = Some(false);
                    }
                    OutputChannel::OutB => {
                        self.ui_snapshot.tps_b = SelfCheckCommState::Err;
                        self.ui_snapshot.tps_b_enabled = Some(false);
                    }
                }
                defmt::error!(
                    "power: tps addr=0x{=u8:x} stage={} err={}",
                    addr,
                    stage.as_str(),
                    kind
                );
                if kind == "i2c_nack" && ch == OutputChannel::OutB {
                    defmt::warn!(
                        "power: tps addr=0x75 nack_hint=maybe_address_changed; power-cycle TPS rails to restore preset address"
                    );
                }
            }
        }
        self.recompute_ui_mode();
    }

    fn mark_tps_ok(&mut self, ch: OutputChannel) {
        match ch {
            OutputChannel::OutA => self.tps_a_ready = true,
            OutputChannel::OutB => self.tps_b_ready = true,
        }
    }

    fn mark_tps_failed(&mut self, ch: OutputChannel, next: Instant) {
        match ch {
            OutputChannel::OutA => {
                self.tps_a_ready = false;
                self.tps_a_next_retry_at = Some(next);
            }
            OutputChannel::OutB => {
                self.tps_b_ready = false;
                self.tps_b_next_retry_at = Some(next);
            }
        }
    }

    fn maybe_handle_fault(&mut self, irq: &IrqSnapshot) {
        if self.output_state.requested_outputs == EnabledOutputs::None {
            return;
        }

        let now = Instant::now();
        if self.i2c1_int.is_low() || irq.i2c1_int != 0 {
            if tps55288::should_log_fault(
                now,
                &mut self.last_fault_log_at,
                self.cfg.fault_log_min_interval,
            ) {
                if self
                    .output_state
                    .requested_outputs
                    .is_enabled(OutputChannel::OutA)
                {
                    tps55288::log_fault_status(&mut self.i2c, OutputChannel::OutA, self.ina_ready);
                }
                if self
                    .output_state
                    .requested_outputs
                    .is_enabled(OutputChannel::OutB)
                {
                    tps55288::log_fault_status(&mut self.i2c, OutputChannel::OutB, self.ina_ready);
                }
            }
            self.refresh_tps_audio_state();
        }
    }

    fn note_skipped_vin_telemetry_if_due(&mut self, now: Instant) {
        if now < self.next_vin_telemetry_skip_at {
            return;
        }
        self.next_vin_telemetry_skip_at = now + self.cfg.telemetry_period;
        mark_vin_telemetry_unavailable(
            self.cfg.telemetry_include_vin_ch3,
            &mut self.ui_snapshot.vin_vbus_mv,
            &mut self.ui_snapshot.vin_iin_ma,
            &mut self.ui_snapshot.vin_mains_present,
            &mut self.vin_sample_missing_streak,
        );
        self.recompute_ui_mode();
    }

    fn refresh_vin_telemetry(&mut self, now: Instant) {
        self.next_vin_telemetry_skip_at = now + self.cfg.telemetry_period;
        if self.cfg.telemetry_include_vin_ch3 {
            if self.ina_ready {
                let bus = ina3221::read_bus_mv(&mut self.i2c, ina3221::Channel::Ch3);
                let shunt = ina3221::read_shunt_uv(&mut self.i2c, ina3221::Channel::Ch3);
                let vbus_mv = match bus {
                    Ok(v) => {
                        self.ui_snapshot.vin_vbus_mv = u16::try_from(v).ok();
                        self.ui_snapshot.vin_mains_present =
                            mains_present_from_vin(self.ui_snapshot.vin_vbus_mv);
                        self.vin_sample_missing_streak = 0;
                        TelemetryValue::Value(v)
                    }
                    Err(e) => {
                        mark_vin_telemetry_unavailable(
                            self.cfg.telemetry_include_vin_ch3,
                            &mut self.ui_snapshot.vin_vbus_mv,
                            &mut self.ui_snapshot.vin_iin_ma,
                            &mut self.ui_snapshot.vin_mains_present,
                            &mut self.vin_sample_missing_streak,
                        );
                        TelemetryValue::Err(ina_error_kind(e))
                    }
                };
                let current_ma = match shunt {
                    Ok(shunt_uv) => {
                        let current_ma = ina3221::shunt_uv_to_current_ma(shunt_uv, 7);
                        self.ui_snapshot.vin_iin_ma = Some(current_ma);
                        TelemetryValue::Value(current_ma)
                    }
                    Err(e) => {
                        self.ui_snapshot.vin_iin_ma = None;
                        TelemetryValue::Err(ina_error_kind(e))
                    }
                };
                defmt::info!(
                    "telemetry ch=vin addr=0x40 vbus_mv={} current_ma={}",
                    vbus_mv,
                    current_ma
                );
            } else {
                mark_vin_telemetry_unavailable(
                    self.cfg.telemetry_include_vin_ch3,
                    &mut self.ui_snapshot.vin_vbus_mv,
                    &mut self.ui_snapshot.vin_iin_ma,
                    &mut self.ui_snapshot.vin_mains_present,
                    &mut self.vin_sample_missing_streak,
                );
                defmt::info!(
                    "telemetry ch=vin addr=0x40 vbus_mv={} current_ma={}",
                    TelemetryValue::Err("ina_uninit"),
                    TelemetryValue::Err("ina_uninit")
                );
            }
        } else {
            mark_vin_telemetry_unavailable(
                self.cfg.telemetry_include_vin_ch3,
                &mut self.ui_snapshot.vin_vbus_mv,
                &mut self.ui_snapshot.vin_iin_ma,
                &mut self.ui_snapshot.vin_mains_present,
                &mut self.vin_sample_missing_streak,
            );
        }
    }

    fn maybe_print_telemetry(&mut self) -> bool {
        let now = Instant::now();
        if now < self.next_telemetry_at {
            return false;
        }
        self.next_telemetry_at = now + self.cfg.telemetry_period;

        if self.output_state.requested_outputs == EnabledOutputs::None {
            self.refresh_tmp112_snapshot(OutputChannel::OutA);
            self.refresh_tmp112_snapshot(OutputChannel::OutB);
            self.next_fan_temp_refresh_at = now + self.cfg.telemetry_period;
            self.refresh_vin_telemetry(now);
            self.recompute_ui_mode();
            return true;
        }

        let therm_kill_n: u8 = if self.therm_kill.is_low() { 0 } else { 1 };
        if therm_kill_n == 0
            && tps55288::should_log_fault(
                now,
                &mut self.last_therm_kill_hint_at,
                self.cfg.fault_log_min_interval,
            )
        {
            self.log_therm_kill_hint();
        }

        if self
            .output_state
            .requested_outputs
            .is_enabled(OutputChannel::OutA)
        {
            let capture = tps55288::print_telemetry_line(
                &mut self.i2c,
                OutputChannel::OutA,
                self.ina_ready,
                therm_kill_n,
            );
            self.ui_snapshot.tps_a = if !capture.comm_ok {
                SelfCheckCommState::Err
            } else if capture.fault_active {
                SelfCheckCommState::Warn
            } else {
                SelfCheckCommState::Ok
            };
            if let Some(enabled) = capture.output_enabled {
                self.ui_snapshot.tps_a_enabled = Some(enabled);
            }
            self.ui_snapshot.out_a_vbus_mv = capture.vbus_mv;
            self.ui_snapshot.tps_a_iout_ma = capture.current_ma;
            self.ui_snapshot.tmp_a = if capture.temp_c_x16.is_some() {
                SelfCheckCommState::Ok
            } else {
                SelfCheckCommState::Err
            };
            self.ui_snapshot.tmp_a_c_x16 = capture.temp_c_x16;
            self.ui_snapshot.tmp_a_c = capture.temp_c_x16.map(|v| v / 16);
        }
        if self
            .output_state
            .requested_outputs
            .is_enabled(OutputChannel::OutB)
        {
            let capture = tps55288::print_telemetry_line(
                &mut self.i2c,
                OutputChannel::OutB,
                self.ina_ready,
                therm_kill_n,
            );
            self.ui_snapshot.tps_b = if !capture.comm_ok {
                SelfCheckCommState::Err
            } else if capture.fault_active {
                SelfCheckCommState::Warn
            } else {
                SelfCheckCommState::Ok
            };
            if let Some(enabled) = capture.output_enabled {
                self.ui_snapshot.tps_b_enabled = Some(enabled);
            }
            self.ui_snapshot.out_b_vbus_mv = capture.vbus_mv;
            self.ui_snapshot.tps_b_iout_ma = capture.current_ma;
            self.ui_snapshot.tmp_b = if capture.temp_c_x16.is_some() {
                SelfCheckCommState::Ok
            } else {
                SelfCheckCommState::Err
            };
            self.ui_snapshot.tmp_b_c_x16 = capture.temp_c_x16;
            self.ui_snapshot.tmp_b_c = capture.temp_c_x16.map(|v| v / 16);
        } else {
            self.refresh_tmp112_snapshot(OutputChannel::OutB);
        }
        if !self
            .output_state
            .requested_outputs
            .is_enabled(OutputChannel::OutA)
        {
            self.refresh_tmp112_snapshot(OutputChannel::OutA);
        }
        self.next_fan_temp_refresh_at = now + self.cfg.telemetry_period;

        self.refresh_tps_audio_state();

        self.ui_snapshot.ina_total_ma = match (
            self.ui_snapshot.tps_a_iout_ma,
            self.ui_snapshot.tps_b_iout_ma,
        ) {
            (Some(a), Some(b)) => Some(a + b),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };

        self.refresh_vin_telemetry(now);
        self.recompute_ui_mode();
        true
    }

    fn maybe_poll_charger(&mut self, irq: &IrqSnapshot) {
        if !self.charger_allowed {
            self.ui_snapshot.bq25792_allow_charge = Some(false);
            self.ui_snapshot.bq25792_ichg_ma = None;
            self.ui_snapshot.bq25792_vbat_present = None;
            self.clear_charger_detail_snapshot();
            self.recompute_ui_mode();
            return;
        }

        // Keep the charger polling independent from the TPS/INA telemetry period.
        const INT_MIN_INTERVAL: Duration = Duration::from_millis(50);

        let now = Instant::now();
        let activation_pending = self.bms_activation_state == BmsActivationState::Pending;
        let activation_auto_probe_hold_charge = activation_pending
            && self.bms_activation_current_is_auto
            && self.bms_activation_phase == BmsActivationPhase::ProbeWithoutCharge;
        let activation_force_charge = activation_pending
            && self.bms_activation_force_charge_requested
            && (bms_activation_phase_allows_force_charge(self.bms_activation_phase)
                || activation_auto_probe_hold_charge);
        let activation_force_charge_off =
            activation_pending && bms_activation_phase_forces_charge_off(self.bms_activation_phase);
        let poll_period = if activation_pending {
            BMS_ACTIVATION_CHARGER_POLL_PERIOD
        } else {
            Duration::from_secs(1)
        };
        let auto_force_charge = self.cfg.bms_boot_diag_auto_validate
            && !activation_pending
            && self
                .bms_activation_auto_force_charge_until
                .map_or(false, |until| now < until)
            && (!self.bms_activation_auto_attempted || BMS_BOOT_DIAG_TOOL_STYLE_PROBE_ONLY);
        let boot_diag_ship_reset_due = self.cfg.bms_boot_diag_auto_validate
            && !activation_pending
            && !self.bms_activation_auto_attempted
            && cfg!(feature = "bms-dual-probe-diag")
            && !self.bms_boot_diag_ship_reset_attempted
            && self.bms_addr.is_none()
            && now >= self.bms_boot_diag_started_at + BMS_BOOT_DIAG_SHIP_RESET_DELAY;
        let mut due = now >= self.chg_next_poll_at;
        if irq.chg_int != 0 && !activation_pending && !auto_force_charge {
            let allow = self
                .chg_last_int_poll_at
                .map_or(true, |t| now >= t + INT_MIN_INTERVAL);
            if allow {
                due = true;
                self.chg_last_int_poll_at = Some(now);
            }
        }
        if !due {
            return;
        }
        if let Some(next_retry_at) = self.chg_next_retry_at {
            if now < next_retry_at {
                return;
            }
        }
        self.chg_next_poll_at = now + poll_period;

        // Snapshot key registers with multi-byte reads (BQ25792 supports crossing boundaries).
        let mut st = [0u8; 5];
        let mut fault = [0u8; 2];

        let ctrl0 = match bq25792::read_u8(&mut self.i2c, bq25792::reg::CHARGER_CONTROL_0) {
            Ok(v) => v,
            Err(e) => {
                self.mark_charger_poll_failed(now);
                defmt::error!(
                    "charger: bq25792 err stage=ctrl0_read err={}",
                    i2c_error_kind(e)
                );
                return;
            }
        };

        // Only enforce ship-FET path when charging is policy-enabled.
        let (sfet_present_before, sfet_present_after, ship_mode_before, ship_mode_after) =
            if self.cfg.charger_enabled || activation_force_charge || auto_force_charge {
                match bq25792::ensure_ship_fet_path_enabled(&mut self.i2c) {
                    Ok(state) => (
                        (state.ctrl5_before & bq25792::ctrl5::SFET_PRESENT) != 0,
                        (state.ctrl5_after & bq25792::ctrl5::SFET_PRESENT) != 0,
                        state.ship.sdrv_ctrl_before,
                        state.ship.sdrv_ctrl_after,
                    ),
                    Err(e) => {
                        self.mark_charger_poll_failed(now);
                        defmt::error!(
                            "charger: bq25792 err stage=ship_fet_path err={}",
                            i2c_error_kind(e)
                        );
                        return;
                    }
                }
            } else {
                let ctrl5_before = bq25792::read_u8(&mut self.i2c, bq25792::reg::CHARGER_CONTROL_5)
                    .unwrap_or_default();
                let ctrl2_before = bq25792::read_u8(&mut self.i2c, bq25792::reg::CHARGER_CONTROL_2)
                    .unwrap_or_default();
                let sdrv_ctrl_before = (ctrl2_before & bq25792::ctrl2::SDRV_CTRL_MASK)
                    >> bq25792::ctrl2::SDRV_CTRL_SHIFT;
                (
                    (ctrl5_before & bq25792::ctrl5::SFET_PRESENT) != 0,
                    (ctrl5_before & bq25792::ctrl5::SFET_PRESENT) != 0,
                    sdrv_ctrl_before,
                    sdrv_ctrl_before,
                )
            };

        if boot_diag_ship_reset_due {
            self.bms_boot_diag_ship_reset_attempted = true;
            match bq25792::set_sdrv_ctrl_mode(&mut self.i2c, 0b11) {
                Ok(ctrl2_reset) => {
                    spin_delay(BMS_BOOT_DIAG_SHIP_RESET_SETTLE);
                    match bq25792::set_sdrv_ctrl_mode(&mut self.i2c, 0b00) {
                        Ok(ctrl2_idle) => {
                            defmt::info!(
                                "bms_diag: stage=ship_reset_pulse ctrl2_reset=0x{=u8:x} ctrl2_idle=0x{=u8:x}",
                                ctrl2_reset,
                                ctrl2_idle
                            );
                        }
                        Err(e) => {
                            defmt::info!(
                                "bms_diag: stage=ship_reset_idle_restore err={}",
                                i2c_error_kind(e)
                            );
                        }
                    }
                }
                Err(e) => {
                    defmt::info!("bms_diag: stage=ship_reset_pulse err={}", i2c_error_kind(e));
                }
            }
        }

        if let Err(e) = bq25792::read_block(&mut self.i2c, bq25792::reg::CHARGER_STATUS_0, &mut st)
        {
            self.mark_charger_poll_failed(now);
            defmt::error!(
                "charger: bq25792 err stage=status_read err={}",
                i2c_error_kind(e)
            );
            return;
        }
        if let Err(e) = bq25792::read_block(&mut self.i2c, bq25792::reg::FAULT_STATUS_0, &mut fault)
        {
            self.mark_charger_poll_failed(now);
            defmt::error!(
                "charger: bq25792 err stage=fault_read err={}",
                i2c_error_kind(e)
            );
            return;
        }

        let status0 = st[0];
        let status1 = st[1];
        let status2 = st[2];
        let status3 = st[3];
        let status4 = st[4];
        let fault0 = fault[0];
        let fault1 = fault[1];

        let vbus_present = (status0 & bq25792::status0::VBUS_PRESENT_STAT) != 0;
        let ac1_present = (status0 & bq25792::status0::AC1_PRESENT_STAT) != 0;
        let ac2_present = (status0 & bq25792::status0::AC2_PRESENT_STAT) != 0;
        let pg = (status0 & bq25792::status0::PG_STAT) != 0;
        let poorsrc = (status0 & bq25792::status0::POORSRC_STAT) != 0;
        let wd = (status0 & bq25792::status0::WD_STAT) != 0;
        let vindpm = (status0 & bq25792::status0::VINDPM_STAT) != 0;
        let iindpm = (status0 & bq25792::status0::IINDPM_STAT) != 0;

        let vbat_present = (status2 & bq25792::status2::VBAT_PRESENT_STAT) != 0;
        let treg = (status2 & bq25792::status2::TREG_STAT) != 0;
        let dpdm = (status2 & bq25792::status2::DPDM_STAT) != 0;
        let ico_stat = bq25792::status2::ico_stat(status2);

        let ts_cold = (status4 & bq25792::status4::TS_COLD_STAT) != 0;
        let ts_cool = (status4 & bq25792::status4::TS_COOL_STAT) != 0;
        let ts_warm = (status4 & bq25792::status4::TS_WARM_STAT) != 0;
        let ts_hot = (status4 & bq25792::status4::TS_HOT_STAT) != 0;
        let ac_rb2_present = (status3 & bq25792::status3::ACRB2_STAT) != 0;
        let ac_rb1_present = (status3 & bq25792::status3::ACRB1_STAT) != 0;
        let adc_done = (status3 & bq25792::status3::ADC_DONE_STAT) != 0;
        let vsys_min_reg = (status3 & bq25792::status3::VSYS_STAT) != 0;

        let input_present = vbus_present || ac1_present || ac2_present || pg;
        let adc_state = match bq25792::ensure_adc_power_path(&mut self.i2c) {
            Ok(adc_state) => Some(adc_state),
            Err(e) => {
                defmt::info!(
                    "charger: bq25792 adc_cfg err={} action=skip_adc_samples",
                    i2c_error_kind(e)
                );
                None
            }
        };
        let adc_enabled = adc_state
            .map(|adc_state| bq25792::power_path_adc_enabled(adc_state.ctrl))
            .unwrap_or(false);
        let raw_ibus_adc_ma =
            adc_state.and_then(|_| bq25792::read_i16(&mut self.i2c, bq25792::reg::IBUS_ADC).ok());
        let raw_vbus_adc_mv =
            adc_state.and_then(|_| bq25792::read_u16(&mut self.i2c, bq25792::reg::VBUS_ADC).ok());
        let vbat_adc_mv =
            adc_state.and_then(|_| bq25792::read_u16(&mut self.i2c, bq25792::reg::VBAT_ADC).ok());
        let vsys_adc_mv =
            adc_state.and_then(|_| bq25792::read_u16(&mut self.i2c, bq25792::reg::VSYS_ADC).ok());
        let adc_ready = match adc_state {
            Some(adc_state) => bq25792::power_path_adc_ready(adc_state, status3),
            None => false,
        };
        let input_sample = normalize_charger_input_power_sample(
            input_present,
            adc_ready,
            raw_vbus_adc_mv,
            raw_ibus_adc_ma,
        );
        self.maybe_log_charger_input_power_anomaly(
            now,
            input_sample,
            adc_state,
            adc_ready,
            status0,
            status1,
            status3,
        );

        let can_enable = input_present && !ts_cold && !ts_hot;
        let activation_probe_without_charge = activation_pending
            && self.bms_activation_phase == BmsActivationPhase::ProbeWithoutCharge;
        let activation_normal_hold_charge = false;
        let boot_diag_hold_charge = false;
        let normal_allow_charge = can_enable && vbat_present && !activation_probe_without_charge;
        let force_allow_charge = (activation_force_charge || auto_force_charge) && can_enable;
        let allow_charge = if activation_force_charge_off {
            false
        } else {
            (normal_allow_charge && self.cfg.charger_enabled)
                || activation_normal_hold_charge
                || boot_diag_hold_charge
                || force_allow_charge
        };
        let mut applied_ctrl0 = ctrl0;
        let mut applied_vreg_mv: Option<u16> = None;
        let mut applied_ichg_ma: Option<u16> = None;
        let mut applied_iindpm_ma: Option<u16> = None;

        if allow_charge {
            // Ensure we are not braking the converter (ILIM_HIZ < 0.75V forces non-switching).
            self.chg_ilim_hiz_brk.set_low();

            if force_allow_charge {
                if let Err(reason) = self.ensure_bms_activation_charger_backup() {
                    self.mark_charger_poll_failed(now);
                    defmt::error!("charger: bq25792 err stage=backup_capture err={}", reason);
                    return;
                }

                fn decode_voltage_mv(reg: u16) -> u16 {
                    (reg & 0x07FF) * 10
                }

                fn decode_cur_ma(reg: u16) -> u16 {
                    (reg & 0x01FF) * 10
                }

                match bq25792::set_charge_voltage_limit_mv(
                    &mut self.i2c,
                    BMS_ACTIVATION_FORCE_VREG_MV,
                ) {
                    Ok(v) => applied_vreg_mv = Some(decode_voltage_mv(v)),
                    Err(e) => {
                        self.mark_charger_poll_failed(now);
                        defmt::error!(
                            "charger: bq25792 err stage=vreg_write err={}",
                            i2c_error_kind(e)
                        );
                        return;
                    }
                }

                match bq25792::set_charge_current_limit_ma(
                    &mut self.i2c,
                    BMS_ACTIVATION_FORCE_ICHG_MA,
                ) {
                    Ok(v) => applied_ichg_ma = Some(decode_cur_ma(v)),
                    Err(e) => {
                        self.mark_charger_poll_failed(now);
                        defmt::error!(
                            "charger: bq25792 err stage=ichg_write err={}",
                            i2c_error_kind(e)
                        );
                        return;
                    }
                }

                match bq25792::set_input_current_limit_ma(
                    &mut self.i2c,
                    BMS_ACTIVATION_FORCE_IINDPM_MA,
                ) {
                    Ok(v) => applied_iindpm_ma = Some(decode_cur_ma(v)),
                    Err(e) => {
                        self.mark_charger_poll_failed(now);
                        defmt::error!(
                            "charger: bq25792 err stage=iindpm_write err={}",
                            i2c_error_kind(e)
                        );
                        return;
                    }
                }

                match bq25792::read_watchdog_state(&mut self.i2c) {
                    Ok(state) => {
                        if state.watchdog_before != 0 {
                            if let Err(e) = bq25792::kick_watchdog(&mut self.i2c) {
                                defmt::warn!(
                                    "charger: bq25792 warn stage=watchdog_kick err={} watchdog_before=0x{=u8:x}",
                                    i2c_error_kind(e),
                                    state.watchdog_before
                                );
                            }
                        }
                    }
                    Err(e) => {
                        defmt::warn!(
                            "charger: bq25792 warn stage=watchdog_read err={}",
                            i2c_error_kind(e)
                        );
                    }
                }
            }

            // Charge is enabled only when both `EN_CHG=1` and `CE=LOW`.
            let mut desired_ctrl0 = (ctrl0 | bq25792::ctrl0::EN_CHG) & !bq25792::ctrl0::EN_HIZ;
            if force_allow_charge || auto_force_charge || activation_pending {
                desired_ctrl0 &= !bq25792::ctrl0::EN_AUTO_IBATDIS;
            }
            if desired_ctrl0 != ctrl0 {
                match bq25792::write_u8(
                    &mut self.i2c,
                    bq25792::reg::CHARGER_CONTROL_0,
                    desired_ctrl0,
                ) {
                    Ok(()) => applied_ctrl0 = desired_ctrl0,
                    Err(e) => {
                        self.mark_charger_poll_failed(now);
                        defmt::error!(
                            "charger: bq25792 err stage=ctrl0_write err={}",
                            i2c_error_kind(e)
                        );
                        return;
                    }
                }
            }

            self.chg_ce.set_low();
            self.chg_enabled = true;
        } else {
            self.chg_ce.set_high();
            self.chg_enabled = false;
        }

        if !(auto_force_charge || activation_pending) {
            defmt::info!(
                "charger: enabled={=bool} force_min_charge={=bool} auto_boot_force_charge={=bool} boot_diag_hold_charge={=bool} activation_normal_hold_charge={=bool} activation_auto_probe_hold_charge={=bool} activation_force_charge_off={=bool} normal_allow_charge={=bool} force_allow_charge={=bool} allow_charge={=bool} input_present={=bool} vbus_present={=bool} ac1_present={=bool} ac2_present={=bool} pg={=bool} vbat_present={=bool} ibus_adc_ma={=?} vbus_adc_mv={=?} vbat_adc_mv={=?} vsys_adc_mv={=?} adc_enabled={=bool} adc_done={=bool} ac_rb1_present={=bool} ac_rb2_present={=bool} vsys_min_reg={=bool} ts_cold={=bool} ts_cool={=bool} ts_warm={=bool} ts_hot={=bool} vreg_mv={=?} ichg_ma={=?} iindpm_ma={=?} sfet_present_before={=bool} sfet_present_after={=bool} ship_mode_before={=u8} ship_mode_after={=u8} chg_stat={} vbus_stat={} ico={} treg={=bool} dpdm={=bool} wd={=bool} poorsrc={=bool} vindpm={=bool} iindpm={=bool} st0=0x{=u8:x} st1=0x{=u8:x} st2=0x{=u8:x} st3=0x{=u8:x} st4=0x{=u8:x} fault0=0x{=u8:x} fault1=0x{=u8:x} ctrl0=0x{=u8:x}",
                self.chg_enabled,
                self.cfg.force_min_charge,
                auto_force_charge,
                boot_diag_hold_charge,
                activation_normal_hold_charge,
                activation_auto_probe_hold_charge,
                activation_force_charge_off,
                normal_allow_charge,
                force_allow_charge,
                allow_charge,
                input_present,
                vbus_present,
                ac1_present,
                ac2_present,
                pg,
                vbat_present,
                raw_ibus_adc_ma,
                raw_vbus_adc_mv,
                vbat_adc_mv,
                vsys_adc_mv,
                adc_enabled,
                adc_done,
                ac_rb1_present,
                ac_rb2_present,
                vsys_min_reg,
                ts_cold,
                ts_cool,
                ts_warm,
                ts_hot,
                applied_vreg_mv,
                applied_ichg_ma,
                applied_iindpm_ma,
                sfet_present_before,
                sfet_present_after,
                ship_mode_before,
                ship_mode_after,
                bq25792::decode_chg_stat(bq25792::status1::chg_stat(status1)),
                bq25792::decode_vbus_stat(bq25792::status1::vbus_stat(status1)),
                bq25792::decode_ico_stat(ico_stat),
                treg,
                dpdm,
                wd,
                poorsrc,
                vindpm,
                iindpm,
                status0,
                status1,
                status2,
                status3,
                status4,
                fault0,
                fault1,
                applied_ctrl0
            );
        }

        self.charger_audio = ChargerAudioState {
            input_present: Some(input_present),
            phase: audio_charge_phase_from_chg_stat(bq25792::status1::chg_stat(status1)),
            thermal_stress: ts_cool || ts_warm || treg,
            over_voltage: (fault0
                & (CHARGER_FAULT0_VBUS_OVP
                    | CHARGER_FAULT0_VBAT_OVP
                    | CHARGER_FAULT0_VAC1_OVP
                    | CHARGER_FAULT0_VAC2_OVP))
                != 0
                || (fault1 & (CHARGER_FAULT1_VSYS_OVP | CHARGER_FAULT1_OTG_OVP)) != 0,
            over_current: (fault0
                & (CHARGER_FAULT0_IBUS_OCP | CHARGER_FAULT0_IBAT_OCP | CHARGER_FAULT0_CONV_OCP))
                != 0
                || (fault1 & CHARGER_FAULT1_VSYS_SHORT) != 0,
            shutdown_protection: (fault1 & (CHARGER_FAULT1_VSYS_SHORT | CHARGER_FAULT1_TSHUT)) != 0,
            module_fault: false,
        };

        let charger_fault = fault0 != 0 || fault1 != 0 || ts_cold || ts_hot;
        self.ui_snapshot.bq25792 = if charger_fault {
            SelfCheckCommState::Warn
        } else {
            SelfCheckCommState::Ok
        };
        self.ui_snapshot.bq25792_allow_charge = Some(allow_charge);
        self.ui_snapshot.bq25792_vbat_present = Some(vbat_present);
        self.ui_snapshot.input_vbus_mv = input_sample.ui_vbus_mv;
        self.ui_snapshot.input_ibus_ma = input_sample.ui_ibus_ma;
        self.ui_snapshot.bq25792_ichg_ma = if allow_charge {
            if let Some(v) = applied_ichg_ma {
                Some(v)
            } else {
                bq25792::read_u16(&mut self.i2c, bq25792::reg::CHARGE_CURRENT_LIMIT)
                    .ok()
                    .map(|v| (v & 0x01ff) * 10)
            }
        } else {
            None
        };
        self.ui_snapshot.fusb302_vbus_present = Some(input_present);
        self.ui_snapshot.dashboard_detail.input_source =
            detail_input_source(vbus_present, ac1_present, ac2_present);
        self.ui_snapshot.dashboard_detail.charger_active = Some(matches!(
            audio_charge_phase_from_chg_stat(bq25792::status1::chg_stat(status1)),
            AudioChargePhase::Charging
        ));
        self.ui_snapshot.dashboard_detail.charger_status = Some(detail_charger_status_text(
            charger_fault,
            input_present,
            allow_charge,
            bq25792::status1::chg_stat(status1),
        ));
        self.ui_snapshot.dashboard_detail.charger_notice = Some("LIVE DATA");
        self.recompute_ui_mode();

        if auto_force_charge {
            self.bms_activation_auto_force_charge_programmed = true;
            if let Some(until) = self.bms_activation_auto_force_charge_until {
                if BMS_BOOT_DIAG_TOOL_STYLE_PROBE_ONLY && self.bms_activation_auto_attempted {
                    let hold_remaining = if until > now {
                        until - now
                    } else {
                        Duration::ZERO
                    };
                    self.chg_next_poll_at = now + Duration::from_secs(1);
                    defmt::info!(
                        "bms: boot_diag hold_charge keepalive poll_ms={=u32} hold_ms_remaining={=u64}",
                        1_000u32,
                        hold_remaining.as_millis() as u64
                    );
                } else {
                    let prewarm_remaining = if until > now {
                        until - now
                    } else {
                        Duration::ZERO
                    };
                    self.chg_next_poll_at = now + Duration::from_secs(1);
                    defmt::info!(
                        "bms: boot_diag prewarm keepalive poll_ms={=u32} hold_ms_remaining={=u64}",
                        1_000u32,
                        prewarm_remaining.as_millis() as u64
                    );
                }
            }
        } else {
            if self.bms_activation_auto_force_charge_programmed
                && self.bms_activation_state != BmsActivationState::Pending
                && !activation_force_charge
            {
                if let Some(restore_chg_enabled) =
                    self.restore_bms_activation_charger_backup("auto_force_charge_complete")
                {
                    if restore_chg_enabled {
                        self.chg_ce.set_low();
                        self.chg_enabled = true;
                    } else {
                        self.chg_ce.set_high();
                        self.chg_enabled = false;
                    }
                }
            }
            self.bms_activation_auto_force_charge_programmed = false;
        }

        self.chg_next_retry_at = None;
    }

    fn mark_charger_poll_failed(&mut self, now: Instant) {
        if self.bms_activation_state == BmsActivationState::Pending {
            self.finish_bms_activation(BmsResultKind::NotDetected, "charger_poll_failed");
        }
        self.chg_ce.set_high();
        self.chg_enabled = false;
        self.chg_next_retry_at = Some(now + self.cfg.retry_backoff);
        self.charger_audio = ChargerAudioState {
            input_present: None,
            phase: AudioChargePhase::Unknown,
            thermal_stress: false,
            over_voltage: false,
            over_current: false,
            shutdown_protection: false,
            module_fault: true,
        };
        self.ui_snapshot.bq25792 = SelfCheckCommState::Err;
        self.ui_snapshot.bq25792_allow_charge = Some(false);
        self.ui_snapshot.bq25792_ichg_ma = None;
        self.ui_snapshot.bq25792_vbat_present = None;
        self.ui_snapshot.fusb302_vbus_present = None;
        self.ui_snapshot.input_vbus_mv = None;
        self.ui_snapshot.input_ibus_ma = None;
        self.clear_charger_detail_snapshot();
        self.recompute_ui_mode();
    }

    fn maybe_poll_bms(&mut self, irq: &IrqSnapshot) -> bool {
        const POLL_PERIOD: Duration = Duration::from_secs(2);
        const INT_MIN_INTERVAL: Duration = Duration::from_millis(100);

        let now = Instant::now();
        let auto_quiet_until =
            if self.bms_activation_auto_poll_release_at > self.bms_activation_auto_due_at {
                self.bms_activation_auto_poll_release_at
            } else {
                self.bms_activation_auto_due_at
            };
        if self.cfg.bms_boot_diag_auto_validate
            && !self.bms_activation_auto_attempted
            && now < auto_quiet_until
        {
            self.bms_next_poll_at = auto_quiet_until;
            self.bms_next_retry_at = None;
            if !self.bms_activation_auto_defer_logged {
                self.bms_activation_auto_defer_logged = true;
                defmt::info!(
                    "bms: boot_diag defer_poll until_auto_request settle_ms={=u64}",
                    (auto_quiet_until - now).as_millis() as u64
                );
            }
            return false;
        }
        let mut due = now >= self.bms_next_poll_at;
        if irq.bms_btp_int_h != 0 {
            let allow = self
                .bms_last_int_poll_at
                .map_or(true, |t| now >= t + INT_MIN_INTERVAL);
            if allow {
                due = true;
                self.bms_last_int_poll_at = Some(now);
            }
        }
        if !due {
            return false;
        }
        if let Some(next_retry_at) = self.bms_next_retry_at {
            if now < next_retry_at {
                return false;
            }
        }
        self.bms_next_poll_at = now + POLL_PERIOD;
        self.bms_poll_seq = self.bms_poll_seq.wrapping_add(1);
        let poll_seq = self.bms_poll_seq;
        let auto_observation_active = !self.bms_activation_auto_attempted && now < auto_quiet_until;
        let boot_diag_probe_hold_active = BMS_BOOT_DIAG_TOOL_STYLE_PROBE_ONLY
            && self.bms_activation_auto_attempted
            && self
                .bms_activation_auto_force_charge_until
                .map_or(false, |until| now < until);
        let suppress_retry_backoff = auto_observation_active
            || self.bms_activation_state == BmsActivationState::Pending
            || boot_diag_probe_hold_active;

        let btp_int_h = self.bms_btp_int_h.is_high() || irq.bms_btp_int_h != 0;

        #[cfg(feature = "bms-dual-probe-diag")]
        let (addr_order, addr_count): ([u8; 2], usize) = match self.bms_addr {
            Some(a) if a == bq40z50::I2C_ADDRESS_FALLBACK => (
                [bq40z50::I2C_ADDRESS_FALLBACK, bq40z50::I2C_ADDRESS_PRIMARY],
                2,
            ),
            Some(a) if a == bq40z50::I2C_ADDRESS_PRIMARY => (
                [bq40z50::I2C_ADDRESS_PRIMARY, bq40z50::I2C_ADDRESS_FALLBACK],
                2,
            ),
            _ => (bq40z50::I2C_ADDRESS_CANDIDATES, 2),
        };

        #[cfg(not(feature = "bms-dual-probe-diag"))]
        let (addr_order, addr_count): ([u8; 2], usize) = (
            [bq40z50::I2C_ADDRESS_PRIMARY, bq40z50::I2C_ADDRESS_PRIMARY],
            1,
        );

        for (idx, addr) in addr_order.into_iter().take(addr_count).enumerate() {
            match self.read_bq40z50_snapshot_strict(addr) {
                Ok(s) => {
                    self.bms_addr = Some(addr);
                    self.bms_runtime_seen = true;
                    self.bms_next_retry_at = None;
                    self.bms_ok_streak = self.bms_ok_streak.saturating_add(1);
                    self.bms_err_streak = 0;
                    let rca_alarm = (s.batt_status & bq40z50::battery_status::RCA) != 0;
                    let low_pack = bq40_pack_indicates_no_battery(s.vpack_mv);
                    let discharge_ready = Self::bq40_discharge_ready(s.op_status);
                    self.ui_snapshot.bq40z50 =
                        if low_pack || rca_alarm || !matches!(discharge_ready, Some(true)) {
                            SelfCheckCommState::Warn
                        } else {
                            SelfCheckCommState::Ok
                        };
                    self.ui_snapshot.bq40z50_pack_mv = Some(s.vpack_mv);
                    self.ui_snapshot.bq40z50_current_ma = Some(s.current_ma);
                    self.ui_snapshot.bq40z50_soc_pct = Some(s.rsoc_pct);
                    self.ui_snapshot.bq40z50_rca_alarm = Some(rca_alarm);
                    self.ui_snapshot.bq40z50_no_battery = Some(low_pack);
                    self.ui_snapshot.bq40z50_discharge_ready = discharge_ready;
                    self.apply_bms_detail_snapshot(&s);
                    let protection_active = bq40_op_bit(s.op_status, bq40z50::operation_status::PF)
                        == Some(true)
                        || bq40z50::battery_status::error_code(s.batt_status) != 0
                        || (s.batt_status
                            & (bq40z50::battery_status::OCA
                                | bq40z50::battery_status::TCA
                                | bq40z50::battery_status::OTA
                                | bq40z50::battery_status::TDA))
                            != 0;
                    self.bms_audio = BmsAudioState {
                        rca_alarm: Some(rca_alarm),
                        protection_active,
                        module_fault: false,
                    };
                    self.log_bq40z50_snapshot(addr, poll_seq, self.bms_ok_streak, btp_int_h, &s);
                    return true;
                }
                Err(Bq40SnapshotReadError::Invalid(s)) => {
                    if idx + 1 == addr_count {
                        self.bms_addr = None;
                        self.bms_ok_streak = 0;
                        self.bms_err_streak = self.bms_err_streak.saturating_add(1);
                        self.bms_next_retry_at = if suppress_retry_backoff {
                            None
                        } else {
                            Some(now + self.cfg.retry_backoff)
                        };
                        if self.should_hold_no_battery_result() {
                            self.hold_no_battery_result_audio_state();
                        } else {
                            self.reset_bms_detail_mac_cache(now);
                            self.ui_snapshot.bq40z50 = SelfCheckCommState::Warn;
                            self.ui_snapshot.bq40z50_pack_mv = None;
                            self.ui_snapshot.bq40z50_current_ma = None;
                            self.ui_snapshot.bq40z50_soc_pct = None;
                            self.ui_snapshot.bq40z50_rca_alarm = None;
                            self.ui_snapshot.bq40z50_no_battery = None;
                            self.ui_snapshot.bq40z50_discharge_ready = None;
                            self.clear_bms_detail_snapshot();
                            self.bms_audio = BmsAudioState {
                                rca_alarm: None,
                                protection_active: false,
                                module_fault: true,
                            };
                        }
                        let temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(s.temp_k_x10);
                        let flow = bq40_decode_current_flow(s.current_ma);
                        let (charge_ready, charge_reason) = bq40_decode_charge_path(s.op_status);
                        let (discharge_ready, discharge_reason) =
                            bq40_decode_discharge_path(s.op_status);
                        let primary_reason = bq40_primary_reason(
                            s.batt_status,
                            s.op_status,
                            charge_reason,
                            discharge_reason,
                        );
                        defmt::warn!(
                            "bms: bq40z50 invalid addrs={} poll_seq={=u32} err_streak={=u16} temp_c_x10={=i32} vpack_mv={=u16} rsoc_pct={=u16} flow={} chg_ready={=?} dsg_ready={=?} chg_reason={} dsg_reason={} primary_reason={}",
                            BMS_ADDR_LOG,
                            poll_seq,
                            self.bms_err_streak,
                            temp_c_x10,
                            s.vpack_mv,
                            s.rsoc_pct,
                            flow,
                            charge_ready,
                            discharge_ready,
                            charge_reason,
                            discharge_reason,
                            primary_reason
                        );
                    }
                }
                Err(Bq40SnapshotReadError::I2c(kind)) => {
                    // Only log one line after the final address attempt.
                    if idx + 1 == addr_count {
                        self.bms_addr = None;
                        self.bms_ok_streak = 0;
                        self.bms_err_streak = self.bms_err_streak.saturating_add(1);
                        self.bms_next_retry_at = if suppress_retry_backoff {
                            None
                        } else {
                            Some(now + self.cfg.retry_backoff)
                        };
                        if self.should_hold_no_battery_result() {
                            self.hold_no_battery_result_audio_state();
                        } else {
                            self.reset_bms_detail_mac_cache(now);
                            self.ui_snapshot.bq40z50 = SelfCheckCommState::Err;
                            self.ui_snapshot.bq40z50_pack_mv = None;
                            self.ui_snapshot.bq40z50_current_ma = None;
                            self.ui_snapshot.bq40z50_soc_pct = None;
                            self.ui_snapshot.bq40z50_rca_alarm = None;
                            self.ui_snapshot.bq40z50_no_battery = None;
                            self.ui_snapshot.bq40z50_discharge_ready = None;
                            self.clear_bms_detail_snapshot();
                            self.bms_audio = BmsAudioState {
                                rca_alarm: None,
                                protection_active: false,
                                module_fault: true,
                            };
                        }

                        if kind == "i2c_nack" || kind == "i2c_timeout" {
                            defmt::warn!(
                                "bms: bq40z50 absent addrs={} poll_seq={=u32} err_streak={=u16} err={} btp_int_h={=bool}",
                                BMS_ADDR_LOG,
                                poll_seq,
                                self.bms_err_streak,
                                kind,
                                btp_int_h
                            );
                        } else {
                            defmt::error!(
                                "bms: bq40z50 err addrs={} poll_seq={=u32} err_streak={=u16} err={} btp_int_h={=bool}",
                                BMS_ADDR_LOG,
                                poll_seq,
                                self.bms_err_streak,
                                kind,
                                btp_int_h
                            );
                        }
                    }
                }
            }
        }
        true
    }

    fn is_bq40_snapshot_reasonable(s: &Bq40z50Snapshot) -> bool {
        let temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(s.temp_k_x10);
        (-400..=1250).contains(&temp_c_x10)
            && (2_500..=20_000).contains(&s.vpack_mv)
            && s.rsoc_pct <= 100
    }

    fn is_bq40_low_pack_snapshot_candidate(s: &Bq40z50Snapshot) -> bool {
        let temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(s.temp_k_x10);
        (-400..=1250).contains(&temp_c_x10)
            && bq40_pack_indicates_no_battery(s.vpack_mv)
            && s.rsoc_pct <= 100
    }

    fn bq40_low_pack_snapshots_match(a: &Bq40z50Snapshot, b: &Bq40z50Snapshot) -> bool {
        a.vpack_mv == b.vpack_mv
            && a.current_ma == b.current_ma
            && a.rsoc_pct == b.rsoc_pct
            && a.batt_status == b.batt_status
            && a.cell_mv == b.cell_mv
    }

    fn bq40_discharge_ready(op_status: Option<u32>) -> Option<bool> {
        bq40_decode_discharge_path(op_status).0
    }

    fn read_bq40z50_snapshot_strict(
        &mut self,
        addr: u8,
    ) -> Result<Bq40z50Snapshot, Bq40SnapshotReadError> {
        const MAX_FULL_SNAPSHOT_ATTEMPTS: usize = 2;
        let mut last_i2c_kind: Option<&'static str> = None;
        let mut last_invalid: Option<Bq40z50Snapshot> = None;
        let mut low_pack_candidate: Option<Bq40z50Snapshot> = None;

        for _ in 0..MAX_FULL_SNAPSHOT_ATTEMPTS {
            match self.read_bq40z50_snapshot_retry(addr) {
                Ok(snapshot) => {
                    if Self::is_bq40_snapshot_reasonable(&snapshot) {
                        return Ok(snapshot);
                    }
                    if Self::is_bq40_low_pack_snapshot_candidate(&snapshot) {
                        if let Some(previous) = low_pack_candidate {
                            if Self::bq40_low_pack_snapshots_match(&previous, &snapshot) {
                                return Ok(snapshot);
                            }
                        }
                        low_pack_candidate = Some(snapshot);
                    }
                    last_invalid = Some(snapshot);
                }
                Err(e) => {
                    last_i2c_kind = Some(bq40_activation_read_error_kind(e));
                }
            }
        }

        if let Some(snapshot) = last_invalid {
            return Err(Bq40SnapshotReadError::Invalid(snapshot));
        }

        Err(Bq40SnapshotReadError::I2c(last_i2c_kind.unwrap_or("i2c")))
    }

    fn read_bq40z50_snapshot_retry(
        &mut self,
        addr: u8,
    ) -> Result<Bq40z50Snapshot, Bq40ActivationReadError> {
        const MAX_ATTEMPTS: usize = 3;

        for attempt in 0..MAX_ATTEMPTS {
            match self.read_bq40z50_snapshot(addr) {
                Ok(snapshot) => return Ok(snapshot),
                Err(e) => {
                    let retryable = matches!(
                        e,
                        Bq40ActivationReadError::I2c("i2c_timeout")
                            | Bq40ActivationReadError::I2c("i2c_nack")
                    );
                    if retryable && attempt + 1 < MAX_ATTEMPTS {
                        continue;
                    }
                    return Err(e);
                }
            }
        }

        unreachable!()
    }

    fn read_bq40z50_snapshot(
        &mut self,
        addr: u8,
    ) -> Result<Bq40z50Snapshot, Bq40ActivationReadError> {
        let now = Instant::now();
        let battery_mode =
            self.read_bq40_u16_with_optional_pec(addr, bq40z50::cmd::BATTERY_MODE)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let mut temp_k_x10 =
            self.read_bq40_u16_with_optional_pec(addr, bq40z50::cmd::TEMPERATURE)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let vpack_mv = self.read_bq40_u16_with_optional_pec(addr, bq40z50::cmd::VOLTAGE)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let current_ma = self.read_bq40_i16_with_optional_pec(addr, bq40z50::cmd::CURRENT)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let rsoc_pct =
            self.read_bq40_u16_with_optional_pec(addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let remcap =
            self.read_bq40_u16_with_optional_pec(addr, bq40z50::cmd::REMAINING_CAPACITY)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let fcc = self.read_bq40_u16_with_optional_pec(addr, bq40z50::cmd::FULL_CHARGE_CAPACITY)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let batt_status =
            self.read_bq40_u16_with_optional_pec(addr, bq40z50::cmd::BATTERY_STATUS)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let cell1_mv = self.read_bq40_u16_with_optional_pec(addr, bq40z50::cmd::CELL_VOLTAGE_1)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let cell2_mv = self.read_bq40_u16_with_optional_pec(addr, bq40z50::cmd::CELL_VOLTAGE_2)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let cell3_mv = self.read_bq40_u16_with_optional_pec(addr, bq40z50::cmd::CELL_VOLTAGE_3)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let cell4_mv = self.read_bq40_u16_with_optional_pec(addr, bq40z50::cmd::CELL_VOLTAGE_4)?;

        let temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(temp_k_x10);
        if !(-400..=1250).contains(&temp_c_x10) {
            spin_delay(BMS_ACTIVATION_WORD_GAP);
            if let Ok(retry_temp_k_x10) =
                self.read_bq40_u16_with_optional_pec(addr, bq40z50::cmd::TEMPERATURE)
            {
                let retry_temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(retry_temp_k_x10);
                if (-400..=1250).contains(&retry_temp_c_x10) {
                    temp_k_x10 = retry_temp_k_x10;
                }
            }
        }

        let op_status = bq40z50::read_operation_status(&mut self.i2c, addr)
            .ok()
            .flatten();
        let mut da_status2 = self.bms_cached_da_status2;
        if now >= self.bms_next_da_status2_refresh_at {
            spin_delay(BMS_ACTIVATION_WORD_GAP);
            if let Ok(snapshot) = bq40z50::read_da_status2(&mut self.i2c, addr) {
                da_status2 = snapshot;
                self.bms_cached_da_status2 = snapshot;
            }
            self.bms_next_da_status2_refresh_at = now + BMS_DETAIL_MAC_REFRESH_PERIOD;
        }
        let mut filter_capacity = self.bms_cached_filter_capacity;
        if now >= self.bms_next_filter_capacity_refresh_at {
            spin_delay(BMS_ACTIVATION_WORD_GAP);
            if let Ok(snapshot) = bq40z50::read_filter_capacity(&mut self.i2c, addr) {
                filter_capacity = snapshot;
                self.bms_cached_filter_capacity = snapshot;
            }
            self.bms_next_filter_capacity_refresh_at = now + BMS_DETAIL_MAC_REFRESH_PERIOD;
        }
        Ok(Bq40z50Snapshot {
            battery_mode,
            temp_k_x10,
            vpack_mv,
            current_ma,
            rsoc_pct,
            remcap,
            fcc,
            batt_status,
            op_status,
            da_status2,
            filter_capacity,
            cell_mv: [cell1_mv, cell2_mv, cell3_mv, cell4_mv],
        })
    }

    fn log_bq40z50_snapshot(
        &self,
        addr: u8,
        poll_seq: u32,
        ok_streak: u16,
        btp_int_h: bool,
        s: &Bq40z50Snapshot,
    ) {
        let temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(s.temp_k_x10);

        let bs = s.batt_status;
        let init = (bs & bq40z50::battery_status::INIT) != 0;
        let dsg = (bs & bq40z50::battery_status::DSG) != 0;
        let fc = (bs & bq40z50::battery_status::FC) != 0;
        let fd = (bs & bq40z50::battery_status::FD) != 0;

        let oca = (bs & bq40z50::battery_status::OCA) != 0;
        let tca = (bs & bq40z50::battery_status::TCA) != 0;
        let ota = (bs & bq40z50::battery_status::OTA) != 0;
        let tda = (bs & bq40z50::battery_status::TDA) != 0;
        let rca = (bs & bq40z50::battery_status::RCA) != 0;
        let rta = (bs & bq40z50::battery_status::RTA) != 0;
        let xchg = bq40_op_bit(s.op_status, bq40z50::operation_status::XCHG);
        let xdsg = bq40_op_bit(s.op_status, bq40z50::operation_status::XDSG);
        let chg_fet = bq40_op_bit(s.op_status, bq40z50::operation_status::CHG);
        let dsg_fet = bq40_op_bit(s.op_status, bq40z50::operation_status::DSG);
        let (chg_ready, chg_reason) = bq40_decode_charge_path(s.op_status);
        let (dsg_ready, dsg_reason) = bq40_decode_discharge_path(s.op_status);
        let pres = bq40_op_bit(s.op_status, bq40z50::operation_status::PRES);
        let sleep = bq40_op_bit(s.op_status, bq40z50::operation_status::SLEEP);
        let pf = bq40_op_bit(s.op_status, bq40z50::operation_status::PF);
        let flow = bq40_decode_current_flow(s.current_ma);
        let flow_abs_ma = s.current_ma.wrapping_abs() as u16;
        let pack_power_mw = (s.vpack_mv as i32 * s.current_ma as i32) / 1000;
        let primary_reason = bq40_primary_reason(bs, s.op_status, chg_reason, dsg_reason);
        let no_battery = bq40_pack_indicates_no_battery(s.vpack_mv);
        let (cell_min_mv, cell_max_mv, cell_delta_mv) = bq40_cell_min_max_delta(&s.cell_mv);
        let op_status_read_ok = s.op_status.is_some();

        let ec = bq40z50::battery_status::error_code(bs);

        defmt::info!(
            "bms: bq40z50 addr=0x{=u8:x} poll_seq={=u32} ok_streak={=u16} btp_int_h={=bool} temp_c_x10={=i32} vpack_mv={=u16} no_battery={=bool} current_ma={=i16} flow={} flow_abs_ma={=u16} pack_power_mw={=i32} rsoc_pct={=u16} remcap={=u16} fcc={=u16} batt_status=0x{=u16:x} op_status={=?} op_status_read_ok={=bool} init={=bool} dsg={=bool} fc={=bool} fd={=bool} xchg={=?} xdsg={=?} chg_fet={=?} dsg_fet={=?} chg_ready={=?} dsg_ready={=?} chg_reason={} dsg_reason={} primary_reason={} pres={=?} sleep={=?} pf={=?} oca={=bool} tca={=bool} ota={=bool} tda={=bool} rca={=bool} rta={=bool} ec=0x{=u8:x} ec_str={} cell_min_mv={=u16} cell_max_mv={=u16} cell_delta_mv={=u16} c1_mv={=u16} c2_mv={=u16} c3_mv={=u16} c4_mv={=u16}",
            addr,
            poll_seq,
            ok_streak,
            btp_int_h,
            temp_c_x10,
            s.vpack_mv,
            no_battery,
            s.current_ma,
            flow,
            flow_abs_ma,
            pack_power_mw,
            s.rsoc_pct,
            s.remcap,
            s.fcc,
            bs,
            s.op_status,
            op_status_read_ok,
            init,
            dsg,
            fc,
            fd,
            xchg,
            xdsg,
            chg_fet,
            dsg_fet,
            chg_ready,
            dsg_ready,
            chg_reason,
            dsg_reason,
            primary_reason,
            pres,
            sleep,
            pf,
            oca,
            tca,
            ota,
            tda,
            rca,
            rta,
            ec,
            bq40z50::decode_error_code(ec),
            cell_min_mv,
            cell_max_mv,
            cell_delta_mv,
            s.cell_mv[0],
            s.cell_mv[1],
            s.cell_mv[2],
            s.cell_mv[3],
        );
    }

    fn log_therm_kill_hint(&mut self) {
        const TMP112_OUT_A_ADDR: u8 = 0x48;
        const TMP112_OUT_B_ADDR: u8 = 0x49;

        let a = tmp112::read_temp_c_x16(&mut self.i2c, TMP112_OUT_A_ADDR);
        let b = tmp112::read_temp_c_x16(&mut self.i2c, TMP112_OUT_B_ADDR);

        let a_active = matches!(&a, Ok(t) if *t >= self.cfg.tmp112_tlow_c_x16);
        let b_active = matches!(&b, Ok(t) if *t >= self.cfg.tmp112_tlow_c_x16);

        let hint = if a_active && b_active {
            "both"
        } else if a_active {
            "out_a"
        } else if b_active {
            "out_b"
        } else {
            "unknown"
        };

        defmt::warn!(
            "power: therm_kill_n asserted hint={} tlow_c_x16={=i16} thigh_c_x16={=i16} out_a_temp_c_x16={=?} out_b_temp_c_x16={=?}",
            hint,
            self.cfg.tmp112_tlow_c_x16,
            self.cfg.tmp112_thigh_c_x16,
            a.map_err(i2c_error_kind),
            b.map_err(i2c_error_kind),
        );
    }
}

#[derive(Clone, Copy)]
struct Bq40z50Snapshot {
    battery_mode: u16,
    temp_k_x10: u16,
    vpack_mv: u16,
    current_ma: i16,
    rsoc_pct: u16,
    remcap: u16,
    fcc: u16,
    batt_status: u16,
    op_status: Option<u32>,
    da_status2: Option<bq40z50::DaStatus2>,
    filter_capacity: Option<bq40z50::FilterCapacity>,
    cell_mv: [u16; 4],
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Bq40ActivationSignature {
    vpack_mv: u16,
    current_ma: i16,
    rsoc_pct: u16,
    batt_status: u16,
}

#[derive(Clone, Copy)]
struct Bq40ActivationPatternTracker {
    last_signature: Option<Bq40ActivationSignature>,
    repeat_count: u8,
}

impl Bq40ActivationPatternTracker {
    const fn new() -> Self {
        Self {
            last_signature: None,
            repeat_count: 0,
        }
    }

    fn observe(&mut self, signature: Bq40ActivationSignature) -> u8 {
        if self.last_signature == Some(signature) {
            self.repeat_count = self.repeat_count.saturating_add(1);
        } else {
            self.last_signature = Some(signature);
            self.repeat_count = 1;
        }
        self.repeat_count
    }
}

fn observe_bq40_activation_signature(
    tracker: &mut Bq40ActivationPatternTracker,
    vpack_mv: u16,
    current_ma: i16,
    rsoc_pct: u16,
    batt_status: u16,
) -> u8 {
    tracker.observe(Bq40ActivationSignature {
        vpack_mv,
        current_ma,
        rsoc_pct,
        batt_status,
    })
}

fn bq40_activation_signature_is_stale(
    vpack_mv: u16,
    current_ma: i16,
    batt_status: u16,
    repeat_count: u8,
) -> bool {
    vpack_mv == BMS_SUSPICIOUS_VOLTAGE_MV
        && current_ma == BMS_SUSPICIOUS_CURRENT_MA
        && batt_status == BMS_SUSPICIOUS_STATUS
        && repeat_count >= 3
}

#[derive(Clone, Copy)]
enum Bq40ActivationProbeResult {
    Pending,
    Rom,
    Working { addr: u8, snapshot: Bq40z50Snapshot },
}

#[derive(Clone, Copy)]
struct Bq40ActivationBlockReadRaw {
    declared_len: u8,
    payload_len: u8,
    payload: [u8; 32],
}

#[derive(Clone, Copy)]
enum Bq40ActivationReadError {
    I2c(&'static str),
    BadRange,
    StalePattern,
    InconsistentSample,
}

fn bq40_activation_read_error_kind(err: Bq40ActivationReadError) -> &'static str {
    match err {
        Bq40ActivationReadError::I2c(kind) => kind,
        Bq40ActivationReadError::BadRange => "bad_range",
        Bq40ActivationReadError::StalePattern => "stale_pattern",
        Bq40ActivationReadError::InconsistentSample => "inconsistent_sample",
    }
}

enum Bq40SnapshotReadError {
    I2c(&'static str),
    Invalid(Bq40z50Snapshot),
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
    fn detail_charger_status_maps_runtime_states_to_short_tokens() {
        assert_eq!(detail_charger_status_text(true, true, true, 3), "FAULT");
        assert_eq!(detail_charger_status_text(false, false, true, 3), "NOAC");
        assert_eq!(detail_charger_status_text(false, true, false, 3), "LOCK");
        assert_eq!(detail_charger_status_text(false, true, true, 3), "CHG");
        assert_eq!(detail_charger_status_text(false, true, true, 7), "DONE");
        assert_eq!(detail_charger_status_text(false, true, true, 0), "READY");
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
            cell_mv: [4184, 4188, 4149, 4157],
        };

        assert_eq!(detail_bms_energy_mwh(&snapshot), Some(65_505));
        assert_eq!(detail_bms_full_capacity_mwh(&snapshot), Some(65_505));
    }

    #[test]
    fn fan_rpm_tracker_uses_two_pulses_per_rev() {
        let mut tracker = FanRpmTracker::new();
        let cfg = fan::Config {
            low_temp_c_x16: 40 * 16,
            high_temp_c_x16: 50 * 16,
            hysteresis_c_x16: 3 * 16,
            cooldown_ms: 10_000,
            tach_timeout_ms: 2_000,
            tach_pulses_per_rev: 2,
            mid_pwm_pct: 60,
        };
        let status = fan::Status {
            requested_command: fan::FanLevel::High,
            command: fan::FanLevel::High,
            thermal_level: fan::FanLevel::High,
            temp_source: fan::TempSource::Max,
            control_temp_c_x16: Some(55 * 16),
            tach_fault: false,
            tach_pulse_seen_recently: true,
            cooldown_active: false,
        };

        assert_eq!(tracker.observe(0, 0, status, cfg), None);
        assert_eq!(tracker.observe(500, 40, status, cfg), Some(2400));
    }

    #[test]
    fn fan_rpm_tracker_clears_when_fan_turns_off() {
        let mut tracker = FanRpmTracker::new();
        let cfg = fan::Config {
            low_temp_c_x16: 40 * 16,
            high_temp_c_x16: 50 * 16,
            hysteresis_c_x16: 3 * 16,
            cooldown_ms: 10_000,
            tach_timeout_ms: 2_000,
            tach_pulses_per_rev: 2,
            mid_pwm_pct: 60,
        };
        let running = fan::Status {
            requested_command: fan::FanLevel::Mid,
            command: fan::FanLevel::Mid,
            thermal_level: fan::FanLevel::Mid,
            temp_source: fan::TempSource::Max,
            control_temp_c_x16: Some(45 * 16),
            tach_fault: false,
            tach_pulse_seen_recently: true,
            cooldown_active: false,
        };
        let off = fan::Status {
            requested_command: fan::FanLevel::Off,
            command: fan::FanLevel::Off,
            thermal_level: fan::FanLevel::Off,
            temp_source: fan::TempSource::Max,
            control_temp_c_x16: Some(35 * 16),
            tach_fault: false,
            tach_pulse_seen_recently: false,
            cooldown_active: false,
        };

        assert_eq!(tracker.observe(0, 0, running, cfg), None);
        assert_eq!(tracker.observe(500, 20, running, cfg), Some(1200));
        assert_eq!(tracker.observe(800, 0, off, cfg), None);
        assert_eq!(tracker.rpm, None);
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
