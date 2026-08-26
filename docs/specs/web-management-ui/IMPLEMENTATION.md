# Web management UI Implementation（web-management-ui）

## 当前状态

- `web/` 新增独立 Vite + React + TypeScript + Bun 应用。
- 根 `package.json` 增加 workspace 与 `web:dev` / `web:preview` / `web:check` / `web:build` 脚本。
- Web App 已接入 `vite-plugin-pwa`，生产构建生成 `manifest.webmanifest`、`sw.js`、PWA metadata 和 192/512 PNG maskable icons；`PAGES_BASE=./` 使用相对 `start_url` / `scope`，显式子路径部署保留对应 base。
- PWA service worker 预缓存 app shell、Vite 构建产物、Pages fallback、PWA 图标、相对 base 深链 navigation helper 和 bundled static firmware artifacts；真实设备 `/api`、`/events`、LAN HTTP/SSE、USB/Web Serial 与 GitHub Release live catalog 不进入离线模拟缓存。
- PWA 更新策略使用 prompt 模式：新 app shell 在后台安装并缓存完成后，Web 只显示非阻塞 `New version available` 提示；用户点击 `Update` 并通过确认对话后才调用 `updateSW(true)` 切换并刷新页面。
- `DeviceRegistry` 维护浏览器侧设备清单、localStorage 持久化、LAN 探活、settings 读取、SSE 订阅与轮询兜底，并持有当前浏览器连接内的 USB CDC `SerialPort`。
- USB CDC / Web Serial 设备使用 `serial:` target，不持久化真实 `SerialPort`；刷新后需要重新授权。
- `web/src/serial/transport.ts` 实现 JSONL framing、`request_id` response matching、握手、状态读取、WiFi 配网、日志级别与手动充电偏好命令。
- `web/src/firmware/` 负责 firmware catalog 合并、Bundled 优先去重、Web Serial 烧录 helper 和 Firmware 页面数据流。
- 固件新增 `usb_cdc_protocol` host-testable 协议模块，定义 `hello/status/log/request/response/error/wifi_config` frame、WiFi secret validation、PSK redaction 与 128B EEPROM WiFi config record CRC。
- 主固件默认启用 `web_serial + net_http`，使用 ESP32-S3 USB Serial/JTAG CDC 通道读取 JSONL 命令，返回 identity/status/ack/error/log frame，并在 `get_status` 上生成 `status` / `output` / `charger` / `battery` / `network` 结构化日志；WiFi config 写入 EEPROM `0x0160` 起始的 4 个 32B block，`set` 后运行时立即连接，`clear` 后清空 EEPROM slot，并让 WiFi task 以 250ms 周期观察配置 generation，立即标记 `network.state=disabled` 后执行 disconnect/stop。
- `mock:` 设备用于稳定开发预览和视觉证据，不发真实网络请求。
- 管理端页面已覆盖 Fleet、Connect、Overview、Power、Battery、Thermal、Device、Settings、API。
- 页面级信息归属已收口：Fleet Header 只在 Fleet 渲染；Connect 和每条单设备路由拥有内容区 `h1`；完整 `DeviceStatusBand` 只在 Overview 出现，移动导航仅显示紧凑的设备/路由上下文。
- 管理端新增 `/devices/:device_id/firmware`，支持 Web Serial 直烧与 devd 代理烧录，并展示 catalog 去重来源、确认区、阶段进度和终态摘要。
- Firmware 抽屉在烧录运行中会拦截页面刷新/关闭，禁用抽屉关闭、确认框与重复烧录入口；Web Serial 烧录复用当前已连接的串口并在完成/失败路径尝试复位回应用态。
- Settings 页对 LAN、USB CDC 或 devd 连接设备开放，提供 WiFi SSID/PSK 覆盖/清除、手动充电偏好、设备日志级别和 USB Console；USB Console 保留当前 Web Serial 或 devd transport 的 tx/rx frame、raw / ignored CDC 行和协议 payload，支持等级过滤、方向过滤、搜索高亮、虚拟滚动、全屏查看与 payload 折行开关，PSK 脱敏。
- Web App 已移除独立 USB HTTP bridge 分支，devd 成为 localhost USB 控制面；同一 `identity.device_id` 的 LAN 与 USB 来源合并为一条设备记录，并在 Fleet / Connect 中显示 WiFi 与 USB 标记。
- hosted/self-hosted devd Connect UI 只显示 devd discovery：USB 候选通过 devd lease/usb-http bridge 接入，LAN 候选在保存到 `DeviceRegistry` 时直接落为硬件 HTTP target，不再额外显示 Web Serial / 手动 LAN fallback 面板。
- Connect discovery 行为语义已收敛到“先纳管、后进入”：新发现 USB 候选显示 `Bind USB`，新发现 LAN 候选显示 `Add WiFi`；只有已存在的浏览器设备记录才显示 `Open`、`Use WiFi`、`Use USB`，避免把 discovery 候选误表述为通用 `Connect`。
- GitHub Pages/public-static 构建现在显式写入运行模式标记，不再默认把 `VITE_DEFAULT_DEVD_URL` 视为 same-origin devd，也不再依赖 `/api/v1/devices` 失败来反推出 LAN fallback。Pages Connect 直接把 browser-direct LAN 入口作为主路径，只在 hosted devd 或显式 devd URL 时显示 devd discovery。
- Pages/browser-direct LAN 入口新增 Chrome 142+ + secure context 能力闸门、统一手动目标合同，以及手动 IPv4 CIDR 扫描。扫描只做浏览器侧 `GET /api/v1/identity` 发现，固定并发 `8`、单地址超时 `800ms`，候选先保留为 session-local 卡片，只有显式 `Add WiFi` / `Open` 后才写入 `DeviceRegistry`。
- 手动目标和扫描候选在成功持久化时都会优先保存 `hostname_fqdn` 直连地址，同时把当前 IPv4 地址保留为 `rememberedChannels.http.fallbackBaseUrl`，与现有 `hostname_fqdn > hostname > ip:port` 记忆通道优先级保持一致。
- 当 USB candidate 还处于 `identity pending` 但 owner 已知其对应的 WiFi 设备时，Connect 页会先把该 stable USB id 绑定到已有 logical device，再把 discovery 行内的 USB/WiFi 渠道归并到同一设备卡片；绑定完成前不再把 `Bind USB` 误当成“立即进入设备”。
- USB `Bind USB` 成功后，如果 devd 返回 `companion_lan_candidate`，Connect 会在同一 discovery card 内显示 inline `Bind LAN companion`；确认后浏览器记录会把 `http://<hostname_fqdn>` 作为默认 Web 直连地址，同时保留 `http://<ip>:<port>` 作为回退地址，并把 `preferredTransport` 设为 `http`。
- 未确认的 companion-LAN candidate 不会自动进入 remembered WiFi channels，也不会立刻出现在 `Use WiFi` 切换动作里；只有已确认的 `binding.lan_companion` 或真实 LAN transport 才会成为可切换通道。
- Fleet 入口改为消费“浏览器本地保存记录 + 当前 devd discovery”的混合视图：已保存设备继续保留 alias/location，本轮 discovery 负责补当前 WiFi/USB 渠道、在线态和 live-only 设备卡片；empty state 不再把“没保存记录”误报成“没有设备”。
- devd Web USB control lease 已落地：多候选设备必须由用户选择，Web 创建 lease 后 heartbeat 续租，断开/移除/页面卸载时释放，TTL 到期自动释放，settings 写入与 USB Console hydration 均要求有效 lease。
- Web Serial 与 devd 连接路径在读取 USB identity 后都会匹配 firmware artifact catalog。未命中时返回 `firmware_artifact_mismatch` 气泡并阻断可写 session；devd 路径会释放刚创建的 lease，用户点击显式忽略按钮后才重新发起连接。
- USB Console 保留 raw/ignored 串口记录本身，不再额外显示 `Decode issue` 或 `defmt decoder unavailable` 诊断标签；连接时的 firmware artifact 匹配门禁负责阻断不匹配固件。
- Settings 和 Connect 失败反馈统一为气泡 callout；WiFi 保存、WiFi 清除和 charge-control/advanced-power 写入在固件/devd 返回前显示 spinner 并禁用并发写入。
- Power 页 owner-facing 手动充电控制已迁到单弹窗 `charge-control` 流：当前态解释来自 `GET /api/v1/charge-control`，preview 来自无副作用 `/preview`，`START/STOP/confirm_loop` 来自 action endpoint。
- Fleet 卡片使用用户可理解的摘要字段，技术细节保留到单设备详情与 API 调试页。
- Demo 复用正式前端路由，通过 `demo=true` 进入 mock-only 运行态；场景切换由左上角 Demo Logo 打开的悬浮控制面板完成，覆盖默认 fleet、空数据、全离线、大数量、USB、Critical Battery、Backup、API Debug 等路径，不再通过 public `seed=` URL 深链暴露。

## 验证状态

- `bun install`: 已通过。
- `bun run web:check`: 已通过。
- `bun run web:build`: 已通过。
- `bun run web:test`: 已通过。
- `cargo test --manifest-path firmware/host-unit-tests/Cargo.toml usb_cdc_protocol`: 已通过。
- `cd firmware && cargo +esp check`: 已通过。
- `cd firmware && cargo +esp check --no-default-features`: 已通过。
- Storybook：已使用最新 `storybook` / `@storybook/react-vite` 10.3.6 建立 `UPS Management/Settings/WiFi Provisioning Feedback` 状态矩阵，覆盖连接失败、保存失败、清除失败、保存中、清除中与成功反馈；`UPS Management/Connect/Firmware mismatch warning` 覆盖连接前 firmware artifact 不匹配与显式忽略入口。
- Storybook：`UPS Management/Add device` 已补齐 Pages direct LAN 支持态、非支持浏览器降级态、手动目标成功态与 CIDR 扫描命中态。
- Storybook：`UPS Management/PWA Update` 覆盖 update ready、activating、offline ready、registration error、mobile 和 state gallery，并验证 ready 状态下点击 `Update` 会先进入确认对话。
- 本地预览：已通过端口租约启动 Vite mock-data 前端。
- 浏览器验证：已确认 Fleet、Connect 和单设备 Dashboard 可渲染，控制台无 warn/error。
- 浏览器验证：真实 devd 驱动的 `/connect` 已确认显示 `Bind USB` 发现动作，不再暴露旧的 `Connect devd` 样式与语义。
- 浏览器验证：Fleet 现在会把本地 saved records 与当前 devd discovery 合并展示；只有 live discovery 的设备会落成独立卡片并回到 Connect 完成纳管，不再显示误导性的 `No UPS devices saved` 空态。
- 视觉证据：已生成 desktop Fleet、mobile Fleet、empty Fleet、large Fleet、single-device Dashboard 的 mock UI 截图；截图已回传给主人，并作为 spec assets 落盘供 owner-facing review 使用。
- Review-loop：已通过，未发现剩余可操作问题。

## 当前缺口

- 多 USB CDC candidates 场景需要在 `/connect` 显示选择器，不能自动选择已连接或已识别设备。
- devd 控制 session 需要短 TTL lease；正常关闭立即释放，异常断开默认 8-9 秒内释放。
- WiFi config 与 settings 写入需要携带有效 lease，避免 Web 不存在时 devd 继续占用或写入硬件。

## PR 状态

- PR: https://github.com/IvanLi-CN/mains-aegis/pull/71
- Stop condition: Step 5C Ready，等待主人确认后再合并。

## Migrated Implementation Record

- Status: 已完成（USB CDC safe-control follow-up, firmware flash addendum, hosted Connect semantics）
- Created: 2026-04-28
- Last: 2026-06-07

- [x] M1: 安装 Cohere `DESIGN.md` 并建立 Web 管理端规划。
- [x] M2: 新增 `web/` Vite + React + TypeScript + Bun 应用骨架。
- [x] M3: 完成多设备 Fleet 卡片网格、设备管理与单设备详情页。
- [x] M4: 完成只读 API/SSE 客户端、mock fixtures、类型检查、生产构建和 mock UI 视觉验证。
- [x] M5: 创建 PR #71 并完成快车道 review / CI 收敛到 merge-ready。
- [x] M6: 增加 USB CDC / Web Serial safe-control follow-up，完成协议、Web UI、固件处理、文档与视觉证据。
- [x] M7: 增加 PWA install/offline/update prompt 合同，完成 manifest、service worker、icons、Storybook 状态画廊与构建验证。

## References

- `./SPEC.md`
- `./HISTORY.md`
