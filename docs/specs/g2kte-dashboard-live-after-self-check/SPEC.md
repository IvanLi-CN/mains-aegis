# 自检后切入真实仪表盘（#g2kte）

## 状态

- Status: 已完成
- Created: 2026-03-15
- Last: 2026-03-15

## 背景 / 问题陈述

- 现有主固件会在开机自检后停留在 `Variant C` 自检页。
- `Variant B` Dashboard 仍由演示模型驱动，关键读数不是来自真实外设采样。
- 新需求要求：保留开机自检过程展示，但自检完成后自动进入 Dashboard，并且 Dashboard 的所有数值都必须来自真实外设。

## 目标 / 非目标

### Goals

- 开机阶段继续显示 `Variant C` 自检页，并按既有阶段回调逐步更新状态。
- 自检完成且运行态首份快照准备好后，自动切换到 `Variant B` Dashboard。
- Dashboard 所有指标改为真实数据源：
  - `PIN W`：`BQ25792 VBUS_ADC * IBUS_ADC`
  - `POUT / VOUT / IOUT / TPS OUT`：`INA3221 + TPS55288`
  - `BATTERY / DISCHG`：`BQ40Z50`
  - `TEMP`：`TMP112`
- 严格真实源：任何缺失值显示 `N/A`，不再回落到演示波动值。
- `PIN W` 的输入样本必须先经过有效性过滤：仅在输入存在、`BQ25792` ADC 已完成稳定转换、且 `VBUS/IBUS` 落在合法量程内时才允许进入 Dashboard。

### Non-goals

- 不新增 Dashboard 返回自检页的入口。
- 不改 `Variant B` 的布局、文案体系与视觉 token。
- 不改既有 BMS 激活/告警流程，只改变自检完成后的默认落点。

## 范围（Scope）

### In scope

- `firmware/src/front_panel_scene.rs`
  - 扩展 `SelfCheckUiSnapshot` 承载 Dashboard 真实字段。
  - 保留演示 fallback 渲染路径，仅在传入真实快照时启用 live Dashboard。
- `firmware/src/output/mod.rs`
  - 启动自检、运行态 TPS/BQ25792/BQ40 轮询统一写回 Dashboard 所需真实字段。
- `firmware/src/output/tps55288.rs`
  - `TelemetryCapture` 带回 `vbus_mv`。
- `firmware/src/bq25792.rs`
  - 新增 `IBUS_ADC` / `VBUS_ADC` 寄存器与 signed ADC 读取辅助。
- `firmware/src/front_panel.rs`
  - 自检完成后切换到 `Variant B`，运行期默认停留 Dashboard。
- `tools/front-panel-preview/src/main.rs`
  - 新增真实 Dashboard fixture 场景：`dashboard-runtime-standby` / `assist` / `backup`。

### Out of scope

- 新增远程遥测、存档或历史趋势图。
- 修复 `INA3221 CH3` 量测偏差问题；本任务明确绕过该不稳定来源。

## 接口变更（Interfaces）

- `front_panel_scene::SelfCheckUiSnapshot`
  - 新增：`input_vbus_mv`、`input_ibus_ma`、`out_a_vbus_mv`、`out_b_vbus_mv`、`bq40z50_pack_mv`、`bq40z50_current_ma`
- `bq25792`
  - 新增：`reg::IBUS_ADC`、`reg::VBUS_ADC`、`read_i16(...)`
  - 新增：稳定 `power-path ADC` 配置/就绪辅助，要求 `continuous + averaging + stable sample rate`，并显式校验 `ADC_DONE`
- `tps55288::TelemetryCapture`
  - 新增：`vbus_mv`
- `FrontPanel`
  - 新增：`enter_dashboard()`

## 验收标准（Acceptance Criteria）

- Given 屏幕链路可用，When 开机进入主固件，Then 首屏仍为 `SELF CHECK`。
- Given 自检结束且 `PowerManager` 已产出运行态快照，When UI 进入 steady state，Then 自动切换到 `UPS DASHBOARD`，且运行中不再回切自检页。
- Given Dashboard 某个真实字段缺失，When 渲染对应区域，Then 显示 `N/A`，meter 归零，且不显示任何演示波动数值。
- Given 运行态输入侧存在 VBUS/IBUS ADC 样本，When Dashboard 显示 `PIN W`，Then 读数来自 `BQ25792` ADC，不依赖 `INA3221 CH3`。
- Given `IBUS_ADC<=0` 但输入状态仍为在线，When Dashboard 计算 `PIN W`，Then 显示 `0.0W`，不再把逆流/空载样本转成正功率。
- Given `ADC_DONE=0`、`VBUS_ADC>30000mV`、`|IBUS_ADC|>5000mA` 或寄存器样本缺失，When Dashboard 渲染 `PIN W`，Then 显示 `N/A`，并把原始寄存器保留到诊断日志。
- Given `tools/front-panel-preview` 运行真实 Dashboard 场景，When 导出 PNG，Then `preview.png` 分辨率为 `320x172`。

## 实现记录

- 通过 `FrontPanel::enter_dashboard()` 把页面状态从自检页切到 Dashboard，并在切换时清空 overlay。
- `render_variant_b` 改为优先消费 `SelfCheckUiSnapshot`；只有未传快照时才保留历史 demo fallback。
- `PowerManager` 在 BQ25792/TPS/BQ40 轮询中持续维护 Dashboard 真实字段，保证启动自检与运行态共用同一份 UI 快照。
- `PowerManager` 对 `BQ25792` 输入侧 ADC 增加“稳定配置 + 样本净化”步骤，只把合法正向输入功率样本写入 `SelfCheckUiSnapshot`。
- 对 `>200W` 的异常原始输入功率样本增加限频诊断日志，日志固定带出 `raw_ibus_adc`、`raw_vbus_adc`、`ADC_CONTROL`、`ADC_DONE`、`VBUS_STAT` 与输入存在位。
- Preview 工具增加 3 组 runtime fixture，用于验证 `standby / assist / backup` 三个 Dashboard 场景。

## 验证记录

- `cargo build --manifest-path /Users/ivan/.codex/worktrees/a8e4/mains-aegis/tools/front-panel-preview/Cargo.toml`
- `cargo test --manifest-path /Users/ivan/.codex/worktrees/a8e4/mains-aegis/firmware/Cargo.toml front_panel_scene --lib`
- `cargo build --release --manifest-path /Users/ivan/.codex/worktrees/a8e4/mains-aegis/firmware/Cargo.toml`
- `cargo run --manifest-path /Users/ivan/.codex/worktrees/a8e4/mains-aegis/tools/front-panel-preview/Cargo.toml -- --variant B --focus idle --scenario dashboard-runtime-standby --out-dir <ABS_PATH>`
- `cargo run --manifest-path /Users/ivan/.codex/worktrees/a8e4/mains-aegis/tools/front-panel-preview/Cargo.toml -- --variant B --focus idle --scenario dashboard-runtime-assist --out-dir <ABS_PATH>`
- `cargo run --manifest-path /Users/ivan/.codex/worktrees/a8e4/mains-aegis/tools/front-panel-preview/Cargo.toml -- --variant B --focus idle --scenario dashboard-runtime-backup --out-dir <ABS_PATH>`
- 上板日志验证：适配器在线与纯电池两种状态下，`PIN W` 不再跳到 `~1000W`；若原始 ADC 仍出现异常，只能在限频诊断日志中看到，Dashboard 不再误显。

## 关联规格

- `docs/specs/7n4qd-mcu-self-check-live-panel/SPEC.md`
- `docs/specs/6qrjs-front-panel-industrial-ui-preview/SPEC.md`

## 变更记录（Change log）

- 2026-03-15: 收紧 `PIN W` 输入样本契约：`BQ25792` ADC 必须处于稳定连续采样并完成转换；逆流/空载样本显示 `0.0W`，未就绪/越界样本显示 `N/A`，并新增异常原始样本诊断日志。
- 2026-03-15: 自检页不再作为 steady-state 默认页；切换为“自检 -> 真实 Dashboard”。
