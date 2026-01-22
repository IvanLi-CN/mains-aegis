# 文件格式（File formats）

将文件/目录格式视为一种接口契约来描述。

## Demo 音频素材与产物布局（`docs/plan/**` + `firmware/**`）

- 范围（Scope）: internal
- 变更（Change）: New
- 编码（Encoding）: binary（音频文件）

### Schema（结构）

本文件为本计划的**冻结契约**：后续实现以这里为准。

源素材（计划冻结闸门要求）：必须落在本计划目录下，作为“可回归”的事实依据。

```text
docs/plan/0004:firmware-audio-playback-demo/
  assets/
    demo-playlist/
      01_*.wav
      02_*.wav
      03_*.wav
      04_*.wav
      05_*.wav
      06_*.wav
```

实现阶段的固件侧引用（建议形状）：固件按文件名前缀顺序播放，并在段间插入 1s 静音。

```text
firmware/
  assets/
    audio/
      demo-playlist/
        01_*.wav         # mono；WAV(PCM) 或 WAV(IMA-ADPCM)
        02_*.wav
        03_*.wav
        04_*.wav
        05_*.wav
        06_*.wav
```

字段约束（已冻结）：

- Channel: mono（固定）
- Inter-segment silence: 1s（固定）
- Sample rate: `8000 Hz`（固定）
- Encoding/container:
  - PCM: `WAV(PCM16LE)`（默认）
  - ADPCM: `WAV(IMA-ADPCM, 4-bit)`（默认）
- Level/headroom: 峰值 `<= -6 dBFS`（固定；不得削波）

内容覆盖（固定要求）：

- 至少 1 段为 `扫频`
- 至少 1 段为 `旋律`
- 至少 1 段为 `WAV(PCM)`
- 至少 1 段为 `WAV(IMA-ADPCM)`

段数与每段时长（已冻结）：

- Segment count: `6`（固定）
- Segment duration: 约 `10s`（每段的**实际长度**以“文件清单”中的 `samples@8000Hz` 为准；不强行卡整秒）
- Total duration: `sum(segments) + 5*1s`（其中 `segments` 取决于各文件样本数）

文件清单（固定；按前缀顺序播放；段间由固件插入 1s 静音）：

1. `01_sweep_pcm.wav`：扫频；`WAV(PCM16LE)`；`8000 Hz`；`78400 samples`（`9.8s`）
2. `02_melody_adpcm.wav`：旋律；`WAV(IMA-ADPCM)`；`8000 Hz`；`84800 samples`（`10.6s`）
3. `03_sweep_adpcm.wav`：扫频；`WAV(IMA-ADPCM)`；`8000 Hz`；`74396 samples`（`9.2995s`）
4. `04_melody_pcm.wav`：旋律；`WAV(PCM16LE)`；`8000 Hz`；`88800 samples`（`11.1s`）
5. `05_sweep_pcm2.wav`：扫频；`WAV(PCM16LE)`；`8000 Hz`；`77600 samples`（`9.7s`）
6. `06_melody_adpcm2.wav`：旋律；`WAV(IMA-ADPCM)`；`8000 Hz`；`81600 samples`（`10.2s`）

### Examples（示例）

- 计划侧源素材：
  - `docs/plan/0004:firmware-audio-playback-demo/assets/demo-playlist/01_sweep_pcm.wav`
  - `docs/plan/0004:firmware-audio-playback-demo/assets/demo-playlist/02_melody_adpcm.wav`
  - `docs/plan/0004:firmware-audio-playback-demo/assets/demo-playlist/03_sweep_adpcm.wav`
  - `docs/plan/0004:firmware-audio-playback-demo/assets/demo-playlist/04_melody_pcm.wav`
  - `docs/plan/0004:firmware-audio-playback-demo/assets/demo-playlist/05_sweep_pcm2.wav`
  - `docs/plan/0004:firmware-audio-playback-demo/assets/demo-playlist/06_melody_adpcm2.wav`
- 固件侧建议引用形状：
  - `firmware/assets/audio/demo-playlist/01_sweep_pcm.wav`
  - `firmware/assets/audio/demo-playlist/02_melody_adpcm.wav`
  - `firmware/assets/audio/demo-playlist/03_sweep_adpcm.wav`
  - `firmware/assets/audio/demo-playlist/04_melody_pcm.wav`
  - `firmware/assets/audio/demo-playlist/05_sweep_pcm2.wav`
  - `firmware/assets/audio/demo-playlist/06_melody_adpcm2.wav`

### 兼容性与迁移（Compatibility / migration）

- 若未来需要更换段数/顺序：以“新增文件并调整编号前缀”为主；避免复用旧文件名导致历史验证不可复现。
- 若未来需要引入非 WAV 的裸格式：应新增子目录（例如 `demo-playlist-raw/`）并在计划中冻结新的读取契约，避免对现有 WAV 工作流造成破坏。
