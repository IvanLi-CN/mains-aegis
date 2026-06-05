# 设备操作纪律防护（Agent 设备闸门）（#0003）

## 状态

- Status: 已完成
- Created: 2026-01-22
- Last: 2026-01-24

## 背景 / 问题陈述

- 仓库包含固件 bring-up 工作流，且常见场景为“多设备/多端口候选并存”。
- Agent 在排错过程中若具备自动尝试、自动切换端口或自动写入能力，将高概率误操作其他设备并违反操作纪律。
- 本计划将“设备操作闸门”制度化：Agent 禁止端口枚举与切换端口，禁止使用 `mcu-agentd` 作为 Mains Aegis 设备操作路径，并禁止直接调用 `espflash`（含 `cargo espflash` / `cargo-espflash`）。Agent 设备操作必须通过 released `mains-aegis` / `mains-aegis-devd` 的 owner-visible 绑定设备路径执行。

## 目标 / 非目标

### Goals

- 受控设备操作：Agent 只通过 released `mains-aegis` / `mains-aegis-devd` 的 owner-visible 绑定设备路径执行。
- 禁止直接使用 `espflash`：Agent 不得直接调用 `espflash` CLI / `cargo espflash` / `cargo-espflash`。
- 禁止 `mcu-agentd` 设备路径：Agent 不得调用 `mcu-agentd` 执行 Mains Aegis 设备操作。
- 不枚举端口：Agent 不得执行任何“列出候选端口/枚举串口设备”的动作。
- 不切换端口：Agent 不得执行任何“切换端口”的动作，也不得自行“换一个端口试试”。
- 可追溯：设备相关动作必须输出“允许/拒绝 + 原因 + 下一步”的最小说明（不要求任何会话级汇总或落盘产出）。

### Non-goals

- 不对所有端口进行“轮询尝试”。
- 不因为“端口已设置好”就推断“允许切换端口/允许写入/允许操作其他设备”。
- 不绕过闸门：不允许跳过“禁止端口枚举与切换端口 / 禁止自动换端口”的约束。
- 不限制开发者手工维护历史工具链文件（本计划仅约束 Agent 的行为）。

## 范围（Scope）

### In scope

- 将设备操作纪律固化为仓库可复用的规则（AGENTS 指南/bring-up 文档/计划契约）。
- 明确闸门规则（G0–G5）。
- 定义一套可复制的“最小说明格式”，用于每次设备相关动作的允许/拒绝决策输出。

### Out of scope

- 新增任何“自动修复/自动切换端口/自动尝试”的排错策略。
- 将“允许端口枚举/自动换端口”的路径制度化。

## 需求（Requirements）

### MUST

- **直接 espflash 禁用（G0）**：拒绝执行任何直接调用 `espflash` 的命令（包括但不限于 `espflash ...` / `cargo espflash ...` / `cargo-espflash ...`）。
- **禁止 mcu-agentd 设备路径（G1）**：Agent 不得调用 `mcu-agentd` 执行 Mains Aegis 设备操作。
- **端口不可枚举（G2）**：Agent 不得通过任何方式枚举/列出候选端口。
- **端口不可切换（G3）**：Agent 不得执行切换端口动作。
- **禁止自动换端口（G4）**：Agent 不得自行把端口从 A 换到 B“试试”；只能由用户在本机手工切换。
- **devd 绑定设备路径（G5）**：released `mains-aegis` / `mains-aegis-devd` 是 Agent 的 Mains Aegis 设备操作入口；真实 flash/reset/monitor 仍需要明确已绑定设备与 owner authorization。
- **输出规范**：每次设备相关动作必须输出“Decision summary”（类型/命令/为何允许或拒绝/下一步）；不要求会话汇总，也不要求任何落盘日志产出。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes） |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 设备操作约束输入（devd 绑定设备） | Config | internal | New | ./contracts/config.md | tooling | agent, developers | Agent 禁止 mcu-agentd 设备路径、端口枚举与切换；released devd 是设备入口 |
| 设备相关动作的最小说明输出（Decision summary） | CLI | internal | New | ./contracts/audit-output.md | tooling | agent, developers | 仅聊天内说明；不要求会话汇总/落盘 |

### 契约文档（按 Kind 拆分）

- [contracts/config.md](./contracts/config.md)
- [contracts/audit-output.md](./contracts/audit-output.md)

## 验收标准（Acceptance Criteria）

- Agent 不直接调用 `espflash` / `cargo espflash` / `cargo-espflash`。
- Agent 不使用 `mcu-agentd` 作为 Mains Aegis 设备操作路径。
- Agent 不枚举候选端口。
- Agent 不切换端口，也不会“换一个端口试试”。
- Agent 设备操作通过 released devd 的已绑定设备路径执行。
- 每次设备相关动作均有最小说明输出（允许/拒绝 + 原因 + 下一步）。

## 实现前置条件（Definition of Ready / Preconditions）

- “直接 espflash 禁用”口径已冻结。
- 文档落点已确定（AGENTS 指南与 bring-up 文档）；本计划不修改其他计划文档。
- 目标 `device_id` 必须来自 released devd 的 owner-visible 绑定设备状态。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- 本仓库无自动化测试：以文档约束 + 手工验收为主。

### Quality checks

- 文档一致性：关键术语（write / erase / state-changing）在 AGENTS 与相关文档中用词一致。
- 可操作性：审计输出模板可复制粘贴；示例命令必须标注“Agent 可做/不可做/需用户执行”。

## 文档更新（Docs to Update）

（是否在 plan 阶段直接修改这些文档，取决于主人决策；见本计划实现阶段。）

- `AGENTS.md`: 增补“设备操作纪律”章节（禁用直接 `espflash`、禁用 `mcu-agentd` 设备路径、禁止端口枚举/切换端口、最小说明输出要求）。
## 实现里程碑（Milestones）

- [x] 在 `AGENTS.md` 固化 G0–G5 与 Decision summary 输出要求（可复制粘贴）
- [x] 在项目设备文档标注 Agent 操作边界，并明确：禁用 `mcu-agentd` 设备路径、禁止端口枚举与切换端口


## 方案概述（Approach, high-level）

- 以“devd 绑定设备路径”为准：拒绝任何端口枚举与端口切换，同时禁止自动换端口。
- 对任何直接 `espflash` 调用统一拒绝。
- 对任何 Agent 发起的 `mcu-agentd` Mains Aegis 设备操作统一拒绝。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：不同工具/平台对串口 DTR/RTS 的默认行为差异可能导致“监视/连接动作触发 reset”的副作用；本计划将其视为可接受，并将风险控制重点放在“禁止端口枚举/切换端口”上。
- 开放问题：见本计划在对话/评审中的冻结结论；未冻结前保持 `待设计`。

## 参考（References）

（如需对齐其他计划的实现口径，应由对应计划的 owner 在其实现流程中自行合入本计划的纪律要求。）

## Change log

- 2026-01-22: 落地 Agent 设备闸门的文档约束与示例标注（AGENTS + firmware bring-up README）。
- 2026-01-22: 更新闸门策略：禁用 Agent 的 `mcu-agentd` 设备路径，并禁止端口枚举/切换端口。
- 2026-01-24: 调整闸门策略：不再要求“每次动作前读取/校验当前端口”；约束聚焦为“禁止端口枚举/切换端口（不去改就不会错）”。
