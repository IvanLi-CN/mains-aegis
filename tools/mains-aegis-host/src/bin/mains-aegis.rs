use clap::{Args, Parser, Subcommand};
use mains_aegis_host::{default_ipc_endpoint, ipc_call, release_version};
use serde_json::{json, Value};

#[derive(Debug, Parser)]
#[command(name = "mains-aegis")]
#[command(version = release_version())]
#[command(about = "Mains Aegis host CLI")]
struct Cli {
    /// IPC socket or named-pipe endpoint.
    #[arg(long, global = true, env = "MAINS_AEGIS_DEVD_IPC")]
    ipc: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
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
    Unbind,
    Connect,
    Disconnect,
    Connection,
    Identity,
    Settings,
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

#[derive(Debug, Args)]
struct TraceArgs {
    #[arg(long)]
    logs_limit: Option<usize>,
    #[arg(long)]
    trace_limit: Option<usize>,
    #[arg(long)]
    lease_id: Option<String>,
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
    let (method, params) = command_to_ipc(cli.command);
    let result = ipc_call(&endpoint, method, params).await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

fn command_to_ipc(command: Command) -> (&'static str, Value) {
    match command {
        Command::Health => ("devd.health", json!({})),
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
    }
}

fn device_to_ipc(device_id: String, command: DeviceCommand) -> (&'static str, Value) {
    match command {
        DeviceCommand::Bind { alias } => (
            "device.bind",
            json!({ "device_id": device_id, "alias": alias }),
        ),
        DeviceCommand::Unbind => ("device.unbind", json!({ "device_id": device_id })),
        DeviceCommand::Connect => ("device.connect", json!({ "device_id": device_id })),
        DeviceCommand::Disconnect => ("device.disconnect", json!({ "device_id": device_id })),
        DeviceCommand::Connection => ("device.connection", json!({ "device_id": device_id })),
        DeviceCommand::Identity => ("device.identity", json!({ "device_id": device_id })),
        DeviceCommand::Settings => ("device.settings", json!({ "device_id": device_id })),
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
    }
}
