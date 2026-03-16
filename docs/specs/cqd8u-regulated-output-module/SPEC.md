# 稳压输出模块文档体系与启停状态机（#cqd8u）

## 状态

- Status: 已完成
- Created: 2026-03-16
- Last: 2026-03-16

## 背景 / 问题陈述

- 当前稳压输出相关事实散落在 `firmware/README.md`、`docs/boot-self-test-flow.md`、`docs/power-monitoring-design.md` 与旧 `docs/plan/**` 中，默认 profile 与恢复语义已出现和当前代码不一致的描述。
- 运行态输出启停规则分散在 boot self-test、BMS 恢复与 fault 观测路径里，缺少统一的模块级状态机语义。
- 需要先把 `TPS55288 + TMP112 + INA3221(CH1/CH2)` 收敛为单一模块文档，再在固件里补齐统一 enable/disable/restore 状态机，为后续 UI/串口控制入口留出稳定接口。

## 目标 / 非目标

### Goals

- 在 `docs/` 下新增 `docs/modules/`，用一份索引管理“一个模块一个文件”的功能文档。
- 新增 `docs/modules/regulated-output.md`，冻结稳压输出模块的边界、硬件映射、默认 profile、遥测字段与启停状态机语义。
- 把当前代码真相定为文档 SoT：`I2C1=25kHz`、默认输出集合=`out_a`、默认 `19V/3.5A`。
- 在 `firmware/src/output/` 引入统一输出状态机，显式区分 `requested / active / recoverable / blocked`。
- 提供 `PowerManager::request_output_restore()` 与 `PowerManager::output_restore_pending()`，但本轮不接前面板/串口入口。
- 运行态门控解除后，仅当 `VIN` 在线时进入“可恢复未恢复”状态；本轮不自动重新使能输出。

### Non-goals

- 不实现前面板按钮、串口命令或其它外部 restore 控制入口。
- 不修改硬件链路、地址分配或 `TPS55288` bring-up 基础寄存器配置。
- 不扩展其它模块正文文档。

## 范围（Scope）

### In scope

- `docs/modules/README.md`
- `docs/modules/regulated-output.md`
- `docs/README.md`
- `docs/boot-self-test-flow.md`
- `firmware/README.md`
- `docs/specs/README.md`
- `firmware/src/output/mod.rs`
- `firmware/src/main.rs`

### Out of scope

- UI 渲染模型修改。
- 新增 persistent 配置。
- GitHub 远端协作动作。

## 接口变更（Interfaces）

- `output::PowerManager`
  - 新增 `request_output_restore()`：显式请求恢复 `recoverable_outputs`。
  - 新增 `output_restore_pending()`：告知当前是否处于“可恢复未恢复”状态。
- `output` 内部新增：
  - `OutputGateReason`
  - `OutputRuntimeState`
- `output::Config` / `BootSelfTestResult`：使用 `requested_outputs / active_outputs / recoverable_outputs / output_gate_reason` 新语义，替代旧的 BMS-only restore 字段。

## 验收标准（Acceptance Criteria）

- `docs/modules/README.md` 存在，并说明目录规则与当前模块清单。
- `docs/modules/regulated-output.md` 覆盖：模块边界、器件与通道映射、默认 profile、`THERM_KILL_N -> TPS_EN`、自检/运行态启停状态机、遥测字段、恢复 API 预留。
- `docs/README.md` 与 `firmware/README.md` 链接到新模块文档，并移除与当前代码冲突的默认值。
- 固件运行态命中 `THERM_KILL_N`、`TPS fault` 或 `BMS not ready` 时，统一进入输出门控状态，不再自动恢复输出。
- 门控解除后，若 `VIN` 离线则保持 blocked；若 `VIN` 在线则仅进入 recoverable，不自动重开。
- `request_output_restore()` 仅在 `VIN` 在线、无活动门控且存在 `recoverable_outputs` 时生效。
- 纯逻辑测试至少覆盖：
  - `BMS block -> no VIN -> still blocked`
  - `gate cleared + VIN online -> recoverable not enabled`
  - `therm_kill/tps_fault never auto-restore`
  - `restore pending only when VIN online`

## 里程碑（Milestones）

- [x] M1: 新建 spec 与 `docs/modules/` 索引。
- [x] M2: 整理稳压输出模块 SoT 文档，并同步 README / boot flow 口径。
- [x] M3: 落地统一输出状态机与恢复 API。
- [x] M4: 补测试与本地验证，达到 local PR-ready。
