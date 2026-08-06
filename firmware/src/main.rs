#![no_std]
#![no_main]

#[cfg(feature = "net_http")]
extern crate alloc;

use core::{cell::RefCell, fmt::Write as _, ptr};

esp_bootloader_esp_idf::esp_app_desc!();

mod front_panel;
mod front_panel_logic;
mod front_panel_scene;
mod irq;
mod net_bridge;
mod output;
mod runtime_audio_recovery;

#[cfg(feature = "net_http")]
use embassy_executor::Spawner;
#[cfg(not(feature = "net_http"))]
use embassy_futures::block_on;
#[cfg(feature = "net_http")]
use embassy_futures::yield_now;
#[cfg(feature = "net_http")]
use embassy_time::Timer;
use embedded_hal_bus::i2c::RefCellDevice;
use esp_backtrace as _;
use esp_firmware::audio::{AudioCue, AudioManager, AudioRoute, PLAYBACK_SAMPLE_RATE_HZ};
use esp_firmware::usb_pd::UsbPdSinkManager;
#[cfg(feature = "web_serial")]
use esp_firmware::{
    mdns_wire::{derive_device_identity, DeviceIdentity},
    net_contract::{
        render_charge_control_result_json, render_compact_status_json, render_diag_snapshot_json,
        render_identity_json_with_write_controls, render_status_json, BuildInfo,
    },
    net_types::{UpsStatusSnapshot, WifiConnectionState, WifiErrorKind},
    usb_cdc_protocol::{
        parse_frame, render_error_json, render_error_json_with_details, render_hello_json,
        render_log_json, render_protocol_error_json, render_response_json,
        render_status_frame_json, render_wifi_config_ack_json, request_id_hint, LogLevel,
        UsbCdcFrame, UsbCdcLineBuffer, UsbCdcRequest, WifiConfigCommand,
        WEB_SERIAL_DIAG_SNAPSHOT_BODY_CAP, WEB_SERIAL_DIAG_SNAPSHOT_FRAME_CAP,
        WEB_SERIAL_RESPONSE_BODY_CAP, WEB_SERIAL_RESPONSE_FRAME_CAP,
    },
};
use esp_hal::clock::CpuClock;
use esp_hal::dma::DmaError;
use esp_hal::gpio::{
    AnyPin, DriveMode, Event, Flex, Input, InputConfig, Io, Level, Output, OutputConfig, Pull,
};
use esp_hal::i2c::master::{Config as I2cConfig, I2c, SoftwareTimeout};
use esp_hal::i2s::master::{Channels, Config as I2sConfig, DataFormat, I2s};
use esp_hal::ledc::channel::{self, ChannelIFace};
use esp_hal::ledc::timer::{self, TimerIFace};
use esp_hal::ledc::{LSGlobalClkSource, Ledc, LowSpeed};
#[cfg(not(feature = "net_http"))]
use esp_hal::main;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::spi::Mode as SpiMode;
use esp_hal::time::{Duration, Instant, Rate};
use esp_hal::timer::{
    systimer::SystemTimer,
    timg::{MwdtStage, TimerGroup},
};
#[cfg(feature = "web_serial")]
use esp_hal::usb_serial_jtag::UsbSerialJtag;
use esp_hal::Blocking;
use esp_println as _;
#[cfg(feature = "web_serial")]
use heapless::String as HeaplessString;
use runtime_audio_recovery::{RuntimeAudioRecoveryDecision, RuntimeAudioRecoveryState};

// Bring-up default profile.
const DEFAULT_ENABLED_OUTPUTS: output::EnabledOutputs = output::EnabledOutputs::Both;
const DEFAULT_VOUT_MV: u16 = if cfg!(feature = "main-vout-19v") {
    19_000
} else {
    12_000
};
const DEFAULT_STANDBY_VOUT_MV: u16 =
    DEFAULT_VOUT_MV.saturating_sub(if cfg!(feature = "main-vout-19v") {
        1_200
    } else {
        700
    });
const DEFAULT_ASSIST_LOW_VOUT_MV: u16 = DEFAULT_VOUT_MV.saturating_sub(600);
const DEFAULT_ILIMIT_MA: u16 = 3_500;
const TELEMETRY_PERIOD: Duration = Duration::from_millis(500);
const RETRY_BACKOFF: Duration = Duration::from_secs(5);
const FAULT_LOG_MIN_INTERVAL: Duration = Duration::from_millis(200);
const TELEMETRY_INCLUDE_VIN_CH3: bool = true;
const FORCE_MIN_CHARGE: bool = cfg!(feature = "force-min-charge");
const BMS_BOOT_DIAG_AUTO_VALIDATE: bool = false;
const I2C1_FREQ_KHZ: u32 = 25;
const I2C1_BUS_CLEAR_PULSES: u8 = 18;
const I2C1_BUS_CLEAR_HALF_PERIOD: Duration = Duration::from_micros(20);
const I2C1_BUS_TIMEOUT_LOW: Duration = Duration::from_millis(40);
const I2C1_BITBANG_HALF_PERIOD: Duration = Duration::from_micros(100);
const BMS_PRETOUCH_ENABLED: bool = false;

const FW_BUILD_PROFILE: &str = env!("FW_BUILD_PROFILE");
const FW_GIT_SHA: &str = env!("FW_GIT_SHA");
const FW_SRC_HASH: &str = env!("FW_SRC_HASH");
const FW_GIT_DIRTY: &str = env!("FW_GIT_DIRTY");
const FW_BUILD_ID: &str = env!("FW_BUILD_ID");
#[cfg(feature = "web_serial")]
const WEB_SERIAL_BUILD_INFO: BuildInfo = BuildInfo {
    package_version: env!("CARGO_PKG_VERSION"),
    build_profile: env!("FW_BUILD_PROFILE"),
    build_id: env!("FW_BUILD_ID"),
    git_sha: env!("FW_GIT_SHA"),
    src_hash: env!("FW_SRC_HASH"),
    git_dirty: env!("FW_GIT_DIRTY"),
    features: env!("FW_FEATURES"),
};
const USB_PD_FIXED_5V_ENABLED: bool = !cfg!(feature = "no-pd-sink-5v");
const USB_PD_FIXED_9V_ENABLED: bool = !cfg!(feature = "no-pd-sink-9v");
const USB_PD_FIXED_12V_ENABLED: bool = !cfg!(feature = "no-pd-sink-12v");
const USB_PD_FIXED_15V_ENABLED: bool = !cfg!(feature = "no-pd-sink-15v");
const USB_PD_FIXED_20V_ENABLED: bool = !cfg!(feature = "no-pd-sink-20v");
const USB_PD_PPS_ENABLED: bool = !cfg!(feature = "no-pps");
const USB_PD_NEGOTIATION_FOCUS_SLICE: Duration = Duration::from_millis(25);
const WEB_SERIAL_SERVICE_INTERVAL: Duration = Duration::from_millis(100);
const MCU_WATCHDOG_BOOT_TIMEOUT: Duration = Duration::from_secs(60);
const MCU_WATCHDOG_RUNTIME_TIMEOUT: Duration = Duration::from_secs(8);

#[unsafe(link_section = ".rtc_slow.persistent.boot_recovery")]
static mut BOOT_RECOVERY_SLOTS: [[u8; esp_firmware::boot_recovery::RECORD_LEN]; 2] =
    [[0; esp_firmware::boot_recovery::RECORD_LEN]; 2];

fn read_boot_recovery_slots() -> [[u8; esp_firmware::boot_recovery::RECORD_LEN]; 2] {
    unsafe { ptr::read_volatile(ptr::addr_of!(BOOT_RECOVERY_SLOTS)) }
}

fn write_boot_recovery_record(record: esp_firmware::boot_recovery::BootRecord) {
    let slot = esp_firmware::boot_recovery::next_slot(record);
    let encoded = record.encode();
    unsafe {
        ptr::write_volatile(ptr::addr_of_mut!(BOOT_RECOVERY_SLOTS[slot]), encoded);
    }
}

fn normalized_reset_cause() -> esp_firmware::boot_recovery::ResetCause {
    use esp_firmware::boot_recovery::ResetCause;
    use esp_hal::rtc_cntl::SocResetReason;
    match esp_hal::system::reset_reason() {
        Some(SocResetReason::ChipPowerOn) => ResetCause::PowerOn,
        Some(SocResetReason::CoreSw | SocResetReason::CpuSw) => ResetCause::Software,
        Some(
            SocResetReason::CoreMwdt0
            | SocResetReason::CoreMwdt1
            | SocResetReason::CoreRtcWdt
            | SocResetReason::CpuMwdt0
            | SocResetReason::CpuMwdt1
            | SocResetReason::CpuRtcWdt
            | SocResetReason::SysRtcWdt,
        ) => ResetCause::Watchdog,
        Some(SocResetReason::SysBrownOut | SocResetReason::CorePwrGlitch) => ResetCause::Brownout,
        Some(SocResetReason::CoreUsbJtag | SocResetReason::CoreUsbUart) => {
            ResetCause::ExternalDebug
        }
        _ => ResetCause::Unknown,
    }
}

// External SYNC for TPS55288 DITH/SYNC pins (SYNCA=0°, SYNCB=180°).
// RFSW on board is 43kΩ (U17/U18 pin 8), so nominal fSW ≈ 20MHz / 43kΩ ≈ 465kHz.
// External clock must be within ±30% of the configured fSW.
// Debug: disable external SYNC to check if INA3221 shunt readings are polluted by coupling.
const TPS_SYNC_ENABLE: bool = true;
const TPS_SYNC_FREQ_KHZ: u32 = 465;
const TPS_SYNC_DUTY_PCT: u8 = 50;
const TPS_SYNC_PHASE_TICKS: u16 = 64; // 180° at Duty7Bit => 128 ticks/period.
const FRONT_PANEL_BACKLIGHT_PWM_FREQ_KHZ: u32 = 20;
const FRONT_PANEL_BACKLIGHT_PWM_OFF_OUTPUT_HIGH_PCT: u8 = 100;

// Do not assert THERM_KILL_N during normal bring-up.
const FORCE_THERM_KILL_N_ASSERTED: bool = false;

// TMP112A alert settings (Spec tps-tmp112-alert-overtemp-hold).
const TMP112_OUT_A_ADDR: u8 = 0x48;
const TMP112_OUT_B_ADDR: u8 = 0x49;
const TMP112_THIGH_C_X16: i16 = 62 * 16;
const TMP112_TLOW_C_X16: i16 = 60 * 16;
const TMP_OUTPUT_PROTECT_DERATE_C_X16: i16 = 55 * 16;
const TMP_OUTPUT_PROTECT_RESUME_C_X16: i16 = 52 * 16;
const TMP_OUTPUT_PROTECT_SHUTDOWN_C_X16: i16 = 60 * 16;
const OTHER_OUTPUT_PROTECT_DERATE_C_X16: i16 = 50 * 16;
const OTHER_OUTPUT_PROTECT_RESUME_C_X16: i16 = 47 * 16;
const OTHER_OUTPUT_PROTECT_SHUTDOWN_C_X16: i16 = 55 * 16;
const OUTPUT_PROTECT_TEMP_HOLD: Duration = Duration::from_secs(5);
const OUTPUT_PROTECT_CURRENT_DERATE_MA: i32 = 3_250;
const OUTPUT_PROTECT_CURRENT_RESUME_MA: i32 = 3_000;
const OUTPUT_PROTECT_CURRENT_HOLD: Duration = Duration::from_secs(3);
const OUTPUT_PROTECT_ILIM_STEP_MA: u16 = 250;
const OUTPUT_PROTECT_ILIM_STEP_INTERVAL: Duration = Duration::from_secs(2);
const OUTPUT_PROTECT_MIN_ILIM_MA: u16 = 1_000;
const OUTPUT_PROTECT_SHUTDOWN_VOUT_MV: u16 = 14_000;
const OUTPUT_PROTECT_SHUTDOWN_HOLD: Duration = Duration::from_secs(2);
const FAN_PWM_FREQ_KHZ: u32 = 25;
const FAN_STOP_TEMP_C_X16: i16 = 37 * 16;
const FAN_TARGET_TEMP_C_X16: i16 = 40 * 16;
const FAN_MIN_RUN_PWM_PCT: u8 = 10;
const FAN_STEP_DOWN_PWM_PCT: u8 = 5;
const FAN_STEP_UP_SMALL_DELTA_C_X16: i16 = 1 * 16;
const FAN_STEP_UP_MEDIUM_DELTA_C_X16: i16 = 3 * 16;
const FAN_STEP_UP_SMALL_PWM_PCT: u8 = 5;
const FAN_STEP_UP_MEDIUM_PWM_PCT: u8 = 10;
const FAN_STEP_UP_LARGE_PWM_PCT: u8 = 15;
const FAN_CONTROL_INTERVAL: Duration = Duration::from_millis(500);
const FAN_TACH_TIMEOUT: Duration = Duration::from_secs(2);
const TMP_HW_PROTECT_TEST_MODE: bool = cfg!(feature = "tmp-hw-protect-test");
const FAN_TACH_PULSES_PER_REV: u8 = esp_firmware::fan::tach_pulses_per_rev_from_features();

#[derive(Clone, Copy, PartialEq, Eq)]
struct AppliedFanOutput {
    enabled: bool,
    drive_pct: u8,
    vset_duty_pct: u8,
}

fn latch_fan_vset_fail_safe(fan_vset_fail_safe: &mut Option<Output<'static>>) {
    if fan_vset_fail_safe.is_none() {
        *fan_vset_fail_safe = Some(Output::new(
            unsafe { AnyPin::steal(36) },
            Level::Low,
            OutputConfig::default()
                .with_drive_mode(DriveMode::PushPull)
                .with_pull(Pull::None),
        ));
    } else if let Some(pin) = fan_vset_fail_safe.as_mut() {
        pin.set_low();
    }
}

fn fan_vset_duty_pct_from_drive_pct(enabled: bool, drive_pct: u8) -> u8 {
    if !enabled {
        return 0;
    }

    100u8.saturating_sub(drive_pct.min(100))
}

fn apply_fan_command(
    fan_en: &mut Flex<'_>,
    fan_pwm: &channel::Channel<'_, LowSpeed>,
    applied: &mut Option<AppliedFanOutput>,
    pwm_degraded: &mut bool,
    fan_vset_fail_safe: &mut Option<Output<'static>>,
    status: esp_firmware::fan::Status,
) -> output::AppliedFanState {
    if TMP_HW_PROTECT_TEST_MODE {
        if let Err(err) = fan_pwm.set_duty(0) {
            defmt::warn!("fan: test-mode pwm disable err={=?}", err);
        }
        fan_en.set_low();
        *applied = Some(AppliedFanOutput {
            enabled: false,
            drive_pct: 0,
            vset_duty_pct: 0,
        });
        return output::AppliedFanState {
            command: esp_firmware::fan::FanLevel::Off,
            pwm_pct: 0,
            vset_duty_pct: 0,
            degraded: false,
            disabled_by_feature: true,
        };
    }

    let next = AppliedFanOutput {
        enabled: status.command.enabled(),
        drive_pct: status.pwm_pct,
        vset_duty_pct: fan_vset_duty_pct_from_drive_pct(status.command.enabled(), status.pwm_pct),
    };
    if *pwm_degraded {
        latch_fan_vset_fail_safe(fan_vset_fail_safe);
        fan_en.set_high();
        return output::AppliedFanState {
            command: esp_firmware::fan::FanLevel::High,
            pwm_pct: 100,
            vset_duty_pct: 0,
            degraded: true,
            disabled_by_feature: false,
        };
    }

    if applied.as_ref() == Some(&next) {
        return output::AppliedFanState {
            command: status.command,
            pwm_pct: next.drive_pct,
            vset_duty_pct: next.vset_duty_pct,
            degraded: false,
            disabled_by_feature: false,
        };
    }

    if let Err(err) = fan_pwm.set_duty(next.vset_duty_pct) {
        defmt::error!(
            "fan: pwm apply err vset_duty_pct={} err={=?} fallback=fan_en_high_vset_low",
            next.vset_duty_pct,
            err
        );
        *pwm_degraded = true;
        *applied = None;
        latch_fan_vset_fail_safe(fan_vset_fail_safe);
        fan_en.set_high();
        return output::AppliedFanState {
            command: esp_firmware::fan::FanLevel::High,
            pwm_pct: 100,
            vset_duty_pct: 0,
            degraded: true,
            disabled_by_feature: false,
        };
    }

    if next.enabled {
        fan_en.set_high();
    } else {
        fan_en.set_low();
    }
    *applied = Some(next);
    output::AppliedFanState {
        command: status.command,
        pwm_pct: next.drive_pct,
        vset_duty_pct: next.vset_duty_pct,
        degraded: false,
        disabled_by_feature: false,
    }
}
// Keep enough capacity for bring-up stalls, but cap refill watermarks so runtime cues stay snappy.
const AUDIO_DMA_BUFFER_BYTES: usize = 16 * 4092;
const AUDIO_BOOT_WATERMARK_BYTES: usize = 8 * 4092;
const AUDIO_SELF_TEST_WATERMARK_BYTES: usize = 7 * 4092;
// Opening modal overlays can stall the main loop close to a second while the
// panel redraw completes, so runtime audio needs a larger steady-state buffer.
const AUDIO_RUNTIME_WATERMARK_BYTES: usize = 10 * 4092;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeAudioReprimeResult {
    Ready { refill_budget: u32 },
    Late,
    Fatal,
}

fn log_boot_stage(stage: &'static str) {
    esp_println::println!("boot: stage={}", stage);
    defmt::info!("boot: stage={}", stage);
}

fn log_usb_pd_feature_summary() {
    esp_println::println!(
        "fw: usb_pd_features fixed5={} fixed9={} fixed12={} fixed15={} fixed20={} pps={}",
        USB_PD_FIXED_5V_ENABLED,
        USB_PD_FIXED_9V_ENABLED,
        USB_PD_FIXED_12V_ENABLED,
        USB_PD_FIXED_15V_ENABLED,
        USB_PD_FIXED_20V_ENABLED,
        USB_PD_PPS_ENABLED
    );
    defmt::info!(
        "fw: usb_pd_features fixed5={=bool} fixed9={=bool} fixed12={=bool} fixed15={=bool} fixed20={=bool} pps={=bool}",
        USB_PD_FIXED_5V_ENABLED,
        USB_PD_FIXED_9V_ENABLED,
        USB_PD_FIXED_12V_ENABLED,
        USB_PD_FIXED_15V_ENABLED,
        USB_PD_FIXED_20V_ENABLED,
        USB_PD_PPS_ENABLED
    );
}

fn log_usb_pd_port_state(stage: &'static str, state: esp_firmware::usb_pd::UsbPdPortState) {
    esp_println::println!(
        "usb_pd: stage={} enabled={} ready={} attached={} charge_ready={} unsafe={} vbus_present={}",
        stage,
        state.enabled,
        state.controller_ready,
        state.attached,
        state.charge_ready,
        state.unsafe_source_latched,
        state.vbus_present.unwrap_or(false)
    );
    defmt::info!(
        "usb_pd: stage={} enabled={=bool} ready={=bool} attached={=bool} charge_ready={=bool} unsafe={=bool} vbus_present={=?}",
        stage,
        state.enabled,
        state.controller_ready,
        state.attached,
        state.charge_ready,
        state.unsafe_source_latched,
        state.vbus_present
    );
}

fn audio_refill_budget(available: usize, target_buffered_bytes: usize) -> usize {
    let buffered = AUDIO_DMA_BUFFER_BYTES.saturating_sub(available);
    target_buffered_bytes.saturating_sub(buffered) & !0x3
}

fn spin_delay(wait: Duration) {
    let start = Instant::now();
    while start.elapsed() < wait {}
}

fn prepare_bitbang_pin(pin: &mut Flex<'_>) {
    let input_cfg = InputConfig::default().with_pull(Pull::Up);
    let output_cfg = OutputConfig::default()
        .with_drive_mode(DriveMode::OpenDrain)
        .with_pull(Pull::Up);
    pin.apply_input_config(&input_cfg);
    pin.set_input_enable(true);
    pin.apply_output_config(&output_cfg);
    pin.set_output_enable(true);
    pin.set_high();
}

fn release_bitbang_pin(pin: &mut Flex<'_>) {
    pin.set_high();
    pin.set_output_enable(false);
}

fn bitbang_release(pin: &mut Flex<'_>) {
    pin.set_high();
}

fn bitbang_pull_low(pin: &mut Flex<'_>) {
    pin.set_low();
}

fn bitbang_start(sda: &mut Flex<'_>, scl: &mut Flex<'_>) {
    bitbang_release(sda);
    bitbang_release(scl);
    spin_delay(I2C1_BITBANG_HALF_PERIOD);
    bitbang_pull_low(sda);
    spin_delay(I2C1_BITBANG_HALF_PERIOD);
    bitbang_pull_low(scl);
    spin_delay(I2C1_BITBANG_HALF_PERIOD);
}

fn bitbang_stop(sda: &mut Flex<'_>, scl: &mut Flex<'_>) {
    bitbang_pull_low(sda);
    spin_delay(I2C1_BITBANG_HALF_PERIOD);
    bitbang_release(scl);
    spin_delay(I2C1_BITBANG_HALF_PERIOD);
    bitbang_release(sda);
    spin_delay(I2C1_BITBANG_HALF_PERIOD);
}

fn bitbang_write_byte(sda: &mut Flex<'_>, scl: &mut Flex<'_>, byte: u8) -> bool {
    for shift in (0..8).rev() {
        if ((byte >> shift) & 1) != 0 {
            bitbang_release(sda);
        } else {
            bitbang_pull_low(sda);
        }
        spin_delay(I2C1_BITBANG_HALF_PERIOD);
        bitbang_release(scl);
        spin_delay(I2C1_BITBANG_HALF_PERIOD);
        bitbang_pull_low(scl);
    }

    bitbang_release(sda);
    spin_delay(I2C1_BITBANG_HALF_PERIOD);
    bitbang_release(scl);
    spin_delay(I2C1_BITBANG_HALF_PERIOD / 2);
    let ack = sda.is_low();
    spin_delay(I2C1_BITBANG_HALF_PERIOD / 2);
    bitbang_pull_low(scl);
    spin_delay(I2C1_BITBANG_HALF_PERIOD);
    ack
}

fn bitbang_touch_bq(sda: &mut Flex<'_>, scl: &mut Flex<'_>, addr: u8, cmd: u8) {
    prepare_bitbang_pin(sda);
    prepare_bitbang_pin(scl);
    bitbang_start(sda, scl);
    let addr_ack = bitbang_write_byte(sda, scl, addr << 1);
    let cmd_ack = if addr_ack {
        bitbang_write_byte(sda, scl, cmd)
    } else {
        false
    };
    bitbang_stop(sda, scl);
    release_bitbang_pin(sda);
    release_bitbang_pin(scl);
    defmt::info!(
        "i2c_bitbang_touch: addr=0x{=u8:x} cmd=0x{=u8:x} addr_ack={=bool} cmd_ack={=bool}",
        addr,
        cmd,
        addr_ack,
        cmd_ack
    );
}

fn clear_i2c_bus(sda: &mut Flex<'_>, scl: &mut Flex<'_>, bus: &'static str) {
    let input_cfg = InputConfig::default().with_pull(Pull::Up);
    let output_cfg = OutputConfig::default()
        .with_drive_mode(DriveMode::OpenDrain)
        .with_pull(Pull::Up);

    sda.apply_input_config(&input_cfg);
    sda.set_input_enable(true);
    sda.set_output_enable(false);
    scl.apply_input_config(&input_cfg);
    scl.set_input_enable(true);
    scl.set_output_enable(false);

    let sda_high_before = sda.is_high();
    let scl_high_before = scl.is_high();

    sda.apply_output_config(&output_cfg);
    scl.apply_output_config(&output_cfg);
    sda.set_high();
    scl.set_high();
    sda.set_output_enable(true);
    scl.set_output_enable(true);
    spin_delay(I2C1_BUS_CLEAR_HALF_PERIOD);

    scl.set_low();
    spin_delay(I2C1_BUS_TIMEOUT_LOW);
    scl.set_high();
    spin_delay(I2C1_BUS_CLEAR_HALF_PERIOD);

    for _ in 0..I2C1_BUS_CLEAR_PULSES {
        scl.set_low();
        spin_delay(I2C1_BUS_CLEAR_HALF_PERIOD);
        scl.set_high();
        spin_delay(I2C1_BUS_CLEAR_HALF_PERIOD);
    }

    sda.set_low();
    spin_delay(I2C1_BUS_CLEAR_HALF_PERIOD);
    scl.set_high();
    spin_delay(I2C1_BUS_CLEAR_HALF_PERIOD);
    sda.set_high();
    spin_delay(I2C1_BUS_CLEAR_HALF_PERIOD);

    sda.set_output_enable(false);
    scl.set_output_enable(false);

    let sda_high_after = sda.is_high();
    let scl_high_after = scl.is_high();
    defmt::info!(
        "i2c_bus_clear: bus={} pulses={=u8} timeout_low_ms={=u64} sda_before={=bool} scl_before={=bool} sda_after={=bool} scl_after={=bool}",
        bus,
        I2C1_BUS_CLEAR_PULSES,
        I2C1_BUS_TIMEOUT_LOW.as_millis() as u64,
        sda_high_before,
        scl_high_before,
        sda_high_after,
        scl_high_after
    );
    if !sda_high_after || !scl_high_after {
        defmt::warn!(
            "i2c_bus_clear: bus={} idle_not_high sda_after={=bool} scl_after={=bool}",
            bus,
            sda_high_after,
            scl_high_after
        );
    }
}

#[cfg(not(feature = "net_http"))]
#[main]
fn main() -> ! {
    block_on(firmware_main(()))
}

#[cfg(feature = "net_http")]
#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    firmware_main(spawner).await
}

#[cfg(not(feature = "net_http"))]
type MainEntry = ();

#[cfg(feature = "net_http")]
type MainEntry = Spawner;

async fn firmware_main(main_entry: MainEntry) -> ! {
    #[cfg(not(feature = "net_http"))]
    let _ = main_entry;
    #[cfg(feature = "net_http")]
    let _ = main_entry;

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::_160MHz);
    let peripherals = esp_hal::init(config);
    #[cfg(feature = "web_serial")]
    let mut web_serial = UsbSerialJtag::new(peripherals.USB_DEVICE);
    #[cfg(feature = "web_serial")]
    let mut web_serial_lines = UsbCdcLineBuffer::<1024>::new();
    #[cfg(feature = "web_serial")]
    let web_serial_identity = derive_device_identity(esp_hal::efuse::Efuse::mac_address());

    #[cfg(feature = "net_http")]
    {
        esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 64 * 1024);
    }

    // GPIO interrupt aggregator (see `docs/i2c-address-map.md`).
    let mut _io = Io::new(peripherals.IO_MUX);
    _io.set_interrupt_handler(irq::gpio_isr);

    // Audio demo peripherals (I2S/TDM TX -> MAX98357A).
    let i2s0 = peripherals.I2S0;
    let dma_channel = peripherals.DMA_CH0;
    let audio_bclk = peripherals.GPIO4;
    let audio_ws = peripherals.GPIO5;
    let audio_dout = peripherals.GPIO6;

    // TPS55288 external sync (SYNCA/SYNCB -> DITH/SYNC).
    // Keep these variables alive for the whole program so PWM keeps running.
    let mut _tps_sync_ledc = Ledc::new(peripherals.LEDC);
    _tps_sync_ledc.set_global_slow_clock(LSGlobalClkSource::APBClk);
    let mut _tps_sync_timer0 = _tps_sync_ledc.timer::<LowSpeed>(timer::Number::Timer0);
    let mut _tps_sync_a = _tps_sync_ledc.channel(channel::Number::Channel0, peripherals.GPIO41);
    let mut _tps_sync_b = _tps_sync_ledc.channel(channel::Number::Channel1, peripherals.GPIO42);
    let mut _fan_pwm_timer1 = _tps_sync_ledc.timer::<LowSpeed>(timer::Number::Timer1);
    let mut _fan_pwm_channel =
        _tps_sync_ledc.channel(channel::Number::Channel2, peripherals.GPIO36);
    let mut _front_panel_bl_pwm_timer2 = _tps_sync_ledc.timer::<LowSpeed>(timer::Number::Timer2);
    let mut _front_panel_bl_pwm_channel = None;

    let mut tps_sync_ok = true;
    if TPS_SYNC_ENABLE {
        match _tps_sync_timer0.configure(timer::config::Config {
            duty: timer::config::Duty::Duty7Bit,
            clock_source: timer::LSClockSource::APBClk,
            frequency: Rate::from_khz(TPS_SYNC_FREQ_KHZ),
        }) {
            Ok(()) => {
                let ok_a = _tps_sync_a.configure(channel::config::Config {
                    timer: &_tps_sync_timer0,
                    duty_pct: TPS_SYNC_DUTY_PCT,
                    drive_mode: DriveMode::PushPull,
                });
                let ok_b = _tps_sync_b.configure(channel::config::Config {
                    timer: &_tps_sync_timer0,
                    duty_pct: TPS_SYNC_DUTY_PCT,
                    drive_mode: DriveMode::PushPull,
                });

                match (ok_a, ok_b) {
                    (Ok(()), Ok(())) => {
                        // Apply 180° phase shift to SYNCB via hpoint.
                        let ledc_regs = esp_hal::peripherals::LEDC::regs();
                        ledc_regs
                            .ch(1)
                            .hpoint()
                            .write(|w| unsafe { w.hpoint().bits(TPS_SYNC_PHASE_TICKS) });

                        defmt::info!(
                            "power: tps_sync ok freq_khz={} duty_pct={} phase_ticks={=u16}",
                            TPS_SYNC_FREQ_KHZ,
                            TPS_SYNC_DUTY_PCT,
                            TPS_SYNC_PHASE_TICKS
                        );
                    }
                    (a, b) => {
                        tps_sync_ok = false;
                        defmt::error!("power: tps_sync err ch0={=?} ch1={=?}", a, b);
                    }
                }
            }
            Err(e) => {
                tps_sync_ok = false;
                defmt::error!("power: tps_sync timer err={=?}", e);
            }
        }
    } else {
        tps_sync_ok = false;
        defmt::info!("power: tps_sync disabled (pins reserved)");
    }

    let mut fan_pwm_ready = false;
    match _fan_pwm_timer1.configure(timer::config::Config {
        duty: timer::config::Duty::Duty8Bit,
        clock_source: timer::LSClockSource::APBClk,
        frequency: Rate::from_khz(FAN_PWM_FREQ_KHZ),
    }) {
        Ok(()) => match _fan_pwm_channel.configure(channel::config::Config {
            timer: &_fan_pwm_timer1,
            duty_pct: 0,
            drive_mode: DriveMode::PushPull,
        }) {
            Ok(()) => {
                fan_pwm_ready = true;
                defmt::info!(
                    "fan: pwm ok freq_khz={} duty_pct={} control_interval_ms={=u64}",
                    FAN_PWM_FREQ_KHZ,
                    0,
                    FAN_CONTROL_INTERVAL.as_millis() as u64
                );
            }
            Err(err) => defmt::error!("fan: pwm channel err={=?}", err),
        },
        Err(err) => defmt::error!("fan: pwm timer err={=?}", err),
    }

    let front_panel_backlight = match _front_panel_bl_pwm_timer2.configure(timer::config::Config {
        duty: timer::config::Duty::Duty10Bit,
        clock_source: timer::LSClockSource::APBClk,
        frequency: Rate::from_khz(FRONT_PANEL_BACKLIGHT_PWM_FREQ_KHZ),
    }) {
        Ok(()) => {
            let mut channel3 =
                _tps_sync_ledc.channel(channel::Number::Channel3, peripherals.GPIO13);
            match channel3.configure(channel::config::Config {
                timer: &_front_panel_bl_pwm_timer2,
                duty_pct: FRONT_PANEL_BACKLIGHT_PWM_OFF_OUTPUT_HIGH_PCT,
                drive_mode: DriveMode::PushPull,
            }) {
                Ok(()) => {
                    defmt::info!(
                        "ui: backlight_pwm ok channel=3 freq_khz={} duty_bits=10 idle_output_high_pct={=u8}",
                        FRONT_PANEL_BACKLIGHT_PWM_FREQ_KHZ,
                        FRONT_PANEL_BACKLIGHT_PWM_OFF_OUTPUT_HIGH_PCT
                    );
                    _front_panel_bl_pwm_channel = Some(channel3);
                    front_panel::BacklightControl::LedcChannel3 { brightness_pct: 0 }
                }
                Err(err) => {
                    defmt::error!("ui: backlight_pwm channel err={=?}", err);
                    esp_println::println!(
                        "ui: backlight_pwm channel configure failed; refusing unconfigured PWM backend"
                    );
                    panic!("front panel backlight pwm channel configure failed");
                }
            }
        }
        Err(err) => {
            defmt::error!(
                "ui: backlight_pwm timer err={=?}; falling back to gpio backlight",
                err
            );
            front_panel::BacklightControl::Gpio(Flex::new(peripherals.GPIO13))
        }
    };

    // Ensure the system timer is enabled before calling `Instant::now()`.
    let _systimer = SystemTimer::new(peripherals.SYSTIMER);

    let reset_cause = normalized_reset_cause();
    let previous_boot = esp_firmware::boot_recovery::newest_valid(read_boot_recovery_slots());
    let mut boot_record =
        esp_firmware::boot_recovery::BootRecord::begin_boot(previous_boot, reset_cause);
    write_boot_recovery_record(boot_record);
    esp_firmware::boot_recovery::publish_diagnostics(boot_record);

    // TIMG0 timer0 is owned by esp-rtos; the watchdog remains an independent resource.
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    #[cfg(feature = "net_http")]
    let timg0_timer0 = timg0.timer0;
    let mut wdt0 = timg0.wdt;
    #[cfg(feature = "net_http")]
    esp_rtos::start(timg0_timer0);
    wdt0.set_timeout(MwdtStage::Stage0, MCU_WATCHDOG_BOOT_TIMEOUT);
    wdt0.enable();

    let timg1 = TimerGroup::new(peripherals.TIMG1);
    let mut wdt1 = timg1.wdt;
    wdt1.disable();

    // Human-readable marker (plain serial) to help bring-up when defmt decoding isn't available yet.
    esp_println::println!("esp: boot (serial)");
    defmt::info!("esp: boot");
    log_boot_stage("early_boot");
    defmt::info!(
        "fw: pkg_version={} git_sha={} profile={}",
        env!("CARGO_PKG_VERSION"),
        FW_GIT_SHA,
        FW_BUILD_PROFILE
    );
    esp_println::println!(
        "fw: build_id={} src_hash={} git_dirty={}",
        FW_BUILD_ID,
        FW_SRC_HASH,
        FW_GIT_DIRTY
    );
    defmt::info!(
        "fw: build_id={} src_hash={} git_dirty={}",
        FW_BUILD_ID,
        FW_SRC_HASH,
        FW_GIT_DIRTY
    );
    #[cfg(feature = "net_http")]
    esp_firmware::net::log_wifi_config();
    log_usb_pd_feature_summary();
    defmt::info!(
        "fan: policy stop_c_x16={=i16} target_c_x16={=i16} min_pwm_pct={=u8} step_down_pct={=u8} step_up_small_pct={=u8} step_up_medium_pct={=u8} step_up_large_pct={=u8} control_interval_ms={=u64} tach_timeout_ms={=u64} tach_watchdog_enabled={=bool} tach_ppr={=u8} test_mode={=bool}",
        FAN_STOP_TEMP_C_X16,
        FAN_TARGET_TEMP_C_X16,
        FAN_MIN_RUN_PWM_PCT,
        FAN_STEP_DOWN_PWM_PCT,
        FAN_STEP_UP_SMALL_PWM_PCT,
        FAN_STEP_UP_MEDIUM_PWM_PCT,
        FAN_STEP_UP_LARGE_PWM_PCT,
        FAN_CONTROL_INTERVAL.as_millis() as u64,
        FAN_TACH_TIMEOUT.as_millis() as u64,
        !TMP_HW_PROTECT_TEST_MODE,
        FAN_TACH_PULSES_PER_REV,
        TMP_HW_PROTECT_TEST_MODE
    );
    defmt::info!(
        "fw: default_vout_mv={=u16} default_ilimit_ma={=u16}",
        DEFAULT_VOUT_MV,
        DEFAULT_ILIMIT_MA
    );
    defmt::info!("fw: force_min_charge={=bool}", FORCE_MIN_CHARGE);
    defmt::info!(
        "fw: bq40_addr_mode={} addr7=0x0b addr8_w=0x16 addr8_r=0x17",
        if cfg!(feature = "bms-dual-probe-diag") {
            "dual-diag"
        } else {
            "canonical"
        }
    );
    defmt::info!("fw: i2c1_khz={=u32}", I2C1_FREQ_KHZ);

    let mut i2c1_sda = Flex::new(peripherals.GPIO48);
    let mut i2c1_scl = Flex::new(peripherals.GPIO47);
    clear_i2c_bus(&mut i2c1_sda, &mut i2c1_scl, "i2c1");
    if BMS_PRETOUCH_ENABLED {
        bitbang_touch_bq(
            &mut i2c1_sda,
            &mut i2c1_scl,
            esp_firmware::bq40z50::I2C_ADDRESS_PRIMARY,
            esp_firmware::bq40z50::cmd::RELATIVE_STATE_OF_CHARGE,
        );
    }

    let i2c1_config = I2cConfig::default()
        .with_frequency(Rate::from_khz(I2C1_FREQ_KHZ))
        .with_software_timeout(SoftwareTimeout::Transaction(Duration::from_millis(100)));
    let mut i2c: I2c<'static, Blocking> = I2c::new(peripherals.I2C1, i2c1_config)
        .unwrap()
        .with_sda(i2c1_sda.into_peripheral_output())
        .with_scl(i2c1_scl.into_peripheral_output());
    log_boot_stage("i2c1_ready");

    let i2c1_int_cfg = InputConfig::default().with_pull(Pull::Up);
    let mut i2c1_int = Input::new(peripherals.GPIO33, i2c1_int_cfg);
    i2c1_int.clear_interrupt();
    i2c1_int.listen(Event::FallingEdge);

    // TPS2490 input hot-swap control. GPIO3 drives Q23: HIGH pulls TPS2490 EN low
    // and cuts the input path; LOW releases EN to the hardware UVLO divider.
    let mut ups_in_ce = Flex::new(peripherals.GPIO3);
    ups_in_ce.apply_output_config(
        &OutputConfig::default()
            .with_drive_mode(DriveMode::PushPull)
            .with_pull(Pull::Down),
    );
    ups_in_ce.set_low();
    ups_in_ce.set_output_enable(true);
    let ups_in_pg = Input::new(
        peripherals.GPIO2,
        InputConfig::default().with_pull(Pull::Up),
    );

    // I2C2 interrupt/alert line (open-drain, active-low).
    let i2c2_int_cfg = InputConfig::default().with_pull(Pull::Up);
    let mut _i2c2_int = Input::new(peripherals.GPIO7, i2c2_int_cfg);
    _i2c2_int.clear_interrupt();
    _i2c2_int.listen(Event::FallingEdge);

    // INA3221 alerts (open-drain, active-low).
    let ina_alert_cfg = InputConfig::default().with_pull(Pull::Up);
    let mut _ina_pv = Input::new(peripherals.GPIO37, ina_alert_cfg);
    _ina_pv.clear_interrupt();
    _ina_pv.listen(Event::FallingEdge);
    let mut _ina_critical = Input::new(peripherals.GPIO38, ina_alert_cfg);
    _ina_critical.clear_interrupt();
    _ina_critical.listen(Event::FallingEdge);
    let mut _ina_warning = Input::new(peripherals.GPIO39, ina_alert_cfg);
    _ina_warning.clear_interrupt();
    _ina_warning.listen(Event::FallingEdge);

    // BMS interrupt/alert line (active-high on MCU side after an inverter stage).
    // External pull-up is provided by a resistor network on the mainboard.
    let bms_btp_int_cfg = InputConfig::default().with_pull(Pull::None);
    let mut bms_btp_int_h = Input::new(peripherals.GPIO21, bms_btp_int_cfg);
    bms_btp_int_h.clear_interrupt();
    bms_btp_int_h.listen(Event::RisingEdge);

    // BQ25792 charger control pins.
    //
    // CE is active-low; hardware pull-up keeps it disabled during reset. We still
    // drive it HIGH as early as possible and only enable charging after we can
    // validate charger/battery status over I2C.
    let mut chg_ce = Flex::new(peripherals.GPIO15);
    chg_ce.apply_output_config(
        &OutputConfig::default()
            .with_drive_mode(DriveMode::OpenDrain)
            .with_pull(Pull::Up),
    );
    chg_ce.set_high();
    chg_ce.set_output_enable(true);

    // ILIM_HIZ "brake" (drives an external NMOS that pulls ILIM_HIZ low).
    // Keep deasserted (LOW) during normal bring-up.
    let mut chg_ilim_hiz_brk = Flex::new(peripherals.GPIO16);
    chg_ilim_hiz_brk.apply_output_config(
        &OutputConfig::default()
            .with_drive_mode(DriveMode::PushPull)
            .with_pull(Pull::None),
    );
    chg_ilim_hiz_brk.set_low();
    chg_ilim_hiz_brk.set_output_enable(true);

    // CHG_INT is an open-drain 256us active-low pulse. We poll status registers
    // periodically, but also count pulses via ISR for timely snapshots.
    let chg_int_cfg = InputConfig::default().with_pull(Pull::Up);
    let mut _chg_int = Input::new(peripherals.GPIO17, chg_int_cfg);
    _chg_int.clear_interrupt();
    _chg_int.listen(Event::FallingEdge);

    let fan_tach_cfg = InputConfig::default().with_pull(Pull::Up);
    let mut _fan_tach = Input::new(peripherals.GPIO34, fan_tach_cfg);
    _fan_tach.clear_interrupt();
    _fan_tach.listen(Event::RisingEdge);

    let mut fan_en = Flex::new(peripherals.GPIO35);
    fan_en.apply_output_config(
        &OutputConfig::default()
            .with_drive_mode(DriveMode::PushPull)
            .with_pull(Pull::None),
    );
    fan_en.set_low();
    fan_en.set_output_enable(true);
    let mut fan_vset_fail_safe: Option<Output<'static>> = None;
    if !fan_pwm_ready {
        // If PWM cannot be configured, at least power the fan continuously so cooling
        // does not silently disappear while the control path keeps logging activity.
        latch_fan_vset_fail_safe(&mut fan_vset_fail_safe);
        fan_en.set_high();
        defmt::warn!("fan: pwm unavailable; forcing fan_en high + vset low for fail-safe cooling");
    }

    // Front panel: I2C2 + SPI display bring-up (Spec front-panel-industrial-ui-preview).
    // Keep these variables alive for the whole program.
    let i2c2_config = I2cConfig::default()
        .with_frequency(Rate::from_khz(400))
        .with_software_timeout(SoftwareTimeout::Transaction(Duration::from_millis(100)));
    let i2c2: I2c<'static, Blocking> = I2c::new(peripherals.I2C0, i2c2_config)
        .unwrap()
        .with_sda(peripherals.GPIO8)
        .with_scl(peripherals.GPIO9);

    let spi_cfg = SpiConfig::default()
        .with_frequency(Rate::from_mhz(10))
        .with_mode(SpiMode::_0);
    let spi: Spi<'static, Blocking> = Spi::new(peripherals.SPI2, spi_cfg)
        .unwrap()
        .with_sck(peripherals.GPIO12)
        .with_mosi(peripherals.GPIO11);

    let tca_reset_n = Flex::new(peripherals.GPIO1);
    let dc = Flex::new(peripherals.GPIO10);
    let btn_center = Input::new(
        peripherals.GPIO0,
        InputConfig::default().with_pull(Pull::None),
    );
    let ctp_irq = Input::new(
        peripherals.GPIO14,
        InputConfig::default().with_pull(Pull::None),
    );

    // Ensure THERM_KILL_N is released. This net can hard-disable both TPS via TPS_EN.
    // Configure as open-drain output, set HIGH (release), and also enable input so we can observe if
    // something external is holding it low.
    let mut therm_kill = Flex::new(peripherals.GPIO40);
    therm_kill.apply_input_config(&InputConfig::default().with_pull(Pull::Up));
    therm_kill.set_input_enable(true);
    let low_before = therm_kill.is_low();
    therm_kill.apply_output_config(
        &OutputConfig::default()
            .with_drive_mode(DriveMode::OpenDrain)
            .with_pull(Pull::Up),
    );
    therm_kill.set_high();
    therm_kill.set_output_enable(true);
    if FORCE_THERM_KILL_N_ASSERTED {
        therm_kill.set_low();
    }
    let low_after = therm_kill.is_low();
    therm_kill.clear_interrupt();
    therm_kill.listen(Event::FallingEdge);
    defmt::info!(
        "power: therm_kill_n low_before={=bool} low_after={=bool} forced={=bool}",
        low_before,
        low_after,
        FORCE_THERM_KILL_N_ASSERTED
    );
    if low_after {
        defmt::warn!(
            "power: therm_kill_n asserted; TPS_EN likely forced low (power stage disabled)"
        );
    }

    // Program TMP112A alert thresholds and debounce.
    let tmp112_cfg = esp_firmware::tmp112::AlertConfig {
        t_high_c_x16: TMP112_THIGH_C_X16,
        t_low_c_x16: TMP112_TLOW_C_X16,
        fault_queue: esp_firmware::tmp112::FaultQueue::F4,
        conversion_rate: esp_firmware::tmp112::ConversionRate::Hz1,
    };
    let mut tmp_out_a_ok = false;
    let mut tmp_out_b_ok = false;
    for addr in [TMP112_OUT_A_ADDR, TMP112_OUT_B_ADDR] {
        match esp_firmware::tmp112::program_alert_config(&mut i2c, addr, tmp112_cfg) {
            Ok(rb) => {
                defmt::info!(
                    "power: tmp112 ok addr=0x{=u8:x} cfg=0x{=u16:x} tlow=0x{=u16:x} thigh=0x{=u16:x}",
                    addr,
                    rb.config,
                    rb.tlow,
                    rb.thigh
                );
                if addr == TMP112_OUT_A_ADDR {
                    tmp_out_a_ok = true;
                }
                if addr == TMP112_OUT_B_ADDR {
                    tmp_out_b_ok = true;
                }
            }
            Err(e) => {
                defmt::error!(
                    "power: tmp112 err addr=0x{=u8:x} err={}",
                    addr,
                    output::i2c_error_kind(e)
                );
            }
        }
    }
    if !tmp_out_a_ok && !tmp_out_b_ok {
        defmt::error!(
            "power: tmp112 init failed for both channels; outputs likely disabled (self-test)"
        );
    }

    // Boot self-test: detect online devices and decide which modules are allowed to run.
    let i2c2_bus = RefCell::new(i2c2);
    let panel_probe = {
        let mut i2c2_probe = RefCellDevice::new(&i2c2_bus);
        output::log_i2c2_presence(&mut i2c2_probe)
    };
    esp_println::println!(
        "self_test: panel screen_present={} typec_present={}",
        panel_probe.screen_present(),
        panel_probe.fusb302_present
    );
    defmt::info!(
        "self_test: panel screen_present={=bool} typec_present={=bool}",
        panel_probe.screen_present(),
        panel_probe.fusb302_present
    );

    log_boot_stage("front_panel_init_begin");
    let mut front_panel = front_panel::FrontPanel::new(
        RefCellDevice::new(&i2c2_bus),
        spi,
        peripherals.DMA_CH1,
        peripherals.PSRAM,
        btn_center,
        ctp_irq,
        tca_reset_n,
        dc,
        front_panel_backlight,
    );
    if !panel_probe.screen_present() {
        defmt::warn!(
            "ui: panel_io probe is missing; attempting display init anyway in case the initial scan was transient"
        );
    }
    front_panel.init_best_effort();
    log_boot_stage("front_panel_init_done");
    front_panel.update_self_check_snapshot(front_panel_scene::SelfCheckUiSnapshot::pending(
        front_panel_scene::UpsMode::Standby,
    ));

    let (_, _, tx_buffer, tx_descriptors) =
        esp_hal::dma_circular_buffers!(0, AUDIO_DMA_BUFFER_BYTES);
    let mut audio_manager = AudioManager::new();
    let mut audio_disabled_reason: Option<&'static str> = None;
    let mut i2s_tx = match I2s::new(
        i2s0,
        dma_channel,
        I2sConfig::new_tdm_philips()
            .with_sample_rate(Rate::from_hz(PLAYBACK_SAMPLE_RATE_HZ))
            .with_data_format(DataFormat::Data16Channel16)
            .with_channels(Channels::STEREO),
    ) {
        Ok(i2s) => Some(
            i2s.i2s_tx
                .with_bclk(audio_bclk)
                .with_ws(audio_ws)
                .with_dout(audio_dout)
                .build(tx_descriptors),
        ),
        Err(err) => {
            defmt::warn!(
                "audio: disable runtime audio because i2s init failed err={=?}",
                err
            );
            audio_disabled_reason = Some("i2s_init_failed");
            None
        }
    };
    let mut audio_transfer = match i2s_tx.as_mut() {
        Some(i2s_tx) => match i2s_tx.write_dma_circular(&tx_buffer) {
            Ok(transfer) => Some(transfer),
            Err(err) => {
                defmt::warn!(
                    "audio: disable runtime audio because dma init failed err={=?}",
                    err
                );
                audio_disabled_reason = Some("dma_init_failed");
                None
            }
        },
        None => None,
    };
    let mut audio_enabled = audio_transfer.is_some();
    if audio_enabled {
        defmt::info!(
            "audio: runtime enabled sample_rate_hz={} bclk_gpio=4 ws_gpio=5 dout_gpio=6",
            PLAYBACK_SAMPLE_RATE_HZ
        );
        esp_println::println!(
            "audio: runtime enabled sample_rate_hz={} bclk_gpio=4 ws_gpio=5 dout_gpio=6",
            PLAYBACK_SAMPLE_RATE_HZ
        );
    } else {
        if audio_disabled_reason.is_none() {
            audio_disabled_reason = Some("init_unavailable");
        }
        defmt::warn!("audio: runtime disabled before boot cue");
        esp_println::println!("audio: runtime disabled before boot cue");
    }
    let mut audio_recovery = RuntimeAudioRecoveryState::new();
    let mut usb_pd = UsbPdSinkManager::new(RefCellDevice::new(&i2c2_bus));
    log_boot_stage("usb_pd_init_begin");
    let initial_pd_state = usb_pd.init_best_effort();
    log_usb_pd_port_state("init_done", initial_pd_state);

    macro_rules! disable_runtime_audio {
        ($reason:expr) => {{
            audio_enabled = false;
            audio_disabled_reason = Some($reason);
            audio_transfer = None;
            audio_manager.stop();
            audio_recovery.clear();
        }};
    }

    macro_rules! reprime_runtime_audio_dma {
        ($push_failed_msg:literal, $available_failed_msg:literal, $restart_failed_msg:literal) => {{
            audio_transfer = None;
            let primed = audio_manager.fill(&mut tx_buffer[..]);
            if primed < tx_buffer.len() {
                tx_buffer[primed..].fill(0);
            }
            if let Some(i2s_tx) = i2s_tx.as_mut() {
                match i2s_tx.write_dma_circular(&tx_buffer) {
                    Ok(mut transfer) => match transfer.available() {
                        Ok(available) if available >= 4 => {
                            let budget =
                                audio_refill_budget(available, AUDIO_RUNTIME_WATERMARK_BYTES);
                            if budget >= 4
                                && transfer
                                    .push_with(|buf| {
                                        let len = budget.min(buf.len()) & !0x3;
                                        audio_manager.fill(&mut buf[..len])
                                    })
                                    .is_err()
                            {
                                defmt::warn!($push_failed_msg);
                                RuntimeAudioReprimeResult::Fatal
                            } else {
                                audio_transfer = Some(transfer);
                                RuntimeAudioReprimeResult::Ready {
                                    refill_budget: budget as u32,
                                }
                            }
                        }
                        Ok(_) => {
                            audio_transfer = Some(transfer);
                            RuntimeAudioReprimeResult::Late
                        }
                        Err(DmaError::Late) => {
                            audio_transfer = Some(transfer);
                            RuntimeAudioReprimeResult::Late
                        }
                        Err(err) => {
                            defmt::warn!($available_failed_msg, err);
                            RuntimeAudioReprimeResult::Fatal
                        }
                    },
                    Err(err) => {
                        defmt::warn!($restart_failed_msg, err);
                        RuntimeAudioReprimeResult::Fatal
                    }
                }
            } else {
                RuntimeAudioReprimeResult::Fatal
            }
        }};
    }

    macro_rules! log_runtime_audio_recovered {
        ($refill_budget:expr) => {{
            if let Some(snapshot) = audio_recovery.note_transport_healthy() {
                let status = audio_manager.status();
                defmt::info!(
                    "audio: dma underrun recovered current={=?} queued={=u8} refill_budget={=u32} consecutive_late={=u8} recovery_attempts={=u8}",
                    status.current,
                    status.queued,
                    ($refill_budget) as u32,
                    snapshot.consecutive_late,
                    snapshot.recovery_attempts
                );
            }
        }};
    }

    macro_rules! service_runtime_audio {
        ($power:ident) => {{
            if audio_enabled {
                let now = Instant::now();
                let audio_edges = $power.take_audio_edges();
                let flush_runtime_audio = audio_edges.battery_low_changed.is_some()
                    || audio_edges.module_fault_changed.is_some()
                    || audio_edges.battery_protection_changed.is_some();
                sync_runtime_audio(&mut audio_manager, now, $power.audio_signals(), audio_edges);
                audio_manager.tick(now);
                if flush_runtime_audio {
                    audio_manager.arm_transition_bridge();
                    match reprime_runtime_audio_dma!(
                        "audio: dma transition flush push failed; disabling runtime audio",
                        "audio: dma transition flush available failed err={=?}; disabling runtime audio",
                        "audio: dma transition flush restart failed err={=?}; disabling runtime audio"
                    ) {
                        RuntimeAudioReprimeResult::Ready { .. } => {
                            audio_recovery.clear();
                        }
                        RuntimeAudioReprimeResult::Late => {}
                        RuntimeAudioReprimeResult::Fatal => {
                            disable_runtime_audio!("transition_flush_reprime_failed");
                        }
                    }
                    continue;
                }

                let mut disable_audio = false;
                let mut underrun_disable_logged = false;
                if let Some(audio_transfer) = audio_transfer.as_mut() {
                    match audio_transfer.available() {
                        Ok(available) if available >= 4 => {
                            let budget =
                                audio_refill_budget(available, AUDIO_RUNTIME_WATERMARK_BYTES);
                            if budget >= 4 {
                                if audio_transfer
                                    .push_with(|buf| {
                                        let len = budget.min(buf.len()) & !0x3;
                                        audio_manager.fill(&mut buf[..len])
                                    })
                                    .is_err()
                                {
                                    defmt::warn!("audio: dma push failed; disabling runtime audio");
                                    audio_disabled_reason = Some("runtime_dma_push_failed");
                                    disable_audio = true;
                                } else {
                                    log_runtime_audio_recovered!(budget);
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(DmaError::Late) => {
                            let status = audio_manager.status();
                            match audio_recovery.note_late(now) {
                                RuntimeAudioRecoveryDecision::AttemptRecover {
                                    first_in_burst,
                                    snapshot,
                                } => {
                                    if first_in_burst {
                                        defmt::warn!(
                                            "audio: dma underrun detected current={=?} queued={=u8} refill_budget={=u32} consecutive_late={=u8} recovery_attempts={=u8}",
                                            status.current,
                                            status.queued,
                                            0u32,
                                            snapshot.consecutive_late,
                                            snapshot.recovery_attempts
                                        );
                                    }
                                    match reprime_runtime_audio_dma!(
                                        "audio: dma recovery push failed; disabling runtime audio",
                                        "audio: dma recovery available failed err={=?}; disabling runtime audio",
                                        "audio: dma recovery restart failed err={=?}; disabling runtime audio"
                                    ) {
                                        RuntimeAudioReprimeResult::Ready { .. } => {}
                                        RuntimeAudioReprimeResult::Late => {}
                                        RuntimeAudioReprimeResult::Fatal => {
                                            audio_disabled_reason =
                                                Some("runtime_dma_reprime_failed");
                                            disable_audio = true;
                                        }
                                    }
                                }
                                RuntimeAudioRecoveryDecision::Disable { snapshot } => {
                                    defmt::warn!(
                                        "audio: dma underrun disabled current={=?} queued={=u8} refill_budget={=u32} consecutive_late={=u8} recovery_attempts={=u8}",
                                        status.current,
                                        status.queued,
                                        0u32,
                                        snapshot.consecutive_late,
                                        snapshot.recovery_attempts
                                    );
                                    audio_disabled_reason = Some("runtime_dma_late");
                                    disable_audio = true;
                                    underrun_disable_logged = true;
                                }
                            }
                        }
                        Err(err) => {
                            defmt::warn!(
                                "audio: dma available failed err={=?}; disabling runtime audio",
                                err
                            );
                            audio_disabled_reason = Some("runtime_dma_available_failed");
                            disable_audio = true;
                        }
                    }
                } else {
                    audio_disabled_reason = Some("runtime_dma_missing_transfer");
                    disable_audio = true;
                }
                if disable_audio {
                    if !underrun_disable_logged {
                        if let Some(snapshot) = audio_recovery.snapshot_if_active() {
                            let status = audio_manager.status();
                            defmt::warn!(
                                "audio: dma underrun disabled current={=?} queued={=u8} refill_budget={=u32} consecutive_late={=u8} recovery_attempts={=u8}",
                                status.current,
                                status.queued,
                                0u32,
                                snapshot.consecutive_late,
                                snapshot.recovery_attempts
                            );
                        }
                    }
                    disable_runtime_audio!(
                        audio_disabled_reason.unwrap_or("runtime_dma_disabled")
                    );
                }
            }
        }};
    }

    macro_rules! trigger_action_feedback {
        ($trigger:expr, $disable_reason:expr, $push_failed_msg:literal, $available_failed_msg:literal, $restart_failed_msg:literal) => {{
            $trigger;
            if audio_enabled {
                audio_manager.arm_transition_bridge();
                match reprime_runtime_audio_dma!(
                    $push_failed_msg,
                    $available_failed_msg,
                    $restart_failed_msg
                ) {
                    RuntimeAudioReprimeResult::Ready { .. } => {
                        audio_recovery.clear();
                    }
                    RuntimeAudioReprimeResult::Late => {}
                    RuntimeAudioReprimeResult::Fatal => {
                        disable_runtime_audio!($disable_reason);
                    }
                }
            }
        }};
    }

    if audio_enabled {
        defmt::info!("audio: trigger boot cue");
        esp_println::println!("audio: trigger boot cue");
        audio_manager.trigger(AudioCue::BootStartup);
        let mut disable_audio = false;
        if let Some(audio_transfer) = audio_transfer.as_mut() {
            match audio_transfer.available() {
                Ok(available) if available >= 4 => {
                    let budget = audio_refill_budget(available, AUDIO_BOOT_WATERMARK_BYTES);
                    defmt::info!(
                        "audio: boot prefill available={} budget={}",
                        available,
                        budget
                    );
                    esp_println::println!(
                        "audio: boot prefill available={} budget={}",
                        available,
                        budget
                    );
                    if budget >= 4
                        && audio_transfer
                            .push_with(|buf| {
                                let len = budget.min(buf.len()) & !0x3;
                                audio_manager.fill(&mut buf[..len])
                            })
                            .is_err()
                    {
                        defmt::warn!(
                            "audio: dma push failed during boot prefill; disabling runtime audio"
                        );
                        audio_disabled_reason = Some("boot_prefill_push_failed");
                        disable_audio = true;
                    }
                }
                Ok(available) => {
                    defmt::info!("audio: boot prefill skipped available={}", available);
                    esp_println::println!("audio: boot prefill skipped available={}", available);
                }
                Err(DmaError::Late) => {
                    match reprime_runtime_audio_dma!(
                        "audio: boot prefill recovery push failed; disabling runtime audio",
                        "audio: boot prefill recovery available failed err={=?}; disabling runtime audio",
                        "audio: boot prefill recovery restart failed err={=?}; disabling runtime audio"
                    ) {
                        RuntimeAudioReprimeResult::Ready { .. } => {
                            audio_recovery.clear();
                        }
                        RuntimeAudioReprimeResult::Late => {}
                        RuntimeAudioReprimeResult::Fatal => {
                            audio_disabled_reason = Some("boot_prefill_reprime_failed");
                            disable_audio = true;
                        }
                    }
                }
                Err(err) => {
                    defmt::warn!(
                        "audio: dma available failed during boot prefill err={=?}; disabling runtime audio",
                        err
                    );
                    audio_disabled_reason = Some("boot_prefill_available_failed");
                    disable_audio = true;
                }
            }
        } else {
            audio_disabled_reason = Some("boot_prefill_missing_transfer");
            disable_audio = true;
        }
        if disable_audio {
            audio_enabled = false;
            audio_transfer = None;
            audio_manager.stop();
        }
    }

    log_boot_stage("boot_self_test_begin");
    let mut self_test_audio_late_logged = false;
    let self_test = output::boot_self_test_with_report(
        &mut i2c,
        DEFAULT_ENABLED_OUTPUTS,
        DEFAULT_VOUT_MV,
        DEFAULT_ILIMIT_MA,
        TELEMETRY_INCLUDE_VIN_CH3,
        tmp_out_a_ok,
        tmp_out_b_ok,
        tps_sync_ok,
        panel_probe,
        low_after,
        FORCE_MIN_CHARGE,
        BMS_BOOT_DIAG_AUTO_VALIDATE,
        BMS_BOOT_DIAG_AUTO_VALIDATE,
        |_, snapshot| {
            front_panel.update_self_check_snapshot(snapshot);
            if audio_enabled {
                let now = Instant::now();
                audio_manager.tick(now);
                let mut disable_audio = false;
                if let Some(audio_transfer) = audio_transfer.as_mut() {
                    match audio_transfer.available() {
                        Ok(available) if available >= 4 => {
                            let budget =
                                audio_refill_budget(available, AUDIO_SELF_TEST_WATERMARK_BYTES);
                            if budget >= 4
                                && audio_transfer
                                    .push_with(|buf| {
                                        let len = budget.min(buf.len()) & !0x3;
                                        audio_manager.fill(&mut buf[..len])
                                    })
                                    .is_err()
                            {
                                defmt::warn!(
                                    "audio: dma push failed during self-test; disabling runtime audio"
                                );
                                audio_disabled_reason = Some("self_test_push_failed");
                                disable_audio = true;
                            }
                        }
                        Ok(_) => {}
                        Err(DmaError::Late) => {
                            if !self_test_audio_late_logged {
                                defmt::warn!(
                                    "audio: dma late during self-test; deferring runtime recovery"
                                );
                                self_test_audio_late_logged = true;
                            }
                        }
                        Err(err) => {
                            defmt::warn!(
                                "audio: dma available failed during self-test err={=?}; disabling runtime audio",
                                err
                            );
                            audio_disabled_reason = Some("self_test_available_failed");
                            disable_audio = true;
                        }
                    }
                } else {
                    audio_disabled_reason = Some("self_test_missing_transfer");
                    disable_audio = true;
                }
                if disable_audio {
                    audio_enabled = false;
                    audio_transfer = None;
                    audio_manager.stop();
                }
            }
        },
    );
    log_boot_stage("boot_self_test_done");

    let cfg = output::Config {
        firmware_safe_mode: boot_record.safe_mode(),
        ina_detected: self_test.ina_detected,
        detected_tmp_outputs: self_test.detected_tmp_outputs,
        detected_tps_outputs: self_test.detected_tps_outputs,
        requested_outputs: self_test.requested_outputs,
        active_outputs: self_test.active_outputs,
        recoverable_outputs: self_test.recoverable_outputs,
        output_gate_reason: self_test.output_gate_reason,
        vout_mv: DEFAULT_VOUT_MV,
        standby_vout_mv: DEFAULT_STANDBY_VOUT_MV,
        assist_low_vout_mv: DEFAULT_ASSIST_LOW_VOUT_MV,
        ilimit_ma: DEFAULT_ILIMIT_MA,
        telemetry_period: TELEMETRY_PERIOD,
        retry_backoff: RETRY_BACKOFF,
        fault_log_min_interval: FAULT_LOG_MIN_INTERVAL,
        telemetry_include_vin_ch3: TELEMETRY_INCLUDE_VIN_CH3,
        tmp112_tlow_c_x16: TMP112_TLOW_C_X16,
        tmp112_thigh_c_x16: TMP112_THIGH_C_X16,
        protect_tmp_temp_derate_c_x16: TMP_OUTPUT_PROTECT_DERATE_C_X16,
        protect_tmp_temp_resume_c_x16: TMP_OUTPUT_PROTECT_RESUME_C_X16,
        protect_tmp_temp_shutdown_c_x16: TMP_OUTPUT_PROTECT_SHUTDOWN_C_X16,
        protect_other_temp_derate_c_x16: OTHER_OUTPUT_PROTECT_DERATE_C_X16,
        protect_other_temp_resume_c_x16: OTHER_OUTPUT_PROTECT_RESUME_C_X16,
        protect_other_temp_shutdown_c_x16: OTHER_OUTPUT_PROTECT_SHUTDOWN_C_X16,
        protect_temp_hold: OUTPUT_PROTECT_TEMP_HOLD,
        protect_current_derate_ma: OUTPUT_PROTECT_CURRENT_DERATE_MA,
        protect_current_resume_ma: OUTPUT_PROTECT_CURRENT_RESUME_MA,
        protect_current_hold: OUTPUT_PROTECT_CURRENT_HOLD,
        protect_ilim_step_ma: OUTPUT_PROTECT_ILIM_STEP_MA,
        protect_ilim_step_interval: OUTPUT_PROTECT_ILIM_STEP_INTERVAL,
        protect_min_ilim_ma: OUTPUT_PROTECT_MIN_ILIM_MA,
        protect_shutdown_vout_mv: OUTPUT_PROTECT_SHUTDOWN_VOUT_MV,
        protect_shutdown_hold: OUTPUT_PROTECT_SHUTDOWN_HOLD,
        fan_config: esp_firmware::fan::Config {
            stop_temp_c_x16: FAN_STOP_TEMP_C_X16,
            target_temp_c_x16: FAN_TARGET_TEMP_C_X16,
            min_run_pwm_pct: FAN_MIN_RUN_PWM_PCT,
            step_down_pwm_pct: FAN_STEP_DOWN_PWM_PCT,
            step_up_small_delta_c_x16: FAN_STEP_UP_SMALL_DELTA_C_X16,
            step_up_medium_delta_c_x16: FAN_STEP_UP_MEDIUM_DELTA_C_X16,
            step_up_small_pwm_pct: FAN_STEP_UP_SMALL_PWM_PCT,
            step_up_medium_pwm_pct: FAN_STEP_UP_MEDIUM_PWM_PCT,
            step_up_large_pwm_pct: FAN_STEP_UP_LARGE_PWM_PCT,
            control_interval_ms: FAN_CONTROL_INTERVAL.as_millis() as u64,
            tach_timeout_ms: FAN_TACH_TIMEOUT.as_millis(),
            tach_pulses_per_rev: FAN_TACH_PULSES_PER_REV,
            tach_watchdog_enabled: !TMP_HW_PROTECT_TEST_MODE,
        },
        fan_control_enabled: !TMP_HW_PROTECT_TEST_MODE,
        thermal_protection_enabled: !TMP_HW_PROTECT_TEST_MODE,
        tmp_hw_protect_test_mode: TMP_HW_PROTECT_TEST_MODE,
        charger_probe_ok: self_test.charger_probe_ok,
        charger_enabled: self_test.charger_enabled,
        initial_audio_charge_phase: self_test.initial_audio_charge_phase,
        initial_bms_protection_active: self_test.initial_bms_protection_active,
        initial_tps_a_over_voltage: self_test.initial_tps_a_over_voltage,
        initial_tps_b_over_voltage: self_test.initial_tps_b_over_voltage,
        initial_tps_a_over_current: self_test.initial_tps_a_over_current,
        initial_tps_b_over_current: self_test.initial_tps_b_over_current,
        force_min_charge: FORCE_MIN_CHARGE,
        bms_boot_diag_auto_validate: BMS_BOOT_DIAG_AUTO_VALIDATE,
        bms_addr: self_test.bms_addr,
        self_check_snapshot: self_test.self_check_snapshot,
    };

    let mut power = output::PowerManager::new(
        i2c,
        i2c1_int,
        bms_btp_int_h,
        ups_in_ce,
        ups_in_pg,
        therm_kill,
        chg_ce,
        chg_ilim_hiz_brk,
        cfg,
    );
    let beeper_prefs = power.beeper_prefs_snapshot();
    front_panel.set_beeper_prefs(beeper_prefs);
    audio_manager.set_action_volume_step(beeper_volume_step(beeper_prefs.action_volume));
    audio_manager.set_system_volume_step(beeper_volume_step(beeper_prefs.system_volume));
    defmt::info!(
        "power: requested_outputs={} active_outputs={} recoverable_outputs={} gate_reason={} target_vout_mv={=u16} target_ilimit_ma={=u16}",
        cfg.requested_outputs.describe(),
        cfg.active_outputs.describe(),
        cfg.recoverable_outputs.describe(),
        cfg.output_gate_reason.as_str(),
        cfg.vout_mv,
        cfg.ilimit_ma
    );
    power.init_best_effort();
    if boot_record.safe_mode() {
        power.enter_firmware_safe_mode();
        defmt::error!(
            "boot: safe_mode reset={} abnormal_boots={} rollback=unsupported_layout",
            boot_record.last_reset.as_str(),
            boot_record.abnormal_boots
        );
        esp_println::println!(
            "boot: RECOVERY SAFE MODE reset={} abnormal_boots={} recovery=install_confirmed_firmware rollback=unsupported_layout",
            boot_record.last_reset.as_str(),
            boot_record.abnormal_boots
        );
    }
    log_boot_stage("power_init_done");
    power.update_usb_pd_state(initial_pd_state);
    #[cfg(feature = "net_http")]
    {
        let usb_wifi_config = match power.read_web_serial_wifi_config() {
            Ok(config) => {
                if config.is_some() {
                    esp_println::println!("net: usb wifi config loaded from eeprom");
                    defmt::info!("net: usb wifi config loaded from eeprom");
                }
                config
            }
            Err(_) => {
                esp_println::println!("net: usb wifi config load failed; wifi disabled");
                defmt::warn!("net: usb wifi config load failed; wifi disabled");
                None
            }
        };
        let prefs = power.manual_charge_prefs_snapshot();
        esp_firmware::net::set_manual_charge_settings(
            manual_charge_target_api_value(prefs.target),
            manual_charge_speed_api_value(prefs.speed),
            prefs.timer_limit.hours(),
            manual_charge_power_path_api_value(prefs.power_path),
        );
        esp_firmware::net::publish_charge_control_detail(
            power.current_manual_charge_control_detail_snapshot(),
        );
        sync_advanced_power_net_settings(&power);
        esp_firmware::net::set_device_log_level("info");
        esp_firmware::net::spawn_wifi_and_http(&main_entry, peripherals.WIFI, usb_wifi_config);
        yield_now().await;
    }
    let initial_snapshot = power.ui_snapshot();
    net_bridge::publish_status_snapshot(initial_snapshot);
    #[cfg(feature = "net_http")]
    esp_firmware::net::publish_charge_control_detail(
        power.current_manual_charge_control_detail_snapshot(),
    );
    #[cfg(feature = "net_http")]
    esp_firmware::net::publish_diag_snapshot(power.derived_power_snapshot());
    front_panel.update_self_check_snapshot(initial_snapshot);
    front_panel.update_bms_activation_state(power.bms_activation_state());
    if boot_record.safe_mode() {
        front_panel
            .enter_firmware_safe_mode(boot_record.last_reset.as_str(), boot_record.abnormal_boots);
    }
    let mut last_dashboard_block_reason = None;
    if front_panel_scene::self_check_can_enter_dashboard(&initial_snapshot) {
        front_panel.enter_dashboard();
    } else {
        last_dashboard_block_reason =
            front_panel_scene::self_check_dashboard_block_reason(&initial_snapshot);
        defmt::warn!(
            "ui: stay on self-check reason={}",
            last_dashboard_block_reason.unwrap_or("boot_self_check_not_clear")
        );
    }
    let mut applied_fan = None;
    let mut fan_pwm_degraded = false;
    let mut applied_fan_state = output::AppliedFanState {
        command: esp_firmware::fan::FanLevel::Off,
        pwm_pct: 0,
        vset_duty_pct: 0,
        degraded: false,
        disabled_by_feature: TMP_HW_PROTECT_TEST_MODE,
    };
    if fan_pwm_ready {
        applied_fan_state = apply_fan_command(
            &mut fan_en,
            &_fan_pwm_channel,
            &mut applied_fan,
            &mut fan_pwm_degraded,
            &mut fan_vset_fail_safe,
            power.fan_command(),
        );
    }
    power.set_applied_fan_state(applied_fan_state);

    if audio_enabled {
        sync_runtime_audio(
            &mut audio_manager,
            Instant::now(),
            power.audio_signals(),
            power.take_audio_edges(),
        );
        audio_manager.tick(Instant::now());
    }
    front_panel.set_attention_hold(front_panel_attention_hold(
        power.audio_signals(),
        !front_panel_scene::self_check_can_enter_dashboard(&initial_snapshot),
    ));

    let mut irq_tracker = irq::IrqTracker::new();
    let pd_started_at = Instant::now();
    let boot_stable_started_at = Instant::now();
    let mut boot_marked_healthy = false;
    let mut last_irq_log_at: Option<Instant> = None;
    let mut last_fan_tach_log_at: Option<Instant> = None;
    let mut last_audio_diag_at: Option<Instant> = None;
    let mut usb_c_insert_feedback_tracker =
        esp_firmware::usb_pd::UsbCInsertFeedbackTracker::new(initial_pd_state);
    #[cfg(feature = "web_serial")]
    let mut web_serial_log_state = UsbCdcLogState::new();
    #[cfg(feature = "web_serial")]
    let mut last_web_serial_service_at: Option<Instant> = None;
    // Arm the short runtime window only after all boot and network initialization completed.
    wdt0.set_timeout(MwdtStage::Stage0, MCU_WATCHDOG_RUNTIME_TIMEOUT);
    wdt0.feed();
    log_boot_stage("main_loop_enter");

    #[cfg(feature = "hil-watchdog-stall")]
    if !boot_record.safe_mode() {
        defmt::error!(
            "hil: watchdog stall injected abnormal_boots={=u8}",
            boot_record.abnormal_boots
        );
        esp_println::println!(
            "hil: WATCHDOG STALL INJECTED abnormal_boots={}",
            boot_record.abnormal_boots
        );
        loop {
            core::hint::spin_loop();
        }
    }

    loop {
        defmt::info!("esp: heartbeat");
        if last_audio_diag_at.is_none_or(|last| last.elapsed() >= Duration::from_secs(10)) {
            last_audio_diag_at = Some(Instant::now());
            let audio_status = audio_manager.status();
            defmt::info!(
                "audio: status enabled={=bool} reason={} playing={=bool} current={=?} route={=?} preview={=bool} queued={} dropped={} preempted={}",
                audio_enabled,
                audio_disabled_reason.unwrap_or("none"),
                audio_status.playing,
                audio_status.current,
                audio_status.current_route,
                audio_status.previewing,
                audio_status.queued,
                audio_status.dropped,
                audio_status.preempted
            );
            esp_println::println!(
                "audio: status enabled={} reason={} playing={} current={:?} route={:?} preview={} queued={} dropped={} preempted={}",
                audio_enabled,
                audio_disabled_reason.unwrap_or("none"),
                audio_status.playing,
                audio_status.current,
                audio_status.current_route,
                audio_status.previewing,
                audio_status.queued,
                audio_status.dropped,
                audio_status.preempted
            );
        }
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(2_000) {
            #[cfg(feature = "web_serial")]
            {
                let web_serial_snapshot = power.ui_snapshot();
                service_web_serial_if_due(
                    &mut web_serial,
                    &mut web_serial_lines,
                    &web_serial_identity,
                    &mut power,
                    web_serial_snapshot,
                    &mut web_serial_log_state,
                    &mut last_web_serial_service_at,
                    false,
                );
            }
            let mut irq_events = irq_tracker.take_delta();
            let usb_pd_now_ms = pd_started_at.elapsed().as_millis() as u32;
            let mut pd_state = usb_pd.tick(
                power.usb_pd_demand(),
                irq_events.i2c2_int != 0,
                usb_pd_now_ms,
            );
            power.update_usb_pd_state(pd_state);
            let mut usb_c_insert_feedback =
                usb_c_insert_feedback_tracker.update(pd_state, usb_pd_now_ms);

            if pd_state.attached && pd_state.contract.is_none() {
                let focus_start = Instant::now();
                while focus_start.elapsed() < USB_PD_NEGOTIATION_FOCUS_SLICE
                    && pd_state.attached
                    && pd_state.contract.is_none()
                {
                    let extra_irq = irq_tracker.take_delta();
                    irq_events.i2c1_int = irq_events.i2c1_int.wrapping_add(extra_irq.i2c1_int);
                    irq_events.i2c2_int = irq_events.i2c2_int.wrapping_add(extra_irq.i2c2_int);
                    irq_events.chg_int = irq_events.chg_int.wrapping_add(extra_irq.chg_int);
                    irq_events.fan_tach = irq_events.fan_tach.wrapping_add(extra_irq.fan_tach);
                    irq_events.ina_pv = irq_events.ina_pv.wrapping_add(extra_irq.ina_pv);
                    irq_events.ina_warning =
                        irq_events.ina_warning.wrapping_add(extra_irq.ina_warning);
                    irq_events.ina_critical =
                        irq_events.ina_critical.wrapping_add(extra_irq.ina_critical);
                    irq_events.bms_btp_int_h = irq_events
                        .bms_btp_int_h
                        .wrapping_add(extra_irq.bms_btp_int_h);
                    irq_events.therm_kill_n =
                        irq_events.therm_kill_n.wrapping_add(extra_irq.therm_kill_n);

                    let usb_pd_now_ms = pd_started_at.elapsed().as_millis() as u32;
                    pd_state = usb_pd.tick(
                        power.usb_pd_demand(),
                        extra_irq.i2c2_int != 0,
                        usb_pd_now_ms,
                    );
                    power.update_usb_pd_state(pd_state);
                    if usb_c_insert_feedback_tracker.update(pd_state, usb_pd_now_ms) {
                        usb_c_insert_feedback = true;
                    }
                    #[cfg(feature = "web_serial")]
                    {
                        let web_serial_snapshot = power.ui_snapshot();
                        service_web_serial_if_due(
                            &mut web_serial,
                            &mut web_serial_lines,
                            &web_serial_identity,
                            &mut power,
                            web_serial_snapshot,
                            &mut web_serial_log_state,
                            &mut last_web_serial_service_at,
                            false,
                        );
                    }
                    service_runtime_audio!(power);
                }
            }
            if usb_c_insert_feedback {
                trigger_action_feedback!(
                    audio_manager.trigger_usb_c_insert(),
                    "usb_c_insert_feedback_reprime_failed",
                    "audio: usb-c insert feedback push failed; disabling runtime audio",
                    "audio: usb-c insert feedback available failed err={=?}; disabling runtime audio",
                    "audio: usb-c insert feedback restart failed err={=?}; disabling runtime audio"
                );
            }

            let fan_telemetry_due = power.tick(&irq_events);
            power.poll_front_panel_bms_discharge_authorization_recovery();
            #[cfg(feature = "web_serial")]
            {
                let web_serial_snapshot = power.ui_snapshot();
                service_web_serial_if_due(
                    &mut web_serial,
                    &mut web_serial_lines,
                    &web_serial_identity,
                    &mut power,
                    web_serial_snapshot,
                    &mut web_serial_log_state,
                    &mut last_web_serial_service_at,
                    false,
                );
            }
            if fan_pwm_ready {
                applied_fan_state = apply_fan_command(
                    &mut fan_en,
                    &_fan_pwm_channel,
                    &mut applied_fan,
                    &mut fan_pwm_degraded,
                    &mut fan_vset_fail_safe,
                    power.fan_command(),
                );
            }
            power.set_applied_fan_state(applied_fan_state);
            if fan_telemetry_due {
                power.log_fan_telemetry(applied_fan_state);
            }
            service_runtime_audio!(power);
            let now = Instant::now();
            let ui_snapshot = power.ui_snapshot();
            net_bridge::publish_status_snapshot(ui_snapshot);
            #[cfg(feature = "net_http")]
            esp_firmware::net::publish_charge_control_detail(
                power.current_manual_charge_control_detail_snapshot(),
            );
            #[cfg(feature = "net_http")]
            esp_firmware::net::publish_diag_snapshot(power.derived_power_snapshot());
            #[cfg(feature = "net_http")]
            {
                while let Some(command) = esp_firmware::net::take_pending_lan_command() {
                    match command {
                        esp_firmware::net::LanManagementCommand::SetWifi(secret) => {
                            let ssid = secret.ssid.clone();
                            match power.write_web_serial_wifi_config(Some(&secret)) {
                                Ok(()) => {
                                    esp_firmware::net::set_usb_wifi_config(Some(secret));
                                    defmt::info!(
                                        "net: LAN WiFi config accepted ssid={}",
                                        ssid.as_str()
                                    );
                                }
                                Err(_) => {
                                    defmt::warn!("net: LAN WiFi config write failed");
                                }
                            }
                        }
                        esp_firmware::net::LanManagementCommand::ClearWifi => {
                            match power.write_web_serial_wifi_config(None) {
                                Ok(()) => {
                                    esp_firmware::net::set_usb_wifi_config(None);
                                    defmt::info!("net: LAN WiFi config cleared");
                                }
                                Err(_) => {
                                    defmt::warn!("net: LAN WiFi config clear failed");
                                }
                            }
                        }
                        esp_firmware::net::LanManagementCommand::SetLogLevel(level) => {
                            #[cfg(feature = "web_serial")]
                            web_serial_log_state.set_level(level);
                            esp_firmware::net::set_device_log_level(level.as_str());
                            defmt::info!("net: LAN log level updated level={}", level.as_str());
                        }
                        esp_firmware::net::LanManagementCommand::SetManualCharge(prefs) => {
                            power.set_web_serial_manual_charge_prefs(prefs);
                            let current = power.manual_charge_prefs_snapshot();
                            esp_firmware::net::set_manual_charge_settings(
                                manual_charge_target_api_value(current.target),
                                manual_charge_speed_api_value(current.speed),
                                current.timer_limit.hours(),
                                manual_charge_power_path_api_value(current.power_path),
                            );
                            esp_firmware::net::publish_charge_control_detail(
                                power.current_manual_charge_control_detail_snapshot(),
                            );
                            defmt::info!("net: LAN manual charge preferences updated");
                        }
                        esp_firmware::net::LanManagementCommand::PreviewChargeControl(prefs) => {
                            let mut body = heapless::String::<
                                { esp_firmware::net::HTTP_RESPONSE_BODY_CAP },
                            >::new();
                            render_charge_control_result_json(
                                &mut body,
                                power.preview_manual_charge_control_detail_snapshot(prefs),
                            );
                            esp_firmware::net::set_lan_command_result(
                                esp_firmware::net::LanCommandResult::Json(body),
                            );
                        }
                        esp_firmware::net::LanManagementCommand::ControlManualCharge(command) => {
                            match power.control_manual_charge(command) {
                                Ok(_) => {
                                    let mut body = heapless::String::<
                                        { esp_firmware::net::HTTP_RESPONSE_BODY_CAP },
                                    >::new();
                                    let detail =
                                        power.current_manual_charge_control_detail_snapshot();
                                    render_charge_control_result_json(&mut body, detail);
                                    esp_firmware::net::publish_charge_control_detail(detail);
                                    esp_firmware::net::set_lan_command_result(
                                        esp_firmware::net::LanCommandResult::Json(body),
                                    );
                                    defmt::info!("net: LAN manual charge control applied");
                                }
                                Err(err) => {
                                    let mut details = heapless::String::<
                                        { esp_firmware::net::HTTP_RESPONSE_BODY_CAP },
                                    >::new();
                                    render_charge_control_result_json(
                                        &mut details,
                                        power.current_manual_charge_control_detail_snapshot(),
                                    );
                                    esp_firmware::net::set_lan_command_result(
                                        esp_firmware::net::LanCommandResult::ManualChargeControlError {
                                            code: err.code,
                                            message: err.message,
                                            details,
                                        },
                                    );
                                    defmt::warn!(
                                        "net: LAN manual charge control rejected code={}",
                                        err.code
                                    );
                                }
                            }
                        }
                        esp_firmware::net::LanManagementCommand::SetAdvancedPower(settings) => {
                            match power.apply_advanced_power_settings(settings) {
                                Ok(()) => {
                                    sync_advanced_power_net_settings(&power);
                                    esp_firmware::net::set_lan_command_result(
                                        esp_firmware::net::LanCommandResult::Ok,
                                    );
                                    defmt::info!("net: LAN advanced power settings updated");
                                }
                                Err(output::AdvancedPowerApplyError::Validation(err)) => {
                                    esp_firmware::net::set_lan_command_result(
                                        esp_firmware::net::LanCommandResult::AdvancedPowerValidation {
                                            code: err.code(),
                                            message: err.message(),
                                        },
                                    );
                                    defmt::warn!(
                                        "net: LAN advanced power update failed err={=?}",
                                        err
                                    );
                                }
                                Err(output::AdvancedPowerApplyError::Storage(err)) => {
                                    esp_firmware::net::set_lan_command_result(
                                        esp_firmware::net::LanCommandResult::AdvancedPowerStorageFailed,
                                    );
                                    defmt::warn!(
                                        "net: LAN advanced power update failed err={=?}",
                                        err
                                    );
                                }
                            }
                        }
                        esp_firmware::net::LanManagementCommand::ResetAdvancedPower => {
                            match power.reset_advanced_power_settings() {
                                Ok(()) => {
                                    sync_advanced_power_net_settings(&power);
                                    esp_firmware::net::set_lan_command_result(
                                        esp_firmware::net::LanCommandResult::Ok,
                                    );
                                    defmt::info!("net: LAN advanced power settings reset");
                                }
                                Err(output::AdvancedPowerApplyError::Validation(err)) => {
                                    esp_firmware::net::set_lan_command_result(
                                        esp_firmware::net::LanCommandResult::AdvancedPowerValidation {
                                            code: err.code(),
                                            message: err.message(),
                                        },
                                    );
                                    defmt::warn!(
                                        "net: LAN advanced power reset failed err={=?}",
                                        err
                                    );
                                }
                                Err(output::AdvancedPowerApplyError::Storage(err)) => {
                                    esp_firmware::net::set_lan_command_result(
                                        esp_firmware::net::LanCommandResult::AdvancedPowerStorageFailed,
                                    );
                                    defmt::warn!(
                                        "net: LAN advanced power reset failed err={=?}",
                                        err
                                    );
                                }
                            }
                        }
                        esp_firmware::net::LanManagementCommand::RecoverBmsDischargeAuthorization => {
                            if let Some(body) = power.begin_bms_discharge_authorization_recovery_json::<
                                { esp_firmware::net::HTTP_RESPONSE_BODY_CAP },
                            >("lan_http") {
                                esp_firmware::net::set_lan_command_result(
                                    esp_firmware::net::LanCommandResult::Json(body),
                                );
                                defmt::info!(
                                    "net: LAN BMS discharge authorization recovery completed"
                                );
                            } else {
                                defmt::info!(
                                    "net: LAN BMS discharge authorization recovery pending"
                                );
                            }
                        }
                        esp_firmware::net::LanManagementCommand::Reset => {
                            defmt::warn!("net: LAN reset requested");
                            Timer::after(embassy_time::Duration::from_millis(100)).await;
                            esp_hal::system::software_reset();
                        }
                    }
                }
                let prefs = power.manual_charge_prefs_snapshot();
                if let Some(body) = power
                    .take_completed_bms_discharge_authorization_recovery_json_for_source::<
                        { esp_firmware::net::HTTP_RESPONSE_BODY_CAP },
                    >("lan_http")
                {
                    esp_firmware::net::set_lan_command_result(
                        esp_firmware::net::LanCommandResult::Json(body),
                    );
                    defmt::info!("net: LAN BMS discharge authorization recovery completed");
                }
                esp_firmware::net::set_manual_charge_settings(
                    manual_charge_target_api_value(prefs.target),
                    manual_charge_speed_api_value(prefs.speed),
                    prefs.timer_limit.hours(),
                    manual_charge_power_path_api_value(prefs.power_path),
                );
                sync_advanced_power_net_settings(&power);
            }
            #[cfg(feature = "web_serial")]
            service_web_serial_if_due(
                &mut web_serial,
                &mut web_serial_lines,
                &web_serial_identity,
                &mut power,
                ui_snapshot,
                &mut web_serial_log_state,
                &mut last_web_serial_service_at,
                true,
            );
            front_panel.update_self_check_snapshot(ui_snapshot);
            front_panel.update_bms_activation_state(power.bms_activation_state());
            let self_check_blocked = front_panel.is_showing_self_check()
                && !front_panel_scene::self_check_can_enter_dashboard(&ui_snapshot);
            front_panel.set_attention_hold(front_panel_attention_hold(
                power.audio_signals(),
                self_check_blocked,
            ));
            if let Some(action) = front_panel.tick() {
                let trigger_interaction_feedback =
                    front_panel::ui_action_triggers_interaction_feedback(&action);
                match action {
                    front_panel::UiAction::RequestBmsRecovery(action) => {
                        power.request_bms_recovery_action(action);
                        front_panel.update_bms_activation_state(power.bms_activation_state());
                    }
                    front_panel::UiAction::ManualCharge(action) => {
                        power.request_manual_charge_action(action);
                    }
                    front_panel::UiAction::BeeperPrefsChanged { prefs } => {
                        power.set_beeper_prefs(prefs);
                    }
                    front_panel::UiAction::BeeperPreview { prefs, target } => {
                        power.set_beeper_prefs(prefs);
                        audio_manager
                            .set_action_volume_step(beeper_volume_step(prefs.action_volume));
                        audio_manager
                            .set_system_volume_step(beeper_volume_step(prefs.system_volume));
                        audio_manager.trigger_volume_preview(match target {
                            front_panel_scene::BeeperSettingTarget::Action => AudioRoute::Action,
                            front_panel_scene::BeeperSettingTarget::System => AudioRoute::System,
                        });
                        if audio_enabled {
                            audio_manager.arm_transition_bridge();
                            match reprime_runtime_audio_dma!(
                                "audio: volume preview push failed; disabling runtime audio",
                                "audio: volume preview available failed err={=?}; disabling runtime audio",
                                "audio: volume preview restart failed err={=?}; disabling runtime audio"
                            ) {
                                RuntimeAudioReprimeResult::Ready { .. } => {
                                    audio_recovery.clear();
                                }
                                RuntimeAudioReprimeResult::Late => {}
                                RuntimeAudioReprimeResult::Fatal => {
                                    disable_runtime_audio!("volume_preview_reprime_failed");
                                }
                            }
                        }
                    }
                    front_panel::UiAction::ClearBmsActivationResult => {
                        power.clear_bms_activation_state();
                        front_panel.update_bms_activation_state(power.bms_activation_state());
                    }
                }
                if trigger_interaction_feedback {
                    front_panel.note_interaction_feedback();
                }
            }
            if front_panel.take_interaction_feedback() {
                trigger_action_feedback!(
                    audio_manager.trigger_interaction_feedback(),
                    "interaction_feedback_reprime_failed",
                    "audio: interaction feedback push failed; disabling runtime audio",
                    "audio: interaction feedback available failed err={=?}; disabling runtime audio",
                    "audio: interaction feedback restart failed err={=?}; disabling runtime audio"
                );
            }
            if front_panel_scene::self_check_can_enter_dashboard(&ui_snapshot) {
                last_dashboard_block_reason = None;
                if matches!(
                    power.bms_activation_state(),
                    front_panel_scene::BmsActivationState::Result(
                        front_panel_scene::BmsResultKind::Success
                    )
                ) {
                    power.clear_bms_activation_state();
                    front_panel.update_bms_activation_state(power.bms_activation_state());
                }
                front_panel.enter_dashboard();
            } else {
                let reason = front_panel_scene::self_check_dashboard_block_reason(&ui_snapshot);
                if reason != last_dashboard_block_reason {
                    last_dashboard_block_reason = reason;
                    defmt::warn!(
                        "ui: stay on self-check reason={}",
                        reason.unwrap_or("self_check_not_clear")
                    );
                    esp_println::println!(
                        "ui: stay on self-check reason={}",
                        reason.unwrap_or("self_check_not_clear")
                    );
                }
            }
            service_runtime_audio!(power);
            if irq_events.any()
                && output::tps55288::should_log_fault(
                    now,
                    &mut last_irq_log_at,
                    Duration::from_millis(200),
                )
            {
                defmt::info!(
                    "irq: i2c1_int={=u32} i2c2_int={=u32} chg_int={=u32} fan_tach={=u32} ina_pv={=u32} ina_warning={=u32} ina_critical={=u32} bms_btp_int_h={=u32} therm_kill_n={=u32}",
                    irq_events.i2c1_int,
                    irq_events.i2c2_int,
                    irq_events.chg_int,
                    irq_events.fan_tach,
                    irq_events.ina_pv,
                    irq_events.ina_warning,
                    irq_events.ina_critical,
                    irq_events.bms_btp_int_h,
                    irq_events.therm_kill_n
                );
            }
            if irq_events.fan_tach != 0
                && output::tps55288::should_log_fault(
                    now,
                    &mut last_fan_tach_log_at,
                    Duration::from_secs(1),
                )
            {
                defmt::info!("irq: fan_tach={=u32}", irq_events.fan_tach);
            }
            #[cfg(feature = "net_http")]
            yield_now().await;
        }
        if !boot_record.safe_mode()
            && !boot_marked_healthy
            && boot_stable_started_at.elapsed()
                >= Duration::from_millis(esp_firmware::boot_recovery::STABLE_RUNTIME_MS as u64)
        {
            boot_record = boot_record.mark_healthy();
            write_boot_recovery_record(boot_record);
            esp_firmware::boot_recovery::publish_diagnostics(boot_record);
            boot_marked_healthy = true;
            defmt::info!(
                "boot: healthy reset={} rollback={}",
                reset_cause.as_str(),
                boot_record.candidate.as_str()
            );
        }
        // This is the only feed point: every critical loop slice above completed.
        wdt0.feed();
    }
}

#[cfg(feature = "web_serial")]
fn service_web_serial_if_due<'d, I2C>(
    serial: &mut UsbSerialJtag<'static, Blocking>,
    lines: &mut UsbCdcLineBuffer<1024>,
    identity: &DeviceIdentity,
    power: &mut output::PowerManager<'d, I2C>,
    ui_snapshot: front_panel_scene::SelfCheckUiSnapshot,
    log_state: &mut UsbCdcLogState,
    last_service_at: &mut Option<Instant>,
    force: bool,
) where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let now = Instant::now();
    if !force
        && last_service_at
            .map(|last| now < last + WEB_SERIAL_SERVICE_INTERVAL)
            .unwrap_or(false)
    {
        return;
    }
    *last_service_at = Some(now);
    service_web_serial(serial, lines, identity, power, ui_snapshot, log_state);
}

#[cfg(feature = "web_serial")]
fn service_web_serial<'d, I2C>(
    serial: &mut UsbSerialJtag<'static, Blocking>,
    lines: &mut UsbCdcLineBuffer<1024>,
    identity: &DeviceIdentity,
    power: &mut output::PowerManager<'d, I2C>,
    ui_snapshot: front_panel_scene::SelfCheckUiSnapshot,
    log_state: &mut UsbCdcLogState,
) where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let mut rx = [0u8; 128];
    let mut count = 0;
    while count < rx.len() {
        match serial.read_byte() {
            Ok(byte) => {
                rx[count] = byte;
                count += 1;
            }
            Err(_) => break,
        }
    }
    for byte in rx.iter().take(count) {
        match lines.push_byte(*byte) {
            Ok(Some(line)) => {
                handle_web_serial_frame(
                    serial,
                    identity,
                    power,
                    ui_snapshot,
                    line.as_str(),
                    log_state,
                );
            }
            Ok(None) => {}
            Err(err) => {
                let mut frame = heapless::String::<512>::new();
                render_protocol_error_json(&mut frame, None, err);
                write_web_serial_line(serial, frame.as_str());
            }
        }
    }
}

#[cfg(feature = "web_serial")]
fn handle_web_serial_frame<'d, I2C>(
    serial: &mut UsbSerialJtag<'static, Blocking>,
    identity: &DeviceIdentity,
    power: &mut output::PowerManager<'d, I2C>,
    ui_snapshot: front_panel_scene::SelfCheckUiSnapshot,
    line: &str,
    log_state: &mut UsbCdcLogState,
) where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let status = net_bridge::build_status_snapshot(ui_snapshot);

    match parse_frame(line) {
        Ok(UsbCdcFrame::Hello { request_id }) => {
            let mut body = heapless::String::<WEB_SERIAL_RESPONSE_BODY_CAP>::new();
            let mut frame = heapless::String::<WEB_SERIAL_RESPONSE_FRAME_CAP>::new();
            log_state.reset();
            render_identity_json_with_write_controls(
                &mut body,
                identity,
                net_bridge::current_wifi_snapshot(),
                WEB_SERIAL_BUILD_INFO,
                true,
            );
            render_hello_json(
                &mut frame,
                request_id.as_ref().map(|id| id.as_str()),
                body.as_str(),
            );
            write_web_serial_line(serial, frame.as_str());
            render_log_json(
                &mut frame,
                LogLevel::Info,
                "usb_cdc",
                "web serial session negotiated",
            );
            write_web_serial_line(serial, frame.as_str());
        }
        Ok(UsbCdcFrame::Request { request_id, op }) => match op {
            UsbCdcRequest::GetIdentity => {
                let mut body = heapless::String::<WEB_SERIAL_RESPONSE_BODY_CAP>::new();
                let mut frame = heapless::String::<WEB_SERIAL_RESPONSE_FRAME_CAP>::new();
                render_identity_json_with_write_controls(
                    &mut body,
                    identity,
                    net_bridge::current_wifi_snapshot(),
                    WEB_SERIAL_BUILD_INFO,
                    true,
                );
                render_response_json(&mut frame, request_id.as_str(), body.as_str());
                write_web_serial_line(serial, frame.as_str());
            }
            UsbCdcRequest::GetStatus => {
                let mut body = heapless::String::<WEB_SERIAL_RESPONSE_BODY_CAP>::new();
                let mut frame = heapless::String::<WEB_SERIAL_RESPONSE_FRAME_CAP>::new();
                if request_id.as_str().starts_with("devd-monitor-status")
                    || request_id.as_str().starts_with("devd-status")
                {
                    render_compact_status_json(&mut body, status);
                } else {
                    render_status_json(&mut body, status);
                }
                render_response_json(&mut frame, request_id.as_str(), body.as_str());
                write_web_serial_line(serial, frame.as_str());
                if !request_id.as_str().starts_with("devd-") {
                    render_status_frame_json(&mut frame, body.as_str());
                    write_web_serial_line(serial, frame.as_str());
                    log_state.emit_status_logs(serial, status);
                }
            }
            UsbCdcRequest::GetSettings => {
                let mut body = heapless::String::<WEB_SERIAL_RESPONSE_BODY_CAP>::new();
                let mut frame = heapless::String::<WEB_SERIAL_RESPONSE_FRAME_CAP>::new();
                {
                    let settings = web_serial_settings_snapshot(power, log_state.level());
                    esp_firmware::net_contract::render_settings_json(&mut body, &settings);
                }
                render_response_json(&mut frame, request_id.as_str(), body.as_str());
                write_web_serial_line(serial, frame.as_str());
            }
            UsbCdcRequest::GetChargeControl => {
                let mut body = heapless::String::<WEB_SERIAL_RESPONSE_BODY_CAP>::new();
                let mut frame = heapless::String::<WEB_SERIAL_RESPONSE_FRAME_CAP>::new();
                render_charge_control_result_json(
                    &mut body,
                    power.current_manual_charge_control_detail_snapshot(),
                );
                render_response_json(&mut frame, request_id.as_str(), body.as_str());
                write_web_serial_line(serial, frame.as_str());
            }
            UsbCdcRequest::GetDiagSnapshot(request) => {
                let mut body = heapless::String::<WEB_SERIAL_DIAG_SNAPSHOT_BODY_CAP>::new();
                let mut frame = heapless::String::<WEB_SERIAL_DIAG_SNAPSHOT_FRAME_CAP>::new();
                power.refresh_diag_snapshot_packages(request.packages.as_slice());
                render_diag_snapshot_json(
                    &mut body,
                    request.packages.as_slice(),
                    status,
                    power.derived_power_snapshot(),
                );
                if !diag_snapshot_response_complete(body.as_str()) {
                    render_error_json(
                        &mut frame,
                        Some(request_id.as_str()),
                        "diag_snapshot_too_large",
                        "diag-snapshot response exceeded USB CDC response capacity",
                        true,
                    );
                    write_web_serial_line(serial, frame.as_str());
                    return;
                }
                render_response_json(&mut frame, request_id.as_str(), body.as_str());
                write_web_serial_line(serial, frame.as_str());
            }
            UsbCdcRequest::RecoverBmsDischargeAuthorization => {
                let body = power
                    .recover_bms_discharge_authorization_json::<WEB_SERIAL_RESPONSE_BODY_CAP>(
                        "usb_cdc",
                    );
                let mut frame = heapless::String::<WEB_SERIAL_RESPONSE_FRAME_CAP>::new();
                render_response_json(&mut frame, request_id.as_str(), body.as_str());
                write_web_serial_line(serial, frame.as_str());
            }
            UsbCdcRequest::SetLogLevel(level) => {
                let mut body = heapless::String::<WEB_SERIAL_RESPONSE_BODY_CAP>::new();
                let mut frame = heapless::String::<WEB_SERIAL_RESPONSE_FRAME_CAP>::new();
                log_state.set_level(level);
                #[cfg(feature = "net_http")]
                esp_firmware::net::set_device_log_level(level.as_str());
                let _ = write!(body, r#"{{"log_level":"{}"}}"#, level.as_str());
                render_response_json(&mut frame, request_id.as_str(), body.as_str());
                write_web_serial_line(serial, frame.as_str());
                log_state.emit(
                    serial,
                    LogLevel::Info,
                    "usb_cdc",
                    "log level updated for USB session",
                );
            }
            UsbCdcRequest::SetManualChargePrefs(prefs) => {
                let mut body = heapless::String::<WEB_SERIAL_RESPONSE_BODY_CAP>::new();
                let mut frame = heapless::String::<WEB_SERIAL_RESPONSE_FRAME_CAP>::new();
                power.set_web_serial_manual_charge_prefs(prefs);
                #[cfg(feature = "net_http")]
                {
                    let current = power.manual_charge_prefs_snapshot();
                    esp_firmware::net::set_manual_charge_settings(
                        manual_charge_target_api_value(current.target),
                        manual_charge_speed_api_value(current.speed),
                        current.timer_limit.hours(),
                        manual_charge_power_path_api_value(current.power_path),
                    );
                }
                let _ = body.push_str(r#"{"manual_charge_prefs":"updated"}"#);
                render_response_json(&mut frame, request_id.as_str(), body.as_str());
                write_web_serial_line(serial, frame.as_str());
                log_state.emit(
                    serial,
                    LogLevel::Info,
                    "manual_charge",
                    "safe manual charge preferences updated over USB",
                );
            }
            UsbCdcRequest::PreviewChargeControl(prefs) => {
                let mut body = heapless::String::<WEB_SERIAL_RESPONSE_BODY_CAP>::new();
                let mut frame = heapless::String::<WEB_SERIAL_RESPONSE_FRAME_CAP>::new();
                render_charge_control_result_json(
                    &mut body,
                    power.preview_manual_charge_control_detail_snapshot(prefs),
                );
                render_response_json(&mut frame, request_id.as_str(), body.as_str());
                write_web_serial_line(serial, frame.as_str());
            }
            UsbCdcRequest::ControlManualCharge(command) => {
                let mut body = heapless::String::<WEB_SERIAL_RESPONSE_BODY_CAP>::new();
                let mut frame = heapless::String::<WEB_SERIAL_RESPONSE_FRAME_CAP>::new();
                match power.control_manual_charge(command) {
                    Ok(_) => {
                        #[cfg(feature = "net_http")]
                        esp_firmware::net::publish_charge_control_detail(
                            power.current_manual_charge_control_detail_snapshot(),
                        );
                        render_charge_control_result_json(
                            &mut body,
                            power.current_manual_charge_control_detail_snapshot(),
                        );
                        render_response_json(&mut frame, request_id.as_str(), body.as_str());
                        write_web_serial_line(serial, frame.as_str());
                    }
                    Err(err) => {
                        render_charge_control_result_json(
                            &mut body,
                            power.current_manual_charge_control_detail_snapshot(),
                        );
                        render_error_json_with_details(
                            &mut frame,
                            Some(request_id.as_str()),
                            err.code,
                            err.message,
                            false,
                            Some(body.as_str()),
                        );
                        write_web_serial_line(serial, frame.as_str());
                    }
                }
            }
            UsbCdcRequest::SetAdvancedPower(settings) => {
                let mut body = heapless::String::<WEB_SERIAL_RESPONSE_BODY_CAP>::new();
                let mut frame = heapless::String::<WEB_SERIAL_RESPONSE_FRAME_CAP>::new();
                match power.apply_advanced_power_settings(settings) {
                    Ok(()) => {
                        #[cfg(feature = "net_http")]
                        sync_advanced_power_net_settings(&power);
                        let _ = body.push_str(r#"{"advanced_power":"updated"}"#);
                        render_response_json(&mut frame, request_id.as_str(), body.as_str());
                        write_web_serial_line(serial, frame.as_str());
                        log_state.emit(
                            serial,
                            LogLevel::Info,
                            "advanced_power",
                            "advanced power settings updated over USB",
                        );
                    }
                    Err(output::AdvancedPowerApplyError::Validation(err)) => {
                        render_error_json(
                            &mut frame,
                            Some(request_id.as_str()),
                            err.code(),
                            err.message(),
                            false,
                        );
                        write_web_serial_line(serial, frame.as_str());
                    }
                    Err(output::AdvancedPowerApplyError::Storage(_)) => {
                        render_error_json(
                            &mut frame,
                            Some(request_id.as_str()),
                            "advanced_power_write_failed",
                            "failed to persist advanced power settings",
                            true,
                        );
                        write_web_serial_line(serial, frame.as_str());
                    }
                }
            }
            UsbCdcRequest::ResetAdvancedPower => {
                let mut body = heapless::String::<WEB_SERIAL_RESPONSE_BODY_CAP>::new();
                let mut frame = heapless::String::<WEB_SERIAL_RESPONSE_FRAME_CAP>::new();
                match power.reset_advanced_power_settings() {
                    Ok(()) => {
                        #[cfg(feature = "net_http")]
                        sync_advanced_power_net_settings(&power);
                        let _ = body.push_str(r#"{"advanced_power":"reset"}"#);
                        render_response_json(&mut frame, request_id.as_str(), body.as_str());
                        write_web_serial_line(serial, frame.as_str());
                        log_state.emit(
                            serial,
                            LogLevel::Info,
                            "advanced_power",
                            "advanced power settings reset over USB",
                        );
                    }
                    Err(output::AdvancedPowerApplyError::Validation(err)) => {
                        render_error_json(
                            &mut frame,
                            Some(request_id.as_str()),
                            err.code(),
                            err.message(),
                            false,
                        );
                        write_web_serial_line(serial, frame.as_str());
                    }
                    Err(output::AdvancedPowerApplyError::Storage(_)) => {
                        render_error_json(
                            &mut frame,
                            Some(request_id.as_str()),
                            "advanced_power_reset_failed",
                            "failed to reset advanced power settings",
                            true,
                        );
                        write_web_serial_line(serial, frame.as_str());
                    }
                }
            }
            UsbCdcRequest::EnableOutputBypass => {
                let mut frame = heapless::String::<WEB_SERIAL_RESPONSE_FRAME_CAP>::new();
                match power.enable_output_bypass() {
                    Ok(()) => {
                        let mut body = heapless::String::<WEB_SERIAL_RESPONSE_BODY_CAP>::new();
                        let _ = body.push_str(r#"{"output_bypass":"enabled"}"#);
                        render_response_json(&mut frame, request_id.as_str(), body.as_str());
                    }
                    Err(code) => render_error_json(
                        &mut frame,
                        Some(request_id.as_str()),
                        code,
                        "output bypass requires a stable VIN source",
                        false,
                    ),
                }
                write_web_serial_line(serial, frame.as_str());
            }
            UsbCdcRequest::RestoreOutput => {
                power.restore_output();
                let mut body = heapless::String::<WEB_SERIAL_RESPONSE_BODY_CAP>::new();
                let mut frame = heapless::String::<WEB_SERIAL_RESPONSE_FRAME_CAP>::new();
                let _ = body.push_str(r#"{"output_bypass":"restored"}"#);
                render_response_json(&mut frame, request_id.as_str(), body.as_str());
                write_web_serial_line(serial, frame.as_str());
            }
        },
        Ok(UsbCdcFrame::WifiConfig {
            request_id,
            command,
        }) => match command {
            WifiConfigCommand::Set(secret) => {
                let mut frame = heapless::String::<WEB_SERIAL_RESPONSE_FRAME_CAP>::new();
                let ssid = secret.ssid.clone();
                match power.write_web_serial_wifi_config(Some(&secret)) {
                    Ok(()) => {
                        #[cfg(feature = "net_http")]
                        esp_firmware::net::set_usb_wifi_config(Some(secret.clone()));
                        render_wifi_config_ack_json(
                            &mut frame,
                            request_id.as_str(),
                            true,
                            Some(ssid.as_str()),
                        );
                        write_web_serial_line(serial, frame.as_str());
                        log_state.emit(
                            serial,
                            LogLevel::Info,
                            "wifi_config",
                            "WiFi credentials updated in EEPROM",
                        );
                    }
                    Err(_) => {
                        render_error_json(
                            &mut frame,
                            Some(request_id.as_str()),
                            "wifi_config_write_failed",
                            "failed to persist WiFi credentials",
                            true,
                        );
                        write_web_serial_line(serial, frame.as_str());
                    }
                }
            }
            WifiConfigCommand::Clear => {
                let mut frame = heapless::String::<WEB_SERIAL_RESPONSE_FRAME_CAP>::new();
                match power.write_web_serial_wifi_config(None) {
                    Ok(()) => {
                        #[cfg(feature = "net_http")]
                        esp_firmware::net::set_usb_wifi_config(None);
                        render_wifi_config_ack_json(&mut frame, request_id.as_str(), false, None);
                        write_web_serial_line(serial, frame.as_str());
                        log_state.emit(
                            serial,
                            LogLevel::Info,
                            "wifi_config",
                            "WiFi credentials cleared from EEPROM",
                        );
                    }
                    Err(_) => {
                        render_error_json(
                            &mut frame,
                            Some(request_id.as_str()),
                            "wifi_config_clear_failed",
                            "failed to clear WiFi credentials",
                            true,
                        );
                        write_web_serial_line(serial, frame.as_str());
                    }
                }
            }
        },
        Err(err) => {
            let mut frame = heapless::String::<WEB_SERIAL_RESPONSE_FRAME_CAP>::new();
            let request_id = request_id_hint(line);
            render_protocol_error_json(&mut frame, request_id.as_ref().map(|id| id.as_str()), err);
            write_web_serial_line(serial, frame.as_str());
        }
    }
}

#[cfg(feature = "web_serial")]
fn diag_snapshot_response_complete(body: &str) -> bool {
    body.starts_with("{\"packages\":{") && body.ends_with("}}")
}

#[cfg(feature = "web_serial")]
fn write_web_serial_line(serial: &mut UsbSerialJtag<'static, Blocking>, line: &str) {
    let _ = serial.write(line.as_bytes());
    let _ = serial.write(b"\n");
}

#[cfg(feature = "net_http")]
const fn manual_charge_target_api_value(
    target: front_panel_scene::ManualChargeTarget,
) -> &'static str {
    match target {
        front_panel_scene::ManualChargeTarget::Pack3V7 => "pack_3v7",
        front_panel_scene::ManualChargeTarget::Rsoc80 => "rsoc_80",
        front_panel_scene::ManualChargeTarget::Full100 => "full_100",
    }
}

#[cfg(feature = "net_http")]
const fn manual_charge_speed_api_value(
    speed: front_panel_scene::ManualChargeSpeed,
) -> &'static str {
    match speed {
        front_panel_scene::ManualChargeSpeed::Ma100 => "ma_100",
        front_panel_scene::ManualChargeSpeed::Ma500 => "ma_500",
        front_panel_scene::ManualChargeSpeed::Ma1000 => "ma_1000",
    }
}

#[cfg(feature = "net_http")]
const fn manual_charge_power_path_api_value(
    power_path: front_panel_scene::ManualChargePowerPath,
) -> &'static str {
    match power_path {
        front_panel_scene::ManualChargePowerPath::Auto => "auto",
        front_panel_scene::ManualChargePowerPath::DcIn => "dcin",
        front_panel_scene::ManualChargePowerPath::UsbC => "usbc",
    }
}

#[cfg(feature = "net_http")]
fn sync_advanced_power_net_settings<I2C>(power: &output::PowerManager<'_, I2C>)
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let settings = power.advanced_power_settings_snapshot();
    let capabilities = power.advanced_power_capabilities_snapshot();
    esp_firmware::net::set_advanced_power_settings(settings, capabilities);
}

const fn beeper_volume_step(level: front_panel_scene::BeeperVolumeLevel) -> u8 {
    level.step()
}

#[cfg(feature = "web_serial")]
#[derive(Clone, Copy, PartialEq, Eq)]
struct UsbCdcStatusLogSnapshot {
    mode: &'static str,
    active_outputs: &'static str,
    output_gate_reason: &'static str,
    network_state: WifiConnectionState,
    network_error: Option<WifiErrorKind>,
    charger_state: &'static str,
    charger_allow_charge: Option<bool>,
    battery_state: &'static str,
    battery_soc_pct: Option<u16>,
    battery_issue_detail: Option<&'static str>,
}

#[cfg(feature = "web_serial")]
impl UsbCdcStatusLogSnapshot {
    const fn from_status(status: UpsStatusSnapshot) -> Self {
        Self {
            mode: status.mode,
            active_outputs: status.active_outputs,
            output_gate_reason: status.output_gate_reason,
            network_state: status.network.state,
            network_error: status.network.last_error,
            charger_state: status.charger_state,
            charger_allow_charge: status.charger_allow_charge,
            battery_state: status.battery_state,
            battery_soc_pct: status.battery_soc_pct,
            battery_issue_detail: status.battery_issue_detail,
        }
    }
}

#[cfg(feature = "web_serial")]
struct UsbCdcLogState {
    previous: Option<UsbCdcStatusLogSnapshot>,
    last_summary_at: Option<Instant>,
    level: LogLevel,
}

#[cfg(feature = "web_serial")]
impl UsbCdcLogState {
    const fn new() -> Self {
        Self {
            previous: None,
            last_summary_at: None,
            level: LogLevel::Info,
        }
    }

    fn reset(&mut self) {
        self.previous = None;
        self.last_summary_at = None;
        self.level = LogLevel::Info;
    }

    fn set_level(&mut self, level: LogLevel) {
        self.level = level;
    }

    fn level(&self) -> LogLevel {
        self.level
    }

    fn emit_status_logs(
        &mut self,
        serial: &mut UsbSerialJtag<'static, Blocking>,
        status: UpsStatusSnapshot,
    ) {
        let now = Instant::now();
        let current = UsbCdcStatusLogSnapshot::from_status(status);
        let Some(previous) = self.previous else {
            self.emit_summary(serial, status);
            self.emit_output(serial, status);
            self.emit_charger(serial, status);
            self.emit_battery(serial, status);
            self.emit_network(serial, status);
            self.previous = Some(current);
            self.last_summary_at = Some(now);
            return;
        };

        if self
            .last_summary_at
            .map(|last| last.elapsed() >= Duration::from_secs(30))
            .unwrap_or(true)
        {
            self.emit_summary(serial, status);
            self.last_summary_at = Some(now);
        }

        if current.mode != previous.mode {
            self.emit_summary(serial, status);
        }
        if current.active_outputs != previous.active_outputs
            || current.output_gate_reason != previous.output_gate_reason
        {
            self.emit_output(serial, status);
        }
        if current.charger_state != previous.charger_state
            || current.charger_allow_charge != previous.charger_allow_charge
        {
            self.emit_charger(serial, status);
        }
        if current.battery_state != previous.battery_state
            || current.battery_issue_detail != previous.battery_issue_detail
            || current.battery_soc_pct != previous.battery_soc_pct
        {
            self.emit_battery(serial, status);
        }
        if current.network_state != previous.network_state
            || current.network_error != previous.network_error
        {
            self.emit_network(serial, status);
        }
        self.previous = Some(current);
    }

    fn emit_summary(
        &self,
        serial: &mut UsbSerialJtag<'static, Blocking>,
        status: UpsStatusSnapshot,
    ) {
        let mut message = heapless::String::<192>::new();
        let _ = write!(
            message,
            "mode={} active={} gate={} battery_soc=",
            status.mode, status.active_outputs, status.output_gate_reason
        );
        push_opt_u16(&mut message, status.battery_soc_pct);
        let _ = message.push_str(" input_vbus_mv=");
        push_opt_u16(&mut message, status.input_vbus_mv);
        let _ = write!(message, " network={}", status.network.state.as_str());
        self.emit(serial, LogLevel::Info, "status", message.as_str());
    }

    fn emit_output(
        &self,
        serial: &mut UsbSerialJtag<'static, Blocking>,
        status: UpsStatusSnapshot,
    ) {
        let mut message = heapless::String::<192>::new();
        let _ = write!(
            message,
            "active={} gate={} out_a={} enabled=",
            status.active_outputs, status.output_gate_reason, status.out_a_state
        );
        push_opt_bool(&mut message, status.out_a_enabled);
        let _ = write!(message, " out_b={} enabled=", status.out_b_state);
        push_opt_bool(&mut message, status.out_b_enabled);
        let level = if status.output_gate_reason == "none" {
            LogLevel::Info
        } else {
            LogLevel::Warn
        };
        self.emit(serial, level, "output", message.as_str());
    }

    fn emit_charger(
        &self,
        serial: &mut UsbSerialJtag<'static, Blocking>,
        status: UpsStatusSnapshot,
    ) {
        let mut message = heapless::String::<160>::new();
        let _ = write!(message, "state={} allow_charge=", status.charger_state);
        push_opt_bool(&mut message, status.charger_allow_charge);
        let _ = message.push_str(" ichg_ma=");
        push_opt_u16(&mut message, status.charger_ichg_ma);
        let _ = message.push_str(" ibat_ma=");
        push_opt_i16(&mut message, status.charger_ibat_ma);
        self.emit(
            serial,
            comm_state_log_level(status.charger_state),
            "charger",
            message.as_str(),
        );
    }

    fn emit_battery(
        &self,
        serial: &mut UsbSerialJtag<'static, Blocking>,
        status: UpsStatusSnapshot,
    ) {
        let mut message = heapless::String::<192>::new();
        let _ = write!(message, "state={} soc=", status.battery_state);
        push_opt_u16(&mut message, status.battery_soc_pct);
        let _ = message.push_str(" pack_mv=");
        push_opt_u16(&mut message, status.battery_pack_mv);
        if let Some(issue) = status.battery_issue_detail {
            let _ = write!(message, " issue={}", issue);
        } else {
            let _ = message.push_str(" issue=none");
        }
        self.emit(
            serial,
            comm_state_log_level(status.battery_state),
            "battery",
            message.as_str(),
        );
    }

    fn emit_network(
        &self,
        serial: &mut UsbSerialJtag<'static, Blocking>,
        status: UpsStatusSnapshot,
    ) {
        let mut message = heapless::String::<160>::new();
        let _ = write!(message, "state={}", status.network.state.as_str());
        if let Some(ipv4) = status.network.ipv4 {
            let _ = write!(
                message,
                " ipv4={}.{}.{}.{}",
                ipv4[0], ipv4[1], ipv4[2], ipv4[3]
            );
        }
        if let Some(error) = status.network.last_error {
            let _ = write!(message, " error={}", error.as_str());
        }
        let level = if matches!(status.network.state, WifiConnectionState::Error) {
            LogLevel::Warn
        } else {
            LogLevel::Info
        };
        self.emit(serial, level, "network", message.as_str());
    }

    fn emit(
        &self,
        serial: &mut UsbSerialJtag<'static, Blocking>,
        level: LogLevel,
        target: &str,
        message: &str,
    ) {
        if !self.level.allows(level) {
            return;
        }
        let mut frame = heapless::String::<256>::new();
        render_log_json(&mut frame, level, target, message);
        write_web_serial_line(serial, frame.as_str());
    }
}

#[cfg(feature = "web_serial")]
fn web_serial_settings_snapshot<I2C>(
    power: &mut output::PowerManager<'_, I2C>,
    log_level: LogLevel,
) -> esp_firmware::net_types::DeviceSettingsSnapshot
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    let rated_vout_mv = power.advanced_power_capabilities_snapshot().rated_vout_mv;
    let mut settings =
        esp_firmware::net_types::DeviceSettingsSnapshot::defaults_for_rated_vout(rated_vout_mv);
    let wifi = power.read_web_serial_wifi_config().ok().flatten();
    settings.wifi = esp_firmware::net_types::WifiSettingsSnapshot {
        configured: wifi.is_some(),
        ssid: wifi.map(|secret| {
            let mut ssid = HeaplessString::<32>::new();
            let _ = ssid.push_str(secret.ssid.as_str());
            ssid
        }),
    };
    let prefs = power.manual_charge_prefs_snapshot();
    settings.log_level = log_level.as_str();
    settings.manual_charge = esp_firmware::net_types::ManualChargeSettingsSnapshot {
        target: manual_charge_target_api_value(prefs.target),
        speed: manual_charge_speed_api_value(prefs.speed),
        timer_h: prefs.timer_limit.hours(),
        power_path: manual_charge_power_path_api_value(prefs.power_path),
    };
    settings.advanced_power = power.advanced_power_settings_snapshot();
    settings.advanced_power_capabilities = power.advanced_power_capabilities_snapshot();
    settings
}

#[cfg(feature = "web_serial")]
fn comm_state_log_level(state: &str) -> LogLevel {
    match state {
        "err" | "warn" | "not_available" => LogLevel::Warn,
        _ => LogLevel::Info,
    }
}

#[cfg(feature = "web_serial")]
fn push_opt_u16<const N: usize>(out: &mut heapless::String<N>, value: Option<u16>) {
    if let Some(value) = value {
        let _ = write!(out, "{}", value);
    } else {
        let _ = out.push_str("none");
    }
}

#[cfg(feature = "web_serial")]
fn push_opt_i16<const N: usize>(out: &mut heapless::String<N>, value: Option<i16>) {
    if let Some(value) = value {
        let _ = write!(out, "{}", value);
    } else {
        let _ = out.push_str("none");
    }
}

#[cfg(feature = "web_serial")]
fn push_opt_bool<const N: usize>(out: &mut heapless::String<N>, value: Option<bool>) {
    if let Some(value) = value {
        let _ = out.push_str(if value { "true" } else { "false" });
    } else {
        let _ = out.push_str("none");
    }
}

fn sync_runtime_audio(
    audio_manager: &mut AudioManager,
    now: Instant,
    signals: output::AudioSignalSnapshot,
    edges: output::AudioSignalEvents,
) {
    if edges.mains_present_changed == Some(true) {
        audio_manager.trigger(AudioCue::MainsPresentDc);
    }
    if matches!(
        edges.charge_phase_changed,
        Some(output::AudioChargePhase::Charging)
    ) {
        audio_manager.trigger(AudioCue::ChargeStarted);
    }
    if matches!(
        edges.charge_phase_changed,
        Some(output::AudioChargePhase::Completed)
    ) {
        audio_manager.trigger(AudioCue::ChargeCompleted);
    }
    let mains_absent_active = match signals.mains_present {
        Some(false) => {
            edges.mains_present_changed == Some(false)
                || audio_manager.is_cue_active(AudioCue::MainsAbsentDc)
        }
        None => audio_manager.is_cue_active(AudioCue::MainsAbsentDc),
        Some(true) => false,
    };

    audio_manager.set_cue_active(AudioCue::MainsAbsentDc, mains_absent_active, now);
    audio_manager.set_cue_active(AudioCue::HighStress, signals.thermal_stress, now);
    audio_manager.set_cue_active(
        AudioCue::BatteryLowNoMains,
        signals.battery_low == output::AudioBatteryLowState::NoMains,
        now,
    );
    audio_manager.set_cue_active(
        AudioCue::BatteryLowWithMains,
        signals.battery_low == output::AudioBatteryLowState::WithMains,
        now,
    );
    audio_manager.set_cue_active(
        AudioCue::ShutdownProtection,
        signals.shutdown_protection,
        now,
    );
    audio_manager.set_cue_active(AudioCue::IoOverVoltage, signals.io_over_voltage, now);
    audio_manager.set_cue_active(AudioCue::IoOverCurrent, signals.io_over_current, now);
    audio_manager.set_cue_active(AudioCue::ModuleFault, signals.module_fault, now);
    audio_manager.set_cue_active(AudioCue::BatteryProtection, signals.battery_protection, now);
}

fn front_panel_attention_hold(
    signals: output::AudioSignalSnapshot,
    self_check_blocked: bool,
) -> bool {
    self_check_blocked
        || signals.thermal_stress
        || matches!(
            signals.battery_low,
            output::AudioBatteryLowState::NoMains | output::AudioBatteryLowState::WithMains
        )
        || signals.battery_protection
        || signals.module_fault
        || signals.io_over_voltage
        || signals.io_over_current
        || signals.shutdown_protection
}
