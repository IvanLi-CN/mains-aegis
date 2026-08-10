use std::{
    convert::Infallible,
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process,
};

use image::{Rgb, RgbImage};

extern crate self as mains_aegis_firmware;

#[path = "../../../firmware/src/output_state.rs"]
pub mod output_state;

#[path = "../../../firmware/src/net_types.rs"]
pub mod net_types;

#[path = "../../../firmware/src/front_panel_scene.rs"]
mod front_panel_scene;

use front_panel_scene::{
    demo_mode_from_focus, AlertPreviewItem, AlertPreviewKind, AlertPreviewSeverity,
    AlertPreviewSoundState, AudioTestUiState, BeeperPrefs, BeeperSettingTarget, BeeperVolumeLevel,
    BmsRecoveryUiAction, BmsResultKind, DashboardChargerProtocol, DashboardDetailPage,
    DashboardDetailSnapshot, DashboardHomeFocus, DashboardInputSource, DashboardMenuStyle,
    DashboardPrimaryPage, DashboardRoute, DashboardShellState, DisplayDiagnosticMeta,
    ManualChargeStopReason, MenuItem, SelfCheckCommState, SelfCheckHardwareTarget,
    SelfCheckOverlay, SelfCheckUiSnapshot, TestFunctionUi, TpsTestChargerSnapshot,
    TpsTestOutputSnapshot, TpsTestUiSnapshot, TpsTestVoutProfile, UiFocus, UiModel, UiPainter,
    UiVariant, UpsMode, UI_H, UI_W,
};
use net_types::{WifiConnectionState, WifiErrorKind, WifiSnapshot};

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
    detail.balance_enabled = Some(true);
    detail.balance_cfg_match = Some(true);
    detail.balance_active = Some(true);
    detail.balance_mask = Some(0b0100);
    detail.balance_cell = Some(3);
    detail.remcap_mah = Some(3_666);
    detail.fcc_mah = Some(3_704);
    detail.battery_energy_mwh = Some(46_850);
    detail.battery_full_capacity_mwh = Some(63_200);
    detail.charge_ready = Some(true);
    detail.discharge_ready = Some(true);
    detail.xchg = Some(false);
    detail.xdsg = Some(false);
    detail.charge_fet_on = Some(matches!(mode, UpsMode::Standby));
    detail.discharge_fet_on = Some(matches!(mode, UpsMode::Supplement | UpsMode::Backup));
    detail.precharge_fet_on = Some(matches!(mode, UpsMode::Standby));
    detail.learn_qen = Some(true);
    detail.learn_vok = Some(true);
    detail.learn_rest = Some(false);
    detail.fc = Some(false);
    detail.fd = Some(false);
    detail.pf = Some(false);
    detail.rca_alarm = Some(false);
    detail.reason_key = Some("nominal");
    detail.reason_label = Some("SYSTEM READY");
    detail.input_source = Some(match page {
        Some(DashboardDetailPage::Charger) => DashboardInputSource::UsbC,
        _ => DashboardInputSource::DcIn,
    });
    detail.charger_protocol = Some(match page {
        Some(DashboardDetailPage::Charger) => DashboardChargerProtocol::Pps,
        _ => DashboardChargerProtocol::DcIn,
    });
    detail.charger_active = Some(matches!(mode, UpsMode::Standby));
    detail.charger_home_status = Some(match mode {
        UpsMode::Standby => "CHG500",
        UpsMode::Supplement => "LOAD",
        UpsMode::Backup => "NOAC",
        UpsMode::Blocked => unreachable!("blocked is not dashboard-renderable"),
        UpsMode::Off => "WAIT",
    });
    detail.charger_status = detail.charger_home_status;
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
    detail.cells_notice = Some("EXT CHG+RELAX");
    detail.battery_notice = Some("PACK FLOW MOCKED - LIVE SOURCE NEXT");
    detail.bms_notice = Some("LIVE DATA");
    detail.output_notice = Some("OUT-B STANDBY PATH HELD");
    detail.charger_notice = Some(match mode {
        UpsMode::Standby => "charging_500ma",
        UpsMode::Supplement => "blocked_output_over_limit",
        UpsMode::Backup => "blocked_no_input",
        UpsMode::Blocked => unreachable!("blocked is not dashboard-renderable"),
        UpsMode::Off => "idle_wait_threshold",
    });
    detail.thermal_notice = Some("FAN RPM MOCKED - SENSOR WIRING NEXT");
    detail.wifi = wifi_snapshot_for_state(if matches!(page, Some(DashboardDetailPage::Wifi)) {
        WifiPreviewState::Connected
    } else {
        WifiPreviewState::Disabled
    });

    if matches!(page, Some(DashboardDetailPage::Output)) {
        detail.out_b_temp_c = None;
    }

    detail
}

fn dashboard_shell_fixture(
    page: DashboardPrimaryPage,
    home_focus: DashboardHomeFocus,
    menu_selected: MenuItem,
    menu_style: DashboardMenuStyle,
    beeper_prefs: BeeperPrefs,
    dashboard_menu_offset_y: i16,
) -> (UpsMode, DashboardShellState, SelfCheckUiSnapshot) {
    let mode = UpsMode::Standby;
    let snapshot = dashboard_snapshot_for_mode(mode);
    (
        mode,
        DashboardShellState {
            page,
            dashboard_route: DashboardRoute::Home,
            home_focus,
            menu_selected,
            menu_style,
            beeper_prefs,
            dashboard_menu_offset_y,
        },
        snapshot,
    )
}

#[derive(Clone, Copy, Debug)]
enum CellsBalancePreviewState {
    Active,
    Idle,
    ConfigMismatch,
}

#[derive(Clone, Copy, Debug)]
enum BmsDetailPreviewState {
    Nominal,
    ChargeBlocked,
    BalanceMulti,
    NoData,
}

#[derive(Clone, Copy, Debug)]
enum WifiPreviewState {
    Disabled,
    Connecting,
    ConnectedWeak,
    ConnectedMedium,
    Connected,
    ConnectedLongIp,
    Error,
}

fn wifi_snapshot_for_state(state: WifiPreviewState) -> WifiSnapshot {
    match state {
        WifiPreviewState::Disabled => WifiSnapshot::disabled(),
        WifiPreviewState::Connecting => WifiSnapshot {
            state: WifiConnectionState::Connecting,
            mac: Some([0xAC, 0x13, 0xF3, 0x52, 0x88, 0x19]),
            ..WifiSnapshot::disabled()
        },
        WifiPreviewState::ConnectedWeak => WifiSnapshot {
            state: WifiConnectionState::Connected,
            ipv4: Some([192, 168, 31, 45]),
            gateway: Some([192, 168, 31, 1]),
            dns: Some([192, 168, 31, 1]),
            is_static: false,
            rssi_dbm: Some(-82),
            mac: Some([0xAC, 0x13, 0xF3, 0x52, 0x88, 0x19]),
            ..WifiSnapshot::disabled()
        },
        WifiPreviewState::ConnectedMedium => WifiSnapshot {
            state: WifiConnectionState::Connected,
            ipv4: Some([192, 168, 31, 45]),
            gateway: Some([192, 168, 31, 1]),
            dns: Some([192, 168, 31, 1]),
            is_static: false,
            rssi_dbm: Some(-67),
            mac: Some([0xAC, 0x13, 0xF3, 0x52, 0x88, 0x19]),
            ..WifiSnapshot::disabled()
        },
        WifiPreviewState::Connected => WifiSnapshot {
            state: WifiConnectionState::Connected,
            ipv4: Some([192, 168, 31, 45]),
            gateway: Some([192, 168, 31, 1]),
            dns: Some([192, 168, 31, 1]),
            is_static: false,
            rssi_dbm: Some(-54),
            mac: Some([0xAC, 0x13, 0xF3, 0x52, 0x88, 0x19]),
            ..WifiSnapshot::disabled()
        },
        WifiPreviewState::ConnectedLongIp => WifiSnapshot {
            state: WifiConnectionState::Connected,
            ipv4: Some([255, 255, 255, 255]),
            gateway: Some([192, 168, 255, 254]),
            dns: Some([208, 67, 222, 222]),
            is_static: false,
            rssi_dbm: Some(-54),
            mac: Some([0xAC, 0x13, 0xF3, 0x52, 0x88, 0x19]),
            ..WifiSnapshot::disabled()
        },
        WifiPreviewState::Error => WifiSnapshot {
            state: WifiConnectionState::Error,
            is_static: false,
            last_error: Some(WifiErrorKind::DhcpTimeout),
            mac: Some([0xAC, 0x13, 0xF3, 0x52, 0x88, 0x19]),
            ..WifiSnapshot::disabled()
        },
    }
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
        UpsMode::Blocked => unreachable!("blocked is not dashboard-renderable"),
    }

    snapshot
}

fn dashboard_detail_snapshot_for_page(page: DashboardDetailPage) -> (UpsMode, SelfCheckUiSnapshot) {
    let mode = match page {
        DashboardDetailPage::Cells => UpsMode::Standby,
        DashboardDetailPage::BmsDetail => UpsMode::Standby,
        DashboardDetailPage::BatteryFlow => UpsMode::Backup,
        DashboardDetailPage::Output => UpsMode::Supplement,
        DashboardDetailPage::Charger => UpsMode::Standby,
        DashboardDetailPage::Thermal => UpsMode::Backup,
        DashboardDetailPage::Wifi => UpsMode::Standby,
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

fn dashboard_detail_snapshot_for_bms_state(
    state: BmsDetailPreviewState,
) -> (UpsMode, SelfCheckUiSnapshot) {
    match state {
        BmsDetailPreviewState::NoData => {
            let mut snapshot = SelfCheckUiSnapshot::pending(UpsMode::Standby);
            snapshot.gc9307 = SelfCheckCommState::Ok;
            snapshot.tca6408a = SelfCheckCommState::Ok;
            snapshot.dashboard_detail = DashboardDetailSnapshot::pending();
            (UpsMode::Standby, snapshot)
        }
        BmsDetailPreviewState::Nominal => {
            let (mode, mut snapshot) =
                dashboard_detail_snapshot_for_page(DashboardDetailPage::BmsDetail);
            let detail = &mut snapshot.dashboard_detail;
            detail.remcap_mah = Some(3_666);
            detail.fcc_mah = Some(3_704);
            detail.charge_ready = Some(true);
            detail.discharge_ready = Some(true);
            detail.xchg = Some(false);
            detail.xdsg = Some(false);
            detail.charge_fet_on = Some(true);
            detail.discharge_fet_on = Some(true);
            detail.learn_qen = Some(true);
            detail.learn_vok = Some(true);
            detail.learn_rest = Some(false);
            detail.fc = Some(false);
            detail.fd = Some(false);
            detail.pf = Some(false);
            detail.rca_alarm = Some(false);
            detail.reason_key = Some("nominal");
            detail.reason_label = Some("SYSTEM READY");
            detail.bms_notice = Some("LIVE DATA");
            (mode, snapshot)
        }
        BmsDetailPreviewState::ChargeBlocked => {
            let (mode, mut snapshot) =
                dashboard_detail_snapshot_for_bms_state(BmsDetailPreviewState::Nominal);
            let detail = &mut snapshot.dashboard_detail;
            detail.charge_ready = Some(false);
            detail.xchg = Some(true);
            detail.charge_fet_on = Some(false);
            detail.learn_qen = Some(true);
            detail.learn_vok = Some(false);
            detail.learn_rest = Some(true);
            detail.reason_key = Some("xchg_blocked");
            detail.reason_label = Some("CHG BLOCKED");
            detail.bms_notice = Some("LIVE DATA");
            (mode, snapshot)
        }
        BmsDetailPreviewState::BalanceMulti => {
            let (mode, mut snapshot) =
                dashboard_detail_snapshot_for_bms_state(BmsDetailPreviewState::Nominal);
            let detail = &mut snapshot.dashboard_detail;
            detail.balance_active = Some(true);
            detail.balance_mask = Some(0b0101);
            detail.balance_cell = None;
            detail.cells_notice = Some("EXT CHG+RELAX");
            (mode, snapshot)
        }
    }
}

fn dashboard_runtime_snapshot_for_wifi(state: WifiPreviewState) -> SelfCheckUiSnapshot {
    let mut snapshot = dashboard_snapshot_for_mode(UpsMode::Standby);
    snapshot.dashboard_detail.wifi = wifi_snapshot_for_state(state);
    snapshot
}

fn dashboard_detail_snapshot_for_wifi(state: WifiPreviewState) -> (UpsMode, SelfCheckUiSnapshot) {
    let mut snapshot = dashboard_snapshot_for_mode(UpsMode::Standby);
    snapshot.dashboard_detail =
        dashboard_detail_fixture(UpsMode::Standby, Some(DashboardDetailPage::Wifi));
    snapshot.dashboard_detail.wifi = wifi_snapshot_for_state(state);
    (UpsMode::Standby, snapshot)
}

fn dashboard_detail_snapshot_for_cells_balance(
    state: CellsBalancePreviewState,
) -> (UpsMode, SelfCheckUiSnapshot) {
    let (mode, mut snapshot) = dashboard_detail_snapshot_for_page(DashboardDetailPage::Cells);
    let detail = &mut snapshot.dashboard_detail;

    match state {
        CellsBalancePreviewState::Active => {
            detail.balance_enabled = Some(true);
            detail.balance_cfg_match = Some(true);
            detail.balance_active = Some(true);
            detail.balance_mask = Some(0b0100);
            detail.balance_cell = Some(3);
            detail.cells_notice = Some("EXT CHG+RELAX");
        }
        CellsBalancePreviewState::Idle => {
            detail.balance_enabled = Some(true);
            detail.balance_cfg_match = Some(true);
            detail.balance_active = Some(false);
            detail.balance_mask = Some(0);
            detail.balance_cell = None;
            detail.cells_notice = Some("EXT CHG+RELAX");
        }
        CellsBalancePreviewState::ConfigMismatch => {
            detail.balance_enabled = Some(true);
            detail.balance_cfg_match = Some(false);
            detail.balance_active = Some(false);
            detail.balance_mask = Some(0);
            detail.balance_cell = None;
            detail.cells_notice = Some("CFG MISMATCH");
        }
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
    BackupUsbLowOutput,
    BackupUsbOutputHighLatched,
    BackupUsbTelemetryLostLatched,
    Warm,
    Charge100mADcDerated,
    RecoveringLowVoltage,
    FullLatched,
    BlockedOutputOverload,
    BlockedOutputPowerUnknown,
    BlockedNoBms,
}

fn charger_policy_snapshot_for_state(
    state: ChargerPolicyPreviewState,
) -> (UpsMode, SelfCheckUiSnapshot) {
    let mode = match state {
        ChargerPolicyPreviewState::BackupUsbLowOutput
        | ChargerPolicyPreviewState::BackupUsbOutputHighLatched
        | ChargerPolicyPreviewState::BackupUsbTelemetryLostLatched => UpsMode::Backup,
        _ => UpsMode::Standby,
    };
    let mut snapshot = dashboard_snapshot_for_mode(mode);
    snapshot.dashboard_detail = dashboard_detail_fixture(mode, Some(DashboardDetailPage::Charger));
    snapshot.dashboard_detail.input_source = Some(DashboardInputSource::UsbC);
    snapshot.dashboard_detail.charger_protocol = Some(DashboardChargerProtocol::Pps);
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
        ChargerPolicyPreviewState::BackupUsbLowOutput => {
            snapshot.vin_mains_present = Some(false);
            snapshot.vin_vbus_mv = None;
            snapshot.vin_iin_ma = None;
            snapshot.dashboard_detail.charger_active = Some(true);
            snapshot.dashboard_detail.charger_status = Some("CHG500");
            snapshot.dashboard_detail.charger_notice = Some("backup_usb_low_output_charge");
            snapshot.bq25792_allow_charge = Some(true);
            snapshot.bq25792_ichg_ma = Some(500);
            snapshot.bq25792_ibat_ma = Some(470);
            snapshot.bq40z50_current_ma = Some(460);
            snapshot.bq40z50_soc_pct = Some(67);
            snapshot.bq40z50_pack_mv = Some(15_260);
            snapshot.input_ibus_ma = Some(1_240);
            snapshot.tps_a_enabled = Some(true);
            snapshot.out_a_vbus_mv = Some(20_000);
            snapshot.tps_a_iout_ma = Some(95);
            snapshot.ina_total_ma = Some(95);
        }
        ChargerPolicyPreviewState::BackupUsbOutputHighLatched => {
            snapshot.vin_mains_present = Some(false);
            snapshot.vin_vbus_mv = None;
            snapshot.vin_iin_ma = None;
            snapshot.dashboard_detail.charger_status = Some("LOAD");
            snapshot.dashboard_detail.charger_notice = Some("backup_usb_output_high_latched");
            snapshot.bq25792_allow_charge = Some(false);
            snapshot.bq25792_ibat_ma = Some(0);
            snapshot.bq40z50_current_ma = Some(-150);
            snapshot.bq40z50_soc_pct = Some(67);
            snapshot.bq40z50_pack_mv = Some(15_250);
            snapshot.input_ibus_ma = Some(220);
            snapshot.tps_a_enabled = Some(true);
            snapshot.out_a_vbus_mv = Some(20_000);
            snapshot.tps_a_iout_ma = Some(155);
            snapshot.ina_total_ma = Some(155);
        }
        ChargerPolicyPreviewState::BackupUsbTelemetryLostLatched => {
            snapshot.vin_mains_present = Some(false);
            snapshot.vin_vbus_mv = None;
            snapshot.vin_iin_ma = None;
            snapshot.dashboard_detail.charger_status = Some("LOCK");
            snapshot.dashboard_detail.charger_notice = Some("backup_usb_telemetry_lost_latched");
            snapshot.bq25792_allow_charge = Some(false);
            snapshot.bq25792_ibat_ma = Some(0);
            snapshot.bq40z50_current_ma = Some(-110);
            snapshot.bq40z50_soc_pct = Some(67);
            snapshot.bq40z50_pack_mv = Some(15_250);
            snapshot.input_ibus_ma = Some(210);
            snapshot.tps_a_enabled = Some(true);
            snapshot.out_a_vbus_mv = Some(20_000);
            snapshot.tps_a_iout_ma = None;
            snapshot.ina_total_ma = None;
        }
        ChargerPolicyPreviewState::Warm => {
            snapshot.dashboard_detail.charger_active = Some(true);
            snapshot.dashboard_detail.charger_status = Some("WARM");
            snapshot.dashboard_detail.charger_notice = Some("BQ25792 TS WARM - FAN FORCED HIGH");
            snapshot.dashboard_detail.fan_status = Some("HIGH");
            snapshot.dashboard_detail.fan_pwm_pct = Some(100);
            snapshot.dashboard_detail.fan_rpm = Some(4_120);
            snapshot.bq25792_allow_charge = Some(true);
            snapshot.bq25792_ichg_ma = Some(500);
            snapshot.bq25792_ibat_ma = Some(470);
            snapshot.bq40z50_current_ma = Some(455);
            snapshot.bq40z50_soc_pct = Some(76);
            snapshot.bq40z50_pack_mv = Some(15_710);
            snapshot.input_ibus_ma = Some(1_180);
            snapshot.vin_iin_ma = Some(1_180);
        }
        ChargerPolicyPreviewState::Charge100mADcDerated => {
            snapshot.dashboard_detail.input_source = Some(DashboardInputSource::DcIn);
            snapshot.dashboard_detail.charger_protocol = Some(DashboardChargerProtocol::DcIn);
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
        ChargerPolicyPreviewState::RecoveringLowVoltage => {
            snapshot.dashboard_detail.charger_active = Some(true);
            snapshot.dashboard_detail.charger_status = Some("RECOV");
            snapshot.dashboard_detail.charger_notice = Some("bq25792_precharge");
            snapshot.bq25792_allow_charge = Some(true);
            snapshot.bq25792_ichg_ma = Some(100);
            snapshot.bq25792_ibat_ma = Some(82);
            snapshot.bq40z50_current_ma = Some(88);
            snapshot.bq40z50_soc_pct = Some(9);
            snapshot.bq40z50_pack_mv = Some(11_760);
            snapshot.input_ibus_ma = Some(520);
            snapshot.vin_iin_ma = Some(520);
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
            snapshot.dashboard_detail.charger_protocol = Some(DashboardChargerProtocol::DcIn);
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
        ChargerPolicyPreviewState::BlockedOutputPowerUnknown => {
            snapshot.dashboard_detail.charger_status = Some("LOAD");
            snapshot.dashboard_detail.charger_notice = Some("blocked_output_power_unknown");
            snapshot.dashboard_detail.input_source = Some(DashboardInputSource::DcIn);
            snapshot.dashboard_detail.charger_protocol = Some(DashboardChargerProtocol::DcIn);
            snapshot.tps_a_enabled = Some(true);
            snapshot.out_a_vbus_mv = Some(19_040);
            snapshot.tps_a_iout_ma = None;
            snapshot.tps_b_enabled = Some(true);
            snapshot.out_b_vbus_mv = Some(19_000);
            snapshot.tps_b_iout_ma = Some(120);
            snapshot.ina_total_ma = None;
            snapshot.input_ibus_ma = Some(1_020);
            snapshot.vin_iin_ma = Some(1_020);
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

    snapshot.dashboard_detail.charger_home_status = snapshot.dashboard_detail.charger_status;

    (mode, snapshot)
}

#[derive(Clone, Copy, Debug)]
enum ManualChargePreviewState {
    Default,
    AutoCharging,
    Active,
    LoopbackConfirm,
    LoopbackConfirmed,
    StopHold,
    ResetAuto,
    Blocked,
}

fn manual_charge_snapshot_for_state(
    state: ManualChargePreviewState,
) -> (UpsMode, DashboardRoute, SelfCheckUiSnapshot) {
    let mode = match state {
        ManualChargePreviewState::LoopbackConfirm | ManualChargePreviewState::LoopbackConfirmed => {
            UpsMode::Backup
        }
        _ => UpsMode::Standby,
    };
    let mut snapshot = dashboard_snapshot_for_mode(mode);
    snapshot.dashboard_detail = dashboard_detail_fixture(mode, Some(DashboardDetailPage::Charger));
    snapshot.dashboard_detail.input_source = Some(DashboardInputSource::UsbC);
    snapshot.dashboard_detail.charger_protocol = Some(DashboardChargerProtocol::Pps);
    snapshot.dashboard_detail.manual_charge.prefs =
        front_panel_scene::ManualChargePrefs::defaults();
    snapshot
        .dashboard_detail
        .manual_charge
        .runtime
        .last_stop_reason = ManualChargeStopReason::None;
    snapshot.dashboard_detail.charger_active = Some(false);
    snapshot.dashboard_detail.charger_status = Some("WAIT");
    snapshot.dashboard_detail.charger_home_status = Some("WAIT");
    snapshot.dashboard_detail.charger_notice = Some("idle_wait_threshold");
    snapshot.bq25792_allow_charge = Some(false);
    snapshot.bq25792_ichg_ma = None;
    snapshot.bq25792_ibat_ma = Some(0);
    snapshot.bq40z50_pack_mv = Some(15_260);
    snapshot.bq40z50_current_ma = Some(0);
    snapshot.bq40z50_soc_pct = Some(67);

    if matches!(
        state,
        ManualChargePreviewState::LoopbackConfirm | ManualChargePreviewState::LoopbackConfirmed
    ) {
        snapshot.vin_mains_present = Some(false);
        snapshot.vin_vbus_mv = None;
        snapshot.vin_iin_ma = None;
        snapshot.fusb302_vbus_present = Some(true);
        snapshot.input_vbus_mv = Some(20_060);
        snapshot.input_ibus_ma = Some(1_240);
    }

    match state {
        ManualChargePreviewState::Default => {}
        ManualChargePreviewState::AutoCharging => {
            snapshot.dashboard_detail.manual_charge.prefs.speed =
                front_panel_scene::ManualChargeSpeed::Ma1000;
            snapshot.dashboard_detail.charger_active = Some(true);
            snapshot.dashboard_detail.charger_status = Some("CHG500");
            snapshot.dashboard_detail.charger_home_status = Some("CHG500");
            snapshot.dashboard_detail.charger_notice = Some("charging_500ma");
            snapshot.bq25792_allow_charge = Some(true);
            snapshot.bq25792_ichg_ma = Some(500);
            snapshot.bq25792_ibat_ma = Some(450);
            snapshot.bq40z50_current_ma = Some(440);
        }
        ManualChargePreviewState::Active => {
            snapshot.dashboard_detail.manual_charge.runtime.active = true;
            snapshot.dashboard_detail.manual_charge.runtime.takeover = true;
            snapshot
                .dashboard_detail
                .manual_charge
                .runtime
                .remaining_minutes = Some(92);
            snapshot.dashboard_detail.charger_active = Some(true);
            snapshot.dashboard_detail.charger_status = Some("CHG500");
            snapshot.dashboard_detail.charger_home_status = Some("CHG500");
            snapshot.dashboard_detail.charger_notice = Some("charging_500ma");
            snapshot.bq25792_allow_charge = Some(true);
            snapshot.bq25792_ichg_ma = Some(500);
            snapshot.bq25792_ibat_ma = Some(480);
            snapshot.bq40z50_current_ma = Some(470);
        }
        ManualChargePreviewState::LoopbackConfirm => {}
        ManualChargePreviewState::LoopbackConfirmed => {
            snapshot.dashboard_detail.manual_charge.runtime.active = true;
            snapshot.dashboard_detail.manual_charge.runtime.takeover = true;
            snapshot
                .dashboard_detail
                .manual_charge
                .runtime
                .loopback_override = true;
            snapshot
                .dashboard_detail
                .manual_charge
                .runtime
                .remaining_minutes = Some(92);
            snapshot.dashboard_detail.charger_active = Some(true);
            snapshot.dashboard_detail.charger_status = Some("CHG500");
            snapshot.dashboard_detail.charger_home_status = Some("CHG500");
            snapshot.dashboard_detail.charger_notice =
                Some("manual_loopback_confirmed_charging_500ma");
            snapshot.bq25792_allow_charge = Some(true);
            snapshot.bq25792_ichg_ma = Some(500);
            snapshot.bq25792_ibat_ma = Some(480);
            snapshot.bq40z50_current_ma = Some(470);
        }
        ManualChargePreviewState::StopHold => {
            snapshot.dashboard_detail.manual_charge.runtime.stop_inhibit = true;
            snapshot
                .dashboard_detail
                .manual_charge
                .runtime
                .last_stop_reason = ManualChargeStopReason::UserStop;
            snapshot.dashboard_detail.charger_notice = Some("manual_user_stop_inhibit");
        }
        ManualChargePreviewState::ResetAuto => {
            snapshot.dashboard_detail.manual_charge.prefs.target =
                front_panel_scene::ManualChargeTarget::Rsoc80;
            snapshot.dashboard_detail.manual_charge.prefs.speed =
                front_panel_scene::ManualChargeSpeed::Ma1000;
            snapshot.dashboard_detail.manual_charge.prefs.timer_limit =
                front_panel_scene::ManualChargeTimerLimit::H6;
        }
        ManualChargePreviewState::Blocked => {
            snapshot.fusb302_vbus_present = Some(false);
            snapshot.input_vbus_mv = None;
            snapshot.input_ibus_ma = None;
            snapshot.vin_vbus_mv = None;
            snapshot.vin_iin_ma = None;
            snapshot
                .dashboard_detail
                .manual_charge
                .runtime
                .last_stop_reason = ManualChargeStopReason::SafetyBlocked;
            snapshot.dashboard_detail.charger_status = Some("NOAC");
            snapshot.dashboard_detail.charger_home_status = Some("NOAC");
            snapshot.dashboard_detail.charger_notice = Some("manual_safety_blocked");
        }
    }

    (mode, DashboardRoute::ManualCharge, snapshot)
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
        ScenarioArg::FirmwareSafeMode => SelfCheckOverlay::None,
        ScenarioArg::SelfCheckOutAFailed => {
            snapshot.mode = UpsMode::Blocked;
            snapshot.bq40z50 = SelfCheckCommState::Ok;
            snapshot.bq40z50_discharge_ready = Some(true);
            snapshot.bq40z50_no_battery = Some(false);
            snapshot.bq25792_vbat_present = Some(true);
            snapshot.requested_outputs = mains_aegis_firmware::output_state::EnabledOutputs::Both;
            snapshot.active_outputs = mains_aegis_firmware::output_state::EnabledOutputs::Only(
                mains_aegis_firmware::output_state::OutputSelector::OutB,
            );
            snapshot.recoverable_outputs = mains_aegis_firmware::output_state::EnabledOutputs::Only(
                mains_aegis_firmware::output_state::OutputSelector::OutB,
            );
            snapshot.tps_a = SelfCheckCommState::Err;
            snapshot.tps_a_enabled = Some(false);
            snapshot.out_a_vbus_mv = None;
            snapshot.tps_a_iout_ma = None;
            snapshot.tps_b = SelfCheckCommState::Ok;
            snapshot.tps_b_enabled = Some(true);
            snapshot.out_b_vbus_mv = Some(11_376);
            snapshot.tps_b_iout_ma = Some(420);
            SelfCheckOverlay::None
        }
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
            snapshot.requested_outputs = mains_aegis_firmware::output_state::EnabledOutputs::Only(
                mains_aegis_firmware::output_state::OutputSelector::OutA,
            );
            snapshot.active_outputs = mains_aegis_firmware::output_state::EnabledOutputs::None;
            snapshot.recoverable_outputs = mains_aegis_firmware::output_state::EnabledOutputs::Only(
                mains_aegis_firmware::output_state::OutputSelector::OutA,
            );
            snapshot.output_gate_reason =
                mains_aegis_firmware::output_state::OutputGateReason::BmsNotReady;
            SelfCheckOverlay::None
        }
        ScenarioArg::Bq40Offline => SelfCheckOverlay::None,
        ScenarioArg::Bq40OfflineDialog => SelfCheckOverlay::BmsActivateConfirm,
        ScenarioArg::Bq40DischargeBlocked => {
            snapshot.mode = UpsMode::Blocked;
            snapshot.bq40z50 = SelfCheckCommState::Warn;
            snapshot.bq40z50_pack_mv = Some(15_420);
            snapshot.bq40z50_current_ma = Some(115);
            snapshot.bq40z50_soc_pct = Some(76);
            snapshot.bq40z50_rca_alarm = Some(false);
            snapshot.bq40z50_no_battery = Some(false);
            snapshot.bq40z50_discharge_ready = Some(false);
            snapshot.bq40z50_issue_detail = Some("xdsg_blocked");
            snapshot.bq40z50_recovery_action = Some(BmsRecoveryUiAction::DischargeAuthorization);
            snapshot.requested_outputs = mains_aegis_firmware::output_state::EnabledOutputs::Only(
                mains_aegis_firmware::output_state::OutputSelector::OutA,
            );
            snapshot.active_outputs = mains_aegis_firmware::output_state::EnabledOutputs::None;
            snapshot.recoverable_outputs = mains_aegis_firmware::output_state::EnabledOutputs::Only(
                mains_aegis_firmware::output_state::OutputSelector::OutA,
            );
            snapshot.output_gate_reason =
                mains_aegis_firmware::output_state::OutputGateReason::BmsNotReady;
            snapshot.bq25792_allow_charge = Some(true);
            snapshot.bq25792_vbat_present = Some(true);
            snapshot.tps_a = SelfCheckCommState::Warn;
            snapshot.tps_a_enabled = Some(false);
            snapshot.out_a_vbus_mv = None;
            snapshot.tps_a_iout_ma = None;
            SelfCheckOverlay::None
        }
        ScenarioArg::Bq40EmshutBlocked => {
            snapshot.mode = UpsMode::Blocked;
            snapshot.bq40z50 = SelfCheckCommState::Warn;
            snapshot.bq40z50_pack_mv = Some(16_270);
            snapshot.bq40z50_current_ma = Some(0);
            snapshot.bq40z50_soc_pct = Some(99);
            snapshot.bq40z50_rca_alarm = Some(false);
            snapshot.bq40z50_no_battery = Some(false);
            snapshot.bq40z50_discharge_ready = Some(false);
            snapshot.bq40z50_issue_detail = Some("emshut_active");
            snapshot.bq40z50_recovery_action = Some(BmsRecoveryUiAction::DischargeAuthorization);
            snapshot.requested_outputs = mains_aegis_firmware::output_state::EnabledOutputs::Both;
            snapshot.active_outputs = mains_aegis_firmware::output_state::EnabledOutputs::None;
            snapshot.recoverable_outputs = mains_aegis_firmware::output_state::EnabledOutputs::Both;
            snapshot.output_gate_reason =
                mains_aegis_firmware::output_state::OutputGateReason::BmsNotReady;
            snapshot.bq25792_allow_charge = Some(true);
            snapshot.bq25792_vbat_present = Some(false);
            snapshot.tps_a = SelfCheckCommState::Warn;
            snapshot.tps_a_enabled = Some(false);
            snapshot.out_a_vbus_mv = None;
            snapshot.tps_a_iout_ma = None;
            snapshot.tps_b = SelfCheckCommState::Warn;
            snapshot.tps_b_enabled = Some(false);
            snapshot.out_b_vbus_mv = None;
            snapshot.tps_b_iout_ma = None;
            SelfCheckOverlay::None
        }
        ScenarioArg::Bq40EmshutDialog => {
            let (blocked, _) = bq40_snapshot_for_scenario(mode, ScenarioArg::Bq40EmshutBlocked);
            snapshot = blocked;
            SelfCheckOverlay::BmsDischargeAuthorizeConfirm
        }
        ScenarioArg::Bq40DischargeDialog => {
            let (_, overlay) = bq40_snapshot_for_scenario(mode, ScenarioArg::Bq40DischargeBlocked);
            let mut blocked = base_bq40_snapshot(mode);
            blocked.mode = UpsMode::Blocked;
            blocked.bq40z50 = SelfCheckCommState::Warn;
            blocked.bq40z50_pack_mv = Some(15_420);
            blocked.bq40z50_current_ma = Some(115);
            blocked.bq40z50_soc_pct = Some(76);
            blocked.bq40z50_rca_alarm = Some(false);
            blocked.bq40z50_no_battery = Some(false);
            blocked.bq40z50_discharge_ready = Some(false);
            blocked.bq40z50_issue_detail = Some("xdsg_blocked");
            blocked.bq40z50_recovery_action = Some(BmsRecoveryUiAction::DischargeAuthorization);
            blocked.requested_outputs = mains_aegis_firmware::output_state::EnabledOutputs::Only(
                mains_aegis_firmware::output_state::OutputSelector::OutA,
            );
            blocked.active_outputs = mains_aegis_firmware::output_state::EnabledOutputs::None;
            blocked.recoverable_outputs = mains_aegis_firmware::output_state::EnabledOutputs::Only(
                mains_aegis_firmware::output_state::OutputSelector::OutA,
            );
            blocked.output_gate_reason =
                mains_aegis_firmware::output_state::OutputGateReason::BmsNotReady;
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
            snapshot.mode = UpsMode::Blocked;
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
            snapshot.requested_outputs = mains_aegis_firmware::output_state::EnabledOutputs::Only(
                mains_aegis_firmware::output_state::OutputSelector::OutA,
            );
            snapshot.active_outputs = mains_aegis_firmware::output_state::EnabledOutputs::None;
            snapshot.recoverable_outputs = mains_aegis_firmware::output_state::EnabledOutputs::Only(
                mains_aegis_firmware::output_state::OutputSelector::OutA,
            );
            snapshot.output_gate_reason =
                mains_aegis_firmware::output_state::OutputGateReason::BmsNotReady;
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
            snapshot.requested_outputs = mains_aegis_firmware::output_state::EnabledOutputs::Only(
                mains_aegis_firmware::output_state::OutputSelector::OutA,
            );
            snapshot.active_outputs = mains_aegis_firmware::output_state::EnabledOutputs::Only(
                mains_aegis_firmware::output_state::OutputSelector::OutA,
            );
            snapshot.recoverable_outputs = snapshot.active_outputs;
            snapshot.output_gate_reason =
                mains_aegis_firmware::output_state::OutputGateReason::None;
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
            snapshot.bq40z50_soc_pct = Some(77);
            snapshot.bq40z50_rca_alarm = Some(false);
            snapshot.bq40z50_discharge_ready = Some(false);
            snapshot.bq40z50_issue_detail = Some("xdsg_blocked");
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
        ScenarioArg::Bq40IssueDialog => {
            snapshot.bq25792_vbat_present = Some(false);
            snapshot.bq40z50 = SelfCheckCommState::Warn;
            snapshot.bq40z50_no_battery = Some(true);
            snapshot.bq40z50_discharge_ready = Some(false);
            snapshot.bq40z50_issue_detail = Some("no_battery");
            snapshot.bq40z50_recovery_action = None;
            snapshot.bq40z50_last_result = Some(BmsResultKind::NoBattery);
            SelfCheckOverlay::HardwareIssue(SelfCheckHardwareTarget::Bq40z50)
        }
        ScenarioArg::TpsAIssueDialog => {
            snapshot.bq40z50 = SelfCheckCommState::Warn;
            snapshot.bq40z50_pack_mv = Some(15_420);
            snapshot.bq40z50_current_ma = Some(115);
            snapshot.bq40z50_soc_pct = Some(76);
            snapshot.bq40z50_rca_alarm = Some(false);
            snapshot.bq40z50_no_battery = Some(false);
            snapshot.bq40z50_discharge_ready = Some(false);
            snapshot.bq40z50_issue_detail = Some("xdsg_blocked");
            snapshot.bq40z50_recovery_action = None;
            snapshot.requested_outputs = mains_aegis_firmware::output_state::EnabledOutputs::Only(
                mains_aegis_firmware::output_state::OutputSelector::OutA,
            );
            snapshot.active_outputs = mains_aegis_firmware::output_state::EnabledOutputs::None;
            snapshot.recoverable_outputs = mains_aegis_firmware::output_state::EnabledOutputs::Only(
                mains_aegis_firmware::output_state::OutputSelector::OutA,
            );
            snapshot.output_gate_reason =
                mains_aegis_firmware::output_state::OutputGateReason::BmsNotReady;
            snapshot.tps_a = SelfCheckCommState::Warn;
            snapshot.tps_a_enabled = Some(false);
            snapshot.out_a_vbus_mv = None;
            snapshot.tps_a_iout_ma = None;
            SelfCheckOverlay::HardwareIssue(SelfCheckHardwareTarget::TpsA)
        }
        ScenarioArg::Default
        | ScenarioArg::DisplayDiag
        | ScenarioArg::DashboardAlert
        | ScenarioArg::AlertList
        | ScenarioArg::AlertDetail
        | ScenarioArg::DashboardRuntimeStandby
        | ScenarioArg::DashboardRuntimeStandbyTouchZones
        | ScenarioArg::DashboardRuntimeStandbyWifiDisabled
        | ScenarioArg::DashboardRuntimeStandbyWifiConnecting
        | ScenarioArg::DashboardRuntimeStandbyWifiConnectedWeak
        | ScenarioArg::DashboardRuntimeStandbyWifiConnectedMedium
        | ScenarioArg::DashboardRuntimeStandbyWifiConnected
        | ScenarioArg::DashboardRuntimeStandbyWifiError
        | ScenarioArg::DashboardHomeFocusOutput
        | ScenarioArg::DashboardHomeFocusBatteryFlow
        | ScenarioArg::DashboardMenuDashboard
        | ScenarioArg::DashboardMenuBeeper
        | ScenarioArg::DashboardMenuConceptDenseBadge
        | ScenarioArg::DashboardMenuConceptDockBar
        | ScenarioArg::DashboardMenuConceptSplitRail
        | ScenarioArg::DashboardMenuConceptSignalPlate
        | ScenarioArg::DashboardAudioActionFocus
        | ScenarioArg::DashboardAudioSystemFocus
        | ScenarioArg::DashboardAudioSystemOff
        | ScenarioArg::DashboardAudioTouchZones
        | ScenarioArg::DashboardMenuTransitionMid
        | ScenarioArg::DashboardMenuTransitionEnd
        | ScenarioArg::DashboardRuntimeAssist
        | ScenarioArg::DashboardRuntimeBackup
        | ScenarioArg::DashboardDetailCells
        | ScenarioArg::DashboardDetailCellsBalanceActive
        | ScenarioArg::DashboardDetailCellsBalanceIdle
        | ScenarioArg::DashboardDetailCellsBalanceConfigMismatch
        | ScenarioArg::DashboardDetailBms
        | ScenarioArg::DashboardDetailBmsChargeBlocked
        | ScenarioArg::DashboardDetailBmsBalanceMulti
        | ScenarioArg::DashboardDetailBmsNoData
        | ScenarioArg::DashboardDetailBatteryFlow
        | ScenarioArg::DashboardDetailOutput
        | ScenarioArg::DashboardDetailCharger
        | ScenarioArg::DashboardDetailThermal
        | ScenarioArg::DashboardDetailWifiConnected
        | ScenarioArg::DashboardDetailWifiConnectedLongIp
        | ScenarioArg::DashboardDetailWifiDisabled
        | ScenarioArg::DashboardDetailThermalTestMode
        | ScenarioArg::DashboardDetailThermKillAsserted
        | ScenarioArg::DashboardDetailChargerWait
        | ScenarioArg::DashboardDetailCharger500mA
        | ScenarioArg::DashboardDetailChargerBackupUsbLowOutput
        | ScenarioArg::DashboardDetailChargerBackupUsbOutputHighLatched
        | ScenarioArg::DashboardDetailChargerBackupUsbTelemetryLostLatched
        | ScenarioArg::DashboardDetailChargerWarm
        | ScenarioArg::DashboardDetailCharger100mADcDerated
        | ScenarioArg::DashboardDetailChargerRecovery
        | ScenarioArg::DashboardDetailChargerFullLatched
        | ScenarioArg::DashboardDetailChargerBlockedOutputOverload
        | ScenarioArg::DashboardDetailChargerBlockedOutputUnknown
        | ScenarioArg::DashboardDetailChargerBlockedNoBms
        | ScenarioArg::DashboardManualChargeDefault
        | ScenarioArg::DashboardManualChargeAutoCharging
        | ScenarioArg::DashboardManualChargeActive
        | ScenarioArg::DashboardManualChargeLoopbackConfirm
        | ScenarioArg::DashboardManualChargeLoopbackConfirmed
        | ScenarioArg::DashboardManualChargeStopHold
        | ScenarioArg::DashboardManualChargeResetAuto
        | ScenarioArg::DashboardManualChargeBlocked
        | ScenarioArg::WifiIconGallery
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
        ScenarioArg::DashboardAlert | ScenarioArg::AlertList | ScenarioArg::AlertDetail => {
            ModeArg::Standby
        }
        ScenarioArg::DashboardRuntimeStandby => ModeArg::Standby,
        ScenarioArg::DashboardRuntimeStandbyTouchZones => ModeArg::Standby,
        ScenarioArg::DashboardRuntimeStandbyWifiDisabled => ModeArg::Standby,
        ScenarioArg::DashboardRuntimeStandbyWifiConnecting => ModeArg::Standby,
        ScenarioArg::DashboardRuntimeStandbyWifiConnectedWeak => ModeArg::Standby,
        ScenarioArg::DashboardRuntimeStandbyWifiConnectedMedium => ModeArg::Standby,
        ScenarioArg::DashboardRuntimeStandbyWifiConnected => ModeArg::Standby,
        ScenarioArg::DashboardRuntimeStandbyWifiError => ModeArg::Standby,
        ScenarioArg::WifiIconGallery => ModeArg::Standby,
        ScenarioArg::DashboardRuntimeAssist => ModeArg::Supplement,
        ScenarioArg::DashboardRuntimeBackup => ModeArg::Backup,
        ScenarioArg::DashboardHomeFocusOutput => ModeArg::Standby,
        ScenarioArg::DashboardHomeFocusBatteryFlow => ModeArg::Standby,
        ScenarioArg::DashboardMenuDashboard => ModeArg::Standby,
        ScenarioArg::DashboardMenuBeeper => ModeArg::Standby,
        ScenarioArg::DashboardMenuConceptDenseBadge => ModeArg::Standby,
        ScenarioArg::DashboardMenuConceptDockBar => ModeArg::Standby,
        ScenarioArg::DashboardMenuConceptSplitRail => ModeArg::Standby,
        ScenarioArg::DashboardMenuConceptSignalPlate => ModeArg::Standby,
        ScenarioArg::DashboardAudioActionFocus => ModeArg::Standby,
        ScenarioArg::DashboardAudioSystemFocus => ModeArg::Standby,
        ScenarioArg::DashboardAudioSystemOff => ModeArg::Standby,
        ScenarioArg::DashboardAudioTouchZones => ModeArg::Standby,
        ScenarioArg::DashboardMenuTransitionMid => ModeArg::Standby,
        ScenarioArg::DashboardMenuTransitionEnd => ModeArg::Standby,
        ScenarioArg::DashboardDetailCells => ModeArg::Standby,
        ScenarioArg::DashboardDetailCellsBalanceActive => ModeArg::Standby,
        ScenarioArg::DashboardDetailCellsBalanceIdle => ModeArg::Standby,
        ScenarioArg::DashboardDetailCellsBalanceConfigMismatch => ModeArg::Standby,
        ScenarioArg::DashboardDetailBms => ModeArg::Standby,
        ScenarioArg::DashboardDetailBmsChargeBlocked => ModeArg::Standby,
        ScenarioArg::DashboardDetailBmsBalanceMulti => ModeArg::Standby,
        ScenarioArg::DashboardDetailBmsNoData => ModeArg::Standby,
        ScenarioArg::DashboardDetailBatteryFlow => ModeArg::Backup,
        ScenarioArg::DashboardDetailOutput => ModeArg::Supplement,
        ScenarioArg::DashboardDetailCharger => ModeArg::Standby,
        ScenarioArg::DashboardDetailThermal => ModeArg::Backup,
        ScenarioArg::DashboardDetailWifiConnected => ModeArg::Standby,
        ScenarioArg::DashboardDetailWifiConnectedLongIp => ModeArg::Standby,
        ScenarioArg::DashboardDetailWifiDisabled => ModeArg::Standby,
        ScenarioArg::DashboardDetailChargerWait => ModeArg::Standby,
        ScenarioArg::DashboardDetailCharger500mA => ModeArg::Standby,
        ScenarioArg::DashboardDetailChargerBackupUsbLowOutput => ModeArg::Backup,
        ScenarioArg::DashboardDetailChargerBackupUsbOutputHighLatched => ModeArg::Backup,
        ScenarioArg::DashboardDetailChargerBackupUsbTelemetryLostLatched => ModeArg::Backup,
        ScenarioArg::DashboardDetailChargerWarm => ModeArg::Standby,
        ScenarioArg::DashboardDetailCharger100mADcDerated => ModeArg::Standby,
        ScenarioArg::DashboardDetailChargerFullLatched => ModeArg::Standby,
        ScenarioArg::DashboardDetailChargerBlockedNoBms => ModeArg::Standby,
        ScenarioArg::DashboardManualChargeDefault => ModeArg::Standby,
        ScenarioArg::DashboardManualChargeAutoCharging => ModeArg::Standby,
        ScenarioArg::DashboardManualChargeActive => ModeArg::Standby,
        ScenarioArg::DashboardManualChargeLoopbackConfirm => ModeArg::Backup,
        ScenarioArg::DashboardManualChargeLoopbackConfirmed => ModeArg::Backup,
        ScenarioArg::DashboardManualChargeStopHold => ModeArg::Standby,
        ScenarioArg::DashboardManualChargeResetAuto => ModeArg::Standby,
        ScenarioArg::DashboardManualChargeBlocked => ModeArg::Standby,
        _ => args.mode,
    };

    let frame_dir = args
        .out_dir
        .join(format!("variant-{}", args.variant.as_tag()))
        .join(format!("mode-{}", effective_mode.as_tag()))
        .join(format!("focus-{}", args.focus.as_tag()))
        .join(format!("scenario-{}", args.output_tag()));
    fs::create_dir_all(&frame_dir).map_err(|e| format!("create output dir failed: {e}"))?;

    let mut framebuffer = FrameBuffer::new(UI_W as usize, UI_H as usize);
    let model = UiModel {
        mode: effective_mode.into_scene(),
        focus: args.focus.into_scene(),
        touch_irq: args.focus.into_scene() == UiFocus::Touch,
        frame_no: args.frame_no,
    };

    match args.scenario {
        ScenarioArg::FirmwareSafeMode => {
            front_panel_scene::render_firmware_safe_mode(
                &mut framebuffer,
                args.variant.into_scene(),
                "watchdog",
                3,
            )
            .map_err(|_| "render failed unexpectedly".to_string())?;
        }
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
        ScenarioArg::DashboardAlert => {
            let snapshot = dashboard_runtime_snapshot_for_wifi(WifiPreviewState::Connected);
            let dashboard_model = UiModel {
                mode: UpsMode::Standby,
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
            front_panel_scene::draw_dashboard_alert_preview_indicator(
                &mut framebuffer,
                UiVariant::InstrumentB,
                args.alert_severity,
                args.alert_sound,
                args.frame_no,
            )
            .map_err(|_| "alert indicator render failed unexpectedly".to_string())?;
        }
        ScenarioArg::AlertList => {
            let alerts = alert_preview_items(args.alert_list, &args);
            front_panel_scene::render_alert_list_preview(
                &mut framebuffer,
                UiVariant::InstrumentB,
                &alerts,
                args.alert_selected,
                args.alert_top,
                args.alert_touch_overlay,
            )
            .map_err(|_| "alert list render failed unexpectedly".to_string())?;
        }
        ScenarioArg::AlertDetail => {
            let alert = if args.alert_cleared {
                AlertPreviewItem::cleared(args.alert_kind)
            } else {
                AlertPreviewItem::active(args.alert_kind, args.alert_severity, args.alert_sound)
            };
            front_panel_scene::render_alert_detail_preview(
                &mut framebuffer,
                UiVariant::InstrumentB,
                alert,
                args.alert_touch_overlay,
            )
            .map_err(|_| "alert detail render failed unexpectedly".to_string())?;
        }
        ScenarioArg::WifiIconGallery => {
            front_panel_scene::render_wifi_icon_gallery(&mut framebuffer, UiVariant::InstrumentB)
                .map_err(|_| "render failed unexpectedly".to_string())?;
        }
        ScenarioArg::DashboardRuntimeStandby
        | ScenarioArg::DashboardRuntimeStandbyTouchZones
        | ScenarioArg::DashboardRuntimeStandbyWifiDisabled
        | ScenarioArg::DashboardRuntimeStandbyWifiConnecting
        | ScenarioArg::DashboardRuntimeStandbyWifiConnectedWeak
        | ScenarioArg::DashboardRuntimeStandbyWifiConnectedMedium
        | ScenarioArg::DashboardRuntimeStandbyWifiConnected
        | ScenarioArg::DashboardRuntimeStandbyWifiError
        | ScenarioArg::DashboardRuntimeAssist
        | ScenarioArg::DashboardRuntimeBackup => {
            let (mode, snapshot) = match args.scenario {
                ScenarioArg::DashboardRuntimeStandby => (
                    UpsMode::Standby,
                    dashboard_snapshot_for_mode(UpsMode::Standby),
                ),
                ScenarioArg::DashboardRuntimeStandbyTouchZones => (
                    UpsMode::Standby,
                    dashboard_runtime_snapshot_for_wifi(WifiPreviewState::Connected),
                ),
                ScenarioArg::DashboardRuntimeStandbyWifiDisabled => (
                    UpsMode::Standby,
                    dashboard_runtime_snapshot_for_wifi(WifiPreviewState::Disabled),
                ),
                ScenarioArg::DashboardRuntimeStandbyWifiConnecting => (
                    UpsMode::Standby,
                    dashboard_runtime_snapshot_for_wifi(WifiPreviewState::Connecting),
                ),
                ScenarioArg::DashboardRuntimeStandbyWifiConnectedWeak => (
                    UpsMode::Standby,
                    dashboard_runtime_snapshot_for_wifi(WifiPreviewState::ConnectedWeak),
                ),
                ScenarioArg::DashboardRuntimeStandbyWifiConnectedMedium => (
                    UpsMode::Standby,
                    dashboard_runtime_snapshot_for_wifi(WifiPreviewState::ConnectedMedium),
                ),
                ScenarioArg::DashboardRuntimeStandbyWifiConnected => (
                    UpsMode::Standby,
                    dashboard_runtime_snapshot_for_wifi(WifiPreviewState::Connected),
                ),
                ScenarioArg::DashboardRuntimeStandbyWifiError => (
                    UpsMode::Standby,
                    dashboard_runtime_snapshot_for_wifi(WifiPreviewState::Error),
                ),
                ScenarioArg::DashboardRuntimeAssist => (
                    UpsMode::Supplement,
                    dashboard_snapshot_for_mode(UpsMode::Supplement),
                ),
                ScenarioArg::DashboardRuntimeBackup => (
                    UpsMode::Backup,
                    dashboard_snapshot_for_mode(UpsMode::Backup),
                ),
                _ => unreachable!(),
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
                DashboardRoute::Home,
                Some(&snapshot),
                SelfCheckOverlay::None,
            )
            .map_err(|_| "render failed unexpectedly".to_string())?;
            if matches!(
                args.scenario,
                ScenarioArg::DashboardRuntimeStandbyTouchZones
            ) {
                front_panel_scene::render_dashboard_touch_regions_overlay(
                    &mut framebuffer,
                    UiVariant::InstrumentB,
                    DashboardRoute::Home,
                )
                .map_err(|_| "touch overlay render failed unexpectedly".to_string())?;
            }
        }
        ScenarioArg::DashboardHomeFocusOutput
        | ScenarioArg::DashboardHomeFocusBatteryFlow
        | ScenarioArg::DashboardMenuDashboard
        | ScenarioArg::DashboardMenuBeeper
        | ScenarioArg::DashboardMenuConceptDenseBadge
        | ScenarioArg::DashboardMenuConceptDockBar
        | ScenarioArg::DashboardMenuConceptSplitRail
        | ScenarioArg::DashboardMenuConceptSignalPlate
        | ScenarioArg::DashboardAudioActionFocus
        | ScenarioArg::DashboardAudioSystemFocus
        | ScenarioArg::DashboardAudioSystemOff
        | ScenarioArg::DashboardAudioTouchZones
        | ScenarioArg::DashboardMenuTransitionMid
        | ScenarioArg::DashboardMenuTransitionEnd => {
            let (mode, shell, snapshot) = match args.scenario {
                ScenarioArg::DashboardHomeFocusOutput => dashboard_shell_fixture(
                    DashboardPrimaryPage::DashboardHome,
                    DashboardHomeFocus::Output,
                    MenuItem::Dashboard,
                    DashboardMenuStyle::default_preview(),
                    BeeperPrefs::defaults(),
                    0,
                ),
                ScenarioArg::DashboardHomeFocusBatteryFlow => dashboard_shell_fixture(
                    DashboardPrimaryPage::DashboardHome,
                    DashboardHomeFocus::BatteryFlow,
                    MenuItem::Dashboard,
                    DashboardMenuStyle::default_preview(),
                    BeeperPrefs::defaults(),
                    0,
                ),
                ScenarioArg::DashboardMenuDashboard => dashboard_shell_fixture(
                    DashboardPrimaryPage::Menu,
                    DashboardHomeFocus::Output,
                    MenuItem::Dashboard,
                    DashboardMenuStyle::default_preview(),
                    BeeperPrefs::defaults(),
                    UI_H as i16,
                ),
                ScenarioArg::DashboardMenuBeeper => dashboard_shell_fixture(
                    DashboardPrimaryPage::Menu,
                    DashboardHomeFocus::Charger,
                    MenuItem::Beeper,
                    DashboardMenuStyle::default_preview(),
                    BeeperPrefs::defaults(),
                    UI_H as i16,
                ),
                ScenarioArg::DashboardMenuConceptDenseBadge => dashboard_shell_fixture(
                    DashboardPrimaryPage::Menu,
                    DashboardHomeFocus::Output,
                    MenuItem::Dashboard,
                    DashboardMenuStyle::DenseBadge,
                    BeeperPrefs::defaults(),
                    UI_H as i16,
                ),
                ScenarioArg::DashboardMenuConceptDockBar => dashboard_shell_fixture(
                    DashboardPrimaryPage::Menu,
                    DashboardHomeFocus::Output,
                    MenuItem::Dashboard,
                    DashboardMenuStyle::DockBar,
                    BeeperPrefs::defaults(),
                    UI_H as i16,
                ),
                ScenarioArg::DashboardMenuConceptSplitRail => dashboard_shell_fixture(
                    DashboardPrimaryPage::Menu,
                    DashboardHomeFocus::Output,
                    MenuItem::Dashboard,
                    DashboardMenuStyle::SplitRail,
                    BeeperPrefs::defaults(),
                    UI_H as i16,
                ),
                ScenarioArg::DashboardMenuConceptSignalPlate => dashboard_shell_fixture(
                    DashboardPrimaryPage::Menu,
                    DashboardHomeFocus::Output,
                    MenuItem::Dashboard,
                    DashboardMenuStyle::SignalPlate,
                    BeeperPrefs::defaults(),
                    UI_H as i16,
                ),
                ScenarioArg::DashboardAudioActionFocus => dashboard_shell_fixture(
                    DashboardPrimaryPage::BeeperSettings,
                    DashboardHomeFocus::Charger,
                    MenuItem::Beeper,
                    DashboardMenuStyle::default_preview(),
                    BeeperPrefs::new(
                        BeeperVolumeLevel::L2,
                        BeeperVolumeLevel::L6,
                        BeeperSettingTarget::Action,
                    ),
                    UI_H as i16,
                ),
                ScenarioArg::DashboardAudioSystemFocus => dashboard_shell_fixture(
                    DashboardPrimaryPage::BeeperSettings,
                    DashboardHomeFocus::Charger,
                    MenuItem::Beeper,
                    DashboardMenuStyle::default_preview(),
                    BeeperPrefs::new(
                        BeeperVolumeLevel::L2,
                        BeeperVolumeLevel::L4,
                        BeeperSettingTarget::System,
                    ),
                    UI_H as i16,
                ),
                ScenarioArg::DashboardAudioSystemOff => dashboard_shell_fixture(
                    DashboardPrimaryPage::BeeperSettings,
                    DashboardHomeFocus::Charger,
                    MenuItem::Beeper,
                    DashboardMenuStyle::default_preview(),
                    BeeperPrefs::new(
                        BeeperVolumeLevel::L3,
                        BeeperVolumeLevel::Off,
                        BeeperSettingTarget::System,
                    ),
                    UI_H as i16,
                ),
                ScenarioArg::DashboardAudioTouchZones => dashboard_shell_fixture(
                    DashboardPrimaryPage::BeeperSettings,
                    DashboardHomeFocus::Charger,
                    MenuItem::Beeper,
                    DashboardMenuStyle::default_preview(),
                    BeeperPrefs::new(
                        BeeperVolumeLevel::L2,
                        BeeperVolumeLevel::L4,
                        BeeperSettingTarget::Action,
                    ),
                    UI_H as i16,
                ),
                ScenarioArg::DashboardMenuTransitionMid => dashboard_shell_fixture(
                    DashboardPrimaryPage::Menu,
                    DashboardHomeFocus::Charger,
                    MenuItem::Beeper,
                    DashboardMenuStyle::default_preview(),
                    BeeperPrefs::defaults(),
                    (UI_H / 2) as i16,
                ),
                ScenarioArg::DashboardMenuTransitionEnd => dashboard_shell_fixture(
                    DashboardPrimaryPage::Menu,
                    DashboardHomeFocus::Charger,
                    MenuItem::Beeper,
                    DashboardMenuStyle::default_preview(),
                    BeeperPrefs::defaults(),
                    UI_H as i16,
                ),
                _ => unreachable!(),
            };
            let dashboard_model = UiModel {
                mode,
                focus: UiFocus::Idle,
                touch_irq: false,
                frame_no: args.frame_no,
            };
            front_panel_scene::render_dashboard_shell(
                &mut framebuffer,
                &dashboard_model,
                UiVariant::InstrumentB,
                shell,
                Some(&snapshot),
            )
            .map_err(|_| "render failed unexpectedly".to_string())?;
            if matches!(args.scenario, ScenarioArg::DashboardAudioTouchZones) {
                front_panel_scene::render_beeper_settings_touch_regions_overlay(
                    &mut framebuffer,
                    UiVariant::InstrumentB,
                )
                .map_err(|_| "touch overlay render failed unexpectedly".to_string())?;
            }
        }
        ScenarioArg::DashboardDetailCells
        | ScenarioArg::DashboardDetailCellsBalanceActive
        | ScenarioArg::DashboardDetailCellsBalanceIdle
        | ScenarioArg::DashboardDetailCellsBalanceConfigMismatch
        | ScenarioArg::DashboardDetailBms
        | ScenarioArg::DashboardDetailBmsChargeBlocked
        | ScenarioArg::DashboardDetailBmsBalanceMulti
        | ScenarioArg::DashboardDetailBmsNoData
        | ScenarioArg::DashboardDetailBatteryFlow
        | ScenarioArg::DashboardDetailOutput
        | ScenarioArg::DashboardDetailCharger
        | ScenarioArg::DashboardDetailThermal
        | ScenarioArg::DashboardDetailWifiConnected
        | ScenarioArg::DashboardDetailWifiConnectedLongIp
        | ScenarioArg::DashboardDetailWifiDisabled
        | ScenarioArg::DashboardDetailThermalTestMode
        | ScenarioArg::DashboardDetailThermKillAsserted
        | ScenarioArg::DashboardDetailChargerWait
        | ScenarioArg::DashboardDetailCharger500mA
        | ScenarioArg::DashboardDetailChargerBackupUsbLowOutput
        | ScenarioArg::DashboardDetailChargerBackupUsbOutputHighLatched
        | ScenarioArg::DashboardDetailChargerBackupUsbTelemetryLostLatched
        | ScenarioArg::DashboardDetailChargerWarm
        | ScenarioArg::DashboardDetailCharger100mADcDerated
        | ScenarioArg::DashboardDetailChargerRecovery
        | ScenarioArg::DashboardDetailChargerFullLatched
        | ScenarioArg::DashboardDetailChargerBlockedOutputOverload
        | ScenarioArg::DashboardDetailChargerBlockedOutputUnknown
        | ScenarioArg::DashboardDetailChargerBlockedNoBms => {
            let page = match args.scenario {
                ScenarioArg::DashboardDetailCells
                | ScenarioArg::DashboardDetailCellsBalanceActive
                | ScenarioArg::DashboardDetailCellsBalanceIdle
                | ScenarioArg::DashboardDetailCellsBalanceConfigMismatch => {
                    DashboardDetailPage::Cells
                }
                ScenarioArg::DashboardDetailBms
                | ScenarioArg::DashboardDetailBmsChargeBlocked
                | ScenarioArg::DashboardDetailBmsBalanceMulti
                | ScenarioArg::DashboardDetailBmsNoData => DashboardDetailPage::BmsDetail,
                ScenarioArg::DashboardDetailBatteryFlow => DashboardDetailPage::BatteryFlow,
                ScenarioArg::DashboardDetailOutput => DashboardDetailPage::Output,
                ScenarioArg::DashboardDetailCharger => DashboardDetailPage::Charger,
                ScenarioArg::DashboardDetailThermal
                | ScenarioArg::DashboardDetailThermalTestMode
                | ScenarioArg::DashboardDetailThermKillAsserted => DashboardDetailPage::Thermal,
                ScenarioArg::DashboardDetailWifiConnected
                | ScenarioArg::DashboardDetailWifiConnectedLongIp
                | ScenarioArg::DashboardDetailWifiDisabled => DashboardDetailPage::Wifi,
                ScenarioArg::DashboardDetailChargerWait
                | ScenarioArg::DashboardDetailCharger500mA
                | ScenarioArg::DashboardDetailChargerBackupUsbLowOutput
                | ScenarioArg::DashboardDetailChargerBackupUsbOutputHighLatched
                | ScenarioArg::DashboardDetailChargerBackupUsbTelemetryLostLatched
                | ScenarioArg::DashboardDetailChargerWarm
                | ScenarioArg::DashboardDetailCharger100mADcDerated
                | ScenarioArg::DashboardDetailChargerRecovery
                | ScenarioArg::DashboardDetailChargerFullLatched
                | ScenarioArg::DashboardDetailChargerBlockedOutputOverload
                | ScenarioArg::DashboardDetailChargerBlockedOutputUnknown
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
                ScenarioArg::DashboardDetailChargerBackupUsbLowOutput => {
                    charger_policy_snapshot_for_state(ChargerPolicyPreviewState::BackupUsbLowOutput)
                }
                ScenarioArg::DashboardDetailChargerBackupUsbOutputHighLatched => {
                    charger_policy_snapshot_for_state(
                        ChargerPolicyPreviewState::BackupUsbOutputHighLatched,
                    )
                }
                ScenarioArg::DashboardDetailChargerBackupUsbTelemetryLostLatched => {
                    charger_policy_snapshot_for_state(
                        ChargerPolicyPreviewState::BackupUsbTelemetryLostLatched,
                    )
                }
                ScenarioArg::DashboardDetailChargerWarm => {
                    charger_policy_snapshot_for_state(ChargerPolicyPreviewState::Warm)
                }
                ScenarioArg::DashboardDetailCharger100mADcDerated => {
                    charger_policy_snapshot_for_state(
                        ChargerPolicyPreviewState::Charge100mADcDerated,
                    )
                }
                ScenarioArg::DashboardDetailChargerRecovery => charger_policy_snapshot_for_state(
                    ChargerPolicyPreviewState::RecoveringLowVoltage,
                ),
                ScenarioArg::DashboardDetailChargerFullLatched => {
                    charger_policy_snapshot_for_state(ChargerPolicyPreviewState::FullLatched)
                }
                ScenarioArg::DashboardDetailChargerBlockedOutputOverload => {
                    charger_policy_snapshot_for_state(
                        ChargerPolicyPreviewState::BlockedOutputOverload,
                    )
                }
                ScenarioArg::DashboardDetailChargerBlockedOutputUnknown => {
                    charger_policy_snapshot_for_state(
                        ChargerPolicyPreviewState::BlockedOutputPowerUnknown,
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
                ScenarioArg::DashboardDetailCellsBalanceActive => {
                    dashboard_detail_snapshot_for_cells_balance(CellsBalancePreviewState::Active)
                }
                ScenarioArg::DashboardDetailCellsBalanceIdle => {
                    dashboard_detail_snapshot_for_cells_balance(CellsBalancePreviewState::Idle)
                }
                ScenarioArg::DashboardDetailCellsBalanceConfigMismatch => {
                    dashboard_detail_snapshot_for_cells_balance(
                        CellsBalancePreviewState::ConfigMismatch,
                    )
                }
                ScenarioArg::DashboardDetailBms => {
                    dashboard_detail_snapshot_for_bms_state(BmsDetailPreviewState::Nominal)
                }
                ScenarioArg::DashboardDetailBmsChargeBlocked => {
                    dashboard_detail_snapshot_for_bms_state(BmsDetailPreviewState::ChargeBlocked)
                }
                ScenarioArg::DashboardDetailBmsBalanceMulti => {
                    dashboard_detail_snapshot_for_bms_state(BmsDetailPreviewState::BalanceMulti)
                }
                ScenarioArg::DashboardDetailBmsNoData => {
                    dashboard_detail_snapshot_for_bms_state(BmsDetailPreviewState::NoData)
                }
                ScenarioArg::DashboardDetailWifiConnected => {
                    dashboard_detail_snapshot_for_wifi(WifiPreviewState::Connected)
                }
                ScenarioArg::DashboardDetailWifiConnectedLongIp => {
                    dashboard_detail_snapshot_for_wifi(WifiPreviewState::ConnectedLongIp)
                }
                ScenarioArg::DashboardDetailWifiDisabled => {
                    dashboard_detail_snapshot_for_wifi(WifiPreviewState::Disabled)
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
        ScenarioArg::DashboardManualChargeDefault
        | ScenarioArg::DashboardManualChargeAutoCharging
        | ScenarioArg::DashboardManualChargeActive
        | ScenarioArg::DashboardManualChargeLoopbackConfirm
        | ScenarioArg::DashboardManualChargeLoopbackConfirmed
        | ScenarioArg::DashboardManualChargeStopHold
        | ScenarioArg::DashboardManualChargeResetAuto
        | ScenarioArg::DashboardManualChargeBlocked => {
            let (mode, route, snapshot) = match args.scenario {
                ScenarioArg::DashboardManualChargeDefault => {
                    manual_charge_snapshot_for_state(ManualChargePreviewState::Default)
                }
                ScenarioArg::DashboardManualChargeAutoCharging => {
                    manual_charge_snapshot_for_state(ManualChargePreviewState::AutoCharging)
                }
                ScenarioArg::DashboardManualChargeActive => {
                    manual_charge_snapshot_for_state(ManualChargePreviewState::Active)
                }
                ScenarioArg::DashboardManualChargeLoopbackConfirm => {
                    manual_charge_snapshot_for_state(ManualChargePreviewState::LoopbackConfirm)
                }
                ScenarioArg::DashboardManualChargeLoopbackConfirmed => {
                    manual_charge_snapshot_for_state(ManualChargePreviewState::LoopbackConfirmed)
                }
                ScenarioArg::DashboardManualChargeStopHold => {
                    manual_charge_snapshot_for_state(ManualChargePreviewState::StopHold)
                }
                ScenarioArg::DashboardManualChargeResetAuto => {
                    manual_charge_snapshot_for_state(ManualChargePreviewState::ResetAuto)
                }
                ScenarioArg::DashboardManualChargeBlocked => {
                    manual_charge_snapshot_for_state(ManualChargePreviewState::Blocked)
                }
                _ => unreachable!(),
            };
            let dashboard_model = UiModel {
                mode,
                focus: UiFocus::Idle,
                touch_irq: false,
                frame_no: args.frame_no,
            };
            let overlay = if matches!(
                args.scenario,
                ScenarioArg::DashboardManualChargeLoopbackConfirm
            ) {
                SelfCheckOverlay::ManualChargeLoopbackConfirm
            } else {
                SelfCheckOverlay::None
            };
            front_panel_scene::render_frame_with_dashboard_route_overlay(
                &mut framebuffer,
                &dashboard_model,
                UiVariant::InstrumentB,
                route,
                Some(&snapshot),
                overlay,
            )
            .map_err(|_| "render failed unexpectedly".to_string())?;
        }
        ScenarioArg::Bq40Offline
        | ScenarioArg::SelfCheckOutAFailed
        | ScenarioArg::SelfCheckBmsMissingTpsWarn
        | ScenarioArg::Bq40OfflineDialog
        | ScenarioArg::Bq40DischargeBlocked
        | ScenarioArg::Bq40EmshutBlocked
        | ScenarioArg::Bq40EmshutDialog
        | ScenarioArg::Bq40DischargeDialog
        | ScenarioArg::Bq40DischargeRecovering
        | ScenarioArg::Bq40Activating
        | ScenarioArg::Bq40ResultSuccess
        | ScenarioArg::Bq40ResultNoBattery
        | ScenarioArg::Bq40ResultRomMode
        | ScenarioArg::Bq40ResultAbnormal
        | ScenarioArg::Bq40ResultNotDetected
        | ScenarioArg::Bq40IssueDialog
        | ScenarioArg::TpsAIssueDialog => {
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
            "blocked" | "block" | "lock" => Err(
                "unsupported --mode value: blocked (blocked is not dashboard-renderable)"
                    .to_string(),
            ),
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
            UpsMode::Blocked => unreachable!("demo focus never maps to blocked"),
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
    FirmwareSafeMode,
    DisplayDiag,
    DashboardAlert,
    AlertList,
    AlertDetail,
    DashboardRuntimeStandby,
    DashboardRuntimeStandbyTouchZones,
    DashboardRuntimeStandbyWifiDisabled,
    DashboardRuntimeStandbyWifiConnecting,
    DashboardRuntimeStandbyWifiConnectedWeak,
    DashboardRuntimeStandbyWifiConnectedMedium,
    DashboardRuntimeStandbyWifiConnected,
    DashboardRuntimeStandbyWifiError,
    WifiIconGallery,
    DashboardRuntimeAssist,
    DashboardRuntimeBackup,
    DashboardHomeFocusOutput,
    DashboardHomeFocusBatteryFlow,
    DashboardMenuDashboard,
    DashboardMenuBeeper,
    DashboardMenuConceptDenseBadge,
    DashboardMenuConceptDockBar,
    DashboardMenuConceptSplitRail,
    DashboardMenuConceptSignalPlate,
    DashboardAudioActionFocus,
    DashboardAudioSystemFocus,
    DashboardAudioSystemOff,
    DashboardAudioTouchZones,
    DashboardMenuTransitionMid,
    DashboardMenuTransitionEnd,
    DashboardDetailCells,
    DashboardDetailCellsBalanceActive,
    DashboardDetailCellsBalanceIdle,
    DashboardDetailCellsBalanceConfigMismatch,
    DashboardDetailBms,
    DashboardDetailBmsChargeBlocked,
    DashboardDetailBmsBalanceMulti,
    DashboardDetailBmsNoData,
    DashboardDetailBatteryFlow,
    DashboardDetailOutput,
    DashboardDetailCharger,
    DashboardDetailThermal,
    DashboardDetailWifiConnected,
    DashboardDetailWifiConnectedLongIp,
    DashboardDetailWifiDisabled,
    DashboardDetailThermalTestMode,
    DashboardDetailThermKillAsserted,
    DashboardDetailChargerWait,
    DashboardDetailCharger500mA,
    DashboardDetailChargerBackupUsbLowOutput,
    DashboardDetailChargerBackupUsbOutputHighLatched,
    DashboardDetailChargerBackupUsbTelemetryLostLatched,
    DashboardDetailChargerWarm,
    DashboardDetailCharger100mADcDerated,
    DashboardDetailChargerRecovery,
    DashboardDetailChargerFullLatched,
    DashboardDetailChargerBlockedOutputOverload,
    DashboardDetailChargerBlockedOutputUnknown,
    DashboardDetailChargerBlockedNoBms,
    DashboardManualChargeDefault,
    DashboardManualChargeAutoCharging,
    DashboardManualChargeActive,
    DashboardManualChargeLoopbackConfirm,
    DashboardManualChargeLoopbackConfirmed,
    DashboardManualChargeStopHold,
    DashboardManualChargeResetAuto,
    DashboardManualChargeBlocked,
    SelfCheckOutAFailed,
    SelfCheckBmsMissingTpsWarn,
    Bq40Offline,
    Bq40OfflineDialog,
    Bq40DischargeBlocked,
    Bq40EmshutBlocked,
    Bq40EmshutDialog,
    Bq40DischargeDialog,
    Bq40DischargeRecovering,
    Bq40Activating,
    Bq40ResultSuccess,
    Bq40ResultNoBattery,
    Bq40ResultRomMode,
    Bq40ResultAbnormal,
    Bq40ResultNotDetected,
    Bq40IssueDialog,
    TpsAIssueDialog,
    TpsTest,
    TestAudio,
    TestNavigation,
}

impl ScenarioArg {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw.to_ascii_lowercase().as_str() {
            "default" => Ok(Self::Default),
            "firmware-safe-mode" => Ok(Self::FirmwareSafeMode),
            "display-diag" => Ok(Self::DisplayDiag),
            "dashboard-alert" => Ok(Self::DashboardAlert),
            "alert-list" => Ok(Self::AlertList),
            "alert-detail" => Ok(Self::AlertDetail),
            "dashboard-runtime-standby" => Ok(Self::DashboardRuntimeStandby),
            "dashboard-runtime-standby-touch-zones" => Ok(Self::DashboardRuntimeStandbyTouchZones),
            "dashboard-runtime-standby-wifi-disabled" => {
                Ok(Self::DashboardRuntimeStandbyWifiDisabled)
            }
            "dashboard-runtime-standby-wifi-connecting" => {
                Ok(Self::DashboardRuntimeStandbyWifiConnecting)
            }
            "dashboard-runtime-standby-wifi-connected-weak" => {
                Ok(Self::DashboardRuntimeStandbyWifiConnectedWeak)
            }
            "dashboard-runtime-standby-wifi-connected-medium" => {
                Ok(Self::DashboardRuntimeStandbyWifiConnectedMedium)
            }
            "dashboard-runtime-standby-wifi-connected" => {
                Ok(Self::DashboardRuntimeStandbyWifiConnected)
            }
            "dashboard-runtime-standby-wifi-error" => Ok(Self::DashboardRuntimeStandbyWifiError),
            "wifi-icon-gallery" => Ok(Self::WifiIconGallery),
            "dashboard-runtime-assist" => Ok(Self::DashboardRuntimeAssist),
            "dashboard-runtime-backup" => Ok(Self::DashboardRuntimeBackup),
            "dashboard-home-focus-output" => Ok(Self::DashboardHomeFocusOutput),
            "dashboard-home-focus-battery-flow" => Ok(Self::DashboardHomeFocusBatteryFlow),
            "dashboard-menu-dashboard" => Ok(Self::DashboardMenuDashboard),
            "dashboard-menu-beeper" => Ok(Self::DashboardMenuBeeper),
            "dashboard-menu-concept-dense-badge" => Ok(Self::DashboardMenuConceptDenseBadge),
            "dashboard-menu-concept-dock-bar" => Ok(Self::DashboardMenuConceptDockBar),
            "dashboard-menu-concept-split-rail" => Ok(Self::DashboardMenuConceptSplitRail),
            "dashboard-menu-concept-signal-plate" => Ok(Self::DashboardMenuConceptSignalPlate),
            "dashboard-audio-action-focus" => Ok(Self::DashboardAudioActionFocus),
            "dashboard-audio-system-focus" => Ok(Self::DashboardAudioSystemFocus),
            "dashboard-audio-system-off" => Ok(Self::DashboardAudioSystemOff),
            "dashboard-audio-touch-zones" => Ok(Self::DashboardAudioTouchZones),
            "dashboard-beeper-volume-off" => Ok(Self::DashboardAudioSystemOff),
            "dashboard-beeper-volume-mid" => Ok(Self::DashboardAudioActionFocus),
            "dashboard-beeper-volume-max" => Ok(Self::DashboardAudioSystemFocus),
            "dashboard-menu-transition-mid" => Ok(Self::DashboardMenuTransitionMid),
            "dashboard-menu-transition-end" => Ok(Self::DashboardMenuTransitionEnd),
            "dashboard-detail-cells" => Ok(Self::DashboardDetailCells),
            "dashboard-detail-cells-balance-active" => Ok(Self::DashboardDetailCellsBalanceActive),
            "dashboard-detail-cells-balance-idle" => Ok(Self::DashboardDetailCellsBalanceIdle),
            "dashboard-detail-cells-balance-config-mismatch" => {
                Ok(Self::DashboardDetailCellsBalanceConfigMismatch)
            }
            "dashboard-detail-bms" => Ok(Self::DashboardDetailBms),
            "dashboard-detail-bms-charge-blocked" => Ok(Self::DashboardDetailBmsChargeBlocked),
            "dashboard-detail-bms-balance-multi" => Ok(Self::DashboardDetailBmsBalanceMulti),
            "dashboard-detail-bms-no-data" => Ok(Self::DashboardDetailBmsNoData),
            "dashboard-detail-battery-flow" => Ok(Self::DashboardDetailBatteryFlow),
            "dashboard-detail-output" => Ok(Self::DashboardDetailOutput),
            "dashboard-detail-charger" => Ok(Self::DashboardDetailCharger),
            "dashboard-detail-thermal" => Ok(Self::DashboardDetailThermal),
            "dashboard-detail-wifi-connected" => Ok(Self::DashboardDetailWifiConnected),
            "dashboard-detail-wifi-connected-long-ip" => {
                Ok(Self::DashboardDetailWifiConnectedLongIp)
            }
            "dashboard-detail-wifi-disabled" => Ok(Self::DashboardDetailWifiDisabled),
            "dashboard-detail-thermal-test-mode" => Ok(Self::DashboardDetailThermalTestMode),
            "dashboard-detail-therm-kill-asserted" => Ok(Self::DashboardDetailThermKillAsserted),
            "dashboard-detail-charger-wait" => Ok(Self::DashboardDetailChargerWait),
            "dashboard-detail-charger-500ma" => Ok(Self::DashboardDetailCharger500mA),
            "dashboard-detail-charger-backup-usb-low-output" => {
                Ok(Self::DashboardDetailChargerBackupUsbLowOutput)
            }
            "dashboard-detail-charger-backup-usb-output-high-latched" => {
                Ok(Self::DashboardDetailChargerBackupUsbOutputHighLatched)
            }
            "dashboard-detail-charger-backup-usb-telemetry-lost-latched" => {
                Ok(Self::DashboardDetailChargerBackupUsbTelemetryLostLatched)
            }
            "dashboard-detail-charger-warm" => Ok(Self::DashboardDetailChargerWarm),
            "dashboard-detail-charger-100ma-dc-derated" => {
                Ok(Self::DashboardDetailCharger100mADcDerated)
            }
            "dashboard-detail-charger-recovery" => Ok(Self::DashboardDetailChargerRecovery),
            "dashboard-detail-charger-full-latched" => Ok(Self::DashboardDetailChargerFullLatched),
            "dashboard-detail-charger-blocked-output-overload" => {
                Ok(Self::DashboardDetailChargerBlockedOutputOverload)
            }
            "dashboard-detail-charger-blocked-output-unknown" => {
                Ok(Self::DashboardDetailChargerBlockedOutputUnknown)
            }
            "dashboard-detail-charger-blocked-no-bms" => {
                Ok(Self::DashboardDetailChargerBlockedNoBms)
            }
            "dashboard-manual-charge-default" => Ok(Self::DashboardManualChargeDefault),
            "dashboard-manual-charge-auto-charging" => Ok(Self::DashboardManualChargeAutoCharging),
            "dashboard-manual-charge-active" => Ok(Self::DashboardManualChargeActive),
            "dashboard-manual-charge-loopback-confirm" => {
                Ok(Self::DashboardManualChargeLoopbackConfirm)
            }
            "dashboard-manual-charge-loopback-confirmed" => {
                Ok(Self::DashboardManualChargeLoopbackConfirmed)
            }
            "dashboard-manual-charge-stop-hold" => Ok(Self::DashboardManualChargeStopHold),
            "dashboard-manual-charge-reset-auto" => Ok(Self::DashboardManualChargeResetAuto),
            "dashboard-manual-charge-blocked" => Ok(Self::DashboardManualChargeBlocked),
            "self-check-out-a-failed" => Ok(Self::SelfCheckOutAFailed),
            "self-check-bms-missing-tps-warn" => Ok(Self::SelfCheckBmsMissingTpsWarn),
            "bq40-offline" => Ok(Self::Bq40Offline),
            "bq40-offline-dialog" => Ok(Self::Bq40OfflineDialog),
            "bq40-discharge-blocked" => Ok(Self::Bq40DischargeBlocked),
            "bq40-emshut-blocked" => Ok(Self::Bq40EmshutBlocked),
            "bq40-emshut-dialog" => Ok(Self::Bq40EmshutDialog),
            "bq40-discharge-dialog" => Ok(Self::Bq40DischargeDialog),
            "bq40-discharge-recovering" => Ok(Self::Bq40DischargeRecovering),
            "bq40-activating" => Ok(Self::Bq40Activating),
            "bq40-result-success" => Ok(Self::Bq40ResultSuccess),
            "bq40-result-no-battery" => Ok(Self::Bq40ResultNoBattery),
            "bq40-result-rom-mode" => Ok(Self::Bq40ResultRomMode),
            "bq40-result-abnormal" => Ok(Self::Bq40ResultAbnormal),
            "bq40-result-not-detected" => Ok(Self::Bq40ResultNotDetected),
            "bq40-issue-dialog" => Ok(Self::Bq40IssueDialog),
            "tps-a-issue-dialog" => Ok(Self::TpsAIssueDialog),
            "tps-test" => Ok(Self::TpsTest),
            "test-audio" => Ok(Self::TestAudio),
            "test-navigation" => Ok(Self::TestNavigation),
            _ => Err(format!(
                "unsupported --scenario value: {raw}\n\n{}",
                help_text()
            )),
        }
    }

    fn as_tag(self) -> &'static str {
        match self {
            ScenarioArg::Default => "default",
            ScenarioArg::FirmwareSafeMode => "firmware-safe-mode",
            ScenarioArg::DisplayDiag => "display-diag",
            ScenarioArg::DashboardAlert => "dashboard-alert",
            ScenarioArg::AlertList => "alert-list",
            ScenarioArg::AlertDetail => "alert-detail",
            ScenarioArg::DashboardRuntimeStandby => "dashboard-runtime-standby",
            ScenarioArg::DashboardRuntimeStandbyTouchZones => {
                "dashboard-runtime-standby-touch-zones"
            }
            ScenarioArg::DashboardRuntimeStandbyWifiDisabled => {
                "dashboard-runtime-standby-wifi-disabled"
            }
            ScenarioArg::DashboardRuntimeStandbyWifiConnecting => {
                "dashboard-runtime-standby-wifi-connecting"
            }
            ScenarioArg::DashboardRuntimeStandbyWifiConnectedWeak => {
                "dashboard-runtime-standby-wifi-connected-weak"
            }
            ScenarioArg::DashboardRuntimeStandbyWifiConnectedMedium => {
                "dashboard-runtime-standby-wifi-connected-medium"
            }
            ScenarioArg::DashboardRuntimeStandbyWifiConnected => {
                "dashboard-runtime-standby-wifi-connected"
            }
            ScenarioArg::DashboardRuntimeStandbyWifiError => "dashboard-runtime-standby-wifi-error",
            ScenarioArg::WifiIconGallery => "wifi-icon-gallery",
            ScenarioArg::DashboardRuntimeAssist => "dashboard-runtime-assist",
            ScenarioArg::DashboardRuntimeBackup => "dashboard-runtime-backup",
            ScenarioArg::DashboardHomeFocusOutput => "dashboard-home-focus-output",
            ScenarioArg::DashboardHomeFocusBatteryFlow => "dashboard-home-focus-battery-flow",
            ScenarioArg::DashboardMenuDashboard => "dashboard-menu-dashboard",
            ScenarioArg::DashboardMenuBeeper => "dashboard-menu-beeper",
            ScenarioArg::DashboardMenuConceptDenseBadge => "dashboard-menu-concept-dense-badge",
            ScenarioArg::DashboardMenuConceptDockBar => "dashboard-menu-concept-dock-bar",
            ScenarioArg::DashboardMenuConceptSplitRail => "dashboard-menu-concept-split-rail",
            ScenarioArg::DashboardMenuConceptSignalPlate => "dashboard-menu-concept-signal-plate",
            ScenarioArg::DashboardAudioActionFocus => "dashboard-audio-action-focus",
            ScenarioArg::DashboardAudioSystemFocus => "dashboard-audio-system-focus",
            ScenarioArg::DashboardAudioSystemOff => "dashboard-audio-system-off",
            ScenarioArg::DashboardAudioTouchZones => "dashboard-audio-touch-zones",
            ScenarioArg::DashboardMenuTransitionMid => "dashboard-menu-transition-mid",
            ScenarioArg::DashboardMenuTransitionEnd => "dashboard-menu-transition-end",
            ScenarioArg::DashboardDetailCells => "dashboard-detail-cells",
            ScenarioArg::DashboardDetailCellsBalanceActive => {
                "dashboard-detail-cells-balance-active"
            }
            ScenarioArg::DashboardDetailCellsBalanceIdle => "dashboard-detail-cells-balance-idle",
            ScenarioArg::DashboardDetailCellsBalanceConfigMismatch => {
                "dashboard-detail-cells-balance-config-mismatch"
            }
            ScenarioArg::DashboardDetailBms => "dashboard-detail-bms",
            ScenarioArg::DashboardDetailBmsChargeBlocked => "dashboard-detail-bms-charge-blocked",
            ScenarioArg::DashboardDetailBmsBalanceMulti => "dashboard-detail-bms-balance-multi",
            ScenarioArg::DashboardDetailBmsNoData => "dashboard-detail-bms-no-data",
            ScenarioArg::DashboardDetailBatteryFlow => "dashboard-detail-battery-flow",
            ScenarioArg::DashboardDetailOutput => "dashboard-detail-output",
            ScenarioArg::DashboardDetailCharger => "dashboard-detail-charger",
            ScenarioArg::DashboardDetailThermal => "dashboard-detail-thermal",
            ScenarioArg::DashboardDetailWifiConnected => "dashboard-detail-wifi-connected",
            ScenarioArg::DashboardDetailWifiConnectedLongIp => {
                "dashboard-detail-wifi-connected-long-ip"
            }
            ScenarioArg::DashboardDetailWifiDisabled => "dashboard-detail-wifi-disabled",
            ScenarioArg::DashboardDetailThermalTestMode => "dashboard-detail-thermal-test-mode",
            ScenarioArg::DashboardDetailThermKillAsserted => "dashboard-detail-therm-kill-asserted",
            ScenarioArg::DashboardDetailChargerWait => "dashboard-detail-charger-wait",
            ScenarioArg::DashboardDetailCharger500mA => "dashboard-detail-charger-500ma",
            ScenarioArg::DashboardDetailChargerBackupUsbLowOutput => {
                "dashboard-detail-charger-backup-usb-low-output"
            }
            ScenarioArg::DashboardDetailChargerBackupUsbOutputHighLatched => {
                "dashboard-detail-charger-backup-usb-output-high-latched"
            }
            ScenarioArg::DashboardDetailChargerBackupUsbTelemetryLostLatched => {
                "dashboard-detail-charger-backup-usb-telemetry-lost-latched"
            }
            ScenarioArg::DashboardDetailChargerWarm => "dashboard-detail-charger-warm",
            ScenarioArg::DashboardDetailCharger100mADcDerated => {
                "dashboard-detail-charger-100ma-dc-derated"
            }
            ScenarioArg::DashboardDetailChargerRecovery => "dashboard-detail-charger-recovery",
            ScenarioArg::DashboardDetailChargerFullLatched => {
                "dashboard-detail-charger-full-latched"
            }
            ScenarioArg::DashboardDetailChargerBlockedOutputOverload => {
                "dashboard-detail-charger-blocked-output-overload"
            }
            ScenarioArg::DashboardDetailChargerBlockedOutputUnknown => {
                "dashboard-detail-charger-blocked-output-unknown"
            }
            ScenarioArg::DashboardDetailChargerBlockedNoBms => {
                "dashboard-detail-charger-blocked-no-bms"
            }
            ScenarioArg::DashboardManualChargeDefault => "dashboard-manual-charge-default",
            ScenarioArg::DashboardManualChargeAutoCharging => {
                "dashboard-manual-charge-auto-charging"
            }
            ScenarioArg::DashboardManualChargeActive => "dashboard-manual-charge-active",
            ScenarioArg::DashboardManualChargeLoopbackConfirm => {
                "dashboard-manual-charge-loopback-confirm"
            }
            ScenarioArg::DashboardManualChargeLoopbackConfirmed => {
                "dashboard-manual-charge-loopback-confirmed"
            }
            ScenarioArg::DashboardManualChargeStopHold => "dashboard-manual-charge-stop-hold",
            ScenarioArg::DashboardManualChargeResetAuto => "dashboard-manual-charge-reset-auto",
            ScenarioArg::DashboardManualChargeBlocked => "dashboard-manual-charge-blocked",
            ScenarioArg::SelfCheckOutAFailed => "self-check-out-a-failed",
            ScenarioArg::SelfCheckBmsMissingTpsWarn => "self-check-bms-missing-tps-warn",
            ScenarioArg::Bq40Offline => "bq40-offline",
            ScenarioArg::Bq40OfflineDialog => "bq40-offline-dialog",
            ScenarioArg::Bq40DischargeBlocked => "bq40-discharge-blocked",
            ScenarioArg::Bq40EmshutBlocked => "bq40-emshut-blocked",
            ScenarioArg::Bq40EmshutDialog => "bq40-emshut-dialog",
            ScenarioArg::Bq40DischargeDialog => "bq40-discharge-dialog",
            ScenarioArg::Bq40DischargeRecovering => "bq40-discharge-recovering",
            ScenarioArg::Bq40Activating => "bq40-activating",
            ScenarioArg::Bq40ResultSuccess => "bq40-result-success",
            ScenarioArg::Bq40ResultNoBattery => "bq40-result-no-battery",
            ScenarioArg::Bq40ResultRomMode => "bq40-result-rom-mode",
            ScenarioArg::Bq40ResultAbnormal => "bq40-result-abnormal",
            ScenarioArg::Bq40ResultNotDetected => "bq40-result-not-detected",
            ScenarioArg::Bq40IssueDialog => "bq40-issue-dialog",
            ScenarioArg::TpsAIssueDialog => "tps-a-issue-dialog",
            ScenarioArg::TpsTest => "tps-test",
            ScenarioArg::TestAudio => "test-audio",
            ScenarioArg::TestNavigation => "test-navigation",
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum AlertListArg {
    Empty,
    Single,
    Mixed,
    Overflow,
}

impl AlertListArg {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw.to_ascii_lowercase().as_str() {
            "empty" => Ok(Self::Empty),
            "single" => Ok(Self::Single),
            "mixed" => Ok(Self::Mixed),
            "overflow" => Ok(Self::Overflow),
            _ => Err(format!(
                "unsupported --alert-list value: {raw} (expected empty|single|mixed|overflow)"
            )),
        }
    }

    fn as_tag(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Single => "single",
            Self::Mixed => "mixed",
            Self::Overflow => "overflow",
        }
    }
}

fn parse_alert_kind(raw: &str) -> Result<AlertPreviewKind, String> {
    match raw.to_ascii_lowercase().as_str() {
        "mains-absent-dc" => Ok(AlertPreviewKind::MainsAbsentDc),
        "high-stress" => Ok(AlertPreviewKind::HighStress),
        "battery-low-no-mains" => Ok(AlertPreviewKind::BatteryLowNoMains),
        "battery-low-with-mains" => Ok(AlertPreviewKind::BatteryLowWithMains),
        "shutdown-protection" => Ok(AlertPreviewKind::ShutdownProtection),
        "io-over-voltage" => Ok(AlertPreviewKind::IoOverVoltage),
        "io-over-current" => Ok(AlertPreviewKind::IoOverCurrent),
        "module-fault" => Ok(AlertPreviewKind::ModuleFault),
        "battery-protection" => Ok(AlertPreviewKind::BatteryProtection),
        _ => Err(format!(
            "unsupported --alert-kind value: {raw} (expected mains-absent-dc|high-stress|battery-low-no-mains|battery-low-with-mains|shutdown-protection|io-over-voltage|io-over-current|module-fault|battery-protection)"
        )),
    }
}

fn alert_kind_tag(kind: AlertPreviewKind) -> &'static str {
    match kind {
        AlertPreviewKind::MainsAbsentDc => "mains-absent-dc",
        AlertPreviewKind::HighStress => "high-stress",
        AlertPreviewKind::BatteryLowNoMains => "battery-low-no-mains",
        AlertPreviewKind::BatteryLowWithMains => "battery-low-with-mains",
        AlertPreviewKind::ShutdownProtection => "shutdown-protection",
        AlertPreviewKind::IoOverVoltage => "io-over-voltage",
        AlertPreviewKind::IoOverCurrent => "io-over-current",
        AlertPreviewKind::ModuleFault => "module-fault",
        AlertPreviewKind::BatteryProtection => "battery-protection",
    }
}

fn parse_alert_severity(raw: &str) -> Result<AlertPreviewSeverity, String> {
    match raw.to_ascii_lowercase().as_str() {
        "warning" => Ok(AlertPreviewSeverity::Warning),
        "critical" => Ok(AlertPreviewSeverity::Critical),
        _ => Err(format!(
            "unsupported --alert-severity value: {raw} (expected warning|critical)"
        )),
    }
}

fn alert_severity_tag(severity: AlertPreviewSeverity) -> &'static str {
    match severity {
        AlertPreviewSeverity::Warning => "warning",
        AlertPreviewSeverity::Critical => "critical",
    }
}

fn parse_alert_sound(raw: &str) -> Result<AlertPreviewSoundState, String> {
    match raw.to_ascii_lowercase().as_str() {
        "audible" => Ok(AlertPreviewSoundState::Audible),
        "muted" => Ok(AlertPreviewSoundState::Muted),
        "system-silent" => Ok(AlertPreviewSoundState::SystemSilent),
        "policy-silent" => Ok(AlertPreviewSoundState::PolicySilent),
        _ => Err(format!(
            "unsupported --alert-sound value: {raw} (expected audible|muted|system-silent|policy-silent)"
        )),
    }
}

fn alert_sound_tag(sound: AlertPreviewSoundState) -> &'static str {
    match sound {
        AlertPreviewSoundState::Audible => "audible",
        AlertPreviewSoundState::Muted => "muted",
        AlertPreviewSoundState::SystemSilent => "system-silent",
        AlertPreviewSoundState::PolicySilent => "policy-silent",
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
    alert_kind: AlertPreviewKind,
    alert_severity: AlertPreviewSeverity,
    alert_sound: AlertPreviewSoundState,
    alert_list: AlertListArg,
    alert_selected: usize,
    alert_top: usize,
    alert_cleared: bool,
    alert_touch_overlay: bool,
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
        let mut alert_kind = AlertPreviewKind::MainsAbsentDc;
        let mut alert_severity: Option<AlertPreviewSeverity> = None;
        let mut alert_sound = AlertPreviewSoundState::Audible;
        let mut alert_list = AlertListArg::Mixed;
        let mut alert_selected: usize = 0;
        let mut alert_top: usize = 0;
        let mut alert_cleared = false;
        let mut alert_touch_overlay = false;

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
                "--alert-kind" => {
                    let value = iter.next().ok_or("missing value for --alert-kind")?;
                    alert_kind = parse_alert_kind(&value)?;
                }
                "--alert-severity" => {
                    let value = iter.next().ok_or("missing value for --alert-severity")?;
                    alert_severity = Some(parse_alert_severity(&value)?);
                }
                "--alert-sound" => {
                    let value = iter.next().ok_or("missing value for --alert-sound")?;
                    alert_sound = parse_alert_sound(&value)?;
                }
                "--alert-list" => {
                    let value = iter.next().ok_or("missing value for --alert-list")?;
                    alert_list = AlertListArg::parse(&value)?;
                }
                "--alert-selected" => {
                    let value = iter.next().ok_or("missing value for --alert-selected")?;
                    alert_selected = value
                        .parse::<usize>()
                        .map_err(|_| format!("invalid --alert-selected value: {value}"))?;
                }
                "--alert-top" => {
                    let value = iter.next().ok_or("missing value for --alert-top")?;
                    alert_top = value
                        .parse::<usize>()
                        .map_err(|_| format!("invalid --alert-top value: {value}"))?;
                }
                "--alert-cleared" => alert_cleared = true,
                "--alert-touch-zones" => alert_touch_overlay = true,
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
        let alert_severity = alert_severity.unwrap_or_else(|| alert_kind.default_severity());

        Ok(Self {
            variant,
            mode,
            focus,
            scenario,
            out_dir,
            frame_no,
            alert_kind,
            alert_severity,
            alert_sound,
            alert_list,
            alert_selected,
            alert_top,
            alert_cleared,
            alert_touch_overlay,
        })
    }

    fn output_tag(&self) -> String {
        match self.scenario {
            ScenarioArg::DashboardAlert => format!(
                "dashboard-alert-{}-{}-phase-{}-frame-{}",
                alert_severity_tag(self.alert_severity),
                alert_sound_tag(self.alert_sound),
                if self.frame_no % 2 == 0 {
                    "white"
                } else {
                    "severity"
                },
                self.frame_no,
            ),
            ScenarioArg::AlertList => format!(
                "alert-list-{}-{}-{}-selected-{}-top-{}{}",
                self.alert_list.as_tag(),
                alert_kind_tag(self.alert_kind),
                alert_sound_tag(self.alert_sound),
                self.alert_selected,
                self.alert_top,
                if self.alert_touch_overlay {
                    "-touch-zones"
                } else {
                    ""
                },
            ),
            ScenarioArg::AlertDetail => format!(
                "alert-detail-{}-{}{}{}{}",
                alert_kind_tag(self.alert_kind),
                if self.alert_cleared {
                    "cleared"
                } else {
                    "active"
                },
                if self.alert_cleared { "" } else { "-" },
                if self.alert_cleared {
                    ""
                } else {
                    alert_sound_tag(self.alert_sound)
                },
                if self.alert_touch_overlay {
                    "-touch-zones"
                } else {
                    ""
                },
            ),
            _ => self.scenario.as_tag().to_string(),
        }
    }
}

fn alert_preview_items(state: AlertListArg, args: &Args) -> Vec<AlertPreviewItem> {
    match state {
        AlertListArg::Empty => Vec::new(),
        AlertListArg::Single => vec![AlertPreviewItem::active(
            args.alert_kind,
            args.alert_severity,
            args.alert_sound,
        )],
        AlertListArg::Mixed => vec![
            AlertPreviewItem::active(
                AlertPreviewKind::MainsAbsentDc,
                AlertPreviewSeverity::Warning,
                AlertPreviewSoundState::Audible,
            ),
            AlertPreviewItem::active(
                AlertPreviewKind::BatteryLowNoMains,
                AlertPreviewSeverity::Warning,
                AlertPreviewSoundState::Muted,
            ),
            AlertPreviewItem::active(
                AlertPreviewKind::IoOverCurrent,
                AlertPreviewSeverity::Critical,
                AlertPreviewSoundState::SystemSilent,
            ),
        ],
        AlertListArg::Overflow => AlertPreviewKind::ALL
            .iter()
            .enumerate()
            .map(|(index, kind)| {
                let sound = match index % 4 {
                    0 => AlertPreviewSoundState::Audible,
                    1 => AlertPreviewSoundState::Muted,
                    2 => AlertPreviewSoundState::SystemSilent,
                    _ => AlertPreviewSoundState::PolicySilent,
                };
                AlertPreviewItem::active(*kind, kind.default_severity(), sound)
            })
            .collect(),
    }
}

fn help_text() -> String {
    [
        "Usage:",
        "  front-panel-preview --variant {A|B|C|D} --focus {idle|up|down|left|right|center|touch} [--mode {off|standby|supplement|backup}] [--scenario <scenario>] --out-dir <ABS_PATH> [--frame-no <n>]",
        "",
        "Alert preview scenarios:",
        "  dashboard-alert --alert-severity {warning|critical} --alert-sound {audible|muted|system-silent|policy-silent}",
        "  alert-list --alert-list {empty|single|mixed|overflow} [--alert-selected <n>] [--alert-top <n>] [--alert-touch-zones]",
        "  alert-detail --alert-kind <alert-id> [--alert-sound <state>] [--alert-cleared] [--alert-touch-zones]",
        "",
        "Common charger scenarios:",
        "  dashboard-detail-charger-wait",
        "  dashboard-detail-charger-500ma",
        "  dashboard-detail-charger-100ma-dc-derated",
        "  dashboard-detail-charger-recovery",
        "  dashboard-detail-charger-full-latched",
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
