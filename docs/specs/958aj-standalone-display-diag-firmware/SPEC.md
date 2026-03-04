# 独立屏幕诊断固件（颜色/方向/镜像）(#958aj)

## 状态

- Status: 已完成
- Created: 2026-03-05
- Last: 2026-03-05

## 背景 / 问题陈述

- 前面板颜色存在通道错位风险（RGB/BGR），需要现场快速判定与回归。
- 仅靠主业务固件难以稳定排查“颜色映射 / 旋转方向 / 镜像”问题。
- 需要一套可反复烧录、与主业务逻辑隔离的专用诊断固件。

## 目标 / 非目标

### Goals

- 提供独立二进制 `display-diag-fw`，不依赖 feature 切换主固件行为。
- 在屏幕上固定渲染诊断锚点，覆盖颜色、方向、镜像三类问题。
- 保持主业务固件入口与行为不变。
- 提供独立 `mcu-agentd` 诊断项目配置，支持稳定烧录 `esp-diag`。

### Non-goals

- 不改电源管理、自检门控、充放电策略。
- 不新增业务页面或触摸交互流程。
- 不调整前面板硬件连线与初始化时序。

## 范围（Scope）

### In scope

- `firmware/src/bin/display-diag-fw.rs`
  - 新增独立诊断固件入口，仅初始化前面板显示链路并循环渲染诊断页。
- `firmware/src/front_panel.rs`
  - 暴露诊断渲染入口与就绪状态查询。
  - 将面板颜色顺序参数抽为常量并用于驱动配置。
  - 统一 I2C 错误日志映射，避免依赖 `output` 模块。
- `firmware/src/front_panel_scene.rs`
  - 新增独立诊断画面渲染：方向箭头、四角色块、RGBYCMWK 色条、8 级灰阶、心跳块。
- `firmware/display-diag/mcu-agentd.toml`
  - 新增独立诊断项目配置（`project.id=esp_diag_fw`，`mcu_id=esp-diag`）。
- `firmware/README.md`
  - 新增独立诊断固件构建、烧录、拍照复核口径。
- `.gitignore`
  - 忽略 `firmware/display-diag/.mcu-agentd/` 运行态目录。

### Out of scope

- 修改 `firmware/src/main.rs` 主固件启动路径。
- 修改 `docs/specs/7n4qd-*` 既有验收口径。

## 接口变更（Interfaces）

- 新增二进制入口：`display-diag-fw`。
- 新增诊断渲染接口：
  - `front_panel_scene::render_display_diagnostic(...)`
  - `FrontPanel::render_display_diagnostic(...)`
  - `FrontPanel::is_ready()`

## 验收标准（Acceptance Criteria）

- 构建通过：
  - `cargo build --release --bin display-diag-fw`
  - `cargo build --release --bin esp-firmware`
- 独立烧录通过：
  - 在 `firmware/display-diag/` 下执行 `mcu-agentd flash esp-diag` 成功。
- 串口日志包含：
  - `diag: front-panel display probe boot`
  - `diag: rendering display diagnostic screen`
- 实屏诊断锚点正确：
  - `UP ^` 朝上；
  - `TL=R, TR=G, BL=B, BR=Y`；
  - 色条顺序 `R G B Y C M W K`；
  - 灰阶由黑到白单调递增；
  - 心跳块按固定周期闪烁。

## 里程碑（Milestones）

- [x] M1: 独立诊断固件入口创建并可构建。
- [x] M2: 诊断画面渲染能力完成并接入固件。
- [x] M3: 现场拍照闭环定位颜色通道问题并修复。
- [x] M4: 独立烧录配置与文档同步完成。

## 关联规格

- `docs/specs/7n4qd-mcu-self-check-live-panel/SPEC.md`

## 变更记录（Change log）

- 2026-03-05: 新增独立诊断固件与独立 `mcu-agentd` 配置；现场照片确认并修复颜色顺序为 `BGR565`。
