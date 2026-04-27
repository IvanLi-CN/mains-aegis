# Web management UI Implementation（#ypfpu）

## 当前状态

- `web/` 新增独立 Vite + React + TypeScript + Bun 应用。
- 根 `package.json` 增加 workspace 与 `web:dev` / `web:preview` / `web:check` / `web:build` 脚本。
- `DeviceRegistry` 维护浏览器侧设备清单、localStorage 持久化、只读探活、SSE 订阅与轮询兜底。
- `mock:` 设备用于稳定开发预览和视觉证据，不发真实网络请求。
- 管理端页面已覆盖 Fleet、Connect、Overview、Power、Battery、Thermal、Device、API。

## 验证状态

- `bun install`: 已通过。
- `bun run web:check`: 已通过。
- `bun run web:build`: 已通过。
- 本地预览：已通过端口租约启动 `web-preview`。
- 浏览器验证：已确认 Fleet、Connect 和单设备 Dashboard 可渲染，控制台无 warn/error。
- 视觉证据：已生成 desktop Fleet、mobile Fleet、single-device Dashboard 的 mock UI 截图并回传给主人；按计划不提交截图资产。

## 剩余工作

- 创建 PR。
- 完成 fast-track review-loop / CI 收敛。
- PR 达到 merge-ready 后，将 spec 状态更新到最终收口状态。
