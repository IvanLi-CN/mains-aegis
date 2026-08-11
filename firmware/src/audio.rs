use crate::time::{Duration, Instant};

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
    VolumePreview,
    InteractionTouch,
    UsbCInsert,
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
const MAX_GAIN_Q8: u16 = 256;
const GAIN_Q8_LUT: [u16; 7] = [0, 32, 64, 91, 128, 181, MAX_GAIN_Q8];
const DEFAULT_VOLUME_STEP: u8 = 4;
const PREVIEW_HALF_PERIOD_SAMPLES: u32 = PLAYBACK_SAMPLE_RATE_HZ / 1_500;
const PREVIEW_PULSE_SAMPLES: u32 = (PLAYBACK_SAMPLE_RATE_HZ * 110) / 1_000;
const PREVIEW_EDGE_RAMP_SAMPLES: u32 = (PLAYBACK_SAMPLE_RATE_HZ * 6) / 1_000;
const PREVIEW_TOTAL_SAMPLES: u32 = PREVIEW_PULSE_SAMPLES;
const PREVIEW_PEAK_AMPLITUDE: i16 = 10_500;

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
const WAV_INTERACTION_TOUCH: &[u8] =
    include_bytes!("../assets/audio/interaction-cues/interaction_touch.wav");
const WAV_USB_C_INSERT: &[u8] = include_bytes!("../assets/audio/interaction-cues/usb_c_insert.wav");

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
            Self::VolumePreview | Self::InteractionTouch | Self::UsbCInsert => {
                panic!("action cue does not have a runtime loop index")
            }
        }
    }
}

#[derive(defmt::Format, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioRoute {
    Action,
    System,
}

#[derive(defmt::Format, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AudioPriority {
    Boot = 0,
    Status = 1,
    Preview = 2,
    Warning = 3,
    Error = 4,
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
    pub route: AudioRoute,
    pub preview: bool,
}

#[derive(defmt::Format, Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioStatus {
    pub playing: bool,
    pub current: Option<AudioCue>,
    pub current_route: Option<AudioRoute>,
    pub previewing: bool,
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
    immediate_dma_flush_requested: bool,
    action_gain_q8: u16,
    system_gain_q8: u16,
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
            immediate_dma_flush_requested: false,
            action_gain_q8: gain_q8_for_step(DEFAULT_VOLUME_STEP),
            system_gain_q8: gain_q8_for_step(DEFAULT_VOLUME_STEP),
        }
    }

    pub fn request(&mut self, request: AudioRequest) {
        if should_suppress_duplicate_request(request.cue) && self.has_queued_or_current(request.cue)
        {
            return;
        }

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

    pub fn set_action_volume_step(&mut self, step: u8) {
        self.action_gain_q8 = gain_q8_for_step(step);
    }

    pub fn set_system_volume_step(&mut self, step: u8) {
        self.system_gain_q8 = gain_q8_for_step(step);
    }

    pub fn trigger_volume_preview(&mut self, route: AudioRoute) {
        let request = preview_request(route);
        self.remove_queued_previews();
        let interrupted = self.current.map(|current| current.request);
        self.current = Some(Self::start_playback(request));
        if let Some(interrupted) = interrupted {
            self.requeue_preempted_loop(interrupted);
        }
    }

    pub fn trigger_interaction_feedback(&mut self) {
        self.request_cue(AudioCue::InteractionTouch);
    }

    pub fn trigger_usb_c_insert(&mut self) {
        self.request_cue(AudioCue::UsbCInsert);
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

    /// Stop an active cue and ask the runtime transport to replace buffered samples with silence.
    ///
    /// Clearing the manager alone does not affect the already-written circular DMA buffer, so
    /// the main loop consumes this request and re-primes the transport immediately.
    pub fn stop_cue_immediate(&mut self, cue: AudioCue) -> bool {
        let was_current = self.current.map(|current| current.request.cue) == Some(cue);
        self.stop_cue(cue);
        if was_current {
            self.last_output_sample = 0;
            self.bridge_from_sample = 0;
            self.bridge_samples_remaining = 0;
            self.immediate_dma_flush_requested = true;
        }
        was_current
    }

    pub fn take_immediate_dma_flush_request(&mut self) -> bool {
        let requested = self.immediate_dma_flush_requested;
        self.immediate_dma_flush_requested = false;
        requested
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
        self.immediate_dma_flush_requested = false;
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
            current_route: self.current.map(|v| v.request.route),
            previewing: self.current.is_some_and(|v| v.request.preview),
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
            let sample = self.scale_sample(
                sample,
                self.current.expect("playback must exist").request.route,
            );
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
                    Some(now + Duration::from_millis(interval_ms as u64))
                }
            };
        }
    }

    fn start_playback(request: AudioRequest) -> ActivePlayback {
        defmt::info!(
            "audio: start_playback cue={=?} priority={=?} route={=?} preview={=bool}",
            request.cue,
            request.priority,
            request.route,
            request.preview
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
        if request.preview {
            return;
        }
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

    fn scale_sample(&self, sample: i16, route: AudioRoute) -> i16 {
        let gain_q8 = match route {
            AudioRoute::Action => self.action_gain_q8,
            AudioRoute::System => self.system_gain_q8,
        };
        let scaled = (i32::from(sample) * i32::from(gain_q8)) / i32::from(MAX_GAIN_Q8);
        scaled.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
    }

    fn remove_queued_previews(&mut self) {
        if self.queue_len == 0 {
            return;
        }
        let mut next = [None; QUEUE_CAPACITY];
        let mut kept_len = 0usize;
        let mut idx = 0usize;
        while idx < self.queue_len {
            let slot = (self.queue_head + idx) % QUEUE_CAPACITY;
            let request = self.queue[slot].take();
            if !request.is_some_and(|value| value.preview) {
                next[kept_len] = request;
                kept_len += 1;
            }
            idx += 1;
        }
        self.queue = next;
        self.queue_head = 0;
        self.queue_len = kept_len;
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

    fn decode_left_samples(buf: &[u8]) -> heapless::Vec<i16, 16> {
        let mut samples = heapless::Vec::new();
        for frame in buf.chunks_exact(4) {
            let sample = i16::from_le_bytes([frame[0], frame[1]]);
            samples.push(sample).expect("sample buffer must have room");
        }
        samples
    }

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
        let start = Instant::now();
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
        let now = Instant::now();

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
        let now = Instant::now();

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

    #[test]
    fn continuous_loop_wrap_preserves_resample_remainder() {
        let mut active = AudioManager::start_playback(default_request(AudioCue::BatteryProtection));
        active.fade_in_samples_remaining = 0;

        let sample_count = active.pcm.len() / 2;
        let span_q16 = (sample_count as u64) << 16;
        let wrapped_idx = 2usize;
        let wrapped_rem = 63_936u64;
        let overflow_q16 = span_q16 + ((wrapped_idx as u64) << 16) + wrapped_rem;
        active.source_pos_q16 = overflow_q16 as u32;

        let sample = next_mono_sample(&mut active).expect("continuous loop should wrap");
        let base = wrapped_idx * 2;
        let expected = i16::from_le_bytes([active.pcm[base], active.pcm[base + 1]]);
        assert_eq!(sample, expected);
    }

    #[test]
    fn volume_preview_uses_dedicated_internal_cue() {
        let request = preview_request(AudioRoute::Action);

        assert_eq!(request.cue, AudioCue::VolumePreview);
        assert!(request.preview);
        assert_eq!(request.priority, AudioPriority::Preview);
        assert_eq!(playback_mode_for_cue(request.cue), CuePlaybackMode::OneShot);
    }

    #[test]
    fn action_preview_uses_action_gain() {
        let mut manager = AudioManager::new();
        manager.set_action_volume_step(0);
        manager.set_system_volume_step(6);
        manager.trigger_volume_preview(AudioRoute::Action);

        let status = manager.status();
        assert_eq!(status.current, Some(AudioCue::VolumePreview));
        assert_eq!(status.current_route, Some(AudioRoute::Action));
        assert!(status.previewing);

        let mut buf = [0u8; 32];
        let filled = manager.fill(&mut buf);
        assert_eq!(filled, buf.len());
        let samples = decode_left_samples(&buf);
        assert!(samples.iter().all(|sample| *sample == 0));
    }

    #[test]
    fn system_preview_uses_system_gain() {
        let mut manager = AudioManager::new();
        manager.set_action_volume_step(0);
        manager.set_system_volume_step(6);
        manager.trigger_volume_preview(AudioRoute::System);

        let status = manager.status();
        assert_eq!(status.current, Some(AudioCue::VolumePreview));
        assert_eq!(status.current_route, Some(AudioRoute::System));
        assert!(status.previewing);

        let mut buf = [0u8; 32];
        let filled = manager.fill(&mut buf);
        assert_eq!(filled, buf.len());
        let samples = decode_left_samples(&buf);
        assert!(samples.iter().any(|sample| *sample != 0));
    }

    #[test]
    fn volume_preview_preempts_warning_loop_for_immediate_feedback() {
        let now = Instant::now();
        let mut manager = AudioManager::new();
        manager.set_cue_active(AudioCue::HighStress, true, now);
        manager.tick(now);
        assert_eq!(manager.status().current, Some(AudioCue::HighStress));

        manager.trigger_volume_preview(AudioRoute::Action);
        let status = manager.status();
        assert_eq!(status.current, Some(AudioCue::VolumePreview));
        assert_eq!(status.current_route, Some(AudioRoute::Action));
        assert!(status.previewing);
    }

    #[test]
    fn volume_preview_is_audible_near_start_of_large_prefill() {
        let mut manager = AudioManager::new();
        manager.set_action_volume_step(6);
        manager.trigger_volume_preview(AudioRoute::Action);

        let mut buf = [0u8; 4096];
        let filled = manager.fill(&mut buf);
        assert_eq!(filled, buf.len());
        assert!(buf[..512]
            .chunks_exact(4)
            .any(|frame| { i16::from_le_bytes([frame[0], frame[1]]) != 0 }));
    }

    #[test]
    fn volume_preview_is_single_contiguous_pulse() {
        let mut active = ActivePlayback {
            request: preview_request(AudioRoute::Action),
            pcm: &[],
            source_pos_q16: 0,
            fade_in_samples_remaining: 0,
        };

        let mut samples = [0i16; PREVIEW_TOTAL_SAMPLES as usize];
        for sample in samples.iter_mut() {
            *sample = next_preview_sample(&mut active).expect("preview should still be active");
        }
        assert_eq!(next_preview_sample(&mut active), None);
        assert_eq!(PREVIEW_TOTAL_SAMPLES, PREVIEW_PULSE_SAMPLES);
        assert!(samples.iter().all(|sample| *sample != 0));
    }

    #[test]
    fn interaction_cues_use_action_route() {
        for cue in [AudioCue::InteractionTouch, AudioCue::UsbCInsert] {
            let request = default_request(cue);
            assert_eq!(request.route, AudioRoute::Action);
            assert_eq!(request.priority, AudioPriority::Preview);
            assert_eq!(playback_mode_for_cue(cue), CuePlaybackMode::OneShot);
        }
    }

    #[test]
    fn interaction_assets_decode_to_pcm() {
        for cue in [AudioCue::InteractionTouch, AudioCue::UsbCInsert] {
            assert!(!pcm_for_cue(cue).is_empty());
        }
    }

    #[test]
    fn interaction_feedback_uses_action_gain() {
        let mut manager = AudioManager::new();
        manager.set_action_volume_step(0);
        manager.set_system_volume_step(6);
        manager.trigger_interaction_feedback();

        let status = manager.status();
        assert_eq!(status.current, Some(AudioCue::InteractionTouch));
        assert_eq!(status.current_route, Some(AudioRoute::Action));

        let mut buf = [0u8; 32];
        let filled = manager.fill(&mut buf);
        assert_eq!(filled, buf.len());
        let samples = decode_left_samples(&buf);
        assert!(samples.iter().all(|sample| *sample == 0));
    }

    #[test]
    fn usb_c_insert_feedback_ignores_duplicate_while_active() {
        let mut manager = AudioManager::new();
        manager.trigger_usb_c_insert();
        assert_eq!(manager.status().current, Some(AudioCue::UsbCInsert));

        manager.trigger_usb_c_insert();
        manager.trigger_usb_c_insert();

        let status = manager.status();
        assert_eq!(status.current, Some(AudioCue::UsbCInsert));
        assert_eq!(status.queued, 0);
        assert_eq!(status.dropped, 0);
    }

    #[test]
    fn immediate_stop_requests_dma_flush_and_outputs_silence() {
        let mut manager = AudioManager::new();
        manager.trigger(AudioCue::HighStress);
        let mut active_buf = [0u8; 4096];
        manager.fill(&mut active_buf);
        assert!(manager.status().playing);
        assert!(manager.stop_cue_immediate(AudioCue::HighStress));
        assert!(manager.take_immediate_dma_flush_request());

        let mut silent_buf = [0xA5u8; 4096];
        manager.fill(&mut silent_buf);
        assert!(silent_buf.iter().all(|sample| *sample == 0));
        assert!(!manager.status().playing);
        assert!(!manager.take_immediate_dma_flush_request());
    }
}

pub const fn default_request(cue: AudioCue) -> AudioRequest {
    AudioRequest {
        cue,
        priority: priority_for_cue(cue),
        route: route_for_cue(cue),
        preview: false,
    }
}

pub const fn preview_request(route: AudioRoute) -> AudioRequest {
    AudioRequest {
        cue: AudioCue::VolumePreview,
        priority: AudioPriority::Preview,
        route,
        preview: true,
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
        AudioCue::VolumePreview | AudioCue::InteractionTouch | AudioCue::UsbCInsert => {
            AudioPriority::Preview
        }
    }
}

pub const fn route_for_cue(cue: AudioCue) -> AudioRoute {
    match cue {
        AudioCue::VolumePreview | AudioCue::InteractionTouch | AudioCue::UsbCInsert => {
            AudioRoute::Action
        }
        _ => AudioRoute::System,
    }
}

pub const fn gain_q8_for_step(step: u8) -> u16 {
    match step {
        0 => GAIN_Q8_LUT[0],
        1 => GAIN_Q8_LUT[1],
        2 => GAIN_Q8_LUT[2],
        3 => GAIN_Q8_LUT[3],
        4 => GAIN_Q8_LUT[4],
        5 => GAIN_Q8_LUT[5],
        _ => GAIN_Q8_LUT[6],
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
        AudioCue::VolumePreview | AudioCue::InteractionTouch | AudioCue::UsbCInsert => {
            CuePlaybackMode::OneShot
        }
    }
}

const fn should_suppress_duplicate_request(cue: AudioCue) -> bool {
    matches!(cue, AudioCue::UsbCInsert)
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
        AudioCue::InteractionTouch => WAV_INTERACTION_TOUCH,
        AudioCue::UsbCInsert => WAV_USB_C_INSERT,
        AudioCue::VolumePreview => return &[],
    };
    parse_wav_pcm16le_mono(wav)
}

fn next_mono_sample(active: &mut ActivePlayback) -> Option<i16> {
    if active.request.preview {
        let mut sample = next_preview_sample(active)?;
        if active.fade_in_samples_remaining > 0 {
            let total = i32::from(TRANSITION_RAMP_SAMPLES.max(1));
            let progressed = total - i32::from(active.fade_in_samples_remaining) + 1;
            let scaled = (i32::from(sample) * progressed) / total;
            active.fade_in_samples_remaining -= 1;
            sample = scaled as i16;
        }
        return Some(sample);
    }

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
        let sample_span_q16 = (sample_count as u64) << 16;
        active.source_pos_q16 = ((active.source_pos_q16 as u64) % sample_span_q16) as u32;
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

fn next_preview_sample(active: &mut ActivePlayback) -> Option<i16> {
    let sample_index = active.source_pos_q16;
    if sample_index >= PREVIEW_TOTAL_SAMPLES {
        return None;
    }
    active.source_pos_q16 = active.source_pos_q16.saturating_add(1);

    let edge_ramp = PREVIEW_EDGE_RAMP_SAMPLES.max(1);
    let attack = sample_index.saturating_add(1).min(edge_ramp);
    let release = PREVIEW_PULSE_SAMPLES
        .saturating_sub(sample_index)
        .min(edge_ramp);
    let envelope = attack.min(release);
    let amplitude = (i32::from(PREVIEW_PEAK_AMPLITUDE) * envelope as i32) / edge_ramp as i32;
    let polarity = if (sample_index / PREVIEW_HALF_PERIOD_SAMPLES.max(1)) % 2 == 0 {
        1
    } else {
        -1
    };
    Some((amplitude * polarity) as i16)
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
