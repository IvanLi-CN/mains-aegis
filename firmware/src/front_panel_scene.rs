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
    pub mode: UpsMode,
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
const FOOTER_H: u16 = 0;

// User preference: non-numeric text uses Font A, numeric fields use fixed-width Font B.
static FONT_A_TITLE: FontRenderer = FontRenderer::new::<fonts::u8g2_font_8x13B_tf>();
static FONT_A_BODY: FontRenderer = FontRenderer::new::<fonts::u8g2_font_7x14B_tf>();
static FONT_B_NUM: FontRenderer = FontRenderer::new::<fonts::u8g2_font_8x13_mf>();
static FONT_B_NUM_BIG: FontRenderer = FontRenderer::new::<fonts::u8g2_font_t0_22b_tn>();
// Compact roles intentionally reuse >=10px fonts to enforce minimum glyph height.

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
    TextCompact,
    Num,
    NumCompact,
    NumBig,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpsMode {
    Off,
    Standby,
    Supplement,
    Backup,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelfCheckCommState {
    Pending,
    Ok,
    Warn,
    Err,
    NotAvailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelfCheckUiSnapshot {
    pub mode: UpsMode,
    pub gc9307: SelfCheckCommState,
    pub tca6408a: SelfCheckCommState,
    pub fusb302: SelfCheckCommState,
    pub fusb302_vbus_present: Option<bool>,
    pub ina3221: SelfCheckCommState,
    pub ina_total_ma: Option<i32>,
    pub bq25792: SelfCheckCommState,
    pub bq25792_allow_charge: Option<bool>,
    pub bq25792_ichg_ma: Option<u16>,
    pub bq40z50: SelfCheckCommState,
    pub bq40z50_soc_pct: Option<u16>,
    pub bq40z50_rca_alarm: Option<bool>,
    pub tps_a: SelfCheckCommState,
    pub tps_a_enabled: Option<bool>,
    pub tps_a_iout_ma: Option<i32>,
    pub tps_b: SelfCheckCommState,
    pub tps_b_enabled: Option<bool>,
    pub tps_b_iout_ma: Option<i32>,
    pub tmp_a: SelfCheckCommState,
    pub tmp_a_c: Option<i16>,
    pub tmp_b: SelfCheckCommState,
    pub tmp_b_c: Option<i16>,
}

impl SelfCheckUiSnapshot {
    pub const fn pending(mode: UpsMode) -> Self {
        Self {
            mode,
            gc9307: SelfCheckCommState::Pending,
            tca6408a: SelfCheckCommState::Pending,
            fusb302: SelfCheckCommState::Pending,
            fusb302_vbus_present: None,
            ina3221: SelfCheckCommState::Pending,
            ina_total_ma: None,
            bq25792: SelfCheckCommState::Pending,
            bq25792_allow_charge: None,
            bq25792_ichg_ma: None,
            bq40z50: SelfCheckCommState::Pending,
            bq40z50_soc_pct: None,
            bq40z50_rca_alarm: None,
            tps_a: SelfCheckCommState::Pending,
            tps_a_enabled: None,
            tps_a_iout_ma: None,
            tps_b: SelfCheckCommState::Pending,
            tps_b_enabled: None,
            tps_b_iout_ma: None,
            tmp_a: SelfCheckCommState::Pending,
            tmp_a_c: None,
            tmp_b: SelfCheckCommState::Pending,
            tmp_b_c: None,
        }
    }
}

#[allow(dead_code)]
pub const fn demo_mode_from_focus(focus: UiFocus) -> UpsMode {
    match focus {
        UiFocus::Center => UpsMode::Off,
        UiFocus::Idle | UiFocus::Left => UpsMode::Standby,
        UiFocus::Up | UiFocus::Right => UpsMode::Supplement,
        UiFocus::Down | UiFocus::Touch => UpsMode::Backup,
    }
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
struct DashboardData {
    mode: UpsMode,
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
    load_ma: u16,
    batt_pack_mv: u16,
    bms_balancing: bool,
    bms_soc_pct: u16,
    therm_a_c: u16,
    therm_b_c: u16,
}

impl DashboardData {
    fn from_model(model: &UiModel) -> Self {
        let mode = model.mode;
        let out_a_on = matches!(
            mode,
            UpsMode::Standby | UpsMode::Supplement | UpsMode::Backup
        );
        let out_b_on = matches!(mode, UpsMode::Supplement | UpsMode::Backup);
        let bms_on = model.focus == UiFocus::Left;
        let chg_on = model.focus == UiFocus::Right;
        let therm_on = model.focus == UiFocus::Center;
        let mains_present = mode_is_mains(mode);
        let alert_on = model.focus == UiFocus::Touch || model.touch_irq;
        let charge_enabled = matches!(mode, UpsMode::Standby);
        let wave = (model.frame_no % 6) as u16;

        let (out_a_ma, out_b_ma, load_ma) = match mode {
            UpsMode::Off => (0, 0, 1_360 + wave * 8),
            UpsMode::Standby => (0, 0, 1_120 + wave * 8),
            UpsMode::Supplement => (560 + wave * 6, 480 + wave * 5, 1_860 + wave * 9),
            UpsMode::Backup => (980 + wave * 7, 920 + wave * 6, 1_900 + wave * 8),
        };

        Self {
            mode,
            focus: model.focus,
            touch_irq: model.touch_irq,
            mains_present,
            out_a_on,
            out_b_on,
            bms_on,
            chg_on,
            therm_on,
            alert_on,
            out_a_mv: if matches!(mode, UpsMode::Backup) {
                18_850 + wave * 8
            } else {
                19_020 + wave * 6
            },
            out_a_ma,
            out_b_mv: if matches!(mode, UpsMode::Backup) {
                18_820 + wave * 8
            } else {
                19_010 + wave * 5
            },
            out_b_ma,
            chg_iin_ma: if charge_enabled { 320 + wave * 3 } else { 0 },
            load_ma,
            batt_pack_mv: if matches!(mode, UpsMode::Backup) {
                14_800 + wave * 12
            } else if matches!(mode, UpsMode::Supplement) {
                14_960 + wave * 10
            } else {
                15_200 + wave * 12
            },
            bms_balancing: model.focus == UiFocus::Left && !matches!(mode, UpsMode::Off),
            bms_soc_pct: if matches!(mode, UpsMode::Backup) {
                56 + (wave % 5)
            } else if matches!(mode, UpsMode::Supplement) {
                59 + (wave % 5)
            } else {
                61 + (wave % 5)
            },
            therm_a_c: if therm_on {
                52 + (wave % 2)
            } else if matches!(mode, UpsMode::Supplement | UpsMode::Backup) {
                40 + (wave % 2)
            } else {
                37 + (wave % 2)
            },
            therm_b_c: if therm_on {
                50 + (wave % 2)
            } else if matches!(mode, UpsMode::Supplement | UpsMode::Backup) {
                39 + (wave % 2)
            } else {
                35 + (wave % 2)
            },
        }
    }
}

fn mode_label(mode: UpsMode) -> &'static str {
    match mode {
        UpsMode::Off => "BYPASS",
        UpsMode::Standby => "STANDBY",
        UpsMode::Supplement => "ASSIST",
        UpsMode::Backup => "BACKUP",
    }
}

fn mode_accent_color(palette: Palette, mode: UpsMode, touch_irq: bool) -> u16 {
    if touch_irq {
        return palette.touch;
    }
    match mode {
        UpsMode::Off => palette.text_dim,
        UpsMode::Standby => palette.right,
        UpsMode::Supplement => palette.accent,
        UpsMode::Backup => palette.down,
    }
}

fn mode_is_mains(mode: UpsMode) -> bool {
    !matches!(mode, UpsMode::Backup)
}

#[allow(dead_code)]
pub fn render_frame<P: UiPainter>(
    painter: &mut P,
    model: &UiModel,
    variant: UiVariant,
) -> Result<(), P::Error> {
    render_frame_with_self_check(painter, model, variant, None)
}

pub fn render_frame_with_self_check<P: UiPainter>(
    painter: &mut P,
    model: &UiModel,
    variant: UiVariant,
    self_check: Option<&SelfCheckUiSnapshot>,
) -> Result<(), P::Error> {
    let palette = palette_for(variant);
    let data = DashboardData::from_model(model);

    fill(painter, 0, 0, UI_W, UI_H, palette.bg)?;
    draw_background_grid(painter, palette)?;
    draw_outline(painter, 0, 0, UI_W, UI_H, palette.border)?;

    match variant {
        UiVariant::InstrumentA => render_variant_a(painter, variant, palette, data)?,
        UiVariant::InstrumentB => render_variant_b(painter, variant, palette, data)?,
        UiVariant::RetroC => render_variant_c(painter, variant, palette, data, self_check)?,
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
    let kpi_label_y = 27;
    let kpi_value_y = 44;

    let load_ma = data.load_ma as u32;
    let tps_out_ma = data.out_a_ma as u32 + data.out_b_ma as u32;
    let bus_mv = ((data.out_a_mv as u32 + data.out_b_mv as u32) / 2) as u16;
    let charge_batt_ma = if matches!(data.mode, UpsMode::Standby) {
        data.chg_iin_ma as u32
    } else {
        0
    };
    let input_current_ma = if data.mains_present {
        match data.mode {
            UpsMode::Off => load_ma,
            UpsMode::Standby => load_ma + charge_batt_ma,
            UpsMode::Supplement => {
                let supplement_ma = tps_out_ma.min(load_ma.saturating_sub(120));
                load_ma.saturating_sub(supplement_ma)
            }
            UpsMode::Backup => 0,
        }
    } else {
        0
    };
    let output_current_ma = load_ma.max(120);
    let batt_discharge_ma = match data.mode {
        UpsMode::Off | UpsMode::Standby => 0,
        UpsMode::Supplement => tps_out_ma.min(load_ma),
        UpsMode::Backup => load_ma,
    };
    let input_power_w10 = ((bus_mv as u32) * input_current_ma) / 100_000;
    let output_power_w10 = ((bus_mv as u32) * output_current_ma) / 100_000;

    let mode_accent = mode_accent_color(palette, data.mode, data.touch_irq);
    let mode_tag = if data.touch_irq {
        "IRQ ON"
    } else {
        mode_label(data.mode)
    };
    draw_top_bar_with_status(
        painter,
        variant,
        palette,
        data.focus,
        "UPS DASHBOARD",
        "",
        mode_tag,
        mode_accent,
    )?;

    draw_panel(painter, 6, 22, 196, 52, palette, true, mode_accent)?;
    if data.mains_present {
        text(
            painter,
            variant,
            FontRole::TextBody,
            "PIN W",
            Point::new(14, kpi_label_y),
            HorizontalAlignment::Left,
            palette.bg,
        )?;
        text(
            painter,
            variant,
            FontRole::TextBody,
            "POUT W",
            Point::new(194, kpi_label_y),
            HorizontalAlignment::Right,
            palette.bg,
        )?;
        text(
            painter,
            variant,
            FontRole::NumBig,
            format_args!("{:>2}.{:01}", input_power_w10 / 10, input_power_w10 % 10),
            Point::new(14, kpi_value_y),
            HorizontalAlignment::Left,
            palette.bg,
        )?;
        text(
            painter,
            variant,
            FontRole::NumBig,
            format_args!("{:>2}.{:01}", output_power_w10 / 10, output_power_w10 % 10),
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

    draw_panel(painter, 6, 76, 196, 94, palette, false, palette.accent)?;
    match data.mode {
        UpsMode::Off => {
            text(
                painter,
                variant,
                FontRole::TextBody,
                "BYPASS ACTIVE",
                Point::new(14, 81),
                HorizontalAlignment::Left,
                palette.text,
            )?;
            text(
                painter,
                variant,
                FontRole::TextBody,
                "TPS OUT",
                Point::new(14, 108),
                HorizontalAlignment::Left,
                palette.text_dim,
            )?;
            text(
                painter,
                variant,
                FontRole::Num,
                "0.00A",
                Point::new(194, 108),
                HorizontalAlignment::Right,
                palette.text,
            )?;
            text(
                painter,
                variant,
                FontRole::TextBody,
                "BAT CHG",
                Point::new(14, 132),
                HorizontalAlignment::Left,
                palette.text_dim,
            )?;
            text(
                painter,
                variant,
                FontRole::Num,
                "LOCK",
                Point::new(194, 132),
                HorizontalAlignment::Right,
                palette.text,
            )?;
            draw_meter(
                painter,
                14,
                154,
                180,
                6,
                (output_power_w10 * 100 / 380).min(100),
                palette.text_dim,
                palette.panel_alt,
            )?;
        }
        UpsMode::Standby => {
            text(
                painter,
                variant,
                FontRole::TextBody,
                "STANDBY CHARGE",
                Point::new(14, 81),
                HorizontalAlignment::Left,
                palette.text,
            )?;
            text(
                painter,
                variant,
                FontRole::TextBody,
                "TPS OUT",
                Point::new(14, 108),
                HorizontalAlignment::Left,
                palette.text_dim,
            )?;
            text(
                painter,
                variant,
                FontRole::Num,
                "0.00A",
                Point::new(194, 108),
                HorizontalAlignment::Right,
                palette.text,
            )?;
            text(
                painter,
                variant,
                FontRole::TextBody,
                "BAT CHG",
                Point::new(14, 132),
                HorizontalAlignment::Left,
                palette.text_dim,
            )?;
            text(
                painter,
                variant,
                FontRole::Num,
                format_args!(
                    "{:>1}.{:02}A",
                    (charge_batt_ma as u16) / 1000,
                    ((charge_batt_ma as u16) % 1000) / 10
                ),
                Point::new(194, 132),
                HorizontalAlignment::Right,
                palette.text,
            )?;
            draw_meter(
                painter,
                14,
                154,
                180,
                6,
                (charge_batt_ma * 100 / 1200).min(100),
                palette.right,
                palette.panel_alt,
            )?;
        }
        UpsMode::Supplement => {
            text(
                painter,
                variant,
                FontRole::TextBody,
                "ASSIST",
                Point::new(14, 81),
                HorizontalAlignment::Left,
                palette.text,
            )?;
            text(
                painter,
                variant,
                FontRole::TextBody,
                "TPS OUT",
                Point::new(14, 108),
                HorizontalAlignment::Left,
                palette.text_dim,
            )?;
            text(
                painter,
                variant,
                FontRole::Num,
                format_args!(
                    "{:>1}.{:02}A",
                    (tps_out_ma as u16) / 1000,
                    ((tps_out_ma as u16) % 1000) / 10
                ),
                Point::new(194, 108),
                HorizontalAlignment::Right,
                palette.text,
            )?;
            text(
                painter,
                variant,
                FontRole::TextBody,
                "BAT CHG",
                Point::new(14, 132),
                HorizontalAlignment::Left,
                palette.text_dim,
            )?;
            text(
                painter,
                variant,
                FontRole::Num,
                "LOCK",
                Point::new(194, 132),
                HorizontalAlignment::Right,
                palette.text,
            )?;
            draw_meter(
                painter,
                14,
                154,
                180,
                6,
                ((tps_out_ma * 100) / output_current_ma).min(100),
                palette.accent,
                palette.panel_alt,
            )?;
        }
        UpsMode::Backup => {
            text(
                painter,
                variant,
                FontRole::TextBody,
                "OUTPUT",
                Point::new(14, 81),
                HorizontalAlignment::Left,
                palette.text,
            )?;
            text(
                painter,
                variant,
                FontRole::TextBody,
                "VOUT",
                Point::new(14, 102),
                HorizontalAlignment::Left,
                palette.text_dim,
            )?;
            text(
                painter,
                variant,
                FontRole::Num,
                format_args!("{:>2}.{:01}V", bus_mv / 1000, (bus_mv % 1000) / 100),
                Point::new(194, 102),
                HorizontalAlignment::Right,
                palette.text,
            )?;
            text(
                painter,
                variant,
                FontRole::TextBody,
                "TEMP",
                Point::new(14, 126),
                HorizontalAlignment::Left,
                palette.text_dim,
            )?;
            text(
                painter,
                variant,
                FontRole::Num,
                format_args!("{:02}/{:02}C", data.therm_a_c, data.therm_b_c),
                Point::new(194, 126),
                HorizontalAlignment::Right,
                palette.text,
            )?;
            text(
                painter,
                variant,
                FontRole::TextBody,
                "SOC",
                Point::new(14, 150),
                HorizontalAlignment::Left,
                palette.text_dim,
            )?;
            text(
                painter,
                variant,
                FontRole::Num,
                format_args!("{:>2}%", data.bms_soc_pct),
                Point::new(194, 150),
                HorizontalAlignment::Right,
                palette.text,
            )?;
        }
    }

    let batt_status = if data.bms_balancing {
        "BAL"
    } else {
        match data.mode {
            UpsMode::Off => "BYP",
            UpsMode::Standby => {
                if charge_batt_ma > 80 {
                    "CHG"
                } else {
                    "IDLE"
                }
            }
            UpsMode::Supplement => "DSG",
            UpsMode::Backup => {
                if data.bms_soc_pct <= 20 {
                    "LOW"
                } else {
                    "DSG"
                }
            }
        }
    };
    let charge_status = match data.mode {
        UpsMode::Standby => {
            if charge_batt_ma > 80 {
                "CHG"
            } else {
                "READY"
            }
        }
        UpsMode::Backup => "NOAC",
        UpsMode::Off | UpsMode::Supplement => "LOCK",
    };
    let discharge_status = match data.mode {
        UpsMode::Off => "BYP",
        UpsMode::Standby => "IDLE",
        UpsMode::Supplement => "ASSIST",
        UpsMode::Backup => "LOAD",
    };

    let batt_max_c = data.therm_a_c.max(data.therm_b_c);
    draw_health_block(
        painter,
        variant,
        palette,
        HealthBlock {
            x: 206,
            y: 22,
            w: 108,
            h: 48,
            title: "BATTERY",
            value: format_args!("{:>2}% {:02}C", data.bms_soc_pct, batt_max_c),
            note: batt_status,
            meter: data.bms_soc_pct as u32,
            active: data.bms_on,
            accent: palette.left,
        },
    )?;
    if matches!(data.mode, UpsMode::Standby) {
        draw_health_block(
            painter,
            variant,
            palette,
            HealthBlock {
                x: 206,
                y: 72,
                w: 108,
                h: 48,
                title: "CHARGE",
                value: format_args!(
                    "{:>1}.{:02}A {:02}C",
                    (charge_batt_ma as u16) / 1000,
                    ((charge_batt_ma as u16) % 1000) / 10,
                    batt_max_c
                ),
                note: charge_status,
                meter: (charge_batt_ma * 100 / 1200).min(100),
                active: true,
                accent: palette.right,
            },
        )?;
    } else {
        draw_health_block(
            painter,
            variant,
            palette,
            HealthBlock {
                x: 206,
                y: 72,
                w: 108,
                h: 48,
                title: "CHARGE",
                value: format_args!("{:>1}.{:02}A {:02}C", 0, 0, batt_max_c),
                note: charge_status,
                meter: 0,
                active: data.chg_on,
                accent: palette.right,
            },
        )?;
    }
    draw_health_block(
        painter,
        variant,
        palette,
        HealthBlock {
            x: 206,
            y: 122,
            w: 108,
            h: 48,
            title: "DISCHG",
            value: format_args!(
                "{:>1}.{:02}A {:02}C",
                (batt_discharge_ma as u16) / 1000,
                ((batt_discharge_ma as u16) % 1000) / 10,
                batt_max_c
            ),
            note: discharge_status,
            meter: (batt_discharge_ma * 100 / 2200).min(100),
            active: matches!(data.mode, UpsMode::Supplement | UpsMode::Backup),
            accent: if data.mains_present {
                palette.accent
            } else {
                palette.down
            },
        },
    )?;
    Ok(())
}

fn render_variant_c<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    data: DashboardData,
    self_check: Option<&SelfCheckUiSnapshot>,
) -> Result<(), P::Error> {
    let snapshot = self_check.copied().unwrap_or_else(|| {
        let mut fallback = SelfCheckUiSnapshot::pending(data.mode);
        fallback.gc9307 = SelfCheckCommState::Ok;
        fallback.tca6408a = if data.touch_irq {
            SelfCheckCommState::Warn
        } else {
            SelfCheckCommState::Ok
        };
        fallback.fusb302 = SelfCheckCommState::Warn;
        fallback.fusb302_vbus_present = Some(data.mains_present);
        fallback.ina3221 = SelfCheckCommState::Warn;
        fallback.ina_total_ma = None;
        fallback.bq25792 = SelfCheckCommState::Warn;
        fallback.bq25792_allow_charge = Some(matches!(data.mode, UpsMode::Standby));
        fallback.bq40z50 = SelfCheckCommState::Warn;
        fallback.bq40z50_soc_pct = None;
        fallback.tps_a = if data.out_a_on {
            SelfCheckCommState::Warn
        } else {
            SelfCheckCommState::NotAvailable
        };
        fallback.tps_a_enabled = Some(data.out_a_on);
        fallback.tps_b = if data.out_b_on {
            SelfCheckCommState::Warn
        } else {
            SelfCheckCommState::NotAvailable
        };
        fallback.tps_b_enabled = Some(data.out_b_on);
        fallback.tmp_a = SelfCheckCommState::Warn;
        fallback.tmp_b = SelfCheckCommState::Warn;
        fallback
    });

    let mode_accent = mode_accent_color(palette, snapshot.mode, data.touch_irq);
    draw_top_bar_with_status(
        painter,
        variant,
        palette,
        data.focus,
        "SELF CHECK",
        "",
        mode_label(snapshot.mode),
        mode_accent,
    )?;

    let col_left_x = 6;
    let col_right_x = 163;
    let col_w = 151;
    let row_h = 29;
    let start_y = 22;

    let ina_has = snapshot.ina_total_ma.is_some();
    let ina_ma = snapshot.ina_total_ma.unwrap_or_default();
    let ina_abs = ina_ma.wrapping_abs() as u32;
    let ina_sign = if ina_ma < 0 { "-" } else { "" };
    let ina_whole = ina_abs / 1000;
    let ina_frac = (ina_abs % 1000) / 10;

    let ichg_has = snapshot.bq25792_ichg_ma.is_some();
    let ichg_ma = snapshot.bq25792_ichg_ma.unwrap_or(0);
    let ichg_whole = ichg_ma / 1000;
    let ichg_frac = (ichg_ma % 1000) / 10;

    let bms_soc_has = snapshot.bq40z50_soc_pct.is_some();
    let bms_soc = snapshot.bq40z50_soc_pct.unwrap_or(0);

    let tps_a_has = snapshot.tps_a_iout_ma.is_some();
    let tps_a_ma = snapshot.tps_a_iout_ma.unwrap_or_default();
    let tps_a_abs = tps_a_ma.wrapping_abs() as u32;
    let tps_a_sign = if tps_a_ma < 0 { "-" } else { "" };
    let tps_a_whole = tps_a_abs / 1000;
    let tps_a_frac = (tps_a_abs % 1000) / 10;

    let tps_b_has = snapshot.tps_b_iout_ma.is_some();
    let tps_b_ma = snapshot.tps_b_iout_ma.unwrap_or_default();
    let tps_b_abs = tps_b_ma.wrapping_abs() as u32;
    let tps_b_sign = if tps_b_ma < 0 { "-" } else { "" };
    let tps_b_whole = tps_b_abs / 1000;
    let tps_b_frac = (tps_b_abs % 1000) / 10;

    let tmp_a_has = snapshot.tmp_a_c.is_some();
    let tmp_a_c = snapshot.tmp_a_c.unwrap_or(0);
    let tmp_b_has = snapshot.tmp_b_c.is_some();
    let tmp_b_c = snapshot.tmp_b_c.unwrap_or(0);

    let ina_key = if ina_has {
        format_args!("ISUM {}{:>1}.{:02}A", ina_sign, ina_whole, ina_frac)
    } else {
        format_args!("ISUM N/A")
    };
    let chg_key = if snapshot.bq25792_allow_charge == Some(false) {
        format_args!("CHG DISABLED")
    } else if ichg_has {
        format_args!("ICHG {:>1}.{:02}A", ichg_whole, ichg_frac)
    } else {
        format_args!("ICHG N/A")
    };
    let bms_key = if snapshot.bq40z50_rca_alarm == Some(true) {
        format_args!("RCA ALARM")
    } else if bms_soc_has {
        format_args!("SOC {:>2}%", bms_soc)
    } else {
        format_args!("SOC N/A")
    };
    let tps_a_key = if tps_a_has {
        format_args!("IOUT {}{:>1}.{:02}A", tps_a_sign, tps_a_whole, tps_a_frac)
    } else {
        format_args!("IOUT N/A")
    };
    let tps_b_key = if tps_b_has {
        format_args!("IOUT {}{:>1}.{:02}A", tps_b_sign, tps_b_whole, tps_b_frac)
    } else {
        format_args!("IOUT N/A")
    };
    let tmp_a_key = if tmp_a_has {
        format_args!("TMAX {:>2}C", tmp_a_c)
    } else {
        format_args!("TMAX N/A")
    };
    let tmp_b_key = if tmp_b_has {
        format_args!("TMAX {:>2}C", tmp_b_c)
    } else {
        format_args!("TMAX N/A")
    };

    draw_diag_card(
        painter,
        variant,
        palette,
        DiagCard {
            x: col_left_x,
            y: start_y,
            w: col_w,
            h: row_h,
            module: "GC9307",
            status: comm_label(snapshot.gc9307),
            key: "RGB565 320x172",
            active: false,
            accent: palette.accent,
        },
    )?;
    draw_diag_card(
        painter,
        variant,
        palette,
        DiagCard {
            x: col_left_x,
            y: start_y + row_h,
            w: col_w,
            h: row_h,
            module: "TCA6408A",
            status: comm_label(snapshot.tca6408a),
            key: "I2C2 ADDR 0x21",
            active: data.focus == UiFocus::Touch || data.touch_irq,
            accent: palette.touch,
        },
    )?;
    draw_diag_card(
        painter,
        variant,
        palette,
        DiagCard {
            x: col_left_x,
            y: start_y + row_h * 2,
            w: col_w,
            h: row_h,
            module: "FUSB302",
            status: comm_label(snapshot.fusb302),
            key: vbus_key_text(snapshot.fusb302_vbus_present),
            active: false,
            accent: palette.accent,
        },
    )?;
    draw_diag_card(
        painter,
        variant,
        palette,
        DiagCard {
            x: col_left_x,
            y: start_y + row_h * 3,
            w: col_w,
            h: row_h,
            module: "INA3221",
            status: comm_label(snapshot.ina3221),
            key: ina_key,
            active: data.focus == UiFocus::Touch,
            accent: palette.touch,
        },
    )?;
    draw_diag_card(
        painter,
        variant,
        palette,
        DiagCard {
            x: col_left_x,
            y: start_y + row_h * 4,
            w: col_w,
            h: row_h,
            module: "BQ25792",
            status: charger_label(snapshot.bq25792, snapshot.bq25792_allow_charge),
            key: chg_key,
            active: data.focus == UiFocus::Right,
            accent: palette.right,
        },
    )?;

    draw_diag_card(
        painter,
        variant,
        palette,
        DiagCard {
            x: col_right_x,
            y: start_y,
            w: col_w,
            h: row_h,
            module: "BQ40Z50",
            status: bms_label(snapshot.bq40z50, snapshot.bq40z50_rca_alarm),
            key: bms_key,
            active: data.focus == UiFocus::Left,
            accent: palette.left,
        },
    )?;
    draw_diag_card(
        painter,
        variant,
        palette,
        DiagCard {
            x: col_right_x,
            y: start_y + row_h,
            w: col_w,
            h: row_h,
            module: "TPS55288-A",
            status: tps_label(snapshot.tps_a, snapshot.tps_a_enabled),
            key: tps_a_key,
            active: data.focus == UiFocus::Up,
            accent: palette.up,
        },
    )?;
    draw_diag_card(
        painter,
        variant,
        palette,
        DiagCard {
            x: col_right_x,
            y: start_y + row_h * 2,
            w: col_w,
            h: row_h,
            module: "TPS55288-B",
            status: tps_label(snapshot.tps_b, snapshot.tps_b_enabled),
            key: tps_b_key,
            active: data.focus == UiFocus::Down,
            accent: palette.down,
        },
    )?;
    draw_diag_card(
        painter,
        variant,
        palette,
        DiagCard {
            x: col_right_x,
            y: start_y + row_h * 3,
            w: col_w,
            h: row_h,
            module: "TMP112-A",
            status: tmp_label(snapshot.tmp_a, snapshot.tmp_a_c),
            key: tmp_a_key,
            active: data.focus == UiFocus::Center,
            accent: palette.center,
        },
    )?;
    draw_diag_card(
        painter,
        variant,
        palette,
        DiagCard {
            x: col_right_x,
            y: start_y + row_h * 4,
            w: col_w,
            h: row_h,
            module: "TMP112-B",
            status: tmp_label(snapshot.tmp_b, snapshot.tmp_b_c),
            key: tmp_b_key,
            active: data.focus == UiFocus::Center,
            accent: palette.center,
        },
    )?;

    Ok(())
}

fn comm_label(state: SelfCheckCommState) -> &'static str {
    match state {
        SelfCheckCommState::Pending => "PEND",
        SelfCheckCommState::Ok => "OK",
        SelfCheckCommState::Warn => "WARN",
        SelfCheckCommState::Err => "ERR",
        SelfCheckCommState::NotAvailable => "N/A",
    }
}

fn tps_label(state: SelfCheckCommState, enabled: Option<bool>) -> &'static str {
    match state {
        SelfCheckCommState::Pending => "PEND",
        SelfCheckCommState::Warn => "WARN",
        SelfCheckCommState::Err => "ERR",
        SelfCheckCommState::NotAvailable => "N/A",
        SelfCheckCommState::Ok => match enabled {
            Some(true) => "RUN",
            Some(false) => "IDLE",
            None => "OK",
        },
    }
}

fn charger_label(state: SelfCheckCommState, allow_charge: Option<bool>) -> &'static str {
    match state {
        SelfCheckCommState::Pending => "PEND",
        SelfCheckCommState::Warn => "WARN",
        SelfCheckCommState::Err => "ERR",
        SelfCheckCommState::NotAvailable => "N/A",
        SelfCheckCommState::Ok => match allow_charge {
            Some(true) => "RUN",
            Some(false) => "LOCK",
            None => "OK",
        },
    }
}

fn bms_label(state: SelfCheckCommState, rca_alarm: Option<bool>) -> &'static str {
    match state {
        SelfCheckCommState::Pending => "PEND",
        SelfCheckCommState::Warn => {
            if rca_alarm == Some(true) {
                "RCA"
            } else {
                "WARN"
            }
        }
        SelfCheckCommState::Err => "ERR",
        SelfCheckCommState::NotAvailable => "N/A",
        SelfCheckCommState::Ok => {
            if rca_alarm == Some(true) {
                "RCA"
            } else {
                "OK"
            }
        }
    }
}

fn tmp_label(state: SelfCheckCommState, temp_c: Option<i16>) -> &'static str {
    match state {
        SelfCheckCommState::Pending => "PEND",
        SelfCheckCommState::Warn => "WARN",
        SelfCheckCommState::Err => "ERR",
        SelfCheckCommState::NotAvailable => "N/A",
        SelfCheckCommState::Ok => match temp_c {
            Some(v) if v >= 50 => "HOT",
            Some(_) => "OK",
            None => "OK",
        },
    }
}

fn vbus_key_text(vbus_present: Option<bool>) -> &'static str {
    match vbus_present {
        Some(true) => "VBUS PRESENT",
        Some(false) => "VBUS LOST",
        None => "VBUS N/A",
    }
}

fn render_variant_d<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    data: DashboardData,
) -> Result<(), P::Error> {
    render_variant_b(painter, variant, palette, data)
}

struct DiagCard<T>
where
    T: Content,
{
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    module: &'static str,
    status: &'static str,
    key: T,
    active: bool,
    accent: u16,
}

fn draw_diag_card<P: UiPainter, T: Content>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    spec: DiagCard<T>,
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
    let dim_color = if spec.active {
        fade_color(palette.bg, spec.accent)
    } else {
        palette.text_dim
    };
    text(
        painter,
        variant,
        FontRole::TextCompact,
        spec.module,
        Point::new((spec.x + 6) as i32, (spec.y + 3) as i32),
        HorizontalAlignment::Left,
        text_color,
    )?;
    text(
        painter,
        variant,
        FontRole::NumCompact,
        spec.status,
        Point::new((spec.x + spec.w - 6) as i32, (spec.y + 4) as i32),
        HorizontalAlignment::Right,
        if spec.active { palette.bg } else { dim_color },
    )?;
    text(
        painter,
        variant,
        FontRole::NumCompact,
        spec.key,
        Point::new((spec.x + 6) as i32, (spec.y + 15) as i32),
        HorizontalAlignment::Left,
        text_color,
    )?;
    Ok(())
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

#[allow(dead_code)]
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
        FontRole::TextCompact,
        "MODULE",
        Point::new(x as i32, y as i32),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    text(
        painter,
        variant,
        FontRole::TextCompact,
        "COMM",
        Point::new((x + 194) as i32, y as i32),
        HorizontalAlignment::Right,
        palette.text,
    )?;
    text(
        painter,
        variant,
        FontRole::TextCompact,
        "KEY PARAM",
        Point::new((x + 296) as i32, y as i32),
        HorizontalAlignment::Right,
        palette.text,
    )?;
    Ok(())
}

#[allow(dead_code)]
struct TableRow<TK>
where
    TK: Content,
{
    x: u16,
    y: u16,
    h: u16,
    module: &'static str,
    status: &'static str,
    key: TK,
    active: bool,
    accent: u16,
    odd: bool,
}

#[allow(dead_code)]
fn draw_table_row<P: UiPainter, TK: Content>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    spec: TableRow<TK>,
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
        FontRole::TextCompact,
        spec.module,
        Point::new((spec.x + 4) as i32, (spec.y + 2) as i32),
        HorizontalAlignment::Left,
        text_color,
    )?;
    text(
        painter,
        variant,
        FontRole::NumCompact,
        spec.status,
        Point::new((spec.x + 194) as i32, (spec.y + 2) as i32),
        HorizontalAlignment::Right,
        dim_color,
    )?;
    text(
        painter,
        variant,
        FontRole::NumCompact,
        spec.key,
        Point::new((spec.x + 300) as i32, (spec.y + 2) as i32),
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

#[allow(dead_code)]
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

#[allow(dead_code)]
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
        FontRole::TextCompact => &FONT_A_BODY,
        FontRole::Num => &FONT_B_NUM,
        FontRole::NumCompact => &FONT_B_NUM,
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

#[allow(dead_code)]
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
