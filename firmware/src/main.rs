#![no_std]
#![no_main]

esp_bootloader_esp_idf::esp_app_desc!();

mod audio_demo;
mod irq;
mod output;

use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::gpio::{DriveMode, Event, Flex, Input, InputConfig, Io, OutputConfig, Pull};
use esp_hal::i2c::master::{Config as I2cConfig, I2c, SoftwareTimeout};
use esp_hal::ledc::channel::{self, ChannelIFace};
use esp_hal::ledc::timer::{self, TimerIFace};
use esp_hal::ledc::{LSGlobalClkSource, Ledc, LowSpeed};
use esp_hal::time::{Duration, Instant, Rate};
use esp_hal::timer::{systimer::SystemTimer, timg::TimerGroup};
use esp_hal::{main, Blocking};
use esp_println as _;

// Bring-up default profile.
// Bring-up safety: disable OUT-B by default until we explicitly validate the power stage.
const DEFAULT_ENABLED_OUTPUTS: output::EnabledOutputs =
    output::EnabledOutputs::Only(output::OutputChannel::OutA);
const DEFAULT_VOUT_MV: u16 = 19_000;
const DEFAULT_ILIMIT_MA: u16 = 3_500;
const TELEMETRY_PERIOD: Duration = Duration::from_millis(500);
const RETRY_BACKOFF: Duration = Duration::from_secs(5);
const FAULT_LOG_MIN_INTERVAL: Duration = Duration::from_millis(200);
const TELEMETRY_INCLUDE_VIN_CH3: bool = false;
const FORCE_MIN_CHARGE: bool = cfg!(feature = "force-min-charge");

const FW_BUILD_PROFILE: &str = env!("FW_BUILD_PROFILE");
const FW_GIT_SHA: &str = env!("FW_GIT_SHA");

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
const TMP112_THIGH_C_X16: i16 = 50 * 16;
const TMP112_TLOW_C_X16: i16 = 40 * 16;

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

    let i2c1_config = I2cConfig::default()
        // SMBus-gauge bring-up is more stable at 100kHz on the shared bus.
        .with_frequency(Rate::from_khz(100))
        .with_software_timeout(SoftwareTimeout::Transaction(Duration::from_millis(100)));
    let mut i2c: I2c<'static, Blocking> = I2c::new(peripherals.I2C1, i2c1_config)
        .unwrap()
        .with_sda(peripherals.GPIO48)
        .with_scl(peripherals.GPIO47);

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
    let i2c2_config = I2cConfig::default()
        .with_frequency(Rate::from_khz(400))
        .with_software_timeout(SoftwareTimeout::Transaction(Duration::from_millis(100)));
    let mut i2c2: I2c<'static, Blocking> = I2c::new(peripherals.I2C0, i2c2_config)
        .unwrap()
        .with_sda(peripherals.GPIO8)
        .with_scl(peripherals.GPIO9);
    let panel_probe = output::log_i2c2_presence(&mut i2c2);
    defmt::info!(
        "self_test: panel screen_present={=bool} typec_present={=bool}",
        panel_probe.screen_present(),
        panel_probe.fusb302_present
    );

    let self_test = output::boot_self_test(
        &mut i2c,
        DEFAULT_ENABLED_OUTPUTS,
        DEFAULT_VOUT_MV,
        DEFAULT_ILIMIT_MA,
        TELEMETRY_INCLUDE_VIN_CH3,
        tmp_out_a_ok,
        tmp_out_b_ok,
        tps_sync_ok,
        panel_probe.screen_present(),
        low_after,
        FORCE_MIN_CHARGE,
    );

    let cfg = output::Config {
        enabled_outputs: self_test.enabled_outputs,
        vout_mv: DEFAULT_VOUT_MV,
        ilimit_ma: DEFAULT_ILIMIT_MA,
        telemetry_period: TELEMETRY_PERIOD,
        retry_backoff: RETRY_BACKOFF,
        fault_log_min_interval: FAULT_LOG_MIN_INTERVAL,
        telemetry_include_vin_ch3: TELEMETRY_INCLUDE_VIN_CH3,
        tmp112_tlow_c_x16: TMP112_TLOW_C_X16,
        tmp112_thigh_c_x16: TMP112_THIGH_C_X16,
        charger_enabled: self_test.charger_enabled,
        force_min_charge: FORCE_MIN_CHARGE,
        bms_addr: self_test.bms_addr,
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
        "power: enabled_outputs={} target_vout_mv={=u16} target_ilimit_ma={=u16}",
        cfg.enabled_outputs.describe(),
        cfg.vout_mv,
        cfg.ilimit_ma
    );
    power.init_best_effort();

    let mut irq_tracker = irq::IrqTracker::new();
    let mut last_irq_log_at: Option<Instant> = None;

    match audio_demo::play_demo_playlist(
        i2s0,
        dma_channel,
        audio_bclk,
        audio_ws,
        audio_dout,
        || {
            let irq_events = irq_tracker.take_delta();
            power.tick(&irq_events);

            if irq_events.any()
                && output::tps55288::should_log_fault(
                    Instant::now(),
                    &mut last_irq_log_at,
                    Duration::from_millis(200),
                )
            {
                defmt::info!(
                    "irq: i2c1_int={=u32} i2c2_int={=u32} chg_int={=u32} ina_pv={=u32} ina_warning={=u32} ina_critical={=u32} bms_btp_int_h={=u32} therm_kill_n={=u32}",
                    irq_events.i2c1_int,
                    irq_events.i2c2_int,
                    irq_events.chg_int,
                    irq_events.ina_pv,
                    irq_events.ina_warning,
                    irq_events.ina_critical,
                    irq_events.bms_btp_int_h,
                    irq_events.therm_kill_n
                );
            }
        },
    ) {
        Ok(()) => {}
        Err(err) => defmt::error!("audio: demo playlist error: {=?}", err),
    }

    loop {
        defmt::info!("esp: heartbeat");
        let irq_events = irq_tracker.take_delta();
        power.tick(&irq_events);
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(2_000) {}
    }
}
