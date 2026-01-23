# 设备操作授权输入（Config）

本契约定义 Agent 在涉及“设备相关动作”时所需的最小输入。**未提供这些输入时，Agent 必须拒绝设备相关动作（仅允许提问）**。

## Inputs

### `MCU_ID`（required）

- Type: string
- Meaning: `mcu-agentd` 的目标 MCU 标识（用于读取 selector 状态）。
- Example: `esp`

### 目标端口来源（required）

- Meaning: 目标端口只允许来自用户本机 `mcu-agentd` 的 selector 状态（即用户先手工完成选择，Agent 只读校验）。
- Rules:
  - 用户负责手工完成“选择唯一端口”（例如 `mcu-agentd selector set <MCU_ID> <PORT>`）
  - Agent 只允许通过 `mcu-agentd selector get <MCU_ID>` 读取已选择的目标端口（只读）
  - Agent 禁止执行任何“枚举候选端口”的动作（例如 `mcu-agentd selector list`、列目录等）
  - 若 `selector get` 结果为空/不存在：Agent 必须拒绝设备动作，并要求用户先完成选择

## Validation rules

- 当 `mcu-agentd selector get <MCU_ID>` 无法得到唯一端口：拒绝一切设备操作（仅允许提问）。
- 写入/擦除：仅允许 `mcu-agentd flash <MCU_ID>`（写入），且每次执行前必须先校验唯一目标端口并获得明确 yes/no；其他写入/擦除/分区改写类命令一律拒绝。
- 禁止直接使用 `espflash`（含 `espflash` / `cargo espflash` / `cargo-espflash`）；但不限制 `mcu-agentd` 的内部后端实现。
