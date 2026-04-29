# Web management UI Implementation（#ypfpu）

## 当前状态

- `web/` 新增独立 Vite + React + TypeScript + Bun 应用。
- 根 `package.json` 增加 workspace 与 `web:dev` / `web:preview` / `web:check` / `web:build` 脚本。
- `DeviceRegistry` 维护浏览器侧设备清单、localStorage 持久化、LAN 只读探活、SSE 订阅与轮询兜底，并持有当前浏览器 session 内的 USB CDC `SerialPort`。
- USB CDC / Web Serial 设备使用 `serial:` target，不持久化真实 `SerialPort`；刷新后需要重新授权。
- `web/src/serial/transport.ts` 实现 JSONL framing、`request_id` response matching、握手、状态读取、WiFi 配网、日志级别与手动充电偏好命令。
- 固件新增 `usb_cdc_protocol` host-testable 协议模块，定义 `hello/status/log/request/response/error/wifi_config` frame、WiFi secret validation、PSK redaction 与 128B EEPROM WiFi config record CRC。
- 主固件新增 `web_serial` feature，使用 ESP32-S3 USB Serial/JTAG CDC 通道读取 JSONL 命令，返回 identity/status/ack/error/log frame，并在 `get_status` 上生成 `status` / `output` / `charger` / `battery` / `network` 结构化日志；WiFi config 写入 EEPROM `0x0160` 起始的 4 个 32B block，`net_http` 启用时优先加载该记录，并在 USB 写入后更新运行时 WiFi 配置。
- `mock:` 设备用于稳定开发预览和视觉证据，不发真实网络请求。
- 管理端页面已覆盖 Fleet、Connect、Overview、Power、Battery、Thermal、Device、Settings、API。
- Settings 页仅对 USB CDC 连接设备开放，提供 WiFi SSID/PSK 覆盖/清除、手动充电偏好、USB session 日志级别、structured log 面板和 USB Developer Console；Developer Console 保留当前 Web Serial session 的 tx/rx frame、raw / ignored CDC 行和协议 payload，支持全屏查看与 payload 折行开关，PSK 脱敏。
- Fleet 卡片使用用户可理解的摘要字段，技术细节保留到单设备详情与 API 调试页。
- Demo 复用正式前端路由，通过 `seed` 参数切换 mock 数据场景，覆盖默认 fleet、空数据、全离线、大数量、Critical Battery、Backup、API Debug 等路径。

## 验证状态

- `bun install`: 已通过。
- `bun run web:check`: 已通过。
- `bun run web:build`: 已通过。
- `cargo test --manifest-path firmware/host-unit-tests/Cargo.toml usb_cdc_protocol`: 已通过。
- `cd firmware && cargo +esp check --features web_serial`: 已通过。
- `cd firmware && MAINS_AEGIS_WIFI_SSID=LabNet MAINS_AEGIS_WIFI_PSK=correct-horse cargo +esp check --features web_serial,net_http`: 已通过。
- Storybook：已从 Demo 工作流移除。
- 本地预览：已通过端口租约启动 Vite mock-data 前端。
- 浏览器验证：已确认 Fleet、Connect 和单设备 Dashboard 可渲染，控制台无 warn/error。
- 视觉证据：已生成 desktop Fleet、mobile Fleet、empty Fleet、large Fleet、single-device Dashboard 的 mock UI 截图；截图已回传给主人，并作为 spec assets 落盘供 owner-facing review 使用。
- Review-loop：已通过，未发现剩余可操作问题。
- PR #71 CI：当前分支推送后以 GitHub checks 最新结果为准。

## PR 状态

- PR: https://github.com/IvanLi-CN/mains-aegis/pull/71
- Stop condition: Step 5C Ready，等待主人确认后再合并。
