use esp_firmware::ina3221;
use esp_firmware::tmp112;

use ::tps55288::data_types::{
    CableCompLevel, CableCompOption, FeedbackSource, I2cAddress, InternalFeedbackRatio,
    LightLoadMode, LightLoadOverride, OcpDelay, VccSource, VoutSlewRate,
};
use ::tps55288::registers::{addr as tps_addr, ModeBits, StatusBits};

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

    pub const fn addr_enum(self) -> I2cAddress {
        match self {
            Self::OutA => I2cAddress::Addr0x74,
            Self::OutB => I2cAddress::Addr0x75,
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
    CdcPostEnable,
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
            Self::CdcPostEnable => "cdc_post_enable",
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
    tps.set_mode_control(
        LightLoadOverride::FromRegister,
        VccSource::External5v,
        ch.addr_enum(),
        LightLoadMode::Pfm,
    )
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
        tps.set_cable_comp(
            CableCompOption::Internal,
            CableCompLevel::V0p0,
            true,
            true,
            true,
        )
        .map_err(|e| ConfigureFailure {
            stage: ConfigureStage::CdcPostEnable,
            kind: tps_error_kind(&e),
            retryable: matches!(e, ::tps55288::Error::I2c(_)),
        })?;
    }

    Ok(())
}

pub fn force_disable_output<I2C>(i2c: &mut I2C, ch: OutputChannel) -> Result<(), &'static str>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let mut tps = ::tps55288::Tps55288::with_address(&mut *i2c, ch.addr());
    tps.disable_output().map_err(|err| tps_error_kind(&err))
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
