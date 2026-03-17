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
    pub const BATTERY_MODE: u8 = 0x03;
    pub const TEMPERATURE: u8 = 0x08;
    pub const VOLTAGE: u8 = 0x09;
    pub const CURRENT: u8 = 0x0A;
    pub const RELATIVE_STATE_OF_CHARGE: u8 = 0x0D;
    pub const REMAINING_CAPACITY: u8 = 0x0F;
    pub const FULL_CHARGE_CAPACITY: u8 = 0x10;
    pub const BATTERY_STATUS: u8 = 0x16;
    pub const OPERATION_STATUS: u8 = 0x54;
    pub const DA_STATUS_2: u8 = 0x72;
    pub const CB_STATUS: u8 = 0x76;

    pub const CELL_VOLTAGE_4: u8 = 0x3C;
    pub const CELL_VOLTAGE_3: u8 = 0x3D;
    pub const CELL_VOLTAGE_2: u8 = 0x3E;
    pub const CELL_VOLTAGE_1: u8 = 0x3F;
}

pub mod battery_mode {
    pub const CAPM: u16 = 1 << 15;
}

pub mod operation_status {
    pub const CB: u32 = 1 << 28;
    pub const SLEEP: u32 = 1 << 15;
    pub const XCHG: u32 = 1 << 14;
    pub const XDSG: u32 = 1 << 13;
    pub const PF: u32 = 1 << 12;
    pub const PCHG: u32 = 1 << 3;
    pub const CHG: u32 = 1 << 2;
    pub const DSG: u32 = 1 << 1;
    pub const PRES: u32 = 1 << 0;
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

fn crc8_smbus(bytes: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &byte in bytes {
        crc ^= byte;
        for _ in 0..8 {
            crc = if (crc & 0x80) != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
        }
    }
    crc
}

fn read_u16_with_pec<I2C>(i2c: &mut I2C, addr: u8, sbscmd: u8) -> Option<u16>
where
    I2C: embedded_hal::i2c::I2c,
{
    let mut buf = [0u8; 3];
    i2c.write_read(addr, &[sbscmd], &mut buf).ok()?;

    let addr_w = addr << 1;
    let addr_r = addr_w | 1;
    let expected = crc8_smbus(&[addr_w, sbscmd, addr_r, buf[0], buf[1]]);
    if expected != buf[2] {
        return None;
    }

    Some(u16::from_le_bytes([buf[0], buf[1]]))
}

pub fn read_u16<I2C>(i2c: &mut I2C, addr: u8, sbscmd: u8) -> Result<u16, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    if let Some(v) = read_u16_with_pec(i2c, addr, sbscmd) {
        return Ok(v);
    }

    let mut buf = [0u8; 2];
    i2c.write_read(addr, &[sbscmd], &mut buf)?;
    Ok(u16::from_le_bytes(buf))
}

pub fn read_i16<I2C>(i2c: &mut I2C, addr: u8, sbscmd: u8) -> Result<i16, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    read_u16(i2c, addr, sbscmd).map(|raw| i16::from_le_bytes(raw.to_le_bytes()))
}

pub const MAX_BLOCK_PAYLOAD_LEN: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockReadRaw {
    pub declared_len: u8,
    pub payload_len: u8,
    pub payload: [u8; MAX_BLOCK_PAYLOAD_LEN],
}

fn parse_block_read_raw(declared_len: u8, payload_bytes: &[u8]) -> Option<BlockReadRaw> {
    let declared_len = declared_len as usize;
    if !(1..=MAX_BLOCK_PAYLOAD_LEN).contains(&declared_len) {
        return None;
    }
    if payload_bytes.len() < declared_len {
        return None;
    }

    let mut payload = [0u8; MAX_BLOCK_PAYLOAD_LEN];
    payload[..declared_len].copy_from_slice(&payload_bytes[..declared_len]);
    Some(BlockReadRaw {
        declared_len: declared_len as u8,
        payload_len: declared_len as u8,
        payload,
    })
}

fn parse_pec_block_read_raw(
    buf: &[u8; MAX_BLOCK_PAYLOAD_LEN + 2],
    addr: u8,
    sbscmd: u8,
) -> Option<BlockReadRaw> {
    let declared_len = buf[0] as usize;
    if !(1..=MAX_BLOCK_PAYLOAD_LEN).contains(&declared_len) {
        return None;
    }

    let addr_w = addr << 1;
    let addr_r = addr_w | 1;
    let mut crc_buf = [0u8; MAX_BLOCK_PAYLOAD_LEN + 4];
    crc_buf[0] = addr_w;
    crc_buf[1] = sbscmd;
    crc_buf[2] = addr_r;
    crc_buf[3] = buf[0];
    crc_buf[4..(4 + declared_len)].copy_from_slice(&buf[1..(1 + declared_len)]);
    let expected_pec = crc8_smbus(&crc_buf[..(4 + declared_len)]);
    if expected_pec != buf[1 + declared_len] {
        return None;
    }

    parse_block_read_raw(buf[0], &buf[1..(1 + declared_len)])
}

fn read_plain_block_raw<I2C>(
    i2c: &mut I2C,
    addr: u8,
    sbscmd: u8,
) -> Result<Option<BlockReadRaw>, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    let mut buf = [0u8; MAX_BLOCK_PAYLOAD_LEN + 1];
    i2c.write_read(addr, &[sbscmd], &mut buf)?;
    Ok(parse_block_read_raw(buf[0], &buf[1..]))
}

pub fn read_block_raw<I2C>(
    i2c: &mut I2C,
    addr: u8,
    sbscmd: u8,
) -> Result<Option<BlockReadRaw>, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    let mut buf = [0u8; MAX_BLOCK_PAYLOAD_LEN + 2];
    i2c.write_read(addr, &[sbscmd], &mut buf)?;

    if let Some(raw) = parse_pec_block_read_raw(&buf, addr, sbscmd) {
        return Ok(Some(raw));
    }

    read_plain_block_raw(i2c, addr, sbscmd)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DaStatus2 {
    pub int_temp_k_x10: u16,
    pub ts_temp_k_x10: [u16; 4],
    pub cell_temp_k_x10: u16,
    pub fet_temp_k_x10: u16,
    pub gauging_temp_k_x10: u16,
}

pub fn read_da_status2<I2C>(i2c: &mut I2C, addr: u8) -> Result<Option<DaStatus2>, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    let Some(raw) = read_block_raw(i2c, addr, cmd::DA_STATUS_2)? else {
        return Ok(None);
    };
    if raw.payload_len < 16 {
        return Ok(None);
    }

    Ok(Some(DaStatus2 {
        int_temp_k_x10: u16::from_le_bytes([raw.payload[0], raw.payload[1]]),
        ts_temp_k_x10: [
            u16::from_le_bytes([raw.payload[2], raw.payload[3]]),
            u16::from_le_bytes([raw.payload[4], raw.payload[5]]),
            u16::from_le_bytes([raw.payload[6], raw.payload[7]]),
            u16::from_le_bytes([raw.payload[8], raw.payload[9]]),
        ],
        cell_temp_k_x10: u16::from_le_bytes([raw.payload[10], raw.payload[11]]),
        fet_temp_k_x10: u16::from_le_bytes([raw.payload[12], raw.payload[13]]),
        gauging_temp_k_x10: u16::from_le_bytes([raw.payload[14], raw.payload[15]]),
    }))
}

/// Read the 32-bit OperationStatus() block response.
///
/// TRM marks 0x54 as an H4/block command, so reading it as a plain word can
/// return stale or misaligned bytes.
pub fn read_operation_status<I2C>(i2c: &mut I2C, addr: u8) -> Result<Option<u32>, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    let Some(raw) = read_block_raw(i2c, addr, cmd::OPERATION_STATUS)? else {
        return Ok(None);
    };
    if raw.payload_len < 4 {
        return Ok(None);
    }

    Ok(Some(u32::from_le_bytes([
        raw.payload[0],
        raw.payload[1],
        raw.payload[2],
        raw.payload[3],
    ])))
}

/// Convert Temperature() units (0.1 K) to 0.1 C (i.e., C * 10).
pub const fn temp_c_x10_from_k_x10(temp_k_x10: u16) -> i32 {
    temp_k_x10 as i32 - 2731
}

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_hal::i2c::{ErrorKind, ErrorType, I2c, Operation};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct FakeError;

    impl embedded_hal::i2c::Error for FakeError {
        fn kind(&self) -> ErrorKind {
            ErrorKind::Other
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    enum Step {
        Write(u8, Vec<u8>),
        Read(u8, Vec<u8>),
    }

    struct ScriptedI2c {
        steps: std::collections::VecDeque<Step>,
    }

    impl ScriptedI2c {
        fn new(steps: impl IntoIterator<Item = Step>) -> Self {
            Self {
                steps: steps.into_iter().collect(),
            }
        }
    }

    impl ErrorType for ScriptedI2c {
        type Error = FakeError;
    }

    impl I2c for ScriptedI2c {
        fn transaction(
            &mut self,
            address: u8,
            operations: &mut [Operation<'_>],
        ) -> Result<(), Self::Error> {
            for operation in operations {
                match operation {
                    Operation::Write(buf) => {
                        let Some(Step::Write(expected_addr, expected)) = self.steps.pop_front()
                        else {
                            panic!("missing scripted write step");
                        };
                        assert_eq!(address, expected_addr);
                        assert_eq!(buf, expected.as_slice());
                    }
                    Operation::Read(buf) => {
                        let Some(Step::Read(expected_addr, data)) = self.steps.pop_front() else {
                            panic!("missing scripted read step");
                        };
                        assert_eq!(address, expected_addr);
                        assert_eq!(buf.len(), data.len());
                        buf.copy_from_slice(&data);
                    }
                }
            }
            Ok(())
        }
    }

    #[test]
    fn read_block_raw_accepts_valid_pec_block_frames() {
        let addr = I2C_ADDRESS_PRIMARY;
        let cmd = cmd::OPERATION_STATUS;
        let payload = [0x44, 0x33, 0x22, 0x11];
        let addr_w = addr << 1;
        let addr_r = addr_w | 1;
        let pec = crc8_smbus(&[
            addr_w, cmd, addr_r, 4, payload[0], payload[1], payload[2], payload[3],
        ]);

        let mut frame = vec![0u8; MAX_BLOCK_PAYLOAD_LEN + 2];
        frame[0] = 4;
        frame[1..5].copy_from_slice(&payload);
        frame[5] = pec;

        let mut i2c = ScriptedI2c::new([Step::Write(addr, vec![cmd]), Step::Read(addr, frame)]);

        let raw = read_block_raw(&mut i2c, addr, cmd).unwrap().unwrap();

        assert_eq!(raw.declared_len, 4);
        assert_eq!(raw.payload_len, 4);
        assert_eq!(&raw.payload[..4], &payload);
    }

    #[test]
    fn read_block_raw_falls_back_to_plain_block_frames_when_pec_is_absent() {
        let addr = I2C_ADDRESS_PRIMARY;
        let cmd = cmd::CB_STATUS;
        let payload = [0x0c, 0x00, 0x00, 0x00];

        let mut pec_probe = vec![0u8; MAX_BLOCK_PAYLOAD_LEN + 2];
        pec_probe[0] = 4;
        pec_probe[1..5].copy_from_slice(&payload);

        let mut plain_frame = vec![0u8; MAX_BLOCK_PAYLOAD_LEN + 1];
        plain_frame[0] = 4;
        plain_frame[1..5].copy_from_slice(&payload);

        let mut i2c = ScriptedI2c::new([
            Step::Write(addr, vec![cmd]),
            Step::Read(addr, pec_probe),
            Step::Write(addr, vec![cmd]),
            Step::Read(addr, plain_frame),
        ]);

        let raw = read_block_raw(&mut i2c, addr, cmd).unwrap().unwrap();

        assert_eq!(raw.declared_len, 4);
        assert_eq!(raw.payload_len, 4);
        assert_eq!(&raw.payload[..4], &payload);
    }

    #[test]
    fn read_block_raw_rejects_frames_when_neither_pec_nor_plain_is_confirmed() {
        let addr = I2C_ADDRESS_PRIMARY;
        let cmd = cmd::CB_STATUS;
        let payload = [0x0c, 0x00, 0x00, 0x00];

        let mut pec_probe = vec![0u8; MAX_BLOCK_PAYLOAD_LEN + 2];
        pec_probe[0] = 4;
        pec_probe[1..5].copy_from_slice(&payload);
        pec_probe[5] = 0xaa;

        let mut invalid_plain_frame = vec![0u8; MAX_BLOCK_PAYLOAD_LEN + 1];
        invalid_plain_frame[0] = 0;

        let mut i2c = ScriptedI2c::new([
            Step::Write(addr, vec![cmd]),
            Step::Read(addr, pec_probe),
            Step::Write(addr, vec![cmd]),
            Step::Read(addr, invalid_plain_frame),
        ]);

        let raw = read_block_raw(&mut i2c, addr, cmd).unwrap();

        assert_eq!(raw, None);
    }
}
