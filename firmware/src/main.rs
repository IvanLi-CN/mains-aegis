#![no_std]
#![no_main]

esp_bootloader_esp_idf::esp_app_desc!();

mod front_panel;
mod front_panel_scene;
mod irq;
mod output;

use esp_backtrace as _;
use esp_firmware::audio::{AudioCue, AudioManager, PLAYBACK_SAMPLE_RATE_HZ};
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
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::spi::Mode as SpiMode;
use esp_hal::time::{Duration, Instant, Rate};
use esp_hal::timer::{systimer::SystemTimer, timg::TimerGroup};
use esp_hal::{main, Blocking};
use esp_println as _;

// Bring-up default profile.
const DEFAULT_ENABLED_OUTPUTS: output::EnabledOutputs = output::EnabledOutputs::Both;
const DEFAULT_VOUT_MV: u16 = if cfg!(feature = "main-vout-19v") {
    19_000
} else {
    12_000
};
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

// External SYNC for TPS55288 DITH/SYNC pins (SYNCA=0°, SYNCB=180°).
// RFSW on board is 43kΩ (U17/U18 pin 8), so nominal fSW ≈ 20MHz / 43kΩ ≈ 465kHz.
// External clock must be within ±30% of the configured fSW.
// Debug: disable external SYNC to check if INA3221 shunt readings are polluted by coupling.
const TPS_SYNC_ENABLE: bool = true;
const TPS_SYNC_FREQ_KHZ: u32 = 465;
const TPS_SYNC_DUTY_PCT: u8 = 50;
const TPS_SYNC_PHASE_TICKS: u16 = 64; // 180° at Duty7Bit => 128 ticks/period.

// Do not assert THERM_KILL_N during normal bring-up.
const FORCE_THERM_KILL_N_ASSERTED: bool = false;

// TMP112A alert settings (Plan v5hze).
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
// Temporary hardware assumption until the exact fan tach characteristics are confirmed.
const FAN_TACH_PULSES_PER_REV: u8 = 2;

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

#[main]
fn main() -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::_160MHz);
    let peripherals = esp_hal::init(config);

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

    // Ensure the system timer is enabled before calling `Instant::now()`.
    let _systimer = SystemTimer::new(peripherals.SYSTIMER);

    // Disable watchdog timers for the simplest possible bring-up loop.
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let mut wdt0 = timg0.wdt;
    wdt0.disable();

    let timg1 = TimerGroup::new(peripherals.TIMG1);
    let mut wdt1 = timg1.wdt;
    wdt1.disable();

    // Human-readable marker (plain serial) to help bring-up when defmt decoding isn't available yet.
    esp_println::println!("esp: boot (serial)");
    defmt::info!("esp: boot");
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

    let i2c1_int_cfg = InputConfig::default().with_pull(Pull::Up);
    let mut i2c1_int = Input::new(peripherals.GPIO33, i2c1_int_cfg);
    i2c1_int.clear_interrupt();
    i2c1_int.listen(Event::FallingEdge);

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

    // Front panel: I2C2 + SPI display bring-up (Plan #3kz8p).
    // Keep these variables alive for the whole program.
    let i2c2_config = I2cConfig::default()
        .with_frequency(Rate::from_khz(400))
        .with_software_timeout(SoftwareTimeout::Transaction(Duration::from_millis(100)));
    let mut i2c2: I2c<'static, Blocking> = I2c::new(peripherals.I2C0, i2c2_config)
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
    let bl = Flex::new(peripherals.GPIO13);
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
    let panel_probe = output::log_i2c2_presence(&mut i2c2);
    defmt::info!(
        "self_test: panel screen_present={=bool} typec_present={=bool}",
        panel_probe.screen_present(),
        panel_probe.fusb302_present
    );

    let mut front_panel = front_panel::FrontPanel::new(
        i2c2,
        spi,
        peripherals.DMA_CH1,
        peripherals.PSRAM,
        btn_center,
        ctp_irq,
        tca_reset_n,
        dc,
        bl,
    );
    if !panel_probe.screen_present() {
        defmt::warn!(
            "ui: panel_io probe is missing; attempting display init anyway in case the initial scan was transient"
        );
    }
    front_panel.init_best_effort();
    front_panel.update_self_check_snapshot(front_panel_scene::SelfCheckUiSnapshot::pending(
        front_panel_scene::UpsMode::Standby,
    ));

    let (_, _, tx_buffer, tx_descriptors) =
        esp_hal::dma_circular_buffers!(0, AUDIO_DMA_BUFFER_BYTES);
    let mut audio_manager = AudioManager::new();
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
                None
            }
        },
        None => None,
    };
    let mut audio_enabled = audio_transfer.is_some();

    macro_rules! restart_runtime_audio_dma {
        () => {{
            audio_transfer = None;
            tx_buffer.fill(0);
            let mut disable_audio = false;
            if let Some(i2s_tx) = i2s_tx.as_mut() {
                match i2s_tx.write_dma_circular(&tx_buffer) {
                    Ok(mut transfer) => {
                        match transfer.available() {
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
                                    defmt::warn!(
                                        "audio: dma push failed during runtime flush; disabling runtime audio"
                                    );
                                    disable_audio = true;
                                } else {
                                    audio_transfer = Some(transfer);
                                }
                            }
                            Ok(_) => {
                                audio_transfer = Some(transfer);
                            }
                            Err(err) => {
                                defmt::warn!(
                                    "audio: dma available failed during runtime flush err={=?}; disabling runtime audio",
                                    err
                                );
                                disable_audio = true;
                            }
                        }
                    }
                    Err(err) => {
                        defmt::warn!(
                            "audio: dma restart failed err={=?}; disabling runtime audio",
                            err
                        );
                        disable_audio = true;
                    }
                }
            } else {
                disable_audio = true;
            }
            if disable_audio {
                audio_enabled = false;
                audio_transfer = None;
                audio_manager.stop();
            }
        }};
    }

    if audio_enabled {
        audio_manager.trigger(AudioCue::BootStartup);
        let mut disable_audio = false;
        if let Some(audio_transfer) = audio_transfer.as_mut() {
            match audio_transfer.available() {
                Ok(available) if available >= 4 => {
                    let budget = audio_refill_budget(available, AUDIO_BOOT_WATERMARK_BYTES);
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
                        disable_audio = true;
                    }
                }
                Ok(_) => {}
                Err(err) => {
                    defmt::warn!(
                        "audio: dma available failed during boot prefill err={=?}; disabling runtime audio",
                        err
                    );
                    disable_audio = true;
                }
            }
        } else {
            disable_audio = true;
        }
        if disable_audio {
            audio_enabled = false;
            audio_transfer = None;
            audio_manager.stop();
        }
    }

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
                                disable_audio = true;
                            }
                        }
                        Ok(_) => {}
                        Err(err) => {
                            defmt::warn!(
                                "audio: dma available failed during self-test err={=?}; disabling runtime audio",
                                err
                            );
                            disable_audio = true;
                        }
                    }
                } else {
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

    let cfg = output::Config {
        ina_detected: self_test.ina_detected,
        detected_tmp_outputs: self_test.detected_tmp_outputs,
        detected_tps_outputs: self_test.detected_tps_outputs,
        requested_outputs: self_test.requested_outputs,
        active_outputs: self_test.active_outputs,
        recoverable_outputs: self_test.recoverable_outputs,
        output_gate_reason: self_test.output_gate_reason,
        vout_mv: DEFAULT_VOUT_MV,
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
        therm_kill,
        chg_ce,
        chg_ilim_hiz_brk,
        cfg,
    );
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
    let initial_snapshot = power.ui_snapshot();
    front_panel.update_self_check_snapshot(initial_snapshot);
    front_panel.update_bms_activation_state(power.bms_activation_state());
    if front_panel_scene::self_check_can_enter_dashboard(&initial_snapshot) {
        front_panel.enter_dashboard();
    } else {
        defmt::warn!("ui: stay on self-check reason=boot_self_check_not_clear");
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

    let mut irq_tracker = irq::IrqTracker::new();
    let mut last_irq_log_at: Option<Instant> = None;
    let mut last_fan_tach_log_at: Option<Instant> = None;

    loop {
        defmt::info!("esp: heartbeat");
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(2_000) {
            let irq_events = irq_tracker.take_delta();
            let fan_telemetry_due = power.tick(&irq_events);
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
            let now = Instant::now();
            if audio_enabled {
                let audio_edges = power.take_audio_edges();
                let flush_runtime_audio = audio_edges.battery_low_changed.is_some()
                    || audio_edges.module_fault_changed.is_some()
                    || audio_edges.battery_protection_changed.is_some();
                sync_runtime_audio(&mut audio_manager, now, power.audio_signals(), audio_edges);
                audio_manager.tick(now);
                if flush_runtime_audio {
                    audio_manager.arm_transition_bridge();
                    restart_runtime_audio_dma!();
                    continue;
                }
                let mut disable_audio = false;
                if let Some(audio_transfer) = audio_transfer.as_mut() {
                    match audio_transfer.available() {
                        Ok(available) if available >= 4 => {
                            let budget =
                                audio_refill_budget(available, AUDIO_RUNTIME_WATERMARK_BYTES);
                            if budget >= 4
                                && audio_transfer
                                    .push_with(|buf| {
                                        let len = budget.min(buf.len()) & !0x3;
                                        audio_manager.fill(&mut buf[..len])
                                    })
                                    .is_err()
                            {
                                defmt::warn!("audio: dma push failed; disabling runtime audio");
                                disable_audio = true;
                            }
                        }
                        Ok(_) => {}
                        Err(DmaError::Late) => {
                            defmt::warn!(
                                "audio: dma available failed err=Late; recover runtime audio on next refill"
                            );
                        }
                        Err(err) => {
                            defmt::warn!(
                                "audio: dma available failed err={=?}; disabling runtime audio",
                                err
                            );
                            disable_audio = true;
                        }
                    }
                } else {
                    disable_audio = true;
                }
                if disable_audio {
                    audio_enabled = false;
                    audio_transfer = None;
                    audio_manager.stop();
                }
            }
            let ui_snapshot = power.ui_snapshot();
            front_panel.update_self_check_snapshot(ui_snapshot);
            front_panel.update_bms_activation_state(power.bms_activation_state());
            if let Some(action) = front_panel.tick() {
                match action {
                    front_panel::UiAction::RequestBmsRecovery(action) => {
                        power.request_bms_recovery_action(action);
                        front_panel.update_bms_activation_state(power.bms_activation_state());
                    }
                    front_panel::UiAction::ClearBmsActivationResult => {
                        power.clear_bms_activation_state();
                        front_panel.update_bms_activation_state(power.bms_activation_state());
                    }
                }
            }
            if front_panel_scene::self_check_can_enter_dashboard(&ui_snapshot) {
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
            }
            if audio_enabled {
                let now = Instant::now();
                let audio_edges = power.take_audio_edges();
                let flush_runtime_audio = audio_edges.battery_low_changed.is_some()
                    || audio_edges.module_fault_changed.is_some()
                    || audio_edges.battery_protection_changed.is_some();
                sync_runtime_audio(&mut audio_manager, now, power.audio_signals(), audio_edges);
                audio_manager.tick(now);
                if flush_runtime_audio {
                    audio_manager.arm_transition_bridge();
                    restart_runtime_audio_dma!();
                    continue;
                }
                let mut disable_audio = false;
                if let Some(audio_transfer) = audio_transfer.as_mut() {
                    match audio_transfer.available() {
                        Ok(available) if available >= 4 => {
                            let budget =
                                audio_refill_budget(available, AUDIO_RUNTIME_WATERMARK_BYTES);
                            if budget >= 4
                                && audio_transfer
                                    .push_with(|buf| {
                                        let len = budget.min(buf.len()) & !0x3;
                                        audio_manager.fill(&mut buf[..len])
                                    })
                                    .is_err()
                            {
                                defmt::warn!("audio: dma push failed; disabling runtime audio");
                                disable_audio = true;
                            }
                        }
                        Ok(_) => {}
                        Err(DmaError::Late) => {
                            defmt::warn!(
                                "audio: dma available failed err=Late; recover runtime audio on next refill"
                            );
                        }
                        Err(err) => {
                            defmt::warn!(
                                "audio: dma available failed err={=?}; disabling runtime audio",
                                err
                            );
                            disable_audio = true;
                        }
                    }
                } else {
                    disable_audio = true;
                }
                if disable_audio {
                    audio_enabled = false;
                    audio_transfer = None;
                    audio_manager.stop();
                }
            }
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
        }
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
