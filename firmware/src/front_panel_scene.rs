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

const CELL_W: u16 = 44;
const CELL_H: u16 = 44;
const PAD_X: u16 = 18;
const PAD_Y: u16 = 24;
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

static FONT_A_TITLE: FontRenderer = FontRenderer::new::<fonts::u8g2_font_8x13B_tf>();
static FONT_A_BODY: FontRenderer = FontRenderer::new::<fonts::u8g2_font_7x14B_tf>();

static FONT_B_TITLE: FontRenderer = FontRenderer::new::<fonts::u8g2_font_9x15_tf>();
static FONT_B_BODY: FontRenderer = FontRenderer::new::<fonts::u8g2_font_8x13_tf>();

static FONT_C_TITLE: FontRenderer = FontRenderer::new::<fonts::u8g2_font_pxplusibmvga8_tf>();
static FONT_C_BODY: FontRenderer = FontRenderer::new::<fonts::u8g2_font_pxplusibmvga8_tr>();

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

    // Header and status bars.
    fill(painter, 0, 0, UI_W, 18, palette.panel)?;
    fill(painter, 0, UI_H - 20, UI_W, 20, palette.panel)?;

    // Group containers.
    fill(painter, 8, 20, 186, 128, palette.panel)?;
    draw_outline(painter, 8, 20, 186, 128, palette.panel_border)?;

    fill(painter, 204, 20, 108, 128, palette.panel)?;
    draw_outline(painter, 204, 20, 108, 128, palette.panel_border)?;

    let up_on = model.focus == UiFocus::Up;
    let down_on = model.focus == UiFocus::Down;
    let left_on = model.focus == UiFocus::Left;
    let right_on = model.focus == UiFocus::Right;
    let center_on = model.focus == UiFocus::Center;
    let touch_on = model.focus == UiFocus::Touch || model.touch_irq;

    draw_key(
        painter,
        UP_X,
        UP_Y,
        CELL_W,
        CELL_H,
        up_on,
        palette.key_idle,
        palette.key_border,
        palette.up_active,
    )?;
    draw_key(
        painter,
        DOWN_X,
        DOWN_Y,
        CELL_W,
        CELL_H,
        down_on,
        palette.key_idle,
        palette.key_border,
        palette.down_active,
    )?;
    draw_key(
        painter,
        LEFT_X,
        LEFT_Y,
        CELL_W,
        CELL_H,
        left_on,
        palette.key_idle,
        palette.key_border,
        palette.left_active,
    )?;
    draw_key(
        painter,
        RIGHT_X,
        RIGHT_Y,
        CELL_W,
        CELL_H,
        right_on,
        palette.key_idle,
        palette.key_border,
        palette.right_active,
    )?;
    draw_key(
        painter,
        CENTER_X,
        CENTER_Y,
        CELL_W,
        CELL_H,
        center_on,
        palette.key_idle,
        palette.key_border,
        palette.center_active,
    )?;
    draw_key(
        painter,
        TOUCH_X,
        TOUCH_Y,
        TOUCH_W,
        TOUCH_H,
        touch_on,
        palette.key_idle,
        palette.key_border,
        palette.touch_active,
    )?;

    // Focus tracker bar.
    fill(painter, 214, 136, 88, 6, palette.key_idle)?;
    let tracker_w = ((model.frame_no % 88) as u16).saturating_add(1);
    fill(painter, 214, 136, tracker_w, 6, palette.accent)?;

    // Header text.
    render_text(
        painter,
        variant,
        FontRole::Title,
        "MAINS AEGIS",
        Point::new(10, 3),
        HorizontalAlignment::Left,
        palette.text_primary,
    )?;
    render_text(
        painter,
        variant,
        FontRole::Body,
        style_label(variant),
        Point::new((UI_W - 10) as i32, 4),
        HorizontalAlignment::Right,
        palette.accent,
    )?;

    // Key labels.
    render_text(
        painter,
        variant,
        FontRole::Body,
        "UP",
        Point::new((UP_X + CELL_W / 2) as i32, (UP_Y + 14) as i32),
        HorizontalAlignment::Center,
        palette.text_primary,
    )?;
    render_text(
        painter,
        variant,
        FontRole::Body,
        "DN",
        Point::new((DOWN_X + CELL_W / 2) as i32, (DOWN_Y + 14) as i32),
        HorizontalAlignment::Center,
        palette.text_primary,
    )?;
    render_text(
        painter,
        variant,
        FontRole::Body,
        "LT",
        Point::new((LEFT_X + CELL_W / 2) as i32, (LEFT_Y + 14) as i32),
        HorizontalAlignment::Center,
        palette.text_primary,
    )?;
    render_text(
        painter,
        variant,
        FontRole::Body,
        "RT",
        Point::new((RIGHT_X + CELL_W / 2) as i32, (RIGHT_Y + 14) as i32),
        HorizontalAlignment::Center,
        palette.text_primary,
    )?;
    render_text(
        painter,
        variant,
        FontRole::Body,
        "OK",
        Point::new((CENTER_X + CELL_W / 2) as i32, (CENTER_Y + 14) as i32),
        HorizontalAlignment::Center,
        palette.text_primary,
    )?;
    render_text(
        painter,
        variant,
        FontRole::Body,
        "TOUCH",
        Point::new((TOUCH_X + TOUCH_W / 2) as i32, (TOUCH_Y + 52) as i32),
        HorizontalAlignment::Center,
        palette.text_primary,
    )?;

    // Footer text.
    render_text(
        painter,
        variant,
        FontRole::Body,
        format_args!("FRAME {:06}", model.frame_no),
        Point::new(10, (UI_H - 16) as i32),
        HorizontalAlignment::Left,
        palette.text_muted,
    )?;
    render_text(
        painter,
        variant,
        FontRole::Body,
        focus_label(model.focus),
        Point::new((UI_W / 2) as i32, (UI_H - 16) as i32),
        HorizontalAlignment::Center,
        palette.text_primary,
    )?;
    render_text(
        painter,
        variant,
        FontRole::Body,
        if model.touch_irq { "IRQ ON" } else { "IRQ OFF" },
        Point::new((UI_W - 10) as i32, (UI_H - 16) as i32),
        HorizontalAlignment::Right,
        if model.touch_irq {
            palette.touch_active
        } else {
            palette.text_muted
        },
    )?;

    Ok(())
}

fn draw_background_grid<P: UiPainter>(painter: &mut P, palette: Palette) -> Result<(), P::Error> {
    let mut y = 18;
    while y < UI_H - 20 {
        fill(painter, 0, y, UI_W, 1, palette.grid)?;
        y += 8;
    }

    let mut x = 0;
    while x < UI_W {
        fill(painter, x, 18, 1, UI_H - 38, palette.grid)?;
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

fn style_label(variant: UiVariant) -> &'static str {
    match variant {
        UiVariant::InstrumentA => "STYLE A",
        UiVariant::InstrumentB => "STYLE B",
        UiVariant::RetroC => "STYLE C",
    }
}

fn focus_label(focus: UiFocus) -> &'static str {
    match focus {
        UiFocus::Idle => "FOCUS IDLE",
        UiFocus::Up => "FOCUS UP",
        UiFocus::Down => "FOCUS DOWN",
        UiFocus::Left => "FOCUS LEFT",
        UiFocus::Right => "FOCUS RIGHT",
        UiFocus::Center => "FOCUS OK",
        UiFocus::Touch => "FOCUS TOUCH",
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
