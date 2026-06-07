use std::{net::SocketAddr, path::PathBuf, time::Duration};

use anyhow::Context as _;
use clap::{Parser, Subcommand};
use mains_aegis_host::{
    default_ipc_endpoint, release_version, serve_http_service, serve_ipc, HttpServiceConfig,
    IpcConfig, DEFAULT_BIND,
};

#[derive(Debug, Parser)]
#[command(name = "mains-aegis-devd")]
#[command(version = release_version())]
#[command(about = "Mains Aegis local device daemon")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Serve the local IPC daemon.
    Serve {
        /// IPC socket or named-pipe endpoint.
        #[arg(long, env = "MAINS_AEGIS_DEVD_IPC")]
        ipc: Option<String>,
        /// Exit after this many idle seconds. Use 0 to disable idle shutdown.
        #[arg(long, default_value_t = mains_aegis_host::DEFAULT_IPC_IDLE_TIMEOUT_SECS)]
        idle_timeout_secs: u64,
        /// Allow real host power profile/suspend/shutdown actions.
        #[arg(long, env = "MAINS_AEGIS_DEVD_ALLOW_HOST_POWER_ACTIONS")]
        allow_host_power_actions: bool,
    },
    /// Expose the devd HTTP service explicitly.
    ServeHttp {
        /// IPC socket or named-pipe endpoint shared with the HTTP service.
        #[arg(long, env = "MAINS_AEGIS_DEVD_IPC")]
        ipc: Option<String>,
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mains_aegis_host=info".into()),
        )
        .init();

    match Cli::parse().command {
        Command::Serve {
            ipc,
            idle_timeout_secs,
            allow_host_power_actions,
        } => {
            let idle_timeout = if idle_timeout_secs == 0 {
                None
            } else {
                Some(Duration::from_secs(idle_timeout_secs))
            };
            serve_ipc(
                IpcConfig::new(ipc.unwrap_or_else(default_ipc_endpoint))
                    .with_idle_timeout(idle_timeout)
                    .with_host_power_actions(allow_host_power_actions),
            )
            .await
        }
        Command::ServeHttp {
            ipc,
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
                        .with_context(|| format!("read auth token file {}", path.display()))
                        .map(|token| token.trim().to_string())
                })
                .transpose()?;
            serve_http_service(HttpServiceConfig {
                ipc_endpoint: ipc.unwrap_or_else(default_ipc_endpoint),
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
