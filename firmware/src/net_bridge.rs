use esp_firmware::net_types::{
    ChargeControlSnapshot, NetworkUiSummary, UpsStatusSnapshot, WifiSnapshot,
};
use esp_firmware::output_state::{EnabledOutputs, OutputSelector};

use crate::front_panel_scene::SelfCheckUiSnapshot;
use crate::front_panel_scene::{BmsResultKind, DashboardInputSource, SelfCheckCommState, UpsMode};

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

fn current_front_panel_runtime_summary() -> esp_firmware::net_types::FrontPanelRuntimeSnapshot {
    #[cfg(feature = "net_http")]
    {
        return esp_firmware::net::current_front_panel_runtime();
    }

    #[cfg(not(feature = "net_http"))]
    {
        esp_firmware::net_types::FrontPanelRuntimeSnapshot::unavailable()
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
        input_source: snapshot
            .dashboard_detail
            .input_source
            .map(input_source_slug)
            .unwrap_or("unknown"),
        input_vbus_mv: snapshot.input_vbus_mv,
        input_ibus_ma: snapshot.input_ibus_ma,
        mains_present: if snapshot.dashboard_detail.input_gate_state == Some("cutoff") {
            Some(false)
        } else {
            snapshot
                .vin_vbus_mv
                .map(|mv| mv >= 3_000)
                .or(snapshot.vin_mains_present)
                .or(snapshot.aggregate_input_present)
        },
        pre_tps_vin_mv: snapshot.vin_vbus_mv,
        vin_vbus_mv: snapshot.vin_vbus_mv,
        input_gate_state: snapshot.dashboard_detail.input_gate_state,
        input_gate_reason: snapshot.dashboard_detail.input_gate_reason,
        input_power_good: snapshot.dashboard_detail.input_power_good,
        vin_iin_ma: snapshot.vin_iin_ma,
        tps_total_iout_ma: snapshot.dashboard_detail.input_tps_total_iout_ma,
        tps_limit_threshold_ma: snapshot.dashboard_detail.input_tps_limit_threshold_ma,
        input_pressure_state: snapshot
            .dashboard_detail
            .input_pressure_state
            .unwrap_or("inactive"),
        input_pressure_score_pct: snapshot.dashboard_detail.input_pressure_score_pct,
        input_pressure_reason: snapshot.dashboard_detail.input_pressure_reason,
        input_vin_baseline_mv: snapshot.dashboard_detail.input_vin_baseline_mv,
        input_vin_drop_mv: snapshot.dashboard_detail.input_vin_drop_mv,
        assist_power_stage: snapshot.dashboard_detail.assist_power_stage,
        assist_target_vout_mv: snapshot.dashboard_detail.assist_target_vout_mv,
        backup_reason: snapshot.dashboard_detail.backup_reason,
        charger_state: comm_state_slug(snapshot.bq25792),
        charger_allow_charge: snapshot.bq25792_allow_charge,
        charger_ichg_ma: snapshot.bq25792_ichg_ma,
        charger_ibat_ma: snapshot.bq25792_ibat_ma,
        charger_vbat_present: snapshot.bq25792_vbat_present,
        charger_policy_target_ichg_ma: snapshot.dashboard_detail.charger_policy_target_ichg_ma,
        charger_limit_active: snapshot.dashboard_detail.charger_limit_active,
        charger_limit_reason: snapshot.dashboard_detail.charger_limit_reason,
        charger_limit_detail: snapshot.dashboard_detail.charger_limit_detail,
        charger_limit_threshold_ma: snapshot.dashboard_detail.charger_limit_threshold_ma,
        charger_detail_status: snapshot.dashboard_detail.charger_detail_status,
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
        charge_control: ChargeControlSnapshot {
            mode: if snapshot.dashboard_detail.manual_charge.runtime.active {
                "manual"
            } else {
                "auto"
            },
            manual_active: snapshot.dashboard_detail.manual_charge.runtime.active,
            takeover: snapshot.dashboard_detail.manual_charge.runtime.takeover,
            stop_inhibit: snapshot.dashboard_detail.manual_charge.runtime.stop_inhibit,
            last_stop_reason: manual_charge_stop_reason_slug(
                snapshot
                    .dashboard_detail
                    .manual_charge
                    .runtime
                    .last_stop_reason,
            ),
            remaining_minutes: snapshot
                .dashboard_detail
                .manual_charge
                .runtime
                .remaining_minutes,
            requested_power_path: manual_charge_power_path_slug(
                snapshot
                    .dashboard_detail
                    .manual_charge
                    .runtime
                    .requested_power_path,
            ),
            bound_power_path: snapshot
                .dashboard_detail
                .manual_charge
                .runtime
                .bound_power_path
                .map(input_source_slug),
            binding_reason: snapshot
                .dashboard_detail
                .manual_charge
                .runtime
                .binding_reason,
            start_state: snapshot.dashboard_detail.manual_charge.runtime.start_state,
            start_block_reason: snapshot
                .dashboard_detail
                .manual_charge
                .runtime
                .start_block_reason,
            loop_confirmation_required: snapshot
                .dashboard_detail
                .manual_charge
                .runtime
                .loop_confirmation_required,
            loop_override_active: snapshot
                .dashboard_detail
                .manual_charge
                .runtime
                .loopback_override,
            output_power_w10: snapshot
                .dashboard_detail
                .manual_charge
                .runtime
                .output_power_w10,
            power_telemetry_fresh: snapshot
                .dashboard_detail
                .manual_charge
                .runtime
                .power_telemetry_fresh,
        },
        front_panel: current_front_panel_runtime_summary(),
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
        UpsMode::Blocked => "blocked",
    }
}

fn input_source_slug(source: DashboardInputSource) -> &'static str {
    match source {
        DashboardInputSource::DcIn => "dcin",
        DashboardInputSource::UsbC => "usbc",
        DashboardInputSource::Auto => "auto",
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

fn manual_charge_power_path_slug(
    path: crate::front_panel_scene::ManualChargePowerPath,
) -> &'static str {
    match path {
        crate::front_panel_scene::ManualChargePowerPath::Auto => "auto",
        crate::front_panel_scene::ManualChargePowerPath::DcIn => "dcin",
        crate::front_panel_scene::ManualChargePowerPath::UsbC => "usbc",
    }
}

fn manual_charge_stop_reason_slug(
    reason: crate::front_panel_scene::ManualChargeStopReason,
) -> &'static str {
    match reason {
        crate::front_panel_scene::ManualChargeStopReason::None => "none",
        crate::front_panel_scene::ManualChargeStopReason::UserStop => "user_stop",
        crate::front_panel_scene::ManualChargeStopReason::TimerExpired => "timer_expired",
        crate::front_panel_scene::ManualChargeStopReason::PackReached => "pack_reached",
        crate::front_panel_scene::ManualChargeStopReason::RsocReached => "rsoc_reached",
        crate::front_panel_scene::ManualChargeStopReason::FullReached => "full_reached",
        crate::front_panel_scene::ManualChargeStopReason::SafetyBlocked => "safety_blocked",
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
    use super::{build_status_snapshot, outputs_slug};
    use crate::front_panel_scene::{
        DashboardDetailSnapshot, SelfCheckCommState, SelfCheckUiSnapshot, UpsMode,
    };
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

    #[test]
    fn status_snapshot_preserves_raw_tps_total_iout_when_runtime_charge_is_blocked() {
        let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Supplement);
        snapshot.requested_outputs = EnabledOutputs::Both;
        snapshot.active_outputs = EnabledOutputs::Both;
        snapshot.recoverable_outputs = EnabledOutputs::Both;
        snapshot.vin_vbus_mv = Some(12_032);
        snapshot.vin_iin_ma = Some(28);
        snapshot.bq25792 = SelfCheckCommState::Ok;
        snapshot.bq25792_allow_charge = Some(false);
        snapshot.dashboard_detail = DashboardDetailSnapshot {
            input_tps_total_iout_ma: Some(136),
            input_tps_limit_threshold_ma: Some(100),
            charger_detail_status: Some("LOAD"),
            ..DashboardDetailSnapshot::pending()
        };

        let status = build_status_snapshot(snapshot);
        assert_eq!(status.mode, "supplement");
        assert_eq!(status.tps_total_iout_ma, Some(136));
        assert_eq!(status.tps_limit_threshold_ma, Some(100));
        assert_eq!(status.charger_allow_charge, Some(false));
        assert_eq!(status.charger_detail_status, Some("LOAD"));
    }
}
