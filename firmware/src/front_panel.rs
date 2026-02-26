use core::convert::Infallible;

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

// TCA bit assignments.
const TCA_BIT_CS: u8 = 5; // P5, active-low
const TCA_BIT_RES: u8 = 6; // P6, active-low
const TCA_BIT_TP_RESET: u8 = 7; // P7, active-low

// DCS commands used for post-init test pattern writes.
const CMD_CASET: u8 = 0x2A;
const CMD_RASET: u8 = 0x2B;
const CMD_RAMWR: u8 = 0x2C;

// Match gc9307-async example profile (Landscape + 320x172 + dy=34).
const LCD_W: u16 = 320;
const LCD_H: u16 = 172;
const OFFSET_X: u16 = 0;
const OFFSET_Y: u16 = 34;

const BACKLIGHT_ACTIVE_LOW: bool = true;

const FRAME_INTERVAL: Duration = Duration::from_millis(50);

const UI_BG: u16 = 0x0000;
const UI_INACTIVE: u16 = 0x2104;
const UI_BORDER: u16 = 0xFFFF;
const UI_TOUCH_ACTIVE: u16 = RGB565_WHITE;
const UI_UP_ACTIVE: u16 = 0xFFE0;
const UI_DOWN_ACTIVE: u16 = 0x07FF;
const UI_LEFT_ACTIVE: u16 = RGB565_BLUE;
const UI_RIGHT_ACTIVE: u16 = RGB565_RED;
const UI_CENTER_ACTIVE: u16 = 0xF81F;

const CELL_W: u16 = 44;
const CELL_H: u16 = 44;
const PAD_X: u16 = 18;
const PAD_Y: u16 = 20;
const PAD_GAP: u16 = 6;

const UP_X: u16 = PAD_X + CELL_W + PAD_GAP;
const UP_Y: u16 = PAD_Y;
const DOWN_X: u16 = PAD_X + CELL_W + PAD_GAP;
const DOWN_Y: u16 = PAD_Y + (CELL_H + PAD_GAP) * 2;
const LEFT_X: u16 = PAD_X;
const LEFT_Y: u16 = PAD_Y + CELL_H + PAD_GAP;
const RIGHT_X: u16 = PAD_X + (CELL_W + PAD_GAP) * 2;
const RIGHT_Y: u16 = PAD_Y + CELL_H + PAD_GAP;
const CENTER_X: u16 = PAD_X + CELL_W + PAD_GAP;
const CENTER_Y: u16 = PAD_Y + CELL_H + PAD_GAP;

const TOUCH_X: u16 = 210;
const TOUCH_Y: u16 = 24;
const TOUCH_W: u16 = 94;
const TOUCH_H: u16 = 124;

const RGB565_WHITE: u16 = 0xFFFF;
const RGB565_RED: u16 = 0xF800;
const RGB565_BLUE: u16 = 0x001F;

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
        }
    }

    pub fn init_best_effort(&mut self) {
        self.configure_dc();
        self.configure_backlight();
        self.configure_tca_reset();

        // Reset expander and force known screen-safe defaults.
        self.pulse_tca_reset(Duration::from_millis(10));

        if let Err(e) = self.tca_init() {
            defmt::error!(
                "ui: tca6408a init failed err={}",
                crate::output::i2c_error_kind(e)
            );
            self.state = InitState::Disabled;
            return;
        }

        if let Err(e) = self.tca_set_res_released(false) {
            defmt::error!(
                "ui: tca set res failed err={}",
                crate::output::i2c_error_kind(e)
            );
            self.state = InitState::Disabled;
            return;
        }
        if let Err(e) = self.tca_set_tp_reset_released(false) {
            defmt::error!(
                "ui: tca set tp_reset failed err={}",
                crate::output::i2c_error_kind(e)
            );
            self.state = InitState::Disabled;
            return;
        }
        if let Err(e) = self.tca_set_cs_enabled(false) {
            defmt::error!(
                "ui: tca set cs failed err={}",
                crate::output::i2c_error_kind(e)
            );
            self.state = InitState::Disabled;
            return;
        }

        busy_wait(Duration::from_millis(10));

        // Hardware reset through expander lines before handing over to driver init.
        if let Err(e) = self.tca_set_res_released(true) {
            defmt::error!(
                "ui: tca release res failed err={}",
                crate::output::i2c_error_kind(e)
            );
            self.state = InitState::Disabled;
            return;
        }
        if let Err(e) = self.tca_set_tp_reset_released(true) {
            defmt::error!(
                "ui: tca release tp_reset failed err={}",
                crate::output::i2c_error_kind(e)
            );
            self.state = InitState::Disabled;
            return;
        }
        busy_wait(Duration::from_millis(120));

        if let Err(e) = self.tca_set_cs_enabled(true) {
            defmt::error!(
                "ui: tca enable cs failed err={}",
                crate::output::i2c_error_kind(e)
            );
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

        if let Err(e) = self.draw_input_dashboard() {
            defmt::error!("ui: draw dashboard failed err={=?}", e);
        }

        match self.read_inputs() {
            Ok(snapshot) => {
                if let Err(e) = self.render_inputs(snapshot) {
                    defmt::error!("ui: render input state failed err={=?}", e);
                } else {
                    self.last_inputs = Some(snapshot);
                }
            }
            Err(e) => {
                defmt::error!(
                    "ui: read input state failed err={}",
                    crate::output::i2c_error_kind(e)
                );
            }
        }

        self.set_backlight(true);
        self.state = InitState::Ready;
        self.next_frame_deadline = Instant::now();

        defmt::info!(
            "ui: front panel ready (driver=gc9307-async mode=input-test res={}x{} offset=({},{}))",
            LCD_W,
            LCD_H,
            OFFSET_X,
            OFFSET_Y
        );
    }

    pub fn tick(&mut self) {
        if self.state != InitState::Ready {
            return;
        }

        let now = Instant::now();
        if now < self.next_frame_deadline {
            return;
        }
        self.next_frame_deadline += FRAME_INTERVAL;

        match self.read_inputs() {
            Ok(snapshot) => {
                if self.last_inputs != Some(snapshot) {
                    if let Err(e) = self.render_inputs(snapshot) {
                        defmt::error!("ui: update input state failed err={=?}", e);
                    } else {
                        self.last_inputs = Some(snapshot);
                    }
                }
            }
            Err(e) => {
                defmt::error!(
                    "ui: poll input state failed err={}",
                    crate::output::i2c_error_kind(e)
                );
            }
        }
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
            orientation: Orientation::Landscape,
            width: LCD_W,
            height: LCD_H,
            dx: OFFSET_X,
            dy: OFFSET_Y,
            rgb: true,
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

    fn draw_input_dashboard(&mut self) -> Result<(), esp_hal::spi::Error> {
        self.fill_rect(0, 0, LCD_W, LCD_H, UI_BG)?;
        self.draw_cell(UP_X, UP_Y, CELL_W, CELL_H, UI_INACTIVE)?;
        self.draw_cell(DOWN_X, DOWN_Y, CELL_W, CELL_H, UI_INACTIVE)?;
        self.draw_cell(LEFT_X, LEFT_Y, CELL_W, CELL_H, UI_INACTIVE)?;
        self.draw_cell(RIGHT_X, RIGHT_Y, CELL_W, CELL_H, UI_INACTIVE)?;
        self.draw_cell(CENTER_X, CENTER_Y, CELL_W, CELL_H, UI_INACTIVE)?;
        self.draw_cell(TOUCH_X, TOUCH_Y, TOUCH_W, TOUCH_H, UI_INACTIVE)?;
        Ok(())
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

        Ok(InputSnapshot {
            up,
            down,
            left,
            right,
            center,
            touch,
        })
    }

    fn render_inputs(&mut self, snapshot: InputSnapshot) -> Result<(), esp_hal::spi::Error> {
        self.draw_state_cell(UP_X, UP_Y, CELL_W, CELL_H, snapshot.up, UI_UP_ACTIVE)?;
        self.draw_state_cell(
            DOWN_X,
            DOWN_Y,
            CELL_W,
            CELL_H,
            snapshot.down,
            UI_DOWN_ACTIVE,
        )?;
        self.draw_state_cell(
            LEFT_X,
            LEFT_Y,
            CELL_W,
            CELL_H,
            snapshot.left,
            UI_LEFT_ACTIVE,
        )?;
        self.draw_state_cell(
            RIGHT_X,
            RIGHT_Y,
            CELL_W,
            CELL_H,
            snapshot.right,
            UI_RIGHT_ACTIVE,
        )?;
        self.draw_state_cell(
            CENTER_X,
            CENTER_Y,
            CELL_W,
            CELL_H,
            snapshot.center,
            UI_CENTER_ACTIVE,
        )?;
        self.draw_state_cell(
            TOUCH_X,
            TOUCH_Y,
            TOUCH_W,
            TOUCH_H,
            snapshot.touch,
            UI_TOUCH_ACTIVE,
        )?;
        Ok(())
    }

    fn draw_state_cell(
        &mut self,
        x: u16,
        y: u16,
        w: u16,
        h: u16,
        active: bool,
        active_color: u16,
    ) -> Result<(), esp_hal::spi::Error> {
        let color = if active { active_color } else { UI_INACTIVE };
        self.draw_cell(x, y, w, h, color)
    }

    fn draw_cell(
        &mut self,
        x: u16,
        y: u16,
        w: u16,
        h: u16,
        fill: u16,
    ) -> Result<(), esp_hal::spi::Error> {
        self.fill_rect(x, y, w, h, UI_BORDER)?;
        if w > 4 && h > 4 {
            self.fill_rect(x + 2, y + 2, w - 4, h - 4, fill)?;
        }
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
