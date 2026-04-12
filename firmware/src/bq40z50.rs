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

use crate::time::{Duration, Instant};

/// Default (project) 7-bit SMBus address for BQ40Z50 (per `docs/i2c-address-map.md`).
pub const I2C_ADDRESS_PRIMARY: u8 = 0x0B;

/// Default (TI TRM data flash) 7-bit SMBus address for BQ40Z50.
///
/// TRM note: the SMBus address is configurable via data flash and falls back to 0x16 if the
/// programmed values are invalid.
pub const I2C_ADDRESS_FALLBACK: u8 = 0x16;

pub const I2C_ADDRESS_CANDIDATES: [u8; 2] = [I2C_ADDRESS_PRIMARY, I2C_ADDRESS_FALLBACK];

pub mod cmd {
    pub const MANUFACTURER_ACCESS: u8 = 0x00;
    pub const BATTERY_MODE: u8 = 0x03;
    pub const TEMPERATURE: u8 = 0x08;
    pub const VOLTAGE: u8 = 0x09;
    pub const CURRENT: u8 = 0x0A;
    pub const RELATIVE_STATE_OF_CHARGE: u8 = 0x0D;
    pub const REMAINING_CAPACITY: u8 = 0x0F;
    pub const FULL_CHARGE_CAPACITY: u8 = 0x10;
    pub const BATTERY_STATUS: u8 = 0x16;
    pub const MANUFACTURER_DATA: u8 = 0x23;
    pub const OPERATION_STATUS: u8 = 0x54;
    pub const CHARGING_STATUS: u8 = 0x55;
    pub const GAUGING_STATUS: u8 = 0x56;
    pub const AFE_REGISTER: u8 = 0x58;

    pub const CELL_VOLTAGE_4: u8 = 0x3C;
    pub const CELL_VOLTAGE_3: u8 = 0x3D;
    pub const CELL_VOLTAGE_2: u8 = 0x3E;
    pub const CELL_VOLTAGE_1: u8 = 0x3F;
}

pub mod charging_status {
    pub const UT: u32 = 1 << 0;
    pub const LT: u32 = 1 << 1;
    pub const STL: u32 = 1 << 2;
    pub const RT: u32 = 1 << 3;
    pub const STH: u32 = 1 << 4;
    pub const HT: u32 = 1 << 5;
    pub const OT: u32 = 1 << 6;
    pub const PV: u32 = 1 << 8;
    pub const LV: u32 = 1 << 9;
    pub const MV: u32 = 1 << 10;
    pub const HV: u32 = 1 << 11;
    pub const IN: u32 = 1 << 12;
    pub const SU: u32 = 1 << 13;
    pub const MCHG: u32 = 1 << 14;
    pub const VCT: u32 = 1 << 15;
    pub const CCR: u32 = 1 << 16;
    pub const CVR: u32 = 1 << 17;
    pub const CCC: u32 = 1 << 18;
    pub const NCT: u32 = 1 << 19;
}

pub mod gauging_status {
    pub const QEN: u32 = 1 << 12;
    pub const VOK: u32 = 1 << 11;
    pub const R_DIS: u32 = 1 << 10;
    pub const REST: u32 = 1 << 8;
    pub const FC: u32 = 1 << 1;
    pub const FD: u32 = 1 << 0;
}

pub mod mac {
    pub const SAFETY_STATUS: u16 = 0x0051;
    pub const PF_STATUS: u16 = 0x0053;
    pub const MANUFACTURING_STATUS: u16 = 0x0057;
    pub const DA_STATUS_2: u16 = 0x0072;
    pub const FILTER_CAPACITY: u16 = 0x0078;
}

pub mod data_flash {
    pub const UPDATE_STATUS: u16 = 0x4312;
    pub const CURRENT_AT_EOC: u16 = 0x431B;
    pub const NO_VALID_CHARGE_TERM: u16 = 0x43D0;
    pub const LAST_VALID_CHARGE_TERM: u16 = 0x43D2;
    pub const NO_OF_QMAX_UPDATES: u16 = 0x43D4;
    pub const NO_OF_RA_UPDATES: u16 = 0x43D8;
    pub const SBS_CONFIGURATION: u16 = 0x4889;
    pub const POWER_CONFIG: u16 = 0x488B;
    pub const BALANCING_CONFIGURATION: u16 = 0x4908;
    pub const MIN_START_BALANCE_DELTA: u16 = 0x490D;
    pub const RELAX_BALANCE_INTERVAL: u16 = 0x490E;
    pub const MIN_RSOC_FOR_BALANCING: u16 = 0x4912;
    pub const CHARGE_TEMP_T1: u16 = 0x4A0B;
    pub const CHARGE_TEMP_T2: u16 = 0x4A0D;
    pub const CHARGE_TEMP_T5: u16 = 0x4A0F;
    pub const CHARGE_TEMP_T6: u16 = 0x4A11;
    pub const CHARGE_TEMP_T3: u16 = 0x4A13;
    pub const CHARGE_TEMP_T4: u16 = 0x4A15;
    pub const CHARGE_TEMP_HYSTERESIS: u16 = 0x4A17;
    pub const TEMPERATURE_ENABLE: u16 = 0x4A7B;
    pub const TEMPERATURE_MODE: u16 = 0x4A7C;
    pub const DA_CONFIGURATION: u16 = 0x4A7D;
}

const MAC_WRITE_SETTLE: Duration = Duration::from_millis(66);

pub mod battery_mode {
    pub const CAPM: u16 = 1 << 15;
}

pub mod operation_status {
    pub const EMSHUT: u32 = 1 << 29;
    pub const CB: u32 = 1 << 28;
    pub const SLEEP: u32 = 1 << 15;
    pub const XCHG: u32 = 1 << 14;
    pub const XDSG: u32 = 1 << 13;
    pub const PF: u32 = 1 << 12;
    pub const SEC0: u32 = 1 << 8;
    pub const BTP_INT: u32 = 1 << 7;
    pub const PCHG: u32 = 1 << 3;
    pub const CHG: u32 = 1 << 2;
    pub const DSG: u32 = 1 << 1;
    pub const PRES: u32 = 1 << 0;
}

pub mod da_configuration {
    pub const EMSHUT_PEXIT_DIS: u16 = 1 << 8;
    pub const FTEMP: u16 = 1 << 7;
    pub const EMSHUT_EN: u16 = 1 << 5;
    pub const SLEEP: u16 = 1 << 4;
    pub const IN_SYSTEM_SLEEP: u16 = 1 << 3;
    pub const NR: u16 = 1 << 2;
}

pub mod power_config {
    pub const CHECK_WAKE_FET: u16 = 1 << 5;
    pub const CHECK_WAKE: u16 = 1 << 4;
    pub const EMSHUT_EXIT_COMM: u16 = 1 << 3;
    pub const EMSHUT_EXIT_VPACK: u16 = 1 << 2;
    pub const PWR_SAVE_VSHUT: u16 = 1 << 1;
    pub const AUTO_SHIP_EN: u16 = 1 << 0;
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

pub mod safety_status {
    pub const CUV: u32 = 1 << 0;
    pub const COV: u32 = 1 << 1;
    pub const OCC1: u32 = 1 << 2;
    pub const OCC2: u32 = 1 << 3;
    pub const OCD1: u32 = 1 << 4;
    pub const OCD2: u32 = 1 << 5;
    pub const AOLD: u32 = 1 << 6;
    pub const AOLDL: u32 = 1 << 7;
    pub const ASCC: u32 = 1 << 8;
    pub const ASCCL: u32 = 1 << 9;
    pub const ASCD: u32 = 1 << 10;
    pub const ASCDL: u32 = 1 << 11;
    pub const OTC: u32 = 1 << 12;
    pub const OTD: u32 = 1 << 13;
    pub const CUVC: u32 = 1 << 14;
    pub const OC: u32 = 1 << 22;
}

pub mod pf_status {
    pub const SUV: u32 = 1 << 0;
    pub const SOV: u32 = 1 << 1;
    pub const SOCC: u32 = 1 << 2;
    pub const SOCD: u32 = 1 << 3;
    pub const COVL: u32 = 1 << 5;
    pub const QIM: u32 = 1 << 8;
    pub const IMP: u32 = 1 << 9;
    pub const CD: u32 = 1 << 10;
    pub const VIMR: u32 = 1 << 11;
    pub const VIMA: u32 = 1 << 12;
    pub const AOLDL: u32 = 1 << 13;
    pub const ASCCL: u32 = 1 << 14;
    pub const ASCDL: u32 = 1 << 15;
    pub const CFETF: u32 = 1 << 16;
    pub const DFETF: u32 = 1 << 17;
    pub const OCDL: u32 = 1 << 18;
    pub const FUSE: u32 = 1 << 19;
    pub const AFER: u32 = 1 << 20;
    pub const AFEC: u32 = 1 << 21;
    pub const SECOND_LEVEL: u32 = 1 << 22;
}

pub mod manufacturing_status {
    pub const PCHG_EN: u32 = 1 << 0;
    pub const CHG_EN: u32 = 1 << 1;
    pub const DSG_EN: u32 = 1 << 2;
    pub const GAUGE_EN: u32 = 1 << 3;
    pub const FET_EN: u32 = 1 << 4;
    pub const LF_EN: u32 = 1 << 5;
    pub const PF_EN: u32 = 1 << 6;
    pub const BBR_EN: u32 = 1 << 7;
    pub const FUSE_EN: u32 = 1 << 8;
    pub const LED_EN: u32 = 1 << 9;
    pub const LT_TEST: u32 = 1 << 14;
    pub const CAL_TEST: u32 = 1 << 15;
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

fn spin_delay(wait: Duration) {
    let start = Instant::now();
    while start.elapsed() < wait {
        core::hint::spin_loop();
    }
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
pub const BLOCK_READ_TRACE_PREFIX_LEN: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockReadSource {
    Pec,
    Plain,
}

pub const fn block_read_source_name(source: Option<BlockReadSource>) -> &'static str {
    match source {
        Some(BlockReadSource::Pec) => "pec",
        Some(BlockReadSource::Plain) => "plain",
        None => "none",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockReadProbeStatus {
    NotAttempted,
    Ok,
    InvalidDeclaredLen,
    PecMismatch,
}

pub const fn block_read_probe_status_name(status: BlockReadProbeStatus) -> &'static str {
    match status {
        BlockReadProbeStatus::NotAttempted => "not_attempted",
        BlockReadProbeStatus::Ok => "ok",
        BlockReadProbeStatus::InvalidDeclaredLen => "invalid_declared_len",
        BlockReadProbeStatus::PecMismatch => "pec_mismatch",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockReadRaw {
    pub declared_len: u8,
    pub payload_len: u8,
    pub payload: [u8; MAX_BLOCK_PAYLOAD_LEN],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockReadProbe {
    pub status: BlockReadProbeStatus,
    pub declared_len: u8,
    pub payload_len: u8,
    pub prefix_len: u8,
    pub prefix: [u8; BLOCK_READ_TRACE_PREFIX_LEN],
}

impl BlockReadProbe {
    const fn empty(status: BlockReadProbeStatus) -> Self {
        Self {
            status,
            declared_len: 0,
            payload_len: 0,
            prefix_len: 0,
            prefix: [0; BLOCK_READ_TRACE_PREFIX_LEN],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockReadTrace {
    pub selected_source: Option<BlockReadSource>,
    pub raw: Option<BlockReadRaw>,
    pub pec: BlockReadProbe,
    pub plain: BlockReadProbe,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChargingStatusTrace {
    pub block: BlockReadTrace,
    pub value: Option<u32>,
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

fn capture_probe_prefix(bytes: &[u8]) -> (u8, [u8; BLOCK_READ_TRACE_PREFIX_LEN]) {
    let mut prefix = [0u8; BLOCK_READ_TRACE_PREFIX_LEN];
    let prefix_len = core::cmp::min(bytes.len(), BLOCK_READ_TRACE_PREFIX_LEN);
    prefix[..prefix_len].copy_from_slice(&bytes[..prefix_len]);
    (prefix_len as u8, prefix)
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

fn inspect_plain_block_read_raw(buf: &[u8; MAX_BLOCK_PAYLOAD_LEN + 1]) -> BlockReadProbe {
    let declared_len = buf[0];
    let (prefix_len, prefix) = capture_probe_prefix(&buf[1..]);
    let declared_len_usize = declared_len as usize;
    if !(1..=MAX_BLOCK_PAYLOAD_LEN).contains(&declared_len_usize) {
        return BlockReadProbe {
            status: BlockReadProbeStatus::InvalidDeclaredLen,
            declared_len,
            payload_len: 0,
            prefix_len,
            prefix,
        };
    }

    BlockReadProbe {
        status: BlockReadProbeStatus::Ok,
        declared_len,
        payload_len: declared_len,
        prefix_len,
        prefix,
    }
}

fn inspect_pec_block_read_raw(
    buf: &[u8; MAX_BLOCK_PAYLOAD_LEN + 2],
    addr: u8,
    sbscmd: u8,
) -> BlockReadProbe {
    let declared_len = buf[0];
    let (prefix_len, prefix) = capture_probe_prefix(&buf[1..]);
    let declared_len_usize = declared_len as usize;
    if !(1..=MAX_BLOCK_PAYLOAD_LEN).contains(&declared_len_usize) {
        return BlockReadProbe {
            status: BlockReadProbeStatus::InvalidDeclaredLen,
            declared_len,
            payload_len: 0,
            prefix_len,
            prefix,
        };
    }

    let addr_w = addr << 1;
    let addr_r = addr_w | 1;
    let mut crc_buf = [0u8; MAX_BLOCK_PAYLOAD_LEN + 4];
    crc_buf[0] = addr_w;
    crc_buf[1] = sbscmd;
    crc_buf[2] = addr_r;
    crc_buf[3] = buf[0];
    crc_buf[4..(4 + declared_len_usize)].copy_from_slice(&buf[1..(1 + declared_len_usize)]);
    let expected_pec = crc8_smbus(&crc_buf[..(4 + declared_len_usize)]);
    let status = if expected_pec == buf[1 + declared_len_usize] {
        BlockReadProbeStatus::Ok
    } else {
        BlockReadProbeStatus::PecMismatch
    };

    BlockReadProbe {
        status,
        declared_len,
        payload_len: declared_len,
        prefix_len,
        prefix,
    }
}

pub fn read_block_raw<I2C>(
    i2c: &mut I2C,
    addr: u8,
    sbscmd: u8,
) -> Result<Option<BlockReadRaw>, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    Ok(read_block_raw_trace(i2c, addr, sbscmd)?.raw)
}

pub fn read_block_raw_trace<I2C>(
    i2c: &mut I2C,
    addr: u8,
    sbscmd: u8,
) -> Result<BlockReadTrace, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    let mut pec_buf = [0u8; MAX_BLOCK_PAYLOAD_LEN + 2];
    i2c.write_read(addr, &[sbscmd], &mut pec_buf)?;
    let pec = inspect_pec_block_read_raw(&pec_buf, addr, sbscmd);
    if matches!(pec.status, BlockReadProbeStatus::Ok) {
        return Ok(BlockReadTrace {
            selected_source: Some(BlockReadSource::Pec),
            raw: parse_pec_block_read_raw(&pec_buf, addr, sbscmd),
            pec,
            plain: BlockReadProbe::empty(BlockReadProbeStatus::NotAttempted),
        });
    }

    let mut plain_buf = [0u8; MAX_BLOCK_PAYLOAD_LEN + 1];
    i2c.write_read(addr, &[sbscmd], &mut plain_buf)?;
    let plain = inspect_plain_block_read_raw(&plain_buf);
    let raw = if matches!(plain.status, BlockReadProbeStatus::Ok) {
        parse_block_read_raw(plain_buf[0], &plain_buf[1..])
    } else {
        None
    };

    Ok(BlockReadTrace {
        selected_source: raw.map(|_| BlockReadSource::Plain),
        raw,
        pec,
        plain,
    })
}

pub fn read_mac_block_raw<I2C>(
    i2c: &mut I2C,
    addr: u8,
    mac_cmd: u16,
) -> Result<Option<BlockReadRaw>, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    let mac_cmd = mac_cmd.to_le_bytes();
    i2c.write(addr, &[cmd::MANUFACTURER_ACCESS, mac_cmd[0], mac_cmd[1]])?;
    spin_delay(MAC_WRITE_SETTLE);
    read_block_raw(i2c, addr, cmd::MANUFACTURER_DATA)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DaStatus2 {
    pub int_temp_k_x10: u16,
    pub ts_temp_k_x10: [u16; 4],
    pub cell_temp_k_x10: u16,
    pub fet_temp_k_x10: u16,
    pub gauging_temp_k_x10: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FilterCapacity {
    pub remaining_capacity_mah: u16,
    pub remaining_energy_cwh: u16,
    pub full_charge_capacity_mah: u16,
    pub full_charge_energy_cwh: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BalanceConfig {
    pub raw: u8,
    pub min_start_balance_delta_mv: u8,
    pub relax_balance_interval_s: u32,
    pub min_rsoc_for_balancing_pct: u8,
}

impl BalanceConfig {
    pub const fn cb(self) -> bool {
        (self.raw & (1 << 0)) != 0
    }

    pub const fn cbm(self) -> bool {
        (self.raw & (1 << 1)) != 0
    }

    pub const fn cbr(self) -> bool {
        (self.raw & (1 << 2)) != 0
    }

    pub const fn cbs(self) -> bool {
        (self.raw & (1 << 5)) != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AfeRegister {
    pub cell_balance_status: u8,
}

pub fn read_da_status2<I2C>(i2c: &mut I2C, addr: u8) -> Result<Option<DaStatus2>, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    let Some(raw) = read_mac_block_raw(i2c, addr, mac::DA_STATUS_2)? else {
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

pub fn read_filter_capacity<I2C>(
    i2c: &mut I2C,
    addr: u8,
) -> Result<Option<FilterCapacity>, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    let Some(raw) = read_mac_block_raw(i2c, addr, mac::FILTER_CAPACITY)? else {
        return Ok(None);
    };
    if raw.payload_len < 8 {
        return Ok(None);
    }

    Ok(Some(FilterCapacity {
        remaining_capacity_mah: u16::from_le_bytes([raw.payload[0], raw.payload[1]]),
        remaining_energy_cwh: u16::from_le_bytes([raw.payload[2], raw.payload[3]]),
        full_charge_capacity_mah: u16::from_le_bytes([raw.payload[4], raw.payload[5]]),
        full_charge_energy_cwh: u16::from_le_bytes([raw.payload[6], raw.payload[7]]),
    }))
}

pub fn read_balance_config<I2C>(
    i2c: &mut I2C,
    addr: u8,
) -> Result<Option<BalanceConfig>, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    let Some(raw) = read_data_flash_u8(i2c, addr, data_flash::BALANCING_CONFIGURATION)? else {
        return Ok(None);
    };
    let Some(min_start_balance_delta_mv) =
        read_data_flash_u8(i2c, addr, data_flash::MIN_START_BALANCE_DELTA)?
    else {
        return Ok(None);
    };
    let Some(relax_balance_interval_s) =
        read_data_flash_u32(i2c, addr, data_flash::RELAX_BALANCE_INTERVAL)?
    else {
        return Ok(None);
    };
    let Some(min_rsoc_for_balancing_pct) =
        read_data_flash_u8(i2c, addr, data_flash::MIN_RSOC_FOR_BALANCING)?
    else {
        return Ok(None);
    };

    Ok(Some(BalanceConfig {
        raw,
        min_start_balance_delta_mv,
        relax_balance_interval_s,
        min_rsoc_for_balancing_pct,
    }))
}

pub fn read_afe_register<I2C>(i2c: &mut I2C, addr: u8) -> Result<Option<AfeRegister>, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    let Some(raw) = read_block_raw(i2c, addr, cmd::AFE_REGISTER)? else {
        return Ok(None);
    };
    if raw.payload_len < 11 {
        return Ok(None);
    }

    Ok(Some(AfeRegister {
        cell_balance_status: raw.payload[10],
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

/// Read the 32-bit ChargingStatus() block response.
///
/// TRM marks 0x55 as an H4/block command, so reading it as a plain word can
/// return stale or truncated bytes.
pub fn read_charging_status<I2C>(i2c: &mut I2C, addr: u8) -> Result<Option<u32>, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    let Some(raw) = read_block_raw(i2c, addr, cmd::CHARGING_STATUS)? else {
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

pub fn read_charging_status_trace<I2C>(
    i2c: &mut I2C,
    addr: u8,
) -> Result<ChargingStatusTrace, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    let block = read_block_raw_trace(i2c, addr, cmd::CHARGING_STATUS)?;
    let value = block.raw.and_then(|raw| {
        (raw.payload_len >= 4).then_some(u32::from_le_bytes([
            raw.payload[0],
            raw.payload[1],
            raw.payload[2],
            raw.payload[3],
        ]))
    });
    Ok(ChargingStatusTrace { block, value })
}

/// Read the 32-bit GaugingStatus() block response.
///
/// TRM marks 0x56 as an H4/block command, so reading it as a plain word can
/// return stale or truncated bytes.
pub fn read_gauging_status<I2C>(i2c: &mut I2C, addr: u8) -> Result<Option<u32>, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    let Some(raw) = read_block_raw(i2c, addr, cmd::GAUGING_STATUS)? else {
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

pub fn read_mac_u32<I2C>(i2c: &mut I2C, addr: u8, mac_cmd: u16) -> Result<Option<u32>, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    let Some(raw) = read_mac_block_raw(i2c, addr, mac_cmd)? else {
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

pub fn read_mac_u16<I2C>(i2c: &mut I2C, addr: u8, mac_cmd: u16) -> Result<Option<u16>, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    let Some(raw) = read_mac_block_raw(i2c, addr, mac_cmd)? else {
        return Ok(None);
    };
    if raw.payload_len < 2 {
        return Ok(None);
    }

    Ok(Some(u16::from_le_bytes([raw.payload[0], raw.payload[1]])))
}

pub fn read_data_flash_u16<I2C>(
    i2c: &mut I2C,
    addr: u8,
    df_addr: u16,
) -> Result<Option<u16>, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    read_mac_u16(i2c, addr, df_addr)
}

pub fn read_data_flash_u32<I2C>(
    i2c: &mut I2C,
    addr: u8,
    df_addr: u16,
) -> Result<Option<u32>, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    read_mac_u32(i2c, addr, df_addr)
}

pub fn read_data_flash_u8<I2C>(
    i2c: &mut I2C,
    addr: u8,
    df_addr: u16,
) -> Result<Option<u8>, I2C::Error>
where
    I2C: embedded_hal::i2c::I2c,
{
    let Some(raw) = read_mac_block_raw(i2c, addr, df_addr)? else {
        return Ok(None);
    };
    if raw.payload_len < 1 {
        return Ok(None);
    }

    Ok(Some(raw.payload[0]))
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
                        assert_eq!(buf.to_vec(), expected);
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
        let cmd = cmd::MANUFACTURER_DATA;
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
        let cmd = cmd::MANUFACTURER_DATA;
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

    #[test]
    fn read_mac_block_raw_writes_manufacturer_access_then_reads_manufacturer_data() {
        let addr = I2C_ADDRESS_PRIMARY;
        let payload = [0x11, 0x22, 0x33, 0x44];
        let mut plain_frame = vec![0u8; MAX_BLOCK_PAYLOAD_LEN + 1];
        plain_frame[0] = 4;
        plain_frame[1..5].copy_from_slice(&payload);

        let mut i2c = ScriptedI2c::new([
            Step::Write(addr, vec![cmd::MANUFACTURER_ACCESS, 0x72, 0x00]),
            Step::Write(addr, vec![cmd::MANUFACTURER_DATA]),
            Step::Read(addr, vec![0u8; MAX_BLOCK_PAYLOAD_LEN + 2]),
            Step::Write(addr, vec![cmd::MANUFACTURER_DATA]),
            Step::Read(addr, plain_frame),
        ]);

        let raw = read_mac_block_raw(&mut i2c, addr, mac::DA_STATUS_2)
            .unwrap()
            .unwrap();

        assert_eq!(raw.declared_len, 4);
        assert_eq!(&raw.payload[..4], &payload);
    }

    #[test]
    fn read_filter_capacity_decodes_energy_and_capacity_fields() {
        let addr = I2C_ADDRESS_PRIMARY;
        let payload = [0x34, 0x12, 0x78, 0x56, 0xbc, 0x9a, 0xf0, 0xde];
        let mut plain_frame = vec![0u8; MAX_BLOCK_PAYLOAD_LEN + 1];
        plain_frame[0] = 8;
        plain_frame[1..9].copy_from_slice(&payload);

        let mut i2c = ScriptedI2c::new([
            Step::Write(addr, vec![cmd::MANUFACTURER_ACCESS, 0x78, 0x00]),
            Step::Write(addr, vec![cmd::MANUFACTURER_DATA]),
            Step::Read(addr, vec![0u8; MAX_BLOCK_PAYLOAD_LEN + 2]),
            Step::Write(addr, vec![cmd::MANUFACTURER_DATA]),
            Step::Read(addr, plain_frame),
        ]);

        let filter = read_filter_capacity(&mut i2c, addr).unwrap().unwrap();

        assert_eq!(filter.remaining_capacity_mah, 0x1234);
        assert_eq!(filter.remaining_energy_cwh, 0x5678);
        assert_eq!(filter.full_charge_capacity_mah, 0x9abc);
        assert_eq!(filter.full_charge_energy_cwh, 0xdef0);
    }

    #[test]
    fn read_data_flash_u16_decodes_little_endian_values() {
        let addr = I2C_ADDRESS_PRIMARY;
        let payload = [0x27, 0x81];
        let mut plain_frame = vec![0u8; MAX_BLOCK_PAYLOAD_LEN + 1];
        plain_frame[0] = 2;
        plain_frame[1..3].copy_from_slice(&payload);

        let mut i2c = ScriptedI2c::new([
            Step::Write(addr, vec![cmd::MANUFACTURER_ACCESS, 0x7d, 0x4a]),
            Step::Write(addr, vec![cmd::MANUFACTURER_DATA]),
            Step::Read(addr, vec![0u8; MAX_BLOCK_PAYLOAD_LEN + 2]),
            Step::Write(addr, vec![cmd::MANUFACTURER_DATA]),
            Step::Read(addr, plain_frame),
        ]);

        let value = read_data_flash_u16(&mut i2c, addr, data_flash::DA_CONFIGURATION)
            .unwrap()
            .unwrap();

        assert_eq!(value, 0x8127);
    }

    #[test]
    fn read_charging_status_decodes_h4_block_payloads() {
        let addr = I2C_ADDRESS_PRIMARY;
        let payload = [0x03, 0x20, 0x00, 0x00];
        let mut plain_frame = vec![0u8; MAX_BLOCK_PAYLOAD_LEN + 1];
        plain_frame[0] = 4;
        plain_frame[1..5].copy_from_slice(&payload);

        let mut i2c = ScriptedI2c::new([
            Step::Write(addr, vec![cmd::CHARGING_STATUS]),
            Step::Read(addr, vec![0u8; MAX_BLOCK_PAYLOAD_LEN + 2]),
            Step::Write(addr, vec![cmd::CHARGING_STATUS]),
            Step::Read(addr, plain_frame),
        ]);

        let value = read_charging_status(&mut i2c, addr).unwrap().unwrap();

        assert_eq!(value, 0x0000_2003);
    }

    #[test]
    fn read_charging_status_trace_reports_plain_short_payloads() {
        let addr = I2C_ADDRESS_PRIMARY;

        let mut pec_probe = vec![0u8; MAX_BLOCK_PAYLOAD_LEN + 2];
        pec_probe[0] = 2;
        pec_probe[1] = 0x34;
        pec_probe[2] = 0x12;

        let mut plain_frame = vec![0u8; MAX_BLOCK_PAYLOAD_LEN + 1];
        plain_frame[0] = 2;
        plain_frame[1] = 0x34;
        plain_frame[2] = 0x12;

        let mut i2c = ScriptedI2c::new([
            Step::Write(addr, vec![cmd::CHARGING_STATUS]),
            Step::Read(addr, pec_probe),
            Step::Write(addr, vec![cmd::CHARGING_STATUS]),
            Step::Read(addr, plain_frame),
        ]);

        let trace = read_charging_status_trace(&mut i2c, addr).unwrap();

        assert_eq!(trace.block.selected_source, Some(BlockReadSource::Plain));
        assert_eq!(trace.block.pec.status, BlockReadProbeStatus::PecMismatch);
        assert_eq!(trace.block.plain.status, BlockReadProbeStatus::Ok);
        assert_eq!(trace.block.plain.declared_len, 2);
        assert_eq!(trace.block.plain.payload_len, 2);
        assert_eq!(trace.block.plain.prefix[0], 0x34);
        assert_eq!(trace.block.plain.prefix[1], 0x12);
        assert_eq!(trace.value, None);
    }

    #[test]
    fn read_charging_status_trace_preserves_pec_source() {
        let addr = I2C_ADDRESS_PRIMARY;
        let payload = [0x00, 0x80, 0x00, 0x00];
        let addr_w = addr << 1;
        let addr_r = addr_w | 1;
        let pec = crc8_smbus(&[
            addr_w,
            cmd::CHARGING_STATUS,
            addr_r,
            4,
            payload[0],
            payload[1],
            payload[2],
            payload[3],
        ]);

        let mut frame = vec![0u8; MAX_BLOCK_PAYLOAD_LEN + 2];
        frame[0] = 4;
        frame[1..5].copy_from_slice(&payload);
        frame[5] = pec;

        let mut i2c = ScriptedI2c::new([
            Step::Write(addr, vec![cmd::CHARGING_STATUS]),
            Step::Read(addr, frame),
        ]);

        let trace = read_charging_status_trace(&mut i2c, addr).unwrap();

        assert_eq!(trace.block.selected_source, Some(BlockReadSource::Pec));
        assert_eq!(trace.block.pec.status, BlockReadProbeStatus::Ok);
        assert_eq!(trace.block.plain.status, BlockReadProbeStatus::NotAttempted);
        assert_eq!(trace.value, Some(0x0000_8000));
    }

    #[test]
    fn read_gauging_status_decodes_h4_block_payloads() {
        let addr = I2C_ADDRESS_PRIMARY;
        let payload = [0x00, 0x19, 0x00, 0x00];
        let mut plain_frame = vec![0u8; MAX_BLOCK_PAYLOAD_LEN + 1];
        plain_frame[0] = 4;
        plain_frame[1..5].copy_from_slice(&payload);

        let mut i2c = ScriptedI2c::new([
            Step::Write(addr, vec![cmd::GAUGING_STATUS]),
            Step::Read(addr, vec![0u8; MAX_BLOCK_PAYLOAD_LEN + 2]),
            Step::Write(addr, vec![cmd::GAUGING_STATUS]),
            Step::Read(addr, plain_frame),
        ]);

        let value = read_gauging_status(&mut i2c, addr).unwrap().unwrap();

        assert_eq!(value, 0x0000_1900);
        assert!(value & gauging_status::QEN != 0);
        assert!(value & gauging_status::VOK != 0);
        assert!(value & gauging_status::REST != 0);
    }

    #[test]
    fn read_data_flash_u8_reads_single_byte_payloads() {
        let addr = I2C_ADDRESS_PRIMARY;
        let payload = [0x1e];
        let mut plain_frame = vec![0u8; MAX_BLOCK_PAYLOAD_LEN + 1];
        plain_frame[0] = 1;
        plain_frame[1] = payload[0];

        let mut i2c = ScriptedI2c::new([
            Step::Write(addr, vec![cmd::MANUFACTURER_ACCESS, 0x7b, 0x4a]),
            Step::Write(addr, vec![cmd::MANUFACTURER_DATA]),
            Step::Read(addr, vec![0u8; MAX_BLOCK_PAYLOAD_LEN + 2]),
            Step::Write(addr, vec![cmd::MANUFACTURER_DATA]),
            Step::Read(addr, plain_frame),
        ]);

        let value = read_data_flash_u8(&mut i2c, addr, data_flash::TEMPERATURE_ENABLE)
            .unwrap()
            .unwrap();

        assert_eq!(value, 0x1e);
    }
}
