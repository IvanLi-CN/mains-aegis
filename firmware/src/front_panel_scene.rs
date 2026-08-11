use core::fmt::Write as _;

use embedded_graphics_core::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Point, Size},
    pixelcolor::{raw::RawU16, Rgb565},
    prelude::RawData,
    Pixel,
};
use mains_aegis_firmware::net_types::{WifiConnectionState, WifiSnapshot};
use mains_aegis_firmware::output_state::{EnabledOutputs, OutputGateReason, OutputSelector};
use u8g2_fonts::{
    fonts,
    types::{FontColor, HorizontalAlignment, VerticalPosition},
    Content, Error as FontError, FontRenderer,
};

pub const UI_W: u16 = 320;
pub const UI_H: u16 = 172;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TouchRect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl TouchRect {
    pub const fn new(x: u16, y: u16, w: u16, h: u16) -> Self {
        Self { x, y, w, h }
    }

    pub const fn contains(self, x: u16, y: u16) -> bool {
        x >= self.x
            && x < self.x.saturating_add(self.w)
            && y >= self.y
            && y < self.y.saturating_add(self.h)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub const fn area(self) -> u32 {
        self.w as u32 * self.h as u32
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub const fn overlaps(self, other: Self) -> bool {
        self.x < other.x.saturating_add(other.w)
            && other.x < self.x.saturating_add(self.w)
            && self.y < other.y.saturating_add(other.h)
            && other.y < self.y.saturating_add(self.h)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub const fn within_screen(self) -> bool {
        self.w > 0
            && self.h > 0
            && self.x.saturating_add(self.w) <= UI_W
            && self.y.saturating_add(self.h) <= UI_H
    }
}
const VIN_MAINS_PRESENT_THRESHOLD_MV: u16 = 3_000;

fn mains_present_from_vin(vin_vbus_mv: Option<u16>) -> Option<bool> {
    vin_vbus_mv.map(|mv| mv >= VIN_MAINS_PRESENT_THRESHOLD_MV)
}

fn snapshot_mains_present_value(snapshot: &SelfCheckUiSnapshot) -> Option<bool> {
    mains_present_from_vin(snapshot.vin_vbus_mv)
        .or(snapshot.vin_mains_present)
        .or(snapshot.aggregate_input_present)
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
const ATTENTION_COLOR: u16 = 0xFCA0;
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
    Blocked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DashboardDetailPage {
    Cells,
    BmsDetail,
    BatteryFlow,
    Output,
    Charger,
    Thermal,
    Wifi,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManualChargeTarget {
    Pack3V7,
    Rsoc80,
    Full100,
}

impl ManualChargeTarget {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pack3V7 => "3.7V",
            Self::Rsoc80 => "80%",
            Self::Full100 => "100%",
        }
    }
}

impl Default for ManualChargeTarget {
    fn default() -> Self {
        Self::Full100
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManualChargeSpeed {
    Ma100,
    Ma500,
    Ma1000,
}

impl ManualChargeSpeed {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ma100 => "100mA",
            Self::Ma500 => "500mA",
            Self::Ma1000 => "1A",
        }
    }

    pub const fn ichg_ma(self) -> u16 {
        match self {
            Self::Ma100 => 100,
            Self::Ma500 => 500,
            Self::Ma1000 => 1_000,
        }
    }
}

impl Default for ManualChargeSpeed {
    fn default() -> Self {
        Self::Ma500
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManualChargeTimerLimit {
    H1,
    H2,
    H6,
}

impl ManualChargeTimerLimit {
    pub const fn label(self) -> &'static str {
        match self {
            Self::H1 => "1h",
            Self::H2 => "2h",
            Self::H6 => "6h",
        }
    }

    pub const fn hours(self) -> u8 {
        match self {
            Self::H1 => 1,
            Self::H2 => 2,
            Self::H6 => 6,
        }
    }
}

impl Default for ManualChargeTimerLimit {
    fn default() -> Self {
        Self::H2
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ManualChargePowerPath {
    #[default]
    Auto,
    DcIn,
    UsbC,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ManualChargePrefs {
    pub target: ManualChargeTarget,
    pub speed: ManualChargeSpeed,
    pub timer_limit: ManualChargeTimerLimit,
    pub power_path: ManualChargePowerPath,
}

impl ManualChargePrefs {
    pub const fn defaults() -> Self {
        Self {
            target: ManualChargeTarget::Full100,
            speed: ManualChargeSpeed::Ma500,
            timer_limit: ManualChargeTimerLimit::H2,
            power_path: ManualChargePowerPath::Auto,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManualChargeStopReason {
    None,
    UserStop,
    TimerExpired,
    PackReached,
    RsocReached,
    FullReached,
    SafetyBlocked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ManualChargeRuntimeState {
    pub active: bool,
    pub takeover: bool,
    pub loopback_override: bool,
    pub stop_inhibit: bool,
    pub last_stop_reason: ManualChargeStopReason,
    pub remaining_minutes: Option<u16>,
    pub requested_power_path: ManualChargePowerPath,
    pub bound_power_path: Option<DashboardInputSource>,
    pub binding_reason: Option<&'static str>,
    pub start_state: &'static str,
    pub start_block_reason: Option<&'static str>,
    pub loop_confirmation_required: bool,
    pub output_power_w10: Option<u32>,
    pub power_telemetry_fresh: bool,
}

impl ManualChargeRuntimeState {
    pub const fn idle() -> Self {
        Self {
            active: false,
            takeover: false,
            loopback_override: false,
            stop_inhibit: false,
            last_stop_reason: ManualChargeStopReason::None,
            remaining_minutes: None,
            requested_power_path: ManualChargePowerPath::Auto,
            bound_power_path: None,
            binding_reason: None,
            start_state: "blocked",
            start_block_reason: Some("manual_charge_path_unavailable"),
            loop_confirmation_required: false,
            output_power_w10: None,
            power_telemetry_fresh: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ManualChargeUiSnapshot {
    pub prefs: ManualChargePrefs,
    pub runtime: ManualChargeRuntimeState,
}

impl ManualChargeUiSnapshot {
    pub const fn pending() -> Self {
        Self {
            prefs: ManualChargePrefs::defaults(),
            runtime: ManualChargeRuntimeState::idle(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManualChargeUiAction {
    SetTarget(ManualChargeTarget),
    SetSpeed(ManualChargeSpeed),
    SetTimerLimit(ManualChargeTimerLimit),
    SetPowerPath(ManualChargePowerPath),
    Start,
    StartConfirmedLoopback,
    Stop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DashboardRoute {
    Home,
    Detail(DashboardDetailPage),
    ManualCharge,
}

/// Preview-facing alert categories. Runtime alert state is intentionally not
/// connected until the owner approves the rendered interaction set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertPreviewKind {
    MainsAbsentDc,
    HighStress,
    BatteryLowNoMains,
    BatteryLowWithMains,
    ShutdownProtection,
    IoOverVoltage,
    IoOverCurrent,
    ModuleFault,
    BatteryProtection,
}

impl AlertPreviewKind {
    pub const ALL: [Self; 9] = [
        Self::MainsAbsentDc,
        Self::HighStress,
        Self::BatteryLowNoMains,
        Self::BatteryLowWithMains,
        Self::ShutdownProtection,
        Self::IoOverVoltage,
        Self::IoOverCurrent,
        Self::ModuleFault,
        Self::BatteryProtection,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::MainsAbsentDc => "MAINS LOST",
            Self::HighStress => "HIGH STRESS",
            Self::BatteryLowNoMains | Self::BatteryLowWithMains => "BATTERY LOW",
            Self::ShutdownProtection => "SHUTDOWN PROTECT",
            Self::IoOverVoltage => "IO OVER VOLTAGE",
            Self::IoOverCurrent => "IO OVER CURRENT",
            Self::ModuleFault => "MODULE FAULT",
            Self::BatteryProtection => "BATTERY PROTECT",
        }
    }

    pub const fn summary(self) -> &'static str {
        match self {
            Self::MainsAbsentDc => "RUNNING ON BATTERY",
            Self::HighStress => "CHECK THERMAL LOAD",
            Self::BatteryLowNoMains => "NO MAINS - REDUCE LOAD",
            Self::BatteryLowWithMains => "CHECK CHARGING PATH",
            Self::ShutdownProtection => "OUTPUT PROTECTION ACTIVE",
            Self::IoOverVoltage => "CHECK OUTPUT LOAD",
            Self::IoOverCurrent => "REDUCE OUTPUT LOAD",
            Self::ModuleFault => "CHECK DEVICE DIAGNOSTICS",
            Self::BatteryProtection => "CHECK BATTERY STATUS",
        }
    }

    pub const fn default_severity(self) -> AlertPreviewSeverity {
        match self {
            Self::MainsAbsentDc
            | Self::HighStress
            | Self::BatteryLowNoMains
            | Self::BatteryLowWithMains => AlertPreviewSeverity::Warning,
            Self::ShutdownProtection
            | Self::IoOverVoltage
            | Self::IoOverCurrent
            | Self::ModuleFault
            | Self::BatteryProtection => AlertPreviewSeverity::Critical,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertPreviewSeverity {
    Warning,
    Critical,
}

impl AlertPreviewSeverity {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Warning => "WARNING",
            Self::Critical => "CRITICAL",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertPreviewSoundState {
    Audible,
    Muted,
    SystemSilent,
    PolicySilent,
}

impl AlertPreviewSoundState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Audible => "AUDIBLE",
            Self::Muted => "MUTED",
            Self::SystemSilent => "SYSTEM SILENT",
            Self::PolicySilent => "POLICY SILENT",
        }
    }

    const fn can_mute(self) -> bool {
        matches!(self, Self::Audible)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlertPreviewItem {
    pub kind: AlertPreviewKind,
    pub instance_id: u32,
    pub severity: AlertPreviewSeverity,
    pub sound: AlertPreviewSoundState,
    pub cleared: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertPreviewTouchTarget {
    Back,
    Row(usize),
    Mute(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertDetailTouchTarget {
    Back,
    Mute,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DashboardHomeTouchTarget {
    Alerts,
    Dashboard(DashboardTouchTarget),
}

pub const fn dashboard_alert_hit_test(x: u16, y: u16) -> bool {
    DASHBOARD_HOME_ALERT_TOUCH.contains(x, y)
}

pub fn dashboard_home_hit_test(
    alerts_available: bool,
    x: u16,
    y: u16,
) -> Option<DashboardHomeTouchTarget> {
    if alerts_available && dashboard_alert_hit_test(x, y) {
        Some(DashboardHomeTouchTarget::Alerts)
    } else {
        dashboard_hit_test(DashboardRoute::Home, x, y).map(DashboardHomeTouchTarget::Dashboard)
    }
}

pub const fn alert_list_hit_test(x: u16, y: u16, top: usize) -> Option<AlertPreviewTouchTarget> {
    if ALERT_LIST_TOP_BACK_TOUCH.contains(x, y) {
        return Some(AlertPreviewTouchTarget::Back);
    }
    let mut slot = 0;
    while slot < ALERT_LIST_ROW_TOUCH.len() {
        if ALERT_LIST_MUTE_TOUCH[slot].contains(x, y) {
            return Some(AlertPreviewTouchTarget::Mute(top + slot));
        }
        if ALERT_LIST_ROW_TOUCH[slot].contains(x, y) {
            return Some(AlertPreviewTouchTarget::Row(top + slot));
        }
        slot += 1;
    }
    None
}

pub const fn alert_detail_hit_test(x: u16, y: u16) -> Option<AlertDetailTouchTarget> {
    if ALERT_DETAIL_TOP_BACK_TOUCH.contains(x, y) {
        Some(AlertDetailTouchTarget::Back)
    } else if ALERT_DETAIL_MUTE_TOUCH.contains(x, y) {
        Some(AlertDetailTouchTarget::Mute)
    } else if ALERT_DETAIL_ACTION_TOUCH.contains(x, y) {
        Some(AlertDetailTouchTarget::Mute)
    } else {
        None
    }
}

impl AlertPreviewItem {
    pub const fn active(
        kind: AlertPreviewKind,
        severity: AlertPreviewSeverity,
        sound: AlertPreviewSoundState,
    ) -> Self {
        Self::active_with_instance_id(kind, 1, severity, sound)
    }

    pub const fn active_with_instance_id(
        kind: AlertPreviewKind,
        instance_id: u32,
        severity: AlertPreviewSeverity,
        sound: AlertPreviewSoundState,
    ) -> Self {
        Self {
            kind,
            instance_id,
            severity,
            sound,
            cleared: false,
        }
    }

    pub const fn cleared(kind: AlertPreviewKind) -> Self {
        Self {
            kind,
            instance_id: 0,
            severity: kind.default_severity(),
            sound: AlertPreviewSoundState::Muted,
            cleared: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DashboardHomeFocus {
    Output,
    Thermal,
    Cells,
    Charger,
    BatteryFlow,
}

impl DashboardHomeFocus {
    pub const ALL: [Self; 5] = [
        Self::Output,
        Self::Thermal,
        Self::Cells,
        Self::Charger,
        Self::BatteryFlow,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Output => "OUTPUT",
            Self::Thermal => "THERMAL",
            Self::Cells => "BATTERY",
            Self::Charger => "CHARGE",
            Self::BatteryFlow => "DISCHG",
        }
    }

    pub const fn up(self) -> Self {
        match self {
            Self::Output | Self::Cells => self,
            Self::Thermal => Self::Output,
            Self::Charger => Self::Cells,
            Self::BatteryFlow => Self::Charger,
        }
    }

    pub const fn down(self) -> Self {
        match self {
            Self::Output => Self::Thermal,
            Self::Thermal | Self::BatteryFlow => self,
            Self::Cells => Self::Charger,
            Self::Charger => Self::BatteryFlow,
        }
    }

    pub const fn left(self) -> Self {
        match self {
            Self::Output | Self::Thermal => self,
            Self::Cells => Self::Output,
            Self::Charger | Self::BatteryFlow => Self::Thermal,
        }
    }

    pub const fn right(self) -> Self {
        match self {
            Self::Output => Self::Cells,
            Self::Thermal => Self::BatteryFlow,
            Self::Cells | Self::BatteryFlow => self,
            Self::Charger => Self::BatteryFlow,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DashboardPrimaryPage {
    DashboardHome,
    Menu,
    BeeperSettings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuItem {
    Dashboard,
    Beeper,
}

impl MenuItem {
    pub const ALL: [Self; 2] = [Self::Dashboard, Self::Beeper];

    pub const fn index(self) -> usize {
        match self {
            Self::Dashboard => 0,
            Self::Beeper => 1,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Dashboard => "DASHBOARD",
            Self::Beeper => "AUDIO",
        }
    }

    pub const fn previous(self) -> Self {
        match self {
            Self::Dashboard => Self::Beeper,
            Self::Beeper => Self::Dashboard,
        }
    }

    pub const fn next(self) -> Self {
        match self {
            Self::Dashboard => Self::Beeper,
            Self::Beeper => Self::Dashboard,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DashboardMenuStyle {
    DenseBadge,
    DockBar,
    SplitRail,
    SignalPlate,
}

impl DashboardMenuStyle {
    pub const fn default_preview() -> Self {
        Self::DenseBadge
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuRailItem {
    Dashboard,
    Add,
    Audio,
    Settings,
    Stats,
}

impl MenuRailItem {
    const ALL: [Self; 5] = [
        Self::Dashboard,
        Self::Add,
        Self::Audio,
        Self::Settings,
        Self::Stats,
    ];

    const fn index(self) -> usize {
        match self {
            Self::Dashboard => 0,
            Self::Add => 1,
            Self::Audio => 2,
            Self::Settings => 3,
            Self::Stats => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BeeperVolumeLevel {
    Off,
    L1,
    L2,
    L3,
    L4,
    L5,
    L6,
}

impl BeeperVolumeLevel {
    pub const ALL: [Self; 7] = [
        Self::Off,
        Self::L1,
        Self::L2,
        Self::L3,
        Self::L4,
        Self::L5,
        Self::L6,
    ];

    pub const fn scale_label(self) -> &'static str {
        match self {
            Self::Off => "0",
            Self::L1 => "1",
            Self::L2 => "2",
            Self::L3 => "3",
            Self::L4 => "4",
            Self::L5 => "5",
            Self::L6 => "6",
        }
    }

    pub const fn badge_label(self) -> &'static str {
        match self {
            Self::Off => "OFF",
            Self::L1 => "1",
            Self::L2 => "2",
            Self::L3 => "3",
            Self::L4 => "4",
            Self::L5 => "5",
            Self::L6 => "6",
        }
    }

    pub const fn step(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::L1 => 1,
            Self::L2 => 2,
            Self::L3 => 3,
            Self::L4 => 4,
            Self::L5 => 5,
            Self::L6 => 6,
        }
    }

    pub const fn from_step(step: u8) -> Self {
        match step {
            0 => Self::Off,
            1 => Self::L1,
            2 => Self::L2,
            3 => Self::L3,
            4 => Self::L4,
            5 => Self::L5,
            _ => Self::L6,
        }
    }

    pub const fn decrease(self) -> Self {
        Self::from_step(self.step().saturating_sub(1))
    }

    pub const fn increase(self) -> Self {
        Self::from_step(self.step().saturating_add(1))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BeeperSettingTarget {
    Action,
    System,
}

impl BeeperSettingTarget {
    pub const ALL: [Self; 2] = [Self::Action, Self::System];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Action => "ACTION",
            Self::System => "SYSTEM",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BeeperPrefs {
    pub action_volume: BeeperVolumeLevel,
    pub system_volume: BeeperVolumeLevel,
    pub selected_target: BeeperSettingTarget,
}

impl BeeperPrefs {
    pub const fn defaults() -> Self {
        Self {
            action_volume: BeeperVolumeLevel::L4,
            system_volume: BeeperVolumeLevel::L4,
            selected_target: BeeperSettingTarget::Action,
        }
    }

    pub const fn new(
        action_volume: BeeperVolumeLevel,
        system_volume: BeeperVolumeLevel,
        selected_target: BeeperSettingTarget,
    ) -> Self {
        Self {
            action_volume,
            system_volume,
            selected_target,
        }
    }

    pub const fn volume_for(self, target: BeeperSettingTarget) -> BeeperVolumeLevel {
        match target {
            BeeperSettingTarget::Action => self.action_volume,
            BeeperSettingTarget::System => self.system_volume,
        }
    }

    pub const fn selected_volume(self) -> BeeperVolumeLevel {
        self.volume_for(self.selected_target)
    }

    pub const fn with_selected_target(self, target: BeeperSettingTarget) -> Self {
        Self {
            selected_target: target,
            ..self
        }
    }

    pub const fn with_volume(self, target: BeeperSettingTarget, level: BeeperVolumeLevel) -> Self {
        match target {
            BeeperSettingTarget::Action => Self {
                action_volume: level,
                ..self
            },
            BeeperSettingTarget::System => Self {
                system_volume: level,
                ..self
            },
        }
    }

    pub const fn with_selected_volume(self, level: BeeperVolumeLevel) -> Self {
        self.with_volume(self.selected_target, level)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DashboardShellState {
    pub page: DashboardPrimaryPage,
    pub dashboard_route: DashboardRoute,
    pub home_focus: DashboardHomeFocus,
    pub menu_selected: MenuItem,
    pub menu_style: DashboardMenuStyle,
    pub beeper_prefs: BeeperPrefs,
    pub dashboard_menu_offset_y: i16,
}

impl DashboardShellState {
    pub const fn defaults() -> Self {
        Self {
            page: DashboardPrimaryPage::DashboardHome,
            dashboard_route: DashboardRoute::Home,
            home_focus: DashboardHomeFocus::Output,
            menu_selected: MenuItem::Dashboard,
            menu_style: DashboardMenuStyle::default_preview(),
            beeper_prefs: BeeperPrefs::defaults(),
            dashboard_menu_offset_y: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DashboardTouchTarget {
    HomeWifi,
    HomeOutput,
    HomeThermal,
    HomeCells,
    HomeCharger,
    HomeBatteryFlow,
    DetailBack,
    CellsAdvancedEntry,
    CellsAdvancedBack,
    ChargerManualEntry,
    ManualBack,
    ManualTarget3V7,
    ManualTarget80,
    ManualTarget100,
    ManualSpeed100,
    ManualSpeed500,
    ManualSpeed1A,
    ManualTimer1h,
    ManualTimer2h,
    ManualTimer6h,
    ManualStart,
    ManualStop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DashboardMenuTouchTarget {
    Previous,
    Next,
    Dashboard,
    Beeper,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BeeperSettingsTouchTarget {
    Back,
    Target(BeeperSettingTarget),
    Volume {
        target: BeeperSettingTarget,
        level: BeeperVolumeLevel,
    },
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DashboardInputSource {
    DcIn,
    UsbC,
    Auto,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DashboardChargerProtocol {
    DcIn,
    Pps,
    PdFixed,
    Usb5V,
    NoCc,
    SourceCapsUnknown,
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
pub enum BmsRecoveryUiAction {
    Activation,
    DischargeAuthorization,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelfCheckHardwareTarget {
    Gc9307,
    Tca6408a,
    Fusb302,
    Ina3221,
    Bq25792,
    Bq40z50,
    TpsA,
    TpsB,
    TmpA,
    TmpB,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DashboardDetailSnapshot {
    pub cell_mv: [Option<u16>; 4],
    pub cell_temp_c: [Option<i16>; 4],
    pub balance_enabled: Option<bool>,
    pub balance_cfg_match: Option<bool>,
    pub balance_active: Option<bool>,
    pub balance_mask: Option<u8>,
    pub balance_cell: Option<u8>,
    pub balance_min_start_delta_mv: Option<u8>,
    pub remcap_mah: Option<u16>,
    pub fcc_mah: Option<u16>,
    pub battery_energy_mwh: Option<u32>,
    pub battery_full_capacity_mwh: Option<u32>,
    pub charge_ready: Option<bool>,
    pub discharge_ready: Option<bool>,
    pub xchg: Option<bool>,
    pub xdsg: Option<bool>,
    pub charge_fet_on: Option<bool>,
    pub discharge_fet_on: Option<bool>,
    pub precharge_fet_on: Option<bool>,
    pub learn_qen: Option<bool>,
    pub learn_vok: Option<bool>,
    pub learn_rest: Option<bool>,
    pub fc: Option<bool>,
    pub fd: Option<bool>,
    pub pf: Option<bool>,
    pub rca_alarm: Option<bool>,
    pub reason_key: Option<&'static str>,
    pub reason_label: Option<&'static str>,
    pub input_source: Option<DashboardInputSource>,
    pub input_pressure_state: Option<&'static str>,
    pub input_pressure_score_pct: Option<u8>,
    pub input_pressure_reason: Option<&'static str>,
    pub input_vin_baseline_mv: Option<u16>,
    pub input_vin_drop_mv: Option<u16>,
    pub input_gate_state: Option<&'static str>,
    pub input_gate_reason: Option<&'static str>,
    pub input_power_good: Option<bool>,
    pub assist_power_stage: Option<&'static str>,
    pub assist_target_vout_mv: Option<u16>,
    pub backup_reason: Option<&'static str>,
    pub input_tps_total_iout_ma: Option<i32>,
    pub input_tps_limit_threshold_ma: Option<i32>,
    pub charger_protocol: Option<DashboardChargerProtocol>,
    pub charger_active: Option<bool>,
    pub charger_home_status: Option<&'static str>,
    pub charger_status: Option<&'static str>,
    pub charger_detail_status: Option<&'static str>,
    pub charger_policy_target_ichg_ma: Option<u16>,
    pub charger_limit_active: Option<bool>,
    pub charger_limit_reason: Option<&'static str>,
    pub charger_limit_detail: Option<&'static str>,
    pub charger_limit_threshold_ma: Option<i32>,
    pub manual_charge: ManualChargeUiSnapshot,
    pub out_a_temp_c: Option<i16>,
    pub out_b_temp_c: Option<i16>,
    pub board_temp_c: Option<i16>,
    pub battery_temp_c: Option<i16>,
    pub fan_rpm: Option<u16>,
    pub fan_pwm_pct: Option<u8>,
    pub fan_status: Option<&'static str>,
    pub cells_notice: Option<&'static str>,
    pub battery_notice: Option<&'static str>,
    pub bms_notice: Option<&'static str>,
    pub output_notice: Option<&'static str>,
    pub charger_notice: Option<&'static str>,
    pub thermal_notice: Option<&'static str>,
    pub wifi: WifiSnapshot,
}

impl DashboardDetailSnapshot {
    pub const fn pending() -> Self {
        Self {
            cell_mv: [None, None, None, None],
            cell_temp_c: [None, None, None, None],
            balance_enabled: None,
            balance_cfg_match: None,
            balance_active: None,
            balance_mask: None,
            balance_cell: None,
            balance_min_start_delta_mv: None,
            remcap_mah: None,
            fcc_mah: None,
            battery_energy_mwh: None,
            battery_full_capacity_mwh: None,
            charge_ready: None,
            discharge_ready: None,
            xchg: None,
            xdsg: None,
            charge_fet_on: None,
            discharge_fet_on: None,
            precharge_fet_on: None,
            learn_qen: None,
            learn_vok: None,
            learn_rest: None,
            fc: None,
            fd: None,
            pf: None,
            rca_alarm: None,
            reason_key: None,
            reason_label: None,
            input_source: None,
            input_pressure_state: None,
            input_pressure_score_pct: None,
            input_pressure_reason: None,
            input_vin_baseline_mv: None,
            input_vin_drop_mv: None,
            input_gate_state: None,
            input_gate_reason: None,
            input_power_good: None,
            assist_power_stage: None,
            assist_target_vout_mv: None,
            backup_reason: None,
            input_tps_total_iout_ma: None,
            input_tps_limit_threshold_ma: None,
            charger_protocol: None,
            charger_active: None,
            charger_home_status: None,
            charger_status: None,
            charger_detail_status: None,
            charger_policy_target_ichg_ma: None,
            charger_limit_active: None,
            charger_limit_reason: None,
            charger_limit_detail: None,
            charger_limit_threshold_ma: None,
            manual_charge: ManualChargeUiSnapshot::pending(),
            out_a_temp_c: None,
            out_b_temp_c: None,
            board_temp_c: None,
            battery_temp_c: None,
            fan_rpm: None,
            fan_pwm_pct: None,
            fan_status: None,
            cells_notice: None,
            battery_notice: None,
            bms_notice: None,
            output_notice: None,
            charger_notice: None,
            thermal_notice: None,
            wifi: WifiSnapshot::disabled(),
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
    pub dcin_present: Option<bool>,
    pub aggregate_input_present: Option<bool>,
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
    pub bq25792_ibat_ma: Option<i16>,
    pub bq25792_vbat_present: Option<bool>,
    pub bq25792_vsys_mv: Option<u16>,
    pub bq40z50: SelfCheckCommState,
    pub bq40z50_pack_mv: Option<u16>,
    pub bq40z50_current_ma: Option<i16>,
    pub bq40z50_soc_pct: Option<u16>,
    pub bq40z50_rca_alarm: Option<bool>,
    pub bq40z50_no_battery: Option<bool>,
    pub bq40z50_discharge_ready: Option<bool>,
    pub bq40z50_issue_detail: Option<&'static str>,
    pub bq40z50_recovery_action: Option<BmsRecoveryUiAction>,
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
            dcin_present: None,
            aggregate_input_present: None,
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
            bq25792_ibat_ma: None,
            bq25792_vbat_present: None,
            bq25792_vsys_mv: None,
            bq40z50: SelfCheckCommState::Pending,
            bq40z50_pack_mv: None,
            bq40z50_current_ma: None,
            bq40z50_soc_pct: None,
            bq40z50_rca_alarm: None,
            bq40z50_no_battery: None,
            bq40z50_discharge_ready: None,
            bq40z50_issue_detail: None,
            bq40z50_recovery_action: None,
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

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpsTestVoutProfile {
    V5,
    V12,
    V15,
    V19,
}

impl TpsTestVoutProfile {
    #[allow(dead_code)]
    pub const fn label(self) -> &'static str {
        match self {
            Self::V5 => "5V",
            Self::V12 => "12V",
            Self::V15 => "15V",
            Self::V19 => "19V",
        }
    }

    #[allow(dead_code)]
    pub const fn target_mv(self) -> u16 {
        match self {
            Self::V5 => 5_000,
            Self::V12 => 12_000,
            Self::V15 => 15_000,
            Self::V19 => 19_000,
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TpsTestChargerSnapshot {
    pub requested_enabled: bool,
    pub actual_enabled: bool,
    pub comm_state: SelfCheckCommState,
    pub input_present: Option<bool>,
    pub vbat_present: Option<bool>,
    pub vbat_mv: Option<u16>,
    pub ibat_ma: Option<i32>,
    pub vreg_mv: Option<u16>,
    pub ichg_ma: Option<u16>,
    pub status: &'static str,
    pub fault: Option<&'static str>,
}

impl TpsTestChargerSnapshot {
    #[allow(dead_code)]
    pub const fn pending() -> Self {
        Self {
            requested_enabled: false,
            actual_enabled: false,
            comm_state: SelfCheckCommState::Pending,
            input_present: None,
            vbat_present: None,
            vbat_mv: None,
            ibat_ma: None,
            vreg_mv: None,
            ichg_ma: None,
            status: "PEND",
            fault: None,
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TpsTestOutputSnapshot {
    pub requested_enabled: bool,
    pub actual_enabled: Option<bool>,
    pub comm_state: SelfCheckCommState,
    pub vset_mv: Option<u16>,
    pub vbus_mv: Option<u16>,
    pub iout_ma: Option<i32>,
    pub temp_c_x16: Option<i16>,
    pub status_bits: Option<u8>,
    pub fault: Option<&'static str>,
}

impl TpsTestOutputSnapshot {
    #[allow(dead_code)]
    pub const fn pending(requested_enabled: bool) -> Self {
        Self {
            requested_enabled,
            actual_enabled: None,
            comm_state: SelfCheckCommState::Pending,
            vset_mv: None,
            vbus_mv: None,
            iout_ma: None,
            temp_c_x16: None,
            status_bits: None,
            fault: None,
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TpsTestUiSnapshot {
    pub build_profile: &'static str,
    pub build_id: &'static str,
    pub vout_profile: TpsTestVoutProfile,
    pub ilim_ma: u16,
    pub charger: TpsTestChargerSnapshot,
    pub out_a: TpsTestOutputSnapshot,
    pub out_b: TpsTestOutputSnapshot,
    pub footer_notice: Option<&'static str>,
    pub footer_alert: Option<&'static str>,
}

impl TpsTestUiSnapshot {
    #[allow(dead_code)]
    pub const fn pending(
        build_profile: &'static str,
        build_id: &'static str,
        vout_profile: TpsTestVoutProfile,
        ilim_ma: u16,
        out_a_enabled: bool,
        out_b_enabled: bool,
    ) -> Self {
        Self {
            build_profile,
            build_id,
            vout_profile,
            ilim_ma,
            charger: TpsTestChargerSnapshot::pending(),
            out_a: TpsTestOutputSnapshot::pending(out_a_enabled),
            out_b: TpsTestOutputSnapshot::pending(out_b_enabled),
            footer_notice: Some("FIXED PROFILE / NO TOUCH CONTROLS"),
            footer_alert: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum SelfCheckTouchTarget {
    HardwareCard(SelfCheckHardwareTarget),
    ActivateCancel,
    ActivateConfirm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum SelfCheckOverlay {
    None,
    ManualChargeLoopbackConfirm,
    BmsActivateConfirm,
    BmsActivateProgress,
    BmsDischargeAuthorizeConfirm,
    BmsDischargeAuthorizeProgress,
    BmsActivateResult(BmsResultKind),
    HardwareIssue(SelfCheckHardwareTarget),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ManualChargeLoopbackConfirmTarget {
    Cancel,
    Confirm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum BmsActivationState {
    Idle,
    Pending,
    Result(BmsResultKind),
}

const SELF_CHECK_LEFT_CARD_X: u16 = 6;
const SELF_CHECK_RIGHT_CARD_X: u16 = 163;
const SELF_CHECK_CARD_Y: u16 = 22;
const SELF_CHECK_CARD_W: u16 = 151;
const SELF_CHECK_CARD_H: u16 = 29;

const SELF_CHECK_DIALOG_X: u16 = 20;
const SELF_CHECK_DIALOG_Y: u16 = 34;
const SELF_CHECK_DIALOG_W: u16 = 280;
const SELF_CHECK_DIALOG_H: u16 = 112;

const SELF_CHECK_CANCEL_BTN_X: u16 = 32;
const SELF_CHECK_CANCEL_BTN_Y: u16 = 116;
const SELF_CHECK_CANCEL_BTN_W: u16 = 108;
const SELF_CHECK_CANCEL_BTN_H: u16 = 24;

const SELF_CHECK_CONFIRM_BTN_X: u16 = 152;
const SELF_CHECK_CONFIRM_BTN_Y: u16 = 116;
const SELF_CHECK_CONFIRM_BTN_W: u16 = 136;
const SELF_CHECK_CONFIRM_BTN_H: u16 = 24;

const DASHBOARD_HOME_OUTPUT_X: u16 = 6;
const DASHBOARD_HOME_OUTPUT_Y: u16 = 22;
const DASHBOARD_HOME_OUTPUT_W: u16 = 196;
const DASHBOARD_HOME_OUTPUT_H: u16 = 52;

const DASHBOARD_HOME_WIFI_ICON_X: u16 = 128;
const DASHBOARD_HOME_WIFI_ICON_Y: u16 = 2;
const DASHBOARD_HOME_WIFI_ICON_W: u16 = 18;
const DASHBOARD_HOME_WIFI_ICON_H: u16 = 14;
const DASHBOARD_HOME_WIFI_TOUCH_X: u16 = 112;
const DASHBOARD_HOME_WIFI_TOUCH_Y: u16 = 0;
const DASHBOARD_HOME_WIFI_TOUCH_W: u16 = 38;
const DASHBOARD_HOME_WIFI_TOUCH_H: u16 = 36;
const DASHBOARD_HOME_ALERT_ICON_X: u16 = 150;
const DASHBOARD_HOME_ALERT_ICON_Y: u16 = 2;
const DASHBOARD_HOME_ALERT_ICON_SIZE: u16 = 16;
const DASHBOARD_HOME_ALERT_TOUCH_X: u16 = 150;
const DASHBOARD_HOME_ALERT_TOUCH_Y: u16 = 0;
const DASHBOARD_HOME_ALERT_TOUCH_W: u16 = 38;
const DASHBOARD_HOME_ALERT_TOUCH_H: u16 = 36;

pub const DASHBOARD_HOME_WIFI_TOUCH: TouchRect = TouchRect::new(
    DASHBOARD_HOME_WIFI_TOUCH_X,
    DASHBOARD_HOME_WIFI_TOUCH_Y,
    DASHBOARD_HOME_WIFI_TOUCH_W,
    DASHBOARD_HOME_WIFI_TOUCH_H,
);
pub const DASHBOARD_HOME_ALERT_TOUCH: TouchRect = TouchRect::new(
    DASHBOARD_HOME_ALERT_TOUCH_X,
    DASHBOARD_HOME_ALERT_TOUCH_Y,
    DASHBOARD_HOME_ALERT_TOUCH_W,
    DASHBOARD_HOME_ALERT_TOUCH_H,
);

pub const ALERT_LIST_ROW_TOUCH: [TouchRect; 3] = [
    TouchRect::new(0, 24, 272, 36),
    TouchRect::new(0, 60, 272, 36),
    TouchRect::new(0, 96, 272, 36),
];
pub const ALERT_LIST_MUTE_TOUCH: [TouchRect; 3] = [
    TouchRect::new(272, 24, 48, 36),
    TouchRect::new(272, 60, 48, 36),
    TouchRect::new(272, 96, 48, 36),
];
pub const ALERT_LIST_TOP_BACK_TOUCH: TouchRect = TouchRect::new(0, 0, 96, 24);
pub const ALERT_DETAIL_TOP_BACK_TOUCH: TouchRect = TouchRect::new(0, 0, 96, 32);
pub const ALERT_DETAIL_MUTE_TOUCH: TouchRect = TouchRect::new(264, 72, 56, 40);
pub const ALERT_DETAIL_ACTION_TOUCH: TouchRect = TouchRect::new(8, 112, 304, 28);

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
const DASHBOARD_HOME_FOCUS_LABEL_H: u16 = 17;

const DASHBOARD_MENU_STACK_H: i16 = (UI_H as i16) * 2;
const DASHBOARD_MENU_HEADER_H: u16 = 30;
const DASHBOARD_MENU_HEADER_CENTER_Y: u16 = DASHBOARD_MENU_HEADER_H / 2;
const DASHBOARD_MENU_ICON_CENTER_X: i16 = (UI_W as i16) / 2;
const DASHBOARD_MENU_ICON_Y: u16 = 46;
const DASHBOARD_MENU_ICON_W: u16 = 52;
const DASHBOARD_MENU_ICON_H: u16 = 52;
const DASHBOARD_MENU_ICON_GAP: u16 = 12;
const DASHBOARD_MENU_GLYPH_INSET: u16 = 5;
const DASHBOARD_MENU_FOOTER_Y: u16 = 124;
const DASHBOARD_MENU_FOOTER_H: u16 = UI_H - DASHBOARD_MENU_FOOTER_Y;
const DASHBOARD_MENU_NAV_HINT_BOX_W: u16 = 36;
const DASHBOARD_MENU_NAV_HINT_BOX_H: u16 = 20;
const DASHBOARD_MENU_FOOTER_CENTER_Y: u16 = DASHBOARD_MENU_FOOTER_Y + (DASHBOARD_MENU_FOOTER_H / 2);
const DASHBOARD_MENU_NAV_HINT_Y: u16 =
    DASHBOARD_MENU_FOOTER_CENTER_Y - (DASHBOARD_MENU_NAV_HINT_BOX_H / 2);
const DASHBOARD_MENU_NAV_HINT_LEFT_X: u16 = 24;
const DASHBOARD_MENU_NAV_HINT_RIGHT_X: u16 =
    UI_W - DASHBOARD_MENU_NAV_HINT_LEFT_X - DASHBOARD_MENU_NAV_HINT_BOX_W;
const DASHBOARD_MENU_FOOTER_ACCENT_W: u16 = 56;
const DASHBOARD_MENU_FOOTER_BADGE_W: u16 = 140;
const DASHBOARD_MENU_FOOTER_BADGE_H: u16 = 24;

const AUDIO_SCALE_Y: u16 = 50;
const AUDIO_ROW_X: u16 = 12;
const AUDIO_ROW_W: u16 = 296;
const AUDIO_ROW_H: u16 = 28;
const AUDIO_ACTION_ROW_Y: u16 = 66;
const AUDIO_SYSTEM_ROW_Y: u16 = 104;
const AUDIO_TRACK_X: u16 = 102;
const AUDIO_TRACK_W: u16 = 146;
const AUDIO_TRACK_H: u16 = 18;
const AUDIO_VOLUME_TOUCH_X: u16 = 76;
const AUDIO_VOLUME_TOUCH_W: u16 = 196;
const AUDIO_VOLUME_TOUCH_Y_INSET: u16 = 6;
const AUDIO_VOLUME_TOUCH_H: u16 = 40;
const AUDIO_ROW_TOUCH_X: u16 = 6;
const AUDIO_ROW_TOUCH_W: u16 = 308;
const AUDIO_ROW_TOUCH_Y_INSET: u16 = 4;
const AUDIO_ROW_TOUCH_H: u16 = 36;
const AUDIO_NODE_SIZE: u16 = 8;
const AUDIO_BADGE_X: u16 = 260;
const AUDIO_BADGE_W: u16 = 36;
const AUDIO_BADGE_H: u16 = 20;

const DASHBOARD_DETAIL_BACK_X: u16 = 8;
const DASHBOARD_DETAIL_BACK_Y: u16 = 2;
const DASHBOARD_DETAIL_BACK_W: u16 = 56;
const DASHBOARD_DETAIL_BACK_H: u16 = 14;
const DASHBOARD_DETAIL_BACK_HIT_X: u16 = 0;
const DASHBOARD_DETAIL_BACK_HIT_Y: u16 = 0;
const DASHBOARD_DETAIL_BACK_HIT_W: u16 = 96;
const DASHBOARD_DETAIL_BACK_HIT_H: u16 = 24;

const DASHBOARD_CELLS_ADVANCED_ENTRY_X: u16 = 6;
const DASHBOARD_CELLS_ADVANCED_ENTRY_Y: u16 = 22;
const DASHBOARD_CELLS_ADVANCED_ENTRY_W: u16 = 308;
const DASHBOARD_CELLS_ADVANCED_ENTRY_H: u16 = 120;

const DASHBOARD_CHARGER_MANUAL_ENTRY_X: u16 = 6;
const DASHBOARD_CHARGER_MANUAL_ENTRY_Y: u16 = 60;
const DASHBOARD_CHARGER_MANUAL_ENTRY_W: u16 = 150;
const DASHBOARD_CHARGER_MANUAL_ENTRY_H: u16 = 82;

const MANUAL_ROW_X: u16 = 6;
const MANUAL_ROW_W: u16 = 308;
const MANUAL_ROW_H: u16 = 30;
const MANUAL_TARGET_ROW_Y: u16 = 24;
const MANUAL_SPEED_ROW_Y: u16 = 58;
const MANUAL_TIMER_ROW_Y: u16 = 92;
const MANUAL_SEGMENT_X: u16 = 76;
const MANUAL_SEGMENT_W: u16 = 74;
const MANUAL_SEGMENT_H: u16 = 24;
const MANUAL_SEGMENT_GAP: u16 = 4;
const MANUAL_SEGMENT_Y_INSET: u16 = 3;
const MANUAL_BACK_X: u16 = 6;
const MANUAL_BACK_Y: u16 = 132;
const MANUAL_BACK_W: u16 = 88;
const MANUAL_BACK_H: u16 = 30;
const MANUAL_BACK_HIT_X: u16 = 0;
const MANUAL_BACK_HIT_Y: u16 = 126;
const MANUAL_BACK_HIT_W: u16 = 112;
const MANUAL_BACK_HIT_H: u16 = UI_H - MANUAL_BACK_HIT_Y;
const MANUAL_STATUS_X: u16 = 100;
const MANUAL_STATUS_Y: u16 = 132;
const MANUAL_STATUS_W: u16 = 120;
const MANUAL_STATUS_H: u16 = 30;
const MANUAL_ACTION_X: u16 = 226;
const MANUAL_ACTION_Y: u16 = 132;
const MANUAL_ACTION_W: u16 = 88;
const MANUAL_ACTION_H: u16 = 30;

const fn manual_segment_x(idx: u16) -> u16 {
    MANUAL_SEGMENT_X + idx * (MANUAL_SEGMENT_W + MANUAL_SEGMENT_GAP)
}

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
    is_bq40_offline(snapshot)
}

fn discharge_authorization_input_ready(snapshot: &SelfCheckUiSnapshot) -> bool {
    snapshot.fusb302_vbus_present == Some(true) || snapshot_mains_present(snapshot)
}

#[allow(dead_code)]
pub fn bq40_recovery_action(snapshot: &SelfCheckUiSnapshot) -> Option<BmsRecoveryUiAction> {
    if is_bq40_activation_needed(snapshot) {
        Some(BmsRecoveryUiAction::Activation)
    } else if snapshot.requested_outputs != EnabledOutputs::None
        && snapshot.output_gate_reason == OutputGateReason::BmsNotReady
        && snapshot.bq40z50 != SelfCheckCommState::Err
        && snapshot.bq40z50_no_battery != Some(true)
        && snapshot.bq40z50_rca_alarm != Some(true)
        && snapshot.bq40z50_issue_detail != Some("cell_undervoltage")
        && snapshot.bq40z50_discharge_ready == Some(false)
        && discharge_authorization_input_ready(snapshot)
        && snapshot.bq25792 != SelfCheckCommState::Err
    {
        Some(BmsRecoveryUiAction::DischargeAuthorization)
    } else {
        None
    }
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

fn tps_upstream_warning_reason(snapshot: &SelfCheckUiSnapshot) -> Option<&'static str> {
    if snapshot.bq40z50_discharge_ready == Some(false)
        || snapshot.output_gate_reason == OutputGateReason::BmsNotReady
    {
        Some("WAIT BMS")
    } else if snapshot.bq25792_vbat_present != Some(true) {
        Some("VBAT UNK")
    } else {
        None
    }
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

fn display_tps_state(
    snapshot: &SelfCheckUiSnapshot,
    selector: OutputSelector,
) -> SelfCheckCommState {
    match snapshot_tps_state(snapshot, selector) {
        SelfCheckCommState::Err if tps_upstream_warning_reason(snapshot).is_some() => {
            SelfCheckCommState::Warn
        }
        state => state,
    }
}

fn self_check_tps_summary_name(
    snapshot: &SelfCheckUiSnapshot,
    selector: OutputSelector,
) -> &'static str {
    match display_tps_state(snapshot, selector) {
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
    self_check_dashboard_block_reason(snapshot).is_none()
}

pub fn self_check_dashboard_block_reason(snapshot: &SelfCheckUiSnapshot) -> Option<&'static str> {
    fn state_ok(state: SelfCheckCommState) -> bool {
        matches!(
            state,
            SelfCheckCommState::Ok | SelfCheckCommState::NotAvailable
        )
    }

    if !state_ok(snapshot.gc9307) {
        return Some("gc9307");
    }
    if !state_ok(snapshot.tca6408a) {
        return Some("tca6408a");
    }
    if !state_ok(snapshot.fusb302) {
        return Some("fusb302");
    }
    if !state_ok(snapshot.ina3221) {
        return Some("ina3221");
    }
    if !state_ok(snapshot.bq25792) {
        return Some("bq25792");
    }
    if !bq40_dashboard_clear(snapshot) {
        return Some("bq40z50");
    }
    if snapshot.output_gate_reason != OutputGateReason::None {
        return Some(snapshot.output_gate_reason.as_str());
    }
    if outputs_include(snapshot, OutputSelector::OutA)
        && (!state_ok(snapshot.tps_a)
            || output_hold_for(snapshot, OutputSelector::OutA)
            || !active_outputs_include(snapshot, OutputSelector::OutA))
    {
        return Some("out_a");
    }
    if outputs_include(snapshot, OutputSelector::OutB)
        && (!state_ok(snapshot.tps_b)
            || output_hold_for(snapshot, OutputSelector::OutB)
            || !active_outputs_include(snapshot, OutputSelector::OutB))
    {
        return Some("out_b");
    }
    if !state_ok(snapshot.tmp_a) {
        return Some("tmp_a");
    }
    if !state_ok(snapshot.tmp_b) {
        return Some("tmp_b");
    }
    None
}

fn bq40_dashboard_clear(snapshot: &SelfCheckUiSnapshot) -> bool {
    if snapshot.bq40z50_no_battery == Some(true)
        || snapshot.bq40z50_discharge_ready == Some(false)
        || snapshot.bq40z50_recovery_pending
    {
        return false;
    }

    match snapshot.bq40z50 {
        SelfCheckCommState::Ok => true,
        SelfCheckCommState::Warn => matches!(
            snapshot.bq40z50_issue_detail,
            Some("xchg_blocked" | "chg_fet_off")
        ),
        SelfCheckCommState::Pending
        | SelfCheckCommState::Err
        | SelfCheckCommState::NotAvailable => false,
    }
}

#[allow(dead_code)]
pub fn bq40_result_overlay(snapshot: &SelfCheckUiSnapshot) -> Option<SelfCheckOverlay> {
    snapshot
        .bq40z50_last_result
        .filter(|result| *result != BmsResultKind::Success)
        .map(SelfCheckOverlay::BmsActivateResult)
}

#[allow(dead_code)]
pub fn bq40_recovery_overlay(snapshot: &SelfCheckUiSnapshot) -> Option<SelfCheckOverlay> {
    match snapshot.bq40z50_recovery_action {
        Some(BmsRecoveryUiAction::Activation) => Some(SelfCheckOverlay::BmsActivateConfirm),
        Some(BmsRecoveryUiAction::DischargeAuthorization) => {
            Some(SelfCheckOverlay::BmsDischargeAuthorizeConfirm)
        }
        None => None,
    }
}

#[allow(dead_code)]
pub fn self_check_hardware_issue_overlay(
    snapshot: &SelfCheckUiSnapshot,
    target: SelfCheckHardwareTarget,
) -> Option<SelfCheckOverlay> {
    if self_check_hardware_target_has_issue(snapshot, target) {
        Some(SelfCheckOverlay::HardwareIssue(target))
    } else {
        None
    }
}

fn self_check_hardware_target_at(x: u16, y: u16) -> Option<SelfCheckHardwareTarget> {
    const LEFT: [SelfCheckHardwareTarget; 5] = [
        SelfCheckHardwareTarget::Gc9307,
        SelfCheckHardwareTarget::Tca6408a,
        SelfCheckHardwareTarget::Fusb302,
        SelfCheckHardwareTarget::Ina3221,
        SelfCheckHardwareTarget::Bq25792,
    ];
    const RIGHT: [SelfCheckHardwareTarget; 5] = [
        SelfCheckHardwareTarget::Bq40z50,
        SelfCheckHardwareTarget::TpsA,
        SelfCheckHardwareTarget::TpsB,
        SelfCheckHardwareTarget::TmpA,
        SelfCheckHardwareTarget::TmpB,
    ];

    let target_from_column = |col_x, targets: [SelfCheckHardwareTarget; 5]| {
        if !contains(
            x,
            y,
            col_x,
            SELF_CHECK_CARD_Y,
            SELF_CHECK_CARD_W,
            SELF_CHECK_CARD_H * 5,
        ) {
            return None;
        }
        let row = ((y - SELF_CHECK_CARD_Y) / SELF_CHECK_CARD_H) as usize;
        targets.get(row).copied()
    };

    target_from_column(SELF_CHECK_LEFT_CARD_X, LEFT)
        .or_else(|| target_from_column(SELF_CHECK_RIGHT_CARD_X, RIGHT))
}

fn self_check_state_has_issue(state: SelfCheckCommState) -> bool {
    !matches!(
        state,
        SelfCheckCommState::Ok | SelfCheckCommState::NotAvailable
    )
}

fn self_check_hardware_target_has_issue(
    snapshot: &SelfCheckUiSnapshot,
    target: SelfCheckHardwareTarget,
) -> bool {
    match target {
        SelfCheckHardwareTarget::Gc9307 => self_check_state_has_issue(snapshot.gc9307),
        SelfCheckHardwareTarget::Tca6408a => self_check_state_has_issue(snapshot.tca6408a),
        SelfCheckHardwareTarget::Fusb302 => {
            self_check_state_has_issue(snapshot.fusb302)
                || snapshot.fusb302_vbus_present == Some(false)
        }
        SelfCheckHardwareTarget::Ina3221 => self_check_state_has_issue(snapshot.ina3221),
        SelfCheckHardwareTarget::Bq25792 => self_check_state_has_issue(snapshot.bq25792),
        SelfCheckHardwareTarget::Bq40z50 => {
            self_check_state_has_issue(snapshot.bq40z50)
                || snapshot.bq40z50_issue_detail.is_some()
                || snapshot.bq40z50_no_battery == Some(true)
                || snapshot.bq40z50_discharge_ready == Some(false)
                || snapshot.bq40z50_rca_alarm == Some(true)
        }
        SelfCheckHardwareTarget::TpsA => {
            self_check_state_has_issue(display_tps_state(snapshot, OutputSelector::OutA))
                || output_hold_for(snapshot, OutputSelector::OutA)
        }
        SelfCheckHardwareTarget::TpsB => {
            self_check_state_has_issue(display_tps_state(snapshot, OutputSelector::OutB))
                || output_hold_for(snapshot, OutputSelector::OutB)
        }
        SelfCheckHardwareTarget::TmpA => self_check_state_has_issue(snapshot.tmp_a),
        SelfCheckHardwareTarget::TmpB => self_check_state_has_issue(snapshot.tmp_b),
    }
}

#[allow(dead_code)]
pub fn self_check_hit_test(
    x: u16,
    y: u16,
    overlay: SelfCheckOverlay,
) -> Option<SelfCheckTouchTarget> {
    match overlay {
        SelfCheckOverlay::None => {
            self_check_hardware_target_at(x, y).map(SelfCheckTouchTarget::HardwareCard)
        }
        SelfCheckOverlay::ManualChargeLoopbackConfirm => None,
        SelfCheckOverlay::BmsActivateConfirm | SelfCheckOverlay::BmsDischargeAuthorizeConfirm => {
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
        SelfCheckOverlay::BmsActivateProgress
        | SelfCheckOverlay::BmsDischargeAuthorizeProgress
        | SelfCheckOverlay::BmsActivateResult(..)
        | SelfCheckOverlay::HardwareIssue(..) => None,
    }
}

#[allow(dead_code)]
pub fn manual_charge_loopback_confirm_hit_test(
    x: u16,
    y: u16,
) -> Option<ManualChargeLoopbackConfirmTarget> {
    if contains(
        x,
        y,
        SELF_CHECK_CANCEL_BTN_X,
        SELF_CHECK_CANCEL_BTN_Y,
        SELF_CHECK_CANCEL_BTN_W,
        SELF_CHECK_CANCEL_BTN_H,
    ) {
        Some(ManualChargeLoopbackConfirmTarget::Cancel)
    } else if contains(
        x,
        y,
        SELF_CHECK_CONFIRM_BTN_X,
        SELF_CHECK_CONFIRM_BTN_Y,
        SELF_CHECK_CONFIRM_BTN_W,
        SELF_CHECK_CONFIRM_BTN_H,
    ) {
        Some(ManualChargeLoopbackConfirmTarget::Confirm)
    } else {
        None
    }
}

pub const fn manual_charge_loopback_confirm_key_target(
    left: bool,
    right: bool,
    center: bool,
) -> Option<ManualChargeLoopbackConfirmTarget> {
    if left {
        Some(ManualChargeLoopbackConfirmTarget::Cancel)
    } else if right || center {
        Some(ManualChargeLoopbackConfirmTarget::Confirm)
    } else {
        None
    }
}

#[allow(dead_code)]
pub fn dashboard_hit_test(route: DashboardRoute, x: u16, y: u16) -> Option<DashboardTouchTarget> {
    match route {
        DashboardRoute::Home => {
            if DASHBOARD_HOME_WIFI_TOUCH.contains(x, y) {
                Some(DashboardTouchTarget::HomeWifi)
            } else if contains(
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
                DASHBOARD_DETAIL_BACK_HIT_X,
                DASHBOARD_DETAIL_BACK_HIT_Y,
                DASHBOARD_DETAIL_BACK_HIT_W,
                DASHBOARD_DETAIL_BACK_HIT_H,
            ) {
                if matches!(
                    route,
                    DashboardRoute::Detail(DashboardDetailPage::BmsDetail)
                ) {
                    Some(DashboardTouchTarget::CellsAdvancedBack)
                } else {
                    Some(DashboardTouchTarget::DetailBack)
                }
            } else if matches!(route, DashboardRoute::Detail(DashboardDetailPage::Cells))
                && contains(
                    x,
                    y,
                    DASHBOARD_CELLS_ADVANCED_ENTRY_X,
                    DASHBOARD_CELLS_ADVANCED_ENTRY_Y,
                    DASHBOARD_CELLS_ADVANCED_ENTRY_W,
                    DASHBOARD_CELLS_ADVANCED_ENTRY_H,
                )
            {
                Some(DashboardTouchTarget::CellsAdvancedEntry)
            } else if matches!(route, DashboardRoute::Detail(DashboardDetailPage::Charger))
                && contains(
                    x,
                    y,
                    DASHBOARD_CHARGER_MANUAL_ENTRY_X,
                    DASHBOARD_CHARGER_MANUAL_ENTRY_Y,
                    DASHBOARD_CHARGER_MANUAL_ENTRY_W,
                    DASHBOARD_CHARGER_MANUAL_ENTRY_H,
                )
            {
                Some(DashboardTouchTarget::ChargerManualEntry)
            } else {
                None
            }
        }
        DashboardRoute::ManualCharge => {
            if contains(
                x,
                y,
                MANUAL_BACK_HIT_X,
                MANUAL_BACK_HIT_Y,
                MANUAL_BACK_HIT_W,
                MANUAL_BACK_HIT_H,
            ) || contains(
                x,
                y,
                DASHBOARD_DETAIL_BACK_HIT_X,
                DASHBOARD_DETAIL_BACK_HIT_Y,
                DASHBOARD_DETAIL_BACK_HIT_W,
                DASHBOARD_DETAIL_BACK_HIT_H,
            ) {
                Some(DashboardTouchTarget::ManualBack)
            } else if contains(
                x,
                y,
                MANUAL_ACTION_X,
                MANUAL_ACTION_Y,
                MANUAL_ACTION_W,
                MANUAL_ACTION_H,
            ) {
                Some(DashboardTouchTarget::ManualStart)
            } else if contains(
                x,
                y,
                manual_segment_x(0),
                MANUAL_TARGET_ROW_Y + MANUAL_SEGMENT_Y_INSET,
                MANUAL_SEGMENT_W,
                MANUAL_SEGMENT_H,
            ) {
                Some(DashboardTouchTarget::ManualTarget3V7)
            } else if contains(
                x,
                y,
                manual_segment_x(1),
                MANUAL_TARGET_ROW_Y + MANUAL_SEGMENT_Y_INSET,
                MANUAL_SEGMENT_W,
                MANUAL_SEGMENT_H,
            ) {
                Some(DashboardTouchTarget::ManualTarget80)
            } else if contains(
                x,
                y,
                manual_segment_x(2),
                MANUAL_TARGET_ROW_Y + MANUAL_SEGMENT_Y_INSET,
                MANUAL_SEGMENT_W,
                MANUAL_SEGMENT_H,
            ) {
                Some(DashboardTouchTarget::ManualTarget100)
            } else if contains(
                x,
                y,
                manual_segment_x(0),
                MANUAL_SPEED_ROW_Y + MANUAL_SEGMENT_Y_INSET,
                MANUAL_SEGMENT_W,
                MANUAL_SEGMENT_H,
            ) {
                Some(DashboardTouchTarget::ManualSpeed100)
            } else if contains(
                x,
                y,
                manual_segment_x(1),
                MANUAL_SPEED_ROW_Y + MANUAL_SEGMENT_Y_INSET,
                MANUAL_SEGMENT_W,
                MANUAL_SEGMENT_H,
            ) {
                Some(DashboardTouchTarget::ManualSpeed500)
            } else if contains(
                x,
                y,
                manual_segment_x(2),
                MANUAL_SPEED_ROW_Y + MANUAL_SEGMENT_Y_INSET,
                MANUAL_SEGMENT_W,
                MANUAL_SEGMENT_H,
            ) {
                Some(DashboardTouchTarget::ManualSpeed1A)
            } else if contains(
                x,
                y,
                manual_segment_x(0),
                MANUAL_TIMER_ROW_Y + MANUAL_SEGMENT_Y_INSET,
                MANUAL_SEGMENT_W,
                MANUAL_SEGMENT_H,
            ) {
                Some(DashboardTouchTarget::ManualTimer1h)
            } else if contains(
                x,
                y,
                manual_segment_x(1),
                MANUAL_TIMER_ROW_Y + MANUAL_SEGMENT_Y_INSET,
                MANUAL_SEGMENT_W,
                MANUAL_SEGMENT_H,
            ) {
                Some(DashboardTouchTarget::ManualTimer2h)
            } else if contains(
                x,
                y,
                manual_segment_x(2),
                MANUAL_TIMER_ROW_Y + MANUAL_SEGMENT_Y_INSET,
                MANUAL_SEGMENT_W,
                MANUAL_SEGMENT_H,
            ) {
                Some(DashboardTouchTarget::ManualTimer6h)
            } else {
                None
            }
        }
    }
}

#[allow(dead_code)]
pub fn dashboard_menu_hit_test(
    selected: MenuItem,
    x: u16,
    y: u16,
) -> Option<DashboardMenuTouchTarget> {
    if contains(
        x,
        y,
        DASHBOARD_MENU_NAV_HINT_LEFT_X,
        DASHBOARD_MENU_NAV_HINT_Y,
        DASHBOARD_MENU_NAV_HINT_BOX_W,
        DASHBOARD_MENU_NAV_HINT_BOX_H,
    ) {
        return Some(DashboardMenuTouchTarget::Previous);
    }
    if contains(
        x,
        y,
        DASHBOARD_MENU_NAV_HINT_RIGHT_X,
        DASHBOARD_MENU_NAV_HINT_Y,
        DASHBOARD_MENU_NAV_HINT_BOX_W,
        DASHBOARD_MENU_NAV_HINT_BOX_H,
    ) {
        return Some(DashboardMenuTouchTarget::Next);
    }

    let rail_origin_x = dashboard_menu_rail_origin_x(selected);
    let step: i16 = (DASHBOARD_MENU_ICON_W + DASHBOARD_MENU_ICON_GAP) as i16;
    let icon_y = DASHBOARD_MENU_ICON_Y + 4;
    for item in MenuRailItem::ALL {
        let icon_x = rail_origin_x + (item.index() as i16 * step);
        if icon_x < 0 || icon_x + DASHBOARD_MENU_ICON_W as i16 > UI_W as i16 {
            continue;
        }
        if !contains(
            x,
            y,
            icon_x as u16,
            icon_y,
            DASHBOARD_MENU_ICON_W,
            DASHBOARD_MENU_ICON_H,
        ) {
            continue;
        }

        return match menu_item_for_rail_item(item) {
            Some(MenuItem::Dashboard) => Some(DashboardMenuTouchTarget::Dashboard),
            Some(MenuItem::Beeper) => Some(DashboardMenuTouchTarget::Beeper),
            None => None,
        };
    }

    None
}

#[allow(dead_code)]
pub fn beeper_settings_hit_test(x: u16, y: u16) -> Option<BeeperSettingsTouchTarget> {
    if contains(
        x,
        y,
        DASHBOARD_DETAIL_BACK_HIT_X,
        DASHBOARD_DETAIL_BACK_HIT_Y,
        DASHBOARD_DETAIL_BACK_HIT_W,
        DASHBOARD_DETAIL_BACK_HIT_H,
    ) {
        return Some(BeeperSettingsTouchTarget::Back);
    }

    for target in BeeperSettingTarget::ALL {
        let row_y = beeper_setting_row_y(target);
        if contains(
            x,
            y,
            AUDIO_VOLUME_TOUCH_X,
            audio_volume_touch_y(row_y),
            AUDIO_VOLUME_TOUCH_W,
            AUDIO_VOLUME_TOUCH_H,
        ) {
            return Some(BeeperSettingsTouchTarget::Volume {
                target,
                level: beeper_volume_level_for_x(x),
            });
        }

        if contains(
            x,
            y,
            AUDIO_ROW_TOUCH_X,
            audio_row_touch_y(row_y),
            AUDIO_ROW_TOUCH_W,
            AUDIO_ROW_TOUCH_H,
        ) {
            return Some(BeeperSettingsTouchTarget::Target(target));
        }
    }

    None
}

#[allow(dead_code)]
pub const fn dashboard_route_for_target(target: DashboardTouchTarget) -> DashboardRoute {
    match target {
        DashboardTouchTarget::HomeWifi => DashboardRoute::Detail(DashboardDetailPage::Wifi),
        DashboardTouchTarget::HomeOutput => DashboardRoute::Detail(DashboardDetailPage::Output),
        DashboardTouchTarget::HomeThermal => DashboardRoute::Detail(DashboardDetailPage::Thermal),
        DashboardTouchTarget::HomeCells => DashboardRoute::Detail(DashboardDetailPage::Cells),
        DashboardTouchTarget::HomeCharger => DashboardRoute::Detail(DashboardDetailPage::Charger),
        DashboardTouchTarget::HomeBatteryFlow => {
            DashboardRoute::Detail(DashboardDetailPage::BatteryFlow)
        }
        DashboardTouchTarget::DetailBack => DashboardRoute::Home,
        DashboardTouchTarget::CellsAdvancedEntry => {
            DashboardRoute::Detail(DashboardDetailPage::BmsDetail)
        }
        DashboardTouchTarget::CellsAdvancedBack => {
            DashboardRoute::Detail(DashboardDetailPage::Cells)
        }
        DashboardTouchTarget::ChargerManualEntry => DashboardRoute::ManualCharge,
        DashboardTouchTarget::ManualBack => DashboardRoute::Detail(DashboardDetailPage::Charger),
        DashboardTouchTarget::ManualTarget3V7
        | DashboardTouchTarget::ManualTarget80
        | DashboardTouchTarget::ManualTarget100
        | DashboardTouchTarget::ManualSpeed100
        | DashboardTouchTarget::ManualSpeed500
        | DashboardTouchTarget::ManualSpeed1A
        | DashboardTouchTarget::ManualTimer1h
        | DashboardTouchTarget::ManualTimer2h
        | DashboardTouchTarget::ManualTimer6h
        | DashboardTouchTarget::ManualStart
        | DashboardTouchTarget::ManualStop => DashboardRoute::ManualCharge,
    }
}

#[allow(dead_code)]
pub const fn dashboard_route_for_home_focus(focus: DashboardHomeFocus) -> DashboardRoute {
    match focus {
        DashboardHomeFocus::Output => DashboardRoute::Detail(DashboardDetailPage::Output),
        DashboardHomeFocus::Thermal => DashboardRoute::Detail(DashboardDetailPage::Thermal),
        DashboardHomeFocus::Cells => DashboardRoute::Detail(DashboardDetailPage::Cells),
        DashboardHomeFocus::Charger => DashboardRoute::Detail(DashboardDetailPage::Charger),
        DashboardHomeFocus::BatteryFlow => DashboardRoute::Detail(DashboardDetailPage::BatteryFlow),
    }
}

#[allow(dead_code)]
pub const fn dashboard_manual_charge_action_for_target(
    target: DashboardTouchTarget,
) -> Option<ManualChargeUiAction> {
    match target {
        DashboardTouchTarget::ManualTarget3V7 => {
            Some(ManualChargeUiAction::SetTarget(ManualChargeTarget::Pack3V7))
        }
        DashboardTouchTarget::ManualTarget80 => {
            Some(ManualChargeUiAction::SetTarget(ManualChargeTarget::Rsoc80))
        }
        DashboardTouchTarget::ManualTarget100 => {
            Some(ManualChargeUiAction::SetTarget(ManualChargeTarget::Full100))
        }
        DashboardTouchTarget::ManualSpeed100 => {
            Some(ManualChargeUiAction::SetSpeed(ManualChargeSpeed::Ma100))
        }
        DashboardTouchTarget::ManualSpeed500 => {
            Some(ManualChargeUiAction::SetSpeed(ManualChargeSpeed::Ma500))
        }
        DashboardTouchTarget::ManualSpeed1A => {
            Some(ManualChargeUiAction::SetSpeed(ManualChargeSpeed::Ma1000))
        }
        DashboardTouchTarget::ManualTimer1h => Some(ManualChargeUiAction::SetTimerLimit(
            ManualChargeTimerLimit::H1,
        )),
        DashboardTouchTarget::ManualTimer2h => Some(ManualChargeUiAction::SetTimerLimit(
            ManualChargeTimerLimit::H2,
        )),
        DashboardTouchTarget::ManualTimer6h => Some(ManualChargeUiAction::SetTimerLimit(
            ManualChargeTimerLimit::H6,
        )),
        DashboardTouchTarget::ManualStart => Some(ManualChargeUiAction::Start),
        DashboardTouchTarget::ManualStop => Some(ManualChargeUiAction::Stop),
        _ => None,
    }
}

#[allow(dead_code)]
pub fn dashboard_route_has_active_animation(
    route: DashboardRoute,
    snapshot: &SelfCheckUiSnapshot,
) -> bool {
    snapshot.dashboard_detail.wifi.state == WifiConnectionState::Connecting
        || (matches!(route, DashboardRoute::Detail(DashboardDetailPage::Thermal))
            && thermal_fan_motion(
                snapshot.dashboard_detail.fan_rpm,
                snapshot.dashboard_detail.fan_pwm_pct,
                snapshot.dashboard_detail.fan_status,
            ) != ThermalFanMotion::Off)
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
            UpsMode::Blocked => (0, 0, 0),
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
            bms_balancing: model.focus == UiFocus::Left
                && !matches!(mode, UpsMode::Off | UpsMode::Blocked),
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
            chg_iin_ma: snapshot
                .bq25792_ibat_ma
                .and_then(|ma| u16::try_from(ma.max(0)).ok())
                .or(snapshot.bq25792_ichg_ma),
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
            DashboardDetailPage::BmsDetail => self
                .detail
                .bms_notice
                .unwrap_or("BMS DETAIL SOURCE PENDING"),
            DashboardDetailPage::BatteryFlow => self
                .detail
                .battery_notice
                .unwrap_or("PACK DETAIL SOURCE PENDING"),
            DashboardDetailPage::Output => self
                .detail
                .output_notice
                .unwrap_or("OUTPUT DETAIL SOURCE PENDING"),
            DashboardDetailPage::Charger => match self.detail.charger_notice {
                Some("backup_usb_low_output_charge") => "USB BACKUP: CHARGING ACTIVE",
                Some("backup_usb_output_high_latched") => "USB BACKUP: LOAD PRESENT",
                Some("backup_usb_telemetry_lost_latched") => "USB BACKUP: LOAD DATA LOST",
                Some("manual_loopback_confirmed_charging_100ma")
                | Some("manual_loopback_confirmed_charging_500ma")
                | Some("manual_loopback_confirmed_charging_1a") => "MANUAL: LOOP CHECK OK",
                Some(notice) => notice,
                None => "DETAIL UI ONLY - SOURCE PENDING",
            },
            DashboardDetailPage::Thermal => self
                .detail
                .thermal_notice
                .unwrap_or("DETAIL UI ONLY - FAN SOURCE PENDING"),
            DashboardDetailPage::Wifi => match self.detail.wifi.state {
                WifiConnectionState::Disabled | WifiConnectionState::Idle => "WIFI NOT ENABLED",
                WifiConnectionState::Connecting => "JOINING ACCESS POINT",
                WifiConnectionState::Connected => "LAN READY FOR API CLIENTS",
                WifiConnectionState::Error => match self.detail.wifi.last_error {
                    Some(error) => match error.ui_hint() {
                        "STATIC CFG" => "CHECK STATIC IP SETTINGS",
                        "JOIN FAIL" => "CHECK ACCESS POINT CREDENTIALS",
                        "DHCP WAIT" => "CHECK DHCP SERVER AVAILABILITY",
                        "LINK LOST" => "ACCESS POINT LINK LOST",
                        _ => "CHECK WIFI STATUS",
                    },
                    None => "CHECK WIFI STATUS",
                },
            },
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
        UpsMode::Blocked => "BLOCKED",
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
        UpsMode::Blocked => palette.down,
    }
}

fn mode_is_mains(mode: UpsMode) -> bool {
    !matches!(mode, UpsMode::Backup | UpsMode::Blocked)
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
pub fn render_wifi_icon_gallery<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
) -> Result<(), P::Error> {
    let palette = palette_for(variant);
    fill(painter, 0, 0, UI_W, UI_H, palette.bg)?;
    draw_background_grid(painter, palette)?;
    draw_outline(painter, 0, 0, UI_W, UI_H, palette.border)?;
    text(
        painter,
        variant,
        FontRole::TextTitle,
        "WIFI ICONS 1:1",
        Point::new(8, 5),
        HorizontalAlignment::Left,
        palette.text,
    )?;

    let entries = [
        ("OFF", WifiSnapshot::disabled(), 0),
        (
            "ERROR",
            WifiSnapshot {
                state: WifiConnectionState::Error,
                last_error: Some(mains_aegis_firmware::net_types::WifiErrorKind::DhcpTimeout),
                ..WifiSnapshot::disabled()
            },
            0,
        ),
        ("CONN 0", WifiSnapshot::connecting(), 0),
        ("CONN 1", WifiSnapshot::connecting(), 4),
        ("CONN 2", WifiSnapshot::connecting(), 8),
        (
            "LOW",
            WifiSnapshot {
                state: WifiConnectionState::Connected,
                rssi_dbm: Some(-82),
                ..WifiSnapshot::disabled()
            },
            0,
        ),
        (
            "MID",
            WifiSnapshot {
                state: WifiConnectionState::Connected,
                rssi_dbm: Some(-67),
                ..WifiSnapshot::disabled()
            },
            0,
        ),
        (
            "HIGH",
            WifiSnapshot {
                state: WifiConnectionState::Connected,
                rssi_dbm: Some(-50),
                ..WifiSnapshot::disabled()
            },
            0,
        ),
    ];

    for (idx, (label, wifi, frame_no)) in entries.iter().enumerate() {
        let col = (idx % 3) as u16;
        let row = (idx / 3) as u16;
        let cell_x = 8 + col * 104;
        let cell_y = 30 + row * 44;
        draw_panel(
            painter,
            cell_x,
            cell_y,
            96,
            36,
            palette,
            false,
            palette.accent,
        )?;
        text(
            painter,
            variant,
            FontRole::TextCompact,
            *label,
            Point::new((cell_x + 6) as i32, (cell_y + 5) as i32),
            HorizontalAlignment::Left,
            palette.text_dim,
        )?;
        draw_dashboard_wifi_icon_at(
            painter,
            cell_x + 41,
            cell_y + 18,
            14,
            palette,
            *wifi,
            *frame_no,
        )?;
    }

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

pub fn render_firmware_safe_mode<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    reset_cause: &str,
    abnormal_boots: u8,
) -> Result<(), P::Error> {
    let palette = palette_for(variant);
    fill(painter, 0, 0, UI_W, UI_H, palette.bg)?;
    draw_background_grid(painter, palette)?;
    draw_outline(painter, 0, 0, UI_W, UI_H, ERROR_COLOR)?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        "RECOVERY SAFE MODE",
        Point::new(12, 12),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        "SAFE",
        Point::new(304, 12),
        HorizontalAlignment::Right,
        ERROR_COLOR,
    )?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        "BOOT LOOP BLOCKED",
        Point::new(12, 31),
        HorizontalAlignment::Left,
        palette.text_dim,
    )?;
    fill(painter, 12, 44, 296, 2, ERROR_COLOR)?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        "OUTPUTS + CHARGER HELD OFF",
        Point::new(160, 66),
        HorizontalAlignment::Center,
        palette.text,
    )?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        "RESET",
        Point::new(28, 91),
        HorizontalAlignment::Left,
        palette.text_dim,
    )?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        reset_cause,
        Point::new(108, 91),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    let mut count = heapless::String::<24>::new();
    let _ = write!(count, "EARLY BOOTS  {} / 3", abnormal_boots);
    text(
        painter,
        variant,
        FontRole::TextBody,
        count.as_str(),
        Point::new(160, 112),
        HorizontalAlignment::Center,
        ERROR_COLOR,
    )?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        "RECOVERY",
        Point::new(28, 136),
        HorizontalAlignment::Left,
        palette.text_dim,
    )?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        "INSTALL CONFIRMED FIRMWARE",
        Point::new(28, 155),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    Ok(())
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

    if variant == UiVariant::InstrumentB && overlay == SelfCheckOverlay::ManualChargeLoopbackConfirm
    {
        draw_dashboard_manual_loopback_confirm_overlay(painter, variant, palette)?;
    }

    Ok(())
}

#[allow(dead_code)]
pub fn render_dashboard_shell<P: UiPainter>(
    painter: &mut P,
    model: &UiModel,
    variant: UiVariant,
    shell: DashboardShellState,
    self_check: Option<&SelfCheckUiSnapshot>,
) -> Result<(), P::Error> {
    if variant != UiVariant::InstrumentB {
        return render_frame_with_dashboard_route_overlay(
            painter,
            model,
            variant,
            shell.dashboard_route,
            self_check,
            SelfCheckOverlay::None,
        );
    }

    let palette = palette_for(variant);
    let data = DashboardData::from_model(model);
    fill(painter, 0, 0, UI_W, UI_H, palette.bg)?;
    draw_background_grid(painter, palette)?;
    if shell.page == DashboardPrimaryPage::BeeperSettings {
        return render_beeper_settings_page(
            painter,
            variant,
            palette,
            shell.beeper_prefs,
            shell.menu_selected,
        );
    }

    let offset = i32::from(shell.dashboard_menu_offset_y)
        .clamp(0, i32::from(DASHBOARD_MENU_STACK_H - (UI_H as i16))) as i16;
    {
        let mut translated = TranslatedPainter::new(painter, 0, -offset);
        render_variant_b(
            &mut translated,
            variant,
            palette,
            data,
            shell.dashboard_route,
            self_check,
        )?;
        if shell.dashboard_route == DashboardRoute::Home {
            draw_dashboard_home_focus_overlay(&mut translated, variant, palette, shell.home_focus)?;
        }
    }
    {
        let mut translated = TranslatedPainter::new(painter, 0, (UI_H as i16) - offset);
        render_dashboard_menu_page(
            &mut translated,
            variant,
            palette,
            shell.menu_selected,
            shell.home_focus,
            shell.menu_style,
        )?;
    }

    Ok(())
}

#[allow(dead_code)]
pub fn render_dashboard_touch_regions_overlay<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    route: DashboardRoute,
) -> Result<(), P::Error> {
    let palette = palette_for(variant);

    match route {
        DashboardRoute::Home => {
            draw_dashboard_touch_region_overlay(
                painter,
                variant,
                palette,
                DASHBOARD_HOME_WIFI_TOUCH_X,
                DASHBOARD_HOME_WIFI_TOUCH_Y,
                DASHBOARD_HOME_WIFI_TOUCH_W,
                DASHBOARD_HOME_WIFI_TOUCH_H,
                "W",
                palette.touch,
                113,
                2,
            )?;
            draw_dashboard_touch_region_overlay(
                painter,
                variant,
                palette,
                DASHBOARD_HOME_OUTPUT_X,
                DASHBOARD_HOME_OUTPUT_Y,
                DASHBOARD_HOME_OUTPUT_W,
                DASHBOARD_HOME_OUTPUT_H,
                "2",
                palette.accent,
                10,
                24,
            )?;
            draw_dashboard_touch_region_overlay(
                painter,
                variant,
                palette,
                DASHBOARD_HOME_THERMAL_X,
                DASHBOARD_HOME_THERMAL_Y,
                DASHBOARD_HOME_THERMAL_W,
                DASHBOARD_HOME_THERMAL_H,
                "3",
                palette.center,
                10,
                78,
            )?;
            draw_dashboard_touch_region_overlay(
                painter,
                variant,
                palette,
                DASHBOARD_HOME_CELLS_X,
                DASHBOARD_HOME_CELLS_Y,
                DASHBOARD_HOME_CELLS_W,
                DASHBOARD_HOME_CELLS_H,
                "4",
                palette.left,
                210,
                24,
            )?;
            draw_dashboard_touch_region_overlay(
                painter,
                variant,
                palette,
                DASHBOARD_HOME_CHARGER_X,
                DASHBOARD_HOME_CHARGER_Y,
                DASHBOARD_HOME_CHARGER_W,
                DASHBOARD_HOME_CHARGER_H,
                "5",
                palette.right,
                210,
                74,
            )?;
            draw_dashboard_touch_region_overlay(
                painter,
                variant,
                palette,
                DASHBOARD_HOME_BATTERY_FLOW_X,
                DASHBOARD_HOME_BATTERY_FLOW_Y,
                DASHBOARD_HOME_BATTERY_FLOW_W,
                DASHBOARD_HOME_BATTERY_FLOW_H,
                "6",
                palette.down,
                210,
                124,
            )?;
        }
        DashboardRoute::Detail(_) | DashboardRoute::ManualCharge => {}
    }

    Ok(())
}

#[allow(dead_code)]
pub fn render_beeper_settings_touch_regions_overlay<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
) -> Result<(), P::Error> {
    let palette = palette_for(variant);

    draw_dashboard_touch_region_overlay(
        painter,
        variant,
        palette,
        AUDIO_VOLUME_TOUCH_X,
        audio_volume_touch_y(AUDIO_ACTION_ROW_Y),
        AUDIO_VOLUME_TOUCH_W,
        AUDIO_VOLUME_TOUCH_H,
        "A",
        palette.right,
        AUDIO_VOLUME_TOUCH_X + AUDIO_VOLUME_TOUCH_W - 12,
        audio_volume_touch_y(AUDIO_ACTION_ROW_Y) + 2,
    )?;
    draw_dashboard_touch_region_overlay(
        painter,
        variant,
        palette,
        AUDIO_VOLUME_TOUCH_X,
        audio_volume_touch_y(AUDIO_SYSTEM_ROW_Y),
        AUDIO_VOLUME_TOUCH_W,
        AUDIO_VOLUME_TOUCH_H,
        "S",
        palette.center,
        AUDIO_VOLUME_TOUCH_X + AUDIO_VOLUME_TOUCH_W - 12,
        audio_volume_touch_y(AUDIO_SYSTEM_ROW_Y) + 2,
    )
}

fn dashboard_home_focus_bounds(focus: DashboardHomeFocus) -> (u16, u16, u16, u16) {
    match focus {
        DashboardHomeFocus::Output => (
            DASHBOARD_HOME_OUTPUT_X,
            DASHBOARD_HOME_OUTPUT_Y,
            DASHBOARD_HOME_OUTPUT_W,
            DASHBOARD_HOME_OUTPUT_H,
        ),
        DashboardHomeFocus::Thermal => (
            DASHBOARD_HOME_THERMAL_X,
            DASHBOARD_HOME_THERMAL_Y,
            DASHBOARD_HOME_THERMAL_W,
            DASHBOARD_HOME_THERMAL_H,
        ),
        DashboardHomeFocus::Cells => (
            DASHBOARD_HOME_CELLS_X,
            DASHBOARD_HOME_CELLS_Y,
            DASHBOARD_HOME_CELLS_W,
            DASHBOARD_HOME_CELLS_H,
        ),
        DashboardHomeFocus::Charger => (
            DASHBOARD_HOME_CHARGER_X,
            DASHBOARD_HOME_CHARGER_Y,
            DASHBOARD_HOME_CHARGER_W,
            DASHBOARD_HOME_CHARGER_H,
        ),
        DashboardHomeFocus::BatteryFlow => (
            DASHBOARD_HOME_BATTERY_FLOW_X,
            DASHBOARD_HOME_BATTERY_FLOW_Y,
            DASHBOARD_HOME_BATTERY_FLOW_W,
            DASHBOARD_HOME_BATTERY_FLOW_H,
        ),
    }
}

fn dashboard_home_focus_color(palette: Palette, focus: DashboardHomeFocus) -> u16 {
    match focus {
        DashboardHomeFocus::Output => palette.up,
        DashboardHomeFocus::Thermal => palette.center,
        DashboardHomeFocus::Cells => palette.left,
        DashboardHomeFocus::Charger => palette.right,
        DashboardHomeFocus::BatteryFlow => palette.down,
    }
}

fn draw_dashboard_home_focus_overlay<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    focus: DashboardHomeFocus,
) -> Result<(), P::Error> {
    let (x, y, w, h) = dashboard_home_focus_bounds(focus);
    let color = dashboard_home_focus_color(palette, focus);
    draw_outline(painter, x, y, w, h, color)?;
    if w > 4 && h > 4 {
        draw_outline(
            painter,
            x + 2,
            y + 2,
            w - 4,
            h - 4,
            fade_color(color, palette.text),
        )?;
    }
    fill(
        painter,
        x,
        y,
        w.min(68),
        DASHBOARD_HOME_FOCUS_LABEL_H,
        color,
    )?;
    text(
        painter,
        variant,
        FontRole::DetailBody,
        focus.label(),
        Point::new((x + 4) as i32, (y + 1) as i32),
        HorizontalAlignment::Left,
        palette.bg,
    )
}

fn render_dashboard_menu_page<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    selected: MenuItem,
    home_focus: DashboardHomeFocus,
    style: DashboardMenuStyle,
) -> Result<(), P::Error> {
    let selected_color = menu_item_color(palette, selected, home_focus);
    let selected_rail = selected_menu_rail_item(selected);
    text_with_position(
        painter,
        variant,
        FontRole::DetailTitle,
        "MENU",
        Point::new(8, DASHBOARD_MENU_HEADER_CENTER_Y as i32),
        VerticalPosition::Center,
        HorizontalAlignment::Left,
        palette.text,
    )?;
    let rule_y = DASHBOARD_MENU_HEADER_H;
    fill(
        painter,
        8,
        rule_y,
        UI_W - 16,
        1,
        fade_color(palette.border, palette.panel),
    )?;
    fill(painter, 142, rule_y, 36, 2, selected_color)?;

    let step: i16 = (DASHBOARD_MENU_ICON_W + DASHBOARD_MENU_ICON_GAP) as i16;
    let rail_origin_x = DASHBOARD_MENU_ICON_CENTER_X
        - (DASHBOARD_MENU_ICON_W as i16 / 2)
        - (selected_rail.index() as i16 * step);

    for item in MenuRailItem::ALL {
        let x = rail_origin_x + (item.index() as i16 * step);
        let y = (DASHBOARD_MENU_ICON_Y + 4) as i16;
        if x + DASHBOARD_MENU_ICON_W as i16 <= 0 || x >= UI_W as i16 {
            continue;
        }

        let item_selected = item == selected_rail;
        let accent = menu_rail_item_color(palette, item);
        draw_menu_icon_tile(
            painter,
            palette,
            item,
            x.max(0) as u16,
            y.max(0) as u16,
            item_selected,
            accent,
            style,
        )?;
    }

    draw_dashboard_menu_footer(
        painter,
        variant,
        palette,
        selected,
        selected_rail,
        selected_color,
        style,
    )
}

fn draw_dashboard_menu_footer<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    selected: MenuItem,
    selected_rail: MenuRailItem,
    selected_color: u16,
    style: DashboardMenuStyle,
) -> Result<(), P::Error> {
    match style {
        DashboardMenuStyle::DenseBadge => {
            draw_menu_footer_dense_badge(painter, variant, palette, selected, selected_color)
        }
        DashboardMenuStyle::DockBar => {
            draw_menu_footer_dock_bar(painter, variant, palette, selected, selected_color)
        }
        DashboardMenuStyle::SplitRail => {
            draw_menu_footer_split_rail(painter, variant, palette, selected, selected_color)
        }
        DashboardMenuStyle::SignalPlate => {
            draw_menu_footer_signal_plate(painter, variant, palette, selected, selected_color)
        }
    }?;
    draw_menu_footer_nav_hints(painter, palette, selected_rail, selected_color)
}

fn draw_menu_footer_dense_badge<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    selected: MenuItem,
    selected_color: u16,
) -> Result<(), P::Error> {
    let footer_bg = fade_color(palette.panel_alt, palette.bg);
    let footer_rule = fade_color(palette.border, palette.panel);

    fill(
        painter,
        0,
        DASHBOARD_MENU_FOOTER_Y,
        UI_W,
        DASHBOARD_MENU_FOOTER_H,
        footer_bg,
    )?;
    fill(
        painter,
        8,
        DASHBOARD_MENU_FOOTER_Y,
        UI_W - 16,
        1,
        footer_rule,
    )?;
    text_with_position(
        painter,
        variant,
        FontRole::DetailTitle,
        selected.label(),
        Point::new((UI_W / 2) as i32, DASHBOARD_MENU_FOOTER_CENTER_Y as i32),
        VerticalPosition::Center,
        HorizontalAlignment::Center,
        selected_color,
    )
}

fn draw_menu_footer_dock_bar<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    selected: MenuItem,
    selected_color: u16,
) -> Result<(), P::Error> {
    let footer_bg = fade_color(palette.panel_alt, palette.bg);
    let footer_rule = fade_color(palette.border, palette.panel);

    fill(
        painter,
        0,
        DASHBOARD_MENU_FOOTER_Y,
        UI_W,
        DASHBOARD_MENU_FOOTER_H,
        footer_bg,
    )?;
    fill(
        painter,
        8,
        DASHBOARD_MENU_FOOTER_Y,
        UI_W - 16,
        1,
        footer_rule,
    )?;
    text_with_position(
        painter,
        variant,
        FontRole::DetailTitle,
        selected.label(),
        Point::new((UI_W / 2) as i32, DASHBOARD_MENU_FOOTER_CENTER_Y as i32),
        VerticalPosition::Center,
        HorizontalAlignment::Center,
        selected_color,
    )
}

fn draw_menu_footer_split_rail<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    selected: MenuItem,
    selected_color: u16,
) -> Result<(), P::Error> {
    let footer_bg = fade_color(palette.panel_alt, palette.bg);
    let footer_rule = fade_color(palette.border, palette.panel);

    fill(
        painter,
        0,
        DASHBOARD_MENU_FOOTER_Y,
        UI_W,
        DASHBOARD_MENU_FOOTER_H,
        footer_bg,
    )?;
    fill(
        painter,
        8,
        DASHBOARD_MENU_FOOTER_Y,
        UI_W - 16,
        1,
        footer_rule,
    )?;
    text_with_position(
        painter,
        variant,
        FontRole::DetailTitle,
        selected.label(),
        Point::new((UI_W / 2) as i32, DASHBOARD_MENU_FOOTER_CENTER_Y as i32),
        VerticalPosition::Center,
        HorizontalAlignment::Center,
        selected_color,
    )
}

fn draw_menu_footer_signal_plate<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    selected: MenuItem,
    selected_color: u16,
) -> Result<(), P::Error> {
    let footer_bg = fade_color(palette.panel_alt, palette.bg);
    let footer_rule = fade_color(palette.border, palette.panel);

    fill(
        painter,
        0,
        DASHBOARD_MENU_FOOTER_Y,
        UI_W,
        DASHBOARD_MENU_FOOTER_H,
        footer_bg,
    )?;
    fill(
        painter,
        8,
        DASHBOARD_MENU_FOOTER_Y,
        UI_W - 16,
        1,
        footer_rule,
    )?;
    text_with_position(
        painter,
        variant,
        FontRole::DetailTitle,
        selected.label(),
        Point::new((UI_W / 2) as i32, DASHBOARD_MENU_FOOTER_CENTER_Y as i32),
        VerticalPosition::Center,
        HorizontalAlignment::Center,
        selected_color,
    )
}

fn draw_menu_footer_nav_hints<P: UiPainter>(
    painter: &mut P,
    palette: Palette,
    selected_rail: MenuRailItem,
    selected_color: u16,
) -> Result<(), P::Error> {
    let active_left = selected_rail.index() > 0;
    let active_right = selected_rail.index() + 1 < MenuRailItem::ALL.len();

    draw_menu_footer_nav_hint(
        painter,
        DASHBOARD_MENU_NAV_HINT_LEFT_X,
        DASHBOARD_MENU_NAV_HINT_Y,
        false,
        if active_left {
            selected_color
        } else {
            fade_color(palette.border, palette.text_dim)
        },
        fade_color(palette.panel, palette.bg),
    )?;
    draw_menu_footer_nav_hint(
        painter,
        DASHBOARD_MENU_NAV_HINT_RIGHT_X,
        DASHBOARD_MENU_NAV_HINT_Y,
        true,
        if active_right {
            selected_color
        } else {
            fade_color(palette.border, palette.text_dim)
        },
        fade_color(palette.panel, palette.bg),
    )
}

fn draw_menu_footer_nav_hint<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    right: bool,
    accent: u16,
    _panel_fill: u16,
) -> Result<(), P::Error> {
    draw_icon_blocks_centered(
        painter,
        x,
        y,
        DASHBOARD_MENU_NAV_HINT_BOX_W,
        DASHBOARD_MENU_NAV_HINT_BOX_H,
        if right {
            MENU_ICON_CHEVRON_RIGHT_26
        } else {
            MENU_ICON_CHEVRON_LEFT_26
        },
        accent,
    )
}

fn menu_tile_fill_color(
    style: DashboardMenuStyle,
    palette: Palette,
    accent: u16,
    selected: bool,
) -> u16 {
    if !selected {
        return fade_color(palette.panel, palette.bg);
    }
    match style {
        DashboardMenuStyle::DenseBadge => {
            fade_color(accent, fade_color(palette.panel_alt, palette.bg))
        }
        DashboardMenuStyle::DockBar => fade_color(accent, palette.panel),
        DashboardMenuStyle::SplitRail => fade_color(accent, fade_color(palette.panel, palette.bg)),
        DashboardMenuStyle::SignalPlate => {
            fade_color(accent, fade_color(palette.panel_alt, palette.bg))
        }
    }
}

fn menu_tile_border_color(
    style: DashboardMenuStyle,
    palette: Palette,
    accent: u16,
    selected: bool,
) -> u16 {
    if !selected {
        return fade_color(palette.border, palette.text_dim);
    }
    match style {
        DashboardMenuStyle::DenseBadge
        | DashboardMenuStyle::DockBar
        | DashboardMenuStyle::SplitRail
        | DashboardMenuStyle::SignalPlate => accent,
    }
}

fn draw_menu_tile_style_decoration<P: UiPainter>(
    painter: &mut P,
    _palette: Palette,
    x: u16,
    y: u16,
    selected: bool,
    accent: u16,
    _border: u16,
    style: DashboardMenuStyle,
) -> Result<(), P::Error> {
    if !selected {
        return Ok(());
    }

    match style {
        DashboardMenuStyle::DenseBadge | DashboardMenuStyle::SplitRail => Ok(()),
        DashboardMenuStyle::DockBar | DashboardMenuStyle::SignalPlate => fill(
            painter,
            x + 12,
            y + 7,
            DASHBOARD_MENU_ICON_W - 24,
            2,
            accent,
        ),
    }
}

fn draw_menu_icon_tile<P: UiPainter>(
    painter: &mut P,
    palette: Palette,
    item: MenuRailItem,
    x: u16,
    y: u16,
    selected: bool,
    accent: u16,
    style: DashboardMenuStyle,
) -> Result<(), P::Error> {
    let fill_color = menu_tile_fill_color(style, palette, accent, selected);
    let border = menu_tile_border_color(style, palette, accent, selected);
    fill(
        painter,
        x,
        y,
        DASHBOARD_MENU_ICON_W,
        DASHBOARD_MENU_ICON_H,
        border,
    )?;
    if DASHBOARD_MENU_ICON_W > 2 && DASHBOARD_MENU_ICON_H > 2 {
        fill(
            painter,
            x + 1,
            y + 1,
            DASHBOARD_MENU_ICON_W - 2,
            DASHBOARD_MENU_ICON_H - 2,
            fill_color,
        )?;
    }
    draw_outline(
        painter,
        x + 3,
        y + 3,
        DASHBOARD_MENU_ICON_W - 6,
        DASHBOARD_MENU_ICON_H - 6,
        fade_color(border, palette.bg),
    )?;
    match item {
        MenuRailItem::Dashboard => draw_dashboard_menu_dashboard_icon(
            painter,
            x,
            y,
            accent,
            if selected {
                palette.text
            } else {
                palette.text_dim
            },
            fill_color,
        ),
        MenuRailItem::Add => draw_dashboard_menu_add_icon(
            painter,
            x,
            y,
            accent,
            if selected {
                palette.text
            } else {
                palette.text_dim
            },
        ),
        MenuRailItem::Audio => draw_dashboard_menu_audio_icon(
            painter,
            x,
            y,
            accent,
            if selected {
                palette.text
            } else {
                palette.text_dim
            },
            fill_color,
        ),
        MenuRailItem::Settings => draw_dashboard_menu_settings_icon(
            painter,
            x,
            y,
            accent,
            if selected {
                palette.text
            } else {
                palette.text_dim
            },
            fill_color,
        ),
        MenuRailItem::Stats => draw_dashboard_menu_stats_icon(
            painter,
            x,
            y,
            accent,
            if selected {
                palette.text
            } else {
                palette.text_dim
            },
        ),
    }?;
    draw_menu_tile_style_decoration(painter, palette, x, y, selected, accent, border, style)?;
    Ok(())
}

fn draw_dashboard_menu_dashboard_icon<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    accent: u16,
    _fg: u16,
    _bg: u16,
) -> Result<(), P::Error> {
    draw_icon_blocks_centered(
        painter,
        x + DASHBOARD_MENU_GLYPH_INSET,
        y + DASHBOARD_MENU_GLYPH_INSET,
        DASHBOARD_MENU_ICON_W - (DASHBOARD_MENU_GLYPH_INSET * 2),
        DASHBOARD_MENU_ICON_H - (DASHBOARD_MENU_GLYPH_INSET * 2),
        MENU_ICON_SPEED_28,
        accent,
    )
}

fn draw_dashboard_menu_add_icon<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    accent: u16,
    _fg: u16,
) -> Result<(), P::Error> {
    draw_icon_blocks_centered(
        painter,
        x + DASHBOARD_MENU_GLYPH_INSET,
        y + DASHBOARD_MENU_GLYPH_INSET,
        DASHBOARD_MENU_ICON_W - (DASHBOARD_MENU_GLYPH_INSET * 2),
        DASHBOARD_MENU_ICON_H - (DASHBOARD_MENU_GLYPH_INSET * 2),
        MENU_ICON_ADD_28,
        accent,
    )
}

fn draw_dashboard_menu_audio_icon<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    accent: u16,
    _fg: u16,
    _bg: u16,
) -> Result<(), P::Error> {
    draw_icon_blocks_centered(
        painter,
        x + DASHBOARD_MENU_GLYPH_INSET,
        y + DASHBOARD_MENU_GLYPH_INSET,
        DASHBOARD_MENU_ICON_W - (DASHBOARD_MENU_GLYPH_INSET * 2),
        DASHBOARD_MENU_ICON_H - (DASHBOARD_MENU_GLYPH_INSET * 2),
        MENU_ICON_VOLUME_UP_28,
        accent,
    )
}

fn draw_dashboard_menu_settings_icon<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    accent: u16,
    _fg: u16,
    _bg: u16,
) -> Result<(), P::Error> {
    draw_icon_blocks_centered(
        painter,
        x + DASHBOARD_MENU_GLYPH_INSET,
        y + DASHBOARD_MENU_GLYPH_INSET,
        DASHBOARD_MENU_ICON_W - (DASHBOARD_MENU_GLYPH_INSET * 2),
        DASHBOARD_MENU_ICON_H - (DASHBOARD_MENU_GLYPH_INSET * 2),
        MENU_ICON_SETTINGS_28,
        accent,
    )
}

fn draw_dashboard_menu_stats_icon<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    accent: u16,
    _fg: u16,
) -> Result<(), P::Error> {
    draw_icon_blocks_centered(
        painter,
        x + DASHBOARD_MENU_GLYPH_INSET,
        y + DASHBOARD_MENU_GLYPH_INSET,
        DASHBOARD_MENU_ICON_W - (DASHBOARD_MENU_GLYPH_INSET * 2),
        DASHBOARD_MENU_ICON_H - (DASHBOARD_MENU_GLYPH_INSET * 2),
        MENU_ICON_BAR_CHART_28,
        accent,
    )
}

fn render_beeper_settings_page<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    prefs: BeeperPrefs,
    _menu_selected: MenuItem,
) -> Result<(), P::Error> {
    draw_beeper_settings_top_bar(
        painter,
        variant,
        palette,
        prefs.selected_target.label(),
        beeper_target_focus_color(palette, prefs.selected_target),
    )?;
    draw_beeper_settings_scale(painter, variant, palette)?;
    for target in BeeperSettingTarget::ALL {
        draw_beeper_settings_row(
            painter,
            variant,
            palette,
            target,
            prefs.volume_for(target),
            prefs.selected_target == target,
        )?;
    }
    Ok(())
}

fn draw_beeper_settings_top_bar<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    status_tag: &'static str,
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
        "AUDIO",
        Point::new(DETAIL_TITLE_X, 2),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    text(
        painter,
        variant,
        FontRole::DetailBody,
        status_tag,
        Point::new(DETAIL_STATUS_X, 2),
        HorizontalAlignment::Right,
        status_color,
    )
}

fn draw_beeper_settings_scale<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
) -> Result<(), P::Error> {
    let step = AUDIO_TRACK_W / 6;
    for (idx, level) in BeeperVolumeLevel::ALL.iter().enumerate() {
        let node_x = AUDIO_TRACK_X + idx as u16 * step;
        text(
            painter,
            variant,
            FontRole::DetailBody,
            level.scale_label(),
            Point::new(node_x as i32, AUDIO_SCALE_Y as i32),
            HorizontalAlignment::Center,
            palette.text_dim,
        )?;
    }
    Ok(())
}

fn draw_beeper_settings_row<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    target: BeeperSettingTarget,
    level: BeeperVolumeLevel,
    selected: bool,
) -> Result<(), P::Error> {
    let row_y = beeper_setting_row_y(target);
    let row_fill = if selected {
        fade_color(palette.panel_alt, palette.bg)
    } else {
        fade_color(palette.panel, palette.bg)
    };
    let focus_accent = beeper_target_focus_color(palette, target);
    let level_accent = beeper_volume_color(palette, level);
    let step = AUDIO_TRACK_W / 6;
    let active_idx = beeper_volume_index(level) as u16;
    let active_x = AUDIO_TRACK_X + active_idx * step;
    let row_center_y = row_y + (AUDIO_ROW_H / 2);
    let track_y = row_center_y - (AUDIO_TRACK_H / 2);
    let badge_y = row_center_y - (AUDIO_BADGE_H / 2);
    let badge_fill = fade_color(palette.panel, palette.bg);

    draw_manual_action_button(
        painter,
        AUDIO_ROW_X,
        row_y,
        AUDIO_ROW_W,
        AUDIO_ROW_H,
        row_fill,
        row_fill,
    )?;
    fill(
        painter,
        AUDIO_ROW_X + 6,
        row_y + 5,
        3,
        AUDIO_ROW_H - 10,
        if selected {
            focus_accent
        } else {
            palette.border
        },
    )?;
    text_with_position(
        painter,
        variant,
        FontRole::DetailTitle,
        target.label(),
        Point::new((AUDIO_ROW_X + 16) as i32, row_center_y as i32),
        VerticalPosition::Center,
        HorizontalAlignment::Left,
        if selected {
            palette.text
        } else {
            palette.text_dim
        },
    )?;

    fill(
        painter,
        AUDIO_TRACK_X,
        track_y + (AUDIO_TRACK_H / 2) - 1,
        AUDIO_TRACK_W,
        2,
        fade_color(palette.border, palette.text_dim),
    )?;
    fill(
        painter,
        AUDIO_TRACK_X,
        track_y + (AUDIO_TRACK_H / 2) - 1,
        active_x.saturating_sub(AUDIO_TRACK_X) + 1,
        2,
        level_accent,
    )?;

    for idx in 0..BeeperVolumeLevel::ALL.len() {
        let node_x = AUDIO_TRACK_X + idx as u16 * step;
        let active = idx as u16 == active_idx;
        let node_size = if active {
            AUDIO_NODE_SIZE + 4
        } else {
            AUDIO_NODE_SIZE
        };
        let node_half = node_size / 2;
        draw_manual_action_button(
            painter,
            node_x.saturating_sub(node_half),
            row_center_y - node_half,
            node_size,
            node_size,
            if active {
                fade_color(level_accent, palette.panel_alt)
            } else {
                fade_color(palette.panel, palette.bg)
            },
            if active {
                level_accent
            } else {
                fade_color(palette.border, palette.text_dim)
            },
        )?;
    }

    draw_manual_action_button(
        painter,
        AUDIO_BADGE_X,
        badge_y,
        AUDIO_BADGE_W,
        AUDIO_BADGE_H,
        badge_fill,
        badge_fill,
    )?;
    text_with_position(
        painter,
        variant,
        FontRole::DetailTitle,
        level.badge_label(),
        Point::new(
            (AUDIO_BADGE_X + (AUDIO_BADGE_W / 2)) as i32,
            row_center_y as i32,
        ),
        VerticalPosition::Center,
        HorizontalAlignment::Center,
        if level == BeeperVolumeLevel::Off {
            if selected {
                palette.text
            } else {
                palette.text_dim
            }
        } else {
            level_accent
        },
    )
}

fn selected_menu_rail_item(selected: MenuItem) -> MenuRailItem {
    match selected {
        MenuItem::Dashboard => MenuRailItem::Dashboard,
        MenuItem::Beeper => MenuRailItem::Audio,
    }
}

fn menu_item_for_rail_item(item: MenuRailItem) -> Option<MenuItem> {
    match item {
        MenuRailItem::Dashboard => Some(MenuItem::Dashboard),
        MenuRailItem::Audio => Some(MenuItem::Beeper),
        MenuRailItem::Add | MenuRailItem::Settings | MenuRailItem::Stats => None,
    }
}

fn dashboard_menu_rail_origin_x(selected: MenuItem) -> i16 {
    let selected_rail = selected_menu_rail_item(selected);
    let step: i16 = (DASHBOARD_MENU_ICON_W + DASHBOARD_MENU_ICON_GAP) as i16;
    DASHBOARD_MENU_ICON_CENTER_X
        - (DASHBOARD_MENU_ICON_W as i16 / 2)
        - (selected_rail.index() as i16 * step)
}

fn menu_rail_item_color(palette: Palette, item: MenuRailItem) -> u16 {
    match item {
        MenuRailItem::Dashboard => palette.left,
        MenuRailItem::Audio => palette.right,
        MenuRailItem::Add | MenuRailItem::Settings | MenuRailItem::Stats => palette.text_dim,
    }
}

fn menu_item_color(palette: Palette, item: MenuItem, _home_focus: DashboardHomeFocus) -> u16 {
    match item {
        MenuItem::Dashboard => palette.left,
        MenuItem::Beeper => palette.right,
    }
}

fn menu_footer_underline_w(selected: MenuItem) -> u16 {
    match selected {
        MenuItem::Dashboard => 72,
        MenuItem::Beeper => 44,
    }
}

fn beeper_volume_color(palette: Palette, level: BeeperVolumeLevel) -> u16 {
    match level {
        BeeperVolumeLevel::Off => palette.border,
        BeeperVolumeLevel::L1 | BeeperVolumeLevel::L2 => palette.left,
        BeeperVolumeLevel::L3 | BeeperVolumeLevel::L4 => palette.accent,
        BeeperVolumeLevel::L5 => palette.center,
        BeeperVolumeLevel::L6 => palette.right,
    }
}

fn beeper_volume_index(level: BeeperVolumeLevel) -> usize {
    BeeperVolumeLevel::ALL
        .iter()
        .position(|candidate| *candidate == level)
        .unwrap_or(0)
}

fn beeper_volume_level_for_x(x: u16) -> BeeperVolumeLevel {
    let step = AUDIO_TRACK_W / 6;
    let idx = if x <= AUDIO_TRACK_X {
        0
    } else if x >= AUDIO_TRACK_X + AUDIO_TRACK_W {
        6
    } else {
        ((x - AUDIO_TRACK_X + (step / 2)) / step).min(6)
    };
    BeeperVolumeLevel::from_step(idx as u8)
}

fn beeper_setting_row_y(target: BeeperSettingTarget) -> u16 {
    match target {
        BeeperSettingTarget::Action => AUDIO_ACTION_ROW_Y,
        BeeperSettingTarget::System => AUDIO_SYSTEM_ROW_Y,
    }
}

const fn audio_volume_touch_y(row_y: u16) -> u16 {
    row_y.saturating_sub(AUDIO_VOLUME_TOUCH_Y_INSET)
}

const fn audio_row_touch_y(row_y: u16) -> u16 {
    row_y.saturating_sub(AUDIO_ROW_TOUCH_Y_INSET)
}

fn beeper_target_focus_color(palette: Palette, target: BeeperSettingTarget) -> u16 {
    match target {
        BeeperSettingTarget::Action => palette.right,
        BeeperSettingTarget::System => palette.center,
    }
}

struct TranslatedPainter<'a, P> {
    painter: &'a mut P,
    dx: i16,
    dy: i16,
}

impl<'a, P> TranslatedPainter<'a, P> {
    fn new(painter: &'a mut P, dx: i16, dy: i16) -> Self {
        Self { painter, dx, dy }
    }
}

impl<P: UiPainter> UiPainter for TranslatedPainter<'_, P> {
    type Error = P::Error;

    fn fill_rect(
        &mut self,
        x: u16,
        y: u16,
        w: u16,
        h: u16,
        rgb565: u16,
    ) -> Result<(), Self::Error> {
        let x0 = i32::from(x) + i32::from(self.dx);
        let y0 = i32::from(y) + i32::from(self.dy);
        let x1 = x0 + i32::from(w);
        let y1 = y0 + i32::from(h);

        let clip_x0 = x0.max(0);
        let clip_y0 = y0.max(0);
        let clip_x1 = x1.min(i32::from(UI_W));
        let clip_y1 = y1.min(i32::from(UI_H));

        if clip_x0 >= clip_x1 || clip_y0 >= clip_y1 {
            return Ok(());
        }

        self.painter.fill_rect(
            clip_x0 as u16,
            clip_y0 as u16,
            (clip_x1 - clip_x0) as u16,
            (clip_y1 - clip_y0) as u16,
            rgb565,
        )
    }
}

#[allow(dead_code)]
pub fn render_tps_test_status<P: UiPainter>(
    painter: &mut P,
    model: &UiModel,
    variant: UiVariant,
    snapshot: &TpsTestUiSnapshot,
) -> Result<(), P::Error> {
    let palette = palette_for(variant);

    fill(painter, 0, 0, UI_W, UI_H, palette.bg)?;
    draw_background_grid(painter, palette)?;
    draw_outline(painter, 0, 0, UI_W, UI_H, palette.border)?;

    fill(painter, 0, 0, UI_W, HEADER_H, palette.panel)?;
    text(
        painter,
        variant,
        FontRole::DetailTitle,
        format_args!(
            "{}  {:>1}.{:01}A  A{}  B{}",
            snapshot.vout_profile.label(),
            snapshot.ilim_ma / 1000,
            (snapshot.ilim_ma % 1000) / 100,
            tps_test_bool_compact(snapshot.out_a.requested_enabled),
            tps_test_bool_compact(snapshot.out_b.requested_enabled),
        ),
        Point::new(8, 2),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    text(
        painter,
        variant,
        FontRole::DetailTitle,
        tps_test_build_label(snapshot.build_profile),
        Point::new((UI_W - 8) as i32, 2),
        HorizontalAlignment::Right,
        if snapshot.footer_alert.is_some() {
            palette.touch
        } else {
            palette.accent
        },
    )?;

    render_tps_test_charger_card(painter, variant, palette, snapshot, model.frame_no)?;
    fill(
        painter,
        8,
        68,
        UI_W - 16,
        1,
        fade_color(palette.panel, palette.border),
    )?;
    fill(
        painter,
        160,
        72,
        1,
        UI_H - 80,
        fade_color(palette.panel, palette.border),
    )?;
    render_tps_test_output_card(
        painter,
        variant,
        palette,
        "OUT-A",
        8,
        72,
        146,
        90,
        snapshot.vout_profile,
        snapshot.out_a,
        palette.up,
    )?;
    render_tps_test_output_card(
        painter,
        variant,
        palette,
        "OUT-B",
        168,
        72,
        144,
        90,
        snapshot.vout_profile,
        snapshot.out_b,
        palette.down,
    )?;
    render_tps_test_footer(painter, variant, palette, snapshot)?;

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
            UpsMode::Off | UpsMode::Blocked => load_ma,
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
        UpsMode::Off | UpsMode::Standby | UpsMode::Blocked => 0,
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
    draw_dashboard_home_wifi_icon(painter, palette, WifiSnapshot::disabled(), data.frame_no)?;

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
        UpsMode::Off | UpsMode::Blocked => {
            text(
                painter,
                variant,
                FontRole::TextBody,
                if matches!(data.mode, UpsMode::Blocked) {
                    "OUTPUT BLOCKED"
                } else {
                    "BYPASS ACTIVE"
                },
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
            UpsMode::Blocked => "LOCK",
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
        UpsMode::Off | UpsMode::Supplement | UpsMode::Blocked => "LOCK",
    };
    let discharge_status = match data.mode {
        UpsMode::Off => "BYP",
        UpsMode::Standby => "IDLE",
        UpsMode::Supplement => "ASSIST",
        UpsMode::Backup => "LOAD",
        UpsMode::Blocked => "LOCK",
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
    match dashboard_route {
        DashboardRoute::Detail(page) => {
            return render_dashboard_detail_page(painter, variant, palette, data, page);
        }
        DashboardRoute::ManualCharge => {
            return render_dashboard_manual_charge_page(painter, variant, palette, data);
        }
        DashboardRoute::Home => {}
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
    let battery_flow_charge_note = battery_flow_charge_state_text(data);
    let battery_flow_charge_note_color = detail_status_color(palette, battery_flow_charge_note);
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
    draw_dashboard_home_wifi_icon(painter, palette, data.detail.wifi, data.frame_no)?;

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
        UpsMode::Off | UpsMode::Blocked => {
            text(
                painter,
                variant,
                FontRole::TextBody,
                if matches!(data.mode, UpsMode::Blocked) {
                    "OUTPUT BLOCKED"
                } else {
                    "BYPASS ACTIVE"
                },
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
                battery_flow_charge_note,
                Point::new(194, 132),
                HorizontalAlignment::Right,
                battery_flow_charge_note_color,
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
                battery_flow_charge_note,
                Point::new(194, 132),
                HorizontalAlignment::Right,
                battery_flow_charge_note_color,
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
    let charge_note = home_charge_state_text(data);
    let charge_note_color = detail_status_color(palette, charge_note);
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
                UpsMode::Blocked => "LOCK",
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
            active: charger_active_value(data).unwrap_or(false),
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
        DashboardDetailPage::BmsDetail => palette.left,
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
        DashboardDetailPage::Wifi => dashboard_wifi_accent(palette, data.detail.wifi),
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

    if page == DashboardDetailPage::BmsDetail {
        render_dashboard_bms_detail_page(painter, variant, palette, data, accent)?;
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
        draw_bms_detail_footer_reason(
            painter,
            variant,
            palette,
            data,
            detail_status_color(palette, status),
        )?;
        return Ok(());
    }

    let (left_panel_x, left_panel_w, right_panel_x, right_panel_w) = match page {
        DashboardDetailPage::Wifi => (6, 172, 186, 128),
        _ => (6, 150, 164, 150),
    };

    draw_panel(painter, 6, 22, 308, 38, palette, true, accent)?;
    draw_panel(
        painter,
        left_panel_x,
        60,
        left_panel_w,
        82,
        palette,
        false,
        accent,
    )?;
    draw_panel(
        painter,
        right_panel_x,
        60,
        right_panel_w,
        82,
        palette,
        false,
        accent,
    )?;
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
        DashboardDetailPage::BmsDetail => unreachable!(),
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
        DashboardDetailPage::Wifi => render_dashboard_wifi_detail(painter, variant, palette, data)?,
    }

    draw_dashboard_detail_footer_notice(painter, variant, palette, page, data)?;

    Ok(())
}

fn detail_balance_summary_text(detail: DashboardDetailSnapshot) -> &'static str {
    match detail.balance_enabled {
        Some(false) => "OFF",
        Some(true) => match detail.balance_active {
            Some(false) => "IDLE",
            Some(true) => match detail.balance_mask.map(|mask| mask & 0x0F) {
                Some(0b0001) => "C1",
                Some(0b0010) => "C2",
                Some(0b0100) => "C3",
                Some(0b1000) => "C4",
                Some(mask) if mask.count_ones() > 1 => "MULTI",
                _ => "ACTIVE",
            },
            None => "N/A",
        },
        None => "N/A",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BmsDetailStateTone {
    Ok,
    Warn,
    Fault,
    Off,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BalanceCellVisualState {
    Active,
    Inactive,
    Unknown,
}

fn bms_detail_state_tone(
    value: Option<bool>,
    true_tone: BmsDetailStateTone,
    false_tone: BmsDetailStateTone,
) -> BmsDetailStateTone {
    match value {
        Some(true) => true_tone,
        Some(false) => false_tone,
        None => BmsDetailStateTone::Unknown,
    }
}

fn bms_detail_reason_text(detail: DashboardDetailSnapshot) -> &'static str {
    detail.reason_label.unwrap_or("STATUS N/A")
}

fn bms_detail_ready(data: DashboardLiveData) -> bool {
    data.bms_state == SelfCheckCommState::Err
        || data.bms_no_battery == Some(true)
        || data.detail.remcap_mah.is_some()
        || data.detail.fcc_mah.is_some()
        || data.detail.learn_qen.is_some()
        || data.detail.learn_vok.is_some()
        || data.detail.learn_rest.is_some()
        || data.detail.balance_cfg_match.is_some()
        || data.detail.charge_ready.is_some()
        || data.detail.discharge_ready.is_some()
        || data.detail.xchg.is_some()
        || data.detail.xdsg.is_some()
        || data.detail.charge_fet_on.is_some()
        || data.detail.discharge_fet_on.is_some()
        || data.detail.fc.is_some()
        || data.detail.fd.is_some()
        || data.detail.pf.is_some()
        || data.detail.rca_alarm.is_some()
        || data.detail.reason_label.is_some()
        || data.detail.balance_mask.is_some()
}

fn bms_detail_balance_cell_state(
    detail: DashboardDetailSnapshot,
    cell_idx: usize,
) -> BalanceCellVisualState {
    match (
        detail.balance_active,
        detail.balance_mask.map(|mask| mask & 0x0F),
    ) {
        (Some(true), Some(mask)) => {
            if mask & (1 << cell_idx) != 0 {
                BalanceCellVisualState::Active
            } else {
                BalanceCellVisualState::Inactive
            }
        }
        (Some(true), None) | (None, _) => BalanceCellVisualState::Unknown,
        _ => BalanceCellVisualState::Inactive,
    }
}

fn bms_detail_status_tone_color(palette: Palette, tone: BmsDetailStateTone) -> u16 {
    match tone {
        BmsDetailStateTone::Ok => SUCCESS_COLOR,
        BmsDetailStateTone::Warn => ATTENTION_COLOR,
        BmsDetailStateTone::Fault => ERROR_COLOR,
        BmsDetailStateTone::Off | BmsDetailStateTone::Unknown => palette.text_dim,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BmsDetailStateGlyph {
    BatteryCharge,
    BatteryDischarge,
    PathBlocked,
    PathAllowed,
    Unknown,
    FetOn,
    FetOff,
    BatteryFull,
    BatteryEmpty,
    BatteryAlert,
    WarningTriangle,
}

const LUCIDE_LOCK_22: &[(u8, u8, u8, u8)] = &[
    (8, 1, 6, 1),
    (7, 2, 8, 1),
    (6, 3, 3, 1),
    (13, 3, 3, 1),
    (5, 4, 3, 1),
    (14, 4, 3, 1),
    (5, 5, 3, 1),
    (14, 5, 3, 1),
    (5, 6, 3, 1),
    (14, 6, 3, 1),
    (5, 7, 3, 1),
    (14, 7, 3, 1),
    (5, 8, 3, 1),
    (14, 8, 3, 1),
    (3, 9, 16, 1),
    (2, 10, 18, 1),
    (2, 11, 2, 1),
    (18, 11, 2, 1),
    (1, 12, 3, 1),
    (18, 12, 3, 1),
    (1, 13, 3, 1),
    (18, 13, 3, 1),
    (1, 14, 3, 1),
    (18, 14, 3, 1),
    (1, 15, 3, 1),
    (18, 15, 3, 1),
    (1, 16, 3, 1),
    (18, 16, 3, 1),
    (1, 17, 3, 1),
    (18, 17, 3, 1),
    (2, 18, 2, 1),
    (18, 18, 2, 1),
    (2, 19, 18, 1),
    (2, 20, 18, 1),
];

const LUCIDE_LOCK_OPEN_22: &[(u8, u8, u8, u8)] = &[
    (8, 1, 6, 1),
    (7, 2, 8, 1),
    (6, 3, 3, 1),
    (12, 3, 4, 1),
    (5, 4, 3, 1),
    (14, 4, 3, 1),
    (5, 5, 3, 1),
    (14, 5, 3, 1),
    (5, 6, 3, 1),
    (15, 6, 1, 1),
    (5, 7, 3, 1),
    (5, 8, 3, 1),
    (3, 9, 16, 1),
    (2, 10, 18, 1),
    (2, 11, 2, 1),
    (18, 11, 2, 1),
    (1, 12, 3, 1),
    (18, 12, 3, 1),
    (1, 13, 3, 1),
    (18, 13, 3, 1),
    (1, 14, 3, 1),
    (18, 14, 3, 1),
    (1, 15, 3, 1),
    (18, 15, 3, 1),
    (1, 16, 3, 1),
    (18, 16, 3, 1),
    (1, 17, 3, 1),
    (18, 17, 3, 1),
    (2, 18, 2, 1),
    (18, 18, 2, 1),
    (2, 19, 18, 1),
    (2, 20, 18, 1),
];

const LUCIDE_TOGGLE_LEFT_22: &[(u8, u8, u8, u8)] = &[
    (7, 3, 8, 1),
    (4, 4, 14, 1),
    (3, 5, 16, 1),
    (2, 6, 4, 1),
    (16, 6, 4, 1),
    (1, 7, 3, 1),
    (6, 7, 4, 1),
    (18, 7, 3, 1),
    (1, 8, 3, 1),
    (5, 8, 6, 1),
    (18, 8, 3, 1),
    (1, 9, 2, 1),
    (5, 9, 7, 1),
    (19, 9, 2, 1),
    (1, 10, 2, 1),
    (4, 10, 3, 1),
    (10, 10, 2, 1),
    (19, 10, 2, 1),
    (1, 11, 2, 1),
    (4, 11, 3, 1),
    (10, 11, 2, 1),
    (19, 11, 2, 1),
    (1, 12, 2, 1),
    (5, 12, 7, 1),
    (19, 12, 2, 1),
    (1, 13, 3, 1),
    (5, 13, 6, 1),
    (18, 13, 3, 1),
    (1, 14, 3, 1),
    (6, 14, 4, 1),
    (17, 14, 4, 1),
    (2, 15, 4, 1),
    (16, 15, 4, 1),
    (3, 16, 16, 1),
    (4, 17, 14, 1),
    (7, 18, 8, 1),
];

const LUCIDE_TOGGLE_RIGHT_22: &[(u8, u8, u8, u8)] = &[
    (7, 3, 8, 1),
    (4, 4, 14, 1),
    (3, 5, 16, 1),
    (2, 6, 4, 1),
    (16, 6, 4, 1),
    (1, 7, 3, 1),
    (12, 7, 4, 1),
    (18, 7, 3, 1),
    (1, 8, 3, 1),
    (11, 8, 6, 1),
    (18, 8, 3, 1),
    (1, 9, 2, 1),
    (10, 9, 7, 1),
    (19, 9, 2, 1),
    (1, 10, 2, 1),
    (10, 10, 2, 1),
    (15, 10, 3, 1),
    (19, 10, 2, 1),
    (1, 11, 2, 1),
    (10, 11, 2, 1),
    (15, 11, 3, 1),
    (19, 11, 2, 1),
    (1, 12, 2, 1),
    (10, 12, 7, 1),
    (19, 12, 2, 1),
    (1, 13, 3, 1),
    (11, 13, 6, 1),
    (18, 13, 3, 1),
    (1, 14, 3, 1),
    (12, 14, 4, 1),
    (17, 14, 4, 1),
    (2, 15, 4, 1),
    (16, 15, 4, 1),
    (3, 16, 16, 1),
    (4, 17, 14, 1),
    (7, 18, 8, 1),
];

const LUCIDE_TRIANGLE_ALERT_22: &[(u8, u8, u8, u8)] = &[
    (9, 2, 4, 1),
    (8, 3, 6, 1),
    (7, 4, 3, 1),
    (12, 4, 3, 1),
    (7, 5, 3, 1),
    (12, 5, 3, 1),
    (6, 6, 3, 1),
    (13, 6, 3, 1),
    (6, 7, 3, 1),
    (10, 7, 2, 1),
    (13, 7, 3, 1),
    (5, 8, 3, 1),
    (10, 8, 2, 1),
    (14, 8, 3, 1),
    (5, 9, 3, 1),
    (10, 9, 2, 1),
    (14, 9, 3, 1),
    (4, 10, 3, 1),
    (10, 10, 2, 1),
    (15, 10, 3, 1),
    (3, 11, 3, 1),
    (10, 11, 2, 1),
    (16, 11, 3, 1),
    (3, 12, 3, 1),
    (10, 12, 2, 1),
    (16, 12, 3, 1),
    (2, 13, 3, 1),
    (17, 13, 3, 1),
    (2, 14, 3, 1),
    (10, 14, 2, 1),
    (17, 14, 3, 1),
    (1, 15, 3, 1),
    (10, 15, 2, 1),
    (18, 15, 3, 1),
    (1, 16, 3, 1),
    (10, 16, 2, 1),
    (18, 16, 3, 1),
    (1, 17, 2, 1),
    (19, 17, 2, 1),
    (1, 18, 20, 1),
    (1, 19, 19, 1),
    (4, 20, 14, 1),
];

const LUCIDE_BATTERY_FULL_22: &[(u8, u8, u8, u8)] = &[
    (2, 4, 14, 1),
    (1, 5, 16, 1),
    (1, 6, 17, 1),
    (1, 7, 2, 1),
    (15, 7, 3, 1),
    (1, 8, 2, 1),
    (4, 8, 3, 1),
    (8, 8, 2, 1),
    (12, 8, 2, 1),
    (15, 8, 3, 1),
    (19, 8, 2, 1),
    (1, 9, 2, 1),
    (4, 9, 3, 1),
    (8, 9, 2, 1),
    (12, 9, 2, 1),
    (15, 9, 3, 1),
    (19, 9, 2, 1),
    (1, 10, 2, 1),
    (4, 10, 3, 1),
    (8, 10, 2, 1),
    (12, 10, 2, 1),
    (15, 10, 3, 1),
    (19, 10, 2, 1),
    (1, 11, 2, 1),
    (4, 11, 3, 1),
    (8, 11, 2, 1),
    (12, 11, 2, 1),
    (15, 11, 3, 1),
    (19, 11, 2, 1),
    (1, 12, 2, 1),
    (4, 12, 3, 1),
    (8, 12, 2, 1),
    (12, 12, 2, 1),
    (15, 12, 3, 1),
    (19, 12, 2, 1),
    (1, 13, 2, 1),
    (4, 13, 3, 1),
    (8, 13, 2, 1),
    (12, 13, 2, 1),
    (15, 13, 3, 1),
    (19, 13, 2, 1),
    (1, 14, 2, 1),
    (15, 14, 3, 1),
    (1, 15, 17, 1),
    (1, 16, 16, 1),
    (3, 17, 13, 1),
];

const LUCIDE_BATTERY_LOW_22: &[(u8, u8, u8, u8)] = &[
    (2, 4, 14, 1),
    (1, 5, 16, 1),
    (1, 6, 17, 1),
    (1, 7, 2, 1),
    (15, 7, 3, 1),
    (1, 8, 2, 1),
    (4, 8, 3, 1),
    (15, 8, 3, 1),
    (19, 8, 2, 1),
    (1, 9, 2, 1),
    (4, 9, 3, 1),
    (15, 9, 3, 1),
    (19, 9, 2, 1),
    (1, 10, 2, 1),
    (4, 10, 3, 1),
    (15, 10, 3, 1),
    (19, 10, 2, 1),
    (1, 11, 2, 1),
    (4, 11, 3, 1),
    (15, 11, 3, 1),
    (19, 11, 2, 1),
    (1, 12, 2, 1),
    (4, 12, 3, 1),
    (15, 12, 3, 1),
    (19, 12, 2, 1),
    (1, 13, 2, 1),
    (4, 13, 3, 1),
    (15, 13, 3, 1),
    (19, 13, 2, 1),
    (1, 14, 2, 1),
    (15, 14, 3, 1),
    (1, 15, 17, 1),
    (1, 16, 16, 1),
    (3, 17, 13, 1),
];

const LUCIDE_BATTERY_WARNING_22: &[(u8, u8, u8, u8)] = &[
    (2, 4, 4, 1),
    (12, 4, 4, 1),
    (1, 5, 6, 1),
    (8, 5, 2, 1),
    (12, 5, 5, 1),
    (1, 6, 5, 1),
    (8, 6, 2, 1),
    (12, 6, 6, 1),
    (1, 7, 2, 1),
    (8, 7, 2, 1),
    (15, 7, 3, 1),
    (1, 8, 2, 1),
    (8, 8, 2, 1),
    (15, 8, 3, 1),
    (19, 8, 2, 1),
    (1, 9, 2, 1),
    (8, 9, 2, 1),
    (15, 9, 3, 1),
    (19, 9, 2, 1),
    (1, 10, 2, 1),
    (8, 10, 2, 1),
    (15, 10, 3, 1),
    (19, 10, 2, 1),
    (1, 11, 2, 1),
    (8, 11, 2, 1),
    (15, 11, 3, 1),
    (19, 11, 2, 1),
    (1, 12, 2, 1),
    (8, 12, 2, 1),
    (15, 12, 3, 1),
    (19, 12, 2, 1),
    (1, 13, 2, 1),
    (15, 13, 3, 1),
    (19, 13, 2, 1),
    (1, 14, 2, 1),
    (9, 14, 1, 1),
    (15, 14, 3, 1),
    (1, 15, 5, 1),
    (8, 15, 2, 1),
    (12, 15, 6, 1),
    (1, 16, 6, 1),
    (8, 16, 2, 1),
    (12, 16, 5, 1),
    (3, 17, 3, 1),
    (12, 17, 4, 1),
];

const BMS_FLOW_CHARGE_22: &[(u8, u8, u8, u8)] = &[
    (7, 0, 12, 1),
    (6, 1, 14, 1),
    (6, 2, 2, 1),
    (18, 2, 2, 1),
    (6, 3, 2, 1),
    (18, 3, 2, 1),
    (6, 4, 2, 1),
    (18, 4, 2, 1),
    (6, 5, 2, 1),
    (18, 5, 2, 1),
    (6, 6, 2, 1),
    (18, 6, 2, 1),
    (6, 7, 2, 1),
    (9, 7, 2, 1),
    (18, 7, 4, 1),
    (9, 8, 3, 1),
    (18, 8, 4, 1),
    (0, 9, 13, 1),
    (18, 9, 4, 1),
    (0, 10, 14, 1),
    (18, 10, 4, 1),
    (0, 11, 6, 1),
    (9, 11, 4, 1),
    (18, 11, 4, 1),
    (9, 12, 3, 1),
    (18, 12, 4, 1),
    (6, 13, 2, 1),
    (9, 13, 2, 1),
    (18, 13, 4, 1),
    (6, 14, 2, 1),
    (18, 14, 2, 1),
    (6, 15, 2, 1),
    (18, 15, 2, 1),
    (6, 16, 2, 1),
    (18, 16, 2, 1),
    (6, 17, 2, 1),
    (18, 17, 2, 1),
    (6, 18, 2, 1),
    (18, 18, 2, 1),
    (6, 19, 14, 1),
    (7, 20, 12, 1),
];

const BMS_FLOW_DISCHARGE_22: &[(u8, u8, u8, u8)] = &[
    (1, 0, 12, 1),
    (0, 1, 14, 1),
    (0, 2, 2, 1),
    (12, 2, 2, 1),
    (0, 3, 2, 1),
    (12, 3, 2, 1),
    (0, 4, 2, 1),
    (12, 4, 2, 1),
    (0, 5, 2, 1),
    (12, 5, 2, 1),
    (0, 6, 2, 1),
    (12, 6, 2, 1),
    (0, 7, 2, 1),
    (12, 7, 2, 1),
    (16, 7, 1, 1),
    (0, 8, 2, 1),
    (16, 8, 2, 1),
    (0, 9, 2, 1),
    (16, 9, 3, 1),
    (0, 10, 2, 1),
    (7, 10, 14, 1),
    (0, 11, 2, 1),
    (7, 11, 13, 1),
    (0, 12, 2, 1),
    (16, 12, 3, 1),
    (0, 13, 2, 1),
    (13, 13, 1, 1),
    (16, 13, 2, 1),
    (0, 14, 2, 1),
    (12, 14, 2, 1),
    (16, 14, 1, 1),
    (0, 15, 2, 1),
    (12, 15, 2, 1),
    (0, 16, 2, 1),
    (12, 16, 2, 1),
    (0, 17, 2, 1),
    (12, 17, 2, 1),
    (0, 18, 2, 1),
    (12, 18, 2, 1),
    (0, 19, 2, 1),
    (12, 19, 2, 1),
    (0, 20, 14, 1),
    (1, 21, 12, 1),
];

const BMS_PATH_ALLOWED_22: &[(u8, u8, u8, u8)] = &[
    (1, 8, 3, 1),
    (8, 8, 7, 1),
    (18, 8, 3, 1),
    (0, 9, 2, 1),
    (3, 9, 1, 1),
    (7, 9, 8, 1),
    (18, 9, 1, 1),
    (20, 9, 2, 1),
    (0, 10, 1, 1),
    (4, 10, 14, 1),
    (21, 10, 1, 1),
    (0, 11, 2, 1),
    (3, 11, 1, 1),
    (7, 11, 8, 1),
    (18, 11, 1, 1),
    (20, 11, 2, 1),
    (1, 12, 3, 1),
    (8, 12, 7, 1),
    (18, 12, 3, 1),
];

const BMS_PATH_BLOCKED_22: &[(u8, u8, u8, u8)] = &[
    (1, 8, 3, 1),
    (10, 8, 2, 1),
    (18, 8, 3, 1),
    (0, 9, 2, 1),
    (3, 9, 2, 1),
    (10, 9, 2, 1),
    (17, 9, 2, 1),
    (20, 9, 2, 1),
    (0, 10, 1, 1),
    (4, 10, 5, 1),
    (10, 10, 2, 1),
    (13, 10, 5, 1),
    (21, 10, 1, 1),
    (0, 11, 2, 1),
    (3, 11, 2, 1),
    (10, 11, 2, 1),
    (17, 11, 2, 1),
    (20, 11, 2, 1),
    (1, 12, 3, 1),
    (10, 12, 2, 1),
    (18, 12, 3, 1),
];

fn draw_bms_glyph_battery_charge<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    rgb565: u16,
) -> Result<(), P::Error> {
    draw_icon_blocks(painter, x, y, BMS_FLOW_CHARGE_22, rgb565)
}

fn draw_bms_glyph_battery_discharge<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    rgb565: u16,
) -> Result<(), P::Error> {
    draw_icon_blocks(painter, x, y, BMS_FLOW_DISCHARGE_22, rgb565)
}

fn draw_bms_glyph_lock<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    closed: bool,
    rgb565: u16,
) -> Result<(), P::Error> {
    let blocks = if closed {
        LUCIDE_LOCK_22
    } else {
        LUCIDE_LOCK_OPEN_22
    };
    draw_icon_blocks(painter, x, y, blocks, rgb565)
}

fn draw_bms_glyph_path_terminal<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    rgb565: u16,
) -> Result<(), P::Error> {
    fill(painter, x + 2, y, 3, 1, rgb565)?;
    fill(painter, x + 1, y + 1, 5, 1, rgb565)?;
    fill(painter, x, y + 2, 2, 3, rgb565)?;
    fill(painter, x + 5, y + 2, 2, 3, rgb565)?;
    fill(painter, x + 1, y + 5, 5, 1, rgb565)?;
    fill(painter, x + 2, y + 6, 3, 1, rgb565)
}

fn draw_bms_glyph_path_enabled<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    rgb565: u16,
) -> Result<(), P::Error> {
    draw_icon_blocks(painter, x, y, BMS_PATH_ALLOWED_22, rgb565)
}

fn draw_bms_glyph_path_blocked<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    rgb565: u16,
) -> Result<(), P::Error> {
    draw_icon_blocks(painter, x, y, BMS_PATH_BLOCKED_22, rgb565)
}

fn draw_bms_glyph_unknown<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    rgb565: u16,
) -> Result<(), P::Error> {
    draw_icon_blocks_centered(painter, x, y, 22, 22, CARBON_HELP_18, rgb565)
}

fn draw_bms_glyph_fet_base<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    rgb565: u16,
) -> Result<(), P::Error> {
    fill(painter, x + 1, y + 10, 5, 2, rgb565)?;
    fill(painter, x + 7, y + 6, 8, 1, rgb565)?;
    fill(painter, x + 6, y + 7, 10, 8, rgb565)?;
    fill(painter, x + 7, y + 15, 8, 1, rgb565)?;
    fill(painter, x + 16, y + 10, 5, 2, rgb565)?;
    fill(painter, x + 10, y + 2, 2, 5, rgb565)
}

fn draw_bms_glyph_fet_on<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    rgb565: u16,
) -> Result<(), P::Error> {
    draw_bms_glyph_fet_base(painter, x, y, rgb565)
}

fn draw_bms_glyph_fet_off<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    fg: u16,
    bg: u16,
) -> Result<(), P::Error> {
    draw_bms_glyph_fet_base(painter, x, y, fg)?;

    // keep the slash in the same glyph color, but carve a 1px gutter on both
    // sides so the strike-through stays visually separated from the device body.
    const SLASH_SEGMENTS: &[(u16, u16)] = &[
        (5, 18),
        (6, 16),
        (7, 14),
        (8, 12),
        (9, 10),
        (10, 8),
        (11, 6),
        (12, 4),
        (13, 2),
        (14, 0),
    ];

    for &(sx, sy) in SLASH_SEGMENTS {
        fill(painter, x + sx - 1, y + sy, 1, 2, bg)?;
        fill(painter, x + sx, y + sy, 1, 2, fg)?;
        fill(painter, x + sx + 1, y + sy, 1, 2, bg)?;
    }

    Ok(())
}

fn draw_bms_glyph_warning_triangle<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    rgb565: u16,
) -> Result<(), P::Error> {
    draw_icon_blocks(painter, x, y, LUCIDE_TRIANGLE_ALERT_22, rgb565)
}

fn draw_bms_glyph_battery_alert<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    rgb565: u16,
) -> Result<(), P::Error> {
    draw_icon_blocks(painter, x, y, LUCIDE_BATTERY_WARNING_22, rgb565)
}

fn draw_bms_state_glyph<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    glyph: BmsDetailStateGlyph,
    fg: u16,
    bg: u16,
) -> Result<(), P::Error> {
    match glyph {
        BmsDetailStateGlyph::BatteryCharge => draw_bms_glyph_battery_charge(painter, x, y, fg),
        BmsDetailStateGlyph::BatteryDischarge => {
            draw_bms_glyph_battery_discharge(painter, x, y, fg)
        }
        BmsDetailStateGlyph::PathBlocked => draw_bms_glyph_path_blocked(painter, x, y, fg),
        BmsDetailStateGlyph::PathAllowed => draw_bms_glyph_path_enabled(painter, x, y, fg),
        BmsDetailStateGlyph::Unknown => draw_bms_glyph_unknown(painter, x, y, fg),
        BmsDetailStateGlyph::FetOn => draw_bms_glyph_fet_on(painter, x, y, fg),
        BmsDetailStateGlyph::FetOff => draw_bms_glyph_fet_off(painter, x, y, fg, bg),
        BmsDetailStateGlyph::BatteryFull => {
            draw_icon_blocks(painter, x, y, LUCIDE_BATTERY_FULL_22, fg)
        }
        BmsDetailStateGlyph::BatteryEmpty => {
            draw_icon_blocks(painter, x, y, LUCIDE_BATTERY_LOW_22, fg)
        }
        BmsDetailStateGlyph::BatteryAlert => draw_bms_glyph_battery_alert(painter, x, y, fg),
        BmsDetailStateGlyph::WarningTriangle => draw_bms_glyph_warning_triangle(painter, x, y, fg),
    }
}

fn draw_bms_detail_metric<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    x: u16,
    w: u16,
    label: &'static str,
    value_mah: Option<u16>,
) -> Result<(), P::Error> {
    text(
        painter,
        variant,
        FontRole::TextBody,
        label,
        Point::new((x + 4) as i32, 26),
        HorizontalAlignment::Left,
        palette.bg,
    )?;
    match value_mah {
        Some(value_mah) => text(
            painter,
            variant,
            FontRole::DetailNum,
            format_args!("{:>4}mAh", value_mah),
            Point::new((x + w.saturating_sub(4)) as i32, 42),
            HorizontalAlignment::Right,
            palette.bg,
        ),
        None => text(
            painter,
            variant,
            FontRole::DetailNum,
            "N/A",
            Point::new((x + w.saturating_sub(4)) as i32, 42),
            HorizontalAlignment::Right,
            palette.bg,
        ),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BmsDetailSummaryBadge {
    label: &'static str,
    tone: BmsDetailStateTone,
}

fn bms_detail_learn_badge(detail: DashboardDetailSnapshot) -> BmsDetailSummaryBadge {
    match (detail.learn_qen, detail.learn_vok, detail.learn_rest) {
        (Some(false), _, _) => BmsDetailSummaryBadge {
            label: "LEARN OFF",
            tone: BmsDetailStateTone::Off,
        },
        (Some(true), _, Some(true)) => BmsDetailSummaryBadge {
            label: "LEARN REST",
            tone: BmsDetailStateTone::Warn,
        },
        (Some(true), Some(true), _) => BmsDetailSummaryBadge {
            label: "LEARN OK",
            tone: BmsDetailStateTone::Ok,
        },
        (Some(true), Some(false), Some(false)) => BmsDetailSummaryBadge {
            label: "LEARN WAIT",
            tone: BmsDetailStateTone::Warn,
        },
        _ => BmsDetailSummaryBadge {
            label: "LEARN ?",
            tone: BmsDetailStateTone::Unknown,
        },
    }
}

fn bms_detail_balance_cfg_badge(detail: DashboardDetailSnapshot) -> BmsDetailSummaryBadge {
    match detail.balance_cfg_match {
        Some(true) => BmsDetailSummaryBadge {
            label: "BALCFG OK",
            tone: BmsDetailStateTone::Ok,
        },
        Some(false) => BmsDetailSummaryBadge {
            label: "BALCFG MIS",
            tone: BmsDetailStateTone::Fault,
        },
        None => BmsDetailSummaryBadge {
            label: "BALCFG ?",
            tone: BmsDetailStateTone::Unknown,
        },
    }
}

fn bms_detail_path_glyph(blocked: Option<bool>) -> BmsDetailStateGlyph {
    match blocked {
        Some(true) => BmsDetailStateGlyph::PathBlocked,
        Some(false) => BmsDetailStateGlyph::PathAllowed,
        None => BmsDetailStateGlyph::Unknown,
    }
}

fn bms_detail_fet_glyph(on: Option<bool>) -> BmsDetailStateGlyph {
    match on {
        Some(true) => BmsDetailStateGlyph::FetOn,
        Some(false) => BmsDetailStateGlyph::FetOff,
        None => BmsDetailStateGlyph::Unknown,
    }
}

fn draw_bms_detail_summary_badge<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    badge: BmsDetailSummaryBadge,
) -> Result<(), P::Error> {
    let border = bms_detail_status_tone_color(palette, badge.tone);
    fill(painter, x, y, w, h, palette.panel_alt)?;
    draw_outline(painter, x, y, w, h, fade_color(border, palette.border))?;
    text(
        painter,
        variant,
        FontRole::TextCompact,
        badge.label,
        Point::new((x + w / 2) as i32, (y + 1) as i32),
        HorizontalAlignment::Center,
        border,
    )
}

fn draw_bms_detail_balance_summary<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    data: DashboardLiveData,
) -> Result<(), P::Error> {
    text(
        painter,
        variant,
        FontRole::TextBody,
        "BAL",
        Point::new(220, 26),
        HorizontalAlignment::Left,
        palette.bg,
    )?;

    for idx in 0..4 {
        let x = 246 + (idx as u16) * 15;
        let label = match idx {
            0 => "1",
            1 => "2",
            2 => "3",
            _ => "4",
        };
        draw_bms_balance_cell_icon(
            painter,
            variant,
            palette,
            x,
            24,
            label,
            bms_detail_balance_cell_state(data.detail, idx),
        )?;
    }

    Ok(())
}

fn draw_bms_detail_balance_cluster<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    data: DashboardLiveData,
) -> Result<(), P::Error> {
    text(
        painter,
        variant,
        FontRole::TextCompact,
        "BAL",
        Point::new(184, 68),
        HorizontalAlignment::Left,
        palette.text_dim,
    )?;

    for idx in 0..4 {
        let x = 209 + (idx as u16) * 22;
        let label = match idx {
            0 => "1",
            1 => "2",
            2 => "3",
            _ => "4",
        };
        draw_bms_balance_cell_icon_large(
            painter,
            variant,
            palette,
            x,
            72,
            label,
            bms_detail_balance_cell_state(data.detail, idx),
        )?;
    }
    Ok(())
}

fn draw_bms_balance_cell_icon<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    x: u16,
    y: u16,
    label: &'static str,
    state: BalanceCellVisualState,
) -> Result<(), P::Error> {
    let (border, fill_color, text_color) = match state {
        BalanceCellVisualState::Active => (
            SUCCESS_COLOR,
            fade_color(SUCCESS_COLOR, palette.bg),
            palette.bg,
        ),
        BalanceCellVisualState::Inactive => (
            palette.text_dim,
            fade_color(palette.panel, palette.bg),
            palette.text,
        ),
        BalanceCellVisualState::Unknown => (
            ATTENTION_COLOR,
            fade_color(ATTENTION_COLOR, palette.bg),
            palette.bg,
        ),
    };
    fill(painter, x + 4, y + 1, 4, 2, border)?;
    draw_outline(painter, x + 1, y + 3, 10, 12, border)?;
    fill(painter, x + 3, y + 5, 6, 8, fill_color)?;
    text(
        painter,
        variant,
        FontRole::TextCompact,
        label,
        Point::new((x + 6) as i32, (y + 6) as i32),
        HorizontalAlignment::Center,
        text_color,
    )
}

fn draw_bms_balance_cell_icon_large<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    x: u16,
    y: u16,
    label: &'static str,
    state: BalanceCellVisualState,
) -> Result<(), P::Error> {
    let (border, fill_color) = match state {
        BalanceCellVisualState::Active => (SUCCESS_COLOR, fade_color(SUCCESS_COLOR, palette.bg)),
        BalanceCellVisualState::Inactive => {
            (palette.text_dim, fade_color(palette.panel, palette.bg))
        }
        BalanceCellVisualState::Unknown => {
            (ATTENTION_COLOR, fade_color(ATTENTION_COLOR, palette.bg))
        }
    };
    fill(painter, x + 4, y, 6, 2, border)?;
    draw_outline(painter, x + 1, y + 2, 12, 14, border)?;
    fill(painter, x + 3, y + 4, 8, 10, fill_color)?;
    text(
        painter,
        variant,
        FontRole::TextCompact,
        label,
        Point::new((x + 7) as i32, (y + 19) as i32),
        HorizontalAlignment::Center,
        palette.text,
    )
}

fn bms_detail_footer_reason(data: DashboardLiveData) -> &'static str {
    if data.bms_state == SelfCheckCommState::Err {
        "BMS LINK FAULT"
    } else if data.bms_no_battery == Some(true) {
        "NO BATTERY"
    } else if data.detail.reason_label.is_some() {
        bms_detail_reason_text(data.detail)
    } else {
        data.page_notice(DashboardDetailPage::BmsDetail)
    }
}

fn draw_bms_detail_footer_reason<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    data: DashboardLiveData,
    status_color: u16,
) -> Result<(), P::Error> {
    let reason_text = bms_detail_footer_reason(data);
    let fill_color = fade_color(status_color, palette.panel_alt);
    fill(painter, 10, 150, 300, 12, fill_color)?;
    draw_outline(painter, 10, 150, 300, 12, status_color)?;
    text(
        painter,
        variant,
        FontRole::DetailBody,
        reason_text,
        Point::new(160, 150),
        HorizontalAlignment::Center,
        palette.text,
    )
}

fn draw_bms_detail_state_tile<P: UiPainter>(
    painter: &mut P,
    palette: Palette,
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    glyph: BmsDetailStateGlyph,
    tone: BmsDetailStateTone,
) -> Result<(), P::Error> {
    let tint = match tone {
        BmsDetailStateTone::Ok => fade_color(SUCCESS_COLOR, palette.panel_alt),
        BmsDetailStateTone::Warn => fade_color(ATTENTION_COLOR, palette.panel_alt),
        BmsDetailStateTone::Fault => fade_color(ERROR_COLOR, palette.panel_alt),
        BmsDetailStateTone::Off | BmsDetailStateTone::Unknown => {
            fade_color(palette.panel_alt, palette.bg)
        }
    };
    let border = fade_color(bms_detail_status_tone_color(palette, tone), palette.border);
    fill(painter, x + 3, y, w.saturating_sub(6), h, tint)?;
    fill(painter, x, y + 3, w, h.saturating_sub(6), tint)?;
    fill(painter, x + 3, y, w.saturating_sub(6), 1, border)?;
    fill(
        painter,
        x + 3,
        y + h.saturating_sub(1),
        w.saturating_sub(6),
        1,
        border,
    )?;
    fill(painter, x, y + 3, 1, h.saturating_sub(6), border)?;
    fill(
        painter,
        x + w.saturating_sub(1),
        y + 3,
        1,
        h.saturating_sub(6),
        border,
    )?;
    fill(painter, x + 1, y + 1, 1, 1, border)?;
    fill(painter, x + 2, y + 1, 1, 1, border)?;
    fill(painter, x + 1, y + 2, 1, 1, border)?;
    fill(painter, x + w.saturating_sub(2), y + 1, 1, 1, border)?;
    fill(painter, x + w.saturating_sub(3), y + 1, 1, 1, border)?;
    fill(painter, x + w.saturating_sub(2), y + 2, 1, 1, border)?;
    fill(painter, x + 1, y + h.saturating_sub(2), 1, 1, border)?;
    fill(painter, x + 2, y + h.saturating_sub(2), 1, 1, border)?;
    fill(painter, x + 1, y + h.saturating_sub(3), 1, 1, border)?;
    fill(
        painter,
        x + w.saturating_sub(2),
        y + h.saturating_sub(2),
        1,
        1,
        border,
    )?;
    fill(
        painter,
        x + w.saturating_sub(3),
        y + h.saturating_sub(2),
        1,
        1,
        border,
    )?;
    fill(
        painter,
        x + w.saturating_sub(2),
        y + h.saturating_sub(3),
        1,
        1,
        border,
    )?;
    draw_bms_state_glyph(
        painter,
        x + (w.saturating_sub(22)) / 2,
        y + (h.saturating_sub(22)) / 2,
        glyph,
        bms_detail_status_tone_color(palette, tone),
        tint,
    )
}

fn draw_bms_detail_state_text_tile<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    label: &'static str,
    tone: BmsDetailStateTone,
) -> Result<(), P::Error> {
    let tint = match tone {
        BmsDetailStateTone::Ok => fade_color(SUCCESS_COLOR, palette.panel_alt),
        BmsDetailStateTone::Warn => fade_color(ATTENTION_COLOR, palette.panel_alt),
        BmsDetailStateTone::Fault => fade_color(ERROR_COLOR, palette.panel_alt),
        BmsDetailStateTone::Off | BmsDetailStateTone::Unknown => {
            fade_color(palette.panel_alt, palette.bg)
        }
    };
    let border = fade_color(bms_detail_status_tone_color(palette, tone), palette.border);
    fill(painter, x + 3, y, w.saturating_sub(6), h, tint)?;
    fill(painter, x, y + 3, w, h.saturating_sub(6), tint)?;
    fill(painter, x + 3, y, w.saturating_sub(6), 1, border)?;
    fill(
        painter,
        x + 3,
        y + h.saturating_sub(1),
        w.saturating_sub(6),
        1,
        border,
    )?;
    fill(painter, x, y + 3, 1, h.saturating_sub(6), border)?;
    fill(
        painter,
        x + w.saturating_sub(1),
        y + 3,
        1,
        h.saturating_sub(6),
        border,
    )?;
    fill(painter, x + 1, y + 1, 1, 1, border)?;
    fill(painter, x + 2, y + 1, 1, 1, border)?;
    fill(painter, x + 1, y + 2, 1, 1, border)?;
    fill(painter, x + w.saturating_sub(2), y + 1, 1, 1, border)?;
    fill(painter, x + w.saturating_sub(3), y + 1, 1, 1, border)?;
    fill(painter, x + w.saturating_sub(2), y + 2, 1, 1, border)?;
    fill(painter, x + 1, y + h.saturating_sub(2), 1, 1, border)?;
    fill(painter, x + 2, y + h.saturating_sub(2), 1, 1, border)?;
    fill(painter, x + 1, y + h.saturating_sub(3), 1, 1, border)?;
    fill(
        painter,
        x + w.saturating_sub(2),
        y + h.saturating_sub(2),
        1,
        1,
        border,
    )?;
    fill(
        painter,
        x + w.saturating_sub(3),
        y + h.saturating_sub(2),
        1,
        1,
        border,
    )?;
    fill(
        painter,
        x + w.saturating_sub(2),
        y + h.saturating_sub(3),
        1,
        1,
        border,
    )?;
    text(
        painter,
        variant,
        FontRole::TextCompact,
        label,
        Point::new((x + w / 2) as i32, (y + 10) as i32),
        HorizontalAlignment::Center,
        bms_detail_status_tone_color(palette, tone),
    )
}

fn render_dashboard_bms_detail_page<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    data: DashboardLiveData,
    accent: u16,
) -> Result<(), P::Error> {
    draw_panel(painter, 6, 22, 308, 38, palette, true, accent)?;
    fill(painter, 108, 24, 1, 34, fade_color(palette.bg, accent))?;
    fill(painter, 208, 24, 1, 34, fade_color(palette.bg, accent))?;
    draw_bms_detail_metric(
        painter,
        variant,
        palette,
        10,
        94,
        "REMCAP",
        data.detail.remcap_mah,
    )?;
    draw_bms_detail_metric(
        painter,
        variant,
        palette,
        110,
        94,
        "FCC",
        data.detail.fcc_mah,
    )?;
    draw_bms_detail_summary_badge(
        painter,
        variant,
        palette,
        216,
        25,
        88,
        14,
        bms_detail_learn_badge(data.detail),
    )?;
    draw_bms_detail_summary_badge(
        painter,
        variant,
        palette,
        216,
        41,
        88,
        14,
        bms_detail_balance_cfg_badge(data.detail),
    )?;
    draw_panel(painter, 6, 62, 308, 82, palette, false, accent)?;
    fill(painter, 176, 66, 1, 74, fade_color(palette.border, accent))?;
    text(
        painter,
        variant,
        FontRole::TextCompact,
        "CHG",
        Point::new(14, 78),
        HorizontalAlignment::Left,
        palette.text_dim,
    )?;
    text(
        painter,
        variant,
        FontRole::TextCompact,
        "DSG",
        Point::new(14, 112),
        HorizontalAlignment::Left,
        palette.text_dim,
    )?;
    draw_bms_detail_balance_cluster(painter, variant, palette, data)?;
    let charge_tiles = [
        (
            BmsDetailStateGlyph::BatteryCharge,
            bms_detail_state_tone(
                data.detail.charge_ready,
                BmsDetailStateTone::Ok,
                BmsDetailStateTone::Warn,
            ),
        ),
        (
            bms_detail_path_glyph(data.detail.xchg),
            bms_detail_state_tone(
                data.detail.xchg,
                BmsDetailStateTone::Fault,
                BmsDetailStateTone::Ok,
            ),
        ),
        (
            bms_detail_fet_glyph(data.detail.charge_fet_on),
            bms_detail_state_tone(
                data.detail.charge_fet_on,
                BmsDetailStateTone::Ok,
                BmsDetailStateTone::Off,
            ),
        ),
    ];
    let discharge_tiles = [
        (
            BmsDetailStateGlyph::BatteryDischarge,
            bms_detail_state_tone(
                data.detail.discharge_ready,
                BmsDetailStateTone::Ok,
                BmsDetailStateTone::Warn,
            ),
        ),
        (
            bms_detail_path_glyph(data.detail.xdsg),
            bms_detail_state_tone(
                data.detail.xdsg,
                BmsDetailStateTone::Fault,
                BmsDetailStateTone::Ok,
            ),
        ),
        (
            bms_detail_fet_glyph(data.detail.discharge_fet_on),
            bms_detail_state_tone(
                data.detail.discharge_fet_on,
                BmsDetailStateTone::Ok,
                BmsDetailStateTone::Off,
            ),
        ),
    ];
    let flag_tiles = [
        (
            BmsDetailStateGlyph::BatteryFull,
            bms_detail_state_tone(
                data.detail.fc,
                BmsDetailStateTone::Warn,
                BmsDetailStateTone::Off,
            ),
        ),
        (
            BmsDetailStateGlyph::WarningTriangle,
            bms_detail_state_tone(
                data.detail.pf,
                BmsDetailStateTone::Fault,
                BmsDetailStateTone::Off,
            ),
        ),
        (
            BmsDetailStateGlyph::BatteryEmpty,
            bms_detail_state_tone(
                data.detail.fd,
                BmsDetailStateTone::Fault,
                BmsDetailStateTone::Off,
            ),
        ),
        (
            BmsDetailStateGlyph::BatteryAlert,
            bms_detail_state_tone(
                data.detail.rca_alarm,
                BmsDetailStateTone::Warn,
                BmsDetailStateTone::Off,
            ),
        ),
    ];

    for (idx, (glyph, tone)) in charge_tiles.into_iter().enumerate() {
        let tile_x = 46 + idx as u16 * 35;
        draw_bms_detail_state_tile(painter, palette, tile_x, 70, 30, 30, glyph, tone)?;
    }
    for (idx, (glyph, tone)) in discharge_tiles.into_iter().enumerate() {
        let tile_x = 46 + idx as u16 * 35;
        draw_bms_detail_state_tile(painter, palette, tile_x, 104, 30, 30, glyph, tone)?;
    }
    for (idx, (glyph, tone)) in flag_tiles.into_iter().enumerate() {
        let tile_x = 181 + idx as u16 * 33;
        draw_bms_detail_state_tile(painter, palette, tile_x, 105, 28, 28, glyph, tone)?;
    }

    Ok(())
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
    draw_charger_protocol_badge(painter, variant, palette, data.detail, 46, 60)?;
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
        "IBAT",
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
    draw_dashboard_entry_marker(
        painter,
        DASHBOARD_CHARGER_MANUAL_ENTRY_X,
        DASHBOARD_CHARGER_MANUAL_ENTRY_Y,
        DASHBOARD_CHARGER_MANUAL_ENTRY_W,
        DASHBOARD_CHARGER_MANUAL_ENTRY_H,
        palette.right,
    )?;
    Ok(())
}

fn render_dashboard_manual_charge_page<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    data: DashboardLiveData,
) -> Result<(), P::Error> {
    let status = detail_status_tag(DashboardDetailPage::Charger, data);
    let status_color = detail_status_color(palette, status);
    let prefs = data.detail.manual_charge.prefs;
    let settings_locked = manual_charge_settings_locked(data);
    let action_label = manual_charge_action_label(data);
    let (action_fill, action_border, action_text) =
        manual_charge_action_style(action_label, palette);

    draw_manual_charge_top_bar(
        painter,
        variant,
        palette,
        manual_charge_mode_text(data),
        status,
        status_color,
    )?;

    draw_manual_option_row(
        painter,
        variant,
        palette,
        MANUAL_TARGET_ROW_Y,
        "TARGET",
        [
            (
                prefs.target == ManualChargeTarget::Pack3V7,
                ManualChargeTarget::Pack3V7.label(),
            ),
            (
                prefs.target == ManualChargeTarget::Rsoc80,
                ManualChargeTarget::Rsoc80.label(),
            ),
            (
                prefs.target == ManualChargeTarget::Full100,
                ManualChargeTarget::Full100.label(),
            ),
        ],
        settings_locked,
        palette.left,
    )?;
    draw_manual_option_row(
        painter,
        variant,
        palette,
        MANUAL_SPEED_ROW_Y,
        "SPEED",
        [
            (
                prefs.speed == ManualChargeSpeed::Ma100,
                ManualChargeSpeed::Ma100.label(),
            ),
            (
                prefs.speed == ManualChargeSpeed::Ma500,
                ManualChargeSpeed::Ma500.label(),
            ),
            (
                prefs.speed == ManualChargeSpeed::Ma1000,
                ManualChargeSpeed::Ma1000.label(),
            ),
        ],
        settings_locked,
        palette.right,
    )?;
    draw_manual_option_row(
        painter,
        variant,
        palette,
        MANUAL_TIMER_ROW_Y,
        "TIMER",
        [
            (
                prefs.timer_limit == ManualChargeTimerLimit::H1,
                ManualChargeTimerLimit::H1.label(),
            ),
            (
                prefs.timer_limit == ManualChargeTimerLimit::H2,
                ManualChargeTimerLimit::H2.label(),
            ),
            (
                prefs.timer_limit == ManualChargeTimerLimit::H6,
                ManualChargeTimerLimit::H6.label(),
            ),
        ],
        settings_locked,
        palette.center,
    )?;

    draw_panel(
        painter,
        MANUAL_BACK_X,
        MANUAL_BACK_Y,
        MANUAL_BACK_W,
        MANUAL_BACK_H,
        palette,
        false,
        palette.accent,
    )?;
    draw_panel(
        painter,
        MANUAL_STATUS_X,
        MANUAL_STATUS_Y,
        MANUAL_STATUS_W,
        MANUAL_STATUS_H,
        palette,
        true,
        status_color,
    )?;
    draw_manual_action_button(
        painter,
        MANUAL_ACTION_X,
        MANUAL_ACTION_Y,
        MANUAL_ACTION_W,
        MANUAL_ACTION_H,
        action_fill,
        action_border,
    )?;
    text_with_position(
        painter,
        variant,
        FontRole::DetailBody,
        "BACK",
        Point::new(
            (MANUAL_BACK_X + MANUAL_BACK_W / 2) as i32,
            (MANUAL_BACK_Y + MANUAL_BACK_H / 2) as i32,
        ),
        VerticalPosition::Center,
        HorizontalAlignment::Center,
        palette.text,
    )?;
    text_with_position(
        painter,
        variant,
        FontRole::TextBody,
        manual_charge_footer_text(data),
        Point::new(
            (MANUAL_STATUS_X + MANUAL_STATUS_W / 2) as i32,
            (MANUAL_STATUS_Y + MANUAL_STATUS_H / 2) as i32,
        ),
        VerticalPosition::Center,
        HorizontalAlignment::Center,
        palette.bg,
    )?;
    text_with_position(
        painter,
        variant,
        FontRole::DetailBody,
        action_label,
        Point::new(
            (MANUAL_ACTION_X + MANUAL_ACTION_W / 2) as i32,
            (MANUAL_ACTION_Y + MANUAL_ACTION_H / 2) as i32,
        ),
        VerticalPosition::Center,
        HorizontalAlignment::Center,
        action_text,
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
        "CTRL",
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

fn render_dashboard_wifi_detail<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    data: DashboardLiveData,
) -> Result<(), P::Error> {
    const WIFI_DETAIL_LEFT_X: u16 = 14;
    const WIFI_DETAIL_LEFT_W: u16 = 156;
    const WIFI_DETAIL_RIGHT_X: u16 = 194;
    const WIFI_DETAIL_RIGHT_W: u16 = 112;

    let wifi = data.detail.wifi;
    let status = wifi_detail_status_tag(wifi);
    let summary_color = if status == "OFF" {
        palette.text_dim
    } else {
        palette.bg
    };

    text(
        painter,
        variant,
        FontRole::DetailBody,
        "WIFI",
        Point::new(14, 26),
        HorizontalAlignment::Left,
        palette.bg,
    )?;
    text(
        painter,
        variant,
        FontRole::NumBig,
        status,
        Point::new(214, 30),
        HorizontalAlignment::Right,
        summary_color,
    )?;
    draw_dashboard_wifi_icon_at(painter, 248, 28, 14, palette, wifi, data.frame_no)?;

    match wifi.ipv4 {
        Some([a, b, c, d]) => text(
            painter,
            variant,
            FontRole::Num,
            format_args!("IP {}.{}.{}.{}", a, b, c, d),
            Point::new(308, 46),
            HorizontalAlignment::Right,
            palette.bg,
        )?,
        None => text(
            painter,
            variant,
            FontRole::DetailBody,
            wifi_summary_line(wifi),
            Point::new(308, 46),
            HorizontalAlignment::Right,
            palette.bg,
        )?,
    }

    text(
        painter,
        variant,
        FontRole::DetailBody,
        "NETWORK",
        Point::new(WIFI_DETAIL_LEFT_X as i32, 64),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    draw_detail_ip_row(
        painter,
        variant,
        palette,
        WIFI_DETAIL_LEFT_X,
        DETAIL_ROW_Y_1,
        WIFI_DETAIL_LEFT_W,
        "IPV4",
        wifi.ipv4,
    )?;
    draw_detail_ip_row(
        painter,
        variant,
        palette,
        WIFI_DETAIL_LEFT_X,
        DETAIL_ROW_Y_2,
        WIFI_DETAIL_LEFT_W,
        "GATE",
        wifi.gateway,
    )?;
    draw_detail_ip_row(
        painter,
        variant,
        palette,
        WIFI_DETAIL_LEFT_X,
        DETAIL_ROW_Y_3,
        WIFI_DETAIL_LEFT_W,
        "DNS",
        wifi.dns,
    )?;
    draw_detail_text_row_with_width(
        painter,
        variant,
        palette,
        WIFI_DETAIL_LEFT_X,
        DETAIL_ROW_Y_4,
        WIFI_DETAIL_LEFT_W,
        "CFG",
        DetailTextValue::Static(
            if matches!(
                wifi.state,
                WifiConnectionState::Disabled | WifiConnectionState::Idle
            ) && wifi.ipv4.is_none()
                && wifi.gateway.is_none()
                && wifi.dns.is_none()
            {
                "N/A"
            } else if wifi.is_static {
                "STATIC"
            } else {
                "DHCP"
            },
        ),
    )?;

    text(
        painter,
        variant,
        FontRole::DetailBody,
        "RADIO",
        Point::new(WIFI_DETAIL_RIGHT_X as i32, 64),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    draw_detail_text_row_with_width(
        painter,
        variant,
        palette,
        WIFI_DETAIL_RIGHT_X,
        DETAIL_ROW_Y_1,
        WIFI_DETAIL_RIGHT_W,
        "STATE",
        DetailTextValue::Static(wifi_state_label(wifi.state)),
    )?;
    draw_detail_rssi_row(
        painter,
        variant,
        palette,
        WIFI_DETAIL_RIGHT_X,
        DETAIL_ROW_Y_2,
        WIFI_DETAIL_RIGHT_W,
        wifi.rssi_dbm,
    )?;
    draw_detail_text_row_with_width(
        painter,
        variant,
        palette,
        WIFI_DETAIL_RIGHT_X,
        DETAIL_ROW_Y_3,
        WIFI_DETAIL_RIGHT_W,
        "ERROR",
        DetailTextValue::Static(wifi_error_label(wifi)),
    )?;
    draw_detail_mac_tail_row(
        painter,
        variant,
        palette,
        WIFI_DETAIL_RIGHT_X,
        DETAIL_ROW_Y_4,
        WIFI_DETAIL_RIGHT_W,
        wifi.mac,
    )?;

    Ok(())
}

fn detail_page_title(page: DashboardDetailPage) -> &'static str {
    match page {
        DashboardDetailPage::Cells => "CELL DETAIL",
        DashboardDetailPage::BmsDetail => "BMS DETAIL",
        DashboardDetailPage::BatteryFlow => "BATTERY FLOW",
        DashboardDetailPage::Output => "OUTPUT DETAIL",
        DashboardDetailPage::Charger => "CHARGER DETAIL",
        DashboardDetailPage::Thermal => "THERMAL DETAIL",
        DashboardDetailPage::Wifi => "WIFI DETAIL",
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
            } else if data.detail.balance_enabled == Some(false) {
                "OFF"
            } else if data.detail.balance_active == Some(true) {
                "BAL ON"
            } else {
                "READY"
            }
        }
        DashboardDetailPage::BmsDetail => {
            if data.bms_state == SelfCheckCommState::Err
                || data.detail.pf == Some(true)
                || data.detail.reason_key == Some("sbs_error_code")
                || data.detail.reason_key == Some("permanent_failure")
            {
                "FAULT"
            } else if data.bms_no_battery == Some(true) {
                "LIMIT"
            } else if data.detail.reason_key == Some("op_status_unavailable") {
                "N/A"
            } else if !bms_detail_ready(data) {
                "N/A"
            } else if data.bms_recovery_pending
                || data.detail.xchg == Some(true)
                || data.detail.xdsg == Some(true)
                || data.detail.charge_ready == Some(false)
                || data.detail.discharge_ready == Some(false)
            {
                "LIMIT"
            } else if data.bms_state == SelfCheckCommState::Warn
                || data.detail.fd == Some(true)
                || data.detail.rca_alarm == Some(true)
            {
                "WARN"
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
            } else if let Some(status) = data.detail.charger_status {
                status
            } else {
                charger_state_text(data)
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
        DashboardDetailPage::Wifi => wifi_detail_status_tag(data.detail.wifi),
    }
}

fn manual_charge_mode_text(data: DashboardLiveData) -> &'static str {
    let runtime = data.detail.manual_charge.runtime;
    if runtime.active {
        if runtime.takeover {
            "TAKEOVER"
        } else {
            "MANUAL"
        }
    } else if charger_active_value(data) == Some(true) {
        "AUTO CHG"
    } else if runtime.stop_inhibit {
        "STOPPED"
    } else {
        "AUTO"
    }
}

fn manual_charge_control_active(data: DashboardLiveData) -> bool {
    data.detail.manual_charge.runtime.active
}

const fn manual_charge_stop_footer_text(reason: ManualChargeStopReason) -> &'static str {
    match reason {
        ManualChargeStopReason::UserStop | ManualChargeStopReason::None => "AUTO HELD",
        ManualChargeStopReason::TimerExpired => "TIMER DONE",
        ManualChargeStopReason::PackReached => "3.7V DONE",
        ManualChargeStopReason::RsocReached => "80% DONE",
        ManualChargeStopReason::FullReached => "100% DONE",
        ManualChargeStopReason::SafetyBlocked => "SAFETY STOP",
    }
}

fn manual_charge_settings_locked(data: DashboardLiveData) -> bool {
    manual_charge_control_active(data)
}

fn manual_charge_action_label(data: DashboardLiveData) -> &'static str {
    if manual_charge_control_active(data) {
        "STOP"
    } else {
        "START"
    }
}

fn manual_charge_action_style(action_label: &'static str, palette: Palette) -> (u16, u16, u16) {
    if action_label == "STOP" {
        (
            fade_color(ERROR_COLOR, palette.panel_alt),
            fade_color(ERROR_COLOR, palette.border),
            palette.text,
        )
    } else {
        (palette.accent, palette.accent, palette.bg)
    }
}

fn manual_charge_footer_text(data: DashboardLiveData) -> &'static str {
    let runtime = data.detail.manual_charge.runtime;
    if runtime.active {
        if runtime.loopback_override {
            "LOOP OK"
        } else {
            "MANUAL ACTIVE"
        }
    } else if runtime.stop_inhibit && charger_active_value(data) != Some(true) {
        manual_charge_stop_footer_text(runtime.last_stop_reason)
    } else if let Some(minutes) = runtime.remaining_minutes {
        if minutes > 0 {
            "TIMER ARMED"
        } else {
            "TIMER DONE"
        }
    } else if let Some(notice) = data.detail.charger_notice {
        match notice {
            "charging_500ma" | "charging_100ma_dc_derated" | "charging_1a_manual" => "LIVE DATA",
            "manual_user_stop_inhibit" => "AUTO HELD",
            "manual_timer_expired" => "TIMER DONE",
            "manual_target_pack_reached" => "3.7V DONE",
            "manual_target_rsoc_reached" => "80% DONE",
            "manual_target_full_reached" => "100% DONE",
            "manual_safety_blocked" => "SAFETY STOP",
            _ => "LIVE DATA",
        }
    } else {
        "LIVE DATA"
    }
}

fn detail_status_color(palette: Palette, status: &'static str) -> u16 {
    match status {
        "FAULT" | "HOT" => ERROR_COLOR,
        "WARN" | "WARM" | "LOCK" | "NOAC" | "TEMP" | "LOAD" | "LIMIT" | "HOLD" | "RECOV" => {
            ATTENTION_COLOR
        }
        "OFF" => palette.text_dim,
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

fn draw_dashboard_home_wifi_icon<P: UiPainter>(
    painter: &mut P,
    palette: Palette,
    wifi: WifiSnapshot,
    frame_no: u32,
) -> Result<(), P::Error> {
    draw_dashboard_wifi_icon_at(
        painter,
        DASHBOARD_HOME_WIFI_ICON_X,
        DASHBOARD_HOME_WIFI_ICON_Y,
        14,
        palette,
        wifi,
        frame_no,
    )
}

/// Draw the approved-candidate dashboard marker without changing the runtime
/// dashboard route or alert behavior. The runtime calls will be added only
/// after the owner accepts this scene.
pub fn draw_dashboard_alert_preview_indicator<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    severity: AlertPreviewSeverity,
    sound: AlertPreviewSoundState,
    frame_no: u32,
) -> Result<(), P::Error> {
    let palette = palette_for(variant);
    let color = alert_preview_indicator_color(palette, severity, sound, frame_no);
    draw_icon_blocks_scaled(
        painter,
        DASHBOARD_HOME_ALERT_ICON_X,
        DASHBOARD_HOME_ALERT_ICON_Y,
        LUCIDE_TRIANGLE_ALERT_22,
        22,
        DASHBOARD_HOME_ALERT_ICON_SIZE,
        color,
    )
}

/// Draw the Dashboard alert entry on top of the runtime touch-region overlay.
pub fn draw_dashboard_alert_preview_touch_overlay<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
) -> Result<(), P::Error> {
    let palette = palette_for(variant);
    draw_dashboard_touch_region_overlay(
        painter,
        variant,
        palette,
        DASHBOARD_HOME_ALERT_TOUCH_X,
        DASHBOARD_HOME_ALERT_TOUCH_Y,
        DASHBOARD_HOME_ALERT_TOUCH_W,
        DASHBOARD_HOME_ALERT_TOUCH_H,
        "A",
        palette.center,
        151,
        2,
    )
}

/// Render the alert list interaction using the firmware scene, bitmap fonts,
/// and RGB565 palette.
pub fn render_alert_list_preview<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    alerts: &[AlertPreviewItem],
    selected: usize,
    top: usize,
    touch_overlay: bool,
) -> Result<(), P::Error> {
    let palette = palette_for(variant);
    fill(painter, 0, 0, UI_W, UI_H, palette.bg)?;
    draw_background_grid(painter, palette)?;

    let status = if alerts.is_empty() { "CLEAR" } else { "ACTIVE" };
    let status_color = if alerts.is_empty() {
        palette.accent
    } else {
        alert_preview_severity_color(palette, alerts[0].severity)
    };
    draw_top_bar_with_status(
        painter,
        variant,
        palette,
        UiFocus::Idle,
        "ALERTS",
        "",
        "",
        status_color,
    )?;
    draw_panel(painter, 8, 2, 80, 18, palette, false, palette.accent)?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        "BACK",
        Point::new(48, 4),
        HorizontalAlignment::Center,
        palette.text,
    )?;
    text(
        painter,
        variant,
        FontRole::TextCompact,
        status,
        Point::new(220, 5),
        HorizontalAlignment::Right,
        status_color,
    )?;

    if touch_overlay {
        let back = ALERT_LIST_TOP_BACK_TOUCH;
        draw_dashboard_touch_region_overlay(
            painter,
            variant,
            palette,
            back.x,
            back.y,
            back.w,
            back.h,
            "B",
            palette.left,
            back.x + 2,
            back.y + 2,
        )?;
    }

    if alerts.is_empty() {
        draw_panel(painter, 8, 42, 304, 66, palette, false, palette.accent)?;
        text(
            painter,
            variant,
            FontRole::TextTitle,
            "NO ACTIVE ALERTS",
            Point::new((UI_W / 2) as i32, 57),
            HorizontalAlignment::Center,
            palette.text,
        )?;
        text(
            painter,
            variant,
            FontRole::TextBody,
            "ALL ALERTS CLEARED",
            Point::new((UI_W / 2) as i32, 78),
            HorizontalAlignment::Center,
            palette.text_dim,
        )?;
        draw_alert_preview_footer(painter, variant, palette, "LEFT BACK")?;
        return Ok(());
    }

    for slot in 0..3usize {
        let index = top.saturating_add(slot);
        let Some(alert) = alerts.get(index).copied() else {
            break;
        };
        let y = 24 + (slot as u16) * 36;
        let severity_color = alert_preview_severity_color(palette, alert.severity);
        draw_panel(painter, 8, y, 304, 34, palette, false, severity_color)?;
        if index == selected {
            draw_outline(painter, 8, y, 304, 34, palette.center)?;
        }
        draw_bms_glyph_warning_triangle(painter, 13, y + 5, severity_color)?;
        text(
            painter,
            variant,
            FontRole::TextBody,
            alert.kind.label(),
            Point::new(42, (y + 3) as i32),
            HorizontalAlignment::Left,
            palette.text,
        )?;
        text(
            painter,
            variant,
            FontRole::TextCompact,
            alert.kind.summary(),
            Point::new(42, (y + 18) as i32),
            HorizontalAlignment::Left,
            palette.text_dim,
        )?;
        draw_alert_preview_sound_icon(
            painter,
            278,
            y + 3,
            alert_preview_sound_color(palette, alert),
            fade_color(palette.panel, palette.panel_alt),
            !alert.sound.can_mute(),
        )?;
    }

    let last_visible = top.saturating_add(3).min(alerts.len());
    text(
        painter,
        variant,
        FontRole::NumCompact,
        format_args!("{}/{}", last_visible, alerts.len()),
        Point::new(306, 133),
        HorizontalAlignment::Right,
        palette.text_dim,
    )?;
    draw_alert_preview_footer(
        painter,
        variant,
        palette,
        "UP/DN SELECT  RIGHT MUTE  CENTER DETAIL",
    )?;

    if touch_overlay {
        for slot in 0..3usize {
            let row = ALERT_LIST_ROW_TOUCH[slot];
            let mute = ALERT_LIST_MUTE_TOUCH[slot];
            draw_dashboard_touch_region_overlay(
                painter,
                variant,
                palette,
                row.x,
                row.y,
                row.w,
                row.h,
                match slot {
                    0 => "1",
                    1 => "2",
                    _ => "3",
                },
                palette.touch,
                row.x + 4,
                row.y + 2,
            )?;
            draw_dashboard_touch_region_overlay(
                painter,
                variant,
                palette,
                mute.x,
                mute.y,
                mute.w,
                mute.h,
                "M",
                palette.center,
                mute.x + mute.w - 14,
                mute.y + 2,
            )?;
        }
    }

    Ok(())
}

/// Render one alert instance in the design candidate. A cleared instance is
/// retained on this screen to make the terminal state explicit to the user.
pub fn render_alert_detail_preview<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    alert: AlertPreviewItem,
    touch_overlay: bool,
) -> Result<(), P::Error> {
    let palette = palette_for(variant);
    fill(painter, 0, 0, UI_W, UI_H, palette.bg)?;
    draw_background_grid(painter, palette)?;

    let status = if alert.cleared {
        "CLEARED"
    } else {
        alert.severity.label()
    };
    let status_color = if alert.cleared {
        palette.accent
    } else {
        alert_preview_severity_color(palette, alert.severity)
    };
    draw_dashboard_detail_top_bar(
        painter,
        variant,
        palette,
        "ALERT DETAIL",
        status,
        status_color,
    )?;

    draw_panel(painter, 8, 26, 304, 42, palette, false, status_color)?;
    draw_bms_glyph_warning_triangle(painter, 16, 34, status_color)?;
    text(
        painter,
        variant,
        FontRole::TextTitle,
        alert.kind.label(),
        Point::new(48, 30),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    text(
        painter,
        variant,
        FontRole::TextCompact,
        alert.kind.summary(),
        Point::new(48, 48),
        HorizontalAlignment::Left,
        palette.text_dim,
    )?;

    let sound_label = if alert.cleared {
        "OFF"
    } else {
        alert.sound.label()
    };
    let sound_color = if alert.cleared {
        palette.text_dim
    } else {
        alert_preview_sound_color(palette, alert)
    };
    draw_panel(painter, 8, 74, 304, 30, palette, false, sound_color)?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        "SOUND",
        Point::new(16, 80),
        HorizontalAlignment::Left,
        palette.text_dim,
    )?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        sound_label,
        Point::new(258, 80),
        HorizontalAlignment::Right,
        sound_color,
    )?;
    draw_alert_preview_sound_icon(
        painter,
        274,
        76,
        sound_color,
        fade_color(palette.panel, palette.panel_alt),
        alert.cleared || !alert.sound.can_mute(),
    )?;

    let action = if alert.cleared {
        "ALERT CLEARED"
    } else if alert.sound.can_mute() {
        "MUTE THIS ALERT"
    } else {
        alert.sound.label()
    };
    let action_color = if alert.cleared {
        palette.accent
    } else if alert.sound.can_mute() {
        palette.center
    } else {
        palette.text_dim
    };
    draw_panel(painter, 8, 112, 304, 28, palette, false, action_color)?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        action,
        Point::new((UI_W / 2) as i32, 118),
        HorizontalAlignment::Center,
        action_color,
    )?;
    draw_alert_preview_footer(
        painter,
        variant,
        palette,
        if alert.cleared {
            "LEFT BACK"
        } else if alert.sound.can_mute() {
            "RIGHT MUTE  LEFT BACK"
        } else {
            "LEFT BACK"
        },
    )?;

    if touch_overlay {
        let top_back = ALERT_DETAIL_TOP_BACK_TOUCH;
        draw_dashboard_touch_region_overlay(
            painter,
            variant,
            palette,
            top_back.x,
            top_back.y,
            top_back.w,
            top_back.h,
            "B",
            palette.touch,
            4,
            3,
        )?;
        if !alert.cleared && alert.sound.can_mute() {
            let mute = ALERT_DETAIL_MUTE_TOUCH;
            draw_dashboard_touch_region_overlay(
                painter,
                variant,
                palette,
                mute.x,
                mute.y,
                mute.w,
                mute.h,
                "M",
                palette.center,
                mute.x + 4,
                mute.y + 2,
            )?;
            let action = ALERT_DETAIL_ACTION_TOUCH;
            draw_dashboard_touch_region_overlay(
                painter,
                variant,
                palette,
                action.x,
                action.y,
                action.w,
                action.h,
                "M",
                palette.center,
                action.x + 4,
                action.y + 2,
            )?;
        }
    }

    Ok(())
}

fn alert_preview_severity_color(_palette: Palette, severity: AlertPreviewSeverity) -> u16 {
    match severity {
        AlertPreviewSeverity::Warning => ATTENTION_COLOR,
        AlertPreviewSeverity::Critical => ERROR_COLOR,
    }
}

fn alert_preview_indicator_color(
    palette: Palette,
    severity: AlertPreviewSeverity,
    sound: AlertPreviewSoundState,
    frame_no: u32,
) -> u16 {
    if sound.can_mute() && (frame_no % 2 == 0) {
        palette.text
    } else {
        alert_preview_severity_color(palette, severity)
    }
}

fn alert_preview_sound_color(palette: Palette, alert: AlertPreviewItem) -> u16 {
    if alert.cleared {
        return palette.text_dim;
    }
    match alert.sound {
        AlertPreviewSoundState::Audible => alert_preview_severity_color(palette, alert.severity),
        AlertPreviewSoundState::Muted => palette.center,
        AlertPreviewSoundState::SystemSilent | AlertPreviewSoundState::PolicySilent => {
            palette.text_dim
        }
    }
}

fn draw_alert_preview_footer<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    hint: &'static str,
) -> Result<(), P::Error> {
    fill(painter, 0, 148, UI_W, 24, palette.panel)?;
    text(
        painter,
        variant,
        FontRole::TextCompact,
        hint,
        Point::new((UI_W / 2) as i32, 153),
        HorizontalAlignment::Center,
        palette.text_dim,
    )
}

fn draw_alert_preview_sound_icon<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    fg: u16,
    bg: u16,
    muted: bool,
) -> Result<(), P::Error> {
    draw_icon_blocks(painter, x, y, MENU_ICON_VOLUME_UP_28, fg)?;
    if muted {
        for step in 0..18u16 {
            let sx = x + 5 + step;
            let sy = y + 23 - step;
            fill(painter, sx, sy, 1, 1, bg)?;
            fill(painter, sx + 1, sy, 1, 1, fg)?;
            fill(painter, sx + 2, sy, 1, 1, bg)?;
        }
    }
    Ok(())
}

fn draw_dashboard_wifi_icon_at<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    size: u16,
    palette: Palette,
    wifi: WifiSnapshot,
    frame_no: u32,
) -> Result<(), P::Error> {
    let origin_x = x + size.saturating_sub(14) / 2;
    let origin_y = y + size.saturating_sub(14) / 2;
    match wifi.state {
        WifiConnectionState::Disabled | WifiConnectionState::Idle => {
            draw_icon_blocks(
                painter,
                origin_x,
                origin_y,
                WIFI_OFF_SYMBOL_ROUNDED_14,
                palette.text_dim,
            )?;
        }
        WifiConnectionState::Error => {
            draw_icon_blocks(
                painter,
                origin_x,
                origin_y,
                WIFI_OFF_SYMBOL_ROUNDED_14,
                ERROR_COLOR,
            )?;
        }
        WifiConnectionState::Connecting => {
            let level = connecting_wifi_signal_level(frame_no);
            let active = dashboard_wifi_icon_color(palette, wifi, frame_no);
            draw_wifi_signal_level(painter, origin_x, origin_y, level, active)?;
        }
        WifiConnectionState::Connected => {
            let inactive = fade_color(palette.panel_alt, palette.text_dim);
            draw_icon_blocks(
                painter,
                origin_x,
                origin_y,
                WIFI_SYMBOL_ROUNDED_14,
                inactive,
            )?;
            let level = wifi_signal_level(wifi.rssi_dbm);
            let active = dashboard_connected_wifi_icon_color(palette, level);
            draw_wifi_signal_level(painter, origin_x, origin_y, level, active)?;
        }
    }
    Ok(())
}

fn dashboard_wifi_icon_color(palette: Palette, wifi: WifiSnapshot, _frame_no: u32) -> u16 {
    match wifi.state {
        WifiConnectionState::Connecting => ATTENTION_COLOR,
        WifiConnectionState::Connected => {
            dashboard_connected_wifi_icon_color(palette, wifi_signal_level(wifi.rssi_dbm))
        }
        WifiConnectionState::Disabled | WifiConnectionState::Idle | WifiConnectionState::Error => {
            palette.text_dim
        }
    }
}

fn draw_wifi_signal_level<P: UiPainter>(
    painter: &mut P,
    origin_x: u16,
    origin_y: u16,
    level: u8,
    active: u16,
) -> Result<(), P::Error> {
    draw_icon_blocks(painter, origin_x, origin_y, WIFI_SIGNAL_DOT_14, active)?;
    if level >= 1 {
        draw_icon_blocks(
            painter,
            origin_x,
            origin_y,
            WIFI_SIGNAL_INNER_ARC_14,
            active,
        )?;
    }
    if level >= 2 {
        draw_icon_blocks(
            painter,
            origin_x,
            origin_y,
            WIFI_SIGNAL_OUTER_ARC_14,
            active,
        )?;
    }
    Ok(())
}

const fn connecting_wifi_signal_level(frame_no: u32) -> u8 {
    match (frame_no / 4) % 3 {
        0 => 0,
        1 => 1,
        _ => 2,
    }
}

const fn wifi_signal_level(rssi_dbm: Option<i8>) -> u8 {
    match rssi_dbm {
        None => 2,
        Some(rssi) if rssi >= -60 => 2,
        Some(rssi) if rssi >= -75 => 1,
        Some(_) => 0,
    }
}

const fn dashboard_connected_wifi_icon_color(palette: Palette, level: u8) -> u16 {
    match level {
        0 => ATTENTION_COLOR,
        _ => palette.text,
    }
}

fn dashboard_wifi_accent(palette: Palette, wifi: WifiSnapshot) -> u16 {
    match wifi.state {
        WifiConnectionState::Disabled | WifiConnectionState::Idle => {
            fade_color(palette.panel_alt, palette.border)
        }
        WifiConnectionState::Connecting | WifiConnectionState::Connected => palette.accent,
        WifiConnectionState::Error => ERROR_COLOR,
    }
}

fn wifi_detail_status_tag(wifi: WifiSnapshot) -> &'static str {
    match wifi.state {
        WifiConnectionState::Disabled | WifiConnectionState::Idle => "OFF",
        WifiConnectionState::Connecting => "JOIN",
        WifiConnectionState::Connected => "READY",
        WifiConnectionState::Error => "FAULT",
    }
}

fn wifi_summary_line(wifi: WifiSnapshot) -> &'static str {
    match wifi.state {
        WifiConnectionState::Disabled | WifiConnectionState::Idle => "WIFI NOT ENABLED",
        WifiConnectionState::Connecting => "JOINING ACCESS POINT",
        WifiConnectionState::Connected => "LAN READY FOR API CLIENTS",
        WifiConnectionState::Error => wifi_error_label(wifi),
    }
}

const fn wifi_state_label(state: WifiConnectionState) -> &'static str {
    match state {
        WifiConnectionState::Disabled => "DISABLED",
        WifiConnectionState::Idle => "IDLE",
        WifiConnectionState::Connecting => "CONNECTING",
        WifiConnectionState::Connected => "CONNECTED",
        WifiConnectionState::Error => "ERROR",
    }
}

fn wifi_error_label(wifi: WifiSnapshot) -> &'static str {
    match wifi.last_error {
        Some(error) => error.ui_hint(),
        None => match wifi.state {
            WifiConnectionState::Disabled | WifiConnectionState::Idle => "NOT SET",
            WifiConnectionState::Connecting => "ASSOC",
            WifiConnectionState::Connected => "CLEAR",
            WifiConnectionState::Error => "CHECK WIFI",
        },
    }
}

fn draw_manual_charge_top_bar<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    mode: &'static str,
    status: &'static str,
    status_color: u16,
) -> Result<(), P::Error> {
    fill(painter, 0, 0, UI_W, HEADER_H, palette.panel)?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        mode,
        Point::new(8, 4),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    text(
        painter,
        variant,
        FontRole::DetailTitle,
        "MANUAL",
        Point::new((UI_W / 2) as i32, 2),
        HorizontalAlignment::Center,
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

fn draw_dashboard_touch_region_overlay<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    label: &'static str,
    color: u16,
    label_x: u16,
    label_y: u16,
) -> Result<(), P::Error> {
    draw_outline(painter, x, y, w, h, color)?;
    if w > 2 && h > 2 {
        draw_outline(
            painter,
            x + 1,
            y + 1,
            w - 2,
            h - 2,
            fade_color(color, palette.bg),
        )?;
    }

    let label_w = 10;
    let label_h = 12;
    fill(painter, label_x, label_y, label_w, label_h, color)?;
    text(
        painter,
        variant,
        FontRole::Num,
        label,
        Point::new((label_x + 2) as i32, label_y as i32),
        HorizontalAlignment::Left,
        palette.bg,
    )?;

    Ok(())
}

fn draw_manual_option_row<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    y: u16,
    title: &'static str,
    items: [(bool, &'static str); 3],
    locked: bool,
    accent: u16,
) -> Result<(), P::Error> {
    text_with_position(
        painter,
        variant,
        FontRole::TextBody,
        title,
        Point::new((MANUAL_ROW_X + 8) as i32, (y + MANUAL_ROW_H / 2) as i32),
        VerticalPosition::Center,
        HorizontalAlignment::Left,
        palette.text,
    )?;

    for (idx, (selected, label)) in items.into_iter().enumerate() {
        let cell_x = manual_segment_x(idx as u16);
        let cell_y = y + MANUAL_SEGMENT_Y_INSET;
        let cell_bg = if selected {
            accent
        } else {
            fade_color(palette.panel_alt, palette.bg)
        };
        fill(
            painter,
            cell_x,
            cell_y,
            MANUAL_SEGMENT_W,
            MANUAL_SEGMENT_H,
            cell_bg,
        )?;
        draw_outline(
            painter,
            cell_x,
            cell_y,
            MANUAL_SEGMENT_W,
            MANUAL_SEGMENT_H,
            palette.border,
        )?;
        text_with_position(
            painter,
            variant,
            FontRole::DetailBody,
            label,
            Point::new(
                (cell_x + MANUAL_SEGMENT_W / 2) as i32,
                (cell_y + MANUAL_SEGMENT_H / 2) as i32,
            ),
            VerticalPosition::Center,
            HorizontalAlignment::Center,
            if selected {
                palette.bg
            } else if locked {
                palette.text_dim
            } else {
                palette.text
            },
        )?;
    }

    Ok(())
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
    draw_detail_text_row_with_width(painter, variant, palette, x, y, 132, label, value)
}

fn draw_detail_text_row_with_width<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    x: u16,
    y: u16,
    width: u16,
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
            Point::new((x + width) as i32, y as i32),
            HorizontalAlignment::Right,
            palette.text,
        ),
        DetailTextValue::Percent(value) => text(
            painter,
            variant,
            FontRole::Num,
            format_args!("{:>3}%", value),
            Point::new((x + width) as i32, y as i32),
            HorizontalAlignment::Right,
            palette.text,
        ),
        DetailTextValue::Na => text(
            painter,
            variant,
            FontRole::Num,
            "N/A",
            Point::new((x + width) as i32, y as i32),
            HorizontalAlignment::Right,
            palette.text,
        ),
    }
}

fn draw_detail_ip_row<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    x: u16,
    y: u16,
    width: u16,
    label: &'static str,
    value: Option<[u8; 4]>,
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
        Some([a, b, c, d]) => text(
            painter,
            variant,
            FontRole::Num,
            format_args!("{}.{}.{}.{}", a, b, c, d),
            Point::new((x + width) as i32, y as i32),
            HorizontalAlignment::Right,
            palette.text,
        ),
        None => text(
            painter,
            variant,
            FontRole::Num,
            "N/A",
            Point::new((x + width) as i32, y as i32),
            HorizontalAlignment::Right,
            palette.text,
        ),
    }
}

fn draw_detail_rssi_row<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    x: u16,
    y: u16,
    width: u16,
    rssi_dbm: Option<i8>,
) -> Result<(), P::Error> {
    text(
        painter,
        variant,
        FontRole::TextBody,
        "RSSI",
        Point::new(x as i32, y as i32),
        HorizontalAlignment::Left,
        palette.text_dim,
    )?;
    match rssi_dbm {
        Some(rssi) => text(
            painter,
            variant,
            FontRole::Num,
            format_args!("{}dBm", rssi),
            Point::new((x + width) as i32, y as i32),
            HorizontalAlignment::Right,
            palette.text,
        ),
        None => text(
            painter,
            variant,
            FontRole::Num,
            "N/A",
            Point::new((x + width) as i32, y as i32),
            HorizontalAlignment::Right,
            palette.text,
        ),
    }
}

fn draw_detail_mac_tail_row<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    x: u16,
    y: u16,
    width: u16,
    mac: Option<[u8; 6]>,
) -> Result<(), P::Error> {
    text(
        painter,
        variant,
        FontRole::TextBody,
        "MAC",
        Point::new(x as i32, y as i32),
        HorizontalAlignment::Left,
        palette.text_dim,
    )?;
    match mac {
        Some([_, _, _, d, e, f]) => text(
            painter,
            variant,
            FontRole::Num,
            format_args!("{:02X}:{:02X}:{:02X}", d, e, f),
            Point::new((x + width) as i32, y as i32),
            HorizontalAlignment::Right,
            palette.text,
        ),
        None => text(
            painter,
            variant,
            FontRole::Num,
            "N/A",
            Point::new((x + width) as i32, y as i32),
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
        (_, _, Some("OFF")) => ThermalFanMotion::Off,
        (_, _, Some("HIGH")) => ThermalFanMotion::High,
        (_, _, Some("MID")) => ThermalFanMotion::Mid,
        (_, _, Some("LOW" | "RUN")) => ThermalFanMotion::Low,
        (_, Some(pwm), _) if pwm >= 67 => ThermalFanMotion::High,
        (Some(rpm), _, _) if rpm >= 3_600 => ThermalFanMotion::High,
        (_, Some(pwm), _) if pwm >= 34 => ThermalFanMotion::Mid,
        (Some(rpm), _, _) if rpm >= 1_800 => ThermalFanMotion::Mid,
        (_, Some(pwm), _) if pwm > 0 => ThermalFanMotion::Low,
        (Some(rpm), _, _) if rpm > 0 => ThermalFanMotion::Low,
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
        || data.detail.balance_enabled.is_some()
        || data.detail.balance_cfg_match.is_some()
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
    if data.charger_state == SelfCheckCommState::Err {
        "FAULT"
    } else if data.charger_state == SelfCheckCommState::Warn {
        "WARN"
    } else if let Some(status) = data.detail.charger_status {
        status
    } else if !data.mains_present {
        "NOAC"
    } else if matches!((data.charge_allowed, data.chg_iin_ma), (Some(true), Some(ma)) if ma > 0) {
        "CHG"
    } else if charger_data_ready(data) {
        "WAIT"
    } else {
        "N/A"
    }
}

fn home_charge_state_text(data: DashboardLiveData) -> &'static str {
    fn clamp(status: &'static str) -> Option<&'static str> {
        match status {
            "CHG1A" | "CHG500" | "CHG100" | "RECOV" | "CHG" => Some("CHG"),
            "IDLE" | "READY" | "WAIT" => Some("WAIT"),
            "FULL" | "WARM" | "TEMP" | "LOAD" | "LOCK" | "NOAC" => Some(status),
            _ => None,
        }
    }

    if let Some(status) = data.detail.charger_home_status.and_then(clamp) {
        return status;
    }

    if let Some(status) = data.detail.charger_status.and_then(clamp) {
        return status;
    }

    if let Some(status) = clamp(charger_state_text(data)) {
        return status;
    }

    if !data.mains_present {
        "NOAC"
    } else if matches!(
        data.charger_state,
        SelfCheckCommState::Err | SelfCheckCommState::Warn
    ) {
        "LOCK"
    } else if matches!((data.charge_allowed, data.chg_iin_ma), (Some(true), Some(ma)) if ma > 0) {
        "CHG"
    } else if charger_data_ready(data) {
        "WAIT"
    } else {
        "LOCK"
    }
}

fn battery_flow_charge_state_text(data: DashboardLiveData) -> &'static str {
    home_charge_state_text(data)
}

fn thermal_fault_present(data: DashboardLiveData) -> bool {
    data.therm_a_state == SelfCheckCommState::Err
        || data.therm_b_state == SelfCheckCommState::Err
        || data.detail.fan_status == Some("FAULT")
        || data.detail.thermal_notice == Some("THERM KILL ASSERTED")
}

fn thermal_warn_present(data: DashboardLiveData) -> bool {
    data.therm_a_state == SelfCheckCommState::Warn
        || data.therm_b_state == SelfCheckCommState::Warn
        || data.detail.thermal_notice == Some("TMP HW PROTECT TEST MODE")
}

fn detail_footer_notice(page: DashboardDetailPage, data: DashboardLiveData) -> &'static str {
    if !detail_data_ready(page, data) {
        return data.page_notice(page);
    }

    if page == DashboardDetailPage::Charger
        && matches!(
            data.detail.charger_notice,
            Some(
                "backup_usb_low_output_charge"
                    | "backup_usb_output_high_latched"
                    | "backup_usb_telemetry_lost_latched"
                    | "manual_loopback_confirmed_charging_100ma"
                    | "manual_loopback_confirmed_charging_500ma"
                    | "manual_loopback_confirmed_charging_1a"
            )
        )
    {
        return data.page_notice(page);
    }

    match detail_status_tag(page, data) {
        "FAULT" => detail_fault_notice(page, data),
        "WARN" => "WARNING ACTIVE - CHECK STATUS",
        "LIMIT" => "UPSTREAM PATH LIMITED - CHECK MODULE STATUS",
        "HOLD" => "OUTPUT WAITING FOR BMS DISCHARGE PERMISSION",
        "RECOV" => "RECOVERY IN PROGRESS - HOLD OUTPUTS",
        _ => data.page_notice(page),
    }
}

fn detail_fault_notice(page: DashboardDetailPage, data: DashboardLiveData) -> &'static str {
    match page {
        DashboardDetailPage::BmsDetail => {
            if data.bms_state == SelfCheckCommState::Err {
                "BMS LINK FAULT"
            } else if data.detail.pf == Some(true)
                || data.detail.reason_key == Some("permanent_failure")
            {
                "PERMANENT FAILURE"
            } else if data.detail.reason_key == Some("sbs_error_code") {
                "SBS ERROR ACTIVE"
            } else if !bms_detail_ready(data) {
                "N/A"
            } else {
                data.page_notice(page)
            }
        }
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
        DashboardDetailPage::Wifi => data.page_notice(page),
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

    if page == DashboardDetailPage::Cells && notice == "EXT CHG+RELAX" {
        return (DetailFooterIcon::Live, "BAL CFG");
    }

    if page == DashboardDetailPage::Cells && notice == "CFG MISMATCH" {
        return (DetailFooterIcon::Warn, "CHECK STATUS");
    }

    if page == DashboardDetailPage::Charger {
        return match data.detail.charger_notice {
            Some("backup_usb_low_output_charge") => (DetailFooterIcon::Live, "CHARGING ACTIVE"),
            Some("backup_usb_output_high_latched") => (DetailFooterIcon::Warn, "LOAD: CHG PAUSED"),
            Some("backup_usb_telemetry_lost_latched") => (DetailFooterIcon::Warn, "LOAD DATA LOST"),
            Some("manual_loopback_confirmed_charging_100ma")
            | Some("manual_loopback_confirmed_charging_500ma")
            | Some("manual_loopback_confirmed_charging_1a") => {
                (DetailFooterIcon::Live, "LOOP CHECK OK")
            }
            _ => match status {
                "FAULT" => (DetailFooterIcon::Fault, "LINK FAULT"),
                "WARN" | "HOT" | "WARM" | "LOCK" | "NOAC" | "TEMP" | "LIMIT" | "HOLD" | "RECOV" => {
                    (DetailFooterIcon::Warn, "CHECK STATUS")
                }
                _ if notice.contains("PENDING")
                    || notice.contains("SOURCE")
                    || notice.contains("UI ONLY") =>
                {
                    (DetailFooterIcon::Unknown, "SOURCE NXT")
                }
                _ => (DetailFooterIcon::Live, "LIVE DATA"),
            },
        };
    }

    if page == DashboardDetailPage::Wifi {
        return match data.detail.wifi.state {
            WifiConnectionState::Disabled | WifiConnectionState::Idle => {
                (DetailFooterIcon::Unknown, "WIFI OFF")
            }
            WifiConnectionState::Connecting => (DetailFooterIcon::Warn, "JOINING AP"),
            WifiConnectionState::Connected => (DetailFooterIcon::Live, "LAN READY"),
            WifiConnectionState::Error => (DetailFooterIcon::Fault, "CHECK WIFI"),
        };
    }

    match status {
        "FAULT" => (
            DetailFooterIcon::Fault,
            match page {
                DashboardDetailPage::BatteryFlow if data.bms_state == SelfCheckCommState::Err => {
                    "BMS FAULT"
                }
                DashboardDetailPage::BmsDetail if data.detail.pf == Some(true) => "PF ACTIVE",
                DashboardDetailPage::BmsDetail
                    if data.detail.reason_key == Some("sbs_error_code") =>
                {
                    "SBS ERROR"
                }
                DashboardDetailPage::BmsDetail => "BMS FAULT",
                DashboardDetailPage::BatteryFlow => "PACK ALARM",
                DashboardDetailPage::Charger => "LINK FAULT",
                DashboardDetailPage::Thermal => "SENSE FAULT",
                _ => "FAULT",
            },
        ),
        "WARN" | "HOT" | "WARM" | "LOCK" | "NOAC" | "TEMP" | "LIMIT" | "HOLD" | "RECOV" => {
            (DetailFooterIcon::Warn, "CHECK STATUS")
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
        DashboardDetailPage::Wifi => match data.detail.wifi.state {
            WifiConnectionState::Disabled | WifiConnectionState::Idle => "OFF",
            WifiConnectionState::Connecting => "JOIN",
            WifiConnectionState::Connected => "CLEAR",
            WifiConnectionState::Error => "FAULT",
        },
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
        DashboardDetailPage::BmsDetail => bms_detail_ready(data),
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
        DashboardDetailPage::Wifi => true,
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

    let ichg_ma = snapshot
        .bq25792_ibat_ma
        .and_then(|ma| u16::try_from(ma.max(0)).ok())
        .or(snapshot.bq25792_ichg_ma);
    let ichg_has = ichg_ma.is_some();
    let ichg_ma = ichg_ma.unwrap_or(0);
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
        format_args!("IBAT {:>1}.{:02}A", ichg_whole, ichg_frac)
    } else {
        format_args!("IBAT N/A")
    };
    let bms_issue_key = bq40_issue_card_key(&snapshot);
    let bms_key = if snapshot.bq40z50_recovery_pending {
        format_args!("AUTH ACTIVE")
    } else if snapshot.bq40z50 == SelfCheckCommState::Ok && bms_soc_has {
        format_args!("SOC {:>2}%", bms_soc)
    } else {
        format_args!("{}", bms_issue_key)
    };
    let tps_warning_reason = tps_upstream_warning_reason(&snapshot).unwrap_or("IOUT N/A");
    let tps_a_key = if tps_a_has {
        format_args!("IOUT {}{:>1}.{:02}A", tps_a_sign, tps_a_whole, tps_a_frac)
    } else if display_tps_state(&snapshot, OutputSelector::OutA) == SelfCheckCommState::Warn {
        format_args!("{}", tps_warning_reason)
    } else {
        format_args!("IOUT N/A")
    };
    let tps_b_key = if tps_b_has {
        format_args!("IOUT {}{:>1}.{:02}A", tps_b_sign, tps_b_whole, tps_b_frac)
    } else if display_tps_state(&snapshot, OutputSelector::OutB) == SelfCheckCommState::Warn {
        format_args!("{}", tps_warning_reason)
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
    let tps_a_status_state = display_tps_state(&snapshot, OutputSelector::OutA);
    let tps_b_status_state = display_tps_state(&snapshot, OutputSelector::OutB);

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
                tps_a_status_state,
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
                tps_b_status_state,
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

    draw_self_check_overlay(painter, variant, palette, &snapshot, overlay)?;

    Ok(())
}

fn draw_self_check_overlay<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    snapshot: &SelfCheckUiSnapshot,
    overlay: SelfCheckOverlay,
) -> Result<(), P::Error> {
    if matches!(
        overlay,
        SelfCheckOverlay::None | SelfCheckOverlay::ManualChargeLoopbackConfirm
    ) {
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
        SelfCheckOverlay::BmsActivateConfirm
        | SelfCheckOverlay::BmsActivateProgress
        | SelfCheckOverlay::BmsDischargeAuthorizeConfirm
        | SelfCheckOverlay::BmsDischargeAuthorizeProgress => "BQ40 RECOVERY",
        SelfCheckOverlay::BmsActivateResult(..) => "BQ40 RESULT",
        SelfCheckOverlay::HardwareIssue(..) => "SELF CHECK ISSUE",
        SelfCheckOverlay::None | SelfCheckOverlay::ManualChargeLoopbackConfirm => "",
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
            let copy = bq40_recovery_dialog_copy(snapshot, BmsRecoveryUiAction::Activation);
            draw_bms_confirm_dialog(
                painter,
                variant,
                body_text,
                cancel_border,
                cancel_fill,
                cancel_text,
                confirm_border,
                confirm_fill,
                confirm_text,
                copy,
            )?;
        }
        SelfCheckOverlay::BmsDischargeAuthorizeConfirm => {
            let copy =
                bq40_recovery_dialog_copy(snapshot, BmsRecoveryUiAction::DischargeAuthorization);
            draw_bms_confirm_dialog(
                painter,
                variant,
                body_text,
                cancel_border,
                cancel_fill,
                cancel_text,
                confirm_border,
                confirm_fill,
                confirm_text,
                copy,
            )?;
        }
        SelfCheckOverlay::BmsActivateProgress => {
            let copy = bq40_recovery_dialog_copy(snapshot, BmsRecoveryUiAction::Activation);
            draw_bms_progress_dialog(painter, variant, body_text, copy)?;
        }
        SelfCheckOverlay::BmsDischargeAuthorizeProgress => {
            let copy =
                bq40_recovery_dialog_copy(snapshot, BmsRecoveryUiAction::DischargeAuthorization);
            draw_bms_progress_dialog(painter, variant, body_text, copy)?;
        }
        SelfCheckOverlay::BmsActivateResult(result) => {
            let icon_x = SELF_CHECK_DIALOG_X + 10;
            let icon_y = SELF_CHECK_DIALOG_Y + 28;
            let text_x = SELF_CHECK_DIALOG_X + 50;
            let (headline, body1, body2, accent, icon) = match result {
                BmsResultKind::Success => (
                    "Problem cleared.",
                    "Self-check state refreshed.",
                    "Returning to live status...",
                    SUCCESS_COLOR,
                    ActivationIcon::Success,
                ),
                BmsResultKind::NoBattery => (
                    "Battery not detected.",
                    "Check pack connection.",
                    "Tap to close",
                    ATTENTION_COLOR,
                    ActivationIcon::Failed,
                ),
                BmsResultKind::RomMode => (
                    "Gauge is in ROM mode.",
                    "Use BQ40 tool recovery.",
                    "Tap to close",
                    ATTENTION_COLOR,
                    ActivationIcon::Failed,
                ),
                BmsResultKind::Abnormal => (
                    "Recovery did not clear it.",
                    bq40_issue_detail_body(snapshot),
                    "Tap to close",
                    ATTENTION_COLOR,
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
        SelfCheckOverlay::HardwareIssue(target) => {
            let copy = self_check_hardware_issue_copy(snapshot, target);
            draw_activation_icon(
                painter,
                SELF_CHECK_DIALOG_X + 10,
                SELF_CHECK_DIALOG_Y + 28,
                ActivationIcon::Failed,
            )?;
            text(
                painter,
                variant,
                FontRole::TextBody,
                copy.headline,
                Point::new(
                    (SELF_CHECK_DIALOG_X + 50) as i32,
                    (SELF_CHECK_DIALOG_Y + 28) as i32,
                ),
                HorizontalAlignment::Left,
                copy.accent,
            )?;
            text(
                painter,
                variant,
                FontRole::TextBody,
                copy.body1,
                Point::new(
                    (SELF_CHECK_DIALOG_X + 50) as i32,
                    (SELF_CHECK_DIALOG_Y + 46) as i32,
                ),
                HorizontalAlignment::Left,
                body_text,
            )?;
            text(
                painter,
                variant,
                FontRole::TextBody,
                copy.body2,
                Point::new(
                    (SELF_CHECK_DIALOG_X + 50) as i32,
                    (SELF_CHECK_DIALOG_Y + 84) as i32,
                ),
                HorizontalAlignment::Left,
                palette.text_dim,
            )?;
        }
        SelfCheckOverlay::None | SelfCheckOverlay::ManualChargeLoopbackConfirm => {}
    }

    Ok(())
}

fn draw_dashboard_manual_loopback_confirm_overlay<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
) -> Result<(), P::Error> {
    fill(
        painter,
        0,
        HEADER_H,
        UI_W,
        UI_H - HEADER_H,
        fade_color(palette.bg, 0x0000),
    )?;

    let dialog_border = fade_color(palette.right, palette.border);
    let dialog_fill = fade_color(palette.panel_alt, palette.bg);
    let title_fill = fade_color(dialog_border, palette.bg);
    let divider = fade_color(title_fill, palette.text_dim);
    let cancel_border = fade_color(palette.border, palette.text_dim);
    let cancel_fill = fade_color(palette.panel, palette.panel_alt);
    let confirm_border = fade_color(palette.right, 0x0000);
    let confirm_fill = palette.right;

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
        "USB-C LOOP CHECK",
        Point::new(
            (SELF_CHECK_DIALOG_X + 10) as i32,
            (SELF_CHECK_DIALOG_Y + 4) as i32,
        ),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        "Confirm USB-C IN is not",
        Point::new(
            (SELF_CHECK_DIALOG_X + 10) as i32,
            (SELF_CHECK_DIALOG_Y + 28) as i32,
        ),
        HorizontalAlignment::Left,
        ATTENTION_COLOR,
    )?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        "wired to UPS OUT.",
        Point::new(
            (SELF_CHECK_DIALOG_X + 10) as i32,
            (SELF_CHECK_DIALOG_Y + 46) as i32,
        ),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    draw_manual_action_button(
        painter,
        SELF_CHECK_CANCEL_BTN_X,
        SELF_CHECK_CANCEL_BTN_Y,
        SELF_CHECK_CANCEL_BTN_W,
        SELF_CHECK_CANCEL_BTN_H,
        cancel_fill,
        cancel_border,
    )?;
    draw_manual_action_button(
        painter,
        SELF_CHECK_CONFIRM_BTN_X,
        SELF_CHECK_CONFIRM_BTN_Y,
        SELF_CHECK_CONFIRM_BTN_W,
        SELF_CHECK_CONFIRM_BTN_H,
        confirm_fill,
        confirm_border,
    )?;
    text_with_position(
        painter,
        variant,
        FontRole::Num,
        "CANCEL",
        Point::new(
            (SELF_CHECK_CANCEL_BTN_X + SELF_CHECK_CANCEL_BTN_W / 2) as i32,
            (SELF_CHECK_CANCEL_BTN_Y + SELF_CHECK_CANCEL_BTN_H / 2) as i32,
        ),
        VerticalPosition::Center,
        HorizontalAlignment::Center,
        palette.text,
    )?;
    text_with_position(
        painter,
        variant,
        FontRole::Num,
        "CONFIRM",
        Point::new(
            (SELF_CHECK_CONFIRM_BTN_X + SELF_CHECK_CONFIRM_BTN_W / 2) as i32,
            (SELF_CHECK_CONFIRM_BTN_Y + SELF_CHECK_CONFIRM_BTN_H / 2) as i32,
        ),
        VerticalPosition::Center,
        HorizontalAlignment::Center,
        palette.bg,
    )?;

    Ok(())
}

#[derive(Clone, Copy)]
struct HardwareIssueDialogCopy {
    headline: &'static str,
    body1: &'static str,
    body2: &'static str,
    accent: u16,
}

#[derive(Clone, Copy)]
struct Bq40RecoveryDialogCopy {
    headline: &'static str,
    body1: &'static str,
    body2: &'static str,
    confirm_label: &'static str,
    progress1: &'static str,
    progress2: &'static str,
}

fn self_check_hardware_issue_copy(
    snapshot: &SelfCheckUiSnapshot,
    target: SelfCheckHardwareTarget,
) -> HardwareIssueDialogCopy {
    match target {
        SelfCheckHardwareTarget::Gc9307 => HardwareIssueDialogCopy {
            headline: "GC9307 DISPLAY",
            body1: "Screen path is not ready.",
            body2: "Check panel power and flex.",
            accent: ERROR_COLOR,
        },
        SelfCheckHardwareTarget::Tca6408a => HardwareIssueDialogCopy {
            headline: "TCA6408A IO",
            body1: "Panel IO expander failed.",
            body2: "Check I2C2 and reset lines.",
            accent: ERROR_COLOR,
        },
        SelfCheckHardwareTarget::Fusb302 => HardwareIssueDialogCopy {
            headline: "FUSB302 TYPE-C",
            body1: if snapshot.fusb302_vbus_present == Some(false) {
                "Input VBUS is not present."
            } else {
                "PD controller is not ready."
            },
            body2: "Check Type-C source and CC path.",
            accent: ATTENTION_COLOR,
        },
        SelfCheckHardwareTarget::Ina3221 => HardwareIssueDialogCopy {
            headline: "INA3221 MONITOR",
            body1: "Power monitor is not ready.",
            body2: "Check I2C1 and shunt rails.",
            accent: ERROR_COLOR,
        },
        SelfCheckHardwareTarget::Bq25792 => HardwareIssueDialogCopy {
            headline: "BQ25792 CHARGER",
            body1: if snapshot.bq25792 == SelfCheckCommState::Warn {
                "Charger reports a warning."
            } else {
                "Charger did not initialize."
            },
            body2: "Check input and battery path.",
            accent: ATTENTION_COLOR,
        },
        SelfCheckHardwareTarget::Bq40z50 => HardwareIssueDialogCopy {
            headline: bq40_issue_card_key(snapshot),
            body1: bq40_issue_detail_body(snapshot),
            body2: bq40_issue_detail_footer(snapshot),
            accent: ATTENTION_COLOR,
        },
        SelfCheckHardwareTarget::TpsA => tps_issue_dialog_copy(snapshot, OutputSelector::OutA),
        SelfCheckHardwareTarget::TpsB => tps_issue_dialog_copy(snapshot, OutputSelector::OutB),
        SelfCheckHardwareTarget::TmpA => HardwareIssueDialogCopy {
            headline: "TMP112-A SENSOR",
            body1: "Output A thermal sensor issue.",
            body2: "Check TMP112-A and ALERT path.",
            accent: ATTENTION_COLOR,
        },
        SelfCheckHardwareTarget::TmpB => HardwareIssueDialogCopy {
            headline: "TMP112-B SENSOR",
            body1: "Output B thermal sensor issue.",
            body2: "Check TMP112-B and ALERT path.",
            accent: ATTENTION_COLOR,
        },
    }
}

fn tps_issue_dialog_copy(
    snapshot: &SelfCheckUiSnapshot,
    selector: OutputSelector,
) -> HardwareIssueDialogCopy {
    let headline = match selector {
        OutputSelector::OutA => "TPS55288-A",
        OutputSelector::OutB => "TPS55288-B",
    };
    let state = display_tps_state(snapshot, selector);
    let (body1, body2, accent) = if output_hold_for(snapshot, selector) {
        (
            tps_upstream_warning_reason(snapshot).unwrap_or("Output is held."),
            "Recover BMS before output.",
            ATTENTION_COLOR,
        )
    } else if state == SelfCheckCommState::Warn {
        (
            "Converter reports warning.",
            "Check fault status and load.",
            ATTENTION_COLOR,
        )
    } else {
        (
            "Converter is not reachable.",
            "Check I2C1, VCC and MODE.",
            ERROR_COLOR,
        )
    };
    HardwareIssueDialogCopy {
        headline,
        body1,
        body2,
        accent,
    }
}

fn bq40_issue_card_key(snapshot: &SelfCheckUiSnapshot) -> &'static str {
    match snapshot.bq40z50_issue_detail {
        Some("emshut_active") => "EMSHUT ACTIVE",
        Some("pack_output_path_open") => "PACK PATH OPEN",
        Some("physical_vbat_absent") => "VBAT ABSENT",
        Some("xdsg_blocked") => "XDSG BLOCKED",
        Some("dsg_fet_off") => "DSG FET OFF",
        Some("xchg_blocked") => "CHG BLOCKED",
        Some("cell_undervoltage") => "CELL UV",
        Some("remaining_capacity_alarm") => "RCA ALARM",
        Some("permanent_failure") => "PERM FAIL",
        Some("sleep_mode") => "SLEEP MODE",
        Some("no_battery") => "NO BATTERY",
        _ if snapshot.bq40z50 == SelfCheckCommState::Err => "NOT DETECTED",
        _ if snapshot.bq40z50_no_battery == Some(true) => "NO BATTERY",
        _ if snapshot.bq40z50_discharge_ready == Some(false) => "DSG BLOCKED",
        _ if snapshot.bq40z50_rca_alarm == Some(true) => "RCA ALARM",
        _ if snapshot.bq40z50 == SelfCheckCommState::Warn => "ABNORMAL",
        _ => "SOC N/A",
    }
}

fn bq40_issue_headline(
    snapshot: &SelfCheckUiSnapshot,
    action: BmsRecoveryUiAction,
) -> &'static str {
    match action {
        BmsRecoveryUiAction::Activation => "NOT DETECTED",
        BmsRecoveryUiAction::DischargeAuthorization => match snapshot.bq40z50_issue_detail {
            Some("emshut_active") => "EMSHUT ACTIVE",
            Some("pack_output_path_open") => "PACK PATH OPEN",
            Some("physical_vbat_absent") => "VBAT ABSENT",
            Some("xdsg_blocked") => "XDSG BLOCKED",
            Some("dsg_fet_off") => "DSG FET OFF",
            Some("remaining_capacity_alarm") => "RCA ALARM",
            Some("permanent_failure") => "PERMANENT FAILURE",
            _ => "DISCHARGE BLOCKED",
        },
    }
}

fn bq40_issue_detail_body(snapshot: &SelfCheckUiSnapshot) -> &'static str {
    match snapshot.bq40z50_issue_detail {
        Some("emshut_active") => "Gauge is in emergency shutdown.",
        Some("pack_output_path_open") => "Pack output path is open.",
        Some("physical_vbat_absent") => "Pack is not powering VBAT.",
        Some("xdsg_blocked") => "BQ40 keeps discharge path off.",
        Some("dsg_fet_off") => "Discharge FET is still off.",
        Some("xchg_blocked") => "Charge path is still blocked.",
        Some("cell_undervoltage") => "Pack is below discharge voltage.",
        Some("remaining_capacity_alarm") => "Pack is in remaining-capacity alarm.",
        Some("permanent_failure") => "Pack reports permanent failure.",
        Some("sleep_mode") => "Gauge is asleep but still responds.",
        Some("no_battery") => "Pack present check failed.",
        _ => "Gauge did not answer the expected state.",
    }
}

fn bq40_issue_detail_footer(snapshot: &SelfCheckUiSnapshot) -> &'static str {
    if (snapshot.bq40z50_rca_alarm == Some(true)
        || snapshot.bq40z50_issue_detail == Some("cell_undervoltage"))
        && snapshot.bq25792_allow_charge == Some(true)
        && snapshot.fusb302_vbus_present == Some(true)
    {
        "Charging recovery is active."
    } else if snapshot.bq40z50_rca_alarm == Some(true)
        || snapshot.bq40z50_issue_detail == Some("cell_undervoltage")
    {
        "Connect input and charge pack."
    } else {
        "No safe auto recovery."
    }
}

fn bq40_recovery_dialog_copy(
    snapshot: &SelfCheckUiSnapshot,
    action: BmsRecoveryUiAction,
) -> Bq40RecoveryDialogCopy {
    match action {
        BmsRecoveryUiAction::Activation => Bq40RecoveryDialogCopy {
            headline: bq40_issue_headline(snapshot, action),
            body1: "Gauge is not answering SMBus.",
            body2: "Try activation now?",
            confirm_label: "Activate",
            progress1: "Applying wake profile.",
            progress2: "Checking gauge state...",
        },
        BmsRecoveryUiAction::DischargeAuthorization => Bq40RecoveryDialogCopy {
            headline: bq40_issue_headline(snapshot, action),
            body1: bq40_issue_detail_body(snapshot),
            body2: "Try discharge recovery?",
            confirm_label: "Recover",
            progress1: "Trying discharge recovery.",
            progress2: "Checking path ready...",
        },
    }
}

fn draw_bms_confirm_dialog<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    body_text: u16,
    cancel_border: u16,
    cancel_fill: u16,
    cancel_text: u16,
    confirm_border: u16,
    confirm_fill: u16,
    confirm_text: u16,
    copy: Bq40RecoveryDialogCopy,
) -> Result<(), P::Error> {
    text(
        painter,
        variant,
        FontRole::TextBody,
        copy.headline,
        Point::new(
            (SELF_CHECK_DIALOG_X + 10) as i32,
            (SELF_CHECK_DIALOG_Y + 24) as i32,
        ),
        HorizontalAlignment::Left,
        ATTENTION_COLOR,
    )?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        copy.body1,
        Point::new(
            (SELF_CHECK_DIALOG_X + 10) as i32,
            (SELF_CHECK_DIALOG_Y + 44) as i32,
        ),
        HorizontalAlignment::Left,
        body_text,
    )?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        copy.body2,
        Point::new(
            (SELF_CHECK_DIALOG_X + 10) as i32,
            (SELF_CHECK_DIALOG_Y + 60) as i32,
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
            (SELF_CHECK_CANCEL_BTN_X + (SELF_CHECK_CANCEL_BTN_W.saturating_sub(6 * 8)) / 2) as i32,
            (SELF_CHECK_CANCEL_BTN_Y + 6) as i32,
        ),
        HorizontalAlignment::Left,
        cancel_text,
    )?;
    text(
        painter,
        variant,
        FontRole::Num,
        copy.confirm_label,
        Point::new(
            (SELF_CHECK_CONFIRM_BTN_X
                + (SELF_CHECK_CONFIRM_BTN_W.saturating_sub(copy.confirm_label.len() as u16 * 8))
                    / 2) as i32,
            (SELF_CHECK_CONFIRM_BTN_Y + 6) as i32,
        ),
        HorizontalAlignment::Left,
        confirm_text,
    )?;
    Ok(())
}

fn draw_bms_progress_dialog<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    body_text: u16,
    copy: Bq40RecoveryDialogCopy,
) -> Result<(), P::Error> {
    let icon_x = SELF_CHECK_DIALOG_X + 10;
    let icon_y = SELF_CHECK_DIALOG_Y + 28;
    let text_x = SELF_CHECK_DIALOG_X + 50;
    draw_activation_icon(painter, icon_x, icon_y, ActivationIcon::Progress)?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        copy.progress1,
        Point::new(text_x as i32, (SELF_CHECK_DIALOG_Y + 28) as i32),
        HorizontalAlignment::Left,
        body_text,
    )?;
    text(
        painter,
        variant,
        FontRole::Num,
        copy.progress2,
        Point::new(text_x as i32, (SELF_CHECK_DIALOG_Y + 46) as i32),
        HorizontalAlignment::Left,
        ATTENTION_COLOR,
    )?;
    Ok(())
}

// Icon source: Google Material Symbols Rounded
// - wifi_wght700_24px.svg
// - wifi_off_wght700_24px.svg
// Icon source: Iconify / material-symbols:speed
// Extracted from official SVG via rsvg-convert at 28x28, alpha threshold >= 32.
const MENU_ICON_SPEED_28: &[(u8, u8, u8, u8)] = &[
    (12, 4, 4, 1),
    (9, 5, 10, 1),
    (7, 6, 14, 1),
    (6, 7, 16, 1),
    (5, 8, 18, 1),
    (4, 9, 20, 1),
    (4, 10, 15, 1),
    (20, 10, 4, 1),
    (3, 11, 15, 1),
    (20, 11, 5, 1),
    (3, 12, 13, 1),
    (19, 12, 6, 1),
    (2, 13, 13, 1),
    (18, 13, 8, 1),
    (2, 14, 11, 1),
    (18, 14, 8, 1),
    (2, 15, 10, 1),
    (17, 15, 9, 1),
    (2, 16, 10, 1),
    (16, 16, 10, 1),
    (2, 17, 10, 1),
    (16, 17, 10, 1),
    (2, 18, 24, 2),
    (3, 20, 22, 2),
    (4, 22, 20, 1),
    (5, 23, 18, 1),
];

// Icon source: Iconify / material-symbols:add
// Extracted from official SVG via rsvg-convert at 28x28, alpha threshold >= 32.
const MENU_ICON_ADD_28: &[(u8, u8, u8, u8)] = &[
    (13, 5, 2, 1),
    (12, 6, 4, 6),
    (6, 12, 16, 1),
    (5, 13, 18, 2),
    (6, 15, 16, 1),
    (12, 16, 4, 6),
    (13, 22, 2, 1),
];

// Icon source: Iconify / material-symbols:volume-up
// Extracted from official SVG via rsvg-convert at 28x28, alpha threshold >= 32.
const MENU_ICON_VOLUME_UP_28: &[(u8, u8, u8, u8)] = &[
    (16, 4, 3, 1),
    (13, 5, 1, 1),
    (16, 5, 5, 1),
    (12, 6, 2, 1),
    (16, 6, 6, 1),
    (11, 7, 3, 1),
    (18, 7, 5, 1),
    (10, 8, 4, 1),
    (20, 8, 3, 1),
    (9, 9, 5, 1),
    (16, 9, 1, 1),
    (20, 9, 4, 1),
    (3, 10, 11, 1),
    (16, 10, 2, 1),
    (21, 10, 3, 1),
    (3, 11, 11, 1),
    (16, 11, 3, 1),
    (21, 11, 4, 1),
    (3, 12, 11, 1),
    (16, 12, 3, 1),
    (22, 12, 3, 1),
    (3, 13, 11, 1),
    (16, 13, 4, 1),
    (22, 13, 3, 1),
    (3, 14, 11, 1),
    (16, 14, 4, 1),
    (22, 14, 3, 1),
    (3, 15, 11, 1),
    (16, 15, 3, 1),
    (22, 15, 3, 1),
    (3, 16, 11, 1),
    (16, 16, 3, 1),
    (21, 16, 4, 1),
    (3, 17, 11, 1),
    (16, 17, 2, 1),
    (21, 17, 3, 1),
    (9, 18, 5, 1),
    (16, 18, 1, 1),
    (20, 18, 4, 1),
    (10, 19, 4, 1),
    (20, 19, 3, 1),
    (11, 20, 3, 1),
    (18, 20, 5, 1),
    (12, 21, 2, 1),
    (16, 21, 6, 1),
    (13, 22, 1, 1),
    (16, 22, 5, 1),
    (16, 23, 3, 1),
];

// Icon source: Iconify / material-symbols:settings
// Extracted from official SVG via rsvg-convert at 28x28, alpha threshold >= 32.
const MENU_ICON_SETTINGS_28: &[(u8, u8, u8, u8)] = &[
    (10, 2, 8, 3),
    (5, 5, 2, 1),
    (10, 5, 8, 1),
    (21, 5, 2, 1),
    (4, 6, 20, 2),
    (3, 8, 22, 2),
    (2, 10, 11, 1),
    (15, 10, 11, 1),
    (2, 11, 9, 1),
    (17, 11, 9, 1),
    (4, 12, 7, 1),
    (17, 12, 7, 1),
    (5, 13, 5, 1),
    (18, 13, 5, 1),
    (5, 14, 5, 1),
    (18, 14, 5, 1),
    (4, 15, 7, 1),
    (17, 15, 7, 1),
    (2, 16, 9, 1),
    (17, 16, 9, 1),
    (2, 17, 11, 1),
    (15, 17, 11, 1),
    (3, 18, 22, 2),
    (4, 20, 20, 2),
    (5, 22, 2, 1),
    (10, 22, 8, 1),
    (21, 22, 2, 1),
    (10, 23, 8, 3),
];

// Icon source: Iconify / material-symbols:bar-chart
// Extracted from official SVG via rsvg-convert at 28x28, alpha threshold >= 32.
const MENU_ICON_BAR_CHART_28: &[(u8, u8, u8, u8)] = &[
    (12, 4, 4, 1),
    (11, 5, 6, 5),
    (4, 10, 6, 1),
    (11, 10, 6, 1),
    (4, 11, 6, 1),
    (11, 11, 6, 1),
    (4, 12, 6, 1),
    (11, 12, 6, 1),
    (4, 13, 6, 1),
    (11, 13, 6, 1),
    (4, 14, 6, 1),
    (11, 14, 6, 1),
    (4, 15, 6, 1),
    (11, 15, 6, 1),
    (18, 15, 6, 1),
    (4, 16, 6, 1),
    (11, 16, 6, 1),
    (18, 16, 6, 1),
    (4, 17, 6, 1),
    (11, 17, 6, 1),
    (18, 17, 6, 1),
    (4, 18, 6, 1),
    (11, 18, 6, 1),
    (18, 18, 6, 1),
    (4, 19, 6, 1),
    (11, 19, 6, 1),
    (18, 19, 6, 1),
    (4, 20, 6, 1),
    (11, 20, 6, 1),
    (18, 20, 6, 1),
    (4, 21, 6, 1),
    (11, 21, 6, 1),
    (18, 21, 6, 1),
    (4, 22, 6, 1),
    (11, 22, 6, 1),
    (18, 22, 6, 1),
    (5, 23, 4, 1),
    (12, 23, 4, 1),
    (19, 23, 4, 1),
];

// Icon source: Iconify / material-symbols:chevron-left
// Extracted from official SVG via rsvg-convert at 26x26, alpha threshold >= 16.
const MENU_ICON_CHEVRON_LEFT_26: &[(u8, u8, u8, u8)] = &[
    (15, 6, 1, 1),
    (14, 7, 3, 1),
    (13, 8, 4, 1),
    (12, 9, 4, 1),
    (11, 10, 4, 1),
    (10, 11, 4, 1),
    (9, 12, 4, 1),
    (9, 13, 4, 1),
    (10, 14, 4, 1),
    (11, 15, 4, 1),
    (12, 16, 4, 1),
    (13, 17, 4, 1),
    (14, 18, 3, 1),
    (15, 19, 1, 1),
];

// Icon source: Iconify / material-symbols:chevron-right
// Extracted from official SVG via rsvg-convert at 26x26, alpha threshold >= 16.
const MENU_ICON_CHEVRON_RIGHT_26: &[(u8, u8, u8, u8)] = &[
    (10, 6, 1, 1),
    (9, 7, 3, 1),
    (9, 8, 4, 1),
    (10, 9, 4, 1),
    (11, 10, 4, 1),
    (12, 11, 4, 1),
    (13, 12, 4, 1),
    (13, 13, 4, 1),
    (12, 14, 4, 1),
    (11, 15, 4, 1),
    (10, 16, 4, 1),
    (9, 17, 4, 1),
    (9, 18, 3, 1),
    (10, 19, 1, 1),
];

const WIFI_SYMBOL_ROUNDED_14: &[(u8, u8, u8, u8)] = &[
    (3, 1, 8, 1),
    (2, 2, 10, 1),
    (1, 3, 12, 1),
    (1, 4, 2, 1),
    (10, 4, 3, 1),
    (1, 5, 1, 1),
    (5, 5, 4, 1),
    (12, 5, 1, 1),
    (3, 6, 7, 1),
    (3, 7, 8, 1),
    (3, 8, 2, 1),
    (9, 8, 2, 1),
    (6, 10, 2, 1),
    (5, 11, 4, 1),
    (6, 12, 2, 1),
];

const WIFI_SIGNAL_DOT_14: &[(u8, u8, u8, u8)] = &[(6, 10, 2, 1), (5, 11, 4, 1), (6, 12, 2, 1)];

const WIFI_SIGNAL_INNER_ARC_14: &[(u8, u8, u8, u8)] =
    &[(3, 6, 7, 1), (3, 7, 8, 1), (3, 8, 2, 1), (9, 8, 2, 1)];

const WIFI_SIGNAL_OUTER_ARC_14: &[(u8, u8, u8, u8)] = &[
    (3, 1, 8, 1),
    (2, 2, 10, 1),
    (1, 3, 12, 1),
    (1, 4, 2, 1),
    (10, 4, 3, 1),
    (1, 5, 1, 1),
    (5, 5, 4, 1),
    (12, 5, 1, 1),
];

const WIFI_OFF_SYMBOL_ROUNDED_14: &[(u8, u8, u8, u8)] = &[
    (1, 1, 2, 1),
    (6, 1, 4, 1),
    (1, 2, 2, 1),
    (5, 2, 6, 1),
    (1, 3, 4, 1),
    (6, 3, 7, 1),
    (1, 4, 5, 1),
    (10, 4, 3, 1),
    (1, 5, 1, 1),
    (4, 5, 2, 1),
    (12, 5, 1, 1),
    (3, 6, 5, 1),
    (9, 6, 2, 1),
    (3, 7, 5, 1),
    (10, 7, 1, 1),
    (7, 8, 2, 1),
    (6, 9, 4, 1),
    (5, 10, 6, 1),
    (6, 11, 2, 1),
    (10, 11, 2, 1),
    (10, 12, 2, 1),
];

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
        ActivationIcon::Progress => (ATTENTION_COLOR, CARBON_IN_PROGRESS_32),
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

fn draw_icon_blocks_scaled<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    blocks: &[(u8, u8, u8, u8)],
    source_size: u16,
    target_size: u16,
    rgb565: u16,
) -> Result<(), P::Error> {
    for &(bx, by, bw, bh) in blocks {
        if bw == 0 || bh == 0 {
            continue;
        }
        let left = u16::from(bx) * target_size / source_size;
        let top = u16::from(by) * target_size / source_size;
        let right = (u16::from(bx + bw) * target_size).div_ceil(source_size);
        let bottom = (u16::from(by + bh) * target_size).div_ceil(source_size);
        fill(
            painter,
            x + left,
            y + top,
            right.saturating_sub(left).max(1),
            bottom.saturating_sub(top).max(1),
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

fn charger_protocol_badge(detail: DashboardDetailSnapshot) -> &'static str {
    match detail.charger_protocol {
        Some(DashboardChargerProtocol::Pps) => "PPS",
        Some(DashboardChargerProtocol::PdFixed) => "PD",
        Some(DashboardChargerProtocol::Usb5V) => "5V",
        Some(DashboardChargerProtocol::DcIn) => "DC",
        Some(DashboardChargerProtocol::NoCc) => "NO CC",
        Some(DashboardChargerProtocol::SourceCapsUnknown) => "CAP?",
        None => "N/A",
    }
}

fn draw_charger_protocol_badge<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    detail: DashboardDetailSnapshot,
    x: u16,
    y: u16,
) -> Result<(), P::Error> {
    text_with_position(
        painter,
        variant,
        FontRole::DetailBody,
        charger_protocol_badge(detail),
        Point::new(x as i32, y as i32),
        VerticalPosition::Bottom,
        HorizontalAlignment::Left,
        palette.bg,
    )
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
        SelfCheckCommState::Warn => ATTENTION_COLOR,
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
    } else {
        match spec.status_state {
            SelfCheckCommState::Err => ERROR_COLOR,
            SelfCheckCommState::Warn => ATTENTION_COLOR,
            _ => dim_color,
        }
    };
    let key_color = if spec.active {
        text_color
    } else {
        match spec.status_state {
            SelfCheckCommState::Warn => ATTENTION_COLOR,
            SelfCheckCommState::Err => text_color,
            _ => text_color,
        }
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
        key_color,
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
                "BATTERY CHARGE",
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

#[allow(dead_code)]
fn render_tps_test_charger_card<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    snapshot: &TpsTestUiSnapshot,
    frame_no: u32,
) -> Result<(), P::Error> {
    let x = 8;
    let y = 24;
    let w = 304;
    let _h = 46;
    let _accent = if snapshot.charger.actual_enabled && ((frame_no / 8) & 1) == 0 {
        palette.right
    } else {
        palette.accent
    };

    text(
        painter,
        variant,
        FontRole::DetailTitle,
        "BQ25792",
        Point::new((x + 0) as i32, (y + 2) as i32),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    text(
        painter,
        variant,
        FontRole::Num,
        snapshot.charger.status,
        Point::new((x + 180) as i32, (y + 2) as i32),
        HorizontalAlignment::Right,
        tps_test_comm_color(palette, snapshot.charger.comm_state, snapshot.charger.fault),
    )?;
    text(
        painter,
        variant,
        FontRole::DetailBody,
        format_args!(
            "CHG {}  INPUT {}  PACK {}",
            tps_test_bool(snapshot.charger.actual_enabled),
            tps_test_opt_bool(snapshot.charger.input_present),
            tps_test_opt_bool(snapshot.charger.vbat_present),
        ),
        Point::new((x + 0) as i32, (y + 18) as i32),
        HorizontalAlignment::Left,
        palette.text_dim,
    )?;
    text(
        painter,
        variant,
        FontRole::DetailBody,
        format_args!(
            "CHG {}/{}",
            TpsTestMetricVoltage(snapshot.charger.vreg_mv),
            TpsTestMetricChargeCurrent(snapshot.charger.ichg_ma),
        ),
        Point::new((x + 0) as i32, (y + 34) as i32),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    text(
        painter,
        variant,
        FontRole::DetailBody,
        "VOUT",
        Point::new((x + 212) as i32, (y + 8) as i32),
        HorizontalAlignment::Left,
        palette.text_dim,
    )?;
    text(
        painter,
        variant,
        FontRole::NumHero,
        format_args!("{}", TpsTestVoltage(tps_test_shared_vout_mv(snapshot))),
        Point::new((x + 208) as i32, (y + 28) as i32),
        HorizontalAlignment::Left,
        tps_test_shared_vout_color(palette, snapshot),
    )?;

    if let Some(fault) = snapshot.charger.fault {
        text(
            painter,
            variant,
            FontRole::DetailBody,
            fault,
            Point::new((x + w - 6) as i32, (y + 18) as i32),
            HorizontalAlignment::Right,
            palette.touch,
        )?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
fn render_tps_test_output_card<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    title: &'static str,
    x: u16,
    y: u16,
    w: u16,
    _h: u16,
    _profile: TpsTestVoutProfile,
    snapshot: TpsTestOutputSnapshot,
    _accent: u16,
) -> Result<(), P::Error> {
    text(
        painter,
        variant,
        FontRole::DetailTitle,
        title,
        Point::new((x + 0) as i32, (y + 2) as i32),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    text(
        painter,
        variant,
        FontRole::Num,
        tps_test_output_status(snapshot),
        Point::new((x + w - 2) as i32, (y + 2) as i32),
        HorizontalAlignment::Right,
        tps_test_comm_color(palette, snapshot.comm_state, snapshot.fault),
    )?;
    text(
        painter,
        variant,
        FontRole::DetailBody,
        format_args!("CFG {}", tps_test_bool(snapshot.requested_enabled)),
        Point::new((x + 0) as i32, (y + 16) as i32),
        HorizontalAlignment::Left,
        palette.text_dim,
    )?;
    text(
        painter,
        variant,
        FontRole::DetailBody,
        format_args!("LIVE {}", tps_test_opt_bool(snapshot.actual_enabled)),
        Point::new((x + w - 2) as i32, (y + 16) as i32),
        HorizontalAlignment::Right,
        if snapshot.actual_enabled == Some(true) {
            palette.accent
        } else {
            palette.text_dim
        },
    )?;

    text(
        painter,
        variant,
        FontRole::DetailBody,
        format_args!("V {}", TpsTestVoltage(snapshot.vset_mv)),
        Point::new((x + 0) as i32, (y + 34) as i32),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    text(
        painter,
        variant,
        FontRole::DetailBody,
        format_args!("I {}", TpsTestCurrent(snapshot.iout_ma)),
        Point::new((x + 0) as i32, (y + 50) as i32),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    text(
        painter,
        variant,
        FontRole::DetailBody,
        format_args!("T {}", TpsTestTemperature(snapshot.temp_c_x16)),
        Point::new((x + 0) as i32, (y + 66) as i32),
        HorizontalAlignment::Left,
        palette.text,
    )?;
    if let Some(status_line) = tps_test_detail_line(snapshot) {
        text(
            painter,
            variant,
            if snapshot.fault.is_some() {
                FontRole::TextBody
            } else {
                FontRole::DetailBody
            },
            status_line,
            Point::new((x + 0) as i32, (y + 82) as i32),
            HorizontalAlignment::Left,
            if snapshot.fault.is_some() {
                palette.touch
            } else {
                palette.text_dim
            },
        )?;
    }

    Ok(())
}

#[allow(dead_code)]
fn render_tps_test_footer<P: UiPainter>(
    painter: &mut P,
    variant: UiVariant,
    palette: Palette,
    snapshot: &TpsTestUiSnapshot,
) -> Result<(), P::Error> {
    let y = 154;
    let h = 12;
    fill(
        painter,
        8,
        y,
        UI_W - 16,
        h,
        fade_color(palette.panel, palette.panel_alt),
    )?;
    text(
        painter,
        variant,
        FontRole::TextBody,
        if let Some(alert) = snapshot.footer_alert {
            alert
        } else {
            snapshot.footer_notice.unwrap_or("FIXED PROFILE")
        },
        Point::new((UI_W / 2) as i32, (y + 1) as i32),
        HorizontalAlignment::Center,
        if snapshot.footer_alert.is_some() {
            palette.touch
        } else {
            palette.text_dim
        },
    )
}

#[allow(dead_code)]
fn tps_test_output_status(snapshot: TpsTestOutputSnapshot) -> &'static str {
    if !snapshot.requested_enabled {
        return "STBY";
    }
    if let Some(fault) = snapshot.fault {
        return match fault {
            "i2c_nack" | "i2c_timeout" | "i2c_arbitration" | "i2c" => "COMM",
            "THERM" => "THERM",
            _ => "FAULT",
        };
    }
    match snapshot.comm_state {
        SelfCheckCommState::Pending => "PEND",
        SelfCheckCommState::Warn => "WARN",
        SelfCheckCommState::Err => "ERR",
        SelfCheckCommState::NotAvailable => "STBY",
        SelfCheckCommState::Ok => match snapshot.actual_enabled {
            Some(true) => "RUN",
            Some(false) => "IDLE",
            None => "OK",
        },
    }
}

fn tps_test_fault_text(fault: Option<&'static str>) -> Option<&'static str> {
    match fault {
        Some("i2c_nack") => Some("I2C NACK"),
        Some("i2c_timeout") => Some("I2C TIMEOUT"),
        Some("i2c_arbitration") => Some("I2C ARB"),
        Some(other) => Some(other),
        None => None,
    }
}

#[allow(dead_code)]
fn tps_test_comm_color(
    palette: Palette,
    state: SelfCheckCommState,
    fault: Option<&'static str>,
) -> u16 {
    if fault.is_some() {
        return palette.touch;
    }
    match state {
        SelfCheckCommState::Pending | SelfCheckCommState::NotAvailable => palette.text_dim,
        SelfCheckCommState::Ok => palette.accent,
        SelfCheckCommState::Warn => ATTENTION_COLOR,
        SelfCheckCommState::Err => ERROR_COLOR,
    }
}

#[allow(dead_code)]
fn tps_test_bool(value: bool) -> &'static str {
    if value {
        "ON"
    } else {
        "OFF"
    }
}

#[allow(dead_code)]
fn tps_test_bool_compact(value: bool) -> &'static str {
    if value {
        "ON"
    } else {
        "OFF"
    }
}

#[allow(dead_code)]
fn tps_test_build_label(profile: &'static str) -> &'static str {
    match profile {
        "release" => "REL",
        "debug" => "DBG",
        other => other,
    }
}

#[allow(dead_code)]
fn tps_test_opt_bool(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "ON",
        Some(false) => "OFF",
        None => "NA",
    }
}

#[allow(dead_code)]
fn tps_test_detail_line(snapshot: TpsTestOutputSnapshot) -> Option<&'static str> {
    if let Some(fault) = tps_test_fault_text(snapshot.fault) {
        return Some(fault);
    }
    match snapshot.status_bits {
        Some(0) => Some("STAT 0x00"),
        Some(_) => Some("STAT SET"),
        None => None,
    }
}

#[allow(dead_code)]
fn tps_test_shared_vout_mv(snapshot: &TpsTestUiSnapshot) -> Option<u16> {
    if snapshot.out_a.actual_enabled == Some(true) {
        return snapshot.out_a.vbus_mv;
    }
    if snapshot.out_b.actual_enabled == Some(true) {
        return snapshot.out_b.vbus_mv;
    }
    snapshot.out_a.vbus_mv.or(snapshot.out_b.vbus_mv)
}

#[allow(dead_code)]
fn tps_test_shared_vout_color(palette: Palette, snapshot: &TpsTestUiSnapshot) -> u16 {
    if snapshot.out_a.fault.is_some() || snapshot.out_b.fault.is_some() {
        palette.touch
    } else if snapshot.out_a.actual_enabled == Some(true)
        || snapshot.out_b.actual_enabled == Some(true)
    {
        palette.accent
    } else {
        palette.text
    }
}

#[allow(dead_code)]
struct TpsTestVoltage(Option<u16>);
#[allow(dead_code)]
struct TpsTestCurrent(Option<i32>);
#[allow(dead_code)]
struct TpsTestChargeCurrent(Option<u16>);
#[allow(dead_code)]
struct TpsTestMetricVoltage(Option<u16>);
#[allow(dead_code)]
struct TpsTestMetricCurrent(Option<i32>);
#[allow(dead_code)]
struct TpsTestMetricChargeCurrent(Option<u16>);
#[allow(dead_code)]
struct TpsTestTemperature(Option<i16>);
#[allow(dead_code)]
struct TpsTestStatusBits(Option<u8>);

impl core::fmt::Display for TpsTestVoltage {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0 {
            Some(mv) => {
                let whole = mv / 1000;
                let frac = (mv % 1000) / 10;
                write!(f, "{:>2}.{:02}V", whole, frac)
            }
            None => write!(f, " N/A "),
        }
    }
}

impl core::fmt::Display for TpsTestCurrent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0 {
            Some(ma) => {
                let sign = if ma < 0 { '-' } else { ' ' };
                let abs = ma.unsigned_abs();
                let whole = abs / 1000;
                let frac = (abs % 1000) / 10;
                write!(f, "{}{}.{:02}A", sign, whole, frac)
            }
            None => write!(f, " N/A "),
        }
    }
}

impl core::fmt::Display for TpsTestChargeCurrent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0 {
            Some(ma) => write!(f, "{:>4}mA", ma),
            None => write!(f, " N/A "),
        }
    }
}

impl core::fmt::Display for TpsTestMetricVoltage {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0 {
            Some(mv) => write!(f, "{:>2}.{:02}V", mv / 1000, (mv % 1000) / 10),
            None => write!(f, "N/A"),
        }
    }
}

impl core::fmt::Display for TpsTestMetricCurrent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0 {
            Some(ma) => {
                let abs = ma.unsigned_abs();
                write!(f, "{:>1}.{:02}A", abs / 1000, (abs % 1000) / 10)
            }
            None => write!(f, "N/A"),
        }
    }
}

impl core::fmt::Display for TpsTestMetricChargeCurrent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0 {
            Some(ma) if ma >= 1000 => write!(f, "{:>1}.{:02}A", ma / 1000, (ma % 1000) / 10),
            Some(ma) => write!(f, "{}mA", ma),
            None => write!(f, "N/A"),
        }
    }
}

impl core::fmt::Display for TpsTestTemperature {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0 {
            Some(temp_c_x16) => {
                let temp_abs = temp_c_x16.unsigned_abs() as u16;
                let whole = temp_abs / 16;
                let frac = ((temp_abs % 16) * 10) / 16;
                if temp_c_x16 < 0 {
                    write!(f, "-{:>2}.{}C", whole, frac)
                } else {
                    write!(f, " {:>2}.{}C", whole, frac)
                }
            }
            None => write!(f, " N/A "),
        }
    }
}

impl core::fmt::Display for TpsTestStatusBits {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0 {
            Some(bits) => write!(f, "0x{:02X}", bits),
            None => write!(f, " N/A "),
        }
    }
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

fn draw_manual_action_button<P: UiPainter>(
    painter: &mut P,
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    fill_color: u16,
    border_color: u16,
) -> Result<(), P::Error> {
    fill(painter, x, y, w, h, border_color)?;
    if w > 2 && h > 2 {
        fill(painter, x + 1, y + 1, w - 2, h - 2, fill_color)?;
    }
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

fn text<P: UiPainter>(
    painter: &mut P,
    _variant: UiVariant,
    role: FontRole,
    content: impl Content,
    anchor: Point,
    align: HorizontalAlignment,
    color: u16,
) -> Result<(), P::Error> {
    text_with_position(
        painter,
        _variant,
        role,
        content,
        anchor,
        VerticalPosition::Top,
        align,
        color,
    )
}

fn text_with_position<P: UiPainter>(
    painter: &mut P,
    _variant: UiVariant,
    role: FontRole,
    content: impl Content,
    anchor: Point,
    vpos: VerticalPosition,
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
        vpos,
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
    fn beeper_defaults_start_at_level_four() {
        let prefs = BeeperPrefs::defaults();

        assert_eq!(prefs.action_volume, BeeperVolumeLevel::L4);
        assert_eq!(prefs.system_volume, BeeperVolumeLevel::L4);
        assert_eq!(prefs.selected_target, BeeperSettingTarget::Action);
    }

    #[test]
    fn dashboard_focus_label_background_covers_detail_body_text() {
        assert!(DASHBOARD_HOME_FOCUS_LABEL_H >= 17);
    }

    #[test]
    fn dashboard_alert_preview_icon_matches_wifi_scale() {
        assert!(DASHBOARD_HOME_ALERT_ICON_SIZE <= DASHBOARD_HOME_WIFI_ICON_H + 2);
        assert_eq!(
            DASHBOARD_HOME_ALERT_TOUCH_X,
            DASHBOARD_HOME_WIFI_TOUCH_X + DASHBOARD_HOME_WIFI_TOUCH_W
        );
    }

    #[test]
    fn alert_preview_catalog_covers_the_nine_mutable_runtime_alerts() {
        assert_eq!(AlertPreviewKind::ALL.len(), 9);
        assert_eq!(
            AlertPreviewKind::MainsAbsentDc.default_severity(),
            AlertPreviewSeverity::Warning
        );
        assert_eq!(
            AlertPreviewKind::BatteryLowWithMains.default_severity(),
            AlertPreviewSeverity::Warning
        );
        assert_eq!(
            AlertPreviewKind::BatteryProtection.default_severity(),
            AlertPreviewSeverity::Critical
        );
    }

    #[test]
    fn alert_preview_indicator_flashes_only_for_audible_alerts() {
        let palette = palette_for(UiVariant::InstrumentB);

        assert_eq!(
            alert_preview_indicator_color(
                palette,
                AlertPreviewSeverity::Warning,
                AlertPreviewSoundState::Audible,
                0,
            ),
            palette.text
        );
        assert_eq!(
            alert_preview_indicator_color(
                palette,
                AlertPreviewSeverity::Warning,
                AlertPreviewSoundState::Audible,
                1,
            ),
            ATTENTION_COLOR
        );
        assert_eq!(
            alert_preview_indicator_color(
                palette,
                AlertPreviewSeverity::Critical,
                AlertPreviewSoundState::Muted,
                0,
            ),
            ERROR_COLOR
        );
        assert_eq!(
            alert_preview_indicator_color(
                palette,
                AlertPreviewSeverity::Warning,
                AlertPreviewSoundState::PolicySilent,
                1,
            ),
            ATTENTION_COLOR
        );
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

        snapshot.bq40z50_last_result = Some(BmsResultKind::Success);
        assert!(is_bq40_activation_needed(&snapshot));

        snapshot.bq40z50_last_result = Some(BmsResultKind::NotDetected);
        assert!(is_bq40_activation_needed(&snapshot));
    }

    #[test]
    fn bq40_recovery_action_surfaces_discharge_authorization_when_bms_gate_is_recoverable() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.requested_outputs = EnabledOutputs::Only(OutputSelector::OutA);
        snapshot.output_gate_reason = OutputGateReason::BmsNotReady;
        snapshot.fusb302 = SelfCheckCommState::Ok;
        snapshot.fusb302_vbus_present = Some(true);
        snapshot.bq25792 = SelfCheckCommState::Ok;
        snapshot.bq40z50 = SelfCheckCommState::Warn;
        snapshot.bq40z50_discharge_ready = Some(false);
        snapshot.bq40z50_issue_detail = Some("xdsg_blocked");
        snapshot.bq40z50_recovery_action = Some(BmsRecoveryUiAction::DischargeAuthorization);

        assert_eq!(
            bq40_recovery_action(&snapshot),
            Some(BmsRecoveryUiAction::DischargeAuthorization)
        );
        assert_eq!(
            bq40_recovery_overlay(&snapshot),
            Some(SelfCheckOverlay::BmsDischargeAuthorizeConfirm)
        );
    }

    #[test]
    fn bq40_recovery_action_blocks_discharge_authorization_during_cell_undervoltage() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.requested_outputs = EnabledOutputs::Only(OutputSelector::OutA);
        snapshot.output_gate_reason = OutputGateReason::BmsNotReady;
        snapshot.fusb302 = SelfCheckCommState::Ok;
        snapshot.fusb302_vbus_present = Some(true);
        snapshot.bq25792 = SelfCheckCommState::Ok;
        snapshot.bq25792_allow_charge = Some(true);
        snapshot.bq40z50 = SelfCheckCommState::Warn;
        snapshot.bq40z50_discharge_ready = Some(false);
        snapshot.bq40z50_issue_detail = Some("cell_undervoltage");

        assert_eq!(bq40_recovery_action(&snapshot), None);
        assert_eq!(
            self_check_hardware_issue_overlay(&snapshot, SelfCheckHardwareTarget::Bq40z50),
            Some(SelfCheckOverlay::HardwareIssue(
                SelfCheckHardwareTarget::Bq40z50
            ))
        );
    }

    #[test]
    fn bq40_recovery_overlay_only_uses_backend_authorized_action() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.requested_outputs = EnabledOutputs::Only(OutputSelector::OutA);
        snapshot.output_gate_reason = OutputGateReason::BmsNotReady;
        snapshot.fusb302 = SelfCheckCommState::Ok;
        snapshot.fusb302_vbus_present = Some(true);
        snapshot.bq25792 = SelfCheckCommState::Ok;
        snapshot.bq40z50 = SelfCheckCommState::Warn;
        snapshot.bq40z50_discharge_ready = Some(false);
        snapshot.bq40z50_issue_detail = Some("xdsg_blocked");

        assert_eq!(
            bq40_recovery_action(&snapshot),
            Some(BmsRecoveryUiAction::DischargeAuthorization)
        );
        assert_eq!(bq40_recovery_overlay(&snapshot), None);

        snapshot.bq40z50_recovery_action = Some(BmsRecoveryUiAction::DischargeAuthorization);
        assert_eq!(
            bq40_recovery_overlay(&snapshot),
            Some(SelfCheckOverlay::BmsDischargeAuthorizeConfirm)
        );
    }

    #[test]
    fn bq40_failed_result_does_not_block_retryable_recovery_action() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.requested_outputs = EnabledOutputs::Only(OutputSelector::OutA);
        snapshot.output_gate_reason = OutputGateReason::BmsNotReady;
        snapshot.fusb302 = SelfCheckCommState::Ok;
        snapshot.fusb302_vbus_present = Some(true);
        snapshot.bq25792 = SelfCheckCommState::Ok;
        snapshot.bq40z50 = SelfCheckCommState::Warn;
        snapshot.bq40z50_discharge_ready = Some(false);
        snapshot.bq40z50_last_result = Some(BmsResultKind::Abnormal);

        assert_eq!(
            bq40_recovery_action(&snapshot),
            Some(BmsRecoveryUiAction::DischargeAuthorization)
        );

        snapshot.bq40z50 = SelfCheckCommState::Err;
        snapshot.bq40z50_last_result = Some(BmsResultKind::NotDetected);
        assert_eq!(
            bq40_recovery_action(&snapshot),
            Some(BmsRecoveryUiAction::Activation)
        );
    }

    #[test]
    fn bq40_result_overlay_ignores_success_but_keeps_failures_reopenable() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.bq40z50_last_result = Some(BmsResultKind::Success);
        assert_eq!(bq40_result_overlay(&snapshot), None);

        snapshot.bq40z50_last_result = Some(BmsResultKind::Abnormal);
        assert_eq!(
            bq40_result_overlay(&snapshot),
            Some(SelfCheckOverlay::BmsActivateResult(BmsResultKind::Abnormal))
        );
    }

    #[test]
    fn self_check_hit_test_maps_all_hardware_cards() {
        let cases = [
            (8, 24, SelfCheckHardwareTarget::Gc9307),
            (8, 53, SelfCheckHardwareTarget::Tca6408a),
            (8, 82, SelfCheckHardwareTarget::Fusb302),
            (8, 111, SelfCheckHardwareTarget::Ina3221),
            (8, 140, SelfCheckHardwareTarget::Bq25792),
            (165, 24, SelfCheckHardwareTarget::Bq40z50),
            (165, 53, SelfCheckHardwareTarget::TpsA),
            (165, 82, SelfCheckHardwareTarget::TpsB),
            (165, 111, SelfCheckHardwareTarget::TmpA),
            (165, 140, SelfCheckHardwareTarget::TmpB),
        ];

        for (x, y, target) in cases {
            assert_eq!(
                self_check_hit_test(x, y, SelfCheckOverlay::None),
                Some(SelfCheckTouchTarget::HardwareCard(target))
            );
        }
    }

    #[test]
    fn hardware_issue_overlay_surfaces_unrecoverable_bq40_issue() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.bq40z50 = SelfCheckCommState::Warn;
        snapshot.bq40z50_no_battery = Some(true);
        snapshot.bq40z50_issue_detail = Some("no_battery");
        snapshot.bq40z50_recovery_action = None;

        assert_eq!(
            self_check_hardware_issue_overlay(&snapshot, SelfCheckHardwareTarget::Bq40z50),
            Some(SelfCheckOverlay::HardwareIssue(
                SelfCheckHardwareTarget::Bq40z50
            ))
        );
    }

    #[test]
    fn hardware_issue_overlay_ignores_clear_modules() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.gc9307 = SelfCheckCommState::Ok;
        snapshot.bq40z50 = SelfCheckCommState::Ok;
        snapshot.bq40z50_no_battery = Some(false);
        snapshot.bq40z50_discharge_ready = Some(true);

        assert_eq!(
            self_check_hardware_issue_overlay(&snapshot, SelfCheckHardwareTarget::Gc9307),
            None
        );
        assert_eq!(
            self_check_hardware_issue_overlay(&snapshot, SelfCheckHardwareTarget::Bq40z50),
            None
        );
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
    fn self_check_blocks_dashboard_when_either_required_output_is_inactive() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Blocked);
        snapshot.gc9307 = SelfCheckCommState::Ok;
        snapshot.tca6408a = SelfCheckCommState::Ok;
        snapshot.fusb302 = SelfCheckCommState::Ok;
        snapshot.ina3221 = SelfCheckCommState::Ok;
        snapshot.bq25792 = SelfCheckCommState::Ok;
        snapshot.bq40z50 = SelfCheckCommState::Ok;
        snapshot.bq40z50_discharge_ready = Some(true);
        snapshot.requested_outputs = EnabledOutputs::Both;
        snapshot.tps_a = SelfCheckCommState::Ok;
        snapshot.tps_b = SelfCheckCommState::Ok;
        snapshot.tmp_a = SelfCheckCommState::Ok;
        snapshot.tmp_b = SelfCheckCommState::Ok;

        snapshot.active_outputs = EnabledOutputs::Only(OutputSelector::OutA);
        assert_eq!(self_check_dashboard_block_reason(&snapshot), Some("out_b"));

        snapshot.active_outputs = EnabledOutputs::Only(OutputSelector::OutB);
        assert_eq!(self_check_dashboard_block_reason(&snapshot), Some("out_a"));

        snapshot.active_outputs = EnabledOutputs::Both;
        assert!(self_check_can_enter_dashboard(&snapshot));
    }

    #[test]
    fn self_check_can_enter_dashboard_when_only_bms_charge_path_is_blocked() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.gc9307 = SelfCheckCommState::Ok;
        snapshot.tca6408a = SelfCheckCommState::Ok;
        snapshot.fusb302 = SelfCheckCommState::Ok;
        snapshot.ina3221 = SelfCheckCommState::Ok;
        snapshot.bq25792 = SelfCheckCommState::Ok;
        snapshot.bq40z50 = SelfCheckCommState::Warn;
        snapshot.bq40z50_no_battery = Some(false);
        snapshot.bq40z50_discharge_ready = Some(true);
        snapshot.bq40z50_issue_detail = Some("xchg_blocked");
        snapshot.requested_outputs = EnabledOutputs::Both;
        snapshot.active_outputs = EnabledOutputs::Both;
        snapshot.tps_a = SelfCheckCommState::Ok;
        snapshot.tps_b = SelfCheckCommState::Ok;
        snapshot.tmp_a = SelfCheckCommState::Ok;
        snapshot.tmp_b = SelfCheckCommState::Ok;

        assert!(self_check_can_enter_dashboard(&snapshot));
    }

    #[test]
    fn self_check_blocks_dashboard_when_bms_ready_but_vbat_absent() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.gc9307 = SelfCheckCommState::Ok;
        snapshot.tca6408a = SelfCheckCommState::Ok;
        snapshot.fusb302 = SelfCheckCommState::Ok;
        snapshot.ina3221 = SelfCheckCommState::Ok;
        snapshot.bq25792 = SelfCheckCommState::Ok;
        snapshot.bq25792_vbat_present = Some(false);
        snapshot.bq40z50 = SelfCheckCommState::Warn;
        snapshot.bq40z50_no_battery = Some(false);
        snapshot.bq40z50_discharge_ready = Some(false);
        snapshot.bq40z50_issue_detail = Some("physical_vbat_absent");
        snapshot.requested_outputs = EnabledOutputs::Both;
        snapshot.active_outputs = EnabledOutputs::None;
        snapshot.output_gate_reason = OutputGateReason::BmsNotReady;
        snapshot.tps_a = SelfCheckCommState::Ok;
        snapshot.tps_b = SelfCheckCommState::Ok;
        snapshot.tmp_a = SelfCheckCommState::Ok;
        snapshot.tmp_b = SelfCheckCommState::Ok;

        assert!(!self_check_can_enter_dashboard(&snapshot));
    }

    #[test]
    fn self_check_still_blocks_dashboard_when_bms_discharge_path_is_blocked() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.gc9307 = SelfCheckCommState::Ok;
        snapshot.tca6408a = SelfCheckCommState::Ok;
        snapshot.fusb302 = SelfCheckCommState::Ok;
        snapshot.ina3221 = SelfCheckCommState::Ok;
        snapshot.bq25792 = SelfCheckCommState::Ok;
        snapshot.bq40z50 = SelfCheckCommState::Warn;
        snapshot.bq40z50_no_battery = Some(false);
        snapshot.bq40z50_discharge_ready = Some(false);
        snapshot.bq40z50_issue_detail = Some("xdsg_blocked");
        snapshot.requested_outputs = EnabledOutputs::Both;
        snapshot.active_outputs = EnabledOutputs::Both;
        snapshot.tps_a = SelfCheckCommState::Ok;
        snapshot.tps_b = SelfCheckCommState::Ok;
        snapshot.tmp_a = SelfCheckCommState::Ok;
        snapshot.tmp_b = SelfCheckCommState::Ok;

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
    fn self_check_tps_summary_maps_expected_unpowered_tps_probe_failures_to_warn() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.output_gate_reason = OutputGateReason::BmsNotReady;
        snapshot.bq25792_vbat_present = Some(false);
        snapshot.tps_a = SelfCheckCommState::Err;
        snapshot.tps_b = SelfCheckCommState::Err;

        assert_eq!(self_check_tps_a_summary_name(&snapshot), "warn");
        assert_eq!(self_check_tps_b_summary_name(&snapshot), "warn");
    }

    #[test]
    fn self_check_tps_summary_keeps_err_when_upstream_power_is_available() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.bq25792_vbat_present = Some(true);
        snapshot.bq40z50_discharge_ready = Some(true);
        snapshot.tps_a = SelfCheckCommState::Err;
        snapshot.tps_b = SelfCheckCommState::Err;

        assert_eq!(self_check_tps_a_summary_name(&snapshot), "err");
        assert_eq!(self_check_tps_b_summary_name(&snapshot), "err");
    }

    #[test]
    fn charger_detail_keeps_wait_when_input_present_but_runtime_status_is_absent() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.fusb302_vbus_present = Some(true);
        snapshot.bq25792 = SelfCheckCommState::Ok;
        snapshot.bq25792_allow_charge = Some(false);

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);

        assert_eq!(
            detail_status_tag(DashboardDetailPage::Charger, live),
            "WAIT"
        );
        assert_eq!(charger_state_text(live), "WAIT");
    }

    #[test]
    fn home_charge_state_compresses_runtime_charge_tokens() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.bq25792 = SelfCheckCommState::Ok;
        snapshot.fusb302_vbus_present = Some(true);
        snapshot.dashboard_detail.charger_status = Some("CHG500");
        let charge_500 = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);
        assert_eq!(home_charge_state_text(charge_500), "CHG");

        snapshot.dashboard_detail.charger_status = Some("CHG100");
        let charge_100 = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);
        assert_eq!(home_charge_state_text(charge_100), "CHG");

        snapshot.dashboard_detail.charger_status = Some("CHG1A");
        let charge_1a = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);
        assert_eq!(home_charge_state_text(charge_1a), "CHG");

        snapshot.dashboard_detail.charger_status = Some("RECOV");
        let recovery = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);
        assert_eq!(home_charge_state_text(recovery), "CHG");
    }

    #[test]
    fn home_charge_state_preserves_runtime_non_charge_tokens() {
        for status in ["WAIT", "FULL", "WARM", "TEMP", "LOAD", "LOCK", "NOAC"] {
            let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
            snapshot.bq25792 = SelfCheckCommState::Ok;
            snapshot.dashboard_detail.charger_status = Some(status);
            if status == "NOAC" {
                snapshot.fusb302_vbus_present = Some(false);
            } else {
                snapshot.fusb302_vbus_present = Some(true);
            }

            let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);
            assert_eq!(home_charge_state_text(live), status);
        }
    }

    #[test]
    fn home_charge_state_clamps_unsupported_fault_tokens() {
        let mut warn_snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        warn_snapshot.bq25792 = SelfCheckCommState::Warn;
        warn_snapshot.fusb302_vbus_present = Some(true);
        warn_snapshot.dashboard_detail.charger_home_status = Some("TEMP");
        warn_snapshot.dashboard_detail.charger_status = Some("TEMP");
        let warn_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &warn_snapshot);
        assert_eq!(home_charge_state_text(warn_live), "TEMP");

        let mut fault_snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        fault_snapshot.bq25792 = SelfCheckCommState::Err;
        fault_snapshot.fusb302_vbus_present = Some(true);
        fault_snapshot.dashboard_detail.charger_home_status = Some("LOCK");
        fault_snapshot.dashboard_detail.charger_status = Some("FAULT");
        let fault_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &fault_snapshot);
        assert_eq!(home_charge_state_text(fault_live), "LOCK");
    }

    #[test]
    fn battery_flow_charge_state_uses_runtime_wait_in_assist_mode() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Supplement);
        snapshot.bq25792 = SelfCheckCommState::Ok;
        snapshot.fusb302_vbus_present = Some(true);
        snapshot.dashboard_detail.charger_status = Some("WAIT");

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Supplement), &snapshot);

        assert_eq!(battery_flow_charge_state_text(live), "WAIT");
    }

    #[test]
    fn battery_flow_charge_state_preserves_runtime_load_and_noac_tokens() {
        let mut assist_snapshot = SelfCheckUiSnapshot::pending(UpsMode::Supplement);
        assist_snapshot.bq25792 = SelfCheckCommState::Ok;
        assist_snapshot.fusb302_vbus_present = Some(true);
        assist_snapshot.dashboard_detail.charger_status = Some("LOAD");

        let assist_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Supplement), &assist_snapshot);
        assert_eq!(battery_flow_charge_state_text(assist_live), "LOAD");

        let mut off_snapshot = SelfCheckUiSnapshot::pending(UpsMode::Off);
        off_snapshot.bq25792 = SelfCheckCommState::Ok;
        off_snapshot.fusb302_vbus_present = Some(false);
        off_snapshot.dashboard_detail.charger_status = Some("NOAC");

        let off_live = DashboardLiveData::from_snapshot(base_model(UpsMode::Off), &off_snapshot);
        assert_eq!(battery_flow_charge_state_text(off_live), "NOAC");
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
    fn live_dashboard_prefers_actual_ibat_over_target_ichg() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.bq25792_allow_charge = Some(true);
        snapshot.bq25792_ichg_ma = Some(500);
        snapshot.bq25792_ibat_ma = Some(460);

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);

        assert_eq!(live.charge_current_ma(), Some(460));
    }

    #[test]
    fn live_dashboard_falls_back_to_target_ichg_when_ibat_is_missing() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.bq25792_allow_charge = Some(true);
        snapshot.bq25792_ichg_ma = Some(500);
        snapshot.bq25792_ibat_ma = None;

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);

        assert_eq!(live.charge_current_ma(), Some(500));
    }

    #[test]
    fn dashboard_hit_test_maps_fixed_home_regions() {
        assert_eq!(
            dashboard_hit_test(
                DashboardRoute::Home,
                DASHBOARD_HOME_WIFI_ICON_X + 4,
                DASHBOARD_HOME_WIFI_ICON_Y + 4
            ),
            Some(DashboardTouchTarget::HomeWifi)
        );
        assert_eq!(
            dashboard_hit_test(
                DashboardRoute::Home,
                DASHBOARD_HOME_WIFI_TOUCH_X + 2,
                DASHBOARD_HOME_WIFI_TOUCH_Y + 20
            ),
            Some(DashboardTouchTarget::HomeWifi)
        );
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
    fn dashboard_header_touch_contract_has_equal_usable_targets() {
        assert_eq!(DASHBOARD_HOME_WIFI_TOUCH, TouchRect::new(112, 0, 38, 36));
        assert_eq!(DASHBOARD_HOME_ALERT_TOUCH, TouchRect::new(150, 0, 38, 36));
        assert_eq!(DASHBOARD_HOME_WIFI_TOUCH.area(), 1_368);
        assert_eq!(DASHBOARD_HOME_ALERT_TOUCH.area(), 1_368);
        assert!(DASHBOARD_HOME_WIFI_TOUCH.within_screen());
        assert!(DASHBOARD_HOME_ALERT_TOUCH.within_screen());
        assert!(!DASHBOARD_HOME_WIFI_TOUCH.overlaps(DASHBOARD_HOME_ALERT_TOUCH));

        assert!(DASHBOARD_HOME_WIFI_TOUCH
            .contains(DASHBOARD_HOME_WIFI_ICON_X, DASHBOARD_HOME_WIFI_ICON_Y));
        assert!(DASHBOARD_HOME_WIFI_TOUCH.contains(
            DASHBOARD_HOME_WIFI_ICON_X + DASHBOARD_HOME_WIFI_ICON_W - 1,
            DASHBOARD_HOME_WIFI_ICON_Y + DASHBOARD_HOME_WIFI_ICON_H - 1
        ));
        assert!(DASHBOARD_HOME_ALERT_TOUCH
            .contains(DASHBOARD_HOME_ALERT_ICON_X, DASHBOARD_HOME_ALERT_ICON_Y));
        assert!(DASHBOARD_HOME_ALERT_TOUCH.contains(
            DASHBOARD_HOME_ALERT_ICON_X + DASHBOARD_HOME_ALERT_ICON_SIZE - 1,
            DASHBOARD_HOME_ALERT_ICON_Y + DASHBOARD_HOME_ALERT_ICON_SIZE - 1
        ));
    }

    #[test]
    fn dashboard_home_layering_prioritizes_alerts_then_wifi_over_output() {
        assert_eq!(
            dashboard_home_hit_test(true, 160, 30),
            Some(DashboardHomeTouchTarget::Alerts)
        );
        assert_eq!(
            dashboard_home_hit_test(false, 160, 30),
            Some(DashboardHomeTouchTarget::Dashboard(
                DashboardTouchTarget::HomeOutput
            ))
        );
        assert_eq!(
            dashboard_home_hit_test(true, 130, 30),
            Some(DashboardHomeTouchTarget::Dashboard(
                DashboardTouchTarget::HomeWifi
            ))
        );

        for y in 0..UI_H {
            for x in 0..UI_W {
                let resolved = dashboard_home_hit_test(true, x, y);
                if DASHBOARD_HOME_ALERT_TOUCH.contains(x, y) {
                    assert_eq!(resolved, Some(DashboardHomeTouchTarget::Alerts));
                } else if DASHBOARD_HOME_WIFI_TOUCH.contains(x, y) {
                    assert_eq!(
                        resolved,
                        Some(DashboardHomeTouchTarget::Dashboard(
                            DashboardTouchTarget::HomeWifi
                        ))
                    );
                }
            }
        }
    }

    #[test]
    fn alert_list_touch_contract_covers_rows_mute_and_back_without_overlap() {
        for slot in 0..3 {
            let row = ALERT_LIST_ROW_TOUCH[slot];
            let mute = ALERT_LIST_MUTE_TOUCH[slot];
            assert!(row.within_screen());
            assert!(mute.within_screen());
            assert_eq!(row, TouchRect::new(0, 24 + slot as u16 * 36, 272, 36));
            assert_eq!(mute, TouchRect::new(272, 24 + slot as u16 * 36, 48, 36));
            assert!(!row.overlaps(mute));
            assert_eq!(row.area(), 9_792);
            assert_eq!(mute.area(), 1_728);
            assert_eq!(
                alert_list_hit_test(row.x + row.w - 1, row.y + row.h - 1, 4),
                Some(AlertPreviewTouchTarget::Row(4 + slot))
            );
            assert_eq!(
                alert_list_hit_test(mute.x, mute.y, 4),
                Some(AlertPreviewTouchTarget::Mute(4 + slot))
            );
        }
        assert_eq!(ALERT_LIST_TOP_BACK_TOUCH, TouchRect::new(0, 0, 96, 24));
        assert!(ALERT_LIST_TOP_BACK_TOUCH.within_screen());
        assert_eq!(ALERT_LIST_TOP_BACK_TOUCH.area(), 2_304);
        assert_eq!(
            alert_list_hit_test(0, 0, 0),
            Some(AlertPreviewTouchTarget::Back)
        );
        assert_eq!(
            alert_list_hit_test(95, 23, 0),
            Some(AlertPreviewTouchTarget::Back)
        );
        assert_eq!(alert_list_hit_test(96, 23, 0), None);
        assert_eq!(alert_list_hit_test(0, 140, 0), None);
        assert_eq!(alert_list_hit_test(319, 171, 0), None);
        assert_eq!(alert_list_hit_test(100, 132, 0), None);
        assert_eq!(alert_list_hit_test(100, 139, 0), None);
    }

    #[test]
    fn alert_detail_touch_contract_keeps_actions_disjoint() {
        let regions = [ALERT_DETAIL_TOP_BACK_TOUCH, ALERT_DETAIL_MUTE_TOUCH];
        for (index, region) in regions.iter().copied().enumerate() {
            assert!(region.within_screen());
            for other in regions.iter().copied().skip(index + 1) {
                assert!(!region.overlaps(other));
            }
        }
        assert_eq!(ALERT_DETAIL_TOP_BACK_TOUCH, TouchRect::new(0, 0, 96, 32));
        assert_eq!(ALERT_DETAIL_MUTE_TOUCH, TouchRect::new(264, 72, 56, 40));
        assert_eq!(
            alert_detail_hit_test(95, 31),
            Some(AlertDetailTouchTarget::Back)
        );
        assert_eq!(
            alert_detail_hit_test(264, 72),
            Some(AlertDetailTouchTarget::Mute)
        );
        assert_eq!(
            alert_detail_hit_test(319, 111),
            Some(AlertDetailTouchTarget::Mute)
        );
        assert_eq!(alert_detail_hit_test(319, 140), None);
        assert_eq!(alert_detail_hit_test(160, 100), None);
        assert_eq!(alert_detail_hit_test(319, 139), None);
    }

    #[test]
    fn dashboard_menu_hit_test_maps_icons_and_nav_arrows() {
        assert_eq!(
            dashboard_menu_hit_test(
                MenuItem::Dashboard,
                DASHBOARD_MENU_NAV_HINT_LEFT_X + 2,
                DASHBOARD_MENU_NAV_HINT_Y + 2
            ),
            Some(DashboardMenuTouchTarget::Previous)
        );
        assert_eq!(
            dashboard_menu_hit_test(
                MenuItem::Dashboard,
                DASHBOARD_MENU_NAV_HINT_RIGHT_X + 2,
                DASHBOARD_MENU_NAV_HINT_Y + 2
            ),
            Some(DashboardMenuTouchTarget::Next)
        );
        assert_eq!(
            dashboard_menu_hit_test(
                MenuItem::Dashboard,
                DASHBOARD_MENU_ICON_CENTER_X as u16,
                DASHBOARD_MENU_ICON_Y + 12
            ),
            Some(DashboardMenuTouchTarget::Dashboard)
        );
        assert_eq!(
            dashboard_menu_hit_test(
                MenuItem::Dashboard,
                DASHBOARD_MENU_ICON_CENTER_X as u16 + (DASHBOARD_MENU_ICON_W * 2),
                DASHBOARD_MENU_ICON_Y + 12
            ),
            Some(DashboardMenuTouchTarget::Beeper)
        );
        assert_eq!(
            dashboard_menu_hit_test(
                MenuItem::Dashboard,
                DASHBOARD_MENU_ICON_CENTER_X as u16 + DASHBOARD_MENU_ICON_W,
                DASHBOARD_MENU_ICON_Y + 12
            ),
            None
        );
        assert_eq!(
            dashboard_menu_hit_test(MenuItem::Beeper, 12, DASHBOARD_MENU_ICON_Y + 12),
            Some(DashboardMenuTouchTarget::Dashboard)
        );
    }

    #[test]
    fn beeper_settings_hit_test_maps_back_rows_and_volume_track() {
        assert_eq!(
            beeper_settings_hit_test(8, 8),
            Some(BeeperSettingsTouchTarget::Back)
        );
        assert_eq!(beeper_settings_hit_test(UI_W / 2, 8), None);
        assert_eq!(
            beeper_settings_hit_test(AUDIO_ROW_X + 16, AUDIO_ACTION_ROW_Y + 8),
            Some(BeeperSettingsTouchTarget::Target(
                BeeperSettingTarget::Action
            ))
        );
        assert_eq!(
            beeper_settings_hit_test(AUDIO_ROW_X + 16, AUDIO_SYSTEM_ROW_Y + 8),
            Some(BeeperSettingsTouchTarget::Target(
                BeeperSettingTarget::System
            ))
        );
        assert_eq!(
            beeper_settings_hit_test(
                AUDIO_TRACK_X - 20,
                AUDIO_ACTION_ROW_Y.saturating_sub(AUDIO_VOLUME_TOUCH_Y_INSET)
            ),
            Some(BeeperSettingsTouchTarget::Volume {
                target: BeeperSettingTarget::Action,
                level: BeeperVolumeLevel::Off
            })
        );
        assert_eq!(
            beeper_settings_hit_test(AUDIO_TRACK_X, AUDIO_ACTION_ROW_Y + (AUDIO_ROW_H / 2)),
            Some(BeeperSettingsTouchTarget::Volume {
                target: BeeperSettingTarget::Action,
                level: BeeperVolumeLevel::Off
            })
        );
        assert_eq!(
            beeper_settings_hit_test(
                AUDIO_TRACK_X + (AUDIO_TRACK_W / 6) * 4,
                AUDIO_ACTION_ROW_Y + (AUDIO_ROW_H / 2)
            ),
            Some(BeeperSettingsTouchTarget::Volume {
                target: BeeperSettingTarget::Action,
                level: BeeperVolumeLevel::L4
            })
        );
        assert_eq!(
            beeper_settings_hit_test(
                AUDIO_TRACK_X + AUDIO_TRACK_W,
                AUDIO_SYSTEM_ROW_Y + (AUDIO_ROW_H / 2)
            ),
            Some(BeeperSettingsTouchTarget::Volume {
                target: BeeperSettingTarget::System,
                level: BeeperVolumeLevel::L6
            })
        );
        assert_eq!(
            beeper_settings_hit_test(
                AUDIO_TRACK_X + AUDIO_TRACK_W + 20,
                AUDIO_SYSTEM_ROW_Y + AUDIO_ROW_H + AUDIO_VOLUME_TOUCH_Y_INSET - 1
            ),
            Some(BeeperSettingsTouchTarget::Volume {
                target: BeeperSettingTarget::System,
                level: BeeperVolumeLevel::L6
            })
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
        assert_eq!(
            dashboard_route_for_target(DashboardTouchTarget::HomeWifi),
            DashboardRoute::Detail(DashboardDetailPage::Wifi)
        );
    }

    #[test]
    fn dashboard_detail_back_hit_zone_accepts_edge_taps() {
        assert_eq!(
            dashboard_hit_test(DashboardRoute::Detail(DashboardDetailPage::Output), 5, 1),
            Some(DashboardTouchTarget::DetailBack)
        );
        assert_eq!(
            dashboard_hit_test(
                DashboardRoute::Detail(DashboardDetailPage::BmsDetail),
                48,
                1
            ),
            Some(DashboardTouchTarget::CellsAdvancedBack)
        );
    }

    #[test]
    fn cells_detail_body_maps_to_bms_detail_and_bms_back_returns_to_cells() {
        assert_eq!(
            dashboard_hit_test(
                DashboardRoute::Detail(DashboardDetailPage::Cells),
                DASHBOARD_CELLS_ADVANCED_ENTRY_X + 24,
                DASHBOARD_CELLS_ADVANCED_ENTRY_Y + 24
            ),
            Some(DashboardTouchTarget::CellsAdvancedEntry)
        );
        assert_eq!(
            dashboard_route_for_target(DashboardTouchTarget::CellsAdvancedEntry),
            DashboardRoute::Detail(DashboardDetailPage::BmsDetail)
        );
        assert_eq!(
            dashboard_hit_test(
                DashboardRoute::Detail(DashboardDetailPage::BmsDetail),
                DASHBOARD_DETAIL_BACK_X + 4,
                DASHBOARD_DETAIL_BACK_Y + 4
            ),
            Some(DashboardTouchTarget::CellsAdvancedBack)
        );
        assert_eq!(
            dashboard_route_for_target(DashboardTouchTarget::CellsAdvancedBack),
            DashboardRoute::Detail(DashboardDetailPage::Cells)
        );
    }

    #[test]
    fn charger_detail_manual_entry_maps_to_manual_page() {
        assert_eq!(
            dashboard_hit_test(
                DashboardRoute::Detail(DashboardDetailPage::Charger),
                DASHBOARD_CHARGER_MANUAL_ENTRY_X + 20,
                DASHBOARD_CHARGER_MANUAL_ENTRY_Y + 20
            ),
            Some(DashboardTouchTarget::ChargerManualEntry)
        );
        assert_eq!(
            dashboard_route_for_target(DashboardTouchTarget::ChargerManualEntry),
            DashboardRoute::ManualCharge
        );
    }

    #[test]
    fn manual_page_routes_back_and_maps_actions() {
        assert_eq!(
            dashboard_hit_test(
                DashboardRoute::ManualCharge,
                DASHBOARD_DETAIL_BACK_X + 4,
                DASHBOARD_DETAIL_BACK_Y + 4
            ),
            Some(DashboardTouchTarget::ManualBack)
        );
        assert_eq!(
            dashboard_hit_test(
                DashboardRoute::ManualCharge,
                MANUAL_BACK_X + 8,
                MANUAL_BACK_Y + 8
            ),
            Some(DashboardTouchTarget::ManualBack)
        );
        assert_eq!(
            dashboard_route_for_target(DashboardTouchTarget::ManualBack),
            DashboardRoute::Detail(DashboardDetailPage::Charger)
        );
        assert_eq!(
            dashboard_hit_test(DashboardRoute::ManualCharge, 4, UI_H - 2),
            Some(DashboardTouchTarget::ManualBack)
        );
        assert_eq!(
            dashboard_manual_charge_action_for_target(DashboardTouchTarget::ManualSpeed1A),
            Some(ManualChargeUiAction::SetSpeed(ManualChargeSpeed::Ma1000))
        );
        assert_eq!(
            dashboard_manual_charge_action_for_target(DashboardTouchTarget::ManualTimer6h),
            Some(ManualChargeUiAction::SetTimerLimit(
                ManualChargeTimerLimit::H6
            ))
        );
    }

    #[test]
    fn manual_page_mode_text_prefers_stop_and_takeover_states() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);
        assert_eq!(manual_charge_mode_text(live), "AUTO");

        snapshot.dashboard_detail.manual_charge.runtime.stop_inhibit = true;
        let held_live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);
        assert_eq!(manual_charge_mode_text(held_live), "STOPPED");

        snapshot.dashboard_detail.charger_active = Some(true);
        let held_charging_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);
        assert_eq!(manual_charge_mode_text(held_charging_live), "AUTO CHG");

        snapshot.dashboard_detail.manual_charge.runtime.stop_inhibit = false;
        snapshot.dashboard_detail.manual_charge.runtime.active = true;
        snapshot.dashboard_detail.manual_charge.runtime.takeover = true;
        let active_live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);
        assert_eq!(manual_charge_mode_text(active_live), "TAKEOVER");
    }

    #[test]
    fn manual_page_controls_treat_runtime_active_as_stop_and_locked() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.dashboard_detail.manual_charge.runtime.active = true;

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);

        assert!(manual_charge_settings_locked(live));
        assert_eq!(manual_charge_action_label(live), "STOP");
    }

    #[test]
    fn manual_page_allows_takeover_start_while_auto_charging() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.dashboard_detail.charger_active = Some(true);
        snapshot.bq25792_allow_charge = Some(true);

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);

        assert_eq!(manual_charge_mode_text(live), "AUTO CHG");
        assert!(!manual_charge_settings_locked(live));
        assert_eq!(manual_charge_action_label(live), "START");
    }

    #[test]
    fn manual_page_footer_uses_safety_stop_notice() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.dashboard_detail.charger_notice = Some("manual_safety_blocked");

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);

        assert_eq!(manual_charge_footer_text(live), "SAFETY STOP");
    }

    #[test]
    fn manual_page_footer_marks_a_confirmed_loopback_override_session() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Backup);
        snapshot.dashboard_detail.manual_charge.runtime.active = true;
        snapshot
            .dashboard_detail
            .manual_charge
            .runtime
            .loopback_override = true;

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Backup), &snapshot);

        assert_eq!(manual_charge_footer_text(live), "LOOP OK");
    }

    #[test]
    fn manual_loopback_confirmation_hit_test_requires_explicit_button_choice() {
        assert_eq!(
            manual_charge_loopback_confirm_hit_test(
                SELF_CHECK_CANCEL_BTN_X + 2,
                SELF_CHECK_CANCEL_BTN_Y + 2,
            ),
            Some(ManualChargeLoopbackConfirmTarget::Cancel)
        );
        assert_eq!(
            manual_charge_loopback_confirm_hit_test(
                SELF_CHECK_CONFIRM_BTN_X + 2,
                SELF_CHECK_CONFIRM_BTN_Y + 2,
            ),
            Some(ManualChargeLoopbackConfirmTarget::Confirm)
        );
        assert_eq!(manual_charge_loopback_confirm_hit_test(8, 8), None);
    }

    #[test]
    fn manual_loopback_confirmation_keys_match_the_button_order() {
        assert_eq!(
            manual_charge_loopback_confirm_key_target(true, false, false),
            Some(ManualChargeLoopbackConfirmTarget::Cancel)
        );
        assert_eq!(
            manual_charge_loopback_confirm_key_target(false, true, false),
            Some(ManualChargeLoopbackConfirmTarget::Confirm)
        );
        assert_eq!(
            manual_charge_loopback_confirm_key_target(false, false, true),
            Some(ManualChargeLoopbackConfirmTarget::Confirm)
        );
    }

    #[test]
    fn charger_detail_notice_names_backup_usb_guard_states() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Backup);
        snapshot.bq25792 = SelfCheckCommState::Ok;
        snapshot.fusb302_vbus_present = Some(true);
        snapshot.dashboard_detail.charger_status = Some("CHG500");
        snapshot.dashboard_detail.charger_notice = Some("backup_usb_low_output_charge");
        let low = DashboardLiveData::from_snapshot(base_model(UpsMode::Backup), &snapshot);
        assert_eq!(
            low.page_notice(DashboardDetailPage::Charger),
            "USB BACKUP: CHARGING ACTIVE"
        );
        assert_eq!(
            detail_footer_badge(DashboardDetailPage::Charger, low),
            (DetailFooterIcon::Live, "CHARGING ACTIVE")
        );

        snapshot.dashboard_detail.charger_status = Some("LOAD");
        snapshot.dashboard_detail.charger_notice = Some("backup_usb_output_high_latched");
        let high = DashboardLiveData::from_snapshot(base_model(UpsMode::Backup), &snapshot);
        assert_eq!(
            high.page_notice(DashboardDetailPage::Charger),
            "USB BACKUP: LOAD PRESENT"
        );
        assert_eq!(
            detail_footer_badge(DashboardDetailPage::Charger, high),
            (DetailFooterIcon::Warn, "LOAD: CHG PAUSED")
        );

        snapshot.dashboard_detail.charger_status = Some("LOCK");
        snapshot.dashboard_detail.charger_notice = Some("backup_usb_telemetry_lost_latched");
        let lost = DashboardLiveData::from_snapshot(base_model(UpsMode::Backup), &snapshot);
        assert_eq!(
            lost.page_notice(DashboardDetailPage::Charger),
            "USB BACKUP: LOAD DATA LOST"
        );
        assert_eq!(
            detail_footer_badge(DashboardDetailPage::Charger, lost),
            (DetailFooterIcon::Warn, "LOAD DATA LOST")
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
    fn wifi_detail_status_and_footer_follow_runtime_state() {
        let disabled_snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        let disabled_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &disabled_snapshot);
        assert_eq!(
            detail_status_tag(DashboardDetailPage::Wifi, disabled_live),
            "OFF"
        );
        assert_eq!(
            detail_footer_badge(DashboardDetailPage::Wifi, disabled_live),
            (DetailFooterIcon::Unknown, "WIFI OFF")
        );

        let mut connecting_snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        connecting_snapshot.dashboard_detail.wifi = WifiSnapshot::connecting();
        let connecting_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &connecting_snapshot);
        assert_eq!(
            detail_status_tag(DashboardDetailPage::Wifi, connecting_live),
            "JOIN"
        );
        assert_eq!(
            detail_footer_badge(DashboardDetailPage::Wifi, connecting_live),
            (DetailFooterIcon::Warn, "JOINING AP")
        );

        let mut ready_snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        ready_snapshot.dashboard_detail.wifi = WifiSnapshot {
            state: WifiConnectionState::Connected,
            ipv4: Some([192, 168, 31, 45]),
            ..WifiSnapshot::disabled()
        };
        let ready_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &ready_snapshot);
        assert_eq!(
            detail_status_tag(DashboardDetailPage::Wifi, ready_live),
            "READY"
        );
        assert_eq!(
            detail_footer_badge(DashboardDetailPage::Wifi, ready_live),
            (DetailFooterIcon::Live, "LAN READY")
        );
    }

    #[test]
    fn connecting_wifi_icon_color_stays_attention() {
        let palette = palette_for(UiVariant::InstrumentB);
        let wifi = WifiSnapshot::connecting();

        assert_eq!(dashboard_wifi_icon_color(palette, wifi, 0), ATTENTION_COLOR);
        assert_eq!(dashboard_wifi_icon_color(palette, wifi, 4), ATTENTION_COLOR);
        assert_eq!(dashboard_wifi_icon_color(palette, wifi, 8), ATTENTION_COLOR);
    }

    #[test]
    fn connecting_wifi_signal_steps_sweep_dot_inner_full() {
        assert_eq!(connecting_wifi_signal_level(0), 0);
        assert_eq!(connecting_wifi_signal_level(4), 1);
        assert_eq!(connecting_wifi_signal_level(8), 2);
        assert_eq!(connecting_wifi_signal_level(12), 0);
        assert_eq!(connecting_wifi_signal_level(16), 1);
    }

    #[test]
    fn connecting_wifi_keeps_dashboard_frame_animation_active() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.dashboard_detail.wifi = WifiSnapshot::connecting();

        assert!(dashboard_route_has_active_animation(
            DashboardRoute::Home,
            &snapshot
        ));
        assert!(dashboard_route_has_active_animation(
            DashboardRoute::Detail(DashboardDetailPage::Wifi),
            &snapshot
        ));
    }

    #[test]
    fn connected_wifi_signal_levels_follow_rssi_thresholds() {
        assert_eq!(wifi_signal_level(None), 2);
        assert_eq!(wifi_signal_level(Some(-50)), 2);
        assert_eq!(wifi_signal_level(Some(-67)), 1);
        assert_eq!(wifi_signal_level(Some(-82)), 0);
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
    fn thermal_fan_motion_prefers_control_band_over_noisy_rpm() {
        assert_eq!(
            thermal_fan_motion(Some(4_200), Some(25), Some("LOW")),
            ThermalFanMotion::Low
        );
        assert_eq!(
            thermal_fan_motion(Some(900), Some(72), Some("HIGH")),
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
        snapshot.dashboard_detail.balance_enabled = Some(true);
        snapshot.dashboard_detail.balance_active = Some(true);
        snapshot.dashboard_detail.balance_mask = Some(0b0010);
        snapshot.dashboard_detail.balance_cell = Some(2);

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);

        assert_eq!(detail_status_tag(DashboardDetailPage::Cells, live), "WARN");
    }

    #[test]
    fn cells_detail_shows_multi_balance_summary_when_multiple_cells_are_active() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.dashboard_detail.balance_enabled = Some(true);
        snapshot.dashboard_detail.balance_active = Some(true);
        snapshot.dashboard_detail.balance_mask = Some(0b0101);

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);

        assert_eq!(
            detail_status_tag(DashboardDetailPage::Cells, live),
            "BAL ON"
        );
        assert_eq!(detail_balance_summary_text(live.detail), "MULTI");
    }

    #[test]
    fn cells_detail_balance_summary_distinguishes_off_idle_and_active_without_mask() {
        let mut off_snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        off_snapshot.dashboard_detail.balance_enabled = Some(false);
        let off_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &off_snapshot);
        assert_eq!(
            detail_status_tag(DashboardDetailPage::Cells, off_live),
            "OFF"
        );
        assert_eq!(detail_balance_summary_text(off_live.detail), "OFF");

        let mut idle_snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        idle_snapshot.dashboard_detail.balance_enabled = Some(true);
        idle_snapshot.dashboard_detail.balance_active = Some(false);
        let idle_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &idle_snapshot);
        assert_eq!(
            detail_status_tag(DashboardDetailPage::Cells, idle_live),
            "READY"
        );
        assert_eq!(detail_balance_summary_text(idle_live.detail), "IDLE");

        let mut active_snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        active_snapshot.dashboard_detail.balance_enabled = Some(true);
        active_snapshot.dashboard_detail.balance_active = Some(true);
        let active_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &active_snapshot);
        assert_eq!(
            detail_status_tag(DashboardDetailPage::Cells, active_live),
            "BAL ON"
        );
        assert_eq!(detail_balance_summary_text(active_live.detail), "ACTIVE");
    }

    #[test]
    fn cells_footer_badge_uses_balance_config_notice() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.dashboard_detail.balance_enabled = Some(true);
        snapshot.dashboard_detail.balance_cfg_match = Some(true);
        snapshot.dashboard_detail.cells_notice = Some("EXT CHG+RELAX");
        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);
        assert_eq!(
            detail_footer_badge(DashboardDetailPage::Cells, live),
            (DetailFooterIcon::Live, "BAL CFG")
        );

        let mut mismatch_snapshot = snapshot;
        mismatch_snapshot.dashboard_detail.balance_cfg_match = Some(false);
        mismatch_snapshot.dashboard_detail.cells_notice = Some("CFG MISMATCH");
        let mismatch_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &mismatch_snapshot);
        assert_eq!(
            detail_footer_badge(DashboardDetailPage::Cells, mismatch_live),
            (DetailFooterIcon::Warn, "CHECK STATUS")
        );
    }

    #[test]
    fn pending_detail_pages_stay_na_instead_of_reporting_healthy() {
        let snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);

        assert_eq!(detail_status_tag(DashboardDetailPage::Cells, live), "N/A");
        assert_eq!(
            detail_status_tag(DashboardDetailPage::BmsDetail, live),
            "N/A"
        );
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
            detail_footer_notice(DashboardDetailPage::BmsDetail, live),
            "BMS DETAIL SOURCE PENDING"
        );
        assert_eq!(
            detail_footer_notice(DashboardDetailPage::Output, live),
            "OUTPUT DETAIL SOURCE PENDING"
        );
        assert_eq!(
            detail_footer_badge(DashboardDetailPage::BatteryFlow, live),
            (DetailFooterIcon::Unknown, "NO DATA")
        );
        assert_eq!(
            detail_footer_badge(DashboardDetailPage::BmsDetail, live),
            (DetailFooterIcon::Unknown, "NO DATA")
        );
    }

    #[test]
    fn bms_detail_status_prioritizes_fault_limit_and_warn_states() {
        let mut fault_snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        fault_snapshot.dashboard_detail.remcap_mah = Some(3666);
        fault_snapshot.dashboard_detail.fcc_mah = Some(3704);
        fault_snapshot.dashboard_detail.reason_key = Some("permanent_failure");
        fault_snapshot.dashboard_detail.reason_label = Some("PERM FAIL");
        fault_snapshot.dashboard_detail.pf = Some(true);
        let fault_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &fault_snapshot);
        assert_eq!(
            detail_status_tag(DashboardDetailPage::BmsDetail, fault_live),
            "FAULT"
        );

        let mut limit_snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        limit_snapshot.dashboard_detail.remcap_mah = Some(3666);
        limit_snapshot.dashboard_detail.fcc_mah = Some(3704);
        limit_snapshot.dashboard_detail.reason_key = Some("xchg_blocked");
        limit_snapshot.dashboard_detail.reason_label = Some("CHG BLOCKED");
        limit_snapshot.dashboard_detail.xchg = Some(true);
        limit_snapshot.dashboard_detail.charge_ready = Some(false);
        let limit_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &limit_snapshot);
        assert_eq!(
            detail_status_tag(DashboardDetailPage::BmsDetail, limit_live),
            "LIMIT"
        );

        let mut no_battery_snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        no_battery_snapshot.bq40z50_no_battery = Some(true);
        let no_battery_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &no_battery_snapshot);
        assert_eq!(
            detail_status_tag(DashboardDetailPage::BmsDetail, no_battery_live),
            "LIMIT"
        );

        let mut warn_snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        warn_snapshot.dashboard_detail.remcap_mah = Some(3666);
        warn_snapshot.dashboard_detail.fcc_mah = Some(3704);
        warn_snapshot.dashboard_detail.reason_key = Some("remaining_capacity_alarm");
        warn_snapshot.dashboard_detail.reason_label = Some("RCA ALARM");
        warn_snapshot.dashboard_detail.rca_alarm = Some(true);
        let warn_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &warn_snapshot);
        assert_eq!(
            detail_status_tag(DashboardDetailPage::BmsDetail, warn_live),
            "WARN"
        );

        let mut unavailable_snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        unavailable_snapshot.dashboard_detail.remcap_mah = Some(3666);
        unavailable_snapshot.dashboard_detail.fcc_mah = Some(3704);
        unavailable_snapshot.dashboard_detail.reason_key = Some("op_status_unavailable");
        unavailable_snapshot.dashboard_detail.reason_label = Some("STATUS N/A");
        let unavailable_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &unavailable_snapshot);
        assert_eq!(
            detail_status_tag(DashboardDetailPage::BmsDetail, unavailable_live),
            "N/A"
        );

        let mut full_snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        full_snapshot.dashboard_detail.remcap_mah = Some(3666);
        full_snapshot.dashboard_detail.fcc_mah = Some(3704);
        full_snapshot.dashboard_detail.fc = Some(true);
        let full_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &full_snapshot);
        assert_eq!(
            detail_status_tag(DashboardDetailPage::BmsDetail, full_live),
            "READY"
        );
    }

    #[test]
    fn bms_detail_footer_reason_prefers_reason_label_and_link_fault() {
        let mut ready_snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        ready_snapshot.dashboard_detail.remcap_mah = Some(3666);
        ready_snapshot.dashboard_detail.fcc_mah = Some(3704);
        ready_snapshot.dashboard_detail.reason_label = Some("CHG BLOCKED");
        let ready_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &ready_snapshot);
        assert_eq!(bms_detail_footer_reason(ready_live), "CHG BLOCKED");

        let mut fault_snapshot = ready_snapshot;
        fault_snapshot.bq40z50 = SelfCheckCommState::Err;
        let fault_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &fault_snapshot);
        assert_eq!(bms_detail_footer_reason(fault_live), "BMS LINK FAULT");

        let mut no_battery_snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        no_battery_snapshot.bq40z50_no_battery = Some(true);
        let no_battery_live =
            DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &no_battery_snapshot);
        assert_eq!(bms_detail_footer_reason(no_battery_live), "NO BATTERY");
    }

    #[test]
    fn bms_detail_link_fault_stays_fault_even_without_detail_metrics() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.bq40z50 = SelfCheckCommState::Err;

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);
        assert_eq!(
            detail_status_tag(DashboardDetailPage::BmsDetail, live),
            "FAULT"
        );
    }

    #[test]
    fn bms_detail_learning_badge_prefers_ok_rest_wait_and_off_states() {
        let mut detail = DashboardDetailSnapshot::pending();
        assert_eq!(
            bms_detail_learn_badge(detail),
            BmsDetailSummaryBadge {
                label: "LEARN ?",
                tone: BmsDetailStateTone::Unknown,
            }
        );

        detail.learn_qen = Some(true);
        detail.learn_vok = Some(true);
        detail.learn_rest = Some(false);
        assert_eq!(
            bms_detail_learn_badge(detail),
            BmsDetailSummaryBadge {
                label: "LEARN OK",
                tone: BmsDetailStateTone::Ok,
            }
        );

        detail.learn_rest = Some(true);
        assert_eq!(
            bms_detail_learn_badge(detail),
            BmsDetailSummaryBadge {
                label: "LEARN REST",
                tone: BmsDetailStateTone::Warn,
            }
        );

        detail.learn_vok = Some(false);
        detail.learn_rest = Some(true);
        assert_eq!(
            bms_detail_learn_badge(detail),
            BmsDetailSummaryBadge {
                label: "LEARN REST",
                tone: BmsDetailStateTone::Warn,
            }
        );

        detail.learn_rest = Some(false);
        assert_eq!(
            bms_detail_learn_badge(detail),
            BmsDetailSummaryBadge {
                label: "LEARN WAIT",
                tone: BmsDetailStateTone::Warn,
            }
        );

        detail.learn_qen = Some(false);
        assert_eq!(
            bms_detail_learn_badge(detail),
            BmsDetailSummaryBadge {
                label: "LEARN OFF",
                tone: BmsDetailStateTone::Off,
            }
        );
    }

    #[test]
    fn bms_detail_state_glyphs_keep_unknown_telemetry_unknown() {
        assert_eq!(
            bms_detail_path_glyph(Some(true)),
            BmsDetailStateGlyph::PathBlocked
        );
        assert_eq!(
            bms_detail_path_glyph(Some(false)),
            BmsDetailStateGlyph::PathAllowed
        );
        assert_eq!(bms_detail_path_glyph(None), BmsDetailStateGlyph::Unknown);

        assert_eq!(bms_detail_fet_glyph(Some(true)), BmsDetailStateGlyph::FetOn);
        assert_eq!(
            bms_detail_fet_glyph(Some(false)),
            BmsDetailStateGlyph::FetOff
        );
        assert_eq!(bms_detail_fet_glyph(None), BmsDetailStateGlyph::Unknown);
    }

    #[test]
    fn bms_detail_balance_cfg_badge_tracks_match_state() {
        let mut detail = DashboardDetailSnapshot::pending();
        assert_eq!(
            bms_detail_balance_cfg_badge(detail),
            BmsDetailSummaryBadge {
                label: "BALCFG ?",
                tone: BmsDetailStateTone::Unknown,
            }
        );

        detail.balance_cfg_match = Some(true);
        assert_eq!(
            bms_detail_balance_cfg_badge(detail),
            BmsDetailSummaryBadge {
                label: "BALCFG OK",
                tone: BmsDetailStateTone::Ok,
            }
        );

        detail.balance_cfg_match = Some(false);
        assert_eq!(
            bms_detail_balance_cfg_badge(detail),
            BmsDetailSummaryBadge {
                label: "BALCFG MIS",
                tone: BmsDetailStateTone::Fault,
            }
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
            (DetailFooterIcon::Warn, "CHECK STATUS")
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
    fn charger_warm_detail_uses_warm_status_and_warn_footer() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
        snapshot.fusb302_vbus_present = Some(true);
        snapshot.dashboard_detail.charger_active = Some(true);
        snapshot.dashboard_detail.charger_status = Some("WARM");
        snapshot.dashboard_detail.charger_notice = Some("BQ25792 TS WARM - FAN FORCED HIGH");

        let live = DashboardLiveData::from_snapshot(base_model(UpsMode::Standby), &snapshot);

        assert_eq!(
            detail_status_tag(DashboardDetailPage::Charger, live),
            "WARM"
        );
        assert_eq!(
            detail_footer_badge(DashboardDetailPage::Charger, live),
            (DetailFooterIcon::Warn, "CHECK STATUS")
        );
        assert_eq!(
            detail_footer_notice(DashboardDetailPage::Charger, live),
            "BQ25792 TS WARM - FAN FORCED HIGH"
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
