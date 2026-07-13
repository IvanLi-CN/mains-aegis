# 规格（Spec）总览

本目录用于管理工作项的规格与追踪：记录范围、验收标准、任务清单与状态，作为实现与验证的依据。

> Legacy compatibility: 旧规格仍可保留在 `docs/plan/**/PLAN.md`。新规格统一写入 `docs/specs/**/SPEC.md`。

## 目录与命名规则

- 每个规格一个目录：`docs/specs/<id>-<title>/`
- `<id>`：推荐 5 个字符 nanoId 风格（字符集：`23456789abcdefghjkmnpqrstuvwxyz`）
- `<title>`：短标题 slug（kebab-case）
- 主文档：`docs/specs/<id>-<title>/SPEC.md`

## Index 状态（Status）说明

Index 表格的 `Status` 仅允许使用：`active`、`superseded(#<id>)`、`archived`。
各 `SPEC.md` 文件头仍兼容旧状态词，用于保留历史实现进度与迁移上下文。

## Index（固定表格）

| ID | Title | Status | Spec | Last | Notes |
| ---: | --- | --- | --- | --- | --- |
| xjpvj | UPS runtime mode switching | active | `xjpvj-runtime-mode-switching/SPEC.md` | 2026-06-24 | 统一 `STANDBY / ASSIST / BACKUP` 自动切换、`BYPASS` 边界与 VIN/fallback 真相源；`BACKUP` 默认 `NOAC`，受控 USB-C 低输出充电例外由 #eu2b8 承接 |
| rzx5v | Client transport priority matrix | archived | `rzx5v-client-transport-priority/SPEC.md` | 2026-06-08 | 跨 Web / devd / CLI 的通信方案优先级已抽成独立 topic spec；冻结 Web confirmed companion 的 FQDN-first 与 devd/CLI 的 USB-first 规则 |
| k4vzn | LAN management convergence | archived | `k4vzn-lan-management-convergence/SPEC.md` | 2026-06-03 | 设备本体 settings 读写 API、devd LAN discovery/scan trace/LAN settings 写路径、USB 优先合并、trace+connection 查询面、Web direct LAN/devd LAN settings 与最终视觉证据已收口 |
| 7jqrq | Mains Aegis CLI / devd alignment | archived | `7jqrq-mains-aegis-cli-devd-alignment/SPEC.md` | 2026-06-03 | 主机工具对齐与 release/install 基线已完成；CLI `device session` 与新查询面迁移改由 #k4vzn 接管 |
| p8k3d | Mains Aegis Device Daemon | archived | `p8k3d-mains-aegis-devd/SPEC.md` | 2026-06-14 | devd v1 foundation 已完成；`diag-snapshot`、synthetic power event 与 CLI trace follow 已补齐 `TPS output current` 停充根因、`lan_derived` 兼容面与 DC IN `1000mA/96%` 输入限值观测 |
| ypfpu | Web management UI | active | `ypfpu-web-management-ui/SPEC.md` | 2026-06-03 | Web 管理端 v1 基线已完成；LAN 管理、settings 与 LAN/USB 收敛改由 #k4vzn 接管 |
| hn29u | USB-C PD/PPS sink v1 | archived | `hn29u-usb-c-pd-sink-pps/SPEC.md` | 2026-04-23 | hotplug PPS 恢复已稳定闭环：reset 基线约 `1.67s` 回到 `PPS`，主人实测真实热插拔也已恢复到秒级协商成功 |
| nq7s2 | BQ40 balance baseline + observability | archived | `nq7s2-bq40-balance-baseline-and-observability/SPEC.md` | 2026-04-07 | PR #59 已完成实现/文档/预览收口；实板已确认 DF 对齐，active balancing 触发待后续在完整 charge/relax 条件下复核 |
| edbpk | BQ40 Cell4 protocol-safe diagnostics | archived | `edbpk-bq40-cell4-protocol-diagnostics/SPEC.md` | 2026-03-15 | 已完成协议修正、只读诊断收敛、flash/monitor 互斥与 reply PEC 探测；`Cell4` 根因已排除工具误读路径 |
| tmdtq | BQ40 tool reflash / recovery convergence | archived | `tmdtq-bq40-tool-reflash-recovery/SPEC.md` | 2026-03-11 | 工具链已可区分 ROM 检测/写入/退出与 post-flash 无效运行态；剩余问题收敛为样片硬件状态 |
| g2kte | Dashboard live after self-check | archived | `g2kte-dashboard-live-after-self-check/SPEC.md` | 2026-03-15 | 主固件改为“自检完成后自动进入 Dashboard”，并把 live Dashboard 的市电真相源统一到 `DC5025 VIN>=3V`；同时补齐 `PIN W` 的输入 ADC 样本净化与异常日志，避免 `~1000W` 误显 |
| f3c2g | Dashboard detail drill-down | archived | `f3c2g-dashboard-detail-drilldown/SPEC.md` | 2026-04-09 | 首页 5 区点击进入二级仪表盘；`Cells` 新增唯一 `BMS DETAIL` 子页，已补齐高级 BMS 状态 UI、预览图与 spec 视觉证据 |
| 4t9wx | Install UI UX Pro Max skill（Codex） | archived | `4t9wx-install-ui-ux-pro-max-skill/SPEC.md` | 2026-03-02 | 项目内安装并入库；修正 `.codex` 脚本路径与 pycache 忽略 |
| 6qrjs | Front panel industrial UI preview（320x172） | superseded(#7n4qd) | `6qrjs-front-panel-industrial-ui-preview/SPEC.md` | 2026-06-08 | 视觉基线保留；dashboard/menu/audio 的 `ACTION / SYSTEM` 预览口径已冻结，运行态试听接线转入 #h43mk，自检页运行语义仍由 #7n4qd 承载 |
| 7n4qd | MCU self-check live panel (resident Variant C) | superseded(#g2kte) | `7n4qd-mcu-self-check-live-panel/SPEC.md` | 2026-03-15 | 开机自检实时化能力保留，但默认 steady-state 页面已由 #g2kte 改为真实 Dashboard |
| 958aj | Standalone display diagnostic firmware | superseded(#uwt77) | `958aj-standalone-display-diag-firmware/SPEC.md` | 2026-03-05 | 已被 feature 驱动 `test-fw` 方案替代 |
| uwt77 | Test firmware navigation + audio priority | archived | `uwt77-test-fw-audio-navigation/SPEC.md` | 2026-03-05 | `test-fw` 已替换 display-diag；功能导航与音频优先级协调已验收 |
| h43mk | Main firmware runtime audio cues | archived | `h43mk-main-firmware-runtime-audio-cues/SPEC.md` | 2026-07-01 | 主固件已接入 B. Warm Tap 有效触摸/按键与 USB-C 插入 `ACTION` route 操作音；既有 15 组 `SYSTEM` cue 语义与 runtime DMA underrun 收敛策略保持不变 |
| hg3dw | Front panel visual language systematization | archived | `hg3dw-front-panel-visual-language/SPEC.md` | 2026-03-02 | 建立 Token/组件契约/视觉回归清单，补充 bitmap 字体字高白名单与预览图 |
| mecb8 | Status/warning/error speaker cues preview assets | archived | `mecb8-audio-cues-preview/SPEC.md` | 2026-03-05 | 15 组提示音试听资产（score + mid + wav）与增强预览页 |
| xy6cz | Front panel refresh pipeline | active | `xy6cz-front-panel-refresh-pipeline/SPEC.md` | 2026-03-15 | PR #41 已创建；已完成 PSRAM 双缓冲、dirty-band framebuffer 与 SPI DMA 主路径，等待 review-loop / 40MHz 联调结论回填 |
| ygmqn | Fan control with thermal/tach fail-safe | active | `ygmqn-fan-control/SPEC.md` | 2026-03-13 | PR #36 已创建；等待 review-loop 收敛 |
| 6n4qm | PCB netlist sync (2026-03-19) | archived | `6n4qm-pcb-netlist-sync-20260319/SPEC.md` | 2026-03-19 | 主板网表已同步到 2026-03-19 导出版本；前面板导出已确认与仓库零差异 |
| cqd8u | Regulated output module docs + runtime gate state machine | archived | `cqd8u-regulated-output-module/SPEC.md` | 2026-03-16 | 已建立 `docs/modules/`、收敛稳压输出 SoT，并落地显式恢复状态机与本地验证 |
| frsr9 | Regulated output active derating + shutdown | archived | `frsr9-regulated-output-active-protection/SPEC.md` | 2026-03-16 | 已落地温度/电流双门限主动降额、低压主动停机与显式恢复前置条件 |
| 2uqhm | TPS/BQ power test firmware | archived | `2uqhm-tps-bq-power-test-firmware/SPEC.md` | 2026-03-21 | 已实现独立 `tps-test-fw`、固定 profile 电源运行时、专用 `TPS TEST` 屏显与三组 `cargo +esp check` 验证 |
| eu2b8 | BQ25792 500mA charge policy + DC derate | archived | `eu2b8-bq25792-charge-policy/SPEC.md` | 2026-06-14 | 主线 charger state machine 已作为 SoT；DC IN 停充真相源使用 `TPS output current > 100mA`，并新增 BACKUP USB-C `<2W` 自动放行、`>3W`/两次缺样锁存与可观测状态 |
| 2drzf | BQ40 mainboard DF protection baseline | archived | `2drzf-bq40-mainboard-df-protection-baseline/SPEC.md` | 2026-04-03 | 冻结 `asset-df-mainboard` 的 `OCC/OCD/SOCC/SOCD` 主板基线，并把 `TMP + BMS` 最高温收敛为共享热控真相源 |
| mturr | Front panel display-chain long-press diagnostics | archived | `mturr-front-panel-display-chain-diagnostics/SPEC.md` | 2026-04-04 | 已完成主固件实现、本地构建、真机 flash/monitor 与 `CENTER` 长按 defmt 取证 |
| zp4cg | Manual charge dashboard page + EEPROM prefs | archived | `zp4cg-manual-charge-dashboard/SPEC.md` | 2026-04-07 | `MANUAL` 三级页面仅保存 prefs；手动 START 现需 USB-C 回环确认，确认 flag 仅在当前 RAM 会话有效 |
| jxz2t | GitHub Pages docs site handbooks | archived | `jxz2t-docs-site-handbooks/SPEC.md` | 2026-05-05 | Pages 根站点改由 Web App 发布，文档站保留为 `/docs/` 子路径；原 `docs-site/`、手册页面与 PR #63 记录仍为历史基线 |
| h6sae | BQ40 `LOCK` root cause + closure | active | `h6sae-bq40-lock-root-cause/SPEC.md` | 2026-04-13 | 已命中 `termination` 分流并提交 `ITERM` 对齐修复；下一步需要 `<90%` 解锁后的 live 闭环复验 |
| amc32 | WiFi / service discovery / read-only API foundation | active | `amc32-wifi-service-discovery-api-foundation/SPEC.md` | 2026-06-03 | `net_http` 与 `web_serial` 已成为默认主固件能力；“LAN 只读 API” 假设由 #k4vzn 继续演进 |
| d8p4q | Front panel auto sleep | active | `d8p4q-front-panel-auto-sleep/SPEC.md` | 2026-04-27 | 测试版 `30s / 35s / 40s` 自动低亮、关背光、GC9307 sleep；硬件确认后恢复正式默认 `180s / 240s / 245s` |
| 6xb4z | EEPROM storage layout | active | `6xb4z-eeprom-storage-layout/SPEC.md` | 2026-06-09 | 全局 EEPROM map、record 编码、CRC、默认回退与扩展规则的长期规范 |
| 0003 | Mains Aegis device operation guardrails | active | `0003-device-operation-guardrails/SPEC.md` | 2026-07-13 | Agent 真机操作的安全边界、devd 路由和授权要求 |
