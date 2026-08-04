# 主固件运行时音效接入，替代开机 Demo 播放链路 演进历史

> 这里记录影响当前规范理解的关键演进；当前有效合同仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-03-12: 初始化规格，冻结主固件运行时 cue 映射、dormant cue 结论与验收口径。
- 2026-03-12: 实现完成，主固件切换到运行时 cue 服务，共享音频核心与文档/构建验证同步落地。
- 2026-03-12: review fix，修正 warning cue 在 steady-state 轮询下的重播间隔，保持 2000ms 节流语义。
- 2026-03-13: merge-proof fix，补齐 I2S/DMA 初始化失败的静默降级路径、抢占后 active loop cue 的立即恢复语义，以及 TMP112 `TLOW` 驱动的 `high_stress` 触发。
- 2026-03-13: merge-proof fix，补齐 BMS 激活 / isolation 窗口内的音效快照刷新，并把 TPS OVP/OCP seed 改为按通道保留、按成功读回覆盖。
- 2026-03-13: merge-proof fix，修正 `mains_absent_dc` 在电池冷启动时的误报，并把 BMS protection / PF seed 接入运行时 `battery_protection`。
- 2026-03-13: merge-proof fix，缩短 DMA ring 并把运行时 cue 同步提前到 DMA refill 之前，降低高优先级告警的实际播报延迟；同时让 `mains_absent_dc` 跨 charger `Unknown` 抖动保持激活态。
- 2026-03-13: merge-proof fix，给运行时 BMS 建链增加 sticky presence，避免激活后掉线时 `module_fault` 被启动快照门控吞掉。
- 2026-03-13: merge-proof fix，补齐运行期 DMA 故障后的静默降级路径，避免 `AudioManager` 在无 DMA 消费者时卡在假播放状态。
- 2026-03-13: hotfix，恢复约 2.0 秒 DMA ring 容量，并把 boot prefill / 自检 / 运行期 refill 收敛到分阶段受控水位，修复开始音截断与后续告警静音回归，同时避免高优先级 cue 再次被长缓存拖慢。
- 2026-03-13: hotfix，补齐“开机已判定 BMS 缺失/错误且 charger path 仍存在”时的 `module_fault` 种子化，避免开机起即缺失的 BMS 被 runtime-seen 门控静默掉故障音。
- 2026-03-13: hotfix，运行期水位进一步上调到约 1.3 秒，并在 UI tick 后立即补一次 DMA，修复打开 BMS 激活弹层等重型面板重绘时 runtime DMA `Late` 后整条音频服务被永久关闭的问题。
- 2026-03-13: hotfix，BMS 激活可信快照现在会同步刷新 `bms_audio`，并把运行期 `DmaError::Late` 从永久禁音改为可恢复重拉，避免 `no_battery` 结果只显示在 UI 上却没有后续 warning cue。
- 2026-03-13: hotfix，BMS 激活返回 `no_battery` 后会把该结果保持为运行时 warning 语义，直到拿到新的正常 BMS 快照或再次发起激活，避免 `BatteryLowWithMains` 与 `ModuleFault` 在后续抖动轮询里来回抢占。
- 2026-03-13: hotfix，`no_battery` 保持态会同时屏蔽同一轮自检残留的 `module_fault` 聚合，避免 `tps_a/tps_b=err` 等旧快照继续把错误音混进激活后的 warning 播报。
- 2026-03-13: hotfix，`no_battery` 保持态会同时压住 `battery_protection`，避免 `BatteryProtection` 与 `BatteryLowWithMains` 在激活完成后重叠播放。
- 2026-03-13: hotfix，运行期 `DmaError::Late` 改为“保留当前播放状态并等待下次 refill 恢复”，避免 warning cue 每次 underrun 都从头重启而听成第二个嘟声。
- 2026-03-14: hotfix，运行期 cue 发生切换时会重建并清空 DMA ring，避免旧的 `ModuleFault` 采样残留到激活后的 `BatteryLowWithMains` 阶段。
- 2026-03-14: hotfix，运行期 DMA flush 现在会带约 5 ms 的淡出/淡入过渡，降低 cue 切换瞬间的爆音。
- 2026-03-14: hotfix，`battery_protection` 现在全局高于低电提示音；当保护与低电同时成立时，只保留 `BatteryProtection`，并在日志中明确标记低电提示音被保护音压制。
- 2026-03-14: hotfix，`continuous_loop` cue 改为在样本末尾无缝回绕，避免 `BatteryProtection` 每约 1 秒重新 `start_playback` 一次而产生额外起音。
- 2026-03-14: hotfix，`continuous_loop` 的样本回绕现在保留重采样余量，不再在边界重复开头样本或丢掉尾部样本，进一步降低保护音循环接缝感。
- 2026-03-15: hotfix，运行时市电音效判定改为只消费 `DC5025 VIN>=3V`，修复 charger `input_present` 抖动导致的误判来电/断电音。
- 2026-03-15: hotfix，`VIN` 瞬时采样缺失时保留最近一次已知市电状态，避免 INA CH3 单次读失败伪造 `mains_absent_dc` / `battery_low_no_mains`；连续缺失则退回 charger `input_present` 兜底。
- 2026-04-04: hotfix，针对 idle 主板周期性“滴”声回归，把运行期 `DmaError::Late` 从每轮噪声日志改为 burst 级 `detected / recovered / disabled` 状态机；首次 `Late` 立即 re-prime transport，恢复成功后清 burst，5 秒窗口内连续 3 次 recovery attempt 仍失败则静默降级，避免在无业务 cue 边沿时由 DMA underrun 继续制造杂音。
- 2026-04-05: hotfix，`BQ25792 TS_WARM` 从 `high_stress` 播报条件中移除，避免温区预警在正常充电 warm 档位下每 2 秒重复提示；`TS_WARM` 改由界面状态与风扇全速 override 承载。
- 2026-06-08: 将 `Dashboard/Menu/AUDIO` 运行态正式接入 `AudioManager`，新增 `ACTION / SYSTEM` 分组 gain LUT 与独立 `volume_preview` 试听音；主固件 release 构建、host-side audio tests 与 `front-panel-preview` 已通过，`test-fw` 默认特性构建仍保留既有 `esp_rtos_*` 链接缺口。
- 2026-07-01: 冻结 `B. Warm Tap` 为有效触摸/按键与 USB-C 插入操作音；新增 ACTION route one-shot cue、文档站点对照页，并把前面板有效操作与 USB-PD attach 上升沿接入主固件播放路径。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
