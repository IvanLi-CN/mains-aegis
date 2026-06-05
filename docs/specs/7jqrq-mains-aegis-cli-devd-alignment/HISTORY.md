# History（#7jqrq）

## 2026-06-02

- 决定采用 host-tools single-crate 结构：`tools/mains-aegis-host` 产出 CLI 与 devd。
- 决定 hard switch：`serve` 只提供 IPC，HTTP/Web 改为显式 `bridge-http`。
- 决定用户操作必须依赖 release 安装工具；源码构建保留给开发 skill。
- 决定 Web bridge token 必须保持显式 opt-in：devd probe/status 请求携带 token，普通 LAN 探活和 LAN status SSE 不携带 token。
- 决定普通 HTTP 添加流程用免认证 `/api/v1/bootstrap` 探测 bridge auth requirement；只有确认是 token bridge 的目标才持久化 bridge auth 标记并给 status SSE 添加 query token。
- 决定 bridge token 以 normalized bridge URL 为作用域存储；Connect UI 必须允许用户在首次 probe/scan 前输入 token，避免 protected bridge 在干净浏览器中无法添加。

## 2026-06-06

- 决定本仓 Codex 默认路由为 `$mains-aegis-devd-flow`，覆盖开发、验证、诊断、现场排查和硬件 read/session-read 检查。
- 决定 `$mains-aegis-user-operations` 只作为显式 end-user/released host-tools 路径，保留缺少 release 工具时硬阻断的语义。
- 决定 read/session-read 默认无需额外授权：scan/list、connect/disconnect、identity/status/power-diag、monitor start/stop/log reading；持久绑定变更、settings 写入、reset、flash 和真实 host power action 仍需明确授权。
