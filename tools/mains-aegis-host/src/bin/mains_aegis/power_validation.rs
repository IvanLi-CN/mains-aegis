use anyhow::{anyhow, bail, Context};
use chrono::Utc;
use clap::{Args, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::time::{sleep, timeout, Duration, Instant, MissedTickBehavior};

const DEFAULT_REPORT_ROOT: &str = "tools/hil/reports";
const DEFAULT_SAMPLE_INTERVAL_MS: u64 = 200;
const DEFAULT_WATCH_FRESHNESS_MS: u64 = 750;
const MIN_FORMAL_SAMPLE_RATE_HZ: f64 = 2.0;
const ENGINEERING_SAMPLE_RATE_HZ: f64 = 3.0;
const MAX_SAMPLE_GAP_S: f64 = 0.5;
const SOURCE_DISCONNECT_CONFIRM_ATTEMPTS: usize = 50;
const SOURCE_DISCONNECT_CONFIRM_INTERVAL_MS: u64 = 100;
const UPS_ONLINE_RECOVER_ATTEMPTS: usize = 100;
const UPS_ONLINE_RECOVER_INTERVAL_MS: u64 = 100;
const SOURCE_LIMITED_ENTRY_MAX_S: f64 = 2.0;
const SOURCE_LIMITED_LOAD_MARGIN_MV: i64 = 1_000;
const SOURCE_LIMITED_MAX_LOW_VOLTAGE_S: f64 = 1.0;

#[derive(Debug)]
pub struct PowerValidationArgs {
    pub ups_ipc: String,
    pub no_auto_start: bool,
}

#[derive(Debug, Subcommand)]
pub enum PowerValidationCommand {
    /// Check adapter and UPS telemetry readiness without running a power scene.
    Check(CheckArgs),
    /// Run one scene or the full 12V/19V validation suite.
    Run(RunArgs),
    /// Verify and summarize an existing power-validation report directory.
    Report(ReportArgs),
    /// Compose a suite report from existing scene result directories.
    Compose(ComposeArgs),
    /// Print the external adapter JSON protocol contract.
    AdapterProtocol,
}

#[derive(Debug, Args)]
pub struct CheckArgs {
    #[command(flatten)]
    bench: BenchArgs,
    /// Samples to request from each streaming adapter.
    #[arg(long, default_value_t = 40)]
    samples: usize,
}

#[derive(Debug, Args)]
pub struct RunArgs {
    #[command(flatten)]
    bench: BenchArgs,
    /// Validation suite contract. The default preserves the existing four-scene suite.
    #[arg(long, value_enum, default_value_t = SuiteContract::Standard)]
    suite_contract: SuiteContract,
    /// Output profiles to run.
    #[arg(long = "profile", value_enum, default_values_t = [OutputProfile::V12, OutputProfile::V19])]
    profiles: Vec<OutputProfile>,
    /// Scenes to run.
    #[arg(long = "scene", value_enum, default_values_t = [SceneKind::AssistPath, SceneKind::BackupOnly])]
    scenes: Vec<SceneKind>,
    /// Report root directory.
    #[arg(long, default_value = DEFAULT_REPORT_ROOT)]
    report_root: PathBuf,
    /// Optional suite id. Defaults to a UTC timestamped id.
    #[arg(long)]
    suite_id: Option<String>,
    /// Generate commands and metadata without changing hardware state.
    #[arg(long)]
    dry_run: bool,
    /// Allow the runner to flash the requested 12V/19V firmware profile when the UPS profile does not match.
    #[arg(long)]
    allow_profile_flash: bool,
    /// Manifest used when switching the UPS to the 12V output profile.
    #[arg(long)]
    artifact_manifest_12v: Option<PathBuf>,
    /// Manifest used when switching the UPS to the 19V output profile.
    #[arg(long)]
    artifact_manifest_19v: Option<PathBuf>,
    /// Seconds to sample before load is enabled.
    #[arg(long, default_value_t = 8.0)]
    pre_s: f64,
    /// Seconds to hold the configured load before backup cut.
    #[arg(long, default_value_t = 16.0)]
    hold_s: f64,
    /// Seconds to hold after input cut.
    #[arg(long, default_value_t = 12.0)]
    backup_s: f64,
    /// Seconds to hold after input restore.
    #[arg(long, default_value_t = 12.0)]
    restore_s: f64,
    /// Seconds to sample after load is disabled.
    #[arg(long, default_value_t = 8.0)]
    post_s: f64,
    /// Seconds to wait after a profile firmware flash before reading UPS identity again.
    #[arg(long, default_value_t = 12)]
    profile_flash_settle_s: u64,
    /// Expected input UVLO cutoff for source-limited suite settings preflight.
    #[arg(long)]
    expected_input_uvlo_cutoff_mv: Option<u16>,
    /// Expected input UVLO recovery threshold for source-limited suite settings preflight.
    #[arg(long)]
    expected_input_uvlo_recover_mv: Option<u16>,
    /// Expected consecutive fresh samples required by the source-limited suite input UVLO preflight.
    #[arg(long)]
    expected_input_uvlo_required_samples: Option<u8>,
    /// Render chart HTML by invoking tools/hil/render_voltage_chart_html.py.
    #[arg(long, default_value = "tools/hil/render_voltage_chart_html.py")]
    render_chart: PathBuf,
}

#[derive(Debug, Args)]
pub struct ReportArgs {
    /// Existing suite directory or suite summary JSON.
    path: PathBuf,
    /// Rewrite suite-overview.html from the verified suite summary.
    #[arg(long)]
    write_overview: bool,
}

#[derive(Debug, Args)]
pub struct ComposeArgs {
    /// Validation suite contract used by the supplied scene reports.
    #[arg(long, value_enum, default_value_t = SuiteContract::Standard)]
    suite_contract: SuiteContract,
    /// Suite id to write into the generated suite-summary.json.
    #[arg(long)]
    suite_id: String,
    /// Output suite directory that will receive suite-summary.json and suite-overview.html.
    #[arg(long)]
    output_dir: PathBuf,
    /// Existing scene report directories containing results.json, timeseries.jsonl, and voltage-chart.html.
    #[arg(required = true)]
    scene_dirs: Vec<PathBuf>,
}

#[derive(Debug, Args, Clone)]
pub struct BenchArgs {
    /// UPS saved devd device id.
    #[arg(long, env = "MAINS_AEGIS_UPS_DEVICE_ID")]
    ups_device: String,
    /// UPS CLI path. Defaults to this executable.
    #[arg(long)]
    ups_cli: Option<PathBuf>,
    /// Power source adapter.
    #[arg(long, value_enum, default_value_t = PowerAdapterKind::Isolapurr)]
    power_adapter: PowerAdapterKind,
    /// External command implementing the power-source adapter protocol.
    #[arg(long)]
    power_adapter_cmd: Option<PathBuf>,
    /// Power source saved device id for the built-in IsolaPurr adapter.
    #[arg(long, env = "MAINS_AEGIS_POWER_DEVICE_ID")]
    power_device: String,
    /// Electronic load adapter.
    #[arg(long, value_enum, default_value_t = LoadAdapterKind::Loadlynx)]
    load_adapter: LoadAdapterKind,
    /// External command implementing the electronic-load adapter protocol.
    #[arg(long)]
    load_adapter_cmd: Option<PathBuf>,
    /// Electronic load saved device id for the built-in LoadLynx adapter.
    #[arg(long, env = "MAINS_AEGIS_LOAD_DEVICE_ID")]
    load_device: String,
    /// LoadLynx CLI path for the built-in adapter.
    #[arg(long, env = "LOADLYNX_CLI")]
    load_cli: Option<PathBuf>,
    /// LoadLynx devd IPC endpoint for the built-in adapter.
    #[arg(long)]
    load_ipc: Option<String>,
    /// IsolaPurr CLI path for the built-in adapter.
    #[arg(long, default_value = "isolapurr")]
    isolapurr_cli: PathBuf,
    /// IsolaPurr devd IPC endpoint for the built-in adapter.
    #[arg(long, env = "ISOLAPURR_IPC")]
    isolapurr_ipc: Option<String>,
    /// IsolaPurr URL for the built-in adapter. Use when the stable source path is LAN HTTP.
    #[arg(long, env = "ISOLAPURR_URL")]
    isolapurr_url: Option<String>,
    /// Sample interval in milliseconds.
    #[arg(long, default_value_t = DEFAULT_SAMPLE_INTERVAL_MS)]
    sample_interval_ms: u64,
    /// UPS watch freshness budget in milliseconds.
    #[arg(long, default_value_t = DEFAULT_WATCH_FRESHNESS_MS)]
    ups_watch_freshness_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum OutputProfile {
    #[value(name = "12v")]
    V12,
    #[value(name = "19v")]
    V19,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum SuiteContract {
    #[value(name = "standard")]
    Standard,
    #[value(name = "source-limited-12v")]
    #[serde(rename = "source_limited_12v")]
    SourceLimited12v,
    #[value(name = "source-limited-19v")]
    #[serde(rename = "source_limited_19v")]
    SourceLimited19v,
}

impl SuiteContract {
    fn key(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::SourceLimited12v => "source_limited_12v",
            Self::SourceLimited19v => "source_limited_19v",
        }
    }

    fn is_source_limited(self) -> bool {
        matches!(self, Self::SourceLimited12v | Self::SourceLimited19v)
    }

    fn selected_profiles(self, requested: &[OutputProfile]) -> Vec<OutputProfile> {
        match self {
            Self::Standard => requested.to_vec(),
            Self::SourceLimited12v => vec![OutputProfile::V12],
            Self::SourceLimited19v => vec![OutputProfile::V19],
        }
    }

    fn selected_scenes(self, requested: &[SceneKind]) -> Vec<SceneKind> {
        match self {
            Self::Standard => requested.to_vec(),
            Self::SourceLimited12v => vec![
                SceneKind::BackupOnly,
                SceneKind::SourceInBudget,
                SceneKind::SourceLimitedOnline,
                SceneKind::SourceLimitedCut,
            ],
            Self::SourceLimited19v => vec![
                SceneKind::BackupOnly,
                SceneKind::SourceInBudget,
                SceneKind::SourceLimitedOnline,
                SceneKind::SourceLimitedCut,
            ],
        }
    }

    fn expected_reports(self) -> Vec<(&'static str, &'static str)> {
        match self {
            Self::Standard => vec![
                ("12v", "assist_path"),
                ("12v", "backup_only"),
                ("19v", "assist_path"),
                ("19v", "backup_only"),
            ],
            Self::SourceLimited12v => vec![
                ("12v", "backup_only"),
                ("12v", "source_in_budget"),
                ("12v", "source_limited_online"),
                ("12v", "source_limited_cut"),
            ],
            Self::SourceLimited19v => vec![
                ("19v", "backup_only"),
                ("19v", "source_in_budget"),
                ("19v", "source_limited_online"),
                ("19v", "source_limited_cut"),
            ],
        }
    }

    fn from_summary(value: &Value) -> anyhow::Result<Self> {
        match value.get("suite_contract").and_then(Value::as_str) {
            None | Some("standard") => Ok(Self::Standard),
            Some("source_limited_12v") => Ok(Self::SourceLimited12v),
            Some("source_limited_19v") => Ok(Self::SourceLimited19v),
            Some(other) => bail!("unknown suite contract in report: {other}"),
        }
    }
}

impl OutputProfile {
    fn key(self) -> &'static str {
        match self {
            Self::V12 => "12v",
            Self::V19 => "19v",
        }
    }

    fn source_voltage_mv(self) -> u32 {
        match self {
            Self::V12 => 12_000,
            Self::V19 => 19_000,
        }
    }

    fn rated_vout_mv(self) -> u32 {
        self.source_voltage_mv()
    }

    fn source_current_limit_ma(self) -> u32 {
        3_000
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SourceLimitedUvloExpectation {
    cutoff_mv: u16,
    recover_mv: u16,
    required_samples: u8,
}

#[derive(Debug, Clone, Copy)]
struct SourceLimitedSettingsExpectation {
    source_limited_vin_drop_pct: u8,
    source_limited_enter_delta_ma: i16,
    source_limited_exit_delta_ma: i16,
    source_limited_required_samples: u8,
    source_limited_recover_margin_mv: u16,
    vin_drop_threshold_pct: u8,
}

impl SourceLimitedUvloExpectation {
    fn for_profile(profile: OutputProfile) -> Self {
        match profile {
            OutputProfile::V12 => Self {
                cutoff_mv: 11_300,
                recover_mv: 11_500,
                required_samples: 3,
            },
            OutputProfile::V19 => Self {
                cutoff_mv: 18_200,
                recover_mv: 18_400,
                required_samples: 3,
            },
        }
    }

    fn from_run_args(profile: OutputProfile, args: &RunArgs) -> Self {
        let defaults = Self::for_profile(profile);
        Self {
            cutoff_mv: args
                .expected_input_uvlo_cutoff_mv
                .unwrap_or(defaults.cutoff_mv),
            recover_mv: args
                .expected_input_uvlo_recover_mv
                .unwrap_or(defaults.recover_mv),
            required_samples: args
                .expected_input_uvlo_required_samples
                .unwrap_or(defaults.required_samples),
        }
    }
}

impl SourceLimitedSettingsExpectation {
    fn for_profile(profile: OutputProfile) -> Self {
        match profile {
            OutputProfile::V12 => Self {
                source_limited_vin_drop_pct: 1,
                source_limited_enter_delta_ma: 2_500,
                source_limited_exit_delta_ma: 0,
                source_limited_required_samples: 2,
                source_limited_recover_margin_mv: 400,
                vin_drop_threshold_pct: 4,
            },
            OutputProfile::V19 => Self {
                source_limited_vin_drop_pct: 1,
                source_limited_enter_delta_ma: 1_000,
                source_limited_exit_delta_ma: 0,
                source_limited_required_samples: 2,
                source_limited_recover_margin_mv: 400,
                vin_drop_threshold_pct: 4,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum SceneKind {
    #[value(name = "assist-path")]
    AssistPath,
    #[value(name = "backup-only")]
    BackupOnly,
    #[value(name = "source-in-budget")]
    SourceInBudget,
    #[value(name = "source-limited-online")]
    SourceLimitedOnline,
    #[value(name = "source-limited-cut")]
    SourceLimitedCut,
}

impl SceneKind {
    fn key(self) -> &'static str {
        match self {
            Self::AssistPath => "assist_path",
            Self::BackupOnly => "backup_only",
            Self::SourceInBudget => "source_in_budget",
            Self::SourceLimitedOnline => "source_limited_online",
            Self::SourceLimitedCut => "source_limited_cut",
        }
    }

    fn target_ma(self) -> u32 {
        match self {
            Self::AssistPath => 3_900,
            Self::BackupOnly => 1_000,
            Self::SourceInBudget => 2_500,
            Self::SourceLimitedOnline | Self::SourceLimitedCut => 3_900,
        }
    }

    fn include_backup(self) -> bool {
        match self {
            Self::AssistPath | Self::BackupOnly | Self::SourceLimitedCut => true,
            Self::SourceInBudget | Self::SourceLimitedOnline => false,
        }
    }

    fn requires_source_limited(self) -> bool {
        matches!(self, Self::SourceLimitedOnline | Self::SourceLimitedCut)
    }

    fn requires_non_backup_online(self) -> bool {
        matches!(self, Self::SourceInBudget)
    }

    fn requires_input_absent_after_cut(self) -> bool {
        matches!(self, Self::BackupOnly | Self::SourceLimitedCut)
    }

    fn requires_source_limited_before_cut(self) -> bool {
        matches!(self, Self::SourceLimitedCut)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PowerAdapterKind {
    Isolapurr,
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LoadAdapterKind {
    Loadlynx,
    External,
}

#[derive(Debug, Serialize)]
struct SuitePlan {
    suite_id: String,
    suite_contract: SuiteContract,
    created_at: String,
    transport: TransportPlan,
    thresholds: Thresholds,
    load_protection: LoadProtection,
    profiles: Vec<ProfilePlan>,
    reports: Vec<ScenePlan>,
}

#[derive(Debug, Serialize)]
struct TransportPlan {
    ups: String,
    power_source: String,
    electronic_load: String,
}

#[derive(Debug, Serialize)]
struct Thresholds {
    engineering_sample_rate_hz: f64,
    minimum_sample_rate_hz: f64,
    max_sample_gap_s: f64,
    sample_interval_ms: u64,
    ups_watch_freshness_ms: u64,
}

#[derive(Debug, Serialize, Clone, Copy)]
struct LoadProtection {
    load_min_v_mv: u32,
    load_max_i_ma_total: u32,
    load_max_p_mw: u32,
}

#[derive(Debug, Serialize)]
struct ProfilePlan {
    output_profile: &'static str,
    source_voltage_mv: u32,
    source_current_limit_ma: u32,
    rated_vout_mv: u32,
}

#[derive(Debug, Serialize)]
struct ScenePlan {
    output_profile: &'static str,
    scene_type: &'static str,
    target_ma: u32,
    include_backup: bool,
    report_dir: String,
    source_voltage_mv: u32,
    source_current_limit_ma: u32,
    load_min_v_mv: u32,
    load_max_i_ma_total: u32,
    load_max_p_mw: u32,
    commands: SceneCommands,
}

#[derive(Debug, Serialize)]
struct SceneCommands {
    power_capabilities: Vec<String>,
    load_capabilities: Vec<String>,
    load_disable: Vec<String>,
    power_disable: Vec<String>,
    ups_artifact_select: Vec<String>,
    ups_flash: Vec<String>,
    ups_identity: Vec<String>,
    ups_settings: Vec<String>,
    power_configure_off: Vec<String>,
    power_enable: Vec<String>,
    power_port_enable: Vec<String>,
    load_cc: Vec<String>,
    ups_status_watch: Vec<String>,
    load_stream: Vec<String>,
    power_stream: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct AdapterFrame {
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    ok: Option<bool>,
    #[serde(default)]
    sample: Option<AdapterSample>,
    #[serde(flatten)]
    raw: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Serialize)]
struct RawFrame {
    received_ms: i64,
    value: Value,
}

#[derive(Debug)]
struct JsonlProcessCollector {
    name: String,
    cmd: Vec<String>,
    rows: Arc<Mutex<Vec<RawFrame>>>,
    errors: Arc<Mutex<Vec<String>>>,
    child: Child,
    stop_flag: Option<Arc<AtomicBool>>,
}

#[derive(Debug, Deserialize, Serialize)]
struct SceneSample {
    t_s: f64,
    unix_ms: i64,
    phase: String,
    stage: Option<Value>,
    mode: Option<Value>,
    backup_reason: Option<Value>,
    load_target_i_ma: u32,
    ups_status_cache_age_ms: Option<Value>,
    ups_status_cache_fresh: Option<Value>,
    ups_status_monitor_running: Option<Value>,
    port_c_enabled: Option<Value>,
    isolapurr_port_c_mv: Option<Value>,
    isolapurr_port_c_ma: Option<Value>,
    mains_present: Option<Value>,
    assist_target_vout_mv: Option<Value>,
    vin_vbus_mv: Option<Value>,
    vin_iin_ma: Option<Value>,
    tps_total_iout_ma: Option<Value>,
    battery_current_ma: Option<Value>,
    charger_state: Option<Value>,
    charger_allow_charge: Option<Value>,
    out_a_vbus_mv: Option<Value>,
    out_b_vbus_mv: Option<Value>,
    out_a_iout_ma: Option<Value>,
    out_b_iout_ma: Option<Value>,
    diag_stage: Option<Value>,
    diag_backup_reason: Option<Value>,
    diag_charger_notice: Option<Value>,
    diag_assist_target_vout_mv: Option<Value>,
    diag_vin_baseline_mv: Option<Value>,
    diag_vin_drop_mv: Option<Value>,
    diag_tps_total_iout_ma: Option<Value>,
    load_output_enabled: Option<Value>,
    load_v_local_mv: Option<Value>,
    load_i_total_ma: Option<Value>,
}

const HOLD_TPS_POWER_MAX_MW: i64 = 2_000;

#[derive(Debug, Serialize)]
struct SceneSummary {
    scene_complete: bool,
    failures: Vec<String>,
    effective_sample_rate_hz: Option<f64>,
    max_sample_gap_s: Option<f64>,
    required_voltage_series: BTreeMap<String, bool>,
}

#[derive(Debug, Deserialize)]
struct AdapterSample {
    #[serde(default)]
    timestamp_ms: Option<i64>,
    #[serde(default)]
    voltage_mv: Option<f64>,
    #[serde(default)]
    current_ma: Option<f64>,
    #[serde(default)]
    power_mw: Option<f64>,
    #[serde(default)]
    enabled: Option<bool>,
}

pub async fn run(
    command: PowerValidationCommand,
    context: PowerValidationArgs,
) -> anyhow::Result<()> {
    match command {
        PowerValidationCommand::Check(args) => run_check(args, context).await,
        PowerValidationCommand::Run(args) => run_suite(args, context).await,
        PowerValidationCommand::Report(args) => run_report(args).await,
        PowerValidationCommand::Compose(args) => run_compose_report(args).await,
        PowerValidationCommand::AdapterProtocol => {
            println!("{}", serde_json::to_string_pretty(&adapter_protocol())?);
            Ok(())
        }
    }
}

async fn run_check(args: CheckArgs, context: PowerValidationArgs) -> anyhow::Result<()> {
    let ups_monitor =
        run_cmd_output(ups_command(&args.bench, &context, ["monitor", "start"])).await;
    let source_command = power_stream_command(&args.bench)?;
    let load_command = preflight_load_stream_command(&args.bench, args.samples)?;
    let ups_warmup = if ups_monitor.is_ok() {
        wait_for_ups_watch_ready(&args.bench, &context, "check_warmup").await
    } else {
        Err(anyhow!("UPS monitor did not start"))
    };
    // Preflight isolates each device's cadence from host-side process contention.
    let ups_status = run_json_probe(
        ups_watch_command(&args.bench, &context, "status", args.samples),
        args.samples,
    )
    .await;
    let ups_diag_snapshot = run_json_probe(
        ups_watch_command(&args.bench, &context, "diag-snapshot", args.samples),
        args.samples,
    )
    .await;
    let source = run_polling_json_probe(
        source_command,
        args.samples,
        Duration::from_millis(args.bench.sample_interval_ms),
    )
    .await;
    let load = run_adapter_probe(load_command, args.samples).await;
    let ups_ok = ups_monitor.is_ok()
        && ups_warmup.as_ref().is_ok_and(|probe| probe.ok)
        && ups_status.as_ref().is_ok_and(|probe| probe.ok)
        && ups_diag_snapshot.as_ref().is_ok_and(|probe| probe.ok);
    let ok = ups_ok
        && source.as_ref().is_ok_and(|probe| probe.ok)
        && load.as_ref().is_ok_and(|probe| probe.ok);
    let output = json!({
        "ok": ok,
        "ups": {
            "device_id": args.bench.ups_device,
            "ipc": context.ups_ipc,
            "monitor": match ups_monitor {
                Ok(value) => json!({"ok": true, "result": value}),
                Err(error) => json!({"ok": false, "error": error.to_string()}),
            },
            "warmup": match ups_warmup {
                Ok(probe) => json!(probe),
                Err(error) => json!({"ok": false, "error": error.to_string()}),
            },
            "status": match ups_status {
                Ok(probe) => json!(probe),
                Err(error) => json!({"ok": false, "error": error.to_string()}),
            },
            "diag_snapshot": match ups_diag_snapshot {
                Ok(probe) => json!(probe),
                Err(error) => json!({"ok": false, "error": error.to_string()}),
            },
        },
        "power_source": {
            "adapter": format!("{:?}", args.bench.power_adapter),
            "probe": match source {
                Ok(probe) => json!(probe),
                Err(error) => json!({"ok": false, "error": error.to_string()}),
            },
        },
        "electronic_load": {
            "adapter": format!("{:?}", args.bench.load_adapter),
            "probe": match load {
                Ok(probe) => json!(probe),
                Err(error) => json!({"ok": false, "error": error.to_string()}),
            },
        },
    });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

async fn run_suite(args: RunArgs, context: PowerValidationArgs) -> anyhow::Result<()> {
    let suite_id = args
        .suite_id
        .clone()
        .unwrap_or_else(|| format!("power-validation-{}", Utc::now().format("%Y%m%dT%H%M%SZ")));
    let suite_dir = args.report_root.join(&suite_id);
    fs::create_dir_all(&suite_dir).with_context(|| format!("creating {}", suite_dir.display()))?;
    let plan = build_suite_plan(&args, &context, &suite_id, &suite_dir)?;
    let summary_path = suite_dir.join("suite-summary.json");
    write_json(&summary_path, &plan)?;
    if args.dry_run {
        write_suite_overview(
            &suite_dir.join("suite-overview.html"),
            &serde_json::to_value(&plan)?,
        )?;
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "ok": true,
                "dry_run": true,
                "suite_dir": suite_dir,
                "summary": summary_path,
                "overview": suite_dir.join("suite-overview.html"),
                "plan": plan,
            }))?
        );
        return Ok(());
    }
    let mut reports = Vec::new();
    for profile in args.suite_contract.selected_profiles(&args.profiles) {
        for scene in args.suite_contract.selected_scenes(&args.scenes) {
            reports.push(
                run_scene(
                    &args,
                    &context,
                    &suite_dir,
                    profile,
                    scene,
                    args.suite_contract,
                )
                .await?,
            );
        }
    }
    let mut suite_value = serde_json::to_value(&plan)?;
    suite_value["reports"] = json!(reports);
    write_json_value(&summary_path, &suite_value)?;
    write_suite_overview(&suite_dir.join("suite-overview.html"), &suite_value)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "ok": true,
            "suite_dir": suite_dir,
            "summary": summary_path,
            "overview": suite_dir.join("suite-overview.html"),
            "reports": reports,
        }))?
    );
    Ok(())
}

async fn run_report(args: ReportArgs) -> anyhow::Result<()> {
    let result = verify_report(args.path, args.write_overview)?;
    let signoff_valid = result
        .get("signoff_valid")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    println!("{}", serde_json::to_string_pretty(&result)?);
    if !signoff_valid {
        bail!("power-validation report is not valid for sign-off");
    }
    Ok(())
}

fn verify_report(path: PathBuf, write_overview: bool) -> anyhow::Result<Value> {
    let summary_path = if path.is_dir() {
        path.join("suite-summary.json")
    } else {
        path.clone()
    };
    let summary_dir = summary_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let value: Value = serde_json::from_str(
        &fs::read_to_string(&summary_path)
            .with_context(|| format!("reading {}", summary_path.display()))?,
    )?;
    let reports = value
        .get("reports")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let failures: Vec<Value> = reports
        .iter()
        .filter(|report| report.get("signoff_valid").and_then(Value::as_bool) != Some(true))
        .cloned()
        .collect();
    let suite_contract = SuiteContract::from_summary(&value)?;
    let mut suite_failures = Vec::new();
    let mut report_failures = Vec::new();
    let expected_reports = suite_contract.expected_reports();
    if reports.len() != expected_reports.len() {
        suite_failures.push(json!({
            "suite_failure": "unexpected_report_count",
            "suite_contract": suite_contract.key(),
            "expected": expected_reports.len(),
            "actual": reports.len(),
        }));
    }
    for (profile, scene) in expected_reports {
        let found = reports.iter().any(|report| {
            report.get("output_profile").and_then(Value::as_str) == Some(profile)
                && report.get("scene_type").and_then(Value::as_str) == Some(scene)
        });
        if !found {
            suite_failures.push(json!({"suite_failure": "missing_scene", "output_profile": profile, "scene_type": scene}));
        }
    }
    for report in &reports {
        report_failures.extend(validate_scene_report(&summary_dir, report, suite_contract)?);
    }
    let overview_path = summary_dir.join("suite-overview.html");
    if write_overview {
        write_suite_overview(&overview_path, &value)?;
    }
    let signoff_valid =
        failures.is_empty() && suite_failures.is_empty() && report_failures.is_empty();
    Ok(json!({
        "ok": signoff_valid,
        "summary": summary_path,
        "overview": overview_path,
        "overview_written": write_overview,
        "reports": reports.len(),
        "signoff_valid": signoff_valid,
        "failures": failures,
        "suite_failures": suite_failures,
        "report_failures": report_failures,
        "suite_id": value.get("suite_id"),
        "suite_contract": suite_contract.key(),
        "thresholds": value.get("thresholds"),
    }))
}

async fn run_compose_report(args: ComposeArgs) -> anyhow::Result<()> {
    fs::create_dir_all(&args.output_dir)
        .with_context(|| format!("creating {}", args.output_dir.display()))?;
    let mut reports = Vec::new();
    for scene_dir in &args.scene_dirs {
        reports.push(compose_scene_report_entry(&args.output_dir, scene_dir)?);
    }
    let suite = json!({
        "suite_id": args.suite_id,
        "suite_contract": args.suite_contract.key(),
        "created_at": Utc::now().to_rfc3339(),
        "transport": infer_suite_transport(&reports),
        "thresholds": {
            "engineering_sample_rate_hz": ENGINEERING_SAMPLE_RATE_HZ,
            "minimum_sample_rate_hz": MIN_FORMAL_SAMPLE_RATE_HZ,
            "max_sample_gap_s": MAX_SAMPLE_GAP_S,
        },
        "load_protection": infer_load_protection(&reports),
        "profiles": infer_profiles(&reports),
        "reports": reports,
    });
    let summary_path = args.output_dir.join("suite-summary.json");
    let overview_path = args.output_dir.join("suite-overview.html");
    write_json_value(&summary_path, &suite)?;
    write_suite_overview(&overview_path, &suite)?;
    let verification = verify_report(summary_path.clone(), true)?;
    let signoff_valid = verification
        .get("signoff_valid")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "ok": signoff_valid,
            "summary": summary_path,
            "overview": overview_path,
            "reports": suite.get("reports").and_then(Value::as_array).map(Vec::len).unwrap_or(0),
            "verification": verification,
        }))?
    );
    if !signoff_valid {
        bail!("power-validation composed report is not valid for sign-off");
    }
    Ok(())
}

fn compose_scene_report_entry(suite_dir: &Path, scene_dir: &Path) -> anyhow::Result<Value> {
    let results_path = scene_dir.join("results.json");
    let results: Value = serde_json::from_str(
        &fs::read_to_string(&results_path)
            .with_context(|| format!("reading {}", results_path.display()))?,
    )?;
    let metadata = results.get("metadata").unwrap_or(&Value::Null);
    let completeness = results
        .pointer("/summary/all/completeness")
        .unwrap_or(&Value::Null);
    let acceptance = results
        .pointer("/summary/all/acceptance")
        .unwrap_or(&Value::Null);
    let output_profile = metadata
        .get("output_profile")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("{} missing metadata.output_profile", results_path.display()))?;
    let scene_type = metadata
        .get("scene_type")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("{} missing metadata.scene_type", results_path.display()))?;
    let target_ma = metadata
        .get("target_ma")
        .and_then(Value::as_i64)
        .ok_or_else(|| anyhow!("{} missing metadata.target_ma", results_path.display()))?;
    let report_dir = path_relative_to(scene_dir, suite_dir);
    Ok(json!({
        "report_dir": report_dir,
        "suite_contract": metadata.get("suite_contract").cloned().unwrap_or_else(|| json!("standard")),
        "output_profile": output_profile,
        "scene_type": scene_type,
        "target_ma": target_ma,
        "include_backup": metadata.get("include_backup").cloned().unwrap_or_else(|| json!(true)),
        "source_voltage_mv": metadata.get("source_voltage_mv").cloned().unwrap_or(Value::Null),
        "source_current_limit_ma": metadata.get("source_current_limit_ma").cloned().unwrap_or(Value::Null),
        "load_min_v_mv": metadata.get("load_min_v_mv").cloned().unwrap_or(Value::Null),
        "load_max_i_ma_total": metadata.get("max_i_ma_total").cloned().unwrap_or(Value::Null),
        "load_max_p_mw": metadata.get("max_p_mw").cloned().unwrap_or(Value::Null),
        "effective_sample_rate_hz": completeness.get("effective_sample_rate_hz").cloned().unwrap_or(Value::Null),
        "max_sample_gap_s": completeness.get("max_sample_gap_s").cloned().unwrap_or(Value::Null),
        "scene_complete": completeness.get("scene_complete").cloned().unwrap_or(Value::Null),
        "run_validity": acceptance.get("run_validity").cloned().unwrap_or(Value::Null),
        "signoff_valid": acceptance.get("signoff_valid").cloned().unwrap_or(Value::Null),
        "failures": completeness.get("failures").cloned().unwrap_or_else(|| json!([])),
        "failed_acceptance_checks": acceptance.get("failed_acceptance_checks").cloned().unwrap_or_else(|| json!([])),
        "advanced_power": results.pointer("/settings_snapshot/advanced_power").cloned().unwrap_or_else(|| json!({})),
        "scene_assertions": results.get("scene_assertions").cloned().unwrap_or(Value::Null),
        "transport": {
            "ups": metadata.get("ups_transport").cloned().unwrap_or(Value::Null),
            "power_source": metadata.get("power_transport").cloned().unwrap_or(Value::Null),
            "electronic_load": metadata.get("load_transport").cloned().unwrap_or(Value::Null),
        },
    }))
}

fn path_relative_to(path: &Path, base: &Path) -> String {
    let absolute_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let absolute_base = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
    if let Ok(stripped) = absolute_path.strip_prefix(&absolute_base) {
        return stripped.to_string_lossy().to_string();
    }
    relative_path_between(&absolute_path, &absolute_base)
        .unwrap_or(absolute_path)
        .to_string_lossy()
        .to_string()
}

fn relative_path_between(path: &Path, base: &Path) -> Option<PathBuf> {
    let path_components = path.components().collect::<Vec<_>>();
    let base_components = base.components().collect::<Vec<_>>();
    let mut common = 0usize;
    while common < path_components.len()
        && common < base_components.len()
        && path_components[common] == base_components[common]
    {
        common += 1;
    }
    if common == 0 {
        return None;
    }
    let mut relative = PathBuf::new();
    for component in &base_components[common..] {
        match component {
            Component::Normal(_) => relative.push(".."),
            Component::CurDir => {}
            _ => return None,
        }
    }
    for component in &path_components[common..] {
        relative.push(component.as_os_str());
    }
    Some(relative)
}

fn infer_suite_transport(reports: &[Value]) -> Value {
    reports
        .iter()
        .find_map(|report| report.get("transport").cloned())
        .unwrap_or_else(|| {
            json!({
                "ups": "mains-aegis CLI + native IPC + USB",
                "power_source": "selected power-validation adapter",
                "electronic_load": "selected power-validation adapter",
            })
        })
}

fn infer_load_protection(reports: &[Value]) -> Value {
    let first = reports.first().unwrap_or(&Value::Null);
    json!({
        "load_min_v_mv": first.get("load_min_v_mv").cloned().unwrap_or(Value::Null),
        "load_max_i_ma_total": first.get("load_max_i_ma_total").cloned().unwrap_or(Value::Null),
        "load_max_p_mw": first.get("load_max_p_mw").cloned().unwrap_or(Value::Null),
    })
}

fn infer_profiles(reports: &[Value]) -> Vec<Value> {
    let mut by_profile: BTreeMap<String, Value> = BTreeMap::new();
    for report in reports {
        let Some(profile) = report.get("output_profile").and_then(Value::as_str) else {
            continue;
        };
        by_profile.entry(profile.to_string()).or_insert_with(|| {
            json!({
                "output_profile": profile,
                "source_voltage_mv": report.get("source_voltage_mv").cloned().unwrap_or(Value::Null),
                "source_current_limit_ma": report.get("source_current_limit_ma").cloned().unwrap_or(Value::Null),
                "rated_vout_mv": report.get("source_voltage_mv").cloned().unwrap_or(Value::Null),
            })
        });
    }
    by_profile.into_values().collect()
}

fn validate_scene_report(
    summary_dir: &Path,
    report: &Value,
    suite_contract: SuiteContract,
) -> anyhow::Result<Vec<Value>> {
    let mut failures = Vec::new();
    let Some(report_dir_value) = report.get("report_dir").and_then(Value::as_str) else {
        failures.push(json!({"report_failure": "missing_report_dir", "report": report}));
        return Ok(failures);
    };
    let report_dir = if Path::new(report_dir_value).is_absolute() {
        PathBuf::from(report_dir_value)
    } else {
        summary_dir.join(report_dir_value)
    };
    let results_path = report_dir.join("results.json");
    let timeseries_path = report_dir.join("timeseries.jsonl");
    let chart_path = report_dir.join("voltage-chart.html");

    let results_text = match fs::read_to_string(&results_path) {
        Ok(text) => text,
        Err(error) => {
            failures.push(json!({
                "report_failure": "missing_results_json",
                "report_dir": report_dir_value,
                "path": results_path,
                "error": error.to_string(),
            }));
            return Ok(failures);
        }
    };
    let results: Value = match serde_json::from_str(&results_text) {
        Ok(value) => value,
        Err(error) => {
            failures.push(json!({
                "report_failure": "invalid_results_json",
                "report_dir": report_dir_value,
                "path": results_path,
                "error": error.to_string(),
            }));
            return Ok(failures);
        }
    };

    let expected_profile = report.get("output_profile").and_then(Value::as_str);
    let expected_scene = report.get("scene_type").and_then(Value::as_str);
    let metadata = results.get("metadata").unwrap_or(&Value::Null);
    if metadata.get("output_profile").and_then(Value::as_str) != expected_profile {
        failures.push(json!({
            "report_failure": "results_profile_mismatch",
            "report_dir": report_dir_value,
            "summary": expected_profile,
            "results": metadata.get("output_profile"),
        }));
    }
    if metadata.get("scene_type").and_then(Value::as_str) != expected_scene {
        failures.push(json!({
            "report_failure": "results_scene_mismatch",
            "report_dir": report_dir_value,
            "summary": expected_scene,
            "results": metadata.get("scene_type"),
        }));
    }
    if metadata.get("target_ma") != report.get("target_ma") {
        failures.push(json!({
            "report_failure": "results_target_ma_mismatch",
            "report_dir": report_dir_value,
            "summary": report.get("target_ma"),
            "results": metadata.get("target_ma"),
        }));
    }
    if suite_contract.is_source_limited() {
        if metadata.get("suite_contract").and_then(Value::as_str) != Some(suite_contract.key()) {
            failures.push(json!({
                "report_failure": "results_suite_contract_mismatch",
                "report_dir": report_dir_value,
                "expected": suite_contract.key(),
                "results": metadata.get("suite_contract"),
            }));
        }
        if results
            .get("scene_assertions")
            .and_then(|value| value.get("passed"))
            .and_then(Value::as_bool)
            != Some(true)
        {
            failures.push(json!({
                "report_failure": "source_limited_scene_assertions_failed",
                "report_dir": report_dir_value,
                "assertions": results.get("scene_assertions"),
            }));
        }
    }

    let completeness = results
        .pointer("/summary/all/completeness")
        .unwrap_or(&Value::Null);
    let acceptance = results
        .pointer("/summary/all/acceptance")
        .unwrap_or(&Value::Null);
    if acceptance.get("signoff_valid").and_then(Value::as_bool) != Some(true) {
        failures.push(
            json!({"report_failure": "results_not_signoff_valid", "report_dir": report_dir_value}),
        );
    }
    if completeness.get("scene_complete").and_then(Value::as_bool) != Some(true) {
        failures.push(
            json!({"report_failure": "results_scene_incomplete", "report_dir": report_dir_value}),
        );
    }
    if completeness
        .get("failures")
        .and_then(Value::as_array)
        .is_some_and(|values| !values.is_empty())
    {
        failures.push(json!({
            "report_failure": "results_completeness_failures",
            "report_dir": report_dir_value,
            "failures": completeness.get("failures"),
        }));
    }
    let required_series = completeness
        .get("required_voltage_series")
        .and_then(Value::as_object);
    for series in [
        "source_output_voltage",
        "ups_dcin_voltage",
        "ups_output_voltage",
        "load_actual_voltage",
    ] {
        if required_series
            .and_then(|values| values.get(series))
            .and_then(Value::as_bool)
            != Some(true)
        {
            failures.push(json!({
                "report_failure": "missing_required_voltage_series",
                "report_dir": report_dir_value,
                "series": series,
            }));
        }
    }
    let rate = completeness
        .get("effective_sample_rate_hz")
        .and_then(Value::as_f64);
    if rate.is_none_or(|value| value < MIN_FORMAL_SAMPLE_RATE_HZ) {
        failures.push(json!({
            "report_failure": "results_sample_rate_below_2hz",
            "report_dir": report_dir_value,
            "effective_sample_rate_hz": rate,
        }));
    }
    let max_gap = completeness.get("max_sample_gap_s").and_then(Value::as_f64);
    if max_gap.is_none_or(|value| value > MAX_SAMPLE_GAP_S) {
        failures.push(json!({
            "report_failure": "results_sample_gap_above_0_5s",
            "report_dir": report_dir_value,
            "max_sample_gap_s": max_gap,
        }));
    }

    let sample_count = results
        .get("samples")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    if sample_count < 2 {
        failures.push(json!({
            "report_failure": "results_too_few_samples",
            "report_dir": report_dir_value,
            "samples": sample_count,
        }));
    }
    let timeseries_text = match fs::read_to_string(&timeseries_path) {
        Ok(text) => text,
        Err(error) => {
            failures.push(json!({
                "report_failure": "missing_timeseries_jsonl",
                "report_dir": report_dir_value,
                "path": timeseries_path,
                "error": error.to_string(),
            }));
            String::new()
        }
    };
    let mut timeseries_count = 0usize;
    let mut timeseries_samples = Vec::new();
    for (index, line) in timeseries_text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        timeseries_count += 1;
        match serde_json::from_str::<SceneSample>(line) {
            Ok(sample) => timeseries_samples.push(sample),
            Err(error) => {
                failures.push(json!({
                    "report_failure": "invalid_timeseries_jsonl_line",
                    "report_dir": report_dir_value,
                    "line": index + 1,
                    "error": error.to_string(),
                }));
                break;
            }
        }
    }
    if timeseries_count != sample_count {
        failures.push(json!({
            "report_failure": "timeseries_sample_count_mismatch",
            "report_dir": report_dir_value,
            "results_samples": sample_count,
            "timeseries_rows": timeseries_count,
        }));
    }
    if failures.iter().all(|failure| {
        failure.get("report_failure") != Some(&json!("invalid_timeseries_jsonl_line"))
    }) {
        let timeseries_summary = summarize_scene(&timeseries_samples);
        validate_timeseries_summary_against_results(
            &mut failures,
            report_dir_value,
            &timeseries_summary,
            completeness,
        );
        if suite_contract.is_source_limited()
            && matches!(
                expected_scene,
                Some("source_limited_online" | "source_limited_cut")
            )
        {
            let (_, hold_failures) = hold_power_assertions(&timeseries_samples);
            for hold_failure in hold_failures {
                failures.push(json!({
                    "report_failure": hold_failure,
                    "report_dir": report_dir_value,
                }));
            }
        }
    }
    if !chart_path.exists() {
        failures.push(json!({
            "report_failure": "missing_voltage_chart",
            "report_dir": report_dir_value,
            "path": chart_path,
        }));
    }
    Ok(failures)
}

fn validate_timeseries_summary_against_results(
    failures: &mut Vec<Value>,
    report_dir_value: &str,
    timeseries_summary: &SceneSummary,
    results_completeness: &Value,
) {
    for (series, present) in &timeseries_summary.required_voltage_series {
        if !present {
            failures.push(json!({
                "report_failure": "timeseries_missing_required_voltage_series",
                "report_dir": report_dir_value,
                "series": series,
            }));
        }
        let results_present = results_completeness
            .get("required_voltage_series")
            .and_then(Value::as_object)
            .and_then(|values| values.get(series))
            .and_then(Value::as_bool);
        if results_present != Some(*present) {
            failures.push(json!({
                "report_failure": "timeseries_required_voltage_series_mismatch",
                "report_dir": report_dir_value,
                "series": series,
                "results": results_present,
                "timeseries": present,
            }));
        }
    }

    let timeseries_rate = timeseries_summary.effective_sample_rate_hz;
    if timeseries_rate.is_none_or(|value| value < MIN_FORMAL_SAMPLE_RATE_HZ) {
        failures.push(json!({
            "report_failure": "timeseries_sample_rate_below_2hz",
            "report_dir": report_dir_value,
            "effective_sample_rate_hz": timeseries_rate,
        }));
    }
    let results_rate = results_completeness
        .get("effective_sample_rate_hz")
        .and_then(Value::as_f64);
    if results_rate != timeseries_rate {
        failures.push(json!({
            "report_failure": "timeseries_sample_rate_mismatch",
            "report_dir": report_dir_value,
            "results": results_rate,
            "timeseries": timeseries_rate,
        }));
    }

    let timeseries_gap = timeseries_summary.max_sample_gap_s;
    if timeseries_gap.is_none_or(|value| value > MAX_SAMPLE_GAP_S) {
        failures.push(json!({
            "report_failure": "timeseries_sample_gap_above_0_5s",
            "report_dir": report_dir_value,
            "max_sample_gap_s": timeseries_gap,
        }));
    }
    let results_gap = results_completeness
        .get("max_sample_gap_s")
        .and_then(Value::as_f64);
    if results_gap != timeseries_gap {
        failures.push(json!({
            "report_failure": "timeseries_sample_gap_mismatch",
            "report_dir": report_dir_value,
            "results": results_gap,
            "timeseries": timeseries_gap,
        }));
    }
}

fn build_suite_plan(
    args: &RunArgs,
    context: &PowerValidationArgs,
    suite_id: &str,
    suite_dir: &Path,
) -> anyhow::Result<SuitePlan> {
    let protection = LoadProtection {
        load_min_v_mv: 3_000,
        load_max_i_ma_total: 4_000,
        load_max_p_mw: 80_000,
    };
    let selected_profiles = args.suite_contract.selected_profiles(&args.profiles);
    let selected_scenes = args.suite_contract.selected_scenes(&args.scenes);
    let profiles = selected_profiles
        .iter()
        .copied()
        .map(|profile| ProfilePlan {
            output_profile: profile.key(),
            source_voltage_mv: profile.source_voltage_mv(),
            source_current_limit_ma: profile.source_current_limit_ma(),
            rated_vout_mv: profile.rated_vout_mv(),
        })
        .collect::<Vec<_>>();
    let mut reports = Vec::new();
    for profile in &selected_profiles {
        for scene in &selected_scenes {
            let report_name = format!("{}-{}-{}ma", profile.key(), scene.key(), scene.target_ma());
            let report_dir = suite_dir.join(&report_name);
            reports.push(ScenePlan {
                output_profile: profile.key(),
                scene_type: scene.key(),
                target_ma: scene.target_ma(),
                include_backup: scene.include_backup(),
                report_dir: report_dir.to_string_lossy().to_string(),
                source_voltage_mv: profile.source_voltage_mv(),
                source_current_limit_ma: profile.source_current_limit_ma(),
                load_min_v_mv: protection.load_min_v_mv,
                load_max_i_ma_total: protection.load_max_i_ma_total,
                load_max_p_mw: protection.load_max_p_mw,
                commands: scene_commands(args, context, *profile, *scene, protection)?,
            });
        }
    }
    Ok(SuitePlan {
        suite_id: suite_id.to_string(),
        suite_contract: args.suite_contract,
        created_at: Utc::now().to_rfc3339(),
        transport: TransportPlan {
            ups: "mains-aegis CLI + native IPC + USB".to_string(),
            power_source: power_adapter_label(&args.bench),
            electronic_load: load_adapter_label(&args.bench),
        },
        thresholds: Thresholds {
            engineering_sample_rate_hz: ENGINEERING_SAMPLE_RATE_HZ,
            minimum_sample_rate_hz: MIN_FORMAL_SAMPLE_RATE_HZ,
            max_sample_gap_s: MAX_SAMPLE_GAP_S,
            sample_interval_ms: args.bench.sample_interval_ms,
            ups_watch_freshness_ms: args.bench.ups_watch_freshness_ms,
        },
        load_protection: protection,
        profiles,
        reports,
    })
}

fn scene_commands(
    args: &RunArgs,
    context: &PowerValidationArgs,
    profile: OutputProfile,
    scene: SceneKind,
    protection: LoadProtection,
) -> anyhow::Result<SceneCommands> {
    Ok(SceneCommands {
        power_capabilities: power_capabilities_command(&args.bench)?,
        load_capabilities: load_capabilities_command(&args.bench)?,
        load_disable: load_disable_command(&args.bench)?,
        power_disable: power_disable_command(&args.bench)?,
        ups_artifact_select: ups_artifact_select_command(args, context, profile)?,
        ups_flash: ups_flash_command(args, context, profile)?,
        ups_identity: ups_command(&args.bench, context, ["identity"]),
        ups_settings: ups_command(&args.bench, context, ["settings"]),
        power_configure_off: power_configure_off_command(&args.bench, profile)?,
        power_enable: power_enable_command(&args.bench, profile)?,
        power_port_enable: power_port_enable_command(&args.bench, profile)?,
        load_cc: load_cc_command(&args.bench, scene.target_ma(), protection)?,
        ups_status_watch: ups_watch_command(&args.bench, context, "status", 0),
        load_stream: load_stream_command(&args.bench, 0)?,
        power_stream: power_stream_command(&args.bench)?,
    })
}

fn power_adapter_label(args: &BenchArgs) -> String {
    match args.power_adapter {
        PowerAdapterKind::Isolapurr => {
            if args.isolapurr_url.is_some() {
                "Isolapurr:cli+url".to_string()
            } else if args.isolapurr_ipc.is_some() {
                "Isolapurr:cli+ipc+usb".to_string()
            } else {
                "Isolapurr:cli+default".to_string()
            }
        }
        PowerAdapterKind::External => match args.power_adapter_cmd.as_ref() {
            Some(cmd) => format!("External:{}", cmd.display()),
            None => "External".to_string(),
        },
    }
}

fn load_adapter_label(args: &BenchArgs) -> String {
    match args.load_adapter {
        LoadAdapterKind::Loadlynx => "Loadlynx:released-cli+saved-device".to_string(),
        LoadAdapterKind::External => match args.load_adapter_cmd.as_ref() {
            Some(cmd) => format!("External:{}", cmd.display()),
            None => "External".to_string(),
        },
    }
}

impl From<LoadAdapterKind> for PowerAdapterKind {
    fn from(value: LoadAdapterKind) -> Self {
        match value {
            LoadAdapterKind::Loadlynx => PowerAdapterKind::Isolapurr,
            LoadAdapterKind::External => PowerAdapterKind::External,
        }
    }
}

fn this_exe_or(args: &BenchArgs) -> PathBuf {
    args.ups_cli
        .clone()
        .or_else(|| std::env::current_exe().ok())
        .unwrap_or_else(|| PathBuf::from("mains-aegis"))
}

fn ups_command<const N: usize>(
    args: &BenchArgs,
    context: &PowerValidationArgs,
    parts: [&str; N],
) -> Vec<String> {
    let mut cmd = vec![
        this_exe_or(args).to_string_lossy().to_string(),
        "--ipc".to_string(),
        context.ups_ipc.clone(),
    ];
    if context.no_auto_start {
        cmd.push("--no-auto-start".to_string());
    }
    cmd.extend(["device".to_string(), args.ups_device.clone()]);
    cmd.extend(parts.into_iter().map(ToOwned::to_owned));
    cmd
}

fn ups_watch_command(
    args: &BenchArgs,
    context: &PowerValidationArgs,
    kind: &str,
    samples: usize,
) -> Vec<String> {
    let mut cmd = ups_command(args, context, [kind]);
    cmd.extend([
        "--watch".to_string(),
        "--interval-ms".to_string(),
        args.sample_interval_ms.to_string(),
        "--watch-freshness-ms".to_string(),
        args.ups_watch_freshness_ms.to_string(),
        "--include-meta".to_string(),
    ]);
    if samples > 0 {
        cmd.extend(["--samples".to_string(), samples.to_string()]);
    }
    cmd
}

fn ups_artifact_select_command(
    args: &RunArgs,
    context: &PowerValidationArgs,
    profile: OutputProfile,
) -> anyhow::Result<Vec<String>> {
    let manifest = artifact_manifest_for(args, profile)
        .unwrap_or_else(|_| PathBuf::from(format!("<required-{}-manifest.json>", profile.key())));
    let mut cmd = ups_command(&args.bench, context, ["artifact", "select"]);
    cmd.extend([
        "--manifest-path".to_string(),
        manifest.to_string_lossy().to_string(),
    ]);
    Ok(cmd)
}

fn ups_flash_command(
    args: &RunArgs,
    context: &PowerValidationArgs,
    _profile: OutputProfile,
) -> anyhow::Result<Vec<String>> {
    let mut cmd = ups_command(&args.bench, context, ["flash"]);
    cmd.push("--real".to_string());
    Ok(cmd)
}

fn artifact_manifest_for(args: &RunArgs, profile: OutputProfile) -> anyhow::Result<PathBuf> {
    let manifest = match profile {
        OutputProfile::V12 => args.artifact_manifest_12v.as_ref(),
        OutputProfile::V19 => args.artifact_manifest_19v.as_ref(),
    };
    manifest
        .cloned()
        .ok_or_else(|| anyhow!("missing artifact manifest for {} profile", profile.key()))
}

fn power_capabilities_command(args: &BenchArgs) -> anyhow::Result<Vec<String>> {
    match args.power_adapter {
        PowerAdapterKind::Isolapurr => {
            let mut cmd = isolapurr_base(args);
            cmd.extend(["power".to_string(), "show".to_string()]);
            append_isolapurr_selector(&mut cmd, args);
            Ok(cmd)
        }
        PowerAdapterKind::External => external_adapter_command(
            args.power_adapter_cmd.as_ref(),
            AdapterRole::PowerSource,
            "capabilities",
            &[],
        ),
    }
}

fn power_config_show_command(args: &BenchArgs) -> anyhow::Result<Vec<String>> {
    match args.power_adapter {
        PowerAdapterKind::Isolapurr => {
            let mut cmd = isolapurr_base(args);
            cmd.extend([
                "power".to_string(),
                "config".to_string(),
                "show".to_string(),
            ]);
            append_isolapurr_selector(&mut cmd, args);
            Ok(cmd)
        }
        PowerAdapterKind::External => Ok(Vec::new()),
    }
}

fn power_disable_command(args: &BenchArgs) -> anyhow::Result<Vec<String>> {
    match args.power_adapter {
        PowerAdapterKind::Isolapurr => isolapurr_runtime_output_command(args, false),
        PowerAdapterKind::External => external_adapter_command(
            args.power_adapter_cmd.as_ref(),
            AdapterRole::PowerSource,
            "disable",
            &[],
        ),
    }
}

fn power_configure_off_command(
    args: &BenchArgs,
    profile: OutputProfile,
) -> anyhow::Result<Vec<String>> {
    match args.power_adapter {
        PowerAdapterKind::Isolapurr => {
            let mut cmd = isolapurr_base(args);
            cmd.extend(["power".to_string(), "config".to_string(), "set".to_string()]);
            append_isolapurr_selector(&mut cmd, args);
            cmd.extend([
                "--tps-mode".to_string(),
                "manual".to_string(),
                "--voltage-mv".to_string(),
                profile.source_voltage_mv().to_string(),
                "--current-limit-ma".to_string(),
                profile.source_current_limit_ma().to_string(),
                "--usb-c-path".to_string(),
                "disconnected".to_string(),
            ]);
            Ok(cmd)
        }
        PowerAdapterKind::External => external_adapter_command(
            args.power_adapter_cmd.as_ref(),
            AdapterRole::PowerSource,
            "configure",
            &[
                ("--voltage-mv", profile.source_voltage_mv().to_string()),
                (
                    "--current-limit-ma",
                    profile.source_current_limit_ma().to_string(),
                ),
                ("--enabled", "false".to_string()),
            ],
        ),
    }
}

fn power_enable_command(args: &BenchArgs, profile: OutputProfile) -> anyhow::Result<Vec<String>> {
    match args.power_adapter {
        PowerAdapterKind::Isolapurr => {
            let _ = profile;
            isolapurr_runtime_output_command(args, true)
        }
        PowerAdapterKind::External => external_adapter_command(
            args.power_adapter_cmd.as_ref(),
            AdapterRole::PowerSource,
            "enable",
            &[
                ("--voltage-mv", profile.source_voltage_mv().to_string()),
                (
                    "--current-limit-ma",
                    profile.source_current_limit_ma().to_string(),
                ),
            ],
        ),
    }
}

fn power_port_enable_command(
    args: &BenchArgs,
    profile: OutputProfile,
) -> anyhow::Result<Vec<String>> {
    match args.power_adapter {
        PowerAdapterKind::Isolapurr => power_enable_command(args, profile),
        PowerAdapterKind::External => external_adapter_command(
            args.power_adapter_cmd.as_ref(),
            AdapterRole::PowerSource,
            "enable",
            &[
                ("--voltage-mv", profile.source_voltage_mv().to_string()),
                (
                    "--current-limit-ma",
                    profile.source_current_limit_ma().to_string(),
                ),
            ],
        ),
    }
}

fn power_stream_command(args: &BenchArgs) -> anyhow::Result<Vec<String>> {
    match args.power_adapter {
        PowerAdapterKind::Isolapurr => {
            let mut cmd = isolapurr_base(args);
            cmd.extend([
                "power".to_string(),
                "config".to_string(),
                "show".to_string(),
            ]);
            append_isolapurr_selector(&mut cmd, args);
            Ok(cmd)
        }
        PowerAdapterKind::External => external_adapter_command(
            args.power_adapter_cmd.as_ref(),
            AdapterRole::PowerSource,
            "stream",
            &[("--interval-ms", args.sample_interval_ms.to_string())],
        ),
    }
}

async fn read_isolapurr_tps_cdc_rise_mv(
    args: &BenchArgs,
    actions: &mut Vec<Value>,
    action_name: &str,
) -> anyhow::Result<Option<Value>> {
    if args.power_adapter != PowerAdapterKind::Isolapurr {
        return Ok(None);
    }
    for attempt in 0..2 {
        match run_cmd_json(power_config_show_command(args)?, action_name, actions).await {
            Ok(config) => {
                return config
                    .pointer("/manual/tps_cdc_rise_mv")
                    .cloned()
                    .map(Some)
                    .ok_or_else(|| {
                        anyhow!("IsolaPurr power config does not expose manual.tps_cdc_rise_mv")
                    });
            }
            Err(error) if attempt == 0 => {
                actions.push(json!({
                    "power_tps_cdc_rise_read_retry": {
                        "action": action_name,
                        "delay_ms": 500,
                        "error": error.to_string(),
                    }
                }));
                sleep(Duration::from_millis(500)).await;
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("two-attempt IsolaPurr config read loop always returns")
}

fn ensure_isolapurr_tps_cdc_rise_preserved(
    before: Option<&Value>,
    after: Option<&Value>,
    actions: &mut Vec<Value>,
) -> anyhow::Result<()> {
    let preserved = before.is_some() && before == after;
    actions.push(json!({
        "power_tps_cdc_rise_guard": {
            "before_mv": before,
            "after_mv": after,
            "preserved": preserved,
        }
    }));
    if preserved {
        Ok(())
    } else {
        bail!("IsolaPurr manual.tps_cdc_rise_mv changed or disappeared during power configuration")
    }
}

fn isolapurr_runtime_output_command(
    args: &BenchArgs,
    enabled: bool,
) -> anyhow::Result<Vec<String>> {
    let mut cmd = isolapurr_base(args);
    cmd.extend([
        "power".to_string(),
        "runtime".to_string(),
        "output".to_string(),
    ]);
    append_isolapurr_selector(&mut cmd, args);
    cmd.extend(["--enabled".to_string(), enabled.to_string()]);
    Ok(cmd)
}

fn isolapurr_base(args: &BenchArgs) -> Vec<String> {
    let mut cmd = vec![args.isolapurr_cli.to_string_lossy().to_string()];
    if let Some(ipc) = &args.isolapurr_ipc {
        cmd.push("--ipc".to_string());
        cmd.push(ipc.clone());
    }
    cmd.push("--json".to_string());
    cmd
}

fn append_isolapurr_selector(cmd: &mut Vec<String>, args: &BenchArgs) {
    if let Some(url) = &args.isolapurr_url {
        cmd.push("--url".to_string());
        cmd.push(url.clone());
    } else {
        cmd.push("--device-id".to_string());
        cmd.push(args.power_device.clone());
    }
}

fn load_capabilities_command(args: &BenchArgs) -> anyhow::Result<Vec<String>> {
    match args.load_adapter {
        LoadAdapterKind::Loadlynx => load_cli_base(args).map(|mut cmd| {
            cmd.push("--help".to_string());
            cmd
        }),
        LoadAdapterKind::External => external_adapter_command(
            args.load_adapter_cmd.as_ref(),
            AdapterRole::ElectronicLoad,
            "capabilities",
            &[],
        ),
    }
}

fn load_disable_command(args: &BenchArgs) -> anyhow::Result<Vec<String>> {
    match args.load_adapter {
        LoadAdapterKind::Loadlynx => load_cli_base(args).map(|mut cmd| {
            cmd.extend([
                "--json".to_string(),
                "control".to_string(),
                "set".to_string(),
                "--device".to_string(),
                args.load_device.clone(),
                "--disable".to_string(),
            ]);
            cmd
        }),
        LoadAdapterKind::External => external_adapter_command(
            args.load_adapter_cmd.as_ref(),
            AdapterRole::ElectronicLoad,
            "disable",
            &[],
        ),
    }
}

fn load_cc_command(
    args: &BenchArgs,
    target_ma: u32,
    protection: LoadProtection,
) -> anyhow::Result<Vec<String>> {
    match args.load_adapter {
        LoadAdapterKind::Loadlynx => load_cli_base(args).map(|mut cmd| {
            cmd.extend([
                "--json".to_string(),
                "cc".to_string(),
                target_ma.to_string(),
                "--device".to_string(),
                args.load_device.clone(),
                "--min-v-mv".to_string(),
                protection.load_min_v_mv.to_string(),
                "--max-i-ma-total".to_string(),
                protection.load_max_i_ma_total.to_string(),
                "--max-p-mw".to_string(),
                protection.load_max_p_mw.to_string(),
            ]);
            cmd
        }),
        LoadAdapterKind::External => external_adapter_command(
            args.load_adapter_cmd.as_ref(),
            AdapterRole::ElectronicLoad,
            "set-load",
            &[
                ("--target-ma", target_ma.to_string()),
                ("--min-v-mv", protection.load_min_v_mv.to_string()),
                (
                    "--max-i-ma-total",
                    protection.load_max_i_ma_total.to_string(),
                ),
                ("--max-p-mw", protection.load_max_p_mw.to_string()),
            ],
        ),
    }
}

fn load_stream_command(args: &BenchArgs, samples: usize) -> anyhow::Result<Vec<String>> {
    match args.load_adapter {
        LoadAdapterKind::Loadlynx => load_cli_base(args).map(|mut cmd| {
            cmd.extend([
                "--json".to_string(),
                "status-stream".to_string(),
                "--device".to_string(),
                args.load_device.clone(),
                "--interval-ms".to_string(),
                args.sample_interval_ms.to_string(),
            ]);
            if samples > 0 {
                cmd.extend(["--count".to_string(), samples.to_string()]);
            }
            cmd
        }),
        LoadAdapterKind::External => {
            let mut extra = vec![("--interval-ms", args.sample_interval_ms.to_string())];
            if samples > 0 {
                extra.push(("--count", samples.to_string()));
            }
            external_adapter_command(
                args.load_adapter_cmd.as_ref(),
                AdapterRole::ElectronicLoad,
                "stream",
                &extra,
            )
        }
    }
}

fn preflight_load_stream_command(args: &BenchArgs, samples: usize) -> anyhow::Result<Vec<String>> {
    // Match the formal collector: it owns termination after receiving the requested
    // number of samples, so LoadLynx never enters its bounded-stream exit path.
    let _ = samples;
    load_stream_command(args, 0)
}

fn load_cli_base(args: &BenchArgs) -> anyhow::Result<Vec<String>> {
    let cli = args
        .load_cli
        .as_ref()
        .ok_or_else(|| anyhow!("LoadLynx adapter requires --load-cli or LOADLYNX_CLI"))?;
    // Current released LoadLynx owns its devd lifecycle and no longer exposes a
    // top-level --ipc option. Keep accepting the legacy runner argument so old
    // bench scripts remain parseable, but never forward it to the child CLI.
    Ok(vec![cli.to_string_lossy().to_string()])
}

#[derive(Debug, Clone, Copy)]
enum AdapterRole {
    PowerSource,
    ElectronicLoad,
}

impl AdapterRole {
    fn key(self) -> &'static str {
        match self {
            Self::PowerSource => "power-source",
            Self::ElectronicLoad => "electronic-load",
        }
    }
}

fn external_adapter_command(
    cmd: Option<&PathBuf>,
    role: AdapterRole,
    action: &str,
    extra: &[(&str, String)],
) -> anyhow::Result<Vec<String>> {
    let cmd = cmd.ok_or_else(|| anyhow!("external adapter requires --*-adapter-cmd"))?;
    let mut out = vec![
        cmd.to_string_lossy().to_string(),
        "--role".to_string(),
        role.key().to_string(),
        "--action".to_string(),
        action.to_string(),
    ];
    for (key, value) in extra {
        out.push((*key).to_string());
        out.push(value.clone());
    }
    Ok(out)
}

async fn run_scene(
    args: &RunArgs,
    context: &PowerValidationArgs,
    suite_dir: &Path,
    profile: OutputProfile,
    scene: SceneKind,
    suite_contract: SuiteContract,
) -> anyhow::Result<Value> {
    let result = run_scene_inner(args, context, suite_dir, profile, scene, suite_contract).await;
    let cleanup = cleanup_scene(args).await;
    match (result, cleanup) {
        (Ok(report), Ok(_)) => Ok(report),
        (Ok(mut report), Err(cleanup_error)) => {
            report["cleanup_error"] = json!(cleanup_error.to_string());
            Ok(report)
        }
        (Err(error), Ok(_)) => Err(error),
        (Err(error), Err(cleanup_error)) => {
            Err(error).with_context(|| format!("post-scene cleanup also failed: {cleanup_error}"))
        }
    }
}

async fn run_scene_inner(
    args: &RunArgs,
    context: &PowerValidationArgs,
    suite_dir: &Path,
    profile: OutputProfile,
    scene: SceneKind,
    suite_contract: SuiteContract,
) -> anyhow::Result<Value> {
    let protection = LoadProtection {
        load_min_v_mv: 3_000,
        load_max_i_ma_total: 4_000,
        load_max_p_mw: 80_000,
    };
    let report_name = format!("{}-{}-{}ma", profile.key(), scene.key(), scene.target_ma());
    let report_dir = suite_dir.join(&report_name);
    fs::create_dir_all(&report_dir)?;
    let mut actions = Vec::<Value>::new();
    let mut samples = Vec::<SceneSample>::new();
    run_cmd_json_retry(
        load_disable_command(&args.bench)?,
        "load_disable_before_scene",
        &mut actions,
        3,
    )
    .await?;
    run_cmd_json_retry(
        power_disable_command(&args.bench)?,
        "power_disable_before_scene",
        &mut actions,
        3,
    )
    .await?;
    ensure_source_disconnected(args, context, &mut actions, "before_scene").await?;
    run_cmd_json_retry(
        ups_command(&args.bench, context, ["monitor", "start"]),
        "ups_monitor_start_before_scene",
        &mut actions,
        3,
    )
    .await?;
    wait_for_ups_status_watch_ready(args, context, "after_profile_switch").await?;
    let (_identity, settings, profile_gate) =
        ensure_profile_ready(args, context, profile, suite_contract, &mut actions).await?;
    actions.push(json!({"ups_profile_gate": profile_gate}));
    let tps_cdc_rise_before = read_isolapurr_tps_cdc_rise_mv(
        &args.bench,
        &mut actions,
        "power_tps_cdc_rise_before_configure",
    )
    .await?;
    run_cmd_json_retry(
        power_configure_off_command(&args.bench, profile)?,
        "power_configure_off",
        &mut actions,
        3,
    )
    .await?;
    let tps_cdc_rise_after = read_isolapurr_tps_cdc_rise_mv(
        &args.bench,
        &mut actions,
        "power_tps_cdc_rise_after_configure",
    )
    .await?;
    ensure_isolapurr_tps_cdc_rise_preserved(
        tps_cdc_rise_before.as_ref(),
        tps_cdc_rise_after.as_ref(),
        &mut actions,
    )?;
    run_cmd_json_retry(
        power_enable_command(&args.bench, profile)?,
        "power_enable",
        &mut actions,
        3,
    )
    .await?;
    run_cmd_json_retry(
        power_port_enable_command(&args.bench, profile)?,
        "power_port_enable",
        &mut actions,
        3,
    )
    .await?;
    wait_for_ups_online_recovery(args, context, &mut actions).await?;

    let mut collectors = start_scene_collectors(args, context).await?;
    let start = Instant::now();
    let started_unix_ms = now_ms();
    collect_for(
        args,
        &collectors,
        &mut samples,
        start,
        started_unix_ms,
        "pre",
        scene.target_ma(),
        args.pre_s,
    )
    .await;
    run_cmd_json_with_sampling(
        load_cc_command(&args.bench, scene.target_ma(), protection)?,
        "load_cc",
        &mut actions,
        args,
        &collectors,
        &mut samples,
        start,
        started_unix_ms,
        "transition_load",
        scene.target_ma(),
    )
    .await?;
    collect_for(
        args,
        &collectors,
        &mut samples,
        start,
        started_unix_ms,
        "hold",
        scene.target_ma(),
        args.hold_s,
    )
    .await;
    let source_limited_before_cut = samples
        .iter()
        .rev()
        .find(|sample| matches!(sample.phase.as_str(), "transition_load" | "hold"))
        .is_some_and(sample_is_source_limited);
    let should_cut_source = scene.include_backup()
        && (!scene.requires_source_limited_before_cut() || source_limited_before_cut);
    if scene.requires_source_limited_before_cut() && !source_limited_before_cut {
        actions.push(json!({
            "power_cut_skipped": "source_limited_not_latched_before_cut",
        }));
    }
    if should_cut_source {
        run_cmd_json_with_sampling_retry(
            power_disable_command(&args.bench)?,
            "power_cut_for_backup",
            &mut actions,
            args,
            &collectors,
            &mut samples,
            start,
            started_unix_ms,
            "transition_backup",
            scene.target_ma(),
            3,
        )
        .await?;
        ensure_source_disconnected_with_sampling(
            args,
            context,
            &mut actions,
            &collectors,
            &mut samples,
            start,
            started_unix_ms,
            "backup_cut",
            "transition_backup",
            scene.target_ma(),
        )
        .await?;
        collect_for(
            args,
            &collectors,
            &mut samples,
            start,
            started_unix_ms,
            "backup",
            scene.target_ma(),
            args.backup_s,
        )
        .await;
        run_cmd_json_with_sampling_retry(
            power_enable_command(&args.bench, profile)?,
            "power_restore",
            &mut actions,
            args,
            &collectors,
            &mut samples,
            start,
            started_unix_ms,
            "transition_restore",
            scene.target_ma(),
            3,
        )
        .await?;
        run_cmd_json_with_sampling_retry(
            power_port_enable_command(&args.bench, profile)?,
            "power_port_enable_restore",
            &mut actions,
            args,
            &collectors,
            &mut samples,
            start,
            started_unix_ms,
            "transition_restore",
            scene.target_ma(),
            3,
        )
        .await?;
        collect_for(
            args,
            &collectors,
            &mut samples,
            start,
            started_unix_ms,
            "restore",
            scene.target_ma(),
            args.restore_s,
        )
        .await;
    }
    run_cmd_json_with_sampling(
        load_disable_command(&args.bench)?,
        "load_disable_after_scene",
        &mut actions,
        args,
        &collectors,
        &mut samples,
        start,
        started_unix_ms,
        "transition_unload",
        scene.target_ma(),
    )
    .await?;
    collect_for(
        args,
        &collectors,
        &mut samples,
        start,
        started_unix_ms,
        "post",
        scene.target_ma(),
        args.post_s,
    )
    .await;
    stop_collectors(&mut collectors).await;
    backfill_scene_samples_from_ups(
        &mut samples,
        &collectors,
        started_unix_ms,
        scene.target_ma(),
        (args.bench.sample_interval_ms as i64 / 2).max(1),
    );
    let collector_diagnostics = collector_diagnostics(&collectors);
    let mut summary = summarize_scene(&samples);
    for (name, diag) in collector_diagnostics.as_object().into_iter().flatten() {
        if diag
            .get("errors")
            .and_then(Value::as_array)
            .is_some_and(|errors| !errors.is_empty())
        {
            summary.failures.push(format!("{name}_collector_error"));
        }
    }
    classify_load_acceptance_phases(scene, &mut samples);
    let scene_assertions = evaluate_scene_assertions(scene, profile, &samples);
    for failure in scene_assertions
        .get("failures")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        summary.failures.push(failure.to_string());
    }
    summary.scene_complete = summary.failures.is_empty();
    let acceptance = json!({
        "signoff_valid": summary.scene_complete,
        "run_validity": if summary.scene_complete { "valid_for_signoff" } else { "invalid_diagnostic_only" },
        "failed_acceptance_checks": summary.failures,
        "required_sample_rate_hz": MIN_FORMAL_SAMPLE_RATE_HZ,
    });
    write_timeseries(&report_dir.join("timeseries.jsonl"), &samples)?;
    let metadata = json!({
        "suite_contract": suite_contract.key(),
        "output_profile": profile.key(),
        "scene_type": scene.key(),
        "target_ma": scene.target_ma(),
        "include_backup": scene.include_backup(),
        "load_min_v_mv": protection.load_min_v_mv,
        "max_i_ma_total": protection.load_max_i_ma_total,
        "max_p_mw": protection.load_max_p_mw,
        "source_voltage_mv": profile.source_voltage_mv(),
        "source_current_limit_ma": profile.source_current_limit_ma(),
        "ups_transport": "cli+ipc+usb",
        "load_transport": load_adapter_label(&args.bench),
        "power_transport": power_adapter_label(&args.bench),
    });
    let advanced_power = settings
        .get("advanced_power")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let results = json!({
        "metadata": metadata,
        "actions": actions,
        "collector_diagnostics": collector_diagnostics,
        "settings_snapshot": settings,
        "source_tps_cdc_rise_mv": {
            "before_configure": tps_cdc_rise_before,
            "after_configure": tps_cdc_rise_after,
        },
        "scene_assertions": scene_assertions,
        "samples": samples,
        "summary": {"all": {"completeness": summary, "acceptance": acceptance}},
    });
    write_json_value(&report_dir.join("results.json"), &results)?;
    let _ = render_scene_chart(args, &report_dir, profile, scene).await;
    Ok(json!({
        "suite_contract": suite_contract.key(),
        "output_profile": profile.key(),
        "scene_type": scene.key(),
        "target_ma": scene.target_ma(),
        "include_backup": scene.include_backup(),
        "load_min_v_mv": protection.load_min_v_mv,
        "load_max_i_ma_total": protection.load_max_i_ma_total,
        "load_max_p_mw": protection.load_max_p_mw,
        "source_voltage_mv": profile.source_voltage_mv(),
        "source_current_limit_ma": profile.source_current_limit_ma(),
        "report_dir": report_name,
        "scene_complete": summary.scene_complete,
        "run_validity": if summary.scene_complete { "valid_for_signoff" } else { "invalid_diagnostic_only" },
        "signoff_valid": summary.scene_complete,
        "failures": summary.failures,
        "failed_acceptance_checks": summary.failures,
        "effective_sample_rate_hz": summary.effective_sample_rate_hz,
        "max_sample_gap_s": summary.max_sample_gap_s,
        "advanced_power": advanced_power,
        "scene_assertions": results.get("scene_assertions").cloned().unwrap_or(Value::Null),
    }))
}

fn sample_string(value: &Option<Value>) -> Option<&str> {
    value.as_ref().and_then(Value::as_str)
}

fn sample_number(value: &Option<Value>) -> Option<i64> {
    value.as_ref().and_then(Value::as_i64)
}

fn sample_backup_reason(sample: &SceneSample) -> Option<&str> {
    sample_string(&sample.backup_reason).or_else(|| sample_string(&sample.diag_backup_reason))
}

fn sample_is_backup(sample: &SceneSample) -> bool {
    sample_string(&sample.mode) == Some("backup")
        && (sample_string(&sample.stage) == Some("backup")
            || sample_string(&sample.diag_stage) == Some("backup"))
}

fn sample_has_backup_signal(sample: &SceneSample) -> bool {
    sample_string(&sample.mode) == Some("backup")
        || sample_string(&sample.stage) == Some("backup")
        || sample_string(&sample.diag_stage) == Some("backup")
}

fn sample_is_source_limited(sample: &SceneSample) -> bool {
    sample_is_backup(sample) && sample_backup_reason(sample) == Some("source_limited")
}

fn sample_mains_present(sample: &SceneSample) -> Option<bool> {
    sample
        .mains_present
        .as_ref()
        .and_then(Value::as_bool)
        .or_else(|| {
            sample
                .diag_backup_reason
                .as_ref()
                .and_then(Value::as_str)
                .map(|reason| reason != "input_absent")
        })
}

fn sample_vin_vbus_mv(sample: &SceneSample) -> Option<i64> {
    sample_number(&sample.vin_vbus_mv)
}

fn sample_tps_output_power_mw(sample: &SceneSample) -> Option<i64> {
    let current_ma = sample_number(&sample.tps_total_iout_ma)?;
    let mut voltages = [None, None];
    voltages[0] = sample_number(&sample.out_a_vbus_mv);
    voltages[1] = sample_number(&sample.out_b_vbus_mv);
    let present = voltages.into_iter().flatten().collect::<Vec<_>>();
    if present.is_empty() {
        return None;
    }
    let average_mv = present.iter().sum::<i64>() / present.len() as i64;
    Some(current_ma.saturating_mul(average_mv) / 1_000)
}

fn classify_load_acceptance_phases(scene: SceneKind, samples: &mut [SceneSample]) {
    classify_backup_transition_phases(scene, samples);
    if !scene.requires_source_limited() {
        return;
    }
    let mut transition_started = false;
    let mut backup_started = false;
    for sample in samples.iter_mut().filter(|sample| is_load_phase(sample)) {
        if sample_is_source_limited(sample) {
            backup_started = true;
        } else if sample_tps_output_power_mw(sample)
            .is_some_and(|power_mw| power_mw > HOLD_TPS_POWER_MAX_MW)
        {
            transition_started = true;
        }
        if backup_started {
            sample.phase = "backup_online".to_string();
        } else if transition_started {
            sample.phase = "transition_source_limited".to_string();
        }
    }
}

fn classify_backup_transition_phases(scene: SceneKind, samples: &mut [SceneSample]) {
    if !scene.include_backup() {
        return;
    }

    let Some(first_transition_idx) = samples
        .iter()
        .position(|sample| sample.phase == "transition_backup")
    else {
        return;
    };

    let hold_like = if first_transition_idx > 0 {
        samples[..first_transition_idx].iter().rev().find(|sample| {
            matches!(
                sample.phase.as_str(),
                "hold" | "transition_load" | "backup_online"
            )
        })
    } else {
        None
    };
    let hold_mains_present = hold_like.and_then(sample_mains_present);
    let hold_vin_vbus_mv = hold_like.and_then(sample_vin_vbus_mv);

    let mut first_effect_idx = None;
    let mut first_backup_idx = None;
    for (idx, sample) in samples.iter().enumerate().skip(first_transition_idx) {
        if sample.phase != "transition_backup" && sample.phase != "backup" {
            continue;
        }
        let mains_changed = hold_mains_present
            .zip(sample_mains_present(sample))
            .is_some_and(|(hold, current)| current != hold);
        let vin_changed = hold_vin_vbus_mv
            .zip(sample_vin_vbus_mv(sample))
            .is_some_and(|(hold, current)| (hold - current).abs() >= 200);
        if first_effect_idx.is_none()
            && (sample_is_backup(sample)
                || sample_backup_reason(sample) == Some("input_absent")
                || mains_changed
                || vin_changed)
        {
            first_effect_idx = Some(idx);
        }
        if first_backup_idx.is_none() && sample_is_backup(sample) {
            first_backup_idx = Some(idx);
        }
    }

    let Some(first_effect_idx) = first_effect_idx else {
        return;
    };
    for sample in &mut samples[first_transition_idx..first_effect_idx] {
        if sample.phase == "transition_backup" {
            sample.phase = "hold".to_string();
        }
    }

    let first_backup_idx = first_backup_idx.unwrap_or(first_effect_idx + 1);
    let backup_start_idx = if first_backup_idx == first_effect_idx {
        first_effect_idx.saturating_add(1)
    } else {
        first_backup_idx
    };
    for sample in samples.iter_mut().skip(backup_start_idx) {
        if sample.phase != "transition_backup" {
            continue;
        }
        sample.phase = "backup".to_string();
    }
}

fn hold_power_assertions(samples: &[SceneSample]) -> (Value, Vec<String>) {
    let hold = samples
        .iter()
        .filter(|sample| sample.phase == "hold")
        .collect::<Vec<_>>();
    let powers = hold
        .iter()
        .filter_map(|sample| sample_tps_output_power_mw(sample))
        .collect::<Vec<_>>();
    let missing = hold.len().saturating_sub(powers.len());
    let over = powers
        .iter()
        .filter(|power_mw| **power_mw > HOLD_TPS_POWER_MAX_MW)
        .count();
    let mut failures = Vec::new();
    if missing > 0 {
        failures.push("hold_tps_power_missing".to_string());
    }
    if over > 0 {
        failures.push("hold_tps_power_over_2w".to_string());
    }
    (
        json!({
            "maximum_mw": powers.iter().max(),
            "over_2w_samples": over,
            "missing_samples": missing,
            "limit_mw": HOLD_TPS_POWER_MAX_MW,
        }),
        failures,
    )
}

fn is_load_phase(sample: &SceneSample) -> bool {
    matches!(
        sample.phase.as_str(),
        "transition_load" | "hold" | "transition_source_limited" | "backup_online"
    )
}

fn sample_load_current_ma(sample: &SceneSample) -> Option<i64> {
    sample_number(&sample.load_i_total_ma)
}

fn sample_load_is_applied(sample: &SceneSample) -> bool {
    let minimum_current_ma = i64::from(sample.load_target_i_ma) * 80 / 100;
    sample.load_output_enabled.as_ref().and_then(Value::as_bool) == Some(true)
        && sample_load_current_ma(sample).is_some_and(|current_ma| current_ma >= minimum_current_ma)
}

fn source_limited_min_load_mv(profile: OutputProfile) -> i64 {
    i64::from(profile.rated_vout_mv()) - SOURCE_LIMITED_LOAD_MARGIN_MV
}

fn longest_low_voltage_duration_s(samples: &[&SceneSample], min_load_mv: i64) -> Option<f64> {
    let mut observed = false;
    let mut low_start = None;
    let mut low_end = None;
    let mut longest = 0.0_f64;

    for sample in samples {
        let Some(voltage_mv) = sample_number(&sample.load_v_local_mv) else {
            continue;
        };
        observed = true;
        if voltage_mv < min_load_mv {
            low_start.get_or_insert(sample.t_s);
            low_end = Some(sample.t_s);
        } else if let (Some(start), Some(end)) = (low_start.take(), low_end.take()) {
            longest = longest.max(end - start);
        }
    }
    if let (Some(start), Some(end)) = (low_start, low_end) {
        longest = longest.max(end - start);
    }
    observed.then_some(round3(longest))
}

fn evaluate_scene_assertions(
    scene: SceneKind,
    profile: OutputProfile,
    samples: &[SceneSample],
) -> Value {
    let (hold_tps_power, hold_power_failures) = hold_power_assertions(samples);
    let mut failures = if scene.requires_source_limited() {
        hold_power_failures
    } else {
        Vec::new()
    };
    let load_samples = samples
        .iter()
        .filter(|sample| is_load_phase(sample))
        .collect::<Vec<_>>();
    let source_limited_entry = load_samples
        .iter()
        .position(|sample| sample_is_source_limited(sample));
    let source_limited = if scene.requires_source_limited() {
        let Some(entry_index) = source_limited_entry else {
            failures.push("source_limited_not_observed".to_string());
            return json!({
                "passed": false,
                "failures": failures,
                "source_limited": {
                    "observed": false,
                    "entry_delay_s": Value::Null,
                    "post_latch_min_load_mv": Value::Null,
                    "pre_latch_low_voltage_max_duration_s": Value::Null,
                    "post_latch_low_voltage_max_duration_s": Value::Null,
                },
            });
        };
        let entry = load_samples[entry_index];
        let load_start = load_samples
            .iter()
            .find(|sample| sample_load_is_applied(sample))
            .map(|sample| sample.t_s)
            .or_else(|| load_samples.first().map(|sample| sample.t_s))
            .unwrap_or(entry.t_s);
        let entry_delay_s = round3(entry.t_s - load_start);
        if entry_delay_s > SOURCE_LIMITED_ENTRY_MAX_S {
            failures.push("source_limited_entry_after_2s".to_string());
        }
        // UPS status and load telemetry are collected independently. The decision
        // sample can still contain the load reading taken before target application.
        let pre_latch = load_samples[..=entry_index].to_vec();
        let post_latch = load_samples[(entry_index + 1)..].to_vec();
        let post_latch_min_load_mv = post_latch
            .iter()
            .filter_map(|sample| sample_number(&sample.load_v_local_mv))
            .min();
        let min_load_mv = source_limited_min_load_mv(profile);
        if post_latch_min_load_mv.is_none() {
            failures.push("source_limited_post_latch_load_voltage_missing".to_string());
        } else if post_latch_min_load_mv < Some(min_load_mv) {
            failures.push("source_limited_post_latch_load_below_minimum".to_string());
        }
        let post_latch_low_voltage_max_duration_s =
            longest_low_voltage_duration_s(&post_latch, min_load_mv);
        if post_latch_low_voltage_max_duration_s
            .is_some_and(|duration| duration > SOURCE_LIMITED_MAX_LOW_VOLTAGE_S)
        {
            failures.push("source_limited_post_latch_low_voltage_over_1s".to_string());
        }
        let pre_latch_low_voltage_max_duration_s =
            longest_low_voltage_duration_s(&pre_latch, min_load_mv);
        if pre_latch_low_voltage_max_duration_s
            .is_some_and(|duration| duration > SOURCE_LIMITED_MAX_LOW_VOLTAGE_S)
        {
            failures.push("source_limited_pre_latch_low_voltage_over_1s".to_string());
        }
        let target_observed = post_latch.iter().any(|sample| {
            sample_number(&sample.assist_target_vout_mv)
                .or_else(|| sample_number(&sample.diag_assist_target_vout_mv))
                == Some(profile.rated_vout_mv() as i64)
        });
        if !target_observed {
            failures.push("source_limited_target_not_rated".to_string());
        }
        json!({
            "observed": true,
            "entry_t_s": entry.t_s,
            "entry_delay_s": entry_delay_s,
            "target_rated_observed": target_observed,
            "minimum_load_mv": min_load_mv,
            "post_latch_min_load_mv": post_latch_min_load_mv,
            "pre_latch_low_voltage_max_duration_s": pre_latch_low_voltage_max_duration_s,
            "post_latch_low_voltage_max_duration_s": post_latch_low_voltage_max_duration_s,
        })
    } else {
        Value::Null
    };

    let in_budget_guard = if scene.requires_non_backup_online() {
        let applied_samples = load_samples
            .iter()
            .copied()
            .filter(|sample| sample_load_is_applied(sample))
            .collect::<Vec<_>>();
        let backup_samples = applied_samples
            .iter()
            .filter(|sample| sample_has_backup_signal(sample))
            .count();
        let source_limited_samples = applied_samples
            .iter()
            .filter(|sample| sample_backup_reason(sample) == Some("source_limited"))
            .count();
        let backup_reason_samples = applied_samples
            .iter()
            .filter(|sample| sample_backup_reason(sample).is_some())
            .count();
        let offline_samples = applied_samples
            .iter()
            .filter(|sample| sample_mains_present(sample) != Some(true))
            .count();
        if applied_samples.is_empty() {
            failures.push("source_in_budget_load_not_observed".to_string());
        }
        if backup_samples > 0 || backup_reason_samples > 0 {
            failures.push("source_in_budget_entered_backup".to_string());
        }
        if offline_samples > 0 {
            failures.push("source_in_budget_mains_not_continuously_online".to_string());
        }
        json!({
            "target_ma": scene.target_ma(),
            "applied_samples": applied_samples.len(),
            "backup_samples": backup_samples,
            "source_limited_samples": source_limited_samples,
            "backup_reason_samples": backup_reason_samples,
            "offline_samples": offline_samples,
        })
    } else {
        Value::Null
    };

    let backup_cut = if scene.requires_input_absent_after_cut() {
        let backup_samples = samples
            .iter()
            .filter(|sample| matches!(sample.phase.as_str(), "transition_backup" | "backup"))
            .collect::<Vec<_>>();
        let input_absent_observed = backup_samples.iter().any(|sample| {
            sample_is_backup(sample) && sample_backup_reason(sample) == Some("input_absent")
        });
        if !input_absent_observed {
            failures.push("input_absent_not_observed_after_cut".to_string());
        }
        let stable_backup_samples = backup_samples
            .iter()
            .filter(|sample| sample.phase == "backup")
            .collect::<Vec<_>>();
        let backup_continuous = !stable_backup_samples.is_empty()
            && stable_backup_samples
                .iter()
                .all(|sample| sample_is_backup(sample));
        if !backup_continuous {
            failures.push("backup_not_continuous_after_cut".to_string());
        }
        json!({
            "input_absent_observed": input_absent_observed,
            "backup_continuous": backup_continuous,
        })
    } else {
        Value::Null
    };

    json!({
        "passed": failures.is_empty(),
        "failures": failures,
        "hold_tps_power": hold_tps_power,
        "transition_source_limited_started_at_s": samples.iter().find(|sample| sample.phase == "transition_source_limited").map(|sample| sample.t_s),
        "backup_online_started_at_s": samples.iter().find(|sample| sample.phase == "backup_online").map(|sample| sample.t_s),
        "in_budget_guard": in_budget_guard,
        "source_limited": source_limited,
        "backup_cut": backup_cut,
    })
}

async fn ensure_source_disconnected(
    args: &RunArgs,
    context: &PowerValidationArgs,
    actions: &mut Vec<Value>,
    label: &str,
) -> anyhow::Result<()> {
    let mut last = SourceDisconnectState::default();
    for attempt in 0..SOURCE_DISCONNECT_CONFIRM_ATTEMPTS {
        let suffix = if attempt == 0 {
            String::new()
        } else {
            format!("_retry_{attempt}")
        };
        let power = run_cmd_json(
            power_capabilities_command(&args.bench)?,
            &format!("{label}_power_readback{suffix}"),
            actions,
        )
        .await?;
        let status = run_cmd_json(
            ups_status_fresh_command(&args.bench, context),
            &format!("{label}_ups_status{suffix}"),
            actions,
        )
        .await?;
        last = source_disconnect_state(&power, &status);
        if !last.ups_still_live {
            return Ok(());
        }
        sleep(Duration::from_millis(SOURCE_DISCONNECT_CONFIRM_INTERVAL_MS)).await;
    }
    bail!(
        "source disconnect gate failed for {label}: UPS still sees DCIN after {} attempts; source_mv={:?} mains_present={:?} source={:?} input_vbus_mv={:?} pre_tps_vin_mv={:?} input_gate_state={:?} input_gate_reason={:?}",
        SOURCE_DISCONNECT_CONFIRM_ATTEMPTS,
        last.source_mv,
        last.mains_present,
        last.input_source,
        last.input_vbus_mv,
        last.vin_vbus_mv,
        last.input_gate_state,
        last.input_gate_reason
    );
}

#[allow(clippy::too_many_arguments)]
async fn ensure_source_disconnected_with_sampling(
    _args: &RunArgs,
    _context: &PowerValidationArgs,
    actions: &mut Vec<Value>,
    collectors: &BTreeMap<String, JsonlProcessCollector>,
    samples: &mut Vec<SceneSample>,
    start: Instant,
    started_unix_ms: i64,
    label: &str,
    phase: &str,
    target_ma: u32,
) -> anyhow::Result<()> {
    let mut last = SourceDisconnectState::default();
    for attempt in 0..SOURCE_DISCONNECT_CONFIRM_ATTEMPTS {
        let sample = collect_scene_sample(collectors, start, started_unix_ms, phase, target_ma);
        let power = collectors
            .get("power")
            .and_then(|collector| collector.latest_before(sample.unix_ms))
            .map(|value| unwrap_cli_result(Some(value)))
            .unwrap_or_else(|| json!({}));
        let status = collectors
            .get("ups_status")
            .and_then(|collector| collector.latest_before(sample.unix_ms))
            .map(|value| unwrap_cli_result(Some(value)))
            .unwrap_or_else(|| json!({}));
        if sample
            .ups_status_cache_fresh
            .as_ref()
            .and_then(Value::as_bool)
            != Some(false)
        {
            samples.push(sample);
        }
        let suffix = if attempt == 0 {
            String::new()
        } else {
            format!("_retry_{attempt}")
        };
        last = source_disconnect_state(&power, &status);
        actions.push(json!({
            format!("{label}_collector_gate{suffix}"): {
                "source_mv": last.source_mv,
                "mains_present": last.mains_present,
                "source": last.input_source,
                "input_vbus_mv": last.input_vbus_mv,
                "pre_tps_vin_mv": last.vin_vbus_mv,
                "input_gate_state": last.input_gate_state,
                "input_gate_reason": last.input_gate_reason,
                "ups_still_live": last.ups_still_live,
            }
        }));
        if !last.ups_still_live {
            return Ok(());
        }
        sleep(Duration::from_millis(SOURCE_DISCONNECT_CONFIRM_INTERVAL_MS)).await;
    }
    bail!(
        "source disconnect gate failed for {label}: UPS still sees DCIN after {} attempts; source_mv={:?} mains_present={:?} source={:?} input_vbus_mv={:?} pre_tps_vin_mv={:?} input_gate_state={:?} input_gate_reason={:?}",
        SOURCE_DISCONNECT_CONFIRM_ATTEMPTS,
        last.source_mv,
        last.mains_present,
        last.input_source,
        last.input_vbus_mv,
        last.vin_vbus_mv,
        last.input_gate_state,
        last.input_gate_reason
    );
}

#[derive(Default)]
struct SourceDisconnectState {
    source_mv: Option<i64>,
    mains_present: Option<bool>,
    input_source: Option<String>,
    input_vbus_mv: Option<i64>,
    vin_vbus_mv: Option<i64>,
    input_gate_state: Option<String>,
    input_gate_reason: Option<String>,
    ups_still_live: bool,
}

const UPS_INPUT_CUT_MAX_VIN_MV: i64 = 2_999;

fn source_disconnect_state(power: &Value, status: &Value) -> SourceDisconnectState {
    let source_mv = source_live_voltage_mv(power);
    let mains_present = status
        .pointer("/sample/input/mains_present")
        .or_else(|| status.pointer("/input/mains_present"))
        .and_then(Value::as_bool);
    let input_source = status
        .pointer("/sample/input/source")
        .or_else(|| status.pointer("/input/source"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let input_vbus_mv = status
        .pointer("/sample/input/input_vbus_mv")
        .or_else(|| status.pointer("/input/input_vbus_mv"))
        .and_then(Value::as_i64);
    let vin_vbus_mv = status
        .pointer("/sample/input/pre_tps_vin_mv")
        .or_else(|| status.pointer("/input/pre_tps_vin_mv"))
        .or_else(|| status.pointer("/sample/input/vin_vbus_mv"))
        .or_else(|| status.pointer("/input/vin_vbus_mv"))
        .and_then(Value::as_i64);
    let input_gate_state = status
        .pointer("/sample/input/input_gate_state")
        .or_else(|| status.pointer("/input/input_gate_state"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let input_gate_reason = status
        .pointer("/sample/input/input_gate_reason")
        .or_else(|| status.pointer("/input/input_gate_reason"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let mode = status
        .pointer("/sample/mode")
        .or_else(|| status.pointer("/mode"))
        .and_then(Value::as_str);
    let assist_power_stage = status
        .pointer("/sample/input/assist_power_stage")
        .or_else(|| status.pointer("/input/assist_power_stage"))
        .and_then(Value::as_str);
    let backup_truth = mode == Some("backup") || assist_power_stage == Some("backup");
    let firmware_gate_cutoff = input_gate_state.as_deref() == Some("cutoff")
        && input_gate_reason.as_deref() == Some("pre_tps_undervoltage");
    let dcin_cut_truth = mains_present == Some(false)
        && (firmware_gate_cutoff || vin_vbus_mv.is_some_and(|mv| mv <= UPS_INPUT_CUT_MAX_VIN_MV));
    let ups_still_live = !(dcin_cut_truth && backup_truth);
    SourceDisconnectState {
        source_mv,
        mains_present,
        input_source,
        input_vbus_mv,
        vin_vbus_mv,
        input_gate_state,
        input_gate_reason,
        ups_still_live,
    }
}

async fn wait_for_ups_online_recovery(
    args: &RunArgs,
    context: &PowerValidationArgs,
    actions: &mut Vec<Value>,
) -> anyhow::Result<()> {
    let mut last_status = Value::Null;
    for attempt in 0..UPS_ONLINE_RECOVER_ATTEMPTS {
        let name = format!("ups_online_recovery_{attempt}");
        let cmd = ups_status_fresh_command(&args.bench, context);
        let status = match run_cmd_output(cmd.clone()).await {
            Ok(status) => {
                actions.push(json!({name: {"cmd": cmd, "result": status}}));
                status
            }
            Err(error) => {
                actions.push(json!({name: {"cmd": cmd, "error": error.to_string()}}));
                sleep(Duration::from_millis(UPS_ONLINE_RECOVER_INTERVAL_MS)).await;
                continue;
            }
        };
        let mains_present = status
            .pointer("/sample/input/mains_present")
            .or_else(|| status.pointer("/input/mains_present"))
            .and_then(Value::as_bool);
        let mode = status
            .pointer("/sample/mode")
            .or_else(|| status.pointer("/mode"))
            .and_then(Value::as_str);
        let stage = status
            .pointer("/sample/input/assist_power_stage")
            .or_else(|| status.pointer("/input/assist_power_stage"))
            .and_then(Value::as_str);
        if mains_present == Some(true) && mode != Some("backup") && stage != Some("backup") {
            return Ok(());
        }
        last_status = status;
        sleep(Duration::from_millis(UPS_ONLINE_RECOVER_INTERVAL_MS)).await;
    }
    bail!(
        "UPS did not recover to online standby before scene within {} ms: {last_status}",
        UPS_ONLINE_RECOVER_ATTEMPTS * UPS_ONLINE_RECOVER_INTERVAL_MS as usize
    );
}

fn ups_status_fresh_command(args: &BenchArgs, context: &PowerValidationArgs) -> Vec<String> {
    ups_command(args, context, ["status", "--fresh", "--include-meta"])
}

fn source_live_voltage_mv(power: &Value) -> Option<i64> {
    power
        .pointer("/diagnostics/usb_c_actual/voltage_mv")
        .and_then(Value::as_i64)
        .or_else(|| {
            power
                .pointer("/ports/ports")
                .and_then(Value::as_array)
                .and_then(|ports| {
                    ports
                        .iter()
                        .find(|port| port.get("portId").and_then(Value::as_str) == Some("port_c"))
                        .and_then(|port| port.pointer("/telemetry/voltage_mv"))
                        .and_then(Value::as_i64)
                })
        })
        .or_else(|| {
            power
                .pointer("/payload/ports")
                .and_then(Value::as_array)
                .and_then(|ports| {
                    ports
                        .iter()
                        .find(|port| port.get("portId").and_then(Value::as_str) == Some("port_c"))
                        .and_then(|port| port.pointer("/telemetry/voltage_mv"))
                        .and_then(Value::as_i64)
                })
        })
}

async fn ensure_profile_ready(
    args: &RunArgs,
    context: &PowerValidationArgs,
    profile: OutputProfile,
    suite_contract: SuiteContract,
    actions: &mut Vec<Value>,
) -> anyhow::Result<(Value, Value, Value)> {
    let uvlo_expectation = SourceLimitedUvloExpectation::from_run_args(profile, args);
    let mut identity = run_cmd_json_retry(
        ups_command(&args.bench, context, ["identity"]),
        "ups_identity",
        actions,
        3,
    )
    .await?;
    let mut settings = run_cmd_json_retry(
        ups_command(&args.bench, context, ["settings"]),
        "ups_settings",
        actions,
        3,
    )
    .await?;
    let mut profile_gate = validate_profile_gate(profile, &identity, &settings);
    if profile_gate.get("ok").and_then(Value::as_bool) == Some(true) {
        validate_suite_settings(
            profile,
            suite_contract,
            &settings,
            &mut profile_gate,
            uvlo_expectation,
        )?;
        return Ok((identity, settings, profile_gate));
    }
    if !args.allow_profile_flash {
        bail!(
            "UPS profile gate failed before source enable and --allow-profile-flash was not set: {profile_gate}"
        );
    }
    let manifest = artifact_manifest_for(args, profile)?;
    if !manifest.is_file() {
        bail!(
            "artifact manifest for {} profile does not exist: {}",
            profile.key(),
            manifest.display()
        );
    }
    run_cmd_json(
        load_disable_command(&args.bench)?,
        "profile_switch_load_disable",
        actions,
    )
    .await?;
    run_cmd_json(
        power_disable_command(&args.bench)?,
        "profile_switch_power_disable",
        actions,
    )
    .await?;
    ensure_source_disconnected(args, context, actions, "profile_switch").await?;
    run_cmd_json_retry(
        ups_artifact_select_command(args, context, profile)?,
        "ups_artifact_select",
        actions,
        3,
    )
    .await?;
    run_cmd_json_retry(
        ups_flash_command(args, context, profile)?,
        "ups_flash",
        actions,
        2,
    )
    .await?;
    sleep(Duration::from_secs(args.profile_flash_settle_s)).await;
    run_cmd_json_retry(
        ups_command(&args.bench, context, ["connect"]),
        "ups_connect_after_profile_switch",
        actions,
        3,
    )
    .await?;
    run_cmd_json_retry(
        ups_command(&args.bench, context, ["monitor", "start"]),
        "ups_monitor_start_after_profile_switch",
        actions,
        3,
    )
    .await?;
    sleep(Duration::from_secs(2)).await;
    identity = run_cmd_json_retry(
        ups_command(&args.bench, context, ["identity"]),
        "ups_identity_after_profile_switch",
        actions,
        5,
    )
    .await?;
    settings = run_cmd_json_retry(
        ups_command(&args.bench, context, ["settings"]),
        "ups_settings_after_profile_switch",
        actions,
        5,
    )
    .await?;
    profile_gate = validate_profile_gate(profile, &identity, &settings);
    if profile_gate.get("ok").and_then(Value::as_bool) != Some(true) {
        bail!("UPS profile gate still failed after profile switch: {profile_gate}");
    }
    validate_suite_settings(
        profile,
        suite_contract,
        &settings,
        &mut profile_gate,
        uvlo_expectation,
    )?;
    Ok((identity, settings, profile_gate))
}

async fn cleanup_scene(args: &RunArgs) -> anyhow::Result<()> {
    let mut errors = Vec::new();
    if let Err(error) = run_cmd_output_retry(load_disable_command(&args.bench)?, 3).await {
        errors.push(format!("load_disable: {error}"));
    }
    if let Err(error) = run_cmd_output_retry(power_disable_command(&args.bench)?, 3).await {
        errors.push(format!("power_disable: {error}"));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        bail!(errors.join("; "))
    }
}

async fn start_scene_collectors(
    args: &RunArgs,
    context: &PowerValidationArgs,
) -> anyhow::Result<BTreeMap<String, JsonlProcessCollector>> {
    wait_for_ups_status_watch_ready(args, context, "before_scene").await?;
    let mut collectors = BTreeMap::new();
    collectors.insert(
        "ups_status".to_string(),
        JsonlProcessCollector::spawn(
            "ups_status",
            ups_watch_command(&args.bench, context, "status", 0),
            JsonFrameMode::Raw,
        )
        .await?,
    );
    collectors.insert("load".to_string(), spawn_load_collector(args).await?);
    wait_for_ups_status_watch_ready(args, context, "collector_warmup").await?;
    Ok(collectors)
}

async fn wait_for_ups_status_watch_ready(
    args: &RunArgs,
    context: &PowerValidationArgs,
    label: &str,
) -> anyhow::Result<ProbeSummary> {
    wait_for_ups_watch_ready(&args.bench, context, label).await
}

async fn wait_for_ups_watch_ready(
    bench: &BenchArgs,
    context: &PowerValidationArgs,
    label: &str,
) -> anyhow::Result<ProbeSummary> {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(20);
    let mut last_probe = None;
    while Instant::now() < deadline {
        let probe = run_json_probe(ups_watch_command(bench, context, "status", 8), 8)
            .await
            .with_context(|| format!("probing UPS status watch {label}"))?;
        if probe.ok {
            return Ok(probe);
        }
        last_probe = Some(probe);
        sleep(Duration::from_millis(bench.sample_interval_ms)).await;
    }
    bail!(
        "UPS status watch did not become ready for {label} within {:?}: {:?}",
        started.elapsed(),
        last_probe
    );
}

async fn spawn_load_collector(args: &RunArgs) -> anyhow::Result<JsonlProcessCollector> {
    JsonlProcessCollector::spawn(
        "load",
        load_stream_command(&args.bench, 0)?,
        JsonFrameMode::Raw,
    )
    .await
}

#[derive(Debug, Clone, Copy)]
enum JsonFrameMode {
    Raw,
}

impl JsonlProcessCollector {
    async fn spawn(name: &str, cmd: Vec<String>, _mode: JsonFrameMode) -> anyhow::Result<Self> {
        let mut child = Command::new(&cmd[0])
            .args(&cmd[1..])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("spawning {name}: {:?}", cmd))?;
        let stdout = child.stdout.take().context("missing stdout")?;
        let stderr = child.stderr.take().context("missing stderr")?;
        let rows = Arc::new(Mutex::new(Vec::new()));
        let errors = Arc::new(Mutex::new(Vec::new()));
        spawn_stdout_reader(name.to_string(), stdout, rows.clone(), errors.clone());
        spawn_stderr_reader(name.to_string(), stderr, errors.clone());
        Ok(Self {
            name: name.to_string(),
            cmd,
            rows,
            errors,
            child,
            stop_flag: None,
        })
    }

    fn latest_before(&self, unix_ms: i64) -> Option<Value> {
        let rows = self.rows.lock().ok()?;
        rows.iter()
            .filter(|row| row.received_ms <= unix_ms && json_frame_has_telemetry(&row.value))
            .last()
            .or_else(|| rows.iter().filter(|row| row.received_ms <= unix_ms).last())
            .map(|row| row.value.clone())
    }

    async fn stop(&mut self) {
        if let Some(stop_flag) = &self.stop_flag {
            stop_flag.store(true, Ordering::Relaxed);
        }
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }
}

fn spawn_stdout_reader(
    name: String,
    stdout: impl tokio::io::AsyncRead + Unpin + Send + 'static,
    rows: Arc<Mutex<Vec<RawFrame>>>,
    errors: Arc<Mutex<Vec<String>>>,
) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Value>(&line) {
                Ok(value) => {
                    let _ = rows.lock().map(|mut rows| {
                        rows.push(RawFrame {
                            received_ms: now_ms(),
                            value,
                        })
                    });
                }
                Err(error) => {
                    let _ = errors
                        .lock()
                        .map(|mut errors| errors.push(format!("{name}: {error}: {line}")));
                }
            }
        }
    });
}

fn spawn_stderr_reader(
    name: String,
    stderr: impl tokio::io::AsyncRead + Unpin + Send + 'static,
    errors: Arc<Mutex<Vec<String>>>,
) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            let _ = errors
                .lock()
                .map(|mut errors| errors.push(format!("{name} stderr: {line}")));
        }
    });
}

async fn stop_collectors(collectors: &mut BTreeMap<String, JsonlProcessCollector>) {
    for collector in collectors.values_mut() {
        collector.stop().await;
    }
}

fn collector_diagnostics(collectors: &BTreeMap<String, JsonlProcessCollector>) -> Value {
    let mut out = serde_json::Map::new();
    for (key, collector) in collectors {
        let rows = collector.rows.lock().map(|rows| rows.len()).unwrap_or(0);
        let errors = collector
            .errors
            .lock()
            .map(|errors| errors.clone())
            .unwrap_or_else(|_| vec!["collector_errors_lock_poisoned".to_string()]);
        out.insert(
            key.clone(),
            json!({
                "name": collector.name,
                "cmd": collector.cmd,
                "rows": rows,
                "errors": errors,
            }),
        );
    }
    Value::Object(out)
}

async fn collect_for(
    args: &RunArgs,
    collectors: &BTreeMap<String, JsonlProcessCollector>,
    samples: &mut Vec<SceneSample>,
    start: Instant,
    started_unix_ms: i64,
    phase: &str,
    target_ma: u32,
    duration_s: f64,
) {
    let deadline = Instant::now() + Duration::from_secs_f64(duration_s);
    let mut ticker = tokio::time::interval(Duration::from_millis(args.bench.sample_interval_ms));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Burst);
    while Instant::now() < deadline {
        ticker.tick().await;
        if Instant::now() >= deadline {
            break;
        }
        push_scene_sample_if_fresh(
            samples,
            collectors,
            start,
            started_unix_ms,
            phase,
            target_ma,
        );
    }
}

fn push_scene_sample_if_fresh(
    samples: &mut Vec<SceneSample>,
    collectors: &BTreeMap<String, JsonlProcessCollector>,
    start: Instant,
    started_unix_ms: i64,
    phase: &str,
    target_ma: u32,
) {
    let sample = collect_scene_sample(collectors, start, started_unix_ms, phase, target_ma);
    if sample
        .ups_status_cache_fresh
        .as_ref()
        .and_then(Value::as_bool)
        != Some(false)
    {
        samples.push(sample);
    }
}

fn collect_scene_sample(
    collectors: &BTreeMap<String, JsonlProcessCollector>,
    start: Instant,
    started_unix_ms: i64,
    phase: &str,
    target_ma: u32,
) -> SceneSample {
    let elapsed_ms = start.elapsed().as_millis() as i64;
    collect_scene_sample_at(collectors, elapsed_ms, started_unix_ms, phase, target_ma)
}

fn collect_scene_sample_at(
    collectors: &BTreeMap<String, JsonlProcessCollector>,
    elapsed_ms: i64,
    started_unix_ms: i64,
    phase: &str,
    target_ma: u32,
) -> SceneSample {
    let unix_ms = started_unix_ms + elapsed_ms;
    let status_frame = collectors
        .get("ups_status")
        .and_then(|c| c.latest_before(unix_ms));
    let status_meta = status_frame
        .as_ref()
        .and_then(|value| value.pointer("/result/meta"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let status = unwrap_cli_result(status_frame);
    let diag = unwrap_cli_result(
        collectors
            .get("ups_diag_snapshot")
            .and_then(|c| c.latest_before(unix_ms)),
    );
    let load = unwrap_cli_result(
        collectors
            .get("load")
            .and_then(|c| c.latest_before(unix_ms)),
    );
    let power = json!({});
    let input = status
        .pointer("/input")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let output = status
        .pointer("/output")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let out_a = output
        .pointer("/out_a")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let out_b = output
        .pointer("/out_b")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let diag_input = diag
        .pointer("/input")
        .or_else(|| status.pointer("/input"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let charger = status
        .pointer("/charger")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let diag_charger = diag
        .pointer("/charger")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let load_status = load
        .pointer("/payload/status")
        .or_else(|| load.pointer("/status"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let load_control = load
        .pointer("/payload/control")
        .or_else(|| load.pointer("/control"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let port_c = port_c_value(&power);
    let port_telemetry = port_c
        .pointer("/telemetry")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let port_state = port_c
        .pointer("/state")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let runtime_output_enabled = power
        .pointer("/config/runtime/output_enabled")
        .or_else(|| power.pointer("/runtime/output_enabled"))
        .and_then(Value::as_bool)
        .or_else(|| adapter_sample_value(&power, "enabled").and_then(|value| value.as_bool()));
    let source_setpoint_mv = power
        .pointer("/diagnostics/tps_setpoint/mv")
        .cloned()
        .or_else(|| power.pointer("/config/manual/voltage_mv").cloned())
        .or_else(|| power.pointer("/manual/voltage_mv").cloned())
        .or_else(|| adapter_sample_value(&power, "voltage_mv"));
    let source_output_mv = match runtime_output_enabled {
        Some(false) => Some(json!(0)),
        Some(true) => port_telemetry
            .get("voltage_mv")
            .cloned()
            .or_else(|| adapter_sample_value(&power, "voltage_mv"))
            .or(source_setpoint_mv),
        None => port_telemetry
            .get("voltage_mv")
            .cloned()
            .or_else(|| adapter_sample_value(&power, "voltage_mv")),
    };
    let adapter_load_current = adapter_sample_value(&load, "current_ma");
    let measured_load_current = adapter_load_current
        .or_else(|| load_total_current_from_status(&load_status).map(Value::from));
    SceneSample {
        t_s: round3(elapsed_ms as f64 / 1000.0),
        unix_ms,
        phase: phase.to_string(),
        stage: input.get("assist_power_stage").cloned(),
        mode: status.get("mode").cloned(),
        backup_reason: input.get("backup_reason").cloned(),
        load_target_i_ma: target_ma,
        ups_status_cache_age_ms: status_meta.get("cache_age_ms").cloned(),
        ups_status_cache_fresh: status_meta.get("cache_fresh").cloned(),
        ups_status_monitor_running: status_meta.get("monitor_running").cloned(),
        port_c_enabled: port_state
            .get("power_enabled")
            .cloned()
            .or_else(|| runtime_output_enabled.map(Value::from))
            .or_else(|| adapter_sample_value(&power, "enabled")),
        isolapurr_port_c_mv: source_output_mv.or_else(|| {
            input
                .get("pre_tps_vin_mv")
                .cloned()
                .or_else(|| input.get("vin_vbus_mv").cloned())
        }),
        isolapurr_port_c_ma: port_telemetry
            .get("current_ma")
            .cloned()
            .or_else(|| adapter_sample_value(&power, "current_ma")),
        mains_present: input.get("mains_present").cloned(),
        assist_target_vout_mv: input.get("assist_target_vout_mv").cloned(),
        vin_vbus_mv: input
            .get("pre_tps_vin_mv")
            .cloned()
            .or_else(|| input.get("vin_vbus_mv").cloned())
            .or_else(|| input.get("input_vbus_mv").cloned()),
        vin_iin_ma: input
            .get("vin_iin_ma")
            .cloned()
            .or_else(|| input.get("input_ibus_ma").cloned()),
        tps_total_iout_ma: input.get("tps_total_iout_ma").cloned(),
        battery_current_ma: status.pointer("/battery/current_ma").cloned(),
        charger_state: charger.get("state").cloned(),
        charger_allow_charge: charger.get("allow_charge").cloned(),
        out_a_vbus_mv: out_a.get("vbus_mv").cloned(),
        out_b_vbus_mv: out_b.get("vbus_mv").cloned(),
        out_a_iout_ma: out_a.get("iout_ma").cloned(),
        out_b_iout_ma: out_b.get("iout_ma").cloned(),
        diag_stage: diag_input.get("assist_power_stage").cloned(),
        diag_backup_reason: diag_input.get("backup_reason").cloned(),
        diag_charger_notice: diag_charger
            .pointer("/policy/notice")
            .or_else(|| diag_charger.get("notice"))
            .cloned(),
        diag_assist_target_vout_mv: diag_input.get("assist_target_vout_mv").cloned(),
        diag_vin_baseline_mv: diag_input.get("vin_baseline_mv").cloned(),
        diag_vin_drop_mv: diag_input.get("vin_drop_mv").cloned(),
        diag_tps_total_iout_ma: diag_input.get("tps_total_iout_ma").cloned(),
        load_output_enabled: load_control
            .get("output_enabled")
            .cloned()
            .or_else(|| adapter_sample_value(&load, "enabled")),
        load_v_local_mv: load_status
            .get("v_local_mv")
            .cloned()
            .or_else(|| adapter_sample_value(&load, "voltage_mv")),
        load_i_total_ma: measured_load_current,
    }
}

fn backfill_scene_samples_from_ups(
    samples: &mut Vec<SceneSample>,
    collectors: &BTreeMap<String, JsonlProcessCollector>,
    started_unix_ms: i64,
    target_ma: u32,
    minimum_spacing_ms: i64,
) {
    let Some(status_rows) = collectors
        .get("ups_status")
        .and_then(|collector| collector.rows.lock().ok().map(|rows| rows.clone()))
    else {
        return;
    };
    let Some(first_unix_ms) = samples.first().map(|sample| sample.unix_ms) else {
        return;
    };
    let Some(last_unix_ms) = samples.last().map(|sample| sample.unix_ms) else {
        return;
    };
    let timeline = samples
        .iter()
        .map(|sample| (sample.unix_ms, sample.phase.clone()))
        .collect::<Vec<_>>();
    let mut backfill = Vec::new();
    for row in status_rows {
        if row.received_ms < first_unix_ms || row.received_ms > last_unix_ms {
            continue;
        }
        if samples
            .iter()
            .any(|sample| (sample.unix_ms - row.received_ms).abs() < minimum_spacing_ms)
        {
            continue;
        }
        let phase = timeline
            .iter()
            .rev()
            .find(|(unix_ms, _)| *unix_ms <= row.received_ms)
            .or_else(|| timeline.first())
            .map(|(_, phase)| phase.as_str())
            .unwrap_or("pre");
        let sample = collect_scene_sample_at(
            collectors,
            row.received_ms - started_unix_ms,
            started_unix_ms,
            phase,
            target_ma,
        );
        if sample
            .ups_status_cache_fresh
            .as_ref()
            .and_then(Value::as_bool)
            != Some(false)
        {
            backfill.push(sample);
        }
    }
    samples.extend(backfill);
    samples.sort_by_key(|sample| sample.unix_ms);
    samples.dedup_by_key(|sample| sample.unix_ms);
}

fn load_total_current_from_status(status: &Value) -> Option<i64> {
    let voltage_mv = status.get("v_local_mv").and_then(Value::as_i64)?;
    let power_mw = status.get("calc_p_mw").and_then(Value::as_i64)?;
    (voltage_mv > 0).then(|| power_mw.saturating_mul(1_000) / voltage_mv)
}

fn adapter_sample_value(value: &Value, field: &str) -> Option<Value> {
    value
        .pointer(&format!("/sample/{field}"))
        .or_else(|| value.get(field))
        .cloned()
}

#[derive(Debug, Serialize)]
struct ProbeSummary {
    ok: bool,
    command: Vec<String>,
    samples: usize,
    effective_sample_rate_hz: Option<f64>,
    max_gap_s: Option<f64>,
    failures: Vec<String>,
}

async fn run_adapter_probe(cmd: Vec<String>, samples: usize) -> anyhow::Result<ProbeSummary> {
    let rows = collect_ndjson(cmd.clone(), samples, Duration::from_secs(15)).await?;
    let mut times = rows
        .iter()
        .filter_map(|row| frame_time_ms(row))
        .collect::<Vec<_>>();
    times.sort_unstable();
    let mut failures = Vec::new();
    if rows.len() < samples {
        failures.push("too_few_samples".to_string());
    }
    if !rows.iter().any(frame_has_telemetry) {
        failures.push("missing_telemetry_fields".to_string());
    }
    let (rate, max_gap) = sample_rate_and_gap(&times);
    if rate.is_some_and(|rate| rate < ENGINEERING_SAMPLE_RATE_HZ) || rate.is_none() {
        failures.push("sample_rate_below_3hz".to_string());
    }
    if max_gap.is_some_and(|gap| gap > MAX_SAMPLE_GAP_S) || max_gap.is_none() {
        failures.push("sample_gap_above_0_5s".to_string());
    }
    Ok(ProbeSummary {
        ok: failures.is_empty(),
        command: cmd,
        samples: rows.len(),
        effective_sample_rate_hz: rate,
        max_gap_s: max_gap,
        failures,
    })
}

async fn run_json_probe(cmd: Vec<String>, samples: usize) -> anyhow::Result<ProbeSummary> {
    let rows = collect_json_lines(cmd.clone(), samples, Duration::from_secs(15)).await?;
    let mut times = rows
        .iter()
        .filter_map(|row| json_frame_time_ms(row))
        .collect::<Vec<_>>();
    times.sort_unstable();
    let mut failures = Vec::new();
    if rows.len() < samples {
        failures.push("too_few_samples".to_string());
    }
    if !rows.iter().any(json_frame_has_telemetry) {
        failures.push("missing_telemetry_fields".to_string());
    }
    if rows.iter().any(|row| !json_frame_is_fresh(row)) {
        failures.push("stale_or_missing_cache_sample".to_string());
    }
    let (rate, max_gap) = sample_rate_and_gap(&times);
    if rate.is_some_and(|rate| rate < ENGINEERING_SAMPLE_RATE_HZ) || rate.is_none() {
        failures.push("sample_rate_below_3hz".to_string());
    }
    if max_gap.is_some_and(|gap| gap > MAX_SAMPLE_GAP_S) || max_gap.is_none() {
        failures.push("sample_gap_above_0_5s".to_string());
    }
    Ok(ProbeSummary {
        ok: failures.is_empty(),
        command: cmd,
        samples: rows.len(),
        effective_sample_rate_hz: rate,
        max_gap_s: max_gap,
        failures,
    })
}

async fn run_polling_json_probe(
    cmd: Vec<String>,
    samples: usize,
    interval: Duration,
) -> anyhow::Result<ProbeSummary> {
    let mut rows = Vec::new();
    for _ in 0..samples {
        let sample_started = Instant::now();
        let payload = run_cmd_output(cmd.clone()).await?;
        rows.push(json!({
            "received_at_ms": now_ms(),
            "payload": payload,
        }));
        sleep(interval.saturating_sub(sample_started.elapsed())).await;
    }
    let mut times = rows
        .iter()
        .filter_map(json_frame_time_ms)
        .collect::<Vec<_>>();
    times.sort_unstable();
    let mut failures = Vec::new();
    if rows.len() < samples {
        failures.push("too_few_samples".to_string());
    }
    if !rows.iter().any(json_frame_has_telemetry) {
        failures.push("missing_telemetry_fields".to_string());
    }
    let (rate, max_gap) = sample_rate_and_gap(&times);
    if rate.is_some_and(|rate| rate < ENGINEERING_SAMPLE_RATE_HZ) || rate.is_none() {
        failures.push("sample_rate_below_3hz".to_string());
    }
    if max_gap.is_some_and(|gap| gap > MAX_SAMPLE_GAP_S) || max_gap.is_none() {
        failures.push("sample_gap_above_0_5s".to_string());
    }
    Ok(ProbeSummary {
        ok: failures.is_empty(),
        command: cmd,
        samples: rows.len(),
        effective_sample_rate_hz: rate,
        max_gap_s: max_gap,
        failures,
    })
}

async fn collect_json_lines(
    cmd: Vec<String>,
    limit: usize,
    timeout_after: Duration,
) -> anyhow::Result<Vec<Value>> {
    if cmd.is_empty() {
        bail!("empty command");
    }
    let mut child = Command::new(&cmd[0])
        .args(&cmd[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("spawning {:?}", cmd))?;
    let stdout = child.stdout.take().context("missing stdout")?;
    let mut reader = BufReader::new(stdout).lines();
    let deadline = Instant::now() + timeout_after;
    let mut rows = Vec::new();
    while rows.len() < limit {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match timeout(remaining, reader.next_line()).await {
            Ok(Ok(Some(line))) => {
                if line.trim().is_empty() {
                    continue;
                }
                let frame: Value = serde_json::from_str(&line)
                    .with_context(|| format!("parsing JSONL line: {line}"))?;
                rows.push(frame);
            }
            Ok(Ok(None)) => break,
            Ok(Err(error)) => return Err(error.into()),
            Err(_) => break,
        }
    }
    if child.id().is_some() {
        let _ = child.kill().await;
    }
    let _ = child.wait().await;
    Ok(rows)
}

async fn collect_ndjson(
    cmd: Vec<String>,
    limit: usize,
    timeout_after: Duration,
) -> anyhow::Result<Vec<AdapterFrame>> {
    if cmd.is_empty() {
        bail!("empty command");
    }
    let mut child = Command::new(&cmd[0])
        .args(&cmd[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("spawning {:?}", cmd))?;
    let stdout = child.stdout.take().context("missing stdout")?;
    let mut reader = BufReader::new(stdout).lines();
    let deadline = Instant::now() + timeout_after;
    let mut rows = Vec::new();
    while rows.len() < limit {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match timeout(remaining, reader.next_line()).await {
            Ok(Ok(Some(line))) => {
                if line.trim().is_empty() {
                    continue;
                }
                let frame: AdapterFrame = serde_json::from_str(&line)
                    .with_context(|| format!("parsing adapter NDJSON line: {line}"))?;
                rows.push(frame);
            }
            Ok(Ok(None)) => break,
            Ok(Err(error)) => return Err(error.into()),
            Err(_) => break,
        }
    }
    if child.id().is_some() {
        let _ = child.kill().await;
    }
    let _ = child.wait().await;
    Ok(rows)
}

fn frame_time_ms(frame: &AdapterFrame) -> Option<i64> {
    frame
        .sample
        .as_ref()
        .and_then(|sample| sample.timestamp_ms)
        .or_else(|| {
            frame
                .raw
                .get("status_sampled_at_ms")
                .and_then(Value::as_i64)
        })
        .or_else(|| frame.raw.get("received_at_ms").and_then(Value::as_i64))
        .or_else(|| frame.raw.get("timestamp_ms").and_then(Value::as_i64))
        .or_else(|| frame.raw.get("requested_at_ms").and_then(Value::as_i64))
}

fn frame_has_telemetry(frame: &AdapterFrame) -> bool {
    let standard_envelope = frame.event.as_deref() == Some("sample") || frame.ok.is_some();
    if let Some(sample) = &frame.sample {
        if sample.voltage_mv.is_some()
            || sample.current_ma.is_some()
            || sample.power_mw.is_some()
            || sample.enabled.is_some()
        {
            return true;
        }
    }
    let Some(payload) = frame.raw.get("payload") else {
        return false;
    };
    payload
        .pointer("/status/v_local_mv")
        .or_else(|| payload.pointer("/status/calc_p_mw"))
        .or_else(|| payload.pointer("/status/i_local_ma"))
        .or_else(|| payload.pointer("/control/output_enabled"))
        .is_some()
        || standard_envelope && frame.sample.is_some()
}

fn json_frame_time_ms(value: &Value) -> Option<i64> {
    value
        .pointer("/meta/emitted_at_ms")
        .and_then(Value::as_i64)
        .or_else(|| {
            value
                .pointer("/meta/received_at_ms")
                .and_then(Value::as_i64)
        })
        .or_else(|| value.get("received_at_ms").and_then(Value::as_i64))
        .or_else(|| value.get("sample_received_at_ms").and_then(Value::as_i64))
        .or_else(|| value.get("sampled_at_ms").and_then(Value::as_i64))
        .or_else(|| value.get("status_sampled_at_ms").and_then(Value::as_i64))
        .or_else(|| {
            value
                .pointer("/payload/status_sampled_at_ms")
                .and_then(Value::as_i64)
        })
        .or_else(|| value.get("timestamp_ms").and_then(Value::as_i64))
        .or_else(|| value.get("requested_at_ms").and_then(Value::as_i64))
}

fn json_frame_has_telemetry(value: &Value) -> bool {
    value.pointer("/result/input/vin_vbus_mv").is_some()
        || value.pointer("/result/sample/input/vin_vbus_mv").is_some()
        || value
            .pointer("/result/sample/packages/derived.power/payload/input/vin_vbus_mv")
            .is_some()
        || value.pointer("/result/input/mains_present").is_some()
        || value
            .pointer("/result/sample/input/mains_present")
            .is_some()
        || value
            .pointer("/result/sample/packages/derived.power/payload/input/mains_present")
            .is_some()
        || value.pointer("/result/input/tps_total_iout_ma").is_some()
        || value
            .pointer("/result/sample/input/tps_total_iout_ma")
            .is_some()
        || value
            .pointer("/result/sample/packages/derived.power/payload/input/tps_total_iout_ma")
            .is_some()
        || value.pointer("/result/output/out_a/vbus_mv").is_some()
        || value
            .pointer("/result/sample/output/out_a/vbus_mv")
            .is_some()
        || value.pointer("/result/output/out_b/vbus_mv").is_some()
        || value
            .pointer("/result/sample/output/out_b/vbus_mv")
            .is_some()
        || value.pointer("/result/input/vin_baseline_mv").is_some()
        || value
            .pointer("/result/sample/input/vin_baseline_mv")
            .is_some()
        || value
            .pointer("/ports")
            .and_then(Value::as_array)
            .is_some_and(|ports| !ports.is_empty())
        || ((value.pointer("/manual/voltage_mv").is_some()
            && value.pointer("/runtime/output_enabled").is_some())
            || (value.pointer("/payload/manual/voltage_mv").is_some()
                && value.pointer("/payload/runtime/output_enabled").is_some()))
        || value
            .pointer("/ports/ports")
            .and_then(Value::as_array)
            .is_some_and(|ports| !ports.is_empty())
        || value
            .pointer("/payload/ports")
            .and_then(Value::as_array)
            .is_some_and(|ports| !ports.is_empty())
        || value
            .pointer("/payload/ports/ports")
            .and_then(Value::as_array)
            .is_some_and(|ports| !ports.is_empty())
        || value.pointer("/payload/sample/voltage_mv").is_some()
        || value.pointer("/payload/sample/current_ma").is_some()
        || value.pointer("/payload/sample/enabled").is_some()
        || value.pointer("/payload/status/v_local_mv").is_some()
        || value.pointer("/status/v_local_mv").is_some()
        || value.pointer("/status/calc_p_mw").is_some()
        || value.pointer("/status/i_local_ma").is_some()
        || value.pointer("/control/output_enabled").is_some()
}

fn json_frame_is_fresh(value: &Value) -> bool {
    if value.get("miss").and_then(Value::as_bool).unwrap_or(false) {
        return false;
    }
    let Some(result) = value.get("result") else {
        return true;
    };
    let Some(meta) = result.get("meta") else {
        return true;
    };
    let cache_fresh = meta
        .get("cache_fresh")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let monitor_running = meta
        .get("monitor_running")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    cache_fresh && monitor_running
}

fn sample_rate_and_gap(times: &[i64]) -> (Option<f64>, Option<f64>) {
    if times.len() < 2 {
        return (None, None);
    }
    let span_ms = (times[times.len() - 1] - times[0]).max(1) as f64;
    let rate = ((times.len() - 1) as f64) / (span_ms / 1000.0);
    let max_gap = times
        .windows(2)
        .map(|pair| (pair[1] - pair[0]) as f64 / 1000.0)
        .fold(0.0, f64::max);
    (
        Some((rate * 1000.0).round() / 1000.0),
        Some((max_gap * 1000.0).round() / 1000.0),
    )
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(value)? + "\n")?;
    Ok(())
}

fn write_json_value(path: &Path, value: &Value) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(value)? + "\n")?;
    Ok(())
}

fn write_timeseries(path: &Path, samples: &[SceneSample]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut out = String::new();
    for sample in samples {
        out.push_str(&serde_json::to_string(sample)?);
        out.push('\n');
    }
    fs::write(path, out)?;
    Ok(())
}

fn write_suite_overview(path: &Path, suite: &Value) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let suite_id = suite
        .get("suite_id")
        .and_then(Value::as_str)
        .unwrap_or("power-validation-suite");
    let reports = suite
        .get("reports")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut cards = String::new();
    for report in reports {
        let profile = html_escape(
            report
                .get("output_profile")
                .and_then(Value::as_str)
                .unwrap_or("-"),
        );
        let scene = html_escape(
            report
                .get("scene_type")
                .and_then(Value::as_str)
                .unwrap_or("-"),
        );
        let dir = report
            .get("report_dir")
            .and_then(Value::as_str)
            .unwrap_or("");
        let report_label = Path::new(dir)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or(dir);
        let report_href = html_escape(dir);
        let valid = report
            .get("signoff_valid")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let rate = report
            .get("effective_sample_rate_hz")
            .map(Value::to_string)
            .unwrap_or_else(|| "-".to_string());
        let gap = report
            .get("max_sample_gap_s")
            .map(Value::to_string)
            .unwrap_or_else(|| "-".to_string());
        let failures = html_escape(
            &report
                .get("failed_acceptance_checks")
                .cloned()
                .unwrap_or_else(|| json!([]))
                .to_string(),
        );
        let chart = format!("{report_href}/voltage-chart.html?embed=1");
        cards.push_str(&format!(
            r#"<section class="card {valid_class}">
  <header>
    <h2>{profile} / {scene}</h2>
    <span>{valid_label}</span>
  </header>
  <iframe src="{chart}" loading="lazy"></iframe>
  <dl>
    <dt>report</dt><dd><a href="{report_href}/results.json">{report_label}</a></dd>
    <dt>sample rate</dt><dd>{rate} Hz</dd>
    <dt>max gap</dt><dd>{gap} s</dd>
    <dt>failures</dt><dd>{failures}</dd>
  </dl>
</section>
"#,
            valid_class = if valid { "valid" } else { "invalid" },
            valid_label = if valid {
                "valid_for_signoff"
            } else {
                "invalid"
            },
            report_label = html_escape(report_label),
        ));
    }
    let html = format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Mains Aegis Power Path Validation - {suite_id}</title>
  <style>
    :root {{ color-scheme: light; --bg: #f5f7fa; --panel: #ffffff; --ink: #17202a; --muted: #5d6875; --line: #d7dee8; --ok: #2e7d46; --bad: #b54a42; --accent: #1769d2; }}
    * {{ box-sizing: border-box; }}
    body {{ margin: 0; padding: 20px; background: var(--bg); color: var(--ink); font-family: ui-monospace, "SFMono-Regular", Menlo, monospace; }}
    h1 {{ margin: 0; font-size: 24px; line-height: 1.2; }}
    .subtitle {{ margin: 6px 0 16px; color: var(--muted); max-width: 110ch; font-size: 13px; line-height: 1.45; }}
    .grid {{ display: grid; gap: 18px; grid-template-columns: repeat(auto-fit, minmax(min(100%, 1000px), 1fr)); }}
    .card {{ border: 1px solid var(--line); border-radius: 14px; padding: 14px; background: var(--panel); }}
    .card.valid {{ border-color: #9bd5ad; }}
    .card.invalid {{ border-color: #e5a19b; }}
    header {{ display: flex; align-items: center; justify-content: space-between; gap: 16px; margin-bottom: 10px; }}
    h2 {{ margin: 0; font-size: 18px; }}
    span {{ border-radius: 999px; padding: 4px 9px; background: #eaf6ee; color: var(--ok); border: 1px solid #bee4c9; font-size: 12px; }}
    dl {{ display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 8px 14px; margin: 10px 0 0; font-size: 12px; }}
    dt {{ color: var(--muted); }}
    dd {{ margin: 0; overflow-wrap: anywhere; }}
    iframe {{ width: 100%; height: min(72vh, 760px); min-height: 560px; border: 1px solid var(--line); border-radius: 12px; background: white; display: block; }}
    @media (max-width: 900px) {{ dl {{ grid-template-columns: 110px 1fr; }} iframe {{ height: 640px; }} }}
  </style>
</head>
<body>
  <h1>Mains Aegis Power Path Validation</h1>
  <p class="subtitle">Suite {suite_id}. Formal evidence uses the selected CLI adapter transports; UPS and LoadLynx use native IPC + USB for the explicitly selected bench, while the power-source adapter may use its stable CLI transport. The historical directory name may still contain <code>hil</code>.</p>
  <main class="grid">
{cards}
  </main>
</body>
</html>
"#,
        suite_id = html_escape(suite_id),
        cards = cards
    );
    fs::write(path, html)?;
    Ok(())
}

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

async fn run_cmd_json(
    cmd: Vec<String>,
    name: &str,
    actions: &mut Vec<Value>,
) -> anyhow::Result<Value> {
    let result = run_cmd_output(cmd.clone())
        .await
        .with_context(|| format!("running {name}"))?;
    actions.push(json!({name: {"cmd": cmd, "result": result}}));
    Ok(result)
}

async fn run_cmd_json_retry(
    cmd: Vec<String>,
    name: &str,
    actions: &mut Vec<Value>,
    attempts: usize,
) -> anyhow::Result<Value> {
    let attempts = attempts.max(1);
    let mut last_error = None;
    for attempt in 1..=attempts {
        match run_cmd_output(cmd.clone()).await {
            Ok(result) => {
                actions.push(json!({name: {"cmd": cmd, "attempt": attempt, "result": result}}));
                return Ok(result);
            }
            Err(error) => {
                let message = error.to_string();
                actions.push(json!({name: {"cmd": cmd, "attempt": attempt, "error": message}}));
                last_error = Some(error);
                if attempt < attempts {
                    sleep(Duration::from_millis(500)).await;
                }
            }
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow!("command retry failed without error")))
        .with_context(|| format!("running {name}"))
}

async fn run_cmd_output_retry(cmd: Vec<String>, attempts: usize) -> anyhow::Result<Value> {
    let attempts = attempts.max(1);
    let mut last_error = None;
    for attempt in 1..=attempts {
        match run_cmd_output(cmd.clone()).await {
            Ok(result) => return Ok(result),
            Err(error) => {
                last_error = Some(error);
                if attempt < attempts {
                    sleep(Duration::from_millis(500)).await;
                }
            }
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow!("command retry failed without error")))
}

#[allow(clippy::too_many_arguments)]
async fn run_cmd_json_with_sampling(
    cmd: Vec<String>,
    name: &str,
    actions: &mut Vec<Value>,
    args: &RunArgs,
    collectors: &BTreeMap<String, JsonlProcessCollector>,
    samples: &mut Vec<SceneSample>,
    start: Instant,
    started_unix_ms: i64,
    phase: &str,
    target_ma: u32,
) -> anyhow::Result<Value> {
    if cmd.is_empty() {
        bail!("empty command");
    }
    let child = Command::new(&cmd[0])
        .args(&cmd[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("spawning {name}: {:?}", cmd))?;
    let mut wait = Box::pin(child.wait_with_output());
    let mut tick = tokio::time::interval(Duration::from_millis(args.bench.sample_interval_ms));
    loop {
        tokio::select! {
            output = &mut wait => {
                let output = output.with_context(|| format!("waiting for {name}: {:?}", cmd))?;
                let stdout_text = String::from_utf8(output.stdout)?;
                let stderr_text = String::from_utf8_lossy(&output.stderr).to_string();
                if !output.status.success() {
                    bail!(
                        "command failed {:?}: status={} stdout={} stderr={}",
                        cmd,
                        output.status,
                        stdout_text,
                        stderr_text
                    );
                }
                let result = parse_command_stdout(&stdout_text)?;
                actions.push(json!({name: {"cmd": cmd, "result": result}}));
                return Ok(result);
            },
            _ = tick.tick() => {
                push_scene_sample_if_fresh(
                    samples,
                    collectors,
                    start,
                    started_unix_ms,
                    phase,
                    target_ma,
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_cmd_json_with_sampling_retry(
    cmd: Vec<String>,
    name: &str,
    actions: &mut Vec<Value>,
    args: &RunArgs,
    collectors: &BTreeMap<String, JsonlProcessCollector>,
    samples: &mut Vec<SceneSample>,
    start: Instant,
    started_unix_ms: i64,
    phase: &str,
    target_ma: u32,
    attempts: usize,
) -> anyhow::Result<Value> {
    let attempts = attempts.max(1);
    let mut last_error = None;
    for attempt in 1..=attempts {
        match run_cmd_json_with_sampling(
            cmd.clone(),
            name,
            actions,
            args,
            collectors,
            samples,
            start,
            started_unix_ms,
            phase,
            target_ma,
        )
        .await
        {
            Ok(result) => return Ok(result),
            Err(error) => {
                actions.push(
                    json!({name: {"cmd": cmd, "attempt": attempt, "error": error.to_string()}}),
                );
                last_error = Some(error);
                if attempt < attempts {
                    sleep(Duration::from_millis(500)).await;
                }
            }
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow!("sampled command retry failed without error")))
}

async fn run_cmd_output(cmd: Vec<String>) -> anyhow::Result<Value> {
    if cmd.is_empty() {
        bail!("empty command");
    }
    let output = Command::new(&cmd[0])
        .args(&cmd[1..])
        .output()
        .await
        .with_context(|| format!("spawning {:?}", cmd))?;
    if !output.status.success() {
        bail!(
            "command failed {:?}: status={} stdout={} stderr={}",
            cmd,
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8(output.stdout)?;
    parse_command_stdout(&stdout)
}

fn parse_command_stdout(stdout: &str) -> anyhow::Result<Value> {
    Ok(if stdout.trim().is_empty() {
        json!({})
    } else if let Ok(value) = serde_json::from_str(stdout.trim()) {
        value
    } else {
        let json_line = stdout
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or(stdout.trim());
        serde_json::from_str(json_line.trim())?
    })
}

fn unwrap_cli_result(value: Option<Value>) -> Value {
    let Some(value) = value else {
        return json!({});
    };
    if let Some(result) = value.get("result") {
        if let Some(sample) = result.get("sample") {
            return sample.clone();
        }
        return result.clone();
    }
    if let Some(payload) = value.get("payload") {
        return payload.clone();
    }
    if let Some(sample) = value.get("sample") {
        return sample.clone();
    }
    value
}

fn port_c_value(power: &Value) -> Value {
    power
        .get("ports")
        .and_then(Value::as_array)
        .and_then(|ports| {
            ports
                .iter()
                .find(|port| port.get("portId").and_then(Value::as_str) == Some("port_c"))
        })
        .cloned()
        .unwrap_or_else(|| json!({}))
}

fn validate_profile_gate(profile: OutputProfile, identity: &Value, settings: &Value) -> Value {
    let expected_profile = profile.key();
    let expected_vout = profile.rated_vout_mv() as i64;
    let identity_caps = identity
        .get("hardware_capabilities")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let settings_caps = settings
        .get("advanced_power_capabilities")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut failures = Vec::new();
    if identity_caps.get("output_profile").and_then(Value::as_str) != Some(expected_profile) {
        failures.push("identity_output_profile_mismatch");
    }
    if identity_caps.get("rated_vout_mv").and_then(Value::as_i64) != Some(expected_vout) {
        failures.push("identity_rated_vout_mismatch");
    }
    if settings_caps.get("rated_vout_mv").and_then(Value::as_i64) != Some(expected_vout) {
        failures.push("settings_rated_vout_mismatch");
    }
    json!({
        "ok": failures.is_empty(),
        "failures": failures,
        "identity_caps": identity_caps,
        "settings_caps": settings_caps,
    })
}

fn validate_suite_settings(
    profile: OutputProfile,
    suite_contract: SuiteContract,
    settings: &Value,
    profile_gate: &mut Value,
    uvlo_expectation: SourceLimitedUvloExpectation,
) -> anyhow::Result<()> {
    if !suite_contract.is_source_limited() {
        return Ok(());
    }
    let advanced_power = settings
        .get("advanced_power")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let defaults = SourceLimitedSettingsExpectation::for_profile(profile);
    let expected = json!({
        "source_limited_vin_drop_pct": defaults.source_limited_vin_drop_pct,
        "source_limited_enter_delta_ma": defaults.source_limited_enter_delta_ma,
        "source_limited_exit_delta_ma": defaults.source_limited_exit_delta_ma,
        "source_limited_required_samples": defaults.source_limited_required_samples,
        "source_limited_recover_margin_mv": defaults.source_limited_recover_margin_mv,
        "vin_drop_threshold_pct": defaults.vin_drop_threshold_pct,
        "input_uvlo_cutoff_mv": uvlo_expectation.cutoff_mv,
        "input_uvlo_recover_mv": uvlo_expectation.recover_mv,
        "input_uvlo_required_samples": uvlo_expectation.required_samples,
    });
    let mut failures = Vec::new();
    for (key, expected_value) in expected.as_object().into_iter().flatten() {
        if advanced_power.get(key) != Some(expected_value) {
            failures.push(format!("advanced_power_{key}_mismatch"));
        }
    }
    let settings_gate = json!({
        "ok": failures.is_empty(),
        "failures": failures,
        "expected": expected,
        "actual": advanced_power,
    });
    profile_gate["source_limited_settings"] = settings_gate.clone();
    if settings_gate.get("ok").and_then(Value::as_bool) != Some(true) {
        bail!("source-limited settings preflight failed: {settings_gate}");
    }
    Ok(())
}

fn summarize_scene(samples: &[SceneSample]) -> SceneSummary {
    let mut failures = Vec::new();
    if samples.len() < 2 {
        failures.push("too_few_samples".to_string());
    }
    let times = samples.iter().map(|sample| sample.t_s).collect::<Vec<_>>();
    let span = times.last().copied().unwrap_or(0.0) - times.first().copied().unwrap_or(0.0);
    let rate = if samples.len() > 1 && span > 0.0 {
        Some(round3((samples.len() - 1) as f64 / span))
    } else {
        None
    };
    let max_gap = if times.len() > 1 {
        Some(round3(
            times
                .windows(2)
                .map(|pair| pair[1] - pair[0])
                .fold(0.0, f64::max),
        ))
    } else {
        None
    };
    if rate.is_none_or(|rate| rate < MIN_FORMAL_SAMPLE_RATE_HZ) {
        failures.push("sample_rate_below_2hz".to_string());
    }
    if max_gap.is_none_or(|gap| gap > MAX_SAMPLE_GAP_S) {
        failures.push("sample_gap_above_0_5s".to_string());
    }
    let stale_ups_samples = samples
        .iter()
        .filter(|sample| {
            sample
                .ups_status_cache_fresh
                .as_ref()
                .and_then(Value::as_bool)
                != Some(true)
        })
        .count();
    let stopped_ups_monitor_samples = samples
        .iter()
        .filter(|sample| {
            sample
                .ups_status_monitor_running
                .as_ref()
                .and_then(Value::as_bool)
                != Some(true)
        })
        .count();
    if stale_ups_samples > 0 {
        failures.push(format!("stale_ups_status_samples:{stale_ups_samples}"));
    }
    if stopped_ups_monitor_samples > 0 {
        failures.push(format!(
            "stopped_ups_monitor_samples:{stopped_ups_monitor_samples}"
        ));
    }
    let mut required = BTreeMap::new();
    required.insert(
        "source_output_voltage".to_string(),
        samples
            .iter()
            .any(|sample| sample.isolapurr_port_c_mv.is_some()),
    );
    required.insert(
        "ups_dcin_voltage".to_string(),
        samples.iter().any(|sample| sample.vin_vbus_mv.is_some()),
    );
    required.insert(
        "ups_output_voltage".to_string(),
        samples
            .iter()
            .any(|sample| sample.out_a_vbus_mv.is_some() || sample.out_b_vbus_mv.is_some()),
    );
    required.insert(
        "load_actual_voltage".to_string(),
        samples
            .iter()
            .any(|sample| sample.load_v_local_mv.is_some()),
    );
    for (key, ok) in &required {
        if !ok {
            failures.push(format!("missing_{key}"));
        }
    }
    SceneSummary {
        scene_complete: failures.is_empty(),
        failures,
        effective_sample_rate_hz: rate,
        max_sample_gap_s: max_gap,
        required_voltage_series: required,
    }
}

async fn render_scene_chart(
    args: &RunArgs,
    report_dir: &Path,
    profile: OutputProfile,
    scene: SceneKind,
) -> anyhow::Result<()> {
    if !args.render_chart.exists() {
        return Ok(());
    }
    let title = format!("{} {}", profile.key(), scene.key());
    let cmd = vec![
        "python3".to_string(),
        args.render_chart.to_string_lossy().to_string(),
        "--input".to_string(),
        report_dir
            .join("timeseries.jsonl")
            .to_string_lossy()
            .to_string(),
        "--output".to_string(),
        report_dir
            .join("voltage-chart.html")
            .to_string_lossy()
            .to_string(),
        "--title".to_string(),
        title,
    ];
    let _ = run_cmd_output(cmd).await;
    Ok(())
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

fn adapter_protocol() -> Value {
    json!({
        "name": "mains-aegis power-validation external adapter protocol",
        "transport": "process stdio",
        "invocation": "<adapter> --role <power-source|electronic-load> --action <capabilities|configure|enable|disable|set-load|stream> [action parameters]",
        "stdout": {
            "one_shot": "one JSON object",
            "stream": "strict NDJSON; each line is one JSON object; diagnostics belong on stderr"
        },
        "common_args": {
            "--role": ["power-source", "electronic-load"],
            "--action": ["capabilities", "configure", "enable", "disable", "set-load", "stream"]
        },
        "power_source_actions": {
            "capabilities": [],
            "configure": ["--voltage-mv", "--current-limit-ma", "--enabled false"],
            "enable": ["--voltage-mv", "--current-limit-ma"],
            "disable": [],
            "stream": ["--interval-ms"]
        },
        "electronic_load_actions": {
            "capabilities": [],
            "disable": [],
            "set-load": ["--target-ma", "--min-v-mv", "--max-i-ma-total", "--max-p-mw"],
            "stream": ["--interval-ms", "--count optional"]
        },
        "one_shot_success": {
            "ok": true,
            "applied": {
                "voltage_mv": 12000,
                "current_limit_ma": 3000,
                "target_ma": 3900,
                "enabled": true
            }
        },
        "one_shot_failure": {
            "ok": false,
            "error_code": "unsupported_or_validation_failed",
            "message": "human-readable error"
        },
        "required_stream_sample": {
            "event": "sample",
            "sample": {
                "timestamp_ms": 1760000000000_i64,
                "voltage_mv": 12000,
                "current_ma": 1000,
                "power_mw": 12000,
                "enabled": true,
                "protection_state": "normal"
            }
        },
        "rules": [
            "adapters must never silently clamp requested voltage/current/load values",
            "adapters must reject unsafe or unsupported requests with ok=false rather than applying a nearby setting",
            "unsupported operations must return ok=false and error_code=unsupported",
            "stream stdout must remain NDJSON only; progress logs belong on stderr",
            "stream actions must sustain >=2Hz and max sample gap <=0.5s for formal evidence",
            "power-source adapters must support configure disabled, enable, disable, and telemetry",
            "electronic-load adapters must support disable, CC load with UVP/OCP/OPP protection, and telemetry"
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bench(load_cli: Option<&str>) -> BenchArgs {
        BenchArgs {
            ups_device: "serial-test".to_string(),
            ups_cli: Some(PathBuf::from("mains-aegis")),
            power_adapter: PowerAdapterKind::Isolapurr,
            power_adapter_cmd: None,
            power_device: "source-test".to_string(),
            load_adapter: LoadAdapterKind::Loadlynx,
            load_adapter_cmd: None,
            load_device: "load-test".to_string(),
            load_cli: load_cli.map(PathBuf::from),
            load_ipc: Some("/tmp/load.sock".to_string()),
            isolapurr_cli: PathBuf::from("isolapurr"),
            isolapurr_ipc: Some("/tmp/isolapurr.sock".to_string()),
            isolapurr_url: None,
            sample_interval_ms: 200,
            ups_watch_freshness_ms: 750,
        }
    }

    fn external_bench() -> BenchArgs {
        BenchArgs {
            ups_device: "serial-test".to_string(),
            ups_cli: Some(PathBuf::from("mains-aegis")),
            power_adapter: PowerAdapterKind::External,
            power_adapter_cmd: Some(PathBuf::from("/opt/source-adapter")),
            power_device: "source-test".to_string(),
            load_adapter: LoadAdapterKind::External,
            load_adapter_cmd: Some(PathBuf::from("/opt/load-adapter")),
            load_device: "load-test".to_string(),
            load_cli: None,
            load_ipc: None,
            isolapurr_cli: PathBuf::from("isolapurr"),
            isolapurr_ipc: None,
            isolapurr_url: None,
            sample_interval_ms: 200,
            ups_watch_freshness_ms: 750,
        }
    }

    fn scene_sample(
        t_s: f64,
        phase: &str,
        stage: &str,
        mode: &str,
        backup_reason: Option<&str>,
        load_v_local_mv: i64,
    ) -> SceneSample {
        SceneSample {
            t_s,
            unix_ms: (t_s * 1000.0) as i64,
            phase: phase.to_string(),
            stage: Some(json!(stage)),
            mode: Some(json!(mode)),
            backup_reason: backup_reason.map(|reason| json!(reason)),
            load_target_i_ma: 3_900,
            ups_status_cache_age_ms: Some(json!(20)),
            ups_status_cache_fresh: Some(json!(true)),
            ups_status_monitor_running: Some(json!(true)),
            port_c_enabled: Some(json!(true)),
            isolapurr_port_c_mv: Some(json!(12_000)),
            isolapurr_port_c_ma: Some(json!(3_000)),
            mains_present: Some(json!(backup_reason != Some("input_absent"))),
            assist_target_vout_mv: Some(json!(12_000)),
            vin_vbus_mv: Some(json!(11_800)),
            vin_iin_ma: Some(json!(3_000)),
            tps_total_iout_ma: Some(json!(3_900)),
            battery_current_ma: Some(json!(-500)),
            charger_state: Some(json!(if backup_reason == Some("input_absent") {
                "NOAC"
            } else {
                "LOAD"
            })),
            charger_allow_charge: Some(json!(false)),
            out_a_vbus_mv: Some(json!(12_000)),
            out_b_vbus_mv: None,
            out_a_iout_ma: Some(json!(3_900)),
            out_b_iout_ma: None,
            diag_stage: Some(json!(stage)),
            diag_backup_reason: backup_reason.map(|reason| json!(reason)),
            diag_charger_notice: None,
            diag_assist_target_vout_mv: Some(json!(12_000)),
            diag_vin_baseline_mv: Some(json!(12_000)),
            diag_vin_drop_mv: Some(json!(200)),
            diag_tps_total_iout_ma: Some(json!(3_900)),
            load_output_enabled: Some(json!(true)),
            load_v_local_mv: Some(json!(load_v_local_mv)),
            load_i_total_ma: Some(json!(3_900)),
        }
    }

    #[test]
    fn ups_commands_propagate_no_auto_start() {
        let cmd = ups_status_fresh_command(
            &bench(Some("loadlynx")),
            &PowerValidationArgs {
                ups_ipc: "/tmp/ups.sock".to_string(),
                no_auto_start: true,
            },
        );

        assert_eq!(cmd[0], "mains-aegis");
        assert!(cmd.contains(&"--ipc".to_string()));
        assert!(cmd.contains(&"/tmp/ups.sock".to_string()));
        assert!(cmd.contains(&"--no-auto-start".to_string()));
        assert!(cmd.contains(&"status".to_string()));
    }

    #[test]
    fn loadlynx_stream_requires_explicit_cli() {
        let err = load_stream_command(&bench(None), 4).unwrap_err();
        assert!(err.to_string().contains("--load-cli"));
    }

    #[test]
    fn loadlynx_stream_uses_released_cli_without_legacy_ipc_flag() {
        let cmd = load_stream_command(&bench(Some("/opt/loadlynx")), 40).unwrap();
        assert_eq!(cmd[0], "/opt/loadlynx");
        assert!(!cmd.contains(&"--ipc".to_string()));
        assert!(!cmd.contains(&"/tmp/load.sock".to_string()));
        assert!(cmd.contains(&"status-stream".to_string()));
        assert!(cmd.contains(&"--interval-ms".to_string()));
        assert!(cmd.contains(&"200".to_string()));
        assert!(cmd.contains(&"--count".to_string()));
        assert!(cmd.contains(&"40".to_string()));
    }

    #[test]
    fn loadlynx_preflight_stream_matches_formal_unbounded_collector() {
        let cmd = preflight_load_stream_command(&bench(Some("/opt/loadlynx")), 40).unwrap();
        assert!(!cmd.contains(&"--count".to_string()));
    }

    #[test]
    fn isolapurr_commands_include_selected_ipc_endpoint() {
        let bench = bench(Some("loadlynx"));
        for cmd in [
            power_capabilities_command(&bench).unwrap(),
            power_config_show_command(&bench).unwrap(),
            power_disable_command(&bench).unwrap(),
            power_stream_command(&bench).unwrap(),
        ] {
            assert_eq!(cmd[0], "isolapurr");
            assert!(cmd.contains(&"--ipc".to_string()));
            assert!(cmd.contains(&"/tmp/isolapurr.sock".to_string()));
        }
        let stream = power_stream_command(&bench).unwrap();
        assert!(stream.contains(&"show".to_string()));
        assert!(!stream.contains(&"telemetry".to_string()));
    }

    #[test]
    fn isolapurr_tps_cdc_rise_guard_requires_an_unchanged_value() {
        let before = json!(300);
        let changed_after = json!(0);
        let mut actions = Vec::new();
        ensure_isolapurr_tps_cdc_rise_preserved(Some(&before), Some(&before), &mut actions)
            .unwrap();
        assert_eq!(
            actions[0].pointer("/power_tps_cdc_rise_guard/preserved"),
            Some(&json!(true))
        );

        let mut actions = Vec::new();
        assert!(ensure_isolapurr_tps_cdc_rise_preserved(
            Some(&before),
            Some(&changed_after),
            &mut actions,
        )
        .is_err());
    }

    #[test]
    fn isolapurr_commands_can_use_explicit_url_transport() {
        let mut bench = bench(Some("loadlynx"));
        bench.isolapurr_ipc = None;
        bench.isolapurr_url = Some("http://127.0.0.1:30182".to_string());
        let cmd = power_capabilities_command(&bench).unwrap();
        assert!(cmd.contains(&"--url".to_string()));
        assert!(cmd.contains(&"http://127.0.0.1:30182".to_string()));
        assert!(!cmd.contains(&"--ipc".to_string()));
        assert!(!cmd.contains(&"--device-id".to_string()));
        assert_eq!(
            cmd,
            vec![
                "isolapurr",
                "--json",
                "power",
                "show",
                "--url",
                "http://127.0.0.1:30182",
            ]
        );

        let disable = power_disable_command(&bench).unwrap();
        assert!(disable
            .windows(3)
            .any(|pair| pair == ["power", "runtime", "output"]));
        assert!(disable
            .windows(2)
            .any(|pair| pair == ["--enabled", "false"]));

        let configure = power_configure_off_command(&bench, OutputProfile::V19).unwrap();
        assert!(configure
            .windows(3)
            .any(|pair| pair == ["power", "config", "set"]));
        assert!(configure
            .windows(2)
            .any(|pair| pair == ["--tps-mode", "manual"]));
        assert!(!configure.contains(&"--device-id".to_string()));
        assert!(configure
            .windows(2)
            .any(|pair| pair == ["--usb-c-path", "disconnected"]));

        let enable = power_port_enable_command(&bench, OutputProfile::V19).unwrap();
        assert!(enable
            .windows(3)
            .any(|pair| pair == ["power", "runtime", "output"]));
        assert!(enable.windows(2).any(|pair| pair == ["--enabled", "true"]));

        let stream = power_stream_command(&bench).unwrap();
        assert_eq!(
            stream,
            vec![
                "isolapurr",
                "--json",
                "power",
                "config",
                "show",
                "--url",
                "http://127.0.0.1:30182",
            ]
        );
    }

    #[test]
    fn external_power_adapter_receives_source_parameters() {
        let bench = external_bench();
        let configure = power_configure_off_command(&bench, OutputProfile::V19).unwrap();
        assert_eq!(configure[0], "/opt/source-adapter");
        assert!(configure
            .windows(2)
            .any(|pair| pair == ["--role", "power-source"]));
        assert!(configure
            .windows(2)
            .any(|pair| pair == ["--action", "configure"]));
        assert!(configure
            .windows(2)
            .any(|pair| pair == ["--voltage-mv", "19000"]));
        assert!(configure
            .windows(2)
            .any(|pair| pair == ["--current-limit-ma", "3000"]));
        assert!(configure
            .windows(2)
            .any(|pair| pair == ["--enabled", "false"]));

        let stream = power_stream_command(&bench).unwrap();
        assert!(stream.windows(2).any(|pair| pair == ["--action", "stream"]));
        assert!(stream
            .windows(2)
            .any(|pair| pair == ["--interval-ms", "200"]));

        let enable_again = power_port_enable_command(&bench, OutputProfile::V19).unwrap();
        assert!(enable_again
            .windows(2)
            .any(|pair| pair == ["--action", "enable"]));
        assert!(enable_again
            .windows(2)
            .any(|pair| pair == ["--voltage-mv", "19000"]));
        assert!(enable_again
            .windows(2)
            .any(|pair| pair == ["--current-limit-ma", "3000"]));
    }

    #[test]
    fn external_load_adapter_receives_load_and_protection_parameters() {
        let bench = external_bench();
        let set_load = load_cc_command(
            &bench,
            3900,
            LoadProtection {
                load_min_v_mv: 3000,
                load_max_i_ma_total: 4000,
                load_max_p_mw: 80000,
            },
        )
        .unwrap();
        assert_eq!(set_load[0], "/opt/load-adapter");
        assert!(set_load
            .windows(2)
            .any(|pair| pair == ["--role", "electronic-load"]));
        assert!(set_load
            .windows(2)
            .any(|pair| pair == ["--action", "set-load"]));
        assert!(set_load
            .windows(2)
            .any(|pair| pair == ["--target-ma", "3900"]));
        assert!(set_load
            .windows(2)
            .any(|pair| pair == ["--min-v-mv", "3000"]));
        assert!(set_load
            .windows(2)
            .any(|pair| pair == ["--max-i-ma-total", "4000"]));
        assert!(set_load
            .windows(2)
            .any(|pair| pair == ["--max-p-mw", "80000"]));

        let stream = load_stream_command(&bench, 40).unwrap();
        assert!(stream.windows(2).any(|pair| pair == ["--action", "stream"]));
        assert!(stream
            .windows(2)
            .any(|pair| pair == ["--interval-ms", "200"]));
        assert!(stream.windows(2).any(|pair| pair == ["--count", "40"]));
    }

    #[test]
    fn probe_accepts_loadlynx_status_stream_shape() {
        let frame: AdapterFrame = serde_json::from_str(
            r#"{"seq":1,"requested_at_ms":1000,"received_at_ms":1030,"payload":{"control":{"output_enabled":false},"status":{"v_local_mv":12000,"i_local_ma":10,"calc_p_mw":120}}}"#,
        )
        .unwrap();
        assert_eq!(frame_time_ms(&frame), Some(1030));
        assert!(frame_has_telemetry(&frame));
    }

    #[test]
    fn json_probe_accepts_loadlynx_status_snapshot_shape() {
        let frame = json!({
            "status_sampled_at_ms": 1030,
            "control": {"output_enabled": false},
            "status": {"v_local_mv": 12000, "i_local_ma": 10, "calc_p_mw": 120}
        });
        assert_eq!(json_frame_time_ms(&frame), Some(1030));
        assert!(json_frame_has_telemetry(&frame));
    }

    #[test]
    fn json_probe_accepts_isolapurr_nested_port_telemetry() {
        let frame = json!({
            "received_at_ms": 1000,
            "payload": {
                "ports": {
                    "ports": [{
                        "portId": "port_a",
                        "telemetry": {"voltage_mv": 5013, "current_ma": 0}
                    }]
                }
            }
        });
        assert!(json_frame_has_telemetry(&frame));
    }

    #[test]
    fn json_probe_accepts_isolapurr_power_config_snapshot() {
        let frame = json!({
            "received_at_ms": 1000,
            "manual": {"voltage_mv": 12000, "tps_cdc_rise_mv": 300},
            "runtime": {"output_enabled": false}
        });
        assert!(json_frame_has_telemetry(&frame));
        assert!(json_frame_has_telemetry(&json!({"payload": frame})));
    }

    #[test]
    fn json_probe_accepts_ups_diag_snapshot_power_package() {
        let frame = json!({
            "sample_received_at_ms": 1000,
            "result": {
                "sample": {
                    "packages": {
                        "derived.power": {
                            "payload": {"input": {"vin_vbus_mv": 12_000}}
                        }
                    }
                }
            }
        });
        assert!(json_frame_has_telemetry(&frame));
    }

    #[test]
    fn suite_plan_has_four_default_scenes() {
        let run = RunArgs {
            bench: bench(Some("loadlynx")),
            suite_contract: SuiteContract::Standard,
            profiles: vec![OutputProfile::V12, OutputProfile::V19],
            scenes: vec![SceneKind::AssistPath, SceneKind::BackupOnly],
            report_root: PathBuf::from("reports"),
            suite_id: Some("suite".to_string()),
            dry_run: true,
            allow_profile_flash: false,
            artifact_manifest_12v: None,
            artifact_manifest_19v: None,
            pre_s: 0.1,
            hold_s: 0.1,
            backup_s: 0.1,
            restore_s: 0.1,
            post_s: 0.1,
            profile_flash_settle_s: 12,
            expected_input_uvlo_cutoff_mv: None,
            expected_input_uvlo_recover_mv: None,
            expected_input_uvlo_required_samples: None,
            render_chart: PathBuf::from("tools/hil/render_voltage_chart_html.py"),
        };
        let plan = build_suite_plan(
            &run,
            &PowerValidationArgs {
                ups_ipc: "/tmp/ups.sock".to_string(),
                no_auto_start: false,
            },
            "suite",
            Path::new("reports/suite"),
        )
        .unwrap();
        assert_eq!(plan.reports.len(), 4);
        assert_eq!(plan.reports[0].output_profile, "12v");
        assert_eq!(plan.reports[0].scene_type, "assist_path");
        assert_eq!(plan.reports[0].target_ma, 3900);
        assert!(plan.reports[0]
            .commands
            .ups_artifact_select
            .contains(&"<required-12v-manifest.json>".to_string()));
        assert!(plan.reports[0]
            .commands
            .ups_flash
            .contains(&"--real".to_string()));
        assert_eq!(plan.reports[1].scene_type, "backup_only");
        assert_eq!(plan.reports[1].target_ma, 1000);
        assert_eq!(plan.reports[1].include_backup, true);
    }

    #[test]
    fn source_limited_contract_forces_four_12v_scenes() {
        let run = RunArgs {
            bench: bench(Some("loadlynx")),
            suite_contract: SuiteContract::SourceLimited12v,
            profiles: vec![OutputProfile::V19],
            scenes: vec![SceneKind::AssistPath],
            report_root: PathBuf::from("reports"),
            suite_id: Some("suite".to_string()),
            dry_run: true,
            allow_profile_flash: false,
            artifact_manifest_12v: None,
            artifact_manifest_19v: None,
            pre_s: 0.1,
            hold_s: 0.1,
            backup_s: 0.1,
            restore_s: 0.1,
            post_s: 0.1,
            profile_flash_settle_s: 12,
            expected_input_uvlo_cutoff_mv: None,
            expected_input_uvlo_recover_mv: None,
            expected_input_uvlo_required_samples: None,
            render_chart: PathBuf::from("tools/hil/render_voltage_chart_html.py"),
        };
        let plan = build_suite_plan(
            &run,
            &PowerValidationArgs {
                ups_ipc: "/tmp/ups.sock".to_string(),
                no_auto_start: false,
            },
            "suite",
            Path::new("reports/suite"),
        )
        .unwrap();

        assert_eq!(plan.suite_contract, SuiteContract::SourceLimited12v);
        assert_eq!(plan.profiles.len(), 1);
        assert_eq!(plan.profiles[0].output_profile, "12v");
        assert_eq!(
            plan.reports
                .iter()
                .map(|report| report.scene_type)
                .collect::<Vec<_>>(),
            vec![
                "backup_only",
                "source_in_budget",
                "source_limited_online",
                "source_limited_cut",
            ]
        );
        assert!(!plan.reports[1].include_backup);
        assert_eq!(plan.reports[1].target_ma, 2_500);
        assert!(!plan.reports[2].include_backup);
        assert!(plan.reports[3].include_backup);
    }

    #[test]
    fn source_limited_19v_contract_forces_four_19v_scenes() {
        let contract = SuiteContract::SourceLimited19v;
        assert_eq!(
            contract.selected_profiles(&[OutputProfile::V12]),
            vec![OutputProfile::V19]
        );
        assert_eq!(
            contract.selected_scenes(&[SceneKind::AssistPath]),
            vec![
                SceneKind::BackupOnly,
                SceneKind::SourceInBudget,
                SceneKind::SourceLimitedOnline,
                SceneKind::SourceLimitedCut,
            ]
        );
        assert_eq!(
            contract.expected_reports(),
            vec![
                ("19v", "backup_only"),
                ("19v", "source_in_budget"),
                ("19v", "source_limited_online"),
                ("19v", "source_limited_cut"),
            ]
        );
    }

    #[test]
    fn source_limited_load_floor_tracks_the_rated_output_profile() {
        assert_eq!(source_limited_min_load_mv(OutputProfile::V12), 11_000);
        assert_eq!(source_limited_min_load_mv(OutputProfile::V19), 18_000);
    }

    #[test]
    fn source_limited_settings_preflight_requires_the_12v_bench_defaults() {
        let mut profile_gate = json!({"ok": true});
        let settings = json!({
            "advanced_power": {
                "source_limited_vin_drop_pct": 1,
                "source_limited_enter_delta_ma": 2500,
                "source_limited_exit_delta_ma": 0,
                "source_limited_required_samples": 2,
                "source_limited_recover_margin_mv": 400,
                "vin_drop_threshold_pct": 4,
                "input_uvlo_cutoff_mv": 11_300,
                "input_uvlo_recover_mv": 11_500,
                "input_uvlo_required_samples": 3,
            }
        });
        validate_suite_settings(
            OutputProfile::V12,
            SuiteContract::SourceLimited12v,
            &settings,
            &mut profile_gate,
            SourceLimitedUvloExpectation::for_profile(OutputProfile::V12),
        )
        .unwrap();
        assert_eq!(
            profile_gate.pointer("/source_limited_settings/ok"),
            Some(&json!(true))
        );

        let mut mismatch_gate = json!({"ok": true});
        let mismatch = json!({"advanced_power": {}});
        assert!(validate_suite_settings(
            OutputProfile::V12,
            SuiteContract::SourceLimited12v,
            &mismatch,
            &mut mismatch_gate,
            SourceLimitedUvloExpectation::for_profile(OutputProfile::V12),
        )
        .is_err());
        assert!(mismatch_gate
            .pointer("/source_limited_settings/failures")
            .and_then(Value::as_array)
            .is_some_and(|failures| !failures.is_empty()));
    }

    #[test]
    fn source_limited_settings_preflight_accepts_uvlo_override() {
        let mut profile_gate = json!({"ok": true});
        let settings = json!({
            "advanced_power": {
                "source_limited_vin_drop_pct": 1,
                "source_limited_enter_delta_ma": 2500,
                "source_limited_exit_delta_ma": 0,
                "source_limited_required_samples": 2,
                "source_limited_recover_margin_mv": 400,
                "vin_drop_threshold_pct": 4,
                "input_uvlo_cutoff_mv": 11_400,
                "input_uvlo_recover_mv": 11_600,
                "input_uvlo_required_samples": 3,
            }
        });
        validate_suite_settings(
            OutputProfile::V12,
            SuiteContract::SourceLimited12v,
            &settings,
            &mut profile_gate,
            SourceLimitedUvloExpectation {
                cutoff_mv: 11_400,
                recover_mv: 11_600,
                required_samples: 3,
            },
        )
        .unwrap();
        assert_eq!(
            profile_gate.pointer("/source_limited_settings/expected/input_uvlo_cutoff_mv"),
            Some(&json!(11_400))
        );
        assert_eq!(
            profile_gate.pointer("/source_limited_settings/expected/input_uvlo_recover_mv"),
            Some(&json!(11_600))
        );
    }

    #[test]
    fn source_limited_settings_preflight_requires_the_19v_bench_defaults() {
        let mut profile_gate = json!({"ok": true});
        let settings = json!({
            "advanced_power": {
                "source_limited_vin_drop_pct": 1,
                "source_limited_enter_delta_ma": 1000,
                "source_limited_exit_delta_ma": 0,
                "source_limited_required_samples": 2,
                "source_limited_recover_margin_mv": 400,
                "vin_drop_threshold_pct": 4,
                "input_uvlo_cutoff_mv": 18_200,
                "input_uvlo_recover_mv": 18_400,
                "input_uvlo_required_samples": 3,
            }
        });
        validate_suite_settings(
            OutputProfile::V19,
            SuiteContract::SourceLimited19v,
            &settings,
            &mut profile_gate,
            SourceLimitedUvloExpectation::for_profile(OutputProfile::V19),
        )
        .unwrap();
        assert_eq!(
            profile_gate.pointer("/source_limited_settings/expected/source_limited_enter_delta_ma"),
            Some(&json!(1000))
        );
        assert_eq!(
            profile_gate.pointer("/source_limited_settings/expected/input_uvlo_cutoff_mv"),
            Some(&json!(18_200))
        );
    }

    #[test]
    fn source_limited_online_assertions_require_fast_rated_handoff_and_stable_load() {
        let samples = vec![
            scene_sample(
                0.0,
                "transition_load",
                "assist_low",
                "supplement",
                None,
                11_400,
            ),
            scene_sample(
                0.2,
                "hold",
                "backup",
                "backup",
                Some("source_limited"),
                11_100,
            ),
            scene_sample(
                0.3,
                "hold",
                "backup",
                "backup",
                Some("source_limited"),
                11_100,
            ),
            scene_sample(
                0.4,
                "hold",
                "backup",
                "backup",
                Some("source_limited"),
                11_050,
            ),
        ];

        let mut samples = samples;
        classify_load_acceptance_phases(SceneKind::SourceLimitedOnline, &mut samples);

        let assertions =
            evaluate_scene_assertions(SceneKind::SourceLimitedOnline, OutputProfile::V12, &samples);
        assert_eq!(assertions.get("passed"), Some(&json!(true)));
        assert_eq!(
            assertions.pointer("/source_limited/entry_delay_s"),
            Some(&json!(0.2))
        );
        assert_eq!(
            assertions.pointer("/source_limited/post_latch_min_load_mv"),
            Some(&json!(11_050))
        );
    }

    #[test]
    fn source_in_budget_assertions_require_online_non_backup_operation() {
        let mut samples = vec![
            scene_sample(0.0, "transition_load", "standby", "standby", None, 11_700),
            scene_sample(0.2, "hold", "standby", "standby", None, 11_700),
            scene_sample(0.4, "hold", "standby", "standby", None, 11_700),
        ];
        for sample in &mut samples {
            sample.load_target_i_ma = 2_900;
            sample.load_i_total_ma = Some(json!(2_900));
            sample.tps_total_iout_ma = Some(json!(32));
        }

        let assertions =
            evaluate_scene_assertions(SceneKind::SourceInBudget, OutputProfile::V12, &samples);
        assert_eq!(assertions.get("passed"), Some(&json!(true)));
        assert_eq!(
            assertions.pointer("/in_budget_guard/backup_samples"),
            Some(&json!(0))
        );
        assert_eq!(
            assertions.pointer("/in_budget_guard/applied_samples"),
            Some(&json!(3))
        );
    }

    #[test]
    fn source_in_budget_allows_non_backup_supplement_power() {
        let mut sample = scene_sample(0.2, "hold", "supplement", "online", None, 11_700);
        sample.load_target_i_ma = 2_900;
        sample.load_i_total_ma = Some(json!(2_900));
        sample.tps_total_iout_ma = Some(json!(500));
        sample.out_a_vbus_mv = Some(json!(12_000));
        sample.out_a_iout_ma = Some(json!(500));

        let assertions =
            evaluate_scene_assertions(SceneKind::SourceInBudget, OutputProfile::V12, &[sample]);
        assert_eq!(assertions.get("passed"), Some(&json!(true)));
        assert_eq!(
            assertions.pointer("/hold_tps_power/maximum_mw"),
            Some(&json!(6_000))
        );
    }

    #[test]
    fn source_in_budget_assertions_reject_any_backup_signal() {
        let mut sample = scene_sample(0.2, "hold", "standby", "backup", None, 11_700);
        sample.load_target_i_ma = 2_900;
        sample.load_i_total_ma = Some(json!(2_900));

        let assertions =
            evaluate_scene_assertions(SceneKind::SourceInBudget, OutputProfile::V12, &[sample]);
        assert_eq!(assertions.get("passed"), Some(&json!(false)));
        assert!(assertions
            .get("failures")
            .and_then(Value::as_array)
            .is_some_and(|failures| failures.contains(&json!("source_in_budget_entered_backup"))));
    }

    #[test]
    fn load_total_current_uses_terminal_power_and_voltage() {
        assert_eq!(
            load_total_current_from_status(&json!({
                "v_local_mv": 11_684,
                "calc_p_mw": 45_567,
                "i_local_ma": 1_949,
                "i_remote_ma": 1_949,
            })),
            Some(3_899)
        );
    }

    #[test]
    fn backup_only_transition_starts_on_first_live_cut_effect() {
        let mut samples = vec![
            scene_sample(0.0, "hold", "standby", "standby", None, 18_320),
            scene_sample(0.1, "transition_backup", "standby", "standby", None, 18_320),
            scene_sample(0.2, "transition_backup", "standby", "standby", None, 18_320),
            scene_sample(
                0.3,
                "transition_backup",
                "backup",
                "backup",
                Some("input_absent"),
                18_060,
            ),
            scene_sample(
                0.4,
                "transition_backup",
                "backup",
                "backup",
                Some("input_absent"),
                18_072,
            ),
            scene_sample(
                0.5,
                "transition_backup",
                "backup",
                "backup",
                Some("input_absent"),
                18_450,
            ),
            scene_sample(
                0.6,
                "backup",
                "backup",
                "backup",
                Some("input_absent"),
                18_980,
            ),
        ];
        samples[0].vin_vbus_mv = Some(json!(19_000));
        samples[0].mains_present = Some(json!(true));
        samples[1].vin_vbus_mv = Some(json!(19_000));
        samples[1].mains_present = Some(json!(true));
        samples[2].vin_vbus_mv = Some(json!(19_000));
        samples[2].mains_present = Some(json!(true));
        samples[3].vin_vbus_mv = Some(json!(3_016));
        samples[3].mains_present = Some(json!(true));
        samples[4].vin_vbus_mv = Some(json!(3_016));
        samples[4].mains_present = Some(json!(true));
        samples[5].vin_vbus_mv = Some(json!(2_024));
        samples[5].mains_present = Some(json!(false));
        samples[6].vin_vbus_mv = Some(json!(2_024));
        samples[6].mains_present = Some(json!(false));

        classify_load_acceptance_phases(SceneKind::BackupOnly, &mut samples);

        let phases = samples
            .iter()
            .map(|sample| sample.phase.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            phases,
            vec![
                "hold",
                "hold",
                "hold",
                "transition_backup",
                "backup",
                "backup",
                "backup",
            ]
        );
    }

    #[test]
    fn source_limited_cut_assertions_require_input_absent_and_continuous_backup() {
        let samples = vec![
            scene_sample(
                0.0,
                "transition_load",
                "assist_low",
                "supplement",
                None,
                11_400,
            ),
            scene_sample(
                0.2,
                "hold",
                "backup",
                "backup",
                Some("source_limited"),
                11_100,
            ),
            scene_sample(
                0.3,
                "hold",
                "backup",
                "backup",
                Some("source_limited"),
                11_100,
            ),
            scene_sample(
                0.4,
                "transition_backup",
                "backup",
                "backup",
                Some("source_limited"),
                11_100,
            ),
            scene_sample(
                0.6,
                "backup",
                "backup",
                "backup",
                Some("input_absent"),
                11_050,
            ),
        ];

        let mut samples = samples;
        classify_load_acceptance_phases(SceneKind::SourceLimitedCut, &mut samples);

        let assertions =
            evaluate_scene_assertions(SceneKind::SourceLimitedCut, OutputProfile::V12, &samples);
        assert_eq!(assertions.get("passed"), Some(&json!(true)));
        assert_eq!(
            assertions.pointer("/backup_cut/input_absent_observed"),
            Some(&json!(true))
        );
        assert_eq!(
            assertions.pointer("/backup_cut/backup_continuous"),
            Some(&json!(true))
        );
    }

    #[test]
    fn source_limited_report_verifier_requires_four_reports() {
        let temp = tempfile::tempdir().unwrap();
        let summary = temp.path().join("suite-summary.json");
        write_json_value(
            &summary,
            &json!({
                "suite_id": "source-limited",
                "suite_contract": "source_limited_12v",
                "reports": [],
            }),
        )
        .unwrap();

        let verification = verify_report(summary, false).unwrap();
        assert_eq!(verification.get("signoff_valid"), Some(&json!(false)));
        assert!(verification
            .get("suite_failures")
            .and_then(Value::as_array)
            .is_some_and(|failures| failures.iter().any(|failure| {
                failure.get("suite_failure") == Some(&json!("unexpected_report_count"))
                    && failure.get("expected") == Some(&json!(4))
            })));
    }

    #[test]
    fn source_limited_contract_serializes_with_verifier_key() {
        assert_eq!(
            serde_json::to_value(SuiteContract::SourceLimited12v).unwrap(),
            json!("source_limited_12v")
        );
        assert_eq!(
            serde_json::to_value(SuiteContract::SourceLimited19v).unwrap(),
            json!("source_limited_19v")
        );
    }

    #[test]
    fn backup_only_scene_contract_still_requires_backup_transition() {
        assert!(SceneKind::AssistPath.include_backup());
        assert!(SceneKind::BackupOnly.include_backup());
    }

    #[test]
    fn source_disconnect_gate_requires_explicit_mains_present_false() {
        let power = json!({});
        let status = json!({
            "input": {
                "mains_present": true,
                "source": "battery",
                "vin_vbus_mv": 0,
                "assist_power_stage": "backup"
            },
            "mode": "backup"
        });
        let state = source_disconnect_state(&power, &status);
        assert!(state.ups_still_live);

        let disconnected_status = json!({
            "input": {
                "mains_present": false,
                "source": "battery",
                "vin_vbus_mv": 2999,
                "assist_power_stage": "backup"
            }
        });
        let disconnected = source_disconnect_state(&power, &disconnected_status);
        assert!(!disconnected.ups_still_live);
    }

    #[test]
    fn source_disconnect_gate_treats_sub_8v_residual_dcin_as_live_without_backup_truth() {
        let power = json!({});
        let partial_feed_status = json!({
            "input": {
                "mains_present": false,
                "source": "battery",
                "vin_vbus_mv": 5100,
                "assist_power_stage": "assist_low"
            },
            "mode": "supplement"
        });
        let state = source_disconnect_state(&power, &partial_feed_status);
        assert!(state.ups_still_live);
    }

    #[test]
    fn source_disconnect_gate_requires_backup_truth_even_when_vin_is_low() {
        let power = json!({});
        let low_vin_without_backup = json!({
            "input": {
                "mains_present": false,
                "source": "battery",
                "vin_vbus_mv": 2500,
                "assist_power_stage": "assist_low"
            },
            "mode": "standby"
        });
        let state = source_disconnect_state(&power, &low_vin_without_backup);
        assert!(state.ups_still_live);
    }

    #[test]
    fn source_disconnect_gate_accepts_firmware_cutoff_with_pre_tps_residual_voltage() {
        let power = json!({});
        let status = json!({
            "input": {
                "mains_present": false,
                "source": "usbc",
                "pre_tps_vin_mv": 3624,
                "vin_vbus_mv": 3624,
                "input_gate_state": "cutoff",
                "input_gate_reason": "pre_tps_undervoltage",
                "assist_power_stage": "backup"
            },
            "mode": "backup"
        });
        let state = source_disconnect_state(&power, &status);
        assert!(!state.ups_still_live);
    }

    #[test]
    fn compose_scene_report_entry_reads_existing_results_directory() {
        let temp = tempfile::tempdir().unwrap();
        let suite_dir = temp.path().join("suite");
        let scene_dir = suite_dir.join("12v-assist_path-3900ma");
        fs::create_dir_all(&scene_dir).unwrap();
        let results = json!({
            "metadata": {
                "output_profile": "12v",
                "scene_type": "assist_path",
                "target_ma": 3900,
                "include_backup": true,
                "source_voltage_mv": 12000,
                "source_current_limit_ma": 3000,
                "load_min_v_mv": 3000,
                "max_i_ma_total": 4000,
                "max_p_mw": 80000,
                "ups_transport": "mains-aegis CLI + native IPC + USB",
                "power_transport": "Isolapurr:cli+url",
                "load_transport": "Loadlynx:cli+ipc+usb"
            },
            "settings_snapshot": {
                "advanced_power": {
                    "standby_drop_mv": 1200
                }
            },
            "summary": {
                "all": {
                    "completeness": {
                        "scene_complete": true,
                        "failures": [],
                        "effective_sample_rate_hz": 4.2,
                        "max_sample_gap_s": 0.25
                    },
                    "acceptance": {
                        "run_validity": "valid_for_signoff",
                        "signoff_valid": true,
                        "failed_acceptance_checks": []
                    }
                }
            },
            "samples": [{}, {}]
        });
        write_json_value(&scene_dir.join("results.json"), &results).unwrap();
        let entry = compose_scene_report_entry(&suite_dir, &scene_dir).unwrap();
        assert_eq!(entry["report_dir"], "12v-assist_path-3900ma");
        assert_eq!(entry["output_profile"], "12v");
        assert_eq!(entry["scene_type"], "assist_path");
        assert_eq!(entry["target_ma"], 3900);
        assert_eq!(entry["signoff_valid"], true);
        assert_eq!(entry["advanced_power"]["standby_drop_mv"], 1200);
    }

    #[test]
    fn compose_scene_report_entry_uses_relative_path_from_suite_dir() {
        let temp = tempfile::tempdir().unwrap();
        let source_root = temp.path().join("raw");
        let suite_dir = temp.path().join("composed");
        let scene_dir = source_root.join("12v-backup_only-1000ma");
        fs::create_dir_all(&scene_dir).unwrap();
        fs::create_dir_all(&suite_dir).unwrap();
        let results = json!({
            "metadata": {
                "output_profile": "12v",
                "scene_type": "backup_only",
                "target_ma": 1000
            },
            "summary": {
                "all": {
                    "completeness": {
                        "scene_complete": true,
                        "failures": [],
                        "effective_sample_rate_hz": 4.0,
                        "max_sample_gap_s": 0.25
                    },
                    "acceptance": {
                        "signoff_valid": true,
                        "failed_acceptance_checks": []
                    }
                }
            },
            "samples": [{}, {}]
        });
        write_json_value(&scene_dir.join("results.json"), &results).unwrap();
        let entry = compose_scene_report_entry(&suite_dir, &scene_dir).unwrap();
        assert_eq!(entry["report_dir"], "../raw/12v-backup_only-1000ma");
    }

    #[test]
    fn suite_overview_preserves_nested_report_dir_href() {
        let temp = tempfile::tempdir().unwrap();
        let overview = temp.path().join("suite-overview.html");
        let suite = json!({
            "suite_id": "suite",
            "reports": [{
                "report_dir": "../raw/12v-backup_only-1000ma",
                "output_profile": "12v",
                "scene_type": "backup_only",
                "signoff_valid": true,
                "effective_sample_rate_hz": 4.0,
                "max_sample_gap_s": 0.25,
                "failed_acceptance_checks": []
            }]
        });
        write_suite_overview(&overview, &suite).unwrap();
        let html = fs::read_to_string(overview).unwrap();
        assert!(html.contains("src=\"../raw/12v-backup_only-1000ma/voltage-chart.html?embed=1\""));
        assert!(html.contains("href=\"../raw/12v-backup_only-1000ma/results.json\""));
    }

    #[test]
    fn validate_scene_report_recomputes_timeseries_summary() {
        let temp = tempfile::tempdir().unwrap();
        let suite_dir = temp.path().join("suite");
        let report_dir = suite_dir.join("12v-assist_path-3900ma");
        fs::create_dir_all(&report_dir).unwrap();

        let results = json!({
            "metadata": {
                "output_profile": "12v",
                "scene_type": "assist_path",
                "target_ma": 3900
            },
            "summary": {
                "all": {
                    "completeness": {
                        "scene_complete": true,
                        "failures": [],
                        "effective_sample_rate_hz": 4.2,
                        "max_sample_gap_s": 0.25,
                        "required_voltage_series": {
                            "source_output_voltage": true,
                            "ups_dcin_voltage": true,
                            "ups_output_voltage": true,
                            "load_actual_voltage": true
                        }
                    },
                    "acceptance": {
                        "run_validity": "valid_for_signoff",
                        "signoff_valid": true,
                        "failed_acceptance_checks": []
                    }
                }
            },
            "samples": [{}, {}, {}]
        });
        write_json_value(&report_dir.join("results.json"), &results).unwrap();
        fs::write(
            report_dir.join("timeseries.jsonl"),
            [
                serde_json::to_string(&SceneSample {
                    t_s: 0.0,
                    unix_ms: 0,
                    phase: "hold".to_string(),
                    stage: None,
                    mode: None,
                    backup_reason: None,
                    load_target_i_ma: 3900,
                    ups_status_cache_age_ms: None,
                    ups_status_cache_fresh: None,
                    ups_status_monitor_running: None,
                    port_c_enabled: None,
                    isolapurr_port_c_mv: Some(json!(12000)),
                    isolapurr_port_c_ma: None,
                    mains_present: None,
                    assist_target_vout_mv: None,
                    vin_vbus_mv: Some(json!(11800)),
                    vin_iin_ma: None,
                    tps_total_iout_ma: None,
                    battery_current_ma: None,
                    charger_state: None,
                    charger_allow_charge: None,
                    out_a_vbus_mv: Some(json!(10800)),
                    out_b_vbus_mv: None,
                    out_a_iout_ma: None,
                    out_b_iout_ma: None,
                    diag_stage: None,
                    diag_backup_reason: None,
                    diag_charger_notice: None,
                    diag_assist_target_vout_mv: None,
                    diag_vin_baseline_mv: None,
                    diag_vin_drop_mv: None,
                    diag_tps_total_iout_ma: None,
                    load_output_enabled: None,
                    load_v_local_mv: None,
                    load_i_total_ma: None,
                })
                .unwrap(),
                serde_json::to_string(&SceneSample {
                    t_s: 1.2,
                    unix_ms: 1200,
                    phase: "hold".to_string(),
                    stage: None,
                    mode: None,
                    backup_reason: None,
                    load_target_i_ma: 3900,
                    ups_status_cache_age_ms: None,
                    ups_status_cache_fresh: None,
                    ups_status_monitor_running: None,
                    port_c_enabled: None,
                    isolapurr_port_c_mv: Some(json!(12010)),
                    isolapurr_port_c_ma: None,
                    mains_present: None,
                    assist_target_vout_mv: None,
                    vin_vbus_mv: Some(json!(11810)),
                    vin_iin_ma: None,
                    tps_total_iout_ma: None,
                    battery_current_ma: None,
                    charger_state: None,
                    charger_allow_charge: None,
                    out_a_vbus_mv: Some(json!(10810)),
                    out_b_vbus_mv: None,
                    out_a_iout_ma: None,
                    out_b_iout_ma: None,
                    diag_stage: None,
                    diag_backup_reason: None,
                    diag_charger_notice: None,
                    diag_assist_target_vout_mv: None,
                    diag_vin_baseline_mv: None,
                    diag_vin_drop_mv: None,
                    diag_tps_total_iout_ma: None,
                    load_output_enabled: None,
                    load_v_local_mv: None,
                    load_i_total_ma: None,
                })
                .unwrap(),
                serde_json::to_string(&SceneSample {
                    t_s: 2.4,
                    unix_ms: 2400,
                    phase: "hold".to_string(),
                    stage: None,
                    mode: None,
                    backup_reason: None,
                    load_target_i_ma: 3900,
                    ups_status_cache_age_ms: None,
                    ups_status_cache_fresh: None,
                    ups_status_monitor_running: None,
                    port_c_enabled: None,
                    isolapurr_port_c_mv: Some(json!(12020)),
                    isolapurr_port_c_ma: None,
                    mains_present: None,
                    assist_target_vout_mv: None,
                    vin_vbus_mv: Some(json!(11820)),
                    vin_iin_ma: None,
                    tps_total_iout_ma: None,
                    battery_current_ma: None,
                    charger_state: None,
                    charger_allow_charge: None,
                    out_a_vbus_mv: Some(json!(10820)),
                    out_b_vbus_mv: None,
                    out_a_iout_ma: None,
                    out_b_iout_ma: None,
                    diag_stage: None,
                    diag_backup_reason: None,
                    diag_charger_notice: None,
                    diag_assist_target_vout_mv: None,
                    diag_vin_baseline_mv: None,
                    diag_vin_drop_mv: None,
                    diag_tps_total_iout_ma: None,
                    load_output_enabled: None,
                    load_v_local_mv: None,
                    load_i_total_ma: None,
                })
                .unwrap(),
            ]
            .join("\n")
                + "\n",
        )
        .unwrap();
        fs::write(report_dir.join("voltage-chart.html"), "<html></html>").unwrap();

        let report = json!({
            "report_dir": "12v-assist_path-3900ma",
            "output_profile": "12v",
            "scene_type": "assist_path",
            "target_ma": 3900,
            "signoff_valid": true
        });
        let failures = validate_scene_report(&suite_dir, &report, SuiteContract::Standard).unwrap();
        assert!(failures.iter().any(|failure| {
            failure.get("report_failure")
                == Some(&json!("timeseries_missing_required_voltage_series"))
        }));
        assert!(failures.iter().any(|failure| {
            failure.get("report_failure") == Some(&json!("timeseries_sample_rate_below_2hz"))
        }));
        assert!(failures.iter().any(|failure| {
            failure.get("report_failure") == Some(&json!("timeseries_sample_gap_above_0_5s"))
        }));
    }
}
