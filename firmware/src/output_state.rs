#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputSelector {
    OutA,
    OutB,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnabledOutputs {
    None,
    Only(OutputSelector),
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputGateReason {
    None,
    BmsNotReady,
    ThermKill,
    TpsFault,
    ActiveProtection,
}

impl OutputGateReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            OutputGateReason::None => "none",
            OutputGateReason::BmsNotReady => "bms_not_ready",
            OutputGateReason::ThermKill => "therm_kill",
            OutputGateReason::TpsFault => "tps_fault",
            OutputGateReason::ActiveProtection => "active_protection",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputRuntimeState {
    pub requested_outputs: EnabledOutputs,
    pub active_outputs: EnabledOutputs,
    pub recoverable_outputs: EnabledOutputs,
    pub gate_reason: OutputGateReason,
}

impl OutputRuntimeState {
    pub const fn new(
        requested_outputs: EnabledOutputs,
        active_outputs: EnabledOutputs,
        recoverable_outputs: EnabledOutputs,
        gate_reason: OutputGateReason,
    ) -> Self {
        Self {
            requested_outputs,
            active_outputs,
            recoverable_outputs,
            gate_reason,
        }
    }
}

pub fn output_state_gate_transition(
    state: OutputRuntimeState,
    gate_reason: OutputGateReason,
) -> OutputRuntimeState {
    if gate_reason == OutputGateReason::None {
        return OutputRuntimeState {
            gate_reason: OutputGateReason::None,
            ..state
        };
    }

    if state.gate_reason == gate_reason && state.active_outputs == EnabledOutputs::None {
        return state;
    }

    let recoverable_outputs = if state.active_outputs != EnabledOutputs::None {
        state.active_outputs
    } else {
        state.recoverable_outputs
    };

    OutputRuntimeState {
        active_outputs: EnabledOutputs::None,
        recoverable_outputs,
        gate_reason,
        ..state
    }
}

pub fn output_restore_pending_from_state(
    state: OutputRuntimeState,
    mains_present: Option<bool>,
) -> bool {
    state.gate_reason == OutputGateReason::None
        && state.active_outputs == EnabledOutputs::None
        && state.recoverable_outputs != EnabledOutputs::None
        && mains_present == Some(true)
}

#[cfg(test)]
mod tests {
    use super::{
        output_restore_pending_from_state, output_state_gate_transition, EnabledOutputs,
        OutputGateReason, OutputRuntimeState, OutputSelector,
    };

    #[test]
    fn output_state_bms_block_without_vin_stays_blocked() {
        let state = OutputRuntimeState::new(
            EnabledOutputs::Only(OutputSelector::OutA),
            EnabledOutputs::Only(OutputSelector::OutA),
            EnabledOutputs::None,
            OutputGateReason::None,
        );

        let gated = output_state_gate_transition(state, OutputGateReason::BmsNotReady);

        assert_eq!(gated.active_outputs, EnabledOutputs::None);
        assert_eq!(
            gated.recoverable_outputs,
            EnabledOutputs::Only(OutputSelector::OutA)
        );
        assert_eq!(gated.gate_reason, OutputGateReason::BmsNotReady);
        assert!(!output_restore_pending_from_state(gated, Some(false)));
        assert!(!output_restore_pending_from_state(gated, None));
    }

    #[test]
    fn output_state_gate_cleared_with_vin_becomes_recoverable_not_enabled() {
        let state = OutputRuntimeState::new(
            EnabledOutputs::Only(OutputSelector::OutA),
            EnabledOutputs::None,
            EnabledOutputs::Only(OutputSelector::OutA),
            OutputGateReason::BmsNotReady,
        );

        let cleared = output_state_gate_transition(state, OutputGateReason::None);

        assert_eq!(cleared.active_outputs, EnabledOutputs::None);
        assert_eq!(
            cleared.recoverable_outputs,
            EnabledOutputs::Only(OutputSelector::OutA)
        );
        assert_eq!(cleared.gate_reason, OutputGateReason::None);
        assert!(output_restore_pending_from_state(cleared, Some(true)));
    }

    #[test]
    fn output_state_therm_kill_never_auto_restores() {
        let state = OutputRuntimeState::new(
            EnabledOutputs::Both,
            EnabledOutputs::Both,
            EnabledOutputs::None,
            OutputGateReason::None,
        );

        let gated = output_state_gate_transition(state, OutputGateReason::ThermKill);
        let cleared = output_state_gate_transition(gated, OutputGateReason::None);

        assert_eq!(cleared.active_outputs, EnabledOutputs::None);
        assert_eq!(cleared.recoverable_outputs, EnabledOutputs::Both);
        assert_eq!(cleared.gate_reason, OutputGateReason::None);
        assert!(output_restore_pending_from_state(cleared, Some(true)));
    }

    #[test]
    fn output_restore_pending_requires_vin_online_and_no_gate() {
        let state = OutputRuntimeState::new(
            EnabledOutputs::Only(OutputSelector::OutA),
            EnabledOutputs::None,
            EnabledOutputs::Only(OutputSelector::OutA),
            OutputGateReason::None,
        );

        assert!(!output_restore_pending_from_state(state, None));
        assert!(!output_restore_pending_from_state(state, Some(false)));
        assert!(output_restore_pending_from_state(state, Some(true)));

        let fault_gated = output_state_gate_transition(state, OutputGateReason::TpsFault);
        assert!(!output_restore_pending_from_state(fault_gated, Some(true)));
    }
}
