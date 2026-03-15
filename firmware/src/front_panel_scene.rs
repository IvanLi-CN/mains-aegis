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
const VIN_MAINS_PRESENT_THRESHOLD_MV: u16 = 3_000;

fn mains_present_from_vin(vin_vbus_mv: Option<u16>) -> bool {
    vin_vbus_mv.is_some_and(|mv| mv >= VIN_MAINS_PRESENT_THRESHOLD_MV)
}

fn snapshot_mains_present(snapshot: &SelfCheckUiSnapshot) -> bool {
    snapshot
        .vin_mains_present
        .unwrap_or_else(|| mains_present_from_vin(snapshot.vin_vbus_mv))
}

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

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestFunctionUi {
    ScreenStatic,
    AudioPlayback,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioEventUi {
    BootStartup,
    MainsPresentDc,
    ChargeStarted,
    ChargeCompleted,
    ShutdownModeEntered,
    MainsAbsentDc,
    HighStress,
    BatteryLowNoMains,
    BatteryLowWithMains,
    ShutdownProtection,
    IoOverVoltage,
    IoOverCurrent,
    IoOverPower,
    ModuleFault,
    BatteryProtection,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioTestUiState {
    pub playing: bool,
    pub queued: u8,
    pub current: Option<AudioEventUi>,
    pub selected_idx: u8,
    pub list_top: u8,
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
const ERROR_COLOR: u16 = 0xF800;
const SUCCESS_COLOR: u16 = 0x07E0;
const PROGRESS_COLOR: u16 = 0xFD20;

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
pub enum BmsResultKind {
    Success,
    #[allow(dead_code)]
    NoBattery,
    RomMode,
    Abnormal,
    NotDetected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelfCheckUiSnapshot {
    pub mode: UpsMode,
    pub gc9307: SelfCheckCommState,
    pub tca6408a: SelfCheckCommState,
    pub fusb302: SelfCheckCommState,
    pub fusb302_vbus_present: Option<bool>,
    pub input_vbus_mv: Option<u16>,
    pub input_ibus_ma: Option<i32>,
    pub vin_mains_present: Option<bool>,
    pub vin_vbus_mv: Option<u16>,
    pub vin_iin_ma: Option<i32>,
    pub ina3221: SelfCheckCommState,
    pub ina_total_ma: Option<i32>,
    pub bq25792: SelfCheckCommState,
    pub bq25792_allow_charge: Option<bool>,
    pub bq25792_ichg_ma: Option<u16>,
    pub bq25792_vbat_present: Option<bool>,
    pub bq40z50: SelfCheckCommState,
    pub bq40z50_pack_mv: Option<u16>,
    pub bq40z50_current_ma: Option<i16>,
    pub bq40z50_soc_pct: Option<u16>,
    pub bq40z50_rca_alarm: Option<bool>,
    pub bq40z50_no_battery: Option<bool>,
    pub bq40z50_discharge_ready: Option<bool>,
    pub bq40z50_last_result: Option<BmsResultKind>,
    pub tps_a: SelfCheckCommState,
    pub tps_a_enabled: Option<bool>,
    pub out_a_vbus_mv: Option<u16>,
    pub tps_a_iout_ma: Option<i32>,
    pub tps_b: SelfCheckCommState,
    pub tps_b_enabled: Option<bool>,
    pub out_b_vbus_mv: Option<u16>,
    pub tps_b_iout_ma: Option<i32>,
    pub tmp_a: SelfCheckCommState,
    pub tmp_a_c: Option<i16>,
    pub tmp_a_c_x16: Option<i16>,
    pub tmp_b: SelfCheckCommState,
    pub tmp_b_c: Option<i16>,
    pub tmp_b_c_x16: Option<i16>,
}

impl SelfCheckUiSnapshot {
    pub const fn pending(mode: UpsMode) -> Self {
        Self {
            mode,
            gc9307: SelfCheckCommState::Pending,
            tca6408a: SelfCheckCommState::Pending,
            fusb302: SelfCheckCommState::Pending,
            fusb302_vbus_present: None,
            input_vbus_mv: None,
            input_ibus_ma: None,
            vin_mains_present: None,
            vin_vbus_mv: None,
            vin_iin_ma: None,
            ina3221: SelfCheckCommState::Pending,
            ina_total_ma: None,
            bq25792: SelfCheckCommState::Pending,
            bq25792_allow_charge: None,
            bq25792_ichg_ma: None,
            bq25792_vbat_present: None,
            bq40z50: SelfCheckCommState::Pending,
            bq40z50_pack_mv: None,
            bq40z50_current_ma: None,
            bq40z50_soc_pct: None,
            bq40z50_rca_alarm: None,
            bq40z50_no_battery: None,
            bq40z50_discharge_ready: None,
            bq40z50_last_result: None,
            tps_a: SelfCheckCommState::Pending,
            tps_a_enabled: None,
            out_a_vbus_mv: None,
            tps_a_iout_ma: None,
            tps_b: SelfCheckCommState::Pending,
            tps_b_enabled: None,
            out_b_vbus_mv: None,
            tps_b_iout_ma: None,
            tmp_a: SelfCheckCommState::Pending,
            tmp_a_c: None,
            tmp_a_c_x16: None,
            tmp_b: SelfCheckCommState::Pending,
            tmp_b_c: None,
            tmp_b_c_x16: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum SelfCheckTouchTarget {
    Bq40Card,
    ActivateCancel,
    ActivateConfirm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum SelfCheckOverlay {
    None,
    BmsActivateConfirm,
    BmsActivateProgress,
    BmsActivateResult(BmsResultKind),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum BmsActivationState {
    Idle,
    Pending,
    Result(BmsResultKind),
}

const SELF_CHECK_BQ40_CARD_X: u16 = 163;
const SELF_CHECK_BQ40_CARD_Y: u16 = 22;
const SELF_CHECK_BQ40_CARD_W: u16 = 151;
const SELF_CHECK_BQ40_CARD_H: u16 = 29;

const SELF_CHECK_DIALOG_X: u16 = 20;
const SELF_CHECK_DIALOG_Y: u16 = 34;
const SELF_CHECK_DIALOG_W: u16 = 280;
const SELF_CHECK_DIALOG_H: u16 = 112;

const SELF_CHECK_CANCEL_BTN_X: u16 = 32;
const SELF_CHECK_CANCEL_BTN_Y: u16 = 110;
const SELF_CHECK_CANCEL_BTN_W: u16 = 108;
const SELF_CHECK_CANCEL_BTN_H: u16 = 24;

const SELF_CHECK_CONFIRM_BTN_X: u16 = 152;
const SELF_CHECK_CONFIRM_BTN_Y: u16 = 110;
const SELF_CHECK_CONFIRM_BTN_W: u16 = 136;
const SELF_CHECK_CONFIRM_BTN_H: u16 = 24;

#[allow(dead_code)]
const TEST_NAV_CARD_X: u16 = 20;
#[allow(dead_code)]
const TEST_NAV_CARD_Y: u16 = 42;
#[allow(dead_code)]
const TEST_NAV_CARD_W: u16 = 280;
#[allow(dead_code)]
const TEST_NAV_CARD_H: u16 = 44;
#[allow(dead_code)]
const TEST_NAV_CARD_GAP: u16 = 14;

#[allow(dead_code)]
const TEST_BACK_BTN_X: u16 = 12;
#[allow(dead_code)]
const TEST_BACK_BTN_Y: u16 = 142;
#[allow(dead_code)]
const TEST_BACK_BTN_W: u16 = 84;
#[allow(dead_code)]
const TEST_BACK_BTN_H: u16 = 20;

#[allow(dead_code)]
const TEST_AUDIO_LIST_X: u16 = 12;
#[allow(dead_code)]
const TEST_AUDIO_LIST_Y: u16 = 24;
#[allow(dead_code)]
const TEST_AUDIO_LIST_W: u16 = 296;
#[allow(dead_code)]
const TEST_AUDIO_LIST_H: u16 = 144;

#[allow(dead_code)]
const TEST_AUDIO_ROW_X: u16 = TEST_AUDIO_LIST_X + 6;
#[allow(dead_code)]
const TEST_AUDIO_ROW_Y: u16 = TEST_AUDIO_LIST_Y + 6;
#[allow(dead_code)]
const TEST_AUDIO_ROW_W: u16 = TEST_AUDIO_LIST_W - 12;
#[allow(dead_code)]
const TEST_AUDIO_ROW_H: u16 = 20;
#[allow(dead_code)]
const TEST_AUDIO_ROW_GAP: u16 = 2;
#[allow(dead_code)]
pub const TEST_AUDIO_VISIBLE_ROWS: usize = 6;

#[allow(dead_code)]
const TEST_AUDIO_BACK_BTN_X: u16 = UI_W - 72;
#[allow(dead_code)]
const TEST_AUDIO_BACK_BTN_Y: u16 = 2;
#[allow(dead_code)]
const TEST_AUDIO_BACK_BTN_W: u16 = 68;
#[allow(dead_code)]
const TEST_AUDIO_BACK_BTN_H: u16 = 18;

#[allow(dead_code)]
const TEST_AUDIO_SCROLLBAR_W: u16 = 4;

#[allow(dead_code)]
const AUDIO_TEST_ITEM_COUNT: usize = 15;
#[allow(dead_code)]
const AUDIO_TEST_LABELS: [&str; AUDIO_TEST_ITEM_COUNT] = [
    "BOOT STARTUP",
    "MAINS PRESENT DC",
    "CHARGE STARTED",
    "CHARGE COMPLETED",
    "SHUTDOWN MODE ENTERED",
    "MAINS ABSENT DC",
    "HIGH STRESS",
    "BATTERY LOW NO MAINS",
    "BATTERY LOW WITH MAINS",
    "SHUTDOWN PROTECTION",
    "IO OVER VOLTAGE",
    "IO OVER CURRENT",
    "IO OVER POWER",
    "MODULE FAULT",
    "BATTERY PROTECTION",
];

#[allow(dead_code)]
pub fn is_bq40_offline(snapshot: &SelfCheckUiSnapshot) -> bool {
    snapshot.bq40z50 == SelfCheckCommState::Err
}

#[allow(dead_code)]
pub fn is_bq40_activation_needed(snapshot: &SelfCheckUiSnapshot) -> bool {
    snapshot.bq40z50_last_result.is_none() && is_bq40_offline(snapshot)
}

#[allow(dead_code)]
pub fn bq40_result_overlay(snapshot: &SelfCheckUiSnapshot) -> Option<SelfCheckOverlay> {
    snapshot
        .bq40z50_last_result
        .map(SelfCheckOverlay::BmsActivateResult)
}

#[allow(dead_code)]
pub fn self_check_hit_test(
    x: u16,
    y: u16,
    overlay: SelfCheckOverlay,
) -> Option<SelfCheckTouchTarget> {
    match overlay {
        SelfCheckOverlay::None => {
            if contains(
                x,
                y,
                SELF_CHECK_BQ40_CARD_X,
                SELF_CHECK_BQ40_CARD_Y,
                SELF_CHECK_BQ40_CARD_W,
                SELF_CHECK_BQ40_CARD_H,
            ) {
                Some(SelfCheckTouchTarget::Bq40Card)
            } else {
                None
            }
        }
        SelfCheckOverlay::BmsActivateConfirm => {
            if contains(
                x,
                y,
                SELF_CHECK_CANCEL_BTN_X,
                SELF_CHECK_CANCEL_BTN_Y,
                SELF_CHECK_CANCEL_BTN_W,
                SELF_CHECK_CANCEL_BTN_H,
            ) {
                Some(SelfCheckTouchTarget::ActivateCancel)
            } else if contains(
                x,
                y,
                SELF_CHECK_CONFIRM_BTN_X,
                SELF_CHECK_CONFIRM_BTN_Y,
                SELF_CHECK_CONFIRM_BTN_W,
                SELF_CHECK_CONFIRM_BTN_H,
            ) {
                Some(SelfCheckTouchTarget::ActivateConfirm)
            } else {
                None
            }
        }
        SelfCheckOverlay::BmsActivateProgress | SelfCheckOverlay::BmsActivateResult(..) => None,
    }
}

#[allow(dead_code)]
pub fn test_navigation_hit_test(x: u16, y: u16) -> Option<TestFunctionUi> {
    if contains(
        x,
        y,
        TEST_NAV_CARD_X,
        TEST_NAV_CARD_Y,
        TEST_NAV_CARD_W,
        TEST_NAV_CARD_H,
    ) {
        return Some(TestFunctionUi::ScreenStatic);
    }

    if contains(
        x,
        y,
        TEST_NAV_CARD_X,
        TEST_NAV_CARD_Y + TEST_NAV_CARD_H + TEST_NAV_CARD_GAP,
        TEST_NAV_CARD_W,
        TEST_NAV_CARD_H,
    ) {
        return Some(TestFunctionUi::AudioPlayback);
    }

    None
}

#[allow(dead_code)]
pub fn test_back_hit_test(x: u16, y: u16) -> bool {
    contains(
        x,
        y,
        TEST_BACK_BTN_X,
        TEST_BACK_BTN_Y,
        TEST_BACK_BTN_W,
        TEST_BACK_BTN_H,
    )
}

#[allow(dead_code)]
pub fn test_audio_list_scroll_hit_test(x: u16, y: u16) -> bool {
    contains(
        x,
        y,
        TEST_AUDIO_LIST_X,
        TEST_AUDIO_LIST_Y,
        TEST_AUDIO_LIST_W,
        TEST_AUDIO_LIST_H,
    )
}

#[allow(dead_code)]
pub fn test_audio_list_hit_test(x: u16, y: u16, list_top: usize) -> Option<usize> {
    if !test_audio_list_scroll_hit_test(x, y) {
        return None;
    }

    if y < TEST_AUDIO_ROW_Y {
        return None;
    }

    let rel_y = y - TEST_AUDIO_ROW_Y;
    let stride = TEST_AUDIO_ROW_H + TEST_AUDIO_ROW_GAP;
    let row = (rel_y / stride) as usize;
    if row >= TEST_AUDIO_VISIBLE_ROWS {
        return None;
    }
    if (rel_y % stride) >= TEST_AUDIO_ROW_H {
        return None;
    }

    let idx = list_top + row;
    if idx >= AUDIO_TEST_ITEM_COUNT {
        None
    } else {
        Some(idx)
    }
}

#[allow(dead_code)]
pub fn test_audio_back_hit_test(x: u16, y: u16) -> bool {
    contains(
        x,
        y,
        TEST_AUDIO_BACK_BTN_X,
        TEST_AUDIO_BACK_BTN_Y,
        TEST_AUDIO_BACK_BTN_W,
        TEST_AUDIO_BACK_BTN_H,
    )
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

#[derive(Clone, Copy)]
struct DashboardLiveData {
    mode: UpsMode,
    focus: UiFocus,
    touch_irq: bool,
    mains_present: bool,
    out_a_on: bool,
    out_b_on: bool,
    bms_on: bool,
    vin_vbus_mv: Option<u16>,
    vin_iin_ma: Option<i32>,
    out_a_mv: Option<u16>,
    out_a_ma: Option<i32>,
    out_b_mv: Option<u16>,
    out_b_ma: Option<i32>,
    chg_iin_ma: Option<u16>,
    batt_pack_mv: Option<u16>,
    bms_current_ma: Option<i16>,
    bms_soc_pct: Option<u16>,
    therm_a_c: Option<i16>,
    therm_b_c: Option<i16>,
    charge_allowed: Option<bool>,
    bms_state: SelfCheckCommState,
    charger_state: SelfCheckCommState,
    bms_rca_alarm: Option<bool>,
    bms_no_battery: Option<bool>,
    bms_discharge_ready: Option<bool>,
}

impl DashboardLiveData {
    fn from_snapshot(model: DashboardData, snapshot: &SelfCheckUiSnapshot) -> Self {
        Self {
            mode: snapshot.mode,
            focus: model.focus,
            touch_irq: model.touch_irq,
            mains_present: snapshot_mains_present(snapshot),
            out_a_on: snapshot.tps_a_enabled == Some(true),
            out_b_on: snapshot.tps_b_enabled == Some(true),
            bms_on: model.bms_on,
            vin_vbus_mv: snapshot.vin_vbus_mv,
            vin_iin_ma: snapshot.vin_iin_ma,
            out_a_mv: snapshot.out_a_vbus_mv,
            out_a_ma: snapshot.tps_a_iout_ma,
            out_b_mv: snapshot.out_b_vbus_mv,
            out_b_ma: snapshot.tps_b_iout_ma,
            chg_iin_ma: snapshot.bq25792_ichg_ma,
            batt_pack_mv: snapshot.bq40z50_pack_mv,
            bms_current_ma: snapshot.bq40z50_current_ma,
            bms_soc_pct: snapshot.bq40z50_soc_pct,
            therm_a_c: snapshot.tmp_a_c,
            therm_b_c: snapshot.tmp_b_c,
            charge_allowed: snapshot.bq25792_allow_charge,
            bms_state: snapshot.bq40z50,
            charger_state: snapshot.bq25792,
            bms_rca_alarm: snapshot.bq40z50_rca_alarm,
            bms_no_battery: snapshot.bq40z50_no_battery,
            bms_discharge_ready: snapshot.bq40z50_discharge_ready,
        }
    }

    fn output_bus_mv(self) -> Option<u16> {
        match (
            self.out_a_on.then_some(self.out_a_mv).flatten(),
            self.out_b_on.then_some(self.out_b_mv).flatten(),
        ) {
            (Some(a), Some(b)) => Some(((a as u32 + b as u32) / 2) as u16),
            (Some(a), None) if !self.out_b_on => Some(a),
            (None, Some(b)) if !self.out_a_on => Some(b),
            (Some(a), None) if a > 0 && !self.out_b_on => Some(a),
            (None, Some(b)) if b > 0 && !self.out_a_on => Some(b),
            _ => None,
        }
    }

    fn output_current_ma(self) -> Option<u32> {
        if !self.out_a_on && !self.out_b_on {
            return None;
        }
        let a = if self.out_a_on {
            Some(self.out_a_ma?.unsigned_abs())
        } else {
            Some(0)
        }?;
        let b = if self.out_b_on {
            Some(self.out_b_ma?.unsigned_abs())
        } else {
            Some(0)
        }?;
        Some(a + b)
    }

    fn input_power_w10(self) -> Option<u32> {
        let vin_ma = self.vin_iin_ma?;
        Some((self.vin_vbus_mv? as u32 * vin_ma.max(0) as u32) / 100_000)
    }

    fn output_power_w10(self) -> Option<u32> {
        Some((self.output_bus_mv()? as u32 * self.output_current_ma()?) / 100_000)
    }

    fn charge_current_ma(self) -> Option<u16> {
        match self.charge_allowed {
            Some(true) => self.chg_iin_ma,
            Some(false) => Some(0),
            None => None,
        }
    }

    fn battery_discharge_ma(self) -> Option<u32> {
        match self.bms_current_ma {
            Some(ma) if ma < 0 => Some(ma.unsigned_abs() as u32),
            Some(_) => Some(0),
            None => None,
        }
    }

    #[cfg(test)]
    fn battery_max_temp_c(self) -> Option<i16> {
        match (self.therm_a_c, self.therm_b_c) {
            (Some(a), Some(b)) => Some(if a > b { a } else { b }),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
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
pub struct DisplayDiagnosticMeta {
    pub orientation_label: &'static str,
    pub color_order_label: &'static str,
    pub heartbeat_on: bool,
}

#[allow(dead_code)]
pub fn render_display_diagnostic<P: UiPainter>(
    painter: &mut P,
    meta: &DisplayDiagnosticMeta,
) -> Result<(), P::Error> {
    const BG: u16 = 0x0000;
    const FG: u16 = 0xFFFF;
    const MUTED: u16 = 0x7BEF;
    const ACCENT: u16 = 0x07FF;

    fill(painter, 0, 0, UI_W, UI_H, BG)?;
    draw_outline(painter, 0, 0, UI_W, UI_H, FG)?;

    fill(painter, 0, 0, UI_W, 20, 0x0841)?;
    text(
        painter,
        UiVariant::RetroC,
        FontRole::TextCompact,
        "DISPLAY DIAG",
        Point::new((UI_W / 2) as i32, 6),
        HorizontalAlignment::Center,
        FG,
    )?;

    text(
        painter,
        UiVariant::RetroC,
        FontRole::TextCompact,
        "UP ^",
        Point::new((UI_W / 2) as i32, 24),
        HorizontalAlignment::Center,
        ACCENT,
    )?;
    fill(painter, UI_W / 2, 34, 1, 24, ACCENT)?;
    fill(painter, UI_W / 2 - 3, 34, 7, 1, ACCENT)?;
    fill(painter, UI_W / 2 - 2, 35, 5, 1, ACCENT)?;
    fill(painter, UI_W / 2 - 1, 36, 3, 1, ACCENT)?;

    fill(painter, 4, 24, 30, 18, 0xF800)?;
    fill(painter, UI_W - 34, 24, 30, 18, 0x07E0)?;
    fill(painter, 4, UI_H - 22, 30, 18, 0x001F)?;
    fill(painter, UI_W - 34, UI_H - 22, 30, 18, 0xFFE0)?;
    text(
        painter,
        UiVariant::RetroC,
        FontRole::TextCompact,
        "TL",
        Point::new(19, 29),
        HorizontalAlignment::Center,
        FG,
    )?;
    text(
        painter,
        UiVariant::RetroC,
        FontRole::TextCompact,
        "TR",
        Point::new((UI_W - 19) as i32, 29),
        HorizontalAlignment::Center,
        0x0000,
    )?;
    text(
        painter,
        UiVariant::RetroC,
        FontRole::TextCompact,
        "BL",
        Point::new(19, (UI_H - 17) as i32),
        HorizontalAlignment::Center,
        FG,
    )?;
    text(
        painter,
        UiVariant::RetroC,
        FontRole::TextCompact,
        "BR",
        Point::new((UI_W - 19) as i32, (UI_H - 17) as i32),
        HorizontalAlignment::Center,
        0x0000,
    )?;

    text(
        painter,
        UiVariant::RetroC,
        FontRole::TextCompact,
        "LEFT",
        Point::new(6, 47),
        HorizontalAlignment::Left,
        ACCENT,
    )?;
    text(
        painter,
        UiVariant::RetroC,
        FontRole::TextCompact,
        "RIGHT",
        Point::new((UI_W - 6) as i32, 47),
        HorizontalAlignment::Right,
        ACCENT,
    )?;

    const BARS: [(u16, &str); 8] = [
        (0xF800, "R"),
        (0x07E0, "G"),
        (0x001F, "B"),
        (0xFFE0, "Y"),
        (0x07FF, "C"),
        (0xF81F, "M"),
        (0xFFFF, "W"),
        (0x0000, "K"),
    ];
    let bar_y = 60;
    let bar_h = 46;
    let bar_w = UI_W / (BARS.len() as u16);
    for (idx, &(color, label)) in BARS.iter().enumerate() {
        let x = (idx as u16) * bar_w;
        fill(painter, x, bar_y, bar_w, bar_h, color)?;
        draw_outline(
            painter,
            x,
            bar_y,
            bar_w,
            bar_h,
            if color == 0x0000 { FG } else { BG },
        )?;
        text(
            painter,
            UiVariant::RetroC,
            FontRole::TextCompact,
            label,
            Point::new((x + bar_w / 2) as i32, (bar_y + bar_h + 2) as i32),
            HorizontalAlignment::Center,
            if color == 0x0000 { FG } else { BG },
        )?;
    }

    let gray_y = 118;
    let gray_h = 16;
    let gray_w = UI_W / 8;
    for idx in 0..8u16 {
        let r = (idx * 31 / 7) & 0x1f;
        let g = (idx * 63 / 7) & 0x3f;
        let b = (idx * 31 / 7) & 0x1f;
        let gray = (r << 11) | (g << 5) | b;
        fill(painter, idx * gray_w, gray_y, gray_w, gray_h, gray)?;
        draw_outline(painter, idx * gray_w, gray_y, gray_w, gray_h, MUTED)?;
    }

    text(
        painter,
        UiVariant::RetroC,
        FontRole::TextCompact,
        meta.orientation_label,
        Point::new(6, 140),
        HorizontalAlignment::Left,
        FG,
    )?;
    text(
        painter,
        UiVariant::RetroC,
        FontRole::TextCompact,
        meta.color_order_label,
        Point::new(6, 150),
        HorizontalAlignment::Left,
        FG,
    )?;
    text(
        painter,
        UiVariant::RetroC,
        FontRole::TextCompact,
        "EXPECT: TL-R TR-G BL-B BR-Y",
        Point::new(6, 160),
        HorizontalAlignment::Left,
        MUTED,
    )?;

    fill(
        painter,
        UI_W - 16,
        4,
        10,
        10,
        if meta.heartbeat_on { 0x07E0 } else { 0x39E7 },
    )?;
    draw_outline(painter, UI_W - 16, 4, 10, 10, FG)?;

    Ok(())
}

#[allow(dead_code)]
pub fn render_test_navigation<P: UiPainter>(
    painter: &mut P,
    selected: TestFunctionUi,
    _default_test: Option<TestFunctionUi>,
) -> Result<(), P::Error> {
    let variant = UiVariant::RetroC;
    let palette = palette_for(variant);

    fill(painter, 0, 0, UI_W, UI_H, palette.bg)?;
    draw_background_grid(painter, palette)?;
    draw_outline(painter, 0, 0, UI_W, UI_H, palette.border)?;
    draw_top_bar_with_status(
        painter,
        variant,
        palette,
        UiFocus::Idle,
        "HW TEST FW",
        "TEST ITEM LIST",
        "",
        palette.accent,
    )?;
    let list_x = TEST_NAV_CARD_X - 8;
    let list_y = TEST_NAV_CARD_Y - 8;
    let list_w = TEST_NAV_CARD_W + 16;
    let list_h = (TEST_NAV_CARD_H * 2) + TEST_NAV_CARD_GAP + 16;
    draw_panel(
        painter,
        list_x,
        list_y,
        list_w,
        list_h,
        palette,
        false,
        palette.accent,
    )?;

    let row_x = TEST_NAV_CARD_X;
    let row_w = TEST_NAV_CARD_W;
    let row_h = TEST_NAV_CARD_H;
    let screen_y = TEST_NAV_CARD_Y;
    let audio_y = TEST_NAV_CARD_Y + TEST_NAV_CARD_H + TEST_NAV_CARD_GAP;
    let screen_selected = selected == TestFunctionUi::ScreenStatic;
    let audio_selected = selected == TestFunctionUi::AudioPlayback;

    draw_panel(
        painter,
        row_x,
        screen_y,
        row_w,
        row_h,
        palette,
        screen_selected,
        palette.right,
    )?;
    text(
        painter,
        variant,
        FontRole::TextTitle,
        "01  SCREEN STATIC",
        Point::new((row_x + 12) as i32, (screen_y + 12) as i32),
        HorizontalAlignment::Left,
        if screen_selected {
            palette.bg
        } else {
            palette.text
        },
    )?;
    if screen_selected {
        text(
            painter,
            variant,
            FontRole::TextTitle,
            ">",
            Point::new((row_x + row_w - 10) as i32, (screen_y + 12) as i32),
            HorizontalAlignment::Right,
            palette.bg,
        )?;
    }

    draw_panel(
        painter,
        row_x,
        audio_y,
        row_w,
        row_h,
        palette,
        audio_selected,
        palette.down,
    )?;
    text(
        painter,
        variant,
        FontRole::TextTitle,
        "02  AUDIO PLAYBACK",
        Point::new((row_x + 12) as i32, (audio_y + 12) as i32),
        HorizontalAlignment::Left,
        if audio_selected {
            palette.bg
        } else {
            palette.text
        },
    )?;
    if audio_selected {
        text(
            painter,
            variant,
            FontRole::TextTitle,
            ">",
            Point::new((row_x + row_w - 10) as i32, (audio_y + 12) as i32),
            HorizontalAlignment::Right,
            palette.bg,
        )?;
    }
    Ok(())
}

#[allow(dead_code)]
pub fn render_test_screen_static<P: UiPainter>(
    painter: &mut P,
    back_enabled: bool,
    color_order_label: &'static str,
) -> Result<(), P::Error> {
    let meta = DisplayDiagnosticMeta {
        orientation_label: "SCREEN STATIC TEST",
        color_order_label,
        heartbeat_on: true,
    };
    render_display_diagnostic(painter, &meta)?;

    text(
        painter,
        UiVariant::RetroC,
        FontRole::TextBody,
        "Static pattern validation page",
        Point::new(8, 132),
        HorizontalAlignment::Left,
        0xFFFF,
    )?;
    text(
        painter,
        UiVariant::RetroC,
        FontRole::TextBody,
        if back_enabled {
            "LEFT key or BACK button to return"
        } else {
            "Single test mode: BACK disabled"
        },
        Point::new(8, 144),
        HorizontalAlignment::Left,
        0x7BEF,
    )?;

    render_test_back_button(painter, back_enabled)
}

#[allow(dead_code)]
pub fn render_test_audio_playback<P: UiPainter>(
    painter: &mut P,
    back_enabled: bool,
    state: AudioTestUiState,
) -> Result<(), P::Error> {
    let variant = UiVariant::RetroC;
    let palette = palette_for(variant);

    fill(painter, 0, 0, UI_W, UI_H, palette.bg)?;
    draw_background_grid(painter, palette)?;
    draw_outline(painter, 0, 0, UI_W, UI_H, palette.border)?;
    draw_top_bar_with_status(
        painter,
        variant,
        palette,
        UiFocus::Idle,
        "AUDIO TEST",
        "cue list",
        if state.playing { "PLAYING" } else { "IDLE" },
        if state.playing {
            SUCCESS_COLOR
        } else {
            palette.text_dim
        },
    )?;
    draw_panel(
        painter,
        TEST_AUDIO_BACK_BTN_X,
        TEST_AUDIO_BACK_BTN_Y,
        TEST_AUDIO_BACK_BTN_W,
        TEST_AUDIO_BACK_BTN_H,
        palette,
        false,
        if back_enabled {
            palette.left
        } else {
            palette.panel_alt
        },
    )?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        if back_enabled { "BACK" } else { "BACK OFF" },
        Point::new(
            (TEST_AUDIO_BACK_BTN_X + TEST_AUDIO_BACK_BTN_W / 2) as i32,
            (TEST_AUDIO_BACK_BTN_Y + 4) as i32,
        ),
        HorizontalAlignment::Center,
        if back_enabled {
            palette.text
        } else {
            palette.text_dim
        },
    )?;

    let selected_idx = core::cmp::min(
        state.selected_idx as usize,
        AUDIO_TEST_ITEM_COUNT.saturating_sub(1),
    );
    let max_top = AUDIO_TEST_ITEM_COUNT.saturating_sub(TEST_AUDIO_VISIBLE_ROWS);
    let list_top = core::cmp::min(state.list_top as usize, max_top);
    let current_idx = state.current.and_then(audio_event_ui_index);

    draw_panel(
        painter,
        TEST_AUDIO_LIST_X,
        TEST_AUDIO_LIST_Y,
        TEST_AUDIO_LIST_W,
        TEST_AUDIO_LIST_H,
        palette,
        false,
        palette.accent,
    )?;
    let stride = TEST_AUDIO_ROW_H + TEST_AUDIO_ROW_GAP;
    let mut row = 0usize;
    while row < TEST_AUDIO_VISIBLE_ROWS {
        let idx = list_top + row;
        if idx >= AUDIO_TEST_ITEM_COUNT {
            break;
        }
        let row_y = TEST_AUDIO_ROW_Y + (row as u16) * stride;
        let selected = idx == selected_idx;
        let is_current = state.playing && current_idx == Some(idx);
        let accent = if is_current {
            SUCCESS_COLOR
        } else if selected {
            palette.right
        } else {
            palette.panel_alt
        };
        draw_panel(
            painter,
            TEST_AUDIO_ROW_X,
            row_y,
            TEST_AUDIO_ROW_W,
            TEST_AUDIO_ROW_H,
            palette,
            selected,
            accent,
        )?;
        text(
            painter,
            variant,
            FontRole::TextBody,
            format_args!("{:02}. {}", idx + 1, AUDIO_TEST_LABELS[idx]),
            Point::new((TEST_AUDIO_ROW_X + 8) as i32, (row_y + 6) as i32),
            HorizontalAlignment::Left,
            if selected { palette.bg } else { palette.text },
        )?;
        if is_current {
            text(
                painter,
                variant,
                FontRole::TextCompact,
                "PLAY",
                Point::new(
                    (TEST_AUDIO_ROW_X + TEST_AUDIO_ROW_W - 8) as i32,
                    (row_y + 6) as i32,
                ),
                HorizontalAlignment::Right,
                if selected { palette.bg } else { SUCCESS_COLOR },
            )?;
        }
        row += 1;
    }

    if AUDIO_TEST_ITEM_COUNT > TEST_AUDIO_VISIBLE_ROWS {
        let track_x = TEST_AUDIO_LIST_X + TEST_AUDIO_LIST_W - 8;
        let track_y = TEST_AUDIO_LIST_Y + 4;
        let track_h = TEST_AUDIO_LIST_H - 8;
        draw_panel(
            painter,
            track_x,
            track_y,
            TEST_AUDIO_SCROLLBAR_W,
            track_h,
            palette,
            false,
            palette.panel_alt,
        )?;

        let thumb_h = core::cmp::max(
            12,
            (track_h as usize * TEST_AUDIO_VISIBLE_ROWS / AUDIO_TEST_ITEM_COUNT) as u16,
        );
        let max_top = AUDIO_TEST_ITEM_COUNT - TEST_AUDIO_VISIBLE_ROWS;
        let travel = track_h.saturating_sub(thumb_h);
        let thumb_off = if max_top == 0 {
            0
        } else {
            (travel as usize * list_top / max_top) as u16
        };
        fill(
            painter,
            track_x,
            track_y + thumb_off,
            TEST_AUDIO_SCROLLBAR_W,
            thumb_h,
            if state.playing {
                SUCCESS_COLOR
            } else {
                palette.accent
            },
        )?;
    }

    Ok(())
}

fn audio_event_ui_index(event: AudioEventUi) -> Option<usize> {
    match event {
        AudioEventUi::BootStartup => Some(0),
        AudioEventUi::MainsPresentDc => Some(1),
        AudioEventUi::ChargeStarted => Some(2),
        AudioEventUi::ChargeCompleted => Some(3),
        AudioEventUi::ShutdownModeEntered => Some(4),
        AudioEventUi::MainsAbsentDc => Some(5),
        AudioEventUi::HighStress => Some(6),
        AudioEventUi::BatteryLowNoMains => Some(7),
        AudioEventUi::BatteryLowWithMains => Some(8),
        AudioEventUi::ShutdownProtection => Some(9),
        AudioEventUi::IoOverVoltage => Some(10),
        AudioEventUi::IoOverCurrent => Some(11),
        AudioEventUi::IoOverPower => Some(12),
        AudioEventUi::ModuleFault => Some(13),
        AudioEventUi::BatteryProtection => Some(14),
    }
}

#[allow(dead_code)]
pub fn render_test_back_button<P: UiPainter>(
    painter: &mut P,
    enabled: bool,
) -> Result<(), P::Error> {
    let variant = UiVariant::RetroC;
    let palette = palette_for(variant);
    draw_panel(
        painter,
        TEST_BACK_BTN_X,
        TEST_BACK_BTN_Y,
        TEST_BACK_BTN_W,
        TEST_BACK_BTN_H,
        palette,
        enabled,
        if enabled {
            palette.left
        } else {
            palette.panel_alt
        },
    )?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        if enabled { "BACK" } else { "BACK (DISABLED)" },
        Point::new(
            (TEST_BACK_BTN_X + TEST_BACK_BTN_W / 2) as i32,
            (TEST_BACK_BTN_Y + 4) as i32,
        ),
        HorizontalAlignment::Center,
        if enabled {
            palette.bg
        } else {
            palette.text_dim
        },
    )
}

#[allow(dead_code)]
pub fn render_frame<P: UiPainter>(
    painter: &mut P,
    model: &UiModel,
    variant: UiVariant,
) -> Result<(), P::Error> {
    render_frame_with_self_check_overlay(painter, model, variant, None, SelfCheckOverlay::None)
}

#[allow(dead_code)]
pub fn render_frame_with_self_check<P: UiPainter>(
    painter: &mut P,
    model: &UiModel,
    variant: UiVariant,
    self_check: Option<&SelfCheckUiSnapshot>,
) -> Result<(), P::Error> {
    render_frame_with_self_check_overlay(
        painter,
        model,
        variant,
        self_check,
        SelfCheckOverlay::None,
    )
}

pub fn render_frame_with_self_check_overlay<P: UiPainter>(
    painter: &mut P,
    model: &UiModel,
    variant: UiVariant,
    self_check: Option<&SelfCheckUiSnapshot>,
    overlay: SelfCheckOverlay,
) -> Result<(), P::Error> {
    let palette = palette_for(variant);
    let data = DashboardData::from_model(model);

    fill(painter, 0, 0, UI_W, UI_H, palette.bg)?;
    draw_background_grid(painter, palette)?;
    draw_outline(painter, 0, 0, UI_W, UI_H, palette.border)?;

    match variant {
        UiVariant::InstrumentA => render_variant_a(painter, variant, palette, data, self_check)?,
        UiVariant::InstrumentB => render_variant_b(painter, variant, palette, data, self_check)?,
        UiVariant::RetroC => {
            render_variant_c(painter, variant, palette, data, self_check, overlay)?
        }
        UiVariant::InstrumentD => render_variant_d(painter, variant, palette, data, self_check)?,
    }

    Ok(())
}

fn render_variant_a<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    data: DashboardData,
    self_check: Option<&SelfCheckUiSnapshot>,
) -> Result<(), P::Error> {
    render_variant_b(painter, variant, palette, data, self_check)
}

fn render_variant_b<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    data: DashboardData,
    self_check: Option<&SelfCheckUiSnapshot>,
) -> Result<(), P::Error> {
    if let Some(snapshot) = self_check {
        return render_variant_b_live(
            painter,
            variant,
            palette,
            DashboardLiveData::from_snapshot(data, snapshot),
        );
    }

    render_variant_b_demo(painter, variant, palette, data)
}

fn render_variant_b_demo<P: UiPainter>(
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
            note_color: palette.text_dim,
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
                note_color: palette.text_dim,
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
                note_color: palette.text_dim,
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
            note_color: palette.text_dim,
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

fn render_variant_b_live<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    data: DashboardLiveData,
) -> Result<(), P::Error> {
    let kpi_label_y = 27;
    let kpi_value_y = 44;
    let mode_accent = mode_accent_color(palette, data.mode, data.touch_irq);
    let mode_tag = if data.touch_irq {
        "IRQ ON"
    } else {
        mode_label(data.mode)
    };

    let input_power_w10 = data.input_power_w10();
    let output_power_w10 = data.output_power_w10();
    let output_current_ma = data.output_current_ma();
    let output_bus_mv = data.output_bus_mv();
    let charge_batt_ma = data.charge_current_ma();
    let tps_out_ma = data.output_current_ma();
    let batt_discharge_ma = data.battery_discharge_ma();
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
        match input_power_w10 {
            Some(pin_w10) => text(
                painter,
                variant,
                FontRole::NumBig,
                format_args!("{:>2}.{:01}", pin_w10 / 10, pin_w10 % 10),
                Point::new(14, kpi_value_y),
                HorizontalAlignment::Left,
                palette.bg,
            )?,
            None => text(
                painter,
                variant,
                FontRole::NumBig,
                "N/A",
                Point::new(14, kpi_value_y),
                HorizontalAlignment::Left,
                palette.bg,
            )?,
        }
        match output_power_w10 {
            Some(pout_w10) => text(
                painter,
                variant,
                FontRole::NumBig,
                format_args!("{:>2}.{:01}", pout_w10 / 10, pout_w10 % 10),
                Point::new(194, kpi_value_y),
                HorizontalAlignment::Right,
                palette.bg,
            )?,
            None => text(
                painter,
                variant,
                FontRole::NumBig,
                "N/A",
                Point::new(194, kpi_value_y),
                HorizontalAlignment::Right,
                palette.bg,
            )?,
        }
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
        match output_power_w10 {
            Some(pout_w10) => text(
                painter,
                variant,
                FontRole::NumBig,
                format_args!("{:>2}.{:01}", pout_w10 / 10, pout_w10 % 10),
                Point::new(14, kpi_value_y),
                HorizontalAlignment::Left,
                palette.bg,
            )?,
            None => text(
                painter,
                variant,
                FontRole::NumBig,
                "N/A",
                Point::new(14, kpi_value_y),
                HorizontalAlignment::Left,
                palette.bg,
            )?,
        }
        match output_current_ma {
            Some(iout_ma) => text(
                painter,
                variant,
                FontRole::NumBig,
                format_args!("{:>1}.{:01}", iout_ma / 1000, (iout_ma % 1000) / 100),
                Point::new(194, kpi_value_y),
                HorizontalAlignment::Right,
                palette.bg,
            )?,
            None => text(
                painter,
                variant,
                FontRole::NumBig,
                "N/A",
                Point::new(194, kpi_value_y),
                HorizontalAlignment::Right,
                palette.bg,
            )?,
        }
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
                if data.mains_present { "LOCK" } else { "NOAC" },
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
                0,
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
            match charge_batt_ma {
                Some(chg_ma) => text(
                    painter,
                    variant,
                    FontRole::Num,
                    format_args!("{:>1}.{:02}A", chg_ma / 1000, (chg_ma % 1000) / 10),
                    Point::new(194, 132),
                    HorizontalAlignment::Right,
                    palette.text,
                )?,
                None => text(
                    painter,
                    variant,
                    FontRole::Num,
                    "N/A",
                    Point::new(194, 132),
                    HorizontalAlignment::Right,
                    palette.text,
                )?,
            }
            draw_meter(
                painter,
                14,
                154,
                180,
                6,
                charge_batt_ma
                    .map(|ma| (u32::from(ma) * 100 / 1200).min(100))
                    .unwrap_or(0),
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
            match tps_out_ma {
                Some(out_ma) => text(
                    painter,
                    variant,
                    FontRole::Num,
                    format_args!("{:>1}.{:02}A", out_ma / 1000, (out_ma % 1000) / 10),
                    Point::new(194, 108),
                    HorizontalAlignment::Right,
                    palette.text,
                )?,
                None => text(
                    painter,
                    variant,
                    FontRole::Num,
                    "N/A",
                    Point::new(194, 108),
                    HorizontalAlignment::Right,
                    palette.text,
                )?,
            }
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
                match (tps_out_ma, output_current_ma) {
                    (Some(out_ma), Some(iout_ma)) if iout_ma > 0 => {
                        (out_ma * 100 / iout_ma).min(100)
                    }
                    _ => 0,
                },
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
            match output_bus_mv {
                Some(bus_mv) => text(
                    painter,
                    variant,
                    FontRole::Num,
                    format_args!("{:>2}.{:01}V", bus_mv / 1000, (bus_mv % 1000) / 100),
                    Point::new(194, 102),
                    HorizontalAlignment::Right,
                    palette.text,
                )?,
                None => text(
                    painter,
                    variant,
                    FontRole::Num,
                    "N/A",
                    Point::new(194, 102),
                    HorizontalAlignment::Right,
                    palette.text,
                )?,
            }
            text(
                painter,
                variant,
                FontRole::TextBody,
                "TEMP",
                Point::new(14, 126),
                HorizontalAlignment::Left,
                palette.text_dim,
            )?;
            match (data.therm_a_c, data.therm_b_c) {
                (Some(a), Some(b)) => text(
                    painter,
                    variant,
                    FontRole::Num,
                    format_args!("{:02}/{:02}C", a, b),
                    Point::new(194, 126),
                    HorizontalAlignment::Right,
                    palette.text,
                )?,
                _ => text(
                    painter,
                    variant,
                    FontRole::Num,
                    "N/A",
                    Point::new(194, 126),
                    HorizontalAlignment::Right,
                    palette.text,
                )?,
            }
            text(
                painter,
                variant,
                FontRole::TextBody,
                "SOC",
                Point::new(14, 150),
                HorizontalAlignment::Left,
                palette.text_dim,
            )?;
            match data.bms_soc_pct {
                Some(soc) => text(
                    painter,
                    variant,
                    FontRole::Num,
                    format_args!("{:>2}%", soc),
                    Point::new(194, 150),
                    HorizontalAlignment::Right,
                    palette.text,
                )?,
                None => text(
                    painter,
                    variant,
                    FontRole::Num,
                    "N/A",
                    Point::new(194, 150),
                    HorizontalAlignment::Right,
                    palette.text,
                )?,
            }
        }
    }

    let battery_note = if data.bms_state == SelfCheckCommState::Err {
        "FAULT"
    } else if data.bms_no_battery == Some(true) {
        "NOBAT"
    } else if data.bms_rca_alarm == Some(true) {
        "ALARM"
    } else if data.bms_discharge_ready == Some(false) {
        "BLOCK"
    } else if matches!(data.bms_current_ma, Some(ma) if ma < 0) {
        "DSG"
    } else if matches!(data.bms_current_ma, Some(ma) if ma > 0) {
        "CHG"
    } else {
        "READY"
    };
    let battery_note_color = comm_state_color(palette, data.bms_state);
    let charge_note = match data.charge_allowed {
        Some(true) => {
            if data.charger_state == SelfCheckCommState::Warn {
                "WARN"
            } else if data.chg_iin_ma.unwrap_or(0) > 80 {
                "CHG"
            } else {
                "READY"
            }
        }
        Some(false) => {
            if data.mains_present {
                "LOCK"
            } else {
                "NOAC"
            }
        }
        None => "N/A",
    };
    let charge_note_color = comm_state_color(palette, data.charger_state);
    let discharge_note = if data.bms_state == SelfCheckCommState::Err {
        "FAULT"
    } else if data.bms_no_battery == Some(true) {
        "NOBAT"
    } else if data.bms_discharge_ready == Some(false) {
        "BLOCK"
    } else {
        match data.mode {
            UpsMode::Off => "BYP",
            UpsMode::Standby => "IDLE",
            UpsMode::Supplement => "ASSIST",
            UpsMode::Backup => "LOAD",
        }
    };
    let discharge_note_color = comm_state_color(palette, data.bms_state);
    let battery_soc = data.bms_soc_pct.unwrap_or(0);
    let charge_current = charge_batt_ma.unwrap_or(0);
    let discharge_current = batt_discharge_ma.unwrap_or(0);
    let battery_value = match (data.batt_pack_mv, data.bms_soc_pct) {
        (Some(pack_mv), Some(_)) => format_args!(
            "{:>2}.{:01}V {:>2}%",
            pack_mv / 1000,
            (pack_mv % 1000) / 100,
            battery_soc
        ),
        (Some(pack_mv), None) => {
            format_args!("{:>2}.{:01}V N/A", pack_mv / 1000, (pack_mv % 1000) / 100)
        }
        (None, Some(_)) => format_args!("N/A {:>2}%", battery_soc),
        (None, None) => format_args!("N/A"),
    };
    let charge_value = match (charge_batt_ma, data.batt_pack_mv) {
        (Some(_), Some(pack_mv)) => format_args!(
            "{:>1}.{:02}A {:>2}.{:01}V",
            charge_current / 1000,
            (charge_current % 1000) / 10,
            pack_mv / 1000,
            (pack_mv % 1000) / 100
        ),
        (Some(_), None) => format_args!(
            "{:>1}.{:02}A N/A",
            charge_current / 1000,
            (charge_current % 1000) / 10
        ),
        (None, Some(pack_mv)) => {
            format_args!("N/A {:>2}.{:01}V", pack_mv / 1000, (pack_mv % 1000) / 100)
        }
        (None, None) => format_args!("N/A"),
    };
    let discharge_value = match (batt_discharge_ma, data.batt_pack_mv) {
        (Some(_), Some(pack_mv)) => format_args!(
            "{:>1}.{:02}A {:>2}.{:01}V",
            discharge_current / 1000,
            (discharge_current % 1000) / 10,
            pack_mv / 1000,
            (pack_mv % 1000) / 100
        ),
        (Some(_), None) => format_args!(
            "{:>1}.{:02}A N/A",
            discharge_current / 1000,
            (discharge_current % 1000) / 10
        ),
        (None, Some(pack_mv)) => {
            format_args!("N/A {:>2}.{:01}V", pack_mv / 1000, (pack_mv % 1000) / 100)
        }
        (None, None) => format_args!("N/A"),
    };

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
            value: battery_value,
            note: battery_note,
            note_color: battery_note_color,
            meter: data.bms_soc_pct.map(u32::from).unwrap_or(0),
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
            y: 72,
            w: 108,
            h: 48,
            title: "CHARGE",
            value: charge_value,
            note: charge_note,
            note_color: charge_note_color,
            meter: charge_batt_ma
                .map(|ma| (u32::from(ma) * 100 / 1200).min(100))
                .unwrap_or(0),
            active: data.charge_allowed == Some(true) && data.chg_iin_ma.is_some(),
            accent: palette.right,
        },
    )?;
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
            value: discharge_value,
            note: discharge_note,
            note_color: discharge_note_color,
            meter: batt_discharge_ma
                .map(|ma| (ma * 100 / 2200).min(100))
                .unwrap_or(0),
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
    overlay: SelfCheckOverlay,
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
    let bms_key = if snapshot.bq40z50 == SelfCheckCommState::Err {
        format_args!("NOT DETECTED")
    } else if snapshot.bq40z50_no_battery == Some(true) {
        format_args!("NO BATTERY")
    } else if snapshot.bq40z50_rca_alarm == Some(true) {
        format_args!("RCA ALARM")
    } else if snapshot.bq40z50 == SelfCheckCommState::Warn {
        format_args!("ABNORMAL")
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
            status_state: snapshot.gc9307,
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
            status_state: snapshot.tca6408a,
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
            status_state: snapshot.fusb302,
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
            status_state: snapshot.ina3221,
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
            status_state: snapshot.bq25792,
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
            status_state: snapshot.bq40z50,
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
            status_state: snapshot.tps_a,
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
            status_state: snapshot.tps_b,
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
            status_state: snapshot.tmp_a,
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
            status_state: snapshot.tmp_b,
            status: tmp_label(snapshot.tmp_b, snapshot.tmp_b_c),
            key: tmp_b_key,
            active: data.focus == UiFocus::Center,
            accent: palette.center,
        },
    )?;

    draw_self_check_overlay(painter, variant, palette, overlay)?;

    Ok(())
}

fn draw_self_check_overlay<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    overlay: SelfCheckOverlay,
) -> Result<(), P::Error> {
    if overlay == SelfCheckOverlay::None {
        return Ok(());
    }

    fill(
        painter,
        0,
        HEADER_H,
        UI_W,
        UI_H - HEADER_H,
        fade_color(palette.bg, 0x0000),
    )?;

    let dialog_border = fade_color(palette.left, palette.border);
    let dialog_fill = fade_color(palette.left, palette.panel_alt);
    let title_fill = fade_color(dialog_fill, palette.bg);
    let title_text = palette.text;
    let body_text = palette.text;
    let divider = fade_color(title_fill, palette.text_dim);

    let cancel_border = fade_color(palette.border, palette.text_dim);
    let cancel_fill = fade_color(palette.panel, palette.panel_alt);
    let cancel_text = palette.text;

    let confirm_border = fade_color(palette.right, 0x0000);
    let confirm_fill = palette.right;
    let confirm_text = fade_color(palette.bg, 0x0000);

    let title = match overlay {
        SelfCheckOverlay::BmsActivateConfirm | SelfCheckOverlay::BmsActivateProgress => {
            "BQ40 ACTIVATE"
        }
        SelfCheckOverlay::BmsActivateResult(..) => "BQ40 RESULT",
        SelfCheckOverlay::None => "",
    };

    fill(
        painter,
        SELF_CHECK_DIALOG_X,
        SELF_CHECK_DIALOG_Y,
        SELF_CHECK_DIALOG_W,
        SELF_CHECK_DIALOG_H,
        dialog_border,
    )?;
    fill(
        painter,
        SELF_CHECK_DIALOG_X + 1,
        SELF_CHECK_DIALOG_Y + 1,
        SELF_CHECK_DIALOG_W - 2,
        SELF_CHECK_DIALOG_H - 2,
        dialog_fill,
    )?;
    fill(
        painter,
        SELF_CHECK_DIALOG_X + 1,
        SELF_CHECK_DIALOG_Y + 1,
        SELF_CHECK_DIALOG_W - 2,
        20,
        title_fill,
    )?;
    fill(
        painter,
        SELF_CHECK_DIALOG_X + 1,
        SELF_CHECK_DIALOG_Y + 21,
        SELF_CHECK_DIALOG_W - 2,
        1,
        divider,
    )?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        title,
        Point::new(
            (SELF_CHECK_DIALOG_X + 10) as i32,
            (SELF_CHECK_DIALOG_Y + 4) as i32,
        ),
        HorizontalAlignment::Left,
        title_text,
    )?;

    match overlay {
        SelfCheckOverlay::BmsActivateConfirm => {
            text(
                painter,
                variant,
                FontRole::TextBody,
                "No SBS response yet.",
                Point::new(
                    (SELF_CHECK_DIALOG_X + 10) as i32,
                    (SELF_CHECK_DIALOG_Y + 26) as i32,
                ),
                HorizontalAlignment::Left,
                body_text,
            )?;
            text(
                painter,
                variant,
                FontRole::TextBody,
                "Try activation now?",
                Point::new(
                    (SELF_CHECK_DIALOG_X + 10) as i32,
                    (SELF_CHECK_DIALOG_Y + 46) as i32,
                ),
                HorizontalAlignment::Left,
                body_text,
            )?;

            fill(
                painter,
                SELF_CHECK_CANCEL_BTN_X,
                SELF_CHECK_CANCEL_BTN_Y,
                SELF_CHECK_CANCEL_BTN_W,
                SELF_CHECK_CANCEL_BTN_H,
                cancel_border,
            )?;
            fill(
                painter,
                SELF_CHECK_CANCEL_BTN_X + 1,
                SELF_CHECK_CANCEL_BTN_Y + 1,
                SELF_CHECK_CANCEL_BTN_W - 2,
                SELF_CHECK_CANCEL_BTN_H - 2,
                cancel_fill,
            )?;
            fill(
                painter,
                SELF_CHECK_CONFIRM_BTN_X,
                SELF_CHECK_CONFIRM_BTN_Y,
                SELF_CHECK_CONFIRM_BTN_W,
                SELF_CHECK_CONFIRM_BTN_H,
                confirm_border,
            )?;
            fill(
                painter,
                SELF_CHECK_CONFIRM_BTN_X + 1,
                SELF_CHECK_CONFIRM_BTN_Y + 1,
                SELF_CHECK_CONFIRM_BTN_W - 2,
                SELF_CHECK_CONFIRM_BTN_H - 2,
                confirm_fill,
            )?;
            text(
                painter,
                variant,
                FontRole::Num,
                "Cancel",
                Point::new(
                    (SELF_CHECK_CANCEL_BTN_X + (SELF_CHECK_CANCEL_BTN_W.saturating_sub(6 * 8)) / 2)
                        as i32,
                    (SELF_CHECK_CANCEL_BTN_Y + 6) as i32,
                ),
                HorizontalAlignment::Left,
                cancel_text,
            )?;
            text(
                painter,
                variant,
                FontRole::Num,
                "Activate",
                Point::new(
                    (SELF_CHECK_CONFIRM_BTN_X
                        + (SELF_CHECK_CONFIRM_BTN_W.saturating_sub(8 * 8)) / 2)
                        as i32,
                    (SELF_CHECK_CONFIRM_BTN_Y + 6) as i32,
                ),
                HorizontalAlignment::Left,
                confirm_text,
            )?;
        }
        SelfCheckOverlay::BmsActivateProgress => {
            let icon_x = SELF_CHECK_DIALOG_X + 10;
            let icon_y = SELF_CHECK_DIALOG_Y + 28;
            let text_x = SELF_CHECK_DIALOG_X + 50;
            draw_activation_icon(painter, icon_x, icon_y, ActivationIcon::Progress)?;
            text(
                painter,
                variant,
                FontRole::TextBody,
                "Applying wake profile.",
                Point::new(text_x as i32, (SELF_CHECK_DIALOG_Y + 28) as i32),
                HorizontalAlignment::Left,
                body_text,
            )?;
            text(
                painter,
                variant,
                FontRole::Num,
                "Checking pack state...",
                Point::new(text_x as i32, (SELF_CHECK_DIALOG_Y + 50) as i32),
                HorizontalAlignment::Left,
                PROGRESS_COLOR,
            )?;
        }
        SelfCheckOverlay::BmsActivateResult(result) => {
            let icon_x = SELF_CHECK_DIALOG_X + 10;
            let icon_y = SELF_CHECK_DIALOG_Y + 28;
            let text_x = SELF_CHECK_DIALOG_X + 50;
            let (headline, body1, body2, accent, icon) = match result {
                BmsResultKind::Success => (
                    "Activation succeeded.",
                    "BQ40Z50 is ready.",
                    "Tap to close",
                    SUCCESS_COLOR,
                    ActivationIcon::Success,
                ),
                BmsResultKind::NoBattery => (
                    "Battery not detected.",
                    "Check pack connection.",
                    "Tap to close",
                    PROGRESS_COLOR,
                    ActivationIcon::Failed,
                ),
                BmsResultKind::RomMode => (
                    "Gauge is in ROM mode.",
                    "Use BQ40 tool recovery.",
                    "Tap to close",
                    PROGRESS_COLOR,
                    ActivationIcon::Failed,
                ),
                BmsResultKind::Abnormal => (
                    "Gauge responded abnormally.",
                    "Review pack status.",
                    "Tap to close",
                    PROGRESS_COLOR,
                    ActivationIcon::Failed,
                ),
                BmsResultKind::NotDetected => (
                    "Still not detected.",
                    "Check power and wiring.",
                    "Tap to close",
                    ERROR_COLOR,
                    ActivationIcon::Failed,
                ),
            };
            draw_activation_icon(painter, icon_x, icon_y, icon)?;
            text(
                painter,
                variant,
                FontRole::TextBody,
                headline,
                Point::new(text_x as i32, (SELF_CHECK_DIALOG_Y + 28) as i32),
                HorizontalAlignment::Left,
                accent,
            )?;
            text(
                painter,
                variant,
                FontRole::TextBody,
                body1,
                Point::new(text_x as i32, (SELF_CHECK_DIALOG_Y + 46) as i32),
                HorizontalAlignment::Left,
                body_text,
            )?;
            text(
                painter,
                variant,
                FontRole::TextBody,
                body2,
                Point::new(text_x as i32, (SELF_CHECK_DIALOG_Y + 84) as i32),
                HorizontalAlignment::Left,
                palette.text_dim,
            )?;
        }
        SelfCheckOverlay::None => {}
    }

    Ok(())
}

#[derive(Clone, Copy)]
enum ActivationIcon {
    Progress,
    Success,
    Failed,
}

fn draw_activation_icon<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    icon: ActivationIcon,
) -> Result<(), P::Error> {
    // Icon source: Iconify / carbon.
    // Use original glyphs directly (no secondary reinterpretation/composition).
    let (icon_color, icon_blocks) = match icon {
        ActivationIcon::Progress => (PROGRESS_COLOR, CARBON_IN_PROGRESS_32),
        ActivationIcon::Success => (SUCCESS_COLOR, CARBON_CHECKMARK_OUTLINE_32),
        ActivationIcon::Failed => (ERROR_COLOR, CARBON_CLOSE_OUTLINE_32),
    };

    draw_icon_blocks(painter, x, y, icon_blocks, icon_color)?;

    Ok(())
}

fn draw_icon_blocks<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    blocks: &[(u8, u8, u8, u8)],
    rgb565: u16,
) -> Result<(), P::Error> {
    for &(bx, by, bw, bh) in blocks {
        if bw == 0 || bh == 0 {
            continue;
        }
        fill(
            painter,
            x + u16::from(bx),
            y + u16::from(by),
            u16::from(bw),
            u16::from(bh),
            rgb565,
        )?;
    }
    Ok(())
}

const CARBON_IN_PROGRESS_32: &[(u8, u8, u8, u8)] = &[
    (11, 2, 10, 1),
    (9, 3, 14, 1),
    (7, 4, 8, 1),
    (16, 4, 9, 1),
    (6, 5, 5, 1),
    (16, 5, 10, 1),
    (5, 6, 4, 1),
    (16, 6, 11, 1),
    (4, 7, 4, 1),
    (16, 7, 12, 1),
    (4, 8, 3, 1),
    (16, 8, 12, 1),
    (3, 9, 3, 1),
    (16, 9, 13, 1),
    (3, 10, 3, 1),
    (16, 10, 13, 1),
    (2, 11, 3, 1),
    (16, 11, 14, 1),
    (2, 12, 3, 1),
    (16, 12, 14, 1),
    (2, 13, 3, 1),
    (16, 13, 14, 1),
    (2, 14, 3, 1),
    (16, 14, 14, 1),
    (2, 15, 2, 1),
    (16, 15, 14, 1),
    (2, 16, 2, 1),
    (16, 16, 14, 1),
    (2, 17, 3, 1),
    (17, 17, 13, 1),
    (2, 18, 3, 1),
    (18, 18, 12, 1),
    (2, 19, 3, 1),
    (19, 19, 11, 1),
    (2, 20, 3, 1),
    (20, 20, 10, 1),
    (3, 21, 3, 1),
    (21, 21, 8, 1),
    (3, 22, 3, 1),
    (22, 22, 7, 1),
    (4, 23, 3, 1),
    (23, 23, 5, 1),
    (4, 24, 4, 1),
    (24, 24, 4, 1),
    (5, 25, 4, 1),
    (23, 25, 4, 1),
    (6, 26, 5, 1),
    (21, 26, 5, 1),
    (7, 27, 8, 1),
    (17, 27, 8, 1),
    (9, 28, 14, 1),
    (11, 29, 10, 1),
];

const CARBON_CHECKMARK_OUTLINE_32: &[(u8, u8, u8, u8)] = &[
    (11, 2, 10, 1),
    (9, 3, 14, 1),
    (7, 4, 8, 1),
    (17, 4, 8, 1),
    (6, 5, 5, 1),
    (21, 5, 5, 1),
    (5, 6, 4, 1),
    (23, 6, 4, 1),
    (4, 7, 4, 1),
    (24, 7, 4, 1),
    (4, 8, 3, 1),
    (25, 8, 3, 1),
    (3, 9, 3, 1),
    (26, 9, 3, 1),
    (3, 10, 3, 1),
    (26, 10, 3, 1),
    (2, 11, 3, 1),
    (20, 11, 3, 1),
    (27, 11, 3, 1),
    (2, 12, 3, 1),
    (19, 12, 4, 1),
    (27, 12, 3, 1),
    (2, 13, 3, 1),
    (18, 13, 5, 1),
    (27, 13, 3, 1),
    (2, 14, 3, 1),
    (17, 14, 5, 1),
    (27, 14, 3, 1),
    (2, 15, 2, 1),
    (9, 15, 3, 1),
    (16, 15, 5, 1),
    (28, 15, 2, 1),
    (2, 16, 2, 1),
    (9, 16, 4, 1),
    (15, 16, 5, 1),
    (28, 16, 2, 1),
    (2, 17, 3, 1),
    (9, 17, 10, 1),
    (27, 17, 3, 1),
    (2, 18, 3, 1),
    (10, 18, 8, 1),
    (27, 18, 3, 1),
    (2, 19, 3, 1),
    (11, 19, 6, 1),
    (27, 19, 3, 1),
    (2, 20, 3, 1),
    (12, 20, 4, 1),
    (27, 20, 3, 1),
    (3, 21, 3, 1),
    (13, 21, 2, 1),
    (26, 21, 3, 1),
    (3, 22, 3, 1),
    (26, 22, 3, 1),
    (4, 23, 3, 1),
    (25, 23, 3, 1),
    (4, 24, 4, 1),
    (24, 24, 4, 1),
    (5, 25, 4, 1),
    (23, 25, 4, 1),
    (6, 26, 5, 1),
    (21, 26, 5, 1),
    (7, 27, 8, 1),
    (17, 27, 8, 1),
    (9, 28, 14, 1),
    (11, 29, 10, 1),
];

const CARBON_CLOSE_OUTLINE_32: &[(u8, u8, u8, u8)] = &[
    (11, 2, 10, 1),
    (9, 3, 14, 1),
    (7, 4, 8, 1),
    (17, 4, 8, 1),
    (6, 5, 5, 1),
    (21, 5, 5, 1),
    (5, 6, 4, 1),
    (23, 6, 4, 1),
    (4, 7, 4, 1),
    (24, 7, 4, 1),
    (4, 8, 3, 1),
    (25, 8, 3, 1),
    (3, 9, 3, 1),
    (9, 9, 3, 1),
    (20, 9, 3, 1),
    (26, 9, 3, 1),
    (3, 10, 3, 1),
    (9, 10, 4, 1),
    (19, 10, 4, 1),
    (26, 10, 3, 1),
    (2, 11, 3, 1),
    (9, 11, 5, 1),
    (18, 11, 5, 1),
    (27, 11, 3, 1),
    (2, 12, 3, 1),
    (10, 12, 5, 1),
    (17, 12, 5, 1),
    (27, 12, 3, 1),
    (2, 13, 3, 1),
    (11, 13, 10, 1),
    (27, 13, 3, 1),
    (2, 14, 3, 1),
    (12, 14, 8, 1),
    (27, 14, 3, 1),
    (2, 15, 2, 1),
    (13, 15, 6, 1),
    (28, 15, 2, 1),
    (2, 16, 2, 1),
    (13, 16, 6, 1),
    (28, 16, 2, 1),
    (2, 17, 3, 1),
    (12, 17, 8, 1),
    (27, 17, 3, 1),
    (2, 18, 3, 1),
    (11, 18, 10, 1),
    (27, 18, 3, 1),
    (2, 19, 3, 1),
    (10, 19, 5, 1),
    (17, 19, 5, 1),
    (27, 19, 3, 1),
    (2, 20, 3, 1),
    (9, 20, 5, 1),
    (18, 20, 5, 1),
    (27, 20, 3, 1),
    (3, 21, 3, 1),
    (9, 21, 4, 1),
    (19, 21, 4, 1),
    (26, 21, 3, 1),
    (3, 22, 3, 1),
    (10, 22, 2, 1),
    (20, 22, 2, 1),
    (26, 22, 3, 1),
    (4, 23, 3, 1),
    (25, 23, 3, 1),
    (4, 24, 4, 1),
    (24, 24, 4, 1),
    (5, 25, 4, 1),
    (23, 25, 4, 1),
    (6, 26, 5, 1),
    (21, 26, 5, 1),
    (7, 27, 8, 1),
    (17, 27, 8, 1),
    (9, 28, 14, 1),
    (11, 29, 10, 1),
];

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

fn bms_label(state: SelfCheckCommState, _rca_alarm: Option<bool>) -> &'static str {
    match state {
        SelfCheckCommState::Pending => "PEND",
        SelfCheckCommState::Warn => "WARN",
        SelfCheckCommState::Err => "ERR",
        SelfCheckCommState::NotAvailable => "N/A",
        SelfCheckCommState::Ok => "OK",
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

fn comm_state_color(palette: Palette, state: SelfCheckCommState) -> u16 {
    match state {
        SelfCheckCommState::Ok => SUCCESS_COLOR,
        SelfCheckCommState::Warn => PROGRESS_COLOR,
        SelfCheckCommState::Err => ERROR_COLOR,
        SelfCheckCommState::Pending | SelfCheckCommState::NotAvailable => palette.text_dim,
    }
}

fn contains(x: u16, y: u16, rx: u16, ry: u16, rw: u16, rh: u16) -> bool {
    x >= rx && y >= ry && x < rx + rw && y < ry + rh
}

fn render_variant_d<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    data: DashboardData,
    self_check: Option<&SelfCheckUiSnapshot>,
) -> Result<(), P::Error> {
    render_variant_b(painter, variant, palette, data, self_check)
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
    status_state: SelfCheckCommState,
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
    let status_color = if spec.active {
        palette.bg
    } else if spec.status_state == SelfCheckCommState::Err {
        ERROR_COLOR
    } else {
        dim_color
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
        status_color,
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
    note_color: u16,
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
        if spec.active {
            text_color
        } else {
            spec.note_color
        },
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

#[cfg(test)]
mod tests {
    use super::*;

    fn base_model(mode: UpsMode) -> DashboardData {
        DashboardData::from_model(&UiModel {
            mode,
            focus: UiFocus::Idle,
            touch_irq: false,
            frame_no: 0,
        })
    }

    #[test]
    fn live_dashboard_keeps_missing_metrics_as_na_inputs() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.fusb302_vbus_present = Some(true);
        snapshot.bq25792 = SelfCheckCommState::Ok;
        snapshot.bq40z50 = SelfCheckCommState::Ok;
        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);

        assert!(!live.mains_present);
        assert_eq!(live.input_power_w10(), None);
        assert_eq!(live.output_power_w10(), None);
        assert_eq!(live.output_current_ma(), None);
        assert_eq!(live.charge_current_ma(), None);
    }

    #[test]
    fn live_dashboard_uses_real_snapshot_metrics_without_demo_fallback() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Backup);
        snapshot.fusb302_vbus_present = Some(false);
        snapshot.vin_vbus_mv = Some(19_200);
        snapshot.vin_iin_ma = Some(910);
        snapshot.tps_a_enabled = Some(true);
        snapshot.out_a_vbus_mv = Some(18_860);
        snapshot.tps_a_iout_ma = Some(980);
        snapshot.tps_b_enabled = Some(true);
        snapshot.out_b_vbus_mv = Some(18_830);
        snapshot.tps_b_iout_ma = Some(920);
        snapshot.bq25792_allow_charge = Some(false);
        snapshot.bq40z50 = SelfCheckCommState::Ok;
        snapshot.bq40z50_pack_mv = Some(14_820);
        snapshot.bq40z50_current_ma = Some(-1880);
        snapshot.bq40z50_soc_pct = Some(53);
        snapshot.tmp_a_c = Some(41);
        snapshot.tmp_b_c = Some(39);

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Backup), &snapshot);

        assert!(live.mains_present);
        assert_eq!(live.input_power_w10(), Some(174));
        assert_eq!(live.output_bus_mv(), Some(18_845));
        assert_eq!(live.output_current_ma(), Some(1_900));
        assert_eq!(live.output_power_w10(), Some(358));
        assert_eq!(live.battery_discharge_ma(), Some(1_880));
        assert_eq!(live.battery_max_temp_c(), Some(41));
    }

    #[test]
    fn live_dashboard_clamps_reverse_input_current_to_zero_power() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.fusb302_vbus_present = Some(true);
        snapshot.vin_vbus_mv = Some(20_100);
        snapshot.vin_iin_ma = Some(-1_250);

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);

        assert!(live.mains_present);
        assert_eq!(live.input_power_w10(), Some(0));
    }

    #[test]
    fn live_dashboard_keeps_zero_input_current_as_zero_power() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.fusb302_vbus_present = Some(true);
        snapshot.vin_vbus_mv = Some(20_100);
        snapshot.vin_iin_ma = Some(0);

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);

        assert!(live.mains_present);
        assert_eq!(live.input_power_w10(), Some(0));
    }

    #[test]
    fn live_dashboard_keeps_invalid_input_sample_as_na() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.fusb302_vbus_present = Some(true);
        snapshot.vin_vbus_mv = None;
        snapshot.vin_iin_ma = None;

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);

        assert!(!live.mains_present);
        assert_eq!(live.input_power_w10(), None);
    }

    #[test]
    fn live_dashboard_ignores_charger_present_when_vin_is_below_threshold() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.fusb302_vbus_present = Some(true);
        snapshot.vin_vbus_mv = Some(2_900);

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);

        assert!(!live.mains_present);
    }

    #[test]
    fn live_dashboard_keeps_mains_present_when_vin_is_online_without_charger_flag() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.fusb302_vbus_present = Some(false);
        snapshot.vin_vbus_mv = Some(19_200);

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);

        assert!(live.mains_present);
    }

    #[test]
    fn live_dashboard_keeps_latched_mains_when_vin_sample_is_temporarily_missing() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.vin_mains_present = Some(true);
        snapshot.vin_vbus_mv = None;

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);

        assert!(live.mains_present);
    }
}
