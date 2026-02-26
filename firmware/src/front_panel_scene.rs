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

static FONT_LABEL_STRONG: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvB12_tf>();
static FONT_LABEL: FontRenderer = FontRenderer::new::<fonts::u8g2_font_helvR10_tf>();
static FONT_VALUE_STRONG: FontRenderer = FontRenderer::new::<fonts::u8g2_font_9x15_mf>();
static FONT_VALUE: FontRenderer = FontRenderer::new::<fonts::u8g2_font_8x13_mf>();

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
    LabelStrong,
    Label,
    ValueStrong,
    Value,
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

    let out_a_mv: u16 = 19_000 + if out_a_on { 120 } else { 0 };
    let out_a_ma: u16 = if out_a_on { 820 } else { 140 };
    let out_b_mv: u16 = 19_000 + if out_b_on { 80 } else { 0 };
    let out_b_ma: u16 = if out_b_on { 760 } else { 0 };

    let chg_vbus_mv: u16 = 20_100 + if chg_on { 120 } else { 0 };
    let chg_iin_ma: u16 = if chg_on { 1_250 } else { 180 };

    let bms_pack_mv: u16 = 15_700 + ((model.frame_no % 3) as u16) * 10;
    let bms_soc_pct: u16 = 62 + ((model.frame_no % 4) as u16);

    let therm_a_c: u16 = if therm_on { 52 } else { 37 };
    let therm_b_c: u16 = if therm_on { 50 } else { 35 };

    // Header text.
    render_text(
        painter,
        variant,
        FontRole::LabelStrong,
        "MAINS AEGIS",
        Point::new(10, 3),
        HorizontalAlignment::Left,
        palette.text_primary,
    )?;
    render_text(
        painter,
        variant,
        FontRole::Label,
        "POWER DASHBOARD",
        Point::new((UI_W - 10) as i32, 4),
        HorizontalAlignment::Right,
        palette.accent,
    )?;

    // OUT-A block.
    render_text(
        painter,
        variant,
        FontRole::LabelStrong,
        "OUT-A",
        Point::new((CARD_A_X + 8) as i32, (CARD_A_Y + 6) as i32),
        HorizontalAlignment::Left,
        palette.text_primary,
    )?;
    render_text(
        painter,
        variant,
        FontRole::Label,
        if out_a_on { "ENABLED" } else { "STANDBY" },
        Point::new((CARD_A_X + CARD_A_W - 8) as i32, (CARD_A_Y + 7) as i32),
        HorizontalAlignment::Right,
        if out_a_on {
            palette.up_active
        } else {
            palette.text_muted
        },
    )?;
    render_text(
        painter,
        variant,
        FontRole::Label,
        "VOUT",
        Point::new((CARD_A_X + 8) as i32, (CARD_A_Y + 24) as i32),
        HorizontalAlignment::Left,
        palette.text_muted,
    )?;
    render_text(
        painter,
        variant,
        FontRole::Value,
        format_args!("{:>2}.{:01}V", out_a_mv / 1000, (out_a_mv % 1000) / 100),
        Point::new((CARD_A_X + CARD_A_W - 8) as i32, (CARD_A_Y + 22) as i32),
        HorizontalAlignment::Right,
        palette.text_primary,
    )?;
    render_text(
        painter,
        variant,
        FontRole::Label,
        "IOUT",
        Point::new((CARD_A_X + 8) as i32, (CARD_A_Y + 41) as i32),
        HorizontalAlignment::Left,
        palette.text_muted,
    )?;
    render_text(
        painter,
        variant,
        FontRole::Value,
        format_args!("{:>1}.{:02}A", out_a_ma / 1000, (out_a_ma % 1000) / 10),
        Point::new((CARD_A_X + CARD_A_W - 8) as i32, (CARD_A_Y + 39) as i32),
        HorizontalAlignment::Right,
        palette.text_primary,
    )?;

    // OUT-B block.
    render_text(
        painter,
        variant,
        FontRole::LabelStrong,
        "OUT-B",
        Point::new((CARD_B_X + 8) as i32, (CARD_B_Y + 6) as i32),
        HorizontalAlignment::Left,
        palette.text_primary,
    )?;
    render_text(
        painter,
        variant,
        FontRole::Label,
        if out_b_on { "ENABLED" } else { "GATED" },
        Point::new((CARD_B_X + CARD_B_W - 8) as i32, (CARD_B_Y + 7) as i32),
        HorizontalAlignment::Right,
        if out_b_on {
            palette.down_active
        } else {
            palette.text_muted
        },
    )?;
    render_text(
        painter,
        variant,
        FontRole::Label,
        "VOUT",
        Point::new((CARD_B_X + 8) as i32, (CARD_B_Y + 24) as i32),
        HorizontalAlignment::Left,
        palette.text_muted,
    )?;
    render_text(
        painter,
        variant,
        FontRole::Value,
        format_args!("{:>2}.{:01}V", out_b_mv / 1000, (out_b_mv % 1000) / 100),
        Point::new((CARD_B_X + CARD_B_W - 8) as i32, (CARD_B_Y + 22) as i32),
        HorizontalAlignment::Right,
        palette.text_primary,
    )?;
    render_text(
        painter,
        variant,
        FontRole::Label,
        "IOUT",
        Point::new((CARD_B_X + 8) as i32, (CARD_B_Y + 41) as i32),
        HorizontalAlignment::Left,
        palette.text_muted,
    )?;
    render_text(
        painter,
        variant,
        FontRole::Value,
        format_args!("{:>1}.{:02}A", out_b_ma / 1000, (out_b_ma % 1000) / 10),
        Point::new((CARD_B_X + CARD_B_W - 8) as i32, (CARD_B_Y + 39) as i32),
        HorizontalAlignment::Right,
        palette.text_primary,
    )?;

    // CHARGER block.
    render_text(
        painter,
        variant,
        FontRole::LabelStrong,
        "CHARGER",
        Point::new((CARD_CHG_X + 8) as i32, (CARD_CHG_Y + 6) as i32),
        HorizontalAlignment::Left,
        palette.text_primary,
    )?;
    render_text(
        painter,
        variant,
        FontRole::Label,
        if chg_on { "ACTIVE" } else { "SAFE" },
        Point::new(
            (CARD_CHG_X + CARD_CHG_W - 8) as i32,
            (CARD_CHG_Y + 7) as i32,
        ),
        HorizontalAlignment::Right,
        if chg_on {
            palette.right_active
        } else {
            palette.text_muted
        },
    )?;
    render_text(
        painter,
        variant,
        FontRole::Label,
        "VBUS",
        Point::new((CARD_CHG_X + 8) as i32, (CARD_CHG_Y + 22) as i32),
        HorizontalAlignment::Left,
        palette.text_muted,
    )?;
    render_text(
        painter,
        variant,
        FontRole::ValueStrong,
        format_args!(
            "{:>2}.{:01}V",
            chg_vbus_mv / 1000,
            (chg_vbus_mv % 1000) / 100
        ),
        Point::new(
            (CARD_CHG_X + CARD_CHG_W - 8) as i32,
            (CARD_CHG_Y + 20) as i32,
        ),
        HorizontalAlignment::Right,
        palette.text_primary,
    )?;

    // BMS block.
    render_text(
        painter,
        variant,
        FontRole::LabelStrong,
        "BMS",
        Point::new((CARD_BMS_X + 8) as i32, (CARD_BMS_Y + 6) as i32),
        HorizontalAlignment::Left,
        palette.text_primary,
    )?;
    render_text(
        painter,
        variant,
        FontRole::Label,
        "ADDR 0X0B",
        Point::new(
            (CARD_BMS_X + CARD_BMS_W - 8) as i32,
            (CARD_BMS_Y + 7) as i32,
        ),
        HorizontalAlignment::Right,
        if bms_on {
            palette.left_active
        } else {
            palette.text_muted
        },
    )?;
    render_text(
        painter,
        variant,
        FontRole::Label,
        "PACK",
        Point::new((CARD_BMS_X + 8) as i32, (CARD_BMS_Y + 22) as i32),
        HorizontalAlignment::Left,
        palette.text_muted,
    )?;
    render_text(
        painter,
        variant,
        FontRole::ValueStrong,
        format_args!(
            "{:>2}.{:01}V",
            bms_pack_mv / 1000,
            (bms_pack_mv % 1000) / 100
        ),
        Point::new(
            (CARD_BMS_X + CARD_BMS_W - 8) as i32,
            (CARD_BMS_Y + 20) as i32,
        ),
        HorizontalAlignment::Right,
        palette.text_primary,
    )?;

    // Alert row.
    render_text(
        painter,
        variant,
        FontRole::Label,
        "THERM",
        Point::new((CARD_ALERT_X + 6) as i32, (CARD_ALERT_Y + 7) as i32),
        HorizontalAlignment::Left,
        palette.text_muted,
    )?;
    render_text(
        painter,
        variant,
        FontRole::Value,
        format_args!("A{:02}C B{:02}C", therm_a_c, therm_b_c),
        Point::new((CARD_ALERT_X + 54) as i32, (CARD_ALERT_Y + 5) as i32),
        HorizontalAlignment::Left,
        if therm_on {
            palette.center_active
        } else {
            palette.text_primary
        },
    )?;
    render_text(
        painter,
        variant,
        FontRole::Label,
        if model.touch_irq { "IRQ ON" } else { "IRQ OFF" },
        Point::new(
            (CARD_ALERT_X + CARD_ALERT_W - 6) as i32,
            (CARD_ALERT_Y + 7) as i32,
        ),
        HorizontalAlignment::Right,
        if model.touch_irq {
            palette.touch_active
        } else {
            palette.text_muted
        },
    )?;

    // Footer: key mapping and live numeric summary.
    render_text(
        painter,
        variant,
        FontRole::Label,
        "UP:A  DN:B  LT:BMS  RT:CHG  OK:THERM",
        Point::new(8, (UI_H - 15) as i32),
        HorizontalAlignment::Left,
        palette.text_muted,
    )?;
    render_text(
        painter,
        variant,
        FontRole::Value,
        format_args!(
            "SOC {:02}%  IIN {:>1}.{:02}A",
            bms_soc_pct,
            chg_iin_ma / 1000,
            (chg_iin_ma % 1000) / 10
        ),
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

fn select_renderer(_variant: UiVariant, role: FontRole) -> &'static FontRenderer {
    match role {
        FontRole::LabelStrong => &FONT_LABEL_STRONG,
        FontRole::Label => &FONT_LABEL,
        FontRole::ValueStrong => &FONT_VALUE_STRONG,
        FontRole::Value => &FONT_VALUE,
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
