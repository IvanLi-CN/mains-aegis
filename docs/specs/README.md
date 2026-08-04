# 规格（Spec）总览

本目录是 topic-level specification catalog：每个 spec 对应一个稳定主题、能力、接口或约束的长期契约，不是单次任务的进度卡片。

每个 spec 目录包含：

- `SPEC.md`：当前有效规范与主题契约
- `IMPLEMENTATION.md`：实现覆盖、当前状态、剩余缺口与 rollout 事实
- `HISTORY.md`：关键演进原因与决策级历史

## 新建 Spec

1. 选择唯一、稳定的 kebab-case topic slug。
2. 新建 `docs/specs/<topic>/` 并一次创建三个 companion documents。
3. 在 Index 表尾追加一行，默认 `Lifecycle=active`、`Successor=-`。

Spec 不使用 ID。`Implementation` 只保留轻量摘要；详细状态写入对应的 `IMPLEMENTATION.md`。

## Lifecycle

- `active`：当前主题 spec 是有效真相源。
- `superseded`：已被 `Successor` 指向的 topic spec 取代。
- `archived`：仅保留历史参考，不再作为当前规范输入。

## Index

| Topic | Lifecycle | Implementation | Spec | Successor | Notes |
| --- | --- | --- | --- | --- | --- |
| Mains Aegis device operation guardrails | active | [current](device-operation-guardrails/IMPLEMENTATION.md) | [SPEC](device-operation-guardrails/SPEC.md) | - | Agent 真机操作的安全边界、devd 路由和授权要求 |
| BQ40 mainboard DF protection baseline | archived | [complete](bq40-mainboard-df-protection-baseline/IMPLEMENTATION.md) | [SPEC](bq40-mainboard-df-protection-baseline/SPEC.md) | - | 冻结 `asset-df-mainboard` 的 `OCC/OCD/SOCC/SOCD` 主板基线，并把 `TMP + BMS` 最高温收敛为共享热控真相源 |
| TPS/BQ power test firmware | archived | [complete](tps-bq-power-test-firmware/IMPLEMENTATION.md) | [SPEC](tps-bq-power-test-firmware/SPEC.md) | - | 已实现独立 `tps-test-fw`、固定 profile 电源运行时、专用 `TPS TEST` 屏显与三组 `cargo +esp check` 验证 |
| UI UX Pro Max skill 存档 | archived | [complete](install-ui-ux-pro-max-skill/IMPLEMENTATION.md) | [SPEC](install-ui-ux-pro-max-skill/SPEC.md) | - | 通用 Skill 不再由项目内置；保留归档决策与边界 |
| BQ40 self-check result dialogs | active | [current](bq40-self-check-result-dialogs/IMPLEMENTATION.md) | [SPEC](bq40-self-check-result-dialogs/SPEC.md) | - | BQ40 自检问题态、显式恢复入口与结果弹窗的当前契约 |
| PCB netlist sync (2026-03-19) | archived | [complete](pcb-netlist-sync-20260319/IMPLEMENTATION.md) | [SPEC](pcb-netlist-sync-20260319/SPEC.md) | - | 主板网表已同步到 2026-03-19 导出版本；前面板导出已确认与仓库零差异 |
| Front panel industrial UI preview（320x172） | superseded | [replaced](front-panel-industrial-ui-preview/IMPLEMENTATION.md) | [SPEC](front-panel-industrial-ui-preview/SPEC.md) | [mcu-self-check-live-panel](mcu-self-check-live-panel/SPEC.md) | 视觉基线保留；dashboard/menu/audio 的 `ACTION / SYSTEM` 预览口径已冻结，运行态试听接线转入 main-firmware-runtime-audio-cues，自检页运行语义仍由 mcu-self-check-live-panel 承载 |
| EEPROM storage layout | active | [current](eeprom-storage-layout/IMPLEMENTATION.md) | [SPEC](eeprom-storage-layout/SPEC.md) | - | 全局 EEPROM map、record 编码、CRC、默认回退与扩展规则的长期规范 |
| Mains Aegis CLI / devd alignment | archived | [complete](mains-aegis-cli-devd-alignment/IMPLEMENTATION.md) | [SPEC](mains-aegis-cli-devd-alignment/SPEC.md) | - | 主机工具对齐与 release/install 基线已完成；CLI `device session` 与新查询面迁移改由 lan-management-convergence 接管 |
| MCU self-check live panel (resident Variant C) | superseded | [replaced](mcu-self-check-live-panel/IMPLEMENTATION.md) | [SPEC](mcu-self-check-live-panel/SPEC.md) | [dashboard-live-after-self-check](dashboard-live-after-self-check/SPEC.md) | 开机自检实时化能力保留，但默认 steady-state 页面已由 dashboard-live-after-self-check 改为真实 Dashboard |
| Standalone display diagnostic firmware | superseded | [replaced](standalone-display-diag-firmware/IMPLEMENTATION.md) | [SPEC](standalone-display-diag-firmware/SPEC.md) | [test-fw-audio-navigation](test-fw-audio-navigation/SPEC.md) | 已被 feature 驱动 `test-fw` 方案替代 |
| WiFi / service discovery / read-only API foundation | active | [current](wifi-service-discovery-api-foundation/IMPLEMENTATION.md) | [SPEC](wifi-service-discovery-api-foundation/SPEC.md) | - | `net_http` 与 `web_serial` 已成为默认主固件能力；“LAN 只读 API” 假设由 lan-management-convergence 继续演进 |
| Regulated output module docs + runtime gate state machine | archived | [complete](regulated-output-module/IMPLEMENTATION.md) | [SPEC](regulated-output-module/SPEC.md) | - | 已建立 `docs/modules/`、收敛稳压输出 SoT，并落地显式恢复状态机与本地验证 |
| Front panel auto sleep | active | [current](front-panel-auto-sleep/IMPLEMENTATION.md) | [SPEC](front-panel-auto-sleep/SPEC.md) | - | 测试版 `30s / 35s / 40s` 自动低亮、关背光、GC9307 sleep；硬件确认后恢复正式默认 `180s / 240s / 245s` |
| BQ40 Cell4 protocol-safe diagnostics | archived | [complete](bq40-cell4-protocol-diagnostics/IMPLEMENTATION.md) | [SPEC](bq40-cell4-protocol-diagnostics/SPEC.md) | - | 已完成协议修正、只读诊断收敛、flash/monitor 互斥与 reply PEC 探测；`Cell4` 根因已排除工具误读路径 |
| BQ25792 500mA charge policy + DC derate | archived | [complete](bq25792-charge-policy/IMPLEMENTATION.md) | [SPEC](bq25792-charge-policy/SPEC.md) | - | 主线 charger state machine 已作为 SoT；DC IN 停充真相源使用 `TPS output current > 100mA`，并新增 BACKUP USB-C `<2W` 自动放行、`>3W`/两次缺样锁存与可观测状态 |
| Dashboard detail drill-down | archived | [complete](dashboard-detail-drilldown/IMPLEMENTATION.md) | [SPEC](dashboard-detail-drilldown/SPEC.md) | - | 首页 5 区点击进入二级仪表盘；`Cells` 新增唯一 `BMS DETAIL` 子页，已补齐高级 BMS 状态 UI、预览图与 spec 视觉证据 |
| Regulated output active derating + shutdown | archived | [complete](regulated-output-active-protection/IMPLEMENTATION.md) | [SPEC](regulated-output-active-protection/SPEC.md) | - | 已落地温度/电流双门限主动降额、低压主动停机与显式恢复前置条件 |
| Dashboard live after self-check | archived | [complete](dashboard-live-after-self-check/IMPLEMENTATION.md) | [SPEC](dashboard-live-after-self-check/SPEC.md) | - | 主固件改为“自检完成后自动进入 Dashboard”，并把 live Dashboard 的市电真相源统一到 `DC5025 VIN>=3V`；同时补齐 `PIN W` 的输入 ADC 样本净化与异常日志，避免 `~1000W` 误显 |
| Main firmware runtime audio cues | archived | [complete](main-firmware-runtime-audio-cues/IMPLEMENTATION.md) | [SPEC](main-firmware-runtime-audio-cues/SPEC.md) | - | 主固件已接入 B. Warm Tap 有效触摸/按键与 USB-C 插入 `ACTION` route 操作音；既有 15 组 `SYSTEM` cue 语义与 runtime DMA underrun 收敛策略保持不变 |
| BQ40 `LOCK` root cause + closure | active | [current](bq40-lock-root-cause/IMPLEMENTATION.md) | [SPEC](bq40-lock-root-cause/SPEC.md) | - | 已命中 `termination` 分流并提交 `ITERM` 对齐修复；下一步需要 `<90%` 解锁后的 live 闭环复验 |
| Front panel visual language systematization | archived | [complete](front-panel-visual-language/IMPLEMENTATION.md) | [SPEC](front-panel-visual-language/SPEC.md) | - | 建立 Token/组件契约/视觉回归清单，补充 bitmap 字体字高白名单与预览图 |
| USB-C PD/PPS sink v1 | archived | [complete](usb-c-pd-sink-pps/IMPLEMENTATION.md) | [SPEC](usb-c-pd-sink-pps/SPEC.md) | - | hotplug PPS 恢复已稳定闭环：reset 基线约 `1.67s` 回到 `PPS`，主人实测真实热插拔也已恢复到秒级协商成功 |
| GitHub Pages docs site handbooks | archived | [complete](docs-site-handbooks/IMPLEMENTATION.md) | [SPEC](docs-site-handbooks/SPEC.md) | - | Pages 根站点改由 Web App 发布，文档站保留为 `/docs/` 子路径；原 `docs-site/`、手册页面与 PR #63 记录仍为历史基线 |
| LAN management convergence | archived | [complete](lan-management-convergence/IMPLEMENTATION.md) | [SPEC](lan-management-convergence/SPEC.md) | - | 设备本体 settings 读写 API、devd LAN discovery/scan trace/LAN settings 写路径、USB 优先合并、trace+connection 查询面、Web direct LAN/devd LAN settings 与最终视觉证据已收口 |
| Status/warning/error speaker cues preview assets | archived | [complete](audio-cues-preview/IMPLEMENTATION.md) | [SPEC](audio-cues-preview/SPEC.md) | - | 15 组提示音试听资产（score + mid + wav）与增强预览页 |
| Front panel display-chain long-press diagnostics | archived | [complete](front-panel-display-chain-diagnostics/IMPLEMENTATION.md) | [SPEC](front-panel-display-chain-diagnostics/SPEC.md) | - | 已完成主固件实现、本地构建、真机 flash/monitor 与 `CENTER` 长按 defmt 取证 |
| BQ40 balance baseline + observability | archived | [complete](bq40-balance-baseline-and-observability/IMPLEMENTATION.md) | [SPEC](bq40-balance-baseline-and-observability/SPEC.md) | - | PR #59 已完成实现/文档/预览收口；实板已确认 DF 对齐，active balancing 触发待后续在完整 charge/relax 条件下复核 |
| Mains Aegis Device Daemon | archived | [complete](mains-aegis-devd/IMPLEMENTATION.md) | [SPEC](mains-aegis-devd/SPEC.md) | - | devd v1 foundation 已完成；`diag-snapshot`、synthetic power event 与 CLI trace follow 已补齐 `TPS output current` 停充根因、`lan_derived` 兼容面与 DC IN `1000mA/96%` 输入限值观测 |
| Owner-facing charge control | active | [current](owner-facing-charge-control/IMPLEMENTATION.md) | [SPEC](owner-facing-charge-control/SPEC.md) | - | 统一 status summary + charge-control detail/preview/action 合同；Power 页改为当前态卡片 + 单弹窗手动充电控制 |
| Client transport priority matrix | archived | [complete](client-transport-priority/IMPLEMENTATION.md) | [SPEC](client-transport-priority/SPEC.md) | - | 跨 Web / devd / CLI 的通信方案优先级已抽成独立 topic spec；冻结 Web confirmed companion 的 FQDN-first 与 devd/CLI 的 USB-first 规则 |
| BQ40 tool reflash / recovery convergence | archived | [complete](bq40-tool-reflash-recovery/IMPLEMENTATION.md) | [SPEC](bq40-tool-reflash-recovery/SPEC.md) | - | 工具链已可区分 ROM 检测/写入/退出与 post-flash 无效运行态；剩余问题收敛为样片硬件状态 |
| Test firmware navigation + audio priority | archived | [complete](test-fw-audio-navigation/IMPLEMENTATION.md) | [SPEC](test-fw-audio-navigation/SPEC.md) | - | `test-fw` 已替换 display-diag；功能导航与音频优先级协调已验收 |
| UPS runtime mode switching | active | [current](runtime-mode-switching/IMPLEMENTATION.md) | [SPEC](runtime-mode-switching/SPEC.md) | - | 统一 `STANDBY / ASSIST / BACKUP` 自动切换、`BYPASS` 边界与 VIN/fallback 真相源；`BACKUP` 默认 `NOAC`，受控 USB-C 低输出充电例外由 bq25792-charge-policy 承接 |
| Front panel refresh pipeline | active | [current](front-panel-refresh-pipeline/IMPLEMENTATION.md) | [SPEC](front-panel-refresh-pipeline/SPEC.md) | - | PR #41 已创建；已完成 PSRAM 双缓冲、dirty-band framebuffer 与 SPI DMA 主路径，等待 review-loop / 40MHz 联调结论回填 |
| Fan control with thermal/tach fail-safe | active | [current](fan-control/IMPLEMENTATION.md) | [SPEC](fan-control/SPEC.md) | - | PR #36 已创建；等待 review-loop 收敛 |
| Web management UI | active | [current](web-management-ui/IMPLEMENTATION.md) | [SPEC](web-management-ui/SPEC.md) | - | Web 管理端 v1 基线已完成；LAN 管理、settings 与 LAN/USB 收敛改由 lan-management-convergence 接管 |
| Manual charge dashboard page + EEPROM prefs | archived | [complete](manual-charge-dashboard/IMPLEMENTATION.md) | [SPEC](manual-charge-dashboard/SPEC.md) | - | `MANUAL` 三级页面仅保存 prefs；Web/API owner-facing 充电控制真相已由 owner-facing-charge-control 接管 |
| 初始化 ESP32-S3（esp-rs / esp-hal）no_std 固件工程 | archived | [complete](esp-rs-no-std-firmware-bootstrap/IMPLEMENTATION.md) | [SPEC](esp-rs-no-std-firmware-bootstrap/SPEC.md) | - | Migrated from the retired planning catalog. |
| 仓库代码质量门槛：Git hooks + GitHub Actions | archived | [complete](quality-gates-ci-hooks/IMPLEMENTATION.md) | [SPEC](quality-gates-ci-hooks/SPEC.md) | - | Migrated from the retired planning catalog. |
| 固件音频播放 + Demo 素材 | archived | [complete](firmware-audio-playback-demo/IMPLEMENTATION.md) | [SPEC](firmware-audio-playback-demo/SPEC.md) | - | Migrated from the retired planning catalog. |
| TPS55288 双路输出控制 | archived | [complete](tps55288-control/IMPLEMENTATION.md) | [SPEC](tps55288-control/SPEC.md) | - | Migrated from the retired planning catalog. |
| TPS 热点温度采样：TMP112A 读数与日志口径 | archived | [complete](tps-tmp112-temperature-reading/IMPLEMENTATION.md) | [SPEC](tps-tmp112-temperature-reading/SPEC.md) | - | Migrated from the retired planning catalog. |
| INA3221 VBUS 读数偏高排查 | active | [pending](ina3221-vbus-offset/IMPLEMENTATION.md) | [SPEC](ina3221-vbus-offset/SPEC.md) | - | Migrated from the retired planning catalog. |
| BQ25792 charging enable + status capture | archived | [complete](bq25792-charging-enable/IMPLEMENTATION.md) | [SPEC](bq25792-charging-enable/SPEC.md) | - | Migrated from the retired planning catalog. |
| BQ40Z50 BMS bring-up (SMBus poll + fault expectations) | archived | [complete](bq40z50-bms-bringup/IMPLEMENTATION.md) | [SPEC](bq40z50-bms-bringup/SPEC.md) | - | Migrated from the retired planning catalog. |
| TMP112A 过温告警输出：Comparator 模式保持输出 | archived | [complete](tps-tmp112-alert-overtemp-hold/IMPLEMENTATION.md) | [SPEC](tps-tmp112-alert-overtemp-hold/SPEC.md) | - | Migrated from the retired planning catalog. |
