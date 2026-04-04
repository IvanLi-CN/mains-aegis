#![allow(dead_code)]

#[path = "../../../firmware/src/runtime_audio_recovery.rs"]
mod runtime_audio_recovery;

#[cfg(test)]
mod tests {
    use super::runtime_audio_recovery::{
        RuntimeAudioRecoveryDecision, RuntimeAudioRecoverySnapshot, RuntimeAudioRecoveryState,
        AUDIO_RUNTIME_LATE_RECOVERY_WINDOW,
    };
    use std::time::{Duration, Instant};

    #[test]
    fn runtime_audio_recovery_clears_after_healthy_refill() {
        let start = Instant::now();
        let mut state = RuntimeAudioRecoveryState::new();
        assert_eq!(
            state.note_late(start),
            RuntimeAudioRecoveryDecision::AttemptRecover {
                first_in_burst: true,
                snapshot: RuntimeAudioRecoverySnapshot {
                    consecutive_late: 1,
                    recovery_attempts: 1,
                },
            }
        );
        assert_eq!(
            state.note_late(start + Duration::from_secs(1)),
            RuntimeAudioRecoveryDecision::AttemptRecover {
                first_in_burst: false,
                snapshot: RuntimeAudioRecoverySnapshot {
                    consecutive_late: 2,
                    recovery_attempts: 2,
                },
            }
        );
        assert_eq!(
            state.note_transport_healthy(),
            Some(RuntimeAudioRecoverySnapshot {
                consecutive_late: 2,
                recovery_attempts: 2,
            })
        );
        assert_eq!(state.note_transport_healthy(), None);
    }

    #[test]
    fn runtime_audio_recovery_treats_post_recovery_late_as_new_burst() {
        let start = Instant::now();
        let mut state = RuntimeAudioRecoveryState::new();
        assert!(matches!(
            state.note_late(start),
            RuntimeAudioRecoveryDecision::AttemptRecover {
                first_in_burst: true,
                ..
            }
        ));
        assert_eq!(
            state.note_transport_healthy(),
            Some(RuntimeAudioRecoverySnapshot {
                consecutive_late: 1,
                recovery_attempts: 1,
            })
        );
        assert_eq!(
            state.note_late(start + Duration::from_secs(1)),
            RuntimeAudioRecoveryDecision::AttemptRecover {
                first_in_burst: true,
                snapshot: RuntimeAudioRecoverySnapshot {
                    consecutive_late: 1,
                    recovery_attempts: 1,
                },
            }
        );
    }

    #[test]
    fn runtime_audio_recovery_disables_after_three_failed_retries() {
        let start = Instant::now();
        let mut state = RuntimeAudioRecoveryState::new();
        assert!(matches!(
            state.note_late(start),
            RuntimeAudioRecoveryDecision::AttemptRecover { .. }
        ));
        assert!(matches!(
            state.note_late(start + Duration::from_secs(1)),
            RuntimeAudioRecoveryDecision::AttemptRecover { .. }
        ));
        assert!(matches!(
            state.note_late(start + Duration::from_secs(2)),
            RuntimeAudioRecoveryDecision::AttemptRecover { .. }
        ));
        assert_eq!(
            state.note_late(start + Duration::from_secs(3)),
            RuntimeAudioRecoveryDecision::Disable {
                snapshot: RuntimeAudioRecoverySnapshot {
                    consecutive_late: 4,
                    recovery_attempts: 3,
                },
            }
        );
    }

    #[test]
    fn runtime_audio_recovery_resets_after_timeout_window() {
        let start = Instant::now();
        let mut state = RuntimeAudioRecoveryState::new();
        assert!(matches!(
            state.note_late(start),
            RuntimeAudioRecoveryDecision::AttemptRecover {
                first_in_burst: true,
                ..
            }
        ));
        assert_eq!(
            state.note_late(start + AUDIO_RUNTIME_LATE_RECOVERY_WINDOW + Duration::from_millis(1)),
            RuntimeAudioRecoveryDecision::AttemptRecover {
                first_in_burst: true,
                snapshot: RuntimeAudioRecoverySnapshot {
                    consecutive_late: 1,
                    recovery_attempts: 1,
                },
            }
        );
    }
}
