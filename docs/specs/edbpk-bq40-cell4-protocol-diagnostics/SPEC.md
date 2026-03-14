# BQ40 Cell4 protocol-safe diagnostics（#edbpk）

## 状态

- Status: 部分完成（1/2）
- Created: 2026-03-14
- Last: 2026-03-14

## 背景 / 问题陈述

- 当前板上 `BQ40Z50-R2` 持续上报 `CellVoltage4() = 0`，但实测电芯堆栈为 4 串，且 `VC4-VC3` 约为单节电压。
- 既有 `tools/bq40-comm-tool` 固件在 BQ 诊断链路中同时存在两类风险：
  - `0x00 -> 0x23` 的 ManufacturerAccess / ManufacturerData 协议实现不够严谨，可能污染固定信息读取结论。
  - 诊断代码会主动切换 `GAUGING` / `CAL` 状态，导致采样结果混入工具自身写操作副作用。
- 在定位 `Cell4=0` 根因前，必须先把工具链收敛到“协议正确、采样只读、flash/monitor 不互抢”的状态。

## 目标 / 非目标

### Goals

- 修正 `tools/bq40-comm-tool` 中 `BQ40Z50` 的 `0x00 -> 0x23` 通信实现，至少稳定读出 `DeviceType` 与 `FirmwareVersion`。
- 禁止诊断流程在常规采样期间主动切换 `GAUGING` / `CAL`，确保 `DA Configuration`、`AFE Register`、`DAStatus1` 为只读观测结果。
- 为 `flash` / `monitor` / `run` 增加互斥，避免同一时刻抢占同一 MCU 会话。
- 通过一次干净 live diagnose 确认：当前实际 `DA Configuration` 中电池串数配置值。

### Non-goals

- 本规格不直接改写 BQ40 Data Flash，也不执行 ROM reflash 修复。
- 本规格不对最终硬件根因下结论到“芯片损坏”或“板级开路”。
- 本规格不引入新的 GUI 或 Web 诊断入口。

## 范围（Scope）

### In scope

- `tools/bq40-comm-tool/firmware/src/output/mod.rs`
- `tools/bq40-comm-tool/firmware/src/bq40z50.rs`
- `tools/bq40-comm-tool/bin/run.sh`
- `tools/bq40-comm-tool/bin/flash.sh`
- `tools/bq40-comm-tool/bin/monitor.sh`
- `tools/bq40-comm-tool/bin/common.sh`

### Out of scope

- 主工程根目录 `firmware/`
- BQ40 实际 DF 修复写入
- 电池包硬件返修

## 需求（Requirements）

### MUST

- `0x00 -> 0x23` 路径必须按实机验证后的正确方式读写，不能继续混用错误字节序或错误 payload 解析。
- 常规 `diagnose` 不得主动发送会改变 BQ 运行状态的 `GAUGING` / `CAL` MAC 命令。
- `flash` 与 `monitor` 不得并发执行；`run.sh` 必须保证 fresh flash 后 monitor 从干净 reset 边界附着。
- live diagnose 必须输出并记录：
  - `DA Configuration`
  - `AFE Register`
  - `DAStatus1`
  - `OperationStatus`
  - `SafetyAlert/SafetyStatus/PFStatus`

### SHOULD

- 固件应显式记录 `DF` 中与当前问题最相关的配置字段，方便把“串数配置问题”与“其他 DF 异常”分开。
- 临时运行备份目录不应再误入 git 工作区。

## 验收标准（Acceptance Criteria）

- Given 修复后的工具固件，
  When 运行一次 `diagnose --mode canonical` live 流程，
  Then monitor 中不再出现 `bms_gauge_toggle`、`bms_cal`、`bms_da1_after_gauge` 这类主动写状态日志。

- Given 修复后的 `0x00 -> 0x23` 协议实现，
  When 读取固定芯片信息，
  Then 至少能稳定得到：
  - `DeviceType = 0x4500`
  - `FirmwareVersion raw = 45 00 01 06 00 24 00 03 85 02 00`

- Given 当前问题板卡，
  When 读取 `DA Configuration`,
  Then 应能稳定确认当前串数配置，而不是依赖推测。

## 当前阶段结论

- 已确认工具主机侧存在真实协议 bug，且已修复。
- 已确认当前常规诊断链路可以在不主动切换 `GAUGING/CAL` 的前提下读取稳定数据。
- 已确认当前 `DA Configuration = 0x8103`，即 `4 cells`。
- 已确认在上述前提下，`CellVoltage4()` 仍然为 `0`。

## 里程碑（Milestones）

- [x] M1: 修正 `0x00 -> 0x23` 读取实现并完成实机验证。
- [x] M2: 去除常规诊断对 `GAUGING/CAL` 的主动扰动。
- [x] M3: 为 `flash/monitor` 增加互斥并完成干净 monitor 验证。
- [ ] M4: 在所有关键读路径上补齐 reply PEC 校验，并重新验证 `DA Configuration` / `DAStatus1`。

## 质量门槛（Quality Gates）

- `bash -n tools/bq40-comm-tool/bin/flash.sh tools/bq40-comm-tool/bin/monitor.sh tools/bq40-comm-tool/bin/run.sh tools/bq40-comm-tool/bin/common.sh`
- `cargo check -q`（目录：`tools/bq40-comm-tool/firmware`）
- 至少一轮 live diagnose 报告，且 `summary.json` 为有效样本通过

## 参考（References）

- `tools/bq40-comm-tool/reports/20260314_md23_proto_fix/summary.json`
- `tools/bq40-comm-tool/reports/20260314_cleaner_cell4_check2/summary.json`
- `docs/manuals/BQ40Z50-R2-TRM/BQ40Z50-R2-TRM.md`
