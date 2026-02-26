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
    InstrumentD,
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

const HEADER_H: u16 = 18;
const FOOTER_H: u16 = 18;

// User preference: non-numeric text uses Font A, numeric fields use fixed-width Font B.
static FONT_A_TITLE: FontRenderer = FontRenderer::new::<fonts::u8g2_font_8x13B_tf>();
static FONT_A_BODY: FontRenderer = FontRenderer::new::<fonts::u8g2_font_7x14B_tf>();
static FONT_B_NUM: FontRenderer = FontRenderer::new::<fonts::u8g2_font_8x13_mf>();
static FONT_B_NUM_BIG: FontRenderer = FontRenderer::new::<fonts::u8g2_font_10x20_mf>();

#[derive(Clone, Copy)]
struct Palette {
    bg: u16,
    panel: u16,
    panel_alt: u16,
    border: u16,
    text: u16,
    text_dim: u16,
    accent: u16,
    up: u16,
    down: u16,
    left: u16,
    right: u16,
    center: u16,
    touch: u16,
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
enum FontRole {
    TextTitle,
    TextBody,
    Num,
    NumBig,
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
struct DashboardData {
    focus: UiFocus,
    touch_irq: bool,
    mains_present: bool,
    out_a_on: bool,
    out_b_on: bool,
    bms_on: bool,
    chg_on: bool,
    therm_on: bool,
    alert_on: bool,
    out_a_mv: u16,
    out_a_ma: u16,
    out_b_mv: u16,
    out_b_ma: u16,
    chg_iin_ma: u16,
    bms_soc_pct: u16,
    therm_a_c: u16,
    therm_b_c: u16,
}

impl DashboardData {
    fn from_model(model: &UiModel) -> Self {
        let out_a_on = model.focus == UiFocus::Up;
        let out_b_on = model.focus == UiFocus::Down;
        let bms_on = model.focus == UiFocus::Left;
        let chg_on = model.focus == UiFocus::Right;
        let therm_on = model.focus == UiFocus::Center;
        let mains_present = matches!(model.focus, UiFocus::Idle | UiFocus::Left | UiFocus::Right);
        let alert_on = model.focus == UiFocus::Touch || model.touch_irq;
        let wave = (model.frame_no % 6) as u16;

        Self {
            focus: model.focus,
            touch_irq: model.touch_irq,
            mains_present,
            out_a_on,
            out_b_on,
            bms_on,
            chg_on,
            therm_on,
            alert_on,
            out_a_mv: 19_050 + if out_a_on { 120 } else { wave * 6 },
            out_a_ma: if out_a_on {
                820 + wave * 5
            } else if mains_present {
                260 + wave * 2
            } else {
                420 + wave * 3
            },
            out_b_mv: 19_020 + if out_b_on { 95 } else { wave * 4 },
            out_b_ma: if out_b_on {
                760 + wave * 4
            } else if mains_present {
                180 + wave * 2
            } else {
                360 + wave * 3
            },
            chg_iin_ma: if mains_present {
                if chg_on {
                    1250 + wave * 7
                } else {
                    320 + wave * 3
                }
            } else {
                0
            },
            bms_soc_pct: 61 + (wave % 5),
            therm_a_c: if therm_on {
                52 + (wave % 2)
            } else {
                37 + (wave % 2)
            },
            therm_b_c: if therm_on {
                50 + (wave % 2)
            } else {
                35 + (wave % 2)
            },
        }
    }
}

pub fn render_frame<P: UiPainter>(
    painter: &mut P,
    model: &UiModel,
    variant: UiVariant,
) -> Result<(), P::Error> {
    let palette = palette_for(variant);
    let data = DashboardData::from_model(model);

    fill(painter, 0, 0, UI_W, UI_H, palette.bg)?;
    draw_background_grid(painter, palette)?;
    draw_outline(painter, 0, 0, UI_W, UI_H, palette.border)?;

    match variant {
        UiVariant::InstrumentA => render_variant_a(painter, variant, palette, data)?,
        UiVariant::InstrumentB => render_variant_b(painter, variant, palette, data)?,
        UiVariant::RetroC => render_variant_c(painter, variant, palette, data)?,
        UiVariant::InstrumentD => render_variant_d(painter, variant, palette, data)?,
    }

    Ok(())
}

fn render_variant_a<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    data: DashboardData,
) -> Result<(), P::Error> {
    render_variant_b(painter, variant, palette, data)
}

fn render_variant_b<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    data: DashboardData,
) -> Result<(), P::Error> {
    // Tighten top KPI rhythm: keep label/value on separate rows with visible gap.
    let kpi_label_y = 29;
    let kpi_value_y = 50;

    let style_tag = match variant {
        UiVariant::InstrumentA => "CALM BLUE",
        UiVariant::InstrumentB => "NEUTRAL",
        UiVariant::InstrumentD => "WARM",
        UiVariant::RetroC => "DIAG",
    };

    let load_ma = data.out_a_ma as u32 + data.out_b_ma as u32;
    let bus_mv = ((data.out_a_mv as u32 + data.out_b_mv as u32) / 2) as u16;
    let charger_input_ma = if data.mains_present {
        data.chg_iin_ma as u32
    } else {
        0
    };
    let output_current_ma = load_ma.max(120);
    let input_current_ma = if data.mains_present {
        output_current_ma + charger_input_ma
    } else {
        0
    };
    let input_power_w10 = ((bus_mv as u32) * input_current_ma) / 100_000;
    let output_power_w10 = ((bus_mv as u32) * output_current_ma) / 100_000;

    let mode_accent = if data.touch_irq {
        palette.touch
    } else if data.mains_present {
        if data.chg_on {
            palette.right
        } else {
            palette.accent
        }
    } else if data.therm_on {
        palette.center
    } else {
        palette.left
    };
    let mode_tag = if data.touch_irq {
        "IRQ MODE"
    } else if data.mains_present {
        "AC MODE"
    } else {
        "BATT MODE"
    };
    draw_top_bar_with_status(
        painter,
        variant,
        palette,
        data.focus,
        "DASHBOARD",
        style_tag,
        mode_tag,
        mode_accent,
    )?;

    draw_panel(painter, 6, 22, 196, 52, palette, true, mode_accent)?;
    if data.mains_present {
        text(
            painter,
            variant,
            FontRole::TextBody,
            "POUT W",
            Point::new(14, kpi_label_y),
            HorizontalAlignment::Left,
            palette.bg,
        )?;
        text(
            painter,
            variant,
            FontRole::TextBody,
            "PIN W",
            Point::new(194, kpi_label_y),
            HorizontalAlignment::Right,
            palette.bg,
        )?;
        text(
            painter,
            variant,
            FontRole::NumBig,
            format_args!("{:>2}.{:01}", output_power_w10 / 10, output_power_w10 % 10),
            Point::new(14, kpi_value_y),
            HorizontalAlignment::Left,
            palette.bg,
        )?;
        text(
            painter,
            variant,
            FontRole::NumBig,
            format_args!("{:>2}.{:01}", input_power_w10 / 10, input_power_w10 % 10),
            Point::new(194, kpi_value_y),
            HorizontalAlignment::Right,
            palette.bg,
        )?;
    } else {
        text(
            painter,
            variant,
            FontRole::TextBody,
            "POUT W",
            Point::new(14, kpi_label_y),
            HorizontalAlignment::Left,
            palette.bg,
        )?;
        text(
            painter,
            variant,
            FontRole::TextBody,
            "IOUT A",
            Point::new(194, kpi_label_y),
            HorizontalAlignment::Right,
            palette.bg,
        )?;
        text(
            painter,
            variant,
            FontRole::NumBig,
            format_args!("{:>2}.{:01}", output_power_w10 / 10, output_power_w10 % 10),
            Point::new(14, kpi_value_y),
            HorizontalAlignment::Left,
            palette.bg,
        )?;
        text(
            painter,
            variant,
            FontRole::NumBig,
            format_args!(
                "{:>1}.{:01}",
                (output_current_ma / 1000),
                ((output_current_ma % 1000) / 100)
            ),
            Point::new(194, kpi_value_y),
            HorizontalAlignment::Right,
            palette.bg,
        )?;
    }

    draw_panel(painter, 6, 78, 196, 72, palette, false, palette.accent)?;
    if data.mains_present {
        text(
            painter,
            variant,
            FontRole::TextBody,
            "FLOW",
            Point::new(14, 83),
            HorizontalAlignment::Left,
            palette.text,
        )?;
        text(
            painter,
            variant,
            FontRole::TextBody,
            "IIN",
            Point::new(14, 99),
            HorizontalAlignment::Left,
            palette.text_dim,
        )?;
        text(
            painter,
            variant,
            FontRole::Num,
            format_args!(
                "{:>1}.{:02}A",
                (input_current_ma as u16) / 1000,
                ((input_current_ma as u16) % 1000) / 10
            ),
            Point::new(194, 99),
            HorizontalAlignment::Right,
            palette.text,
        )?;
        text(
            painter,
            variant,
            FontRole::TextBody,
            "ICHG",
            Point::new(14, 116),
            HorizontalAlignment::Left,
            palette.text_dim,
        )?;
        text(
            painter,
            variant,
            FontRole::Num,
            format_args!(
                "{:>1}.{:02}A",
                (charger_input_ma as u16) / 1000,
                ((charger_input_ma as u16) % 1000) / 10
            ),
            Point::new(194, 116),
            HorizontalAlignment::Right,
            palette.text,
        )?;
        text(
            painter,
            variant,
            FontRole::TextBody,
            "IOUT NET",
            Point::new(14, 133),
            HorizontalAlignment::Left,
            palette.text_dim,
        )?;
        text(
            painter,
            variant,
            FontRole::Num,
            format_args!(
                "{:>1}.{:02}A",
                (output_current_ma as u16) / 1000,
                ((output_current_ma as u16) % 1000) / 10
            ),
            Point::new(194, 133),
            HorizontalAlignment::Right,
            palette.text,
        )?;
    } else {
        text(
            painter,
            variant,
            FontRole::TextBody,
            "OUTPUT",
            Point::new(14, 83),
            HorizontalAlignment::Left,
            palette.text,
        )?;
        text(
            painter,
            variant,
            FontRole::TextBody,
            "VOUT",
            Point::new(14, 99),
            HorizontalAlignment::Left,
            palette.text_dim,
        )?;
        text(
            painter,
            variant,
            FontRole::Num,
            format_args!("{:>2}.{:01}V", bus_mv / 1000, (bus_mv % 1000) / 100),
            Point::new(194, 99),
            HorizontalAlignment::Right,
            palette.text,
        )?;
        text(
            painter,
            variant,
            FontRole::TextBody,
            "IOUT",
            Point::new(14, 116),
            HorizontalAlignment::Left,
            palette.text_dim,
        )?;
        text(
            painter,
            variant,
            FontRole::Num,
            format_args!(
                "{:>1}.{:02}A",
                (output_current_ma as u16) / 1000,
                ((output_current_ma as u16) % 1000) / 10
            ),
            Point::new(194, 116),
            HorizontalAlignment::Right,
            palette.text,
        )?;
        text(
            painter,
            variant,
            FontRole::TextBody,
            "POUT",
            Point::new(14, 133),
            HorizontalAlignment::Left,
            palette.text_dim,
        )?;
        text(
            painter,
            variant,
            FontRole::Num,
            format_args!("{:>2}.{:01}W", output_power_w10 / 10, output_power_w10 % 10),
            Point::new(194, 133),
            HorizontalAlignment::Right,
            palette.text,
        )?;
    }

    draw_health_block(
        painter,
        variant,
        palette,
        HealthBlock {
            x: 206,
            y: 22,
            w: 108,
            h: 40,
            title: "BMS SOC",
            value: format_args!("{:>2}%", data.bms_soc_pct),
            note: if data.bms_on { "SEL" } else { "NORM" },
            meter: data.bms_soc_pct as u32,
            active: data.bms_on,
            accent: palette.left,
        },
    )?;
    draw_health_block(
        painter,
        variant,
        palette,
        HealthBlock {
            x: 206,
            y: 66,
            w: 108,
            h: 40,
            title: "THERM",
            value: format_args!("{:02}/{:02}C", data.therm_a_c, data.therm_b_c),
            note: if data.therm_on { "LIM" } else { "NORM" },
            meter: ((data.therm_a_c as u32 + data.therm_b_c as u32).saturating_sub(50)).min(100),
            active: data.therm_on,
            accent: palette.center,
        },
    )?;
    draw_health_block(
        painter,
        variant,
        palette,
        HealthBlock {
            x: 206,
            y: 110,
            w: 108,
            h: 40,
            title: "MODE/IRQ",
            value: if data.mains_present {
                "AC ONLINE"
            } else {
                "AC LOST"
            },
            note: if data.touch_irq { "IRQ ON" } else { "IRQ OFF" },
            meter: if data.mains_present { 90 } else { 20 },
            active: data.mains_present || data.touch_irq,
            accent: if data.touch_irq {
                palette.touch
            } else {
                palette.right
            },
        },
    )?;

    draw_bottom_bar(
        painter,
        variant,
        palette,
        if data.mains_present {
            "AC: PIN / POUT / IOUT NET"
        } else {
            "BATT: VOUT / IOUT / POUT / TEMP / SOC"
        },
    )
}

fn render_variant_c<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    data: DashboardData,
) -> Result<(), P::Error> {
    draw_top_bar(
        painter,
        variant,
        palette,
        data.focus,
        "SELF CHECK",
        "DIAG TABLE",
    )?;

    let x = 6;
    let y = 22;
    let w = 308;
    let header_h = 16;
    let row_h = 18;

    draw_panel(painter, x, y, w, 124, palette, false, palette.accent)?;

    fill(painter, x + 2, y + 2, w - 4, header_h, palette.panel_alt)?;
    draw_column_headers(painter, variant, palette, x + 4, y + 3)?;

    draw_table_row(
        painter,
        variant,
        palette,
        TableRow {
            x: x + 2,
            y: y + 2 + header_h,
            h: row_h,
            module: "OUT-A",
            status: if data.out_a_on { "ACT" } else { "OK " },
            voltage: "IO-A",
            current: format_args!(
                "{:>2}.{:01}V",
                data.out_a_mv / 1000,
                (data.out_a_mv % 1000) / 100
            ),
            temp: "--/--",
            active: data.focus == UiFocus::Up,
            accent: palette.up,
            odd: false,
        },
    )?;
    draw_table_row(
        painter,
        variant,
        palette,
        TableRow {
            x: x + 2,
            y: y + 2 + header_h + row_h,
            h: row_h,
            module: "OUT-B",
            status: if data.out_b_on { "ACT" } else { "OK " },
            voltage: "IO-B",
            current: format_args!(
                "{:>2}.{:01}V",
                data.out_b_mv / 1000,
                (data.out_b_mv % 1000) / 100
            ),
            temp: "--/--",
            active: data.focus == UiFocus::Down,
            accent: palette.down,
            odd: true,
        },
    )?;
    draw_table_row(
        painter,
        variant,
        palette,
        TableRow {
            x: x + 2,
            y: y + 2 + header_h + row_h * 2,
            h: row_h,
            module: "CHG",
            status: if data.chg_on { "RUN" } else { "STB" },
            voltage: "I2C",
            current: format_args!(
                "{:>1}.{:02}A",
                data.chg_iin_ma / 1000,
                (data.chg_iin_ma % 1000) / 10
            ),
            temp: "OK  ",
            active: data.focus == UiFocus::Right,
            accent: palette.right,
            odd: false,
        },
    )?;
    draw_table_row(
        painter,
        variant,
        palette,
        TableRow {
            x: x + 2,
            y: y + 2 + header_h + row_h * 3,
            h: row_h,
            module: "BMS",
            status: if data.bms_on { "SEL" } else { "OK " },
            voltage: "SMB",
            current: format_args!("{:>2}%", data.bms_soc_pct),
            temp: "OK  ",
            active: data.focus == UiFocus::Left,
            accent: palette.left,
            odd: true,
        },
    )?;
    draw_table_row(
        painter,
        variant,
        palette,
        TableRow {
            x: x + 2,
            y: y + 2 + header_h + row_h * 4,
            h: row_h,
            module: "THERM",
            status: if data.therm_on { "HOT" } else { "OK " },
            voltage: "NTC",
            temp: format_args!("{:02}/{:02}", data.therm_a_c, data.therm_b_c),
            current: "READ",
            active: data.focus == UiFocus::Center,
            accent: palette.center,
            odd: false,
        },
    )?;
    draw_table_row(
        painter,
        variant,
        palette,
        TableRow {
            x: x + 2,
            y: y + 2 + header_h + row_h * 5,
            h: row_h,
            module: "IRQ",
            status: if data.touch_irq { "ON " } else { "OFF" },
            voltage: "INT",
            current: if data.touch_irq { "EDGE" } else { "NONE" },
            temp: if data.touch_irq { "ALRT" } else { "OK  " },
            active: data.focus == UiFocus::Touch || data.touch_irq,
            accent: palette.touch,
            odd: true,
        },
    )?;

    draw_bottom_bar(
        painter,
        variant,
        palette,
        "SELF CHECK | MOD | STATE | CODE | READ",
    )
}

fn render_variant_d<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    data: DashboardData,
) -> Result<(), P::Error> {
    render_variant_b(painter, variant, palette, data)
}

#[allow(dead_code)]
struct ChannelCard {
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    label: &'static str,
    mv: u16,
    ma: u16,
    active: bool,
    accent: u16,
}

#[allow(dead_code)]
fn draw_channel_card<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    spec: ChannelCard,
) -> Result<(), P::Error> {
    draw_panel(
        painter,
        spec.x,
        spec.y,
        spec.w,
        spec.h,
        palette,
        spec.active,
        spec.accent,
    )?;

    let text_color = if spec.active {
        palette.bg
    } else {
        palette.text
    };
    text(
        painter,
        variant,
        FontRole::TextTitle,
        spec.label,
        Point::new((spec.x + 8) as i32, (spec.y + 5) as i32),
        HorizontalAlignment::Left,
        text_color,
    )?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        if spec.active { "ON " } else { "OFF" },
        Point::new((spec.x + spec.w - 8) as i32, (spec.y + 5) as i32),
        HorizontalAlignment::Right,
        text_color,
    )?;
    text(
        painter,
        variant,
        FontRole::NumBig,
        format_args!("{:>2}.{:01}V", spec.mv / 1000, (spec.mv % 1000) / 100),
        Point::new((spec.x + 8) as i32, (spec.y + 23) as i32),
        HorizontalAlignment::Left,
        text_color,
    )?;
    text(
        painter,
        variant,
        FontRole::Num,
        format_args!("{:>1}.{:02}A", spec.ma / 1000, (spec.ma % 1000) / 10),
        Point::new((spec.x + spec.w - 8) as i32, (spec.y + 27) as i32),
        HorizontalAlignment::Right,
        text_color,
    )?;

    Ok(())
}

#[allow(dead_code)]
struct SmallMetricTile<T>
where
    T: Content,
{
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    title: &'static str,
    value: T,
    status: &'static str,
    active: bool,
    accent: u16,
}

#[allow(dead_code)]
fn draw_small_metric_tile<P: UiPainter, T: Content>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    spec: SmallMetricTile<T>,
) -> Result<(), P::Error> {
    draw_panel(
        painter,
        spec.x,
        spec.y,
        spec.w,
        spec.h,
        palette,
        spec.active,
        spec.accent,
    )?;

    let text_color = if spec.active {
        palette.bg
    } else {
        palette.text
    };
    text(
        painter,
        variant,
        FontRole::TextBody,
        spec.title,
        Point::new((spec.x + 6) as i32, (spec.y + 4) as i32),
        HorizontalAlignment::Left,
        text_color,
    )?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        spec.status,
        Point::new((spec.x + spec.w - 6) as i32, (spec.y + 4) as i32),
        HorizontalAlignment::Right,
        text_color,
    )?;
    text(
        painter,
        variant,
        FontRole::Num,
        spec.value,
        Point::new((spec.x + spec.w / 2) as i32, (spec.y + 18) as i32),
        HorizontalAlignment::Center,
        text_color,
    )?;

    Ok(())
}

struct HealthBlock<T>
where
    T: Content,
{
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    title: &'static str,
    value: T,
    note: &'static str,
    meter: u32,
    active: bool,
    accent: u16,
}

fn draw_health_block<P: UiPainter, T: Content>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    spec: HealthBlock<T>,
) -> Result<(), P::Error> {
    draw_panel(
        painter,
        spec.x,
        spec.y,
        spec.w,
        spec.h,
        palette,
        spec.active,
        spec.accent,
    )?;

    let text_color = if spec.active {
        palette.bg
    } else {
        palette.text
    };
    text(
        painter,
        variant,
        FontRole::TextBody,
        spec.title,
        Point::new((spec.x + 6) as i32, (spec.y + 4) as i32),
        HorizontalAlignment::Left,
        text_color,
    )?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        spec.note,
        Point::new((spec.x + spec.w - 6) as i32, (spec.y + 4) as i32),
        HorizontalAlignment::Right,
        text_color,
    )?;
    text(
        painter,
        variant,
        FontRole::Num,
        spec.value,
        Point::new((spec.x + 6) as i32, (spec.y + 19) as i32),
        HorizontalAlignment::Left,
        text_color,
    )?;
    draw_meter(
        painter,
        spec.x + 6,
        spec.y + spec.h - 9,
        spec.w - 12,
        5,
        spec.meter,
        if spec.active { palette.bg } else { spec.accent },
        if spec.active {
            fade_color(spec.accent, palette.bg)
        } else {
            palette.panel_alt
        },
    )
}

fn draw_column_headers<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    x: u16,
    y: u16,
) -> Result<(), P::Error> {
    text(
        painter,
        variant,
        FontRole::TextBody,
        "MODULE",
        Point::new(x as i32, y as i32),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        "STATE",
        Point::new((x + 84) as i32, y as i32),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        "CODE",
        Point::new((x + 168) as i32, y as i32),
        HorizontalAlignment::Right,
        palette.text,
    )?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        "READ",
        Point::new((x + 236) as i32, y as i32),
        HorizontalAlignment::Right,
        palette.text,
    )?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        "TEMP",
        Point::new((x + 292) as i32, y as i32),
        HorizontalAlignment::Right,
        palette.text,
    )?;
    Ok(())
}

struct TableRow<TV, TC, TT>
where
    TV: Content,
    TC: Content,
    TT: Content,
{
    x: u16,
    y: u16,
    h: u16,
    module: &'static str,
    status: &'static str,
    voltage: TV,
    current: TC,
    temp: TT,
    active: bool,
    accent: u16,
    odd: bool,
}

fn draw_table_row<P: UiPainter, TV: Content, TC: Content, TT: Content>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    spec: TableRow<TV, TC, TT>,
) -> Result<(), P::Error> {
    let row_bg = if spec.active {
        spec.accent
    } else if spec.odd {
        fade_color(palette.panel_alt, palette.bg)
    } else {
        palette.panel
    };

    fill(painter, spec.x, spec.y, 304, spec.h, row_bg)?;
    draw_outline(painter, spec.x, spec.y, 304, spec.h, palette.border)?;

    let text_color = if spec.active {
        palette.bg
    } else {
        palette.text
    };
    let dim_color = if spec.active {
        palette.bg
    } else {
        palette.text_dim
    };

    text(
        painter,
        variant,
        FontRole::TextBody,
        spec.module,
        Point::new((spec.x + 4) as i32, (spec.y + 3) as i32),
        HorizontalAlignment::Left,
        text_color,
    )?;
    text(
        painter,
        variant,
        FontRole::Num,
        spec.status,
        Point::new((spec.x + 112) as i32, (spec.y + 3) as i32),
        HorizontalAlignment::Left,
        dim_color,
    )?;
    text(
        painter,
        variant,
        FontRole::Num,
        spec.voltage,
        Point::new((spec.x + 190) as i32, (spec.y + 3) as i32),
        HorizontalAlignment::Right,
        text_color,
    )?;
    text(
        painter,
        variant,
        FontRole::Num,
        spec.current,
        Point::new((spec.x + 254) as i32, (spec.y + 3) as i32),
        HorizontalAlignment::Right,
        text_color,
    )?;
    text(
        painter,
        variant,
        FontRole::Num,
        spec.temp,
        Point::new((spec.x + 300) as i32, (spec.y + 3) as i32),
        HorizontalAlignment::Right,
        text_color,
    )?;

    Ok(())
}

#[allow(dead_code)]
struct ModuleChip {
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    label: &'static str,
    active: bool,
    accent: u16,
}

#[allow(dead_code)]
fn draw_module_chip<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    spec: ModuleChip,
) -> Result<(), P::Error> {
    draw_panel(
        painter,
        spec.x,
        spec.y,
        spec.w,
        spec.h,
        palette,
        spec.active,
        spec.accent,
    )?;

    text(
        painter,
        variant,
        FontRole::TextBody,
        spec.label,
        Point::new((spec.x + spec.w / 2) as i32, (spec.y + 4) as i32),
        HorizontalAlignment::Center,
        if spec.active {
            palette.bg
        } else {
            palette.text
        },
    )
}

#[allow(dead_code)]
fn render_focus_center_value<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    data: DashboardData,
) -> Result<(), P::Error> {
    match data.focus {
        UiFocus::Up => {
            text(
                painter,
                variant,
                FontRole::NumBig,
                format_args!(
                    "{:>2}.{:01}V",
                    data.out_a_mv / 1000,
                    (data.out_a_mv % 1000) / 100
                ),
                Point::new(92, 52),
                HorizontalAlignment::Left,
                palette.text,
            )?;
            text(
                painter,
                variant,
                FontRole::Num,
                format_args!(
                    "OUT-A {:>1}.{:02}A",
                    data.out_a_ma / 1000,
                    (data.out_a_ma % 1000) / 10
                ),
                Point::new(92, 84),
                HorizontalAlignment::Left,
                palette.text_dim,
            )?;
        }
        UiFocus::Down => {
            text(
                painter,
                variant,
                FontRole::NumBig,
                format_args!(
                    "{:>2}.{:01}V",
                    data.out_b_mv / 1000,
                    (data.out_b_mv % 1000) / 100
                ),
                Point::new(92, 52),
                HorizontalAlignment::Left,
                palette.text,
            )?;
            text(
                painter,
                variant,
                FontRole::Num,
                format_args!(
                    "OUT-B {:>1}.{:02}A",
                    data.out_b_ma / 1000,
                    (data.out_b_ma % 1000) / 10
                ),
                Point::new(92, 84),
                HorizontalAlignment::Left,
                palette.text_dim,
            )?;
        }
        UiFocus::Left => {
            text(
                painter,
                variant,
                FontRole::NumBig,
                format_args!("{:>2}%", data.bms_soc_pct),
                Point::new(92, 52),
                HorizontalAlignment::Left,
                palette.text,
            )?;
            text(
                painter,
                variant,
                FontRole::Num,
                "BMS BALANCE",
                Point::new(92, 84),
                HorizontalAlignment::Left,
                palette.text_dim,
            )?;
        }
        UiFocus::Right => {
            text(
                painter,
                variant,
                FontRole::NumBig,
                format_args!(
                    "{:>1}.{:02}A",
                    data.chg_iin_ma / 1000,
                    (data.chg_iin_ma % 1000) / 10
                ),
                Point::new(92, 52),
                HorizontalAlignment::Left,
                palette.text,
            )?;
            text(
                painter,
                variant,
                FontRole::Num,
                "CHARGER INPUT",
                Point::new(92, 84),
                HorizontalAlignment::Left,
                palette.text_dim,
            )?;
        }
        UiFocus::Center => {
            text(
                painter,
                variant,
                FontRole::NumBig,
                format_args!("{:02}/{:02}C", data.therm_a_c, data.therm_b_c),
                Point::new(92, 52),
                HorizontalAlignment::Left,
                palette.text,
            )?;
            text(
                painter,
                variant,
                FontRole::Num,
                "THERM A / B",
                Point::new(92, 84),
                HorizontalAlignment::Left,
                palette.text_dim,
            )?;
        }
        UiFocus::Touch => {
            text(
                painter,
                variant,
                FontRole::TextTitle,
                if data.touch_irq {
                    "IRQ ACTIVE"
                } else {
                    "IRQ CLEAR"
                },
                Point::new(92, 56),
                HorizontalAlignment::Left,
                if data.touch_irq {
                    palette.touch
                } else {
                    palette.text
                },
            )?;
            text(
                painter,
                variant,
                FontRole::TextBody,
                "TOUCH INTERRUPT EVENT",
                Point::new(92, 86),
                HorizontalAlignment::Left,
                palette.text_dim,
            )?;
        }
        UiFocus::Idle => {
            text(
                painter,
                variant,
                FontRole::NumBig,
                format_args!("{:>2}%", data.bms_soc_pct),
                Point::new(92, 52),
                HorizontalAlignment::Left,
                palette.text,
            )?;
            text(
                painter,
                variant,
                FontRole::TextBody,
                "SYSTEM READY / IDLE",
                Point::new(92, 86),
                HorizontalAlignment::Left,
                palette.text_dim,
            )?;
        }
    }

    draw_meter(
        painter,
        92,
        112,
        138,
        8,
        if data.alert_on {
            100
        } else {
            data.bms_soc_pct as u32
        },
        if data.alert_on {
            palette.touch
        } else {
            palette.accent
        },
        palette.panel_alt,
    )
}

#[allow(dead_code)]
struct RightStat<T>
where
    T: Content,
{
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    label: &'static str,
    value: T,
    active: bool,
    accent: u16,
}

#[allow(dead_code)]
fn draw_right_stat<P: UiPainter, T: Content>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    spec: RightStat<T>,
) -> Result<(), P::Error> {
    draw_panel(
        painter,
        spec.x,
        spec.y,
        spec.w,
        spec.h,
        palette,
        spec.active,
        spec.accent,
    )?;

    let text_color = if spec.active {
        palette.bg
    } else {
        palette.text
    };
    text(
        painter,
        variant,
        FontRole::TextBody,
        spec.label,
        Point::new((spec.x + 4) as i32, (spec.y + 4) as i32),
        HorizontalAlignment::Left,
        text_color,
    )?;
    text(
        painter,
        variant,
        FontRole::Num,
        spec.value,
        Point::new((spec.x + spec.w - 4) as i32, (spec.y + 20) as i32),
        HorizontalAlignment::Right,
        text_color,
    )
}

fn draw_top_bar<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    focus: UiFocus,
    title: &'static str,
    subtitle: &'static str,
) -> Result<(), P::Error> {
    draw_top_bar_with_status(
        painter,
        variant,
        palette,
        focus,
        title,
        subtitle,
        focus_tag(focus),
        focus_color(palette, focus),
    )
}

fn draw_top_bar_with_status<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    _focus: UiFocus,
    title: &'static str,
    subtitle: &'static str,
    status_tag: &'static str,
    status_color: u16,
) -> Result<(), P::Error> {
    fill(painter, 0, 0, UI_W, HEADER_H, palette.panel)?;
    text(
        painter,
        variant,
        FontRole::TextTitle,
        title,
        Point::new(8, 2),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        subtitle,
        Point::new(106, 2),
        HorizontalAlignment::Left,
        palette.text_dim,
    )?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        status_tag,
        Point::new((UI_W - 8) as i32, 2),
        HorizontalAlignment::Right,
        status_color,
    )
}

fn draw_bottom_bar<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    hint: &'static str,
) -> Result<(), P::Error> {
    fill(painter, 0, UI_H - FOOTER_H, UI_W, FOOTER_H, palette.panel)?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        hint,
        Point::new(8, (UI_H - 14) as i32),
        HorizontalAlignment::Left,
        palette.text_dim,
    )
}

fn draw_panel<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    palette: Palette,
    active: bool,
    accent: u16,
) -> Result<(), P::Error> {
    let border = if active { accent } else { palette.border };
    let fill_color = if active {
        accent
    } else {
        fade_color(palette.panel, palette.panel_alt)
    };

    fill(painter, x, y, w, h, border)?;
    if w > 2 && h > 2 {
        fill(painter, x + 1, y + 1, w - 2, h - 2, fill_color)?;
    }
    Ok(())
}

fn draw_background_grid<P: UiPainter>(painter: &mut P, palette: Palette) -> Result<(), P::Error> {
    let body_top = HEADER_H;
    let body_bottom = UI_H - FOOTER_H;
    let line = fade_color(palette.bg, palette.panel);

    let mut y = body_top + 6;
    while y < body_bottom {
        fill(painter, 1, y, UI_W - 2, 1, line)?;
        y = y.saturating_add(14);
    }

    let mut x = 8;
    while x < UI_W - 8 {
        fill(
            painter,
            x,
            body_top + 1,
            1,
            body_bottom - body_top - 2,
            line,
        )?;
        x = x.saturating_add(16);
    }

    Ok(())
}

fn draw_meter<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    percent: u32,
    fg: u16,
    bg: u16,
) -> Result<(), P::Error> {
    if w < 3 || h < 3 {
        return Ok(());
    }

    fill(painter, x, y, w, h, bg)?;
    fill(painter, x + 1, y + 1, w - 2, h - 2, fade_color(bg, 0x0000))?;

    let inner_w = w - 2;
    let fill_w = ((inner_w as u32) * percent.min(100) / 100) as u16;
    if fill_w > 0 {
        fill(painter, x + 1, y + 1, fill_w, h - 2, fg)?;
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
    fill(painter, x + w.saturating_sub(1), y, 1, h, color)
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

fn text<P: UiPainter>(
    painter: &mut P,
    _variant: UiVariant,
    role: FontRole,
    content: impl Content,
    anchor: Point,
    align: HorizontalAlignment,
    color: u16,
) -> Result<(), P::Error> {
    let renderer = match role {
        FontRole::TextTitle => &FONT_A_TITLE,
        FontRole::TextBody => &FONT_A_BODY,
        FontRole::Num => &FONT_B_NUM,
        FontRole::NumBig => &FONT_B_NUM_BIG,
    };

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

fn focus_tag(focus: UiFocus) -> &'static str {
    match focus {
        UiFocus::Idle => "IDLE",
        UiFocus::Up => "OUT-A",
        UiFocus::Down => "OUT-B",
        UiFocus::Left => "BMS",
        UiFocus::Right => "CHARGER",
        UiFocus::Center => "THERM",
        UiFocus::Touch => "ALERT",
    }
}

fn focus_color(palette: Palette, focus: UiFocus) -> u16 {
    match focus {
        UiFocus::Idle => palette.accent,
        UiFocus::Up => palette.up,
        UiFocus::Down => palette.down,
        UiFocus::Left => palette.left,
        UiFocus::Right => palette.right,
        UiFocus::Center => palette.center,
        UiFocus::Touch => palette.touch,
    }
}

fn fade_color(a: u16, b: u16) -> u16 {
    let ar = (a >> 11) & 0x1f;
    let ag = (a >> 5) & 0x3f;
    let ab = a & 0x1f;

    let br = (b >> 11) & 0x1f;
    let bg = (b >> 5) & 0x3f;
    let bb = b & 0x1f;

    let r = ((ar as u32 + br as u32) / 2) as u16;
    let g = ((ag as u32 + bg as u32) / 2) as u16;
    let bl = ((ab as u32 + bb as u32) / 2) as u16;

    (r << 11) | (g << 5) | bl
}

fn palette_for(variant: UiVariant) -> Palette {
    match variant {
        UiVariant::InstrumentA => Palette {
            bg: 0x08A4,
            panel: 0x1106,
            panel_alt: 0x1969,
            border: 0x4AEF,
            text: 0xFFFF,
            text_dim: 0xADB8,
            accent: 0x4E1E,
            up: 0x2533,
            down: 0x2DBE,
            left: 0x65CD,
            right: 0xFDA9,
            center: 0xFEA9,
            touch: 0xEA8A,
        },
        UiVariant::InstrumentB => Palette {
            bg: 0x10C4,
            panel: 0x1905,
            panel_alt: 0x2167,
            border: 0x5B0E,
            text: 0xFFFF,
            text_dim: 0xB5F8,
            accent: 0x8D37,
            up: 0x7D34,
            down: 0x6CF8,
            left: 0x8D91,
            right: 0xCD2F,
            center: 0xCD8F,
            touch: 0xB410,
        },
        UiVariant::RetroC => Palette {
            bg: 0x0044,
            panel: 0x0867,
            panel_alt: 0x10A9,
            border: 0x8C51,
            text: 0xFFFF,
            text_dim: 0xBDF7,
            accent: 0xFF20,
            up: 0x07FF,
            down: 0x47FF,
            left: 0xAFDF,
            right: 0xFD00,
            center: 0xFFD0,
            touch: 0xF800,
        },
        UiVariant::InstrumentD => Palette {
            bg: 0x18A2,
            panel: 0x2903,
            panel_alt: 0x3144,
            border: 0x7B4B,
            text: 0xFFFF,
            text_dim: 0xD679,
            accent: 0x4DB5,
            up: 0x8658,
            down: 0x4E1E,
            left: 0xAEB0,
            right: 0xFE70,
            center: 0xFDA9,
            touch: 0xE38E,
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
