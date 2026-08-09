#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpsConfigRetryDecision {
    Retry,
    Latch,
}

pub const DEFAULT_TPS_CONFIG_MAX_RETRY_ATTEMPTS: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpsFailureProtection {
    SoftwareStop,
    SoftwareStopThenHardInhibit,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TpsEnableInhibitState {
    mcu_drive_low: bool,
}

impl TpsEnableInhibitState {
    pub const fn mcu_drive_low(self) -> bool {
        self.mcu_drive_low
    }

    pub const fn can_assert(self) -> bool {
        !self.mcu_drive_low
    }

    pub fn assert_low(&mut self) -> bool {
        if !self.can_assert() {
            return false;
        }
        self.mcu_drive_low = true;
        true
    }

    /// Returns whether the MCU had already released its open-drain hold.
    pub fn release(&mut self) -> bool {
        let already_released = !self.mcu_drive_low;
        self.mcu_drive_low = false;
        already_released
    }
}

pub fn is_tps_config_error_retryable(kind: &'static str) -> bool {
    matches!(kind, "i2c_timeout" | "i2c_nack" | "i2c_arbitration" | "i2c")
}

pub fn tps_config_retry_decision(
    kind: &'static str,
    consecutive_failures: u8,
    max_retry_attempts: u8,
) -> TpsConfigRetryDecision {
    if !is_tps_config_error_retryable(kind) {
        return TpsConfigRetryDecision::Latch;
    }

    if consecutive_failures <= max_retry_attempts {
        TpsConfigRetryDecision::Retry
    } else {
        TpsConfigRetryDecision::Latch
    }
}

/// A TPS_EN hard inhibit is reserved for a retryable I2C failure after the
/// bounded retry budget has been exhausted. Configuration and status failures
/// continue through the existing software-only protective stop.
pub fn tps_i2c_retry_exhaustion_should_hard_inhibit(
    kind: &'static str,
    decision: TpsConfigRetryDecision,
) -> bool {
    is_tps_config_error_retryable(kind) && matches!(decision, TpsConfigRetryDecision::Latch)
}

/// All TPS failures first preserve the existing software stop. Only a bounded
/// retryable I2C exhaustion adds the board-level TPS_EN hardware inhibit.
pub fn tps_failure_protection(
    kind: &'static str,
    decision: TpsConfigRetryDecision,
) -> TpsFailureProtection {
    if tps_i2c_retry_exhaustion_should_hard_inhibit(kind, decision) {
        TpsFailureProtection::SoftwareStopThenHardInhibit
    } else {
        TpsFailureProtection::SoftwareStop
    }
}

pub const fn tps_enable_interlock_source(
    mcu_drive_low: bool,
    therm_kill_n_low: bool,
) -> &'static str {
    if mcu_drive_low {
        "mcu_i2c_retry_exhausted"
    } else if therm_kill_n_low {
        "external_or_unknown"
    } else {
        "released"
    }
}

#[cfg(test)]
mod tests {
    use super::{
        is_tps_config_error_retryable, tps_config_retry_decision, tps_enable_interlock_source,
        tps_failure_protection, tps_i2c_retry_exhaustion_should_hard_inhibit,
        TpsConfigRetryDecision, TpsEnableInhibitState, TpsFailureProtection,
        DEFAULT_TPS_CONFIG_MAX_RETRY_ATTEMPTS,
    };

    #[test]
    fn transient_i2c_errors_are_retryable() {
        assert!(is_tps_config_error_retryable("i2c_nack"));
        assert!(is_tps_config_error_retryable("i2c_timeout"));
        assert!(is_tps_config_error_retryable("i2c_arbitration"));
        assert!(is_tps_config_error_retryable("i2c"));
    }

    #[test]
    fn config_errors_latch_immediately() {
        assert_eq!(
            tps_config_retry_decision("invalid_config", 1, DEFAULT_TPS_CONFIG_MAX_RETRY_ATTEMPTS),
            TpsConfigRetryDecision::Latch
        );
        assert_eq!(
            tps_config_retry_decision("out_of_range", 1, DEFAULT_TPS_CONFIG_MAX_RETRY_ATTEMPTS),
            TpsConfigRetryDecision::Latch
        );
    }

    #[test]
    fn transient_failures_only_retry_within_budget() {
        assert_eq!(
            tps_config_retry_decision("i2c_nack", 1, DEFAULT_TPS_CONFIG_MAX_RETRY_ATTEMPTS),
            TpsConfigRetryDecision::Retry
        );
        assert_eq!(
            tps_config_retry_decision("i2c_nack", 2, DEFAULT_TPS_CONFIG_MAX_RETRY_ATTEMPTS),
            TpsConfigRetryDecision::Retry
        );
        assert_eq!(
            tps_config_retry_decision("i2c_nack", 3, DEFAULT_TPS_CONFIG_MAX_RETRY_ATTEMPTS),
            TpsConfigRetryDecision::Latch
        );
    }

    #[test]
    fn hard_inhibit_requires_retryable_i2c_exhaustion() {
        assert!(!tps_i2c_retry_exhaustion_should_hard_inhibit(
            "i2c_nack",
            TpsConfigRetryDecision::Retry,
        ));
        assert!(tps_i2c_retry_exhaustion_should_hard_inhibit(
            "i2c_timeout",
            TpsConfigRetryDecision::Latch,
        ));
        assert!(!tps_i2c_retry_exhaustion_should_hard_inhibit(
            "invalid_config",
            TpsConfigRetryDecision::Latch,
        ));
        assert_eq!(
            tps_failure_protection("i2c_arbitration", TpsConfigRetryDecision::Latch),
            TpsFailureProtection::SoftwareStopThenHardInhibit,
            "the hardware inhibit is always second to the software stop",
        );
        assert_eq!(
            tps_failure_protection("scp", TpsConfigRetryDecision::Latch),
            TpsFailureProtection::SoftwareStop,
        );
    }

    #[test]
    fn tps_enable_inhibit_release_is_idempotent_and_rearms_for_a_new_failure() {
        let mut state = TpsEnableInhibitState::default();

        assert!(state.can_assert());
        assert!(state.assert_low());
        assert!(state.mcu_drive_low());
        assert!(!state.assert_low());
        assert!(!state.release());
        assert!(!state.mcu_drive_low());
        assert!(state.release());
        assert!(state.assert_low());
    }

    #[test]
    fn released_mcu_hold_preserves_external_low_as_thermal_protection() {
        assert_eq!(
            tps_enable_interlock_source(true, true),
            "mcu_i2c_retry_exhausted"
        );
        assert_eq!(
            tps_enable_interlock_source(false, true),
            "external_or_unknown"
        );
        assert_eq!(tps_enable_interlock_source(false, false), "released");
    }
}
