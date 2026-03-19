use esp_firmware::ina3221;
use esp_firmware::tmp112;
use esp_hal::time::{Duration, Instant};

use ::tps55288::data_types::{
    CableCompLevel, CableCompOption, FeedbackSource, I2cAddress, InternalFeedbackRatio,
    LightLoadMode, LightLoadOverride, OcpDelay, VccSource, VoutSlewRate,
};
use ::tps55288::registers::{
    addr as tps_addr, CdcBits, ModeBits, StatusBits, VoutFsBits, VoutSrBits,
};

use super::{
    i2c_error_kind, ina_error_kind, tps_error_kind, TelemetryBool, TelemetryTempC, TelemetryU16,
    TelemetryU8, TelemetryValue,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputChannel {
    OutA,
    OutB,
}

impl OutputChannel {
    pub const fn addr(self) -> u8 {
        match self {
            OutputChannel::OutA => 0x74,
            OutputChannel::OutB => 0x75,
        }
    }

    pub const fn addr_enum(self) -> I2cAddress {
        match self {
            OutputChannel::OutA => I2cAddress::Addr0x74,
            OutputChannel::OutB => I2cAddress::Addr0x75,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            OutputChannel::OutA => "out_a",
            OutputChannel::OutB => "out_b",
        }
    }

    pub const fn ina_ch(self) -> ina3221::Channel {
        // Frozen by docs/plan/0005:tps55288-control/contracts/config.md:
        // INA3221 CH2 -> OUT-A, CH1 -> OUT-B
        match self {
            OutputChannel::OutA => ina3221::Channel::Ch2,
            OutputChannel::OutB => ina3221::Channel::Ch1,
        }
    }

    pub const fn tmp_addr(self) -> u8 {
        match self {
            OutputChannel::OutA => 0x48,
            OutputChannel::OutB => 0x49,
        }
    }
}

pub fn configure_one<I2C>(
    i2c: &mut I2C,
    ch: OutputChannel,
    enabled: bool,
    default_vout_mv: u16,
    default_ilimit_ma: u16,
) -> Result<
    (),
    (
        ConfigureStage,
        ::tps55288::Error<esp_hal::i2c::master::Error>,
    ),
>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let addr = ch.addr();
    let mut tps = ::tps55288::Tps55288::with_address(&mut *i2c, addr);

    // Start from a known safe state.
    tps.disable_output()
        .map_err(|e| (ConfigureStage::Disable, e))?;
    tps.init().map_err(|e| (ConfigureStage::Init, e))?;

    // Per TPS55288 datasheet: if MODE[0]=1 (register control), we must explicitly set:
    // - VCC source (VCC_EXT) to match board wiring
    // - I2C address (I2CADD) to avoid "address lost" across MCU-only resets
    // - Light-load mode (PFM/FPWM)
    //
    // This board intends to use external 5V VCC and 0x74/0x75 split addresses.
    tps.set_mode_control(
        LightLoadOverride::FromRegister,
        VccSource::External5v,
        ch.addr_enum(),
        LightLoadMode::Pfm, // Do not force PWM (FPWM disabled)
    )
    .map_err(|e| (ConfigureStage::Mode, e))?;

    // Start-up transients can falsely trip OCP on some boards (large Cout, wiring, or sense noise).
    // Keep the same ILIM target, but widen the OCP response delay + slow the ramp for bring-up.
    tps.set_vout_sr(VoutSlewRate::Sr1p25MvPerUs, OcpDelay::Ms12_288)
        .map_err(|e| (ConfigureStage::VoutSr, e))?;
    tps.set_feedback(FeedbackSource::Internal, InternalFeedbackRatio::R0_0564)
        .map_err(|e| (ConfigureStage::Feedback, e))?;
    tps.set_cable_comp(
        CableCompOption::Internal,
        CableCompLevel::V0p0,
        false, // do not mask faults; we want interrupts + observable status during bring-up
        false,
        false,
    )
    .map_err(|e| (ConfigureStage::Cdc, e))?;
    tps.set_vout_mv(default_vout_mv)
        .map_err(|e| (ConfigureStage::Vout, e))?;
    tps.set_ilim_ma(default_ilimit_ma, enabled)
        .map_err(|e| (ConfigureStage::Ilim, e))?;

    if enabled {
        tps.enable_output()
            .map_err(|e| (ConfigureStage::Enable, e))?;
    }

    Ok(())
}

#[derive(Clone, Copy)]
pub enum ConfigureStage {
    Disable,
    Init,
    Mode,
    VoutSr,
    Feedback,
    Cdc,
    Vout,
    Ilim,
    Enable,
}

impl ConfigureStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            ConfigureStage::Disable => "disable",
            ConfigureStage::Init => "init",
            ConfigureStage::Mode => "mode",
            ConfigureStage::VoutSr => "vout_sr",
            ConfigureStage::Feedback => "feedback",
            ConfigureStage::Cdc => "cdc",
            ConfigureStage::Vout => "vout",
            ConfigureStage::Ilim => "ilim",
            ConfigureStage::Enable => "enable",
        }
    }
}

pub fn log_configured<I2C>(i2c: &mut I2C, ch: OutputChannel, enabled: bool, defer_status_read: bool)
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let addr = ch.addr();
    let mut tps = ::tps55288::Tps55288::with_address(&mut *i2c, addr);

    let vset = tps.get_vout_mv();
    let ilim = tps.get_ilim_ma();

    let mode = match tps.read_reg(tps_addr::MODE) {
        Ok(v) => TelemetryU8::Value(v),
        Err(e) => TelemetryU8::Err(tps_error_kind(e)),
    };
    let vout_fs = match tps.read_reg(tps_addr::VOUT_FS) {
        Ok(v) => TelemetryU8::Value(v),
        Err(e) => TelemetryU8::Err(tps_error_kind(e)),
    };
    let status = if defer_status_read {
        None
    } else {
        Some(match tps.read_reg(tps_addr::STATUS) {
            Ok(v) => TelemetryU8::Value(v),
            Err(e) => TelemetryU8::Err(tps_error_kind(e)),
        })
    };

    let oe = match mode {
        TelemetryU8::Value(v) => {
            TelemetryBool::Value(ModeBits::from_bits_truncate(v).contains(ModeBits::OE))
        }
        TelemetryU8::Err(e) => TelemetryBool::Err(e),
    };
    let fpwm = match mode {
        TelemetryU8::Value(v) => {
            TelemetryBool::Value(ModeBits::from_bits_truncate(v).contains(ModeBits::PFM))
        }
        TelemetryU8::Err(e) => TelemetryBool::Err(e),
    };
    let mode_override = match mode {
        TelemetryU8::Value(v) => {
            TelemetryBool::Value(ModeBits::from_bits_truncate(v).contains(ModeBits::MODE))
        }
        TelemetryU8::Err(e) => TelemetryBool::Err(e),
    };
    let fb_ext = match vout_fs {
        TelemetryU8::Value(v) => {
            TelemetryBool::Value(VoutFsBits::from_bits_truncate(v).contains(VoutFsBits::FB_EXT))
        }
        TelemetryU8::Err(e) => TelemetryBool::Err(e),
    };

    let (status, scp, ocp, ovp) = match status {
        Some(TelemetryU8::Value(v)) => {
            let bits = StatusBits::from_bits_truncate(v);
            (
                TelemetryU8::Value(v),
                TelemetryBool::Value(bits.contains(StatusBits::SCP)),
                TelemetryBool::Value(bits.contains(StatusBits::OCP)),
                TelemetryBool::Value(bits.contains(StatusBits::OVP)),
            )
        }
        Some(TelemetryU8::Err(e)) => (
            TelemetryU8::Err(e),
            TelemetryBool::Err(e),
            TelemetryBool::Err(e),
            TelemetryBool::Err(e),
        ),
        None => (
            TelemetryU8::Err("startup_capture"),
            TelemetryBool::Err("startup_capture"),
            TelemetryBool::Err("startup_capture"),
            TelemetryBool::Err("startup_capture"),
        ),
    };

    defmt::info!(
        "power: tps addr=0x{=u8:x} configured enabled={=bool} mode={} override={} oe={} fpwm={} vout_fs={} fb_ext={} status={} scp={} ocp={} ovp={} vout_mv={=?} ilim_ma={=?}",
        addr,
        enabled,
        mode,
        mode_override,
        oe,
        fpwm,
        vout_fs,
        fb_ext,
        status,
        scp,
        ocp,
        ovp,
        vset,
        ilim
    );
}

pub fn log_fault_status<I2C>(i2c: &mut I2C, ch: OutputChannel, ina_ready: bool)
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let addr = ch.addr();
    let mut should_log_ina = false;
    {
        let mut tps = ::tps55288::Tps55288::with_address(&mut *i2c, addr);

        let status = tps.read_status_raw();
        let mode = tps.read_reg(tps_addr::MODE);
        let vout_sr = tps.read_reg(tps_addr::VOUT_SR);
        let vout_fs = tps.read_reg(tps_addr::VOUT_FS);
        let cdc = tps.read_reg(tps_addr::CDC);
        let iout_limit = tps.read_reg(tps_addr::IOUT_LIMIT);

        match (status, mode, vout_sr, vout_fs, cdc, iout_limit) {
            (Ok(status), Ok(mode), Ok(vout_sr), Ok(vout_fs), Ok(cdc), Ok(iout_limit)) => {
                should_log_ina = ina_ready;

                let faults = StatusBits::from_bits_truncate(status.bits());
                let mode_bits = status.bits() & 0b11;

                let mode_bits_reg = ModeBits::from_bits_truncate(mode);
                let vout_sr_bits = VoutSrBits::from_bits_truncate(vout_sr);
                let vout_fs_bits = VoutFsBits::from_bits_truncate(vout_fs);
                let cdc_bits = CdcBits::from_bits_truncate(cdc);

                defmt::warn!(
                    "power: fault ch={} addr=0x{=u8:x} status=0x{=u8:x} mode_bits={} scp={} ocp={} ovp={} mode=0x{=u8:x} override={} oe={} i2cadd={} vcc_ext={} fpwm={} hiccup={} dischg={} fswdbl={} vout_sr=0x{=u8:x} ocp_delay0={} ocp_delay1={} sr0={} sr1={} vout_fs=0x{=u8:x} fb_ext={} cdc=0x{=u8:x} mask_sc={} mask_ocp={} mask_ovp={} iout_limit=0x{=u8:x}",
                    ch.name(),
                    addr,
                    status.bits(),
                    mode_bits,
                    faults.contains(StatusBits::SCP),
                    faults.contains(StatusBits::OCP),
                    faults.contains(StatusBits::OVP),
                    mode,
                    mode_bits_reg.contains(ModeBits::MODE),
                    mode_bits_reg.contains(ModeBits::OE),
                    mode_bits_reg.contains(ModeBits::I2CADD),
                    mode_bits_reg.contains(ModeBits::VCC_EXT),
                    mode_bits_reg.contains(ModeBits::PFM),
                    mode_bits_reg.contains(ModeBits::HICCUP),
                    mode_bits_reg.contains(ModeBits::DISCHG),
                    mode_bits_reg.contains(ModeBits::FSWDBL),
                    vout_sr,
                    vout_sr_bits.contains(VoutSrBits::OCP_DELAY0),
                    vout_sr_bits.contains(VoutSrBits::OCP_DELAY1),
                    vout_sr_bits.contains(VoutSrBits::SR0),
                    vout_sr_bits.contains(VoutSrBits::SR1),
                    vout_fs,
                    vout_fs_bits.contains(VoutFsBits::FB_EXT),
                    cdc,
                    cdc_bits.contains(CdcBits::SC_MASK),
                    cdc_bits.contains(CdcBits::OCP_MASK),
                    cdc_bits.contains(CdcBits::OVP_MASK),
                    iout_limit,
                );

                // Write-1-to-clear latched faults and observe whether they re-assert.
                let clear =
                    status.bits() & (StatusBits::SCP | StatusBits::OCP | StatusBits::OVP).bits();
                if clear != 0 {
                    let before = status.bits();
                    let _ = tps.write_reg(tps_addr::STATUS, clear);
                    let after = tps.read_reg(tps_addr::STATUS);
                    match after {
                        Ok(after) => defmt::warn!(
                            "power: fault_clear ch={} addr=0x{=u8:x} before=0x{=u8:x} wrote=0x{=u8:x} after=0x{=u8:x}",
                            ch.name(),
                            addr,
                            before,
                            clear,
                            after
                        ),
                        Err(e) => defmt::warn!(
                            "power: fault_clear ch={} addr=0x{=u8:x} before=0x{=u8:x} wrote=0x{=u8:x} after=err({})",
                            ch.name(),
                            addr,
                            before,
                            clear,
                            tps_error_kind(e)
                        ),
                    }
                }
            }
            (status, mode, vout_sr, vout_fs, cdc, iout_limit) => defmt::warn!(
                "power: fault ch={} addr=0x{=u8:x} status={:?} mode={:?} vout_sr={:?} vout_fs={:?} cdc={:?} iout_limit={:?}",
                ch.name(),
                addr,
                status.map(|v| v.bits()).map_err(tps_error_kind),
                mode.map_err(tps_error_kind),
                vout_sr.map_err(tps_error_kind),
                vout_fs.map_err(tps_error_kind),
                cdc.map_err(tps_error_kind),
                iout_limit.map_err(tps_error_kind),
            ),
        }
    }

    if should_log_ina {
        let bus = ina3221::read_bus_mv(&mut *i2c, ch.ina_ch());
        let shunt = ina3221::read_shunt_uv(&mut *i2c, ch.ina_ch());
        match (bus, shunt) {
            (Ok(vbus_mv), Ok(shunt_uv)) => defmt::warn!(
                "power: fault_ina ch={} vbus_mv={} shunt_uv={} current_ma={}",
                ch.name(),
                vbus_mv,
                shunt_uv,
                ina3221::shunt_uv_to_current_ma(shunt_uv, 10)
            ),
            (bus, shunt) => defmt::warn!(
                "power: fault_ina ch={} vbus_mv={=?} shunt_uv={=?}",
                ch.name(),
                bus.map_err(ina_error_kind),
                shunt.map_err(ina_error_kind)
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TelemetryCapture {
    pub comm_ok: bool,
    pub fault_active: bool,
    pub output_enabled: Option<bool>,
    pub vbus_mv: Option<u16>,
    pub current_ma: Option<i32>,
    pub temp_c_x16: Option<i16>,
}

pub fn print_telemetry_line<I2C>(
    i2c: &mut I2C,
    ch: OutputChannel,
    ina_ready: bool,
    therm_kill_n: u8,
) -> TelemetryCapture
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let addr = ch.addr();
    let (vset_mv, mode, status, vout_sr, cdc, iout_limit) = {
        let mut tps = ::tps55288::Tps55288::with_address(&mut *i2c, addr);
        let vset_mv = match tps.get_vout_mv() {
            Ok(v) => TelemetryValue::Value(v as i32),
            Err(e) => TelemetryValue::Err(tps_error_kind(e)),
        };
        let mode = match tps.read_reg(tps_addr::MODE) {
            Ok(v) => TelemetryU8::Value(v),
            Err(e) => TelemetryU8::Err(tps_error_kind(e)),
        };
        let status = match tps.read_reg(tps_addr::STATUS) {
            Ok(v) => TelemetryU8::Value(v),
            Err(e) => TelemetryU8::Err(tps_error_kind(e)),
        };
        let vout_sr = match tps.read_reg(tps_addr::VOUT_SR) {
            Ok(v) => TelemetryU8::Value(v),
            Err(e) => TelemetryU8::Err(tps_error_kind(e)),
        };
        let cdc = match tps.read_reg(tps_addr::CDC) {
            Ok(v) => TelemetryU8::Value(v),
            Err(e) => TelemetryU8::Err(tps_error_kind(e)),
        };
        let iout_limit = match tps.read_reg(tps_addr::IOUT_LIMIT) {
            Ok(v) => TelemetryU8::Value(v),
            Err(e) => TelemetryU8::Err(tps_error_kind(e)),
        };
        (vset_mv, mode, status, vout_sr, cdc, iout_limit)
    };

    let (oe, fpwm, scp, ocp, ovp) = {
        let oe = match mode {
            TelemetryU8::Value(v) => {
                TelemetryBool::Value(ModeBits::from_bits_truncate(v).contains(ModeBits::OE))
            }
            TelemetryU8::Err(e) => TelemetryBool::Err(e),
        };

        let fpwm = match mode {
            TelemetryU8::Value(v) => {
                TelemetryBool::Value(ModeBits::from_bits_truncate(v).contains(ModeBits::PFM))
            }
            TelemetryU8::Err(e) => TelemetryBool::Err(e),
        };

        let (scp, ocp, ovp) = match status {
            TelemetryU8::Value(v) => {
                let bits = StatusBits::from_bits_truncate(v);
                (
                    TelemetryBool::Value(bits.contains(StatusBits::SCP)),
                    TelemetryBool::Value(bits.contains(StatusBits::OCP)),
                    TelemetryBool::Value(bits.contains(StatusBits::OVP)),
                )
            }
            TelemetryU8::Err(e) => (
                TelemetryBool::Err(e),
                TelemetryBool::Err(e),
                TelemetryBool::Err(e),
            ),
        };

        (oe, fpwm, scp, ocp, ovp)
    };

    let (vbus_mv, vbus_reg, shunt_uv, current_ma) = if ina_ready {
        let bus = ina3221::read_bus_mv(i2c, ch.ina_ch());
        let shunt = ina3221::read_shunt_uv(i2c, ch.ina_ch());
        let vbus_reg = match read_vbus_reg_u16(i2c, ch.ina_ch()) {
            Ok(v) => TelemetryU16::Value(v),
            Err(e) => TelemetryU16::Err(ina_error_kind(e)),
        };

        let vbus_mv = match bus {
            Ok(v) => TelemetryValue::Value(v),
            Err(e) => TelemetryValue::Err(ina_error_kind(e)),
        };

        let (shunt_uv, current_ma) = match shunt {
            Ok(shunt_uv) => (
                TelemetryValue::Value(shunt_uv),
                TelemetryValue::Value(ina3221::shunt_uv_to_current_ma(shunt_uv, 10)),
            ),
            Err(e) => {
                let kind = ina_error_kind(e);
                (TelemetryValue::Err(kind), TelemetryValue::Err(kind))
            }
        };

        (vbus_mv, vbus_reg, shunt_uv, current_ma)
    } else {
        (
            TelemetryValue::Err("ina_uninit"),
            TelemetryU16::Err("ina_uninit"),
            TelemetryValue::Err("ina_uninit"),
            TelemetryValue::Err("ina_uninit"),
        )
    };

    let dv_mv = match (vbus_mv, vset_mv) {
        (TelemetryValue::Value(vbus), TelemetryValue::Value(vset)) => {
            TelemetryValue::Value(vbus - vset)
        }
        (TelemetryValue::Err(e), _) => TelemetryValue::Err(e),
        (_, TelemetryValue::Err(e)) => TelemetryValue::Err(e),
    };

    // Keep the first fields stable per docs/plan/0005:tps55288-control/contracts/cli.md.
    // Extra fields are appended for bring-up/debugging.
    let tmp_addr = ch.tmp_addr();
    let tmp_addr_value = TelemetryU8::Value(tmp_addr);
    let (temp_c_x16, temp_c) = match tmp112::read_temp_c_x16(&mut *i2c, tmp_addr) {
        Ok(v) => (
            TelemetryValue::Value(v as i32),
            TelemetryTempC::Value(v as i32),
        ),
        Err(e) => {
            let kind = i2c_error_kind(e);
            (TelemetryValue::Err(kind), TelemetryTempC::Err(kind))
        }
    };
    match ch {
        OutputChannel::OutA => defmt::info!(
            "telemetry ch=out_a addr=0x74 vset_mv={} vbus_mv={} current_ma={} dv_mv={} vbus_reg={} shunt_uv={} oe={} fpwm={} status={} scp={} ocp={} ovp={} vout_sr={} cdc={} iout_limit={} tmp_addr={} temp_c_x16={} therm_kill_n={=u8} temp_c={}",
            vset_mv, vbus_mv, current_ma, dv_mv, vbus_reg, shunt_uv, oe, fpwm, status, scp, ocp, ovp, vout_sr, cdc, iout_limit, tmp_addr_value, temp_c_x16, therm_kill_n, temp_c
        ),
        OutputChannel::OutB => defmt::info!(
            "telemetry ch=out_b addr=0x75 vset_mv={} vbus_mv={} current_ma={} dv_mv={} vbus_reg={} shunt_uv={} oe={} fpwm={} status={} scp={} ocp={} ovp={} vout_sr={} cdc={} iout_limit={} tmp_addr={} temp_c_x16={} therm_kill_n={=u8} temp_c={}",
            vset_mv, vbus_mv, current_ma, dv_mv, vbus_reg, shunt_uv, oe, fpwm, status, scp, ocp, ovp, vout_sr, cdc, iout_limit, tmp_addr_value, temp_c_x16, therm_kill_n, temp_c
        ),
    }

    let comm_ok = matches!(mode, TelemetryU8::Value(_)) && matches!(status, TelemetryU8::Value(_));
    let fault_active = matches!(scp, TelemetryBool::Value(true))
        || matches!(ocp, TelemetryBool::Value(true))
        || matches!(ovp, TelemetryBool::Value(true));
    let output_enabled = match oe {
        TelemetryBool::Value(v) => Some(v),
        TelemetryBool::Err(_) => None,
    };
    let current_ma = match current_ma {
        TelemetryValue::Value(v) => Some(v),
        TelemetryValue::Err(_) => None,
    };
    let vbus_mv = match vbus_mv {
        TelemetryValue::Value(v) => Some(v as u16),
        TelemetryValue::Err(_) => None,
    };
    let temp_c_x16 = match temp_c_x16 {
        TelemetryValue::Value(v) => Some(v as i16),
        TelemetryValue::Err(_) => None,
    };

    TelemetryCapture {
        comm_ok,
        fault_active,
        output_enabled,
        vbus_mv,
        current_ma,
        temp_c_x16,
    }
}

fn read_vbus_reg_u16<I2C>(
    i2c: &mut I2C,
    ch: ina3221::Channel,
) -> Result<u16, ina3221::Error<esp_hal::i2c::master::Error>>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    use ina3221::registers::addr;
    let reg = match ch {
        ina3221::Channel::Ch1 => addr::CH1_BUS,
        ina3221::Channel::Ch2 => addr::CH2_BUS,
        ina3221::Channel::Ch3 => addr::CH3_BUS,
    };

    let mut dev = ina3221::Ina3221::new(&mut *i2c);
    dev.read_reg_u16_be(reg)
}

pub fn should_log_fault(now: Instant, last: &mut Option<Instant>, min_interval: Duration) -> bool {
    let allow = match *last {
        None => true,
        Some(t) => (now - t) >= min_interval,
    };
    if allow {
        *last = Some(now);
    }
    allow
}
