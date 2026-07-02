use core::fmt::Write as _;

use heapless::String;

use crate::{
    mdns_wire::DeviceIdentity,
    net_types::{
        format_ipv4, DerivedPowerBmsSnapshot, DerivedPowerChargerSnapshot, DerivedPowerSnapshot,
        DeviceSettingsSnapshot, UpsStatusSnapshot, WifiSnapshot, API_VERSION,
    },
};

const DIAG_SNAPSHOT_DERIVED_POWER_BODY_CAP: usize = 8192;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildInfo {
    pub package_version: &'static str,
    pub build_profile: &'static str,
    pub build_id: &'static str,
    pub git_sha: &'static str,
    pub src_hash: &'static str,
    pub git_dirty: &'static str,
    pub features: &'static str,
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
    } else {
        let _ = buf.push_str(",\"details\":null");
    }
    let _ = buf.push_str("}}");
}

pub fn render_identity_json<const N: usize>(
    buf: &mut String<N>,
    identity: &DeviceIdentity,
    wifi: WifiSnapshot,
    build: BuildInfo,
) {
    render_identity_json_with_write_controls(buf, identity, wifi, build, false);
}

pub fn render_identity_json_with_write_controls<const N: usize>(
    buf: &mut String<N>,
    identity: &DeviceIdentity,
    wifi: WifiSnapshot,
    build: BuildInfo,
    write_controls: bool,
) {
    let (output_profile, rated_vout_mv) = hardware_output_profile_for_features(build.features);
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
    json_field_str(buf, "git_dirty", build.git_dirty, true);
    json_array_from_csv(buf, "features", build.features, true);
    json_field_str(buf, "protocol", "mains-aegis.cdc.v1", true);
    let _ = buf.push_str(
        "\"defmt\":{\"enabled\":true,\"encoding\":\"defmt-espflash\",\"table_hash\":null}",
    );
    let _ = buf.push_str("},\"network\":");
    write_network_object(buf, wifi);
    let _ = buf.push_str(
        ",\"capabilities\":{\"sse\":true,\"mdns\":true,\"dns_sd\":true,\"write_controls\":",
    );
    let _ = buf.push_str(if write_controls { "true" } else { "false" });
    let _ = buf.push_str("},\"hardware_capabilities\":{");
    let _ = write!(
        buf,
        "\"output_profile\":\"{}\",\"rated_vout_mv\":{}",
        output_profile, rated_vout_mv
    );
    let _ = buf.push_str("}}");
}

fn json_array_from_csv<const N: usize>(buf: &mut String<N>, key: &str, csv: &str, comma: bool) {
    let _ = write!(buf, "\"{}\":[", key);
    let mut first = true;
    for item in csv.split(',').filter(|item| !item.is_empty()) {
        if !first {
            let _ = buf.push(',');
        }
        first = false;
        let _ = buf.push('"');
        write_json_string_escaped(buf, item);
        let _ = buf.push('"');
    }
    let _ = buf.push(']');
    if comma {
        let _ = buf.push(',');
    }
}

fn hardware_output_profile_for_features(features_csv: &str) -> (&'static str, u16) {
    if csv_has_feature(features_csv, "main-vout-19v") {
        ("19v", 19_000)
    } else {
        ("12v", 12_000)
    }
}

fn csv_has_feature(features_csv: &str, expected: &str) -> bool {
    features_csv
        .split(',')
        .map(str::trim)
        .any(|feature| feature == expected)
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

pub fn render_settings_json<const N: usize>(
    buf: &mut String<N>,
    settings: &DeviceSettingsSnapshot,
) {
    buf.clear();
    let _ = buf.push('{');
    let _ = buf.push_str("\"wifi\":{");
    let _ = write!(
        buf,
        "\"configured\":{}",
        if settings.wifi.configured {
            "true"
        } else {
            "false"
        }
    );
    if let Some(ssid) = settings.wifi.ssid.as_ref() {
        let _ = buf.push_str(",\"ssid\":\"");
        write_json_string_escaped(buf, ssid.as_str());
        let _ = buf.push('"');
    } else {
        let _ = buf.push_str(",\"ssid\":null");
    }
    let _ = buf.push_str("},");
    json_field_str(buf, "log_level", settings.log_level, true);
    let _ = buf.push_str("\"manual_charge\":{");
    json_field_str(buf, "target", settings.manual_charge.target, true);
    json_field_str(buf, "speed", settings.manual_charge.speed, true);
    let _ = write!(buf, "\"timer_h\":{}", settings.manual_charge.timer_h);
    let _ = buf.push_str("},\"advanced_power\":{");
    let _ = write!(
        buf,
        "\"standby_drop_mv\":{},\"assist_low_drop_mv\":{},\"assist_enter_delta_ma\":{},\"assist_exit_delta_ma\":{},\"assist_required_samples\":{},\"assist_ramp_step_mv\":{},\"assist_ramp_interval_ms\":{},\"rated_enter_delta_ma\":{},\"rated_exit_delta_ma\":{},\"vin_drop_threshold_pct\":{},\"required_samples\":{}",
        settings.advanced_power.standby_drop_mv,
        settings.advanced_power.assist_low_drop_mv,
        settings.advanced_power.assist_enter_delta_ma,
        settings.advanced_power.assist_exit_delta_ma,
        settings.advanced_power.assist_required_samples,
        settings.advanced_power.assist_ramp_step_mv,
        settings.advanced_power.assist_ramp_interval_ms,
        settings.advanced_power.rated_enter_delta_ma,
        settings.advanced_power.rated_exit_delta_ma,
        settings.advanced_power.vin_drop_threshold_pct,
        settings.advanced_power.required_samples,
    );
    let _ = buf.push_str("},\"advanced_power_capabilities\":{");
    let _ = write!(
        buf,
        "\"rated_vout_mv\":{},\"standby_drop_mv\":{{\"default\":{},\"min\":{},\"max\":{},\"step\":{}}},\"assist_low_drop_mv\":{{\"default\":{},\"min\":{},\"max\":{},\"step\":{}}},\"assist_enter_delta_ma\":{{\"default\":{},\"min\":{},\"max\":{},\"step\":{}}},\"assist_exit_delta_ma\":{{\"default\":{},\"min\":{},\"max\":{},\"step\":{}}},\"assist_required_samples\":{{\"default\":{},\"min\":{},\"max\":{},\"step\":{}}},\"assist_ramp_step_mv\":{{\"default\":{},\"min\":{},\"max\":{},\"step\":{}}},\"assist_ramp_interval_ms\":{{\"default\":{},\"min\":{},\"max\":{},\"step\":{}}},\"rated_enter_delta_ma\":{{\"default\":{},\"min\":{},\"max\":{},\"step\":{}}},\"rated_exit_delta_ma\":{{\"default\":{},\"min\":{},\"max\":{},\"step\":{}}},\"vin_drop_threshold_pct\":{{\"default\":{},\"min\":{},\"max\":{},\"step\":{}}},\"required_samples\":{{\"default\":{},\"min\":{},\"max\":{},\"step\":{}}}",
        settings.advanced_power_capabilities.rated_vout_mv,
        settings.advanced_power_capabilities.standby_drop_mv.default,
        settings.advanced_power_capabilities.standby_drop_mv.min,
        settings.advanced_power_capabilities.standby_drop_mv.max,
        settings.advanced_power_capabilities.standby_drop_mv.step,
        settings.advanced_power_capabilities.assist_low_drop_mv.default,
        settings.advanced_power_capabilities.assist_low_drop_mv.min,
        settings.advanced_power_capabilities.assist_low_drop_mv.max,
        settings.advanced_power_capabilities.assist_low_drop_mv.step,
        settings.advanced_power_capabilities.assist_enter_delta_ma.default,
        settings.advanced_power_capabilities.assist_enter_delta_ma.min,
        settings.advanced_power_capabilities.assist_enter_delta_ma.max,
        settings.advanced_power_capabilities.assist_enter_delta_ma.step,
        settings.advanced_power_capabilities.assist_exit_delta_ma.default,
        settings.advanced_power_capabilities.assist_exit_delta_ma.min,
        settings.advanced_power_capabilities.assist_exit_delta_ma.max,
        settings.advanced_power_capabilities.assist_exit_delta_ma.step,
        settings.advanced_power_capabilities.assist_required_samples.default,
        settings.advanced_power_capabilities.assist_required_samples.min,
        settings.advanced_power_capabilities.assist_required_samples.max,
        settings.advanced_power_capabilities.assist_required_samples.step,
        settings.advanced_power_capabilities.assist_ramp_step_mv.default,
        settings.advanced_power_capabilities.assist_ramp_step_mv.min,
        settings.advanced_power_capabilities.assist_ramp_step_mv.max,
        settings.advanced_power_capabilities.assist_ramp_step_mv.step,
        settings.advanced_power_capabilities.assist_ramp_interval_ms.default,
        settings.advanced_power_capabilities.assist_ramp_interval_ms.min,
        settings.advanced_power_capabilities.assist_ramp_interval_ms.max,
        settings.advanced_power_capabilities.assist_ramp_interval_ms.step,
        settings.advanced_power_capabilities.rated_enter_delta_ma.default,
        settings.advanced_power_capabilities.rated_enter_delta_ma.min,
        settings.advanced_power_capabilities.rated_enter_delta_ma.max,
        settings.advanced_power_capabilities.rated_enter_delta_ma.step,
        settings.advanced_power_capabilities.rated_exit_delta_ma.default,
        settings.advanced_power_capabilities.rated_exit_delta_ma.min,
        settings.advanced_power_capabilities.rated_exit_delta_ma.max,
        settings.advanced_power_capabilities.rated_exit_delta_ma.step,
        settings.advanced_power_capabilities.vin_drop_threshold_pct.default,
        settings.advanced_power_capabilities.vin_drop_threshold_pct.min,
        settings.advanced_power_capabilities.vin_drop_threshold_pct.max,
        settings.advanced_power_capabilities.vin_drop_threshold_pct.step,
        settings.advanced_power_capabilities.required_samples.default,
        settings.advanced_power_capabilities.required_samples.min,
        settings.advanced_power_capabilities.required_samples.max,
        settings.advanced_power_capabilities.required_samples.step,
    );
    let _ = buf.push_str("}}");
}

pub fn render_status_json<const N: usize>(buf: &mut String<N>, status: UpsStatusSnapshot) {
    buf.clear();
    let _ = buf.push('{');
    json_field_str(buf, "mode", status.mode, true);
    let _ = buf.push_str("\"input\":{");
    json_field_str(buf, "source", status.input_source, true);
    json_field_opt_bool(buf, "mains_present", status.mains_present, true);
    json_field_opt_u16(buf, "input_vbus_mv", status.input_vbus_mv, true);
    json_field_opt_i32(buf, "input_ibus_ma", status.input_ibus_ma, true);
    json_field_opt_u16(buf, "vin_vbus_mv", status.vin_vbus_mv, true);
    json_field_opt_i32(buf, "vin_iin_ma", status.vin_iin_ma, true);
    json_field_opt_i32(buf, "tps_total_iout_ma", status.tps_total_iout_ma, true);
    json_field_opt_i32(
        buf,
        "tps_limit_threshold_ma",
        status.tps_limit_threshold_ma,
        true,
    );
    json_field_str(buf, "pressure_state", status.input_pressure_state, true);
    json_field_opt_u8(
        buf,
        "pressure_score_pct",
        status.input_pressure_score_pct,
        true,
    );
    json_field_opt_str(buf, "pressure_reason", status.input_pressure_reason, true);
    json_field_opt_u16(buf, "vin_baseline_mv", status.input_vin_baseline_mv, true);
    json_field_opt_u16(buf, "vin_drop_mv", status.input_vin_drop_mv, true);
    json_field_opt_str(buf, "assist_power_stage", status.assist_power_stage, true);
    json_field_opt_u16(
        buf,
        "assist_target_vout_mv",
        status.assist_target_vout_mv,
        false,
    );
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
    json_field_opt_bool(buf, "vbat_present", status.charger_vbat_present, true);
    json_field_opt_u16(
        buf,
        "policy_target_ichg_ma",
        status.charger_policy_target_ichg_ma,
        true,
    );
    json_field_opt_bool(buf, "limit_active", status.charger_limit_active, true);
    json_field_opt_str(buf, "limit_reason", status.charger_limit_reason, true);
    json_field_opt_str(buf, "limit_detail", status.charger_limit_detail, true);
    json_field_opt_i32(
        buf,
        "limit_threshold_ma",
        status.charger_limit_threshold_ma,
        true,
    );
    json_field_opt_str(buf, "detail_status", status.charger_detail_status, false);
    let _ = buf.push_str("},\"battery\":{");
    json_field_str(buf, "state", status.battery_state, true);
    json_field_opt_u16(buf, "pack_mv", status.battery_pack_mv, true);
    json_field_opt_i16(buf, "current_ma", status.battery_current_ma, true);
    json_field_opt_u16(buf, "soc_pct", status.battery_soc_pct, true);
    json_field_opt_u16_array(buf, "cell_mv", status.battery_cell_mv, true);
    json_field_opt_u16(buf, "cell_delta_mv", status.battery_cell_delta_mv, true);
    json_field_opt_bool(buf, "balance_enabled", status.battery_balance_enabled, true);
    json_field_opt_bool(
        buf,
        "balance_cfg_match",
        status.battery_balance_cfg_match,
        true,
    );
    json_field_opt_bool(buf, "balance_active", status.battery_balance_active, true);
    json_field_opt_u8(buf, "balance_mask", status.battery_balance_mask, true);
    json_field_opt_u8(buf, "balance_cell", status.battery_balance_cell, true);
    json_field_opt_u8(
        buf,
        "balance_min_start_delta_mv",
        status.battery_balance_min_start_delta_mv,
        true,
    );
    json_field_opt_bool(buf, "no_battery", status.battery_no_battery, true);
    json_field_opt_bool(buf, "discharge_ready", status.battery_discharge_ready, true);
    json_field_opt_bool(buf, "charge_fet_on", status.battery_charge_fet_on, true);
    json_field_opt_bool(
        buf,
        "discharge_fet_on",
        status.battery_discharge_fet_on,
        true,
    );
    json_field_opt_bool(
        buf,
        "precharge_fet_on",
        status.battery_precharge_fet_on,
        true,
    );
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
    let _ = buf.push_str("},\"front_panel\":{");
    json_field_str(buf, "init_state", status.front_panel.init_state, true);
    json_field_str(
        buf,
        "display_power_mode",
        status.front_panel.display_power_mode,
        true,
    );
    json_field_str(buf, "ui_variant", status.front_panel.ui_variant, true);
    json_field_u32(buf, "frame_no", status.front_panel.frame_no, true);
    json_field_bool(buf, "ready", status.front_panel.ready, true);
    json_field_bool(buf, "needs_redraw", status.front_panel.needs_redraw, true);
    json_field_bool(
        buf,
        "attention_hold",
        status.front_panel.attention_hold,
        false,
    );
    let _ = buf.push_str("},\"network\":");
    write_network_summary_object(
        buf,
        status.network.state.as_str(),
        status.network.ipv4,
        status.network.last_error.map(|err| err.as_str()),
    );
    let _ = buf.push('}');
}

pub fn render_compact_status_json<const N: usize>(buf: &mut String<N>, status: UpsStatusSnapshot) {
    buf.clear();
    let _ = buf.push('{');
    json_field_str(buf, "mode", status.mode, true);
    let _ = buf.push_str("\"input\":{");
    json_field_str(buf, "source", status.input_source, true);
    json_field_opt_bool(buf, "mains_present", status.mains_present, true);
    json_field_opt_u16(buf, "input_vbus_mv", status.input_vbus_mv, true);
    json_field_opt_i32(buf, "input_ibus_ma", status.input_ibus_ma, true);
    json_field_opt_u16(buf, "vin_vbus_mv", status.vin_vbus_mv, true);
    json_field_opt_i32(buf, "vin_iin_ma", status.vin_iin_ma, true);
    json_field_opt_i32(buf, "tps_total_iout_ma", status.tps_total_iout_ma, true);
    json_field_opt_i32(
        buf,
        "tps_limit_threshold_ma",
        status.tps_limit_threshold_ma,
        true,
    );
    json_field_str(buf, "pressure_state", status.input_pressure_state, true);
    json_field_opt_u8(
        buf,
        "pressure_score_pct",
        status.input_pressure_score_pct,
        true,
    );
    json_field_opt_str(buf, "pressure_reason", status.input_pressure_reason, true);
    json_field_opt_u16(buf, "vin_baseline_mv", status.input_vin_baseline_mv, true);
    json_field_opt_u16(buf, "vin_drop_mv", status.input_vin_drop_mv, true);
    json_field_opt_str(buf, "assist_power_stage", status.assist_power_stage, true);
    json_field_opt_u16(
        buf,
        "assist_target_vout_mv",
        status.assist_target_vout_mv,
        false,
    );
    let _ = buf.push_str("},\"output\":{");
    json_field_str(buf, "requested", status.requested_outputs, true);
    json_field_str(buf, "active", status.active_outputs, true);
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
    json_field_opt_bool(buf, "vbat_present", status.charger_vbat_present, true);
    json_field_opt_u16(
        buf,
        "policy_target_ichg_ma",
        status.charger_policy_target_ichg_ma,
        true,
    );
    json_field_opt_bool(buf, "limit_active", status.charger_limit_active, true);
    json_field_opt_str(buf, "limit_reason", status.charger_limit_reason, true);
    json_field_opt_str(buf, "limit_detail", status.charger_limit_detail, true);
    json_field_opt_i32(
        buf,
        "limit_threshold_ma",
        status.charger_limit_threshold_ma,
        false,
    );
    let _ = buf.push_str("},\"battery\":{");
    json_field_str(buf, "state", status.battery_state, true);
    json_field_opt_u16(buf, "pack_mv", status.battery_pack_mv, true);
    json_field_opt_i16(buf, "current_ma", status.battery_current_ma, true);
    json_field_opt_u16(buf, "soc_pct", status.battery_soc_pct, true);
    json_field_opt_bool(buf, "discharge_ready", status.battery_discharge_ready, true);
    json_field_opt_bool(buf, "charge_fet_on", status.battery_charge_fet_on, true);
    json_field_opt_bool(
        buf,
        "discharge_fet_on",
        status.battery_discharge_fet_on,
        false,
    );
    let _ = buf.push_str("},\"front_panel\":{");
    json_field_str(buf, "init_state", status.front_panel.init_state, true);
    json_field_str(
        buf,
        "display_power_mode",
        status.front_panel.display_power_mode,
        true,
    );
    json_field_str(buf, "ui_variant", status.front_panel.ui_variant, true);
    json_field_u32(buf, "frame_no", status.front_panel.frame_no, true);
    json_field_bool(buf, "ready", status.front_panel.ready, true);
    json_field_bool(buf, "needs_redraw", status.front_panel.needs_redraw, true);
    json_field_bool(
        buf,
        "attention_hold",
        status.front_panel.attention_hold,
        false,
    );
    let _ = buf.push_str("}}");
}

pub fn render_derived_power_json<const N: usize>(buf: &mut String<N>, diag: DerivedPowerSnapshot) {
    buf.clear();
    let _ = buf.push('{');
    let _ = buf.push_str("\"input\":{");
    json_field_str(buf, "source", diag.input.source, true);
    json_field_opt_bool(buf, "mains_present", diag.input.mains_present, true);
    json_field_opt_u16(buf, "input_vbus_mv", diag.input.input_vbus_mv, true);
    json_field_opt_i32(buf, "input_ibus_ma", diag.input.input_ibus_ma, true);
    json_field_opt_u16(buf, "vin_vbus_mv", diag.input.vin_vbus_mv, true);
    json_field_opt_i32(buf, "vin_iin_ma", diag.input.vin_iin_ma, true);
    json_field_opt_i32(buf, "tps_total_iout_ma", diag.input.tps_total_iout_ma, true);
    json_field_opt_i32(
        buf,
        "tps_limit_threshold_ma",
        diag.input.tps_limit_threshold_ma,
        true,
    );
    json_field_str(buf, "pressure_state", diag.input.pressure_state, true);
    json_field_opt_u8(
        buf,
        "pressure_score_pct",
        diag.input.pressure_score_pct,
        true,
    );
    json_field_opt_str(buf, "pressure_reason", diag.input.pressure_reason, true);
    json_field_opt_u16(buf, "vin_baseline_mv", diag.input.vin_baseline_mv, true);
    json_field_opt_u16(buf, "vin_drop_mv", diag.input.vin_drop_mv, true);
    json_field_opt_str(
        buf,
        "assist_power_stage",
        diag.input.assist_power_stage,
        true,
    );
    json_field_opt_u16(
        buf,
        "assist_target_vout_mv",
        diag.input.assist_target_vout_mv,
        true,
    );
    json_field_bool(buf, "usb_pd_attached", diag.input.usb_pd_attached, true);
    json_field_bool(
        buf,
        "usb_pd_charge_ready",
        diag.input.usb_pd_charge_ready,
        true,
    );
    json_field_opt_bool(
        buf,
        "usb_pd_vbus_present",
        diag.input.usb_pd_vbus_present,
        true,
    );
    json_field_bool(
        buf,
        "usb_pd_unsafe_source_latched",
        diag.input.usb_pd_unsafe_source_latched,
        true,
    );
    json_field_opt_str(
        buf,
        "usb_pd_contract_kind",
        diag.input.usb_pd_contract_kind,
        true,
    );
    json_field_opt_u16(
        buf,
        "usb_pd_contract_mv",
        diag.input.usb_pd_contract_mv,
        true,
    );
    json_field_opt_u16(
        buf,
        "usb_pd_contract_ma",
        diag.input.usb_pd_contract_ma,
        true,
    );
    json_field_opt_u16(buf, "usb_pd_vac1_mv", diag.input.usb_pd_vac1_mv, true);
    json_field_opt_u16(buf, "usb_pd_vsys_mv", diag.input.usb_pd_vsys_mv, false);
    let _ = buf.push_str("},\"charger\":{");
    json_field_bool(buf, "poll_valid", diag.charger.poll_valid, true);
    json_field_bool(buf, "enabled", diag.charger.enabled, true);
    json_field_bool(buf, "ce_low", diag.charger.ce_low, true);
    json_field_bool(buf, "ilim_hiz_brk_low", diag.charger.ilim_hiz_brk_low, true);
    json_field_bool(buf, "allow_charge", diag.charger.allow_charge, true);
    json_field_bool(
        buf,
        "normal_allow_charge",
        diag.charger.normal_allow_charge,
        true,
    );
    json_field_bool(
        buf,
        "force_allow_charge",
        diag.charger.force_allow_charge,
        true,
    );
    json_field_bool(buf, "can_enable", diag.charger.can_enable, true);
    json_field_bool(
        buf,
        "usb_pd_charge_gate_ready",
        diag.charger.usb_pd_charge_gate_ready,
        true,
    );
    json_field_bool(buf, "input_present", diag.charger.input_present, true);
    json_field_bool(buf, "vbus_present", diag.charger.vbus_present, true);
    json_field_bool(buf, "ac1_present", diag.charger.ac1_present, true);
    json_field_bool(buf, "ac2_present", diag.charger.ac2_present, true);
    json_field_bool(buf, "pg", diag.charger.pg, true);
    json_field_bool(buf, "vbat_present", diag.charger.vbat_present, true);
    json_field_bool(buf, "adc_enabled", diag.charger.adc_enabled, true);
    json_field_bool(buf, "adc_done", diag.charger.adc_done, true);
    json_field_bool(buf, "adc_ready", diag.charger.adc_ready, true);
    json_field_opt_i16(buf, "ibus_adc_ma", diag.charger.ibus_adc_ma, true);
    json_field_opt_i16(buf, "ibat_adc_ma", diag.charger.ibat_adc_ma, true);
    json_field_opt_u16(buf, "vbus_adc_mv", diag.charger.vbus_adc_mv, true);
    json_field_opt_u16(buf, "vbat_adc_mv", diag.charger.vbat_adc_mv, true);
    json_field_opt_u16(buf, "vsys_adc_mv", diag.charger.vsys_adc_mv, true);
    json_field_opt_u16(buf, "vac1_adc_mv", diag.charger.vac1_adc_mv, true);
    json_field_opt_u16(buf, "vac2_adc_mv", diag.charger.vac2_adc_mv, true);
    json_field_opt_u16(buf, "vreg_mv", diag.charger.vreg_mv, true);
    json_field_opt_u16(buf, "ichg_ma", diag.charger.ichg_ma, true);
    json_field_opt_u16(buf, "vindpm_mv", diag.charger.vindpm_mv, true);
    json_field_opt_u16(buf, "iindpm_ma", diag.charger.iindpm_ma, true);
    json_field_opt_u16(
        buf,
        "vbat_lowv_pct_x10",
        diag.charger.vbat_lowv_pct_x10,
        true,
    );
    json_field_opt_u16(buf, "iprechg_ma", diag.charger.iprechg_ma, true);
    json_field_opt_u16(buf, "iterm_ma", diag.charger.iterm_ma, true);
    json_field_str(buf, "chg_stat", diag.charger.chg_stat, true);
    json_field_str(buf, "vbus_stat", diag.charger.vbus_stat, true);
    json_field_str(buf, "ico_stat", diag.charger.ico_stat, true);
    json_field_bool(buf, "treg", diag.charger.treg, true);
    json_field_bool(buf, "dpdm", diag.charger.dpdm, true);
    json_field_bool(buf, "wd", diag.charger.wd, true);
    json_field_bool(buf, "poorsrc", diag.charger.poorsrc, true);
    json_field_bool(buf, "vindpm", diag.charger.vindpm, true);
    json_field_bool(buf, "iindpm", diag.charger.iindpm, true);
    json_field_bool(buf, "ts_cold", diag.charger.ts_cold, true);
    json_field_bool(buf, "ts_hot", diag.charger.ts_hot, true);
    json_field_opt_u8(buf, "st0", diag.charger.st0, true);
    json_field_opt_u8(buf, "st1", diag.charger.st1, true);
    json_field_opt_u8(buf, "st2", diag.charger.st2, true);
    json_field_opt_u8(buf, "st3", diag.charger.st3, true);
    json_field_opt_u8(buf, "st4", diag.charger.st4, true);
    json_field_opt_u8(buf, "fault0", diag.charger.fault0, true);
    json_field_opt_u8(buf, "fault1", diag.charger.fault1, true);
    json_field_opt_u8(buf, "ctrl0", diag.charger.ctrl0, true);
    json_field_opt_u8(buf, "ctrl3", diag.charger.ctrl3, true);
    json_field_opt_u8(buf, "ctrl4", diag.charger.ctrl4, true);
    json_field_str(buf, "acdrv_path", diag.charger.acdrv_path, true);
    json_field_opt_u16(buf, "term_ctrl", diag.charger.term_ctrl, false);
    let _ = buf.push_str("},\"policy\":{");
    json_field_opt_str(buf, "state", diag.policy.state, true);
    json_field_str(buf, "status", diag.policy.status, true);
    json_field_str(buf, "notice", diag.policy.notice, true);
    json_field_str(buf, "input_source", diag.policy.input_source, true);
    json_field_opt_str(buf, "start_reason", diag.policy.start_reason, true);
    json_field_opt_str(buf, "full_reason", diag.policy.full_reason, true);
    json_field_opt_str(
        buf,
        "output_block_reason",
        diag.policy.output_block_reason,
        true,
    );
    json_field_opt_str(buf, "recovery_stage", diag.policy.recovery_stage, true);
    json_field_opt_u16(buf, "target_ichg_ma", diag.policy.target_ichg_ma, true);
    json_field_opt_u16(
        buf,
        "adaptive_cap_ichg_ma",
        diag.policy.adaptive_cap_ichg_ma,
        true,
    );
    json_field_opt_u16(
        buf,
        "effective_target_ichg_ma",
        diag.policy.effective_target_ichg_ma,
        true,
    );
    json_field_bool(buf, "limit_active", diag.policy.limit_active, true);
    json_field_opt_str(buf, "limit_reason", diag.policy.limit_reason, true);
    json_field_opt_str(buf, "limit_detail", diag.policy.limit_detail, true);
    json_field_opt_str(buf, "detail_status", diag.policy.detail_status, true);
    json_field_str(buf, "pressure_state", diag.policy.pressure_state, true);
    json_field_opt_str(buf, "pressure_reason", diag.policy.pressure_reason, true);
    json_field_opt_u8(
        buf,
        "pressure_score_pct",
        diag.policy.pressure_score_pct,
        true,
    );
    json_field_opt_u16(buf, "vin_baseline_mv", diag.policy.vin_baseline_mv, true);
    json_field_opt_u16(buf, "vin_drop_mv", diag.policy.vin_drop_mv, true);
    json_field_opt_i32(
        buf,
        "tps_total_iout_ma",
        diag.policy.tps_total_iout_ma,
        true,
    );
    json_field_opt_i32(
        buf,
        "tps_limit_threshold_ma",
        diag.policy.tps_limit_threshold_ma,
        true,
    );
    json_field_opt_u32(buf, "output_power_w10", diag.policy.output_power_w10, true);
    json_field_bool(buf, "charge_latched", diag.policy.charge_latched, true);
    json_field_bool(buf, "full_latched", diag.policy.full_latched, true);
    json_field_bool(buf, "dc_derated", diag.policy.dc_derated, true);
    json_field_bool(buf, "output_blocked", diag.policy.output_blocked, true);
    json_field_bool(buf, "manual_active", diag.policy.manual_active, true);
    json_field_bool(
        buf,
        "manual_stop_inhibit",
        diag.policy.manual_stop_inhibit,
        false,
    );
    let _ = buf.push_str("},\"bms\":{");
    json_field_opt_u8(buf, "addr", diag.bms.addr, true);
    json_field_str(buf, "state", diag.bms.state, true);
    json_field_opt_u16(buf, "pack_mv", diag.bms.pack_mv, true);
    json_field_opt_i16(buf, "current_ma", diag.bms.current_ma, true);
    json_field_opt_u16(buf, "soc_pct", diag.bms.soc_pct, true);
    json_field_opt_u16(buf, "cell_min_mv", diag.bms.cell_min_mv, true);
    json_field_opt_u16(buf, "cell_max_mv", diag.bms.cell_max_mv, true);
    json_field_opt_bool(buf, "no_battery", diag.bms.no_battery, true);
    json_field_opt_bool(buf, "discharge_ready", diag.bms.discharge_ready, true);
    json_field_opt_bool(buf, "charge_ready", diag.bms.charge_ready, true);
    json_field_opt_bool(buf, "full", diag.bms.full, true);
    json_field_opt_str(buf, "issue_detail", diag.bms.issue_detail, true);
    json_field_opt_bool(buf, "rca_alarm", diag.bms.rca_alarm, true);
    json_field_opt_u32(buf, "safety_status", diag.bms.safety_status, true);
    json_field_opt_u32(buf, "pf_status", diag.bms.pf_status, true);
    json_field_opt_u32(
        buf,
        "manufacturing_status",
        diag.bms.manufacturing_status,
        true,
    );
    json_field_opt_u32(buf, "gauging_status", diag.bms.gauging_status, true);
    json_field_opt_u32(buf, "op_status", diag.bms.op_status, true);
    json_field_opt_u8(buf, "op_status_raw_len", diag.bms.op_status_raw_len, true);
    json_field_opt_u8_array(
        buf,
        "op_status_raw_bytes",
        diag.bms
            .op_status_raw_bytes
            .as_ref()
            .map(|bytes| &bytes[..]),
        true,
    );
    json_field_opt_bool(buf, "emshut", diag.bms.emshut, true);
    json_field_opt_bool(buf, "pres", diag.bms.pres, true);
    json_field_opt_bool(buf, "xchg", diag.bms.xchg, true);
    json_field_opt_bool(buf, "xdsg", diag.bms.xdsg, true);
    json_field_opt_bool(buf, "chg_fet", diag.bms.chg_fet, true);
    json_field_opt_bool(buf, "dsg_fet", diag.bms.dsg_fet, true);
    json_field_opt_bool(buf, "pchg_fet", diag.bms.pchg_fet, true);
    json_field_opt_bool(buf, "cuv", diag.bms.cuv, true);
    json_field_opt_bool(buf, "cuvc", diag.bms.cuvc, true);
    json_field_opt_u16(buf, "cuv_recovery_mv", diag.bms.cuv_recovery_mv, true);
    json_field_opt_bool(buf, "cuv_recov_chg", diag.bms.cuv_recov_chg, true);
    json_field_opt_bool(buf, "fet_en", diag.bms.fet_en, true);
    json_field_opt_bool(buf, "chg_en", diag.bms.chg_en, true);
    json_field_opt_bool(buf, "dsg_en", diag.bms.dsg_en, true);
    json_field_opt_bool(buf, "charging_inhibit", diag.bms.charging_inhibit, true);
    json_field_opt_bool(buf, "charging_suspend", diag.bms.charging_suspend, true);
    json_field_opt_bool(buf, "charging_hv", diag.bms.charging_hv, true);
    json_field_opt_u16(buf, "current_at_eoc_ma", diag.bms.current_at_eoc_ma, true);
    json_field_opt_u16(buf, "da_configuration", diag.bms.da_configuration, true);
    json_field_opt_u16(buf, "power_config", diag.bms.power_config, true);
    json_field_opt_bool(buf, "emshut_en", diag.bms.emshut_en, true);
    json_field_opt_bool(buf, "emshut_pexit_dis", diag.bms.emshut_pexit_dis, true);
    json_field_opt_bool(buf, "emshut_exit_comm", diag.bms.emshut_exit_comm, true);
    json_field_opt_bool(buf, "emshut_exit_vpack", diag.bms.emshut_exit_vpack, false);
    let _ = buf.push_str("}}");
}

pub fn render_diag_snapshot_json<'a, const N: usize>(
    buf: &mut String<N>,
    requested_packages: &'a [String<32>],
    status: UpsStatusSnapshot,
    diag: DerivedPowerSnapshot,
) {
    buf.clear();
    let _ = buf.push_str("{\"packages\":{");
    let mut emitted = DiagEmitState::<'a>::default();
    if requested_packages.is_empty() {
        render_diag_snapshot_package(buf, &mut emitted, "mcu.runtime", status, diag);
        render_diag_snapshot_package(buf, &mut emitted, "derived.power", status, diag);
    } else {
        for package in requested_packages {
            match package.as_str() {
                "core" => {
                    render_diag_snapshot_package(buf, &mut emitted, "mcu.runtime", status, diag);
                    render_diag_snapshot_package(buf, &mut emitted, "derived.power", status, diag);
                }
                id => render_diag_snapshot_package(buf, &mut emitted, id, status, diag),
            }
        }
    }
    let _ = buf.push_str("},\"errors\":{");
    let mut first_error = true;
    for error in emitted.errors.iter() {
        if !first_error {
            let _ = buf.push(',');
        }
        first_error = false;
        let _ = buf.push('"');
        write_json_string_escaped(buf, error);
        let _ = buf.push_str(
            "\":{\"code\":\"unsupported_package\",\"message\":\"diagnostic package is not supported\"}",
        );
    }
    let _ = buf.push_str("}}");
}

#[derive(Default)]
struct DiagEmitState<'a> {
    packages: heapless::Vec<&'a str, 16>,
    errors: heapless::Vec<&'a str, 8>,
}

fn render_diag_snapshot_package<'a, const N: usize>(
    buf: &mut String<N>,
    emitted: &mut DiagEmitState<'a>,
    id: &'a str,
    status: UpsStatusSnapshot,
    diag: DerivedPowerSnapshot,
) {
    if emitted.packages.iter().any(|seen| *seen == id) {
        return;
    }
    match id {
        "mcu.runtime" => {
            render_diag_package_header(buf, emitted, id, "runtime_cache", 0);
            render_diag_mcu_runtime_payload(buf, status);
            let _ = buf.push('}');
        }
        "bq40.core" => {
            render_diag_package_header(buf, emitted, id, "power_cache", 0);
            render_diag_bms_payload(buf, diag.bms);
            let _ = buf.push('}');
        }
        "bq40.manufacturing" => {
            render_diag_package_header(buf, emitted, id, "fresh_i2c", 0);
            render_diag_bms_payload(buf, diag.bms);
            let _ = buf.push('}');
        }
        "bq25792.regs" => {
            render_diag_package_header(buf, emitted, id, "power_cache", 0);
            render_diag_charger_payload(buf, diag.charger);
            let _ = buf.push('}');
        }
        "tps55288.out_a" => {
            render_diag_package_header(buf, emitted, id, "status_cache", 0);
            render_diag_output_payload(
                buf,
                status.out_a_state,
                status.out_a_enabled,
                status.out_a_vbus_mv,
                status.out_a_iout_ma,
            );
            let _ = buf.push('}');
        }
        "tps55288.out_b" => {
            render_diag_package_header(buf, emitted, id, "status_cache", 0);
            render_diag_output_payload(
                buf,
                status.out_b_state,
                status.out_b_enabled,
                status.out_b_vbus_mv,
                status.out_b_iout_ma,
            );
            let _ = buf.push('}');
        }
        "ina3221.regs" => {
            render_diag_package_header(buf, emitted, id, "status_cache", 0);
            render_diag_ina_payload(buf, status);
            let _ = buf.push('}');
        }
        "tmp112.out_a" => {
            render_diag_package_header(buf, emitted, id, "status_cache", 0);
            render_diag_tmp_payload(buf, status.tmp_a_state, status.tmp_a_c);
            let _ = buf.push('}');
        }
        "tmp112.out_b" => {
            render_diag_package_header(buf, emitted, id, "status_cache", 0);
            render_diag_tmp_payload(buf, status.tmp_b_state, status.tmp_b_c);
            let _ = buf.push('}');
        }
        "fusb302.regs" => {
            render_diag_package_header(buf, emitted, id, "power_cache", 0);
            render_diag_fusb_payload(buf, diag);
            let _ = buf.push('}');
        }
        "usbpd.policy" => {
            render_diag_package_header(buf, emitted, id, "power_cache", 0);
            render_diag_usbpd_policy_payload(buf, diag);
            let _ = buf.push('}');
        }
        "front_panel.io" => {
            render_diag_package_header(buf, emitted, id, "status_cache", 0);
            render_diag_front_panel_payload(buf, status);
            let _ = buf.push('}');
        }
        "derived.power" => {
            render_diag_package_header(buf, emitted, id, "power_cache", 0);
            let mut nested = String::<DIAG_SNAPSHOT_DERIVED_POWER_BODY_CAP>::new();
            render_derived_power_json(&mut nested, diag);
            let _ = buf.push_str(nested.as_str());
            let _ = buf.push('}');
        }
        _ => {
            let _ = emitted.errors.push(id);
        }
    }
}

fn render_diag_package_header<'a, const N: usize>(
    buf: &mut String<N>,
    emitted: &mut DiagEmitState<'a>,
    id: &'a str,
    source: &str,
    duration_ms: u16,
) {
    if !emitted.packages.is_empty() {
        let _ = buf.push(',');
    }
    let _ = emitted.packages.push(id);
    let _ = buf.push('"');
    write_json_string_escaped(buf, id);
    let _ = buf.push_str("\":{\"ok\":true,\"source\":\"");
    write_json_string_escaped(buf, source);
    let _ = write!(buf, "\",\"duration_ms\":{},\"payload\":", duration_ms);
}

fn render_diag_mcu_runtime_payload<const N: usize>(buf: &mut String<N>, status: UpsStatusSnapshot) {
    let _ = buf.push('{');
    json_field_str(buf, "mode", status.mode, true);
    json_field_str(buf, "requested_outputs", status.requested_outputs, true);
    json_field_str(buf, "active_outputs", status.active_outputs, true);
    json_field_str(buf, "recoverable_outputs", status.recoverable_outputs, true);
    json_field_str(buf, "output_gate_reason", status.output_gate_reason, true);
    json_field_str(buf, "input_source", status.input_source, false);
    let _ = buf.push('}');
}

fn render_diag_bms_payload<const N: usize>(buf: &mut String<N>, bms: DerivedPowerBmsSnapshot) {
    let _ = buf.push('{');
    json_field_opt_u8(buf, "addr", bms.addr, true);
    json_field_str(buf, "state", bms.state, true);
    json_field_opt_u16(buf, "pack_mv", bms.pack_mv, true);
    json_field_opt_i16(buf, "current_ma", bms.current_ma, true);
    json_field_opt_u16(buf, "soc_pct", bms.soc_pct, true);
    json_field_opt_bool(buf, "discharge_ready", bms.discharge_ready, true);
    json_field_opt_bool(buf, "charge_ready", bms.charge_ready, true);
    json_field_opt_str(buf, "issue_detail", bms.issue_detail, true);
    json_field_opt_u32(buf, "safety_status", bms.safety_status, true);
    json_field_opt_u32(buf, "pf_status", bms.pf_status, true);
    json_field_opt_u32(buf, "manufacturing_status", bms.manufacturing_status, true);
    json_field_opt_u32(buf, "gauging_status", bms.gauging_status, true);
    json_field_opt_u32(buf, "charging_status", bms.charging_status, true);
    json_field_opt_u32(buf, "op_status", bms.op_status, true);
    json_field_opt_u8(buf, "op_status_raw_len", bms.op_status_raw_len, true);
    json_field_opt_u8_array(
        buf,
        "op_status_raw_bytes",
        bms.op_status_raw_bytes.as_ref().map(|bytes| &bytes[..]),
        true,
    );
    json_field_opt_bool(buf, "emshut", bms.emshut, true);
    json_field_opt_bool(buf, "pres", bms.pres, true);
    json_field_opt_bool(buf, "xchg", bms.xchg, true);
    json_field_opt_bool(buf, "xdsg", bms.xdsg, true);
    json_field_opt_bool(buf, "chg_fet", bms.chg_fet, true);
    json_field_opt_bool(buf, "dsg_fet", bms.dsg_fet, true);
    json_field_opt_bool(buf, "pchg_fet", bms.pchg_fet, true);
    json_field_opt_bool(buf, "cuv", bms.cuv, true);
    json_field_opt_bool(buf, "cuvc", bms.cuvc, true);
    json_field_opt_bool(buf, "fet_en", bms.fet_en, true);
    json_field_opt_bool(buf, "chg_en", bms.chg_en, true);
    json_field_opt_bool(buf, "dsg_en", bms.dsg_en, true);
    json_field_opt_bool(buf, "charging_inhibit", bms.charging_inhibit, true);
    json_field_opt_bool(buf, "charging_suspend", bms.charging_suspend, true);
    json_field_opt_bool(buf, "charging_hv", bms.charging_hv, true);
    json_field_opt_u16(buf, "current_at_eoc_ma", bms.current_at_eoc_ma, true);
    json_field_opt_u16(buf, "da_configuration", bms.da_configuration, true);
    json_field_opt_u16(buf, "power_config", bms.power_config, true);
    json_field_opt_bool(buf, "emshut_en", bms.emshut_en, true);
    json_field_opt_bool(buf, "emshut_pexit_dis", bms.emshut_pexit_dis, true);
    json_field_opt_bool(buf, "emshut_exit_comm", bms.emshut_exit_comm, true);
    json_field_opt_bool(buf, "emshut_exit_vpack", bms.emshut_exit_vpack, false);
    let _ = buf.push('}');
}

fn render_diag_charger_payload<const N: usize>(
    buf: &mut String<N>,
    charger: DerivedPowerChargerSnapshot,
) {
    let _ = buf.push('{');
    json_field_bool(buf, "poll_valid", charger.poll_valid, true);
    json_field_bool(buf, "enabled", charger.enabled, true);
    json_field_bool(buf, "allow_charge", charger.allow_charge, true);
    json_field_bool(buf, "input_present", charger.input_present, true);
    json_field_bool(buf, "vbat_present", charger.vbat_present, true);
    json_field_opt_i16(buf, "ibus_adc_ma", charger.ibus_adc_ma, true);
    json_field_opt_i16(buf, "ibat_adc_ma", charger.ibat_adc_ma, true);
    json_field_opt_u16(buf, "vbus_adc_mv", charger.vbus_adc_mv, true);
    json_field_opt_u16(buf, "vbat_adc_mv", charger.vbat_adc_mv, true);
    json_field_opt_u16(buf, "vsys_adc_mv", charger.vsys_adc_mv, true);
    json_field_opt_u16(buf, "vac1_adc_mv", charger.vac1_adc_mv, true);
    json_field_opt_u16(buf, "vac2_adc_mv", charger.vac2_adc_mv, true);
    json_field_opt_u16(buf, "vreg_mv", charger.vreg_mv, true);
    json_field_opt_u16(buf, "ichg_ma", charger.ichg_ma, true);
    json_field_opt_u16(buf, "vindpm_mv", charger.vindpm_mv, true);
    json_field_opt_u16(buf, "iindpm_ma", charger.iindpm_ma, true);
    json_field_opt_u8(buf, "st0", charger.st0, true);
    json_field_opt_u8(buf, "st1", charger.st1, true);
    json_field_opt_u8(buf, "st2", charger.st2, true);
    json_field_opt_u8(buf, "st3", charger.st3, true);
    json_field_opt_u8(buf, "st4", charger.st4, true);
    json_field_opt_u8(buf, "fault0", charger.fault0, true);
    json_field_opt_u8(buf, "fault1", charger.fault1, true);
    json_field_opt_u8(buf, "ctrl0", charger.ctrl0, true);
    json_field_opt_u8(buf, "ctrl3", charger.ctrl3, true);
    json_field_opt_u8(buf, "ctrl4", charger.ctrl4, true);
    json_field_str(buf, "acdrv_path", charger.acdrv_path, true);
    json_field_opt_u16(buf, "term_ctrl", charger.term_ctrl, false);
    let _ = buf.push('}');
}

fn render_diag_output_payload<const N: usize>(
    buf: &mut String<N>,
    state: &str,
    enabled: Option<bool>,
    vbus_mv: Option<u16>,
    iout_ma: Option<i32>,
) {
    let _ = buf.push('{');
    json_field_str(buf, "state", state, true);
    json_field_opt_bool(buf, "enabled", enabled, true);
    json_field_opt_u16(buf, "vbus_mv", vbus_mv, true);
    json_field_opt_i32(buf, "iout_ma", iout_ma, false);
    let _ = buf.push('}');
}

fn render_diag_ina_payload<const N: usize>(buf: &mut String<N>, status: UpsStatusSnapshot) {
    let _ = buf.push('{');
    json_field_opt_u16(buf, "input_vbus_mv", status.input_vbus_mv, true);
    json_field_opt_i32(buf, "input_ibus_ma", status.input_ibus_ma, true);
    json_field_opt_u16(buf, "vin_vbus_mv", status.vin_vbus_mv, true);
    json_field_opt_i32(buf, "vin_iin_ma", status.vin_iin_ma, true);
    json_field_opt_i32(buf, "tps_total_iout_ma", status.tps_total_iout_ma, false);
    let _ = buf.push('}');
}

fn render_diag_tmp_payload<const N: usize>(
    buf: &mut String<N>,
    state: &str,
    temp_c_x16: Option<i16>,
) {
    let _ = buf.push('{');
    json_field_str(buf, "state", state, true);
    json_field_opt_i16(buf, "temp_c_x16", temp_c_x16, false);
    let _ = buf.push('}');
}

fn render_diag_fusb_payload<const N: usize>(buf: &mut String<N>, diag: DerivedPowerSnapshot) {
    let _ = buf.push('{');
    json_field_bool(buf, "attached", diag.input.usb_pd_attached, true);
    json_field_opt_bool(buf, "vbus_present", diag.input.usb_pd_vbus_present, true);
    json_field_opt_u16(buf, "vac1_mv", diag.input.usb_pd_vac1_mv, false);
    let _ = buf.push('}');
}

fn render_diag_usbpd_policy_payload<const N: usize>(
    buf: &mut String<N>,
    diag: DerivedPowerSnapshot,
) {
    let _ = buf.push('{');
    json_field_bool(buf, "attached", diag.input.usb_pd_attached, true);
    json_field_bool(buf, "charge_ready", diag.input.usb_pd_charge_ready, true);
    json_field_bool(
        buf,
        "unsafe_source_latched",
        diag.input.usb_pd_unsafe_source_latched,
        true,
    );
    json_field_opt_str(buf, "contract_kind", diag.input.usb_pd_contract_kind, true);
    json_field_opt_u16(buf, "contract_mv", diag.input.usb_pd_contract_mv, true);
    json_field_opt_u16(buf, "contract_ma", diag.input.usb_pd_contract_ma, false);
    let _ = buf.push('}');
}

fn render_diag_front_panel_payload<const N: usize>(buf: &mut String<N>, status: UpsStatusSnapshot) {
    let _ = buf.push('{');
    json_field_str(buf, "init_state", status.front_panel.init_state, true);
    json_field_str(
        buf,
        "display_power_mode",
        status.front_panel.display_power_mode,
        true,
    );
    json_field_str(buf, "ui_variant", status.front_panel.ui_variant, true);
    json_field_u32(buf, "frame_no", status.front_panel.frame_no, true);
    json_field_bool(buf, "ready", status.front_panel.ready, true);
    json_field_bool(buf, "needs_redraw", status.front_panel.needs_redraw, true);
    json_field_bool(
        buf,
        "attention_hold",
        status.front_panel.attention_hold,
        false,
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

fn json_field_bool<const N: usize>(
    buf: &mut String<N>,
    key: &str,
    value: bool,
    trailing_comma: bool,
) {
    let _ = buf.push('"');
    let _ = buf.push_str(key);
    let _ = buf.push_str("\":");
    let _ = buf.push_str(if value { "true" } else { "false" });
    if trailing_comma {
        let _ = buf.push(',');
    }
}

fn json_field_u32<const N: usize>(
    buf: &mut String<N>,
    key: &str,
    value: u32,
    trailing_comma: bool,
) {
    json_field_opt_num(buf, key, Some(value as i64), trailing_comma);
}

fn json_field_opt_u8<const N: usize>(
    buf: &mut String<N>,
    key: &str,
    value: Option<u8>,
    trailing_comma: bool,
) {
    json_field_opt_num(buf, key, value.map(|value| value as i64), trailing_comma);
}

fn json_field_opt_u16<const N: usize>(
    buf: &mut String<N>,
    key: &str,
    value: Option<u16>,
    trailing_comma: bool,
) {
    json_field_opt_num(buf, key, value.map(|value| value as i64), trailing_comma);
}

fn json_field_opt_u16_array<const N: usize>(
    buf: &mut String<N>,
    key: &str,
    value: [Option<u16>; 4],
    trailing_comma: bool,
) {
    let _ = write!(buf, "\"{}\":[", key);
    for (index, item) in value.iter().enumerate() {
        if index != 0 {
            let _ = buf.push(',');
        }
        if let Some(item) = item {
            let _ = write!(buf, "{}", item);
        } else {
            let _ = buf.push_str("null");
        }
    }
    let _ = buf.push(']');
    if trailing_comma {
        let _ = buf.push(',');
    }
}

fn json_field_opt_u8_array<const N: usize>(
    buf: &mut String<N>,
    key: &str,
    value: Option<&[u8]>,
    trailing_comma: bool,
) {
    let _ = write!(buf, "\"{}\":", key);
    if let Some(values) = value {
        let _ = buf.push('[');
        for (index, item) in values.iter().enumerate() {
            if index != 0 {
                let _ = buf.push(',');
            }
            let _ = write!(buf, "{}", item);
        }
        let _ = buf.push(']');
    } else {
        let _ = buf.push_str("null");
    }
    if trailing_comma {
        let _ = buf.push(',');
    }
}

fn json_field_opt_u32<const N: usize>(
    buf: &mut String<N>,
    key: &str,
    value: Option<u32>,
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
        accepts_event_stream, render_compact_status_json, render_identity_json,
        render_settings_json, render_status_json, write_error_body, write_sse_event, BuildInfo,
    };
    use crate::{
        mdns_wire::derive_device_identity,
        net_types::{
            DeviceSettingsSnapshot, ManualChargeSettingsSnapshot, NetworkUiSummary,
            UpsStatusSnapshot, WifiConnectionState, WifiSettingsSnapshot, WifiSnapshot,
        },
    };
    use heapless::String;
    use serde_json::Value;

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
            r#"{"error":{"code":"unavailable","message":"wifi down","retryable":true,"details":null}}"#
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
                features: "web_serial",
            },
        );
        assert!(body
            .as_str()
            .contains("\"device_id\":\"mains-aegis-123456\""));
        assert!(body.as_str().contains("\"dns_sd\":true"));
        assert!(body.as_str().contains("\"ipv4\":\"192.168.31.15\""));
        assert!(body.as_str().contains(
            "\"hardware_capabilities\":{\"output_profile\":\"12v\",\"rated_vout_mv\":12000}"
        ));
        let parsed =
            serde_json::from_str::<Value>(body.as_str()).expect("identity JSON should be valid");
        assert_eq!(
            parsed["hardware_capabilities"]["output_profile"].as_str(),
            Some("12v")
        );
        assert_eq!(
            parsed["hardware_capabilities"]["rated_vout_mv"].as_u64(),
            Some(12_000)
        );
    }

    #[test]
    fn status_json_keeps_network_summary() {
        let mut body = String::<2048>::new();
        let mut status = UpsStatusSnapshot::empty();
        status.mode = "backup";
        status.input_source = "dcin";
        status.input_pressure_state = "limited";
        status.input_pressure_score_pct = Some(88);
        status.input_pressure_reason = Some("vindpm");
        status.tps_total_iout_ma = Some(128);
        status.tps_limit_threshold_ma = Some(100);
        status.input_vin_baseline_mv = Some(19_400);
        status.input_vin_drop_mv = Some(920);
        status.assist_power_stage = Some("assist_rated");
        status.assist_target_vout_mv = Some(12_000);
        status.charger_policy_target_ichg_ma = Some(300);
        status.charger_limit_active = Some(true);
        status.charger_limit_reason = Some("pressure_vindpm");
        status.charger_limit_threshold_ma = Some(100);
        status.charger_detail_status = Some("LIMIT");
        status.battery_cell_mv = [Some(3812), Some(3817), Some(3809), Some(3822)];
        status.battery_cell_delta_mv = Some(13);
        status.battery_balance_enabled = Some(true);
        status.battery_balance_cfg_match = Some(true);
        status.battery_balance_active = Some(true);
        status.battery_balance_mask = Some(0b1010);
        status.battery_balance_cell = None;
        status.battery_balance_min_start_delta_mv = Some(3);
        status.battery_charge_fet_on = Some(false);
        status.battery_discharge_fet_on = Some(true);
        status.battery_precharge_fet_on = Some(false);
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
        assert!(body.as_str().contains("\"source\":\"dcin\""));
        assert!(body.as_str().contains("\"pressure_state\":\"limited\""));
        assert!(body.as_str().contains("\"pressure_score_pct\":88"));
        assert!(body.as_str().contains("\"pressure_reason\":\"vindpm\""));
        assert!(body.as_str().contains("\"tps_total_iout_ma\":128"));
        assert!(body.as_str().contains("\"tps_limit_threshold_ma\":100"));
        assert!(body.as_str().contains("\"vin_baseline_mv\":19400"));
        assert!(body.as_str().contains("\"vin_drop_mv\":920"));
        assert!(body
            .as_str()
            .contains("\"assist_power_stage\":\"assist_rated\""));
        assert!(body.as_str().contains("\"assist_target_vout_mv\":12000"));
        assert!(body.as_str().contains("\"policy_target_ichg_ma\":300"));
        assert!(body.as_str().contains("\"limit_active\":true"));
        assert!(body
            .as_str()
            .contains("\"limit_reason\":\"pressure_vindpm\""));
        assert!(body.as_str().contains("\"limit_threshold_ma\":100"));
        assert!(body.as_str().contains("\"detail_status\":\"LIMIT\""));
        assert!(body.as_str().contains("\"cell_mv\":[3812,3817,3809,3822]"));
        assert!(body.as_str().contains("\"cell_delta_mv\":13"));
        assert!(body.as_str().contains("\"balance_enabled\":true"));
        assert!(body.as_str().contains("\"balance_cfg_match\":true"));
        assert!(body.as_str().contains("\"balance_active\":true"));
        assert!(body.as_str().contains("\"balance_mask\":10"));
        assert!(body.as_str().contains("\"balance_min_start_delta_mv\":3"));
        assert!(body.as_str().contains("\"charge_fet_on\":false"));
        assert!(body.as_str().contains("\"discharge_fet_on\":true"));
        assert!(body.as_str().contains("\"precharge_fet_on\":false"));
        assert!(body.as_str().contains("\"last_error\":\"link_lost\""));
    }

    #[test]
    fn compact_status_json_keeps_hil_observation_fields() {
        let mut body = String::<1536>::new();
        let mut status = UpsStatusSnapshot::empty();
        status.mode = "supplement";
        status.input_source = "dcin";
        status.mains_present = Some(true);
        status.vin_vbus_mv = Some(11_920);
        status.vin_iin_ma = Some(2_900);
        status.tps_total_iout_ma = Some(840);
        status.input_vin_baseline_mv = Some(12_020);
        status.input_vin_drop_mv = Some(100);
        status.assist_power_stage = Some("assist_low");
        status.assist_target_vout_mv = Some(11_400);
        status.out_a_vbus_mv = Some(11_380);
        status.out_b_vbus_mv = Some(11_390);
        status.charger_allow_charge = Some(true);
        status.charger_limit_reason = Some("none");
        status.battery_current_ma = Some(-720);
        status.battery_cell_mv = [Some(3812), Some(3817), Some(3809), Some(3822)];
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

        render_compact_status_json(&mut body, status);

        assert!(body.as_str().contains("\"mode\":\"supplement\""));
        assert!(body.as_str().contains("\"vin_vbus_mv\":11920"));
        assert!(body.as_str().contains("\"vin_iin_ma\":2900"));
        assert!(body.as_str().contains("\"tps_total_iout_ma\":840"));
        assert!(body.as_str().contains("\"vin_baseline_mv\":12020"));
        assert!(body.as_str().contains("\"vin_drop_mv\":100"));
        assert!(body
            .as_str()
            .contains("\"assist_power_stage\":\"assist_low\""));
        assert!(body.as_str().contains("\"assist_target_vout_mv\":11400"));
        assert!(body.as_str().contains("\"vbus_mv\":11380"));
        assert!(body.as_str().contains("\"current_ma\":-720"));
        assert!(!body.as_str().contains("\"cell_mv\""));
        assert!(!body.as_str().contains("\"network\""));
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

    #[test]
    fn settings_json_redacts_psk_and_exposes_manual_charge() {
        let mut body = String::<3072>::new();
        let mut ssid = String::<32>::new();
        ssid.push_str("LabNet").unwrap();
        render_settings_json(
            &mut body,
            &DeviceSettingsSnapshot {
                wifi: WifiSettingsSnapshot {
                    configured: true,
                    ssid: Some(ssid),
                },
                log_level: "debug",
                manual_charge: ManualChargeSettingsSnapshot {
                    target: "rsoc_80",
                    speed: "ma_500",
                    timer_h: 2,
                },
                advanced_power: crate::net_types::AdvancedPowerSettingsSnapshot::defaults(),
                advanced_power_capabilities:
                    crate::net_types::AdvancedPowerCapabilitiesSnapshot::for_rated_vout(12_000),
            },
        );
        let value: serde_json::Value = serde_json::from_str(body.as_str())
            .expect("settings JSON should fit the production buffer and remain valid JSON");
        assert!(body.as_str().contains("\"configured\":true"));
        assert!(body.as_str().contains("\"ssid\":\"LabNet\""));
        assert!(body.as_str().contains("\"log_level\":\"debug\""));
        assert!(body.as_str().contains("\"target\":\"rsoc_80\""));
        assert!(body.as_str().contains("\"advanced_power\":{"));
        assert!(body.as_str().contains("\"advanced_power_capabilities\":{"));
        assert!(!body.as_str().contains("psk"));
        assert_eq!(value["wifi"]["ssid"], "LabNet");
        assert_eq!(value["manual_charge"]["target"], "rsoc_80");
        assert!(value.get("advanced_power").is_some());
        assert!(value.get("advanced_power_capabilities").is_some());
    }
}
