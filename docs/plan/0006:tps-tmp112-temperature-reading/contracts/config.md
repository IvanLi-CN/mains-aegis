# TMP112A 温度采样：通道/地址映射与采样周期（Config）

本契约定义固件读取 `TMP112A`（TPS 热点）时需要冻结的最小配置形状：**通道命名、I2C 地址、采样周期与单位口径**。

## Inputs（fixed）

### I2C 总线（fixed）

- Bus: `I2C1`
- Pins: `GPIO48=I2C1_SDA`，`GPIO47=I2C1_SCL`
- Frequency: `400kHz`
- Source of truth: `docs/i2c-address-map.md`

### 设备与通道映射（fixed）

| Logical channel | Board naming | I2C address | Notes |
| --- | --- | ---: | --- |
| `tps_a_hotspot` | `TMP112A(OUT-A 热点 / TPS-A 热点)` | `0x48` | `ADD0=GND`（见 `docs/power-monitoring-design.md`） |
| `tps_b_hotspot` | `TMP112A(OUT-B 热点 / TPS-B 热点)` | `0x49` | `ADD0=V+`（见 `docs/power-monitoring-design.md`） |

## Sampling（required）

- `temp_period_ms`: integer
  - Meaning: 温度采样与日志输出周期（ms）
  - Default: `500`

## Units（required）

- `temp_c_x16`: integer (signed)
  - Meaning: 温度（`°C * 16`，即 `1/16 °C`）
  - Rationale: 与 `TMP112A` 原生分辨率一致（`0.0625°C`）；避免浮点；对负温无歧义
  - Display conversion: `temp_c = temp_c_x16 / 16`

## THERM_KILL_N（required）

- Net: `THERM_KILL_N`
- MCU GPIO: `GPIO40`
- Output field: `therm_kill_n=<0|1>`（见 `./cli.md`）

## TMP112A 解码（fixed）

- Temperature register: normal mode（12-bit）
- Resolution: `1/16 °C` per LSB（`0.0625 °C`）
- Signed: two’s complement（负温为补码表示）

## Error behavior（required）

- 读取失败时仍按周期输出一行日志（每通道一行）；失败字段使用 `err(<kind>)`（具体分类见 `./cli.md`）。
- 读失败的重试/刷屏控制必须在实现中限频；限频口径见 `./cli.md`。
