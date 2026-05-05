# Mains Aegis Device Daemon（#p8k3d）

## 状态

- Status: 已完成（v1 devd foundation）
- Created: 2026-05-02
- Last: 2026-05-02

## 背景 / 问题陈述

`mcu-agentd` 曾承担烧录、reset 与 defmt monitor，但 Web App 的 USB CDC 业务通信也需要独占同一 USB Serial/JTAG CDC 口。多个进程同时抢串口会导致日志、配网、状态读取和烧录互相干扰。

项目需要一个独立于 `mcu-agentd` 的本地设备入口：它可以管理多个硬件、为 Web UI 提供 HTTP/SSE API、托管生产 Web 静态资源，并用统一 Firmware Catalog 处理烧录与 defmt artifact 匹配。

## 目标 / 非目标

### Goals

- 新增 `tools/mains-aegis-devd`，作为 Mains Aegis 专用设备 daemon。
- `serve` 启动不接收设备端口；设备通过 API 扫描、列出、绑定、连接、断开和解绑。
- HTTP API 覆盖 identity、session、events、artifact selection、reset、monitor start/stop、flash 与 USB CDC safe settings 写入。
- 吸收旧本地 USB HTTP bridge 的兼容面：`/api/v1/serial/session`、WiFi config、log level 和 manual charge endpoints 由 devd 直接提供。
- Firmware Catalog 成为 Web Direct、devd、本地构建和 GitHub Release 的统一 artifact 合同。
- 固件 identity 暴露 build/profile/features/protocol/defmt 信息，devd 用它与 artifact manifest 匹配；不匹配时日志解码必须标记 `unverified`。
- Web 开发期由 Vite dev server 反代 `/api` 到 devd；生产期可由 devd 托管静态 Web。
- 新增项目 skill，固化 devd 设备操作、安全边界和验证流程。

### Non-goals

- 第一版不实现浏览器端完整 ESP ROM 烧录；Web Direct flash 只保留 catalog/client 边界。
- 不删除 `mcu-agentd.toml`；`mcu-agentd` 作为 legacy/fallback 保留。
- 不优化多设备并发烧录；v1 使用 per-device 状态与安全串行模型。
- 不在无硬件环境执行真机烧录、reset 或 monitor。

## 功能规格

### devd API

- `GET /api/v1/devices`: 返回当前已知设备与绑定。
- `POST /api/v1/devices/scan`: 枚举本机 serial candidates，只发现不自动连接。
- `POST /api/v1/devices/{id}/bind`: 为已知设备创建稳定绑定与别名。
- `POST /api/v1/devices/{id}/connect`: 连接设备并读取/缓存 identity。
- `POST /api/v1/devices/{id}/disconnect`: 断开设备 session。
- `DELETE /api/v1/devices/{id}/binding`: 移除绑定。
- `GET /api/v1/devices/{id}/identity`: 返回设备 firmware identity。
- `GET|POST /api/v1/devices/{id}/artifact`: 查询或选择 artifact manifest。
- `POST /api/v1/devices/{id}/flash`: 校验 artifact hash 后执行烧录；无硬件验证使用 `dry_run=true`。
- `POST /api/v1/devices/{id}/reset`: 设备 reset 请求。
- `POST /api/v1/devices/{id}/monitor/start|stop`: monitor 生命周期请求。
- `GET /api/v1/devices/{id}/session`: 返回 bounded logs/trace 与 `log_decode`。
- `GET /api/v1/devices/{id}/events`: 设备事件 SSE。
- `POST /api/v1/wifi-config` / `DELETE /api/v1/wifi-config`: 通过指定 `device_id` 的已连接 USB CDC 设备写入或清除 WiFi 配置，成功后返回固件 ack result；未指定 `device_id` 时仅允许单 USB 设备连接场景。
- `POST /api/v1/settings/log-level`: 通过指定 `device_id` 的 USB CDC session 更新日志级别。
- `POST /api/v1/settings/manual-charge`: 通过指定 `device_id` 的 USB CDC session 更新手动充电偏好。

### Firmware Catalog

- Canonical schema: `schemas/firmware-catalog.schema.json`。
- 本地生成脚本: `tools/firmware-artifact/build-catalog-entry.py`。
- Web fallback catalog: `web/public/firmware/firmware-catalog.json`。
- 每个 artifact 记录 `artifact_id/name/version/git_sha/build_id/target_chip/profile/features/protocol/defmt/files`。
- `files[].sha256` 必须在 devd flash 前重新校验。

### 固件身份

- HTTP identity 与 USB CDC `hello/get_identity` 的 `firmware` 字段必须包含：
  - `package_version`
  - `build_profile`
  - `build_id`
  - `git_sha`
  - `src_hash`
  - `git_dirty`
  - `features`
  - `protocol`
  - `defmt`
- devd 匹配规则：只有 `build_id`、build profile 与 feature set 都和 selected artifact 精确匹配才可标记 `log_decode.status=verified`；`git_sha` 只能作为 provenance 展示，不能单独证明 defmt artifact 匹配。

## 验收标准

- `tools/mains-aegis-devd` 能编译并通过单元测试。
- devd 可无端口启动，并通过 mock device 验证设备管理、artifact selection、dry-run flash 与 session API。
- `tools/firmware-artifact/build-catalog-entry.py` 能为 ELF 生成 manifest、catalog 和 `SHA256SUMS`。
- 固件 identity JSON 包含 features/protocol/defmt 字段。
- Web typecheck 通过，且 dev server proxy 将 `/api` 反代到 devd。
- 文档与 AGENTS guardrails 清晰说明 devd 是推荐入口，mcu-agentd 为 fallback。

## 实现状态

- `tools/mains-aegis-devd`: v1 daemon/API/mock validation foundation，并提供 Web App localhost USB safe-control surface。
- `schemas/firmware-catalog.schema.json`: v1 catalog schema。
- `tools/firmware-artifact/build-catalog-entry.py`: local manifest/catalog generator。
- `web/src/api/*`: devd mode client contracts。
- `firmware/src/net_contract.rs`: firmware identity metadata extension。
