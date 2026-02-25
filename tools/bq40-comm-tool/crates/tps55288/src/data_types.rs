//! Data types for TPS55288 driver (skeleton, based on datasheet).
//! Concrete value mappings will be filled when register bitfields are implemented.

/// I2C slave addresses available via MODE pin presets.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum I2cAddress {
    Addr0x74,
    Addr0x75,
}

/// Light-load operating mode.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LightLoadMode {
    /// Pulse-frequency modulation at light load.
    Pfm,
    /// Forced PWM at light load.
    Pwm,
}

/// VCC source selection.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VccSource {
    /// Internal LDO.
    Internal,
    /// External 5 V supply.
    External5v,
}

/// Light-load operating mode selection (PFM/PWM).
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LightLoadOverride {
    /// Follow external resistor preset (MODE bit0 = 0).
    FromPreset,
    /// Force choice via MODE register (MODE bit0 = 1).
    FromRegister,
}

/// Output slew rate options for VOUT changes.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VoutSlewRate {
    Sr1p25MvPerUs,
    Sr2p5MvPerUs,
    Sr5MvPerUs,
    Sr10MvPerUs,
}

/// Overcurrent response delay selections.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OcpDelay {
    Us128,
    Ms3_072,
    Ms6_144,
    Ms12_288,
}

/// Feedback source selection.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FeedbackSource {
    Internal,
    External,
}

/// Internal feedback ratios (per datasheet INTFB bits).
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InternalFeedbackRatio {
    R0_2256,
    R0_1128,
    R0_0752,
    R0_0564,
}

/// Cable droop compensation mode.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CableCompOption {
    Internal,
    External,
}

/// Cable droop compensation level (CDC[2:0]).
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CableCompLevel {
    V0p0,
    V0p1,
    V0p2,
    V0p3,
    V0p4,
    V0p5,
    V0p6,
    V0p7,
}

/// MODE pin resistor preset entry from datasheet table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModePreset {
    pub resistor_kohm: Option<f32>,
    pub vcc_source: VccSource,
    pub address: I2cAddress,
    pub light_load_mode: LightLoadMode,
}

/// Known presets from datasheet (ohms in kΩ). Values are for documentation; not used programmatically yet.
pub const MODE_PRESETS: [ModePreset; 8] = [
    ModePreset {
        resistor_kohm: Some(0.0),
        vcc_source: VccSource::Internal,
        address: I2cAddress::Addr0x74,
        light_load_mode: LightLoadMode::Pwm,
    },
    ModePreset {
        resistor_kohm: Some(6.19),
        vcc_source: VccSource::Internal,
        address: I2cAddress::Addr0x74,
        light_load_mode: LightLoadMode::Pfm,
    },
    ModePreset {
        resistor_kohm: Some(14.3),
        vcc_source: VccSource::Internal,
        address: I2cAddress::Addr0x75,
        light_load_mode: LightLoadMode::Pwm,
    },
    ModePreset {
        resistor_kohm: Some(24.9),
        vcc_source: VccSource::Internal,
        address: I2cAddress::Addr0x75,
        light_load_mode: LightLoadMode::Pfm,
    },
    ModePreset {
        resistor_kohm: Some(51.1),
        vcc_source: VccSource::External5v,
        address: I2cAddress::Addr0x74,
        light_load_mode: LightLoadMode::Pwm,
    },
    ModePreset {
        resistor_kohm: Some(75.0),
        vcc_source: VccSource::External5v,
        address: I2cAddress::Addr0x74,
        light_load_mode: LightLoadMode::Pfm,
    },
    ModePreset {
        resistor_kohm: Some(105.0),
        vcc_source: VccSource::External5v,
        address: I2cAddress::Addr0x75,
        light_load_mode: LightLoadMode::Pwm,
    },
    ModePreset {
        resistor_kohm: None,
        vcc_source: VccSource::External5v,
        address: I2cAddress::Addr0x75,
        light_load_mode: LightLoadMode::Pfm,
    },
];

/// STATUS decoded operating mode.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperatingStatus {
    Boost,
    Buck,
    BuckBoost,
    Reserved,
}

/// Fault flags decoded from STATUS.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FaultStatus {
    pub short_circuit: bool,
    pub over_current: bool,
    pub over_voltage: bool,
}

/// Placeholder for operating status bits (to be populated from STATUS register details).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatusFlags {
    pub raw: u8,
}

/// Placeholder for fault flags (write-1-to-clear in STATUS).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FaultFlags {
    pub raw: u8,
}

/// VOUT configuration placeholder.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VoutSetting {
    pub millivolts: u16,
}

/// Output current limit configuration placeholder.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CurrentLimitSetting {
    pub milliamps: u16,
}
