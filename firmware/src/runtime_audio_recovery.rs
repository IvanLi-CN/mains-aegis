#[cfg(not(any(test, codex_host_test)))]
use esp_hal::time::{Duration, Instant};

#[cfg(any(test, codex_host_test))]
use std::time::{Duration, Instant};

pub(crate) const AUDIO_RUNTIME_LATE_RECOVERY_WINDOW: Duration = Duration::from_secs(5);
pub(crate) const AUDIO_RUNTIME_LATE_MAX_RECOVERY_ATTEMPTS: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeAudioRecoverySnapshot {
    pub(crate) consecutive_late: u8,
    pub(crate) recovery_attempts: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeAudioRecoveryDecision {
    AttemptRecover {
        first_in_burst: bool,
        snapshot: RuntimeAudioRecoverySnapshot,
    },
    Disable {
        snapshot: RuntimeAudioRecoverySnapshot,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RuntimeAudioRecoveryState {
    burst_started_at: Option<Instant>,
    consecutive_late: u8,
    recovery_attempts: u8,
}

impl RuntimeAudioRecoveryState {
    pub(crate) const fn new() -> Self {
        Self {
            burst_started_at: None,
            consecutive_late: 0,
            recovery_attempts: 0,
        }
    }

    pub(crate) fn note_late(&mut self, now: Instant) -> RuntimeAudioRecoveryDecision {
        #[cfg(not(any(test, codex_host_test)))]
        let reset_window = self
            .burst_started_at
            .is_none_or(|started| now >= started + AUDIO_RUNTIME_LATE_RECOVERY_WINDOW);

        #[cfg(any(test, codex_host_test))]
        let reset_window = self.burst_started_at.map_or(true, |started| {
            now.duration_since(started) >= AUDIO_RUNTIME_LATE_RECOVERY_WINDOW
        });
        if reset_window {
            self.burst_started_at = Some(now);
            self.consecutive_late = 1;
            self.recovery_attempts = 0;
            return self.start_recovery(true);
        }

        self.consecutive_late = self.consecutive_late.saturating_add(1);
        self.start_recovery(false)
    }

    pub(crate) fn note_healthy_refill(&mut self) -> Option<RuntimeAudioRecoverySnapshot> {
        let snapshot = self.snapshot_if_active()?;
        self.clear();
        Some(snapshot)
    }

    pub(crate) fn snapshot_if_active(&self) -> Option<RuntimeAudioRecoverySnapshot> {
        self.burst_started_at.map(|_| RuntimeAudioRecoverySnapshot {
            consecutive_late: self.consecutive_late,
            recovery_attempts: self.recovery_attempts,
        })
    }

    pub(crate) fn clear(&mut self) {
        self.burst_started_at = None;
        self.consecutive_late = 0;
        self.recovery_attempts = 0;
    }

    fn start_recovery(&mut self, first_in_burst: bool) -> RuntimeAudioRecoveryDecision {
        if self.recovery_attempts >= AUDIO_RUNTIME_LATE_MAX_RECOVERY_ATTEMPTS {
            return RuntimeAudioRecoveryDecision::Disable {
                snapshot: self
                    .snapshot_if_active()
                    .expect("recovery snapshot must exist while burst is active"),
            };
        }

        self.recovery_attempts = self.recovery_attempts.saturating_add(1);
        RuntimeAudioRecoveryDecision::AttemptRecover {
            first_in_burst,
            snapshot: self
                .snapshot_if_active()
                .expect("recovery snapshot must exist while burst is active"),
        }
    }
}
