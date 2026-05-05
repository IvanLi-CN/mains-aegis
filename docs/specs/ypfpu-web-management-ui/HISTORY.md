# Web management UI History（#ypfpu）

## 2026-04-28

- 选择多设备优先的信息架构：默认入口为 Fleet 卡片网格，单设备详情位于 `/devices/:device_id/*`。
- 保持首版只读：Web 端只消费设备侧 `v1` API / SSE，不新增写控制和设备侧聚合 API。
- 选择独立 `web/` 应用而不是复用 `docs-site/`，避免文档站与运维台职责混合。
- 使用 `DESIGN.md` 的 Cohere 风格作为视觉基线，但管理端以可扫描、稳定、低装饰的产品 UI 为主。
- 视觉证据采用 mock UI + 本地预览；截图只用于 owner-facing 验收，不作为 PR 图片资产提交。

## 2026-05-05

- 删除独立 USB HTTP bridge 路径，统一由 `mains-aegis-devd` 承担 localhost USB 控制面。
- 以固件 `identity.device_id` 作为同一硬件判定键；LAN 与 USB 同时存在时在 Web App 内合并成一条设备记录，并同时展示 WiFi 与 USB 连接标记。
