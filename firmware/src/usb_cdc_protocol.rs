use core::fmt::Write as _;

use heapless::{String, Vec};

use crate::{
    net_contract::write_json_string_escaped,
    net_types::{
        validate_advanced_power_settings, AdvancedPowerSettingsSnapshot,
        AdvancedPowerValidationError,
    },
};

pub const PROTOCOL_NAME: &str = "mains-aegis.cdc.v1";
pub const WIFI_CONFIG_RECORD_LEN: usize = 128;
pub const WIFI_SSID_MAX_LEN: usize = 32;
pub const WIFI_PSK_MAX_LEN: usize = 63;
pub const WEB_SERIAL_RESPONSE_BODY_CAP: usize = 4096;
pub const WEB_SERIAL_RESPONSE_FRAME_CAP: usize = 4608;
pub const WEB_SERIAL_POWER_DIAG_BODY_CAP: usize = 6144;
pub const WEB_SERIAL_POWER_DIAG_FRAME_CAP: usize = 6656;

const WIFI_CONFIG_MAGIC: [u8; 4] = *b"MAWF";
const WIFI_CONFIG_VERSION: u8 = 1;
const WIFI_CONFIG_CRC_INDEX: usize = WIFI_CONFIG_RECORD_LEN - 1;
const WIFI_CONFIG_SSID_OFFSET: usize = 8;
const WIFI_CONFIG_PSK_OFFSET: usize = WIFI_CONFIG_SSID_OFFSET + WIFI_SSID_MAX_LEN;
const WIFI_ACK_RESULT_CAPACITY: usize = 160;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UsbCdcFrame {
    Hello {
        request_id: Option<String<32>>,
    },
    Request {
        request_id: String<32>,
        op: UsbCdcRequest,
    },
    WifiConfig {
        request_id: String<32>,
        command: WifiConfigCommand,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UsbCdcRequest {
    GetIdentity,
    GetStatus,
    GetSettings,
    GetPowerDiag,
    SetLogLevel(LogLevel),
    SetManualChargePrefs(ManualChargePrefsCommand),
    SetAdvancedPower(AdvancedPowerSettingsSnapshot),
    ResetAdvancedPower,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }

    pub const fn severity(self) -> u8 {
        match self {
            Self::Error => 0,
            Self::Warn => 1,
            Self::Info => 2,
            Self::Debug => 3,
            Self::Trace => 4,
        }
    }

    pub const fn allows(self, emitted: Self) -> bool {
        emitted.severity() <= self.severity()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ManualChargePrefsCommand {
    pub target: ManualChargeTarget,
    pub speed: ManualChargeSpeed,
    pub timer_limit: ManualChargeTimerLimit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManualChargeTarget {
    Pack3V7,
    Rsoc80,
    Full100,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManualChargeSpeed {
    Ma100,
    Ma500,
    Ma1000,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManualChargeTimerLimit {
    H1,
    H2,
    H6,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WifiConfigCommand {
    Set(WifiConfigSecret),
    Clear,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WifiConfigSecret {
    pub ssid: String<WIFI_SSID_MAX_LEN>,
    pub psk: String<64>,
}

impl WifiConfigSecret {
    pub fn new(ssid: &str, psk: &str) -> Result<Self, UsbCdcProtocolError> {
        validate_wifi_ssid(ssid)?;
        validate_wifi_psk(psk)?;
        let mut ssid_buf = String::<WIFI_SSID_MAX_LEN>::new();
        ssid_buf
            .push_str(ssid)
            .map_err(|_| UsbCdcProtocolError::InvalidWifiSsid)?;
        let mut psk_buf = String::<64>::new();
        psk_buf
            .push_str(psk)
            .map_err(|_| UsbCdcProtocolError::InvalidWifiPsk)?;
        Ok(Self {
            ssid: ssid_buf,
            psk: psk_buf,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsbCdcProtocolError {
    InvalidJson,
    MissingField,
    InvalidRequestId,
    UnsupportedType,
    UnsupportedOperation,
    UnsafeOperation,
    InvalidLogLevel,
    InvalidManualChargePrefs,
    InvalidAdvancedPowerSettings,
    InvalidWifiSsid,
    InvalidWifiPsk,
    FrameTooLarge,
}

impl UsbCdcProtocolError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidJson => "invalid_json",
            Self::MissingField => "missing_field",
            Self::InvalidRequestId => "invalid_request_id",
            Self::UnsupportedType => "unsupported_type",
            Self::UnsupportedOperation => "unsupported_operation",
            Self::UnsafeOperation => "unsafe_operation",
            Self::InvalidLogLevel => "invalid_log_level",
            Self::InvalidManualChargePrefs => "invalid_manual_charge_prefs",
            Self::InvalidAdvancedPowerSettings => "invalid_advanced_power_settings",
            Self::InvalidWifiSsid => "invalid_wifi_ssid",
            Self::InvalidWifiPsk => "invalid_wifi_psk",
            Self::FrameTooLarge => "frame_too_large",
        }
    }

    pub const fn message(self) -> &'static str {
        match self {
            Self::InvalidJson => "frame is not supported JSON",
            Self::MissingField => "required frame field is missing",
            Self::InvalidRequestId => "request_id must be 1..32 printable characters",
            Self::UnsupportedType => "frame type is not supported by this endpoint",
            Self::UnsupportedOperation => "request operation is not supported",
            Self::UnsafeOperation => "requested operation is outside the safe USB control surface",
            Self::InvalidLogLevel => "log level must be error, warn, info, debug, or trace",
            Self::InvalidManualChargePrefs => "manual charge prefs are outside the safe set",
            Self::InvalidAdvancedPowerSettings => {
                "advanced power settings are outside the supported range or ordering"
            }
            Self::InvalidWifiSsid => "WiFi SSID must be 1..32 non-control bytes",
            Self::InvalidWifiPsk => "WiFi PSK must be 8..63 non-control bytes",
            Self::FrameTooLarge => "CDC frame exceeds line buffer capacity",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiConfigStorageError {
    InvalidCrc,
    InvalidMagic,
    UnsupportedVersion,
    InvalidSecret,
}

pub struct UsbCdcLineBuffer<const N: usize> {
    buf: Vec<u8, N>,
}

impl<const N: usize> UsbCdcLineBuffer<N> {
    pub const fn new() -> Self {
        Self { buf: Vec::new() }
    }

    pub fn push_byte(&mut self, byte: u8) -> Result<Option<String<N>>, UsbCdcProtocolError> {
        match byte {
            b'\n' => {
                if self.buf.is_empty() {
                    return Ok(None);
                }
                let mut frame = String::<N>::new();
                let text = match core::str::from_utf8(self.buf.as_slice()) {
                    Ok(text) => text,
                    Err(_) => {
                        self.buf.clear();
                        return Err(UsbCdcProtocolError::InvalidJson);
                    }
                };
                frame
                    .push_str(text)
                    .map_err(|_| UsbCdcProtocolError::FrameTooLarge)?;
                self.buf.clear();
                Ok(Some(frame))
            }
            b'\r' => Ok(None),
            byte => {
                self.buf
                    .push(byte)
                    .map_err(|_| UsbCdcProtocolError::FrameTooLarge)?;
                Ok(None)
            }
        }
    }
}

impl<const N: usize> Default for UsbCdcLineBuffer<N> {
    fn default() -> Self {
        Self::new()
    }
}

pub fn parse_frame(line: &str) -> Result<UsbCdcFrame, UsbCdcProtocolError> {
    let frame_type =
        json_string_field::<32>(line, "type")?.ok_or(UsbCdcProtocolError::MissingField)?;
    match frame_type.as_str() {
        "hello" => Ok(UsbCdcFrame::Hello {
            request_id: json_string_field::<32>(line, "request_id")?,
        }),
        "request" => {
            let request_id = parse_request_id(line)?;
            let op =
                json_string_field::<32>(line, "op")?.ok_or(UsbCdcProtocolError::MissingField)?;
            Ok(UsbCdcFrame::Request {
                request_id,
                op: parse_request_op(line, op.as_str())?,
            })
        }
        "wifi_config" => {
            let request_id = parse_request_id(line)?;
            let op =
                json_string_field::<16>(line, "op")?.ok_or(UsbCdcProtocolError::MissingField)?;
            let command = match op.as_str() {
                "set" => {
                    let ssid = json_string_field::<WIFI_SSID_MAX_LEN>(line, "ssid")?
                        .ok_or(UsbCdcProtocolError::MissingField)?;
                    let psk = json_string_field::<64>(line, "psk")?
                        .ok_or(UsbCdcProtocolError::MissingField)?;
                    WifiConfigCommand::Set(WifiConfigSecret::new(ssid.as_str(), psk.as_str())?)
                }
                "clear" => WifiConfigCommand::Clear,
                _ => return Err(UsbCdcProtocolError::UnsupportedOperation),
            };
            Ok(UsbCdcFrame::WifiConfig {
                request_id,
                command,
            })
        }
        _ => Err(UsbCdcProtocolError::UnsupportedType),
    }
}

pub fn parse_http_log_level_request(body: &str) -> Result<LogLevel, UsbCdcProtocolError> {
    let level = json_string_field::<16>(body, "level")?.ok_or(UsbCdcProtocolError::MissingField)?;
    parse_log_level(level.as_str())
}

pub fn parse_http_manual_charge_request(
    body: &str,
) -> Result<ManualChargePrefsCommand, UsbCdcProtocolError> {
    parse_manual_charge_prefs(body)
}

pub fn parse_http_advanced_power_request(
    body: &str,
) -> Result<AdvancedPowerSettingsSnapshot, UsbCdcProtocolError> {
    parse_advanced_power_settings(body)
}

pub fn parse_http_wifi_config_request(body: &str) -> Result<WifiConfigSecret, UsbCdcProtocolError> {
    let ssid = json_string_field::<WIFI_SSID_MAX_LEN>(body, "ssid")?
        .ok_or(UsbCdcProtocolError::MissingField)?;
    let psk = json_string_field::<64>(body, "psk")?.ok_or(UsbCdcProtocolError::MissingField)?;
    WifiConfigSecret::new(ssid.as_str(), psk.as_str())
}

pub fn parse_http_reset_request(body: &str) -> Result<(), UsbCdcProtocolError> {
    let confirm =
        json_string_field::<16>(body, "confirm")?.ok_or(UsbCdcProtocolError::MissingField)?;
    if confirm.as_str() == "reset" {
        Ok(())
    } else {
        Err(UsbCdcProtocolError::UnsafeOperation)
    }
}

pub fn request_id_hint(line: &str) -> Option<String<32>> {
    parse_request_id(line).ok()
}

pub fn render_hello_json<const N: usize>(
    buf: &mut String<N>,
    request_id: Option<&str>,
    identity_json: &str,
) {
    buf.clear();
    let _ = buf.push_str(r#"{"type":"hello","#);
    if let Some(request_id) = request_id {
        let _ = buf.push_str(r#""request_id":""#);
        write_json_string_escaped(buf, request_id);
        let _ = buf.push_str(r#"","#);
    }
    let _ = buf.push_str(r#""protocol":""#);
    let _ = buf.push_str(PROTOCOL_NAME);
    let _ = buf.push_str(
        r#"","framing":"jsonl","capabilities":{"status":true,"structured_logs":true,"settings":true,"wifi_config":true,"psk_echo":false},"identity":"#,
    );
    let _ = buf.push_str(identity_json);
    let _ = buf.push('}');
}

pub fn render_status_frame_json<const N: usize>(buf: &mut String<N>, status_json: &str) {
    buf.clear();
    let _ = buf.push_str(r#"{"type":"status","status":"#);
    let _ = buf.push_str(status_json);
    let _ = buf.push('}');
}

pub fn render_response_json<const N: usize>(
    buf: &mut String<N>,
    request_id: &str,
    result_json: &str,
) {
    buf.clear();
    let _ = buf.push_str(r#"{"type":"response","request_id":""#);
    write_json_string_escaped(buf, request_id);
    let _ = buf.push_str(r#"","ok":true,"result":"#);
    let _ = buf.push_str(result_json);
    let _ = buf.push('}');
}

pub fn render_wifi_config_ack_json<const N: usize>(
    buf: &mut String<N>,
    request_id: &str,
    configured: bool,
    ssid: Option<&str>,
) {
    // A valid 32-byte SSID can double in size when quotes/backslashes are escaped.
    let mut result = String::<WIFI_ACK_RESULT_CAPACITY>::new();
    let _ = result.push_str(r#"{"wifi_configured":"#);
    let _ = result.push_str(if configured { "true" } else { "false" });
    let _ = result.push_str(r#","psk_present":"#);
    let _ = result.push_str(if configured { "true" } else { "false" });
    let _ = result.push_str(r#","psk_echoed":false"#);
    if let Some(ssid) = ssid {
        let _ = result.push_str(r#","ssid":""#);
        write_json_string_escaped(&mut result, ssid);
        let _ = result.push('"');
    }
    let _ = result.push('}');
    render_response_json(buf, request_id, result.as_str());
}

pub fn render_log_json<const N: usize>(
    buf: &mut String<N>,
    level: LogLevel,
    target: &str,
    message: &str,
) {
    buf.clear();
    let _ = buf.push_str(r#"{"type":"log","level":""#);
    let _ = buf.push_str(level.as_str());
    let _ = buf.push_str(r#"","target":""#);
    write_json_string_escaped(buf, target);
    let _ = buf.push_str(r#"","message":""#);
    write_json_string_escaped(buf, message);
    let _ = buf.push_str(r#""}"#);
}

pub fn render_protocol_error_json<const N: usize>(
    buf: &mut String<N>,
    request_id: Option<&str>,
    error: UsbCdcProtocolError,
) {
    render_error_json(buf, request_id, error.code(), error.message(), false);
}

pub fn render_error_json<const N: usize>(
    buf: &mut String<N>,
    request_id: Option<&str>,
    code: &str,
    message: &str,
    retryable: bool,
) {
    buf.clear();
    let _ = buf.push_str(r#"{"type":"error","#);
    if let Some(request_id) = request_id {
        let _ = buf.push_str(r#""request_id":""#);
        write_json_string_escaped(buf, request_id);
        let _ = buf.push_str(r#"","#);
    }
    let _ = buf.push_str(r#""error":{"code":""#);
    write_json_string_escaped(buf, code);
    let _ = buf.push_str(r#"","message":""#);
    write_json_string_escaped(buf, message);
    let _ = write!(buf, r#"","retryable":{},"details":null}}"#, retryable);
}

pub fn encode_wifi_config_record(
    config: Option<&WifiConfigSecret>,
) -> [u8; WIFI_CONFIG_RECORD_LEN] {
    let mut bytes = [0u8; WIFI_CONFIG_RECORD_LEN];
    let Some(config) = config else {
        return bytes;
    };
    bytes[0..4].copy_from_slice(&WIFI_CONFIG_MAGIC);
    bytes[4] = WIFI_CONFIG_VERSION;
    bytes[5] = 1;
    bytes[6] = config.ssid.len() as u8;
    bytes[7] = config.psk.len() as u8;
    bytes[WIFI_CONFIG_SSID_OFFSET..WIFI_CONFIG_SSID_OFFSET + config.ssid.len()]
        .copy_from_slice(config.ssid.as_bytes());
    bytes[WIFI_CONFIG_PSK_OFFSET..WIFI_CONFIG_PSK_OFFSET + config.psk.len()]
        .copy_from_slice(config.psk.as_bytes());
    bytes[WIFI_CONFIG_CRC_INDEX] = storage_crc8(&bytes[..WIFI_CONFIG_CRC_INDEX]);
    bytes
}

pub fn decode_wifi_config_record(
    bytes: &[u8; WIFI_CONFIG_RECORD_LEN],
) -> Result<Option<WifiConfigSecret>, WifiConfigStorageError> {
    if bytes.iter().all(|byte| *byte == 0 || *byte == 0xff) {
        return Ok(None);
    }
    if bytes[0..4] != WIFI_CONFIG_MAGIC {
        return Err(WifiConfigStorageError::InvalidMagic);
    }
    if bytes[4] != WIFI_CONFIG_VERSION {
        return Err(WifiConfigStorageError::UnsupportedVersion);
    }
    if bytes[WIFI_CONFIG_CRC_INDEX] != storage_crc8(&bytes[..WIFI_CONFIG_CRC_INDEX]) {
        return Err(WifiConfigStorageError::InvalidCrc);
    }
    if bytes[5] == 0 {
        return Ok(None);
    }
    let ssid_len = bytes[6] as usize;
    let psk_len = bytes[7] as usize;
    if ssid_len == 0 || ssid_len > WIFI_SSID_MAX_LEN || psk_len < 8 || psk_len > WIFI_PSK_MAX_LEN {
        return Err(WifiConfigStorageError::InvalidSecret);
    }
    let ssid =
        core::str::from_utf8(&bytes[WIFI_CONFIG_SSID_OFFSET..WIFI_CONFIG_SSID_OFFSET + ssid_len])
            .map_err(|_| WifiConfigStorageError::InvalidSecret)?;
    let psk =
        core::str::from_utf8(&bytes[WIFI_CONFIG_PSK_OFFSET..WIFI_CONFIG_PSK_OFFSET + psk_len])
            .map_err(|_| WifiConfigStorageError::InvalidSecret)?;
    WifiConfigSecret::new(ssid, psk)
        .map(Some)
        .map_err(|_| WifiConfigStorageError::InvalidSecret)
}

fn parse_request_id(line: &str) -> Result<String<32>, UsbCdcProtocolError> {
    let request_id =
        json_string_field::<32>(line, "request_id")?.ok_or(UsbCdcProtocolError::MissingField)?;
    if request_id.is_empty()
        || request_id
            .as_bytes()
            .iter()
            .any(|byte| *byte < 0x21 || *byte > 0x7e)
    {
        return Err(UsbCdcProtocolError::InvalidRequestId);
    }
    Ok(request_id)
}

fn parse_request_op(line: &str, op: &str) -> Result<UsbCdcRequest, UsbCdcProtocolError> {
    match op {
        "get_identity" => Ok(UsbCdcRequest::GetIdentity),
        "get_status" => Ok(UsbCdcRequest::GetStatus),
        "get_settings" => Ok(UsbCdcRequest::GetSettings),
        "get_power_diag" => Ok(UsbCdcRequest::GetPowerDiag),
        "set_log_level" => {
            let level =
                json_string_field::<16>(line, "level")?.ok_or(UsbCdcProtocolError::MissingField)?;
            Ok(UsbCdcRequest::SetLogLevel(parse_log_level(level.as_str())?))
        }
        "set_manual_charge_prefs" => Ok(UsbCdcRequest::SetManualChargePrefs(
            parse_manual_charge_prefs(line)?,
        )),
        "set_advanced_power" => Ok(UsbCdcRequest::SetAdvancedPower(
            parse_advanced_power_settings(line)?,
        )),
        "reset_advanced_power" => Ok(UsbCdcRequest::ResetAdvancedPower),
        "output_enable" | "output_disable" | "clear_fault" | "start_charge" | "stop_charge" => {
            Err(UsbCdcProtocolError::UnsafeOperation)
        }
        _ => Err(UsbCdcProtocolError::UnsupportedOperation),
    }
}

fn parse_log_level(level: &str) -> Result<LogLevel, UsbCdcProtocolError> {
    match level {
        "error" => Ok(LogLevel::Error),
        "warn" => Ok(LogLevel::Warn),
        "info" => Ok(LogLevel::Info),
        "debug" => Ok(LogLevel::Debug),
        "trace" => Ok(LogLevel::Trace),
        _ => Err(UsbCdcProtocolError::InvalidLogLevel),
    }
}

fn parse_manual_charge_prefs(line: &str) -> Result<ManualChargePrefsCommand, UsbCdcProtocolError> {
    let target = match json_string_field::<16>(line, "target")?
        .ok_or(UsbCdcProtocolError::MissingField)?
        .as_str()
    {
        "pack_3v7" => ManualChargeTarget::Pack3V7,
        "rsoc_80" => ManualChargeTarget::Rsoc80,
        "full_100" => ManualChargeTarget::Full100,
        _ => return Err(UsbCdcProtocolError::InvalidManualChargePrefs),
    };
    let speed = match json_string_field::<16>(line, "speed")?
        .ok_or(UsbCdcProtocolError::MissingField)?
        .as_str()
    {
        "ma_100" => ManualChargeSpeed::Ma100,
        "ma_500" => ManualChargeSpeed::Ma500,
        "ma_1000" => ManualChargeSpeed::Ma1000,
        _ => return Err(UsbCdcProtocolError::InvalidManualChargePrefs),
    };
    let timer_limit =
        match json_u8_field(line, "timer_h")?.ok_or(UsbCdcProtocolError::MissingField)? {
            1 => ManualChargeTimerLimit::H1,
            2 => ManualChargeTimerLimit::H2,
            6 => ManualChargeTimerLimit::H6,
            _ => return Err(UsbCdcProtocolError::InvalidManualChargePrefs),
        };
    Ok(ManualChargePrefsCommand {
        target,
        speed,
        timer_limit,
    })
}

fn parse_advanced_power_settings(
    line: &str,
) -> Result<AdvancedPowerSettingsSnapshot, UsbCdcProtocolError> {
    let settings = AdvancedPowerSettingsSnapshot {
        standby_drop_mv: json_u16_field(line, "standby_drop_mv")?
            .ok_or(UsbCdcProtocolError::MissingField)?,
        assist_low_drop_mv: json_u16_field(line, "assist_low_drop_mv")?
            .ok_or(UsbCdcProtocolError::MissingField)?,
        assist_enter_delta_ma: json_i16_field(line, "assist_enter_delta_ma")?
            .ok_or(UsbCdcProtocolError::MissingField)?,
        assist_exit_delta_ma: json_i16_field(line, "assist_exit_delta_ma")?
            .ok_or(UsbCdcProtocolError::MissingField)?,
        assist_required_samples: json_u8_field(line, "assist_required_samples")?
            .ok_or(UsbCdcProtocolError::MissingField)?,
        assist_ramp_step_mv: json_u16_field(line, "assist_ramp_step_mv")?
            .ok_or(UsbCdcProtocolError::MissingField)?,
        assist_ramp_interval_ms: json_u16_field(line, "assist_ramp_interval_ms")?
            .ok_or(UsbCdcProtocolError::MissingField)?,
        rated_enter_delta_ma: json_i16_field(line, "rated_enter_delta_ma")?
            .ok_or(UsbCdcProtocolError::MissingField)?,
        rated_exit_delta_ma: json_i16_field(line, "rated_exit_delta_ma")?
            .ok_or(UsbCdcProtocolError::MissingField)?,
        vin_drop_threshold_pct: json_u8_field(line, "vin_drop_threshold_pct")?
            .ok_or(UsbCdcProtocolError::MissingField)?,
        required_samples: json_u8_field(line, "required_samples")?
            .ok_or(UsbCdcProtocolError::MissingField)?,
    };
    validate_advanced_power_settings(settings).map_err(advanced_power_validation_protocol_error)?;
    Ok(settings)
}

fn advanced_power_validation_protocol_error(
    _err: AdvancedPowerValidationError,
) -> UsbCdcProtocolError {
    UsbCdcProtocolError::InvalidAdvancedPowerSettings
}

fn validate_wifi_ssid(ssid: &str) -> Result<(), UsbCdcProtocolError> {
    if ssid.is_empty()
        || ssid.len() > WIFI_SSID_MAX_LEN
        || ssid.as_bytes().iter().any(|byte| *byte < 0x20)
    {
        Err(UsbCdcProtocolError::InvalidWifiSsid)
    } else {
        Ok(())
    }
}

fn validate_wifi_psk(psk: &str) -> Result<(), UsbCdcProtocolError> {
    if psk.len() < 8
        || psk.len() > WIFI_PSK_MAX_LEN
        || psk.as_bytes().iter().any(|byte| *byte < 0x20)
    {
        Err(UsbCdcProtocolError::InvalidWifiPsk)
    } else {
        Ok(())
    }
}

fn json_string_field<const N: usize>(
    line: &str,
    key: &str,
) -> Result<Option<String<N>>, UsbCdcProtocolError> {
    let Some(value_offset) = json_value_offset(line, key) else {
        return Ok(None);
    };
    let bytes = line.as_bytes();
    if bytes.get(value_offset) != Some(&b'"') {
        return Err(UsbCdcProtocolError::InvalidJson);
    }
    let mut idx = value_offset + 1;
    let mut segment_start = idx;
    let mut out = String::<N>::new();
    while idx < bytes.len() {
        match bytes[idx] {
            b'"' => {
                push_json_string_segment(&mut out, &bytes[segment_start..idx])?;
                return Ok(Some(out));
            }
            b'\\' => {
                push_json_string_segment(&mut out, &bytes[segment_start..idx])?;
                idx += 1;
                if idx >= bytes.len() {
                    return Err(UsbCdcProtocolError::InvalidJson);
                }
                let ch = match bytes[idx] {
                    b'"' => '"',
                    b'\\' => '\\',
                    b'/' => '/',
                    b'b' => '\u{0008}',
                    b'f' => '\u{000c}',
                    b'n' => '\n',
                    b'r' => '\r',
                    b't' => '\t',
                    b'u' => return Err(UsbCdcProtocolError::InvalidJson),
                    _ => return Err(UsbCdcProtocolError::InvalidJson),
                };
                out.push(ch).map_err(|_| UsbCdcProtocolError::InvalidJson)?;
                idx += 1;
                segment_start = idx;
                continue;
            }
            byte if byte < 0x20 => return Err(UsbCdcProtocolError::InvalidJson),
            _ => {}
        }
        idx += 1;
    }
    Err(UsbCdcProtocolError::InvalidJson)
}

fn push_json_string_segment<const N: usize>(
    out: &mut String<N>,
    segment: &[u8],
) -> Result<(), UsbCdcProtocolError> {
    if segment.is_empty() {
        return Ok(());
    }
    let text = core::str::from_utf8(segment).map_err(|_| UsbCdcProtocolError::InvalidJson)?;
    out.push_str(text)
        .map_err(|_| UsbCdcProtocolError::InvalidJson)
}

fn json_u8_field(line: &str, key: &str) -> Result<Option<u8>, UsbCdcProtocolError> {
    let Some(mut idx) = json_value_offset(line, key) else {
        return Ok(None);
    };
    let bytes = line.as_bytes();
    let start = idx;
    while idx < bytes.len() && bytes[idx].is_ascii_digit() {
        idx += 1;
    }
    if idx == start {
        return Err(UsbCdcProtocolError::InvalidJson);
    }
    line[start..idx]
        .parse::<u8>()
        .map(Some)
        .map_err(|_| UsbCdcProtocolError::InvalidJson)
}

fn json_u16_field(line: &str, key: &str) -> Result<Option<u16>, UsbCdcProtocolError> {
    let Some(mut idx) = json_value_offset(line, key) else {
        return Ok(None);
    };
    let bytes = line.as_bytes();
    let start = idx;
    while idx < bytes.len() && bytes[idx].is_ascii_digit() {
        idx += 1;
    }
    if idx == start {
        return Err(UsbCdcProtocolError::InvalidJson);
    }
    line[start..idx]
        .parse::<u16>()
        .map(Some)
        .map_err(|_| UsbCdcProtocolError::InvalidJson)
}

fn json_i16_field(line: &str, key: &str) -> Result<Option<i16>, UsbCdcProtocolError> {
    let Some(mut idx) = json_value_offset(line, key) else {
        return Ok(None);
    };
    let bytes = line.as_bytes();
    let start = idx;
    if bytes.get(idx) == Some(&b'-') {
        idx += 1;
    }
    let digits_start = idx;
    while idx < bytes.len() && bytes[idx].is_ascii_digit() {
        idx += 1;
    }
    if idx == digits_start {
        return Err(UsbCdcProtocolError::InvalidJson);
    }
    line[start..idx]
        .parse::<i16>()
        .map(Some)
        .map_err(|_| UsbCdcProtocolError::InvalidJson)
}

fn json_value_offset(line: &str, key: &str) -> Option<usize> {
    let mut pattern = String::<40>::new();
    let _ = pattern.push('"');
    let _ = pattern.push_str(key);
    let _ = pattern.push('"');
    let offset = line.find(pattern.as_str())?;
    let bytes = line.as_bytes();
    let mut idx = offset + pattern.len();
    while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
        idx += 1;
    }
    if bytes.get(idx) != Some(&b':') {
        return None;
    }
    idx += 1;
    while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
        idx += 1;
    }
    Some(idx)
}

const fn storage_crc8(bytes: &[u8]) -> u8 {
    let mut crc = 0u8;
    let mut idx = 0;
    while idx < bytes.len() {
        crc ^= bytes[idx];
        let mut bit = 0;
        while bit < 8 {
            crc = if (crc & 0x80) != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
            bit += 1;
        }
        idx += 1;
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net_contract::render_power_diag_json;
    use crate::net_types::{
        PowerDiagBmsSnapshot, PowerDiagChargerSnapshot, PowerDiagInputSnapshot,
        PowerDiagPolicySnapshot, PowerDiagSnapshot,
    };

    #[test]
    fn log_level_filter_allows_equal_and_more_severe_entries() {
        assert!(LogLevel::Warn.allows(LogLevel::Error));
        assert!(LogLevel::Warn.allows(LogLevel::Warn));
        assert!(!LogLevel::Warn.allows(LogLevel::Info));
        assert!(LogLevel::Trace.allows(LogLevel::Debug));
        assert!(!LogLevel::Error.allows(LogLevel::Warn));
    }

    #[test]
    fn parses_wifi_config_without_psk_echo_paths() {
        let frame = parse_frame(
            r#"{"type":"wifi_config","request_id":"req-1","op":"set","ssid":"LabNet","psk":"correct horse"}"#,
        )
        .unwrap();
        assert_eq!(
            frame,
            UsbCdcFrame::WifiConfig {
                request_id: String::try_from("req-1").unwrap(),
                command: WifiConfigCommand::Set(
                    WifiConfigSecret::new("LabNet", "correct horse").unwrap()
                )
            }
        );
    }

    #[test]
    fn parses_wifi_config_utf8_ssid_without_mojibake() {
        const SSID: &str = "\u{5b9e}\u{9a8c}\u{5ba4}";
        let mut line = String::<128>::new();
        write!(
            line,
            r#"{{"type":"wifi_config","request_id":"req-u8","op":"set","ssid":"{}","psk":"correct horse"}}"#,
            SSID
        )
        .unwrap();

        let frame = parse_frame(line.as_str()).unwrap();
        assert_eq!(
            frame,
            UsbCdcFrame::WifiConfig {
                request_id: String::try_from("req-u8").unwrap(),
                command: WifiConfigCommand::Set(
                    WifiConfigSecret::new(SSID, "correct horse").unwrap()
                )
            }
        );
    }

    #[test]
    fn wifi_config_ack_handles_worst_case_escaped_ssid() {
        let mut ssid = String::<WIFI_SSID_MAX_LEN>::new();
        for _ in 0..WIFI_SSID_MAX_LEN {
            ssid.push('"').unwrap();
        }
        let mut frame = String::<512>::new();
        render_wifi_config_ack_json(&mut frame, "req-escape", true, Some(ssid.as_str()));

        let mut expected_ssid = String::<96>::new();
        expected_ssid.push_str(r#""ssid":""#).unwrap();
        for _ in 0..WIFI_SSID_MAX_LEN {
            expected_ssid.push_str(r#"\""#).unwrap();
        }
        expected_ssid.push('"').unwrap();

        assert!(frame.as_str().contains(expected_ssid.as_str()));
        assert!(frame.as_str().ends_with("}}"));
    }

    #[test]
    fn rejects_unsafe_power_operations() {
        let err = parse_frame(r#"{"type":"request","request_id":"req-2","op":"output_enable"}"#)
            .unwrap_err();
        assert_eq!(err, UsbCdcProtocolError::UnsafeOperation);
    }

    #[test]
    fn parses_power_diag_request_over_usb_cdc() {
        let frame =
            parse_frame(r#"{"type":"request","request_id":"req-diag","op":"get_power_diag"}"#)
                .unwrap();
        assert_eq!(
            frame,
            UsbCdcFrame::Request {
                request_id: String::try_from("req-diag").unwrap(),
                op: UsbCdcRequest::GetPowerDiag,
            }
        );
    }

    #[test]
    fn power_diag_response_supports_expanded_payload() {
        let diag = PowerDiagSnapshot {
            input: PowerDiagInputSnapshot {
                source: "dcin",
                mains_present: Some(true),
                input_vbus_mv: Some(12_340),
                input_ibus_ma: Some(1_234),
                vin_vbus_mv: Some(12_180),
                vin_iin_ma: Some(980),
                tps_total_iout_ma: Some(128),
                tps_limit_threshold_ma: Some(100),
                pressure_state: "limited",
                pressure_score_pct: Some(92),
                pressure_reason: Some("tps_output_current"),
                vin_baseline_mv: Some(12_300),
                vin_drop_mv: Some(120),
                assist_power_stage: Some("assist_rated"),
                assist_target_vout_mv: Some(12_000),
                usb_pd_attached: false,
                usb_pd_charge_ready: false,
                usb_pd_vbus_present: Some(true),
                usb_pd_unsafe_source_latched: false,
                usb_pd_contract_kind: Some("fixed"),
                usb_pd_contract_mv: Some(12_000),
                usb_pd_contract_ma: Some(1_000),
                usb_pd_vac1_mv: Some(12_340),
                usb_pd_vsys_mv: Some(12_210),
            },
            charger: PowerDiagChargerSnapshot {
                poll_valid: true,
                enabled: true,
                ce_low: false,
                ilim_hiz_brk_low: false,
                allow_charge: true,
                normal_allow_charge: true,
                force_allow_charge: false,
                can_enable: true,
                usb_pd_charge_gate_ready: true,
                input_present: true,
                vbus_present: true,
                ac1_present: true,
                ac2_present: false,
                pg: true,
                vbat_present: true,
                adc_enabled: true,
                adc_done: true,
                adc_ready: true,
                ibus_adc_ma: Some(980),
                ibat_adc_ma: Some(120),
                vbus_adc_mv: Some(12_180),
                vbat_adc_mv: Some(12_010),
                vsys_adc_mv: Some(12_040),
                vac1_adc_mv: Some(12_340),
                vac2_adc_mv: None,
                vreg_mv: Some(12_600),
                ichg_ma: Some(100),
                vindpm_mv: Some(11_700),
                iindpm_ma: Some(1_000),
                vbat_lowv_pct_x10: Some(320),
                iprechg_ma: Some(100),
                iterm_ma: Some(150),
                chg_stat: "charge",
                vbus_stat: "pg",
                ico_stat: "ok",
                treg: false,
                dpdm: false,
                wd: false,
                poorsrc: false,
                vindpm: false,
                iindpm: false,
                ts_cold: false,
                ts_hot: false,
                st0: Some(0),
                st1: Some(1),
                st2: Some(2),
                st3: Some(3),
                st4: Some(4),
                fault0: Some(0),
                fault1: Some(0),
                ctrl0: Some(0),
                ctrl3: Some(0x18),
                ctrl4: Some(0x19),
                acdrv_path: "ac1",
                term_ctrl: Some(0x1234),
            },
            policy: PowerDiagPolicySnapshot {
                state: Some("charging"),
                status: "charging",
                notice: "active",
                input_source: "dcin",
                start_reason: Some("manual_charge"),
                full_reason: Some("battery_full"),
                output_block_reason: Some("none"),
                recovery_stage: Some("hold"),
                target_ichg_ma: Some(500),
                adaptive_cap_ichg_ma: Some(100),
                effective_target_ichg_ma: Some(100),
                limit_active: true,
                limit_reason: Some("pressure_tps_output_current"),
                limit_detail: Some("tps_output_current_over_limit"),
                detail_status: Some("COOLDOWN"),
                pressure_state: "limited",
                pressure_reason: Some("tps_output_current"),
                pressure_score_pct: Some(92),
                vin_baseline_mv: Some(12_300),
                vin_drop_mv: Some(120),
                tps_total_iout_ma: Some(128),
                tps_limit_threshold_ma: Some(100),
                output_power_w10: Some(1_560),
                charge_latched: false,
                full_latched: false,
                dc_derated: true,
                output_blocked: false,
                manual_active: true,
                manual_stop_inhibit: false,
            },
            bms: PowerDiagBmsSnapshot {
                addr: Some(11),
                state: "ready",
                pack_mv: Some(12_000),
                current_ma: Some(-120),
                soc_pct: Some(87),
                cell_min_mv: Some(3_980),
                cell_max_mv: Some(4_005),
                no_battery: Some(false),
                discharge_ready: Some(true),
                charge_ready: Some(true),
                full: Some(false),
                issue_detail: Some("none"),
                rca_alarm: Some(false),
                safety_status: Some(0),
                pf_status: Some(0),
                manufacturing_status: Some(0),
                gauging_status: Some(0),
                op_status: Some(0),
                xchg: Some(true),
                chg_fet: Some(true),
                dsg_fet: Some(true),
                pchg_fet: Some(false),
                cuv: Some(false),
                cuvc: Some(false),
                cuv_recovery_mv: Some(3_000),
                cuv_recov_chg: Some(true),
                fet_en: Some(true),
                chg_en: Some(true),
                dsg_en: Some(true),
                charging_inhibit: Some(false),
                charging_suspend: Some(false),
                charging_hv: Some(false),
                current_at_eoc_ma: Some(150),
            },
        };

        let mut body = String::<WEB_SERIAL_POWER_DIAG_BODY_CAP>::new();
        render_power_diag_json(&mut body, diag);
        assert!(body
            .as_str()
            .contains("\"pressure_reason\":\"tps_output_current\""));
        assert!(body.as_str().contains("\"tps_total_iout_ma\":128"));
        assert!(body.len() < WEB_SERIAL_POWER_DIAG_BODY_CAP);

        let mut frame = String::<WEB_SERIAL_POWER_DIAG_FRAME_CAP>::new();
        render_response_json(&mut frame, "req-diag", body.as_str());
        assert!(frame.as_str().contains("\"type\":\"response\""));
        assert!(frame.as_str().contains("\"request_id\":\"req-diag\""));
        assert!(frame
            .as_str()
            .contains("\"pressure_reason\":\"tps_output_current\""));
        assert!(frame.len() < WEB_SERIAL_POWER_DIAG_FRAME_CAP);
    }

    #[test]
    fn keeps_request_id_available_after_validation_errors() {
        let line = r#"{"type":"request","request_id":"req-err","op":"output_enable"}"#;
        assert_eq!(
            parse_frame(line).unwrap_err(),
            UsbCdcProtocolError::UnsafeOperation
        );
        assert_eq!(request_id_hint(line).unwrap().as_str(), "req-err");
    }

    #[test]
    fn validates_safe_manual_charge_preferences() {
        let frame = parse_frame(
            r#"{"type":"request","request_id":"req-3","op":"set_manual_charge_prefs","target":"rsoc_80","speed":"ma_500","timer_h":2}"#,
        )
        .unwrap();
        assert_eq!(
            frame,
            UsbCdcFrame::Request {
                request_id: String::try_from("req-3").unwrap(),
                op: UsbCdcRequest::SetManualChargePrefs(ManualChargePrefsCommand {
                    target: ManualChargeTarget::Rsoc80,
                    speed: ManualChargeSpeed::Ma500,
                    timer_limit: ManualChargeTimerLimit::H2,
                })
            }
        );
    }

    #[test]
    fn reset_http_request_requires_explicit_confirmation() {
        assert!(parse_http_reset_request(r#"{"confirm":"reset"}"#).is_ok());
        assert_eq!(
            parse_http_reset_request(r#"{"confirm":"reboot"}"#).unwrap_err(),
            UsbCdcProtocolError::UnsafeOperation
        );
    }

    #[test]
    fn wifi_config_storage_round_trips_and_detects_crc_errors() {
        let config = WifiConfigSecret::new("LabNet", "correct horse").unwrap();
        let mut bytes = encode_wifi_config_record(Some(&config));
        assert_eq!(decode_wifi_config_record(&bytes).unwrap(), Some(config));
        bytes[WIFI_CONFIG_SSID_OFFSET] ^= 0x01;
        assert_eq!(
            decode_wifi_config_record(&bytes).unwrap_err(),
            WifiConfigStorageError::InvalidCrc
        );
    }

    #[test]
    fn wifi_config_clear_record_is_valid_and_empty() {
        let bytes = encode_wifi_config_record(None);
        assert_eq!(decode_wifi_config_record(&bytes).unwrap(), None);
    }

    #[test]
    fn line_buffer_returns_complete_jsonl_frame() {
        let mut buffer = UsbCdcLineBuffer::<64>::new();
        assert_eq!(buffer.push_byte(b'{').unwrap(), None);
        assert_eq!(buffer.push_byte(b'}').unwrap(), None);
        assert_eq!(buffer.push_byte(b'\n').unwrap().unwrap().as_str(), "{}");
    }

    #[test]
    fn line_buffer_preserves_utf8_bytes() {
        const LINE: &str =
            "{\"type\":\"wifi_config\",\"request_id\":\"req-u8\",\"op\":\"set\",\"ssid\":\"\u{5b9e}\u{9a8c}\",\"psk\":\"correct horse\"}";
        let mut buffer = UsbCdcLineBuffer::<160>::new();
        for byte in LINE.as_bytes() {
            assert_eq!(buffer.push_byte(*byte).unwrap(), None);
        }
        let frame = buffer.push_byte(b'\n').unwrap().unwrap();
        assert_eq!(frame.as_str(), LINE);
        assert!(matches!(
            parse_frame(frame.as_str()).unwrap(),
            UsbCdcFrame::WifiConfig { .. }
        ));
    }
}
