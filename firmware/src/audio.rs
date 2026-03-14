use esp_hal::time::Instant;

#[derive(defmt::Format, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioCue {
    BootStartup,
    MainsPresentDc,
    ChargeStarted,
    ChargeCompleted,
    ShutdownModeEntered,
    MainsAbsentDc,
    HighStress,
    BatteryLowNoMains,
    BatteryLowWithMains,
    ShutdownProtection,
    IoOverVoltage,
    IoOverCurrent,
    IoOverPower,
    ModuleFault,
    BatteryProtection,
}

pub const AUDIO_CUE_COUNT: usize = 15;
pub const AUDIO_CUE_LABELS: [&str; AUDIO_CUE_COUNT] = [
    "BOOT STARTUP",
    "MAINS PRESENT DC",
    "CHARGE STARTED",
    "CHARGE COMPLETED",
    "SHUTDOWN MODE ENTERED",
    "MAINS ABSENT DC",
    "HIGH STRESS",
    "BATTERY LOW NO MAINS",
    "BATTERY LOW WITH MAINS",
    "SHUTDOWN PROTECTION",
    "IO OVER VOLTAGE",
    "IO OVER CURRENT",
    "IO OVER POWER",
    "MODULE FAULT",
    "BATTERY PROTECTION",
];

pub const PLAYBACK_SAMPLE_RATE_HZ: u32 = 8_000;
pub const WARNING_INTERVAL_MS: u32 = 2_000;
const SOURCE_SAMPLE_RATE_HZ: u32 = 44_100;
const TRANSITION_RAMP_SAMPLES: u16 = (PLAYBACK_SAMPLE_RATE_HZ / 200) as u16; // ~5 ms
const RESAMPLE_STEP_Q16: u32 =
    ((SOURCE_SAMPLE_RATE_HZ as u64 * 65_536u64) / PLAYBACK_SAMPLE_RATE_HZ as u64) as u32;
const QUEUE_CAPACITY: usize = 16;

const WAV_BOOT_STARTUP: &[u8] = include_bytes!("../assets/audio/test-fw-cues/boot_startup.wav");
const WAV_MAINS_PRESENT_DC: &[u8] =
    include_bytes!("../assets/audio/test-fw-cues/mains_present_dc.wav");
const WAV_CHARGE_STARTED: &[u8] = include_bytes!("../assets/audio/test-fw-cues/charge_started.wav");
const WAV_CHARGE_COMPLETED: &[u8] =
    include_bytes!("../assets/audio/test-fw-cues/charge_completed.wav");
const WAV_SHUTDOWN_MODE_ENTERED: &[u8] =
    include_bytes!("../assets/audio/test-fw-cues/shutdown_mode_entered.wav");
const WAV_MAINS_ABSENT_DC: &[u8] =
    include_bytes!("../assets/audio/test-fw-cues/mains_absent_dc.wav");
const WAV_HIGH_STRESS: &[u8] = include_bytes!("../assets/audio/test-fw-cues/high_stress.wav");
const WAV_BATTERY_LOW_NO_MAINS: &[u8] =
    include_bytes!("../assets/audio/test-fw-cues/battery_low_no_mains.wav");
const WAV_BATTERY_LOW_WITH_MAINS: &[u8] =
    include_bytes!("../assets/audio/test-fw-cues/battery_low_with_mains.wav");
const WAV_SHUTDOWN_PROTECTION: &[u8] =
    include_bytes!("../assets/audio/test-fw-cues/shutdown_protection.wav");
const WAV_IO_OVER_VOLTAGE: &[u8] =
    include_bytes!("../assets/audio/test-fw-cues/io_over_voltage.wav");
const WAV_IO_OVER_CURRENT: &[u8] =
    include_bytes!("../assets/audio/test-fw-cues/io_over_current.wav");
const WAV_IO_OVER_POWER: &[u8] = include_bytes!("../assets/audio/test-fw-cues/io_over_power.wav");
const WAV_MODULE_FAULT: &[u8] = include_bytes!("../assets/audio/test-fw-cues/module_fault.wav");
const WAV_BATTERY_PROTECTION: &[u8] =
    include_bytes!("../assets/audio/test-fw-cues/battery_protection.wav");

impl AudioCue {
    pub fn from_index(idx: usize) -> Option<Self> {
        Some(match idx {
            0 => Self::BootStartup,
            1 => Self::MainsPresentDc,
            2 => Self::ChargeStarted,
            3 => Self::ChargeCompleted,
            4 => Self::ShutdownModeEntered,
            5 => Self::MainsAbsentDc,
            6 => Self::HighStress,
            7 => Self::BatteryLowNoMains,
            8 => Self::BatteryLowWithMains,
            9 => Self::ShutdownProtection,
            10 => Self::IoOverVoltage,
            11 => Self::IoOverCurrent,
            12 => Self::IoOverPower,
            13 => Self::ModuleFault,
            14 => Self::BatteryProtection,
            _ => return None,
        })
    }

    pub const fn index(self) -> usize {
        match self {
            Self::BootStartup => 0,
            Self::MainsPresentDc => 1,
            Self::ChargeStarted => 2,
            Self::ChargeCompleted => 3,
            Self::ShutdownModeEntered => 4,
            Self::MainsAbsentDc => 5,
            Self::HighStress => 6,
            Self::BatteryLowNoMains => 7,
            Self::BatteryLowWithMains => 8,
            Self::ShutdownProtection => 9,
            Self::IoOverVoltage => 10,
            Self::IoOverCurrent => 11,
            Self::IoOverPower => 12,
            Self::ModuleFault => 13,
            Self::BatteryProtection => 14,
        }
    }
}

#[derive(defmt::Format, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AudioPriority {
    Boot = 0,
    Status = 1,
    Warning = 2,
    Error = 3,
}

#[derive(defmt::Format, Clone, Copy, Debug, PartialEq, Eq)]
pub enum CuePlaybackMode {
    OneShot,
    IntervalLoop { interval_ms: u32 },
    ContinuousLoop,
}

#[derive(defmt::Format, Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioRequest {
    pub cue: AudioCue,
    pub priority: AudioPriority,
}

#[derive(defmt::Format, Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioStatus {
    pub playing: bool,
    pub current: Option<AudioCue>,
    pub queued: u8,
    pub dropped: u32,
    pub preempted: u32,
}

#[derive(Clone, Copy, Debug)]
struct ActivePlayback {
    request: AudioRequest,
    pcm: &'static [u8],
    source_pos_q16: u32,
    fade_in_samples_remaining: u16,
}

#[derive(Clone, Copy, Debug)]
struct CueLoopState {
    active: bool,
    next_due_at: Option<Instant>,
}

impl CueLoopState {
    const INACTIVE: Self = Self {
        active: false,
        next_due_at: None,
    };
}

#[derive(Clone, Copy)]
struct WavView {
    audio_format: u16,
    channels: u16,
    sample_rate_hz: u32,
    bits_per_sample: u16,
    data: &'static [u8],
}

pub struct AudioManager {
    current: Option<ActivePlayback>,
    queue: [Option<AudioRequest>; QUEUE_CAPACITY],
    queue_head: usize,
    queue_len: usize,
    dropped: u32,
    preempted: u32,
    loops: [CueLoopState; AUDIO_CUE_COUNT],
    last_output_sample: i16,
    bridge_from_sample: i16,
    bridge_samples_remaining: u16,
}

impl AudioManager {
    pub const fn new() -> Self {
        Self {
            current: None,
            queue: [None; QUEUE_CAPACITY],
            queue_head: 0,
            queue_len: 0,
            dropped: 0,
            preempted: 0,
            loops: [CueLoopState::INACTIVE; AUDIO_CUE_COUNT],
            last_output_sample: 0,
            bridge_from_sample: 0,
            bridge_samples_remaining: 0,
        }
    }

    pub fn request(&mut self, request: AudioRequest) {
        if let Some(current) = self.current {
            if request.priority > current.request.priority {
                let preempted = current.request;
                self.preempted = self.preempted.saturating_add(1);
                self.current = Some(Self::start_playback(request));
                self.requeue_preempted_loop(preempted);
                return;
            }
            if !self.enqueue(request) {
                self.dropped = self.dropped.saturating_add(1);
            }
            return;
        }
        self.current = Some(Self::start_playback(request));
    }

    pub fn request_cue(&mut self, cue: AudioCue) {
        self.request(default_request(cue));
    }

    pub fn trigger(&mut self, cue: AudioCue) {
        self.request_cue(cue);
    }

    pub fn set_cue_active(&mut self, cue: AudioCue, active: bool, now: Instant) {
        let idx = cue.index();
        let was_active = self.loops[idx].active;
        match playback_mode_for_cue(cue) {
            CuePlaybackMode::OneShot => {
                self.loops[idx].active = active;
                if active && !was_active {
                    self.request_cue(cue);
                } else if !active {
                    self.loops[idx].next_due_at = None;
                }
            }
            CuePlaybackMode::ContinuousLoop => {
                if active {
                    if !was_active {
                        self.loops[idx].active = true;
                        self.loops[idx].next_due_at = Some(now);
                    }
                } else if was_active {
                    self.stop_cue(cue);
                }
            }
            CuePlaybackMode::IntervalLoop { .. } => {
                if active {
                    if !was_active {
                        self.loops[idx].active = true;
                        self.loops[idx].next_due_at = Some(now);
                    }
                } else if was_active {
                    self.stop_cue(cue);
                }
            }
        }
    }

    pub fn stop_cue(&mut self, cue: AudioCue) {
        self.loops[cue.index()] = CueLoopState::INACTIVE;
        if self.current.map(|current| current.request.cue) == Some(cue) {
            self.current = None;
        }
        self.remove_queued_cue(cue);
    }

    pub fn tick(&mut self, now: Instant) {
        self.tick_loops(now);
        if self.current.is_none() {
            self.promote_next();
        }
    }

    pub fn stop(&mut self) {
        self.current = None;
        self.queue = [None; QUEUE_CAPACITY];
        self.queue_head = 0;
        self.queue_len = 0;
        self.loops = [CueLoopState::INACTIVE; AUDIO_CUE_COUNT];
        self.last_output_sample = 0;
        self.bridge_from_sample = 0;
        self.bridge_samples_remaining = 0;
    }

    pub fn arm_transition_bridge(&mut self) {
        self.bridge_from_sample = self.last_output_sample;
        self.bridge_samples_remaining = TRANSITION_RAMP_SAMPLES;
    }

    pub fn is_cue_active(&self, cue: AudioCue) -> bool {
        self.loops[cue.index()].active
    }

    pub fn status(&self) -> AudioStatus {
        AudioStatus {
            playing: self.current.is_some(),
            current: self.current.map(|v| v.request.cue),
            queued: self.queue_len as u8,
            dropped: self.dropped,
            preempted: self.preempted,
        }
    }

    pub fn fill(&mut self, buf: &mut [u8]) -> usize {
        let want = buf.len() & !0x3;
        if want == 0 {
            return 0;
        }

        let mut out = 0usize;
        while out < want {
            if self.bridge_samples_remaining > 0 {
                let remaining = i32::from(self.bridge_samples_remaining);
                let total = i32::from(TRANSITION_RAMP_SAMPLES.max(1));
                let sample = (i32::from(self.bridge_from_sample) * remaining) / total;
                self.bridge_samples_remaining -= 1;
                self.last_output_sample = sample as i16;
                let [lo, hi] = (sample as i16).to_le_bytes();
                buf[out] = lo;
                buf[out + 1] = hi;
                buf[out + 2] = lo;
                buf[out + 3] = hi;
                out += 4;
                continue;
            }
            if self.current.is_none() {
                self.promote_next();
                if self.current.is_none() {
                    for b in &mut buf[out..want] {
                        *b = 0;
                    }
                    self.last_output_sample = 0;
                    return want;
                }
            }

            let sample = {
                let active = self
                    .current
                    .as_mut()
                    .expect("audio playback must exist after promote");
                next_mono_sample(active)
            };
            let Some(sample) = sample else {
                self.current = None;
                continue;
            };
            self.last_output_sample = sample;
            let [lo, hi] = sample.to_le_bytes();
            buf[out] = lo;
            buf[out + 1] = hi;
            buf[out + 2] = lo;
            buf[out + 3] = hi;
            out += 4;
        }

        out
    }

    fn tick_loops(&mut self, now: Instant) {
        for cue_idx in 0..AUDIO_CUE_COUNT {
            let cue = AudioCue::from_index(cue_idx).expect("cue index must stay valid");
            let state = self.loops[cue_idx];
            if !state.active {
                continue;
            }
            let due = state.next_due_at.is_none_or(|deadline| now >= deadline);
            if !due || self.has_queued_or_current(cue) {
                continue;
            }
            self.request_cue(cue);
            self.loops[cue_idx].next_due_at = match playback_mode_for_cue(cue) {
                CuePlaybackMode::OneShot => None,
                CuePlaybackMode::ContinuousLoop => Some(now),
                CuePlaybackMode::IntervalLoop { interval_ms } => {
                    Some(now + esp_hal::time::Duration::from_millis(interval_ms as u64))
                }
            };
        }
    }

    fn start_playback(request: AudioRequest) -> ActivePlayback {
        defmt::info!(
            "audio: start_playback cue={=?} priority={=?}",
            request.cue,
            request.priority
        );
        ActivePlayback {
            request,
            pcm: pcm_for_cue(request.cue),
            source_pos_q16: 0,
            fade_in_samples_remaining: TRANSITION_RAMP_SAMPLES,
        }
    }

    fn enqueue(&mut self, request: AudioRequest) -> bool {
        if self.queue_len >= QUEUE_CAPACITY {
            return false;
        }

        let mut insert_at = self.queue_len;
        let mut i = 0usize;
        while i < self.queue_len {
            let idx = (self.queue_head + i) % QUEUE_CAPACITY;
            let queued = self.queue[idx].expect("queue slot must be populated");
            if queued.priority < request.priority {
                insert_at = i;
                break;
            }
            i += 1;
        }

        let mut move_idx = self.queue_len;
        while move_idx > insert_at {
            let from = (self.queue_head + move_idx - 1) % QUEUE_CAPACITY;
            let to = (self.queue_head + move_idx) % QUEUE_CAPACITY;
            self.queue[to] = self.queue[from];
            move_idx -= 1;
        }

        let slot = (self.queue_head + insert_at) % QUEUE_CAPACITY;
        self.queue[slot] = Some(request);
        self.queue_len += 1;
        true
    }

    fn promote_next(&mut self) {
        if self.queue_len == 0 {
            return;
        }
        let req = self.queue[self.queue_head];
        self.queue[self.queue_head] = None;
        self.queue_head = (self.queue_head + 1) % QUEUE_CAPACITY;
        self.queue_len -= 1;
        if let Some(request) = req {
            self.current = Some(Self::start_playback(request));
        }
    }

    fn has_queued_or_current(&self, cue: AudioCue) -> bool {
        if self.current.map(|current| current.request.cue) == Some(cue) {
            return true;
        }
        let mut idx = 0usize;
        while idx < self.queue_len {
            let slot = (self.queue_head + idx) % QUEUE_CAPACITY;
            if self.queue[slot].map(|request| request.cue) == Some(cue) {
                return true;
            }
            idx += 1;
        }
        false
    }

    fn remove_queued_cue(&mut self, cue: AudioCue) {
        if self.queue_len == 0 {
            return;
        }
        let mut next = [None; QUEUE_CAPACITY];
        let mut kept_len = 0usize;
        let mut idx = 0usize;
        while idx < self.queue_len {
            let slot = (self.queue_head + idx) % QUEUE_CAPACITY;
            let request = self.queue[slot].take();
            if request.map(|value| value.cue) != Some(cue) {
                next[kept_len] = request;
                kept_len += 1;
            }
            idx += 1;
        }
        self.queue = next;
        self.queue_head = 0;
        self.queue_len = kept_len;
    }

    fn requeue_preempted_loop(&mut self, request: AudioRequest) {
        if matches!(playback_mode_for_cue(request.cue), CuePlaybackMode::OneShot) {
            return;
        }
        if !self.loops[request.cue.index()].active {
            return;
        }
        if self.has_queued_or_current(request.cue) {
            return;
        }
        if !self.enqueue(request) {
            self.dropped = self.dropped.saturating_add(1);
        }
    }
}

impl Default for AudioManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use esp_hal::time::Duration;

    fn drain_current(manager: &mut AudioManager) {
        let mut buf = [0u8; 512];
        let mut attempts = 0usize;
        while manager.status().playing {
            manager.fill(&mut buf);
            attempts += 1;
            assert!(attempts < 8_192, "audio playback did not drain");
        }
    }

    #[test]
    fn warning_loop_keeps_interval_during_steady_state_updates() {
        let cue = AudioCue::HighStress;
        let start = Instant::EPOCH;
        let early = start + Duration::from_millis(500);
        let due = start + Duration::from_millis(WARNING_INTERVAL_MS as u64);

        let mut manager = AudioManager::new();
        manager.set_cue_active(cue, true, start);
        manager.tick(start);
        assert_eq!(manager.status().current, Some(cue));

        drain_current(&mut manager);
        assert!(!manager.status().playing);

        manager.set_cue_active(cue, true, early);
        manager.tick(early);
        assert!(!manager.status().playing);

        manager.tick(due);
        assert_eq!(manager.status().current, Some(cue));
    }

    #[test]
    fn preempted_loop_cue_resumes_without_waiting_for_next_interval() {
        let warning = AudioCue::HighStress;
        let error = AudioCue::ModuleFault;
        let now = Instant::EPOCH;

        let mut manager = AudioManager::new();
        manager.set_cue_active(warning, true, now);
        manager.tick(now);
        assert_eq!(manager.status().current, Some(warning));

        manager.set_cue_active(error, true, now);
        manager.tick(now);
        assert_eq!(manager.status().current, Some(error));
        assert!(manager.has_queued_or_current(warning));

        manager.set_cue_active(error, false, now);
        manager.tick(now);
        assert_eq!(manager.status().current, Some(warning));
    }

    #[test]
    fn continuous_loop_wraps_without_retrigger() {
        let cue = AudioCue::BatteryProtection;
        let now = Instant::EPOCH;

        let mut manager = AudioManager::new();
        manager.set_cue_active(cue, true, now);
        manager.tick(now);
        assert_eq!(manager.status().current, Some(cue));

        let mut buf = [0u8; 65_536];
        let filled = manager.fill(&mut buf);
        assert_eq!(filled, buf.len());
        assert_eq!(manager.status().current, Some(cue));
        assert!(manager.is_cue_active(cue));
        assert_eq!(manager.status().queued, 0);
    }
}

pub const fn default_request(cue: AudioCue) -> AudioRequest {
    AudioRequest {
        cue,
        priority: priority_for_cue(cue),
    }
}

pub const fn priority_for_cue(cue: AudioCue) -> AudioPriority {
    match cue {
        AudioCue::BootStartup => AudioPriority::Boot,
        AudioCue::MainsPresentDc
        | AudioCue::ChargeStarted
        | AudioCue::ChargeCompleted
        | AudioCue::ShutdownModeEntered => AudioPriority::Status,
        AudioCue::MainsAbsentDc
        | AudioCue::HighStress
        | AudioCue::BatteryLowNoMains
        | AudioCue::BatteryLowWithMains => AudioPriority::Warning,
        AudioCue::ShutdownProtection
        | AudioCue::IoOverVoltage
        | AudioCue::IoOverCurrent
        | AudioCue::IoOverPower
        | AudioCue::ModuleFault
        | AudioCue::BatteryProtection => AudioPriority::Error,
    }
}

pub const fn playback_mode_for_cue(cue: AudioCue) -> CuePlaybackMode {
    match cue {
        AudioCue::BootStartup
        | AudioCue::MainsPresentDc
        | AudioCue::ChargeStarted
        | AudioCue::ChargeCompleted
        | AudioCue::ShutdownModeEntered => CuePlaybackMode::OneShot,
        AudioCue::MainsAbsentDc
        | AudioCue::HighStress
        | AudioCue::BatteryLowNoMains
        | AudioCue::BatteryLowWithMains => CuePlaybackMode::IntervalLoop {
            interval_ms: WARNING_INTERVAL_MS,
        },
        AudioCue::ShutdownProtection
        | AudioCue::IoOverVoltage
        | AudioCue::IoOverCurrent
        | AudioCue::IoOverPower
        | AudioCue::ModuleFault
        | AudioCue::BatteryProtection => CuePlaybackMode::ContinuousLoop,
    }
}

fn pcm_for_cue(cue: AudioCue) -> &'static [u8] {
    let wav = match cue {
        AudioCue::BootStartup => WAV_BOOT_STARTUP,
        AudioCue::MainsPresentDc => WAV_MAINS_PRESENT_DC,
        AudioCue::ChargeStarted => WAV_CHARGE_STARTED,
        AudioCue::ChargeCompleted => WAV_CHARGE_COMPLETED,
        AudioCue::ShutdownModeEntered => WAV_SHUTDOWN_MODE_ENTERED,
        AudioCue::MainsAbsentDc => WAV_MAINS_ABSENT_DC,
        AudioCue::HighStress => WAV_HIGH_STRESS,
        AudioCue::BatteryLowNoMains => WAV_BATTERY_LOW_NO_MAINS,
        AudioCue::BatteryLowWithMains => WAV_BATTERY_LOW_WITH_MAINS,
        AudioCue::ShutdownProtection => WAV_SHUTDOWN_PROTECTION,
        AudioCue::IoOverVoltage => WAV_IO_OVER_VOLTAGE,
        AudioCue::IoOverCurrent => WAV_IO_OVER_CURRENT,
        AudioCue::IoOverPower => WAV_IO_OVER_POWER,
        AudioCue::ModuleFault => WAV_MODULE_FAULT,
        AudioCue::BatteryProtection => WAV_BATTERY_PROTECTION,
    };
    parse_wav_pcm16le_mono(wav)
}

fn next_mono_sample(active: &mut ActivePlayback) -> Option<i16> {
    let sample_count = active.pcm.len() / 2;
    if sample_count == 0 {
        return None;
    }
    let continuous_loop = matches!(
        playback_mode_for_cue(active.request.cue),
        CuePlaybackMode::ContinuousLoop
    );
    let idx = (active.source_pos_q16 >> 16) as usize;
    if idx >= sample_count {
        if !continuous_loop {
            active.source_pos_q16 = (sample_count as u32) << 16;
            return None;
        }
        active.source_pos_q16 = 0;
    }
    let idx = (active.source_pos_q16 >> 16) as usize;
    let base = idx * 2;
    let lo = active.pcm[base];
    let hi = active.pcm[base + 1];
    active.source_pos_q16 = active.source_pos_q16.saturating_add(RESAMPLE_STEP_Q16);
    let mut sample = i16::from_le_bytes([lo, hi]);
    if active.fade_in_samples_remaining > 0 {
        let total = i32::from(TRANSITION_RAMP_SAMPLES.max(1));
        let progressed = total - i32::from(active.fade_in_samples_remaining) + 1;
        let scaled = (i32::from(sample) * progressed) / total;
        active.fade_in_samples_remaining -= 1;
        sample = scaled as i16;
    }
    Some(sample)
}

fn parse_wav_pcm16le_mono(bytes: &'static [u8]) -> &'static [u8] {
    let Ok(view) = parse_wav_view(bytes) else {
        return &[];
    };
    if view.audio_format != 1 {
        return &[];
    }
    if view.channels != 1 {
        return &[];
    }
    if view.bits_per_sample != 16 {
        return &[];
    }
    if view.sample_rate_hz != SOURCE_SAMPLE_RATE_HZ {
        return &[];
    }
    view.data
}

fn parse_wav_view(bytes: &'static [u8]) -> Result<WavView, ()> {
    if bytes.len() < 44 {
        return Err(());
    }
    if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(());
    }

    let mut fmt: Option<(u16, u16, u32, u16)> = None;
    let mut data: Option<&'static [u8]> = None;
    let mut offset = 12usize;

    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let size = u32::from_le_bytes([
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ]) as usize;
        offset += 8;

        if offset + size > bytes.len() {
            return Err(());
        }

        if id == b"fmt " {
            if size < 16 {
                return Err(());
            }
            fmt = Some((
                u16::from_le_bytes([bytes[offset], bytes[offset + 1]]),
                u16::from_le_bytes([bytes[offset + 2], bytes[offset + 3]]),
                u32::from_le_bytes([
                    bytes[offset + 4],
                    bytes[offset + 5],
                    bytes[offset + 6],
                    bytes[offset + 7],
                ]),
                u16::from_le_bytes([bytes[offset + 14], bytes[offset + 15]]),
            ));
        } else if id == b"data" {
            data = Some(&bytes[offset..offset + size]);
        }

        offset += size + (size % 2);
        if fmt.is_some() && data.is_some() {
            break;
        }
    }

    let (audio_format, channels, sample_rate_hz, bits_per_sample) = fmt.ok_or(())?;
    let data = data.ok_or(())?;
    Ok(WavView {
        audio_format,
        channels,
        sample_rate_hz,
        bits_per_sample,
        data,
    })
}
