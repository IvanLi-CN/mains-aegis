pub mod contract_tracker;
pub mod fusb302;
pub mod pd;
pub mod sink_policy;

mod recovery;
mod runtime;

use contract_tracker::ContractTracker;
use defmt::{debug, info, warn};
use fusb302::{CcPolarity, Fusb302, IrqSnapshot};
use pd::{ControlMessageType, DataMessageType, Message, MessageHeader, SpecRevision};
use sink_policy::{ContractPlan, LocalCapabilities, SourceOffer};

const PHY_POLL_INTERVAL_MS: u32 = 250;
const PHY_NEGOTIATION_POLL_INTERVAL_MS: u32 = 50;
const ERROR_RETRY_INTERVAL_MS: u32 = 1_000;
const SOURCE_CAPS_WAIT_TIMEOUT_MS: u32 = 400;
const SOURCE_CAPS_REQUERY_DELAY_MS: u32 = 1_000;
const SOURCE_CAPS_REQUERY_RETRY_MS: u32 = 5_000;
const NO_CONTRACT_SOURCE_CAPS_REARM_TIMEOUT_MS: u32 = 3_000;
const NO_CONTRACT_ATTACH_STABILIZE_MS: u32 = 12_000;
const HARD_RESET_SEND_SETTLE_MS: u32 = 120;
const HARD_RESET_WAIT_FOR_SOURCE_CAPS_MS: u32 = 1_200;
const CONTRACT_REQUEST_TIMEOUT_MS: u32 = 1_500;
const PARTIAL_RX_RECOVERY_GRACE_MS: u32 = 250;
const RAW_VBUS_DETACH_DEBOUNCE_POLLS: u8 = 2;
const EFFECTIVE_VBUS_DETACH_DEBOUNCE_POLLS: u8 = 2;
const CC_ABSENT_DETACH_DEBOUNCE_POLLS: u8 = 2;
const BOOT_UNATTACHED_STABLE_MS: u32 = 2_000;
const CHARGER_VBUS_PRESENT_THRESHOLD_MV: u16 = 4_500;
const CONTRACT_CHARGE_READY_DELAY_MS: u32 = 350;
const DEFAULT_5V_CHARGE_READY_DELAY_MS: u32 = 500;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NoContractRecoveryPhase {
    FreshAttach,
    HardResetSent,
    HardResetWaitCaps,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UsbPdPortState {
    pub enabled: bool,
    pub controller_ready: bool,
    pub attached: bool,
    pub charge_ready: bool,
    pub vbus_present: Option<bool>,
    pub polarity: Option<CcPolarity>,
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
    pub battery_voltage_mv: Option<u16>,
    pub measured_input_voltage_mv: Option<u16>,
    pub charging_enabled: bool,
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

pub struct UsbPdSinkManager<I2C> {
    phy: Fusb302<I2C>,
    local_capabilities: LocalCapabilities,
    state: UsbPdPortState,
    initialized: bool,
    next_retry_at_ms: u32,
    last_phy_poll_at_ms: u32,
    last_request_at_ms: u32,
    attached_at_ms: Option<u32>,
    no_contract_phase_started_at_ms: Option<u32>,
    no_contract_recovery_phase: Option<NoContractRecoveryPhase>,
    source_caps_recovery_attempted: bool,
    last_source_caps_requery_at_ms: Option<u32>,
    message_id: u8,
    tx_spec_revision: SpecRevision,
    peer_spec_revision: SpecRevision,
    source_capabilities: Option<pd::SourceCapabilities>,
    last_source_caps_recovery_at_ms: Option<u32>,
    contract_tracker: ContractTracker<ActiveContract>,
    consecutive_cc_absent_polls: u8,
    consecutive_raw_vbus_absent_polls: u8,
    consecutive_effective_vbus_absent_polls: u8,
    unsafe_hard_reset_sent: bool,
    charge_ready_at_ms: Option<u32>,
    partial_rx_started_at_ms: Option<u32>,
    observed_unattached_since_boot: bool,
    boot_unattached_candidate_since_ms: Option<u32>,
}

fn polarity_name(polarity: CcPolarity) -> &'static str {
    match polarity {
        CcPolarity::Cc1 => "cc1",
        CcPolarity::Cc2 => "cc2",
    }
}

fn contract_kind_name(kind: ContractKind) -> &'static str {
    match kind {
        ContractKind::Fixed => "fixed",
        ContractKind::Pps => "pps",
    }
}

fn control_message_name(kind: ControlMessageType) -> &'static str {
    match kind {
        ControlMessageType::GoodCrc => "goodcrc",
        ControlMessageType::Accept => "accept",
        ControlMessageType::Reject => "reject",
        ControlMessageType::PsRdy => "ps_rdy",
        ControlMessageType::GetSourceCap => "get_source_cap",
        ControlMessageType::Wait => "wait",
        ControlMessageType::SoftReset => "soft_reset",
    }
}

fn data_message_name(kind: DataMessageType) -> &'static str {
    match kind {
        DataMessageType::SourceCapabilities => "source_caps",
        DataMessageType::Request => "request",
        DataMessageType::SinkCapabilities => "sink_caps",
    }
}

fn log_filtered_source_capabilities(
    message: &Message,
    source_caps: &pd::SourceCapabilities,
    filtered: &sink_policy::FilteredSourceCapabilities,
) {
    esp_println::println!(
        "usb_pd: source_caps spec_rev_bits={} header=0x{:x} raw_len={} filtered_len={}",
        source_caps.spec_revision.bits(),
        message.header.raw(),
        source_caps.len(),
        filtered.len()
    );
    info!(
        "usb_pd: source_caps spec_rev_bits={=u8} header=0x{=u16:x} raw_len={=usize} filtered_len={=usize}",
        source_caps.spec_revision.bits(),
        message.header.raw(),
        source_caps.len(),
        filtered.len()
    );

    for (index, raw_pdo) in source_caps.iter() {
        info!(
            "usb_pd: source_pdo_raw obj_pos={=u8} raw=0x{=u32:x}",
            (index as u8) + 1,
            raw_pdo.raw()
        );
    }

    for offer in filtered.iter() {
        match offer {
            SourceOffer::Fixed(offer) => info!(
                "usb_pd: source_offer kind=fixed obj_pos={=u8} voltage_mv={=u16} max_current_ma={=u16}",
                offer.object_position,
                offer.voltage_mv,
                offer.max_current_ma
            ),
            SourceOffer::Pps(offer) => info!(
                "usb_pd: source_offer kind=pps obj_pos={=u8} min_voltage_mv={=u16} max_voltage_mv={=u16} max_current_ma={=u16}",
                offer.object_position,
                offer.min_voltage_mv,
                offer.max_voltage_mv,
                offer.max_current_ma
            ),
            SourceOffer::Unsupported => {}
        }
    }
}

fn log_contract_plan(plan: &ContractPlan, demand: UsbPdPowerDemand) {
    info!(
        "usb_pd: select_plan kind={} obj_pos={=u8} voltage_mv={=u16} current_ma={=u16} source_max_current_ma={=u16} vindpm_mv={=?} input_current_limit_ma={=?} charging_enabled={=bool} requested_charge_voltage_mv={=u16} requested_charge_current_ma={=u16} battery_voltage_mv={=?}",
        contract_kind_name(plan.contract.kind),
        plan.contract.object_position,
        plan.contract.voltage_mv,
        plan.contract.current_ma,
        plan.contract.source_max_current_ma,
        plan.contract.vindpm_mv,
        plan.contract.input_current_limit_ma,
        demand.charging_enabled,
        demand.requested_charge_voltage_mv,
        demand.requested_charge_current_ma,
        demand.battery_voltage_mv
    );
}

fn fusb302_error_kind(err: &fusb302::Error) -> &'static str {
    match err {
        fusb302::Error::I2c(e) => match *e {
            esp_hal::i2c::master::Error::FifoExceeded => "i2c_fifo_exceeded",
            esp_hal::i2c::master::Error::AcknowledgeCheckFailed(_) => "i2c_nack",
            esp_hal::i2c::master::Error::Timeout => "i2c_timeout",
            esp_hal::i2c::master::Error::ArbitrationLost => "i2c_arb_lost",
            esp_hal::i2c::master::Error::ExecutionIncomplete => "i2c_exec_incomplete",
            _ => "i2c_other",
        },
        fusb302::Error::Protocol(reason) => reason,
    }
}

fn filtered_source_has_pps(filtered: &sink_policy::FilteredSourceCapabilities) -> bool {
    filtered
        .iter()
        .any(|offer| matches!(offer, SourceOffer::Pps(_)))
}

const fn charger_input_confirms_vbus(measured_input_voltage_mv: Option<u16>) -> bool {
    matches!(
        measured_input_voltage_mv,
        Some(voltage_mv) if voltage_mv >= CHARGER_VBUS_PRESENT_THRESHOLD_MV
    )
}

const fn effective_vbus_present(
    raw_vbus_present: bool,
    measured_input_voltage_mv: Option<u16>,
) -> bool {
    raw_vbus_present || charger_input_confirms_vbus(measured_input_voltage_mv)
}

const fn next_absent_polls(current: u8, signal_present: bool) -> u8 {
    if signal_present {
        0
    } else {
        current.saturating_add(1)
    }
}

const fn raw_vbus_detach_debounce_elapsed(consecutive_raw_vbus_absent_polls: u8) -> bool {
    consecutive_raw_vbus_absent_polls >= RAW_VBUS_DETACH_DEBOUNCE_POLLS
}

const fn effective_vbus_detach_debounce_elapsed(
    consecutive_effective_vbus_absent_polls: u8,
) -> bool {
    consecutive_effective_vbus_absent_polls >= EFFECTIVE_VBUS_DETACH_DEBOUNCE_POLLS
}

const fn cc_absent_detach_debounce_elapsed(consecutive_cc_absent_polls: u8) -> bool {
    consecutive_cc_absent_polls >= CC_ABSENT_DETACH_DEBOUNCE_POLLS
}

const fn retry_fail_should_defer_for_rx(snapshot: IrqSnapshot) -> bool {
    snapshot.retry_failed()
        && (snapshot.rx_fifo_non_empty() || snapshot.tx_sent() || snapshot.gcrc_sent())
}

const fn negotiated_spec_revision(spec_revision: SpecRevision) -> SpecRevision {
    pd::clamp_fusb302_spec_revision(spec_revision)
}

impl<I2C> UsbPdSinkManager<I2C>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    pub fn new(i2c: I2C) -> Self {
        let local_capabilities = LocalCapabilities::from_features();
        let state = UsbPdPortState {
            enabled: local_capabilities.pd_enabled(),
            ..UsbPdPortState::default()
        };
        Self {
            phy: Fusb302::new(i2c),
            local_capabilities,
            state,
            initialized: false,
            next_retry_at_ms: 0,
            last_phy_poll_at_ms: 0,
            last_request_at_ms: 0,
            attached_at_ms: None,
            no_contract_phase_started_at_ms: None,
            no_contract_recovery_phase: None,
            source_caps_recovery_attempted: false,
            last_source_caps_requery_at_ms: None,
            last_source_caps_recovery_at_ms: None,
            message_id: 0,
            tx_spec_revision: pd::FUSB302_MAX_SPEC_REVISION,
            peer_spec_revision: pd::FUSB302_MAX_SPEC_REVISION,
            source_capabilities: None,
            contract_tracker: ContractTracker::default(),
            consecutive_cc_absent_polls: 0,
            consecutive_raw_vbus_absent_polls: 0,
            consecutive_effective_vbus_absent_polls: 0,
            unsafe_hard_reset_sent: false,
            charge_ready_at_ms: None,
            partial_rx_started_at_ms: None,
            observed_unattached_since_boot: false,
            boot_unattached_candidate_since_ms: None,
        }
    }

    pub fn init_best_effort(&mut self) -> UsbPdPortState {
        if !self.state.enabled {
            return self.state;
        }
        match self.phy.init_sink(self.tx_spec_revision) {
            Ok(device_id) => {
                self.initialized = true;
                self.state.controller_ready = true;
                let switches1 = self.phy.read_switches1().ok();
                info!(
                    "usb_pd: fusb302 init ok device_id=0x{=u8:x} switches1={=?} spec_rev_bits={=u8}",
                    device_id,
                    switches1,
                    self.tx_spec_revision.bits()
                );
            }
            Err(err) => {
                self.initialized = false;
                self.state.controller_ready = false;
                warn!(
                    "usb_pd: fusb302 init failed err={}",
                    fusb302_error_kind(&err)
                );
            }
        }
        self.state
    }

    pub fn state(&self) -> UsbPdPortState {
        self.state
    }

    pub fn tick(
        &mut self,
        demand: UsbPdPowerDemand,
        irq_asserted: bool,
        now_ms: u32,
    ) -> UsbPdPortState {
        if !self.state.enabled {
            return self.state;
        }
        if !self.initialized {
            if now_ms < self.next_retry_at_ms {
                return self.state;
            }
            if self.phy.init_sink(self.tx_spec_revision).is_ok() {
                self.initialized = true;
                self.state.controller_ready = true;
                self.next_retry_at_ms = 0;
            } else {
                self.state.controller_ready = false;
                self.next_retry_at_ms = now_ms.wrapping_add(ERROR_RETRY_INTERVAL_MS);
                return self.state;
            }
        }

        if sink_policy::is_input_voltage_unsafe(demand.measured_input_voltage_mv) {
            if !self.state.unsafe_source_latched {
                warn!(
                    "usb_pd: unsafe input latched vin_mv={=?}",
                    demand.measured_input_voltage_mv
                );
            }
            self.state.unsafe_source_latched = true;
            self.clear_contract_tracking();
            self.source_capabilities = None;
            if self.state.attached && !self.unsafe_hard_reset_sent {
                if self.phy.send_hard_reset().is_ok() {
                    self.note_recovery_event(UsbPdRecoveryEvent::HardResetSent);
                    self.unsafe_hard_reset_sent = true;
                }
            }
        }

        let poll_due = irq_asserted
            || now_ms.wrapping_sub(self.last_phy_poll_at_ms) >= self.phy_poll_interval_ms();
        if poll_due {
            self.last_phy_poll_at_ms = now_ms;
            match self.phy.poll_status() {
                Ok(snapshot) => {
                    self.state.controller_ready = true;
                    self.handle_irq_snapshot(snapshot, demand, now_ms);
                }
                Err(err) => {
                    warn!(
                        "usb_pd: fusb302 poll failed err={}",
                        fusb302_error_kind(&err)
                    );
                    self.teardown_controller_state_on_phy_error();
                    self.state.controller_ready = false;
                    self.initialized = false;
                    self.next_retry_at_ms = now_ms.wrapping_add(ERROR_RETRY_INTERVAL_MS);
                    return self.state;
                }
            }
        }

        self.maybe_recover_stalled_contract_request(now_ms);

        if let Some(source_caps) = self.source_capabilities {
            let filtered = sink_policy::filter_source_capabilities(&source_caps);

            if let Some(active_contract) = self.contract_tracker.active_contract() {
                if active_contract.kind == ContractKind::Fixed
                    && !filtered_source_has_pps(&filtered)
                    && !self.contract_tracker.request_in_flight()
                    && self.source_caps_requery_due(now_ms)
                {
                    let retrying = self.last_source_caps_requery_at_ms.is_some();
                    if retrying {
                        esp_println::println!(
                            "usb_pd: retrying source caps probe after fixed fallback retry_after_ms={} tx_spec_rev_bits={} peer_spec_rev_bits={}",
                            now_ms.wrapping_sub(self.last_source_caps_requery_at_ms.unwrap_or(now_ms)),
                            self.tx_spec_revision.bits(),
                            self.peer_spec_revision.bits()
                        );
                        info!(
                            "usb_pd: retrying source caps probe after fixed fallback retry_after_ms={=u32} tx_spec_rev_bits={=u8} peer_spec_rev_bits={=u8}",
                            now_ms.wrapping_sub(self.last_source_caps_requery_at_ms.unwrap_or(now_ms)),
                            self.tx_spec_revision.bits(),
                            self.peer_spec_revision.bits()
                        );
                    } else {
                        esp_println::println!(
                            "usb_pd: probing source caps after fixed contract tx_spec_rev_bits={} peer_spec_rev_bits={}",
                            self.tx_spec_revision.bits(),
                            self.peer_spec_revision.bits()
                        );
                        info!(
                            "usb_pd: probing source caps after fixed contract tx_spec_rev_bits={=u8} peer_spec_rev_bits={=u8}",
                            self.tx_spec_revision.bits(),
                            self.peer_spec_revision.bits()
                        );
                    }
                    match self.send_control_message(
                        ControlMessageType::GetSourceCap,
                        self.tx_spec_revision,
                    ) {
                        Ok(()) => {
                            self.last_source_caps_requery_at_ms = Some(now_ms);
                        }
                        Err(err) => {
                            warn!(
                                "usb_pd: get_source_cap probe failed err={}",
                                fusb302_error_kind(&err)
                            );
                        }
                    }
                }

                if active_contract.kind == ContractKind::Pps
                    && !self.state.unsafe_source_latched
                    && !self.contract_tracker.request_in_flight()
                {
                    if let Some(plan) = sink_policy::select_contract_from_filtered(
                        &self.local_capabilities,
                        &filtered,
                        demand,
                    ) {
                        if sink_policy::should_refresh_pps_contract(
                            active_contract,
                            plan.contract,
                            now_ms,
                            self.last_request_at_ms,
                        ) {
                            if let Err(err) =
                                self.send_contract_request(plan, self.tx_spec_revision, now_ms)
                            {
                                warn!(
                                    "usb_pd: pps refresh request failed err={}",
                                    fusb302_error_kind(&err)
                                );
                            }
                        }
                    }
                }
            } else {
                self.maybe_request_contract_from_cached_source_caps(filtered, demand, now_ms);
            }
        }

        self.maybe_recover_missing_source_caps(demand, now_ms);
        self.maybe_recover_stalled_no_contract_with_cached_caps(now_ms);
        self.maybe_arm_default_5v_charge_ready(now_ms);
        self.update_charge_ready_state(now_ms);

        self.state
    }
}

const fn deadline_elapsed(now_ms: u32, deadline_ms: u32) -> bool {
    now_ms.wrapping_sub(deadline_ms) < 0x8000_0000
}

#[cfg(test)]
mod tests;
