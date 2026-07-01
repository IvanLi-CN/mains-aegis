# History（#7jqrq）

## 2026-06-02

- 决定采用 host-tools single-crate 结构：`tools/mains-aegis-host` 产出 CLI 与 devd。
- 决定 hard switch：`serve` 只提供 IPC，HTTP/Web 改为显式 `serve-http`。
- 决定用户操作必须依赖 release 安装工具；源码构建保留给开发 skill。
- 决定 hosted `serve-http` 默认走 same-origin 模式：嵌入式 HTML 注入进程内 app-session secret，API 请求必须带固定 header，SSE 使用 matching query param。
- 决定 `serve-http --allow-dev-cors` 仅作为 loopback 开发入口：暴露 API-only 模式，不托管嵌入式页面，也不允许与 `--open-browser` 组合。
- 决定 hosted Connect UI 不再暴露 devd URL 或 token 表单；devd endpoint/token 仅保留给 Vite 开发环境和显式 LAN bridge 场景。

## 2026-06-06

- 决定本仓 Codex 默认路由为 `$mains-aegis-devd-flow`，覆盖开发、验证、诊断、现场排查和硬件 read/session-read 检查。
- 决定 `$mains-aegis-user-operations` 只作为显式 end-user/released host-tools 路径，保留缺少 release 工具时硬阻断的语义。
- 决定 read/session-read 默认无需额外授权：scan/list、connect/disconnect、identity/status/diag-snapshot、monitor start/stop/log reading；持久绑定变更、settings 写入、reset、flash 和真实 host power action 仍需明确授权。

## 2026-06-07

- 决定 hosted/self-hosted `serve-http` Connect 页只保留 devd discovery；Web Serial 与手动 LAN fallback 只属于独立浏览器 / Vite 开发场景。
- 决定 devd discovery 里的 transport 语义按“发现源”和“实际连接源”分离：USB 候选通过 devd Web lease / usb-http bridge 接入，LAN 候选则落为设备本体 HTTP target，而不是 `devd transport`。
- 决定 Connect discovery 的 owner-facing 动作命名回到绑定模型：未纳管 USB 候选显示 `Bind USB`，未纳管 LAN 候选显示 `Add WiFi`，已纳管设备才显示 `Open` / `Use ...`。
- 决定 USB `bind` 成功后若已可验证 companion LAN，CLI 交互式 TTY 直接询问是否同时保存 `mDNS + IP:Port`；非交互场景只输出建议命令，不自动持久化。
