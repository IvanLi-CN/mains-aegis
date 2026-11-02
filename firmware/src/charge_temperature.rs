//! Shared BQ40/BQ25792 charge-temperature policy.
//!
//! The policy is intentionally pure so the boundary cases can be exercised on
//! the host without requiring an ESP target or a connected gauge.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChargeTemperatureBand {
    NoCharge,
    Low,
    Normal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChargeTemperatureDecision {
    pub band: ChargeTemperatureBand,
    pub target_current_ma: Option<u16>,
    pub target_voltage_mv: Option<u16>,
}

impl ChargeTemperatureDecision {
    pub const fn no_charge() -> Self {
        Self {
            band: ChargeTemperatureBand::NoCharge,
            target_current_ma: None,
            target_voltage_mv: None,
        }
    }

    pub const fn low() -> Self {
        Self {
            band: ChargeTemperatureBand::Low,
            target_current_ma: Some(100),
            target_voltage_mv: Some(16_800),
        }
    }

    pub const fn normal() -> Self {
        Self {
            band: ChargeTemperatureBand::Normal,
            target_current_ma: Some(500),
            target_voltage_mv: Some(16_800),
        }
    }

    pub const fn allows_charge(self) -> bool {
        matches!(
            self.band,
            ChargeTemperatureBand::Low | ChargeTemperatureBand::Normal
        )
    }
}

/// BQ40 stores temperature in 0.1 K. These are the rounded values used by
/// the profile and therefore avoid a second, ambiguous Celsius conversion at
/// the runtime boundary.
pub const TEMP_ZERO_K_X10: u16 = 2_732;
pub const TEMP_COOL_K_X10: u16 = 2_882;
pub const TEMP_WARM_K_X10: u16 = 3_182;

pub const BQ40_REQUEST_MIN_CURRENT_MA: u16 = 50;
pub const BQ40_REQUEST_MAX_CURRENT_MA: u16 = 5_000;
pub const BQ40_REQUEST_MIN_VOLTAGE_MV: u16 = 3_000;
pub const BQ40_REQUEST_MAX_VOLTAGE_MV: u16 = 16_800;
pub const BQ40_REQUEST_MAX_AGE_MS: u64 = 1_000;
pub const CHARGE_TEMPERATURE_MAX_AGE_MS: u64 = 1_000;

/// The already-selected BQ40 project configuration that gives DA_STATUS2
/// TS1..TS4 their board/cell meaning. The charge profile does not write these
/// fields; they are a design-time prerequisite for interpreting the vectors.
pub const BQ40_PROJECT_TEMPERATURE_ENABLE: u8 = 0x1E;
pub const BQ40_PROJECT_TEMPERATURE_MODE: u8 = 0x00;

pub const fn bq40_project_temperature_configuration_matches(
    temperature_enable: u8,
    temperature_mode: u8,
) -> bool {
    temperature_enable == BQ40_PROJECT_TEMPERATURE_ENABLE
        && temperature_mode == BQ40_PROJECT_TEMPERATURE_MODE
}

pub const fn bq40_temperature_configuration_status(
    temperature_enable: Option<u8>,
    temperature_mode: Option<u8>,
) -> Option<bool> {
    match (temperature_enable, temperature_mode) {
        (Some(enable), Some(mode)) => {
            Some(bq40_project_temperature_configuration_matches(enable, mode))
        }
        (Some(enable), None) if enable != BQ40_PROJECT_TEMPERATURE_ENABLE => Some(false),
        (None, Some(mode)) if mode != BQ40_PROJECT_TEMPERATURE_MODE => Some(false),
        _ => None,
    }
}

pub const fn bq40_temperature_configuration_verified(value: Option<bool>) -> bool {
    match value {
        Some(true) => true,
        Some(false) | None => false,
    }
}

pub const fn bq40_activation_temperature_configuration_allows(value: Option<bool>) -> bool {
    !matches!(value, Some(false))
}

pub const fn is_temperature_fresh(now_ms: u64, observed_at_ms: u64) -> bool {
    now_ms.saturating_sub(observed_at_ms) <= CHARGE_TEMPERATURE_MAX_AGE_MS
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bq40ChargeRequest {
    pub current_ma: u16,
    pub voltage_mv: u16,
    pub observed_at_ms: u64,
}

impl Bq40ChargeRequest {
    pub const fn is_valid(self) -> bool {
        self.current_ma >= BQ40_REQUEST_MIN_CURRENT_MA
            && self.current_ma <= BQ40_REQUEST_MAX_CURRENT_MA
            && self.voltage_mv >= BQ40_REQUEST_MIN_VOLTAGE_MV
            && self.voltage_mv <= BQ40_REQUEST_MAX_VOLTAGE_MV
    }

    pub const fn is_fresh(self, now_ms: u64) -> bool {
        self.is_valid() && now_ms.saturating_sub(self.observed_at_ms) <= BQ40_REQUEST_MAX_AGE_MS
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChargeTarget {
    pub current_ma: u16,
    pub voltage_mv: u16,
}

pub const fn clamp_target_to_bq40_request(
    policy_current_ma: u16,
    policy_voltage_mv: u16,
    request: Bq40ChargeRequest,
) -> Option<ChargeTarget> {
    if !request.is_valid() {
        return None;
    }

    let current_ma = if policy_current_ma < request.current_ma {
        policy_current_ma
    } else {
        request.current_ma
    };
    let voltage_mv = if policy_voltage_mv < request.voltage_mv {
        policy_voltage_mv
    } else {
        request.voltage_mv
    };
    if current_ma == 0 || voltage_mv == 0 {
        None
    } else {
        Some(ChargeTarget {
            current_ma,
            voltage_mv,
        })
    }
}

pub const fn clamp_policy_to_bq40_request(
    policy_current_ma: u16,
    policy_voltage_mv: u16,
    temperature: ChargeTemperatureDecision,
    request: Bq40ChargeRequest,
) -> Option<ChargeTarget> {
    if !temperature.allows_charge() {
        return None;
    }

    let temperature_current_ma = match temperature.target_current_ma {
        Some(current_ma) => current_ma,
        None => return None,
    };
    let temperature_voltage_mv = match temperature.target_voltage_mv {
        Some(voltage_mv) => voltage_mv,
        None => return None,
    };
    let policy_current_ma = if policy_current_ma < temperature_current_ma {
        policy_current_ma
    } else {
        temperature_current_ma
    };
    let policy_voltage_mv = if policy_voltage_mv < temperature_voltage_mv {
        policy_voltage_mv
    } else {
        temperature_voltage_mv
    };
    clamp_target_to_bq40_request(policy_current_ma, policy_voltage_mv, request)
}

/// Resolve the live charger target from the temperature policy and the most
/// recent BQ40 request. The runtime deliberately fails closed when the
/// request is absent, invalid, or older than the one-second request window.
pub const fn runtime_charge_target(
    now_ms: u64,
    temperature: ChargeTemperatureDecision,
    request: Option<Bq40ChargeRequest>,
    policy_current_ma: Option<u16>,
    policy_voltage_mv: u16,
) -> Option<ChargeTarget> {
    let Some(request) = request else {
        return None;
    };
    if !request.is_fresh(now_ms) {
        return None;
    }
    let Some(policy_current_ma) = policy_current_ma else {
        return None;
    };
    clamp_policy_to_bq40_request(policy_current_ma, policy_voltage_mv, temperature, request)
}

pub const fn bq40_product_charge_evidence_fresh(
    now_ms: u64,
    temperature: ChargeTemperatureDecision,
    request: Option<Bq40ChargeRequest>,
) -> bool {
    if !temperature.allows_charge() {
        return false;
    }
    match request {
        Some(request) => request.is_fresh(now_ms),
        None => false,
    }
}

/// Gate the exceptional activation wake current. With no BQ40 observation at
/// all, the caller may rely on the independent charger-side JEITA interlock to
/// wake a sleeping gauge. Once either a request or a temperature observation
/// exists, both fresh request and fresh normal-band temperature evidence are
/// required.
pub const fn activation_wake_charge_allowed(
    now_ms: u64,
    temperature_observed_at_ms: Option<u64>,
    temperature: Option<ChargeTemperatureDecision>,
    request_observed: bool,
    request: Option<Bq40ChargeRequest>,
) -> bool {
    if temperature_observed_at_ms.is_none() && !request_observed {
        return true;
    }
    if !request_observed {
        return false;
    }
    let Some(request) = request else {
        return false;
    };
    if !request.is_fresh(now_ms) {
        return false;
    }
    matches!(
        temperature,
        Some(ChargeTemperatureDecision {
            band: ChargeTemperatureBand::Normal,
            ..
        })
    )
}

pub const fn activation_wake_charge_should_disable(
    now_ms: u64,
    temperature_observed_at_ms: Option<u64>,
    temperature: Option<ChargeTemperatureDecision>,
    request_observed: bool,
    request: Option<Bq40ChargeRequest>,
    charger_enabled: bool,
) -> bool {
    charger_enabled
        && !activation_wake_charge_allowed(
            now_ms,
            temperature_observed_at_ms,
            temperature,
            request_observed,
            request,
        )
}

/// Latch any successfully read half of the BQ40 current/voltage request. A
/// partial read is still safety evidence; a later transport failure must not
/// reopen the fixed activation-wake fallback.
pub const fn latch_bq40_charge_request_observed(
    previously_observed: bool,
    current_read_succeeded: bool,
    voltage_read_succeeded: bool,
) -> bool {
    previously_observed || current_read_succeeded || voltage_read_succeeded
}

pub const fn evaluate_cell_temperature_k_x10(temp_k_x10: Option<u16>) -> ChargeTemperatureDecision {
    match temp_k_x10 {
        Some(temp_k_x10) if temp_k_x10 < TEMP_ZERO_K_X10 => ChargeTemperatureDecision::no_charge(),
        Some(temp_k_x10) if temp_k_x10 < TEMP_COOL_K_X10 => ChargeTemperatureDecision::low(),
        Some(temp_k_x10) if temp_k_x10 < TEMP_WARM_K_X10 => ChargeTemperatureDecision::normal(),
        _ => ChargeTemperatureDecision::no_charge(),
    }
}

/// Return the maximum of TS2/TS3/TS4. TS1 is deliberately ignored for the
/// cell-temperature decision; an absent or zero cell sensor is fail-closed.
pub const fn max_cell_temperature_k_x10(ts_temp_k_x10: [u16; 4]) -> Option<u16> {
    if ts_temp_k_x10[1] == 0 || ts_temp_k_x10[2] == 0 || ts_temp_k_x10[3] == 0 {
        return None;
    }

    let max = if ts_temp_k_x10[1] > ts_temp_k_x10[2] {
        ts_temp_k_x10[1]
    } else {
        ts_temp_k_x10[2]
    };
    Some(if max > ts_temp_k_x10[3] {
        max
    } else {
        ts_temp_k_x10[3]
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bq40TemperatureField {
    T1,
    T2,
    T5,
    T6,
    T3,
    T4,
    Hysteresis,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bq40TemperatureFieldVector {
    pub field: Bq40TemperatureField,
    pub address: u16,
    pub raw: u16,
    pub bytes_le: [u8; 2],
}

pub const BQ40_CHARGE_TEMPERATURE_VECTORS: [Bq40TemperatureFieldVector; 7] = [
    Bq40TemperatureFieldVector {
        field: Bq40TemperatureField::T1,
        address: 0x4A0B,
        raw: 2_732,
        bytes_le: [0xAC, 0x0A],
    },
    Bq40TemperatureFieldVector {
        field: Bq40TemperatureField::T2,
        address: 0x4A0D,
        raw: 2_882,
        bytes_le: [0x42, 0x0B],
    },
    Bq40TemperatureFieldVector {
        field: Bq40TemperatureField::T5,
        address: 0x4A0F,
        raw: 2_932,
        bytes_le: [0x74, 0x0B],
    },
    Bq40TemperatureFieldVector {
        field: Bq40TemperatureField::T6,
        address: 0x4A11,
        raw: 3_082,
        bytes_le: [0x0A, 0x0C],
    },
    Bq40TemperatureFieldVector {
        field: Bq40TemperatureField::T3,
        address: 0x4A13,
        raw: 3_132,
        bytes_le: [0x3C, 0x0C],
    },
    Bq40TemperatureFieldVector {
        field: Bq40TemperatureField::T4,
        address: 0x4A15,
        raw: 3_182,
        bytes_le: [0x6E, 0x0C],
    },
    Bq40TemperatureFieldVector {
        field: Bq40TemperatureField::Hysteresis,
        address: 0x4A17,
        raw: 20,
        bytes_le: [0x14, 0x00],
    },
];

/// Current/voltage entries are a static manifest only. They are not Data
/// Flash writer targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bq40ChargeProfileManifestEntry {
    pub label: &'static str,
    pub current_ma: u16,
    pub voltage_mv: u16,
}

pub const BQ40_CHARGE_PROFILE_MANIFEST: [Bq40ChargeProfileManifestEntry; 5] = [
    Bq40ChargeProfileManifestEntry {
        label: "LT",
        current_ma: 100,
        voltage_mv: 16_800,
    },
    Bq40ChargeProfileManifestEntry {
        label: "STL",
        current_ma: 500,
        voltage_mv: 16_800,
    },
    Bq40ChargeProfileManifestEntry {
        label: "RT",
        current_ma: 500,
        voltage_mv: 16_800,
    },
    Bq40ChargeProfileManifestEntry {
        label: "STH",
        current_ma: 500,
        voltage_mv: 16_800,
    },
    Bq40ChargeProfileManifestEntry {
        label: "HT",
        current_ma: 500,
        voltage_mv: 16_800,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_only_ts2_ts3_ts4_and_fail_closes_missing_cell_sensor() {
        assert_eq!(
            max_cell_temperature_k_x10([3_500, 2_800, 2_900, 2_850]),
            Some(2_900)
        );
        assert_eq!(max_cell_temperature_k_x10([2_700, 0, 2_900, 2_850]), None);
    }

    #[test]
    fn project_temperature_configuration_is_explicit() {
        assert!(bq40_project_temperature_configuration_matches(0x1E, 0x00));
        assert!(!bq40_project_temperature_configuration_matches(0x00, 0x00));
        assert!(!bq40_project_temperature_configuration_matches(0x1E, 0x01));
    }

    #[test]
    fn incomplete_temperature_configuration_stays_unknown() {
        assert_eq!(
            bq40_temperature_configuration_status(Some(0x1E), Some(0x00)),
            Some(true)
        );
        assert_eq!(
            bq40_temperature_configuration_status(Some(0x00), Some(0x00)),
            Some(false)
        );
        assert_eq!(
            bq40_temperature_configuration_status(Some(0x1E), None),
            None
        );
        assert_eq!(
            bq40_temperature_configuration_status(None, Some(0x00)),
            None
        );
        assert_eq!(
            bq40_temperature_configuration_status(Some(0x00), None),
            Some(false)
        );
    }

    #[test]
    fn product_charge_evidence_must_be_fresh_and_in_a_charge_band() {
        let request = Bq40ChargeRequest {
            current_ma: 500,
            voltage_mv: 16_800,
            observed_at_ms: 100,
        };
        assert!(bq40_product_charge_evidence_fresh(
            1_100,
            ChargeTemperatureDecision::normal(),
            Some(request)
        ));
        assert!(!bq40_product_charge_evidence_fresh(
            1_101,
            ChargeTemperatureDecision::normal(),
            Some(request)
        ));
        assert!(!bq40_product_charge_evidence_fresh(
            100,
            ChargeTemperatureDecision::no_charge(),
            Some(request)
        ));
        assert!(!bq40_product_charge_evidence_fresh(
            100,
            ChargeTemperatureDecision::normal(),
            None
        ));
    }

    #[test]
    fn unknown_temperature_configuration_fails_closed() {
        assert!(bq40_temperature_configuration_verified(Some(true)));
        assert!(!bq40_temperature_configuration_verified(Some(false)));
        assert!(!bq40_temperature_configuration_verified(None));
    }

    #[test]
    fn activation_wake_allows_unknown_configuration_until_bq40_is_observed() {
        assert!(bq40_activation_temperature_configuration_allows(None));
        assert!(bq40_activation_temperature_configuration_allows(Some(true)));
        assert!(!bq40_activation_temperature_configuration_allows(Some(
            false
        )));
    }

    #[test]
    fn charge_profile_manifest_keeps_each_bq40_temperature_band_explicit() {
        assert_eq!(BQ40_CHARGE_PROFILE_MANIFEST[0].label, "LT");
        assert_eq!(BQ40_CHARGE_PROFILE_MANIFEST[1].label, "STL");
        assert_eq!(BQ40_CHARGE_PROFILE_MANIFEST[2].label, "RT");
        assert_eq!(BQ40_CHARGE_PROFILE_MANIFEST[3].label, "STH");
        assert_eq!(BQ40_CHARGE_PROFILE_MANIFEST[4].label, "HT");
        assert!(BQ40_CHARGE_PROFILE_MANIFEST
            .iter()
            .all(|entry| entry.voltage_mv == 16_800));
        assert_eq!(BQ40_CHARGE_PROFILE_MANIFEST[0].current_ma, 100);
        assert!(BQ40_CHARGE_PROFILE_MANIFEST[1..]
            .iter()
            .all(|entry| entry.current_ma == 500));
    }

    #[test]
    fn activation_wake_gate_only_allows_missing_bq40_evidence() {
        let request = Bq40ChargeRequest {
            current_ma: 500,
            voltage_mv: 16_800,
            observed_at_ms: 100,
        };
        assert!(activation_wake_charge_allowed(100, None, None, false, None));
        assert!(!activation_wake_charge_allowed(
            100,
            Some(100),
            None,
            false,
            None
        ));
        assert!(!activation_wake_charge_allowed(
            100,
            None,
            None,
            true,
            Some(request)
        ));
        assert!(!activation_wake_charge_allowed(100, None, None, true, None));
        assert!(activation_wake_charge_allowed(
            100,
            Some(100),
            Some(ChargeTemperatureDecision::normal()),
            true,
            Some(request)
        ));
        assert!(!activation_wake_charge_allowed(
            100,
            Some(100),
            Some(ChargeTemperatureDecision::low()),
            true,
            Some(request)
        ));
        assert!(!activation_wake_charge_allowed(
            1_101,
            Some(100),
            Some(ChargeTemperatureDecision::normal()),
            true,
            Some(request)
        ));
        assert!(activation_wake_charge_should_disable(
            1_101,
            Some(100),
            Some(ChargeTemperatureDecision::normal()),
            true,
            Some(request),
            true,
        ));
        assert!(!activation_wake_charge_should_disable(
            1_101,
            Some(100),
            Some(ChargeTemperatureDecision::normal()),
            true,
            Some(request),
            false,
        ));
    }

    #[test]
    fn invalid_observed_request_blocks_activation_wake() {
        let invalid = Bq40ChargeRequest {
            current_ma: 0,
            voltage_mv: 0,
            observed_at_ms: 100,
        };
        assert!(!activation_wake_charge_allowed(
            100,
            None,
            None,
            true,
            Some(invalid)
        ));
    }

    #[test]
    fn partial_bq40_request_read_latches_safety_evidence() {
        assert!(latch_bq40_charge_request_observed(false, true, false));
        assert!(latch_bq40_charge_request_observed(false, false, true));
        assert!(latch_bq40_charge_request_observed(true, false, false));
        assert!(!latch_bq40_charge_request_observed(false, false, false));
    }

    #[test]
    fn temperature_boundaries_are_conservative_and_monotonic() {
        assert_eq!(
            evaluate_cell_temperature_k_x10(Some(2_731)).band,
            ChargeTemperatureBand::NoCharge
        );
        assert_eq!(
            evaluate_cell_temperature_k_x10(Some(2_732)).target_current_ma,
            Some(100)
        );
        assert_eq!(
            evaluate_cell_temperature_k_x10(Some(2_881)).target_current_ma,
            Some(100)
        );
        assert_eq!(
            evaluate_cell_temperature_k_x10(Some(2_882)).target_current_ma,
            Some(500)
        );
        assert_eq!(
            evaluate_cell_temperature_k_x10(Some(3_181)).target_current_ma,
            Some(500)
        );
        assert_eq!(
            evaluate_cell_temperature_k_x10(Some(3_182)).band,
            ChargeTemperatureBand::NoCharge
        );
        assert_eq!(
            evaluate_cell_temperature_k_x10(Some(3_282)).band,
            ChargeTemperatureBand::NoCharge
        );
    }

    #[test]
    fn vectors_match_the_golden_little_endian_bytes() {
        assert_eq!(
            BQ40_CHARGE_TEMPERATURE_VECTORS.map(|vector| vector.bytes_le),
            [
                [0xAC, 0x0A],
                [0x42, 0x0B],
                [0x74, 0x0B],
                [0x0A, 0x0C],
                [0x3C, 0x0C],
                [0x6E, 0x0C],
                [0x14, 0x00],
            ]
        );
        assert!(BQ40_CHARGE_TEMPERATURE_VECTORS[..6]
            .windows(2)
            .all(|pair| pair[0].raw <= pair[1].raw));
    }

    #[test]
    fn invalid_or_stale_requests_do_not_clamp_a_charge_target() {
        let invalid = Bq40ChargeRequest {
            current_ma: 0,
            voltage_mv: 16_800,
            observed_at_ms: 0,
        };
        assert_eq!(clamp_target_to_bq40_request(500, 16_800, invalid), None);

        let stale = Bq40ChargeRequest {
            current_ma: 500,
            voltage_mv: 16_800,
            observed_at_ms: 0,
        };
        assert!(!stale.is_fresh(1_001));
        assert_eq!(
            clamp_target_to_bq40_request(500, 16_800, stale),
            Some(ChargeTarget {
                current_ma: 500,
                voltage_mv: 16_800
            })
        );
    }

    #[test]
    fn request_is_an_upper_bound() {
        let request = Bq40ChargeRequest {
            current_ma: 100,
            voltage_mv: 15_000,
            observed_at_ms: 10,
        };
        assert_eq!(
            clamp_target_to_bq40_request(500, 16_800, request),
            Some(ChargeTarget {
                current_ma: 100,
                voltage_mv: 15_000,
            })
        );
    }

    #[test]
    fn temperature_target_is_applied_before_bq40_clamp() {
        let request = Bq40ChargeRequest {
            current_ma: 500,
            voltage_mv: 16_800,
            observed_at_ms: 10,
        };
        assert_eq!(
            clamp_policy_to_bq40_request(500, 16_800, ChargeTemperatureDecision::low(), request),
            Some(ChargeTarget {
                current_ma: 100,
                voltage_mv: 16_800,
            })
        );
        assert_eq!(
            clamp_policy_to_bq40_request(500, 16_800, ChargeTemperatureDecision::normal(), request,),
            Some(ChargeTarget {
                current_ma: 500,
                voltage_mv: 16_800,
            })
        );
    }

    #[test]
    fn runtime_target_fails_closed_for_stale_invalid_or_blocked_inputs() {
        let request = Bq40ChargeRequest {
            current_ma: 500,
            voltage_mv: 16_800,
            observed_at_ms: 1_000,
        };
        assert_eq!(
            runtime_charge_target(
                2_000,
                ChargeTemperatureDecision::normal(),
                Some(request),
                Some(500),
                16_800,
            ),
            Some(ChargeTarget {
                current_ma: 500,
                voltage_mv: 16_800,
            })
        );
        assert_eq!(
            runtime_charge_target(
                2_001,
                ChargeTemperatureDecision::normal(),
                Some(request),
                Some(500),
                16_800,
            ),
            None
        );
        assert_eq!(
            runtime_charge_target(
                1_000,
                ChargeTemperatureDecision::no_charge(),
                Some(request),
                Some(500),
                16_800,
            ),
            None
        );
        assert_eq!(
            runtime_charge_target(
                1_000,
                ChargeTemperatureDecision::normal(),
                Some(request),
                None,
                16_800,
            ),
            None
        );
    }

    #[test]
    fn runtime_target_clamps_low_and_normal_modes() {
        let request = Bq40ChargeRequest {
            current_ma: 5_000,
            voltage_mv: 16_800,
            observed_at_ms: 0,
        };
        assert_eq!(
            runtime_charge_target(
                1_000,
                ChargeTemperatureDecision::low(),
                Some(request),
                Some(500),
                16_800,
            ),
            Some(ChargeTarget {
                current_ma: 100,
                voltage_mv: 16_800,
            })
        );
        assert_eq!(
            runtime_charge_target(
                1_000,
                ChargeTemperatureDecision::normal(),
                Some(request),
                Some(500),
                16_800,
            ),
            Some(ChargeTarget {
                current_ma: 500,
                voltage_mv: 16_800,
            })
        );
    }

    #[test]
    fn temperature_cache_expires_after_one_second() {
        assert!(is_temperature_fresh(1_000, 0));
        assert!(!is_temperature_fresh(1_001, 0));
    }
}
