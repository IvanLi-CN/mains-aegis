#![no_std]
#![no_main]
#![allow(dead_code)]

esp_bootloader_esp_idf::esp_app_desc!();

#[path = "../front_panel.rs"]
mod front_panel;
#[path = "../front_panel_scene.rs"]
mod front_panel_scene;
#[path = "../irq.rs"]
mod irq;

use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::gpio::{Input, InputConfig, Io, Pull};
use esp_hal::i2c::master::{Config as I2cConfig, I2c, SoftwareTimeout};
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::spi::Mode as SpiMode;
use esp_hal::time::{Duration, Instant, Rate};
use esp_hal::timer::{systimer::SystemTimer, timg::TimerGroup};
use esp_hal::{main, Blocking};
use esp_println as _;

const FRAME_INTERVAL: Duration = Duration::from_millis(500);

#[main]
fn main() -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::_160MHz);
    let peripherals = esp_hal::init(config);

    let mut io = Io::new(peripherals.IO_MUX);
    io.set_interrupt_handler(irq::gpio_isr);

    let _systimer = SystemTimer::new(peripherals.SYSTIMER);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let mut wdt0 = timg0.wdt;
    wdt0.disable();

    let timg1 = TimerGroup::new(peripherals.TIMG1);
    let mut wdt1 = timg1.wdt;
    wdt1.disable();

    esp_println::println!("diag: front-panel display probe boot");
    defmt::info!("diag: front-panel display probe boot");

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

    let tca_reset_n = esp_hal::gpio::Flex::new(peripherals.GPIO1);
    let dc = esp_hal::gpio::Flex::new(peripherals.GPIO10);
    let bl = esp_hal::gpio::Flex::new(peripherals.GPIO13);
    let btn_center = Input::new(
        peripherals.GPIO0,
        InputConfig::default().with_pull(Pull::None),
    );
    let ctp_irq = Input::new(
        peripherals.GPIO14,
        InputConfig::default().with_pull(Pull::None),
    );

    let mut panel =
        front_panel::FrontPanel::new(i2c2, spi, btn_center, ctp_irq, tca_reset_n, dc, bl);
    panel.init_best_effort();

    if !panel.is_ready() {
        defmt::error!("diag: panel init failed; keeping heartbeat for logs");
        loop {
            defmt::warn!("diag: panel unavailable");
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(2) {}
        }
    }

    defmt::info!("diag: rendering display diagnostic screen");
    let mut heartbeat_on = false;
    let mut next_frame = Instant::now();

    loop {
        let now = Instant::now();
        if now >= next_frame {
            heartbeat_on = !heartbeat_on;
            panel.render_display_diagnostic(heartbeat_on);
            next_frame += FRAME_INTERVAL;
        }
    }
}
