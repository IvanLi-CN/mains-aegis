# 设备操作纪律防护（Agent 设备闸门）（#0003）

## 状态

- Status: 已完成
- Created: 2026-01-22
- Last: 2026-01-24

## 背景 / 问题陈述

- 仓库包含固件 bring-up 工作流，且常见场景为“多设备/多端口候选并存”。
- Agent 在排错过程中若具备自动尝试、自动切换端口或自动写入能力，将高概率误操作其他设备并违反操作纪律。
- 本计划将“设备操作闸门”制度化：在未唯一确认目标端口前，Agent 仅允许只读动作；当你明确授权（仅写入/flash）时，允许通过 `mcu-agentd` 执行受控烧录（写入）；状态改变仅允许走明确 allowlist（目前仅 `mcu-agentd monitor <MCU_ID> --reset`）。同时仍禁止端口枚举与自动换端口，并禁止直接调用 `espflash`（含 `cargo espflash` / `cargo-espflash`）。

## 目标 / 非目标

### Goals

- 受控烧录：允许 Agent 通过 `mcu-agentd flash <MCU_ID>` 执行烧录（写入），但必须满足“唯一目标端口校验 + 禁止端口枚举/自动换端口”。
- 禁止直接使用 `espflash`：Agent 不得直接调用 `espflash` CLI / `cargo espflash` / `cargo-espflash`（但允许 `mcu-agentd` 使用 `espflash` 作为内部后端）。
- 唯一目标：未唯一确认目标设备（端口白名单）前，Agent 只能执行只读动作。
- 不枚举端口：Agent 不得执行任何“列出候选端口/枚举串口设备”的动作；目标端口必须由用户先在 `mcu-agentd` 中手工配置好，Agent 仅允许只读校验。
- 状态改变 allowlist：仅允许执行 `mcu-agentd monitor <MCU_ID> --reset`；除此之外的状态改变类命令一律拒绝。
- 写入 allowlist：仅允许执行 `mcu-agentd flash <MCU_ID>`，且执行前必须校验唯一目标端口；不进行确认询问。
- 可追溯：设备相关动作必须输出“允许/拒绝 + 原因 + 下一步”的最小说明（不要求任何会话级汇总或落盘产出）。

### Non-goals

- 不对所有端口进行“轮询尝试”。
- 不因为“端口已设置好”就推断“允许切换端口/允许写入/允许操作其他设备”。
- 不绕过闸门：不允许跳过“唯一端口校验 / 禁止端口枚举与自动换端口”的约束。
- 不限制开发者手工使用 `espflash`/`mcu-agentd` 的工作流口径（本计划仅约束 Agent 的行为）。

## 范围（Scope）

### In scope

- 将设备操作纪律固化为仓库可复用的规则（AGENTS 指南/bring-up 文档/计划契约）。
- 明确闸门规则（G0–G4）。
- 定义一套可复制的“最小说明格式”，用于每次设备相关动作的允许/拒绝决策输出。

### Out of scope

- 新增任何“自动修复/自动切换端口/自动尝试”的排错策略。
- 将“允许端口枚举/自动换端口”的路径制度化。

## 需求（Requirements）

### MUST

- **直接 espflash 禁用（G0）**：拒绝执行任何直接调用 `espflash` 的命令（包括但不限于 `espflash ...` / `cargo espflash ...` / `cargo-espflash ...`）。
- **受控写入（G1）**：允许执行 `mcu-agentd flash <MCU_ID>`（写入），但必须满足 G2/G3/G4；除 `mcu-agentd flash` 外，其他写入/擦除/分区改写类命令一律拒绝。
- **端口唯一性（G2）**：当 `mcu-agentd selector get <MCU_ID>` 无法得到唯一目标端口时，拒绝一切设备操作（仅允许提问）；最小提问为“请先在你本机用 `mcu-agentd selector set <MCU_ID> <PORT>` 选择唯一目标端口，然后再继续”。
- **端口不可枚举（G2a）**：Agent 不得通过任何方式枚举/列出候选端口（包括 `mcu-agentd selector list`、列目录等）；必须要求用户先手工在 `mcu-agentd` 中完成唯一选择（`mcu-agentd selector set <MCU_ID> <PORT>`）。
- **端口来源固定（G2b）**：目标端口只允许来自 `mcu-agentd` 的 selector 状态：Agent 仅允许读取 `mcu-agentd selector get <MCU_ID>` 的结果；若无唯一结果则拒绝设备动作。
- **禁止自动换端口（G3）**：Agent 不得自行把端口从 A 换到 B“试试”；只能在用户更新白名单后才可切换。
- **状态改变 allowlist + 写入 allowlist（G4）**：
  - 状态改变仅允许 `mcu-agentd monitor <MCU_ID> --reset`，且执行前必须先用 `mcu-agentd selector get <MCU_ID>` 校验唯一目标端口；除此之外的状态改变类命令一律拒绝。
  - 写入仅允许 `mcu-agentd flash <MCU_ID>`，且执行前必须先用 `mcu-agentd selector get <MCU_ID>` 校验唯一目标端口；除此之外的写入/擦除/分区改写类命令一律拒绝。
- **输出规范**：每次设备相关动作必须输出“Decision summary”（类型/端口/命令/为何允许或拒绝/下一步）；不要求会话汇总，也不要求任何落盘日志产出。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes） |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 设备操作授权输入（mcu-agentd 中的唯一目标端口） | Config | internal | New | ./contracts/config.md | tooling | agent, developers | 端口是设备唯一身份；由用户在 mcu-agentd 配置/状态中设定 |
| 设备相关动作的最小说明输出（Decision summary） | CLI | internal | New | ./contracts/audit-output.md | tooling | agent, developers | 仅聊天内说明；不要求会话汇总/落盘 |

### 契约文档（按 Kind 拆分）

- [contracts/config.md](./contracts/config.md)
- [contracts/audit-output.md](./contracts/audit-output.md)

## 验收标准（Acceptance Criteria）

- Agent 不直接调用 `espflash` / `cargo espflash` / `cargo-espflash`。
- Agent 允许通过 `mcu-agentd flash <MCU_ID>` 执行烧录（写入），但每次均在“唯一目标端口校验”后执行；其他写入/擦除命令一律拒绝。
- 多端口场景下，Agent 不会尝试其他端口，也不会枚举候选端口；只会要求用户先在 `mcu-agentd` 中完成唯一选择（`mcu-agentd selector set <MCU_ID> <PORT>`），并只读验证（`mcu-agentd selector get <MCU_ID>`）。
- 状态改变仅允许 `mcu-agentd monitor <MCU_ID> --reset`（allowlist），且必须在“唯一目标端口校验”后执行；其他状态改变一律拒绝。
- 每次设备相关动作均有最小说明输出（允许/拒绝 + 原因 + 下一步）。

## 实现前置条件（Definition of Ready / Preconditions）

- “直接 espflash 禁用”口径已冻结（不限制 `mcu-agentd` 的内部后端实现）。
- 文档落点已确定（AGENTS 指南与 bring-up 文档）；本计划不修改其他计划文档。
- `MCU_ID` 的取值需要在会话中明确给出；且 Agent 仅允许读取 `mcu-agentd selector get <MCU_ID>`（只读）来校验唯一目标端口。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- 本仓库无自动化测试：以文档约束 + 手工验收为主。

### Quality checks

- 文档一致性：关键术语（write / erase / state-changing / allowlist）在 AGENTS 与相关文档中用词一致。
- 可操作性：审计输出模板可复制粘贴；示例命令必须标注“Agent 可做/不可做/需用户执行”。

## 文档更新（Docs to Update）

（是否在 plan 阶段直接修改这些文档，取决于主人决策；见本计划实现阶段。）

- `AGENTS.md`: 增补“设备操作纪律”章节（禁用直接 `espflash`、受控写入（仅 `mcu-agentd flash`）、端口白名单、最小说明输出要求）。
## 实现里程碑（Milestones）

- [x] 在 `AGENTS.md` 固化 G0–G4 与 Decision summary 输出要求（可复制粘贴）
- [x] 在 `firmware/README.md` 标注“Human-only / Agent-allowed（read-only / state-changing / write）”，并明确：禁止端口枚举与自动换端口；写入仅允许 `mcu-agentd flash`（需唯一端口校验）


## 方案概述（Approach, high-level）

- 以“默认拒绝”为准：先要求用户手工在 `mcu-agentd` 中完成唯一选择（`mcu-agentd selector set <MCU_ID> <PORT>`），Agent 仅允许只读校验（`mcu-agentd selector get <MCU_ID>`），再判定命令类别（read-only / state-changing / write）。
- 对任何直接 `espflash` 调用统一拒绝并给出替代路径（优先建议使用 `mcu-agentd`）。
- 对写入/擦除：仅允许 `mcu-agentd flash <MCU_ID>`，其余一律拒绝并给出“用户自行执行/改用 mcu-agentd”的下一步。
- 对状态改变：仅允许 allowlist（目前仅 `mcu-agentd monitor <MCU_ID> --reset`），其余一律拒绝。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：不同工具/平台对串口 DTR/RTS 的默认行为差异可能导致“监视/连接动作触发 reset”的副作用；本计划将其视为可接受（仅允许 `mcu-agentd monitor <MCU_ID> --reset`），并将风险控制重点放在“端口唯一性 + 禁止枚举/换端口”上。
- 开放问题：见本计划在对话/评审中的冻结结论；未冻结前保持 `待设计`。

## 参考（References）

（如需对齐其他计划的实现口径，应由对应计划的 owner 在其实现流程中自行合入本计划的纪律要求。）

## Change log

- 2026-01-22: 落地 Agent 设备闸门的文档约束与示例标注（AGENTS + firmware bring-up README）。
- 2026-01-22: 更新闸门策略：允许 Agent 在“唯一目标端口校验”后通过 `mcu-agentd flash` 执行受控烧录（写入）。
- 2026-01-24: 调整闸门策略：状态改变与写入均仅允许 allowlist（`mcu-agentd monitor <MCU_ID> --reset` / `mcu-agentd flash <MCU_ID>`），且仅要求“唯一目标端口校验”（禁止枚举/换端口），不进行确认询问。
