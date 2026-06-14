use clap::{Args, Parser, Subcommand};
use mains_aegis_host::{default_ipc_endpoint, ipc_call, release_version};
use serde_json::{json, Value};
use std::io::{self, IsTerminal, Write};
use tokio::time::{sleep, Duration};

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
    CompanionLan {
        #[command(subcommand)]
        command: CompanionLanCommand,
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
    #[arg(long)]
    follow: bool,
    #[arg(long)]
    kind: Option<String>,
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
    match cli.command {
        Command::Device {
            device_id,
            command: DeviceCommand::Trace(args),
        } if args.follow => {
            follow_device_trace(&endpoint, &device_id, args).await?;
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
            let mut result = ipc_call(&endpoint, method, params).await?;
            if let Some((device_id, _alias)) = interactive_bind {
                if io::stdin().is_terminal()
                    && io::stdout().is_terminal()
                    && maybe_confirm_companion_lan(&endpoint, &device_id, &result).await?
                {
                    result = ipc_call(
                        &endpoint,
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

async fn follow_device_trace(
    endpoint: &str,
    device_id: &str,
    args: TraceArgs,
) -> anyhow::Result<()> {
    let initial = ipc_call(
        endpoint,
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
        let result = ipc_call(
            endpoint,
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

async fn maybe_confirm_companion_lan(
    endpoint: &str,
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
    let result = ipc_call(
        endpoint,
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
    use super::{collect_new_matching_entries, seed_seen_ids, trace_entry_matches_kind};
    use serde_json::json;

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
