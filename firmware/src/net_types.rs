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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UpsStatusSnapshot {
    pub mode: &'static str,
    pub requested_outputs: &'static str,
    pub active_outputs: &'static str,
    pub recoverable_outputs: &'static str,
    pub output_gate_reason: &'static str,
    pub input_vbus_mv: Option<u16>,
    pub input_ibus_ma: Option<i32>,
    pub mains_present: Option<bool>,
    pub vin_vbus_mv: Option<u16>,
    pub vin_iin_ma: Option<i32>,
    pub charger_state: &'static str,
    pub charger_allow_charge: Option<bool>,
    pub charger_ichg_ma: Option<u16>,
    pub charger_ibat_ma: Option<i16>,
    pub charger_vbat_present: Option<bool>,
    pub battery_state: &'static str,
    pub battery_pack_mv: Option<u16>,
    pub battery_current_ma: Option<i16>,
    pub battery_soc_pct: Option<u16>,
    pub battery_no_battery: Option<bool>,
    pub battery_discharge_ready: Option<bool>,
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

impl UpsStatusSnapshot {
    pub const fn empty() -> Self {
        Self {
            mode: "standby",
            requested_outputs: "none",
            active_outputs: "none",
            recoverable_outputs: "none",
            output_gate_reason: "none",
            input_vbus_mv: None,
            input_ibus_ma: None,
            mains_present: None,
            vin_vbus_mv: None,
            vin_iin_ma: None,
            charger_state: "pending",
            charger_allow_charge: None,
            charger_ichg_ma: None,
            charger_ibat_ma: None,
            charger_vbat_present: None,
            battery_state: "pending",
            battery_pack_mv: None,
            battery_current_ma: None,
            battery_soc_pct: None,
            battery_no_battery: None,
            battery_discharge_ready: None,
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
