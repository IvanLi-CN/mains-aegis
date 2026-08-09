# TPS55288 双路输出控制

> 当前有效规范以本文为准；实现覆盖与当前状态见 `./IMPLEMENTATION.md`，关键演进原因见 `./HISTORY.md`。

## 背景 / 问题陈述

- 主板包含两路可编程升降压输出：`U17/U18(TPS55288RPMR)`，通过 `I2C1`（`GPIO48/47`）进行寄存器配置（地址：`0x74/0x75`，见 `docs/i2c-address-map.md`）。
- 主板包含电源监测：`U22(INA3221)`（I2C 地址 `0x40`），其中 `CH2/CH1` 分别采样 `TPS55288 OUT-A/OUT-B` 的电压/电流（见 `docs/power-monitoring-design.md`）。
- 当前 `firmware/` 已落地两颗 `TPS55288` 的启动配置、运行态门控与遥测。
- 本规范冻结正常主固件双路输出 profile，以及诊断/测试 profile 的显式例外边界。

## 目标 / 非目标

### Goals

- 固件能够通过 `I2C1` 识别并配置两颗 `TPS55288`（`0x74` / `0x75`），并在启动后按“默认配置（Default profile）”设置输出参数。
- 正常主固件默认同时启用 OUT-A 与 OUT-B，目标输出 `12V`，目标电流限制 `3.5A`；单路仅允许显式诊断/测试 profile。
- 固件初始化 `INA3221` 并每 `500ms` 打印 OUT-A/OUT-B 两路的设置电压、实际电压与电流（输出格式见 `./contracts/cli.md`）。
- 当 I2C 通信失败或检测到 fault/告警时，固件能在日志中给出可定位的错误口径，并保持系统可继续运行（不 panic）。

### Non-goals

- UPS OUT 的运行态门控与双路启动契约由 `../runtime-mode-switching/SPEC.md` 定义；本文冻结 TPS 器件可控能力与主固件默认 profile。
- 不在本规格内设计/修改硬件（跳线、并联、反馈网络等）与其验证闭环（示波器波形、EMI 等）。
- 不在本规格内引入复杂的交互控制面（例如屏幕菜单、持久化配置、完整命令行控制台）。

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
  - 总线：`I2C1`（`GPIO48=I2C1_SDA`，`GPIO47=I2C1_SCL`；目标速率 `25kHz`；见 `docs/i2c-address-map.md`）
- 固件在启动后必须应用默认 profile：
  - 正常主固件默认启用 `out_a+out_b`
  - 正常主固件默认输出电压目标：`12V`
  - 正常主固件默认电流限制目标：`3.5A`
  - 显式诊断/测试 profile 可独立选择单路与临时电气参数
- 任一 `TPS55288` I2C 通信失败（NACK/timeout/CRC 等）时，固件不得 panic；必须输出可定位日志（包含：地址、步骤、错误类别），并进入“保守策略”（不得继续对该器件反复写寄存器刷屏；允许周期性重试但需限频）。
- 启动自检只有一路 TPS 无通信错误且未报告 `SCP/OCP/OVP` 时，固件必须保留双路 `requested_outputs` 合同，并将缺失通道纳入有限退避重试。
- 任一路要求 TPS 的通信/配置重试耗尽后，固件必须锁存 `tps_config_failed`，停止双路输出，保留失败通道错误与 `mode=blocked`，并等待显式 restore。
- 当且仅当 `i2c_nack`、`i2c_timeout`、`i2c_arbitration` 或通用 `i2c` 错误耗尽该重试预算时，固件必须先完成双路软件停机和尽力 `disable_output()`，再由 GPIO40 以开漏方式拉低 `THERM_KILL_N`，通过板级链路抑制共享 `TPS_EN`。`invalid_config`、读回异常和 `SCP/OCP/OVP` 继续使用既有软件保护停机，不得触发这一 MCU 硬抑制。
- MCU 对 `THERM_KILL_N` 的拉低只在当前运行期保持。受限 release 仅释放 MCU 自己的开漏拉低，不清除 `tps_config_failed`、不探测 TPS、也不恢复输出；后续仍须先诊断并走既有显式 restore。
- `mcu.runtime` 诊断必须提供 `tps_enable_interlock`：物理 `THERM_KILL_N` 电平、MCU 驱动意图、推导的 `TPS_EN` 抑制状态、来源、触发/最近释放时间和最近 I2C 失败通道/阶段/错误。`TPS_EN` 没有独立可读 GPIO，禁止把推导状态表述为直接引脚读数。
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
| TPS55288 默认 profile 与通道/地址映射 | Config | internal | New | ./contracts/config.md | firmware | firmware | 冻结正常主固件双路默认 profile |
| 遥测日志输出（串口/日志） | CLI | internal | New | ./contracts/cli.md | firmware | developers | 每 `500ms` 输出两路 `vset/vbus/current` |

### 契约文档（按 Kind 拆分）

- [contracts/README.md](./contracts/README.md)
- [contracts/config.md](./contracts/config.md)
- [contracts/cli.md](./contracts/cli.md)

## 验收标准（Acceptance Criteria）

- Given 主板已供电且 `I2C1` 可用，
  When 固件启动运行并完成初始化，
  Then 日志中能看到对 `0x74/0x75` 的配置结果，且两路被设置为正常主固件默认目标，并且每 `500ms` 打印一次 OUT-A/OUT-B 的遥测日志（见 `./contracts/cli.md`）。

- Given 两颗 `TPS55288` 仅有一颗可响应（另一颗缺件/焊接异常/总线故障），
  When 固件启动并尝试配置两颗器件，
  Then 固件不 panic；日志中能明确指出失败器件地址与错误类型；缺失通道只在有限退避重试预算内重试，耗尽后锁存 `tps_config_failed` 并停止双路输出；双路请求合同与 `mode=blocked` 保持不变。

- Given 任一已请求 TPS 的可重试 I2C 错误耗尽预算，
  When 固件进入保护停机，
  Then 两路软件输出先停止并尽力发送两路 disable，再由 MCU 拉低 `THERM_KILL_N` 抑制 `TPS_EN`；非重试配置错误和 TPS `SCP/OCP/OVP` 不得拉低 GPIO40。

- Given MCU 已因 TPS I2C 耗尽持有 `THERM_KILL_N` 低电平，
  When 操作者发出已确认的 release，
  Then 仅 MCU 开漏被释放，故障锁存和双路停止状态保持；若线路仍为低，诊断必须报告 `external_or_unknown`，且输出继续受保护。

- Given `INA3221` 可响应，
  When 固件按固定配置初始化并读取 OUT-A/OUT-B 两路电压/电流，
  Then 遥测日志中 `vset_mv/vbus_mv/current_ma` 字段可读、单位一致；若 I2C 单次失败，按 `./contracts/cli.md` 输出 `err(...)` 占位且不 panic/不刷屏重试。

- Given `I2C1_INT(GPIO33)` 出现 fault/告警（电平或边沿），
  When 固件收到该信号并读取/解析故障状态（若该路径可用），
  Then 日志中能看到“fault 发生 + 哪颗 TPS + 关键状态字段（或至少 raw 状态值）”，且固件不 panic。

## 实现前置条件（Definition of Ready / Preconditions）

- 已冻结显式诊断/测试 profile 的单通道关闭策略：在 `TPS_EN` 共用的前提下，通过 I2C/寄存器独立 enable/disable（见 `./contracts/config.md`）。
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
- `docs/i2c-address-map.md`: 若本规格最终冻结 I2C1 速率、故障线处理口径，补充对应说明（不改动地址表本身）。
- `docs/ups-output-design.md`: 保持正常主固件 `12V/19V` 输出策略与双路要求一致。

## 方案概述（Approach, high-level）

- 以 `docs/i2c-address-map.md` 为事实来源固定 I2C 地址与引脚；以 `./contracts/config.md` 冻结默认 profile 与通道命名映射。
- 默认策略优先保证“可观测 + 不 panic + 保守失败处理”，避免在 I2C 故障场景下死循环重试或刷屏日志。
- `INA3221` 采用最小寄存器读写实现（不新增外部依赖），配置与换算口径以 `./contracts/config.md` 为准。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：
  - 两颗 `TPS55288` 的硬件使能网共用（`TPS_EN`）；诊断/测试 profile 的单通道控制依赖 I2C/寄存器独立 enable/disable。正常主固件不得缩窄双路请求合同或把部分恢复发布为整体成功；任一路通信/配置失败锁存均须停止双路输出。
- 假设（需主人确认）：
  - None

## 参考（References）

- `docs/i2c-address-map.md`
- `docs/ups-output-design.md`
- `docs/power-monitoring-design.md`
- `docs/pcbs/mainboard/README.md`

## Visual Evidence

PR: none
