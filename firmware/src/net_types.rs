use core::fmt::Write as _;

use heapless::String;

pub const API_VERSION: &str = "v1";
pub const SERVICE_ROLE: &str = "ups";
pub const SERVICE_TYPE: &str = "_mains-aegis-ups._tcp.local";
pub const HOSTNAME_PREFIX: &str = "mains-aegis-";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiConnectionState {
    Disabled,
    Idle,
    Connecting,
    Connected,
    Error,
}

impl WifiConnectionState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Idle => "idle",
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::Error => "error",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiErrorKind {
    BadStaticConfig,
    ConnectFailed,
    DhcpTimeout,
    LinkLost,
}

impl WifiErrorKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BadStaticConfig => "bad_static_config",
            Self::ConnectFailed => "connect_failed",
            Self::DhcpTimeout => "dhcp_timeout",
            Self::LinkLost => "link_lost",
        }
    }

    pub const fn ui_hint(self) -> &'static str {
        match self {
            Self::BadStaticConfig => "STATIC CFG",
            Self::ConnectFailed => "JOIN FAIL",
            Self::DhcpTimeout => "DHCP WAIT",
            Self::LinkLost => "LINK LOST",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WifiSnapshot {
    pub state: WifiConnectionState,
    pub ipv4: Option<[u8; 4]>,
    pub gateway: Option<[u8; 4]>,
    pub dns: Option<[u8; 4]>,
    pub is_static: bool,
    pub last_error: Option<WifiErrorKind>,
    pub rssi_dbm: Option<i8>,
    pub mac: Option<[u8; 6]>,
}

impl WifiSnapshot {
    pub const fn disabled() -> Self {
        Self {
            state: WifiConnectionState::Disabled,
            ipv4: None,
            gateway: None,
            dns: None,
            is_static: false,
            last_error: None,
            rssi_dbm: None,
            mac: None,
        }
    }

    pub const fn connecting() -> Self {
        Self {
            state: WifiConnectionState::Connecting,
            ..Self::disabled()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetworkUiSummary {
    pub state: WifiConnectionState,
    pub ipv4: Option<[u8; 4]>,
    pub last_error: Option<WifiErrorKind>,
}

impl NetworkUiSummary {
    pub const fn disabled() -> Self {
        Self {
            state: WifiConnectionState::Disabled,
            ipv4: None,
            last_error: None,
        }
    }

    pub fn from_wifi(snapshot: WifiSnapshot) -> Self {
        Self {
            state: snapshot.state,
            ipv4: snapshot.ipv4,
            last_error: snapshot.last_error,
        }
    }

    pub fn subtitle(self) -> String<32> {
        let mut out = String::<32>::new();
        match self.state {
            WifiConnectionState::Disabled | WifiConnectionState::Idle => {
                let _ = out.push_str("WIFI OFF");
            }
            WifiConnectionState::Connecting => {
                let _ = out.push_str("WIFI CONNECTING");
            }
            WifiConnectionState::Connected => {
                if let Some(ipv4) = self.ipv4 {
                    let _ = write!(out, "IP {}.{}.{}.{}", ipv4[0], ipv4[1], ipv4[2], ipv4[3]);
                } else {
                    let _ = out.push_str("WIFI READY");
                }
            }
            WifiConnectionState::Error => {
                let _ = out.push_str("WIFI RETRY");
                if let Some(kind) = self.last_error {
                    let _ = out.push(' ');
                    let _ = out.push_str(kind.ui_hint());
                }
            }
        }
        out
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WifiSettingsSnapshot {
    pub configured: bool,
    pub ssid: Option<String<32>>,
}

impl WifiSettingsSnapshot {
    pub fn unconfigured() -> Self {
        Self {
            configured: false,
            ssid: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ManualChargeSettingsSnapshot {
    pub target: &'static str,
    pub speed: &'static str,
    pub timer_h: u8,
}

impl ManualChargeSettingsSnapshot {
    pub const fn defaults() -> Self {
        Self {
            target: "full_100",
            speed: "ma_500",
            timer_h: 2,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceSettingsSnapshot {
    pub wifi: WifiSettingsSnapshot,
    pub log_level: &'static str,
    pub manual_charge: ManualChargeSettingsSnapshot,
}

impl DeviceSettingsSnapshot {
    pub fn defaults() -> Self {
        Self {
            wifi: WifiSettingsSnapshot::unconfigured(),
            log_level: "info",
            manual_charge: ManualChargeSettingsSnapshot::defaults(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UpsStatusSnapshot {
    pub mode: &'static str,
    pub requested_outputs: &'static str,
    pub active_outputs: &'static str,
    pub recoverable_outputs: &'static str,
    pub output_gate_reason: &'static str,
    pub input_source: &'static str,
    pub input_vbus_mv: Option<u16>,
    pub input_ibus_ma: Option<i32>,
    pub mains_present: Option<bool>,
    pub vin_vbus_mv: Option<u16>,
    pub vin_iin_ma: Option<i32>,
    pub input_pressure_state: &'static str,
    pub input_pressure_score_pct: Option<u8>,
    pub input_pressure_reason: Option<&'static str>,
    pub input_vin_baseline_mv: Option<u16>,
    pub input_vin_drop_mv: Option<u16>,
    pub charger_state: &'static str,
    pub charger_allow_charge: Option<bool>,
    pub charger_ichg_ma: Option<u16>,
    pub charger_ibat_ma: Option<i16>,
    pub charger_vbat_present: Option<bool>,
    pub charger_policy_target_ichg_ma: Option<u16>,
    pub charger_limit_active: Option<bool>,
    pub charger_limit_reason: Option<&'static str>,
    pub charger_detail_status: Option<&'static str>,
    pub battery_state: &'static str,
    pub battery_pack_mv: Option<u16>,
    pub battery_current_ma: Option<i16>,
    pub battery_soc_pct: Option<u16>,
    pub battery_cell_mv: [Option<u16>; 4],
    pub battery_cell_delta_mv: Option<u16>,
    pub battery_balance_enabled: Option<bool>,
    pub battery_balance_cfg_match: Option<bool>,
    pub battery_balance_active: Option<bool>,
    pub battery_balance_mask: Option<u8>,
    pub battery_balance_cell: Option<u8>,
    pub battery_balance_min_start_delta_mv: Option<u8>,
    pub battery_no_battery: Option<bool>,
    pub battery_discharge_ready: Option<bool>,
    pub battery_charge_fet_on: Option<bool>,
    pub battery_discharge_fet_on: Option<bool>,
    pub battery_precharge_fet_on: Option<bool>,
    pub battery_issue_detail: Option<&'static str>,
    pub battery_recovery_pending: bool,
    pub battery_last_result: Option<&'static str>,
    pub out_a_state: &'static str,
    pub out_a_enabled: Option<bool>,
    pub out_a_vbus_mv: Option<u16>,
    pub out_a_iout_ma: Option<i32>,
    pub out_b_state: &'static str,
    pub out_b_enabled: Option<bool>,
    pub out_b_vbus_mv: Option<u16>,
    pub out_b_iout_ma: Option<i32>,
    pub tmp_a_state: &'static str,
    pub tmp_a_c: Option<i16>,
    pub tmp_b_state: &'static str,
    pub tmp_b_c: Option<i16>,
    pub network: NetworkUiSummary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PowerDiagSnapshot {
    pub input: PowerDiagInputSnapshot,
    pub charger: PowerDiagChargerSnapshot,
    pub policy: PowerDiagPolicySnapshot,
    pub bms: PowerDiagBmsSnapshot,
}

impl PowerDiagSnapshot {
    pub const fn empty() -> Self {
        Self {
            input: PowerDiagInputSnapshot::empty(),
            charger: PowerDiagChargerSnapshot::empty(),
            policy: PowerDiagPolicySnapshot::empty(),
            bms: PowerDiagBmsSnapshot::empty(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PowerDiagInputSnapshot {
    pub source: &'static str,
    pub mains_present: Option<bool>,
    pub input_vbus_mv: Option<u16>,
    pub input_ibus_ma: Option<i32>,
    pub vin_vbus_mv: Option<u16>,
    pub vin_iin_ma: Option<i32>,
    pub pressure_state: &'static str,
    pub pressure_score_pct: Option<u8>,
    pub pressure_reason: Option<&'static str>,
    pub vin_baseline_mv: Option<u16>,
    pub vin_drop_mv: Option<u16>,
    pub usb_pd_attached: bool,
    pub usb_pd_charge_ready: bool,
    pub usb_pd_vbus_present: Option<bool>,
    pub usb_pd_unsafe_source_latched: bool,
    pub usb_pd_contract_kind: Option<&'static str>,
    pub usb_pd_contract_mv: Option<u16>,
    pub usb_pd_contract_ma: Option<u16>,
    pub usb_pd_vac1_mv: Option<u16>,
    pub usb_pd_vsys_mv: Option<u16>,
}

impl PowerDiagInputSnapshot {
    pub const fn empty() -> Self {
        Self {
            source: "unknown",
            mains_present: None,
            input_vbus_mv: None,
            input_ibus_ma: None,
            vin_vbus_mv: None,
            vin_iin_ma: None,
            pressure_state: "inactive",
            pressure_score_pct: None,
            pressure_reason: None,
            vin_baseline_mv: None,
            vin_drop_mv: None,
            usb_pd_attached: false,
            usb_pd_charge_ready: false,
            usb_pd_vbus_present: None,
            usb_pd_unsafe_source_latched: false,
            usb_pd_contract_kind: None,
            usb_pd_contract_mv: None,
            usb_pd_contract_ma: None,
            usb_pd_vac1_mv: None,
            usb_pd_vsys_mv: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PowerDiagChargerSnapshot {
    pub poll_valid: bool,
    pub enabled: bool,
    pub ce_low: bool,
    pub ilim_hiz_brk_low: bool,
    pub allow_charge: bool,
    pub normal_allow_charge: bool,
    pub force_allow_charge: bool,
    pub can_enable: bool,
    pub usb_pd_charge_gate_ready: bool,
    pub input_present: bool,
    pub vbus_present: bool,
    pub ac1_present: bool,
    pub ac2_present: bool,
    pub pg: bool,
    pub vbat_present: bool,
    pub adc_enabled: bool,
    pub adc_done: bool,
    pub adc_ready: bool,
    pub ibus_adc_ma: Option<i16>,
    pub ibat_adc_ma: Option<i16>,
    pub vbus_adc_mv: Option<u16>,
    pub vbat_adc_mv: Option<u16>,
    pub vsys_adc_mv: Option<u16>,
    pub vac1_adc_mv: Option<u16>,
    pub vac2_adc_mv: Option<u16>,
    pub vreg_mv: Option<u16>,
    pub ichg_ma: Option<u16>,
    pub vindpm_mv: Option<u16>,
    pub iindpm_ma: Option<u16>,
    pub vbat_lowv_pct_x10: Option<u16>,
    pub iprechg_ma: Option<u16>,
    pub iterm_ma: Option<u16>,
    pub chg_stat: &'static str,
    pub vbus_stat: &'static str,
    pub ico_stat: &'static str,
    pub treg: bool,
    pub dpdm: bool,
    pub wd: bool,
    pub poorsrc: bool,
    pub vindpm: bool,
    pub iindpm: bool,
    pub ts_cold: bool,
    pub ts_hot: bool,
    pub st0: Option<u8>,
    pub st1: Option<u8>,
    pub st2: Option<u8>,
    pub st3: Option<u8>,
    pub st4: Option<u8>,
    pub fault0: Option<u8>,
    pub fault1: Option<u8>,
    pub ctrl0: Option<u8>,
    pub term_ctrl: Option<u16>,
}

impl PowerDiagChargerSnapshot {
    pub const fn empty() -> Self {
        Self {
            poll_valid: false,
            enabled: false,
            ce_low: false,
            ilim_hiz_brk_low: false,
            allow_charge: false,
            normal_allow_charge: false,
            force_allow_charge: false,
            can_enable: false,
            usb_pd_charge_gate_ready: false,
            input_present: false,
            vbus_present: false,
            ac1_present: false,
            ac2_present: false,
            pg: false,
            vbat_present: false,
            adc_enabled: false,
            adc_done: false,
            adc_ready: false,
            ibus_adc_ma: None,
            ibat_adc_ma: None,
            vbus_adc_mv: None,
            vbat_adc_mv: None,
            vsys_adc_mv: None,
            vac1_adc_mv: None,
            vac2_adc_mv: None,
            vreg_mv: None,
            ichg_ma: None,
            vindpm_mv: None,
            iindpm_ma: None,
            vbat_lowv_pct_x10: None,
            iprechg_ma: None,
            iterm_ma: None,
            chg_stat: "unknown",
            vbus_stat: "unknown",
            ico_stat: "unknown",
            treg: false,
            dpdm: false,
            wd: false,
            poorsrc: false,
            vindpm: false,
            iindpm: false,
            ts_cold: false,
            ts_hot: false,
            st0: None,
            st1: None,
            st2: None,
            st3: None,
            st4: None,
            fault0: None,
            fault1: None,
            ctrl0: None,
            term_ctrl: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PowerDiagPolicySnapshot {
    pub state: Option<&'static str>,
    pub status: &'static str,
    pub notice: &'static str,
    pub input_source: &'static str,
    pub start_reason: Option<&'static str>,
    pub full_reason: Option<&'static str>,
    pub output_block_reason: Option<&'static str>,
    pub recovery_stage: Option<&'static str>,
    pub target_ichg_ma: Option<u16>,
    pub adaptive_cap_ichg_ma: Option<u16>,
    pub effective_target_ichg_ma: Option<u16>,
    pub limit_active: bool,
    pub limit_reason: Option<&'static str>,
    pub detail_status: Option<&'static str>,
    pub pressure_state: &'static str,
    pub pressure_reason: Option<&'static str>,
    pub pressure_score_pct: Option<u8>,
    pub vin_baseline_mv: Option<u16>,
    pub vin_drop_mv: Option<u16>,
    pub output_power_w10: Option<u32>,
    pub charge_latched: bool,
    pub full_latched: bool,
    pub dc_derated: bool,
    pub output_blocked: bool,
    pub manual_active: bool,
    pub manual_stop_inhibit: bool,
}

impl PowerDiagPolicySnapshot {
    pub const fn empty() -> Self {
        Self {
            state: None,
            status: "unknown",
            notice: "unavailable",
            input_source: "unknown",
            start_reason: None,
            full_reason: None,
            output_block_reason: None,
            recovery_stage: None,
            target_ichg_ma: None,
            adaptive_cap_ichg_ma: None,
            effective_target_ichg_ma: None,
            limit_active: false,
            limit_reason: None,
            detail_status: None,
            pressure_state: "inactive",
            pressure_reason: None,
            pressure_score_pct: None,
            vin_baseline_mv: None,
            vin_drop_mv: None,
            output_power_w10: None,
            charge_latched: false,
            full_latched: false,
            dc_derated: false,
            output_blocked: false,
            manual_active: false,
            manual_stop_inhibit: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PowerDiagBmsSnapshot {
    pub addr: Option<u8>,
    pub state: &'static str,
    pub pack_mv: Option<u16>,
    pub current_ma: Option<i16>,
    pub soc_pct: Option<u16>,
    pub cell_min_mv: Option<u16>,
    pub cell_max_mv: Option<u16>,
    pub no_battery: Option<bool>,
    pub discharge_ready: Option<bool>,
    pub charge_ready: Option<bool>,
    pub full: Option<bool>,
    pub issue_detail: Option<&'static str>,
    pub rca_alarm: Option<bool>,
    pub safety_status: Option<u32>,
    pub pf_status: Option<u32>,
    pub manufacturing_status: Option<u32>,
    pub gauging_status: Option<u32>,
    pub op_status: Option<u32>,
    pub xchg: Option<bool>,
    pub chg_fet: Option<bool>,
    pub dsg_fet: Option<bool>,
    pub pchg_fet: Option<bool>,
    pub cuv: Option<bool>,
    pub cuvc: Option<bool>,
    pub cuv_recovery_mv: Option<u16>,
    pub cuv_recov_chg: Option<bool>,
    pub fet_en: Option<bool>,
    pub chg_en: Option<bool>,
    pub dsg_en: Option<bool>,
    pub charging_inhibit: Option<bool>,
    pub charging_suspend: Option<bool>,
    pub charging_hv: Option<bool>,
    pub current_at_eoc_ma: Option<u16>,
}

impl PowerDiagBmsSnapshot {
    pub const fn empty() -> Self {
        Self {
            addr: None,
            state: "pending",
            pack_mv: None,
            current_ma: None,
            soc_pct: None,
            cell_min_mv: None,
            cell_max_mv: None,
            no_battery: None,
            discharge_ready: None,
            charge_ready: None,
            full: None,
            issue_detail: None,
            rca_alarm: None,
            safety_status: None,
            pf_status: None,
            manufacturing_status: None,
            gauging_status: None,
            op_status: None,
            xchg: None,
            chg_fet: None,
            dsg_fet: None,
            pchg_fet: None,
            cuv: None,
            cuvc: None,
            cuv_recovery_mv: None,
            cuv_recov_chg: None,
            fet_en: None,
            chg_en: None,
            dsg_en: None,
            charging_inhibit: None,
            charging_suspend: None,
            charging_hv: None,
            current_at_eoc_ma: None,
        }
    }
}

impl UpsStatusSnapshot {
    pub const fn empty() -> Self {
        Self {
            mode: "standby",
            requested_outputs: "none",
            active_outputs: "none",
            recoverable_outputs: "none",
            output_gate_reason: "none",
            input_source: "unknown",
            input_vbus_mv: None,
            input_ibus_ma: None,
            mains_present: None,
            vin_vbus_mv: None,
            vin_iin_ma: None,
            input_pressure_state: "inactive",
            input_pressure_score_pct: None,
            input_pressure_reason: None,
            input_vin_baseline_mv: None,
            input_vin_drop_mv: None,
            charger_state: "pending",
            charger_allow_charge: None,
            charger_ichg_ma: None,
            charger_ibat_ma: None,
            charger_vbat_present: None,
            charger_policy_target_ichg_ma: None,
            charger_limit_active: None,
            charger_limit_reason: None,
            charger_detail_status: None,
            battery_state: "pending",
            battery_pack_mv: None,
            battery_current_ma: None,
            battery_soc_pct: None,
            battery_cell_mv: [None, None, None, None],
            battery_cell_delta_mv: None,
            battery_balance_enabled: None,
            battery_balance_cfg_match: None,
            battery_balance_active: None,
            battery_balance_mask: None,
            battery_balance_cell: None,
            battery_balance_min_start_delta_mv: None,
            battery_no_battery: None,
            battery_discharge_ready: None,
            battery_charge_fet_on: None,
            battery_discharge_fet_on: None,
            battery_precharge_fet_on: None,
            battery_issue_detail: None,
            battery_recovery_pending: false,
            battery_last_result: None,
            out_a_state: "pending",
            out_a_enabled: None,
            out_a_vbus_mv: None,
            out_a_iout_ma: None,
            out_b_state: "pending",
            out_b_enabled: None,
            out_b_vbus_mv: None,
            out_b_iout_ma: None,
            tmp_a_state: "pending",
            tmp_a_c: None,
            tmp_b_state: "pending",
            tmp_b_c: None,
            network: NetworkUiSummary::disabled(),
        }
    }
}

pub fn format_ipv4(buf: &mut String<16>, ipv4: [u8; 4]) {
    let _ = write!(buf, "{}.{}.{}.{}", ipv4[0], ipv4[1], ipv4[2], ipv4[3]);
}

#[cfg(test)]
mod tests {
    use super::{NetworkUiSummary, WifiConnectionState, WifiErrorKind, WifiSnapshot};

    #[test]
    fn connected_ui_summary_prefers_ip_text() {
        let summary = NetworkUiSummary::from_wifi(WifiSnapshot {
            state: WifiConnectionState::Connected,
            ipv4: Some([192, 168, 31, 15]),
            ..WifiSnapshot::disabled()
        });
        assert_eq!(summary.subtitle().as_str(), "IP 192.168.31.15");
    }

    #[test]
    fn error_ui_summary_includes_short_hint() {
        let summary = NetworkUiSummary::from_wifi(WifiSnapshot {
            state: WifiConnectionState::Error,
            last_error: Some(WifiErrorKind::DhcpTimeout),
            ..WifiSnapshot::disabled()
        });
        assert_eq!(summary.subtitle().as_str(), "WIFI RETRY DHCP WAIT");
    }
}
