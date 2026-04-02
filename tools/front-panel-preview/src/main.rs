use std::{
    convert::Infallible,
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process,
};

use image::{Rgb, RgbImage};

extern crate self as esp_firmware;

#[path = "../../../firmware/src/output_state.rs"]
pub mod output_state;

#[path = "../../../firmware/src/front_panel_scene.rs"]
mod front_panel_scene;

use front_panel_scene::{
    demo_mode_from_focus, AudioTestUiState, BmsRecoveryUiAction, BmsResultKind,
    DashboardDetailPage, DashboardDetailSnapshot, DashboardInputSource, DashboardRoute,
    DisplayDiagnosticMeta, SelfCheckCommState, SelfCheckOverlay, SelfCheckUiSnapshot,
    TestFunctionUi, TpsTestChargerSnapshot, TpsTestOutputSnapshot, TpsTestUiSnapshot,
    TpsTestVoutProfile, UiFocus, UiModel, UiPainter, UiVariant, UpsMode, UI_H, UI_W,
};

#[allow(dead_code)]
fn base_bq40_snapshot(mode: UpsMode) -> SelfCheckUiSnapshot {
    let mut snapshot = SelfCheckUiSnapshot::pending(mode);
    snapshot.gc9307 = SelfCheckCommState::Ok;
    snapshot.tca6408a = SelfCheckCommState::Ok;
    snapshot.fusb302 = SelfCheckCommState::Ok;
    snapshot.fusb302_vbus_present = Some(true);
    snapshot.input_vbus_mv = Some(19_240);
    snapshot.input_ibus_ma = Some(1180);
    snapshot.vin_vbus_mv = Some(19_240);
    snapshot.vin_iin_ma = Some(1180);
    snapshot.ina3221 = SelfCheckCommState::Ok;
    snapshot.ina_total_ma = Some(1130);
    snapshot.bq25792 = SelfCheckCommState::Ok;
    snapshot.bq25792_allow_charge = Some(true);
    snapshot.bq25792_ichg_ma = Some(520);
    snapshot.bq25792_ibat_ma = Some(510);
    snapshot.bq25792_vbat_present = Some(true);
    snapshot.bq40z50 = SelfCheckCommState::Err;
    snapshot.bq40z50_pack_mv = None;
    snapshot.bq40z50_current_ma = None;
    snapshot.bq40z50_soc_pct = None;
    snapshot.bq40z50_rca_alarm = None;
    snapshot.bq40z50_issue_detail = None;
    snapshot.bq40z50_recovery_action = Some(BmsRecoveryUiAction::Activation);
    snapshot.bq40z50_discharge_ready = None;
    snapshot.bq40z50_last_result = None;
    snapshot.tps_a = SelfCheckCommState::Ok;
    snapshot.tps_a_enabled = Some(true);
    snapshot.out_a_vbus_mv = Some(19_020);
    snapshot.tps_a_iout_ma = Some(430);
    snapshot.tps_b = SelfCheckCommState::Ok;
    snapshot.tps_b_enabled = Some(false);
    snapshot.out_b_vbus_mv = Some(19_010);
    snapshot.tps_b_iout_ma = Some(0);
    snapshot.tmp_a = SelfCheckCommState::Ok;
    snapshot.tmp_a_c = Some(39);
    snapshot.tmp_b = SelfCheckCommState::Ok;
    snapshot.tmp_b_c = Some(37);
    snapshot.dashboard_detail = dashboard_detail_fixture(mode, None);
    snapshot
}

fn dashboard_detail_fixture(
    mode: UpsMode,
    page: Option<DashboardDetailPage>,
) -> DashboardDetailSnapshot {
    let mut detail = DashboardDetailSnapshot::pending();
    detail.cell_mv = [Some(4088), Some(4094), Some(4102), Some(4098)];
    detail.cell_temp_c = [Some(31), Some(32), Some(33), Some(31)];
    detail.balance_cell = Some(3);
    detail.battery_energy_mwh = Some(46_850);
    detail.battery_full_capacity_mwh = Some(63_200);
    detail.charge_fet_on = Some(matches!(mode, UpsMode::Standby));
    detail.discharge_fet_on = Some(matches!(mode, UpsMode::Supplement | UpsMode::Backup));
    detail.precharge_fet_on = Some(matches!(mode, UpsMode::Standby));
    detail.input_source = Some(match page {
        Some(DashboardDetailPage::Charger) => DashboardInputSource::UsbC,
        _ => DashboardInputSource::DcIn,
    });
    detail.charger_active = Some(matches!(mode, UpsMode::Standby));
    detail.charger_status = Some(if matches!(mode, UpsMode::Standby) {
        "CHG"
    } else {
        "LOCK"
    });
    detail.out_a_temp_c = Some(41);
    detail.out_b_temp_c = Some(43);
    detail.board_temp_c = Some(36);
    detail.battery_temp_c = Some(34);
    detail.fan_rpm = Some(if matches!(mode, UpsMode::Backup) {
        4120
    } else {
        2380
    });
    detail.fan_pwm_pct = Some(if matches!(mode, UpsMode::Backup) {
        100
    } else {
        52
    });
    detail.fan_status = Some(if matches!(mode, UpsMode::Backup) {
        "HIGH"
    } else {
        "MID"
    });
    detail.cells_notice = Some("CELL DELTA 14mV - BALANCE ACTIVE");
    detail.battery_notice = Some("PACK FLOW MOCKED - LIVE SOURCE NEXT");
    detail.output_notice = Some("OUT-B STANDBY PATH HELD");
    detail.charger_notice = Some("USB-C PROFILE MOCKED - DC SWITCH NEXT");
    detail.thermal_notice = Some("FAN RPM MOCKED - SENSOR WIRING NEXT");

    if matches!(page, Some(DashboardDetailPage::Output)) {
        detail.out_b_temp_c = None;
    }

    detail
}

fn dashboard_snapshot_for_mode(mode: UpsMode) -> SelfCheckUiSnapshot {
    let mut snapshot = base_bq40_snapshot(mode);
    snapshot.dashboard_detail = dashboard_detail_fixture(mode, None);
    snapshot.bq40z50 = SelfCheckCommState::Ok;
    snapshot.bq40z50_rca_alarm = Some(false);
    snapshot.bq40z50_no_battery = Some(false);
    snapshot.bq40z50_discharge_ready = Some(true);

    match mode {
        UpsMode::Off => {
            snapshot.fusb302_vbus_present = Some(true);
            snapshot.input_vbus_mv = Some(19_110);
            snapshot.input_ibus_ma = Some(1260);
            snapshot.vin_vbus_mv = Some(19_110);
            snapshot.vin_iin_ma = Some(1260);
            snapshot.bq25792_allow_charge = Some(false);
            snapshot.bq25792_ichg_ma = None;
            snapshot.bq25792_ibat_ma = Some(0);
            snapshot.tps_a_enabled = Some(false);
            snapshot.out_a_vbus_mv = None;
            snapshot.tps_a_iout_ma = None;
            snapshot.tps_b_enabled = Some(false);
            snapshot.out_b_vbus_mv = None;
            snapshot.tps_b_iout_ma = None;
            snapshot.ina_total_ma = None;
            snapshot.bq40z50_pack_mv = Some(15_180);
            snapshot.bq40z50_current_ma = Some(60);
            snapshot.bq40z50_soc_pct = Some(64);
        }
        UpsMode::Standby => {
            snapshot.fusb302_vbus_present = Some(true);
            snapshot.input_vbus_mv = Some(19_220);
            snapshot.input_ibus_ma = Some(1320);
            snapshot.vin_vbus_mv = Some(19_220);
            snapshot.vin_iin_ma = Some(1320);
            snapshot.bq25792_allow_charge = Some(true);
            snapshot.bq25792_ichg_ma = Some(540);
            snapshot.bq25792_ibat_ma = Some(520);
            snapshot.tps_a_enabled = Some(false);
            snapshot.out_a_vbus_mv = None;
            snapshot.tps_a_iout_ma = None;
            snapshot.tps_b_enabled = Some(false);
            snapshot.out_b_vbus_mv = None;
            snapshot.tps_b_iout_ma = None;
            snapshot.ina_total_ma = None;
            snapshot.bq40z50_pack_mv = Some(15_260);
            snapshot.bq40z50_current_ma = Some(520);
            snapshot.bq40z50_soc_pct = Some(67);
        }
        UpsMode::Supplement => {
            snapshot.fusb302_vbus_present = Some(true);
            snapshot.input_vbus_mv = Some(19_180);
            snapshot.input_ibus_ma = Some(820);
            snapshot.vin_vbus_mv = Some(19_180);
            snapshot.vin_iin_ma = Some(820);
            snapshot.bq25792_allow_charge = Some(false);
            snapshot.bq25792_ichg_ma = None;
            snapshot.bq25792_ibat_ma = Some(0);
            snapshot.tps_a_enabled = Some(true);
            snapshot.out_a_vbus_mv = Some(19_040);
            snapshot.tps_a_iout_ma = Some(620);
            snapshot.tps_b_enabled = Some(true);
            snapshot.out_b_vbus_mv = Some(19_000);
            snapshot.tps_b_iout_ma = Some(510);
            snapshot.ina_total_ma = Some(1130);
            snapshot.bq40z50_pack_mv = Some(14_980);
            snapshot.bq40z50_current_ma = Some(-900);
            snapshot.bq40z50_soc_pct = Some(59);
        }
        UpsMode::Backup => {
            snapshot.fusb302_vbus_present = Some(false);
            snapshot.input_vbus_mv = None;
            snapshot.input_ibus_ma = None;
            snapshot.vin_vbus_mv = None;
            snapshot.vin_iin_ma = None;
            snapshot.bq25792_allow_charge = Some(false);
            snapshot.bq25792_ichg_ma = None;
            snapshot.bq25792_ibat_ma = None;
            snapshot.tps_a_enabled = Some(true);
            snapshot.out_a_vbus_mv = Some(18_860);
            snapshot.tps_a_iout_ma = Some(980);
            snapshot.tps_b_enabled = Some(true);
            snapshot.out_b_vbus_mv = Some(18_830);
            snapshot.tps_b_iout_ma = Some(910);
            snapshot.ina_total_ma = Some(1890);
            snapshot.bq40z50_pack_mv = Some(14_820);
            snapshot.bq40z50_current_ma = Some(-1880);
            snapshot.bq40z50_soc_pct = Some(53);
        }
    }

    snapshot
}

fn dashboard_detail_snapshot_for_page(page: DashboardDetailPage) -> (UpsMode, SelfCheckUiSnapshot) {
    let mode = match page {
        DashboardDetailPage::Cells => UpsMode::Standby,
        DashboardDetailPage::BatteryFlow => UpsMode::Backup,
        DashboardDetailPage::Output => UpsMode::Supplement,
        DashboardDetailPage::Charger => UpsMode::Standby,
        DashboardDetailPage::Thermal => UpsMode::Backup,
    };
    let mut snapshot = dashboard_snapshot_for_mode(mode);
    snapshot.dashboard_detail = dashboard_detail_fixture(mode, Some(page));
    if matches!(page, DashboardDetailPage::Output) {
        snapshot.tps_b_enabled = Some(false);
        snapshot.out_b_vbus_mv = None;
        snapshot.tps_b_iout_ma = None;
    }
    if matches!(page, DashboardDetailPage::Charger) {
        snapshot.input_vbus_mv = Some(20_060);
        snapshot.input_ibus_ma = Some(1180);
        snapshot.vin_vbus_mv = Some(20_060);
        snapshot.vin_iin_ma = Some(1180);
    }
    (mode, snapshot)
}

fn dashboard_detail_snapshot_for_thermal_notice(
    thermal_notice: &'static str,
    fan_pwm_pct: u8,
    fan_status: &'static str,
    fan_rpm: Option<u16>,
) -> (UpsMode, SelfCheckUiSnapshot) {
    let mode = UpsMode::Backup;
    let mut snapshot = dashboard_snapshot_for_mode(mode);
    snapshot.dashboard_detail = dashboard_detail_fixture(mode, Some(DashboardDetailPage::Thermal));
    snapshot.dashboard_detail.thermal_notice = Some(thermal_notice);
    snapshot.dashboard_detail.fan_pwm_pct = Some(fan_pwm_pct);
    snapshot.dashboard_detail.fan_status = Some(fan_status);
    snapshot.dashboard_detail.fan_rpm = fan_rpm;
    (mode, snapshot)
}

#[derive(Clone, Copy, Debug)]
enum ChargerPolicyPreviewState {
    Wait,
    Charge500mA,
    Charge100mADcDerated,
    FullLatched,
    BlockedOutputOverload,
    BlockedNoBms,
}

fn charger_policy_snapshot_for_state(
    state: ChargerPolicyPreviewState,
) -> (UpsMode, SelfCheckUiSnapshot) {
    let mode = UpsMode::Standby;
    let mut snapshot = dashboard_snapshot_for_mode(mode);
    snapshot.dashboard_detail = dashboard_detail_fixture(mode, Some(DashboardDetailPage::Charger));
    snapshot.dashboard_detail.input_source = Some(DashboardInputSource::UsbC);
    snapshot.dashboard_detail.charger_active = Some(false);
    snapshot.dashboard_detail.charger_status = Some("WAIT");
    snapshot.dashboard_detail.charger_notice = Some("idle_wait_threshold");
    snapshot.bq40z50 = SelfCheckCommState::Ok;
    snapshot.bq40z50_rca_alarm = Some(false);
    snapshot.bq40z50_no_battery = Some(false);
    snapshot.bq40z50_discharge_ready = Some(true);
    snapshot.bq25792 = SelfCheckCommState::Ok;
    snapshot.bq25792_vbat_present = Some(true);
    snapshot.fusb302_vbus_present = Some(true);
    snapshot.input_vbus_mv = Some(20_060);
    snapshot.input_ibus_ma = Some(640);
    snapshot.vin_vbus_mv = Some(20_060);
    snapshot.vin_iin_ma = Some(640);
    snapshot.bq25792_allow_charge = Some(false);
    snapshot.bq25792_ichg_ma = None;
    snapshot.bq25792_ibat_ma = Some(0);
    snapshot.bq40z50_pack_mv = Some(15_980);
    snapshot.bq40z50_current_ma = Some(0);
    snapshot.bq40z50_soc_pct = Some(82);
    snapshot.ina_total_ma = Some(0);
    snapshot.tps_a_enabled = Some(false);
    snapshot.out_a_vbus_mv = None;
    snapshot.tps_a_iout_ma = None;
    snapshot.tps_b_enabled = Some(false);
    snapshot.out_b_vbus_mv = None;
    snapshot.tps_b_iout_ma = None;

    match state {
        ChargerPolicyPreviewState::Wait => {}
        ChargerPolicyPreviewState::Charge500mA => {
            snapshot.dashboard_detail.charger_active = Some(true);
            snapshot.dashboard_detail.charger_status = Some("CHG500");
            snapshot.dashboard_detail.charger_notice = Some("charging_500ma");
            snapshot.bq25792_allow_charge = Some(true);
            snapshot.bq25792_ichg_ma = Some(500);
            snapshot.bq25792_ibat_ma = Some(480);
            snapshot.bq40z50_current_ma = Some(500);
            snapshot.bq40z50_soc_pct = Some(67);
            snapshot.bq40z50_pack_mv = Some(15_260);
            snapshot.input_ibus_ma = Some(1_260);
            snapshot.vin_iin_ma = Some(1_260);
        }
        ChargerPolicyPreviewState::Charge100mADcDerated => {
            snapshot.dashboard_detail.input_source = Some(DashboardInputSource::DcIn);
            snapshot.dashboard_detail.charger_active = Some(true);
            snapshot.dashboard_detail.charger_status = Some("CHG100");
            snapshot.dashboard_detail.charger_notice = Some("charging_100ma_dc_derated");
            snapshot.bq25792_allow_charge = Some(true);
            snapshot.bq25792_ichg_ma = Some(100);
            snapshot.bq25792_ibat_ma = Some(95);
            snapshot.bq40z50_current_ma = Some(110);
            snapshot.bq40z50_soc_pct = Some(74);
            snapshot.bq40z50_pack_mv = Some(15_420);
            snapshot.input_ibus_ma = Some(3_150);
            snapshot.vin_iin_ma = Some(3_150);
            snapshot.ina_total_ma = Some(0);
        }
        ChargerPolicyPreviewState::FullLatched => {
            snapshot.dashboard_detail.charger_status = Some("FULL");
            snapshot.dashboard_detail.charger_notice = Some("full_latched");
            snapshot.bq25792_ibat_ma = Some(0);
            snapshot.bq40z50_current_ma = Some(0);
            snapshot.bq40z50_soc_pct = Some(100);
            snapshot.bq40z50_pack_mv = Some(16_720);
            snapshot.input_ibus_ma = Some(180);
            snapshot.vin_iin_ma = Some(180);
        }
        ChargerPolicyPreviewState::BlockedOutputOverload => {
            snapshot.dashboard_detail.charger_status = Some("LOAD");
            snapshot.dashboard_detail.charger_notice = Some("blocked_output_over_limit");
            snapshot.dashboard_detail.input_source = Some(DashboardInputSource::DcIn);
            snapshot.tps_a_enabled = Some(true);
            snapshot.out_a_vbus_mv = Some(19_040);
            snapshot.tps_a_iout_ma = Some(150);
            snapshot.tps_b_enabled = Some(true);
            snapshot.out_b_vbus_mv = Some(19_000);
            snapshot.tps_b_iout_ma = Some(140);
            snapshot.ina_total_ma = Some(290);
            snapshot.input_ibus_ma = Some(1_180);
            snapshot.vin_iin_ma = Some(1_180);
            snapshot.bq25792_ibat_ma = Some(0);
            snapshot.bq40z50_current_ma = Some(0);
            snapshot.bq40z50_soc_pct = Some(68);
            snapshot.bq40z50_pack_mv = Some(15_240);
        }
        ChargerPolicyPreviewState::BlockedNoBms => {
            snapshot.dashboard_detail.charger_status = Some("LOCK");
            snapshot.dashboard_detail.charger_notice = Some("blocked_no_bms");
            snapshot.bq40z50 = SelfCheckCommState::Warn;
            snapshot.bq25792_ibat_ma = Some(0);
            snapshot.bq40z50_discharge_ready = Some(false);
            snapshot.bq40z50_current_ma = Some(0);
            snapshot.bq40z50_soc_pct = Some(76);
            snapshot.bq40z50_pack_mv = Some(15_540);
        }
    }

    (mode, snapshot)
}

fn tps_test_snapshot_fixture() -> TpsTestUiSnapshot {
    TpsTestUiSnapshot {
        build_profile: "release",
        build_id: "preview-local",
        vout_profile: TpsTestVoutProfile::V5,
        ilim_ma: 3_500,
        charger: TpsTestChargerSnapshot {
            requested_enabled: false,
            actual_enabled: false,
            comm_state: SelfCheckCommState::Ok,
            input_present: Some(true),
            vbat_present: Some(true),
            vbat_mv: Some(12_060),
            ibat_ma: Some(0),
            vreg_mv: Some(16_800),
            ichg_ma: Some(200),
            status: "LOCK",
            fault: None,
        },
        out_a: TpsTestOutputSnapshot {
            requested_enabled: true,
            actual_enabled: Some(false),
            comm_state: SelfCheckCommState::Err,
            vset_mv: Some(5_000),
            vbus_mv: Some(0),
            iout_ma: Some(0),
            temp_c_x16: Some(32 * 16),
            status_bits: None,
            fault: Some("i2c_nack"),
        },
        out_b: TpsTestOutputSnapshot {
            requested_enabled: false,
            actual_enabled: Some(false),
            comm_state: SelfCheckCommState::NotAvailable,
            vset_mv: Some(5_000),
            vbus_mv: None,
            iout_ma: None,
            temp_c_x16: Some(31 * 16),
            status_bits: None,
            fault: None,
        },
        footer_notice: Some("FIXED PROFILE / NO TOUCH CONTROLS"),
        footer_alert: Some("OUT-A I2C NACK"),
    }
}

#[allow(dead_code)]
fn bq40_snapshot_for_scenario(
    mode: UpsMode,
    scenario: ScenarioArg,
) -> (SelfCheckUiSnapshot, SelfCheckOverlay) {
    let mut snapshot = base_bq40_snapshot(mode);
    let overlay = match scenario {
        ScenarioArg::SelfCheckBmsMissingTpsWarn => {
            snapshot.bq25792 = SelfCheckCommState::Ok;
            snapshot.bq25792_allow_charge = Some(false);
            snapshot.bq25792_vbat_present = Some(false);
            snapshot.bq40z50 = SelfCheckCommState::Err;
            snapshot.bq40z50_pack_mv = None;
            snapshot.bq40z50_current_ma = None;
            snapshot.bq40z50_soc_pct = None;
            snapshot.bq40z50_rca_alarm = None;
            snapshot.bq40z50_discharge_ready = None;
            snapshot.tps_a = SelfCheckCommState::Err;
            snapshot.tps_a_enabled = Some(false);
            snapshot.out_a_vbus_mv = None;
            snapshot.tps_a_iout_ma = None;
            snapshot.tps_b = SelfCheckCommState::Err;
            snapshot.tps_b_enabled = Some(false);
            snapshot.out_b_vbus_mv = None;
            snapshot.tps_b_iout_ma = None;
            snapshot.requested_outputs = esp_firmware::output_state::EnabledOutputs::Only(
                esp_firmware::output_state::OutputSelector::OutA,
            );
            snapshot.active_outputs = esp_firmware::output_state::EnabledOutputs::None;
            snapshot.recoverable_outputs = esp_firmware::output_state::EnabledOutputs::Only(
                esp_firmware::output_state::OutputSelector::OutA,
            );
            snapshot.output_gate_reason = esp_firmware::output_state::OutputGateReason::BmsNotReady;
            SelfCheckOverlay::None
        }
        ScenarioArg::Bq40Offline => SelfCheckOverlay::None,
        ScenarioArg::Bq40OfflineDialog => SelfCheckOverlay::BmsActivateConfirm,
        ScenarioArg::Bq40DischargeBlocked => {
            snapshot.bq40z50 = SelfCheckCommState::Warn;
            snapshot.bq40z50_pack_mv = Some(15_420);
            snapshot.bq40z50_current_ma = Some(115);
            snapshot.bq40z50_soc_pct = Some(76);
            snapshot.bq40z50_rca_alarm = Some(false);
            snapshot.bq40z50_no_battery = Some(false);
            snapshot.bq40z50_discharge_ready = Some(false);
            snapshot.bq40z50_issue_detail = Some("xdsg_blocked");
            snapshot.bq40z50_recovery_action = Some(BmsRecoveryUiAction::DischargeAuthorization);
            snapshot.requested_outputs = esp_firmware::output_state::EnabledOutputs::Only(
                esp_firmware::output_state::OutputSelector::OutA,
            );
            snapshot.active_outputs = esp_firmware::output_state::EnabledOutputs::None;
            snapshot.recoverable_outputs = esp_firmware::output_state::EnabledOutputs::Only(
                esp_firmware::output_state::OutputSelector::OutA,
            );
            snapshot.output_gate_reason = esp_firmware::output_state::OutputGateReason::BmsNotReady;
            snapshot.bq25792_allow_charge = Some(true);
            snapshot.bq25792_vbat_present = Some(true);
            snapshot.tps_a = SelfCheckCommState::Warn;
            snapshot.tps_a_enabled = Some(false);
            snapshot.out_a_vbus_mv = None;
            snapshot.tps_a_iout_ma = None;
            SelfCheckOverlay::None
        }
        ScenarioArg::Bq40DischargeDialog => {
            let (_, overlay) = bq40_snapshot_for_scenario(mode, ScenarioArg::Bq40DischargeBlocked);
            let mut blocked = base_bq40_snapshot(mode);
            blocked.bq40z50 = SelfCheckCommState::Warn;
            blocked.bq40z50_pack_mv = Some(15_420);
            blocked.bq40z50_current_ma = Some(115);
            blocked.bq40z50_soc_pct = Some(76);
            blocked.bq40z50_rca_alarm = Some(false);
            blocked.bq40z50_no_battery = Some(false);
            blocked.bq40z50_discharge_ready = Some(false);
            blocked.bq40z50_issue_detail = Some("xdsg_blocked");
            blocked.bq40z50_recovery_action = Some(BmsRecoveryUiAction::DischargeAuthorization);
            blocked.requested_outputs = esp_firmware::output_state::EnabledOutputs::Only(
                esp_firmware::output_state::OutputSelector::OutA,
            );
            blocked.active_outputs = esp_firmware::output_state::EnabledOutputs::None;
            blocked.recoverable_outputs = esp_firmware::output_state::EnabledOutputs::Only(
                esp_firmware::output_state::OutputSelector::OutA,
            );
            blocked.output_gate_reason = esp_firmware::output_state::OutputGateReason::BmsNotReady;
            blocked.bq25792_allow_charge = Some(true);
            blocked.bq25792_vbat_present = Some(true);
            blocked.tps_a = SelfCheckCommState::Warn;
            blocked.tps_a_enabled = Some(false);
            blocked.out_a_vbus_mv = None;
            blocked.tps_a_iout_ma = None;
            snapshot = blocked;
            let _ = overlay;
            SelfCheckOverlay::BmsDischargeAuthorizeConfirm
        }
        ScenarioArg::Bq40DischargeRecovering => {
            snapshot.bq40z50 = SelfCheckCommState::Warn;
            snapshot.bq40z50_pack_mv = Some(15_420);
            snapshot.bq40z50_current_ma = Some(115);
            snapshot.bq40z50_soc_pct = Some(76);
            snapshot.bq40z50_rca_alarm = Some(false);
            snapshot.bq40z50_no_battery = Some(false);
            snapshot.bq40z50_discharge_ready = Some(false);
            snapshot.bq40z50_issue_detail = Some("xdsg_blocked");
            snapshot.bq40z50_recovery_action = Some(BmsRecoveryUiAction::DischargeAuthorization);
            snapshot.bq40z50_recovery_pending = true;
            snapshot.requested_outputs = esp_firmware::output_state::EnabledOutputs::Only(
                esp_firmware::output_state::OutputSelector::OutA,
            );
            snapshot.active_outputs = esp_firmware::output_state::EnabledOutputs::None;
            snapshot.recoverable_outputs = esp_firmware::output_state::EnabledOutputs::Only(
                esp_firmware::output_state::OutputSelector::OutA,
            );
            snapshot.output_gate_reason = esp_firmware::output_state::OutputGateReason::BmsNotReady;
            snapshot.tps_a = SelfCheckCommState::Warn;
            snapshot.tps_a_enabled = Some(false);
            snapshot.out_a_vbus_mv = None;
            snapshot.tps_a_iout_ma = None;
            SelfCheckOverlay::BmsDischargeAuthorizeProgress
        }
        ScenarioArg::Bq40Activating => SelfCheckOverlay::BmsActivateProgress,
        ScenarioArg::Bq40ResultSuccess => {
            snapshot.bq40z50 = SelfCheckCommState::Ok;
            snapshot.bq40z50_soc_pct = Some(78);
            snapshot.bq40z50_rca_alarm = Some(false);
            snapshot.bq40z50_discharge_ready = Some(true);
            snapshot.bq40z50_issue_detail = None;
            snapshot.bq40z50_recovery_action = None;
            snapshot.bq25792_vbat_present = Some(true);
            snapshot.requested_outputs = esp_firmware::output_state::EnabledOutputs::Only(
                esp_firmware::output_state::OutputSelector::OutA,
            );
            snapshot.active_outputs = esp_firmware::output_state::EnabledOutputs::Only(
                esp_firmware::output_state::OutputSelector::OutA,
            );
            snapshot.recoverable_outputs = snapshot.active_outputs;
            snapshot.output_gate_reason = esp_firmware::output_state::OutputGateReason::None;
            snapshot.bq40z50_last_result = Some(BmsResultKind::Success);
            SelfCheckOverlay::None
        }
        ScenarioArg::Bq40ResultNoBattery => {
            snapshot.bq25792_vbat_present = Some(false);
            snapshot.bq40z50_issue_detail = Some("no_battery");
            snapshot.bq40z50_recovery_action = None;
            snapshot.bq40z50_last_result = Some(BmsResultKind::NoBattery);
            SelfCheckOverlay::BmsActivateResult(BmsResultKind::NoBattery)
        }
        ScenarioArg::Bq40ResultRomMode => {
            snapshot.bq40z50_issue_detail = Some("rom_mode");
            snapshot.bq40z50_recovery_action = None;
            snapshot.bq40z50_last_result = Some(BmsResultKind::RomMode);
            SelfCheckOverlay::BmsActivateResult(BmsResultKind::RomMode)
        }
        ScenarioArg::Bq40ResultAbnormal => {
            snapshot.bq40z50 = SelfCheckCommState::Warn;
            snapshot.bq40z50_soc_pct = Some(61);
            snapshot.bq40z50_rca_alarm = Some(true);
            snapshot.bq40z50_discharge_ready = Some(false);
            snapshot.bq40z50_issue_detail = Some("remaining_capacity_alarm");
            snapshot.bq40z50_recovery_action = None;
            snapshot.bq25792_vbat_present = Some(true);
            snapshot.bq40z50_last_result = Some(BmsResultKind::Abnormal);
            SelfCheckOverlay::BmsActivateResult(BmsResultKind::Abnormal)
        }
        ScenarioArg::Bq40ResultNotDetected => {
            snapshot.bq40z50_issue_detail = None;
            snapshot.bq40z50_recovery_action = Some(BmsRecoveryUiAction::Activation);
            snapshot.bq40z50_last_result = Some(BmsResultKind::NotDetected);
            SelfCheckOverlay::BmsActivateResult(BmsResultKind::NotDetected)
        }
        ScenarioArg::Default
        | ScenarioArg::DisplayDiag
        | ScenarioArg::DashboardRuntimeStandby
        | ScenarioArg::DashboardRuntimeAssist
        | ScenarioArg::DashboardRuntimeBackup
        | ScenarioArg::DashboardDetailCells
        | ScenarioArg::DashboardDetailBatteryFlow
        | ScenarioArg::DashboardDetailOutput
        | ScenarioArg::DashboardDetailCharger
        | ScenarioArg::DashboardDetailThermal
        | ScenarioArg::DashboardDetailThermalTestMode
        | ScenarioArg::DashboardDetailThermKillAsserted
        | ScenarioArg::DashboardDetailChargerWait
        | ScenarioArg::DashboardDetailCharger500mA
        | ScenarioArg::DashboardDetailCharger100mADcDerated
        | ScenarioArg::DashboardDetailChargerFullLatched
        | ScenarioArg::DashboardDetailChargerBlockedOutputOverload
        | ScenarioArg::DashboardDetailChargerBlockedNoBms
        | ScenarioArg::TpsTest
        | ScenarioArg::TestAudio
        | ScenarioArg::TestNavigation => SelfCheckOverlay::None,
    };
    (snapshot, overlay)
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = Args::parse(env::args().skip(1))?;

    if !args.out_dir.is_absolute() {
        return Err("--out-dir must be an absolute path".into());
    }

    let effective_mode = match args.scenario {
        ScenarioArg::DashboardRuntimeStandby => ModeArg::Standby,
        ScenarioArg::DashboardRuntimeAssist => ModeArg::Supplement,
        ScenarioArg::DashboardRuntimeBackup => ModeArg::Backup,
        ScenarioArg::DashboardDetailCells => ModeArg::Standby,
        ScenarioArg::DashboardDetailBatteryFlow => ModeArg::Backup,
        ScenarioArg::DashboardDetailOutput => ModeArg::Supplement,
        ScenarioArg::DashboardDetailCharger => ModeArg::Standby,
        ScenarioArg::DashboardDetailThermal => ModeArg::Backup,
        ScenarioArg::DashboardDetailChargerWait => ModeArg::Standby,
        ScenarioArg::DashboardDetailCharger500mA => ModeArg::Standby,
        ScenarioArg::DashboardDetailCharger100mADcDerated => ModeArg::Standby,
        ScenarioArg::DashboardDetailChargerFullLatched => ModeArg::Standby,
        ScenarioArg::DashboardDetailChargerBlockedNoBms => ModeArg::Standby,
        _ => args.mode,
    };

    let frame_dir = args
        .out_dir
        .join(format!("variant-{}", args.variant.as_tag()))
        .join(format!("mode-{}", effective_mode.as_tag()))
        .join(format!("focus-{}", args.focus.as_tag()))
        .join(format!("scenario-{}", args.scenario.as_tag()));
    fs::create_dir_all(&frame_dir).map_err(|e| format!("create output dir failed: {e}"))?;

    let mut framebuffer = FrameBuffer::new(UI_W as usize, UI_H as usize);
    let model = UiModel {
        mode: effective_mode.into_scene(),
        focus: args.focus.into_scene(),
        touch_irq: args.focus.into_scene() == UiFocus::Touch,
        frame_no: args.frame_no,
    };

    match args.scenario {
        ScenarioArg::Default => {
            front_panel_scene::render_frame_with_dashboard_route_overlay(
                &mut framebuffer,
                &model,
                args.variant.into_scene(),
                DashboardRoute::Home,
                None,
                SelfCheckOverlay::None,
            )
            .map_err(|_| "render failed unexpectedly".to_string())?;
        }
        ScenarioArg::DisplayDiag => {
            let meta = DisplayDiagnosticMeta {
                orientation_label: "ORI: LANDSCAPE_SWAP (MADCTL=0xE0)",
                color_order_label: "COLOR ORDER: BGR565",
                heartbeat_on: (args.frame_no % 2) == 0,
            };
            front_panel_scene::render_display_diagnostic(&mut framebuffer, &meta)
                .map_err(|_| "render failed unexpectedly".to_string())?;
        }
        ScenarioArg::DashboardRuntimeStandby
        | ScenarioArg::DashboardRuntimeAssist
        | ScenarioArg::DashboardRuntimeBackup => {
            let mode = match args.scenario {
                ScenarioArg::DashboardRuntimeStandby => UpsMode::Standby,
                ScenarioArg::DashboardRuntimeAssist => UpsMode::Supplement,
                ScenarioArg::DashboardRuntimeBackup => UpsMode::Backup,
                _ => unreachable!(),
            };
            let snapshot = dashboard_snapshot_for_mode(mode);
            let dashboard_model = UiModel {
                mode,
                focus: UiFocus::Idle,
                touch_irq: false,
                frame_no: args.frame_no,
            };
            front_panel_scene::render_frame_with_dashboard_route_overlay(
                &mut framebuffer,
                &dashboard_model,
                UiVariant::InstrumentB,
                DashboardRoute::Home,
                Some(&snapshot),
                SelfCheckOverlay::None,
            )
            .map_err(|_| "render failed unexpectedly".to_string())?;
        }
        ScenarioArg::DashboardDetailCells
        | ScenarioArg::DashboardDetailBatteryFlow
        | ScenarioArg::DashboardDetailOutput
        | ScenarioArg::DashboardDetailCharger
        | ScenarioArg::DashboardDetailThermal
        | ScenarioArg::DashboardDetailThermalTestMode
        | ScenarioArg::DashboardDetailThermKillAsserted
        | ScenarioArg::DashboardDetailChargerWait
        | ScenarioArg::DashboardDetailCharger500mA
        | ScenarioArg::DashboardDetailCharger100mADcDerated
        | ScenarioArg::DashboardDetailChargerFullLatched
        | ScenarioArg::DashboardDetailChargerBlockedOutputOverload
        | ScenarioArg::DashboardDetailChargerBlockedNoBms => {
            let page = match args.scenario {
                ScenarioArg::DashboardDetailCells => DashboardDetailPage::Cells,
                ScenarioArg::DashboardDetailBatteryFlow => DashboardDetailPage::BatteryFlow,
                ScenarioArg::DashboardDetailOutput => DashboardDetailPage::Output,
                ScenarioArg::DashboardDetailCharger => DashboardDetailPage::Charger,
                ScenarioArg::DashboardDetailThermal
                | ScenarioArg::DashboardDetailThermalTestMode
                | ScenarioArg::DashboardDetailThermKillAsserted => DashboardDetailPage::Thermal,
                ScenarioArg::DashboardDetailChargerWait
                | ScenarioArg::DashboardDetailCharger500mA
                | ScenarioArg::DashboardDetailCharger100mADcDerated
                | ScenarioArg::DashboardDetailChargerFullLatched
                | ScenarioArg::DashboardDetailChargerBlockedOutputOverload
                | ScenarioArg::DashboardDetailChargerBlockedNoBms => DashboardDetailPage::Charger,
                _ => unreachable!(),
            };
            let (mode, snapshot) = match args.scenario {
                ScenarioArg::DashboardDetailChargerWait => {
                    charger_policy_snapshot_for_state(ChargerPolicyPreviewState::Wait)
                }
                ScenarioArg::DashboardDetailCharger500mA => {
                    charger_policy_snapshot_for_state(ChargerPolicyPreviewState::Charge500mA)
                }
                ScenarioArg::DashboardDetailCharger100mADcDerated => {
                    charger_policy_snapshot_for_state(
                        ChargerPolicyPreviewState::Charge100mADcDerated,
                    )
                }
                ScenarioArg::DashboardDetailChargerFullLatched => {
                    charger_policy_snapshot_for_state(ChargerPolicyPreviewState::FullLatched)
                }
                ScenarioArg::DashboardDetailChargerBlockedOutputOverload => {
                    charger_policy_snapshot_for_state(
                        ChargerPolicyPreviewState::BlockedOutputOverload,
                    )
                }
                ScenarioArg::DashboardDetailChargerBlockedNoBms => {
                    charger_policy_snapshot_for_state(ChargerPolicyPreviewState::BlockedNoBms)
                }
                ScenarioArg::DashboardDetailThermalTestMode => {
                    dashboard_detail_snapshot_for_thermal_notice(
                        "TMP HW PROTECT TEST MODE",
                        0,
                        "OFF",
                        None,
                    )
                }
                ScenarioArg::DashboardDetailThermKillAsserted => {
                    dashboard_detail_snapshot_for_thermal_notice(
                        "THERM KILL ASSERTED",
                        0,
                        "OFF",
                        None,
                    )
                }
                _ => dashboard_detail_snapshot_for_page(page),
            };
            let dashboard_model = UiModel {
                mode,
                focus: UiFocus::Idle,
                touch_irq: false,
                frame_no: args.frame_no,
            };
            front_panel_scene::render_frame_with_dashboard_route_overlay(
                &mut framebuffer,
                &dashboard_model,
                UiVariant::InstrumentB,
                DashboardRoute::Detail(page),
                Some(&snapshot),
                SelfCheckOverlay::None,
            )
            .map_err(|_| "render failed unexpectedly".to_string())?;
        }
        ScenarioArg::Bq40Offline
        | ScenarioArg::SelfCheckBmsMissingTpsWarn
        | ScenarioArg::Bq40OfflineDialog
        | ScenarioArg::Bq40DischargeBlocked
        | ScenarioArg::Bq40DischargeDialog
        | ScenarioArg::Bq40DischargeRecovering
        | ScenarioArg::Bq40Activating
        | ScenarioArg::Bq40ResultSuccess
        | ScenarioArg::Bq40ResultNoBattery
        | ScenarioArg::Bq40ResultRomMode
        | ScenarioArg::Bq40ResultAbnormal
        | ScenarioArg::Bq40ResultNotDetected => {
            let (snapshot, overlay) =
                bq40_snapshot_for_scenario(args.mode.into_scene(), args.scenario);
            front_panel_scene::render_frame_with_dashboard_route_overlay(
                &mut framebuffer,
                &model,
                args.variant.into_scene(),
                DashboardRoute::Home,
                Some(&snapshot),
                overlay,
            )
            .map_err(|_| "render failed unexpectedly".to_string())?;
        }
        ScenarioArg::TestAudio => {
            let state = AudioTestUiState {
                playing: false,
                queued: 0,
                current: None,
                selected_idx: 3,
                list_top: 0,
            };
            front_panel_scene::render_test_audio_playback(&mut framebuffer, false, state)
                .map_err(|_| "render failed unexpectedly".to_string())?;
        }
        ScenarioArg::TpsTest => {
            let snapshot = tps_test_snapshot_fixture();
            let tps_model = UiModel {
                mode: UpsMode::Standby,
                focus: UiFocus::Idle,
                touch_irq: false,
                frame_no: args.frame_no,
            };
            front_panel_scene::render_tps_test_status(
                &mut framebuffer,
                &tps_model,
                UiVariant::InstrumentB,
                &snapshot,
            )
            .map_err(|_| "render failed unexpectedly".to_string())?;
        }
        ScenarioArg::TestNavigation => {
            front_panel_scene::render_test_navigation(
                &mut framebuffer,
                TestFunctionUi::AudioPlayback,
                Some(TestFunctionUi::ScreenStatic),
            )
            .map_err(|_| "render failed unexpectedly".to_string())?;
        }
    }

    let bin_path = frame_dir.join("framebuffer.bin");
    framebuffer
        .write_raw_le(&bin_path)
        .map_err(|e| format!("write framebuffer failed: {e}"))?;

    let png_path = frame_dir.join("preview.png");
    framebuffer
        .write_png(&png_path)
        .map_err(|e| format!("write preview png failed: {e}"))?;

    println!("wrote {} and {}", bin_path.display(), png_path.display());
    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum VariantArg {
    A,
    B,
    C,
    D,
}

impl VariantArg {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw.to_ascii_lowercase().as_str() {
            "a" => Ok(Self::A),
            "b" => Ok(Self::B),
            "c" => Ok(Self::C),
            "d" => Ok(Self::D),
            _ => Err(format!(
                "unsupported --variant value: {raw} (expected A|B|C|D)"
            )),
        }
    }

    fn as_tag(self) -> &'static str {
        match self {
            VariantArg::A => "A",
            VariantArg::B => "B",
            VariantArg::C => "C",
            VariantArg::D => "D",
        }
    }

    fn into_scene(self) -> UiVariant {
        match self {
            VariantArg::A => UiVariant::InstrumentA,
            VariantArg::B => UiVariant::InstrumentB,
            VariantArg::C => UiVariant::RetroC,
            VariantArg::D => UiVariant::InstrumentD,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum FocusArg {
    Idle,
    Up,
    Down,
    Left,
    Right,
    Center,
    Touch,
}

impl FocusArg {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw.to_ascii_lowercase().as_str() {
            "idle" => Ok(Self::Idle),
            "up" => Ok(Self::Up),
            "down" => Ok(Self::Down),
            "left" => Ok(Self::Left),
            "right" => Ok(Self::Right),
            "center" => Ok(Self::Center),
            "touch" => Ok(Self::Touch),
            _ => Err(format!(
                "unsupported --focus value: {raw} (expected idle|up|down|left|right|center|touch)"
            )),
        }
    }

    fn as_tag(self) -> &'static str {
        match self {
            FocusArg::Idle => "idle",
            FocusArg::Up => "up",
            FocusArg::Down => "down",
            FocusArg::Left => "left",
            FocusArg::Right => "right",
            FocusArg::Center => "center",
            FocusArg::Touch => "touch",
        }
    }

    fn into_scene(self) -> UiFocus {
        match self {
            FocusArg::Idle => UiFocus::Idle,
            FocusArg::Up => UiFocus::Up,
            FocusArg::Down => UiFocus::Down,
            FocusArg::Left => UiFocus::Left,
            FocusArg::Right => UiFocus::Right,
            FocusArg::Center => UiFocus::Center,
            FocusArg::Touch => UiFocus::Touch,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum ModeArg {
    Off,
    Standby,
    Supplement,
    Backup,
}

impl ModeArg {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw.to_ascii_lowercase().as_str() {
            "off" => Ok(Self::Off),
            "standby" | "stby" => Ok(Self::Standby),
            "supplement" | "supp" => Ok(Self::Supplement),
            "backup" | "batt" => Ok(Self::Backup),
            _ => Err(format!(
                "unsupported --mode value: {raw} (expected off|standby|supplement|backup)"
            )),
        }
    }

    fn from_focus(focus: FocusArg) -> Self {
        match demo_mode_from_focus(focus.into_scene()) {
            UpsMode::Off => Self::Off,
            UpsMode::Standby => Self::Standby,
            UpsMode::Supplement => Self::Supplement,
            UpsMode::Backup => Self::Backup,
        }
    }

    fn into_scene(self) -> UpsMode {
        match self {
            ModeArg::Off => UpsMode::Off,
            ModeArg::Standby => UpsMode::Standby,
            ModeArg::Supplement => UpsMode::Supplement,
            ModeArg::Backup => UpsMode::Backup,
        }
    }

    fn as_tag(self) -> &'static str {
        match self {
            ModeArg::Off => "off",
            ModeArg::Standby => "standby",
            ModeArg::Supplement => "supplement",
            ModeArg::Backup => "backup",
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum ScenarioArg {
    Default,
    DisplayDiag,
    DashboardRuntimeStandby,
    DashboardRuntimeAssist,
    DashboardRuntimeBackup,
    DashboardDetailCells,
    DashboardDetailBatteryFlow,
    DashboardDetailOutput,
    DashboardDetailCharger,
    DashboardDetailThermal,
    DashboardDetailThermalTestMode,
    DashboardDetailThermKillAsserted,
    DashboardDetailChargerWait,
    DashboardDetailCharger500mA,
    DashboardDetailCharger100mADcDerated,
    DashboardDetailChargerFullLatched,
    DashboardDetailChargerBlockedOutputOverload,
    DashboardDetailChargerBlockedNoBms,
    SelfCheckBmsMissingTpsWarn,
    Bq40Offline,
    Bq40OfflineDialog,
    Bq40DischargeBlocked,
    Bq40DischargeDialog,
    Bq40DischargeRecovering,
    Bq40Activating,
    Bq40ResultSuccess,
    Bq40ResultNoBattery,
    Bq40ResultRomMode,
    Bq40ResultAbnormal,
    Bq40ResultNotDetected,
    TpsTest,
    TestAudio,
    TestNavigation,
}

impl ScenarioArg {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw.to_ascii_lowercase().as_str() {
            "default" => Ok(Self::Default),
            "display-diag" => Ok(Self::DisplayDiag),
            "dashboard-runtime-standby" => Ok(Self::DashboardRuntimeStandby),
            "dashboard-runtime-assist" => Ok(Self::DashboardRuntimeAssist),
            "dashboard-runtime-backup" => Ok(Self::DashboardRuntimeBackup),
            "dashboard-detail-cells" => Ok(Self::DashboardDetailCells),
            "dashboard-detail-battery-flow" => Ok(Self::DashboardDetailBatteryFlow),
            "dashboard-detail-output" => Ok(Self::DashboardDetailOutput),
            "dashboard-detail-charger" => Ok(Self::DashboardDetailCharger),
            "dashboard-detail-thermal" => Ok(Self::DashboardDetailThermal),
            "dashboard-detail-thermal-test-mode" => Ok(Self::DashboardDetailThermalTestMode),
            "dashboard-detail-therm-kill-asserted" => Ok(Self::DashboardDetailThermKillAsserted),
            "dashboard-detail-charger-wait" => Ok(Self::DashboardDetailChargerWait),
            "dashboard-detail-charger-500ma" => Ok(Self::DashboardDetailCharger500mA),
            "dashboard-detail-charger-100ma-dc-derated" => {
                Ok(Self::DashboardDetailCharger100mADcDerated)
            }
            "dashboard-detail-charger-full-latched" => {
                Ok(Self::DashboardDetailChargerFullLatched)
            }
            "dashboard-detail-charger-blocked-output-overload" => {
                Ok(Self::DashboardDetailChargerBlockedOutputOverload)
            }
            "dashboard-detail-charger-blocked-no-bms" => {
                Ok(Self::DashboardDetailChargerBlockedNoBms)
            }
            "self-check-bms-missing-tps-warn" => Ok(Self::SelfCheckBmsMissingTpsWarn),
            "bq40-offline" => Ok(Self::Bq40Offline),
            "bq40-offline-dialog" => Ok(Self::Bq40OfflineDialog),
            "bq40-discharge-blocked" => Ok(Self::Bq40DischargeBlocked),
            "bq40-discharge-dialog" => Ok(Self::Bq40DischargeDialog),
            "bq40-discharge-recovering" => Ok(Self::Bq40DischargeRecovering),
            "bq40-activating" => Ok(Self::Bq40Activating),
            "bq40-result-success" => Ok(Self::Bq40ResultSuccess),
            "bq40-result-no-battery" => Ok(Self::Bq40ResultNoBattery),
            "bq40-result-rom-mode" => Ok(Self::Bq40ResultRomMode),
            "bq40-result-abnormal" => Ok(Self::Bq40ResultAbnormal),
            "bq40-result-not-detected" => Ok(Self::Bq40ResultNotDetected),
            "tps-test" => Ok(Self::TpsTest),
            "test-audio" => Ok(Self::TestAudio),
            "test-navigation" => Ok(Self::TestNavigation),
            _ => Err(format!(
                "unsupported --scenario value: {raw} (expected default|display-diag|dashboard-runtime-standby|dashboard-runtime-assist|dashboard-runtime-backup|dashboard-detail-cells|dashboard-detail-battery-flow|dashboard-detail-output|dashboard-detail-charger|dashboard-detail-thermal|dashboard-detail-thermal-test-mode|dashboard-detail-therm-kill-asserted|dashboard-detail-charger-wait|dashboard-detail-charger-500ma|dashboard-detail-charger-100ma-dc-derated|dashboard-detail-charger-full-latched|dashboard-detail-charger-blocked-output-overload|dashboard-detail-charger-blocked-no-bms|self-check-bms-missing-tps-warn|bq40-offline|bq40-offline-dialog|bq40-discharge-blocked|bq40-discharge-dialog|bq40-discharge-recovering|bq40-activating|bq40-result-success|bq40-result-no-battery|bq40-result-rom-mode|bq40-result-abnormal|bq40-result-not-detected|tps-test|test-audio|test-navigation)"
            )),
        }
    }

    fn as_tag(self) -> &'static str {
        match self {
            ScenarioArg::Default => "default",
            ScenarioArg::DisplayDiag => "display-diag",
            ScenarioArg::DashboardRuntimeStandby => "dashboard-runtime-standby",
            ScenarioArg::DashboardRuntimeAssist => "dashboard-runtime-assist",
            ScenarioArg::DashboardRuntimeBackup => "dashboard-runtime-backup",
            ScenarioArg::DashboardDetailCells => "dashboard-detail-cells",
            ScenarioArg::DashboardDetailBatteryFlow => "dashboard-detail-battery-flow",
            ScenarioArg::DashboardDetailOutput => "dashboard-detail-output",
            ScenarioArg::DashboardDetailCharger => "dashboard-detail-charger",
            ScenarioArg::DashboardDetailThermal => "dashboard-detail-thermal",
            ScenarioArg::DashboardDetailThermalTestMode => "dashboard-detail-thermal-test-mode",
            ScenarioArg::DashboardDetailThermKillAsserted => "dashboard-detail-therm-kill-asserted",
            ScenarioArg::DashboardDetailChargerWait => "dashboard-detail-charger-wait",
            ScenarioArg::DashboardDetailCharger500mA => "dashboard-detail-charger-500ma",
            ScenarioArg::DashboardDetailCharger100mADcDerated => {
                "dashboard-detail-charger-100ma-dc-derated"
            }
            ScenarioArg::DashboardDetailChargerFullLatched => {
                "dashboard-detail-charger-full-latched"
            }
            ScenarioArg::DashboardDetailChargerBlockedOutputOverload => {
                "dashboard-detail-charger-blocked-output-overload"
            }
            ScenarioArg::DashboardDetailChargerBlockedNoBms => {
                "dashboard-detail-charger-blocked-no-bms"
            }
            ScenarioArg::SelfCheckBmsMissingTpsWarn => "self-check-bms-missing-tps-warn",
            ScenarioArg::Bq40Offline => "bq40-offline",
            ScenarioArg::Bq40OfflineDialog => "bq40-offline-dialog",
            ScenarioArg::Bq40DischargeBlocked => "bq40-discharge-blocked",
            ScenarioArg::Bq40DischargeDialog => "bq40-discharge-dialog",
            ScenarioArg::Bq40DischargeRecovering => "bq40-discharge-recovering",
            ScenarioArg::Bq40Activating => "bq40-activating",
            ScenarioArg::Bq40ResultSuccess => "bq40-result-success",
            ScenarioArg::Bq40ResultNoBattery => "bq40-result-no-battery",
            ScenarioArg::Bq40ResultRomMode => "bq40-result-rom-mode",
            ScenarioArg::Bq40ResultAbnormal => "bq40-result-abnormal",
            ScenarioArg::Bq40ResultNotDetected => "bq40-result-not-detected",
            ScenarioArg::TpsTest => "tps-test",
            ScenarioArg::TestAudio => "test-audio",
            ScenarioArg::TestNavigation => "test-navigation",
        }
    }
}

#[derive(Debug)]
struct Args {
    variant: VariantArg,
    mode: ModeArg,
    focus: FocusArg,
    scenario: ScenarioArg,
    out_dir: PathBuf,
    frame_no: u32,
}

impl Args {
    fn parse<I>(mut iter: I) -> Result<Self, String>
    where
        I: Iterator<Item = String>,
    {
        let mut variant: Option<VariantArg> = None;
        let mut mode: Option<ModeArg> = None;
        let mut focus: Option<FocusArg> = None;
        let mut scenario: Option<ScenarioArg> = None;
        let mut out_dir: Option<PathBuf> = None;
        let mut frame_no: u32 = 0;

        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--variant" => {
                    let value = iter.next().ok_or("missing value for --variant")?;
                    variant = Some(VariantArg::parse(&value)?);
                }
                "--focus" => {
                    let value = iter.next().ok_or("missing value for --focus")?;
                    focus = Some(FocusArg::parse(&value)?);
                }
                "--mode" => {
                    let value = iter.next().ok_or("missing value for --mode")?;
                    mode = Some(ModeArg::parse(&value)?);
                }
                "--scenario" => {
                    let value = iter.next().ok_or("missing value for --scenario")?;
                    scenario = Some(ScenarioArg::parse(&value)?);
                }
                "--out-dir" => {
                    let value = iter.next().ok_or("missing value for --out-dir")?;
                    out_dir = Some(PathBuf::from(value));
                }
                "--frame-no" => {
                    let value = iter.next().ok_or("missing value for --frame-no")?;
                    frame_no = value
                        .parse::<u32>()
                        .map_err(|_| format!("invalid --frame-no value: {value}"))?;
                }
                "--help" | "-h" => {
                    return Err(help_text());
                }
                unknown => {
                    return Err(format!("unknown argument: {unknown}\n\n{}", help_text()));
                }
            }
        }

        let variant = variant.ok_or_else(|| format!("missing --variant\n\n{}", help_text()))?;
        let focus = focus.ok_or_else(|| format!("missing --focus\n\n{}", help_text()))?;
        let out_dir = out_dir.ok_or_else(|| format!("missing --out-dir\n\n{}", help_text()))?;
        let mode = mode.unwrap_or_else(|| ModeArg::from_focus(focus));
        let scenario = scenario.unwrap_or(ScenarioArg::Default);

        Ok(Self {
            variant,
            mode,
            focus,
            scenario,
            out_dir,
            frame_no,
        })
    }
}

fn help_text() -> String {
    [
        "Usage:",
        "  front-panel-preview --variant {A|B|C|D} --focus {idle|up|down|left|right|center|touch} [--mode {off|standby|supplement|backup}] [--scenario {default|display-diag|dashboard-runtime-standby|dashboard-runtime-assist|dashboard-runtime-backup|dashboard-detail-cells|dashboard-detail-battery-flow|dashboard-detail-output|dashboard-detail-charger|dashboard-detail-thermal|dashboard-detail-thermal-test-mode|dashboard-detail-therm-kill-asserted|dashboard-detail-charger-wait|dashboard-detail-charger-500ma|dashboard-detail-charger-100ma-dc-derated|dashboard-detail-charger-full-latched|dashboard-detail-charger-blocked-output-overload|dashboard-detail-charger-blocked-no-bms|self-check-bms-missing-tps-warn|bq40-offline|bq40-offline-dialog|bq40-discharge-blocked|bq40-discharge-dialog|bq40-discharge-recovering|bq40-activating|bq40-result-success|bq40-result-no-battery|bq40-result-rom-mode|bq40-result-abnormal|bq40-result-not-detected|tps-test|test-audio|test-navigation}] --out-dir <ABS_PATH> [--frame-no <n>]",
        "",
        "Example:",
        "  cargo run --manifest-path tools/front-panel-preview/Cargo.toml -- --variant C --focus idle --mode standby --scenario bq40-offline-dialog --out-dir /tmp/front-panel-preview",
    ]
    .join("\n")
}

struct FrameBuffer {
    width: usize,
    height: usize,
    pixels: Vec<u16>,
}

impl FrameBuffer {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; width * height],
        }
    }

    fn write_raw_le(&self, path: &Path) -> io::Result<()> {
        let mut file = fs::File::create(path)?;
        for pixel in &self.pixels {
            file.write_all(&pixel.to_le_bytes())?;
        }
        Ok(())
    }

    fn write_png(&self, path: &Path) -> io::Result<()> {
        let mut image = RgbImage::new(self.width as u32, self.height as u32);

        for (index, pixel) in self.pixels.iter().enumerate() {
            let x = (index % self.width) as u32;
            let y = (index / self.width) as u32;
            image.put_pixel(x, y, Rgb(rgb565_to_rgb888(*pixel)));
        }

        image.save(path).map_err(io::Error::other)
    }
}

impl UiPainter for FrameBuffer {
    type Error = Infallible;

    fn fill_rect(
        &mut self,
        x: u16,
        y: u16,
        w: u16,
        h: u16,
        rgb565: u16,
    ) -> Result<(), Self::Error> {
        let x0 = x as usize;
        let y0 = y as usize;
        let x1 = x0.saturating_add(w as usize).min(self.width);
        let y1 = y0.saturating_add(h as usize).min(self.height);

        for yy in y0..y1 {
            let row = yy * self.width;
            for xx in x0..x1 {
                self.pixels[row + xx] = rgb565;
            }
        }

        Ok(())
    }
}

fn rgb565_to_rgb888(raw: u16) -> [u8; 3] {
    let r = ((raw >> 11) & 0x1f) as u8;
    let g = ((raw >> 5) & 0x3f) as u8;
    let b = (raw & 0x1f) as u8;

    [
        (r as u16 * 255 / 31) as u8,
        (g as u16 * 255 / 63) as u8,
        (b as u16 * 255 / 31) as u8,
    ]
}
