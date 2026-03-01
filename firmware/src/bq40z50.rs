//! Minimal BQ40Z50-R2 helpers (bring-up oriented).
//!
//! This module intentionally stays small:
//! - SBS command ids we need for observability
//! - raw SMBus "word" read helpers
//! - a few bit definitions to decode BatteryStatus()
//!
//! References:
//! - TRM: `docs/manuals/BQ40Z50-R2-TRM/BQ40Z50-R2-TRM.md`
//! - I2C map: `docs/i2c-address-map.md`

/// Default (project) 7-bit SMBus address for BQ40Z50 (per `docs/i2c-address-map.md`).
pub const I2C_ADDRESS_PRIMARY: u8 = 0x0B;

/// Default (TI TRM data flash) 7-bit SMBus address for BQ40Z50.
///
/// TRM note: the SMBus address is configurable via data flash and falls back to 0x16 if the
/// programmed values are invalid.
pub const I2C_ADDRESS_FALLBACK: u8 = 0x16;

pub const I2C_ADDRESS_CANDIDATES: [u8; 2] = [I2C_ADDRESS_PRIMARY, I2C_ADDRESS_FALLBACK];

pub mod cmd {
    pub const TEMPERATURE: u8 = 0x08;
    pub const VOLTAGE: u8 = 0x09;
    pub const CURRENT: u8 = 0x0A;
    pub const RELATIVE_STATE_OF_CHARGE: u8 = 0x0D;
    pub const REMAINING_CAPACITY: u8 = 0x0F;
    pub const FULL_CHARGE_CAPACITY: u8 = 0x10;
    pub const BATTERY_STATUS: u8 = 0x16;
    pub const OPERATION_STATUS: u8 = 0x54;

    pub const CELL_VOLTAGE_4: u8 = 0x3C;
    pub const CELL_VOLTAGE_3: u8 = 0x3D;
    pub const CELL_VOLTAGE_2: u8 = 0x3E;
    pub const CELL_VOLTAGE_1: u8 = 0x3F;
}

pub mod operation_status {
    pub const SLEEP: u16 = 1 << 15;
    pub const XCHG: u16 = 1 << 14;
    pub const XDSG: u16 = 1 << 13;
    pub const PF: u16 = 1 << 12;
    pub const PCHG: u16 = 1 << 3;
    pub const CHG: u16 = 1 << 2;
    pub const DSG: u16 = 1 << 1;
    pub const PRES: u16 = 1 << 0;
}

pub mod battery_status {
    pub const OCA: u16 = 1 << 15;
    pub const TCA: u16 = 1 << 14;
    pub const OTA: u16 = 1 << 12;
    pub const TDA: u16 = 1 << 11;
    pub const RCA: u16 = 1 << 9;
    pub const RTA: u16 = 1 << 8;

    pub const INIT: u16 = 1 << 7;
    pub const DSG: u16 = 1 << 6;
    pub const FC: u16 = 1 << 5;
    pub const FD: u16 = 1 << 4;

    pub const fn error_code(raw: u16) -> u8 {
        (raw & 0x0F) as u8
    }
}

pub const fn decode_error_code(code: u8) -> &'static str {
    match code & 0x0F {
        0x0 => "ok",
        0x1 => "busy",
        0x2 => "reserved_cmd",
        0x3 => "unsupported_cmd",
        0x4 => "access_denied",
        0x5 => "overflow_underflow",
        0x6 => "bad_size",
        _ => "unknown",
    }
}

pub fn read_u16<I2C>(i2c: &mut I2C, addr: u8, sbscmd: u8) -> Result<u16, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    let mut buf = [0u8; 2];
    i2c.write_read(addr, &[sbscmd], &mut buf)?;
    Ok(u16::from_le_bytes(buf))
}

pub fn read_i16<I2C>(i2c: &mut I2C, addr: u8, sbscmd: u8) -> Result<i16, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    let mut buf = [0u8; 2];
    i2c.write_read(addr, &[sbscmd], &mut buf)?;
    Ok(i16::from_le_bytes(buf))
}

/// Convert Temperature() units (0.1 K) to 0.1 C (i.e., C * 10).
pub const fn temp_c_x10_from_k_x10(temp_k_x10: u16) -> i32 {
    temp_k_x10 as i32 - 2731
}
