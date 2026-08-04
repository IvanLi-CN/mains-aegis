# Implementation（lan-management-convergence）

## 当前结论

- 设备本体 API 是 LAN 管理真相源；Web 无 devd 路径与 devd 走 LAN 路径都必须消费同一组设备端点。
- `session` / `safeSettings` 全局废弃；新的信息架构固定为 `connection / identity / status / settings / trace`。
- `connection` 只属于 devd / Web / CLI 层，不属于设备本体 API。
- USB `bind` 成功后，devd 会执行一次只读 companion-LAN 探测：先读取 USB `identity`，必要时补 `get_status.network`，再用 `device_id` 校验 mDNS 与 `IP:Port` 是否都命中同一硬件。
- 当前不支持的设备本体 LAN 能力（如 LAN flash、LAN monitor/defmt）不纳入首版目标面。
- LAN 日志能力以结构化 HTTP client trace 交付，不追求等价于 USB monitor/defmt。

## 当前 PR 归属

- Active branch: `th/mains-aegis-host-tools-alignment`
- Active PR: [#80 feat(host-tools): align CLI and devd host tooling](https://github.com/IvanLi-CN/mains-aegis/pull/80)
- 结论：**本规格默认在当前 PR #80 上渐进推进**。当前阶段先落规格、契约和迁移顺序；后续若 diff / review 面失控，再由主人显式决定是否拆分 PR。

## 分阶段推进

### Phase 0: 规格与契约先行

- 落地本规格与历史 spec 回链。
- 明确“设备本体 API 优先”的实施顺序。
- 把 `session` / `safeSettings` 标记为废弃概念，阻止后续实现继续围绕它们扩张。

Exit:

- 新规格、旧规格回链、spec index 完整。
- 当前 PR #80 的实施顺序写清楚，后续实现不再需要重复设计辩论。

### Phase 1: 设备本体 API 真相层

- 在 firmware 侧补齐 `GET /api/v1/settings`。
- 设备本体 LAN 写接口落地为 `POST|DELETE /api/v1/wifi-config`、`POST /api/v1/settings/log-level`、`POST /api/v1/settings/manual-charge`、`POST /api/v1/reset`。
- LAN 写接口由 HTTP worker 接收并进入主循环 pending command 队列，真实执行复用现有 USB CDC / PowerManager 写入路径，避免在 HTTP worker 里直接触碰硬件资源。
- 更新 `wifi-service-discovery-api-foundation` 契约文档与 host-side tests。

Exit:

- 设备本体 API 可以单独支撑 Web 无 devd 场景的 LAN 管理。
- 设备 API 的字段和错误 envelope 已稳定，host/web 不再需要额外私有语义。

Status:

- `GET /api/v1/settings` 已实现并通过 `cargo +esp check`。
- LAN settings 写接口已实现为 `202 Accepted` 队列语义；`reset` 需要 `{"confirm":"reset"}`。
- `firmware/host-unit-tests` 覆盖 reset confirmation parser 与 USB CDC settings parser。

### Phase 2: devd / CLI 收敛

- devd 新增 LAN discovery、LAN transport、scan run trace、按 `device_id` 聚合的 logical device 模型。
- CLI 移除 `device session` 用户命令面。
- CLI 查询面收敛为 `connection / identity / status / settings / trace`。
- 新增 owner-facing `trace` 命令族；`connection` 查询返回 transport 选择上下文。

Exit:

- devd/CLI 不再把 `session` 作为用户概念。
- devd LAN 路径已是设备本体 API 的客户端，而不是另一套 LAN 真相源。

Status:

- devd `DeviceTransport` 已增加 `lan` record；`DeviceRecord` 暴露 `lan_address` 与 `lan_conflict_addresses`。
- devd `devices.scan` 已按 `mDNS/DNS-SD -> CIDR/default routed /24` 发现 LAN 设备，只以 `GET /api/v1/identity` 作为命中判据，并返回 bounded `scan_trace`。
- LAN identity probe 的 HTTP `tx/rx` 摘要会写入对应 device trace；同一 `identity.device_id` 的 LAN record 会并入已有 USB logical device，默认保持 USB 为 active transport。
- `device.bind` 现在会返回只读 `companion_lan_candidate`；候选只存在于运行态，不写入状态文件。只有显式 `POST /api/v1/devices/{id}/companion-lan` 或 CLI `device <id> companion-lan bind` 才会把 `lan_companion { mdns_host, ip, port, confirmed_at, last_verified_at }` 写入绑定记录。
- 当 `lan_conflict_addresses` 非空，或 mDNS / `IP:Port` 不能同时验证到同一 `device_id` 时，companion-LAN 持久化会被阻断；devd 继续保留 USB owner，不自动切换 active transport。
- settings 写路径已按 target transport 分流：Web USB lease 继续走 USB CDC；无 lease 的 devd/CLI settings 目标可在 LAN-only 或 USB 不可用时走设备本体 LAN API，并在写成功后重新读取 `GET /api/v1/settings` 快照。
- `device.connection` 已返回 USB/LAN/mock reachability、active/connected 状态、last_error 与 transport switch hint。
- `device.trace` 继续保留兼容 tail，同时返回 `transports.usb` / `transports.lan` 分组；scan trace 与 device trace 已进入 bounded devd state persistence。
- CLI `devices scan` 已支持 `--cidr`、`--no-lan`、`--no-mdns`；`devices scan-trace` 可查询最近 scan trace。
- Status: complete for M3.

### Phase 3: Web 收敛

- Web LAN 直连接入新的设备本体 `settings` 读接口与 LAN 写接口。
- Connect / Settings / DeviceRegistry / Trace UI 切到新的 `connection / settings / trace` 模型。
- `safeSettings` 回填、LAN 只读假设、依赖 `serial/session` 的 UI 逻辑全部移除或改造成 Web 内部快照机制。

Exit:

- Web 无 devd 与 devd transport 使用同一信息架构。
- LAN 与 USB 通过 `device_id` 合并成同一 logical device，但 trace 仍按 transport 分开查看。

Status:

- Web direct LAN records now read `GET /api/v1/settings` and write the device API endpoints for WiFi config, log level, and manual charge preferences; each successful write refreshes the full settings snapshot.
- Web devd records can use USB lease-backed control or devd-discovered LAN transport; LAN-only devd writes omit `lease_id` and refresh `/api/v1/devices/{id}/settings` after success.
- Connect no longer labels LAN as read-only and lists devd USB/LAN candidates without auto-selecting among multiple devices.
- GitHub Pages/public-static Connect now runs as an explicit browser-direct LAN surface: no default same-origin devd discovery, no implicit `/api/v1/devices` polling, Chrome 142+ + secure context gating, and manual IPv4 CIDR discovery that only persists devices after explicit user confirmation.
- Settings no longer requires USB when a LAN/devd settings snapshot is present.
- Status: complete for M4.

### Phase 4: 清理与收口

- 更新 `docs/usb-cdc-web-serial-protocol.md`、`docs/web-management-ui.md` 等人类文档。
- 收口旧兼容接口与历史表述。
- 补齐最终验证、视觉证据与 PR 说明。

Exit:

- 规格、实现、文档、CLI 命令面与 Web 交互模型一致。

Status:

- USB CDC hello capability uses `settings` instead of legacy `safe_settings`; Web types and protocol docs match.
- `/api/v1/serial/session` remains only as the explicitly documented Web compatibility snapshot; owner-facing query names are `connection / identity / status / settings / trace`.
- Final Connect visual evidence is stored in this spec under `## Visual Evidence`.
- Status: complete for M5.

## 实施约束

- 不得先让 devd 或 Web 发明新的 LAN 私有语义，再倒逼 firmware 追认。
- 不得把“当前控制台快照”重新包装成用户可见 CLI 命令。
- 不得为了兼容旧 `session` 语义而继续让 `logs / trace / settings` 混在一个 owner-facing 接口里。

## 验证状态

- `cargo test --manifest-path firmware/host-unit-tests/Cargo.toml usb_cdc_protocol`
- `cd firmware && cargo +esp check`
- `cargo test --manifest-path tools/mains-aegis-host/Cargo.toml`
- `bun run web:check`
- `git diff --check`

## Migrated Implementation Record

- Status: 已完成（5/5）
- Created: 2026-06-03
- Last: 2026-06-03

- [x] M1: 冻结新的设备本体 API 契约，明确 `settings` 读接口与 LAN 写接口边界
- [x] M2: 固件实现设备本体 API 真相层，补齐 host-side 契约测试
- [x] M3: devd 增加 LAN discovery / transport / trace / `connection` 查询面，并从 CLI 用户命令面移除 `device session`
- [x] M4: Web 改接新的 `connection / settings / trace` 模型，移除 `safeSettings` 与 LAN 只读假设
- [x] M5: 清理历史兼容叙述与旧接口，完成文档、验证和视觉证据收口

## References

- `./SPEC.md`
- `./HISTORY.md`
