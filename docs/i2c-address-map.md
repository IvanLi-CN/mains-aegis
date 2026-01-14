# I2C/SMBus 地址与中断分配（设计拟定）

## 总线（目标速率）

| 总线 | 目标速率 | SDA | SCL | INT/告警汇总 |
|---|---:|---|---|---|
| `I2C1` | `400kHz` | `GPIO48`（`I2C1_SDA`） | `GPIO47`（`I2C1_SCL`） | `GPIO33`（`I2C1_INT`） |
| `I2C2` | `400kHz` | `GPIO8`（`I2C2_SDA`） | `GPIO9`（`I2C2_SCL`） | `GPIO7`（`I2C2_INT`） |

## 设备地址（7-bit）

| 总线 | 7-bit 地址 | 器件 | 支持速率（器件能力） | 中断/告警输出 | MCU GPIO |
|---|---:|---|---:|---|---|
| `I2C2` | `0x22` | `FUSB302BMPX` | `100kHz`；`400kHz`；`1MHz`（FMP） | `INT_N`（开漏；低有效） | `GPIO7`（`I2C2_INT`） |
| `I2C1` | `0x40` | `INA3221` | `400kHz`；`2.44MHz`（HS） | `PV/WARNING/CRITICAL`（开漏） | `GPIO37(PV)/GPIO38(CRITICAL)/GPIO39(WARNING)` |
| `I2C1` | `0x48` | `TMP112A`（OUT-A 热点） | `1MHz`（FMP） | `ALERT`（开漏） | `GPIO40`（`THERM_KILL_N`） |
| `I2C1` | `0x49` | `TMP112A`（OUT-B 热点） | `1MHz`（FMP） | `ALERT`（开漏） | `GPIO40`（`THERM_KILL_N`） |
| `I2C1` | `0x50` | `M24C64-FMC6TG` | `1MHz` | — | — |
| `I2C1` | `0x6B` | `BQ25792RQMR` | `≤1MHz` | `INT`（开漏；低有效 `256µs` 脉冲） | `GPIO17`（`CHG_INT`） |
| `I2C1` | `0x74` | `TPS55288`（OUT-A） | `1MHz` | `FB/INT`（需上拉） | `GPIO33`（`I2C1_INT`） |
| `I2C1` | `0x75` | `TPS55288`（OUT-B） | `1MHz` | `FB/INT`（需上拉） | `GPIO33`（`I2C1_INT`） |
| `I2C1` | `0x0B` | `BQ40Z50RSMR-R2` | `100kHz`（SMBus）；`400kHz`（SMBus XL） | `BTP_INT`（可配置极性） | `GPIO21`（`BMS_BTP_INT_H`） |
| `I2C2` | `0x21` | `TCA6408A`（面板 IO 扩展） | `100kHz`；`400kHz` | `INT`（开漏；可选） | `GPIO7`（`I2C2_INT`） |

> 备注：
> - 前面板网表中 `TCA6408A.ADDR` 上拉到 `3V3`，因此地址为 `0x21`。
> - `TPS55288` 的 I2C 地址由 `MODE`→`AGND` 电阻选择（并同时决定 `VCC` 来源与轻载 `PFM/FPWM`）。本项目约定“外部 `5V` 供 `VCC` + 默认 `PFM`”，因此：OUT‑A 用 `75.0kΩ`（`0x74`），OUT‑B 用 `DNP/Open`（`0x75`）；详见 `docs/ups-output-design.md`。

## 中断/告警信号（汇总）

| 信号 | 来源 | 类型 | MCU GPIO |
|---|---|---|---|
| `I2C1_INT` | `TPS55288(OUT-A).FB/INT` + `TPS55288(OUT-B).FB/INT` | 开漏线与 | `GPIO33` |
| `I2C2_INT` | `TCA6408A.INT` + `FUSB302B.INT_N`（可选：触摸 `IRQ`） | 开漏线与 | `GPIO7` |
| `CHG_INT` | `BQ25792.INT`（低有效 `256µs` 脉冲） | 开漏/脉冲型中断 | `GPIO17` |
| `INA3221_PV` | `INA3221.PV` | 开漏/电平型告警 | `GPIO37` |
| `INA3221_CRITICAL` | `INA3221.CRITICAL` | 开漏/电平型告警 | `GPIO38` |
| `INA3221_WARNING` | `INA3221.WARNING` | 开漏/电平型告警 | `GPIO39` |
| `BMS_BTP_INT_H` | `BQ40Z50.BTP_INT`（经 `NMOSFET` 取反） | 可配置极性 | `GPIO21` |
| `THERM_KILL_N` | `TMP112A(OUT-A).ALERT` + `TMP112A(OUT-B).ALERT` | 开漏/电平型告警 | `GPIO40` |
