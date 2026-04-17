# 规格（Spec）总览

本目录用于管理工作项的规格与追踪：记录范围、验收标准、任务清单与状态，作为实现与验证的依据。

> Legacy compatibility: 旧规格仍可保留在 `docs/plan/**/PLAN.md`。新规格统一写入 `docs/specs/**/SPEC.md`。

## 目录与命名规则

- 每个规格一个目录：`docs/specs/<id>-<title>/`
- `<id>`：推荐 5 个字符 nanoId 风格（字符集：`23456789abcdefghjkmnpqrstuvwxyz`）
- `<title>`：短标题 slug（kebab-case）
- 主文档：`docs/specs/<id>-<title>/SPEC.md`

## 状态（Status）说明

仅允许使用：`待设计`、`待实现`、`跳过`、`部分完成（x/y）`、`已完成`、`作废`、`重新设计（#<id>）`。

## Index（固定表格）

| ID | Title | Status | Spec | Last | Notes |
| ---: | --- | --- | --- | --- | --- |
| hn29u | USB-C PD/PPS sink v1 | 已完成 | `hn29u-usb-c-pd-sink-pps/SPEC.md` | 2026-04-08 | 已完成默认全开 + blacklist feature、USB-C 协商/重协商禁充 gate、PPS keep-alive、host audit 覆盖，以及 review-loop 收敛的协议版本/5V 回落/WAIT 恢复/系统负载预算修正 |
| nq7s2 | BQ40 balance baseline + observability | 已完成 | `nq7s2-bq40-balance-baseline-and-observability/SPEC.md` | 2026-04-07 | PR #59 已完成实现/文档/预览收口；实板已确认 DF 对齐，active balancing 触发待后续在完整 charge/relax 条件下复核 |
| edbpk | BQ40 Cell4 protocol-safe diagnostics | 已完成 | `edbpk-bq40-cell4-protocol-diagnostics/SPEC.md` | 2026-03-15 | 已完成协议修正、只读诊断收敛、flash/monitor 互斥与 reply PEC 探测；`Cell4` 根因已排除工具误读路径 |
| tmdtq | BQ40 tool reflash / recovery convergence | 已完成 | `tmdtq-bq40-tool-reflash-recovery/SPEC.md` | 2026-03-11 | 工具链已可区分 ROM 检测/写入/退出与 post-flash 无效运行态；剩余问题收敛为样片硬件状态 |
| g2kte | Dashboard live after self-check | 已完成 | `g2kte-dashboard-live-after-self-check/SPEC.md` | 2026-03-15 | 主固件改为“自检完成后自动进入 Dashboard”，并把 live Dashboard 的市电真相源统一到 `DC5025 VIN>=3V`；同时补齐 `PIN W` 的输入 ADC 样本净化与异常日志，避免 `~1000W` 误显 |
| f3c2g | Dashboard detail drill-down | 已完成 | `f3c2g-dashboard-detail-drilldown/SPEC.md` | 2026-04-09 | 首页 5 区点击进入二级仪表盘；`Cells` 新增唯一 `BMS DETAIL` 子页，已补齐高级 BMS 状态 UI、预览图与 spec 视觉证据 |
| 4t9wx | Install UI UX Pro Max skill（Codex） | 已完成 | `4t9wx-install-ui-ux-pro-max-skill/SPEC.md` | 2026-03-02 | 项目内安装并入库；修正 `.codex` 脚本路径与 pycache 忽略 |
| 6qrjs | Front panel industrial UI preview（320x172） | 重新设计（#7n4qd） | `6qrjs-front-panel-industrial-ui-preview/SPEC.md` | 2026-03-01 | 视觉基线保留；自检页运行语义迁移到 #7n4qd |
| 7n4qd | MCU self-check live panel (resident Variant C) | 重新设计（#g2kte） | `7n4qd-mcu-self-check-live-panel/SPEC.md` | 2026-03-15 | 开机自检实时化能力保留，但默认 steady-state 页面已由 #g2kte 改为真实 Dashboard |
| 958aj | Standalone display diagnostic firmware | 重新设计（#uwt77） | `958aj-standalone-display-diag-firmware/SPEC.md` | 2026-03-05 | 已被 feature 驱动 `test-fw` 方案替代 |
| uwt77 | Test firmware navigation + audio priority | 已完成 | `uwt77-test-fw-audio-navigation/SPEC.md` | 2026-03-05 | `test-fw` 已替换 display-diag；功能导航与音频优先级协调已验收 |
| h43mk | Main firmware runtime audio cues | 已完成 | `h43mk-main-firmware-runtime-audio-cues/SPEC.md` | 2026-04-04 | 主固件已补齐 runtime DMA underrun burst 恢复与止损策略；idle 板卡的周期性滴声按 transport 级 `Late` 回归收敛 |
| hg3dw | Front panel visual language systematization | 已完成 | `hg3dw-front-panel-visual-language/SPEC.md` | 2026-03-02 | 建立 Token/组件契约/视觉回归清单，补充 bitmap 字体字高白名单与预览图 |
| mecb8 | Status/warning/error speaker cues preview assets | 已完成 | `mecb8-audio-cues-preview/SPEC.md` | 2026-03-05 | 15 组提示音试听资产（score + mid + wav）与增强预览页 |
| xy6cz | Front panel refresh pipeline | 部分完成（4/5） | `xy6cz-front-panel-refresh-pipeline/SPEC.md` | 2026-03-15 | PR #41 已创建；已完成 PSRAM 双缓冲、dirty-band framebuffer 与 SPI DMA 主路径，等待 review-loop / 40MHz 联调结论回填 |
| ygmqn | Fan control with thermal/tach fail-safe | 部分完成（4/5） | `ygmqn-fan-control/SPEC.md` | 2026-03-13 | PR #36 已创建；等待 review-loop 收敛 |
| 6n4qm | PCB netlist sync (2026-03-19) | 已完成 | `6n4qm-pcb-netlist-sync-20260319/SPEC.md` | 2026-03-19 | 主板网表已同步到 2026-03-19 导出版本；前面板导出已确认与仓库零差异 |
| cqd8u | Regulated output module docs + runtime gate state machine | 已完成 | `cqd8u-regulated-output-module/SPEC.md` | 2026-03-16 | 已建立 `docs/modules/`、收敛稳压输出 SoT，并落地显式恢复状态机与本地验证 |
| frsr9 | Regulated output active derating + shutdown | 已完成 | `frsr9-regulated-output-active-protection/SPEC.md` | 2026-03-16 | 已落地温度/电流双门限主动降额、低压主动停机与显式恢复前置条件 |
| 2uqhm | TPS/BQ power test firmware | 已完成 | `2uqhm-tps-bq-power-test-firmware/SPEC.md` | 2026-03-21 | 已实现独立 `tps-test-fw`、固定 profile 电源运行时、专用 `TPS TEST` 屏显与三组 `cargo +esp check` 验证 |
| eu2b8 | BQ25792 500mA charge policy + DC derate | 已完成 | `eu2b8-bq25792-charge-policy/SPEC.md` | 2026-04-06 | 主线 charger state machine 已作为 SoT；已补齐 `LOAD` 的 `2入3出` 回差、输出功率未知保守禁充、首页 `ChargeCard` runtime 紧凑映射，以及 `BQ25792` ADC 的 `MSB-first` 遥测解码 |
| 2drzf | BQ40 mainboard DF protection baseline | 已完成 | `2drzf-bq40-mainboard-df-protection-baseline/SPEC.md` | 2026-04-03 | 冻结 `asset-df-mainboard` 的 `OCC/OCD/SOCC/SOCD` 主板基线，并把 `TMP + BMS` 最高温收敛为共享热控真相源 |
| mturr | Front panel display-chain long-press diagnostics | 已完成 | `mturr-front-panel-display-chain-diagnostics/SPEC.md` | 2026-04-04 | 已完成主固件实现、本地构建、真机 flash/monitor 与 `CENTER` 长按 defmt 取证 |
| zp4cg | Manual charge dashboard page + EEPROM prefs | 已完成 | `zp4cg-manual-charge-dashboard/SPEC.md` | 2026-04-07 | 已完成 `MANUAL` 三级页面、小屏触控布局、运行时手动接管/停止抑制、仅保存 prefs 的 EEPROM schema v1，以及预览/真机验证闭环 |
| jxz2t | GitHub Pages docs site handbooks | 已完成 | `jxz2t-docs-site-handbooks/SPEC.md` | 2026-04-08 | 已完成 `docs-site/`、GitHub Pages workflow、双手册页面、视觉证据与 PR #63 收敛 |
| 9rmmn | Front panel screen docs restructure | 已完成 | `9rmmn-front-panel-screen-docs/SPEC.md` | 2026-04-09 | PR #65 已完成文档 IA 重构、SoT 收敛、visual evidence、preview 验证与 review-loop 收口 |
