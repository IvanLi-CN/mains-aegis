use core::convert::Infallible;

use crate::front_panel_scene::{
    self, AudioTestUiState, BmsActivationState, BmsResultKind, SelfCheckCommState,
    SelfCheckOverlay, SelfCheckTouchTarget, SelfCheckUiSnapshot, TestFunctionUi, UiFocus, UiModel,
    UiPainter, UiVariant, UpsMode,
};
use embedded_hal::digital::OutputPin;
use embedded_hal::spi::{Operation, SpiBus, SpiDevice};
use esp_firmware::display_pipeline::{
    DirtyRows, DisplayBufferError, DisplayBuffers, DMA_STAGING_BYTES, FRAME_HEIGHT, FRAME_WIDTH,
};
use esp_hal::dma::{DmaChannelFor, DmaRxBuf, DmaTxBuf};
use esp_hal::gpio::{DriveMode, Flex, Input, OutputConfig, Pull};
use esp_hal::i2c::master::I2c as HalI2c;
use esp_hal::peripherals::PSRAM;
use esp_hal::psram;
use esp_hal::spi::{
    master::{AnySpi, Spi as HalSpi, SpiDmaBus},
    Mode,
};
use esp_hal::time::{Duration, Instant, Rate};
use esp_hal::Blocking;
use gc9307_async::{Config as GcConfig, Orientation, Timer as GcTimer, GC9307C};

// Front panel: GC9307 over SPI + slow control lines via TCA6408A (I2C2).
// This module uses gc9307-async (crates.io) for controller init.

const TCA6408A_ADDR: u8 = 0x21;

// TCA6408A registers.
const TCA_REG_INPUT: u8 = 0x00;
const TCA_REG_OUTPUT: u8 = 0x01;
const TCA_REG_POLARITY: u8 = 0x02;
const TCA_REG_CONFIG: u8 = 0x03;

const CST816D_ADDR: u8 = 0x15;
const CST816D_REG_GESTURE: u8 = 0x01;
const CST816D_TOUCH_REG_LEN: usize = 6;

// TCA bit assignments.
const TCA_BIT_CS: u8 = 5; // P5, active-low
const TCA_BIT_RES: u8 = 6; // P6, active-low
const TCA_BIT_TP_RESET: u8 = 7; // P7, active-low

// DCS commands used for post-init test pattern writes.
const CMD_CASET: u8 = 0x2A;
const CMD_RASET: u8 = 0x2B;
const CMD_RAMWR: u8 = 0x2C;

// Diagnostic step: rotate 180° via driver orientation API (LandscapeSwapped).
const LCD_W: u16 = 320;
const LCD_H: u16 = 172;
const OFFSET_X: u16 = 0;
const OFFSET_Y: u16 = 34;
const PANEL_ORIENTATION: Orientation = Orientation::LandscapeSwapped;
const PANEL_RGB_ORDER: bool = false;
const UI_ORIENTATION_MARKER: &str = "FP_ORI_PROBE_20260227";

const BACKLIGHT_ACTIVE_LOW: bool = true;

const FRAME_INTERVAL: Duration = Duration::from_millis(50);
const BOOT_SPLASH_HOLD: Duration = Duration::from_millis(900);
const SELF_CHECK_VARIANT: UiVariant = UiVariant::RetroC;
const PANEL_INIT_SPI_FREQ_MHZ: u32 = 10;
const PANEL_RUNTIME_SPI_FREQ_MHZ: u32 = if cfg!(feature = "display-spi-20mhz") {
    20
} else {
    40
};
const DASHBOARD_VARIANT: UiVariant = UiVariant::InstrumentB;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiAction {
    RequestBmsActivation,
    ClearBmsActivationResult,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestInputEvent {
    Up,
    Down,
    Left,
    Right,
    Center,
    Touch { x: u16, y: u16 },
    TouchDrag { x: u16, y: u16, dy: i16 },
    TouchRelease { x: u16, y: u16 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitState {
    Disabled,
    Ready,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct InputSnapshot {
    up: bool,
    down: bool,
    left: bool,
    right: bool,
    center: bool,
    touch: bool,
    touch_point: Option<(u16, u16)>,
}

impl InputSnapshot {
    const fn idle() -> Self {
        Self {
            up: false,
            down: false,
            left: false,
            right: false,
            center: false,
            touch: false,
            touch_point: None,
        }
    }
}

pub struct FrontPanel {
    i2c: HalI2c<'static, Blocking>,
    panel_io: PanelIo,
    btn_center: Input<'static>,
    ctp_irq: Input<'static>,
    tca_reset_n: Flex<'static>,
    bl: Flex<'static>,
    display_buffers: DisplayBuffers,
    dirty_rows: DirtyRows,

    tca_output: u8,

    state: InitState,
    next_frame_deadline: Instant,
    last_inputs: Option<InputSnapshot>,
    last_test_touch_point: Option<(u16, u16)>,
    needs_redraw: bool,
    ui_variant: UiVariant,
    self_check_snapshot: SelfCheckUiSnapshot,
    bms_activation_state: BmsActivationState,
    self_check_overlay: SelfCheckOverlay,
    touch_irq_stuck_hint_logged: bool,
    frame_no: u32,
}

impl FrontPanel {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        i2c: HalI2c<'static, Blocking>,
        spi: HalSpi<'static, Blocking>,
        dma_channel: impl DmaChannelFor<AnySpi<'static>>,
        psram: PSRAM<'static>,
        btn_center: Input<'static>,
        ctp_irq: Input<'static>,
        tca_reset_n: Flex<'static>,
        dc: Flex<'static>,
        bl: Flex<'static>,
    ) -> Self {
        let display_buffers = unsafe {
            let (psram_ptr, psram_bytes) = psram::psram_raw_parts(&psram);
            DisplayBuffers::from_psram_raw_parts(psram_ptr, psram_bytes).unwrap_or_else(|err| {
                match err {
                    DisplayBufferError::MisalignedPsram => {
                        panic!("display PSRAM alignment is invalid")
                    }
                    DisplayBufferError::InsufficientPsram => {
                        panic!("display PSRAM capacity is insufficient")
                    }
                }
            })
        };

        let (rx_buffer, rx_descriptors, tx_buffer, tx_descriptors) =
            esp_hal::dma_buffers!(4, DMA_STAGING_BYTES);
        let dma_rx_buf =
            DmaRxBuf::new(rx_descriptors, rx_buffer).expect("display dma rx buffer init failed");
        let dma_tx_buf =
            DmaTxBuf::new(tx_descriptors, tx_buffer).expect("display dma tx buffer init failed");
        let panel_io = PanelIo {
            spi: spi
                .with_dma(dma_channel)
                .with_buffers(dma_rx_buf, dma_tx_buf),
            dc,
        };

        Self {
            i2c,
            panel_io,
            btn_center,
            ctp_irq,
            tca_reset_n,
            bl,
            display_buffers,
            dirty_rows: DirtyRows::new(),
            tca_output: 0,
            state: InitState::Disabled,
            next_frame_deadline: Instant::now(),
            last_inputs: None,
            last_test_touch_point: None,
            needs_redraw: false,
            ui_variant: SELF_CHECK_VARIANT,
            self_check_snapshot: SelfCheckUiSnapshot::pending(front_panel_scene::UpsMode::Standby),
            bms_activation_state: BmsActivationState::Idle,
            self_check_overlay: SelfCheckOverlay::None,
            touch_irq_stuck_hint_logged: false,
            frame_no: 0,
        }
    }

    pub fn init_best_effort(&mut self) {
        self.panel_io.configure_dc();
        self.configure_backlight();
        self.configure_tca_reset();

        // Reset expander and force known screen-safe defaults.
        self.pulse_tca_reset(Duration::from_millis(10));

        if let Err(e) = self.tca_init() {
            defmt::error!("ui: tca6408a init failed err={}", i2c_error_kind(e));
            self.set_backlight(true);
            self.state = InitState::Disabled;
            return;
        }

        if let Err(e) = self.tca_set_res_released(false) {
            defmt::error!("ui: tca set res failed err={}", i2c_error_kind(e));
            self.set_backlight(true);
            self.state = InitState::Disabled;
            return;
        }
        if let Err(e) = self.tca_set_tp_reset_released(false) {
            defmt::error!("ui: tca set tp_reset failed err={}", i2c_error_kind(e));
            self.set_backlight(true);
            self.state = InitState::Disabled;
            return;
        }
        if let Err(e) = self.tca_set_cs_enabled(false) {
            defmt::error!("ui: tca set cs failed err={}", i2c_error_kind(e));
            self.set_backlight(true);
            self.state = InitState::Disabled;
            return;
        }

        busy_wait(Duration::from_millis(10));

        // Hardware reset through expander lines before handing over to driver init.
        if let Err(e) = self.tca_set_res_released(true) {
            defmt::error!("ui: tca release res failed err={}", i2c_error_kind(e));
            self.set_backlight(true);
            self.state = InitState::Disabled;
            return;
        }
        if let Err(e) = self.tca_set_tp_reset_released(true) {
            defmt::error!("ui: tca release tp_reset failed err={}", i2c_error_kind(e));
            self.set_backlight(true);
            self.state = InitState::Disabled;
            return;
        }
        busy_wait(Duration::from_millis(120));

        if let Err(e) = self.tca_set_cs_enabled(true) {
            defmt::error!("ui: tca enable cs failed err={}", i2c_error_kind(e));
            self.set_backlight(true);
            self.state = InitState::Disabled;
            return;
        }
        busy_wait(Duration::from_millis(5));

        if self.gc9307_driver_init().is_err() {
            defmt::error!("ui: gc9307 driver init failed");
            let _ = self.tca_set_cs_enabled(false);
            let _ = self.tca_set_res_released(false);
            self.set_backlight(true);
            self.state = InitState::Disabled;
            return;
        }

        self.set_backlight(true);
        self.state = InitState::Ready;
        self.next_frame_deadline = Instant::now();

        self.render_boot_confirmation_splash();
        busy_wait(BOOT_SPLASH_HOLD);

        let snapshot = match self.read_inputs() {
            Ok(snapshot) => {
                self.last_inputs = Some(snapshot);
                snapshot
            }
            Err(e) => {
                defmt::error!("ui: read input state failed err={}", i2c_error_kind(e));
                let idle = InputSnapshot::idle();
                self.last_inputs = Some(idle);
                idle
            }
        };

        if let Err(e) = self.render_inputs(snapshot) {
            defmt::error!("ui: render input state failed err={=?}", e);
        }

        defmt::info!(
            "ui: front panel ready (driver=gc9307-async mode=industrial-demo variant={} res={}x{} offset=({},{}))",
            variant_name(self.ui_variant),
            LCD_W,
            LCD_H,
            OFFSET_X,
            OFFSET_Y
        );
        defmt::info!(
            "ui: marker={} orientation_madctl=0x{=u8:02x}",
            UI_ORIENTATION_MARKER,
            PANEL_ORIENTATION as u8
        );
        esp_println::println!(
            "ui: marker={} orientation_madctl=0x{:02x}",
            UI_ORIENTATION_MARKER,
            PANEL_ORIENTATION as u8
        );
        esp_println::println!("ui: boot splash -> self-check");
    }

    pub fn update_self_check_snapshot(&mut self, snapshot: SelfCheckUiSnapshot) {
        let previous = self.self_check_snapshot;
        if previous == snapshot {
            return;
        }
        log_self_check_snapshot_transition(&previous, &snapshot);
        self.self_check_snapshot = snapshot;
        if self.self_check_overlay == SelfCheckOverlay::BmsActivateConfirm
            && !front_panel_scene::is_bq40_activation_needed(&self.self_check_snapshot)
        {
            defmt::info!(
                "ui: bms activation dialog auto_close reason=activation_not_needed bq40_state={} last_result={}",
                self_check_comm_state_name(self.self_check_snapshot.bq40z50),
                bms_result_option_name(self.self_check_snapshot.bq40z50_last_result)
            );
            self.self_check_overlay = SelfCheckOverlay::None;
        }
        self.needs_redraw = true;
        if self.state != InitState::Ready {
            return;
        }
        let current_inputs = self.last_inputs.unwrap_or_else(InputSnapshot::idle);
        if let Err(e) = self.render_inputs(current_inputs) {
            defmt::error!("ui: render self-check snapshot failed err={=?}", e);
        } else {
            self.last_inputs = Some(current_inputs);
            self.needs_redraw = false;
        }
    }

    pub fn update_bms_activation_state(&mut self, state: BmsActivationState) {
        let previous = self.bms_activation_state;
        if previous == state {
            return;
        }
        self.bms_activation_state = state;
        self.self_check_overlay = if self.ui_variant == SELF_CHECK_VARIANT {
            match state {
                BmsActivationState::Idle => SelfCheckOverlay::None,
                BmsActivationState::Pending => SelfCheckOverlay::BmsActivateProgress,
                BmsActivationState::Result(result) => SelfCheckOverlay::BmsActivateResult(result),
            }
        } else {
            SelfCheckOverlay::None
        };
        defmt::info!(
            "ui: bms activation state old={} new={} overlay={}",
            bms_activation_state_name(previous),
            bms_activation_state_name(state),
            overlay_name(self.self_check_overlay)
        );
        self.needs_redraw = self.ui_variant == SELF_CHECK_VARIANT;
    }

    pub fn enter_dashboard(&mut self) {
        if self.ui_variant == DASHBOARD_VARIANT {
            return;
        }

        let previous_variant = self.ui_variant;
        self.ui_variant = DASHBOARD_VARIANT;
        self.self_check_overlay = SelfCheckOverlay::None;
        self.needs_redraw = true;
        defmt::info!(
            "ui: page switch old={} new={}",
            variant_name(previous_variant),
            variant_name(self.ui_variant)
        );
        esp_println::println!(
            "ui: page switch old={} new={}",
            variant_name(previous_variant),
            variant_name(self.ui_variant)
        );

        if self.state != InitState::Ready {
            return;
        }

        let current_inputs = self.last_inputs.unwrap_or_else(InputSnapshot::idle);
        if let Err(e) = self.render_inputs(current_inputs) {
            defmt::error!("ui: render dashboard failed err={=?}", e);
            self.needs_redraw = true;
        } else {
            self.last_inputs = Some(current_inputs);
            self.needs_redraw = false;
        }
    }

    pub fn tick(&mut self) -> Option<UiAction> {
        if self.state != InitState::Ready {
            return None;
        }

        let now = Instant::now();
        if now < self.next_frame_deadline {
            return None;
        }
        self.next_frame_deadline += FRAME_INTERVAL;

        let mut ui_action = None;
        match self.read_inputs() {
            Ok(snapshot) => {
                if self.ui_variant == SELF_CHECK_VARIANT {
                    ui_action = self.process_bms_activation_button_action(snapshot);
                    if ui_action.is_none() {
                        ui_action = self.process_touch_action(snapshot);
                    }
                }
                let inputs_changed = self.last_inputs != Some(snapshot);
                let should_render =
                    self.needs_redraw || (self.ui_variant == SELF_CHECK_VARIANT && inputs_changed);
                if should_render {
                    if let Err(e) = self.render_inputs(snapshot) {
                        defmt::error!("ui: update input state failed err={=?}", e);
                        self.needs_redraw = true;
                    } else {
                        self.last_inputs = Some(snapshot);
                        self.needs_redraw = false;
                    }
                }
            }
            Err(e) => {
                defmt::error!("ui: poll input state failed err={}", i2c_error_kind(e));
            }
        }

        ui_action
    }

    fn process_bms_activation_button_action(
        &mut self,
        snapshot: InputSnapshot,
    ) -> Option<UiAction> {
        let prev = self.last_inputs.unwrap_or_else(InputSnapshot::idle);
        let left_edge = snapshot.left && !prev.left;
        let right_edge = snapshot.right && !prev.right;
        let center_edge = snapshot.center && !prev.center;

        if matches!(
            self.self_check_overlay,
            SelfCheckOverlay::BmsActivateResult(..)
        ) {
            if left_edge || right_edge || center_edge {
                self.self_check_overlay = SelfCheckOverlay::None;
                self.needs_redraw = true;
                defmt::info!("ui: bms result dialog close via key");
                return Some(UiAction::ClearBmsActivationResult);
            }
            return None;
        }

        match self.self_check_overlay {
            SelfCheckOverlay::None => {
                if (left_edge || center_edge)
                    && self.bms_activation_state != BmsActivationState::Pending
                {
                    if let Some(result_overlay) =
                        front_panel_scene::bq40_result_overlay(&self.self_check_snapshot)
                    {
                        self.self_check_overlay = result_overlay;
                        self.needs_redraw = true;
                        defmt::info!("ui: bms result dialog reopen via key");
                    } else if front_panel_scene::is_bq40_activation_needed(
                        &self.self_check_snapshot,
                    ) {
                        self.self_check_overlay = SelfCheckOverlay::BmsActivateConfirm;
                        self.needs_redraw = true;
                        defmt::info!("ui: bms activation dialog open via key");
                    }
                }
            }
            SelfCheckOverlay::BmsActivateConfirm => {
                if right_edge {
                    self.self_check_overlay = SelfCheckOverlay::None;
                    self.needs_redraw = true;
                    defmt::info!("ui: bms activation dialog cancel via key");
                } else if left_edge || center_edge {
                    self.self_check_overlay = SelfCheckOverlay::BmsActivateProgress;
                    self.needs_redraw = true;
                    defmt::info!("ui: bms activation request via key");
                    return Some(UiAction::RequestBmsActivation);
                }
            }
            SelfCheckOverlay::BmsActivateProgress => {}
            SelfCheckOverlay::BmsActivateResult(..) => {}
        }

        None
    }

    #[allow(dead_code)]
    pub fn is_ready(&self) -> bool {
        self.state == InitState::Ready
    }

    #[allow(dead_code)]
    pub fn render_display_diagnostic(&mut self, heartbeat_on: bool) {
        if self.state != InitState::Ready {
            return;
        }
        let meta = front_panel_scene::DisplayDiagnosticMeta {
            orientation_label: orientation_label(PANEL_ORIENTATION),
            color_order_label: if PANEL_RGB_ORDER {
                "COLOR ORDER: RGB565"
            } else {
                "COLOR ORDER: BGR565"
            },
            heartbeat_on,
        };
        if let Err(e) = self
            .render_scene(|painter| front_panel_scene::render_display_diagnostic(painter, &meta))
        {
            defmt::error!("ui: render display diag failed err={=?}", e);
        }
    }

    fn render_boot_confirmation_splash(&mut self) {
        let meta = front_panel_scene::DisplayDiagnosticMeta {
            orientation_label: "BOOT CHECK 320x172",
            color_order_label: "BACKLIGHT + SPI + TCA",
            heartbeat_on: true,
        };
        if let Err(e) = self
            .render_scene(|painter| front_panel_scene::render_display_diagnostic(painter, &meta))
        {
            defmt::error!("ui: render boot splash failed err={=?}", e);
        } else {
            esp_println::println!("ui: boot splash rendered");
        }
    }

    #[allow(dead_code)]
    pub fn render_test_navigation(
        &mut self,
        selected: TestFunctionUi,
        default_test: Option<TestFunctionUi>,
    ) {
        if self.state != InitState::Ready {
            return;
        }
        if let Err(e) = self.render_scene(|painter| {
            front_panel_scene::render_test_navigation(painter, selected, default_test)
        }) {
            defmt::error!("ui: render test navigation failed err={=?}", e);
        }
    }

    #[allow(dead_code)]
    pub fn render_test_screen_static(&mut self, back_enabled: bool) {
        if self.state != InitState::Ready {
            return;
        }
        let color_order_label = if PANEL_RGB_ORDER {
            "COLOR ORDER: RGB565"
        } else {
            "COLOR ORDER: BGR565"
        };
        if let Err(e) = self.render_scene(|painter| {
            front_panel_scene::render_test_screen_static(painter, back_enabled, color_order_label)
        }) {
            defmt::error!("ui: render screen static test failed err={=?}", e);
        }
    }

    #[allow(dead_code)]
    pub fn render_test_audio_playback(&mut self, back_enabled: bool, state: AudioTestUiState) {
        if self.state != InitState::Ready {
            return;
        }
        if let Err(e) = self.render_scene(|painter| {
            front_panel_scene::render_test_audio_playback(painter, back_enabled, state)
        }) {
            defmt::error!("ui: render audio playback test failed err={=?}", e);
        }
    }

    #[allow(dead_code)]
    pub fn poll_test_input_event(&mut self) -> Option<TestInputEvent> {
        if self.state != InitState::Ready {
            return None;
        }

        let snapshot = match self.read_inputs() {
            Ok(v) => v,
            Err(e) => {
                defmt::error!("ui: test input read failed err={}", i2c_error_kind(e));
                return None;
            }
        };
        let prev = self.last_inputs.unwrap_or_else(InputSnapshot::idle);
        let mut event = None;
        let mut next_snapshot = snapshot;

        if snapshot.center && !prev.center {
            event = Some(TestInputEvent::Center);
        } else if snapshot.up && !prev.up {
            event = Some(TestInputEvent::Up);
        } else if snapshot.down && !prev.down {
            event = Some(TestInputEvent::Down);
        } else if snapshot.left && !prev.left {
            event = Some(TestInputEvent::Left);
        } else if snapshot.right && !prev.right {
            event = Some(TestInputEvent::Right);
        } else if let Some((x, y)) = snapshot.touch_point {
            let mut emitted = false;
            if let Some((_, prev_y)) = self.last_test_touch_point {
                let dy = y as i16 - prev_y as i16;
                if dy != 0 {
                    event = Some(TestInputEvent::TouchDrag { x, y, dy });
                    emitted = true;
                }
            }
            if !emitted && snapshot.touch && !prev.touch {
                event = Some(TestInputEvent::Touch { x, y });
            }
            self.last_test_touch_point = Some((x, y));
        } else {
            if !snapshot.touch && prev.touch {
                if let Some((x, y)) = self.last_test_touch_point.or(prev.touch_point) {
                    event = Some(TestInputEvent::TouchRelease { x, y });
                }
            } else if snapshot.touch && !prev.touch {
                // Keep touch edge pending until we have a usable coordinate sample.
                next_snapshot.touch = false;
                next_snapshot.touch_point = None;
            }
            self.last_test_touch_point = None;
        }

        self.last_inputs = Some(next_snapshot);
        event
    }

    fn configure_backlight(&mut self) {
        self.bl.apply_output_config(
            &OutputConfig::default()
                .with_drive_mode(DriveMode::PushPull)
                .with_pull(Pull::None),
        );
        self.set_backlight(false);
        self.bl.set_output_enable(true);
    }

    fn configure_tca_reset(&mut self) {
        self.tca_reset_n.apply_output_config(
            &OutputConfig::default()
                .with_drive_mode(DriveMode::OpenDrain)
                .with_pull(Pull::Up),
        );
        self.tca_reset_n.set_high();
        self.tca_reset_n.set_output_enable(true);
    }

    fn pulse_tca_reset(&mut self, hold: Duration) {
        self.tca_reset_n.set_low();
        busy_wait(hold);
        self.tca_reset_n.set_high();
        busy_wait(Duration::from_millis(2));
    }

    fn set_backlight(&mut self, on: bool) {
        if BACKLIGHT_ACTIVE_LOW {
            if on {
                self.bl.set_low();
            } else {
                self.bl.set_high();
            }
        } else if on {
            self.bl.set_high();
        } else {
            self.bl.set_low();
        }
    }

    fn tca_init(&mut self) -> Result<(), esp_hal::i2c::master::Error> {
        self.i2c.write(TCA6408A_ADDR, &[TCA_REG_POLARITY, 0x00])?;

        self.tca_output = 0;
        self.tca_output |= 1 << TCA_BIT_CS;
        self.tca_output &= !(1 << TCA_BIT_RES);
        self.tca_output &= !(1 << TCA_BIT_TP_RESET);
        self.i2c
            .write(TCA6408A_ADDR, &[TCA_REG_OUTPUT, self.tca_output])?;

        // P0..P4 inputs; P5..P7 outputs.
        self.i2c.write(TCA6408A_ADDR, &[TCA_REG_CONFIG, 0x1F])?;

        let mut inb = [0u8; 1];
        let _ = self
            .i2c
            .write_read(TCA6408A_ADDR, &[TCA_REG_INPUT], &mut inb);

        self.log_tca_state("init");
        Ok(())
    }

    fn tca_set_cs_enabled(&mut self, enabled: bool) -> Result<(), esp_hal::i2c::master::Error> {
        if enabled {
            self.tca_output &= !(1 << TCA_BIT_CS);
        } else {
            self.tca_output |= 1 << TCA_BIT_CS;
        }
        self.i2c
            .write(TCA6408A_ADDR, &[TCA_REG_OUTPUT, self.tca_output])?;
        self.log_tca_state("set-cs");
        Ok(())
    }

    fn tca_set_res_released(&mut self, released: bool) -> Result<(), esp_hal::i2c::master::Error> {
        if released {
            self.tca_output |= 1 << TCA_BIT_RES;
        } else {
            self.tca_output &= !(1 << TCA_BIT_RES);
        }
        self.i2c
            .write(TCA6408A_ADDR, &[TCA_REG_OUTPUT, self.tca_output])?;
        self.log_tca_state("set-res");
        Ok(())
    }

    fn tca_set_tp_reset_released(
        &mut self,
        released: bool,
    ) -> Result<(), esp_hal::i2c::master::Error> {
        if released {
            self.tca_output |= 1 << TCA_BIT_TP_RESET;
        } else {
            self.tca_output &= !(1 << TCA_BIT_TP_RESET);
        }
        self.i2c
            .write(TCA6408A_ADDR, &[TCA_REG_OUTPUT, self.tca_output])?;
        self.log_tca_state("set-tp-reset");
        Ok(())
    }

    fn log_tca_state(&self, stage: &str) {
        let cs_enabled = (self.tca_output & (1 << TCA_BIT_CS)) == 0;
        let res_released = (self.tca_output & (1 << TCA_BIT_RES)) != 0;
        let tp_released = (self.tca_output & (1 << TCA_BIT_TP_RESET)) != 0;
        defmt::info!(
            "ui: tca stage={} out=0x{=u8:02x} cs_en={=bool} res_rel={=bool} tp_rel={=bool}",
            stage,
            self.tca_output,
            cs_enabled,
            res_released,
            tp_released
        );
    }

    fn gc9307_driver_init(&mut self) -> Result<(), gc9307_async::Error<esp_hal::spi::Error>> {
        let cfg = esp_hal::spi::master::Config::default()
            .with_frequency(Rate::from_mhz(PANEL_INIT_SPI_FREQ_MHZ))
            .with_mode(Mode::_0);
        let _ = self.panel_io.spi.apply_config(&cfg);
        defmt::info!(
            "ui: gc9307 driver=gc9307-async source=crates.io init_freq_mhz={} mode=0",
            PANEL_INIT_SPI_FREQ_MHZ
        );

        let mut display_buf = [0u8; 1536];

        let panel_cfg = GcConfig {
            orientation: PANEL_ORIENTATION,
            width: LCD_W,
            height: LCD_H,
            dx: OFFSET_X,
            dy: OFFSET_Y,
            rgb: PANEL_RGB_ORDER,
            ..GcConfig::default()
        };

        let spi_dev = NoCsSpiDevice {
            bus: &mut self.panel_io.spi,
        };
        let dc_pin = DcPin {
            pin: &mut self.panel_io.dc,
        };
        let rst_pin = NullRstPin;

        let mut drv = GC9307C::<_, _, _, LocalDelayTimer>::new(
            panel_cfg,
            spi_dev,
            dc_pin,
            rst_pin,
            &mut display_buf,
        );
        drv.init()?;
        // Keep orientation control on the driver API path.
        drv.set_orientation(PANEL_ORIENTATION)?;

        self.panel_io.apply_runtime_config();

        Ok(())
    }

    fn read_inputs(&mut self) -> Result<InputSnapshot, esp_hal::i2c::master::Error> {
        let mut input = [0u8; 1];
        self.i2c
            .write_read(TCA6408A_ADDR, &[TCA_REG_INPUT], &mut input)?;
        let bits = input[0];

        // Front-panel buttons are externally pulled up and shorted to GND when pressed.
        // On current board wiring, P0/P3 are swapped against silk-screen UP/DOWN labels.
        let up = (bits & (1 << 3)) == 0;
        let left = (bits & (1 << 1)) == 0;
        let right = (bits & (1 << 2)) == 0;
        let down = (bits & (1 << 0)) == 0;

        let center = self.btn_center.is_low();
        let touch_irq_active = self.ctp_irq.is_low();
        let touch_point = self.read_touch_point();
        let touch = touch_point.is_some();

        if touch_irq_active && touch_point.is_none() {
            if !self.touch_irq_stuck_hint_logged {
                defmt::warn!(
                    "ui: ctp_irq active without coordinates; ignore irq-only touch to avoid stuck edge"
                );
                self.touch_irq_stuck_hint_logged = true;
            }
        } else if !touch_irq_active {
            self.touch_irq_stuck_hint_logged = false;
        }

        Ok(InputSnapshot {
            up,
            down,
            left,
            right,
            center,
            touch,
            touch_point,
        })
    }

    fn read_touch_point(&mut self) -> Option<(u16, u16)> {
        let mut buf = [0u8; CST816D_TOUCH_REG_LEN];
        if self
            .i2c
            .write_read(CST816D_ADDR, &[CST816D_REG_GESTURE], &mut buf)
            .is_err()
        {
            return None;
        }

        let finger_count = buf[1] & 0x0f;
        if finger_count == 0 {
            return None;
        }

        let x_raw = (((buf[2] & 0x0f) as u16) << 8) | buf[3] as u16;
        let y_raw = (((buf[4] & 0x0f) as u16) << 8) | buf[5] as u16;
        Self::map_touch_to_ui(x_raw, y_raw)
    }

    fn map_touch_to_ui(x_raw: u16, y_raw: u16) -> Option<(u16, u16)> {
        let ui_w = front_panel_scene::UI_W;
        let ui_h = front_panel_scene::UI_H;

        // CST816D on this board reports in a portrait-like space (x=0..UI_H, y=0..UI_W).
        // Front panel is rendered as LandscapeSwapped, so map by axis swap + horizontal mirror.
        if x_raw < ui_h && y_raw < ui_w {
            return Some((ui_w.saturating_sub(1).saturating_sub(y_raw), x_raw));
        }

        // Fallback path for legacy coordinate orderings.
        if x_raw < ui_w && y_raw < ui_h {
            return Some((x_raw, y_raw));
        }
        if y_raw < ui_w && x_raw < ui_h {
            return Some((y_raw, x_raw));
        }
        None
    }

    fn process_touch_action(&mut self, snapshot: InputSnapshot) -> Option<UiAction> {
        let prev = self.last_inputs.unwrap_or_else(InputSnapshot::idle);
        if !snapshot.touch || prev.touch {
            return None;
        }

        if matches!(
            self.self_check_overlay,
            SelfCheckOverlay::BmsActivateResult(..)
        ) {
            self.self_check_overlay = SelfCheckOverlay::None;
            self.needs_redraw = true;
            defmt::info!("ui: bms result dialog close via touch");
            return Some(UiAction::ClearBmsActivationResult);
        }

        let (x, y) = snapshot.touch_point?;

        defmt::info!(
            "ui: touch edge x={=u16} y={=u16} overlay={}",
            x,
            y,
            overlay_name(self.self_check_overlay)
        );

        match front_panel_scene::self_check_hit_test(x, y, self.self_check_overlay) {
            Some(SelfCheckTouchTarget::ActivateCancel) => {
                self.self_check_overlay = SelfCheckOverlay::None;
                self.needs_redraw = true;
                None
            }
            Some(SelfCheckTouchTarget::ActivateConfirm) => {
                self.self_check_overlay = SelfCheckOverlay::BmsActivateProgress;
                self.needs_redraw = true;
                Some(UiAction::RequestBmsActivation)
            }
            Some(SelfCheckTouchTarget::Bq40Card) => {
                let activation_needed =
                    front_panel_scene::is_bq40_activation_needed(&self.self_check_snapshot);
                if self.self_check_overlay == SelfCheckOverlay::None
                    && self.bms_activation_state != BmsActivationState::Pending
                {
                    if let Some(result_overlay) =
                        front_panel_scene::bq40_result_overlay(&self.self_check_snapshot)
                    {
                        self.self_check_overlay = result_overlay;
                        self.needs_redraw = true;
                        defmt::info!("ui: bms result dialog reopen via touch");
                    } else if activation_needed {
                        self.self_check_overlay = SelfCheckOverlay::BmsActivateConfirm;
                        self.needs_redraw = true;
                        defmt::info!("ui: bms activation dialog open via touch");
                    } else {
                        defmt::info!(
                            "ui: bms touch ignored overlay={} activation_needed={=bool} bms_state={}",
                            overlay_name(self.self_check_overlay),
                            activation_needed,
                            bms_activation_state_name(self.bms_activation_state)
                        );
                    }
                } else {
                    defmt::info!(
                        "ui: bms touch ignored overlay={} activation_needed={=bool} bms_state={}",
                        overlay_name(self.self_check_overlay),
                        activation_needed,
                        bms_activation_state_name(self.bms_activation_state)
                    );
                }
                None
            }
            None => {
                defmt::info!(
                    "ui: touch target none x={=u16} y={=u16} overlay={}",
                    x,
                    y,
                    overlay_name(self.self_check_overlay)
                );
                None
            }
        }
    }

    fn snapshot_to_model(&self, _snapshot: InputSnapshot) -> UiModel {
        UiModel {
            mode: self.self_check_snapshot.mode,
            // Runtime pages are data-driven; keys only serve self-check actions.
            focus: UiFocus::Idle,
            touch_irq: false,
            frame_no: self.frame_no,
        }
    }

    fn render_scene<F>(&mut self, draw: F) -> Result<(), esp_hal::spi::Error>
    where
        F: FnOnce(&mut FrameBufferPainter<'_>) -> Result<(), esp_hal::spi::Error>,
    {
        self.display_buffers.copy_displayed_to_render();
        self.dirty_rows.clear();
        {
            let mut painter =
                FrameBufferPainter::new(self.display_buffers.render_mut(), &mut self.dirty_rows);
            draw(&mut painter)?;
        }
        self.dirty_rows.retain_differences(
            self.display_buffers.displayed(),
            self.display_buffers.render(),
        );
        self.panel_io
            .present(&mut self.display_buffers, &mut self.dirty_rows)?;
        self.frame_no = self.frame_no.wrapping_add(1);
        Ok(())
    }

    fn render_inputs(&mut self, snapshot: InputSnapshot) -> Result<(), esp_hal::spi::Error> {
        let model = self.snapshot_to_model(snapshot);
        let variant = self.ui_variant;
        let self_check_snapshot = self.self_check_snapshot;
        let self_check_overlay = self.self_check_overlay;
        self.render_scene(|painter| {
            front_panel_scene::render_frame_with_self_check_overlay(
                painter,
                &model,
                variant,
                Some(&self_check_snapshot),
                self_check_overlay,
            )
        })
    }
}

fn variant_name(variant: UiVariant) -> &'static str {
    match variant {
        UiVariant::InstrumentA => "A",
        UiVariant::InstrumentB => "B",
        UiVariant::RetroC => "C",
        UiVariant::InstrumentD => "D",
    }
}

fn log_self_check_snapshot_transition(previous: &SelfCheckUiSnapshot, next: &SelfCheckUiSnapshot) {
    let summary_changed = previous.mode != next.mode
        || previous.gc9307 != next.gc9307
        || previous.tca6408a != next.tca6408a
        || previous.fusb302 != next.fusb302
        || previous.ina3221 != next.ina3221
        || previous.bq25792 != next.bq25792
        || previous.bq40z50 != next.bq40z50
        || previous.tps_a != next.tps_a
        || previous.tps_b != next.tps_b
        || previous.tmp_a != next.tmp_a
        || previous.tmp_b != next.tmp_b;
    if summary_changed {
        defmt::info!(
            "ui: self_check summary mode={} gc9307={} tca6408a={} fusb302={} ina3221={} bq25792={} bq40z50={} tps_a={} tps_b={} tmp_a={} tmp_b={}",
            ups_mode_name(next.mode),
            self_check_comm_state_name(next.gc9307),
            self_check_comm_state_name(next.tca6408a),
            self_check_comm_state_name(next.fusb302),
            self_check_comm_state_name(next.ina3221),
            self_check_comm_state_name(next.bq25792),
            self_check_comm_state_name(next.bq40z50),
            self_check_comm_state_name(next.tps_a),
            self_check_comm_state_name(next.tps_b),
            self_check_comm_state_name(next.tmp_a),
            self_check_comm_state_name(next.tmp_b)
        );
    }

    let power_detail_changed = previous.fusb302_vbus_present != next.fusb302_vbus_present
        || previous.bq25792_allow_charge != next.bq25792_allow_charge
        || previous.bq25792_ichg_ma != next.bq25792_ichg_ma
        || previous.bq25792_vbat_present != next.bq25792_vbat_present
        || previous.bq40z50_soc_pct != next.bq40z50_soc_pct
        || previous.bq40z50_rca_alarm != next.bq40z50_rca_alarm
        || previous.bq40z50_no_battery != next.bq40z50_no_battery
        || previous.bq40z50_discharge_ready != next.bq40z50_discharge_ready
        || previous.bq40z50_last_result != next.bq40z50_last_result;
    if power_detail_changed {
        defmt::info!(
            "ui: self_check power_detail vbus_present={=?} chg_allow={=?} chg_ichg_ma={=?} vbat_present={=?} bq40_soc_pct={=?} bq40_rca_alarm={=?} bq40_no_battery={=?} bq40_dsg_ready={=?} bq40_last_result={}",
            next.fusb302_vbus_present,
            next.bq25792_allow_charge,
            next.bq25792_ichg_ma,
            next.bq25792_vbat_present,
            next.bq40z50_soc_pct,
            next.bq40z50_rca_alarm,
            next.bq40z50_no_battery,
            next.bq40z50_discharge_ready,
            bms_result_option_name(next.bq40z50_last_result)
        );
    }
}

fn overlay_name(overlay: SelfCheckOverlay) -> &'static str {
    match overlay {
        SelfCheckOverlay::None => "none",
        SelfCheckOverlay::BmsActivateConfirm => "confirm",
        SelfCheckOverlay::BmsActivateProgress => "progress",
        SelfCheckOverlay::BmsActivateResult(front_panel_scene::BmsResultKind::Success) => {
            "result_success"
        }
        SelfCheckOverlay::BmsActivateResult(front_panel_scene::BmsResultKind::NoBattery) => {
            "result_no_battery"
        }
        SelfCheckOverlay::BmsActivateResult(front_panel_scene::BmsResultKind::RomMode) => {
            "result_rom_mode"
        }
        SelfCheckOverlay::BmsActivateResult(front_panel_scene::BmsResultKind::Abnormal) => {
            "result_abnormal"
        }
        SelfCheckOverlay::BmsActivateResult(front_panel_scene::BmsResultKind::NotDetected) => {
            "result_not_detected"
        }
    }
}

fn ups_mode_name(mode: UpsMode) -> &'static str {
    match mode {
        UpsMode::Off => "off",
        UpsMode::Standby => "standby",
        UpsMode::Supplement => "supplement",
        UpsMode::Backup => "backup",
    }
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

fn bms_activation_state_name(state: BmsActivationState) -> &'static str {
    match state {
        BmsActivationState::Idle => "idle",
        BmsActivationState::Pending => "pending",
        BmsActivationState::Result(result) => match result {
            BmsResultKind::Success => "result_success",
            BmsResultKind::NoBattery => "result_no_battery",
            BmsResultKind::RomMode => "result_rom_mode",
            BmsResultKind::Abnormal => "result_abnormal",
            BmsResultKind::NotDetected => "result_not_detected",
        },
    }
}

#[allow(dead_code)]
fn orientation_label(orientation: Orientation) -> &'static str {
    match orientation {
        Orientation::Portrait => "ORI: PORTRAIT (MADCTL=0x40)",
        Orientation::Landscape => "ORI: LANDSCAPE (MADCTL=0x20)",
        Orientation::PortraitSwapped => "ORI: PORTRAIT_SWAP (MADCTL=0x80)",
        Orientation::LandscapeSwapped => "ORI: LANDSCAPE_SWAP (MADCTL=0xE0)",
    }
}

struct PanelIo {
    spi: SpiDmaBus<'static, Blocking>,
    dc: Flex<'static>,
}

impl PanelIo {
    fn configure_dc(&mut self) {
        self.dc.apply_output_config(
            &OutputConfig::default()
                .with_drive_mode(DriveMode::PushPull)
                .with_pull(Pull::None),
        );
        self.dc.set_low();
        self.dc.set_output_enable(true);
    }

    fn apply_runtime_config(&mut self) {
        let cfg = esp_hal::spi::master::Config::default()
            .with_frequency(Rate::from_mhz(PANEL_RUNTIME_SPI_FREQ_MHZ))
            .with_mode(Mode::_0);
        self.spi
            .apply_config(&cfg)
            .expect("display runtime spi config should be valid");
        defmt::info!(
            "ui: display runtime path mode=dma freq_mhz={} staging_bytes={}",
            PANEL_RUNTIME_SPI_FREQ_MHZ,
            DMA_STAGING_BYTES
        );
    }

    fn write_cmd(&mut self, cmd: u8) -> Result<(), esp_hal::spi::Error> {
        self.dc.set_low();
        self.spi.write(&[cmd])
    }

    fn write_data(&mut self, data: &[u8]) -> Result<(), esp_hal::spi::Error> {
        self.dc.set_high();
        self.spi.write(data)
    }

    fn set_window(
        &mut self,
        x0: u16,
        y0: u16,
        x1: u16,
        y1: u16,
    ) -> Result<(), esp_hal::spi::Error> {
        let sx = x0.saturating_add(OFFSET_X);
        let sy = y0.saturating_add(OFFSET_Y);
        let ex = x1.saturating_add(OFFSET_X);
        let ey = y1.saturating_add(OFFSET_Y);

        self.write_cmd(CMD_CASET)?;
        self.write_data(&u16_be_pair(sx, ex))?;
        self.write_cmd(CMD_RASET)?;
        self.write_data(&u16_be_pair(sy, ey))?;
        Ok(())
    }

    fn present(
        &mut self,
        display_buffers: &mut DisplayBuffers,
        dirty_rows: &mut DirtyRows,
    ) -> Result<(), esp_hal::spi::Error> {
        if dirty_rows.any() {
            let source = display_buffers.render();
            for band in dirty_rows.bands() {
                let start = band.start_row * FRAME_WIDTH;
                let pixels = band.row_count * FRAME_WIDTH;
                let byte_len = pixels * core::mem::size_of::<u16>();
                let band_bytes = unsafe {
                    core::slice::from_raw_parts(
                        source[start..start + pixels].as_ptr().cast(),
                        byte_len,
                    )
                };
                self.set_window(
                    0,
                    band.start_row as u16,
                    (FRAME_WIDTH - 1) as u16,
                    (band.start_row + band.row_count - 1) as u16,
                )?;
                self.write_cmd(CMD_RAMWR)?;
                self.write_data(band_bytes)?;
            }
        }

        display_buffers.commit_present();
        dirty_rows.clear();
        Ok(())
    }
}

struct FrameBufferPainter<'a> {
    frame: &'a mut [u16],
    dirty_rows: &'a mut DirtyRows,
}

impl<'a> FrameBufferPainter<'a> {
    fn new(frame: &'a mut [u16], dirty_rows: &'a mut DirtyRows) -> Self {
        Self { frame, dirty_rows }
    }
}

impl UiPainter for FrameBufferPainter<'_> {
    type Error = esp_hal::spi::Error;

    fn fill_rect(
        &mut self,
        x: u16,
        y: u16,
        w: u16,
        h: u16,
        rgb565: u16,
    ) -> Result<(), Self::Error> {
        if w == 0 || h == 0 {
            return Ok(());
        }

        let x0 = x as usize;
        let y0 = y as usize;
        if x0 >= FRAME_WIDTH || y0 >= FRAME_HEIGHT {
            return Ok(());
        }

        let x1 = x0.saturating_add(w as usize).min(FRAME_WIDTH);
        let y1 = y0.saturating_add(h as usize).min(FRAME_HEIGHT);
        if x1 <= x0 || y1 <= y0 {
            return Ok(());
        }

        let stored_color = rgb565.to_be();
        self.dirty_rows.mark_range(y0, y1 - y0);
        for row in y0..y1 {
            let start = row * FRAME_WIDTH + x0;
            let end = row * FRAME_WIDTH + x1;
            self.frame[start..end].fill(stored_color);
        }

        Ok(())
    }
}

struct LocalDelayTimer;

impl GcTimer for LocalDelayTimer {
    fn after_millis(milliseconds: u64) -> impl core::future::Future<Output = ()> {
        busy_wait(Duration::from_millis(milliseconds));
        core::future::ready(())
    }
}

struct NullRstPin;

impl embedded_hal::digital::ErrorType for NullRstPin {
    type Error = Infallible;
}

impl OutputPin for NullRstPin {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn set_high(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

struct DcPin<'a> {
    pin: &'a mut Flex<'static>,
}

impl embedded_hal::digital::ErrorType for DcPin<'_> {
    type Error = Infallible;
}

impl OutputPin for DcPin<'_> {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        self.pin.set_low();
        Ok(())
    }

    fn set_high(&mut self) -> Result<(), Self::Error> {
        self.pin.set_high();
        Ok(())
    }
}

struct NoCsSpiDevice<'a, BUS> {
    bus: &'a mut BUS,
}

impl<BUS> embedded_hal::spi::ErrorType for NoCsSpiDevice<'_, BUS>
where
    BUS: SpiBus<Error = esp_hal::spi::Error>,
{
    type Error = esp_hal::spi::Error;
}

impl<BUS> SpiDevice for NoCsSpiDevice<'_, BUS>
where
    BUS: SpiBus<Error = esp_hal::spi::Error>,
{
    fn transaction(&mut self, operations: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
        for op in operations {
            match op {
                Operation::Read(buf) => self.bus.read(buf)?,
                Operation::Write(buf) => self.bus.write(buf)?,
                Operation::Transfer(read, write) => {
                    let count = core::cmp::min(read.len(), write.len());
                    self.bus.transfer(&mut read[..count], &write[..count])?;
                }
                Operation::TransferInPlace(buf) => self.bus.transfer_in_place(buf)?,
                Operation::DelayNs(ns) => {
                    let micros = (*ns as u64).saturating_add(999) / 1000;
                    if micros > 0 {
                        busy_wait(Duration::from_micros(micros));
                    }
                }
            }
        }
        Ok(())
    }
}

fn busy_wait(duration: Duration) {
    let start = Instant::now();
    while start.elapsed() < duration {}
}

fn u16_be_pair(a: u16, b: u16) -> [u8; 4] {
    let [a0, a1] = a.to_be_bytes();
    let [b0, b1] = b.to_be_bytes();
    [a0, a1, b0, b1]
}

fn i2c_error_kind(e: esp_hal::i2c::master::Error) -> &'static str {
    use esp_hal::i2c::master::Error;

    match e {
        Error::FifoExceeded => "i2c_fifo_exceeded",
        Error::AcknowledgeCheckFailed(_) => "i2c_nack",
        Error::Timeout => "i2c_timeout",
        Error::ArbitrationLost => "i2c_arb_lost",
        Error::ExecutionIncomplete => "i2c_exec_incomplete",
        _ => "i2c_other",
    }
}
