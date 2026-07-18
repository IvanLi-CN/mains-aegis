# Web 管理界面规划

## 目标

Web 管理界面是 UPS 的浏览器侧运维台，负责设备发现、多设备实时状态查看、基础诊断、接口验证、LAN HTTP API 管理与 USB CDC 安全设置。界面不替代前面板小屏；USB CDC / Web Serial 负责本地 USB 会话，LAN 设备始终通过设备本体 HTTP API 连接。

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
- 目的：维护浏览器当前关注的 UPS 清单；USB CDC 用于本地安全会话，LAN 用于设备本体 HTTP API 的状态与当前已支持设置写入。
- 主要内容：`mains-aegis-devd` 自动发现面、浏览器支持状态、串口授权/占用错误、LAN 新增连接目标、手动 IPv4 CIDR 扫描、探活结果、设备身份摘要、网络状态、API 版本兼容提示、已保存设备列表。
- 接口对接：USB CDC 使用 Web Serial JSONL frame；devd 默认通过 IPC 由 `mains-aegis` CLI 自动启动或复用；Web 需要 HTTP 时显式启动 `mains-aegis daemon http`，默认本地地址为 `http://127.0.0.1:30080`。默认 hosted 模式把嵌入式 Web App 与 `/api` 绑定到同一 same-origin HTTP 服务，并用进程内 app-session secret 保护 API；`--allow-dev-cors` 仅用于 loopback Vite 开发源的 API-only 模式。hosted / self-hosted devd UI 的 Connect 页只保留 devd discovery：USB 设备通过 devd 的 Web lease / usb-http bridge 接入，LAN 设备由 devd 列出后仍直连硬件本体 HTTP API；独立浏览器 / Vite 开发场景才保留 Web Serial 与手动 LAN fallback 面板。GitHub Pages/public-static 构建明确收口为 `public-static browser-direct LAN`：默认不假设 same-origin devd，不轮询 `/api/v1/devices`，而是把手动 LAN 目标与手动 CIDR 扫描作为主入口；只有 hosted devd 或显式配置的 devd URL 才显示 devd discovery 语义。Pages 直连能力只正式支持 `Chrome 142+` 且 secure context；其他浏览器只显示迁移指引并禁用连接/扫描动作。devd 会持久化绑定、别名和 artifact selection；连接、租约、monitor 与日志仍是运行态，daemon 重启后必须重新连接。LAN 入口只连接硬件本体的 HTTP/SSE 端点，不接受 devd HTTP service 作为 LAN 目标。
- owner-facing 真机表述必须明确区分 real 与 mock：`mock:` 数据源、`mock_hosted=1`、`mock_devd_target=...`、`stored_target_preset=...` 只属于纯前端 demo / 视觉证据，不得写成真机连接步骤，也不得作为 owner-facing 实机 handoff URL。
- 交互语义：新发现但未纳管的 USB 候选显示 `Bind USB`，新发现的 LAN 候选显示 `Add WiFi`；只有已经落入浏览器设备清单的设备才显示 `Open` 和 `Use WiFi` / `Use USB` 这类切换动作。Connect 页不把 devd discovery 候选直接表述成通用 `Connect` 按钮。
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
- 主要内容：市电输入、`input_vbus_mv` / `input_ibus_ma`、输出请求与实际激活通道、`gate_reason`、OUT A/B 电压电流、recoverable 状态，以及 owner-facing `charge-control` 当前态摘要。
- 输入面板必须直接展示 `source / pressure_state / pressure_score_pct / pressure_reason / tps_total_iout_ma / tps_limit_threshold_ma / vin_baseline_mv / vin_drop_mv`，让 owner 能判断当前 `DC IN` 压力，尤其是 `TPS output current > 100mA` 的停充场景。
- 充电主卡片只展示当前态：当前模式、当前输入/绑定路径、策略目标、`IBAT` 实测、当前限流摘要、当前环路避免状态、剩余时间、停止/阻断原因与直接证据摘要。不是当前态的规则、合同表或推导性说明不得常驻显示。
- 接口对接：
  - `GET /api/v1/status` 的 `input`、`output`、`charger` 与紧凑 `charge_control` 摘要
  - `GET /api/v1/charge-control` 作为 Power 页当前态与手动充电弹窗的权威详情面
  - `POST /api/v1/charge-control/preview` 用于回答“如果现在点 START 会怎样”
  - `POST /api/v1/control/manual-charge` 用于 `START/STOP/confirm_loop`
- 交互：
  - 手动充电只通过弹窗发起，不在 Power 页常驻表单。
  - 同一弹窗内完成 defaults 编辑、preview、`START/STOP` 与 USB-C 环路确认。
  - 若 `readiness.state=confirm_required`，确认态必须留在同一弹窗内，不再跳出第二个 owner-facing 窗口。

### 5. 电池与 BMS

- 入口：`/devices/:device_id/battery`
- 目的：集中展示电池包和 BMS 是否可放电、是否恢复中、是否存在保护或无电池状态。
- 主要内容：SOC、pack voltage、current、四节 `cell_mv`、cell delta、均衡起步阈值、`BAL OFF / IDLE / Cn / MULTI / ACTIVE / --`、`discharge_ready`、`no_battery`、`charge_fet_on` / `discharge_fet_on` / `precharge_fet_on`、`issue_detail`、`recovery_pending`、`last_result`。
- 均衡视觉：cell delta 不只用颜色表达，必须同时显示 mV 数值与 BAL 状态；颜色分级要呼应主板均衡基线，`<= balance_min_start_delta_mv` 为正常，超过起步阈值但不高于 `25mV` 为轻微关注，`25..200mV` 为 warning，`>200mV` 进入服务/预均衡关注，不暗示常规自动均衡一定能拉回。
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

### 8. Settings 与 WiFi 配网

- 入口：`/devices/:device_id/settings`
- 目的：对单台实机执行当前设备 API 支持的 settings 写入；LAN 直连与 devd transport 使用同一字段语义。
- 可写范围：WiFi SSID/PSK 覆盖或清除、设备日志级别，以及 Advanced Power staged assist/takeover 高级参数。手动充电 owner-facing 控制已迁到 Power 页弹窗；Settings 只保留持久化 `manual_charge` prefs 的兼容入口，不承担 `START/STOP`。
- Secret 规则：PSK 只在用户提交时通过 USB CDC 或 LAN API 写入固件 EEPROM，不在 UI、日志或 ack 中回显；提交后清空表单。默认固件启用 `net_http`，但不存在默认 WiFi 凭据；固件优先读取 EEPROM WiFi config，写入后更新运行时 WiFi 配置，清除后清空 EEPROM slot 并立即断开 WiFi。
- 反馈规则：WiFi 保存/清除必须等固件 ack 或 LAN accepted response 与连接状态反馈后才显示结果；等待期间按钮显示 loading。连接硬件、保存 WiFi、清除 WiFi 和 settings 失败统一以气泡 callout 展示。
- LAN 直连：HTTP 设备通过 `/api/v1/settings` 读取快照；WiFi / log level / advanced power 继续走 settings 写接口。手动充电偏好仍可持久化到 `/api/v1/settings/manual-charge`，但 owner-facing 运行态控制与原因解释必须走 `charge-control` 合同。
- devd 控制面：通过 `mains-aegis-devd` 持有 USB CDC 或 LAN transport 后，Web App 可使用同一 Settings 表单；devd 仍然独占 USB CDC，但日志与 trace 通过新的 `trace` 模型呈现在 USB Console。
- 日志：Settings 页提供 USB Console。USB Console 展示当前 Web Serial 或 devd transport 内 Web 可见的 CDC/HTTP trace：Web 发出的 `tx/request`、固件返回的 `rx/response`、structured `log`、`status`、`hello`、`error`，以及夹杂在 CDC 行流中的 raw / ignored 非协议行。控制台支持等级过滤、方向过滤、关键词搜索高亮、虚拟滚动、全屏查看，并允许用户切换 payload 自动折行或横向滚动。WiFi PSK 在 trace 中脱敏。完整 `defmt` monitor 仍由 devd 在 artifact identity 匹配后解码。

### 9. 接口调试

- 入口：`/devices/:device_id/api`
- 目的：为开发和 bench 调试提供最小 API 可视化，验证 JSON 和 SSE 是否工作。
- 主要内容：endpoint 列表、最近一次响应、SSE 连接状态、USB CDC protocol 状态、host power dry-run 状态、错误 envelope 展示、structured log 与 USB Console。
- USB Console / trace 视图必须把 `kind=event,target=power` 作为可读 power event 显示，而不是只显示原始 JSON blob；当 `pressure_reason=tps_output_current` 时，必须直接显示 `TPS actual / threshold`。
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
- Settings / charge-control requests：`get_identity`、`get_status`、`get_settings`、`get_charge_control`、`preview_charge_control`、`control_manual_charge`、`set_log_level`、`set_manual_charge_prefs`、`set_advanced_power`、`reset_advanced_power`。`set_advanced_power` 必须整块提交 11 个数字字段，不允许以 owner-facing 绝对电压或绝对电流值写入。
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
- `serial transport`：浏览器侧持有当前连接的 `SerialPort`，解析 USB CDC JSONL frame，复用 `Identity`、`NetworkSummary`、`UpsStatus`，并把 write ack/error/log 映射回设备记录。
- `devd transport`：`mains-aegis` CLI-managed devd 持有 USB CDC transport，并负责发现本机可见 USB/LAN 设备；`mains-aegis daemon serve` 是 developer/debug IPC daemon，`mains-aegis daemon http` 是显式 Web + API HTTP 服务。默认 hosted 模式下，Web/App 通过 same-origin HTTP 读取 identity/status/network/settings、提交 settings 写入，并通过 `trace` 模型读取 bounded tail structured logs 与 CDC trace；LAN 设备即使由 devd 发现，落到 Web `DeviceRecord` 时仍保持设备本体 HTTP API 直连。开发期 `--allow-dev-cors` 只暴露 API，不托管嵌入式页面。
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
- GitHub Pages：根站点发布 Web App，文档站发布在同一 Pages artifact 的 `/docs/` 子路径；App 使用 History API path router，并通过 `PAGES_BASE` / `VITE_BASE` 支持仓库子路径和未来自定义域名根路径。Pages 构建会显式写入 `public_static` 运行模式标记，Connect 页默认展示 browser-direct LAN 入口，并只在用户点击后执行 IPv4 CIDR 扫描；扫描结果只保留在当前页面状态，只有显式 `Add WiFi` / `Open` 后才会落入浏览器持久化设备列表。
- PWA：Web App 通过 `vite-plugin-pwa` 生成 `manifest.webmanifest`、`sw.js` 与 192/512 PNG maskable icons。首次成功打开后，service worker 会预缓存 app shell、Vite 构建产物、Pages fallback、相对 base 深链 navigation helper、PWA 图标和 bundled static firmware artifacts；真实设备 `/api`、`/events`、LAN HTTP/SSE、USB/Web Serial 与 GitHub Release live catalog 不做离线伪造。新版本使用 `registerType="prompt"`：浏览器后台下载新 app shell 后只显示非阻塞更新提示，用户点击 `Update` 并确认后才切换到新版本并刷新页面。
- 全局导航：App Layout 侧栏固定提供 `Docs` 入口，打开 `${BASE_URL}docs/`，保持当前运维台页面与连接状态不被替换。
- 数据接入：`DeviceRegistry` 负责 localStorage 设备清单、LAN 探活、settings 读取、SSE 订阅与轮询兜底、当前 Web Serial USB CDC transport，以及 devd 本地 control transport；同一 `identity.device_id` 的 LAN 与 USB 来源合并为一条设备记录。devd 发现出的 LAN 设备会直接落为 HTTP target，USB 设备才保留 devd lease / serial 上下文。
- Connect 发现动作使用项目既有小号主次按钮体系；未纳管设备使用 `Bind USB` / `Add WiFi`，已纳管设备使用 `Open` 与 `Use ...`，不引入独立的 split-button 控件族。
- 验证命令：`bun run web:check`、`bun run web:test`、`PAGES_BASE=/mains-aegis/ bun run web:build`、`DOCS_BASE=/mains-aegis/docs/ bun run --cwd docs-site build`、`cargo test --manifest-path firmware/host-unit-tests/Cargo.toml usb_cdc_protocol`、`cargo test --manifest-path tools/mains-aegis-host/Cargo.toml`、`cd firmware && cargo +esp check`。
- 本地设备 daemon：普通 CLI 验证会自动启动 repo-local IPC daemon；开发前台日志使用 `just devd-serve`；Vite 开发期 API 验证使用 `just devd-http`；hosted 模式由 `mains-aegis daemon http` 直接托管嵌入式 Web 产物。
- 纯前端 Demo：`bun run web:dev` 后访问正式路由并加 `?demo=true`，例如 `/?demo=true`、`/connect?demo=true`、`/devices/mains-aegis-e4f5a6/battery?demo=true`。Demo 场景切换在左上角 Demo Logo 打开的悬浮控制面板内完成，不再通过 `seed=` URL 参数暴露。
