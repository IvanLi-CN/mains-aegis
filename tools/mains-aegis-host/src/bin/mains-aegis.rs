use clap::{Args, Parser, Subcommand};
use mains_aegis_host::{
    default_ipc_endpoint, ipc_call, release_version, serve_http_service, serve_ipc,
    HttpServiceConfig, IpcConfig, DEFAULT_BIND,
};
use serde_json::{json, Value};
use std::{
    io::{self, IsTerminal, Write},
    net::SocketAddr,
    path::PathBuf,
    process::Stdio,
};
use tokio::time::{sleep, Duration};

mod mains_aegis;
use mains_aegis::power_validation::{PowerValidationArgs, PowerValidationCommand};

const DEFAULT_WATCH_FRESHNESS_MS: u64 = 750;

#[derive(Debug, Parser)]
#[command(name = "mains-aegis")]
#[command(version = release_version())]
#[command(about = "Mains Aegis host CLI")]
struct Cli {
    /// IPC socket or named-pipe endpoint.
    #[arg(long, global = true, env = "MAINS_AEGIS_DEVD_IPC")]
    ipc: Option<String>,
    /// Do not auto-start the local IPC daemon when it is not reachable.
    #[arg(long, global = true)]
    no_auto_start: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Daemon {
        #[command(subcommand)]
        command: DaemonCommand,
    },
    Health,
    Devices {
        #[command(subcommand)]
        command: DevicesCommand,
    },
    Device {
        device_id: String,
        #[command(subcommand)]
        command: DeviceCommand,
    },
    Serial {
        #[command(subcommand)]
        command: SerialCommand,
    },
    Host {
        #[command(subcommand)]
        command: HostCommand,
    },
    Settings {
        #[command(subcommand)]
        command: SettingsCommand,
    },
    PowerValidation {
        #[command(subcommand)]
        command: PowerValidationCommand,
    },
}

#[derive(Debug, Subcommand)]
enum DaemonCommand {
    /// Run the IPC daemon in the foreground. Intended for development/debugging.
    Serve {
        /// Exit after this many idle seconds. Use 0 to disable idle shutdown.
        #[arg(long, default_value_t = mains_aegis_host::DEFAULT_IPC_IDLE_TIMEOUT_SECS)]
        idle_timeout_secs: u64,
        /// Allow real host power profile/suspend/shutdown actions.
        #[arg(long, env = "MAINS_AEGIS_DEVD_ALLOW_HOST_POWER_ACTIONS")]
        allow_host_power_actions: bool,
    },
    /// Run the explicit HTTP API / hosted Web service in the foreground.
    Http {
        /// HTTP bind address.
        #[arg(long, default_value = DEFAULT_BIND, env = "MAINS_AEGIS_DEVD_BIND")]
        bind: SocketAddr,
        /// Allow local development CORS origins.
        #[arg(long, env = "MAINS_AEGIS_DEVD_ALLOW_DEV_CORS")]
        allow_dev_cors: bool,
        /// Allow real host power profile/suspend/shutdown actions.
        #[arg(long, env = "MAINS_AEGIS_DEVD_ALLOW_HOST_POWER_ACTIONS")]
        allow_host_power_actions: bool,
        /// Permit a non-loopback bridge bind when paired with an auth token file.
        #[arg(long, env = "MAINS_AEGIS_DEVD_ALLOW_LAN_BRIDGE")]
        allow_lan_bridge: bool,
        /// File containing the bearer token required for LAN bridge mode.
        #[arg(long, env = "MAINS_AEGIS_DEVD_AUTH_TOKEN_FILE")]
        auth_token_file: Option<PathBuf>,
        /// Open the hosted app in the default browser after the service starts.
        #[arg(long, env = "MAINS_AEGIS_DEVD_OPEN_BROWSER")]
        open_browser: bool,
    },
}

#[derive(Debug, Subcommand)]
enum DevicesCommand {
    List,
    Scan(DevicesScanArgs),
    ScanTrace {
        #[arg(long)]
        trace_limit: Option<usize>,
    },
}

#[derive(Debug, Args)]
struct DevicesScanArgs {
    /// IPv4 CIDR to scan after mDNS/DNS-SD discovery, for example 192.168.1.0/24.
    #[arg(long)]
    cidr: Option<String>,
    /// Skip LAN discovery and scan only local USB candidates.
    #[arg(long)]
    no_lan: bool,
    /// Skip mDNS/DNS-SD discovery and only use the CIDR/default subnet path.
    #[arg(long)]
    no_mdns: bool,
}

#[derive(Debug, Subcommand)]
enum DeviceCommand {
    Bind {
        #[arg(long)]
        alias: Option<String>,
    },
    CompanionLan {
        #[command(subcommand)]
        command: CompanionLanCommand,
    },
    Unbind,
    Connect,
    Disconnect,
    Connection,
    Identity,
    Status(DeviceReadArgs),
    DiagSnapshot(DiagSnapshotArgs),
    Recovery {
        #[command(subcommand)]
        command: RecoveryCommand,
    },
    Settings,
    OutputBypass {
        #[arg(long, conflicts_with = "restore")]
        enable: bool,
        #[arg(long, conflicts_with = "enable")]
        restore: bool,
    },
    Trace(TraceArgs),
    Artifact {
        #[command(subcommand)]
        command: ArtifactCommand,
    },
    Flash {
        #[arg(long)]
        artifact_id: Option<String>,
        #[arg(long, conflicts_with = "real")]
        dry_run: bool,
        #[arg(long, conflicts_with = "dry_run")]
        real: bool,
    },
    Reset,
    Monitor {
        #[command(subcommand)]
        command: MonitorCommand,
    },
}

#[derive(Debug, Subcommand)]
enum RecoveryCommand {
    BmsDischargeAuthorization,
}

#[derive(Debug, Args)]
struct TraceArgs {
    #[arg(long)]
    logs_limit: Option<usize>,
    #[arg(long)]
    trace_limit: Option<usize>,
    #[arg(long)]
    lease_id: Option<String>,
    #[arg(long)]
    follow: bool,
    #[arg(long)]
    kind: Option<String>,
}

#[derive(Debug, Args, Clone)]
struct DeviceReadArgs {
    /// Bypass the devd monitor cache and request a fresh device read.
    #[arg(long, conflicts_with = "cache_only")]
    fresh: bool,
    /// Return only the current devd monitor cache instead of issuing a CDC request.
    #[arg(long)]
    cache_only: bool,
    /// Include cache age and monitor metadata with the returned sample.
    #[arg(long)]
    include_meta: bool,
    /// Continuously emit newline-delimited JSON samples.
    #[arg(long)]
    watch: bool,
    /// Watch interval in milliseconds.
    #[arg(long, default_value_t = 333)]
    interval_ms: u64,
    /// Override the cache freshness budget used for watch mode.
    #[arg(long)]
    watch_freshness_ms: Option<u64>,
    /// Allow watch mode to emit stale cache samples instead of failing freshness.
    #[arg(long)]
    allow_stale_cache: bool,
    /// Stop after this many watch samples. Omit to run until interrupted.
    #[arg(long)]
    samples: Option<u64>,
}

#[derive(Debug, Args, Clone)]
struct DiagSnapshotArgs {
    #[command(flatten)]
    read: DeviceReadArgs,
    /// Diagnostic package id to include. Repeat for multiple packages.
    #[arg(long = "package")]
    packages: Vec<String>,
}

#[derive(Debug, Subcommand)]
enum ArtifactCommand {
    Get,
    Select {
        #[arg(long)]
        manifest_path: Option<String>,
        #[arg(long)]
        artifact_id: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum MonitorCommand {
    Start,
    Stop,
}

#[derive(Debug, Subcommand)]
enum CompanionLanCommand {
    Bind {
        #[arg(long)]
        mdns_host: Option<String>,
        #[arg(long)]
        ip: Option<String>,
        #[arg(long)]
        port: Option<u16>,
    },
    Clear,
}

#[derive(Debug, Subcommand)]
enum SerialCommand {
    Lease {
        #[command(subcommand)]
        command: LeaseCommand,
    },
}

#[derive(Debug, Subcommand)]
enum LeaseCommand {
    Create { device_id: String },
    Heartbeat { lease_id: String },
    Release { lease_id: String },
}

#[derive(Debug, Subcommand)]
enum HostCommand {
    Power {
        #[command(subcommand)]
        command: HostPowerCommand,
    },
}

#[derive(Debug, Subcommand)]
enum HostPowerCommand {
    Status,
    Profile {
        profile: String,
        #[arg(long, conflicts_with = "real")]
        dry_run: bool,
        #[arg(long, conflicts_with = "dry_run")]
        real: bool,
    },
    Suspend {
        #[arg(long, conflicts_with = "real")]
        dry_run: bool,
        #[arg(long, conflicts_with = "dry_run")]
        real: bool,
    },
    Shutdown {
        #[arg(long)]
        delay_sec: Option<u64>,
        #[arg(long, conflicts_with = "real")]
        dry_run: bool,
        #[arg(long, conflicts_with = "dry_run")]
        real: bool,
        #[arg(long)]
        confirm: Option<String>,
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Subcommand)]
enum SettingsCommand {
    Wifi {
        #[command(subcommand)]
        command: WifiCommand,
    },
    LogLevel {
        level: String,
        #[arg(long)]
        device_id: Option<String>,
        #[arg(long)]
        lease_id: Option<String>,
    },
    ManualCharge {
        target: String,
        speed: String,
        timer_h: u8,
        #[arg(long)]
        device_id: Option<String>,
        #[arg(long)]
        lease_id: Option<String>,
    },
    AdvancedPower {
        standby_drop_mv: u16,
        assist_low_drop_mv: u16,
        assist_enter_delta_ma: i16,
        assist_exit_delta_ma: i16,
        assist_required_samples: u8,
        assist_ramp_step_mv: u16,
        assist_ramp_interval_ms: u16,
        rated_enter_delta_ma: i16,
        rated_exit_delta_ma: i16,
        vin_drop_threshold_pct: u8,
        required_samples: u8,
        source_limited_vin_drop_pct: u8,
        source_limited_enter_delta_ma: i16,
        source_limited_exit_delta_ma: i16,
        source_limited_required_samples: u8,
        source_limited_recover_margin_mv: u16,
        #[arg(long)]
        device_id: Option<String>,
        #[arg(long)]
        lease_id: Option<String>,
    },
    AdvancedPowerReset {
        #[arg(long)]
        device_id: Option<String>,
        #[arg(long)]
        lease_id: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum WifiCommand {
    Set {
        ssid: String,
        #[arg(long)]
        psk: String,
        #[arg(long)]
        device_id: Option<String>,
        #[arg(long)]
        lease_id: Option<String>,
    },
    Clear {
        #[arg(long)]
        device_id: Option<String>,
        #[arg(long)]
        lease_id: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let endpoint = cli.ipc.unwrap_or_else(default_ipc_endpoint);
    let devd = DevdClient {
        endpoint,
        auto_start: !cli.no_auto_start,
    };
    match cli.command {
        Command::Daemon { command } => run_daemon_command(&devd.endpoint, command).await?,
        Command::PowerValidation { command } => {
            mains_aegis::power_validation::run(
                command,
                PowerValidationArgs {
                    ups_ipc: devd.endpoint,
                    no_auto_start: cli.no_auto_start,
                },
            )
            .await?;
        }
        Command::Device {
            device_id,
            command: DeviceCommand::Trace(args),
        } if args.follow => {
            follow_device_trace(&devd, &device_id, args).await?;
        }
        Command::Device {
            device_id,
            command: DeviceCommand::Status(args),
        } if args.watch => {
            watch_device_read(&devd, &device_id, "status", "device.status", args, None).await?;
        }
        Command::Device {
            device_id,
            command: DeviceCommand::DiagSnapshot(args),
        } if args.read.watch => {
            watch_device_read(
                &devd,
                &device_id,
                "diag_snapshot",
                "device.diag_snapshot",
                args.read,
                Some(args.packages),
            )
            .await?;
        }
        command => {
            let interactive_bind = match &command {
                Command::Device {
                    device_id,
                    command: DeviceCommand::Bind { alias },
                } => Some((device_id.clone(), alias.clone())),
                _ => None,
            };
            let (method, params) = command_to_ipc(command);
            let mut result = devd_ipc_call(&devd, method, params).await?;
            if let Some((device_id, _alias)) = interactive_bind {
                if io::stdin().is_terminal()
                    && io::stdout().is_terminal()
                    && maybe_confirm_companion_lan(&devd, &device_id, &result).await?
                {
                    result = devd_ipc_call(
                        &devd,
                        "device.connection",
                        json!({ "device_id": device_id }),
                    )
                    .await?;
                }
            }
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
    }
    Ok(())
}

async fn run_daemon_command(endpoint: &str, command: DaemonCommand) -> anyhow::Result<()> {
    init_daemon_tracing();
    match command {
        DaemonCommand::Serve {
            idle_timeout_secs,
            allow_host_power_actions,
        } => {
            eprintln!("mains-aegis daemon IPC endpoint: {endpoint}");
            let idle_timeout =
                (idle_timeout_secs > 0).then(|| Duration::from_secs(idle_timeout_secs));
            serve_ipc(
                IpcConfig::new(endpoint.to_string())
                    .with_idle_timeout(idle_timeout)
                    .with_host_power_actions(allow_host_power_actions),
            )
            .await
        }
        DaemonCommand::Http {
            bind,
            allow_dev_cors,
            allow_host_power_actions,
            allow_lan_bridge,
            auth_token_file,
            open_browser,
        } => {
            let auth_token = auth_token_file
                .map(|path| {
                    std::fs::read_to_string(&path)
                        .map(|token| token.trim().to_string())
                        .map_err(|error| {
                            anyhow::anyhow!("read auth token file {}: {error}", path.display())
                        })
                })
                .transpose()?;
            eprintln!("mains-aegis daemon HTTP listening on http://{bind}");
            eprintln!("mains-aegis daemon HTTP IPC endpoint: {endpoint}");
            serve_http_service(HttpServiceConfig {
                ipc_endpoint: endpoint.to_string(),
                bind,
                allow_dev_cors,
                allow_host_power_actions,
                allow_lan_bridge,
                auth_token,
                open_browser,
            })
            .await
        }
    }
}

fn init_daemon_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mains_aegis_host=info".into()),
        )
        .try_init();
}

#[derive(Debug, Clone)]
struct DevdClient {
    endpoint: String,
    auto_start: bool,
}

async fn devd_ipc_call(devd: &DevdClient, method: &str, params: Value) -> anyhow::Result<Value> {
    match ipc_call(&devd.endpoint, method, params.clone()).await {
        Ok(value) => Ok(value),
        Err(error) if devd.auto_start && looks_like_ipc_connect_error(&error) => {
            start_devd(&devd.endpoint)?;
            wait_for_devd_health(&devd.endpoint).await?;
            ipc_call(&devd.endpoint, method, params).await
        }
        Err(error) => Err(error),
    }
}

fn looks_like_ipc_connect_error(error: &anyhow::Error) -> bool {
    let message = error.to_string();
    message.contains("connect IPC socket") || message.contains("connect IPC pipe")
}

fn start_devd(endpoint: &str) -> anyhow::Result<()> {
    let devd_bin = std::env::var_os("MAINS_AEGIS_DEVD_BIN")
        .map(PathBuf::from)
        .or_else(|| {
            let mut path = std::env::current_exe().ok()?;
            path.set_file_name(format!("mains-aegis-devd{}", std::env::consts::EXE_SUFFIX));
            Some(path)
        })
        .ok_or_else(|| anyhow::anyhow!("cannot resolve mains-aegis-devd path"))?;
    if !devd_bin.is_file() {
        anyhow::bail!(
            "mains-aegis-devd was not found next to mains-aegis; install host tools or build both binaries"
        );
    }
    std::process::Command::new(devd_bin)
        .arg("serve")
        .arg("--ipc")
        .arg(endpoint)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| anyhow::anyhow!("start mains-aegis-devd IPC daemon: {error}"))
}

async fn wait_for_devd_health(endpoint: &str) -> anyhow::Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(4);
    let mut last_error = None;
    while tokio::time::Instant::now() < deadline {
        match ipc_call(endpoint, "devd.health", json!({})).await {
            Ok(_) => return Ok(()),
            Err(error) => {
                last_error = Some(error);
                sleep(Duration::from_millis(100)).await;
            }
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("mains-aegis-devd IPC daemon did not start")))
}

async fn watch_device_read(
    devd: &DevdClient,
    device_id: &str,
    kind: &str,
    method: &'static str,
    args: DeviceReadArgs,
    packages: Option<Vec<String>>,
) -> anyhow::Result<()> {
    let mut index = 0_u64;
    let started = std::time::Instant::now();
    let interval = Duration::from_millis(args.interval_ms);
    let watch_freshness_ms = args
        .watch_freshness_ms
        .unwrap_or(DEFAULT_WATCH_FRESHNESS_MS);
    let mut next_sample_at = tokio::time::Instant::now();
    loop {
        tokio::time::sleep_until(next_sample_at).await;
        next_sample_at += interval;
        let sampled_at_ms = started.elapsed().as_millis() as u64;
        let result = match devd_ipc_call(
            devd,
            method,
            device_read_ipc_params(device_id, &args, true, true, packages.clone()),
        )
        .await
        {
            Ok(result) => result,
            Err(error) if is_watch_retryable_cache_error(&error) => {
                let sample_received_at_ms = started.elapsed().as_millis() as u64;
                println!(
                    "{}",
                    serde_json::to_string(&json!({
                        "sample_index": index,
                        "sample_seq": index,
                        "sampled_at_ms": sampled_at_ms,
                        "sample_received_at_ms": sample_received_at_ms,
                        "kind": kind,
                        "device_id": device_id,
                        "fresh": args.fresh,
                        "cache_only": !args.fresh || args.cache_only,
                        "watch_freshness_ms": watch_freshness_ms,
                        "miss": true,
                        "error": error.to_string(),
                    }))?
                );
                io::stdout().flush()?;
                index += 1;
                if args.samples.is_some_and(|limit| index >= limit) {
                    break;
                }
                continue;
            }
            Err(error) => return Err(error),
        };
        let sample_received_at_ms = started.elapsed().as_millis() as u64;
        println!(
            "{}",
            serde_json::to_string(&json!({
                "sample_index": index,
                "sample_seq": index,
                "sampled_at_ms": sampled_at_ms,
                "sample_received_at_ms": sample_received_at_ms,
                "kind": kind,
                "device_id": device_id,
                "fresh": args.fresh,
                "cache_only": !args.fresh || args.cache_only,
                "watch_freshness_ms": watch_freshness_ms,
                "result": result,
            }))?
        );
        io::stdout().flush()?;
        index += 1;
        if args.samples.is_some_and(|limit| index >= limit) {
            break;
        }
    }
    Ok(())
}

fn is_watch_retryable_cache_error(error: &anyhow::Error) -> bool {
    let message = error.to_string();
    message.contains("device_status_cache_unavailable")
        || message.contains("device_diag_snapshot_cache_unavailable")
}

async fn follow_device_trace(
    devd: &DevdClient,
    device_id: &str,
    args: TraceArgs,
) -> anyhow::Result<()> {
    let initial = devd_ipc_call(
        devd,
        "device.trace",
        json!({
            "device_id": device_id,
            "logs_limit": args.logs_limit,
            "trace_limit": args.trace_limit,
            "lease_id": args.lease_id,
        }),
    )
    .await?;
    let mut seen_ids = initial
        .get("trace")
        .and_then(Value::as_array)
        .map(|trace| seed_seen_ids(trace))
        .unwrap_or_default();
    loop {
        let result = devd_ipc_call(
            devd,
            "device.trace",
            json!({
                "device_id": device_id,
                "logs_limit": args.logs_limit,
                "trace_limit": args.trace_limit,
                "lease_id": args.lease_id,
            }),
        )
        .await?;
        if let Some(trace) = result.get("trace").and_then(Value::as_array) {
            for entry in collect_new_matching_entries(trace, &mut seen_ids, args.kind.as_deref()) {
                println!("{}", serde_json::to_string(&entry)?);
            }
        }
        sleep(Duration::from_millis(1000)).await;
    }
}

fn seed_seen_ids(trace: &[Value]) -> std::collections::BTreeSet<String> {
    trace
        .iter()
        .filter_map(|entry| entry.get("id").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect()
}

fn collect_new_matching_entries(
    trace: &[Value],
    seen_ids: &mut std::collections::BTreeSet<String>,
    kind: Option<&str>,
) -> Vec<Value> {
    trace
        .iter()
        .filter_map(|entry| {
            let id = entry.get("id").and_then(Value::as_str)?;
            if !seen_ids.insert(id.to_string()) {
                return None;
            }
            if !trace_entry_matches_kind(entry, kind) {
                return None;
            }
            Some(entry.clone())
        })
        .collect()
}

fn trace_entry_matches_kind(entry: &Value, kind: Option<&str>) -> bool {
    match kind {
        None => true,
        Some("event") => {
            entry.get("kind").and_then(Value::as_str) == Some("event")
                && entry.get("target").and_then(Value::as_str) == Some("power")
        }
        Some(expected_kind) => entry
            .get("kind")
            .and_then(Value::as_str)
            .is_some_and(|actual| actual == expected_kind),
    }
}

fn command_to_ipc(command: Command) -> (&'static str, Value) {
    match command {
        Command::Health => ("devd.health", json!({})),
        Command::Daemon { .. } => unreachable!("handled before IPC dispatch"),
        Command::Devices { command } => match command {
            DevicesCommand::List => ("devices.list", json!({})),
            DevicesCommand::Scan(args) => (
                "devices.scan",
                json!({
                    "cidr": args.cidr,
                    "lan": !args.no_lan,
                    "mdns": !args.no_mdns,
                }),
            ),
            DevicesCommand::ScanTrace { trace_limit } => {
                ("devices.scan_trace", json!({ "trace_limit": trace_limit }))
            }
        },
        Command::Device { device_id, command } => device_to_ipc(device_id, command),
        Command::Serial { command } => match command {
            SerialCommand::Lease { command } => match command {
                LeaseCommand::Create { device_id } => {
                    ("serial.lease.create", json!({ "device_id": device_id }))
                }
                LeaseCommand::Heartbeat { lease_id } => {
                    ("serial.lease.heartbeat", json!({ "lease_id": lease_id }))
                }
                LeaseCommand::Release { lease_id } => {
                    ("serial.lease.release", json!({ "lease_id": lease_id }))
                }
            },
        },
        Command::Host { command } => match command {
            HostCommand::Power { command } => host_power_to_ipc(command),
        },
        Command::Settings { command } => settings_to_ipc(command),
        Command::PowerValidation { .. } => unreachable!("handled before IPC dispatch"),
    }
}

fn device_to_ipc(device_id: String, command: DeviceCommand) -> (&'static str, Value) {
    match command {
        DeviceCommand::Bind { alias } => (
            "device.bind",
            json!({ "device_id": device_id, "alias": alias }),
        ),
        DeviceCommand::CompanionLan { command } => match command {
            CompanionLanCommand::Bind {
                mdns_host,
                ip,
                port,
            } => (
                "device.companion_lan.bind",
                json!({
                    "device_id": device_id,
                    "mdns_host": mdns_host,
                    "ip": ip,
                    "port": port,
                }),
            ),
            CompanionLanCommand::Clear => (
                "device.companion_lan.clear",
                json!({ "device_id": device_id }),
            ),
        },
        DeviceCommand::Unbind => ("device.unbind", json!({ "device_id": device_id })),
        DeviceCommand::Connect => ("device.connect", json!({ "device_id": device_id })),
        DeviceCommand::Disconnect => ("device.disconnect", json!({ "device_id": device_id })),
        DeviceCommand::Connection => ("device.connection", json!({ "device_id": device_id })),
        DeviceCommand::Identity => ("device.identity", json!({ "device_id": device_id })),
        DeviceCommand::Status(args) => (
            "device.status",
            device_read_ipc_params(&device_id, &args, args.include_meta, false, None),
        ),
        DeviceCommand::DiagSnapshot(args) => (
            "device.diag_snapshot",
            device_read_ipc_params(
                &device_id,
                &args.read,
                args.read.include_meta,
                false,
                Some(args.packages),
            ),
        ),
        DeviceCommand::Recovery { command } => match command {
            RecoveryCommand::BmsDischargeAuthorization => (
                "device.recovery.bms_discharge_authorization",
                json!({ "device_id": device_id }),
            ),
        },
        DeviceCommand::Settings => ("device.settings", json!({ "device_id": device_id })),
        DeviceCommand::OutputBypass { enable, restore } => (
            "device.output_bypass",
            json!({ "device_id": device_id, "enable": enable, "restore": restore }),
        ),
        DeviceCommand::Trace(args) => (
            "device.trace",
            json!({
                "device_id": device_id,
                "logs_limit": args.logs_limit,
                "trace_limit": args.trace_limit,
                "lease_id": args.lease_id,
            }),
        ),
        DeviceCommand::Artifact { command } => match command {
            ArtifactCommand::Get => ("device.artifact.get", json!({ "device_id": device_id })),
            ArtifactCommand::Select {
                manifest_path,
                artifact_id,
            } => (
                "device.artifact.select",
                json!({
                    "device_id": device_id,
                    "manifest_path": manifest_path,
                    "artifact_id": artifact_id,
                }),
            ),
        },
        DeviceCommand::Flash {
            artifact_id,
            dry_run,
            real,
        } => (
            "device.flash",
            json!({ "device_id": device_id, "artifact_id": artifact_id, "dry_run": dry_run || !real }),
        ),
        DeviceCommand::Reset => ("device.reset", json!({ "device_id": device_id })),
        DeviceCommand::Monitor { command } => match command {
            MonitorCommand::Start => ("device.monitor.start", json!({ "device_id": device_id })),
            MonitorCommand::Stop => ("device.monitor.stop", json!({ "device_id": device_id })),
        },
    }
}

fn device_read_ipc_params(
    device_id: &str,
    args: &DeviceReadArgs,
    include_meta: bool,
    watch: bool,
    packages: Option<Vec<String>>,
) -> Value {
    let mut params = json!({
        "device_id": device_id,
        "fresh": args.fresh,
        // Telemetry reads default to the devd monitor cache over IPC. A direct
        // CDC read must be explicit because it competes with the monitor owner.
        "cache_only": !args.fresh || args.cache_only,
        // Watch mode is a telemetry stream: emit the last cache snapshot on
        // schedule and mark freshness in meta instead of blocking the timeline.
        "allow_stale_cache": args.allow_stale_cache || watch || (!watch && args.cache_only),
        "include_meta": include_meta,
    });
    if watch {
        params["watch_freshness_ms"] = json!(args
            .watch_freshness_ms
            .unwrap_or(DEFAULT_WATCH_FRESHNESS_MS));
    }
    if let Some(packages) = packages {
        if !packages.is_empty() {
            params["packages"] = json!(packages);
        }
    }
    params
}

async fn maybe_confirm_companion_lan(
    devd: &DevdClient,
    device_id: &str,
    bind_result: &Value,
) -> anyhow::Result<bool> {
    let Some(candidate) = bind_result.get("companion_lan_candidate") else {
        return Ok(false);
    };
    let mdns_host = candidate
        .get("mdns_host")
        .and_then(Value::as_str)
        .unwrap_or("");
    let ip = candidate.get("ip").and_then(Value::as_str).unwrap_or("");
    let port = candidate.get("port").and_then(Value::as_u64).unwrap_or(80);
    if mdns_host.is_empty() || ip.is_empty() {
        return Ok(false);
    }
    eprintln!(
        "Detected reachable LAN companion for {device_id}: devd can use {mdns_host}, Web can use http://{ip}:{port}."
    );
    eprint!("Bind this LAN companion now? [y/N]: ");
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        eprintln!(
            "Skipped LAN companion persistence. Later command: mains-aegis device {device_id} companion-lan bind --mdns-host {mdns_host} --ip {ip} --port {port}"
        );
        return Ok(false);
    }
    let result = devd_ipc_call(
        devd,
        "device.companion_lan.bind",
        json!({
            "device_id": device_id,
            "mdns_host": mdns_host,
            "ip": ip,
            "port": port,
        }),
    )
    .await?;
    let serialized = serde_json::to_string_pretty(&result)?;
    eprintln!("{serialized}");
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::{
        collect_new_matching_entries, device_read_ipc_params, device_to_ipc,
        looks_like_ipc_connect_error, seed_seen_ids, trace_entry_matches_kind, Cli, Command,
        DaemonCommand, DeviceCommand, DeviceReadArgs, RecoveryCommand,
    };
    use clap::Parser as _;
    use serde_json::json;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn cli_parses_no_auto_start_global_flag() {
        let cli = Cli::try_parse_from([
            "mains-aegis",
            "--ipc",
            ".tmp/devd.sock",
            "--no-auto-start",
            "health",
        ])
        .unwrap();

        assert_eq!(cli.ipc.as_deref(), Some(".tmp/devd.sock"));
        assert!(cli.no_auto_start);
        assert!(matches!(cli.command, Command::Health));
    }

    #[test]
    fn cli_parses_daemon_serve_as_developer_foreground_command() {
        let cli = Cli::try_parse_from([
            "mains-aegis",
            "--ipc",
            ".tmp/devd.sock",
            "daemon",
            "serve",
            "--idle-timeout-secs",
            "0",
        ])
        .unwrap();

        assert!(matches!(
            cli.command,
            Command::Daemon {
                command: DaemonCommand::Serve {
                    idle_timeout_secs: 0,
                    allow_host_power_actions: false,
                }
            }
        ));
    }

    #[test]
    fn cli_maps_bms_discharge_authorization_recovery_to_ipc() {
        let cli = Cli::try_parse_from([
            "mains-aegis",
            "device",
            "serial-04f3bb3f5367",
            "recovery",
            "bms-discharge-authorization",
        ])
        .unwrap();

        let Command::Device { device_id, command } = cli.command else {
            panic!("expected device command");
        };
        assert!(matches!(
            command,
            DeviceCommand::Recovery {
                command: RecoveryCommand::BmsDischargeAuthorization
            }
        ));

        let (method, params) = device_to_ipc(device_id, command);
        assert_eq!(method, "device.recovery.bms_discharge_authorization");
        assert_eq!(params, json!({"device_id": "serial-04f3bb3f5367"}));
    }

    #[test]
    fn cli_parses_daemon_http_as_explicit_web_service_command() {
        let cli = Cli::try_parse_from([
            "mains-aegis",
            "--ipc",
            ".tmp/devd.sock",
            "daemon",
            "http",
            "--bind",
            "127.0.0.1:30081",
            "--allow-dev-cors",
        ])
        .unwrap();

        match cli.command {
            Command::Daemon {
                command:
                    DaemonCommand::Http {
                        bind,
                        allow_dev_cors,
                        allow_host_power_actions,
                        allow_lan_bridge,
                        auth_token_file,
                        open_browser,
                    },
            } => {
                assert_eq!(bind.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
                assert_eq!(bind.port(), 30081);
                assert!(allow_dev_cors);
                assert!(!allow_host_power_actions);
                assert!(!allow_lan_bridge);
                assert!(auth_token_file.is_none());
                assert!(!open_browser);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn auto_start_only_retries_native_ipc_connect_errors() {
        let socket_error = anyhow::anyhow!("connect IPC socket .tmp/devd.sock: not found");
        let pipe_error =
            anyhow::anyhow!("connect IPC pipe \\\\.\\pipe\\mains-aegis-devd: not found");
        let protocol_error = anyhow::anyhow!("IPC endpoint must be a native IPC endpoint");

        assert!(looks_like_ipc_connect_error(&socket_error));
        assert!(looks_like_ipc_connect_error(&pipe_error));
        assert!(!looks_like_ipc_connect_error(&protocol_error));
    }

    #[test]
    fn event_kind_only_matches_power_target() {
        let power_event = json!({
            "kind": "event",
            "target": "power",
        });
        let other_event = json!({
            "kind": "event",
            "target": "host_power",
        });
        let info_log = json!({
            "kind": "log",
            "target": "power",
        });

        assert!(trace_entry_matches_kind(&power_event, Some("event")));
        assert!(!trace_entry_matches_kind(&other_event, Some("event")));
        assert!(!trace_entry_matches_kind(&info_log, Some("event")));
        assert!(trace_entry_matches_kind(&info_log, Some("log")));
        assert!(trace_entry_matches_kind(&power_event, None));
    }

    #[test]
    fn seed_seen_ids_marks_existing_tail_as_seen() {
        let trace = vec![
            json!({"id": "old-1", "kind": "event", "target": "power"}),
            json!({"id": "old-2", "kind": "log", "target": "power"}),
        ];

        let seen = seed_seen_ids(&trace);

        assert!(seen.contains("old-1"));
        assert!(seen.contains("old-2"));
    }

    #[test]
    fn collect_new_matching_entries_ignores_old_tail_and_filters_by_kind() {
        let initial = vec![
            json!({"id": "old-1", "kind": "event", "target": "power"}),
            json!({"id": "old-2", "kind": "log", "target": "power"}),
        ];
        let next = vec![
            json!({"id": "old-1", "kind": "event", "target": "power"}),
            json!({"id": "new-1", "kind": "event", "target": "power"}),
            json!({"id": "new-2", "kind": "event", "target": "host_power"}),
            json!({"id": "new-3", "kind": "log", "target": "power"}),
        ];

        let mut seen = seed_seen_ids(&initial);
        let new_entries = collect_new_matching_entries(&next, &mut seen, Some("event"));

        assert_eq!(new_entries.len(), 1);
        assert_eq!(new_entries[0]["id"], "new-1");
        assert!(seen.contains("new-1"));
        assert!(seen.contains("new-2"));
        assert!(seen.contains("new-3"));
    }

    #[test]
    fn device_read_single_read_defaults_to_monitor_cache_only() {
        let args = DeviceReadArgs {
            fresh: false,
            cache_only: false,
            include_meta: false,
            watch: false,
            interval_ms: 250,
            watch_freshness_ms: None,
            allow_stale_cache: false,
            samples: None,
        };

        assert_eq!(
            device_read_ipc_params("serial-1", &args, false, false, None),
            json!({
                "device_id": "serial-1",
                "fresh": false,
                "cache_only": true,
                "allow_stale_cache": false,
                "include_meta": false,
            })
        );
    }

    #[test]
    fn device_read_watch_defaults_to_monitor_cache_only() {
        let args = DeviceReadArgs {
            fresh: false,
            cache_only: false,
            include_meta: false,
            watch: true,
            interval_ms: 250,
            watch_freshness_ms: None,
            allow_stale_cache: false,
            samples: Some(4),
        };

        assert_eq!(
            device_read_ipc_params("serial-1", &args, true, true, None),
            json!({
                "device_id": "serial-1",
                "fresh": false,
                "cache_only": true,
                "allow_stale_cache": true,
                "include_meta": true,
                "watch_freshness_ms": 750,
            })
        );
    }

    #[test]
    fn device_read_watch_default_interval_keeps_monitor_cache_tolerant() {
        let args = DeviceReadArgs {
            fresh: false,
            cache_only: false,
            include_meta: false,
            watch: true,
            interval_ms: 333,
            watch_freshness_ms: None,
            allow_stale_cache: false,
            samples: Some(4),
        };

        assert_eq!(
            device_read_ipc_params("serial-1", &args, true, true, None)["watch_freshness_ms"],
            json!(750)
        );
    }

    #[test]
    fn device_read_watch_always_allows_stale_cache_for_continuous_timeline() {
        let args = DeviceReadArgs {
            fresh: false,
            cache_only: false,
            include_meta: false,
            watch: true,
            interval_ms: 333,
            watch_freshness_ms: None,
            allow_stale_cache: true,
            samples: Some(4),
        };

        assert_eq!(
            device_read_ipc_params("serial-1", &args, true, true, None)["allow_stale_cache"],
            json!(true)
        );
    }

    #[test]
    fn device_read_watch_fresh_explicitly_bypasses_monitor_cache() {
        let args = DeviceReadArgs {
            fresh: true,
            cache_only: false,
            include_meta: false,
            watch: true,
            interval_ms: 250,
            watch_freshness_ms: Some(600),
            allow_stale_cache: false,
            samples: Some(4),
        };

        assert_eq!(
            device_read_ipc_params("serial-1", &args, true, true, None),
            json!({
                "device_id": "serial-1",
                "fresh": true,
                "cache_only": false,
                "allow_stale_cache": true,
                "include_meta": true,
                "watch_freshness_ms": 600,
            })
        );
    }
}

fn host_power_to_ipc(command: HostPowerCommand) -> (&'static str, Value) {
    match command {
        HostPowerCommand::Status => ("host.power.status", json!({})),
        HostPowerCommand::Profile {
            profile,
            dry_run,
            real,
        } => (
            "host.power.profile",
            json!({ "profile": profile, "dry_run": dry_run || !real }),
        ),
        HostPowerCommand::Suspend { dry_run, real } => {
            ("host.power.suspend", json!({ "dry_run": dry_run || !real }))
        }
        HostPowerCommand::Shutdown {
            delay_sec,
            dry_run,
            real,
            confirm,
            force,
        } => (
            "host.power.shutdown",
            json!({
                "delay_sec": delay_sec,
                "dry_run": dry_run || !real,
                "confirm": confirm,
                "force": force,
            }),
        ),
    }
}

fn settings_to_ipc(command: SettingsCommand) -> (&'static str, Value) {
    match command {
        SettingsCommand::Wifi { command } => match command {
            WifiCommand::Set {
                ssid,
                psk,
                device_id,
                lease_id,
            } => (
                "settings.wifi.set",
                json!({
                    "ssid": ssid,
                    "psk": psk,
                    "device_id": device_id,
                    "lease_id": lease_id,
                }),
            ),
            WifiCommand::Clear {
                device_id,
                lease_id,
            } => (
                "settings.wifi.clear",
                json!({
                    "device_id": device_id,
                    "lease_id": lease_id,
                }),
            ),
        },
        SettingsCommand::LogLevel {
            level,
            device_id,
            lease_id,
        } => (
            "settings.log_level.set",
            json!({ "level": level, "device_id": device_id, "lease_id": lease_id }),
        ),
        SettingsCommand::ManualCharge {
            target,
            speed,
            timer_h,
            device_id,
            lease_id,
        } => (
            "settings.manual_charge.set",
            json!({
                "target": target,
                "speed": speed,
                "timer_h": timer_h,
                "device_id": device_id,
                "lease_id": lease_id,
            }),
        ),
        SettingsCommand::AdvancedPower {
            standby_drop_mv,
            assist_low_drop_mv,
            assist_enter_delta_ma,
            assist_exit_delta_ma,
            assist_required_samples,
            assist_ramp_step_mv,
            assist_ramp_interval_ms,
            rated_enter_delta_ma,
            rated_exit_delta_ma,
            vin_drop_threshold_pct,
            required_samples,
            source_limited_vin_drop_pct,
            source_limited_enter_delta_ma,
            source_limited_exit_delta_ma,
            source_limited_required_samples,
            source_limited_recover_margin_mv,
            device_id,
            lease_id,
        } => (
            "settings.advanced_power.set",
            json!({
                "standby_drop_mv": standby_drop_mv,
                "assist_low_drop_mv": assist_low_drop_mv,
                "assist_enter_delta_ma": assist_enter_delta_ma,
                "assist_exit_delta_ma": assist_exit_delta_ma,
                "assist_required_samples": assist_required_samples,
                "assist_ramp_step_mv": assist_ramp_step_mv,
                "assist_ramp_interval_ms": assist_ramp_interval_ms,
                "rated_enter_delta_ma": rated_enter_delta_ma,
                "rated_exit_delta_ma": rated_exit_delta_ma,
                "vin_drop_threshold_pct": vin_drop_threshold_pct,
                "required_samples": required_samples,
                "source_limited_vin_drop_pct": source_limited_vin_drop_pct,
                "source_limited_enter_delta_ma": source_limited_enter_delta_ma,
                "source_limited_exit_delta_ma": source_limited_exit_delta_ma,
                "source_limited_required_samples": source_limited_required_samples,
                "source_limited_recover_margin_mv": source_limited_recover_margin_mv,
                "device_id": device_id,
                "lease_id": lease_id,
            }),
        ),
        SettingsCommand::AdvancedPowerReset {
            device_id,
            lease_id,
        } => (
            "settings.advanced_power.reset",
            json!({
                "device_id": device_id,
                "lease_id": lease_id,
            }),
        ),
    }
}
