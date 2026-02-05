# 前面板屏幕显示：Hello World + FPS overlay bring-up（#3kz8p）

## 状态

- Status: 部分完成（3/4）
- Created: 2026-02-05
- Last: 2026-02-05

## 背景 / 问题陈述

- 本项目已确定前面板具备：TFT 屏幕（SPI）、电容触摸（I2C2）、五向按键（上/下/左/右/中）、以及用于屏幕/触摸慢控制线的 IO 扩展器 `TCA6408A`（I2C2，`0x21`）。
- 屏幕驱动 IC 为 `GC9307`（`240RGBx320`、262K color；支持串行接口），触摸控制器为 `CST816D`（I2C；IRQ 为独立 `CTP_IRQ`）。
- 现状：固件 bring-up 已覆盖电源侧遥测与告警链路（日志可观测），但尚未建立“屏幕可观测”的最小闭环。缺少这条闭环会使联调强依赖串口日志，不利于快速确认屏幕连线、初始化顺序与背光控制是否正确。

## 目标 / 非目标

### Goals

- 冻结并交付一个“Hello World 级别”的最小屏幕显示：上电后能稳定显示一行文本（用于确认屏幕初始化链路正确）。
- 冻结屏幕相关的上电初始化顺序与失败恢复策略：遵循硬件默认安全态（上电默认不选中屏幕、屏幕保持复位），异常时可回到安全态并可重试恢复。

### Non-goals

- 不在本计划内实现 Dashboard/菜单/告警页等“功能性 UI”（例如遥测展示、状态页、强告警 overlay）。
- 不在本计划内实现空闲熄屏/调光（idle backlight policy）。
- 不在本计划内引入重型 GUI 框架（例如 LVGL）作为默认方案；本阶段仅做 bring-up 级验证与最小渲染。
- 不在本计划内实现触摸（不初始化 `CST816D`；默认保持 `TP_RESET` 为低，使触摸处于复位态）。
- 不在本计划内更改硬件（屏幕/触摸器件、连线、接口定义等）。

## 范围（Scope）

### In scope

- 固件侧最小屏幕 bring-up：能初始化并显示文本（`Hello World`）。
- 背光：bring-up 阶段把背光打开即可（不实现空闲熄屏/调光策略）。
- 文档沉淀：初始化顺序、失败策略、以及上板验证步骤（人类操作）。

### Out of scope

- 遥测数据（OUT-A/OUT-B 电压/电流/温度等）的屏幕展示与刷新策略。
- 五向按键导航与页面切换（如需，再开后续计划或在本计划完成后追加范围变更）。
- 触摸 bring-up（`CST816D` 与 `CTP_IRQ`）；若要做，建议另开计划，避免把屏幕 bring-up 拖成大杂烩。
- 与 Wi-Fi/BLE 配网、手机 App、Web 控制台等相关的 UI（若需要，另开计划）。

## 事实来源（Hardware facts）

> 本计划对硬件事实的引用以仓库离线文档为准：
>
> - 前面板：`docs/pcbs/front-panel/README.md`
> - MCU GPIO：`docs/hardware-selection/esp32-s3-fh4r2-gpio.md`
> - I2C 地址与中断汇总：`docs/i2c-address-map.md`
> - 屏幕：`docs/datasheets/GC9307/GC9307.md`
> - 触摸：`docs/datasheets/CST816D/CST816D.md`

## 关键硬件约束（Constraints）

- 显示接口：
  - `SPI`：`SCLK(GPIO12)` / `MOSI(GPIO11)` / `DC(GPIO10)`（固定分配）
  - `CS/RES`：由 `TCA6408A` 提供（面板侧将 `CS` 作为“使能/闸门”使用，预期不需要每次传输翻转）
  - 背光：`BLK(GPIO13)` 控制前面板 `Q16(BSS84)` 高边开关（需在 bring-up 时确认极性与默认行为）
- 触摸接口：
  - `I2C2`（`GPIO8/9`，目标 `400kHz`）
  - `CTP_IRQ(GPIO14)` 为独立触摸中断线（不与 `I2C2_INT(GPIO7)` 共线）
  - `TP_RESET` 由 `TCA6408A` 控制（低有效）
- 默认安全态（由外部 100k 偏置决定）：
  - `CS` 默认上拉为高（不选中屏幕，SPI 访问被屏蔽）
  - `RES/TP_RESET` 默认下拉为低（屏幕/触摸保持复位）
  - 结论：在 `TCA6408A` 未初始化前，不应尝试访问屏幕/触摸（必须先初始化扩展器并显式释放复位/使能）

## 需求（Requirements）

### MUST（最小可交付）

- 必须在屏幕上显示固定文本（建议固定为 ASCII，便于回归）：`Hello World`。
- 必须在屏幕角落显示“刷新率”（本计划定义为固件渲染循环的帧率 `fps`，不是面板物理扫描频率）：
  - 位置：右上角（或左上角，二选一，但需固定）
  - 格式：`fps=<n>`（`n` 为整数或一位小数）
  - 更新频率：至少 `1Hz`（每秒更新一次即可）
- 背光：在屏幕初始化完成后打开背光（`BLK(GPIO13)`）；本计划不实现 idle 熄屏/调光。
- 必须冻结“屏幕初始化顺序”与“失败恢复策略”：
  - 初始化顺序：先 `TCA_RESET#` / `TCA6408A`，再 `RES/CS`（屏幕），最后打开背光
  - 触摸策略（冻结）：本计划不做触摸；`TP_RESET` 保持为低（触摸保持复位态）
  - 恢复策略：若 I2C2 异常则通过 `TCA_RESET#` 复位扩展器以回到安全态，并允许重试初始化
- 任一初始化失败（I2C2/SPI NACK/timeout 等）时，固件不得 panic；应保持硬件默认安全态（`CS` 不选中、`RES/TP_RESET` 复位保持），并继续通过串口日志输出用于定位的信息。

### SHOULD（增强，但不阻塞最小交付）

- 屏幕刷新策略：最小实现允许全屏清屏+重绘；后续若需要更高帧率再引入脏矩形/局部刷新。

## UI 内容（冻结口径）

> 本计划不做“页面/导航”。仅冻结屏幕上要出现的最小内容，便于实现与回归。

- 第一行：`Hello World`
- 角落：`fps=<n>`

## 验收标准（Acceptance Criteria）

- Given 前面板已连接且 `TCA6408A(0x21)`、屏幕（GC9307）可访问，
  When 固件启动完成初始化并打开背光，
  Then 屏幕在 `<= 2s` 内显示 `Hello World` 与 `fps=<n>`，且 `fps` 数值至少每秒更新一次（允许低帧率；本计划不设最低 fps 门槛）。

- Given 屏幕初始化失败，
  When 系统进入保守策略继续运行，
  Then 屏幕保持在硬件默认安全态（`CS` 不选中、`RES/TP_RESET` 复位保持），且固件继续输出串口日志用于定位问题（不要求屏幕有输出）。

## 实现前置条件（Definition of Ready / Preconditions）

- 已确认本计划范围为“Hello World bring-up”，不包含遥测显示、菜单、告警页、触摸与空闲熄屏策略。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Build: `cargo build --release`（固件）
- Manual bring-up: 固化“屏幕显示验证”的人类操作步骤到 `firmware/README.md`（不要求 Agent 执行写入类设备操作）。

## 文档更新（Docs to Update）

- `firmware/README.md`: 增加“前面板 UI bring-up”章节（初始化顺序、验证步骤、常见故障与恢复策略）。
- `docs/pcbs/front-panel/README.md`: 若 bring-up 过程中发现 `BLK/CS/RES/TP_RESET` 极性与文档缺口，补齐最小说明（不改动网表事实）。

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones）

- [x] M1: 面板控制线 bring-up：初始化 `TCA6408A(0x21)`，可控 `CS/RES/TP_RESET`，并确认 `BLK(GPIO13)` 能点亮背光
- [x] M2: 屏幕显示 bring-up：初始化 `GC9307` 并显示 `Hello World`
- [x] M3: 文档固化：`firmware/README.md` 验证步骤
- [ ] M4: 上板验证（人类操作）：确认 `Hello World` + `fps=<n>` 可见；若黑屏优先排查背光极性与初始化顺序

## 方案概述（Approach, high-level）

- 渲染栈默认建议从轻量路线开始：`embedded-hal` SPI +（可选）`embedded-graphics` 文本/图形；后续若交互复杂度上升再评估 GUI 框架。
- 刷新策略先可实现正确性（稳定显示 + 不影响主循环），再以测量数据驱动性能优化（SPI 频率、DMA、局部刷新）。
- 以硬件默认安全态为底线：任何异常都应可回到“屏幕不被选中 + 复位保持”的状态，避免在 I2C/SPI 异常时造成系统不可恢复。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：
  - `BLK(GPIO13)` 极性与背光默认行为需要上板确认，否则可能出现“屏幕已显示但看起来黑屏”的误判。
- 开放问题（需要决策）：
  - 后续是否扩展为“遥测 Dashboard + 告警页 + 按键导航”？若要扩展，建议另开计划以避免把 bring-up 计划拖成大杂烩。
  - 触摸 bring-up 是否需要单独立项？（本计划已明确不做触摸）
- 假设（已由主人确认）：
  - 本计划仅做 `Hello World` 级别屏幕显示，暂不做触摸，且暂不做空闲熄屏/调光策略。

## 变更记录（Change log）

- 2026-02-05: 新建计划骨架（待冻结 UI 信息架构与验收口径）
- 2026-02-05: 冻结 MVP：不做触摸；屏幕仅 `Hello World`；暂不做 idle backlight policy
- 2026-02-05: 实现：屏幕显示 `Hello World` + `fps=<n>`，并补齐 `firmware/README.md` bring-up 说明

## 参考（References）

- `docs/pcbs/front-panel/README.md`
- `docs/hardware-selection/esp32-s3-fh4r2-gpio.md`
- `docs/i2c-address-map.md`
- `docs/datasheets/GC9307/GC9307.md`
- `docs/datasheets/CST816D/CST816D.md`
- `firmware/README.md`
