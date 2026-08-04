# Implementation（client-transport-priority）

## 状态

- Status: 部分实现

## 当前覆盖

- `web/src/device-registry/DeviceRegistry.tsx`
  - confirmed companion 后把 `rememberedChannels.http.baseUrl` 记为 `http://<hostname_fqdn>`。
  - 同时保留 `fallbackBaseUrl=http://<ip>:<port>`。
  - 当前 remembered channel 仅保存 `seenAt`，尚未为每种连接方式单独保存 `last_connected_at` 与 `last_connect_attempt_at`。
- `web/src/app/App.tsx`
  - Web 直连 base URL 选择顺序为 `hostname_fqdn > hostname > ip:port`。
  - 当前 channel 选择主要依赖 `preferredTransport`，尚未按“上次成功连接的方案”统一排序。
- `tools/mains-aegis-host`
  - companion-LAN 候选必须同时通过 `http://<hostname_fqdn>/api/v1/identity` 与 `http://<ip>:80/api/v1/identity` 验证后，才可进入 confirm 路径。
- CLI / devd
  - 默认 transport 仍为 `USB`；LAN 不自动抢占默认 owner path。

## 已知边界

- 本规格只定义优先级与 remembered channel 记忆字段，不重复记录 Web prompt、companion confirm UX、或 devd API 全量命令面。
- `last_connected_at` / `last_connect_attempt_at` 的 per-channel 落盘与基于它们的默认排序仍待实现对齐。

## Migrated Implementation Record

- Status: 已完成
- Created: 2026-06-08
- Last: 2026-06-08

## References

- `./SPEC.md`
- `./HISTORY.md`
