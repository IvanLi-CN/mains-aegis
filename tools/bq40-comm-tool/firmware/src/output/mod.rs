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
        Error::AcknowledgeCheckFailed(reason) => match reason {
            esp_hal::i2c::master::AcknowledgeCheckFailedReason::Address => "i2c_nack_addr",
            esp_hal::i2c::master::AcknowledgeCheckFailedReason::Data => "i2c_nack_data",
            _ => "i2c_nack",
        },
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

const BMS_POLL_PERIOD: Duration = Duration::from_secs(2);
const BMS_INT_MIN_INTERVAL: Duration = Duration::from_millis(100);
const BMS_ISOLATION_WINDOW: Duration = Duration::from_millis(40);
const BMS_TRANSPORT_LOSS_THRESHOLD: u8 = 3;
const BMS_TRANSPORT_RETRY_BACKOFF: Duration = Duration::from_millis(250);
const BMS_WORD_DIAG_MIN_INTERVAL: Duration = Duration::from_secs(45);
// TI E2E reports that some BQ40 MAC/read flows need ~22ms read spacing and ~66ms after
// writes while the gauge finishes internal processing. Keep that slowdown confined to the
// diagnostic-only mac-only build so normal polling stays responsive.
const BMS_WORD_GAP: Duration = if cfg!(feature = "bms-mac-probe-only") {
    Duration::from_millis(22)
} else {
    Duration::from_millis(2)
};
const BMS_MAC_WRITE_SETTLE: Duration = if cfg!(feature = "bms-mac-probe-only") {
    Duration::from_millis(66)
} else {
    BMS_WORD_GAP
};
const BMS_WAKE_SETTLE: Duration = Duration::from_secs(30);
const BMS_BOOT_STAGE_POLL_PERIOD: Duration = Duration::from_secs(5);
const BMS_WAIT_ROM_FAST_POLL_PERIOD: Duration = Duration::from_millis(250);
const BMS_BOOT_MIN_CHARGE_SETTLE: Duration = Duration::from_secs(2);
const BMS_POST_FLASH_BOOT_QUIET: Duration = Duration::from_secs(10);
const BMS_WORKING_INFO_PERIOD: Duration = Duration::from_secs(5);
const BMS_FORCE_MIN_CHARGE_REPOWER_OFF: Duration = Duration::from_secs(10);
const BMS_ROM_RECOVER_MIN_INTERVAL: Duration = Duration::from_secs(30);
const BMS_ROM_FORCE_MIN_CHARGE_DWELL: Duration = Duration::from_secs(2);
const BMS_POST_FLASH_RESUME_WINDOW: Duration = Duration::from_secs(30);
const BMS_MAC_TOGGLE_SETTLE: Duration = Duration::from_millis(40);
const BMS_WAKE_KEEPALIVE_GAP: Duration = Duration::from_millis(40);
const BMS_WAKE_KEEPALIVE_ROUNDS: usize = 3;
const BMS_WAKE_READ_GAPS_MS: [u64; 3] = [2, 22, 40];
const BMS_WAKE_TOUCH_READ_GAPS_MS: [u64; 3] = [22, 40, 66];
const BMS_MAC_CMD_GAUGING: u16 = 0x0021;
const BMS_MAC_CMD_FET_CONTROL: u16 = 0x0022;
const BMS_MAC_CMD_OPERATION_STATUS: u16 = 0x0054;
const BMS_MAC_CMD_MANUFACTURING_STATUS: u16 = 0x0057;
const BMS_MFG_STATUS_GAUGE_EN: u32 = 1 << 3;
const BMS_MFG_STATUS_FET_EN: u32 = 1 << 4;
// TI's standalone SREC note sends Execute FW, waits quietly for about 1 s, then polls 0x0D.
// Keep the first post-execute transaction as a plain 0x0D read so we do not disturb the ROM
// -> FW reboot window with extra SMBus probes unless the quiet path has already failed.
const BMS_ROM_EXECUTE_FLASH_FIRST_CHECK: Duration = Duration::from_secs(1);
const BMS_ROM_EXECUTE_FLASH_SETTLE: Duration = Duration::from_millis(4_000);
const BMS_ROM_EXECUTE_FLASH_POLL_WINDOW: Duration = Duration::from_secs(4);
const BMS_ROM_FLASH_WRITE_GAP: Duration = Duration::from_millis(10);
const BMS_ROM_FLASH_WORD_GAP: Duration = Duration::from_millis(50);
const BMS_ROM_FLASH_ERASE_GAP: Duration = Duration::from_secs(1);
const BMS_ROM_FLASH_BLOCK_BYTES_MAX: usize = 64;
// TI's standalone SREC note programs Section1/2 in 64-byte data blocks.
// Our recovery path was still stuck in ROM with 32-byte writes, so fall back
// to the documented block geometry before trying tool-specific variants.
const BMS_ROM_FLASH_BLOCK_BYTES_SEC12: usize = 64;
const BMS_ROM_FLASH_BLOCK_BYTES_SEC3: usize = 32;
const BMS_ROM_INFO_LAYOUT_TAG: &str = if cfg!(feature = "bms-rom-full-info") {
    "full-info"
} else {
    "stock-sparse"
};
#[cfg(all(feature = "bms-rom-image-r3", feature = "bms-rom-image-r5"))]
compile_error!("Select at most one BQ ROM image feature");
#[cfg(all(feature = "bms-rom-full-info", feature = "bms-rom-image-r5"))]
compile_error!("bms-rom-image-r5 currently supports only stock-sparse info writes");
#[cfg(feature = "bms-rom-image-r5")]
const BMS_ROM_FLASH_IMAGE_TAG: &str = "bq40z50-r5-v5.05-build96";
#[cfg(all(not(feature = "bms-rom-image-r5"), feature = "bms-rom-image-r3"))]
const BMS_ROM_FLASH_IMAGE_TAG: &str = "bq40z50-r3-v3.09-build73";
#[cfg(all(not(feature = "bms-rom-image-r5"), not(feature = "bms-rom-image-r3")))]
const BMS_ROM_FLASH_IMAGE_TAG: &str = "bq40z50-r2-v2.11-build52";
#[cfg(feature = "bms-rom-image-r5")]
const BMS_ROM_SECTION1_IMAGE: &[u8] =
    include_bytes!("../../assets/bq40z50_r5_v5_05_build_96/section1.bin");
#[cfg(feature = "bms-rom-image-r5")]
const BMS_ROM_SECTION1_USED_LEN: usize = 0x1614;
#[cfg(all(not(feature = "bms-rom-image-r5"), feature = "bms-rom-image-r3"))]
const BMS_ROM_SECTION1_IMAGE: &[u8] =
    include_bytes!("../../assets/bq40z50_r3_v3_09_build_73/section1.bin");
#[cfg(all(not(feature = "bms-rom-image-r5"), feature = "bms-rom-image-r3"))]
const BMS_ROM_SECTION1_USED_LEN: usize = 0x12AC;
#[cfg(all(not(feature = "bms-rom-image-r5"), not(feature = "bms-rom-image-r3")))]
const BMS_ROM_SECTION1_IMAGE: &[u8] =
    include_bytes!("../../assets/bq40z50_r2_v2_11_build_52/section1.bin");
#[cfg(all(not(feature = "bms-rom-image-r5"), not(feature = "bms-rom-image-r3")))]
const BMS_ROM_SECTION1_USED_LEN: usize = 0x0DEC;
#[cfg(feature = "bms-rom-image-r5")]
const BMS_ROM_SECTION2_IMAGE: &[u8] =
    include_bytes!("../../assets/bq40z50_r5_v5_05_build_96/section2.bin");
#[cfg(feature = "bms-rom-image-r5")]
const BMS_ROM_SECTION2_USED_LEN: usize = 0xDA63;
#[cfg(all(not(feature = "bms-rom-image-r5"), feature = "bms-rom-image-r3"))]
const BMS_ROM_SECTION2_IMAGE: &[u8] =
    include_bytes!("../../assets/bq40z50_r3_v3_09_build_73/section2.bin");
#[cfg(all(not(feature = "bms-rom-image-r5"), feature = "bms-rom-image-r3"))]
const BMS_ROM_SECTION2_USED_LEN: usize = 0xBE23;
#[cfg(all(not(feature = "bms-rom-image-r5"), not(feature = "bms-rom-image-r3")))]
const BMS_ROM_SECTION2_IMAGE: &[u8] =
    include_bytes!("../../assets/bq40z50_r2_v2_11_build_52/section2.bin");
#[cfg(all(not(feature = "bms-rom-image-r5"), not(feature = "bms-rom-image-r3")))]
const BMS_ROM_SECTION2_USED_LEN: usize = 0xB69D;
#[cfg(feature = "bms-rom-image-r5")]
const BMS_ROM_SECTION3_BLK00: &[u8] =
    include_bytes!("../../assets/bq40z50_r5_v5_05_build_96/section3_blk00.bin");
#[cfg(all(not(feature = "bms-rom-image-r5"), feature = "bms-rom-image-r3"))]
const BMS_ROM_SECTION3_BLK00: &[u8] =
    include_bytes!("../../assets/bq40z50_r3_v3_09_build_73/section3_blk00.bin");
#[cfg(all(not(feature = "bms-rom-image-r5"), not(feature = "bms-rom-image-r3")))]
const BMS_ROM_SECTION3_BLK00: &[u8] =
    include_bytes!("../../assets/bq40z50_r2_v2_11_build_52/section3_blk00.bin");
#[cfg(feature = "bms-rom-image-r5")]
const BMS_ROM_SECTION3_BLK80: &[u8] =
    include_bytes!("../../assets/bq40z50_r5_v5_05_build_96/section3_blk80.bin");
#[cfg(all(not(feature = "bms-rom-image-r5"), feature = "bms-rom-image-r3"))]
const BMS_ROM_SECTION3_BLK80: &[u8] =
    include_bytes!("../../assets/bq40z50_r3_v3_09_build_73/section3_blk80.bin");
#[cfg(all(not(feature = "bms-rom-image-r5"), not(feature = "bms-rom-image-r3")))]
const BMS_ROM_SECTION3_BLK80: &[u8] =
    include_bytes!("../../assets/bq40z50_r2_v2_11_build_52/section3_blk80.bin");
#[cfg(feature = "bms-rom-image-r3")]
const BMS_ROM_SECTION3_INFO_IMAGE: &[u8] =
    include_bytes!("../../assets/bq40z50_r3_v3_09_build_73/section3_info.bin");
#[cfg(all(not(feature = "bms-rom-image-r3"), not(feature = "bms-rom-image-r5")))]
const BMS_ROM_SECTION3_INFO_IMAGE: &[u8] =
    include_bytes!("../../assets/bq40z50_r2_v2_11_build_52/section3_info.bin");
#[cfg(feature = "bms-rom-image-r5")]
const BMS_ROM_SECTION4_BLK: &[u8] =
    include_bytes!("../../assets/bq40z50_r5_v5_05_build_96/section4_blk.bin");
#[cfg(all(not(feature = "bms-rom-image-r5"), feature = "bms-rom-image-r3"))]
const BMS_ROM_SECTION4_BLK: &[u8] =
    include_bytes!("../../assets/bq40z50_r3_v3_09_build_73/section4_blk.bin");
#[cfg(all(not(feature = "bms-rom-image-r5"), not(feature = "bms-rom-image-r3")))]
const BMS_ROM_SECTION4_BLK: &[u8] =
    include_bytes!("../../assets/bq40z50_r2_v2_11_build_52/section4_blk.bin");
const BMS_SUSPICIOUS_VOLTAGE_MV: u16 = 5_911;
const BMS_SUSPICIOUS_CURRENT_MA: i16 = 5_911;
const BMS_SUSPICIOUS_STATUS: u16 = 0x1717;
const BMS_ROM_MODE_SIGNATURE: u16 = 0x9002;
// TI docs describe a ~2 s CHECK_WAKE communication window after the pack sees a wake event.
// Keep staged probes inside that window so the tool can deliver a valid SMBus transaction
// before the gauge decides the wake was unintended and drops back out again.
const BMS_WAKE_WINDOW_PROBE_DELAYS_MS: [u64; 3] = [0, 800, 1_600];
// After the staged wake probes miss, keep minimum charge applied and repeatedly send
// valid gauge-address commands for a few seconds. This explicitly exercises the
// documented EMSHUT/SHUTDOWN communication exit path before we conclude the gauge is mute.
const BMS_EXIT_EXERCISE_WINDOW: Duration = Duration::from_secs(6);
const BMS_EXIT_EXERCISE_PERIOD: Duration = Duration::from_millis(500);
const BMS_DIAG_SCAN_INTERVAL: Duration = Duration::from_secs(30);
const BMS_MISSING_VERBOSE_REPROBE_INTERVAL: Duration = Duration::from_secs(30);
const BMS_DIAG_SCAN_MIN_ADDR: u8 = 0x03;
const BMS_DIAG_SCAN_MAX_ADDR: u8 = 0x77;
const BMS_SHIP_RESET_DELAY: Duration = Duration::from_secs(20);
const BMS_SHIP_RESET_SETTLE: Duration = Duration::from_millis(800);

#[derive(Clone, Copy, PartialEq, Eq)]
struct BmsSignature {
    voltage_mv: u16,
    current_ma: i16,
    soc_pct: u16,
    status_raw: u16,
}

#[derive(Clone, Copy)]
struct BmsPatternTracker {
    last_signature: Option<BmsSignature>,
    repeat_count: u8,
}

impl BmsPatternTracker {
    const fn new() -> Self {
        Self {
            last_signature: None,
            repeat_count: 0,
        }
    }

    fn observe(&mut self, signature: BmsSignature) -> u8 {
        if self.last_signature == Some(signature) {
            self.repeat_count = self.repeat_count.saturating_add(1);
        } else {
            self.repeat_count = 1;
            self.last_signature = Some(signature);
        }
        self.repeat_count
    }
}

#[derive(Clone, Copy)]
struct ValidatedBmsSnapshot {
    temp_c_x10: i32,
    voltage_mv: u16,
    current_ma: i16,
    soc_pct: u16,
    status_raw: u16,
    cell1_mv: u16,
    cell2_mv: u16,
    cell3_mv: u16,
    cell4_mv: u16,
    err_code: u8,
    remaining_cap_mah: Result<u16, &'static str>,
    full_cap_mah: Result<u16, &'static str>,
}

#[derive(Clone, Copy)]
struct BmsMacProbeSnapshot {
    declared_len: u8,
    payload_len: u8,
    b0: u8,
    b1: u8,
    b2: u8,
    b3: u8,
}

fn log_bms_diag(
    addr: u8,
    stage: &'static str,
    err: bq40z50::BmsDiagError,
    raw: &'static str,
    parsed: &'static str,
) {
    defmt::warn!(
        "bms_diag: addr=0x{=u8:x} stage={} err={} raw={} parsed={}",
        addr,
        stage,
        err,
        raw,
        parsed
    );
}

fn read_bms_mac_probe_checked<I2C>(
    i2c: &mut I2C,
    addr: u8,
) -> Result<BmsMacProbeSnapshot, bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    // ManufacturerAccess() 0x0001 DeviceType query via ManufacturerData() (0x23).
    // Per TRM, data bytes for cmd 0x00 are written MSB-first.
    i2c.write(addr, &[bq40z50::cmd::MANUFACTURER_ACCESS, 0x00, 0x01])
        .map_err(|_| bq40z50::BmsDiagError::I2cNack)?;
    spin_delay(BMS_MAC_WRITE_SETTLE);
    let raw = bq40z50::read_block_raw_checked(i2c, addr, bq40z50::cmd::MANUFACTURER_DATA)?;
    let payload_len = raw.payload_len as usize;

    let b0 = if payload_len > 0 { raw.payload[0] } else { 0 };
    let b1 = if payload_len > 1 { raw.payload[1] } else { 0 };
    let b2 = if payload_len > 2 { raw.payload[2] } else { 0 };
    let b3 = if payload_len > 3 { raw.payload[3] } else { 0 };

    // Temporary liveness gate: reject the known ghost frame signature.
    let looks_like_ghost = raw.declared_len == 23
        && raw.payload_len >= 8
        && raw.payload[..(raw.payload_len as usize)]
            .iter()
            .all(|b| *b == 0x17);
    if looks_like_ghost {
        return Err(bq40z50::BmsDiagError::StalePattern);
    }

    Ok(BmsMacProbeSnapshot {
        declared_len: raw.declared_len,
        payload_len: raw.payload_len,
        b0,
        b1,
        b2,
        b3,
    })
}

fn read_bms_mac_block_via_md23<I2C>(
    i2c: &mut I2C,
    addr: u8,
    mac_cmd: u16,
) -> Result<bq40z50::BlockReadRaw, bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    // ManufacturerAccess() writes use MSB-first ordering on cmd 0x00, then the reply is read
    // back from ManufacturerData() (0x23).
    i2c.write(
        addr,
        &[
            bq40z50::cmd::MANUFACTURER_ACCESS,
            (mac_cmd >> 8) as u8,
            (mac_cmd & 0x00FF) as u8,
        ],
    )
    .map_err(|_| bq40z50::BmsDiagError::I2cNack)?;
    spin_delay(BMS_MAC_WRITE_SETTLE);
    bq40z50::read_block_raw_checked(i2c, addr, bq40z50::cmd::MANUFACTURER_DATA)
}

fn read_bms_mac_block_via_mb44<I2C>(
    i2c: &mut I2C,
    addr: u8,
    mac_cmd: u16,
) -> Result<bq40z50::BlockReadRaw, bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let cmd = mac_cmd.to_le_bytes();
    let direct = [
        bq40z50::cmd::MANUFACTURER_BLOCK_ACCESS,
        0x02,
        cmd[0],
        cmd[1],
    ];
    let addr_w = addr << 1;
    let pec = crc8_smbus(&[addr_w, direct[0], direct[1], direct[2], direct[3]]);
    let with_pec = [direct[0], direct[1], direct[2], direct[3], pec];
    let mut last_err = bq40z50::BmsDiagError::I2cNack;

    for frame in [&direct[..], &with_pec[..]] {
        if i2c.write(addr, frame).is_err() {
            continue;
        }
        spin_delay(BMS_MAC_WRITE_SETTLE);
        match bq40z50::read_block_raw_checked(i2c, addr, bq40z50::cmd::MANUFACTURER_BLOCK_ACCESS) {
            Ok(raw) => return Ok(raw),
            Err(e) => last_err = e,
        }
    }

    Err(last_err)
}

fn parse_bms_mac_u32(raw: &bq40z50::BlockReadRaw, mac_cmd: u16) -> Option<u32> {
    let payload_len = raw.payload_len as usize;
    let cmd = mac_cmd.to_le_bytes();
    if payload_len >= 6 && raw.payload[0] == cmd[0] && raw.payload[1] == cmd[1] {
        return Some(u32::from_le_bytes([
            raw.payload[2],
            raw.payload[3],
            raw.payload[4],
            raw.payload[5],
        ]));
    }
    if payload_len >= 4 {
        return Some(u32::from_le_bytes([
            raw.payload[0],
            raw.payload[1],
            raw.payload[2],
            raw.payload[3],
        ]));
    }
    None
}

fn read_bms_mac_u32<I2C>(
    i2c: &mut I2C,
    addr: u8,
    mac_cmd: u16,
) -> Result<u32, bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    if let Ok(raw) = read_bms_mac_block_via_md23(i2c, addr, mac_cmd) {
        if let Some(value) = parse_bms_mac_u32(&raw, mac_cmd) {
            return Ok(value);
        }
    }

    let raw = read_bms_mac_block_via_mb44(i2c, addr, mac_cmd)?;
    parse_bms_mac_u32(&raw, mac_cmd).ok_or(bq40z50::BmsDiagError::BadBlockLen)
}

fn send_bms_manufacturer_toggle<I2C>(
    i2c: &mut I2C,
    addr: u8,
    mac_cmd: u16,
    stage: &'static str,
    quiet: bool,
) -> Result<(), bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let direct = [
        bq40z50::cmd::MANUFACTURER_ACCESS,
        (mac_cmd >> 8) as u8,
        (mac_cmd & 0x00FF) as u8,
    ];
    let addr_w = addr << 1;
    let pec = crc8_smbus(&[addr_w, direct[0], direct[1], direct[2]]);
    let with_pec = [direct[0], direct[1], direct[2], pec];

    for (via, frame) in [("direct", &direct[..]), ("pec", &with_pec[..])] {
        if i2c.write(addr, frame).is_ok() {
            if !quiet {
                defmt::warn!(
                    "bms_diag: addr=0x{=u8:x} stage={} mac_cmd=0x{=u16:x} via={}",
                    addr,
                    stage,
                    mac_cmd,
                    via,
                );
            }
            spin_delay(BMS_MAC_TOGGLE_SETTLE);
            return Ok(());
        }
    }

    Err(bq40z50::BmsDiagError::I2cNack)
}

fn maybe_enable_bms_runtime_after_flash<I2C>(i2c: &mut I2C, addr: u8, quiet: bool)
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let mut mfg_status = match read_bms_mac_u32(i2c, addr, BMS_MAC_CMD_MANUFACTURING_STATUS) {
        Ok(bits) => {
            if !quiet {
                defmt::warn!(
                    "bms_diag: addr=0x{=u8:x} stage=post_flash_mfg_status bits=0x{=u32:x} gauge_en={=bool} fet_en={=bool}",
                    addr,
                    bits,
                    (bits & BMS_MFG_STATUS_GAUGE_EN) != 0,
                    (bits & BMS_MFG_STATUS_FET_EN) != 0,
                );
            }
            Some(bits)
        }
        Err(e) => {
            if !quiet {
                log_bms_diag(addr, "post_flash_mfg_status", e, "block", "mac");
            }
            None
        }
    };

    if let Some(bits) = mfg_status {
        if bits & BMS_MFG_STATUS_GAUGE_EN == 0 {
            if let Err(e) = send_bms_manufacturer_toggle(
                i2c,
                addr,
                BMS_MAC_CMD_GAUGING,
                "post_flash_gauge_en",
                quiet,
            ) {
                if !quiet {
                    log_bms_diag(addr, "post_flash_gauge_en", e, "word", "mac");
                }
            }
        }
        if bits & BMS_MFG_STATUS_FET_EN == 0 {
            if let Err(e) = send_bms_manufacturer_toggle(
                i2c,
                addr,
                BMS_MAC_CMD_FET_CONTROL,
                "post_flash_fet_en",
                quiet,
            ) {
                if !quiet {
                    log_bms_diag(addr, "post_flash_fet_en", e, "word", "mac");
                }
            }
        }

        mfg_status = match read_bms_mac_u32(i2c, addr, BMS_MAC_CMD_MANUFACTURING_STATUS) {
            Ok(bits_after) => {
                if !quiet {
                    defmt::warn!(
                        "bms_diag: addr=0x{=u8:x} stage=post_flash_mfg_status_after bits=0x{=u32:x} gauge_en={=bool} fet_en={=bool}",
                        addr,
                        bits_after,
                        (bits_after & BMS_MFG_STATUS_GAUGE_EN) != 0,
                        (bits_after & BMS_MFG_STATUS_FET_EN) != 0,
                    );
                }
                Some(bits_after)
            }
            Err(e) => {
                if !quiet {
                    log_bms_diag(addr, "post_flash_mfg_status_after", e, "block", "mac");
                }
                mfg_status
            }
        };
    }

    if let Ok(op_status) = read_bms_mac_u32(i2c, addr, BMS_MAC_CMD_OPERATION_STATUS) {
        if !quiet {
            defmt::warn!(
                "bms_diag: addr=0x{=u8:x} stage=post_flash_op_status bits=0x{=u32:x} emshut={=bool} init={=bool} xchg={=bool} xdsg={=bool} ss={=bool} chg={=bool} dsg={=bool} pres={=bool}",
                addr,
                op_status,
                (op_status & (1 << 29)) != 0,
                (op_status & (1 << 24)) != 0,
                (op_status & (1 << 14)) != 0,
                (op_status & (1 << 13)) != 0,
                (op_status & (1 << 11)) != 0,
                (op_status & (1 << 2)) != 0,
                (op_status & (1 << 1)) != 0,
                (op_status & (1 << 0)) != 0,
            );
        }
    } else if !quiet {
        log_bms_diag(
            addr,
            "post_flash_op_status",
            bq40z50::BmsDiagError::I2cNack,
            "block",
            "mac",
        );
    }
}

fn is_bms_ghost_block(raw: &bq40z50::BlockReadRaw) -> bool {
    raw.declared_len == 23
        && raw.payload_len >= 8
        && raw.payload[..(raw.payload_len as usize)]
            .iter()
            .all(|b| *b == 0x17)
}

fn is_bms_mb44_reply_for_cmd(raw: &bq40z50::BlockReadRaw, mac_cmd: u16) -> bool {
    let cmd = mac_cmd.to_le_bytes();
    raw.payload_len >= 4
        && raw.payload[0] == cmd[0]
        && raw.payload[1] == cmd[1]
        && !is_bms_ghost_block(raw)
}

fn log_execute_fw_window_probe<I2C>(
    i2c: &mut I2C,
    addr: u8,
    stage: &'static str,
    attempt: u8,
    quiet: bool,
) -> bool
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let mut saw_fw = false;

    if let Ok(raw) = bq40z50::read_block_raw_checked(i2c, addr, bq40z50::cmd::LIFETIME_DATA_BLOCK_2)
    {
        saw_fw = true;
        let payload_len = raw.payload_len as usize;
        let payload = &raw.payload[..payload_len];
        if !quiet {
            defmt::warn!(
                "bms_diag: addr=0x{=u8:x} stage={} fw_probe=lt2 attempt={=u8} len={=u8} payload={=u8} b0=0x{=u8:x} b1=0x{=u8:x} b2=0x{=u8:x} b3=0x{=u8:x} b4=0x{=u8:x} b5=0x{=u8:x} b6=0x{=u8:x} b7=0x{=u8:x}",
                addr,
                stage,
                attempt,
                raw.declared_len,
                raw.payload_len,
                payload.get(0).copied().unwrap_or(0),
                payload.get(1).copied().unwrap_or(0),
                payload.get(2).copied().unwrap_or(0),
                payload.get(3).copied().unwrap_or(0),
                payload.get(4).copied().unwrap_or(0),
                payload.get(5).copied().unwrap_or(0),
                payload.get(6).copied().unwrap_or(0),
                payload.get(7).copied().unwrap_or(0),
            );
        }
    }

    if let Ok(raw) = bq40z50::read_block_raw_checked(i2c, addr, bq40z50::cmd::PF_STATUS) {
        saw_fw = true;
        let payload_len = raw.payload_len as usize;
        let payload = &raw.payload[..payload_len];
        let mut pf_bytes = [0u8; 4];
        let copy_len = payload_len.min(4);
        pf_bytes[..copy_len].copy_from_slice(&payload[..copy_len]);
        let pf_le = u32::from_le_bytes(pf_bytes);
        let pf_be = u32::from_be_bytes(pf_bytes);
        if !quiet {
            defmt::warn!(
                "bms_diag: addr=0x{=u8:x} stage={} fw_probe=pf_status attempt={=u8} len={=u8} payload={=u8} b0=0x{=u8:x} b1=0x{=u8:x} b2=0x{=u8:x} b3=0x{=u8:x} le=0x{=u32:x} be=0x{=u32:x} ifc_le={=bool} ifc_be={=bool}",
                addr,
                stage,
                attempt,
                raw.declared_len,
                raw.payload_len,
                payload.get(0).copied().unwrap_or(0),
                payload.get(1).copied().unwrap_or(0),
                payload.get(2).copied().unwrap_or(0),
                payload.get(3).copied().unwrap_or(0),
                pf_le,
                pf_be,
                (pf_le & (1 << 24)) != 0,
                (pf_be & (1 << 24)) != 0,
            );
        }
    }

    for (probe_name, mac_cmd) in [("fw_ver_mb44", 0x0002u16), ("if_sig_mb44", 0x0004u16)] {
        if let Ok(raw) = read_bms_mac_block_via_mb44(i2c, addr, mac_cmd) {
            let payload_len = raw.payload_len as usize;
            let payload = &raw.payload[..payload_len];
            let valid = is_bms_mb44_reply_for_cmd(&raw, mac_cmd);
            if valid {
                saw_fw = true;
            }
            if !quiet {
                defmt::warn!(
                    "bms_diag: addr=0x{=u8:x} stage={} fw_probe={} attempt={=u8} valid={=bool} len={=u8} payload={=u8} b0=0x{=u8:x} b1=0x{=u8:x} b2=0x{=u8:x} b3=0x{=u8:x} b4=0x{=u8:x} b5=0x{=u8:x}",
                    addr,
                    stage,
                    probe_name,
                    attempt,
                    valid,
                    raw.declared_len,
                    raw.payload_len,
                    payload.get(0).copied().unwrap_or(0),
                    payload.get(1).copied().unwrap_or(0),
                    payload.get(2).copied().unwrap_or(0),
                    payload.get(3).copied().unwrap_or(0),
                    payload.get(4).copied().unwrap_or(0),
                    payload.get(5).copied().unwrap_or(0),
                );
            }
        }
    }

    if let Ok(raw) = read_bms_mac_block_via_md23(i2c, addr, 0x0004) {
        let payload_len = raw.payload_len as usize;
        let payload = &raw.payload[..payload_len];
        let ghost = is_bms_ghost_block(&raw);
        if !quiet {
            defmt::warn!(
                "bms_diag: addr=0x{=u8:x} stage={} fw_probe={} attempt={=u8} len={=u8} payload={=u8} b0=0x{=u8:x} b1=0x{=u8:x} b2=0x{=u8:x} b3=0x{=u8:x}",
                addr,
                stage,
                if ghost { "if_sig_md23_ghost" } else { "if_sig_md23" },
                attempt,
                raw.declared_len,
                raw.payload_len,
                payload.get(0).copied().unwrap_or(0),
                payload.get(1).copied().unwrap_or(0),
                payload.get(2).copied().unwrap_or(0),
                payload.get(3).copied().unwrap_or(0),
            );
        }
    }

    saw_fw
}

fn read_ascii_block_checked<'a, I2C>(
    i2c: &mut I2C,
    addr: u8,
    cmd: u8,
    scratch: &'a mut [u8; 32],
) -> Result<&'a str, bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let raw = bq40z50::read_block_raw_checked(i2c, addr, cmd)?;
    let payload_len = raw.payload_len as usize;
    let payload = &raw.payload[..payload_len];
    if payload
        .iter()
        .any(|b| !((0x20..=0x7E).contains(b) || *b == b'\t'))
    {
        return Err(bq40z50::BmsDiagError::BadAscii);
    }
    scratch[..payload_len].copy_from_slice(payload);
    core::str::from_utf8(&scratch[..payload_len]).map_err(|_| bq40z50::BmsDiagError::BadAscii)
}

fn read_u16_consistent<I2C>(
    i2c: &mut I2C,
    addr: u8,
    cmd: u8,
    tolerance: u16,
) -> Result<u16, bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let a = read_u16_with_optional_pec(i2c, addr, cmd)?;
    if a == BMS_SUSPICIOUS_STATUS || a == BMS_ROM_MODE_SIGNATURE {
        return Ok(a);
    }
    spin_delay(BMS_WORD_GAP);
    let b = read_u16_with_optional_pec(i2c, addr, cmd)?;
    let ab_diff = a.max(b) - a.min(b);
    if ab_diff <= tolerance {
        return Ok(b);
    }

    // Guard against one-off corrupted reads: take a third sample and accept it if it matches
    // either previous sample within tolerance.
    spin_delay(BMS_WORD_GAP);
    let c = read_u16_with_optional_pec(i2c, addr, cmd)?;
    let ac_diff = a.max(c) - a.min(c);
    if ac_diff <= tolerance {
        return Ok(c);
    }
    let bc_diff = b.max(c) - b.min(c);
    if bc_diff <= tolerance {
        return Ok(c);
    }

    Err(bq40z50::BmsDiagError::InconsistentSample)
}

fn read_i16_consistent<I2C>(
    i2c: &mut I2C,
    addr: u8,
    cmd: u8,
    tolerance: i16,
) -> Result<i16, bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let a = read_i16_with_optional_pec(i2c, addr, cmd)?;
    if a == BMS_SUSPICIOUS_CURRENT_MA || a == BMS_ROM_MODE_SIGNATURE as i16 {
        return Ok(a);
    }
    spin_delay(BMS_WORD_GAP);
    let b = read_i16_with_optional_pec(i2c, addr, cmd)?;
    let ab_diff = (a as i32 - b as i32).abs();
    if ab_diff <= i32::from(tolerance) {
        return Ok(b);
    }

    spin_delay(BMS_WORD_GAP);
    let c = read_i16_with_optional_pec(i2c, addr, cmd)?;
    let ac_diff = (a as i32 - c as i32).abs();
    if ac_diff <= i32::from(tolerance) {
        return Ok(c);
    }
    let bc_diff = (b as i32 - c as i32).abs();
    if bc_diff <= i32::from(tolerance) {
        return Ok(c);
    }

    Err(bq40z50::BmsDiagError::InconsistentSample)
}

fn crc8_smbus(bytes: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &byte in bytes {
        crc ^= byte;
        for _ in 0..8 {
            if (crc & 0x80) != 0 {
                crc = (crc << 1) ^ 0x07;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

fn spin_delay(wait: Duration) {
    let start = Instant::now();
    while start.elapsed() < wait {}
}

fn spin_until_elapsed(start: Instant, elapsed: Duration) {
    while start.elapsed() < elapsed {}
}

#[derive(Clone, Copy)]
struct WordDiagRaw {
    err: &'static str,
    lo: u8,
    hi: u8,
}

#[derive(Clone, Copy)]
struct WordDiagSplit {
    err: &'static str,
    write_err: &'static str,
    read_err: &'static str,
    lo: u8,
    hi: u8,
}

#[derive(Clone, Copy)]
struct WordDiagReadOnly {
    err: &'static str,
    len: u8,
    b0: u8,
    b1: u8,
}

fn word_diag_write_read<I2C>(i2c: &mut I2C, addr: u8, cmd: u8) -> WordDiagRaw
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let mut buf = [0u8; 2];
    match i2c.write_read(addr, &[cmd], &mut buf) {
        Ok(()) => WordDiagRaw {
            err: "ok",
            lo: buf[0],
            hi: buf[1],
        },
        Err(e) => WordDiagRaw {
            err: i2c_error_kind(e),
            lo: 0,
            hi: 0,
        },
    }
}

fn word_diag_split<I2C>(i2c: &mut I2C, addr: u8, cmd: u8) -> WordDiagSplit
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let write_err = match i2c.write(addr, &[cmd]) {
        Ok(()) => "ok",
        Err(e) => {
            return WordDiagSplit {
                err: i2c_error_kind(e),
                write_err: i2c_error_kind(e),
                read_err: "skip",
                lo: 0,
                hi: 0,
            };
        }
    };

    spin_delay(BMS_WORD_GAP);
    let mut buf = [0u8; 2];
    match i2c.read(addr, &mut buf) {
        Ok(()) => WordDiagSplit {
            err: "ok",
            write_err,
            read_err: "ok",
            lo: buf[0],
            hi: buf[1],
        },
        Err(e) => WordDiagSplit {
            err: i2c_error_kind(e),
            write_err,
            read_err: i2c_error_kind(e),
            lo: 0,
            hi: 0,
        },
    }
}

fn word_diag_read_only<I2C>(i2c: &mut I2C, addr: u8, len: usize) -> WordDiagReadOnly
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let len = len.min(2);
    let mut buf = [0u8; 2];
    match i2c.read(addr, &mut buf[..len]) {
        Ok(()) => WordDiagReadOnly {
            err: "ok",
            len: len as u8,
            b0: buf[0],
            b1: buf[1],
        },
        Err(e) => WordDiagReadOnly {
            err: i2c_error_kind(e),
            len: len as u8,
            b0: 0,
            b1: 0,
        },
    }
}

#[derive(Clone, Copy)]
struct WordDiagPec {
    err: &'static str,
    lo: u8,
    hi: u8,
    rx_pec: u8,
    expect_pec: u8,
}

#[derive(Clone, Copy)]
struct WordDiagBlock {
    err: &'static str,
    declared_len: u8,
    payload_len: u8,
    b0: u8,
    b1: u8,
    b2: u8,
}

#[derive(Clone, Copy)]
struct WordDiagBlockSplit {
    err: &'static str,
    write_err: &'static str,
    read_err: &'static str,
    declared_len: u8,
    payload_len: u8,
    b0: u8,
    b1: u8,
    b2: u8,
}

fn word_diag_block<I2C>(i2c: &mut I2C, addr: u8, cmd: u8) -> WordDiagBlock
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    match bq40z50::read_block_raw_checked(i2c, addr, cmd) {
        Ok(raw) => {
            let payload_len = raw.payload_len as usize;
            WordDiagBlock {
                err: "ok",
                declared_len: raw.declared_len,
                payload_len: raw.payload_len,
                b0: if payload_len > 0 { raw.payload[0] } else { 0 },
                b1: if payload_len > 1 { raw.payload[1] } else { 0 },
                b2: if payload_len > 2 { raw.payload[2] } else { 0 },
            }
        }
        Err(e) => WordDiagBlock {
            err: e.as_str(),
            declared_len: 0,
            payload_len: 0,
            b0: 0,
            b1: 0,
            b2: 0,
        },
    }
}

fn word_diag_block_split<I2C>(i2c: &mut I2C, addr: u8, cmd: u8) -> WordDiagBlockSplit
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let write_err = match i2c.write(addr, &[cmd]) {
        Ok(()) => "ok",
        Err(e) => {
            return WordDiagBlockSplit {
                err: i2c_error_kind(e),
                write_err: i2c_error_kind(e),
                read_err: "skip",
                declared_len: 0,
                payload_len: 0,
                b0: 0,
                b1: 0,
                b2: 0,
            };
        }
    };

    spin_delay(BMS_WORD_GAP);
    let mut buf = [0u8; 33];
    match i2c.read(addr, &mut buf) {
        Ok(()) => {
            let declared_len = buf[0];
            let payload_len = declared_len.min(32);
            let payload_len_usize = payload_len as usize;
            WordDiagBlockSplit {
                err: if declared_len == 0 || declared_len > 32 {
                    "bad_len"
                } else {
                    "ok"
                },
                write_err,
                read_err: "ok",
                declared_len,
                payload_len,
                b0: if payload_len_usize > 0 { buf[1] } else { 0 },
                b1: if payload_len_usize > 1 { buf[2] } else { 0 },
                b2: if payload_len_usize > 2 { buf[3] } else { 0 },
            }
        }
        Err(e) => WordDiagBlockSplit {
            err: i2c_error_kind(e),
            write_err,
            read_err: i2c_error_kind(e),
            declared_len: 0,
            payload_len: 0,
            b0: 0,
            b1: 0,
            b2: 0,
        },
    }
}

fn word_diag_pec<I2C>(i2c: &mut I2C, addr: u8, cmd: u8) -> WordDiagPec
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let mut buf = [0u8; 3];
    if let Err(e) = i2c.write_read(addr, &[cmd], &mut buf) {
        return WordDiagPec {
            err: i2c_error_kind(e),
            lo: 0,
            hi: 0,
            rx_pec: 0,
            expect_pec: 0,
        };
    }

    let addr_w = addr << 1;
    let addr_r = addr_w | 1;
    let expect_pec = crc8_smbus(&[addr_w, cmd, addr_r, buf[0], buf[1]]);
    let err = if expect_pec == buf[2] {
        "ok"
    } else {
        "pec_mismatch"
    };

    WordDiagPec {
        err,
        lo: buf[0],
        hi: buf[1],
        rx_pec: buf[2],
        expect_pec,
    }
}

fn log_bms_word_diag_for_cmd<I2C>(i2c: &mut I2C, addr: u8, cmd: u8, name: &'static str)
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let wr = word_diag_write_read(i2c, addr, cmd);
    let split = word_diag_split(i2c, addr, cmd);
    let pec = word_diag_pec(i2c, addr, cmd);
    let blk = word_diag_block(i2c, addr, cmd);
    let blk_split = word_diag_block_split(i2c, addr, cmd);
    let read1 = word_diag_read_only(i2c, addr, 1);
    let read2 = word_diag_read_only(i2c, addr, 2);

    defmt::warn!(
        "bms_diag_word: addr=0x{=u8:x} cmd=0x{=u8:x} name={} wr={} wr_raw=0x{=u8:x} 0x{=u8:x} split={} split_wr={} split_rd={} split_raw=0x{=u8:x} 0x{=u8:x} pec={} pec_raw=0x{=u8:x} 0x{=u8:x} rx_pec=0x{=u8:x} exp_pec=0x{=u8:x} blk={} blk_len={=u8} blk_payload={=u8} blk_b0=0x{=u8:x} blk_b1=0x{=u8:x} blk_b2=0x{=u8:x} blk_split={} blk_split_wr={} blk_split_rd={} blk_split_len={=u8} blk_split_payload={=u8} blk_split_b0=0x{=u8:x} blk_split_b1=0x{=u8:x} blk_split_b2=0x{=u8:x} raw_read1={} raw_read1_len={=u8} raw_read1_b0=0x{=u8:x} raw_read2={} raw_read2_len={=u8} raw_read2_b0=0x{=u8:x} raw_read2_b1=0x{=u8:x}",
        addr,
        cmd,
        name,
        wr.err,
        wr.lo,
        wr.hi,
        split.err,
        split.write_err,
        split.read_err,
        split.lo,
        split.hi,
        pec.err,
        pec.lo,
        pec.hi,
        pec.rx_pec,
        pec.expect_pec,
        blk.err,
        blk.declared_len,
        blk.payload_len,
        blk.b0,
        blk.b1,
        blk.b2,
        blk_split.err,
        blk_split.write_err,
        blk_split.read_err,
        blk_split.declared_len,
        blk_split.payload_len,
        blk_split.b0,
        blk_split.b1,
        blk_split.b2,
        read1.err,
        read1.len,
        read1.b0,
        read2.err,
        read2.len,
        read2.b0,
        read2.b1,
    );
}

fn log_bms_word_diag_set<I2C>(
    i2c: &mut I2C,
    addr: u8,
    stage: &'static str,
    err: bq40z50::BmsDiagError,
) where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let temp_wr = word_diag_write_read(i2c, addr, bq40z50::cmd::TEMPERATURE);
    let temp_split = word_diag_split(i2c, addr, bq40z50::cmd::TEMPERATURE);
    let rsoc_wr = word_diag_write_read(i2c, addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE);
    let rsoc_split = word_diag_split(i2c, addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE);
    let md23 = word_diag_block_split(i2c, addr, bq40z50::cmd::MANUFACTURER_DATA);
    let mb44 = word_diag_block_split(i2c, addr, bq40z50::cmd::MANUFACTURER_BLOCK_ACCESS);
    let raw_read1 = word_diag_read_only(i2c, addr, 1);
    let raw_read2 = word_diag_read_only(i2c, addr, 2);

    defmt::warn!(
        "bms_diag_compact: addr=0x{=u8:x} stage={} err={} temp_wr={} temp_split={} rsoc_wr={} rsoc_split={} md23={} md23_wr={} md23_rd={} mb44={} mb44_wr={} mb44_rd={} raw1={} raw1_len={=u8} raw1_b0=0x{=u8:x} raw2={} raw2_len={=u8} raw2_b0=0x{=u8:x} raw2_b1=0x{=u8:x}",
        addr,
        stage,
        err,
        temp_wr.err,
        temp_split.err,
        rsoc_wr.err,
        rsoc_split.err,
        md23.err,
        md23.write_err,
        md23.read_err,
        mb44.err,
        mb44.write_err,
        mb44.read_err,
        raw_read1.err,
        raw_read1.len,
        raw_read1.b0,
        raw_read2.err,
        raw_read2.len,
        raw_read2.b0,
        raw_read2.b1,
    );
}

fn log_bms_mac_diag<I2C>(i2c: &mut I2C, addr: u8)
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    // Try ManufacturerAccess() 0x0001 (DeviceType) legacy path:
    // write word to 0x00 (MSB first per TRM), then read block from 0x23.
    let mac_write = i2c.write(addr, &[bq40z50::cmd::MANUFACTURER_ACCESS, 0x00, 0x01]);
    spin_delay(BMS_MAC_WRITE_SETTLE);
    let mac_read = bq40z50::read_block_raw_checked(i2c, addr, bq40z50::cmd::MANUFACTURER_DATA);

    match mac_read {
        Ok(raw) => {
            let payload_len = raw.payload_len as usize;
            let b0 = if payload_len > 0 { raw.payload[0] } else { 0 };
            let b1 = if payload_len > 1 { raw.payload[1] } else { 0 };
            let b2 = if payload_len > 2 { raw.payload[2] } else { 0 };
            let b3 = if payload_len > 3 { raw.payload[3] } else { 0 };
            defmt::warn!(
                "bms_diag_mac: path=ma00->md23 write={} read=ok len={=u8} payload={=u8} b0=0x{=u8:x} b1=0x{=u8:x} b2=0x{=u8:x} b3=0x{=u8:x}",
                mac_write.map(|_| "ok").unwrap_or_else(i2c_error_kind),
                raw.declared_len,
                raw.payload_len,
                b0,
                b1,
                b2,
                b3
            );
        }
        Err(e) => {
            defmt::warn!(
                "bms_diag_mac: path=ma00->md23 write={} read={} ",
                mac_write.map(|_| "ok").unwrap_or_else(i2c_error_kind),
                e
            );
        }
    }

    // Try MA path with explicit SMBus PEC on the write transaction.
    let ma_pec = crc8_smbus(&[(addr << 1), bq40z50::cmd::MANUFACTURER_ACCESS, 0x00, 0x01]);
    let mac_write_pec = i2c.write(
        addr,
        &[bq40z50::cmd::MANUFACTURER_ACCESS, 0x00, 0x01, ma_pec],
    );
    spin_delay(BMS_MAC_WRITE_SETTLE);
    let mac_read_pec = bq40z50::read_block_raw_checked(i2c, addr, bq40z50::cmd::MANUFACTURER_DATA);
    match mac_read_pec {
        Ok(raw) => {
            let payload_len = raw.payload_len as usize;
            let b0 = if payload_len > 0 { raw.payload[0] } else { 0 };
            let b1 = if payload_len > 1 { raw.payload[1] } else { 0 };
            let b2 = if payload_len > 2 { raw.payload[2] } else { 0 };
            let b3 = if payload_len > 3 { raw.payload[3] } else { 0 };
            defmt::warn!(
                "bms_diag_mac: path=ma00_pec->md23 write={} read=ok len={=u8} payload={=u8} b0=0x{=u8:x} b1=0x{=u8:x} b2=0x{=u8:x} b3=0x{=u8:x}",
                mac_write_pec.map(|_| "ok").unwrap_or_else(i2c_error_kind),
                raw.declared_len,
                raw.payload_len,
                b0,
                b1,
                b2,
                b3
            );
        }
        Err(e) => {
            defmt::warn!(
                "bms_diag_mac: path=ma00_pec->md23 write={} read={} ",
                mac_write_pec.map(|_| "ok").unwrap_or_else(i2c_error_kind),
                e
            );
        }
    }

    // Try ManufacturerBlockAccess() 0x44 SMBus-block path:
    // write block [len=2, cmd_lsb, cmd_msb], then read block from 0x44.
    let mba_block_write = i2c.write(
        addr,
        &[bq40z50::cmd::MANUFACTURER_BLOCK_ACCESS, 0x02, 0x01, 0x00],
    );
    spin_delay(BMS_MAC_WRITE_SETTLE);
    let mba_block_read =
        bq40z50::read_block_raw_checked(i2c, addr, bq40z50::cmd::MANUFACTURER_BLOCK_ACCESS);

    match mba_block_read {
        Ok(raw) => {
            let payload_len = raw.payload_len as usize;
            let b0 = if payload_len > 0 { raw.payload[0] } else { 0 };
            let b1 = if payload_len > 1 { raw.payload[1] } else { 0 };
            let b2 = if payload_len > 2 { raw.payload[2] } else { 0 };
            let b3 = if payload_len > 3 { raw.payload[3] } else { 0 };
            defmt::warn!(
                "bms_diag_mac: path=mb44_block->mb44 write={} read=ok len={=u8} payload={=u8} b0=0x{=u8:x} b1=0x{=u8:x} b2=0x{=u8:x} b3=0x{=u8:x}",
                mba_block_write.map(|_| "ok").unwrap_or_else(i2c_error_kind),
                raw.declared_len,
                raw.payload_len,
                b0,
                b1,
                b2,
                b3
            );
        }
        Err(e) => {
            defmt::warn!(
                "bms_diag_mac: path=mb44_block->mb44 write={} read={} ",
                mba_block_write.map(|_| "ok").unwrap_or_else(i2c_error_kind),
                e
            );
        }
    }

    let mba_block_pec = crc8_smbus(&[
        addr << 1,
        bq40z50::cmd::MANUFACTURER_BLOCK_ACCESS,
        0x02,
        0x01,
        0x00,
    ]);
    let mba_block_write_pec = i2c.write(
        addr,
        &[
            bq40z50::cmd::MANUFACTURER_BLOCK_ACCESS,
            0x02,
            0x01,
            0x00,
            mba_block_pec,
        ],
    );
    spin_delay(BMS_MAC_WRITE_SETTLE);
    let mba_block_read_pec =
        bq40z50::read_block_raw_checked(i2c, addr, bq40z50::cmd::MANUFACTURER_BLOCK_ACCESS);
    match mba_block_read_pec {
        Ok(raw) => {
            let payload_len = raw.payload_len as usize;
            let b0 = if payload_len > 0 { raw.payload[0] } else { 0 };
            let b1 = if payload_len > 1 { raw.payload[1] } else { 0 };
            let b2 = if payload_len > 2 { raw.payload[2] } else { 0 };
            let b3 = if payload_len > 3 { raw.payload[3] } else { 0 };
            defmt::warn!(
                "bms_diag_mac: path=mb44_block_pec->mb44 write={} read=ok len={=u8} payload={=u8} b0=0x{=u8:x} b1=0x{=u8:x} b2=0x{=u8:x} b3=0x{=u8:x}",
                mba_block_write_pec.map(|_| "ok").unwrap_or_else(i2c_error_kind),
                raw.declared_len,
                raw.payload_len,
                b0,
                b1,
                b2,
                b3
            );
        }
        Err(e) => {
            defmt::warn!(
                "bms_diag_mac: path=mb44_block_pec->mb44 write={} read={} ",
                mba_block_write_pec
                    .map(|_| "ok")
                    .unwrap_or_else(i2c_error_kind),
                e
            );
        }
    }

    // Alternate wire format probe used by some hosts: write cmd bytes without SMBus count.
    let mba_word_write = i2c.write(addr, &[bq40z50::cmd::MANUFACTURER_BLOCK_ACCESS, 0x01, 0x00]);
    spin_delay(BMS_MAC_WRITE_SETTLE);
    let mba_word_read =
        bq40z50::read_block_raw_checked(i2c, addr, bq40z50::cmd::MANUFACTURER_BLOCK_ACCESS);

    match mba_word_read {
        Ok(raw) => {
            let payload_len = raw.payload_len as usize;
            let b0 = if payload_len > 0 { raw.payload[0] } else { 0 };
            let b1 = if payload_len > 1 { raw.payload[1] } else { 0 };
            let b2 = if payload_len > 2 { raw.payload[2] } else { 0 };
            let b3 = if payload_len > 3 { raw.payload[3] } else { 0 };
            defmt::warn!(
                "bms_diag_mac: path=mb44_word->mb44 write={} read=ok len={=u8} payload={=u8} b0=0x{=u8:x} b1=0x{=u8:x} b2=0x{=u8:x} b3=0x{=u8:x}",
                mba_word_write.map(|_| "ok").unwrap_or_else(i2c_error_kind),
                raw.declared_len,
                raw.payload_len,
                b0,
                b1,
                b2,
                b3
            );
        }
        Err(e) => {
            defmt::warn!(
                "bms_diag_mac: path=mb44_word->mb44 write={} read={} ",
                mba_word_write.map(|_| "ok").unwrap_or_else(i2c_error_kind),
                e
            );
        }
    }

    let md23_split = word_diag_block_split(i2c, addr, bq40z50::cmd::MANUFACTURER_DATA);
    let mb44_split = word_diag_block_split(i2c, addr, bq40z50::cmd::MANUFACTURER_BLOCK_ACCESS);
    let raw_read1 = word_diag_read_only(i2c, addr, 1);
    let raw_read2 = word_diag_read_only(i2c, addr, 2);
    defmt::warn!(
        "bms_diag_bus: addr=0x{=u8:x} md23={} md23_wr={} md23_rd={} md23_len={=u8} md23_payload={=u8} md23_b0=0x{=u8:x} md23_b1=0x{=u8:x} mb44={} mb44_wr={} mb44_rd={} mb44_len={=u8} mb44_payload={=u8} mb44_b0=0x{=u8:x} mb44_b1=0x{=u8:x} raw_read1={} raw_read1_len={=u8} raw_read1_b0=0x{=u8:x} raw_read2={} raw_read2_len={=u8} raw_read2_b0=0x{=u8:x} raw_read2_b1=0x{=u8:x}",
        addr,
        md23_split.err,
        md23_split.write_err,
        md23_split.read_err,
        md23_split.declared_len,
        md23_split.payload_len,
        md23_split.b0,
        md23_split.b1,
        mb44_split.err,
        mb44_split.write_err,
        mb44_split.read_err,
        mb44_split.declared_len,
        mb44_split.payload_len,
        mb44_split.b0,
        mb44_split.b1,
        raw_read1.err,
        raw_read1.len,
        raw_read1.b0,
        raw_read2.err,
        raw_read2.len,
        raw_read2.b0,
        raw_read2.b1,
    );
}

fn write_bms_rom_word<I2C>(
    i2c: &mut I2C,
    addr: u8,
    cmd: u8,
    lo: u8,
    hi: u8,
) -> Result<(), bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    // TI's standalone SREC programming note uses plain WriteWord frames, but keep a compile-time
    // PEC-first experiment available in case this board's ROM only commits writes with SMBus PEC.
    let addr_w = addr << 1;
    let pec = crc8_smbus(&[addr_w, cmd, lo, hi]);
    let direct = [cmd, lo, hi];
    let with_pec = [cmd, lo, hi, pec];
    let frames: [(&str, &[u8]); 2] = if cfg!(feature = "bms-rom-force-pec") {
        [("pec", &with_pec), ("direct", &direct)]
    } else {
        [("direct", &direct), ("pec", &with_pec)]
    };

    for (via, frame) in frames {
        if i2c.write(addr, frame).is_ok() {
            if cfg!(feature = "bms-dual-probe-diag") {
                defmt::warn!(
                    "bms_diag: addr=0x{=u8:x} stage=rom_word_write cmd=0x{=u8:x} via={}",
                    addr,
                    cmd,
                    via
                );
            }
            return Ok(());
        }
    }

    if cfg!(feature = "bms-dual-probe-diag") {
        defmt::warn!(
            "bms_diag: addr=0x{=u8:x} stage=rom_word_write cmd=0x{=u8:x} via=fail",
            addr,
            cmd
        );
    }

    Err(bq40z50::BmsDiagError::I2cNack)
}

fn send_bms_rom_cmd<I2C>(i2c: &mut I2C, addr: u8, cmd: u8) -> Result<(), bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let addr_w = addr << 1;
    let pec = crc8_smbus(&[addr_w, cmd]);
    let direct = [cmd];
    let with_pec = [cmd, pec];
    let frames: [(&str, &[u8]); 2] = if cfg!(feature = "bms-rom-force-pec") {
        [("pec", &with_pec), ("direct", &direct)]
    } else {
        [("direct", &direct), ("pec", &with_pec)]
    };

    for (via, frame) in frames {
        if i2c.write(addr, frame).is_ok() {
            if cfg!(feature = "bms-dual-probe-diag") {
                defmt::warn!(
                    "bms_diag: addr=0x{=u8:x} stage=rom_send_cmd cmd=0x{=u8:x} via={}",
                    addr,
                    cmd,
                    via
                );
            }
            return Ok(());
        }
    }

    if cfg!(feature = "bms-dual-probe-diag") {
        defmt::warn!(
            "bms_diag: addr=0x{=u8:x} stage=rom_send_cmd cmd=0x{=u8:x} via=fail",
            addr,
            cmd
        );
    }

    Err(bq40z50::BmsDiagError::I2cNack)
}

fn write_bms_rom_bytes<I2C>(
    i2c: &mut I2C,
    addr: u8,
    bytes: &[u8],
) -> Result<(), bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    if i2c.write(addr, bytes).is_ok() {
        return Ok(());
    }

    // Retry once with SMBus PEC appended.
    let mut frame = [0u8; BMS_ROM_FLASH_BLOCK_BYTES_MAX + 5];
    if bytes.len() > (frame.len() - 1) {
        return Err(bq40z50::BmsDiagError::BadBlockLen);
    }
    frame[..bytes.len()].copy_from_slice(bytes);
    let addr_w = addr << 1;
    let mut pec_input = [0u8; BMS_ROM_FLASH_BLOCK_BYTES_MAX + 6];
    pec_input[0] = addr_w;
    pec_input[1..(1 + bytes.len())].copy_from_slice(bytes);
    let pec = crc8_smbus(&pec_input[..(1 + bytes.len())]);
    frame[bytes.len()] = pec;
    i2c.write(addr, &frame[..(bytes.len() + 1)])
        .map_err(|_| bq40z50::BmsDiagError::I2cNack)
}

fn touch_bms_command<I2C>(i2c: &mut I2C, addr: u8, cmd: u8) -> Result<(), bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    i2c.write(addr, &[cmd])
        .map_err(|_| bq40z50::BmsDiagError::I2cNack)
}

fn write_bms_rom_bytes_trace<I2C>(
    i2c: &mut I2C,
    addr: u8,
    stage: &'static str,
    frame_idx: u8,
    bytes: &[u8],
    quiet: bool,
) -> Result<(), bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    if bytes.len() > (BMS_ROM_FLASH_BLOCK_BYTES_MAX + 4) {
        return Err(bq40z50::BmsDiagError::BadBlockLen);
    }

    match i2c.write(addr, bytes) {
        Ok(()) => {
            if !quiet {
                defmt::warn!(
                    "bms_diag: addr=0x{=u8:x} stage={} frame={=u8} via=direct",
                    addr,
                    stage,
                    frame_idx
                );
            }
            return Ok(());
        }
        Err(direct_err) => {
            let direct_kind = i2c_error_kind(direct_err);
            let mut frame = [0u8; BMS_ROM_FLASH_BLOCK_BYTES_MAX + 5];
            frame[..bytes.len()].copy_from_slice(bytes);
            let addr_w = addr << 1;
            let mut pec_input = [0u8; BMS_ROM_FLASH_BLOCK_BYTES_MAX + 6];
            pec_input[0] = addr_w;
            pec_input[1..(1 + bytes.len())].copy_from_slice(bytes);
            let pec = crc8_smbus(&pec_input[..(1 + bytes.len())]);
            frame[bytes.len()] = pec;
            match i2c.write(addr, &frame[..(bytes.len() + 1)]) {
                Ok(()) => {
                    if !quiet {
                        defmt::warn!(
                            "bms_diag: addr=0x{=u8:x} stage={} frame={=u8} via=pec direct_err={}",
                            addr,
                            stage,
                            frame_idx,
                            direct_kind
                        );
                    }
                    Ok(())
                }
                Err(pec_err) => {
                    if !quiet {
                        defmt::warn!(
                            "bms_diag: addr=0x{=u8:x} stage={} frame={=u8} direct_err={} pec_err={}",
                            addr,
                            stage,
                            frame_idx,
                            direct_kind,
                            i2c_error_kind(pec_err)
                        );
                    }
                    Err(bq40z50::BmsDiagError::I2cNack)
                }
            }
        }
    }
}

fn write_bms_rom_block<I2C>(
    i2c: &mut I2C,
    addr: u8,
    cmd: u8,
    payload: &[u8],
) -> Result<(), bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    if payload.is_empty() || payload.len() > (BMS_ROM_FLASH_BLOCK_BYTES_MAX + 2) {
        return Err(bq40z50::BmsDiagError::BadBlockLen);
    }

    let mut frame = [0u8; BMS_ROM_FLASH_BLOCK_BYTES_MAX + 4];
    frame[0] = cmd;
    frame[1] = payload.len() as u8;
    frame[2..(2 + payload.len())].copy_from_slice(payload);
    let frame_len = 2 + payload.len();

    let mut frame_pec = [0u8; BMS_ROM_FLASH_BLOCK_BYTES_MAX + 5];
    frame_pec[..frame_len].copy_from_slice(&frame[..frame_len]);
    let addr_w = addr << 1;
    let mut pec_input = [0u8; BMS_ROM_FLASH_BLOCK_BYTES_MAX + 6];
    pec_input[0] = addr_w;
    pec_input[1..(1 + frame_len)].copy_from_slice(&frame[..frame_len]);
    let pec = crc8_smbus(&pec_input[..(1 + frame_len)]);
    frame_pec[frame_len] = pec;
    let frames: [(&str, &[u8]); 2] = if cfg!(feature = "bms-rom-force-pec") {
        [
            ("pec", &frame_pec[..(frame_len + 1)]),
            ("direct", &frame[..frame_len]),
        ]
    } else {
        [
            ("direct", &frame[..frame_len]),
            ("pec", &frame_pec[..(frame_len + 1)]),
        ]
    };

    for (via, candidate) in frames {
        if i2c.write(addr, candidate).is_ok() {
            if cfg!(feature = "bms-dual-probe-diag") {
                defmt::warn!(
                    "bms_diag: addr=0x{=u8:x} stage=rom_block_write cmd=0x{=u8:x} via={} len={=u8}",
                    addr,
                    cmd,
                    via,
                    payload.len() as u8
                );
            }
            return Ok(());
        }
    }

    if cfg!(feature = "bms-dual-probe-diag") {
        defmt::warn!(
            "bms_diag: addr=0x{=u8:x} stage=rom_block_write cmd=0x{=u8:x} via=fail len={=u8}",
            addr,
            cmd,
            payload.len() as u8
        );
    }

    Err(bq40z50::BmsDiagError::I2cNack)
}

fn program_bms_rom_section<I2C>(
    i2c: &mut I2C,
    addr: u8,
    cmd: u8,
    start_addr: u16,
    block_bytes: usize,
    image: &[u8],
    stage: &'static str,
    quiet: bool,
) -> Result<(), bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    if block_bytes == 0 || block_bytes > BMS_ROM_FLASH_BLOCK_BYTES_MAX || image.is_empty() {
        return Err(bq40z50::BmsDiagError::BadBlockLen);
    }

    let total_blocks = image.len().div_ceil(block_bytes);
    let mut payload = [0u8; BMS_ROM_FLASH_BLOCK_BYTES_MAX + 2];
    for (idx, chunk) in image.chunks(block_bytes).enumerate() {
        let word_addr = start_addr.wrapping_add((idx * block_bytes) as u16);
        payload[0] = (word_addr & 0x00FF) as u8;
        payload[1] = (word_addr >> 8) as u8;
        payload[2..(2 + chunk.len())].copy_from_slice(chunk);
        write_bms_rom_block(i2c, addr, cmd, &payload[..(chunk.len() + 2)])?;
        spin_delay(BMS_ROM_FLASH_WRITE_GAP);

        if !quiet && (idx == 0 || ((idx + 1) % 128) == 0 || (idx + 1) == total_blocks) {
            defmt::warn!(
                "bms_diag: addr=0x{=u8:x} stage={} block={=u16}/{=u16} len={=u8}",
                addr,
                stage,
                (idx + 1) as u16,
                total_blocks as u16,
                chunk.len() as u8
            );
        }
    }

    Ok(())
}

fn program_bms_rom_sparse_info_sections<I2C>(
    i2c: &mut I2C,
    addr: u8,
    quiet: bool,
) -> Result<(), bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    for (stage, payload, block_off) in [
        ("rom_flash_sec3_blk00", BMS_ROM_SECTION3_BLK00, 0x00u8),
        ("rom_flash_sec3_blk80", BMS_ROM_SECTION3_BLK80, 0x80u8),
    ] {
        if let Err(e) = write_bms_rom_word(i2c, addr, 0x1A, 0xDE, 0x83) {
            if !quiet {
                log_bms_diag(addr, stage, e, "word", "srec");
            }
            return Err(e);
        }
        spin_delay(BMS_ROM_FLASH_WORD_GAP);
        if let Err(e) = write_bms_rom_block(i2c, addr, 0x05, payload) {
            if !quiet {
                log_bms_diag(addr, stage, e, "block", "srec");
            }
            return Err(e);
        }
        spin_delay(BMS_ROM_FLASH_WRITE_GAP);
        if !quiet {
            defmt::warn!(
                "bms_diag: addr=0x{=u8:x} stage={} block=0x{=u8:x}",
                addr,
                stage,
                block_off
            );
        }
    }

    Ok(())
}

fn program_bms_rom_info_section<I2C>(
    i2c: &mut I2C,
    addr: u8,
    image: &[u8],
    quiet: bool,
) -> Result<(), bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    if image.len() != 0x100 {
        return Err(bq40z50::BmsDiagError::BadBlockLen);
    }

    let mut payload = [0u8; BMS_ROM_FLASH_BLOCK_BYTES_SEC3 + 2];
    for (idx, chunk) in image
        .chunks_exact(BMS_ROM_FLASH_BLOCK_BYTES_SEC3)
        .enumerate()
    {
        let block_off = (idx * BMS_ROM_FLASH_BLOCK_BYTES_SEC3) as u16;
        if let Err(e) = write_bms_rom_word(i2c, addr, 0x1A, 0xDE, 0x83) {
            if !quiet {
                log_bms_diag(addr, "rom_flash_sec3_preface", e, "word", "srec");
            }
            return Err(e);
        }
        spin_delay(BMS_ROM_FLASH_WORD_GAP);

        payload[0] = (block_off & 0x00FF) as u8;
        payload[1] = (block_off >> 8) as u8;
        payload[2..].copy_from_slice(chunk);
        if block_off == 0 {
            payload[2] = 0xFF;
            payload[3] = 0xFF;
        }

        if let Err(e) = write_bms_rom_block(i2c, addr, 0x05, &payload) {
            if !quiet {
                log_bms_diag(addr, "rom_flash_sec3_info", e, "block", "srec");
            }
            return Err(e);
        }
        spin_delay(BMS_ROM_FLASH_WRITE_GAP);

        if !quiet {
            defmt::warn!(
                "bms_diag: addr=0x{=u8:x} stage=rom_flash_sec3_info block=0x{=u8:x}",
                addr,
                block_off as u8
            );
        }
    }

    Ok(())
}

fn run_bms_rom_erase_preface_sequence<I2C>(
    i2c: &mut I2C,
    addr: u8,
    quiet: bool,
) -> Result<(), bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    // TI `prog_srec_v0p9.py` erase preface.
    if let Err(e) = write_bms_rom_word(i2c, addr, 0x1A, 0xDE, 0x83) {
        if !quiet {
            log_bms_diag(addr, "rom_flash_preface_1a", e, "word", "srec");
        }
        return Err(e);
    }
    spin_delay(BMS_ROM_FLASH_WORD_GAP);
    if let Err(e) = write_bms_rom_word(i2c, addr, 0x06, 0x00, 0x00) {
        if !quiet {
            log_bms_diag(addr, "rom_flash_preface_06", e, "word", "srec");
        }
        return Err(e);
    }
    spin_delay(BMS_ROM_FLASH_ERASE_GAP);
    if let Err(e) = write_bms_rom_word(i2c, addr, 0x07, 0xDE, 0x83) {
        if !quiet {
            log_bms_diag(addr, "rom_flash_preface_07", e, "word", "srec");
        }
        return Err(e);
    }
    spin_delay(BMS_ROM_FLASH_ERASE_GAP);
    if let Err(e) = write_bms_rom_word(i2c, addr, 0x11, 0xDE, 0x83) {
        if !quiet {
            log_bms_diag(addr, "rom_flash_preface_11", e, "word", "srec");
        }
        return Err(e);
    }
    spin_delay(BMS_ROM_FLASH_ERASE_GAP);
    Ok(())
}

fn run_bms_rom_flash_recover_sequence<I2C>(
    i2c: &mut I2C,
    addr: u8,
    quiet: bool,
) -> Result<bool, bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let force_recover = cfg!(feature = "bms-rom-recover-force");
    let sig = match read_u16_with_optional_pec(i2c, addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE) {
        Ok(v) => v,
        Err(e) if force_recover => {
            if !quiet {
                log_bms_diag(addr, "rom_flash_force_sig_read", e, "word", "srec");
            }
            0xFFFF
        }
        Err(e) => return Err(e),
    };
    if sig != BMS_ROM_MODE_SIGNATURE && !force_recover {
        return Ok(false);
    }
    if force_recover && sig != BMS_ROM_MODE_SIGNATURE && !quiet {
        defmt::warn!(
            "bms_diag: addr=0x{=u8:x} stage=rom_flash_force_non_rom rsoc=0x{=u16:x}",
            addr,
            sig
        );
    }

    let section1_image = BMS_ROM_SECTION1_IMAGE;
    let section2_image = BMS_ROM_SECTION2_IMAGE;

    if !quiet {
        defmt::warn!(
            "bms_diag: addr=0x{=u8:x} stage=rom_flash_start image={} info_layout={} sec1={=u32} sec1_used={=u32} sec2={=u32} sec2_used={=u32} blk12={=u8}",
            addr,
            BMS_ROM_FLASH_IMAGE_TAG,
            BMS_ROM_INFO_LAYOUT_TAG,
            section1_image.len() as u32,
            BMS_ROM_SECTION1_USED_LEN as u32,
            section2_image.len() as u32,
            BMS_ROM_SECTION2_USED_LEN as u32,
            BMS_ROM_FLASH_BLOCK_BYTES_SEC12 as u8
        );
        defmt::warn!(
            "bms_diag: addr=0x{=u8:x} stage=rom_flash_preface_begin",
            addr
        );
    }
    log_bms_rom_checksum_probe(i2c, addr, "rom_flash_ck_before_erase", quiet);

    if let Err(_preface_err) = run_bms_rom_erase_preface_sequence(i2c, addr, quiet) {
        if !quiet {
            defmt::warn!(
                "bms_diag: addr=0x{=u8:x} stage=rom_flash_e2e_unlock_begin",
                addr
            );
        }
        match run_bms_rom_e2e_unlock_sequence(i2c, addr, quiet) {
            Ok(()) => {
                spin_delay(BMS_ROM_FLASH_WORD_GAP);
                if !quiet {
                    defmt::warn!(
                        "bms_diag: addr=0x{=u8:x} stage=rom_flash_e2e_unlock_done",
                        addr
                    );
                }
            }
            Err(e) => {
                if !quiet {
                    log_bms_diag(addr, "rom_flash_e2e_unlock", e, "word", "e2e");
                }
            }
        }

        if !quiet {
            defmt::warn!("bms_diag: addr=0x{=u8:x} stage=rom_flash_token_begin", addr);
        }
        match run_bms_rom_token_recover_sequence(i2c, addr) {
            Ok(()) => {
                if !quiet {
                    defmt::warn!("bms_diag: addr=0x{=u8:x} stage=rom_flash_token_done", addr);
                }
            }
            Err(e) => {
                if !quiet {
                    log_bms_diag(addr, "rom_flash_token", e, "word", "token");
                }
            }
        }

        if !quiet {
            defmt::warn!(
                "bms_diag: addr=0x{=u8:x} stage=rom_flash_preface_retry_begin",
                addr
            );
        }
        if let Err(retry_err) = run_bms_rom_erase_preface_sequence(i2c, addr, quiet) {
            if !quiet {
                log_bms_diag(addr, "rom_flash_preface_retry", retry_err, "word", "srec");
            }
            return Err(retry_err);
        }
        if !quiet {
            defmt::warn!(
                "bms_diag: addr=0x{=u8:x} stage=rom_flash_preface_retry_done",
                addr
            );
        }

        if sig != BMS_ROM_MODE_SIGNATURE && !quiet {
            defmt::warn!(
                "bms_diag: addr=0x{=u8:x} stage=rom_flash_force_preface_override rsoc_before=0x{=u16:x}",
                addr,
                sig
            );
        }
    }

    // Section1: Data Flash 0x4000..0x5FFF (cmd 0x0F).
    if let Err(e) = program_bms_rom_section(
        i2c,
        addr,
        0x0F,
        0x4000,
        BMS_ROM_FLASH_BLOCK_BYTES_SEC12,
        section1_image,
        "rom_flash_sec1",
        quiet,
    ) {
        if !quiet {
            log_bms_diag(addr, "rom_flash_sec1", e, "block", "srec");
        }
        return Err(e);
    }

    // Section2: Instruction Flash 0x100000.. (cmd 0x05, 16-bit address window).
    if let Err(e) = program_bms_rom_section(
        i2c,
        addr,
        0x05,
        0x0000,
        BMS_ROM_FLASH_BLOCK_BYTES_SEC12,
        section2_image,
        "rom_flash_sec2",
        quiet,
    ) {
        if !quiet {
            log_bms_diag(addr, "rom_flash_sec2", e, "block", "srec");
        }
        return Err(e);
    }
    log_bms_rom_checksum_probe(i2c, addr, "rom_flash_ck_after_sec2", quiet);

    #[cfg(feature = "bms-rom-full-info")]
    {
        // Keep the wider info-window rewrite available as an opt-in experiment, but default back
        // to TI's stock sparse-info flow when trying to leave ROM cleanly.
        if let Err(e) = program_bms_rom_info_section(i2c, addr, BMS_ROM_SECTION3_INFO_IMAGE, quiet)
        {
            if !quiet {
                log_bms_diag(addr, "rom_flash_sec3_info", e, "block", "srec");
            }
            return Err(e);
        }
    }
    #[cfg(not(feature = "bms-rom-full-info"))]
    if let Err(e) = program_bms_rom_sparse_info_sections(i2c, addr, quiet) {
        if !quiet {
            log_bms_diag(addr, "rom_flash_sec3_sparse", e, "block", "srec");
        }
        return Err(e);
    }
    log_bms_rom_checksum_probe(i2c, addr, "rom_flash_ck_after_sec3", quiet);

    if let Err(e) = write_bms_rom_word(i2c, addr, 0x1A, 0xDE, 0x83) {
        if !quiet {
            log_bms_diag(addr, "rom_flash_sec4_preface_1a", e, "word", "srec");
        }
        return Err(e);
    }
    spin_delay(BMS_ROM_FLASH_WORD_GAP);
    if let Err(e) = write_bms_rom_block(i2c, addr, 0x05, BMS_ROM_SECTION4_BLK) {
        if !quiet {
            log_bms_diag(addr, "rom_flash_sec4_blk", e, "block", "srec");
        }
        return Err(e);
    }
    spin_delay(BMS_ROM_FLASH_WORD_GAP);
    log_bms_rom_checksum_probe(i2c, addr, "rom_flash_ck_after_sec4", quiet);

    let mut after = BMS_ROM_MODE_SIGNATURE;
    if let Some(v) = run_bms_rom_exact_execute_probe(i2c, addr, "rom_flash_exec_exact", quiet)? {
        after = v;
    }
    if after == BMS_ROM_MODE_SIGNATURE {
        spin_delay(BMS_ROM_EXECUTE_FLASH_SETTLE);
        match read_u16_with_optional_pec(i2c, addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE) {
            Ok(v) => {
                if !quiet {
                    defmt::warn!(
                        "bms_diag: addr=0x{=u8:x} stage=rom_flash_exec_exact_quiet rsoc_after=0x{=u16:x}",
                        addr,
                        v
                    );
                }
                after = v;
            }
            Err(e) => {
                if !quiet {
                    log_bms_diag(addr, "rom_flash_exec_exact_quiet", e, "word", "srec");
                }
            }
        }
    }

    if after == BMS_ROM_MODE_SIGNATURE {
        if !quiet {
            defmt::warn!(
                "bms_diag: addr=0x{=u8:x} stage=rom_flash_exec_exact_still_rom keep_charge=true",
                addr
            );
        }
        return Err(bq40z50::BmsDiagError::InconsistentSample);
    }

    if after > 100 {
        if !quiet {
            defmt::warn!(
                "bms_diag: addr=0x{=u8:x} stage=rom_flash_invalid_fw_state rsoc_after=0x{=u16:x}",
                addr,
                after
            );
        }
        return Err(bq40z50::BmsDiagError::InconsistentSample);
    }

    if !quiet {
        defmt::warn!(
            "bms_diag: addr=0x{=u8:x} stage=rom_flash_done rsoc_after=0x{=u16:x}",
            addr,
            after
        );
    }
    Ok(true)
}

fn run_bms_rom_token_recover_sequence<I2C>(
    i2c: &mut I2C,
    addr: u8,
) -> Result<(), bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    // Sequence adapted from TI bq40z50 flashstream examples. We keep it as a last-ditch
    // helper after the exact raw E2E unlock frames, because some half-programmed gauges
    // only accept one of the two wire encodings.
    write_bms_rom_word(i2c, addr, 0x09, 0x00, 0x00)?;
    write_bms_rom_word(i2c, addr, 0x0A, 0x02, 0x00)?;
    write_bms_rom_word(i2c, addr, 0x09, 0x02, 0x00)?;
    write_bms_rom_word(i2c, addr, 0x0A, 0x00, 0x00)?;
    write_bms_rom_word(i2c, addr, 0x00, 0x00, 0x00)?;
    write_bms_rom_word(i2c, addr, 0x1A, 0xDE, 0x83)?;
    write_bms_rom_word(i2c, addr, 0x06, 0x00, 0x00)?;
    spin_delay(Duration::from_millis(250));
    write_bms_rom_word(i2c, addr, 0x00, 0x80, 0x00)?;
    write_bms_rom_word(i2c, addr, 0x1A, 0xDE, 0x83)?;
    write_bms_rom_word(i2c, addr, 0x06, 0x80, 0x00)?;
    spin_delay(Duration::from_millis(250));

    write_bms_rom_word(i2c, addr, 0x09, 0x00, 0x00)?;
    write_bms_rom_word(i2c, addr, 0x0A, 0x08, 0x00)?;
    write_bms_rom_word(i2c, addr, 0x09, 0x02, 0x00)?;
    write_bms_rom_word(i2c, addr, 0x0A, 0xB8, 0x00)?;
    write_bms_rom_word(i2c, addr, 0x00, 0x80, 0x01)?;
    write_bms_rom_word(i2c, addr, 0x1A, 0xDE, 0x83)?;
    write_bms_rom_word(i2c, addr, 0x06, 0x80, 0x01)?;
    spin_delay(Duration::from_millis(250));

    write_bms_rom_word(i2c, addr, 0x09, 0x00, 0x00)?;
    write_bms_rom_word(i2c, addr, 0x0A, 0x02, 0x00)?;
    write_bms_rom_word(i2c, addr, 0x09, 0x02, 0x00)?;
    write_bms_rom_word(i2c, addr, 0x0A, 0x00, 0x00)?;
    send_bms_rom_cmd(i2c, addr, 0x08)?;
    Ok(())
}

fn run_bms_rom_e2e_preface_sequence<I2C>(
    i2c: &mut I2C,
    addr: u8,
) -> Result<(), bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    // Transaction bytes captured from TI E2E bq40z50 SREC logs. This is kept as a
    // last-chance post-program execute nudge when the stock 0x08 path never leaves ROM.
    const E2E_ROM_SEQ: [&[u8]; 24] = [
        &[0x09, 0x00, 0x00, 0x29],
        &[0x0A, 0x00, 0x00, 0x94],
        &[0x09, 0x02, 0x00, 0x03],
        &[0x0A, 0x00, 0x00, 0x94],
        &[0x00, 0x00, 0x00, 0x13],
        &[0x1A, 0xDE, 0x83, 0xDA],
        &[0x06, 0x00, 0x00, 0x6E],
        &[0x00, 0x80, 0x00, 0xA5],
        &[0x1A, 0xDE, 0x83, 0xDA],
        &[0x06, 0x80, 0x00, 0xD8],
        &[0x09, 0x00, 0x00, 0x29],
        &[0x0A, 0x08, 0x00, 0x3C],
        &[0x09, 0x02, 0x00, 0x03],
        &[0x0A, 0xB8, 0x00, 0x73],
        &[0x00, 0x80, 0x01, 0xA2],
        &[0x1A, 0xDE, 0x83, 0xDA],
        &[0x06, 0x80, 0x01, 0xDF],
        &[0x09, 0x00, 0x00, 0x29],
        &[0x0A, 0x00, 0x00, 0x94],
        &[0x09, 0x02, 0x00, 0x03],
        &[0x0A, 0x00, 0x00, 0x94],
        &[0x11, 0xDE, 0x83, 0x36],
        &[0x07, 0xDE, 0x83, 0xE9],
        &[0x08, 0x11],
    ];

    for frame in E2E_ROM_SEQ {
        write_bms_rom_bytes(i2c, addr, frame)?;
        spin_delay(BMS_WORD_GAP);
    }

    let pec = crc8_smbus(&[addr << 1, 0x08, 0x11]);
    write_bms_rom_bytes(i2c, addr, &[0x08, 0x11, pec])?;
    spin_delay(BMS_WORD_GAP);
    write_bms_rom_bytes(i2c, addr, &[0x08])?;
    Ok(())
}

fn run_bms_rom_e2e_unlock_sequence<I2C>(
    i2c: &mut I2C,
    addr: u8,
    quiet: bool,
) -> Result<(), bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    // Raw SMBus frames captured in TI E2E SREC programming logs. Use them only as a
    // recovery unlock fallback before retrying the stock GEOSTASIS/TI erase preface.
    const E2E_ROM_SEQ: [&[u8]; 17] = [
        &[0x09, 0x00, 0x00, 0x29],
        &[0x0A, 0x00, 0x00, 0x94],
        &[0x09, 0x02, 0x00, 0x03],
        &[0x0A, 0x00, 0x00, 0x94],
        &[0x00, 0x00, 0x00, 0x13],
        &[0x1A, 0xDE, 0x83, 0xDA],
        &[0x06, 0x00, 0x00, 0x6E],
        &[0x00, 0x80, 0x00, 0xA5],
        &[0x1A, 0xDE, 0x83, 0xDA],
        &[0x06, 0x80, 0x00, 0xD8],
        &[0x09, 0x00, 0x00, 0x29],
        &[0x0A, 0x08, 0x00, 0x3C],
        &[0x09, 0x02, 0x00, 0x03],
        &[0x0A, 0xB8, 0x00, 0x73],
        &[0x00, 0x80, 0x01, 0xA2],
        &[0x1A, 0xDE, 0x83, 0xDA],
        &[0x06, 0x80, 0x01, 0xDF],
    ];

    for (idx, frame) in E2E_ROM_SEQ.iter().enumerate() {
        write_bms_rom_bytes_trace(
            i2c,
            addr,
            "rom_flash_e2e_unlock_step",
            idx as u8,
            frame,
            quiet,
        )?;
        spin_delay(BMS_WORD_GAP);
    }

    Ok(())
}

fn run_bms_rom_execute_flash_sequence<I2C>(
    i2c: &mut I2C,
    addr: u8,
) -> Result<(), bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    // Cross-family TI ROM execute-flash sequence (0x00=0x000F then 0x64=0x000F).
    // Both steps are SMBus word writes; use the ROM helpers so PEC/swapped-byte retries stay available.
    write_bms_rom_word(i2c, addr, 0x00, 0x0F, 0x00)?;
    spin_delay(BMS_MAC_TOGGLE_SETTLE);
    write_bms_rom_word(i2c, addr, 0x64, 0x0F, 0x00)?;
    spin_delay(BMS_ROM_EXECUTE_FLASH_SETTLE);
    Ok(())
}

fn log_bms_rom_checksum_probe<I2C>(i2c: &mut I2C, addr: u8, stage: &'static str, quiet: bool)
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    // Keep the stock ROM recovery path bus-quiet around erase/program/execute. TI's public
    // bq40z50 SREC flow does not use these extra ROM reads, so leave the hook in place but do
    // not issue additional traffic during normal recovery attempts.
    let _ = (i2c, addr, stage, quiet);
}

fn run_bms_rom_exact_execute_probe<I2C>(
    i2c: &mut I2C,
    addr: u8,
    stage: &'static str,
    quiet: bool,
) -> Result<Option<u16>, bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    if !quiet {
        defmt::warn!("bms_diag: addr=0x{=u8:x} stage={} begin", addr, stage);
    }

    if let Err(e) = send_bms_rom_cmd(i2c, addr, 0x08) {
        if !quiet {
            log_bms_diag(addr, stage, e, "cmd", "srec");
        }
        return Ok(None);
    }

    spin_delay(BMS_ROM_EXECUTE_FLASH_FIRST_CHECK);
    match read_u16_with_optional_pec(i2c, addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE) {
        Ok(after) => {
            if !quiet {
                defmt::warn!(
                    "bms_diag: addr=0x{=u8:x} stage={} rsoc_after=0x{=u16:x}",
                    addr,
                    stage,
                    after
                );
            }
            Ok(Some(after))
        }
        Err(e) => {
            if !quiet {
                log_bms_diag(addr, stage, e, "word", "srec");
            }
            Ok(None)
        }
    }
}

fn observe_bms_rom_execute_result<I2C>(
    i2c: &mut I2C,
    addr: u8,
    stage: &'static str,
    initial_settle: Duration,
    probe_fw_window: bool,
    quiet: bool,
) -> Result<Option<u16>, bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let poll_deadline = Instant::now() + BMS_ROM_EXECUTE_FLASH_POLL_WINDOW;
    let mut read_attempt = 0u8;
    let mut last_success = None;
    let mut saw_fw_window = false;
    spin_delay(initial_settle);
    loop {
        read_attempt = read_attempt.saturating_add(1);

        match read_u16_with_optional_pec(i2c, addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE) {
            Ok(after) => {
                if !quiet {
                    defmt::warn!(
                        "bms_diag: addr=0x{=u8:x} stage={} rsoc_after=0x{=u16:x} attempt={=u8} fw_window={=bool}",
                        addr,
                        stage,
                        after,
                        read_attempt,
                        saw_fw_window
                    );
                }
                last_success = Some(after);
                if after != BMS_ROM_MODE_SIGNATURE || Instant::now() >= poll_deadline {
                    return Ok(Some(after));
                }
            }
            Err(e) => {
                if !quiet {
                    defmt::warn!(
                        "bms_diag: addr=0x{=u8:x} stage={} read_err={} attempt={=u8} fw_window={=bool}",
                        addr,
                        stage,
                        e,
                        read_attempt,
                        saw_fw_window
                    );
                }
                if Instant::now() >= poll_deadline {
                    return Ok(last_success);
                }
            }
        }

        if probe_fw_window {
            saw_fw_window |= log_execute_fw_window_probe(i2c, addr, stage, read_attempt, quiet);
        }
        spin_delay(BMS_ROM_EXECUTE_FLASH_SETTLE);
    }
}

fn try_bms_rom_execute_frame<I2C>(
    i2c: &mut I2C,
    addr: u8,
    stage: &'static str,
    frame: &[u8],
    settle: Duration,
    probe_fw_window: bool,
    quiet: bool,
) -> Result<Option<u16>, bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    if !quiet {
        defmt::warn!("bms_diag: addr=0x{=u8:x} stage={} begin", addr, stage);
    }

    if let Err(e) = i2c.write(addr, frame) {
        if !quiet {
            defmt::warn!(
                "bms_diag: addr=0x{=u8:x} stage={} write_err={}",
                addr,
                stage,
                i2c_error_kind(e)
            );
        }
        return Ok(None);
    }

    observe_bms_rom_execute_result(i2c, addr, stage, settle, probe_fw_window, quiet)
}

fn run_bms_rom_postflash_resume_sequence<I2C>(
    i2c: &mut I2C,
    addr: u8,
    quiet: bool,
) -> Result<bool, bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let sig = read_u16_with_optional_pec(i2c, addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE)?;
    if sig != BMS_ROM_MODE_SIGNATURE {
        if !quiet {
            defmt::warn!(
                "bms_diag: addr=0x{=u8:x} stage=rom_post_flash_resume_not_rom rsoc=0x{=u16:x}",
                addr,
                sig
            );
        }
        return Ok(true);
    }

    if !quiet {
        defmt::warn!(
            "bms_diag: addr=0x{=u8:x} stage=rom_post_flash_resume_observe rsoc=0x{=u16:x}",
            addr,
            sig
        );
    }
    Ok(false)
}

fn read_u16_with_pec<I2C>(i2c: &mut I2C, addr: u8, cmd: u8) -> Result<u16, bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let mut buf = [0u8; 3];
    i2c.write_read(addr, &[cmd], &mut buf)
        .map_err(|_| bq40z50::BmsDiagError::I2cNack)?;

    let addr_w = addr << 1;
    let addr_r = addr_w | 1;
    let expected = crc8_smbus(&[addr_w, cmd, addr_r, buf[0], buf[1]]);
    if expected != buf[2] {
        return Err(bq40z50::BmsDiagError::InconsistentSample);
    }

    Ok(u16::from_le_bytes([buf[0], buf[1]]))
}

fn read_u16_split<I2C>(i2c: &mut I2C, addr: u8, cmd: u8) -> Result<u16, bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    i2c.write(addr, &[cmd])
        .map_err(|_| bq40z50::BmsDiagError::I2cNack)?;
    spin_delay(BMS_WORD_GAP);
    let mut buf = [0u8; 2];
    i2c.read(addr, &mut buf)
        .map_err(|_| bq40z50::BmsDiagError::I2cNack)?;
    Ok(u16::from_le_bytes(buf))
}

fn read_u16_split_with_gap<I2C>(
    i2c: &mut I2C,
    addr: u8,
    cmd: u8,
    gap: Duration,
) -> Result<u16, bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    i2c.write(addr, &[cmd])
        .map_err(|_| bq40z50::BmsDiagError::I2cNack)?;
    spin_delay(gap);
    let mut buf = [0u8; 2];
    i2c.read(addr, &mut buf)
        .map_err(|_| bq40z50::BmsDiagError::I2cNack)?;
    Ok(u16::from_le_bytes(buf))
}

fn read_u16_wake_probe<I2C>(
    i2c: &mut I2C,
    addr: u8,
    cmd: u8,
    stage: &'static str,
    step: u8,
    delay_ms: u64,
    round: u8,
    quiet: bool,
) -> Result<u16, bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    for gap_ms in BMS_WAKE_READ_GAPS_MS {
        let gap = Duration::from_millis(gap_ms);
        match read_u16_split_with_gap(i2c, addr, cmd, gap) {
            Ok(raw) => {
                if !quiet {
                    defmt::warn!(
                        "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} round={=u8} gap_ms={=u64} raw=0x{=u16:x}",
                        addr,
                        stage,
                        step,
                        delay_ms,
                        round,
                        gap_ms,
                        raw
                    );
                }
                return Ok(raw);
            }
            Err(e) => {
                if !quiet {
                    defmt::warn!(
                        "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} round={=u8} gap_ms={=u64} err={}",
                        addr,
                        stage,
                        step,
                        delay_ms,
                        round,
                        gap_ms,
                        e
                    );
                }
            }
        }
    }

    read_u16_with_optional_pec(i2c, addr, cmd)
}

fn read_u16_with_optional_pec<I2C>(
    i2c: &mut I2C,
    addr: u8,
    cmd: u8,
) -> Result<u16, bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    const ATTEMPTS: u8 = 2;

    // A single SMBus transaction can occasionally NACK while the gauge is still healthy.
    // Retry the full fallback chain once before surfacing a hard transport error.
    for attempt in 0..ATTEMPTS {
        if let Ok(v) = read_u16_with_pec(i2c, addr, cmd) {
            return Ok(v);
        }
        if let Ok(v) = read_u16_split(i2c, addr, cmd) {
            return Ok(v);
        }
        if let Ok(v) = bq40z50::read_u16(i2c, addr, cmd) {
            return Ok(v);
        }

        if attempt + 1 < ATTEMPTS {
            spin_delay(BMS_WORD_GAP);
        }
    }

    Err(bq40z50::BmsDiagError::I2cNack)
}

fn read_i16_with_optional_pec<I2C>(
    i2c: &mut I2C,
    addr: u8,
    cmd: u8,
) -> Result<i16, bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    read_u16_with_optional_pec(i2c, addr, cmd).map(|v| i16::from_le_bytes(v.to_le_bytes()))
}

fn prime_bms_command_window<I2C>(i2c: &mut I2C, addr: u8) -> Result<(), bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let _ = touch_bms_command(i2c, addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE);
    Ok(())
}

fn log_bms_wake_read_only_diag<I2C>(
    i2c: &mut I2C,
    addr: u8,
    stage: &'static str,
    step: u8,
    delay_ms: u64,
    quiet: bool,
) where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    if quiet {
        return;
    }

    for len in [1usize, 2usize] {
        let diag = word_diag_read_only(i2c, addr, len);
        defmt::warn!(
            "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} len={=u8} err={} b0=0x{=u8:x} b1=0x{=u8:x}",
            addr,
            stage,
            step,
            delay_ms,
            diag.len,
            diag.err,
            diag.b0,
            diag.b1
        );
    }
}

fn read_u16_after_successful_touch<I2C>(
    i2c: &mut I2C,
    addr: u8,
    stage: &'static str,
    step: u8,
    delay_ms: u64,
    quiet: bool,
) -> Result<u16, bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    for gap_ms in BMS_WAKE_TOUCH_READ_GAPS_MS {
        spin_delay(Duration::from_millis(gap_ms));
        let mut buf = [0u8; 2];
        match i2c.read(addr, &mut buf) {
            Ok(()) => {
                let raw = u16::from_le_bytes(buf);
                if !quiet {
                    defmt::warn!(
                        "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} gap_ms={=u64} raw=0x{=u16:x}",
                        addr,
                        stage,
                        step,
                        delay_ms,
                        gap_ms,
                        raw
                    );
                }
                return Ok(raw);
            }
            Err(e) => {
                if !quiet {
                    defmt::warn!(
                        "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} gap_ms={=u64} err={}",
                        addr,
                        stage,
                        step,
                        delay_ms,
                        gap_ms,
                        i2c_error_kind(e)
                    );
                }
            }
        }
    }

    Err(bq40z50::BmsDiagError::I2cNack)
}

fn touch_then_read_wake_probe<I2C>(
    i2c: &mut I2C,
    addr: u8,
    cmd: u8,
    touch_stage: &'static str,
    read_stage: &'static str,
    step: u8,
    delay_ms: u64,
    quiet: bool,
) -> Result<u16, bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    for gap_ms in BMS_WAKE_READ_GAPS_MS {
        match touch_bms_command(i2c, addr, cmd) {
            Ok(()) => {
                if !quiet {
                    defmt::warn!(
                        "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} gap_ms={=u64}",
                        addr,
                        touch_stage,
                        step,
                        delay_ms,
                        gap_ms
                    );
                }
            }
            Err(e) => {
                if !quiet {
                    log_bms_diag(addr, touch_stage, e, "cmd", "wake");
                }
                continue;
            }
        }

        spin_delay(Duration::from_millis(gap_ms));
        let mut buf = [0u8; 2];
        match i2c.read(addr, &mut buf) {
            Ok(()) => {
                let raw = u16::from_le_bytes(buf);
                if !quiet {
                    defmt::warn!(
                        "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} gap_ms={=u64} raw=0x{=u16:x}",
                        addr,
                        read_stage,
                        step,
                        delay_ms,
                        gap_ms,
                        raw
                    );
                }
                return Ok(raw);
            }
            Err(e) => {
                if !quiet {
                    defmt::warn!(
                        "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} gap_ms={=u64} err={}",
                        addr,
                        read_stage,
                        step,
                        delay_ms,
                        gap_ms,
                        i2c_error_kind(e)
                    );
                }
            }
        }
    }

    Err(bq40z50::BmsDiagError::I2cNack)
}

fn try_enter_bms_rom_mode_wake_diag<I2C>(
    i2c: &mut I2C,
    addr: u8,
    step: u8,
    delay_ms: u64,
    quiet: bool,
) -> Result<bool, bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let addr_w = addr << 1;
    let pec_44_0f00 = crc8_smbus(&[addr_w, 0x44, 0x02, 0x00, 0x0F]);
    let pec_44_0033 = crc8_smbus(&[addr_w, 0x44, 0x02, 0x33, 0x00]);
    let pec_0f00 = crc8_smbus(&[addr_w, 0x00, 0x0F, 0x00]);
    let pec_0033 = crc8_smbus(&[addr_w, 0x00, 0x00, 0x33]);

    let frames: [(&'static str, &[u8], Duration); 8] = [
        (
            "wake_rom_enter_44_0f00",
            &[0x44, 0x02, 0x00, 0x0F],
            BMS_MAC_TOGGLE_SETTLE,
        ),
        (
            "wake_rom_enter_44_0033",
            &[0x44, 0x02, 0x33, 0x00],
            BMS_MAC_TOGGLE_SETTLE,
        ),
        (
            "wake_rom_enter_44_0f00_pec",
            &[0x44, 0x02, 0x00, 0x0F, pec_44_0f00],
            BMS_MAC_TOGGLE_SETTLE,
        ),
        (
            "wake_rom_enter_44_0033_pec",
            &[0x44, 0x02, 0x33, 0x00, pec_44_0033],
            BMS_MAC_TOGGLE_SETTLE,
        ),
        (
            "wake_rom_enter_0f00",
            &[0x00, 0x0F, 0x00],
            BMS_MAC_TOGGLE_SETTLE,
        ),
        (
            "wake_rom_enter_0033",
            &[0x00, 0x00, 0x33],
            BMS_MAC_TOGGLE_SETTLE,
        ),
        (
            "wake_rom_enter_0f00_pec",
            &[0x00, 0x0F, 0x00, pec_0f00],
            BMS_MAC_TOGGLE_SETTLE,
        ),
        (
            "wake_rom_enter_0033_pec",
            &[0x00, 0x00, 0x33, pec_0033],
            BMS_MAC_TOGGLE_SETTLE,
        ),
    ];

    for (stage, frame, settle) in frames {
        let _ = touch_bms_command(i2c, addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE);
        match i2c.write(addr, frame) {
            Ok(()) => {
                if !quiet {
                    defmt::warn!(
                        "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} write=ok len={=u8}",
                        addr,
                        stage,
                        step,
                        delay_ms,
                        frame.len() as u8
                    );
                }
                spin_delay(settle);
            }
            Err(e) => {
                if !quiet {
                    defmt::warn!(
                        "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} write_err={}",
                        addr,
                        stage,
                        step,
                        delay_ms,
                        i2c_error_kind(e)
                    );
                }
                continue;
            }
        }

        log_bms_wake_read_only_diag(i2c, addr, "wake_rom_raw_read", step, delay_ms, quiet);
        match read_u16_with_optional_pec(i2c, addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE) {
            Ok(sig) => {
                if !quiet {
                    defmt::warn!(
                        "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} rsoc_after=0x{=u16:x}",
                        addr,
                        stage,
                        step,
                        delay_ms,
                        sig
                    );
                }
                if sig == BMS_ROM_MODE_SIGNATURE {
                    return Ok(true);
                }
            }
            Err(e) => {
                if !quiet {
                    log_bms_diag(addr, stage, e, "word", "wake-rom");
                }
            }
        }
    }

    Ok(false)
}

fn confirm_bq40_wake_snapshot<I2C>(
    i2c: &mut I2C,
    addr: u8,
    strict_validation: bool,
    tracker: &mut BmsPatternTracker,
    stage: &'static str,
    step: u8,
    delay_ms: u64,
    quiet: bool,
) -> bool
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    match read_bms_snapshot_strict(i2c, addr, strict_validation, tracker) {
        Ok(snapshot) => {
            if !quiet {
                defmt::warn!(
                    "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} voltage_mv={=u16} soc_pct={=u16} temp_c_x10={=i32}",
                    addr,
                    stage,
                    step,
                    delay_ms,
                    snapshot.voltage_mv,
                    snapshot.soc_pct,
                    snapshot.temp_c_x10,
                );
            }
            true
        }
        Err(e) => {
            if !quiet {
                log_bms_diag(addr, stage, e, "word", "wake-confirm");
            }
            false
        }
    }
}

fn probe_bq40z50_after_wake_touch<I2C>(
    i2c: &mut I2C,
    addr: u8,
    strict_validation: bool,
    tracker: &mut BmsPatternTracker,
    step: u8,
    delay_ms: u64,
    quiet: bool,
) -> Result<WakeWindowProbeResult, bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    match touch_then_read_wake_probe(
        i2c,
        addr,
        bq40z50::cmd::RELATIVE_STATE_OF_CHARGE,
        "wake_touch_read_rsoc",
        "wake_touch_read_rsoc_raw",
        step,
        delay_ms,
        quiet,
    ) {
        Ok(rsoc) => {
            if rsoc == BMS_ROM_MODE_SIGNATURE {
                return Ok(WakeWindowProbeResult::Rom(addr));
            }
            if rsoc <= 100 {
                match touch_then_read_wake_probe(
                    i2c,
                    addr,
                    bq40z50::cmd::TEMPERATURE,
                    "wake_touch_read_temp",
                    "wake_touch_read_temp_raw",
                    step,
                    delay_ms,
                    quiet,
                ) {
                    Ok(temp) if (2_000..=4_300).contains(&temp) => {
                        if confirm_bq40_wake_snapshot(
                            i2c,
                            addr,
                            strict_validation,
                            tracker,
                            "wake_snapshot_confirm_touch",
                            step,
                            delay_ms,
                            quiet,
                        ) {
                            return Ok(WakeWindowProbeResult::Working(addr));
                        }
                    }
                    Ok(_) | Err(_) => {}
                }
            }
        }
        Err(_) => {}
    }

    for round in 0..BMS_WAKE_KEEPALIVE_ROUNDS {
        if round > 0 {
            spin_delay(BMS_WAKE_KEEPALIVE_GAP);
        }

        for (cmd, name) in [
            (
                bq40z50::cmd::RELATIVE_STATE_OF_CHARGE,
                "wake_keepalive_rsoc",
            ),
            (bq40z50::cmd::TEMPERATURE, "wake_keepalive_temp"),
        ] {
            match touch_bms_command(i2c, addr, cmd) {
                Ok(()) => {
                    if !quiet {
                        defmt::warn!(
                            "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} round={=u8}",
                            addr,
                            name,
                            step,
                            delay_ms,
                            round as u8
                        );
                    }
                }
                Err(e) => {
                    if !quiet {
                        log_bms_diag(addr, name, e, "cmd", "wake");
                    }
                }
            }
        }

        log_bms_wake_read_only_diag(i2c, addr, "wake_raw_read", step, delay_ms, quiet);

        match read_u16_wake_probe(
            i2c,
            addr,
            bq40z50::cmd::RELATIVE_STATE_OF_CHARGE,
            "wake_read_rsoc_split",
            step,
            delay_ms,
            round as u8,
            quiet,
        ) {
            Ok(rsoc) => {
                if !quiet {
                    defmt::warn!(
                        "bms_diag: addr=0x{=u8:x} stage=wake_read_rsoc step={=u8} delay_ms={=u64} round={=u8} raw=0x{=u16:x}",
                        addr,
                        step,
                        delay_ms,
                        round as u8,
                        rsoc
                    );
                }
                if rsoc == BMS_ROM_MODE_SIGNATURE {
                    if !quiet {
                        defmt::warn!(
                            "bms_diag: addr=0x{=u8:x} stage=wake_window_rom_signature step={=u8} delay_ms={=u64}",
                            addr,
                            step,
                            delay_ms
                        );
                    }
                    return Ok(WakeWindowProbeResult::Rom(addr));
                }
                if rsoc <= 100 {
                    match read_u16_wake_probe(
                        i2c,
                        addr,
                        bq40z50::cmd::TEMPERATURE,
                        "wake_read_temp_split",
                        step,
                        delay_ms,
                        round as u8,
                        quiet,
                    ) {
                        Ok(temp) => {
                            if !quiet {
                                defmt::warn!(
                                    "bms_diag: addr=0x{=u8:x} stage=wake_read_temp step={=u8} delay_ms={=u64} round={=u8} raw=0x{=u16:x}",
                                    addr,
                                    step,
                                    delay_ms,
                                    round as u8,
                                    temp
                                );
                            }
                            if (2_000..=4_300).contains(&temp)
                                && confirm_bq40_wake_snapshot(
                                    i2c,
                                    addr,
                                    strict_validation,
                                    tracker,
                                    "wake_snapshot_confirm_split",
                                    step,
                                    delay_ms,
                                    quiet,
                                )
                            {
                                return Ok(WakeWindowProbeResult::Working(addr));
                            }
                        }
                        Err(e) => {
                            if !quiet {
                                log_bms_diag(addr, "wake_read_temp", e, "word", "wake");
                            }
                        }
                    }
                }
            }
            Err(e) => {
                if !quiet {
                    log_bms_diag(addr, "wake_read_rsoc", e, "word", "wake");
                }
            }
        }
    }

    if try_enter_bms_rom_mode_wake_diag(i2c, addr, step, delay_ms, quiet)? {
        if !quiet {
            defmt::warn!(
                "bms_diag: addr=0x{=u8:x} stage=wake_window_rom_signature step={=u8} delay_ms={=u64}",
                addr,
                step,
                delay_ms
            );
        }
        return Ok(WakeWindowProbeResult::Rom(addr));
    }

    Ok(WakeWindowProbeResult::Miss)
}

fn maybe_enter_bms_rom_mode_diag<I2C>(
    i2c: &mut I2C,
    addr: u8,
    quiet: bool,
) -> Result<bool, bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let addr_w = addr << 1;

    let mut try_access_word =
        |stage: &'static str, lo: u8, hi: u8| match write_bms_rom_word(i2c, addr, 0x00, lo, hi) {
            Ok(()) => {
                if !quiet {
                    defmt::warn!("bms_diag: addr=0x{=u8:x} stage={} write=ok", addr, stage,);
                }
                spin_delay(BMS_MAC_TOGGLE_SETTLE);
            }
            Err(e) => {
                if !quiet {
                    log_bms_diag(addr, stage, e, "word", "security");
                }
            }
        };

    // Try TI default security transitions before boot-ROM entry. Repo TRM notes:
    // default UNSEAL=0x0414/0x3672, default FULL ACCESS=0xFFFF/0xFFFF.
    try_access_word("security_unseal_0414", 0x14, 0x04);
    try_access_word("security_unseal_3672", 0x72, 0x36);
    try_access_word("security_full_access_ffff_1", 0xFF, 0xFF);
    try_access_word("security_full_access_ffff_2", 0xFF, 0xFF);

    let mut try_enter = |stage: &'static str,
                         frame: &[u8],
                         settle: Duration|
     -> Result<bool, bq40z50::BmsDiagError> {
        if !quiet {
            defmt::warn!("bms_diag: addr=0x{=u8:x} stage={} begin", addr, stage);
        }

        if let Err(e) = i2c.write(addr, frame) {
            if !quiet {
                defmt::warn!(
                    "bms_diag: addr=0x{=u8:x} stage={} write_err={}",
                    addr,
                    stage,
                    i2c_error_kind(e)
                );
            }
            return Ok(false);
        }

        spin_delay(settle);
        match read_u16_with_optional_pec(i2c, addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE) {
            Ok(after) => {
                if !quiet {
                    defmt::warn!(
                        "bms_diag: addr=0x{=u8:x} stage={} rsoc_after=0x{=u16:x}",
                        addr,
                        stage,
                        after
                    );
                }
                Ok(after == BMS_ROM_MODE_SIGNATURE)
            }
            Err(e) => {
                if !quiet {
                    defmt::warn!(
                        "bms_diag: addr=0x{=u8:x} stage={} read_err={}",
                        addr,
                        stage,
                        e
                    );
                }
                Ok(false)
            }
        }
    };

    if try_enter(
        "rom_mode_enter_44_0f00",
        &[0x44, 0x02, 0x00, 0x0F],
        BMS_MAC_TOGGLE_SETTLE,
    )? {
        return Ok(true);
    }
    if try_enter(
        "rom_mode_enter_44_0033",
        &[0x44, 0x02, 0x33, 0x00],
        BMS_MAC_TOGGLE_SETTLE,
    )? {
        return Ok(true);
    }

    let pec_44_0f00 = crc8_smbus(&[addr_w, 0x44, 0x02, 0x00, 0x0F]);
    if try_enter(
        "rom_mode_enter_44_0f00_pec",
        &[0x44, 0x02, 0x00, 0x0F, pec_44_0f00],
        BMS_MAC_TOGGLE_SETTLE,
    )? {
        return Ok(true);
    }

    let pec_44_0033 = crc8_smbus(&[addr_w, 0x44, 0x02, 0x33, 0x00]);
    if try_enter(
        "rom_mode_enter_44_0033_pec",
        &[0x44, 0x02, 0x33, 0x00, pec_44_0033],
        BMS_MAC_TOGGLE_SETTLE,
    )? {
        return Ok(true);
    }

    if try_enter(
        "rom_mode_enter_0f00",
        &[0x00, 0x0F, 0x00],
        BMS_MAC_TOGGLE_SETTLE,
    )? {
        return Ok(true);
    }
    if try_enter(
        "rom_mode_enter_0033",
        &[0x00, 0x00, 0x33],
        BMS_MAC_TOGGLE_SETTLE,
    )? {
        return Ok(true);
    }

    let pec_0f00 = crc8_smbus(&[addr_w, 0x00, 0x0F, 0x00]);
    if try_enter(
        "rom_mode_enter_0f00_pec",
        &[0x00, 0x0F, 0x00, pec_0f00],
        BMS_MAC_TOGGLE_SETTLE,
    )? {
        return Ok(true);
    }

    let pec_0033 = crc8_smbus(&[addr_w, 0x00, 0x00, 0x33]);
    if try_enter(
        "rom_mode_enter_0033_pec",
        &[0x00, 0x00, 0x33, pec_0033],
        BMS_MAC_TOGGLE_SETTLE,
    )? {
        return Ok(true);
    }

    Ok(false)
}

fn maybe_exit_bms_rom_mode<I2C>(
    i2c: &mut I2C,
    addr: u8,
    quiet: bool,
) -> Result<bool, bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let sig = read_u16_with_optional_pec(i2c, addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE)?;
    if sig != BMS_ROM_MODE_SIGNATURE {
        return Ok(true);
    }

    if !quiet {
        defmt::warn!(
            "bms_diag: addr=0x{=u8:x} stage=rom_mode_detected rsoc=0x{=u16:x}",
            addr,
            sig
        );
    }

    let mut try_exit = |stage: &'static str,
                        frame: &[u8],
                        settle: Duration|
     -> Result<bool, bq40z50::BmsDiagError> {
        if let Err(e) = i2c.write(addr, frame) {
            if !quiet {
                defmt::warn!(
                    "bms_diag: addr=0x{=u8:x} stage={} write_err={}",
                    addr,
                    stage,
                    i2c_error_kind(e)
                );
            }
            return Ok(false);
        }

        spin_delay(settle);
        let mut read_attempt = 0u8;
        let after = loop {
            match read_u16_with_optional_pec(i2c, addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE) {
                Ok(v) => break Some(v),
                Err(e) => {
                    read_attempt = read_attempt.saturating_add(1);
                    if !quiet {
                        defmt::warn!(
                            "bms_diag: addr=0x{=u8:x} stage={} read_err={} attempt={=u8}",
                            addr,
                            stage,
                            e,
                            read_attempt
                        );
                    }
                    if read_attempt >= 2 {
                        break None;
                    }
                    // Execute-FW can briefly drop SMBus responses while rebooting.
                    spin_delay(BMS_ROM_EXECUTE_FLASH_SETTLE);
                }
            }
        };

        let Some(after) = after else {
            return Ok(false);
        };

        if !quiet {
            defmt::warn!(
                "bms_diag: addr=0x{=u8:x} stage={} rsoc_before=0x{=u16:x} rsoc_after=0x{=u16:x}",
                addr,
                stage,
                sig,
                after
            );
        }
        Ok(after != BMS_ROM_MODE_SIGNATURE)
    };

    // Recover the broader historical ROM-exit matrix before declaring the gauge stuck in 0x9002.
    let addr_w = addr << 1;
    let pec_08 = crc8_smbus(&[addr_w, 0x08]);
    let pec_0811 = crc8_smbus(&[addr_w, 0x08, 0x11]);
    if try_exit(
        "rom_mode_exit_write_08",
        &[0x08],
        BMS_ROM_EXECUTE_FLASH_SETTLE,
    )? {
        return Ok(true);
    }
    if try_exit(
        "rom_mode_exit_write_08_11",
        &[0x08, 0x11],
        BMS_ROM_EXECUTE_FLASH_SETTLE,
    )? {
        return Ok(true);
    }
    if try_exit(
        "rom_mode_exit_write_08_pec",
        &[0x08, pec_08],
        BMS_ROM_EXECUTE_FLASH_SETTLE,
    )? {
        return Ok(true);
    }
    if try_exit(
        "rom_mode_exit_write_08_11_pec",
        &[0x08, 0x11, pec_0811],
        BMS_ROM_EXECUTE_FLASH_SETTLE,
    )? {
        return Ok(true);
    }

    if !quiet {
        defmt::warn!(
            "bms_diag: addr=0x{=u8:x} stage=rom_mode_exit_failed rsoc_after=0x{=u16:x}",
            addr,
            BMS_ROM_MODE_SIGNATURE
        );
    }
    Ok(false)
}

fn read_bms_snapshot_strict<I2C>(
    i2c: &mut I2C,
    addr: u8,
    strict_validation: bool,
    tracker: &mut BmsPatternTracker,
) -> Result<ValidatedBmsSnapshot, bq40z50::BmsDiagError>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    prime_bms_command_window(i2c, addr)?;
    let mut temp_k_x10 = read_u16_consistent(i2c, addr, bq40z50::cmd::TEMPERATURE, 5)?;
    spin_delay(BMS_WORD_GAP);
    prime_bms_command_window(i2c, addr)?;
    let voltage_mv = read_u16_consistent(i2c, addr, bq40z50::cmd::VOLTAGE, 20)?;
    spin_delay(BMS_WORD_GAP);
    prime_bms_command_window(i2c, addr)?;
    let current_ma = read_i16_consistent(i2c, addr, bq40z50::cmd::CURRENT, 100)?;
    spin_delay(BMS_WORD_GAP);
    prime_bms_command_window(i2c, addr)?;
    let soc_pct = read_u16_consistent(i2c, addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE, 1)?;
    spin_delay(BMS_WORD_GAP);
    prime_bms_command_window(i2c, addr)?;
    let status_raw = read_u16_consistent(i2c, addr, bq40z50::cmd::BATTERY_STATUS, 0)?;
    spin_delay(BMS_WORD_GAP);
    prime_bms_command_window(i2c, addr)?;
    let cell1_mv = read_u16_consistent(i2c, addr, bq40z50::cmd::CELL_VOLTAGE_1, 20)?;
    spin_delay(BMS_WORD_GAP);
    prime_bms_command_window(i2c, addr)?;
    let cell2_mv = read_u16_consistent(i2c, addr, bq40z50::cmd::CELL_VOLTAGE_2, 20)?;
    spin_delay(BMS_WORD_GAP);
    prime_bms_command_window(i2c, addr)?;
    let cell3_mv = read_u16_consistent(i2c, addr, bq40z50::cmd::CELL_VOLTAGE_3, 20)?;
    spin_delay(BMS_WORD_GAP);
    prime_bms_command_window(i2c, addr)?;
    let cell4_mv = read_u16_consistent(i2c, addr, bq40z50::cmd::CELL_VOLTAGE_4, 20)?;
    let mut temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(temp_k_x10);
    if strict_validation && !(-400..=1250).contains(&temp_c_x10) {
        // Temperature occasionally glitches to a transient out-of-range value while other fields
        // remain stable; retry the temperature command once before failing the whole snapshot.
        spin_delay(BMS_WORD_GAP);
        let retry_temp_k_x10 = read_u16_consistent(i2c, addr, bq40z50::cmd::TEMPERATURE, 5)?;
        let retry_temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(retry_temp_k_x10);
        if (-400..=1250).contains(&retry_temp_c_x10) {
            temp_k_x10 = retry_temp_k_x10;
            temp_c_x10 = retry_temp_c_x10;
        }
    }
    let err_code = bq40z50::battery_status::error_code(status_raw);

    let signature = BmsSignature {
        voltage_mv,
        current_ma,
        soc_pct,
        status_raw,
    };
    let repeat_count = tracker.observe(signature);
    let suspicious_tuple = voltage_mv == BMS_SUSPICIOUS_VOLTAGE_MV
        && current_ma == BMS_SUSPICIOUS_CURRENT_MA
        && status_raw == BMS_SUSPICIOUS_STATUS;
    if strict_validation && suspicious_tuple && repeat_count >= 3 {
        defmt::warn!(
            "bms_diag_raw: addr=0x{=u8:x} temp_k_x10={=u16} temp_c_x10={=i32} voltage_mv={=u16} current_ma={=i16} soc_pct={=u16} status=0x{=u16:x} cell1_mv={=u16} cell2_mv={=u16} cell3_mv={=u16} cell4_mv={=u16} repeats={=u8}",
            addr,
            temp_k_x10,
            temp_c_x10,
            voltage_mv,
            current_ma,
            soc_pct,
            status_raw,
            cell1_mv,
            cell2_mv,
            cell3_mv,
            cell4_mv,
            repeat_count
        );
        return Err(bq40z50::BmsDiagError::StalePattern);
    }

    if strict_validation
        && (!(-400..=1250).contains(&temp_c_x10)
            || !(2_500..=20_000).contains(&(voltage_mv as i32))
            || soc_pct > 100)
    {
        defmt::warn!(
            "bms_diag_raw: addr=0x{=u8:x} temp_k_x10={=u16} temp_c_x10={=i32} voltage_mv={=u16} current_ma={=i16} soc_pct={=u16} status=0x{=u16:x} cell1_mv={=u16} cell2_mv={=u16} cell3_mv={=u16} cell4_mv={=u16}",
            addr,
            temp_k_x10,
            temp_c_x10,
            voltage_mv,
            current_ma,
            soc_pct,
            status_raw,
            cell1_mv,
            cell2_mv,
            cell3_mv,
            cell4_mv,
        );
        return Err(bq40z50::BmsDiagError::BadRange);
    }

    // Keep the mandatory snapshot lean. Optional capacity reads move to telemetry-only paths.
    let remaining_cap_mah = Err("skipped");
    let full_cap_mah = Err("skipped");

    Ok(ValidatedBmsSnapshot {
        temp_c_x10,
        voltage_mv,
        current_ma,
        soc_pct,
        status_raw,
        cell1_mv,
        cell2_mv,
        cell3_mv,
        cell4_mv,
        err_code,
        remaining_cap_mah,
        full_cap_mah,
    })
}

fn ascii_contains_case_insensitive(haystack: &str, needle: &[u8]) -> bool {
    let bytes = haystack.as_bytes();
    bytes
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

fn accumulate_irq(dst: &mut IrqSnapshot, src: &IrqSnapshot) {
    dst.i2c1_int = dst.i2c1_int.wrapping_add(src.i2c1_int);
    dst.i2c2_int = dst.i2c2_int.wrapping_add(src.i2c2_int);
    dst.chg_int = dst.chg_int.wrapping_add(src.chg_int);
    dst.ina_pv = dst.ina_pv.wrapping_add(src.ina_pv);
    dst.ina_critical = dst.ina_critical.wrapping_add(src.ina_critical);
    dst.ina_warning = dst.ina_warning.wrapping_add(src.ina_warning);
    dst.bms_btp_int_h = dst.bms_btp_int_h.wrapping_add(src.bms_btp_int_h);
    dst.therm_kill_n = dst.therm_kill_n.wrapping_add(src.therm_kill_n);
}

const fn bms_verbose_diag(mode: bq40z50::BmsAddressMode) -> bool {
    matches!(mode, bq40z50::BmsAddressMode::DualProbeDiag)
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
    bms_address_mode: bq40z50::BmsAddressMode,
    bms_strict_validation: bool,
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
    let tps_a_mode = ::tps55288::Tps55288::with_address(&mut *i2c, OutputChannel::OutA.addr())
        .read_reg(::tps55288::registers::addr::MODE)
        .map(TelemetryU8::Value)
        .unwrap_or_else(|e| TelemetryU8::Err(tps_error_kind(e)));
    let tps_b_mode = ::tps55288::Tps55288::with_address(&mut *i2c, OutputChannel::OutB.addr())
        .read_reg(::tps55288::registers::addr::MODE)
        .map(TelemetryU8::Value)
        .unwrap_or_else(|e| TelemetryU8::Err(tps_error_kind(e)));
    let tps_a_present = matches!(tps_a_mode, TelemetryU8::Value(_));
    let tps_b_present = matches!(tps_b_mode, TelemetryU8::Value(_));
    let tmp_a_present = tmp112::read_temp_c_x16(&mut *i2c, OutputChannel::OutA.tmp_addr()).is_ok();
    let tmp_b_present = tmp112::read_temp_c_x16(&mut *i2c, OutputChannel::OutB.tmp_addr()).is_ok();

    defmt::info!(
        "self_test: i2c1 presence ina3221={=bool} tps_a={=bool} tps_b={=bool} tps_a_mode={} tps_b_mode={} tmp_a={=bool} tmp_b={=bool} bq25792={=bool}",
        ina_present,
        tps_a_present,
        tps_b_present,
        tps_a_mode,
        tps_b_mode,
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

    if want_outputs && !sync_ok {
        defmt::warn!("self_test: tps_sync unavailable; continue output bring-up without sync");
    }

    if want_outputs && ina_present {
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

    let _ = (bms_address_mode, bms_strict_validation);
    let bms_addr: Option<u8> = None;
    defmt::info!(
        "self_test: bq40z50 probe deferred until wake_settle_ms={=u64}",
        BMS_WAKE_SETTLE.as_millis()
    );

    let outputs_ok = (!want_out_a || out_a_ok) && (!want_out_b || out_b_ok);
    defmt::info!(
        "self_test: done enabled_outputs={} outputs_ok={=bool} charger_enabled={=bool}",
        enabled_outputs.describe(),
        outputs_ok,
        charger_enabled
    );

    BootSelfTestResult {
        enabled_outputs,
        charger_enabled,
        bms_addr,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BootChargeMode {
    Off,
    MinCharge,
}

impl BootChargeMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::MinCharge => "min_charge",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BmsStartupStage {
    ProbeWithoutCharge,
    WaitChargeOff,
    WaitMinChargeSettle,
    ProbeWithMinCharge,
    WaitRom,
    Monitoring,
}

impl BmsStartupStage {
    fn as_str(self) -> &'static str {
        match self {
            Self::ProbeWithoutCharge => "probe_without_charge",
            Self::WaitChargeOff => "wait_charge_off",
            Self::WaitMinChargeSettle => "wait_min_charge_settle",
            Self::ProbeWithMinCharge => "probe_with_min_charge",
            Self::WaitRom => "wait_rom",
            Self::Monitoring => "monitoring",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WakeWindowProbeResult {
    Miss,
    Working(u8),
    Rom(u8),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PostFlashResumeResult {
    WaitingBoot,
    WaitingRom,
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

    chg_next_poll_at: Instant,
    chg_next_retry_at: Option<Instant>,
    chg_enabled: bool,
    charger_allowed: bool,
    chg_last_int_poll_at: Option<Instant>,
    charge_mode: BootChargeMode,

    bms_addr: Option<u8>,
    bms_next_poll_at: Instant,
    bms_next_retry_at: Option<Instant>,
    bms_last_int_poll_at: Option<Instant>,
    bms_pattern_tracker: BmsPatternTracker,
    bms_weak_pass_votes: u8,
    bms_last_word_diag_at: Option<Instant>,
    bms_last_word_diag_addr: Option<u8>,
    bms_diag_scan_next_at: Instant,
    bms_missing_diag_next_at: Option<Instant>,
    bms_probe_mode_last: Option<bool>,
    boot_at: Instant,
    bms_isolation_until: Option<Instant>,
    pending_irq: IrqSnapshot,
    bms_btp_irq_total: u32,
    bms_force_recover_attempted: bool,
    bms_last_rom_recover_primary_at: Option<Instant>,
    bms_last_rom_recover_fallback_at: Option<Instant>,
    bms_rom_flash_attempted: bool,
    bms_ship_reset_attempted: bool,
    bms_transport_fail_count: u8,
    bms_startup_stage: BmsStartupStage,
    bms_stage_next_at: Instant,
    bms_wait_rom_started_at: Option<Instant>,
    bms_wait_rom_status_next_at: Option<Instant>,
    bms_exit_exercise_next_at: Option<Instant>,
    bms_exit_exercise_attempts: u16,
    bms_exit_exercise_ack_count: u16,
    bms_exit_exercise_reported: bool,
    bms_post_flash_resume_addr: Option<u8>,
    bms_post_flash_resume_started_at: Option<Instant>,
    bms_post_flash_repower_attempted: bool,
    bms_last_working_info_at: Option<Instant>,
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
    pub charge_allowed: bool,
    pub force_min_charge: bool,
    pub bms_addr: Option<u8>,
    pub bms_diag_isolation: bool,
    pub bms_address_mode: bq40z50::BmsAddressMode,
    pub bms_strict_validation: bool,
    pub bms_staged_probe: bool,
    pub bms_mac_probe_only: bool,
    pub bms_mac_probe_boot_window: Duration,
    pub bms_rom_recover: bool,
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

            chg_next_poll_at: now,
            chg_next_retry_at: Some(now),
            chg_enabled: false,
            charger_allowed,
            chg_last_int_poll_at: None,
            charge_mode: BootChargeMode::Off,

            bms_addr,
            bms_next_poll_at: now,
            bms_next_retry_at: if bms_addr.is_some() { Some(now) } else { None },
            bms_last_int_poll_at: None,
            bms_pattern_tracker: BmsPatternTracker::new(),
            bms_weak_pass_votes: 0,
            bms_last_word_diag_at: None,
            bms_last_word_diag_addr: None,
            bms_diag_scan_next_at: now,
            bms_missing_diag_next_at: if bms_addr.is_some() {
                None
            } else {
                Some(now + BMS_MISSING_VERBOSE_REPROBE_INTERVAL)
            },
            bms_probe_mode_last: None,
            boot_at: now,
            bms_isolation_until: None,
            pending_irq: IrqSnapshot::default(),
            bms_btp_irq_total: 0,
            bms_force_recover_attempted: false,
            bms_last_rom_recover_primary_at: None,
            bms_last_rom_recover_fallback_at: None,
            bms_rom_flash_attempted: false,
            bms_ship_reset_attempted: false,
            bms_transport_fail_count: 0,
            bms_startup_stage: BmsStartupStage::ProbeWithoutCharge,
            bms_stage_next_at: now,
            bms_wait_rom_started_at: None,
            bms_wait_rom_status_next_at: None,
            bms_exit_exercise_next_at: None,
            bms_exit_exercise_attempts: 0,
            bms_exit_exercise_ack_count: 0,
            bms_exit_exercise_reported: false,
            bms_post_flash_resume_addr: None,
            bms_post_flash_resume_started_at: None,
            bms_post_flash_repower_attempted: false,
            bms_last_working_info_at: None,
        }
    }

    fn set_charge_mode(&mut self, mode: BootChargeMode) {
        if self.charge_mode == mode {
            return;
        }

        self.charge_mode = mode;
        self.chg_next_retry_at = Some(Instant::now());
        self.chg_next_poll_at = Instant::now();
        defmt::warn!("charger: boot_charge_mode={}", mode.as_str());
    }

    fn log_bms_signal_line(&mut self, irq: &IrqSnapshot, reason: &'static str) {
        self.bms_btp_irq_total = self.bms_btp_irq_total.wrapping_add(irq.bms_btp_int_h);
        defmt::info!(
            "bms_signal: reason={} stage={} charge_mode={} gpio21_high={=bool} edge_delta={=u32} edge_total={=u32} addr={=?} post_flash_addr={=?} i2c1_int_delta={=u32}",
            reason,
            self.bms_startup_stage.as_str(),
            self.charge_mode.as_str(),
            self.bms_btp_int_h.is_high(),
            irq.bms_btp_int_h,
            self.bms_btp_irq_total,
            self.bms_addr,
            self.bms_post_flash_resume_addr,
            irq.i2c1_int,
        );
    }

    fn probe_bq40z50_without_recover(&mut self, quiet: bool) -> Option<u8> {
        let old = self.cfg.bms_rom_recover;
        self.cfg.bms_rom_recover = false;
        let found = self.probe_bq40z50_impl(quiet);
        self.cfg.bms_rom_recover = old;
        found
    }

    fn detect_bq40z50_rom_signature(&mut self, quiet: bool) -> Option<u8> {
        for &addr in self.cfg.bms_address_mode.candidates() {
            match read_u16_with_optional_pec(
                &mut self.i2c,
                addr,
                bq40z50::cmd::RELATIVE_STATE_OF_CHARGE,
            ) {
                Ok(sig) if sig == BMS_ROM_MODE_SIGNATURE => {
                    if !quiet {
                        defmt::warn!(
                            "bms_diag: addr=0x{=u8:x} stage=rom_mode_detected rsoc=0x{=u16:x}",
                            addr,
                            sig
                        );
                    }
                    return Some(addr);
                }
                _ => {}
            }
        }
        None
    }

    fn clear_post_flash_resume(&mut self) {
        self.bms_post_flash_resume_addr = None;
        self.bms_post_flash_resume_started_at = None;
        self.bms_post_flash_repower_attempted = false;
    }

    fn arm_post_flash_resume(&mut self, addr: u8, started_at: Instant) {
        self.bms_post_flash_resume_addr = Some(addr);
        self.bms_post_flash_resume_started_at = Some(started_at);
        self.bms_post_flash_repower_attempted = false;
    }

    fn mark_bms_working(&mut self, addr: u8) {
        let now = Instant::now();
        self.bms_addr = Some(addr);
        self.bms_next_retry_at = Some(now);
        self.bms_next_poll_at = now;
        self.bms_last_int_poll_at = None;
        self.bms_transport_fail_count = 0;
        self.bms_missing_diag_next_at = None;
        self.bms_startup_stage = BmsStartupStage::Monitoring;
        self.bms_stage_next_at = now;
        self.bms_wait_rom_started_at = None;
        self.bms_wait_rom_status_next_at = None;
        self.bms_exit_exercise_next_at = None;
        self.bms_exit_exercise_attempts = 0;
        self.bms_exit_exercise_ack_count = 0;
        self.bms_exit_exercise_reported = false;
        self.clear_post_flash_resume();
        self.bms_last_working_info_at = None;
        defmt::warn!(
            "bms_flow: stage={} addr=0x{=u8:x} charge_mode={}",
            self.bms_startup_stage.as_str(),
            addr,
            self.charge_mode.as_str()
        );
    }

    fn rom_recover_due(&self, addr: u8, now: Instant) -> bool {
        let last = if addr == bq40z50::I2C_ADDRESS_FALLBACK {
            self.bms_last_rom_recover_fallback_at
        } else {
            self.bms_last_rom_recover_primary_at
        };
        last.map_or(true, |prev| now >= prev + BMS_ROM_RECOVER_MIN_INTERVAL)
    }

    fn note_rom_recover_attempt(&mut self, addr: u8, now: Instant) {
        if addr == bq40z50::I2C_ADDRESS_FALLBACK {
            self.bms_last_rom_recover_fallback_at = Some(now);
        } else {
            self.bms_last_rom_recover_primary_at = Some(now);
        }
        self.bms_rom_flash_attempted = true;
    }

    fn blind_force_recover_ready(&self, now: Instant) -> bool {
        if !cfg!(feature = "bms-rom-recover-force") {
            return false;
        }
        if self.bms_startup_stage != BmsStartupStage::WaitRom {
            return true;
        }
        self.bms_wait_rom_started_at.map_or(false, |started| {
            now >= started + BMS_ROM_FORCE_MIN_CHARGE_DWELL
        })
    }

    fn attempt_bq40_rom_flash(&mut self, addr: u8, quiet: bool) {
        let now = Instant::now();
        if !self.rom_recover_due(addr, now) {
            if !quiet {
                defmt::warn!(
                    "bms_flow: stage={} addr=0x{=u8:x} recover=deferred wait_ms={=u64}",
                    self.bms_startup_stage.as_str(),
                    addr,
                    BMS_ROM_RECOVER_MIN_INTERVAL.as_millis() as u64
                );
            }
            return;
        }

        self.note_rom_recover_attempt(addr, now);
        self.clear_post_flash_resume();
        self.maybe_dwell_before_rom_flash(addr, quiet);
        if !quiet {
            defmt::warn!("bms_diag: addr=0x{=u8:x} stage=probe_rom_flash_begin", addr);
        }
        match run_bms_rom_flash_recover_sequence(&mut self.i2c, addr, quiet) {
            Ok(true) => {
                maybe_enable_bms_runtime_after_flash(&mut self.i2c, addr, quiet);
                self.clear_post_flash_resume();
                if !quiet {
                    defmt::warn!("bms_diag: addr=0x{=u8:x} stage=probe_rom_flash_done", addr);
                }
            }
            Ok(false) => {
                self.clear_post_flash_resume();
                if !quiet {
                    defmt::warn!(
                        "bms_diag: addr=0x{=u8:x} stage=probe_rom_flash_skipped",
                        addr
                    );
                }
            }
            Err(e) => {
                if !quiet {
                    log_bms_diag(addr, "probe_rom_flash", e, "word", "srec");
                }
                if matches!(e, bq40z50::BmsDiagError::InconsistentSample) {
                    let pending_at = Instant::now();
                    self.arm_post_flash_resume(addr, pending_at);
                    self.bms_stage_next_at = pending_at + BMS_POST_FLASH_BOOT_QUIET;
                    if !quiet {
                        defmt::warn!(
                            "bms_diag: addr=0x{=u8:x} stage=probe_rom_flash_resume_armed keep_charge=true next_probe_ms={=u64}",
                            addr,
                            BMS_POST_FLASH_BOOT_QUIET.as_millis() as u64
                        );
                    }
                }
            }
        }
    }

    fn maybe_exercise_bms_exit_conditions(
        &mut self,
        now: Instant,
        quiet: bool,
    ) -> Option<WakeWindowProbeResult> {
        if self.bms_startup_stage != BmsStartupStage::WaitRom || self.bms_rom_flash_attempted {
            return None;
        }

        let Some(started) = self.bms_wait_rom_started_at else {
            return None;
        };

        let elapsed = now - started;
        if elapsed >= BMS_EXIT_EXERCISE_WINDOW {
            if !self.bms_exit_exercise_reported {
                defmt::warn!(
                    "bms_diag: stage=emshut_shutdown_exit_window_done keep_charge={=bool} dwell_ms={=u64} attempts={=u16} ack_total={=u16}",
                    self.charge_mode == BootChargeMode::MinCharge,
                    elapsed.as_millis() as u64,
                    self.bms_exit_exercise_attempts,
                    self.bms_exit_exercise_ack_count,
                );
                self.bms_exit_exercise_reported = true;
            }
            self.bms_exit_exercise_next_at = None;
            return None;
        }

        if self
            .bms_exit_exercise_next_at
            .map_or(false, |next| now < next)
        {
            return None;
        }

        self.bms_exit_exercise_next_at = Some(now + BMS_EXIT_EXERCISE_PERIOD);
        self.bms_exit_exercise_attempts = self.bms_exit_exercise_attempts.saturating_add(1);
        let attempt = self.bms_exit_exercise_attempts;
        let dwell_ms = elapsed.as_millis() as u64;

        if !quiet {
            defmt::warn!(
                "bms_diag: stage=emshut_shutdown_exit_window keep_charge={=bool} dwell_ms={=u64} attempt={=u16}",
                self.charge_mode == BootChargeMode::MinCharge,
                dwell_ms,
                attempt,
            );
        }

        for &addr in self.cfg.bms_address_mode.candidates() {
            let mut addr_acked = false;
            for &cmd in &[
                bq40z50::cmd::RELATIVE_STATE_OF_CHARGE,
                bq40z50::cmd::TEMPERATURE,
            ] {
                match touch_bms_command(&mut self.i2c, addr, cmd) {
                    Ok(()) => {
                        addr_acked = true;
                        self.bms_exit_exercise_ack_count =
                            self.bms_exit_exercise_ack_count.saturating_add(1);
                        defmt::warn!(
                            "bms_diag: addr=0x{=u8:x} stage=emshut_shutdown_exit_touch cmd=0x{=u8:x} dwell_ms={=u64} attempt={=u16}",
                            addr,
                            cmd,
                            dwell_ms,
                            attempt,
                        );

                        match probe_bq40z50_after_wake_touch(
                            &mut self.i2c,
                            addr,
                            self.cfg.bms_strict_validation,
                            &mut self.bms_pattern_tracker,
                            attempt.min(u8::MAX as u16) as u8,
                            dwell_ms,
                            quiet,
                        ) {
                            Ok(WakeWindowProbeResult::Working(found)) => {
                                return Some(WakeWindowProbeResult::Working(found));
                            }
                            Ok(WakeWindowProbeResult::Rom(found)) => {
                                return Some(WakeWindowProbeResult::Rom(found));
                            }
                            Ok(WakeWindowProbeResult::Miss) => {}
                            Err(e) => {
                                if !quiet {
                                    log_bms_diag(
                                        addr,
                                        "emshut_shutdown_exit_probe",
                                        e,
                                        "word",
                                        "exit",
                                    );
                                }
                            }
                        }
                    }
                    Err(e) => {
                        if !quiet {
                            log_bms_diag(addr, "emshut_shutdown_exit_touch", e, "cmd", "exit");
                        }
                    }
                }
            }

            if !addr_acked && !quiet {
                defmt::warn!(
                    "bms_diag: addr=0x{=u8:x} stage=emshut_shutdown_exit_addr_miss dwell_ms={=u64} attempt={=u16}",
                    addr,
                    dwell_ms,
                    attempt,
                );
            }
        }

        None
    }

    fn maybe_handle_post_flash_resume(&mut self, quiet: bool) -> Option<PostFlashResumeResult> {
        let now = Instant::now();
        let addr = self.bms_post_flash_resume_addr?;
        let started = self.bms_post_flash_resume_started_at.unwrap_or(now);
        let deadline = started + BMS_POST_FLASH_RESUME_WINDOW;

        if now < deadline {
            if !quiet {
                defmt::warn!(
                    "bms_diag: addr=0x{=u8:x} stage=probe_rom_post_flash_quiet_wait remaining_ms={=u64}",
                    addr,
                    (deadline - now).as_millis() as u64
                );
            }
            return Some(PostFlashResumeResult::WaitingBoot);
        }

        if !quiet {
            defmt::warn!(
                "bms_diag: addr=0x{=u8:x} stage=probe_rom_post_flash_expired window_ms={=u64}",
                addr,
                BMS_POST_FLASH_RESUME_WINDOW.as_millis() as u64
            );
        }

        match run_bms_rom_postflash_resume_sequence(&mut self.i2c, addr, quiet) {
            Ok(true) => {
                self.clear_post_flash_resume();
                if !quiet {
                    defmt::warn!(
                        "bms_diag: addr=0x{=u8:x} stage=probe_rom_post_flash_fw_seen",
                        addr
                    );
                }
                None
            }
            Ok(false) => {
                self.clear_post_flash_resume();
                if !quiet {
                    defmt::warn!(
                        "bms_diag: addr=0x{=u8:x} stage=probe_rom_post_flash_still_rom keep_charge=true",
                        addr
                    );
                }
                None
            }
            Err(e) => {
                self.clear_post_flash_resume();
                if !quiet {
                    log_bms_diag(addr, "probe_rom_post_flash_resume", e, "word", "rom-mode");
                }
                None
            }
        }
    }

    fn maybe_run_bms_startup_flow(&mut self) -> bool {
        let now = Instant::now();
        if now < self.bms_stage_next_at {
            return false;
        }

        match self.bms_startup_stage {
            BmsStartupStage::ProbeWithoutCharge => {
                if let Some(addr) = self.probe_bq40z50_without_recover(false) {
                    self.mark_bms_working(addr);
                    return true;
                }

                self.bms_startup_stage = BmsStartupStage::WaitChargeOff;
                self.bms_stage_next_at = now + BMS_FORCE_MIN_CHARGE_REPOWER_OFF;
                defmt::warn!(
                    "bms_flow: stage={} next={} off_ms={=u64}",
                    BmsStartupStage::ProbeWithoutCharge.as_str(),
                    self.bms_startup_stage.as_str(),
                    BMS_FORCE_MIN_CHARGE_REPOWER_OFF.as_millis() as u64
                );
                true
            }
            BmsStartupStage::WaitChargeOff => {
                self.set_charge_mode(BootChargeMode::MinCharge);
                self.maybe_poll_charger(&IrqSnapshot::default());
                // Match the agreed recovery flow exactly: after the forced no-charge repower
                // window, hold minimum charge for 2 s before the first BQ probe.
                self.bms_startup_stage = BmsStartupStage::WaitMinChargeSettle;
                self.bms_stage_next_at = now + BMS_BOOT_MIN_CHARGE_SETTLE;
                defmt::warn!(
                    "bms_flow: stage={} next={} settle_ms={=u64} charge_mode=min_charge",
                    BmsStartupStage::WaitChargeOff.as_str(),
                    self.bms_startup_stage.as_str(),
                    BMS_BOOT_MIN_CHARGE_SETTLE.as_millis() as u64
                );
                true
            }
            BmsStartupStage::WaitMinChargeSettle => {
                self.bms_startup_stage = BmsStartupStage::ProbeWithMinCharge;
                self.bms_stage_next_at = now;
                defmt::warn!(
                    "bms_flow: stage={} next={}",
                    BmsStartupStage::WaitMinChargeSettle.as_str(),
                    self.bms_startup_stage.as_str()
                );
                true
            }
            BmsStartupStage::ProbeWithMinCharge => {
                match self.probe_bq40z50_wake_window(false) {
                    WakeWindowProbeResult::Working(addr) => {
                        self.mark_bms_working(addr);
                        return true;
                    }
                    WakeWindowProbeResult::Rom(addr) => {
                        if self.cfg.bms_rom_recover && self.bms_post_flash_resume_addr.is_none() {
                            self.attempt_bq40_rom_flash(addr, false);
                            if self.bms_post_flash_resume_addr.is_some() {
                                return true;
                            }
                        }
                    }
                    WakeWindowProbeResult::Miss => {}
                }

                self.bms_startup_stage = BmsStartupStage::WaitRom;
                self.bms_stage_next_at = now;
                self.bms_wait_rom_started_at = Some(now);
                self.bms_wait_rom_status_next_at = Some(now);
                self.bms_exit_exercise_next_at = Some(now);
                self.bms_exit_exercise_attempts = 0;
                self.bms_exit_exercise_ack_count = 0;
                self.bms_exit_exercise_reported = false;
                defmt::warn!(
                    "bms_flow: stage={} next={} probe_ms={=u64} status_ms={=u64}",
                    BmsStartupStage::ProbeWithMinCharge.as_str(),
                    self.bms_startup_stage.as_str(),
                    BMS_WAIT_ROM_FAST_POLL_PERIOD.as_millis() as u64,
                    BMS_BOOT_STAGE_POLL_PERIOD.as_millis() as u64
                );
                true
            }
            BmsStartupStage::WaitRom => {
                let status_probe_due = self
                    .bms_wait_rom_status_next_at
                    .map_or(true, |next| now >= next);
                let quiet = !status_probe_due;

                if self.bms_post_flash_resume_addr.is_some() {
                    if let Some(started) = self.bms_post_flash_resume_started_at {
                        let passive_deadline = started + BMS_POST_FLASH_RESUME_WINDOW;
                        if now < passive_deadline {
                            if status_probe_due {
                                defmt::warn!(
                                    "bms_flow: stage={} rom=post_flash_passive_wait keep_charge=true next_probe_ms={=u64}",
                                    self.bms_startup_stage.as_str(),
                                    (passive_deadline - now).as_millis() as u64
                                );
                            }
                            self.bms_stage_next_at = passive_deadline;
                            if status_probe_due {
                                self.bms_wait_rom_status_next_at = Some(passive_deadline);
                            }
                            return true;
                        }
                    }

                    if let Some(result) = self.maybe_handle_post_flash_resume(quiet) {
                        match result {
                            PostFlashResumeResult::WaitingBoot => {
                                self.bms_stage_next_at = now + BMS_POST_FLASH_BOOT_QUIET;
                                return true;
                            }
                            PostFlashResumeResult::WaitingRom => {
                                if status_probe_due {
                                    defmt::warn!(
                                        "bms_flow: stage={} rom=post_flash_resume_wait keep_charge=true next_probe_ms={=u64}",
                                        self.bms_startup_stage.as_str(),
                                        BMS_BOOT_STAGE_POLL_PERIOD.as_millis() as u64
                                    );
                                }
                                self.bms_stage_next_at = now + BMS_BOOT_STAGE_POLL_PERIOD;
                                if status_probe_due {
                                    self.bms_wait_rom_status_next_at =
                                        Some(now + BMS_BOOT_STAGE_POLL_PERIOD);
                                }
                                return true;
                            }
                        }
                    }
                }

                if let Some(result) = self.maybe_exercise_bms_exit_conditions(now, quiet) {
                    match result {
                        WakeWindowProbeResult::Working(addr) => {
                            self.mark_bms_working(addr);
                            return true;
                        }
                        WakeWindowProbeResult::Rom(addr) => {
                            if self.cfg.bms_rom_recover && self.bms_post_flash_resume_addr.is_none()
                            {
                                self.attempt_bq40_rom_flash(addr, quiet);
                                if self.bms_post_flash_resume_addr.is_some() {
                                    return true;
                                }
                            }
                        }
                        WakeWindowProbeResult::Miss => {}
                    }
                }

                if let Some(addr) = self.probe_bq40z50_without_recover(quiet) {
                    self.mark_bms_working(addr);
                    return true;
                }

                let mut rom_waiting = true;
                if self.cfg.bms_rom_recover {
                    if let Some(addr) = self.probe_bq40z50_impl(quiet) {
                        self.mark_bms_working(addr);
                        return true;
                    }
                    rom_waiting = !self.bms_rom_flash_attempted;
                } else if let Some(addr) = self.detect_bq40z50_rom_signature(quiet) {
                    rom_waiting = false;
                    if status_probe_due {
                        defmt::warn!(
                            "bms_flow: stage={} rom=detected recover=disabled addr=0x{=u8:x}",
                            self.bms_startup_stage.as_str(),
                            addr
                        );
                    }
                }

                if rom_waiting && status_probe_due {
                    let blind_force_wait_ms = if cfg!(feature = "bms-rom-recover-force") {
                        self.bms_wait_rom_started_at.map(|started| {
                            let deadline = started + BMS_ROM_FORCE_MIN_CHARGE_DWELL;
                            if now >= deadline {
                                0
                            } else {
                                (deadline - now).as_millis() as u64
                            }
                        })
                    } else {
                        None
                    };

                    if let Some(wait_ms) = blind_force_wait_ms {
                        defmt::warn!(
                            "bms_flow: stage={} rom=waiting keep_charge=true next_probe_ms={=u64} blind_force_wait_ms={=u64}",
                            self.bms_startup_stage.as_str(),
                            BMS_WAIT_ROM_FAST_POLL_PERIOD.as_millis() as u64,
                            wait_ms
                        );
                    } else {
                        defmt::warn!(
                            "bms_flow: stage={} rom=waiting keep_charge=true next_probe_ms={=u64}",
                            self.bms_startup_stage.as_str(),
                            BMS_WAIT_ROM_FAST_POLL_PERIOD.as_millis() as u64
                        );
                    }
                }
                self.bms_stage_next_at = now + BMS_WAIT_ROM_FAST_POLL_PERIOD;
                if status_probe_due {
                    self.bms_wait_rom_status_next_at = Some(now + BMS_BOOT_STAGE_POLL_PERIOD);
                }
                true
            }
            BmsStartupStage::Monitoring => false,
        }
    }

    fn maybe_dwell_before_rom_flash(&mut self, addr: u8, quiet: bool) {
        if !self.cfg.force_min_charge || !self.charger_allowed {
            return;
        }

        const ROM_RECOVER_ICHG_MA: u16 = 200;
        const ROM_RECOVER_IINDPM_MA: u16 = 500;

        if !quiet {
            defmt::warn!(
                "bms_diag: addr=0x{=u8:x} stage=rom_flash_charge_dwell_begin ms={=u64}",
                addr,
                BMS_ROM_FORCE_MIN_CHARGE_DWELL.as_millis() as u64
            );
        }

        // The 10 s force-min-charge repower has already happened before ROM recovery. Keep the
        // proven wake profile active while flashing instead of boosting current again, so the
        // pack stays biased but we avoid introducing another large power-state step right here.
        if let Err(e) = bq25792::set_charge_current_limit_ma(&mut self.i2c, ROM_RECOVER_ICHG_MA) {
            defmt::warn!(
                "charger: rom_recover_boost ichg_write err={}",
                i2c_error_kind(e)
            );
        }
        if let Err(e) = bq25792::set_input_current_limit_ma(&mut self.i2c, ROM_RECOVER_IINDPM_MA) {
            defmt::warn!(
                "charger: rom_recover_boost iindpm_write err={}",
                i2c_error_kind(e)
            );
        }

        spin_delay(BMS_ROM_FORCE_MIN_CHARGE_DWELL);

        if !quiet {
            defmt::warn!(
                "bms_diag: addr=0x{=u8:x} stage=rom_flash_charge_dwell_done",
                addr
            );
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

        // New recovery flow: stop charging first, probe BQ normally, then switch to minimum
        // charge and continue staged probing/recovery from the main loop.
        self.set_charge_mode(BootChargeMode::Off);
        if self.charger_allowed {
            self.maybe_poll_charger(&IrqSnapshot::default());
        }

        self.bms_addr = None;
        self.bms_next_retry_at = None;
        self.bms_next_poll_at = Instant::now();
        self.bms_missing_diag_next_at = Some(Instant::now() + BMS_MISSING_VERBOSE_REPROBE_INTERVAL);
        self.bms_startup_stage = BmsStartupStage::ProbeWithoutCharge;
        self.bms_stage_next_at = Instant::now();
        self.bms_wait_rom_started_at = None;
        self.bms_wait_rom_status_next_at = None;
        self.bms_exit_exercise_next_at = None;
        self.bms_exit_exercise_attempts = 0;
        self.bms_exit_exercise_ack_count = 0;
        self.bms_exit_exercise_reported = false;
        self.clear_post_flash_resume();
        self.bms_last_working_info_at = None;
        defmt::warn!(
            "bms_flow: stage={} charge_mode={} rom_recover={=bool}",
            self.bms_startup_stage.as_str(),
            self.charge_mode.as_str(),
            self.cfg.bms_rom_recover
        );
        self.log_bms_signal_line(&IrqSnapshot::default(), "boot_init");
    }

    pub fn tick(&mut self, irq: &IrqSnapshot) {
        accumulate_irq(&mut self.pending_irq, irq);

        if let Some(until) = self.bms_isolation_until {
            if self.cfg.bms_diag_isolation && Instant::now() < until {
                return;
            }
            self.bms_isolation_until = None;
        }

        let irq = self.pending_irq;
        self.pending_irq = IrqSnapshot::default();

        self.maybe_retry();
        self.maybe_handle_fault(&irq);

        // Keep the SMBus completely quiet during the post-flash boot window so the gauge gets the
        // same uninterrupted settle time that TI's SREC flow expects after Execute 0x08.
        if self.bms_post_flash_resume_addr.is_some() && Instant::now() < self.bms_stage_next_at {
            return;
        }

        // Keep the charger alive during both the staged recovery flow and steady-state monitoring.
        self.maybe_poll_charger(&irq);

        let bms_i2c_active = if self.bms_startup_stage == BmsStartupStage::Monitoring {
            self.maybe_poll_bms(&irq)
        } else {
            self.maybe_run_bms_startup_flow()
        };
        if self.cfg.bms_diag_isolation && bms_i2c_active {
            self.bms_isolation_until = Some(Instant::now() + BMS_ISOLATION_WINDOW);
            return;
        }

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

    fn probe_bq40z50(&mut self) -> Option<u8> {
        self.probe_bq40z50_impl(false)
    }

    fn probe_bq40z50_quiet(&mut self) -> Option<u8> {
        self.probe_bq40z50_impl(true)
    }

    fn probe_bq40z50_diag_scan(&mut self, quiet: bool) -> Option<u8> {
        for addr in BMS_DIAG_SCAN_MIN_ADDR..=BMS_DIAG_SCAN_MAX_ADDR {
            if addr == bq40z50::I2C_ADDRESS_PRIMARY || addr == bq40z50::I2C_ADDRESS_FALLBACK {
                continue;
            }
            // Skip known non-BMS devices on this board to reduce false positives.
            if matches!(addr, 0x40 | 0x48 | 0x49 | 0x6B | 0x74 | 0x75) {
                continue;
            }

            let rsoc = match read_u16_with_optional_pec(
                &mut self.i2c,
                addr,
                bq40z50::cmd::RELATIVE_STATE_OF_CHARGE,
            ) {
                Ok(v) => v,
                Err(_) => continue,
            };

            // Candidate profile:
            // - ROM mode signature (0x9002), or
            // - runtime RSOC 0..=100 with a plausible temperature word.
            if rsoc != BMS_ROM_MODE_SIGNATURE && rsoc > 100 {
                continue;
            }

            let temp =
                read_u16_with_optional_pec(&mut self.i2c, addr, bq40z50::cmd::TEMPERATURE).ok();
            let temp_plausible = temp.map_or(false, |t| (2_000..=4_300).contains(&t));
            if rsoc != BMS_ROM_MODE_SIGNATURE && !temp_plausible {
                continue;
            }

            if !quiet {
                defmt::warn!(
                    "bms_diag: stage=addr_scan_hit addr=0x{=u8:x} rsoc_raw=0x{=u16:x} temp_raw={=?}",
                    addr,
                    rsoc,
                    temp
                );
            }
            return Some(addr);
        }
        None
    }

    fn probe_bq40z50_impl(&mut self, quiet: bool) -> Option<u8> {
        let elapsed = self.boot_at.elapsed();
        let use_mac_probe_only =
            self.cfg.bms_mac_probe_only && elapsed < self.cfg.bms_mac_probe_boot_window;
        if self.bms_probe_mode_last != Some(use_mac_probe_only) {
            self.bms_probe_mode_last = Some(use_mac_probe_only);
            defmt::info!(
                "bms: probe_mode={} elapsed_ms={=u64} window_ms={=u64}",
                if use_mac_probe_only {
                    "mac_only"
                } else {
                    "strict_word"
                },
                elapsed.as_millis(),
                self.cfg.bms_mac_probe_boot_window.as_millis()
            );
        }

        let read_name_blocks = matches!(
            self.cfg.bms_address_mode,
            bq40z50::BmsAddressMode::DualProbeDiag
        );
        let force_rom_recover = cfg!(feature = "bms-rom-recover-force");
        let blind_force_recover =
            force_rom_recover && self.blind_force_recover_ready(Instant::now());
        for &addr in self.cfg.bms_address_mode.candidates() {
            if self.cfg.bms_rom_recover {
                let now = Instant::now();
                let last_recover_at = if addr == bq40z50::I2C_ADDRESS_FALLBACK {
                    self.bms_last_rom_recover_fallback_at
                } else {
                    self.bms_last_rom_recover_primary_at
                };
                let should_recover =
                    last_recover_at.map_or(true, |last| now >= last + BMS_ROM_RECOVER_MIN_INTERVAL);
                if should_recover {
                    let mut rom_mode_ready = false;
                    if self.bms_rom_flash_attempted {
                        match read_u16_with_optional_pec(
                            &mut self.i2c,
                            addr,
                            bq40z50::cmd::RELATIVE_STATE_OF_CHARGE,
                        ) {
                            Ok(sig) if sig == BMS_ROM_MODE_SIGNATURE => {
                                rom_mode_ready = true;
                                if !quiet {
                                    defmt::warn!(
                                        "bms_diag: addr=0x{=u8:x} stage=rom_mode_detected_post_flash rsoc=0x{=u16:x}",
                                        addr,
                                        sig
                                    );
                                }
                            }
                            Ok(_) => {}
                            Err(e) => {
                                if !quiet {
                                    log_bms_diag(
                                        addr,
                                        "probe_rom_post_flash_state",
                                        e,
                                        "word",
                                        "rom-mode",
                                    );
                                }
                            }
                        }
                    } else {
                        match maybe_exit_bms_rom_mode(&mut self.i2c, addr, quiet) {
                            Ok(true) => {
                                if matches!(
                                    self.cfg.bms_address_mode,
                                    bq40z50::BmsAddressMode::DualProbeDiag
                                ) {
                                    match maybe_enter_bms_rom_mode_diag(&mut self.i2c, addr, quiet)
                                    {
                                        Ok(true) => {
                                            rom_mode_ready = true;
                                            if !quiet {
                                                defmt::warn!(
                                                    "bms_diag: addr=0x{=u8:x} stage=rom_mode_detected_after_enter",
                                                    addr
                                                );
                                            }
                                        }
                                        Ok(false) => {
                                            // `--recover force` must be able to run recovery even when ROM
                                            // signature is not observed.
                                            if blind_force_recover && !self.bms_rom_flash_attempted
                                            {
                                                self.attempt_bq40_rom_flash(addr, quiet);
                                            }
                                        }
                                        Err(e) => {
                                            if !quiet {
                                                log_bms_diag(
                                                    addr,
                                                    "probe_rom_enter",
                                                    e,
                                                    "word",
                                                    "rom-mode",
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                            Ok(false) => {
                                rom_mode_ready = true;
                            }
                            Err(e) => {
                                if !quiet {
                                    log_bms_diag(addr, "probe_rom_exit", e, "word", "rom-mode");
                                    if bms_verbose_diag(self.cfg.bms_address_mode) {
                                        self.maybe_log_bms_word_diag(addr, "probe_rom_exit", e);
                                    }
                                }
                                if matches!(
                                    self.cfg.bms_address_mode,
                                    bq40z50::BmsAddressMode::DualProbeDiag
                                ) {
                                    match maybe_enter_bms_rom_mode_diag(&mut self.i2c, addr, quiet)
                                    {
                                        Ok(true) => {
                                            rom_mode_ready = true;
                                            if !quiet {
                                                defmt::warn!(
                                                    "bms_diag: addr=0x{=u8:x} stage=rom_mode_detected_after_enter",
                                                    addr
                                                );
                                            }
                                        }
                                        Ok(false) => {
                                            if blind_force_recover && !self.bms_rom_flash_attempted
                                            {
                                                self.attempt_bq40_rom_flash(addr, quiet);
                                            }
                                        }
                                        Err(enter_err) => {
                                            if !quiet {
                                                log_bms_diag(
                                                    addr,
                                                    "probe_rom_enter",
                                                    enter_err,
                                                    "word",
                                                    "rom-mode",
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if rom_mode_ready {
                        // TI's ROM-mode recovery guidance allows restarting the flashstream from
                        // the first ROM command if the part is still stuck in ROM after a failed
                        // attempt. Keep doing that on our bounded retry cadence instead of giving
                        // up after the first incomplete write.
                        if matches!(
                            self.cfg.bms_address_mode,
                            bq40z50::BmsAddressMode::DualProbeDiag
                        ) {
                            if !self.bms_rom_flash_attempted {
                                self.attempt_bq40_rom_flash(addr, quiet);
                            } else if should_recover {
                                if !quiet {
                                    defmt::warn!(
                                        "bms_diag: addr=0x{=u8:x} stage=probe_rom_flash_retry keep_charge=true",
                                        addr
                                    );
                                }
                                self.attempt_bq40_rom_flash(addr, quiet);
                            }
                        }

                        if !quiet {
                            defmt::warn!(
                                "bms: bq40z50 probe_wait addr=0x{=u8:x} reason=rom_mode",
                                addr
                            );
                        }
                        continue;
                    }
                }
            }

            if use_mac_probe_only {
                match read_bms_mac_probe_checked(&mut self.i2c, addr) {
                    Ok(snapshot) => {
                        self.bms_weak_pass_votes = 0;
                        if !quiet {
                            defmt::warn!(
                                "bms: bq40z50 mac_probe_ok addr=0x{=u8:x} len={=u8} payload={=u8} b0=0x{=u8:x} b1=0x{=u8:x} b2=0x{=u8:x} b3=0x{=u8:x}",
                                addr,
                                snapshot.declared_len,
                                snapshot.payload_len,
                                snapshot.b0,
                                snapshot.b1,
                                snapshot.b2,
                                snapshot.b3
                            );
                        }
                        return Some(addr);
                    }
                    Err(e) => {
                        self.bms_weak_pass_votes = 0;
                        if !quiet {
                            log_bms_diag(addr, "probe_mac", e, "block", "mac-only");
                            if bms_verbose_diag(self.cfg.bms_address_mode) {
                                self.maybe_log_bms_word_diag(addr, "probe_mac", e);
                                log_bms_mac_diag(&mut self.i2c, addr);
                            }
                            defmt::warn!("bms: bq40z50 probe_miss addr=0x{=u8:x} err={}", addr, e);
                        }
                    }
                }
                continue;
            }

            let mut mfg_buf = [0u8; 32];
            let mut dev_buf = [0u8; 32];
            let mfg_name = if read_name_blocks {
                match read_ascii_block_checked(
                    &mut self.i2c,
                    addr,
                    bq40z50::cmd::MANUFACTURER_NAME,
                    &mut mfg_buf,
                ) {
                    Ok(v) => Some(v),
                    Err(e) => {
                        if !quiet {
                            log_bms_diag(addr, "probe_mfg", e, "block", "ascii");
                        }
                        None
                    }
                }
            } else {
                None
            };
            let dev_name = if read_name_blocks {
                match read_ascii_block_checked(
                    &mut self.i2c,
                    addr,
                    bq40z50::cmd::DEVICE_NAME,
                    &mut dev_buf,
                ) {
                    Ok(v) => Some(v),
                    Err(e) => {
                        if !quiet {
                            log_bms_diag(addr, "probe_dev", e, "block", "ascii");
                        }
                        None
                    }
                }
            } else {
                None
            };
            let name_ok = dev_name
                .map(|name| ascii_contains_case_insensitive(name, b"bq40"))
                .unwrap_or(false)
                || mfg_name
                    .map(|name| ascii_contains_case_insensitive(name, b"texas"))
                    .unwrap_or(false);
            let accept_snapshot_without_name = !read_name_blocks;

            match read_bms_snapshot_strict(
                &mut self.i2c,
                addr,
                self.cfg.bms_strict_validation,
                &mut self.bms_pattern_tracker,
            ) {
                Ok(snapshot) => {
                    if name_ok || !self.cfg.bms_strict_validation || accept_snapshot_without_name {
                        self.bms_weak_pass_votes = 0;
                        if !quiet {
                            defmt::info!(
                                "bms: bq40z50 probe_ok addr=0x{=u8:x} voltage_mv={=u16} soc_pct={=u16} cell1_mv={=u16} cell2_mv={=u16} cell3_mv={=u16} cell4_mv={=u16}",
                                addr,
                                snapshot.voltage_mv,
                                snapshot.soc_pct,
                                snapshot.cell1_mv,
                                snapshot.cell2_mv,
                                snapshot.cell3_mv,
                                snapshot.cell4_mv,
                            );
                        }
                        return Some(addr);
                    }

                    self.bms_weak_pass_votes = self.bms_weak_pass_votes.saturating_add(1);
                    if self.bms_weak_pass_votes >= 2 {
                        if !quiet {
                            defmt::warn!(
                                "bms: bq40z50 weak_pass addr=0x{=u8:x} votes={=u8}",
                                addr,
                                self.bms_weak_pass_votes
                            );
                        }
                        return Some(addr);
                    }

                    if !quiet {
                        defmt::warn!(
                            "bms: bq40z50 probe_miss addr=0x{=u8:x} err=bms_name_weak_pending votes={=u8}",
                            addr,
                            self.bms_weak_pass_votes
                        );
                    }
                }
                Err(e) => {
                    self.bms_weak_pass_votes = 0;
                    if !quiet {
                        log_bms_diag(addr, "probe_snapshot", e, "word", "strict");
                        if bms_verbose_diag(self.cfg.bms_address_mode) {
                            self.maybe_log_bms_word_diag(addr, "probe_snapshot", e);
                        }
                        if matches!(
                            e,
                            bq40z50::BmsDiagError::BadRange
                                | bq40z50::BmsDiagError::StalePattern
                                | bq40z50::BmsDiagError::InconsistentSample
                        ) {
                            defmt::warn!(
                                "bms: bq40z50 probe_degraded addr=0x{=u8:x} err={}",
                                addr,
                                e
                            );
                        } else {
                            defmt::warn!("bms: bq40z50 probe_miss addr=0x{=u8:x} err={}", addr, e);
                        }
                    }
                }
            }
        }

        if matches!(
            self.cfg.bms_address_mode,
            bq40z50::BmsAddressMode::DualProbeDiag
        ) {
            let now = Instant::now();
            if now >= self.bms_diag_scan_next_at {
                self.bms_diag_scan_next_at = now + BMS_DIAG_SCAN_INTERVAL;
                if let Some(addr) = self.probe_bq40z50_diag_scan(quiet) {
                    return Some(addr);
                }
                if !quiet {
                    defmt::warn!(
                        "bms_diag: stage=addr_scan_miss range=0x{=u8:x}-0x{=u8:x}",
                        BMS_DIAG_SCAN_MIN_ADDR,
                        BMS_DIAG_SCAN_MAX_ADDR
                    );
                }
            }
        }

        None
    }

    fn probe_bq40z50_wake_window(&mut self, quiet: bool) -> WakeWindowProbeResult {
        let mode = if self.cfg.bms_mac_probe_only {
            "mac_only"
        } else {
            "strict_word"
        };
        let staged_start = Instant::now();

        for (stage, delay_ms) in BMS_WAKE_WINDOW_PROBE_DELAYS_MS.iter().enumerate() {
            spin_until_elapsed(staged_start, Duration::from_millis(*delay_ms));

            for &addr in self.cfg.bms_address_mode.candidates() {
                let mut touched = false;

                match touch_bms_command(&mut self.i2c, addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE)
                {
                    Ok(()) => {
                        touched = true;
                        if !quiet {
                            defmt::warn!(
                                "bms_diag: addr=0x{=u8:x} stage=wake_touch_rsoc step={=u8} delay_ms={=u64}",
                                addr,
                                stage as u8,
                                *delay_ms
                            );
                        }
                        match read_u16_after_successful_touch(
                            &mut self.i2c,
                            addr,
                            "wake_touch_rsoc_raw",
                            stage as u8,
                            *delay_ms,
                            quiet,
                        ) {
                            Ok(rsoc) => {
                                if rsoc == BMS_ROM_MODE_SIGNATURE {
                                    return WakeWindowProbeResult::Rom(addr);
                                }
                                if rsoc <= 100 {
                                    match touch_bms_command(
                                        &mut self.i2c,
                                        addr,
                                        bq40z50::cmd::TEMPERATURE,
                                    ) {
                                        Ok(()) => {
                                            if !quiet {
                                                defmt::warn!(
                                                    "bms_diag: addr=0x{=u8:x} stage=wake_touch_temp step={=u8} delay_ms={=u64}",
                                                    addr,
                                                    stage as u8,
                                                    *delay_ms
                                                );
                                            }
                                            if let Ok(temp) = read_u16_after_successful_touch(
                                                &mut self.i2c,
                                                addr,
                                                "wake_touch_temp_raw",
                                                stage as u8,
                                                *delay_ms,
                                                quiet,
                                            ) {
                                                if (2_000..=4_300).contains(&temp)
                                                    && confirm_bq40_wake_snapshot(
                                                        &mut self.i2c,
                                                        addr,
                                                        self.cfg.bms_strict_validation,
                                                        &mut self.bms_pattern_tracker,
                                                        "wake_snapshot_confirm_direct",
                                                        stage as u8,
                                                        *delay_ms,
                                                        quiet,
                                                    )
                                                {
                                                    if !quiet {
                                                        defmt::warn!(
                                                            "bms_diag: stage=wake_window_hit addr=0x{=u8:x} probe_mode={} step={=u8} delay_ms={=u64}",
                                                            addr,
                                                            mode,
                                                            stage as u8,
                                                            *delay_ms
                                                        );
                                                    }
                                                    return WakeWindowProbeResult::Working(addr);
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            if !quiet {
                                                log_bms_diag(
                                                    addr,
                                                    "wake_touch_temp",
                                                    e,
                                                    "cmd",
                                                    "wake",
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                            Err(_) => {}
                        }
                    }
                    Err(e) => {
                        if !quiet {
                            log_bms_diag(addr, "wake_touch_rsoc", e, "cmd", "wake");
                        }
                    }
                }

                if !touched {
                    match touch_bms_command(&mut self.i2c, addr, bq40z50::cmd::TEMPERATURE) {
                        Ok(()) => {
                            touched = true;
                            if !quiet {
                                defmt::warn!(
                                    "bms_diag: addr=0x{=u8:x} stage=wake_touch_temp step={=u8} delay_ms={=u64}",
                                    addr,
                                    stage as u8,
                                    *delay_ms
                                );
                            }
                            let _ = read_u16_after_successful_touch(
                                &mut self.i2c,
                                addr,
                                "wake_touch_temp_raw",
                                stage as u8,
                                *delay_ms,
                                quiet,
                            );
                        }
                        Err(e) => {
                            if !quiet {
                                log_bms_diag(addr, "wake_touch_temp", e, "cmd", "wake");
                            }
                        }
                    }
                }

                if !touched {
                    continue;
                }

                match probe_bq40z50_after_wake_touch(
                    &mut self.i2c,
                    addr,
                    self.cfg.bms_strict_validation,
                    &mut self.bms_pattern_tracker,
                    stage as u8,
                    *delay_ms,
                    quiet,
                ) {
                    Ok(WakeWindowProbeResult::Working(addr)) => {
                        if !quiet {
                            defmt::warn!(
                                "bms_diag: stage=wake_window_hit addr=0x{=u8:x} probe_mode={} step={=u8} delay_ms={=u64}",
                                addr,
                                mode,
                                stage as u8,
                                *delay_ms
                            );
                        }
                        return WakeWindowProbeResult::Working(addr);
                    }
                    Ok(WakeWindowProbeResult::Rom(addr)) => {
                        if !quiet {
                            defmt::warn!(
                                "bms_diag: stage=wake_window_rom addr=0x{=u8:x} probe_mode={} step={=u8} delay_ms={=u64}",
                                addr,
                                mode,
                                stage as u8,
                                *delay_ms
                            );
                        }
                        return WakeWindowProbeResult::Rom(addr);
                    }
                    Ok(WakeWindowProbeResult::Miss) => {}
                    Err(e) => {
                        if !quiet {
                            log_bms_diag(addr, "wake_probe_followup", e, "word", "wake");
                        }
                    }
                }
            }

            if !quiet {
                defmt::warn!(
                    "bms_diag: stage=wake_window_miss probe_mode={} step={=u8} delay_ms={=u64}",
                    mode,
                    stage as u8,
                    *delay_ms
                );
            }
        }

        WakeWindowProbeResult::Miss
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
        if self.bms_post_flash_resume_addr.is_some() {
            return;
        }

        if !self.charger_allowed {
            if let Some(next_retry_at) = self.chg_next_retry_at {
                if Instant::now() < next_retry_at {
                    return;
                }
            }
            match bq25792::read_u8(&mut self.i2c, bq25792::reg::CHARGER_CONTROL_0) {
                Ok(ctrl0) => {
                    self.charger_allowed = true;
                    self.chg_next_retry_at = Some(Instant::now());
                    defmt::warn!("charger: bq25792 recovered ctrl0=0x{=u8:x}", ctrl0);
                }
                Err(_) => {
                    self.chg_next_retry_at = Some(Instant::now() + self.cfg.retry_backoff);
                    return;
                }
            }
        }

        // Keep the charger polling independent from the TPS/INA telemetry period.
        const POLL_PERIOD: Duration = Duration::from_secs(1);
        const INT_MIN_INTERVAL: Duration = Duration::from_millis(50);

        let now = Instant::now();
        let mut due = now >= self.chg_next_poll_at;
        if irq.chg_int != 0 && !self.cfg.bms_diag_isolation {
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

        let watchdog = match bq25792::ensure_watchdog_disabled(&mut self.i2c) {
            Ok(state) => state,
            Err(e) => {
                self.chg_ce.set_high();
                self.chg_enabled = false;
                self.chg_next_retry_at = Some(now + self.cfg.retry_backoff);
                defmt::error!(
                    "charger: bq25792 err stage=watchdog_cfg err={}",
                    i2c_error_kind(e)
                );
                return;
            }
        };

        // Keep external ship FET path enabled and force SDRV_CTRL=00 (IDLE).
        let ship_path = match bq25792::ensure_ship_fet_path_enabled(&mut self.i2c) {
            Ok(state) => state,
            Err(e) => {
                self.chg_ce.set_high();
                self.chg_enabled = false;
                self.chg_next_retry_at = Some(now + self.cfg.retry_backoff);
                defmt::error!(
                    "charger: bq25792 err stage=ship_fet_path err={}",
                    i2c_error_kind(e)
                );
                return;
            }
        };

        // Diagnostic-only nudge: if BMS stays missing after charger wakeup, trigger one
        // SDRV system-reset pulse (11 -> 00) to re-open the battery-side path.
        // Do not do this during force-min-charge recovery: that flow must keep the pack rail
        // as undisturbed as possible and the pulse can look like a brief charge drop.
        if self.bms_addr.is_none()
            && self.bms_post_flash_resume_addr.is_none()
            && !self.bms_ship_reset_attempted
            && !self.cfg.force_min_charge
            && matches!(
                self.cfg.bms_address_mode,
                bq40z50::BmsAddressMode::DualProbeDiag
            )
            && self.boot_at.elapsed() >= BMS_SHIP_RESET_DELAY
        {
            self.bms_ship_reset_attempted = true;
            match bq25792::set_sdrv_ctrl_mode(&mut self.i2c, 0b11) {
                Ok(ctrl2_reset) => {
                    spin_delay(BMS_SHIP_RESET_SETTLE);
                    match bq25792::set_sdrv_ctrl_mode(&mut self.i2c, 0b00) {
                        Ok(ctrl2_idle) => {
                            defmt::warn!(
                                "bms_diag: stage=ship_reset_pulse ctrl2_reset=0x{=u8:x} ctrl2_idle=0x{=u8:x}",
                                ctrl2_reset,
                                ctrl2_idle
                            );
                        }
                        Err(e) => {
                            defmt::warn!(
                                "bms_diag: stage=ship_reset_idle_restore err={}",
                                i2c_error_kind(e)
                            );
                        }
                    }
                }
                Err(e) => {
                    defmt::warn!("bms_diag: stage=ship_reset_pulse err={}", i2c_error_kind(e));
                }
            }
        }

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

        let adc_state = match bq25792::ensure_adc_power_path(&mut self.i2c) {
            Ok(state) => state,
            Err(e) => {
                self.chg_ce.set_high();
                self.chg_enabled = false;
                self.chg_next_retry_at = Some(now + self.cfg.retry_backoff);
                defmt::error!(
                    "charger: bq25792 err stage=adc_cfg err={}",
                    i2c_error_kind(e)
                );
                return;
            }
        };

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

        // Bring-up policy:
        // - Keep SYS regulation alive (never force HiZ / non-switching here).
        // - Default path: charge only when runtime conditions are valid and VBAT is present.
        // - Diagnostic override: `force_min_charge` can bypass VBAT presence and apply the
        //   proven wake profile (VREG=16.8V / ICHG=200mA / IINDPM=500mA) for no-pack benches.
        let input_present = vbus_present || ac1_present || ac2_present || pg;
        let can_enable = input_present && !ts_cold && !ts_hot;
        let normal_allow_charge = self.cfg.charge_allowed && can_enable && vbat_present;
        let force_allow_charge = matches!(self.charge_mode, BootChargeMode::MinCharge)
            && self.cfg.force_min_charge
            && can_enable;
        let allow_charge = normal_allow_charge || force_allow_charge;
        let mut applied_ctrl0 = ctrl0;
        let mut applied_vreg_mv: Option<u16> = None;
        let mut applied_ichg_ma: Option<u16> = None;
        let mut applied_iindpm_ma: Option<u16> = None;

        // Always deassert the ILIM_HIZ "brake" so the converter can switch.
        self.chg_ilim_hiz_brk.set_low();

        if allow_charge {
            const FORCE_WAKE_VREG_MV: u16 = 16_800;
            const FORCE_WAKE_ICHG_MA: u16 = 200;
            const FORCE_WAKE_IINDPM_MA: u16 = 500;

            fn decode_voltage_mv(reg: u16) -> u16 {
                (reg & 0x07FF) * 10
            }

            fn decode_cur_ma(reg: u16) -> u16 {
                (reg & 0x01FF) * 10
            }

            if force_allow_charge {
                match bq25792::set_charge_voltage_limit_mv(&mut self.i2c, FORCE_WAKE_VREG_MV) {
                    Ok(v) => applied_vreg_mv = Some(decode_voltage_mv(v)),
                    Err(e) => {
                        self.chg_ce.set_high();
                        self.chg_enabled = false;
                        self.chg_next_retry_at = Some(now + self.cfg.retry_backoff);
                        defmt::error!(
                            "charger: bq25792 err stage=vreg_write err={}",
                            i2c_error_kind(e)
                        );
                        return;
                    }
                }

                match bq25792::set_charge_current_limit_ma(&mut self.i2c, FORCE_WAKE_ICHG_MA) {
                    Ok(v) => applied_ichg_ma = Some(decode_cur_ma(v)),
                    Err(e) => {
                        self.chg_ce.set_high();
                        self.chg_enabled = false;
                        self.chg_next_retry_at = Some(now + self.cfg.retry_backoff);
                        defmt::error!(
                            "charger: bq25792 err stage=ichg_write err={}",
                            i2c_error_kind(e)
                        );
                        return;
                    }
                }

                match bq25792::set_input_current_limit_ma(&mut self.i2c, FORCE_WAKE_IINDPM_MA) {
                    Ok(v) => applied_iindpm_ma = Some(decode_cur_ma(v)),
                    Err(e) => {
                        self.chg_ce.set_high();
                        self.chg_enabled = false;
                        self.chg_next_retry_at = Some(now + self.cfg.retry_backoff);
                        defmt::error!(
                            "charger: bq25792 err stage=iindpm_write err={}",
                            i2c_error_kind(e)
                        );
                        return;
                    }
                }
            }

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
            // Keep charging disabled, but ensure we are not in HIZ (HIZ stops switching).
            let desired_ctrl0 = ctrl0 & !bq25792::ctrl0::EN_HIZ;
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

            self.chg_ce.set_high();
            self.chg_enabled = false;
        }

        let ibus_adc_raw = bq25792::read_u16(&mut self.i2c, bq25792::reg::IBUS_ADC);
        let ibat_adc_raw = bq25792::read_u16(&mut self.i2c, bq25792::reg::IBAT_ADC);
        let vbus_adc_mv = bq25792::read_u16(&mut self.i2c, bq25792::reg::VBUS_ADC);
        let vac1_adc_mv = bq25792::read_u16(&mut self.i2c, bq25792::reg::VAC1_ADC);
        let vac2_adc_mv = bq25792::read_u16(&mut self.i2c, bq25792::reg::VAC2_ADC);
        let vbat_adc_mv = bq25792::read_u16(&mut self.i2c, bq25792::reg::VBAT_ADC);
        let vsys_adc_mv = bq25792::read_u16(&mut self.i2c, bq25792::reg::VSYS_ADC);
        let (ibus_adc_raw, ibus_adc_ma) = match ibus_adc_raw {
            Ok(raw) => (Ok(raw), Ok(i16::from_le_bytes(raw.to_le_bytes()) as i32)),
            Err(e) => {
                let kind = i2c_error_kind(e);
                (Err(kind), Err(kind))
            }
        };
        let (ibat_adc_raw, ibat_adc_ma) = match ibat_adc_raw {
            Ok(raw) => (Ok(raw), Ok(i16::from_le_bytes(raw.to_le_bytes()) as i32)),
            Err(e) => {
                let kind = i2c_error_kind(e);
                (Err(kind), Err(kind))
            }
        };
        let vbus_adc_mv = vbus_adc_mv.map_err(i2c_error_kind);
        let vac1_adc_mv = vac1_adc_mv.map_err(i2c_error_kind);
        let vac2_adc_mv = vac2_adc_mv.map_err(i2c_error_kind);
        let vbat_adc_mv = vbat_adc_mv.map_err(i2c_error_kind);
        let vsys_adc_mv = vsys_adc_mv.map_err(i2c_error_kind);

        defmt::info!(
            "charger: enabled={=bool} charge_allowed={=bool} force_min_charge={=bool} normal_allow_charge={=bool} allow_charge={=bool} input_present={=bool} vbus_present={=bool} ac1_present={=bool} ac2_present={=bool} pg={=bool} vbat_present={=bool} ts_cold={=bool} ts_cool={=bool} ts_warm={=bool} ts_hot={=bool} vreg_mv={=?} ichg_ma={=?} iindpm_ma={=?} wd_cfg_before={=u8} wd_cfg_after={=u8} ctrl1_before=0x{=u8:x} ctrl1_after=0x{=u8:x} sfet_present_before={=bool} sfet_present_after={=bool} ship_ctrl2_before=0x{=u8:x} ship_ctrl2_after=0x{=u8:x} ship_mode_before={=u8} ship_mode_after={=u8} adc_ctrl=0x{=u8:x} adc_dis0=0x{=u8:x} adc_dis1=0x{=u8:x} vbus_adc_mv={=?} vac1_adc_mv={=?} vac2_adc_mv={=?} vbat_adc_mv={=?} vsys_adc_mv={=?} ibus_adc_raw={=?} ibus_adc_ma={=?} ibat_adc_raw={=?} ibat_adc_ma={=?} chg_stat={} vbus_stat={} ico={} treg={=bool} dpdm={=bool} wd={=bool} poorsrc={=bool} vindpm={=bool} iindpm={=bool} st0=0x{=u8:x} st1=0x{=u8:x} st2=0x{=u8:x} st3=0x{=u8:x} st4=0x{=u8:x} fault0=0x{=u8:x} fault1=0x{=u8:x} ctrl0=0x{=u8:x}",
            self.chg_enabled,
            self.cfg.charge_allowed,
            self.cfg.force_min_charge,
            normal_allow_charge,
            allow_charge,
            input_present,
            vbus_present,
            ac1_present,
            ac2_present,
            pg,
            vbat_present,
            ts_cold,
            ts_cool,
            ts_warm,
            ts_hot,
            applied_vreg_mv,
            applied_ichg_ma,
            applied_iindpm_ma,
            watchdog.watchdog_before,
            watchdog.watchdog_after,
            watchdog.ctrl1_before,
            watchdog.ctrl1_after,
            (ship_path.ctrl5_before & bq25792::ctrl5::SFET_PRESENT) != 0,
            (ship_path.ctrl5_after & bq25792::ctrl5::SFET_PRESENT) != 0,
            ship_path.ship.ctrl2_before,
            ship_path.ship.ctrl2_after,
            ship_path.ship.sdrv_ctrl_before,
            ship_path.ship.sdrv_ctrl_after,
            adc_state.ctrl,
            adc_state.disable0,
            adc_state.disable1,
            vbus_adc_mv,
            vac1_adc_mv,
            vac2_adc_mv,
            vbat_adc_mv,
            vsys_adc_mv,
            ibus_adc_raw,
            ibus_adc_ma,
            ibat_adc_raw,
            ibat_adc_ma,
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
        if self.bms_addr.is_none()
            || self.bms_startup_stage != BmsStartupStage::Monitoring
            || irq.bms_btp_int_h != 0
        {
            self.log_bms_signal_line(irq, "charger_poll");
        }

        self.chg_next_retry_at = None;
    }

    fn maybe_poll_bms(&mut self, irq: &IrqSnapshot) -> bool {
        // If the gauge was missing at boot (e.g. pack in SHUTDOWN/SHIP), keep probing so we can
        // latch it once it wakes up.
        if self.bms_addr.is_none() {
            let now = Instant::now();
            if now < self.bms_next_poll_at {
                return false;
            }
            self.bms_next_poll_at = now + BMS_POLL_PERIOD;

            let verbose_missing_probe = (self.cfg.bms_mac_probe_only
                || matches!(
                    self.cfg.bms_address_mode,
                    bq40z50::BmsAddressMode::DualProbeDiag
                ))
                && self
                    .bms_missing_diag_next_at
                    .map_or(false, |next| now >= next);
            if verbose_missing_probe {
                self.bms_missing_diag_next_at = Some(now + BMS_MISSING_VERBOSE_REPROBE_INTERVAL);
                defmt::warn!(
                    "bms_diag: stage=missing_reprobe probe_mode={} addr_mode={} elapsed_ms={=u64}",
                    if self.cfg.bms_mac_probe_only {
                        "mac_only"
                    } else {
                        "strict_word"
                    },
                    self.cfg.bms_address_mode.as_str(),
                    self.boot_at.elapsed().as_millis()
                );
            }

            let probe = if verbose_missing_probe {
                self.probe_bq40z50()
            } else {
                self.probe_bq40z50_quiet()
            };

            if let Some(addr) = probe {
                self.bms_addr = Some(addr);
                self.bms_next_retry_at = Some(now);
                self.bms_next_poll_at = now;
                self.bms_last_int_poll_at = None;
                self.bms_weak_pass_votes = 0;
                self.bms_transport_fail_count = 0;
                self.bms_missing_diag_next_at = None;
                defmt::info!("bms: bq40z50 discovered addr=0x{=u8:x}", addr);
            }
            return true;
        }

        let Some(addr) = self.bms_addr else {
            return false;
        };

        let now = Instant::now();
        let poll_due = now >= self.bms_next_poll_at;
        let retry_due = self.bms_next_retry_at.map_or(false, |t| now >= t);
        let mut due = poll_due || retry_due;
        if irq.bms_btp_int_h != 0 && !self.cfg.bms_diag_isolation {
            let allow = self
                .bms_last_int_poll_at
                .map_or(true, |t| now >= t + BMS_INT_MIN_INTERVAL);
            if allow {
                due = true;
                self.bms_last_int_poll_at = Some(now);
            }
        }
        if !due {
            return false;
        }
        self.bms_next_poll_at = now + BMS_POLL_PERIOD;

        let mut sample = read_bms_snapshot_strict(
            &mut self.i2c,
            addr,
            self.cfg.bms_strict_validation,
            &mut self.bms_pattern_tracker,
        );
        if let Err(first_err) = sample {
            if matches!(
                first_err,
                bq40z50::BmsDiagError::I2cNack | bq40z50::BmsDiagError::InconsistentSample
            ) {
                // Retry transient transport/consistency errors a bit more aggressively before
                // degrading the snapshot.
                let mut last_err = first_err;
                for _ in 0..2 {
                    spin_delay(BMS_WORD_GAP);
                    sample = read_bms_snapshot_strict(
                        &mut self.i2c,
                        addr,
                        self.cfg.bms_strict_validation,
                        &mut self.bms_pattern_tracker,
                    );
                    match sample {
                        Ok(_) => {
                            defmt::warn!(
                                "bms_diag: addr=0x{=u8:x} stage=poll_snapshot_retry_ok first_err={}",
                                addr,
                                first_err
                            );
                            break;
                        }
                        Err(retry_err) => {
                            last_err = retry_err;
                            if !matches!(
                                retry_err,
                                bq40z50::BmsDiagError::I2cNack
                                    | bq40z50::BmsDiagError::InconsistentSample
                            ) {
                                sample = Err(retry_err);
                                break;
                            }
                            sample = Err(retry_err);
                        }
                    }
                }
                if sample.is_err() && last_err != first_err {
                    defmt::warn!(
                        "bms_diag: addr=0x{=u8:x} stage=poll_snapshot_retry_fail first_err={} retry_err={}",
                        addr,
                        first_err,
                        last_err
                    );
                }
            } else {
                sample = Err(first_err);
            }
        }

        match sample {
            Ok(snapshot) => {
                if self
                    .bms_last_working_info_at
                    .map_or(true, |last| now >= last + BMS_WORKING_INFO_PERIOD)
                {
                    defmt::info!(
                        "bms: addr=0x{=u8:x} temp_c_x10={=i32} voltage_mv={=u16} current_ma={=i16} soc_pct={=u16} status=0x{=u16:x} cell1_mv={=u16} cell2_mv={=u16} cell3_mv={=u16} cell4_mv={=u16} err_code={} err_str={} rem_cap_mah={=?} full_cap_mah={=?}",
                        addr,
                        snapshot.temp_c_x10,
                        snapshot.voltage_mv,
                        snapshot.current_ma,
                        snapshot.soc_pct,
                        snapshot.status_raw,
                        snapshot.cell1_mv,
                        snapshot.cell2_mv,
                        snapshot.cell3_mv,
                        snapshot.cell4_mv,
                        snapshot.err_code,
                        bq40z50::decode_error_code(snapshot.err_code),
                        snapshot.remaining_cap_mah,
                        snapshot.full_cap_mah,
                    );
                    self.bms_last_working_info_at = Some(now);
                }
                self.bms_next_retry_at = None;
                self.bms_transport_fail_count = 0;
            }
            Err(e) => {
                log_bms_diag(addr, "poll_snapshot", e, "word", "strict");
                if bms_verbose_diag(self.cfg.bms_address_mode) {
                    self.maybe_log_bms_word_diag(addr, "poll_snapshot", e);
                }
                if matches!(
                    e,
                    bq40z50::BmsDiagError::I2cNack
                        | bq40z50::BmsDiagError::BadBlockLen
                        | bq40z50::BmsDiagError::BadAscii
                ) {
                    self.bms_transport_fail_count = self.bms_transport_fail_count.saturating_add(1);
                    if self.bms_transport_fail_count >= BMS_TRANSPORT_LOSS_THRESHOLD {
                        self.bms_addr = None;
                        self.bms_next_retry_at = None;
                        self.bms_transport_fail_count = 0;
                        self.bms_missing_diag_next_at =
                            Some(now + BMS_MISSING_VERBOSE_REPROBE_INTERVAL);
                        self.bms_last_working_info_at = None;
                        defmt::error!(
                            "bms: bq40z50 transport_lost addr=0x{=u8:x} err={} fail_streak={=u8}",
                            addr,
                            e,
                            BMS_TRANSPORT_LOSS_THRESHOLD
                        );
                    } else {
                        self.bms_next_retry_at = Some(now + BMS_TRANSPORT_RETRY_BACKOFF);
                        defmt::warn!(
                            "bms: bq40z50 transport_retry addr=0x{=u8:x} err={} fail_count={=u8}/{=u8}",
                            addr,
                            e,
                            self.bms_transport_fail_count,
                            BMS_TRANSPORT_LOSS_THRESHOLD
                        );
                    }
                } else {
                    self.bms_transport_fail_count = 0;
                    self.bms_next_retry_at = Some(now + Duration::from_millis(400));
                    defmt::warn!("bms: bq40z50 degraded addr=0x{=u8:x} err={}", addr, e);
                }
            }
        }
        true
    }

    fn maybe_log_bms_word_diag(
        &mut self,
        addr: u8,
        stage: &'static str,
        err: bq40z50::BmsDiagError,
    ) {
        if !matches!(
            err,
            bq40z50::BmsDiagError::I2cNack
                | bq40z50::BmsDiagError::BadRange
                | bq40z50::BmsDiagError::StalePattern
                | bq40z50::BmsDiagError::InconsistentSample
        ) {
            return;
        }

        let now = Instant::now();
        if self
            .bms_last_word_diag_at
            .map_or(false, |last| now < last + BMS_WORD_DIAG_MIN_INTERVAL)
        {
            return;
        }
        self.bms_last_word_diag_at = Some(now);
        self.bms_last_word_diag_addr = Some(addr);
        log_bms_word_diag_set(&mut self.i2c, addr, stage, err);
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
