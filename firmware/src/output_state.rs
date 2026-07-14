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
    TpsConfigFailed,
    ActiveProtection,
    ManualBypass,
}

impl OutputGateReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            OutputGateReason::None => "none",
            OutputGateReason::BmsNotReady => "bms_not_ready",
            OutputGateReason::ThermKill => "therm_kill",
            OutputGateReason::TpsFault => "tps_fault",
            OutputGateReason::TpsConfigFailed => "tps_config_failed",
            OutputGateReason::ActiveProtection => "active_protection",
            OutputGateReason::ManualBypass => "manual_bypass",
        }
    }
}

pub const LOW_BATTERY_OUTPUT_RESTORE_RSOC_PCT: u16 = 20;
pub const PRE_TPS_UNDERVOLTAGE_REQUIRED_SAMPLES: u8 = 3;
pub const PRE_TPS_UNDERVOLTAGE_12V_CUTOFF_MV: u16 = 11_300;
pub const PRE_TPS_UNDERVOLTAGE_12V_RECOVER_MV: u16 = 11_500;
pub const PRE_TPS_UNDERVOLTAGE_LEGACY_CUTOFF_MV: u16 = 10_000;
pub const PRE_TPS_UNDERVOLTAGE_LEGACY_RECOVER_MV: u16 = 11_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputGateThresholds {
    pub cutoff_mv: u16,
    pub recover_mv: u16,
    pub required_samples: u8,
}

impl InputGateThresholds {
    pub const fn for_rated_vout(rated_vout_mv: u16) -> Self {
        if rated_vout_mv <= 12_000 {
            Self {
                cutoff_mv: PRE_TPS_UNDERVOLTAGE_12V_CUTOFF_MV,
                recover_mv: PRE_TPS_UNDERVOLTAGE_12V_RECOVER_MV,
                required_samples: PRE_TPS_UNDERVOLTAGE_REQUIRED_SAMPLES,
            }
        } else {
            Self {
                cutoff_mv: PRE_TPS_UNDERVOLTAGE_LEGACY_CUTOFF_MV,
                recover_mv: PRE_TPS_UNDERVOLTAGE_LEGACY_RECOVER_MV,
                required_samples: PRE_TPS_UNDERVOLTAGE_REQUIRED_SAMPLES,
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputGateTracker {
    pub cutoff: bool,
    thresholds: InputGateThresholds,
    low_streak: u8,
    recover_streak: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputGateAction {
    None,
    Cutoff,
    Enable,
}

impl InputGateTracker {
    pub const fn new() -> Self {
        Self::for_rated_vout(12_000)
    }

    pub const fn for_rated_vout(rated_vout_mv: u16) -> Self {
        Self {
            cutoff: false,
            thresholds: InputGateThresholds::for_rated_vout(rated_vout_mv),
            low_streak: 0,
            recover_streak: 0,
        }
    }

    pub fn step(&mut self, fresh_pre_tps_vin_mv: Option<u16>) -> InputGateAction {
        let Some(vin_mv) = fresh_pre_tps_vin_mv else {
            self.low_streak = 0;
            self.recover_streak = 0;
            return InputGateAction::None;
        };

        if self.cutoff {
            self.low_streak = 0;
            if vin_mv > self.thresholds.recover_mv {
                self.recover_streak = self.recover_streak.saturating_add(1);
                if self.recover_streak >= self.thresholds.required_samples {
                    self.cutoff = false;
                    self.recover_streak = 0;
                    return InputGateAction::Enable;
                }
            } else {
                self.recover_streak = 0;
            }
            return InputGateAction::None;
        }

        self.recover_streak = 0;
        if vin_mv < self.thresholds.cutoff_mv {
            self.low_streak = self.low_streak.saturating_add(1);
            if self.low_streak >= self.thresholds.required_samples {
                self.cutoff = true;
                self.low_streak = 0;
                return InputGateAction::Cutoff;
            }
        } else {
            self.low_streak = 0;
        }
        InputGateAction::None
    }

    pub const fn state_slug(self) -> &'static str {
        if self.cutoff {
            "cutoff"
        } else {
            "enabled"
        }
    }

    pub const fn reason_slug(self) -> &'static str {
        if self.cutoff {
            "pre_tps_undervoltage"
        } else {
            "none"
        }
    }
}

impl Default for InputGateTracker {
    fn default() -> Self {
        Self::new()
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
    matches!(
        state.gate_reason,
        OutputGateReason::None | OutputGateReason::TpsFault | OutputGateReason::TpsConfigFailed
    ) && state.active_outputs == EnabledOutputs::None
        && state.recoverable_outputs != EnabledOutputs::None
        && mains_present == Some(true)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LowBatteryOutputHoldReleaseInput {
    pub state: OutputRuntimeState,
    pub recoverable_source: OutputGateReason,
    pub mains_present: Option<bool>,
    pub discharge_ready: Option<bool>,
    pub rca_alarm: Option<bool>,
    pub no_battery: Option<bool>,
    pub low_voltage_blocked: Option<bool>,
    pub rsoc_pct: Option<u16>,
}

pub fn low_battery_output_hold_release_allowed(input: LowBatteryOutputHoldReleaseInput) -> bool {
    if input.recoverable_source != OutputGateReason::BmsNotReady {
        return false;
    }
    if input.state.gate_reason != OutputGateReason::None {
        return false;
    }
    if input.state.requested_outputs == EnabledOutputs::None {
        return false;
    }
    if input.state.active_outputs != EnabledOutputs::None {
        return false;
    }
    if input.state.recoverable_outputs == EnabledOutputs::None {
        return false;
    }
    if input.mains_present != Some(true) {
        return false;
    }
    if input.discharge_ready != Some(true) {
        return false;
    }
    if input.rca_alarm != Some(false) {
        return false;
    }
    if input.no_battery != Some(false) {
        return false;
    }
    if input.low_voltage_blocked != Some(false) {
        return false;
    }

    match input.rsoc_pct {
        Some(pct) => (LOW_BATTERY_OUTPUT_RESTORE_RSOC_PCT..=100).contains(&pct),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        low_battery_output_hold_release_allowed, output_restore_pending_from_state,
        output_state_gate_transition, EnabledOutputs, InputGateAction, InputGateThresholds,
        InputGateTracker, LowBatteryOutputHoldReleaseInput, OutputGateReason, OutputRuntimeState,
        OutputSelector,
    };

    #[test]
    fn input_gate_requires_three_consecutive_low_fresh_samples() {
        let mut tracker = InputGateTracker::new();
        assert_eq!(tracker.step(Some(11_299)), InputGateAction::None);
        assert_eq!(tracker.step(None), InputGateAction::None);
        assert_eq!(tracker.step(Some(11_000)), InputGateAction::None);
        assert_eq!(tracker.step(Some(11_300)), InputGateAction::None);
        assert_eq!(tracker.step(Some(11_250)), InputGateAction::None);
        assert_eq!(tracker.step(Some(11_200)), InputGateAction::None);
        assert_eq!(tracker.step(Some(11_100)), InputGateAction::Cutoff);
        assert!(tracker.cutoff);
    }

    #[test]
    fn input_gate_recovers_only_after_three_samples_above_11v5_for_12v_profile() {
        let mut tracker = InputGateTracker::new();
        for sample in [11_200, 11_100, 11_000] {
            let _ = tracker.step(Some(sample));
        }
        assert_eq!(tracker.step(Some(11_501)), InputGateAction::None);
        assert_eq!(tracker.step(Some(11_500)), InputGateAction::None);
        assert_eq!(tracker.step(Some(11_520)), InputGateAction::None);
        assert_eq!(tracker.step(Some(11_540)), InputGateAction::None);
        assert_eq!(tracker.step(Some(11_560)), InputGateAction::Enable);
        assert!(!tracker.cutoff);
    }

    #[test]
    fn input_gate_thresholds_follow_output_profile() {
        assert_eq!(
            InputGateThresholds::for_rated_vout(12_000),
            InputGateThresholds {
                cutoff_mv: 11_300,
                recover_mv: 11_500,
                required_samples: 3,
            }
        );
        assert_eq!(
            InputGateThresholds::for_rated_vout(19_000),
            InputGateThresholds {
                cutoff_mv: 10_000,
                recover_mv: 11_000,
                required_samples: 3,
            }
        );
    }

    #[test]
    fn input_gate_19v_profile_keeps_legacy_thresholds() {
        let mut tracker = InputGateTracker::for_rated_vout(19_000);
        assert_eq!(tracker.step(Some(10_100)), InputGateAction::None);
        assert_eq!(tracker.step(Some(9_950)), InputGateAction::None);
        assert_eq!(tracker.step(Some(9_900)), InputGateAction::None);
        assert_eq!(tracker.step(Some(9_800)), InputGateAction::Cutoff);
        assert!(tracker.cutoff);
        assert_eq!(tracker.step(Some(11_001)), InputGateAction::None);
        assert_eq!(tracker.step(Some(11_100)), InputGateAction::None);
        assert_eq!(tracker.step(Some(11_200)), InputGateAction::Enable);
    }

    fn low_battery_release_input() -> LowBatteryOutputHoldReleaseInput {
        LowBatteryOutputHoldReleaseInput {
            state: OutputRuntimeState::new(
                EnabledOutputs::Only(OutputSelector::OutA),
                EnabledOutputs::None,
                EnabledOutputs::Only(OutputSelector::OutA),
                OutputGateReason::None,
            ),
            recoverable_source: OutputGateReason::BmsNotReady,
            mains_present: Some(true),
            discharge_ready: Some(true),
            rca_alarm: Some(false),
            no_battery: Some(false),
            low_voltage_blocked: Some(false),
            rsoc_pct: Some(20),
        }
    }

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
    fn output_restore_pending_requires_vin_online_and_restoreable_gate() {
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
        assert!(output_restore_pending_from_state(fault_gated, Some(true)));

        let config_failed = output_state_gate_transition(state, OutputGateReason::TpsConfigFailed);
        assert!(output_restore_pending_from_state(config_failed, Some(true)));

        let bms_blocked = output_state_gate_transition(state, OutputGateReason::BmsNotReady);
        assert!(!output_restore_pending_from_state(bms_blocked, Some(true)));
    }

    #[test]
    fn low_battery_hold_release_accepts_rsoc_20_with_safe_bms_and_vin() {
        assert!(low_battery_output_hold_release_allowed(
            low_battery_release_input()
        ));
    }

    #[test]
    fn low_battery_hold_release_rejects_rsoc_below_20() {
        let mut input = low_battery_release_input();
        input.rsoc_pct = Some(19);

        assert!(!low_battery_output_hold_release_allowed(input));
    }

    #[test]
    fn low_battery_hold_release_requires_vin_online() {
        let mut input = low_battery_release_input();
        input.mains_present = Some(false);
        assert!(!low_battery_output_hold_release_allowed(input));

        input.mains_present = None;
        assert!(!low_battery_output_hold_release_allowed(input));
    }

    #[test]
    fn low_battery_hold_release_requires_safe_bq40_state() {
        let mut input = low_battery_release_input();
        input.discharge_ready = Some(false);
        assert!(!low_battery_output_hold_release_allowed(input));

        input = low_battery_release_input();
        input.rca_alarm = Some(true);
        assert!(!low_battery_output_hold_release_allowed(input));

        input = low_battery_release_input();
        input.no_battery = Some(true);
        assert!(!low_battery_output_hold_release_allowed(input));

        input = low_battery_release_input();
        input.low_voltage_blocked = Some(true);
        assert!(!low_battery_output_hold_release_allowed(input));

        input = low_battery_release_input();
        input.low_voltage_blocked = None;
        assert!(!low_battery_output_hold_release_allowed(input));
    }

    #[test]
    fn low_battery_hold_release_rejects_other_recoverable_sources() {
        for source in [
            OutputGateReason::ThermKill,
            OutputGateReason::TpsFault,
            OutputGateReason::TpsConfigFailed,
            OutputGateReason::ActiveProtection,
            OutputGateReason::None,
        ] {
            let mut input = low_battery_release_input();
            input.recoverable_source = source;
            assert!(!low_battery_output_hold_release_allowed(input));
        }
    }

    #[test]
    fn low_battery_hold_release_does_not_bypass_existing_admission_state() {
        let mut input = low_battery_release_input();
        input.state.active_outputs = EnabledOutputs::Only(OutputSelector::OutA);
        assert!(!low_battery_output_hold_release_allowed(input));

        input = low_battery_release_input();
        input.state.recoverable_outputs = EnabledOutputs::None;
        assert!(!low_battery_output_hold_release_allowed(input));

        input = low_battery_release_input();
        input.state.gate_reason = OutputGateReason::TpsFault;
        assert!(!low_battery_output_hold_release_allowed(input));

        input = low_battery_release_input();
        input.state.requested_outputs = EnabledOutputs::None;
        assert!(!low_battery_output_hold_release_allowed(input));
    }
}
