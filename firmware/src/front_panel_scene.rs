use embedded_graphics_core::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Point, Size},
    pixelcolor::{raw::RawU16, Rgb565},
    prelude::RawData,
    Pixel,
};
use esp_firmware::output_state::{EnabledOutputs, OutputGateReason, OutputSelector};
use u8g2_fonts::{
    fonts,
    types::{FontColor, HorizontalAlignment, VerticalPosition},
    Content, Error as FontError, FontRenderer,
};

pub const UI_W: u16 = 320;
pub const UI_H: u16 = 172;
const VIN_MAINS_PRESENT_THRESHOLD_MV: u16 = 3_000;

fn mains_present_from_vin(vin_vbus_mv: Option<u16>) -> Option<bool> {
    vin_vbus_mv.map(|mv| mv >= VIN_MAINS_PRESENT_THRESHOLD_MV)
}

fn snapshot_mains_present_value(snapshot: &SelfCheckUiSnapshot) -> Option<bool> {
    mains_present_from_vin(snapshot.vin_vbus_mv)
        .or(snapshot.vin_mains_present)
        .or(snapshot.fusb302_vbus_present)
}

fn snapshot_mains_present(snapshot: &SelfCheckUiSnapshot) -> bool {
    snapshot_mains_present_value(snapshot).unwrap_or(false)
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
const DETAIL_TITLE_X: i32 = 74;
const DETAIL_STATUS_X: i32 = (UI_W - 8) as i32;
const DETAIL_ROW_Y_1: u16 = 78;
const DETAIL_ROW_Y_2: u16 = 94;
const DETAIL_ROW_Y_3: u16 = 110;
const DETAIL_ROW_Y_4: u16 = 126;

// User preference: non-numeric text uses Font A, numeric fields use fixed-width Font B.
static FONT_A_TITLE: FontRenderer = FontRenderer::new::<fonts::u8g2_font_8x13B_tf>();
static FONT_A_BODY: FontRenderer = FontRenderer::new::<fonts::u8g2_font_7x14B_tf>();
static FONT_B_NUM: FontRenderer = FontRenderer::new::<fonts::u8g2_font_8x13_mf>();
static FONT_B_NUM_BIG: FontRenderer = FontRenderer::new::<fonts::u8g2_font_t0_22b_tn>();
static FONT_B_NUM_HERO: FontRenderer = FontRenderer::new::<fonts::u8g2_font_t0_30b_tn>();
static FONT_A_DETAIL: FontRenderer = FontRenderer::new::<fonts::u8g2_font_9x15B_tf>();
static FONT_B_DETAIL: FontRenderer = FontRenderer::new::<fonts::u8g2_font_9x15_mf>();
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
    DetailTitle,
    DetailBody,
    Num,
    NumCompact,
    DetailNum,
    NumBig,
    NumHero,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpsMode {
    Off,
    Standby,
    Supplement,
    Backup,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DashboardDetailPage {
    Cells,
    BatteryFlow,
    Output,
    Charger,
    Thermal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DashboardRoute {
    Home,
    Detail(DashboardDetailPage),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DashboardTouchTarget {
    HomeOutput,
    HomeThermal,
    HomeCells,
    HomeCharger,
    HomeBatteryFlow,
    DetailBack,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DashboardInputSource {
    DcIn,
    UsbC,
    Auto,
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
pub struct DashboardDetailSnapshot {
    pub cell_mv: [Option<u16>; 4],
    pub cell_temp_c: [Option<i16>; 4],
    pub balance_active: Option<bool>,
    pub balance_mask: Option<u8>,
    pub balance_cell: Option<u8>,
    pub battery_energy_mwh: Option<u32>,
    pub battery_full_capacity_mwh: Option<u32>,
    pub charge_fet_on: Option<bool>,
    pub discharge_fet_on: Option<bool>,
    pub precharge_fet_on: Option<bool>,
    pub input_source: Option<DashboardInputSource>,
    pub charger_active: Option<bool>,
    pub charger_status: Option<&'static str>,
    pub out_a_temp_c: Option<i16>,
    pub out_b_temp_c: Option<i16>,
    pub board_temp_c: Option<i16>,
    pub battery_temp_c: Option<i16>,
    pub fan_rpm: Option<u16>,
    pub fan_pwm_pct: Option<u8>,
    pub fan_status: Option<&'static str>,
    pub cells_notice: Option<&'static str>,
    pub battery_notice: Option<&'static str>,
    pub output_notice: Option<&'static str>,
    pub charger_notice: Option<&'static str>,
    pub thermal_notice: Option<&'static str>,
}

impl DashboardDetailSnapshot {
    pub const fn pending() -> Self {
        Self {
            cell_mv: [None, None, None, None],
            cell_temp_c: [None, None, None, None],
            balance_active: None,
            balance_mask: None,
            balance_cell: None,
            battery_energy_mwh: None,
            battery_full_capacity_mwh: None,
            charge_fet_on: None,
            discharge_fet_on: None,
            precharge_fet_on: None,
            input_source: None,
            charger_active: None,
            charger_status: None,
            out_a_temp_c: None,
            out_b_temp_c: None,
            board_temp_c: None,
            battery_temp_c: None,
            fan_rpm: None,
            fan_pwm_pct: None,
            fan_status: None,
            cells_notice: None,
            battery_notice: None,
            output_notice: None,
            charger_notice: None,
            thermal_notice: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelfCheckUiSnapshot {
    pub mode: UpsMode,
    pub requested_outputs: EnabledOutputs,
    pub active_outputs: EnabledOutputs,
    pub recoverable_outputs: EnabledOutputs,
    pub output_gate_reason: OutputGateReason,
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
    pub bq40z50_recovery_pending: bool,
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
    pub dashboard_detail: DashboardDetailSnapshot,
}

impl SelfCheckUiSnapshot {
    pub const fn pending(mode: UpsMode) -> Self {
        Self {
            mode,
            requested_outputs: EnabledOutputs::None,
            active_outputs: EnabledOutputs::None,
            recoverable_outputs: EnabledOutputs::None,
            output_gate_reason: OutputGateReason::None,
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
            bq40z50_recovery_pending: false,
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
            dashboard_detail: DashboardDetailSnapshot::pending(),
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

const DASHBOARD_HOME_OUTPUT_X: u16 = 6;
const DASHBOARD_HOME_OUTPUT_Y: u16 = 22;
const DASHBOARD_HOME_OUTPUT_W: u16 = 196;
const DASHBOARD_HOME_OUTPUT_H: u16 = 52;

const DASHBOARD_HOME_THERMAL_X: u16 = 6;
const DASHBOARD_HOME_THERMAL_Y: u16 = 76;
const DASHBOARD_HOME_THERMAL_W: u16 = 196;
const DASHBOARD_HOME_THERMAL_H: u16 = 94;

const DASHBOARD_HOME_CELLS_X: u16 = 206;
const DASHBOARD_HOME_CELLS_Y: u16 = 22;
const DASHBOARD_HOME_CELLS_W: u16 = 108;
const DASHBOARD_HOME_CELLS_H: u16 = 48;

const DASHBOARD_HOME_CHARGER_X: u16 = 206;
const DASHBOARD_HOME_CHARGER_Y: u16 = 72;
const DASHBOARD_HOME_CHARGER_W: u16 = 108;
const DASHBOARD_HOME_CHARGER_H: u16 = 48;

const DASHBOARD_HOME_BATTERY_FLOW_X: u16 = 206;
const DASHBOARD_HOME_BATTERY_FLOW_Y: u16 = 122;
const DASHBOARD_HOME_BATTERY_FLOW_W: u16 = 108;
const DASHBOARD_HOME_BATTERY_FLOW_H: u16 = 48;

const DASHBOARD_DETAIL_BACK_X: u16 = 8;
const DASHBOARD_DETAIL_BACK_Y: u16 = 2;
const DASHBOARD_DETAIL_BACK_W: u16 = 56;
const DASHBOARD_DETAIL_BACK_H: u16 = 14;

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

fn outputs_include(snapshot: &SelfCheckUiSnapshot, selector: OutputSelector) -> bool {
    matches!(
        (snapshot.requested_outputs, selector),
        (EnabledOutputs::Both, _)
            | (
                EnabledOutputs::Only(OutputSelector::OutA),
                OutputSelector::OutA
            )
            | (
                EnabledOutputs::Only(OutputSelector::OutB),
                OutputSelector::OutB
            )
    )
}

fn active_outputs_include(snapshot: &SelfCheckUiSnapshot, selector: OutputSelector) -> bool {
    matches!(
        (snapshot.active_outputs, selector),
        (EnabledOutputs::Both, _)
            | (
                EnabledOutputs::Only(OutputSelector::OutA),
                OutputSelector::OutA
            )
            | (
                EnabledOutputs::Only(OutputSelector::OutB),
                OutputSelector::OutB
            )
    )
}

fn bms_limited(snapshot: &SelfCheckUiSnapshot) -> bool {
    snapshot.bq40z50 != SelfCheckCommState::Err
        && !snapshot.bq40z50_recovery_pending
        && snapshot.bq40z50_no_battery != Some(true)
        && snapshot.bq40z50_discharge_ready == Some(false)
}

fn output_hold_for(snapshot: &SelfCheckUiSnapshot, selector: OutputSelector) -> bool {
    outputs_include(snapshot, selector)
        && !active_outputs_include(snapshot, selector)
        && snapshot.output_gate_reason == OutputGateReason::BmsNotReady
}

fn snapshot_tps_state(
    snapshot: &SelfCheckUiSnapshot,
    selector: OutputSelector,
) -> SelfCheckCommState {
    match selector {
        OutputSelector::OutA => snapshot.tps_a,
        OutputSelector::OutB => snapshot.tps_b,
    }
}

fn snapshot_tps_enabled(snapshot: &SelfCheckUiSnapshot, selector: OutputSelector) -> Option<bool> {
    match selector {
        OutputSelector::OutA => snapshot.tps_a_enabled,
        OutputSelector::OutB => snapshot.tps_b_enabled,
    }
}

fn self_check_tps_summary_name(
    snapshot: &SelfCheckUiSnapshot,
    selector: OutputSelector,
) -> &'static str {
    match snapshot_tps_state(snapshot, selector) {
        SelfCheckCommState::Pending => "pending",
        SelfCheckCommState::Ok => "ok",
        SelfCheckCommState::Warn => "warn",
        SelfCheckCommState::Err => "err",
        SelfCheckCommState::NotAvailable => "na",
    }
}

pub fn self_check_tps_a_summary_name(snapshot: &SelfCheckUiSnapshot) -> &'static str {
    self_check_tps_summary_name(snapshot, OutputSelector::OutA)
}

pub fn self_check_tps_b_summary_name(snapshot: &SelfCheckUiSnapshot) -> &'static str {
    self_check_tps_summary_name(snapshot, OutputSelector::OutB)
}

pub fn self_check_can_enter_dashboard(snapshot: &SelfCheckUiSnapshot) -> bool {
    fn state_ok(state: SelfCheckCommState) -> bool {
        matches!(
            state,
            SelfCheckCommState::Ok | SelfCheckCommState::NotAvailable
        )
    }

    let bms_clear = snapshot.bq40z50 == SelfCheckCommState::Ok
        && snapshot.bq40z50_no_battery != Some(true)
        && snapshot.bq40z50_discharge_ready != Some(false)
        && !snapshot.bq40z50_recovery_pending;
    let out_a_clear = !outputs_include(snapshot, OutputSelector::OutA)
        || (state_ok(snapshot.tps_a)
            && !output_hold_for(snapshot, OutputSelector::OutA)
            && active_outputs_include(snapshot, OutputSelector::OutA));
    let out_b_clear = !outputs_include(snapshot, OutputSelector::OutB)
        || (state_ok(snapshot.tps_b)
            && !output_hold_for(snapshot, OutputSelector::OutB)
            && active_outputs_include(snapshot, OutputSelector::OutB));

    state_ok(snapshot.gc9307)
        && state_ok(snapshot.tca6408a)
        && state_ok(snapshot.fusb302)
        && state_ok(snapshot.ina3221)
        && state_ok(snapshot.bq25792)
        && bms_clear
        && snapshot.output_gate_reason == OutputGateReason::None
        && out_a_clear
        && out_b_clear
        && state_ok(snapshot.tmp_a)
        && state_ok(snapshot.tmp_b)
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
pub fn dashboard_hit_test(route: DashboardRoute, x: u16, y: u16) -> Option<DashboardTouchTarget> {
    match route {
        DashboardRoute::Home => {
            if contains(
                x,
                y,
                DASHBOARD_HOME_OUTPUT_X,
                DASHBOARD_HOME_OUTPUT_Y,
                DASHBOARD_HOME_OUTPUT_W,
                DASHBOARD_HOME_OUTPUT_H,
            ) {
                Some(DashboardTouchTarget::HomeOutput)
            } else if contains(
                x,
                y,
                DASHBOARD_HOME_THERMAL_X,
                DASHBOARD_HOME_THERMAL_Y,
                DASHBOARD_HOME_THERMAL_W,
                DASHBOARD_HOME_THERMAL_H,
            ) {
                Some(DashboardTouchTarget::HomeThermal)
            } else if contains(
                x,
                y,
                DASHBOARD_HOME_CELLS_X,
                DASHBOARD_HOME_CELLS_Y,
                DASHBOARD_HOME_CELLS_W,
                DASHBOARD_HOME_CELLS_H,
            ) {
                Some(DashboardTouchTarget::HomeCells)
            } else if contains(
                x,
                y,
                DASHBOARD_HOME_CHARGER_X,
                DASHBOARD_HOME_CHARGER_Y,
                DASHBOARD_HOME_CHARGER_W,
                DASHBOARD_HOME_CHARGER_H,
            ) {
                Some(DashboardTouchTarget::HomeCharger)
            } else if contains(
                x,
                y,
                DASHBOARD_HOME_BATTERY_FLOW_X,
                DASHBOARD_HOME_BATTERY_FLOW_Y,
                DASHBOARD_HOME_BATTERY_FLOW_W,
                DASHBOARD_HOME_BATTERY_FLOW_H,
            ) {
                Some(DashboardTouchTarget::HomeBatteryFlow)
            } else {
                None
            }
        }
        DashboardRoute::Detail(_) => {
            if contains(
                x,
                y,
                DASHBOARD_DETAIL_BACK_X,
                DASHBOARD_DETAIL_BACK_Y,
                DASHBOARD_DETAIL_BACK_W,
                DASHBOARD_DETAIL_BACK_H,
            ) {
                Some(DashboardTouchTarget::DetailBack)
            } else {
                None
            }
        }
    }
}

#[allow(dead_code)]
pub const fn dashboard_route_for_target(target: DashboardTouchTarget) -> DashboardRoute {
    match target {
        DashboardTouchTarget::HomeOutput => DashboardRoute::Detail(DashboardDetailPage::Output),
        DashboardTouchTarget::HomeThermal => DashboardRoute::Detail(DashboardDetailPage::Thermal),
        DashboardTouchTarget::HomeCells => DashboardRoute::Detail(DashboardDetailPage::Cells),
        DashboardTouchTarget::HomeCharger => DashboardRoute::Detail(DashboardDetailPage::Charger),
        DashboardTouchTarget::HomeBatteryFlow => {
            DashboardRoute::Detail(DashboardDetailPage::BatteryFlow)
        }
        DashboardTouchTarget::DetailBack => DashboardRoute::Home,
    }
}

#[allow(dead_code)]
pub fn dashboard_route_has_active_animation(
    route: DashboardRoute,
    snapshot: &SelfCheckUiSnapshot,
) -> bool {
    matches!(route, DashboardRoute::Detail(DashboardDetailPage::Thermal))
        && thermal_fan_motion(
            snapshot.dashboard_detail.fan_rpm,
            snapshot.dashboard_detail.fan_pwm_pct,
            snapshot.dashboard_detail.fan_status,
        ) != ThermalFanMotion::Off
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
    frame_no: u32,
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
            frame_no: model.frame_no,
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
    frame_no: u32,
    mains_present: bool,
    requested_outputs: EnabledOutputs,
    active_outputs: EnabledOutputs,
    output_gate_reason: OutputGateReason,
    out_a_on: bool,
    out_b_on: bool,
    bms_on: bool,
    charger_input_vbus_mv: Option<u16>,
    charger_input_ibus_ma: Option<i32>,
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
    therm_a_state: SelfCheckCommState,
    therm_b_state: SelfCheckCommState,
    charge_allowed: Option<bool>,
    bms_state: SelfCheckCommState,
    charger_state: SelfCheckCommState,
    tps_a_state: SelfCheckCommState,
    tps_b_state: SelfCheckCommState,
    bms_rca_alarm: Option<bool>,
    bms_no_battery: Option<bool>,
    bms_discharge_ready: Option<bool>,
    bms_recovery_pending: bool,
    detail: DashboardDetailSnapshot,
}

impl DashboardLiveData {
    fn from_snapshot(model: DashboardData, snapshot: &SelfCheckUiSnapshot) -> Self {
        Self {
            mode: snapshot.mode,
            focus: model.focus,
            touch_irq: model.touch_irq,
            frame_no: model.frame_no,
            mains_present: snapshot_mains_present(snapshot),
            requested_outputs: snapshot.requested_outputs,
            active_outputs: snapshot.active_outputs,
            output_gate_reason: snapshot.output_gate_reason,
            out_a_on: snapshot.tps_a_enabled == Some(true),
            out_b_on: snapshot.tps_b_enabled == Some(true),
            bms_on: model.bms_on,
            charger_input_vbus_mv: snapshot.input_vbus_mv,
            charger_input_ibus_ma: snapshot.input_ibus_ma,
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
            therm_a_state: snapshot.tmp_a,
            therm_b_state: snapshot.tmp_b,
            charge_allowed: snapshot.bq25792_allow_charge,
            bms_state: snapshot.bq40z50,
            charger_state: snapshot.bq25792,
            tps_a_state: snapshot.tps_a,
            tps_b_state: snapshot.tps_b,
            bms_rca_alarm: snapshot.bq40z50_rca_alarm,
            bms_no_battery: snapshot.bq40z50_no_battery,
            bms_discharge_ready: snapshot.bq40z50_discharge_ready,
            bms_recovery_pending: snapshot.bq40z50_recovery_pending,
            detail: snapshot.dashboard_detail,
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
        if let (Some(vbus_mv), Some(ibus_ma)) =
            (self.charger_input_vbus_mv, self.charger_input_ibus_ma)
        {
            return Some((vbus_mv as u32 * ibus_ma.max(0) as u32) / 100_000);
        }

        let vin_ma = self.vin_iin_ma?;
        Some((self.vin_vbus_mv? as u32 * vin_ma.max(0) as u32) / 100_000)
    }

    fn battery_charge_power_w10(self) -> Option<u32> {
        Some((self.batt_pack_mv? as u32 * self.charge_current_ma()? as u32) / 100_000)
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

    fn output_requested(self, selector: OutputSelector) -> bool {
        matches!(
            (self.requested_outputs, selector),
            (EnabledOutputs::Both, _)
                | (
                    EnabledOutputs::Only(OutputSelector::OutA),
                    OutputSelector::OutA
                )
                | (
                    EnabledOutputs::Only(OutputSelector::OutB),
                    OutputSelector::OutB
                )
        )
    }

    fn output_hold(self, selector: OutputSelector) -> bool {
        self.output_requested(selector)
            && self.output_gate_reason == OutputGateReason::BmsNotReady
            && !matches!(
                (self.active_outputs, selector),
                (EnabledOutputs::Both, _)
                    | (
                        EnabledOutputs::Only(OutputSelector::OutA),
                        OutputSelector::OutA
                    )
                    | (
                        EnabledOutputs::Only(OutputSelector::OutB),
                        OutputSelector::OutB
                    )
            )
    }

    fn output_recovery_pending(self, selector: OutputSelector) -> bool {
        self.output_hold(selector) && self.bms_recovery_pending
    }

    fn page_notice(self, page: DashboardDetailPage) -> &'static str {
        match page {
            DashboardDetailPage::Cells => self
                .detail
                .cells_notice
                .unwrap_or("CELL DETAIL SOURCE PENDING"),
            DashboardDetailPage::BatteryFlow => self
                .detail
                .battery_notice
                .unwrap_or("PACK DETAIL SOURCE PENDING"),
            DashboardDetailPage::Output => self
                .detail
                .output_notice
                .unwrap_or("OUTPUT DETAIL SOURCE PENDING"),
            DashboardDetailPage::Charger => self
                .detail
                .charger_notice
                .unwrap_or("DETAIL UI ONLY - SOURCE PENDING"),
            DashboardDetailPage::Thermal => self
                .detail
                .thermal_notice
                .unwrap_or("DETAIL UI ONLY - FAN SOURCE PENDING"),
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
    render_frame_with_dashboard_route_overlay(
        painter,
        model,
        variant,
        DashboardRoute::Home,
        None,
        SelfCheckOverlay::None,
    )
}

#[allow(dead_code)]
pub fn render_frame_with_self_check<P: UiPainter>(
    painter: &mut P,
    model: &UiModel,
    variant: UiVariant,
    self_check: Option<&SelfCheckUiSnapshot>,
) -> Result<(), P::Error> {
    render_frame_with_dashboard_route_overlay(
        painter,
        model,
        variant,
        DashboardRoute::Home,
        self_check,
        SelfCheckOverlay::None,
    )
}

#[allow(dead_code)]
pub fn render_frame_with_self_check_overlay<P: UiPainter>(
    painter: &mut P,
    model: &UiModel,
    variant: UiVariant,
    self_check: Option<&SelfCheckUiSnapshot>,
    overlay: SelfCheckOverlay,
) -> Result<(), P::Error> {
    render_frame_with_dashboard_route_overlay(
        painter,
        model,
        variant,
        DashboardRoute::Home,
        self_check,
        overlay,
    )
}

pub fn render_frame_with_dashboard_route_overlay<P: UiPainter>(
    painter: &mut P,
    model: &UiModel,
    variant: UiVariant,
    dashboard_route: DashboardRoute,
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
        UiVariant::InstrumentB => {
            render_variant_b(painter, variant, palette, data, dashboard_route, self_check)?
        }
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
    render_variant_b(
        painter,
        variant,
        palette,
        data,
        DashboardRoute::Home,
        self_check,
    )
}

fn render_variant_b<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    data: DashboardData,
    dashboard_route: DashboardRoute,
    self_check: Option<&SelfCheckUiSnapshot>,
) -> Result<(), P::Error> {
    if let Some(snapshot) = self_check {
        return render_variant_b_live(
            painter,
            variant,
            palette,
            dashboard_route,
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
            FontRole::NumHero,
            format_args!("{:>2}.{:01}", input_power_w10 / 10, input_power_w10 % 10),
            Point::new(14, kpi_value_y),
            HorizontalAlignment::Left,
            palette.bg,
        )?;
        text(
            painter,
            variant,
            FontRole::NumHero,
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
            FontRole::NumHero,
            format_args!("{:>2}.{:01}", output_power_w10 / 10, output_power_w10 % 10),
            Point::new(14, kpi_value_y),
            HorizontalAlignment::Left,
            palette.bg,
        )?;
        text(
            painter,
            variant,
            FontRole::NumHero,
            format_args!(
                "{:>1}.{:01}",
                (output_current_ma / 1000),
                ((output_current_ma % 1000) / 100)
            ),
            Point::new(194, kpi_value_y),
            HorizontalAlignment::Left,
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
                FontRole::DetailNum,
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
                FontRole::DetailNum,
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
                FontRole::DetailNum,
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
                FontRole::DetailNum,
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
                FontRole::DetailNum,
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
                FontRole::DetailNum,
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
    dashboard_route: DashboardRoute,
    data: DashboardLiveData,
) -> Result<(), P::Error> {
    if let DashboardRoute::Detail(page) = dashboard_route {
        return render_dashboard_detail_page(painter, variant, palette, data, page);
    }

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
    let headline_output_power_w10 = output_power_w10.or({
        if !data.out_a_on && !data.out_b_on {
            Some(0)
        } else {
            None
        }
    });
    let headline_output_current_ma = output_current_ma.or({
        if !data.out_a_on && !data.out_b_on {
            Some(0)
        } else {
            None
        }
    });
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
                FontRole::NumHero,
                format_args!("{:>2}.{:01}", pin_w10 / 10, pin_w10 % 10),
                Point::new(14, kpi_value_y),
                HorizontalAlignment::Left,
                palette.bg,
            )?,
            None => text(
                painter,
                variant,
                FontRole::DetailNum,
                "N/A",
                Point::new(14, kpi_value_y),
                HorizontalAlignment::Left,
                palette.bg,
            )?,
        }
        match headline_output_power_w10 {
            Some(pout_w10) => text(
                painter,
                variant,
                FontRole::NumHero,
                format_args!("{:>2}.{:01}", pout_w10 / 10, pout_w10 % 10),
                Point::new(194, kpi_value_y),
                HorizontalAlignment::Right,
                palette.bg,
            )?,
            None => text(
                painter,
                variant,
                FontRole::DetailNum,
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
        match headline_output_power_w10 {
            Some(pout_w10) => text(
                painter,
                variant,
                FontRole::NumHero,
                format_args!("{:>2}.{:01}", pout_w10 / 10, pout_w10 % 10),
                Point::new(14, kpi_value_y),
                HorizontalAlignment::Left,
                palette.bg,
            )?,
            None => text(
                painter,
                variant,
                FontRole::DetailNum,
                "N/A",
                Point::new(14, kpi_value_y),
                HorizontalAlignment::Left,
                palette.bg,
            )?,
        }
        match headline_output_current_ma {
            Some(iout_ma) => text(
                painter,
                variant,
                FontRole::NumHero,
                format_args!("{:>1}.{:01}", iout_ma / 1000, (iout_ma % 1000) / 100),
                Point::new(194, kpi_value_y),
                HorizontalAlignment::Right,
                palette.bg,
            )?,
            None => text(
                painter,
                variant,
                FontRole::DetailNum,
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
                FontRole::DetailNum,
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
                FontRole::DetailNum,
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
                FontRole::DetailNum,
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
                    FontRole::DetailNum,
                    format_args!("{:>1}.{:02}A", chg_ma / 1000, (chg_ma % 1000) / 10),
                    Point::new(194, 132),
                    HorizontalAlignment::Right,
                    palette.text,
                )?,
                None => text(
                    painter,
                    variant,
                    FontRole::DetailNum,
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
                    FontRole::DetailNum,
                    format_args!("{:>1}.{:02}A", out_ma / 1000, (out_ma % 1000) / 10),
                    Point::new(194, 108),
                    HorizontalAlignment::Right,
                    palette.text,
                )?,
                None => text(
                    painter,
                    variant,
                    FontRole::DetailNum,
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
                FontRole::DetailNum,
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
    } else if data.bms_recovery_pending {
        "RECOV"
    } else if data.bms_no_battery == Some(true) {
        "NOBAT"
    } else if data.bms_rca_alarm == Some(true) {
        "ALARM"
    } else if data.bms_discharge_ready == Some(false) {
        "LIMIT"
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
                "IDLE"
            } else {
                "NOAC"
            }
        }
        None => "N/A",
    };
    let charge_note_color = comm_state_color(palette, data.charger_state);
    let discharge_note =
        if data.output_hold(OutputSelector::OutA) || data.output_hold(OutputSelector::OutB) {
            if data.bms_recovery_pending {
                "RECOV"
            } else {
                "HOLD"
            }
        } else if data.bms_state == SelfCheckCommState::Err {
            "FAULT"
        } else if data.bms_no_battery == Some(true) {
            "NOBAT"
        } else if data.bms_discharge_ready == Some(false) {
            "LIMIT"
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

    draw_dashboard_entry_marker(
        painter,
        DASHBOARD_HOME_OUTPUT_X,
        DASHBOARD_HOME_OUTPUT_Y,
        DASHBOARD_HOME_OUTPUT_W,
        DASHBOARD_HOME_OUTPUT_H,
        mode_accent,
    )?;
    draw_dashboard_entry_marker(
        painter,
        DASHBOARD_HOME_THERMAL_X,
        DASHBOARD_HOME_THERMAL_Y,
        DASHBOARD_HOME_THERMAL_W,
        DASHBOARD_HOME_THERMAL_H,
        palette.center,
    )?;
    draw_dashboard_entry_marker(
        painter,
        DASHBOARD_HOME_CELLS_X,
        DASHBOARD_HOME_CELLS_Y,
        DASHBOARD_HOME_CELLS_W,
        DASHBOARD_HOME_CELLS_H,
        palette.left,
    )?;
    draw_dashboard_entry_marker(
        painter,
        DASHBOARD_HOME_CHARGER_X,
        DASHBOARD_HOME_CHARGER_Y,
        DASHBOARD_HOME_CHARGER_W,
        DASHBOARD_HOME_CHARGER_H,
        palette.right,
    )?;
    draw_dashboard_entry_marker(
        painter,
        DASHBOARD_HOME_BATTERY_FLOW_X,
        DASHBOARD_HOME_BATTERY_FLOW_Y,
        DASHBOARD_HOME_BATTERY_FLOW_W,
        DASHBOARD_HOME_BATTERY_FLOW_H,
        if data.mains_present {
            palette.accent
        } else {
            palette.down
        },
    )?;

    Ok(())
}

fn render_dashboard_detail_page<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    data: DashboardLiveData,
    page: DashboardDetailPage,
) -> Result<(), P::Error> {
    let accent = match page {
        DashboardDetailPage::Cells => palette.left,
        DashboardDetailPage::BatteryFlow => {
            if data.mains_present {
                palette.accent
            } else {
                palette.down
            }
        }
        DashboardDetailPage::Output => palette.accent,
        DashboardDetailPage::Charger => palette.right,
        DashboardDetailPage::Thermal => palette.center,
    };
    let status = detail_status_tag(page, data);

    draw_dashboard_detail_top_bar(
        painter,
        variant,
        palette,
        detail_page_title(page),
        status,
        detail_status_color(palette, status),
    )?;

    draw_panel(painter, 6, 22, 308, 38, palette, true, accent)?;
    draw_panel(painter, 6, 60, 150, 82, palette, false, accent)?;
    draw_panel(painter, 164, 60, 150, 82, palette, false, accent)?;
    draw_panel(
        painter,
        6,
        146,
        308,
        20,
        palette,
        true,
        detail_status_color(palette, status),
    )?;

    match page {
        DashboardDetailPage::Cells => {
            render_dashboard_cells_detail(painter, variant, palette, data)?
        }
        DashboardDetailPage::BatteryFlow => {
            render_dashboard_battery_flow_detail(painter, variant, palette, data)?
        }
        DashboardDetailPage::Output => {
            render_dashboard_output_detail(painter, variant, palette, data)?
        }
        DashboardDetailPage::Charger => {
            render_dashboard_charger_detail(painter, variant, palette, data)?
        }
        DashboardDetailPage::Thermal => {
            render_dashboard_thermal_detail(painter, variant, palette, data)?
        }
    }

    draw_dashboard_detail_footer_notice(painter, variant, palette, page, data)?;

    Ok(())
}

fn detail_balance_summary_text(detail: DashboardDetailSnapshot) -> &'static str {
    match detail.balance_mask {
        Some(0) => "NONE",
        Some(0b0001) => "C1",
        Some(0b0010) => "C2",
        Some(0b0100) => "C3",
        Some(0b1000) => "C4",
        Some(0b0011) => "C1+C2",
        Some(0b0101) => "C1+C3",
        Some(0b0110) => "C2+C3",
        Some(0b1001) => "C1+C4",
        Some(0b1010) => "C2+C4",
        Some(0b1100) => "C3+C4",
        Some(_) => "MULTI",
        None => match detail.balance_active {
            Some(true) => "ACTIVE",
            Some(false) => "NONE",
            None => "N/A",
        },
    }
}

fn render_dashboard_cells_detail<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    data: DashboardLiveData,
) -> Result<(), P::Error> {
    text(
        painter,
        variant,
        FontRole::DetailBody,
        "PACK",
        Point::new(14, 26),
        HorizontalAlignment::Left,
        palette.bg,
    )?;
    match (data.batt_pack_mv, data.bms_soc_pct) {
        (Some(pack_mv), Some(soc)) => text(
            painter,
            variant,
            FontRole::NumBig,
            format_args!(
                "{:>2}.{:01}V {:>2}%",
                pack_mv / 1000,
                (pack_mv % 1000) / 100,
                soc
            ),
            Point::new(308, 28),
            HorizontalAlignment::Right,
            palette.bg,
        )?,
        (Some(pack_mv), None) => text(
            painter,
            variant,
            FontRole::NumBig,
            format_args!("{:>2}.{:01}V N/A", pack_mv / 1000, (pack_mv % 1000) / 100),
            Point::new(308, 28),
            HorizontalAlignment::Right,
            palette.bg,
        )?,
        _ => text(
            painter,
            variant,
            FontRole::NumBig,
            "N/A",
            Point::new(308, 28),
            HorizontalAlignment::Right,
            palette.bg,
        )?,
    }
    text(
        painter,
        variant,
        FontRole::DetailBody,
        "BAL STATE",
        Point::new(14, 44),
        HorizontalAlignment::Left,
        palette.bg,
    )?;
    text(
        painter,
        variant,
        FontRole::DetailNum,
        detail_balance_summary_text(data.detail),
        Point::new(308, 42),
        HorizontalAlignment::Right,
        palette.bg,
    )?;

    text(
        painter,
        variant,
        FontRole::DetailBody,
        "CELL MV",
        Point::new(14, 64),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    draw_detail_row(
        painter,
        variant,
        palette,
        14,
        DETAIL_ROW_Y_1,
        "C1",
        data.detail.cell_mv[0],
        DetailValueFmt::MilliVolt,
    )?;
    draw_detail_row(
        painter,
        variant,
        palette,
        14,
        DETAIL_ROW_Y_2,
        "C2",
        data.detail.cell_mv[1],
        DetailValueFmt::MilliVolt,
    )?;
    draw_detail_row(
        painter,
        variant,
        palette,
        14,
        DETAIL_ROW_Y_3,
        "C3",
        data.detail.cell_mv[2],
        DetailValueFmt::MilliVolt,
    )?;
    draw_detail_row(
        painter,
        variant,
        palette,
        14,
        DETAIL_ROW_Y_4,
        "C4",
        data.detail.cell_mv[3],
        DetailValueFmt::MilliVolt,
    )?;

    text(
        painter,
        variant,
        FontRole::DetailBody,
        "CELL TEMP",
        Point::new(172, 64),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    draw_detail_row(
        painter,
        variant,
        palette,
        172,
        DETAIL_ROW_Y_1,
        "T1",
        data.detail.cell_temp_c[0],
        DetailValueFmt::Celsius,
    )?;
    draw_detail_row(
        painter,
        variant,
        palette,
        172,
        DETAIL_ROW_Y_2,
        "T2",
        data.detail.cell_temp_c[1],
        DetailValueFmt::Celsius,
    )?;
    draw_detail_row(
        painter,
        variant,
        palette,
        172,
        DETAIL_ROW_Y_3,
        "T3",
        data.detail.cell_temp_c[2],
        DetailValueFmt::Celsius,
    )?;
    draw_detail_row(
        painter,
        variant,
        palette,
        172,
        DETAIL_ROW_Y_4,
        "T4",
        data.detail.cell_temp_c[3],
        DetailValueFmt::Celsius,
    )?;
    Ok(())
}

fn render_dashboard_battery_flow_detail<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    data: DashboardLiveData,
) -> Result<(), P::Error> {
    text(
        painter,
        variant,
        FontRole::DetailBody,
        "VPACK V",
        Point::new(14, 26),
        HorizontalAlignment::Left,
        palette.bg,
    )?;
    match data.batt_pack_mv {
        Some(pack_mv) => text(
            painter,
            variant,
            FontRole::NumHero,
            format_args!("{:>2}.{:01}", pack_mv / 1000, (pack_mv % 1000) / 100),
            Point::new(154, 30),
            HorizontalAlignment::Right,
            palette.bg,
        )?,
        None => text(
            painter,
            variant,
            FontRole::DetailNum,
            "N/A",
            Point::new(14, 38),
            HorizontalAlignment::Left,
            palette.bg,
        )?,
    }
    text(
        painter,
        variant,
        FontRole::DetailBody,
        "IPACK A",
        Point::new(174, 26),
        HorizontalAlignment::Left,
        palette.bg,
    )?;
    match data.bms_current_ma {
        Some(current_ma) => text(
            painter,
            variant,
            FontRole::NumHero,
            format_args!(
                "{:>1}.{:02}",
                current_ma.abs() / 1000,
                (current_ma.abs() % 1000) / 10
            ),
            Point::new(304, 30),
            HorizontalAlignment::Right,
            palette.bg,
        )?,
        None => text(
            painter,
            variant,
            FontRole::DetailNum,
            "N/A",
            Point::new(308, 38),
            HorizontalAlignment::Right,
            palette.bg,
        )?,
    }

    text(
        painter,
        variant,
        FontRole::DetailBody,
        "ENERGY",
        Point::new(14, 64),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    draw_detail_row(
        painter,
        variant,
        palette,
        14,
        DETAIL_ROW_Y_1,
        "STORE",
        data.detail.battery_energy_mwh,
        DetailValueFmt::MilliWattHour,
    )?;
    draw_detail_row(
        painter,
        variant,
        palette,
        14,
        DETAIL_ROW_Y_2,
        "FULL",
        data.detail.battery_full_capacity_mwh,
        DetailValueFmt::MilliWattHour,
    )?;
    draw_detail_text_row(
        painter,
        variant,
        palette,
        14,
        DETAIL_ROW_Y_3,
        "SOC",
        match data.bms_soc_pct {
            Some(_) => DetailTextValue::Percent(data.bms_soc_pct.unwrap_or(0)),
            None => DetailTextValue::Na,
        },
    )?;
    draw_detail_text_row(
        painter,
        variant,
        palette,
        14,
        DETAIL_ROW_Y_4,
        "STATE",
        match data.bms_current_ma {
            Some(ma) if ma > 0 => DetailTextValue::Static("CHG"),
            Some(ma) if ma < 0 => DetailTextValue::Static("DSG"),
            Some(_) => DetailTextValue::Static("IDLE"),
            None => DetailTextValue::Static("N/A"),
        },
    )?;

    text(
        painter,
        variant,
        FontRole::DetailBody,
        "GATE STATE",
        Point::new(172, 64),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    draw_detail_text_row(
        painter,
        variant,
        palette,
        172,
        DETAIL_ROW_Y_1,
        "CHG",
        bool_text_value(data.detail.charge_fet_on),
    )?;
    draw_detail_text_row(
        painter,
        variant,
        palette,
        172,
        DETAIL_ROW_Y_2,
        "DSG",
        bool_text_value(data.detail.discharge_fet_on),
    )?;
    draw_detail_text_row(
        painter,
        variant,
        palette,
        172,
        DETAIL_ROW_Y_3,
        "PCHG",
        bool_text_value(data.detail.precharge_fet_on),
    )?;
    draw_detail_text_row(
        painter,
        variant,
        palette,
        172,
        DETAIL_ROW_Y_4,
        "FAULT",
        DetailTextValue::Static(detail_fault_row_text(
            DashboardDetailPage::BatteryFlow,
            data,
        )),
    )?;
    Ok(())
}

fn render_dashboard_output_detail<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    data: DashboardLiveData,
) -> Result<(), P::Error> {
    text(
        painter,
        variant,
        FontRole::DetailBody,
        "VOUT V",
        Point::new(14, 26),
        HorizontalAlignment::Left,
        palette.bg,
    )?;
    match data.output_bus_mv() {
        Some(bus_mv) => text(
            painter,
            variant,
            FontRole::NumHero,
            format_args!("{:>2}.{:01}", bus_mv / 1000, (bus_mv % 1000) / 100),
            Point::new(154, 30),
            HorizontalAlignment::Right,
            palette.bg,
        )?,
        None => text(
            painter,
            variant,
            FontRole::DetailNum,
            "N/A",
            Point::new(14, 38),
            HorizontalAlignment::Left,
            palette.bg,
        )?,
    }
    text(
        painter,
        variant,
        FontRole::DetailBody,
        "POUT W",
        Point::new(174, 26),
        HorizontalAlignment::Left,
        palette.bg,
    )?;
    match data.output_power_w10() {
        Some(power_w10) => text(
            painter,
            variant,
            FontRole::NumHero,
            format_args!("{:>2}.{:01}", power_w10 / 10, power_w10 % 10),
            Point::new(304, 30),
            HorizontalAlignment::Right,
            palette.bg,
        )?,
        None => text(
            painter,
            variant,
            FontRole::DetailNum,
            "N/A",
            Point::new(308, 38),
            HorizontalAlignment::Right,
            palette.bg,
        )?,
    }

    text(
        painter,
        variant,
        FontRole::DetailBody,
        "OUT-A",
        Point::new(14, 64),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    draw_output_current_row(
        painter,
        variant,
        palette,
        14,
        DETAIL_ROW_Y_1,
        "I",
        data.out_a_on,
        data.out_a_ma,
    )?;
    draw_detail_row(
        painter,
        variant,
        palette,
        14,
        DETAIL_ROW_Y_2,
        "TEMP",
        data.detail.out_a_temp_c.or(data.therm_a_c),
        DetailValueFmt::Celsius,
    )?;
    draw_detail_text_row(
        painter,
        variant,
        palette,
        14,
        DETAIL_ROW_Y_3,
        "STATE",
        if data.output_recovery_pending(OutputSelector::OutA) {
            DetailTextValue::Static("RECOVER")
        } else if data.output_hold(OutputSelector::OutA) {
            DetailTextValue::Static("HOLD")
        } else if data.out_a_on {
            DetailTextValue::Static("RUN")
        } else {
            DetailTextValue::Static("OFF")
        },
    )?;
    draw_detail_text_row(
        painter,
        variant,
        palette,
        14,
        DETAIL_ROW_Y_4,
        "FAULT",
        DetailTextValue::Static(output_fault_row_text(
            data.tps_a_state,
            data.out_a_on,
            data.output_hold(OutputSelector::OutA),
            data.output_recovery_pending(OutputSelector::OutA),
            "HOLD",
        )),
    )?;

    text(
        painter,
        variant,
        FontRole::DetailBody,
        "OUT-B",
        Point::new(172, 64),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    draw_output_current_row(
        painter,
        variant,
        palette,
        172,
        DETAIL_ROW_Y_1,
        "I",
        data.out_b_on,
        data.out_b_ma,
    )?;
    draw_detail_row(
        painter,
        variant,
        palette,
        172,
        DETAIL_ROW_Y_2,
        "TEMP",
        data.detail.out_b_temp_c.or(data.therm_b_c),
        DetailValueFmt::Celsius,
    )?;
    draw_detail_text_row(
        painter,
        variant,
        palette,
        172,
        DETAIL_ROW_Y_3,
        "STATE",
        if data.output_recovery_pending(OutputSelector::OutB) {
            DetailTextValue::Static("RECOVER")
        } else if data.output_hold(OutputSelector::OutB) {
            DetailTextValue::Static("HOLD")
        } else if data.out_b_on {
            DetailTextValue::Static("RUN")
        } else {
            DetailTextValue::Static("OFF")
        },
    )?;
    draw_detail_text_row(
        painter,
        variant,
        palette,
        172,
        DETAIL_ROW_Y_4,
        "FAULT",
        DetailTextValue::Static(output_fault_row_text(
            data.tps_b_state,
            data.out_b_on,
            data.output_hold(OutputSelector::OutB),
            data.output_recovery_pending(OutputSelector::OutB),
            "STBY",
        )),
    )?;
    Ok(())
}

fn render_dashboard_charger_detail<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    data: DashboardLiveData,
) -> Result<(), P::Error> {
    text(
        painter,
        variant,
        FontRole::DetailBody,
        "IN W",
        Point::new(14, 24),
        HorizontalAlignment::Left,
        palette.bg,
    )?;
    text(
        painter,
        variant,
        FontRole::DetailBody,
        "CHARGE W",
        Point::new(174, 24),
        HorizontalAlignment::Left,
        palette.bg,
    )?;
    draw_charger_source_indicator(
        painter,
        variant,
        palette,
        data.detail.input_source,
        14,
        42,
        26,
        12,
    )?;
    draw_icon_blocks_centered(
        painter,
        174,
        42,
        26,
        12,
        if data.battery_charge_power_w10().unwrap_or(0) > 0 {
            RI_BATTERY_CHARGE_LINE_24
        } else {
            RI_BATTERY_LINE_24
        },
        palette.bg,
    )?;
    match data.input_power_w10() {
        Some(pin_w10) => text(
            painter,
            variant,
            FontRole::NumHero,
            format_args!("{:>2}.{:01}", pin_w10 / 10, pin_w10 % 10),
            Point::new(154, 34),
            HorizontalAlignment::Right,
            palette.bg,
        )?,
        None => text(
            painter,
            variant,
            FontRole::DetailNum,
            "N/A",
            Point::new(154, 38),
            HorizontalAlignment::Right,
            palette.bg,
        )?,
    }
    match data.battery_charge_power_w10() {
        Some(pack_w10) => text(
            painter,
            variant,
            FontRole::NumHero,
            format_args!("{:>2}.{:01}", pack_w10 / 10, pack_w10 % 10),
            Point::new(304, 34),
            HorizontalAlignment::Right,
            palette.bg,
        )?,
        None => text(
            painter,
            variant,
            FontRole::DetailNum,
            "N/A",
            Point::new(308, 38),
            HorizontalAlignment::Right,
            palette.bg,
        )?,
    }
    text(
        painter,
        variant,
        FontRole::DetailBody,
        "SESSION",
        Point::new(14, 64),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    draw_detail_text_row(
        painter,
        variant,
        palette,
        14,
        DETAIL_ROW_Y_1,
        "ACTIVE",
        bool_text_value(charger_active_value(data)),
    )?;
    draw_detail_text_row(
        painter,
        variant,
        palette,
        14,
        DETAIL_ROW_Y_2,
        "STATE",
        DetailTextValue::Static(charger_state_text(data)),
    )?;
    draw_detail_row(
        painter,
        variant,
        palette,
        14,
        DETAIL_ROW_Y_3,
        "ICHG",
        data.chg_iin_ma,
        DetailValueFmt::MilliAmp,
    )?;
    draw_detail_text_row(
        painter,
        variant,
        palette,
        14,
        DETAIL_ROW_Y_4,
        "INPUT",
        DetailTextValue::Static(if data.mains_present {
            "PRESENT"
        } else {
            "ABSENT"
        }),
    )?;

    text(
        painter,
        variant,
        FontRole::DetailBody,
        "PACK SIDE",
        Point::new(172, 64),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    draw_detail_row(
        painter,
        variant,
        palette,
        172,
        DETAIL_ROW_Y_1,
        "VPACK",
        data.batt_pack_mv,
        DetailValueFmt::MilliVolt,
    )?;
    draw_detail_text_row(
        painter,
        variant,
        palette,
        172,
        DETAIL_ROW_Y_2,
        "BMS",
        DetailTextValue::Static(if data.bms_recovery_pending {
            "RECOVER"
        } else if data.bms_discharge_ready == Some(false) {
            "LIMIT"
        } else {
            match data.bms_state {
                SelfCheckCommState::Ok => "READY",
                SelfCheckCommState::Warn => "WARN",
                SelfCheckCommState::Err => "FAULT",
                SelfCheckCommState::Pending => "PEND",
                SelfCheckCommState::NotAvailable => "N/A",
            }
        }),
    )?;
    draw_detail_text_row(
        painter,
        variant,
        palette,
        172,
        DETAIL_ROW_Y_3,
        "CHG",
        bool_text_value(data.charge_allowed),
    )?;
    draw_detail_text_row(
        painter,
        variant,
        palette,
        172,
        DETAIL_ROW_Y_4,
        "FAULT",
        DetailTextValue::Static(detail_fault_row_text(DashboardDetailPage::Charger, data)),
    )?;
    Ok(())
}

fn render_dashboard_thermal_detail<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    data: DashboardLiveData,
) -> Result<(), P::Error> {
    let hotspot_c = thermal_hotspot_c(data);
    let fan_icon_color = thermal_fan_icon_color(palette, data);

    text(
        painter,
        variant,
        FontRole::DetailBody,
        "HOTSPOT C",
        Point::new(14, 26),
        HorizontalAlignment::Left,
        palette.bg,
    )?;
    match hotspot_c {
        Some(temp_c) if temp_c >= 0 => {
            text(
                painter,
                variant,
                FontRole::NumHero,
                format_args!("{}", temp_c),
                Point::new(154, 30),
                HorizontalAlignment::Right,
                palette.bg,
            )?;
        }
        Some(temp_c) => text(
            painter,
            variant,
            FontRole::DetailNum,
            format_args!("{temp_c}C"),
            Point::new(174, 30),
            HorizontalAlignment::Right,
            palette.bg,
        )?,
        None => text(
            painter,
            variant,
            FontRole::DetailNum,
            "N/A",
            Point::new(174, 30),
            HorizontalAlignment::Right,
            palette.bg,
        )?,
    }
    text(
        painter,
        variant,
        FontRole::DetailBody,
        "FAN",
        Point::new(174, 26),
        HorizontalAlignment::Left,
        palette.bg,
    )?;
    draw_icon_blocks_centered(
        painter,
        174,
        28,
        130,
        20,
        thermal_fan_blocks(thermal_fan_frame(
            data.frame_no,
            data.detail.fan_rpm,
            data.detail.fan_pwm_pct,
            data.detail.fan_status,
        )),
        fan_icon_color,
    )?;
    text(
        painter,
        variant,
        FontRole::DetailBody,
        "SENSORS",
        Point::new(14, 64),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    draw_detail_row(
        painter,
        variant,
        palette,
        14,
        DETAIL_ROW_Y_1,
        "TMP-A",
        data.therm_a_c,
        DetailValueFmt::Celsius,
    )?;
    draw_detail_row(
        painter,
        variant,
        palette,
        14,
        DETAIL_ROW_Y_2,
        "TMP-B",
        data.therm_b_c,
        DetailValueFmt::Celsius,
    )?;
    draw_detail_row(
        painter,
        variant,
        palette,
        14,
        DETAIL_ROW_Y_3,
        "BOARD",
        data.detail.board_temp_c,
        DetailValueFmt::Celsius,
    )?;
    draw_detail_row(
        painter,
        variant,
        palette,
        14,
        DETAIL_ROW_Y_4,
        "BAT",
        data.detail.battery_temp_c,
        DetailValueFmt::Celsius,
    )?;

    text(
        painter,
        variant,
        FontRole::DetailBody,
        "FAN CTRL",
        Point::new(172, 64),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    draw_detail_row(
        painter,
        variant,
        palette,
        172,
        DETAIL_ROW_Y_1,
        "RPM",
        data.detail.fan_rpm,
        DetailValueFmt::Rpm,
    )?;
    draw_detail_row(
        painter,
        variant,
        palette,
        172,
        DETAIL_ROW_Y_2,
        "PWM",
        data.detail.fan_pwm_pct,
        DetailValueFmt::Percent,
    )?;
    draw_detail_text_row(
        painter,
        variant,
        palette,
        172,
        DETAIL_ROW_Y_3,
        "MODE",
        DetailTextValue::Static(data.detail.fan_status.unwrap_or("N/A")),
    )?;
    draw_detail_text_row(
        painter,
        variant,
        palette,
        172,
        DETAIL_ROW_Y_4,
        "FAULT",
        DetailTextValue::Static(detail_fault_row_text(DashboardDetailPage::Thermal, data)),
    )?;
    Ok(())
}

fn detail_page_title(page: DashboardDetailPage) -> &'static str {
    match page {
        DashboardDetailPage::Cells => "CELL DETAIL",
        DashboardDetailPage::BatteryFlow => "BATTERY FLOW",
        DashboardDetailPage::Output => "OUTPUT DETAIL",
        DashboardDetailPage::Charger => "CHARGER DETAIL",
        DashboardDetailPage::Thermal => "THERMAL DETAIL",
    }
}

fn detail_status_tag(page: DashboardDetailPage, data: DashboardLiveData) -> &'static str {
    match page {
        DashboardDetailPage::Cells => {
            if data.bms_state == SelfCheckCommState::Err {
                "FAULT"
            } else if data.bms_recovery_pending || data.bms_discharge_ready == Some(false) {
                "LIMIT"
            } else if data.bms_state == SelfCheckCommState::Warn || data.bms_rca_alarm == Some(true)
            {
                "WARN"
            } else if !cells_detail_ready(data) {
                "N/A"
            } else if data.detail.balance_active == Some(true) {
                "BAL ON"
            } else {
                "READY"
            }
        }
        DashboardDetailPage::BatteryFlow => {
            if data.bms_state == SelfCheckCommState::Err {
                "FAULT"
            } else if data.bms_recovery_pending || data.bms_discharge_ready == Some(false) {
                "LIMIT"
            } else if data.bms_state == SelfCheckCommState::Warn || data.bms_rca_alarm == Some(true)
            {
                "WARN"
            } else if !battery_flow_detail_ready(data) {
                "N/A"
            } else {
                match data.bms_current_ma {
                    Some(ma) if ma > 0 => "CHG",
                    Some(ma) if ma < 0 => "DSG",
                    Some(_) => "IDLE",
                    None => "N/A",
                }
            }
        }
        DashboardDetailPage::Output => {
            if data.output_hold(OutputSelector::OutA) || data.output_hold(OutputSelector::OutB) {
                if data.bms_recovery_pending {
                    "RECOV"
                } else {
                    "HOLD"
                }
            } else if data.tps_a_state == SelfCheckCommState::Err
                || data.tps_b_state == SelfCheckCommState::Err
            {
                "FAULT"
            } else if data.tps_a_state == SelfCheckCommState::Warn
                || data.tps_b_state == SelfCheckCommState::Warn
            {
                "WARN"
            } else if !output_detail_ready(data) {
                "N/A"
            } else if !data.out_a_on && !data.out_b_on {
                "IDLE"
            } else if !data.out_a_on || !data.out_b_on {
                "WARN"
            } else {
                "REG OK"
            }
        }
        DashboardDetailPage::Charger => {
            if data.charger_state == SelfCheckCommState::Err {
                "FAULT"
            } else if data.charger_state == SelfCheckCommState::Warn {
                "WARN"
            } else if charger_active_value(data) == Some(true) {
                "ACTIVE"
            } else if !data.mains_present {
                "NOAC"
            } else if data.charge_allowed == Some(false) {
                "IDLE"
            } else if charger_data_ready(data) {
                "IDLE"
            } else {
                "N/A"
            }
        }
        DashboardDetailPage::Thermal => {
            if thermal_fault_present(data) {
                "FAULT"
            } else if thermal_warn_present(data) {
                "WARN"
            } else {
                match thermal_hotspot_c(data) {
                    Some(temp) if temp >= 60 => "HOT",
                    Some(temp) if temp >= 45 => "WARM",
                    Some(_) => "COOL",
                    None => "N/A",
                }
            }
        }
    }
}

fn detail_status_color(palette: Palette, status: &'static str) -> u16 {
    match status {
        "FAULT" | "HOT" => ERROR_COLOR,
        "WARN" | "WARM" | "LOCK" | "NOAC" => PROGRESS_COLOR,
        _ => palette.accent,
    }
}

fn draw_dashboard_detail_top_bar<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    title: &'static str,
    status: &'static str,
    status_color: u16,
) -> Result<(), P::Error> {
    fill(painter, 0, 0, UI_W, HEADER_H, palette.panel)?;
    draw_panel(
        painter,
        DASHBOARD_DETAIL_BACK_X,
        DASHBOARD_DETAIL_BACK_Y,
        DASHBOARD_DETAIL_BACK_W,
        DASHBOARD_DETAIL_BACK_H,
        palette,
        false,
        palette.accent,
    )?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        "BACK",
        Point::new(
            (DASHBOARD_DETAIL_BACK_X + DASHBOARD_DETAIL_BACK_W / 2) as i32,
            4,
        ),
        HorizontalAlignment::Center,
        palette.text,
    )?;
    text(
        painter,
        variant,
        FontRole::DetailTitle,
        title,
        Point::new(DETAIL_TITLE_X, 2),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    text(
        painter,
        variant,
        FontRole::DetailBody,
        status,
        Point::new(DETAIL_STATUS_X, 2),
        HorizontalAlignment::Right,
        status_color,
    )
}

fn draw_dashboard_entry_marker<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    accent: u16,
) -> Result<(), P::Error> {
    let marker_x = x + w - 11;
    let marker_y = y + h - 11;
    fill(painter, marker_x, marker_y + 6, 8, 2, accent)?;
    fill(painter, marker_x + 6, marker_y, 2, 8, accent)
}

enum DetailValueFmt {
    MilliVolt,
    MilliAmp,
    MilliWattHour,
    Celsius,
    Percent,
    Rpm,
}

enum DetailTextValue {
    Static(&'static str),
    Percent(u16),
    Na,
}

fn draw_detail_row<P: UiPainter, T: Copy + IntoDetailValue>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    x: u16,
    y: u16,
    label: &'static str,
    value: Option<T>,
    fmt: DetailValueFmt,
) -> Result<(), P::Error> {
    text(
        painter,
        variant,
        FontRole::TextBody,
        label,
        Point::new(x as i32, y as i32),
        HorizontalAlignment::Left,
        palette.text_dim,
    )?;
    match (value.map(IntoDetailValue::into_detail_value), fmt) {
        (Some(DetailValue::U16(raw)), DetailValueFmt::MilliVolt) => text(
            painter,
            variant,
            FontRole::Num,
            format_args!("{:>2}.{:03}V", raw / 1000, raw % 1000),
            Point::new((x + 132) as i32, y as i32),
            HorizontalAlignment::Right,
            palette.text,
        ),
        (Some(DetailValue::U16(raw)), DetailValueFmt::MilliAmp) => text(
            painter,
            variant,
            FontRole::Num,
            format_args!("{:>1}.{:02}A", raw / 1000, (raw % 1000) / 10),
            Point::new((x + 132) as i32, y as i32),
            HorizontalAlignment::Right,
            palette.text,
        ),
        (Some(DetailValue::U32(raw)), DetailValueFmt::MilliWattHour) => text(
            painter,
            variant,
            FontRole::Num,
            format_args!("{:>5}mWh", raw),
            Point::new((x + 132) as i32, y as i32),
            HorizontalAlignment::Right,
            palette.text,
        ),
        (Some(DetailValue::I16(raw)), DetailValueFmt::Celsius) => text(
            painter,
            variant,
            FontRole::Num,
            format_args!("{:>2}C", raw),
            Point::new((x + 132) as i32, y as i32),
            HorizontalAlignment::Right,
            palette.text,
        ),
        (Some(DetailValue::U8(raw)), DetailValueFmt::Percent) => text(
            painter,
            variant,
            FontRole::Num,
            format_args!("{:>3}%", raw),
            Point::new((x + 132) as i32, y as i32),
            HorizontalAlignment::Right,
            palette.text,
        ),
        (Some(DetailValue::U16(raw)), DetailValueFmt::Rpm) => text(
            painter,
            variant,
            FontRole::Num,
            format_args!("{:>4}RPM", raw),
            Point::new((x + 132) as i32, y as i32),
            HorizontalAlignment::Right,
            palette.text,
        ),
        _ => text(
            painter,
            variant,
            FontRole::Num,
            "N/A",
            Point::new((x + 132) as i32, y as i32),
            HorizontalAlignment::Right,
            palette.text,
        ),
    }
}

fn draw_detail_text_row<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    x: u16,
    y: u16,
    label: &'static str,
    value: DetailTextValue,
) -> Result<(), P::Error> {
    text(
        painter,
        variant,
        FontRole::TextBody,
        label,
        Point::new(x as i32, y as i32),
        HorizontalAlignment::Left,
        palette.text_dim,
    )?;
    match value {
        DetailTextValue::Static(value) => text(
            painter,
            variant,
            FontRole::Num,
            value,
            Point::new((x + 132) as i32, y as i32),
            HorizontalAlignment::Right,
            palette.text,
        ),
        DetailTextValue::Percent(value) => text(
            painter,
            variant,
            FontRole::Num,
            format_args!("{:>3}%", value),
            Point::new((x + 132) as i32, y as i32),
            HorizontalAlignment::Right,
            palette.text,
        ),
        DetailTextValue::Na => text(
            painter,
            variant,
            FontRole::Num,
            "N/A",
            Point::new((x + 132) as i32, y as i32),
            HorizontalAlignment::Right,
            palette.text,
        ),
    }
}

fn draw_output_current_row<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    x: u16,
    y: u16,
    label: &'static str,
    enabled: bool,
    current_ma: Option<i32>,
) -> Result<(), P::Error> {
    text(
        painter,
        variant,
        FontRole::TextBody,
        label,
        Point::new(x as i32, y as i32),
        HorizontalAlignment::Left,
        palette.text_dim,
    )?;
    if !enabled {
        return text(
            painter,
            variant,
            FontRole::Num,
            "--",
            Point::new((x + 132) as i32, y as i32),
            HorizontalAlignment::Right,
            palette.text,
        );
    }
    match current_ma {
        Some(current_ma) if current_ma >= 0 => text(
            painter,
            variant,
            FontRole::Num,
            format_args!(
                "{:>1}.{:02}A",
                (current_ma as u32) / 1000,
                ((current_ma as u32) % 1000) / 10
            ),
            Point::new((x + 132) as i32, y as i32),
            HorizontalAlignment::Right,
            palette.text,
        ),
        Some(_) => text(
            painter,
            variant,
            FontRole::Num,
            "--",
            Point::new((x + 132) as i32, y as i32),
            HorizontalAlignment::Right,
            palette.text,
        ),
        None => text(
            painter,
            variant,
            FontRole::Num,
            "N/A",
            Point::new((x + 132) as i32, y as i32),
            HorizontalAlignment::Right,
            palette.text,
        ),
    }
}

fn bool_text_value(value: Option<bool>) -> DetailTextValue {
    match value {
        Some(true) => DetailTextValue::Static("ON"),
        Some(false) => DetailTextValue::Static("OFF"),
        None => DetailTextValue::Na,
    }
}

fn max_optional_i16(a: Option<i16>, b: Option<i16>) -> Option<i16> {
    match (a, b) {
        (Some(a), Some(b)) => Some(if a > b { a } else { b }),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn thermal_hotspot_c(data: DashboardLiveData) -> Option<i16> {
    max_optional_i16(
        data.therm_a_c,
        max_optional_i16(
            data.therm_b_c,
            max_optional_i16(data.detail.board_temp_c, data.detail.battery_temp_c),
        ),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ThermalFanMotion {
    Off,
    Low,
    Mid,
    High,
}

fn thermal_fan_motion(
    rpm: Option<u16>,
    pwm_pct: Option<u8>,
    status: Option<&'static str>,
) -> ThermalFanMotion {
    match (rpm, pwm_pct, status) {
        (Some(rpm), _, _) if rpm >= 3_600 => ThermalFanMotion::High,
        (_, Some(pwm), _) if pwm >= 90 => ThermalFanMotion::High,
        (_, _, Some("HIGH")) => ThermalFanMotion::High,
        (Some(rpm), _, _) if rpm >= 1_800 => ThermalFanMotion::Mid,
        (_, Some(pwm), _) if pwm >= 55 => ThermalFanMotion::Mid,
        (_, _, Some("MID")) => ThermalFanMotion::Mid,
        (Some(rpm), _, _) if rpm > 0 => ThermalFanMotion::Low,
        (_, Some(pwm), _) if pwm > 0 => ThermalFanMotion::Low,
        (_, _, Some("LOW" | "RUN")) => ThermalFanMotion::Low,
        _ => ThermalFanMotion::Off,
    }
}

fn thermal_fan_frame(
    frame_no: u32,
    rpm: Option<u16>,
    pwm_pct: Option<u8>,
    status: Option<&'static str>,
) -> usize {
    match thermal_fan_motion(rpm, pwm_pct, status) {
        ThermalFanMotion::Off => 0,
        ThermalFanMotion::Low => ((frame_no / 18) % 2) as usize,
        ThermalFanMotion::Mid => ((frame_no / 10) % 2) as usize,
        ThermalFanMotion::High => ((frame_no / 5) % 2) as usize,
    }
}

fn thermal_fan_icon_color(palette: Palette, data: DashboardLiveData) -> u16 {
    if data.detail.fan_status == Some("FAULT") {
        ERROR_COLOR
    } else {
        match thermal_fan_motion(
            data.detail.fan_rpm,
            data.detail.fan_pwm_pct,
            data.detail.fan_status,
        ) {
            ThermalFanMotion::Off => fade_color(palette.bg, palette.panel_alt),
            _ => palette.bg,
        }
    }
}

fn cells_detail_ready(data: DashboardLiveData) -> bool {
    data.batt_pack_mv.is_some()
        || data.detail.balance_active.is_some()
        || data.detail.balance_mask.is_some()
        || data.detail.balance_cell.is_some()
        || data.detail.cell_mv.iter().any(|value| value.is_some())
        || data.detail.cell_temp_c.iter().any(|value| value.is_some())
}

fn battery_flow_detail_ready(data: DashboardLiveData) -> bool {
    data.batt_pack_mv.is_some()
        || data.bms_current_ma.is_some()
        || data.bms_soc_pct.is_some()
        || data.detail.battery_energy_mwh.is_some()
        || data.detail.battery_full_capacity_mwh.is_some()
        || data.detail.charge_fet_on.is_some()
        || data.detail.discharge_fet_on.is_some()
        || data.detail.precharge_fet_on.is_some()
}

fn output_detail_ready(data: DashboardLiveData) -> bool {
    data.out_a_mv.is_some()
        || data.out_a_ma.is_some()
        || data.out_b_mv.is_some()
        || data.out_b_ma.is_some()
        || data.detail.out_a_temp_c.is_some()
        || data.detail.out_b_temp_c.is_some()
        || data.tps_a_state != SelfCheckCommState::Pending
        || data.tps_b_state != SelfCheckCommState::Pending
}

fn charger_data_ready(data: DashboardLiveData) -> bool {
    data.detail.charger_active.is_some()
        || data.detail.charger_status.is_some()
        || data.charge_allowed.is_some()
        || data.chg_iin_ma.is_some()
        || matches!(
            data.charger_state,
            SelfCheckCommState::Err | SelfCheckCommState::Warn | SelfCheckCommState::Ok
        )
}

fn charger_active_value(data: DashboardLiveData) -> Option<bool> {
    data.detail
        .charger_active
        .or(match (data.charge_allowed, data.chg_iin_ma) {
            (Some(true), Some(ma)) => Some(ma > 0),
            (Some(false), _) => Some(false),
            _ => None,
        })
}

fn charger_state_text(data: DashboardLiveData) -> &'static str {
    if let Some(status) = data.detail.charger_status {
        status
    } else if data.charger_state == SelfCheckCommState::Err {
        "FAULT"
    } else if data.charge_allowed == Some(false) && data.mains_present {
        "IDLE"
    } else if !data.mains_present {
        "NOAC"
    } else {
        match (data.charge_allowed, data.chg_iin_ma) {
            (Some(true), Some(ma)) if ma > 0 => "CHG",
            (Some(true), Some(_)) => "READY",
            _ => "N/A",
        }
    }
}

fn thermal_fault_present(data: DashboardLiveData) -> bool {
    data.therm_a_state == SelfCheckCommState::Err
        || data.therm_b_state == SelfCheckCommState::Err
        || data.detail.fan_status == Some("FAULT")
}

fn thermal_warn_present(data: DashboardLiveData) -> bool {
    data.therm_a_state == SelfCheckCommState::Warn || data.therm_b_state == SelfCheckCommState::Warn
}

fn detail_footer_notice(page: DashboardDetailPage, data: DashboardLiveData) -> &'static str {
    if !detail_data_ready(page, data) {
        return data.page_notice(page);
    }

    match detail_status_tag(page, data) {
        "FAULT" => detail_fault_notice(page, data),
        "WARN" => "WARNING ACTIVE - CHECK DETAIL ROWS",
        "LIMIT" => "UPSTREAM PATH LIMITED - CHECK MODULE STATUS",
        "HOLD" => "OUTPUT WAITING FOR BMS DISCHARGE PERMISSION",
        "RECOV" => "RECOVERY IN PROGRESS - HOLD OUTPUTS",
        _ => data.page_notice(page),
    }
}

fn detail_fault_notice(page: DashboardDetailPage, data: DashboardLiveData) -> &'static str {
    match page {
        DashboardDetailPage::BatteryFlow => {
            if data.bms_state == SelfCheckCommState::Err {
                "BMS LINK FAULT"
            } else if data.bms_recovery_pending {
                "BMS RECOVERY IN PROGRESS"
            } else if data.bms_discharge_ready == Some(false) {
                "DISCHARGE PATH LIMITED"
            } else if data.bms_rca_alarm == Some(true) {
                "PACK ALARM ACTIVE"
            } else if !battery_flow_detail_ready(data) {
                "N/A"
            } else {
                data.page_notice(page)
            }
        }
        DashboardDetailPage::Charger => {
            if data.charger_state == SelfCheckCommState::Err {
                "CHARGER LINK FAULT"
            } else if !charger_data_ready(data) {
                "N/A"
            } else {
                data.page_notice(page)
            }
        }
        DashboardDetailPage::Output => {
            if data.output_hold(OutputSelector::OutA) || data.output_hold(OutputSelector::OutB) {
                if data.bms_recovery_pending {
                    "OUTPUT WAITING FOR BMS RECOVERY"
                } else {
                    "OUTPUT HELD BY BMS DISCHARGE POLICY"
                }
            } else if data.tps_a_state == SelfCheckCommState::Err
                || data.tps_b_state == SelfCheckCommState::Err
            {
                "TPS LINK FAULT"
            } else if data.tps_a_state == SelfCheckCommState::Warn
                || data.tps_b_state == SelfCheckCommState::Warn
            {
                "TPS PROTECTION ACTIVE"
            } else {
                data.page_notice(page)
            }
        }
        DashboardDetailPage::Thermal => {
            if thermal_fault_present(data) {
                "THERMAL SENSE FAULT"
            } else if thermal_hotspot_c(data).is_some() {
                data.page_notice(page)
            } else {
                "N/A"
            }
        }
        _ => data.page_notice(page),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DetailFooterIcon {
    Live,
    Mock,
    Warn,
    Fault,
    Unknown,
}

fn detail_footer_badge(
    page: DashboardDetailPage,
    data: DashboardLiveData,
) -> (DetailFooterIcon, &'static str) {
    let notice = detail_footer_notice(page, data);
    let status = detail_status_tag(page, data);

    if !detail_data_ready(page, data) || status == "N/A" || notice == "N/A" {
        return (DetailFooterIcon::Unknown, "NO DATA");
    }

    if notice.contains("MOCK") {
        return (DetailFooterIcon::Mock, "MOCK DATA");
    }

    if notice.contains("BALANCE") {
        return (DetailFooterIcon::Live, "BAL ACTIVE");
    }

    match status {
        "FAULT" => (
            DetailFooterIcon::Fault,
            match page {
                DashboardDetailPage::BatteryFlow if data.bms_state == SelfCheckCommState::Err => {
                    "BMS FAULT"
                }
                DashboardDetailPage::BatteryFlow => "PACK ALARM",
                DashboardDetailPage::Charger => "LINK FAULT",
                DashboardDetailPage::Thermal => "SENSE FAULT",
                _ => "FAULT",
            },
        ),
        "WARN" | "HOT" | "WARM" | "LOCK" | "NOAC" | "LIMIT" | "HOLD" | "RECOV" => {
            (DetailFooterIcon::Warn, "CHECK ROWS")
        }
        _ if notice.contains("PENDING")
            || notice.contains("SOURCE")
            || notice.contains("UI ONLY") =>
        {
            (DetailFooterIcon::Unknown, "SOURCE NXT")
        }
        _ => (DetailFooterIcon::Live, "LIVE DATA"),
    }
}

fn draw_dashboard_detail_footer_notice<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    page: DashboardDetailPage,
    data: DashboardLiveData,
) -> Result<(), P::Error> {
    let (icon, label) = detail_footer_badge(page, data);
    fill(painter, 34, 148, 1, 16, palette.border)?;
    draw_detail_footer_icon(painter, 12, 147, icon, palette.bg)?;
    text(
        painter,
        variant,
        FontRole::DetailBody,
        label,
        Point::new(40, 150),
        HorizontalAlignment::Left,
        palette.bg,
    )
}

fn draw_detail_footer_icon<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    icon: DetailFooterIcon,
    rgb565: u16,
) -> Result<(), P::Error> {
    let blocks = match icon {
        DetailFooterIcon::Live => CARBON_CHECKMARK_OUTLINE_18,
        DetailFooterIcon::Mock => CARBON_CHECKBOX_INDETERMINATE_18,
        DetailFooterIcon::Warn => CARBON_WARNING_ALT_18,
        DetailFooterIcon::Fault => CARBON_ERROR_OUTLINE_18,
        DetailFooterIcon::Unknown => CARBON_HELP_18,
    };
    draw_icon_blocks(painter, x, y, blocks, rgb565)
}

fn detail_fault_row_text(page: DashboardDetailPage, data: DashboardLiveData) -> &'static str {
    match page {
        DashboardDetailPage::BatteryFlow => {
            if data.bms_state == SelfCheckCommState::Err {
                "LINK"
            } else if data.bms_recovery_pending || data.bms_discharge_ready == Some(false) {
                "LIMIT"
            } else if data.bms_state == SelfCheckCommState::Warn {
                "WARN"
            } else if data.bms_rca_alarm == Some(true) {
                "ALARM"
            } else if !battery_flow_detail_ready(data) {
                "N/A"
            } else {
                "CLEAR"
            }
        }
        DashboardDetailPage::Charger => {
            if data.charger_state == SelfCheckCommState::Err {
                "LINK"
            } else if data.charger_state == SelfCheckCommState::Warn {
                "WARN"
            } else if !charger_data_ready(data) {
                "N/A"
            } else {
                "CLEAR"
            }
        }
        DashboardDetailPage::Thermal => {
            if thermal_fault_present(data) {
                "SENSE"
            } else if thermal_warn_present(data) {
                "WARN"
            } else if thermal_hotspot_c(data).is_some() {
                "CLEAR"
            } else {
                "N/A"
            }
        }
        _ => "CLEAR",
    }
}

fn output_fault_row_text(
    state: SelfCheckCommState,
    enabled: bool,
    hold: bool,
    recovering: bool,
    off_text: &'static str,
) -> &'static str {
    if recovering {
        "RECOV"
    } else if hold {
        "HOLD"
    } else if state == SelfCheckCommState::Err {
        "FAULT"
    } else if state == SelfCheckCommState::Warn {
        "WARN"
    } else if state == SelfCheckCommState::Pending {
        "N/A"
    } else if enabled {
        "CLEAR"
    } else {
        off_text
    }
}

fn detail_data_ready(page: DashboardDetailPage, data: DashboardLiveData) -> bool {
    match page {
        DashboardDetailPage::Cells => cells_detail_ready(data),
        DashboardDetailPage::BatteryFlow => battery_flow_detail_ready(data),
        DashboardDetailPage::Output => output_detail_ready(data),
        DashboardDetailPage::Charger => charger_data_ready(data),
        DashboardDetailPage::Thermal => {
            thermal_fault_present(data)
                || thermal_hotspot_c(data).is_some()
                || data.detail.fan_rpm.is_some()
                || data.detail.fan_pwm_pct.is_some()
                || data.detail.fan_status.is_some()
        }
    }
}

enum DetailValue {
    U8(u8),
    U16(u16),
    U32(u32),
    I16(i16),
}

trait IntoDetailValue {
    fn into_detail_value(self) -> DetailValue;
}

impl IntoDetailValue for u8 {
    fn into_detail_value(self) -> DetailValue {
        DetailValue::U8(self)
    }
}

impl IntoDetailValue for u16 {
    fn into_detail_value(self) -> DetailValue {
        DetailValue::U16(self)
    }
}

impl IntoDetailValue for u32 {
    fn into_detail_value(self) -> DetailValue {
        DetailValue::U32(self)
    }
}

impl IntoDetailValue for i16 {
    fn into_detail_value(self) -> DetailValue {
        DetailValue::I16(self)
    }
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
    let chg_key = if snapshot.bq25792 == SelfCheckCommState::Ok
        && snapshot.bq25792_allow_charge == Some(false)
        && snapshot.fusb302_vbus_present == Some(true)
    {
        format_args!("INPUT ONLY")
    } else if snapshot.bq25792_allow_charge == Some(false) {
        format_args!("CHG IDLE")
    } else if ichg_has {
        format_args!("ICHG {:>1}.{:02}A", ichg_whole, ichg_frac)
    } else {
        format_args!("ICHG N/A")
    };
    let bms_key = if snapshot.bq40z50_recovery_pending {
        format_args!("AUTH ACTIVE")
    } else if snapshot.bq40z50 == SelfCheckCommState::Err {
        format_args!("NOT DETECTED")
    } else if snapshot.bq40z50_no_battery == Some(true) {
        format_args!("NO BATTERY")
    } else if snapshot.bq40z50_discharge_ready == Some(false) {
        format_args!("DSG BLOCKED")
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
    let tps_a_status_state = snapshot_tps_state(&snapshot, OutputSelector::OutA);
    let tps_b_status_state = snapshot_tps_state(&snapshot, OutputSelector::OutB);

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
            status: charger_label(
                snapshot.bq25792,
                snapshot.bq25792_allow_charge,
                snapshot.fusb302_vbus_present,
            ),
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
            status: bms_label(&snapshot),
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
            status_state: tps_a_status_state,
            status: tps_label(
                &snapshot,
                OutputSelector::OutA,
                snapshot_tps_state(&snapshot, OutputSelector::OutA),
                snapshot_tps_enabled(&snapshot, OutputSelector::OutA),
            ),
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
            status_state: tps_b_status_state,
            status: tps_label(
                &snapshot,
                OutputSelector::OutB,
                snapshot_tps_state(&snapshot, OutputSelector::OutB),
                snapshot_tps_enabled(&snapshot, OutputSelector::OutB),
            ),
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

fn icon_block_bounds(blocks: &[(u8, u8, u8, u8)]) -> Option<(u8, u8, u8, u8)> {
    let mut iter = blocks
        .iter()
        .copied()
        .filter(|&(_, _, bw, bh)| bw != 0 && bh != 0);
    let (mut min_x, mut min_y, mut max_x, mut max_y) = match iter.next() {
        Some((bx, by, bw, bh)) => (bx, by, bx + bw, by + bh),
        None => return None,
    };

    for (bx, by, bw, bh) in iter {
        min_x = min_x.min(bx);
        min_y = min_y.min(by);
        max_x = max_x.max(bx + bw);
        max_y = max_y.max(by + bh);
    }

    Some((min_x, min_y, max_x - min_x, max_y - min_y))
}

fn draw_icon_blocks_centered<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    box_w: u16,
    box_h: u16,
    blocks: &[(u8, u8, u8, u8)],
    rgb565: u16,
) -> Result<(), P::Error> {
    let Some((min_x, min_y, icon_w, icon_h)) = icon_block_bounds(blocks) else {
        return Ok(());
    };

    let origin_x = i32::from(x) + ((i32::from(box_w) - i32::from(icon_w)) / 2) - i32::from(min_x);
    let origin_y = i32::from(y) + ((i32::from(box_h) - i32::from(icon_h)) / 2) - i32::from(min_y);

    for &(bx, by, bw, bh) in blocks {
        if bw == 0 || bh == 0 {
            continue;
        }
        fill(
            painter,
            (origin_x + i32::from(bx)) as u16,
            (origin_y + i32::from(by)) as u16,
            u16::from(bw),
            u16::from(bh),
            rgb565,
        )?;
    }
    Ok(())
}

fn thermal_fan_blocks(frame: usize) -> &'static [(u8, u8, u8, u8)] {
    match frame % 2 {
        0 => CARBON_FAN_OUTLINE_CARDINAL_24,
        _ => CARBON_FAN_OUTLINE_DIAGONAL_24,
    }
}

fn draw_charger_source_indicator<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    source: Option<DashboardInputSource>,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
) -> Result<(), P::Error> {
    match source {
        Some(DashboardInputSource::UsbC) => draw_icon_blocks_centered(
            painter,
            x,
            y,
            width,
            height,
            CARBON_USB_C_OUTLINE_24,
            palette.bg,
        ),
        Some(DashboardInputSource::DcIn) => draw_icon_blocks_centered(
            painter,
            x,
            y,
            width,
            height,
            CARBON_DC_BARREL_OUTLINE_24,
            palette.bg,
        ),
        Some(DashboardInputSource::Auto) => text(
            painter,
            variant,
            FontRole::DetailNum,
            "AUTO",
            Point::new((x + width / 2) as i32, (y + height / 2 + 3) as i32),
            HorizontalAlignment::Center,
            palette.bg,
        ),
        None => text(
            painter,
            variant,
            FontRole::DetailNum,
            "N/A",
            Point::new((x + width / 2) as i32, (y + height / 2 + 3) as i32),
            HorizontalAlignment::Center,
            palette.bg,
        ),
    }
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

// Icon source: Iconify / material-symbols-light:mode-fan-outline
const CARBON_FAN_OUTLINE_CARDINAL_24: &[(u8, u8, u8, u8)] = &[
    (10, 3, 5, 1),
    (9, 4, 7, 1),
    (9, 5, 1, 1),
    (14, 5, 2, 1),
    (8, 6, 2, 1),
    (13, 6, 2, 1),
    (9, 7, 1, 1),
    (12, 7, 2, 1),
    (4, 8, 2, 1),
    (9, 8, 2, 1),
    (12, 8, 2, 1),
    (17, 8, 1, 1),
    (3, 9, 4, 1),
    (9, 9, 11, 1),
    (3, 10, 2, 1),
    (6, 10, 10, 1),
    (19, 10, 2, 1),
    (3, 11, 2, 1),
    (7, 11, 4, 1),
    (13, 11, 2, 1),
    (19, 11, 2, 1),
    (3, 12, 2, 1),
    (9, 12, 2, 1),
    (13, 12, 4, 1),
    (19, 12, 2, 1),
    (3, 13, 3, 1),
    (8, 13, 10, 1),
    (19, 13, 2, 1),
    (4, 14, 11, 1),
    (17, 14, 4, 1),
    (6, 15, 1, 1),
    (10, 15, 2, 1),
    (13, 15, 2, 1),
    (18, 15, 1, 1),
    (10, 16, 2, 1),
    (14, 16, 1, 1),
    (9, 17, 2, 1),
    (14, 17, 2, 1),
    (8, 18, 2, 1),
    (14, 18, 1, 1),
    (8, 19, 7, 1),
    (9, 20, 5, 1),
];

const CARBON_FAN_OUTLINE_DIAGONAL_24: &[(u8, u8, u8, u8)] = &[
    (7, 3, 2, 1),
    (6, 4, 4, 1),
    (14, 4, 4, 1),
    (5, 5, 2, 1),
    (8, 5, 2, 1),
    (13, 5, 6, 1),
    (4, 6, 2, 1),
    (8, 6, 2, 1),
    (13, 6, 2, 1),
    (18, 6, 2, 1),
    (4, 7, 2, 1),
    (8, 7, 2, 1),
    (12, 7, 2, 1),
    (19, 7, 2, 1),
    (4, 8, 2, 1),
    (9, 8, 2, 1),
    (12, 8, 2, 1),
    (16, 8, 5, 1),
    (4, 9, 3, 1),
    (9, 9, 11, 1),
    (5, 10, 11, 1),
    (7, 11, 4, 1),
    (13, 11, 2, 1),
    (9, 12, 2, 1),
    (13, 12, 4, 1),
    (8, 13, 11, 1),
    (4, 14, 11, 1),
    (17, 14, 3, 1),
    (3, 15, 5, 1),
    (10, 15, 2, 1),
    (13, 15, 2, 1),
    (18, 15, 2, 1),
    (3, 16, 2, 1),
    (10, 16, 2, 1),
    (14, 16, 2, 1),
    (18, 16, 2, 1),
    (4, 17, 2, 1),
    (9, 17, 2, 1),
    (14, 17, 2, 1),
    (18, 17, 2, 1),
    (5, 18, 6, 1),
    (14, 18, 2, 1),
    (17, 18, 2, 1),
    (6, 19, 4, 1),
    (14, 19, 4, 1),
    (15, 20, 2, 1),
];

// Icon sources:
// - Iconify / mdi:usb-c-port
// - Iconify / mdi:audio-input-stereo-minijack (used as DC5025 indicator by product decision)
// - Iconify / ri:battery-charge-line
// - Iconify / ri:battery-line
const CARBON_USB_C_OUTLINE_24: &[(u8, u8, u8, u8)] = &[
    (3, 8, 18, 1),
    (2, 9, 20, 1),
    (1, 10, 4, 1),
    (19, 10, 4, 1),
    (1, 11, 3, 1),
    (20, 11, 3, 1),
    (1, 12, 2, 1),
    (5, 12, 14, 1),
    (21, 12, 2, 1),
    (1, 13, 2, 1),
    (5, 13, 14, 1),
    (21, 13, 2, 1),
    (1, 14, 3, 1),
    (20, 14, 3, 1),
    (1, 15, 4, 1),
    (19, 15, 4, 1),
    (2, 16, 20, 1),
    (3, 17, 18, 1),
];

const CARBON_DC_BARREL_OUTLINE_24: &[(u8, u8, u8, u8)] = &[
    (11, 2, 2, 1),
    (11, 3, 2, 1),
    (11, 5, 2, 1),
    (11, 6, 2, 1),
    (11, 7, 2, 1),
    (11, 8, 2, 1),
    (9, 9, 6, 1),
    (9, 10, 6, 1),
    (9, 11, 6, 1),
    (9, 12, 6, 1),
    (9, 13, 6, 1),
    (9, 14, 6, 1),
    (9, 15, 6, 1),
    (9, 16, 6, 1),
    (10, 17, 4, 1),
    (11, 18, 2, 1),
    (11, 19, 2, 1),
    (11, 20, 2, 1),
    (11, 21, 2, 1),
];

const RI_BATTERY_CHARGE_LINE_24: &[(u8, u8, u8, u8)] = &[
    (2, 5, 8, 1),
    (11, 5, 1, 1),
    (14, 5, 6, 1),
    (2, 6, 7, 1),
    (11, 6, 1, 1),
    (14, 6, 6, 1),
    (2, 7, 2, 1),
    (10, 7, 2, 1),
    (18, 7, 2, 1),
    (2, 8, 2, 1),
    (9, 8, 3, 1),
    (18, 8, 2, 1),
    (2, 9, 2, 1),
    (9, 9, 3, 1),
    (18, 9, 2, 1),
    (21, 9, 2, 1),
    (2, 10, 2, 1),
    (8, 10, 4, 1),
    (18, 10, 2, 1),
    (21, 10, 2, 1),
    (2, 11, 2, 1),
    (8, 11, 7, 1),
    (18, 11, 2, 1),
    (21, 11, 2, 1),
    (2, 12, 2, 1),
    (7, 12, 7, 1),
    (18, 12, 2, 1),
    (21, 12, 2, 1),
    (2, 13, 2, 1),
    (10, 13, 4, 1),
    (18, 13, 2, 1),
    (21, 13, 2, 1),
    (2, 14, 2, 1),
    (10, 14, 3, 1),
    (18, 14, 2, 1),
    (21, 14, 2, 1),
    (2, 15, 2, 1),
    (10, 15, 3, 1),
    (18, 15, 2, 1),
    (2, 16, 2, 1),
    (10, 16, 2, 1),
    (18, 16, 2, 1),
    (2, 17, 6, 1),
    (10, 17, 1, 1),
    (13, 17, 7, 1),
    (2, 18, 6, 1),
    (10, 18, 1, 1),
    (12, 18, 8, 1),
];

const RI_BATTERY_LINE_24: &[(u8, u8, u8, u8)] = &[
    (2, 5, 18, 1),
    (2, 6, 18, 1),
    (2, 7, 2, 1),
    (18, 7, 2, 1),
    (2, 8, 2, 1),
    (18, 8, 2, 1),
    (2, 9, 2, 1),
    (18, 9, 2, 1),
    (21, 9, 2, 1),
    (2, 10, 2, 1),
    (18, 10, 2, 1),
    (21, 10, 2, 1),
    (2, 11, 2, 1),
    (18, 11, 2, 1),
    (21, 11, 2, 1),
    (2, 12, 2, 1),
    (18, 12, 2, 1),
    (21, 12, 2, 1),
    (2, 13, 2, 1),
    (18, 13, 2, 1),
    (21, 13, 2, 1),
    (2, 14, 2, 1),
    (18, 14, 2, 1),
    (21, 14, 2, 1),
    (2, 15, 2, 1),
    (18, 15, 2, 1),
    (2, 16, 2, 1),
    (18, 16, 2, 1),
    (2, 17, 18, 1),
    (2, 18, 18, 1),
];

// 18px outline footer icons for better legibility on the small touch display.
const CARBON_CHECKMARK_OUTLINE_18: &[(u8, u8, u8, u8)] = &[
    (7, 1, 4, 1),
    (5, 2, 2, 1),
    (11, 2, 2, 1),
    (3, 3, 2, 1),
    (13, 3, 2, 1),
    (3, 4, 1, 1),
    (14, 4, 1, 1),
    (2, 5, 1, 1),
    (15, 5, 1, 1),
    (2, 6, 1, 1),
    (15, 6, 1, 1),
    (1, 7, 1, 1),
    (11, 7, 1, 1),
    (16, 7, 1, 1),
    (1, 8, 1, 1),
    (10, 8, 1, 1),
    (16, 8, 1, 1),
    (1, 9, 1, 1),
    (5, 9, 2, 1),
    (9, 9, 1, 1),
    (16, 9, 1, 1),
    (1, 10, 1, 1),
    (6, 10, 3, 1),
    (16, 10, 1, 1),
    (2, 11, 1, 1),
    (7, 11, 1, 1),
    (15, 11, 1, 1),
    (2, 12, 1, 1),
    (15, 12, 1, 1),
    (3, 13, 1, 1),
    (14, 13, 1, 1),
    (3, 14, 2, 1),
    (13, 14, 2, 1),
    (5, 15, 2, 1),
    (11, 15, 2, 1),
    (7, 16, 4, 1),
];

const CARBON_CHECKBOX_INDETERMINATE_18: &[(u8, u8, u8, u8)] = &[
    (3, 2, 12, 1),
    (2, 3, 2, 1),
    (14, 3, 2, 1),
    (2, 4, 1, 1),
    (15, 4, 1, 1),
    (2, 5, 1, 1),
    (15, 5, 1, 1),
    (2, 6, 1, 1),
    (15, 6, 1, 1),
    (2, 7, 1, 1),
    (15, 7, 1, 1),
    (2, 8, 1, 1),
    (6, 8, 6, 1),
    (15, 8, 1, 1),
    (2, 9, 1, 1),
    (6, 9, 6, 1),
    (15, 9, 1, 1),
    (2, 10, 1, 1),
    (15, 10, 1, 1),
    (2, 11, 1, 1),
    (15, 11, 1, 1),
    (2, 12, 1, 1),
    (15, 12, 1, 1),
    (2, 13, 1, 1),
    (15, 13, 1, 1),
    (2, 14, 2, 1),
    (14, 14, 2, 1),
    (3, 15, 12, 1),
];

const CARBON_WARNING_ALT_18: &[(u8, u8, u8, u8)] = &[
    (8, 2, 2, 1),
    (8, 3, 2, 1),
    (7, 4, 1, 1),
    (10, 4, 1, 1),
    (7, 5, 1, 1),
    (10, 5, 1, 1),
    (6, 6, 1, 1),
    (11, 6, 1, 1),
    (6, 7, 1, 1),
    (8, 7, 2, 1),
    (11, 7, 1, 1),
    (5, 8, 1, 1),
    (8, 8, 2, 1),
    (12, 8, 1, 1),
    (5, 9, 1, 1),
    (8, 9, 2, 1),
    (12, 9, 1, 1),
    (4, 10, 1, 1),
    (8, 10, 2, 1),
    (13, 10, 1, 1),
    (4, 11, 1, 1),
    (13, 11, 1, 1),
    (3, 12, 1, 1),
    (14, 12, 1, 1),
    (3, 13, 1, 1),
    (8, 13, 2, 1),
    (14, 13, 1, 1),
    (2, 14, 1, 1),
    (15, 14, 1, 1),
    (1, 15, 2, 1),
    (15, 15, 2, 1),
    (1, 16, 16, 1),
];

const CARBON_ERROR_OUTLINE_18: &[(u8, u8, u8, u8)] = &[
    (7, 1, 4, 1),
    (5, 2, 2, 1),
    (11, 2, 2, 1),
    (3, 3, 2, 1),
    (13, 3, 2, 1),
    (3, 4, 1, 1),
    (14, 4, 1, 1),
    (2, 5, 1, 1),
    (15, 5, 1, 1),
    (2, 6, 1, 1),
    (6, 6, 1, 1),
    (15, 6, 1, 1),
    (1, 7, 1, 1),
    (7, 7, 1, 1),
    (16, 7, 1, 1),
    (1, 8, 1, 1),
    (8, 8, 1, 1),
    (16, 8, 1, 1),
    (1, 9, 1, 1),
    (9, 9, 1, 1),
    (16, 9, 1, 1),
    (1, 10, 1, 1),
    (10, 10, 1, 1),
    (16, 10, 1, 1),
    (2, 11, 1, 1),
    (11, 11, 1, 1),
    (15, 11, 1, 1),
    (2, 12, 1, 1),
    (15, 12, 1, 1),
    (3, 13, 1, 1),
    (14, 13, 1, 1),
    (3, 14, 2, 1),
    (13, 14, 2, 1),
    (5, 15, 2, 1),
    (11, 15, 2, 1),
    (7, 16, 4, 1),
];

const CARBON_HELP_18: &[(u8, u8, u8, u8)] = &[
    (7, 1, 4, 1),
    (5, 2, 2, 1),
    (11, 2, 2, 1),
    (3, 3, 2, 1),
    (13, 3, 2, 1),
    (3, 4, 1, 1),
    (14, 4, 1, 1),
    (2, 5, 1, 1),
    (7, 5, 5, 1),
    (15, 5, 1, 1),
    (2, 6, 1, 1),
    (6, 6, 1, 1),
    (11, 6, 1, 1),
    (15, 6, 1, 1),
    (1, 7, 1, 1),
    (11, 7, 1, 1),
    (16, 7, 1, 1),
    (1, 8, 1, 1),
    (9, 8, 3, 1),
    (16, 8, 1, 1),
    (1, 9, 1, 1),
    (8, 9, 2, 1),
    (16, 9, 1, 1),
    (1, 10, 1, 1),
    (8, 10, 2, 1),
    (16, 10, 1, 1),
    (2, 11, 1, 1),
    (15, 11, 1, 1),
    (2, 12, 1, 1),
    (15, 12, 1, 1),
    (3, 13, 1, 1),
    (8, 13, 2, 1),
    (14, 13, 1, 1),
    (3, 14, 2, 1),
    (13, 14, 2, 1),
    (5, 15, 2, 1),
    (11, 15, 2, 1),
    (7, 16, 4, 1),
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

fn tps_label(
    snapshot: &SelfCheckUiSnapshot,
    selector: OutputSelector,
    state: SelfCheckCommState,
    enabled: Option<bool>,
) -> &'static str {
    let _ = snapshot;
    let _ = selector;
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

fn charger_label(
    state: SelfCheckCommState,
    allow_charge: Option<bool>,
    input_present: Option<bool>,
) -> &'static str {
    match state {
        SelfCheckCommState::Pending => "PEND",
        SelfCheckCommState::Warn => "WARN",
        SelfCheckCommState::Err => "ERR",
        SelfCheckCommState::NotAvailable => "N/A",
        SelfCheckCommState::Ok => match allow_charge {
            Some(true) => "RUN",
            Some(false) if input_present == Some(true) => "IDLE",
            Some(false) => "IDLE",
            None => "OK",
        },
    }
}

fn bms_label(snapshot: &SelfCheckUiSnapshot) -> &'static str {
    if snapshot.bq40z50_recovery_pending {
        return "RECOVER";
    }
    if snapshot.bq40z50_no_battery == Some(true) || bms_limited(snapshot) {
        return "LIMIT";
    }

    match snapshot.bq40z50 {
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
    render_variant_b(
        painter,
        variant,
        palette,
        data,
        DashboardRoute::Home,
        self_check,
    )
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
        FontRole::DetailTitle,
        title,
        Point::new(8, 2),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    text(
        painter,
        variant,
        FontRole::DetailBody,
        subtitle,
        Point::new(106, 2),
        HorizontalAlignment::Left,
        palette.text_dim,
    )?;
    text(
        painter,
        variant,
        FontRole::DetailBody,
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
        FontRole::DetailTitle => &FONT_A_DETAIL,
        FontRole::DetailBody => &FONT_A_DETAIL,
        FontRole::Num => &FONT_B_NUM,
        FontRole::NumCompact => &FONT_B_NUM,
        FontRole::DetailNum => &FONT_B_DETAIL,
        FontRole::NumBig => &FONT_B_NUM_BIG,
        FontRole::NumHero => &FONT_B_NUM_HERO,
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

        assert!(live.mains_present);
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
    fn bq40_activation_requires_offline_state() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.bq40z50 = SelfCheckCommState::Warn;
        snapshot.bq40z50_discharge_ready = Some(false);

        assert!(!is_bq40_activation_needed(&snapshot));

        snapshot.bq40z50 = SelfCheckCommState::Err;
        assert!(is_bq40_activation_needed(&snapshot));
    }

    #[test]
    fn self_check_can_enter_dashboard_only_when_all_modules_clear() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.gc9307 = SelfCheckCommState::Ok;
        snapshot.tca6408a = SelfCheckCommState::Ok;
        snapshot.fusb302 = SelfCheckCommState::Ok;
        snapshot.ina3221 = SelfCheckCommState::Ok;
        snapshot.bq25792 = SelfCheckCommState::Ok;
        snapshot.bq40z50 = SelfCheckCommState::Ok;
        snapshot.bq40z50_discharge_ready = Some(true);
        snapshot.requested_outputs = EnabledOutputs::Only(OutputSelector::OutA);
        snapshot.active_outputs = EnabledOutputs::Only(OutputSelector::OutA);
        snapshot.tps_a = SelfCheckCommState::Ok;
        snapshot.tps_b = SelfCheckCommState::Ok;
        snapshot.tmp_a = SelfCheckCommState::Ok;
        snapshot.tmp_b = SelfCheckCommState::Ok;

        assert!(self_check_can_enter_dashboard(&snapshot));

        snapshot.output_gate_reason = OutputGateReason::BmsNotReady;
        snapshot.active_outputs = EnabledOutputs::None;
        assert!(!self_check_can_enter_dashboard(&snapshot));
    }

    #[test]
    fn battery_flow_uses_limit_when_bms_blocks_discharge() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.bq40z50 = SelfCheckCommState::Warn;
        snapshot.bq40z50_discharge_ready = Some(false);
        snapshot.bq25792 = SelfCheckCommState::Ok;

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);

        assert_eq!(
            detail_status_tag(DashboardDetailPage::BatteryFlow, live),
            "LIMIT"
        );
        assert_eq!(
            detail_fault_row_text(DashboardDetailPage::BatteryFlow, live),
            "LIMIT"
        );
    }

    #[test]
    fn output_detail_uses_hold_when_bms_gate_blocks_requested_output() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.requested_outputs = EnabledOutputs::Only(OutputSelector::OutA);
        snapshot.active_outputs = EnabledOutputs::None;
        snapshot.recoverable_outputs = EnabledOutputs::Only(OutputSelector::OutA);
        snapshot.output_gate_reason = OutputGateReason::BmsNotReady;
        snapshot.bq40z50 = SelfCheckCommState::Warn;
        snapshot.bq40z50_discharge_ready = Some(false);
        snapshot.tps_a = SelfCheckCommState::Err;

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);

        assert_eq!(detail_status_tag(DashboardDetailPage::Output, live), "HOLD");
        assert_eq!(
            output_fault_row_text(
                live.tps_a_state,
                live.out_a_on,
                live.output_hold(OutputSelector::OutA),
                live.output_recovery_pending(OutputSelector::OutA),
                "HOLD",
            ),
            "HOLD"
        );
    }

    #[test]
    fn self_check_tps_summary_uses_raw_probe_state() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.requested_outputs = EnabledOutputs::Only(OutputSelector::OutA);
        snapshot.active_outputs = EnabledOutputs::None;
        snapshot.recoverable_outputs = EnabledOutputs::Only(OutputSelector::OutA);
        snapshot.output_gate_reason = OutputGateReason::BmsNotReady;
        snapshot.tps_a = SelfCheckCommState::Ok;
        snapshot.tps_b = SelfCheckCommState::Err;

        assert_eq!(self_check_tps_a_summary_name(&snapshot), "ok");
        assert_eq!(self_check_tps_b_summary_name(&snapshot), "err");
    }

    #[test]
    fn charger_detail_keeps_idle_when_input_present_but_charge_not_allowed() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.fusb302_vbus_present = Some(true);
        snapshot.bq25792 = SelfCheckCommState::Ok;
        snapshot.bq25792_allow_charge = Some(false);

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);

        assert_eq!(
            detail_status_tag(DashboardDetailPage::Charger, live),
            "IDLE"
        );
        assert_eq!(charger_state_text(live), "IDLE");
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

        assert!(live.mains_present);
        assert_eq!(live.input_power_w10(), None);
    }

    #[test]
    fn live_dashboard_prefers_charger_adc_input_power_when_available() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.fusb302_vbus_present = Some(true);
        snapshot.input_vbus_mv = Some(20_000);
        snapshot.input_ibus_ma = Some(1_500);
        snapshot.vin_vbus_mv = Some(19_200);
        snapshot.vin_iin_ma = Some(910);

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);

        assert_eq!(live.input_power_w10(), Some(300));
    }

    #[test]
    fn dashboard_hit_test_maps_fixed_home_regions() {
        assert_eq!(
            dashboard_hit_test(DashboardRoute::Home, 30, 40),
            Some(DashboardTouchTarget::HomeOutput)
        );
        assert_eq!(
            dashboard_hit_test(DashboardRoute::Home, 30, 120),
            Some(DashboardTouchTarget::HomeThermal)
        );
        assert_eq!(
            dashboard_hit_test(DashboardRoute::Home, 250, 40),
            Some(DashboardTouchTarget::HomeCells)
        );
        assert_eq!(
            dashboard_hit_test(DashboardRoute::Home, 250, 90),
            Some(DashboardTouchTarget::HomeCharger)
        );
        assert_eq!(
            dashboard_hit_test(DashboardRoute::Home, 250, 140),
            Some(DashboardTouchTarget::HomeBatteryFlow)
        );
    }

    #[test]
    fn dashboard_detail_back_target_maps_to_home() {
        assert_eq!(
            dashboard_hit_test(
                DashboardRoute::Detail(DashboardDetailPage::Output),
                DASHBOARD_DETAIL_BACK_X + 4,
                DASHBOARD_DETAIL_BACK_Y + 4
            ),
            Some(DashboardTouchTarget::DetailBack)
        );
        assert_eq!(
            dashboard_route_for_target(DashboardTouchTarget::DetailBack),
            DashboardRoute::Home
        );
        assert_eq!(
            dashboard_route_for_target(DashboardTouchTarget::HomeOutput),
            DashboardRoute::Detail(DashboardDetailPage::Output)
        );
    }

    #[test]
    fn thermal_detail_uses_battery_temp_for_hotspot_and_status() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.tmp_a_c = Some(36);
        snapshot.tmp_b_c = Some(39);
        snapshot.dashboard_detail.battery_temp_c = Some(67);

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);

        assert_eq!(thermal_hotspot_c(live), Some(67));
        assert_eq!(detail_status_tag(DashboardDetailPage::Thermal, live), "HOT");
    }

    #[test]
    fn detail_status_tags_prioritize_fault_states() {
        let mut battery_fault = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        battery_fault.bq40z50 = SelfCheckCommState::Err;
        battery_fault.bq40z50_current_ma = Some(1200);
        let battery_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &battery_fault);
        assert_eq!(
            detail_status_tag(DashboardDetailPage::BatteryFlow, battery_live),
            "FAULT"
        );

        let mut charger_fault = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        charger_fault.bq25792 = SelfCheckCommState::Err;
        charger_fault.bq25792_allow_charge = Some(true);
        charger_fault.bq25792_ichg_ma = Some(900);
        let charger_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &charger_fault);
        assert_eq!(
            detail_status_tag(DashboardDetailPage::Charger, charger_live),
            "FAULT"
        );

        let mut output_fault = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        output_fault.tps_a = SelfCheckCommState::Err;
        output_fault.tps_a_enabled = Some(true);
        output_fault.tps_b_enabled = Some(true);
        let output_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &output_fault);
        assert_eq!(
            detail_status_tag(DashboardDetailPage::Output, output_live),
            "FAULT"
        );
    }

    #[test]
    fn charger_detail_keeps_missing_session_data_as_na() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.fusb302_vbus_present = Some(true);

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);

        assert_eq!(charger_active_value(live), None);
        assert_eq!(charger_state_text(live), "N/A");
        assert_eq!(detail_status_tag(DashboardDetailPage::Charger, live), "N/A");
    }

    #[test]
    fn thermal_fault_status_beats_temperature_band() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.tmp_a = SelfCheckCommState::Err;
        snapshot.tmp_a_c = Some(38);
        snapshot.dashboard_detail.battery_temp_c = Some(42);

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);

        assert!(thermal_fault_present(live));
        assert_eq!(
            detail_status_tag(DashboardDetailPage::Thermal, live),
            "FAULT"
        );
    }

    #[test]
    fn thermal_fan_motion_uses_discrete_speed_bands() {
        assert_eq!(
            thermal_fan_motion(Some(0), Some(0), Some("OFF")),
            ThermalFanMotion::Off
        );
        assert_eq!(
            thermal_fan_motion(Some(1_250), Some(32), Some("LOW")),
            ThermalFanMotion::Low
        );
        assert_eq!(
            thermal_fan_motion(Some(2_380), Some(52), Some("MID")),
            ThermalFanMotion::Mid
        );
        assert_eq!(
            thermal_fan_motion(Some(4_120), Some(100), Some("HIGH")),
            ThermalFanMotion::High
        );
    }

    #[test]
    fn thermal_fan_frame_steps_faster_at_higher_rpm() {
        assert_eq!(thermal_fan_frame(0, Some(0), Some(0), Some("OFF")), 0);
        assert_eq!(thermal_fan_frame(17, Some(1_250), Some(32), Some("LOW")), 0);
        assert_eq!(thermal_fan_frame(18, Some(1_250), Some(32), Some("LOW")), 1);
        assert_eq!(thermal_fan_frame(9, Some(2_380), Some(52), Some("MID")), 0);
        assert_eq!(thermal_fan_frame(10, Some(2_380), Some(52), Some("MID")), 1);
        assert_eq!(
            thermal_fan_frame(4, Some(4_120), Some(100), Some("HIGH")),
            0
        );
        assert_eq!(
            thermal_fan_frame(5, Some(4_120), Some(100), Some("HIGH")),
            1
        );
    }

    #[test]
    fn cells_warn_status_beats_balance_indicator() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.bq40z50_rca_alarm = Some(true);
        snapshot.dashboard_detail.balance_active = Some(true);
        snapshot.dashboard_detail.balance_mask = Some(0b0010);
        snapshot.dashboard_detail.balance_cell = Some(2);

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);

        assert_eq!(detail_status_tag(DashboardDetailPage::Cells, live), "WARN");
    }

    #[test]
    fn cells_detail_shows_multi_balance_summary_when_multiple_cells_are_active() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.dashboard_detail.balance_active = Some(true);
        snapshot.dashboard_detail.balance_mask = Some(0b0101);

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);

        assert_eq!(
            detail_status_tag(DashboardDetailPage::Cells, live),
            "BAL ON"
        );
        assert_eq!(detail_balance_summary_text(live.detail), "C1+C3");
    }

    #[test]
    fn pending_detail_pages_stay_na_instead_of_reporting_healthy() {
        let snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);

        assert_eq!(detail_status_tag(DashboardDetailPage::Cells, live), "N/A");
        assert_eq!(
            detail_status_tag(DashboardDetailPage::BatteryFlow, live),
            "N/A"
        );
        assert_eq!(detail_status_tag(DashboardDetailPage::Output, live), "N/A");
        assert_eq!(
            detail_footer_notice(DashboardDetailPage::Cells, live),
            "CELL DETAIL SOURCE PENDING"
        );
        assert_eq!(
            detail_footer_notice(DashboardDetailPage::BatteryFlow, live),
            "PACK DETAIL SOURCE PENDING"
        );
        assert_eq!(
            detail_footer_notice(DashboardDetailPage::Output, live),
            "OUTPUT DETAIL SOURCE PENDING"
        );
        assert_eq!(
            detail_footer_badge(DashboardDetailPage::BatteryFlow, live),
            (DetailFooterIcon::Unknown, "NO DATA")
        );
    }

    #[test]
    fn footer_badges_prefer_mock_and_warn_short_forms() {
        let mut mock_snapshot = SelfCheckUiSnapshot::pending(UpsMode::Backup);
        mock_snapshot.bq40z50 = SelfCheckCommState::Ok;
        mock_snapshot.bq40z50_pack_mv = Some(14_820);
        mock_snapshot.bq40z50_current_ma = Some(-1_880);
        mock_snapshot.dashboard_detail.battery_energy_mwh = Some(46_850);
        mock_snapshot.dashboard_detail.battery_full_capacity_mwh = Some(63_200);
        mock_snapshot.dashboard_detail.charge_fet_on = Some(false);
        mock_snapshot.dashboard_detail.discharge_fet_on = Some(true);
        mock_snapshot.dashboard_detail.precharge_fet_on = Some(false);
        mock_snapshot.dashboard_detail.battery_notice = Some("PACK FLOW MOCKED - LIVE SOURCE NEXT");
        let mock_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Backup), &mock_snapshot);
        assert_eq!(
            detail_footer_badge(DashboardDetailPage::BatteryFlow, mock_live),
            (DetailFooterIcon::Mock, "MOCK DATA")
        );

        let mut warn_snapshot = SelfCheckUiSnapshot::pending(UpsMode::Supplement);
        warn_snapshot.tps_a = SelfCheckCommState::Ok;
        warn_snapshot.tps_b = SelfCheckCommState::Ok;
        warn_snapshot.tps_a_enabled = Some(true);
        warn_snapshot.tps_b_enabled = Some(false);
        warn_snapshot.out_a_vbus_mv = Some(19_040);
        warn_snapshot.tps_a_iout_ma = Some(620);
        warn_snapshot.dashboard_detail.output_notice = Some("OUT-B STANDBY PATH HELD");
        let warn_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Supplement), &warn_snapshot);
        assert_eq!(
            detail_footer_badge(DashboardDetailPage::Output, warn_live),
            (DetailFooterIcon::Warn, "CHECK ROWS")
        );
    }

    #[test]
    fn fault_rows_follow_page_fault_state() {
        let mut charger_fault = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        charger_fault.bq25792 = SelfCheckCommState::Err;
        let charger_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &charger_fault);
        assert_eq!(
            detail_fault_row_text(DashboardDetailPage::Charger, charger_live),
            "LINK"
        );
        assert_eq!(
            detail_fault_notice(DashboardDetailPage::Charger, charger_live),
            "CHARGER LINK FAULT"
        );

        let mut thermal_fault = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        thermal_fault.tmp_b = SelfCheckCommState::Err;
        let thermal_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &thermal_fault);
        assert_eq!(
            detail_fault_row_text(DashboardDetailPage::Thermal, thermal_live),
            "SENSE"
        );
        assert_eq!(
            detail_fault_notice(DashboardDetailPage::Thermal, thermal_live),
            "THERMAL SENSE FAULT"
        );

        assert_eq!(
            output_fault_row_text(SelfCheckCommState::Err, true, false, false, "HOLD"),
            "FAULT"
        );
    }

    #[test]
    fn warn_states_surface_as_warn_in_detail_status_and_rows() {
        let mut battery_warn = SelfCheckUiSnapshot::pending(UpsMode::Backup);
        battery_warn.bq40z50 = SelfCheckCommState::Warn;
        battery_warn.bq40z50_pack_mv = Some(16_540);
        battery_warn.bq40z50_current_ma = Some(237);
        let battery_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Backup), &battery_warn);
        assert_eq!(
            detail_status_tag(DashboardDetailPage::BatteryFlow, battery_live),
            "WARN"
        );
        assert_eq!(
            detail_fault_row_text(DashboardDetailPage::BatteryFlow, battery_live),
            "WARN"
        );

        let mut charger_warn = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        charger_warn.bq25792 = SelfCheckCommState::Warn;
        charger_warn.fusb302_vbus_present = Some(true);
        charger_warn.dashboard_detail.charger_active = Some(true);
        charger_warn.dashboard_detail.charger_status = Some("CHG");
        let charger_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &charger_warn);
        assert_eq!(
            detail_status_tag(DashboardDetailPage::Charger, charger_live),
            "WARN"
        );
        assert_eq!(
            detail_fault_row_text(DashboardDetailPage::Charger, charger_live),
            "WARN"
        );

        assert_eq!(
            output_fault_row_text(SelfCheckCommState::Warn, true, false, false, "HOLD"),
            "WARN"
        );
    }

    #[test]
    fn thermal_warn_status_beats_temperature_band_without_escalating_to_fault() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.tmp_a = SelfCheckCommState::Warn;
        snapshot.tmp_a_c = Some(38);
        snapshot.dashboard_detail.battery_temp_c = Some(42);

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);

        assert!(!thermal_fault_present(live));
        assert!(thermal_warn_present(live));
        assert_eq!(
            detail_status_tag(DashboardDetailPage::Thermal, live),
            "WARN"
        );
        assert_eq!(
            detail_fault_row_text(DashboardDetailPage::Thermal, live),
            "WARN"
        );
    }

    #[test]
    fn non_fault_rows_use_short_clear_tokens() {
        let mut charger_ok = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        charger_ok.fusb302_vbus_present = Some(true);
        charger_ok.dashboard_detail.charger_active = Some(true);
        charger_ok.dashboard_detail.charger_status = Some("CHG");
        let charger_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &charger_ok);
        assert_eq!(
            detail_fault_row_text(DashboardDetailPage::Charger, charger_live),
            "CLEAR"
        );

        let mut thermal_ok = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        thermal_ok.tmp_a_c = Some(34);
        thermal_ok.dashboard_detail.fan_status = Some("HIGH");
        let thermal_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &thermal_ok);
        assert_eq!(
            detail_fault_row_text(DashboardDetailPage::Thermal, thermal_live),
            "CLEAR"
        );
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
        snapshot.fusb302_vbus_present = Some(false);
        snapshot.vin_mains_present = Some(true);
        snapshot.vin_vbus_mv = None;

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);

        assert!(live.mains_present);
    }

    #[test]
    fn live_dashboard_falls_back_to_charger_after_stale_vin_latch_expires() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.fusb302_vbus_present = Some(false);
        snapshot.vin_mains_present = None;
        snapshot.vin_vbus_mv = None;

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);

        assert!(!live.mains_present);
    }
}
