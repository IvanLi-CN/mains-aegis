//! Register map and conversion helpers for INA3221.

/// Default I2C address (7-bit).
pub const DEFAULT_I2C_ADDRESS: u8 = 0x40;

/// Register addresses (8-bit).
pub mod addr {
    pub const CONFIG: u8 = 0x00;

    pub const CH1_SHUNT: u8 = 0x01;
    pub const CH1_BUS: u8 = 0x02;
    pub const CH2_SHUNT: u8 = 0x03;
    pub const CH2_BUS: u8 = 0x04;
    pub const CH3_SHUNT: u8 = 0x05;
    pub const CH3_BUS: u8 = 0x06;

    pub const MANUFACTURER_ID: u8 = 0xFE;
    pub const DIE_ID: u8 = 0xFF;
}

/// Frozen by docs/plan/0005:tps55288-control/contracts/config.md.
pub const CONFIG_VALUE_CH12: u16 = 0x6527;
pub const CONFIG_VALUE_CH123: u16 = 0x7527;

/// A more noise-tolerant continuous mode configuration.
///
/// - CH1+CH2 enabled, CH3 disabled
/// - AVG = 1024
/// - VBUSCT = 8.244ms
/// - VSHCT = 8.244ms
/// - MODE = shunt+bus continuous
pub const CONFIG_VALUE_CH12_STABLE: u16 = 0x6FFF;

/// Same as [`CONFIG_VALUE_CH12_STABLE`], but with CH3 enabled.
pub const CONFIG_VALUE_CH123_STABLE: u16 = 0x7FFF;

/// Shunt voltage LSB (after right-shift by 3) in microvolts.
pub const VSHUNT_LSB_UV: i32 = 40;
/// Bus voltage LSB (after right-shift by 3) in millivolts.
pub const VBUS_LSB_MV: i32 = 8;

pub fn decode_shift3_signed_i16(be: [u8; 2]) -> i16 {
    let word = i16::from_be_bytes(be);
    word >> 3
}
