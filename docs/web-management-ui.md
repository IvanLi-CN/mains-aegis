# Web 管理界面规划

## 目标

Web 管理界面是 UPS 的浏览器侧运维台，负责设备发现、多设备实时状态查看、基础诊断、接口验证与 USB CDC 安全设置。界面不替代前面板小屏；LAN HTTP/SSE 保持只读，USB CDC / Web Serial 是首个受限写入通道。

设计基线使用根目录 `DESIGN.md` 的 Cohere 风格：白色编辑式主画布、深绿黑状态带、克制企业感、少量高信号色。管理端以任务效率为优先，使用紧凑网格、清晰状态标签、稳定表格和可扫描的数据层级。

## 信息架构

### 1. 设备群总览

- 入口：`/`
- 目的：同时查看多台 UPS 的在线状态、运行模式、供电风险与需要处理的告警。
- 主要内容：设备列表、在线/离线状态、运行模式、电池 SOC、输出状态、最高告警级别、最后更新时间。
- 接口对接：对每台已知设备读取 `GET /api/v1/identity`、`GET /api/v1/network`、`GET /api/v1/status`；在线设备优先建立 status SSE。
- 布局：顶部显示设备总数、在线数、critical/warning 数；主体使用响应式设备卡片网格。
- 交互：点击设备行进入单设备总览；支持按状态、位置或 hostname 搜索过滤。
- 示例组织：Demo 复用正式前端与正式路由，使用 Vite 纯前端 mock 数据和 `seed` 参数切换演示场景；不要为 Demo 维护一套不同于产品的页面。

#### 设备卡片结构

每张设备卡片固定展示同一套面向日常判断的字段，避免把通道、电流、寄存器语义等技术细节放到总览第一屏：

- 顶部：设备别名 / hostname、位置标签、在线状态点、最后更新时间。
- 主状态：运行模式（`STANDBY / ASSIST / BACKUP / FAULT / OFFLINE`）和最高告警级别。
- 用户摘要：SOC、供电来源、负载是否供电、电池是否可用、是否需要处理、连接状态。
- 技术细节：OUT A/B、charger、pack voltage、raw API payload 等只在单设备详情或 API 调试页展示。
- 底部操作：`Details` 进入详情。

网格规则：

- 桌面：3 到 4 列，卡片最小宽度约 `280px`。
- 平板：2 列。
- 手机：1 列。
- Critical 卡片排序靠前；Warning 次之；离线设备保留在列表内并显示 stale 时间。
- 卡片只使用 8px 圆角和轻边框，状态色用于 badge / 指示点，不使用整卡大面积染色。

### 2. 连接与设备管理

- 入口：`/connect`
- 目的：维护浏览器当前关注的 UPS 清单；USB CDC 用于安全设置与配网，LAN 用于只读状态。
- 主要内容：USB CDC / Web Serial 连接入口、`mains-aegis-devd` 本地 daemon 连接入口、浏览器支持状态、串口授权/占用错误、LAN 新增连接目标、探活结果、设备身份摘要、网络状态、API 版本兼容提示、已保存设备列表。
- 接口对接：USB CDC 使用 Web Serial JSONL frame；devd 默认通过 IPC 由 `mains-aegis` CLI 访问；Web 需要 HTTP 时显式启动 `mains-aegis-devd bridge-http`，默认本地地址为 `http://127.0.0.1:30080`，且同一 bridge 进程也持有共享状态的 IPC listener 供 CLI 使用。devd 会持久化绑定、别名和 artifact selection；连接、租约、monitor 与日志仍是运行态，daemon 重启后必须重新连接。LAN 入口只连接硬件本体的 HTTP/SSE 端点，不接受 devd bridge 作为 LAN 目标。
- 空状态：提示用户连接 USB CDC 或输入 `mains-aegis-<short_id>.local` / 局域网 IP。

### 3. 单设备总览 Dashboard

- 入口：`/devices/:device_id`
- 目的：给出 UPS 当前能否供电、是否在充电、是否有保护限制的第一屏判断。
- 主要内容：运行模式、输入电压电流、输出 A/B 状态、电池 SOC、充电器状态、温度摘要、网络摘要。
- 接口对接：首屏 `GET /api/v1/status`，随后用 `GET /api/v1/status` + `Accept: text/event-stream` 订阅实时状态。
- 布局：顶部深绿黑运行状态带；下方两列或三列仪表区；底部放最近一次错误或阻断原因。

### 4. 电源路径

- 入口：`/devices/:device_id/power`
- 目的：拆解输入、充电、输出之间的能量路径，便于调试 UPS 行为。
- 主要内容：市电输入、`input_vbus_mv` / `input_ibus_ma`、输出请求与实际激活通道、`gate_reason`、OUT A/B 电压电流、recoverable 状态。
- 接口对接：`GET /api/v1/status` 的 `input`、`output`、`charger`。
- 交互：按 `Input`、`Charger`、`Output A`、`Output B` 分段，所有写操作按钮首版不出现。

### 5. 电池与 BMS

- 入口：`/devices/:device_id/battery`
- 目的：集中展示电池包和 BMS 是否可放电、是否恢复中、是否存在保护或无电池状态。
- 主要内容：SOC、pack voltage、current、`discharge_ready`、`no_battery`、`issue_detail`、`recovery_pending`、`last_result`。
- 接口对接：`GET /api/v1/status` 的 `battery`。
- 视觉规则：正常态用低饱和绿色；保护、无电池、恢复等待用明确但不过饱和的 warning/error 标签。

### 6. 温度与保护

- 入口：`/devices/:device_id/thermal`
- 目的：展示温度采样与热保护风险，避免把过温原因埋在总览里。
- 主要内容：TMP A/B 状态和温度、热相关保护摘要、输出或充电限制的关联原因。
- 接口对接：`GET /api/v1/status` 的 `thermal`，以及 `output.gate_reason`、`charger.state` 的派生说明。
- 后续扩展：若设备侧新增历史采样，再增加趋势图；首版只展示实时值和状态解释。

### 7. 网络与设备信息

- 入口：`/devices/:device_id/device`
- 目的：确认当前连接的是哪台 UPS，并提供调试所需的固件与网络信息。
- 主要内容：device_id、hostname、FQDN、short_id、role、api_version、firmware build 信息、capabilities、IPv4、gateway、DNS、RSSI、last_error。
- 接口对接：`GET /api/v1/identity`、`GET /api/v1/network`。
- 结构：设备身份在上，网络状态在中，能力矩阵和固件构建信息在下。

### 8. 安全设置与 WiFi 配网

- 入口：`/devices/:device_id/settings`
- 目的：通过 USB CDC 对单台实机执行受限安全设置。
- 可写范围：WiFi SSID/PSK 覆盖或清除、手动充电偏好、USB session 日志级别。
- Secret 规则：PSK 只在用户提交时通过 USB 写入固件 EEPROM，不在 UI、API payload、日志或 ack 中回显；提交后清空表单。默认固件启用 `net_http`，但不存在默认 WiFi 凭据；固件优先读取 EEPROM WiFi config，USB 写入后更新运行时 WiFi 配置，USB 清除后清空 EEPROM slot 并立即断开 WiFi。
- 反馈规则：WiFi 保存/清除必须等固件 ack 与连接状态反馈后才显示结果；等待期间按钮显示 loading。连接硬件、保存 WiFi、清除 WiFi 和 safe settings 失败统一以气泡 callout 展示。
- LAN 限制：HTTP/SSE 设备进入该页时只显示 USB required 状态，不提供写表单。
- devd 控制面：通过 `mains-aegis-devd` 持有 USB CDC 后，Web App 可使用同一 Settings 表单；devd 仍然独占 CDC，但日志与 trace 通过 `/api/v1/serial/session` 呈现在 USB Console。
- 日志：Settings 页提供 USB Console。USB Console 展示当前 Web Serial 或 devd session 内 Web 可见的 CDC trace：Web 发出的 `tx` frame、固件返回的 `rx` frame、structured `log`、`status`、`hello`、`response`、`error`，以及夹杂在 CDC 行流中的 raw / ignored 非协议行。控制台支持等级过滤、方向过滤、关键词搜索高亮、虚拟滚动、全屏查看，并允许用户切换 payload 自动折行或横向滚动。WiFi PSK 在 trace 中脱敏。完整 `defmt` monitor 仍由 devd 在 artifact identity 匹配后解码。

### 9. 接口调试

- 入口：`/devices/:device_id/api`
- 目的：为开发和 bench 调试提供最小 API 可视化，验证 JSON 和 SSE 是否工作。
- 主要内容：endpoint 列表、最近一次响应、SSE 连接状态、USB CDC protocol 状态、host power dry-run 状态、错误 envelope 展示、structured log 与 USB Console。
- 接口对接：`/api/v1/ping`、`/api/v1/identity`、`/api/v1/network`、`/api/v1/status`、status SSE、`/api/v1/host/power`、`/api/v1/host/power/events`。
- 限制：只读请求，不提供任意 URL fetch，避免浏览器端变成不受控代理。

### 10. 固件烧录

- 入口：`/devices/:device_id/firmware`
- 目的：在浏览器里完成 Web Serial 直烧或通过本地 `mains-aegis-devd` 代理烧录。
- 主要内容：当前设备固件摘要、catalog 来源、去重后的可用 artifact、Web Serial 支持状态、devd 绑定状态、校验状态、确认区、进度条、阶段日志和结果摘要。
- 固件来源：Web App 合并 bundled static catalog 与 GitHub Release catalog，按 `artifact_id` 去重，bundled 优先。
- Web Serial 规则：只烧录带 `flash_address` 的 `image` 文件，并在写入前校验 `sha256`。
- devd 规则：先 select artifact，再 dry-run，然后真实 flash，必须要求显式绑定设备；devd 只能烧录已随 Web 静态资源 staging 到本地磁盘的 bundled artifact，GitHub Release-only artifact 由 Web Serial 路径烧录。
- devd 目标选择：Firmware 抽屉只能使用 `identity.device_id` 与当前记录一致的 devd device；找不到精确匹配时必须阻断，不得 fallback 到其它已绑定设备。
- 运行保护：Web Serial 或 devd 烧录运行中必须拦截页面刷新/关闭，锁定抽屉关闭、确认框与重复烧录入口，直到烧录成功或失败后恢复。

## USB CDC / Web Serial 协议

- Framing：LF 分隔 JSON frame。
- Protocol：`mains-aegis.cdc.v1`。
- Frame types：`hello`、`status`、`log`、`request`、`response`、`error`、`wifi_config`。
- Web 写命令必须带 `request_id`，固件返回同 ID 的 `response` 或 `error`。
- Safe requests：`get_identity`、`get_status`、`set_log_level`、`set_manual_charge_prefs`。
- WiFi config：`{"type":"wifi_config","request_id":"...","op":"set","ssid":"...","psk":"..."}` 或 `op:"clear"`。
- Error envelope：`{ code, message, retryable, details }`，与 HTTP API 错误形状一致。

## 应用结构

建议后续新增 `web/` 作为独立前端应用目录：

```text
web/
  src/
    app/
      App.tsx
      routes.tsx
      layout/
    api/
      client.ts
      identity.ts
      network.ts
      status.ts
      status-stream.ts
      types.ts
    features/
      fleet/
      connect/
      dashboard/
      power/
      battery/
      thermal/
      device/
      api-debug/
    components/
      ui/
      status/
      charts/
    styles/
      tokens.css
      globals.css
```

首版技术选择建议：Vite + React + TypeScript。状态获取使用轻量 fetch 封装即可；`/api/v1/status` 的轮询兜底和 SSE 订阅应封装在 `api/status-stream.ts`，避免页面各自处理重连。多设备连接管理应额外提供 `DeviceRegistry`，保存 `device_id -> base_url -> latest snapshot -> connection state` 的映射。

## 接口状态模型

- `identity`：应用启动和设备切换时读取，作为当前设备真相源。
- `network`：连接页和设备页读取，用于解释 `.local`、IPv4 与 WiFi 状态。
- `status`：所有运行态页面共享的实时快照。
- `status-stream`：在线时使用 SSE；断开、409、503 或浏览器限制时退回定时 `GET /api/v1/status`。
- `device registry`：浏览器侧维护多设备清单；设备侧首版不需要新增聚合 API。
- `serial transport`：浏览器侧持有当前 session 的 `SerialPort`，解析 USB CDC JSONL frame，复用 `Identity`、`NetworkSummary`、`UpsStatus`，并把 write ack/error/log 映射回设备记录。
- `devd transport`：`mains-aegis-devd` 持有 USB CDC；`serve` 是 CLI-only IPC daemon，`bridge-http` 是 Web + CLI 共享状态 bridge。Web/App 在显式 `bridge-http` 模式下通过 HTTP 读取 identity/status/network、提交 safe settings，并通过 `/api/v1/serial/session` 读取 bounded tail structured logs 与 CDC trace。
- `host power transport`：devd localhost API 提供主机级 power profile 查询、低功耗运行 dry-run、suspend dry-run、shutdown dry-run 与 `host_power` SSE。它不是设备 USB 控制面；Web 首版只在 API Debug 暴露观察与 dry-run，不提供正式一键关机 UI。
- `error envelope`：所有页面统一渲染 `{ code, message, retryable, details }`，不要在组件里各自拼错误文案。

## 导航结构

- 顶部：当前视图、设备数量、在线状态、全局告警摘要。
- 群总览左侧导航：Fleet、Connect。
- 单设备左侧导航：Overview、Power、Battery、Thermal、Device、Settings、API。
- 内容区：宽屏使用 12 栅格；平板降为 2 列；手机保留顶部设备条并把侧边导航折叠为菜单。
- 状态层级：`critical` 优先于 `warning`，`warning` 优先于 `info`，正常态只在必要位置显示。

## 首版交付顺序

1. 建立 `web/` 应用骨架与 Cohere token 映射。
2. 完成 API 类型、fetch client、SSE client、错误 envelope 与 `DeviceRegistry`。
3. 完成设备卡片网格、连接与设备管理、单设备总览。
4. 补齐电源路径、电池与 BMS、温度与保护页面。
5. 增加 API 调试页、mock fixtures、多设备模拟数据与基础视觉回归入口。
6. 为 Fleet、Connect、单设备 Dashboard 和 API Debug 维护可复现 seed 场景，作为后续 UI 验收入口。

## 当前实现

- 应用目录：`web/`
- 技术栈：Vite + React + TypeScript + Bun。
- 默认数据：内置 6 台 mock UPS，覆盖 standby、assist、backup、warning、critical、offline。
- GitHub Pages：根站点发布 Web App，文档站发布在同一 Pages artifact 的 `/docs/` 子路径；App 使用 History API path router，并通过 `PAGES_BASE` / `VITE_BASE` 支持仓库子路径和未来自定义域名根路径。
- 全局导航：App Layout 侧栏固定提供 `Docs` 入口，打开 `${BASE_URL}docs/`，保持当前运维台页面与连接状态不被替换。
- 数据接入：`DeviceRegistry` 负责 localStorage 设备清单、LAN 只读探活、SSE 订阅与轮询兜底、当前 session 的 Web Serial USB CDC transport，以及 devd 本地 USB control transport；同一 `identity.device_id` 的 LAN 与 USB 来源合并为一条设备记录，devd safe-control 写入始终携带当前记录的 `identity.device_id`。
- 验证命令：`bun run web:check`、`PAGES_BASE=/mains-aegis/ bun run web:build`、`DOCS_BASE=/mains-aegis/docs/ bun run --cwd docs-site build`、`cargo test --manifest-path firmware/host-unit-tests/Cargo.toml usb_cdc_protocol`、`cargo test --manifest-path tools/mains-aegis-host/Cargo.toml`、`cd firmware && cargo +esp check`。
- 本地设备 daemon：开发 IPC-only CLI 验证使用 `cargo run --manifest-path tools/mains-aegis-host/Cargo.toml --bin mains-aegis-devd -- serve`；Web + CLI 共享状态验证使用 `cargo run --manifest-path tools/mains-aegis-host/Cargo.toml --bin mains-aegis-devd -- bridge-http --allow-dev-cors`，生产模式可通过 `--web-root <dir>` 托管 Web 静态资源。
- 纯前端 Demo：`bun run web:dev` 后访问正式路由，例如 `/`、`/?seed=empty`、`/?seed=large`、`/devices/mains-aegis-e4f5a6/battery?seed=default`。
