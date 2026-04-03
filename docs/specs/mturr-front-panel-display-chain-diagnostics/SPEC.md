# 前面板显示链路长按诊断与重初始化（#mturr）

## 状态

- Status: 已完成
- Created: 2026-04-03
- Last: 2026-04-04

## 背景 / 问题陈述

- PR #50 已把 `TCA_RESET#` 改为 MCU 端推挽驱动，显著降低了前面板黑屏复现概率，但现场仍偶发出现显示异常。
- 当前主固件缺少一个“运行时就地取证 + 原地恢复”的入口；一旦屏幕链路出现异常，只能依赖重新上电或重新烧录后的串口日志推断。
- 需要把 `CENTER` 长按重新定义为“显示链路诊断钩子”：在不改页面布局和不引入独立测试固件的前提下，实时采样 `TCA6408A`、触摸控制器与关键 MCU GPIO 状态，并立刻重走一遍屏幕初始化链路。

## 目标 / 非目标

### Goals

- 在主固件里新增一个前面板就绪后全局可用的 `CENTER` 长按诊断入口，阈值固定为约 `800ms`。
- 长按命中后先通过 `defmt` 打印显示链路现场状态，再执行一次完整的面板重初始化。
- 诊断日志必须覆盖 `TCA6408A` 原始寄存器、解释后的关键位状态、`CST816D` 可达性/触摸头信息，以及 `GPIO1/10/13/0/14` 电平。
- 重初始化必须复用启动链路时序：`TCA_RESET# -> TCA 安全态 -> RES/TP_RESET/CS -> GC9307 init`，成功后恢复当前 UI 页面与上层状态。
- 保持现有中键短按行为即时生效；若继续按住超过阈值，再额外触发一次长按诊断，且同次按压只触发一次。

### Non-goals

- 不新增独立诊断页面、`test-fw` 路由或新的屏幕 UI 布局。
- 不把 `FUSB302B`、`BQ25792`、`INA3221` 等非显示链路器件纳入本次长按采样范围。
- 不尝试读取 `GC9307` 控制器内部寄存器，也不新增 SPI 回读通道。

## 范围（Scope）

### In scope

- `firmware/src/front_panel.rs`
  - `CENTER` 长按状态机与单次按压闸门。
  - `ui: display_diag ...` / `ui: display_reinit ...` 日志。
  - 共享显示链路 bring-up / recover helper。
  - 运行时恢复后的当前页面重绘。
- `firmware/README.md`
  - 新增长按诊断入口说明与预期日志。
- `docs/specs/README.md`
  - 新增本规格索引。

### Out of scope

- 修改 `firmware/src/bin/test-fw.rs`。
- 调整 `front_panel_scene` 页面视觉设计。
- 新增或修改任何 GitHub 外部协议/CLI 接口。

## 接口变更（Interfaces）

- 无新的对外接口；改动均为主固件内部运行时能力。
- `FrontPanel` 内部新增：
  - `CENTER` 按压起始时间与本次按压是否已触发长按的状态。
  - `TCA6408A` 四寄存器采样 helper。
  - `CST816D` 原始触摸头采样 helper。
  - 共用的显示链路 `reinitialize_display_path()` 恢复入口。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- `FrontPanel::tick()` 继续保留现有短按逻辑：`CENTER` 按下边沿触发的弹窗确认/关闭、详情返回等行为不做延迟。
- 当 `CENTER` 持续保持按下且累计时间达到 `800ms` 时，固件立刻打印一次 `ui: display_diag trigger=center_long_press ...`，并在同一主循环周期内执行面板重初始化。
- 长按触发后，在本次按压释放前不得再次触发诊断或重初始化。
- 显示链路恢复成功后，固件按当前 `ui_variant / dashboard_route / self_check_overlay / self_check_snapshot / bms_activation_state` 重绘当前页面；不重置业务状态。
- 若重初始化任一阶段失败，固件输出带 stage 的 `ui: display_reinit ... result=err ...` 日志，并回到前面板 disabled/fail-safe 状态。

### 采样矩阵

- `TCA6408A`：`INPUT / OUTPUT / POLARITY / CONFIG` 四个寄存器原始值。
- `TCA6408A` 解释态：`up / down / left / right / usb2_pg / cs_enabled / res_released / tp_reset_released`。
- `CST816D`：一次 `0x01..0x06` 触摸头读取结果，包含 `gesture / finger_count / raw_x / raw_y / mapped_x / mapped_y`；失败时打印 `i2c_error_kind(...)`。
- MCU GPIO：`GPIO1(TCA_RESET#)`、`GPIO10(DC)`、`GPIO13(BLK)`、`GPIO0(BTN_CENTER)`、`GPIO14(CTP_IRQ)` 当前电平；同时给出 `backlight_on` 推导值。

### Edge cases / errors

- `TCA6408A` 或 `CST816D` 任一采样失败时，只记录错误，不阻断后续采样与重初始化。
- 在 Dashboard detail 页长按 `CENTER` 时，允许先执行短按返回 Home，再在继续按住达到阈值后触发诊断。
- 如果 `redraw_restore` 失败，固件应把前面板拉回 fail-safe，而不是停留在“驱动已初始化但页面未知”的中间态。

## 验收标准（Acceptance Criteria）

- 长按 `CENTER` 超过 `800ms` 后，日志中必须出现一次且仅一次 `ui: display_diag trigger=center_long_press ...`。
- `ui: display_diag` 必须包含：
  - `TCA6408A` 的原始 `INPUT/OUTPUT/POLARITY/CONFIG`。
  - 解释后的 `up/down/left/right/usb2_pg/cs_enabled/res_released/tp_reset_released`。
  - `GPIO1/10/13/0/14` 当前电平。
  - `CST816D` probe 成功或失败结果。
- `ui: display_reinit` 必须能区分 `tca_reset`、`tca_init`、`release_lines`、`gc9307_init`、`redraw_restore`、最终 `ok/err`。
- 自检页与 Dashboard 首页长按后，页面必须恢复到触发前所在页面。
- Dashboard detail 页长按后，允许先执行短按返回 Home，再在同次长按中触发诊断与恢复。
- 恢复成功后前面板继续正常刷新；恢复失败时主循环不得卡死。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- `cargo +esp fmt --all`
- `cargo +esp check --release`
- 真机 `mcu-agentd` 烧录 + `defmt` 观察：至少覆盖自检页、Dashboard 首页、Dashboard 详情页三类场景。

### 验证证据

- 2026-04-04 使用 `mcu-agentd --non-interactive flash esp` 将 `th/front-panel-display-chain-diagnostics` / `aeead36` 烧录到已绑定设备 `/dev/cu.usbmodem412101`。
- 监视日志写入 `/.mcu-agentd/monitor/esp/20260403_193616.mon.ndjson`；启动阶段可见 `ui: display_reinit trigger=boot_init stage=tca_reset|tca_init|release_lines|gc9307_init`。
- 在 Dashboard Home 实机长按 `CENTER` 后，日志出现且仅出现一次 `ui: display_diag trigger=center_long_press page=B route=home overlay=none bms_state=idle center=true touch=false`。
- 同一次长按后，日志按顺序出现 `ui: display_reinit trigger=center_long_press stage=tca_reset|tca_init|release_lines|gc9307_init|redraw_restore|ok`，且未出现 `result=err`。

### Quality checks

- 所有新增日志前缀必须稳定为 `ui: display_diag` 或 `ui: display_reinit`。
- 不得改变现有页面布局或新增额外导航入口。

## 文档更新（Docs to Update）

- `docs/specs/README.md`
- `firmware/README.md`

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 建立规格并登记到 `docs/specs/README.md`
- [x] M2: 在主固件里新增 `CENTER` 长按状态机与单次按压闸门
- [x] M3: 新增长按诊断采样日志与共享显示链路重初始化 helper
- [x] M4: README 同步长按诊断入口、日志契约与运行时行为
- [x] M5: 真机验证 + fast-track PR 收敛到 merge-ready

## 风险 / 假设（Risks, Assumptions）

- 风险：当前 `firmware/` 的依赖子模块若未初始化，本地 `cargo +esp check --release` 可能无法在当前 worktree 直接跑通；需要在验证前先补齐子模块。
- 假设：`Flex` 输出 GPIO 在开启 input enable 后可稳定读取当前电平，不会影响现有驱动能力。
- 假设：保持 `CENTER` 短按优先不会影响本次诊断入口的可用性，反而更符合当前 UI 契约。

## 变更记录（Change log）

- 2026-04-03: 新增显示链路长按诊断与重初始化规格，冻结采样矩阵、触发策略与恢复口径。
- 2026-04-04: 完成真机 flash/monitor 验证，确认 `CENTER` 长按日志单次触发与显示链路重初始化阶段完整。
