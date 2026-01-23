use esp_hal::dma::DmaError;
use esp_hal::i2s::master::{Channels, Config, DataFormat, I2s};
use esp_hal::time::Rate;

#[derive(defmt::Format, Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioDemoError {
    I2sConfig,
    I2s,
    Dma { op: DmaOp, err: DmaError },
    WavInvalid,
    WavUnsupported,
}

#[derive(defmt::Format, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmaOp {
    Available,
    PushWith,
    Stop,
}

struct WavView<'a> {
    sample_rate_hz: u32,
    data: &'a [u8],
}

pub fn play_demo_playlist(
    i2s0: esp_hal::peripherals::I2S0,
    dma_channel: esp_hal::peripherals::DMA_CH0,
    bclk: esp_hal::peripherals::GPIO4,
    ws: esp_hal::peripherals::GPIO5,
    dout: esp_hal::peripherals::GPIO6,
) -> Result<(), AudioDemoError> {
    defmt::info!("audio: demo playlist start (PCM-only)");

    // NOTE: MAX98357A is an I2S DAC/amp. We keep a simple Philips-like I2S/TDM config
    // with 16-bit samples and mono duplication (same data for L/R).
    let i2s = I2s::new(
        i2s0,
        dma_channel,
        Config::new_tdm_philips()
            .with_sample_rate(Rate::from_hz(8_000))
            .with_data_format(DataFormat::Data16Channel16)
            .with_channels(Channels::MONO),
    )
    .map_err(|_| AudioDemoError::I2sConfig)?;

    // Circular DMA TX buffer.
    let (_, _, mut tx_buffer, tx_descriptors) = esp_hal::dma_circular_buffers!(0, 16 * 4092);
    let tx_capacity = tx_buffer.len();
    // Keep logging conservative: only log when the DMA ring is almost full.
    let log_safe_free_bytes = core::cmp::min(tx_capacity, 2 * 4092);

    let mut i2s_tx = i2s
        .i2s_tx
        .with_bclk(bclk)
        .with_ws(ws)
        .with_dout(dout)
        .build(tx_descriptors);

    let mut transfer = i2s_tx
        .write_dma_circular(&mut tx_buffer)
        .map_err(|_| AudioDemoError::I2s)?;
    let zeros = [0u8; 512];

    // Playlist is defined by the Plan #0004 contract.
    let playlist: [(&'static str, &'static [u8]); 6] = [
        (
            "01_sweep_pcm.wav",
            include_bytes!("../assets/audio/demo-playlist/01_sweep_pcm.wav"),
        ),
        (
            "02_melody_pcm2.wav",
            include_bytes!("../assets/audio/demo-playlist/02_melody_pcm2.wav"),
        ),
        (
            "03_sweep_pcm3.wav",
            include_bytes!("../assets/audio/demo-playlist/03_sweep_pcm3.wav"),
        ),
        (
            "04_melody_pcm.wav",
            include_bytes!("../assets/audio/demo-playlist/04_melody_pcm.wav"),
        ),
        (
            "05_sweep_pcm2.wav",
            include_bytes!("../assets/audio/demo-playlist/05_sweep_pcm2.wav"),
        ),
        (
            "06_melody_pcm3.wav",
            include_bytes!("../assets/audio/demo-playlist/06_melody_pcm3.wav"),
        ),
    ];

    // Parse upfront (avoid work at segment boundaries).
    let wavs: [WavView<'_>; 6] = [
        parse_wav_pcm16le_mono_8khz(playlist[0].1)?,
        parse_wav_pcm16le_mono_8khz(playlist[1].1)?,
        parse_wav_pcm16le_mono_8khz(playlist[2].1)?,
        parse_wav_pcm16le_mono_8khz(playlist[3].1)?,
        parse_wav_pcm16le_mono_8khz(playlist[4].1)?,
        parse_wav_pcm16le_mono_8khz(playlist[5].1)?,
    ];

    let mut seg_idx = 0usize;
    let mut audio_remaining: &[u8] = wavs[0].data;
    let mut silence_remaining_bytes: usize = 0;
    let mut pending_log_seg: Option<usize> = Some(0);

    // Tail handling for 32-bit alignment safety (should be unused for current assets but kept robust).
    let mut tail = [0u8; 4];
    let mut tail_len = 0usize;

    while seg_idx < playlist.len() {
        // If we've queued the final segment completely, we can stop producing and just drain.
        if seg_idx + 1 == playlist.len() && audio_remaining.is_empty() && tail_len == 0 {
            break;
        }

        if let Some(i) = pending_log_seg {
            let avail = transfer.available().map_err(|err| AudioDemoError::Dma {
                op: DmaOp::Available,
                err,
            })?;
            if avail <= log_safe_free_bytes {
                defmt::info!(
                    "audio: segment {}/{} start: {}",
                    i + 1,
                    playlist.len(),
                    playlist[i].0
                );
                pending_log_seg = None;
            }
        }

        let mut advanced_to: Option<usize> = None;
        let wrote = transfer
            .push_with(|buf| {
                let want = buf.len() & !0x3;
                if want == 0 {
                    return 0;
                }

                let mut out = 0usize;

                // Flush any pending tail first.
                if tail_len != 0 && out + 4 <= want {
                    buf[out..out + 4].copy_from_slice(&tail);
                    out += 4;
                    tail_len = 0;
                }

                while out < want {
                    if silence_remaining_bytes > 0 {
                        let mut take = core::cmp::min(want - out, silence_remaining_bytes);
                        take &= !0x3;
                        if take == 0 {
                            break;
                        }
                        let mut filled = 0usize;
                        while filled < take {
                            let chunk = core::cmp::min(zeros.len(), take - filled);
                            buf[out + filled..out + filled + chunk]
                                .copy_from_slice(&zeros[..chunk]);
                            filled += chunk;
                        }
                        out += take;
                        silence_remaining_bytes -= take;
                        continue;
                    }

                    if !audio_remaining.is_empty() {
                        let mut take = core::cmp::min(want - out, audio_remaining.len());
                        take &= !0x3;
                        if take == 0 {
                            // Buffer remainder into tail.
                            let take2 = core::cmp::min(4 - tail_len, audio_remaining.len());
                            tail[tail_len..tail_len + take2]
                                .copy_from_slice(&audio_remaining[..take2]);
                            tail_len += take2;
                            audio_remaining = &audio_remaining[take2..];
                            break;
                        }
                        buf[out..out + take].copy_from_slice(&audio_remaining[..take]);
                        out += take;
                        audio_remaining = &audio_remaining[take..];
                        continue;
                    }

                    // Current segment done: insert 1s silence between segments, then advance.
                    if seg_idx + 1 >= playlist.len() {
                        break;
                    }
                    silence_remaining_bytes = (wavs[seg_idx].sample_rate_hz as usize) * 2;
                    seg_idx += 1;
                    audio_remaining = wavs[seg_idx].data;
                    advanced_to = Some(seg_idx);
                }

                out
            })
            .map_err(|err| AudioDemoError::Dma {
                op: DmaOp::PushWith,
                err,
            })?;

        if let Some(i) = advanced_to {
            pending_log_seg = Some(i);
        }

        if wrote == 0 {
            continue;
        }
    }

    // Best-effort drain: wait until the DMA circular buffer is fully available, then stop.
    for _ in 0..2_000_000 {
        if transfer.available().map_err(|err| AudioDemoError::Dma {
            op: DmaOp::Available,
            err,
        })? >= tx_capacity
        {
            break;
        }
    }

    transfer.stop().map_err(|err| AudioDemoError::Dma {
        op: DmaOp::Stop,
        err,
    })?;

    defmt::info!("audio: demo playlist done (PCM-only)");
    Ok(())
}

fn parse_wav_pcm16le_mono_8khz(bytes: &[u8]) -> Result<WavView<'_>, AudioDemoError> {
    if bytes.len() < 44 {
        return Err(AudioDemoError::WavInvalid);
    }
    if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(AudioDemoError::WavInvalid);
    }

    let mut fmt: Option<(u16, u16, u32, u16)> = None; // (audio_format, channels, sample_rate, bits_per_sample)
    let mut data: Option<&[u8]> = None;

    let mut offset = 12;
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
            return Err(AudioDemoError::WavInvalid);
        }

        match id {
            b"fmt " => {
                if size < 16 {
                    return Err(AudioDemoError::WavInvalid);
                }
                let audio_format = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
                let channels = u16::from_le_bytes([bytes[offset + 2], bytes[offset + 3]]);
                let sample_rate = u32::from_le_bytes([
                    bytes[offset + 4],
                    bytes[offset + 5],
                    bytes[offset + 6],
                    bytes[offset + 7],
                ]);
                let bits_per_sample = u16::from_le_bytes([bytes[offset + 14], bytes[offset + 15]]);
                fmt = Some((audio_format, channels, sample_rate, bits_per_sample));
            }
            b"data" => {
                data = Some(&bytes[offset..offset + size]);
            }
            _ => {}
        }

        // Chunks are padded to 16-bit alignment.
        offset += size + (size % 2);
        if fmt.is_some() && data.is_some() {
            break;
        }
    }

    let (audio_format, channels, sample_rate, bits_per_sample) =
        fmt.ok_or(AudioDemoError::WavInvalid)?;
    let data = data.ok_or(AudioDemoError::WavInvalid)?;

    // PCM = 1.
    if audio_format != 1 {
        return Err(AudioDemoError::WavUnsupported);
    }
    if channels != 1 {
        return Err(AudioDemoError::WavUnsupported);
    }
    if sample_rate != 8_000 {
        return Err(AudioDemoError::WavUnsupported);
    }
    if bits_per_sample != 16 {
        return Err(AudioDemoError::WavUnsupported);
    }
    if (data.len() % 2) != 0 {
        return Err(AudioDemoError::WavInvalid);
    }

    Ok(WavView {
        sample_rate_hz: sample_rate,
        data,
    })
}
