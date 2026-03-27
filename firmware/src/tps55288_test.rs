use esp_firmware::ina3221;
use esp_firmware::tmp112;

use ::tps55288::data_types::{
    CableCompLevel, CableCompOption, FeedbackSource, InternalFeedbackRatio, OcpDelay, VoutSlewRate,
};
use ::tps55288::registers::{addr as tps_addr, CdcBits, ModeBits, StatusBits};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputChannel {
    OutA,
    OutB,
}

impl OutputChannel {
    pub const fn addr(self) -> u8 {
        match self {
            Self::OutA => 0x74,
            Self::OutB => 0x75,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::OutA => "out_a",
            Self::OutB => "out_b",
        }
    }

    pub const fn ina_ch(self) -> ina3221::Channel {
        match self {
            Self::OutA => ina3221::Channel::Ch2,
            Self::OutB => ina3221::Channel::Ch1,
        }
    }

    pub const fn tmp_addr(self) -> u8 {
        match self {
            Self::OutA => 0x48,
            Self::OutB => 0x49,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestSwitchingMode {
    Fpwm,
    Pfm,
}

impl TestSwitchingMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Fpwm => "FPWM",
            Self::Pfm => "PFM",
        }
    }
}

fn configure_mode_register<I2C>(
    tps: &mut ::tps55288::Tps55288<I2C>,
    ch: OutputChannel,
    switching_mode: TestSwitchingMode,
) -> Result<(), ::tps55288::Error<esp_hal::i2c::master::Error>>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let mut mode = ModeBits::from_bits_truncate(tps.read_reg(tps_addr::MODE)?);
    mode.insert(ModeBits::MODE | ModeBits::VCC_EXT);
    match switching_mode {
        TestSwitchingMode::Fpwm => mode.insert(ModeBits::PFM),
        TestSwitchingMode::Pfm => mode.remove(ModeBits::PFM),
    }
    match ch {
        OutputChannel::OutA => mode.remove(ModeBits::I2CADD),
        OutputChannel::OutB => mode.insert(ModeBits::I2CADD),
    }
    tps.write_reg(tps_addr::MODE, mode.bits())
}

fn fpwm_enabled_from_mode(mode: u8) -> bool {
    ModeBits::from_bits_truncate(mode).contains(ModeBits::PFM)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
            Self::Disable => "disable",
            Self::Init => "init",
            Self::Mode => "mode",
            Self::VoutSr => "vout_sr",
            Self::Feedback => "feedback",
            Self::Cdc => "cdc",
            Self::Vout => "vout",
            Self::Ilim => "ilim",
            Self::Enable => "enable",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfigureFailure {
    pub stage: ConfigureStage,
    pub kind: &'static str,
    pub retryable: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TpsTelemetrySnapshot {
    pub output_enabled: Option<bool>,
    pub vset_mv: Option<u16>,
    pub vbus_mv: Option<u16>,
    pub current_ma: Option<i32>,
    pub temp_c_x16: Option<i16>,
    pub status: Option<u8>,
    pub scp: bool,
    pub ocp: bool,
    pub ovp: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TpsDiagSnapshot {
    pub mode: u8,
    pub status: u8,
    pub vout_sr: Option<u8>,
    pub cdc: Option<u8>,
    pub iout_limit: Option<u8>,
    pub output_enabled: bool,
    pub dischg_enabled: bool,
    pub fpwm_enabled: bool,
    pub register_mode: bool,
    pub external_vcc: bool,
    pub ilim_enabled: Option<bool>,
    pub ilim_ma: Option<u16>,
    pub vset_mv: Option<u16>,
    pub vbus_mv: Option<u16>,
    pub current_ma: Option<i32>,
    pub temp_c_x16: Option<i16>,
    pub scp: bool,
    pub ocp: bool,
    pub ovp: bool,
    pub sc_mask: Option<bool>,
    pub ocp_mask: Option<bool>,
    pub ovp_mask: Option<bool>,
}

pub fn i2c_error_kind(err: esp_hal::i2c::master::Error) -> &'static str {
    use esp_hal::i2c::master::Error;

    match err {
        Error::Timeout => "i2c_timeout",
        Error::AcknowledgeCheckFailed(_) => "i2c_nack",
        Error::ArbitrationLost => "i2c_arbitration",
        _ => "i2c",
    }
}

pub fn tps_error_kind(err: &::tps55288::Error<esp_hal::i2c::master::Error>) -> &'static str {
    match err {
        ::tps55288::Error::I2c(e) => i2c_error_kind(*e),
        ::tps55288::Error::OutOfRange => "out_of_range",
        ::tps55288::Error::InvalidConfig => "invalid_config",
    }
}

pub fn ina_error_kind(err: ina3221::Error<esp_hal::i2c::master::Error>) -> &'static str {
    match err {
        ina3221::Error::I2c(e) => i2c_error_kind(e),
        ina3221::Error::OutOfRange => "out_of_range",
        ina3221::Error::InvalidConfig => "invalid_config",
    }
}

pub fn configure_output<I2C>(
    i2c: &mut I2C,
    ch: OutputChannel,
    enabled: bool,
    target_vout_mv: u16,
    ilimit_ma: u16,
    switching_mode: TestSwitchingMode,
) -> Result<(), ConfigureFailure>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let mut tps = ::tps55288::Tps55288::with_address(&mut *i2c, ch.addr());

    tps.disable_output().map_err(|e| ConfigureFailure {
        stage: ConfigureStage::Disable,
        kind: tps_error_kind(&e),
        retryable: matches!(e, ::tps55288::Error::I2c(_)),
    })?;
    tps.init().map_err(|e| ConfigureFailure {
        stage: ConfigureStage::Init,
        kind: tps_error_kind(&e),
        retryable: matches!(e, ::tps55288::Error::I2c(_)),
    })?;
    configure_mode_register(&mut tps, ch, switching_mode).map_err(|e| ConfigureFailure {
        stage: ConfigureStage::Mode,
        kind: tps_error_kind(&e),
        retryable: matches!(e, ::tps55288::Error::I2c(_)),
    })?;
    tps.set_vout_sr(VoutSlewRate::Sr1p25MvPerUs, OcpDelay::Ms12_288)
        .map_err(|e| ConfigureFailure {
            stage: ConfigureStage::VoutSr,
            kind: tps_error_kind(&e),
            retryable: matches!(e, ::tps55288::Error::I2c(_)),
        })?;
    tps.set_feedback(FeedbackSource::Internal, InternalFeedbackRatio::R0_0564)
        .map_err(|e| ConfigureFailure {
            stage: ConfigureStage::Feedback,
            kind: tps_error_kind(&e),
            retryable: matches!(e, ::tps55288::Error::I2c(_)),
        })?;
    tps.set_cable_comp(
        CableCompOption::Internal,
        CableCompLevel::V0p0,
        true,
        true,
        true,
    )
    .map_err(|e| ConfigureFailure {
        stage: ConfigureStage::Cdc,
        kind: tps_error_kind(&e),
        retryable: matches!(e, ::tps55288::Error::I2c(_)),
    })?;
    tps.set_vout_mv(target_vout_mv)
        .map_err(|e| ConfigureFailure {
            stage: ConfigureStage::Vout,
            kind: tps_error_kind(&e),
            retryable: matches!(e, ::tps55288::Error::I2c(_)),
        })?;
    tps.set_ilim_ma(ilimit_ma, enabled)
        .map_err(|e| ConfigureFailure {
            stage: ConfigureStage::Ilim,
            kind: tps_error_kind(&e),
            retryable: matches!(e, ::tps55288::Error::I2c(_)),
        })?;

    if enabled {
        tps.enable_output().map_err(|e| ConfigureFailure {
            stage: ConfigureStage::Enable,
            kind: tps_error_kind(&e),
            retryable: matches!(e, ::tps55288::Error::I2c(_)),
        })?;
    }

    Ok(())
}

pub fn apply_minimal_output<I2C>(
    i2c: &mut I2C,
    ch: OutputChannel,
    enabled: bool,
    target_vout_mv: u16,
    ilimit_ma: u16,
    switching_mode: TestSwitchingMode,
) -> Result<(), ConfigureFailure>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let mut tps = ::tps55288::Tps55288::with_address(&mut *i2c, ch.addr());

    log_minimal_raw_regs(&mut tps, ch, "pre");
    if !enabled {
        tps.disable_output().map_err(|e| ConfigureFailure {
            stage: ConfigureStage::Disable,
            kind: tps_error_kind(&e),
            retryable: matches!(e, ::tps55288::Error::I2c(_)),
        })?;
        log_minimal_raw_regs(&mut tps, ch, "post_disable");
    }

    configure_mode_register(&mut tps, ch, switching_mode).map_err(|e| ConfigureFailure {
        stage: ConfigureStage::Mode,
        kind: tps_error_kind(&e),
        retryable: matches!(e, ::tps55288::Error::I2c(_)),
    })?;
    log_minimal_raw_regs(&mut tps, ch, "post_mode");
    tps.set_cable_comp(
        CableCompOption::Internal,
        CableCompLevel::V0p0,
        false,
        false,
        false,
    )
    .map_err(|e| ConfigureFailure {
        stage: ConfigureStage::Cdc,
        kind: tps_error_kind(&e),
        retryable: matches!(e, ::tps55288::Error::I2c(_)),
    })?;
    log_minimal_raw_regs(&mut tps, ch, "post_cdc");
    tps.set_ilim_ma(ilimit_ma, true)
        .map_err(|e| ConfigureFailure {
            stage: ConfigureStage::Ilim,
            kind: tps_error_kind(&e),
            retryable: matches!(e, ::tps55288::Error::I2c(_)),
        })?;
    log_minimal_raw_regs(&mut tps, ch, "post_ilim");
    tps.set_vout_mv(target_vout_mv)
        .map_err(|e| ConfigureFailure {
            stage: ConfigureStage::Vout,
            kind: tps_error_kind(&e),
            retryable: matches!(e, ::tps55288::Error::I2c(_)),
        })?;
    log_minimal_raw_regs(&mut tps, ch, "post_vout");

    if enabled {
        raw_enable_output_probe(&mut tps, ch).map_err(|e| ConfigureFailure {
            stage: ConfigureStage::Enable,
            kind: tps_error_kind(&e),
            retryable: matches!(e, ::tps55288::Error::I2c(_)),
        })?;
        log_minimal_raw_regs(&mut tps, ch, "post_oe");
    }

    Ok(())
}

fn raw_enable_output_probe<I2C>(
    tps: &mut ::tps55288::Tps55288<I2C>,
    ch: OutputChannel,
) -> Result<(), ::tps55288::Error<esp_hal::i2c::master::Error>>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let pre = tps.read_reg(tps_addr::MODE)?;
    let target = pre | ModeBits::OE.bits();
    tps.write_reg(tps_addr::MODE, target)?;
    let rb0 = tps.read_reg(tps_addr::MODE)?;
    let rb1 = tps.read_reg(tps_addr::MODE)?;
    let rb2 = tps.read_reg(tps_addr::MODE)?;
    defmt::info!(
        "tps-test: mode_probe ch={} pre=0x{:02x} target=0x{:02x} rb0=0x{:02x} rb1=0x{:02x} rb2=0x{:02x}",
        ch.name(),
        pre,
        target,
        rb0,
        rb1,
        rb2,
    );
    Ok(())
}

fn log_minimal_raw_regs<I2C>(
    tps: &mut ::tps55288::Tps55288<I2C>,
    ch: OutputChannel,
    stage: &'static str,
) where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let mode = tps.read_reg(tps_addr::MODE);
    let vout_sr = tps.read_reg(tps_addr::VOUT_SR);
    let vout_fs = tps.read_reg(tps_addr::VOUT_FS);
    let cdc = tps.read_reg(tps_addr::CDC);
    let iout_limit = tps.read_reg(tps_addr::IOUT_LIMIT);
    let ref0 = tps.read_reg(tps_addr::REF0);
    let ref1 = tps.read_reg(tps_addr::REF1);

    defmt::info!(
        "tps-test: minimal_raw ch={} stage={} mode={=?} vout_sr={=?} vout_fs={=?} cdc={=?} iout_limit={=?} ref0={=?} ref1={=?}",
        ch.name(),
        stage,
        mode.map_err(|err| tps_error_kind(&err)),
        vout_sr.map_err(|err| tps_error_kind(&err)),
        vout_fs.map_err(|err| tps_error_kind(&err)),
        cdc.map_err(|err| tps_error_kind(&err)),
        iout_limit.map_err(|err| tps_error_kind(&err)),
        ref0.map_err(|err| tps_error_kind(&err)),
        ref1.map_err(|err| tps_error_kind(&err)),
    );
}

pub fn force_disable_output<I2C>(i2c: &mut I2C, ch: OutputChannel) -> Result<(), &'static str>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let mut tps = ::tps55288::Tps55288::with_address(&mut *i2c, ch.addr());
    tps.disable_output().map_err(|err| tps_error_kind(&err))
}

pub fn read_status_snapshot<I2C>(i2c: &mut I2C, ch: OutputChannel) -> Result<u8, &'static str>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let mut tps = ::tps55288::Tps55288::with_address(&mut *i2c, ch.addr());
    tps.read_reg(tps_addr::STATUS)
        .map_err(|err| tps_error_kind(&err))
}

pub fn configure_output_disabled<I2C>(
    i2c: &mut I2C,
    ch: OutputChannel,
    target_vout_mv: u16,
    ilimit_ma: u16,
    switching_mode: TestSwitchingMode,
) -> Result<(), ConfigureFailure>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let mut tps = ::tps55288::Tps55288::with_address(&mut *i2c, ch.addr());

    let mut mode = ModeBits::from_bits_truncate(tps.read_reg(tps_addr::MODE).map_err(|e| {
        ConfigureFailure {
            stage: ConfigureStage::Disable,
            kind: tps_error_kind(&e),
            retryable: matches!(e, ::tps55288::Error::I2c(_)),
        }
    })?);
    mode.insert(ModeBits::MODE | ModeBits::VCC_EXT);
    mode.remove(ModeBits::OE | ModeBits::DISCHG | ModeBits::PFM);
    match ch {
        OutputChannel::OutA => mode.remove(ModeBits::I2CADD),
        OutputChannel::OutB => mode.insert(ModeBits::I2CADD),
    }
    tps.write_reg(tps_addr::MODE, mode.bits())
        .map_err(|e| ConfigureFailure {
            stage: ConfigureStage::Disable,
            kind: tps_error_kind(&e),
            retryable: matches!(e, ::tps55288::Error::I2c(_)),
        })?;

    tps.init().map_err(|e| ConfigureFailure {
        stage: ConfigureStage::Init,
        kind: tps_error_kind(&e),
        retryable: matches!(e, ::tps55288::Error::I2c(_)),
    })?;
    configure_mode_register(&mut tps, ch, switching_mode).map_err(|e| ConfigureFailure {
        stage: ConfigureStage::Mode,
        kind: tps_error_kind(&e),
        retryable: matches!(e, ::tps55288::Error::I2c(_)),
    })?;

    let mut mode = ModeBits::from_bits_truncate(tps.read_reg(tps_addr::MODE).map_err(|e| {
        ConfigureFailure {
            stage: ConfigureStage::Mode,
            kind: tps_error_kind(&e),
            retryable: matches!(e, ::tps55288::Error::I2c(_)),
        }
    })?);
    mode.remove(ModeBits::OE | ModeBits::DISCHG);
    tps.write_reg(tps_addr::MODE, mode.bits())
        .map_err(|e| ConfigureFailure {
            stage: ConfigureStage::Mode,
            kind: tps_error_kind(&e),
            retryable: matches!(e, ::tps55288::Error::I2c(_)),
        })?;

    tps.set_vout_sr(VoutSlewRate::Sr1p25MvPerUs, OcpDelay::Ms12_288)
        .map_err(|e| ConfigureFailure {
            stage: ConfigureStage::VoutSr,
            kind: tps_error_kind(&e),
            retryable: matches!(e, ::tps55288::Error::I2c(_)),
        })?;
    tps.set_feedback(FeedbackSource::Internal, InternalFeedbackRatio::R0_0564)
        .map_err(|e| ConfigureFailure {
            stage: ConfigureStage::Feedback,
            kind: tps_error_kind(&e),
            retryable: matches!(e, ::tps55288::Error::I2c(_)),
        })?;
    tps.set_cable_comp(
        CableCompOption::Internal,
        CableCompLevel::V0p0,
        false,
        false,
        false,
    )
    .map_err(|e| ConfigureFailure {
        stage: ConfigureStage::Cdc,
        kind: tps_error_kind(&e),
        retryable: matches!(e, ::tps55288::Error::I2c(_)),
    })?;
    tps.set_vout_mv(target_vout_mv)
        .map_err(|e| ConfigureFailure {
            stage: ConfigureStage::Vout,
            kind: tps_error_kind(&e),
            retryable: matches!(e, ::tps55288::Error::I2c(_)),
        })?;
    tps.set_ilim_ma(ilimit_ma, false)
        .map_err(|e| ConfigureFailure {
            stage: ConfigureStage::Ilim,
            kind: tps_error_kind(&e),
            retryable: matches!(e, ::tps55288::Error::I2c(_)),
        })?;

    Ok(())
}

pub fn read_telemetry_snapshot<I2C>(
    i2c: &mut I2C,
    ch: OutputChannel,
    ina_ready: bool,
) -> Result<TpsTelemetrySnapshot, &'static str>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let mut tps = ::tps55288::Tps55288::with_address(&mut *i2c, ch.addr());
    let mode = tps
        .read_reg(tps_addr::MODE)
        .map_err(|err| tps_error_kind(&err))?;
    let status = tps
        .read_reg(tps_addr::STATUS)
        .map_err(|err| tps_error_kind(&err))?;
    let vset_mv = tps.get_vout_mv().ok();

    let status_bits = StatusBits::from_bits_truncate(status);
    let output_enabled = Some(ModeBits::from_bits_truncate(mode).contains(ModeBits::OE));

    let (vbus_mv, current_ma) = if ina_ready {
        let vbus_mv = ina3221::read_bus_mv(i2c, ch.ina_ch())
            .ok()
            .and_then(|mv| u16::try_from(mv).ok());
        let current_ma = ina3221::read_shunt_uv(i2c, ch.ina_ch())
            .ok()
            .map(|shunt_uv| ina3221::shunt_uv_to_current_ma(shunt_uv, 10));
        (vbus_mv, current_ma)
    } else {
        (None, None)
    };

    let temp_c_x16 = tmp112::read_temp_c_x16(i2c, ch.tmp_addr()).ok();

    Ok(TpsTelemetrySnapshot {
        output_enabled,
        vset_mv,
        vbus_mv,
        current_ma,
        temp_c_x16,
        status: Some(status),
        scp: status_bits.contains(StatusBits::SCP),
        ocp: status_bits.contains(StatusBits::OCP),
        ovp: status_bits.contains(StatusBits::OVP),
    })
}

pub fn read_diag_snapshot<I2C>(
    i2c: &mut I2C,
    ch: OutputChannel,
    ina_ready: bool,
) -> Result<TpsDiagSnapshot, &'static str>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let mut tps = ::tps55288::Tps55288::with_address(&mut *i2c, ch.addr());
    let mode = tps
        .read_reg(tps_addr::MODE)
        .map_err(|err| tps_error_kind(&err))?;
    let status = tps
        .read_reg(tps_addr::STATUS)
        .map_err(|err| tps_error_kind(&err))?;

    let mode_bits = ModeBits::from_bits_truncate(mode);
    let status_bits = StatusBits::from_bits_truncate(status);
    let vout_sr = tps.read_reg(tps_addr::VOUT_SR).ok();
    let cdc = tps.read_reg(tps_addr::CDC).ok();
    let iout_limit = tps.read_reg(tps_addr::IOUT_LIMIT).ok();
    let ilim = tps.get_ilim_ma().ok();
    let vset_mv = tps.get_vout_mv().ok();

    let (vbus_mv, current_ma) = if ina_ready {
        let vbus_mv = ina3221::read_bus_mv(i2c, ch.ina_ch())
            .ok()
            .and_then(|mv| u16::try_from(mv).ok());
        let current_ma = ina3221::read_shunt_uv(i2c, ch.ina_ch())
            .ok()
            .map(|shunt_uv| ina3221::shunt_uv_to_current_ma(shunt_uv, 10));
        (vbus_mv, current_ma)
    } else {
        (None, None)
    };

    let temp_c_x16 = tmp112::read_temp_c_x16(i2c, ch.tmp_addr()).ok();

    Ok(TpsDiagSnapshot {
        mode,
        status,
        vout_sr,
        cdc,
        iout_limit,
        output_enabled: mode_bits.contains(ModeBits::OE),
        dischg_enabled: mode_bits.contains(ModeBits::DISCHG),
        fpwm_enabled: fpwm_enabled_from_mode(mode),
        register_mode: mode_bits.contains(ModeBits::MODE),
        external_vcc: mode_bits.contains(ModeBits::VCC_EXT),
        ilim_enabled: ilim.map(|(_, enabled)| enabled),
        ilim_ma: ilim.map(|(ma, _)| ma),
        vset_mv,
        vbus_mv,
        current_ma,
        temp_c_x16,
        scp: status_bits.contains(StatusBits::SCP),
        ocp: status_bits.contains(StatusBits::OCP),
        ovp: status_bits.contains(StatusBits::OVP),
        sc_mask: cdc.map(|raw| CdcBits::from_bits_truncate(raw).contains(CdcBits::SC_MASK)),
        ocp_mask: cdc.map(|raw| CdcBits::from_bits_truncate(raw).contains(CdcBits::OCP_MASK)),
        ovp_mask: cdc.map(|raw| CdcBits::from_bits_truncate(raw).contains(CdcBits::OVP_MASK)),
    })
}
