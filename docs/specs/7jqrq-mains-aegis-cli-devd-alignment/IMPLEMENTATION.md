# Implementation（#7jqrq）

## 当前实现

- `tools/mains-aegis-host` 统一产出 `mains-aegis` CLI 与 `mains-aegis-devd` daemon。
- `mains-aegis-devd serve` 为 IPC-only daemon；`bridge-http` 为显式 HTTP/Web bridge，并在同一进程内启动共享状态的 IPC listener，供 CLI 与 Web 观察同一个 daemon 状态。
- CLI 通过 newline JSON IPC 调度设备、serial lease、safe settings、artifact、flash/reset/monitor 与 host power 命令族。
- LAN bridge 启动时要求 `--allow-lan-bridge` 与 `--auth-token-file`，API 请求使用 bearer token；`GET /api/v1/bootstrap` 保持免认证并报告真实 token requirement；浏览器 EventSource 使用 `bridge_token` query 参数授权。
- Host-tools release workflow 产出三平台 archive、安装脚本和可直接校验 release assets 的 `SHA256SUMS`；手动 release 在 tag 已存在时按该 tag 重建资产、tag 不存在时从当前 dispatch commit 创建新 release tag；`MAINS_AEGIS_RELEASE_VERSION` 同时驱动构建产物与 `mains-aegis` / `mains-aegis-devd` 的 `--version` 输出。
- Web client 支持从 localStorage 读取按 bridge URL 分桶的 bearer token，并且只在 devd/bridge API、devd probe/status 请求与 devd EventSource 请求上附加 token；普通 HTTP 目标先免认证探测 `/api/v1/bootstrap`，仅当 bridge 明确报告 token required 时持久化 bridge auth 标记。Connect UI 为 LAN bridge 与 devd bridge 提供 token 输入入口。

## 验证

- `cargo check --manifest-path tools/mains-aegis-host/Cargo.toml`
- `cargo check --manifest-path tools/mains-aegis-host/Cargo.toml --all-targets`
- `cargo test --manifest-path tools/mains-aegis-host/Cargo.toml`
- `bun run --cwd web check`
- `mains-aegis-devd serve --help` verified IPC-only help without `--bind`
- `mains-aegis-devd bridge-http --bind 0.0.0.0:30080` verified non-loopback denial without token
- `mains-aegis-devd bridge-http --auth-token-file <file>` verified unauthenticated `/api/v1/bootstrap` returns `token_required=true`, unauthenticated `/api/v1/status` returns `401`, and bearer-authenticated host power status succeeds
- `mains-aegis --ipc /tmp/mains-aegis-host-smoke.sock health` verified CLI to IPC health
- `mains-aegis --ipc /tmp/mains-aegis-host-smoke.sock devices list` verified CLI to IPC mock device list
- `mains-aegis-devd bridge-http --ipc /tmp/mains-aegis-bridge-smoke.sock --bind 127.0.0.1:<leased-port> --allow-dev-cors` verified HTTP bridge and CLI IPC share one daemon state by binding `mock-devkit` over HTTP and reading the same binding over CLI IPC
- Mock UI visual evidence captured at `/connect?seed=default`
