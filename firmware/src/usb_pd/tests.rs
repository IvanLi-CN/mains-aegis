use super::*;
use std::vec::Vec;

struct NoopI2c;
struct LenientI2c;
#[derive(Default)]
struct RecordingI2c {
    writes: Vec<Vec<u8>>,
}

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

impl embedded_hal::i2c::ErrorType for RecordingI2c {
    type Error = esp_hal::i2c::master::Error;
}

impl embedded_hal::i2c::I2c for RecordingI2c {
    fn transaction(
        &mut self,
        _address: u8,
        operations: &mut [embedded_hal::i2c::Operation<'_>],
    ) -> Result<(), Self::Error> {
        for operation in operations {
            match operation {
                embedded_hal::i2c::Operation::Read(buffer) => buffer.fill(0),
                embedded_hal::i2c::Operation::Write(bytes) => self.writes.push(bytes.to_vec()),
            }
        }
        Ok(())
    }
}

impl RecordingI2c {
    fn reset_write_count(&self) -> usize {
        self.writes
            .iter()
            .filter(|write| write.as_slice() == [fusb302::reg::RESET, fusb302::reset::SW_RESET])
            .count()
    }

    fn pd_reset_write_count(&self) -> usize {
        self.writes
            .iter()
            .filter(|write| write.as_slice() == [fusb302::reg::RESET, fusb302::reset::PD_RESET])
            .count()
    }

    fn hard_reset_tx_count(&self) -> usize {
        self.writes
            .iter()
            .filter(|write| {
                write.first() == Some(&fusb302::reg::FIFOS)
                    && write.get(1..)
                        == Some(
                            [
                                0x15, // TOKEN_RESET1
                                0x15, // TOKEN_RESET1
                                0x15, // TOKEN_RESET1
                                0x16, // TOKEN_RESET2
                                0xA1, // TOKEN_TX_ON
                            ]
                            .as_slice(),
                        )
            })
            .count()
    }

    fn get_source_cap_tx_count(&self) -> usize {
        self.writes
            .iter()
            .filter(|write| {
                write.first() == Some(&fusb302::reg::FIFOS)
                    && write.len() > 7
                    && write[6] & 0x0f == ControlMessageType::GetSourceCap as u8
            })
            .count()
    }

    fn soft_reset_tx_count(&self) -> usize {
        self.writes
            .iter()
            .filter(|write| {
                write.first() == Some(&fusb302::reg::FIFOS)
                    && write.len() > 7
                    && write[6] & 0x0f == ControlMessageType::SoftReset as u8
            })
            .count()
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

const fn irq_snapshot_with_status(status1a: u8, status0: u8) -> IrqSnapshot {
    IrqSnapshot {
        status0a: 0,
        status1a,
        interrupta: 0,
        interruptb: 0,
        status0,
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

const fn irq_snapshot_with_hard_reset(status1a: u8, vbus_ok: bool) -> IrqSnapshot {
    IrqSnapshot {
        status0a: 0,
        status1a,
        interrupta: fusb302::interrupta::HARD_RESET,
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

const fn irq_snapshot_with_partial_rx_hard_reset(status1a: u8, vbus_ok: bool) -> IrqSnapshot {
    IrqSnapshot {
        status0a: 0,
        status1a,
        interrupta: fusb302::interrupta::HARD_RESET,
        interruptb: fusb302::interruptb::GCRC_SENT,
        status0: if vbus_ok {
            fusb302::status0::VBUS_OK
        } else {
            0
        },
        status1: 0, // RX_EMPTY clear => fifo non-empty
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
fn required_power_includes_charge_power_in_mw_when_enabled() {
    let demand = UsbPdPowerDemand {
        requested_charge_voltage_mv: 16_800,
        requested_charge_current_ma: 500,
        system_load_power_mw: 2_500,
        battery_voltage_mv: Some(15_000),
        measured_input_voltage_mv: None,
        charging_enabled: true,
    };

    assert_eq!(demand.required_power_mw(), 10_900);
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
fn attached_session_detaches_on_sustained_cc_absent_with_vbus_still_present() {
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

    let cc_absent_but_vbus_high = fusb302::status0::VBUS_OK | 0; // BC_LVL=00, ACTIVITY=0
    manager.handle_irq_snapshot(
        irq_snapshot_with_status(fusb302::status1a::TOGS_OFF, cc_absent_but_vbus_high),
        UsbPdPowerDemand {
            measured_input_voltage_mv: Some(5_100),
            ..UsbPdPowerDemand::default()
        },
        1_000,
    );

    assert!(manager.state.attached);
    assert_eq!(manager.consecutive_cc_absent_polls, 1);
    assert_eq!(
        manager.state.contract.map(|contract| contract.kind),
        Some(ContractKind::Pps)
    );

    manager.handle_irq_snapshot(
        irq_snapshot_with_status(fusb302::status1a::TOGS_OFF, cc_absent_but_vbus_high),
        UsbPdPowerDemand {
            measured_input_voltage_mv: Some(5_100),
            ..UsbPdPowerDemand::default()
        },
        1_250,
    );

    assert!(!manager.state.attached);
    assert_eq!(manager.state.vbus_present, Some(false));
    assert!(manager.state.contract.is_none());
    assert_eq!(manager.state.polarity, None);
}

#[test]
fn attached_session_does_not_count_cc_absence_while_line_is_active() {
    let mut manager = UsbPdSinkManager::new(LenientI2c);
    manager.state.attached = true;
    manager.state.polarity = Some(CcPolarity::Cc1);
    manager.state.vbus_present = Some(true);
    manager.attached_at_ms = Some(500);

    let cc_activity_with_unknown_level = fusb302::status0::VBUS_OK | fusb302::status0::ACTIVITY;
    manager.handle_irq_snapshot(
        irq_snapshot_with_status(fusb302::status1a::TOGS_OFF, cc_activity_with_unknown_level),
        UsbPdPowerDemand {
            measured_input_voltage_mv: Some(5_100),
            ..UsbPdPowerDemand::default()
        },
        1_000,
    );

    assert!(manager.state.attached);
    assert_eq!(manager.consecutive_cc_absent_polls, 0);
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

#[test]
fn inherited_attach_timeout_holds_5v_and_skips_hard_reset_until_replug() {
    let mut manager = UsbPdSinkManager::new(RecordingI2c::default());
    manager.state.attached = true;
    manager.state.enabled = true;
    manager.state.controller_ready = true;
    manager.state.vbus_present = Some(true);
    manager.state.polarity = Some(CcPolarity::Cc1);
    manager.attached_at_ms = Some(0);

    manager.maybe_recover_missing_source_caps(
        UsbPdPowerDemand::default(),
        SOURCE_CAPS_WAIT_TIMEOUT_MS,
    );

    assert!(manager.state.attached);
    assert!(!manager.in_no_contract_hard_reset_sent());
    assert!(!manager.in_no_contract_hard_reset_wait());
    assert!(!manager.source_caps_recovery_attempted);
    assert!(manager.inherited_source_caps_probe_pending);
    assert_eq!(
        manager.state.recovery_event,
        Some(UsbPdRecoveryEvent::HardResetInhibited)
    );

    manager.maybe_recover_missing_source_caps(
        UsbPdPowerDemand::default(),
        SOURCE_CAPS_WAIT_TIMEOUT_MS + 1,
    );

    assert!(!manager.inherited_source_caps_probe_pending);
    assert_eq!(
        manager.state.recovery_event,
        Some(UsbPdRecoveryEvent::GetSourceCapSent)
    );

    manager.maybe_recover_missing_source_caps(
        UsbPdPowerDemand::default(),
        SOURCE_CAPS_WAIT_TIMEOUT_MS + SOURCE_CAPS_REQUERY_DELAY_MS + 1,
    );

    assert!(manager.source_caps_recovery_attempted);
    assert_eq!(
        manager.state.recovery_event,
        Some(UsbPdRecoveryEvent::SoftResetSent)
    );

    manager.maybe_recover_missing_source_caps(
        UsbPdPowerDemand::default(),
        SOURCE_CAPS_WAIT_TIMEOUT_MS + SOURCE_CAPS_REQUERY_DELAY_MS + 2,
    );
    assert_eq!(
        manager.state.recovery_event,
        Some(UsbPdRecoveryEvent::HardResetInhibited)
    );

    manager.maybe_arm_default_5v_charge_ready(
        SOURCE_CAPS_WAIT_TIMEOUT_MS + SOURCE_CAPS_REQUERY_DELAY_MS,
    );
    assert_eq!(
        manager.charge_ready_at_ms,
        Some(
            SOURCE_CAPS_WAIT_TIMEOUT_MS
                + SOURCE_CAPS_REQUERY_DELAY_MS
                + DEFAULT_5V_CHARGE_READY_DELAY_MS
        )
    );

    let i2c = manager.phy.release_i2c();
    assert_eq!(i2c.hard_reset_tx_count(), 0);
    assert_eq!(i2c.get_source_cap_tx_count(), 1);
    assert_eq!(i2c.soft_reset_tx_count(), 1);
}

#[test]
fn first_attach_after_observed_detach_can_still_issue_hard_reset_recovery() {
    let mut manager = UsbPdSinkManager::new(RecordingI2c::default());
    manager.mark_unattached_observed();
    manager.state.attached = true;
    manager.state.enabled = true;
    manager.state.controller_ready = true;
    manager.state.vbus_present = Some(true);
    manager.state.polarity = Some(CcPolarity::Cc1);
    manager.attached_at_ms = Some(0);

    manager.maybe_recover_missing_source_caps(
        UsbPdPowerDemand::default(),
        SOURCE_CAPS_WAIT_TIMEOUT_MS,
    );

    assert!(manager.state.attached);
    assert!(manager.in_no_contract_hard_reset_sent());
    assert!(!manager.source_caps_recovery_attempted);
    assert_eq!(
        manager.state.recovery_event,
        Some(UsbPdRecoveryEvent::HardResetSent)
    );

    let i2c = manager.phy.release_i2c();
    assert_eq!(i2c.hard_reset_tx_count(), 1);
}

#[test]
fn boot_unattached_must_be_stable_before_active_recovery_is_allowed() {
    let mut manager = UsbPdSinkManager::new(NoopI2c);

    manager.observe_boot_unattached_candidate(true, 1_000);
    assert!(!manager.active_no_contract_recovery_allowed());

    manager.observe_boot_unattached_candidate(false, 1_500);
    assert!(!manager.active_no_contract_recovery_allowed());

    manager.observe_boot_unattached_candidate(true, 2_000);
    manager.observe_boot_unattached_candidate(true, 2_000 + BOOT_UNATTACHED_STABLE_MS);
    assert!(manager.active_no_contract_recovery_allowed());
}

#[test]
fn inherited_attach_resume_does_not_repeat_full_fusb_reset() {
    let mut manager = UsbPdSinkManager::new(RecordingI2c::default());
    manager.initialized = true;
    manager.state.enabled = true;
    manager.state.controller_ready = true;

    manager.handle_irq_snapshot(
        irq_snapshot_with_cc_and_vbus(fusb302::status1a::TOGS_SNK1, true),
        UsbPdPowerDemand {
            measured_input_voltage_mv: Some(5_100),
            ..UsbPdPowerDemand::default()
        },
        2_000,
    );

    assert!(manager.state.attached);
    assert_eq!(
        manager.state.recovery_event,
        Some(UsbPdRecoveryEvent::BootInheritedAttach)
    );

    let i2c = manager.phy.release_i2c();
    assert_eq!(i2c.reset_write_count(), 0);
    assert_eq!(i2c.pd_reset_write_count(), 0);
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
fn attached_state_ignores_missing_togs_when_vbus_is_present() {
    let mut manager = UsbPdSinkManager::new(LenientI2c);
    manager.state.attached = true;
    manager.state.vbus_present = Some(true);
    manager.attached_at_ms = Some(1_000);

    for now_ms in [1_100, 1_200, 1_300, 1_400] {
        manager.handle_irq_snapshot(
            irq_snapshot_with_cc_and_vbus(fusb302::status1a::TOGS_NONE, true),
            UsbPdPowerDemand {
                measured_input_voltage_mv: Some(5_100),
                ..UsbPdPowerDemand::default()
            },
            now_ms,
        );
    }

    assert!(manager.state.attached);
    assert_eq!(manager.state.vbus_present, Some(true));
}

#[test]
fn detach_reset_clears_cc_absent_debounce() {
    let mut manager = UsbPdSinkManager::new(LenientI2c);
    manager.consecutive_cc_absent_polls = 7;
    manager.reset_contract_state(true);
    assert_eq!(manager.consecutive_cc_absent_polls, 0);
}

#[test]
fn hard_reset_without_contract_enters_wait_and_keeps_attach() {
    let mut manager = UsbPdSinkManager::new(LenientI2c);
    manager.initialized = true;
    manager.state.enabled = true;
    manager.state.controller_ready = true;
    manager.state.attached = true;
    manager.state.vbus_present = Some(true);
    manager.state.polarity = Some(CcPolarity::Cc1);
    manager.attached_at_ms = Some(0);

    manager.handle_irq_snapshot(
        irq_snapshot_with_hard_reset(fusb302::status1a::TOGS_SNK1, true),
        UsbPdPowerDemand {
            measured_input_voltage_mv: Some(5_100),
            ..UsbPdPowerDemand::default()
        },
        1_000,
    );

    assert!(manager.state.attached);
    assert_eq!(manager.state.vbus_present, Some(true));
    assert!(manager.initialized);
    assert!(manager.state.controller_ready);
    assert_eq!(manager.attached_at_ms, Some(1_000));
    assert!(manager.in_no_contract_hard_reset_wait());
}

#[test]
fn repeated_hard_resets_do_not_extend_wait_deadline() {
    let mut manager = UsbPdSinkManager::new(LenientI2c);
    manager.initialized = true;
    manager.state.enabled = true;
    manager.state.controller_ready = true;
    manager.state.attached = true;
    manager.state.vbus_present = Some(true);
    manager.state.polarity = Some(CcPolarity::Cc1);
    manager.attached_at_ms = Some(0);

    manager.handle_irq_snapshot(
        irq_snapshot_with_hard_reset(fusb302::status1a::TOGS_SNK1, true),
        UsbPdPowerDemand {
            measured_input_voltage_mv: Some(5_100),
            ..UsbPdPowerDemand::default()
        },
        1_000,
    );
    let first_wait_started_at_ms = manager.no_contract_phase_started_at_ms;
    assert_eq!(first_wait_started_at_ms, Some(1_000));
    assert!(manager.in_no_contract_hard_reset_wait());

    manager.handle_irq_snapshot(
        irq_snapshot_with_hard_reset(fusb302::status1a::TOGS_SNK1, true),
        UsbPdPowerDemand {
            measured_input_voltage_mv: Some(5_100),
            ..UsbPdPowerDemand::default()
        },
        1_250,
    );

    assert_eq!(
        manager.no_contract_phase_started_at_ms,
        first_wait_started_at_ms
    );
    assert!(manager.in_no_contract_hard_reset_wait());
}

#[test]
fn partial_rx_hard_reset_does_not_force_immediate_recovery() {
    let mut manager = UsbPdSinkManager::new(LenientI2c);
    manager.initialized = true;
    manager.state.enabled = true;
    manager.state.controller_ready = true;
    manager.state.attached = true;
    manager.state.vbus_present = Some(true);
    manager.state.polarity = Some(CcPolarity::Cc1);
    manager.attached_at_ms = Some(0);

    manager.handle_irq_snapshot(
        irq_snapshot_with_partial_rx_hard_reset(fusb302::status1a::TOGS_SNK1, true),
        UsbPdPowerDemand {
            measured_input_voltage_mv: Some(5_100),
            ..UsbPdPowerDemand::default()
        },
        1_000,
    );

    assert!(manager.state.attached);
    assert_eq!(manager.state.vbus_present, Some(true));
    assert!(!manager.in_no_contract_hard_reset_wait());
    assert_eq!(manager.attached_at_ms, Some(0));
}

#[test]
fn retry_fail_without_contract_keeps_attach_and_restarts_recovery() {
    let mut manager = UsbPdSinkManager::new(LenientI2c);
    manager.initialized = true;
    manager.state.enabled = true;
    manager.state.controller_ready = true;
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

    assert!(manager.state.attached);
    assert_eq!(manager.state.vbus_present, Some(true));
    assert!(manager.initialized);
    assert!(manager.state.controller_ready);
    assert_eq!(manager.attached_at_ms, Some(0));
}

#[test]
fn hard_reset_wait_timeout_rearms_phy() {
    let mut manager = UsbPdSinkManager::new(NoopI2c);
    manager.state.attached = true;
    manager.state.vbus_present = Some(true);
    manager.attached_at_ms = Some(0);
    manager.no_contract_phase_started_at_ms = Some(0);
    manager.no_contract_recovery_phase = Some(NoContractRecoveryPhase::HardResetWaitCaps);

    manager.maybe_recover_missing_source_caps(
        UsbPdPowerDemand::default(),
        HARD_RESET_WAIT_FOR_SOURCE_CAPS_MS - 1,
    );
    assert!(manager.state.attached);

    manager.maybe_recover_missing_source_caps(
        UsbPdPowerDemand::default(),
        HARD_RESET_WAIT_FOR_SOURCE_CAPS_MS,
    );
    assert!(!manager.state.attached);
    assert_eq!(manager.attached_at_ms, None);
}

#[test]
fn hard_reset_wait_vbus_drop_does_not_detach_before_timeout() {
    let mut manager = UsbPdSinkManager::new(LenientI2c);
    manager.initialized = true;
    manager.state.enabled = true;
    manager.state.controller_ready = true;
    manager.state.attached = true;
    manager.state.vbus_present = Some(true);
    manager.attached_at_ms = Some(0);
    manager.no_contract_phase_started_at_ms = Some(0);
    manager.no_contract_recovery_phase = Some(NoContractRecoveryPhase::HardResetWaitCaps);

    manager.handle_irq_snapshot(
        irq_snapshot_with_cc_and_vbus(fusb302::status1a::TOGS_NONE, false),
        UsbPdPowerDemand {
            measured_input_voltage_mv: Some(2_950),
            ..UsbPdPowerDemand::default()
        },
        250,
    );
    assert!(manager.state.attached);

    manager.handle_irq_snapshot(
        irq_snapshot_with_cc_and_vbus(fusb302::status1a::TOGS_NONE, false),
        UsbPdPowerDemand {
            measured_input_voltage_mv: Some(2_950),
            ..UsbPdPowerDemand::default()
        },
        500,
    );

    assert!(manager.state.attached);
    assert_eq!(manager.attached_at_ms, Some(0));

    manager.maybe_recover_missing_source_caps(
        UsbPdPowerDemand::default(),
        HARD_RESET_WAIT_FOR_SOURCE_CAPS_MS - 1,
    );
    assert!(manager.state.attached);

    manager.maybe_recover_missing_source_caps(
        UsbPdPowerDemand::default(),
        HARD_RESET_WAIT_FOR_SOURCE_CAPS_MS,
    );
    assert!(!manager.state.attached);
    assert_eq!(manager.attached_at_ms, None);
}

#[test]
fn hard_reset_sent_vbus_drop_does_not_detach_before_settle_timeout() {
    let mut manager = UsbPdSinkManager::new(LenientI2c);
    manager.initialized = true;
    manager.state.enabled = true;
    manager.state.controller_ready = true;
    manager.state.attached = true;
    manager.state.vbus_present = Some(true);
    manager.state.polarity = Some(CcPolarity::Cc1);
    manager.attached_at_ms = Some(0);
    manager.no_contract_phase_started_at_ms = Some(0);
    manager.no_contract_recovery_phase = Some(NoContractRecoveryPhase::HardResetSent);

    manager.handle_irq_snapshot(
        irq_snapshot_with_cc_and_vbus(fusb302::status1a::TOGS_NONE, false),
        UsbPdPowerDemand {
            measured_input_voltage_mv: Some(2_950),
            ..UsbPdPowerDemand::default()
        },
        50,
    );
    assert!(manager.state.attached);
    assert_eq!(
        manager.no_contract_recovery_phase,
        Some(NoContractRecoveryPhase::HardResetSent)
    );

    manager
        .maybe_recover_missing_source_caps(UsbPdPowerDemand::default(), HARD_RESET_SEND_SETTLE_MS);

    assert!(manager.state.attached);
    assert_eq!(
        manager.no_contract_recovery_phase,
        Some(NoContractRecoveryPhase::HardResetWaitCaps)
    );
}

#[test]
fn recent_partial_rx_defers_missing_source_caps_recovery() {
    let mut manager = UsbPdSinkManager::new(NoopI2c);
    manager.state.attached = true;
    manager.state.vbus_present = Some(true);
    manager.attached_at_ms = Some(0);
    manager.partial_rx_started_at_ms = Some(SOURCE_CAPS_WAIT_TIMEOUT_MS);

    manager.maybe_recover_missing_source_caps(
        UsbPdPowerDemand::default(),
        SOURCE_CAPS_WAIT_TIMEOUT_MS + PARTIAL_RX_RECOVERY_GRACE_MS - 1,
    );

    assert!(manager.state.attached);
    assert!(!manager.in_no_contract_hard_reset_sent());

    manager.maybe_recover_missing_source_caps(
        UsbPdPowerDemand::default(),
        SOURCE_CAPS_WAIT_TIMEOUT_MS + PARTIAL_RX_RECOVERY_GRACE_MS,
    );

    assert!(manager.in_no_contract_hard_reset_sent());
}

#[test]
fn repeated_partial_rx_observations_do_not_extend_recovery_grace() {
    let mut manager = UsbPdSinkManager::new(LenientI2c);
    manager.state.attached = true;
    manager.state.vbus_present = Some(true);
    manager.attached_at_ms = Some(0);

    manager.handle_irq_snapshot(
        irq_snapshot_with_partial_rx_hard_reset(fusb302::status1a::TOGS_SNK2, true),
        UsbPdPowerDemand::default(),
        SOURCE_CAPS_WAIT_TIMEOUT_MS,
    );
    assert_eq!(
        manager.partial_rx_started_at_ms,
        Some(SOURCE_CAPS_WAIT_TIMEOUT_MS)
    );

    manager.handle_irq_snapshot(
        irq_snapshot_with_partial_rx_hard_reset(fusb302::status1a::TOGS_SNK2, true),
        UsbPdPowerDemand::default(),
        SOURCE_CAPS_WAIT_TIMEOUT_MS + 100,
    );
    assert_eq!(
        manager.partial_rx_started_at_ms,
        Some(SOURCE_CAPS_WAIT_TIMEOUT_MS)
    );

    manager.maybe_recover_missing_source_caps(
        UsbPdPowerDemand::default(),
        SOURCE_CAPS_WAIT_TIMEOUT_MS + PARTIAL_RX_RECOVERY_GRACE_MS,
    );

    assert!(manager.in_no_contract_hard_reset_sent());
}

fn source_caps_requery_uses_initial_delay_before_first_probe() {
    let mut manager = UsbPdSinkManager::new(NoopI2c);
    manager.last_request_at_ms = 0;

    assert!(!manager.source_caps_requery_due(SOURCE_CAPS_REQUERY_DELAY_MS - 1));
    assert!(manager.source_caps_requery_due(SOURCE_CAPS_REQUERY_DELAY_MS));
}

#[test]
fn negotiation_poll_interval_is_faster_while_attached_without_contract() {
    let mut manager = UsbPdSinkManager::new(NoopI2c);
    manager.state.attached = true;
    manager.state.contract = None;

    assert_eq!(
        manager.phy_poll_interval_ms(),
        PHY_NEGOTIATION_POLL_INTERVAL_MS
    );

    manager.state.contract = Some(ActiveContract {
        kind: ContractKind::Pps,
        object_position: 1,
        voltage_mv: 16_000,
        current_ma: 100,
        source_max_current_ma: 100,
        input_current_limit_ma: Some(100),
        vindpm_mv: Some(15_000),
    });

    assert_eq!(manager.phy_poll_interval_ms(), PHY_POLL_INTERVAL_MS);
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
            (0xC0000000u32 | ((5_000u32 / 100) << 17) | ((18_000u32 / 100) << 8) | (3_000u32 / 50)),
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
fn missing_source_caps_retry_rearms_when_charge_path_is_active() {
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

    assert!(!manager.state.attached);
    assert_eq!(manager.attached_at_ms, None);
    assert_eq!(manager.message_id, 0);
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

#[test]
fn soft_reset_uses_message_id_zero_and_resets_counter() {
    let mut manager = UsbPdSinkManager::new(NoopI2c);
    manager.message_id = 5;

    manager
        .send_control_message(ControlMessageType::SoftReset, SpecRevision::Rev30)
        .unwrap();

    assert_eq!(manager.message_id, 0);
}

#[test]
fn sticky_status0a_retry_fail_does_not_trigger_retry_event_without_interrupt() {
    let snapshot = IrqSnapshot {
        status0a: fusb302::status0a::RETRY_FAIL,
        status1a: 0,
        interrupta: 0,
        interruptb: 0,
        status0: 0,
        status1: 0,
        interrupt: 0,
    };

    assert!(!snapshot.retry_failed());
}

#[test]
fn retry_fail_is_deferred_when_rx_activity_is_present() {
    let snapshot = IrqSnapshot {
        status0a: 0,
        status1a: 0,
        interrupta: fusb302::interrupta::RETRY_FAIL | fusb302::interrupta::TX_SENT,
        interruptb: 0,
        status0: 0,
        status1: 0,
        interrupt: 0,
    };

    assert!(retry_fail_should_defer_for_rx(snapshot));
}
