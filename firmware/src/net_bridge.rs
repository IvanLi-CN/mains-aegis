use esp_firmware::net_types::NetworkUiSummary;

#[cfg(feature = "net_http")]
use esp_firmware::net_types::UpsStatusSnapshot;
#[cfg(feature = "net_http")]
use esp_firmware::output_state::{EnabledOutputs, OutputSelector};

use crate::front_panel_scene::SelfCheckUiSnapshot;
#[cfg(feature = "net_http")]
use crate::front_panel_scene::{BmsResultKind, SelfCheckCommState, UpsMode};

pub fn current_network_summary() -> NetworkUiSummary {
    #[cfg(feature = "net_http")]
    {
        return esp_firmware::net::current_network_ui_summary();
    }

    #[cfg(not(feature = "net_http"))]
    {
        NetworkUiSummary::disabled()
    }
}

pub fn apply_live_network_summary(mut snapshot: SelfCheckUiSnapshot) -> SelfCheckUiSnapshot {
    snapshot.network_summary = current_network_summary();
    snapshot
}

pub fn publish_status_snapshot(snapshot: SelfCheckUiSnapshot) {
    #[cfg(feature = "net_http")]
    {
        esp_firmware::net::publish_ups_status(build_status_snapshot(snapshot));
    }

    #[cfg(not(feature = "net_http"))]
    {
        let _ = snapshot;
    }
}

#[cfg(feature = "net_http")]
pub fn build_status_snapshot(snapshot: SelfCheckUiSnapshot) -> UpsStatusSnapshot {
    UpsStatusSnapshot {
        mode: mode_slug(snapshot.mode),
        requested_outputs: outputs_slug(snapshot.requested_outputs),
        active_outputs: outputs_slug(snapshot.active_outputs),
        recoverable_outputs: outputs_slug(snapshot.recoverable_outputs),
        output_gate_reason: snapshot.output_gate_reason.as_str(),
        input_vbus_mv: snapshot.input_vbus_mv,
        input_ibus_ma: snapshot.input_ibus_ma,
        mains_present: snapshot.vin_mains_present,
        vin_vbus_mv: snapshot.vin_vbus_mv,
        vin_iin_ma: snapshot.vin_iin_ma,
        charger_state: comm_state_slug(snapshot.bq25792),
        charger_allow_charge: snapshot.bq25792_allow_charge,
        charger_ichg_ma: snapshot.bq25792_ichg_ma,
        charger_ibat_ma: snapshot.bq25792_ibat_ma,
        charger_vbat_present: snapshot.bq25792_vbat_present,
        battery_state: comm_state_slug(snapshot.bq40z50),
        battery_pack_mv: snapshot.bq40z50_pack_mv,
        battery_current_ma: snapshot.bq40z50_current_ma,
        battery_soc_pct: snapshot.bq40z50_soc_pct,
        battery_no_battery: snapshot.bq40z50_no_battery,
        battery_discharge_ready: snapshot.bq40z50_discharge_ready,
        battery_issue_detail: snapshot.bq40z50_issue_detail,
        battery_recovery_pending: snapshot.bq40z50_recovery_pending,
        battery_last_result: snapshot.bq40z50_last_result.map(bms_result_slug),
        out_a_state: comm_state_slug(snapshot.tps_a),
        out_a_enabled: snapshot.tps_a_enabled,
        out_a_vbus_mv: snapshot.out_a_vbus_mv,
        out_a_iout_ma: snapshot.tps_a_iout_ma,
        out_b_state: comm_state_slug(snapshot.tps_b),
        out_b_enabled: snapshot.tps_b_enabled,
        out_b_vbus_mv: snapshot.out_b_vbus_mv,
        out_b_iout_ma: snapshot.tps_b_iout_ma,
        tmp_a_state: comm_state_slug(snapshot.tmp_a),
        tmp_a_c: snapshot.tmp_a_c,
        tmp_b_state: comm_state_slug(snapshot.tmp_b),
        tmp_b_c: snapshot.tmp_b_c,
        network: snapshot.network_summary,
    }
}

#[cfg(feature = "net_http")]
fn mode_slug(mode: UpsMode) -> &'static str {
    match mode {
        UpsMode::Off => "off",
        UpsMode::Standby => "standby",
        UpsMode::Supplement => "supplement",
        UpsMode::Backup => "backup",
    }
}

#[cfg(feature = "net_http")]
fn outputs_slug(outputs: EnabledOutputs) -> &'static str {
    match outputs {
        EnabledOutputs::None => "none",
        EnabledOutputs::Only(OutputSelector::OutA) => "out_a",
        EnabledOutputs::Only(OutputSelector::OutB) => "out_b",
        EnabledOutputs::Both => "out_a+out_b",
    }
}

#[cfg(feature = "net_http")]
fn comm_state_slug(state: SelfCheckCommState) -> &'static str {
    match state {
        SelfCheckCommState::Pending => "pending",
        SelfCheckCommState::Ok => "ok",
        SelfCheckCommState::Warn => "warn",
        SelfCheckCommState::Err => "err",
        SelfCheckCommState::NotAvailable => "not_available",
    }
}

#[cfg(feature = "net_http")]
fn bms_result_slug(kind: BmsResultKind) -> &'static str {
    match kind {
        BmsResultKind::Success => "success",
        BmsResultKind::NoBattery => "no_battery",
        BmsResultKind::RomMode => "rom_mode",
        BmsResultKind::Abnormal => "abnormal",
        BmsResultKind::NotDetected => "not_detected",
    }
}
