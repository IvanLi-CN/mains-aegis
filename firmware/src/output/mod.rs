pub mod tps55288;

use esp_firmware::bq25792;
use esp_firmware::bq40z50;
use esp_firmware::ina3221;
use esp_firmware::tmp112;
use esp_hal::gpio::{Flex, Input};
use esp_hal::time::{Duration, Instant};

use crate::irq::IrqSnapshot;

use ::tps55288::Error as TpsError;

pub use self::tps55288::OutputChannel;

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
    pub charger_enabled: bool,
    pub bms_addr: Option<u8>,
}

pub fn log_i2c2_presence<I2C>(i2c: &mut I2C)
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    defmt::info!("self_test: i2c2 scan begin");
    for (addr, name) in [(0x21u8, "tca6408a"), (0x22u8, "fusb302b")] {
        let mut buf = [0u8; 1];
        match i2c.write_read(addr, &[0x00], &mut buf) {
            Ok(()) => defmt::info!(
                "self_test: i2c2 ok addr=0x{=u8:x} dev={} reg0=0x{=u8:x}",
                addr,
                name,
                buf[0]
            ),
            Err(e) => defmt::warn!(
                "self_test: i2c2 miss addr=0x{=u8:x} dev={} err={}",
                addr,
                name,
                i2c_error_kind(e)
            ),
        }
    }
}

pub fn boot_self_test<I2C>(
    i2c: &mut I2C,
    desired_outputs: EnabledOutputs,
    vout_mv: u16,
    ilimit_ma: u16,
    include_vin_ch3: bool,
    tmp_out_a_ok: bool,
    tmp_out_b_ok: bool,
    sync_ok: bool,
) -> BootSelfTestResult
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    defmt::info!(
        "self_test: begin vout_mv={=u16} ilimit_ma={=u16} tmp_a_ok={=bool} tmp_b_ok={=bool} sync_ok={=bool}",
        vout_mv,
        ilimit_ma,
        tmp_out_a_ok,
        tmp_out_b_ok,
        sync_ok
    );

    let ina_present = ina3221::read_manufacturer_id(&mut *i2c).is_ok();
    let tps_a_present = ::tps55288::Tps55288::with_address(&mut *i2c, OutputChannel::OutA.addr())
        .read_reg(::tps55288::registers::addr::MODE)
        .is_ok();
    let tps_b_present = ::tps55288::Tps55288::with_address(&mut *i2c, OutputChannel::OutB.addr())
        .read_reg(::tps55288::registers::addr::MODE)
        .is_ok();
    let tmp_a_present = tmp112::read_temp_c_x16(&mut *i2c, OutputChannel::OutA.tmp_addr()).is_ok();
    let tmp_b_present = tmp112::read_temp_c_x16(&mut *i2c, OutputChannel::OutB.tmp_addr()).is_ok();

    defmt::info!(
        "self_test: i2c1 presence ina3221={=bool} tps_a={=bool} tps_b={=bool} tmp_a={=bool} tmp_b={=bool} bq25792={=bool}",
        ina_present,
        tps_a_present,
        tps_b_present,
        tmp_a_present,
        tmp_b_present,
        bq25792::read_u8(&mut *i2c, bq25792::reg::CHARGER_CONTROL_0).is_ok()
    );

    let mut enabled_outputs = EnabledOutputs::None;
    let mut out_a_ok = false;
    let mut out_b_ok = false;

    let want_out_a = desired_outputs.is_enabled(OutputChannel::OutA);
    let want_out_b = desired_outputs.is_enabled(OutputChannel::OutB);
    let want_outputs = want_out_a || want_out_b;

    let out_a_devices_present = tps_a_present && tmp_a_present && tmp_out_a_ok;
    let out_b_devices_present = tps_b_present && tmp_b_present && tmp_out_b_ok;

    if want_outputs && sync_ok && ina_present {
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

        let ina_ok = ina3221::init_with_config(&mut *i2c, ina_cfg).is_ok();
        if !ina_ok {
            defmt::error!("self_test: ina3221 init failed; outputs disabled");
        } else {
            // Fail-safe: ensure both channels start disabled (even across MCU-only resets).
            if tps_a_present {
                let _ = ::tps55288::Tps55288::with_address(&mut *i2c, OutputChannel::OutA.addr())
                    .disable_output();
            }
            if tps_b_present {
                let _ = ::tps55288::Tps55288::with_address(&mut *i2c, OutputChannel::OutB.addr())
                    .disable_output();
            }

            let mut out_a_enabled = false;
            let mut out_b_enabled = false;

            if want_out_a {
                if out_a_devices_present {
                    if let Err((stage, e)) = tps55288::configure_one(
                        &mut *i2c,
                        OutputChannel::OutA,
                        true,
                        vout_mv,
                        ilimit_ma,
                    ) {
                        defmt::error!(
                            "self_test: tps out_a err stage={} err={}",
                            stage.as_str(),
                            tps_error_kind(e)
                        );
                    } else {
                        out_a_enabled = true;
                    }
                } else {
                    defmt::warn!(
                        "self_test: out_a skipped want=true tps_present={=bool} tmp_present={=bool} tmp_cfg_ok={=bool}",
                        tps_a_present,
                        tmp_a_present,
                        tmp_out_a_ok
                    );
                }
            }

            if want_out_b {
                if out_b_devices_present {
                    if let Err((stage, e)) = tps55288::configure_one(
                        &mut *i2c,
                        OutputChannel::OutB,
                        true,
                        vout_mv,
                        ilimit_ma,
                    ) {
                        defmt::error!(
                            "self_test: tps out_b err stage={} err={}",
                            stage.as_str(),
                            tps_error_kind(e)
                        );
                    } else {
                        out_b_enabled = true;
                    }
                } else {
                    defmt::warn!(
                        "self_test: out_b skipped want=true tps_present={=bool} tmp_present={=bool} tmp_cfg_ok={=bool}",
                        tps_b_present,
                        tmp_b_present,
                        tmp_out_b_ok
                    );
                }
            }

            if out_a_enabled || out_b_enabled {
                let start = Instant::now();
                while start.elapsed() < Duration::from_millis(500) {}

                // NOTE: `INA3221 VBUS` is known to read high on some boards (see Plan #0007).
                // Temporary policy: allow ±20% window for bring-up, but only enforce the lower-bound
                // to avoid false negatives caused by VBUS offset.
                const VBUS_TOL_PCT: u32 = 20;
                let lower = (vout_mv as u32) * (100 - VBUS_TOL_PCT) / 100;
                let upper = (vout_mv as u32) * (100 + VBUS_TOL_PCT) / 100;

                let vbus_a = if out_a_enabled {
                    ina3221::read_bus_mv(&mut *i2c, OutputChannel::OutA.ina_ch())
                        .map_err(ina_error_kind)
                } else {
                    Err("skipped")
                };
                let vbus_b = if out_b_enabled {
                    ina3221::read_bus_mv(&mut *i2c, OutputChannel::OutB.ina_ch())
                        .map_err(ina_error_kind)
                } else {
                    Err("skipped")
                };

                let status_a = if out_a_enabled {
                    ::tps55288::Tps55288::with_address(&mut *i2c, OutputChannel::OutA.addr())
                        .read_reg(::tps55288::registers::addr::STATUS)
                        .map_err(tps_error_kind)
                } else {
                    Err("skipped")
                };
                let status_b = if out_b_enabled {
                    ::tps55288::Tps55288::with_address(&mut *i2c, OutputChannel::OutB.addr())
                        .read_reg(::tps55288::registers::addr::STATUS)
                        .map_err(tps_error_kind)
                } else {
                    Err("skipped")
                };

                let fault_a = match &status_a {
                    Ok(v) => {
                        let bits = ::tps55288::registers::StatusBits::from_bits_truncate(*v);
                        bits.intersects(
                            ::tps55288::registers::StatusBits::SCP
                                | ::tps55288::registers::StatusBits::OCP
                                | ::tps55288::registers::StatusBits::OVP,
                        )
                    }
                    Err(_) => true,
                };
                let fault_b = match &status_b {
                    Ok(v) => {
                        let bits = ::tps55288::registers::StatusBits::from_bits_truncate(*v);
                        bits.intersects(
                            ::tps55288::registers::StatusBits::SCP
                                | ::tps55288::registers::StatusBits::OCP
                                | ::tps55288::registers::StatusBits::OVP,
                        )
                    }
                    Err(_) => true,
                };

                let in_range_a =
                    matches!(&vbus_a, Ok(v) if (*v as u32) >= lower && (*v as u32) <= upper);
                let in_range_b =
                    matches!(&vbus_b, Ok(v) if (*v as u32) >= lower && (*v as u32) <= upper);

                out_a_ok = out_a_enabled
                    && matches!(&vbus_a, Ok(v) if (*v as u32) >= lower)
                    && matches!(&status_a, Ok(_))
                    && !fault_a;
                out_b_ok = out_b_enabled
                    && matches!(&vbus_b, Ok(v) if (*v as u32) >= lower)
                    && matches!(&status_b, Ok(_))
                    && !fault_b;

                defmt::info!(
                    "self_test: outputs check vout_mv={=u16} tol_pct={=u32} lower_mv={=u32} upper_mv={=u32} out_a_vbus_mv={=?} out_b_vbus_mv={=?} out_a_in_range={=bool} out_b_in_range={=bool} out_a_status={=?} out_b_status={=?} out_a_fault={=bool} out_b_fault={=bool} out_a_ok={=bool} out_b_ok={=bool}",
                    vout_mv,
                    VBUS_TOL_PCT,
                    lower,
                    upper,
                    vbus_a,
                    vbus_b,
                    in_range_a,
                    in_range_b,
                    status_a,
                    status_b,
                    fault_a,
                    fault_b,
                    out_a_ok,
                    out_b_ok
                );

                enabled_outputs = match desired_outputs {
                    EnabledOutputs::None => EnabledOutputs::None,
                    EnabledOutputs::Only(OutputChannel::OutA) => {
                        if out_a_ok {
                            EnabledOutputs::Only(OutputChannel::OutA)
                        } else {
                            EnabledOutputs::None
                        }
                    }
                    EnabledOutputs::Only(OutputChannel::OutB) => {
                        if out_b_ok {
                            EnabledOutputs::Only(OutputChannel::OutB)
                        } else {
                            EnabledOutputs::None
                        }
                    }
                    EnabledOutputs::Both => match (out_a_ok, out_b_ok) {
                        (true, true) => EnabledOutputs::Both,
                        (true, false) => EnabledOutputs::Only(OutputChannel::OutA),
                        (false, true) => EnabledOutputs::Only(OutputChannel::OutB),
                        (false, false) => EnabledOutputs::None,
                    },
                };

                // Best-effort disable any channel that should not remain enabled.
                if out_a_enabled && !enabled_outputs.is_enabled(OutputChannel::OutA) {
                    let _ =
                        ::tps55288::Tps55288::with_address(&mut *i2c, OutputChannel::OutA.addr())
                            .disable_output();
                }
                if out_b_enabled && !enabled_outputs.is_enabled(OutputChannel::OutB) {
                    let _ =
                        ::tps55288::Tps55288::with_address(&mut *i2c, OutputChannel::OutB.addr())
                            .disable_output();
                }
            }
        }
    } else if want_outputs {
        defmt::warn!(
            "self_test: outputs skipped want_a={=bool} want_b={=bool} ina_present={=bool} sync_ok={=bool} tps_a_present={=bool} tps_b_present={=bool} tmp_a_present={=bool} tmp_b_present={=bool} tmp_a_cfg_ok={=bool} tmp_b_cfg_ok={=bool}",
            want_out_a,
            want_out_b,
            ina_present,
            sync_ok,
            tps_a_present,
            tps_b_present,
            tmp_a_present,
            tmp_b_present,
            tmp_out_a_ok,
            tmp_out_b_ok
        );
    }

    if want_outputs && enabled_outputs == EnabledOutputs::None {
        // Best-effort disable (even if one TPS is missing, this will still shut down the other).
        let _ = tps55288::configure_one(&mut *i2c, OutputChannel::OutA, false, vout_mv, ilimit_ma);
        let _ = tps55288::configure_one(&mut *i2c, OutputChannel::OutB, false, vout_mv, ilimit_ma);
    }

    let charger_enabled = bq25792::read_u8(&mut *i2c, bq25792::reg::CHARGER_CONTROL_0)
        .map(|v| {
            defmt::info!("self_test: bq25792 ok ctrl0=0x{=u8:x}", v);
        })
        .is_ok();
    if !charger_enabled {
        defmt::warn!("self_test: bq25792 missing/err; charger disabled");
    }

    let mut bms_addr: Option<u8> = None;
    for addr in bq40z50::I2C_ADDRESS_CANDIDATES {
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
            break;
        }

        defmt::warn!("self_test: bq40z50 miss addr=0x{=u8:x}", addr);
    }

    if bms_addr.is_none() {
        defmt::warn!("self_test: bq40z50 missing/err; battery module disabled");
    }

    defmt::info!(
        "self_test: done enabled_outputs={} outputs_ok={=bool} charger_enabled={=bool}",
        enabled_outputs.describe(),
        out_a_ok && out_b_ok,
        charger_enabled
    );

    BootSelfTestResult {
        enabled_outputs,
        charger_enabled,
        bms_addr,
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
    pub charger_enabled: bool,
    pub bms_addr: Option<u8>,
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
        let charger_allowed = cfg.charger_enabled;
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
        }

        if !self.charger_allowed {
            defmt::warn!("charger: bq25792 disabled (boot self-test)");
            self.chg_ce.set_high();
            self.chg_enabled = false;
        }

        if self.bms_addr.is_none() {
            defmt::warn!("bms: bq40z50 disabled (boot self-test)");
        }
    }

    pub fn tick(&mut self, irq: &IrqSnapshot) {
        self.maybe_retry();
        self.maybe_handle_fault(irq);
        self.maybe_poll_charger(irq);
        self.maybe_poll_bms(irq);
        self.maybe_print_telemetry();
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
            }
            Err((stage, e)) => {
                let kind = tps_error_kind(e);
                self.mark_tps_failed(ch, Instant::now() + self.cfg.retry_backoff);
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
            tps55288::print_telemetry_line(
                &mut self.i2c,
                OutputChannel::OutA,
                self.ina_ready,
                therm_kill_n,
            );
        }
        if self.cfg.enabled_outputs.is_enabled(OutputChannel::OutB) {
            tps55288::print_telemetry_line(
                &mut self.i2c,
                OutputChannel::OutB,
                self.ina_ready,
                therm_kill_n,
            );
        }

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
    }

    fn maybe_poll_charger(&mut self, irq: &IrqSnapshot) {
        if !self.charger_allowed {
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
                self.chg_ce.set_high();
                self.chg_enabled = false;
                self.chg_next_retry_at = Some(now + self.cfg.retry_backoff);
                defmt::error!(
                    "charger: bq25792 err stage=ctrl0_read err={}",
                    i2c_error_kind(e)
                );
                return;
            }
        };

        if let Err(e) = bq25792::read_block(&mut self.i2c, bq25792::reg::CHARGER_STATUS_0, &mut st)
        {
            self.chg_ce.set_high();
            self.chg_enabled = false;
            self.chg_next_retry_at = Some(now + self.cfg.retry_backoff);
            defmt::error!(
                "charger: bq25792 err stage=status_read err={}",
                i2c_error_kind(e)
            );
            return;
        }
        if let Err(e) = bq25792::read_block(&mut self.i2c, bq25792::reg::FAULT_STATUS_0, &mut fault)
        {
            self.chg_ce.set_high();
            self.chg_enabled = false;
            self.chg_next_retry_at = Some(now + self.cfg.retry_backoff);
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

        let can_enable = vbat_present && !ts_cold && !ts_hot;
        let mut applied_ctrl0 = ctrl0;

        if can_enable {
            // Ensure we are not braking the converter (ILIM_HIZ < 0.75V forces non-switching).
            self.chg_ilim_hiz_brk.set_low();

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
                        self.chg_ce.set_high();
                        self.chg_enabled = false;
                        self.chg_next_retry_at = Some(now + self.cfg.retry_backoff);
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
            "charger: enabled={=bool} vbus_present={=bool} ac1_present={=bool} ac2_present={=bool} pg={=bool} vbat_present={=bool} ts_cold={=bool} ts_cool={=bool} ts_warm={=bool} ts_hot={=bool} chg_stat={} vbus_stat={} ico={} treg={=bool} dpdm={=bool} wd={=bool} poorsrc={=bool} vindpm={=bool} iindpm={=bool} st0=0x{=u8:x} st1=0x{=u8:x} st2=0x{=u8:x} st3=0x{=u8:x} st4=0x{=u8:x} fault0=0x{=u8:x} fault1=0x{=u8:x} ctrl0=0x{=u8:x}",
            self.chg_enabled,
            vbus_present,
            ac1_present,
            ac2_present,
            pg,
            vbat_present,
            ts_cold,
            ts_cool,
            ts_warm,
            ts_hot,
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

        self.chg_next_retry_at = None;
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

        // The BQ40Z50 SMBus address is data-flash configurable. The project address map uses 0x0B,
        // while the TI TRM states a DF default of 0x16. Probe both to keep bring-up resilient.
        let addr_order: [u8; 2] = match self.bms_addr {
            Some(a) if a == bq40z50::I2C_ADDRESS_FALLBACK => {
                [bq40z50::I2C_ADDRESS_FALLBACK, bq40z50::I2C_ADDRESS_PRIMARY]
            }
            Some(a) if a == bq40z50::I2C_ADDRESS_PRIMARY => {
                [bq40z50::I2C_ADDRESS_PRIMARY, bq40z50::I2C_ADDRESS_FALLBACK]
            }
            _ => bq40z50::I2C_ADDRESS_CANDIDATES,
        };

        for (idx, addr) in addr_order.iter().copied().enumerate() {
            match self.read_bq40z50_snapshot(addr) {
                Ok(s) => {
                    self.bms_addr = Some(addr);
                    self.bms_next_retry_at = None;
                    self.log_bq40z50_snapshot(addr, btp_int_h, &s);
                    return;
                }
                Err(e) => {
                    // Only log one line after the final address attempt.
                    if idx + 1 == addr_order.len() {
                        self.bms_addr = None;
                        self.bms_next_retry_at = Some(now + self.cfg.retry_backoff);

                        let kind = i2c_error_kind(e);
                        if kind == "i2c_nack" || kind == "i2c_timeout" {
                            defmt::warn!(
                                "bms: bq40z50 absent addrs=0x{=u8:x}/0x{=u8:x} err={} btp_int_h={=bool}",
                                bq40z50::I2C_ADDRESS_PRIMARY,
                                bq40z50::I2C_ADDRESS_FALLBACK,
                                kind,
                                btp_int_h
                            );
                        } else {
                            defmt::error!(
                                "bms: bq40z50 err addrs=0x{=u8:x}/0x{=u8:x} err={} btp_int_h={=bool}",
                                bq40z50::I2C_ADDRESS_PRIMARY,
                                bq40z50::I2C_ADDRESS_FALLBACK,
                                kind,
                                btp_int_h
                            );
                        }
                    }
                }
            }
        }
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
