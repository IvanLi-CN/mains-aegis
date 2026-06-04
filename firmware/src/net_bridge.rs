use esp_firmware::net_types::NetworkUiSummary;
use esp_firmware::net_types::UpsStatusSnapshot;
use esp_firmware::net_types::WifiSnapshot;
use esp_firmware::output_state::{EnabledOutputs, OutputSelector};

use crate::front_panel_scene::SelfCheckUiSnapshot;
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

pub fn current_wifi_snapshot() -> WifiSnapshot {
    #[cfg(feature = "net_http")]
    {
        return esp_firmware::net::current_wifi_snapshot();
    }

    #[cfg(not(feature = "net_http"))]
    {
        WifiSnapshot::disabled()
    }
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
        battery_cell_mv: snapshot.dashboard_detail.cell_mv,
        battery_cell_delta_mv: cell_delta_mv(snapshot.dashboard_detail.cell_mv),
        battery_balance_enabled: snapshot.dashboard_detail.balance_enabled,
        battery_balance_cfg_match: snapshot.dashboard_detail.balance_cfg_match,
        battery_balance_active: snapshot.dashboard_detail.balance_active,
        battery_balance_mask: snapshot.dashboard_detail.balance_mask,
        battery_balance_cell: snapshot.dashboard_detail.balance_cell,
        battery_balance_min_start_delta_mv: snapshot.dashboard_detail.balance_min_start_delta_mv,
        battery_no_battery: snapshot.bq40z50_no_battery,
        battery_discharge_ready: snapshot.bq40z50_discharge_ready,
        battery_charge_fet_on: snapshot.dashboard_detail.charge_fet_on,
        battery_discharge_fet_on: snapshot.dashboard_detail.discharge_fet_on,
        battery_precharge_fet_on: snapshot.dashboard_detail.precharge_fet_on,
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
        network: current_network_summary(),
    }
}

fn cell_delta_mv(cells: [Option<u16>; 4]) -> Option<u16> {
    let mut min_mv: Option<u16> = None;
    let mut max_mv: Option<u16> = None;
    for cell in cells.into_iter().flatten() {
        min_mv = Some(min_mv.map_or(cell, |min| min.min(cell)));
        max_mv = Some(max_mv.map_or(cell, |max| max.max(cell)));
    }
    Some(max_mv?.saturating_sub(min_mv?))
}

fn mode_slug(mode: UpsMode) -> &'static str {
    match mode {
        UpsMode::Off => "off",
        UpsMode::Standby => "standby",
        UpsMode::Supplement => "supplement",
        UpsMode::Backup => "backup",
    }
}

#[cfg_attr(not(any(feature = "net_http", test)), allow(dead_code))]
fn outputs_slug(outputs: EnabledOutputs) -> &'static str {
    match outputs {
        EnabledOutputs::None => "none",
        EnabledOutputs::Only(OutputSelector::OutA) => "out_a",
        EnabledOutputs::Only(OutputSelector::OutB) => "out_b",
        EnabledOutputs::Both => "both",
    }
}

fn comm_state_slug(state: SelfCheckCommState) -> &'static str {
    match state {
        SelfCheckCommState::Pending => "pending",
        SelfCheckCommState::Ok => "ok",
        SelfCheckCommState::Warn => "warn",
        SelfCheckCommState::Err => "err",
        SelfCheckCommState::NotAvailable => "not_available",
    }
}

fn bms_result_slug(kind: BmsResultKind) -> &'static str {
    match kind {
        BmsResultKind::Success => "success",
        BmsResultKind::NoBattery => "no_battery",
        BmsResultKind::RomMode => "rom_mode",
        BmsResultKind::Abnormal => "abnormal",
        BmsResultKind::NotDetected => "not_detected",
    }
}

#[cfg(test)]
mod tests {
    use super::outputs_slug;
    use esp_firmware::output_state::{EnabledOutputs, OutputSelector};

    #[test]
    fn outputs_slug_uses_frozen_dual_output_contract_value() {
        assert_eq!(outputs_slug(EnabledOutputs::Both), "both");
        assert_eq!(
            outputs_slug(EnabledOutputs::Only(OutputSelector::OutA)),
            "out_a"
        );
        assert_eq!(
            outputs_slug(EnabledOutputs::Only(OutputSelector::OutB)),
            "out_b"
        );
    }
}
