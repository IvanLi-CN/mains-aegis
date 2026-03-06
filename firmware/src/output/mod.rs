pub mod tps55288;

use esp_firmware::bq25792;
use esp_firmware::bq40z50;
use esp_firmware::ina3221;
use esp_firmware::tmp112;
use esp_hal::gpio::{Flex, Input};
use esp_hal::time::{Duration, Instant};

use crate::front_panel_scene::{
    is_bq40_activation_needed, BmsActivationState, SelfCheckCommState, SelfCheckUiSnapshot, UpsMode,
};
use crate::irq::IrqSnapshot;

use ::tps55288::Error as TpsError;

pub use self::tps55288::OutputChannel;

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

const BMS_ACTIVATION_WINDOW: Duration = Duration::from_secs(15);
const BMS_ACTIVATION_FORCE_ICHG_MA: u16 = 50;
const BMS_ACTIVATION_FORCE_IINDPM_MA: u16 = 100;
const BQ40_CURRENT_IDLE_THRESHOLD_MA: i16 = 20;

fn bq40_op_bit(op_status: Option<u16>, mask: u16) -> Option<bool> {
    op_status.map(|raw| (raw & mask) != 0)
}

fn bq40_decode_charge_path(op_status: Option<u16>) -> (Option<bool>, &'static str) {
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

fn bq40_decode_discharge_path(op_status: Option<u16>) -> (Option<bool>, &'static str) {
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

fn bq40_primary_reason(
    batt_status: u16,
    op_status: Option<u16>,
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

#[derive(Clone, Copy, PartialEq, Eq)]
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

pub(super) fn tps_error_kind(err: TpsError<esp_hal::i2c::master::Error>) -> &'static str {
    match err {
        TpsError::I2c(e) => i2c_error_kind(e),
        TpsError::OutOfRange => "out_of_range",
        TpsError::InvalidConfig => "invalid_config",
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
    pub enabled_outputs: EnabledOutputs,
    pub outputs_restore_on_bms_ready: EnabledOutputs,
    pub outputs_blocked_by_bms: bool,
    pub charger_probe_ok: bool,
    pub charger_enabled: bool,
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

fn ups_mode_from_vbus(vbus_present: Option<bool>, has_output: bool) -> UpsMode {
    match vbus_present {
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
    mut reporter: F,
) -> BootSelfTestResult
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
    F: FnMut(SelfCheckStage, SelfCheckUiSnapshot),
{
    defmt::info!(
        "self_test: begin vout_mv={=u16} ilimit_ma={=u16} tmp_a_ok={=bool} tmp_b_ok={=bool} sync_ok={=bool} screen_present={=bool} therm_kill_asserted={=bool} force_min_charge={=bool}",
        vout_mv,
        ilimit_ma,
        tmp_out_a_ok,
        tmp_out_b_ok,
        sync_ok,
        panel_probe.screen_present(),
        therm_kill_asserted,
        force_min_charge
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
    ui.tmp_a_c = tmp_a_read.ok().map(|v| v / 16);
    ui.tmp_b_c = tmp_b_read.ok().map(|v| v / 16);
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
    let mut bms_soc_pct: Option<u16> = None;
    let mut bms_rca_alarm: Option<bool> = None;
    let mut bms_discharge_ready: Option<bool> = None;
    let mut bms_discharge_reason: Option<&'static str> = None;
    let mut bms_charge_ready: Option<bool> = None;
    let mut bms_charge_reason: Option<&'static str> = None;
    let mut bms_flow: Option<&'static str> = None;
    let mut bms_primary_reason: Option<&'static str> = None;
    for addr in bms_probe_candidates().iter().copied() {
        let temp = bq40z50::read_u16(&mut *i2c, addr, bq40z50::cmd::TEMPERATURE);
        let voltage = bq40z50::read_u16(&mut *i2c, addr, bq40z50::cmd::VOLTAGE);
        let current = bq40z50::read_i16(&mut *i2c, addr, bq40z50::cmd::CURRENT);
        let soc = bq40z50::read_u16(&mut *i2c, addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE);
        let status = bq40z50::read_u16(&mut *i2c, addr, bq40z50::cmd::BATTERY_STATUS);
        let op_status = bq40z50::read_u16(&mut *i2c, addr, bq40z50::cmd::OPERATION_STATUS).ok();

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
            bms_soc_pct = Some(soc_pct);
            bms_rca_alarm = Some((status_raw & bq40z50::battery_status::RCA) != 0);
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
    ui.bq40z50_soc_pct = bms_soc_pct;
    ui.bq40z50_rca_alarm = bms_rca_alarm;
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
    if charger_enabled {
        charger_status0 = bq25792::read_u8(&mut *i2c, bq25792::reg::CHARGER_STATUS_0).ok();
        let charger_status2 = bq25792::read_u8(&mut *i2c, bq25792::reg::CHARGER_STATUS_2).ok();
        let charger_status3 = bq25792::read_u8(&mut *i2c, bq25792::reg::CHARGER_STATUS_3).ok();
        let charger_vbat_adc_mv = bq25792::read_u16(&mut *i2c, bq25792::reg::VBAT_ADC).ok();
        let charger_vsys_adc_mv = bq25792::read_u16(&mut *i2c, bq25792::reg::VSYS_ADC).ok();

        let vbat_present = charger_status2.map(|v| (v & bq25792::status2::VBAT_PRESENT_STAT) != 0);
        charger_vbat_present = vbat_present;
        let vsys_min_reg = charger_status3.map(|v| (v & bq25792::status3::VSYS_STAT) != 0);
        defmt::info!(
            "self_test: bq25792 ctrl0={=?} status0={=?} status2={=?} status3={=?} vbat_present={=?} vsys_min_reg={=?} vbat_adc_mv={=?} vsys_adc_mv={=?}",
            charger_ctrl0,
            charger_status0,
            charger_status2,
            charger_status3,
            vbat_present,
            vsys_min_reg,
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
    let mut outputs_restore_on_bms_ready = enabled_outputs_from_flags(out_a_allowed, out_b_allowed);
    let mut outputs_blocked_by_bms = false;

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
        outputs_blocked_by_bms = outputs_restore_on_bms_ready != EnabledOutputs::None;
        out_a_allowed = false;
        out_b_allowed = false;

        if bms_addr.is_none() {
            if force_min_charge {
                defmt::warn!(
                    "self_test: bq40z50 missing; keep charger module for force_min_charge (charger_probe_ok={=bool})",
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
        outputs_restore_on_bms_ready = EnabledOutputs::None;
        outputs_blocked_by_bms = false;
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

    ui.mode = ups_mode_from_vbus(ui.fusb302_vbus_present, out_a_allowed || out_b_allowed);

    defmt::info!(
        "self_test: done enabled_outputs={} restore_on_bms_ready={} blocked_by_bms={=bool} charger_enabled={=bool} bms_present={=bool}",
        enabled_outputs.describe(),
        outputs_restore_on_bms_ready.describe(),
        outputs_blocked_by_bms,
        charger_enabled,
        bms_addr.is_some()
    );

    reporter(SelfCheckStage::Done, ui);

    BootSelfTestResult {
        enabled_outputs,
        outputs_restore_on_bms_ready,
        outputs_blocked_by_bms,
        charger_probe_ok,
        charger_enabled,
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
    last_fault_log_at: Option<Instant>,
    last_therm_kill_hint_at: Option<Instant>,

    ina_ready: bool,
    ina_next_retry_at: Option<Instant>,

    tps_a_ready: bool,
    tps_a_next_retry_at: Option<Instant>,
    tps_b_ready: bool,
    tps_b_next_retry_at: Option<Instant>,

    bms_addr: Option<u8>,
    bms_next_poll_at: Instant,
    bms_next_retry_at: Option<Instant>,
    bms_last_int_poll_at: Option<Instant>,
    bms_poll_seq: u32,
    bms_ok_streak: u16,
    bms_err_streak: u16,

    chg_next_poll_at: Instant,
    chg_next_retry_at: Option<Instant>,
    chg_enabled: bool,
    charger_allowed: bool,
    chg_last_int_poll_at: Option<Instant>,
    bms_activation_state: BmsActivationState,
    bms_activation_deadline: Option<Instant>,
    bms_activation_backup: Option<ChargerActivationBackup>,
    outputs_restore_on_bms_ready: EnabledOutputs,
    outputs_blocked_by_bms: bool,

    ui_snapshot: SelfCheckUiSnapshot,
}

#[derive(Clone, Copy)]
struct ChargerActivationBackup {
    ctrl0: u8,
    ichg_reg: u16,
    iindpm_reg: u16,
    chg_enabled: bool,
}

#[derive(Clone, Copy)]
pub struct Config {
    pub enabled_outputs: EnabledOutputs,
    pub outputs_restore_on_bms_ready: EnabledOutputs,
    pub outputs_blocked_by_bms: bool,
    pub vout_mv: u16,
    pub ilimit_ma: u16,
    pub telemetry_period: Duration,
    pub retry_backoff: Duration,
    pub fault_log_min_interval: Duration,
    pub telemetry_include_vin_ch3: bool,
    pub tmp112_tlow_c_x16: i16,
    pub tmp112_thigh_c_x16: i16,
    pub charger_probe_ok: bool,
    pub charger_enabled: bool,
    pub force_min_charge: bool,
    pub bms_addr: Option<u8>,
    pub self_check_snapshot: SelfCheckUiSnapshot,
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
        let outputs_allowed = cfg.enabled_outputs != EnabledOutputs::None;
        let out_a_allowed = cfg.enabled_outputs.is_enabled(OutputChannel::OutA);
        let out_b_allowed = cfg.enabled_outputs.is_enabled(OutputChannel::OutB);
        let charger_allowed = cfg.charger_probe_ok;
        let bms_addr = cfg.bms_addr;

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
            last_fault_log_at: None,
            last_therm_kill_hint_at: None,

            ina_ready: false,
            ina_next_retry_at: if outputs_allowed { Some(now) } else { None },

            tps_a_ready: false,
            tps_a_next_retry_at: if out_a_allowed { Some(now) } else { None },
            tps_b_ready: false,
            tps_b_next_retry_at: if out_b_allowed { Some(now) } else { None },

            bms_addr,
            bms_next_poll_at: now,
            bms_next_retry_at: Some(now),
            bms_last_int_poll_at: None,
            bms_poll_seq: 0,
            bms_ok_streak: 0,
            bms_err_streak: 0,

            chg_next_poll_at: now,
            chg_next_retry_at: if charger_allowed { Some(now) } else { None },
            chg_enabled: false,
            charger_allowed,
            chg_last_int_poll_at: None,
            bms_activation_state: BmsActivationState::Idle,
            bms_activation_deadline: None,
            bms_activation_backup: None,
            outputs_restore_on_bms_ready: cfg.outputs_restore_on_bms_ready,
            outputs_blocked_by_bms: cfg.outputs_blocked_by_bms,
            ui_snapshot: cfg.self_check_snapshot,
        }
    }

    pub fn init_best_effort(&mut self) {
        if self.cfg.enabled_outputs != EnabledOutputs::None {
            self.try_init_ina();
            if self.cfg.enabled_outputs.is_enabled(OutputChannel::OutA) {
                self.try_configure_tps(OutputChannel::OutA);
            }
            if self.cfg.enabled_outputs.is_enabled(OutputChannel::OutB) {
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
        if self.outputs_blocked_by_bms {
            defmt::warn!("power: outputs blocked by bms state (boot self-test)");
        }

        if self.ui_snapshot.bq25792_allow_charge.is_none() {
            self.ui_snapshot.bq25792_allow_charge =
                Some(self.cfg.charger_enabled && self.charger_allowed);
        }
        if self.ui_snapshot.tps_a_enabled.is_none() {
            self.ui_snapshot.tps_a_enabled =
                Some(self.cfg.enabled_outputs.is_enabled(OutputChannel::OutA));
        }
        if self.ui_snapshot.tps_b_enabled.is_none() {
            self.ui_snapshot.tps_b_enabled =
                Some(self.cfg.enabled_outputs.is_enabled(OutputChannel::OutB));
        }
        self.recompute_ui_mode();
    }

    fn force_disable_outputs(&mut self) {
        self.tps_a_ready = false;
        self.tps_b_ready = false;
        self.tps_a_next_retry_at = None;
        self.tps_b_next_retry_at = None;
        self.ui_snapshot.tps_a_enabled = Some(false);
        self.ui_snapshot.tps_b_enabled = Some(false);

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

    pub fn tick(&mut self, irq: &IrqSnapshot) {
        self.maybe_retry();
        self.maybe_handle_fault(irq);
        self.maybe_poll_charger(irq);
        self.maybe_poll_bms(irq);
        self.maybe_track_bms_activation();
        self.maybe_print_telemetry();
    }

    pub fn ui_snapshot(&self) -> SelfCheckUiSnapshot {
        self.ui_snapshot
    }

    pub fn bms_activation_state(&self) -> BmsActivationState {
        self.bms_activation_state
    }

    pub fn clear_bms_activation_state(&mut self) {
        if self.bms_activation_state != BmsActivationState::Pending {
            self.bms_activation_state = BmsActivationState::Idle;
        }
    }

    pub fn request_bms_activation(&mut self) {
        if self.bms_activation_state == BmsActivationState::Pending {
            return;
        }
        if !is_bq40_activation_needed(&self.ui_snapshot) {
            return;
        }
        if !self.charger_allowed {
            self.finish_bms_activation(BmsActivationState::FailedComm);
            return;
        }

        let status0 = match bq25792::read_u8(&mut self.i2c, bq25792::reg::CHARGER_STATUS_0) {
            Ok(v) => v,
            Err(_) => {
                self.finish_bms_activation(BmsActivationState::FailedComm);
                return;
            }
        };
        let input_present = (status0 & bq25792::status0::VBUS_PRESENT_STAT) != 0
            || (status0 & bq25792::status0::AC1_PRESENT_STAT) != 0
            || (status0 & bq25792::status0::AC2_PRESENT_STAT) != 0
            || (status0 & bq25792::status0::PG_STAT) != 0;
        if !input_present {
            self.finish_bms_activation(BmsActivationState::FailedNoInput);
            return;
        }

        let ctrl0 = match bq25792::read_u8(&mut self.i2c, bq25792::reg::CHARGER_CONTROL_0) {
            Ok(v) => v,
            Err(_) => {
                self.finish_bms_activation(BmsActivationState::FailedComm);
                return;
            }
        };
        let ichg_reg = match bq25792::read_u16(&mut self.i2c, bq25792::reg::CHARGE_CURRENT_LIMIT) {
            Ok(v) => v,
            Err(_) => {
                self.finish_bms_activation(BmsActivationState::FailedComm);
                return;
            }
        };
        let iindpm_reg = match bq25792::read_u16(&mut self.i2c, bq25792::reg::INPUT_CURRENT_LIMIT) {
            Ok(v) => v,
            Err(_) => {
                self.finish_bms_activation(BmsActivationState::FailedComm);
                return;
            }
        };
        self.bms_activation_backup = Some(ChargerActivationBackup {
            ctrl0,
            ichg_reg,
            iindpm_reg,
            chg_enabled: self.chg_enabled,
        });

        self.chg_ilim_hiz_brk.set_low();
        if bq25792::set_charge_current_limit_ma(&mut self.i2c, BMS_ACTIVATION_FORCE_ICHG_MA)
            .is_err()
            || bq25792::set_input_current_limit_ma(&mut self.i2c, BMS_ACTIVATION_FORCE_IINDPM_MA)
                .is_err()
        {
            self.finish_bms_activation(BmsActivationState::FailedComm);
            return;
        }

        let desired_ctrl0 = (ctrl0 | bq25792::ctrl0::EN_CHG) & !bq25792::ctrl0::EN_HIZ;
        if bq25792::write_u8(
            &mut self.i2c,
            bq25792::reg::CHARGER_CONTROL_0,
            desired_ctrl0,
        )
        .is_err()
        {
            self.finish_bms_activation(BmsActivationState::FailedComm);
            return;
        }
        self.chg_ce.set_low();
        self.chg_enabled = true;

        let now = Instant::now();
        self.bms_activation_state = BmsActivationState::Pending;
        self.bms_activation_deadline = Some(now + BMS_ACTIVATION_WINDOW);
        self.bms_next_poll_at = now;
        self.bms_next_retry_at = None;
        self.chg_next_poll_at = now;
        self.chg_next_retry_at = None;
    }

    fn maybe_track_bms_activation(&mut self) {
        if self.bms_activation_state != BmsActivationState::Pending {
            return;
        }

        let bms_online = self.ui_snapshot.bq40z50_soc_pct.is_some()
            && matches!(
                self.ui_snapshot.bq40z50,
                SelfCheckCommState::Ok | SelfCheckCommState::Warn
            );
        let dsg_ready = self.ui_snapshot.bq40z50_discharge_ready == Some(true);
        let vbat_present = self.ui_snapshot.bq25792_vbat_present == Some(true);

        if bms_online && dsg_ready && vbat_present {
            self.finish_bms_activation(BmsActivationState::Succeeded);
            return;
        }

        let Some(deadline) = self.bms_activation_deadline else {
            self.finish_bms_activation(BmsActivationState::FailedComm);
            return;
        };
        if Instant::now() >= deadline {
            self.finish_bms_activation(BmsActivationState::FailedTimeout);
        }
    }

    fn finish_bms_activation(&mut self, result: BmsActivationState) {
        let mut restore_chg_enabled = false;
        if let Some(backup) = self.bms_activation_backup.take() {
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
            restore_chg_enabled = backup.chg_enabled;
        }
        if restore_chg_enabled {
            self.chg_ce.set_low();
            self.chg_enabled = true;
        } else {
            self.chg_ce.set_high();
            self.chg_enabled = false;
        }
        self.bms_activation_deadline = None;
        self.bms_activation_state = result;
        self.chg_next_poll_at = Instant::now();
        if result == BmsActivationState::Succeeded {
            self.try_restore_outputs_after_bms_ready();
        }
    }

    fn try_restore_outputs_after_bms_ready(&mut self) {
        if !self.outputs_blocked_by_bms {
            return;
        }
        let restore = self.outputs_restore_on_bms_ready;
        if restore == EnabledOutputs::None {
            self.outputs_blocked_by_bms = false;
            return;
        }

        defmt::info!(
            "power: bms recovered; restore outputs {}",
            restore.describe()
        );
        self.cfg.enabled_outputs = restore;
        self.ui_snapshot.tps_a_enabled = Some(restore.is_enabled(OutputChannel::OutA));
        self.ui_snapshot.tps_b_enabled = Some(restore.is_enabled(OutputChannel::OutB));
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
        self.outputs_blocked_by_bms = false;
    }

    fn recompute_ui_mode(&mut self) {
        let has_output = self.ui_snapshot.tps_a_enabled == Some(true)
            || self.ui_snapshot.tps_b_enabled == Some(true);
        self.ui_snapshot.mode =
            ups_mode_from_vbus(self.ui_snapshot.fusb302_vbus_present, has_output);
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

        if !self.tps_a_ready && self.cfg.enabled_outputs.is_enabled(OutputChannel::OutA) {
            if let Some(t) = self.tps_a_next_retry_at {
                if now >= t {
                    self.tps_a_next_retry_at = None;
                    self.try_configure_tps(OutputChannel::OutA);
                }
            }
        }

        if !self.tps_b_ready && self.cfg.enabled_outputs.is_enabled(OutputChannel::OutB) {
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
        let enabled = self.cfg.enabled_outputs.is_enabled(ch);
        let addr = ch.addr();

        match tps55288::configure_one(
            &mut self.i2c,
            ch,
            enabled,
            self.cfg.vout_mv,
            self.cfg.ilimit_ma,
        ) {
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
        if self.cfg.enabled_outputs == EnabledOutputs::None {
            return;
        }

        let now = Instant::now();
        if self.i2c1_int.is_low() || irq.i2c1_int != 0 {
            if tps55288::should_log_fault(
                now,
                &mut self.last_fault_log_at,
                self.cfg.fault_log_min_interval,
            ) {
                if self.cfg.enabled_outputs.is_enabled(OutputChannel::OutA) {
                    tps55288::log_fault_status(&mut self.i2c, OutputChannel::OutA, self.ina_ready);
                }
                if self.cfg.enabled_outputs.is_enabled(OutputChannel::OutB) {
                    tps55288::log_fault_status(&mut self.i2c, OutputChannel::OutB, self.ina_ready);
                }
            }
        }
    }

    fn maybe_print_telemetry(&mut self) {
        if self.cfg.enabled_outputs == EnabledOutputs::None {
            return;
        }

        let now = Instant::now();
        if now < self.next_telemetry_at {
            return;
        }
        self.next_telemetry_at = now + self.cfg.telemetry_period;

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

        if self.cfg.enabled_outputs.is_enabled(OutputChannel::OutA) {
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
            self.ui_snapshot.tps_a_iout_ma = capture.current_ma;
            self.ui_snapshot.tmp_a = if capture.temp_c_x16.is_some() {
                SelfCheckCommState::Ok
            } else {
                SelfCheckCommState::Err
            };
            self.ui_snapshot.tmp_a_c = capture.temp_c_x16.map(|v| v / 16);
        }
        if self.cfg.enabled_outputs.is_enabled(OutputChannel::OutB) {
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
            self.ui_snapshot.tps_b_iout_ma = capture.current_ma;
            self.ui_snapshot.tmp_b = if capture.temp_c_x16.is_some() {
                SelfCheckCommState::Ok
            } else {
                SelfCheckCommState::Err
            };
            self.ui_snapshot.tmp_b_c = capture.temp_c_x16.map(|v| v / 16);
        }

        self.ui_snapshot.ina_total_ma = match (
            self.ui_snapshot.tps_a_iout_ma,
            self.ui_snapshot.tps_b_iout_ma,
        ) {
            (Some(a), Some(b)) => Some(a + b),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };

        if self.cfg.telemetry_include_vin_ch3 {
            if self.ina_ready {
                let bus = ina3221::read_bus_mv(&mut self.i2c, ina3221::Channel::Ch3);
                let shunt = ina3221::read_shunt_uv(&mut self.i2c, ina3221::Channel::Ch3);
                let vbus_mv = match bus {
                    Ok(v) => TelemetryValue::Value(v),
                    Err(e) => TelemetryValue::Err(ina_error_kind(e)),
                };
                let current_ma = match shunt {
                    Ok(shunt_uv) => {
                        TelemetryValue::Value(ina3221::shunt_uv_to_current_ma(shunt_uv, 7))
                    }
                    Err(e) => TelemetryValue::Err(ina_error_kind(e)),
                };
                defmt::info!(
                    "telemetry ch=vin addr=0x40 vbus_mv={} current_ma={}",
                    vbus_mv,
                    current_ma
                );
            } else {
                defmt::info!(
                    "telemetry ch=vin addr=0x40 vbus_mv={} current_ma={}",
                    TelemetryValue::Err("ina_uninit"),
                    TelemetryValue::Err("ina_uninit")
                );
            }
        }
        self.recompute_ui_mode();
    }

    fn maybe_poll_charger(&mut self, irq: &IrqSnapshot) {
        if !self.charger_allowed {
            self.ui_snapshot.bq25792_allow_charge = Some(false);
            self.ui_snapshot.bq25792_ichg_ma = None;
            self.ui_snapshot.bq25792_vbat_present = None;
            self.recompute_ui_mode();
            return;
        }

        // Keep the charger polling independent from the TPS/INA telemetry period.
        const POLL_PERIOD: Duration = Duration::from_secs(1);
        const INT_MIN_INTERVAL: Duration = Duration::from_millis(50);

        let now = Instant::now();
        let mut due = now >= self.chg_next_poll_at;
        if irq.chg_int != 0 {
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
        self.chg_next_poll_at = now + POLL_PERIOD;

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

        let activation_pending = self.bms_activation_state == BmsActivationState::Pending;

        // Only enforce ship-FET path when charging is policy-enabled.
        let (sfet_present_before, sfet_present_after, ship_mode_before, ship_mode_after) =
            if self.cfg.charger_enabled || activation_pending {
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

        let (adc_enabled, vbat_adc_mv, vsys_adc_mv) = match bq25792::update_u8(
            &mut self.i2c,
            bq25792::reg::ADC_CONTROL,
            0,
            bq25792::adc_ctrl::ADC_EN,
        ) {
            Ok(adc_ctrl) => (
                (adc_ctrl & bq25792::adc_ctrl::ADC_EN) != 0,
                bq25792::read_u16(&mut self.i2c, bq25792::reg::VBAT_ADC).ok(),
                bq25792::read_u16(&mut self.i2c, bq25792::reg::VSYS_ADC).ok(),
            ),
            Err(e) => {
                defmt::warn!(
                    "charger: bq25792 warn stage=adc_ctrl err={} action=skip_adc_samples",
                    i2c_error_kind(e)
                );
                (false, None, None)
            }
        };

        let input_present = vbus_present || ac1_present || ac2_present || pg;
        let can_enable = input_present && !ts_cold && !ts_hot;
        let normal_allow_charge = can_enable && vbat_present;
        let force_allow_charge =
            (self.cfg.force_min_charge && can_enable) || (activation_pending && can_enable);
        let allow_charge = if activation_pending {
            force_allow_charge
        } else {
            (normal_allow_charge || force_allow_charge) && self.cfg.charger_enabled
        };
        let mut applied_ctrl0 = ctrl0;
        let mut applied_ichg_ma: Option<u16> = None;
        let mut applied_iindpm_ma: Option<u16> = None;

        if allow_charge {
            // Ensure we are not braking the converter (ILIM_HIZ < 0.75V forces non-switching).
            self.chg_ilim_hiz_brk.set_low();

            if force_allow_charge {
                fn decode_cur_ma(reg: u16) -> u16 {
                    (reg & 0x01FF) * 10
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
            }

            // Charge is enabled only when both `EN_CHG=1` and `CE=LOW`.
            let desired_ctrl0 = (ctrl0 | bq25792::ctrl0::EN_CHG) & !bq25792::ctrl0::EN_HIZ;
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

        defmt::info!(
            "charger: enabled={=bool} force_min_charge={=bool} normal_allow_charge={=bool} force_allow_charge={=bool} allow_charge={=bool} input_present={=bool} vbus_present={=bool} ac1_present={=bool} ac2_present={=bool} pg={=bool} vbat_present={=bool} vbat_adc_mv={=?} vsys_adc_mv={=?} adc_enabled={=bool} adc_done={=bool} ac_rb1_present={=bool} ac_rb2_present={=bool} vsys_min_reg={=bool} ts_cold={=bool} ts_cool={=bool} ts_warm={=bool} ts_hot={=bool} ichg_ma={=?} iindpm_ma={=?} sfet_present_before={=bool} sfet_present_after={=bool} ship_mode_before={=u8} ship_mode_after={=u8} chg_stat={} vbus_stat={} ico={} treg={=bool} dpdm={=bool} wd={=bool} poorsrc={=bool} vindpm={=bool} iindpm={=bool} st0=0x{=u8:x} st1=0x{=u8:x} st2=0x{=u8:x} st3=0x{=u8:x} st4=0x{=u8:x} fault0=0x{=u8:x} fault1=0x{=u8:x} ctrl0=0x{=u8:x}",
            self.chg_enabled,
            self.cfg.force_min_charge,
            normal_allow_charge,
            force_allow_charge,
            allow_charge,
            input_present,
            vbus_present,
            ac1_present,
            ac2_present,
            pg,
            vbat_present,
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

        let charger_fault = fault0 != 0 || fault1 != 0 || ts_cold || ts_hot;
        self.ui_snapshot.bq25792 = if charger_fault {
            SelfCheckCommState::Warn
        } else {
            SelfCheckCommState::Ok
        };
        self.ui_snapshot.bq25792_allow_charge = Some(allow_charge);
        self.ui_snapshot.bq25792_vbat_present = Some(vbat_present);
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
        self.recompute_ui_mode();

        self.chg_next_retry_at = None;
    }

    fn mark_charger_poll_failed(&mut self, now: Instant) {
        if self.bms_activation_state == BmsActivationState::Pending {
            self.finish_bms_activation(BmsActivationState::FailedComm);
        }
        self.chg_ce.set_high();
        self.chg_enabled = false;
        self.chg_next_retry_at = Some(now + self.cfg.retry_backoff);
        self.ui_snapshot.bq25792 = SelfCheckCommState::Err;
        self.ui_snapshot.bq25792_allow_charge = Some(false);
        self.ui_snapshot.bq25792_ichg_ma = None;
        self.ui_snapshot.bq25792_vbat_present = None;
        self.ui_snapshot.fusb302_vbus_present = None;
        self.recompute_ui_mode();
    }

    fn maybe_poll_bms(&mut self, irq: &IrqSnapshot) {
        const POLL_PERIOD: Duration = Duration::from_secs(2);
        const INT_MIN_INTERVAL: Duration = Duration::from_millis(100);

        let now = Instant::now();
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
            return;
        }
        if let Some(next_retry_at) = self.bms_next_retry_at {
            if now < next_retry_at {
                return;
            }
        }
        self.bms_next_poll_at = now + POLL_PERIOD;
        self.bms_poll_seq = self.bms_poll_seq.wrapping_add(1);
        let poll_seq = self.bms_poll_seq;

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
                    self.bms_next_retry_at = None;
                    self.bms_ok_streak = self.bms_ok_streak.saturating_add(1);
                    self.bms_err_streak = 0;
                    let rca_alarm = (s.batt_status & bq40z50::battery_status::RCA) != 0;
                    let discharge_ready = Self::bq40_discharge_ready(s.op_status);
                    self.ui_snapshot.bq40z50 = if discharge_ready != Some(true) || rca_alarm {
                        SelfCheckCommState::Warn
                    } else {
                        SelfCheckCommState::Ok
                    };
                    self.ui_snapshot.bq40z50_soc_pct = Some(s.rsoc_pct);
                    self.ui_snapshot.bq40z50_rca_alarm = Some(rca_alarm);
                    self.ui_snapshot.bq40z50_discharge_ready = discharge_ready;
                    if discharge_ready == Some(true) {
                        self.try_restore_outputs_after_bms_ready();
                    }
                    self.log_bq40z50_snapshot(addr, poll_seq, self.bms_ok_streak, btp_int_h, &s);
                    return;
                }
                Err(Bq40SnapshotReadError::Invalid(s)) => {
                    if idx + 1 == addr_count {
                        self.bms_addr = None;
                        self.bms_ok_streak = 0;
                        self.bms_err_streak = self.bms_err_streak.saturating_add(1);
                        self.bms_next_retry_at = Some(now + self.cfg.retry_backoff);
                        self.ui_snapshot.bq40z50 = SelfCheckCommState::Warn;
                        self.ui_snapshot.bq40z50_soc_pct = None;
                        self.ui_snapshot.bq40z50_rca_alarm = None;
                        self.ui_snapshot.bq40z50_discharge_ready = None;
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
                        self.bms_next_retry_at = Some(now + self.cfg.retry_backoff);
                        self.ui_snapshot.bq40z50 = SelfCheckCommState::Err;
                        self.ui_snapshot.bq40z50_soc_pct = None;
                        self.ui_snapshot.bq40z50_rca_alarm = None;
                        self.ui_snapshot.bq40z50_discharge_ready = None;

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
    }

    fn is_bq40_snapshot_reasonable(s: &Bq40z50Snapshot) -> bool {
        let temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(s.temp_k_x10);
        (-400..=1250).contains(&temp_c_x10)
            && (2500..=20_000).contains(&s.vpack_mv)
            && s.rsoc_pct <= 100
    }

    fn bq40_discharge_ready(op_status: Option<u16>) -> Option<bool> {
        bq40_decode_discharge_path(op_status).0
    }

    fn read_bq40z50_snapshot_strict(
        &mut self,
        addr: u8,
    ) -> Result<Bq40z50Snapshot, Bq40SnapshotReadError> {
        const MAX_FULL_SNAPSHOT_ATTEMPTS: usize = 2;
        let mut last_i2c_kind: Option<&'static str> = None;
        let mut last_invalid: Option<Bq40z50Snapshot> = None;

        for _ in 0..MAX_FULL_SNAPSHOT_ATTEMPTS {
            match self.read_bq40z50_snapshot_retry(addr) {
                Ok(snapshot) => {
                    if Self::is_bq40_snapshot_reasonable(&snapshot) {
                        return Ok(snapshot);
                    }
                    last_invalid = Some(snapshot);
                }
                Err(e) => {
                    last_i2c_kind = Some(i2c_error_kind(e));
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
    ) -> Result<Bq40z50Snapshot, esp_hal::i2c::master::Error> {
        const MAX_ATTEMPTS: usize = 3;

        for attempt in 0..MAX_ATTEMPTS {
            match self.read_bq40z50_snapshot(addr) {
                Ok(snapshot) => return Ok(snapshot),
                Err(e) => {
                    let retryable = matches!(
                        e,
                        esp_hal::i2c::master::Error::Timeout
                            | esp_hal::i2c::master::Error::AcknowledgeCheckFailed(_)
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
    ) -> Result<Bq40z50Snapshot, esp_hal::i2c::master::Error> {
        Ok(Bq40z50Snapshot {
            temp_k_x10: bq40z50::read_u16(&mut self.i2c, addr, bq40z50::cmd::TEMPERATURE)?,
            vpack_mv: bq40z50::read_u16(&mut self.i2c, addr, bq40z50::cmd::VOLTAGE)?,
            current_ma: bq40z50::read_i16(&mut self.i2c, addr, bq40z50::cmd::CURRENT)?,
            rsoc_pct: bq40z50::read_u16(
                &mut self.i2c,
                addr,
                bq40z50::cmd::RELATIVE_STATE_OF_CHARGE,
            )?,
            remcap: bq40z50::read_u16(&mut self.i2c, addr, bq40z50::cmd::REMAINING_CAPACITY)?,
            fcc: bq40z50::read_u16(&mut self.i2c, addr, bq40z50::cmd::FULL_CHARGE_CAPACITY)?,
            batt_status: bq40z50::read_u16(&mut self.i2c, addr, bq40z50::cmd::BATTERY_STATUS)?,
            op_status: bq40z50::read_u16(&mut self.i2c, addr, bq40z50::cmd::OPERATION_STATUS).ok(),
            cell_mv: [
                bq40z50::read_u16(&mut self.i2c, addr, bq40z50::cmd::CELL_VOLTAGE_1)?,
                bq40z50::read_u16(&mut self.i2c, addr, bq40z50::cmd::CELL_VOLTAGE_2)?,
                bq40z50::read_u16(&mut self.i2c, addr, bq40z50::cmd::CELL_VOLTAGE_3)?,
                bq40z50::read_u16(&mut self.i2c, addr, bq40z50::cmd::CELL_VOLTAGE_4)?,
            ],
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
        let (cell_min_mv, cell_max_mv, cell_delta_mv) = bq40_cell_min_max_delta(&s.cell_mv);
        let op_status_read_ok = s.op_status.is_some();

        let ec = bq40z50::battery_status::error_code(bs);

        defmt::info!(
            "bms: bq40z50 addr=0x{=u8:x} poll_seq={=u32} ok_streak={=u16} btp_int_h={=bool} temp_c_x10={=i32} vpack_mv={=u16} current_ma={=i16} flow={} flow_abs_ma={=u16} pack_power_mw={=i32} rsoc_pct={=u16} remcap={=u16} fcc={=u16} batt_status=0x{=u16:x} op_status={=?} op_status_read_ok={=bool} init={=bool} dsg={=bool} fc={=bool} fd={=bool} xchg={=?} xdsg={=?} chg_fet={=?} dsg_fet={=?} chg_ready={=?} dsg_ready={=?} chg_reason={} dsg_reason={} primary_reason={} pres={=?} sleep={=?} pf={=?} oca={=bool} tca={=bool} ota={=bool} tda={=bool} rca={=bool} rta={=bool} ec=0x{=u8:x} ec_str={} cell_min_mv={=u16} cell_max_mv={=u16} cell_delta_mv={=u16} c1_mv={=u16} c2_mv={=u16} c3_mv={=u16} c4_mv={=u16}",
            addr,
            poll_seq,
            ok_streak,
            btp_int_h,
            temp_c_x10,
            s.vpack_mv,
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
    temp_k_x10: u16,
    vpack_mv: u16,
    current_ma: i16,
    rsoc_pct: u16,
    remcap: u16,
    fcc: u16,
    batt_status: u16,
    op_status: Option<u16>,
    cell_mv: [u16; 4],
}

enum Bq40SnapshotReadError {
    I2c(&'static str),
    Invalid(Bq40z50Snapshot),
}
