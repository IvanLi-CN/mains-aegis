#![no_std]
#![no_main]
#![allow(dead_code)]

esp_bootloader_esp_idf::esp_app_desc!();

use core::cell::RefCell;

#[cfg(all(
    feature = "test-fw",
    not(any(feature = "test-fw-screen-static", feature = "test-fw-audio-playback"))
))]
compile_error!(
    "test-fw requires at least one feature: test-fw-screen-static or test-fw-audio-playback"
);
#[cfg(all(
    feature = "test-fw-default-screen-static",
    feature = "test-fw-default-audio-playback"
))]
compile_error!("test-fw default features conflict: choose only one default test");
#[cfg(all(
    feature = "test-fw-default-screen-static",
    not(feature = "test-fw-screen-static")
))]
compile_error!("test-fw-default-screen-static requires test-fw-screen-static");
#[cfg(all(
    feature = "test-fw-default-audio-playback",
    not(feature = "test-fw-audio-playback")
))]
compile_error!("test-fw-default-audio-playback requires test-fw-audio-playback");

#[path = "../front_panel.rs"]
mod front_panel;
#[path = "../front_panel_logic.rs"]
mod front_panel_logic;
#[path = "../front_panel_scene.rs"]
mod front_panel_scene;
#[path = "../irq.rs"]
mod irq;
#[path = "../test_audio.rs"]
mod test_audio;
#[path = "../test_harness.rs"]
mod test_harness;

use embedded_hal_bus::i2c::RefCellDevice;
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::gpio::{Input, InputConfig, Io, Pull};
use esp_hal::i2c::master::{Config as I2cConfig, I2c, SoftwareTimeout};
#[cfg(feature = "test-fw-audio-playback")]
use esp_hal::i2s::master::{Channels, Config as I2sConfig, DataFormat, I2s};
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::spi::Mode as SpiMode;
use esp_hal::time::{Duration, Instant, Rate};
use esp_hal::timer::{systimer::SystemTimer, timg::TimerGroup};
use esp_hal::{main, Blocking};
use esp_println as _;

#[cfg(feature = "test-fw-audio-playback")]
use test_audio::AudioCue;
use test_harness::{HarnessInputEvent, TestHarnessState, TestRoute};

const POLL_INTERVAL: Duration = Duration::from_millis(20);
const LOG_INTERVAL: Duration = Duration::from_secs(2);

#[main]
fn main() -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::_160MHz);
    let peripherals = esp_hal::init(config);

    let mut io = Io::new(peripherals.IO_MUX);
    io.set_interrupt_handler(irq::gpio_isr);

    #[cfg(feature = "test-fw-audio-playback")]
    let i2s0 = peripherals.I2S0;
    #[cfg(feature = "test-fw-audio-playback")]
    let dma_channel = peripherals.DMA_CH0;
    #[cfg(feature = "test-fw-audio-playback")]
    let audio_bclk = peripherals.GPIO4;
    #[cfg(feature = "test-fw-audio-playback")]
    let audio_ws = peripherals.GPIO5;
    #[cfg(feature = "test-fw-audio-playback")]
    let audio_dout = peripherals.GPIO6;

    let _systimer = SystemTimer::new(peripherals.SYSTIMER);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let mut wdt0 = timg0.wdt;
    wdt0.disable();

    let timg1 = TimerGroup::new(peripherals.TIMG1);
    let mut wdt1 = timg1.wdt;
    wdt1.disable();

    esp_println::println!("test-fw: boot");
    defmt::info!("test-fw: boot");

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

    if !panel.is_ready() {
        defmt::error!("test-fw: panel init failed; keep alive for logs");
        loop {
            defmt::warn!("test-fw: panel unavailable");
            let start = Instant::now();
            while start.elapsed() < LOG_INTERVAL {}
        }
    }

    let cfg = test_harness::config_from_features();
    let mut harness = TestHarnessState::new(cfg);

    #[cfg(feature = "test-fw-audio-playback")]
    let mut audio_manager = test_audio::AudioManager::new();

    #[cfg(feature = "test-fw-audio-playback")]
    {
        audio_manager.trigger(AudioCue::BootStartup);
    }

    #[cfg(feature = "test-fw-audio-playback")]
    let i2s = I2s::new(
        i2s0,
        dma_channel,
        I2sConfig::new_tdm_philips()
            .with_sample_rate(Rate::from_hz(test_audio::PLAYBACK_SAMPLE_RATE_HZ))
            .with_data_format(DataFormat::Data16Channel16)
            .with_channels(Channels::STEREO),
    )
    .unwrap();

    #[cfg(feature = "test-fw-audio-playback")]
    let (_, _, tx_buffer, tx_descriptors) = esp_hal::dma_circular_buffers!(0, 16 * 4092);
    #[cfg(feature = "test-fw-audio-playback")]
    let mut i2s_tx = i2s
        .i2s_tx
        .with_bclk(audio_bclk)
        .with_ws(audio_ws)
        .with_dout(audio_dout)
        .build(tx_descriptors);
    #[cfg(feature = "test-fw-audio-playback")]
    let mut audio_transfer = i2s_tx.write_dma_circular(&tx_buffer).unwrap();

    render_route(
        &mut panel,
        &harness,
        #[cfg(feature = "test-fw-audio-playback")]
        audio_manager.status(),
    );

    let mut next_poll = Instant::now();
    let mut last_log = Instant::now();

    #[cfg(feature = "test-fw-audio-playback")]
    let mut last_audio_status = audio_manager.status();

    loop {
        #[cfg(feature = "test-fw-audio-playback")]
        {
            match audio_transfer.available() {
                Ok(available) if available >= 4 => {
                    if audio_transfer
                        .push_with(|buf| audio_manager.fill(buf))
                        .is_err()
                    {
                        defmt::warn!("test-fw: dma push failed");
                    }
                }
                Ok(_) => {}
                Err(_) => defmt::warn!("test-fw: dma available failed"),
            }
            audio_manager.tick(Instant::now());
        }

        let now = Instant::now();
        if now < next_poll {
            continue;
        }
        next_poll += POLL_INTERVAL;

        let mut needs_redraw = false;
        if let Some(input) = panel.poll_test_input_event() {
            if let Some(mapped) = map_input(input) {
                let result = harness.handle_input(mapped);
                if result.stop_audio {
                    #[cfg(feature = "test-fw-audio-playback")]
                    {
                        audio_manager.stop();
                        loop {
                            let Ok(available) = audio_transfer.available() else {
                                break;
                            };
                            if available == 0 {
                                break;
                            }
                            if audio_transfer
                                .push_with(|buf| {
                                    for b in buf.iter_mut() {
                                        *b = 0;
                                    }
                                    buf.len()
                                })
                                .is_err()
                            {
                                defmt::warn!("test-fw: dma silence flush failed");
                                break;
                            }
                        }
                    }
                }
                #[cfg(feature = "test-fw-audio-playback")]
                if let Some(audio_cue) = result.audio_cue {
                    audio_manager.trigger(audio_cue);
                }
                if result.needs_redraw {
                    needs_redraw = true;
                }
            }
        }

        #[cfg(feature = "test-fw-audio-playback")]
        {
            let status = audio_manager.status();
            if status != last_audio_status {
                last_audio_status = status;
                if harness.route() == TestRoute::AudioPlayback {
                    needs_redraw = true;
                }
            }
        }

        if needs_redraw {
            render_route(
                &mut panel,
                &harness,
                #[cfg(feature = "test-fw-audio-playback")]
                audio_manager.status(),
            );
        }

        if now - last_log >= LOG_INTERVAL {
            last_log = now;
            defmt::info!(
                "test-fw: route={} back_enabled={=bool}",
                route_name(harness.route()),
                harness.back_enabled()
            );
            #[cfg(feature = "test-fw-audio-playback")]
            {
                let s = audio_manager.status();
                defmt::info!(
                    "test-fw: audio playing={=bool} current={} queued={=u8} dropped={=u32} preempted={=u32}",
                    s.playing,
                    audio_event_name(s.current),
                    s.queued,
                    s.dropped,
                    s.preempted
                );
            }
        }
    }
}

fn render_route<I2C>(
    panel: &mut front_panel::FrontPanel<I2C>,
    harness: &TestHarnessState,
    #[cfg(feature = "test-fw-audio-playback")] audio_status: test_audio::AudioStatus,
) where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    match harness.route() {
        TestRoute::Navigation => {
            panel.render_test_navigation(
                harness.selected_function_ui(),
                harness.default_function_ui(),
            );
        }
        TestRoute::ScreenStatic => {
            panel.render_test_screen_static(harness.back_enabled());
        }
        TestRoute::AudioPlayback => {
            panel.render_test_audio_playback(
                harness.back_enabled(),
                #[cfg(feature = "test-fw-audio-playback")]
                map_audio_ui_state(harness, audio_status),
                #[cfg(not(feature = "test-fw-audio-playback"))]
                front_panel_scene::AudioTestUiState {
                    playing: false,
                    queued: 0,
                    current: None,
                    selected_idx: 0,
                    list_top: 0,
                },
            );
        }
    }
}

fn map_input(input: front_panel::TestInputEvent) -> Option<HarnessInputEvent> {
    Some(match input {
        front_panel::TestInputEvent::Up => HarnessInputEvent::Up,
        front_panel::TestInputEvent::Down => HarnessInputEvent::Down,
        front_panel::TestInputEvent::Left => HarnessInputEvent::Left,
        front_panel::TestInputEvent::Right => HarnessInputEvent::Right,
        front_panel::TestInputEvent::Center => HarnessInputEvent::Center,
        front_panel::TestInputEvent::Touch { x, y } => HarnessInputEvent::Touch { x, y },
        front_panel::TestInputEvent::TouchDrag { x, y, dy } => {
            HarnessInputEvent::TouchDrag { x, y, dy }
        }
        front_panel::TestInputEvent::TouchRelease { x, y } => {
            HarnessInputEvent::TouchRelease { x, y }
        }
    })
}

#[cfg(feature = "test-fw-audio-playback")]
fn map_audio_ui_state(
    harness: &TestHarnessState,
    status: test_audio::AudioStatus,
) -> front_panel_scene::AudioTestUiState {
    front_panel_scene::AudioTestUiState {
        playing: status.playing,
        queued: status.queued,
        current: status.current.map(|cue| match cue {
            test_audio::AudioCue::BootStartup => front_panel_scene::AudioEventUi::BootStartup,
            test_audio::AudioCue::MainsPresentDc => front_panel_scene::AudioEventUi::MainsPresentDc,
            test_audio::AudioCue::ChargeStarted => front_panel_scene::AudioEventUi::ChargeStarted,
            test_audio::AudioCue::ChargeCompleted => {
                front_panel_scene::AudioEventUi::ChargeCompleted
            }
            test_audio::AudioCue::ShutdownModeEntered => {
                front_panel_scene::AudioEventUi::ShutdownModeEntered
            }
            test_audio::AudioCue::MainsAbsentDc => front_panel_scene::AudioEventUi::MainsAbsentDc,
            test_audio::AudioCue::HighStress => front_panel_scene::AudioEventUi::HighStress,
            test_audio::AudioCue::BatteryLowNoMains => {
                front_panel_scene::AudioEventUi::BatteryLowNoMains
            }
            test_audio::AudioCue::BatteryLowWithMains => {
                front_panel_scene::AudioEventUi::BatteryLowWithMains
            }
            test_audio::AudioCue::ShutdownProtection => {
                front_panel_scene::AudioEventUi::ShutdownProtection
            }
            test_audio::AudioCue::IoOverVoltage => front_panel_scene::AudioEventUi::IoOverVoltage,
            test_audio::AudioCue::IoOverCurrent => front_panel_scene::AudioEventUi::IoOverCurrent,
            test_audio::AudioCue::IoOverPower => front_panel_scene::AudioEventUi::IoOverPower,
            test_audio::AudioCue::ModuleFault => front_panel_scene::AudioEventUi::ModuleFault,
            test_audio::AudioCue::BatteryProtection => {
                front_panel_scene::AudioEventUi::BatteryProtection
            }
        }),
        selected_idx: harness.audio_selected_index() as u8,
        list_top: harness.audio_list_top() as u8,
    }
}

fn route_name(route: TestRoute) -> &'static str {
    match route {
        TestRoute::Navigation => "navigation",
        TestRoute::ScreenStatic => "screen-static",
        TestRoute::AudioPlayback => "audio-playback",
    }
}

#[cfg(feature = "test-fw-audio-playback")]
fn audio_event_name(event: Option<test_audio::AudioCue>) -> &'static str {
    match event {
        Some(test_audio::AudioCue::BootStartup) => "boot_startup",
        Some(test_audio::AudioCue::MainsPresentDc) => "mains_present_dc",
        Some(test_audio::AudioCue::ChargeStarted) => "charge_started",
        Some(test_audio::AudioCue::ChargeCompleted) => "charge_completed",
        Some(test_audio::AudioCue::ShutdownModeEntered) => "shutdown_mode_entered",
        Some(test_audio::AudioCue::MainsAbsentDc) => "mains_absent_dc",
        Some(test_audio::AudioCue::HighStress) => "high_stress",
        Some(test_audio::AudioCue::BatteryLowNoMains) => "battery_low_no_mains",
        Some(test_audio::AudioCue::BatteryLowWithMains) => "battery_low_with_mains",
        Some(test_audio::AudioCue::ShutdownProtection) => "shutdown_protection",
        Some(test_audio::AudioCue::IoOverVoltage) => "io_over_voltage",
        Some(test_audio::AudioCue::IoOverCurrent) => "io_over_current",
        Some(test_audio::AudioCue::IoOverPower) => "io_over_power",
        Some(test_audio::AudioCue::ModuleFault) => "module_fault",
        Some(test_audio::AudioCue::BatteryProtection) => "battery_protection",
        None => "none",
    }
}
