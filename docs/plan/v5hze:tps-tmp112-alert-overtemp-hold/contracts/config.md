# Config contracts：TMP112A 过温告警（#v5hze）

## Summary

本契约冻结两颗 `TMP112A`（`0x48/0x49`）的过温告警配置，使 `ALERT` 输出符合“过温时保持输出（电平型）”的项目设计语义，并规定配置失败时的处置策略。

## Devices

| Logical name | I2C bus | Address (7-bit) | Notes |
| --- | --- | --- | --- |
| `TMP112A(TPS-A hot spot)` | `I2C1` | `0x48` | `ADD0=GND` |
| `TMP112A(TPS-B hot spot)` | `I2C1` | `0x49` | `ADD0=V+` |

## Register pointers (I2C)

（用于实现落地；寄存器值均为 16-bit，大端传输。）

| Register | Pointer |
| --- | --- |
| Temperature | `0x00` |
| Configuration | `0x01` |
| TLOW | `0x02` |
| THIGH | `0x03` |

## Alert semantics (MUST)

- Mode: Comparator（`TM=0`）
- Polarity: active-low（`POL=0`；`ALERT=0` 表示告警有效）
- Latch behavior:
  - Assert: `TEMP >= T(HIGH)` → `ALERT` 拉低
  - Release: `TEMP < T(LOW)` → `ALERT` 释放（高阻，由外部上拉保持为高）
- Debounce: 使用 Fault Queue（具体值见下）

## Thresholds (MUST)

阈值以 `°C * 16` 的整数形式冻结（与固件内部 `temp_c_x16` 一致），实现时按 `TMP112A` 阈值寄存器格式写入。

- `T(HIGH)_c_x16`: `800`（`50°C * 16`）
- `T(LOW)_c_x16`: `640`（`40°C * 16`）

约束：

- 必须满足 `T(LOW) < T(HIGH)`（形成滞回窗口）。
- 两颗 `TMP112A` 默认使用同一组阈值；若确有差异需求，必须在此处显式拆分并给出原因。

## Fault queue / conversion rate (MUST)

为避免瞬态噪声/抖动触发硬停机，需要冻结去抖与采样速率（若不需要差异化，可保持两颗一致）：

- Fault queue: `4`
- Conversion rate: `1 Hz`

Notes:

- 以 `1 Hz` 且 fault queue=`4` 估算：触发时间为“秒级”（约 `~4s` 量级，取决于采样相位与实现细节）。若后续发现响应过慢，可在不改硬件的前提下通过提高 conversion rate 或降低 fault queue 调整。

## Boot-time programming (MUST)

固件必须在启动阶段完成以下步骤（对 `0x48/0x49` 两颗都执行）：

1. 写入 Configuration（使能上述语义：Comparator + active-low + 去抖/速率等）。
2. 写入 `TLOW` 与 `THIGH`。
3. （推荐）回读 `Configuration/TLOW/THIGH` 并在日志中输出摘要（便于 bring-up 对齐）。

## Power-up defaults (FYI)

实现不得依赖上电默认值满足本计划阈值要求；这里仅记录默认值用于 bring-up 对齐与故障定位：

- Default conversion rate: `4 Hz`
- Reset thresholds: `THIGH=+80°C`, `TLOW=+75°C`

## Failure policy (MUST)

当发生以下任一情况时，视为“配置失败”：

- 任一步 I2C 写入/回读失败（NACK/timeout/bus error）。
- 回读值与期望不一致（若启用回读校验）。

失败策略（已冻结为 fail-safe）：

- fail-safe: 当任一 `TMP112A` 配置写入失败时，固件必须**不允许使能 TPS 输出**（保持输出禁用/不上电），并打印错误信息（包含地址与错误分类）。

## Observability (MUST)

固件必须提供最低限度的可观测性以支撑验收与回归：

- 在启动日志中输出每颗 `TMP112A` 的配置结果（成功/失败 + 地址 + 回读摘要或错误分类）。
- 在运行日志中能观察到 `therm_kill_n` 电平变化（已有字段：`therm_kill_n=<0|1>`；见 `#0006`）。

## Overtemp source hinting (MUST, logs only)

由于 `THERM_KILL_N` 为线与汇总，无法从该信号本身区分“哪一路 `ALERT` 拉低”。若 `THERM_KILL_N=0`，实现阶段应通过读取两颗 `TMP112A` 当前温度并与 `T(LOW)/T(HIGH)` 比较，在日志中给出“可能来源”的提示（例如 `out_a/out_b/both/unknown`）。
