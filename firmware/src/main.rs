#![no_std]
#![no_main]

esp_bootloader_esp_idf::esp_app_desc!();

mod audio_demo;
mod output;

use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::gpio::{DriveMode, Flex, Input, InputConfig, OutputConfig, Pull};
use esp_hal::i2c::master::{Config as I2cConfig, I2c, SoftwareTimeout};
use esp_hal::ledc::channel::{self, ChannelIFace};
use esp_hal::ledc::timer::{self, TimerIFace};
use esp_hal::ledc::{LSGlobalClkSource, Ledc, LowSpeed};
use esp_hal::time::{Duration, Instant, Rate};
use esp_hal::timer::{systimer::SystemTimer, timg::TimerGroup};
use esp_hal::{main, Blocking};
use esp_println as _;

// Bring-up default profile.
const DEFAULT_ENABLED_OUTPUTS: output::EnabledOutputs = output::EnabledOutputs::Both;
const DEFAULT_VOUT_MV: u16 = 19_000;
const DEFAULT_ILIMIT_MA: u16 = 3_500;
const TELEMETRY_PERIOD: Duration = Duration::from_millis(500);
const RETRY_BACKOFF: Duration = Duration::from_secs(5);
const FAULT_LOG_MIN_INTERVAL: Duration = Duration::from_millis(200);
const TELEMETRY_INCLUDE_VIN_CH3: bool = false;

const FW_BUILD_PROFILE: &str = env!("FW_BUILD_PROFILE");
const FW_GIT_SHA: &str = env!("FW_GIT_SHA");

// External SYNC for TPS55288 DITH/SYNC pins (SYNCA=0°, SYNCB=180°).
// RFSW on board is 43kΩ (U17/U18 pin 8), so nominal fSW ≈ 20MHz / 43kΩ ≈ 465kHz.
// External clock must be within ±30% of the configured fSW.
// Debug: disable external SYNC to check if INA3221 shunt readings are polluted by coupling.
const TPS_SYNC_ENABLE: bool = false;
const TPS_SYNC_FREQ_KHZ: u32 = 465;
const TPS_SYNC_DUTY_PCT: u8 = 50;
const TPS_SYNC_PHASE_TICKS: u16 = 64; // 180° at Duty7Bit => 128 ticks/period.

// Do not assert THERM_KILL_N during normal bring-up.
const FORCE_THERM_KILL_N_ASSERTED: bool = false;

#[main]
fn main() -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::_160MHz);
    let peripherals = esp_hal::init(config);

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
                    (a, b) => defmt::error!("power: tps_sync err ch0={=?} ch1={=?}", a, b),
                }
            }
            Err(e) => defmt::error!("power: tps_sync timer err={=?}", e),
        }
    } else {
        defmt::info!("power: tps_sync disabled (pins reserved)");
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

    let i2c_config = I2cConfig::default()
        .with_frequency(Rate::from_khz(400))
        .with_software_timeout(SoftwareTimeout::Transaction(Duration::from_millis(100)));
    let i2c: I2c<'static, Blocking> = I2c::new(peripherals.I2C1, i2c_config)
        .unwrap()
        .with_sda(peripherals.GPIO48)
        .with_scl(peripherals.GPIO47);

    let i2c1_int_cfg = InputConfig::default().with_pull(Pull::Up);
    let i2c1_int = Input::new(peripherals.GPIO33, i2c1_int_cfg);

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
    let _therm_kill = therm_kill;

    let cfg = output::Config {
        enabled_outputs: DEFAULT_ENABLED_OUTPUTS,
        vout_mv: DEFAULT_VOUT_MV,
        ilimit_ma: DEFAULT_ILIMIT_MA,
        telemetry_period: TELEMETRY_PERIOD,
        retry_backoff: RETRY_BACKOFF,
        fault_log_min_interval: FAULT_LOG_MIN_INTERVAL,
        telemetry_include_vin_ch3: TELEMETRY_INCLUDE_VIN_CH3,
    };

    let mut power = output::PowerManager::new(i2c, i2c1_int, cfg);
    defmt::info!(
        "power: enabled_outputs={} target_vout_mv={=u16} target_ilimit_ma={=u16}",
        cfg.enabled_outputs.describe(),
        cfg.vout_mv,
        cfg.ilimit_ma
    );
    power.init_best_effort();

    match audio_demo::play_demo_playlist(
        i2s0,
        dma_channel,
        audio_bclk,
        audio_ws,
        audio_dout,
        || {
            power.tick();
        },
    ) {
        Ok(()) => {}
        Err(err) => defmt::error!("audio: demo playlist error: {=?}", err),
    }

    loop {
        defmt::info!("esp: heartbeat");
        power.tick();
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(2_000) {}
    }
}
