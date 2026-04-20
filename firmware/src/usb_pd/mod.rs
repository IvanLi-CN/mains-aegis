pub mod contract_tracker;
pub mod fusb302;
pub mod pd;
pub mod sink_policy;

use contract_tracker::ContractTracker;
use defmt::{debug, info, warn};
use fusb302::{CcPolarity, Fusb302, IrqSnapshot};
use pd::{ControlMessageType, DataMessageType, Message, MessageHeader, SpecRevision};
use sink_policy::{ContractPlan, LocalCapabilities, SourceOffer};

const PHY_POLL_INTERVAL_MS: u32 = 250;
const ERROR_RETRY_INTERVAL_MS: u32 = 1_000;
const SOURCE_CAPS_WAIT_TIMEOUT_MS: u32 = 800;
const SOURCE_CAPS_REQUERY_DELAY_MS: u32 = 1_000;
const SOURCE_CAPS_REQUERY_RETRY_MS: u32 = 5_000;
const SOURCE_CAPS_RECOVERY_RETRY_MS: u32 = 1_000;
const SOURCE_CAPS_HARD_RECOVERY_TIMEOUT_MS: u32 = 2_500;
const NO_CONTRACT_SOURCE_CAPS_REARM_TIMEOUT_MS: u32 = 3_000;
const NO_CONTRACT_ATTACH_STABILIZE_MS: u32 = 12_000;
const CONTRACT_REQUEST_TIMEOUT_MS: u32 = 1_500;
const RAW_VBUS_DETACH_DEBOUNCE_POLLS: u8 = 2;
const EFFECTIVE_VBUS_DETACH_DEBOUNCE_POLLS: u8 = 2;
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
            self.requested_charge_voltage_mv as u32 * self.requested_charge_current_ma as u32
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
                let _ = self.phy.send_hard_reset();
                self.unsafe_hard_reset_sent = true;
            }
        }

        let poll_due =
            irq_asserted || now_ms.wrapping_sub(self.last_phy_poll_at_ms) >= PHY_POLL_INTERVAL_MS;
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
                        info!(
                            "usb_pd: retrying source caps probe after fixed fallback retry_after_ms={=u32} tx_spec_rev_bits={=u8} peer_spec_rev_bits={=u8}",
                            now_ms.wrapping_sub(self.last_source_caps_requery_at_ms.unwrap_or(now_ms)),
                            self.tx_spec_revision.bits(),
                            self.peer_spec_revision.bits()
                        );
                    } else {
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

    fn maybe_recover_stalled_contract_request(&mut self, now_ms: u32) {
        if !self.contract_tracker.request_in_flight() {
            return;
        }

        if now_ms.wrapping_sub(self.last_request_at_ms) < CONTRACT_REQUEST_TIMEOUT_MS {
            return;
        }

        warn!(
            "usb_pd: contract request timed out accept_wait={=bool} ps_rdy_wait={=bool} active_kind={=?}",
            self.contract_tracker.waiting_for_accept(),
            self.contract_tracker.waiting_for_ps_rdy(),
            self.contract_tracker
                .active_contract()
                .map(|contract| contract_kind_name(contract.kind))
        );
        self.contract_tracker.cancel_pending_request();
        self.last_source_caps_requery_at_ms = None;

        if self.contract_tracker.active_contract().is_none() {
            self.disarm_charge_ready("request_timeout");
        }
    }

    fn maybe_request_contract_from_cached_source_caps(
        &mut self,
        filtered: sink_policy::FilteredSourceCapabilities,
        demand: UsbPdPowerDemand,
        now_ms: u32,
    ) {
        if self.state.unsafe_source_latched
            || self.contract_tracker.request_in_flight()
            || !self.source_caps_requery_due(now_ms)
        {
            return;
        }

        let Some(plan) =
            sink_policy::select_contract_from_filtered(&self.local_capabilities, &filtered, demand)
        else {
            return;
        };

        info!(
            "usb_pd: retrying contract request from cached source caps tx_spec_rev_bits={=u8} peer_spec_rev_bits={=u8}",
            self.tx_spec_revision.bits(),
            self.peer_spec_revision.bits()
        );
        log_contract_plan(&plan, demand);
        if let Err(err) = self.send_contract_request(plan, self.tx_spec_revision, now_ms) {
            warn!(
                "usb_pd: cached source caps request failed err={}",
                fusb302_error_kind(&err)
            );
        }
    }

    fn maybe_recover_missing_source_caps(&mut self, _demand: UsbPdPowerDemand, now_ms: u32) {
        if !self.state.attached
            || self.state.unsafe_source_latched
            || self.source_capabilities.is_some()
            || self.contract_tracker.request_in_flight()
        {
            return;
        }

        let Some(attached_at_ms) = self.attached_at_ms else {
            return;
        };

        let waited_ms = now_ms.wrapping_sub(attached_at_ms);
        if waited_ms < SOURCE_CAPS_WAIT_TIMEOUT_MS {
            return;
        }

        let first_attempt = !self.source_caps_recovery_attempted;
        if !first_attempt {
            let Some(last_recovery_at_ms) = self.last_source_caps_recovery_at_ms else {
                return;
            };
            if waited_ms >= SOURCE_CAPS_HARD_RECOVERY_TIMEOUT_MS {
                warn!(
                    "usb_pd: source caps recovery stalled, rearming phy waited_ms={=u32} retry_after_ms={=u32}",
                    waited_ms,
                    now_ms.wrapping_sub(last_recovery_at_ms)
                );
                self.rearm_after_detach(now_ms, "source_caps_stalled");
                return;
            }
            if now_ms.wrapping_sub(last_recovery_at_ms) < SOURCE_CAPS_RECOVERY_RETRY_MS {
                return;
            }
        }

        if first_attempt {
            info!(
                "usb_pd: no source caps after attach, issuing soft reset waited_ms={=u32} tx_spec_rev_bits={=u8}",
                waited_ms,
                self.tx_spec_revision.bits()
            );
        } else {
            info!(
                "usb_pd: retrying source caps recovery after default_5v fallback retry_after_ms={=u32} tx_spec_rev_bits={=u8}",
                now_ms.wrapping_sub(self.last_source_caps_recovery_at_ms.unwrap_or(now_ms)),
                self.tx_spec_revision.bits()
            );
        }
        if let Err(err) =
            self.send_control_message(ControlMessageType::SoftReset, self.tx_spec_revision)
        {
            warn!(
                "usb_pd: source caps soft reset failed err={}",
                fusb302_error_kind(&err)
            );
            return;
        }
        self.source_caps_recovery_attempted = true;
        self.last_source_caps_recovery_at_ms = Some(now_ms);
    }

    fn no_contract_attach_stabilizing(&self, now_ms: u32) -> bool {
        if !self.state.attached || self.state.contract.is_some() {
            return false;
        }

        let Some(attached_at_ms) = self.attached_at_ms else {
            return false;
        };

        now_ms.wrapping_sub(attached_at_ms) < NO_CONTRACT_ATTACH_STABILIZE_MS
    }

    fn maybe_recover_stalled_no_contract_with_cached_caps(&mut self, now_ms: u32) {
        if !self.state.attached
            || self.state.unsafe_source_latched
            || self.state.contract.is_some()
            || self.source_capabilities.is_none()
            || self.contract_tracker.request_in_flight()
            || !matches!(self.state.vbus_present, Some(true))
        {
            return;
        }

        let Some(attached_at_ms) = self.attached_at_ms else {
            return;
        };

        let waited_ms = now_ms.wrapping_sub(attached_at_ms);
        if waited_ms < NO_CONTRACT_SOURCE_CAPS_REARM_TIMEOUT_MS {
            return;
        }

        warn!(
            "usb_pd: cached source caps stalled without contract, rearming phy waited_ms={=u32} tx_spec_rev_bits={=u8} peer_spec_rev_bits={=u8}",
            waited_ms,
            self.tx_spec_revision.bits(),
            self.peer_spec_revision.bits()
        );
        self.rearm_after_detach(now_ms, "cached_caps_no_contract");
    }

    fn source_caps_requery_due(&self, now_ms: u32) -> bool {
        if let Some(last_requery_at_ms) = self.last_source_caps_requery_at_ms {
            now_ms.wrapping_sub(last_requery_at_ms) >= SOURCE_CAPS_REQUERY_RETRY_MS
        } else {
            now_ms.wrapping_sub(self.last_request_at_ms) >= SOURCE_CAPS_REQUERY_DELAY_MS
        }
    }

    fn maybe_arm_default_5v_charge_ready(&mut self, now_ms: u32) {
        if !self.state.attached
            || self.state.unsafe_source_latched
            || self.state.contract.is_some()
            || self.source_capabilities.is_some()
            || self.contract_tracker.request_in_flight()
            || !self.source_caps_recovery_attempted
            || self.state.charge_ready
            || self.charge_ready_at_ms.is_some()
        {
            return;
        }

        self.apply_default_5v_input_limits(None, "default_5v");
        self.arm_default_5v_charge_ready(now_ms, "default_5v");
    }

    fn handle_irq_snapshot(
        &mut self,
        snapshot: IrqSnapshot,
        demand: UsbPdPowerDemand,
        now_ms: u32,
    ) {
        let detected_attach_polarity = snapshot.attached_sink_polarity();
        let raw_vbus_present = snapshot.vbus_present();
        let measured_input_voltage_mv = demand.measured_input_voltage_mv;
        let vbus_present = effective_vbus_present(raw_vbus_present, measured_input_voltage_mv);
        self.state.vbus_present = Some(vbus_present);
        self.consecutive_cc_absent_polls = if self.state.attached {
            0
        } else {
            next_absent_polls(
                self.consecutive_cc_absent_polls,
                detected_attach_polarity.is_some(),
            )
        };
        self.consecutive_raw_vbus_absent_polls =
            next_absent_polls(self.consecutive_raw_vbus_absent_polls, raw_vbus_present);
        self.consecutive_effective_vbus_absent_polls =
            next_absent_polls(self.consecutive_effective_vbus_absent_polls, vbus_present);
        let attach_recovery_in_progress = self.no_contract_attach_stabilizing(now_ms);

        if attach_recovery_in_progress {
            self.consecutive_raw_vbus_absent_polls = 0;
            self.consecutive_effective_vbus_absent_polls = 0;
        }

        if self.state.attached && !attach_recovery_in_progress && !raw_vbus_present && !vbus_present
        {
            if !raw_vbus_detach_debounce_elapsed(self.consecutive_raw_vbus_absent_polls) {
                debug!(
                    "usb_pd: raw vbus detach debounce waiting effective_vbus_ok={=bool} vin_mv={=?} absent_polls={=u8}",
                    vbus_present,
                    measured_input_voltage_mv,
                    self.consecutive_raw_vbus_absent_polls
                );
                return;
            }
            info!(
                "usb_pd: detached raw_vbus_lost effective_vbus_ok={=bool} vin_mv={=?}",
                vbus_present, measured_input_voltage_mv
            );
            self.rearm_after_detach(now_ms, "raw_vbus_lost");
            return;
        }

        if self.state.attached && !attach_recovery_in_progress && !vbus_present {
            if !effective_vbus_detach_debounce_elapsed(self.consecutive_effective_vbus_absent_polls)
            {
                debug!(
                    "usb_pd: effective vbus detach debounce waiting raw_vbus_ok={=bool} vin_mv={=?} absent_polls={=u8}",
                    raw_vbus_present,
                    measured_input_voltage_mv,
                    self.consecutive_effective_vbus_absent_polls
                );
                return;
            }
            info!("usb_pd: detached effective_vbus_lost");
            self.rearm_after_detach(now_ms, "effective_vbus_lost");
            return;
        }

        if !self.state.attached {
            let Some(polarity) = detected_attach_polarity else {
                return;
            };
            if !vbus_present {
                return;
            };

            self.state.attached = true;
            self.state.polarity = Some(polarity);
            self.message_id = 0;
            self.attached_at_ms = Some(now_ms);
            self.source_caps_recovery_attempted = false;
            self.last_source_caps_requery_at_ms = None;
            self.last_source_caps_recovery_at_ms = None;
            self.consecutive_cc_absent_polls = 0;
            self.consecutive_raw_vbus_absent_polls = 0;
            self.consecutive_effective_vbus_absent_polls = 0;
            self.clear_contract_tracking();
            self.source_capabilities = None;
            self.disarm_charge_ready("attach");
            if let Err(err) = self.phy.init_sink(self.tx_spec_revision) {
                warn!(
                    "usb_pd: attach reinit failed err={}",
                    fusb302_error_kind(&err)
                );
                self.initialized = false;
                self.state.controller_ready = false;
                return;
            }
            if let Err(err) = self.phy.enable_pd_receive(polarity, self.tx_spec_revision) {
                warn!("usb_pd: enable rx failed err={}", fusb302_error_kind(&err));
                self.initialized = false;
                self.state.controller_ready = false;
                return;
            }
            self.initialized = true;
            self.state.controller_ready = true;
            self.next_retry_at_ms = 0;
            let switches1 = self.phy.read_switches1().ok();
            info!(
                "usb_pd: attached polarity={} switches1={=?} spec_rev_bits={=u8} action=full_reinit",
                polarity_name(polarity),
                switches1,
                self.tx_spec_revision.bits()
            );
        } else if self.state.polarity.is_none() {
            if let Some(polarity) = detected_attach_polarity {
                self.state.polarity = Some(polarity);
            }
        }

        if self.state.attached
            && self.source_capabilities.is_none()
            && (snapshot.interrupta != 0
                || snapshot.interruptb != 0
                || snapshot.interrupt != 0
                || snapshot.rx_fifo_non_empty())
        {
            info!(
                "usb_pd: irq st0a=0x{=u8:x} st1a=0x{=u8:x} inta=0x{=u8:x} intb=0x{=u8:x} st0=0x{=u8:x} st1=0x{=u8:x} int=0x{=u8:x} rx_non_empty={=bool} rx_ready={=bool} tx_sent={=bool} gcrc_sent={=bool}",
                snapshot.status0a,
                snapshot.status1a,
                snapshot.interrupta,
                snapshot.interruptb,
                snapshot.status0,
                snapshot.status1,
                snapshot.interrupt,
                snapshot.rx_fifo_non_empty(),
                snapshot.rx_message_ready(),
                snapshot.tx_sent(),
                snapshot.gcrc_sent()
            );
        }

        if snapshot.hard_reset_received() || snapshot.retry_failed() {
            warn!(
                "usb_pd: reset/retry event hard={=bool} retry_fail={=bool}",
                snapshot.hard_reset_received(),
                snapshot.retry_failed()
            );
            if self.state.contract.is_none() {
                let reason = if snapshot.hard_reset_received() {
                    "hard_reset_no_contract"
                } else {
                    "retry_fail_no_contract"
                };
                self.rearm_after_detach(now_ms, reason);
                return;
            }
            self.reset_contract_state(false);
            self.attached_at_ms = Some(now_ms);
            if let Some(polarity) = self.state.polarity {
                let _ = self.phy.enable_pd_receive(polarity, self.tx_spec_revision);
            }
        }

        if snapshot.soft_reset_received() && !snapshot.rx_message_ready() {
            warn!("usb_pd: source requested soft reset without fifo payload");
            self.handle_peer_soft_reset(self.peer_spec_revision, now_ms);
        }

        if snapshot.rx_fifo_non_empty() || snapshot.gcrc_sent() || snapshot.tx_sent() {
            let mut trust_snapshot_fifo = snapshot.rx_fifo_non_empty();
            loop {
                let read_result = if trust_snapshot_fifo {
                    trust_snapshot_fifo = false;
                    self.phy.read_message_unchecked()
                } else {
                    self.phy.read_message()
                };

                match read_result {
                    Ok(Some(message)) => self.handle_message(message, demand, now_ms),
                    Ok(None) => break,
                    Err(err) => {
                        warn!(
                            "usb_pd: read message failed err={}",
                            fusb302_error_kind(&err)
                        );
                        let _ = self.phy.flush_rx();
                        break;
                    }
                }

                match self.phy.rx_fifo_non_empty() {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(err) => {
                        warn!(
                            "usb_pd: read fifo status failed err={}",
                            fusb302_error_kind(&err)
                        );
                        break;
                    }
                }
            }
        }
    }

    fn handle_message(&mut self, message: Message, demand: UsbPdPowerDemand, now_ms: u32) {
        if let Some(kind) = message.header.data_message_type() {
            debug!(
                "usb_pd: rx data kind={} spec_rev_bits={=u8} msg_id={=u8} obj_count={=usize}",
                data_message_name(kind),
                message.header.spec_revision().bits(),
                message.header.message_id(),
                message.header.object_count()
            );
            match kind {
                DataMessageType::SourceCapabilities => {
                    if self.state.unsafe_source_latched {
                        debug!("usb_pd: ignoring source caps because unsafe source is latched");
                        return;
                    }
                    self.disarm_charge_ready("source_caps");
                    let Some(source_caps) = pd::SourceCapabilities::from_message(&message) else {
                        return;
                    };
                    let filtered = sink_policy::filter_source_capabilities(&source_caps);
                    log_filtered_source_capabilities(&message, &source_caps, &filtered);
                    let pending_contract_supported = self
                        .contract_tracker
                        .pending_contract()
                        .is_some_and(|pending_contract| {
                            sink_policy::filtered_source_supports_contract(
                                &filtered,
                                pending_contract,
                            )
                        });
                    self.contract_tracker
                        .refresh_source_capabilities(pending_contract_supported);
                    if let Some(active_contract) = self.contract_tracker.active_contract() {
                        if !sink_policy::filtered_source_supports_contract(
                            &filtered,
                            active_contract,
                        ) && !pending_contract_supported
                        {
                            warn!("usb_pd: active contract no longer advertised, clearing state");
                            self.clear_contract_tracking();
                        }
                    }
                    self.observe_peer_spec_revision(source_caps.spec_revision, "source_caps");
                    self.source_capabilities = Some(source_caps);
                    self.source_caps_recovery_attempted = false;
                    if filtered_source_has_pps(&filtered) {
                        self.last_source_caps_requery_at_ms = None;
                    }
                    if self.contract_tracker.request_in_flight() {
                        debug!("usb_pd: preserving in-flight contract across source caps refresh");
                        return;
                    }
                    if let Some(plan) = sink_policy::select_contract_from_filtered(
                        &self.local_capabilities,
                        &filtered,
                        demand,
                    ) {
                        log_contract_plan(&plan, demand);
                        if let Err(err) =
                            self.send_contract_request(plan, self.tx_spec_revision, now_ms)
                        {
                            warn!(
                                "usb_pd: request send failed err={}",
                                fusb302_error_kind(&err)
                            );
                        }
                    } else {
                        warn!("usb_pd: no safe PD/PPS contract available");
                        if self.contract_tracker.active_contract().is_none()
                            && !self.contract_tracker.request_in_flight()
                        {
                            self.apply_default_5v_input_limits(Some(&filtered), "no_safe_contract");
                            self.arm_default_5v_charge_ready(now_ms, "no_safe_contract");
                        }
                    }
                }
                DataMessageType::Request | DataMessageType::SinkCapabilities => {}
            }
            return;
        }

        if let Some(kind) = message.header.control_message_type() {
            self.observe_peer_spec_revision(message.header.spec_revision(), "ctrl");
            info!(
                "usb_pd: rx ctrl kind={} peer_spec_rev_bits={=u8} tx_spec_rev_bits={=u8} msg_id={=u8} header=0x{=u16:x}",
                control_message_name(kind),
                message.header.spec_revision().bits(),
                self.tx_spec_revision.bits(),
                message.header.message_id(),
                message.header.raw()
            );
        }

        match message.header.control_message_type() {
            Some(ControlMessageType::Accept) if self.contract_tracker.mark_accept_received() => {
                info!("usb_pd: contract accepted");
            }
            Some(ControlMessageType::PsRdy) => {
                if let Some(contract) = self.contract_tracker.commit_pending_contract() {
                    let same_contract = self.state.contract == Some(contract);
                    self.state.contract = Some(contract);
                    self.state.input_current_limit_ma = contract.input_current_limit_ma;
                    self.state.vindpm_mv = contract.vindpm_mv;
                    info!(
                        "usb_pd: contract active kind={} voltage_mv={=u16} current_ma={=u16}",
                        contract_kind_name(contract.kind),
                        contract.voltage_mv,
                        contract.current_ma
                    );
                    if !same_contract || !self.state.charge_ready {
                        self.arm_charge_ready_after(
                            now_ms,
                            CONTRACT_CHARGE_READY_DELAY_MS,
                            "contract_ps_rdy",
                        );
                    }
                }
            }
            Some(ControlMessageType::Reject) | Some(ControlMessageType::Wait) => {
                warn!("usb_pd: source deferred request");
                self.contract_tracker.cancel_pending_request();
                if self.state.contract.is_some() {
                    self.arm_charge_ready_after(
                        now_ms,
                        CONTRACT_CHARGE_READY_DELAY_MS,
                        "request_deferred",
                    );
                } else {
                    let filtered = self
                        .source_capabilities
                        .map(|source_caps| sink_policy::filter_source_capabilities(&source_caps));
                    self.apply_default_5v_input_limits(
                        filtered.as_ref(),
                        "request_deferred_default_5v",
                    );
                    self.arm_default_5v_charge_ready(now_ms, "request_deferred_default_5v");
                }
            }
            Some(ControlMessageType::SoftReset) => {
                self.handle_peer_soft_reset(message.header.spec_revision(), now_ms);
            }
            _ => {}
        }
    }

    fn send_contract_request(
        &mut self,
        plan: ContractPlan,
        spec_revision: SpecRevision,
        now_ms: u32,
    ) -> Result<(), fusb302::Error> {
        let header = MessageHeader::for_data(
            DataMessageType::Request,
            1,
            self.message_id,
            spec_revision,
            false,
            false,
        );
        let mut payload = [0u32; pd::MAX_DATA_OBJECTS];
        payload[0] = plan.request.raw();
        let message = Message::new(header, payload);
        info!(
            "usb_pd: tx data kind=request spec_rev_bits={=u8} peer_spec_rev_bits={=u8} msg_id={=u8} obj_pos={=u8} voltage_mv={=u16} current_ma={=u16}",
            spec_revision.bits(),
            self.peer_spec_revision.bits(),
            self.message_id,
            plan.contract.object_position,
            plan.contract.voltage_mv,
            plan.contract.current_ma
        );
        if self.state.contract != Some(plan.contract) {
            self.disarm_charge_ready("request_transition");
        }
        self.phy.send_message(&message)?;
        self.contract_tracker.begin_request(plan.contract);
        self.last_request_at_ms = now_ms;
        self.message_id = (self.message_id + 1) & 0x07;
        Ok(())
    }

    fn send_control_message(
        &mut self,
        kind: ControlMessageType,
        spec_revision: SpecRevision,
    ) -> Result<(), fusb302::Error> {
        let header = MessageHeader::for_control(kind, self.message_id, spec_revision, false, false);
        let message = Message::new(header, [0u32; pd::MAX_DATA_OBJECTS]);
        info!(
            "usb_pd: tx ctrl kind={} spec_rev_bits={=u8} peer_spec_rev_bits={=u8} msg_id={=u8}",
            control_message_name(kind),
            spec_revision.bits(),
            self.peer_spec_revision.bits(),
            self.message_id
        );
        self.phy.send_message(&message)?;
        self.message_id = (self.message_id + 1) & 0x07;
        Ok(())
    }

    fn clear_contract_tracking(&mut self) {
        self.contract_tracker.clear_all();
        self.state.contract = None;
        self.state.input_current_limit_ma = None;
        self.state.vindpm_mv = None;
    }

    fn rearm_after_detach(&mut self, now_ms: u32, reason: &'static str) {
        self.reset_contract_state(true);
        match self.phy.init_sink(self.tx_spec_revision) {
            Ok(_) => {
                self.initialized = true;
                self.state.controller_ready = true;
                self.next_retry_at_ms = 0;
                info!(
                    "usb_pd: phy rearmed after detach reason={} tx_spec_rev_bits={=u8}",
                    reason,
                    self.tx_spec_revision.bits()
                );
            }
            Err(err) => {
                warn!(
                    "usb_pd: phy rearm after detach failed reason={} err={}",
                    reason,
                    fusb302_error_kind(&err)
                );
                self.initialized = false;
                self.state.controller_ready = false;
                self.next_retry_at_ms = now_ms.wrapping_add(ERROR_RETRY_INTERVAL_MS);
            }
        }
    }

    fn apply_default_5v_input_limits(
        &mut self,
        filtered: Option<&sink_policy::FilteredSourceCapabilities>,
        reason: &'static str,
    ) {
        let limits = sink_policy::default_5v_input_limits(filtered);
        if self.state.vindpm_mv != Some(limits.vindpm_mv)
            || self.state.input_current_limit_ma != Some(limits.input_current_limit_ma)
        {
            info!(
                "usb_pd: default_5v_limits reason={} vindpm_mv={=u16} input_current_limit_ma={=u16}",
                reason,
                limits.vindpm_mv,
                limits.input_current_limit_ma
            );
        }
        self.state.vindpm_mv = Some(limits.vindpm_mv);
        self.state.input_current_limit_ma = Some(limits.input_current_limit_ma);
    }

    fn teardown_controller_state_on_phy_error(&mut self) {
        self.clear_contract_tracking();
        self.disarm_charge_ready("controller_error");
        self.source_capabilities = None;
        self.message_id = 0;
        self.consecutive_cc_absent_polls = 0;
        self.consecutive_raw_vbus_absent_polls = 0;
        self.consecutive_effective_vbus_absent_polls = 0;
        self.attached_at_ms = None;
        self.source_caps_recovery_attempted = false;
        self.last_source_caps_requery_at_ms = None;
        self.last_source_caps_recovery_at_ms = None;
        self.tx_spec_revision = pd::FUSB302_MAX_SPEC_REVISION;
        self.peer_spec_revision = pd::FUSB302_MAX_SPEC_REVISION;
        self.state.attached = false;
        self.state.vbus_present = None;
        self.state.polarity = None;
        self.unsafe_hard_reset_sent = false;
    }

    fn arm_charge_ready_after(&mut self, now_ms: u32, delay_ms: u32, reason: &'static str) {
        self.state.charge_ready = false;
        self.charge_ready_at_ms = Some(now_ms.wrapping_add(delay_ms));
        info!(
            "usb_pd: charge gate arm reason={} delay_ms={=u32}",
            reason, delay_ms
        );
    }

    fn disarm_charge_ready(&mut self, reason: &'static str) {
        if self.state.charge_ready || self.charge_ready_at_ms.is_some() {
            info!("usb_pd: charge gate hold reason={}", reason);
        }
        self.state.charge_ready = false;
        self.charge_ready_at_ms = None;
    }

    fn arm_default_5v_charge_ready(&mut self, now_ms: u32, reason: &'static str) {
        self.arm_charge_ready_after(now_ms, DEFAULT_5V_CHARGE_READY_DELAY_MS, reason);
    }

    fn update_charge_ready_state(&mut self, now_ms: u32) {
        let Some(ready_at_ms) = self.charge_ready_at_ms else {
            return;
        };
        if !deadline_elapsed(now_ms, ready_at_ms) {
            return;
        }

        self.charge_ready_at_ms = None;
        self.state.charge_ready = true;
        info!(
            "usb_pd: charge gate ready kind={} contract_mv={=?}",
            self.state
                .contract
                .map(|contract| contract_kind_name(contract.kind))
                .unwrap_or("default_5v"),
            self.state.contract.map(|contract| contract.voltage_mv)
        );
    }

    fn observe_peer_spec_revision(&mut self, spec_revision: SpecRevision, context: &'static str) {
        let spec_revision = negotiated_spec_revision(spec_revision);
        if self.peer_spec_revision != spec_revision {
            info!(
                "usb_pd: peer_spec_update context={} peer_spec_rev_bits={=u8} tx_spec_rev_bits={=u8}",
                context,
                spec_revision.bits(),
                self.tx_spec_revision.bits()
            );
        }
        self.peer_spec_revision = spec_revision;
        self.tx_spec_revision = spec_revision;
    }

    fn handle_peer_soft_reset(&mut self, spec_revision: SpecRevision, now_ms: u32) {
        warn!("usb_pd: source requested soft reset");
        self.observe_peer_spec_revision(spec_revision, "soft_reset");
        self.reset_contract_state(false);
        self.attached_at_ms = Some(now_ms);
        if let Err(err) =
            self.send_control_message(ControlMessageType::Accept, self.tx_spec_revision)
        {
            warn!(
                "usb_pd: soft reset accept failed err={}",
                fusb302_error_kind(&err)
            );
        }
    }

    fn reset_contract_state(&mut self, detach: bool) {
        self.clear_contract_tracking();
        self.disarm_charge_ready(if detach { "detach" } else { "reset" });
        self.source_capabilities = None;
        self.message_id = 0;
        self.consecutive_effective_vbus_absent_polls = 0;
        self.consecutive_raw_vbus_absent_polls = 0;
        self.attached_at_ms = None;
        self.source_caps_recovery_attempted = false;
        self.last_source_caps_requery_at_ms = None;
        self.last_source_caps_recovery_at_ms = None;
        if detach {
            self.tx_spec_revision = pd::FUSB302_MAX_SPEC_REVISION;
            self.peer_spec_revision = pd::FUSB302_MAX_SPEC_REVISION;
            self.state.attached = false;
            self.state.vbus_present = Some(false);
            self.state.polarity = None;
            self.state.unsafe_source_latched = false;
            self.unsafe_hard_reset_sent = false;
        }
    }
}

const fn deadline_elapsed(now_ms: u32, deadline_ms: u32) -> bool {
    now_ms.wrapping_sub(deadline_ms) < 0x8000_0000
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoopI2c;
    struct LenientI2c;

    impl embedded_hal::i2c::ErrorType for NoopI2c {
        type Error = esp_hal::i2c::master::Error;
    }

    impl embedded_hal::i2c::I2c for NoopI2c {
        fn transaction(
            &mut self,
            _address: u8,
            _operations: &mut [embedded_hal::i2c::Operation<'_>],
        ) -> Result<(), Self::Error> {
            panic!("unexpected I2C transaction in usb_pd unit test");
        }
    }

    impl embedded_hal::i2c::ErrorType for LenientI2c {
        type Error = esp_hal::i2c::master::Error;
    }

    impl embedded_hal::i2c::I2c for LenientI2c {
        fn transaction(
            &mut self,
            _address: u8,
            operations: &mut [embedded_hal::i2c::Operation<'_>],
        ) -> Result<(), Self::Error> {
            for operation in operations {
                if let embedded_hal::i2c::Operation::Read(buffer) = operation {
                    buffer.fill(0);
                }
            }
            Ok(())
        }
    }

    const fn fixed_pdo_raw(voltage_mv: u16, current_ma: u16) -> u32 {
        ((voltage_mv as u32 / pd::FIXED_VOLTAGE_STEP_MV as u32) << 10)
            | (current_ma as u32 / pd::FIXED_CURRENT_STEP_MA as u32)
    }

    const fn irq_snapshot_with_cc_and_vbus(status1a: u8, vbus_ok: bool) -> IrqSnapshot {
        IrqSnapshot {
            status0a: 0,
            status1a,
            interrupta: 0,
            interruptb: 0,
            status0: if vbus_ok {
                fusb302::status0::VBUS_OK
            } else {
                0
            },
            status1: 0,
            interrupt: 0,
        }
    }

    const fn irq_snapshot_with_retry_fail(status1a: u8, vbus_ok: bool) -> IrqSnapshot {
        IrqSnapshot {
            status0a: 0,
            status1a,
            interrupta: fusb302::interrupta::RETRY_FAIL,
            interruptb: 0,
            status0: if vbus_ok {
                fusb302::status0::VBUS_OK
            } else {
                0
            },
            status1: 0,
            interrupt: 0,
        }
    }

    fn source_caps_message(spec_revision: SpecRevision, payload_words: &[u32]) -> Message {
        let mut payload = [0u32; pd::MAX_DATA_OBJECTS];
        let mut count = 0usize;
        while count < payload_words.len() {
            payload[count] = payload_words[count];
            count += 1;
        }

        Message::new(
            MessageHeader::for_data(
                DataMessageType::SourceCapabilities,
                count,
                0,
                spec_revision,
                true,
                false,
            ),
            payload,
        )
    }

    fn control_message(kind: ControlMessageType, spec_revision: SpecRevision) -> Message {
        Message::new(
            MessageHeader::for_control(kind, 0, spec_revision, true, false),
            [0u32; pd::MAX_DATA_OBJECTS],
        )
    }

    #[test]
    fn charger_input_voltage_can_confirm_vbus_presence() {
        assert!(charger_input_confirms_vbus(Some(4_500)));
        assert!(effective_vbus_present(false, Some(5_200)));
        assert!(!effective_vbus_present(false, Some(4_499)));
        assert!(!effective_vbus_present(false, None));
    }

    #[test]
    fn detach_requires_consecutive_absent_polls() {
        let first = next_absent_polls(0, false);
        assert_eq!(first, 1);
        assert!(!raw_vbus_detach_debounce_elapsed(first));

        let second = next_absent_polls(first, false);
        assert_eq!(second, 2);
        assert!(raw_vbus_detach_debounce_elapsed(second));

        assert_eq!(next_absent_polls(second, true), 0);
    }

    #[test]
    fn deadline_elapsed_uses_wrap_safe_half_range_compare() {
        assert!(!deadline_elapsed(100, 200));
        assert!(deadline_elapsed(200, 200));
        assert!(deadline_elapsed(250, 200));
    }

    #[test]
    fn required_power_includes_system_load_when_charge_is_disabled() {
        let demand = UsbPdPowerDemand {
            requested_charge_voltage_mv: 16_800,
            requested_charge_current_ma: 500,
            system_load_power_mw: 2_500,
            battery_voltage_mv: Some(15_000),
            measured_input_voltage_mv: None,
            charging_enabled: false,
        };

        assert_eq!(demand.required_power_mw(), 2_500);
    }

    #[test]
    fn peer_revision_downgrades_transmit_revision() {
        let mut manager = UsbPdSinkManager::new(NoopI2c);
        assert_eq!(manager.tx_spec_revision, SpecRevision::Rev30);

        manager.observe_peer_spec_revision(SpecRevision::Rev20, "test");

        assert_eq!(manager.peer_spec_revision, SpecRevision::Rev20);
        assert_eq!(manager.tx_spec_revision, SpecRevision::Rev20);
    }

    #[test]
    fn source_caps_without_safe_contract_arm_default_5v_charge_ready() {
        let mut manager = UsbPdSinkManager::new(NoopI2c);
        manager.state.attached = true;
        manager.source_caps_recovery_attempted = true;

        let message = source_caps_message(SpecRevision::Rev20, &[fixed_pdo_raw(21_000, 3_000)]);
        manager.handle_message(message, UsbPdPowerDemand::default(), 1_000);

        assert!(manager.state.contract.is_none());
        assert_eq!(
            manager.state.input_current_limit_ma,
            Some(sink_policy::DEFAULT_5V_FALLBACK_IINDPM_MA)
        );
        assert_eq!(manager.state.vindpm_mv, Some(4_000));
        assert_eq!(
            manager.charge_ready_at_ms,
            Some(1_000 + DEFAULT_5V_CHARGE_READY_DELAY_MS)
        );
        assert!(!manager.state.charge_ready);
    }

    #[test]
    fn source_caps_without_safe_contract_preserve_5v_source_current_cap() {
        let mut manager = UsbPdSinkManager::new(NoopI2c);
        manager.state.attached = true;
        manager.source_caps_recovery_attempted = true;

        let message = source_caps_message(
            SpecRevision::Rev20,
            &[fixed_pdo_raw(5_000, 500), fixed_pdo_raw(9_000, 500)],
        );
        let demand = UsbPdPowerDemand {
            system_load_power_mw: 3_000,
            ..UsbPdPowerDemand::default()
        };
        manager.handle_message(message, demand, 1_000);

        assert!(manager.state.contract.is_none());
        assert_eq!(manager.state.input_current_limit_ma, Some(500));
        assert_eq!(manager.state.vindpm_mv, Some(4_000));
    }

    #[test]
    fn deferred_first_request_restores_default_5v_limits() {
        let mut manager = UsbPdSinkManager::new(NoopI2c);
        manager.state.attached = true;
        manager.source_capabilities = pd::SourceCapabilities::from_message(&source_caps_message(
            SpecRevision::Rev20,
            &[fixed_pdo_raw(5_000, 500), fixed_pdo_raw(9_000, 3_000)],
        ));
        manager.contract_tracker.begin_request(ActiveContract {
            kind: ContractKind::Fixed,
            object_position: 2,
            voltage_mv: 9_000,
            current_ma: 300,
            source_max_current_ma: 3_000,
            input_current_limit_ma: Some(300),
            vindpm_mv: Some(8_000),
        });

        manager.handle_message(
            control_message(ControlMessageType::Wait, SpecRevision::Rev20),
            UsbPdPowerDemand::default(),
            2_000,
        );

        assert!(manager.state.contract.is_none());
        assert_eq!(manager.state.input_current_limit_ma, Some(500));
        assert_eq!(manager.state.vindpm_mv, Some(4_000));
        assert_eq!(
            manager.charge_ready_at_ms,
            Some(2_000 + DEFAULT_5V_CHARGE_READY_DELAY_MS)
        );
    }

    #[test]
    fn controller_error_tears_down_stale_pd_state() {
        let mut manager = UsbPdSinkManager::new(NoopI2c);
        manager.state.attached = true;
        manager.state.charge_ready = true;
        manager.state.vbus_present = Some(true);
        manager.state.polarity = Some(CcPolarity::Cc1);
        manager.state.contract = Some(ActiveContract {
            kind: ContractKind::Fixed,
            object_position: 2,
            voltage_mv: 9_000,
            current_ma: 1_000,
            source_max_current_ma: 3_000,
            input_current_limit_ma: Some(1_000),
            vindpm_mv: Some(8_000),
        });
        manager.state.input_current_limit_ma = Some(1_000);
        manager.state.vindpm_mv = Some(8_000);
        manager.source_capabilities = Some(pd::SourceCapabilities::empty(SpecRevision::Rev20));
        manager.attached_at_ms = Some(1_000);
        manager.source_caps_recovery_attempted = true;

        manager.teardown_controller_state_on_phy_error();

        assert!(!manager.state.attached);
        assert!(!manager.state.charge_ready);
        assert_eq!(manager.state.contract, None);
        assert_eq!(manager.state.vbus_present, None);
        assert_eq!(manager.state.polarity, None);
        assert_eq!(manager.state.input_current_limit_ma, None);
        assert_eq!(manager.state.vindpm_mv, None);
        assert_eq!(manager.source_capabilities, None);
        assert_eq!(manager.attached_at_ms, None);
        assert_eq!(manager.tx_spec_revision, SpecRevision::Rev30);
        assert_eq!(manager.peer_spec_revision, SpecRevision::Rev30);
    }

    #[test]
    fn raw_vbus_loss_with_lingering_input_voltage_keeps_existing_session() {
        let mut manager = UsbPdSinkManager::new(LenientI2c);
        manager.state.attached = true;
        manager.state.charge_ready = true;
        manager.state.vbus_present = Some(true);
        manager.state.polarity = Some(CcPolarity::Cc1);
        manager.state.contract = Some(ActiveContract {
            kind: ContractKind::Pps,
            object_position: 3,
            voltage_mv: 16_000,
            current_ma: 1_000,
            source_max_current_ma: 3_000,
            input_current_limit_ma: Some(1_000),
            vindpm_mv: Some(14_000),
        });
        manager.source_capabilities = Some(pd::SourceCapabilities::empty(SpecRevision::Rev30));
        manager.attached_at_ms = Some(1_000);
        manager.source_caps_recovery_attempted = true;
        manager.last_source_caps_requery_at_ms = Some(1_500);

        manager.handle_irq_snapshot(
            irq_snapshot_with_cc_and_vbus(fusb302::status1a::TOGS_SNK1, false),
            UsbPdPowerDemand {
                measured_input_voltage_mv: Some(5_100),
                ..UsbPdPowerDemand::default()
            },
            1_500,
        );

        assert!(manager.state.attached);
        assert_eq!(manager.consecutive_raw_vbus_absent_polls, 1);
        assert_eq!(
            manager.state.contract.map(|contract| contract.kind),
            Some(ContractKind::Pps)
        );

        manager.handle_irq_snapshot(
            irq_snapshot_with_cc_and_vbus(fusb302::status1a::TOGS_SNK1, false),
            UsbPdPowerDemand {
                measured_input_voltage_mv: Some(5_100),
                ..UsbPdPowerDemand::default()
            },
            2_000,
        );

        assert!(manager.state.attached);
        assert_eq!(
            manager.state.contract.map(|contract| contract.kind),
            Some(ContractKind::Pps)
        );
        assert_eq!(manager.state.polarity, Some(CcPolarity::Cc1));
        assert_eq!(manager.state.vbus_present, Some(true));
        assert_eq!(manager.attached_at_ms, Some(1_000));
        assert!(manager.source_caps_recovery_attempted);
        assert_eq!(manager.last_source_caps_requery_at_ms, Some(1_500));
    }

    #[test]
    fn attached_session_ignores_togs_loss_while_vbus_remains_present() {
        let mut manager = UsbPdSinkManager::new(LenientI2c);
        manager.state.attached = true;
        manager.state.polarity = Some(CcPolarity::Cc1);
        manager.state.vbus_present = Some(true);
        manager.state.contract = Some(ActiveContract {
            kind: ContractKind::Pps,
            object_position: 3,
            voltage_mv: 16_000,
            current_ma: 1_000,
            source_max_current_ma: 3_000,
            input_current_limit_ma: Some(1_000),
            vindpm_mv: Some(14_000),
        });
        manager.attached_at_ms = Some(500);
        manager.source_caps_recovery_attempted = true;
        manager.last_source_caps_requery_at_ms = Some(900);
        manager.last_source_caps_recovery_at_ms = Some(900);

        manager.handle_irq_snapshot(
            irq_snapshot_with_cc_and_vbus(fusb302::status1a::TOGS_OFF, true),
            UsbPdPowerDemand {
                measured_input_voltage_mv: Some(5_100),
                ..UsbPdPowerDemand::default()
            },
            1_000,
        );

        assert!(manager.state.attached);
        assert_eq!(manager.consecutive_cc_absent_polls, 0);
        assert_eq!(
            manager.state.contract.map(|contract| contract.kind),
            Some(ContractKind::Pps)
        );

        manager.handle_irq_snapshot(
            irq_snapshot_with_cc_and_vbus(fusb302::status1a::TOGS_OFF, true),
            UsbPdPowerDemand {
                measured_input_voltage_mv: Some(5_100),
                ..UsbPdPowerDemand::default()
            },
            1_500,
        );

        assert!(manager.state.attached);
        assert_eq!(manager.state.polarity, Some(CcPolarity::Cc1));
        assert_eq!(
            manager.state.contract.map(|contract| contract.kind),
            Some(ContractKind::Pps)
        );
        assert_eq!(manager.attached_at_ms, Some(500));
        assert!(manager.source_caps_recovery_attempted);
        assert_eq!(manager.last_source_caps_requery_at_ms, Some(900));
        assert_eq!(manager.last_source_caps_recovery_at_ms, Some(900));
    }

    #[test]
    fn fresh_attach_fully_reinitializes_phy_before_enabling_receive() {
        let mut manager = UsbPdSinkManager::new(LenientI2c);
        manager.initialized = true;
        manager.state.enabled = true;
        manager.state.controller_ready = true;

        manager.handle_irq_snapshot(
            irq_snapshot_with_cc_and_vbus(fusb302::status1a::TOGS_SNK2, true),
            UsbPdPowerDemand {
                measured_input_voltage_mv: Some(5_100),
                ..UsbPdPowerDemand::default()
            },
            2_000,
        );

        assert!(manager.state.attached);
        assert_eq!(manager.state.polarity, Some(CcPolarity::Cc2));
        assert!(manager.initialized);
        assert!(manager.state.controller_ready);
        assert_eq!(manager.attached_at_ms, Some(2_000));
    }

    #[test]
    fn attached_session_can_backfill_missing_polarity_from_detected_attach_state() {
        let mut manager = UsbPdSinkManager::new(LenientI2c);
        manager.state.attached = true;
        manager.state.polarity = None;
        manager.state.vbus_present = Some(true);

        manager.handle_irq_snapshot(
            irq_snapshot_with_cc_and_vbus(fusb302::status1a::TOGS_SNK2, true),
            UsbPdPowerDemand {
                measured_input_voltage_mv: Some(5_100),
                ..UsbPdPowerDemand::default()
            },
            2_000,
        );

        assert!(manager.state.attached);
        assert_eq!(manager.state.polarity, Some(CcPolarity::Cc2));
    }

    #[test]
    fn persistent_no_source_caps_rearms_phy_after_hard_timeout() {
        let mut manager = UsbPdSinkManager::new(LenientI2c);
        manager.initialized = true;
        manager.state.enabled = true;
        manager.state.attached = true;
        manager.state.controller_ready = true;
        manager.state.vbus_present = Some(true);
        manager.state.polarity = Some(CcPolarity::Cc1);
        manager.attached_at_ms = Some(1_000);
        manager.source_caps_recovery_attempted = true;
        manager.last_source_caps_recovery_at_ms = Some(4_000);

        manager.maybe_recover_missing_source_caps(UsbPdPowerDemand::default(), 9_500);

        assert!(!manager.state.attached);
        assert_eq!(manager.state.contract, None);
        assert_eq!(manager.state.polarity, None);
        assert_eq!(manager.attached_at_ms, None);
        assert!(manager.initialized);
        assert!(manager.state.controller_ready);
        assert!(!manager.source_caps_recovery_attempted);
        assert_eq!(manager.last_source_caps_recovery_at_ms, None);
    }

    fn raw_vbus_loss_rearms_attach_and_source_caps_recovery_on_replug() {
        let mut manager = UsbPdSinkManager::new(LenientI2c);
        manager.state.attached = true;
        manager.state.polarity = Some(CcPolarity::Cc1);
        manager.state.vbus_present = Some(true);
        manager.state.contract = Some(ActiveContract {
            kind: ContractKind::Fixed,
            object_position: 1,
            voltage_mv: 5_000,
            current_ma: 500,
            source_max_current_ma: 3_000,
            input_current_limit_ma: Some(500),
            vindpm_mv: Some(4_000),
        });
        manager.attached_at_ms = Some(500);
        manager.source_caps_recovery_attempted = true;
        manager.last_source_caps_requery_at_ms = Some(900);
        manager.last_source_caps_recovery_at_ms = Some(900);

        manager.handle_irq_snapshot(
            irq_snapshot_with_cc_and_vbus(fusb302::status1a::TOGS_SNK1, false),
            UsbPdPowerDemand {
                measured_input_voltage_mv: Some(5_100),
                ..UsbPdPowerDemand::default()
            },
            1_000,
        );

        assert!(manager.state.attached);
        assert_eq!(manager.consecutive_raw_vbus_absent_polls, 1);

        manager.handle_irq_snapshot(
            irq_snapshot_with_cc_and_vbus(fusb302::status1a::TOGS_SNK1, false),
            UsbPdPowerDemand {
                measured_input_voltage_mv: Some(300),
                ..UsbPdPowerDemand::default()
            },
            1_500,
        );

        assert!(!manager.state.attached);

        manager.handle_irq_snapshot(
            irq_snapshot_with_cc_and_vbus(fusb302::status1a::TOGS_SNK2, true),
            UsbPdPowerDemand {
                measured_input_voltage_mv: Some(5_100),
                ..UsbPdPowerDemand::default()
            },
            2_000,
        );

        assert!(manager.state.attached);
        assert_eq!(manager.state.polarity, Some(CcPolarity::Cc2));
        assert_eq!(manager.state.contract, None);
        assert_eq!(manager.attached_at_ms, Some(2_000));
        assert!(!manager.source_caps_recovery_attempted);
        assert_eq!(manager.last_source_caps_requery_at_ms, None);
        assert_eq!(manager.last_source_caps_recovery_at_ms, None);
    }

    #[test]
    fn single_raw_vbus_loss_glitch_does_not_detach_active_session() {
        let mut manager = UsbPdSinkManager::new(LenientI2c);
        manager.state.attached = true;
        manager.state.polarity = Some(CcPolarity::Cc2);
        manager.state.vbus_present = Some(true);
        manager.state.contract = Some(ActiveContract {
            kind: ContractKind::Fixed,
            object_position: 1,
            voltage_mv: 5_000,
            current_ma: 500,
            source_max_current_ma: 3_000,
            input_current_limit_ma: Some(500),
            vindpm_mv: Some(4_000),
        });

        manager.handle_irq_snapshot(
            irq_snapshot_with_cc_and_vbus(fusb302::status1a::TOGS_SNK2, false),
            UsbPdPowerDemand {
                measured_input_voltage_mv: Some(5_100),
                ..UsbPdPowerDemand::default()
            },
            1_000,
        );

        manager.handle_irq_snapshot(
            irq_snapshot_with_cc_and_vbus(fusb302::status1a::TOGS_SNK2, true),
            UsbPdPowerDemand {
                measured_input_voltage_mv: Some(5_100),
                ..UsbPdPowerDemand::default()
            },
            1_500,
        );

        assert!(manager.state.attached);
        assert_eq!(manager.state.polarity, Some(CcPolarity::Cc2));
        assert_eq!(manager.consecutive_raw_vbus_absent_polls, 0);
        assert_eq!(
            manager.state.contract.map(|contract| contract.kind),
            Some(ContractKind::Fixed)
        );
    }

    #[test]
    fn attach_recovery_ignores_vbus_glitches_while_cc_is_still_present() {
        let mut manager = UsbPdSinkManager::new(LenientI2c);
        manager.state.attached = true;
        manager.state.polarity = Some(CcPolarity::Cc2);
        manager.state.vbus_present = Some(true);
        manager.attached_at_ms = Some(500);
        manager.source_caps_recovery_attempted = true;
        manager.last_source_caps_recovery_at_ms = Some(900);

        manager.handle_irq_snapshot(
            irq_snapshot_with_cc_and_vbus(fusb302::status1a::TOGS_SNK2, false),
            UsbPdPowerDemand {
                measured_input_voltage_mv: Some(320),
                ..UsbPdPowerDemand::default()
            },
            1_000,
        );

        assert!(manager.state.attached);
        assert_eq!(manager.state.polarity, Some(CcPolarity::Cc2));
        assert_eq!(manager.state.contract, None);
        assert_eq!(manager.consecutive_raw_vbus_absent_polls, 0);
        assert_eq!(manager.consecutive_effective_vbus_absent_polls, 0);
    }

    #[test]
    fn deferred_request_rearms_existing_contract_charge_gate() {
        let mut manager = UsbPdSinkManager::new(NoopI2c);
        let active_contract = ActiveContract {
            kind: ContractKind::Fixed,
            object_position: 1,
            voltage_mv: 9_000,
            current_ma: 1_000,
            source_max_current_ma: 3_000,
            input_current_limit_ma: Some(1_000),
            vindpm_mv: Some(8_000),
        };
        let requested_contract = ActiveContract {
            voltage_mv: 15_000,
            ..active_contract
        };

        manager.state.contract = Some(active_contract);
        manager.contract_tracker.begin_request(active_contract);
        assert!(manager.contract_tracker.mark_accept_received());
        assert_eq!(
            manager.contract_tracker.commit_pending_contract(),
            Some(active_contract)
        );
        manager.contract_tracker.begin_request(requested_contract);
        manager.disarm_charge_ready("test");

        manager.handle_message(
            control_message(ControlMessageType::Wait, SpecRevision::Rev20),
            UsbPdPowerDemand::default(),
            2_000,
        );

        assert_eq!(
            manager.contract_tracker.active_contract(),
            Some(active_contract)
        );
        assert!(!manager.contract_tracker.request_in_flight());
        assert_eq!(
            manager.charge_ready_at_ms,
            Some(2_000 + CONTRACT_CHARGE_READY_DELAY_MS)
        );
        assert!(!manager.state.charge_ready);
    }

    #[test]
    fn cached_source_caps_stall_rearms_phy_after_timeout() {
        let mut manager = UsbPdSinkManager::new(LenientI2c);
        manager.initialized = true;
        manager.state.enabled = true;
        manager.state.attached = true;
        manager.state.controller_ready = true;
        manager.state.vbus_present = Some(true);
        manager.state.polarity = Some(CcPolarity::Cc1);
        manager.attached_at_ms = Some(1_000);
        manager.source_capabilities = Some(pd::SourceCapabilities::empty(SpecRevision::Rev30));

        manager.maybe_recover_stalled_no_contract_with_cached_caps(
            1_000 + NO_CONTRACT_SOURCE_CAPS_REARM_TIMEOUT_MS,
        );

        assert!(!manager.state.attached);
        assert_eq!(manager.state.contract, None);
        assert_eq!(manager.state.polarity, None);
        assert_eq!(manager.attached_at_ms, None);
        assert!(manager.initialized);
        assert!(manager.state.controller_ready);
    }

    #[test]
    fn cached_source_caps_recovery_ignores_vbus_glitches_while_cc_is_present() {
        let mut manager = UsbPdSinkManager::new(LenientI2c);
        manager.state.attached = true;
        manager.state.polarity = Some(CcPolarity::Cc2);
        manager.state.vbus_present = Some(true);
        manager.attached_at_ms = Some(500);
        manager.source_capabilities = Some(pd::SourceCapabilities::empty(SpecRevision::Rev30));

        manager.handle_irq_snapshot(
            irq_snapshot_with_cc_and_vbus(fusb302::status1a::TOGS_SNK2, false),
            UsbPdPowerDemand {
                measured_input_voltage_mv: Some(320),
                ..UsbPdPowerDemand::default()
            },
            1_000,
        );

        assert!(manager.state.attached);
        assert_eq!(manager.state.polarity, Some(CcPolarity::Cc2));
        assert_eq!(manager.consecutive_raw_vbus_absent_polls, 0);
        assert_eq!(manager.consecutive_effective_vbus_absent_polls, 0);
    }

    #[test]
    fn no_contract_attach_stabilization_suppresses_detach_without_cc() {
        let mut manager = UsbPdSinkManager::new(LenientI2c);
        manager.state.attached = true;
        manager.state.vbus_present = Some(true);
        manager.attached_at_ms = Some(1_000);

        manager.handle_irq_snapshot(
            irq_snapshot_with_cc_and_vbus(fusb302::status1a::TOGS_NONE, false),
            UsbPdPowerDemand {
                measured_input_voltage_mv: Some(2_950),
                ..UsbPdPowerDemand::default()
            },
            1_500,
        );

        assert!(manager.state.attached);
        assert_eq!(manager.state.vbus_present, Some(false));
        assert_eq!(manager.consecutive_raw_vbus_absent_polls, 0);
        assert_eq!(manager.consecutive_effective_vbus_absent_polls, 0);
    }

    #[test]
    fn no_contract_attach_stabilization_expires_and_allows_detach() {
        let mut manager = UsbPdSinkManager::new(LenientI2c);
        manager.state.attached = true;
        manager.state.vbus_present = Some(true);
        manager.attached_at_ms = Some(0);

        manager.handle_irq_snapshot(
            irq_snapshot_with_cc_and_vbus(fusb302::status1a::TOGS_NONE, false),
            UsbPdPowerDemand {
                measured_input_voltage_mv: Some(2_950),
                ..UsbPdPowerDemand::default()
            },
            NO_CONTRACT_ATTACH_STABILIZE_MS + 10,
        );
        manager.handle_irq_snapshot(
            irq_snapshot_with_cc_and_vbus(fusb302::status1a::TOGS_NONE, false),
            UsbPdPowerDemand {
                measured_input_voltage_mv: Some(2_950),
                ..UsbPdPowerDemand::default()
            },
            NO_CONTRACT_ATTACH_STABILIZE_MS + 20,
        );

        assert!(!manager.state.attached);
        assert_eq!(manager.state.vbus_present, Some(false));
    }

    #[test]
    fn retry_fail_without_contract_forces_full_rearm() {
        let mut manager = UsbPdSinkManager::new(LenientI2c);
        manager.state.attached = true;
        manager.state.vbus_present = Some(true);
        manager.state.polarity = Some(CcPolarity::Cc1);
        manager.attached_at_ms = Some(0);

        manager.handle_irq_snapshot(
            irq_snapshot_with_retry_fail(fusb302::status1a::TOGS_SNK1, true),
            UsbPdPowerDemand {
                measured_input_voltage_mv: Some(5_100),
                ..UsbPdPowerDemand::default()
            },
            1_000,
        );

        assert!(!manager.state.attached);
        assert_eq!(manager.state.vbus_present, Some(false));
        assert!(manager.initialized);
        assert!(manager.state.controller_ready);
    }

    #[test]
    fn source_caps_requery_uses_initial_delay_before_first_probe() {
        let mut manager = UsbPdSinkManager::new(NoopI2c);
        manager.last_request_at_ms = 0;

        assert!(!manager.source_caps_requery_due(SOURCE_CAPS_REQUERY_DELAY_MS - 1));
        assert!(manager.source_caps_requery_due(SOURCE_CAPS_REQUERY_DELAY_MS));
    }

    #[test]
    fn source_caps_requery_uses_retry_interval_after_probe() {
        let mut manager = UsbPdSinkManager::new(NoopI2c);
        manager.last_source_caps_requery_at_ms = Some(0);

        assert!(!manager.source_caps_requery_due(SOURCE_CAPS_REQUERY_RETRY_MS - 1));
        assert!(manager.source_caps_requery_due(SOURCE_CAPS_REQUERY_RETRY_MS));
    }

    #[test]
    fn source_caps_with_pps_clears_requery_retry_timer() {
        let mut manager = UsbPdSinkManager::new(NoopI2c);
        manager.last_source_caps_requery_at_ms = Some(1_234);

        let message = source_caps_message(
            SpecRevision::Rev20,
            &[
                fixed_pdo_raw(5_000, 3_000),
                (0xC0000000u32
                    | ((5_000u32 / 100) << 17)
                    | ((18_000u32 / 100) << 8)
                    | (3_000u32 / 50)),
            ],
        );
        manager.contract_tracker.begin_request(ActiveContract {
            kind: ContractKind::Fixed,
            object_position: 1,
            voltage_mv: 5_000,
            current_ma: 500,
            source_max_current_ma: 3_000,
            input_current_limit_ma: Some(500),
            vindpm_mv: Some(4_000),
        });
        manager.handle_message(message, UsbPdPowerDemand::default(), 2_000);

        assert_eq!(manager.last_source_caps_requery_at_ms, None);
    }

    #[test]
    fn stalled_contract_request_times_out_and_clears_in_flight_state() {
        let mut manager = UsbPdSinkManager::new(LenientI2c);
        manager.state.attached = true;
        manager.attached_at_ms = Some(0);
        manager.contract_tracker.begin_request(ActiveContract {
            kind: ContractKind::Pps,
            object_position: 2,
            voltage_mv: 16_000,
            current_ma: 1_000,
            source_max_current_ma: 3_000,
            input_current_limit_ma: Some(1_000),
            vindpm_mv: Some(14_000),
        });
        manager.last_request_at_ms = 0;

        manager.tick(
            UsbPdPowerDemand::default(),
            false,
            CONTRACT_REQUEST_TIMEOUT_MS - 1,
        );
        assert!(manager.contract_tracker.request_in_flight());

        manager.tick(
            UsbPdPowerDemand::default(),
            false,
            CONTRACT_REQUEST_TIMEOUT_MS,
        );
        assert!(!manager.contract_tracker.request_in_flight());
    }

    #[test]
    fn stalled_contract_request_retries_from_cached_source_caps() {
        let mut manager = UsbPdSinkManager::new(LenientI2c);
        manager.state.attached = true;
        manager.attached_at_ms = Some(0);
        manager.source_capabilities = pd::SourceCapabilities::from_message(&source_caps_message(
            SpecRevision::Rev30,
            &[
                fixed_pdo_raw(5_000, 3_000),
                pd::Apdo::new_pps(15_000, 21_000, 3_000).raw(),
            ],
        ));
        manager.contract_tracker.begin_request(ActiveContract {
            kind: ContractKind::Pps,
            object_position: 2,
            voltage_mv: 16_000,
            current_ma: 1_000,
            source_max_current_ma: 3_000,
            input_current_limit_ma: Some(1_000),
            vindpm_mv: Some(14_000),
        });
        manager.last_request_at_ms = 0;

        let demand = UsbPdPowerDemand {
            requested_charge_voltage_mv: 16_800,
            requested_charge_current_ma: 500,
            system_load_power_mw: 2_500,
            charging_enabled: true,
            ..UsbPdPowerDemand::default()
        };

        manager.tick(demand, false, CONTRACT_REQUEST_TIMEOUT_MS);

        assert!(manager.contract_tracker.request_in_flight());
        assert_eq!(manager.last_request_at_ms, CONTRACT_REQUEST_TIMEOUT_MS);
        assert_eq!(
            manager.contract_tracker.pending_contract().map(|c| c.kind),
            Some(ContractKind::Pps)
        );
    }

    #[test]
    fn missing_source_caps_retry_reissues_soft_reset_when_charge_path_is_active() {
        let mut manager = UsbPdSinkManager::new(NoopI2c);
        manager.state.attached = true;
        manager.state.charge_ready = true;
        manager.attached_at_ms = Some(0);
        manager.source_caps_recovery_attempted = true;
        manager.last_source_caps_recovery_at_ms = Some(0);
        manager.message_id = 3;

        manager.maybe_recover_missing_source_caps(
            UsbPdPowerDemand {
                charging_enabled: true,
                ..UsbPdPowerDemand::default()
            },
            SOURCE_CAPS_RECOVERY_RETRY_MS,
        );

        assert_eq!(manager.message_id, 4);
        assert_eq!(
            manager.last_source_caps_recovery_at_ms,
            Some(SOURCE_CAPS_RECOVERY_RETRY_MS)
        );
    }

    #[test]
    fn missing_source_caps_retry_reissues_soft_reset_even_when_charge_path_is_idle() {
        let mut manager = UsbPdSinkManager::new(NoopI2c);
        manager.state.attached = true;
        manager.attached_at_ms = Some(0);
        manager.source_caps_recovery_attempted = true;
        manager.last_source_caps_recovery_at_ms = Some(0);
        manager.message_id = 7;

        manager.maybe_recover_missing_source_caps(
            UsbPdPowerDemand::default(),
            SOURCE_CAPS_RECOVERY_RETRY_MS,
        );

        assert_eq!(manager.message_id, 8);
        assert_eq!(
            manager.last_source_caps_recovery_at_ms,
            Some(SOURCE_CAPS_RECOVERY_RETRY_MS)
        );
    }

    #[test]
    fn missing_source_caps_retry_waits_for_retry_interval() {
        let mut manager = UsbPdSinkManager::new(NoopI2c);
        manager.state.attached = true;
        manager.state.charge_ready = true;
        manager.attached_at_ms = Some(0);
        manager.source_caps_recovery_attempted = true;
        manager.last_source_caps_recovery_at_ms = Some(0);
        manager.message_id = 3;

        manager.maybe_recover_missing_source_caps(
            UsbPdPowerDemand {
                charging_enabled: true,
                ..UsbPdPowerDemand::default()
            },
            SOURCE_CAPS_RECOVERY_RETRY_MS - 1,
        );

        assert_eq!(manager.message_id, 3);
        assert_eq!(manager.last_source_caps_recovery_at_ms, Some(0));
    }
}
