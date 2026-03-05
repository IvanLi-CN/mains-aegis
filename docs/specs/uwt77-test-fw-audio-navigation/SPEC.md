# 功能验证测试固件（test-fw）与音频优先级协调（#uwt77）

## 状态

- Status: 已完成
- Created: 2026-03-05
- Last: 2026-03-05

## 背景 / 问题陈述

- 现有独立屏幕诊断固件 `display-diag-fw` 仅覆盖静态显示诊断，不支持多测试功能导航。
- 测试固件需要按 feature 组合裁剪功能，并支持默认测试直达或导航页进入。
- 新增音频测试后，需要统一事件优先级，确保错误与告警音效能及时抢占低优先级提示音。

## 目标 / 非目标

### Goals

- 将 `display-diag-fw` 重命名并升级为 `test-fw`，统一承载功能验证测试。
- 支持两个测试功能：`屏幕静态显示测试`、`音频播放测试`。
- 通过 feature 决定功能集，并通过 feature 指定默认测试入口。
- 在多功能场景提供导航页；导航支持五向开关与触摸两种输入。
- 测试页面必须始终显示返回控件；无导航页时显示禁用态。
- 实现音频事件优先级协调：`Error > Warning > ModeSwitch > Interaction > Boot`。

### Non-goals

- 不修改主业务固件 (`esp-firmware`) 的电源管理、自检与运行语义。
- 不引入完整音效资源管理平台（在线音源、混音器、配置持久化）。
- 不改硬件引脚和板级电路设计。

## 范围（Scope）

### In scope

- `firmware/src/bin/test-fw.rs`：测试固件主入口（替换旧 `display-diag-fw`）。
- `firmware/src/test_harness.rs`：测试路由、feature 解析与输入导航状态机。
- `firmware/src/test_audio.rs`：音频优先级队列与抢占逻辑。
- `firmware/src/front_panel_scene.rs`：新增测试导航页/测试页/返回控件渲染入口。
- `firmware/display-test/mcu-agentd.toml`：独立测试固件烧录配置。
- `firmware/Cargo.toml`：新增测试功能与默认测试 feature 门禁。
- `firmware/README.md`：新增 `test-fw` 构建、feature 组合、烧录与验证说明。

### Out of scope

- `firmware/src/main.rs` 主固件启动流程行为变更。
- 新增第三个及以上测试功能。

## 接口变更（Interfaces）

- 新增二进制目标：`test-fw`。
- 删除二进制目标：`display-diag-fw`。
- 新增 feature：
  - `test-fw-screen-static`
  - `test-fw-audio-playback`
  - `test-fw-default-screen-static`
  - `test-fw-default-audio-playback`
- 新增测试路由类型：
  - `TestFunction`
  - `TestRoute`
  - `TestHarnessConfig`
- 新增音频优先级类型与接口：
  - `AudioEvent`
  - `AudioPriority`
  - `AudioRequest`
  - `AudioManager::{new, request, tick, stop, status}`
- 新增 UI 渲染入口：
  - `render_test_navigation(...)`
  - `render_test_screen_static(...)`
  - `render_test_audio_playback(...)`
  - `render_test_back_button(..., enabled: bool)`

## 验收标准（Acceptance Criteria）

- 构建正例：
  - `cargo build --release --bin test-fw --features test-fw-screen-static`
  - `cargo build --release --bin test-fw --features test-fw-audio-playback`
  - `cargo build --release --bin test-fw --features "test-fw-screen-static test-fw-audio-playback"`
  - `cargo build --release --bin test-fw --features "test-fw-screen-static test-fw-audio-playback test-fw-default-screen-static"`
  - `cargo build --release --bin test-fw --features "test-fw-screen-static test-fw-audio-playback test-fw-default-audio-playback"`
- 构建负例（必须失败）：
  - 同时启用 `test-fw-default-screen-static` 与 `test-fw-default-audio-playback`。
  - 启用 `test-fw-default-screen-static` 但未启用 `test-fw-screen-static`。
  - 启用 `test-fw-default-audio-playback` 但未启用 `test-fw-audio-playback`。
  - 未启用任何测试功能 feature。
- 运行行为：
  - 单功能：上电直接进入该测试页，返回控件可见且禁用。
  - 多功能无默认：上电进入导航页；五向与触摸均可切换并进入测试页。
  - 多功能有默认：上电进入默认测试页；返回后进入导航页。
  - 音频优先级满足抢占规则与同级 FIFO 顺序。
- 回归保护：
  - `cargo build --release --bin esp-firmware` 通过。

## 里程碑（Milestones）

- [x] M1: test-fw 入口重命名与独立烧录配置迁移完成。
- [x] M2: feature 矩阵与编译期门禁完成。
- [x] M3: 导航状态机 + UI 页面 + 返回控件语义完成。
- [x] M4: 音频优先级协调模块接入完成。
- [x] M5: 文档与构建矩阵验证完成。

## 实现结果

- 二进制入口完成替换：`display-diag-fw` 已移除，新增 `test-fw`。
- feature 矩阵落地：功能 feature + 默认测试 feature + 编译期冲突/不匹配报错。
- 导航状态机落地：单功能直达、多功能导航、默认测试直达后可返回导航。
- UI 落地：导航页/屏幕静态测试页/音频测试页；返回控件始终显示，无导航时禁用。
- 音频协调落地：高优先级抢占低优先级；队列按优先级出队、同级 FIFO。

## 验证记录

- 正例构建通过：
  - `cargo build --release --bin test-fw --features test-fw-screen-static`
  - `cargo build --release --bin test-fw --features test-fw-audio-playback`
  - `cargo build --release --bin test-fw --features "test-fw-screen-static test-fw-audio-playback"`
  - `cargo build --release --bin test-fw --features "test-fw-screen-static test-fw-audio-playback test-fw-default-screen-static"`
  - `cargo build --release --bin test-fw --features "test-fw-screen-static test-fw-audio-playback test-fw-default-audio-playback"`
  - `cargo build --release --bin esp-firmware`
- 负例构建按预期失败：
  - `cargo build --release --bin test-fw`
  - `cargo build --release --bin test-fw --features "test-fw-screen-static test-fw-audio-playback test-fw-default-screen-static test-fw-default-audio-playback"`
  - `cargo build --release --bin test-fw --features "test-fw-audio-playback test-fw-default-screen-static"`
  - `cargo build --release --bin test-fw --features "test-fw-screen-static test-fw-default-audio-playback"`
  - `cargo build --release --bin test-fw --features test-fw`

## 关联规格

- `docs/specs/958aj-standalone-display-diag-firmware/SPEC.md`
- `docs/specs/7n4qd-mcu-self-check-live-panel/SPEC.md`
