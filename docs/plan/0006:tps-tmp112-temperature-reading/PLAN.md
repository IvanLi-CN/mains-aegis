# TPS 热点温度采样：TMP112A 读数与日志口径（#0006）

## 状态

- Status: 待实现
- Created: 2026-01-24
- Last: 2026-01-24

## 背景 / 问题陈述

- 主板在两颗 `TPS55288` 热源附近各放置 1 颗数字温度传感器 `TMP112A`，用于板级热点温度采样与后续过温策略（见 `docs/power-monitoring-design.md` 与 `docs/i2c-address-map.md`）。
- 本计划要落地“TPS 边上的 TMP（TMP112A）”温度读取，并将读数**追加到**既有的 `telemetry ...` 输出行中（不改变原字段与单位），以便 bring-up 与回归时一眼能把电压/电流/温度对齐观察。
- 需要冻结“读哪两颗、单位是什么、输出追加哪些字段、失败如何表现”的最小契约，避免后续实现阶段边做边改导致验证口径漂移。

## 目标 / 非目标

### Goals

- 固件能够在 `I2C1` 上周期性读取两颗 `TMP112A` 的温度读数，并把读数**追加到** `telemetry ...` 输出行中（见 `./contracts/cli.md`）。
- 固件在温度传感器缺失/总线故障场景下不 panic，且按稳定错误分类输出占位并限频（避免刷屏）。
- 冻结 “TMP112A 逻辑通道 ↔ I2C 地址 ↔ 物理含义（TPS-A/TPS-B 热点）” 与采样周期等最小配置形状（见 `./contracts/config.md`）。

### Non-goals

- 不在本计划内实现过温控制闭环（降额/关断/自动恢复策略）；只交付“读数 + 观测口径”。
- 不在本计划内实现/冻结 `TMP112A.ALERT` 的阈值配置与去抖策略；`THERM_KILL_N` 的硬件链路只做“可见性补充”（是否读取该 GPIO 取决于本计划决策）。
- 不在本计划内与上位机协议对接（HTTP/RPC/文件落盘等）；仅定义固件串口/日志口径。

## 范围（Scope）

### In scope

- `firmware/`：新增 `TMP112A` 最小驱动封装（I2C 读温度寄存器 + 解码），并按周期采样两颗传感器。
- `firmware/`：在既有 `telemetry ...` 行中追加温度字段（两路各 1 行/周期；输出格式见 `./contracts/cli.md`）。
- 文档：冻结并记录通道映射、I2C1 配置、采样周期、输出单位与错误占位策略（见 `./contracts/*.md`）。

### Out of scope

- `THERM_KILL_N(GPIO40)` 的硬停机链路联调与策略闭环（属于后续“过温保护”计划）。
- 任何对 `#0005` 已冻结计划文档的修改（本计划只在自身契约中定义“字段追加规则”，保证兼容）。

## 需求（Requirements）

### MUST

- 固件必须支持读取两颗 `TMP112A`（7-bit I2C 地址）：
  - `TMP112A(OUT-A 热点 / TPS-A 热点)`：`0x48`
  - `TMP112A(OUT-B 热点 / TPS-B 热点)`：`0x49`
  - 总线：`I2C1`（`GPIO48=I2C1_SDA`，`GPIO47=I2C1_SCL`；目标速率 `400kHz`；见 `docs/i2c-address-map.md`）
- 固件必须按固定周期输出 `telemetry ...`（温度字段作为追加字段）：
  - 周期与通道映射见 `./contracts/config.md`
  - 追加字段与错误占位见 `./contracts/cli.md`
- 单颗或两颗 `TMP112A` 不可达（NACK/timeout/总线错误）时：
  - 固件不得 panic；
  - 仍必须按周期输出 `telemetry ...`（对失败字段输出 `err(<kind>)` 占位）；
  - 必须对错误日志做限频（避免刷屏；限频口径写入契约）。
- 温度单位必须在契约中固定为整数单位（避免浮点）：`temp_c_x16`（`°C * 16`，即 `1/16 °C`）并提供可读换算口径（由 `./contracts/cli.md` 冻结）。
- 必须读取并输出 `therm_kill_n`（`GPIO40(THERM_KILL_N)` 电平；`1`=高，`0`=低）。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes） |
| --- | --- | --- | --- | --- | --- | --- | --- |
| TMP112A 通道/地址映射与采样周期 | Config | internal | New | ./contracts/config.md | firmware | firmware | 冻结 0x48/0x49 与通道命名 |
| 遥测日志字段追加（TMP112A 温度 + THERM_KILL_N） | CLI | internal | Modify | ./contracts/cli.md | firmware | developers | 兼容：不改变原字段；只追加新字段 |

### 契约文档（按 Kind 拆分）

- [contracts/README.md](./contracts/README.md)
- [contracts/config.md](./contracts/config.md)
- [contracts/cli.md](./contracts/cli.md)

## 验收标准（Acceptance Criteria）

- Given 主板已供电且 `I2C1` 可用，
  When 固件启动运行并进入采样循环，
  Then 日志中能以固定周期输出两行 `telemetry ...`，且每行追加包含 `tmp_addr/temp_c_x16/therm_kill_n` 字段（见 `./contracts/cli.md`）。

- Given 仅有一颗 `TMP112A` 可响应（另一颗缺件/焊接异常/地址冲突），
  When 固件周期性采样两颗器件，
  Then 固件不 panic；可响应的通道输出 `temp_c_x16=<int>`；不可响应的通道输出 `temp_c_x16=err(<kind>)`，且错误日志不刷屏（限频满足契约）。

- Given `I2C1` 临时故障导致单次读取失败，
  When 下一次采样周期到来，
  Then 仍持续输出 `telemetry ...`；失败周期使用 `err(<kind>)` 占位；恢复后回到 `temp_c_x16=<int>`，且无 panic。

## 实现前置条件（Definition of Ready / Preconditions）

- 已确认“TPS 边上的 TMP”指的是 `TMP112A(0x48/0x49)`（而非其它温度源/ADC NTC/TPS 内部温度）。
- 已冻结温度追加字段形状：`temp_c_x16`（`°C * 16`）与 `therm_kill_n`（见 `./contracts/cli.md`）。
- 已确认兼容策略：只追加字段，不改变既有 `telemetry ...` 字段与单位（见 `./contracts/cli.md`）。
- 已完成最小代码定位：当前固件入口为 `firmware/src/main.rs`；预计新增 `TMP112A` 驱动模块（例如 `firmware/src/tmp112.rs`）并在主循环/定时任务中调用。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: 若实现包含 `TMP112A` 原始温度寄存器到 `temp_c_x16` 的解码与符号处理，需提供最小单元测试覆盖（正温、负温、边界值、保留位处理）。
- Integration tests: 至少一次上板手工验证步骤固化到 `firmware/README.md`（仅描述人类操作：构建/烧录/观察日志与环境温度对比；不要求 Agent 执行设备写入类动作）。

### Quality checks

- 使用仓库既有质量检查（`cargo fmt` / `cargo clippy` / `cargo build` 等），不引入新工具链。

## 文档更新（Docs to Update）

- `firmware/README.md`: 增加 “TMP112A 温度采样 bring-up 验证” 章节（I2C 地址、周期、日志样例、常见故障排查）。
- `docs/power-monitoring-design.md`: 若本计划最终冻结了日志口径与单位，补充“固件上报字段名/单位”小节引用 `#0006`（避免文档与实现口径漂移）。

## 实现里程碑（Milestones）

- [ ] M1: 落地 `TMP112A` 最小驱动封装（I2C 读温度寄存器 + 解码为 `temp_c_x16`）
- [ ] M2: 固化遥测字段追加（`telemetry ...` 行追加 `tmp_addr/temp_c_x16/therm_kill_n`；错误占位与限频）
- [ ] M3: 固化上板验证步骤到 `firmware/README.md`

## 方案概述（Approach, high-level）

- 温度采样走 `I2C1` 同总线策略：实现应避免在故障场景下死循环重试；错误分类与限频口径按契约稳定化，便于回归。
- 日志输出以“可定位、可比较”为优先：建议同时输出 `raw`（原始寄存器值）以降低后续查错成本（是否启用由契约冻结）。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：
  - `I2C1` 上设备较多（`TPS55288/INA3221/TMP112A/...`），实现需避免长时间阻塞与高频重试导致其它设备访问饥饿。
  - `TMP112A` 读数代表“板级热点”而非结温；阈值策略若未来加入，需要样机热测试回归（不在本计划内）。
- 假设（需主人确认）：
  - None

## 变更记录（Change log）

- 2026-01-24: 初始化计划与契约骨架

## 参考（References）

- `docs/i2c-address-map.md`
- `docs/power-monitoring-design.md`
- `docs/plan/0005:tps55288-control/PLAN.md`（I2C1 共用与遥测口径相关）
