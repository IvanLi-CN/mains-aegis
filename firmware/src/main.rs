#![no_std]
#![no_main]

esp_bootloader_esp_idf::esp_app_desc!();

mod audio_demo;

use esp_backtrace as _;
use esp_hal::time::{Duration, Instant};
use esp_hal::timer::{systimer::SystemTimer, timg::TimerGroup};
use esp_hal::{clock::CpuClock, main};
use esp_println as _;

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

    match audio_demo::play_demo_playlist(i2s0, dma_channel, audio_bclk, audio_ws, audio_dout) {
        Ok(()) => {}
        Err(err) => defmt::error!("audio: demo playlist error: {=?}", err),
    }

    loop {
        defmt::info!("esp: heartbeat");
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(2_000) {}
    }
}
