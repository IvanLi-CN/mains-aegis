# 音频/提示音设计（从无源蜂鸣器升级）

目标：在“音质要求不高、节约资源”为前提下，把提示音从无源蜂鸣器升级为**更丰富**的告警/提示音效（约 10 种、每种 3–5 秒）。

结论（推荐默认方案）：

- **MCU**：`ESP32-S3-FH4R2`（本项目已选）
- **输出链路**：`ESP32-S3 TDM (TX) -> MAX98357A -> Speaker`（固件采用 `esp-rs`/`esp-hal`（no_std）时按 TDM 落地）
- **素材格式**：仅接受 `PCM`（`WAV(PCM16LE)`；mono；`8kHz`）
- **发声器件（当前选型）**：`8Ω / 1W`（`15×11×3mm` 级别）

> 说明：我们在实际硬件上尝试过 `IMA-ADPCM`（含多种“降噪/去噪”手段），仍存在明显沙沙底噪且副作用较大（变暗/变小/泵动）。因此当前设计决策收敛为 PCM-only；若未来需要压缩，请另起计划评估硬件噪声底与编码方案。

---

## 1. 为什么蜂鸣器“不够丰富”

无源蜂鸣器通常是强共振、带宽窄的发声器件：即使 MCU 输出更复杂的 PWM 波形，最终也会被其机械/电声特性“滤”成较单一的音色。

想要明显变“丰富”，最有效的改变是：**换成喇叭/小扬声器**，并用功放驱动（数字功放或 DAC+模拟功放）。

---

## 2. 器件选择：MAX98357A（PCM/TDM 数字功放）

`MAX98357A` 是数字 PCM 输入的 Class‑D 功放，适合“提示音/告警音效”这种低码率音频输出：

- **供电范围**：`2.5–5.5V`
- **无需 MCLK**（只需要 `BCLK/LRCLK/DIN`；在 TDM 中，`LRCLK` 实际作为帧同步 `WS` 使用）
- **采样率**：`8kHz–96kHz`
- **输出能力**：典型宣传指标为 `3.2W into 4Ω @ 5V`（具体受供电、负载、热设计限制）
- **Filterless Class‑D 输出**（更少外围器件，但 PCB 走线/EMI 需要专项注意）

参考资料（器件数据手册）：`https://www.analog.com/media/en/technical-documentation/data-sheets/max98357a-max98357b.pdf`

---

## 3. 硬件实现要点（不猜测，按原则写）

### 3.1 最小硬件增量（常见做法）

- `MAX98357A`（封装按可采购/可贴装性选）
- 电源去耦（贴近芯片的旁路电容；按 datasheet 推荐值）
- 喇叭（本项目：`8Ω / 1W`）
- （可选）功放使能/静音脚（`AMP_EN`）：本项目为节省 GPIO，默认不引入；如后续发现上电瞬态/待机漏电问题，再加回

### 3.2 关键注意事项

- `OUTP/OUTN` 为 **BTL/桥接输出**：喇叭两端分别接 `OUTP` 与 `OUTN`，**不要**把喇叭负端接地。
- 供电建议：为了在 `8Ω / 1W` 小喇叭上获得更大的响度余量，优先使用稳定 `5V` 供电；`3.3V` 也可工作但最大响度会更低。
- 供电电流预算：按 `1W` 输出、效率与瞬态余量考虑，建议为功放预留 `≥0.5A` 峰值供电能力（并把去耦贴近芯片放置）。
- 额定功率保护：`8Ω / 1W` 喇叭在 `5V` 供电下仍可能被过驱（取决于增益/素材电平/限幅策略）；建议通过增益档位与素材电平（留 headroom）控制最大输出。
- Class‑D 输出走线/EMI：喇叭线与回流路径、地分割、开关节点环路面积都要控制；必要时评估 EMI 滤波（按 datasheet/参考设计）。

### 3.3 `SD_MODE` / `GAIN_SLOT`（本项目：TDM + 单声道）

参考：

- datasheet：`https://www.analog.com/media/en/technical-documentation/data-sheets/max98357a-max98357b.pdf`
- 应用笔记（MAX98357 WLP）：`https://www.analog.com/en/resources/design-notes/optimize-cost-size-and-performance-with-max98357-wlp.html`

#### 3.3.1 设计结论（只讲连接）

- `SD_MODE`：**固定上拉到 `VDDIO`（高电平）**
- `GAIN_SLOT`：**固定下拉到 `GND`**

该组合在 TDM 模式下选择 **channel 0**（应用笔记 Table 2）。本项目只使用单声道：固件把音频样本写入该 slot，其余 slot 置 0。

> 注：保持网名不变：`AUDIO_I2S_LRCLK` 在 TDM 下作为 `WS` 使用，但网名不改。

---

## 4. 固件资源评估（提示音场景）

提示音播放使用 `ESP32-S3` 的 I2S 外设（TDM 模式）TX + DMA：CPU 主要做“解码/搬运数据”，资源压力很小。

### 4.1 占用的 MCU 资源类型

- **外设**：占用 1 路 I2S（TX，TDM 模式）
- **GPIO**：最少 3 根（`BCLK/LRCLK/DOUT`），可选再加 1 根（`AMP_EN`）
- **RAM**：I2S DMA buffer + 应用层 ring buffer（通常数 KB～数十 KB）
- **CPU**：
  - `PCM`：几乎只有搬运（很轻）
  - `IMA-ADPCM`：当前不支持（仅历史评估；不要作为现行方案）
  - `MP3/AAC`：不推荐用于本项目提示音（复杂度与 CPU/ROM/RAM 成本更高）

### 4.2 音频素材占用（最关键的“资源”）

以“10 种提示音、每种 3–5 秒”为例：

- **8kHz / Mono / PCM(16‑bit)**：约 `16KB/s`
  - 单个音效：`48–80KB`
  - 10 个合计：约 `0.48–0.8MB`

> 若后续需要降低素材体积，优先从“提示音时长/采样率/内容设计”优化；编码压缩（如 ADPCM/Opus 等）仅作为未来评估方向，需结合目标板噪声底与听感另行评估（当前 PCM-only）。

### 4.3 固件实现入口（运行时 cue 服务）

本仓库已将主固件切换为“主循环常驻音效服务”，用于把 I2S/TDM→MAX98357A 音频链路与实际运行时提示音语义接到一起：

- 主固件入口：`../firmware/src/main.rs`（启动后只请求一次 `boot_startup`，随后在主循环内持续调度）
- 共享播放核心：`../firmware/src/audio.rs`
- 运行时信号快照：`../firmware/src/output/mod.rs`
- 运行时资产（固件侧打包）：`../firmware/assets/audio/test-fw-cues/`
- 验证步骤：`../firmware/README.md`（见“运行时音效服务（Plan #h43mk）”章节）

运行时调度冻结为：

- `one_shot`：`boot_startup`、市电恢复、充电开始/完成
- `interval_loop(2000ms)`：市电丢失、高压力、低电（按市电有无拆分）
- `continuous_loop`：保护、过压/过流、模块故障、电池保护

`shutdown_mode_entered` 与 `io_over_power` 继续保留素材定义，但主固件本轮不触发，等待真实状态源后再接入。

### 4.4 状态/告警/错误提示音试听资产与固件资产关系

为了快速评审提示音语义与听感，本仓库继续保留独立的“本地试听资产”目录；它是试听/定义源，不由主固件直接读取：

- 试听资产入口：`./audio-cues-preview/README.md`
- 清单契约：`./audio-cues-preview/cues.manifest.json`
- 本地预览页：`./audio-cues-preview/preview.html`
- 固件打包副本：`../firmware/assets/audio/test-fw-cues/*.wav`

> 说明：`docs/audio-cues-preview/**` 用于“音效定义与试听”，`firmware/assets/audio/test-fw-cues/*.wav` 是当前主固件与 `test-fw` 共用的运行时资产副本。

---

## 5. 引脚预留建议（供原理图阶段落地）

本项目 GPIO 分配表目前仍处于“除已确认外，其余不做假设分配”的阶段；因此这里只给出**预留建议**，不把引脚状态改为“已分配”。

建议在 `docs/hardware-selection/esp32-s3-fh4r2-gpio.md` 中为以下信号预留 3–4 个 GPIO：

- `AUDIO_I2S_BCLK`
- `AUDIO_I2S_LRCLK`
- `AUDIO_I2S_DOUT`
- （可选）`AUDIO_AMP_EN`（本项目默认不引入）

### 5.1 蜂鸣器 + 数字功放（TDM）共存（3 引脚，1 根复用）

为兼容“保留蜂鸣器”与“数字功放（TDM）”，并将 GPIO 成本控制为 **3 根**，建议：

- `AUDIO_I2S_BCLK`：专用
- `AUDIO_I2S_LRCLK`：专用
- 第 3 根 GPIO **二选一复用**：
  - TDM 模式：`AUDIO_I2S_DOUT`
  - 蜂鸣器模式：`BUZZ_PWM`

实现原则：

- 硬件上用“二选一装配 / 二选一跳线 / 模拟开关”等方式，保证同一根 GPIO 不会同时接到两路负载。
- 固件上在不同模式下切换外设功能（TDM TX vs LEDC/PWM）。

---

## 6. 待确认输入（落到 BOM/原理图前必须定）

1. 发声器件：本项目已定 `8Ω / 1W`；若后续改为更大功率/更大体积，需要重新评估供电与响度余量。
2. 供电：是否有可用的 `5V` 轨给功放？还是仅 `3.3V`？
3. EMI/结构：喇叭线长度、是否需要连接器、是否有 EMI 约束（认证/近场敏感器件等）？

---

## 附：PWM 能不能“直接驱动喇叭发声”

可以“用 PWM 做音频”，但通常**不能**用 MCU 的 GPIO 直接去推 `8Ω` 喇叭：

- 从电流上看，`8Ω` 喇叭在“听起来明显”的响度下往往需要数十到数百 mA 的交流电流，GPIO 通常达不到。
- 可行的做法是：PWM 作为音频调制信号，外接**功率驱动**（半桥/全桥/H 桥/专用功放），必要时再做滤波/EMI 处理。

因此本项目如果要“更丰富的提示音”，仍建议走 `TDM + MAX98357A + Speaker`；蜂鸣器则用 `LEDC/PWM` 单独驱动更合适。
