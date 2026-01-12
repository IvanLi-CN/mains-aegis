# I2C/SMBus 地址与中断分配（设计拟定）

本文档记录本项目的 **7-bit I2C/SMBus 地址分配**、各器件支持的总线速率，以及**中断/告警信号**的分组方案。

> 说明：部分手册会用“8-bit 地址（含 R/W 位）”表示（例如 `0x16`），本文统一使用 **7-bit 地址**（例如 `0x0B`）。

---

## 1. 总线与目标速率

### 1.1 `I2C1`（主总线）

- 目标速率：`400kHz`（I2C Fast mode / SMBus XL）
- 上拉：按总线电容选型（典型 `4.7kΩ~10kΩ` 上拉到 `3.3V`）

### 1.2 `I2C2`（面板/扩展总线）

- 目标速率：`400kHz`（Fast mode）
- 上拉：按总线电容选型（典型 `4.7kΩ~10kΩ` 上拉到 `3.3V`）

---

## 2. 地址分配（7-bit）

### 2.1 `I2C1`

> 注：本项目 `I2C1` 实际运行速率固定为 `400kHz`；下表“最高 `fSCL`（器件能力）”仅用于记录器件 datasheet 能力（因此可能出现 `1MHz/2.44MHz` 等），不代表本项目运行配置。

| 7-bit 地址 | 器件 | 最高 `fSCL`（器件能力） | 地址配置（硬件） | 中断/告警输出 | 中断线分组（暂定） |
|---:|---|---:|---|---|---|
| `0x0B` | `BQ40Z50RSMR-R2` | `100kHz`（SMBus）；`400kHz`（SMBus XL） | 固定 | `BTP_INT` | `BMS_BTP_INT_N` |
| `0x40` | `INA3221` | `400kHz`（Fast）；`2.44MHz`（High-speed） | `A0=GND` | `PV/CRITICAL/(WARNING)`（开漏） | `PV→INA3221_PV；CRITICAL/WARNING→I2C1_INT` |
| `0x48` | `TMP112A`（TPS55288 温度：OUT-A） | `1MHz`（Fast Mode Plus） | `ADD0=GND` | `ALERT`（开漏） | `THERM_KILL_N`（硬件过温停机链路；MCU 同一 GPIO 可拉低强制停机） |
| `0x49` | `TMP112A`（TPS55288 温度：OUT-B） | `1MHz`（Fast Mode Plus） | `ADD0=V+` | `ALERT`（开漏） | `THERM_KILL_N`（硬件过温停机链路；MCU 同一 GPIO 可拉低强制停机） |
| `0x50` | `M24C64-FMC6TG` | `1MHz` | `E2/E1/E0=0` | — | — |
| `0x6B` | `BQ25792RQMR` | `1MHz` | 固定 | `INT`（开漏；低有效 `256µs` 脉冲） | `BQ25792_INT` |
| `0x74` | `TPS55288`（OUT-A） | `1MHz` | `MODE` 电阻选择 `0x74` | `FB/INT`（内部反馈模式下为故障指示输出；需上拉） | `INT_TPS` |
| `0x75` | `TPS55288`（OUT-B） | `1MHz` | `MODE` 电阻选择 `0x75` | `FB/INT`（内部反馈模式下为故障指示输出；需上拉） | `INT_TPS` |

### 2.2 `I2C2`

> 预留给面板侧 I2C 器件（若未来引入，再补充地址分配）。

---

## 3. 中断/告警线分组（暂定）

| 中断线 | 来源（建议线与） | 类型 | 上拉 | MCU GPIO（暂定） |
|---|---|---|---|---|
| `INT_TPS` | `TPS55288(OUT-A).FB/INT` + `TPS55288(OUT-B).FB/INT` | 开漏/需上拉（故障指示） | `3.3V` | `GPIO37` |
| `I2C1_INT` | `INA3221.CRITICAL` (+ `INA3221.WARNING`)（+ Type‑C/PD 控制器中断输出可选） | 开漏线与 | `3.3V` | `GPIO34` |
| `BQ25792_INT` | `BQ25792.INT`（开漏；低有效 `256µs` 脉冲） | 开漏/脉冲型中断 | `3.3V` | `GPIO33` |
| `INA3221_PV` | `INA3221.PV`（Power Valid；欠压时拉低） | 开漏/电平型告警 | `VPU=3.3V` | `GPIO18` |
| `BMS_BTP_INT_N` | `BQ40Z50.BTP_INT`（经 `NMOSFET` 取反） | 有效极性可配置（高有效/低有效）；按需要外加上拉 | 视需要外加 | `GPIO6` |
| `THERM_KILL_N` | `TMP112A(OUT-A).ALERT` + `TMP112A(OUT-B).ALERT`（开漏线与，串 `~1kΩ` 建议） | 开漏/电平型告警 | `3.3V` | `GPIO38`（MCU 可读/也可开漏拉低强制停机） |

> 注意：`BQ25792.INT` 为 `256µs` 短脉冲中断；若与“可能长期拉低”的告警脚共线，脉冲可能被掩盖。本项目将其单独接到 `BQ25792_INT`，避免被 `I2C1_INT` 上的电平告警掩盖。

> SMBus ARA（Alert Response Address）：可对 7-bit `0x0C` 发起 Read（8-bit `0x19`）。`INA3221` 支持该机制，可用于在 `I2C1_INT` 拉低时快速定位来源。  
> 说明：`TMP112` 也支持 ARA，但仅在你把 `TMP112A.ALERT` 接入 `I2C1_INT` 且配置为 Interrupt mode（`TM=1`）时才有意义；本项目当前将 `TMP112A.ALERT` 用于硬件过温停机链路（`THERM_KILL_N`），不走 `I2C1_INT`。
