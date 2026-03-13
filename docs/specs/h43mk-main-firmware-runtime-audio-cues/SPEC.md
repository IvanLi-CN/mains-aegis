# 主固件运行时音效接入，替代开机 Demo 播放链路（#h43mk）

## 状态

- Status: 已完成
- Created: 2026-03-12
- Last: 2026-03-13

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

- `boot_startup`：上电进入自检后立即请求一次，可与自检并行，且允许被更高优先级 cue 抢占。
- `mains_present_dc` / `mains_absent_dc`：输入存在位在“已知状态之间”变化时触发；通信从 unknown 恢复到 known 时保持静默。
- `charge_started` / `charge_completed`：charger 状态在“已知相位之间”进入“充电中 / 完成”时触发；首次建链或通信恢复后的 unknown -> known 不补播 one-shot。
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
- 主固件上电后只请求一次 `boot_startup`，允许在自检期间开始播放且不阻塞自检，不再出现 6 段 demo playlist 的阻塞播放与对应日志序列。
- 主循环期间 power/front-panel tick 节奏保持可用，音频服务每轮并入调度而不独占流程。
- 若 I2S / DMA 音频初始化失败，主固件必须记录告警并继续进入主循环；音频链路允许降级为静默，但不得因音频 bring-up 失败而 panic。
- 若运行期 DMA `available()` / `push_with()` 连续失败，主固件必须关闭运行时音频调度并静默降级；不得让 cue 在无 DMA 消费者时停留在“假播放”状态。
- BMS 激活 / isolation 窗口期间，运行时音效快照仍需持续刷新；激活流程可以短路主循环中的其他动作，但不能让 cue 状态冻结。
- 调度语义固定为：
  - `status` -> `one_shot`
  - `warning` -> `interval_loop(2000ms)`
  - `error` -> `continuous_loop`
  - 优先级：`Error > Warning > Status > Boot`
  - 同优先级 `one_shot` 保持 FIFO。
- 运行时场景正确触发/停播：市电恢复/丢失、充电开始/完成、电池低电（区分有无市电）、高压力进入/退出、模块通信故障进入/恢复、保护/过压/过流进入/清除。
- 通信恢复语义：
  - charger 输入/相位从 unknown 恢复到 known 时，不得伪造 `mains_present_dc`、`charge_started`、`charge_completed`。
  - 冷启动时若 `mains_present == Some(false)`，不得仅凭初始快照就触发 `mains_absent_dc`；该 cue 只能由已知状态之间的掉电边沿首发，随后再按 loop 语义保持。
  - 自检阶段已观察到的 TPS OVP/OCP/SCP 必须能种子化到运行时音效状态；运行期只能在成功读取到对应 TPS 通道状态后覆盖该通道 fault 位，不能因为该路输出被门控或单次读失败就把 seed 清零。
  - 自检阶段已观察到的 BMS protection / permanent-failure 状态必须能种子化到运行时音效状态，不能等首次 runtime poll 才补发 `battery_protection`。
  - 自检阶段带入的 warning/error loop cue 必须在进入主循环前完成首次调度，不能在首轮 `power.tick()` 前被静默清掉。
  - 已在播放中的 active loop cue 若被更高优先级 cue 抢占，必须保留待播资格；高优先级 cue 结束后应立即恢复，而不是等待下一个 loop interval。
  - `module_fault` 只针对运行期实际检测到且必需的模块；因配置关闭或本板未装的可选模块不得常驻拉高该 cue。
- `shutdown_mode_entered` 与 `io_over_power` 在主固件本轮保持静默，且文档明确注明等待真实状态源后再接入。

## 里程碑（Milestones）

- [x] M1: 共享音频核心抽出并被 `test-fw` 复用。
- [x] M2: `PowerManager` 输出运行时音效信号/边沿接口。
- [x] M3: 主固件主循环接入常驻音效服务并删除 demo playlist 路径。
- [x] M4: 文档与规格同步更新。
- [x] M5: 主固件与 `test-fw` 构建验证完成。

## 实现结果

- 主固件已移除阻塞式 demo playlist，改为在主循环内常驻调度共享 `AudioManager`。
- 主固件的 I2S / DMA 音频初始化已改为 best-effort；初始化失败时只记录告警并静默降级，不阻断自检与主循环启动。
- 运行期若 DMA refill 持续报错，主固件会关闭音频调度并清空队列，避免 cue 在无消费者时永久卡住。
- 共享播放核心已落到 `firmware/src/audio.rs`，统一 15 组 cue、优先级、WAV 解析/重采样、DMA `fill()` 与播放状态接口。
- Warning cue 的 loop state 只在状态边沿变化时重置，steady-state 轮询期间继续保持 `interval_loop(2000ms)` 节流。
- Active loop cue 被更高优先级 cue 抢占后会回灌待播队列，避免 warning/error loop 在抢占场景下丢失“首次恢复播放”机会。
- `test-fw` 已改为复用共享音频模块，保留人工点播、抢占和同级 FIFO 验证能力。
- `PowerManager` 已输出运行时音效快照与边沿接口，主固件不再依赖 UI snapshot 差分来判定业务音效。
- BMS 激活 / isolation 路径上的 early-return 现在也会刷新音效快照，避免运行时 cue 在激活窗口内冻结。
- `mains_absent_dc` 已区分“初始无市电”与“已知状态之间掉电边沿”，避免电池冷启动时误报一次市电丢失告警。
- `mains_absent_dc` 在 charger 通信临时退回 `Unknown` 期间会保留已激活 loop；只有明确恢复到 `Some(true)` 才停播，避免断电告警在链路抖动后永久静默。
- `high_stress` 运行时信号已并入 TMP112 `TLOW` 条件；即使 charger 未上报热状态，只要实际温度越过 `TLOW` 且未触发停机，仍会触发该 cue。
- BMS protection / permanent-failure 状态已在自检结果中种子化，进入主循环前即可驱动 `battery_protection` 的首次调度。
- TPS OVP/OCP runtime state 已细化为按通道持有；只有成功读取到某路 TPS `STATUS` 时才会覆盖该路 fault seed，未读到的通道继续保留自检/上次有效观测结果。
- 主循环现在会先完成 power/audio 状态同步，再向 DMA ring 推入下一批 PCM 数据，并把 DMA ring 缩短到约 0.5 秒缓存，降低高优先级 cue 的实际听感抢占延迟。
- 运行时后接入的 BMS 现在会把“曾成功建链”状态保留下来；即便后续轮询掉线，`module_fault` 也不会再被启动快照门控吞掉。
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
- 2026-03-12: review fix，修正 warning cue 在 steady-state 轮询下的重播间隔，保持 2000ms 节流语义。
- 2026-03-13: merge-proof fix，补齐 I2S/DMA 初始化失败的静默降级路径、抢占后 active loop cue 的立即恢复语义，以及 TMP112 `TLOW` 驱动的 `high_stress` 触发。
- 2026-03-13: merge-proof fix，补齐 BMS 激活 / isolation 窗口内的音效快照刷新，并把 TPS OVP/OCP seed 改为按通道保留、按成功读回覆盖。
- 2026-03-13: merge-proof fix，修正 `mains_absent_dc` 在电池冷启动时的误报，并把 BMS protection / PF seed 接入运行时 `battery_protection`。
- 2026-03-13: merge-proof fix，缩短 DMA ring 并把运行时 cue 同步提前到 DMA refill 之前，降低高优先级告警的实际播报延迟；同时让 `mains_absent_dc` 跨 charger `Unknown` 抖动保持激活态。
- 2026-03-13: merge-proof fix，给运行时 BMS 建链增加 sticky presence，避免激活后掉线时 `module_fault` 被启动快照门控吞掉。
- 2026-03-13: merge-proof fix，补齐运行期 DMA 故障后的静默降级路径，避免 `AudioManager` 在无 DMA 消费者时卡在假播放状态。
