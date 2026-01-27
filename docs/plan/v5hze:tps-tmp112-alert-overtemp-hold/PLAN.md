# TMP112A 过温告警输出：Comparator 模式保持输出（#v5hze）

## 状态

- Status: 待实现
- Created: 2026-01-27
- Last: 2026-01-27

## 背景 / 问题陈述

- 现状：主板在两颗 `TPS55288` 热源附近各放置 1 颗 `TMP112A`（`0x48/0x49`），并将两颗 `TMP112A.ALERT` 通过开漏线与汇总为 `THERM_KILL_N`，用于“任一路过温 → 双路硬停机”（见 `docs/power-monitoring-design.md` 与 `docs/i2c-address-map.md`）。
- 缺口：固件目前仅做温度读数与 `THERM_KILL_N` 可见性上报（`#0006`），但未冻结/落地 `TMP112A.ALERT` 的阈值与模式配置；若依赖上电默认值，告警语义可能与“项目设计（过温时保持输出）”不一致。
- 说明：`TMP112A` 上电虽默认处于 comparator / active-low 的电平型告警语义，但默认阈值为 `THIGH=80°C` / `TLOW=75°C`，无法满足本项目 `50/40°C` 阈值要求（见 `./contracts/config.md`）。
- 目标：把 `TMP112A.ALERT → THERM_KILL_N → TPS_EN` 这条硬停机链路的“电平型告警语义”冻结为可实现、可测试的契约，并为实现阶段提供明确的阈值/去抖/失败策略决策点。

## 目标 / 非目标

### Goals

- 冻结 `TMP112A.ALERT` 的过温告警语义：**电平型**，在满足过温条件时持续有效，并按项目设计的滞回策略释放（不依赖边沿/中断）。
- 冻结 `THERM_KILL_N` 的系统级语义（低有效、开漏线与、多源驱动）与与 `TPS_EN` 的硬停机关系，确保“任一路过温 → 双路停机”可验证。
- 明确固件在启动阶段对两颗 `TMP112A` 的配置责任：写入模式/阈值/去抖参数，并在失败时按约定进入安全态或降级。

### Non-goals

- 不在本计划内实现温度闭环降额（限流/限功率/限输出电压）或复杂热管理策略。
- 不在本计划内更改硬件链路（`ALERT` 汇总方式、`THERM_KILL_N`/`TPS_EN` 连接关系、器件选型与布局）。
- 不在本计划内新增更多温度采样点或引入其它温度器件。

## 范围（Scope）

### In scope

- `firmware/`：为两颗 `TMP112A(0x48/0x49)` 增加启动配置（模式/阈值/去抖），并在日志中提供最小可观测性（例如配置写入成功/失败与读取回读）。
- `firmware/`：在运行中持续采样温度与读取 `THERM_KILL_N` 电平；当 `THERM_KILL_N` 被拉低时，明确记录“硬停机触发”的可定位信息（但不要求由 MCU 主动关断，硬件链路已会执行关断）。
- 文档：将阈值与语义的最终决策补齐并同步到项目设计文档（见“文档更新”）。

### Out of scope

- 上位机联动（HTTP/RPC/落盘）与远程清故障流程。
- “自动恢复/自动重试”策略的完整状态机（本计划只冻结硬停机链路的告警/释放语义与最小可观测性）。

## 需求（Requirements）

### MUST

- 固件必须在启动阶段对两颗 `TMP112A` 写入配置，使其 `ALERT` 工作在“过温时保持输出”的项目设计模式：
  - Comparator mode（`TM=0`）
  - 低有效（`POL=0`，`ALERT` 拉低表示过温）
  - 使用 `T(HIGH)/T(LOW)` 实现滞回：温度 `≥ T(HIGH)` 时 `ALERT` 拉低，并保持到温度 `< T(LOW)` 才释放
  - 支持去抖（Fault Queue）参数（具体取值见 `./contracts/config.md`）
- 固件不得依赖上电默认值来满足上述语义（必须显式配置两颗器件）。
- 必须冻结两颗器件的阈值（`T(HIGH)` / `T(LOW)`）与去抖参数，并保证两颗器件配置一致或明确差异（见 `./contracts/config.md`）。
- 对 `TMP112A` 配置写入失败（I2C NACK/timeout 等）时，固件必须按冻结的失败策略处置（fail-safe vs degrade；见 `./contracts/config.md`）。
- `THERM_KILL_N` 语义必须与硬件设计一致：开漏线与、低有效；并在固件侧保持“默认不主动拉低”（除非明确开启强制关断模式）。
- 当 `THERM_KILL_N=0` 时，固件必须在日志中给出“可能来源”的提示（`out_a/out_b/both/unknown`），提示算法以读取两颗 `TMP112A` 当前温度并与 `T(LOW)/T(HIGH)` 比较为准（不新增硬件信号；详见 `./contracts/config.md`）。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes） |
| --- | --- | --- | --- | --- | --- | --- | --- |
| TMP112A 过温告警配置（模式/阈值/去抖/失败策略） | Config | internal | Modify | ./contracts/config.md | firmware | firmware, hardware | 覆盖 `0x48/0x49` 两颗器件 |
| `THERM_KILL_N` 硬停机线语义（`ALERT` 汇总 → `TPS_EN`） | HW Signal | internal | Modify | ./contracts/hardware-signals.md | hardware | firmware, hardware | 低有效开漏线与；双路停机 |

### 契约文档（按 Kind 拆分）

- [contracts/README.md](./contracts/README.md)
- [contracts/config.md](./contracts/config.md)
- [contracts/hardware-signals.md](./contracts/hardware-signals.md)

## 验收标准（Acceptance Criteria）

- Given 固件已对 `TMP112A(0x48/0x49)` 完成配置（模式/阈值/去抖）且两路 `TPS55288` 正常工作，
  When 任一路热点温度升至 `T(HIGH)` 或更高，
  Then `TMP112A.ALERT` 拉低并使 `THERM_KILL_N=0`，两路 `TPS55288` 被硬件链路关断，且 `THERM_KILL_N` **保持为低**直到温度降至 `< T(LOW)`（按契约滞回释放）。

- Given 温度在 `T(LOW)` 与 `T(HIGH)` 之间波动，
  When 温度未跨越 `T(HIGH)`，
  Then `THERM_KILL_N` 不应被拉低（满足去抖要求，避免抖动误触发）。

- Given 任一 `TMP112A` 配置写入失败，
  When 固件启动进入工作态，
  Then 固件按契约的失败策略执行（例如进入安全态/强制停机/降级运行并持续告警），且行为可从日志中明确识别。

## 实现前置条件（Definition of Ready / Preconditions）

- 已确认“过温时保持输出”的精确定义：指 Comparator 模式的电平保持 + 滞回释放（而非“中断锁存直到读寄存器/重启”）。
- 已确认阈值与去抖参数：`T(HIGH)=50°C`、`T(LOW)=40°C`、Fault Queue=`4`、Conversion rate=`1 Hz`（见 `./contracts/config.md`）。
- 已确认 `TMP112A` 配置失败时策略为 fail-safe：**不允许使能 TPS 输出，并打印错误信息**（见 `./contracts/config.md`）。
- 已完成最小代码定位：
  - `firmware/src/tmp112.rs`：现有仅读温度寄存器；需扩展为可写配置/阈值寄存器并提供回读
  - `firmware/src/main.rs`：已存在 `THERM_KILL_N` 默认不拉低的约束；需与本计划策略一致
  - `firmware/src/output/tps55288.rs`：现有读取温度与 `therm_kill_n` 上报链路可作为验收观测入口

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: 覆盖阈值寄存器编码/解码（`°C * 16` 与寄存器 12-bit 格式）、配置字节序（大端）等纯逻辑部分。
- Integration tests: 固化“加热触发过温 → 硬停机 → 降温释放”的上板验证步骤到 `firmware/README.md`（仅人类操作步骤与观测点）。

### Quality checks

- 使用仓库既有质量检查（`cargo fmt` / `cargo clippy` / `cargo build` 等），不引入新工具链。

## 文档更新（Docs to Update）

- `docs/power-monitoring-design.md`: 补齐并冻结 `TMP112A.ALERT` 的阈值、去抖与“保持输出”语义引用（指向本计划契约）。
- `docs/i2c-address-map.md`: 补充“TMP112A 除读温度外还会被固件写入告警配置”的说明（避免仅凭地址表推断默认行为）。
- `firmware/README.md`: 增加“过温硬停机链路验证（TMP112A.ALERT / THERM_KILL_N / TPS_EN）”章节。

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones）

- [ ] M1: `TMP112A` 配置与阈值写入/回读 API（两地址 `0x48/0x49`；按 `./contracts/config.md`）
- [ ] M2: 启动阶段应用配置 + fail-safe 落地（配置失败则不使能 TPS 输出；日志可定位）
- [ ] M3: 上板验证步骤与“过温来源提示（日志）”落地到文档（`firmware/README.md`）

## 方案概述（Approach, high-level）

- 以“硬件链路可独立完成停机”为前提：固件主要负责配置 `TMP112A` 告警语义与提供可观测性；控制闭环（降额/恢复）留给后续计划。
- 配置与阈值应做到可追溯：固件在启动阶段记录配置摘要（或回读值），便于 bring-up 与回归对齐。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：
  - 阈值若过保守可能导致误停机；若过激进可能导致热风险；需要结合样机热测试数据回归。
  - 若 `TMP112A` 配置写入失败且仍继续供电，硬件级过温停机能力可能不满足预期（需明确 fail-safe）。
- 开放问题（需要决策）：
  - 见 `./contracts/config.md` 的 `T(HIGH)/T(LOW)` 与失败策略条目。
- 假设（需主人确认）：
  - 本计划的“保持输出”语义以 Comparator 模式 + 滞回释放为准（`TM=0`）。

## 变更记录（Change log）

- 2026-01-27: 初始化计划骨架与契约入口

## 参考（References）

- `docs/power-monitoring-design.md`（4.3 节：TMP112A + `THERM_KILL_N` 链路与 Comparator 模式建议）
- `docs/i2c-address-map.md`
- `docs/hardware-selection/esp32-s3-fh4r2-gpio.md`（GPIO40：`THERM_KILL_N`）
- `docs/plan/0006:tps-tmp112-temperature-reading/PLAN.md`（温度读数与 `THERM_KILL_N` 可见性）
