# Implementation（#7jqrq）

## 当前实现

- `tools/mains-aegis-host` 统一产出 `mains-aegis` CLI 与 `mains-aegis-devd` daemon。
- `mains-aegis` CLI 业务命令默认通过系统原生 IPC 复用 singleton daemon；连接失败时会定位 packaged sibling `mains-aegis-devd` 并按需 auto-start IPC daemon。`--no-auto-start` 会保持纯连接失败语义。
- `mains-aegis daemon serve` 为 developer/debug foreground IPC daemon；`mains-aegis daemon http` 为显式 HTTP/Web 服务，并在同一进程内启动共享状态的 IPC listener，供 CLI 与 Web 观察同一个 daemon 状态。
- CLI 通过 newline JSON IPC 调度设备、serial lease、settings、artifact、flash/reset/monitor 与 host power 命令族；新的查询面使用 `connection / identity / status / settings / trace`。
- CLI `device <id> bind` 在交互式 TTY 下会在 USB bind 成功后提示是否同时保存 LAN companion；非交互场景只返回候选结果和显式后续命令，不自动持久化。
- hosted `mains-aegis daemon http` 默认从 `web/dist` 嵌入静态产物，向 HTML 注入 app-session secret 与运行模式 meta，并要求后续 API 请求携带 `x-mains-aegis-app-session`；SSE 使用 `app_session` query 参数。缺少 `web/dist` 时，host-tools 构建会 fail fast 并提示先执行 `bun run build`。
- `mains-aegis daemon http --allow-dev-cors` 切换到 API-only 开发模式：只允许 loopback HTTP origins 跨源访问 `/api`，根路径返回 API-only 提示页，不托管嵌入式 hosted app。
- 非 loopback `mains-aegis daemon http` 启动时要求 `--allow-lan-bridge` 与 `--auth-token-file`，API 请求仍接受 bearer token；`GET /api/v1/bootstrap` 保持免认证并报告真实 token requirement；浏览器 EventSource 兼容 `service_token` 与 legacy `bridge_token` query 参数授权。
- Host-tools release workflow 产出三平台 archive、安装脚本和可直接校验 release assets 的 `SHA256SUMS`；手动 release 在 tag 已存在时按该 tag 重建资产、tag 不存在时从当前 dispatch commit 创建新 release tag；`MAINS_AEGIS_RELEASE_VERSION` 同时驱动构建产物与 `mains-aegis` / `mains-aegis-devd` 的 `--version` 输出。
- Hosted Web client 从 same-origin HTML meta 读取 app-session secret，并且只向 devd HTTP service API 与 devd EventSource 请求附加该 secret；普通 LAN 目标不携带 app-session。Connect UI 不再暴露 devd URL 或 token 输入入口；hosted/self-hosted 模式只显示 devd discovery，USB 通过 devd 接入，LAN 设备则以直连硬件 HTTP target 落盘。
- Connect discovery 的 owner-facing 动作语义与 devd 持久绑定保持一致：新发现 USB 候选先执行 `Bind USB`，新发现 LAN 候选执行 `Add WiFi`；只有浏览器内已存在的设备记录才显示 `Open` 与 transport 切换动作。
- host-tools 新增 `/api/v1/devices/{id}/companion-lan` 与对应 IPC/CLI `device <id> companion-lan bind|clear`，把“自动发现”和“持久绑定”分成两步；devd 持久保存 mDNS + 最近成功 `IP:Port`，但默认仍保持 USB-first owner 语义。
- host-tools 新增 devd HTTP `POST /api/v1/devices/{id}/recovery/bms-discharge-authorization`、IPC `device.recovery.bms_discharge_authorization` 与 CLI `device <id> recovery bms-discharge-authorization`。native serial 设备只有显式绑定 companion LAN 时才优先尝试设备 LAN HTTP，失败后回退 USB CDC；无 companion LAN 时只走绑定 USB CDC，不把缓存的 `identity/status.network.ipv4` 当作恢复写目标；LAN 设备直接走设备本体 HTTP；mock 设备返回结构化 rejected。devd 会等待固件非 `pending` 终态并刷新 status/diag cache。
- Repo skill routing defaults Codex work inside this repository to `$mains-aegis-devd-flow` for development, validation, diagnostics, field investigation, and hardware read/session-read checks. `$mains-aegis-user-operations` remains the explicit end-user/released host-tools route.

## 验证

- `cargo check --manifest-path tools/mains-aegis-host/Cargo.toml`
- `cargo check --manifest-path tools/mains-aegis-host/Cargo.toml --all-targets`
- `cargo test --manifest-path tools/mains-aegis-host/Cargo.toml`
- `bun run --cwd web check`
- `mains-aegis device <id> recovery bms-discharge-authorization` verified against the low-voltage recovery fixture, returning firmware terminal recovery JSON and not bypassing firmware gates.
- `mains-aegis daemon serve --help` verified IPC-only help without `--bind`
- `mains-aegis daemon http --bind 0.0.0.0:30080` verified non-loopback denial without token
- `mains-aegis daemon http --auth-token-file <file>` verified unauthenticated `/api/v1/bootstrap` returns `token_required=true`, unauthenticated `/api/v1/status` returns `401`, and bearer-authenticated host power status succeeds
- `mains-aegis --ipc /tmp/mains-aegis-host-smoke.sock health` verified CLI to IPC health
- `mains-aegis --ipc /tmp/mains-aegis-host-smoke.sock devices list` verified CLI to IPC device listing, including the empty-daemon case without synthetic runtime devices
- `mains-aegis daemon http --ipc /tmp/mains-aegis-http-service-smoke.sock --bind 127.0.0.1:<leased-port> --allow-dev-cors` verified HTTP service and CLI IPC share one daemon state without requiring a synthetic runtime mock device
- Mock UI visual evidence captured at `/connect?seed=default`
- Policy search verifies AGENTS, skills, and specs no longer make released host tools the default Codex route inside this repository.
