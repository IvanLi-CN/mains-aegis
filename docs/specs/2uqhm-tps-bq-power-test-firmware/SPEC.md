# 独立 TPS/BQ 电源测试固件（固定配置屏显版）(#2uqhm)

## 状态

- Status: 已完成
- Created: 2026-03-21
- Last: 2026-03-21

## 背景 / 问题陈述

- 现有主固件把 `TPS55288`、`BQ25792`、`BQ40Z50`、前面板自检与运行态保护深度耦合，不适合做“只看电源链路”的快速上板测试。
- 当前需要一套可反复烧录、绕开 `BMS/BQ40` 授权、并能直观显示两路输出与充电状态的专用测试固件，用于定位 `TPS55288` 输入侧高边 MOS 损坏与启动瞬态问题。
- 现有 `test-fw` 已承担屏幕静态/音频测试职责，不应继续往里塞电源控制逻辑，避免职责混杂。

## 目标 / 非目标

### Goals

- 提供独立二进制 `tps-test-fw`，与现有 `test-fw` 及主固件职责隔离。
- 用“顶部常量 + 重新编译”方式固定测试 profile，不做运行时触摸/按键改配置。
- 直接控制 `BQ25792` 与两路 `TPS55288`，支持：
  - charger enable/disable
  - `OutA/OutB` 独立 OE
  - 两路共享输出档位 `5V / 12V / 19V`
- 屏幕常驻显示 charger 状态、两路输出状态、`INA3221` 电压/电流与 `TMP112` 温度。
- 保留最基本的硬件保护与锁存语义：`THERM_KILL_N`、charger 输入/热故障/通信失败、TPS `SCP/OCP/OVP` 与通信失败。

### Non-goals

- 不实现运行时交互式配置、串口命令控制或触摸菜单。
- 不接入 `BQ40Z50` 自检、激活、授权恢复与音频提示逻辑。
- 不把主固件 `PowerManager` 抽象成通用框架。
- 不改变现有 `test-fw` 的导航/音频测试行为。

## 范围（Scope）

### In scope

- `firmware/Cargo.toml`
  - 新增独立 `tps-test-fw` feature/bin，不影响现有 `test-fw` feature 组合。
- `firmware/src/bin/tps-test-fw.rs`
  - 新增独立测试固件入口。
- `firmware/src/tps_test_runtime.rs`
  - 新增轻量电源测试运行时与固定 profile 常量。
- `firmware/src/front_panel.rs`
  - 新增 `render_tps_test_status(...)` 专用渲染入口。
- `firmware/src/front_panel_scene.rs`
  - 新增 `TPS TEST` 单页状态 UI 与 snapshot 数据模型。
- `firmware/src/tps55288_test.rs`
  - 新增测试固件专用的 TPS helper，提供配置、关断与只读 telemetry 访问，避免依赖主固件运行时与日志副作用。
- `firmware/README.md`
  - 增补构建、刷机、固定 profile 改值与安全警示说明。

### Out of scope

- 修改主固件的启动路径、默认输出策略与运行态页面流转。
- 修改现有 `test-fw` 的功能集合、默认入口或交互方式。
- 任何与 `BMS/BQ40Z50` 激活、音频、风扇闭环有关的新增行为。

## 需求（Requirements）

### MUST

- 顶部常量固定支持以下配置项：
  - `TEST_CHARGER_ENABLE`
  - `TEST_CHARGE_VREG_MV`
  - `TEST_CHARGE_ICHG_MA`
  - `TEST_INPUT_LIMIT_MA`
  - `TEST_OUT_A_OE`
  - `TEST_OUT_B_OE`
  - `TEST_VOUT_PROFILE={5V,12V,19V}`
  - `TEST_ILIMIT_MA`
- 测试固件必须在 `BMS/BQ40` 缺失时仍能独立运行。
- 屏幕必须稳定点亮，并持续刷新 charger / OUT-A / OUT-B 三块 live status。
- charger 只在“配置允许 + 通信正常 + 输入存在 + 非热故障”时才真正使能。
- TPS 发生 `SCP/OCP/OVP` 或 `THERM_KILL_N` 断言时，相关输出必须强制关闭并锁存，不自动重启。

### SHOULD

- 复用主固件现有的硬件 bring-up 路径：`I2C1 bus clear`、外部同步、`TMP112` 初始化、`INA3221` 初始化、前面板基础设施。
- UI 保持现有 industrial 风格，但内容针对电源测试重新排版。
- 轮询周期与日志节奏适合上板排障，不产生过量串口噪音。

### COULD

- 在屏幕页头附带 build/profile 摘要，便于拍照对照。
- 在页脚聚合展示告警标签，减少查找成本。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 上电后，固件完成最小板级 bring-up：
  - 清理 `I2C1`
  - 配置 `TPS55288` 外部同步
  - 释放 `THERM_KILL_N`
  - 初始化 `BQ25792 CE/ILIM_HIZ`
  - 配置 `TMP112`
  - 初始化 `INA3221`
  - 初始化前面板显示
- Bring-up 完成后，按固定 profile 直接下发：
  - charger enable/disable 与固定 `VREG/ICHG/IINDPM`
  - `OutA/OutB` 独立 OE
  - 两路共享 `5V/12V/19V` 输出档位与统一 `ILIM`
- 进入 steady-state 后，周期性轮询：
  - `BQ25792` 状态寄存器与 `VBUS/IBUS/VBAT`
  - `INA3221 CH1/CH2` 输出电压、电流
  - `TMP112 A/B` 温度
  - 两路 `TPS55288` 状态、OE、故障位
- 屏幕显示：
  - 顶部：固件名、build id、共享 profile
  - Charger 卡片：请求态、实际态、输入存在、`VBUS/IBUS/VBAT`、`ICHG`、fault
  - OUT-A / OUT-B 卡片：配置 OE、实际 OE、目标电压档、实测 `V/I/T`、TPS 通信状态、fault bits
  - 页脚：总线/保护告警

### Edge cases / errors

- 若 `THERM_KILL_N` 为低，两个输出都保持关闭，界面明确显示 `THERM KILL`。
- 若某一路 TPS 通信失败，仅该路锁存关闭；另一条路可继续按配置运行。
- 若 charger 通信失败、输入缺失、`TS_COLD/TS_HOT` 触发，则 charger 必须被强制关闭并显示原因。
- 若 `INA/TMP` 单路不可读，界面该字段显示 `ERR/NA`，但整机不 panic。
- `TPS SCP/OCP/OVP`、`THERM_KILL_N` 这类硬故障不做自动恢复；纯通信/初始化失败仅做定时 best-effort 重试，恢复依靠重新上电或重新刷机。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes） |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `tps-test-fw` | internal | internal | New | None | firmware | 人工测试/上板排障 | 独立二进制入口 |
| `TestVoutProfile` | internal | internal | New | None | firmware | `tps-test-fw` | 共享输出档位 |
| `TpsTestUiSnapshot` | internal | internal | New | None | firmware | `front_panel` / `tps-test-fw` | 专用屏显模型 |
| `FrontPanel::render_tps_test_status` | internal | internal | New | None | firmware | `tps-test-fw` | 不接入旧路由 |
| `read_telemetry_snapshot` | internal | internal | New | None | firmware | `tps-test-fw` | TPS 只读遥测 helper |

### 契约文档（按 Kind 拆分）

None

## 验收标准（Acceptance Criteria）

- Given 当前默认 profile
  When 构建并刷入 `tps-test-fw`
  Then 屏幕点亮并显示 `charger=off`、`OutA=off`、`OutB=off`、目标档位 `5V`

- Given 修改顶部常量为不同组合
  When 重新编译刷机
  Then 不改代码路径即可切换 charger enable、双路 OE 与 `5V/12V/19V` 档位，并在屏幕上同步显示配置态与实测态

- Given `BMS/BQ40` 缺失或未 ready
  When 运行 `tps-test-fw`
  Then 测试固件仍可独立驱动 `BQ25792/TPS55288`，不被主固件授权流阻断

- Given `THERM_KILL_N` 断言、TPS fault latched、或 charger 输入/热故障/通信失败
  When 轮询到故障
  Then 相关功能被强制关闭并明确显示原因，不自动恢复

- Given 仓库构建验证
  When 运行以下命令
  Then 均通过且现有目标不回归：
  - `cargo +esp check --release`
  - `cargo +esp check --release --bin test-fw --features test-fw-screen-static,test-fw-default-screen-static`
  - `cargo +esp check --release --bin tps-test-fw --features tps-test-fw`

## 实现前置条件（Definition of Ready / Preconditions）

- 范围、默认 profile 与安全策略已冻结
- 本规格不依赖新增外部接口契约文档
- 测试固件采用“顶部常量 + 重新编译”方式这一点已确认

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: None
- Integration tests: None
- E2E tests (if applicable): 实机上电观察屏幕与串口日志

### UI / Storybook (if applicable)

- Stories to add/update: None
- Docs pages / state galleries to add/update: None
- `play` / interaction coverage to add/update: None
- Visual regression baseline changes (if any): None

### Quality checks

- `cargo +esp check --release`
- `cargo +esp check --release --bin test-fw --features test-fw-screen-static,test-fw-default-screen-static`
- `cargo +esp check --release --bin tps-test-fw --features tps-test-fw`

## 文档更新（Docs to Update）

- `firmware/README.md`: 增补 `tps-test-fw` 构建/烧录、固定 profile 与安全警示
- `docs/specs/README.md`: 新增本规格索引并更新状态

## 计划资产（Plan assets）

- Directory: `docs/specs/2uqhm-tps-bq-power-test-firmware/assets/`
- In-plan references: `![...](./assets/<file>.png)`
- PR visual evidence source: maintain `## Visual Evidence (PR)` in this spec when PR screenshots are needed.
- If an asset must be used in impl (runtime/test/official docs), list it in `资产晋升（Asset promotion）` and promote it to a stable project path during implementation.

## Visual Evidence (PR)

本轮暂无。

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 新建 `tps-test-fw` feature/bin 与独立规格索引。
- [x] M2: 完成最小板级 bring-up 与固定 profile 运行时。
- [x] M3: 完成 charger/TPS/INA/TMP 轮询、故障锁存与基础保护。
- [x] M4: 完成专用 `TPS TEST` 单页 UI 与 front-panel 渲染入口。
- [x] M5: 更新 `firmware/README.md` 并完成三组 `cargo +esp check`。

## 方案概述（Approach, high-level）

- 不复用主固件 `PowerManager`，避免把 `BMS`/自检授权链拖进测试固件。
- 复用已有底层驱动与板级 bring-up 代码，但把运行时状态机收缩为“固定 profile + 轮询 + 锁存”。
- 屏幕走专用渲染入口，保持前面板基础设施稳定，同时避免污染既有 dashboard / self-check 路由。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：
  - 现有前面板模块偏向 self-check/dashboard，新增专用页时需要谨慎避免回归。
  - `TPS55288` 遥测 helper 若直接复用旧日志函数，容易引入副作用，因此需要单独只读接口。
- 需要决策的问题：
  - None
- 假设（需主人确认）：
  - None

## 变更记录（Change log）

- 2026-03-21: 新建规格，冻结独立 `tps-test-fw` 的范围、默认 profile、UI 与验证门槛。
- 2026-03-21: 实现 `tps-test-fw`、固定 profile 运行时、独立屏显页与构建验证。

## 参考（References）

- `docs/specs/uwt77-test-fw-audio-navigation/SPEC.md`
- `docs/specs/958aj-standalone-display-diag-firmware/SPEC.md`
- `docs/specs/cqd8u-regulated-output-module/SPEC.md`
- `docs/specs/frsr9-regulated-output-active-protection/SPEC.md`
