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
| edbpk | BQ40 Cell4 protocol-safe diagnostics | 已完成 | `edbpk-bq40-cell4-protocol-diagnostics/SPEC.md` | 2026-03-15 | 已完成协议修正、只读诊断收敛、flash/monitor 互斥与 reply PEC 探测；`Cell4` 根因已排除工具误读路径 |
| tmdtq | BQ40 tool reflash / recovery convergence | 已完成 | `tmdtq-bq40-tool-reflash-recovery/SPEC.md` | 2026-03-11 | 工具链已可区分 ROM 检测/写入/退出与 post-flash 无效运行态；剩余问题收敛为样片硬件状态 |
| g2kte | Dashboard live after self-check | 已完成 | `g2kte-dashboard-live-after-self-check/SPEC.md` | 2026-03-15 | 主固件改为“自检完成后自动进入 Dashboard”，并补齐 `PIN W` 的输入 ADC 样本净化与异常日志，避免 `~1000W` 误显 |
| 4t9wx | Install UI UX Pro Max skill（Codex） | 已完成 | `4t9wx-install-ui-ux-pro-max-skill/SPEC.md` | 2026-03-02 | 项目内安装并入库；修正 `.codex` 脚本路径与 pycache 忽略 |
| 6qrjs | Front panel industrial UI preview（320x172） | 重新设计（#7n4qd） | `6qrjs-front-panel-industrial-ui-preview/SPEC.md` | 2026-03-01 | 视觉基线保留；自检页运行语义迁移到 #7n4qd |
| 7n4qd | MCU self-check live panel (resident Variant C) | 重新设计（#g2kte） | `7n4qd-mcu-self-check-live-panel/SPEC.md` | 2026-03-15 | 开机自检实时化能力保留，但默认 steady-state 页面已由 #g2kte 改为真实 Dashboard |
| 958aj | Standalone display diagnostic firmware | 重新设计（#uwt77） | `958aj-standalone-display-diag-firmware/SPEC.md` | 2026-03-05 | 已被 feature 驱动 `test-fw` 方案替代 |
| uwt77 | Test firmware navigation + audio priority | 已完成 | `uwt77-test-fw-audio-navigation/SPEC.md` | 2026-03-05 | `test-fw` 已替换 display-diag；功能导航与音频优先级协调已验收 |
| h43mk | Main firmware runtime audio cues | 已完成 | `h43mk-main-firmware-runtime-audio-cues/SPEC.md` | 2026-03-14 | 主固件已切换到运行时 cue 服务；补齐 DMA / 激活后音频 hotfix，并把 BatteryProtection 固定为高于所有低电提示音的全局规则 |
| hg3dw | Front panel visual language systematization | 已完成 | `hg3dw-front-panel-visual-language/SPEC.md` | 2026-03-02 | 建立 Token/组件契约/视觉回归清单，补充 bitmap 字体字高白名单与预览图 |
| mecb8 | Status/warning/error speaker cues preview assets | 已完成 | `mecb8-audio-cues-preview/SPEC.md` | 2026-03-05 | 15 组提示音试听资产（score + mid + wav）与增强预览页 |
| xy6cz | Front panel refresh pipeline | 部分完成（4/5） | `xy6cz-front-panel-refresh-pipeline/SPEC.md` | 2026-03-15 | PR #41 已创建；已完成 PSRAM 双缓冲、dirty-band framebuffer 与 SPI DMA 主路径，等待 review-loop / 40MHz 联调结论回填 |
| ygmqn | Fan control with thermal/tach fail-safe | 部分完成（4/5） | `ygmqn-fan-control/SPEC.md` | 2026-03-13 | PR #36 已创建；等待 review-loop 收敛 |
