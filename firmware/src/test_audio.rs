#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioEvent {
    Boot,
    TouchInteraction,
    KeyInteraction,
    Warning,
    Error,
    ModeSwitch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AudioPriority {
    Boot = 0,
    Interaction = 1,
    ModeSwitch = 2,
    Warning = 3,
    Error = 4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioRequest {
    pub event: AudioEvent,
    pub priority: AudioPriority,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioStatus {
    pub playing: bool,
    pub current: Option<AudioEvent>,
    pub queued: u8,
    pub dropped: u32,
    pub preempted: u32,
}

#[derive(Clone, Copy, Debug)]
struct ToneStep {
    freq_hz: u16,
    duration_ms: u16,
}

#[derive(Clone, Copy, Debug)]
struct ActivePlayback {
    request: AudioRequest,
    script: &'static [ToneStep],
    step_index: usize,
    samples_left_in_step: u32,
    phase: u32,
}

const SAMPLE_RATE_HZ: u32 = 8_000;
const AMP: i16 = 9_000;
const QUEUE_CAPACITY: usize = 8;

const STEPS_BOOT: [ToneStep; 2] = [
    ToneStep {
        freq_hz: 523,
        duration_ms: 130,
    },
    ToneStep {
        freq_hz: 659,
        duration_ms: 170,
    },
];
const STEPS_TOUCH: [ToneStep; 1] = [ToneStep {
    freq_hz: 1100,
    duration_ms: 45,
}];
const STEPS_KEY: [ToneStep; 1] = [ToneStep {
    freq_hz: 980,
    duration_ms: 45,
}];
const STEPS_MODE_SWITCH: [ToneStep; 2] = [
    ToneStep {
        freq_hz: 700,
        duration_ms: 75,
    },
    ToneStep {
        freq_hz: 920,
        duration_ms: 85,
    },
];
const STEPS_WARNING: [ToneStep; 3] = [
    ToneStep {
        freq_hz: 760,
        duration_ms: 110,
    },
    ToneStep {
        freq_hz: 0,
        duration_ms: 60,
    },
    ToneStep {
        freq_hz: 760,
        duration_ms: 120,
    },
];
const STEPS_ERROR: [ToneStep; 5] = [
    ToneStep {
        freq_hz: 320,
        duration_ms: 150,
    },
    ToneStep {
        freq_hz: 0,
        duration_ms: 70,
    },
    ToneStep {
        freq_hz: 320,
        duration_ms: 160,
    },
    ToneStep {
        freq_hz: 0,
        duration_ms: 70,
    },
    ToneStep {
        freq_hz: 260,
        duration_ms: 220,
    },
];

pub struct AudioManager {
    current: Option<ActivePlayback>,
    queue: [Option<AudioRequest>; QUEUE_CAPACITY],
    queue_head: usize,
    queue_len: usize,
    dropped: u32,
    preempted: u32,
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
        }
    }

    pub fn request(&mut self, request: AudioRequest) {
        if let Some(current) = self.current {
            if request.priority > current.request.priority {
                self.preempted = self.preempted.saturating_add(1);
                self.current = Some(Self::start_playback(request));
                return;
            }
            if !self.enqueue(request) {
                self.dropped = self.dropped.saturating_add(1);
            }
            return;
        }
        self.current = Some(Self::start_playback(request));
    }

    pub fn request_event(&mut self, event: AudioEvent) {
        self.request(AudioRequest {
            priority: priority_for(event),
            event,
        });
    }

    pub fn tick(&mut self) {
        if self.current.is_none() {
            self.promote_next();
        }
    }

    pub fn stop(&mut self) {
        self.current = None;
        self.queue = [None; QUEUE_CAPACITY];
        self.queue_head = 0;
        self.queue_len = 0;
    }

    pub fn status(&self) -> AudioStatus {
        AudioStatus {
            playing: self.current.is_some(),
            current: self.current.map(|v| v.request.event),
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
            if self.current.is_none() {
                self.promote_next();
                if self.current.is_none() {
                    for b in &mut buf[out..want] {
                        *b = 0;
                    }
                    return want;
                }
            }

            let s0 = self.next_sample_i16();
            let s1 = self.next_sample_i16();
            let [a0, a1] = s0.to_le_bytes();
            let [b0, b1] = s1.to_le_bytes();
            buf[out] = a0;
            buf[out + 1] = a1;
            buf[out + 2] = b0;
            buf[out + 3] = b1;
            out += 4;
        }

        out
    }

    fn next_sample_i16(&mut self) -> i16 {
        let Some(active) = self.current.as_mut() else {
            return 0;
        };

        let step = active.script[active.step_index];
        let sample = if step.freq_hz == 0 {
            0
        } else {
            let inc = ((step.freq_hz as u64) << 32) / (SAMPLE_RATE_HZ as u64);
            active.phase = active.phase.wrapping_add(inc as u32);
            if (active.phase & 0x8000_0000) == 0 {
                AMP
            } else {
                -AMP
            }
        };

        if active.samples_left_in_step > 0 {
            active.samples_left_in_step -= 1;
        }
        if active.samples_left_in_step == 0 {
            active.step_index += 1;
            if active.step_index >= active.script.len() {
                self.current = None;
            } else {
                active.phase = 0;
                active.samples_left_in_step = samples_for_step(active.script[active.step_index]);
            }
        }

        sample
    }

    fn start_playback(request: AudioRequest) -> ActivePlayback {
        let script = script_for(request.event);
        ActivePlayback {
            request,
            script,
            step_index: 0,
            samples_left_in_step: samples_for_step(script[0]),
            phase: 0,
        }
    }

    fn enqueue(&mut self, request: AudioRequest) -> bool {
        if self.queue_len >= QUEUE_CAPACITY {
            return false;
        }

        // Keep queue sorted by priority (high -> low) while preserving FIFO
        // order within the same priority class.
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
}

fn priority_for(event: AudioEvent) -> AudioPriority {
    match event {
        AudioEvent::Boot => AudioPriority::Boot,
        AudioEvent::TouchInteraction | AudioEvent::KeyInteraction => AudioPriority::Interaction,
        AudioEvent::ModeSwitch => AudioPriority::ModeSwitch,
        AudioEvent::Warning => AudioPriority::Warning,
        AudioEvent::Error => AudioPriority::Error,
    }
}

fn script_for(event: AudioEvent) -> &'static [ToneStep] {
    match event {
        AudioEvent::Boot => &STEPS_BOOT,
        AudioEvent::TouchInteraction => &STEPS_TOUCH,
        AudioEvent::KeyInteraction => &STEPS_KEY,
        AudioEvent::ModeSwitch => &STEPS_MODE_SWITCH,
        AudioEvent::Warning => &STEPS_WARNING,
        AudioEvent::Error => &STEPS_ERROR,
    }
}

fn samples_for_step(step: ToneStep) -> u32 {
    let mut samples = (SAMPLE_RATE_HZ as u64 * step.duration_ms as u64) / 1000;
    if samples == 0 {
        samples = 1;
    }
    samples as u32
}
