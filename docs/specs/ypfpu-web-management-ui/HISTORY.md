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
- 新增 Firmware 页面，统一展示 catalog 来源、去重结果、匹配 artifact、烧录方式与进度摘要。
- 统一固件来源聚合策略：Web 静态资源与 GitHub Releases 共同入库，按 `artifact_id` 去重，Web bundled 优先，release 重复项标记为 bundled override；`artifact_id` 由 `build_id` 参与生成，避免 dirty/local build 与 clean release build 误去重。
- 扩展 artifact 合同，为 `kind="image"` 文件增加 `flash_address`，浏览器端 Web Serial 仅接受可烧录 image，ELF 继续保留给 devd / defmt 解码。
- Web Serial 路径采用 `esptool-js@0.6.0`，devd 路径复用既有绑定与 `flash` 编排，并要求真实烧录前显式确认。

## 2026-05-07

- 明确 devd 多设备原则：多个 USB CDC candidates 存在时，Web 必须展示候选列表并由用户选择，devd/Web 都不得自动决定控制哪台硬件。
- 明确 devd Web USB control lease：只有存在有效 Web lease 时 devd 才能占用设备；正常关闭立即释放，异常断开依靠短 TTL 自动释放，同时允许短暂网络抖动在 TTL 内恢复。
- 明确 USB 连接前 firmware artifact 匹配门禁：defmt raw/ignored 日志可作为普通控制台记录保留，但 Web Serial 与 devd 建立可写 session 前必须识别固件 artifact 不匹配，并要求用户显式忽略警告后才继续。
- 固件 workflow 现已真实发布 GitHub Release 资产：`firmware-catalog.json` 与同批 artifact 文件会在 `push` 到 `main` 时随 commit SHA release 一并发布，供 Web App 的 release catalog 读取。

## 2026-06-07

- hosted/self-hosted devd Connect UI 收敛为 discovery-only：不再重复渲染 Web Serial 与手动 LAN fallback 面板。
- 明确 devd discovery 的连接语义：USB 候选通过 devd Web lease / usb-http bridge 接入；LAN 候选只借用 devd 发现地址，实际保存为直连硬件 HTTP target。
- 修正 Connect 页 owner-facing 动作文案：未纳管 discovery 候选改用 `Bind USB` / `Add WiFi`，已纳管设备才显示 `Open` 与 `Use ...`，避免把 discovery 阶段误写成“连接功能”。
- 修正 `Bind USB` 的真实绑定语义：当 USB 还未读出 identity 时，允许先把 stable USB id 绑定到已有 logical device，并在 discovery / remembered channels 中通过 `binding.logical_device_id` 归并回同一设备；绑定本身不再冒充“立即进入设备”。
- Fleet 视图改为读取混合模型：浏览器本地记录与当前 devd discovery 必须按 logical device 合并展示；仅 live discovery 的设备允许出现在 Fleet 中，但默认回到 Connect 完成纳管，不再依赖“先保存、后可见”的旧前提。
- 决定 USB 绑定后若 devd 返回 `companion_lan_candidate`，Web 必须在同一卡片内就地显示 `Bind LAN companion`，确认后才把 `mdnsHost + IP:Port` 写入本地记录；未确认 candidate 不能自动升级成 `Use WiFi`。

## 2026-06-08

- companion-LAN 的默认直连优先级改为统一引用 [`#rzx5v`](../rzx5v-client-transport-priority/SPEC.md)；本规格只保留 Web 侧保存字段与交互约束，不再重复维护跨客户端矩阵。
