pub mod contract_tracker;
pub mod fusb302;
pub mod pd;
pub mod sink_policy;

use contract_tracker::ContractTracker;
use defmt::{debug, info, warn};
use fusb302::{CcPolarity, Fusb302, IrqSnapshot};
use pd::{ControlMessageType, DataMessageType, Message, MessageHeader, SpecRevision};
use sink_policy::{ContractPlan, LocalCapabilities};

const PHY_POLL_INTERVAL_MS: u32 = 250;
const ERROR_RETRY_INTERVAL_MS: u32 = 1_000;

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
    pub vbus_present: Option<bool>,
    pub polarity: Option<CcPolarity>,
    pub contract: Option<ActiveContract>,
    pub unsafe_source_latched: bool,
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

pub struct UsbPdSinkManager<I2C> {
    phy: Fusb302<I2C>,
    local_capabilities: LocalCapabilities,
    state: UsbPdPortState,
    initialized: bool,
    next_retry_at_ms: u32,
    last_phy_poll_at_ms: u32,
    last_request_at_ms: u32,
    message_id: u8,
    source_spec_revision: SpecRevision,
    source_capabilities: Option<pd::SourceCapabilities>,
    contract_tracker: ContractTracker<ActiveContract>,
    unsafe_hard_reset_sent: bool,
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
            message_id: 0,
            source_spec_revision: pd::FUSB302_MAX_SPEC_REVISION,
            source_capabilities: None,
            contract_tracker: ContractTracker::default(),
            unsafe_hard_reset_sent: false,
        }
    }

    pub fn init_best_effort(&mut self) -> UsbPdPortState {
        if !self.state.enabled {
            return self.state;
        }
        match self.phy.init_sink(self.source_spec_revision) {
            Ok(device_id) => {
                self.initialized = true;
                self.state.controller_ready = true;
                info!("usb_pd: fusb302 init ok device_id=0x{=u8:x}", device_id);
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
            if self.phy.init_sink(self.source_spec_revision).is_ok() {
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
                    self.state.controller_ready = false;
                    self.initialized = false;
                    self.next_retry_at_ms = now_ms.wrapping_add(ERROR_RETRY_INTERVAL_MS);
                    return self.state;
                }
            }
        }

        if let (Some(source_caps), Some(active_contract)) = (
            self.source_capabilities,
            self.contract_tracker.active_contract(),
        ) {
            if active_contract.kind == ContractKind::Pps
                && !self.state.unsafe_source_latched
                && !self.contract_tracker.request_in_flight()
            {
                if let Some(plan) =
                    sink_policy::select_contract(&self.local_capabilities, &source_caps, demand)
                {
                    if sink_policy::should_refresh_pps_contract(
                        active_contract,
                        plan.contract,
                        now_ms,
                        self.last_request_at_ms,
                    ) {
                        if let Err(err) =
                            self.send_contract_request(plan, self.source_spec_revision, now_ms)
                        {
                            warn!(
                                "usb_pd: pps refresh request failed err={}",
                                fusb302_error_kind(&err)
                            );
                        }
                    }
                }
            }
        }

        self.state
    }

    fn handle_irq_snapshot(
        &mut self,
        snapshot: IrqSnapshot,
        demand: UsbPdPowerDemand,
        now_ms: u32,
    ) {
        self.state.vbus_present = Some(snapshot.vbus_present());

        match snapshot.attached_sink_polarity() {
            Some(polarity) => {
                if !self.state.attached || self.state.polarity != Some(polarity) {
                    self.state.attached = true;
                    self.state.polarity = Some(polarity);
                    self.message_id = 0;
                    self.clear_contract_tracking();
                    self.source_capabilities = None;
                    if let Err(err) = self
                        .phy
                        .enable_pd_receive(polarity, self.source_spec_revision)
                    {
                        warn!("usb_pd: enable rx failed err={}", fusb302_error_kind(&err));
                        self.initialized = false;
                        self.state.controller_ready = false;
                        return;
                    }
                    info!("usb_pd: attached polarity={}", polarity_name(polarity));
                }
            }
            None => {
                if self.state.attached {
                    info!("usb_pd: detached");
                    self.reset_contract_state(true);
                    let _ = self.phy.start_sink_toggle();
                }
                return;
            }
        }

        if snapshot.hard_reset_received() || snapshot.retry_failed() {
            warn!(
                "usb_pd: reset/retry event hard={=bool} retry_fail={=bool}",
                snapshot.hard_reset_received(),
                snapshot.retry_failed()
            );
            self.reset_contract_state(false);
            if let Some(polarity) = self.state.polarity {
                let _ = self
                    .phy
                    .enable_pd_receive(polarity, self.source_spec_revision);
            }
        }

        if snapshot.soft_reset_received() && !snapshot.rx_message_ready() {
            warn!("usb_pd: source requested soft reset without fifo payload");
            self.handle_peer_soft_reset(self.source_spec_revision);
        }

        if snapshot.rx_message_ready() {
            match self.phy.read_message() {
                Ok(Some(message)) => self.handle_message(message, demand, now_ms),
                Ok(None) => {}
                Err(err) => {
                    warn!(
                        "usb_pd: read message failed err={}",
                        fusb302_error_kind(&err)
                    );
                    let _ = self.phy.flush_rx();
                }
            }
        }
    }

    fn handle_message(&mut self, message: Message, demand: UsbPdPowerDemand, now_ms: u32) {
        if let Some(kind) = message.header.data_message_type() {
            match kind {
                DataMessageType::SourceCapabilities => {
                    if self.state.unsafe_source_latched {
                        debug!("usb_pd: ignoring source caps because unsafe source is latched");
                        return;
                    }
                    let Some(source_caps) = pd::SourceCapabilities::from_message(&message) else {
                        return;
                    };
                    let filtered = sink_policy::filter_source_capabilities(&source_caps);
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
                    self.source_spec_revision =
                        pd::clamp_fusb302_spec_revision(source_caps.spec_revision);
                    self.source_capabilities = Some(source_caps);
                    if self.contract_tracker.request_in_flight() {
                        debug!("usb_pd: preserving in-flight contract across source caps refresh");
                        return;
                    }
                    if let Some(plan) = sink_policy::select_contract_from_filtered(
                        &self.local_capabilities,
                        &filtered,
                        demand,
                    ) {
                        if let Err(err) =
                            self.send_contract_request(plan, self.source_spec_revision, now_ms)
                        {
                            warn!(
                                "usb_pd: request send failed err={}",
                                fusb302_error_kind(&err)
                            );
                        }
                    } else {
                        warn!("usb_pd: no safe PD/PPS contract available");
                    }
                }
                DataMessageType::Request | DataMessageType::SinkCapabilities => {}
            }
            return;
        }

        match message.header.control_message_type() {
            Some(ControlMessageType::Accept) if self.contract_tracker.mark_accept_received() => {
                debug!("usb_pd: contract accepted");
            }
            Some(ControlMessageType::PsRdy) => {
                if let Some(contract) = self.contract_tracker.commit_pending_contract() {
                    self.state.contract = Some(contract);
                    info!(
                        "usb_pd: contract active kind={} voltage_mv={=u16} current_ma={=u16}",
                        contract_kind_name(contract.kind),
                        contract.voltage_mv,
                        contract.current_ma
                    );
                }
            }
            Some(ControlMessageType::Reject) | Some(ControlMessageType::Wait) => {
                warn!("usb_pd: source deferred request");
                self.contract_tracker.cancel_pending_request();
            }
            Some(ControlMessageType::SoftReset) => {
                self.handle_peer_soft_reset(message.header.spec_revision());
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
        self.phy.send_message(&message)?;
        self.message_id = (self.message_id + 1) & 0x07;
        Ok(())
    }

    fn clear_contract_tracking(&mut self) {
        self.contract_tracker.clear_all();
        self.state.contract = None;
    }

    fn handle_peer_soft_reset(&mut self, spec_revision: SpecRevision) {
        warn!("usb_pd: source requested soft reset");
        self.source_spec_revision = pd::clamp_fusb302_spec_revision(spec_revision);
        self.reset_contract_state(false);
        if let Err(err) =
            self.send_control_message(ControlMessageType::Accept, self.source_spec_revision)
        {
            warn!(
                "usb_pd: soft reset accept failed err={}",
                fusb302_error_kind(&err)
            );
        }
    }

    fn reset_contract_state(&mut self, detach: bool) {
        self.clear_contract_tracking();
        self.source_capabilities = None;
        self.message_id = 0;
        if detach {
            self.source_spec_revision = pd::FUSB302_MAX_SPEC_REVISION;
            self.state.attached = false;
            self.state.vbus_present = Some(false);
            self.state.polarity = None;
            self.state.unsafe_source_latched = false;
            self.unsafe_hard_reset_sent = false;
        }
    }
}
