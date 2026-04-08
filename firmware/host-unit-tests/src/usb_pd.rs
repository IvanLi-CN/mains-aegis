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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UsbPdPowerDemand {
    pub requested_charge_voltage_mv: u16,
    pub requested_charge_current_ma: u16,
    pub battery_voltage_mv: Option<u16>,
    pub measured_input_voltage_mv: Option<u16>,
    pub charging_enabled: bool,
}

impl UsbPdPowerDemand {
    pub fn required_power_mw(self) -> u32 {
        if !self.charging_enabled {
            return 0;
        }
        self.requested_charge_voltage_mv as u32 * self.requested_charge_current_ma as u32
    }
}

#[path = "../../src/usb_pd/sink_policy.rs"]
pub mod sink_policy;
