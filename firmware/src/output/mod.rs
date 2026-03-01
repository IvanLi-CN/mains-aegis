pub mod tps55288;

use esp_firmware::bq25792;
use esp_firmware::bq40z50;
use esp_firmware::ina3221;
use esp_firmware::tmp112;
use esp_hal::gpio::{Flex, Input};
use esp_hal::time::{Duration, Instant};

use crate::front_panel_scene::{SelfCheckCommState, SelfCheckUiSnapshot, UpsMode};
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
    for addr in bms_probe_candidates().iter().copied() {
        let temp = bq40z50::read_u16(&mut *i2c, addr, bq40z50::cmd::TEMPERATURE);
        let voltage = bq40z50::read_u16(&mut *i2c, addr, bq40z50::cmd::VOLTAGE);
        let current = bq40z50::read_i16(&mut *i2c, addr, bq40z50::cmd::CURRENT);
        let soc = bq40z50::read_u16(&mut *i2c, addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE);
        let status = bq40z50::read_u16(&mut *i2c, addr, bq40z50::cmd::BATTERY_STATUS);

        if let (Ok(temp_k_x10), Ok(voltage_mv), Ok(current_ma), Ok(soc_pct), Ok(status_raw)) =
            (temp, voltage, current, soc, status)
        {
            let temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(temp_k_x10);
            let err_code = bq40z50::battery_status::error_code(status_raw);
            defmt::info!(
                "self_test: bq40z50 ok addr=0x{=u8:x} temp_c_x10={=i32} voltage_mv={=u16} current_ma={=i16} soc_pct={=u16} status=0x{=u16:x} err_code={} err_str={}",
                addr,
                temp_c_x10,
                voltage_mv,
                current_ma,
                soc_pct,
                status_raw,
                err_code,
                bq40z50::decode_error_code(err_code)
            );
            bms_addr = Some(addr);
            bms_soc_pct = Some(soc_pct);
            bms_rca_alarm = Some((status_raw & bq40z50::battery_status::RCA) != 0);
            break;
        }

        defmt::warn!("self_test: bq40z50 miss addr=0x{=u8:x}", addr);
    }

    if bms_addr.is_none() {
        defmt::warn!("self_test: bq40z50 missing/err; battery module disabled");
        ui.bq40z50 = SelfCheckCommState::Err;
    } else if bms_rca_alarm == Some(true) {
        ui.bq40z50 = SelfCheckCommState::Warn;
    } else {
        ui.bq40z50 = SelfCheckCommState::Ok;
    }
    ui.bq40z50_soc_pct = bms_soc_pct;
    ui.bq40z50_rca_alarm = bms_rca_alarm;
    reporter(SelfCheckStage::Bms, ui);

    // Stage 3: BQ25792.
    let mut charger_enabled = match bq25792::read_u8(&mut *i2c, bq25792::reg::CHARGER_CONTROL_0) {
        Ok(v) => {
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
    ui.bq25792 = if charger_enabled {
        SelfCheckCommState::Ok
    } else {
        SelfCheckCommState::Err
    };
    ui.bq25792_ichg_ma = bq25792::read_u16(&mut *i2c, bq25792::reg::CHARGE_CURRENT_LIMIT)
        .ok()
        .map(|v| (v & 0x01ff) * 10);
    if let Ok(status0) = bq25792::read_u8(&mut *i2c, bq25792::reg::CHARGER_STATUS_0) {
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

    if bms_addr.is_none() {
        // Policy: after init, BQ40 missing means TPS outputs must be disabled.
        out_a_allowed = false;
        out_b_allowed = false;

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

    let enabled_outputs = match (out_a_allowed, out_b_allowed) {
        (true, true) => EnabledOutputs::Both,
        (true, false) => EnabledOutputs::Only(OutputChannel::OutA),
        (false, true) => EnabledOutputs::Only(OutputChannel::OutB),
        (false, false) => EnabledOutputs::None,
    };

    ui.mode = ups_mode_from_vbus(ui.fusb302_vbus_present, out_a_allowed || out_b_allowed);

    defmt::info!(
        "self_test: done enabled_outputs={} charger_enabled={=bool} bms_present={=bool}",
        enabled_outputs.describe(),
        charger_enabled,
        bms_addr.is_some()
    );

    reporter(SelfCheckStage::Done, ui);

    BootSelfTestResult {
        enabled_outputs,
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

    chg_next_poll_at: Instant,
    chg_next_retry_at: Option<Instant>,
    chg_enabled: bool,
    charger_allowed: bool,
    chg_last_int_poll_at: Option<Instant>,

    ui_snapshot: SelfCheckUiSnapshot,
}

#[derive(Clone, Copy)]
pub struct Config {
    pub enabled_outputs: EnabledOutputs,
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

            chg_next_poll_at: now,
            chg_next_retry_at: if charger_allowed { Some(now) } else { None },
            chg_enabled: false,
            charger_allowed,
            chg_last_int_poll_at: None,
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
        self.maybe_print_telemetry();
    }

    pub fn ui_snapshot(&self) -> SelfCheckUiSnapshot {
        self.ui_snapshot
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

        // Only enforce ship-FET path when charging is policy-enabled.
        let (sfet_present_before, sfet_present_after, ship_mode_before, ship_mode_after) =
            if self.cfg.charger_enabled {
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

        let input_present = vbus_present || ac1_present || ac2_present || pg;
        let can_enable = input_present && !ts_cold && !ts_hot;
        let normal_allow_charge = can_enable && vbat_present;
        let force_allow_charge = self.cfg.force_min_charge && can_enable;
        let allow_charge = (normal_allow_charge || force_allow_charge) && self.cfg.charger_enabled;
        let mut applied_ctrl0 = ctrl0;
        let mut applied_ichg_ma: Option<u16> = None;
        let mut applied_iindpm_ma: Option<u16> = None;

        if allow_charge {
            // Ensure we are not braking the converter (ILIM_HIZ < 0.75V forces non-switching).
            self.chg_ilim_hiz_brk.set_low();

            if force_allow_charge {
                const FORCE_MIN_ICHG_MA: u16 = 50;
                const FORCE_MIN_IINDPM_MA: u16 = 100;

                fn decode_cur_ma(reg: u16) -> u16 {
                    (reg & 0x01FF) * 10
                }

                match bq25792::set_charge_current_limit_ma(&mut self.i2c, FORCE_MIN_ICHG_MA) {
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

                match bq25792::set_input_current_limit_ma(&mut self.i2c, FORCE_MIN_IINDPM_MA) {
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
            "charger: enabled={=bool} force_min_charge={=bool} normal_allow_charge={=bool} force_allow_charge={=bool} allow_charge={=bool} input_present={=bool} vbus_present={=bool} ac1_present={=bool} ac2_present={=bool} pg={=bool} vbat_present={=bool} ts_cold={=bool} ts_cool={=bool} ts_warm={=bool} ts_hot={=bool} ichg_ma={=?} iindpm_ma={=?} sfet_present_before={=bool} sfet_present_after={=bool} ship_mode_before={=u8} ship_mode_after={=u8} chg_stat={} vbus_stat={} ico={} treg={=bool} dpdm={=bool} wd={=bool} poorsrc={=bool} vindpm={=bool} iindpm={=bool} st0=0x{=u8:x} st1=0x{=u8:x} st2=0x{=u8:x} st3=0x{=u8:x} st4=0x{=u8:x} fault0=0x{=u8:x} fault1=0x{=u8:x} ctrl0=0x{=u8:x}",
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
        self.chg_ce.set_high();
        self.chg_enabled = false;
        self.chg_next_retry_at = Some(now + self.cfg.retry_backoff);
        self.ui_snapshot.bq25792 = SelfCheckCommState::Err;
        self.ui_snapshot.bq25792_allow_charge = Some(false);
        self.ui_snapshot.bq25792_ichg_ma = None;
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
                    let rca_alarm = (s.batt_status & bq40z50::battery_status::RCA) != 0;
                    self.ui_snapshot.bq40z50 = if rca_alarm {
                        SelfCheckCommState::Warn
                    } else {
                        SelfCheckCommState::Ok
                    };
                    self.ui_snapshot.bq40z50_soc_pct = Some(s.rsoc_pct);
                    self.ui_snapshot.bq40z50_rca_alarm = Some(rca_alarm);
                    self.log_bq40z50_snapshot(addr, btp_int_h, &s);
                    return;
                }
                Err(Bq40SnapshotReadError::Invalid(s)) => {
                    if idx + 1 == addr_count {
                        self.bms_addr = None;
                        self.bms_next_retry_at = Some(now + self.cfg.retry_backoff);
                        self.ui_snapshot.bq40z50 = SelfCheckCommState::Warn;
                        self.ui_snapshot.bq40z50_soc_pct = None;
                        self.ui_snapshot.bq40z50_rca_alarm = None;
                        let temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(s.temp_k_x10);
                        defmt::warn!(
                            "bms: bq40z50 invalid addrs={} temp_c_x10={=i32} vpack_mv={=u16} rsoc_pct={=u16}",
                            BMS_ADDR_LOG,
                            temp_c_x10,
                            s.vpack_mv,
                            s.rsoc_pct
                        );
                    }
                }
                Err(Bq40SnapshotReadError::I2c(kind)) => {
                    // Only log one line after the final address attempt.
                    if idx + 1 == addr_count {
                        self.bms_addr = None;
                        self.bms_next_retry_at = Some(now + self.cfg.retry_backoff);
                        self.ui_snapshot.bq40z50 = SelfCheckCommState::Err;
                        self.ui_snapshot.bq40z50_soc_pct = None;
                        self.ui_snapshot.bq40z50_rca_alarm = None;

                        if kind == "i2c_nack" || kind == "i2c_timeout" {
                            defmt::warn!(
                                "bms: bq40z50 absent addrs={} err={} btp_int_h={=bool}",
                                BMS_ADDR_LOG,
                                kind,
                                btp_int_h
                            );
                        } else {
                            defmt::error!(
                                "bms: bq40z50 err addrs={} err={} btp_int_h={=bool}",
                                BMS_ADDR_LOG,
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
            cell_mv: [
                bq40z50::read_u16(&mut self.i2c, addr, bq40z50::cmd::CELL_VOLTAGE_1)?,
                bq40z50::read_u16(&mut self.i2c, addr, bq40z50::cmd::CELL_VOLTAGE_2)?,
                bq40z50::read_u16(&mut self.i2c, addr, bq40z50::cmd::CELL_VOLTAGE_3)?,
                bq40z50::read_u16(&mut self.i2c, addr, bq40z50::cmd::CELL_VOLTAGE_4)?,
            ],
        })
    }

    fn log_bq40z50_snapshot(&self, addr: u8, btp_int_h: bool, s: &Bq40z50Snapshot) {
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

        let ec = bq40z50::battery_status::error_code(bs);

        defmt::info!(
            "bms: bq40z50 addr=0x{=u8:x} btp_int_h={=bool} temp_c_x10={=i32} vpack_mv={=u16} current_ma={=i16} rsoc_pct={=u16} remcap={=u16} fcc={=u16} batt_status=0x{=u16:x} init={=bool} dsg={=bool} fc={=bool} fd={=bool} oca={=bool} tca={=bool} ota={=bool} tda={=bool} rca={=bool} rta={=bool} ec=0x{=u8:x} ec_str={} c1_mv={=u16} c2_mv={=u16} c3_mv={=u16} c4_mv={=u16}",
            addr,
            btp_int_h,
            temp_c_x10,
            s.vpack_mv,
            s.current_ma,
            s.rsoc_pct,
            s.remcap,
            s.fcc,
            bs,
            init,
            dsg,
            fc,
            fd,
            oca,
            tca,
            ota,
            tda,
            rca,
            rta,
            ec,
            bq40z50::decode_error_code(ec),
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
    cell_mv: [u16; 4],
}

enum Bq40SnapshotReadError {
    I2c(&'static str),
    Invalid(Bq40z50Snapshot),
}
