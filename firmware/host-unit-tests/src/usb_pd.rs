#[path = "../../src/usb_pd/pd.rs"]
pub mod pd;

#[path = "../../src/usb_pd/contract_tracker.rs"]
pub mod contract_tracker;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContractKind {
    Fixed,
    Pps,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActiveContract {
    pub kind: ContractKind,
    pub object_position: u8,
    pub voltage_mv: u16,
    pub current_ma: u16,
    pub source_max_current_ma: u16,
    pub input_current_limit_ma: Option<u16>,
    pub vindpm_mv: Option<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsbPdRecoveryEvent {
    BootInheritedAttach,
    HardResetInhibited,
    GetSourceCapSent,
    SoftResetSent,
    HardResetSent,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UsbPdPortState {
    pub enabled: bool,
    pub controller_ready: bool,
    pub attached: bool,
    pub charge_ready: bool,
    pub vbus_present: Option<bool>,
    pub contract: Option<ActiveContract>,
    pub input_current_limit_ma: Option<u16>,
    pub vindpm_mv: Option<u16>,
    pub unsafe_source_latched: bool,
    pub recovery_event: Option<UsbPdRecoveryEvent>,
    pub recovery_event_counter: u16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UsbPdPowerDemand {
    pub requested_charge_voltage_mv: u16,
    pub requested_charge_current_ma: u16,
    pub system_load_power_mw: u32,
    pub system_voltage_mv: Option<u16>,
    pub battery_voltage_mv: Option<u16>,
    pub battery_rsoc_pct: Option<u16>,
    pub measured_input_voltage_mv: Option<u16>,
    pub charging_enabled: bool,
}

pub const fn attach_insert_feedback_edge(
    previous: UsbPdPortState,
    current: UsbPdPortState,
) -> bool {
    current.attached && !previous.attached
}

impl UsbPdPowerDemand {
    pub fn required_power_mw(self) -> u32 {
        let charge_power_mw = if self.charging_enabled {
            (self.requested_charge_voltage_mv as u32 * self.requested_charge_current_ma as u32)
                / 1000
        } else {
            0
        };
        charge_power_mw.saturating_add(self.system_load_power_mw)
    }
}

#[path = "../../src/usb_pd/sink_policy.rs"]
pub mod sink_policy;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attach_insert_feedback_only_fires_on_attach_rising_edge() {
        let detached = UsbPdPortState {
            attached: false,
            ..UsbPdPortState::default()
        };
        let attached_no_contract = UsbPdPortState {
            attached: true,
            vbus_present: Some(true),
            ..UsbPdPortState::default()
        };
        let attached_with_contract = UsbPdPortState {
            attached: true,
            contract: Some(ActiveContract {
                kind: ContractKind::Pps,
                object_position: 3,
                voltage_mv: 9_000,
                current_ma: 2_000,
                source_max_current_ma: 3_000,
                input_current_limit_ma: Some(2_000),
                vindpm_mv: Some(8_800),
            }),
            ..attached_no_contract
        };

        assert!(attach_insert_feedback_edge(detached, attached_no_contract));
        assert!(!attach_insert_feedback_edge(
            attached_no_contract,
            attached_with_contract
        ));
        assert!(!attach_insert_feedback_edge(
            attached_with_contract,
            attached_with_contract
        ));
        assert!(!attach_insert_feedback_edge(
            attached_with_contract,
            detached
        ));
        assert!(attach_insert_feedback_edge(
            detached,
            attached_with_contract
        ));
    }
}
