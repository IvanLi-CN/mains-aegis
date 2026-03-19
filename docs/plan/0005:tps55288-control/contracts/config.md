# TPS55288 默认 profile 与通道/地址映射（Config）

本契约定义固件在控制 `TPS55288`（双路）时需要冻结的最小配置形状：**通道命名、I2C 地址、默认启用通道、默认输出电压与电流限制**。

## Inputs

### Driver crate（fixed）

- Crate: `tps55288`
- Source: crates.io
- Version: `0.2.0`
- Features:
  - `default`（无默认 features）
  - 可选：`defmt`（如需更友好的日志输出）
  - 可选：`async`（如后续选择 async I2C；本计划默认走 sync）

### I2C 总线（fixed）

- Bus: `I2C1`
- Pins: `GPIO48=I2C1_SDA`，`GPIO47=I2C1_SCL`
- Frequency: `400kHz`
- Source of truth: `docs/i2c-address-map.md`

### INA3221 采样映射（fixed）

- Device: `INA3221` (I2C address `0x40`, see `docs/i2c-address-map.md`)
- Purpose in this plan: 采样 `TPS55288 OUT-A/OUT-B` 的实际电压与电流，并每 `500ms` 输出日志。
- Channel mapping (source of truth: `docs/power-monitoring-design.md`):
  - INA3221 `CH2` → `TPS55288 OUT-A`（`RSHUNT = 10 mΩ`）
  - INA3221 `CH1` → `TPS55288 OUT-B`（`RSHUNT = 10 mΩ`）
  - INA3221 `CH3` → `UPS VIN`（`RSHUNT = 7 mΩ`，本计划默认不启用）

### INA3221 初始化配置（fixed）

目标：连续转换（shunt+bus），仅启用 `CH1/CH2`，并使用适度的平均以降低噪声。

- Configuration register (pointer `0x00`), write value: `0x6527`
  - `CH1en=1`, `CH2en=1`, `CH3en=0`
  - `AVG2-0=010` → `16` averages
  - `VBUSCT2-0=100` → `1.1ms` bus conversion time
  - `VSHCT2-0=100` → `1.1ms` shunt conversion time
  - `MODE3-1=111` → shunt and bus, continuous

### INA3221 换算规则（fixed）

- `VSHUNT_LSB = 40 µV`
- `VBUS_LSB = 8 mV`

对每个输出通道（`RSHUNT = 10 mΩ`）：

- `vbus_mv = raw_bus * 8`
- `vshunt_uv = raw_shunt * 40`
- `current_ma = vshunt_uv / 10`（因为 `I(mA) = Vshunt(µV) / Rshunt(mΩ)`，`Rshunt=10mΩ`）

### 设备与通道映射（fixed）

| Logical channel | Board naming | I2C address | Output-side net | Shared output node |
| --- | --- | ---:| --- | --- |
| `out_a` | `TPS55288 OUT-A` / `TPS-A` | `0x74` | `ISP_TPSA` | `VOUT_TPS` |
| `out_b` | `TPS55288 OUT-B` / `TPS-B` | `0x75` | `ISP_TPSB` | `VOUT_TPS` |

> 备注：当前主板网表中两颗 `TPS55288` 的 `VOUT/ISP` 分别落在 `ISP_TPSA` / `ISP_TPSB`，再经 `R68` / `R83` 汇入共享节点 `VOUT_TPS`，后级再由 `U21/Q28` 接入 `VOUT`（见 `docs/pcbs/mainboard/README.md`）；本契约冻结“器件实例、I2C 地址与通道侧输出网络”的逻辑映射。

### INA3221 采样映射（fixed）

- Device: `INA3221`
- I2C address: `0x40`（`I2C1`，见 `docs/i2c-address-map.md`）
- Sampling channels (source of truth: `docs/power-monitoring-design.md`):
  - INA3221 CH2: `TPS55288 OUT-A`（`IN+2=ISP_TPSA`，`IN-2=VOUT_TPS`，`Rshunt=10mΩ`）
  - INA3221 CH1: `TPS55288 OUT-B`（`IN+1=ISP_TPSB`，`IN-1=VOUT_TPS`，`Rshunt=10mΩ`）

### INA3221 初始化配置（fixed）

目标：仅启用 CH1/CH2，连续采样 shunt+bus，并提供稳定读数用于 `500ms` 周期遥测输出。

- Register: Configuration (pointer `0x00`)
- Value: `0x6527`
  - CH1 enabled, CH2 enabled, CH3 disabled
  - AVG = `16` samples（`AVG2-0=010`）
  - VBUSCT = `1.1ms`（`VBUSCT2-0=100`）
  - VSHCT = `1.1ms`（`VSHCT2-0=100`）
  - MODE = shunt+bus continuous（`MODE=111`）

量化口径（用于固件换算与日志输出）：

- `VSHUNT_LSB = 40µV`
- `VBUS_LSB = 8mV`
- 电流换算：`I(mA) = VSHUNT(µV) / RSHUNT(mΩ)`
  - 对 `RSHUNT=10mΩ`：`I_LSB = 40µV / 10mΩ = 4mA`

### 默认 profile（required）

- `default_enabled_channel`: `out_a | out_b | out_a+out_b`
  - Meaning: 上电后默认“主动稳压输出”的通道
  - Default: `out_a+out_b`

- `default_vout_mv`: integer
  - Meaning: 目标输出电压（mV）
  - Default: `19000`

- `default_ilimit_ma`: integer
  - Meaning: 目标电流限制（mA）；实现以 `TPS55288` 的寄存器能力为准（可能是近似值/档位值）
  - Default: `3500`

### 非默认通道关闭策略（required）

必须在实现前明确以下策略之一（用于满足“默认仅启用一路输出”的 MUST）：

- Strategy A（preferred, if supported by IC）: 通过 `TPS55288` 的寄存器将非默认通道置于 disable/standby，不主动稳压输出。
- Strategy B（fallback）: 若器件不支持独立 disable，且硬件 `EN/UVLO` 实际共网（`TPS_EN`），则本计划的“默认仅启用一路输出”需改口径（例如：两路都启用但把其中一路配置为“不会接管负载”的安全档位），并同步更新 `docs/plan/0005:tps55288-control/PLAN.md` 的 MUST 与验收标准。

Decision (confirmed): 采用 Strategy A（通过 I2C/寄存器实现每颗芯片的独立控制；`TPS_EN` 仅作为系统级使能网）。

### Telemetry（required）

- `telemetry_period_ms`: integer
  - Meaning: 遥测输出周期（打印频率）
  - Default: `500`

## Outputs (observable)

- Boot log:
  - Must include: `I2C address`, `selected default_enabled_channel`, and `target vout/ilimit` (human-readable)
  - Must include failures: address + stage + error category

## Validation rules

- 若 `default_enabled_channel` 对应器件 I2C 不可达（NACK/timeout），固件必须进入保守策略：
  - 不得反复高速重试刷屏日志
  - 不得假设另一颗一定可用（可尝试但需清晰记录并按策略处理）
- 若两颗均不可达：固件必须保持可运行（不 panic），并避免对输出产生不可控的“反复开关”行为（具体行为以实现策略冻结为准）。
