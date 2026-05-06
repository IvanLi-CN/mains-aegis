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

## 2026-05-07

- 明确 devd 多设备原则：多个 USB CDC candidates 存在时，Web 必须展示候选列表并由用户选择，devd/Web 都不得自动决定控制哪台硬件。
- 明确 devd Web USB control lease：只有存在有效 Web lease 时 devd 才能占用设备；正常关闭立即释放，异常断开依靠短 TTL 自动释放，同时允许短暂网络抖动在 TTL 内恢复。
- 明确 USB 连接前 firmware artifact 匹配门禁：defmt raw/ignored 日志可作为普通控制台记录保留，但 Web Serial 与 devd 建立可写 session 前必须识别固件 artifact 不匹配，并要求用户显式忽略警告后才继续。
