# Web management UI（#ypfpu）

## 状态

- Status: 部分完成（5/5）
- Created: 2026-04-28
- Last: 2026-04-28

## 背景 / 问题陈述

- `mains-aegis` 已具备设备侧只读 `v1` HTTP API、mDNS / DNS-SD 与 `/api/v1/status` SSE 底座，但缺少浏览器侧管理界面。
- UPS 可能有多台硬件同时在线；单设备 Dashboard 不能作为唯一入口。
- 管理界面必须优先支持多设备快速扫视，同时保持首版只读，避免通过 Web 端引入远程状态改变风险。

## 目标 / 非目标

### Goals

- 新增独立 `web/` 管理端，使用 Vite + React + TypeScript + Bun。
- 使用根目录 `DESIGN.md` 的 Cohere 风格作为视觉基线。
- 实现多设备 Fleet 卡片网格，覆盖 online、offline、warning、critical、assist、backup 等状态。
- 实现设备接入页、单设备总览、电源路径、电池与 BMS、温度与保护、设备信息、API 调试页面。
- 对接设备侧现有只读接口：`/api/v1/ping`、`/api/v1/identity`、`/api/v1/network`、`/api/v1/status` 和 status SSE。
- 提供 mock fixtures，使无实机环境也能稳定预览与截图验证。

### Non-goals

- 不实现远程写控制、清故障、切输出、改充电策略或任何会改变 UPS 状态的操作。
- 不新增设备侧聚合 API；多设备汇总由浏览器端 `DeviceRegistry` 完成。
- 不做用户账号、鉴权、设备绑定、TLS、跨网段发现或云端服务。
- 不改造 `docs-site/`；文档站与管理端保持独立。
- 不引入 Storybook；当前仓库没有现成 Storybook 能力，视觉证据使用 mock UI + 本地预览。

## 功能规格

### Fleet 总览

- `/` 为默认入口，使用响应式设备卡片网格。
- 每张卡片固定展示设备别名/hostname、位置、在线状态、运行模式、最高告警、SOC、供电来源、负载是否供电、电池是否可用、是否需要处理、连接状态和 stale 时间。
- Fleet 卡片不得默认展示 OUT A/B、charger、pack voltage 等技术细节；这些字段保留在单设备详情与 API 调试页。
- 支持搜索设备、hostname、位置，并支持 `all / critical / warning / offline` 过滤。
- 排序规则：Critical 优先，Warning 次之，Info/OK 在后，Offline 保留并显示 stale 时间。

### 设备管理

- `/connect` 支持手动添加 `.local` hostname、IP 或完整 URL。
- 添加时按 `ping -> identity -> network -> status` 探活；失败显示 API-compatible error envelope。
- 浏览器侧保存 `DeviceRegistry` 到 `localStorage`，并提供 demo fleet reset。

### 单设备详情

- `/devices/:device_id` 展示单设备运行状态带与关键摘要。
- `/devices/:device_id/power` 展示 input、charger、output gate、OUT A/B。
- `/devices/:device_id/battery` 展示 pack status、BMS readiness 与 issue detail。
- `/devices/:device_id/thermal` 展示 TMP A/B 与保护上下文。
- `/devices/:device_id/device` 展示 identity、network、firmware。
- `/devices/:device_id/api` 展示固定只读 endpoints 与当前 JSON snapshot。

## 接口与数据流

- Web 端以 `docs/specs/amc32-wifi-service-discovery-api-foundation/contracts/http-apis.md` 为只读 API 契约来源。
- `DeviceRegistry` 保存 `device_id -> base_url -> latest snapshot -> connection state`。
- 在线设备优先使用 `EventSource` 订阅 `/api/v1/status`；SSE 失败后关闭 stream 并回退到 `GET /api/v1/status` 轮询。
- mock 设备使用 `mock:` base URL，不发真实网络请求，供视觉验证和开发预览使用。
- 所有错误统一映射为 `{ code, message, retryable, details }`，页面不得拼接非契约形状错误。

## 验收标准

- `bun install` 成功，并保留根项目 commitlint workflow。
- `bun run web:check` 通过。
- `bun run web:build` 通过。
- Fleet mock 页至少显示 6 台设备，覆盖 standby、assist、backup、warning、critical、offline。
- `/connect` 能显示已保存设备，支持添加设备与探活错误显示。
- 单设备详情页可从 Fleet 卡片进入，并展示 power、battery、thermal、device、api 子页。
- 浏览器视觉验证覆盖 desktop Fleet、mobile Fleet 和单设备 Dashboard。

## 文档更新

- `DESIGN.md`: Cohere 设计基线。
- `docs/web-management-ui.md`: 管理端信息架构与实现结构。
- `docs/README.md`: 增加 Web management UI plan 入口。
- `docs/specs/README.md`: 增加当前 spec 索引。

## Visual Evidence

视觉证据由 `web/` mock UI 本地预览生成；按本轮计划，截图只回传给主人验收，不提交截图资产或 PR 图片引用，除非主人明确批准。

## 实现里程碑

- [x] M1: 安装 Cohere `DESIGN.md` 并建立 Web 管理端规划。
- [x] M2: 新增 `web/` Vite + React + TypeScript + Bun 应用骨架。
- [x] M3: 完成多设备 Fleet 卡片网格、设备管理与单设备详情页。
- [x] M4: 完成只读 API/SSE 客户端、mock fixtures、类型检查、生产构建和 mock UI 视觉验证。
- [x] M5: 创建 PR #71 并完成快车道 review / CI 收敛到 merge-ready。
