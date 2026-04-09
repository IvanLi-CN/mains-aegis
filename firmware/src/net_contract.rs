use core::fmt::Write as _;

use heapless::String;

use crate::{
    mdns_wire::DeviceIdentity,
    net_types::{format_ipv4, UpsStatusSnapshot, WifiSnapshot, API_VERSION},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildInfo {
    pub package_version: &'static str,
    pub build_profile: &'static str,
    pub build_id: &'static str,
    pub git_sha: &'static str,
    pub src_hash: &'static str,
    pub git_dirty: &'static str,
}

pub fn accepts_event_stream(header_value: &str) -> bool {
    header_value
        .split(',')
        .any(|part| part.trim().eq_ignore_ascii_case("text/event-stream"))
}

pub fn is_api_v1_path(path: &str) -> bool {
    path == "/health" || path.starts_with("/api/v1/")
}

pub fn render_ping_json<const N: usize>(buf: &mut String<N>) {
    buf.clear();
    let _ = buf.push_str(r#"{"ok":true}"#);
}

pub fn write_error_body<const N: usize>(
    buf: &mut String<N>,
    code: &str,
    message: &str,
    retryable: bool,
    details_json: Option<&str>,
) {
    buf.clear();
    let _ = buf.push_str("{\"error\":{\"code\":\"");
    write_json_string_escaped(buf, code);
    let _ = buf.push_str("\",\"message\":\"");
    write_json_string_escaped(buf, message);
    let _ = buf.push_str("\",\"retryable\":");
    let _ = buf.push_str(if retryable { "true" } else { "false" });
    if let Some(details_json) = details_json {
        let _ = buf.push_str(",\"details\":");
        let _ = buf.push_str(details_json);
    }
    let _ = buf.push_str("}}");
}

pub fn render_identity_json<const N: usize>(
    buf: &mut String<N>,
    identity: &DeviceIdentity,
    wifi: WifiSnapshot,
    build: BuildInfo,
) {
    buf.clear();
    let _ = buf.push('{');
    json_field_str(buf, "device_id", identity.device_id.as_str(), true);
    json_field_str(buf, "hostname", identity.hostname.as_str(), true);
    json_field_str(buf, "hostname_fqdn", identity.hostname_fqdn.as_str(), true);
    json_field_str(buf, "short_id", identity.short_id.as_str(), true);
    json_field_str(buf, "role", "ups", true);
    let _ = write!(buf, "\"api_version\":\"{}\",", API_VERSION);
    let _ = buf.push_str("\"firmware\":{");
    json_field_str(buf, "package_version", build.package_version, true);
    json_field_str(buf, "build_profile", build.build_profile, true);
    json_field_str(buf, "build_id", build.build_id, true);
    json_field_str(buf, "git_sha", build.git_sha, true);
    json_field_str(buf, "src_hash", build.src_hash, true);
    json_field_str(buf, "git_dirty", build.git_dirty, false);
    let _ = buf.push_str("},\"network\":");
    write_network_object(buf, wifi);
    let _ = buf.push_str(
        ",\"capabilities\":{\"sse\":true,\"mdns\":true,\"dns_sd\":true,\"write_controls\":false}}",
    );
}

pub fn render_network_json<const N: usize>(
    buf: &mut String<N>,
    identity: &DeviceIdentity,
    wifi: WifiSnapshot,
) {
    buf.clear();
    let _ = buf.push('{');
    json_field_str(buf, "device_id", identity.device_id.as_str(), true);
    json_field_str(buf, "hostname", identity.hostname.as_str(), true);
    json_field_str(buf, "hostname_fqdn", identity.hostname_fqdn.as_str(), true);
    let _ = buf.push_str("\"state\":\"");
    let _ = buf.push_str(wifi.state.as_str());
    let _ = buf.push_str("\",");
    write_network_object_fields(buf, wifi, false);
    let _ = buf.push('}');
}

pub fn render_status_json<const N: usize>(buf: &mut String<N>, status: UpsStatusSnapshot) {
    buf.clear();
    let _ = buf.push('{');
    json_field_str(buf, "mode", status.mode, true);
    let _ = buf.push_str("\"input\":{");
    json_field_opt_bool(buf, "mains_present", status.mains_present, true);
    json_field_opt_u16(buf, "input_vbus_mv", status.input_vbus_mv, true);
    json_field_opt_i32(buf, "input_ibus_ma", status.input_ibus_ma, true);
    json_field_opt_u16(buf, "vin_vbus_mv", status.vin_vbus_mv, true);
    json_field_opt_i32(buf, "vin_iin_ma", status.vin_iin_ma, false);
    let _ = buf.push_str("},\"output\":{");
    json_field_str(buf, "requested", status.requested_outputs, true);
    json_field_str(buf, "active", status.active_outputs, true);
    json_field_str(buf, "recoverable", status.recoverable_outputs, true);
    json_field_str(buf, "gate_reason", status.output_gate_reason, true);
    let _ = buf.push_str("\"out_a\":{");
    json_field_str(buf, "state", status.out_a_state, true);
    json_field_opt_bool(buf, "enabled", status.out_a_enabled, true);
    json_field_opt_u16(buf, "vbus_mv", status.out_a_vbus_mv, true);
    json_field_opt_i32(buf, "iout_ma", status.out_a_iout_ma, false);
    let _ = buf.push_str("},\"out_b\":{");
    json_field_str(buf, "state", status.out_b_state, true);
    json_field_opt_bool(buf, "enabled", status.out_b_enabled, true);
    json_field_opt_u16(buf, "vbus_mv", status.out_b_vbus_mv, true);
    json_field_opt_i32(buf, "iout_ma", status.out_b_iout_ma, false);
    let _ = buf.push_str("}},\"charger\":{");
    json_field_str(buf, "state", status.charger_state, true);
    json_field_opt_bool(buf, "allow_charge", status.charger_allow_charge, true);
    json_field_opt_u16(buf, "ichg_ma", status.charger_ichg_ma, true);
    json_field_opt_i16(buf, "ibat_ma", status.charger_ibat_ma, true);
    json_field_opt_bool(buf, "vbat_present", status.charger_vbat_present, false);
    let _ = buf.push_str("},\"battery\":{");
    json_field_str(buf, "state", status.battery_state, true);
    json_field_opt_u16(buf, "pack_mv", status.battery_pack_mv, true);
    json_field_opt_i16(buf, "current_ma", status.battery_current_ma, true);
    json_field_opt_u16(buf, "soc_pct", status.battery_soc_pct, true);
    json_field_opt_bool(buf, "no_battery", status.battery_no_battery, true);
    json_field_opt_bool(buf, "discharge_ready", status.battery_discharge_ready, true);
    json_field_opt_str(buf, "issue_detail", status.battery_issue_detail, true);
    let _ = write!(
        buf,
        "\"recovery_pending\":{}",
        if status.battery_recovery_pending {
            "true"
        } else {
            "false"
        }
    );
    if let Some(last_result) = status.battery_last_result {
        let _ = buf.push_str(",\"last_result\":\"");
        write_json_string_escaped(buf, last_result);
        let _ = buf.push('"');
    }
    let _ = buf.push_str("},\"thermal\":{");
    json_field_str(buf, "tmp_a_state", status.tmp_a_state, true);
    json_field_opt_i16(buf, "tmp_a_c", status.tmp_a_c, true);
    json_field_str(buf, "tmp_b_state", status.tmp_b_state, true);
    json_field_opt_i16(buf, "tmp_b_c", status.tmp_b_c, false);
    let _ = buf.push_str("},\"network\":");
    write_network_summary_object(
        buf,
        status.network.state.as_str(),
        status.network.ipv4,
        status.network.last_error.map(|err| err.as_str()),
    );
    let _ = buf.push('}');
}

pub fn write_sse_event<const N: usize>(
    buf: &mut String<N>,
    event: &str,
    data_json: &str,
    event_id: Option<u32>,
) {
    buf.clear();
    if let Some(event_id) = event_id {
        let _ = write!(buf, "id: {}\n", event_id);
    }
    let _ = buf.push_str("event: ");
    let _ = buf.push_str(event);
    let _ = buf.push('\n');
    let _ = buf.push_str("data: ");
    let _ = buf.push_str(data_json);
    let _ = buf.push_str("\n\n");
}

pub fn write_json_string_escaped<const N: usize>(buf: &mut String<N>, input: &str) {
    for ch in input.chars() {
        match ch {
            '"' => {
                let _ = buf.push_str("\\\"");
            }
            '\\' => {
                let _ = buf.push_str("\\\\");
            }
            '\n' => {
                let _ = buf.push_str("\\n");
            }
            '\r' => {
                let _ = buf.push_str("\\r");
            }
            '\t' => {
                let _ = buf.push_str("\\t");
            }
            c if c < ' ' => {
                let _ = buf.push('?');
            }
            c => {
                let _ = buf.push(c);
            }
        }
    }
}

fn write_network_object<const N: usize>(buf: &mut String<N>, wifi: WifiSnapshot) {
    let _ = buf.push('{');
    write_network_object_fields(buf, wifi, false);
    let _ = buf.push('}');
}

fn write_network_object_fields<const N: usize>(
    buf: &mut String<N>,
    wifi: WifiSnapshot,
    trailing_comma: bool,
) {
    let _ = buf.push_str("\"state\":\"");
    let _ = buf.push_str(wifi.state.as_str());
    let _ = buf.push_str("\",");
    json_field_opt_ipv4(buf, "ipv4", wifi.ipv4, true);
    json_field_opt_ipv4(buf, "gateway", wifi.gateway, true);
    json_field_opt_ipv4(buf, "dns", wifi.dns, true);
    let _ = write!(
        buf,
        "\"is_static\":{}",
        if wifi.is_static { "true" } else { "false" }
    );
    if let Some(last_error) = wifi.last_error {
        let _ = buf.push_str(",\"last_error\":\"");
        let _ = buf.push_str(last_error.as_str());
        let _ = buf.push('"');
    } else {
        let _ = buf.push_str(",\"last_error\":null");
    }
    if let Some(rssi_dbm) = wifi.rssi_dbm {
        let _ = write!(buf, ",\"rssi_dbm\":{}", rssi_dbm);
    } else {
        let _ = buf.push_str(",\"rssi_dbm\":null");
    }
    if trailing_comma {
        let _ = buf.push(',');
    }
}

fn write_network_summary_object<const N: usize>(
    buf: &mut String<N>,
    state: &str,
    ipv4: Option<[u8; 4]>,
    last_error: Option<&str>,
) {
    let _ = buf.push('{');
    json_field_str(buf, "state", state, true);
    json_field_opt_ipv4(buf, "ipv4", ipv4, true);
    json_field_opt_str(buf, "last_error", last_error, false);
    let _ = buf.push('}');
}

fn json_field_str<const N: usize>(
    buf: &mut String<N>,
    key: &str,
    value: &str,
    trailing_comma: bool,
) {
    let _ = buf.push('"');
    let _ = buf.push_str(key);
    let _ = buf.push_str("\":\"");
    write_json_string_escaped(buf, value);
    let _ = buf.push('"');
    if trailing_comma {
        let _ = buf.push(',');
    }
}

fn json_field_opt_str<const N: usize>(
    buf: &mut String<N>,
    key: &str,
    value: Option<&str>,
    trailing_comma: bool,
) {
    let _ = buf.push('"');
    let _ = buf.push_str(key);
    let _ = buf.push_str("\":");
    if let Some(value) = value {
        let _ = buf.push('"');
        write_json_string_escaped(buf, value);
        let _ = buf.push('"');
    } else {
        let _ = buf.push_str("null");
    }
    if trailing_comma {
        let _ = buf.push(',');
    }
}

fn json_field_opt_bool<const N: usize>(
    buf: &mut String<N>,
    key: &str,
    value: Option<bool>,
    trailing_comma: bool,
) {
    let _ = buf.push('"');
    let _ = buf.push_str(key);
    let _ = buf.push_str("\":");
    match value {
        Some(true) => {
            let _ = buf.push_str("true");
        }
        Some(false) => {
            let _ = buf.push_str("false");
        }
        None => {
            let _ = buf.push_str("null");
        }
    }
    if trailing_comma {
        let _ = buf.push(',');
    }
}

fn json_field_opt_u16<const N: usize>(
    buf: &mut String<N>,
    key: &str,
    value: Option<u16>,
    trailing_comma: bool,
) {
    json_field_opt_num(buf, key, value.map(|value| value as i64), trailing_comma);
}

fn json_field_opt_i16<const N: usize>(
    buf: &mut String<N>,
    key: &str,
    value: Option<i16>,
    trailing_comma: bool,
) {
    json_field_opt_num(buf, key, value.map(|value| value as i64), trailing_comma);
}

fn json_field_opt_i32<const N: usize>(
    buf: &mut String<N>,
    key: &str,
    value: Option<i32>,
    trailing_comma: bool,
) {
    json_field_opt_num(buf, key, value.map(|value| value as i64), trailing_comma);
}

fn json_field_opt_num<const N: usize>(
    buf: &mut String<N>,
    key: &str,
    value: Option<i64>,
    trailing_comma: bool,
) {
    let _ = buf.push('"');
    let _ = buf.push_str(key);
    let _ = buf.push_str("\":");
    if let Some(value) = value {
        let _ = write!(buf, "{}", value);
    } else {
        let _ = buf.push_str("null");
    }
    if trailing_comma {
        let _ = buf.push(',');
    }
}

fn json_field_opt_ipv4<const N: usize>(
    buf: &mut String<N>,
    key: &str,
    value: Option<[u8; 4]>,
    trailing_comma: bool,
) {
    let _ = buf.push('"');
    let _ = buf.push_str(key);
    let _ = buf.push_str("\":");
    if let Some(value) = value {
        let _ = buf.push('"');
        let mut addr = String::<16>::new();
        format_ipv4(&mut addr, value);
        let _ = buf.push_str(addr.as_str());
        let _ = buf.push('"');
    } else {
        let _ = buf.push_str("null");
    }
    if trailing_comma {
        let _ = buf.push(',');
    }
}

#[cfg(test)]
mod tests {
    use super::{
        accepts_event_stream, render_identity_json, render_status_json, write_error_body,
        write_sse_event, BuildInfo,
    };
    use crate::{
        mdns_wire::derive_device_identity,
        net_types::{NetworkUiSummary, UpsStatusSnapshot, WifiConnectionState, WifiSnapshot},
    };
    use heapless::String;

    #[test]
    fn event_stream_accept_parser_is_case_insensitive() {
        assert!(accepts_event_stream("application/json, text/event-stream"));
        assert!(accepts_event_stream("TEXT/EVENT-STREAM"));
        assert!(!accepts_event_stream("application/json"));
    }

    #[test]
    fn error_body_uses_shared_envelope() {
        let mut body = String::<256>::new();
        write_error_body(&mut body, "unavailable", "wifi down", true, None);
        assert_eq!(
            body.as_str(),
            r#"{"error":{"code":"unavailable","message":"wifi down","retryable":true}}"#
        );
    }

    #[test]
    fn identity_json_includes_capabilities_and_network_state() {
        let mut body = String::<1024>::new();
        render_identity_json(
            &mut body,
            &derive_device_identity([0x30, 0xae, 0xa4, 0x12, 0x34, 0x56]),
            WifiSnapshot {
                state: WifiConnectionState::Connected,
                ipv4: Some([192, 168, 31, 15]),
                gateway: Some([192, 168, 31, 1]),
                dns: Some([1, 1, 1, 1]),
                is_static: false,
                last_error: None,
                rssi_dbm: Some(-48),
                mac: Some([0x30, 0xae, 0xa4, 0x12, 0x34, 0x56]),
            },
            BuildInfo {
                package_version: "0.1.0",
                build_profile: "release",
                build_id: "abc",
                git_sha: "deadbee",
                src_hash: "1234",
                git_dirty: "clean",
            },
        );
        assert!(body
            .as_str()
            .contains("\"device_id\":\"mains-aegis-123456\""));
        assert!(body.as_str().contains("\"dns_sd\":true"));
        assert!(body.as_str().contains("\"ipv4\":\"192.168.31.15\""));
    }

    #[test]
    fn status_json_keeps_network_summary() {
        let mut body = String::<2048>::new();
        let mut status = UpsStatusSnapshot::empty();
        status.mode = "backup";
        status.network = NetworkUiSummary::from_wifi(WifiSnapshot {
            state: WifiConnectionState::Error,
            ipv4: None,
            gateway: None,
            dns: None,
            is_static: false,
            last_error: crate::net_types::WifiErrorKind::LinkLost.into(),
            rssi_dbm: None,
            mac: None,
        });
        render_status_json(&mut body, status);
        assert!(body.as_str().contains("\"mode\":\"backup\""));
        assert!(body.as_str().contains("\"last_error\":\"link_lost\""));
    }

    #[test]
    fn sse_frame_contains_event_and_data_lines() {
        let mut frame = String::<256>::new();
        write_sse_event(&mut frame, "status", r#"{"ok":true}"#, Some(7));
        assert_eq!(
            frame.as_str(),
            "id: 7\nevent: status\ndata: {\"ok\":true}\n\n"
        );
    }
}
