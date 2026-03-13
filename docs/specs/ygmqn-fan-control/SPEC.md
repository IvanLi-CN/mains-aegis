# 风扇温控与故障保护（#ygmqn）

## 状态

- Status: 待实现
- Created: 2026-03-13
- Last: 2026-03-13

## 背景 / 问题陈述

- 主板已预留 `FAN_TACH(GPIO34)`、`FAN_EN(GPIO35)`、`FAN_VSET_PWM(GPIO36)`，但固件尚未接入任何风扇控制逻辑。
- 当前功率级只有 `TMP112A(0x48)` / `TMP112B(0x49)` 热点温度和 `THERM_KILL_N` 硬停机保护；缺少软调速与风扇反馈兜底。
- 需要先收敛一个可验证、低假设的 V1：按温度分档控速，并在温度或 tach 异常时进入散热保护。

## 目标 / 非目标

### Goals

- 接入 `FAN_EN` 与 `FAN_VSET_PWM`，实现三档风扇控制：关 / 中速 / 全速。
- 控制口径固定为 `max(tmp_a, tmp_b)`；单路温度缺失时退化到另一侧，双路缺失时全速保护。
- 温控阈值固定为 `<40C=关`、`40~49C=中速`、`>=50C=全速`；回滞固定 `3C`。
- 从中速或全速退出后保留 `10s` 余冷，余冷期间维持低速。
- 接入 `FAN_TACH` 边沿计数；当命令档位为中速/全速且 `2s` 内无脉冲时，记录故障并强制全速。
- 输出可观察日志，覆盖档位切换、温度源退化、tach 超时与故障恢复。

### Non-goals

- 不做 RPM 闭环控制，也不假定每转脉冲数。
- 不改前面板 UI 数据模型或新增风扇卡片。
- 不改 PCB / 原理图 / 外部硬件保护网络。

## 范围（Scope）

### In scope

- `firmware/src/main.rs`
  - 初始化 `GPIO35` 为风扇使能输出。
  - 配置一个独立 LEDC low-speed PWM 通道驱动 `GPIO36`，固定 `25kHz`。
  - 初始化 `GPIO34` 为上拉输入并注册 GPIO 中断。
- `firmware/src/irq.rs`
  - 增加 `FAN_TACH` 中断计数与 `IrqSnapshot` 字段。
- `firmware/src/fan.rs`
  - 新增纯逻辑风扇策略模块，承载温度选择、三档曲线、回滞、余冷与 tach 故障状态机。
- `firmware/src/output/mod.rs`
  - `Config` / `PowerManager` 接入风扇策略状态与日志。
  - 输出当前风扇命令状态，供主循环应用到硬件。
- `firmware/README.md`
  - 补充风扇日志契约与 bench 验证步骤。

### Out of scope

- 前面板页面渲染、触摸交互、UI 文案变更。
- 风扇型号特定调优（起转电压、精确 PWM 曲线、RPM 标定）。

## 接口变更（Interfaces）

- `esp_firmware::fan`：新增风扇纯逻辑模块。
- `output::Config`：新增风扇策略配置。
- `PowerManager::fan_command()`：新增只读接口，返回当前风扇输出命令。

## 验收标准（Acceptance Criteria）

- `cargo test`（`firmware/`）通过，至少覆盖：
  - 温度跨越 `40C` / `50C` 时，档位正确切换。
  - 回滞 `3C` 生效，不会在阈值边缘单周期抖动。
  - 退出中速/全速后保留 `10s` 余冷，再关风扇。
  - 单路温度缺失时退化到另一侧；双路缺失时全速保护。
  - 中速/全速命令下 `2s` 无 tach 脉冲触发故障并锁到全速；脉冲恢复后解除故障。
- `cargo build --release`（`firmware/`）通过。
- 运行日志存在 `fan:` 事件，至少覆盖：
  - 档位变化；
  - 温度源退化 / 双路缺失；
  - tach 超时；
  - tach 恢复。
- 若板卡可用，`mcu-agentd monitor esp --reset` 可观察到温度驱动的档位变化与 tach 故障保护日志。

## 里程碑（Milestones）

- [ ] M1: 新增风扇 spec 与索引。
- [ ] M2: 接入 GPIO/PWM/tach 中断初始化。
- [ ] M3: 完成风扇状态机与 `PowerManager` 集成。
- [ ] M4: 补充测试、README 与日志契约。
- [ ] M5: 验证、PR 与 review-loop 收敛。

## 变更记录（Change log）

- 2026-03-13: 首版规格冻结 V1 风扇控制口径：最高温三档、`3C` 回滞、`10s` 余冷、`2s` tach 看门狗、异常全速保护。
