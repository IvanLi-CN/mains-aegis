# 固件音频播放 + Demo 素材（#0004）

## 状态

- Status: 已完成
- Created: 2026-01-22
- Last: 2026-01-23

## 背景 / 问题陈述

- 本项目已选 `ESP32-S3-FH4R2`，并在 `docs/audio-design.md` 中推荐采用 `TDM(I2S) -> MAX98357A -> 8Ω/1W` 的音频输出链路。
- 网表（`docs/pcbs/mainboard/netlist.enet`）显示主板已包含 `MAX98357AETE+T`（Designator：`U6`），其供电接 `+5V`，并连接 `AUDIO_I2S_BCLK/LRCLK/DOUT`，可用于固件侧音频播放闭环验证。
- 目前固件基线已具备最小 bring-up（见 `firmware/`），但尚无可复用的“音频播放”能力与可回归验证的音频素材。
- 需要一组可回归的 Demo 音频（多段，按顺序播放），用于尽早闭环：素材 →（可选转码）→ 固件嵌入/读取 → I2S/TDM DMA 播放 → 喇叭可听验证。

## 目标 / 非目标

### Goals

- 在固件侧提供“可触发、可重复”的音频播放能力（单声道），能按顺序播放一组 Demo 音频并在段间插入 1s 静音。
- 提供一份可提交到仓库的 Demo playlist 音频素材（mono），并定义其格式/落盘位置/（如需）转码与嵌入流程。
- 明确并记录：采样率/格式选择、响度/限幅策略，以及与 `8Ω / 1W` 喇叭匹配的保护约束。

### Non-goals

- 不做通用音频框架（多音轨混音、流式播放、网络音频、录音/回声消除等）。
- 不在本计划内实现“10 种以上提示音库”与完整音效管理策略（如需另开计划或在本计划范围冻结后再扩展）。
- 不在本计划内做 EMC/EMI 的系统级验证与整改闭环（仅记录硬件注意事项与风险）。

## 范围（Scope）

### In scope

- 固件：初始化 I2S/TDM TX + DMA，播放单声道音频（Demo 素材）。
- 素材：提供 Demo playlist（6 段；约 10s/段；mono；段间 1s 静音）并落库；如选择压缩格式，提供确定的转码输入/输出形状与可回归的生成方式。
- 触发方式：定义并实现一个可用于验证的播放触发（例如上电自动播放、串口命令、或绑定到某个输入事件；以契约为准）。
- 观测性：至少能在日志中观察到播放开始/结束、以及 buffer underrun（如发生）的计数或显式报错。

### Out of scope

- 复杂的 UI/交互（屏幕菜单、配置持久化、音量曲线/动态压缩等）。
- 多路音效并发与优先级仲裁（后续按需求再立项）。

## 需求（Requirements）

### MUST

- Demo playlist 为 **单声道（mono）**，由 6 段音频按顺序播放组成，每段 **约 10 秒**（不强制整秒），段与段之间插入 **1 秒静音**。
- Demo playlist 必须覆盖以下要素（至少各 1 次）：`WAV(PCM16LE)`、`旋律`、`扫频`。
- Demo 音频素材必须可在仓库内离线获取（不依赖在线 URL）；其版权/授权必须可用于仓库分发（默认采用“纯合成音/合成波形”避免版权风险）。
- 固件播放链路与硬件设计文档保持一致：以 `ESP32-S3` 的 I2S/TDM TX 驱动 `MAX98357A`（或等价数字功放）输出到 `8Ω / 1W` 喇叭。
- 音频播放不应导致固件 panic；播放过程中不得出现持续性的 DMA underrun（若出现，必须在日志中可定位）。
- 必须有“不过载”的保护口径：通过素材电平（留 headroom）与/或功放增益档位与/或数字限幅，避免长期过驱 `8Ω / 1W` 喇叭。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes） |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Demo 音频素材文件与产物布局 | File format | internal | New | ./contracts/file-formats.md | firmware | firmware | 定义“源素材/（可选）转码产物/固件侧引用”的形状 |

### 契约文档（按 Kind 拆分）

- [contracts/README.md](./contracts/README.md)
- [contracts/file-formats.md](./contracts/file-formats.md)

## 验收标准（Acceptance Criteria）

- Given 目标硬件链路已连接（`ESP32-S3` I2S/TDM → `MAX98357A` → `8Ω/1W` speaker），且固件已按仓库既有流程烧录并运行，
  When 固件上电运行，
  Then 将自动开始播放 Demo playlist：共 6 段，段间有 **1 秒静音**，并在日志中看到“开始/结束”标记且无 panic。

- Given Demo 素材已按契约落库并被固件引用，
  When 构建固件产物（Debug/Release 至少各一次），
  Then 构建成功，且仓库内不存在需要在线下载的音频依赖。

- Given 播放过程中发生 DMA underrun（若实现中提供该观测），
  When underrun 发生，
  Then 日志中能明确记录 underrun（计数或错误），便于定位性能/缓冲问题。

## 实现前置条件（Definition of Ready / Preconditions）

- 已确认本计划的硬件链路与供电条件（网表已确认：主板 `MAX98357AETE+T(U6)` 供电为 `+5V`）。
- 已冻结 Demo playlist 的“素材形态”（采样率/编码/段数/每段时长/段间静音/电平 headroom）与仓库存放位置（见 `./contracts/file-formats.md`）。
- 已确认“播放触发方式”的口径（固定为：上电自动播放）。
- Demo playlist 素材已落地在计划目录内（见 `assets/demo-playlist/` 与 `assets/README.md`）。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: 本计划 PCM-only 不引入解码器；若未来另起计划引入解码器（如压缩音频），需提供可在 host 上运行的最小测试覆盖（至少：解码正确性 + 边界输入）。
- Integration tests: 至少一次“可烧录 + 可播放 + 可观测日志”的手工验证步骤，并固化到文档（例如 `firmware/README.md` 的音频验证章节）。

### Quality checks

- 仅使用仓库既有工具链进行质量检查（例如 `cargo fmt` / `cargo clippy` / `cargo build`），不在本计划内引入新的质量工具。

## 文档更新（Docs to Update）

- `docs/audio-design.md`: 增加指向“固件侧音频播放实现入口/测试方法”的链接（实现阶段更新）。
- `firmware/README.md`: 增加“音频播放 Demo 验证”的步骤与预期日志口径（实现阶段更新）。
- `docs/hardware-selection/esp32-s3-fh4r2-gpio.md`: 如最终冻结了 I2S/TDM 引脚分配，在实现阶段同步更新。

## 实现里程碑（Milestones）

- [x] M1: 落地 Demo playlist 音频文件（6 段；每段约 10s；段间 1s 静音；WAV(PCM16LE)；含 旋律 + 扫频）
- [x] M2: 固件侧 I2S/TDM TX + DMA 播放链路跑通（可播放 Demo）
- [x] M3: 落地可用于验证的触发方式 + 播放日志（start/stop + underrun 可观测）
- [x] M4: 完成一次端到端手工验证记录并同步相关文档入口

## 方案概述（Approach, high-level）

- 优先走 `docs/audio-design.md` 推荐的链路：`ESP32-S3` I2S/TDM TX → `MAX98357A`。
- 素材默认采用“纯合成音”生成（避免版权问题）；在格式选择上，优先满足“简单可用 + 可回归”，再考虑 flash 体积与解码开销。
- 由于 ADPCM 在目标链路上底噪难以满足听感要求，本计划最终**仅接受 PCM（WAV PCM16LE）**；如未来需压缩，应另起新计划评估硬件/电源/放大器噪声底与编码方案。
- 通过“素材电平约束（headroom）+ 增益档位/（可选）数字限幅”的组合来避免过驱 `8Ω/1W` 喇叭。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：
  - Demo 时长上限与采样率/编码选择将显著影响固件镜像体积与 RAM buffer 规模。
  - Class-D 输出的底噪/爆音与 EMI 可能需要硬件/布局配合；固件侧只能做有限缓解（软启动/静音策略等）。
- 开放问题：
  - None
- 假设（需主人确认）：
  - 固件仍以 `esp-hal`（`no_std`）路线为主，且 `firmware/` 是后续实现入口目录。

## 变更记录（Change log）

- 2026-01-22: 初始化计划与契约骨架
- 2026-01-23: 收敛为 PCM-only（WAV PCM16LE mono 8kHz），并落地固件侧 I2S/TDM 播放 Demo、固件侧素材落盘与验证文档入口；播放链路以连续流式 `push_with` 驱动 DMA ring，完成端到端烧录验证。
- 2026-01-23: 决策收敛：只接受 PCM（WAV PCM16LE）；更新契约与素材/固件为 PCM-only
- 2026-01-23: 修复 `DmaError::Late`（环形 DMA 喂数过晚）：改为 `push_with` 单循环流式生成（音频/段间静音）并将日志延后到 ring buffer 高水位；复核端到端 6 段均播放完成

## 参考（References）

- `docs/audio-design.md`
