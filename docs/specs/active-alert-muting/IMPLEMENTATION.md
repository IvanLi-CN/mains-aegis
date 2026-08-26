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
  - CDC、LAN 与前面板消音均立即停止目标 cue，清空已写入的 I2S/DMA 环形缓冲并发布同一份权威快照；请求携带当前 `instance_id`，不会误消音复发实例。
  - malformed Alerts mute 路径在解析阶段返回结构化 `400`，不会因缺失或嵌套 `alert_id` 触发路径切片异常。
  - 市电缺失告警活动期间，VIN 遥测 unknown 保持当前实例与消音状态；只有明确恢复市电才解除。
  - 告警实例更新独立于 I2S/DMA 可用性；音频初始化失败时仍继续发布和显示活动告警。
  - 前面板、USB CDC 和 LAN 命令均修改同一份 `ActiveAlerts`。
- `firmware/src/usb_cdc_protocol.rs`、`firmware/src/net.rs`
  - 实现 `get_alerts`、`mute_alert`、`GET /api/v1/alerts` 与实例绑定的 mute POST。
  - LAN 对 stale/inactive 返回 `409`；CDC 使用结构化 error frame，并在 `details` 中保留同一结果体。
- `tools/mains-aegis-host`
  - devd 提供设备 Alerts HTTP/IPC bridge，按 native serial、LAN 与 mock 路由。
  - devd 对所有传输统一保留 stale/inactive 的 `409` 状态与结构化详情。
  - 仅在 Alerts 调用点把旧固件 CDC `unsupported_operation` 与 Alerts LAN `404` 归一化为 `unsupported`/`501`，不改变其它 CDC 操作的兼容 fallback，也不从遥测推断写能力。
  - Alerts list/mute IPC 均把 conflict/unsupported 保留为机器可读 result；mock 重复消音返回 `already_muted`。
  - CLI 提供 `alerts list|mute`；mute 先读取当前实例再写回，预读时已解除的告警也输出机器可读 inactive JSON。
- `web/src/app/App.tsx`
  - 设备导航新增 `Alerts` 页，支持 direct HTTP、devd/Web lease 与 Web Serial。
  - 每行独立消音、写入中锁定、完成后权威回读，并呈现 unsupported、stale 与传输错误。
  - direct LAN 的旧固件 Alerts `404` 在客户端限定归一化为 `unsupported`，升级提示与 devd/Web Serial 一致。
  - offline 或 unsupported 时清空旧告警快照；瞬时刷新失败时保留最后确认的活动告警作为持续风险提示，但禁用基于过期实例的消音按钮，直到权威回读恢复。
  - 所有在线设备的告警合同每 2 秒自动回读，页面重新可见时立即回读；每个设备同一时间只允许一个回读请求，HTTP/devd 请求有 1.5 秒超时；Fleet 页面展示 fleet `Critical` / `Warning` 指标，当前设备快照只驱动对应设备的 Alerts 页面。
  - Web Serial 保留 CDC error envelope；刷新使用代次保护，mute 冲突后的权威回读不清除 stale/inactive 提示。
  - 告警控制优先使用当前已确认的活动传输，再按可重试错误回退到 devd、LAN 主地址、LAN fallback 地址或 Web Serial；非重试错误不会伪装成另一种传输状态。
  - `system_silent` 与 `policy_silent` 告警仍可写入当前实例的用户消音状态；内置 mock USB 记录使用 mock Alerts transport 验证同一流程。

- `firmware/src/front_panel_scene.rs`
  - 提供同源 Dashboard 指示器、`ALERTS` 列表、详情 `CLEARED` 终态与热区。
  - Dashboard、列表与详情热区由公开 `TouchRect` 常量定义；运行时和 preview overlay 复用相同 hit-test，host 测试锁定面积、边界、互斥和层级优先级。
- `firmware/src/front_panel_logic.rs`
  - 固化 CST816D `LandscapeSwapped` 坐标边界与顶部 WiFi/Alerts 的按下或滑入触发策略；显示区外坐标拒绝，同一热区内移动不重复触发。
- `firmware/src/front_panel.rs`
  - 将获批 scene 接入触摸和 `UP/DOWN/CENTER/RIGHT/LEFT` 导航。
  - Dashboard 可听告警在无输入时仍按获批双相帧持续刷新；列表滚动窗口与详情/底部整行消音均绑定当前实例。
  - 详情缓存当前实例；解除后保留不可操作的 `CLEARED` 终态。
  - 告警屏幕拦截 Dashboard 垂直手势，避免列表/详情滑动改变隐藏页面；最高严重度与任一可听实例的聚合规则由 scene helper 和 host 测试共同锁定。
- `tools/front-panel-preview/src/main.rs`
  - 提供 `dashboard-alert`、`alert-list`、`alert-detail` 场景和参数化矩阵入口。
  - 每次导出保持 `320x172` PNG 与 `110080` 字节 RGB565 framebuffer。
- `web/public/firmware`
  - 内置固件目录从 `258eb996-clean-b48d7000fbe16ae9` clean release artifact 刷新，保留精确 artifact identity，确保浏览器选择的镜像与已验证的固件实现一致。

## 验证记录

- `cargo test --manifest-path tools/front-panel-preview/Cargo.toml`
  - 117 passed。
- `cargo fmt --manifest-path tools/front-panel-preview/Cargo.toml --check`
  - 通过。
- `just firmware-host-test`
  - 518 passed。
- `just firmware-check`
  - 通过。
- `just host-test`
  - 124 library tests 与 56 CLI tests passed。
- `bun test web/src`
  - 105 passed。
- `just web-check`、`just web-build`
  - 通过。
- `just check`
  - 通过。
- 前面板矩阵
  - 46 个场景；每个 `framebuffer.bin` 均为 `110080` 字节、PNG 为 `320x172`，manifest 记录 renderer 参数、源 revision 与 SHA-256，并生成 7 张 review sheet。
- release artifact
  - Web 与 `firmware/artifacts` catalog 均指向 `258eb996` clean build；ELF 与 Web Serial image 的 SHA-256 记录在对应 manifest 和 `SHA256SUMS`。
