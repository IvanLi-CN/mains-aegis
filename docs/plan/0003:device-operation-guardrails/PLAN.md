# 设备操作纪律防护（Agent 设备闸门）（#0003）

## 状态

- Status: 已完成
- Created: 2026-01-22
- Last: 2026-01-22

## 背景 / 问题陈述

- 仓库包含固件 bring-up 工作流，且常见场景为“多设备/多端口候选并存”。
- Agent 在排错过程中若具备自动尝试、自动切换端口或自动写入能力，将高概率误操作其他设备并违反操作纪律。
- 本计划将“设备操作闸门”制度化：在未被显式授权且未唯一确认目标端口前，Agent 仅允许只读动作；任何写入/擦除一律禁止；原则上不得使用 `espflash`（含 `cargo espflash` / `cargo-espflash` 以及其封装/后端）。

## 目标 / 非目标

### Goals

- 禁止写入：Agent 永不执行任何会写入/擦除/改写设备 Flash/分区的操作（含通过其他工具间接触发写入/擦除）。
- 禁止使用 `espflash`：Agent 原则上不得直接或间接使用 `espflash`（包括但不限于 `espflash` CLI、`cargo espflash`、`cargo-espflash`，以及任何底层调用 espflash 的封装工具）。
- 唯一目标：未唯一确认目标设备（端口白名单）前，Agent 只能执行只读动作。
- 不枚举端口：Agent 不得执行任何“列出候选端口/枚举串口设备”的动作；目标端口必须由用户先在 `mcu-agentd` 中手工配置好，Agent 仅允许只读校验。
- 显式授权：任何可能改变设备状态的动作（即便非写入，例如 reset）必须获得显式授权。
- 可追溯：设备相关动作必须输出“允许/拒绝 + 原因 + 下一步”的最小说明（不要求任何会话级汇总或落盘产出）。

### Non-goals

- 不对所有端口进行“轮询尝试”。
- 不因为“端口已设置好”就推断“允许切换端口/允许写入/允许操作其他设备”。
- 不通过封装工具绕过禁令（例如通过 `mcu-agentd` / 其他工具间接触发 `espflash` 或写入/擦除）。
- 不限制开发者手工使用 `espflash`/`mcu-agentd` 的工作流口径（本计划仅约束 Agent 的行为）。

## 范围（Scope）

### In scope

- 将设备操作纪律固化为仓库可复用的规则（AGENTS 指南/bring-up 文档/计划契约）。
- 明确闸门规则（G0–G4）。
- 定义一套可复制的“最小说明格式”，用于每次设备相关动作的允许/拒绝决策输出。

### Out of scope

- 新增任何“自动修复/自动切换端口/自动尝试”的排错策略。
- 为写入/擦除行为设计任何自动化路径（写入始终由用户自行执行）。

## 需求（Requirements）

### MUST

- **espflash 禁用（G0）**：拒绝执行任何 `espflash` 相关命令与任何最终触发 `espflash` 的间接路径（包括但不限于 `cargo espflash` / `cargo-espflash` / `mcu-agentd` 若其后端为 `espflash`）。
- **写入硬禁令（G1）**：拒绝执行任何会写入/擦除 Flash/分区的命令（无论工具为何）。
- **端口唯一性（G2）**：当 `mcu-agentd selector get <MCU_ID>` 无法得到唯一目标端口时，拒绝一切设备操作（仅允许提问）；最小提问为“请先在你本机用 `mcu-agentd selector set <MCU_ID> <PORT>` 选择唯一目标端口，然后再继续”。
- **端口不可枚举（G2a）**：Agent 不得通过任何方式枚举/列出候选端口（包括 `mcu-agentd selector list`、列目录等）；必须要求用户先手工在 `mcu-agentd` 中完成唯一选择（`mcu-agentd selector set <MCU_ID> <PORT>`）。
- **端口来源固定（G2b）**：目标端口只允许来自 `mcu-agentd` 的 selector 状态：Agent 仅允许读取 `mcu-agentd selector get <MCU_ID>` 的结果；若无唯一结果则拒绝设备动作。
- **禁止自动换端口（G3）**：Agent 不得自行把端口从 A 换到 B“试试”；只能在用户更新白名单后才可切换。
- **状态改变二次确认（G4）**：执行任何 reset / monitor-with-reset / 进入下载模式等状态改变操作前，必须复述“端口 X + 操作 Y（不写入）”，并等待明确 yes/no。
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

- Agent 不执行任何 `espflash`（含封装/间接调用）。
- Agent 永不执行任何写入/擦除命令（Flash/分区级别）。
- 多端口场景下，Agent 不会尝试其他端口，也不会枚举候选端口；只会要求用户先在 `mcu-agentd` 中完成唯一选择（`mcu-agentd selector set <MCU_ID> <PORT>`），并只读验证（`mcu-agentd selector get <MCU_ID>`）。
- 任何状态改变操作均在二次确认后执行；未确认则拒绝。
- 每次设备相关动作均有最小说明输出（允许/拒绝 + 原因 + 下一步）。

## 实现前置条件（Definition of Ready / Preconditions）

- “espflash 禁用”口径已冻结（含 `cargo-espflash`、`mcu-agentd` 等间接路径）。
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

- `AGENTS.md`: 增补“设备操作纪律”章节（禁用 `espflash`、写入硬禁令、端口白名单、最小说明输出要求）。
 
## 实现里程碑（Milestones）

- [x] 在 `AGENTS.md` 固化 G0–G4 与 Decision summary 输出要求（可复制粘贴）
- [x] 在 `firmware/README.md` 标注“Human-only / Agent-allowed（read-only）”并避免 Agent 端口枚举与写入


## 方案概述（Approach, high-level）

- 以“默认拒绝”为准：先要求用户手工在 `mcu-agentd` 中完成唯一选择（`mcu-agentd selector set <MCU_ID> <PORT>`），Agent 仅允许只读校验（`mcu-agentd selector get <MCU_ID>`），再判定命令类别（read-only / state-changing / WRITE-BLOCKED）。
- 对任何 `espflash`（含间接路径）统一拒绝并给出替代路径（仅提示原则与需要用户执行的步骤，不提供 Agent 代执行）。
- 对任何写入/擦除统一拒绝并给出“用户自行执行”的下一步。
- 对任何状态改变动作统一走二次确认。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：工具对串口 DTR/RTS 的默认行为差异导致“某些监视/连接动作是否触发 reset”不易静态判定；因此本计划统一将任何状态改变操作置于二次确认闸门下。
- 开放问题：见本计划在对话/评审中的冻结结论；未冻结前保持 `待设计`。

## 参考（References）

（如需对齐其他计划的实现口径，应由对应计划的 owner 在其实现流程中自行合入本计划的纪律要求。）

## Change log

- 2026-01-22: 落地 Agent 设备闸门的文档约束与示例标注（AGENTS + firmware bring-up README）。
