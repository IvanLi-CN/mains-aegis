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

pub const I2C_ADDRESS_CANONICAL: [u8; 1] = [I2C_ADDRESS_PRIMARY];
pub const I2C_ADDRESS_DIAG: [u8; 2] = [I2C_ADDRESS_PRIMARY, I2C_ADDRESS_FALLBACK];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BmsAddressMode {
    Canonical0x0B,
    DualProbeDiag,
}

impl BmsAddressMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            BmsAddressMode::Canonical0x0B => "canonical_0x0b",
            BmsAddressMode::DualProbeDiag => "dual_probe_diag",
        }
    }

    pub const fn candidates(self) -> &'static [u8] {
        match self {
            BmsAddressMode::Canonical0x0B => &I2C_ADDRESS_CANONICAL,
            BmsAddressMode::DualProbeDiag => &I2C_ADDRESS_DIAG,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BmsDiagError {
    I2cNack,
    BadBlockLen,
    BadAscii,
    BadRange,
    StalePattern,
    InconsistentSample,
}

impl BmsDiagError {
    pub const fn as_str(self) -> &'static str {
        match self {
            BmsDiagError::I2cNack => "i2c_nack",
            BmsDiagError::BadBlockLen => "bad_len",
            BmsDiagError::BadAscii => "bad_ascii",
            BmsDiagError::BadRange => "bad_range",
            BmsDiagError::StalePattern => "stale_pattern",
            BmsDiagError::InconsistentSample => "inconsistent_sample",
        }
    }
}

impl defmt::Format for BmsDiagError {
    fn format(&self, fmt: defmt::Formatter) {
        defmt::write!(fmt, "{}", self.as_str());
    }
}

#[derive(Clone, Copy)]
pub struct BlockReadRaw {
    pub declared_len: u8,
    pub payload_len: u8,
    pub payload: [u8; 32],
}

pub mod cmd {
    pub const MANUFACTURER_ACCESS: u8 = 0x00;
    pub const TEMPERATURE: u8 = 0x08;
    pub const VOLTAGE: u8 = 0x09;
    pub const CURRENT: u8 = 0x0A;
    pub const RELATIVE_STATE_OF_CHARGE: u8 = 0x0D;
    pub const REMAINING_CAPACITY: u8 = 0x0F;
    pub const FULL_CHARGE_CAPACITY: u8 = 0x10;
    pub const BATTERY_STATUS: u8 = 0x16;
    pub const MANUFACTURER_DATA: u8 = 0x23;
    pub const MANUFACTURER_BLOCK_ACCESS: u8 = 0x44;

    pub const MANUFACTURER_NAME: u8 = 0x20;
    pub const DEVICE_NAME: u8 = 0x21;

    pub const CELL_VOLTAGE_4: u8 = 0x3C;
    pub const CELL_VOLTAGE_3: u8 = 0x3D;
    pub const CELL_VOLTAGE_2: u8 = 0x3E;
    pub const CELL_VOLTAGE_1: u8 = 0x3F;
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

/// SMBus block read helper (length-prefixed).
///
/// Returns the number of data bytes copied into `data`.
pub fn read_block<I2C>(
    i2c: &mut I2C,
    addr: u8,
    sbscmd: u8,
    data: &mut [u8],
) -> Result<usize, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    // SMBus limits block payloads to 32 bytes. The first returned byte is the payload length.
    let mut buf = [0u8; 33];
    i2c.write_read(addr, &[sbscmd], &mut buf)?;
    let len = (buf[0] as usize).min(32).min(data.len());
    data[..len].copy_from_slice(&buf[1..(1 + len)]);
    Ok(len)
}

pub fn read_block_raw_checked<I2C>(
    i2c: &mut I2C,
    addr: u8,
    sbscmd: u8,
) -> Result<BlockReadRaw, BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c,
{
    let mut buf = [0u8; 33];
    i2c.write_read(addr, &[sbscmd], &mut buf)
        .map_err(|_| BmsDiagError::I2cNack)?;

    let declared_len = buf[0];
    if declared_len == 0 || declared_len > 32 {
        return Err(BmsDiagError::BadBlockLen);
    }

    let payload_len = declared_len.min(32);
    let mut payload = [0u8; 32];
    let payload_len_usize = payload_len as usize;
    payload[..payload_len_usize].copy_from_slice(&buf[1..(1 + payload_len_usize)]);

    Ok(BlockReadRaw {
        declared_len,
        payload_len,
        payload,
    })
}

/// Convert Temperature() units (0.1 K) to 0.1 C (i.e., C * 10).
pub const fn temp_c_x10_from_k_x10(temp_k_x10: u16) -> i32 {
    temp_k_x10 as i32 - 2731
}
