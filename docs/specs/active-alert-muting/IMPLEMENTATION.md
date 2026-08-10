# 主动告警逐实例消音 实现状态

> 当前有效合同以 [`SPEC.md`](./SPEC.md) 为准。

## Current Status

- Lifecycle: active
- 当前覆盖：固件权威实例状态机、cue 级消音、CDC/LAN、devd/CLI、Web App 与前面板运行时路由。
- Owner gate：前面板完整 framebuffer 矩阵已获主人批准，运行时接入遵循获批 scene 与热区。
- 交付状态：实现、验证与最终视觉证据已收敛。

## 实现结构

- `firmware/src/active_alerts.rs`
  - 固定 9 槽告警目录，在 RAM 中维护启动期单调 `instance_id`、活动态、用户消音与策略静默。
  - `mute` 严格匹配当前实例，返回 `muted`、`already_muted`、`stale` 或 `inactive`。
  - 统一输出 `severity` 与 `sound_state`，并映射到既有 `AudioCue`。
- `firmware/src/main.rs`、`firmware/src/audio.rs`
  - 每轮从现有运行期信号更新实例集合，再按有效声音状态启停单个 cue。
  - 前面板、USB CDC 和 LAN 命令均修改同一份 `ActiveAlerts`。
- `firmware/src/usb_cdc_protocol.rs`、`firmware/src/net.rs`
  - 实现 `get_alerts`、`mute_alert`、`GET /api/v1/alerts` 与实例绑定的 mute POST。
  - LAN 对 stale/inactive 返回 `409`，CDC 保留结构化结果。
- `tools/mains-aegis-host`
  - devd 提供设备 Alerts HTTP/IPC bridge，按 native serial、LAN 与 mock 路由。
  - CLI 提供 `alerts list|mute`；mute 先读取当前实例再写回。
- `web/src/app/App.tsx`
  - 设备导航新增 `Alerts` 页，支持 direct HTTP、devd/Web lease 与 Web Serial。
  - 每行独立消音、写入中锁定、完成后权威回读，并呈现 unsupported、stale 与传输错误。

- `firmware/src/front_panel_scene.rs`
  - 提供同源 Dashboard 指示器、`ALERTS` 列表、详情 `CLEARED` 终态与热区。
- `firmware/src/front_panel.rs`
  - 将获批 scene 接入触摸和 `UP/DOWN/CENTER/RIGHT/LEFT` 导航。
  - 详情缓存当前实例；解除后保留不可操作的 `CLEARED` 终态。
- `tools/front-panel-preview/src/main.rs`
  - 提供 `dashboard-alert`、`alert-list`、`alert-detail` 场景和参数化矩阵入口。
  - 每次导出保持 `320x172` PNG 与 `110080` 字节 RGB565 framebuffer。

## 验证记录

- `cargo test --manifest-path tools/front-panel-preview/Cargo.toml`
  - 112 passed。
- `cargo fmt --manifest-path tools/front-panel-preview/Cargo.toml --check`
  - 通过。
- `just firmware-host-test`
  - 506 passed。
- `just firmware-check`
  - 通过。
- `just host-test`
  - 119 library tests 与 54 CLI tests passed。
- `bun test web/src`
  - 95 passed。
- `just web-check`、`just web-build`
  - 通过。
- `just check`
  - 通过。
- 前面板矩阵
  - 46 个场景；每个 `framebuffer.bin` 均为 `110080` 字节，并生成 7 张 review sheet。
