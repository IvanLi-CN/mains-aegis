pub mod channel;
mod pure;
pub mod tps55288;

use crate::front_panel_scene::{
    is_bq40_activation_needed, BmsActivationState, BmsRecoveryUiAction, BmsResultKind,
    ManualChargePrefs, ManualChargeRuntimeState, ManualChargeSpeed, ManualChargeStopReason,
    ManualChargeTarget, ManualChargeTimerLimit, ManualChargeUiAction, SelfCheckCommState,
    SelfCheckUiSnapshot, UpsMode,
};
use crate::irq::IrqSnapshot;
use esp_firmware::bq25792;
use esp_firmware::bq40z50;
use esp_firmware::fan;
use esp_firmware::ina3221;
use esp_firmware::output_protection;
use esp_firmware::output_retry::{self, TpsConfigRetryDecision};
use esp_firmware::output_state as output_state_logic;
use esp_firmware::tmp112;
use esp_firmware::usb_pd;
use esp_hal::gpio::{Flex, Input};
use esp_hal::ram;
use esp_hal::time::{Duration, Instant};

pub use self::channel::OutputChannel;
pub use self::pure::{AppliedFanState, EnabledOutputs};
pub use esp_firmware::output_state::OutputGateReason;

use self::pure::*;

#[cfg(feature = "bms-dual-probe-diag")]
fn bms_probe_candidates() -> &'static [u8] {
    &bq40z50::I2C_ADDRESS_CANDIDATES
}

#[cfg(not(feature = "bms-dual-probe-diag"))]
fn bms_probe_candidates() -> &'static [u8] {
    &[bq40z50::I2C_ADDRESS_PRIMARY]
}

#[cfg(feature = "bms-dual-probe-diag")]
const BMS_ADDR_LOG: &str = "0x0b/0x16";

#[cfg(not(feature = "bms-dual-probe-diag"))]
const BMS_ADDR_LOG: &str = "0x0b";

const BMS_ACTIVATION_WINDOW: Duration = Duration::from_secs(40);
const BMS_ACTIVATION_FORCE_VREG_MV: u16 = 16_800;
const BMS_ACTIVATION_FORCE_ICHG_MA: u16 = 200;
const BMS_ACTIVATION_FORCE_IINDPM_MA: u16 = 500;
// Historical PASS samples needed a light-weight no-charge observe window before any wake bursts.
// Keep the first 12 s on the old 2 s cadence so activation can discover a naturally settling
// gauge before staged wake touches start perturbing the bus.
const BMS_ACTIVATION_PROBE_WITHOUT_CHARGE_WINDOW: Duration = Duration::from_secs(12);
const BMS_ACTIVATION_PROBE_WITHOUT_CHARGE_PERIOD: Duration = Duration::from_secs(2);
const BMS_ACTIVATION_REPOWER_OFF_WINDOW: Duration = Duration::from_secs(10);
const BMS_ACTIVATION_MIN_CHARGE_SETTLE: Duration = Duration::from_secs(4);
// After repower, keep the first recovery window on the tool's fast cadence so we do not miss
// the brief transition out of the bogus 49 mV pattern.
const BMS_ACTIVATION_MIN_CHARGE_PROBE_WINDOW: Duration = Duration::from_secs(12);
const BMS_ACTIVATION_DIAG_STAGE_DELAYS_MS: [u64; 3] = [0, 800, 1_600];
const BMS_ACTIVATION_DIAG_TOUCH_READ_GAPS_MS: [u64; 3] = [22, 40, 66];
const BMS_ACTIVATION_READ_GAPS_MS: [u64; 3] = [22, 40, 66];
const BMS_ACTIVATION_KEEPALIVE_GAP: Duration = Duration::from_millis(40);
const BMS_ACTIVATION_KEEPALIVE_ROUNDS: usize = 3;
const BMS_ACTIVATION_FOLLOWUP_INITIAL_DELAY: Duration = Duration::from_millis(0);
const BMS_ACTIVATION_FOLLOWUP_PERIOD: Duration = Duration::from_secs(2);
const BMS_ACTIVATION_EXIT_EXERCISE_WINDOW: Duration = Duration::from_secs(6);
const BMS_ACTIVATION_EXIT_EXERCISE_PERIOD: Duration = Duration::from_secs(2);
const BMS_ACTIVATION_CHARGER_POLL_PERIOD: Duration = Duration::from_secs(2);
const BMS_ACTIVATION_AUTO_POLL_RELEASE_DELAY: Duration = Duration::from_secs(34);
const BMS_ACTIVATION_AUTO_DELAY: Duration = Duration::from_secs(30);
const BMS_BOOT_DIAG_SHIP_RESET_DELAY: Duration = Duration::from_secs(20);
const BMS_BOOT_DIAG_SHIP_RESET_SETTLE: Duration = Duration::from_millis(800);
// Self-check recovery must stay user-driven: show the issue first, then wait for explicit
// confirmation from the front panel before attempting any BQ40 wake/recovery sequence.
const BMS_SELF_CHECK_AUTO_RECOVERY_ENABLED: bool = false;
// Temporary boot-diag validation: keep the proven 16.8V/200mA/500mA wake bias active through
// the 30 s settle window so auto-validation matches the tool's working conditions.
const BMS_ACTIVATION_AUTO_BOOT_FORCE_CHARGE: bool = true;
// Boot auto-validation must exercise the real activation state machine so the
// captured logs reflect the same wake-window path as the manual self-check flow.
const BMS_BOOT_DIAG_TOOL_STYLE_PROBE_ONLY: bool = false;
const BMS_BOOT_DIAG_TOOL_STYLE_FORCE_HOLD: Duration = BMS_ACTIVATION_MIN_CHARGE_PROBE_WINDOW;
const BMS_ACTIVATION_ISOLATION_WINDOW: Duration = Duration::from_millis(40);
const BMS_ACTIVATION_MAC_WRITE_SETTLE: Duration = Duration::from_millis(66);
const BMS_ROM_MODE_SIGNATURE: u16 = 0x9002;
const BMS_DEVICE_TYPE_BQ40Z50: u16 = 0x4500;
const BMS_ACTIVATION_WORD_GAP: Duration = Duration::from_millis(2);
const BMS_DETAIL_MAC_REFRESH_PERIOD: Duration = Duration::from_secs(8);
const BMS_DETAIL_MAC_REFRESH_STAGGER: Duration = Duration::from_secs(2);
const BMS_DETAIL_BALANCE_CONFIG_REFRESH_STAGGER: Duration = Duration::from_secs(4);
const BMS_DETAIL_GAUGING_STATUS_REFRESH_STAGGER: Duration = Duration::from_secs(6);
const BMS_DETAIL_LOCK_DIAG_REFRESH_STAGGER: Duration = Duration::from_secs(7);
const BMS_BLOCK_DETAIL_LOG_PERIOD: Duration = Duration::from_secs(10);
const BMS_CONFIG_LOG_RETRY_PERIOD: Duration = Duration::from_secs(10);
const BMS_SUSPICIOUS_VOLTAGE_MV: u16 = 5_911;
const BMS_SUSPICIOUS_CURRENT_MA: i16 = 5_911;
const BMS_SUSPICIOUS_STATUS: u16 = 0x1717;
const BMS_NO_BATTERY_VPACK_MAX_MV: u16 = 2_500;
const BMS_SELF_TEST_DISCHARGE_READY_RETRIES: usize = 6;
const BMS_SELF_TEST_DISCHARGE_READY_RETRY_DELAY: Duration = Duration::from_millis(500);
const BQ40_CURRENT_IDLE_THRESHOLD_MA: i16 = 20;
const OUTPUT_PATH_DIAG_LOG_PERIOD: Duration = Duration::from_secs(5);
const OUTPUT_PATH_NOT_RISING_MAX_CURRENT_MA: i32 = 200;
const OUTPUT_PATH_NOT_RISING_MAX_ABS_VOUT_MV: u16 = 2_000;
const OUTPUT_PATH_EXPECTED_MODE_HYSTERESIS_MV: u16 = 500;
const TPS_CONFIG_MAX_RETRY_ATTEMPTS: u8 = output_retry::DEFAULT_TPS_CONFIG_MAX_RETRY_ATTEMPTS;
const CHARGER_FAULT0_VBUS_OVP: u8 = 1 << 6;
const CHARGER_FAULT0_VBAT_OVP: u8 = 1 << 5;
const CHARGER_FAULT0_IBUS_OCP: u8 = 1 << 4;
const BMS_BALANCING_CONFIGURATION_MAINBOARD: u8 = 0x07;
const BMS_MIN_START_BALANCE_DELTA_MAINBOARD_MV: u8 = 3;
const BMS_RELAX_BALANCE_INTERVAL_MAINBOARD_S: u32 = 18_000;
const BMS_MIN_RSOC_FOR_BALANCING_MAINBOARD_PCT: u8 = 80;

fn bq40_balance_config_matches_mainboard(config: bq40z50::BalanceConfig) -> bool {
    config.raw == BMS_BALANCING_CONFIGURATION_MAINBOARD
        && config.min_start_balance_delta_mv == BMS_MIN_START_BALANCE_DELTA_MAINBOARD_MV
        && config.relax_balance_interval_s == BMS_RELAX_BALANCE_INTERVAL_MAINBOARD_S
        && config.min_rsoc_for_balancing_pct == BMS_MIN_RSOC_FOR_BALANCING_MAINBOARD_PCT
}
const CHARGER_FAULT0_IBAT_OCP: u8 = 1 << 3;
const CHARGER_FAULT0_CONV_OCP: u8 = 1 << 2;
const CHARGER_FAULT0_VAC2_OVP: u8 = 1 << 1;
const CHARGER_FAULT0_VAC1_OVP: u8 = 1 << 0;
const CHARGER_FAULT1_VSYS_SHORT: u8 = 1 << 7;
const CHARGER_FAULT1_VSYS_OVP: u8 = 1 << 6;
const CHARGER_FAULT1_OTG_OVP: u8 = 1 << 5;
const CHARGER_FAULT1_TSHUT: u8 = 1 << 2;
const CHARGE_POLICY_VREG_MV: u16 = 16_800;
const CHARGE_POLICY_NORMAL_ICHG_MA: u16 = 500;
const CHARGE_POLICY_DC_DERATED_ICHG_MA: u16 = 100;
const CHARGE_POLICY_START_RSOC_PCT: u16 = 80;
const CHARGE_POLICY_START_CELL_MIN_MV: u16 = 3_700;
const CHARGE_POLICY_DC_DERATE_ENTER_IBUS_MA: i32 = 3_000;
const CHARGE_POLICY_DC_DERATE_EXIT_IBUS_MA: i32 = 2_700;
const CHARGE_POLICY_DC_DERATE_ENTER_HOLD: Duration = Duration::from_secs(1);
const CHARGE_POLICY_DC_DERATE_EXIT_HOLD: Duration = Duration::from_secs(5);
const CHARGE_POLICY_OUTPUT_POWER_LIMIT_W10: u32 = 50;
const USB_PD_SYSTEM_LOAD_FLOOR_MW: u32 = 2_500;
const CHARGER_INPUT_IBUS_MAX_MA: i16 = 5_000;
const CHARGER_INPUT_VBUS_MAX_MV: u16 = 30_000;
const CHARGER_INPUT_POWER_ANOMALY_W10: u32 = 2_000;
const FAN_RPM_SAMPLE_WINDOW_MS: u64 = 1_200;
const FAN_RPM_MAX_SAMPLE_WINDOW_MS: u64 = 2_000;
const FAN_RPM_MIN_SAMPLE_REVS: u32 = 2;
const VIN_MAINS_PRESENT_THRESHOLD_MV: u16 = 3_000;
const VIN_MAINS_LATCH_FAILURE_LIMIT: u8 = 2;
const BMS_DIAG_BREADCRUMB_LEN: usize = 8;
const BMS_DIAG_BREADCRUMB_VERSION: u8 = 1;
const BMS_SBS_CONFIGURATION_SMB_CELL_TEMP: u8 = 1 << 6;
const MANUAL_CHARGE_TARGET_PACK_MV: u16 = 14_800;
const MANUAL_CHARGE_TARGET_RSOC_PCT: u16 = 80;
const MANUAL_CHARGE_STATUS_TEXT_100MA: &str = "CHG100";
const MANUAL_CHARGE_STATUS_TEXT_500MA: &str = "CHG500";
const MANUAL_CHARGE_STATUS_TEXT_1A: &str = "CHG1A";
const EEPROM_ADDR: u8 = 0x50;
const EEPROM_BLOCK_LEN: usize = 32;
const EEPROM_SUPERBLOCK_OFFSET: u16 = 0x0000;
const EEPROM_RECORD_TABLE_OFFSET: u16 = 0x0020;
const EEPROM_MANUAL_PREFS_OFFSET: u16 = 0x0040;
const EEPROM_LAYOUT_MAGIC: [u8; 4] = *b"AEG1";
const EEPROM_SCHEMA_VERSION: u8 = 1;
const EEPROM_MANUAL_PREFS_RECORD_ID: u8 = 1;
const EEPROM_MANUAL_PREFS_RECORD_VERSION: u8 = 1;
const EEPROM_WRITE_POLL_ATTEMPTS: usize = 32;
const EEPROM_WRITE_POLL_GAP: Duration = Duration::from_millis(1);

#[derive(Clone, Copy)]
struct ManualChargeRuntime {
    active: bool,
    takeover: bool,
    stop_inhibit: bool,
    last_stop_reason: ManualChargeStopReason,
    deadline: Option<Instant>,
}

impl ManualChargeRuntime {
    const fn new() -> Self {
        Self {
            active: false,
            takeover: false,
            stop_inhibit: false,
            last_stop_reason: ManualChargeStopReason::None,
            deadline: None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ManualChargeStorageLoad {
    Ready {
        prefs: ManualChargePrefs,
        prefs_offset: u16,
    },
    NeedsInit(ManualChargePrefs),
    Incompatible(u8),
}

#[derive(Clone, Copy)]
struct StorageSuperblockV1 {
    schema_version: u8,
}

impl StorageSuperblockV1 {
    fn encode(self) -> [u8; EEPROM_BLOCK_LEN] {
        let mut bytes = [0u8; EEPROM_BLOCK_LEN];
        bytes[0] = EEPROM_LAYOUT_MAGIC[0];
        bytes[1] = EEPROM_LAYOUT_MAGIC[1];
        bytes[2] = EEPROM_LAYOUT_MAGIC[2];
        bytes[3] = EEPROM_LAYOUT_MAGIC[3];
        bytes[4] = self.schema_version;
        bytes[5] = 1;
        bytes[31] = storage_crc8(&bytes[..31]);
        bytes
    }

    fn decode(bytes: [u8; EEPROM_BLOCK_LEN]) -> Option<Self> {
        if bytes[0..4] != EEPROM_LAYOUT_MAGIC {
            return None;
        }
        if bytes[31] != storage_crc8(&bytes[..31]) {
            return None;
        }
        Some(Self {
            schema_version: bytes[4],
        })
    }
}

#[derive(Clone, Copy)]
struct StorageRecordTableV1 {
    manual_prefs_offset: u16,
}

impl StorageRecordTableV1 {
    fn encode(self) -> [u8; EEPROM_BLOCK_LEN] {
        let mut bytes = [0u8; EEPROM_BLOCK_LEN];
        bytes[0] = EEPROM_MANUAL_PREFS_RECORD_ID;
        bytes[1] = EEPROM_MANUAL_PREFS_RECORD_VERSION;
        bytes[2] = (self.manual_prefs_offset & 0x00ff) as u8;
        bytes[3] = (self.manual_prefs_offset >> 8) as u8;
        bytes[4] = EEPROM_BLOCK_LEN as u8;
        bytes[31] = storage_crc8(&bytes[..31]);
        bytes
    }

    fn decode(bytes: [u8; EEPROM_BLOCK_LEN]) -> Option<Self> {
        if bytes[31] != storage_crc8(&bytes[..31]) {
            return None;
        }
        if bytes[0] != EEPROM_MANUAL_PREFS_RECORD_ID
            || bytes[1] != EEPROM_MANUAL_PREFS_RECORD_VERSION
            || bytes[4] != EEPROM_BLOCK_LEN as u8
        {
            return None;
        }
        Some(Self {
            manual_prefs_offset: u16::from(bytes[2]) | (u16::from(bytes[3]) << 8),
        })
    }
}

#[derive(Clone, Copy)]
struct ManualChargePrefsRecordV1 {
    prefs: ManualChargePrefs,
}

impl ManualChargePrefsRecordV1 {
    fn encode(self) -> [u8; EEPROM_BLOCK_LEN] {
        let mut bytes = [0u8; EEPROM_BLOCK_LEN];
        bytes[0] = EEPROM_MANUAL_PREFS_RECORD_VERSION;
        bytes[1] = manual_charge_target_encode(self.prefs.target);
        bytes[2] = manual_charge_speed_encode(self.prefs.speed);
        bytes[3] = manual_charge_timer_encode(self.prefs.timer_limit);
        bytes[31] = storage_crc8(&bytes[..31]);
        bytes
    }

    fn decode(bytes: [u8; EEPROM_BLOCK_LEN]) -> Option<Self> {
        if bytes[0] != EEPROM_MANUAL_PREFS_RECORD_VERSION {
            return None;
        }
        if bytes[31] != storage_crc8(&bytes[..31]) {
            return None;
        }
        Some(Self {
            prefs: ManualChargePrefs {
                target: manual_charge_target_decode(bytes[1])?,
                speed: manual_charge_speed_decode(bytes[2])?,
                timer_limit: manual_charge_timer_decode(bytes[3])?,
            },
        })
    }
}

const fn storage_crc8(bytes: &[u8]) -> u8 {
    let mut crc = 0u8;
    let mut idx = 0usize;
    while idx < bytes.len() {
        crc ^= bytes[idx];
        let mut bit = 0u8;
        while bit < 8 {
            crc = if (crc & 0x80) != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
            bit += 1;
        }
        idx += 1;
    }
    crc
}

const fn manual_charge_target_encode(target: ManualChargeTarget) -> u8 {
    match target {
        ManualChargeTarget::Pack3V7 => 0,
        ManualChargeTarget::Rsoc80 => 1,
        ManualChargeTarget::Full100 => 2,
    }
}

const fn manual_charge_target_decode(raw: u8) -> Option<ManualChargeTarget> {
    match raw {
        0 => Some(ManualChargeTarget::Pack3V7),
        1 => Some(ManualChargeTarget::Rsoc80),
        2 => Some(ManualChargeTarget::Full100),
        _ => None,
    }
}

const fn manual_charge_speed_encode(speed: ManualChargeSpeed) -> u8 {
    match speed {
        ManualChargeSpeed::Ma100 => 0,
        ManualChargeSpeed::Ma500 => 1,
        ManualChargeSpeed::Ma1000 => 2,
    }
}

const fn manual_charge_speed_decode(raw: u8) -> Option<ManualChargeSpeed> {
    match raw {
        0 => Some(ManualChargeSpeed::Ma100),
        1 => Some(ManualChargeSpeed::Ma500),
        2 => Some(ManualChargeSpeed::Ma1000),
        _ => None,
    }
}

const fn manual_charge_timer_encode(limit: ManualChargeTimerLimit) -> u8 {
    match limit {
        ManualChargeTimerLimit::H1 => 0,
        ManualChargeTimerLimit::H2 => 1,
        ManualChargeTimerLimit::H6 => 2,
    }
}

const fn manual_charge_timer_decode(raw: u8) -> Option<ManualChargeTimerLimit> {
    match raw {
        0 => Some(ManualChargeTimerLimit::H1),
        1 => Some(ManualChargeTimerLimit::H2),
        2 => Some(ManualChargeTimerLimit::H6),
        _ => None,
    }
}

fn manual_charge_timer_duration(limit: ManualChargeTimerLimit) -> Duration {
    Duration::from_secs(limit.hours() as u64 * 3_600)
}

const fn manual_charge_stop_notice(reason: ManualChargeStopReason) -> &'static str {
    match reason {
        ManualChargeStopReason::TimerExpired => "manual_timer_expired",
        ManualChargeStopReason::PackReached => "manual_target_pack_reached",
        ManualChargeStopReason::RsocReached => "manual_target_rsoc_reached",
        ManualChargeStopReason::FullReached => "manual_target_full_reached",
        ManualChargeStopReason::SafetyBlocked => "manual_safety_blocked",
        ManualChargeStopReason::UserStop | ManualChargeStopReason::None => {
            "manual_user_stop_inhibit"
        }
    }
}

const fn manual_charge_should_hold(reason: ManualChargeStopReason) -> bool {
    matches!(
        reason,
        ManualChargeStopReason::UserStop
            | ManualChargeStopReason::TimerExpired
            | ManualChargeStopReason::PackReached
            | ManualChargeStopReason::RsocReached
            | ManualChargeStopReason::FullReached
    )
}

fn manual_charge_status_text(speed: ManualChargeSpeed, derated: bool) -> &'static str {
    if derated {
        MANUAL_CHARGE_STATUS_TEXT_100MA
    } else {
        match speed {
            ManualChargeSpeed::Ma100 => MANUAL_CHARGE_STATUS_TEXT_100MA,
            ManualChargeSpeed::Ma500 => MANUAL_CHARGE_STATUS_TEXT_500MA,
            ManualChargeSpeed::Ma1000 => MANUAL_CHARGE_STATUS_TEXT_1A,
        }
    }
}

fn manual_charge_remaining_minutes(deadline: Option<Instant>, now: Instant) -> Option<u16> {
    deadline.map(|deadline| {
        if deadline > now {
            let remaining = deadline - now;
            ((remaining.as_secs() + 59) / 60) as u16
        } else {
            0
        }
    })
}

fn read_eeprom_block<I2C>(
    i2c: &mut I2C,
    offset: u16,
) -> Result<[u8; EEPROM_BLOCK_LEN], esp_hal::i2c::master::Error>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let mut buf = [0u8; EEPROM_BLOCK_LEN];
    i2c.write_read(EEPROM_ADDR, &offset.to_be_bytes(), &mut buf)?;
    Ok(buf)
}

fn write_eeprom_block<I2C>(
    i2c: &mut I2C,
    offset: u16,
    data: [u8; EEPROM_BLOCK_LEN],
) -> Result<(), esp_hal::i2c::master::Error>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let mut buf = [0u8; EEPROM_BLOCK_LEN + 2];
    let [hi, lo] = offset.to_be_bytes();
    buf[0] = hi;
    buf[1] = lo;
    buf[2..].copy_from_slice(&data);
    i2c.write(EEPROM_ADDR, &buf)?;
    for _ in 0..EEPROM_WRITE_POLL_ATTEMPTS {
        spin_delay(EEPROM_WRITE_POLL_GAP);
        if i2c.write(EEPROM_ADDR, &offset.to_be_bytes()).is_ok() {
            return Ok(());
        }
    }
    spin_delay(EEPROM_WRITE_POLL_GAP);
    i2c.write(EEPROM_ADDR, &offset.to_be_bytes())
}

fn write_manual_charge_storage_layout<I2C>(
    i2c: &mut I2C,
    prefs: ManualChargePrefs,
) -> Result<(), esp_hal::i2c::master::Error>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    write_eeprom_block(
        i2c,
        EEPROM_SUPERBLOCK_OFFSET,
        StorageSuperblockV1 {
            schema_version: EEPROM_SCHEMA_VERSION,
        }
        .encode(),
    )?;
    write_eeprom_block(
        i2c,
        EEPROM_RECORD_TABLE_OFFSET,
        StorageRecordTableV1 {
            manual_prefs_offset: EEPROM_MANUAL_PREFS_OFFSET,
        }
        .encode(),
    )?;
    write_eeprom_block(
        i2c,
        EEPROM_MANUAL_PREFS_OFFSET,
        ManualChargePrefsRecordV1 { prefs }.encode(),
    )
}

fn write_manual_charge_prefs_record<I2C>(
    i2c: &mut I2C,
    offset: u16,
    prefs: ManualChargePrefs,
) -> Result<(), esp_hal::i2c::master::Error>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    write_eeprom_block(i2c, offset, ManualChargePrefsRecordV1 { prefs }.encode())
}

fn load_manual_charge_prefs_from_eeprom<I2C>(
    i2c: &mut I2C,
) -> Result<ManualChargeStorageLoad, esp_hal::i2c::master::Error>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let superblock =
        match StorageSuperblockV1::decode(read_eeprom_block(i2c, EEPROM_SUPERBLOCK_OFFSET)?) {
            Some(superblock) => superblock,
            None => {
                return Ok(ManualChargeStorageLoad::NeedsInit(
                    ManualChargePrefs::defaults(),
                ))
            }
        };
    if superblock.schema_version > EEPROM_SCHEMA_VERSION {
        return Ok(ManualChargeStorageLoad::Incompatible(
            superblock.schema_version,
        ));
    }
    let record_table =
        match StorageRecordTableV1::decode(read_eeprom_block(i2c, EEPROM_RECORD_TABLE_OFFSET)?) {
            Some(record_table) => record_table,
            None => {
                return Ok(ManualChargeStorageLoad::NeedsInit(
                    ManualChargePrefs::defaults(),
                ))
            }
        };
    let record = read_eeprom_block(i2c, record_table.manual_prefs_offset)
        .map(ManualChargePrefsRecordV1::decode)?;
    if superblock.schema_version == EEPROM_SCHEMA_VERSION {
        Ok(match record {
            Some(record) => ManualChargeStorageLoad::Ready {
                prefs: record.prefs,
                prefs_offset: record_table.manual_prefs_offset,
            },
            None => ManualChargeStorageLoad::NeedsInit(ManualChargePrefs::defaults()),
        })
    } else {
        Ok(ManualChargeStorageLoad::NeedsInit(
            record
                .map(|record| record.prefs)
                .unwrap_or_else(ManualChargePrefs::defaults),
        ))
    }
}

fn load_or_init_manual_charge_prefs<I2C>(i2c: &mut I2C) -> (ManualChargePrefs, u16, bool, bool)
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    match load_manual_charge_prefs_from_eeprom(i2c) {
        Ok(ManualChargeStorageLoad::Ready {
            prefs,
            prefs_offset,
        }) => (prefs, prefs_offset, true, false),
        Ok(ManualChargeStorageLoad::NeedsInit(prefs)) => {
            let layout_ready = if let Err(err) = write_manual_charge_storage_layout(i2c, prefs) {
                defmt::warn!(
                    "eeprom: init manual_charge prefs failed err={}",
                    i2c_error_kind(err)
                );
                false
            } else {
                true
            };
            (prefs, EEPROM_MANUAL_PREFS_OFFSET, layout_ready, false)
        }
        Ok(ManualChargeStorageLoad::Incompatible(found_version)) => {
            defmt::warn!(
                "eeprom: manual_charge newer schema detected found={=u8} current={=u8}; keep prefs runtime-only",
                found_version,
                EEPROM_SCHEMA_VERSION
            );
            (
                ManualChargePrefs::defaults(),
                EEPROM_MANUAL_PREFS_OFFSET,
                false,
                true,
            )
        }
        Err(err) => {
            defmt::warn!(
                "eeprom: read manual_charge prefs failed err={}",
                i2c_error_kind(err)
            );
            (
                ManualChargePrefs::defaults(),
                EEPROM_MANUAL_PREFS_OFFSET,
                false,
                false,
            )
        }
    }
}

#[ram(unstable(rtc_fast, persistent))]
static mut BMS_DIAG_BREADCRUMB_RTC: [u8; BMS_DIAG_BREADCRUMB_LEN] = [0; BMS_DIAG_BREADCRUMB_LEN];

fn bms_diag_breadcrumb_note(code: u8, detail: u8) {
    let buf = [
        b'B',
        b'D',
        b'B',
        b'G',
        BMS_DIAG_BREADCRUMB_VERSION,
        code,
        detail,
        0,
    ];
    unsafe {
        core::ptr::write_volatile(core::ptr::addr_of_mut!(BMS_DIAG_BREADCRUMB_RTC), buf);
    }
}

fn bms_diag_breadcrumb_take() -> Option<(u8, u8)> {
    let buf: [u8; BMS_DIAG_BREADCRUMB_LEN] =
        unsafe { core::ptr::read_volatile(core::ptr::addr_of!(BMS_DIAG_BREADCRUMB_RTC)) };
    if buf[0] != b'B'
        || buf[1] != b'D'
        || buf[2] != b'B'
        || buf[3] != b'G'
        || buf[4] != BMS_DIAG_BREADCRUMB_VERSION
    {
        return None;
    }
    unsafe {
        core::ptr::write_volatile(
            core::ptr::addr_of_mut!(BMS_DIAG_BREADCRUMB_RTC),
            [0; BMS_DIAG_BREADCRUMB_LEN],
        );
    }
    Some((buf[5], buf[6]))
}

fn self_check_comm_state_name(state: SelfCheckCommState) -> &'static str {
    match state {
        SelfCheckCommState::Pending => "pending",
        SelfCheckCommState::Ok => "ok",
        SelfCheckCommState::Warn => "warn",
        SelfCheckCommState::Err => "err",
        SelfCheckCommState::NotAvailable => "na",
    }
}

fn bms_result_name(result: BmsResultKind) -> &'static str {
    match result {
        BmsResultKind::Success => "success",
        BmsResultKind::NoBattery => "no_battery",
        BmsResultKind::RomMode => "rom_mode",
        BmsResultKind::Abnormal => "abnormal",
        BmsResultKind::NotDetected => "not_detected",
    }
}

fn bms_result_option_name(result: Option<BmsResultKind>) -> &'static str {
    result.map_or("none", bms_result_name)
}

fn audio_battery_low_state_name(state: AudioBatteryLowState) -> &'static str {
    match state {
        AudioBatteryLowState::Inactive => "inactive",
        AudioBatteryLowState::WithMains => "with_mains",
        AudioBatteryLowState::NoMains => "no_mains",
        AudioBatteryLowState::Unknown => "unknown",
    }
}

fn bq40_pack_indicates_no_battery(vpack_mv: u16) -> bool {
    vpack_mv < BMS_NO_BATTERY_VPACK_MAX_MV
}

fn bq40_low_pack_runtime_signature_matches(
    vpack_mv_a: u16,
    current_ma_a: i16,
    rsoc_pct_a: u16,
    batt_status_a: u16,
    vpack_mv_b: u16,
    current_ma_b: i16,
    rsoc_pct_b: u16,
    batt_status_b: u16,
) -> bool {
    vpack_mv_a == vpack_mv_b
        && current_ma_a == current_ma_b
        && rsoc_pct_a == rsoc_pct_b
        && batt_status_a == batt_status_b
}

fn bq40_self_test_no_battery_confirmed<I2C>(
    i2c: &mut I2C,
    addr: u8,
    temp_k_x10: u16,
    voltage_mv: u16,
    current_ma: i16,
    soc_pct: u16,
    status_raw: u16,
) -> bool
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    if !bq40_pack_indicates_no_battery(voltage_mv)
        || !(-400..=1250).contains(&bq40z50::temp_c_x10_from_k_x10(temp_k_x10))
        || soc_pct > 100
    {
        return false;
    }

    let confirm = (
        bq40z50::read_u16(i2c, addr, bq40z50::cmd::TEMPERATURE),
        bq40z50::read_u16(i2c, addr, bq40z50::cmd::VOLTAGE),
        bq40z50::read_i16(i2c, addr, bq40z50::cmd::CURRENT),
        bq40z50::read_u16(i2c, addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE),
        bq40z50::read_u16(i2c, addr, bq40z50::cmd::BATTERY_STATUS),
    );

    match confirm {
        (
            Ok(confirm_temp_k_x10),
            Ok(confirm_voltage_mv),
            Ok(confirm_current_ma),
            Ok(confirm_soc_pct),
            Ok(confirm_status_raw),
        ) => {
            (-400..=1250).contains(&bq40z50::temp_c_x10_from_k_x10(confirm_temp_k_x10))
                && confirm_soc_pct <= 100
                && bq40_pack_indicates_no_battery(confirm_voltage_mv)
                && bq40_low_pack_runtime_signature_matches(
                    voltage_mv,
                    current_ma,
                    soc_pct,
                    status_raw,
                    confirm_voltage_mv,
                    confirm_current_ma,
                    confirm_soc_pct,
                    confirm_status_raw,
                )
        }
        _ => false,
    }
}

fn bq40_op_bit(op_status: Option<u32>, mask: u32) -> Option<bool> {
    op_status.map(|raw| (raw & mask) != 0)
}

fn bq40_mac_bit(raw: Option<u32>, mask: u32) -> Option<bool> {
    raw.map(|value| (value & mask) != 0)
}

fn bms_detail_gauging_flag(raw: Option<u32>, mask: u32) -> Option<bool> {
    bq40_mac_bit(raw, mask)
}

fn bq40_df_bit(raw: Option<u16>, mask: u16) -> Option<bool> {
    raw.map(|value| (value & mask) != 0)
}

fn bq40_df_byte_bit(raw: Option<u8>, mask: u8) -> Option<bool> {
    raw.map(|value| (value & mask) != 0)
}

fn bq40_temp_c_x10(raw_k_x10: Option<u16>) -> Option<i16> {
    raw_k_x10.and_then(|value| {
        let temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(value);
        (-400..=1250)
            .contains(&temp_c_x10)
            .then_some(temp_c_x10 as i16)
    })
}

fn bq40_pin16_mode_name(da_configuration: Option<u16>) -> &'static str {
    let Some(da_configuration) = da_configuration else {
        return "unknown";
    };
    let nr = (da_configuration & bq40z50::da_configuration::NR) != 0;
    let emshut_en = (da_configuration & bq40z50::da_configuration::EMSHUT_EN) != 0;
    if !nr {
        "pres"
    } else if emshut_en {
        "shutdown"
    } else {
        "non_removable_no_emshut"
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutputExpectedMode {
    Unknown,
    Buck,
    Boost,
    BuckBoost,
}

impl OutputExpectedMode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Buck => "buck",
            Self::Boost => "boost",
            Self::BuckBoost => "buck_boost",
        }
    }
}

fn expected_output_mode(target_vout_mv: u16, vpack_mv: Option<u16>) -> OutputExpectedMode {
    let Some(vpack_mv) = vpack_mv else {
        return OutputExpectedMode::Unknown;
    };
    if vpack_mv > target_vout_mv.saturating_add(OUTPUT_PATH_EXPECTED_MODE_HYSTERESIS_MV) {
        OutputExpectedMode::Buck
    } else if target_vout_mv > vpack_mv.saturating_add(OUTPUT_PATH_EXPECTED_MODE_HYSTERESIS_MV) {
        OutputExpectedMode::Boost
    } else {
        OutputExpectedMode::BuckBoost
    }
}

fn status_mode_name(status_sample: Option<u8>) -> &'static str {
    match status_sample.map(|status| status & 0b11) {
        Some(0b00) => "boost",
        Some(0b01) => "buck",
        Some(0b10) => "buck_boost",
        Some(_) => "reserved",
        None => "na",
    }
}

fn output_diag_suspected_path(expected_mode: OutputExpectedMode) -> &'static str {
    match expected_mode {
        OutputExpectedMode::Buck => "buck_path_open_or_driver_missing",
        OutputExpectedMode::Boost => "boost_leg_or_output_path_issue",
        OutputExpectedMode::BuckBoost => "shared_power_stage_or_comp_issue",
        OutputExpectedMode::Unknown => "output_path_issue",
    }
}

fn output_diag_check_parts(expected_mode: OutputExpectedMode) -> &'static str {
    match expected_mode {
        OutputExpectedMode::Buck => "DR1H/DR1L,Q9/Q16,L5,BOOT1,SW1",
        OutputExpectedMode::Boost => "SW2,BOOT2,ISP/ISN,VOUT_path",
        OutputExpectedMode::BuckBoost => "COMP,TPS_COMP,SW1/SW2,both power stages",
        OutputExpectedMode::Unknown => "power_stage_and_output_path",
    }
}

fn output_not_rising_anomaly(
    target_vout_mv: u16,
    vbus_mv: Option<u16>,
    current_ma: Option<i32>,
    output_enabled: Option<bool>,
    fault_active: bool,
) -> bool {
    if output_enabled != Some(true) || fault_active {
        return false;
    }
    let Some(vbus_mv) = vbus_mv else {
        return false;
    };
    let Some(current_ma) = current_ma else {
        return false;
    };
    let severe_low = vbus_mv <= OUTPUT_PATH_NOT_RISING_MAX_ABS_VOUT_MV
        || vbus_mv <= target_vout_mv.saturating_div(4);
    severe_low && current_ma.unsigned_abs() <= OUTPUT_PATH_NOT_RISING_MAX_CURRENT_MA as u32
}

fn log_output_path_diag(
    ch: OutputChannel,
    stage: &'static str,
    target_vout_mv: u16,
    vpack_mv: Option<u16>,
    vbus_mv: Option<u16>,
    current_ma: Option<i32>,
    output_enabled: Option<bool>,
    status_sample: Option<u8>,
) {
    let expected_mode = expected_output_mode(target_vout_mv, vpack_mv);
    defmt::warn!(
        "power: output_diag ch={} stage={} anomaly=output_not_rising target_vout_mv={=u16} vpack_mv={=?} vout_mv={=?} current_ma={=?} oe={=?} expected_mode={} status_mode={} status_raw={=?} suspected_path={} check_parts={}",
        ch.name(),
        stage,
        target_vout_mv,
        vpack_mv,
        vbus_mv,
        current_ma,
        output_enabled,
        expected_mode.as_str(),
        status_mode_name(status_sample),
        status_sample,
        output_diag_suspected_path(expected_mode),
        output_diag_check_parts(expected_mode),
    );
}

fn log_tps_config_retry_decision(
    ch: OutputChannel,
    addr: u8,
    stage: &'static str,
    kind: &'static str,
    consecutive_failures: u8,
    decision: TpsConfigRetryDecision,
    retry_backoff: Duration,
) {
    match decision {
        TpsConfigRetryDecision::Retry => defmt::warn!(
            "power: tps addr=0x{=u8:x} ch={} action=retry_config stage={} err={} failures={=u8} retry_in_ms={=u64} max_retry_attempts={=u8}",
            addr,
            ch.name(),
            stage,
            kind,
            consecutive_failures,
            retry_backoff.as_millis() as u64,
            TPS_CONFIG_MAX_RETRY_ATTEMPTS,
        ),
        TpsConfigRetryDecision::Latch => defmt::error!(
            "power: tps addr=0x{=u8:x} ch={} action=latch_config_failure stage={} err={} failures={=u8} max_retry_attempts={=u8}",
            addr,
            ch.name(),
            stage,
            kind,
            consecutive_failures,
            TPS_CONFIG_MAX_RETRY_ATTEMPTS,
        ),
    }
}

fn log_bq40_config_detail<I2C>(
    i2c: &mut I2C,
    addr: u8,
    stage: &'static str,
    op_status: Option<u32>,
) -> bool
where
    I2C: embedded_hal::i2c::I2c,
{
    let sbs_configuration =
        bq40z50::read_data_flash_u8(i2c, addr, bq40z50::data_flash::SBS_CONFIGURATION)
            .ok()
            .flatten();
    let temperature_enable =
        bq40z50::read_data_flash_u8(i2c, addr, bq40z50::data_flash::TEMPERATURE_ENABLE)
            .ok()
            .flatten();
    let temperature_mode =
        bq40z50::read_data_flash_u8(i2c, addr, bq40z50::data_flash::TEMPERATURE_MODE)
            .ok()
            .flatten();
    let da_configuration =
        bq40z50::read_data_flash_u16(i2c, addr, bq40z50::data_flash::DA_CONFIGURATION)
            .ok()
            .flatten();
    let power_config = bq40z50::read_data_flash_u16(i2c, addr, bq40z50::data_flash::POWER_CONFIG)
        .ok()
        .flatten();
    let balance_config = bq40z50::read_balance_config(i2c, addr).ok().flatten();

    defmt::info!(
        "bms_diag_cfg: addr=0x{=u8:x} stage={} op_status={=?} emshut={=?} sec0={=?} btp_int={=?} sbs_configuration={=?} smb_cell_temp={=?} temperature_enable={=?} temperature_mode={=?} da_configuration={=?} power_config={=?} balance_raw={=?} balance_match={=?} balance_cb={=?} balance_cbm={=?} balance_cbr={=?} balance_cbs={=?} balance_min_start_mv={=?} balance_relax_s={=?} balance_min_rsoc={=?} pin16_mode={} nr={=?} ftemp={=?} emshut_en={=?} emshut_pexit_dis={=?} in_system_sleep={=?} sleep_df={=?} emshut_exit_comm={=?} emshut_exit_vpack={=?} auto_ship_en={=?}",
        addr,
        stage,
        op_status,
        bq40_op_bit(op_status, bq40z50::operation_status::EMSHUT),
        bq40_op_bit(op_status, bq40z50::operation_status::SEC0),
        bq40_op_bit(op_status, bq40z50::operation_status::BTP_INT),
        sbs_configuration,
        bq40_df_byte_bit(sbs_configuration, BMS_SBS_CONFIGURATION_SMB_CELL_TEMP),
        temperature_enable,
        temperature_mode,
        da_configuration,
        power_config,
        balance_config.map(|config| config.raw),
        balance_config.map(bq40_balance_config_matches_mainboard),
        balance_config.map(|config| config.cb()),
        balance_config.map(|config| config.cbm()),
        balance_config.map(|config| config.cbr()),
        balance_config.map(|config| config.cbs()),
        balance_config.map(|config| config.min_start_balance_delta_mv),
        balance_config.map(|config| config.relax_balance_interval_s),
        balance_config.map(|config| config.min_rsoc_for_balancing_pct),
        bq40_pin16_mode_name(da_configuration),
        bq40_df_bit(da_configuration, bq40z50::da_configuration::NR),
        bq40_df_bit(da_configuration, bq40z50::da_configuration::FTEMP),
        bq40_df_bit(da_configuration, bq40z50::da_configuration::EMSHUT_EN),
        bq40_df_bit(da_configuration, bq40z50::da_configuration::EMSHUT_PEXIT_DIS),
        bq40_df_bit(da_configuration, bq40z50::da_configuration::IN_SYSTEM_SLEEP),
        bq40_df_bit(da_configuration, bq40z50::da_configuration::SLEEP),
        bq40_df_bit(power_config, bq40z50::power_config::EMSHUT_EXIT_COMM),
        bq40_df_bit(power_config, bq40z50::power_config::EMSHUT_EXIT_VPACK),
        bq40_df_bit(power_config, bq40z50::power_config::AUTO_SHIP_EN),
    );

    da_configuration.is_some() || power_config.is_some() || balance_config.is_some()
}

fn log_bq40_charge_temp_detail<I2C>(i2c: &mut I2C, addr: u8, stage: &'static str)
where
    I2C: embedded_hal::i2c::I2c,
{
    let sbs_configuration =
        bq40z50::read_data_flash_u8(i2c, addr, bq40z50::data_flash::SBS_CONFIGURATION)
            .ok()
            .flatten();
    let temperature_enable =
        bq40z50::read_data_flash_u8(i2c, addr, bq40z50::data_flash::TEMPERATURE_ENABLE)
            .ok()
            .flatten();
    let temperature_mode =
        bq40z50::read_data_flash_u8(i2c, addr, bq40z50::data_flash::TEMPERATURE_MODE)
            .ok()
            .flatten();
    let temp_word_c_x10 =
        bq40_temp_c_x10(bq40z50::read_u16(i2c, addr, bq40z50::cmd::TEMPERATURE).ok());
    let t1_c_x10 = bq40_temp_c_x10(
        bq40z50::read_data_flash_u16(i2c, addr, bq40z50::data_flash::CHARGE_TEMP_T1)
            .ok()
            .flatten(),
    );
    let t2_c_x10 = bq40_temp_c_x10(
        bq40z50::read_data_flash_u16(i2c, addr, bq40z50::data_flash::CHARGE_TEMP_T2)
            .ok()
            .flatten(),
    );
    let t5_c_x10 = bq40_temp_c_x10(
        bq40z50::read_data_flash_u16(i2c, addr, bq40z50::data_flash::CHARGE_TEMP_T5)
            .ok()
            .flatten(),
    );
    let t6_c_x10 = bq40_temp_c_x10(
        bq40z50::read_data_flash_u16(i2c, addr, bq40z50::data_flash::CHARGE_TEMP_T6)
            .ok()
            .flatten(),
    );
    let t3_c_x10 = bq40_temp_c_x10(
        bq40z50::read_data_flash_u16(i2c, addr, bq40z50::data_flash::CHARGE_TEMP_T3)
            .ok()
            .flatten(),
    );
    let t4_c_x10 = bq40_temp_c_x10(
        bq40z50::read_data_flash_u16(i2c, addr, bq40z50::data_flash::CHARGE_TEMP_T4)
            .ok()
            .flatten(),
    );
    let hysteresis_c_x10 = bq40_temp_c_x10(
        bq40z50::read_data_flash_u16(i2c, addr, bq40z50::data_flash::CHARGE_TEMP_HYSTERESIS)
            .ok()
            .flatten()
            .map(|value| 2731u16.saturating_add(value)),
    );
    let da_status2 = bq40z50::read_da_status2(i2c, addr).ok().flatten();

    defmt::info!(
        "bms_diag_temp: addr=0x{=u8:x} stage={} smb_cell_temp={=?} temperature_enable={=?} temperature_mode={=?} temp_word_c_x10={=?} t1_c_x10={=?} t2_c_x10={=?} t5_c_x10={=?} t6_c_x10={=?} t3_c_x10={=?} t4_c_x10={=?} hysteresis_c_x10={=?} ts1_c_x10={=?} ts2_c_x10={=?} ts3_c_x10={=?} ts4_c_x10={=?} cell_c_x10={=?} fet_c_x10={=?} gauging_c_x10={=?}",
        addr,
        stage,
        bq40_df_byte_bit(sbs_configuration, BMS_SBS_CONFIGURATION_SMB_CELL_TEMP),
        temperature_enable,
        temperature_mode,
        temp_word_c_x10,
        t1_c_x10,
        t2_c_x10,
        t5_c_x10,
        t6_c_x10,
        t3_c_x10,
        t4_c_x10,
        hysteresis_c_x10,
        da_status2.and_then(|detail| bq40_temp_c_x10(Some(detail.ts_temp_k_x10[0]))),
        da_status2.and_then(|detail| bq40_temp_c_x10(Some(detail.ts_temp_k_x10[1]))),
        da_status2.and_then(|detail| bq40_temp_c_x10(Some(detail.ts_temp_k_x10[2]))),
        da_status2.and_then(|detail| bq40_temp_c_x10(Some(detail.ts_temp_k_x10[3]))),
        da_status2.and_then(|detail| bq40_temp_c_x10(Some(detail.cell_temp_k_x10))),
        da_status2.and_then(|detail| bq40_temp_c_x10(Some(detail.fet_temp_k_x10))),
        da_status2.and_then(|detail| bq40_temp_c_x10(Some(detail.gauging_temp_k_x10))),
    );
}

fn bq40_block_probe_byte(probe: &bq40z50::BlockReadProbe, index: usize) -> u8 {
    if index < probe.prefix_len as usize {
        probe.prefix[index]
    } else {
        0
    }
}

fn bq40_selected_probe(trace: &bq40z50::BlockReadTrace) -> Option<&bq40z50::BlockReadProbe> {
    match trace.selected_source {
        Some(bq40z50::BlockReadSource::Pec) => Some(&trace.pec),
        Some(bq40z50::BlockReadSource::Plain) => Some(&trace.plain),
        None => None,
    }
}

fn bq40_charge_trace_failure_reason(
    charging: Option<&bq40z50::ChargingStatusTrace>,
) -> &'static str {
    let Some(charging) = charging else {
        return "read_failed";
    };
    if charging.value.is_some() {
        return "none";
    }
    if charging.block.raw.is_some() {
        return "value_too_short";
    }
    if !matches!(
        charging.block.plain.status,
        bq40z50::BlockReadProbeStatus::NotAttempted
    ) {
        return bq40z50::block_read_probe_status_name(charging.block.plain.status);
    }
    bq40z50::block_read_probe_status_name(charging.block.pec.status)
}

fn log_bq40_charging_status_trace(
    addr: u8,
    stage: &'static str,
    charging: Option<&bq40z50::ChargingStatusTrace>,
) {
    let selected_source = charging
        .map(|charging| bq40z50::block_read_source_name(charging.block.selected_source))
        .unwrap_or("none");
    let selected_probe = charging.and_then(|charging| bq40_selected_probe(&charging.block));
    let selected_declared_len = selected_probe.map(|probe| probe.declared_len).unwrap_or(0);
    let selected_payload_len = selected_probe.map(|probe| probe.payload_len).unwrap_or(0);
    let selected_b0 = selected_probe
        .map(|probe| bq40_block_probe_byte(probe, 0))
        .unwrap_or(0);
    let selected_b1 = selected_probe
        .map(|probe| bq40_block_probe_byte(probe, 1))
        .unwrap_or(0);
    let selected_b2 = selected_probe
        .map(|probe| bq40_block_probe_byte(probe, 2))
        .unwrap_or(0);
    let selected_b3 = selected_probe
        .map(|probe| bq40_block_probe_byte(probe, 3))
        .unwrap_or(0);
    let pec_probe = charging.map(|charging| charging.block.pec);
    let plain_probe = charging.map(|charging| charging.block.plain);
    let raw = charging.and_then(|charging| charging.value);

    defmt::info!(
        "bms_diag_charge: addr=0x{=u8:x} stage={} source={} selected_declared_len={=u8} selected_payload_len={=u8} selected_b0=0x{=u8:x} selected_b1=0x{=u8:x} selected_b2=0x{=u8:x} selected_b3=0x{=u8:x} failure={} raw={=?} pv={=?} lv={=?} mv={=?} hv={=?} inhibit={=?} suspend={=?} maintenance={=?} vct={=?} ccr={=?} cvr={=?} ccc={=?} nct={=?} pec_status={} pec_declared_len={=u8} pec_payload_len={=u8} pec_b0=0x{=u8:x} pec_b1=0x{=u8:x} pec_b2=0x{=u8:x} pec_b3=0x{=u8:x} plain_status={} plain_declared_len={=u8} plain_payload_len={=u8} plain_b0=0x{=u8:x} plain_b1=0x{=u8:x} plain_b2=0x{=u8:x} plain_b3=0x{=u8:x}",
        addr,
        stage,
        selected_source,
        selected_declared_len,
        selected_payload_len,
        selected_b0,
        selected_b1,
        selected_b2,
        selected_b3,
        bq40_charge_trace_failure_reason(charging),
        raw,
        bq40_mac_bit(raw, bq40z50::charging_status::PV),
        bq40_mac_bit(raw, bq40z50::charging_status::LV),
        bq40_mac_bit(raw, bq40z50::charging_status::MV),
        bq40_mac_bit(raw, bq40z50::charging_status::HV),
        bq40_mac_bit(raw, bq40z50::charging_status::IN),
        bq40_mac_bit(raw, bq40z50::charging_status::SU),
        bq40_mac_bit(raw, bq40z50::charging_status::MCHG),
        bq40_mac_bit(raw, bq40z50::charging_status::VCT),
        bq40_mac_bit(raw, bq40z50::charging_status::CCR),
        bq40_mac_bit(raw, bq40z50::charging_status::CVR),
        bq40_mac_bit(raw, bq40z50::charging_status::CCC),
        bq40_mac_bit(raw, bq40z50::charging_status::NCT),
        pec_probe
            .map(|probe| bq40z50::block_read_probe_status_name(probe.status))
            .unwrap_or("read_failed"),
        pec_probe.map(|probe| probe.declared_len).unwrap_or(0),
        pec_probe.map(|probe| probe.payload_len).unwrap_or(0),
        pec_probe.map(|probe| bq40_block_probe_byte(&probe, 0)).unwrap_or(0),
        pec_probe.map(|probe| bq40_block_probe_byte(&probe, 1)).unwrap_or(0),
        pec_probe.map(|probe| bq40_block_probe_byte(&probe, 2)).unwrap_or(0),
        pec_probe.map(|probe| bq40_block_probe_byte(&probe, 3)).unwrap_or(0),
        plain_probe
            .map(|probe| bq40z50::block_read_probe_status_name(probe.status))
            .unwrap_or("read_failed"),
        plain_probe.map(|probe| probe.declared_len).unwrap_or(0),
        plain_probe.map(|probe| probe.payload_len).unwrap_or(0),
        plain_probe.map(|probe| bq40_block_probe_byte(&probe, 0)).unwrap_or(0),
        plain_probe.map(|probe| bq40_block_probe_byte(&probe, 1)).unwrap_or(0),
        plain_probe.map(|probe| bq40_block_probe_byte(&probe, 2)).unwrap_or(0),
        plain_probe.map(|probe| bq40_block_probe_byte(&probe, 3)).unwrap_or(0),
    );
}

fn read_bq40_lock_diag_snapshot<I2C>(i2c: &mut I2C, addr: u8) -> Bq40LockDiagSnapshot
where
    I2C: embedded_hal::i2c::I2c,
{
    Bq40LockDiagSnapshot {
        charging: bq40z50::read_charging_status_trace(i2c, addr).ok(),
        safety_status: bq40z50::read_mac_u32(i2c, addr, bq40z50::mac::SAFETY_STATUS)
            .ok()
            .flatten(),
        gauging_status: bq40z50::read_gauging_status(i2c, addr).ok().flatten(),
        op_status: bq40z50::read_operation_status(i2c, addr).ok().flatten(),
        update_status: bq40z50::read_data_flash_u8(i2c, addr, bq40z50::data_flash::UPDATE_STATUS)
            .ok()
            .flatten(),
        current_at_eoc_ma: bq40z50::read_data_flash_u16(
            i2c,
            addr,
            bq40z50::data_flash::CURRENT_AT_EOC,
        )
        .ok()
        .flatten(),
        no_valid_charge_term: bq40z50::read_data_flash_u16(
            i2c,
            addr,
            bq40z50::data_flash::NO_VALID_CHARGE_TERM,
        )
        .ok()
        .flatten(),
        last_valid_charge_term: bq40z50::read_data_flash_u16(
            i2c,
            addr,
            bq40z50::data_flash::LAST_VALID_CHARGE_TERM,
        )
        .ok()
        .flatten(),
        no_of_qmax_updates: bq40z50::read_data_flash_u16(
            i2c,
            addr,
            bq40z50::data_flash::NO_OF_QMAX_UPDATES,
        )
        .ok()
        .flatten(),
        no_of_ra_updates: bq40z50::read_data_flash_u16(
            i2c,
            addr,
            bq40z50::data_flash::NO_OF_RA_UPDATES,
        )
        .ok()
        .flatten(),
    }
}

fn log_bq40_lock_diag_snapshot(addr: u8, stage: &'static str, diag: &Bq40LockDiagSnapshot) {
    log_bq40_charging_status_trace(addr, stage, diag.charging.as_ref());
    defmt::info!(
        "bms_diag_state: addr=0x{=u8:x} stage={} safety_status={=?} oc={=?} gauging_status={=?} qen={=?} vok={=?} rest={=?} fc={=?} fd={=?} op_status={=?} xchg={=?} chg_fet={=?} dsg_fet={=?} pchg_fet={=?} update_status={=?} current_at_eoc_ma={=?} no_valid_charge_term={=?} last_valid_charge_term={=?} no_of_qmax_updates={=?} no_of_ra_updates={=?}",
        addr,
        stage,
        diag.safety_status,
        bq40_mac_bit(diag.safety_status, bq40z50::safety_status::OC),
        diag.gauging_status,
        bq40_mac_bit(diag.gauging_status, bq40z50::gauging_status::QEN),
        bq40_mac_bit(diag.gauging_status, bq40z50::gauging_status::VOK),
        bq40_mac_bit(diag.gauging_status, bq40z50::gauging_status::REST),
        bq40_mac_bit(diag.gauging_status, bq40z50::gauging_status::FC),
        bq40_mac_bit(diag.gauging_status, bq40z50::gauging_status::FD),
        diag.op_status,
        bq40_op_bit(diag.op_status, bq40z50::operation_status::XCHG),
        bq40_op_bit(diag.op_status, bq40z50::operation_status::CHG),
        bq40_op_bit(diag.op_status, bq40z50::operation_status::DSG),
        bq40_op_bit(diag.op_status, bq40z50::operation_status::PCHG),
        diag.update_status,
        diag.current_at_eoc_ma,
        diag.no_valid_charge_term,
        diag.last_valid_charge_term,
        diag.no_of_qmax_updates,
        diag.no_of_ra_updates,
    );
}

fn log_bq40_block_detail<I2C>(i2c: &mut I2C, addr: u8, stage: &'static str, op_status: Option<u32>)
where
    I2C: embedded_hal::i2c::I2c,
{
    let _ = log_bq40_config_detail(i2c, addr, stage, op_status);
    let safety_status = bq40z50::read_mac_u32(i2c, addr, bq40z50::mac::SAFETY_STATUS)
        .ok()
        .flatten();
    let pf_status = bq40z50::read_mac_u32(i2c, addr, bq40z50::mac::PF_STATUS)
        .ok()
        .flatten();
    let manufacturing_status = bq40z50::read_mac_u32(i2c, addr, bq40z50::mac::MANUFACTURING_STATUS)
        .ok()
        .flatten();

    defmt::info!(
        "bms_diag_block: addr=0x{=u8:x} stage={} safety_status={=?} pf_status={=?} manufacturing_status={=?} fet_en={=?} gauge_en={=?} pf_en={=?} lf_en={=?} pchg_en={=?} chg_en={=?} dsg_en={=?} cuv={=?} cuvc={=?} ocd1={=?} ocd2={=?} ascd={=?} ascdl={=?} aold={=?} aoldl={=?} cov={=?} occ1={=?} occ2={=?} ascc={=?} asccl={=?} otc={=?} otd={=?} suv={=?} sov={=?} socd={=?} socc={=?} dfetf={=?} cfetf={=?} afec={=?} afer={=?}",
        addr,
        stage,
        safety_status,
        pf_status,
        manufacturing_status,
        bq40_mac_bit(manufacturing_status, bq40z50::manufacturing_status::FET_EN),
        bq40_mac_bit(manufacturing_status, bq40z50::manufacturing_status::GAUGE_EN),
        bq40_mac_bit(manufacturing_status, bq40z50::manufacturing_status::PF_EN),
        bq40_mac_bit(manufacturing_status, bq40z50::manufacturing_status::LF_EN),
        bq40_mac_bit(manufacturing_status, bq40z50::manufacturing_status::PCHG_EN),
        bq40_mac_bit(manufacturing_status, bq40z50::manufacturing_status::CHG_EN),
        bq40_mac_bit(manufacturing_status, bq40z50::manufacturing_status::DSG_EN),
        bq40_mac_bit(safety_status, bq40z50::safety_status::CUV),
        bq40_mac_bit(safety_status, bq40z50::safety_status::CUVC),
        bq40_mac_bit(safety_status, bq40z50::safety_status::OCD1),
        bq40_mac_bit(safety_status, bq40z50::safety_status::OCD2),
        bq40_mac_bit(safety_status, bq40z50::safety_status::ASCD),
        bq40_mac_bit(safety_status, bq40z50::safety_status::ASCDL),
        bq40_mac_bit(safety_status, bq40z50::safety_status::AOLD),
        bq40_mac_bit(safety_status, bq40z50::safety_status::AOLDL),
        bq40_mac_bit(safety_status, bq40z50::safety_status::COV),
        bq40_mac_bit(safety_status, bq40z50::safety_status::OCC1),
        bq40_mac_bit(safety_status, bq40z50::safety_status::OCC2),
        bq40_mac_bit(safety_status, bq40z50::safety_status::ASCC),
        bq40_mac_bit(safety_status, bq40z50::safety_status::ASCCL),
        bq40_mac_bit(safety_status, bq40z50::safety_status::OTC),
        bq40_mac_bit(safety_status, bq40z50::safety_status::OTD),
        bq40_mac_bit(pf_status, bq40z50::pf_status::SUV),
        bq40_mac_bit(pf_status, bq40z50::pf_status::SOV),
        bq40_mac_bit(pf_status, bq40z50::pf_status::SOCD),
        bq40_mac_bit(pf_status, bq40z50::pf_status::SOCC),
        bq40_mac_bit(pf_status, bq40z50::pf_status::DFETF),
        bq40_mac_bit(pf_status, bq40z50::pf_status::CFETF),
        bq40_mac_bit(pf_status, bq40z50::pf_status::AFEC),
        bq40_mac_bit(pf_status, bq40z50::pf_status::AFER),
    );

    let diag = read_bq40_lock_diag_snapshot(i2c, addr);
    log_bq40_lock_diag_snapshot(addr, stage, &diag);

    log_bq40_charge_temp_detail(i2c, addr, stage);
}

fn bq40_decode_charge_path(op_status: Option<u32>) -> (Option<bool>, &'static str) {
    let Some(raw) = op_status else {
        return (None, "op_status_unavailable");
    };

    let xchg = (raw & bq40z50::operation_status::XCHG) != 0;
    let chg_fet = (raw & bq40z50::operation_status::CHG) != 0;

    if xchg {
        (Some(false), "xchg_blocked")
    } else if chg_fet {
        (Some(true), "ready")
    } else {
        (Some(false), "chg_fet_off")
    }
}

fn bq40_decode_discharge_path(op_status: Option<u32>) -> (Option<bool>, &'static str) {
    let Some(raw) = op_status else {
        return (None, "op_status_unavailable");
    };

    let xdsg = (raw & bq40z50::operation_status::XDSG) != 0;
    let dsg_fet = (raw & bq40z50::operation_status::DSG) != 0;

    if xdsg {
        (Some(false), "xdsg_blocked")
    } else if dsg_fet {
        (Some(true), "ready")
    } else {
        (Some(false), "dsg_fet_off")
    }
}

fn bq40_decode_current_flow(current_ma: i16) -> &'static str {
    if current_ma > BQ40_CURRENT_IDLE_THRESHOLD_MA {
        "charging"
    } else if current_ma < -BQ40_CURRENT_IDLE_THRESHOLD_MA {
        "discharging"
    } else {
        "idle"
    }
}

fn audio_charge_phase_from_chg_stat(code: u8) -> AudioChargePhase {
    match code & 0x07 {
        0 => AudioChargePhase::NotCharging,
        1 | 2 | 3 | 4 | 6 => AudioChargePhase::Charging,
        _ if bq25792::is_charge_termination_done(code) => AudioChargePhase::Completed,
        _ => AudioChargePhase::Unknown,
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
    Value(i32),
    Err(&'static str),
}

impl defmt::Format for TelemetryTempC {
    fn format(&self, fmt: defmt::Formatter) {
        match self {
            TelemetryTempC::Value(temp_c_x16) => {
                let neg = *temp_c_x16 < 0;
                let abs = temp_c_x16.wrapping_abs() as u32;
                let int = abs / 16;
                let frac_4 = (abs % 16) * 625;

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
        Error::AcknowledgeCheckFailed(_) => "i2c_nack",
        Error::ArbitrationLost => "i2c_arbitration",
        _ => "i2c",
    }
}

fn spin_delay(wait: Duration) {
    let start = Instant::now();
    while start.elapsed() < wait {}
}

pub(super) fn tps_error_kind(err: ::tps55288::Error<esp_hal::i2c::master::Error>) -> &'static str {
    match err {
        ::tps55288::Error::I2c(e) => i2c_error_kind(e),
        ::tps55288::Error::OutOfRange => "out_of_range",
        ::tps55288::Error::InvalidConfig => "invalid_config",
    }
}

pub(super) fn ina_error_kind(err: ina3221::Error<esp_hal::i2c::master::Error>) -> &'static str {
    match err {
        ina3221::Error::I2c(e) => i2c_error_kind(e),
        ina3221::Error::OutOfRange => "out_of_range",
        ina3221::Error::InvalidConfig => "invalid_config",
    }
}

#[derive(Clone, Copy)]
pub struct BootSelfTestResult {
    pub ina_detected: bool,
    pub detected_tmp_outputs: EnabledOutputs,
    pub detected_tps_outputs: EnabledOutputs,
    pub requested_outputs: EnabledOutputs,
    pub active_outputs: EnabledOutputs,
    pub recoverable_outputs: EnabledOutputs,
    pub output_gate_reason: OutputGateReason,
    pub charger_probe_ok: bool,
    pub charger_enabled: bool,
    pub initial_audio_charge_phase: AudioChargePhase,
    pub initial_bms_protection_active: bool,
    pub initial_tps_a_over_voltage: bool,
    pub initial_tps_b_over_voltage: bool,
    pub initial_tps_a_over_current: bool,
    pub initial_tps_b_over_current: bool,
    pub bms_addr: Option<u8>,
    pub self_check_snapshot: SelfCheckUiSnapshot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelfCheckStage {
    Begin,
    Sensors,
    Screen,
    Bms,
    Charger,
    Tps,
    Done,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct OutputRuntimeState {
    requested_outputs: EnabledOutputs,
    active_outputs: EnabledOutputs,
    recoverable_outputs: EnabledOutputs,
    gate_reason: OutputGateReason,
}

impl OutputRuntimeState {
    const fn new(
        requested_outputs: EnabledOutputs,
        active_outputs: EnabledOutputs,
        recoverable_outputs: EnabledOutputs,
        gate_reason: OutputGateReason,
    ) -> Self {
        Self {
            requested_outputs,
            active_outputs,
            recoverable_outputs,
            gate_reason,
        }
    }
}

#[derive(Clone, Copy, Default)]
struct TpsFaultLatch {
    last_status: Option<u8>,
    scp_latched: bool,
    ocp_latched: bool,
    ovp_latched: bool,
    config_failure_latched: bool,
    config_retry_failures: u8,
}

impl TpsFaultLatch {
    fn record_status(&mut self, status: u8) {
        let bits = ::tps55288::registers::StatusBits::from_bits_truncate(status);
        self.last_status = Some(status);
        self.scp_latched |= bits.contains(::tps55288::registers::StatusBits::SCP);
        self.ocp_latched |= bits.contains(::tps55288::registers::StatusBits::OCP);
        self.ovp_latched |= bits.contains(::tps55288::registers::StatusBits::OVP);
    }

    const fn fault_active(self) -> bool {
        self.scp_latched || self.ocp_latched || self.ovp_latched
    }

    const fn over_current(self) -> bool {
        self.scp_latched || self.ocp_latched
    }

    const fn over_voltage(self) -> bool {
        self.ovp_latched
    }

    fn record_config_failure(&mut self, _stage: &'static str, _kind: &'static str) -> u8 {
        self.config_retry_failures = self.config_retry_failures.saturating_add(1);
        self.config_retry_failures
    }

    fn latch_config_failure(&mut self) {
        self.config_failure_latched = true;
    }

    const fn config_failure_active(self) -> bool {
        self.config_failure_latched
    }

    fn clear(&mut self) {
        *self = Self::default();
    }
}

fn output_state_gate_transition(
    state: OutputRuntimeState,
    gate_reason: OutputGateReason,
) -> OutputRuntimeState {
    output_state_from_logic(output_state_logic::output_state_gate_transition(
        output_state_to_logic(state),
        gate_reason,
    ))
}

fn output_restore_pending_from_state(
    state: OutputRuntimeState,
    mains_present: Option<bool>,
) -> bool {
    output_state_logic::output_restore_pending_from_state(
        output_state_to_logic(state),
        mains_present,
    )
}

#[derive(Clone, Copy)]
pub struct PanelProbeResult {
    pub tca6408_present: bool,
    pub fusb302_present: bool,
}

impl PanelProbeResult {
    pub const fn screen_present(self) -> bool {
        // The front-panel screen path depends on the panel IO expander.
        self.tca6408_present
    }
}

fn mains_present_from_vin(vin_vbus_mv: Option<u16>) -> Option<bool> {
    vin_vbus_mv.map(|mv| mv >= VIN_MAINS_PRESENT_THRESHOLD_MV)
}

fn stable_mains_present(
    vin_mains_present: Option<bool>,
    vin_vbus_mv: Option<u16>,
    charger_present: Option<bool>,
) -> Option<bool> {
    stable_mains_state(vin_mains_present, vin_vbus_mv, charger_present).present
}

fn discharge_authorization_input_ready(
    mains_present: Option<bool>,
    charger_present: Option<bool>,
) -> bool {
    charger_present == Some(true) || mains_present == Some(true)
}

fn bq40_ui_issue_detail(low_pack: bool, primary_reason: &'static str) -> Option<&'static str> {
    if low_pack {
        Some("no_battery")
    } else if primary_reason == "nominal" {
        None
    } else {
        Some(primary_reason)
    }
}

fn bq40_last_result_blocks_auto_recovery(result: Option<BmsResultKind>) -> bool {
    matches!(
        result,
        Some(
            BmsResultKind::NoBattery
                | BmsResultKind::RomMode
                | BmsResultKind::Abnormal
                | BmsResultKind::NotDetected
        )
    )
}

fn bq40_recovery_action_for_snapshot(
    snapshot: &SelfCheckUiSnapshot,
    requested_outputs: output_state_logic::EnabledOutputs,
    gate_reason: OutputGateReason,
    bms_addr: Option<u8>,
    charger_allowed: bool,
    therm_kill_asserted: bool,
) -> Option<BmsRecoveryUiAction> {
    if is_bq40_activation_needed(snapshot) {
        Some(BmsRecoveryUiAction::Activation)
    } else if requested_outputs != output_state_logic::EnabledOutputs::None
        && gate_reason == OutputGateReason::BmsNotReady
        && bms_addr.is_some()
        && snapshot.bq40z50 != SelfCheckCommState::Err
        && snapshot.bq40z50_no_battery != Some(true)
        && snapshot.bq40z50_rca_alarm != Some(true)
        && snapshot.bq40z50_discharge_ready == Some(false)
        && discharge_authorization_input_ready(
            stable_mains_present(
                snapshot.vin_mains_present,
                snapshot.vin_vbus_mv,
                snapshot.fusb302_vbus_present,
            ),
            snapshot.fusb302_vbus_present,
        )
        && charger_allowed
        && snapshot.bq25792 != SelfCheckCommState::Err
        && !therm_kill_asserted
    {
        Some(BmsRecoveryUiAction::DischargeAuthorization)
    } else {
        None
    }
}

fn record_vin_sample_failure(vin_mains_present: &mut Option<bool>, missing_streak: &mut u8) {
    *missing_streak = missing_streak.saturating_add(1);
    if *missing_streak >= VIN_MAINS_LATCH_FAILURE_LIMIT {
        *vin_mains_present = None;
    }
}

fn mark_vin_telemetry_unavailable(
    telemetry_include_vin_ch3: bool,
    vin_vbus_mv: &mut Option<u16>,
    vin_iin_ma: &mut Option<i32>,
    vin_mains_present: &mut Option<bool>,
    missing_streak: &mut u8,
) {
    *vin_vbus_mv = None;
    *vin_iin_ma = None;
    if telemetry_include_vin_ch3 {
        record_vin_sample_failure(vin_mains_present, missing_streak);
    } else {
        *vin_mains_present = None;
        *missing_streak = 0;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AudioMainsSource {
    #[default]
    Unknown,
    Vin,
    ChargerFallback,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct StableMainsState {
    present: Option<bool>,
    source: AudioMainsSource,
}

fn stable_mains_state(
    vin_mains_present: Option<bool>,
    vin_vbus_mv: Option<u16>,
    charger_present: Option<bool>,
) -> StableMainsState {
    if let Some(present) = mains_present_from_vin(vin_vbus_mv) {
        return StableMainsState {
            present: Some(present),
            source: AudioMainsSource::Vin,
        };
    }
    if let Some(present) = vin_mains_present {
        return StableMainsState {
            present: Some(present),
            source: AudioMainsSource::Vin,
        };
    }
    if let Some(present) = charger_present {
        return StableMainsState {
            present: Some(present),
            source: AudioMainsSource::ChargerFallback,
        };
    }
    StableMainsState::default()
}

fn mains_present_edge(prev: StableMainsState, next: StableMainsState) -> Option<bool> {
    if prev.present.is_some() && next.present.is_some() && prev.present != next.present {
        next.present
    } else {
        None
    }
}

fn ups_mode_from_mains(mains_present: Option<bool>, has_output: bool) -> UpsMode {
    match mains_present {
        Some(true) => {
            if has_output {
                UpsMode::Supplement
            } else {
                UpsMode::Standby
            }
        }
        Some(false) => {
            let _ = has_output;
            UpsMode::Backup
        }
        None => {
            // Unknown VBUS is treated conservatively: avoid assuming mains-present.
            if has_output {
                UpsMode::Backup
            } else {
                UpsMode::Standby
            }
        }
    }
}

pub fn log_i2c2_presence<I2C>(i2c: &mut I2C) -> PanelProbeResult
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let mut tca6408_present = false;
    let mut fusb302_present = false;

    defmt::info!("self_test: i2c2 scan begin");
    for (addr, name) in [(0x21u8, "tca6408a"), (0x22u8, "fusb302b")] {
        let mut buf = [0u8; 1];
        match i2c.write_read(addr, &[0x00], &mut buf) {
            Ok(()) => {
                if addr == 0x21 {
                    tca6408_present = true;
                }
                if addr == 0x22 {
                    fusb302_present = true;
                }
                defmt::info!(
                    "self_test: i2c2 ok addr=0x{=u8:x} dev={} reg0=0x{=u8:x}",
                    addr,
                    name,
                    buf[0]
                );
            }
            Err(e) => defmt::warn!(
                "self_test: i2c2 miss addr=0x{=u8:x} dev={} err={}",
                addr,
                name,
                i2c_error_kind(e)
            ),
        }
    }

    defmt::info!(
        "self_test: i2c2 summary panel_io={=bool} fusb302={=bool}",
        tca6408_present,
        fusb302_present
    );

    PanelProbeResult {
        tca6408_present,
        fusb302_present,
    }
}

#[allow(dead_code)]
pub fn boot_self_test<I2C>(
    i2c: &mut I2C,
    desired_outputs: EnabledOutputs,
    vout_mv: u16,
    ilimit_ma: u16,
    include_vin_ch3: bool,
    tmp_out_a_ok: bool,
    tmp_out_b_ok: bool,
    sync_ok: bool,
    panel_probe: PanelProbeResult,
    therm_kill_asserted: bool,
    force_min_charge: bool,
    keep_charger_on_bms_missing: bool,
    defer_bms_probe: bool,
) -> BootSelfTestResult
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    boot_self_test_with_report(
        i2c,
        desired_outputs,
        vout_mv,
        ilimit_ma,
        include_vin_ch3,
        tmp_out_a_ok,
        tmp_out_b_ok,
        sync_ok,
        panel_probe,
        therm_kill_asserted,
        force_min_charge,
        keep_charger_on_bms_missing,
        defer_bms_probe,
        |_, _| {},
    )
}

pub fn boot_self_test_with_report<I2C, F>(
    i2c: &mut I2C,
    desired_outputs: EnabledOutputs,
    vout_mv: u16,
    ilimit_ma: u16,
    include_vin_ch3: bool,
    tmp_out_a_ok: bool,
    tmp_out_b_ok: bool,
    sync_ok: bool,
    panel_probe: PanelProbeResult,
    therm_kill_asserted: bool,
    force_min_charge: bool,
    keep_charger_on_bms_missing: bool,
    defer_bms_probe: bool,
    mut reporter: F,
) -> BootSelfTestResult
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
    F: FnMut(SelfCheckStage, SelfCheckUiSnapshot),
{
    defmt::info!(
        "self_test: begin vout_mv={=u16} ilimit_ma={=u16} tmp_a_ok={=bool} tmp_b_ok={=bool} sync_ok={=bool} screen_present={=bool} therm_kill_asserted={=bool} force_min_charge={=bool} keep_charger_on_bms_missing={=bool}",
        vout_mv,
        ilimit_ma,
        tmp_out_a_ok,
        tmp_out_b_ok,
        sync_ok,
        panel_probe.screen_present(),
        therm_kill_asserted,
        force_min_charge,
        keep_charger_on_bms_missing
    );

    let mut ui = SelfCheckUiSnapshot::pending(UpsMode::Standby);
    ui.requested_outputs = logic_outputs_from_enabled(desired_outputs);
    ui.gc9307 = if panel_probe.screen_present() {
        SelfCheckCommState::Ok
    } else {
        SelfCheckCommState::Err
    };
    ui.tca6408a = if panel_probe.tca6408_present {
        SelfCheckCommState::Ok
    } else {
        SelfCheckCommState::Err
    };
    ui.fusb302 = if panel_probe.fusb302_present {
        SelfCheckCommState::Ok
    } else {
        SelfCheckCommState::Err
    };
    reporter(SelfCheckStage::Begin, ui);

    // Stage 0: configure independent sensors.
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
    let ina_ready = match ina3221::init_with_config(&mut *i2c, ina_cfg) {
        Ok(()) => {
            defmt::info!("self_test: ina3221 ready cfg=0x{=u16:x}", ina_cfg);
            true
        }
        Err(e) => {
            defmt::error!("self_test: ina3221 init err={}", ina_error_kind(e));
            false
        }
    };

    let tmp_a_read = tmp112::read_temp_c_x16(&mut *i2c, OutputChannel::OutA.tmp_addr());
    let tmp_b_read = tmp112::read_temp_c_x16(&mut *i2c, OutputChannel::OutB.tmp_addr());
    let tmp_a_present = tmp_a_read.is_ok();
    let tmp_b_present = tmp_b_read.is_ok();
    defmt::info!(
        "self_test: sensors ina_ready={=bool} tmp_a_present={=bool} tmp_b_present={=bool} tmp_a_cfg_ok={=bool} tmp_b_cfg_ok={=bool}",
        ina_ready,
        tmp_a_present,
        tmp_b_present,
        tmp_out_a_ok,
        tmp_out_b_ok
    );

    ui.ina3221 = if ina_ready {
        SelfCheckCommState::Ok
    } else {
        SelfCheckCommState::Err
    };
    if ina_ready && include_vin_ch3 {
        ui.vin_vbus_mv = ina3221::read_bus_mv(&mut *i2c, ina3221::Channel::Ch3)
            .ok()
            .and_then(|mv| u16::try_from(mv).ok());
        ui.vin_mains_present = mains_present_from_vin(ui.vin_vbus_mv);
        ui.vin_iin_ma = ina3221::read_shunt_uv(&mut *i2c, ina3221::Channel::Ch3)
            .ok()
            .map(|shunt_uv| ina3221::shunt_uv_to_current_ma(shunt_uv, 7));
    }
    ui.tmp_a = if tmp_a_present && tmp_out_a_ok {
        SelfCheckCommState::Ok
    } else if tmp_a_present {
        SelfCheckCommState::Warn
    } else {
        SelfCheckCommState::Err
    };
    ui.tmp_b = if tmp_b_present && tmp_out_b_ok {
        SelfCheckCommState::Ok
    } else if tmp_b_present {
        SelfCheckCommState::Warn
    } else {
        SelfCheckCommState::Err
    };
    ui.tmp_a_c_x16 = tmp_a_read.ok();
    ui.tmp_a_c = ui.tmp_a_c_x16.map(|v| v / 16);
    ui.tmp_b_c_x16 = tmp_b_read.ok();
    ui.tmp_b_c = ui.tmp_b_c_x16.map(|v| v / 16);
    reporter(SelfCheckStage::Sensors, ui);

    // Stage 1: screen module presence (already probed on I2C2 before entering this function).
    if panel_probe.screen_present() {
        defmt::info!("self_test: stage=screen result=present");
    } else {
        defmt::warn!("self_test: stage=screen result=missing action=disable_screen_module_only");
    }
    reporter(SelfCheckStage::Screen, ui);

    // Stage 2: BQ40Z50.
    let mut bms_addr: Option<u8> = None;
    let mut bms_voltage_mv: Option<u16> = None;
    let mut bms_current_ma: Option<i16> = None;
    let mut bms_soc_pct: Option<u16> = None;
    let mut bms_rca_alarm: Option<bool> = None;
    let mut bms_no_battery: Option<bool> = None;
    let mut bms_discharge_ready: Option<bool> = None;
    let mut bms_discharge_reason: Option<&'static str> = None;
    let mut bms_charge_ready: Option<bool> = None;
    let mut bms_charge_reason: Option<&'static str> = None;
    let mut bms_flow: Option<&'static str> = None;
    let mut bms_primary_reason: Option<&'static str> = None;
    let mut initial_bms_protection_active = false;
    if defer_bms_probe {
        defmt::info!(
            "self_test: bq40z50 probe deferred until activation auto_request settle_ms={=u64}",
            BMS_ACTIVATION_AUTO_DELAY.as_millis() as u64
        );
        ui.bq40z50 = SelfCheckCommState::Err;
    } else {
        for addr in bms_probe_candidates().iter().copied() {
            let temp = bq40z50::read_u16(&mut *i2c, addr, bq40z50::cmd::TEMPERATURE);
            let voltage = bq40z50::read_u16(&mut *i2c, addr, bq40z50::cmd::VOLTAGE);
            let current = bq40z50::read_i16(&mut *i2c, addr, bq40z50::cmd::CURRENT);
            let soc = bq40z50::read_u16(&mut *i2c, addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE);
            let status = bq40z50::read_u16(&mut *i2c, addr, bq40z50::cmd::BATTERY_STATUS);
            let op_status = bq40z50::read_operation_status(&mut *i2c, addr)
                .ok()
                .flatten();

            if let (Ok(temp_k_x10), Ok(voltage_mv), Ok(current_ma), Ok(soc_pct), Ok(status_raw)) =
                (temp, voltage, current, soc, status)
            {
                let temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(temp_k_x10);
                let err_code = bq40z50::battery_status::error_code(status_raw);
                let mut op_status = op_status;
                let mut charge_ready;
                let mut charge_reason;
                let mut discharge_ready;
                let mut discharge_reason;
                let mut primary_reason;
                (charge_ready, charge_reason) = bq40_decode_charge_path(op_status);
                (discharge_ready, discharge_reason) = bq40_decode_discharge_path(op_status);
                primary_reason =
                    bq40_primary_reason(status_raw, op_status, charge_reason, discharge_reason);
                if err_code == 0
                    && !bq40_pack_indicates_no_battery(voltage_mv)
                    && discharge_ready != Some(true)
                {
                    for attempt in 1..=BMS_SELF_TEST_DISCHARGE_READY_RETRIES {
                        let start = Instant::now();
                        while start.elapsed() < BMS_SELF_TEST_DISCHARGE_READY_RETRY_DELAY {
                            core::hint::spin_loop();
                        }
                        let retry_op_status = bq40z50::read_operation_status(&mut *i2c, addr)
                            .ok()
                            .flatten();
                        let (retry_charge_ready, retry_charge_reason) =
                            bq40_decode_charge_path(retry_op_status);
                        let (retry_discharge_ready, retry_discharge_reason) =
                            bq40_decode_discharge_path(retry_op_status);
                        let retry_primary_reason = bq40_primary_reason(
                            status_raw,
                            retry_op_status,
                            retry_charge_reason,
                            retry_discharge_reason,
                        );
                        defmt::info!(
                            "self_test: bq40z50 settle attempt={=u8}/{=u8} addr=0x{=u8:x} discharge_ready={=?} charge_ready={=?} primary_reason={} op_status={=?}",
                            attempt as u8,
                            BMS_SELF_TEST_DISCHARGE_READY_RETRIES as u8,
                            addr,
                            retry_discharge_ready,
                            retry_charge_ready,
                            retry_primary_reason,
                            retry_op_status
                        );
                        op_status = retry_op_status;
                        charge_ready = retry_charge_ready;
                        charge_reason = retry_charge_reason;
                        discharge_ready = retry_discharge_ready;
                        discharge_reason = retry_discharge_reason;
                        primary_reason = retry_primary_reason;
                        if discharge_ready == Some(true) {
                            break;
                        }
                    }
                }
                let xchg = bq40_op_bit(op_status, bq40z50::operation_status::XCHG);
                let xdsg = bq40_op_bit(op_status, bq40z50::operation_status::XDSG);
                let chg_fet = bq40_op_bit(op_status, bq40z50::operation_status::CHG);
                let dsg_fet = bq40_op_bit(op_status, bq40z50::operation_status::DSG);
                let flow = bq40_decode_current_flow(current_ma);
                let flow_abs_ma = current_ma.wrapping_abs() as u16;
                let protection_active = bq40_protection_active(status_raw, op_status);
                defmt::info!(
                    "self_test: bq40z50 ok addr=0x{=u8:x} temp_c_x10={=i32} voltage_mv={=u16} current_ma={=i16} flow={} flow_abs_ma={=u16} soc_pct={=u16} status=0x{=u16:x} op_status={=?} xchg={=?} xdsg={=?} chg_fet={=?} dsg_fet={=?} chg_ready={=?} dsg_ready={=?} chg_reason={} dsg_reason={} primary_reason={} err_code={} err_str={}",
                    addr,
                    temp_c_x10,
                    voltage_mv,
                    current_ma,
                    flow,
                    flow_abs_ma,
                    soc_pct,
                    status_raw,
                    op_status,
                    xchg,
                    xdsg,
                    chg_fet,
                    dsg_fet,
                    charge_ready,
                    discharge_ready,
                    charge_reason,
                    discharge_reason,
                    primary_reason,
                    err_code,
                    bq40z50::decode_error_code(err_code)
                );
                if primary_reason == "xdsg_blocked" || primary_reason == "xchg_blocked" {
                    log_bq40_block_detail(&mut *i2c, addr, "self_test_blocked", op_status);
                }
                bms_addr = Some(addr);
                initial_bms_protection_active = protection_active;
                bms_voltage_mv = Some(voltage_mv);
                bms_current_ma = Some(current_ma);
                bms_soc_pct = Some(soc_pct);
                bms_rca_alarm = Some((status_raw & bq40z50::battery_status::RCA) != 0);
                let no_battery_confirmed = bq40_self_test_no_battery_confirmed(
                    &mut *i2c, addr, temp_k_x10, voltage_mv, current_ma, soc_pct, status_raw,
                );
                if bq40_pack_indicates_no_battery(voltage_mv) && !no_battery_confirmed {
                    defmt::info!(
                        "self_test: bq40z50 low_pack candidate rejected addr=0x{=u8:x} voltage_mv={=u16} current_ma={=i16} soc_pct={=u16} status=0x{=u16:x}",
                        addr,
                        voltage_mv,
                        current_ma,
                        soc_pct,
                        status_raw
                    );
                }
                bms_no_battery = Some(no_battery_confirmed);
                bms_charge_ready = charge_ready;
                bms_charge_reason = Some(charge_reason);
                bms_discharge_ready = discharge_ready;
                bms_discharge_reason = Some(discharge_reason);
                bms_flow = Some(flow);
                bms_primary_reason = Some(primary_reason);
                break;
            }

            defmt::warn!("self_test: bq40z50 miss addr=0x{=u8:x}", addr);
        }

        if bms_addr.is_none() {
            defmt::warn!("self_test: bq40z50 missing/err; battery module disabled");
            ui.bq40z50 = SelfCheckCommState::Err;
        } else if bms_no_battery == Some(true) {
            defmt::warn!(
                "self_test: bq40z50 no battery voltage_mv={=?} flow={=?} primary_reason={=?}",
                bms_voltage_mv,
                bms_flow,
                bms_primary_reason
            );
            ui.bq40z50 = SelfCheckCommState::Warn;
        } else if bms_discharge_ready != Some(true) {
            defmt::warn!(
                "self_test: bq40z50 discharge path not ready state={=?} reason={=?} charge_ready={=?} charge_reason={=?} flow={=?} primary_reason={=?}",
                bms_discharge_ready,
                bms_discharge_reason,
                bms_charge_ready,
                bms_charge_reason,
                bms_flow,
                bms_primary_reason
            );
            ui.bq40z50 = SelfCheckCommState::Warn;
        } else if bms_rca_alarm == Some(true) {
            defmt::warn!(
                "self_test: bq40z50 remaining capacity alarm flow={=?} primary_reason={=?}",
                bms_flow,
                bms_primary_reason
            );
            ui.bq40z50 = SelfCheckCommState::Warn;
        } else {
            ui.bq40z50 = SelfCheckCommState::Ok;
        }
    }
    ui.bq40z50_pack_mv = bms_voltage_mv;
    ui.bq40z50_current_ma = bms_current_ma;
    ui.bq40z50_soc_pct = bms_soc_pct;
    ui.bq40z50_rca_alarm = bms_rca_alarm;
    ui.bq40z50_no_battery = bms_no_battery;
    ui.bq40z50_discharge_ready = bms_discharge_ready;
    ui.bq40z50_issue_detail = match (bms_no_battery, bms_primary_reason) {
        (Some(true), _) => Some("no_battery"),
        (_, Some(primary_reason)) => bq40_ui_issue_detail(false, primary_reason),
        _ => None,
    };
    reporter(SelfCheckStage::Bms, ui);

    // Stage 3: BQ25792.
    let mut charger_ctrl0: Option<u8> = None;
    let mut charger_status0: Option<u8> = None;
    let mut charger_input_present: Option<bool> = None;
    let mut charger_enabled = match bq25792::read_u8(&mut *i2c, bq25792::reg::CHARGER_CONTROL_0) {
        Ok(v) => {
            charger_ctrl0 = Some(v);
            defmt::info!("self_test: bq25792 ok ctrl0=0x{=u8:x}", v);
            true
        }
        Err(e) => {
            defmt::warn!(
                "self_test: bq25792 miss err={} action=disable_charger_module",
                i2c_error_kind(e)
            );
            false
        }
    };
    let mut charger_vbat_present: Option<bool> = None;
    let mut charger_vbus_adc_mv: Option<u16> = None;
    let mut charger_ibus_adc_ma: Option<i32> = None;
    let mut charger_ibat_adc_ma: Option<i16> = None;
    let mut initial_audio_charge_phase = AudioChargePhase::Unknown;
    if charger_enabled {
        charger_status0 = bq25792::read_u8(&mut *i2c, bq25792::reg::CHARGER_STATUS_0).ok();
        let charger_status1 = bq25792::read_u8(&mut *i2c, bq25792::reg::CHARGER_STATUS_1).ok();
        let charger_status2 = bq25792::read_u8(&mut *i2c, bq25792::reg::CHARGER_STATUS_2).ok();
        let charger_status3 = bq25792::read_u8(&mut *i2c, bq25792::reg::CHARGER_STATUS_3).ok();
        let adc_state = bq25792::ensure_adc_power_path(&mut *i2c).ok();
        let charger_vbus_adc_mv_local =
            bq25792::read_adc_u16(&mut *i2c, bq25792::reg::VBUS_ADC).ok();
        let charger_ibus_adc_ma_local =
            bq25792::read_adc_i16(&mut *i2c, bq25792::reg::IBUS_ADC).ok();
        let charger_ibat_adc_ma_local =
            bq25792::read_adc_i16(&mut *i2c, bq25792::reg::IBAT_ADC).ok();
        let charger_vbat_adc_mv = bq25792::read_adc_u16(&mut *i2c, bq25792::reg::VBAT_ADC).ok();
        let charger_vsys_adc_mv = bq25792::read_adc_u16(&mut *i2c, bq25792::reg::VSYS_ADC).ok();

        if let Some(status1) = charger_status1 {
            initial_audio_charge_phase =
                audio_charge_phase_from_chg_stat(bq25792::status1::chg_stat(status1));
        }
        let vbat_present = charger_status2.map(|v| (v & bq25792::status2::VBAT_PRESENT_STAT) != 0);
        charger_vbat_present = vbat_present;
        let vsys_min_reg = charger_status3.map(|v| (v & bq25792::status3::VSYS_STAT) != 0);
        let input_present = charger_status0
            .map(|status0| {
                (status0 & bq25792::status0::VBUS_PRESENT_STAT) != 0
                    || (status0 & bq25792::status0::AC1_PRESENT_STAT) != 0
                    || (status0 & bq25792::status0::AC2_PRESENT_STAT) != 0
                    || (status0 & bq25792::status0::PG_STAT) != 0
            })
            .unwrap_or(false);
        charger_input_present = Some(input_present);
        let adc_ready = match (adc_state, charger_status3) {
            (Some(adc_state), Some(status3)) => bq25792::power_path_adc_ready(adc_state, status3),
            _ => false,
        };
        let input_sample = normalize_charger_input_power_sample(
            input_present,
            adc_ready,
            charger_vbus_adc_mv_local,
            charger_ibus_adc_ma_local,
        );
        charger_vbus_adc_mv = input_sample.ui_vbus_mv;
        charger_ibus_adc_ma = input_sample.ui_ibus_ma;
        charger_ibat_adc_ma = adc_ready.then_some(charger_ibat_adc_ma_local).flatten();
        defmt::info!(
            "self_test: bq25792 ctrl0={=?} status0={=?} status1={=?} status2={=?} status3={=?} vbat_present={=?} phase={} vsys_min_reg={=?} vbus_adc_mv={=?} ibus_adc_ma={=?} ibat_adc_ma={=?} ui_vbus_mv={=?} ui_ibus_ma={=?} adc_ready={=bool} vbat_adc_mv={=?} vsys_adc_mv={=?}",
            charger_ctrl0,
            charger_status0,
            charger_status1,
            charger_status2,
            charger_status3,
            vbat_present,
            bq25792::decode_chg_stat(
                charger_status1
                    .map(bq25792::status1::chg_stat)
                    .unwrap_or_default()
            ),
            vsys_min_reg,
            charger_vbus_adc_mv_local,
            charger_ibus_adc_ma_local,
            charger_ibat_adc_ma,
            input_sample.ui_vbus_mv,
            input_sample.ui_ibus_ma,
            adc_ready,
            charger_vbat_adc_mv,
            charger_vsys_adc_mv
        );
    }
    ui.bq25792 = if charger_enabled {
        SelfCheckCommState::Ok
    } else {
        SelfCheckCommState::Err
    };
    ui.bq25792_ichg_ma = bq25792::read_u16(&mut *i2c, bq25792::reg::CHARGE_CURRENT_LIMIT)
        .ok()
        .map(|v| (v & 0x01ff) * 10);
    ui.bq25792_ibat_ma = charger_ibat_adc_ma;
    ui.bq25792_vbat_present = charger_vbat_present;
    ui.input_vbus_mv = charger_vbus_adc_mv;
    ui.input_ibus_ma = charger_ibus_adc_ma;
    if let Some(status0) = charger_status0 {
        let vbus_present = (status0 & bq25792::status0::VBUS_PRESENT_STAT) != 0
            || (status0 & bq25792::status0::AC1_PRESENT_STAT) != 0
            || (status0 & bq25792::status0::AC2_PRESENT_STAT) != 0
            || (status0 & bq25792::status0::PG_STAT) != 0;
        ui.fusb302_vbus_present = Some(vbus_present);
    }
    let charger_probe_ok = charger_enabled;
    reporter(SelfCheckStage::Charger, ui);

    // Stage 4: TPS55288.
    let tps_a_present = ::tps55288::Tps55288::with_address(&mut *i2c, OutputChannel::OutA.addr())
        .read_reg(::tps55288::registers::addr::MODE)
        .is_ok();
    let tps_b_present = ::tps55288::Tps55288::with_address(&mut *i2c, OutputChannel::OutB.addr())
        .read_reg(::tps55288::registers::addr::MODE)
        .is_ok();
    let status_a = if tps_a_present {
        ::tps55288::Tps55288::with_address(&mut *i2c, OutputChannel::OutA.addr())
            .read_reg(::tps55288::registers::addr::STATUS)
            .map_err(tps_error_kind)
    } else {
        Err("not_present")
    };
    let status_b = if tps_b_present {
        ::tps55288::Tps55288::with_address(&mut *i2c, OutputChannel::OutB.addr())
            .read_reg(::tps55288::registers::addr::STATUS)
            .map_err(tps_error_kind)
    } else {
        Err("not_present")
    };

    let tps_a_fault = matches!(
        &status_a,
        Ok(v)
            if ::tps55288::registers::StatusBits::from_bits_truncate(*v).intersects(
                ::tps55288::registers::StatusBits::SCP
                    | ::tps55288::registers::StatusBits::OCP
                    | ::tps55288::registers::StatusBits::OVP
            )
    );
    let initial_tps_a_over_voltage = matches!(
        &status_a,
        Ok(v)
            if ::tps55288::registers::StatusBits::from_bits_truncate(*v)
                .contains(::tps55288::registers::StatusBits::OVP)
    );
    let initial_tps_b_over_voltage = matches!(
        &status_b,
        Ok(v)
            if ::tps55288::registers::StatusBits::from_bits_truncate(*v)
                .contains(::tps55288::registers::StatusBits::OVP)
    );
    let initial_tps_a_over_current = matches!(
        &status_a,
        Ok(v)
            if ::tps55288::registers::StatusBits::from_bits_truncate(*v).intersects(
                ::tps55288::registers::StatusBits::OCP
                    | ::tps55288::registers::StatusBits::SCP
            )
    );
    let initial_tps_b_over_current = matches!(
        &status_b,
        Ok(v)
            if ::tps55288::registers::StatusBits::from_bits_truncate(*v).intersects(
                ::tps55288::registers::StatusBits::OCP
                    | ::tps55288::registers::StatusBits::SCP
            )
    );
    let detected_tmp_outputs = enabled_outputs_from_flags(tmp_a_present, tmp_b_present);
    let detected_tps_outputs = enabled_outputs_from_flags(tps_a_present, tps_b_present);
    let tps_b_fault = matches!(
        &status_b,
        Ok(v)
            if ::tps55288::registers::StatusBits::from_bits_truncate(*v).intersects(
                ::tps55288::registers::StatusBits::SCP
                    | ::tps55288::registers::StatusBits::OCP
                    | ::tps55288::registers::StatusBits::OVP
            )
    );

    let mut out_a_allowed = desired_outputs.is_enabled(OutputChannel::OutA)
        && sync_ok
        && ina_ready
        && tps_a_present
        && status_a.is_ok()
        && !tps_a_fault
        && tmp_a_present
        && tmp_out_a_ok;
    let mut out_b_allowed = desired_outputs.is_enabled(OutputChannel::OutB)
        && sync_ok
        && ina_ready
        && tps_b_present
        && status_b.is_ok()
        && !tps_b_fault
        && tmp_b_present
        && tmp_out_b_ok;
    let mut recoverable_outputs = enabled_outputs_from_flags(out_a_allowed, out_b_allowed);
    let mut output_gate_reason = OutputGateReason::None;
    let bms_block_recoverable_outputs = enabled_outputs_from_flags(
        desired_outputs.is_enabled(OutputChannel::OutA)
            && sync_ok
            && ina_ready
            && !tps_a_fault
            && tmp_a_present
            && tmp_out_a_ok,
        desired_outputs.is_enabled(OutputChannel::OutB)
            && sync_ok
            && ina_ready
            && !tps_b_fault
            && tmp_b_present
            && tmp_out_b_ok,
    );

    if desired_outputs.is_enabled(OutputChannel::OutA) && !out_a_allowed {
        defmt::warn!(
            "self_test: tps out_a disabled sync_ok={=bool} ina_ready={=bool} tps_present={=bool} status={=?} fault={=bool} tmp_present={=bool} tmp_cfg_ok={=bool}",
            sync_ok,
            ina_ready,
            tps_a_present,
            status_a,
            tps_a_fault,
            tmp_a_present,
            tmp_out_a_ok
        );
    }
    if desired_outputs.is_enabled(OutputChannel::OutB) && !out_b_allowed {
        defmt::warn!(
            "self_test: tps out_b disabled sync_ok={=bool} ina_ready={=bool} tps_present={=bool} status={=?} fault={=bool} tmp_present={=bool} tmp_cfg_ok={=bool}",
            sync_ok,
            ina_ready,
            tps_b_present,
            status_b,
            tps_b_fault,
            tmp_b_present,
            tmp_out_b_ok
        );
    }

    if bms_addr.is_none() || bms_discharge_ready != Some(true) {
        // Policy: when BMS comm is missing or discharge path is not ready, keep TPS outputs off.
        recoverable_outputs = enabled_outputs_from_flags(
            recoverable_outputs.is_enabled(OutputChannel::OutA)
                || bms_block_recoverable_outputs.is_enabled(OutputChannel::OutA),
            recoverable_outputs.is_enabled(OutputChannel::OutB)
                || bms_block_recoverable_outputs.is_enabled(OutputChannel::OutB),
        );
        if recoverable_outputs != EnabledOutputs::None {
            output_gate_reason = OutputGateReason::BmsNotReady;
        }
        out_a_allowed = false;
        out_b_allowed = false;

        if bms_addr.is_none() {
            if force_min_charge {
                defmt::warn!(
                    "self_test: bq40z50 missing; keep charger module for force_min_charge (charger_probe_ok={=bool})",
                    charger_enabled
                );
            } else if keep_charger_on_bms_missing {
                defmt::info!(
                    "self_test: bq40z50 missing; keep charger module for boot_diag_auto_validate (charger_probe_ok={=bool})",
                    charger_enabled
                );
            } else {
                if charger_enabled {
                    defmt::warn!("self_test: force disable charger because bq40z50 is missing");
                }
                charger_enabled = false;
            }
        } else {
            defmt::warn!(
                "self_test: bq40z50 discharge path not ready; block tps until activation/recovery"
            );
        }
    }

    let discharge_authorization_reason = if desired_outputs == EnabledOutputs::None {
        "not_requested"
    } else if bms_addr.is_none() {
        "bms_missing"
    } else if bms_no_battery == Some(true) {
        "no_battery"
    } else if bms_rca_alarm == Some(true) {
        "pack_alarm"
    } else if bms_discharge_ready == Some(true) {
        "already_ready"
    } else if therm_kill_asserted {
        "therm_kill_asserted"
    } else if !charger_probe_ok {
        "charger_missing"
    } else if charger_input_present != Some(true) {
        "input_not_present"
    } else {
        "eligible"
    };
    defmt::info!(
        "self_test: discharge_authorization decision={} requested_outputs={} bms_present={=bool} dsg_ready={=?} no_battery={=?} rca_alarm={=?} charger_probe_ok={=bool} input_present={=?} therm_kill_asserted={=bool}",
        discharge_authorization_reason,
        desired_outputs.describe(),
        bms_addr.is_some(),
        bms_discharge_ready,
        bms_no_battery,
        bms_rca_alarm,
        charger_probe_ok,
        charger_input_present,
        therm_kill_asserted
    );

    // Emergency-stop path: only this path is allowed to change TPS output state in self-test.
    if therm_kill_asserted || tps_a_fault || tps_b_fault {
        defmt::error!(
            "self_test: emergency_stop therm_kill_asserted={=bool} tps_a_fault={=bool} tps_b_fault={=bool}",
            therm_kill_asserted,
            tps_a_fault,
            tps_b_fault
        );
        if tps_a_present {
            if let Err(e) =
                ::tps55288::Tps55288::with_address(&mut *i2c, OutputChannel::OutA.addr())
                    .disable_output()
            {
                defmt::warn!(
                    "self_test: emergency out_a disable err={}",
                    tps_error_kind(e)
                );
            }
        }
        if tps_b_present {
            if let Err(e) =
                ::tps55288::Tps55288::with_address(&mut *i2c, OutputChannel::OutB.addr())
                    .disable_output()
            {
                defmt::warn!(
                    "self_test: emergency out_b disable err={}",
                    tps_error_kind(e)
                );
            }
        }
        out_a_allowed = false;
        out_b_allowed = false;
        recoverable_outputs = EnabledOutputs::None;
        output_gate_reason = if therm_kill_asserted {
            OutputGateReason::ThermKill
        } else {
            OutputGateReason::TpsFault
        };
    }

    ui.tps_a = if tps_a_present {
        if tps_a_fault {
            SelfCheckCommState::Warn
        } else if status_a.is_ok() {
            SelfCheckCommState::Ok
        } else {
            SelfCheckCommState::Err
        }
    } else {
        SelfCheckCommState::Err
    };
    ui.tps_b = if tps_b_present {
        if tps_b_fault {
            SelfCheckCommState::Warn
        } else if status_b.is_ok() {
            SelfCheckCommState::Ok
        } else {
            SelfCheckCommState::Err
        }
    } else {
        SelfCheckCommState::Err
    };
    ui.active_outputs =
        logic_outputs_from_enabled(enabled_outputs_from_flags(out_a_allowed, out_b_allowed));
    ui.recoverable_outputs = logic_outputs_from_enabled(recoverable_outputs);
    ui.output_gate_reason = output_gate_reason;
    ui.bq40z50_recovery_action = bq40_recovery_action_for_snapshot(
        &ui,
        logic_outputs_from_enabled(desired_outputs),
        output_gate_reason,
        bms_addr,
        charger_probe_ok,
        therm_kill_asserted,
    );
    ui.bq40z50_recovery_pending = false;
    ui.tps_a_enabled = Some(false);
    ui.tps_b_enabled = Some(false);
    ui.bq25792_allow_charge = Some(charger_enabled);
    reporter(SelfCheckStage::Tps, ui);

    let enabled_outputs = enabled_outputs_from_flags(out_a_allowed, out_b_allowed);

    ui.mode = ups_mode_from_mains(
        stable_mains_present(
            ui.vin_mains_present,
            ui.vin_vbus_mv,
            ui.fusb302_vbus_present,
        ),
        out_a_allowed || out_b_allowed,
    );

    defmt::info!(
        "self_test: done requested_outputs={} active_outputs={} recoverable_outputs={} gate_reason={} charger_enabled={=bool} bms_present={=bool}",
        desired_outputs.describe(),
        enabled_outputs.describe(),
        recoverable_outputs.describe(),
        output_gate_reason.as_str(),
        charger_enabled,
        bms_addr.is_some()
    );

    reporter(SelfCheckStage::Done, ui);

    BootSelfTestResult {
        ina_detected: ina_ready,
        detected_tmp_outputs,
        detected_tps_outputs,
        requested_outputs: enabled_outputs_from_flags(
            enabled_outputs.is_enabled(OutputChannel::OutA)
                || recoverable_outputs.is_enabled(OutputChannel::OutA),
            enabled_outputs.is_enabled(OutputChannel::OutB)
                || recoverable_outputs.is_enabled(OutputChannel::OutB),
        ),
        active_outputs: enabled_outputs,
        recoverable_outputs,
        output_gate_reason,
        charger_probe_ok,
        charger_enabled,
        initial_audio_charge_phase,
        initial_bms_protection_active,
        initial_tps_a_over_voltage,
        initial_tps_b_over_voltage,
        initial_tps_a_over_current,
        initial_tps_b_over_current,
        bms_addr,
        self_check_snapshot: ui,
    }
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
    next_vin_telemetry_skip_at: Instant,
    next_fan_temp_refresh_at: Instant,
    last_fault_log_at: Option<Instant>,
    last_input_power_anomaly_log_at: Option<Instant>,
    last_therm_kill_hint_at: Option<Instant>,
    tps_a_fault_latch: TpsFaultLatch,
    tps_b_fault_latch: TpsFaultLatch,
    fan_started_at: Instant,
    fan_rpm_tracker: FanRpmTracker,
    applied_fan_state: AppliedFanState,

    ina_ready: bool,
    ina_next_retry_at: Option<Instant>,

    tps_a_ready: bool,
    tps_a_next_retry_at: Option<Instant>,
    tps_b_ready: bool,
    tps_b_next_retry_at: Option<Instant>,

    bms_addr: Option<u8>,
    bms_runtime_seen: bool,
    bms_next_poll_at: Instant,
    bms_next_retry_at: Option<Instant>,
    bms_last_int_poll_at: Option<Instant>,
    bms_poll_seq: u32,
    bms_ok_streak: u16,
    bms_err_streak: u16,
    bms_cached_da_status2: Option<bq40z50::DaStatus2>,
    bms_cached_filter_capacity: Option<bq40z50::FilterCapacity>,
    bms_cached_balance_config: Option<bq40z50::BalanceConfig>,
    bms_cached_gauging_status: Option<u32>,
    bms_cached_lock_diag: Option<Bq40LockDiagSnapshot>,
    bms_next_da_status2_refresh_at: Instant,
    bms_next_filter_capacity_refresh_at: Instant,
    bms_next_balance_config_refresh_at: Instant,
    bms_next_gauging_status_refresh_at: Instant,
    bms_next_lock_diag_refresh_at: Instant,
    bms_next_block_detail_log_at: Instant,
    bms_config_logged: bool,
    bms_next_config_log_at: Instant,
    out_a_next_path_diag_log_at: Instant,
    out_b_next_path_diag_log_at: Instant,

    chg_next_poll_at: Instant,
    chg_next_retry_at: Option<Instant>,
    chg_enabled: bool,
    charger_allowed: bool,
    charge_policy: ChargePolicyMemory,
    charge_policy_derate: ChargePolicyDerateTracker,
    charge_policy_output_load: ChargePolicyOutputLoadTracker,
    manual_charge_prefs: ManualChargePrefs,
    manual_charge_prefs_offset: u16,
    manual_charge_storage_ready: bool,
    manual_charge_storage_incompatible: bool,
    manual_charge_runtime: ManualChargeRuntime,
    bms_charge_ready: Option<bool>,
    bms_full: Option<bool>,
    bms_cell_min_mv: Option<u16>,
    chg_last_int_poll_at: Option<Instant>,
    bms_activation_state: BmsActivationState,
    bms_activation_phase: BmsActivationPhase,
    bms_activation_started_at: Option<Instant>,
    bms_activation_deadline: Option<Instant>,
    bms_activation_diag_stage: usize,
    bms_activation_followup_next_at: Option<Instant>,
    bms_activation_followup_attempts: u16,
    bms_activation_exercise_next_at: Option<Instant>,
    bms_activation_pattern_tracker: Bq40ActivationPatternTracker,
    bms_activation_isolation_until: Option<Instant>,
    bms_activation_force_charge_requested: bool,
    bms_boot_diag_started_at: Instant,
    bms_boot_diag_ship_reset_attempted: bool,
    bms_activation_auto_due_at: Instant,
    bms_activation_auto_poll_release_at: Instant,
    bms_activation_auto_attempted: bool,
    bms_activation_current_is_auto: bool,
    bms_activation_request_kind: BmsRecoveryRequestKind,
    bms_activation_auto_force_charge_until: Option<Instant>,
    bms_activation_auto_force_charge_programmed: bool,
    bms_activation_auto_defer_logged: bool,
    bms_activation_backup: Option<ChargerActivationBackup>,
    chg_watchdog_restore: Option<u8>,
    output_state: OutputRuntimeState,
    output_protection: output_protection::ProtectionRuntime,
    fan: fan::Controller,
    vin_sample_missing_streak: u8,
    usb_pd_state: usb_pd::UsbPdPortState,
    usb_pd_input_current_limit_ma: Option<u16>,
    usb_pd_vindpm_mv: Option<u16>,
    usb_pd_vac1_mv: Option<u16>,
    usb_pd_input_limit_backup: Option<UsbPdInputLimitBackup>,
    usb_pd_restore_input_limits_pending: bool,

    ui_snapshot: SelfCheckUiSnapshot,
    audio_snapshot: AudioSignalSnapshot,
    audio_events: AudioSignalEvents,
    audio_signals_ready: bool,
    charger_audio: ChargerAudioState,
    bms_audio: BmsAudioState,
    tps_audio: TpsAudioState,
}

#[derive(Clone, Copy)]
struct ChargerActivationBackup {
    ctrl0: u8,
    vreg_reg: u16,
    ichg_reg: u16,
    iindpm_reg: u16,
    chg_enabled: bool,
}

#[derive(Clone, Copy)]
struct UsbPdInputLimitBackup {
    vindpm_mv: u16,
    iindpm_ma: u16,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BmsActivationPhase {
    ProbeWithoutCharge,
    WaitChargeOff,
    WaitMinChargeSettle,
    MinChargeProbe,
    WakeProbe,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BmsRecoveryRequestKind {
    Activation,
    DischargeAuthorization,
}

impl BmsRecoveryRequestKind {
    const fn request_name(self) -> &'static str {
        match self {
            Self::Activation => "activation",
            Self::DischargeAuthorization => "discharge_authorization",
        }
    }
}

fn bms_activation_phase_name(phase: BmsActivationPhase) -> &'static str {
    match phase {
        BmsActivationPhase::ProbeWithoutCharge => "probe_without_charge",
        BmsActivationPhase::WaitChargeOff => "wait_charge_off",
        BmsActivationPhase::WaitMinChargeSettle => "wait_min_charge_settle",
        BmsActivationPhase::MinChargeProbe => "min_charge_probe",
        BmsActivationPhase::WakeProbe => "wake_probe",
    }
}

fn bms_activation_phase_allows_force_charge(phase: BmsActivationPhase) -> bool {
    matches!(
        phase,
        BmsActivationPhase::WaitMinChargeSettle
            | BmsActivationPhase::MinChargeProbe
            | BmsActivationPhase::WakeProbe
    )
}

fn bms_activation_phase_forces_charge_off(phase: BmsActivationPhase) -> bool {
    matches!(phase, BmsActivationPhase::WaitChargeOff)
}

#[derive(Clone, Copy)]
pub struct Config {
    pub ina_detected: bool,
    pub detected_tmp_outputs: EnabledOutputs,
    pub detected_tps_outputs: EnabledOutputs,
    pub requested_outputs: EnabledOutputs,
    pub active_outputs: EnabledOutputs,
    pub recoverable_outputs: EnabledOutputs,
    pub output_gate_reason: OutputGateReason,
    pub vout_mv: u16,
    pub ilimit_ma: u16,
    pub telemetry_period: Duration,
    pub retry_backoff: Duration,
    pub fault_log_min_interval: Duration,
    pub telemetry_include_vin_ch3: bool,
    pub tmp112_tlow_c_x16: i16,
    pub tmp112_thigh_c_x16: i16,
    pub protect_tmp_temp_derate_c_x16: i16,
    pub protect_tmp_temp_resume_c_x16: i16,
    pub protect_tmp_temp_shutdown_c_x16: i16,
    pub protect_other_temp_derate_c_x16: i16,
    pub protect_other_temp_resume_c_x16: i16,
    pub protect_other_temp_shutdown_c_x16: i16,
    pub protect_temp_hold: Duration,
    pub protect_current_derate_ma: i32,
    pub protect_current_resume_ma: i32,
    pub protect_current_hold: Duration,
    pub protect_ilim_step_ma: u16,
    pub protect_ilim_step_interval: Duration,
    pub protect_min_ilim_ma: u16,
    pub protect_shutdown_vout_mv: u16,
    pub protect_shutdown_hold: Duration,
    pub fan_config: fan::Config,
    pub fan_control_enabled: bool,
    pub thermal_protection_enabled: bool,
    pub tmp_hw_protect_test_mode: bool,
    pub charger_probe_ok: bool,
    pub charger_enabled: bool,
    pub initial_audio_charge_phase: AudioChargePhase,
    pub initial_bms_protection_active: bool,
    pub initial_tps_a_over_voltage: bool,
    pub initial_tps_b_over_voltage: bool,
    pub initial_tps_a_over_current: bool,
    pub initial_tps_b_over_current: bool,
    pub force_min_charge: bool,
    pub bms_boot_diag_auto_validate: bool,
    pub bms_addr: Option<u8>,
    pub self_check_snapshot: SelfCheckUiSnapshot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioChargePhase {
    Unknown,
    NotCharging,
    Charging,
    Completed,
}

impl Default for AudioChargePhase {
    fn default() -> Self {
        Self::Unknown
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioBatteryLowState {
    Unknown,
    Inactive,
    WithMains,
    NoMains,
}

impl Default for AudioBatteryLowState {
    fn default() -> Self {
        Self::Unknown
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioSignalSnapshot {
    pub mains_present: Option<bool>,
    pub mains_source: AudioMainsSource,
    pub charge_phase: AudioChargePhase,
    pub thermal_stress: bool,
    pub battery_low: AudioBatteryLowState,
    pub battery_protection: bool,
    pub module_fault: bool,
    pub io_over_voltage: bool,
    pub io_over_current: bool,
    pub shutdown_protection: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioSignalEvents {
    pub mains_present_changed: Option<bool>,
    pub charge_phase_changed: Option<AudioChargePhase>,
    pub thermal_stress_changed: Option<bool>,
    pub battery_low_changed: Option<AudioBatteryLowState>,
    pub battery_protection_changed: Option<bool>,
    pub module_fault_changed: Option<bool>,
    pub io_over_voltage_changed: Option<bool>,
    pub io_over_current_changed: Option<bool>,
    pub shutdown_protection_changed: Option<bool>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ChargerAudioState {
    input_present: Option<bool>,
    phase: AudioChargePhase,
    thermal_stress: bool,
    ts_warm: bool,
    ts_hot: bool,
    treg: bool,
    over_voltage: bool,
    over_current: bool,
    shutdown_protection: bool,
    module_fault: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChargerInputSampleIssue {
    AdcNotReady,
    VbusMissing,
    VbusOutOfRange,
    IbusMissing,
    IbusOutOfRange,
}

impl ChargerInputSampleIssue {
    const fn as_str(self) -> &'static str {
        match self {
            Self::AdcNotReady => "adc_not_ready",
            Self::VbusMissing => "vbus_missing",
            Self::VbusOutOfRange => "vbus_out_of_range",
            Self::IbusMissing => "ibus_missing",
            Self::IbusOutOfRange => "ibus_out_of_range",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ChargerInputPowerSample {
    raw_vbus_mv: Option<u16>,
    raw_ibus_ma: Option<i16>,
    ui_vbus_mv: Option<u16>,
    ui_ibus_ma: Option<i32>,
    raw_power_w10: Option<u32>,
    issue: Option<ChargerInputSampleIssue>,
}

fn normalize_charger_input_power_sample(
    input_present: bool,
    adc_ready: bool,
    raw_vbus_mv: Option<u16>,
    raw_ibus_ma: Option<i16>,
) -> ChargerInputPowerSample {
    let raw_power_w10 = match (raw_vbus_mv, raw_ibus_ma) {
        (Some(vbus_mv), Some(ibus_ma)) => {
            Some((vbus_mv as u32 * ibus_ma.unsigned_abs() as u32) / 100_000)
        }
        _ => None,
    };

    let mut sample = ChargerInputPowerSample {
        raw_vbus_mv,
        raw_ibus_ma,
        ui_vbus_mv: None,
        ui_ibus_ma: None,
        raw_power_w10,
        issue: None,
    };

    if !input_present {
        return sample;
    }

    if !adc_ready {
        sample.issue = Some(ChargerInputSampleIssue::AdcNotReady);
        return sample;
    }

    let vbus_mv = match raw_vbus_mv {
        Some(vbus_mv) if vbus_mv <= CHARGER_INPUT_VBUS_MAX_MV => vbus_mv,
        Some(_) => {
            sample.issue = Some(ChargerInputSampleIssue::VbusOutOfRange);
            return sample;
        }
        None => {
            sample.issue = Some(ChargerInputSampleIssue::VbusMissing);
            return sample;
        }
    };

    let ibus_ma = match raw_ibus_ma {
        Some(ibus_ma)
            if ibus_ma >= -CHARGER_INPUT_IBUS_MAX_MA && ibus_ma <= CHARGER_INPUT_IBUS_MAX_MA =>
        {
            ibus_ma
        }
        Some(_) => {
            sample.issue = Some(ChargerInputSampleIssue::IbusOutOfRange);
            return sample;
        }
        None => {
            sample.issue = Some(ChargerInputSampleIssue::IbusMissing);
            return sample;
        }
    };

    sample.ui_vbus_mv = Some(vbus_mv);
    sample.ui_ibus_ma = Some(if ibus_ma <= 0 { 0 } else { i32::from(ibus_ma) });
    sample
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct BmsAudioState {
    rca_alarm: Option<bool>,
    protection_active: bool,
    module_fault: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct TpsAudioState {
    out_a_over_voltage: bool,
    out_b_over_voltage: bool,
    out_a_over_current: bool,
    out_b_over_current: bool,
}

impl TpsAudioState {
    const fn any_over_voltage(self) -> bool {
        self.out_a_over_voltage || self.out_b_over_voltage
    }

    const fn any_over_current(self) -> bool {
        self.out_a_over_current || self.out_b_over_current
    }
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
        let mut i2c = i2c;
        let now = Instant::now();
        let (
            manual_charge_prefs,
            manual_charge_prefs_offset,
            manual_charge_storage_ready,
            manual_charge_storage_incompatible,
        ) = load_or_init_manual_charge_prefs(&mut i2c);
        let mut initial_ui_snapshot = cfg.self_check_snapshot;
        initial_ui_snapshot.dashboard_detail.manual_charge.prefs = manual_charge_prefs;
        initial_ui_snapshot.dashboard_detail.manual_charge.runtime =
            ManualChargeRuntimeState::idle();
        let output_state = OutputRuntimeState::new(
            cfg.requested_outputs,
            cfg.active_outputs,
            cfg.recoverable_outputs,
            cfg.output_gate_reason,
        );
        let outputs_allowed = output_state.requested_outputs != EnabledOutputs::None;
        let out_a_allowed = output_state.active_outputs.is_enabled(OutputChannel::OutA);
        let out_b_allowed = output_state.active_outputs.is_enabled(OutputChannel::OutB);
        let charger_allowed = cfg.charger_probe_ok;
        let bms_addr = cfg.bms_addr;
        let bms_auto_recovery_enabled =
            boot_diag_auto_recovery_enabled(cfg.bms_boot_diag_auto_validate);
        let bms_runtime_seen = bms_addr.is_some()
            || output_state.gate_reason == OutputGateReason::BmsNotReady
            || (cfg.charger_probe_ok
                && matches!(cfg.self_check_snapshot.bq40z50, SelfCheckCommState::Err));

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
            next_vin_telemetry_skip_at: now,
            next_fan_temp_refresh_at: now,
            last_fault_log_at: None,
            last_input_power_anomaly_log_at: None,
            last_therm_kill_hint_at: None,
            tps_a_fault_latch: TpsFaultLatch::default(),
            tps_b_fault_latch: TpsFaultLatch::default(),
            fan_started_at: now,
            fan_rpm_tracker: FanRpmTracker::new(),
            applied_fan_state: AppliedFanState {
                command: fan::FanLevel::Off,
                pwm_pct: 0,
                vset_duty_pct: 0,
                degraded: false,
                disabled_by_feature: cfg.tmp_hw_protect_test_mode,
            },

            ina_ready: false,
            ina_next_retry_at: if outputs_allowed { Some(now) } else { None },

            tps_a_ready: false,
            tps_a_next_retry_at: if out_a_allowed { Some(now) } else { None },
            tps_b_ready: false,
            tps_b_next_retry_at: if out_b_allowed { Some(now) } else { None },

            bms_addr,
            bms_runtime_seen,
            bms_next_poll_at: now,
            bms_next_retry_at: Some(now),
            bms_last_int_poll_at: None,
            bms_poll_seq: 0,
            bms_ok_streak: 0,
            bms_err_streak: 0,
            bms_cached_da_status2: None,
            bms_cached_filter_capacity: None,
            bms_cached_balance_config: None,
            bms_cached_gauging_status: None,
            bms_cached_lock_diag: None,
            bms_next_da_status2_refresh_at: now,
            bms_next_filter_capacity_refresh_at: now + BMS_DETAIL_MAC_REFRESH_STAGGER,
            bms_next_balance_config_refresh_at: now + BMS_DETAIL_BALANCE_CONFIG_REFRESH_STAGGER,
            bms_next_gauging_status_refresh_at: now + BMS_DETAIL_GAUGING_STATUS_REFRESH_STAGGER,
            bms_next_lock_diag_refresh_at: now + BMS_DETAIL_LOCK_DIAG_REFRESH_STAGGER,
            bms_next_block_detail_log_at: now,
            bms_config_logged: false,
            bms_next_config_log_at: now,
            out_a_next_path_diag_log_at: now,
            out_b_next_path_diag_log_at: now,

            chg_next_poll_at: now,
            chg_next_retry_at: if charger_allowed { Some(now) } else { None },
            chg_enabled: false,
            charger_allowed,
            charge_policy: ChargePolicyMemory::default(),
            charge_policy_derate: ChargePolicyDerateTracker::default(),
            charge_policy_output_load: ChargePolicyOutputLoadTracker::default(),
            manual_charge_prefs,
            manual_charge_prefs_offset,
            manual_charge_storage_ready,
            manual_charge_storage_incompatible,
            manual_charge_runtime: ManualChargeRuntime::new(),
            bms_charge_ready: None,
            bms_full: None,
            bms_cell_min_mv: None,
            chg_last_int_poll_at: None,
            bms_activation_state: BmsActivationState::Idle,
            bms_activation_phase: BmsActivationPhase::WakeProbe,
            bms_activation_started_at: None,
            bms_activation_deadline: None,
            bms_activation_diag_stage: 0,
            bms_activation_followup_next_at: None,
            bms_activation_followup_attempts: 0,
            bms_activation_exercise_next_at: None,
            bms_activation_pattern_tracker: Bq40ActivationPatternTracker::new(),
            bms_activation_isolation_until: None,
            bms_activation_force_charge_requested: false,
            bms_boot_diag_started_at: now,
            bms_boot_diag_ship_reset_attempted: false,
            bms_activation_auto_due_at: if bms_auto_recovery_enabled {
                now + BMS_ACTIVATION_AUTO_DELAY
            } else {
                now
            },
            bms_activation_auto_poll_release_at: if bms_auto_recovery_enabled {
                now + BMS_ACTIVATION_AUTO_POLL_RELEASE_DELAY
            } else {
                now
            },
            bms_activation_auto_attempted: !bms_auto_recovery_enabled,
            bms_activation_current_is_auto: false,
            bms_activation_request_kind: BmsRecoveryRequestKind::Activation,
            bms_activation_auto_force_charge_until: if bms_auto_recovery_enabled
                && BMS_ACTIVATION_AUTO_BOOT_FORCE_CHARGE
            {
                Some(
                    now + BMS_ACTIVATION_AUTO_DELAY
                        + if BMS_BOOT_DIAG_TOOL_STYLE_PROBE_ONLY {
                            BMS_BOOT_DIAG_TOOL_STYLE_FORCE_HOLD
                        } else {
                            Duration::ZERO
                        },
                )
            } else {
                None
            },
            bms_activation_auto_force_charge_programmed: false,
            bms_activation_auto_defer_logged: false,
            bms_activation_backup: None,
            chg_watchdog_restore: None,
            output_state,
            output_protection: output_protection::ProtectionRuntime::new(cfg.ilimit_ma),
            fan: fan::Controller::new(cfg.fan_config),
            vin_sample_missing_streak: 0,
            usb_pd_state: usb_pd::UsbPdPortState::default(),
            usb_pd_input_current_limit_ma: None,
            usb_pd_vindpm_mv: None,
            usb_pd_vac1_mv: None,
            usb_pd_input_limit_backup: None,
            usb_pd_restore_input_limits_pending: false,
            ui_snapshot: initial_ui_snapshot,
            audio_snapshot: AudioSignalSnapshot::default(),
            audio_events: AudioSignalEvents::default(),
            audio_signals_ready: false,
            charger_audio: ChargerAudioState::default(),
            bms_audio: BmsAudioState::default(),
            tps_audio: TpsAudioState::default(),
        }
    }

    pub fn init_best_effort(&mut self) {
        let _ = bms_diag_breadcrumb_take();
        if self.output_state.requested_outputs != EnabledOutputs::None {
            self.try_init_ina();
            self.try_configure_requested_tps();
        } else {
            defmt::warn!("power: outputs disabled (boot self-test)");
            self.force_disable_outputs();
        }

        if !self.charger_allowed {
            defmt::warn!("charger: bq25792 disabled (boot self-test)");
            self.chg_ce.set_high();
            self.chg_enabled = false;
        }

        if self.bms_addr.is_none() {
            defmt::warn!("bms: bq40z50 disabled (boot self-test)");
        }
        if self.output_state.gate_reason != OutputGateReason::None {
            defmt::warn!(
                "power: outputs gated reason={} (boot self-test)",
                self.output_state.gate_reason.as_str()
            );
        }

        if self.ui_snapshot.bq25792_allow_charge.is_none() {
            self.ui_snapshot.bq25792_allow_charge =
                Some(self.cfg.charger_enabled && self.charger_allowed);
        }
        if self.ui_snapshot.tps_a_enabled.is_none() {
            self.ui_snapshot.tps_a_enabled = Some(
                self.output_state
                    .active_outputs
                    .is_enabled(OutputChannel::OutA),
            );
        }
        if self.ui_snapshot.tps_b_enabled.is_none() {
            self.ui_snapshot.tps_b_enabled = Some(
                self.output_state
                    .active_outputs
                    .is_enabled(OutputChannel::OutB),
            );
        }
        self.charger_audio.input_present = self.ui_snapshot.fusb302_vbus_present;
        self.charger_audio.phase = self.cfg.initial_audio_charge_phase;
        self.charger_audio.module_fault =
            matches!(self.ui_snapshot.bq25792, SelfCheckCommState::Err);
        self.bms_audio.rca_alarm = self.ui_snapshot.bq40z50_rca_alarm;
        self.bms_audio.protection_active = self.cfg.initial_bms_protection_active;
        self.bms_audio.module_fault = matches!(self.ui_snapshot.bq40z50, SelfCheckCommState::Err);
        self.tps_audio.out_a_over_voltage = self.cfg.initial_tps_a_over_voltage;
        self.tps_audio.out_b_over_voltage = self.cfg.initial_tps_b_over_voltage;
        self.tps_audio.out_a_over_current = self.cfg.initial_tps_a_over_current;
        self.tps_audio.out_b_over_current = self.cfg.initial_tps_b_over_current;
        self.update_manual_charge_ui_snapshot(Instant::now());
        self.recompute_ui_mode();
        self.refresh_audio_signals();
    }

    fn force_disable_outputs(&mut self) {
        self.output_state.active_outputs = EnabledOutputs::None;
        self.tps_a_ready = false;
        self.tps_b_ready = false;
        self.tps_a_next_retry_at = None;
        self.tps_b_next_retry_at = None;
        self.ui_snapshot.tps_a_enabled = Some(false);
        self.ui_snapshot.tps_b_enabled = Some(false);
        self.ui_snapshot.out_a_vbus_mv = None;
        self.ui_snapshot.out_b_vbus_mv = None;
        self.ui_snapshot.tps_a_iout_ma = None;
        self.ui_snapshot.tps_b_iout_ma = None;

        let out_a = ::tps55288::Tps55288::with_address(&mut self.i2c, OutputChannel::OutA.addr())
            .disable_output()
            .map_err(tps_error_kind);
        let out_b = ::tps55288::Tps55288::with_address(&mut self.i2c, OutputChannel::OutB.addr())
            .disable_output()
            .map_err(tps_error_kind);

        defmt::info!(
            "power: force_disable_outputs out_a={=?} out_b={=?}",
            out_a,
            out_b
        );
        self.recompute_ui_mode();
    }

    fn maybe_log_charger_input_power_anomaly(
        &mut self,
        now: Instant,
        sample: ChargerInputPowerSample,
        adc_state: Option<bq25792::AdcState>,
        adc_ready: bool,
        status0: u8,
        status1: u8,
        status3: u8,
    ) {
        if sample.raw_power_w10.unwrap_or(0) <= CHARGER_INPUT_POWER_ANOMALY_W10 {
            return;
        }

        if !tps55288::should_log_fault(
            now,
            &mut self.last_input_power_anomaly_log_at,
            self.cfg.fault_log_min_interval,
        ) {
            return;
        }

        let adc_ctrl = adc_state.map(|state| state.ctrl).unwrap_or(0);
        defmt::warn!(
            "charger: input_power_anomaly raw_pin_w10={=u32} raw_ibus_adc_ma={=?} raw_vbus_adc_mv={=?} ui_ibus_ma={=?} ui_vbus_mv={=?} reason={} adc_ready={=bool} adc_ctrl=0x{=u8:x} adc_done={=bool} vbus_stat={} vbus_present={=bool} ac1_present={=bool} ac2_present={=bool} pg={=bool}",
            sample.raw_power_w10.unwrap_or(0),
            sample.raw_ibus_ma,
            sample.raw_vbus_mv,
            sample.ui_ibus_ma,
            sample.ui_vbus_mv,
            sample
                .issue
                .map(ChargerInputSampleIssue::as_str)
                .unwrap_or("none"),
            adc_ready,
            adc_ctrl,
            (status3 & bq25792::status3::ADC_DONE_STAT) != 0,
            bq25792::decode_vbus_stat(bq25792::status1::vbus_stat(status1)),
            (status0 & bq25792::status0::VBUS_PRESENT_STAT) != 0,
            (status0 & bq25792::status0::AC1_PRESENT_STAT) != 0,
            (status0 & bq25792::status0::AC2_PRESENT_STAT) != 0,
            (status0 & bq25792::status0::PG_STAT) != 0,
        );
    }

    pub fn tick(&mut self, irq: &IrqSnapshot) -> bool {
        if let Some(until) = self.bms_activation_isolation_until {
            if Instant::now() < until {
                self.note_skipped_vin_telemetry_if_due(Instant::now());
                self.update_fan_state(irq);
                self.refresh_audio_signals();
                return false;
            }
            self.bms_activation_isolation_until = None;
        }

        let activation_pending = self.bms_activation_state == BmsActivationState::Pending;
        if activation_pending {
            if matches!(
                self.bms_activation_phase,
                BmsActivationPhase::ProbeWithoutCharge
                    | BmsActivationPhase::WaitChargeOff
                    | BmsActivationPhase::WaitMinChargeSettle
                    | BmsActivationPhase::MinChargeProbe
                    | BmsActivationPhase::WakeProbe
            ) {
                self.maybe_poll_charger(irq);
            }
            let mut bms_i2c_active = false;
            if self.bms_activation_phase == BmsActivationPhase::MinChargeProbe {
                // Keep the regular strict snapshot poll alive during the min-charge observe
                // window. The historical passing path recovered here before wake touch logic ran.
                bms_i2c_active |= self.maybe_poll_bms(irq);
            }
            bms_i2c_active |= self.maybe_track_bms_activation();
            if bms_i2c_active {
                self.bms_activation_isolation_until =
                    Some(Instant::now() + BMS_ACTIVATION_ISOLATION_WINDOW);
                self.note_skipped_vin_telemetry_if_due(Instant::now());
                self.update_fan_state(irq);
                self.refresh_audio_signals();
                return false;
            }
            self.note_skipped_vin_telemetry_if_due(Instant::now());
            self.update_fan_state(irq);
            self.refresh_audio_signals();
            return false;
        }

        self.bms_activation_isolation_until = None;
        self.maybe_retry();
        self.maybe_handle_fault(irq);
        self.maybe_poll_charger(irq);
        self.maybe_auto_request_bms_activation();
        self.maybe_auto_request_bms_discharge_authorization();
        if self.bms_activation_state == BmsActivationState::Pending {
            let bms_i2c_active = self.maybe_track_bms_activation();
            if bms_i2c_active {
                self.bms_activation_isolation_until =
                    Some(Instant::now() + BMS_ACTIVATION_ISOLATION_WINDOW);
                self.note_skipped_vin_telemetry_if_due(Instant::now());
                self.update_fan_state(irq);
                self.refresh_audio_signals();
                return false;
            }
            self.note_skipped_vin_telemetry_if_due(Instant::now());
            self.update_fan_state(irq);
            self.refresh_audio_signals();
            return false;
        }
        let mut bms_i2c_active = self.maybe_poll_bms(irq);
        bms_i2c_active |= self.maybe_track_bms_activation();
        if bms_i2c_active {
            self.bms_activation_isolation_until =
                Some(Instant::now() + BMS_ACTIVATION_ISOLATION_WINDOW);
            self.note_skipped_vin_telemetry_if_due(Instant::now());
            self.update_fan_state(irq);
            self.refresh_audio_signals();
            return false;
        }
        if self.bms_activation_state == BmsActivationState::Pending {
            self.note_skipped_vin_telemetry_if_due(Instant::now());
            self.update_fan_state(irq);
            self.refresh_audio_signals();
            return false;
        }
        self.update_output_protection();
        self.reconcile_output_state();
        let telemetry_printed = self.maybe_print_telemetry();
        if telemetry_printed {
            self.update_output_protection();
            self.reconcile_output_state();
        }
        self.update_fan_state(irq);
        self.refresh_audio_signals();
        telemetry_printed
    }

    pub fn ui_snapshot(&self) -> SelfCheckUiSnapshot {
        let mut snapshot = self.ui_snapshot;
        let mut detail = snapshot.dashboard_detail;
        detail.manual_charge.prefs = self.manual_charge_prefs;
        detail.manual_charge.runtime = ManualChargeRuntimeState {
            active: self.manual_charge_runtime.active,
            takeover: self.manual_charge_runtime.takeover,
            stop_inhibit: self.manual_charge_runtime.stop_inhibit,
            last_stop_reason: self.manual_charge_runtime.last_stop_reason,
            remaining_minutes: manual_charge_remaining_minutes(
                self.manual_charge_runtime.deadline,
                Instant::now(),
            ),
        };
        let fan_status = self.fan.status();
        let applied_fan = self.applied_fan_state;
        let bms_recovery_pending = self.bms_activation_state == BmsActivationState::Pending
            && snapshot.bq40z50 != SelfCheckCommState::Err;

        snapshot.requested_outputs =
            logic_outputs_from_enabled(self.output_state.requested_outputs);
        snapshot.active_outputs = logic_outputs_from_enabled(self.output_state.active_outputs);
        snapshot.recoverable_outputs =
            logic_outputs_from_enabled(self.output_state.recoverable_outputs);
        snapshot.output_gate_reason = self.output_state.gate_reason;
        snapshot.bq40z50_recovery_action = bq40_recovery_action_for_snapshot(
            &snapshot,
            snapshot.requested_outputs,
            self.output_state.gate_reason,
            self.bms_addr,
            self.charger_allowed,
            self.therm_kill.is_low(),
        );
        snapshot.bq40z50_recovery_pending = bms_recovery_pending;
        detail.out_a_temp_c = snapshot.tmp_a_c;
        detail.out_b_temp_c = snapshot.tmp_b_c;
        detail.fan_rpm = self.fan_rpm_tracker.display_rpm();
        detail.fan_pwm_pct = Some(applied_fan.pwm_pct);
        detail.fan_status = Some(detail_fan_status_text(applied_fan, fan_status.tach_fault));
        detail.battery_notice = if bms_recovery_pending {
            Some("DISCHARGE AUTHORIZATION IN PROGRESS")
        } else if snapshot.bq40z50_no_battery == Some(true) {
            Some("PACK PRESENT CHECK FAILED")
        } else if snapshot.bq40z50_discharge_ready == Some(false) {
            Some("DISCHARGE PATH LIMITED")
        } else if detail.battery_notice.is_some() {
            detail.battery_notice
        } else {
            Some("LIVE DATA")
        };
        detail.output_notice = if self.output_state.gate_reason == OutputGateReason::BmsNotReady
            && self.output_state.requested_outputs != EnabledOutputs::None
        {
            if bms_recovery_pending {
                Some("WAITING FOR BMS RECOVERY")
            } else {
                Some("OUTPUT HELD BY BMS DISCHARGE POLICY")
            }
        } else if self.output_state.gate_reason == OutputGateReason::TpsConfigFailed {
            Some("OUTPUT HELD BY TPS CONFIG FAILURE")
        } else if self.output_state.gate_reason == OutputGateReason::TpsFault {
            Some("OUTPUT HELD BY TPS FAULT LATCH")
        } else {
            Some("LIVE DATA")
        };
        detail.charger_notice = if self.usb_pd_state.unsafe_source_latched {
            Some("USB-C INPUT UNSAFE")
        } else if snapshot.bq25792 == SelfCheckCommState::Ok
            && snapshot.bq25792_allow_charge == Some(false)
            && snapshot.fusb302_vbus_present == Some(true)
        {
            Some("INPUT READY - BATTERY PATH BLOCKED")
        } else if detail.charger_notice.is_some() {
            detail.charger_notice
        } else {
            Some("LIVE DATA")
        };
        detail.thermal_notice = Some(thermal_notice_text(
            self.therm_kill.is_low(),
            self.cfg.tmp_hw_protect_test_mode,
        ));

        snapshot.dashboard_detail = detail;
        snapshot
    }

    pub fn usb_pd_demand(&self) -> usb_pd::UsbPdPowerDemand {
        let activation_pending = self.bms_activation_state == BmsActivationState::Pending;
        let output_power_w10 =
            charge_policy_output_power_w10(&self.ui_snapshot, self.output_state.active_outputs);
        let output_power_mw = output_power_w10
            .map(|power_w10| power_w10.saturating_mul(100))
            .unwrap_or_else(|| {
                if self.output_state.active_outputs != EnabledOutputs::None {
                    CHARGE_POLICY_OUTPUT_POWER_LIMIT_W10.saturating_mul(100)
                } else {
                    0
                }
            });
        let requested_charge_voltage_mv =
            if activation_pending && self.bms_activation_force_charge_requested {
                BMS_ACTIVATION_FORCE_VREG_MV
            } else {
                CHARGE_POLICY_VREG_MV
            };
        let requested_charge_current_ma =
            if activation_pending && self.bms_activation_force_charge_requested {
                BMS_ACTIVATION_FORCE_ICHG_MA
            } else {
                self.ui_snapshot
                    .bq25792_ichg_ma
                    .unwrap_or(CHARGE_POLICY_NORMAL_ICHG_MA)
            };

        usb_pd::UsbPdPowerDemand {
            requested_charge_voltage_mv,
            requested_charge_current_ma,
            system_load_power_mw: USB_PD_SYSTEM_LOAD_FLOOR_MW.saturating_add(output_power_mw),
            battery_voltage_mv: self.ui_snapshot.bq40z50_pack_mv,
            // Feed the PD manager the raw charger-side VAC1 sample so FUSB302 VBUS_OK glitches
            // do not blind detach / unsafe-voltage decisions.
            measured_input_voltage_mv: self.usb_pd_vac1_mv,
            charging_enabled: usb_pd_charging_enabled(
                self.ui_snapshot.bq25792_allow_charge,
                self.cfg.charger_enabled,
                self.charger_allowed,
            ),
        }
    }

    pub fn update_usb_pd_state(&mut self, state: usb_pd::UsbPdPortState) {
        let previous_state = self.usb_pd_state;
        if previous_state != state {
            defmt::info!(
                "usb_pd: state attached={=bool} ready={=bool} charge_ready={=bool} vbus_present={=?} contract_mv={=?} contract_ma={=?} vindpm_mv={=?} input_current_limit_ma={=?} unsafe={=bool}",
                state.attached,
                state.controller_ready,
                state.charge_ready,
                state.vbus_present,
                state.contract.map(|contract| contract.voltage_mv),
                state.contract.map(|contract| contract.current_ma),
                state.vindpm_mv,
                state.input_current_limit_ma,
                state.unsafe_source_latched
            );
        }
        self.usb_pd_state = state;
        self.usb_pd_input_current_limit_ma = state.input_current_limit_ma;
        self.usb_pd_vindpm_mv = state.vindpm_mv;
        let previous_pd_limits_present =
            previous_state.input_current_limit_ma.is_some() || previous_state.vindpm_mv.is_some();
        let pd_limits_present = state.input_current_limit_ma.is_some() || state.vindpm_mv.is_some();
        match usb_pd_restore_tracking_update(
            previous_pd_limits_present,
            pd_limits_present,
            state.attached,
            self.usb_pd_input_limit_backup.is_some(),
        ) {
            UsbPdRestoreTrackingUpdate::ArmRestore => {
                self.usb_pd_restore_input_limits_pending = true;
            }
            UsbPdRestoreTrackingUpdate::ClearRestorePending => {
                self.usb_pd_restore_input_limits_pending = false;
            }
            UsbPdRestoreTrackingUpdate::None => {}
        }

        if state.enabled {
            self.ui_snapshot.fusb302 = if !state.controller_ready {
                SelfCheckCommState::Err
            } else if state.unsafe_source_latched {
                SelfCheckCommState::Warn
            } else {
                SelfCheckCommState::Ok
            };
            if state.vbus_present.is_some() {
                self.ui_snapshot.fusb302_vbus_present = state.vbus_present;
            }
        }
        self.recompute_ui_mode();
    }

    #[allow(dead_code)]
    pub fn output_restore_pending(&self) -> bool {
        self.can_request_output_restore()
    }

    #[allow(dead_code)]
    pub fn request_output_restore(&mut self) {
        if !self.can_request_output_restore() {
            defmt::warn!(
                "power: output restore ignored gate_reason={} active_outputs={} recoverable_outputs={} mains_present={=?}",
                self.output_state.gate_reason.as_str(),
                self.output_state.active_outputs.describe(),
                self.output_state.recoverable_outputs.describe(),
                self.current_mains_present()
            );
            return;
        }

        let restore = self.output_state.recoverable_outputs;
        self.output_state.active_outputs = restore;
        let now = Instant::now();
        if restore.is_enabled(OutputChannel::OutA) {
            self.clear_tps_fault_latch(OutputChannel::OutA);
            self.tps_a_next_retry_at = Some(now);
        }
        if restore.is_enabled(OutputChannel::OutB) {
            self.clear_tps_fault_latch(OutputChannel::OutB);
            self.tps_b_next_retry_at = Some(now);
        }
        if !self.ina_ready {
            self.ina_next_retry_at = Some(now);
        }
        self.ui_snapshot.tps_a_enabled = Some(restore.is_enabled(OutputChannel::OutA));
        self.ui_snapshot.tps_b_enabled = Some(restore.is_enabled(OutputChannel::OutB));
        self.recompute_ui_mode();
        defmt::info!(
            "power: output restore requested outputs={}",
            restore.describe()
        );
    }

    pub fn fan_command(&self) -> fan::Status {
        self.fan.status()
    }

    pub fn set_applied_fan_state(&mut self, applied: AppliedFanState) {
        self.applied_fan_state = applied;
    }

    fn fan_now_ms(&self) -> u64 {
        self.fan_started_at.elapsed().as_millis()
    }

    fn fan_temps_ready(&self) -> bool {
        self.ui_snapshot.tmp_a != SelfCheckCommState::Pending
            || self.ui_snapshot.tmp_b != SelfCheckCommState::Pending
    }

    fn bms_thermal_ready(&self) -> bool {
        self.ui_snapshot.bq40z50 != SelfCheckCommState::Pending
    }

    fn shared_bms_thermal_max_c_x16(&self) -> Option<i16> {
        bms_thermal_max_c_x16(&self.ui_snapshot)
    }

    fn thermal_control_inputs_ready(&self) -> bool {
        self.fan_temps_ready() || self.bms_thermal_ready()
    }

    fn clear_bms_detail_snapshot(&mut self) {
        let detail = &mut self.ui_snapshot.dashboard_detail;
        detail.cell_mv = [None, None, None, None];
        detail.cell_temp_c = [None, None, None, None];
        detail.remcap_mah = None;
        detail.fcc_mah = None;
        detail.balance_enabled = None;
        detail.balance_cfg_match = None;
        detail.balance_active = None;
        detail.balance_mask = None;
        detail.balance_cell = None;
        detail.battery_energy_mwh = None;
        detail.battery_full_capacity_mwh = None;
        detail.charge_ready = None;
        detail.discharge_ready = None;
        detail.xchg = None;
        detail.xdsg = None;
        detail.charge_fet_on = None;
        detail.discharge_fet_on = None;
        detail.precharge_fet_on = None;
        detail.learn_qen = None;
        detail.learn_vok = None;
        detail.learn_rest = None;
        detail.fc = None;
        detail.fd = None;
        detail.pf = None;
        detail.rca_alarm = None;
        detail.reason_key = None;
        detail.reason_label = None;
        detail.board_temp_c = None;
        detail.battery_temp_c = None;
        detail.cells_notice = None;
        detail.battery_notice = None;
        detail.bms_notice = None;
    }

    fn clear_bms_charge_policy_inputs(&mut self) {
        self.bms_charge_ready = None;
        self.bms_full = None;
        self.bms_cell_min_mv = None;
        self.charge_policy.charge_latched = false;
        self.charge_policy_derate.reset();
        self.charge_policy_output_load.reset();
    }

    fn reset_bms_detail_mac_cache(&mut self, now: Instant) {
        self.bms_cached_da_status2 = None;
        self.bms_cached_filter_capacity = None;
        self.bms_cached_balance_config = None;
        self.bms_cached_gauging_status = None;
        self.bms_cached_lock_diag = None;
        self.bms_next_da_status2_refresh_at = now;
        self.bms_next_filter_capacity_refresh_at = now + BMS_DETAIL_MAC_REFRESH_STAGGER;
        self.bms_next_balance_config_refresh_at = now + BMS_DETAIL_BALANCE_CONFIG_REFRESH_STAGGER;
        self.bms_next_gauging_status_refresh_at = now + BMS_DETAIL_GAUGING_STATUS_REFRESH_STAGGER;
        self.bms_next_lock_diag_refresh_at = now + BMS_DETAIL_LOCK_DIAG_REFRESH_STAGGER;
    }

    fn apply_bms_detail_snapshot(&mut self, snapshot: &Bq40z50Snapshot) {
        let detail = &mut self.ui_snapshot.dashboard_detail;
        let balance_mask = detail_bms_balance_mask(snapshot);
        let balance_config = snapshot.balance_config;
        let balance_cfg_match = balance_config.map(bq40_balance_config_matches_mainboard);
        let (charge_ready, charge_reason) = bq40_decode_charge_path(snapshot.op_status);
        let (discharge_ready, discharge_reason) = bq40_decode_discharge_path(snapshot.op_status);
        let primary_reason = bq40_primary_reason(
            snapshot.batt_status,
            snapshot.op_status,
            charge_reason,
            discharge_reason,
        );
        detail.cell_mv = snapshot.cell_mv.map(Some);
        detail.cell_temp_c = detail_bms_cell_sensor_temps(snapshot);
        detail.remcap_mah = Some(snapshot.remcap);
        detail.fcc_mah = Some(snapshot.fcc);
        detail.balance_enabled = balance_config.map(|config| config.cb());
        detail.balance_cfg_match = balance_cfg_match;
        detail.balance_active = bq40_op_bit(snapshot.op_status, bq40z50::operation_status::CB);
        detail.balance_mask = balance_mask;
        detail.balance_cell = detail_bms_single_balance_cell(balance_mask);
        detail.battery_energy_mwh = detail_bms_energy_mwh(snapshot);
        detail.battery_full_capacity_mwh = detail_bms_full_capacity_mwh(snapshot);
        detail.charge_ready = charge_ready;
        detail.discharge_ready = discharge_ready;
        detail.xchg = bq40_op_bit(snapshot.op_status, bq40z50::operation_status::XCHG);
        detail.xdsg = bq40_op_bit(snapshot.op_status, bq40z50::operation_status::XDSG);
        detail.charge_fet_on = bq40_op_bit(snapshot.op_status, bq40z50::operation_status::CHG);
        detail.discharge_fet_on = bq40_op_bit(snapshot.op_status, bq40z50::operation_status::DSG);
        detail.precharge_fet_on = bq40_op_bit(snapshot.op_status, bq40z50::operation_status::PCHG);
        detail.learn_qen = bq40_mac_bit(snapshot.gauging_status, bq40z50::gauging_status::QEN);
        detail.learn_vok = bq40_mac_bit(snapshot.gauging_status, bq40z50::gauging_status::VOK);
        detail.learn_rest = bq40_mac_bit(snapshot.gauging_status, bq40z50::gauging_status::REST);
        detail.fc = bms_detail_gauging_flag(snapshot.gauging_status, bq40z50::gauging_status::FC);
        detail.fd = bms_detail_gauging_flag(snapshot.gauging_status, bq40z50::gauging_status::FD);
        detail.pf = bq40_op_bit(snapshot.op_status, bq40z50::operation_status::PF);
        detail.rca_alarm = Some((snapshot.batt_status & bq40z50::battery_status::RCA) != 0);
        detail.reason_key = Some(primary_reason);
        detail.reason_label = Some(detail_bms_reason_label(primary_reason));
        detail.board_temp_c = detail_bms_board_temp_c(snapshot);
        detail.battery_temp_c = detail_battery_temp_c(snapshot);
        detail.cells_notice = match balance_cfg_match {
            Some(true) => Some("EXT CHG+RELAX"),
            Some(false) => Some("CFG MISMATCH"),
            None => Some("BAL CFG PENDING"),
        };
        detail.battery_notice = Some("LIVE DATA");
        detail.bms_notice = Some("LIVE DATA");
    }

    fn clear_charger_detail_snapshot(&mut self) {
        let detail = &mut self.ui_snapshot.dashboard_detail;
        detail.input_source = None;
        detail.charger_active = None;
        detail.charger_home_status = None;
        detail.charger_status = None;
        detail.charger_notice = None;
    }

    fn refresh_tmp112_snapshot(&mut self, ch: OutputChannel) {
        let temp_c_x16 = tmp112::read_temp_c_x16(&mut self.i2c, ch.tmp_addr()).ok();
        match ch {
            OutputChannel::OutA => {
                self.ui_snapshot.tmp_a = if temp_c_x16.is_some() {
                    SelfCheckCommState::Ok
                } else {
                    SelfCheckCommState::Err
                };
                self.ui_snapshot.tmp_a_c_x16 = temp_c_x16;
                self.ui_snapshot.tmp_a_c = temp_c_x16.map(|v| v / 16);
            }
            OutputChannel::OutB => {
                self.ui_snapshot.tmp_b = if temp_c_x16.is_some() {
                    SelfCheckCommState::Ok
                } else {
                    SelfCheckCommState::Err
                };
                self.ui_snapshot.tmp_b_c_x16 = temp_c_x16;
                self.ui_snapshot.tmp_b_c = temp_c_x16.map(|v| v / 16);
            }
        }
    }

    fn refresh_fan_temp_snapshots_if_due(&mut self) {
        let now = Instant::now();
        if matches!(self.bms_activation_isolation_until, Some(until) if now < until) {
            return;
        }
        if now < self.next_fan_temp_refresh_at {
            return;
        }
        self.next_fan_temp_refresh_at = now + self.cfg.telemetry_period;
        self.refresh_tmp112_snapshot(OutputChannel::OutA);
        self.refresh_tmp112_snapshot(OutputChannel::OutB);
    }

    fn update_fan_state(&mut self, irq: &IrqSnapshot) {
        self.refresh_fan_temp_snapshots_if_due();
        let prev = self.fan.status();
        let now_ms = self.fan_now_ms();
        let bms_temp_c_x16 = self.shared_bms_thermal_max_c_x16();
        let (status, events) = self.fan.update(fan::Input {
            now_ms,
            temps_ready: self.thermal_control_inputs_ready(),
            force_high: self.charger_audio.ts_warm
                || self.charger_audio.ts_hot
                || self.charger_audio.treg,
            temp_a_c_x16: self.ui_snapshot.tmp_a_c_x16,
            temp_b_c_x16: self.ui_snapshot.tmp_b_c_x16,
            temp_bms_c_x16: bms_temp_c_x16,
            tach_pulse_count: irq.fan_tach,
        });
        let rpm = self
            .fan_rpm_tracker
            .observe(now_ms, irq.fan_tach, status, self.cfg.fan_config);
        let raw_rpm = self.fan_rpm_tracker.raw_rpm();
        let pwm_pct = status.pwm_pct;

        if events.temp_source_changed {
            match status.temp_source {
                fan::TempSource::Pending => {}
                fan::TempSource::Missing => {
                    defmt::warn!(
                        "fan: temp_source missing fallback=full_speed control_temp_c_x16={=?}",
                        status.control_temp_c_x16
                    );
                }
                fan::TempSource::ChargerThermal => {
                    defmt::warn!(
                        "fan: temp_source charger_thermal fallback=full_speed control_temp_c_x16={=?} ts_warm={=bool} ts_hot={=bool} treg={=bool}",
                        status.control_temp_c_x16,
                        self.charger_audio.ts_warm,
                        self.charger_audio.ts_hot,
                        self.charger_audio.treg
                    );
                }
                fan::TempSource::TmpA | fan::TempSource::TmpB | fan::TempSource::Bms => {
                    defmt::warn!(
                        "fan: temp_source degraded source={} control_temp_c_x16={=?}",
                        status.temp_source.as_str(),
                        status.control_temp_c_x16
                    );
                }
                fan::TempSource::Max => {
                    if prev.temp_source.is_degraded() {
                        defmt::info!(
                            "fan: temp_source restored source={} control_temp_c_x16={=?}",
                            status.temp_source.as_str(),
                            status.control_temp_c_x16
                        );
                    }
                }
            }
        }

        if events.output_changed {
            defmt::info!(
                "fan: command mode={} pwm_pct={=u8} rpm={=?} temp_source={} control_temp_c_x16={=?} closed_loop={=bool} tach_fault={=bool}",
                status.command.as_str(),
                pwm_pct,
                rpm,
                status.temp_source.as_str(),
                status.control_temp_c_x16,
                self.cfg.fan_control_enabled,
                status.tach_fault
            );
        }

        if events.tach_fault_changed {
            if status.tach_fault {
                defmt::warn!(
                    "fan: tach_timeout mode={} pwm_pct={=u8} rpm={=?} temp_source={} control_temp_c_x16={=?} timeout_ms={=u64}",
                    status.command.as_str(),
                    pwm_pct,
                    rpm,
                    status.temp_source.as_str(),
                    status.control_temp_c_x16,
                    self.cfg.fan_config.tach_timeout_ms
                );
            } else {
                defmt::info!(
                    "fan: tach_recovered mode={} pwm_pct={=u8} rpm={=?} temp_source={} control_temp_c_x16={=?}",
                    status.command.as_str(),
                    pwm_pct,
                    rpm,
                    status.temp_source.as_str(),
                    status.control_temp_c_x16
                );
            }
        }

        if raw_rpm != rpm && events.output_changed {
            defmt::debug!(
                "fan: command_rpm smoothing rpm_display={=?} rpm_raw={=?}",
                rpm,
                raw_rpm
            );
        }
    }

    pub fn log_fan_telemetry(&self, applied: AppliedFanState) {
        let status = self.fan.status();
        defmt::info!(
            "fan: telemetry requested_mode={} requested_pwm_pct={=u8} applied_mode={} applied_pwm_pct={=u8} applied_vset_duty_pct={=u8} rpm={=?} rpm_raw={=?} output_degraded={=bool} temp_source={} control_temp_c_x16={=?} tach_recent={=bool} tach_fault={=bool} disabled_by_feature={=bool}",
            status.requested_command.as_str(),
            status.requested_pwm_pct,
            applied.command.as_str(),
            applied.pwm_pct,
            applied.vset_duty_pct,
            self.fan_rpm_tracker.display_rpm(),
            self.fan_rpm_tracker.raw_rpm(),
            applied.degraded,
            status.temp_source.as_str(),
            status.control_temp_c_x16,
            status.tach_pulse_seen_recently,
            status.tach_fault,
            applied.disabled_by_feature
        );
    }

    pub fn bms_activation_state(&self) -> BmsActivationState {
        self.bms_activation_state
    }

    pub fn audio_signals(&self) -> AudioSignalSnapshot {
        self.audio_snapshot
    }

    pub fn take_audio_edges(&mut self) -> AudioSignalEvents {
        let events = self.audio_events;
        self.audio_events = AudioSignalEvents::default();
        events
    }

    pub fn clear_bms_activation_state(&mut self) {
        if self.bms_activation_state != BmsActivationState::Pending {
            defmt::info!(
                "bms: activation clear state={} keep_last_result={}",
                match self.bms_activation_state {
                    BmsActivationState::Idle => "idle",
                    BmsActivationState::Pending => "pending",
                    BmsActivationState::Result(result) => bms_result_name(result),
                },
                bms_result_option_name(self.ui_snapshot.bq40z50_last_result)
            );
            self.bms_activation_state = BmsActivationState::Idle;
        }
    }

    fn can_request_bms_discharge_authorization(&self) -> bool {
        self.output_state.requested_outputs != EnabledOutputs::None
            && self.output_state.gate_reason == OutputGateReason::BmsNotReady
            && self.bms_addr.is_some()
            && self.ui_snapshot.bq40z50 != SelfCheckCommState::Err
            && self.ui_snapshot.bq40z50_no_battery != Some(true)
            && self.ui_snapshot.bq40z50_rca_alarm != Some(true)
            && self.ui_snapshot.bq40z50_discharge_ready == Some(false)
            && discharge_authorization_input_ready(
                self.current_mains_present(),
                self.ui_snapshot.fusb302_vbus_present,
            )
            && self.charger_allowed
            && self.ui_snapshot.bq25792 != SelfCheckCommState::Err
            && !self.therm_kill.is_low()
    }

    fn request_bms_discharge_authorization(&mut self, auto_request: bool) {
        if !self.can_request_bms_discharge_authorization() {
            defmt::info!(
                "bms: discharge_authorization ignored reason=not_allowed requested_outputs={} gate_reason={} bms_state={} dsg_ready={=?} no_battery={=?} rca_alarm={=?} charger_state={} input_present={=?} mains_present={=?} therm_kill_asserted={=bool}",
                self.output_state.requested_outputs.describe(),
                self.output_state.gate_reason.as_str(),
                self_check_comm_state_name(self.ui_snapshot.bq40z50),
                self.ui_snapshot.bq40z50_discharge_ready,
                self.ui_snapshot.bq40z50_no_battery,
                self.ui_snapshot.bq40z50_rca_alarm,
                self_check_comm_state_name(self.ui_snapshot.bq25792),
                self.ui_snapshot.fusb302_vbus_present,
                self.current_mains_present(),
                self.therm_kill.is_low()
            );
            return;
        }

        defmt::info!(
            "bms: discharge_authorization requested requested_outputs={} dsg_ready={=?} charger_state={} input_present={=?} mains_present={=?} auto_request={=bool}",
            self.output_state.requested_outputs.describe(),
            self.ui_snapshot.bq40z50_discharge_ready,
            self_check_comm_state_name(self.ui_snapshot.bq25792),
            self.ui_snapshot.fusb302_vbus_present,
            self.current_mains_present(),
            auto_request
        );
        self.request_bms_recovery(
            BmsRecoveryRequestKind::DischargeAuthorization,
            true,
            auto_request,
        );
    }

    pub fn request_bms_activation(&mut self) {
        self.request_bms_recovery(BmsRecoveryRequestKind::Activation, false, false);
    }

    pub fn request_bms_recovery_action(&mut self, action: BmsRecoveryUiAction) {
        match action {
            BmsRecoveryUiAction::Activation => self.request_bms_activation(),
            BmsRecoveryUiAction::DischargeAuthorization => {
                self.request_bms_discharge_authorization(false)
            }
        }
    }

    pub fn request_manual_charge_action(&mut self, action: ManualChargeUiAction) {
        let now = Instant::now();
        let charging_active = self.current_charging_requested();
        match action {
            ManualChargeUiAction::SetTarget(target) => {
                if charging_active {
                    defmt::info!("manual_charge: ignore set_target reason=locked");
                } else if self.manual_charge_prefs.target != target {
                    self.manual_charge_prefs.target = target;
                    self.persist_manual_charge_prefs();
                }
            }
            ManualChargeUiAction::SetSpeed(speed) => {
                if charging_active {
                    defmt::info!("manual_charge: ignore set_speed reason=locked");
                } else if self.manual_charge_prefs.speed != speed {
                    self.manual_charge_prefs.speed = speed;
                    self.persist_manual_charge_prefs();
                }
            }
            ManualChargeUiAction::SetTimerLimit(timer_limit) => {
                if charging_active {
                    defmt::info!("manual_charge: ignore set_timer reason=locked");
                } else if self.manual_charge_prefs.timer_limit != timer_limit {
                    self.manual_charge_prefs.timer_limit = timer_limit;
                    self.persist_manual_charge_prefs();
                }
            }
            ManualChargeUiAction::Start => {
                self.manual_charge_runtime.active = true;
                self.manual_charge_runtime.takeover = charging_active;
                self.manual_charge_runtime.stop_inhibit = false;
                self.manual_charge_runtime.last_stop_reason = ManualChargeStopReason::None;
                self.manual_charge_runtime.deadline =
                    Some(now + manual_charge_timer_duration(self.manual_charge_prefs.timer_limit));
                self.chg_next_poll_at = now;
                defmt::info!(
                    "manual_charge: start target={} speed_ma={=u16} timer_h={=u8} takeover={=bool}",
                    self.manual_charge_target_label(),
                    self.manual_charge_prefs.speed.ichg_ma(),
                    self.manual_charge_prefs.timer_limit.hours(),
                    self.manual_charge_runtime.takeover
                );
            }
            ManualChargeUiAction::Stop => {
                self.bms_activation_force_charge_requested = false;
                self.bms_activation_auto_force_charge_until = None;
                self.bms_activation_auto_force_charge_programmed = false;
                self.stop_manual_charge_session(ManualChargeStopReason::UserStop, true);
                self.chg_next_poll_at = now;
                defmt::info!("manual_charge: stop user_requested={=bool}", true);
            }
        }
        self.update_manual_charge_ui_snapshot(now);
    }

    fn current_charging_requested(&self) -> bool {
        self.ui_snapshot.dashboard_detail.charger_active == Some(true)
            || self.ui_snapshot.bq25792_allow_charge == Some(true)
            || self.manual_charge_runtime.active
    }

    fn persist_manual_charge_prefs(&mut self) {
        if self.manual_charge_storage_incompatible {
            defmt::warn!(
                "eeprom: skip manual_charge prefs save reason=schema_mismatch target={} speed_ma={=u16} timer_h={=u8}",
                self.manual_charge_target_label(),
                self.manual_charge_prefs.speed.ichg_ma(),
                self.manual_charge_prefs.timer_limit.hours()
            );
            return;
        }
        let writing_existing_record = self.manual_charge_storage_ready;
        let write_result = if writing_existing_record {
            write_manual_charge_prefs_record(
                &mut self.i2c,
                self.manual_charge_prefs_offset,
                self.manual_charge_prefs,
            )
        } else {
            write_manual_charge_storage_layout(&mut self.i2c, self.manual_charge_prefs)
        };
        if let Err(err) = write_result {
            defmt::warn!(
                "eeprom: write manual_charge prefs failed err={}",
                i2c_error_kind(err)
            );
        } else {
            self.manual_charge_storage_ready = true;
            if !writing_existing_record {
                self.manual_charge_prefs_offset = EEPROM_MANUAL_PREFS_OFFSET;
            }
            defmt::info!(
                "eeprom: manual_charge prefs saved target={} speed_ma={=u16} timer_h={=u8}",
                self.manual_charge_target_label(),
                self.manual_charge_prefs.speed.ichg_ma(),
                self.manual_charge_prefs.timer_limit.hours()
            );
        }
    }

    fn manual_charge_target_label(&self) -> &'static str {
        self.manual_charge_prefs.target.label()
    }

    fn stop_manual_charge_session(&mut self, reason: ManualChargeStopReason, inhibit: bool) {
        self.manual_charge_runtime.active = false;
        self.manual_charge_runtime.takeover = false;
        self.manual_charge_runtime.stop_inhibit = inhibit;
        self.manual_charge_runtime.last_stop_reason = reason;
        self.manual_charge_runtime.deadline = None;
    }

    fn update_manual_charge_ui_snapshot(&mut self, now: Instant) {
        self.ui_snapshot.dashboard_detail.manual_charge.prefs = self.manual_charge_prefs;
        self.ui_snapshot.dashboard_detail.manual_charge.runtime = ManualChargeRuntimeState {
            active: self.manual_charge_runtime.active,
            takeover: self.manual_charge_runtime.takeover,
            stop_inhibit: self.manual_charge_runtime.stop_inhibit,
            last_stop_reason: self.manual_charge_runtime.last_stop_reason,
            remaining_minutes: manual_charge_remaining_minutes(
                self.manual_charge_runtime.deadline,
                now,
            ),
        };
    }

    fn request_bms_activation_with_diag_override(
        &mut self,
        allow_diag_warn: bool,
        auto_request: bool,
    ) {
        self.request_bms_recovery(
            BmsRecoveryRequestKind::Activation,
            allow_diag_warn,
            auto_request,
        );
    }

    fn request_bms_recovery(
        &mut self,
        request_kind: BmsRecoveryRequestKind,
        allow_diag_warn: bool,
        auto_request: bool,
    ) {
        if self.bms_activation_state == BmsActivationState::Pending {
            defmt::info!(
                "bms: {} ignored reason=already_pending",
                request_kind.request_name()
            );
            return;
        }
        let recovery_needed = match request_kind {
            BmsRecoveryRequestKind::Activation => {
                if allow_diag_warn {
                    self.ui_snapshot.bq40z50_last_result.is_none()
                        && match self.ui_snapshot.bq40z50 {
                            SelfCheckCommState::Err => true,
                            SelfCheckCommState::Warn => !self.has_trusted_bq40_runtime_evidence(),
                            _ => false,
                        }
                } else {
                    is_bq40_activation_needed(&self.ui_snapshot)
                }
            }
            BmsRecoveryRequestKind::DischargeAuthorization => true,
        };
        if !recovery_needed {
            defmt::info!(
                "bms: {} ignored reason=not_needed bq40_state={} trusted_evidence={=bool} dsg_ready={=?} last_result={} diag_override={=bool}",
                request_kind.request_name(),
                self_check_comm_state_name(self.ui_snapshot.bq40z50),
                self.has_trusted_bq40_runtime_evidence(),
                self.ui_snapshot.bq40z50_discharge_ready,
                bms_result_option_name(self.ui_snapshot.bq40z50_last_result),
                allow_diag_warn
            );
            return;
        }
        defmt::info!(
            "bms: {} requested bq40_state={} soc_pct={=?} rca_alarm={=?} dsg_ready={=?} charger_state={} charger_allowed={=bool} vbat_present={=?} input_present={=?} last_result={} diag_override={=bool}",
            request_kind.request_name(),
            self_check_comm_state_name(self.ui_snapshot.bq40z50),
            self.ui_snapshot.bq40z50_soc_pct,
            self.ui_snapshot.bq40z50_rca_alarm,
            self.ui_snapshot.bq40z50_discharge_ready,
            self_check_comm_state_name(self.ui_snapshot.bq25792),
            self.charger_allowed,
            self.ui_snapshot.bq25792_vbat_present,
            self.ui_snapshot.fusb302_vbus_present,
            bms_result_option_name(self.ui_snapshot.bq40z50_last_result),
            allow_diag_warn
        );
        bms_diag_breadcrumb_note(2, allow_diag_warn as u8);
        self.bms_activation_current_is_auto = auto_request;
        self.bms_activation_request_kind = request_kind;
        if !(allow_diag_warn && self.cfg.bms_boot_diag_auto_validate) {
            self.bms_activation_auto_force_charge_until = None;
            self.bms_activation_auto_force_charge_programmed = false;
        }
        self.bms_activation_force_charge_requested = true;
        self.ui_snapshot.bq40z50_last_result = None;
        if !self.charger_allowed {
            self.finish_bms_activation(BmsResultKind::NotDetected, "charger_not_allowed");
            return;
        }

        let status0 = match bq25792::read_u8(&mut self.i2c, bq25792::reg::CHARGER_STATUS_0) {
            Ok(v) => v,
            Err(_) => {
                self.finish_bms_activation(
                    BmsResultKind::NotDetected,
                    "read_charger_status0_failed",
                );
                return;
            }
        };
        defmt::info!(
            "bms: activation request_step=status0_read_ok status0=0x{=u8:x}",
            status0
        );
        let input_present = (status0 & bq25792::status0::VBUS_PRESENT_STAT) != 0
            || (status0 & bq25792::status0::AC1_PRESENT_STAT) != 0
            || (status0 & bq25792::status0::AC2_PRESENT_STAT) != 0
            || (status0 & bq25792::status0::PG_STAT) != 0;
        if !input_present {
            self.finish_bms_activation(BmsResultKind::NotDetected, "input_not_present");
            return;
        }

        let ctrl0 = match bq25792::read_u8(&mut self.i2c, bq25792::reg::CHARGER_CONTROL_0) {
            Ok(v) => v,
            Err(_) => {
                self.finish_bms_activation(BmsResultKind::NotDetected, "read_charger_ctrl0_failed");
                return;
            }
        };
        defmt::info!(
            "bms: activation request_step=ctrl0_read_ok ctrl0=0x{=u8:x}",
            ctrl0
        );
        let vreg_reg = match bq25792::read_u16(&mut self.i2c, bq25792::reg::CHARGE_VOLTAGE_LIMIT) {
            Ok(v) => v,
            Err(_) => {
                self.finish_bms_activation(
                    BmsResultKind::NotDetected,
                    "read_charge_voltage_limit_failed",
                );
                return;
            }
        };
        let ichg_reg = match bq25792::read_u16(&mut self.i2c, bq25792::reg::CHARGE_CURRENT_LIMIT) {
            Ok(v) => v,
            Err(_) => {
                self.finish_bms_activation(
                    BmsResultKind::NotDetected,
                    "read_charge_current_limit_failed",
                );
                return;
            }
        };
        let iindpm_reg = match bq25792::read_u16(&mut self.i2c, bq25792::reg::INPUT_CURRENT_LIMIT) {
            Ok(v) => v,
            Err(_) => {
                self.finish_bms_activation(
                    BmsResultKind::NotDetected,
                    "read_input_current_limit_failed",
                );
                return;
            }
        };
        defmt::info!(
            "bms: activation request_step=limits_read_ok vreg_reg=0x{=u16:x} ichg_reg=0x{=u16:x} iindpm_reg=0x{=u16:x}",
            vreg_reg,
            ichg_reg,
            iindpm_reg
        );
        self.capture_bms_activation_charger_backup(ctrl0, vreg_reg, ichg_reg, iindpm_reg);
        if self
            .maybe_disable_charger_watchdog_for_activation()
            .is_err()
        {
            self.finish_bms_activation(
                BmsResultKind::NotDetected,
                "disable_charger_watchdog_failed",
            );
            return;
        }
        defmt::info!("bms: activation request_step=watchdog_disable_ok");
        self.bms_activation_auto_force_charge_programmed = false;
        self.bms_activation_state = BmsActivationState::Pending;
        defmt::info!("bms: activation request_step=begin_probe_without_charge");
        if let Err(reason) = self.begin_bms_activation_probe_without_charge() {
            self.finish_bms_activation(BmsResultKind::NotDetected, reason);
            return;
        }

        let now = self.bms_activation_started_at.unwrap_or_else(Instant::now);
        let activation_window = if self.bms_activation_force_charge_requested {
            BMS_ACTIVATION_WINDOW
                + BMS_ACTIVATION_MIN_CHARGE_SETTLE
                + BMS_ACTIVATION_MIN_CHARGE_PROBE_WINDOW
        } else {
            BMS_ACTIVATION_WINDOW
        };
        self.bms_activation_deadline = Some(now + activation_window);
        bms_diag_breadcrumb_note(3, self.bms_activation_force_charge_requested as u8);
        defmt::info!(
            "bms: activation start window_ms={=u32} force_min_charge={=bool} phase={} probe_without_charge_window_ms={=u32} probe_without_charge_period_ms={=u32} repower_off_window_ms={=u32} settle_ms={=u32} min_charge_probe_window_ms={=u32} min_charge_direct={=bool} vreg_mv={=u16} ichg_ma={=u16} iindpm_ma={=u16} input_present={=bool}",
            activation_window.as_millis() as u32,
            self.bms_activation_force_charge_requested,
            bms_activation_phase_name(self.bms_activation_phase),
            BMS_ACTIVATION_PROBE_WITHOUT_CHARGE_WINDOW.as_millis() as u32,
            BMS_ACTIVATION_PROBE_WITHOUT_CHARGE_PERIOD.as_millis() as u32,
            BMS_ACTIVATION_REPOWER_OFF_WINDOW.as_millis() as u32,
            BMS_ACTIVATION_MIN_CHARGE_SETTLE.as_millis() as u32,
            BMS_ACTIVATION_MIN_CHARGE_PROBE_WINDOW.as_millis() as u32,
            self.bms_activation_force_charge_requested,
            BMS_ACTIVATION_FORCE_VREG_MV,
            BMS_ACTIVATION_FORCE_ICHG_MA,
            BMS_ACTIVATION_FORCE_IINDPM_MA,
            input_present
        );
    }

    fn maybe_disable_charger_watchdog_for_activation(&mut self) -> Result<(), ()> {
        if self.chg_watchdog_restore.is_some() {
            return Ok(());
        }

        match bq25792::ensure_watchdog_disabled(&mut self.i2c) {
            Ok(state) => {
                if state.watchdog_before != state.watchdog_after {
                    self.chg_watchdog_restore = Some(state.watchdog_before);
                }
                defmt::info!(
                    "bms: activation watchdog stage=disable before=0x{=u8:x} after=0x{=u8:x}",
                    state.watchdog_before,
                    state.watchdog_after
                );
                Ok(())
            }
            Err(_) => Err(()),
        }
    }

    fn maybe_restore_charger_watchdog_after_activation(&mut self) {
        let Some(bits) = self.chg_watchdog_restore else {
            return;
        };

        match bq25792::restore_watchdog(&mut self.i2c, bits) {
            Ok(state) => {
                self.chg_watchdog_restore = None;
                defmt::info!(
                    "bms: activation watchdog stage=restore before=0x{=u8:x} after=0x{=u8:x}",
                    state.watchdog_before,
                    state.watchdog_after
                );
            }
            Err(_) => {
                defmt::info!("bms: activation watchdog stage=restore err=watchdog_restore_failed");
            }
        }
    }

    fn capture_bms_activation_charger_backup(
        &mut self,
        ctrl0: u8,
        vreg_reg: u16,
        ichg_reg: u16,
        iindpm_reg: u16,
    ) {
        if self.bms_activation_backup.is_some() {
            return;
        }
        self.bms_activation_backup = Some(ChargerActivationBackup {
            ctrl0,
            vreg_reg,
            ichg_reg,
            iindpm_reg,
            chg_enabled: self.chg_enabled,
        });
        defmt::info!(
            "bms: activation backup_saved ctrl0=0x{=u8:x} vreg_reg=0x{=u16:x} ichg_reg=0x{=u16:x} iindpm_reg=0x{=u16:x} chg_enabled={=bool}",
            ctrl0,
            vreg_reg,
            ichg_reg,
            iindpm_reg,
            self.chg_enabled
        );
    }

    fn ensure_bms_activation_charger_backup(&mut self) -> Result<(), &'static str> {
        if self.bms_activation_backup.is_some() {
            return Ok(());
        }
        let ctrl0 = bq25792::read_u8(&mut self.i2c, bq25792::reg::CHARGER_CONTROL_0)
            .map_err(|_| "read_charger_ctrl0_backup_failed")?;
        let vreg_reg = bq25792::read_u16(&mut self.i2c, bq25792::reg::CHARGE_VOLTAGE_LIMIT)
            .map_err(|_| "read_charge_voltage_limit_backup_failed")?;
        let ichg_reg = bq25792::read_u16(&mut self.i2c, bq25792::reg::CHARGE_CURRENT_LIMIT)
            .map_err(|_| "read_charge_current_limit_backup_failed")?;
        let iindpm_reg = bq25792::read_u16(&mut self.i2c, bq25792::reg::INPUT_CURRENT_LIMIT)
            .map_err(|_| "read_input_current_limit_backup_failed")?;
        self.capture_bms_activation_charger_backup(ctrl0, vreg_reg, ichg_reg, iindpm_reg);
        Ok(())
    }

    fn restore_bms_activation_charger_backup(&mut self, reason: &'static str) -> Option<bool> {
        let backup = self.bms_activation_backup.take()?;
        let _ = bq25792::write_u16(
            &mut self.i2c,
            bq25792::reg::CHARGE_VOLTAGE_LIMIT,
            backup.vreg_reg,
        );
        let _ = bq25792::write_u16(
            &mut self.i2c,
            bq25792::reg::CHARGE_CURRENT_LIMIT,
            backup.ichg_reg,
        );
        let _ = bq25792::write_u16(
            &mut self.i2c,
            bq25792::reg::INPUT_CURRENT_LIMIT,
            backup.iindpm_reg,
        );
        let _ = bq25792::write_u8(&mut self.i2c, bq25792::reg::CHARGER_CONTROL_0, backup.ctrl0);
        defmt::info!(
            "bms: activation backup_restored reason={} ctrl0=0x{=u8:x} vreg_reg=0x{=u16:x} ichg_reg=0x{=u16:x} iindpm_reg=0x{=u16:x} restore_chg_enabled={=bool}",
            reason,
            backup.ctrl0,
            backup.vreg_reg,
            backup.ichg_reg,
            backup.iindpm_reg,
            backup.chg_enabled
        );
        Some(backup.chg_enabled)
    }

    fn update_bms_activation_auto_due(&mut self, due_at: Instant) {
        let release_margin =
            BMS_ACTIVATION_AUTO_POLL_RELEASE_DELAY.saturating_sub(BMS_ACTIVATION_AUTO_DELAY);
        self.bms_activation_auto_due_at = due_at;
        self.bms_activation_auto_poll_release_at = due_at + release_margin;
        self.bms_activation_auto_defer_logged = false;
        if BMS_ACTIVATION_AUTO_BOOT_FORCE_CHARGE {
            self.bms_activation_auto_force_charge_until = Some(
                due_at
                    + if BMS_BOOT_DIAG_TOOL_STYLE_PROBE_ONLY {
                        BMS_BOOT_DIAG_TOOL_STYLE_FORCE_HOLD
                    } else {
                        Duration::ZERO
                    },
            );
        }
    }

    fn maybe_run_bms_activation_wake_probe(&mut self) -> Option<Bq40ActivationProbeResult> {
        if self.bms_activation_phase != BmsActivationPhase::WakeProbe {
            return None;
        }
        let Some(started_at) = self.bms_activation_started_at else {
            return None;
        };
        let raw_diag = self.bms_activation_current_is_auto;

        while self.bms_activation_diag_stage < BMS_ACTIVATION_DIAG_STAGE_DELAYS_MS.len() {
            let step = self.bms_activation_diag_stage;
            let delay_ms = BMS_ACTIVATION_DIAG_STAGE_DELAYS_MS[step];
            if started_at.elapsed() < Duration::from_millis(delay_ms) {
                break;
            }

            defmt::info!(
                "bms: activation wake_stage step={=u8} delay_ms={=u64} addrs={}",
                step as u8,
                delay_ms,
                BMS_ADDR_LOG
            );
            bms_diag_breadcrumb_note(10, step as u8);

            for addr in bms_probe_candidates().iter().copied() {
                match self.run_bms_activation_wake_probe_step(addr, step as u8, delay_ms, raw_diag)
                {
                    Bq40ActivationProbeResult::Pending => {}
                    result => {
                        bms_diag_breadcrumb_note(11, step as u8);
                        self.bms_activation_diag_stage += 1;
                        return Some(result);
                    }
                }
            }

            defmt::info!(
                "bms: activation wake_stage step={=u8} delay_ms={=u64} result=miss",
                step as u8,
                delay_ms
            );
            bms_diag_breadcrumb_note(12, step as u8);
            self.bms_activation_diag_stage += 1;
        }

        None
    }

    fn maybe_run_bms_activation_probe_without_charge(
        &mut self,
    ) -> Option<Bq40ActivationProbeResult> {
        if self.bms_activation_phase != BmsActivationPhase::ProbeWithoutCharge {
            return None;
        }

        let Some(started_at) = self.bms_activation_started_at else {
            return None;
        };

        let now = Instant::now();
        let next_at = self.bms_activation_followup_next_at.get_or_insert(now);
        if now < *next_at {
            return None;
        }
        *next_at = now + BMS_ACTIVATION_PROBE_WITHOUT_CHARGE_PERIOD;
        self.bms_activation_followup_attempts =
            self.bms_activation_followup_attempts.saturating_add(1);

        let attempt = self.bms_activation_followup_attempts;
        let dwell_ms = started_at.elapsed().as_millis() as u64;
        let raw_diag = self.bms_activation_current_is_auto;
        defmt::info!(
            "bms: activation probe_without_charge attempt={=u16} dwell_ms={=u64} addrs={}",
            attempt,
            dwell_ms,
            BMS_ADDR_LOG
        );

        for addr in bms_probe_candidates().iter().copied() {
            if let Some(snapshot) =
                self.probe_bq40_activation_runtime(addr, attempt, dwell_ms, raw_diag, true)
            {
                return Some(Bq40ActivationProbeResult::Working { addr, snapshot });
            }
        }

        defmt::info!(
            "bms: activation probe_without_charge attempt={=u16} dwell_ms={=u64} result=miss",
            attempt,
            dwell_ms
        );
        None
    }

    fn maybe_run_bms_activation_followup_probe(&mut self) -> Option<Bq40ActivationProbeResult> {
        if self.bms_activation_phase != BmsActivationPhase::WakeProbe
            || self.bms_activation_diag_stage < BMS_ACTIVATION_DIAG_STAGE_DELAYS_MS.len()
        {
            return None;
        }

        let Some(started_at) = self.bms_activation_started_at else {
            return None;
        };

        let now = Instant::now();
        let next_at = self
            .bms_activation_followup_next_at
            .get_or_insert(now + BMS_ACTIVATION_FOLLOWUP_INITIAL_DELAY);
        if now < *next_at {
            return None;
        }
        *next_at = now + BMS_ACTIVATION_FOLLOWUP_PERIOD;
        self.bms_activation_followup_attempts =
            self.bms_activation_followup_attempts.saturating_add(1);

        let attempt = self.bms_activation_followup_attempts;
        let dwell_ms = started_at.elapsed().as_millis() as u64;
        let raw_diag = self.bms_activation_current_is_auto;
        defmt::info!(
            "bms: activation followup attempt={=u16} dwell_ms={=u64} addrs={}",
            attempt,
            dwell_ms,
            BMS_ADDR_LOG
        );

        let exercise_due = dwell_ms <= BMS_ACTIVATION_EXIT_EXERCISE_WINDOW.as_millis() as u64
            && self
                .bms_activation_exercise_next_at
                .map_or(true, |next| now >= next);
        if exercise_due {
            self.bms_activation_exercise_next_at = Some(now + BMS_ACTIVATION_EXIT_EXERCISE_PERIOD);
            for addr in bms_probe_candidates().iter().copied() {
                match self.run_bms_activation_wake_probe_step(
                    addr,
                    attempt.min(u8::MAX as u16) as u8,
                    dwell_ms,
                    raw_diag,
                ) {
                    Bq40ActivationProbeResult::Pending => {}
                    result => return Some(result),
                }
            }

            for addr in bms_probe_candidates().iter().copied() {
                if let Some(snapshot) =
                    self.probe_bq40_activation_runtime(addr, attempt, dwell_ms, raw_diag, false)
                {
                    return Some(Bq40ActivationProbeResult::Working { addr, snapshot });
                }
            }
        }

        for addr in bms_probe_candidates().iter().copied() {
            if let Some(snapshot) =
                self.probe_bq40_activation_runtime(addr, attempt, dwell_ms, raw_diag, false)
            {
                return Some(Bq40ActivationProbeResult::Working { addr, snapshot });
            }
        }

        defmt::info!(
            "bms: activation followup attempt={=u16} dwell_ms={=u64} result=miss",
            attempt,
            dwell_ms
        );
        None
    }

    fn maybe_run_bms_activation_min_charge_probe(&mut self) -> Option<Bq40ActivationProbeResult> {
        if self.bms_activation_phase != BmsActivationPhase::MinChargeProbe {
            return None;
        }

        let Some(started_at) = self.bms_activation_started_at else {
            return None;
        };

        let now = Instant::now();
        let next_at = self.bms_activation_followup_next_at.get_or_insert(now);
        if now < *next_at {
            return None;
        }
        *next_at = now + BMS_ACTIVATION_FOLLOWUP_PERIOD;
        self.bms_activation_followup_attempts =
            self.bms_activation_followup_attempts.saturating_add(1);

        let attempt = self.bms_activation_followup_attempts;
        let dwell_ms = started_at.elapsed().as_millis() as u64;
        defmt::info!(
            "bms: activation min_charge_probe observe attempt={=u16} dwell_ms={=u64} source=normal_poll",
            attempt,
            dwell_ms
        );
        bms_diag_breadcrumb_note(9, attempt.min(u16::from(u8::MAX)) as u8);

        let raw_diag = self.bms_activation_current_is_auto;
        for addr in bms_probe_candidates().iter().copied() {
            match self.read_bq40_activation_snapshot_lean(addr) {
                Ok(snapshot) => {
                    let mut tracker = self.bms_activation_pattern_tracker;
                    match self.read_bq40_activation_snapshot_strict(addr, &mut tracker) {
                        Ok(strict_snapshot) => {
                            self.bms_activation_pattern_tracker = tracker;
                            defmt::info!(
                                "bms: activation min_charge_probe strict addr=0x{=u8:x} attempt={=u16} dwell_ms={=u64} temp_c_x10={=i32} vpack_mv={=u16} current_ma={=i16} rsoc_pct={=u16} batt_status=0x{=u16:x} cell1_mv={=u16} cell2_mv={=u16} cell3_mv={=u16} cell4_mv={=u16}",
                                addr,
                                attempt,
                                dwell_ms,
                                bq40z50::temp_c_x10_from_k_x10(strict_snapshot.temp_k_x10),
                                strict_snapshot.vpack_mv,
                                strict_snapshot.current_ma,
                                strict_snapshot.rsoc_pct,
                                strict_snapshot.batt_status,
                                strict_snapshot.cell_mv[0],
                                strict_snapshot.cell_mv[1],
                                strict_snapshot.cell_mv[2],
                                strict_snapshot.cell_mv[3]
                            );
                            return Some(Bq40ActivationProbeResult::Working {
                                addr,
                                snapshot: strict_snapshot,
                            });
                        }
                        Err(err) => {
                            self.bms_activation_pattern_tracker = tracker;
                            defmt::info!(
                                "bms: activation min_charge_probe lean addr=0x{=u8:x} attempt={=u16} dwell_ms={=u64} temp_c_x10={=i32} vpack_mv={=u16} current_ma={=i16} rsoc_pct={=u16} batt_status=0x{=u16:x} strict_detail_err={} confirm=failed",
                                addr,
                                attempt,
                                dwell_ms,
                                bq40z50::temp_c_x10_from_k_x10(snapshot.temp_k_x10),
                                snapshot.vpack_mv,
                                snapshot.current_ma,
                                snapshot.rsoc_pct,
                                snapshot.batt_status,
                                bq40_activation_read_error_kind(err)
                            );
                            continue;
                        }
                    }
                }
                Err(err) => {
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage=min_charge_probe_lean attempt={=u16} dwell_ms={=u64} err={}",
                            addr,
                            attempt,
                            dwell_ms,
                            bq40_activation_read_error_kind(err)
                        );
                    }
                }
            }
        }
        None
    }

    fn probe_bq40_activation_runtime(
        &mut self,
        addr: u8,
        attempt: u16,
        dwell_ms: u64,
        raw_diag: bool,
        require_trusted_voltage_for_confirm: bool,
    ) -> Option<Bq40z50Snapshot> {
        let voltage_mv = match self.read_bq40_u16_direct(addr, bq40z50::cmd::VOLTAGE) {
            Ok(voltage_mv) if voltage_mv <= 20_000 => {
                defmt::info!(
                    "bms: activation runtime_probe_voltage addr=0x{=u8:x} attempt={=u16} dwell_ms={=u64} vpack_mv={=u16}",
                    addr,
                    attempt,
                    dwell_ms,
                    voltage_mv
                );
                Some(voltage_mv)
            }
            Ok(voltage_mv) => {
                if raw_diag {
                    defmt::info!(
                        "bms_diag: addr=0x{=u8:x} stage=runtime_probe_voltage attempt={=u16} dwell_ms={=u64} vpack_mv={=u16} err=bad_range keep_strict=true",
                        addr,
                        attempt,
                        dwell_ms,
                        voltage_mv
                    );
                }
                Some(voltage_mv)
            }
            Err(err) => {
                if raw_diag {
                    defmt::info!(
                        "bms_diag: addr=0x{=u8:x} stage=runtime_probe_voltage attempt={=u16} dwell_ms={=u64} err={} keep_strict=true",
                        addr,
                        attempt,
                        dwell_ms,
                        bq40_activation_read_error_kind(err)
                    );
                }
                None
            }
        };

        if require_trusted_voltage_for_confirm
            && !voltage_mv.is_some_and(|raw| (2_500..=20_000).contains(&raw))
        {
            if raw_diag {
                defmt::info!(
                    "bms_diag: addr=0x{=u8:x} stage=runtime_probe_confirm_skipped attempt={=u16} dwell_ms={=u64} reason=untrusted_voltage",
                    addr,
                    attempt,
                    dwell_ms
                );
            }
            return None;
        }

        let mut tracker = self.bms_activation_pattern_tracker;
        let confirmed = self.confirm_bq40_activation_snapshot(
            addr,
            attempt.min(u16::from(u8::MAX)) as u8,
            dwell_ms,
            "runtime_probe_confirm",
            &mut tracker,
            raw_diag,
        );
        self.bms_activation_pattern_tracker = tracker;

        if let Some(snapshot) = confirmed {
            let core_only_snapshot = snapshot.op_status.is_none() && snapshot.cell_mv == [0; 4];
            defmt::info!(
                "bms: activation runtime_probe addr=0x{=u8:x} attempt={=u16} dwell_ms={=u64} source={} temp_c_x10={=i32} vpack_mv={=u16} current_ma={=i16} rsoc_pct={=u16} batt_status=0x{=u16:x} cell1_mv={=u16} cell2_mv={=u16} cell3_mv={=u16} cell4_mv={=u16}",
                addr,
                attempt,
                dwell_ms,
                if core_only_snapshot { "core_5word" } else { "strict" },
                bq40z50::temp_c_x10_from_k_x10(snapshot.temp_k_x10),
                snapshot.vpack_mv,
                snapshot.current_ma,
                snapshot.rsoc_pct,
                snapshot.batt_status,
                snapshot.cell_mv[0],
                snapshot.cell_mv[1],
                snapshot.cell_mv[2],
                snapshot.cell_mv[3]
            );
            return Some(snapshot);
        }

        if raw_diag {
            defmt::info!(
                "bms_diag: addr=0x{=u8:x} stage=runtime_probe_confirm attempt={=u16} dwell_ms={=u64} vpack_mv={=?} result=miss",
                addr,
                attempt,
                dwell_ms,
                voltage_mv
            );
        }
        None
    }

    fn run_bms_activation_wake_probe_step(
        &mut self,
        addr: u8,
        step: u8,
        delay_ms: u64,
        raw_diag: bool,
    ) -> Bq40ActivationProbeResult {
        let mut touched = false;
        let mut rsoc_after_touch = None;

        match self.touch_bq40_command(addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE) {
            Ok(()) => {
                touched = true;
                if raw_diag {
                    defmt::info!(
                        "bms_diag: addr=0x{=u8:x} stage=wake_touch_rsoc step={=u8} delay_ms={=u64}",
                        addr,
                        step,
                        delay_ms
                    );
                }
            }
            Err(kind) => {
                if raw_diag {
                    defmt::info!(
                        "bms_diag: addr=0x{=u8:x} stage=wake_touch_rsoc step={=u8} delay_ms={=u64} err={}",
                        addr,
                        step,
                        delay_ms,
                        kind
                    );
                }
            }
        }

        if touched {
            if let Ok(raw) = self.read_bq40_u16_after_touch(
                addr,
                "wake_touch_rsoc_raw",
                step,
                delay_ms,
                raw_diag,
            ) {
                if raw == BMS_ROM_MODE_SIGNATURE {
                    defmt::info!(
                        "bms: activation wake_stage step={=u8} delay_ms={=u64} addr=0x{=u8:x} result=rom_mode",
                        step,
                        delay_ms,
                        addr
                    );
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage=wake_window_rom_signature step={=u8} delay_ms={=u64}",
                            addr,
                            step,
                            delay_ms
                        );
                    }
                    return Bq40ActivationProbeResult::Rom;
                }
                rsoc_after_touch = Some(raw);
            }
        }

        if rsoc_after_touch.is_none() {
            match self.touch_bq40_command(addr, bq40z50::cmd::TEMPERATURE) {
                Ok(()) => {
                    touched = true;
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage=wake_touch_temp step={=u8} delay_ms={=u64}",
                            addr,
                            step,
                            delay_ms
                        );
                    }
                    let _ = self.read_bq40_u16_after_touch(
                        addr,
                        "wake_touch_temp_raw",
                        step,
                        delay_ms,
                        raw_diag,
                    );
                }
                Err(kind) => {
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage=wake_touch_temp step={=u8} delay_ms={=u64} err={}",
                            addr,
                            step,
                            delay_ms,
                            kind
                        );
                    }
                }
            }
        }

        if let Some(rsoc) = rsoc_after_touch {
            match self.touch_bq40_command(addr, bq40z50::cmd::TEMPERATURE) {
                Ok(()) => {
                    touched = true;
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage=wake_touch_temp step={=u8} delay_ms={=u64}",
                            addr,
                            step,
                            delay_ms
                        );
                    }
                    if let Ok(temp_raw) = self.read_bq40_u16_after_touch(
                        addr,
                        "wake_touch_temp_raw",
                        step,
                        delay_ms,
                        raw_diag,
                    ) {
                        if raw_diag {
                            defmt::info!(
                                "bms_diag: addr=0x{=u8:x} stage=wake_window_candidate step={=u8} delay_ms={=u64} rsoc_pct={=u16} temp_raw=0x{=u16:x} temp_c_x10={=i32}",
                                addr,
                                step,
                                delay_ms,
                                rsoc,
                                temp_raw,
                                bq40z50::temp_c_x10_from_k_x10(temp_raw)
                            );
                        }

                        let mut tracker = self.bms_activation_pattern_tracker;
                        if rsoc <= 100 && (2_000..=4_300).contains(&temp_raw) {
                            if let Some(snapshot) = self.confirm_bq40_activation_snapshot(
                                addr,
                                step,
                                delay_ms,
                                "wake_snapshot_confirm_touch",
                                &mut tracker,
                                raw_diag,
                            ) {
                                self.bms_activation_pattern_tracker = tracker;
                                return Bq40ActivationProbeResult::Working { addr, snapshot };
                            }
                        }
                        self.bms_activation_pattern_tracker = tracker;
                    }
                }
                Err(kind) => {
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage=wake_touch_temp step={=u8} delay_ms={=u64} err={}",
                            addr,
                            step,
                            delay_ms,
                            kind
                        );
                    }
                }
            }
        }

        if !touched {
            return Bq40ActivationProbeResult::Pending;
        }

        let mut tracker = self.bms_activation_pattern_tracker;
        match self.touch_then_read_bq40_wake_probe(
            addr,
            bq40z50::cmd::RELATIVE_STATE_OF_CHARGE,
            "wake_touch_read_rsoc",
            "wake_touch_read_rsoc_raw",
            step,
            delay_ms,
            raw_diag,
        ) {
            Ok(rsoc) => {
                if rsoc == BMS_ROM_MODE_SIGNATURE {
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage=wake_window_rom_signature step={=u8} delay_ms={=u64}",
                            addr,
                            step,
                            delay_ms
                        );
                    }
                    return Bq40ActivationProbeResult::Rom;
                }
                if rsoc <= 100 {
                    match self.touch_then_read_bq40_wake_probe(
                        addr,
                        bq40z50::cmd::TEMPERATURE,
                        "wake_touch_read_temp",
                        "wake_touch_read_temp_raw",
                        step,
                        delay_ms,
                        raw_diag,
                    ) {
                        Ok(temp_raw) if (2_000..=4_300).contains(&temp_raw) => {
                            if let Some(snapshot) = self.confirm_bq40_activation_snapshot(
                                addr,
                                step,
                                delay_ms,
                                "wake_snapshot_confirm_split",
                                &mut tracker,
                                raw_diag,
                            ) {
                                self.bms_activation_pattern_tracker = tracker;
                                return Bq40ActivationProbeResult::Working { addr, snapshot };
                            }
                        }
                        Ok(_) | Err(_) => {}
                    }
                }
            }
            Err(_) => {}
        }

        for round in 0..BMS_ACTIVATION_KEEPALIVE_ROUNDS {
            if round > 0 {
                spin_delay(BMS_ACTIVATION_KEEPALIVE_GAP);
            }

            for (cmd, stage_name) in [
                (
                    bq40z50::cmd::RELATIVE_STATE_OF_CHARGE,
                    "wake_keepalive_rsoc",
                ),
                (bq40z50::cmd::TEMPERATURE, "wake_keepalive_temp"),
            ] {
                match self.touch_bq40_command(addr, cmd) {
                    Ok(()) => {
                        if raw_diag {
                            defmt::info!(
                                "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} round={=u8}",
                                addr,
                                stage_name,
                                step,
                                delay_ms,
                                round as u8
                            );
                        }
                    }
                    Err(kind) => {
                        if raw_diag {
                            defmt::info!(
                                "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} round={=u8} err={}",
                                addr,
                                stage_name,
                                step,
                                delay_ms,
                                round as u8,
                                kind
                            );
                        }
                    }
                }
            }

            let rsoc = self
                .read_bq40_u16_wake_probe(
                    addr,
                    bq40z50::cmd::RELATIVE_STATE_OF_CHARGE,
                    "wake_read_rsoc_split",
                    step,
                    delay_ms,
                    round as u8,
                    raw_diag,
                )
                .ok();
            if rsoc == Some(BMS_ROM_MODE_SIGNATURE) {
                defmt::info!(
                    "bms: activation wake_stage step={=u8} delay_ms={=u64} addr=0x{=u8:x} round={=u8} result=rom_mode",
                    step,
                    delay_ms,
                    addr,
                    round as u8
                );
                return Bq40ActivationProbeResult::Rom;
            }

            let temp = self
                .read_bq40_u16_wake_probe(
                    addr,
                    bq40z50::cmd::TEMPERATURE,
                    "wake_read_temp_split",
                    step,
                    delay_ms,
                    round as u8,
                    raw_diag,
                )
                .ok();
            if let (Some(rsoc), Some(temp_raw)) = (rsoc, temp) {
                if raw_diag {
                    defmt::info!(
                        "bms_diag: addr=0x{=u8:x} stage=wake_keepalive_candidate step={=u8} delay_ms={=u64} round={=u8} rsoc_pct={=u16} temp_raw=0x{=u16:x} temp_c_x10={=i32}",
                        addr,
                        step,
                        delay_ms,
                        round as u8,
                        rsoc,
                        temp_raw,
                        bq40z50::temp_c_x10_from_k_x10(temp_raw)
                    );
                }
                if rsoc <= 100 && (2_000..=4_300).contains(&temp_raw) {
                    if let Some(snapshot) = self.confirm_bq40_activation_snapshot(
                        addr,
                        step,
                        delay_ms,
                        "wake_snapshot_confirm_keepalive",
                        &mut tracker,
                        raw_diag,
                    ) {
                        self.bms_activation_pattern_tracker = tracker;
                        return Bq40ActivationProbeResult::Working { addr, snapshot };
                    }
                }
            }
        }

        self.bms_activation_pattern_tracker = tracker;
        Bq40ActivationProbeResult::Pending
    }

    fn touch_bq40_command(&mut self, addr: u8, cmd: u8) -> Result<(), &'static str> {
        self.i2c.write(addr, &[cmd]).map_err(i2c_error_kind)
    }

    fn read_bq40_block_raw_checked(
        &mut self,
        addr: u8,
        cmd: u8,
    ) -> Result<Bq40ActivationBlockReadRaw, &'static str> {
        let mut buf = [0u8; 33];
        self.i2c
            .write_read(addr, &[cmd], &mut buf)
            .map_err(i2c_error_kind)?;

        let declared_len = buf[0];
        if declared_len == 0 || declared_len > 32 {
            return Err("bad_len");
        }

        let payload_len = declared_len.min(32);
        let payload_len_usize = payload_len as usize;
        let mut payload = [0u8; 32];
        payload[..payload_len_usize].copy_from_slice(&buf[1..(1 + payload_len_usize)]);
        Ok(Bq40ActivationBlockReadRaw {
            declared_len,
            payload_len,
            payload,
        })
    }

    fn log_bq40_activation_mac_probe(&mut self, addr: u8, stage: &'static str) {
        const MANUFACTURER_ACCESS_CMD: u8 = 0x00;
        const MANUFACTURER_DATA_CMD: u8 = 0x23;
        const DEVICE_TYPE_CMD_MSB_FIRST: [u8; 2] = [0x00, 0x01];

        match self.i2c.write(
            addr,
            &[
                MANUFACTURER_ACCESS_CMD,
                DEVICE_TYPE_CMD_MSB_FIRST[0],
                DEVICE_TYPE_CMD_MSB_FIRST[1],
            ],
        ) {
            Ok(()) => {
                spin_delay(BMS_ACTIVATION_MAC_WRITE_SETTLE);
            }
            Err(err) => {
                defmt::info!(
                    "bms_diag: addr=0x{=u8:x} stage={} mac_probe err={}",
                    addr,
                    stage,
                    i2c_error_kind(err)
                );
                return;
            }
        }

        match self.read_bq40_block_raw_checked(addr, MANUFACTURER_DATA_CMD) {
            Ok(raw) => {
                let payload_len = raw.payload_len as usize;
                let b0 = if payload_len > 0 { raw.payload[0] } else { 0 };
                let b1 = if payload_len > 1 { raw.payload[1] } else { 0 };
                let b2 = if payload_len > 2 { raw.payload[2] } else { 0 };
                let b3 = if payload_len > 3 { raw.payload[3] } else { 0 };
                let mb44_ok = payload_len >= 4 && b0 == 0x01 && b1 == 0x00;
                let device_type = if payload_len >= 4 {
                    u16::from_le_bytes([b2, b3])
                } else {
                    0
                };
                let verdict = if mb44_ok && device_type == BMS_DEVICE_TYPE_BQ40Z50 {
                    "device_type_ok"
                } else if mb44_ok {
                    "device_type_mismatch"
                } else {
                    "reply_unconfirmed"
                };
                defmt::info!(
                    "bms_diag: addr=0x{=u8:x} stage={} mac_probe len={=u8} payload={=u8} b0=0x{=u8:x} b1=0x{=u8:x} b2=0x{=u8:x} b3=0x{=u8:x} device_type=0x{=u16:x} verdict={}",
                    addr,
                    stage,
                    raw.declared_len,
                    raw.payload_len,
                    b0,
                    b1,
                    b2,
                    b3,
                    device_type,
                    verdict
                );
            }
            Err(err) => {
                defmt::info!(
                    "bms_diag: addr=0x{=u8:x} stage={} mac_probe err={}",
                    addr,
                    stage,
                    err
                );
            }
        }
    }

    fn read_bq40_u16_after_touch(
        &mut self,
        addr: u8,
        stage: &'static str,
        step: u8,
        delay_ms: u64,
        raw_diag: bool,
    ) -> Result<u16, &'static str> {
        for gap_ms in BMS_ACTIVATION_DIAG_TOUCH_READ_GAPS_MS {
            spin_delay(Duration::from_millis(gap_ms));
            let mut buf = [0u8; 2];
            match self.i2c.read(addr, &mut buf) {
                Ok(()) => {
                    let raw = u16::from_le_bytes(buf);
                    if raw_diag {
                        defmt::info!(
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
                Err(err) => {
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} gap_ms={=u64} err={}",
                            addr,
                            stage,
                            step,
                            delay_ms,
                            gap_ms,
                            i2c_error_kind(err)
                        );
                    }
                }
            }
        }

        Err("i2c_nack")
    }

    fn touch_then_read_bq40_wake_probe(
        &mut self,
        addr: u8,
        cmd: u8,
        touch_stage: &'static str,
        read_stage: &'static str,
        step: u8,
        delay_ms: u64,
        raw_diag: bool,
    ) -> Result<u16, &'static str> {
        for gap_ms in BMS_ACTIVATION_READ_GAPS_MS {
            match self.touch_bq40_command(addr, cmd) {
                Ok(()) => {
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} gap_ms={=u64}",
                            addr,
                            touch_stage,
                            step,
                            delay_ms,
                            gap_ms
                        );
                    }
                }
                Err(kind) => {
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} gap_ms={=u64} err={}",
                            addr,
                            touch_stage,
                            step,
                            delay_ms,
                            gap_ms,
                            kind
                        );
                    }
                    continue;
                }
            }

            spin_delay(Duration::from_millis(gap_ms));
            let mut buf = [0u8; 2];
            match self.i2c.read(addr, &mut buf) {
                Ok(()) => {
                    let raw = u16::from_le_bytes(buf);
                    if raw_diag {
                        defmt::info!(
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
                Err(err) => {
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} gap_ms={=u64} err={}",
                            addr,
                            read_stage,
                            step,
                            delay_ms,
                            gap_ms,
                            i2c_error_kind(err)
                        );
                    }
                }
            }
        }

        Err("i2c_nack")
    }

    fn read_bq40_u16_wake_probe(
        &mut self,
        addr: u8,
        cmd: u8,
        stage: &'static str,
        step: u8,
        delay_ms: u64,
        round: u8,
        raw_diag: bool,
    ) -> Result<u16, Bq40ActivationReadError> {
        for gap_ms in BMS_ACTIVATION_READ_GAPS_MS {
            let gap = Duration::from_millis(gap_ms);
            match self.read_bq40_u16_split_with_gap(addr, cmd, gap) {
                Ok(raw) => {
                    if raw_diag {
                        defmt::info!(
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
                Err(err) => {
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} round={=u8} gap_ms={=u64} err={}",
                            addr,
                            stage,
                            step,
                            delay_ms,
                            round,
                            gap_ms,
                            bq40_activation_read_error_kind(err)
                        );
                    }
                }
            }
        }

        self.read_bq40_u16_with_optional_pec(addr, cmd)
    }

    fn read_bq40_u16_with_pec(
        &mut self,
        addr: u8,
        cmd: u8,
    ) -> Result<u16, Bq40ActivationReadError> {
        let mut buf = [0u8; 3];
        self.i2c
            .write_read(addr, &[cmd], &mut buf)
            .map_err(|err| Bq40ActivationReadError::I2c(i2c_error_kind(err)))?;

        let addr_w = addr << 1;
        let addr_r = addr_w | 1;
        let expected = Self::crc8_smbus(&[addr_w, cmd, addr_r, buf[0], buf[1]]);
        if expected != buf[2] {
            return Err(Bq40ActivationReadError::InconsistentSample);
        }

        Ok(u16::from_le_bytes([buf[0], buf[1]]))
    }

    fn read_bq40_u16_split(&mut self, addr: u8, cmd: u8) -> Result<u16, Bq40ActivationReadError> {
        self.i2c
            .write(addr, &[cmd])
            .map_err(|err| Bq40ActivationReadError::I2c(i2c_error_kind(err)))?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let mut buf = [0u8; 2];
        self.i2c
            .read(addr, &mut buf)
            .map_err(|err| Bq40ActivationReadError::I2c(i2c_error_kind(err)))?;
        Ok(u16::from_le_bytes(buf))
    }

    fn read_bq40_u16_split_with_gap(
        &mut self,
        addr: u8,
        cmd: u8,
        gap: Duration,
    ) -> Result<u16, Bq40ActivationReadError> {
        self.i2c
            .write(addr, &[cmd])
            .map_err(|err| Bq40ActivationReadError::I2c(i2c_error_kind(err)))?;
        spin_delay(gap);
        let mut buf = [0u8; 2];
        self.i2c
            .read(addr, &mut buf)
            .map_err(|err| Bq40ActivationReadError::I2c(i2c_error_kind(err)))?;
        Ok(u16::from_le_bytes(buf))
    }

    fn read_bq40_u16_with_optional_pec(
        &mut self,
        addr: u8,
        cmd: u8,
    ) -> Result<u16, Bq40ActivationReadError> {
        const ATTEMPTS: u8 = 2;

        for attempt in 0..ATTEMPTS {
            if let Ok(v) = self.read_bq40_u16_with_pec(addr, cmd) {
                return Ok(v);
            }
            if let Ok(v) = self.read_bq40_u16_split(addr, cmd) {
                return Ok(v);
            }
            if let Ok(v) = bq40z50::read_u16(&mut self.i2c, addr, cmd) {
                return Ok(v);
            }

            if attempt + 1 < ATTEMPTS {
                spin_delay(BMS_ACTIVATION_WORD_GAP);
            }
        }

        Err(Bq40ActivationReadError::I2c("i2c_nack"))
    }

    fn read_bq40_i16_with_optional_pec(
        &mut self,
        addr: u8,
        cmd: u8,
    ) -> Result<i16, Bq40ActivationReadError> {
        self.read_bq40_u16_with_optional_pec(addr, cmd)
            .map(|raw| i16::from_le_bytes(raw.to_le_bytes()))
    }

    fn read_bq40_u16_direct(&mut self, addr: u8, cmd: u8) -> Result<u16, Bq40ActivationReadError> {
        bq40z50::read_u16(&mut self.i2c, addr, cmd)
            .map_err(|e| Bq40ActivationReadError::I2c(i2c_error_kind(e)))
    }

    fn read_bq40_i16_direct(&mut self, addr: u8, cmd: u8) -> Result<i16, Bq40ActivationReadError> {
        bq40z50::read_i16(&mut self.i2c, addr, cmd)
            .map_err(|e| Bq40ActivationReadError::I2c(i2c_error_kind(e)))
    }

    fn prime_bq40_command_window(&mut self, addr: u8) -> Result<(), Bq40ActivationReadError> {
        let _ = self.touch_bq40_command(addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE);
        Ok(())
    }

    fn read_bq40_u16_consistent(
        &mut self,
        addr: u8,
        cmd: u8,
        tolerance: u16,
    ) -> Result<u16, Bq40ActivationReadError> {
        let a = self.read_bq40_u16_with_optional_pec(addr, cmd)?;
        if a == BMS_SUSPICIOUS_STATUS || a == BMS_ROM_MODE_SIGNATURE {
            return Ok(a);
        }
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let b = self.read_bq40_u16_with_optional_pec(addr, cmd)?;
        let ab_diff = a.max(b) - a.min(b);
        if ab_diff <= tolerance {
            return Ok(b);
        }

        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let c = self.read_bq40_u16_with_optional_pec(addr, cmd)?;
        let ac_diff = a.max(c) - a.min(c);
        if ac_diff <= tolerance {
            return Ok(c);
        }
        let bc_diff = b.max(c) - b.min(c);
        if bc_diff <= tolerance {
            return Ok(c);
        }

        Err(Bq40ActivationReadError::InconsistentSample)
    }

    fn read_bq40_i16_consistent(
        &mut self,
        addr: u8,
        cmd: u8,
        tolerance: i16,
    ) -> Result<i16, Bq40ActivationReadError> {
        let a = self.read_bq40_i16_with_optional_pec(addr, cmd)?;
        if a == BMS_SUSPICIOUS_CURRENT_MA || a == BMS_ROM_MODE_SIGNATURE as i16 {
            return Ok(a);
        }
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let b = self.read_bq40_i16_with_optional_pec(addr, cmd)?;
        let ab_diff = (a as i32 - b as i32).abs();
        if ab_diff <= i32::from(tolerance) {
            return Ok(b);
        }

        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let c = self.read_bq40_i16_with_optional_pec(addr, cmd)?;
        let ac_diff = (a as i32 - c as i32).abs();
        if ac_diff <= i32::from(tolerance) {
            return Ok(c);
        }
        let bc_diff = (b as i32 - c as i32).abs();
        if bc_diff <= i32::from(tolerance) {
            return Ok(c);
        }

        Err(Bq40ActivationReadError::InconsistentSample)
    }

    fn read_bq40_activation_snapshot_strict(
        &mut self,
        addr: u8,
        tracker: &mut Bq40ActivationPatternTracker,
    ) -> Result<Bq40z50Snapshot, Bq40ActivationReadError> {
        self.prime_bq40_command_window(addr)?;
        let mut temp_k_x10 = self.read_bq40_u16_consistent(addr, bq40z50::cmd::TEMPERATURE, 5)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        self.prime_bq40_command_window(addr)?;
        let vpack_mv = self.read_bq40_u16_consistent(addr, bq40z50::cmd::VOLTAGE, 20)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        self.prime_bq40_command_window(addr)?;
        let current_ma = self.read_bq40_i16_consistent(addr, bq40z50::cmd::CURRENT, 100)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        self.prime_bq40_command_window(addr)?;
        let rsoc_pct =
            self.read_bq40_u16_consistent(addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE, 1)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        self.prime_bq40_command_window(addr)?;
        let batt_status = self.read_bq40_u16_consistent(addr, bq40z50::cmd::BATTERY_STATUS, 0)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        self.prime_bq40_command_window(addr)?;
        let cell1_mv = self.read_bq40_u16_consistent(addr, bq40z50::cmd::CELL_VOLTAGE_1, 20)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        self.prime_bq40_command_window(addr)?;
        let cell2_mv = self.read_bq40_u16_consistent(addr, bq40z50::cmd::CELL_VOLTAGE_2, 20)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        self.prime_bq40_command_window(addr)?;
        let cell3_mv = self.read_bq40_u16_consistent(addr, bq40z50::cmd::CELL_VOLTAGE_3, 20)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        self.prime_bq40_command_window(addr)?;
        let cell4_mv = self.read_bq40_u16_consistent(addr, bq40z50::cmd::CELL_VOLTAGE_4, 20)?;
        // Keep wake confirm aligned to the tool's mandatory snapshot only. Optional OP_STATUS
        // reads can perturb a fragile wake window without improving result classification.
        let op_status = None;

        let mut temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(temp_k_x10);
        if !(-400..=1250).contains(&temp_c_x10) {
            spin_delay(BMS_ACTIVATION_WORD_GAP);
            let retry_temp_k_x10 =
                self.read_bq40_u16_consistent(addr, bq40z50::cmd::TEMPERATURE, 5)?;
            let retry_temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(retry_temp_k_x10);
            if (-400..=1250).contains(&retry_temp_c_x10) {
                temp_k_x10 = retry_temp_k_x10;
                temp_c_x10 = retry_temp_c_x10;
            }
        }

        let repeat_count =
            observe_bq40_activation_signature(tracker, vpack_mv, current_ma, rsoc_pct, batt_status);
        if bq40_activation_signature_is_stale(vpack_mv, current_ma, batt_status, repeat_count) {
            defmt::info!(
                "bms_diag_raw: addr=0x{=u8:x} reason=stale_pattern temp_k_x10={=u16} temp_c_x10={=i32} vpack_mv={=u16} current_ma={=i16} rsoc_pct={=u16} batt_status=0x{=u16:x} cell1_mv={=u16} cell2_mv={=u16} cell3_mv={=u16} cell4_mv={=u16} repeats={=u8} op_status={=?}",
                addr,
                temp_k_x10,
                temp_c_x10,
                vpack_mv,
                current_ma,
                rsoc_pct,
                batt_status,
                cell1_mv,
                cell2_mv,
                cell3_mv,
                cell4_mv,
                repeat_count,
                op_status
            );
            return Err(Bq40ActivationReadError::StalePattern);
        }

        if !(-400..=1250).contains(&temp_c_x10) || vpack_mv > 20_000 || rsoc_pct > 100 {
            defmt::info!(
                "bms_diag_raw: addr=0x{=u8:x} reason=bad_range temp_k_x10={=u16} temp_c_x10={=i32} vpack_mv={=u16} current_ma={=i16} rsoc_pct={=u16} batt_status=0x{=u16:x} cell1_mv={=u16} cell2_mv={=u16} cell3_mv={=u16} cell4_mv={=u16} op_status={=?}",
                addr,
                temp_k_x10,
                temp_c_x10,
                vpack_mv,
                current_ma,
                rsoc_pct,
                batt_status,
                cell1_mv,
                cell2_mv,
                cell3_mv,
                cell4_mv,
                op_status
            );
            return Err(Bq40ActivationReadError::BadRange);
        }

        Ok(Bq40z50Snapshot {
            battery_mode: 0,
            temp_k_x10,
            vpack_mv,
            current_ma,
            rsoc_pct,
            remcap: 0,
            fcc: 0,
            batt_status,
            op_status,
            da_status2: None,
            filter_capacity: None,
            balance_config: None,
            gauging_status: None,
            afe_register: None,
            lock_diag: None,
            cell_mv: [cell1_mv, cell2_mv, cell3_mv, cell4_mv],
        })
    }

    fn read_bq40_activation_snapshot_core(
        &mut self,
        addr: u8,
    ) -> Result<Bq40z50Snapshot, Bq40ActivationReadError> {
        let temp_k_x10 = self.read_bq40_u16_direct(addr, bq40z50::cmd::TEMPERATURE)?;
        let vpack_mv = self.read_bq40_u16_direct(addr, bq40z50::cmd::VOLTAGE)?;
        let current_ma = self.read_bq40_i16_direct(addr, bq40z50::cmd::CURRENT)?;
        let rsoc_pct = self.read_bq40_u16_direct(addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE)?;
        let batt_status = self.read_bq40_u16_direct(addr, bq40z50::cmd::BATTERY_STATUS)?;
        let temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(temp_k_x10);

        if !(-400..=1250).contains(&temp_c_x10) || vpack_mv > 20_000 || rsoc_pct > 100 {
            return Err(Bq40ActivationReadError::BadRange);
        }

        Ok(Bq40z50Snapshot {
            battery_mode: 0,
            temp_k_x10,
            vpack_mv,
            current_ma,
            rsoc_pct,
            remcap: 0,
            fcc: 0,
            batt_status,
            op_status: None,
            da_status2: None,
            filter_capacity: None,
            balance_config: None,
            gauging_status: None,
            afe_register: None,
            lock_diag: None,
            cell_mv: [0; 4],
        })
    }

    fn read_bq40_activation_snapshot_lean(
        &mut self,
        addr: u8,
    ) -> Result<Bq40z50Snapshot, Bq40ActivationReadError> {
        let temp_k_x10 = self.read_bq40_u16_direct(addr, bq40z50::cmd::TEMPERATURE)?;
        let vpack_mv = self.read_bq40_u16_direct(addr, bq40z50::cmd::VOLTAGE)?;
        let current_ma = self.read_bq40_i16_direct(addr, bq40z50::cmd::CURRENT)?;
        let rsoc_pct = self.read_bq40_u16_direct(addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE)?;
        let batt_status = self.read_bq40_u16_direct(addr, bq40z50::cmd::BATTERY_STATUS)?;
        let temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(temp_k_x10);

        if !(-400..=1250).contains(&temp_c_x10) || vpack_mv > 20_000 || rsoc_pct > 100 {
            defmt::info!(
                "bms_diag_raw: addr=0x{=u8:x} reason=lean_bad_range temp_k_x10={=u16} temp_c_x10={=i32} vpack_mv={=u16} current_ma={=i16} rsoc_pct={=u16} batt_status=0x{=u16:x}",
                addr,
                temp_k_x10,
                temp_c_x10,
                vpack_mv,
                current_ma,
                rsoc_pct,
                batt_status
            );
            return Err(Bq40ActivationReadError::BadRange);
        }

        Ok(Bq40z50Snapshot {
            battery_mode: 0,
            temp_k_x10,
            vpack_mv,
            current_ma,
            rsoc_pct,
            remcap: 0,
            fcc: 0,
            batt_status,
            op_status: None,
            da_status2: None,
            filter_capacity: None,
            balance_config: None,
            gauging_status: None,
            afe_register: None,
            lock_diag: None,
            cell_mv: [0; 4],
        })
    }

    fn confirm_bq40_activation_snapshot(
        &mut self,
        addr: u8,
        step: u8,
        delay_ms: u64,
        stage: &'static str,
        tracker: &mut Bq40ActivationPatternTracker,
        raw_diag: bool,
    ) -> Option<Bq40z50Snapshot> {
        match self.read_bq40_activation_snapshot_core(addr) {
            Ok(snapshot) => {
                let repeat_count = observe_bq40_activation_signature(
                    tracker,
                    snapshot.vpack_mv,
                    snapshot.current_ma,
                    snapshot.rsoc_pct,
                    snapshot.batt_status,
                );
                if bq40_pack_indicates_no_battery(snapshot.vpack_mv) {
                    defmt::info!(
                        "bms: activation confirm_core low_pack_candidate addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} temp_c_x10={=i32} vpack_mv={=u16} current_ma={=i16} rsoc_pct={=u16} batt_status=0x{=u16:x}",
                        addr,
                        stage,
                        step,
                        delay_ms,
                        bq40z50::temp_c_x10_from_k_x10(snapshot.temp_k_x10),
                        snapshot.vpack_mv,
                        snapshot.current_ma,
                        snapshot.rsoc_pct,
                        snapshot.batt_status
                    );
                    return self.confirm_bq40_activation_no_battery(
                        addr, step, delay_ms, stage, tracker, raw_diag, "core", snapshot,
                    );
                }
                if bq40_activation_signature_is_stale(
                    snapshot.vpack_mv,
                    snapshot.current_ma,
                    snapshot.batt_status,
                    repeat_count,
                ) {
                    defmt::info!(
                        "bms: activation confirm_core stale addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} temp_c_x10={=i32} vpack_mv={=u16} current_ma={=i16} rsoc_pct={=u16} batt_status=0x{=u16:x} repeats={=u8}",
                        addr,
                        stage,
                        step,
                        delay_ms,
                        bq40z50::temp_c_x10_from_k_x10(snapshot.temp_k_x10),
                        snapshot.vpack_mv,
                        snapshot.current_ma,
                        snapshot.rsoc_pct,
                        snapshot.batt_status,
                        repeat_count
                    );
                    match self.read_bq40_activation_snapshot_strict(addr, tracker) {
                        Ok(snapshot) => {
                            if bq40_pack_indicates_no_battery(snapshot.vpack_mv) {
                                return self.confirm_bq40_activation_no_battery(
                                    addr,
                                    step,
                                    delay_ms,
                                    stage,
                                    tracker,
                                    raw_diag,
                                    "strict_after_stale",
                                    snapshot,
                                );
                            }
                            defmt::info!(
                                "bms: activation confirm addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} temp_c_x10={=i32} vpack_mv={=u16} current_ma={=i16} rsoc_pct={=u16} batt_status=0x{=u16:x} core_state=stale",
                                addr,
                                stage,
                                step,
                                delay_ms,
                                bq40z50::temp_c_x10_from_k_x10(snapshot.temp_k_x10),
                                snapshot.vpack_mv,
                                snapshot.current_ma,
                                snapshot.rsoc_pct,
                                snapshot.batt_status
                            );
                            return Some(snapshot);
                        }
                        Err(err) => {
                            if raw_diag {
                                defmt::info!(
                                    "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} core_state=stale strict_err={}",
                                    addr,
                                    stage,
                                    step,
                                    delay_ms,
                                    bq40_activation_read_error_kind(err)
                                );
                            }
                            return None;
                        }
                    }
                }
                defmt::info!(
                    "bms: activation confirm_core addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} temp_c_x10={=i32} vpack_mv={=u16} current_ma={=i16} rsoc_pct={=u16} batt_status=0x{=u16:x}",
                    addr,
                    stage,
                    step,
                    delay_ms,
                    bq40z50::temp_c_x10_from_k_x10(snapshot.temp_k_x10),
                    snapshot.vpack_mv,
                    snapshot.current_ma,
                    snapshot.rsoc_pct,
                    snapshot.batt_status
                );
                Some(snapshot)
            }
            Err(core_err) => match self.read_bq40_activation_snapshot_strict(addr, tracker) {
                Ok(snapshot) => {
                    if bq40_pack_indicates_no_battery(snapshot.vpack_mv) {
                        return self.confirm_bq40_activation_no_battery(
                            addr,
                            step,
                            delay_ms,
                            stage,
                            tracker,
                            raw_diag,
                            "strict_after_core_err",
                            snapshot,
                        );
                    }
                    defmt::info!(
                        "bms: activation confirm addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} temp_c_x10={=i32} vpack_mv={=u16} current_ma={=i16} rsoc_pct={=u16} batt_status=0x{=u16:x} op_status={=?} core_err={}",
                        addr,
                        stage,
                        step,
                        delay_ms,
                        bq40z50::temp_c_x10_from_k_x10(snapshot.temp_k_x10),
                        snapshot.vpack_mv,
                        snapshot.current_ma,
                        snapshot.rsoc_pct,
                        snapshot.batt_status,
                        snapshot.op_status,
                        bq40_activation_read_error_kind(core_err)
                    );
                    Some(snapshot)
                }
                Err(err) => {
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} core_err={} strict_err={}",
                            addr,
                            stage,
                            step,
                            delay_ms,
                            bq40_activation_read_error_kind(core_err),
                            bq40_activation_read_error_kind(err)
                        );
                    } else {
                        defmt::info!(
                            "bms: activation confirm miss addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} core_err={} strict_err={}",
                            addr,
                            stage,
                            step,
                            delay_ms,
                            bq40_activation_read_error_kind(core_err),
                            bq40_activation_read_error_kind(err)
                        );
                    }
                    None
                }
            },
        }
    }

    fn confirm_bq40_activation_no_battery(
        &mut self,
        addr: u8,
        step: u8,
        delay_ms: u64,
        stage: &'static str,
        tracker: &mut Bq40ActivationPatternTracker,
        raw_diag: bool,
        source: &'static str,
        snapshot: Bq40z50Snapshot,
    ) -> Option<Bq40z50Snapshot> {
        match self.read_bq40_activation_snapshot_strict(addr, tracker) {
            Ok(confirm) => {
                let confirmed = bq40_pack_indicates_no_battery(confirm.vpack_mv)
                    && bq40_low_pack_runtime_signature_matches(
                        snapshot.vpack_mv,
                        snapshot.current_ma,
                        snapshot.rsoc_pct,
                        snapshot.batt_status,
                        confirm.vpack_mv,
                        confirm.current_ma,
                        confirm.rsoc_pct,
                        confirm.batt_status,
                    );
                if confirmed {
                    defmt::info!(
                        "bms: activation confirm low_pack addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} source={} temp_c_x10={=i32} vpack_mv={=u16} current_ma={=i16} rsoc_pct={=u16} batt_status=0x{=u16:x} cell1_mv={=u16} cell2_mv={=u16} cell3_mv={=u16} cell4_mv={=u16}",
                        addr,
                        stage,
                        step,
                        delay_ms,
                        source,
                        bq40z50::temp_c_x10_from_k_x10(confirm.temp_k_x10),
                        confirm.vpack_mv,
                        confirm.current_ma,
                        confirm.rsoc_pct,
                        confirm.batt_status,
                        confirm.cell_mv[0],
                        confirm.cell_mv[1],
                        confirm.cell_mv[2],
                        confirm.cell_mv[3]
                    );
                    Some(confirm)
                } else {
                    if raw_diag {
                        defmt::info!(
                            "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} low_pack_mismatch source={} first_vpack_mv={=u16} first_current_ma={=i16} first_rsoc_pct={=u16} first_batt_status=0x{=u16:x} confirm_vpack_mv={=u16} confirm_current_ma={=i16} confirm_rsoc_pct={=u16} confirm_batt_status=0x{=u16:x}",
                            addr,
                            stage,
                            step,
                            delay_ms,
                            source,
                            snapshot.vpack_mv,
                            snapshot.current_ma,
                            snapshot.rsoc_pct,
                            snapshot.batt_status,
                            confirm.vpack_mv,
                            confirm.current_ma,
                            confirm.rsoc_pct,
                            confirm.batt_status
                        );
                    } else {
                        defmt::info!(
                            "bms: activation confirm low_pack mismatch addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} source={} first_vpack_mv={=u16} confirm_vpack_mv={=u16}",
                            addr,
                            stage,
                            step,
                            delay_ms,
                            source,
                            snapshot.vpack_mv,
                            confirm.vpack_mv
                        );
                    }
                    None
                }
            }
            Err(err) => {
                if raw_diag {
                    defmt::info!(
                        "bms_diag: addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} low_pack_confirm_miss source={} strict_err={}",
                        addr,
                        stage,
                        step,
                        delay_ms,
                        source,
                        bq40_activation_read_error_kind(err)
                    );
                } else {
                    defmt::info!(
                        "bms: activation confirm low_pack miss addr=0x{=u8:x} stage={} step={=u8} delay_ms={=u64} source={} strict_err={}",
                        addr,
                        stage,
                        step,
                        delay_ms,
                        source,
                        bq40_activation_read_error_kind(err)
                    );
                }
                None
            }
        }
    }

    fn apply_bq40_activation_snapshot(
        &mut self,
        addr: u8,
        snapshot: &Bq40z50Snapshot,
    ) -> BmsResultKind {
        let rca_alarm = (snapshot.batt_status & bq40z50::battery_status::RCA) != 0;
        let core_only_snapshot = snapshot.op_status.is_none() && snapshot.cell_mv == [0; 4];
        let low_pack_runtime = bq40_pack_indicates_no_battery(snapshot.vpack_mv);
        let (charge_ready, charge_reason) = bq40_decode_charge_path(snapshot.op_status);
        let (discharge_ready, discharge_reason) = bq40_decode_discharge_path(snapshot.op_status);
        let protection_active = bq40_protection_active(snapshot.batt_status, snapshot.op_status);
        let primary_reason = bq40_primary_reason(
            snapshot.batt_status,
            snapshot.op_status,
            charge_reason,
            discharge_reason,
        );
        let flow = bq40_decode_current_flow(snapshot.current_ma);
        let state = if core_only_snapshot {
            if rca_alarm || low_pack_runtime {
                SelfCheckCommState::Warn
            } else {
                SelfCheckCommState::Ok
            }
        } else if matches!(discharge_ready, Some(false)) || rca_alarm {
            SelfCheckCommState::Warn
        } else {
            SelfCheckCommState::Ok
        };

        self.bms_addr = Some(addr);
        self.bms_runtime_seen = true;
        self.bms_ok_streak = self.bms_ok_streak.saturating_add(1);
        self.bms_err_streak = 0;
        self.bms_next_retry_at = None;
        self.bms_next_poll_at = Instant::now();
        self.ui_snapshot.bq40z50 = state;
        self.ui_snapshot.bq40z50_pack_mv = Some(snapshot.vpack_mv);
        self.ui_snapshot.bq40z50_current_ma = Some(snapshot.current_ma);
        self.ui_snapshot.bq40z50_soc_pct = Some(snapshot.rsoc_pct);
        self.ui_snapshot.bq40z50_rca_alarm = Some(rca_alarm);
        self.ui_snapshot.bq40z50_no_battery = Some(low_pack_runtime);
        self.ui_snapshot.bq40z50_discharge_ready = discharge_ready;
        self.ui_snapshot.bq40z50_issue_detail =
            bq40_ui_issue_detail(low_pack_runtime, primary_reason);
        self.apply_bms_detail_snapshot(snapshot);
        self.bms_audio = BmsAudioState {
            rca_alarm: Some(rca_alarm),
            protection_active,
            module_fault: false,
        };

        defmt::info!(
            "bms: activation trusted_snapshot addr=0x{=u8:x} source={} state={} no_battery={=bool} temp_c_x10={=i32} vpack_mv={=u16} current_ma={=i16} flow={} rsoc_pct={=u16} batt_status=0x{=u16:x} cell1_mv={=u16} cell2_mv={=u16} cell3_mv={=u16} cell4_mv={=u16} rca_alarm={=bool} chg_ready={=?} dsg_ready={=?} primary_reason={}",
            addr,
            if core_only_snapshot { "core_5word" } else { "strict" },
            self_check_comm_state_name(state),
            low_pack_runtime,
            bq40z50::temp_c_x10_from_k_x10(snapshot.temp_k_x10),
            snapshot.vpack_mv,
            snapshot.current_ma,
            flow,
            snapshot.rsoc_pct,
            snapshot.batt_status,
            snapshot.cell_mv[0],
            snapshot.cell_mv[1],
            snapshot.cell_mv[2],
            snapshot.cell_mv[3],
            rca_alarm,
            charge_ready,
            discharge_ready,
            primary_reason
        );

        self.bms_recovery_result_from_snapshot(state, discharge_ready, low_pack_runtime)
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

    fn maybe_auto_request_bms_activation(&mut self) {
        if !boot_diag_auto_recovery_enabled(self.cfg.bms_boot_diag_auto_validate) {
            return;
        }

        if !self.cfg.bms_boot_diag_auto_validate {
            return;
        }

        if self.bms_activation_auto_attempted
            || self.bms_activation_state != BmsActivationState::Idle
        {
            return;
        }

        let now = Instant::now();
        if now < self.bms_activation_auto_due_at {
            return;
        }

        if self.ui_snapshot.bq25792 == SelfCheckCommState::Pending
            || self.ui_snapshot.fusb302 == SelfCheckCommState::Pending
        {
            let due_at = now + Duration::from_millis(500);
            self.update_bms_activation_auto_due(due_at);
            return;
        }

        let auto_activation_needed =
            !bq40_last_result_blocks_auto_recovery(self.ui_snapshot.bq40z50_last_result)
                && self.ui_snapshot.bq40z50 == SelfCheckCommState::Err;
        if !auto_activation_needed {
            self.bms_activation_auto_attempted = true;
            self.bms_activation_auto_force_charge_until = None;
            self.bms_activation_auto_force_charge_programmed = false;
            if let Some(restore_chg_enabled) =
                self.restore_bms_activation_charger_backup("auto_skip_not_needed")
            {
                if restore_chg_enabled {
                    self.chg_ce.set_low();
                    self.chg_enabled = true;
                } else {
                    self.chg_ce.set_high();
                    self.chg_enabled = false;
                }
            }
            defmt::info!(
                "bms: activation auto_skip reason=bq40_not_err state={} trusted_evidence={=bool} last_result={}",
                self_check_comm_state_name(self.ui_snapshot.bq40z50),
                self.has_trusted_bq40_runtime_evidence(),
                bms_result_option_name(self.ui_snapshot.bq40z50_last_result)
            );
            return;
        }

        if self.ui_snapshot.fusb302_vbus_present != Some(true) {
            self.bms_activation_auto_attempted = true;
            self.bms_activation_auto_force_charge_until = None;
            self.bms_activation_auto_force_charge_programmed = false;
            if let Some(restore_chg_enabled) =
                self.restore_bms_activation_charger_backup("auto_skip_no_input_power")
            {
                if restore_chg_enabled {
                    self.chg_ce.set_low();
                    self.chg_enabled = true;
                } else {
                    self.chg_ce.set_high();
                    self.chg_enabled = false;
                }
            }
            defmt::info!(
                "bms: activation auto_skip reason=no_input_power bq40_state={} charger_state={} input_present={=?}",
                self_check_comm_state_name(self.ui_snapshot.bq40z50),
                self_check_comm_state_name(self.ui_snapshot.bq25792),
                self.ui_snapshot.fusb302_vbus_present
            );
            return;
        }

        if BMS_BOOT_DIAG_TOOL_STYLE_PROBE_ONLY {
            self.bms_activation_auto_attempted = true;
            self.bms_activation_auto_force_charge_until =
                Some(now + BMS_BOOT_DIAG_TOOL_STYLE_FORCE_HOLD);
            self.bms_activation_auto_force_charge_programmed = true;
            self.bms_next_poll_at = now;
            self.bms_next_retry_at = None;
            self.chg_next_poll_at = now;
            self.chg_next_retry_at = None;
            defmt::info!(
                "bms: activation auto_skip reason=tool_style_probe_only hold_ms={=u64} poll_ms={=u32} retry_backoff_suppressed={=bool} bq40_state={} charger_state={} charger_allowed={=bool} input_present={=?} vbat_present={=?}",
                BMS_BOOT_DIAG_TOOL_STYLE_FORCE_HOLD.as_millis() as u64,
                2_000u32,
                true,
                self_check_comm_state_name(self.ui_snapshot.bq40z50),
                self_check_comm_state_name(self.ui_snapshot.bq25792),
                self.charger_allowed,
                self.ui_snapshot.fusb302_vbus_present,
                self.ui_snapshot.bq25792_vbat_present
            );
            return;
        }

        self.bms_activation_auto_attempted = true;
        self.bms_activation_auto_force_charge_until = None;
        bms_diag_breadcrumb_note(1, 0);
        defmt::info!(
            "bms: activation auto_request reason=boot_diag bq40_state={} charger_state={} charger_allowed={=bool} input_present={=?} vbat_present={=?}",
            self_check_comm_state_name(self.ui_snapshot.bq40z50),
            self_check_comm_state_name(self.ui_snapshot.bq25792),
            self.charger_allowed,
            self.ui_snapshot.fusb302_vbus_present,
            self.ui_snapshot.bq25792_vbat_present
        );
        self.request_bms_activation_with_diag_override(true, true);
    }

    fn maybe_auto_request_bms_discharge_authorization(&mut self) {
        if !boot_diag_auto_recovery_enabled(self.cfg.bms_boot_diag_auto_validate) {
            return;
        }

        if self.bms_activation_state != BmsActivationState::Idle
            || bq40_last_result_blocks_auto_recovery(self.ui_snapshot.bq40z50_last_result)
            || !self.can_request_bms_discharge_authorization()
        {
            return;
        }

        self.request_bms_discharge_authorization(true);
    }

    fn maybe_track_bms_activation(&mut self) -> bool {
        if self.bms_activation_state != BmsActivationState::Pending {
            return false;
        }

        if !self.maybe_advance_bms_activation_phase() {
            return false;
        }

        let wake_stage_before = self.bms_activation_diag_stage;
        let probe_without_charge_before = self.bms_activation_followup_attempts;
        if let Some(result) = self.maybe_run_bms_activation_probe_without_charge() {
            match result {
                Bq40ActivationProbeResult::Pending => {}
                Bq40ActivationProbeResult::Rom => {
                    self.finish_bms_activation(BmsResultKind::RomMode, "rom_mode_detected");
                    return true;
                }
                Bq40ActivationProbeResult::Working { addr, snapshot } => {
                    let result = self.apply_bq40_activation_snapshot(addr, &snapshot);
                    if self.bms_recovery_requires_discharge_ready()
                        && result == BmsResultKind::Abnormal
                    {
                        defmt::info!(
                            "bms: discharge_authorization pending phase={} dsg_ready={=?} primary_reason=path_still_blocked",
                            bms_activation_phase_name(self.bms_activation_phase),
                            self.ui_snapshot.bq40z50_discharge_ready
                        );
                        return true;
                    }
                    let reason = match result {
                        BmsResultKind::Success => "bq40_probe_without_charge_ready",
                        BmsResultKind::Abnormal => "bq40_probe_without_charge_abnormal",
                        BmsResultKind::RomMode => "bq40_probe_without_charge_rom_mode",
                        BmsResultKind::NoBattery => "bq40_probe_without_charge_no_battery",
                        BmsResultKind::NotDetected => "bq40_probe_without_charge_not_detected",
                    };
                    self.finish_bms_activation(result, reason);
                    return true;
                }
            }
        }

        if self.bms_activation_phase == BmsActivationPhase::ProbeWithoutCharge {
            let Some(started_at) = self.bms_activation_started_at else {
                self.finish_bms_activation(BmsResultKind::NotDetected, "activation_phase_missing");
                return false;
            };
            if started_at.elapsed() >= BMS_ACTIVATION_PROBE_WITHOUT_CHARGE_WINDOW {
                if self.bms_activation_force_charge_requested {
                    if let Err(reason) = self.begin_bms_activation_repower_window() {
                        self.finish_bms_activation(BmsResultKind::NotDetected, reason);
                    }
                } else {
                    self.begin_bms_activation_wake_probe();
                }
                return true;
            }
        }

        if self.bms_activation_phase == BmsActivationPhase::WaitChargeOff {
            let Some(started_at) = self.bms_activation_started_at else {
                self.finish_bms_activation(BmsResultKind::NotDetected, "activation_phase_missing");
                return false;
            };
            if started_at.elapsed() >= BMS_ACTIVATION_REPOWER_OFF_WINDOW {
                if let Err(reason) = self.begin_bms_activation_min_charge_path() {
                    self.finish_bms_activation(BmsResultKind::NotDetected, reason);
                }
                return true;
            }
        }

        if let Some(result) = self.maybe_run_bms_activation_min_charge_probe() {
            match result {
                Bq40ActivationProbeResult::Pending => {}
                Bq40ActivationProbeResult::Rom => {
                    self.finish_bms_activation(BmsResultKind::RomMode, "rom_mode_detected");
                    return true;
                }
                Bq40ActivationProbeResult::Working { addr, snapshot } => {
                    let result = self.apply_bq40_activation_snapshot(addr, &snapshot);
                    if self.bms_recovery_requires_discharge_ready()
                        && result == BmsResultKind::Abnormal
                    {
                        defmt::info!(
                            "bms: discharge_authorization pending phase={} dsg_ready={=?} primary_reason=path_still_blocked",
                            bms_activation_phase_name(self.bms_activation_phase),
                            self.ui_snapshot.bq40z50_discharge_ready
                        );
                        return true;
                    }
                    let reason = match result {
                        BmsResultKind::Success => "bq40_min_charge_probe_ready",
                        BmsResultKind::Abnormal => "bq40_min_charge_probe_abnormal",
                        BmsResultKind::RomMode => "bq40_min_charge_probe_rom_mode",
                        BmsResultKind::NoBattery => "bq40_min_charge_probe_no_battery",
                        BmsResultKind::NotDetected => "bq40_min_charge_probe_not_detected",
                    };
                    self.finish_bms_activation(result, reason);
                    return true;
                }
            }
        }

        if self.bms_activation_phase == BmsActivationPhase::MinChargeProbe {
            let Some(started_at) = self.bms_activation_started_at else {
                self.finish_bms_activation(BmsResultKind::NotDetected, "activation_phase_missing");
                return false;
            };
            if started_at.elapsed() >= BMS_ACTIVATION_MIN_CHARGE_PROBE_WINDOW {
                self.begin_bms_activation_wake_probe();
                return true;
            }
        }

        if let Some(result) = self.maybe_run_bms_activation_wake_probe() {
            match result {
                Bq40ActivationProbeResult::Pending => {}
                Bq40ActivationProbeResult::Rom => {
                    self.finish_bms_activation(BmsResultKind::RomMode, "rom_mode_detected");
                    return true;
                }
                Bq40ActivationProbeResult::Working { addr, snapshot } => {
                    let result = self.apply_bq40_activation_snapshot(addr, &snapshot);
                    if self.bms_recovery_requires_discharge_ready()
                        && result == BmsResultKind::Abnormal
                    {
                        defmt::info!(
                            "bms: discharge_authorization pending phase={} dsg_ready={=?} primary_reason=path_still_blocked",
                            bms_activation_phase_name(self.bms_activation_phase),
                            self.ui_snapshot.bq40z50_discharge_ready
                        );
                        return true;
                    }
                    let reason = match result {
                        BmsResultKind::Success => "bq40_confirmed_ready",
                        BmsResultKind::Abnormal => "bq40_confirmed_abnormal",
                        BmsResultKind::RomMode => "bq40_confirmed_rom_mode",
                        BmsResultKind::NoBattery => "bq40_confirmed_no_battery",
                        BmsResultKind::NotDetected => "bq40_confirmed_not_detected",
                    };
                    self.finish_bms_activation(result, reason);
                    return true;
                }
            }
        }

        if self.bms_activation_phase == BmsActivationPhase::ProbeWithoutCharge
            && self.bms_activation_diag_stage >= BMS_ACTIVATION_DIAG_STAGE_DELAYS_MS.len()
        {
            if self.bms_activation_force_charge_requested {
                for addr in bms_probe_candidates().iter().copied() {
                    self.log_bq40_activation_mac_probe(addr, "probe_without_charge");
                }
                if let Err(reason) = self.begin_bms_activation_min_charge_path() {
                    self.finish_bms_activation(BmsResultKind::NotDetected, reason);
                }
                return true;
            }
            self.finish_bms_activation(BmsResultKind::NotDetected, "probe_without_charge_miss");
            return true;
        }

        let followup_before = self.bms_activation_followup_attempts;
        if let Some(result) = self.maybe_run_bms_activation_followup_probe() {
            match result {
                Bq40ActivationProbeResult::Pending => {}
                Bq40ActivationProbeResult::Rom => {
                    self.finish_bms_activation(BmsResultKind::RomMode, "rom_mode_detected");
                    return true;
                }
                Bq40ActivationProbeResult::Working { addr, snapshot } => {
                    let result = self.apply_bq40_activation_snapshot(addr, &snapshot);
                    if self.bms_recovery_requires_discharge_ready()
                        && result == BmsResultKind::Abnormal
                    {
                        defmt::info!(
                            "bms: discharge_authorization pending phase={} dsg_ready={=?} primary_reason=path_still_blocked",
                            bms_activation_phase_name(self.bms_activation_phase),
                            self.ui_snapshot.bq40z50_discharge_ready
                        );
                        return true;
                    }
                    let reason = match result {
                        BmsResultKind::Success => "bq40_followup_ready",
                        BmsResultKind::Abnormal => "bq40_followup_abnormal",
                        BmsResultKind::RomMode => "bq40_followup_rom_mode",
                        BmsResultKind::NoBattery => "bq40_followup_no_battery",
                        BmsResultKind::NotDetected => "bq40_followup_not_detected",
                    };
                    self.finish_bms_activation(result, reason);
                    return true;
                }
            }
        }

        let bms_i2c_active = self.bms_activation_diag_stage != wake_stage_before
            || self.bms_activation_followup_attempts != followup_before
            || self.bms_activation_followup_attempts != probe_without_charge_before;

        if self.is_bq40_rom_mode_detected() {
            self.finish_bms_activation(BmsResultKind::RomMode, "rom_mode_detected");
            return bms_i2c_active;
        }

        match self.ui_snapshot.bq40z50 {
            SelfCheckCommState::Ok if !self.bms_recovery_requires_discharge_ready() => {
                self.finish_bms_activation(BmsResultKind::Success, "bq40_ready");
                return bms_i2c_active;
            }
            SelfCheckCommState::Warn
                if !self.bms_recovery_requires_discharge_ready()
                    && self.has_trusted_bq40_runtime_evidence() =>
            {
                self.finish_bms_activation(BmsResultKind::Abnormal, "bq40_warn_after_activation");
                return bms_i2c_active;
            }
            _ => {}
        }

        let Some(deadline) = self.bms_activation_deadline else {
            self.finish_bms_activation(BmsResultKind::NotDetected, "activation_deadline_missing");
            return bms_i2c_active;
        };
        if Instant::now() >= deadline {
            let result = if self.is_bq40_rom_mode_detected() {
                BmsResultKind::RomMode
            } else if self.bms_recovery_requires_discharge_ready()
                && self.ui_snapshot.bq40z50 != SelfCheckCommState::Err
                && self.has_trusted_bq40_runtime_evidence()
            {
                BmsResultKind::Abnormal
            } else {
                BmsResultKind::NotDetected
            };
            let reason = match result {
                BmsResultKind::RomMode => "deadline_elapsed_rom_mode",
                BmsResultKind::Abnormal => "deadline_elapsed_discharge_still_blocked",
                BmsResultKind::NotDetected => "deadline_elapsed_not_detected",
                BmsResultKind::Success => "deadline_elapsed_success",
                BmsResultKind::NoBattery => "deadline_elapsed_no_battery",
            };
            self.finish_bms_activation(result, reason);
        }
        bms_i2c_active
    }

    fn maybe_advance_bms_activation_phase(&mut self) -> bool {
        let Some(started_at) = self.bms_activation_started_at else {
            self.finish_bms_activation(BmsResultKind::NotDetected, "activation_phase_missing");
            return false;
        };

        match self.bms_activation_phase {
            BmsActivationPhase::ProbeWithoutCharge => true,
            BmsActivationPhase::WaitChargeOff => true,
            BmsActivationPhase::WakeProbe => true,
            BmsActivationPhase::WaitMinChargeSettle => {
                if started_at.elapsed() < BMS_ACTIVATION_MIN_CHARGE_SETTLE {
                    return false;
                }
                let now = Instant::now();
                self.bms_activation_phase = BmsActivationPhase::MinChargeProbe;
                self.bms_activation_started_at = Some(now);
                self.bms_activation_diag_stage = 0;
                self.bms_activation_followup_next_at = None;
                self.bms_activation_followup_attempts = 0;
                self.bms_activation_exercise_next_at = None;
                self.bms_activation_pattern_tracker = Bq40ActivationPatternTracker::new();
                self.bms_activation_isolation_until = None;
                self.bms_next_poll_at = now;
                self.bms_next_retry_at = None;
                bms_diag_breadcrumb_note(7, 0);
                defmt::info!(
                    "bms: activation phase old={} new={} settle_ms={=u32} probe_window_ms={=u32}",
                    bms_activation_phase_name(BmsActivationPhase::WaitMinChargeSettle),
                    bms_activation_phase_name(self.bms_activation_phase),
                    BMS_ACTIVATION_MIN_CHARGE_SETTLE.as_millis() as u32,
                    BMS_ACTIVATION_MIN_CHARGE_PROBE_WINDOW.as_millis() as u32
                );
                true
            }
            BmsActivationPhase::MinChargeProbe => true,
        }
    }

    fn begin_bms_activation_repower_window(&mut self) -> Result<(), &'static str> {
        let old_phase = self.bms_activation_phase;
        let now = Instant::now();
        self.bms_activation_auto_force_charge_until = None;
        self.bms_activation_auto_force_charge_programmed = false;
        self.bms_activation_phase = BmsActivationPhase::WaitChargeOff;
        self.bms_activation_started_at = Some(now);
        self.bms_activation_diag_stage = 0;
        self.bms_activation_followup_next_at = None;
        self.bms_activation_followup_attempts = 0;
        self.bms_activation_exercise_next_at = None;
        self.bms_activation_pattern_tracker = Bq40ActivationPatternTracker::new();
        self.bms_activation_isolation_until = None;
        self.chg_next_poll_at = now;
        self.chg_next_retry_at = None;
        self.maybe_poll_charger(&IrqSnapshot::default());
        if self.bms_activation_state != BmsActivationState::Pending {
            return Err("charger_poll_failed");
        }
        if self.chg_enabled {
            return Err("disable_charger_for_repower_failed");
        }
        bms_diag_breadcrumb_note(5, 0);
        defmt::info!(
            "bms: activation phase old={} new={} off_ms={=u32}",
            bms_activation_phase_name(old_phase),
            bms_activation_phase_name(self.bms_activation_phase),
            BMS_ACTIVATION_REPOWER_OFF_WINDOW.as_millis() as u32
        );
        Ok(())
    }

    fn begin_bms_activation_min_charge_path(&mut self) -> Result<(), &'static str> {
        let now = Instant::now();
        let old_phase = self.bms_activation_phase;
        self.bms_activation_phase = BmsActivationPhase::WaitMinChargeSettle;
        if let Err(reason) = self.apply_bms_activation_min_charge_profile() {
            return Err(reason);
        }
        self.bms_activation_started_at = Some(now);
        self.bms_activation_diag_stage = 0;
        self.bms_activation_followup_next_at = None;
        self.bms_activation_followup_attempts = 0;
        self.bms_activation_exercise_next_at = None;
        self.bms_activation_pattern_tracker = Bq40ActivationPatternTracker::new();
        self.bms_activation_isolation_until = Some(now);
        self.bms_next_poll_at = now;
        self.bms_next_retry_at = None;
        self.chg_next_poll_at = now;
        self.chg_next_retry_at = None;
        bms_diag_breadcrumb_note(6, 0);
        defmt::info!(
            "bms: activation phase old={} new={} settle_ms={=u32}",
            bms_activation_phase_name(old_phase),
            bms_activation_phase_name(self.bms_activation_phase),
            BMS_ACTIVATION_MIN_CHARGE_SETTLE.as_millis() as u32
        );
        Ok(())
    }

    fn begin_bms_activation_wake_probe(&mut self) {
        let old_phase = self.bms_activation_phase;
        let now = Instant::now();
        self.bms_activation_phase = BmsActivationPhase::WakeProbe;
        self.bms_activation_started_at = Some(now);
        self.bms_activation_diag_stage = 0;
        self.bms_activation_followup_next_at = None;
        self.bms_activation_followup_attempts = 0;
        self.bms_activation_exercise_next_at = Some(now);
        self.bms_activation_pattern_tracker = Bq40ActivationPatternTracker::new();
        self.bms_activation_isolation_until = Some(now);
        bms_diag_breadcrumb_note(8, 0);
        defmt::info!(
            "bms: activation phase old={} new={} wake_stages={=u8}",
            bms_activation_phase_name(old_phase),
            bms_activation_phase_name(self.bms_activation_phase),
            BMS_ACTIVATION_DIAG_STAGE_DELAYS_MS.len() as u8
        );
    }

    fn begin_bms_activation_probe_without_charge(&mut self) -> Result<(), &'static str> {
        let old_phase = self.bms_activation_phase;
        let now = Instant::now();
        self.bms_activation_phase = BmsActivationPhase::ProbeWithoutCharge;
        self.bms_activation_started_at = Some(now);
        self.bms_activation_diag_stage = 0;
        self.bms_activation_followup_next_at = None;
        self.bms_activation_followup_attempts = 0;
        self.bms_activation_exercise_next_at = None;
        self.bms_activation_pattern_tracker = Bq40ActivationPatternTracker::new();
        self.bms_activation_isolation_until = None;
        self.bms_next_poll_at = now;
        self.bms_next_retry_at = None;
        self.chg_next_poll_at = now;
        self.chg_next_retry_at = None;
        self.maybe_poll_charger(&IrqSnapshot::default());
        if self.bms_activation_state != BmsActivationState::Pending {
            return Err("charger_poll_failed");
        }
        self.chg_next_poll_at = Instant::now() + BMS_ACTIVATION_CHARGER_POLL_PERIOD;
        bms_diag_breadcrumb_note(4, 0);
        defmt::info!(
            "bms: activation phase old={} new={} charger_mode=off",
            bms_activation_phase_name(old_phase),
            bms_activation_phase_name(self.bms_activation_phase)
        );
        Ok(())
    }

    fn has_trusted_bq40_runtime_evidence(&self) -> bool {
        self.ui_snapshot.bq40z50_soc_pct.is_some()
            || self.ui_snapshot.bq40z50_rca_alarm.is_some()
            || self.ui_snapshot.bq40z50_discharge_ready.is_some()
    }

    fn bms_recovery_requires_discharge_ready(&self) -> bool {
        self.bms_activation_request_kind == BmsRecoveryRequestKind::DischargeAuthorization
    }

    fn bms_recovery_result_from_snapshot(
        &self,
        state: SelfCheckCommState,
        discharge_ready: Option<bool>,
        low_pack_runtime: bool,
    ) -> BmsResultKind {
        if low_pack_runtime {
            return BmsResultKind::NoBattery;
        }

        if self.bms_recovery_requires_discharge_ready() {
            if state == SelfCheckCommState::Ok && discharge_ready == Some(true) {
                BmsResultKind::Success
            } else {
                BmsResultKind::Abnormal
            }
        } else if state == SelfCheckCommState::Ok {
            BmsResultKind::Success
        } else {
            BmsResultKind::Abnormal
        }
    }

    fn apply_bms_activation_min_charge_profile(&mut self) -> Result<(), &'static str> {
        self.chg_ilim_hiz_brk.set_low();
        defmt::info!(
            "bms: activation min_charge_step=program_profile_start vreg_mv={=u16} ichg_ma={=u16} iindpm_ma={=u16}",
            BMS_ACTIVATION_FORCE_VREG_MV,
            BMS_ACTIVATION_FORCE_ICHG_MA,
            BMS_ACTIVATION_FORCE_IINDPM_MA
        );
        if bq25792::set_charge_voltage_limit_mv(&mut self.i2c, BMS_ACTIVATION_FORCE_VREG_MV)
            .is_err()
        {
            return Err("program_activation_profile_failed");
        }
        defmt::info!("bms: activation min_charge_step=program_profile_vreg_ok");
        self.chg_next_poll_at = Instant::now();
        self.chg_next_retry_at = None;
        defmt::info!("bms: activation min_charge_step=poll_charger");
        self.maybe_poll_charger(&IrqSnapshot::default());
        if self.bms_activation_state != BmsActivationState::Pending {
            return Err("charger_poll_failed");
        }
        defmt::info!(
            "bms: activation min_charge_step=poll_charger_ok chg_enabled={=bool}",
            self.chg_enabled
        );
        if !self.chg_enabled {
            return Err("enable_charger_for_activation_failed");
        }
        Ok(())
    }

    fn finish_bms_activation(&mut self, result: BmsResultKind, reason: &'static str) {
        bms_diag_breadcrumb_note(
            13,
            match result {
                BmsResultKind::Success => 1,
                BmsResultKind::NoBattery => 2,
                BmsResultKind::RomMode => 3,
                BmsResultKind::Abnormal => 4,
                BmsResultKind::NotDetected => 5,
            },
        );
        let mut restore_chg_enabled = false;
        if let Some(chg_enabled) = self.restore_bms_activation_charger_backup(reason) {
            restore_chg_enabled = chg_enabled;
        }
        if restore_chg_enabled {
            self.chg_ce.set_low();
            self.chg_enabled = true;
        } else {
            self.chg_ce.set_high();
            self.chg_enabled = false;
        }
        self.bms_activation_deadline = None;
        self.bms_activation_phase = BmsActivationPhase::WakeProbe;
        self.bms_activation_started_at = None;
        self.bms_activation_diag_stage = 0;
        self.bms_activation_followup_next_at = None;
        self.bms_activation_followup_attempts = 0;
        self.bms_activation_exercise_next_at = None;
        self.bms_activation_pattern_tracker = Bq40ActivationPatternTracker::new();
        self.bms_activation_isolation_until = None;
        self.bms_activation_force_charge_requested = false;
        self.bms_activation_current_is_auto = false;
        let request_kind = self.bms_activation_request_kind;
        self.bms_activation_request_kind = BmsRecoveryRequestKind::Activation;
        self.bms_activation_state = BmsActivationState::Result(result);
        self.ui_snapshot.bq40z50_last_result = Some(result);
        self.chg_next_poll_at = Instant::now();
        self.maybe_restore_charger_watchdog_after_activation();
        match result {
            BmsResultKind::Success => defmt::info!(
                "bms: activation finish request={} result={} reason={} bq40_state={} soc_pct={=?} rca_alarm={=?} dsg_ready={=?} charger_state={} allow_charge={=?} vbat_present={=?} input_present={=?} restore_chg_enabled={=bool}",
                request_kind.request_name(),
                bms_result_name(result),
                reason,
                self_check_comm_state_name(self.ui_snapshot.bq40z50),
                self.ui_snapshot.bq40z50_soc_pct,
                self.ui_snapshot.bq40z50_rca_alarm,
                self.ui_snapshot.bq40z50_discharge_ready,
                self_check_comm_state_name(self.ui_snapshot.bq25792),
                self.ui_snapshot.bq25792_allow_charge,
                self.ui_snapshot.bq25792_vbat_present,
                self.ui_snapshot.fusb302_vbus_present,
                restore_chg_enabled
            ),
            _ => defmt::warn!(
                "bms: activation finish request={} result={} reason={} bq40_state={} soc_pct={=?} rca_alarm={=?} dsg_ready={=?} charger_state={} allow_charge={=?} vbat_present={=?} input_present={=?} restore_chg_enabled={=bool}",
                request_kind.request_name(),
                bms_result_name(result),
                reason,
                self_check_comm_state_name(self.ui_snapshot.bq40z50),
                self.ui_snapshot.bq40z50_soc_pct,
                self.ui_snapshot.bq40z50_rca_alarm,
                self.ui_snapshot.bq40z50_discharge_ready,
                self_check_comm_state_name(self.ui_snapshot.bq25792),
                self.ui_snapshot.bq25792_allow_charge,
                self.ui_snapshot.bq25792_vbat_present,
                self.ui_snapshot.fusb302_vbus_present,
                restore_chg_enabled
            ),
        }
        if result != BmsResultKind::Success {
            if let Some(addr) = self.bms_addr {
                log_bq40_block_detail(&mut self.i2c, addr, "activation_finish_blocked", None);
            }
        }
    }

    fn maybe_log_bq40_config_runtime(&mut self, addr: u8, op_status: Option<u32>) {
        if self.bms_config_logged {
            return;
        }
        let now = Instant::now();
        if now < self.bms_next_config_log_at {
            return;
        }
        if log_bq40_config_detail(&mut self.i2c, addr, "runtime_config", op_status) {
            self.bms_config_logged = true;
        } else {
            self.bms_next_config_log_at = now + BMS_CONFIG_LOG_RETRY_PERIOD;
        }
    }

    fn maybe_log_bq40_block_detail_runtime(
        &mut self,
        addr: u8,
        primary_reason: &'static str,
        op_status: Option<u32>,
    ) {
        let should_log = matches!(primary_reason, "xdsg_blocked" | "xchg_blocked");
        let now = Instant::now();
        if !should_log {
            self.bms_next_block_detail_log_at = now;
            return;
        }
        if now < self.bms_next_block_detail_log_at {
            return;
        }
        log_bq40_block_detail(&mut self.i2c, addr, "runtime_blocked", op_status);
        self.bms_next_block_detail_log_at = now + BMS_BLOCK_DETAIL_LOG_PERIOD;
    }

    fn output_path_diag_next_log_at_mut(&mut self, ch: OutputChannel) -> &mut Instant {
        match ch {
            OutputChannel::OutA => &mut self.out_a_next_path_diag_log_at,
            OutputChannel::OutB => &mut self.out_b_next_path_diag_log_at,
        }
    }

    fn note_output_path_diag(
        &mut self,
        ch: OutputChannel,
        stage: &'static str,
        output_enabled: Option<bool>,
        vbus_mv: Option<u16>,
        current_ma: Option<i32>,
        fault_active: bool,
        status_sample: Option<u8>,
        rate_limit: bool,
    ) -> bool {
        let anomaly = output_not_rising_anomaly(
            self.cfg.vout_mv,
            vbus_mv,
            current_ma,
            output_enabled,
            fault_active,
        );
        let now = Instant::now();
        let target_vout_mv = self.cfg.vout_mv;
        let pack_mv = self.ui_snapshot.bq40z50_pack_mv;
        let next_log_at = self.output_path_diag_next_log_at_mut(ch);
        if !anomaly {
            *next_log_at = now;
            return false;
        }
        if rate_limit && now < *next_log_at {
            return true;
        }
        log_output_path_diag(
            ch,
            stage,
            target_vout_mv,
            pack_mv,
            vbus_mv,
            current_ma,
            output_enabled,
            status_sample,
        );
        *next_log_at = now + OUTPUT_PATH_DIAG_LOG_PERIOD;
        true
    }

    fn is_bq40_rom_mode_detected(&mut self) -> bool {
        for addr in bms_probe_candidates().iter().copied() {
            match bq40z50::read_u16(&mut self.i2c, addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE) {
                Ok(sig) if sig == BMS_ROM_MODE_SIGNATURE => {
                    defmt::warn!("bms: bq40z50 rom_mode_detected addr=0x{=u8:x}", addr);
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    fn current_mains_present(&self) -> Option<bool> {
        stable_mains_present(
            self.ui_snapshot.vin_mains_present,
            self.ui_snapshot.vin_vbus_mv,
            self.ui_snapshot.fusb302_vbus_present,
        )
    }

    fn current_output_ilimit_ma(&self) -> u16 {
        self.output_protection.applied_ilim_ma
    }

    fn output_protection_config(&self) -> output_protection::ProtectionConfig {
        output_protection::ProtectionConfig {
            tmp_temp_enter_c_x16: self.cfg.protect_tmp_temp_derate_c_x16,
            tmp_temp_exit_c_x16: self.cfg.protect_tmp_temp_resume_c_x16,
            tmp_temp_shutdown_c_x16: self.cfg.protect_tmp_temp_shutdown_c_x16,
            other_temp_enter_c_x16: self.cfg.protect_other_temp_derate_c_x16,
            other_temp_exit_c_x16: self.cfg.protect_other_temp_resume_c_x16,
            other_temp_shutdown_c_x16: self.cfg.protect_other_temp_shutdown_c_x16,
            temp_hold_ms: self.cfg.protect_temp_hold.as_millis() as u64,
            current_enter_ma: self.cfg.protect_current_derate_ma,
            current_exit_ma: self.cfg.protect_current_resume_ma,
            current_hold_ms: self.cfg.protect_current_hold.as_millis() as u64,
            ilim_step_ma: self.cfg.protect_ilim_step_ma,
            ilim_step_interval_ms: self.cfg.protect_ilim_step_interval.as_millis() as u64,
            min_ilim_ma: self.cfg.protect_min_ilim_ma,
            shutdown_vout_mv: self.cfg.protect_shutdown_vout_mv,
            shutdown_hold_ms: self.cfg.protect_shutdown_hold.as_millis() as u64,
        }
    }

    fn output_gate_reason_now(&mut self) -> OutputGateReason {
        if self.therm_kill.is_low() {
            return OutputGateReason::ThermKill;
        }

        if self.bms_addr.is_none() || self.ui_snapshot.bq40z50_discharge_ready != Some(true) {
            return OutputGateReason::BmsNotReady;
        }

        for ch in [OutputChannel::OutA, OutputChannel::OutB] {
            if !self.output_state.requested_outputs.is_enabled(ch) {
                continue;
            }
            let latch = self.tps_fault_latch(ch);
            if latch.config_failure_active() {
                return OutputGateReason::TpsConfigFailed;
            }
            if latch.fault_active() {
                return OutputGateReason::TpsFault;
            }
        }

        if self.output_protection.phase == output_protection::ProtectionPhase::Shutdown {
            return OutputGateReason::ActiveProtection;
        }

        OutputGateReason::None
    }

    fn apply_output_gate(&mut self, gate_reason: OutputGateReason) {
        if gate_reason == OutputGateReason::None {
            return;
        }
        let next_state = output_state_gate_transition(self.output_state, gate_reason);
        if next_state == self.output_state {
            return;
        }
        self.output_state = next_state;
        self.force_disable_outputs();
        defmt::warn!(
            "power: outputs gated reason={} recoverable_outputs={} requested_outputs={}",
            gate_reason.as_str(),
            self.output_state.recoverable_outputs.describe(),
            self.output_state.requested_outputs.describe()
        );
    }

    fn reconcile_output_state(&mut self) {
        let gate_reason = self.output_gate_reason_now();
        if gate_reason != OutputGateReason::None {
            self.apply_output_gate(gate_reason);
            return;
        }

        if self.output_state.gate_reason != OutputGateReason::None {
            defmt::info!(
                "power: outputs gate cleared previous_reason={} recoverable_outputs={} mains_present={=?}",
                self.output_state.gate_reason.as_str(),
                self.output_state.recoverable_outputs.describe(),
                self.current_mains_present()
            );
            self.output_state =
                output_state_gate_transition(self.output_state, OutputGateReason::None);
        }
    }

    #[allow(dead_code)]
    fn can_request_output_restore(&self) -> bool {
        output_restore_pending_from_state(self.output_state, self.current_mains_present())
    }

    fn apply_output_current_limit(&mut self, limit_ma: u16) {
        if self.output_state.active_outputs == EnabledOutputs::Both {
            let out_a =
                ::tps55288::Tps55288::with_address(&mut self.i2c, OutputChannel::OutA.addr())
                    .set_ilim_ma(limit_ma, true)
                    .map_err(tps_error_kind);
            let out_b =
                ::tps55288::Tps55288::with_address(&mut self.i2c, OutputChannel::OutB.addr())
                    .set_ilim_ma(limit_ma, true)
                    .map_err(tps_error_kind);

            match (out_a, out_b) {
                (Ok(()), Ok(())) => {
                    defmt::warn!(
                        "power: active_protection set_ilim ch=out_a+out_b limit_ma={=u16}",
                        limit_ma
                    );
                }
                (res_a, res_b) => {
                    defmt::warn!(
                        "power: active_protection set_ilim_pair_failed limit_ma={=u16} out_a={=?} out_b={=?}; force_disable_outputs",
                        limit_ma,
                        res_a,
                        res_b
                    );
                    self.force_disable_outputs();
                }
            }
            return;
        }

        for ch in [OutputChannel::OutA, OutputChannel::OutB] {
            if !self.output_state.active_outputs.is_enabled(ch) {
                continue;
            }

            let result = ::tps55288::Tps55288::with_address(&mut self.i2c, ch.addr())
                .set_ilim_ma(limit_ma, true)
                .map_err(tps_error_kind);
            match result {
                Ok(()) => defmt::warn!(
                    "power: active_protection set_ilim ch={} limit_ma={=u16}",
                    ch.name(),
                    limit_ma
                ),
                Err(kind) => defmt::warn!(
                    "power: active_protection set_ilim_failed ch={} limit_ma={=u16} err={}",
                    ch.name(),
                    limit_ma,
                    kind
                ),
            }
        }
    }

    fn update_output_protection(&mut self) {
        if self.output_state.requested_outputs == EnabledOutputs::None {
            return;
        }

        let max_tmp_temp_c_x16 = if self.cfg.thermal_protection_enabled {
            max_optional_temp(self.ui_snapshot.tmp_a_c_x16, self.ui_snapshot.tmp_b_c_x16)
        } else {
            None
        };
        let max_other_temp_c_x16 = if self.cfg.thermal_protection_enabled {
            self.shared_bms_thermal_max_c_x16()
        } else {
            None
        };
        let max_temp_c_x16 = max_optional_temp(max_tmp_temp_c_x16, max_other_temp_c_x16);
        let mut max_current_ma = None;
        let mut min_vout_mv = None;

        for ch in [OutputChannel::OutA, OutputChannel::OutB] {
            if !self.output_state.requested_outputs.is_enabled(ch) {
                continue;
            }

            let current = match ch {
                OutputChannel::OutA => self.ui_snapshot.tps_a_iout_ma,
                OutputChannel::OutB => self.ui_snapshot.tps_b_iout_ma,
            };
            if let Some(current_ma) = current {
                let current_ma = current_ma.max(0);
                max_current_ma =
                    Some(max_current_ma.map_or(current_ma, |cur: i32| cur.max(current_ma)));
            }

            let vout = match ch {
                OutputChannel::OutA => self.ui_snapshot.out_a_vbus_mv,
                OutputChannel::OutB => self.ui_snapshot.out_b_vbus_mv,
            };
            if let Some(vout_mv) = vout {
                min_vout_mv = Some(min_vout_mv.map_or(vout_mv, |cur: u16| cur.min(vout_mv)));
            }
        }

        let result = output_protection::step(
            self.fan_now_ms(),
            self.output_protection_config(),
            self.cfg.ilimit_ma,
            self.output_protection,
            output_protection::ProtectionInputs {
                max_tmp_temp_c_x16,
                max_other_temp_c_x16,
                max_current_ma,
                min_vout_mv,
            },
        );

        let prev = self.output_protection;
        self.output_protection = result.runtime;
        match result.action {
            output_protection::ProtectionAction::None => {}
            output_protection::ProtectionAction::ApplyIlim(limit_ma) => {
                self.apply_output_current_limit(limit_ma);
                defmt::warn!(
                    "power: active_protection derating reason={} ilim_ma={=u16} max_tmp_temp_c_x16={=?} max_other_temp_c_x16={=?} max_temp_c_x16={=?} max_current_ma={=?} min_vout_mv={=?}",
                    self.output_protection.status.reason().as_str(),
                    limit_ma,
                    max_tmp_temp_c_x16,
                    max_other_temp_c_x16,
                    max_temp_c_x16,
                    max_current_ma,
                    min_vout_mv
                );
            }
            output_protection::ProtectionAction::RestoreDefaultIlim(limit_ma) => {
                self.apply_output_current_limit(limit_ma);
                defmt::info!(
                    "power: active_protection cleared restore_ilim_ma={=u16}",
                    limit_ma
                );
            }
            output_protection::ProtectionAction::Shutdown(reason) => {
                defmt::error!(
                    "power: active_protection shutdown reason={} ilim_ma={=u16} min_vout_mv={=?} max_tmp_temp_c_x16={=?} max_other_temp_c_x16={=?} max_temp_c_x16={=?} max_current_ma={=?}",
                    reason.as_str(),
                    prev.applied_ilim_ma,
                    min_vout_mv,
                    max_tmp_temp_c_x16,
                    max_other_temp_c_x16,
                    max_temp_c_x16,
                    max_current_ma
                );
                self.apply_output_gate(OutputGateReason::ActiveProtection);
            }
        }
    }

    fn recompute_ui_mode(&mut self) {
        let has_output = self.ui_snapshot.tps_a_enabled == Some(true)
            || self.ui_snapshot.tps_b_enabled == Some(true);
        self.ui_snapshot.mode = ups_mode_from_mains(
            stable_mains_present(
                self.ui_snapshot.vin_mains_present,
                self.ui_snapshot.vin_vbus_mv,
                self.ui_snapshot.fusb302_vbus_present,
            ),
            has_output,
        );
    }

    fn refresh_audio_signals(&mut self) {
        let hold_no_battery_result = self.should_hold_no_battery_result();
        let mains_state = stable_mains_state(
            self.ui_snapshot.vin_mains_present,
            self.ui_snapshot.vin_vbus_mv,
            self.ui_snapshot.fusb302_vbus_present,
        );
        let mains_present = mains_state.present;
        let tmp_a_hot = self
            .cfg
            .detected_tmp_outputs
            .is_enabled(OutputChannel::OutA)
            && self.ui_snapshot.tmp_a_c.is_some_and(|temp_c| {
                temp_c.saturating_mul(16) >= self.cfg.protect_tmp_temp_derate_c_x16
            });
        let tmp_b_hot = self
            .cfg
            .detected_tmp_outputs
            .is_enabled(OutputChannel::OutB)
            && self.ui_snapshot.tmp_b_c.is_some_and(|temp_c| {
                temp_c.saturating_mul(16) >= self.cfg.protect_tmp_temp_derate_c_x16
            });
        let raw_battery_low = match self.bms_audio.rca_alarm {
            Some(true) => match mains_present {
                Some(true) => AudioBatteryLowState::WithMains,
                Some(false) => AudioBatteryLowState::NoMains,
                None => AudioBatteryLowState::Unknown,
            },
            Some(false) => AudioBatteryLowState::Inactive,
            None => AudioBatteryLowState::Unknown,
        };
        let module_fault = if hold_no_battery_result {
            false
        } else {
            (self.cfg.charger_probe_ok && self.charger_audio.module_fault)
                || (self.bms_runtime_seen && self.bms_audio.module_fault)
                || (self.cfg.ina_detected
                    && matches!(self.ui_snapshot.ina3221, SelfCheckCommState::Err))
                || (self
                    .cfg
                    .detected_tps_outputs
                    .is_enabled(OutputChannel::OutA)
                    && matches!(self.ui_snapshot.tps_a, SelfCheckCommState::Err))
                || (self
                    .cfg
                    .detected_tps_outputs
                    .is_enabled(OutputChannel::OutB)
                    && matches!(self.ui_snapshot.tps_b, SelfCheckCommState::Err))
                || (self
                    .cfg
                    .detected_tmp_outputs
                    .is_enabled(OutputChannel::OutA)
                    && matches!(self.ui_snapshot.tmp_a, SelfCheckCommState::Err))
                || (self
                    .cfg
                    .detected_tmp_outputs
                    .is_enabled(OutputChannel::OutB)
                    && matches!(self.ui_snapshot.tmp_b, SelfCheckCommState::Err))
        };
        let therm_kill_asserted = self.therm_kill.is_low();
        let battery_protection = if hold_no_battery_result {
            false
        } else {
            self.bms_audio.protection_active
        };
        let battery_low = if battery_protection {
            AudioBatteryLowState::Inactive
        } else {
            raw_battery_low
        };
        let snapshot = AudioSignalSnapshot {
            mains_present,
            mains_source: mains_state.source,
            charge_phase: self.charger_audio.phase,
            thermal_stress: self.charger_audio.thermal_stress || tmp_a_hot || tmp_b_hot,
            battery_low,
            battery_protection,
            module_fault,
            io_over_voltage: self.charger_audio.over_voltage || self.tps_audio.any_over_voltage(),
            io_over_current: self.charger_audio.over_current || self.tps_audio.any_over_current(),
            shutdown_protection: therm_kill_asserted
                || self.charger_audio.shutdown_protection
                || self.output_protection.phase == output_protection::ProtectionPhase::Shutdown,
        };

        if !self.audio_signals_ready {
            self.audio_snapshot = snapshot;
            self.audio_events = AudioSignalEvents::default();
            self.audio_signals_ready = true;
            return;
        }

        let prev = self.audio_snapshot;
        if let Some(edge) = mains_present_edge(
            StableMainsState {
                present: prev.mains_present,
                source: prev.mains_source,
            },
            StableMainsState {
                present: snapshot.mains_present,
                source: snapshot.mains_source,
            },
        ) {
            self.audio_events.mains_present_changed = Some(edge);
        }
        if prev.charge_phase != AudioChargePhase::Unknown
            && snapshot.charge_phase != AudioChargePhase::Unknown
            && prev.charge_phase != snapshot.charge_phase
        {
            self.audio_events.charge_phase_changed = Some(snapshot.charge_phase);
        }
        if prev.thermal_stress != snapshot.thermal_stress {
            self.audio_events.thermal_stress_changed = Some(snapshot.thermal_stress);
        }
        if prev.battery_low != snapshot.battery_low {
            self.audio_events.battery_low_changed = Some(snapshot.battery_low);
            defmt::info!(
                "audio: battery_low changed old={} new={} raw_new={} suppressed_by_battery_protection={=bool}",
                audio_battery_low_state_name(prev.battery_low),
                audio_battery_low_state_name(snapshot.battery_low),
                audio_battery_low_state_name(raw_battery_low),
                battery_protection && raw_battery_low != AudioBatteryLowState::Inactive
            );
        }
        if prev.battery_protection != snapshot.battery_protection {
            self.audio_events.battery_protection_changed = Some(snapshot.battery_protection);
        }
        if prev.module_fault != snapshot.module_fault {
            self.audio_events.module_fault_changed = Some(snapshot.module_fault);
            defmt::info!(
                "audio: module_fault changed old={=bool} new={=bool} hold_no_battery={=bool} bq40_last_result={} bq40_state={} tps_a={} tps_b={} bms_audio_fault={=bool}",
                prev.module_fault,
                snapshot.module_fault,
                hold_no_battery_result,
                bms_result_option_name(self.ui_snapshot.bq40z50_last_result),
                self_check_comm_state_name(self.ui_snapshot.bq40z50),
                self_check_comm_state_name(self.ui_snapshot.tps_a),
                self_check_comm_state_name(self.ui_snapshot.tps_b),
                self.bms_audio.module_fault
            );
        }
        if prev.io_over_voltage != snapshot.io_over_voltage {
            self.audio_events.io_over_voltage_changed = Some(snapshot.io_over_voltage);
        }
        if prev.io_over_current != snapshot.io_over_current {
            self.audio_events.io_over_current_changed = Some(snapshot.io_over_current);
        }
        if prev.shutdown_protection != snapshot.shutdown_protection {
            self.audio_events.shutdown_protection_changed = Some(snapshot.shutdown_protection);
        }
        self.audio_snapshot = snapshot;
    }

    fn should_hold_no_battery_result(&self) -> bool {
        self.ui_snapshot.bq40z50_last_result == Some(BmsResultKind::NoBattery)
            && self.bms_activation_state != BmsActivationState::Pending
    }

    fn hold_no_battery_result_audio_state(&mut self) {
        self.ui_snapshot.bq40z50 = SelfCheckCommState::Warn;
        self.ui_snapshot.bq40z50_pack_mv = None;
        self.ui_snapshot.bq40z50_current_ma = None;
        self.reset_bms_detail_mac_cache(Instant::now());
        self.clear_bms_detail_snapshot();
        if self.ui_snapshot.bq40z50_soc_pct.is_none() {
            self.ui_snapshot.bq40z50_soc_pct = Some(0);
        }
        self.ui_snapshot.bq40z50_rca_alarm = Some(true);
        self.ui_snapshot.bq40z50_no_battery = Some(true);
        self.ui_snapshot.bq40z50_discharge_ready = Some(false);
        self.ui_snapshot.bq40z50_issue_detail = Some("no_battery");
        self.bms_audio = BmsAudioState {
            rca_alarm: Some(true),
            protection_active: false,
            module_fault: false,
        };
    }

    fn refresh_tps_audio_state(&mut self) {
        for ch in [OutputChannel::OutA, OutputChannel::OutB] {
            if !self.cfg.detected_tps_outputs.is_enabled(ch) {
                continue;
            }
            let latch = self.tps_fault_latch(ch);
            let over_voltage = latch.over_voltage();
            let over_current = latch.over_current();
            match ch {
                OutputChannel::OutA => {
                    self.tps_audio.out_a_over_voltage = over_voltage;
                    self.tps_audio.out_a_over_current = over_current;
                }
                OutputChannel::OutB => {
                    self.tps_audio.out_b_over_voltage = over_voltage;
                    self.tps_audio.out_b_over_current = over_current;
                }
            }
        }
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

        if self.output_state.active_outputs == EnabledOutputs::Both {
            let retry_due = (!self.tps_a_ready
                && self.tps_a_next_retry_at.is_some_and(|t| now >= t))
                || (!self.tps_b_ready && self.tps_b_next_retry_at.is_some_and(|t| now >= t));
            if retry_due {
                self.tps_a_next_retry_at = None;
                self.tps_b_next_retry_at = None;
                self.try_configure_tps_pair();
            }
            return;
        }

        if !self.tps_a_ready
            && self
                .output_state
                .active_outputs
                .is_enabled(OutputChannel::OutA)
            && self.tps_a_next_retry_at.is_some_and(|t| now >= t)
        {
            self.tps_a_next_retry_at = None;
            self.try_configure_tps(OutputChannel::OutA);
        }

        if !self.tps_b_ready
            && self
                .output_state
                .active_outputs
                .is_enabled(OutputChannel::OutB)
            && self.tps_b_next_retry_at.is_some_and(|t| now >= t)
        {
            self.tps_b_next_retry_at = None;
            self.try_configure_tps(OutputChannel::OutB);
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
                self.ui_snapshot.ina3221 = SelfCheckCommState::Ok;
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
                self.ui_snapshot.ina3221 = SelfCheckCommState::Err;
                self.ina_next_retry_at = Some(Instant::now() + self.cfg.retry_backoff);
                defmt::error!("power: ina3221 err={}", ina_error_kind(e));
            }
        }
    }

    fn try_configure_requested_tps(&mut self) {
        match self.output_state.active_outputs {
            EnabledOutputs::Both => self.try_configure_tps_pair(),
            EnabledOutputs::Only(ch) => self.try_configure_tps(ch),
            EnabledOutputs::None => {}
        }
    }

    fn try_configure_tps(&mut self, ch: OutputChannel) {
        let enabled = self.output_state.active_outputs.is_enabled(ch);
        let addr = ch.addr();
        let ilimit_ma = self.current_output_ilimit_ma();

        match tps55288::configure_one(&mut self.i2c, ch, enabled, self.cfg.vout_mv, ilimit_ma) {
            Ok(()) => {
                if enabled {
                    self.clear_tps_fault_latch(ch);
                }
                tps55288::log_configured(&mut self.i2c, ch, enabled, enabled);
                self.mark_tps_ok(ch);
                match ch {
                    OutputChannel::OutA => {
                        self.ui_snapshot.tps_a = SelfCheckCommState::Ok;
                        self.ui_snapshot.tps_a_enabled = Some(enabled);
                    }
                    OutputChannel::OutB => {
                        self.ui_snapshot.tps_b = SelfCheckCommState::Ok;
                        self.ui_snapshot.tps_b_enabled = Some(enabled);
                    }
                }
            }
            Err((stage, e)) => {
                let kind = tps_error_kind(e);
                let consecutive_failures = self
                    .tps_fault_latch_mut(ch)
                    .record_config_failure(stage.as_str(), kind);
                let decision = output_retry::tps_config_retry_decision(
                    kind,
                    consecutive_failures,
                    TPS_CONFIG_MAX_RETRY_ATTEMPTS,
                );
                if matches!(decision, TpsConfigRetryDecision::Latch) {
                    self.tps_fault_latch_mut(ch).latch_config_failure();
                }
                let next_retry = matches!(decision, TpsConfigRetryDecision::Retry)
                    .then_some(Instant::now() + self.cfg.retry_backoff);
                self.mark_tps_failed(ch, next_retry);
                match ch {
                    OutputChannel::OutA => {
                        self.ui_snapshot.tps_a = SelfCheckCommState::Err;
                        self.ui_snapshot.tps_a_enabled = Some(false);
                    }
                    OutputChannel::OutB => {
                        self.ui_snapshot.tps_b = SelfCheckCommState::Err;
                        self.ui_snapshot.tps_b_enabled = Some(false);
                    }
                }
                log_tps_config_retry_decision(
                    ch,
                    addr,
                    stage.as_str(),
                    kind,
                    consecutive_failures,
                    decision,
                    self.cfg.retry_backoff,
                );
                if kind == "i2c_nack" && ch == OutputChannel::OutB {
                    defmt::warn!(
                        "power: tps addr=0x75 nack_hint=maybe_address_changed; power-cycle TPS rails to restore preset address"
                    );
                }
            }
        }
        self.recompute_ui_mode();
    }

    fn try_configure_tps_pair(&mut self) {
        let ilimit_ma = self.current_output_ilimit_ma();

        let prepare_a = tps55288::prepare_enabled_output(
            &mut self.i2c,
            OutputChannel::OutA,
            self.cfg.vout_mv,
            ilimit_ma,
        );
        if let Err((stage, e)) = prepare_a {
            self.handle_tps_joint_failure(OutputChannel::OutA, stage, e);
            self.recompute_ui_mode();
            return;
        }

        let prepare_b = tps55288::prepare_enabled_output(
            &mut self.i2c,
            OutputChannel::OutB,
            self.cfg.vout_mv,
            ilimit_ma,
        );
        if let Err((stage, e)) = prepare_b {
            self.handle_tps_joint_failure(OutputChannel::OutB, stage, e);
            self.recompute_ui_mode();
            return;
        }

        if let Err((stage, e)) = tps55288::enable_output_only(&mut self.i2c, OutputChannel::OutA) {
            self.handle_tps_joint_failure(OutputChannel::OutA, stage, e);
            self.recompute_ui_mode();
            return;
        }

        if let Err((stage, e)) = tps55288::enable_output_only(&mut self.i2c, OutputChannel::OutB) {
            self.handle_tps_joint_failure(OutputChannel::OutB, stage, e);
            self.recompute_ui_mode();
            return;
        }

        self.clear_tps_fault_latch(OutputChannel::OutA);
        self.clear_tps_fault_latch(OutputChannel::OutB);
        tps55288::log_configured(&mut self.i2c, OutputChannel::OutA, true, true);
        tps55288::log_configured(&mut self.i2c, OutputChannel::OutB, true, true);
        self.mark_tps_ok(OutputChannel::OutA);
        self.mark_tps_ok(OutputChannel::OutB);
        self.ui_snapshot.tps_a = SelfCheckCommState::Ok;
        self.ui_snapshot.tps_b = SelfCheckCommState::Ok;
        self.ui_snapshot.tps_a_enabled = Some(true);
        self.ui_snapshot.tps_b_enabled = Some(true);
        self.recompute_ui_mode();
    }

    fn handle_tps_joint_failure(
        &mut self,
        failed_ch: OutputChannel,
        stage: tps55288::ConfigureStage,
        e: ::tps55288::Error<esp_hal::i2c::master::Error>,
    ) {
        let kind = tps_error_kind(e);
        let consecutive_failures = self
            .tps_fault_latch_mut(failed_ch)
            .record_config_failure(stage.as_str(), kind);
        let decision = output_retry::tps_config_retry_decision(
            kind,
            consecutive_failures,
            TPS_CONFIG_MAX_RETRY_ATTEMPTS,
        );
        if matches!(decision, TpsConfigRetryDecision::Latch) {
            self.tps_fault_latch_mut(failed_ch).latch_config_failure();
        }
        let next_retry = matches!(decision, TpsConfigRetryDecision::Retry)
            .then_some(Instant::now() + self.cfg.retry_backoff);

        let _ = tps55288::disable_output_only(&mut self.i2c, OutputChannel::OutA);
        let _ = tps55288::disable_output_only(&mut self.i2c, OutputChannel::OutB);

        self.mark_tps_failed(OutputChannel::OutA, next_retry);
        self.mark_tps_failed(OutputChannel::OutB, next_retry);
        self.ui_snapshot.tps_a_enabled = Some(false);
        self.ui_snapshot.tps_b_enabled = Some(false);
        match failed_ch {
            OutputChannel::OutA => self.ui_snapshot.tps_a = SelfCheckCommState::Err,
            OutputChannel::OutB => self.ui_snapshot.tps_b = SelfCheckCommState::Err,
        }
        log_tps_config_retry_decision(
            failed_ch,
            failed_ch.addr(),
            stage.as_str(),
            kind,
            consecutive_failures,
            decision,
            self.cfg.retry_backoff,
        );
        if kind == "i2c_nack" && failed_ch == OutputChannel::OutB {
            defmt::warn!(
                "power: tps addr=0x75 nack_hint=maybe_address_changed; power-cycle TPS rails to restore preset address"
            );
        }
    }

    fn mark_tps_ok(&mut self, ch: OutputChannel) {
        match ch {
            OutputChannel::OutA => {
                self.tps_a_ready = true;
                self.tps_a_next_retry_at = None;
            }
            OutputChannel::OutB => {
                self.tps_b_ready = true;
                self.tps_b_next_retry_at = None;
            }
        }
    }

    fn mark_tps_failed(&mut self, ch: OutputChannel, next: Option<Instant>) {
        match ch {
            OutputChannel::OutA => {
                self.tps_a_ready = false;
                self.tps_a_next_retry_at = next;
            }
            OutputChannel::OutB => {
                self.tps_b_ready = false;
                self.tps_b_next_retry_at = next;
            }
        }
    }

    fn tps_fault_latch(&self, ch: OutputChannel) -> TpsFaultLatch {
        match ch {
            OutputChannel::OutA => self.tps_a_fault_latch,
            OutputChannel::OutB => self.tps_b_fault_latch,
        }
    }

    fn tps_fault_latch_mut(&mut self, ch: OutputChannel) -> &mut TpsFaultLatch {
        match ch {
            OutputChannel::OutA => &mut self.tps_a_fault_latch,
            OutputChannel::OutB => &mut self.tps_b_fault_latch,
        }
    }

    fn clear_tps_fault_latch(&mut self, ch: OutputChannel) {
        self.tps_fault_latch_mut(ch).clear();
    }

    fn record_tps_fault_status(&mut self, ch: OutputChannel, status: u8) {
        self.tps_fault_latch_mut(ch).record_status(status);
    }

    fn maybe_handle_fault(&mut self, irq: &IrqSnapshot) {
        if self.output_state.requested_outputs == EnabledOutputs::None {
            return;
        }

        let now = Instant::now();
        if self.i2c1_int.is_low() || irq.i2c1_int != 0 {
            let should_log = tps55288::should_log_fault(
                now,
                &mut self.last_fault_log_at,
                self.cfg.fault_log_min_interval,
            );
            if self
                .output_state
                .requested_outputs
                .is_enabled(OutputChannel::OutA)
            {
                let status = if should_log {
                    tps55288::log_fault_status(&mut self.i2c, OutputChannel::OutA, self.ina_ready)
                } else {
                    tps55288::read_status_snapshot(&mut self.i2c, OutputChannel::OutA)
                };
                if let Some(status) = status {
                    self.record_tps_fault_status(OutputChannel::OutA, status);
                }
            }
            if self
                .output_state
                .requested_outputs
                .is_enabled(OutputChannel::OutB)
            {
                let status = if should_log {
                    tps55288::log_fault_status(&mut self.i2c, OutputChannel::OutB, self.ina_ready)
                } else {
                    tps55288::read_status_snapshot(&mut self.i2c, OutputChannel::OutB)
                };
                if let Some(status) = status {
                    self.record_tps_fault_status(OutputChannel::OutB, status);
                }
            }
            self.refresh_tps_audio_state();
        }
    }

    fn note_skipped_vin_telemetry_if_due(&mut self, now: Instant) {
        if now < self.next_vin_telemetry_skip_at {
            return;
        }
        self.next_vin_telemetry_skip_at = now + self.cfg.telemetry_period;
        mark_vin_telemetry_unavailable(
            self.cfg.telemetry_include_vin_ch3,
            &mut self.ui_snapshot.vin_vbus_mv,
            &mut self.ui_snapshot.vin_iin_ma,
            &mut self.ui_snapshot.vin_mains_present,
            &mut self.vin_sample_missing_streak,
        );
        self.recompute_ui_mode();
    }

    fn refresh_vin_telemetry(&mut self, now: Instant) {
        self.next_vin_telemetry_skip_at = now + self.cfg.telemetry_period;
        if self.cfg.telemetry_include_vin_ch3 {
            if self.ina_ready {
                let bus = ina3221::read_bus_mv(&mut self.i2c, ina3221::Channel::Ch3);
                let shunt = ina3221::read_shunt_uv(&mut self.i2c, ina3221::Channel::Ch3);
                let vbus_mv = match bus {
                    Ok(v) => {
                        self.ui_snapshot.vin_vbus_mv = u16::try_from(v).ok();
                        self.ui_snapshot.vin_mains_present =
                            mains_present_from_vin(self.ui_snapshot.vin_vbus_mv);
                        self.vin_sample_missing_streak = 0;
                        TelemetryValue::Value(v)
                    }
                    Err(e) => {
                        mark_vin_telemetry_unavailable(
                            self.cfg.telemetry_include_vin_ch3,
                            &mut self.ui_snapshot.vin_vbus_mv,
                            &mut self.ui_snapshot.vin_iin_ma,
                            &mut self.ui_snapshot.vin_mains_present,
                            &mut self.vin_sample_missing_streak,
                        );
                        TelemetryValue::Err(ina_error_kind(e))
                    }
                };
                let current_ma = match shunt {
                    Ok(shunt_uv) => {
                        let current_ma = ina3221::shunt_uv_to_current_ma(shunt_uv, 7);
                        self.ui_snapshot.vin_iin_ma = Some(current_ma);
                        TelemetryValue::Value(current_ma)
                    }
                    Err(e) => {
                        self.ui_snapshot.vin_iin_ma = None;
                        TelemetryValue::Err(ina_error_kind(e))
                    }
                };
                defmt::info!(
                    "telemetry ch=vin addr=0x40 vbus_mv={} current_ma={}",
                    vbus_mv,
                    current_ma
                );
            } else {
                mark_vin_telemetry_unavailable(
                    self.cfg.telemetry_include_vin_ch3,
                    &mut self.ui_snapshot.vin_vbus_mv,
                    &mut self.ui_snapshot.vin_iin_ma,
                    &mut self.ui_snapshot.vin_mains_present,
                    &mut self.vin_sample_missing_streak,
                );
                defmt::info!(
                    "telemetry ch=vin addr=0x40 vbus_mv={} current_ma={}",
                    TelemetryValue::Err("ina_uninit"),
                    TelemetryValue::Err("ina_uninit")
                );
            }
        } else {
            mark_vin_telemetry_unavailable(
                self.cfg.telemetry_include_vin_ch3,
                &mut self.ui_snapshot.vin_vbus_mv,
                &mut self.ui_snapshot.vin_iin_ma,
                &mut self.ui_snapshot.vin_mains_present,
                &mut self.vin_sample_missing_streak,
            );
        }
    }

    fn maybe_print_telemetry(&mut self) -> bool {
        let now = Instant::now();
        if now < self.next_telemetry_at {
            return false;
        }
        self.next_telemetry_at = now + self.cfg.telemetry_period;

        if self.output_state.requested_outputs == EnabledOutputs::None {
            self.refresh_tmp112_snapshot(OutputChannel::OutA);
            self.refresh_tmp112_snapshot(OutputChannel::OutB);
            self.next_fan_temp_refresh_at = now + self.cfg.telemetry_period;
            self.refresh_vin_telemetry(now);
            self.recompute_ui_mode();
            return true;
        }

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

        if self
            .output_state
            .requested_outputs
            .is_enabled(OutputChannel::OutA)
        {
            let fault_latch = self.tps_fault_latch(OutputChannel::OutA);
            let capture = tps55288::print_telemetry_line(
                &mut self.i2c,
                OutputChannel::OutA,
                self.ina_ready,
                therm_kill_n,
                fault_latch.last_status,
                fault_latch.fault_active(),
                self.i2c1_int.is_high(),
            );
            if let Some(status) = capture.status_sample {
                self.record_tps_fault_status(OutputChannel::OutA, status);
            }
            let _ = self.note_output_path_diag(
                OutputChannel::OutA,
                "runtime",
                capture.output_enabled,
                capture.vbus_mv,
                capture.current_ma,
                capture.fault_active,
                capture.status_sample,
                true,
            );
            self.ui_snapshot.tps_a = if !capture.comm_ok {
                SelfCheckCommState::Err
            } else if fault_latch.config_failure_active() {
                SelfCheckCommState::Err
            } else if capture.fault_active {
                SelfCheckCommState::Warn
            } else {
                SelfCheckCommState::Ok
            };
            if let Some(enabled) = capture.output_enabled {
                self.ui_snapshot.tps_a_enabled = Some(enabled);
            }
            self.ui_snapshot.out_a_vbus_mv = capture.vbus_mv;
            self.ui_snapshot.tps_a_iout_ma = capture.current_ma;
            self.ui_snapshot.tmp_a = if capture.temp_c_x16.is_some() {
                SelfCheckCommState::Ok
            } else {
                SelfCheckCommState::Err
            };
            self.ui_snapshot.tmp_a_c_x16 = capture.temp_c_x16;
            self.ui_snapshot.tmp_a_c = capture.temp_c_x16.map(|v| v / 16);
        }
        if self
            .output_state
            .requested_outputs
            .is_enabled(OutputChannel::OutB)
        {
            let fault_latch = self.tps_fault_latch(OutputChannel::OutB);
            let capture = tps55288::print_telemetry_line(
                &mut self.i2c,
                OutputChannel::OutB,
                self.ina_ready,
                therm_kill_n,
                fault_latch.last_status,
                fault_latch.fault_active(),
                self.i2c1_int.is_high(),
            );
            if let Some(status) = capture.status_sample {
                self.record_tps_fault_status(OutputChannel::OutB, status);
            }
            let _ = self.note_output_path_diag(
                OutputChannel::OutB,
                "runtime",
                capture.output_enabled,
                capture.vbus_mv,
                capture.current_ma,
                capture.fault_active,
                capture.status_sample,
                true,
            );
            self.ui_snapshot.tps_b = if !capture.comm_ok {
                SelfCheckCommState::Err
            } else if fault_latch.config_failure_active() {
                SelfCheckCommState::Err
            } else if capture.fault_active {
                SelfCheckCommState::Warn
            } else {
                SelfCheckCommState::Ok
            };
            if let Some(enabled) = capture.output_enabled {
                self.ui_snapshot.tps_b_enabled = Some(enabled);
            }
            self.ui_snapshot.out_b_vbus_mv = capture.vbus_mv;
            self.ui_snapshot.tps_b_iout_ma = capture.current_ma;
            self.ui_snapshot.tmp_b = if capture.temp_c_x16.is_some() {
                SelfCheckCommState::Ok
            } else {
                SelfCheckCommState::Err
            };
            self.ui_snapshot.tmp_b_c_x16 = capture.temp_c_x16;
            self.ui_snapshot.tmp_b_c = capture.temp_c_x16.map(|v| v / 16);
        } else {
            self.refresh_tmp112_snapshot(OutputChannel::OutB);
        }
        if !self
            .output_state
            .requested_outputs
            .is_enabled(OutputChannel::OutA)
        {
            self.refresh_tmp112_snapshot(OutputChannel::OutA);
        }
        self.next_fan_temp_refresh_at = now + self.cfg.telemetry_period;

        self.refresh_tps_audio_state();

        self.ui_snapshot.ina_total_ma = match (
            self.ui_snapshot.tps_a_iout_ma,
            self.ui_snapshot.tps_b_iout_ma,
        ) {
            (Some(a), Some(b)) => Some(a + b),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };

        self.refresh_vin_telemetry(now);
        self.recompute_ui_mode();
        true
    }

    fn maybe_poll_charger(&mut self, irq: &IrqSnapshot) {
        if !self.charger_allowed {
            if self.manual_charge_runtime.active {
                self.stop_manual_charge_session(ManualChargeStopReason::SafetyBlocked, false);
            }
            let preserve_manual_safety_notice = manual_charge_safety_notice_active(
                self.manual_charge_runtime.last_stop_reason,
                self.manual_charge_runtime.active,
                self.manual_charge_runtime.stop_inhibit,
                true,
            );
            self.ui_snapshot.bq25792_allow_charge = Some(false);
            self.ui_snapshot.bq25792_ichg_ma = None;
            self.ui_snapshot.bq25792_ibat_ma = None;
            self.ui_snapshot.bq25792_vbat_present = None;
            self.clear_charger_detail_snapshot();
            if preserve_manual_safety_notice {
                self.ui_snapshot.dashboard_detail.charger_notice = Some("manual_safety_blocked");
            }
            self.update_manual_charge_ui_snapshot(Instant::now());
            self.recompute_ui_mode();
            return;
        }

        // Keep the charger polling independent from the TPS/INA telemetry period.
        const INT_MIN_INTERVAL: Duration = Duration::from_millis(50);

        let now = Instant::now();
        let activation_pending = self.bms_activation_state == BmsActivationState::Pending;
        let activation_auto_probe_hold_charge = activation_pending
            && self.bms_activation_current_is_auto
            && self.bms_activation_phase == BmsActivationPhase::ProbeWithoutCharge;
        let activation_force_charge = activation_pending
            && self.bms_activation_force_charge_requested
            && (bms_activation_phase_allows_force_charge(self.bms_activation_phase)
                || activation_auto_probe_hold_charge);
        let activation_force_charge_off =
            activation_pending && bms_activation_phase_forces_charge_off(self.bms_activation_phase);
        let poll_period = if activation_pending {
            BMS_ACTIVATION_CHARGER_POLL_PERIOD
        } else {
            Duration::from_secs(1)
        };
        let auto_force_charge = self.cfg.bms_boot_diag_auto_validate
            && !activation_pending
            && self
                .bms_activation_auto_force_charge_until
                .map_or(false, |until| now < until)
            && (!self.bms_activation_auto_attempted || BMS_BOOT_DIAG_TOOL_STYLE_PROBE_ONLY);
        let boot_diag_ship_reset_due = self.cfg.bms_boot_diag_auto_validate
            && !activation_pending
            && !self.bms_activation_auto_attempted
            && cfg!(feature = "bms-dual-probe-diag")
            && !self.bms_boot_diag_ship_reset_attempted
            && self.bms_addr.is_none()
            && now >= self.bms_boot_diag_started_at + BMS_BOOT_DIAG_SHIP_RESET_DELAY;
        let mut due = now >= self.chg_next_poll_at;
        if irq.chg_int != 0 && !activation_pending && !auto_force_charge {
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
        self.chg_next_poll_at = now + poll_period;

        // Snapshot key registers with multi-byte reads (BQ25792 supports crossing boundaries).
        let mut st = [0u8; 5];
        let mut fault = [0u8; 2];

        let ctrl0 = match bq25792::read_u8(&mut self.i2c, bq25792::reg::CHARGER_CONTROL_0) {
            Ok(v) => v,
            Err(e) => {
                self.mark_charger_poll_failed(now);
                defmt::error!(
                    "charger: bq25792 err stage=ctrl0_read err={}",
                    i2c_error_kind(e)
                );
                return;
            }
        };
        let termination_ctrl =
            bq25792::read_u16(&mut self.i2c, bq25792::reg::TERMINATION_CONTROL).ok();
        let en_term = (ctrl0 & bq25792::ctrl0::EN_TERM) != 0;
        let iterm_ma = termination_ctrl.map(bq25792::decode_termination_current_ma);

        // Only enforce ship-FET path when charging is policy-enabled.
        let (sfet_present_before, sfet_present_after, ship_mode_before, ship_mode_after) =
            if self.cfg.charger_enabled || activation_force_charge || auto_force_charge {
                match bq25792::ensure_ship_fet_path_enabled(&mut self.i2c) {
                    Ok(state) => (
                        (state.ctrl5_before & bq25792::ctrl5::SFET_PRESENT) != 0,
                        (state.ctrl5_after & bq25792::ctrl5::SFET_PRESENT) != 0,
                        state.ship.sdrv_ctrl_before,
                        state.ship.sdrv_ctrl_after,
                    ),
                    Err(e) => {
                        self.mark_charger_poll_failed(now);
                        defmt::error!(
                            "charger: bq25792 err stage=ship_fet_path err={}",
                            i2c_error_kind(e)
                        );
                        return;
                    }
                }
            } else {
                let ctrl5_before = bq25792::read_u8(&mut self.i2c, bq25792::reg::CHARGER_CONTROL_5)
                    .unwrap_or_default();
                let ctrl2_before = bq25792::read_u8(&mut self.i2c, bq25792::reg::CHARGER_CONTROL_2)
                    .unwrap_or_default();
                let sdrv_ctrl_before = (ctrl2_before & bq25792::ctrl2::SDRV_CTRL_MASK)
                    >> bq25792::ctrl2::SDRV_CTRL_SHIFT;
                (
                    (ctrl5_before & bq25792::ctrl5::SFET_PRESENT) != 0,
                    (ctrl5_before & bq25792::ctrl5::SFET_PRESENT) != 0,
                    sdrv_ctrl_before,
                    sdrv_ctrl_before,
                )
            };

        if boot_diag_ship_reset_due {
            self.bms_boot_diag_ship_reset_attempted = true;
            match bq25792::set_sdrv_ctrl_mode(&mut self.i2c, 0b11) {
                Ok(ctrl2_reset) => {
                    spin_delay(BMS_BOOT_DIAG_SHIP_RESET_SETTLE);
                    match bq25792::set_sdrv_ctrl_mode(&mut self.i2c, 0b00) {
                        Ok(ctrl2_idle) => {
                            defmt::info!(
                                "bms_diag: stage=ship_reset_pulse ctrl2_reset=0x{=u8:x} ctrl2_idle=0x{=u8:x}",
                                ctrl2_reset,
                                ctrl2_idle
                            );
                        }
                        Err(e) => {
                            defmt::info!(
                                "bms_diag: stage=ship_reset_idle_restore err={}",
                                i2c_error_kind(e)
                            );
                        }
                    }
                }
                Err(e) => {
                    defmt::info!("bms_diag: stage=ship_reset_pulse err={}", i2c_error_kind(e));
                }
            }
        }

        if let Err(e) = bq25792::read_block(&mut self.i2c, bq25792::reg::CHARGER_STATUS_0, &mut st)
        {
            self.mark_charger_poll_failed(now);
            defmt::error!(
                "charger: bq25792 err stage=status_read err={}",
                i2c_error_kind(e)
            );
            return;
        }
        if let Err(e) = bq25792::read_block(&mut self.i2c, bq25792::reg::FAULT_STATUS_0, &mut fault)
        {
            self.mark_charger_poll_failed(now);
            defmt::error!(
                "charger: bq25792 err stage=fault_read err={}",
                i2c_error_kind(e)
            );
            return;
        }

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
        let ac_rb2_present = (status3 & bq25792::status3::ACRB2_STAT) != 0;
        let ac_rb1_present = (status3 & bq25792::status3::ACRB1_STAT) != 0;
        let adc_done = (status3 & bq25792::status3::ADC_DONE_STAT) != 0;
        let vsys_min_reg = (status3 & bq25792::status3::VSYS_STAT) != 0;

        let input_present = vbus_present || ac1_present || ac2_present || pg;
        let adc_state = match bq25792::ensure_adc_power_path(&mut self.i2c) {
            Ok(adc_state) => Some(adc_state),
            Err(e) => {
                defmt::info!(
                    "charger: bq25792 adc_cfg err={} action=skip_adc_samples",
                    i2c_error_kind(e)
                );
                None
            }
        };
        let adc_enabled = adc_state
            .map(|adc_state| bq25792::power_path_adc_enabled(adc_state.ctrl))
            .unwrap_or(false);
        let raw_ibus_adc_ma = adc_state
            .and_then(|_| bq25792::read_adc_i16(&mut self.i2c, bq25792::reg::IBUS_ADC).ok());
        let raw_ibat_adc_ma = adc_state
            .and_then(|_| bq25792::read_adc_i16(&mut self.i2c, bq25792::reg::IBAT_ADC).ok());
        let raw_vac1_adc_mv = adc_state.and_then(|_| bq25792::read_vac1_adc_mv(&mut self.i2c).ok());
        let raw_vbus_adc_mv = adc_state
            .and_then(|_| bq25792::read_adc_u16(&mut self.i2c, bq25792::reg::VBUS_ADC).ok());
        let vbat_adc_mv = adc_state
            .and_then(|_| bq25792::read_adc_u16(&mut self.i2c, bq25792::reg::VBAT_ADC).ok());
        let vsys_adc_mv = adc_state
            .and_then(|_| bq25792::read_adc_u16(&mut self.i2c, bq25792::reg::VSYS_ADC).ok());
        let adc_ready = match adc_state {
            Some(adc_state) => bq25792::power_path_adc_ready(adc_state, status3),
            None => false,
        };
        let input_sample = normalize_charger_input_power_sample(
            input_present,
            adc_ready,
            raw_vbus_adc_mv,
            raw_ibus_adc_ma,
        );
        self.usb_pd_vac1_mv = adc_ready.then_some(raw_vac1_adc_mv).flatten();
        let usb_c_path_present =
            ac1_present || matches!(self.usb_pd_state.vbus_present, Some(true));
        let usb_pd_unsafe_latched = usb_pd_runtime_unsafe_source_latched(
            self.usb_pd_state.unsafe_source_latched,
            usb_c_path_present,
            self.usb_pd_vac1_mv,
        );
        let usb_pd_charge_gate_ready = usb_pd_charge_gate_ready(
            self.usb_pd_state.enabled,
            self.usb_pd_state.controller_ready,
            usb_c_path_present,
            self.usb_pd_state.charge_ready,
        );
        let ibat_adc_ma = adc_ready.then_some(raw_ibat_adc_ma).flatten();
        self.maybe_log_charger_input_power_anomaly(
            now,
            input_sample,
            adc_state,
            adc_ready,
            status0,
            status1,
            status3,
        );

        let can_enable = input_present && !ts_cold && !ts_hot && !usb_pd_unsafe_latched;
        let activation_probe_without_charge = activation_pending
            && self.bms_activation_phase == BmsActivationPhase::ProbeWithoutCharge;
        let activation_normal_hold_charge = false;
        let boot_diag_hold_charge = false;
        let input_source = detail_input_source(vbus_present, ac1_present, ac2_present);
        let output_power_w10 =
            charge_policy_output_power_w10(&self.ui_snapshot, self.output_state.active_outputs);
        let charge_policy_telemetry = if activation_pending {
            None
        } else {
            match (
                self.ui_snapshot.bq40z50_soc_pct,
                self.bms_cell_min_mv,
                self.bms_charge_ready,
                self.bms_full,
            ) {
                (Some(rsoc_pct), Some(cell_min_mv), Some(charge_ready), Some(bms_full))
                    if self.ui_snapshot.bq40z50_no_battery != Some(true) =>
                {
                    Some(ChargePolicyTelemetry {
                        rsoc_pct,
                        cell_min_mv,
                        charge_ready,
                        bms_full,
                    })
                }
                _ => None,
            }
        };
        let charge_policy_now_ms = self.fan_now_ms();
        let charge_policy_decision = if activation_pending {
            None
        } else {
            Some(charge_policy_step(
                &mut self.charge_policy,
                &mut self.charge_policy_derate,
                &mut self.charge_policy_output_load,
                charge_policy_now_ms,
                ChargePolicyInput {
                    input_present,
                    vbat_present,
                    ts_cold,
                    ts_hot,
                    input_source,
                    ibus_ma: input_sample.ui_ibus_ma,
                    output_enabled: charge_policy_output_enabled(
                        &self.ui_snapshot,
                        self.output_state.active_outputs,
                    ),
                    output_power_w10,
                    telemetry: charge_policy_telemetry,
                    charger_done: matches!(
                        audio_charge_phase_from_chg_stat(bq25792::status1::chg_stat(status1)),
                        AudioChargePhase::Completed
                    ),
                },
            ))
        };
        let normal_allow_charge =
            charge_policy_decision.map_or(false, |decision| decision.allow_charge);
        let force_allow_charge = (activation_force_charge || auto_force_charge) && can_enable;
        let mut allow_charge =
            if usb_pd_unsafe_latched || activation_force_charge_off || !usb_pd_charge_gate_ready {
                false
            } else {
                (normal_allow_charge && self.cfg.charger_enabled)
                    || activation_normal_hold_charge
                    || boot_diag_hold_charge
                    || force_allow_charge
            };

        let policy_state = charge_policy_decision.map(|decision| decision.state);
        let mut policy_target_ichg_ma =
            charge_policy_decision.and_then(|decision| decision.target_ichg_ma);
        let policy_start_reason = charge_policy_decision.and_then(|decision| decision.start_reason);
        let policy_full_reason = charge_policy_decision.and_then(|decision| decision.full_reason);
        let policy_output_block_reason =
            charge_policy_decision.and_then(|decision| decision.output_block_reason);
        let mut policy_status_text = if usb_pd_unsafe_latched {
            "FAULT"
        } else if !usb_pd_charge_gate_ready {
            "WAIT"
        } else if force_allow_charge {
            "WAKE"
        } else if activation_force_charge_off {
            "LOCK"
        } else if activation_probe_without_charge {
            "WAIT"
        } else {
            policy_state
                .map(detail_charger_status_text)
                .unwrap_or("READY")
        };
        let mut policy_notice_text = if usb_pd_unsafe_latched {
            "unsafe_source_latched"
        } else if !usb_pd_charge_gate_ready {
            "usb_pd_wait_stable_input"
        } else if force_allow_charge {
            "activation_force_charge"
        } else if activation_force_charge_off {
            "activation_force_charge_off"
        } else if activation_probe_without_charge {
            "activation_probe_without_charge"
        } else {
            policy_output_block_reason
                .map(ChargePolicyOutputBlockReason::as_str)
                .or_else(|| policy_state.map(ChargePolicyState::as_str))
                .unwrap_or("charger_policy_pending")
        };

        let manual_stop_hold_blocks_charge = manual_charge_stop_hold_blocks_charge(
            self.manual_charge_runtime.stop_inhibit,
            activation_pending,
            activation_force_charge,
        );

        if manual_stop_hold_blocks_charge {
            allow_charge = false;
            policy_target_ichg_ma = None;
            if !matches!(
                self.manual_charge_runtime.last_stop_reason,
                ManualChargeStopReason::SafetyBlocked
            ) && input_present
            {
                policy_status_text = "WAIT";
            }
            policy_notice_text =
                manual_charge_stop_notice(self.manual_charge_runtime.last_stop_reason);
        } else if !activation_pending && !force_allow_charge && !auto_force_charge {
            let manual_blocked = !self.cfg.charger_enabled
                || !can_enable
                || matches!(
                    policy_state,
                    Some(
                        ChargePolicyState::BlockedOutputOverload | ChargePolicyState::BlockedNoBms
                    )
                );
            let preserve_manual_safety_notice = manual_charge_safety_notice_active(
                self.manual_charge_runtime.last_stop_reason,
                self.manual_charge_runtime.active,
                self.manual_charge_runtime.stop_inhibit,
                manual_blocked,
            );

            if self.manual_charge_runtime.active {
                let stop_reason = if manual_blocked {
                    Some(ManualChargeStopReason::SafetyBlocked)
                } else if self
                    .manual_charge_runtime
                    .deadline
                    .is_some_and(|deadline| now >= deadline)
                {
                    Some(ManualChargeStopReason::TimerExpired)
                } else {
                    match self.manual_charge_prefs.target {
                        ManualChargeTarget::Pack3V7
                            if self
                                .ui_snapshot
                                .bq40z50_pack_mv
                                .is_some_and(|pack_mv| pack_mv >= MANUAL_CHARGE_TARGET_PACK_MV) =>
                        {
                            Some(ManualChargeStopReason::PackReached)
                        }
                        ManualChargeTarget::Rsoc80
                            if self.ui_snapshot.bq40z50_soc_pct.is_some_and(|soc_pct| {
                                soc_pct >= MANUAL_CHARGE_TARGET_RSOC_PCT
                            }) =>
                        {
                            Some(ManualChargeStopReason::RsocReached)
                        }
                        ManualChargeTarget::Full100
                            if self.charge_policy.full_latched || policy_full_reason.is_some() =>
                        {
                            Some(ManualChargeStopReason::FullReached)
                        }
                        _ => None,
                    }
                };

                if let Some(stop_reason) = stop_reason {
                    self.stop_manual_charge_session(
                        stop_reason,
                        manual_charge_should_hold(stop_reason),
                    );
                    allow_charge = false;
                    policy_target_ichg_ma = None;
                    policy_status_text = match stop_reason {
                        ManualChargeStopReason::FullReached => "FULL",
                        ManualChargeStopReason::SafetyBlocked => policy_status_text,
                        _ => "WAIT",
                    };
                    policy_notice_text = manual_charge_stop_notice(stop_reason);
                } else {
                    let derated = manual_charge_speed_derated(
                        self.manual_charge_prefs.speed,
                        self.charge_policy_derate.derated,
                    );
                    allow_charge = true;
                    policy_target_ichg_ma = Some(if derated {
                        CHARGE_POLICY_DC_DERATED_ICHG_MA
                    } else {
                        self.manual_charge_prefs.speed.ichg_ma()
                    });
                    policy_status_text =
                        manual_charge_status_text(self.manual_charge_prefs.speed, derated);
                    policy_notice_text = if derated {
                        "charging_100ma_dc_derated"
                    } else {
                        match self.manual_charge_prefs.speed {
                            ManualChargeSpeed::Ma100 => "charging_100ma_manual",
                            ManualChargeSpeed::Ma500 => "charging_500ma",
                            ManualChargeSpeed::Ma1000 => "charging_1a_manual",
                        }
                    };
                }
            } else if preserve_manual_safety_notice {
                policy_notice_text =
                    manual_charge_stop_notice(ManualChargeStopReason::SafetyBlocked);
            } else if matches!(
                self.manual_charge_runtime.last_stop_reason,
                ManualChargeStopReason::SafetyBlocked
            ) {
                self.manual_charge_runtime.last_stop_reason = ManualChargeStopReason::None;
            }
        }
        let mut applied_ctrl0 = ctrl0;
        let mut applied_vreg_mv: Option<u16> = None;
        let mut applied_ichg_ma: Option<u16> = None;
        let mut applied_vindpm_mv: Option<u16> = None;
        let mut applied_iindpm_ma: Option<u16> = None;
        let mut applied_iterm_ma: Option<u16> = None;
        let policy_term_target_ma =
            (!force_allow_charge && !auto_force_charge && !activation_pending)
                .then(|| {
                    self.bms_cached_lock_diag
                        .and_then(|diag| diag.current_at_eoc_ma)
                        .map(bq25792::align_termination_current_ma)
                })
                .flatten();

        fn decode_voltage_mv(reg: u16) -> u16 {
            (reg & 0x07FF) * 10
        }

        fn decode_cur_ma(reg: u16) -> u16 {
            (reg & 0x01FF) * 10
        }

        if allow_charge {
            // Ensure we are not braking the converter (ILIM_HIZ < 0.75V forces non-switching).
            self.chg_ilim_hiz_brk.set_low();

            if force_allow_charge {
                if let Err(reason) = self.ensure_bms_activation_charger_backup() {
                    self.mark_charger_poll_failed(now);
                    defmt::error!("charger: bq25792 err stage=backup_capture err={}", reason);
                    return;
                }

                match bq25792::set_charge_voltage_limit_mv(
                    &mut self.i2c,
                    BMS_ACTIVATION_FORCE_VREG_MV,
                ) {
                    Ok(v) => applied_vreg_mv = Some(decode_voltage_mv(v)),
                    Err(e) => {
                        self.mark_charger_poll_failed(now);
                        defmt::error!(
                            "charger: bq25792 err stage=vreg_write err={}",
                            i2c_error_kind(e)
                        );
                        return;
                    }
                }

                match bq25792::set_charge_current_limit_ma(
                    &mut self.i2c,
                    BMS_ACTIVATION_FORCE_ICHG_MA,
                ) {
                    Ok(v) => applied_ichg_ma = Some(decode_cur_ma(v)),
                    Err(e) => {
                        self.mark_charger_poll_failed(now);
                        defmt::error!(
                            "charger: bq25792 err stage=ichg_write err={}",
                            i2c_error_kind(e)
                        );
                        return;
                    }
                }

                match bq25792::set_input_current_limit_ma(
                    &mut self.i2c,
                    BMS_ACTIVATION_FORCE_IINDPM_MA,
                ) {
                    Ok(v) => applied_iindpm_ma = Some(decode_cur_ma(v)),
                    Err(e) => {
                        self.mark_charger_poll_failed(now);
                        defmt::error!(
                            "charger: bq25792 err stage=iindpm_write err={}",
                            i2c_error_kind(e)
                        );
                        return;
                    }
                }

                match bq25792::read_watchdog_state(&mut self.i2c) {
                    Ok(state) => {
                        if state.watchdog_before != 0 {
                            if let Err(e) = bq25792::kick_watchdog(&mut self.i2c) {
                                defmt::warn!(
                                    "charger: bq25792 warn stage=watchdog_kick err={} watchdog_before=0x{=u8:x}",
                                    i2c_error_kind(e),
                                    state.watchdog_before
                                );
                            }
                        }
                    }
                    Err(e) => {
                        defmt::warn!(
                            "charger: bq25792 warn stage=watchdog_read err={}",
                            i2c_error_kind(e)
                        );
                    }
                }
            }

            // Charge is enabled only when both `EN_CHG=1` and `CE=LOW`.
            let mut desired_ctrl0 = (ctrl0 | bq25792::ctrl0::EN_CHG) & !bq25792::ctrl0::EN_HIZ;
            if force_allow_charge || auto_force_charge || activation_pending {
                desired_ctrl0 &= !bq25792::ctrl0::EN_AUTO_IBATDIS;
            }
            if desired_ctrl0 != ctrl0 {
                match bq25792::write_u8(
                    &mut self.i2c,
                    bq25792::reg::CHARGER_CONTROL_0,
                    desired_ctrl0,
                ) {
                    Ok(()) => applied_ctrl0 = desired_ctrl0,
                    Err(e) => {
                        self.mark_charger_poll_failed(now);
                        defmt::error!(
                            "charger: bq25792 err stage=ctrl0_write err={}",
                            i2c_error_kind(e)
                        );
                        return;
                    }
                }
            }

            if !force_allow_charge && !auto_force_charge && !activation_pending {
                if let Some(target_iterm_ma) = policy_term_target_ma {
                    match bq25792::set_termination_current_ma(&mut self.i2c, target_iterm_ma) {
                        Ok(v) => applied_iterm_ma = Some(v),
                        Err(e) => {
                            self.mark_charger_poll_failed(now);
                            defmt::error!(
                                "charger: bq25792 err stage=iterm_write err={}",
                                i2c_error_kind(e)
                            );
                            return;
                        }
                    }
                }
                if let Some(target_ichg_ma) = policy_target_ichg_ma {
                    match bq25792::set_charge_voltage_limit_mv(&mut self.i2c, CHARGE_POLICY_VREG_MV)
                    {
                        Ok(v) => applied_vreg_mv = Some(decode_voltage_mv(v)),
                        Err(e) => {
                            self.mark_charger_poll_failed(now);
                            defmt::error!(
                                "charger: bq25792 err stage=vreg_write err={}",
                                i2c_error_kind(e)
                            );
                            return;
                        }
                    }

                    match bq25792::set_charge_current_limit_ma(&mut self.i2c, target_ichg_ma) {
                        Ok(v) => applied_ichg_ma = Some(decode_cur_ma(v)),
                        Err(e) => {
                            self.mark_charger_poll_failed(now);
                            defmt::error!(
                                "charger: bq25792 err stage=ichg_write err={}",
                                i2c_error_kind(e)
                            );
                            return;
                        }
                    }
                }
            }

            self.chg_ce.set_low();
            self.chg_enabled = true;
        } else {
            self.chg_ce.set_high();
            self.chg_enabled = false;
        }

        match usb_pd_input_limit_update(
            self.usb_pd_input_current_limit_ma.is_some() || self.usb_pd_vindpm_mv.is_some(),
            self.usb_pd_restore_input_limits_pending,
            force_allow_charge,
            auto_force_charge,
            activation_pending,
        ) {
            UsbPdInputLimitUpdate::ApplyContract => {
                if self.usb_pd_input_limit_backup.is_none() {
                    let vindpm_reg =
                        match bq25792::read_u8(&mut self.i2c, bq25792::reg::INPUT_VOLTAGE_LIMIT) {
                            Ok(v) => v,
                            Err(e) => {
                                self.mark_charger_poll_failed(now);
                                defmt::error!(
                                    "charger: bq25792 err stage=usb_pd_backup_vindpm_read err={}",
                                    i2c_error_kind(e)
                                );
                                return;
                            }
                        };
                    let iindpm_reg =
                        match bq25792::read_u16(&mut self.i2c, bq25792::reg::INPUT_CURRENT_LIMIT) {
                            Ok(v) => v,
                            Err(e) => {
                                self.mark_charger_poll_failed(now);
                                defmt::error!(
                                    "charger: bq25792 err stage=usb_pd_backup_iindpm_read err={}",
                                    i2c_error_kind(e)
                                );
                                return;
                            }
                        };
                    self.usb_pd_input_limit_backup = Some(UsbPdInputLimitBackup {
                        vindpm_mv: bq25792::decode_input_voltage_limit_mv(vindpm_reg),
                        iindpm_ma: bq25792::decode_input_current_limit_ma(iindpm_reg),
                    });
                }

                if let Some(target_vindpm_mv) = self.usb_pd_vindpm_mv {
                    match bq25792::set_input_voltage_limit_mv(&mut self.i2c, target_vindpm_mv) {
                        Ok(v) => {
                            applied_vindpm_mv = Some(bq25792::decode_input_voltage_limit_mv(v))
                        }
                        Err(e) => {
                            self.mark_charger_poll_failed(now);
                            defmt::error!(
                                "charger: bq25792 err stage=vindpm_write err={}",
                                i2c_error_kind(e)
                            );
                            return;
                        }
                    };
                }

                if let Some(target_iindpm_ma) = usb_pd_effective_input_current_limit_ma(
                    self.usb_pd_input_current_limit_ma,
                    (force_allow_charge || auto_force_charge)
                        .then_some(BMS_ACTIVATION_FORCE_IINDPM_MA),
                ) {
                    match bq25792::set_input_current_limit_ma(&mut self.i2c, target_iindpm_ma) {
                        Ok(v) => applied_iindpm_ma = Some(decode_cur_ma(v)),
                        Err(e) => {
                            self.mark_charger_poll_failed(now);
                            defmt::error!(
                                "charger: bq25792 err stage=iindpm_write err={}",
                                i2c_error_kind(e)
                            );
                            return;
                        }
                    }
                }
            }
            UsbPdInputLimitUpdate::RestorePrevious => {
                let restore = self
                    .usb_pd_input_limit_backup
                    .map(|backup| (backup.vindpm_mv, Some(backup.iindpm_ma)))
                    .unwrap_or_else(|| {
                        (
                            usb_pd_restore_vindpm_mv(self.ui_snapshot.input_vbus_mv),
                            None,
                        )
                    });

                match bq25792::set_input_voltage_limit_mv(&mut self.i2c, restore.0) {
                    Ok(v) => applied_vindpm_mv = Some(bq25792::decode_input_voltage_limit_mv(v)),
                    Err(e) => {
                        self.mark_charger_poll_failed(now);
                        defmt::error!(
                            "charger: bq25792 err stage=usb_pd_restore_vindpm_write err={}",
                            i2c_error_kind(e)
                        );
                        return;
                    }
                };

                if let Some(target_iindpm_ma) = restore.1 {
                    match bq25792::set_input_current_limit_ma(&mut self.i2c, target_iindpm_ma) {
                        Ok(v) => applied_iindpm_ma = Some(decode_cur_ma(v)),
                        Err(e) => {
                            self.mark_charger_poll_failed(now);
                            defmt::error!(
                                "charger: bq25792 err stage=usb_pd_restore_iindpm_write err={}",
                                i2c_error_kind(e)
                            );
                            return;
                        }
                    }
                }

                self.usb_pd_restore_input_limits_pending = false;
                self.usb_pd_input_limit_backup = None;
            }
            UsbPdInputLimitUpdate::None => {}
        }

        if !(auto_force_charge || activation_pending) {
            defmt::info!(
                "charger: enabled={=bool} force_min_charge={=bool} auto_boot_force_charge={=bool} boot_diag_hold_charge={=bool} activation_normal_hold_charge={=bool} activation_auto_probe_hold_charge={=bool} activation_force_charge_off={=bool} normal_allow_charge={=bool} force_allow_charge={=bool} allow_charge={=bool} policy_state={} policy_status={} policy_input_source={} policy_start_reason={=?} policy_full_reason={=?} policy_output_block_reason={=?} policy_target_ichg_ma={=?} policy_term_target_ma={=?} policy_output_power_w10={=?} policy_charge_latched={=bool} policy_full_latched={=bool} policy_dc_derated={=bool} policy_dc_over_limit_since_ms={=?} policy_dc_recover_since_ms={=?} policy_output_blocked={=bool} policy_output_enter_streak={=u8} policy_output_exit_streak={=u8} input_present={=bool} vbus_present={=bool} ac1_present={=bool} ac2_present={=bool} pg={=bool} vbat_present={=bool} ibus_adc_ma={=?} ibat_adc_ma={=?} vbus_adc_mv={=?} vbat_adc_mv={=?} vsys_adc_mv={=?} adc_enabled={=bool} adc_done={=bool} ac_rb1_present={=bool} ac_rb2_present={=bool} vsys_min_reg={=bool} ts_cold={=bool} ts_cool={=bool} ts_warm={=bool} ts_hot={=bool} vreg_mv={=?} ichg_ma={=?} vindpm_mv={=?} iindpm_ma={=?} iterm_ma={=?} applied_iterm_ma={=?} en_term={=bool} sfet_present_before={=bool} sfet_present_after={=bool} ship_mode_before={=u8} ship_mode_after={=u8} chg_stat={} vbus_stat={} ico={} treg={=bool} dpdm={=bool} wd={=bool} poorsrc={=bool} vindpm={=bool} iindpm={=bool} st0=0x{=u8:x} st1=0x{=u8:x} st2=0x{=u8:x} st3=0x{=u8:x} st4=0x{=u8:x} fault0=0x{=u8:x} fault1=0x{=u8:x} ctrl0=0x{=u8:x} term_ctrl={=?}",
                self.chg_enabled,
                self.cfg.force_min_charge,
                auto_force_charge,
                boot_diag_hold_charge,
                activation_normal_hold_charge,
                activation_auto_probe_hold_charge,
                activation_force_charge_off,
                normal_allow_charge,
                force_allow_charge,
                allow_charge,
                policy_notice_text,
                policy_status_text,
                dashboard_input_source_name(input_source),
                policy_start_reason.map(ChargeStartReason::as_str),
                policy_full_reason.map(ChargeFullReason::as_str),
                policy_output_block_reason.map(ChargePolicyOutputBlockReason::as_str),
                policy_target_ichg_ma,
                policy_term_target_ma,
                output_power_w10,
                self.charge_policy.charge_latched,
                self.charge_policy.full_latched,
                self.charge_policy_derate.derated,
                self.charge_policy_derate.over_limit_since_ms,
                self.charge_policy_derate.recover_since_ms,
                self.charge_policy_output_load.blocked,
                self.charge_policy_output_load.enter_streak,
                self.charge_policy_output_load.exit_streak,
                input_present,
                vbus_present,
                ac1_present,
                ac2_present,
                pg,
                vbat_present,
                raw_ibus_adc_ma,
                ibat_adc_ma,
                raw_vbus_adc_mv,
                vbat_adc_mv,
                vsys_adc_mv,
                adc_enabled,
                adc_done,
                ac_rb1_present,
                ac_rb2_present,
                vsys_min_reg,
                ts_cold,
                ts_cool,
                ts_warm,
                ts_hot,
                applied_vreg_mv,
                applied_ichg_ma,
                applied_vindpm_mv,
                applied_iindpm_ma,
                iterm_ma,
                applied_iterm_ma,
                en_term,
                sfet_present_before,
                sfet_present_after,
                ship_mode_before,
                ship_mode_after,
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
                applied_ctrl0,
                termination_ctrl
            );
        }

        self.charger_audio = ChargerAudioState {
            input_present: Some(input_present),
            phase: audio_charge_phase_from_chg_stat(bq25792::status1::chg_stat(status1)),
            thermal_stress: charger_audio_thermal_stress(ts_cool, treg),
            ts_warm,
            ts_hot,
            treg,
            over_voltage: (fault0
                & (CHARGER_FAULT0_VBUS_OVP
                    | CHARGER_FAULT0_VBAT_OVP
                    | CHARGER_FAULT0_VAC1_OVP
                    | CHARGER_FAULT0_VAC2_OVP))
                != 0
                || (fault1 & (CHARGER_FAULT1_VSYS_OVP | CHARGER_FAULT1_OTG_OVP)) != 0,
            over_current: (fault0
                & (CHARGER_FAULT0_IBUS_OCP | CHARGER_FAULT0_IBAT_OCP | CHARGER_FAULT0_CONV_OCP))
                != 0
                || (fault1 & CHARGER_FAULT1_VSYS_SHORT) != 0,
            shutdown_protection: (fault1 & (CHARGER_FAULT1_VSYS_SHORT | CHARGER_FAULT1_TSHUT)) != 0,
            module_fault: false,
        };

        let charger_fault = fault0 != 0 || fault1 != 0 || ts_cold || ts_hot;
        self.ui_snapshot.bq25792 = if charger_fault {
            SelfCheckCommState::Warn
        } else {
            SelfCheckCommState::Ok
        };
        self.ui_snapshot.bq25792_allow_charge = Some(allow_charge);
        self.ui_snapshot.bq25792_vbat_present = Some(vbat_present);
        self.ui_snapshot.input_vbus_mv = input_sample.ui_vbus_mv;
        self.ui_snapshot.input_ibus_ma = input_sample.ui_ibus_ma;
        self.ui_snapshot.bq25792_ibat_ma = ibat_adc_ma;
        self.ui_snapshot.bq25792_ichg_ma = if allow_charge {
            if let Some(v) = applied_ichg_ma {
                Some(v)
            } else {
                bq25792::read_u16(&mut self.i2c, bq25792::reg::CHARGE_CURRENT_LIMIT)
                    .ok()
                    .map(|v| (v & 0x01ff) * 10)
            }
        } else {
            None
        };
        self.ui_snapshot.fusb302_vbus_present =
            usb_pd_vbus_present(self.usb_pd_state.vbus_present, ac1_present);
        self.ui_snapshot.dashboard_detail.input_source = input_source;
        self.ui_snapshot.dashboard_detail.charger_active = Some(
            if force_allow_charge
                || self.manual_charge_runtime.active
                || self.manual_charge_runtime.stop_inhibit
            {
                allow_charge
            } else {
                policy_state
                    .map(ChargePolicyState::charger_active)
                    .unwrap_or(allow_charge)
            },
        );
        self.ui_snapshot.dashboard_detail.charger_home_status = Some(charger_home_status_text(
            charger_fault,
            ts_cold,
            ts_hot,
            ts_warm,
            policy_status_text,
        ));
        self.ui_snapshot.dashboard_detail.charger_status = Some(charger_detail_status_text(
            charger_fault,
            ts_warm,
            policy_status_text,
        ));
        self.ui_snapshot.dashboard_detail.charger_notice = Some(charger_detail_notice_text(
            charger_fault,
            ts_warm,
            policy_notice_text,
        ));
        self.update_manual_charge_ui_snapshot(now);
        self.recompute_ui_mode();

        if auto_force_charge {
            self.bms_activation_auto_force_charge_programmed = true;
            if let Some(until) = self.bms_activation_auto_force_charge_until {
                if BMS_BOOT_DIAG_TOOL_STYLE_PROBE_ONLY && self.bms_activation_auto_attempted {
                    let hold_remaining = if until > now {
                        until - now
                    } else {
                        Duration::ZERO
                    };
                    self.chg_next_poll_at = now + Duration::from_secs(1);
                    defmt::info!(
                        "bms: boot_diag hold_charge keepalive poll_ms={=u32} hold_ms_remaining={=u64}",
                        1_000u32,
                        hold_remaining.as_millis() as u64
                    );
                } else {
                    let prewarm_remaining = if until > now {
                        until - now
                    } else {
                        Duration::ZERO
                    };
                    self.chg_next_poll_at = now + Duration::from_secs(1);
                    defmt::info!(
                        "bms: boot_diag prewarm keepalive poll_ms={=u32} hold_ms_remaining={=u64}",
                        1_000u32,
                        prewarm_remaining.as_millis() as u64
                    );
                }
            }
        } else {
            if self.bms_activation_auto_force_charge_programmed
                && self.bms_activation_state != BmsActivationState::Pending
                && !activation_force_charge
            {
                if let Some(restore_chg_enabled) =
                    self.restore_bms_activation_charger_backup("auto_force_charge_complete")
                {
                    if restore_chg_enabled {
                        self.chg_ce.set_low();
                        self.chg_enabled = true;
                    } else {
                        self.chg_ce.set_high();
                        self.chg_enabled = false;
                    }
                }
            }
            self.bms_activation_auto_force_charge_programmed = false;
        }

        self.chg_next_retry_at = None;
    }

    fn mark_charger_poll_failed(&mut self, now: Instant) {
        if self.bms_activation_state == BmsActivationState::Pending {
            self.finish_bms_activation(BmsResultKind::NotDetected, "charger_poll_failed");
        }
        self.chg_ce.set_high();
        self.chg_enabled = false;
        self.chg_next_retry_at = Some(now + self.cfg.retry_backoff);
        self.charger_audio = ChargerAudioState {
            input_present: None,
            phase: AudioChargePhase::Unknown,
            thermal_stress: false,
            ts_warm: false,
            ts_hot: false,
            treg: false,
            over_voltage: false,
            over_current: false,
            shutdown_protection: false,
            module_fault: true,
        };
        self.ui_snapshot.bq25792 = SelfCheckCommState::Err;
        self.ui_snapshot.bq25792_allow_charge = Some(false);
        self.ui_snapshot.bq25792_ichg_ma = None;
        self.ui_snapshot.bq25792_ibat_ma = None;
        self.ui_snapshot.bq25792_vbat_present = None;
        self.ui_snapshot.fusb302_vbus_present = None;
        self.ui_snapshot.input_vbus_mv = None;
        self.ui_snapshot.input_ibus_ma = None;
        self.clear_charger_detail_snapshot();
        self.recompute_ui_mode();
    }

    fn maybe_poll_bms(&mut self, irq: &IrqSnapshot) -> bool {
        const POLL_PERIOD: Duration = Duration::from_secs(2);
        const INT_MIN_INTERVAL: Duration = Duration::from_millis(100);

        let now = Instant::now();
        let auto_quiet_until =
            if self.bms_activation_auto_poll_release_at > self.bms_activation_auto_due_at {
                self.bms_activation_auto_poll_release_at
            } else {
                self.bms_activation_auto_due_at
            };
        if self.cfg.bms_boot_diag_auto_validate
            && !self.bms_activation_auto_attempted
            && now < auto_quiet_until
        {
            self.bms_next_poll_at = auto_quiet_until;
            self.bms_next_retry_at = None;
            if !self.bms_activation_auto_defer_logged {
                self.bms_activation_auto_defer_logged = true;
                defmt::info!(
                    "bms: boot_diag defer_poll until_auto_request settle_ms={=u64}",
                    (auto_quiet_until - now).as_millis() as u64
                );
            }
            return false;
        }
        let mut due = now >= self.bms_next_poll_at;
        if irq.bms_btp_int_h != 0 {
            let allow = self
                .bms_last_int_poll_at
                .map_or(true, |t| now >= t + INT_MIN_INTERVAL);
            if allow {
                due = true;
                self.bms_last_int_poll_at = Some(now);
            }
        }
        if !due {
            return false;
        }
        if let Some(next_retry_at) = self.bms_next_retry_at {
            if now < next_retry_at {
                return false;
            }
        }
        self.bms_next_poll_at = now + POLL_PERIOD;
        self.bms_poll_seq = self.bms_poll_seq.wrapping_add(1);
        let poll_seq = self.bms_poll_seq;
        let auto_observation_active = !self.bms_activation_auto_attempted && now < auto_quiet_until;
        let boot_diag_probe_hold_active = BMS_BOOT_DIAG_TOOL_STYLE_PROBE_ONLY
            && self.bms_activation_auto_attempted
            && self
                .bms_activation_auto_force_charge_until
                .map_or(false, |until| now < until);
        let suppress_retry_backoff = auto_observation_active
            || self.bms_activation_state == BmsActivationState::Pending
            || boot_diag_probe_hold_active;

        let btp_int_h = self.bms_btp_int_h.is_high() || irq.bms_btp_int_h != 0;

        #[cfg(feature = "bms-dual-probe-diag")]
        let (addr_order, addr_count): ([u8; 2], usize) = match self.bms_addr {
            Some(a) if a == bq40z50::I2C_ADDRESS_FALLBACK => (
                [bq40z50::I2C_ADDRESS_FALLBACK, bq40z50::I2C_ADDRESS_PRIMARY],
                2,
            ),
            Some(a) if a == bq40z50::I2C_ADDRESS_PRIMARY => (
                [bq40z50::I2C_ADDRESS_PRIMARY, bq40z50::I2C_ADDRESS_FALLBACK],
                2,
            ),
            _ => (bq40z50::I2C_ADDRESS_CANDIDATES, 2),
        };

        #[cfg(not(feature = "bms-dual-probe-diag"))]
        let (addr_order, addr_count): ([u8; 2], usize) = (
            [bq40z50::I2C_ADDRESS_PRIMARY, bq40z50::I2C_ADDRESS_PRIMARY],
            1,
        );

        for (idx, addr) in addr_order.into_iter().take(addr_count).enumerate() {
            match self.read_bq40z50_snapshot_strict(addr) {
                Ok(s) => {
                    self.bms_addr = Some(addr);
                    self.bms_runtime_seen = true;
                    self.bms_next_retry_at = None;
                    self.bms_ok_streak = self.bms_ok_streak.saturating_add(1);
                    self.bms_err_streak = 0;
                    let rca_alarm = (s.batt_status & bq40z50::battery_status::RCA) != 0;
                    let low_pack = bq40_pack_indicates_no_battery(s.vpack_mv);
                    let discharge_ready = Self::bq40_discharge_ready(s.op_status);
                    self.ui_snapshot.bq40z50 =
                        if low_pack || rca_alarm || !matches!(discharge_ready, Some(true)) {
                            SelfCheckCommState::Warn
                        } else {
                            SelfCheckCommState::Ok
                        };
                    self.ui_snapshot.bq40z50_pack_mv = Some(s.vpack_mv);
                    self.ui_snapshot.bq40z50_current_ma = Some(s.current_ma);
                    self.ui_snapshot.bq40z50_soc_pct = Some(s.rsoc_pct);
                    self.ui_snapshot.bq40z50_rca_alarm = Some(rca_alarm);
                    self.ui_snapshot.bq40z50_no_battery = Some(low_pack);
                    self.ui_snapshot.bq40z50_discharge_ready = discharge_ready;
                    self.bms_charge_ready = bq40_decode_charge_path(s.op_status).0;
                    self.bms_full = Some((s.batt_status & bq40z50::battery_status::FC) != 0);
                    self.bms_cell_min_mv = Some(bq40_cell_min_mv(&s));
                    self.apply_bms_detail_snapshot(&s);
                    let protection_active = bq40_protection_active(s.batt_status, s.op_status);
                    self.bms_audio = BmsAudioState {
                        rca_alarm: Some(rca_alarm),
                        protection_active,
                        module_fault: false,
                    };
                    self.log_bq40z50_snapshot(addr, poll_seq, self.bms_ok_streak, btp_int_h, &s);
                    self.maybe_log_bq40_config_runtime(addr, s.op_status);
                    let (_, charge_reason) = bq40_decode_charge_path(s.op_status);
                    let (_, discharge_reason) = bq40_decode_discharge_path(s.op_status);
                    let primary_reason = bq40_primary_reason(
                        s.batt_status,
                        s.op_status,
                        charge_reason,
                        discharge_reason,
                    );
                    self.ui_snapshot.bq40z50_issue_detail =
                        bq40_ui_issue_detail(low_pack, primary_reason);
                    self.maybe_log_bq40_block_detail_runtime(addr, primary_reason, s.op_status);
                    return true;
                }
                Err(Bq40SnapshotReadError::Invalid(s)) => {
                    if idx + 1 == addr_count {
                        self.bms_addr = None;
                        self.bms_ok_streak = 0;
                        self.bms_err_streak = self.bms_err_streak.saturating_add(1);
                        self.bms_next_retry_at = if suppress_retry_backoff {
                            None
                        } else {
                            Some(now + self.cfg.retry_backoff)
                        };
                        self.clear_bms_charge_policy_inputs();
                        if self.should_hold_no_battery_result() {
                            self.hold_no_battery_result_audio_state();
                        } else {
                            self.reset_bms_detail_mac_cache(now);
                            self.ui_snapshot.bq40z50 = SelfCheckCommState::Warn;
                            self.ui_snapshot.bq40z50_pack_mv = None;
                            self.ui_snapshot.bq40z50_current_ma = None;
                            self.ui_snapshot.bq40z50_soc_pct = None;
                            self.ui_snapshot.bq40z50_rca_alarm = None;
                            self.ui_snapshot.bq40z50_no_battery = None;
                            self.ui_snapshot.bq40z50_discharge_ready = None;
                            self.ui_snapshot.bq40z50_issue_detail = None;
                            self.clear_bms_detail_snapshot();
                            self.bms_audio = BmsAudioState {
                                rca_alarm: None,
                                protection_active: false,
                                module_fault: true,
                            };
                        }
                        let temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(s.temp_k_x10);
                        let flow = bq40_decode_current_flow(s.current_ma);
                        let (charge_ready, charge_reason) = bq40_decode_charge_path(s.op_status);
                        let (discharge_ready, discharge_reason) =
                            bq40_decode_discharge_path(s.op_status);
                        let primary_reason = bq40_primary_reason(
                            s.batt_status,
                            s.op_status,
                            charge_reason,
                            discharge_reason,
                        );
                        defmt::warn!(
                            "bms: bq40z50 invalid addrs={} poll_seq={=u32} err_streak={=u16} temp_c_x10={=i32} vpack_mv={=u16} rsoc_pct={=u16} flow={} chg_ready={=?} dsg_ready={=?} chg_reason={} dsg_reason={} primary_reason={}",
                            BMS_ADDR_LOG,
                            poll_seq,
                            self.bms_err_streak,
                            temp_c_x10,
                            s.vpack_mv,
                            s.rsoc_pct,
                            flow,
                            charge_ready,
                            discharge_ready,
                            charge_reason,
                            discharge_reason,
                            primary_reason
                        );
                    }
                }
                Err(Bq40SnapshotReadError::I2c(kind)) => {
                    // Only log one line after the final address attempt.
                    if idx + 1 == addr_count {
                        self.bms_addr = None;
                        self.bms_ok_streak = 0;
                        self.bms_err_streak = self.bms_err_streak.saturating_add(1);
                        self.bms_next_retry_at = if suppress_retry_backoff {
                            None
                        } else {
                            Some(now + self.cfg.retry_backoff)
                        };
                        self.clear_bms_charge_policy_inputs();
                        if self.should_hold_no_battery_result() {
                            self.hold_no_battery_result_audio_state();
                        } else {
                            self.reset_bms_detail_mac_cache(now);
                            self.ui_snapshot.bq40z50 = SelfCheckCommState::Err;
                            self.ui_snapshot.bq40z50_pack_mv = None;
                            self.ui_snapshot.bq40z50_current_ma = None;
                            self.ui_snapshot.bq40z50_soc_pct = None;
                            self.ui_snapshot.bq40z50_rca_alarm = None;
                            self.ui_snapshot.bq40z50_no_battery = None;
                            self.ui_snapshot.bq40z50_discharge_ready = None;
                            self.ui_snapshot.bq40z50_issue_detail = None;
                            self.clear_bms_detail_snapshot();
                            self.bms_audio = BmsAudioState {
                                rca_alarm: None,
                                protection_active: false,
                                module_fault: true,
                            };
                        }

                        if kind == "i2c_nack" || kind == "i2c_timeout" {
                            defmt::warn!(
                                "bms: bq40z50 absent addrs={} poll_seq={=u32} err_streak={=u16} err={} btp_int_h={=bool}",
                                BMS_ADDR_LOG,
                                poll_seq,
                                self.bms_err_streak,
                                kind,
                                btp_int_h
                            );
                        } else {
                            defmt::error!(
                                "bms: bq40z50 err addrs={} poll_seq={=u32} err_streak={=u16} err={} btp_int_h={=bool}",
                                BMS_ADDR_LOG,
                                poll_seq,
                                self.bms_err_streak,
                                kind,
                                btp_int_h
                            );
                        }
                    }
                }
            }
        }
        true
    }

    fn is_bq40_snapshot_reasonable(s: &Bq40z50Snapshot) -> bool {
        let temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(s.temp_k_x10);
        (-400..=1250).contains(&temp_c_x10)
            && (2_500..=20_000).contains(&s.vpack_mv)
            && s.rsoc_pct <= 100
    }

    fn is_bq40_low_pack_snapshot_candidate(s: &Bq40z50Snapshot) -> bool {
        let temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(s.temp_k_x10);
        (-400..=1250).contains(&temp_c_x10)
            && bq40_pack_indicates_no_battery(s.vpack_mv)
            && s.rsoc_pct <= 100
    }

    fn bq40_low_pack_snapshots_match(a: &Bq40z50Snapshot, b: &Bq40z50Snapshot) -> bool {
        a.vpack_mv == b.vpack_mv
            && a.current_ma == b.current_ma
            && a.rsoc_pct == b.rsoc_pct
            && a.batt_status == b.batt_status
            && a.cell_mv == b.cell_mv
    }

    fn bq40_discharge_ready(op_status: Option<u32>) -> Option<bool> {
        bq40_decode_discharge_path(op_status).0
    }

    fn read_bq40z50_snapshot_strict(
        &mut self,
        addr: u8,
    ) -> Result<Bq40z50Snapshot, Bq40SnapshotReadError> {
        const MAX_FULL_SNAPSHOT_ATTEMPTS: usize = 2;
        let mut last_i2c_kind: Option<&'static str> = None;
        let mut last_invalid: Option<Bq40z50Snapshot> = None;
        let mut low_pack_candidate: Option<Bq40z50Snapshot> = None;

        for _ in 0..MAX_FULL_SNAPSHOT_ATTEMPTS {
            match self.read_bq40z50_snapshot_retry(addr) {
                Ok(snapshot) => {
                    if Self::is_bq40_snapshot_reasonable(&snapshot) {
                        return Ok(snapshot);
                    }
                    if Self::is_bq40_low_pack_snapshot_candidate(&snapshot) {
                        if let Some(previous) = low_pack_candidate {
                            if Self::bq40_low_pack_snapshots_match(&previous, &snapshot) {
                                return Ok(snapshot);
                            }
                        }
                        low_pack_candidate = Some(snapshot);
                    }
                    last_invalid = Some(snapshot);
                }
                Err(e) => {
                    last_i2c_kind = Some(bq40_activation_read_error_kind(e));
                }
            }
        }

        if let Some(snapshot) = last_invalid {
            return Err(Bq40SnapshotReadError::Invalid(snapshot));
        }

        Err(Bq40SnapshotReadError::I2c(last_i2c_kind.unwrap_or("i2c")))
    }

    fn read_bq40z50_snapshot_retry(
        &mut self,
        addr: u8,
    ) -> Result<Bq40z50Snapshot, Bq40ActivationReadError> {
        const MAX_ATTEMPTS: usize = 3;

        for attempt in 0..MAX_ATTEMPTS {
            match self.read_bq40z50_snapshot(addr) {
                Ok(snapshot) => return Ok(snapshot),
                Err(e) => {
                    let retryable = matches!(
                        e,
                        Bq40ActivationReadError::I2c("i2c_timeout")
                            | Bq40ActivationReadError::I2c("i2c_nack")
                    );
                    if retryable && attempt + 1 < MAX_ATTEMPTS {
                        continue;
                    }
                    return Err(e);
                }
            }
        }

        unreachable!()
    }

    fn read_bq40z50_snapshot(
        &mut self,
        addr: u8,
    ) -> Result<Bq40z50Snapshot, Bq40ActivationReadError> {
        let now = Instant::now();
        let battery_mode =
            self.read_bq40_u16_with_optional_pec(addr, bq40z50::cmd::BATTERY_MODE)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let mut temp_k_x10 =
            self.read_bq40_u16_with_optional_pec(addr, bq40z50::cmd::TEMPERATURE)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let vpack_mv = self.read_bq40_u16_with_optional_pec(addr, bq40z50::cmd::VOLTAGE)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let current_ma = self.read_bq40_i16_with_optional_pec(addr, bq40z50::cmd::CURRENT)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let rsoc_pct =
            self.read_bq40_u16_with_optional_pec(addr, bq40z50::cmd::RELATIVE_STATE_OF_CHARGE)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let remcap =
            self.read_bq40_u16_with_optional_pec(addr, bq40z50::cmd::REMAINING_CAPACITY)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let fcc = self.read_bq40_u16_with_optional_pec(addr, bq40z50::cmd::FULL_CHARGE_CAPACITY)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let batt_status =
            self.read_bq40_u16_with_optional_pec(addr, bq40z50::cmd::BATTERY_STATUS)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let cell1_mv = self.read_bq40_u16_with_optional_pec(addr, bq40z50::cmd::CELL_VOLTAGE_1)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let cell2_mv = self.read_bq40_u16_with_optional_pec(addr, bq40z50::cmd::CELL_VOLTAGE_2)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let cell3_mv = self.read_bq40_u16_with_optional_pec(addr, bq40z50::cmd::CELL_VOLTAGE_3)?;
        spin_delay(BMS_ACTIVATION_WORD_GAP);
        let cell4_mv = self.read_bq40_u16_with_optional_pec(addr, bq40z50::cmd::CELL_VOLTAGE_4)?;

        let temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(temp_k_x10);
        if !(-400..=1250).contains(&temp_c_x10) {
            spin_delay(BMS_ACTIVATION_WORD_GAP);
            if let Ok(retry_temp_k_x10) =
                self.read_bq40_u16_with_optional_pec(addr, bq40z50::cmd::TEMPERATURE)
            {
                let retry_temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(retry_temp_k_x10);
                if (-400..=1250).contains(&retry_temp_c_x10) {
                    temp_k_x10 = retry_temp_k_x10;
                }
            }
        }

        let op_status = bq40z50::read_operation_status(&mut self.i2c, addr)
            .ok()
            .flatten();
        let mut da_status2 = self.bms_cached_da_status2;
        if now >= self.bms_next_da_status2_refresh_at {
            spin_delay(BMS_ACTIVATION_WORD_GAP);
            if let Ok(snapshot) = bq40z50::read_da_status2(&mut self.i2c, addr) {
                da_status2 = snapshot;
                self.bms_cached_da_status2 = snapshot;
            }
            self.bms_next_da_status2_refresh_at = now + BMS_DETAIL_MAC_REFRESH_PERIOD;
        }
        let mut filter_capacity = self.bms_cached_filter_capacity;
        if now >= self.bms_next_filter_capacity_refresh_at {
            spin_delay(BMS_ACTIVATION_WORD_GAP);
            if let Ok(snapshot) = bq40z50::read_filter_capacity(&mut self.i2c, addr) {
                filter_capacity = snapshot;
                self.bms_cached_filter_capacity = snapshot;
            }
            self.bms_next_filter_capacity_refresh_at = now + BMS_DETAIL_MAC_REFRESH_PERIOD;
        }
        let mut balance_config = self.bms_cached_balance_config;
        if now >= self.bms_next_balance_config_refresh_at {
            spin_delay(BMS_ACTIVATION_WORD_GAP);
            if let Ok(snapshot) = bq40z50::read_balance_config(&mut self.i2c, addr) {
                balance_config = snapshot;
                self.bms_cached_balance_config = snapshot;
            }
            self.bms_next_balance_config_refresh_at = now + BMS_DETAIL_MAC_REFRESH_PERIOD;
        }
        let mut gauging_status = self.bms_cached_gauging_status;
        if now >= self.bms_next_gauging_status_refresh_at {
            spin_delay(BMS_ACTIVATION_WORD_GAP);
            if let Ok(snapshot) = bq40z50::read_gauging_status(&mut self.i2c, addr) {
                gauging_status = snapshot;
                self.bms_cached_gauging_status = snapshot;
            }
            self.bms_next_gauging_status_refresh_at = now + BMS_DETAIL_MAC_REFRESH_PERIOD;
        }
        if now >= self.bms_next_lock_diag_refresh_at {
            spin_delay(BMS_ACTIVATION_WORD_GAP);
            let snapshot = read_bq40_lock_diag_snapshot(&mut self.i2c, addr);
            self.bms_cached_lock_diag = Some(snapshot);
            self.bms_next_lock_diag_refresh_at = now + BMS_DETAIL_MAC_REFRESH_PERIOD;
            log_bq40_lock_diag_snapshot(addr, "runtime_periodic", &snapshot);
        }
        let afe_register = if matches!(
            bq40_op_bit(op_status, bq40z50::operation_status::CB),
            Some(true)
        ) {
            spin_delay(BMS_ACTIVATION_WORD_GAP);
            bq40z50::read_afe_register(&mut self.i2c, addr)
                .ok()
                .flatten()
        } else {
            None
        };
        Ok(Bq40z50Snapshot {
            battery_mode,
            temp_k_x10,
            vpack_mv,
            current_ma,
            rsoc_pct,
            remcap,
            fcc,
            batt_status,
            op_status,
            da_status2,
            filter_capacity,
            balance_config,
            gauging_status,
            lock_diag: self.bms_cached_lock_diag,
            afe_register,
            cell_mv: [cell1_mv, cell2_mv, cell3_mv, cell4_mv],
        })
    }

    fn log_bq40z50_snapshot(
        &self,
        addr: u8,
        poll_seq: u32,
        ok_streak: u16,
        btp_int_h: bool,
        s: &Bq40z50Snapshot,
    ) {
        let temp_c_x10 = bq40z50::temp_c_x10_from_k_x10(s.temp_k_x10);

        let bs = s.batt_status;
        let init = (bs & bq40z50::battery_status::INIT) != 0;
        let dsg = (bs & bq40z50::battery_status::DSG) != 0;
        let fc = (bs & bq40z50::battery_status::FC) != 0;
        let fd = (bs & bq40z50::battery_status::FD) != 0;

        let oca = (bs & bq40z50::battery_status::OCA) != 0;
        let tca = (bs & bq40z50::battery_status::TCA) != 0;
        let ota = (bs & bq40z50::battery_status::OTA) != 0;
        let tda = (bs & bq40z50::battery_status::TDA) != 0;
        let rca = (bs & bq40z50::battery_status::RCA) != 0;
        let rta = (bs & bq40z50::battery_status::RTA) != 0;
        let xchg = bq40_op_bit(s.op_status, bq40z50::operation_status::XCHG);
        let xdsg = bq40_op_bit(s.op_status, bq40z50::operation_status::XDSG);
        let chg_fet = bq40_op_bit(s.op_status, bq40z50::operation_status::CHG);
        let dsg_fet = bq40_op_bit(s.op_status, bq40z50::operation_status::DSG);
        let pchg_fet = bq40_op_bit(s.op_status, bq40z50::operation_status::PCHG);
        let (chg_ready, chg_reason) = bq40_decode_charge_path(s.op_status);
        let (dsg_ready, dsg_reason) = bq40_decode_discharge_path(s.op_status);
        let pres = bq40_op_bit(s.op_status, bq40z50::operation_status::PRES);
        let sleep = bq40_op_bit(s.op_status, bq40z50::operation_status::SLEEP);
        let pf = bq40_op_bit(s.op_status, bq40z50::operation_status::PF);
        let flow = bq40_decode_current_flow(s.current_ma);
        let flow_abs_ma = s.current_ma.wrapping_abs() as u16;
        let pack_power_mw = (s.vpack_mv as i32 * s.current_ma as i32) / 1000;
        let primary_reason = bq40_primary_reason(bs, s.op_status, chg_reason, dsg_reason);
        let no_battery = bq40_pack_indicates_no_battery(s.vpack_mv);
        let (cell_min_mv, cell_max_mv, cell_delta_mv) = bq40_cell_min_max_delta(&s.cell_mv);
        let op_status_read_ok = s.op_status.is_some();
        let lock_diag = s.lock_diag;
        let charging_status = lock_diag
            .and_then(|diag| diag.charging)
            .and_then(|charging| charging.value);
        let oc =
            lock_diag.and_then(|diag| bq40_mac_bit(diag.safety_status, bq40z50::safety_status::OC));
        let learn_qen = lock_diag
            .and_then(|diag| bq40_mac_bit(diag.gauging_status, bq40z50::gauging_status::QEN));
        let learn_vok = lock_diag
            .and_then(|diag| bq40_mac_bit(diag.gauging_status, bq40z50::gauging_status::VOK));
        let learn_rest = lock_diag
            .and_then(|diag| bq40_mac_bit(diag.gauging_status, bq40z50::gauging_status::REST));
        let gs_fc = lock_diag
            .and_then(|diag| bq40_mac_bit(diag.gauging_status, bq40z50::gauging_status::FC));
        let gs_fd = lock_diag
            .and_then(|diag| bq40_mac_bit(diag.gauging_status, bq40z50::gauging_status::FD));
        let vct = lock_diag
            .and_then(|diag| diag.charging)
            .and_then(|charging| bq40_mac_bit(charging.value, bq40z50::charging_status::VCT));
        let nct = lock_diag
            .and_then(|diag| diag.charging)
            .and_then(|charging| bq40_mac_bit(charging.value, bq40z50::charging_status::NCT));
        let ccr = lock_diag
            .and_then(|diag| diag.charging)
            .and_then(|charging| bq40_mac_bit(charging.value, bq40z50::charging_status::CCR));
        let cvr = lock_diag
            .and_then(|diag| diag.charging)
            .and_then(|charging| bq40_mac_bit(charging.value, bq40z50::charging_status::CVR));
        let ccc = lock_diag
            .and_then(|diag| diag.charging)
            .and_then(|charging| bq40_mac_bit(charging.value, bq40z50::charging_status::CCC));

        let ec = bq40z50::battery_status::error_code(bs);

        defmt::info!(
            "bms: bq40z50 addr=0x{=u8:x} poll_seq={=u32} ok_streak={=u16} btp_int_h={=bool} temp_c_x10={=i32} vpack_mv={=u16} no_battery={=bool} current_ma={=i16} flow={} flow_abs_ma={=u16} pack_power_mw={=i32} rsoc_pct={=u16} remcap={=u16} fcc={=u16} batt_status=0x{=u16:x} op_status={=?} op_status_read_ok={=bool} charging_status={=?} init={=bool} dsg={=bool} fc={=bool} fd={=bool} xchg={=?} xdsg={=?} chg_fet={=?} dsg_fet={=?} pchg_fet={=?} chg_ready={=?} dsg_ready={=?} chg_reason={} dsg_reason={} primary_reason={} pres={=?} sleep={=?} pf={=?} oc={=?} learn_qen={=?} learn_vok={=?} learn_rest={=?} gs_fc={=?} gs_fd={=?} vct={=?} ccr={=?} cvr={=?} ccc={=?} nct={=?} update_status={=?} no_valid_charge_term={=?} current_at_eoc_ma={=?} qmax_updates={=?} ra_updates={=?} oca={=bool} tca={=bool} ota={=bool} tda={=bool} rca={=bool} rta={=bool} ec=0x{=u8:x} ec_str={} cell_min_mv={=u16} cell_max_mv={=u16} cell_delta_mv={=u16} c1_mv={=u16} c2_mv={=u16} c3_mv={=u16} c4_mv={=u16}",
            addr,
            poll_seq,
            ok_streak,
            btp_int_h,
            temp_c_x10,
            s.vpack_mv,
            no_battery,
            s.current_ma,
            flow,
            flow_abs_ma,
            pack_power_mw,
            s.rsoc_pct,
            s.remcap,
            s.fcc,
            bs,
            s.op_status,
            op_status_read_ok,
            charging_status,
            init,
            dsg,
            fc,
            fd,
            xchg,
            xdsg,
            chg_fet,
            dsg_fet,
            pchg_fet,
            chg_ready,
            dsg_ready,
            chg_reason,
            dsg_reason,
            primary_reason,
            pres,
            sleep,
            pf,
            oc,
            learn_qen,
            learn_vok,
            learn_rest,
            gs_fc,
            gs_fd,
            vct,
            ccr,
            cvr,
            ccc,
            nct,
            lock_diag.and_then(|diag| diag.update_status),
            lock_diag.and_then(|diag| diag.no_valid_charge_term),
            lock_diag.and_then(|diag| diag.current_at_eoc_ma),
            lock_diag.and_then(|diag| diag.no_of_qmax_updates),
            lock_diag.and_then(|diag| diag.no_of_ra_updates),
            oca,
            tca,
            ota,
            tda,
            rca,
            rta,
            ec,
            bq40z50::decode_error_code(ec),
            cell_min_mv,
            cell_max_mv,
            cell_delta_mv,
            s.cell_mv[0],
            s.cell_mv[1],
            s.cell_mv[2],
            s.cell_mv[3],
        );
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

#[derive(Clone, Copy)]
struct Bq40z50Snapshot {
    battery_mode: u16,
    temp_k_x10: u16,
    vpack_mv: u16,
    current_ma: i16,
    rsoc_pct: u16,
    remcap: u16,
    fcc: u16,
    batt_status: u16,
    op_status: Option<u32>,
    da_status2: Option<bq40z50::DaStatus2>,
    filter_capacity: Option<bq40z50::FilterCapacity>,
    balance_config: Option<bq40z50::BalanceConfig>,
    gauging_status: Option<u32>,
    lock_diag: Option<Bq40LockDiagSnapshot>,
    afe_register: Option<bq40z50::AfeRegister>,
    cell_mv: [u16; 4],
}

#[derive(Clone, Copy)]
struct Bq40LockDiagSnapshot {
    charging: Option<bq40z50::ChargingStatusTrace>,
    safety_status: Option<u32>,
    gauging_status: Option<u32>,
    op_status: Option<u32>,
    update_status: Option<u8>,
    current_at_eoc_ma: Option<u16>,
    no_valid_charge_term: Option<u16>,
    last_valid_charge_term: Option<u16>,
    no_of_qmax_updates: Option<u16>,
    no_of_ra_updates: Option<u16>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Bq40ActivationSignature {
    vpack_mv: u16,
    current_ma: i16,
    rsoc_pct: u16,
    batt_status: u16,
}

#[derive(Clone, Copy)]
struct Bq40ActivationPatternTracker {
    last_signature: Option<Bq40ActivationSignature>,
    repeat_count: u8,
}

impl Bq40ActivationPatternTracker {
    const fn new() -> Self {
        Self {
            last_signature: None,
            repeat_count: 0,
        }
    }

    fn observe(&mut self, signature: Bq40ActivationSignature) -> u8 {
        if self.last_signature == Some(signature) {
            self.repeat_count = self.repeat_count.saturating_add(1);
        } else {
            self.last_signature = Some(signature);
            self.repeat_count = 1;
        }
        self.repeat_count
    }
}

fn observe_bq40_activation_signature(
    tracker: &mut Bq40ActivationPatternTracker,
    vpack_mv: u16,
    current_ma: i16,
    rsoc_pct: u16,
    batt_status: u16,
) -> u8 {
    tracker.observe(Bq40ActivationSignature {
        vpack_mv,
        current_ma,
        rsoc_pct,
        batt_status,
    })
}

fn bq40_activation_signature_is_stale(
    vpack_mv: u16,
    current_ma: i16,
    batt_status: u16,
    repeat_count: u8,
) -> bool {
    vpack_mv == BMS_SUSPICIOUS_VOLTAGE_MV
        && current_ma == BMS_SUSPICIOUS_CURRENT_MA
        && batt_status == BMS_SUSPICIOUS_STATUS
        && repeat_count >= 3
}

#[derive(Clone, Copy)]
enum Bq40ActivationProbeResult {
    Pending,
    Rom,
    Working { addr: u8, snapshot: Bq40z50Snapshot },
}

#[derive(Clone, Copy)]
struct Bq40ActivationBlockReadRaw {
    declared_len: u8,
    payload_len: u8,
    payload: [u8; 32],
}

#[derive(Clone, Copy)]
enum Bq40ActivationReadError {
    I2c(&'static str),
    BadRange,
    StalePattern,
    InconsistentSample,
}

fn bq40_activation_read_error_kind(err: Bq40ActivationReadError) -> &'static str {
    match err {
        Bq40ActivationReadError::I2c(kind) => kind,
        Bq40ActivationReadError::BadRange => "bad_range",
        Bq40ActivationReadError::StalePattern => "stale_pattern",
        Bq40ActivationReadError::InconsistentSample => "inconsistent_sample",
    }
}

enum Bq40SnapshotReadError {
    I2c(&'static str),
    Invalid(Bq40z50Snapshot),
}
