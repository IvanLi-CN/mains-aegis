use axum::{
    extract::{Path, Query, State},
    http::{HeaderValue, Method, StatusCode},
    response::{sse::Event, sse::Sse, IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use chrono::Utc;
use defmt_decoder::{
    log::format::{Formatter, FormatterConfig, FormatterFormat},
    Table,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    env, fs,
    io::{Read, Write},
    net::SocketAddr,
    path::{Path as FsPath, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};
use tokio::{process::Command, sync::broadcast};
use tower_http::{cors::CorsLayer, services::ServeDir};

const DEFAULT_BIND: &str = "127.0.0.1:30080";
const EVENT_LIMIT: usize = 1_000;
const LOG_LIMIT: usize = 2_000;

#[derive(Debug, Clone)]
struct Config {
    bind: SocketAddr,
    web_root: Option<PathBuf>,
    allow_dev_cors: bool,
}

#[derive(Clone)]
struct AppState {
    inner: Arc<Mutex<DevdState>>,
    events: broadcast::Sender<DevdEvent>,
}

#[derive(Debug, Default)]
struct DevdState {
    devices: HashMap<String, DeviceRecord>,
    bindings: HashMap<String, DeviceBinding>,
    artifacts: HashMap<String, FirmwareArtifact>,
    events: VecDeque<DevdEvent>,
    monitors: HashMap<String, NativeMonitorHandle>,
}

#[derive(Debug)]
struct NativeMonitorHandle {
    stop: Arc<AtomicBool>,
    command_tx: mpsc::Sender<NativeMonitorCommand>,
}

#[derive(Debug)]
enum NativeMonitorCommand {
    SendFrame {
        frame: Value,
        request_id: String,
        response_tx: mpsc::Sender<Result<Value, HttpError>>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DeviceRecord {
    id: String,
    display_name: String,
    port_path: Option<String>,
    transport: DeviceTransport,
    binding: Option<DeviceBinding>,
    connection: ConnectionState,
    identity: Option<Value>,
    status: Option<Value>,
    selected_artifact_id: Option<String>,
    log_decode: LogDecodeState,
    safe_settings: SafeSettingsState,
    logs: VecDeque<SerialLogEntry>,
    trace: VecDeque<SerialTraceEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DeviceTransport {
    NativeSerial,
    Mock,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DeviceBinding {
    alias: Option<String>,
    stable_id: String,
    port_path: Option<String>,
    created_at: String,
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
struct SafeSettingsState {
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
}

#[derive(Debug, Deserialize)]
struct ArtifactSelectRequest {
    manifest_path: Option<String>,
    artifact_id: Option<String>,
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
}

#[derive(Debug, Deserialize)]
struct WifiConfigRequest {
    ssid: String,
    psk: String,
    device_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LogLevelRequest {
    level: String,
    device_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ManualChargeRequest {
    target: String,
    speed: String,
    timer_h: u8,
    device_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SafeSettingsTargetQuery {
    device_id: Option<String>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let config = match parse_args() {
        Ok(config) => config,
        Err(message) => {
            eprintln!("{message}");
            eprintln!("usage: mains-aegis-devd serve [--bind 127.0.0.1:30080] [--web-root <dir>] [--allow-dev-cors]");
            std::process::exit(2);
        }
    };
    let (events, _) = broadcast::channel(256);
    let state = AppState {
        inner: Arc::new(Mutex::new(DevdState::default())),
        events,
    };
    seed_mock_device(&state);

    let mut app = Router::new()
        .route("/health", get(health))
        .route("/api/v1/ping", get(health))
        .route("/api/v1/identity", get(devd_compat_identity))
        .route("/api/v1/network", get(devd_compat_network))
        .route("/api/v1/status", get(devd_compat_status))
        .route(
            "/api/v1/wifi-config",
            post(set_wifi_config).delete(clear_wifi_config),
        )
        .route("/api/v1/settings/log-level", post(set_log_level))
        .route("/api/v1/settings/manual-charge", post(set_manual_charge))
        .route("/api/v1/devices", get(list_devices))
        .route("/api/v1/devices/scan", post(scan_devices))
        .route("/api/v1/devices/{id}/bind", post(bind_device))
        .route("/api/v1/devices/{id}/connect", post(connect_device))
        .route("/api/v1/devices/{id}/disconnect", post(disconnect_device))
        .route("/api/v1/devices/{id}/binding", delete(unbind_device))
        .route("/api/v1/devices/{id}/identity", get(device_identity))
        .route(
            "/api/v1/devices/{id}/artifact",
            get(device_artifact).post(select_artifact),
        )
        .route("/api/v1/devices/{id}/flash", post(flash_device))
        .route("/api/v1/devices/{id}/reset", post(reset_device))
        .route("/api/v1/devices/{id}/monitor/start", post(monitor_start))
        .route("/api/v1/devices/{id}/monitor/stop", post(monitor_stop))
        .route("/api/v1/devices/{id}/session", get(device_session))
        .route("/api/v1/devices/{id}/events", get(device_events))
        .route("/api/v1/serial/session", get(devd_compat_session))
        .route("/api/v1/serial/events", get(devd_compat_events))
        .route("/api/v1/defmt/decode", post(defmt_decode))
        .with_state(state);

    if config.allow_dev_cors {
        app = app.layer(
            CorsLayer::new()
                .allow_origin([
                    HeaderValue::from_static("http://127.0.0.1:5173"),
                    HeaderValue::from_static("http://localhost:5173"),
                    HeaderValue::from_static("http://127.0.0.1:30000"),
                    HeaderValue::from_static("http://localhost:30000"),
                ])
                .allow_methods([Method::GET, Method::POST, Method::DELETE])
                .allow_headers(tower_http::cors::Any),
        );
    }
    if let Some(web_root) = config.web_root {
        app = app.fallback_service(ServeDir::new(web_root));
    }

    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .expect("bind mains-aegis-devd");
    tracing::info!("mains-aegis-devd listening on http://{}", config.bind);
    axum::serve(listener, app).await.expect("serve devd");
}

fn parse_args() -> Result<Config, String> {
    let mut args = env::args().skip(1);
    if matches!(args.next().as_deref(), Some("serve")) {
    } else {
        return Err("missing subcommand: serve".to_string());
    }
    let mut bind = env::var("MAINS_AEGIS_DEVD_BIND").unwrap_or_else(|_| DEFAULT_BIND.to_string());
    let mut web_root = env::var("MAINS_AEGIS_DEVD_WEB_ROOT")
        .ok()
        .map(PathBuf::from);
    let mut allow_dev_cors =
        env::var("MAINS_AEGIS_DEVD_ALLOW_DEV_CORS").ok().as_deref() == Some("1");
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bind" => {
                bind = args
                    .next()
                    .ok_or_else(|| "--bind requires an address".to_string())?
            }
            "--web-root" => {
                web_root = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| "--web-root requires a path".to_string())?,
                ))
            }
            "--allow-dev-cors" => allow_dev_cors = true,
            "-h" | "--help" => return Err("mains-aegis-devd serve".to_string()),
            value => return Err(format!("unknown argument: {value}")),
        }
    }
    Ok(Config {
        bind: bind
            .parse()
            .map_err(|_| format!("invalid --bind address: {bind}"))?,
        web_root,
        allow_dev_cors,
    })
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

async fn scan_devices(State(state): State<AppState>) -> Result<Json<Value>, HttpError> {
    let mut discovered = Vec::new();
    let ports = serialport::available_ports()
        .map_err(|error| HttpError::retryable("serial_scan_failed", error.to_string()))?;
    let mut guard = state.inner.lock().expect("state lock");
    let mut seen_native_ids = HashSet::new();
    for port in ports {
        let id = stable_device_id(&port);
        seen_native_ids.insert(id.clone());
        let port_path = port.port_name.clone();
        {
            let entry = guard
                .devices
                .entry(id.clone())
                .or_insert_with(|| DeviceRecord {
                    id: id.clone(),
                    display_name: port_path.clone(),
                    port_path: Some(port_path.clone()),
                    transport: DeviceTransport::NativeSerial,
                    binding: None,
                    connection: ConnectionState::Disconnected,
                    identity: None,
                    status: None,
                    selected_artifact_id: None,
                    log_decode: LogDecodeState::default(),
                    safe_settings: default_safe_settings(),
                    logs: VecDeque::new(),
                    trace: VecDeque::new(),
                });
            let preferred_path = prefer_serial_port_path(entry.port_path.as_deref(), &port_path);
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
    discovered.extend(
        seen_native_ids
            .iter()
            .filter_map(|id| guard.devices.get(id).cloned()),
    );
    discovered.sort_by(|left, right| left.id.cmp(&right.id));
    let stale_ids = guard
        .devices
        .iter()
        .filter_map(|(id, device)| {
            (matches!(device.transport, DeviceTransport::NativeSerial)
                && !seen_native_ids.contains(id))
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
    drop(guard);
    emit(
        &state,
        None,
        "scan",
        "serial scan completed",
        json!({"count": discovered.len()}),
    );
    Ok(Json(json!({"devices": discovered})))
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
    };
    device.binding = Some(binding.clone());
    let device = device.clone();
    guard.bindings.insert(id.clone(), binding);
    drop(guard);
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
        Some(read_native_identity_async(port_path).await?)
    } else {
        None
    };
    let mut guard = state.inner.lock().expect("state lock");
    let selected_artifact = guard
        .devices
        .get(&id)
        .and_then(|device| device.selected_artifact_id.clone())
        .and_then(|artifact_id| guard.artifacts.get(&artifact_id).cloned());
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
    push_log(
        device,
        "info",
        "devd",
        "device connected through mains-aegis-devd",
    );
    let device = device.clone();
    drop(guard);
    emit(&state, Some(id), "connect", "device connected", json!({}));
    Ok(Json(device))
}

async fn disconnect_device(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<DeviceRecord>, HttpError> {
    let stop = {
        let mut guard = state.inner.lock().expect("state lock");
        guard.monitors.remove(&id)
    };
    if let Some(stop) = stop {
        stop.stop.store(true, Ordering::SeqCst);
    }
    let mut guard = state.inner.lock().expect("state lock");
    let device = guard
        .devices
        .get_mut(&id)
        .ok_or_else(|| HttpError::not_found("device_not_found", "device is not known"))?;
    device.connection = ConnectionState::Disconnected;
    let device = device.clone();
    drop(guard);
    emit(
        &state,
        Some(id),
        "disconnect",
        "device disconnected",
        json!({}),
    );
    Ok(Json(device))
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
    drop(guard);
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

async fn select_artifact(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ArtifactSelectRequest>,
) -> Result<Json<Value>, HttpError> {
    let mut loaded_artifact_id = None;
    let mut artifact = None;
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
    drop(guard);
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
    let status = Command::new(
        env::var("MAINS_AEGIS_DEVD_ESPFLASH_BIN").unwrap_or_else(|_| "espflash".to_string()),
    )
    .arg("flash")
    .arg("--port")
    .arg(&port_path)
    .arg(&elf.path)
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .status()
    .await
    .map_err(|error| HttpError::retryable("espflash_launch_failed", error.to_string()))?;
    if !status.success() {
        return Err(HttpError::retryable(
            "espflash_failed",
            format!("espflash exited with {status}"),
        ));
    }
    emit(
        &state,
        Some(id.clone()),
        "flash",
        "flash completed",
        json!({"artifact_id": artifact.artifact_id}),
    );
    Ok(Json(
        json!({"ok": true, "artifact_id": artifact.artifact_id}),
    ))
}

async fn reset_device(
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
    if matches!(transport, DeviceTransport::NativeSerial) {
        let port_path = port_path.ok_or_else(|| {
            HttpError::retryable(
                "device_port_missing",
                "native serial device has no port path",
            )
        })?;
        reset_native_serial_async(port_path).await?;
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
    let ssid = input.ssid.clone();
    let response = send_safe_settings_frame(
        &state,
        json!({"type": "wifi_config", "op": "set", "ssid": input.ssid, "psk": input.psk}),
        input.device_id.as_deref(),
        |settings| {
            settings.wifi_configured = Some(true);
            settings.wifi_ssid = Some(ssid);
        },
        "wifi_config",
        "WiFi credentials saved through mains-aegis-devd",
    )
    .await?;
    Ok(Json(response))
}

async fn clear_wifi_config(
    State(state): State<AppState>,
    Query(query): Query<SafeSettingsTargetQuery>,
) -> Result<Json<Value>, HttpError> {
    let response = send_safe_settings_frame(
        &state,
        json!({"type": "wifi_config", "op": "clear"}),
        query.device_id.as_deref(),
        |settings| {
            settings.wifi_configured = Some(false);
            settings.wifi_ssid = None;
        },
        "wifi_config",
        "WiFi credentials cleared through mains-aegis-devd",
    )
    .await?;
    Ok(Json(response))
}

async fn set_log_level(
    State(state): State<AppState>,
    Json(input): Json<LogLevelRequest>,
) -> Result<Json<Value>, HttpError> {
    let level = input.level.clone();
    let response = send_safe_settings_frame(
        &state,
        json!({"type": "request", "op": "set_log_level", "level": input.level}),
        input.device_id.as_deref(),
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
    let prefs = ManualChargePrefs {
        target: input.target.clone(),
        speed: input.speed.clone(),
        timer_h: input.timer_h,
    };
    let response = send_safe_settings_frame(
        &state,
        json!({
            "type": "request",
            "op": "set_manual_charge_prefs",
            "target": input.target,
            "speed": input.speed,
            "timer_h": input.timer_h
        }),
        input.device_id.as_deref(),
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

async fn device_session(
    Query(query): Query<SessionQuery>,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let guard = state.inner.lock().expect("state lock");
    let device = guard
        .devices
        .get(&id)
        .ok_or_else(|| HttpError::not_found("device_not_found", "device is not known"))?;
    Ok(Json(json!({
        "connected": matches!(device.connection, ConnectionState::Connected),
        "protocol": "mains-aegis.cdc.v1",
        "identity": device.identity,
        "status": device.status,
        "logs": tail(&device.logs, query.logs_limit.unwrap_or(200).min(500)),
        "trace": tail(&device.trace, query.trace_limit.unwrap_or(600).min(2_000)),
        "safeSettings": device.safe_settings,
        "log_decode": device.log_decode
    })))
}

async fn devd_compat_session(
    Query(query): Query<SessionQuery>,
    State(state): State<AppState>,
) -> Json<Value> {
    let guard = state.inner.lock().expect("state lock");
    let device = select_compat_device(&guard);
    Json(json!({
        "connected": device.map(|d| matches!(d.connection, ConnectionState::Connected)).unwrap_or(false),
        "protocol": "mains-aegis.cdc.v1",
        "status": device.and_then(|d| d.status.clone()),
        "logs": device.map(|d| tail(&d.logs, query.logs_limit.unwrap_or(200).min(500))).unwrap_or_default(),
        "trace": device.map(|d| tail(&d.trace, query.trace_limit.unwrap_or(600).min(2_000))).unwrap_or_default(),
        "safeSettings": device.map(|d| json!(d.safe_settings)).unwrap_or_else(|| json!(default_safe_settings()))
    }))
}

async fn devd_compat_identity(State(state): State<AppState>) -> Result<Json<Value>, HttpError> {
    let guard = state.inner.lock().expect("state lock");
    let device = select_compat_device(&guard).ok_or_else(|| {
        HttpError::not_found(
            "identity_unavailable",
            "no device identity is available through devd",
        )
    })?;
    let identity = device.identity.clone().ok_or_else(|| {
        HttpError::non_retryable(
            "identity_unavailable",
            "device identity is unavailable until a devd device is connected",
        )
    })?;
    Ok(Json(identity))
}

async fn devd_compat_network(State(state): State<AppState>) -> Result<Json<Value>, HttpError> {
    let Json(identity) = devd_compat_identity(State(state)).await?;
    let network = identity.get("network").cloned().ok_or_else(|| {
        HttpError::non_retryable(
            "network_unavailable",
            "device identity does not include network",
        )
    })?;
    Ok(Json(network))
}

async fn devd_compat_status(State(state): State<AppState>) -> Result<Json<Value>, HttpError> {
    let guard = state.inner.lock().expect("state lock");
    let device = select_compat_device(&guard).ok_or_else(|| {
        HttpError::not_found(
            "status_unavailable",
            "no device status is available through devd",
        )
    })?;
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

fn select_compat_device(state: &DevdState) -> Option<&DeviceRecord> {
    let mut devices = state.devices.values().collect::<Vec<_>>();
    devices.sort_by(|left, right| left.id.cmp(&right.id));
    devices
        .iter()
        .copied()
        .find(|device| {
            matches!(device.connection, ConnectionState::Connected)
                && !matches!(device.transport, DeviceTransport::Mock)
        })
        .or_else(|| {
            devices.iter().copied().find(|device| {
                !matches!(device.transport, DeviceTransport::Mock) && device.identity.is_some()
            })
        })
        .or_else(|| {
            devices.iter().copied().find(|device| {
                !matches!(device.transport, DeviceTransport::Mock)
                    && (!device.trace.is_empty() || !device.logs.is_empty())
            })
        })
        .or_else(|| {
            devices
                .iter()
                .copied()
                .find(|device| matches!(device.connection, ConnectionState::Connected))
        })
        .or_else(|| {
            devices
                .iter()
                .copied()
                .find(|device| !matches!(device.transport, DeviceTransport::Mock))
        })
        .or_else(|| devices.first().copied())
}

async fn send_safe_settings_frame<F>(
    state: &AppState,
    mut frame: Value,
    target_device_id: Option<&str>,
    apply_settings: F,
    log_target: &str,
    log_message: &str,
) -> Result<Value, HttpError>
where
    F: FnOnce(&mut SafeSettingsState),
{
    let (device_id, port_path, monitor_command_tx) = {
        let guard = state.inner.lock().expect("state lock");
        let device = select_control_device(&guard, target_device_id)?;
        (
            device.id.clone(),
            device.port_path.clone(),
            guard
                .monitors
                .get(&device.id)
                .map(|monitor| monitor.command_tx.clone()),
        )
    };
    let request_id = format!("devd-safe-{}", Utc::now().timestamp_millis());
    if let Value::Object(object) = &mut frame {
        object.insert("request_id".to_string(), Value::String(request_id.clone()));
    }
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

    let result = response.get("result").cloned().unwrap_or(Value::Null);
    record_safe_settings_success(
        state,
        &device_id,
        frame,
        response,
        apply_settings,
        log_target,
        log_message,
    );
    Ok(result)
}

fn select_control_device<'a>(
    state: &'a DevdState,
    target_device_id: Option<&str>,
) -> Result<&'a DeviceRecord, HttpError> {
    let mut devices = state.devices.values().collect::<Vec<_>>();
    devices.sort_by(|left, right| left.id.cmp(&right.id));
    let eligible = devices
        .iter()
        .copied()
        .filter(|device| {
            matches!(device.connection, ConnectionState::Connected)
                && device.identity.is_some()
                && matches!(device.transport, DeviceTransport::NativeSerial)
        })
        .collect::<Vec<_>>();
    if let Some(target_device_id) = target_device_id {
        return eligible
            .into_iter()
            .find(|device| device_matches_identity_id(device, target_device_id))
            .ok_or_else(|| {
                HttpError::retryable(
                    "devd_usb_session_required",
                    format!("connect USB CDC device {target_device_id} through mains-aegis-devd before changing safe settings"),
                )
            });
    }
    match eligible.as_slice() {
        [device] => Ok(*device),
        [] => Err(HttpError::retryable(
            "devd_usb_session_required",
            "connect a USB CDC device through mains-aegis-devd before changing safe settings",
        )),
        _ => Err(HttpError::non_retryable(
            "devd_usb_device_ambiguous",
            "multiple USB CDC devices are connected; provide device_id for safe settings",
        )),
    }
}

fn device_matches_identity_id(device: &DeviceRecord, target_device_id: &str) -> bool {
    device.id == target_device_id
        || device
            .identity
            .as_ref()
            .and_then(|identity| identity.get("device_id"))
            .and_then(Value::as_str)
            .is_some_and(|device_id| device_id == target_device_id)
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

fn send_native_cdc_frame(
    port_path: &str,
    frame: Value,
    request_id: &str,
) -> Result<Value, HttpError> {
    let mut port = serialport::new(port_path, 115_200)
        .timeout(Duration::from_millis(250))
        .open()
        .map_err(|error| {
            HttpError::retryable(
                "native_serial_open_failed",
                format!("failed to open {port_path}: {error}"),
            )
        })?;
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
    let Ok(text) = std::str::from_utf8(line) else {
        return Ok(None);
    };
    let Ok(frame) = serde_json::from_str::<Value>(text.trim()) else {
        return Ok(None);
    };
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
        Ok(Some(frame))
    } else {
        Ok(None)
    }
}

fn record_safe_settings_success<F>(
    state: &AppState,
    device_id: &str,
    tx_frame: Value,
    rx_frame: Value,
    apply_settings: F,
    log_target: &str,
    log_message: &str,
) where
    F: FnOnce(&mut SafeSettingsState),
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
    {
        let mut guard = state.inner.lock().expect("state lock");
        if let Some(device) = guard.devices.get_mut(device_id) {
            apply_settings(&mut device.safe_settings);
            push_bounded(&mut device.trace, tx_trace.clone(), LOG_LIMIT);
            push_bounded(&mut device.trace, rx_trace.clone(), LOG_LIMIT);
            push_bounded(&mut device.logs, log.clone(), LOG_LIMIT);
            device.connection = ConnectionState::Connected;
        }
    }
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
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Sse<impl futures_core::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let receiver = state.events.subscribe();
    let stream = async_stream::stream! {
        let mut receiver = receiver;
        while let Ok(event) = receiver.recv().await {
            if event.device_id.as_deref() == Some(id.as_str()) || event.device_id.is_none() {
                yield Ok(Event::default().event(event.kind.clone()).id(event.id.clone()).json_data(event).expect("serialize event"));
            }
        }
    };
    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}

async fn devd_compat_events(
    State(state): State<AppState>,
) -> Sse<impl futures_core::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let receiver = state.events.subscribe();
    let stream = async_stream::stream! {
        let mut receiver = receiver;
        while let Ok(event) = receiver.recv().await {
            if matches!(event.kind.as_str(), "serial_trace" | "serial_log" | "serial_status" | "monitor") {
                yield Ok(Event::default().event(event.kind.clone()).id(event.id.clone()).json_data(event).expect("serialize event"));
            }
        }
    };
    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
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
        return Err(HttpError::not_found(
            "defmt_elf_not_found",
            format!("embedded firmware ELF not found: {trimmed}"),
        ));
    }
    Ok(path)
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
    guard.devices.insert(
        "mock-devkit".to_string(),
        DeviceRecord {
            id: "mock-devkit".to_string(),
            display_name: "Mock ESP32-S3 DevKit".to_string(),
            port_path: None,
            transport: DeviceTransport::Mock,
            binding: None,
            connection: ConnectionState::Disconnected,
            identity: Some(mock_identity("mock-devkit")),
            status: None,
            selected_artifact_id: None,
            log_decode: LogDecodeState::default(),
            safe_settings: default_safe_settings(),
            logs: VecDeque::new(),
            trace: VecDeque::new(),
        },
    );
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
            "features": ["web_serial"],
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

async fn read_native_identity_async(port_path: String) -> Result<Value, HttpError> {
    tokio::task::spawn_blocking(move || read_native_identity(&port_path))
        .await
        .map_err(|error| HttpError::retryable("native_identity_join_failed", error.to_string()))?
}

async fn reset_native_serial_async(port_path: String) -> Result<(), HttpError> {
    let status = Command::new(
        env::var("MAINS_AEGIS_DEVD_ESPFLASH_BIN").unwrap_or_else(|_| "espflash".to_string()),
    )
    .arg("reset")
    .arg("--port")
    .arg(&port_path)
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .status()
    .await
    .map_err(|error| HttpError::retryable("espflash_reset_launch_failed", error.to_string()))?;
    if !status.success() {
        return Err(HttpError::retryable(
            "espflash_reset_failed",
            format!("espflash reset exited with {status}"),
        ));
    }
    Ok(())
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

fn run_native_monitor_inner(
    state: &AppState,
    device_id: &str,
    port_path: &str,
    stop: &AtomicBool,
    command_rx: mpsc::Receiver<NativeMonitorCommand>,
) -> Result<(), HttpError> {
    let mut port = serialport::new(port_path, 115_200)
        .timeout(Duration::from_millis(250))
        .open()
        .map_err(|error| {
            HttpError::retryable(
                "native_serial_open_failed",
                format!("failed to open {port_path}: {error}"),
            )
        })?;
    let mut next_status_at = std::time::Instant::now();
    let mut line = Vec::new();
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
            Ok(_) if byte[0] == b'\n' => {
                if !line.is_empty() {
                    if let Some((trace, log)) = parse_cdc_line_for_monitor(&line) {
                        append_monitor_trace(state, device_id, trace, log);
                    }
                    line.clear();
                }
            }
            Ok(_) => {
                if line.len() < 16 * 1024 {
                    line.push(byte[0]);
                } else {
                    append_monitor_trace(
                        state,
                        device_id,
                        raw_trace_entry(
                            "rx",
                            "ignored",
                            "CDC line exceeded 16 KiB",
                            "<line too large>",
                        ),
                        None,
                    );
                    line.clear();
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
    drop(guard);
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
        DeviceTransport::NativeSerial => "espflash_reset",
        DeviceTransport::Mock => "mock",
    }
}

fn read_native_identity(port_path: &str) -> Result<Value, HttpError> {
    let mut port = serialport::new(port_path, 115_200)
        .timeout(Duration::from_millis(250))
        .open()
        .map_err(|error| {
            HttpError::retryable(
                "native_serial_open_failed",
                format!("failed to open {port_path}: {error}"),
            )
        })?;
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
    let Ok(text) = std::str::from_utf8(line) else {
        return Ok(None);
    };
    let Ok(frame) = serde_json::from_str::<Value>(text.trim()) else {
        return Ok(None);
    };
    match frame.get("type").and_then(Value::as_str) {
        Some("response")
            if frame.get("request_id").and_then(Value::as_str) == Some("devd-identity") =>
        {
            Ok(frame.get("result").cloned())
        }
        Some("hello") => Ok(frame.get("identity").cloned()),
        _ => Ok(None),
    }
}

fn parse_cdc_line_for_monitor(line: &[u8]) -> Option<(SerialTraceEntry, Option<SerialLogEntry>)> {
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

fn default_safe_settings() -> SafeSettingsState {
    SafeSettingsState {
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
    fn artifact_match_uses_exact_build_id() {
        let mut device = DeviceRecord {
            id: "d".into(),
            display_name: "d".into(),
            port_path: None,
            transport: DeviceTransport::Mock,
            binding: None,
            connection: ConnectionState::Disconnected,
            identity: Some(
                json!({"firmware": {"build_id": "b1", "git_sha": "g1", "build_profile": "release", "features": ["web_serial"]}}),
            ),
            status: None,
            selected_artifact_id: None,
            log_decode: LogDecodeState::default(),
            safe_settings: default_safe_settings(),
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
            transport: DeviceTransport::Mock,
            binding: None,
            connection: ConnectionState::Disconnected,
            identity: Some(
                json!({"firmware": {"build_id": "debug-build", "git_sha": "same", "build_profile": "release", "features": ["web_serial"]}}),
            ),
            status: None,
            selected_artifact_id: None,
            log_decode: LogDecodeState::default(),
            safe_settings: default_safe_settings(),
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
            transport: DeviceTransport::Mock,
            binding: None,
            connection: ConnectionState::Disconnected,
            identity: Some(
                json!({"firmware": {"build_id": "same-build", "build_profile": "release", "features": ["net_http"]}}),
            ),
            status: None,
            selected_artifact_id: None,
            log_decode: LogDecodeState::default(),
            safe_settings: default_safe_settings(),
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
            transport: DeviceTransport::Mock,
            binding: None,
            connection: ConnectionState::Disconnected,
            identity: Some(
                json!({"firmware": {"build_id": "same-build", "build_profile": "debug", "features": ["web_serial"]}}),
            ),
            status: None,
            selected_artifact_id: None,
            log_decode: LogDecodeState::default(),
            safe_settings: default_safe_settings(),
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
            transport: DeviceTransport::NativeSerial,
            binding: None,
            connection: ConnectionState::Disconnected,
            identity: None,
            status: None,
            selected_artifact_id: None,
            log_decode: LogDecodeState::default(),
            safe_settings: default_safe_settings(),
            logs: VecDeque::new(),
            trace: VecDeque::new(),
        };
        assert_eq!(bound_flash_port(&device), None);
        device.binding = Some(DeviceBinding {
            alias: None,
            stable_id: "d".into(),
            port_path: Some("/dev/cu.usbmodem1".into()),
            created_at: "now".into(),
        });
        assert_eq!(bound_flash_port(&device), Some("/dev/cu.usbmodem1".into()));
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
