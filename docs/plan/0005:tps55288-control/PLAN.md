# TPS55288 双路输出控制（#0005）

## 状态

- Status: 已完成
- Created: 2026-01-23
- Last: 2026-01-25

## 背景 / 问题陈述

- 主板包含两路可编程升降压输出：`U17/U18(TPS55288RPMR)`，通过 `I2C1`（`GPIO48/47`）进行寄存器配置（地址：`0x74/0x75`，见 `docs/i2c-address-map.md`）。
- 主板包含电源监测：`U22(INA3221)`（I2C 地址 `0x40`），其中 `CH2/CH1` 分别采样 `TPS55288 OUT-A/OUT-B` 的电压/电流（见 `docs/power-monitoring-design.md`）。
- 当前 `firmware/` 仅提供最小 bring-up（heartbeat），尚未落地对 `TPS55288` 的控制逻辑。
- 需要在固件侧实现对两颗 `TPS55288` 的最小可控能力，并冻结“默认启用哪一路、默认输出电压/电流限制”的口径，便于后续联调与回归验证。

## 目标 / 非目标

### Goals

- 固件能够通过 `I2C1` 识别并配置两颗 `TPS55288`（`0x74` / `0x75`），并在启动后按“默认配置（Default profile）”设置输出参数。
- 默认仅启用一路输出（见 `./contracts/config.md` 的 `default_enabled_channel`），目标输出 `5V`，目标电流限制 `1A`（临时测试）。
- 固件初始化 `INA3221` 并每 `500ms` 打印 OUT-A/OUT-B 两路的设置电压、实际电压与电流（输出格式见 `./contracts/cli.md`）。
- 当 I2C 通信失败或检测到 fault/告警时，固件能在日志中给出可定位的错误口径，并保持系统可继续运行（不 panic）。

### Non-goals

- 不在本计划内冻结 UPS OUT 的最终系统策略（例如 `12V/19V` 两固件版本、并联策略、限流分工策略等）；本计划仅定义“可控能力 + 默认 profile”。
- 不在本计划内设计/修改硬件（跳线、并联、反馈网络等）与其验证闭环（示波器波形、EMI 等）。
- 不在本计划内引入复杂的交互控制面（例如屏幕菜单、持久化配置、完整命令行控制台）。

## 范围（Scope）

### In scope

- `firmware/`：新增 `TPS55288` 的 I2C 访问与最小驱动封装，并在启动流程中应用默认 profile。
- `firmware/`：初始化 `INA3221` 并按 `500ms` 周期输出遥测日志（见 `./contracts/cli.md`）。
- `firmware/`：对 `I2C1_INT(GPIO33)` 的 fault/告警做最小处理（至少：可观测日志 + 不中断系统运行）。
- 文档：冻结并记录默认 profile、通道命名映射与地址映射（见 `./contracts/config.md`）。

### Out of scope

- 对外暴露可交互的“实时调参接口”（串口命令、HTTP、面板 UI 等）。
- 自动端口枚举/自动烧录/任何设备写入类动作（遵循仓库既有设备操作纪律）。

## 需求（Requirements）

### MUST

- 固件必须支持同时访问两颗 `TPS55288`：
  - `TPS55288 OUT-A`：I2C 地址 `0x74`
  - `TPS55288 OUT-B`：I2C 地址 `0x75`
  - 总线：`I2C1`（`GPIO48=I2C1_SDA`，`GPIO47=I2C1_SCL`；目标速率 `400kHz`；见 `docs/i2c-address-map.md`）
- 固件在启动后必须应用默认 profile：
  - 默认启用一路输出（由 `default_enabled_channel` 决定）
  - 默认输出电压目标：`5V`
  - 默认电流限制目标：`1A`
- 非默认输出路在默认 profile 下必须处于“不会主动驱动负载”的状态（具体实现形态见 `./contracts/config.md`；允许因器件/硬件拓扑导致的被动电压存在，但不得主动稳压输出）。
- 任一 `TPS55288` I2C 通信失败（NACK/timeout/CRC 等）时，固件不得 panic；必须输出可定位日志（包含：地址、步骤、错误类别），并进入“保守策略”（不得继续对该器件反复写寄存器刷屏；允许周期性重试但需限频）。
- 固件侧 `TPS55288` 驱动必须明确使用 `tps55288` 这个 crate（crates.io，`0.2.0`）。
- 固件必须初始化 `INA3221 (0x40)`，并按 `./contracts/config.md` 的映射仅启用 OUT-A/OUT-B 的采样通道（CH2/CH1）。
- 固件必须每 `500ms` 打印一次遥测（telemetry）日志，且每次打印必须包含 OUT-A 与 OUT-B 两路：
  - `vset_mv`：从 `TPS55288` 读取的设置电压（mV）
  - `vbus_mv`：从 `INA3221` 读取的实际电压（mV）
  - `current_ma`：从 `INA3221` 读取的实际电流（mA）

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes） |
| --- | --- | --- | --- | --- | --- | --- | --- |
| TPS55288 默认 profile 与通道/地址映射 | Config | internal | New | ./contracts/config.md | firmware | firmware | 冻结默认启用通道与 `5V/1A` 目标 |
| 遥测日志输出（串口/日志） | CLI | internal | New | ./contracts/cli.md | firmware | developers | 每 `500ms` 输出两路 `vset/vbus/current` |

### 契约文档（按 Kind 拆分）

- [contracts/README.md](./contracts/README.md)
- [contracts/config.md](./contracts/config.md)
- [contracts/cli.md](./contracts/cli.md)

## 验收标准（Acceptance Criteria）

- Given 主板已供电且 `I2C1` 可用，
  When 固件启动运行并完成初始化，
  Then 日志中能看到对 `0x74/0x75` 的配置结果，且默认启用的输出路被设置为 `5V/1A` 目标（临时测试），并且每 `500ms` 打印一次 OUT-A/OUT-B 的遥测日志（见 `./contracts/cli.md`）。

- Given 两颗 `TPS55288` 仅有一颗可响应（另一颗缺件/焊接异常/总线故障），
  When 固件启动并尝试配置两颗器件，
  Then 固件不 panic；日志中能明确指出失败器件地址与错误类型；可响应的那颗仍按默认 profile 完成配置（或按保守策略选择整体停用，取决于 `./contracts/config.md` 的策略约定）。

- Given `INA3221` 可响应，
  When 固件按固定配置初始化并读取 OUT-A/OUT-B 两路电压/电流，
  Then 遥测日志中 `vset_mv/vbus_mv/current_ma` 字段可读、单位一致；若 I2C 单次失败，按 `./contracts/cli.md` 输出 `err(...)` 占位且不 panic/不刷屏重试。

- Given `I2C1_INT(GPIO33)` 出现 fault/告警（电平或边沿），
  When 固件收到该信号并读取/解析故障状态（若该路径可用），
  Then 日志中能看到“fault 发生 + 哪颗 TPS + 关键状态字段（或至少 raw 状态值）”，且固件不 panic。

## 实现前置条件（Definition of Ready / Preconditions）

- 已冻结“非默认通道关闭策略”：在 `TPS_EN` 共用的前提下，通过 I2C/寄存器实现每颗芯片独立 enable/disable（见 `./contracts/config.md`）。
- 已确认固件 toolchain 支持依赖 crate 的 edition 要求（`tps55288@0.2.0` 为 Rust 2024 edition；当前 `esp` toolchain 为 `rustc 1.89.0-nightly`，满足）。
- 已冻结 `INA3221` 初始化配置与遥测输出格式（见 `./contracts/config.md` 与 `./contracts/cli.md`）。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: 若实现中包含寄存器编码/单位换算（mV/mA → reg value），需提供最小单元测试覆盖（边界值、非法输入、舍入规则）。
- Integration tests: 至少一次上板手工验证步骤固化到 `firmware/README.md`（仅描述人类操作：构建/烧录/测量点与预期值；不要求 Agent 执行写入类动作）。

### Quality checks

- 使用仓库既有质量检查（如 `cargo fmt` / `cargo clippy` / `cargo build`），不引入新工具链。

## 文档更新（Docs to Update）

- `firmware/README.md`: 增加 “TPS55288 控制 bring-up 验证” 章节（测量点、预期日志、故障排查）。
- `firmware/README.md`: 增加 “INA3221 遥测验证” 章节（`500ms` 日志口径、通道映射、单位与换算）。
- `docs/i2c-address-map.md`: 若本计划最终冻结 I2C1 速率、故障线处理口径，补充对应说明（不改动地址表本身）。
- `docs/ups-output-design.md`: 若后续把 `5V/1A` 作为默认 bring-up 档位长期保留，应在策略章节补一条说明与迁移到 `12V/19V` 的路径（不属于本计划必交付）。

## 实现里程碑（Milestones）

- [x] M1: 落地 `TPS55288` 最小驱动封装（I2C 读写 + 关键寄存器配置）并在启动时应用默认 profile
- [x] M2: 初始化 `INA3221` 并输出 `500ms` 周期遥测日志（OUT-A/OUT-B：`vset/vbus/current`）
- [x] M3: 落地 fault/告警的最小观测与日志口径（`I2C1_INT(GPIO33)` + 状态读取/解析）
- [x] M4: 固化上板验证步骤与测量口径到 `firmware/README.md`

## 方案概述（Approach, high-level）

- 以 `docs/i2c-address-map.md` 为事实来源固定 I2C 地址与引脚；以 `./contracts/config.md` 冻结默认 profile 与通道命名映射。
- 默认策略优先保证“可观测 + 不 panic + 保守失败处理”，避免在 I2C 故障场景下死循环重试或刷屏日志。
- `INA3221` 采用最小寄存器读写实现（不新增外部依赖），配置与换算口径以 `./contracts/config.md` 为准。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：
  - 两颗 `TPS55288` 的硬件使能网共用（`TPS_EN`），因此“默认仅启用一路输出”完全依赖 I2C/寄存器的独立 enable/disable 能力；实现前需用 `tps55288` crate API 明确该路径可用。
  - `5V/1A` profile 与现有 UPS OUT 设计文档（`12V/19V`）存在阶段性冲突，需明确其定位（bring-up vs 长期策略），否则容易在实现阶段跑偏。
- 假设（需主人确认）：
  - None

## 变更记录（Change log）

- 2026-01-22: 初始化计划与契约骨架
- 2026-01-23: 纳入 INA3221 初始化与 `500ms` 遥测日志契约
- 2026-01-24: 实现默认 profile（`out_a=5V/1A`）、遥测输出与 `I2C1_INT` 最小故障观测；补齐 bring-up README
- 2026-01-25: bring-up 增强：解除 CDC fault mask、补全 fault 寄存器打印（含 `VOUT_SR/MODE` 关键位）；调大 `OCP_DELAY` 并放慢 `VOUT` 斜率用于排查启动瞬态误触发；修正 telemetry 字段顺序以符合 CLI 契约
- 2026-01-25: 调试配置：默认启用通道切换为 `out_b`

## 参考（References）

- `docs/i2c-address-map.md`
- `docs/ups-output-design.md`
- `docs/power-monitoring-design.md`
- `docs/pcbs/mainboard/README.md`
