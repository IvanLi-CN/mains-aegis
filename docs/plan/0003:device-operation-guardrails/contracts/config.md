# 设备操作授权输入（Config）

本契约定义 Agent 在涉及“设备相关动作”时所需的最小输入。**未提供这些输入时，Agent 必须拒绝设备相关动作（仅允许提问）**。

## Inputs

### `device_id`（required）

- Type: string
- Meaning: released `mains-aegis-devd` 中已绑定的 Mains Aegis 设备标识。
- Example: `fixture-ups-device`

### 设备绑定（owner-visible）

- Meaning: 设备发现与绑定必须通过 released devd 的 owner-visible scan/list/bind 流程；Agent 不直接枚举或切换端口。
- Rules:
  - released devd scan 只发现候选设备，不自动连接或切换
  - Agent 禁止执行任何直接“枚举候选端口”的动作
  - Agent 禁止执行任何“切换端口”的动作
  - Agent 禁止使用 `mcu-agentd` 作为 Mains Aegis 设备操作路径

## Validation rules

- 禁止 `mcu-agentd` 设备路径：拒绝 Agent 发起的 Mains Aegis `mcu-agentd` 设备操作。
- 禁止端口枚举：拒绝任何直接端口枚举行为。
- 禁止端口切换：拒绝任何“换端口试试”的行为。
- 禁止直接使用 `espflash`（含 `espflash` / `cargo espflash` / `cargo-espflash`）。
- 允许 released `mains-aegis` / `mains-aegis-devd` 的 owner-visible scan/list/bind/connect 和只读状态查询；真实 flash/reset/monitor 需要明确已绑定设备与 owner authorization。
