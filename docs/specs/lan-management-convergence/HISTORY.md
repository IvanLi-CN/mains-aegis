# History（lan-management-convergence）

## 2026-06-03

- 决定设备本体 API 是 LAN 管理真相源；Web 无 devd 与 devd 走 LAN 都必须消费同一组设备端点。
- 决定 LAN 提供所有当前已支持的功能；设备本体尚未支持的能力（如当前的 LAN flash、LAN monitor）不强行放进首版目标面。
- 决定 Web 无 devd 场景必须支持手填 IPv4 CIDR 的子网扫描；devd 侧优先 mDNS/DNS-SD，再补子网扫描。
- 决定扫描只探测 `GET /api/v1/identity`，并以 `identity.device_id` 作为唯一主键；同一 `device_id` 出现在多个 IP 上时阻断自动接入。
- 决定同一 `device_id` 的 USB 与 LAN 关联为同一 logical device；默认首选 USB，切换 transport 需要显式提示，必要时可以硬阻断要求切换到 USB。
- 决定全局废弃 `session` / `safeSettings` 概念；新的查询面固定为 `connection / identity / status / settings / trace`。
- 决定 `connection` 属于 devd / Web / CLI 层，不进入设备本体 API；`settings` 则进入设备本体 API，提供完整快照读接口。
- 决定 LAN 日志能力接受为结构化 HTTP client trace；Web 与 devd 都需要设备级 trace 与 scan run trace 的 bounded 持久化。
- 决定本规格默认在当前 PR #80 上渐进推进，先做规格与契约收敛，再逐步推进 firmware、devd/CLI、Web 的实现改接。

## Completion

- Web devd 入口同时接受 USB CDC 与无冲突 LAN transport 候选；LAN transport 不创建 Web USB lease，settings 写入通过 devd 的设备级 LAN client 路径完成。
- `/api/v1/serial/session` 仅作为 Web USB Console 兼容快照保留；用户可见与新实现查询面固定为 `connection / identity / status / settings / trace`。
- USB CDC hello capability 命名改为 `settings`，不再继续传播 `safe_settings` 旧命名。
- GitHub Pages/public-static 路径明确冻结为 browser-direct LAN：扫描只在用户点击后执行，发现结果只保留在当前 session，且不满足 `Chrome 142+` + secure context 时不再尝试直连动作。

## Decision Trace

- 2026-06-03: 新建规格，接管 LAN 只读假设、`session/safeSettings` 历史模型与 USB/LAN transport 收敛设计。
- 2026-06-03: 设备本体 `settings` 读取与 LAN settings 写接口落地；HTTP 契约补齐为 `202 Accepted` 队列语义。
- 2026-06-03: devd `devices.scan` 增加 LAN discovery、CIDR/default subnet probe、scan trace 返回与 `identity.device_id` 逻辑归并；settings 写路径可在无 Web USB lease 时走设备 LAN API。
- 2026-06-03: M3 完成；devd `connection` 返回 per-transport reachability/switch hint，device trace 返回 USB/LAN 分组，scan/device trace 进入 bounded state persistence。
- 2026-06-03: M4/M5 完成；Web 直连 LAN 与 devd LAN transport 可读写设备 settings，Connect/Settings 移除 LAN 只读与 `safeSettings` 假设，USB CDC hello capability 改为 `settings`，最终验证与视觉证据已收口。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
