use std::{
    convert::Infallible,
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process,
};

use image::{Rgb, RgbImage};

#[path = "../../../firmware/src/front_panel_scene.rs"]
mod front_panel_scene;

use front_panel_scene::{
    demo_mode_from_focus, AudioTestUiState, BmsResultKind, DisplayDiagnosticMeta,
    SelfCheckCommState, SelfCheckOverlay, SelfCheckUiSnapshot, TestFunctionUi, UiFocus, UiModel,
    UiPainter, UiVariant, UpsMode, UI_H, UI_W,
};

#[allow(dead_code)]
fn base_bq40_snapshot(mode: UpsMode) -> SelfCheckUiSnapshot {
    let mut snapshot = SelfCheckUiSnapshot::pending(mode);
    snapshot.gc9307 = SelfCheckCommState::Ok;
    snapshot.tca6408a = SelfCheckCommState::Ok;
    snapshot.fusb302 = SelfCheckCommState::Ok;
    snapshot.fusb302_vbus_present = Some(true);
    snapshot.input_vbus_mv = Some(19_240);
    snapshot.input_ibus_ma = Some(1180);
    snapshot.vin_vbus_mv = Some(19_240);
    snapshot.vin_iin_ma = Some(1180);
    snapshot.ina3221 = SelfCheckCommState::Ok;
    snapshot.ina_total_ma = Some(1130);
    snapshot.bq25792 = SelfCheckCommState::Ok;
    snapshot.bq25792_allow_charge = Some(true);
    snapshot.bq25792_ichg_ma = Some(520);
    snapshot.bq25792_vbat_present = Some(true);
    snapshot.bq40z50 = SelfCheckCommState::Err;
    snapshot.bq40z50_pack_mv = None;
    snapshot.bq40z50_current_ma = None;
    snapshot.bq40z50_soc_pct = None;
    snapshot.bq40z50_rca_alarm = None;
    snapshot.bq40z50_discharge_ready = None;
    snapshot.bq40z50_last_result = None;
    snapshot.tps_a = SelfCheckCommState::Ok;
    snapshot.tps_a_enabled = Some(true);
    snapshot.out_a_vbus_mv = Some(19_020);
    snapshot.tps_a_iout_ma = Some(430);
    snapshot.tps_b = SelfCheckCommState::Ok;
    snapshot.tps_b_enabled = Some(false);
    snapshot.out_b_vbus_mv = Some(19_010);
    snapshot.tps_b_iout_ma = Some(0);
    snapshot.tmp_a = SelfCheckCommState::Ok;
    snapshot.tmp_a_c = Some(39);
    snapshot.tmp_b = SelfCheckCommState::Ok;
    snapshot.tmp_b_c = Some(37);
    snapshot
}

fn dashboard_snapshot_for_mode(mode: UpsMode) -> SelfCheckUiSnapshot {
    let mut snapshot = base_bq40_snapshot(mode);
    snapshot.bq40z50 = SelfCheckCommState::Ok;
    snapshot.bq40z50_rca_alarm = Some(false);
    snapshot.bq40z50_no_battery = Some(false);
    snapshot.bq40z50_discharge_ready = Some(true);

    match mode {
        UpsMode::Off => {
            snapshot.fusb302_vbus_present = Some(true);
            snapshot.input_vbus_mv = Some(19_110);
            snapshot.input_ibus_ma = Some(1260);
            snapshot.vin_vbus_mv = Some(19_110);
            snapshot.vin_iin_ma = Some(1260);
            snapshot.bq25792_allow_charge = Some(false);
            snapshot.bq25792_ichg_ma = None;
            snapshot.tps_a_enabled = Some(false);
            snapshot.out_a_vbus_mv = None;
            snapshot.tps_a_iout_ma = None;
            snapshot.tps_b_enabled = Some(false);
            snapshot.out_b_vbus_mv = None;
            snapshot.tps_b_iout_ma = None;
            snapshot.ina_total_ma = None;
            snapshot.bq40z50_pack_mv = Some(15_180);
            snapshot.bq40z50_current_ma = Some(60);
            snapshot.bq40z50_soc_pct = Some(64);
        }
        UpsMode::Standby => {
            snapshot.fusb302_vbus_present = Some(true);
            snapshot.input_vbus_mv = Some(19_220);
            snapshot.input_ibus_ma = Some(1320);
            snapshot.vin_vbus_mv = Some(19_220);
            snapshot.vin_iin_ma = Some(1320);
            snapshot.bq25792_allow_charge = Some(true);
            snapshot.bq25792_ichg_ma = Some(540);
            snapshot.tps_a_enabled = Some(false);
            snapshot.out_a_vbus_mv = None;
            snapshot.tps_a_iout_ma = None;
            snapshot.tps_b_enabled = Some(false);
            snapshot.out_b_vbus_mv = None;
            snapshot.tps_b_iout_ma = None;
            snapshot.ina_total_ma = None;
            snapshot.bq40z50_pack_mv = Some(15_260);
            snapshot.bq40z50_current_ma = Some(520);
            snapshot.bq40z50_soc_pct = Some(67);
        }
        UpsMode::Supplement => {
            snapshot.fusb302_vbus_present = Some(true);
            snapshot.input_vbus_mv = Some(19_180);
            snapshot.input_ibus_ma = Some(820);
            snapshot.vin_vbus_mv = Some(19_180);
            snapshot.vin_iin_ma = Some(820);
            snapshot.bq25792_allow_charge = Some(false);
            snapshot.bq25792_ichg_ma = None;
            snapshot.tps_a_enabled = Some(true);
            snapshot.out_a_vbus_mv = Some(19_040);
            snapshot.tps_a_iout_ma = Some(620);
            snapshot.tps_b_enabled = Some(true);
            snapshot.out_b_vbus_mv = Some(19_000);
            snapshot.tps_b_iout_ma = Some(510);
            snapshot.ina_total_ma = Some(1130);
            snapshot.bq40z50_pack_mv = Some(14_980);
            snapshot.bq40z50_current_ma = Some(-900);
            snapshot.bq40z50_soc_pct = Some(59);
        }
        UpsMode::Backup => {
            snapshot.fusb302_vbus_present = Some(false);
            snapshot.input_vbus_mv = None;
            snapshot.input_ibus_ma = None;
            snapshot.vin_vbus_mv = None;
            snapshot.vin_iin_ma = None;
            snapshot.bq25792_allow_charge = Some(false);
            snapshot.bq25792_ichg_ma = None;
            snapshot.tps_a_enabled = Some(true);
            snapshot.out_a_vbus_mv = Some(18_860);
            snapshot.tps_a_iout_ma = Some(980);
            snapshot.tps_b_enabled = Some(true);
            snapshot.out_b_vbus_mv = Some(18_830);
            snapshot.tps_b_iout_ma = Some(910);
            snapshot.ina_total_ma = Some(1890);
            snapshot.bq40z50_pack_mv = Some(14_820);
            snapshot.bq40z50_current_ma = Some(-1880);
            snapshot.bq40z50_soc_pct = Some(53);
        }
    }

    snapshot
}

#[allow(dead_code)]
fn bq40_snapshot_for_scenario(
    mode: UpsMode,
    scenario: ScenarioArg,
) -> (SelfCheckUiSnapshot, SelfCheckOverlay) {
    let mut snapshot = base_bq40_snapshot(mode);
    let overlay = match scenario {
        ScenarioArg::Bq40Offline => SelfCheckOverlay::None,
        ScenarioArg::Bq40OfflineDialog => SelfCheckOverlay::BmsActivateConfirm,
        ScenarioArg::Bq40Activating => SelfCheckOverlay::BmsActivateProgress,
        ScenarioArg::Bq40ResultSuccess => {
            snapshot.bq40z50 = SelfCheckCommState::Ok;
            snapshot.bq40z50_soc_pct = Some(78);
            snapshot.bq40z50_rca_alarm = Some(false);
            snapshot.bq40z50_discharge_ready = Some(true);
            snapshot.bq25792_vbat_present = Some(true);
            snapshot.bq40z50_last_result = Some(BmsResultKind::Success);
            SelfCheckOverlay::BmsActivateResult(BmsResultKind::Success)
        }
        ScenarioArg::Bq40ResultNoBattery => {
            snapshot.bq25792_vbat_present = Some(false);
            snapshot.bq40z50_last_result = Some(BmsResultKind::NoBattery);
            SelfCheckOverlay::BmsActivateResult(BmsResultKind::NoBattery)
        }
        ScenarioArg::Bq40ResultRomMode => {
            snapshot.bq40z50_last_result = Some(BmsResultKind::RomMode);
            SelfCheckOverlay::BmsActivateResult(BmsResultKind::RomMode)
        }
        ScenarioArg::Bq40ResultAbnormal => {
            snapshot.bq40z50 = SelfCheckCommState::Warn;
            snapshot.bq40z50_soc_pct = Some(61);
            snapshot.bq40z50_rca_alarm = Some(true);
            snapshot.bq40z50_discharge_ready = Some(false);
            snapshot.bq25792_vbat_present = Some(true);
            snapshot.bq40z50_last_result = Some(BmsResultKind::Abnormal);
            SelfCheckOverlay::BmsActivateResult(BmsResultKind::Abnormal)
        }
        ScenarioArg::Bq40ResultNotDetected => {
            snapshot.bq40z50_last_result = Some(BmsResultKind::NotDetected);
            SelfCheckOverlay::BmsActivateResult(BmsResultKind::NotDetected)
        }
        ScenarioArg::Default
        | ScenarioArg::DisplayDiag
        | ScenarioArg::DashboardRuntimeStandby
        | ScenarioArg::DashboardRuntimeAssist
        | ScenarioArg::DashboardRuntimeBackup
        | ScenarioArg::TestAudio
        | ScenarioArg::TestNavigation => SelfCheckOverlay::None,
    };
    (snapshot, overlay)
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = Args::parse(env::args().skip(1))?;

    if !args.out_dir.is_absolute() {
        return Err("--out-dir must be an absolute path".into());
    }

    let effective_mode = match args.scenario {
        ScenarioArg::DashboardRuntimeStandby => ModeArg::Standby,
        ScenarioArg::DashboardRuntimeAssist => ModeArg::Supplement,
        ScenarioArg::DashboardRuntimeBackup => ModeArg::Backup,
        _ => args.mode,
    };

    let frame_dir = args
        .out_dir
        .join(format!("variant-{}", args.variant.as_tag()))
        .join(format!("mode-{}", effective_mode.as_tag()))
        .join(format!("focus-{}", args.focus.as_tag()))
        .join(format!("scenario-{}", args.scenario.as_tag()));
    fs::create_dir_all(&frame_dir).map_err(|e| format!("create output dir failed: {e}"))?;

    let mut framebuffer = FrameBuffer::new(UI_W as usize, UI_H as usize);
    let model = UiModel {
        mode: effective_mode.into_scene(),
        focus: args.focus.into_scene(),
        touch_irq: args.focus.into_scene() == UiFocus::Touch,
        frame_no: args.frame_no,
    };

    match args.scenario {
        ScenarioArg::Default => {
            front_panel_scene::render_frame_with_self_check_overlay(
                &mut framebuffer,
                &model,
                args.variant.into_scene(),
                None,
                SelfCheckOverlay::None,
            )
            .map_err(|_| "render failed unexpectedly".to_string())?;
        }
        ScenarioArg::DisplayDiag => {
            let meta = DisplayDiagnosticMeta {
                orientation_label: "ORI: LANDSCAPE_SWAP (MADCTL=0xE0)",
                color_order_label: "COLOR ORDER: BGR565",
                heartbeat_on: (args.frame_no % 2) == 0,
            };
            front_panel_scene::render_display_diagnostic(&mut framebuffer, &meta)
                .map_err(|_| "render failed unexpectedly".to_string())?;
        }
        ScenarioArg::DashboardRuntimeStandby
        | ScenarioArg::DashboardRuntimeAssist
        | ScenarioArg::DashboardRuntimeBackup => {
            let mode = match args.scenario {
                ScenarioArg::DashboardRuntimeStandby => UpsMode::Standby,
                ScenarioArg::DashboardRuntimeAssist => UpsMode::Supplement,
                ScenarioArg::DashboardRuntimeBackup => UpsMode::Backup,
                _ => unreachable!(),
            };
            let snapshot = dashboard_snapshot_for_mode(mode);
            let dashboard_model = UiModel {
                mode,
                focus: UiFocus::Idle,
                touch_irq: false,
                frame_no: args.frame_no,
            };
            front_panel_scene::render_frame_with_self_check_overlay(
                &mut framebuffer,
                &dashboard_model,
                UiVariant::InstrumentB,
                Some(&snapshot),
                SelfCheckOverlay::None,
            )
            .map_err(|_| "render failed unexpectedly".to_string())?;
        }
        ScenarioArg::Bq40Offline
        | ScenarioArg::Bq40OfflineDialog
        | ScenarioArg::Bq40Activating
        | ScenarioArg::Bq40ResultSuccess
        | ScenarioArg::Bq40ResultNoBattery
        | ScenarioArg::Bq40ResultRomMode
        | ScenarioArg::Bq40ResultAbnormal
        | ScenarioArg::Bq40ResultNotDetected => {
            let (snapshot, overlay) =
                bq40_snapshot_for_scenario(args.mode.into_scene(), args.scenario);
            front_panel_scene::render_frame_with_self_check_overlay(
                &mut framebuffer,
                &model,
                args.variant.into_scene(),
                Some(&snapshot),
                overlay,
            )
            .map_err(|_| "render failed unexpectedly".to_string())?;
        }
        ScenarioArg::TestAudio => {
            let state = AudioTestUiState {
                playing: false,
                queued: 0,
                current: None,
                selected_idx: 3,
                list_top: 0,
            };
            front_panel_scene::render_test_audio_playback(&mut framebuffer, false, state)
                .map_err(|_| "render failed unexpectedly".to_string())?;
        }
        ScenarioArg::TestNavigation => {
            front_panel_scene::render_test_navigation(
                &mut framebuffer,
                TestFunctionUi::AudioPlayback,
                Some(TestFunctionUi::ScreenStatic),
            )
            .map_err(|_| "render failed unexpectedly".to_string())?;
        }
    }

    let bin_path = frame_dir.join("framebuffer.bin");
    framebuffer
        .write_raw_le(&bin_path)
        .map_err(|e| format!("write framebuffer failed: {e}"))?;

    let png_path = frame_dir.join("preview.png");
    framebuffer
        .write_png(&png_path)
        .map_err(|e| format!("write preview png failed: {e}"))?;

    println!("wrote {} and {}", bin_path.display(), png_path.display());
    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum VariantArg {
    A,
    B,
    C,
    D,
}

impl VariantArg {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw.to_ascii_lowercase().as_str() {
            "a" => Ok(Self::A),
            "b" => Ok(Self::B),
            "c" => Ok(Self::C),
            "d" => Ok(Self::D),
            _ => Err(format!(
                "unsupported --variant value: {raw} (expected A|B|C|D)"
            )),
        }
    }

    fn as_tag(self) -> &'static str {
        match self {
            VariantArg::A => "A",
            VariantArg::B => "B",
            VariantArg::C => "C",
            VariantArg::D => "D",
        }
    }

    fn into_scene(self) -> UiVariant {
        match self {
            VariantArg::A => UiVariant::InstrumentA,
            VariantArg::B => UiVariant::InstrumentB,
            VariantArg::C => UiVariant::RetroC,
            VariantArg::D => UiVariant::InstrumentD,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum FocusArg {
    Idle,
    Up,
    Down,
    Left,
    Right,
    Center,
    Touch,
}

impl FocusArg {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw.to_ascii_lowercase().as_str() {
            "idle" => Ok(Self::Idle),
            "up" => Ok(Self::Up),
            "down" => Ok(Self::Down),
            "left" => Ok(Self::Left),
            "right" => Ok(Self::Right),
            "center" => Ok(Self::Center),
            "touch" => Ok(Self::Touch),
            _ => Err(format!(
                "unsupported --focus value: {raw} (expected idle|up|down|left|right|center|touch)"
            )),
        }
    }

    fn as_tag(self) -> &'static str {
        match self {
            FocusArg::Idle => "idle",
            FocusArg::Up => "up",
            FocusArg::Down => "down",
            FocusArg::Left => "left",
            FocusArg::Right => "right",
            FocusArg::Center => "center",
            FocusArg::Touch => "touch",
        }
    }

    fn into_scene(self) -> UiFocus {
        match self {
            FocusArg::Idle => UiFocus::Idle,
            FocusArg::Up => UiFocus::Up,
            FocusArg::Down => UiFocus::Down,
            FocusArg::Left => UiFocus::Left,
            FocusArg::Right => UiFocus::Right,
            FocusArg::Center => UiFocus::Center,
            FocusArg::Touch => UiFocus::Touch,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum ModeArg {
    Off,
    Standby,
    Supplement,
    Backup,
}

impl ModeArg {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw.to_ascii_lowercase().as_str() {
            "off" => Ok(Self::Off),
            "standby" | "stby" => Ok(Self::Standby),
            "supplement" | "supp" => Ok(Self::Supplement),
            "backup" | "batt" => Ok(Self::Backup),
            _ => Err(format!(
                "unsupported --mode value: {raw} (expected off|standby|supplement|backup)"
            )),
        }
    }

    fn from_focus(focus: FocusArg) -> Self {
        match demo_mode_from_focus(focus.into_scene()) {
            UpsMode::Off => Self::Off,
            UpsMode::Standby => Self::Standby,
            UpsMode::Supplement => Self::Supplement,
            UpsMode::Backup => Self::Backup,
        }
    }

    fn into_scene(self) -> UpsMode {
        match self {
            ModeArg::Off => UpsMode::Off,
            ModeArg::Standby => UpsMode::Standby,
            ModeArg::Supplement => UpsMode::Supplement,
            ModeArg::Backup => UpsMode::Backup,
        }
    }

    fn as_tag(self) -> &'static str {
        match self {
            ModeArg::Off => "off",
            ModeArg::Standby => "standby",
            ModeArg::Supplement => "supplement",
            ModeArg::Backup => "backup",
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum ScenarioArg {
    Default,
    DisplayDiag,
    DashboardRuntimeStandby,
    DashboardRuntimeAssist,
    DashboardRuntimeBackup,
    Bq40Offline,
    Bq40OfflineDialog,
    Bq40Activating,
    Bq40ResultSuccess,
    Bq40ResultNoBattery,
    Bq40ResultRomMode,
    Bq40ResultAbnormal,
    Bq40ResultNotDetected,
    TestAudio,
    TestNavigation,
}

impl ScenarioArg {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw.to_ascii_lowercase().as_str() {
            "default" => Ok(Self::Default),
            "display-diag" => Ok(Self::DisplayDiag),
            "dashboard-runtime-standby" => Ok(Self::DashboardRuntimeStandby),
            "dashboard-runtime-assist" => Ok(Self::DashboardRuntimeAssist),
            "dashboard-runtime-backup" => Ok(Self::DashboardRuntimeBackup),
            "bq40-offline" => Ok(Self::Bq40Offline),
            "bq40-offline-dialog" => Ok(Self::Bq40OfflineDialog),
            "bq40-activating" => Ok(Self::Bq40Activating),
            "bq40-result-success" => Ok(Self::Bq40ResultSuccess),
            "bq40-result-no-battery" => Ok(Self::Bq40ResultNoBattery),
            "bq40-result-rom-mode" => Ok(Self::Bq40ResultRomMode),
            "bq40-result-abnormal" => Ok(Self::Bq40ResultAbnormal),
            "bq40-result-not-detected" => Ok(Self::Bq40ResultNotDetected),
            "test-audio" => Ok(Self::TestAudio),
            "test-navigation" => Ok(Self::TestNavigation),
            _ => Err(format!(
                "unsupported --scenario value: {raw} (expected default|display-diag|dashboard-runtime-standby|dashboard-runtime-assist|dashboard-runtime-backup|bq40-offline|bq40-offline-dialog|bq40-activating|bq40-result-success|bq40-result-no-battery|bq40-result-rom-mode|bq40-result-abnormal|bq40-result-not-detected|test-audio|test-navigation)"
            )),
        }
    }

    fn as_tag(self) -> &'static str {
        match self {
            ScenarioArg::Default => "default",
            ScenarioArg::DisplayDiag => "display-diag",
            ScenarioArg::DashboardRuntimeStandby => "dashboard-runtime-standby",
            ScenarioArg::DashboardRuntimeAssist => "dashboard-runtime-assist",
            ScenarioArg::DashboardRuntimeBackup => "dashboard-runtime-backup",
            ScenarioArg::Bq40Offline => "bq40-offline",
            ScenarioArg::Bq40OfflineDialog => "bq40-offline-dialog",
            ScenarioArg::Bq40Activating => "bq40-activating",
            ScenarioArg::Bq40ResultSuccess => "bq40-result-success",
            ScenarioArg::Bq40ResultNoBattery => "bq40-result-no-battery",
            ScenarioArg::Bq40ResultRomMode => "bq40-result-rom-mode",
            ScenarioArg::Bq40ResultAbnormal => "bq40-result-abnormal",
            ScenarioArg::Bq40ResultNotDetected => "bq40-result-not-detected",
            ScenarioArg::TestAudio => "test-audio",
            ScenarioArg::TestNavigation => "test-navigation",
        }
    }
}

#[derive(Debug)]
struct Args {
    variant: VariantArg,
    mode: ModeArg,
    focus: FocusArg,
    scenario: ScenarioArg,
    out_dir: PathBuf,
    frame_no: u32,
}

impl Args {
    fn parse<I>(mut iter: I) -> Result<Self, String>
    where
        I: Iterator<Item = String>,
    {
        let mut variant: Option<VariantArg> = None;
        let mut mode: Option<ModeArg> = None;
        let mut focus: Option<FocusArg> = None;
        let mut scenario: Option<ScenarioArg> = None;
        let mut out_dir: Option<PathBuf> = None;
        let mut frame_no: u32 = 0;

        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--variant" => {
                    let value = iter.next().ok_or("missing value for --variant")?;
                    variant = Some(VariantArg::parse(&value)?);
                }
                "--focus" => {
                    let value = iter.next().ok_or("missing value for --focus")?;
                    focus = Some(FocusArg::parse(&value)?);
                }
                "--mode" => {
                    let value = iter.next().ok_or("missing value for --mode")?;
                    mode = Some(ModeArg::parse(&value)?);
                }
                "--scenario" => {
                    let value = iter.next().ok_or("missing value for --scenario")?;
                    scenario = Some(ScenarioArg::parse(&value)?);
                }
                "--out-dir" => {
                    let value = iter.next().ok_or("missing value for --out-dir")?;
                    out_dir = Some(PathBuf::from(value));
                }
                "--frame-no" => {
                    let value = iter.next().ok_or("missing value for --frame-no")?;
                    frame_no = value
                        .parse::<u32>()
                        .map_err(|_| format!("invalid --frame-no value: {value}"))?;
                }
                "--help" | "-h" => {
                    return Err(help_text());
                }
                unknown => {
                    return Err(format!("unknown argument: {unknown}\n\n{}", help_text()));
                }
            }
        }

        let variant = variant.ok_or_else(|| format!("missing --variant\n\n{}", help_text()))?;
        let focus = focus.ok_or_else(|| format!("missing --focus\n\n{}", help_text()))?;
        let out_dir = out_dir.ok_or_else(|| format!("missing --out-dir\n\n{}", help_text()))?;
        let mode = mode.unwrap_or_else(|| ModeArg::from_focus(focus));
        let scenario = scenario.unwrap_or(ScenarioArg::Default);

        Ok(Self {
            variant,
            mode,
            focus,
            scenario,
            out_dir,
            frame_no,
        })
    }
}

fn help_text() -> String {
    [
        "Usage:",
        "  front-panel-preview --variant {A|B|C|D} --focus {idle|up|down|left|right|center|touch} [--mode {off|standby|supplement|backup}] [--scenario {default|display-diag|dashboard-runtime-standby|dashboard-runtime-assist|dashboard-runtime-backup|bq40-offline|bq40-offline-dialog|bq40-activating|bq40-result-success|bq40-result-no-battery|bq40-result-rom-mode|bq40-result-abnormal|bq40-result-not-detected|test-audio|test-navigation}] --out-dir <ABS_PATH> [--frame-no <n>]",
        "",
        "Example:",
        "  cargo run --manifest-path tools/front-panel-preview/Cargo.toml -- --variant C --focus idle --mode standby --scenario bq40-offline-dialog --out-dir /tmp/front-panel-preview",
    ]
    .join("\n")
}

struct FrameBuffer {
    width: usize,
    height: usize,
    pixels: Vec<u16>,
}

impl FrameBuffer {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; width * height],
        }
    }

    fn write_raw_le(&self, path: &Path) -> io::Result<()> {
        let mut file = fs::File::create(path)?;
        for pixel in &self.pixels {
            file.write_all(&pixel.to_le_bytes())?;
        }
        Ok(())
    }

    fn write_png(&self, path: &Path) -> io::Result<()> {
        let mut image = RgbImage::new(self.width as u32, self.height as u32);

        for (index, pixel) in self.pixels.iter().enumerate() {
            let x = (index % self.width) as u32;
            let y = (index / self.width) as u32;
            image.put_pixel(x, y, Rgb(rgb565_to_rgb888(*pixel)));
        }

        image.save(path).map_err(io::Error::other)
    }
}

impl UiPainter for FrameBuffer {
    type Error = Infallible;

    fn fill_rect(
        &mut self,
        x: u16,
        y: u16,
        w: u16,
        h: u16,
        rgb565: u16,
    ) -> Result<(), Self::Error> {
        let x0 = x as usize;
        let y0 = y as usize;
        let x1 = x0.saturating_add(w as usize).min(self.width);
        let y1 = y0.saturating_add(h as usize).min(self.height);

        for yy in y0..y1 {
            let row = yy * self.width;
            for xx in x0..x1 {
                self.pixels[row + xx] = rgb565;
            }
        }

        Ok(())
    }
}

fn rgb565_to_rgb888(raw: u16) -> [u8; 3] {
    let r = ((raw >> 11) & 0x1f) as u8;
    let g = ((raw >> 5) & 0x3f) as u8;
    let b = (raw & 0x1f) as u8;

    [
        (r as u16 * 255 / 31) as u8,
        (g as u16 * 255 / 63) as u8,
        (b as u16 * 255 / 31) as u8,
    ]
}
