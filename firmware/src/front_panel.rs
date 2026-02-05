use esp_hal::gpio::{DriveMode, Flex, OutputConfig, Pull};
use esp_hal::i2c::master::I2c as HalI2c;
use esp_hal::spi::{master::Spi as HalSpi, Mode};
use esp_hal::time::{Duration, Instant, Rate};
use esp_hal::Blocking;

// Front panel: TFT (GC9307) over SPI + slow control lines via TCA6408A (I2C2).
//
// Scope is intentionally minimal: show "Hello World" + a corner fps overlay for bring-up.
// Touch remains held in reset (TP_RESET low).

const TCA6408A_ADDR: u8 = 0x21;

// TCA6408A registers.
const TCA_REG_INPUT: u8 = 0x00;
const TCA_REG_OUTPUT: u8 = 0x01;
const TCA_REG_POLARITY: u8 = 0x02;
const TCA_REG_CONFIG: u8 = 0x03;

// TCA port bit assignments (per docs/pcbs/front-panel/README.md).
const TCA_BIT_CS: u8 = 5; // P5
const TCA_BIT_RES: u8 = 6; // P6 (active-low)
const TCA_BIT_TP_RESET: u8 = 7; // P7 (active-low)

// GC9307 (MIPI DCS-ish) commands.
const CMD_SWRESET: u8 = 0x01;
const CMD_SLP_OUT: u8 = 0x11;
const CMD_NORON: u8 = 0x13;
const CMD_DISPON: u8 = 0x29;
const CMD_CASET: u8 = 0x2A;
const CMD_RASET: u8 = 0x2B;
const CMD_RAMWR: u8 = 0x2C;
const CMD_MADCTL: u8 = 0x36;
const CMD_COLMOD: u8 = 0x3A;

const LCD_W: u16 = 240;
const LCD_H: u16 = 320;

// Pixel format: 16-bit / pixel (RGB565) => COLMOD param 0x55 (DPI=101, DBI=101).
const COLMOD_RGB565: u8 = 0x55;

// MADCTL bits (per GC9307 datasheet).
// Keep rotation default; set BGR=1 (common for small TFT modules). If colors are swapped,
// flip this bit to 0 on the next bring-up iteration.
const MADCTL_BGR: u8 = 0x08;

// Backlight: Q16 is a P-channel high-side switch (source=3V3, gate=BLK). BLK low turns on.
const BACKLIGHT_ACTIVE_LOW: bool = true;

// Simple rendering loop used only for fps measurement.
const FRAME_INTERVAL: Duration = Duration::from_millis(33); // ~30 fps target
const FPS_UPDATE_INTERVAL: Duration = Duration::from_secs(1);

// Tiny indicator dot; we toggle it each frame to ensure we're actually issuing display writes.
const DOT_W: u16 = 6;
const DOT_H: u16 = 6;
const DOT_X: u16 = LCD_W - DOT_W - 2;
const DOT_Y: u16 = LCD_H - DOT_H - 2;

// 5x7 font cell (plus 1 px spacing) scaled up for readability.
const FONT_SCALE: u16 = 2;
const GLYPH_W: u16 = 5;
const GLYPH_H: u16 = 7;
const CELL_W: u16 = (GLYPH_W + 1) * FONT_SCALE;
const CELL_H: u16 = (GLYPH_H + 1) * FONT_SCALE;

// Colors in RGB565 (MSB first on the wire).
const RGB565_BLACK: u16 = 0x0000;
const RGB565_WHITE: u16 = 0xFFFF;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitState {
    Disabled,
    Ready,
}

pub struct FrontPanel {
    // Buses / pins
    i2c: HalI2c<'static, Blocking>,
    spi: HalSpi<'static, Blocking>,
    tca_reset_n: Flex<'static>, // GPIO1, open-drain, low-active reset for TCA6408A
    dc: Flex<'static>,          // GPIO10
    bl: Flex<'static>,          // GPIO13

    // Cached expander output register so we can flip bits without reading.
    tca_output: u8,

    // Render loop state
    state: InitState,
    next_frame_deadline: Instant,
    fps_window_start: Instant,
    frames_in_window: u32,
    dot_on: bool,
    last_fps: u32,
}

impl FrontPanel {
    pub fn new(
        i2c: HalI2c<'static, Blocking>,
        spi: HalSpi<'static, Blocking>,
        tca_reset_n: Flex<'static>,
        dc: Flex<'static>,
        bl: Flex<'static>,
    ) -> Self {
        let now = Instant::now();
        Self {
            i2c,
            spi,
            tca_reset_n,
            dc,
            bl,
            tca_output: 0,
            state: InitState::Disabled,
            next_frame_deadline: now,
            fps_window_start: now,
            frames_in_window: 0,
            dot_on: false,
            last_fps: 0,
        }
    }

    pub fn init_best_effort(&mut self) {
        // Configure GPIOs first so "screen black" isn't mistaken for "screen dead".
        self.configure_dc();
        self.configure_backlight();
        self.configure_tca_reset();

        // Reset expander to force known safe state (CS high via 100k pull-up, RES low via 100k).
        self.pulse_tca_reset(Duration::from_millis(10));

        if let Err(e) = self.tca_init() {
            defmt::error!(
                "ui: tca6408a init failed err={}",
                crate::output::i2c_error_kind(e)
            );
            self.state = InitState::Disabled;
            return;
        }

        // Screen: keep CS high, RES low for a moment, then release reset, then enable CS.
        if let Err(e) = self.tca_set_res_released(false) {
            defmt::error!(
                "ui: tca set res failed err={}",
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

        busy_wait(Duration::from_millis(5));

        if let Err(e) = self.tca_set_res_released(true) {
            defmt::error!(
                "ui: tca release res failed err={}",
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

        if let Err(e) = self.gc9307_init() {
            defmt::error!("ui: gc9307 init failed err={=?}", e);
            // Return to safe state: unselect + reset asserted, backlight off.
            let _ = self.tca_set_cs_enabled(false);
            let _ = self.tca_set_res_released(false);
            self.set_backlight(false);
            self.state = InitState::Disabled;
            return;
        }

        // Visible bring-up output: clear + text + initial fps.
        if let Err(e) = self.fill_rect(0, 0, LCD_W, LCD_H, RGB565_BLACK) {
            defmt::error!("ui: clear failed err={=?}", e);
        }
        let _ = self.draw_text(10, 30, "Hello World", RGB565_WHITE, RGB565_BLACK);
        self.last_fps = 0;
        let _ = self.draw_fps(self.last_fps);

        self.set_backlight(true);

        let now = Instant::now();
        self.state = InitState::Ready;
        self.next_frame_deadline = now;
        self.fps_window_start = now;
        self.frames_in_window = 0;
        self.dot_on = false;

        defmt::info!("ui: front panel ready");
    }

    pub fn tick(&mut self) {
        if self.state != InitState::Ready {
            return;
        }

        let now = Instant::now();
        if now >= self.next_frame_deadline {
            self.next_frame_deadline += FRAME_INTERVAL;
            self.frames_in_window = self.frames_in_window.saturating_add(1);
            self.dot_on = !self.dot_on;

            // Minimal "frame" update: toggle a tiny dot.
            let color = if self.dot_on {
                RGB565_WHITE
            } else {
                RGB565_BLACK
            };
            let _ = self.fill_rect(DOT_X, DOT_Y, DOT_W, DOT_H, color);
        }

        let elapsed = now - self.fps_window_start;
        if elapsed >= FPS_UPDATE_INTERVAL {
            let micros = elapsed.as_micros().max(1);
            let fps = ((self.frames_in_window as u64) * 1_000_000 + (micros / 2)) / micros;
            let fps_u32 = core::cmp::min(fps, u32::MAX as u64) as u32;

            // Redraw at least once per second so "fps=<n>" is a live indicator even when stable.
            self.last_fps = fps_u32;
            let _ = self.draw_fps(self.last_fps);

            self.fps_window_start = now;
            self.frames_in_window = 0;
        }
    }

    fn configure_dc(&mut self) {
        self.dc.apply_output_config(
            &OutputConfig::default()
                .with_drive_mode(DriveMode::PushPull)
                .with_pull(Pull::None),
        );
        self.dc.set_low(); // command by default
        self.dc.set_output_enable(true);
    }

    fn configure_backlight(&mut self) {
        self.bl.apply_output_config(
            &OutputConfig::default()
                .with_drive_mode(DriveMode::PushPull)
                .with_pull(Pull::None),
        );
        // Default off until we've written something to the screen.
        self.set_backlight(false);
        self.bl.set_output_enable(true);
    }

    fn configure_tca_reset(&mut self) {
        self.tca_reset_n.apply_output_config(
            &OutputConfig::default()
                .with_drive_mode(DriveMode::OpenDrain)
                .with_pull(Pull::Up),
        );
        self.tca_reset_n.set_high(); // released
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
        // Polarity: default (no inversion).
        self.i2c.write(TCA6408A_ADDR, &[TCA_REG_POLARITY, 0x00])?;

        // Output defaults: CS high (not selected), RES low (asserted), TP_RESET low (touch held reset).
        self.tca_output = 0u8;
        self.tca_output |= 1 << TCA_BIT_CS;
        self.tca_output &= !(1 << TCA_BIT_RES);
        self.tca_output &= !(1 << TCA_BIT_TP_RESET);
        // RES/TP_RESET bits stay 0.
        self.i2c
            .write(TCA6408A_ADDR, &[TCA_REG_OUTPUT, self.tca_output])?;

        // Config: P0..P4 inputs; P5..P7 outputs.
        let config: u8 = 0x1F;
        self.i2c.write(TCA6408A_ADDR, &[TCA_REG_CONFIG, config])?;

        // Optional sanity read (best-effort): input port should be readable once configured.
        let mut inb = [0u8; 1];
        let _ = self
            .i2c
            .write_read(TCA6408A_ADDR, &[TCA_REG_INPUT], &mut inb);

        Ok(())
    }

    fn tca_set_cs_enabled(&mut self, enabled: bool) -> Result<(), esp_hal::i2c::master::Error> {
        // Active-low CS: enable => drive low.
        if enabled {
            self.tca_output &= !(1 << TCA_BIT_CS);
        } else {
            self.tca_output |= 1 << TCA_BIT_CS;
        }
        self.i2c
            .write(TCA6408A_ADDR, &[TCA_REG_OUTPUT, self.tca_output])
    }

    fn tca_set_res_released(&mut self, released: bool) -> Result<(), esp_hal::i2c::master::Error> {
        // Active-low reset: released => drive high.
        if released {
            self.tca_output |= 1 << TCA_BIT_RES;
        } else {
            self.tca_output &= !(1 << TCA_BIT_RES);
        }
        self.i2c
            .write(TCA6408A_ADDR, &[TCA_REG_OUTPUT, self.tca_output])
    }

    fn write_cmd(&mut self, cmd: u8) -> Result<(), esp_hal::spi::Error> {
        self.dc.set_low();
        self.spi.write(&[cmd])
    }

    fn write_data(&mut self, data: &[u8]) -> Result<(), esp_hal::spi::Error> {
        self.dc.set_high();
        self.spi.write(data)
    }

    fn gc9307_init(&mut self) -> Result<(), esp_hal::spi::Error> {
        // Conservative SPI settings for bring-up.
        // Note: this re-applies frequency/mode even if caller already configured it.
        let cfg = esp_hal::spi::master::Config::default()
            .with_frequency(Rate::from_mhz(10))
            .with_mode(Mode::_0);
        let _ = self.spi.apply_config(&cfg);

        // Software reset + standard DCS init.
        self.write_cmd(CMD_SWRESET)?;
        busy_wait(Duration::from_millis(150));

        self.write_cmd(CMD_SLP_OUT)?;
        busy_wait(Duration::from_millis(120));

        self.write_cmd(CMD_NORON)?;

        self.write_cmd(CMD_COLMOD)?;
        self.write_data(&[COLMOD_RGB565])?;

        self.write_cmd(CMD_MADCTL)?;
        self.write_data(&[MADCTL_BGR])?;

        self.write_cmd(CMD_DISPON)?;
        busy_wait(Duration::from_millis(20));

        Ok(())
    }

    fn set_window(
        &mut self,
        x0: u16,
        y0: u16,
        x1: u16,
        y1: u16,
    ) -> Result<(), esp_hal::spi::Error> {
        self.write_cmd(CMD_CASET)?;
        self.write_data(&u16_be_pair(x0, x1))?;
        self.write_cmd(CMD_RASET)?;
        self.write_data(&u16_be_pair(y0, y1))?;
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

    fn draw_text(
        &mut self,
        mut x: u16,
        y: u16,
        s: &str,
        fg: u16,
        bg: u16,
    ) -> Result<(), esp_hal::spi::Error> {
        for ch in s.bytes() {
            self.draw_char(x, y, ch, fg, bg)?;
            x = x.saturating_add(CELL_W);
        }
        Ok(())
    }

    fn draw_fps(&mut self, fps: u32) -> Result<(), esp_hal::spi::Error> {
        // Fixed overlay area: "fps=9999" worst-case width.
        const MAX_CHARS: u16 = 8;
        let overlay_w: u16 = MAX_CHARS * CELL_W;
        let overlay_h: u16 = CELL_H;
        let margin: u16 = 2;
        let x0 = LCD_W.saturating_sub(overlay_w + margin);
        let y0 = margin;

        // Clear overlay area.
        self.fill_rect(x0, y0, overlay_w, overlay_h, RGB565_BLACK)?;

        // Render "fps=<n>" right-aligned within overlay.
        let mut digits = [0u8; 10];
        let digits_len = u32_to_dec(fps, &mut digits);

        // Compose on the fly to avoid allocations.
        let prefix = b"fps=";
        let total_chars = (prefix.len() + digits_len) as u16;
        let text_w = total_chars * CELL_W;
        let x = LCD_W.saturating_sub(text_w + margin);
        let y = y0;

        let mut cx = x;
        for &b in prefix {
            self.draw_char(cx, y, b, RGB565_WHITE, RGB565_BLACK)?;
            cx += CELL_W;
        }
        for i in 0..digits_len {
            self.draw_char(cx, y, digits[i], RGB565_WHITE, RGB565_BLACK)?;
            cx += CELL_W;
        }

        Ok(())
    }

    fn draw_char(
        &mut self,
        x: u16,
        y: u16,
        ch: u8,
        fg: u16,
        bg: u16,
    ) -> Result<(), esp_hal::spi::Error> {
        let glyph = glyph_5x7(ch);

        let w = CELL_W;
        let h = CELL_H;
        let x1 = x.saturating_add(w - 1);
        let y1 = y.saturating_add(h - 1);

        self.set_window(x, y, x1, y1)?;
        self.write_cmd(CMD_RAMWR)?;

        // Small stack buffer: one scanline (scaled) of a single character cell.
        let [fg_hi, fg_lo] = fg.to_be_bytes();
        let [bg_hi, bg_lo] = bg.to_be_bytes();
        let mut line = [0u8; (CELL_W as usize) * 2];

        for row in 0..(GLYPH_H as usize + 1) {
            for _sy in 0..(FONT_SCALE as usize) {
                // Build a scaled scanline into `line`.
                let mut idx = 0usize;
                for col in 0..(GLYPH_W as usize + 1) {
                    let on = if col < GLYPH_W as usize && row < GLYPH_H as usize {
                        let bits = glyph[col];
                        ((bits >> (row as u32)) & 0x01) != 0
                    } else {
                        false
                    };
                    let (hi, lo) = if on { (fg_hi, fg_lo) } else { (bg_hi, bg_lo) };
                    for _sx in 0..(FONT_SCALE as usize) {
                        line[idx] = hi;
                        line[idx + 1] = lo;
                        idx += 2;
                    }
                }

                self.write_data(&line)?;
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

fn u32_to_dec(mut v: u32, out: &mut [u8; 10]) -> usize {
    if v == 0 {
        out[0] = b'0';
        return 1;
    }

    let mut tmp = [0u8; 10];
    let mut n = 0usize;
    while v > 0 && n < tmp.len() {
        tmp[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }

    // Reverse into out.
    for i in 0..n {
        out[i] = tmp[n - 1 - i];
    }
    n
}

// Minimal 5x7 glyphs. Columns are LSB-top.
fn glyph_5x7(ch: u8) -> [u8; 5] {
    match ch {
        b' ' => [0x00, 0x00, 0x00, 0x00, 0x00],
        b'=' => [0x14, 0x14, 0x14, 0x14, 0x14],

        b'0' => [0x3E, 0x51, 0x49, 0x45, 0x3E],
        b'1' => [0x00, 0x42, 0x7F, 0x40, 0x00],
        b'2' => [0x42, 0x61, 0x51, 0x49, 0x46],
        b'3' => [0x21, 0x41, 0x45, 0x4B, 0x31],
        b'4' => [0x18, 0x14, 0x12, 0x7F, 0x10],
        b'5' => [0x27, 0x45, 0x45, 0x45, 0x39],
        b'6' => [0x3C, 0x4A, 0x49, 0x49, 0x30],
        b'7' => [0x01, 0x71, 0x09, 0x05, 0x03],
        b'8' => [0x36, 0x49, 0x49, 0x49, 0x36],
        b'9' => [0x06, 0x49, 0x49, 0x29, 0x1E],

        b'H' => [0x7F, 0x08, 0x08, 0x08, 0x7F],
        b'W' => [0x7F, 0x20, 0x18, 0x20, 0x7F],

        b'd' => [0x30, 0x48, 0x48, 0x44, 0x7F],
        b'e' => [0x38, 0x54, 0x54, 0x54, 0x18],
        b'f' => [0x08, 0x7E, 0x09, 0x01, 0x02],
        b'l' => [0x00, 0x41, 0x7F, 0x40, 0x00],
        b'o' => [0x38, 0x44, 0x44, 0x44, 0x38],
        b'p' => [0x7C, 0x14, 0x14, 0x14, 0x08],
        b'r' => [0x7C, 0x08, 0x04, 0x04, 0x08],
        b's' => [0x48, 0x54, 0x54, 0x54, 0x20],

        _ => [0x00, 0x00, 0x00, 0x00, 0x00],
    }
}
