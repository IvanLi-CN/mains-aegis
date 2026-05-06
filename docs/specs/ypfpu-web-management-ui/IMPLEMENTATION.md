# Web management UI Implementation（#ypfpu）

## 当前状态

- `web/` 新增独立 Vite + React + TypeScript + Bun 应用。
- 根 `package.json` 增加 workspace 与 `web:dev` / `web:preview` / `web:check` / `web:build` 脚本。
- `DeviceRegistry` 维护浏览器侧设备清单、localStorage 持久化、LAN 只读探活、SSE 订阅与轮询兜底，并持有当前浏览器 session 内的 USB CDC `SerialPort`。
- USB CDC / Web Serial 设备使用 `serial:` target，不持久化真实 `SerialPort`；刷新后需要重新授权。
- `web/src/serial/transport.ts` 实现 JSONL framing、`request_id` response matching、握手、状态读取、WiFi 配网、日志级别与手动充电偏好命令。
- 固件新增 `usb_cdc_protocol` host-testable 协议模块，定义 `hello/status/log/request/response/error/wifi_config` frame、WiFi secret validation、PSK redaction 与 128B EEPROM WiFi config record CRC。
- 主固件默认启用 `web_serial + net_http`，使用 ESP32-S3 USB Serial/JTAG CDC 通道读取 JSONL 命令，返回 identity/status/ack/error/log frame，并在 `get_status` 上生成 `status` / `output` / `charger` / `battery` / `network` 结构化日志；WiFi config 写入 EEPROM `0x0160` 起始的 4 个 32B block，`set` 后运行时立即连接，`clear` 后清空 EEPROM slot，并让 WiFi task 以 250ms 周期观察配置 generation，立即标记 `network.state=disabled` 后执行 disconnect/stop。
- `mock:` 设备用于稳定开发预览和视觉证据，不发真实网络请求。
- 管理端页面已覆盖 Fleet、Connect、Overview、Power、Battery、Thermal、Device、Settings、API。
- Settings 页仅对 USB CDC 或 devd 连接设备开放，提供 WiFi SSID/PSK 覆盖/清除、手动充电偏好、USB session 日志级别和 USB Console；USB Console 保留当前 Web Serial 或 devd session 的 tx/rx frame、raw / ignored CDC 行和协议 payload，支持等级过滤、方向过滤、搜索高亮、虚拟滚动、全屏查看与 payload 折行开关，PSK 脱敏。
- Web App 已移除独立 USB HTTP bridge 分支，devd 成为 localhost USB 控制面；同一 `identity.device_id` 的 LAN 与 USB 来源合并为一条设备记录，并在 Fleet / Connect 中显示 WiFi 与 USB 标记。
- devd Web USB control lease 已落地：多候选设备必须由用户选择，Web session 创建 lease 后 heartbeat 续租，断开/移除/页面卸载时释放，TTL 到期自动释放，safe settings 与 serial session 均要求有效 lease。
- Web Serial 与 devd 连接路径在读取 USB identity 后都会匹配 firmware artifact catalog。未命中时返回 `firmware_artifact_mismatch` 气泡并阻断可写 session；devd 路径会释放刚创建的 lease，用户点击显式忽略按钮后才重新发起连接。
- USB Console 保留 raw/ignored 串口记录本身，不再额外显示 `Decode issue` 或 `defmt decoder unavailable` 诊断标签；连接时的 firmware artifact 匹配门禁负责阻断不匹配固件。
- Settings 和 Connect 失败反馈统一为气泡 callout；WiFi 保存、WiFi 清除和 manual charge 写入在固件/devd 返回前显示 spinner 并禁用并发写入。
- Fleet 卡片使用用户可理解的摘要字段，技术细节保留到单设备详情与 API 调试页。
- Demo 复用正式前端路由，通过 `seed` 参数切换 mock 数据场景，覆盖默认 fleet、空数据、全离线、大数量、Critical Battery、Backup、API Debug 等路径。

## 验证状态

- `bun install`: 已通过。
- `bun run web:check`: 已通过。
- `bun run web:build`: 已通过。
- `cargo test --manifest-path firmware/host-unit-tests/Cargo.toml usb_cdc_protocol`: 已通过。
- `cd firmware && cargo +esp check`: 已通过。
- `cd firmware && cargo +esp check --no-default-features`: 已通过。
- Storybook：已使用最新 `storybook` / `@storybook/react-vite` 10.3.6 建立 `UPS Management/Settings/WiFi Provisioning Feedback` 状态矩阵，覆盖连接失败、保存失败、清除失败、保存中、清除中与成功反馈；`UPS Management/Connect/Firmware mismatch warning` 覆盖连接前 firmware artifact 不匹配与显式忽略入口。
- 本地预览：已通过端口租约启动 Vite mock-data 前端。
- 浏览器验证：已确认 Fleet、Connect 和单设备 Dashboard 可渲染，控制台无 warn/error。
- 视觉证据：已生成 desktop Fleet、mobile Fleet、empty Fleet、large Fleet、single-device Dashboard 的 mock UI 截图；截图已回传给主人，并作为 spec assets 落盘供 owner-facing review 使用。
- Review-loop：已通过，未发现剩余可操作问题。
- PR #71 CI：当前分支推送后以 GitHub checks 最新结果为准。

## 当前缺口

- 多 USB CDC candidates 场景需要在 `/connect` 显示选择器，不能自动选择已连接或已识别设备。
- devd 控制 session 需要短 TTL lease；正常关闭立即释放，异常断开默认 8-9 秒内释放。
- WiFi config 与 safe settings 需要携带有效 lease，避免 Web 不存在时 devd 继续占用或写入硬件。

## PR 状态

- PR: https://github.com/IvanLi-CN/mains-aegis/pull/71
- Stop condition: Step 5C Ready，等待主人确认后再合并。
