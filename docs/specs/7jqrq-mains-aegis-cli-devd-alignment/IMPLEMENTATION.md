# Implementation（#7jqrq）

## 当前实现

- `tools/mains-aegis-host` 统一产出 `mains-aegis` CLI 与 `mains-aegis-devd` daemon。
- `mains-aegis-devd serve` 为 IPC-only daemon；`serve-http` 为显式 HTTP/Web 服务，并在同一进程内启动共享状态的 IPC listener，供 CLI 与 Web 观察同一个 daemon 状态。
- CLI 通过 newline JSON IPC 调度设备、serial lease、settings、artifact、flash/reset/monitor 与 host power 命令族；新的查询面使用 `connection / identity / status / settings / trace`。
- hosted `serve-http` 默认从 `web/dist` 嵌入静态产物，向 HTML 注入 app-session secret 与运行模式 meta，并要求后续 API 请求携带 `x-mains-aegis-app-session`；SSE 使用 `app_session` query 参数。缺少 `web/dist` 时，host-tools 构建会 fail fast 并提示先执行 `bun run build`。
- `serve-http --allow-dev-cors` 切换到 API-only 开发模式：只允许 loopback HTTP origins 跨源访问 `/api`，根路径返回 API-only 提示页，不托管嵌入式 hosted app。
- 非 loopback `serve-http` 启动时要求 `--allow-lan-bridge` 与 `--auth-token-file`，API 请求仍接受 bearer token；`GET /api/v1/bootstrap` 保持免认证并报告真实 token requirement；浏览器 EventSource 兼容 `service_token` 与 legacy `bridge_token` query 参数授权。
- Host-tools release workflow 产出三平台 archive、安装脚本和可直接校验 release assets 的 `SHA256SUMS`；手动 release 在 tag 已存在时按该 tag 重建资产、tag 不存在时从当前 dispatch commit 创建新 release tag；`MAINS_AEGIS_RELEASE_VERSION` 同时驱动构建产物与 `mains-aegis` / `mains-aegis-devd` 的 `--version` 输出。
- Hosted Web client 从 same-origin HTML meta 读取 app-session secret，并且只向 devd HTTP service API 与 devd EventSource 请求附加该 secret；普通 LAN 目标不携带 app-session。Connect UI 不再暴露 devd URL 或 token 输入入口，LAN 入口固定为硬件直连。
- Repo skill routing defaults Codex work inside this repository to `$mains-aegis-devd-flow` for development, validation, diagnostics, field investigation, and hardware read/session-read checks. `$mains-aegis-user-operations` remains the explicit end-user/released host-tools route.

## 验证

- `cargo check --manifest-path tools/mains-aegis-host/Cargo.toml`
- `cargo check --manifest-path tools/mains-aegis-host/Cargo.toml --all-targets`
- `cargo test --manifest-path tools/mains-aegis-host/Cargo.toml`
- `bun run --cwd web check`
- `mains-aegis-devd serve --help` verified IPC-only help without `--bind`
- `mains-aegis-devd serve-http --bind 0.0.0.0:30080` verified non-loopback denial without token
- `mains-aegis-devd serve-http --auth-token-file <file>` verified unauthenticated `/api/v1/bootstrap` returns `token_required=true`, unauthenticated `/api/v1/status` returns `401`, and bearer-authenticated host power status succeeds
- `mains-aegis --ipc /tmp/mains-aegis-host-smoke.sock health` verified CLI to IPC health
- `mains-aegis --ipc /tmp/mains-aegis-host-smoke.sock devices list` verified CLI to IPC mock device list
- `mains-aegis-devd serve-http --ipc /tmp/mains-aegis-http-service-smoke.sock --bind 127.0.0.1:<leased-port> --allow-dev-cors` verified HTTP service and CLI IPC share one daemon state by binding `mock-devkit` over HTTP and reading the same binding over CLI IPC
- Mock UI visual evidence captured at `/connect?seed=default`
- Policy search verifies AGENTS, skills, and specs no longer make released host tools the default Codex route inside this repository.
