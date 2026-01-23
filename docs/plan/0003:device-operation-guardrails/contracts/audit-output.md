# 设备相关动作最小说明（Decision summary）

本契约定义 Agent 在执行（或拒绝执行）任何“设备相关动作”时的最小输出格式：只需说明“允许/拒绝 + 原因 + 下一步”。不要求会话级汇总，也不要求任何落盘产出。

## Decision summary（一次动作）

必须输出以下字段（顺序可调整，但语义必须齐全）：

- Operation type: `read-only` / `state-changing` / `write`
- Target port: `<port>`（必须等于 `mcu-agentd selector get <MCU_ID>` 读取到的唯一目标端口；否则为拒绝）
- Command: `<full command>`（完整可复制）
- Decision: `allow|deny`
- Rationale: 为什么允许/拒绝（包含命中哪条闸门：G0–G4）
- Next step: 若为 deny，用户下一步需要做什么（例如先在 `mcu-agentd` 中配置唯一目标端口，或“确认是否允许执行写入/状态改变命令”）
