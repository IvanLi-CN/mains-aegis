# 主动告警逐实例消音 实现状态

> 当前有效合同以 [`SPEC.md`](./SPEC.md) 为准。

## Current Status

- Lifecycle: active
- 当前覆盖：前面板评审 scene、场景化 preview CLI 和跨端合同已建立。
- Owner gate：前面板所有状态矩阵必须在 Chat 展示并获得明确批准。
- 未开始：固件告警实例状态机、音频队列抑制、CDC/LAN/devd、CLI、Web App、前面板运行时输入路由。

## 已实现的评审面

- `firmware/src/front_panel_scene.rs`
  - 定义 9 类 `AlertPreviewKind`、严重度、声音状态与 preview item。
  - 提供同源 Dashboard 指示器、`ALERTS` 列表和详情 `CLEARED` 终态 scene。
  - 未改变现有 `DashboardRoute`、触摸命中或任何运行时告警行为。
- `tools/front-panel-preview/src/main.rs`
  - 提供 `dashboard-alert`、`alert-list`、`alert-detail` 场景和参数化矩阵入口。
  - 每次导出保持 `320x172` PNG 与 `110080` 字节 RGB565 framebuffer。

## 后续接入顺序

1. 展示并批准全部前面板快照。
2. 固件实现权威告警实例状态机与 cue 级抑制。
3. 接入 CDC/LAN、devd bridge 和 JSON CLI。
4. 接入前面板运行时路由与 Web `Alerts` 页。
5. 运行固件、host、Web 验证，生成最终视觉证据并同步本 topic spec。

## 验证记录

- `cargo test --manifest-path tools/front-panel-preview/Cargo.toml`
  - 112 passed。
- `cargo fmt --manifest-path tools/front-panel-preview/Cargo.toml --check`
  - 通过。
