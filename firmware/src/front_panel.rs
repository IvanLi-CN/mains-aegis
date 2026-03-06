use core::convert::Infallible;

use crate::front_panel_scene::{
    self, AudioTestUiState, BmsActivationState, SelfCheckOverlay, SelfCheckTouchTarget,
    SelfCheckUiSnapshot, TestFunctionUi, UiFocus, UiModel, UiPainter, UiVariant,
};
use embedded_hal::digital::OutputPin;
use embedded_hal::spi::{Operation, SpiBus, SpiDevice};
use esp_hal::gpio::{DriveMode, Flex, Input, OutputConfig, Pull};
use esp_hal::i2c::master::I2c as HalI2c;
use esp_hal::spi::{master::Spi as HalSpi, Mode};
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
const SELF_CHECK_VARIANT: UiVariant = UiVariant::RetroC;

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
    spi: HalSpi<'static, Blocking>,
    btn_center: Input<'static>,
    ctp_irq: Input<'static>,
    tca_reset_n: Flex<'static>,
    dc: Flex<'static>,
    bl: Flex<'static>,

    tca_output: u8,

    state: InitState,
    next_frame_deadline: Instant,
    last_inputs: Option<InputSnapshot>,
    needs_redraw: bool,
    ui_variant: UiVariant,
    self_check_snapshot: SelfCheckUiSnapshot,
    bms_activation_state: BmsActivationState,
    self_check_overlay: SelfCheckOverlay,
    frame_no: u32,
}

impl FrontPanel {
    pub fn new(
        i2c: HalI2c<'static, Blocking>,
        spi: HalSpi<'static, Blocking>,
        btn_center: Input<'static>,
        ctp_irq: Input<'static>,
        tca_reset_n: Flex<'static>,
        dc: Flex<'static>,
        bl: Flex<'static>,
    ) -> Self {
        Self {
            i2c,
            spi,
            btn_center,
            ctp_irq,
            tca_reset_n,
            dc,
            bl,
            tca_output: 0,
            state: InitState::Disabled,
            next_frame_deadline: Instant::now(),
            last_inputs: None,
            needs_redraw: false,
            ui_variant: SELF_CHECK_VARIANT,
            self_check_snapshot: SelfCheckUiSnapshot::pending(front_panel_scene::UpsMode::Standby),
            bms_activation_state: BmsActivationState::Idle,
            self_check_overlay: SelfCheckOverlay::None,
            frame_no: 0,
        }
    }

    pub fn init_best_effort(&mut self) {
        self.configure_dc();
        self.configure_backlight();
        self.configure_tca_reset();

        // Reset expander and force known screen-safe defaults.
        self.pulse_tca_reset(Duration::from_millis(10));

        if let Err(e) = self.tca_init() {
            defmt::error!("ui: tca6408a init failed err={}", i2c_error_kind(e));
            self.state = InitState::Disabled;
            return;
        }

        if let Err(e) = self.tca_set_res_released(false) {
            defmt::error!("ui: tca set res failed err={}", i2c_error_kind(e));
            self.state = InitState::Disabled;
            return;
        }
        if let Err(e) = self.tca_set_tp_reset_released(false) {
            defmt::error!("ui: tca set tp_reset failed err={}", i2c_error_kind(e));
            self.state = InitState::Disabled;
            return;
        }
        if let Err(e) = self.tca_set_cs_enabled(false) {
            defmt::error!("ui: tca set cs failed err={}", i2c_error_kind(e));
            self.state = InitState::Disabled;
            return;
        }

        busy_wait(Duration::from_millis(10));

        // Hardware reset through expander lines before handing over to driver init.
        if let Err(e) = self.tca_set_res_released(true) {
            defmt::error!("ui: tca release res failed err={}", i2c_error_kind(e));
            self.state = InitState::Disabled;
            return;
        }
        if let Err(e) = self.tca_set_tp_reset_released(true) {
            defmt::error!("ui: tca release tp_reset failed err={}", i2c_error_kind(e));
            self.state = InitState::Disabled;
            return;
        }
        busy_wait(Duration::from_millis(120));

        if let Err(e) = self.tca_set_cs_enabled(true) {
            defmt::error!("ui: tca enable cs failed err={}", i2c_error_kind(e));
            self.state = InitState::Disabled;
            return;
        }
        busy_wait(Duration::from_millis(5));

        if self.gc9307_driver_init().is_err() {
            defmt::error!("ui: gc9307 driver init failed");
            let _ = self.tca_set_cs_enabled(false);
            let _ = self.tca_set_res_released(false);
            self.set_backlight(false);
            self.state = InitState::Disabled;
            return;
        }

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

        self.set_backlight(true);
        self.state = InitState::Ready;
        self.next_frame_deadline = Instant::now();

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
    }

    pub fn update_self_check_snapshot(&mut self, snapshot: SelfCheckUiSnapshot) {
        if self.self_check_snapshot == snapshot {
            return;
        }
        self.self_check_snapshot = snapshot;
        if self.self_check_overlay == SelfCheckOverlay::BmsActivateConfirm
            && !front_panel_scene::is_bq40_activation_needed(&self.self_check_snapshot)
        {
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
        if self.bms_activation_state == state {
            return;
        }
        self.bms_activation_state = state;
        self.self_check_overlay = match state {
            BmsActivationState::Idle => SelfCheckOverlay::None,
            BmsActivationState::Pending => SelfCheckOverlay::BmsActivateProgress,
            BmsActivationState::Succeeded => SelfCheckOverlay::BmsActivateResult { success: true },
            BmsActivationState::FailedNoInput
            | BmsActivationState::FailedTimeout
            | BmsActivationState::FailedComm => {
                SelfCheckOverlay::BmsActivateResult { success: false }
            }
        };
        self.needs_redraw = true;
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
                ui_action = self.process_touch_action(snapshot);
                let inputs_changed = self.last_inputs != Some(snapshot);
                if inputs_changed || self.needs_redraw {
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
        let mut painter = PanelPainter { panel: self };
        if let Err(e) = front_panel_scene::render_display_diagnostic(&mut painter, &meta) {
            defmt::error!("ui: render display diag failed err={=?}", e);
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
        let mut painter = PanelPainter { panel: self };
        if let Err(e) =
            front_panel_scene::render_test_navigation(&mut painter, selected, default_test)
        {
            defmt::error!("ui: render test navigation failed err={=?}", e);
        }
    }

    #[allow(dead_code)]
    pub fn render_test_screen_static(&mut self, back_enabled: bool) {
        if self.state != InitState::Ready {
            return;
        }
        let mut painter = PanelPainter { panel: self };
        let color_order_label = if PANEL_RGB_ORDER {
            "COLOR ORDER: RGB565"
        } else {
            "COLOR ORDER: BGR565"
        };
        if let Err(e) = front_panel_scene::render_test_screen_static(
            &mut painter,
            back_enabled,
            color_order_label,
        ) {
            defmt::error!("ui: render screen static test failed err={=?}", e);
        }
    }

    #[allow(dead_code)]
    pub fn render_test_audio_playback(&mut self, back_enabled: bool, state: AudioTestUiState) {
        if self.state != InitState::Ready {
            return;
        }
        let mut painter = PanelPainter { panel: self };
        if let Err(e) =
            front_panel_scene::render_test_audio_playback(&mut painter, back_enabled, state)
        {
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
        } else if snapshot.touch && !prev.touch {
            if let Some((x, y)) = snapshot.touch_point {
                event = Some(TestInputEvent::Touch { x, y });
            } else {
                // Keep touch edge pending until we have a usable coordinate sample.
                next_snapshot.touch = false;
                next_snapshot.touch_point = None;
            }
        } else if snapshot.touch && prev.touch {
            if let (Some((x, y)), Some((_, prev_y))) = (snapshot.touch_point, prev.touch_point) {
                let dy = y as i16 - prev_y as i16;
                if dy.abs() >= 3 {
                    event = Some(TestInputEvent::TouchDrag { x, y, dy });
                }
            }
        }

        self.last_inputs = Some(next_snapshot);
        event
    }

    fn configure_dc(&mut self) {
        self.dc.apply_output_config(
            &OutputConfig::default()
                .with_drive_mode(DriveMode::PushPull)
                .with_pull(Pull::None),
        );
        self.dc.set_low();
        self.dc.set_output_enable(true);
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
        // Match the reference driver default bus profile.
        let cfg = esp_hal::spi::master::Config::default()
            .with_frequency(Rate::from_mhz(10))
            .with_mode(Mode::_0);
        let _ = self.spi.apply_config(&cfg);
        defmt::info!("ui: gc9307 driver=gc9307-async source=crates.io freq_mhz=10 mode=0");

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

        let spi_dev = NoCsSpiDevice { bus: &mut self.spi };
        let dc_pin = DcPin { pin: &mut self.dc };
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

        Ok(())
    }

    fn write_cmd(&mut self, cmd: u8) -> Result<(), esp_hal::spi::Error> {
        self.dc.set_low();
        self.spi.write(&[cmd])
    }

    fn write_data(&mut self, data: &[u8]) -> Result<(), esp_hal::spi::Error> {
        self.dc.set_high();
        self.spi.write(data)
    }

    fn read_inputs(&mut self) -> Result<InputSnapshot, esp_hal::i2c::master::Error> {
        let mut input = [0u8; 1];
        self.i2c
            .write_read(TCA6408A_ADDR, &[TCA_REG_INPUT], &mut input)?;
        let bits = input[0];

        // Front-panel buttons are externally pulled up and shorted to GND when pressed.
        // Wiring is physically mirrored relative to the logical D-pad.
        let up = (bits & (1 << 0)) == 0;
        let left = (bits & (1 << 1)) == 0;
        let right = (bits & (1 << 2)) == 0;
        let down = (bits & (1 << 3)) == 0;

        let center = self.btn_center.is_low();
        let touch = self.ctp_irq.is_low();
        let touch_point = self.read_touch_point(touch);

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

    fn read_touch_point(&mut self, touch_active: bool) -> Option<(u16, u16)> {
        if !touch_active {
            return None;
        }

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
        if x_raw < front_panel_scene::UI_W && y_raw < front_panel_scene::UI_H {
            return Some((x_raw, y_raw));
        }
        if y_raw < front_panel_scene::UI_W && x_raw < front_panel_scene::UI_H {
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
            SelfCheckOverlay::BmsActivateResult { .. }
        ) {
            self.self_check_overlay = SelfCheckOverlay::None;
            self.needs_redraw = true;
            return Some(UiAction::ClearBmsActivationResult);
        }

        let Some((x, y)) = snapshot.touch_point else {
            return None;
        };

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
                if self.self_check_overlay == SelfCheckOverlay::None
                    && front_panel_scene::is_bq40_activation_needed(&self.self_check_snapshot)
                    && self.bms_activation_state != BmsActivationState::Pending
                {
                    self.self_check_overlay = SelfCheckOverlay::BmsActivateConfirm;
                    self.needs_redraw = true;
                }
                None
            }
            None => None,
        }
    }

    fn snapshot_to_model(&self, snapshot: InputSnapshot) -> UiModel {
        let focus = if snapshot.center {
            UiFocus::Center
        } else if snapshot.up {
            UiFocus::Up
        } else if snapshot.down {
            UiFocus::Down
        } else if snapshot.left {
            UiFocus::Left
        } else if snapshot.right {
            UiFocus::Right
        } else if snapshot.touch {
            UiFocus::Touch
        } else {
            UiFocus::Idle
        };

        UiModel {
            mode: self.self_check_snapshot.mode,
            focus,
            touch_irq: snapshot.touch,
            frame_no: self.frame_no,
        }
    }

    fn render_inputs(&mut self, snapshot: InputSnapshot) -> Result<(), esp_hal::spi::Error> {
        let model = self.snapshot_to_model(snapshot);
        let variant = self.ui_variant;
        let self_check_snapshot = self.self_check_snapshot;
        let self_check_overlay = self.self_check_overlay;
        {
            let mut painter = PanelPainter { panel: self };
            front_panel_scene::render_frame_with_self_check_overlay(
                &mut painter,
                &model,
                variant,
                Some(&self_check_snapshot),
                self_check_overlay,
            )?;
        }
        self.frame_no = self.frame_no.wrapping_add(1);
        Ok(())
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

    fn fill_rect(
        &mut self,
        x: u16,
        y: u16,
        w: u16,
        h: u16,
        color: u16,
    ) -> Result<(), esp_hal::spi::Error> {
        if w == 0 || h == 0 {
            return Ok(());
        }

        let x1 = x.saturating_add(w - 1);
        let y1 = y.saturating_add(h - 1);

        self.set_window(x, y, x1, y1)?;
        self.write_cmd(CMD_RAMWR)?;

        let [hi, lo] = color.to_be_bytes();
        let mut buf = [0u8; 64];
        for chunk in buf.chunks_exact_mut(2) {
            chunk[0] = hi;
            chunk[1] = lo;
        }

        let mut remaining: u32 = (w as u32) * (h as u32);
        while remaining > 0 {
            let pixels = core::cmp::min(remaining, (buf.len() / 2) as u32);
            let bytes = (pixels as usize) * 2;
            self.write_data(&buf[..bytes])?;
            remaining -= pixels;
        }

        Ok(())
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

#[allow(dead_code)]
fn orientation_label(orientation: Orientation) -> &'static str {
    match orientation {
        Orientation::Portrait => "ORI: PORTRAIT (MADCTL=0x40)",
        Orientation::Landscape => "ORI: LANDSCAPE (MADCTL=0x20)",
        Orientation::PortraitSwapped => "ORI: PORTRAIT_SWAP (MADCTL=0x80)",
        Orientation::LandscapeSwapped => "ORI: LANDSCAPE_SWAP (MADCTL=0xE0)",
    }
}

struct PanelPainter<'a> {
    panel: &'a mut FrontPanel,
}

impl UiPainter for PanelPainter<'_> {
    type Error = esp_hal::spi::Error;

    fn fill_rect(
        &mut self,
        x: u16,
        y: u16,
        w: u16,
        h: u16,
        rgb565: u16,
    ) -> Result<(), Self::Error> {
        self.panel.fill_rect(x, y, w, h, rgb565)
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

struct NoCsSpiDevice<'a> {
    bus: &'a mut HalSpi<'static, Blocking>,
}

impl embedded_hal::spi::ErrorType for NoCsSpiDevice<'_> {
    type Error = esp_hal::spi::Error;
}

impl SpiDevice for NoCsSpiDevice<'_> {
    fn transaction(&mut self, operations: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
        for op in operations {
            match op {
                Operation::Read(buf) => self.bus.read(buf)?,
                Operation::Write(buf) => self.bus.write(buf)?,
                Operation::Transfer(read, write) => {
                    let count = core::cmp::min(read.len(), write.len());
                    read[..count].copy_from_slice(&write[..count]);
                    self.bus.transfer(&mut read[..count])?;
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
