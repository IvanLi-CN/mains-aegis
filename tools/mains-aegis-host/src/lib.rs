#![recursion_limit = "256"]

use axum::{
    extract::{Path, Query, State},
    http::{
        header::{ACCEPT, CONTENT_TYPE},
        HeaderMap, HeaderValue, Method, StatusCode, Uri,
    },
    middleware::{self, Next},
    response::{sse::Event, sse::Sse, IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use chrono::Utc;
use defmt_decoder::{
    log::format::{Formatter, FormatterConfig, FormatterFormat},
    Table,
};
#[cfg(not(test))]
use directories::ProjectDirs;
use getrandom::fill as fill_random;
use include_dir::{include_dir, Dir};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    env, fs,
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr},
    path::{Path as FsPath, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    process::Command,
    sync::{broadcast, Mutex as AsyncMutex},
};
use tower_http::cors::{AllowOrigin, CorsLayer};

pub const DEFAULT_BIND: &str = "127.0.0.1:30080";
pub const DEFAULT_IPC_FILE_NAME: &str = "devd.sock";
pub const DEFAULT_WINDOWS_PIPE_NAME: &str = r"\\.\pipe\mains-aegis-devd";
pub const DEFAULT_IPC_IDLE_TIMEOUT_SECS: u64 = 30;
const EVENT_LIMIT: usize = 1_000;
const LOG_LIMIT: usize = 2_000;
const WEB_LEASE_HEARTBEAT_INTERVAL_MS: u64 = 2_000;
const WEB_LEASE_TTL_MS: u64 = 8_000;
const WEB_LEASE_CLEANUP_INTERVAL_MS: u64 = 1_000;
const LAN_DISCOVERY_SERVICE: &str = "_mains-aegis-ups._tcp.local";
const LAN_DISCOVERY_PORT: u16 = 80;
const LAN_SCAN_CONCURRENCY: usize = 32;
const LAN_PROBE_TIMEOUT_MS: u64 = 800;
const LAN_SCAN_MAX_HOSTS: usize = 4_096;
const DEFAULT_ESPFLASH_TIMEOUT_SECS: u64 = 180;
#[cfg(not(test))]
const DEVD_STATE_FILE_NAME: &str = "devices.json";
#[cfg(not(test))]
const DEVD_STATE_FILE_ENV: &str = "MAINS_AEGIS_DEVD_STATE_FILE";
static STATE_PERSIST_TEMP_SEQ: AtomicU64 = AtomicU64::new(0);
const WEB_DEV_FIRMWARE_CACHE_DIR: &str = "tmp/web-dev-firmware";
const APP_SESSION_HEADER: &str = "x-mains-aegis-app-session";
const APP_SESSION_QUERY_PARAM: &str = "app_session";
const SERVICE_TOKEN_QUERY_PARAM: &str = "service_token";
const LEGACY_BRIDGE_TOKEN_QUERY_PARAM: &str = "bridge_token";
const EMBEDDED_APP_SESSION_PLACEHOLDER: &str = "__MAINS_AEGIS_APP_SESSION__";
const EMBEDDED_HTTP_SERVICE_MODE_PLACEHOLDER: &str = "__MAINS_AEGIS_HTTP_SERVICE_MODE__";
static EMBEDDED_WEB_DIST: Dir<'static> = include_dir!("$MAINS_AEGIS_EMBEDDED_WEB_DIST");

#[derive(Debug, Clone)]
pub struct HttpServiceConfig {
    pub ipc_endpoint: String,
    pub bind: SocketAddr,
    pub allow_dev_cors: bool,
    pub allow_host_power_actions: bool,
    pub allow_lan_bridge: bool,
    pub auth_token: Option<String>,
    pub open_browser: bool,
}

#[derive(Debug, Clone)]
pub struct IpcConfig {
    pub endpoint: String,
    pub idle_timeout: Option<Duration>,
    pub allow_host_power_actions: bool,
}

impl IpcConfig {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            idle_timeout: Some(Duration::from_secs(DEFAULT_IPC_IDLE_TIMEOUT_SECS)),
            allow_host_power_actions: false,
        }
    }

    pub fn with_idle_timeout(mut self, idle_timeout: Option<Duration>) -> Self {
        self.idle_timeout = idle_timeout;
        self
    }

    pub fn with_host_power_actions(mut self, allow_host_power_actions: bool) -> Self {
        self.allow_host_power_actions = allow_host_power_actions;
        self
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IpcRequest {
    pub id: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IpcResponse {
    pub id: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn release_version() -> &'static str {
    option_env!("MAINS_AEGIS_RELEASE_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
}

pub fn default_ipc_endpoint() -> String {
    #[cfg(windows)]
    {
        DEFAULT_WINDOWS_PIPE_NAME.to_string()
    }
    #[cfg(not(windows))]
    {
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::temp_dir().join(format!("mains-aegis-{}", user_id_hint()))
            });
        base.join("mains-aegis")
            .join(DEFAULT_IPC_FILE_NAME)
            .to_string_lossy()
            .to_string()
    }
}

fn user_id_hint() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "user".to_string())
}

#[derive(Clone)]
struct AppState {
    inner: Arc<Mutex<DevdState>>,
    events: broadcast::Sender<DevdEvent>,
    allow_host_power_actions: bool,
    auth_token_required: bool,
    http_service_mode: HttpServiceMode,
    app_session_secret: Option<Arc<str>>,
    persistence: DevdPersistence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HttpServiceMode {
    HostedApp,
    ApiOnly,
}

#[derive(Debug, Clone)]
struct HttpServiceAuth {
    bearer_token: Option<Arc<str>>,
    app_session_secret: Option<Arc<str>>,
}

#[derive(Clone)]
struct IpcRuntime {
    app: AppState,
    lifecycle: Arc<AsyncMutex<IpcLifecycle>>,
}

impl IpcRuntime {
    fn new(app: AppState) -> Self {
        Self {
            app,
            lifecycle: Arc::new(AsyncMutex::new(IpcLifecycle::default())),
        }
    }
}

struct IpcLifecycle {
    active_clients: usize,
    last_activity: Instant,
}

impl Default for IpcLifecycle {
    fn default() -> Self {
        Self {
            active_clients: 0,
            last_activity: Instant::now(),
        }
    }
}

#[derive(Debug, Default)]
struct DevdState {
    devices: HashMap<String, DeviceRecord>,
    bindings: HashMap<String, DeviceBinding>,
    selected_artifacts: HashMap<String, String>,
    artifacts: HashMap<String, FirmwareArtifact>,
    events: VecDeque<DevdEvent>,
    monitors: HashMap<String, NativeMonitorHandle>,
    web_leases: HashMap<String, WebUsbLease>,
    host_power: HostPowerState,
    scan_trace: VecDeque<SerialTraceEntry>,
    persisted_device_trace: HashMap<String, VecDeque<SerialTraceEntry>>,
}

#[derive(Debug, Default)]
struct HostPowerState {
    previous_profile: Option<String>,
    last_action: Option<Value>,
}

#[derive(Debug, Clone)]
struct DevdPersistence {
    state_file: Option<PathBuf>,
}

impl DevdPersistence {
    fn enabled(state_file: PathBuf) -> Self {
        Self {
            state_file: Some(state_file),
        }
    }

    #[cfg(test)]
    fn disabled() -> Self {
        Self { state_file: None }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedDevdState {
    schema_version: u8,
    bindings: HashMap<String, DeviceBinding>,
    selected_artifacts: HashMap<String, String>,
    artifacts: HashMap<String, FirmwareArtifact>,
    #[serde(default)]
    scan_trace: VecDeque<SerialTraceEntry>,
    #[serde(default)]
    device_trace: HashMap<String, VecDeque<SerialTraceEntry>>,
}

impl Default for PersistedDevdState {
    fn default() -> Self {
        Self {
            schema_version: 1,
            bindings: HashMap::new(),
            selected_artifacts: HashMap::new(),
            artifacts: HashMap::new(),
            scan_trace: VecDeque::new(),
            device_trace: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct WebUsbLease {
    lease_id: String,
    device_id: String,
    identity_device_id: Option<String>,
    expires_at: std::time::Instant,
}

#[derive(Debug)]
struct NativeMonitorHandle {
    stop: Arc<AtomicBool>,
    command_tx: mpsc::Sender<NativeMonitorCommand>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeSerialLineStep {
    Dtr(bool),
    Rts(bool),
    SleepMs(u64),
}

#[derive(Debug)]
enum NativeMonitorCommand {
    SendFrame {
        frame: Value,
        request_id: String,
        response_tx: mpsc::Sender<Result<Value, HttpError>>,
    },
    Reset {
        response_tx: mpsc::Sender<Result<(), HttpError>>,
    },
}

#[derive(Debug, PartialEq, Eq)]
enum NativeMonitorInput {
    CdcLine(Vec<u8>),
    DefmtBytes(Vec<u8>),
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DeviceRecord {
    id: String,
    display_name: String,
    port_path: Option<String>,
    lan_address: Option<String>,
    lan_conflict_addresses: Vec<String>,
    transport: DeviceTransport,
    binding: Option<DeviceBinding>,
    connection: ConnectionState,
    identity: Option<Value>,
    status: Option<Value>,
    power_diag: Option<Value>,
    selected_artifact_id: Option<String>,
    log_decode: LogDecodeState,
    settings: DeviceSettingsState,
    logs: VecDeque<SerialLogEntry>,
    trace: VecDeque<SerialTraceEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DeviceTransport {
    NativeSerial,
    Lan,
    Mock,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DeviceBinding {
    alias: Option<String>,
    stable_id: String,
    port_path: Option<String>,
    created_at: String,
    #[serde(default)]
    logical_device_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ConnectionState {
    Disconnected,
    Connected,
    Busy,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LogDecodeState {
    status: String,
    reason: Option<String>,
    artifact_id: Option<String>,
}

impl Default for LogDecodeState {
    fn default() -> Self {
        Self {
            status: "unverified".to_string(),
            reason: Some("no artifact selected".to_string()),
            artifact_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FirmwareArtifact {
    artifact_id: String,
    name: String,
    version: String,
    git_sha: String,
    build_id: String,
    target_chip: String,
    profile: String,
    features: Vec<String>,
    protocol: String,
    defmt: DefmtMetadata,
    files: Vec<ArtifactFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DefmtMetadata {
    enabled: bool,
    encoding: String,
    elf_sha256: Option<String>,
    metadata_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ArtifactFile {
    kind: String,
    path: String,
    sha256: String,
    size: u64,
    flash_address: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DevdEvent {
    id: String,
    timestamp: String,
    device_id: Option<String>,
    kind: String,
    message: String,
    payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SerialLogEntry {
    id: String,
    timestamp: String,
    level: String,
    target: String,
    message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SerialTraceEntry {
    id: String,
    timestamp: String,
    direction: String,
    kind: String,
    #[serde(rename = "frameType")]
    frame_type: Option<String>,
    #[serde(rename = "requestId")]
    request_id: Option<String>,
    target: Option<String>,
    summary: String,
    payload: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DeviceSettingsState {
    wifi_configured: Option<bool>,
    wifi_ssid: Option<String>,
    log_level: String,
    manual_charge: ManualChargePrefs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManualChargePrefs {
    target: String,
    speed: String,
    timer_h: u8,
}

#[derive(Debug, Serialize)]
struct ApiError {
    code: String,
    message: String,
    retryable: bool,
    details: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct BindRequest {
    alias: Option<String>,
    logical_device_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ArtifactSelectRequest {
    manifest_path: Option<String>,
    artifact_id: Option<String>,
    artifact: Option<FirmwareArtifact>,
}

#[derive(Debug, Deserialize)]
struct FlashRequest {
    artifact_id: Option<String>,
    dry_run: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct DefmtDecodeRequest {
    elf_path: String,
    frame_hex: String,
}

#[derive(Debug, Deserialize)]
struct SessionQuery {
    logs_limit: Option<usize>,
    trace_limit: Option<usize>,
    lease_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ScanDevicesQuery {
    cidr: Option<String>,
    lan_cidr: Option<String>,
    lan: Option<bool>,
    mdns: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ScanTraceQuery {
    trace_limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct WifiConfigRequest {
    ssid: String,
    psk: String,
    device_id: Option<String>,
    lease_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LogLevelRequest {
    level: String,
    device_id: Option<String>,
    lease_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ManualChargeRequest {
    target: String,
    speed: String,
    timer_h: u8,
    device_id: Option<String>,
    lease_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SettingsTargetQuery {
    device_id: Option<String>,
    lease_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WebLeaseCreateRequest {
    device_id: String,
}

#[derive(Debug, Deserialize)]
struct WebLeaseQuery {
    lease_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HostPowerProfileRequest {
    profile: String,
    dry_run: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct HostPowerDryRunRequest {
    dry_run: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct HostPowerShutdownRequest {
    delay_sec: Option<u64>,
    dry_run: Option<bool>,
    confirm: Option<String>,
    force: Option<bool>,
}

pub async fn serve_http_service(config: HttpServiceConfig) -> anyhow::Result<()> {
    if !config.bind.ip().is_loopback()
        && (!config.allow_lan_bridge || config.auth_token.as_deref().unwrap_or("").is_empty())
    {
        anyhow::bail!(
            "non-loopback HTTP service requires --allow-lan-bridge and --auth-token-file"
        );
    }
    if config.allow_dev_cors && config.open_browser {
        anyhow::bail!("--open-browser cannot be used together with --allow-dev-cors");
    }
    let auth_token_required = config
        .auth_token
        .as_ref()
        .is_some_and(|token| !token.is_empty());
    let http_service_mode = if config.allow_dev_cors {
        HttpServiceMode::ApiOnly
    } else {
        HttpServiceMode::HostedApp
    };
    let app_session_secret = match http_service_mode {
        HttpServiceMode::HostedApp => Some(generate_app_session_secret()?),
        HttpServiceMode::ApiOnly => None,
    };
    let state = create_app_state_with_auth(
        config.allow_host_power_actions,
        auth_token_required,
        http_service_mode,
        app_session_secret.clone(),
    );
    let ipc_config = IpcConfig::new(config.ipc_endpoint.clone())
        .with_idle_timeout(None)
        .with_host_power_actions(config.allow_host_power_actions);
    let ipc_runtime = IpcRuntime::new(state.clone());
    let auth = HttpServiceAuth {
        bearer_token: config.auth_token.as_deref().map(Arc::<str>::from),
        app_session_secret: app_session_secret.clone(),
    };

    let mut app = Router::new()
        .route("/health", get(health))
        .route("/api/v1/bootstrap", get(bootstrap))
        .route("/api/v1/ping", get(health))
        .route("/api/v1/identity", get(devd_compat_identity))
        .route("/api/v1/network", get(devd_compat_network))
        .route("/api/v1/status", get(devd_compat_status))
        .route("/api/v1/settings", get(devd_compat_settings))
        .route(
            "/api/v1/wifi-config",
            post(set_wifi_config).delete(clear_wifi_config),
        )
        .route("/api/v1/settings/log-level", post(set_log_level))
        .route("/api/v1/settings/manual-charge", post(set_manual_charge))
        .route("/api/v1/devices", get(list_devices))
        .route("/api/v1/devices/events", get(devices_events))
        .route("/api/v1/devices/scan", post(scan_devices))
        .route("/api/v1/devices/scan/trace", get(scan_trace))
        .route("/api/v1/devices/{id}/bind", post(bind_device))
        .route("/api/v1/devices/{id}/connect", post(connect_device))
        .route("/api/v1/devices/{id}/disconnect", post(disconnect_device))
        .route("/api/v1/devices/{id}/binding", delete(unbind_device))
        .route("/api/v1/devices/{id}/identity", get(device_identity))
        .route("/api/v1/devices/{id}/power-diag", get(device_power_diag))
        .route("/api/v1/devices/{id}/connection", get(device_connection))
        .route(
            "/api/v1/devices/{id}/artifact",
            get(device_artifact).post(select_artifact),
        )
        .route("/api/v1/devices/{id}/settings", get(device_settings))
        .route("/api/v1/devices/{id}/trace", get(device_trace))
        .route("/api/v1/devices/{id}/flash", post(flash_device))
        .route("/api/v1/devices/{id}/reset", post(reset_device))
        .route("/api/v1/devices/{id}/monitor/start", post(monitor_start))
        .route("/api/v1/devices/{id}/monitor/stop", post(monitor_stop))
        .route("/api/v1/devices/{id}/events", get(device_events))
        .route("/api/v1/serial/lease", post(create_web_lease))
        .route(
            "/api/v1/serial/lease/{lease_id}",
            post(heartbeat_web_lease).delete(release_web_lease),
        )
        .route("/api/v1/serial/session", get(devd_compat_session))
        .route("/api/v1/serial/events", get(devd_compat_events))
        .route("/api/v1/host/power", get(host_power_status))
        .route("/api/v1/host/power/profile", post(host_power_profile))
        .route("/api/v1/host/power/suspend", post(host_power_suspend))
        .route("/api/v1/host/power/shutdown", post(host_power_shutdown))
        .route("/api/v1/host/power/events", get(host_power_events))
        .route("/api/v1/defmt/decode", post(defmt_decode))
        .fallback(get(http_service_fallback).head(http_service_fallback))
        .with_state(state);

    if auth.bearer_token.is_some() || auth.app_session_secret.is_some() {
        app = app.layer(middleware::from_fn_with_state(
            auth,
            require_http_service_access,
        ));
    }
    if config.allow_dev_cors {
        app = app.layer(
            CorsLayer::new()
                .allow_origin(AllowOrigin::predicate(|origin, _request_parts| {
                    is_local_dev_cors_origin(origin)
                }))
                .allow_methods([Method::GET, Method::POST, Method::DELETE])
                .allow_headers(tower_http::cors::Any),
        );
    }

    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .map_err(|error| anyhow::anyhow!("bind mains-aegis-devd: {error}"))?;
    tracing::info!("mains-aegis-devd listening on http://{}", config.bind);
    tracing::info!(
        "mains-aegis-devd HTTP service IPC listening on {}",
        config.ipc_endpoint
    );
    if config.open_browser {
        if let Err(error) = open_http_service_in_default_browser(config.bind) {
            tracing::warn!(
                "failed to open hosted app in the default browser: {error}. Visit {} manually.",
                http_service_browser_url(config.bind)
            );
        }
    }
    let http_server = axum::serve(listener, app);
    tokio::try_join!(
        serve_ipc_with_runtime(ipc_config, ipc_runtime),
        async move {
            http_server
                .await
                .map_err(|error| anyhow::anyhow!("serve devd: {error}"))
        }
    )?;
    Ok(())
}

async fn require_http_service_access(
    State(auth): State<HttpServiceAuth>,
    request: axum::extract::Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if request.method() == Method::OPTIONS {
        return Ok(next.run(request).await);
    }
    if is_unauthenticated_bootstrap_request(request.method(), request.uri()) {
        return Ok(next.run(request).await);
    }
    if is_static_web_asset_request(request.method(), request.uri()) {
        return Ok(next.run(request).await);
    }

    let header_authorized = auth
        .bearer_token
        .as_deref()
        .is_some_and(|token| bearer_header_matches(request.headers(), token))
        || auth
            .app_session_secret
            .as_deref()
            .is_some_and(|secret| app_session_header_matches(request.headers(), secret));
    let event_stream_authorized =
        is_event_stream_query_auth_request(request.method(), request.uri(), request.headers())
            && auth_query_matches(&auth, request.uri());
    if header_authorized || event_stream_authorized {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

fn bearer_header_matches(headers: &HeaderMap, token: &str) -> bool {
    let expected = format!("Bearer {token}");
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|header| header.to_str().ok())
        .is_some_and(|actual| actual == expected)
}

fn app_session_header_matches(headers: &HeaderMap, secret: &str) -> bool {
    headers
        .get(APP_SESSION_HEADER)
        .and_then(|header| header.to_str().ok())
        .is_some_and(|actual| actual == secret)
}

fn auth_query_matches(auth: &HttpServiceAuth, uri: &Uri) -> bool {
    auth.app_session_secret.as_deref().is_some_and(|secret| {
        query_param(uri, APP_SESSION_QUERY_PARAM).is_some_and(|actual| actual == secret)
    }) || auth.bearer_token.as_deref().is_some_and(|token| {
        query_param(uri, SERVICE_TOKEN_QUERY_PARAM).is_some_and(|actual| actual == token)
            || query_param(uri, LEGACY_BRIDGE_TOKEN_QUERY_PARAM)
                .is_some_and(|actual| actual == token)
    })
}

fn is_event_stream_query_auth_request(method: &Method, uri: &Uri, headers: &HeaderMap) -> bool {
    *method == Method::GET && accepts_event_stream(headers) && is_event_stream_endpoint(uri.path())
}

fn is_event_stream_endpoint(path: &str) -> bool {
    path == "/api/v1/status"
        || path == "/api/v1/serial/events"
        || path == "/api/v1/host/power/events"
        || (path.starts_with("/api/v1/devices/") && path.ends_with("/events"))
}

fn accepts_event_stream(headers: &HeaderMap) -> bool {
    headers
        .get(ACCEPT)
        .and_then(|header| header.to_str().ok())
        .is_some_and(|accept| {
            accept.split(',').any(|part| {
                part.trim()
                    .split(';')
                    .next()
                    .is_some_and(|media_type| media_type.eq_ignore_ascii_case("text/event-stream"))
            })
        })
}

fn query_param(uri: &Uri, key: &str) -> Option<String> {
    uri.query()?.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == key)
            .then(|| percent_decode_query_value(value))
            .and_then(Result::ok)
    })
}

fn is_static_web_asset_request(method: &Method, uri: &Uri) -> bool {
    matches!(*method, Method::GET | Method::HEAD)
        && !uri.path().starts_with("/api/")
        && uri.path() != "/health"
}

fn is_unauthenticated_bootstrap_request(method: &Method, uri: &Uri) -> bool {
    *method == Method::GET && uri.path() == "/api/v1/bootstrap"
}

fn is_local_dev_cors_origin(origin: &HeaderValue) -> bool {
    let Ok(origin) = origin.to_str() else {
        return false;
    };

    matches!(
        origin,
        "http://localhost" | "http://127.0.0.1" | "http://[::1]"
    ) || origin.starts_with("http://localhost:")
        || origin.starts_with("http://127.0.0.1:")
        || origin.starts_with("http://[::1]:")
}

async fn http_service_fallback(
    State(state): State<AppState>,
    uri: Uri,
    method: Method,
) -> Response {
    match state.http_service_mode {
        HttpServiceMode::ApiOnly => api_only_root_response(&uri, &method),
        HttpServiceMode::HostedApp => embedded_web_response(&state, &uri, &method),
    }
}

fn api_only_root_response(uri: &Uri, method: &Method) -> Response {
    if uri.path() != "/" {
        return StatusCode::NOT_FOUND.into_response();
    }
    let body = "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\" /><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\" /><title>Mains Aegis HTTP service</title></head><body><main style=\"font-family: system-ui, sans-serif; max-width: 44rem; margin: 4rem auto; padding: 0 1.5rem; line-height: 1.6;\"><h1>Mains Aegis HTTP service</h1><p>This devd instance is running in API-only development mode.</p><p>Start the Vite dev server and open that URL instead of using this root page.</p></main></body></html>";
    html_response(body.as_bytes(), method)
}

fn embedded_web_response(state: &AppState, uri: &Uri, method: &Method) -> Response {
    let requested = normalized_embedded_path(uri.path());
    let file = EMBEDDED_WEB_DIST.get_file(&requested).or_else(|| {
        requested
            .strip_suffix('/')
            .and_then(|prefix| EMBEDDED_WEB_DIST.get_file(&format!("{prefix}/index.html")))
    });

    if let Some(file) = file {
        return embedded_file_response(
            file.path().to_string_lossy().as_ref(),
            file.contents(),
            state,
            method,
        );
    }

    if requested.contains('.') {
        return StatusCode::NOT_FOUND.into_response();
    }

    match EMBEDDED_WEB_DIST.get_file("index.html") {
        Some(file) => embedded_file_response("index.html", file.contents(), state, method),
        None => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

fn normalized_embedded_path(path: &str) -> String {
    match path.trim_start_matches('/') {
        "" => "index.html".to_string(),
        value => value.to_string(),
    }
}

fn embedded_file_response(
    path: &str,
    contents: &[u8],
    state: &AppState,
    method: &Method,
) -> Response {
    if path.ends_with(".html") {
        let body = embedded_html_body(contents, state);
        return response_with_content_type(body, "text/html; charset=utf-8", method);
    }

    response_with_content_type(contents.to_vec(), embedded_content_type(path), method)
}

fn embedded_html_body(contents: &[u8], state: &AppState) -> Vec<u8> {
    let html = String::from_utf8_lossy(contents);
    let mode = match state.http_service_mode {
        HttpServiceMode::HostedApp => "hosted",
        HttpServiceMode::ApiOnly => "api_only",
    };
    let injected = html
        .replace(
            EMBEDDED_APP_SESSION_PLACEHOLDER,
            state.app_session_secret.as_deref().unwrap_or(""),
        )
        .replace(EMBEDDED_HTTP_SERVICE_MODE_PLACEHOLDER, mode);
    injected.into_bytes()
}

fn embedded_content_type(path: &str) -> &'static str {
    match FsPath::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
    {
        "css" => "text/css; charset=utf-8",
        "html" => "text/html; charset=utf-8",
        "ico" => "image/x-icon",
        "jpeg" | "jpg" => "image/jpeg",
        "js" => "application/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "map" => "application/json; charset=utf-8",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "txt" => "text/plain; charset=utf-8",
        "webmanifest" => "application/manifest+json; charset=utf-8",
        "webp" => "image/webp",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn html_response(body: &[u8], method: &Method) -> Response {
    response_with_content_type(body.to_vec(), "text/html; charset=utf-8", method)
}

fn response_with_content_type(body: Vec<u8>, content_type: &str, method: &Method) -> Response {
    let body = if *method == Method::HEAD {
        Vec::new()
    } else {
        body
    };
    let mut response = body.into_response();
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_str(content_type).expect("valid content type"),
    );
    response
}

fn generate_app_session_secret() -> anyhow::Result<Arc<str>> {
    let mut bytes = [0u8; 32];
    fill_random(&mut bytes)
        .map_err(|error| anyhow::anyhow!("generate app session secret: {error}"))?;
    Ok(hex_lower(&bytes).into())
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from_digit((byte >> 4).into(), 16).expect("hex"));
        output.push(char::from_digit((byte & 0x0f).into(), 16).expect("hex"));
    }
    output
}

fn http_service_browser_url(bind: SocketAddr) -> String {
    format!("http://127.0.0.1:{}/", bind.port())
}

fn open_http_service_in_default_browser(bind: SocketAddr) -> anyhow::Result<()> {
    let url = http_service_browser_url(bind);
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&url)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| anyhow::anyhow!("open {url}: {error}"))?;
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &url])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| anyhow::anyhow!("start {url}: {error}"))?;
        return Ok(());
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&url)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| anyhow::anyhow!("xdg-open {url}: {error}"))?;
        return Ok(());
    }
}

fn percent_decode_query_value(value: &str) -> Result<String, std::string::FromUtf8Error> {
    let mut decoded = Vec::with_capacity(value.len());
    let mut bytes = value.as_bytes().iter().copied();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let hi = bytes.next().unwrap_or(b'0');
            let lo = bytes.next().unwrap_or(b'0');
            if let (Some(hi), Some(lo)) = (query_hex_value(hi), query_hex_value(lo)) {
                decoded.push((hi << 4) | lo);
            }
        } else if byte == b'+' {
            decoded.push(b' ');
        } else {
            decoded.push(byte);
        }
    }
    String::from_utf8(decoded)
}

fn query_hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub async fn serve_ipc(config: IpcConfig) -> anyhow::Result<()> {
    let runtime = IpcRuntime::new(create_app_state(config.allow_host_power_actions));
    serve_ipc_with_runtime(config, runtime).await
}

fn create_app_state(allow_host_power_actions: bool) -> AppState {
    create_app_state_with_auth(
        allow_host_power_actions,
        false,
        HttpServiceMode::ApiOnly,
        None,
    )
}

fn create_app_state_with_auth(
    allow_host_power_actions: bool,
    auth_token_required: bool,
    http_service_mode: HttpServiceMode,
    app_session_secret: Option<Arc<str>>,
) -> AppState {
    create_app_state_with_auth_and_persistence(
        allow_host_power_actions,
        auth_token_required,
        http_service_mode,
        app_session_secret,
        default_devd_persistence(),
    )
}

fn create_app_state_with_auth_and_persistence(
    allow_host_power_actions: bool,
    auth_token_required: bool,
    http_service_mode: HttpServiceMode,
    app_session_secret: Option<Arc<str>>,
    persistence: DevdPersistence,
) -> AppState {
    let (events, _) = broadcast::channel(256);
    let persisted = load_devd_state(&persistence).unwrap_or_else(|error| {
        tracing::warn!("failed to load mains-aegis-devd state: {error}");
        PersistedDevdState::default()
    });
    let state = AppState {
        inner: Arc::new(Mutex::new(DevdState {
            bindings: persisted.bindings,
            selected_artifacts: persisted.selected_artifacts,
            artifacts: persisted.artifacts,
            scan_trace: persisted.scan_trace,
            persisted_device_trace: persisted.device_trace,
            ..DevdState::default()
        })),
        events,
        allow_host_power_actions,
        auth_token_required,
        http_service_mode,
        app_session_secret,
        persistence,
    };
    seed_mock_device(&state);
    spawn_web_lease_reaper(state.clone());
    state
}

#[cfg(not(test))]
fn default_devd_persistence() -> DevdPersistence {
    DevdPersistence::enabled(default_devd_state_file())
}

#[cfg(test)]
fn default_devd_persistence() -> DevdPersistence {
    DevdPersistence::disabled()
}

#[cfg(not(test))]
fn default_devd_state_file() -> PathBuf {
    if let Some(path) = env::var_os(DEVD_STATE_FILE_ENV) {
        return PathBuf::from(path);
    }
    ProjectDirs::from("cc", "mains-aegis", "mains-aegis")
        .map(|dirs| dirs.config_dir().join(DEVD_STATE_FILE_NAME))
        .unwrap_or_else(|| {
            std::env::temp_dir()
                .join(format!("mains-aegis-{}", user_id_hint()))
                .join(DEVD_STATE_FILE_NAME)
        })
}

fn load_devd_state(persistence: &DevdPersistence) -> anyhow::Result<PersistedDevdState> {
    let Some(path) = persistence.state_file.as_ref() else {
        return Ok(PersistedDevdState::default());
    };
    if !path.exists() {
        return Ok(PersistedDevdState::default());
    }
    let text = fs::read_to_string(path)
        .map_err(|error| anyhow::anyhow!("read {}: {error}", path.display()))?;
    serde_json::from_str(&text)
        .map_err(|error| anyhow::anyhow!("parse {}: {error}", path.display()))
}

fn persisted_snapshot(state: &DevdState) -> PersistedDevdState {
    let mut device_trace = state.persisted_device_trace.clone();
    for device in state.devices.values() {
        if !device.trace.is_empty() {
            device_trace.insert(device.id.clone(), bounded_trace_snapshot(&device.trace));
            if let Some(identity_device_id) = device
                .identity
                .as_ref()
                .and_then(|identity| identity.get("device_id"))
                .and_then(Value::as_str)
            {
                device_trace.insert(
                    identity_device_id.to_string(),
                    bounded_trace_snapshot(&device.trace),
                );
            }
        }
    }
    PersistedDevdState {
        schema_version: 1,
        bindings: state.bindings.clone(),
        selected_artifacts: state.selected_artifacts.clone(),
        artifacts: state.artifacts.clone(),
        scan_trace: bounded_trace_snapshot(&state.scan_trace),
        device_trace,
    }
}

fn bounded_trace_snapshot(trace: &VecDeque<SerialTraceEntry>) -> VecDeque<SerialTraceEntry> {
    trace
        .iter()
        .skip(trace.len().saturating_sub(LOG_LIMIT))
        .cloned()
        .collect()
}

fn persist_devd_state(
    persistence: &DevdPersistence,
    snapshot: PersistedDevdState,
) -> Result<(), HttpError> {
    let Some(path) = persistence.state_file.as_ref() else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            HttpError::retryable(
                "state_persist_failed",
                format!("create {}: {error}", parent.display()),
            )
        })?;
    }
    let encoded = serde_json::to_vec_pretty(&snapshot)
        .map_err(|error| HttpError::retryable("state_persist_failed", error.to_string()))?;
    let temp_seq = STATE_PERSIST_TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let temp_path = path.with_extension(format!("json.tmp-{}-{temp_seq}", std::process::id()));
    fs::write(&temp_path, encoded).map_err(|error| {
        HttpError::retryable(
            "state_persist_failed",
            format!("write {}: {error}", temp_path.display()),
        )
    })?;
    match fs::rename(&temp_path, path) {
        Ok(()) => Ok(()),
        Err(first_error) if path.exists() => {
            fs::remove_file(path).map_err(|error| {
                let _ = fs::remove_file(&temp_path);
                HttpError::retryable(
                    "state_persist_failed",
                    format!("replace {}: {error}", path.display()),
                )
            })?;
            fs::rename(&temp_path, path).map_err(|error| {
                let _ = fs::remove_file(&temp_path);
                HttpError::retryable(
                    "state_persist_failed",
                    format!(
                        "replace {}: {error}; first attempt failed: {first_error}",
                        path.display()
                    ),
                )
            })
        }
        Err(error) => {
            let _ = fs::remove_file(&temp_path);
            Err(HttpError::retryable(
                "state_persist_failed",
                format!("replace {}: {error}", path.display()),
            ))
        }
    }
}

async fn serve_ipc_with_runtime(config: IpcConfig, runtime: IpcRuntime) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        serve_ipc_unix(config, runtime).await
    }
    #[cfg(windows)]
    {
        serve_ipc_windows(config, runtime).await
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (config, runtime);
        anyhow::bail!("mains-aegis-devd IPC is unsupported on this platform")
    }
}

#[cfg(unix)]
async fn serve_ipc_unix(config: IpcConfig, runtime: IpcRuntime) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    use tokio::net::UnixListener;

    let path = PathBuf::from(&config.endpoint);
    if let Some(parent) = path.parent() {
        let created_parent = !parent.exists();
        fs::create_dir_all(parent)
            .map_err(|error| anyhow::anyhow!("create {}: {error}", parent.display()))?;
        if created_parent {
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
                .map_err(|error| anyhow::anyhow!("chmod {}: {error}", parent.display()))?;
        }
    }
    if path.exists() {
        remove_stale_ipc_socket(&path).await?;
    }
    let listener = UnixListener::bind(&path)
        .map_err(|error| anyhow::anyhow!("bind IPC {}: {error}", path.display()))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
        .map_err(|error| anyhow::anyhow!("chmod {}: {error}", path.display()))?;
    tracing::info!("mains-aegis-devd IPC listening on {}", path.display());
    let cleanup_path = path.clone();
    loop {
        if let Some(idle_timeout) = config.idle_timeout {
            tokio::select! {
                accepted = listener.accept() => {
                    let (stream, _) = accepted?;
                    spawn_ipc_client(stream, runtime.clone()).await;
                }
                _ = tokio::time::sleep(idle_timeout) => {
                    if ipc_should_shutdown(&runtime, idle_timeout).await {
                        tracing::info!("mains-aegis-devd IPC idle timeout reached; shutting down");
                        break;
                    }
                }
            }
        } else {
            let (stream, _) = listener.accept().await?;
            spawn_ipc_client(stream, runtime.clone()).await;
        }
    }
    let _ = fs::remove_file(cleanup_path);
    Ok(())
}

#[cfg(unix)]
async fn remove_stale_ipc_socket(path: &FsPath) -> anyhow::Result<()> {
    match tokio::net::UnixStream::connect(path).await {
        Ok(_) => anyhow::bail!("IPC endpoint {} is already active", path.display()),
        Err(_) => fs::remove_file(path)
            .map_err(|error| anyhow::anyhow!("remove stale {}: {error}", path.display())),
    }
}

#[cfg(windows)]
async fn serve_ipc_windows(config: IpcConfig, runtime: IpcRuntime) -> anyhow::Result<()> {
    use tokio::net::windows::named_pipe::ServerOptions;

    tracing::info!("mains-aegis-devd IPC listening on {}", config.endpoint);
    loop {
        let server = ServerOptions::new()
            .first_pipe_instance(false)
            .create(&config.endpoint)
            .map_err(|error| anyhow::anyhow!("create IPC pipe {}: {error}", config.endpoint))?;
        if let Some(idle_timeout) = config.idle_timeout {
            tokio::select! {
                connected = server.connect() => {
                    connected.map_err(|error| anyhow::anyhow!("connect IPC pipe client: {error}"))?;
                    spawn_ipc_client(server, runtime.clone()).await;
                }
                _ = tokio::time::sleep(idle_timeout) => {
                    if ipc_should_shutdown(&runtime, idle_timeout).await {
                        tracing::info!("mains-aegis-devd IPC idle timeout reached; shutting down");
                        break;
                    }
                }
            }
        } else {
            server
                .connect()
                .await
                .map_err(|error| anyhow::anyhow!("connect IPC pipe client: {error}"))?;
            spawn_ipc_client(server, runtime.clone()).await;
        }
    }
    Ok(())
}

async fn spawn_ipc_client<S>(stream: S, runtime: IpcRuntime)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    ipc_client_connected(&runtime).await;
    tokio::spawn(async move {
        if let Err(error) = handle_ipc_stream(stream, runtime.clone()).await {
            tracing::warn!("IPC client failed: {error:#}");
        }
        ipc_client_disconnected(&runtime).await;
    });
}

async fn ipc_client_connected(runtime: &IpcRuntime) {
    let mut lifecycle = runtime.lifecycle.lock().await;
    lifecycle.active_clients += 1;
    lifecycle.last_activity = Instant::now();
}

async fn ipc_client_disconnected(runtime: &IpcRuntime) {
    let mut lifecycle = runtime.lifecycle.lock().await;
    lifecycle.active_clients = lifecycle.active_clients.saturating_sub(1);
    lifecycle.last_activity = Instant::now();
}

async fn ipc_mark_activity(runtime: &IpcRuntime) {
    runtime.lifecycle.lock().await.last_activity = Instant::now();
}

async fn ipc_should_shutdown(runtime: &IpcRuntime, idle_timeout: Duration) -> bool {
    let lifecycle = runtime.lifecycle.lock().await;
    lifecycle.active_clients == 0 && lifecycle.last_activity.elapsed() >= idle_timeout
}

async fn handle_ipc_stream<S>(stream: S, runtime: IpcRuntime) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (read, mut write) = tokio::io::split(stream);
    let mut lines = BufReader::new(read).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<IpcRequest>(&line) {
            Ok(request) => handle_ipc_request(&runtime.app, request).await,
            Err(error) => IpcResponse {
                id: "invalid".to_string(),
                ok: false,
                result: None,
                error: Some(format!("invalid IPC request: {error}")),
            },
        };
        let mut encoded = serde_json::to_vec(&response)?;
        encoded.push(b'\n');
        write.write_all(&encoded).await?;
        write.flush().await?;
        ipc_mark_activity(&runtime).await;
    }
    Ok(())
}

async fn handle_ipc_request(state: &AppState, request: IpcRequest) -> IpcResponse {
    let id = request.id;
    match dispatch_ipc_request(state, &request.method, request.params).await {
        Ok(result) => IpcResponse {
            id,
            ok: true,
            result: Some(result),
            error: None,
        },
        Err(error) => IpcResponse {
            id,
            ok: false,
            result: None,
            error: Some(error.to_string()),
        },
    }
}

async fn dispatch_ipc_request(
    state: &AppState,
    method: &str,
    params: Value,
) -> anyhow::Result<Value> {
    match method {
        "devd.health" => Ok(json!({"ok": true, "daemon": "mains-aegis-devd"})),
        "devices.list" => Ok(list_devices(State(state.clone())).await.0),
        "devices.scan" => {
            let query: ScanDevicesQuery = serde_json::from_value(params)?;
            json_result(scan_devices_inner(state, query).await)
        }
        "devices.scan_trace" => {
            let query = ScanTraceQuery {
                trace_limit: params
                    .get("trace_limit")
                    .and_then(Value::as_u64)
                    .map(usize::try_from)
                    .transpose()?,
            };
            json_result(scan_trace(Query(query), State(state.clone())).await)
        }
        "device.bind" => {
            let id = require_param(&params, "device_id")?;
            let input: BindRequest = serde_json::from_value(params)?;
            json_result(bind_device(State(state.clone()), Path(id), Json(input)).await)
        }
        "device.unbind" => {
            let id = require_param(&params, "device_id")?;
            json_result(unbind_device(State(state.clone()), Path(id)).await)
        }
        "device.identity" => {
            let id = require_param(&params, "device_id")?;
            json_result(device_identity(State(state.clone()), Path(id)).await)
        }
        "device.settings" => {
            let id = require_param(&params, "device_id")?;
            json_result(device_settings(State(state.clone()), Path(id)).await)
        }
        "device.trace" => {
            let id = require_param(&params, "device_id")?;
            let query = SessionQuery {
                logs_limit: params
                    .get("logs_limit")
                    .and_then(Value::as_u64)
                    .map(usize::try_from)
                    .transpose()?,
                trace_limit: params
                    .get("trace_limit")
                    .and_then(Value::as_u64)
                    .map(usize::try_from)
                    .transpose()?,
                lease_id: params
                    .get("lease_id")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            };
            json_result(device_trace(Query(query), State(state.clone()), Path(id)).await)
        }
        "device.connect" => {
            let id = require_param(&params, "device_id")?;
            json_result(connect_device(State(state.clone()), Path(id)).await)
        }
        "device.disconnect" => {
            let id = require_param(&params, "device_id")?;
            json_result(disconnect_device(State(state.clone()), Path(id)).await)
        }
        "device.connection" => {
            let id = require_param(&params, "device_id")?;
            json_result(device_connection(State(state.clone()), Path(id)).await)
        }
        "device.artifact.select" => {
            let id = require_param(&params, "device_id")?;
            let input: ArtifactSelectRequest = serde_json::from_value(params)?;
            json_result(select_artifact(State(state.clone()), Path(id), Json(input)).await)
        }
        "device.artifact.get" => {
            let id = require_param(&params, "device_id")?;
            json_result(device_artifact(State(state.clone()), Path(id)).await)
        }
        "device.flash" => {
            let id = require_param(&params, "device_id")?;
            let input: FlashRequest = serde_json::from_value(params)?;
            json_result(flash_device(State(state.clone()), Path(id), Json(input)).await)
        }
        "device.reset" => {
            let id = require_param(&params, "device_id")?;
            json_result(reset_device(State(state.clone()), Path(id)).await)
        }
        "device.monitor.start" => {
            let id = require_param(&params, "device_id")?;
            json_result(monitor_start(State(state.clone()), Path(id)).await)
        }
        "device.monitor.stop" => {
            let id = require_param(&params, "device_id")?;
            json_result(monitor_stop(State(state.clone()), Path(id)).await)
        }
        "serial.lease.create" => {
            let input: WebLeaseCreateRequest = serde_json::from_value(params)?;
            json_result(create_web_lease(State(state.clone()), Json(input)).await)
        }
        "serial.lease.heartbeat" => {
            let lease_id = require_param(&params, "lease_id")?;
            json_result(heartbeat_web_lease(State(state.clone()), Path(lease_id)).await)
        }
        "serial.lease.release" => {
            let lease_id = require_param(&params, "lease_id")?;
            json_result(release_web_lease(State(state.clone()), Path(lease_id)).await)
        }
        "host.power.status" => Ok(host_power_status(State(state.clone())).await.0),
        "host.power.profile" => {
            let input: HostPowerProfileRequest = serde_json::from_value(params)?;
            json_result(host_power_profile(State(state.clone()), Json(input)).await)
        }
        "host.power.suspend" => {
            let input =
                if params.is_null() || params.as_object().is_some_and(|object| object.is_empty()) {
                    None
                } else {
                    Some(Json(serde_json::from_value(params)?))
                };
            json_result(host_power_suspend(State(state.clone()), input).await)
        }
        "host.power.shutdown" => {
            let input =
                if params.is_null() || params.as_object().is_some_and(|object| object.is_empty()) {
                    None
                } else {
                    Some(Json(serde_json::from_value(params)?))
                };
            json_result(host_power_shutdown(State(state.clone()), input).await)
        }
        "settings.wifi.set" => {
            let input: WifiConfigRequest = serde_json::from_value(params)?;
            json_result(set_wifi_config(State(state.clone()), Json(input)).await)
        }
        "settings.wifi.clear" => {
            let query = SettingsTargetQuery {
                device_id: params
                    .get("device_id")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                lease_id: params
                    .get("lease_id")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            };
            json_result(clear_wifi_config(State(state.clone()), Query(query)).await)
        }
        "settings.log_level.set" => {
            let input: LogLevelRequest = serde_json::from_value(params)?;
            json_result(set_log_level(State(state.clone()), Json(input)).await)
        }
        "settings.manual_charge.set" => {
            let input: ManualChargeRequest = serde_json::from_value(params)?;
            json_result(set_manual_charge(State(state.clone()), Json(input)).await)
        }
        _ => anyhow::bail!("unsupported IPC method: {method}"),
    }
}

fn json_result<T>(result: Result<Json<T>, HttpError>) -> anyhow::Result<Value>
where
    T: Serialize,
{
    result
        .map(|Json(value)| serde_json::to_value(value))
        .map_err(|error| anyhow::anyhow!("{error}"))?
        .map_err(Into::into)
}

fn require_param(params: &Value, key: &str) -> anyhow::Result<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("missing required parameter: {key}"))
}

pub async fn ipc_call(endpoint: &str, method: &str, params: Value) -> anyhow::Result<Value> {
    let request = IpcRequest {
        id: next_id(),
        method: method.to_string(),
        params,
    };
    #[cfg(unix)]
    {
        let stream = tokio::net::UnixStream::connect(endpoint)
            .await
            .map_err(|error| anyhow::anyhow!("connect IPC socket {endpoint}: {error}"))?;
        send_ipc_request(stream, request).await
    }
    #[cfg(windows)]
    {
        let stream = tokio::net::windows::named_pipe::ClientOptions::new()
            .open(endpoint)
            .map_err(|error| anyhow::anyhow!("connect IPC pipe {endpoint}: {error}"))?;
        send_ipc_request(stream, request).await
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (endpoint, request);
        anyhow::bail!("mains-aegis IPC is unsupported on this platform")
    }
}

async fn send_ipc_request<S>(mut stream: S, request: IpcRequest) -> anyhow::Result<Value>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut encoded = serde_json::to_vec(&request)?;
    encoded.push(b'\n');
    stream.write_all(&encoded).await?;
    stream.flush().await?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    let response: IpcResponse = serde_json::from_str(line.trim())?;
    if response.ok {
        Ok(response.result.unwrap_or_else(|| json!({})))
    } else {
        anyhow::bail!(
            "{}",
            response
                .error
                .unwrap_or_else(|| "IPC request failed".to_string())
        )
    }
}

async fn bootstrap(State(state): State<AppState>) -> Json<Value> {
    let mode = match state.http_service_mode {
        HttpServiceMode::HostedApp => "http_service",
        HttpServiceMode::ApiOnly => "http_service_api_only",
    };
    Json(json!({
        "token_required": state.auth_token_required,
        "agent_base_url": "",
        "app": {
            "name": "mains-aegis-devd",
            "version": release_version(),
            "mode": mode
        }
    }))
}

async fn health() -> Json<Value> {
    Json(json!({"ok": true, "daemon": "mains-aegis-devd"}))
}

async fn list_devices(State(state): State<AppState>) -> Json<Value> {
    let guard = state.inner.lock().expect("state lock");
    Json(json!({
        "devices": guard.devices.values().cloned().collect::<Vec<_>>(),
        "bindings": guard.bindings.values().cloned().collect::<Vec<_>>()
    }))
}

async fn scan_devices(
    State(state): State<AppState>,
    Query(query): Query<ScanDevicesQuery>,
) -> Result<Json<Value>, HttpError> {
    scan_devices_inner(&state, query).await
}

async fn scan_devices_inner(
    state: &AppState,
    query: ScanDevicesQuery,
) -> Result<Json<Value>, HttpError> {
    let ports = serialport::available_ports()
        .map_err(|error| HttpError::retryable("serial_scan_failed", error.to_string()))?;
    let mut discovered_ids = HashSet::new();
    let snapshot = {
        let mut guard = state.inner.lock().expect("state lock");
        let mut seen_native_ids = HashSet::new();
        for port in ports {
            if !is_native_usb_serial_candidate(&port) {
                continue;
            }
            let id = stable_device_id(&port);
            seen_native_ids.insert(id.clone());
            let port_path = port.port_name.clone();
            {
                let binding = guard.bindings.get(&id).cloned();
                let selected_artifact_id = guard.selected_artifacts.get(&id).cloned();
                let persisted_trace = guard
                    .persisted_device_trace
                    .get(&id)
                    .cloned()
                    .unwrap_or_default();
                let entry = guard
                    .devices
                    .entry(id.clone())
                    .or_insert_with(|| DeviceRecord {
                        id: id.clone(),
                        display_name: port_path.clone(),
                        port_path: Some(port_path.clone()),
                        lan_address: None,
                        lan_conflict_addresses: Vec::new(),
                        transport: DeviceTransport::NativeSerial,
                        binding,
                        connection: ConnectionState::Disconnected,
                        identity: None,
                        status: None,
                        power_diag: None,
                        selected_artifact_id,
                        log_decode: LogDecodeState::default(),
                        settings: default_settings(),
                        logs: VecDeque::new(),
                        trace: persisted_trace,
                    });
                let preferred_path =
                    prefer_serial_port_path(entry.port_path.as_deref(), &port_path);
                entry.display_name = preferred_path.clone();
                entry.port_path = Some(preferred_path.clone());
                if let Some(binding) = entry.binding.as_mut() {
                    binding.port_path = Some(preferred_path.clone());
                }
                if let Some(binding) = guard.bindings.get_mut(&id) {
                    binding.port_path = Some(preferred_path);
                }
            }
        }
        discovered_ids.extend(seen_native_ids.iter().cloned());
        let stale_ids = guard
            .devices
            .iter()
            .filter_map(|(id, device)| {
                native_device_stable_id(device)
                    .is_some_and(|stable_id| !seen_native_ids.contains(stable_id))
                    .then(|| id.clone())
            })
            .collect::<Vec<_>>();
        for id in stale_ids {
            if let Some(device) = guard.devices.get_mut(&id) {
                device.port_path = None;
                device.connection = ConnectionState::Disconnected;
                if let Some(binding) = device.binding.as_mut() {
                    binding.port_path = None;
                }
            }
            if let Some(binding) = guard.bindings.get_mut(&id) {
                binding.port_path = None;
            }
        }
        persisted_snapshot(&guard)
    };
    persist_devd_state(&state.persistence, snapshot)?;
    let (lan_discoveries, mut scan_trace) = if query.lan.unwrap_or(true) {
        discover_lan_devices(&query).await?
    } else {
        (Vec::new(), Vec::new())
    };
    let mut lan_count = 0usize;
    let mut runtime_snapshot = None;
    if !lan_discoveries.is_empty() || !scan_trace.is_empty() {
        let mut guard = state.inner.lock().expect("state lock");
        merge_lan_discoveries(
            &mut guard,
            lan_discoveries,
            &mut discovered_ids,
            &mut lan_count,
        );
        let trace_count = scan_trace.len();
        for trace in scan_trace.drain(..) {
            push_bounded(&mut guard.scan_trace, trace, LOG_LIMIT);
        }
        push_bounded(
            &mut guard.scan_trace,
            structured_trace_entry(
                "rx",
                "scan",
                Some("lan-discovery".to_string()),
                "LAN scan completed",
                json!({
                    "lan_count": lan_count,
                    "trace_count": trace_count,
                    "cidr": query.cidr.as_deref().or(query.lan_cidr.as_deref())
                })
                .to_string(),
            ),
            LOG_LIMIT,
        );
        runtime_snapshot = Some(persisted_snapshot(&guard));
    }
    if let Some(snapshot) = runtime_snapshot {
        persist_devd_state(&state.persistence, snapshot)?;
    }
    let guard = state.inner.lock().expect("state lock");
    let mut discovered = discovered_ids
        .iter()
        .filter_map(|id| guard.devices.get(id).cloned())
        .collect::<Vec<_>>();
    discovered.sort_by(|left, right| left.id.cmp(&right.id));
    let scan_trace_tail = tail(&guard.scan_trace, 200);
    drop(guard);
    emit(
        state,
        None,
        "scan",
        "device scan completed",
        json!({"count": discovered.len(), "lan_count": lan_count}),
    );
    Ok(Json(
        json!({"devices": discovered, "scan_trace": scan_trace_tail}),
    ))
}

async fn scan_trace(
    Query(query): Query<ScanTraceQuery>,
    State(state): State<AppState>,
) -> Result<Json<Value>, HttpError> {
    let guard = state.inner.lock().expect("state lock");
    Ok(Json(json!({
        "trace": tail(&guard.scan_trace, query.trace_limit.unwrap_or(600).min(2_000))
    })))
}

#[derive(Debug)]
struct LanDeviceDiscovery {
    address: String,
    identity: Value,
    trace: Vec<SerialTraceEntry>,
}

async fn discover_lan_devices(
    query: &ScanDevicesQuery,
) -> Result<(Vec<LanDeviceDiscovery>, Vec<SerialTraceEntry>), HttpError> {
    let mut scan_trace = vec![structured_trace_entry(
        "tx",
        "scan",
        Some("lan-discovery".to_string()),
        "LAN discovery started",
        json!({
            "mdns": query.mdns.unwrap_or(true),
            "cidr": query.cidr.as_deref().or(query.lan_cidr.as_deref()),
            "concurrency": LAN_SCAN_CONCURRENCY,
            "probe_timeout_ms": LAN_PROBE_TIMEOUT_MS
        })
        .to_string(),
    )];
    let mut candidates = HashSet::new();
    if query.mdns.unwrap_or(true) {
        match discover_mdns_ipv4_candidates().await {
            Ok(addresses) => {
                for address in addresses {
                    candidates.insert(address);
                }
                scan_trace.push(structured_trace_entry(
                    "rx",
                    "mdns",
                    Some(LAN_DISCOVERY_SERVICE.to_string()),
                    "mDNS discovery completed",
                    json!({"candidate_count": candidates.len()}).to_string(),
                ));
            }
            Err(error) => scan_trace.push(structured_trace_entry(
                "rx",
                "mdns",
                Some(LAN_DISCOVERY_SERVICE.to_string()),
                "mDNS discovery failed",
                json!({"error": error.to_string()}).to_string(),
            )),
        }
    }
    let cidr = query.cidr.as_deref().or(query.lan_cidr.as_deref());
    if let Some(cidr) = cidr {
        for address in ipv4_hosts_from_cidr(cidr)? {
            candidates.insert(address);
        }
    } else if candidates.is_empty() {
        if let Some(default_cidr) = default_lan_scan_cidr() {
            for address in ipv4_hosts_from_cidr(&default_cidr)? {
                candidates.insert(address);
            }
            scan_trace.push(structured_trace_entry(
                "tx",
                "scan",
                Some(default_cidr.clone()),
                "Using default routed /24 LAN scan",
                json!({"cidr": default_cidr}).to_string(),
            ));
        }
    }
    let mut candidates = candidates.into_iter().collect::<Vec<_>>();
    candidates.sort();
    scan_trace.push(structured_trace_entry(
        "tx",
        "scan",
        Some("lan-probe".to_string()),
        "LAN identity probes queued",
        json!({"candidate_count": candidates.len()}).to_string(),
    ));
    let discoveries = probe_lan_candidates(candidates).await;
    scan_trace.push(structured_trace_entry(
        "rx",
        "scan",
        Some("lan-probe".to_string()),
        "LAN identity probes completed",
        json!({"device_count": discoveries.len()}).to_string(),
    ));
    Ok((discoveries, scan_trace))
}

async fn probe_lan_candidates(candidates: Vec<Ipv4Addr>) -> Vec<LanDeviceDiscovery> {
    let mut discoveries = Vec::new();
    let mut join_set = tokio::task::JoinSet::new();
    let mut next = candidates.into_iter();
    loop {
        while join_set.len() < LAN_SCAN_CONCURRENCY {
            let Some(address) = next.next() else {
                break;
            };
            join_set.spawn(async move { probe_lan_identity(address).await });
        }
        if join_set.is_empty() {
            break;
        }
        if let Some(Ok(Some(discovery))) = join_set.join_next().await {
            discoveries.push(discovery);
        }
    }
    discoveries
}

async fn probe_lan_identity(address: Ipv4Addr) -> Option<LanDeviceDiscovery> {
    let target = format!("http://{address}/api/v1/identity");
    let mut trace = vec![structured_trace_entry(
        "tx",
        "http",
        Some(target.clone()),
        "GET /api/v1/identity",
        "GET /api/v1/identity".to_string(),
    )];
    let socket = tokio::time::timeout(
        Duration::from_millis(LAN_PROBE_TIMEOUT_MS),
        tokio::net::TcpStream::connect((address, LAN_DISCOVERY_PORT)),
    )
    .await
    .ok()?
    .ok()?;
    let mut stream = socket;
    let request = format!(
        "GET /api/v1/identity HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    tokio::time::timeout(
        Duration::from_millis(LAN_PROBE_TIMEOUT_MS),
        stream.write_all(request.as_bytes()),
    )
    .await
    .ok()?
    .ok()?;
    let mut response = Vec::new();
    tokio::time::timeout(
        Duration::from_millis(LAN_PROBE_TIMEOUT_MS),
        tokio::io::AsyncReadExt::read_to_end(&mut stream, &mut response),
    )
    .await
    .ok()?
    .ok()?;
    let text = String::from_utf8_lossy(&response);
    if !text.starts_with("HTTP/1.1 200") && !text.starts_with("HTTP/1.0 200") {
        return None;
    }
    let (_, body) = text.split_once("\r\n\r\n")?;
    let identity = serde_json::from_str::<Value>(body.trim()).ok()?;
    identity.get("device_id").and_then(Value::as_str)?;
    trace.push(structured_trace_entry(
        "rx",
        "http",
        Some(target),
        "identity response",
        body.trim().to_string(),
    ));
    Some(LanDeviceDiscovery {
        address: address.to_string(),
        identity,
        trace,
    })
}

async fn lan_http_json(
    address: &str,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> Result<Value, HttpError> {
    let mut stream = tokio::time::timeout(
        Duration::from_millis(LAN_PROBE_TIMEOUT_MS),
        tokio::net::TcpStream::connect((address, LAN_DISCOVERY_PORT)),
    )
    .await
    .map_err(|_| {
        HttpError::retryable(
            "lan_http_connect_timeout",
            format!("timed out connecting to {address}:80"),
        )
    })?
    .map_err(|error| {
        HttpError::retryable(
            "lan_http_connect_failed",
            format!("failed to connect to {address}:80: {error}"),
        )
    })?;
    let encoded_body = body.map(Value::to_string).unwrap_or_default();
    let request = if encoded_body.is_empty() {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
        )
    } else {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            encoded_body.len(),
            encoded_body
        )
    };
    tokio::time::timeout(
        Duration::from_millis(LAN_PROBE_TIMEOUT_MS),
        stream.write_all(request.as_bytes()),
    )
    .await
    .map_err(|_| {
        HttpError::retryable(
            "lan_http_write_timeout",
            format!("timed out writing {method} {path} to {address}"),
        )
    })?
    .map_err(|error| {
        HttpError::retryable(
            "lan_http_write_failed",
            format!("failed to write {method} {path} to {address}: {error}"),
        )
    })?;
    let mut response = Vec::new();
    tokio::time::timeout(
        Duration::from_millis(LAN_PROBE_TIMEOUT_MS),
        tokio::io::AsyncReadExt::read_to_end(&mut stream, &mut response),
    )
    .await
    .map_err(|_| {
        HttpError::retryable(
            "lan_http_read_timeout",
            format!("timed out reading {method} {path} from {address}"),
        )
    })?
    .map_err(|error| {
        HttpError::retryable(
            "lan_http_read_failed",
            format!("failed to read {method} {path} from {address}: {error}"),
        )
    })?;
    parse_lan_http_json_response(&response, method, path, address)
}

fn parse_lan_http_json_response(
    response: &[u8],
    method: &str,
    path: &str,
    address: &str,
) -> Result<Value, HttpError> {
    let text = String::from_utf8_lossy(response);
    let (head, body) = text.split_once("\r\n\r\n").ok_or_else(|| {
        HttpError::retryable(
            "lan_http_response_invalid",
            format!("{method} {path} from {address} did not return HTTP headers"),
        )
    })?;
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or(0);
    if !(200..300).contains(&status) {
        return Err(HttpError::retryable(
            "lan_http_status_failed",
            format!("{method} {path} from {address} returned HTTP {status}"),
        ));
    }
    if body.trim().is_empty() {
        return Ok(json!({ "ok": true, "status": status }));
    }
    serde_json::from_str::<Value>(body.trim()).map_err(|error| {
        HttpError::retryable(
            "lan_http_json_invalid",
            format!("{method} {path} from {address} returned invalid JSON: {error}"),
        )
    })
}

fn settings_state_from_api(value: &Value) -> Result<DeviceSettingsState, HttpError> {
    let wifi = value.get("wifi").ok_or_else(|| {
        HttpError::retryable(
            "settings_snapshot_invalid",
            "settings snapshot is missing wifi",
        )
    })?;
    let manual_charge = value.get("manual_charge").ok_or_else(|| {
        HttpError::retryable(
            "settings_snapshot_invalid",
            "settings snapshot is missing manual_charge",
        )
    })?;
    Ok(DeviceSettingsState {
        wifi_configured: wifi.get("configured").and_then(Value::as_bool),
        wifi_ssid: wifi.get("ssid").and_then(Value::as_str).map(str::to_string),
        log_level: value
            .get("log_level")
            .and_then(Value::as_str)
            .unwrap_or("info")
            .to_string(),
        manual_charge: ManualChargePrefs {
            target: manual_charge
                .get("target")
                .and_then(Value::as_str)
                .unwrap_or("full_100")
                .to_string(),
            speed: manual_charge
                .get("speed")
                .and_then(Value::as_str)
                .unwrap_or("ma_500")
                .to_string(),
            timer_h: manual_charge
                .get("timer_h")
                .and_then(Value::as_u64)
                .and_then(|value| u8::try_from(value).ok())
                .unwrap_or(2),
        },
    })
}

fn redact_lan_body(body: &Value) -> Value {
    let mut redacted = body.clone();
    if let Some(object) = redacted.as_object_mut() {
        if object.contains_key("psk") {
            object.insert("psk".to_string(), Value::String("[redacted]".to_string()));
        }
    }
    redacted
}

fn merge_lan_discoveries(
    state: &mut DevdState,
    discoveries: Vec<LanDeviceDiscovery>,
    discovered_ids: &mut HashSet<String>,
    lan_count: &mut usize,
) {
    let mut by_identity: HashMap<String, Vec<LanDeviceDiscovery>> = HashMap::new();
    for discovery in discoveries {
        if let Some(identity_device_id) = discovery
            .identity
            .get("device_id")
            .and_then(Value::as_str)
            .map(str::to_string)
        {
            by_identity
                .entry(identity_device_id)
                .or_default()
                .push(discovery);
        }
    }
    for (identity_device_id, mut group) in by_identity {
        group.sort_by(|left, right| left.address.cmp(&right.address));
        let conflict_addresses = group
            .iter()
            .map(|discovery| discovery.address.clone())
            .collect::<Vec<_>>();
        let discovery = group.remove(0);
        let record_id = find_logical_device_id_for_identity(state, &identity_device_id)
            .unwrap_or_else(|| identity_device_id.clone());
        let selected_artifact_id = state.selected_artifacts.get(&record_id).cloned();
        let selected_artifact = selected_artifact_id
            .as_ref()
            .and_then(|artifact_id| state.artifacts.get(artifact_id).cloned());
        let persisted_trace = state
            .persisted_device_trace
            .get(&record_id)
            .or_else(|| state.persisted_device_trace.get(&identity_device_id))
            .cloned()
            .unwrap_or_default();
        let device = state
            .devices
            .entry(record_id.clone())
            .or_insert_with(|| DeviceRecord {
                id: record_id.clone(),
                display_name: discovery
                    .identity
                    .get("hostname")
                    .and_then(Value::as_str)
                    .unwrap_or(&identity_device_id)
                    .to_string(),
                port_path: None,
                lan_address: None,
                lan_conflict_addresses: Vec::new(),
                transport: DeviceTransport::Lan,
                binding: state.bindings.get(&record_id).cloned(),
                connection: ConnectionState::Disconnected,
                identity: None,
                status: None,
                power_diag: None,
                selected_artifact_id,
                log_decode: LogDecodeState::default(),
                settings: default_settings(),
                logs: VecDeque::new(),
                trace: persisted_trace,
            });
        device.identity = Some(discovery.identity);
        device.lan_address = Some(discovery.address.clone());
        device.lan_conflict_addresses = if conflict_addresses.len() > 1 {
            conflict_addresses
        } else {
            Vec::new()
        };
        if matches!(device.transport, DeviceTransport::Lan) {
            device.connection = if device.lan_conflict_addresses.is_empty() {
                ConnectionState::Connected
            } else {
                ConnectionState::Error
            };
        }
        for trace in discovery.trace {
            push_bounded(&mut device.trace, trace, LOG_LIMIT);
        }
        apply_artifact_match(device, selected_artifact.as_ref());
        discovered_ids.insert(record_id);
        *lan_count += 1;
    }
}

fn device_identity_device_id(device: &DeviceRecord) -> Option<&str> {
    device
        .identity
        .as_ref()
        .and_then(|identity| identity.get("device_id"))
        .and_then(Value::as_str)
}

fn device_logical_device_id(device: &DeviceRecord) -> Option<&str> {
    device
        .binding
        .as_ref()
        .and_then(|binding| binding.logical_device_id.as_deref())
        .or_else(|| device_identity_device_id(device))
}

fn native_device_stable_id(device: &DeviceRecord) -> Option<&str> {
    if !matches!(device.transport, DeviceTransport::NativeSerial) {
        return None;
    }
    device
        .binding
        .as_ref()
        .map(|binding| binding.stable_id.as_str())
        .or(Some(device.id.as_str()))
}

fn find_logical_device_id_for_identity(
    state: &DevdState,
    identity_device_id: &str,
) -> Option<String> {
    let mut devices = state.devices.values().collect::<Vec<_>>();
    devices.sort_by(|left, right| {
        transport_preference(&left.transport)
            .cmp(&transport_preference(&right.transport))
            .then_with(|| left.id.cmp(&right.id))
    });
    devices
        .into_iter()
        .find(|device| device_matches_identity_id(device, identity_device_id))
        .map(|device| device.id.clone())
}

fn transport_preference(transport: &DeviceTransport) -> u8 {
    match transport {
        DeviceTransport::NativeSerial => 0,
        DeviceTransport::Lan => 1,
        DeviceTransport::Mock => 2,
    }
}

fn transport_name(transport: &DeviceTransport) -> &'static str {
    match transport {
        DeviceTransport::NativeSerial => "usb",
        DeviceTransport::Lan => "lan",
        DeviceTransport::Mock => "mock",
    }
}

fn available_transports(device: &DeviceRecord) -> Vec<&'static str> {
    match device.transport {
        DeviceTransport::NativeSerial => {
            if device.lan_address.is_some() {
                vec!["usb", "lan"]
            } else {
                vec!["usb"]
            }
        }
        DeviceTransport::Lan => vec!["lan"],
        DeviceTransport::Mock => vec!["mock"],
    }
}

fn connection_transports(device: &DeviceRecord) -> Value {
    json!({
        "usb": {
            "available": device.port_path.is_some() || matches!(device.transport, DeviceTransport::NativeSerial),
            "active": matches!(device.transport, DeviceTransport::NativeSerial),
            "connected": matches!(device.transport, DeviceTransport::NativeSerial) && matches!(device.connection, ConnectionState::Connected),
            "port_path": device.port_path,
            "last_error": if matches!(device.transport, DeviceTransport::NativeSerial) && matches!(device.connection, ConnectionState::Error) {
                Some("usb_connection_error")
            } else if matches!(device.transport, DeviceTransport::NativeSerial) && device.port_path.is_none() {
                Some("usb_port_missing")
            } else {
                None
            }
        },
        "lan": {
            "available": device.lan_address.is_some(),
            "active": matches!(device.transport, DeviceTransport::Lan),
            "connected": device.lan_address.is_some() && device.lan_conflict_addresses.is_empty() && (
                matches!(device.transport, DeviceTransport::Lan) && matches!(device.connection, ConnectionState::Connected)
                    || matches!(device.transport, DeviceTransport::NativeSerial)
            ),
            "address": device.lan_address,
            "conflict_addresses": device.lan_conflict_addresses,
            "last_error": if !device.lan_conflict_addresses.is_empty() {
                Some("lan_identity_conflict")
            } else if matches!(device.transport, DeviceTransport::Lan) && matches!(device.connection, ConnectionState::Error) {
                Some("lan_connection_error")
            } else {
                None
            }
        },
        "mock": {
            "available": matches!(device.transport, DeviceTransport::Mock),
            "active": matches!(device.transport, DeviceTransport::Mock),
            "connected": matches!(device.transport, DeviceTransport::Mock) && matches!(device.connection, ConnectionState::Connected)
        }
    })
}

fn connection_switch_hint(device: &DeviceRecord) -> Option<&'static str> {
    if !device.lan_conflict_addresses.is_empty() {
        return Some("LAN identity conflict blocks automatic LAN access; use USB or resolve the duplicate addresses");
    }
    if matches!(device.transport, DeviceTransport::Lan) && device.port_path.is_some() {
        return Some(
            "USB is available and remains the default transport for state-changing operations",
        );
    }
    if matches!(device.transport, DeviceTransport::NativeSerial) && device.lan_address.is_some() {
        return Some("LAN is available; switching away from USB should be explicit");
    }
    None
}

fn merge_lan_record_into_primary(
    state: &mut DevdState,
    primary_id: &str,
    identity_device_id: &str,
) {
    let Some(lan_id) = state.devices.iter().find_map(|(id, device)| {
        (id != primary_id
            && matches!(device.transport, DeviceTransport::Lan)
            && device_matches_identity_id(device, identity_device_id))
        .then(|| id.clone())
    }) else {
        return;
    };
    let Some(lan_record) = state.devices.remove(&lan_id) else {
        return;
    };
    if let Some(primary) = state.devices.get_mut(primary_id) {
        primary.lan_address = lan_record.lan_address;
        primary.lan_conflict_addresses = lan_record.lan_conflict_addresses;
        for trace in lan_record.trace {
            push_bounded(&mut primary.trace, trace, LOG_LIMIT);
        }
        for log in lan_record.logs {
            push_bounded(&mut primary.logs, log, LOG_LIMIT);
        }
    }
}

fn attach_persisted_trace_for_identity(
    state: &mut DevdState,
    device_id: &str,
    identity_device_id: &str,
) {
    let Some(trace) = state
        .persisted_device_trace
        .get(identity_device_id)
        .cloned()
        .filter(|trace| !trace.is_empty())
    else {
        return;
    };
    if let Some(device) = state.devices.get_mut(device_id) {
        if device.trace.is_empty() {
            device.trace = trace;
        }
    }
}

async fn discover_mdns_ipv4_candidates() -> anyhow::Result<Vec<Ipv4Addr>> {
    let socket = tokio::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
    socket.set_multicast_ttl_v4(2)?;
    let query = mdns_ptr_query(LAN_DISCOVERY_SERVICE);
    socket
        .send_to(&query, (Ipv4Addr::new(224, 0, 0, 251), 5353))
        .await?;
    let deadline = tokio::time::Instant::now() + Duration::from_millis(700);
    let mut addresses = HashSet::new();
    let mut buffer = [0u8; 1500];
    loop {
        let Some(remaining) = deadline.checked_duration_since(tokio::time::Instant::now()) else {
            break;
        };
        match tokio::time::timeout(remaining, socket.recv_from(&mut buffer)).await {
            Ok(Ok((size, _))) => {
                for address in mdns_a_records(&buffer[..size]) {
                    addresses.insert(address);
                }
            }
            _ => break,
        }
    }
    let mut addresses = addresses.into_iter().collect::<Vec<_>>();
    addresses.sort();
    Ok(addresses)
}

fn mdns_ptr_query(service: &str) -> Vec<u8> {
    let mut packet = Vec::new();
    packet.extend_from_slice(&0u16.to_be_bytes());
    packet.extend_from_slice(&0u16.to_be_bytes());
    packet.extend_from_slice(&1u16.to_be_bytes());
    packet.extend_from_slice(&0u16.to_be_bytes());
    packet.extend_from_slice(&0u16.to_be_bytes());
    packet.extend_from_slice(&0u16.to_be_bytes());
    for label in service.trim_end_matches('.').split('.') {
        packet.push(label.len() as u8);
        packet.extend_from_slice(label.as_bytes());
    }
    packet.push(0);
    packet.extend_from_slice(&12u16.to_be_bytes());
    packet.extend_from_slice(&1u16.to_be_bytes());
    packet
}

fn mdns_a_records(packet: &[u8]) -> Vec<Ipv4Addr> {
    if packet.len() < 12 {
        return Vec::new();
    }
    let qdcount = u16::from_be_bytes([packet[4], packet[5]]) as usize;
    let ancount = u16::from_be_bytes([packet[6], packet[7]]) as usize;
    let nscount = u16::from_be_bytes([packet[8], packet[9]]) as usize;
    let arcount = u16::from_be_bytes([packet[10], packet[11]]) as usize;
    let mut offset = 12usize;
    for _ in 0..qdcount {
        let Some(next) = dns_skip_name(packet, offset) else {
            return Vec::new();
        };
        offset = next.saturating_add(4);
        if offset > packet.len() {
            return Vec::new();
        }
    }
    let mut addresses = Vec::new();
    for _ in 0..(ancount + nscount + arcount) {
        let Some(next) = dns_skip_name(packet, offset) else {
            break;
        };
        if next + 10 > packet.len() {
            break;
        }
        let record_type = u16::from_be_bytes([packet[next], packet[next + 1]]);
        let class = u16::from_be_bytes([packet[next + 2], packet[next + 3]]) & 0x7fff;
        let rdlen = u16::from_be_bytes([packet[next + 8], packet[next + 9]]) as usize;
        let rdata = next + 10;
        if rdata + rdlen > packet.len() {
            break;
        }
        if record_type == 1 && class == 1 && rdlen == 4 {
            addresses.push(Ipv4Addr::new(
                packet[rdata],
                packet[rdata + 1],
                packet[rdata + 2],
                packet[rdata + 3],
            ));
        }
        offset = rdata + rdlen;
    }
    addresses
}

fn dns_skip_name(packet: &[u8], mut offset: usize) -> Option<usize> {
    let mut jumps = 0usize;
    loop {
        let length = *packet.get(offset)?;
        if length & 0xc0 == 0xc0 {
            packet.get(offset + 1)?;
            return Some(offset + 2);
        }
        offset += 1;
        if length == 0 {
            return Some(offset);
        }
        if length & 0xc0 != 0 {
            return None;
        }
        offset = offset.checked_add(length as usize)?;
        if offset > packet.len() {
            return None;
        }
        jumps += 1;
        if jumps > 128 {
            return None;
        }
    }
}

fn ipv4_hosts_from_cidr(cidr: &str) -> Result<Vec<Ipv4Addr>, HttpError> {
    let (address, prefix) = cidr.split_once('/').ok_or_else(|| {
        HttpError::non_retryable(
            "lan_cidr_invalid",
            "LAN scan CIDR must look like 192.168.1.0/24",
        )
    })?;
    let address = address.parse::<Ipv4Addr>().map_err(|error| {
        HttpError::non_retryable("lan_cidr_invalid", format!("invalid IPv4 address: {error}"))
    })?;
    let prefix = prefix.parse::<u8>().map_err(|error| {
        HttpError::non_retryable("lan_cidr_invalid", format!("invalid CIDR prefix: {error}"))
    })?;
    if prefix > 32 {
        return Err(HttpError::non_retryable(
            "lan_cidr_invalid",
            "CIDR prefix must be <= 32",
        ));
    }
    let host_count = if prefix == 32 {
        1usize
    } else {
        1usize << (32 - prefix)
    };
    if host_count > LAN_SCAN_MAX_HOSTS {
        return Err(HttpError::non_retryable(
            "lan_cidr_too_large",
            format!(
                "LAN scan CIDR contains {host_count} addresses; maximum is {LAN_SCAN_MAX_HOSTS}"
            ),
        ));
    }
    let mask = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    let network = u32::from(address) & mask;
    let mut hosts = Vec::new();
    for index in 0..host_count {
        if prefix <= 30 && (index == 0 || index == host_count - 1) {
            continue;
        }
        hosts.push(Ipv4Addr::from(network + index as u32));
    }
    Ok(hosts)
}

fn default_lan_scan_cidr() -> Option<String> {
    let socket = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(8, 8, 8, 8), 80)).ok()?;
    let local = socket.local_addr().ok()?.ip();
    let std::net::IpAddr::V4(address) = local else {
        return None;
    };
    let octets = address.octets();
    if octets[0] == 127 || octets[0] == 0 || octets[0] == 169 && octets[1] == 254 {
        return None;
    }
    Some(format!("{}.{}.{}.0/24", octets[0], octets[1], octets[2]))
}

async fn bind_device(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<BindRequest>,
) -> Result<Json<DeviceRecord>, HttpError> {
    let mut guard = state.inner.lock().expect("state lock");
    let device = guard
        .devices
        .get_mut(&id)
        .ok_or_else(|| HttpError::not_found("device_not_found", "device is not known"))?;
    let binding = DeviceBinding {
        alias: input.alias,
        stable_id: id.clone(),
        port_path: device.port_path.clone(),
        created_at: now(),
        logical_device_id: input.logical_device_id,
    };
    device.binding = Some(binding.clone());
    let device = device.clone();
    guard.bindings.insert(id.clone(), binding);
    let snapshot = persisted_snapshot(&guard);
    drop(guard);
    persist_devd_state(&state.persistence, snapshot)?;
    emit(
        &state,
        Some(id),
        "bind",
        "device binding updated",
        json!({}),
    );
    Ok(Json(device))
}

async fn connect_device(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<DeviceRecord>, HttpError> {
    Ok(Json(connect_device_inner(&state, id).await?))
}

async fn connect_device_inner(state: &AppState, id: String) -> Result<DeviceRecord, HttpError> {
    let (transport, native_port_path) = {
        let guard = state.inner.lock().expect("state lock");
        let device = guard
            .devices
            .get(&id)
            .ok_or_else(|| HttpError::not_found("device_not_found", "device is not known"))?;
        (device.transport.clone(), device.port_path.clone())
    };
    let native_identity = if matches!(transport, DeviceTransport::NativeSerial) {
        let port_path = native_port_path.ok_or_else(|| {
            HttpError::retryable(
                "device_port_missing",
                "native serial device has no port path",
            )
        })?;
        let monitor_command_tx = {
            let guard = state.inner.lock().expect("state lock");
            guard
                .monitors
                .get(&id)
                .map(|monitor| monitor.command_tx.clone())
        };
        Some(read_device_identity_async(port_path, monitor_command_tx).await?)
    } else {
        None
    };
    let mut guard = state.inner.lock().expect("state lock");
    let selected_artifact = guard
        .devices
        .get(&id)
        .and_then(|device| device.selected_artifact_id.clone())
        .and_then(|artifact_id| guard.artifacts.get(&artifact_id).cloned());
    let identity_device_id = {
        let device = guard
            .devices
            .get_mut(&id)
            .ok_or_else(|| HttpError::not_found("device_not_found", "device is not known"))?;
        device.connection = ConnectionState::Connected;
        if matches!(device.transport, DeviceTransport::Mock) && device.identity.is_none() {
            device.identity = Some(mock_identity(&id));
        }
        if let Some(identity) = native_identity {
            device.identity = Some(identity);
        }
        apply_artifact_match(device, selected_artifact.as_ref());
        device
            .identity
            .as_ref()
            .and_then(|identity| identity.get("device_id"))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    if let Some(identity_device_id) = identity_device_id {
        attach_persisted_trace_for_identity(&mut guard, &id, &identity_device_id);
        merge_lan_record_into_primary(&mut guard, &id, &identity_device_id);
    }
    let device = guard
        .devices
        .get_mut(&id)
        .ok_or_else(|| HttpError::not_found("device_not_found", "device is not known"))?;
    push_log(
        device,
        "info",
        "devd",
        "device connected through mains-aegis-devd",
    );
    let device = device.clone();
    drop(guard);
    emit(&state, Some(id), "connect", "device connected", json!({}));
    Ok(device)
}

async fn disconnect_device(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<DeviceRecord>, HttpError> {
    Ok(Json(disconnect_device_inner(&state, &id).await?))
}

async fn disconnect_device_inner(state: &AppState, id: &str) -> Result<DeviceRecord, HttpError> {
    let stop = {
        let mut guard = state.inner.lock().expect("state lock");
        guard.monitors.remove(id)
    };
    if let Some(stop) = stop {
        stop.stop.store(true, Ordering::SeqCst);
    }
    let mut guard = state.inner.lock().expect("state lock");
    let device = guard
        .devices
        .get_mut(id)
        .ok_or_else(|| HttpError::not_found("device_not_found", "device is not known"))?;
    device.connection = ConnectionState::Disconnected;
    let device = device.clone();
    drop(guard);
    emit(
        &state,
        Some(id.to_string()),
        "disconnect",
        "device disconnected",
        json!({}),
    );
    Ok(device)
}

async fn create_web_lease(
    State(state): State<AppState>,
    Json(input): Json<WebLeaseCreateRequest>,
) -> Result<Json<Value>, HttpError> {
    cleanup_expired_web_leases(&state).await;
    {
        let guard = state.inner.lock().expect("state lock");
        let device = guard
            .devices
            .get(&input.device_id)
            .ok_or_else(|| HttpError::not_found("device_not_found", "device is not known"))?;
        if matches!(device.transport, DeviceTransport::Mock) {
            return Err(HttpError::non_retryable(
                "device_not_usb",
                "web USB lease requires a native serial device",
            ));
        }
        if let Some(existing) = active_lease_for_device(&guard, &input.device_id) {
            return Err(HttpError::non_retryable(
                "device_lease_conflict",
                format!(
                    "device already has an active Web lease: {}",
                    existing.lease_id
                ),
            ));
        }
    }

    let device = connect_device_inner(&state, input.device_id.clone()).await?;
    let identity_device_id = device
        .identity
        .as_ref()
        .and_then(|identity| identity.get("device_id"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let lease_id = next_id();
    let expires_at = std::time::Instant::now() + Duration::from_millis(WEB_LEASE_TTL_MS);
    {
        let mut guard = state.inner.lock().expect("state lock");
        guard.web_leases.insert(
            lease_id.clone(),
            WebUsbLease {
                lease_id: lease_id.clone(),
                device_id: input.device_id.clone(),
                identity_device_id: identity_device_id.clone(),
                expires_at,
            },
        );
    }
    emit(
        &state,
        Some(input.device_id.clone()),
        "web_lease",
        "web USB lease created",
        json!({"lease_id": lease_id, "identity_device_id": identity_device_id}),
    );
    Ok(Json(json!({
        "lease_id": lease_id,
        "device_id": input.device_id,
        "identity_device_id": identity_device_id,
        "expires_at": lease_expires_at_string(WEB_LEASE_TTL_MS),
        "heartbeat_interval_ms": WEB_LEASE_HEARTBEAT_INTERVAL_MS,
        "lease_ttl_ms": WEB_LEASE_TTL_MS,
        "device": device
    })))
}

async fn heartbeat_web_lease(
    State(state): State<AppState>,
    Path(lease_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    cleanup_expired_web_leases(&state).await;
    let mut guard = state.inner.lock().expect("state lock");
    let lease = guard.web_leases.get_mut(&lease_id).ok_or_else(|| {
        HttpError::non_retryable("web_session_expired", "Web USB lease is expired")
    })?;
    lease.expires_at = std::time::Instant::now() + Duration::from_millis(WEB_LEASE_TTL_MS);
    let device_id = lease.device_id.clone();
    let identity_device_id = lease.identity_device_id.clone();
    Ok(Json(json!({
        "lease_id": lease_id,
        "device_id": device_id,
        "identity_device_id": identity_device_id,
        "expires_at": lease_expires_at_string(WEB_LEASE_TTL_MS),
        "heartbeat_interval_ms": WEB_LEASE_HEARTBEAT_INTERVAL_MS,
        "lease_ttl_ms": WEB_LEASE_TTL_MS
    })))
}

async fn release_web_lease(
    State(state): State<AppState>,
    Path(lease_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let released = release_web_lease_inner(&state, &lease_id, "released").await?;
    Ok(Json(json!({"ok": true, "released": released})))
}

async fn unbind_device(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<DeviceRecord>, HttpError> {
    let mut guard = state.inner.lock().expect("state lock");
    guard.bindings.remove(&id);
    let device = guard
        .devices
        .get_mut(&id)
        .ok_or_else(|| HttpError::not_found("device_not_found", "device is not known"))?;
    device.binding = None;
    let device = device.clone();
    let snapshot = persisted_snapshot(&guard);
    drop(guard);
    persist_devd_state(&state.persistence, snapshot)?;
    emit(
        &state,
        Some(id),
        "unbind",
        "device binding removed",
        json!({}),
    );
    Ok(Json(device))
}

async fn device_identity(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let guard = state.inner.lock().expect("state lock");
    let device = guard
        .devices
        .get(&id)
        .ok_or_else(|| HttpError::not_found("device_not_found", "device is not known"))?;
    match device.identity.clone() {
        Some(identity) => Ok(Json(identity)),
        None if matches!(device.transport, DeviceTransport::Mock) => Ok(Json(mock_identity(&id))),
        None => Err(HttpError::retryable(
            "identity_unavailable",
            "device identity is unavailable until the device is connected",
        )),
    }
}

async fn device_power_diag(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let transport = {
        let guard = state.inner.lock().expect("state lock");
        let device = guard
            .devices
            .get(&id)
            .ok_or_else(|| HttpError::not_found("device_not_found", "device is not known"))?;
        device.transport.clone()
    };
    let diag = if matches!(transport, DeviceTransport::Mock) {
        mock_power_diag()
    } else {
        send_device_cdc_request(&state, &id, "get_power_diag", "devd-power-diag").await?
    };
    update_device_power_diag_snapshot(&state, &id, diag.clone());
    Ok(Json(diag))
}

async fn select_artifact(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ArtifactSelectRequest>,
) -> Result<Json<Value>, HttpError> {
    let mut loaded_artifact_id = None;
    let mut artifact = None;
    if let Some(inline_artifact) = input.artifact {
        loaded_artifact_id = Some(inline_artifact.artifact_id.clone());
        artifact = Some(inline_artifact);
    }
    if let Some(path) = input.manifest_path.as_deref() {
        artifact = Some(read_manifest(path)?);
    }
    let mut guard = state.inner.lock().expect("state lock");
    if let Some(artifact) = artifact {
        loaded_artifact_id = Some(artifact.artifact_id.clone());
        guard
            .artifacts
            .insert(artifact.artifact_id.clone(), artifact);
    }
    let selected_id = input.artifact_id.or(loaded_artifact_id).ok_or_else(|| {
        HttpError::non_retryable(
            "artifact_missing",
            "artifact_id or manifest_path is required",
        )
    })?;
    let selected = guard
        .artifacts
        .get(&selected_id)
        .cloned()
        .ok_or_else(|| HttpError::not_found("artifact_not_found", "artifact was not loaded"))?;
    let device = guard
        .devices
        .get_mut(&id)
        .ok_or_else(|| HttpError::not_found("device_not_found", "device is not known"))?;
    device.selected_artifact_id = Some(selected_id.clone());
    apply_artifact_match(device, Some(&selected));
    let decode = device.log_decode.clone();
    guard
        .selected_artifacts
        .insert(id.clone(), selected_id.clone());
    let snapshot = persisted_snapshot(&guard);
    drop(guard);
    persist_devd_state(&state.persistence, snapshot)?;
    emit(
        &state,
        Some(id),
        "artifact",
        "firmware artifact selected",
        json!({"artifact_id": selected_id, "log_decode": decode}),
    );
    Ok(Json(json!({"artifact": selected, "log_decode": decode})))
}

async fn device_artifact(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let guard = state.inner.lock().expect("state lock");
    let device = guard
        .devices
        .get(&id)
        .ok_or_else(|| HttpError::not_found("device_not_found", "device is not known"))?;
    let artifact = device
        .selected_artifact_id
        .as_ref()
        .and_then(|id| guard.artifacts.get(id));
    Ok(Json(
        json!({"artifact": artifact, "log_decode": device.log_decode}),
    ))
}

async fn flash_device(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<FlashRequest>,
) -> Result<Json<Value>, HttpError> {
    let (artifact, dry_run, port_path) = {
        let guard = state.inner.lock().expect("state lock");
        let device = guard
            .devices
            .get(&id)
            .ok_or_else(|| HttpError::not_found("device_not_found", "device is not known"))?;
        let artifact_id = input
            .artifact_id
            .as_ref()
            .or(device.selected_artifact_id.as_ref())
            .ok_or_else(|| {
                HttpError::non_retryable("artifact_missing", "select an artifact before flashing")
            })?;
        let artifact =
            guard.artifacts.get(artifact_id).cloned().ok_or_else(|| {
                HttpError::not_found("artifact_not_found", "artifact was not loaded")
            })?;
        (
            artifact,
            input.dry_run.unwrap_or(false),
            bound_flash_port(device),
        )
    };
    verify_artifact_files(&artifact)?;
    emit(
        &state,
        Some(id.clone()),
        "flash",
        "flash requested",
        json!({"artifact_id": artifact.artifact_id, "dry_run": dry_run}),
    );
    if dry_run {
        return Ok(Json(
            json!({"ok": true, "dry_run": true, "artifact_id": artifact.artifact_id}),
        ));
    }
    let port_path = port_path.ok_or_else(|| {
        HttpError::non_retryable(
            "device_port_unbound",
            "real flash requires a device with a known bound serial port",
        )
    })?;
    let elf = artifact
        .files
        .iter()
        .find(|file| file.kind == "elf")
        .ok_or_else(|| {
            HttpError::non_retryable(
                "artifact_missing_elf",
                "selected artifact does not include an ELF file",
            )
        })?;
    let espflash_timeout = env::var("MAINS_AEGIS_DEVD_ESPFLASH_TIMEOUT_SEC")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_ESPFLASH_TIMEOUT_SECS);
    let mut command = Command::new(
        env::var("MAINS_AEGIS_DEVD_ESPFLASH_BIN").unwrap_or_else(|_| "espflash".to_string()),
    );
    command
        .arg("flash")
        .arg("--port")
        .arg(&port_path)
        .arg("--after")
        .arg("watchdog-reset")
        .arg(&elf.path)
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = tokio::time::timeout(Duration::from_secs(espflash_timeout), command.output())
        .await
        .map_err(|_| {
            HttpError::retryable(
                "espflash_timeout",
                format!("espflash did not finish within {espflash_timeout}s"),
            )
        })?
        .map_err(|error| HttpError::retryable("espflash_launch_failed", error.to_string()))?;
    let backend = json!({
        "status": output.status.to_string(),
        "stdout": String::from_utf8_lossy(&output.stdout),
        "stderr": String::from_utf8_lossy(&output.stderr),
    });
    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(HttpError::retryable(
            "espflash_failed",
            format!(
                "espflash exited with {}; stdout={}; stderr={}",
                output.status, stdout, stderr
            ),
        ));
    }
    emit(
        &state,
        Some(id.clone()),
        "flash",
        "flash completed",
        json!({"artifact_id": artifact.artifact_id, "backend": backend}),
    );
    Ok(Json(
        json!({"ok": true, "artifact_id": artifact.artifact_id, "backend": backend}),
    ))
}

async fn reset_device(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let (transport, port_path, monitor_command_tx) = {
        let guard = state.inner.lock().expect("state lock");
        let device = guard
            .devices
            .get(&id)
            .ok_or_else(|| HttpError::not_found("device_not_found", "device is not known"))?;
        (
            device.transport.clone(),
            device.port_path.clone(),
            guard
                .monitors
                .get(&id)
                .map(|monitor| monitor.command_tx.clone()),
        )
    };
    if matches!(transport, DeviceTransport::NativeSerial) {
        if let Some(command_tx) = monitor_command_tx {
            send_monitor_reset_async(command_tx).await?;
        } else {
            let port_path = port_path.ok_or_else(|| {
                HttpError::retryable(
                    "device_port_missing",
                    "native serial device has no port path",
                )
            })?;
            reset_native_serial_async(port_path).await?;
        }
    }
    {
        let mut guard = state.inner.lock().expect("state lock");
        if let Some(device) = guard.devices.get_mut(&id) {
            device.connection = ConnectionState::Disconnected;
            push_log(device, "info", "reset", "device reset requested");
        }
    }
    emit(
        &state,
        Some(id),
        "reset",
        "reset requested",
        json!({"backend": reset_backend_name(&transport)}),
    );
    Ok(Json(
        json!({"ok": true, "backend": reset_backend_name(&transport)}),
    ))
}

async fn set_wifi_config(
    State(state): State<AppState>,
    Json(input): Json<WifiConfigRequest>,
) -> Result<Json<Value>, HttpError> {
    let target = resolve_settings_control_target(
        &state,
        input.device_id.as_deref(),
        input.lease_id.as_deref(),
    )?;
    ensure_wifi_runtime_supported(&state, &target)?;
    let ssid = input.ssid.clone();
    let response = send_settings_command(
        &state,
        &target,
        LanSettingsRequest {
            method: "POST",
            path: "/api/v1/wifi-config",
            body: Some(json!({"ssid": input.ssid, "psk": input.psk})),
        },
        json!({"type": "wifi_config", "op": "set", "ssid": input.ssid, "psk": input.psk}),
        |settings| {
            settings.wifi_configured = Some(true);
            settings.wifi_ssid = Some(ssid);
        },
        "wifi_config",
        "WiFi credentials saved through mains-aegis-devd",
    )
    .await?;
    let network = wait_for_wifi_connected(&state, &target).await?;
    Ok(Json(json!({
        "wifi_config": response,
        "network": network,
        "applied": true
    })))
}

async fn clear_wifi_config(
    State(state): State<AppState>,
    Query(query): Query<SettingsTargetQuery>,
) -> Result<Json<Value>, HttpError> {
    let target = resolve_settings_control_target(
        &state,
        query.device_id.as_deref(),
        query.lease_id.as_deref(),
    )?;
    let response = send_settings_command(
        &state,
        &target,
        LanSettingsRequest {
            method: "DELETE",
            path: "/api/v1/wifi-config",
            body: None,
        },
        json!({"type": "wifi_config", "op": "clear"}),
        |settings| {
            settings.wifi_configured = Some(false);
            settings.wifi_ssid = None;
        },
        "wifi_config",
        "WiFi credentials cleared through mains-aegis-devd",
    )
    .await?;
    let network = wait_for_wifi_state(&state, &target, "disabled").await?;
    Ok(Json(json!({
        "wifi_config": response,
        "network": network,
        "applied": true
    })))
}

async fn set_log_level(
    State(state): State<AppState>,
    Json(input): Json<LogLevelRequest>,
) -> Result<Json<Value>, HttpError> {
    let target = resolve_settings_control_target(
        &state,
        input.device_id.as_deref(),
        input.lease_id.as_deref(),
    )?;
    let level = input.level.clone();
    let response = send_settings_command(
        &state,
        &target,
        LanSettingsRequest {
            method: "POST",
            path: "/api/v1/settings/log-level",
            body: Some(json!({"level": input.level})),
        },
        json!({"type": "request", "op": "set_log_level", "level": input.level}),
        |settings| settings.log_level = level,
        "usb_cdc",
        "Log level updated through mains-aegis-devd",
    )
    .await?;
    Ok(Json(response))
}

async fn set_manual_charge(
    State(state): State<AppState>,
    Json(input): Json<ManualChargeRequest>,
) -> Result<Json<Value>, HttpError> {
    let target = resolve_settings_control_target(
        &state,
        input.device_id.as_deref(),
        input.lease_id.as_deref(),
    )?;
    let prefs = ManualChargePrefs {
        target: input.target.clone(),
        speed: input.speed.clone(),
        timer_h: input.timer_h,
    };
    let response = send_settings_command(
        &state,
        &target,
        LanSettingsRequest {
            method: "POST",
            path: "/api/v1/settings/manual-charge",
            body: Some(json!({
                "target": input.target,
                "speed": input.speed,
                "timer_h": input.timer_h
            })),
        },
        json!({
            "type": "request",
            "op": "set_manual_charge_prefs",
            "target": input.target,
            "speed": input.speed,
            "timer_h": input.timer_h
        }),
        |settings| settings.manual_charge = prefs,
        "manual_charge",
        "Manual charge preferences updated through mains-aegis-devd",
    )
    .await?;
    Ok(Json(response))
}

async fn monitor_start(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let (transport, port_path) = {
        let guard = state.inner.lock().expect("state lock");
        let device = guard
            .devices
            .get(&id)
            .ok_or_else(|| HttpError::not_found("device_not_found", "device is not known"))?;
        (device.transport.clone(), device.port_path.clone())
    };
    let sample = if matches!(transport, DeviceTransport::NativeSerial) {
        let port_path = port_path.ok_or_else(|| {
            HttpError::retryable(
                "device_port_missing",
                "native serial device has no port path",
            )
        })?;
        start_native_monitor(&state, id.clone(), port_path)?
    } else {
        MonitorStartResult {
            trace_count: 0,
            log_count: 0,
            already_running: false,
        }
    };
    let mut guard = state.inner.lock().expect("state lock");
    let device = guard
        .devices
        .get_mut(&id)
        .ok_or_else(|| HttpError::not_found("device_not_found", "device is not known"))?;
    push_log(device, "info", "monitor", "monitor started");
    if matches!(transport, DeviceTransport::NativeSerial) {
        device.connection = ConnectionState::Connected;
    }
    let decode = device.log_decode.clone();
    let trace_count = device.trace.len();
    let log_count = device.logs.len();
    drop(guard);
    emit(
        &state,
        Some(id),
        "monitor",
        "monitor started",
        json!({"log_decode": decode, "trace_count": trace_count, "log_count": log_count, "already_running": sample.already_running}),
    );
    Ok(Json(
        json!({"ok": true, "log_decode": decode, "trace_count": trace_count, "log_count": log_count, "initial_trace_count": sample.trace_count, "initial_log_count": sample.log_count, "already_running": sample.already_running}),
    ))
}

async fn monitor_stop(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let stop = {
        let mut guard = state.inner.lock().expect("state lock");
        guard.monitors.remove(&id)
    };
    if let Some(stop) = stop {
        stop.stop.store(true, Ordering::SeqCst);
    } else {
        ensure_device(&state, &id)?;
    }
    emit(&state, Some(id), "monitor", "monitor stopped", json!({}));
    Ok(Json(json!({"ok": true})))
}

async fn device_settings(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let guard = state.inner.lock().expect("state lock");
    let device = guard
        .devices
        .get(&id)
        .ok_or_else(|| HttpError::not_found("device_not_found", "device is not known"))?;
    Ok(Json(json!({
        "wifi": {
            "configured": device.settings.wifi_configured.unwrap_or(false),
            "ssid": device.settings.wifi_ssid,
        },
        "log_level": device.settings.log_level,
        "manual_charge": device.settings.manual_charge,
    })))
}

async fn devd_compat_settings(
    Query(query): Query<WebLeaseQuery>,
    State(state): State<AppState>,
) -> Result<Json<Value>, HttpError> {
    let guard = state.inner.lock().expect("state lock");
    let device = select_compat_device(&guard, query.lease_id.as_deref())?;
    Ok(Json(settings_snapshot(&device.settings)))
}

fn settings_snapshot(settings: &DeviceSettingsState) -> Value {
    json!({
        "wifi": {
            "configured": settings.wifi_configured.unwrap_or(false),
            "ssid": settings.wifi_ssid.clone(),
        },
        "log_level": settings.log_level.clone(),
        "manual_charge": settings.manual_charge.clone(),
    })
}

async fn device_connection(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let guard = state.inner.lock().expect("state lock");
    let device = guard
        .devices
        .get(&id)
        .ok_or_else(|| HttpError::not_found("device_not_found", "device is not known"))?;
    Ok(Json(json!({
        "device_id": device.id,
        "transport": transport_name(&device.transport),
        "connection": match device.connection {
            ConnectionState::Disconnected => "disconnected",
            ConnectionState::Connected => "connected",
            ConnectionState::Busy => "busy",
            ConnectionState::Error => "error",
        },
        "port_path": device.port_path,
        "lan_address": device.lan_address,
        "lan_conflict_addresses": device.lan_conflict_addresses,
        "binding": device.binding,
        "identity_device_id": device.identity.as_ref().and_then(|identity| identity.get("device_id")).and_then(Value::as_str),
        "selected_artifact_id": device.selected_artifact_id,
        "available_transports": available_transports(device),
        "transports": connection_transports(device),
        "switch_hint": connection_switch_hint(device),
        "log_decode": device.log_decode,
    })))
}

async fn device_trace(
    Query(query): Query<SessionQuery>,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let guard = state.inner.lock().expect("state lock");
    let device = guard
        .devices
        .get(&id)
        .ok_or_else(|| HttpError::not_found("device_not_found", "device is not known"))?;
    let trace_limit = query.trace_limit.unwrap_or(600).min(2_000);
    Ok(Json(json!({
        "connected": matches!(device.connection, ConnectionState::Connected),
        "protocol": "mains-aegis.cdc.v1",
        "identity": device.identity,
        "status": device.status,
        "power_diag": device.power_diag,
        "log_count": device.logs.len(),
        "trace_count": device.trace.len(),
        "logs": tail(&device.logs, query.logs_limit.unwrap_or(200).min(500)),
        "trace": tail(&device.trace, trace_limit),
        "transports": grouped_trace_by_transport(&device.trace, trace_limit),
        "log_decode": device.log_decode
    })))
}

async fn devd_compat_session(
    Query(query): Query<SessionQuery>,
    State(state): State<AppState>,
) -> Result<Json<Value>, HttpError> {
    let guard = state.inner.lock().expect("state lock");
    let device = select_compat_device(&guard, query.lease_id.as_deref())?;
    let trace_limit = query.trace_limit.unwrap_or(600).min(2_000);
    Ok(Json(json!({
        "connected": matches!(device.connection, ConnectionState::Connected),
        "protocol": "mains-aegis.cdc.v1",
        "identity": device.identity,
        "status": device.status,
        "power_diag": device.power_diag,
        "log_count": device.logs.len(),
        "trace_count": device.trace.len(),
        "logs": tail(&device.logs, query.logs_limit.unwrap_or(200).min(500)),
        "trace": tail(&device.trace, trace_limit),
        "transports": grouped_trace_by_transport(&device.trace, trace_limit),
        "settings": {
            "wifi": {
                "configured": device.settings.wifi_configured.unwrap_or(false),
                "ssid": device.settings.wifi_ssid,
            },
            "log_level": device.settings.log_level,
            "manual_charge": device.settings.manual_charge,
        },
        "log_decode": device.log_decode
    })))
}

async fn devd_compat_identity(
    Query(query): Query<WebLeaseQuery>,
    State(state): State<AppState>,
) -> Result<Json<Value>, HttpError> {
    let guard = state.inner.lock().expect("state lock");
    let device = select_compat_device(&guard, query.lease_id.as_deref())?;
    let identity = device.identity.clone().ok_or_else(|| {
        HttpError::non_retryable(
            "identity_unavailable",
            "device identity is unavailable until a devd device is connected",
        )
    })?;
    Ok(Json(identity))
}

async fn devd_compat_network(
    Query(query): Query<WebLeaseQuery>,
    State(state): State<AppState>,
) -> Result<Json<Value>, HttpError> {
    let Json(identity) = devd_compat_identity(Query(query), State(state)).await?;
    let network = identity.get("network").cloned().ok_or_else(|| {
        HttpError::non_retryable(
            "network_unavailable",
            "device identity does not include network",
        )
    })?;
    Ok(Json(network))
}

async fn devd_compat_status(
    Query(query): Query<WebLeaseQuery>,
    State(state): State<AppState>,
) -> Result<Json<Value>, HttpError> {
    let guard = state.inner.lock().expect("state lock");
    let device = select_compat_device(&guard, query.lease_id.as_deref())?;
    if let Some(status) = device.status.clone() {
        return Ok(Json(status));
    }
    let network = device
        .identity
        .as_ref()
        .and_then(|identity| identity.get("network").cloned())
        .unwrap_or_else(|| json!({"state": "disabled", "ipv4": null, "last_error": null}));
    Ok(Json(json!({
        "mode": "standby",
        "input": {
            "mains_present": false,
            "input_vbus_mv": null,
            "input_ibus_ma": null,
            "vin_vbus_mv": null,
            "vin_iin_ma": null
        },
        "output": {
            "requested": "none",
            "active": "none",
            "recoverable": "none",
            "gate_reason": "none",
            "out_a": {"state": "unknown", "enabled": false, "vbus_mv": null, "iout_ma": null},
            "out_b": {"state": "unknown", "enabled": false, "vbus_mv": null, "iout_ma": null}
        },
        "charger": {
            "state": "unknown",
            "allow_charge": false,
            "ichg_ma": null,
            "ibat_ma": null,
            "vbat_present": false
        },
        "battery": {
            "state": "unknown",
            "pack_mv": null,
            "current_ma": null,
            "soc_pct": null,
            "no_battery": false,
            "discharge_ready": false,
            "issue_detail": null,
            "recovery_pending": false,
            "last_result": null
        },
        "thermal": {
            "tmp_a_state": "unknown",
            "tmp_a_c": null,
            "tmp_b_state": "unknown",
            "tmp_b_c": null
        },
        "network": {
            "state": network.get("state").cloned().unwrap_or_else(|| json!("disabled")),
            "ipv4": network.get("ipv4").cloned().unwrap_or(Value::Null),
            "last_error": network.get("last_error").cloned().unwrap_or(Value::Null)
        }
    })))
}

fn select_compat_device<'a>(
    state: &'a DevdState,
    lease_id: Option<&str>,
) -> Result<&'a DeviceRecord, HttpError> {
    let lease = if let Some(lease_id) = lease_id {
        active_lease_by_id(state, lease_id).ok_or_else(|| {
            HttpError::non_retryable("web_session_expired", "Web USB lease is expired")
        })?
    } else {
        let active = state
            .web_leases
            .iter()
            .filter(|(_, lease)| lease.expires_at > std::time::Instant::now())
            .map(|(lease_id, _)| lease_id.as_str())
            .collect::<Vec<_>>();
        match active.as_slice() {
            [lease_id] => state
                .web_leases
                .get(*lease_id)
                .expect("active lease exists"),
            [] => {
                return Err(HttpError::non_retryable(
                    "web_session_required",
                    "Web USB lease is required for devd USB control",
                ))
            }
            _ => {
                return Err(HttpError::non_retryable(
                    "device_selection_required",
                    "multiple Web USB leases are active; specify lease_id",
                ))
            }
        }
    };
    state
        .devices
        .get(&lease.device_id)
        .ok_or_else(|| HttpError::not_found("device_not_found", "leased device is not known"))
}

async fn send_device_cdc_request(
    state: &AppState,
    device_id: &str,
    op: &str,
    request_prefix: &str,
) -> Result<Value, HttpError> {
    let (port_path, monitor_command_tx) = {
        let guard = state.inner.lock().expect("state lock");
        let device = guard
            .devices
            .get(device_id)
            .ok_or_else(|| HttpError::not_found("device_not_found", "device is not known"))?;
        if !matches!(device.transport, DeviceTransport::NativeSerial) {
            return Err(HttpError::non_retryable(
                "device_transport_unsupported",
                "CDC requests require a native serial device",
            ));
        }
        (
            device.port_path.clone(),
            guard
                .monitors
                .get(device_id)
                .map(|monitor| monitor.command_tx.clone()),
        )
    };
    let request_id = format!("{request_prefix}-{}", Utc::now().timestamp_millis());
    let frame = json!({"type": "request", "request_id": request_id, "op": op});
    let response = if let Some(command_tx) = monitor_command_tx {
        send_monitor_cdc_frame_async(command_tx, frame.clone(), request_id.clone()).await?
    } else {
        let port_path = port_path.ok_or_else(|| {
            HttpError::retryable(
                "device_port_missing",
                "native serial device has no port path",
            )
        })?;
        send_native_cdc_frame_async(port_path, frame.clone(), request_id.clone()).await?
    };

    if response.get("type").and_then(Value::as_str) != Some("response")
        || !response.get("ok").and_then(Value::as_bool).unwrap_or(false)
    {
        return Err(error_from_cdc_response(&response));
    }
    response.get("result").cloned().ok_or_else(|| {
        HttpError::retryable(
            "cdc_response_missing_result",
            format!("CDC {op} response did not include result"),
        )
    })
}

#[derive(Debug, Clone)]
enum SettingsControlTarget {
    Usb {
        device_id: String,
        port_path: Option<String>,
        monitor_command_tx: Option<mpsc::Sender<NativeMonitorCommand>>,
    },
    Lan {
        device_id: String,
        address: String,
    },
}

#[derive(Debug, Clone)]
struct LanSettingsRequest {
    method: &'static str,
    path: &'static str,
    body: Option<Value>,
}

fn resolve_settings_control_target(
    state: &AppState,
    target_device_id: Option<&str>,
    lease_id: Option<&str>,
) -> Result<SettingsControlTarget, HttpError> {
    let guard = state.inner.lock().expect("state lock");
    if let Some(lease_id) = lease_id {
        ensure_web_lease_for_target(&guard, target_device_id, Some(lease_id))?;
        let lease = active_lease_by_id(&guard, lease_id).ok_or_else(|| {
            HttpError::non_retryable("web_session_expired", "Web USB lease is expired")
        })?;
        let device = guard.devices.get(&lease.device_id).ok_or_else(|| {
            HttpError::not_found("device_not_found", "leased device is not known")
        })?;
        if !matches!(device.transport, DeviceTransport::NativeSerial) {
            return Err(HttpError::non_retryable(
                "devd_usb_session_required",
                "Web lease settings control requires a USB CDC device",
            ));
        }
        return Ok(SettingsControlTarget::Usb {
            device_id: device.id.clone(),
            port_path: device.port_path.clone(),
            monitor_command_tx: guard
                .monitors
                .get(&device.id)
                .map(|monitor| monitor.command_tx.clone()),
        });
    }

    let mut candidates = guard.devices.values().collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        transport_preference(&left.transport)
            .cmp(&transport_preference(&right.transport))
            .then_with(|| left.id.cmp(&right.id))
    });
    let candidates = candidates
        .into_iter()
        .filter(|device| {
            target_device_id
                .map(|target| device_matches_identity_id(device, target))
                .unwrap_or(true)
        })
        .filter_map(|device| match device.transport {
            DeviceTransport::NativeSerial
                if matches!(device.connection, ConnectionState::Connected)
                    && device.identity.is_some() =>
            {
                Some(Ok(SettingsControlTarget::Usb {
                    device_id: device.id.clone(),
                    port_path: device.port_path.clone(),
                    monitor_command_tx: guard
                        .monitors
                        .get(&device.id)
                        .map(|monitor| monitor.command_tx.clone()),
                }))
            }
            DeviceTransport::Lan
                if device.lan_address.is_some()
                    && device.identity.is_some()
                    && device.lan_conflict_addresses.is_empty() =>
            {
                Some(Ok(SettingsControlTarget::Lan {
                    device_id: device.id.clone(),
                    address: device.lan_address.clone().expect("lan address checked"),
                }))
            }
            DeviceTransport::Lan if !device.lan_conflict_addresses.is_empty() => Some(Err(
                HttpError::non_retryable(
                    "lan_identity_conflict",
                    "multiple LAN addresses reported the same device_id; select USB or resolve the LAN conflict before writing settings",
                ),
            )),
            _ => None,
        })
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(target_device_id) = target_device_id {
        return candidates.into_iter().next().ok_or_else(|| {
            HttpError::retryable(
                "settings_transport_unavailable",
                format!("no connected USB or reachable LAN transport is available for {target_device_id}"),
            )
        });
    }
    match candidates.as_slice() {
        [target] => Ok(target.clone()),
        [] => Err(HttpError::retryable(
            "settings_transport_unavailable",
            "connect USB CDC or discover a LAN device before changing settings",
        )),
        _ => Err(HttpError::non_retryable(
            "settings_device_ambiguous",
            "multiple devices can accept settings writes; provide device_id",
        )),
    }
}

async fn send_settings_command<F>(
    state: &AppState,
    target: &SettingsControlTarget,
    lan_request: LanSettingsRequest,
    frame: Value,
    apply_settings: F,
    log_target: &str,
    log_message: &str,
) -> Result<Value, HttpError>
where
    F: FnOnce(&mut DeviceSettingsState),
{
    match target {
        SettingsControlTarget::Usb { .. } => {
            send_settings_frame(
                state,
                target,
                frame,
                apply_settings,
                log_target,
                log_message,
            )
            .await
        }
        SettingsControlTarget::Lan { .. } => {
            send_lan_settings_request(
                state,
                target,
                lan_request,
                apply_settings,
                log_target,
                log_message,
            )
            .await
        }
    }
}

async fn send_settings_frame<F>(
    state: &AppState,
    target: &SettingsControlTarget,
    mut frame: Value,
    apply_settings: F,
    log_target: &str,
    log_message: &str,
) -> Result<Value, HttpError>
where
    F: FnOnce(&mut DeviceSettingsState),
{
    let SettingsControlTarget::Usb {
        device_id,
        port_path,
        monitor_command_tx,
    } = target
    else {
        return Err(HttpError::non_retryable(
            "settings_transport_invalid",
            "USB settings frame requires a USB target",
        ));
    };
    let request_id = format!("devd-safe-{}", Utc::now().timestamp_millis());
    if let Value::Object(object) = &mut frame {
        object.insert("request_id".to_string(), Value::String(request_id.clone()));
    }
    let response = if let Some(command_tx) = monitor_command_tx.clone() {
        send_monitor_cdc_frame_async(command_tx, frame.clone(), request_id.clone()).await?
    } else {
        let port_path = port_path.clone().ok_or_else(|| {
            HttpError::retryable(
                "device_port_missing",
                "native serial device has no port path",
            )
        })?;
        send_native_cdc_frame_async(port_path, frame.clone(), request_id.clone()).await?
    };

    if response.get("type").and_then(Value::as_str) != Some("response")
        || !response.get("ok").and_then(Value::as_bool).unwrap_or(false)
    {
        return Err(error_from_cdc_response(&response));
    }

    let result = response.get("result").cloned().unwrap_or(Value::Null);
    record_settings_success(
        state,
        device_id,
        frame,
        response,
        apply_settings,
        log_target,
        log_message,
    );
    Ok(result)
}

async fn send_lan_settings_request<F>(
    state: &AppState,
    target: &SettingsControlTarget,
    request: LanSettingsRequest,
    apply_settings: F,
    log_target: &str,
    log_message: &str,
) -> Result<Value, HttpError>
where
    F: FnOnce(&mut DeviceSettingsState),
{
    let SettingsControlTarget::Lan { device_id, address } = target else {
        return Err(HttpError::non_retryable(
            "settings_transport_invalid",
            "LAN settings request requires a LAN target",
        ));
    };
    let tx_payload = request
        .body
        .as_ref()
        .map(redact_lan_body)
        .map(|body| body.to_string())
        .unwrap_or_default();
    let tx_trace = structured_trace_entry(
        "tx",
        "http",
        Some(format!("http://{address}{}", request.path)),
        format!("{} {}", request.method, request.path).as_str(),
        tx_payload,
    );
    let response =
        lan_http_json(address, request.method, request.path, request.body.as_ref()).await?;
    let rx_trace = structured_trace_entry(
        "rx",
        "http",
        Some(format!("http://{address}{}", request.path)),
        "settings write response",
        response.to_string(),
    );
    let settings_snapshot = lan_http_json(address, "GET", "/api/v1/settings", None).await?;
    let settings_state = settings_state_from_api(&settings_snapshot)?;
    let log = SerialLogEntry {
        id: next_id(),
        timestamp: now(),
        level: "info".to_string(),
        target: log_target.to_string(),
        message: log_message.to_string(),
    };
    let snapshot = {
        let mut guard = state.inner.lock().expect("state lock");
        if let Some(device) = guard.devices.get_mut(device_id) {
            device.settings = settings_state;
            apply_settings(&mut device.settings);
            push_bounded(&mut device.trace, tx_trace.clone(), LOG_LIMIT);
            push_bounded(&mut device.trace, rx_trace.clone(), LOG_LIMIT);
            push_bounded(&mut device.logs, log.clone(), LOG_LIMIT);
            device.connection = ConnectionState::Connected;
        }
        persisted_snapshot(&guard)
    };
    persist_devd_state(&state.persistence, snapshot)?;
    emit(
        state,
        Some(device_id.clone()),
        "lan_trace",
        "LAN HTTP trace",
        json!({"trace": tx_trace}),
    );
    emit(
        state,
        Some(device_id.clone()),
        "lan_trace",
        "LAN HTTP trace",
        json!({"trace": rx_trace}),
    );
    emit(
        state,
        Some(device_id.clone()),
        "serial_log",
        "settings log frame",
        json!({"log": log}),
    );
    Ok(json!({"response": response, "settings": settings_snapshot}))
}

fn ensure_wifi_runtime_supported(
    state: &AppState,
    target: &SettingsControlTarget,
) -> Result<(), HttpError> {
    let guard = state.inner.lock().expect("state lock");
    let device_id = match target {
        SettingsControlTarget::Usb { device_id, .. }
        | SettingsControlTarget::Lan { device_id, .. } => device_id,
    };
    let device = guard
        .devices
        .get(device_id)
        .ok_or_else(|| HttpError::not_found("device_not_found", "device is not known"))?;
    let features = device
        .identity
        .as_ref()
        .and_then(|identity| identity.get("firmware"))
        .and_then(|firmware| firmware.get("features"))
        .and_then(Value::as_array);
    let has_net_http = features.is_some_and(|features| {
        features
            .iter()
            .filter_map(Value::as_str)
            .any(|feature| feature == "net_http")
    });
    if has_net_http {
        return Ok(());
    }
    Err(HttpError::non_retryable(
        "wifi_runtime_unavailable",
        "connected firmware was built without net_http; WiFi credentials can be stored but this firmware cannot start WiFi or connect until a net_http build is flashed",
    ))
}

async fn wait_for_wifi_connected(
    state: &AppState,
    target: &SettingsControlTarget,
) -> Result<Value, HttpError> {
    wait_for_wifi_state(state, target, "connected").await
}

async fn wait_for_wifi_state(
    state: &AppState,
    target: &SettingsControlTarget,
    expected_state: &str,
) -> Result<Value, HttpError> {
    let deadline = std::time::Instant::now() + Duration::from_secs(45);
    let mut last_network = Value::Null;
    while std::time::Instant::now() < deadline {
        let (device_id, status) = match target {
            SettingsControlTarget::Usb {
                device_id,
                port_path,
                monitor_command_tx,
            } => {
                let request_id = format!("devd-wifi-status-{}", Utc::now().timestamp_millis());
                let frame =
                    json!({"type": "request", "request_id": request_id, "op": "get_status"});
                let response = if let Some(command_tx) = monitor_command_tx.clone() {
                    send_monitor_cdc_frame_async(command_tx, frame, request_id).await?
                } else {
                    let port_path = port_path.clone().ok_or_else(|| {
                        HttpError::retryable(
                            "device_port_missing",
                            "native serial device has no port path",
                        )
                    })?;
                    send_native_cdc_frame_async(port_path, frame, request_id).await?
                };
                (
                    device_id.clone(),
                    response.get("result").cloned().unwrap_or(Value::Null),
                )
            }
            SettingsControlTarget::Lan { device_id, address } => (
                device_id.clone(),
                lan_http_json(address, "GET", "/api/v1/status", None).await?,
            ),
        };
        let network = status.get("network").cloned().unwrap_or(Value::Null);
        if !network.is_null() {
            update_device_status_snapshot(state, &device_id, status.clone());
            last_network = network.clone();
        }
        match network.get("state").and_then(Value::as_str) {
            Some(state) if state == expected_state => return Ok(network),
            Some("error") if expected_state != "disabled" => {
                return Err(HttpError::retryable(
                    "wifi_connect_failed",
                    format!(
                        "WiFi failed to connect: {}",
                        network
                            .get("last_error")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown")
                    ),
                ));
            }
            _ => tokio::time::sleep(Duration::from_millis(750)).await,
        }
    }
    Err(HttpError::retryable(
        if expected_state == "disabled" {
            "wifi_disconnect_timeout"
        } else {
            "wifi_connect_timeout"
        },
        format!(
            "timed out waiting for WiFi state {expected_state}; last network state: {last_network}"
        ),
    ))
}

fn update_device_status_snapshot(state: &AppState, device_id: &str, status: Value) {
    let mut guard = state.inner.lock().expect("state lock");
    if let Some(device) = guard.devices.get_mut(device_id) {
        device.status = Some(status.clone());
    }
    drop(guard);
    emit(
        state,
        Some(device_id.to_string()),
        "serial_status",
        "CDC status snapshot",
        json!({"status": status}),
    );
}

fn update_device_power_diag_snapshot(state: &AppState, device_id: &str, power_diag: Value) {
    let mut guard = state.inner.lock().expect("state lock");
    if let Some(device) = guard.devices.get_mut(device_id) {
        device.power_diag = Some(power_diag.clone());
    }
    drop(guard);
    emit(
        state,
        Some(device_id.to_string()),
        "power_diag",
        "CDC power diagnostic snapshot",
        json!({"power_diag": power_diag}),
    );
}

fn spawn_web_lease_reaper(state: AppState) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(WEB_LEASE_CLEANUP_INTERVAL_MS)).await;
            cleanup_expired_web_leases(&state).await;
        }
    });
}

async fn cleanup_expired_web_leases(state: &AppState) {
    let expired = {
        let guard = state.inner.lock().expect("state lock");
        let now = std::time::Instant::now();
        guard
            .web_leases
            .iter()
            .filter_map(|(lease_id, lease)| {
                if lease.expires_at <= now {
                    Some(lease_id.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    };
    for lease_id in expired {
        let _ = release_web_lease_inner(state, &lease_id, "expired").await;
    }
}

async fn release_web_lease_inner(
    state: &AppState,
    lease_id: &str,
    reason: &str,
) -> Result<bool, HttpError> {
    let lease = {
        let mut guard = state.inner.lock().expect("state lock");
        guard.web_leases.remove(lease_id)
    };
    let Some(lease) = lease else {
        return Ok(false);
    };
    let should_disconnect = {
        let guard = state.inner.lock().expect("state lock");
        active_lease_for_device(&guard, &lease.device_id).is_none()
    };
    if should_disconnect {
        let _ = disconnect_device_inner(state, &lease.device_id).await;
    }
    emit(
        state,
        Some(lease.device_id),
        "web_lease",
        format!("web USB lease {reason}").as_str(),
        json!({"lease_id": lease_id}),
    );
    Ok(true)
}

fn active_lease_for_device<'a>(state: &'a DevdState, device_id: &str) -> Option<&'a WebUsbLease> {
    let now = std::time::Instant::now();
    state
        .web_leases
        .values()
        .find(|lease| lease.device_id == device_id && lease.expires_at > now)
}

fn active_lease_by_id<'a>(state: &'a DevdState, lease_id: &str) -> Option<&'a WebUsbLease> {
    let now = std::time::Instant::now();
    state
        .web_leases
        .get(lease_id)
        .filter(|lease| lease.expires_at > now)
}

fn lease_expires_at_string(ttl_ms: u64) -> String {
    (Utc::now() + chrono::Duration::milliseconds(ttl_ms as i64)).to_rfc3339()
}

fn device_matches_target(device: &DeviceRecord, target_device_id: &str) -> bool {
    device.id == target_device_id || device_logical_device_id(device) == Some(target_device_id)
}

fn ensure_web_lease_for_target(
    state: &DevdState,
    target_device_id: Option<&str>,
    lease_id: Option<&str>,
) -> Result<(), HttpError> {
    let Some(lease_id) = lease_id else {
        return Err(HttpError::non_retryable(
            "web_session_required",
            "Web USB lease is required for devd USB control",
        ));
    };
    let lease = active_lease_by_id(state, lease_id).ok_or_else(|| {
        HttpError::non_retryable("web_session_expired", "Web USB lease is expired")
    })?;
    if let Some(target_device_id) = target_device_id {
        let device = state.devices.get(&lease.device_id).ok_or_else(|| {
            HttpError::not_found("device_not_found", "leased device is not known")
        })?;
        if !device_matches_target(device, target_device_id) {
            return Err(HttpError::non_retryable(
                "web_session_device_mismatch",
                "Web USB lease does not match the requested device",
            ));
        }
    }
    Ok(())
}

fn device_matches_identity_id(device: &DeviceRecord, target_device_id: &str) -> bool {
    device.id == target_device_id
        || device_logical_device_id(device).is_some_and(|device_id| device_id == target_device_id)
}

async fn send_native_cdc_frame_async(
    port_path: String,
    frame: Value,
    request_id: String,
) -> Result<Value, HttpError> {
    tokio::task::spawn_blocking(move || send_native_cdc_frame(&port_path, frame, &request_id))
        .await
        .map_err(|error| HttpError::retryable("native_cdc_join_failed", error.to_string()))?
}

async fn send_monitor_cdc_frame_async(
    command_tx: mpsc::Sender<NativeMonitorCommand>,
    frame: Value,
    request_id: String,
) -> Result<Value, HttpError> {
    tokio::task::spawn_blocking(move || {
        let (response_tx, response_rx) = mpsc::channel();
        command_tx
            .send(NativeMonitorCommand::SendFrame {
                frame,
                request_id,
                response_tx,
            })
            .map_err(|_| {
                HttpError::retryable(
                    "native_monitor_command_unavailable",
                    "native monitor stopped before accepting the CDC command",
                )
            })?;
        response_rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => HttpError::retryable(
                    "native_monitor_command_timeout",
                    "timed out waiting for the native monitor to process the CDC command",
                ),
                mpsc::RecvTimeoutError::Disconnected => HttpError::retryable(
                    "native_monitor_command_disconnected",
                    "native monitor stopped before returning the CDC command response",
                ),
            })?
    })
    .await
    .map_err(|error| HttpError::retryable("native_monitor_join_failed", error.to_string()))?
}

async fn send_monitor_reset_async(
    command_tx: mpsc::Sender<NativeMonitorCommand>,
) -> Result<(), HttpError> {
    tokio::task::spawn_blocking(move || {
        let (response_tx, response_rx) = mpsc::channel();
        command_tx
            .send(NativeMonitorCommand::Reset { response_tx })
            .map_err(|_| {
                HttpError::retryable(
                    "native_monitor_command_unavailable",
                    "native monitor stopped before accepting the reset command",
                )
            })?;
        response_rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => HttpError::retryable(
                    "native_monitor_command_timeout",
                    "timed out waiting for the native monitor to process the reset command",
                ),
                mpsc::RecvTimeoutError::Disconnected => HttpError::retryable(
                    "native_monitor_command_disconnected",
                    "native monitor stopped before returning the reset command response",
                ),
            })?
    })
    .await
    .map_err(|error| HttpError::retryable("native_monitor_join_failed", error.to_string()))?
}

fn open_native_serial_port(
    port_path: &str,
    timeout: Duration,
    dtr_ready: bool,
) -> Result<Box<dyn serialport::SerialPort>, HttpError> {
    let mut port = serialport::new(port_path, 115_200)
        .timeout(timeout)
        .open()
        .map_err(|error| {
            HttpError::retryable(
                "native_serial_open_failed",
                format!("failed to open {port_path}: {error}"),
            )
        })?;
    port.write_data_terminal_ready(dtr_ready).map_err(|error| {
        HttpError::retryable(
            "native_serial_dtr_set_failed",
            format!("failed to set DTR={dtr_ready} on {port_path}: {error}"),
        )
    })?;
    port.write_request_to_send(false).map_err(|error| {
        HttpError::retryable(
            "native_serial_rts_release_failed",
            format!("failed to release RTS on {port_path}: {error}"),
        )
    })?;
    Ok(port)
}

fn native_serial_app_reset_steps() -> &'static [NativeSerialLineStep] {
    &[
        NativeSerialLineStep::Rts(false),
        NativeSerialLineStep::Dtr(true),
        NativeSerialLineStep::SleepMs(100),
        NativeSerialLineStep::Rts(true),
        NativeSerialLineStep::Dtr(true),
        NativeSerialLineStep::Rts(true),
        NativeSerialLineStep::SleepMs(100),
        NativeSerialLineStep::Rts(false),
        NativeSerialLineStep::Dtr(true),
    ]
}

#[cfg(test)]
fn native_serial_monitor_preserves_control_lines_on_open() -> bool {
    true
}

fn open_native_monitor_serial_port(
    port_path: &str,
    timeout: Duration,
) -> Result<Box<dyn serialport::SerialPort>, HttpError> {
    serialport::new(port_path, 115_200)
        .timeout(timeout)
        .preserve_dtr_on_open()
        .open()
        .map_err(|error| {
            HttpError::retryable(
                "native_serial_open_failed",
                format!("failed to open {port_path}: {error}"),
            )
        })
}

fn reset_native_serial_to_app_on_port(
    port_path: &str,
    port: &mut dyn serialport::SerialPort,
) -> Result<(), HttpError> {
    for step in native_serial_app_reset_steps() {
        match step {
            NativeSerialLineStep::Dtr(level) => {
                port.write_data_terminal_ready(*level).map_err(|error| {
                    HttpError::retryable(
                        "native_serial_dtr_reset_failed",
                        format!("failed to set DTR={level} on {port_path}: {error}"),
                    )
                })?;
            }
            NativeSerialLineStep::Rts(level) => {
                port.write_request_to_send(*level).map_err(|error| {
                    HttpError::retryable(
                        "native_serial_rts_reset_failed",
                        format!("failed to set RTS={level} on {port_path}: {error}"),
                    )
                })?;
            }
            NativeSerialLineStep::SleepMs(ms) => std::thread::sleep(Duration::from_millis(*ms)),
        }
    }
    Ok(())
}

fn send_native_cdc_frame(
    port_path: &str,
    frame: Value,
    request_id: &str,
) -> Result<Value, HttpError> {
    let mut port = open_native_serial_port(port_path, Duration::from_millis(250), true)?;
    send_cdc_frame_on_port(&mut *port, port_path, frame, request_id, |_| {})
}

fn send_cdc_frame_on_port<F>(
    port: &mut dyn serialport::SerialPort,
    port_path: &str,
    frame: Value,
    request_id: &str,
    mut handle_unmatched_line: F,
) -> Result<Value, HttpError>
where
    F: FnMut(&[u8]),
{
    let payload = serde_json::to_string(&frame)
        .map_err(|error| HttpError::non_retryable("cdc_frame_encode_failed", error.to_string()))?;
    port.write_all(payload.as_bytes())
        .and_then(|_| port.write_all(b"\n"))
        .map_err(|error| {
            HttpError::retryable(
                "native_cdc_write_failed",
                format!("failed to write CDC command to {port_path}: {error}"),
            )
        })?;

    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    while std::time::Instant::now() < deadline {
        match port.read(&mut byte) {
            Ok(0) => continue,
            Ok(_) if byte[0] == b'\n' => {
                if let Some(frame) = parse_matching_cdc_response(&line, request_id)? {
                    return Ok(frame);
                }
                handle_unmatched_line(&line);
                line.clear();
            }
            Ok(_) => {
                if line.len() < 16 * 1024 {
                    line.push(byte[0]);
                } else {
                    line.clear();
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::TimedOut => continue,
            Err(error) => {
                return Err(HttpError::retryable(
                    "native_cdc_read_failed",
                    format!("failed to read CDC response from {port_path}: {error}"),
                ))
            }
        }
    }
    Err(HttpError::retryable(
        "native_cdc_timeout",
        format!("timed out waiting for CDC response from {port_path}"),
    ))
}

fn parse_matching_cdc_response(line: &[u8], request_id: &str) -> Result<Option<Value>, HttpError> {
    for frame in json_frames_from_cdc_line(line) {
        let matches_request = frame
            .get("request_id")
            .and_then(Value::as_str)
            .is_some_and(|candidate| candidate == request_id);
        if matches_request
            && matches!(
                frame.get("type").and_then(Value::as_str),
                Some("response" | "error")
            )
        {
            return Ok(Some(frame));
        }
    }
    Ok(None)
}

fn record_settings_success<F>(
    state: &AppState,
    device_id: &str,
    tx_frame: Value,
    rx_frame: Value,
    apply_settings: F,
    log_target: &str,
    log_message: &str,
) where
    F: FnOnce(&mut DeviceSettingsState),
{
    let tx_trace = trace_entry(
        "tx",
        &serde_json::to_string(&redact_cdc_frame(&tx_frame)).unwrap_or_default(),
    );
    let rx_trace = trace_entry(
        "rx",
        &serde_json::to_string(&redact_cdc_frame(&rx_frame)).unwrap_or_default(),
    );
    let log = SerialLogEntry {
        id: next_id(),
        timestamp: now(),
        level: "info".to_string(),
        target: log_target.to_string(),
        message: log_message.to_string(),
    };
    let snapshot = {
        let mut guard = state.inner.lock().expect("state lock");
        if let Some(device) = guard.devices.get_mut(device_id) {
            apply_settings(&mut device.settings);
            push_bounded(&mut device.trace, tx_trace.clone(), LOG_LIMIT);
            push_bounded(&mut device.trace, rx_trace.clone(), LOG_LIMIT);
            push_bounded(&mut device.logs, log.clone(), LOG_LIMIT);
            device.connection = ConnectionState::Connected;
        }
        persisted_snapshot(&guard)
    };
    let _ = persist_devd_state(&state.persistence, snapshot);
    emit(
        state,
        Some(device_id.to_string()),
        "serial_trace",
        "CDC trace frame",
        json!({"trace": tx_trace}),
    );
    emit(
        state,
        Some(device_id.to_string()),
        "serial_trace",
        "CDC trace frame",
        json!({"trace": rx_trace}),
    );
    emit(
        state,
        Some(device_id.to_string()),
        "serial_log",
        "CDC log frame",
        json!({"log": log}),
    );
}

fn error_from_cdc_response(response: &Value) -> HttpError {
    let error = response.get("error");
    let code = error
        .and_then(|error| error.get("code"))
        .and_then(Value::as_str)
        .unwrap_or("cdc_command_failed");
    let message = error
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("USB CDC command failed");
    let retryable = error
        .and_then(|error| error.get("retryable"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if retryable {
        HttpError::retryable(code, message)
    } else {
        HttpError::non_retryable(code, message)
    }
}

async fn device_events(
    Query(query): Query<WebLeaseQuery>,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Sse<impl futures_core::Stream<Item = Result<Event, std::convert::Infallible>>>, HttpError>
{
    {
        let guard = state.inner.lock().expect("state lock");
        ensure_web_lease_for_target(&guard, Some(&id), query.lease_id.as_deref())?;
    }
    let receiver = state.events.subscribe();
    let stream = async_stream::stream! {
        let mut receiver = receiver;
        while let Ok(event) = receiver.recv().await {
            if event.device_id.as_deref() == Some(id.as_str()) || event.device_id.is_none() {
                yield Ok(Event::default().event(event.kind.clone()).id(event.id.clone()).json_data(event).expect("serialize event"));
            }
        }
    };
    Ok(Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default()))
}

async fn devices_events(
    State(state): State<AppState>,
) -> Result<Sse<impl futures_core::Stream<Item = Result<Event, std::convert::Infallible>>>, HttpError>
{
    let receiver = state.events.subscribe();
    let stream = async_stream::stream! {
        let mut receiver = receiver;
        while let Ok(event) = receiver.recv().await {
            if matches!(
                event.kind.as_str(),
                "scan" | "bind" | "unbind" | "connect" | "disconnect" | "artifact" | "flash" | "reset" | "power_diag"
            ) {
                yield Ok(Event::default().event(event.kind.clone()).id(event.id.clone()).json_data(event).expect("serialize event"));
            }
        }
    };
    Ok(Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default()))
}

async fn devd_compat_events(
    Query(query): Query<WebLeaseQuery>,
    State(state): State<AppState>,
) -> Result<Sse<impl futures_core::Stream<Item = Result<Event, std::convert::Infallible>>>, HttpError>
{
    let leased_device_id = {
        let guard = state.inner.lock().expect("state lock");
        select_compat_device(&guard, query.lease_id.as_deref())?
            .id
            .clone()
    };
    let receiver = state.events.subscribe();
    let stream = async_stream::stream! {
        let mut receiver = receiver;
        while let Ok(event) = receiver.recv().await {
            if matches!(event.kind.as_str(), "serial_trace" | "serial_log" | "serial_status" | "monitor")
                && (event.device_id.as_deref() == Some(leased_device_id.as_str()) || event.device_id.is_none())
            {
                yield Ok(Event::default().event(event.kind.clone()).id(event.id.clone()).json_data(event).expect("serialize event"));
            }
        }
    };
    Ok(Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default()))
}

async fn host_power_events(
    State(state): State<AppState>,
) -> Result<Sse<impl futures_core::Stream<Item = Result<Event, std::convert::Infallible>>>, HttpError>
{
    let receiver = state.events.subscribe();
    let stream = async_stream::stream! {
        let mut receiver = receiver;
        while let Ok(event) = receiver.recv().await {
            if event.kind == "host_power" {
                yield Ok(Event::default().event(event.kind.clone()).id(event.id.clone()).json_data(event).expect("serialize event"));
            }
        }
    };
    Ok(Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default()))
}

async fn host_power_status(State(state): State<AppState>) -> Json<Value> {
    let (previous_profile, last_action) = {
        let guard = state.inner.lock().expect("state lock");
        (
            guard.host_power.previous_profile.clone(),
            guard.host_power.last_action.clone(),
        )
    };
    Json(json!({
        "backend": host_power_backend_name(),
        "platform": env::consts::OS,
        "real_actions_allowed": state.allow_host_power_actions,
        "default_dry_run": true,
        "capabilities": host_power_capabilities(),
        "state": query_host_power_state().await,
        "previous_profile": previous_profile,
        "last_action": last_action
    }))
}

async fn host_power_profile(
    State(state): State<AppState>,
    Json(input): Json<HostPowerProfileRequest>,
) -> Result<Json<Value>, HttpError> {
    let dry_run = input.dry_run.unwrap_or(true);
    ensure_host_power_action_allowed(&state, dry_run, "profile")?;
    let requested_profile = normalize_host_power_profile(&input.profile)?;
    let target_profile = if requested_profile == "restore_previous" {
        let previous = {
            let guard = state.inner.lock().expect("state lock");
            guard.host_power.previous_profile.clone()
        };
        previous.ok_or_else(|| {
            HttpError::non_retryable(
                "host_power_previous_profile_missing",
                "restore_previous requires a previously saved host power profile",
            )
        })?
    } else {
        requested_profile.clone()
    };
    let command = build_profile_command(&target_profile)?;
    let current_profile = query_current_host_power_profile().await.map_err(|error| {
        HttpError::retryable_with_details(
            "host_power_profile_query_failed",
            "failed to query current host power profile",
            error,
        )
    })?;
    if !dry_run {
        run_command_status(&command, "host_power_profile_failed").await?;
    }
    let existing_previous_profile = {
        let guard = state.inner.lock().expect("state lock");
        guard.host_power.previous_profile.clone()
    };
    let next_previous_profile = next_previous_profile(
        dry_run,
        &requested_profile,
        &target_profile,
        &current_profile,
        existing_previous_profile.as_deref(),
    );
    if next_previous_profile != existing_previous_profile {
        let mut guard = state.inner.lock().expect("state lock");
        guard.host_power.previous_profile = next_previous_profile;
    }
    let result = json!({
        "ok": true,
        "dry_run": dry_run,
        "backend": host_power_backend_name(),
        "action": "profile",
        "profile": requested_profile,
        "target_profile": target_profile,
        "previous_profile": current_profile,
        "profile_query": {
            "ok": true,
            "profile": current_profile
        },
        "command": command
    });
    record_host_power_action(&state, "host power profile requested", result.clone());
    Ok(Json(result))
}

async fn host_power_suspend(
    State(state): State<AppState>,
    input: Option<Json<HostPowerDryRunRequest>>,
) -> Result<Json<Value>, HttpError> {
    let dry_run = input
        .map(|Json(input)| input.dry_run)
        .flatten()
        .unwrap_or(true);
    let command = build_suspend_command()?;
    ensure_host_power_action_allowed(&state, dry_run, "suspend")?;
    if !dry_run {
        run_command_status(&command, "host_power_suspend_failed").await?;
    }
    let result = json!({
        "ok": true,
        "dry_run": dry_run,
        "backend": host_power_backend_name(),
        "action": "suspend",
        "command": command
    });
    record_host_power_action(&state, "host suspend requested", result.clone());
    Ok(Json(result))
}

async fn host_power_shutdown(
    State(state): State<AppState>,
    input: Option<Json<HostPowerShutdownRequest>>,
) -> Result<Json<Value>, HttpError> {
    let input = input
        .map(|Json(input)| input)
        .unwrap_or(HostPowerShutdownRequest {
            delay_sec: None,
            dry_run: None,
            confirm: None,
            force: None,
        });
    let dry_run = input.dry_run.unwrap_or(true);
    let delay_sec = input.delay_sec.unwrap_or(60);
    let force = input.force.unwrap_or(false);
    let command = build_shutdown_command(delay_sec, force)?;
    ensure_host_power_action_allowed(&state, dry_run, "shutdown")?;
    if !dry_run && input.confirm.as_deref() != Some("shutdown") {
        record_host_power_action(
            &state,
            "host shutdown request denied",
            json!({
                "ok": false,
                "dry_run": false,
                "backend": host_power_backend_name(),
                "action": "shutdown",
                "delay_sec": delay_sec,
                "error": {
                    "code": "host_power_shutdown_confirmation_required",
                    "message": "real shutdown requires confirm=\"shutdown\"",
                    "retryable": false,
                    "details": null
                },
                "command": command
            }),
        );
        return Err(HttpError::non_retryable(
            "host_power_shutdown_confirmation_required",
            "real shutdown requires confirm=\"shutdown\"",
        ));
    }
    if !dry_run {
        run_command_status(&command, "host_power_shutdown_failed").await?;
    }
    let result = json!({
        "ok": true,
        "dry_run": dry_run,
        "backend": host_power_backend_name(),
        "action": "shutdown",
        "delay_sec": delay_sec,
        "force": force,
        "command": command,
        "scheduled_after_sec": delay_sec,
        "dispatch": if dry_run { "not_dispatched" } else { "command_accepted" }
    });
    record_host_power_action(&state, "host shutdown requested", result.clone());
    Ok(Json(result))
}

async fn defmt_decode(Json(input): Json<DefmtDecodeRequest>) -> Result<Json<Value>, HttpError> {
    let elf_path = resolve_embedded_firmware_path(&input.elf_path)?;
    let elf = fs::read(&elf_path).map_err(|error| {
        HttpError::retryable(
            "defmt_elf_read_failed",
            format!("failed to read {}: {error}", elf_path.display()),
        )
    })?;
    let table = match Table::parse(&elf) {
        Ok(Some(table)) => table,
        Ok(None) => {
            return Err(HttpError::non_retryable(
                "defmt_table_missing",
                "ELF does not contain defmt metadata",
            ))
        }
        Err(error) => {
            return Err(HttpError::non_retryable(
                "defmt_table_parse_failed",
                format!("failed to parse defmt metadata: {error}"),
            ))
        }
    };
    if table.encoding() != defmt_decoder::Encoding::Rzcobs {
        return Err(HttpError::non_retryable(
            "defmt_encoding_unsupported",
            format!("unsupported defmt encoding: {:?}", table.encoding()),
        ));
    }

    let frame_bytes = decode_hex(&input.frame_hex)?;
    let mut decoder = table.new_stream_decoder();
    decoder.received(&frame_bytes);
    decoder.received(&[0]);
    let frame = decoder.decode().map_err(|error| {
        HttpError::retryable(
            "defmt_frame_decode_failed",
            format!("failed to decode defmt frame: {error}"),
        )
    })?;
    let formatter = Formatter::new(FormatterConfig {
        format: FormatterFormat::Custom("{s}"),
        is_timestamp_available: table.has_timestamp(),
    });
    let level = frame
        .level()
        .map(|level| format!("{level:?}").to_lowercase());
    let index = frame.index();
    let message = formatter.format_frame(frame, None, None, None);
    Ok(Json(json!({
        "level": level.unwrap_or_else(|| "info".to_string()),
        "target": "defmt",
        "message": message,
        "index": index,
    })))
}

fn resolve_embedded_firmware_path(input: &str) -> Result<PathBuf, HttpError> {
    let trimmed = input.trim().trim_start_matches('/');
    if trimmed.is_empty()
        || trimmed.contains("..")
        || trimmed.starts_with('~')
        || FsPath::new(trimmed).is_absolute()
    {
        return Err(HttpError::non_retryable(
            "defmt_elf_path_invalid",
            "ELF path must be a relative embedded firmware path",
        ));
    }
    let firmware_rel = trimmed.strip_prefix("firmware/").unwrap_or(trimmed);
    let path = env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("web/public/firmware")
        .join(firmware_rel);
    if !path.is_file() {
        if let Some(dev_path) = resolve_web_dev_firmware_path(firmware_rel)? {
            return Ok(dev_path);
        }
        return Err(HttpError::not_found(
            "defmt_elf_not_found",
            format!("embedded firmware ELF not found: {trimmed}"),
        ));
    }
    Ok(path)
}

fn resolve_web_dev_firmware_path(firmware_rel: &str) -> Result<Option<PathBuf>, HttpError> {
    let mut parts = firmware_rel.split('/');
    let Some(artifact_id) = parts.next() else {
        return Ok(None);
    };
    let Some(file_name) = parts.next_back() else {
        return Ok(None);
    };
    if artifact_id.is_empty()
        || file_name.is_empty()
        || parts.any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Ok(None);
    }

    let manifest_path = env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(WEB_DEV_FIRMWARE_CACHE_DIR)
        .join(format!("{artifact_id}.manifest.json"));
    if !manifest_path.is_file() {
        return Ok(None);
    }
    let manifest = read_manifest(&manifest_path.to_string_lossy())?;
    if manifest.artifact_id != artifact_id {
        return Ok(None);
    }
    for file in manifest.files {
        let path = PathBuf::from(&file.path);
        if path.file_name().and_then(|name| name.to_str()) == Some(file_name) && path.is_file() {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn decode_hex(input: &str) -> Result<Vec<u8>, HttpError> {
    let compact = input
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect::<Vec<_>>();
    if compact.len() % 2 != 0 {
        return Err(HttpError::non_retryable(
            "defmt_hex_invalid",
            "hex payload must have an even number of digits",
        ));
    }
    let mut output = Vec::with_capacity(compact.len() / 2);
    for pair in compact.chunks_exact(2) {
        let hi = hex_value(pair[0])?;
        let lo = hex_value(pair[1])?;
        output.push((hi << 4) | lo);
    }
    Ok(output)
}

fn hex_value(byte: u8) -> Result<u8, HttpError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(HttpError::non_retryable(
            "defmt_hex_invalid",
            "hex payload contains a non-hex digit",
        )),
    }
}

fn seed_mock_device(state: &AppState) {
    let mut guard = state.inner.lock().expect("state lock");
    let id = "mock-devkit".to_string();
    let binding = guard.bindings.get(&id).cloned();
    let selected_artifact_id = guard.selected_artifacts.get(&id).cloned();
    let selected_artifact = selected_artifact_id
        .as_ref()
        .and_then(|artifact_id| guard.artifacts.get(artifact_id).cloned());
    let persisted_trace = guard
        .persisted_device_trace
        .get(&id)
        .cloned()
        .unwrap_or_default();
    let mut device = DeviceRecord {
        id: id.clone(),
        display_name: "Mock ESP32-S3 DevKit".to_string(),
        port_path: None,
        lan_address: None,
        lan_conflict_addresses: Vec::new(),
        transport: DeviceTransport::Mock,
        binding,
        connection: ConnectionState::Disconnected,
        identity: Some(mock_identity(&id)),
        status: None,
        power_diag: None,
        selected_artifact_id,
        log_decode: LogDecodeState::default(),
        settings: default_settings(),
        logs: VecDeque::new(),
        trace: persisted_trace,
    };
    apply_artifact_match(&mut device, selected_artifact.as_ref());
    guard.devices.insert(id, device);
}

fn ensure_device(state: &AppState, id: &str) -> Result<(), HttpError> {
    let guard = state.inner.lock().expect("state lock");
    guard
        .devices
        .get(id)
        .map(|_| ())
        .ok_or_else(|| HttpError::not_found("device_not_found", "device is not known"))
}

fn read_manifest(path: &str) -> Result<FirmwareArtifact, HttpError> {
    let text = fs::read_to_string(path)
        .map_err(|error| HttpError::retryable("manifest_read_failed", error.to_string()))?;
    let mut artifact: FirmwareArtifact = serde_json::from_str(&text)
        .map_err(|error| HttpError::non_retryable("manifest_invalid", error.to_string()))?;
    if let Some(base) = FsPath::new(path).parent() {
        for file in &mut artifact.files {
            let file_path = FsPath::new(&file.path);
            if file_path.is_relative() {
                file.path = base.join(file_path).to_string_lossy().to_string();
            }
        }
    }
    Ok(artifact)
}

fn verify_artifact_files(artifact: &FirmwareArtifact) -> Result<(), HttpError> {
    for file in &artifact.files {
        let bytes = fs::read(&file.path).map_err(|error| {
            HttpError::retryable("artifact_read_failed", format!("{}: {error}", file.path))
        })?;
        let sha = format!("{:x}", Sha256::digest(&bytes));
        if sha != file.sha256 {
            return Err(HttpError::non_retryable(
                "artifact_sha256_mismatch",
                format!("{} hash mismatch", file.path),
            ));
        }
    }
    Ok(())
}

fn apply_artifact_match(device: &mut DeviceRecord, artifact: Option<&FirmwareArtifact>) {
    let Some(artifact) = artifact else {
        device.log_decode = LogDecodeState::default();
        return;
    };
    let firmware = device
        .identity
        .as_ref()
        .and_then(|identity| identity.get("firmware"));
    let device_build_id = firmware
        .and_then(|fw| fw.get("build_id"))
        .and_then(Value::as_str);
    let matched = device_build_id == Some(artifact.build_id.as_str())
        && identity_profile_match(firmware, &artifact.profile)
        && identity_features_match(firmware, &artifact.features);
    device.log_decode = if matched {
        LogDecodeState {
            status: "verified".to_string(),
            reason: None,
            artifact_id: Some(artifact.artifact_id.clone()),
        }
    } else {
        LogDecodeState {
            status: "unverified".to_string(),
            reason: Some("device firmware identity does not match selected artifact".to_string()),
            artifact_id: Some(artifact.artifact_id.clone()),
        }
    };
}

fn identity_profile_match(firmware: Option<&Value>, artifact_profile: &str) -> bool {
    firmware
        .and_then(|fw| fw.get("build_profile"))
        .and_then(Value::as_str)
        == Some(artifact_profile)
}

fn identity_features_match(firmware: Option<&Value>, artifact_features: &[String]) -> bool {
    let Some(features) = firmware
        .and_then(|fw| fw.get("features"))
        .and_then(Value::as_array)
    else {
        return artifact_features.is_empty();
    };
    let mut identity = features
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<Vec<_>>();
    let mut artifact = artifact_features.to_vec();
    identity.sort();
    artifact.sort();
    identity == artifact
}

fn bound_flash_port(device: &DeviceRecord) -> Option<String> {
    device
        .binding
        .as_ref()
        .and_then(|binding| binding.port_path.clone())
}

fn prefer_serial_port_path(current: Option<&str>, candidate: &str) -> String {
    match current {
        None => candidate.to_string(),
        Some(current) if serial_path_score(candidate) > serial_path_score(current) => {
            candidate.to_string()
        }
        Some(current) => current.to_string(),
    }
}

fn serial_path_score(path: &str) -> u8 {
    if path.contains("/cu.") {
        2
    } else if path.contains("/tty.") {
        1
    } else {
        0
    }
}

fn is_native_usb_serial_candidate(port: &serialport::SerialPortInfo) -> bool {
    let port_name = port.port_name.to_ascii_lowercase();
    if port_name.contains("bluetooth") || port_name.contains("debug-console") {
        return false;
    }
    if matches!(port.port_type, serialport::SerialPortType::UsbPort(_)) {
        return true;
    }
    port_name.contains("usbmodem")
        || port_name.contains("usbserial")
        || port_name.contains("wchusbserial")
        || port_name.contains("slab_usb")
}

fn stable_device_id(port: &serialport::SerialPortInfo) -> String {
    let mut hash = Sha256::new();
    match &port.port_type {
        serialport::SerialPortType::UsbPort(usb) => {
            hash.update(b"usb:");
            hash.update(format!("{:04x}:{:04x}:", usb.vid, usb.pid));
            hash.update(usb.serial_number.as_deref().unwrap_or("no-serial"));
            if usb.serial_number.is_none() {
                hash.update(b":local-port:");
                hash.update(port.port_name.as_bytes());
            }
            hash.update(b":");
            hash.update(usb.manufacturer.as_deref().unwrap_or(""));
            hash.update(b":");
            hash.update(usb.product.as_deref().unwrap_or(""));
        }
        _ => {
            hash.update(b"port:");
            hash.update(port.port_name.as_bytes());
        }
    }
    let digest = format!("{:x}", hash.finalize());
    format!("serial-{}", &digest[..12])
}

fn mock_identity(id: &str) -> Value {
    json!({
        "device_id": id,
        "hostname": id,
        "hostname_fqdn": format!("{id}.local"),
        "short_id": id.chars().rev().take(6).collect::<String>(),
        "role": "ups",
        "api_version": "v1",
        "firmware": {
            "package_version": env!("CARGO_PKG_VERSION"),
            "build_profile": "dev",
            "build_id": "mock-devd-build",
            "git_sha": "unknown",
            "src_hash": "unknown",
            "git_dirty": "unknown",
            "features": ["net_http", "web_serial"],
            "protocol": "mains-aegis.cdc.v1",
            "defmt": {"enabled": true, "encoding": "defmt-espflash", "table_hash": null}
        },
        "network": {
            "device_id": id,
            "hostname": id,
            "hostname_fqdn": format!("{id}.local"),
            "state": "idle",
            "ipv4": null,
            "gateway": null,
            "dns": null,
            "is_static": false,
            "last_error": null,
            "rssi_dbm": null
        },
        "capabilities": {"sse": true, "mdns": true, "dns_sd": true, "write_controls": true, "devd": true}
    })
}

fn mock_power_diag() -> Value {
    json!({
        "input": {
            "mains_present": null,
            "input_vbus_mv": null,
            "input_ibus_ma": null,
            "vin_vbus_mv": null,
            "vin_iin_ma": null,
            "usb_pd_attached": false,
            "usb_pd_charge_ready": false,
            "usb_pd_vbus_present": null,
            "usb_pd_unsafe_source_latched": false,
            "usb_pd_contract_kind": null,
            "usb_pd_contract_mv": null,
            "usb_pd_contract_ma": null,
            "usb_pd_vac1_mv": null,
            "usb_pd_vsys_mv": null
        },
        "charger": {
            "poll_valid": false,
            "enabled": false,
            "ce_low": false,
            "ilim_hiz_brk_low": false,
            "allow_charge": false,
            "normal_allow_charge": false,
            "force_allow_charge": false,
            "can_enable": false,
            "usb_pd_charge_gate_ready": false,
            "input_present": false,
            "vbus_present": false,
            "ac1_present": false,
            "ac2_present": false,
            "pg": false,
            "vbat_present": false,
            "adc_enabled": false,
            "adc_done": false,
            "adc_ready": false,
            "ibus_adc_ma": null,
            "ibat_adc_ma": null,
            "vbus_adc_mv": null,
            "vbat_adc_mv": null,
            "vsys_adc_mv": null,
            "vac1_adc_mv": null,
            "vreg_mv": null,
            "ichg_ma": null,
            "vindpm_mv": null,
            "iindpm_ma": null,
            "iterm_ma": null,
            "chg_stat": "unknown",
            "vbus_stat": "unknown",
            "ico_stat": "unknown",
            "treg": false,
            "dpdm": false,
            "wd": false,
            "poorsrc": false,
            "vindpm": false,
            "iindpm": false,
            "ts_cold": false,
            "ts_hot": false,
            "st0": null,
            "st1": null,
            "st2": null,
            "st3": null,
            "st4": null,
            "fault0": null,
            "fault1": null,
            "ctrl0": null,
            "term_ctrl": null
        },
        "policy": {
            "state": null,
            "status": "unknown",
            "notice": "unavailable",
            "input_source": "unknown",
            "start_reason": null,
            "full_reason": null,
            "output_block_reason": null,
            "target_ichg_ma": null,
            "output_power_w10": null,
            "charge_latched": false,
            "full_latched": false,
            "dc_derated": false,
            "output_blocked": false,
            "manual_active": false,
            "manual_stop_inhibit": false
        },
        "bms": {
            "addr": null,
            "state": "pending",
            "pack_mv": null,
            "current_ma": null,
            "soc_pct": null,
            "cell_min_mv": null,
            "cell_max_mv": null,
            "no_battery": null,
            "discharge_ready": null,
            "charge_ready": null,
            "full": null,
            "issue_detail": null,
            "rca_alarm": null,
            "safety_status": null,
            "pf_status": null,
            "manufacturing_status": null,
            "gauging_status": null,
            "op_status": null,
            "xchg": null,
            "chg_fet": null,
            "dsg_fet": null,
            "pchg_fet": null,
            "cuv": null,
            "cuvc": null,
            "fet_en": null,
            "chg_en": null,
            "dsg_en": null,
            "charging_inhibit": null,
            "charging_suspend": null,
            "charging_hv": null,
            "current_at_eoc_ma": null
        }
    })
}

async fn read_native_identity_async(port_path: String) -> Result<Value, HttpError> {
    tokio::task::spawn_blocking(move || read_native_identity(&port_path))
        .await
        .map_err(|error| HttpError::retryable("native_identity_join_failed", error.to_string()))?
}

async fn read_device_identity_async(
    port_path: String,
    monitor_command_tx: Option<mpsc::Sender<NativeMonitorCommand>>,
) -> Result<Value, HttpError> {
    if let Some(command_tx) = monitor_command_tx {
        let request_id = "devd-identity".to_string();
        let frame = json!({"type": "request", "request_id": request_id, "op": "get_identity"});
        let response = send_monitor_cdc_frame_async(command_tx, frame, request_id).await?;
        return response.get("result").cloned().ok_or_else(|| {
            HttpError::retryable(
                "native_identity_missing",
                "identity response did not include result",
            )
        });
    }
    read_native_identity_async(port_path).await
}

async fn reset_native_serial_async(port_path: String) -> Result<(), HttpError> {
    tokio::task::spawn_blocking(move || {
        let mut port = open_native_serial_port(&port_path, Duration::from_millis(250), false)?;
        reset_native_serial_to_app_on_port(&port_path, &mut *port)
    })
    .await
    .map_err(|error| HttpError::retryable("native_reset_join_failed", error.to_string()))?
}

struct MonitorStartResult {
    trace_count: usize,
    log_count: usize,
    already_running: bool,
}

fn start_native_monitor(
    state: &AppState,
    device_id: String,
    port_path: String,
) -> Result<MonitorStartResult, HttpError> {
    let stop = Arc::new(AtomicBool::new(false));
    let (command_tx, command_rx) = mpsc::channel();
    {
        let mut guard = state.inner.lock().expect("state lock");
        if guard.monitors.contains_key(&device_id) {
            let device = guard
                .devices
                .get(&device_id)
                .ok_or_else(|| HttpError::not_found("device_not_found", "device is not known"))?;
            return Ok(MonitorStartResult {
                trace_count: device.trace.len(),
                log_count: device.logs.len(),
                already_running: true,
            });
        }
        guard.monitors.insert(
            device_id.clone(),
            NativeMonitorHandle {
                stop: stop.clone(),
                command_tx,
            },
        );
    }
    let state = state.clone();
    std::thread::spawn(move || run_native_monitor(state, device_id, port_path, stop, command_rx));
    Ok(MonitorStartResult {
        trace_count: 0,
        log_count: 0,
        already_running: false,
    })
}

fn run_native_monitor(
    state: AppState,
    device_id: String,
    port_path: String,
    stop: Arc<AtomicBool>,
    command_rx: mpsc::Receiver<NativeMonitorCommand>,
) {
    if let Err(error) = run_native_monitor_inner(&state, &device_id, &port_path, &stop, command_rx)
    {
        let mut guard = state.inner.lock().expect("state lock");
        guard.monitors.remove(&device_id);
        if let Some(device) = guard.devices.get_mut(&device_id) {
            push_log(
                device,
                "error",
                "monitor",
                format!("monitor stopped: {}", error.0.message).as_str(),
            );
            device.connection = ConnectionState::Error;
        }
    }
}

fn native_monitor_ingest_byte(
    byte: u8,
    cdc_line: &mut Vec<u8>,
    json_candidate: &mut Vec<u8>,
) -> NativeMonitorInput {
    const JSON_FRAME_PREFIX: &[u8] = br#"{"type""#;

    if !json_candidate.is_empty() {
        json_candidate.push(byte);
        if json_candidate.len() <= JSON_FRAME_PREFIX.len()
            && json_candidate.as_slice() != &JSON_FRAME_PREFIX[..json_candidate.len()]
        {
            return NativeMonitorInput::DefmtBytes(std::mem::take(json_candidate));
        }
        if byte == 0 {
            return NativeMonitorInput::DefmtBytes(std::mem::take(json_candidate));
        }
        if byte == b'\n' {
            return NativeMonitorInput::CdcLine(std::mem::take(json_candidate));
        }
        if json_candidate.len() > 16 * 1024 {
            return NativeMonitorInput::DefmtBytes(std::mem::take(json_candidate));
        }
        return NativeMonitorInput::None;
    }

    if !cdc_line.is_empty() {
        cdc_line.push(byte);
        if byte == b'\n' {
            let line = std::mem::take(cdc_line);
            return NativeMonitorInput::CdcLine(line);
        }
        if byte == 0 || cdc_line.len() > 16 * 1024 {
            let bytes = std::mem::take(cdc_line);
            return NativeMonitorInput::DefmtBytes(bytes);
        }
        return NativeMonitorInput::None;
    }

    if byte == b'{' {
        json_candidate.push(byte);
        return NativeMonitorInput::None;
    }

    NativeMonitorInput::DefmtBytes(vec![byte])
}

fn run_native_monitor_inner(
    state: &AppState,
    device_id: &str,
    port_path: &str,
    stop: &AtomicBool,
    command_rx: mpsc::Receiver<NativeMonitorCommand>,
) -> Result<(), HttpError> {
    let defmt_table = load_native_monitor_defmt_table(state, device_id)?;
    let mut defmt_decoder = defmt_table.as_ref().map(|table| table.new_stream_decoder());
    let mut port = open_native_monitor_serial_port(port_path, Duration::from_millis(250))?;
    let mut next_status_at = std::time::Instant::now() + Duration::from_secs(1);
    let mut cdc_line = Vec::new();
    let mut json_candidate = Vec::new();
    let mut defmt_raw = Vec::new();
    let mut byte = [0u8; 1];
    while !stop.load(Ordering::SeqCst) {
        while let Ok(command) = command_rx.try_recv() {
            handle_native_monitor_command(state, device_id, port_path, &mut *port, command);
        }
        if std::time::Instant::now() >= next_status_at {
            let request =
                r#"{"type":"request","request_id":"devd-monitor-status","op":"get_status"}"#;
            port.write_all(request.as_bytes())
                .and_then(|_| port.write_all(b"\n"))
                .map_err(|error| {
                    HttpError::retryable(
                        "native_monitor_write_failed",
                        format!("failed to request monitor sample from {port_path}: {error}"),
                    )
                })?;
            append_monitor_trace(state, device_id, trace_entry("tx", request), None);
            next_status_at = std::time::Instant::now() + Duration::from_secs(2);
        }
        match port.read(&mut byte) {
            Ok(0) => continue,
            Ok(_) => {
                match native_monitor_ingest_byte(byte[0], &mut cdc_line, &mut json_candidate) {
                    NativeMonitorInput::CdcLine(line) => {
                        if let Some((trace, log)) = parse_cdc_line_for_monitor(&line) {
                            append_monitor_trace(state, device_id, trace, log);
                        }
                    }
                    NativeMonitorInput::DefmtBytes(bytes) => {
                        if let Some(decoder) = defmt_decoder.as_mut() {
                            let reached_boundary = bytes.contains(&0);
                            defmt_raw.extend_from_slice(&bytes);
                            decoder.received(&bytes);
                            if reached_boundary {
                                let mut emitted = false;
                                loop {
                                    match decoder.decode() {
                                        Ok(frame) => {
                                            emitted = true;
                                            let level = frame
                                                .level()
                                                .map(|level| format!("{level:?}").to_lowercase())
                                                .unwrap_or_else(|| "info".to_string());
                                            let formatter = Formatter::new(FormatterConfig {
                                                format: FormatterFormat::Custom("{s}"),
                                                is_timestamp_available: false,
                                            });
                                            let message =
                                                formatter.format_frame(frame, None, None, None);
                                            append_monitor_trace(
                                                state,
                                                device_id,
                                                structured_trace_entry(
                                                    "rx",
                                                    "defmt",
                                                    Some("defmt".to_string()),
                                                    &message,
                                                    hex_preview(&defmt_raw),
                                                ),
                                                Some(SerialLogEntry {
                                                    id: next_id(),
                                                    timestamp: now(),
                                                    level,
                                                    target: "defmt".to_string(),
                                                    message,
                                                }),
                                            );
                                        }
                                        Err(defmt_decoder::DecodeError::UnexpectedEof) => break,
                                        Err(error) => {
                                            append_monitor_trace(
                                                state,
                                                device_id,
                                                raw_trace_entry(
                                                    "rx",
                                                    "defmt",
                                                    "defmt decode error",
                                                    &format!(
                                                        "{} ({} bytes)",
                                                        error,
                                                        defmt_raw.len()
                                                    ),
                                                ),
                                                None,
                                            );
                                            break;
                                        }
                                    }
                                }
                                if emitted || !defmt_raw.is_empty() {
                                    defmt_raw.clear();
                                }
                            } else if defmt_raw.len() > 16 * 1024 {
                                append_monitor_trace(
                                    state,
                                    device_id,
                                    raw_trace_entry(
                                        "rx",
                                        "defmt",
                                        "defmt binary frame exceeded 16 KiB",
                                        &hex_preview(&defmt_raw),
                                    ),
                                    None,
                                );
                                defmt_raw.clear();
                            }
                        }
                    }
                    NativeMonitorInput::None => {}
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::TimedOut => continue,
            Err(error) => {
                return Err(HttpError::retryable(
                    "native_monitor_read_failed",
                    format!("failed to read monitor sample from {port_path}: {error}"),
                ))
            }
        }
    }
    let mut guard = state.inner.lock().expect("state lock");
    guard.monitors.remove(device_id);
    Ok(())
}

fn load_native_monitor_defmt_table(
    state: &AppState,
    device_id: &str,
) -> Result<Option<Table>, HttpError> {
    let artifact = {
        let guard = state.inner.lock().expect("state lock");
        let device = guard
            .devices
            .get(device_id)
            .ok_or_else(|| HttpError::not_found("device_not_found", "device is not known"))?;
        let Some(artifact_id) = device.selected_artifact_id.as_ref() else {
            return Ok(None);
        };
        let Some(artifact) = guard.artifacts.get(artifact_id) else {
            return Ok(None);
        };
        artifact.clone()
    };
    let Some(elf_path) = native_monitor_defmt_elf_path(&artifact)? else {
        return Ok(None);
    };
    let elf = fs::read(&elf_path).map_err(|error| {
        HttpError::retryable(
            "defmt_elf_read_failed",
            format!("failed to read {}: {error}", elf_path.display()),
        )
    })?;
    let table = match Table::parse(&elf) {
        Ok(Some(table)) => table,
        Ok(None) => {
            return Err(HttpError::non_retryable(
                "defmt_table_missing",
                format!(
                    "selected artifact {} does not contain defmt metadata",
                    artifact.artifact_id
                ),
            ))
        }
        Err(error) => {
            return Err(HttpError::non_retryable(
                "defmt_table_parse_failed",
                format!(
                    "failed to parse defmt metadata for {}: {error}",
                    elf_path.display()
                ),
            ))
        }
    };
    if table.encoding() != defmt_decoder::Encoding::Rzcobs {
        return Err(HttpError::non_retryable(
            "defmt_encoding_unsupported",
            format!(
                "unsupported defmt encoding for {}: {:?}",
                elf_path.display(),
                table.encoding()
            ),
        ));
    }
    Ok(Some(table))
}

fn native_monitor_defmt_elf_path(
    artifact: &FirmwareArtifact,
) -> Result<Option<PathBuf>, HttpError> {
    if !artifact.defmt.enabled {
        return Ok(None);
    }
    artifact
        .files
        .iter()
        .find(|file| file.kind == "elf")
        .map(|file| Ok(Some(PathBuf::from(&file.path))))
        .unwrap_or_else(|| {
            Err(HttpError::non_retryable(
                "defmt_elf_missing",
                format!(
                    "selected artifact {} has defmt enabled but does not include an ELF file",
                    artifact.artifact_id
                ),
            ))
        })
}

fn handle_native_monitor_command(
    state: &AppState,
    device_id: &str,
    port_path: &str,
    port: &mut dyn serialport::SerialPort,
    command: NativeMonitorCommand,
) {
    match command {
        NativeMonitorCommand::SendFrame {
            frame,
            request_id,
            response_tx,
        } => {
            let result = send_cdc_frame_on_port(port, port_path, frame, &request_id, |line| {
                if let Some((trace, log)) = parse_cdc_line_for_monitor(line) {
                    append_monitor_trace(state, device_id, trace, log);
                }
            });
            let _ = response_tx.send(result);
        }
        NativeMonitorCommand::Reset { response_tx } => {
            let result = reset_native_serial_to_app_on_port(port_path, port);
            let _ = response_tx.send(result);
        }
    }
}

fn append_monitor_trace(
    state: &AppState,
    device_id: &str,
    trace: SerialTraceEntry,
    log: Option<SerialLogEntry>,
) {
    let trace_event = trace.clone();
    let log_event = log.clone();
    let status_event = status_from_trace_payload(&trace_event.payload);
    let snapshot = {
        let mut guard = state.inner.lock().expect("state lock");
        if let Some(device) = guard.devices.get_mut(device_id) {
            device.connection = ConnectionState::Connected;
            push_bounded(&mut device.trace, trace, LOG_LIMIT);
            if let Some(status) = status_event.clone() {
                device.status = Some(status);
            }
            if let Some(log) = log {
                push_bounded(&mut device.logs, log, LOG_LIMIT);
            }
        }
        persisted_snapshot(&guard)
    };
    let _ = persist_devd_state(&state.persistence, snapshot);
    emit(
        state,
        Some(device_id.to_string()),
        "serial_trace",
        "CDC trace frame",
        json!({"trace": trace_event}),
    );
    if let Some(log) = log_event {
        emit(
            state,
            Some(device_id.to_string()),
            "serial_log",
            "CDC log frame",
            json!({"log": log}),
        );
    }
    if let Some(status) = status_event {
        emit(
            state,
            Some(device_id.to_string()),
            "serial_status",
            "CDC status snapshot",
            json!({"status": status}),
        );
    }
}

fn status_from_trace_payload(payload: &str) -> Option<Value> {
    let frame = serde_json::from_str::<Value>(payload).ok()?;
    match frame.get("type").and_then(Value::as_str) {
        Some("status") => frame.get("status").cloned(),
        Some("response")
            if frame.get("ok").and_then(Value::as_bool).unwrap_or(false)
                && frame
                    .get("request_id")
                    .and_then(Value::as_str)
                    .is_some_and(|request_id| request_id == "devd-monitor-status") =>
        {
            frame.get("result").cloned()
        }
        _ => None,
    }
}

fn reset_backend_name(transport: &DeviceTransport) -> &'static str {
    match transport {
        DeviceTransport::NativeSerial => "native_serial_lines",
        DeviceTransport::Lan => "lan_http",
        DeviceTransport::Mock => "mock",
    }
}

fn read_native_identity(port_path: &str) -> Result<Value, HttpError> {
    let mut port = open_native_serial_port(port_path, Duration::from_millis(250), true)?;
    port.write_all(br#"{"type":"request","request_id":"devd-identity","op":"get_identity"}"#)
        .and_then(|_| port.write_all(b"\n"))
        .map_err(|error| {
            HttpError::retryable(
                "native_identity_write_failed",
                format!("failed to request identity from {port_path}: {error}"),
            )
        })?;

    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    while std::time::Instant::now() < deadline {
        match port.read(&mut byte) {
            Ok(0) => continue,
            Ok(_) if byte[0] == b'\n' => {
                if let Some(identity) = parse_identity_line(&line)? {
                    return Ok(identity);
                }
                line.clear();
            }
            Ok(_) => {
                if line.len() < 16 * 1024 {
                    line.push(byte[0]);
                } else {
                    line.clear();
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::TimedOut => continue,
            Err(error) => {
                return Err(HttpError::retryable(
                    "native_identity_read_failed",
                    format!("failed to read identity from {port_path}: {error}"),
                ))
            }
        }
    }
    Err(HttpError::retryable(
        "native_identity_timeout",
        format!("timed out waiting for identity from {port_path}"),
    ))
}

fn parse_identity_line(line: &[u8]) -> Result<Option<Value>, HttpError> {
    for frame in json_frames_from_cdc_line(line) {
        match frame.get("type").and_then(Value::as_str) {
            Some("response")
                if frame.get("request_id").and_then(Value::as_str) == Some("devd-identity") =>
            {
                return Ok(frame.get("result").cloned());
            }
            Some("hello") => return Ok(frame.get("identity").cloned()),
            _ => {}
        }
    }
    Ok(None)
}

fn parse_cdc_line_for_monitor(line: &[u8]) -> Option<(SerialTraceEntry, Option<SerialLogEntry>)> {
    if let Some(frame) = json_frames_from_cdc_line(line).into_iter().next() {
        let payload = serde_json::to_string(&frame).ok()?;
        let trace = trace_entry("rx", &payload);
        let log =
            (frame.get("type").and_then(Value::as_str) == Some("log")).then(|| SerialLogEntry {
                id: next_id(),
                timestamp: now(),
                level: frame
                    .get("level")
                    .and_then(Value::as_str)
                    .unwrap_or("info")
                    .to_string(),
                target: frame
                    .get("target")
                    .and_then(Value::as_str)
                    .unwrap_or("usb_cdc")
                    .to_string(),
                message: frame
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("CDC log")
                    .to_string(),
            });
        return Some((trace, log));
    }
    let Ok(text) = std::str::from_utf8(line) else {
        return Some((
            raw_trace_entry("rx", "defmt", "defmt binary frame", &hex_preview(line)),
            None,
        ));
    };
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let trace = trace_entry("rx", text);
    let log = serde_json::from_str::<Value>(text)
        .ok()
        .filter(|frame| frame.get("type").and_then(Value::as_str) == Some("log"))
        .map(|frame| SerialLogEntry {
            id: next_id(),
            timestamp: now(),
            level: frame
                .get("level")
                .and_then(Value::as_str)
                .unwrap_or("info")
                .to_string(),
            target: frame
                .get("target")
                .and_then(Value::as_str)
                .unwrap_or("usb_cdc")
                .to_string(),
            message: frame
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("CDC log")
                .to_string(),
        });
    Some((trace, log))
}

fn json_frames_from_cdc_line(line: &[u8]) -> Vec<Value> {
    let mut frames = Vec::new();
    for (start, byte) in line.iter().enumerate() {
        if *byte != b'{' {
            continue;
        }
        let mut depth = 0usize;
        let mut in_string = false;
        let mut escape = false;
        for (offset, current) in line[start..].iter().enumerate() {
            if in_string {
                if escape {
                    escape = false;
                } else if *current == b'\\' {
                    escape = true;
                } else if *current == b'"' {
                    in_string = false;
                }
                continue;
            }
            match *current {
                b'"' => in_string = true,
                b'{' => depth += 1,
                b'}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        let end = start + offset + 1;
                        if let Ok(text) = std::str::from_utf8(&line[start..end]) {
                            if let Ok(frame) = serde_json::from_str::<Value>(text) {
                                frames.push(frame);
                            }
                        }
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    frames
}

fn trace_entry(direction: &str, payload: &str) -> SerialTraceEntry {
    let Ok(frame) = serde_json::from_str::<Value>(payload) else {
        return raw_trace_entry(direction, "raw", "raw CDC line", payload);
    };
    SerialTraceEntry {
        id: next_id(),
        timestamp: now(),
        direction: direction.to_string(),
        kind: "frame".to_string(),
        frame_type: frame
            .get("type")
            .and_then(Value::as_str)
            .map(str::to_string),
        request_id: frame
            .get("request_id")
            .and_then(Value::as_str)
            .map(str::to_string),
        target: frame
            .get("target")
            .and_then(Value::as_str)
            .map(str::to_string),
        summary: summarize_cdc_frame(&frame),
        payload: payload.to_string(),
    }
}

fn redact_cdc_frame(frame: &Value) -> Value {
    let mut redacted = frame.clone();
    if redacted.get("type").and_then(Value::as_str) == Some("wifi_config") {
        if let Some(object) = redacted.as_object_mut() {
            if object.contains_key("psk") {
                object.insert("psk".to_string(), Value::String("[redacted]".to_string()));
            }
        }
    }
    redacted
}

fn structured_trace_entry(
    direction: &str,
    kind: &str,
    target: Option<String>,
    summary: &str,
    payload: String,
) -> SerialTraceEntry {
    SerialTraceEntry {
        id: next_id(),
        timestamp: now(),
        direction: direction.to_string(),
        kind: kind.to_string(),
        frame_type: None,
        request_id: None,
        target,
        summary: summary.to_string(),
        payload,
    }
}

fn raw_trace_entry(direction: &str, kind: &str, summary: &str, payload: &str) -> SerialTraceEntry {
    SerialTraceEntry {
        id: next_id(),
        timestamp: now(),
        direction: direction.to_string(),
        kind: kind.to_string(),
        frame_type: None,
        request_id: None,
        target: None,
        summary: summary.to_string(),
        payload: payload.to_string(),
    }
}

fn hex_preview(bytes: &[u8]) -> String {
    let mut output = bytes
        .iter()
        .take(96)
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ");
    if bytes.len() > 96 {
        output.push_str(&format!(" ... ({} bytes)", bytes.len()));
    }
    output
}

fn summarize_cdc_frame(frame: &Value) -> String {
    match frame.get("type").and_then(Value::as_str) {
        Some("log") => frame
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("log")
            .to_string(),
        Some("error") => frame
            .get("error")
            .and_then(|error| error.get("code"))
            .and_then(Value::as_str)
            .unwrap_or("error")
            .to_string(),
        Some("response") => "command response".to_string(),
        Some("status") => "status snapshot".to_string(),
        Some("hello") => "protocol handshake".to_string(),
        Some("request") => frame
            .get("op")
            .and_then(Value::as_str)
            .unwrap_or("request")
            .to_string(),
        Some(kind) => kind.to_string(),
        None => "serial frame".to_string(),
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct CommandSpec {
    program: String,
    args: Vec<String>,
}

impl CommandSpec {
    fn new(program: impl Into<String>, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }
}

fn host_power_backend_name() -> &'static str {
    if cfg!(target_os = "linux") {
        "linux-systemd"
    } else if cfg!(target_os = "macos") {
        "macos-pmset"
    } else {
        "unsupported"
    }
}

fn host_power_capabilities() -> Value {
    json!({
        "low_power_running": cfg!(any(target_os = "linux", target_os = "macos")),
        "profiles": host_power_supported_profiles(),
        "suspend": cfg!(any(target_os = "linux", target_os = "macos")),
        "shutdown": cfg!(any(target_os = "linux", target_os = "macos")),
        "dry_run": true,
        "events": true
    })
}

fn host_power_supported_profiles() -> Vec<&'static str> {
    if cfg!(target_os = "linux") {
        vec!["power_saver", "balanced", "performance", "restore_previous"]
    } else if cfg!(target_os = "macos") {
        vec!["power_saver", "balanced", "restore_previous"]
    } else {
        vec![]
    }
}

async fn query_host_power_state() -> Value {
    match query_current_host_power_profile().await {
        Ok(profile) => json!({"ok": true, "profile": profile}),
        Err(error) => json!({
            "ok": false,
            "error": error
        }),
    }
}

async fn query_current_host_power_profile() -> Result<String, Value> {
    if cfg!(target_os = "linux") {
        let command = linux_get_profile_command();
        let output = run_command_output(&command).await?;
        parse_linux_active_profile(&output).ok_or_else(|| {
            json!({
                "code": "host_power_profile_parse_failed",
                "message": "power-profiles-daemon ActiveProfile output was not recognized",
                "command": command,
                "output": output
            })
        })
    } else if cfg!(target_os = "macos") {
        let command = CommandSpec::new("pmset", ["-g"]);
        let output = run_command_output(&command).await?;
        parse_macos_low_power_mode(&output)
            .map(|enabled| if enabled { "power_saver" } else { "balanced" }.to_string())
            .ok_or_else(|| {
                json!({
                    "code": "host_power_profile_parse_failed",
                    "message": "pmset output did not include lowpowermode",
                    "command": command,
                    "output": output
                })
            })
    } else {
        Err(json!({
            "code": "host_power_backend_unsupported",
            "message": format!("host power control is not supported on {}", env::consts::OS)
        }))
    }
}

fn normalize_host_power_profile(profile: &str) -> Result<String, HttpError> {
    let normalized = profile.replace('-', "_").to_ascii_lowercase();
    if host_power_supported_profiles()
        .iter()
        .any(|supported| supported == &normalized.as_str())
    {
        Ok(normalized)
    } else {
        Err(HttpError::non_retryable(
            "host_power_profile_unsupported",
            format!("unsupported host power profile: {profile}"),
        ))
    }
}

fn should_save_previous_profile(
    dry_run: bool,
    requested_profile: &str,
    target_profile: &str,
    current_profile: &str,
    existing_previous_profile: Option<&str>,
) -> bool {
    !dry_run
        && requested_profile != "restore_previous"
        && target_profile == "power_saver"
        && current_profile != "power_saver"
        && existing_previous_profile.is_none()
}

fn next_previous_profile(
    dry_run: bool,
    requested_profile: &str,
    target_profile: &str,
    current_profile: &str,
    existing_previous_profile: Option<&str>,
) -> Option<String> {
    if should_save_previous_profile(
        dry_run,
        requested_profile,
        target_profile,
        current_profile,
        existing_previous_profile,
    ) {
        Some(current_profile.to_string())
    } else if !dry_run && requested_profile == "restore_previous" {
        None
    } else {
        existing_previous_profile.map(str::to_string)
    }
}

fn build_profile_command(profile: &str) -> Result<CommandSpec, HttpError> {
    if cfg!(target_os = "linux") {
        let active_profile = match profile {
            "power_saver" => "power-saver",
            "balanced" => "balanced",
            "performance" => "performance",
            value => {
                return Err(HttpError::non_retryable(
                    "host_power_profile_unsupported",
                    format!("unsupported Linux host power profile: {value}"),
                ))
            }
        };
        Ok(linux_set_profile_command(active_profile))
    } else if cfg!(target_os = "macos") {
        match profile {
            "power_saver" => Ok(CommandSpec::new("pmset", ["-a", "lowpowermode", "1"])),
            "balanced" => Ok(CommandSpec::new("pmset", ["-a", "lowpowermode", "0"])),
            value => Err(HttpError::non_retryable(
                "host_power_profile_unsupported",
                format!("unsupported macOS host power profile: {value}"),
            )),
        }
    } else {
        Err(HttpError::non_retryable(
            "host_power_backend_unsupported",
            format!("host power control is not supported on {}", env::consts::OS),
        ))
    }
}

fn build_suspend_command() -> Result<CommandSpec, HttpError> {
    if cfg!(target_os = "linux") {
        Ok(CommandSpec::new(
            "busctl",
            [
                "--system",
                "call",
                "org.freedesktop.login1",
                "/org/freedesktop/login1",
                "org.freedesktop.login1.Manager",
                "Suspend",
                "b",
                "false",
            ],
        ))
    } else if cfg!(target_os = "macos") {
        Ok(CommandSpec::new("pmset", ["sleepnow"]))
    } else {
        Err(HttpError::non_retryable(
            "host_power_backend_unsupported",
            format!("host suspend is not supported on {}", env::consts::OS),
        ))
    }
}

fn build_shutdown_command(delay_sec: u64, force: bool) -> Result<CommandSpec, HttpError> {
    if cfg!(target_os = "linux") {
        if force && delay_sec > 0 {
            return Err(HttpError::non_retryable(
                "host_power_shutdown_unsupported",
                "Linux forced shutdown only supports delay_sec=0",
            ));
        }
        let mut args = vec![
            "poweroff".to_string(),
            "--no-block".to_string(),
            "--message=Mains Aegis UPS battery low".to_string(),
        ];
        if force {
            args.push("--force".to_string());
        }
        if delay_sec > 0 {
            args.push(format!("--when=+{delay_sec}s"));
        }
        Ok(CommandSpec::new("systemctl", args))
    } else if cfg!(target_os = "macos") {
        if force {
            return Err(HttpError::non_retryable(
                "host_power_shutdown_unsupported",
                "macOS forced shutdown is not supported by the pmset/shutdown backend",
            ));
        }
        let when = if delay_sec == 0 {
            "now".to_string()
        } else {
            format!("+{}", delay_sec.div_ceil(60))
        };
        Ok(CommandSpec::new(
            "shutdown",
            [
                "-h".to_string(),
                when,
                "Mains Aegis UPS battery low".to_string(),
            ],
        ))
    } else {
        Err(HttpError::non_retryable(
            "host_power_backend_unsupported",
            format!("host shutdown is not supported on {}", env::consts::OS),
        ))
    }
}

fn linux_get_profile_command() -> CommandSpec {
    CommandSpec::new(
        "busctl",
        [
            "--system",
            "get-property",
            "net.hadess.PowerProfiles",
            "/net/hadess/PowerProfiles",
            "net.hadess.PowerProfiles",
            "ActiveProfile",
        ],
    )
}

fn linux_set_profile_command(profile: &str) -> CommandSpec {
    CommandSpec::new(
        "busctl",
        [
            "--system",
            "set-property",
            "net.hadess.PowerProfiles",
            "/net/hadess/PowerProfiles",
            "net.hadess.PowerProfiles",
            "ActiveProfile",
            "s",
            profile,
        ],
    )
}

fn parse_linux_active_profile(output: &str) -> Option<String> {
    let profile = output
        .split('"')
        .nth(1)
        .or_else(|| output.split_whitespace().last())?;
    match profile {
        "power-saver" => Some("power_saver".to_string()),
        "balanced" => Some("balanced".to_string()),
        "performance" => Some("performance".to_string()),
        _ => None,
    }
}

fn parse_macos_low_power_mode(output: &str) -> Option<bool> {
    output.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        (parts.next()? == "lowpowermode")
            .then(|| parts.next())
            .flatten()
            .and_then(|value| match value {
                "0" => Some(false),
                "1" => Some(true),
                _ => None,
            })
    })
}

async fn run_command_output(command: &CommandSpec) -> Result<String, Value> {
    let output = Command::new(&command.program)
        .args(&command.args)
        .output()
        .await
        .map_err(|error| {
            json!({
                "code": "host_power_command_launch_failed",
                "message": error.to_string(),
                "command": command
            })
        })?;
    if !output.status.success() {
        return Err(json!({
            "code": "host_power_command_failed",
            "message": format!("host power command exited with {}", output.status),
            "command": command,
            "stderr": String::from_utf8_lossy(&output.stderr).trim()
        }));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

async fn run_command_status(command: &CommandSpec, code: &str) -> Result<(), HttpError> {
    let status = Command::new(&command.program)
        .args(&command.args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map_err(|error| HttpError::retryable(code, error.to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(HttpError::retryable(
            code,
            format!("host power command exited with {status}"),
        ))
    }
}

fn ensure_host_power_action_allowed(
    state: &AppState,
    dry_run: bool,
    action: &str,
) -> Result<(), HttpError> {
    if dry_run || state.allow_host_power_actions {
        Ok(())
    } else {
        record_host_power_action(
            state,
            "host power request denied",
            json!({
                "ok": false,
                "dry_run": false,
                "backend": host_power_backend_name(),
                "action": action,
                "error": {
                    "code": "host_power_real_action_denied",
                    "message": format!(
                        "real host {action} requires --allow-host-power-actions or MAINS_AEGIS_DEVD_ALLOW_HOST_POWER_ACTIONS=true"
                    ),
                    "retryable": false,
                    "details": null
                }
            }),
        );
        Err(HttpError::non_retryable(
            "host_power_real_action_denied",
            format!(
                "real host {action} requires --allow-host-power-actions or MAINS_AEGIS_DEVD_ALLOW_HOST_POWER_ACTIONS=true"
            ),
        ))
    }
}

fn record_host_power_action(state: &AppState, message: &str, payload: Value) {
    {
        let mut guard = state.inner.lock().expect("state lock");
        guard.host_power.last_action = Some(payload.clone());
    }
    emit(state, None, "host_power", message, payload);
}

fn emit(state: &AppState, device_id: Option<String>, kind: &str, message: &str, payload: Value) {
    let event = DevdEvent {
        id: next_id(),
        timestamp: now(),
        device_id,
        kind: kind.to_string(),
        message: message.to_string(),
        payload,
    };
    {
        let mut guard = state.inner.lock().expect("state lock");
        push_bounded(&mut guard.events, event.clone(), EVENT_LIMIT);
    }
    let _ = state.events.send(event);
}

fn push_log(device: &mut DeviceRecord, level: &str, target: &str, message: &str) {
    push_bounded(
        &mut device.logs,
        SerialLogEntry {
            id: next_id(),
            timestamp: now(),
            level: level.to_string(),
            target: target.to_string(),
            message: message.to_string(),
        },
        LOG_LIMIT,
    );
}

fn push_bounded<T>(queue: &mut VecDeque<T>, item: T, limit: usize) {
    queue.push_back(item);
    while queue.len() > limit {
        queue.pop_front();
    }
}

fn tail<T: Clone>(queue: &VecDeque<T>, limit: usize) -> Vec<T> {
    let skip = queue.len().saturating_sub(limit);
    queue.iter().skip(skip).cloned().collect()
}

fn grouped_trace_by_transport(trace: &VecDeque<SerialTraceEntry>, limit: usize) -> Value {
    let mut usb = VecDeque::new();
    let mut lan = VecDeque::new();
    for entry in trace {
        if trace_entry_transport(entry) == "lan" {
            push_bounded(&mut lan, entry.clone(), limit);
        } else {
            push_bounded(&mut usb, entry.clone(), limit);
        }
    }
    json!({
        "usb": tail(&usb, limit),
        "lan": tail(&lan, limit),
    })
}

fn trace_entry_transport(entry: &SerialTraceEntry) -> &'static str {
    if entry.kind == "http"
        || entry
            .target
            .as_deref()
            .is_some_and(|target| target.starts_with("http://") || target.starts_with("https://"))
    {
        "lan"
    } else {
        "usb"
    }
}

fn default_settings() -> DeviceSettingsState {
    DeviceSettingsState {
        wifi_configured: None,
        wifi_ssid: None,
        log_level: "info".to_string(),
        manual_charge: ManualChargePrefs {
            target: "full_100".to_string(),
            speed: "ma_500".to_string(),
            timer_h: 2,
        },
    }
}

fn next_id() -> String {
    format!(
        "devd-{}",
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    )
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

#[derive(Debug)]
struct HttpError(ApiError, StatusCode);

impl std::fmt::Display for HttpError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.0.code, self.0.message)
    }
}

impl std::error::Error for HttpError {}

impl HttpError {
    fn retryable(code: &str, message: impl Into<String>) -> Self {
        Self(
            ApiError {
                code: code.to_string(),
                message: message.into(),
                retryable: true,
                details: None,
            },
            StatusCode::BAD_GATEWAY,
        )
    }
    fn retryable_with_details(code: &str, message: impl Into<String>, details: Value) -> Self {
        Self(
            ApiError {
                code: code.to_string(),
                message: message.into(),
                retryable: true,
                details: Some(details),
            },
            StatusCode::BAD_GATEWAY,
        )
    }
    fn non_retryable(code: &str, message: impl Into<String>) -> Self {
        Self(
            ApiError {
                code: code.to_string(),
                message: message.into(),
                retryable: false,
                details: None,
            },
            StatusCode::BAD_REQUEST,
        )
    }
    fn not_found(code: &str, message: impl Into<String>) -> Self {
        Self(
            ApiError {
                code: code.to_string(),
                message: message.into(),
                retryable: false,
                details: None,
            },
            StatusCode::NOT_FOUND,
        )
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        (self.1, Json(json!({"error": self.0}))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_event_token_query_decodes_urlsafe_tokens() {
        let uri: Uri = "/api/v1/serial/events?lease_id=abc&bridge_token=a%2Bb%3D%3D"
            .parse()
            .unwrap();
        assert_eq!(query_param(&uri, "bridge_token").as_deref(), Some("a+b=="));
    }

    #[test]
    fn service_event_token_query_is_path_limited_by_caller() {
        let uri: Uri = "/api/v1/serial/events?bridge_token=secret".parse().unwrap();
        assert_eq!(query_param(&uri, "bridge_token").as_deref(), Some("secret"));
        assert_eq!(query_param(&uri, "missing"), None);
    }

    #[test]
    fn service_query_token_accepts_known_event_stream_routes() {
        let mut headers = HeaderMap::new();
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/json, text/event-stream"),
        );
        let status_uri: Uri = "/api/v1/status?bridge_token=secret".parse().unwrap();
        let serial_uri: Uri = "/api/v1/serial/events?bridge_token=secret".parse().unwrap();
        let device_uri: Uri = "/api/v1/devices/mock-devkit/events?bridge_token=secret"
            .parse()
            .unwrap();

        assert!(is_event_stream_query_auth_request(
            &Method::GET,
            &status_uri,
            &headers
        ));
        assert!(is_event_stream_query_auth_request(
            &Method::GET,
            &serial_uri,
            &headers
        ));
        assert!(is_event_stream_query_auth_request(
            &Method::GET,
            &device_uri,
            &headers
        ));
    }

    #[test]
    fn service_query_token_accepts_event_stream_media_params() {
        let mut headers = HeaderMap::new();
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("TEXT/EVENT-STREAM; charset=utf-8"),
        );

        assert!(accepts_event_stream(&headers));
    }

    #[test]
    fn service_query_token_rejects_mutation_api_even_with_event_stream_accept() {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
        let uri: Uri = "/api/v1/host/power/shutdown?bridge_token=secret"
            .parse()
            .unwrap();

        assert!(!is_event_stream_query_auth_request(
            &Method::POST,
            &uri,
            &headers
        ));
    }

    #[test]
    fn http_service_auth_allows_static_web_assets_without_opening_api() {
        let root: Uri = "/".parse().unwrap();
        let asset: Uri = "/assets/app.js".parse().unwrap();
        let api: Uri = "/api/v1/status".parse().unwrap();
        let health: Uri = "/health".parse().unwrap();

        assert!(is_static_web_asset_request(&Method::GET, &root));
        assert!(is_static_web_asset_request(&Method::HEAD, &asset));
        assert!(!is_static_web_asset_request(&Method::GET, &api));
        assert!(!is_static_web_asset_request(&Method::GET, &health));
        assert!(!is_static_web_asset_request(&Method::POST, &asset));
    }

    #[test]
    fn http_service_auth_allows_bootstrap_probe_only() {
        let bootstrap_uri: Uri = "/api/v1/bootstrap".parse().unwrap();
        let status_uri: Uri = "/api/v1/status".parse().unwrap();

        assert!(is_unauthenticated_bootstrap_request(
            &Method::GET,
            &bootstrap_uri
        ));
        assert!(!is_unauthenticated_bootstrap_request(
            &Method::POST,
            &bootstrap_uri
        ));
        assert!(!is_unauthenticated_bootstrap_request(
            &Method::GET,
            &status_uri
        ));
    }

    #[test]
    fn http_service_dev_cors_allows_loopback_dev_origins() {
        assert!(is_local_dev_cors_origin(&HeaderValue::from_static(
            "http://127.0.0.1:49480"
        )));
        assert!(is_local_dev_cors_origin(&HeaderValue::from_static(
            "http://localhost:5173"
        )));
        assert!(is_local_dev_cors_origin(&HeaderValue::from_static(
            "http://[::1]:5173"
        )));
    }

    #[test]
    fn http_service_dev_cors_rejects_non_loopback_origins() {
        assert!(!is_local_dev_cors_origin(&HeaderValue::from_static(
            "http://127.0.0.1.evil.test:49480"
        )));
        assert!(!is_local_dev_cors_origin(&HeaderValue::from_static(
            "https://localhost:49480"
        )));
        assert!(!is_local_dev_cors_origin(&HeaderValue::from_static(
            "http://192.168.31.10:49480"
        )));
    }

    #[tokio::test]
    async fn bootstrap_reports_auth_requirement() {
        let response = bootstrap(State(create_app_state_with_auth(
            false,
            true,
            HttpServiceMode::HostedApp,
            Some(Arc::<str>::from("secret")),
        )))
        .await;

        assert_eq!(response.0["token_required"], true);
        assert_eq!(response.0["app"]["mode"], "http_service");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn active_ipc_socket_is_not_unlinked() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("devd.sock");
        let _listener = tokio::net::UnixListener::bind(&path).unwrap();

        let error = remove_stale_ipc_socket(&path).await.unwrap_err();

        assert!(error.to_string().contains("already active"));
        assert!(path.exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stale_ipc_path_is_removed() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("devd.sock");
        fs::write(&path, b"stale").unwrap();

        remove_stale_ipc_socket(&path).await.unwrap();

        assert!(!path.exists());
    }

    #[test]
    fn stable_device_id_is_deterministic() {
        let port = serialport::SerialPortInfo {
            port_name: "/dev/cu.usbmodem1".into(),
            port_type: serialport::SerialPortType::Unknown,
        };
        assert_eq!(stable_device_id(&port), stable_device_id(&port));
    }

    #[test]
    fn stable_device_id_distinguishes_no_serial_usb_ports() {
        let mut left = usb_port("/dev/cu.usbmodem1", None);
        let right = usb_port("/dev/cu.usbmodem2", None);
        assert_ne!(stable_device_id(&left), stable_device_id(&right));

        left.port_name = "/dev/cu.usbmodem2".into();
        assert_eq!(stable_device_id(&left), stable_device_id(&right));
    }

    #[test]
    fn stable_device_id_uses_usb_serial_over_port_path() {
        let left = usb_port("/dev/cu.usbmodem1", Some("board-a"));
        let right = usb_port("/dev/cu.usbmodem2", Some("board-a"));
        assert_eq!(stable_device_id(&left), stable_device_id(&right));
    }

    #[test]
    fn native_device_stable_id_prefers_binding_stable_id() {
        let device = DeviceRecord {
            id: "mains-aegis-abc123".into(),
            display_name: "USB CDC".into(),
            port_path: Some("/dev/cu.usbmodem1".into()),
            lan_address: None,
            lan_conflict_addresses: Vec::new(),
            transport: DeviceTransport::NativeSerial,
            binding: Some(DeviceBinding {
                alias: None,
                stable_id: "serial-a".into(),
                port_path: Some("/dev/cu.usbmodem1".into()),
                created_at: "now".into(),
                logical_device_id: Some("mains-aegis-abc123".into()),
            }),
            connection: ConnectionState::Disconnected,
            identity: None,
            status: None,
            power_diag: None,
            selected_artifact_id: None,
            log_decode: LogDecodeState::default(),
            settings: default_settings(),
            logs: VecDeque::new(),
            trace: VecDeque::new(),
        };

        assert_eq!(native_device_stable_id(&device), Some("serial-a"));
    }

    #[tokio::test]
    async fn device_binding_persists_across_daemon_state_recreation() {
        let temp = tempfile::tempdir().unwrap();
        let persistence = DevdPersistence::enabled(temp.path().join("state.json"));
        let state = create_app_state_with_auth_and_persistence(
            false,
            false,
            HttpServiceMode::ApiOnly,
            None,
            persistence.clone(),
        );
        {
            let mut guard = state.inner.lock().expect("state lock");
            guard.devices.insert(
                "native-a".into(),
                DeviceRecord {
                    id: "native-a".into(),
                    display_name: "USB CDC".into(),
                    port_path: Some("/dev/cu.usbmodem1".into()),
                    lan_address: None,
                    lan_conflict_addresses: Vec::new(),
                    transport: DeviceTransport::NativeSerial,
                    binding: None,
                    connection: ConnectionState::Disconnected,
                    identity: None,
                    status: None,
                    power_diag: None,
                    selected_artifact_id: None,
                    log_decode: LogDecodeState::default(),
                    settings: default_settings(),
                    logs: VecDeque::new(),
                    trace: VecDeque::new(),
                },
            );
        }

        let _ = bind_device(
            State(state),
            Path("native-a".into()),
            Json(BindRequest {
                alias: Some("Bench unit".into()),
                logical_device_id: Some("mains-aegis-198840".into()),
            }),
        )
        .await
        .unwrap();

        let restarted = create_app_state_with_auth_and_persistence(
            false,
            false,
            HttpServiceMode::ApiOnly,
            None,
            persistence.clone(),
        );
        let guard = restarted.inner.lock().expect("state lock");
        let binding = guard.bindings.get("native-a").unwrap();
        assert_eq!(binding.alias.as_deref(), Some("Bench unit"));
        assert_eq!(binding.port_path.as_deref(), Some("/dev/cu.usbmodem1"));
        assert_eq!(
            binding.logical_device_id.as_deref(),
            Some("mains-aegis-198840")
        );
    }

    #[tokio::test]
    async fn persisted_state_does_not_restore_runtime_connection() {
        let temp = tempfile::tempdir().unwrap();
        let persistence = DevdPersistence::enabled(temp.path().join("state.json"));
        let state = create_app_state_with_auth_and_persistence(
            false,
            false,
            HttpServiceMode::ApiOnly,
            None,
            persistence.clone(),
        );
        {
            let mut guard = state.inner.lock().expect("state lock");
            let binding = DeviceBinding {
                alias: Some("Mock bench".into()),
                stable_id: "mock-devkit".into(),
                port_path: None,
                created_at: "now".into(),
                logical_device_id: Some("mains-aegis-198840".into()),
            };
            let device = guard.devices.get_mut("mock-devkit").unwrap();
            device.connection = ConnectionState::Connected;
            device.binding = Some(binding.clone());
            guard.bindings.insert("mock-devkit".into(), binding);
            persist_devd_state(&persistence, persisted_snapshot(&guard)).unwrap();
        }

        let restarted = create_app_state_with_auth_and_persistence(
            false,
            false,
            HttpServiceMode::ApiOnly,
            None,
            persistence.clone(),
        );
        let guard = restarted.inner.lock().expect("state lock");
        let device = guard.devices.get("mock-devkit").unwrap();
        assert!(matches!(device.connection, ConnectionState::Disconnected));
        assert_eq!(
            device
                .binding
                .as_ref()
                .and_then(|binding| binding.alias.as_deref()),
            Some("Mock bench")
        );
    }

    #[test]
    fn persisted_state_allows_concurrent_writes() {
        let temp = tempfile::tempdir().unwrap();
        let persistence = DevdPersistence::enabled(temp.path().join("state.json"));

        std::thread::scope(|scope| {
            for index in 0..16 {
                let persistence = persistence.clone();
                scope.spawn(move || {
                    let mut bindings = HashMap::new();
                    bindings.insert(
                        format!("device-{index}"),
                        DeviceBinding {
                            alias: Some(format!("Device {index}")),
                            stable_id: format!("device-{index}"),
                            port_path: Some(format!("/dev/cu.usbmodem{index}")),
                            created_at: "now".into(),
                            logical_device_id: Some(format!("logical-{index}")),
                        },
                    );
                    persist_devd_state(
                        &persistence,
                        PersistedDevdState {
                            schema_version: 1,
                            bindings,
                            selected_artifacts: HashMap::new(),
                            artifacts: HashMap::new(),
                            scan_trace: VecDeque::new(),
                            device_trace: HashMap::new(),
                        },
                    )
                    .unwrap();
                });
            }
        });

        let loaded = load_devd_state(&persistence).unwrap();
        assert_eq!(loaded.schema_version, 1);
        assert_eq!(loaded.bindings.len(), 1);
    }

    #[test]
    fn prefer_serial_port_path_uses_cu_on_macos() {
        assert_eq!(
            prefer_serial_port_path(Some("/dev/tty.usbmodem1"), "/dev/cu.usbmodem1"),
            "/dev/cu.usbmodem1"
        );
        assert_eq!(
            prefer_serial_port_path(Some("/dev/cu.usbmodem1"), "/dev/tty.usbmodem1"),
            "/dev/cu.usbmodem1"
        );
    }

    #[test]
    fn native_usb_serial_candidate_filters_virtual_ports() {
        assert!(is_native_usb_serial_candidate(&usb_port(
            "/dev/cu.usbmodem1",
            Some("board-a")
        )));
        assert!(is_native_usb_serial_candidate(
            &serialport::SerialPortInfo {
                port_name: "/dev/cu.usbmodem212101".into(),
                port_type: serialport::SerialPortType::Unknown,
            }
        ));
        assert!(!is_native_usb_serial_candidate(
            &serialport::SerialPortInfo {
                port_name: "/dev/cu.Bluetooth-Incoming-Port".into(),
                port_type: serialport::SerialPortType::Unknown,
            }
        ));
        assert!(!is_native_usb_serial_candidate(
            &serialport::SerialPortInfo {
                port_name: "/dev/tty.debug-console".into(),
                port_type: serialport::SerialPortType::Unknown,
            }
        ));
    }

    #[test]
    fn native_serial_app_reset_steps_keep_boot_released() {
        assert_eq!(
            native_serial_app_reset_steps(),
            &[
                NativeSerialLineStep::Rts(false),
                NativeSerialLineStep::Dtr(true),
                NativeSerialLineStep::SleepMs(100),
                NativeSerialLineStep::Rts(true),
                NativeSerialLineStep::Dtr(true),
                NativeSerialLineStep::Rts(true),
                NativeSerialLineStep::SleepMs(100),
                NativeSerialLineStep::Rts(false),
                NativeSerialLineStep::Dtr(true),
            ]
        );
    }

    #[test]
    fn native_serial_monitor_preserves_control_lines_on_open() {
        assert!(super::native_serial_monitor_preserves_control_lines_on_open());
    }

    #[test]
    fn native_serial_reset_backend_is_in_process_line_control() {
        assert_eq!(
            reset_backend_name(&DeviceTransport::NativeSerial),
            "native_serial_lines"
        );
    }

    #[test]
    fn ipv4_cidr_expands_hosts_without_network_or_broadcast() {
        let hosts = ipv4_hosts_from_cidr("192.168.4.0/30").unwrap();

        assert_eq!(
            hosts,
            vec![Ipv4Addr::new(192, 168, 4, 1), Ipv4Addr::new(192, 168, 4, 2)]
        );
    }

    #[test]
    fn lan_discovery_merges_into_usb_identity_record() {
        let mut state = DevdState::default();
        state.devices.insert(
            "serial-a".into(),
            DeviceRecord {
                id: "serial-a".into(),
                display_name: "USB CDC".into(),
                port_path: Some("/dev/cu.usbmodem1".into()),
                lan_address: None,
                lan_conflict_addresses: Vec::new(),
                transport: DeviceTransport::NativeSerial,
                binding: None,
                connection: ConnectionState::Connected,
                identity: Some(json!({"device_id": "mains-aegis-abc123"})),
                status: None,
                power_diag: None,
                selected_artifact_id: None,
                log_decode: LogDecodeState::default(),
                settings: default_settings(),
                logs: VecDeque::new(),
                trace: VecDeque::new(),
            },
        );
        let discovery = LanDeviceDiscovery {
            address: "192.168.4.25".into(),
            identity: json!({"device_id": "mains-aegis-abc123", "hostname": "mains-aegis-abc123"}),
            trace: vec![structured_trace_entry(
                "rx",
                "http",
                Some("http://192.168.4.25/api/v1/identity".into()),
                "identity response",
                "{}".into(),
            )],
        };
        let mut discovered_ids = HashSet::new();
        let mut lan_count = 0;

        merge_lan_discoveries(
            &mut state,
            vec![discovery],
            &mut discovered_ids,
            &mut lan_count,
        );

        assert_eq!(lan_count, 1);
        assert_eq!(
            discovered_ids.get("serial-a"),
            Some(&"serial-a".to_string())
        );
        let device = state.devices.get("serial-a").unwrap();
        assert!(matches!(device.transport, DeviceTransport::NativeSerial));
        assert_eq!(device.lan_address.as_deref(), Some("192.168.4.25"));
        assert_eq!(available_transports(device), vec!["usb", "lan"]);
    }

    #[test]
    fn lan_discovery_merges_into_bound_usb_logical_record_before_identity() {
        let mut state = DevdState::default();
        state.devices.insert(
            "serial-a".into(),
            DeviceRecord {
                id: "serial-a".into(),
                display_name: "USB CDC".into(),
                port_path: Some("/dev/cu.usbmodem1".into()),
                lan_address: None,
                lan_conflict_addresses: Vec::new(),
                transport: DeviceTransport::NativeSerial,
                binding: Some(DeviceBinding {
                    alias: Some("Bench unit".into()),
                    stable_id: "serial-a".into(),
                    port_path: Some("/dev/cu.usbmodem1".into()),
                    created_at: "now".into(),
                    logical_device_id: Some("mains-aegis-abc123".into()),
                }),
                connection: ConnectionState::Disconnected,
                identity: None,
                status: None,
                power_diag: None,
                selected_artifact_id: None,
                log_decode: LogDecodeState::default(),
                settings: default_settings(),
                logs: VecDeque::new(),
                trace: VecDeque::new(),
            },
        );
        let discovery = LanDeviceDiscovery {
            address: "192.168.4.25".into(),
            identity: json!({"device_id": "mains-aegis-abc123", "hostname": "mains-aegis-abc123"}),
            trace: vec![],
        };
        let mut discovered_ids = HashSet::new();
        let mut lan_count = 0;

        merge_lan_discoveries(
            &mut state,
            vec![discovery],
            &mut discovered_ids,
            &mut lan_count,
        );

        assert_eq!(lan_count, 1);
        assert_eq!(
            discovered_ids.get("serial-a"),
            Some(&"serial-a".to_string())
        );
        assert!(!state.devices.contains_key("mains-aegis-abc123"));
        let device = state.devices.get("serial-a").unwrap();
        assert_eq!(device.lan_address.as_deref(), Some("192.168.4.25"));
        assert_eq!(available_transports(device), vec!["usb", "lan"]);
    }

    #[test]
    fn connection_transports_reports_usb_lan_reachability() {
        let device = DeviceRecord {
            id: "serial-a".into(),
            display_name: "USB CDC".into(),
            port_path: Some("/dev/cu.usbmodem1".into()),
            lan_address: Some("192.168.4.25".into()),
            lan_conflict_addresses: Vec::new(),
            transport: DeviceTransport::NativeSerial,
            binding: None,
            connection: ConnectionState::Connected,
            identity: Some(json!({"device_id": "mains-aegis-abc123"})),
            status: None,
            power_diag: None,
            selected_artifact_id: None,
            log_decode: LogDecodeState::default(),
            settings: default_settings(),
            logs: VecDeque::new(),
            trace: VecDeque::new(),
        };

        let transports = connection_transports(&device);

        assert_eq!(transports["usb"]["available"], true);
        assert_eq!(transports["usb"]["active"], true);
        assert_eq!(transports["lan"]["available"], true);
        assert_eq!(transports["lan"]["connected"], true);
        assert_eq!(
            connection_switch_hint(&device),
            Some("LAN is available; switching away from USB should be explicit")
        );
    }

    #[test]
    fn persisted_snapshot_includes_bounded_runtime_trace() {
        let mut state = DevdState::default();
        state.scan_trace.push_back(structured_trace_entry(
            "rx",
            "scan",
            Some("lan-probe".into()),
            "scan done",
            "{}".into(),
        ));
        let mut device_trace = VecDeque::new();
        device_trace.push_back(structured_trace_entry(
            "rx",
            "http",
            Some("http://192.168.4.25/api/v1/identity".into()),
            "identity response",
            "{}".into(),
        ));
        state.devices.insert(
            "serial-a".into(),
            DeviceRecord {
                id: "serial-a".into(),
                display_name: "USB CDC".into(),
                port_path: Some("/dev/cu.usbmodem1".into()),
                lan_address: Some("192.168.4.25".into()),
                lan_conflict_addresses: Vec::new(),
                transport: DeviceTransport::NativeSerial,
                binding: None,
                connection: ConnectionState::Connected,
                identity: Some(json!({"device_id": "mains-aegis-abc123"})),
                status: None,
                power_diag: None,
                selected_artifact_id: None,
                log_decode: LogDecodeState::default(),
                settings: default_settings(),
                logs: VecDeque::new(),
                trace: device_trace,
            },
        );

        let snapshot = persisted_snapshot(&state);

        assert_eq!(snapshot.scan_trace.len(), 1);
        assert_eq!(snapshot.device_trace.get("serial-a").unwrap().len(), 1);
        assert_eq!(
            snapshot
                .device_trace
                .get("mains-aegis-abc123")
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn groups_trace_by_transport() {
        let mut trace = VecDeque::new();
        trace.push_back(trace_entry(
            "tx",
            r#"{"type":"request","request_id":"req-1","op":"get_status"}"#,
        ));
        trace.push_back(structured_trace_entry(
            "rx",
            "http",
            Some("http://192.168.4.25/api/v1/settings".into()),
            "settings snapshot",
            "{}".into(),
        ));

        let grouped = grouped_trace_by_transport(&trace, 10);

        assert_eq!(grouped["usb"].as_array().unwrap().len(), 1);
        assert_eq!(grouped["lan"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn parses_lan_http_json_response() {
        let response =
            b"HTTP/1.1 202 Accepted\r\nContent-Type: application/json\r\n\r\n{\"accepted\":true}";

        let parsed = parse_lan_http_json_response(
            response,
            "POST",
            "/api/v1/settings/log-level",
            "192.168.4.25",
        )
        .unwrap();

        assert_eq!(parsed["accepted"], true);
    }

    #[test]
    fn maps_settings_snapshot_from_lan_api() {
        let snapshot = json!({
            "wifi": {"configured": true, "ssid": "lab"},
            "log_level": "debug",
            "manual_charge": {"target": "rsoc_80", "speed": "ma_1000", "timer_h": 6}
        });

        let settings = settings_state_from_api(&snapshot).unwrap();

        assert_eq!(settings.wifi_configured, Some(true));
        assert_eq!(settings.wifi_ssid.as_deref(), Some("lab"));
        assert_eq!(settings.log_level, "debug");
        assert_eq!(settings.manual_charge.target, "rsoc_80");
        assert_eq!(settings.manual_charge.speed, "ma_1000");
        assert_eq!(settings.manual_charge.timer_h, 6);
    }

    #[test]
    fn redacts_wifi_psk_from_cdc_trace_frames() {
        let frame = json!({
            "type": "wifi_config",
            "op": "set",
            "ssid": "lab",
            "psk": "super-secret",
            "request_id": "req-1"
        });

        let redacted = redact_cdc_frame(&frame);

        assert_eq!(frame["psk"], "super-secret");
        assert_eq!(redacted["psk"], "[redacted]");
        assert_eq!(redacted["ssid"], "lab");
    }

    #[test]
    fn parses_json_frame_embedded_in_defmt_line() {
        let mut line = vec![0x62, 0x7f, 0x00, 0xff, 0x00];
        line.extend_from_slice(
            br#"{"type":"response","request_id":"devd-identity","ok":true,"result":{"device_id":"mains-aegis-test"}}"#,
        );
        line.extend_from_slice(&[0x07, 0xea, 0x01]);

        let identity = parse_identity_line(&line).unwrap().unwrap();
        let response = parse_matching_cdc_response(&line, "devd-identity")
            .unwrap()
            .unwrap();
        let monitor = parse_cdc_line_for_monitor(&line).unwrap().0;

        assert_eq!(identity["device_id"], "mains-aegis-test");
        assert_eq!(response["request_id"], "devd-identity");
        assert_eq!(monitor.kind, "frame");
        assert_eq!(monitor.frame_type.as_deref(), Some("response"));
    }

    #[test]
    fn native_monitor_ingest_keeps_defmt_ascii_bytes_in_stream() {
        let mut cdc_line = Vec::new();
        let mut json_candidate = Vec::new();
        let input = [0x31, 0x01, b'R', b'T', b'e', 0x0b, 0x86, 0x00];
        let mut output = Vec::new();

        for byte in input {
            if let NativeMonitorInput::DefmtBytes(bytes) =
                native_monitor_ingest_byte(byte, &mut cdc_line, &mut json_candidate)
            {
                output.extend(bytes);
            }
        }

        assert_eq!(output, input);
        assert!(cdc_line.is_empty());
        assert!(json_candidate.is_empty());
    }

    #[test]
    fn native_monitor_ingest_routes_json_lines_to_cdc_parser() {
        let mut cdc_line = Vec::new();
        let mut json_candidate = Vec::new();
        let input = br#"{"type":"response","request_id":"devd-monitor-status","ok":true}"#
            .iter()
            .copied()
            .chain(std::iter::once(b'\n'));
        let mut cdc = Vec::new();
        let mut defmt = Vec::new();

        for byte in input {
            match native_monitor_ingest_byte(byte, &mut cdc_line, &mut json_candidate) {
                NativeMonitorInput::CdcLine(line) => cdc.push(line),
                NativeMonitorInput::DefmtBytes(bytes) => defmt.extend(bytes),
                NativeMonitorInput::None => {}
            }
        }

        assert_eq!(cdc.len(), 1);
        assert_eq!(
            parse_matching_cdc_response(&cdc[0], "devd-monitor-status")
                .unwrap()
                .unwrap()["request_id"],
            "devd-monitor-status"
        );
        assert!(defmt.is_empty());
    }

    #[test]
    fn native_monitor_ingest_returns_false_json_prefix_to_defmt() {
        let mut cdc_line = Vec::new();
        let mut json_candidate = Vec::new();
        let mut output = Vec::new();

        for byte in [b'{', 0x7b, 0x00] {
            if let NativeMonitorInput::DefmtBytes(bytes) =
                native_monitor_ingest_byte(byte, &mut cdc_line, &mut json_candidate)
            {
                output.extend(bytes);
            }
        }

        assert_eq!(output, [b'{', 0x7b, 0x00]);
        assert!(cdc_line.is_empty());
        assert!(json_candidate.is_empty());
    }

    fn test_artifact_with_defmt(
        name: &str,
        defmt_enabled: bool,
        files: Vec<ArtifactFile>,
    ) -> FirmwareArtifact {
        FirmwareArtifact {
            artifact_id: format!("{name}-artifact"),
            name: name.into(),
            version: "0.1.0".into(),
            git_sha: "git".into(),
            build_id: "build".into(),
            target_chip: "esp32s3".into(),
            profile: "release".into(),
            features: vec!["web_serial".into()],
            protocol: "mains-aegis.cdc.v1".into(),
            defmt: DefmtMetadata {
                enabled: defmt_enabled,
                encoding: "defmt-espflash".into(),
                elf_sha256: None,
                metadata_sha256: None,
            },
            files,
        }
    }

    #[test]
    fn native_monitor_defmt_uses_any_defmt_enabled_artifact() {
        let artifact = test_artifact_with_defmt(
            "mains-aegis",
            true,
            vec![ArtifactFile {
                kind: "elf".into(),
                path: "/tmp/mains-aegis.elf".into(),
                sha256: "sha".into(),
                size: 123,
                flash_address: None,
            }],
        );

        let path = native_monitor_defmt_elf_path(&artifact).unwrap().unwrap();

        assert_eq!(path, PathBuf::from("/tmp/mains-aegis.elf"));
    }

    #[test]
    fn native_monitor_defmt_skips_artifacts_without_defmt() {
        let artifact = test_artifact_with_defmt("plain-tool", false, vec![]);

        assert!(native_monitor_defmt_elf_path(&artifact).unwrap().is_none());
    }

    #[test]
    fn native_monitor_defmt_enabled_artifact_requires_elf() {
        let artifact = test_artifact_with_defmt("mains-aegis", true, vec![]);

        let error = native_monitor_defmt_elf_path(&artifact).unwrap_err();

        assert_eq!(error.0.code, "defmt_elf_missing");
    }

    #[test]
    fn artifact_match_uses_exact_build_id() {
        let mut device = DeviceRecord {
            id: "d".into(),
            display_name: "d".into(),
            port_path: None,
            lan_address: None,
            lan_conflict_addresses: Vec::new(),
            transport: DeviceTransport::Mock,
            binding: None,
            connection: ConnectionState::Disconnected,
            identity: Some(
                json!({"firmware": {"build_id": "b1", "git_sha": "g1", "build_profile": "release", "features": ["web_serial"]}}),
            ),
            status: None,
            power_diag: None,
            selected_artifact_id: None,
            log_decode: LogDecodeState::default(),
            settings: default_settings(),
            logs: VecDeque::new(),
            trace: VecDeque::new(),
        };
        let artifact = FirmwareArtifact {
            artifact_id: "a".into(),
            name: "mains-aegis".into(),
            version: "0.1.0".into(),
            git_sha: "g2".into(),
            build_id: "b1".into(),
            target_chip: "esp32s3".into(),
            profile: "release".into(),
            features: vec!["web_serial".into()],
            protocol: "mains-aegis.cdc.v1".into(),
            defmt: DefmtMetadata {
                enabled: true,
                encoding: "defmt-espflash".into(),
                elf_sha256: None,
                metadata_sha256: None,
            },
            files: vec![],
        };
        apply_artifact_match(&mut device, Some(&artifact));
        assert_eq!(device.log_decode.status, "verified");
    }

    #[test]
    fn artifact_match_rejects_same_git_with_different_build_id() {
        let mut device = DeviceRecord {
            id: "d".into(),
            display_name: "d".into(),
            port_path: None,
            lan_address: None,
            lan_conflict_addresses: Vec::new(),
            transport: DeviceTransport::Mock,
            binding: None,
            connection: ConnectionState::Disconnected,
            identity: Some(
                json!({"firmware": {"build_id": "debug-build", "git_sha": "same", "build_profile": "release", "features": ["web_serial"]}}),
            ),
            status: None,
            power_diag: None,
            selected_artifact_id: None,
            log_decode: LogDecodeState::default(),
            settings: default_settings(),
            logs: VecDeque::new(),
            trace: VecDeque::new(),
        };
        let artifact = FirmwareArtifact {
            artifact_id: "a".into(),
            name: "mains-aegis".into(),
            version: "0.1.0".into(),
            git_sha: "same".into(),
            build_id: "release-build".into(),
            target_chip: "esp32s3".into(),
            profile: "release".into(),
            features: vec!["web_serial".into()],
            protocol: "mains-aegis.cdc.v1".into(),
            defmt: DefmtMetadata {
                enabled: true,
                encoding: "defmt-espflash".into(),
                elf_sha256: None,
                metadata_sha256: None,
            },
            files: vec![],
        };
        apply_artifact_match(&mut device, Some(&artifact));
        assert_eq!(device.log_decode.status, "unverified");
    }

    #[test]
    fn artifact_match_rejects_different_features() {
        let mut device = DeviceRecord {
            id: "d".into(),
            display_name: "d".into(),
            port_path: None,
            lan_address: None,
            lan_conflict_addresses: Vec::new(),
            transport: DeviceTransport::Mock,
            binding: None,
            connection: ConnectionState::Disconnected,
            identity: Some(
                json!({"firmware": {"build_id": "same-build", "build_profile": "release", "features": ["net_http"]}}),
            ),
            status: None,
            power_diag: None,
            selected_artifact_id: None,
            log_decode: LogDecodeState::default(),
            settings: default_settings(),
            logs: VecDeque::new(),
            trace: VecDeque::new(),
        };
        let artifact = FirmwareArtifact {
            artifact_id: "a".into(),
            name: "mains-aegis".into(),
            version: "0.1.0".into(),
            git_sha: "same".into(),
            build_id: "same-build".into(),
            target_chip: "esp32s3".into(),
            profile: "release".into(),
            features: vec!["web_serial".into()],
            protocol: "mains-aegis.cdc.v1".into(),
            defmt: DefmtMetadata {
                enabled: true,
                encoding: "defmt-espflash".into(),
                elf_sha256: None,
                metadata_sha256: None,
            },
            files: vec![],
        };
        apply_artifact_match(&mut device, Some(&artifact));
        assert_eq!(device.log_decode.status, "unverified");
    }

    #[test]
    fn artifact_match_rejects_different_profile() {
        let mut device = DeviceRecord {
            id: "d".into(),
            display_name: "d".into(),
            port_path: None,
            lan_address: None,
            lan_conflict_addresses: Vec::new(),
            transport: DeviceTransport::Mock,
            binding: None,
            connection: ConnectionState::Disconnected,
            identity: Some(
                json!({"firmware": {"build_id": "same-build", "build_profile": "debug", "features": ["web_serial"]}}),
            ),
            status: None,
            power_diag: None,
            selected_artifact_id: None,
            log_decode: LogDecodeState::default(),
            settings: default_settings(),
            logs: VecDeque::new(),
            trace: VecDeque::new(),
        };
        let artifact = FirmwareArtifact {
            artifact_id: "a".into(),
            name: "mains-aegis".into(),
            version: "0.1.0".into(),
            git_sha: "same".into(),
            build_id: "same-build".into(),
            target_chip: "esp32s3".into(),
            profile: "release".into(),
            features: vec!["web_serial".into()],
            protocol: "mains-aegis.cdc.v1".into(),
            defmt: DefmtMetadata {
                enabled: true,
                encoding: "defmt-espflash".into(),
                elf_sha256: None,
                metadata_sha256: None,
            },
            files: vec![],
        };
        apply_artifact_match(&mut device, Some(&artifact));
        assert_eq!(device.log_decode.status, "unverified");
    }

    #[test]
    fn real_flash_port_requires_binding() {
        let mut device = DeviceRecord {
            id: "d".into(),
            display_name: "d".into(),
            port_path: Some("/dev/cu.usbmodem1".into()),
            lan_address: None,
            lan_conflict_addresses: Vec::new(),
            transport: DeviceTransport::NativeSerial,
            binding: None,
            connection: ConnectionState::Disconnected,
            identity: None,
            status: None,
            power_diag: None,
            selected_artifact_id: None,
            log_decode: LogDecodeState::default(),
            settings: default_settings(),
            logs: VecDeque::new(),
            trace: VecDeque::new(),
        };
        assert_eq!(bound_flash_port(&device), None);
        device.binding = Some(DeviceBinding {
            alias: None,
            stable_id: "d".into(),
            port_path: Some("/dev/cu.usbmodem1".into()),
            created_at: "now".into(),
            logical_device_id: None,
        });
        assert_eq!(bound_flash_port(&device), Some("/dev/cu.usbmodem1".into()));
    }

    #[test]
    fn parses_linux_power_profiles_active_profile() {
        assert_eq!(
            parse_linux_active_profile("s \"power-saver\"\n").as_deref(),
            Some("power_saver")
        );
        assert_eq!(
            parse_linux_active_profile("s \"balanced\"\n").as_deref(),
            Some("balanced")
        );
        assert_eq!(parse_linux_active_profile("s \"unknown\"\n"), None);
    }

    #[test]
    fn builds_linux_power_profiles_dbus_command() {
        assert_eq!(
            linux_set_profile_command("power-saver"),
            CommandSpec::new(
                "busctl",
                [
                    "--system",
                    "set-property",
                    "net.hadess.PowerProfiles",
                    "/net/hadess/PowerProfiles",
                    "net.hadess.PowerProfiles",
                    "ActiveProfile",
                    "s",
                    "power-saver",
                ],
            )
        );
    }

    #[test]
    fn parses_macos_low_power_mode_from_pmset_output() {
        assert_eq!(
            parse_macos_low_power_mode("System-wide power settings:\n lowpowermode 1\n"),
            Some(true)
        );
        assert_eq!(
            parse_macos_low_power_mode("System-wide power settings:\n lowpowermode 0\n"),
            Some(false)
        );
        assert_eq!(
            parse_macos_low_power_mode("System-wide power settings:\n"),
            None
        );
    }

    #[test]
    fn host_power_real_action_is_denied_by_default() {
        let (events, mut receiver) = broadcast::channel(1);
        let state = test_app_state(events);
        assert!(ensure_host_power_action_allowed(&state, true, "shutdown").is_ok());
        assert!(ensure_host_power_action_allowed(&state, false, "shutdown").is_err());
        let event = receiver.try_recv().unwrap();
        assert_eq!(event.kind, "host_power");
        assert_eq!(event.payload["ok"], false);
        assert_eq!(event.payload["action"], "shutdown");
        assert_eq!(
            event.payload["error"]["code"],
            "host_power_real_action_denied"
        );
    }

    #[test]
    fn linux_shutdown_command_uses_systemctl_poweroff_when_compiled_for_linux() {
        if !cfg!(target_os = "linux") {
            return;
        }
        let command = build_shutdown_command(30, false).unwrap();

        assert_eq!(command.program, "systemctl");
        assert_eq!(command.args[0], "poweroff");
        assert!(command.args.contains(&"--no-block".to_string()));
        assert!(!command.args.contains(&"--force".to_string()));
        assert!(command.args.contains(&"--when=+30s".to_string()));
    }

    #[test]
    fn linux_shutdown_command_uses_force_only_when_requested() {
        if !cfg!(target_os = "linux") {
            return;
        }
        let command = build_shutdown_command(0, true).unwrap();

        assert_eq!(command.program, "systemctl");
        assert_eq!(command.args[0], "poweroff");
        assert!(command.args.contains(&"--no-block".to_string()));
        assert!(command.args.contains(&"--force".to_string()));
        assert!(!command.args.iter().any(|arg| arg.starts_with("--when=")));
    }

    #[test]
    fn linux_forced_delayed_shutdown_is_rejected_when_compiled_for_linux() {
        if !cfg!(target_os = "linux") {
            return;
        }
        let error = build_shutdown_command(30, true).unwrap_err();

        assert_eq!(error.0.code, "host_power_shutdown_unsupported");
    }

    #[test]
    fn macos_shutdown_command_uses_os_scheduler_when_compiled_for_macos() {
        if !cfg!(target_os = "macos") {
            return;
        }
        let command = build_shutdown_command(30, false).unwrap();

        assert_eq!(command.program, "shutdown");
        assert_eq!(command.args[0], "-h");
        assert_eq!(command.args[1], "+1");
    }

    #[test]
    fn macos_forced_shutdown_is_rejected_when_compiled_for_macos() {
        if !cfg!(target_os = "macos") {
            return;
        }
        let error = build_shutdown_command(0, true).unwrap_err();

        assert_eq!(error.0.code, "host_power_shutdown_unsupported");
    }

    #[test]
    fn previous_profile_is_saved_only_for_first_real_saver_entry() {
        assert!(should_save_previous_profile(
            false,
            "power_saver",
            "power_saver",
            "balanced",
            None
        ));
        assert!(!should_save_previous_profile(
            false,
            "power_saver",
            "power_saver",
            "power_saver",
            Some("balanced")
        ));
        assert!(!should_save_previous_profile(
            false,
            "power_saver",
            "power_saver",
            "balanced",
            Some("balanced")
        ));
        assert!(!should_save_previous_profile(
            true,
            "power_saver",
            "power_saver",
            "balanced",
            None
        ));
    }

    #[test]
    fn previous_profile_is_cleared_after_real_restore() {
        assert_eq!(
            next_previous_profile(
                false,
                "restore_previous",
                "balanced",
                "power_saver",
                Some("balanced")
            ),
            None
        );
        assert_eq!(
            next_previous_profile(
                true,
                "restore_previous",
                "balanced",
                "power_saver",
                Some("balanced")
            )
            .as_deref(),
            Some("balanced")
        );
        assert_eq!(
            next_previous_profile(false, "power_saver", "power_saver", "performance", None)
                .as_deref(),
            Some("performance")
        );
    }

    fn test_app_state(events: broadcast::Sender<DevdEvent>) -> AppState {
        AppState {
            inner: Arc::new(Mutex::new(DevdState::default())),
            events,
            allow_host_power_actions: false,
            auth_token_required: false,
            http_service_mode: HttpServiceMode::ApiOnly,
            app_session_secret: None,
            persistence: DevdPersistence::disabled(),
        }
    }

    #[tokio::test]
    async fn real_profile_action_is_denied_before_backend_query() {
        let (events, mut receiver) = broadcast::channel(4);
        let state = test_app_state(events);

        let error = host_power_profile(
            State(state),
            Json(HostPowerProfileRequest {
                profile: "power_saver".to_string(),
                dry_run: Some(false),
            }),
        )
        .await
        .unwrap_err();

        assert_eq!(error.0.code, "host_power_real_action_denied");
        let event = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(event.kind, "host_power");
        assert_eq!(event.payload["action"], "profile");
        assert_eq!(
            event.payload["error"]["code"],
            "host_power_real_action_denied"
        );
    }

    #[tokio::test]
    async fn unsupported_profile_is_rejected_before_backend_query() {
        let (events, _) = broadcast::channel(4);
        let state = test_app_state(events);

        let error = host_power_profile(
            State(state),
            Json(HostPowerProfileRequest {
                profile: "turbo".to_string(),
                dry_run: Some(true),
            }),
        )
        .await
        .unwrap_err();

        assert_eq!(error.0.code, "host_power_profile_unsupported");
    }

    #[tokio::test]
    async fn suspend_accepts_missing_body_as_default_dry_run() {
        let (events, mut receiver) = broadcast::channel(4);
        let state = test_app_state(events);

        let response = host_power_suspend(State(state), None).await.unwrap();

        assert_eq!(response.0["dry_run"], true);
        assert_eq!(response.0["action"], "suspend");
        let event = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(event.kind, "host_power");
        assert_eq!(event.payload["action"], "suspend");
    }

    #[tokio::test]
    async fn shutdown_accepts_missing_body_as_default_dry_run() {
        let (events, mut receiver) = broadcast::channel(4);
        let state = test_app_state(events);

        let response = host_power_shutdown(State(state), None).await.unwrap();

        assert_eq!(response.0["dry_run"], true);
        assert_eq!(response.0["action"], "shutdown");
        assert_eq!(response.0["delay_sec"], 60);
        assert_eq!(response.0["force"], false);
        let event = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(event.kind, "host_power");
        assert_eq!(event.payload["action"], "shutdown");
    }

    #[tokio::test]
    async fn host_power_action_record_emits_event() {
        let (events, mut receiver) = broadcast::channel(4);
        let state = test_app_state(events);
        let payload = json!({
            "ok": true,
            "dry_run": true,
            "action": "profile",
            "target_profile": "power_saver",
            "command": linux_set_profile_command("power-saver")
        });
        record_host_power_action(&state, "host power profile requested", payload);

        let event = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(event.kind, "host_power");
        assert_eq!(event.payload["action"], "profile");
        assert_eq!(event.payload["dry_run"], true);
    }

    fn usb_port(port_name: &str, serial_number: Option<&str>) -> serialport::SerialPortInfo {
        serialport::SerialPortInfo {
            port_name: port_name.into(),
            port_type: serialport::SerialPortType::UsbPort(serialport::UsbPortInfo {
                vid: 0x303a,
                pid: 0x1001,
                serial_number: serial_number.map(str::to_string),
                manufacturer: Some("Espressif".into()),
                product: Some("USB JTAG/serial debug unit".into()),
            }),
        }
    }
}
