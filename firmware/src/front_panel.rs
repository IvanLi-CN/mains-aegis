use core::convert::Infallible;

use crate::front_panel_logic::{
    cst816d_vertical_gesture_direction, dashboard_enter_requires_variant_switch,
    dashboard_page_for_vertical_menu_gesture, dashboard_uses_frame_animation,
    VerticalGestureDirection, DASHBOARD_VARIANT, SELF_CHECK_VARIANT,
};
use crate::front_panel_scene::{
    self, AudioTestUiState, BeeperPrefs, BeeperSettingTarget, BmsActivationState,
    BmsRecoveryUiAction, BmsResultKind, DashboardHomeFocus, DashboardMenuStyle,
    DashboardPrimaryPage, DashboardRoute, DashboardShellState, DashboardTouchTarget,
    ManualChargeUiAction, MenuItem, SelfCheckCommState, SelfCheckHardwareTarget, SelfCheckOverlay,
    SelfCheckTouchTarget, SelfCheckUiSnapshot, TestFunctionUi, TpsTestUiSnapshot, UiFocus, UiModel,
    UiPainter, UiVariant, UpsMode,
};
use embedded_hal::digital::OutputPin;
use embedded_hal::spi::{Operation, SpiBus, SpiDevice};
use esp_firmware::display_pipeline::{
    DirtyRows, DisplayBufferError, DisplayBuffers, DMA_STAGING_BYTES, FRAME_HEIGHT, FRAME_WIDTH,
};
use esp_firmware::display_power::{
    DisplayPowerCommand, DisplayPowerController, DisplayPowerMode, DisplayPowerPolicy,
};
use esp_hal::dma::{DmaChannelFor, DmaRxBuf, DmaTxBuf};
use esp_hal::gpio::{DriveMode, Flex, Input, OutputConfig, Pull};
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
const CMD_SLEEP_IN: u8 = 0x10;
const CMD_SLEEP_OUT: u8 = 0x11;
const CMD_DISPLAY_OFF: u8 = 0x28;
const CMD_DISPLAY_ON: u8 = 0x29;
const CMD_WRITE_DISPLAY_BRIGHTNESS: u8 = 0x51;
const CMD_WRITE_CTRL_DISPLAY: u8 = 0x53;

// Front panel is currently mounted 180° from the original lab orientation.
const LCD_W: u16 = 320;
const LCD_H: u16 = 172;
const OFFSET_X: u16 = 0;
const OFFSET_Y: u16 = 34;
const PANEL_ORIENTATION: Orientation = Orientation::LandscapeSwapped;
const PANEL_RGB_ORDER: bool = false;
const UI_ORIENTATION_MARKER: &str = "FP_ORI_PROBE_20260227";

const BACKLIGHT_ACTIVE_LOW: bool = true;
const BACKLIGHT_PWM_CHANNEL: usize = 3;
const BACKLIGHT_PWM_DUTY_BITS: u32 = 10;
const BACKLIGHT_PWM_FULL_PCT: u8 = 100;
const BACKLIGHT_PWM_DIM_PCT: u8 = 12;
const DISPLAY_BRIGHTNESS_FULL: u8 = 0xFF;
const DISPLAY_BRIGHTNESS_DIM: u8 = 0x40;
const DISPLAY_CTRL_BRIGHTNESS_ON_BACKLIGHT_ON: u8 = 0x24;
const DISPLAY_CTRL_BRIGHTNESS_DIM_BACKLIGHT_ON: u8 = 0x2C;

const FRAME_INTERVAL: Duration = Duration::from_millis(50);
const DASHBOARD_MENU_ANIMATION_FRAME_INTERVAL: Duration = Duration::from_millis(16);
const CENTER_LONG_PRESS_THRESHOLD: Duration = Duration::from_millis(800);
const BOOT_SPLASH_HOLD: Duration = Duration::from_millis(900);
const DASHBOARD_MENU_ANIMATION_STEPS: u8 = 10;
const PANEL_INIT_SPI_FREQ_MHZ: u32 = 10;
const PANEL_RUNTIME_SPI_FREQ_MHZ: u32 = if cfg!(feature = "display-spi-20mhz") {
    20
} else if cfg!(feature = "display-spi-40mhz") {
    40
} else {
    80
};
const DASHBOARD_MENU_DRAG_THRESHOLD_PX: i16 = 28;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiAction {
    RequestBmsRecovery(BmsRecoveryUiAction),
    ManualCharge(ManualChargeUiAction),
    BeeperPreview {
        prefs: BeeperPrefs,
        target: BeeperSettingTarget,
    },
    BeeperPrefsChanged {
        prefs: BeeperPrefs,
    },
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
    touch_gesture_raw: u8,
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
            touch_gesture_raw: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TouchSample {
    gesture_raw: u8,
    point: Option<(u16, u16)>,
}

const fn input_snapshot_has_activity(snapshot: InputSnapshot) -> bool {
    snapshot.up
        || snapshot.down
        || snapshot.left
        || snapshot.right
        || snapshot.center
        || snapshot.touch
        || snapshot.touch_gesture_raw != 0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TcaDiagSnapshot {
    input: u8,
    output: u8,
    polarity: u8,
    config: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cst816dDiagSnapshot {
    gesture: u8,
    finger_count: u8,
    raw_x: u16,
    raw_y: u16,
    mapped_point: Option<(u16, u16)>,
}

struct DashboardMenuAnimation {
    from_offset_y: i16,
    target_offset_y: i16,
    step_index: u8,
}

pub enum BacklightControl {
    Gpio(Flex<'static>),
    LedcChannel3 { brightness_pct: u8 },
}

impl From<Flex<'static>> for BacklightControl {
    fn from(bl: Flex<'static>) -> Self {
        Self::Gpio(bl)
    }
}

impl BacklightControl {
    fn configure(&mut self) {
        match self {
            Self::Gpio(bl) => {
                bl.apply_output_config(
                    &OutputConfig::default()
                        .with_drive_mode(DriveMode::PushPull)
                        .with_pull(Pull::None),
                );
                bl.set_input_enable(true);
                bl.set_output_enable(true);
            }
            Self::LedcChannel3 { .. } => {}
        }
    }

    fn set_brightness_pct(&mut self, pct: u8) {
        let pct = pct.min(100);
        match self {
            Self::Gpio(bl) => {
                let on = pct > 0;
                if BACKLIGHT_ACTIVE_LOW {
                    if on {
                        bl.set_low();
                    } else {
                        bl.set_high();
                    }
                } else if on {
                    bl.set_high();
                } else {
                    bl.set_low();
                }
            }
            Self::LedcChannel3 { brightness_pct } => {
                *brightness_pct = pct;
                set_backlight_pwm_channel3_pct(pct);
            }
        }
    }

    fn raw_level_high(&self) -> Option<bool> {
        match self {
            Self::Gpio(bl) => Some(bl.is_high()),
            Self::LedcChannel3 { .. } => None,
        }
    }

    fn brightness_pct(&self) -> u8 {
        match self {
            Self::Gpio(bl) => {
                let is_on = if BACKLIGHT_ACTIVE_LOW {
                    bl.is_low()
                } else {
                    bl.is_high()
                };
                if is_on {
                    BACKLIGHT_PWM_FULL_PCT
                } else {
                    0
                }
            }
            Self::LedcChannel3 { brightness_pct } => *brightness_pct,
        }
    }

    fn is_on(&self) -> bool {
        self.brightness_pct() > 0
    }
}

fn set_backlight_pwm_channel3_pct(brightness_pct: u8) {
    let output_high_pct = if BACKLIGHT_ACTIVE_LOW {
        100 - brightness_pct.min(100)
    } else {
        brightness_pct.min(100)
    };
    let duty_range = 1u32 << BACKLIGHT_PWM_DUTY_BITS;
    let duty = (duty_range * output_high_pct as u32) / 100;
    let ledc = esp_hal::peripherals::LEDC::regs();
    ledc.ch(BACKLIGHT_PWM_CHANNEL)
        .duty()
        .write(|w| unsafe { w.duty().bits(duty << 4) });
    ledc.ch(BACKLIGHT_PWM_CHANNEL).conf1().write(|w| {
        w.duty_start().set_bit();
        w.duty_inc().set_bit();
        unsafe {
            w.duty_num().bits(0x1);
            w.duty_cycle().bits(0x1);
            w.duty_scale().bits(0x0)
        }
    });
    ledc.ch(BACKLIGHT_PWM_CHANNEL)
        .conf0()
        .modify(|_, w| w.para_up().set_bit());
}

pub struct FrontPanel<I2C>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    i2c: I2C,
    panel_io: PanelIo,
    btn_center: Input<'static>,
    ctp_irq: Input<'static>,
    tca_reset_n: Flex<'static>,
    backlight: BacklightControl,
    display_buffers: DisplayBuffers,
    dirty_rows: DirtyRows,

    tca_output: u8,

    state: InitState,
    next_frame_deadline: Instant,
    last_inputs: Option<InputSnapshot>,
    center_press_started_at: Option<Instant>,
    center_long_press_fired: bool,
    last_test_touch_point: Option<(u16, u16)>,
    needs_redraw: bool,
    ui_variant: UiVariant,
    dashboard_page: DashboardPrimaryPage,
    dashboard_route: DashboardRoute,
    dashboard_home_focus: DashboardHomeFocus,
    dashboard_menu_selected: MenuItem,
    dashboard_menu_offset_y: i16,
    dashboard_menu_animation: Option<DashboardMenuAnimation>,
    beeper_prefs: BeeperPrefs,
    self_check_snapshot: SelfCheckUiSnapshot,
    bms_activation_state: BmsActivationState,
    self_check_overlay: SelfCheckOverlay,
    dashboard_touch_gesture_consumed: bool,
    touch_irq_stuck_hint_logged: bool,
    frame_no: u32,
    display_power_epoch: Instant,
    display_power: DisplayPowerController,
    attention_hold: bool,
}

impl<I2C> FrontPanel<I2C>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        i2c: I2C,
        spi: HalSpi<'static, Blocking>,
        dma_channel: impl DmaChannelFor<AnySpi<'static>>,
        psram: PSRAM<'static>,
        btn_center: Input<'static>,
        ctp_irq: Input<'static>,
        tca_reset_n: Flex<'static>,
        dc: Flex<'static>,
        backlight: impl Into<BacklightControl>,
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
            backlight: backlight.into(),
            display_buffers,
            dirty_rows: DirtyRows::new(),
            tca_output: 0,
            state: InitState::Disabled,
            next_frame_deadline: Instant::now(),
            last_inputs: None,
            center_press_started_at: None,
            center_long_press_fired: false,
            last_test_touch_point: None,
            needs_redraw: false,
            ui_variant: SELF_CHECK_VARIANT,
            dashboard_page: DashboardPrimaryPage::DashboardHome,
            dashboard_route: DashboardRoute::Home,
            dashboard_home_focus: DashboardHomeFocus::Output,
            dashboard_menu_selected: MenuItem::Dashboard,
            dashboard_menu_offset_y: 0,
            dashboard_menu_animation: None,
            beeper_prefs: BeeperPrefs::defaults(),
            self_check_snapshot: SelfCheckUiSnapshot::pending(front_panel_scene::UpsMode::Standby),
            bms_activation_state: BmsActivationState::Idle,
            self_check_overlay: SelfCheckOverlay::None,
            dashboard_touch_gesture_consumed: false,
            touch_irq_stuck_hint_logged: false,
            frame_no: 0,
            display_power_epoch: Instant::now(),
            display_power: DisplayPowerController::new(DisplayPowerPolicy::release_default(), 0),
            attention_hold: false,
        }
    }

    pub fn init_best_effort(&mut self) {
        if self.reinitialize_display_path("boot_init").is_err() {
            return;
        }

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

    fn reinitialize_display_path(&mut self, trigger: &'static str) -> Result<(), ()> {
        self.panel_io.configure_dc();
        self.configure_backlight();
        self.configure_tca_reset();

        defmt::info!("ui: display_reinit trigger={} stage=tca_reset", trigger);
        self.pulse_tca_reset(Duration::from_millis(10));

        defmt::info!("ui: display_reinit trigger={} stage=tca_init", trigger);
        if let Err(e) = self.tca_init() {
            self.fail_display_reinit(trigger, "tca_init", i2c_error_kind(e));
            return Err(());
        }
        if let Err(e) = self.tca_set_res_released(false) {
            self.fail_display_reinit(trigger, "safe_lines_res", i2c_error_kind(e));
            return Err(());
        }
        if let Err(e) = self.tca_set_tp_reset_released(false) {
            self.fail_display_reinit(trigger, "safe_lines_tp_reset", i2c_error_kind(e));
            return Err(());
        }
        if let Err(e) = self.tca_set_cs_enabled(false) {
            self.fail_display_reinit(trigger, "safe_lines_cs", i2c_error_kind(e));
            return Err(());
        }
        busy_wait(Duration::from_millis(10));

        defmt::info!("ui: display_reinit trigger={} stage=release_lines", trigger);
        if let Err(e) = self.tca_set_res_released(true) {
            self.fail_display_reinit(trigger, "release_lines_res", i2c_error_kind(e));
            return Err(());
        }
        if let Err(e) = self.tca_set_tp_reset_released(true) {
            self.fail_display_reinit(trigger, "release_lines_tp_reset", i2c_error_kind(e));
            return Err(());
        }
        busy_wait(Duration::from_millis(120));
        if let Err(e) = self.tca_set_cs_enabled(true) {
            self.fail_display_reinit(trigger, "release_lines_cs", i2c_error_kind(e));
            return Err(());
        }
        busy_wait(Duration::from_millis(5));

        defmt::info!("ui: display_reinit trigger={} stage=gc9307_init", trigger);
        if self.gc9307_driver_init().is_err() {
            self.fail_display_reinit(trigger, "gc9307_init", "gc9307_init_failed");
            return Err(());
        }

        if self
            .set_display_brightness(
                DISPLAY_BRIGHTNESS_FULL,
                DISPLAY_CTRL_BRIGHTNESS_ON_BACKLIGHT_ON,
            )
            .is_err()
        {
            self.fail_display_reinit(trigger, "brightness_full", "spi_write_failed");
            return Err(());
        }
        self.set_backlight(true);
        self.state = InitState::Ready;
        self.display_power.reset(self.display_power_now_ms());
        self.next_frame_deadline = Instant::now();
        Ok(())
    }

    fn fail_display_reinit(
        &mut self,
        trigger: &'static str,
        stage: &'static str,
        err: &'static str,
    ) {
        defmt::error!(
            "ui: display_reinit trigger={} stage={} result=err err={}",
            trigger,
            stage,
            err
        );
        self.enter_display_fail_safe();
    }

    fn enter_display_fail_safe(&mut self) {
        let _ = self.tca_set_cs_enabled(false);
        let _ = self.tca_set_res_released(false);
        let _ = self.tca_set_tp_reset_released(false);
        self.set_backlight(false);
        self.state = InitState::Disabled;
    }

    fn maybe_trigger_center_long_press(&mut self, snapshot: InputSnapshot) -> bool {
        let now = Instant::now();
        if !snapshot.center {
            self.center_press_started_at = None;
            self.center_long_press_fired = false;
            return false;
        }

        match self.center_press_started_at {
            Some(started_at)
                if !self.center_long_press_fired
                    && now >= started_at + CENTER_LONG_PRESS_THRESHOLD =>
            {
                self.center_long_press_fired = true;
                self.run_center_long_press_diag(snapshot);
                true
            }
            Some(_) => false,
            None => {
                self.center_press_started_at = Some(now);
                self.center_long_press_fired = false;
                false
            }
        }
    }

    fn run_center_long_press_diag(&mut self, snapshot: InputSnapshot) {
        self.log_display_diag(snapshot);
        if self.reinitialize_display_path("center_long_press").is_err() {
            return;
        }

        defmt::info!("ui: display_reinit trigger=center_long_press stage=redraw_restore");
        if let Err(e) = self.render_inputs(snapshot) {
            defmt::error!(
                "ui: display_reinit trigger=center_long_press stage=redraw_restore result=err err={=?}",
                e
            );
            self.enter_display_fail_safe();
            self.needs_redraw = true;
            return;
        }

        self.needs_redraw = false;
        defmt::info!("ui: display_reinit trigger=center_long_press stage=redraw_restore result=ok");
        defmt::info!("ui: display_reinit trigger=center_long_press stage=ok");
    }

    fn log_display_diag(&mut self, snapshot: InputSnapshot) {
        defmt::info!(
            "ui: display_diag trigger=center_long_press page={} route={} overlay={} bms_state={} center={=bool} touch={=bool}",
            variant_name(self.ui_variant),
            dashboard_route_name(self.dashboard_route),
            overlay_name(self.self_check_overlay),
            bms_activation_state_name(self.bms_activation_state),
            snapshot.center,
            snapshot.touch
        );
        esp_println::println!(
            "ui: display_diag trigger=center_long_press page={} route={} overlay={} bms_state={} center={} touch={}",
            variant_name(self.ui_variant),
            dashboard_route_name(self.dashboard_route),
            overlay_name(self.self_check_overlay),
            bms_activation_state_name(self.bms_activation_state),
            snapshot.center,
            snapshot.touch
        );

        match self.read_tca_diag_snapshot() {
            Ok(diag) => {
                let actual = diag.input;
                defmt::info!(
                    "ui: display_diag tca_raw input=0x{=u8:02x} output=0x{=u8:02x} polarity=0x{=u8:02x} config=0x{=u8:02x}",
                    diag.input,
                    diag.output,
                    diag.polarity,
                    diag.config
                );
                defmt::info!(
                    "ui: display_diag tca_state up={=bool} down={=bool} left={=bool} right={=bool} usb2_pg={=bool} cs_enabled={=bool} res_released={=bool} tp_reset_released={=bool}",
                    (actual & (1 << 3)) == 0,
                    (actual & (1 << 0)) == 0,
                    (actual & (1 << 1)) == 0,
                    (actual & (1 << 2)) == 0,
                    (actual & (1 << 4)) != 0,
                    (diag.output & (1 << TCA_BIT_CS)) == 0,
                    (diag.output & (1 << TCA_BIT_RES)) != 0,
                    (diag.output & (1 << TCA_BIT_TP_RESET)) != 0
                );
                esp_println::println!(
                    "ui: display_diag tca_raw input=0x{:02x} output=0x{:02x} polarity=0x{:02x} config=0x{:02x}",
                    diag.input,
                    diag.output,
                    diag.polarity,
                    diag.config
                );
                esp_println::println!(
                    "ui: display_diag tca_state up={} down={} left={} right={} usb2_pg={} cs_enabled={} res_released={} tp_reset_released={}",
                    (actual & (1 << 3)) == 0,
                    (actual & (1 << 0)) == 0,
                    (actual & (1 << 1)) == 0,
                    (actual & (1 << 2)) == 0,
                    (actual & (1 << 4)) != 0,
                    (diag.output & (1 << TCA_BIT_CS)) == 0,
                    (diag.output & (1 << TCA_BIT_RES)) != 0,
                    (diag.output & (1 << TCA_BIT_TP_RESET)) != 0
                );
            }
            Err(e) => {
                defmt::error!("ui: display_diag tca_read err={}", i2c_error_kind(e));
                esp_println::println!("ui: display_diag tca_read err={}", i2c_error_kind(e));
            }
        }

        let tca_reset_high = self.tca_reset_n.is_high();
        let dc_high = self.panel_io.dc.is_high();
        let bl_high = self.backlight.raw_level_high();
        let bl_pwm_pct = self.backlight.brightness_pct();
        let center_low = self.btn_center.is_low();
        let ctp_irq_low = self.ctp_irq.is_low();
        defmt::info!(
            "ui: display_diag gpio gpio1_tca_reset_high={=bool} gpio10_dc_high={=bool} gpio13_blk_high={=?} gpio13_blk_pwm_pct={=u8} gpio0_center_low={=bool} gpio14_ctp_irq_low={=bool} backlight_on={=bool}",
            tca_reset_high,
            dc_high,
            bl_high,
            bl_pwm_pct,
            center_low,
            ctp_irq_low,
            self.backlight.is_on()
        );
        esp_println::println!(
            "ui: display_diag gpio gpio1_tca_reset_high={} gpio10_dc_high={} gpio13_blk_high={:?} gpio13_blk_pwm_pct={} gpio0_center_low={} gpio14_ctp_irq_low={} backlight_on={}",
            tca_reset_high,
            dc_high,
            bl_high,
            bl_pwm_pct,
            center_low,
            ctp_irq_low,
            self.backlight.is_on()
        );

        match self.read_touch_diag_snapshot() {
            Ok(diag) => {
                let (mapped_x, mapped_y) = match diag.mapped_point {
                    Some((x, y)) => (Some(x), Some(y)),
                    None => (None, None),
                };
                defmt::info!(
                    "ui: display_diag cst816d probe=ok gesture=0x{=u8:02x} fingers={=u8} raw_x={=u16} raw_y={=u16} mapped_x={=?} mapped_y={=?}",
                    diag.gesture,
                    diag.finger_count,
                    diag.raw_x,
                    diag.raw_y,
                    mapped_x,
                    mapped_y
                );
                let (mapped_present, mapped_x, mapped_y) = match diag.mapped_point {
                    Some((x, y)) => (true, x, y),
                    None => (false, 0, 0),
                };
                esp_println::println!(
                    "ui: display_diag cst816d probe=ok gesture=0x{:02x} fingers={} raw_x={} raw_y={} mapped_present={} mapped_x={} mapped_y={}",
                    diag.gesture,
                    diag.finger_count,
                    diag.raw_x,
                    diag.raw_y,
                    mapped_present,
                    mapped_x,
                    mapped_y
                );
            }
            Err(e) => {
                defmt::error!(
                    "ui: display_diag cst816d probe=err err={}",
                    i2c_error_kind(e)
                );
                esp_println::println!(
                    "ui: display_diag cst816d probe=err err={}",
                    i2c_error_kind(e)
                );
            }
        }
    }

    fn read_tca_diag_snapshot(&mut self) -> Result<TcaDiagSnapshot, esp_hal::i2c::master::Error> {
        Ok(TcaDiagSnapshot {
            input: self.read_tca_reg(TCA_REG_INPUT)?,
            output: self.read_tca_reg(TCA_REG_OUTPUT)?,
            polarity: self.read_tca_reg(TCA_REG_POLARITY)?,
            config: self.read_tca_reg(TCA_REG_CONFIG)?,
        })
    }

    fn read_tca_reg(&mut self, reg: u8) -> Result<u8, esp_hal::i2c::master::Error> {
        let mut buf = [0u8; 1];
        self.i2c.write_read(TCA6408A_ADDR, &[reg], &mut buf)?;
        Ok(buf[0])
    }

    fn read_touch_diag_snapshot(
        &mut self,
    ) -> Result<Cst816dDiagSnapshot, esp_hal::i2c::master::Error> {
        let mut buf = [0u8; CST816D_TOUCH_REG_LEN];
        self.i2c
            .write_read(CST816D_ADDR, &[CST816D_REG_GESTURE], &mut buf)?;
        let raw_x = (((buf[2] & 0x0f) as u16) << 8) | buf[3] as u16;
        let raw_y = (((buf[4] & 0x0f) as u16) << 8) | buf[5] as u16;
        Ok(Cst816dDiagSnapshot {
            gesture: buf[0],
            finger_count: buf[1] & 0x0f,
            raw_x,
            raw_y,
            mapped_point: Self::map_touch_to_ui(raw_x, raw_y),
        })
    }

    pub fn set_attention_hold(&mut self, hold: bool) {
        if self.attention_hold == hold {
            return;
        }
        self.attention_hold = hold;
        defmt::info!("ui: display attention_hold={=bool}", hold);
    }

    fn display_power_now_ms(&self) -> u64 {
        self.display_power_epoch.elapsed().as_millis() as u64
    }

    fn display_accepts_scene_updates(&self) -> bool {
        self.state == InitState::Ready
            && matches!(
                self.display_power.mode(),
                DisplayPowerMode::Awake | DisplayPowerMode::Dimmed
            )
    }

    fn apply_display_power_command(
        &mut self,
        previous_mode: DisplayPowerMode,
        command: DisplayPowerCommand,
    ) -> Result<(), esp_hal::spi::Error> {
        match command {
            DisplayPowerCommand::None => {}
            DisplayPowerCommand::FullBrightness => {
                if previous_mode == DisplayPowerMode::Sleeping {
                    self.panel_io.wake_display()?;
                }
                self.set_display_brightness(
                    DISPLAY_BRIGHTNESS_FULL,
                    DISPLAY_CTRL_BRIGHTNESS_ON_BACKLIGHT_ON,
                )?;
                self.set_backlight_pct(BACKLIGHT_PWM_FULL_PCT);
                self.needs_redraw = true;
                defmt::info!("ui: display_power mode=awake");
            }
            DisplayPowerCommand::Dim => {
                self.set_display_brightness(
                    DISPLAY_BRIGHTNESS_DIM,
                    DISPLAY_CTRL_BRIGHTNESS_DIM_BACKLIGHT_ON,
                )?;
                self.set_backlight_pct(BACKLIGHT_PWM_DIM_PCT);
                defmt::info!(
                    "ui: display_power mode=dimmed brightness={=u8} ctrl=0x{=u8:x} backlight_pwm_pct={=u8}",
                    DISPLAY_BRIGHTNESS_DIM,
                    DISPLAY_CTRL_BRIGHTNESS_DIM_BACKLIGHT_ON,
                    BACKLIGHT_PWM_DIM_PCT
                );
            }
            DisplayPowerCommand::BacklightOff => {
                self.set_backlight_pct(0);
                self.needs_redraw = true;
                defmt::info!("ui: display_power mode=backlight_off");
            }
            DisplayPowerCommand::Sleep => {
                self.set_backlight_pct(0);
                self.panel_io.sleep_display()?;
                self.needs_redraw = true;
                defmt::info!("ui: display_power mode=sleeping");
            }
        }
        Ok(())
    }

    fn set_display_brightness(&mut self, value: u8, ctrl: u8) -> Result<(), esp_hal::spi::Error> {
        self.panel_io.write_cmd(CMD_WRITE_CTRL_DISPLAY)?;
        self.panel_io.write_data(&[ctrl])?;
        self.panel_io.write_cmd(CMD_WRITE_DISPLAY_BRIGHTNESS)?;
        self.panel_io.write_data(&[value])
    }

    pub fn update_self_check_snapshot(&mut self, snapshot: SelfCheckUiSnapshot) {
        let previous = self.self_check_snapshot;
        if previous == snapshot {
            return;
        }
        log_self_check_snapshot_transition(&previous, &snapshot);
        self.self_check_snapshot = snapshot;
        if matches!(
            self.self_check_overlay,
            SelfCheckOverlay::BmsActivateConfirm | SelfCheckOverlay::BmsDischargeAuthorizeConfirm
        ) && front_panel_scene::bq40_recovery_overlay(&self.self_check_snapshot).is_none()
        {
            defmt::info!(
                "ui: bms recovery dialog auto_close reason=recovery_not_needed bq40_state={} last_result={}",
                self_check_comm_state_name(self.self_check_snapshot.bq40z50),
                bms_result_option_name(self.self_check_snapshot.bq40z50_last_result)
            );
            self.self_check_overlay = SelfCheckOverlay::None;
        }
        self.needs_redraw = true;
        if self.ui_variant != SELF_CHECK_VARIANT {
            return;
        }
        if !self.display_accepts_scene_updates() {
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
        let recovery_overlay = front_panel_scene::bq40_recovery_overlay(&self.self_check_snapshot);
        let overlay_allowed = self.ui_variant == SELF_CHECK_VARIANT
            && (recovery_overlay.is_some()
                || matches!(
                    self.self_check_overlay,
                    SelfCheckOverlay::BmsActivateConfirm
                        | SelfCheckOverlay::BmsActivateProgress
                        | SelfCheckOverlay::BmsDischargeAuthorizeConfirm
                        | SelfCheckOverlay::BmsDischargeAuthorizeProgress
                        | SelfCheckOverlay::BmsActivateResult(..)
                ));
        self.self_check_overlay = if overlay_allowed {
            match state {
                BmsActivationState::Idle => SelfCheckOverlay::None,
                BmsActivationState::Pending => {
                    match current_recovery_overlay_action(self.self_check_overlay, recovery_overlay)
                    {
                        Some(BmsRecoveryUiAction::Activation) => {
                            SelfCheckOverlay::BmsActivateProgress
                        }
                        Some(BmsRecoveryUiAction::DischargeAuthorization) => {
                            SelfCheckOverlay::BmsDischargeAuthorizeProgress
                        }
                        None => SelfCheckOverlay::None,
                    }
                }
                BmsActivationState::Result(BmsResultKind::Success) => SelfCheckOverlay::None,
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

    fn dashboard_shell_state(&self) -> DashboardShellState {
        DashboardShellState {
            page: self.dashboard_page,
            dashboard_route: self.dashboard_route,
            home_focus: self.dashboard_home_focus,
            menu_selected: self.dashboard_menu_selected,
            menu_style: DashboardMenuStyle::default_preview(),
            beeper_prefs: self.beeper_prefs,
            dashboard_menu_offset_y: self.dashboard_menu_offset_y,
        }
    }

    fn set_dashboard_page(&mut self, page: DashboardPrimaryPage) {
        if self.dashboard_page == page {
            return;
        }
        let previous = self.dashboard_page;
        let target_offset_y = dashboard_menu_target_offset_y(page);
        self.dashboard_page = page;
        if dashboard_page_transition_is_animated(previous, page) {
            self.dashboard_menu_animation = Some(DashboardMenuAnimation {
                from_offset_y: self.dashboard_menu_offset_y,
                target_offset_y,
                step_index: 0,
            });
        } else {
            self.dashboard_menu_animation = None;
            self.dashboard_menu_offset_y = target_offset_y;
        }
        self.needs_redraw = true;
        defmt::info!(
            "ui: dashboard page old={} new={}",
            dashboard_page_name(previous),
            dashboard_page_name(page)
        );
        esp_println::println!(
            "ui: dashboard page old={} new={}",
            dashboard_page_name(previous),
            dashboard_page_name(page)
        );
    }

    fn set_dashboard_route(&mut self, route: DashboardRoute, source: &'static str) {
        if self.dashboard_route == route {
            return;
        }
        let previous = self.dashboard_route;
        self.dashboard_route = route;
        self.needs_redraw = true;
        defmt::info!(
            "ui: dashboard route old={} new={} source={}",
            dashboard_route_name(previous),
            dashboard_route_name(route),
            source
        );
        esp_println::println!(
            "ui: dashboard route old={} new={} source={}",
            dashboard_route_name(previous),
            dashboard_route_name(route),
            source
        );
    }

    fn set_dashboard_home_focus(&mut self, focus: DashboardHomeFocus) {
        if self.dashboard_home_focus == focus {
            return;
        }
        let previous = self.dashboard_home_focus;
        self.dashboard_home_focus = focus;
        self.needs_redraw = true;
        defmt::info!(
            "ui: dashboard focus old={} new={}",
            dashboard_home_focus_name(previous),
            dashboard_home_focus_name(focus)
        );
        esp_println::println!(
            "ui: dashboard focus old={} new={}",
            dashboard_home_focus_name(previous),
            dashboard_home_focus_name(focus)
        );
    }

    fn set_dashboard_menu_selected(&mut self, item: MenuItem) {
        if self.dashboard_menu_selected == item {
            return;
        }
        let previous = self.dashboard_menu_selected;
        self.dashboard_menu_selected = item;
        self.needs_redraw = true;
        defmt::info!(
            "ui: menu item old={} new={}",
            menu_item_name(previous),
            menu_item_name(item)
        );
        esp_println::println!(
            "ui: menu item old={} new={}",
            menu_item_name(previous),
            menu_item_name(item)
        );
    }

    pub fn enter_dashboard(&mut self) {
        if !dashboard_enter_requires_variant_switch(self.ui_variant) {
            return;
        }

        let previous_variant = self.ui_variant;
        self.ui_variant = DASHBOARD_VARIANT;
        self.dashboard_page = DashboardPrimaryPage::DashboardHome;
        self.dashboard_route = DashboardRoute::Home;
        self.dashboard_home_focus = DashboardHomeFocus::Output;
        self.dashboard_menu_selected = MenuItem::Dashboard;
        self.dashboard_menu_offset_y = 0;
        self.dashboard_menu_animation = None;
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

        if !self.display_accepts_scene_updates() {
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

    pub fn set_beeper_prefs(&mut self, prefs: BeeperPrefs) {
        if self.beeper_prefs == prefs {
            return;
        }
        self.beeper_prefs = prefs;
        self.needs_redraw = true;
    }

    fn update_dashboard_menu_animation(&mut self) -> bool {
        let Some(animation) = self.dashboard_menu_animation.as_mut() else {
            return false;
        };
        animation.step_index = animation
            .step_index
            .saturating_add(1)
            .min(DASHBOARD_MENU_ANIMATION_STEPS);
        let from_offset_y = animation.from_offset_y;
        let target_offset_y = animation.target_offset_y;
        let step_index = animation.step_index;
        let delta = i32::from(target_offset_y - from_offset_y);
        let next_offset = i32::from(from_offset_y)
            + (delta * i32::from(step_index) / i32::from(DASHBOARD_MENU_ANIMATION_STEPS.max(1)));
        self.dashboard_menu_offset_y = next_offset as i16;
        if step_index >= DASHBOARD_MENU_ANIMATION_STEPS {
            self.dashboard_menu_offset_y = target_offset_y;
            self.dashboard_menu_animation = None;
        }
        true
    }

    pub fn tick(&mut self) -> Option<UiAction> {
        if self.state != InitState::Ready {
            return None;
        }

        let now = Instant::now();
        if now < self.next_frame_deadline {
            return None;
        }
        let frame_interval = if self.dashboard_menu_animation.is_some() {
            DASHBOARD_MENU_ANIMATION_FRAME_INTERVAL
        } else {
            FRAME_INTERVAL
        };
        self.next_frame_deadline = now + frame_interval;

        let mut ui_action = None;
        match self.read_inputs() {
            Ok(snapshot) => {
                let previous_power_mode = self.display_power.mode();
                let power_command = self.display_power.step(
                    self.display_power_now_ms(),
                    input_snapshot_has_activity(snapshot),
                    self.attention_hold,
                );
                if let Err(e) = self.apply_display_power_command(previous_power_mode, power_command)
                {
                    defmt::error!("ui: display_power apply failed err={=?}", e);
                    self.needs_redraw = true;
                }

                if power_command == DisplayPowerCommand::FullBrightness
                    && previous_power_mode != DisplayPowerMode::Awake
                {
                    if self.display_accepts_scene_updates() {
                        if let Err(e) = self.render_inputs(snapshot) {
                            defmt::error!("ui: wake redraw failed err={=?}", e);
                            self.needs_redraw = true;
                        } else {
                            self.needs_redraw = false;
                        }
                    }
                    self.last_inputs = Some(snapshot);
                    return None;
                }

                if !self.display_accepts_scene_updates() {
                    self.last_inputs = Some(snapshot);
                    return None;
                }

                if self.ui_variant == SELF_CHECK_VARIANT {
                    ui_action = self.process_bms_activation_button_action(snapshot);
                    if ui_action.is_none() {
                        ui_action = self.process_touch_action(snapshot);
                    }
                } else {
                    ui_action = self.process_dashboard_button_action(snapshot);
                    if ui_action.is_none() {
                        ui_action = self.process_dashboard_gesture_action(snapshot);
                    }
                    if ui_action.is_none() {
                        ui_action = self.process_dashboard_touch_action(snapshot);
                    }
                }
                if self.maybe_trigger_center_long_press(snapshot) {
                    self.last_inputs = Some(snapshot);
                    return ui_action;
                }
                let inputs_changed = self.last_inputs != Some(snapshot);
                if self.dashboard_menu_animation.is_some() {
                    self.next_frame_deadline = now + DASHBOARD_MENU_ANIMATION_FRAME_INTERVAL;
                }
                let menu_animation_active = self.update_dashboard_menu_animation();
                let should_render = self.needs_redraw
                    || menu_animation_active
                    || (self.ui_variant == SELF_CHECK_VARIANT && inputs_changed)
                    || dashboard_uses_frame_animation(
                        self.ui_variant,
                        self.dashboard_route,
                        &self.self_check_snapshot,
                    ) && self.dashboard_page == DashboardPrimaryPage::DashboardHome;
                if should_render {
                    if let Err(e) = self.render_inputs(snapshot) {
                        defmt::error!("ui: update input state failed err={=?}", e);
                        self.needs_redraw = true;
                    } else {
                        self.needs_redraw = false;
                    }
                }
                self.last_inputs = Some(snapshot);
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
            SelfCheckOverlay::BmsActivateResult(..) | SelfCheckOverlay::HardwareIssue(..)
        ) {
            if left_edge || right_edge || center_edge {
                let was_result = matches!(
                    self.self_check_overlay,
                    SelfCheckOverlay::BmsActivateResult(..)
                );
                self.self_check_overlay = SelfCheckOverlay::None;
                self.needs_redraw = true;
                defmt::info!("ui: self-check dialog close via key");
                if was_result {
                    return Some(UiAction::ClearBmsActivationResult);
                }
            }
            return None;
        }

        match self.self_check_overlay {
            SelfCheckOverlay::None => {
                if (left_edge || center_edge)
                    && self.bms_activation_state != BmsActivationState::Pending
                {
                    if let Some(recovery_overlay) =
                        front_panel_scene::bq40_recovery_overlay(&self.self_check_snapshot)
                    {
                        self.self_check_overlay = recovery_overlay;
                        self.needs_redraw = true;
                        defmt::info!("ui: bms recovery dialog open via key");
                    } else if let Some(result_overlay) =
                        front_panel_scene::bq40_result_overlay(&self.self_check_snapshot)
                    {
                        self.self_check_overlay = result_overlay;
                        self.needs_redraw = true;
                        defmt::info!("ui: bms result dialog reopen via key");
                    }
                }
            }
            SelfCheckOverlay::BmsActivateConfirm
            | SelfCheckOverlay::BmsDischargeAuthorizeConfirm => {
                if right_edge {
                    self.self_check_overlay = SelfCheckOverlay::None;
                    self.needs_redraw = true;
                    defmt::info!("ui: bms recovery dialog cancel via key");
                } else if left_edge || center_edge {
                    let action = match self.self_check_overlay {
                        SelfCheckOverlay::BmsActivateConfirm => BmsRecoveryUiAction::Activation,
                        SelfCheckOverlay::BmsDischargeAuthorizeConfirm => {
                            BmsRecoveryUiAction::DischargeAuthorization
                        }
                        _ => unreachable!(),
                    };
                    self.self_check_overlay = match action {
                        BmsRecoveryUiAction::Activation => SelfCheckOverlay::BmsActivateProgress,
                        BmsRecoveryUiAction::DischargeAuthorization => {
                            SelfCheckOverlay::BmsDischargeAuthorizeProgress
                        }
                    };
                    self.needs_redraw = true;
                    defmt::info!(
                        "ui: bms recovery request via key action={}",
                        bms_recovery_ui_action_name(action)
                    );
                    return Some(UiAction::RequestBmsRecovery(action));
                }
            }
            SelfCheckOverlay::BmsActivateProgress
            | SelfCheckOverlay::BmsDischargeAuthorizeProgress => {}
            SelfCheckOverlay::BmsActivateResult(..) | SelfCheckOverlay::HardwareIssue(..) => {}
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
    pub fn render_tps_test_status(&mut self, snapshot: TpsTestUiSnapshot) {
        if self.state != InitState::Ready {
            return;
        }

        let model = UiModel {
            mode: UpsMode::Standby,
            focus: UiFocus::Idle,
            touch_irq: false,
            frame_no: self.frame_no,
        };
        if let Err(e) = self.render_scene(|painter| {
            front_panel_scene::render_tps_test_status(painter, &model, DASHBOARD_VARIANT, &snapshot)
        }) {
            defmt::error!("ui: render tps test status failed err={=?}", e);
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
        self.backlight.configure();
        self.set_backlight_pct(0);
    }

    fn configure_tca_reset(&mut self) {
        self.tca_reset_n.apply_output_config(
            &OutputConfig::default()
                .with_drive_mode(DriveMode::PushPull)
                .with_pull(Pull::Up),
        );
        self.tca_reset_n.set_input_enable(true);
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
        self.set_backlight_pct(if on { BACKLIGHT_PWM_FULL_PCT } else { 0 });
    }

    fn set_backlight_pct(&mut self, pct: u8) {
        self.backlight.set_brightness_pct(pct);
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
        // Current board wiring swaps both vertical and horizontal pairs against silk-screen labels.
        let up = (bits & (1 << 3)) == 0;
        let left = (bits & (1 << 2)) == 0;
        let right = (bits & (1 << 1)) == 0;
        let down = (bits & (1 << 0)) == 0;

        let center = self.btn_center.is_low();
        let touch_irq_active = self.ctp_irq.is_low();
        let touch_sample = self.read_touch_sample();
        let touch_point = touch_sample.point;
        let touch = touch_point.is_some();

        if touch_irq_active && touch_point.is_none() && touch_sample.gesture_raw == 0 {
            if !self.touch_irq_stuck_hint_logged {
                defmt::warn!(
                    "ui: ctp_irq active without coordinates; ignore irq-only touch to avoid stuck edge"
                );
                esp_println::println!(
                    "ui: ctp_irq active_without_coordinates action=ignore_irq_only_touch"
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
            touch_gesture_raw: touch_sample.gesture_raw,
        })
    }

    fn read_touch_sample(&mut self) -> TouchSample {
        let mut buf = [0u8; CST816D_TOUCH_REG_LEN];
        if self
            .i2c
            .write_read(CST816D_ADDR, &[CST816D_REG_GESTURE], &mut buf)
            .is_err()
        {
            return TouchSample {
                gesture_raw: 0,
                point: None,
            };
        }

        let gesture_raw = buf[0];
        let finger_count = buf[1] & 0x0f;
        if finger_count == 0 {
            return TouchSample {
                gesture_raw,
                point: None,
            };
        }

        let x_raw = (((buf[2] & 0x0f) as u16) << 8) | buf[3] as u16;
        let y_raw = (((buf[4] & 0x0f) as u16) << 8) | buf[5] as u16;
        TouchSample {
            gesture_raw,
            point: Self::map_touch_to_ui(x_raw, y_raw),
        }
    }

    fn map_touch_to_ui(x_raw: u16, y_raw: u16) -> Option<(u16, u16)> {
        let ui_w = front_panel_scene::UI_W;
        let ui_h = front_panel_scene::UI_H;

        // CST816D on this board reports in a portrait-like space (x=0..UI_H, y=0..UI_W).
        if x_raw < ui_h && y_raw < ui_w {
            return match PANEL_ORIENTATION {
                Orientation::Landscape => {
                    Some((y_raw, ui_h.saturating_sub(1).saturating_sub(x_raw)))
                }
                Orientation::LandscapeSwapped => {
                    Some((ui_w.saturating_sub(1).saturating_sub(y_raw), x_raw))
                }
                Orientation::Portrait => Some((x_raw, y_raw)),
                Orientation::PortraitSwapped => Some((
                    ui_w.saturating_sub(1).saturating_sub(x_raw),
                    ui_h.saturating_sub(1).saturating_sub(y_raw),
                )),
            };
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
            SelfCheckOverlay::BmsActivateResult(..) | SelfCheckOverlay::HardwareIssue(..)
        ) {
            let was_result = matches!(
                self.self_check_overlay,
                SelfCheckOverlay::BmsActivateResult(..)
            );
            self.self_check_overlay = SelfCheckOverlay::None;
            self.needs_redraw = true;
            defmt::info!("ui: self-check dialog close via touch");
            esp_println::println!("ui: self-check dialog close via touch");
            return was_result.then_some(UiAction::ClearBmsActivationResult);
        }

        let (x, y) = snapshot.touch_point?;

        defmt::info!(
            "ui: touch edge x={=u16} y={=u16} overlay={}",
            x,
            y,
            overlay_name(self.self_check_overlay)
        );
        esp_println::println!(
            "ui: touch edge page=self_check x={} y={} overlay={}",
            x,
            y,
            overlay_name(self.self_check_overlay)
        );

        match front_panel_scene::self_check_hit_test(x, y, self.self_check_overlay) {
            Some(SelfCheckTouchTarget::ActivateCancel) => {
                self.self_check_overlay = SelfCheckOverlay::None;
                self.needs_redraw = true;
                esp_println::println!("ui: touch target=activate_cancel action=close_dialog");
                None
            }
            Some(SelfCheckTouchTarget::ActivateConfirm) => {
                let action = match self.self_check_overlay {
                    SelfCheckOverlay::BmsActivateConfirm => BmsRecoveryUiAction::Activation,
                    SelfCheckOverlay::BmsDischargeAuthorizeConfirm => {
                        BmsRecoveryUiAction::DischargeAuthorization
                    }
                    _ => return None,
                };
                self.self_check_overlay = match action {
                    BmsRecoveryUiAction::Activation => SelfCheckOverlay::BmsActivateProgress,
                    BmsRecoveryUiAction::DischargeAuthorization => {
                        SelfCheckOverlay::BmsDischargeAuthorizeProgress
                    }
                };
                self.needs_redraw = true;
                esp_println::println!(
                    "ui: touch target=activate_confirm action={}",
                    bms_recovery_ui_action_name(action)
                );
                Some(UiAction::RequestBmsRecovery(action))
            }
            Some(SelfCheckTouchTarget::HardwareCard(target)) => {
                esp_println::println!(
                    "ui: touch target=hardware_card card={}",
                    self_check_hardware_target_name(target)
                );
                self.open_self_check_hardware_overlay(target);
                None
            }
            None => {
                defmt::info!(
                    "ui: touch target none x={=u16} y={=u16} overlay={}",
                    x,
                    y,
                    overlay_name(self.self_check_overlay)
                );
                esp_println::println!(
                    "ui: touch target=none page=self_check x={} y={} overlay={}",
                    x,
                    y,
                    overlay_name(self.self_check_overlay)
                );
                None
            }
        }
    }

    fn open_self_check_hardware_overlay(&mut self, target: SelfCheckHardwareTarget) {
        if self.self_check_overlay != SelfCheckOverlay::None {
            defmt::info!(
                "ui: self-check hardware touch ignored overlay={}",
                overlay_name(self.self_check_overlay)
            );
            return;
        }

        let recovery_overlay = if target == SelfCheckHardwareTarget::Bq40z50
            && self.bms_activation_state != BmsActivationState::Pending
        {
            front_panel_scene::bq40_recovery_overlay(&self.self_check_snapshot)
        } else {
            None
        };
        if let Some(recovery_overlay) = recovery_overlay {
            self.self_check_overlay = recovery_overlay;
            self.needs_redraw = true;
            defmt::info!("ui: bms recovery dialog open via touch");
            esp_println::println!(
                "ui: bms recovery dialog open via touch overlay={}",
                overlay_name(self.self_check_overlay)
            );
            return;
        }

        if target == SelfCheckHardwareTarget::Bq40z50 {
            if let Some(result_overlay) =
                front_panel_scene::bq40_result_overlay(&self.self_check_snapshot)
            {
                self.self_check_overlay = result_overlay;
                self.needs_redraw = true;
                defmt::info!("ui: bms result dialog reopen via touch");
                esp_println::println!(
                    "ui: bms result dialog reopen via touch overlay={}",
                    overlay_name(self.self_check_overlay)
                );
                return;
            }
        }

        if let Some(issue_overlay) =
            front_panel_scene::self_check_hardware_issue_overlay(&self.self_check_snapshot, target)
        {
            self.self_check_overlay = issue_overlay;
            self.needs_redraw = true;
            defmt::info!(
                "ui: self-check hardware issue open target={}",
                self_check_hardware_target_name(target)
            );
            esp_println::println!(
                "ui: self-check hardware issue open target={}",
                self_check_hardware_target_name(target)
            );
        } else {
            defmt::info!(
                "ui: self-check hardware touch ignored target={} reason=no_issue",
                self_check_hardware_target_name(target)
            );
            esp_println::println!(
                "ui: self-check hardware touch ignored target={} reason=no_issue",
                self_check_hardware_target_name(target)
            );
        }
    }

    fn process_dashboard_button_action(&mut self, snapshot: InputSnapshot) -> Option<UiAction> {
        let prev = self.last_inputs.unwrap_or_else(InputSnapshot::idle);
        let up_edge = snapshot.up && !prev.up;
        let down_edge = snapshot.down && !prev.down;
        let left_edge = snapshot.left && !prev.left;
        let right_edge = snapshot.right && !prev.right;
        let center_edge = snapshot.center && !prev.center;

        if up_edge || down_edge || left_edge || right_edge || center_edge {
            defmt::info!(
                "ui: dashboard key page={} route={} left={} right={} up={} down={} center={}",
                dashboard_page_name(self.dashboard_page),
                dashboard_route_name(self.dashboard_route),
                left_edge,
                right_edge,
                up_edge,
                down_edge,
                center_edge
            );
            esp_println::println!(
                "ui: dashboard key page={} route={} left={} right={} up={} down={} center={}",
                dashboard_page_name(self.dashboard_page),
                dashboard_route_name(self.dashboard_route),
                left_edge,
                right_edge,
                up_edge,
                down_edge,
                center_edge
            );
        }

        match self.dashboard_page {
            DashboardPrimaryPage::Menu => {
                if up_edge {
                    self.set_dashboard_page(DashboardPrimaryPage::DashboardHome);
                } else if left_edge {
                    self.set_dashboard_menu_selected(self.dashboard_menu_selected.previous());
                } else if right_edge {
                    self.set_dashboard_menu_selected(self.dashboard_menu_selected.next());
                } else if center_edge {
                    match self.dashboard_menu_selected {
                        MenuItem::Dashboard => {
                            self.set_dashboard_page(DashboardPrimaryPage::DashboardHome);
                        }
                        MenuItem::Beeper => {
                            self.set_dashboard_page(DashboardPrimaryPage::BeeperSettings);
                        }
                    }
                }
                return None;
            }
            DashboardPrimaryPage::BeeperSettings => {
                if center_edge {
                    self.set_dashboard_page(DashboardPrimaryPage::Menu);
                    return None;
                }

                let mut next_prefs = self.beeper_prefs;
                let mut preview_target = None;
                if up_edge {
                    next_prefs = next_prefs.with_selected_target(BeeperSettingTarget::Action);
                } else if down_edge {
                    next_prefs = next_prefs.with_selected_target(BeeperSettingTarget::System);
                } else if left_edge {
                    let level = next_prefs.selected_volume().decrease();
                    preview_target = Some(next_prefs.selected_target);
                    next_prefs = next_prefs.with_selected_volume(level);
                } else if right_edge {
                    let level = next_prefs.selected_volume().increase();
                    preview_target = Some(next_prefs.selected_target);
                    next_prefs = next_prefs.with_selected_volume(level);
                }

                let prefs_changed = next_prefs != self.beeper_prefs;
                if prefs_changed {
                    let previous = self.beeper_prefs;
                    self.beeper_prefs = next_prefs;
                    self.needs_redraw = true;
                    defmt::info!(
                        "ui: beeper prefs action {}->{} system {}->{} selected {}->{}",
                        previous.action_volume.badge_label(),
                        next_prefs.action_volume.badge_label(),
                        previous.system_volume.badge_label(),
                        next_prefs.system_volume.badge_label(),
                        beeper_target_name(previous.selected_target),
                        beeper_target_name(next_prefs.selected_target)
                    );
                    esp_println::println!(
                        "ui: beeper prefs action {}->{} system {}->{} selected {}->{}",
                        previous.action_volume.badge_label(),
                        next_prefs.action_volume.badge_label(),
                        previous.system_volume.badge_label(),
                        next_prefs.system_volume.badge_label(),
                        beeper_target_name(previous.selected_target),
                        beeper_target_name(next_prefs.selected_target)
                    );
                }

                if let Some(target) = preview_target {
                    defmt::info!(
                        "ui: beeper preview target={} action={} system={}",
                        beeper_target_name(target),
                        self.beeper_prefs.action_volume.badge_label(),
                        self.beeper_prefs.system_volume.badge_label()
                    );
                    esp_println::println!(
                        "ui: beeper preview target={} action={} system={}",
                        beeper_target_name(target),
                        self.beeper_prefs.action_volume.badge_label(),
                        self.beeper_prefs.system_volume.badge_label()
                    );
                    return Some(UiAction::BeeperPreview {
                        prefs: self.beeper_prefs,
                        target,
                    });
                }
                if prefs_changed {
                    return Some(UiAction::BeeperPrefsChanged {
                        prefs: self.beeper_prefs,
                    });
                }
                return None;
            }
            DashboardPrimaryPage::DashboardHome => {}
        }

        if self.dashboard_route == DashboardRoute::Home {
            if up_edge {
                self.set_dashboard_home_focus(self.dashboard_home_focus.up());
            } else if left_edge {
                self.set_dashboard_home_focus(self.dashboard_home_focus.left());
            } else if right_edge {
                self.set_dashboard_home_focus(self.dashboard_home_focus.right());
            } else if down_edge {
                let next_focus = self.dashboard_home_focus.down();
                if next_focus != self.dashboard_home_focus {
                    self.set_dashboard_home_focus(next_focus);
                } else {
                    self.set_dashboard_page(DashboardPrimaryPage::Menu);
                }
            } else if center_edge {
                self.set_dashboard_route(
                    front_panel_scene::dashboard_route_for_home_focus(self.dashboard_home_focus),
                    "key",
                );
            }
            return None;
        }

        if left_edge || center_edge {
            let next_route = match self.dashboard_route {
                DashboardRoute::Detail(front_panel_scene::DashboardDetailPage::BmsDetail) => Some(
                    DashboardRoute::Detail(front_panel_scene::DashboardDetailPage::Cells),
                ),
                DashboardRoute::Detail(_) => Some(DashboardRoute::Home),
                DashboardRoute::ManualCharge => Some(DashboardRoute::Detail(
                    front_panel_scene::DashboardDetailPage::Charger,
                )),
                DashboardRoute::Home => None,
            };
            if let Some(next_route) = next_route {
                self.set_dashboard_route(next_route, "key");
            }
        }

        None
    }

    fn process_dashboard_gesture_action(&mut self, snapshot: InputSnapshot) -> Option<UiAction> {
        let prev = self.last_inputs.unwrap_or_else(InputSnapshot::idle);
        if !snapshot.touch {
            self.dashboard_touch_gesture_consumed = false;
        }

        let raw_gesture_direction = if snapshot.touch_gesture_raw != 0
            && snapshot.touch_gesture_raw != prev.touch_gesture_raw
        {
            cst816d_vertical_gesture_direction(snapshot.touch_gesture_raw)
        } else {
            None
        };
        let drag_delta = if !self.dashboard_touch_gesture_consumed && snapshot.touch && prev.touch {
            match (prev.touch_point, snapshot.touch_point) {
                (Some((_, prev_y)), Some((_, y))) => {
                    let dy = y as i16 - prev_y as i16;
                    if dy.unsigned_abs() >= DASHBOARD_MENU_DRAG_THRESHOLD_PX as u16 {
                        Some(dy)
                    } else {
                        None
                    }
                }
                _ => None,
            }
        } else {
            None
        };

        let drag_gesture_direction = drag_delta.map(|dy| {
            if dy < 0 {
                VerticalGestureDirection::Up
            } else {
                VerticalGestureDirection::Down
            }
        });
        let Some(gesture_direction) = drag_gesture_direction.or(raw_gesture_direction) else {
            return None;
        };
        if drag_delta.is_some() {
            self.dashboard_touch_gesture_consumed = true;
        }

        let Some(next_page) =
            dashboard_page_for_vertical_menu_gesture(self.dashboard_page, gesture_direction)
        else {
            return None;
        };
        let previous_page = self.dashboard_page;
        self.set_dashboard_page(next_page);
        defmt::info!(
            "ui: dashboard menu gesture page={} new={} direction={} raw=0x{=u8:02x} drag_dy={=i16}",
            dashboard_page_name(previous_page),
            dashboard_page_name(next_page),
            vertical_gesture_direction_name(gesture_direction),
            snapshot.touch_gesture_raw,
            drag_delta.unwrap_or(0)
        );
        esp_println::println!(
            "ui: dashboard menu gesture page={} new={} direction={} raw=0x{:02x} drag_dy={}",
            dashboard_page_name(previous_page),
            dashboard_page_name(next_page),
            vertical_gesture_direction_name(gesture_direction),
            snapshot.touch_gesture_raw,
            drag_delta.unwrap_or(0)
        );
        None
    }

    fn process_dashboard_touch_action(&mut self, snapshot: InputSnapshot) -> Option<UiAction> {
        if self.dashboard_page != DashboardPrimaryPage::DashboardHome {
            return None;
        }

        let prev = self.last_inputs.unwrap_or_else(InputSnapshot::idle);
        if !snapshot.touch || prev.touch {
            return None;
        }

        let (x, y) = match snapshot.touch_point {
            Some(point) => point,
            None => return None,
        };

        esp_println::println!(
            "ui: touch edge page=dashboard route={} x={} y={}",
            dashboard_route_name(self.dashboard_route),
            x,
            y
        );

        if let Some(target) = front_panel_scene::dashboard_hit_test(self.dashboard_route, x, y) {
            let resolved_target = if matches!(target, DashboardTouchTarget::ManualStart)
                && dashboard_manual_action_uses_stop(&self.self_check_snapshot)
            {
                DashboardTouchTarget::ManualStop
            } else {
                target
            };
            let next_route = front_panel_scene::dashboard_route_for_target(resolved_target);
            if next_route != self.dashboard_route {
                if let Some(focus) = dashboard_home_focus_for_touch_target(resolved_target) {
                    self.set_dashboard_home_focus(focus);
                }
                self.set_dashboard_route(next_route, "touch");
                defmt::info!(
                    "ui: dashboard touch target={}",
                    dashboard_touch_target_name(resolved_target)
                );
                esp_println::println!(
                    "ui: dashboard touch target={} source=touch",
                    dashboard_touch_target_name(resolved_target)
                );
            } else {
                defmt::info!(
                    "ui: dashboard route keep={} target={}",
                    dashboard_route_name(self.dashboard_route),
                    dashboard_touch_target_name(resolved_target)
                );
                esp_println::println!(
                    "ui: dashboard route keep={} target={} source=touch",
                    dashboard_route_name(self.dashboard_route),
                    dashboard_touch_target_name(resolved_target)
                );
            }

            if let Some(action) =
                front_panel_scene::dashboard_manual_charge_action_for_target(resolved_target)
            {
                defmt::info!(
                    "ui: manual_charge action={}",
                    manual_charge_ui_action_name(action)
                );
                esp_println::println!(
                    "ui: manual_charge action={}",
                    manual_charge_ui_action_name(action)
                );
                return Some(UiAction::ManualCharge(action));
            }
        } else {
            esp_println::println!(
                "ui: touch target=none page=dashboard route={} x={} y={}",
                dashboard_route_name(self.dashboard_route),
                x,
                y
            );
        }
        None
    }

    fn snapshot_to_model(&self, _snapshot: InputSnapshot) -> UiModel {
        UiModel {
            mode: self.self_check_snapshot.mode,
            // Runtime pages remain data-driven, but D-pad focus/menu state is tracked separately.
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
        let dashboard_shell = self.dashboard_shell_state();
        let dashboard_route = self.dashboard_route;
        let self_check_snapshot = self.self_check_snapshot;
        let self_check_overlay = self.self_check_overlay;
        self.render_scene(|painter| {
            if variant == DASHBOARD_VARIANT {
                front_panel_scene::render_dashboard_shell(
                    painter,
                    &model,
                    variant,
                    dashboard_shell,
                    Some(&self_check_snapshot),
                )
            } else {
                front_panel_scene::render_frame_with_dashboard_route_overlay(
                    painter,
                    &model,
                    variant,
                    dashboard_route,
                    Some(&self_check_snapshot),
                    self_check_overlay,
                )
            }
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

fn dashboard_route_name(route: DashboardRoute) -> &'static str {
    match route {
        DashboardRoute::Home => "home",
        DashboardRoute::Detail(front_panel_scene::DashboardDetailPage::Cells) => "detail_cells",
        DashboardRoute::Detail(front_panel_scene::DashboardDetailPage::BmsDetail) => "detail_bms",
        DashboardRoute::Detail(front_panel_scene::DashboardDetailPage::BatteryFlow) => {
            "detail_battery_flow"
        }
        DashboardRoute::Detail(front_panel_scene::DashboardDetailPage::Output) => "detail_output",
        DashboardRoute::Detail(front_panel_scene::DashboardDetailPage::Charger) => "detail_charger",
        DashboardRoute::Detail(front_panel_scene::DashboardDetailPage::Thermal) => "detail_thermal",
        DashboardRoute::Detail(front_panel_scene::DashboardDetailPage::Wifi) => "detail_wifi",
        DashboardRoute::ManualCharge => "manual_charge",
    }
}

fn dashboard_page_name(page: DashboardPrimaryPage) -> &'static str {
    match page {
        DashboardPrimaryPage::DashboardHome => "dashboard_home",
        DashboardPrimaryPage::Menu => "menu",
        DashboardPrimaryPage::BeeperSettings => "audio",
    }
}

fn dashboard_menu_target_offset_y(page: DashboardPrimaryPage) -> i16 {
    match page {
        DashboardPrimaryPage::DashboardHome => 0,
        DashboardPrimaryPage::Menu | DashboardPrimaryPage::BeeperSettings => {
            front_panel_scene::UI_H as i16
        }
    }
}

fn dashboard_page_transition_is_animated(
    from: DashboardPrimaryPage,
    to: DashboardPrimaryPage,
) -> bool {
    matches!(
        (from, to),
        (
            DashboardPrimaryPage::DashboardHome,
            DashboardPrimaryPage::Menu
        ) | (
            DashboardPrimaryPage::Menu,
            DashboardPrimaryPage::DashboardHome
        )
    )
}

fn vertical_gesture_direction_name(direction: VerticalGestureDirection) -> &'static str {
    match direction {
        VerticalGestureDirection::Up => "up",
        VerticalGestureDirection::Down => "down",
    }
}

fn dashboard_home_focus_name(focus: DashboardHomeFocus) -> &'static str {
    match focus {
        DashboardHomeFocus::Output => "output",
        DashboardHomeFocus::Thermal => "thermal",
        DashboardHomeFocus::Cells => "cells",
        DashboardHomeFocus::Charger => "charger",
        DashboardHomeFocus::BatteryFlow => "battery_flow",
    }
}

fn menu_item_name(item: MenuItem) -> &'static str {
    match item {
        MenuItem::Dashboard => "dashboard",
        MenuItem::Beeper => "audio",
    }
}

fn beeper_target_name(target: BeeperSettingTarget) -> &'static str {
    match target {
        BeeperSettingTarget::Action => "action",
        BeeperSettingTarget::System => "system",
    }
}

fn dashboard_home_focus_for_touch_target(
    target: DashboardTouchTarget,
) -> Option<DashboardHomeFocus> {
    match target {
        DashboardTouchTarget::HomeOutput => Some(DashboardHomeFocus::Output),
        DashboardTouchTarget::HomeThermal => Some(DashboardHomeFocus::Thermal),
        DashboardTouchTarget::HomeCells => Some(DashboardHomeFocus::Cells),
        DashboardTouchTarget::HomeCharger => Some(DashboardHomeFocus::Charger),
        DashboardTouchTarget::HomeBatteryFlow => Some(DashboardHomeFocus::BatteryFlow),
        _ => None,
    }
}

fn dashboard_touch_target_name(target: DashboardTouchTarget) -> &'static str {
    match target {
        DashboardTouchTarget::HomeWifi => "home_wifi",
        DashboardTouchTarget::HomeOutput => "home_output",
        DashboardTouchTarget::HomeThermal => "home_thermal",
        DashboardTouchTarget::HomeCells => "home_cells",
        DashboardTouchTarget::HomeCharger => "home_charger",
        DashboardTouchTarget::HomeBatteryFlow => "home_battery_flow",
        DashboardTouchTarget::DetailBack => "detail_back",
        DashboardTouchTarget::CellsAdvancedEntry => "cells_advanced_entry",
        DashboardTouchTarget::CellsAdvancedBack => "cells_advanced_back",
        DashboardTouchTarget::ChargerManualEntry => "charger_manual_entry",
        DashboardTouchTarget::ManualBack => "manual_back",
        DashboardTouchTarget::ManualTarget3V7 => "manual_target_3v7",
        DashboardTouchTarget::ManualTarget80 => "manual_target_80",
        DashboardTouchTarget::ManualTarget100 => "manual_target_100",
        DashboardTouchTarget::ManualSpeed100 => "manual_speed_100",
        DashboardTouchTarget::ManualSpeed500 => "manual_speed_500",
        DashboardTouchTarget::ManualSpeed1A => "manual_speed_1a",
        DashboardTouchTarget::ManualTimer1h => "manual_timer_1h",
        DashboardTouchTarget::ManualTimer2h => "manual_timer_2h",
        DashboardTouchTarget::ManualTimer6h => "manual_timer_6h",
        DashboardTouchTarget::ManualStart => "manual_start",
        DashboardTouchTarget::ManualStop => "manual_stop",
    }
}

fn dashboard_manual_action_uses_stop(snapshot: &SelfCheckUiSnapshot) -> bool {
    snapshot.dashboard_detail.manual_charge.runtime.active
}

fn manual_charge_ui_action_name(action: ManualChargeUiAction) -> &'static str {
    match action {
        ManualChargeUiAction::SetTarget(front_panel_scene::ManualChargeTarget::Pack3V7) => {
            "set_target_3v7"
        }
        ManualChargeUiAction::SetTarget(front_panel_scene::ManualChargeTarget::Rsoc80) => {
            "set_target_80"
        }
        ManualChargeUiAction::SetTarget(front_panel_scene::ManualChargeTarget::Full100) => {
            "set_target_100"
        }
        ManualChargeUiAction::SetSpeed(front_panel_scene::ManualChargeSpeed::Ma100) => {
            "set_speed_100"
        }
        ManualChargeUiAction::SetSpeed(front_panel_scene::ManualChargeSpeed::Ma500) => {
            "set_speed_500"
        }
        ManualChargeUiAction::SetSpeed(front_panel_scene::ManualChargeSpeed::Ma1000) => {
            "set_speed_1a"
        }
        ManualChargeUiAction::SetTimerLimit(front_panel_scene::ManualChargeTimerLimit::H1) => {
            "set_timer_1h"
        }
        ManualChargeUiAction::SetTimerLimit(front_panel_scene::ManualChargeTimerLimit::H2) => {
            "set_timer_2h"
        }
        ManualChargeUiAction::SetTimerLimit(front_panel_scene::ManualChargeTimerLimit::H6) => {
            "set_timer_6h"
        }
        ManualChargeUiAction::Start => "start",
        ManualChargeUiAction::Stop => "stop",
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
            front_panel_scene::self_check_tps_a_summary_name(next),
            front_panel_scene::self_check_tps_b_summary_name(next),
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
        || previous.bq40z50_issue_detail != next.bq40z50_issue_detail
        || previous.bq40z50_recovery_action != next.bq40z50_recovery_action
        || previous.bq40z50_last_result != next.bq40z50_last_result;
    if power_detail_changed {
        defmt::info!(
            "ui: self_check power_detail vbus_present={=?} chg_allow={=?} chg_ichg_ma={=?} vbat_present={=?} bq40_soc_pct={=?} bq40_rca_alarm={=?} bq40_no_battery={=?} bq40_dsg_ready={=?} bq40_issue_detail={=?} bq40_recovery_action={} bq40_last_result={}",
            next.fusb302_vbus_present,
            next.bq25792_allow_charge,
            next.bq25792_ichg_ma,
            next.bq25792_vbat_present,
            next.bq40z50_soc_pct,
            next.bq40z50_rca_alarm,
            next.bq40z50_no_battery,
            next.bq40z50_discharge_ready,
            next.bq40z50_issue_detail,
            bms_recovery_ui_action_option_name(next.bq40z50_recovery_action),
            bms_result_option_name(next.bq40z50_last_result)
        );
    }
}

fn overlay_name(overlay: SelfCheckOverlay) -> &'static str {
    match overlay {
        SelfCheckOverlay::None => "none",
        SelfCheckOverlay::BmsActivateConfirm => "confirm_activation",
        SelfCheckOverlay::BmsActivateProgress => "progress_activation",
        SelfCheckOverlay::BmsDischargeAuthorizeConfirm => "confirm_discharge",
        SelfCheckOverlay::BmsDischargeAuthorizeProgress => "progress_discharge",
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
        SelfCheckOverlay::HardwareIssue(target) => self_check_hardware_target_name(target),
    }
}

fn self_check_hardware_target_name(target: SelfCheckHardwareTarget) -> &'static str {
    match target {
        SelfCheckHardwareTarget::Gc9307 => "gc9307",
        SelfCheckHardwareTarget::Tca6408a => "tca6408a",
        SelfCheckHardwareTarget::Fusb302 => "fusb302",
        SelfCheckHardwareTarget::Ina3221 => "ina3221",
        SelfCheckHardwareTarget::Bq25792 => "bq25792",
        SelfCheckHardwareTarget::Bq40z50 => "bq40z50",
        SelfCheckHardwareTarget::TpsA => "tps_a",
        SelfCheckHardwareTarget::TpsB => "tps_b",
        SelfCheckHardwareTarget::TmpA => "tmp_a",
        SelfCheckHardwareTarget::TmpB => "tmp_b",
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

fn bms_recovery_ui_action_name(action: BmsRecoveryUiAction) -> &'static str {
    match action {
        BmsRecoveryUiAction::Activation => "activation",
        BmsRecoveryUiAction::DischargeAuthorization => "discharge_authorization",
    }
}

fn bms_recovery_ui_action_option_name(action: Option<BmsRecoveryUiAction>) -> &'static str {
    action.map_or("none", bms_recovery_ui_action_name)
}

fn current_recovery_overlay_action(
    overlay: SelfCheckOverlay,
    recovery_overlay: Option<SelfCheckOverlay>,
) -> Option<BmsRecoveryUiAction> {
    match overlay {
        SelfCheckOverlay::BmsActivateConfirm | SelfCheckOverlay::BmsActivateProgress => {
            Some(BmsRecoveryUiAction::Activation)
        }
        SelfCheckOverlay::BmsDischargeAuthorizeConfirm
        | SelfCheckOverlay::BmsDischargeAuthorizeProgress => {
            Some(BmsRecoveryUiAction::DischargeAuthorization)
        }
        SelfCheckOverlay::BmsActivateResult(..)
        | SelfCheckOverlay::HardwareIssue(..)
        | SelfCheckOverlay::None => match recovery_overlay {
            Some(SelfCheckOverlay::BmsActivateConfirm) => Some(BmsRecoveryUiAction::Activation),
            Some(SelfCheckOverlay::BmsDischargeAuthorizeConfirm) => {
                Some(BmsRecoveryUiAction::DischargeAuthorization)
            }
            _ => None,
        },
    }
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
        self.dc.set_input_enable(true);
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

    fn sleep_display(&mut self) -> Result<(), esp_hal::spi::Error> {
        self.write_cmd(CMD_DISPLAY_OFF)?;
        self.write_cmd(CMD_SLEEP_IN)
    }

    fn wake_display(&mut self) -> Result<(), esp_hal::spi::Error> {
        self.write_cmd(CMD_SLEEP_OUT)?;
        busy_wait(Duration::from_millis(120));
        self.write_cmd(CMD_DISPLAY_ON)
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
