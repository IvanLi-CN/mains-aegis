use embedded_graphics_core::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Point, Size},
    pixelcolor::{raw::RawU16, Rgb565},
    prelude::RawData,
    Pixel,
};
use u8g2_fonts::{
    fonts,
    types::{FontColor, HorizontalAlignment, VerticalPosition},
    Content, Error as FontError, FontRenderer,
};

pub const UI_W: u16 = 320;
pub const UI_H: u16 = 172;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum UiVariant {
    InstrumentA,
    InstrumentB,
    RetroC,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiFocus {
    Idle,
    Up,
    Down,
    Left,
    Right,
    Center,
    Touch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiModel {
    pub focus: UiFocus,
    pub touch_irq: bool,
    pub frame_no: u32,
}

pub trait UiPainter {
    type Error;

    fn fill_rect(&mut self, x: u16, y: u16, w: u16, h: u16, rgb565: u16)
        -> Result<(), Self::Error>;
}

const FRAME_BG_A: u16 = 0x0821;
const FRAME_BG_B: u16 = 0x1062;
const FRAME_BG_C: u16 = 0x0000;

const HEADER_H: u16 = 18;
const FOOTER_H: u16 = 20;

const CARD_A_X: u16 = 8;
const CARD_A_Y: u16 = 22;
const CARD_A_W: u16 = 188;
const CARD_A_H: u16 = 56;

const CARD_B_X: u16 = 8;
const CARD_B_Y: u16 = 84;
const CARD_B_W: u16 = 188;
const CARD_B_H: u16 = 56;

const CARD_CHG_X: u16 = 204;
const CARD_CHG_Y: u16 = 22;
const CARD_CHG_W: u16 = 108;
const CARD_CHG_H: u16 = 42;

const CARD_BMS_X: u16 = 204;
const CARD_BMS_Y: u16 = 68;
const CARD_BMS_W: u16 = 108;
const CARD_BMS_H: u16 = 42;

const CARD_ALERT_X: u16 = 204;
const CARD_ALERT_Y: u16 = 114;
const CARD_ALERT_W: u16 = 108;
const CARD_ALERT_H: u16 = 26;

static FONT_A_TITLE: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvB12_tf>();
static FONT_A_BODY: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvR10_tf>();

static FONT_B_TITLE: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvB14_tf>();
static FONT_B_BODY: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvR12_tf>();

static FONT_C_TITLE: FontRenderer = FontRenderer::new::<fonts::u8g2_font_ncenB14_tf>();
static FONT_C_BODY: FontRenderer = FontRenderer::new::<fonts::u8g2_font_ncenR10_tf>();

#[derive(Clone, Copy)]
struct Palette {
    bg: u16,
    grid: u16,
    panel: u16,
    panel_border: u16,
    key_idle: u16,
    key_border: u16,
    text_primary: u16,
    text_muted: u16,
    accent: u16,
    up_active: u16,
    down_active: u16,
    left_active: u16,
    right_active: u16,
    center_active: u16,
    touch_active: u16,
}

#[derive(Clone, Copy)]
enum FontRole {
    Title,
    Body,
}

pub fn render_frame<P: UiPainter>(
    painter: &mut P,
    model: &UiModel,
    variant: UiVariant,
) -> Result<(), P::Error> {
    let palette = palette_for(variant);

    fill(painter, 0, 0, UI_W, UI_H, palette.bg)?;
    draw_background_grid(painter, palette)?;

    draw_outline(painter, 0, 0, UI_W, UI_H, palette.panel_border)?;

    // Header and footer.
    fill(painter, 0, 0, UI_W, HEADER_H, palette.panel)?;
    fill(painter, 0, UI_H - FOOTER_H, UI_W, FOOTER_H, palette.panel)?;

    let out_a_on = model.focus == UiFocus::Up;
    let out_b_on = model.focus == UiFocus::Down;
    let bms_on = model.focus == UiFocus::Left;
    let chg_on = model.focus == UiFocus::Right;
    let therm_on = model.focus == UiFocus::Center;
    let alert_on = model.focus == UiFocus::Touch || model.touch_irq;

    draw_key(
        painter,
        CARD_A_X,
        CARD_A_Y,
        CARD_A_W,
        CARD_A_H,
        out_a_on,
        palette.key_idle,
        palette.key_border,
        palette.up_active,
    )?;
    draw_key(
        painter,
        CARD_B_X,
        CARD_B_Y,
        CARD_B_W,
        CARD_B_H,
        out_b_on,
        palette.key_idle,
        palette.key_border,
        palette.down_active,
    )?;
    draw_key(
        painter,
        CARD_CHG_X,
        CARD_CHG_Y,
        CARD_CHG_W,
        CARD_CHG_H,
        chg_on,
        palette.key_idle,
        palette.key_border,
        palette.right_active,
    )?;
    draw_key(
        painter,
        CARD_BMS_X,
        CARD_BMS_Y,
        CARD_BMS_W,
        CARD_BMS_H,
        bms_on,
        palette.key_idle,
        palette.key_border,
        palette.left_active,
    )?;
    draw_key(
        painter,
        CARD_ALERT_X,
        CARD_ALERT_Y,
        CARD_ALERT_W,
        CARD_ALERT_H,
        alert_on,
        palette.key_idle,
        palette.key_border,
        palette.touch_active,
    )?;

    if therm_on {
        fill(
            painter,
            CARD_A_X + CARD_A_W - 48,
            CARD_A_Y + 4,
            40,
            10,
            palette.center_active,
        )?;
    }

    // Header text.
    render_text(
        painter,
        variant,
        FontRole::Title,
        "POWER CONTROL",
        Point::new(10, 3),
        HorizontalAlignment::Left,
        palette.text_primary,
    )?;
    render_text(
        painter,
        variant,
        FontRole::Body,
        "BQ40Z50 + BQ25792",
        Point::new((UI_W - 10) as i32, 4),
        HorizontalAlignment::Right,
        palette.accent,
    )?;

    // OUT-A block.
    render_text(
        painter,
        variant,
        FontRole::Title,
        "OUT-A READY",
        Point::new((CARD_A_X + 8) as i32, (CARD_A_Y + 8) as i32),
        HorizontalAlignment::Left,
        palette.text_primary,
    )?;
    render_text(
        painter,
        variant,
        FontRole::Body,
        "TARGET 19.0V",
        Point::new((CARD_A_X + 8) as i32, (CARD_A_Y + 26) as i32),
        HorizontalAlignment::Left,
        palette.text_primary,
    )?;
    render_text(
        painter,
        variant,
        FontRole::Body,
        "ILIMIT 3.5A",
        Point::new((CARD_A_X + 8) as i32, (CARD_A_Y + 40) as i32),
        HorizontalAlignment::Left,
        palette.text_muted,
    )?;

    // OUT-B block.
    render_text(
        painter,
        variant,
        FontRole::Title,
        "OUT-B STBY",
        Point::new((CARD_B_X + 8) as i32, (CARD_B_Y + 8) as i32),
        HorizontalAlignment::Left,
        palette.text_primary,
    )?;
    render_text(
        painter,
        variant,
        FontRole::Body,
        "TARGET 19.0V",
        Point::new((CARD_B_X + 8) as i32, (CARD_B_Y + 26) as i32),
        HorizontalAlignment::Left,
        palette.text_primary,
    )?;
    render_text(
        painter,
        variant,
        FontRole::Body,
        "STATE GATED",
        Point::new((CARD_B_X + 8) as i32, (CARD_B_Y + 40) as i32),
        HorizontalAlignment::Left,
        palette.text_muted,
    )?;

    // CHARGER + BMS block.
    render_text(
        painter,
        variant,
        FontRole::Title,
        "CHARGER",
        Point::new((CARD_CHG_X + 8) as i32, (CARD_CHG_Y + 8) as i32),
        HorizontalAlignment::Left,
        palette.text_primary,
    )?;
    render_text(
        painter,
        variant,
        FontRole::Body,
        "BQ25792 SAFE",
        Point::new((CARD_CHG_X + 8) as i32, (CARD_CHG_Y + 24) as i32),
        HorizontalAlignment::Left,
        palette.text_muted,
    )?;
    render_text(
        painter,
        variant,
        FontRole::Title,
        "BMS",
        Point::new((CARD_BMS_X + 8) as i32, (CARD_BMS_Y + 8) as i32),
        HorizontalAlignment::Left,
        palette.text_primary,
    )?;
    render_text(
        painter,
        variant,
        FontRole::Body,
        "BQ40Z50 0X0B",
        Point::new((CARD_BMS_X + 8) as i32, (CARD_BMS_Y + 24) as i32),
        HorizontalAlignment::Left,
        palette.text_muted,
    )?;

    // Alert row.
    render_text(
        painter,
        variant,
        FontRole::Body,
        if model.touch_irq {
            "ALERT TOUCH IRQ"
        } else {
            "ALERT NONE"
        },
        Point::new((CARD_ALERT_X + 6) as i32, (CARD_ALERT_Y + 7) as i32),
        HorizontalAlignment::Left,
        if model.touch_irq {
            palette.touch_active
        } else {
            palette.text_primary
        },
    )?;

    // Footer: map keys to product actions.
    render_text(
        painter,
        variant,
        FontRole::Body,
        "UP:A DN:B LT:BMS RT:CHG",
        Point::new(8, (UI_H - 15) as i32),
        HorizontalAlignment::Left,
        palette.text_muted,
    )?;
    render_text(
        painter,
        variant,
        FontRole::Body,
        focus_action_label(model.focus),
        Point::new((UI_W - 8) as i32, (UI_H - 15) as i32),
        HorizontalAlignment::Right,
        palette.accent,
    )?;

    Ok(())
}

fn draw_background_grid<P: UiPainter>(painter: &mut P, palette: Palette) -> Result<(), P::Error> {
    let mut y = HEADER_H;
    while y < UI_H - FOOTER_H {
        fill(painter, 0, y, UI_W, 1, palette.grid)?;
        y += 8;
    }

    let mut x = 0;
    while x < UI_W {
        fill(
            painter,
            x,
            HEADER_H,
            1,
            UI_H - HEADER_H - FOOTER_H,
            palette.grid,
        )?;
        x += 16;
    }

    Ok(())
}

fn draw_key<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    active: bool,
    idle_fill: u16,
    border: u16,
    active_fill: u16,
) -> Result<(), P::Error> {
    fill(painter, x, y, w, h, border)?;

    if w > 4 && h > 4 {
        let inner = if active { active_fill } else { idle_fill };
        fill(painter, x + 2, y + 2, w - 4, h - 4, inner)?;
    }

    Ok(())
}

fn draw_outline<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    color: u16,
) -> Result<(), P::Error> {
    if w == 0 || h == 0 {
        return Ok(());
    }

    fill(painter, x, y, w, 1, color)?;
    fill(painter, x, y + h.saturating_sub(1), w, 1, color)?;
    fill(painter, x, y, 1, h, color)?;
    fill(painter, x + w.saturating_sub(1), y, 1, h, color)?;
    Ok(())
}

fn fill<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    color: u16,
) -> Result<(), P::Error> {
    if w == 0 || h == 0 {
        return Ok(());
    }

    painter.fill_rect(x, y, w, h, color)
}

fn render_text<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    role: FontRole,
    content: impl Content,
    anchor: Point,
    align: HorizontalAlignment,
    color: u16,
) -> Result<(), P::Error> {
    let renderer = select_renderer(variant, role);
    let mut target = PainterDrawTarget::new(painter);

    match renderer.render_aligned(
        content,
        anchor,
        VerticalPosition::Top,
        align,
        FontColor::Transparent(rgb565_from_u16(color)),
        &mut target,
    ) {
        Ok(_) => Ok(()),
        Err(FontError::DisplayError(e)) => Err(e),
        Err(FontError::GlyphNotFound(_)) | Err(FontError::BackgroundColorNotSupported) => Ok(()),
    }
}

fn select_renderer(variant: UiVariant, role: FontRole) -> &'static FontRenderer {
    match (variant, role) {
        (UiVariant::InstrumentA, FontRole::Title) => &FONT_A_TITLE,
        (UiVariant::InstrumentA, FontRole::Body) => &FONT_A_BODY,
        (UiVariant::InstrumentB, FontRole::Title) => &FONT_B_TITLE,
        (UiVariant::InstrumentB, FontRole::Body) => &FONT_B_BODY,
        (UiVariant::RetroC, FontRole::Title) => &FONT_C_TITLE,
        (UiVariant::RetroC, FontRole::Body) => &FONT_C_BODY,
    }
}

fn focus_action_label(focus: UiFocus) -> &'static str {
    match focus {
        UiFocus::Idle => "OK:THERM NORMAL",
        UiFocus::Up => "SELECT OUT-A",
        UiFocus::Down => "SELECT OUT-B",
        UiFocus::Left => "SELECT BMS",
        UiFocus::Right => "SELECT CHARGER",
        UiFocus::Center => "THERM CHECK",
        UiFocus::Touch => "TOUCH ALERT ACK",
    }
}

fn palette_for(variant: UiVariant) -> Palette {
    match variant {
        UiVariant::InstrumentA => Palette {
            bg: FRAME_BG_A,
            grid: 0x18C3,
            panel: 0x10A2,
            panel_border: 0x7BEF,
            key_idle: 0x2965,
            key_border: 0xC618,
            text_primary: 0xFFFF,
            text_muted: 0xBDD7,
            accent: 0x07FF,
            up_active: 0xFFE0,
            down_active: 0x07FF,
            left_active: 0x5B5F,
            right_active: 0xF800,
            center_active: 0xF81F,
            touch_active: 0x07FF,
        },
        UiVariant::InstrumentB => Palette {
            bg: FRAME_BG_B,
            grid: 0x2124,
            panel: 0x2945,
            panel_border: 0xEF5D,
            key_idle: 0x3186,
            key_border: 0xD69A,
            text_primary: 0xFFFF,
            text_muted: 0xD69A,
            accent: 0xFD20,
            up_active: 0xFFE0,
            down_active: 0x07FF,
            left_active: 0x7BDE,
            right_active: 0xF800,
            center_active: 0xFD20,
            touch_active: 0xAFE5,
        },
        UiVariant::RetroC => Palette {
            bg: FRAME_BG_C,
            grid: 0x18C3,
            panel: 0x0841,
            panel_border: 0xC618,
            key_idle: 0x1082,
            key_border: 0x9492,
            text_primary: 0xFFFF,
            text_muted: 0xBDF7,
            accent: 0xFFE0,
            up_active: 0xFFE0,
            down_active: 0x07E0,
            left_active: 0x7BEF,
            right_active: 0xF800,
            center_active: 0xF81F,
            touch_active: 0x07FF,
        },
    }
}

fn rgb565_from_u16(raw: u16) -> Rgb565 {
    Rgb565::from(RawU16::new(raw))
}

struct PainterDrawTarget<'a, P> {
    painter: &'a mut P,
}

impl<'a, P> PainterDrawTarget<'a, P> {
    fn new(painter: &'a mut P) -> Self {
        Self { painter }
    }
}

impl<P: UiPainter> DrawTarget for PainterDrawTarget<'_, P> {
    type Color = Rgb565;
    type Error = P::Error;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels {
            if point.x < 0 || point.y < 0 || point.x >= UI_W as i32 || point.y >= UI_H as i32 {
                continue;
            }

            let raw = RawU16::from(color).into_inner();
            self.painter
                .fill_rect(point.x as u16, point.y as u16, 1, 1, raw)?;
        }

        Ok(())
    }
}

impl<P: UiPainter> OriginDimensions for PainterDrawTarget<'_, P> {
    fn size(&self) -> Size {
        Size::new(UI_W as u32, UI_H as u32)
    }
}
