# 风扇温控与故障保护

> 当前有效规范以本文为准；实现覆盖与当前状态见 `./IMPLEMENTATION.md`，关键演进原因见 `./HISTORY.md`。

## 背景 / 问题陈述

- 主板已预留 `FAN_TACH(GPIO34)`、`FAN_EN(GPIO35)`、`FAN_VSET_PWM(GPIO36)`，但固件尚未接入任何风扇控制逻辑。
- 当前功率级只有 `TMP112A(0x48)` / `TMP112B(0x49)` 热点温度和 `THERM_KILL_N` 硬停机保护；缺少软调速与风扇反馈兜底。
- 需要一个可验证、低假设的控制器：按温度渐进调节 PWM，并在温度或 tach 异常时进入散热保护。

## 目标 / 非目标

### Goals

- 接入 `FAN_EN` 与 `FAN_VSET_PWM`，实现 `0..100%` 渐进 PWM 控制；`FanLevel` 仅作为日志与状态展示分组，不是温控策略的固定三档输出。
- 控制口径固定为 `max(tmp_a, tmp_b, bms)`；单路温度缺失时退化到其余来源，双路缺失时全速保护。
- 当 `BQ25792` 进入 `TS_WARM` / `TS_HOT` 或内部 `TREG` 时，风扇必须直接抢占到全速；该 override 不依赖 `TMP112` / BMS 当前数值。
- 温控目标为 `40C`，停转阈值为 `<37C`。控制器每 `500ms` 调整一次：低于目标时按 `5%` 下降但不低于 `10%`；达到目标后按温差分别增加 `5% / 10% / 15%`，最高 `100%`。
- 接入 `FAN_TACH` 边沿计数；风扇被命令运行且 `2s` 内无脉冲时，记录故障并强制全速。
- `tach` 故障必须保持锁存，直到再次观察到已确认的真实 `FAN_TACH` 脉冲活动后才允许清除；单个毛刺边沿不能解除保护。
- `tach` 恢复确认窗口允许主循环在两次真实脉冲之间看到短暂的“无新脉冲”轮询；只有当恢复窗口静默时间达到 `tach_timeout_ms` 后，才允许丢弃之前的恢复证据并重新开始确认。
- BMS activation 的总线静默窗口内，风扇控制只能使用最近一次缓存温度，不能额外发起 TMP112 轮询破坏隔离窗口。
- 输出可观察日志，覆盖档位切换、温度源退化、tach 超时与故障恢复。
- `fan: telemetry` 必须同时输出策略请求值与实际硬件应用值；当进入 fail-safe 时，日志要能看出硬件已被强制到高风量状态。
- 当风扇被 charger thermal override 抢占时，日志必须显式标出 `charger_thermal` 来源，以及 `ts_warm / ts_hot / treg` 快照。
- `FAN_TACH` 的 bring-up 可观察性必须保留，但不能靠通用 IRQ 日志刷屏；需要单独的限频 tach `info` 日志，且在默认 `DEFMT_LOG=info` 配置下可见。
- tach 每转脉冲数是风扇配件相关的构建期参数：支持 `fan-tach-1-ppr` 与 `fan-tach-2-ppr`，未指定时默认 `2 PPR`，同时指定必须编译失败。该参数只用于 RPM 换算与采样窗口，不改变“是否观察到脉冲”的故障保护语义。
- 若 `FAN_VSET_PWM` 初始化失败，或后续运行期 duty 应用失败，固件必须退化到“`FAN_EN` 常开且 `FAN_VSET_PWM` 强制高电平/满占空比”的保守散热模式，而不是静默失去风扇控制。

### Non-goals

- 不做 RPM 闭环控制；PPR 只用于可观察性换算，不能被运行时温控策略当作闭环反馈。
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
  - 新增纯逻辑风扇策略模块，承载温度选择、渐进 PWM 控制与 tach 故障状态机。
- `firmware/src/output/mod.rs`
  - `Config` / `PowerManager` 接入风扇策略状态与日志。
  - 输出当前风扇命令状态，供主循环应用到硬件。
- `firmware/README.md`
  - 补充风扇日志契约与 bench 验证步骤。

### Out of scope

- 前面板页面渲染、触摸交互、UI 文案变更。
- 风扇型号特定调优（起转电压、精确 PWM 曲线、RPM 标定）。

## 接口变更（Interfaces）

- `mains_aegis_firmware::fan`：风扇纯逻辑模块及构建期 PPR 解析。
- `output::Config`：新增风扇策略配置。
- `PowerManager::fan_command()`：新增只读接口，返回当前风扇输出命令。

## 验收标准（Acceptance Criteria）

- `bash firmware/scripts/run-host-unit-tests.sh` 通过，至少覆盖：
  - `37C` 以下停转，达到 `40C` 后按温差与 `500ms` 控制周期渐进升速。
  - 低于 `40C` 时按 `5%` 渐进降速且运行占空比不低于 `10%`。
  - 单路温度缺失时退化到另一侧；双路缺失时全速保护。
  - `BQ25792 TS_WARM/TREG` 可在低于普通风扇阈值时直接把风扇拉到全速，且退出后恢复正常温控。
  - 风扇运行命令下 `2s` 无 tach 脉冲触发故障并锁到全速；脉冲恢复后解除故障。
  - 默认构建与显式 `fan-tach-2-ppr` 均使用 `2 PPR`，显式 `fan-tach-1-ppr` 使用 `1 PPR`，双选构建失败。
- `cargo build --release`（`firmware/`）通过。
- 运行日志存在 `fan:` 事件，至少覆盖：
  - 档位变化；
  - 温度源退化 / 双路缺失；
  - charger thermal override 进入 / 退出；
  - tach 超时；
  - tach 恢复。
- 若板卡可用，`mcu-agentd monitor esp --reset` 可观察到温度驱动的档位变化与 tach 故障保护日志。


## Visual Evidence

PR: none
