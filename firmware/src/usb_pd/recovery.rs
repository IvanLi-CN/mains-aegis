use super::*;
impl<I2C> UsbPdSinkManager<I2C>
where
    I2C: embedded_hal::i2c::I2c<Error = esp_hal::i2c::master::Error>,
{
    pub(super) fn maybe_recover_stalled_contract_request(&mut self, now_ms: u32) {
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

    pub(super) fn maybe_request_contract_from_cached_source_caps(
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

    pub(super) fn phy_poll_interval_ms(&self) -> u32 {
        if self.state.attached && self.state.contract.is_none() {
            PHY_NEGOTIATION_POLL_INTERVAL_MS
        } else {
            PHY_POLL_INTERVAL_MS
        }
    }

    pub(super) fn in_no_contract_hard_reset_wait(&self) -> bool {
        matches!(
            self.no_contract_recovery_phase,
            Some(NoContractRecoveryPhase::HardResetWaitCaps)
        )
    }

    pub(super) fn in_no_contract_hard_reset_sent(&self) -> bool {
        matches!(
            self.no_contract_recovery_phase,
            Some(NoContractRecoveryPhase::HardResetSent)
        )
    }

    pub(super) fn enter_no_contract_hard_reset_sent(&mut self, now_ms: u32) {
        self.no_contract_phase_started_at_ms = Some(now_ms);
        self.no_contract_recovery_phase = Some(NoContractRecoveryPhase::HardResetSent);
        self.attached_at_ms = Some(now_ms);
        self.source_caps_recovery_attempted = false;
        self.last_source_caps_requery_at_ms = None;
        self.last_source_caps_recovery_at_ms = None;
        self.disarm_charge_ready("hard_reset_sent");
    }

    pub(super) fn enter_no_contract_hard_reset_wait(&mut self, now_ms: u32) {
        if !self.in_no_contract_hard_reset_wait() {
            self.no_contract_phase_started_at_ms = Some(now_ms);
        }
        self.no_contract_recovery_phase = Some(NoContractRecoveryPhase::HardResetWaitCaps);
        self.attached_at_ms = Some(now_ms);
        self.source_caps_recovery_attempted = false;
        self.last_source_caps_requery_at_ms = None;
        self.last_source_caps_recovery_at_ms = None;
        self.disarm_charge_ready("hard_reset_wait");
    }

    pub(super) fn restart_no_contract_wait_for_caps(
        &mut self,
        now_ms: u32,
        reason: &'static str,
        preserve_wait_started_at_ms: Option<u32>,
    ) {
        self.reset_contract_state(false);
        self.enter_no_contract_hard_reset_wait(now_ms);
        if let Some(started_at_ms) = preserve_wait_started_at_ms {
            self.no_contract_phase_started_at_ms = Some(started_at_ms);
        }

        info!(
            "usb_pd: restart_wait_for_caps reason={} tx_spec_rev_bits={=u8} peer_spec_rev_bits={=u8}",
            reason,
            self.tx_spec_revision.bits(),
            self.peer_spec_revision.bits()
        );

        if let Err(err) = self.phy.reset_pd_logic() {
            warn!(
                "usb_pd: pd_reset failed reason={} err={}",
                reason,
                fusb302_error_kind(&err)
            );
        } else if let Some(polarity) = self.state.polarity {
            if let Err(err) = self
                .phy
                .enable_pd_receive(polarity, self.recovery_spec_revision())
            {
                warn!(
                    "usb_pd: rx re-enable failed reason={} err={}",
                    reason,
                    fusb302_error_kind(&err)
                );
            }
        }
    }

    pub(super) fn begin_no_contract_hard_reset_recovery(
        &mut self,
        now_ms: u32,
        reason: &'static str,
    ) {
        self.reset_contract_state(false);
        self.enter_no_contract_hard_reset_sent(now_ms);
        info!(
            "usb_pd: begin_hard_reset_recovery reason={} tx_spec_rev_bits={=u8} peer_spec_rev_bits={=u8}",
            reason,
            self.tx_spec_revision.bits(),
            self.peer_spec_revision.bits()
        );

        if let Err(err) = self.phy.send_hard_reset() {
            warn!(
                "usb_pd: hard reset send failed reason={} err={}",
                reason,
                fusb302_error_kind(&err)
            );
            self.restart_no_contract_wait_for_caps(now_ms, "hard_reset_send_failed", None);
        } else {
            self.note_recovery_event(UsbPdRecoveryEvent::HardResetSent);
        }
    }

    pub(super) fn active_no_contract_recovery_allowed(&self) -> bool {
        self.observed_unattached_since_boot
    }

    pub(super) fn mark_unattached_observed(&mut self) {
        self.observed_unattached_since_boot = true;
    }

    pub(super) fn observe_boot_unattached_candidate(&mut self, physically_absent: bool) {
        if !self.observed_unattached_since_boot && physically_absent {
            self.mark_unattached_observed();
        }
    }

    pub(super) fn note_recovery_event(&mut self, event: UsbPdRecoveryEvent) {
        self.state.recovery_event = Some(event);
        self.state.recovery_event_counter = self.state.recovery_event_counter.wrapping_add(1);
    }

    pub(super) fn enter_passive_no_contract_wait(&mut self, now_ms: u32, reason: &'static str) {
        self.reset_contract_state(false);
        self.attached_at_ms = Some(now_ms);
        self.no_contract_phase_started_at_ms = Some(now_ms);
        self.no_contract_recovery_phase = Some(NoContractRecoveryPhase::FreshAttach);
        self.source_caps_recovery_attempted = false;
        self.last_source_caps_requery_at_ms = None;
        self.last_source_caps_recovery_at_ms = None;
        info!(
            "usb_pd: passive_wait_for_caps reason={} waiting_for_replug={=bool}",
            reason,
            !self.active_no_contract_recovery_allowed()
        );
    }

    pub(super) fn maybe_recover_missing_source_caps(
        &mut self,
        _demand: UsbPdPowerDemand,
        now_ms: u32,
    ) {
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
        if let Some(partial_rx_started_at_ms) = self.partial_rx_started_at_ms {
            let partial_rx_age_ms = now_ms.wrapping_sub(partial_rx_started_at_ms);
            if partial_rx_age_ms < PARTIAL_RX_RECOVERY_GRACE_MS {
                debug!(
                    "usb_pd: defer source caps recovery while partial rx is active age_ms={=u32}",
                    partial_rx_age_ms
                );
                return;
            }
        }
        if self.in_no_contract_hard_reset_sent() {
            let hard_reset_sent_at_ms = self
                .no_contract_phase_started_at_ms
                .unwrap_or(attached_at_ms);
            let sent_waited_ms = now_ms.wrapping_sub(hard_reset_sent_at_ms);
            if sent_waited_ms < HARD_RESET_SEND_SETTLE_MS {
                return;
            }

            warn!(
                "usb_pd: hard reset send settle elapsed without irq waited_ms={=u32}",
                sent_waited_ms
            );
            self.restart_no_contract_wait_for_caps(now_ms, "hard_reset_send_settle", None);
            return;
        }

        if self.in_no_contract_hard_reset_wait() {
            let hard_reset_wait_started_at_ms = self
                .no_contract_phase_started_at_ms
                .unwrap_or(attached_at_ms);
            let hard_reset_waited_ms = now_ms.wrapping_sub(hard_reset_wait_started_at_ms);
            if hard_reset_waited_ms < HARD_RESET_WAIT_FOR_SOURCE_CAPS_MS {
                return;
            }

            warn!(
                "usb_pd: hard reset recovery timed out waiting for source caps waited_ms={=u32}",
                hard_reset_waited_ms
            );
            self.rearm_after_detach(now_ms, "hard_reset_wait_timeout");
            return;
        }

        if waited_ms < SOURCE_CAPS_WAIT_TIMEOUT_MS {
            return;
        }

        if !self.active_no_contract_recovery_allowed() {
            let last_attempt_age_ms = self
                .last_source_caps_recovery_at_ms
                .map(|last| now_ms.wrapping_sub(last));

            if self.last_source_caps_recovery_at_ms.is_none() {
                esp_println::println!(
                    "usb_pd: no source caps after inherited attach, inhibiting hard reset waited_ms={} tx_spec_rev_bits={}",
                    waited_ms,
                    self.tx_spec_revision.bits()
                );
                warn!(
                    "usb_pd: inherited attach missing source caps, suppress hard reset waited_ms={=u32} tx_spec_rev_bits={=u8}",
                    waited_ms,
                    self.tx_spec_revision.bits()
                );
                self.note_recovery_event(UsbPdRecoveryEvent::HardResetInhibited);
                self.inherited_source_caps_probe_pending = true;
                self.last_source_caps_recovery_at_ms = Some(now_ms);
                return;
            }

            if self.inherited_source_caps_probe_pending {
                esp_println::println!(
                    "usb_pd: inherited attach requesting source caps waited_ms={} tx_spec_rev_bits={}",
                    waited_ms,
                    self.tx_spec_revision.bits()
                );
                match self.send_control_message(
                    ControlMessageType::GetSourceCap,
                    self.recovery_spec_revision(),
                ) {
                    Ok(()) => {
                        self.inherited_source_caps_probe_pending = false;
                        self.last_source_caps_recovery_at_ms = Some(now_ms);
                    }
                    Err(err) => {
                        warn!(
                            "usb_pd: inherited get_source_cap failed err={}",
                            fusb302_error_kind(&err)
                        );
                    }
                }
                return;
            }

            if !self.source_caps_recovery_attempted
                && last_attempt_age_ms.is_some_and(|age| age >= SOURCE_CAPS_REQUERY_DELAY_MS)
            {
                warn!(
                    "usb_pd: inherited attach still missing source caps, sending soft reset waited_ms={=u32} tx_spec_rev_bits={=u8}",
                    waited_ms,
                    self.tx_spec_revision.bits()
                );
                match self.send_control_message(
                    ControlMessageType::SoftReset,
                    self.recovery_spec_revision(),
                ) {
                    Ok(()) => {
                        self.source_caps_recovery_attempted = true;
                        self.last_source_caps_recovery_at_ms = Some(now_ms);
                    }
                    Err(err) => {
                        warn!(
                            "usb_pd: inherited soft reset failed err={}",
                            fusb302_error_kind(&err)
                        );
                    }
                }
                return;
            }

            if self.source_caps_recovery_attempted {
                if self.state.recovery_event != Some(UsbPdRecoveryEvent::HardResetInhibited) {
                    self.note_recovery_event(UsbPdRecoveryEvent::HardResetInhibited);
                }
                esp_println::println!(
                    "usb_pd: no source caps after inherited attach, holding 5v until replug waited_ms={} tx_spec_rev_bits={}",
                    waited_ms,
                    self.tx_spec_revision.bits()
                );
                warn!(
                    "usb_pd: no source caps after inherited attach, suppress hard reset until replug waited_ms={=u32} tx_spec_rev_bits={=u8}",
                    waited_ms,
                    self.tx_spec_revision.bits()
                );
            }
            return;
        }

        esp_println::println!(
            "usb_pd: no source caps after attach, issuing hard reset waited_ms={} tx_spec_rev_bits={}",
            waited_ms,
            self.tx_spec_revision.bits()
        );
        info!(
            "usb_pd: no source caps after attach, issuing hard reset waited_ms={=u32} tx_spec_rev_bits={=u8}",
            waited_ms,
            self.tx_spec_revision.bits()
        );
        self.begin_no_contract_hard_reset_recovery(now_ms, "source_caps_timeout");
    }

    pub(super) fn no_contract_attach_stabilizing(&self, now_ms: u32) -> bool {
        if !self.state.attached || self.state.contract.is_some() {
            return false;
        }

        let Some(attached_at_ms) = self.attached_at_ms else {
            return false;
        };

        matches!(
            self.no_contract_recovery_phase,
            Some(NoContractRecoveryPhase::FreshAttach)
        ) && now_ms.wrapping_sub(attached_at_ms) < NO_CONTRACT_ATTACH_STABILIZE_MS
    }

    pub(super) fn maybe_recover_stalled_no_contract_with_cached_caps(&mut self, now_ms: u32) {
        if !self.state.attached
            || self.state.unsafe_source_latched
            || self.state.contract.is_some()
            || self.source_capabilities.is_none()
            || self.contract_tracker.request_in_flight()
            || self.in_no_contract_hard_reset_wait()
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

    pub(super) fn source_caps_requery_due(&self, now_ms: u32) -> bool {
        if let Some(last_requery_at_ms) = self.last_source_caps_requery_at_ms {
            now_ms.wrapping_sub(last_requery_at_ms) >= SOURCE_CAPS_REQUERY_RETRY_MS
        } else {
            now_ms.wrapping_sub(self.last_request_at_ms) >= SOURCE_CAPS_REQUERY_DELAY_MS
        }
    }

    pub(super) fn recovery_spec_revision(&self) -> SpecRevision {
        if self.source_capabilities.is_none() && self.state.contract.is_none() {
            pd::FUSB302_MAX_SPEC_REVISION
        } else {
            self.tx_spec_revision
        }
    }

    pub(super) fn maybe_arm_default_5v_charge_ready(&mut self, now_ms: u32) {
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
}
