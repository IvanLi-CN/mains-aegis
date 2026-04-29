# Web management UI Implementation（#ypfpu）

## 当前状态

- `web/` 新增独立 Vite + React + TypeScript + Bun 应用。
- 根 `package.json` 增加 workspace 与 `web:dev` / `web:preview` / `web:check` / `web:build` 脚本。
- `DeviceRegistry` 维护浏览器侧设备清单、localStorage 持久化、只读探活、SSE 订阅与轮询兜底。
- `mock:` 设备用于稳定开发预览和视觉证据，不发真实网络请求。
- 管理端页面已覆盖 Fleet、Connect、Overview、Power、Battery、Thermal、Device、API。
- Fleet 卡片使用用户可理解的摘要字段，技术细节保留到单设备详情与 API 调试页。
- Demo 复用正式前端路由，通过 `seed` 参数切换 mock 数据场景，覆盖默认 fleet、空数据、全离线、大数量、Critical Battery、Backup、API Debug 等路径。

## 验证状态

- `bun install`: 已通过。
- `bun run web:check`: 已通过。
- `bun run web:build`: 已通过。
- Storybook：已从 Demo 工作流移除。
- 本地预览：已通过端口租约启动 Vite mock-data 前端。
- 浏览器验证：已确认 Fleet、Connect 和单设备 Dashboard 可渲染，控制台无 warn/error。
- 视觉证据：已生成 desktop Fleet、mobile Fleet、empty Fleet、large Fleet、single-device Dashboard 的 mock UI 截图；截图已回传给主人，并作为 spec assets 落盘供 owner-facing review 使用。
- Review-loop：已通过，未发现剩余可操作问题。
- PR #71 CI：当前分支推送后以 GitHub checks 最新结果为准。

## PR 状态

- PR: https://github.com/IvanLi-CN/mains-aegis/pull/71
- Stop condition: Step 5C Ready，等待主人确认后再合并。
