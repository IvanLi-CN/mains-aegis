# WiFi / 服务发现 / 只读 API 底座 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Lifecycle: active
- Implementation: 见下方迁移状态与覆盖记录。

## Migrated Implementation Record

- Status: 部分完成（4/5）
- Created: 2026-04-09
- Last: 2026-06-03

- Directory: `docs/specs/wifi-service-discovery-api-foundation/assets/`
- In-spec references: `![...](./assets/<file>.png)`
- Visual evidence source: maintain `## Visual Evidence` in this spec when owner-facing or PR-facing screenshots are needed.

None。

- [x] M1: 新增 `net_http` feature、编译期 WiFi env 注入与 feature-gated 主入口
- [x] M2: 实现共享网络状态模型、WiFi 连接任务、mDNS / DNS-SD 与只读 HTTP/SSE 底座
- [x] M3: 抽出 `UpsStatusSnapshot` / `NetworkUiSummary`，补齐 host-side 契约测试并接入只读 API / SSE 桥接
- [x] M4: 补齐 Dashboard / WiFi 相关视觉证据（不修改自检页面）
- [ ] M5: 完成 fast-track 提交、push、PR 与 review-loop 收敛到 merge-ready

## References

- `./SPEC.md`
- `./HISTORY.md`
