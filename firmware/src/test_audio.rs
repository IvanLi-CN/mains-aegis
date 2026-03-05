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
struct ActivePlayback {
    request: AudioRequest,
    pcm: &'static [u8],
    cursor: usize,
}

pub const PLAYBACK_SAMPLE_RATE_HZ: u32 = 44_100;
const QUEUE_CAPACITY: usize = 8;

const WAV_BOOT: &[u8] = include_bytes!("../assets/audio/test-fw-cues/boot_startup.wav");
const WAV_TOUCH_INTERACTION: &[u8] =
    include_bytes!("../assets/audio/test-fw-cues/mains_present_dc.wav");
const WAV_KEY_INTERACTION: &[u8] =
    include_bytes!("../assets/audio/test-fw-cues/charge_completed.wav");
const WAV_MODE_SWITCH: &[u8] =
    include_bytes!("../assets/audio/test-fw-cues/shutdown_mode_entered.wav");
const WAV_WARNING: &[u8] = include_bytes!("../assets/audio/test-fw-cues/battery_low_no_mains.wav");
const WAV_ERROR: &[u8] = include_bytes!("../assets/audio/test-fw-cues/module_fault.wav");

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

            let written = {
                let active = self
                    .current
                    .as_mut()
                    .expect("audio playback must exist after promote");
                copy_pcm_chunk(active.pcm, &mut active.cursor, &mut buf[out..want])
            };

            if written == 0 {
                self.current = None;
                continue;
            }

            out += written;
            let finished = self
                .current
                .as_ref()
                .map(|active| active.cursor >= active.pcm.len())
                .unwrap_or(false);
            if finished {
                self.current = None;
            }
        }

        out
    }

    fn start_playback(request: AudioRequest) -> ActivePlayback {
        ActivePlayback {
            request,
            pcm: pcm_for_event(request.event),
            cursor: 0,
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

fn pcm_for_event(event: AudioEvent) -> &'static [u8] {
    // test-fw maps high-level events onto the latest approved cue bundle from
    // docs/audio-cues-preview/audio.
    let wav = match event {
        AudioEvent::Boot => WAV_BOOT,
        AudioEvent::TouchInteraction => WAV_TOUCH_INTERACTION,
        AudioEvent::KeyInteraction => WAV_KEY_INTERACTION,
        AudioEvent::Warning => WAV_WARNING,
        AudioEvent::Error => WAV_ERROR,
        AudioEvent::ModeSwitch => WAV_MODE_SWITCH,
    };
    parse_wav_pcm16le_mono(wav)
}

fn copy_pcm_chunk(src: &[u8], cursor: &mut usize, dst: &mut [u8]) -> usize {
    let dst_len = dst.len() & !0x3;
    if dst_len == 0 || *cursor >= src.len() {
        return 0;
    }

    let remaining = src.len() - *cursor;
    let aligned_src = remaining & !0x3;
    if aligned_src > 0 {
        let take = core::cmp::min(dst_len, aligned_src);
        dst[..take].copy_from_slice(&src[*cursor..*cursor + take]);
        *cursor += take;
        return take;
    }

    let take = core::cmp::min(remaining, 4);
    dst[..take].copy_from_slice(&src[*cursor..*cursor + take]);
    for b in &mut dst[take..4] {
        *b = 0;
    }
    *cursor += take;
    4
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
    if view.sample_rate_hz != PLAYBACK_SAMPLE_RATE_HZ {
        return &[];
    }
    view.data
}

#[derive(Clone, Copy)]
struct WavView {
    audio_format: u16,
    channels: u16,
    sample_rate_hz: u32,
    bits_per_sample: u16,
    data: &'static [u8],
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
