#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpsConfigRetryDecision {
    Retry,
    Latch,
}

pub const DEFAULT_TPS_CONFIG_MAX_RETRY_ATTEMPTS: u8 = 2;

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

#[cfg(test)]
mod tests {
    use super::{
        is_tps_config_error_retryable, tps_config_retry_decision, TpsConfigRetryDecision,
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
}
