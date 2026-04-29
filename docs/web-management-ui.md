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
- 主要内容：USB CDC 连接入口、浏览器支持状态、串口授权/占用错误、LAN 新增连接目标、探活结果、设备身份摘要、网络状态、API 版本兼容提示、已保存设备列表。
- 接口对接：USB CDC 使用 Web Serial JSONL frame；LAN 使用 `GET /api/v1/ping`、`GET /api/v1/identity`、`GET /api/v1/network`。
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
- Secret 规则：PSK 只在用户提交时通过 USB 写入固件 EEPROM，不在 UI、API payload、日志或 ack 中回显；提交后清空表单。固件 `net_http` 启用时会优先读取 EEPROM WiFi config，并在 USB 写入后更新运行时 WiFi 配置。
- LAN 限制：HTTP/SSE 设备进入该页时只显示 USB required 状态，不提供写表单。
- 日志：展示当前 USB session 的 structured log；开发期夹杂的 legacy plain serial line 会作为 `raw_serial` debug log 展示，替代 Web App 连接期间的 `mcu-agentd monitor` 并发需求。

### 9. 接口调试

- 入口：`/devices/:device_id/api`
- 目的：为开发和 bench 调试提供最小 API 可视化，验证 JSON 和 SSE 是否工作。
- 主要内容：endpoint 列表、最近一次响应、SSE 连接状态、USB CDC protocol 状态、错误 envelope 展示、structured log。
- 接口对接：`/api/v1/ping`、`/api/v1/identity`、`/api/v1/network`、`/api/v1/status`、status SSE。
- 限制：只读请求，不提供任意 URL fetch，避免浏览器端变成不受控代理。

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
- 数据接入：`DeviceRegistry` 负责 localStorage 设备清单、LAN 只读探活、SSE 订阅与轮询兜底，以及当前 session 的 USB CDC transport。
- 验证命令：`bun run web:check`、`bun run web:build`、`cargo test --manifest-path firmware/host-unit-tests/Cargo.toml usb_cdc_protocol`、`cd firmware && cargo +esp check --features web_serial`。
- 纯前端 Demo：`bun run web:dev` 后访问正式路由，例如 `/`、`/?seed=empty`、`/?seed=large`、`/devices/mains-aegis-e4f5a6/battery?seed=default`。
