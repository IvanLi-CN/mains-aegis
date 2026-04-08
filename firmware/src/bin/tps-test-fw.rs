#![no_std]
#![no_main]

esp_bootloader_esp_idf::esp_app_desc!();

use core::cell::RefCell;

#[path = "../front_panel.rs"]
mod front_panel;
#[path = "../front_panel_logic.rs"]
mod front_panel_logic;
#[path = "../front_panel_scene.rs"]
mod front_panel_scene;
#[path = "../irq.rs"]
mod irq;
#[path = "../tps55288_test.rs"]
mod tps55288_test;
#[path = "../tps_test_runtime.rs"]
mod tps_test_runtime;

use embedded_hal_bus::i2c::RefCellDevice;
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::gpio::{DriveMode, Event, Flex, Input, InputConfig, Io, OutputConfig, Pull};
use esp_hal::i2c::master::{Config as I2cConfig, I2c, SoftwareTimeout};
use esp_hal::ledc::channel::{self, ChannelIFace};
use esp_hal::ledc::timer::{self, TimerIFace};
use esp_hal::ledc::{LSGlobalClkSource, Ledc, LowSpeed};
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::spi::Mode as SpiMode;
use esp_hal::time::{Duration, Instant, Rate};
use esp_hal::timer::{systimer::SystemTimer, timg::TimerGroup};
use esp_hal::{main, Blocking};
use esp_println as _;

const POLL_INTERVAL: Duration = Duration::from_millis(250);
const I2C1_FREQ_KHZ: u32 = 25;
const I2C1_BUS_CLEAR_PULSES: u8 = 18;
const I2C1_BUS_CLEAR_HALF_PERIOD: Duration = Duration::from_micros(20);
const I2C1_BUS_TIMEOUT_LOW: Duration = Duration::from_millis(40);

const FW_BUILD_PROFILE: &str = env!("FW_BUILD_PROFILE");
const FW_GIT_SHA: &str = env!("FW_GIT_SHA");
const FW_BUILD_ID: &str = env!("FW_BUILD_ID");

const TPS_SYNC_ENABLE: bool = true;
const TPS_SYNC_FREQ_KHZ: u32 = 465;
const TPS_SYNC_DUTY_PCT: u8 = 50;
const TPS_SYNC_PHASE_TICKS: u16 = 64;

fn spin_delay(wait: Duration) {
    let start = Instant::now();
    while start.elapsed() < wait {}
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

    defmt::info!(
        "tps-test: i2c_bus_clear bus={} pulses={=u8} sda_before={=bool} scl_before={=bool} sda_after={=bool} scl_after={=bool}",
        bus,
        I2C1_BUS_CLEAR_PULSES,
        sda_high_before,
        scl_high_before,
        sda.is_high(),
        scl.is_high()
    );
}

#[main]
fn main() -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::_160MHz);
    let peripherals = esp_hal::init(config);

    let mut io = Io::new(peripherals.IO_MUX);
    io.set_interrupt_handler(irq::gpio_isr);

    let mut _tps_sync_ledc = Ledc::new(peripherals.LEDC);
    _tps_sync_ledc.set_global_slow_clock(LSGlobalClkSource::APBClk);
    let mut _tps_sync_timer0 = _tps_sync_ledc.timer::<LowSpeed>(timer::Number::Timer0);
    let mut _tps_sync_a = _tps_sync_ledc.channel(channel::Number::Channel0, peripherals.GPIO41);
    let mut _tps_sync_b = _tps_sync_ledc.channel(channel::Number::Channel1, peripherals.GPIO42);

    let mut tps_sync_ok = false;
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
                        let ledc_regs = esp_hal::peripherals::LEDC::regs();
                        ledc_regs
                            .ch(1)
                            .hpoint()
                            .write(|w| unsafe { w.hpoint().bits(TPS_SYNC_PHASE_TICKS) });
                        tps_sync_ok = true;
                    }
                    (a, b) => {
                        defmt::error!("tps-test: sync channel err ch0={=?} ch1={=?}", a, b);
                    }
                }
            }
            Err(err) => {
                defmt::error!("tps-test: sync timer err={=?}", err);
            }
        }
    }

    let _systimer = SystemTimer::new(peripherals.SYSTIMER);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let mut wdt0 = timg0.wdt;
    wdt0.disable();

    let timg1 = TimerGroup::new(peripherals.TIMG1);
    let mut wdt1 = timg1.wdt;
    wdt1.disable();

    esp_println::println!("tps-test: boot");
    let profile = tps_test_runtime::TEST_PROFILE;
    defmt::info!(
        "tps-test: boot build_profile={} build_id={} git_sha={} sync_ok={=bool}",
        FW_BUILD_PROFILE,
        FW_BUILD_ID,
        FW_GIT_SHA,
        tps_sync_ok
    );
    defmt::info!(
        "tps-test: fixed_profile outputs={} mode={} charge={} charger={=bool} out_a={=bool} out_b={=bool} vout={} ilimit_ma={=u16} charge_vreg_mv={=u16} ichg_ma={=u16} iindpm_ma={=u16}",
        profile.output_selection.label(),
        profile.switching_mode.label(),
        profile.charge_profile.label(),
        profile.charger_enable,
        profile.out_a_oe,
        profile.out_b_oe,
        profile.vout_profile.label(),
        profile.ilimit_ma,
        profile.charge_vreg_mv,
        profile.charge_ichg_ma,
        profile.input_limit_ma
    );

    let mut i2c1_sda = Flex::new(peripherals.GPIO48);
    let mut i2c1_scl = Flex::new(peripherals.GPIO47);
    clear_i2c_bus(&mut i2c1_sda, &mut i2c1_scl, "i2c1");

    let i2c1_config = I2cConfig::default()
        .with_frequency(Rate::from_khz(I2C1_FREQ_KHZ))
        .with_software_timeout(SoftwareTimeout::Transaction(Duration::from_millis(100)));
    let i2c: I2c<'static, Blocking> = I2c::new(peripherals.I2C1, i2c1_config)
        .unwrap()
        .with_sda(i2c1_sda.into_peripheral_output())
        .with_scl(i2c1_scl.into_peripheral_output());

    let i2c1_int_cfg = InputConfig::default().with_pull(Pull::Up);
    let mut i2c1_int = Input::new(peripherals.GPIO33, i2c1_int_cfg);
    i2c1_int.clear_interrupt();
    i2c1_int.listen(Event::FallingEdge);

    let mut chg_ce = Flex::new(peripherals.GPIO15);
    chg_ce.apply_output_config(
        &OutputConfig::default()
            .with_drive_mode(DriveMode::OpenDrain)
            .with_pull(Pull::Up),
    );
    chg_ce.set_high();
    chg_ce.set_output_enable(true);

    let mut chg_ilim_hiz_brk = Flex::new(peripherals.GPIO16);
    chg_ilim_hiz_brk.apply_output_config(
        &OutputConfig::default()
            .with_drive_mode(DriveMode::PushPull)
            .with_pull(Pull::None),
    );
    chg_ilim_hiz_brk.set_low();
    chg_ilim_hiz_brk.set_output_enable(true);

    let mut therm_kill = Flex::new(peripherals.GPIO40);
    therm_kill.apply_input_config(&InputConfig::default().with_pull(Pull::Up));
    therm_kill.set_input_enable(true);
    therm_kill.apply_output_config(
        &OutputConfig::default()
            .with_drive_mode(DriveMode::OpenDrain)
            .with_pull(Pull::Up),
    );
    therm_kill.set_high();
    therm_kill.set_output_enable(true);

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
    let bl = Flex::new(peripherals.GPIO13);
    let btn_center = Input::new(
        peripherals.GPIO0,
        InputConfig::default().with_pull(Pull::None),
    );
    let ctp_irq = Input::new(
        peripherals.GPIO14,
        InputConfig::default().with_pull(Pull::None),
    );

    let i2c2_bus = RefCell::new(i2c2);

    let mut panel = front_panel::FrontPanel::new(
        RefCellDevice::new(&i2c2_bus),
        spi,
        peripherals.DMA_CH1,
        peripherals.PSRAM,
        btn_center,
        ctp_irq,
        tca_reset_n,
        dc,
        bl,
    );
    panel.init_best_effort();

    let mut runtime = tps_test_runtime::TpsTestRuntime::new(
        FW_BUILD_PROFILE,
        FW_BUILD_ID,
        i2c,
        chg_ce,
        chg_ilim_hiz_brk,
        therm_kill,
    );

    let profile = tps_test_runtime::TEST_PROFILE;
    let mut last_snapshot = Some(front_panel_scene::TpsTestUiSnapshot::pending(
        FW_BUILD_PROFILE,
        FW_BUILD_ID,
        profile.vout_profile,
        profile.ilimit_ma,
        profile.out_a_oe,
        profile.out_b_oe,
    ));
    if panel.is_ready() {
        if let Some(snapshot) = last_snapshot {
            panel.render_tps_test_status(snapshot);
        }
    } else {
        defmt::error!("tps-test: front panel init failed; continue with logs only");
    }

    let mut next_poll = Instant::now();
    let mut irq_tracker = irq::IrqTracker::new();
    loop {
        let now = Instant::now();
        if now < next_poll {
            continue;
        }
        next_poll += POLL_INTERVAL;

        let irq_events = irq_tracker.take_delta();
        let snapshot = runtime.tick(now, &irq_events, i2c1_int.is_low());
        if panel.is_ready() && last_snapshot != Some(snapshot) {
            panel.render_tps_test_status(snapshot);
            last_snapshot = Some(snapshot);
        }
    }
}
