#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProtectionConfig {
    pub temp_enter_c_x16: i16,
    pub temp_exit_c_x16: i16,
    pub temp_hold_ms: u64,
    pub current_enter_ma: i32,
    pub current_exit_ma: i32,
    pub current_hold_ms: u64,
    pub ilim_step_ma: u16,
    pub ilim_step_interval_ms: u64,
    pub min_ilim_ma: u16,
    pub shutdown_vout_mv: u16,
    pub shutdown_hold_ms: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProtectionInputs {
    pub max_temp_c_x16: Option<i16>,
    pub max_current_ma: Option<i32>,
    pub min_vout_mv: Option<u16>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProtectionStatus {
    pub thermal_active: bool,
    pub current_active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtectionReason {
    None,
    Thermal,
    Current,
    ThermalAndCurrent,
}

impl ProtectionReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            ProtectionReason::None => "none",
            ProtectionReason::Thermal => "thermal",
            ProtectionReason::Current => "current",
            ProtectionReason::ThermalAndCurrent => "thermal+current",
        }
    }
}

impl ProtectionStatus {
    pub const fn reason(self) -> ProtectionReason {
        match (self.thermal_active, self.current_active) {
            (false, false) => ProtectionReason::None,
            (true, false) => ProtectionReason::Thermal,
            (false, true) => ProtectionReason::Current,
            (true, true) => ProtectionReason::ThermalAndCurrent,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtectionPhase {
    Normal,
    Derating,
    Shutdown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProtectionRuntime {
    pub applied_ilim_ma: u16,
    pub phase: ProtectionPhase,
    pub status: ProtectionStatus,
    pub shutdown_reason: ProtectionReason,
    temp_over_since_ms: Option<u64>,
    current_over_since_ms: Option<u64>,
    next_step_due_ms: Option<u64>,
    low_vout_since_ms: Option<u64>,
}

impl ProtectionRuntime {
    pub const fn new(applied_ilim_ma: u16) -> Self {
        Self {
            applied_ilim_ma,
            phase: ProtectionPhase::Normal,
            status: ProtectionStatus {
                thermal_active: false,
                current_active: false,
            },
            shutdown_reason: ProtectionReason::None,
            temp_over_since_ms: None,
            current_over_since_ms: None,
            next_step_due_ms: None,
            low_vout_since_ms: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtectionAction {
    None,
    ApplyIlim(u16),
    RestoreDefaultIlim(u16),
    Shutdown(ProtectionReason),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProtectionStepResult {
    pub runtime: ProtectionRuntime,
    pub action: ProtectionAction,
}

pub fn step(
    now_ms: u64,
    cfg: ProtectionConfig,
    base_ilim_ma: u16,
    mut runtime: ProtectionRuntime,
    inputs: ProtectionInputs,
) -> ProtectionStepResult {
    let temp_state = classify_temp(
        now_ms,
        cfg,
        inputs.max_temp_c_x16,
        runtime.temp_over_since_ms,
    );
    runtime.temp_over_since_ms = temp_state.over_since_ms;

    let current_state = classify_current(
        now_ms,
        cfg,
        runtime.applied_ilim_ma,
        inputs.max_current_ma,
        runtime.current_over_since_ms,
    );
    runtime.current_over_since_ms = current_state.over_since_ms;

    runtime.status = ProtectionStatus {
        thermal_active: temp_state.active,
        current_active: current_state.active,
    };

    let reason = runtime.status.reason();

    if runtime.phase == ProtectionPhase::Shutdown {
        if reason == ProtectionReason::None {
            runtime.phase = ProtectionPhase::Normal;
            runtime.shutdown_reason = ProtectionReason::None;
            runtime.applied_ilim_ma = base_ilim_ma;
            runtime.next_step_due_ms = None;
            runtime.low_vout_since_ms = None;
            return ProtectionStepResult {
                runtime,
                action: ProtectionAction::RestoreDefaultIlim(base_ilim_ma),
            };
        }
        return ProtectionStepResult {
            runtime,
            action: ProtectionAction::None,
        };
    }

    if reason == ProtectionReason::None {
        runtime.phase = ProtectionPhase::Normal;
        runtime.next_step_due_ms = None;
        runtime.low_vout_since_ms = None;
        if runtime.applied_ilim_ma != base_ilim_ma {
            runtime.applied_ilim_ma = base_ilim_ma;
            return ProtectionStepResult {
                runtime,
                action: ProtectionAction::RestoreDefaultIlim(base_ilim_ma),
            };
        }
        return ProtectionStepResult {
            runtime,
            action: ProtectionAction::None,
        };
    }

    runtime.phase = ProtectionPhase::Derating;
    if let Some(vout_mv) = inputs.min_vout_mv {
        if vout_mv <= cfg.shutdown_vout_mv {
            if let Some(since) = runtime.low_vout_since_ms {
                if now_ms.saturating_sub(since) >= cfg.shutdown_hold_ms {
                    runtime.phase = ProtectionPhase::Shutdown;
                    runtime.shutdown_reason = reason;
                    runtime.next_step_due_ms = None;
                    runtime.low_vout_since_ms = None;
                    return ProtectionStepResult {
                        runtime,
                        action: ProtectionAction::Shutdown(reason),
                    };
                }
            } else {
                runtime.low_vout_since_ms = Some(now_ms);
            }
        } else {
            runtime.low_vout_since_ms = None;
        }
    }

    let step_due = match runtime.next_step_due_ms {
        None => true,
        Some(due) => now_ms >= due,
    };
    if step_due && runtime.applied_ilim_ma > cfg.min_ilim_ma {
        let next_ilim = runtime
            .applied_ilim_ma
            .saturating_sub(cfg.ilim_step_ma)
            .max(cfg.min_ilim_ma);
        if next_ilim != runtime.applied_ilim_ma {
            runtime.applied_ilim_ma = next_ilim;
            runtime.next_step_due_ms = Some(now_ms.saturating_add(cfg.ilim_step_interval_ms));
            return ProtectionStepResult {
                runtime,
                action: ProtectionAction::ApplyIlim(next_ilim),
            };
        }
    }

    ProtectionStepResult {
        runtime,
        action: ProtectionAction::None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ConditionState {
    active: bool,
    over_since_ms: Option<u64>,
}

fn classify_temp(
    now_ms: u64,
    cfg: ProtectionConfig,
    max_temp_c_x16: Option<i16>,
    mut over_since_ms: Option<u64>,
) -> ConditionState {
    if let Some(temp_c_x16) = max_temp_c_x16 {
        if temp_c_x16 >= cfg.temp_enter_c_x16 {
            if over_since_ms.is_none() {
                over_since_ms = Some(now_ms);
            }
        } else if temp_c_x16 <= cfg.temp_exit_c_x16 {
            over_since_ms = None;
        }
    }

    let active =
        over_since_ms.is_some_and(|since| now_ms.saturating_sub(since) >= cfg.temp_hold_ms);
    ConditionState {
        active,
        over_since_ms,
    }
}

fn classify_current(
    now_ms: u64,
    cfg: ProtectionConfig,
    applied_ilim_ma: u16,
    max_current_ma: Option<i32>,
    mut over_since_ms: Option<u64>,
) -> ConditionState {
    let applied_ilim_ma = applied_ilim_ma as i32;
    let dynamic_enter_ma = core::cmp::min(cfg.current_enter_ma, (applied_ilim_ma - 250).max(0));
    let dynamic_exit_ma = core::cmp::min(cfg.current_exit_ma, (applied_ilim_ma - 500).max(0));

    if let Some(current_ma) = max_current_ma {
        if current_ma >= dynamic_enter_ma {
            if over_since_ms.is_none() {
                over_since_ms = Some(now_ms);
            }
        } else if current_ma <= dynamic_exit_ma {
            over_since_ms = None;
        }
    }

    let active =
        over_since_ms.is_some_and(|since| now_ms.saturating_sub(since) >= cfg.current_hold_ms);
    ConditionState {
        active,
        over_since_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        step, ProtectionAction, ProtectionConfig, ProtectionInputs, ProtectionPhase,
        ProtectionReason, ProtectionRuntime,
    };

    const CFG: ProtectionConfig = ProtectionConfig {
        temp_enter_c_x16: 40 * 16,
        temp_exit_c_x16: 38 * 16,
        temp_hold_ms: 5_000,
        current_enter_ma: 3_250,
        current_exit_ma: 3_000,
        current_hold_ms: 3_000,
        ilim_step_ma: 250,
        ilim_step_interval_ms: 2_000,
        min_ilim_ma: 1_000,
        shutdown_vout_mv: 14_000,
        shutdown_hold_ms: 2_000,
    };

    #[test]
    fn thermal_derating_reduces_ilim_after_hold() {
        let runtime = ProtectionRuntime {
            temp_over_since_ms: Some(0),
            ..ProtectionRuntime::new(3_500)
        };
        let inputs = ProtectionInputs {
            max_temp_c_x16: Some(41 * 16),
            max_current_ma: Some(1_000),
            min_vout_mv: Some(19_000),
        };

        let held = step(5_000, CFG, 3_500, runtime, inputs);
        assert_eq!(held.runtime.phase, ProtectionPhase::Derating);
        assert_eq!(held.runtime.status.reason(), ProtectionReason::Thermal);
        assert_eq!(held.action, ProtectionAction::ApplyIlim(3_250));
    }

    #[test]
    fn current_derating_uses_dynamic_threshold_while_cc_persists() {
        let runtime = ProtectionRuntime {
            current_over_since_ms: Some(0),
            ..ProtectionRuntime::new(3_250)
        };
        let inputs = ProtectionInputs {
            max_temp_c_x16: Some(25 * 16),
            max_current_ma: Some(3_050),
            min_vout_mv: Some(18_000),
        };

        let held = step(3_000, CFG, 3_500, runtime, inputs);
        assert_eq!(held.runtime.status.reason(), ProtectionReason::Current);
        assert_eq!(held.action, ProtectionAction::ApplyIlim(3_000));
    }

    #[test]
    fn low_vout_escalates_to_shutdown_when_derating_persists() {
        let runtime = ProtectionRuntime {
            phase: ProtectionPhase::Derating,
            status: Default::default(),
            shutdown_reason: ProtectionReason::None,
            temp_over_since_ms: Some(0),
            current_over_since_ms: None,
            next_step_due_ms: Some(10_000),
            low_vout_since_ms: Some(1_000),
            applied_ilim_ma: 3_000,
        };
        let inputs = ProtectionInputs {
            max_temp_c_x16: Some(41 * 16),
            max_current_ma: Some(2_900),
            min_vout_mv: Some(13_500),
        };

        let result = step(5_100, CFG, 3_500, runtime, inputs);
        assert_eq!(result.runtime.phase, ProtectionPhase::Shutdown);
        assert_eq!(result.runtime.shutdown_reason, ProtectionReason::Thermal);
        assert_eq!(
            result.action,
            ProtectionAction::Shutdown(ProtectionReason::Thermal)
        );
    }

    #[test]
    fn clearing_conditions_restores_default_ilim() {
        let runtime = ProtectionRuntime {
            applied_ilim_ma: 2_750,
            phase: ProtectionPhase::Derating,
            status: Default::default(),
            shutdown_reason: ProtectionReason::None,
            temp_over_since_ms: Some(0),
            current_over_since_ms: Some(0),
            next_step_due_ms: Some(10_000),
            low_vout_since_ms: None,
        };
        let inputs = ProtectionInputs {
            max_temp_c_x16: Some(35 * 16),
            max_current_ma: Some(1_500),
            min_vout_mv: Some(19_000),
        };

        let result = step(20_000, CFG, 3_500, runtime, inputs);
        assert_eq!(result.runtime.phase, ProtectionPhase::Normal);
        assert_eq!(result.runtime.applied_ilim_ma, 3_500);
        assert_eq!(result.action, ProtectionAction::RestoreDefaultIlim(3_500));
    }
}
