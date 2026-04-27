# 前面板自动熄屏（#d8p4q）

## 状态

- Lifecycle: active
- Implementation: 已实现（测试时序）
- Created: 2026-04-27
- Last: 2026-04-27

## 背景 / 问题陈述

前面板当前常驻亮屏，适合调试但不适合长时间运行。设备需要在无人操作时逐级降低显示功耗，同时在出现需要用户关注的异常时保持亮屏，避免提示音响起但屏幕已经不可见。

## 目标 / 非目标

### Goals

- 无操作后自动进入低亮、关背光、显示控制器 sleep 三段状态。
- 任意触摸或五向按键可唤醒，唤醒后恢复全亮并重绘当前页面。
- 运行时 warning/error 类用户关注状态保持全亮，不进入 idle 降级。
- 当前实现采用测试时序：`30s` 低亮、`35s` 关背光、`40s` sleep；硬件确认后恢复正式默认 `180s / 240s / 245s`。

### Non-goals

- 不做整机 deep-sleep、RTC GPIO hold 或系统级低功耗策略。
- 不改变前面板 UI 布局、触摸热区、按键导航或音效调度策略。
- 不新增用户可配置的亮度/超时时间设置。

## 范围

### In scope

- `firmware/src/display_power.rs`：前面板 idle power 纯逻辑状态机。
- `firmware/src/front_panel.rs`：GC9307 亮度、背光、sleep/wake 命令接入。
- `firmware/src/main.rs`：从运行时音效/告警信号派生 `attention_hold`。
- `firmware/host-unit-tests`：导出纯逻辑模块并纳入 host test。

### Out of scope

- 实机 flash/monitor 验证由后续硬件确认任务执行。
- `test-fw` 专用调试入口暂不提供独立超时配置。

## 行为规格

- `Awake`：正常全亮，GC9307 DBV 写 `0xFF`，`BLK(GPIO13)` 打开。
- `Dimmed`：空闲 `30s` 后写 GC9307 `0x51` DBV 为最低非零值，背光仍保持打开。
- `BacklightOff`：空闲 `35s` 后关闭 `BLK`。
- `Sleeping`：空闲 `40s` 后发送 `Display OFF (0x28)` 和 `Sleep IN (0x10)`。
- 唤醒：触摸或任意按键在非 `Awake` 状态下只负责唤醒，不透传为业务点击；从 sleep 唤醒时发送 `Sleep OUT (0x11)`，等待 `120ms` 后发送 `Display ON (0x29)`，再恢复 DBV/背光并重绘。
- `attention_hold=true` 时立即保持/恢复 `Awake`，并把 idle 计时重置到当前时刻；解除后重新从完整阈值开始计时。
- `attention_hold` 只覆盖用户可处理或需要避险的状态：高温压力、低电、保护、模块故障、输出过压/过流、关断保护。USB-PD recovery、充电策略等待、单纯输入源缺失等内部恢复/状态提示不阻断熄屏。

## 验收标准

- Host unit tests 覆盖 `30s / 35s / 40s` 三段阈值、触摸/按键唤醒、attention hold 阻断与解除后重新计时。
- 主固件 compile check 通过。
- 代码中保留正式默认常量 `180s / 240s / 245s`，后续恢复时只需替换策略常量。

## References

- `docs/specs/xy6cz-front-panel-refresh-pipeline/SPEC.md`
- `docs/specs/h43mk-main-firmware-runtime-audio-cues/SPEC.md`
- `docs-site/docs/design/front-panel-and-firmware.md`
- `docs/pcbs/front-panel/README.md`
