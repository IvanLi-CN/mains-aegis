# 遥测日志字段追加：TMP112A 温度 + THERM_KILL_N（CLI）

本契约定义固件在既有 `telemetry ...` 输出行中追加 `TMP112A` 温度字段与 `THERM_KILL_N` 电平字段的口径，用于人工验收与后续回归。

## Frequency

- Period: `temp_period_ms`（见 `./config.md`）
- Each period prints: **2 lines**（沿用既有 `telemetry`：`out_a` 与 `out_b` 各 1 行）

## Output format

本契约不改变既有 `telemetry ...` 行中 `#0005` 已定义字段；只在每行**追加**以下字段（顺序固定，单位固定）：

- `tmp_addr=<0x48|0x49>`（对应 `TMP112A` I2C 地址；`out_a→0x48`，`out_b→0x49`）
- `temp_c_x16=<int|err(kind)>`
- `therm_kill_n=<0|1>`（`GPIO40(THERM_KILL_N)` 电平；1=高，0=低）

允许追加字段，但不得改变以上字段的语义与单位。

`temp_c_x16` 的人类可读换算：

- `temp_c = temp_c_x16 / 16`（分辨率 `0.0625°C`；负温按有符号整型解释）

### Example

```text
telemetry ch=out_a addr=0x74 vset_mv=5000 vbus_mv=4984 current_ma=312 tmp_addr=0x48 temp_c_x16=400 therm_kill_n=1
telemetry ch=out_b addr=0x75 vset_mv=5000 vbus_mv=0 current_ma=0 tmp_addr=0x49 temp_c_x16=err(i2c_nack) therm_kill_n=1
```

## Error handling

若某个字段读取失败：

- 该行仍必须输出，且必须保留既有 `telemetry ...` 前缀与字段；
- 将失败字段替换为 `err(<kind>)` 占位（仅对 `temp_c_x16`；`tmp_addr` 必须仍可输出，`therm_kill_n` 不应失败）；
- `<kind>` 为稳定的错误分类字符串（短、稳定；避免输出长错误串刷屏）。

推荐 `kind` 候选（按需选择，最终集合需稳定）：

- `i2c_nack`
- `i2c_timeout`
- `i2c_bus`
- `decode`

## Rate limiting（required）

为避免总线故障时刷屏：

- 若连续失败：允许每周期仍输出两行 `telemetry ... temp_c_x16=err(...)`，但不得在同一周期内为同一通道输出多行错误（禁止“重试日志”刷屏）。
- 若实现希望额外输出“错误摘要/重试信息”，必须限频到 **≤1 次 / 5s** 且不得影响上述每周期两行输出的稳定性。
