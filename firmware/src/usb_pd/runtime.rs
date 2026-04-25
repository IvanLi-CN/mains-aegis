use super::*;

impl<I2C> UsbPdSinkManager<I2C>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    pub(super) fn handle_irq_snapshot(
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
            match snapshot.attached_cc_present_hint() {
                Some(cc_present) => next_absent_polls(self.consecutive_cc_absent_polls, cc_present),
                None => self.consecutive_cc_absent_polls,
            }
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

        let hard_reset_recovering =
            self.in_no_contract_hard_reset_sent() || self.in_no_contract_hard_reset_wait();

        if !self.state.attached {
            self.observe_boot_unattached_candidate(
                detected_attach_polarity.is_none() && !vbus_present,
                now_ms,
            );
        }

        if self.state.attached
            && cc_absent_detach_debounce_elapsed(self.consecutive_cc_absent_polls)
        {
            info!(
                "usb_pd: detached cc_absent bc_lvl={=u8} activity={=bool} absent_polls={=u8}",
                snapshot.cc_level_status(),
                snapshot.cc_activity(),
                self.consecutive_cc_absent_polls
            );
            self.rearm_after_detach(now_ms, "cc_absent");
            return;
        }

        if self.state.attached
            && !attach_recovery_in_progress
            && !hard_reset_recovering
            && !raw_vbus_present
            && !vbus_present
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

        if self.state.attached
            && !attach_recovery_in_progress
            && !hard_reset_recovering
            && !vbus_present
        {
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
            self.no_contract_phase_started_at_ms = Some(now_ms);
            self.no_contract_recovery_phase = Some(NoContractRecoveryPhase::FreshAttach);
            self.source_caps_recovery_attempted = false;
            self.last_source_caps_requery_at_ms = None;
            self.last_source_caps_recovery_at_ms = None;
            self.consecutive_cc_absent_polls = 0;
            self.consecutive_raw_vbus_absent_polls = 0;
            self.consecutive_effective_vbus_absent_polls = 0;
            self.clear_contract_tracking();
            self.source_capabilities = None;
            self.disarm_charge_ready("attach");
            if let Err(err) = self.phy.enable_pd_receive(polarity, self.tx_spec_revision) {
                warn!("usb_pd: enable rx failed err={}", fusb302_error_kind(&err));
                self.initialized = false;
                self.state.controller_ready = false;
                return;
            }
            let _ = self.phy.poll_status();
            self.initialized = true;
            self.state.controller_ready = true;
            self.next_retry_at_ms = 0;
            let switches1 = self.phy.read_switches1().ok();
            if !self.active_no_contract_recovery_allowed() {
                self.note_recovery_event(UsbPdRecoveryEvent::BootInheritedAttach);
            }
            esp_println::println!(
                "usb_pd: attached polarity={} spec_rev_bits={} action=rx_resume",
                polarity_name(polarity),
                self.tx_spec_revision.bits()
            );
            info!(
                "usb_pd: attached polarity={} switches1={=?} spec_rev_bits={=u8} action=rx_resume",
                polarity_name(polarity),
                switches1,
                self.tx_spec_revision.bits()
            );
            return;
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

        let defer_hard_reset_for_partial_rx = snapshot.hard_reset_received()
            && snapshot.rx_fifo_non_empty()
            && !snapshot.rx_message_ready();

        if defer_hard_reset_for_partial_rx {
            warn!(
                "usb_pd: defer hard reset while rx is still incomplete rx_non_empty={=bool} rx_ready={=bool} gcrc_sent={=bool}",
                snapshot.rx_fifo_non_empty(),
                snapshot.rx_message_ready(),
                snapshot.gcrc_sent()
            );
        } else if retry_fail_should_defer_for_rx(snapshot) {
            warn!(
                "usb_pd: defer retry fail during rx activity retry_fail={=bool} rx_non_empty={=bool} tx_sent={=bool} gcrc_sent={=bool}",
                snapshot.retry_failed(),
                snapshot.rx_fifo_non_empty(),
                snapshot.tx_sent(),
                snapshot.gcrc_sent()
            );
        } else if snapshot.retry_failed()
            && self.state.contract.is_none()
            && (self.in_no_contract_hard_reset_sent() || self.in_no_contract_hard_reset_wait())
        {
            warn!(
                "usb_pd: ignore retry fail during no-contract hard reset recovery waiting_send={=bool} waiting_caps={=bool}",
                self.in_no_contract_hard_reset_sent(),
                self.in_no_contract_hard_reset_wait()
            );
            self.state.vbus_present = Some(vbus_present);
            return;
        } else if snapshot.hard_reset_received() || snapshot.retry_failed() {
            let had_active_contract = self.state.contract.is_some();
            let had_source_caps = self.source_capabilities.is_some();
            esp_println::println!(
                "usb_pd: reset/retry event hard={} retry_fail={}",
                snapshot.hard_reset_received(),
                snapshot.retry_failed()
            );
            warn!(
                "usb_pd: reset/retry event hard={=bool} retry_fail={=bool}",
                snapshot.hard_reset_received(),
                snapshot.retry_failed()
            );
            if snapshot.hard_reset_received() && !had_active_contract {
                if !self.active_no_contract_recovery_allowed() {
                    self.enter_passive_no_contract_wait(now_ms, "peer_hard_reset_inherited_attach");
                    self.state.vbus_present = Some(vbus_present);
                    return;
                }

                let prior_wait_started_at_ms = if self.in_no_contract_hard_reset_wait() {
                    self.no_contract_phase_started_at_ms
                } else {
                    None
                };

                self.restart_no_contract_wait_for_caps(
                    now_ms,
                    "peer_hard_reset_no_contract",
                    prior_wait_started_at_ms,
                );
                self.state.vbus_present = Some(vbus_present);
                return;
            }

            self.reset_contract_state(false);
            if snapshot.retry_failed() && !had_active_contract && had_source_caps {
                if !self.active_no_contract_recovery_allowed() {
                    self.enter_passive_no_contract_wait(now_ms, "retry_fail_inherited_attach");
                    self.state.vbus_present = Some(vbus_present);
                    return;
                }
                if let Err(err) = self.phy.reset_pd_logic() {
                    warn!(
                        "usb_pd: retry-fail pd_reset failed err={}",
                        fusb302_error_kind(&err)
                    );
                } else if let Some(polarity) = self.state.polarity {
                    if let Err(err) = self.phy.enable_pd_receive(polarity, self.tx_spec_revision) {
                        warn!(
                            "usb_pd: retry-fail rx re-enable failed err={}",
                            fusb302_error_kind(&err)
                        );
                    }
                }
            } else {
                self.attached_at_ms = if had_active_contract {
                    Some(now_ms)
                } else {
                    self.attached_at_ms.or(Some(now_ms))
                };
                if let Some(polarity) = self.state.polarity {
                    let _ = self.phy.enable_pd_receive(polarity, self.tx_spec_revision);
                }
            }
            self.state.vbus_present = Some(vbus_present);
            return;
        }

        if snapshot.interrupta & fusb302::interrupta::HARD_SENT != 0
            && self.in_no_contract_hard_reset_sent()
        {
            self.restart_no_contract_wait_for_caps(now_ms, "hard_reset_sent", None);
            self.state.vbus_present = Some(vbus_present);
            return;
        }

        if snapshot.soft_reset_received() && !snapshot.rx_message_ready() {
            warn!("usb_pd: source requested soft reset without fifo payload");
            self.handle_peer_soft_reset(self.peer_spec_revision, now_ms);
        }

        if snapshot.rx_fifo_non_empty() && !snapshot.rx_message_ready() {
            self.partial_rx_started_at_ms.get_or_insert(now_ms);
            debug!(
                "usb_pd: defer partial rx rx_non_empty={=bool} rx_ready={=bool} gcrc_sent={=bool}",
                snapshot.rx_fifo_non_empty(),
                snapshot.rx_message_ready(),
                snapshot.gcrc_sent()
            );
        } else {
            self.partial_rx_started_at_ms = None;
        }

        if snapshot.rx_message_ready()
            || snapshot.rx_fifo_non_empty()
            || snapshot.gcrc_sent()
            || snapshot.tx_sent()
        {
            let mut trust_snapshot_message_ready = snapshot.rx_message_ready();
            loop {
                let read_result = if trust_snapshot_message_ready {
                    trust_snapshot_message_ready = false;
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
                        self.partial_rx_started_at_ms = None;
                        let _ = self.phy.flush_rx();
                        break;
                    }
                }

                match self.phy.rx_fifo_non_empty() {
                    Ok(true) => {}
                    Ok(false) => {
                        self.partial_rx_started_at_ms = None;
                        break;
                    }
                    Err(err) => {
                        warn!(
                            "usb_pd: read fifo status failed err={}",
                            fusb302_error_kind(&err)
                        );
                        self.partial_rx_started_at_ms = None;
                        break;
                    }
                }
            }
        }
    }

    pub(super) fn handle_message(
        &mut self,
        message: Message,
        demand: UsbPdPowerDemand,
        now_ms: u32,
    ) {
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
                    self.no_contract_phase_started_at_ms = None;
                    self.no_contract_recovery_phase = None;
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
                    self.no_contract_phase_started_at_ms = None;
                    self.no_contract_recovery_phase = None;
                    self.state.input_current_limit_ma = contract.input_current_limit_ma;
                    self.state.vindpm_mv = contract.vindpm_mv;
                    esp_println::println!(
                        "usb_pd: contract active kind={} voltage_mv={} current_ma={}",
                        contract_kind_name(contract.kind),
                        contract.voltage_mv,
                        contract.current_ma
                    );
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

    pub(super) fn send_contract_request(
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
        esp_println::println!(
            "usb_pd: tx data kind=request spec_rev_bits={} peer_spec_rev_bits={} msg_id={} obj_pos={} voltage_mv={} current_ma={}",
            spec_revision.bits(),
            self.peer_spec_revision.bits(),
            self.message_id,
            plan.contract.object_position,
            plan.contract.voltage_mv,
            plan.contract.current_ma
        );
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

    pub(super) fn send_control_message(
        &mut self,
        kind: ControlMessageType,
        spec_revision: SpecRevision,
    ) -> Result<(), fusb302::Error> {
        let message_id = if matches!(kind, ControlMessageType::SoftReset) {
            0
        } else {
            self.message_id
        };
        let header = MessageHeader::for_control(kind, message_id, spec_revision, false, false);
        let message = Message::new(header, [0u32; pd::MAX_DATA_OBJECTS]);
        info!(
            "usb_pd: tx ctrl kind={} spec_rev_bits={=u8} peer_spec_rev_bits={=u8} msg_id={=u8}",
            control_message_name(kind),
            spec_revision.bits(),
            self.peer_spec_revision.bits(),
            message_id
        );
        self.phy.send_message(&message)?;
        if matches!(kind, ControlMessageType::GetSourceCap) {
            self.note_recovery_event(UsbPdRecoveryEvent::GetSourceCapSent);
        } else if matches!(kind, ControlMessageType::SoftReset) {
            self.note_recovery_event(UsbPdRecoveryEvent::SoftResetSent);
        }
        if matches!(kind, ControlMessageType::SoftReset) {
            self.message_id = 0;
        } else {
            self.message_id = (self.message_id + 1) & 0x07;
        }
        Ok(())
    }

    pub(super) fn clear_contract_tracking(&mut self) {
        self.contract_tracker.clear_all();
        self.state.contract = None;
        self.state.input_current_limit_ma = None;
        self.state.vindpm_mv = None;
    }

    pub(super) fn rearm_after_detach(&mut self, now_ms: u32, reason: &'static str) {
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

    pub(super) fn apply_default_5v_input_limits(
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

    pub(super) fn teardown_controller_state_on_phy_error(&mut self) {
        self.clear_contract_tracking();
        self.disarm_charge_ready("controller_error");
        self.source_capabilities = None;
        self.message_id = 0;
        self.consecutive_cc_absent_polls = 0;
        self.consecutive_raw_vbus_absent_polls = 0;
        self.consecutive_effective_vbus_absent_polls = 0;
        self.attached_at_ms = None;
        self.no_contract_phase_started_at_ms = None;
        self.no_contract_recovery_phase = None;
        self.source_caps_recovery_attempted = false;
        self.inherited_source_caps_probe_pending = false;
        self.last_source_caps_requery_at_ms = None;
        self.last_source_caps_recovery_at_ms = None;
        self.tx_spec_revision = pd::FUSB302_MAX_SPEC_REVISION;
        self.peer_spec_revision = pd::FUSB302_MAX_SPEC_REVISION;
        self.state.attached = false;
        self.state.vbus_present = None;
        self.state.polarity = None;
        self.unsafe_hard_reset_sent = false;
        self.boot_unattached_candidate_since_ms = None;
    }

    pub(super) fn arm_charge_ready_after(
        &mut self,
        now_ms: u32,
        delay_ms: u32,
        reason: &'static str,
    ) {
        self.state.charge_ready = false;
        self.charge_ready_at_ms = Some(now_ms.wrapping_add(delay_ms));
        info!(
            "usb_pd: charge gate arm reason={} delay_ms={=u32}",
            reason, delay_ms
        );
    }

    pub(super) fn disarm_charge_ready(&mut self, reason: &'static str) {
        if self.state.charge_ready || self.charge_ready_at_ms.is_some() {
            info!("usb_pd: charge gate hold reason={}", reason);
        }
        self.state.charge_ready = false;
        self.charge_ready_at_ms = None;
    }

    pub(super) fn arm_default_5v_charge_ready(&mut self, now_ms: u32, reason: &'static str) {
        self.arm_charge_ready_after(now_ms, DEFAULT_5V_CHARGE_READY_DELAY_MS, reason);
    }

    pub(super) fn update_charge_ready_state(&mut self, now_ms: u32) {
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

    pub(super) fn observe_peer_spec_revision(
        &mut self,
        spec_revision: SpecRevision,
        context: &'static str,
    ) {
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

    pub(super) fn handle_peer_soft_reset(&mut self, spec_revision: SpecRevision, now_ms: u32) {
        warn!("usb_pd: source requested soft reset");
        self.observe_peer_spec_revision(spec_revision, "soft_reset");
        self.reset_contract_state(false);
        self.attached_at_ms = Some(now_ms);
        self.no_contract_phase_started_at_ms = Some(now_ms);
        self.no_contract_recovery_phase = Some(NoContractRecoveryPhase::HardResetWaitCaps);
        if let Err(err) =
            self.send_control_message(ControlMessageType::Accept, self.tx_spec_revision)
        {
            warn!(
                "usb_pd: soft reset accept failed err={}",
                fusb302_error_kind(&err)
            );
        }
    }

    pub(super) fn reset_contract_state(&mut self, detach: bool) {
        self.clear_contract_tracking();
        self.disarm_charge_ready(if detach { "detach" } else { "reset" });
        self.source_capabilities = None;
        self.message_id = 0;
        self.consecutive_cc_absent_polls = 0;
        self.consecutive_effective_vbus_absent_polls = 0;
        self.consecutive_raw_vbus_absent_polls = 0;
        self.attached_at_ms = None;
        self.no_contract_phase_started_at_ms = None;
        self.no_contract_recovery_phase = None;
        self.source_caps_recovery_attempted = false;
        self.inherited_source_caps_probe_pending = false;
        self.last_source_caps_requery_at_ms = None;
        self.last_source_caps_recovery_at_ms = None;
        self.partial_rx_started_at_ms = None;
        self.boot_unattached_candidate_since_ms = None;
        if detach {
            self.mark_unattached_observed();
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
