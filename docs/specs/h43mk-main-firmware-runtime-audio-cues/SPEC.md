# 主固件运行时音效接入，替代开机 Demo 播放链路（#h43mk）

## 状态

- Status: 已完成
- Created: 2026-03-12
- Last: 2026-03-12

## 背景 / 问题陈述

- 当前主固件会在启动阶段阻塞播放 `firmware/assets/audio/demo-playlist/*.wav`，导致音频链路验证与实际运行时提示音语义混在一起。
- 已定义的 15 组状态/告警/错误提示音目前只在 `test-fw` 手动测试路径中可用，主固件未复用这些 cue 语义与资产。
- 音频播放核心、提示音资产映射、优先级队列当前只存在于 `firmware/src/test_audio.rs`，主固件与测试固件之间存在重复实现和错误接入风险。

## 目标 / 非目标

### Goals

- 将主固件切换为常驻运行时音效服务，不再在启动阶段播放 demo playlist。
- 抽出共享音频核心，让主固件与 `test-fw` 复用同一套 cue、优先级、WAV 解析/重采样与 DMA 填充逻辑。
- 按当前可可靠判定的运行时状态接入 cue：开机、市电、充电、电池低电、高压力、保护、过压/过流、模块故障、电池保护。
- 保留 `test-fw` 作为音频回归入口，继续支持人工点播与优先级/FIFO 验证。

### Non-goals

- 不修改 GPIO 分配与板级音频链路。
- 不重做音频素材，也不把 `firmware/assets/audio/demo-playlist/` 继续作为运行时资产维护。
- 不新增真实 shutdown flow，也不伪造 `shutdown_mode_entered` 与 `io_over_power` 的运行时触发条件。

## 范围（Scope）

### In scope

- `firmware/src/main.rs`：主循环并入常驻音效服务，移除 demo playlist 调用。
- 共享音频模块：统一 cue 枚举、优先级、调度语义（`one_shot` / `interval_loop` / `continuous_loop`）、WAV 解析/重采样、DMA `fill()`、状态接口。
- `firmware/src/bin/test-fw.rs`：改为使用共享音频模块。
- `firmware/src/output/mod.rs`：暴露供主固件音效策略消费的紧凑运行时信号/边沿接口。
- 文档：`firmware/README.md`、`docs/audio-design.md`、`docs/specs/README.md`。

### Out of scope

- 新增音效素材、混音、持久化配置、在线音源管理。
- 新增第三种以上播放入口或改变 `test-fw` 的 UI 结构。
- 为当前不存在真实状态源的 cue 扩展新的电源/关机业务逻辑。

## 接口变更（Interfaces）

- 新增共享运行时音频模块（供主固件与 `test-fw` 共同使用）。
- 新增运行时音效调度接口，至少覆盖：
  - `request_cue(...)`
  - `tick(now)`
  - `fill(buf)`
  - `status()`
  - 面向主固件的循环/抢占策略入口。
- `PowerManager` 新增供主循环消费的音效信号访问器，输出：
  - mains presence 边沿
  - charge phase 边沿
  - thermal stress 状态
  - battery low / battery protection 状态
  - module fault 状态
  - decoded over-voltage / over-current 状态
- 删除主固件对 `audio_demo::play_demo_playlist(...)` 的运行时依赖。

## 运行时 cue 映射冻结

- `boot_startup`：主循环启动后触发一次。
- `mains_present_dc` / `mains_absent_dc`：输入存在位变化时触发。
- `charge_started` / `charge_completed`：charger 状态进入“充电中 / 完成”时触发。
- `battery_low_no_mains` / `battery_low_with_mains`：BMS `RCA` 低电告警按市电有无拆分。
- `high_stress`：`TS_COOL` / `TS_WARM` / `TREG` 或 TMP112 到达 `TLOW` 但尚未触发停机时触发。
- `shutdown_protection`：`THERM_KILL_N` 断言或保护导致输出被关时触发。
- `io_over_voltage` / `io_over_current`：charger/TPS 解码后的保护位触发。
- `module_fault`：运行期关键模块通信错误期间触发。
- `battery_protection`：BMS `PF`/保护位触发。
- Dormant cue：
  - `shutdown_mode_entered`：本轮不接入，等待真实 shutdown flow。
  - `io_over_power`：本轮不接入，等待独立 over-power 状态源或阈值策略。

## 验收标准（Acceptance Criteria）

- 构建通过：
  - `cargo build --release --bin esp-firmware`
  - `cargo build --release --bin test-fw --features test-fw-audio-playback`
- 主固件上电后只播放一次 `boot_startup`，不再出现 6 段 demo playlist 的阻塞播放与对应日志序列。
- 主循环期间 power/front-panel tick 节奏保持可用，音频服务每轮并入调度而不独占流程。
- 调度语义固定为：
  - `status` -> `one_shot`
  - `warning` -> `interval_loop(2000ms)`
  - `error` -> `continuous_loop`
  - 优先级：`Error > Warning > Status > Boot`
  - 同优先级 `one_shot` 保持 FIFO。
- 运行时场景正确触发/停播：市电恢复/丢失、充电开始/完成、电池低电（区分有无市电）、高压力进入/退出、模块通信故障进入/恢复、保护/过压/过流进入/清除。
- `shutdown_mode_entered` 与 `io_over_power` 在主固件本轮保持静默，且文档明确注明等待真实状态源后再接入。

## 里程碑（Milestones）

- [x] M1: 共享音频核心抽出并被 `test-fw` 复用。
- [x] M2: `PowerManager` 输出运行时音效信号/边沿接口。
- [x] M3: 主固件主循环接入常驻音效服务并删除 demo playlist 路径。
- [x] M4: 文档与规格同步更新。
- [x] M5: 主固件与 `test-fw` 构建验证完成。

## 实现结果

- 主固件已移除阻塞式 demo playlist，改为在主循环内常驻调度共享 `AudioManager`。
- 共享播放核心已落到 `firmware/src/audio.rs`，统一 15 组 cue、优先级、WAV 解析/重采样、DMA `fill()` 与播放状态接口。
- `test-fw` 已改为复用共享音频模块，保留人工点播、抢占和同级 FIFO 验证能力。
- `PowerManager` 已输出运行时音效快照与边沿接口，主固件不再依赖 UI snapshot 差分来判定业务音效。
- `shutdown_mode_entered` 与 `io_over_power` 继续保持 dormant，并在主固件中明确不触发。

## 验证记录

- 构建通过：
  - `cargo build --release --bin test-fw --features test-fw-audio-playback`
  - `cargo build --release --bin esp-firmware`

## 风险 / 假设

- 当前 worktree 初始化前 `ina3221-async` 与 `tps55288` submodule 为空目录；本轮实现前需要补齐子模块内容后再执行构建验证。
- 运行时资产继续复用 `firmware/assets/audio/test-fw-cues/*.wav`，不直接从 `docs/audio-cues-preview/**` 读取。
- 当前主固件没有真实 shutdown flow，且没有独立 over-power 状态源，因此对应 cue 必须保持 dormant。

## 变更记录（Change log）

- 2026-03-12: 初始化规格，冻结主固件运行时 cue 映射、dormant cue 结论与验收口径。
- 2026-03-12: 实现完成，主固件切换到运行时 cue 服务，共享音频核心与文档/构建验证同步落地。
