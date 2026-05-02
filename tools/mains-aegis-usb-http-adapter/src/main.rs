use axum::{
    extract::{Query, State},
    http::{HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use serialport::SerialPort;
use std::{
    collections::{HashMap, VecDeque},
    env,
    io::{ErrorKind, Read, Write},
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, RwLock,
    },
    thread,
    time::Duration,
};
use tokio::{sync::oneshot, time};
use tower_http::cors::CorsLayer;

const BAUD_RATE: u32 = 115_200;
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);
const TRACE_LIMIT: usize = 10_000;
const LOG_LIMIT: usize = 2_000;
const SESSION_TRACE_DEFAULT_LIMIT: usize = 600;
const SESSION_TRACE_MAX_LIMIT: usize = 2_000;
const SESSION_LOG_DEFAULT_LIMIT: usize = 200;
const SESSION_LOG_MAX_LIMIT: usize = 500;

#[derive(Debug, Clone)]
struct Config {
    port: String,
    bind: SocketAddr,
    allowed_origins: Vec<HeaderValue>,
}

#[derive(Clone)]
struct AppState {
    session: Arc<UsbSession>,
}

struct UsbSession {
    writer: Mutex<Box<dyn SerialPort>>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<Result<Value, ApiError>>>>>,
    connected: Arc<AtomicBool>,
    protocol: Arc<RwLock<String>>,
    identity: Arc<RwLock<Option<Value>>>,
    status: Arc<RwLock<Option<Value>>>,
    logs: Arc<Mutex<VecDeque<SerialLogEntry>>>,
    trace: Arc<Mutex<VecDeque<SerialTraceEntry>>>,
    safe_settings: Arc<RwLock<SafeSettingsState>>,
}

trait VecDequeTail<T> {
    fn tail(&self, limit: usize) -> Vec<T>;
}

impl<T: Clone> VecDequeTail<T> for VecDeque<T> {
    fn tail(&self, limit: usize) -> Vec<T> {
        let skip = self.len().saturating_sub(limit);
        self.iter().skip(skip).cloned().collect()
    }
}

#[derive(Debug, Clone, Serialize)]
struct ApiError {
    code: String,
    message: String,
    retryable: bool,
    details: Option<Value>,
}

#[derive(Debug, Serialize)]
struct ErrorEnvelope {
    error: ApiError,
}

#[derive(Debug, Clone, Serialize)]
struct SerialLogEntry {
    id: String,
    timestamp: String,
    level: String,
    target: String,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
struct SerialTraceEntry {
    id: String,
    timestamp: String,
    direction: &'static str,
    kind: &'static str,
    #[serde(rename = "frameType")]
    frame_type: Option<String>,
    #[serde(rename = "requestId")]
    request_id: Option<String>,
    target: Option<String>,
    summary: String,
    payload: String,
}

#[derive(Debug, Clone, Serialize)]
struct SafeSettingsState {
    wifi_configured: Option<bool>,
    wifi_ssid: Option<String>,
    log_level: String,
    manual_charge: ManualChargePrefs,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ManualChargePrefs {
    target: String,
    speed: String,
    timer_h: u8,
}

#[derive(Debug, Deserialize)]
struct WifiConfigRequest {
    ssid: String,
    psk: String,
}

#[derive(Debug, Deserialize)]
struct LogLevelRequest {
    level: String,
}

#[derive(Debug, Serialize)]
struct PingResponse {
    ok: bool,
    adapter: &'static str,
}

#[derive(Debug, Serialize)]
struct SerialSessionResponse {
    connected: bool,
    protocol: String,
    logs: Vec<SerialLogEntry>,
    trace: Vec<SerialTraceEntry>,
    #[serde(rename = "safeSettings")]
    safe_settings: SafeSettingsState,
}

#[derive(Debug, Deserialize)]
struct SerialSessionQuery {
    logs_limit: Option<usize>,
    trace_limit: Option<usize>,
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
            eprintln!(
                "usage: mains-aegis-usb-http-adapter --port <serial-path> [--bind 127.0.0.1:30080] [--allow-origin http://127.0.0.1:5173]"
            );
            std::process::exit(2);
        }
    };

    let session = match UsbSession::open(&config.port).await {
        Ok(session) => Arc::new(session),
        Err(error) => {
            eprintln!("failed to open USB CDC session: {}", error.message);
            std::process::exit(1);
        }
    };

    let app_state = AppState { session };
    let cors = CorsLayer::new()
        .allow_origin(config.allowed_origins)
        .allow_methods([Method::GET, Method::POST, Method::DELETE])
        .allow_headers(tower_http::cors::Any);
    let app = Router::new()
        .route("/health", get(health))
        .route("/api/v1/ping", get(ping))
        .route("/api/v1/identity", get(identity))
        .route("/api/v1/network", get(network))
        .route("/api/v1/status", get(status))
        .route("/api/v1/serial/session", get(serial_session))
        .route(
            "/api/v1/wifi-config",
            post(set_wifi_config).delete(clear_wifi_config),
        )
        .route("/api/v1/settings/log-level", post(set_log_level))
        .route("/api/v1/settings/manual-charge", post(set_manual_charge))
        .layer(cors)
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .expect("bind local HTTP adapter");
    tracing::info!(
        "mains-aegis USB HTTP adapter listening on http://{}",
        config.bind
    );
    axum::serve(listener, app).await.expect("serve adapter");
}

fn parse_args() -> Result<Config, String> {
    let mut args = env::args().skip(1);
    let mut port = env::var("MAINS_AEGIS_USB_PORT").ok();
    let mut bind = env::var("MAINS_AEGIS_ADAPTER_BIND")
        .ok()
        .unwrap_or_else(|| "127.0.0.1:30080".to_string());
    let mut origins = env::var("MAINS_AEGIS_WEB_ORIGINS").ok();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => port = args.next(),
            "--bind" => {
                bind = args
                    .next()
                    .ok_or_else(|| "--bind requires an address".to_string())?
            }
            "--allow-origin" => origins = args.next(),
            "--help" | "-h" => return Err(String::from("mains-aegis USB CDC to HTTP adapter")),
            value => return Err(format!("unknown argument: {value}")),
        }
    }

    let port = port.ok_or_else(|| "--port or MAINS_AEGIS_USB_PORT is required".to_string())?;
    let bind = bind
        .parse()
        .map_err(|_| format!("invalid --bind address: {bind}"))?;
    let allowed_origins = parse_allowed_origins(origins.as_deref())?;
    Ok(Config {
        port,
        bind,
        allowed_origins,
    })
}

fn parse_allowed_origins(input: Option<&str>) -> Result<Vec<HeaderValue>, String> {
    let mut origins = Vec::new();
    for origin in [
        "http://127.0.0.1:30000",
        "http://localhost:30000",
        "http://127.0.0.1:5173",
        "http://localhost:5173",
    ] {
        push_origin(&mut origins, origin)?;
    }
    if let Ok(web_port) = env::var("WEB_PORT") {
        if !web_port.trim().is_empty() {
            push_origin(
                &mut origins,
                &format!("http://127.0.0.1:{}", web_port.trim()),
            )?;
            push_origin(
                &mut origins,
                &format!("http://localhost:{}", web_port.trim()),
            )?;
        }
    }
    if let Some(input) = input {
        for origin in input
            .split(',')
            .map(str::trim)
            .filter(|origin| !origin.is_empty())
        {
            push_origin(&mut origins, origin)?;
        }
    }
    Ok(origins)
}

fn push_origin(origins: &mut Vec<HeaderValue>, origin: &str) -> Result<(), String> {
    let value = origin
        .parse::<HeaderValue>()
        .map_err(|_| format!("invalid allowed origin: {origin}"))?;
    if !origins.iter().any(|existing| existing == &value) {
        origins.push(value);
    }
    Ok(())
}

impl UsbSession {
    async fn open(path: &str) -> Result<Self, ApiError> {
        let reader = serialport::new(path, BAUD_RATE)
            .timeout(Duration::from_millis(40))
            .open()
            .map_err(|error| {
                ApiError::retryable(
                    "serial_open_failed",
                    format!("failed to open {path}: {error}"),
                )
            })?;
        let writer = reader.try_clone().map_err(|error| {
            ApiError::retryable(
                "serial_clone_failed",
                format!("failed to clone serial writer: {error}"),
            )
        })?;

        let session = Self {
            writer: Mutex::new(writer),
            pending: Arc::new(Mutex::new(HashMap::new())),
            connected: Arc::new(AtomicBool::new(true)),
            protocol: Arc::new(RwLock::new("unknown".to_string())),
            identity: Arc::new(RwLock::new(None)),
            status: Arc::new(RwLock::new(None)),
            logs: Arc::new(Mutex::new(VecDeque::new())),
            trace: Arc::new(Mutex::new(VecDeque::new())),
            safe_settings: Arc::new(RwLock::new(default_safe_settings())),
        };

        session.spawn_reader(reader);
        let hello = session
            .send_and_wait(json!({"type":"hello"}), Some("adapter-hello"))
            .await?;
        if hello.get("type").and_then(Value::as_str) != Some("hello") {
            return Err(ApiError::non_retryable(
                "unexpected_hello",
                "USB CDC handshake returned an unexpected frame",
            ));
        }
        if let Some(protocol) = hello.get("protocol").and_then(Value::as_str) {
            *session.protocol.write().expect("protocol lock") = protocol.to_string();
        }
        if let Some(identity) = hello.get("identity").cloned() {
            *session.identity.write().expect("identity lock") = Some(identity);
        }
        session.push_log(
            "info",
            "usb_cdc",
            "USB CDC connected through local HTTP adapter",
        );
        let _ = session.request("get_status", Map::new()).await;
        Ok(session)
    }

    fn spawn_reader(&self, mut reader: Box<dyn SerialPort>) {
        let pending = Arc::clone(&self.pending);
        let identity = Arc::clone(&self.identity);
        let status = Arc::clone(&self.status);
        let logs = Arc::clone(&self.logs);
        let trace = Arc::clone(&self.trace);
        let connected = Arc::clone(&self.connected);
        thread::spawn(move || {
            let mut read_buffer = Vec::new();
            let mut buf = [0_u8; 256];
            while connected.load(Ordering::SeqCst) {
                match reader.read(&mut buf) {
                    Ok(n) if n > 0 => {
                        read_buffer.extend_from_slice(&buf[..n]);
                        consume_reader_buffer(
                            &mut read_buffer,
                            &pending,
                            &identity,
                            &status,
                            &logs,
                            &trace,
                        );
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == ErrorKind::TimedOut => {}
                    Err(error) => {
                        connected.store(false, Ordering::SeqCst);
                        push_log_to(
                            &logs,
                            "error",
                            "usb_cdc",
                            &format!("serial read failed: {error}"),
                        );
                        break;
                    }
                }
            }
        });
    }

    async fn request(&self, op: &str, mut payload: Map<String, Value>) -> Result<Value, ApiError> {
        payload.insert("type".to_string(), Value::String("request".to_string()));
        payload.insert("op".to_string(), Value::String(op.to_string()));
        let frame = Value::Object(payload);
        let response = self.send_and_wait(frame, None).await?;
        if response.get("type").and_then(Value::as_str) != Some("response") {
            return Err(ApiError::retryable(
                "unexpected_response",
                "USB CDC command returned an unexpected frame",
            ));
        }
        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    }

    async fn refresh_identity(&self) -> Result<Value, ApiError> {
        let identity = self.request("get_identity", Map::new()).await?;
        if !identity.is_object() {
            return Err(ApiError::retryable(
                "identity_unavailable",
                "USB CDC identity response was not an object",
            ));
        }
        *self.identity.write().expect("identity lock") = Some(identity.clone());
        Ok(identity)
    }

    async fn wifi_config(
        &self,
        op: &str,
        ssid: Option<String>,
        psk: Option<String>,
    ) -> Result<Value, ApiError> {
        let mut payload = Map::new();
        payload.insert("type".to_string(), Value::String("wifi_config".to_string()));
        payload.insert("op".to_string(), Value::String(op.to_string()));
        if let Some(ssid) = ssid {
            payload.insert("ssid".to_string(), Value::String(ssid));
        }
        if let Some(psk) = psk {
            payload.insert("psk".to_string(), Value::String(psk));
        }
        let response = self.send_and_wait(Value::Object(payload), None).await?;
        if response.get("type").and_then(Value::as_str) != Some("response") {
            return Err(ApiError::retryable(
                "unexpected_response",
                "USB CDC WiFi command returned an unexpected frame",
            ));
        }
        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    }

    async fn send_and_wait(
        &self,
        mut frame: Value,
        fixed_id: Option<&str>,
    ) -> Result<Value, ApiError> {
        if !self.connected.load(Ordering::SeqCst) {
            return Err(ApiError::retryable(
                "serial_disconnected",
                "USB CDC serial port is disconnected",
            ));
        }
        let request_id = fixed_id.map(str::to_string).unwrap_or_else(next_request_id);
        if let Value::Object(object) = &mut frame {
            object.insert("request_id".to_string(), Value::String(request_id.clone()));
        }

        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .expect("pending lock")
            .insert(request_id.clone(), tx);

        let payload = serde_json::to_string(&frame)
            .map_err(|error| ApiError::non_retryable("json_encode_failed", error.to_string()))?;
        self.push_trace(trace_from_frame(
            "tx",
            &frame,
            &serde_json::to_string(&redact_frame(&frame)).unwrap_or(payload.clone()),
        ));
        let write_result = {
            let mut writer = self.writer.lock().expect("writer lock");
            writer
                .write_all(payload.as_bytes())
                .and_then(|_| writer.write_all(b"\n"))
        };
        if let Err(error) = write_result {
            self.pending
                .lock()
                .expect("pending lock")
                .remove(&request_id);
            return Err(ApiError::retryable(
                "serial_write_failed",
                format!("USB CDC write failed: {error}"),
            ));
        }

        match time::timeout(RESPONSE_TIMEOUT, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(ApiError::retryable(
                "serial_response_cancelled",
                "USB CDC response waiter was cancelled",
            )),
            Err(_) => {
                self.pending
                    .lock()
                    .expect("pending lock")
                    .remove(&request_id);
                Err(ApiError::retryable(
                    "serial_response_timeout",
                    "USB CDC response timed out",
                ))
            }
        }
    }

    fn push_trace(&self, entry: SerialTraceEntry) {
        push_trace_to(&self.trace, entry);
    }

    fn push_log(&self, level: &str, target: &str, message: &str) {
        push_log_to(&self.logs, level, target, message);
    }
}

async fn health() -> Json<PingResponse> {
    Json(PingResponse {
        ok: true,
        adapter: "mains-aegis-usb-http-adapter",
    })
}

async fn ping() -> Json<PingResponse> {
    health().await
}

async fn identity(State(state): State<AppState>) -> Result<Json<Value>, HttpError> {
    state
        .session
        .refresh_identity()
        .await
        .map(Json)
        .map_err(HttpError)
}

async fn network(State(state): State<AppState>) -> Result<Json<Value>, HttpError> {
    let identity = state.session.refresh_identity().await.map_err(HttpError)?;
    identity.get("network").cloned().map(Json).ok_or_else(|| {
        HttpError(ApiError::retryable(
            "network_unavailable",
            "USB identity did not include network summary",
        ))
    })
}

async fn status(State(state): State<AppState>) -> Result<Json<Value>, HttpError> {
    let result = state
        .session
        .request("get_status", Map::new())
        .await
        .map_err(HttpError)?;
    if result.is_object() {
        *state.session.status.write().expect("status lock") = Some(result.clone());
        return Ok(Json(result));
    }
    state
        .session
        .status
        .read()
        .expect("status lock")
        .clone()
        .map(Json)
        .ok_or_else(|| {
            HttpError(ApiError::retryable(
                "status_unavailable",
                "USB status is not available yet",
            ))
        })
}

async fn serial_session(
    Query(query): Query<SerialSessionQuery>,
    State(state): State<AppState>,
) -> Json<SerialSessionResponse> {
    let logs_limit = query
        .logs_limit
        .unwrap_or(SESSION_LOG_DEFAULT_LIMIT)
        .min(SESSION_LOG_MAX_LIMIT);
    let trace_limit = query
        .trace_limit
        .unwrap_or(SESSION_TRACE_DEFAULT_LIMIT)
        .min(SESSION_TRACE_MAX_LIMIT);

    Json(SerialSessionResponse {
        connected: state.session.connected.load(Ordering::SeqCst),
        protocol: state
            .session
            .protocol
            .read()
            .expect("protocol lock")
            .clone(),
        logs: state
            .session
            .logs
            .lock()
            .expect("logs lock")
            .tail(logs_limit),
        trace: state
            .session
            .trace
            .lock()
            .expect("trace lock")
            .tail(trace_limit),
        safe_settings: state
            .session
            .safe_settings
            .read()
            .expect("settings lock")
            .clone(),
    })
}

async fn set_wifi_config(
    State(state): State<AppState>,
    Json(input): Json<WifiConfigRequest>,
) -> Result<Json<Value>, HttpError> {
    let result = state
        .session
        .wifi_config("set", Some(input.ssid.clone()), Some(input.psk))
        .await
        .map_err(HttpError)?;
    {
        let mut settings = state.session.safe_settings.write().expect("settings lock");
        settings.wifi_configured = Some(true);
        settings.wifi_ssid = Some(input.ssid);
    }
    Ok(Json(result))
}

async fn clear_wifi_config(State(state): State<AppState>) -> Result<Json<Value>, HttpError> {
    let result = state
        .session
        .wifi_config("clear", None, None)
        .await
        .map_err(HttpError)?;
    {
        let mut settings = state.session.safe_settings.write().expect("settings lock");
        settings.wifi_configured = Some(false);
        settings.wifi_ssid = None;
    }
    Ok(Json(result))
}

async fn set_log_level(
    State(state): State<AppState>,
    Json(input): Json<LogLevelRequest>,
) -> Result<Json<Value>, HttpError> {
    let mut payload = Map::new();
    payload.insert("level".to_string(), Value::String(input.level.clone()));
    let result = state
        .session
        .request("set_log_level", payload)
        .await
        .map_err(HttpError)?;
    state
        .session
        .safe_settings
        .write()
        .expect("settings lock")
        .log_level = input.level;
    Ok(Json(result))
}

async fn set_manual_charge(
    State(state): State<AppState>,
    Json(input): Json<ManualChargePrefs>,
) -> Result<Json<Value>, HttpError> {
    let mut payload = Map::new();
    payload.insert("target".to_string(), Value::String(input.target.clone()));
    payload.insert("speed".to_string(), Value::String(input.speed.clone()));
    payload.insert(
        "timer_h".to_string(),
        Value::Number(serde_json::Number::from(input.timer_h)),
    );
    let result = state
        .session
        .request("set_manual_charge_prefs", payload)
        .await
        .map_err(HttpError)?;
    state
        .session
        .safe_settings
        .write()
        .expect("settings lock")
        .manual_charge = input;
    Ok(Json(result))
}

struct HttpError(ApiError);

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let status = if self.0.retryable {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            StatusCode::BAD_REQUEST
        };
        (status, Json(ErrorEnvelope { error: self.0 })).into_response()
    }
}

impl ApiError {
    fn retryable(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable: true,
            details: None,
        }
    }

    fn non_retryable(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable: false,
            details: None,
        }
    }
}

fn consume_reader_buffer(
    read_buffer: &mut Vec<u8>,
    pending: &Arc<Mutex<HashMap<String, oneshot::Sender<Result<Value, ApiError>>>>>,
    identity: &Arc<RwLock<Option<Value>>>,
    status: &Arc<RwLock<Option<Value>>>,
    logs: &Arc<Mutex<VecDeque<SerialLogEntry>>>,
    trace: &Arc<Mutex<VecDeque<SerialTraceEntry>>>,
) {
    while let Some(newline_index) = read_buffer.iter().position(|byte| *byte == b'\n') {
        let mut raw_bytes = read_buffer.drain(..=newline_index).collect::<Vec<_>>();
        if raw_bytes.last() == Some(&b'\n') {
            raw_bytes.pop();
        }
        if raw_bytes.last() == Some(&b'\r') {
            raw_bytes.pop();
        }
        let raw_line = match String::from_utf8(raw_bytes) {
            Ok(line) => line.trim().to_string(),
            Err(error) => {
                push_trace_to(
                    trace,
                    raw_trace(
                        "rx",
                        "ignored",
                        "invalid UTF-8 CDC line",
                        &String::from_utf8_lossy(error.as_bytes()),
                    ),
                );
                continue;
            }
        };
        if raw_line.is_empty() {
            continue;
        }
        let Some(candidate) = extract_json_candidate(&raw_line) else {
            push_trace_to(trace, raw_trace("rx", "raw", "raw CDC line", &raw_line));
            continue;
        };
        let frame = match serde_json::from_str::<Value>(candidate) {
            Ok(frame) => frame,
            Err(_) => {
                push_trace_to(
                    trace,
                    raw_trace("rx", "ignored", "non-protocol CDC line", &raw_line),
                );
                continue;
            }
        };
        push_trace_to(trace, trace_from_frame("rx", &frame, candidate));
        match frame.get("type").and_then(Value::as_str) {
            Some("hello") => {
                if let Some(value) = frame.get("identity").cloned() {
                    *identity.write().expect("identity lock") = Some(value);
                }
            }
            Some("status") => {
                if let Some(value) = frame.get("status").cloned() {
                    *status.write().expect("status lock") = Some(value);
                }
            }
            Some("log") => {
                let level = frame.get("level").and_then(Value::as_str).unwrap_or("info");
                let target = frame
                    .get("target")
                    .and_then(Value::as_str)
                    .unwrap_or("usb_cdc");
                let message = frame
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("structured USB log");
                push_log_to(logs, level, target, message);
            }
            _ => {}
        }
        if let Some(request_id) = frame.get("request_id").and_then(Value::as_str) {
            if let Some(sender) = pending.lock().expect("pending lock").remove(request_id) {
                let result = if frame.get("type").and_then(Value::as_str) == Some("error") {
                    Err(api_error_from_frame(&frame))
                } else {
                    Ok(frame)
                };
                let _ = sender.send(result);
            }
        }
    }
}

fn api_error_from_frame(frame: &Value) -> ApiError {
    let Some(error) = frame.get("error") else {
        return ApiError::retryable("usb_cdc_error", "USB CDC returned an error frame");
    };
    ApiError {
        code: error
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or("usb_cdc_error")
            .to_string(),
        message: error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("USB CDC returned an error frame")
            .to_string(),
        retryable: error
            .get("retryable")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        details: error.get("details").cloned(),
    }
}

fn extract_json_candidate(raw_line: &str) -> Option<&str> {
    let json_start = raw_line.find('{')?;
    Some(&raw_line[json_start..])
}

fn trace_from_frame(direction: &'static str, frame: &Value, payload: &str) -> SerialTraceEntry {
    SerialTraceEntry {
        id: next_event_id(),
        timestamp: now_iso(),
        direction,
        kind: "frame",
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
        summary: summarize_frame(frame),
        payload: payload.to_string(),
    }
}

fn raw_trace(
    direction: &'static str,
    kind: &'static str,
    summary: &str,
    payload: &str,
) -> SerialTraceEntry {
    SerialTraceEntry {
        id: next_event_id(),
        timestamp: now_iso(),
        direction,
        kind,
        frame_type: None,
        request_id: None,
        target: None,
        summary: summary.to_string(),
        payload: payload.to_string(),
    }
}

fn summarize_frame(frame: &Value) -> String {
    match frame.get("type").and_then(Value::as_str) {
        Some("log") => frame
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("log")
            .to_string(),
        Some("error") => {
            let error = frame.get("error");
            format!(
                "{}: {}",
                error
                    .and_then(|value| value.get("code"))
                    .and_then(Value::as_str)
                    .unwrap_or("error"),
                error
                    .and_then(|value| value.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("USB CDC error")
            )
        }
        Some("response") => "command response".to_string(),
        Some("status") => "status snapshot".to_string(),
        Some("hello") => "protocol handshake".to_string(),
        Some("request") => frame
            .get("op")
            .and_then(Value::as_str)
            .unwrap_or("request")
            .to_string(),
        Some("wifi_config") => format!(
            "wifi_config {}",
            frame.get("op").and_then(Value::as_str).unwrap_or("")
        ),
        Some(frame_type) => frame_type.to_string(),
        None => "serial frame".to_string(),
    }
}

fn redact_frame(frame: &Value) -> Value {
    let mut redacted = frame.clone();
    if redacted.get("type").and_then(Value::as_str) == Some("wifi_config") {
        if let Value::Object(object) = &mut redacted {
            if object.contains_key("psk") {
                object.insert("psk".to_string(), Value::String("[redacted]".to_string()));
            }
        }
    }
    redacted
}

fn push_trace_to(trace: &Arc<Mutex<VecDeque<SerialTraceEntry>>>, entry: SerialTraceEntry) {
    let mut trace = trace.lock().expect("trace lock");
    trace.push_back(entry);
    while trace.len() > TRACE_LIMIT {
        trace.pop_front();
    }
}

fn push_log_to(
    logs: &Arc<Mutex<VecDeque<SerialLogEntry>>>,
    level: &str,
    target: &str,
    message: &str,
) {
    let mut logs = logs.lock().expect("logs lock");
    logs.push_back(SerialLogEntry {
        id: next_event_id(),
        timestamp: now_iso(),
        level: level.to_string(),
        target: target.to_string(),
        message: message.to_string(),
    });
    while logs.len() > LOG_LIMIT {
        logs.pop_front();
    }
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

fn next_request_id() -> String {
    format!(
        "adapter-{}-{}",
        Utc::now().timestamp_millis(),
        randomish_suffix()
    )
}

fn next_event_id() -> String {
    format!("{}-{}", Utc::now().timestamp_millis(), randomish_suffix())
}

fn randomish_suffix() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    format!("{:x}", COUNTER.fetch_add(1, Ordering::Relaxed))
}

fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::oneshot;

    #[test]
    fn redacts_wifi_psk_from_trace_payload() {
        let frame = json!({
            "type": "wifi_config",
            "request_id": "test-1",
            "op": "set",
            "ssid": "LabNet",
            "psk": "secret-password"
        });

        let redacted = redact_frame(&frame);

        assert_eq!(redacted["psk"], "[redacted]");
        assert_eq!(redacted["ssid"], "LabNet");
    }

    #[test]
    fn consumes_prefixed_json_log_lines() {
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let identity = Arc::new(RwLock::new(None));
        let status = Arc::new(RwLock::new(None));
        let logs = Arc::new(Mutex::new(VecDeque::new()));
        let trace = Arc::new(Mutex::new(VecDeque::new()));
        let mut read_buffer =
            b"I (123) app: {\"type\":\"log\",\"level\":\"debug\",\"target\":\"wifi\",\"message\":\"associated\"}\n".to_vec();

        consume_reader_buffer(
            &mut read_buffer,
            &pending,
            &identity,
            &status,
            &logs,
            &trace,
        );

        let logs = logs.lock().expect("logs lock");
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].level, "debug");
        assert_eq!(logs[0].target, "wifi");
        assert_eq!(logs[0].message, "associated");
    }

    #[test]
    fn preserves_utf8_split_across_serial_chunks() {
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let identity = Arc::new(RwLock::new(None));
        let status = Arc::new(RwLock::new(None));
        let logs = Arc::new(Mutex::new(VecDeque::new()));
        let trace = Arc::new(Mutex::new(VecDeque::new()));
        let line = "{\"type\":\"log\",\"level\":\"info\",\"target\":\"wifi\",\"message\":\"ssid=实验室\"}\n";
        let split = line.find("验").expect("split point") + 1;
        let mut read_buffer = line.as_bytes()[..split].to_vec();

        consume_reader_buffer(
            &mut read_buffer,
            &pending,
            &identity,
            &status,
            &logs,
            &trace,
        );
        assert!(logs.lock().expect("logs lock").is_empty());

        read_buffer.extend_from_slice(&line.as_bytes()[split..]);
        consume_reader_buffer(
            &mut read_buffer,
            &pending,
            &identity,
            &status,
            &logs,
            &trace,
        );

        let logs = logs.lock().expect("logs lock");
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].message, "ssid=实验室");
    }

    #[test]
    fn routes_error_frames_to_matching_request() {
        let (tx, rx) = oneshot::channel();
        let pending = Arc::new(Mutex::new(HashMap::from([("req-1".to_string(), tx)])));
        let identity = Arc::new(RwLock::new(None));
        let status = Arc::new(RwLock::new(None));
        let logs = Arc::new(Mutex::new(VecDeque::new()));
        let trace = Arc::new(Mutex::new(VecDeque::new()));
        let mut read_buffer =
            b"{\"type\":\"error\",\"request_id\":\"req-1\",\"error\":{\"code\":\"invalid_wifi_psk\",\"message\":\"bad psk\",\"retryable\":false,\"details\":null}}\n"
                .to_vec();

        consume_reader_buffer(
            &mut read_buffer,
            &pending,
            &identity,
            &status,
            &logs,
            &trace,
        );
        let error = rx
            .blocking_recv()
            .expect("response")
            .expect_err("error frame");

        assert_eq!(error.code, "invalid_wifi_psk");
        assert!(!error.retryable);
        assert!(pending.lock().expect("pending lock").is_empty());
    }
}
