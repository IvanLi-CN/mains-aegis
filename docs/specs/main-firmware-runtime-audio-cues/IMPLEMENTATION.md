# 主固件运行时音效接入，替代开机 Demo 播放链路 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Lifecycle: archived
- Implementation: 见下方迁移状态与覆盖记录。

## Migrated Implementation Record

- Status: 已完成
- Created: 2026-03-12
- Last: 2026-07-01

## Migrated Delivery Record

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
- `Dashboard/Menu/AUDIO` 运行态已接上前面板导航状态机；`UP/DOWN` 切换 `ACTION/SYSTEM` 分组，`LEFT/RIGHT` 调整当前分组音量，并向主循环抛出独立的 beeper preview action。AUDIO 页通过左上 `BACK` 按钮返回 Menu，触摸 `ACTION/SYSTEM` 行切换分组，触摸音量滑条手指热区会按最近档位吸附；按下可就近设置，拖拽可连续切换档位，且只在档位变化或首次按下时触发 beeper preview。
- `B. Warm Tap` 已冻结为交互操作音资产：有效触摸 / 有效按键使用 `interaction_touch.wav`，USB-C 插入使用 `usb_c_insert.wav`；文档站点通过 `docs/audio-cues.md` 集中预览本次 2 个交互音与既有 15 个系统音。
- `AudioManager` 已新增 `InteractionTouch` 与 `UsbCInsert`，二者走 `ACTION` route、one-shot playback，并使用独立固件资产 `firmware/assets/audio/interaction-cues/*.wav`。
- 前面板输入处理通过 `pending_interaction_feedback` 记录实际状态变化或业务 `UiAction`，主循环消费后请求 `interaction_touch`；音量试听 `BeeperPreview` 保持专用 preview cue，不叠加 touch 音。
- USB-PD 主循环以 `UsbPdPortState.attached` 的 false -> true 边沿触发 `usb_c_insert`，并在 negotiation focus 内同步更新边沿状态，避免 attach 保持态和协议刷新误响。
- `Menu` 页触摸命中与按键导航语义一致：左右箭头切换当前菜单项，`DASHBOARD` 图标返回 Dashboard，`AUDIO` 图标进入音量设置，占位图标不触发业务动作。
- `AudioManager` 现在按 `ACTION / SYSTEM` route 持有独立 gain LUT（共享刻度 `0..6`）；用户调节音量时立即播放内部 `volume_preview` 双脉冲试听音，不再借用 `charge_started` 等业务/告警 cue。
- Warning cue 的 loop state 只在状态边沿变化时重置，steady-state 轮询期间继续保持 `interval_loop(2000ms)` 节流。
- Active loop cue 被更高优先级 cue 抢占后会回灌待播队列，避免 warning/error loop 在抢占场景下丢失“首次恢复播放”机会。
- `test-fw` 已改为复用共享音频模块，保留人工点播、抢占和同级 FIFO 验证能力。
- `PowerManager` 已输出运行时音效快照与边沿接口，主固件不再依赖 UI snapshot 差分来判定业务音效。
- BMS 激活 / isolation 路径上的 early-return 现在也会刷新音效快照，避免运行时 cue 在激活窗口内冻结。
- `mains_absent_dc` 已区分“初始无市电”与“已知状态之间掉电边沿”，避免电池冷启动时误报一次市电丢失告警。
- `mains_absent_dc` 在 charger 通信临时退回 `Unknown` 期间会保留已激活 loop；只有明确恢复到 `Some(true)` 才停播，避免断电告警在链路抖动后永久静默。
- 运行时 `mains_present` 真相源已统一到 `DC5025 VIN>=3V`；`BQ25792 input_present` 继续服务 charger 本地逻辑与诊断，但不再参与音频 cue 的市电判定。
- 若 `VIN` 只发生瞬时采样缺失，或因运行态暂时跳过 `VIN` 遥测而错过单个采样周期，运行时音效继续沿用最近一次已知的 `VIN` 市电状态；只有新的有效 `VIN` 样本跨过 `3V` 门槛时才产生来电/断电 cue。
- 若 `VIN` 连续缺失，或连续多个周期都在跳过 `VIN` 遥测而超出瞬时容错窗口，运行时音效回退到 charger `input_present` 作为降级兜底，避免把过期的 `VIN` 市电状态无限期锁存。
- 当运行时只是从 `VIN` 真相源切换到 charger 降级兜底，或从 charger 降级兜底切回 `VIN` 真相源时，不得把“数据源切换”伪造为 `mains_present_dc` / `mains_absent_dc` 边沿。
- `high_stress` 运行时信号已并入 TMP112 `TLOW` 条件；即使 charger 未上报热状态，只要实际温度越过 `TLOW` 且未触发停机，仍会触发该 cue。
- BMS protection / permanent-failure 状态已在自检结果中种子化，进入主循环前即可驱动 `battery_protection` 的首次调度。
- TPS OVP/OCP runtime state 已细化为按通道持有；只有成功读取到某路 TPS `STATUS` 时才会覆盖该路 fault seed，未读到的通道继续保留自检/上次有效观测结果。
- 主循环现在会先完成 power/audio 状态同步，再向 DMA ring 推入下一批 PCM 数据；本轮 hotfix 把 DMA ring 容量恢复到约 2.0 秒，但仅在最早期 boot prefill 保留约 1.0 秒余量；进入自检回调后收敛到约 0.9 秒水位，运行期保持约 1.3 秒水位，并在 UI tick / 重绘后立即补一次 DMA，避免打开 BMS 激活弹层等整帧重绘场景把 runtime DMA ring 拖空后永久静音。
- 运行期若偶发 `DmaError::Late`，应视为可恢复的 refill 迟到，而不是永久熔断音频服务；下一轮 `sync_runtime_audio` 必须能重新拉起仍处于 active 的 warning / error cue。
- BMS 激活流程一旦拿到可信快照，必须同时刷新 UI snapshot 与 `bms_audio` 运行时状态；像 `no_battery + rca_alarm` 这类激活结果不能只显示在界面上而不进入 warning cue 判定。
- BMS 激活若以 `no_battery` 收尾，后续短时间内的 BQ40 invalid / absent 轮询不得立刻把语义翻回 `module_fault`；在下一次明确的正常 pack 快照或新的激活尝试之前，应继续维持 `no_battery + rca_alarm` 的 warning 音频语义，并压住由同一轮自检残留的 `module_fault` 聚合结果，避免 warning / error cue 交替打架。
- BMS 激活若以 `no_battery` 收尾，则同一保持窗口内也不得再额外拉起 `battery_protection`；`PF/OCA/TCA/OTA/TDA` 这类位若来自该次 `no_battery` 快照，只能保留给诊断/UI，不得与 `BatteryLowWithMains` 同时播报。
- 运行期若出现 `DmaError::Late`，允许后续 refill 恢复 DMA 供给，但不得通过 `audio_manager.stop()` 把当前 warning cue 从头重启；否则用户会听到额外的重复起音/“嘟声”伪装成第二个提示音。
- 运行期 `DmaError::Late` 的诊断日志必须从“每轮一条噪声 warn”收敛为 burst 级事件：`detected`、`recovered`、`disabled` 三类，并附带 `current cue / queued / refill_budget / consecutive_late / recovery_attempts`，方便把“业务 cue 误触发”和“transport underrun”区分开。
- 当 `module_fault` / `battery_protection` / `battery_low` 发生切换时，运行期必须刷新 DMA ring，清掉已缓冲的旧 cue 采样；否则即使逻辑层已切 cue，扬声器仍可能继续播出上一段错误音并与新 cue 混音。
- 运行期强制刷新 DMA ring 时，切换边界必须附带短淡出/淡入过渡，避免直接从旧采样硬切到 0 或新 cue 导致 click/pop 爆音。
- 电池相关音效优先级现在固定为：`BatteryProtection > BatteryLowNoMains/BatteryLowWithMains`；若两者同时为真，运行时音频快照必须把低电状态收敛为 `Inactive`，从源头禁止 warning/error 并发。
- `BatteryProtection`、`ModuleFault` 等 `continuous_loop` cue 的播放状态必须跨样本尾部保持同一个 active playback，不得通过“播完后重新 request 同一 cue”来续播。
- 对开机即缺失的 BMS，只要自检已判定 “missing/err” 且当前板级仍存在 charger path 或 BMS 输出恢复门控，运行时音频快照就必须在首次刷新时携带 `module_fault=true`，避免故障音被 `runtime_seen` 门控吞掉。
- 运行时后接入的 BMS 现在会把“曾成功建链”状态保留下来；即便后续轮询掉线，`module_fault` 也不会再被启动快照门控吞掉。
- `shutdown_mode_entered` 与 `io_over_power` 继续保持 dormant，并在主固件中明确不触发。
- 运行期 DMA transport 现在单独维护 underrun burst 状态机：transition flush 仍用于 cue 切换，underrun recovery 只负责零填充 + re-prime transport，不再额外 arm transition bridge 或伪造新的 cue 起音。


## References

- `./SPEC.md`
- `./HISTORY.md`
