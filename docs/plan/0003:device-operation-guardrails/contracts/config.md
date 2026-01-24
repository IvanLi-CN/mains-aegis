# 设备操作授权输入（Config）

本契约定义 Agent 在涉及“设备相关动作”时所需的最小输入。**未提供这些输入时，Agent 必须拒绝设备相关动作（仅允许提问）**。

## Inputs

### `MCU_ID`（required）

- Type: string
- Meaning: `mcu-agentd` 的目标 MCU 标识（用于执行 device ops）。
- Example: `esp`

### 端口选择（human-only）

- Meaning: 端口选择是人类责任；Agent 不枚举、不切换。
- Rules:
  - 用户负责手工完成端口选择（例如 `mcu-agentd selector set <MCU_ID> <PORT>`）
  - Agent 禁止执行任何“枚举候选端口”的动作（例如 `mcu-agentd selector list`、列目录等）
  - Agent 禁止执行任何“切换端口”的动作（例如 `mcu-agentd selector set`）
  - Agent 不需要频繁读取当前端口（例如不需要在每次动作前跑 `mcu-agentd selector get`）

## Validation rules

- 禁止端口枚举：拒绝执行 `mcu-agentd selector list <MCU_ID>` 以及任何端口枚举行为。
- 禁止端口切换：拒绝执行 `mcu-agentd selector set <MCU_ID> <PORT>` 以及任何“换端口试试”的行为。
- `mcu-agentd` 设备操作：除端口枚举/切换外，允许执行其他 `mcu-agentd` 命令（含 `flash` / `monitor` / `erase` / `reset` 等）。
- 禁止直接使用 `espflash`（含 `espflash` / `cargo espflash` / `cargo-espflash`）；但不限制 `mcu-agentd` 的内部后端实现。
