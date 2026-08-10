# 主动告警逐实例消音

> 当前有效规范以本文为准；实现覆盖见 [`IMPLEMENTATION.md`](./IMPLEMENTATION.md)，关键决策见 [`HISTORY.md`](./HISTORY.md)，跨端合同见 [`contracts/alerts.md`](./contracts/alerts.md)。

## 背景

设备已经能对运行期异常播放 `SYSTEM` 提示音，但用户不能在不解除告警的情况下停止当前实例的重复提示。该能力必须由固件维护权威状态，CLI、Web App 与前面板只操作同一个当前实例，不能各自推断或持久化静音状态。

## 目标与边界

### Goals

- 为每个活动告警实例维护稳定的 `alert_id + instance_id`、严重度和声音状态。
- 覆盖以下 9 类当前已有运行期信号：`mains_absent_dc`、`high_stress`、`battery_low_no_mains`、`battery_low_with_mains`、`shutdown_protection`、`io_over_voltage`、`io_over_current`、`module_fault`、`battery_protection`。
- 允许每个活动实例独立消音；解除后自动清除静音，复发生成新实例并可重新提示。
- 通过 USB CDC、LAN HTTP、devd、CLI、Web App 与前面板呈现同一权威状态。
- 前面板复用固件 scene、字体和 `320x172` little-endian RGB565 framebuffer 导出评审图。

### Non-goals

- 不提供批量消音、手动恢复声音、跨实例或跨重启的永久静音，也不写入 EEPROM。
- 不修改告警阈值、优先级、音频资产、全局 `ACTION/SYSTEM` 音量或市电冷启动静默策略。
- 不为没有运行期信号的 `IoOverPower` 虚构活动告警。
- 不在前面板图片获主人批准前接入运行时消音、CDC/LAN/devd、CLI 或 Web 写路径。

## 告警目录与生命周期

| `alert_id` | 默认严重度 | 说明 |
| --- | --- | --- |
| `mains_absent_dc` | `warning` | 市电丢失，设备转入电池供电路径 |
| `high_stress` | `warning` | 热/负载压力超出正常工作带 |
| `battery_low_no_mains` | `warning` | 无市电时电池低电量 |
| `battery_low_with_mains` | `warning` | 有市电时电池低电量或充电路径异常 |
| `shutdown_protection` | `critical` | 输出保护进入停机/限制态 |
| `io_over_voltage` | `critical` | I/O 输出过压 |
| `io_over_current` | `critical` | I/O 输出过流 |
| `module_fault` | `critical` | 已接入模块故障 |
| `battery_protection` | `critical` | BMS 电池保护生效 |

- `inactive -> active` 时，固件为该 `alert_id` 创建新的不透明 `instance_id`。
- `active -> muted` 只改变该实例的提示音抑制状态；告警严重度、保护行为和告警可见性不变。
- `active|muted -> cleared` 时，实例从活动集合移除；前面板详情页可在当前会话中保留 `CLEARED` 终态，但不会保留可操作的静音。
- 下一次 `inactive -> active` 必须产生新 `instance_id`。任意使用旧 `instance_id` 的消音请求不得影响新实例。
- 消音状态仅在 RAM 中存在；重启后没有消音恢复状态。

### 声音状态

活动实例对外报告一个解析后的 `sound_state`：

- `audible`：目标 `SYSTEM` cue 可以播放。
- `muted`：当前实例被用户消音。
- `system_silent`：全局 `SYSTEM` 音量为零。
- `policy_silent`：现有冷启动无市电策略抑制该 cue。

`policy_silent`、`system_silent` 和 `muted` 都不改变告警本身。全局/策略抑制期间发出的消音请求仍记录到当前实例，使全局抑制解除后该实例不会重新发声。

## 跨端行为

- 消音请求必须携带 `alert_id` 与当前 `instance_id`；固件先比对活动实例，再停止或丢弃仅属于该实例的提示 cue。
- 已解除实例返回确定的 `inactive` 结果，过期 `instance_id` 返回 `stale`，二者都不得影响任何新实例。
- 旧固件必须明确返回 `unsupported`；客户端不得从遥测结果猜测可写能力。
- 所有读写合同、HTTP 状态和 CLI 解析规则由 [`contracts/alerts.md`](./contracts/alerts.md) 固化。

## 前面板交互

### Dashboard

- WiFi 图标右侧显示与 WiFi glyph 视觉尺寸一致的 `14px` 告警三角。
- 存在至少一个 `audible` 告警时，三角按最高严重度在白色与严重度色之间双相交替；`warning` 使用黄色，`critical` 使用红色。
- 仅存在 `muted`、`system_silent` 或 `policy_silent` 告警时，三角以最高严重度色静态显示。
- 没有活动告警时不显示三角；点击三角进入 `ALERTS` 列表。

### 列表与详情

- 列表项显示三角、告警摘要、严重度与声音图标。可听告警显示声音图标；被抑制的项显示带斜杠声音图标并用状态文本区分 `MUTED`、`SYSTEM SILENT`、`POLICY SILENT`。
- 触摸行主体进入详情；触摸右侧声音图标只消音当前项。列表支持空态、单项、多项及首/中/末溢出位置。
- 物理按键：`UP/DOWN` 选择、`CENTER` 进入详情、`RIGHT` 消音当前可听项、`LEFT` 返回。
- 详情页显示摘要、`SOUND` 状态与单个 `MUTE THIS ALERT` 动作。解除后的详情显示 `CLEARED` 且不提供动作。

### 评审门禁与矩阵

- 预览必须由 `firmware/src/front_panel_scene.rs` 同源渲染，并为每个场景输出 `320x172` PNG 与 `110080` 字节 little-endian RGB565 framebuffer。
- 评审矩阵包含：首页无告警、warning/critical 双相、`muted`、`system_silent`、`policy_silent`、mixed 及 Dashboard 告警入口热区；列表空/单/mixed/溢出首中末及逐行详情/消音热区；9 类详情的 active、muted、cleared，以及详情触摸区。
- 主人明确批准 Chat 中展示的不可变快照前，任何运行时、协议、CLI 和 Web 消音实现均不得开始。

## 验收标准

- 9 个告警类型均通过 `inactive -> active -> muted -> cleared -> reactivated` 测试，且复发的 `instance_id` 不同。
- stale 或 inactive 消音请求不影响新实例；一个实例消音不移除或停止任何其它活动告警。
- CDC、LAN、devd、CLI 和 Web 对同一设备返回一致的告警状态；旧固件、offline 和 transport error 有显式结果。
- 前面板按本 spec 的矩阵导出真实 framebuffer/PNG，获批准后才连接运行时输入、触摸与按键路由。
