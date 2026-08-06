pub const ABNORMAL_BOOT_THRESHOLD: u8 = 3;
pub const STABLE_RUNTIME_MS: u32 = 30_000;
pub const RECORD_LEN: usize = 16;

use core::sync::atomic::{AtomicU8, Ordering};

static DIAG_RESET: AtomicU8 = AtomicU8::new(ResetCause::Unknown as u8);
static DIAG_PHASE: AtomicU8 = AtomicU8::new(BootPhase::Stabilizing as u8);
static DIAG_ABNORMAL_BOOTS: AtomicU8 = AtomicU8::new(0);
static DIAG_CANDIDATE: AtomicU8 = AtomicU8::new(CandidateState::UnsupportedLayout as u8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ResetCause {
    PowerOn = 0,
    Software = 1,
    Watchdog = 2,
    Brownout = 3,
    ExternalDebug = 4,
    Unknown = 5,
}

impl ResetCause {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PowerOn => "power_on",
            Self::Software => "software",
            Self::Watchdog => "watchdog",
            Self::Brownout => "brownout",
            Self::ExternalDebug => "external_debug",
            Self::Unknown => "unknown",
        }
    }

    const fn is_abnormal(self) -> bool {
        matches!(self, Self::Software | Self::Watchdog | Self::Unknown)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BootPhase {
    Stabilizing = 0,
    Healthy = 1,
    SafeMode = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CandidateState {
    UnsupportedLayout = 0,
    Confirmed = 1,
    PendingVerify = 2,
    RolledBack = 3,
}

impl CandidateState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedLayout => "unsupported_layout",
            Self::Confirmed => "confirmed",
            Self::PendingVerify => "pending_verify",
            Self::RolledBack => "rolled_back",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootRecord {
    pub generation: u32,
    pub abnormal_boots: u8,
    pub phase: BootPhase,
    pub last_reset: ResetCause,
    pub candidate: CandidateState,
}

impl BootRecord {
    pub const fn fresh(reset: ResetCause) -> Self {
        Self {
            generation: 1,
            abnormal_boots: 0,
            phase: BootPhase::Stabilizing,
            last_reset: reset,
            candidate: CandidateState::UnsupportedLayout,
        }
    }

    pub fn begin_boot(previous: Option<Self>, reset: ResetCause) -> Self {
        if matches!(reset, ResetCause::PowerOn | ResetCause::Brownout) {
            return Self::fresh(reset);
        }
        let mut next = previous.unwrap_or_else(|| Self::fresh(reset));
        next.generation = next.generation.wrapping_add(1);
        if next.phase != BootPhase::Healthy && reset.is_abnormal() {
            next.abnormal_boots = next.abnormal_boots.saturating_add(1);
        }
        next.last_reset = reset;
        next.phase = if next.abnormal_boots >= ABNORMAL_BOOT_THRESHOLD {
            BootPhase::SafeMode
        } else {
            BootPhase::Stabilizing
        };
        next
    }

    pub fn mark_healthy(mut self) -> Self {
        self.generation = self.generation.wrapping_add(1);
        self.abnormal_boots = 0;
        self.phase = BootPhase::Healthy;
        if self.candidate == CandidateState::PendingVerify {
            self.candidate = CandidateState::Confirmed;
        }
        self
    }

    pub fn clear_safe_mode_for_confirmed_recovery(mut self) -> Self {
        self.generation = self.generation.wrapping_add(1);
        self.abnormal_boots = 0;
        self.phase = BootPhase::Stabilizing;
        self.candidate = CandidateState::Confirmed;
        self
    }

    pub fn record_failed_candidate_boot(mut self) -> Self {
        if self.candidate == CandidateState::PendingVerify {
            self.generation = self.generation.wrapping_add(1);
            self.candidate = CandidateState::RolledBack;
        }
        self
    }

    pub const fn safe_mode(self) -> bool {
        matches!(self.phase, BootPhase::SafeMode)
    }

    pub fn encode(self) -> [u8; RECORD_LEN] {
        let mut out = [0u8; RECORD_LEN];
        out[0..4].copy_from_slice(b"MAFR");
        out[4] = 1;
        out[5] = self.abnormal_boots;
        out[6] = self.phase as u8;
        out[7] = self.last_reset as u8;
        out[8] = self.candidate as u8;
        out[9..13].copy_from_slice(&self.generation.to_le_bytes());
        let crc = crc16(&out[..14]);
        out[14..16].copy_from_slice(&crc.to_le_bytes());
        out
    }

    pub fn decode(bytes: [u8; RECORD_LEN]) -> Option<Self> {
        if &bytes[0..4] != b"MAFR" || bytes[4] != 1 {
            return None;
        }
        if u16::from_le_bytes([bytes[14], bytes[15]]) != crc16(&bytes[..14]) {
            return None;
        }
        Some(Self {
            generation: u32::from_le_bytes(bytes[9..13].try_into().ok()?),
            abnormal_boots: bytes[5],
            phase: match bytes[6] {
                0 => BootPhase::Stabilizing,
                1 => BootPhase::Healthy,
                2 => BootPhase::SafeMode,
                _ => return None,
            },
            last_reset: match bytes[7] {
                0 => ResetCause::PowerOn,
                1 => ResetCause::Software,
                2 => ResetCause::Watchdog,
                3 => ResetCause::Brownout,
                4 => ResetCause::ExternalDebug,
                5 => ResetCause::Unknown,
                _ => return None,
            },
            candidate: match bytes[8] {
                0 => CandidateState::UnsupportedLayout,
                1 => CandidateState::Confirmed,
                2 => CandidateState::PendingVerify,
                3 => CandidateState::RolledBack,
                _ => return None,
            },
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootDiagnostics {
    pub reset: ResetCause,
    pub phase: BootPhase,
    pub abnormal_boots: u8,
    pub candidate: CandidateState,
}

impl BootDiagnostics {
    pub const fn phase_str(self) -> &'static str {
        match self.phase {
            BootPhase::Stabilizing => "stabilizing",
            BootPhase::Healthy => "healthy",
            BootPhase::SafeMode => "safe_mode",
        }
    }
}

pub fn publish_diagnostics(record: BootRecord) {
    DIAG_RESET.store(record.last_reset as u8, Ordering::Release);
    DIAG_PHASE.store(record.phase as u8, Ordering::Release);
    DIAG_ABNORMAL_BOOTS.store(record.abnormal_boots, Ordering::Release);
    DIAG_CANDIDATE.store(record.candidate as u8, Ordering::Release);
}

pub fn diagnostics() -> BootDiagnostics {
    BootDiagnostics {
        reset: decode_reset(DIAG_RESET.load(Ordering::Acquire)),
        phase: decode_phase(DIAG_PHASE.load(Ordering::Acquire)),
        abnormal_boots: DIAG_ABNORMAL_BOOTS.load(Ordering::Acquire),
        candidate: decode_candidate(DIAG_CANDIDATE.load(Ordering::Acquire)),
    }
}

const fn decode_reset(raw: u8) -> ResetCause {
    match raw {
        0 => ResetCause::PowerOn,
        1 => ResetCause::Software,
        2 => ResetCause::Watchdog,
        3 => ResetCause::Brownout,
        4 => ResetCause::ExternalDebug,
        _ => ResetCause::Unknown,
    }
}

const fn decode_phase(raw: u8) -> BootPhase {
    match raw {
        1 => BootPhase::Healthy,
        2 => BootPhase::SafeMode,
        _ => BootPhase::Stabilizing,
    }
}

const fn decode_candidate(raw: u8) -> CandidateState {
    match raw {
        1 => CandidateState::Confirmed,
        2 => CandidateState::PendingVerify,
        3 => CandidateState::RolledBack,
        _ => CandidateState::UnsupportedLayout,
    }
}

pub fn newest_valid(slots: [[u8; RECORD_LEN]; 2]) -> Option<BootRecord> {
    match (BootRecord::decode(slots[0]), BootRecord::decode(slots[1])) {
        (Some(a), Some(b)) => Some(if b.generation.wrapping_sub(a.generation) < u32::MAX / 2 {
            b
        } else {
            a
        }),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

pub const fn next_slot(record: BootRecord) -> usize {
    (record.generation as usize) & 1
}

fn crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0xffffu16;
    for byte in bytes {
        crc ^= (*byte as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_round_trip_and_corruption_fallback() {
        let record = BootRecord::fresh(ResetCause::PowerOn);
        assert_eq!(BootRecord::decode(record.encode()), Some(record));
        let mut corrupt = record.encode();
        corrupt[5] ^= 1;
        assert_eq!(BootRecord::decode(corrupt), None);
    }

    #[test]
    fn repeated_early_watchdogs_enter_safe_mode() {
        let mut record = BootRecord::fresh(ResetCause::PowerOn);
        for _ in 0..ABNORMAL_BOOT_THRESHOLD {
            record = BootRecord::begin_boot(Some(record), ResetCause::Watchdog);
        }
        assert!(record.safe_mode());
    }

    #[test]
    fn stable_boot_clears_sequence_and_confirms_candidate() {
        let mut record = BootRecord::begin_boot(None, ResetCause::Watchdog);
        record.candidate = CandidateState::PendingVerify;
        record = record.mark_healthy();
        assert_eq!(record.abnormal_boots, 0);
        assert_eq!(record.phase, BootPhase::Healthy);
        assert_eq!(record.candidate, CandidateState::Confirmed);
    }

    #[test]
    fn power_on_and_brownout_start_fresh_sequences() {
        let failing = BootRecord::begin_boot(None, ResetCause::Watchdog);
        assert_eq!(
            BootRecord::begin_boot(Some(failing), ResetCause::PowerOn).abnormal_boots,
            0
        );
        assert_eq!(
            BootRecord::begin_boot(Some(failing), ResetCause::Brownout).abnormal_boots,
            0
        );
    }

    #[test]
    fn newest_valid_ignores_missing_or_corrupt_slot() {
        let a = BootRecord::fresh(ResetCause::PowerOn);
        let b = BootRecord::begin_boot(Some(a), ResetCause::Watchdog);
        assert_eq!(newest_valid([a.encode(), b.encode()]), Some(b));
        assert_eq!(newest_valid([[0; RECORD_LEN], b.encode()]), Some(b));
        assert_eq!(newest_valid([[0; RECORD_LEN]; 2]), None);
    }

    #[test]
    fn confirmed_recovery_exits_safe_mode_through_stabilization() {
        let mut record = BootRecord::fresh(ResetCause::PowerOn);
        record.phase = BootPhase::SafeMode;
        record.abnormal_boots = ABNORMAL_BOOT_THRESHOLD;
        record = record.clear_safe_mode_for_confirmed_recovery();
        assert_eq!(record.phase, BootPhase::Stabilizing);
        assert_eq!(record.abnormal_boots, 0);
        assert_eq!(record.candidate, CandidateState::Confirmed);
    }

    #[test]
    fn failed_pending_candidate_records_rollback_transition() {
        let mut record = BootRecord::fresh(ResetCause::PowerOn);
        record.candidate = CandidateState::PendingVerify;
        assert_eq!(
            record.record_failed_candidate_boot().candidate,
            CandidateState::RolledBack
        );
    }
}
