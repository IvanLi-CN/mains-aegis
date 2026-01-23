# 遥测日志输出（CLI）

本契约定义固件在串口/日志中输出的遥测（telemetry）格式，用于人工验收与后续回归。

## Frequency

- Period: `500ms`
- Each period prints: **2 lines**（`out_a` 与 `out_b` 各 1 行）

## Output format

每行必须包含以下字段（顺序固定，单位固定）：

- `telemetry` 固定前缀
- `ch=<out_a|out_b>`
- `addr=<0x74|0x75>`（对应 `TPS55288` I2C 地址）
- `vset_mv=<int>`
- `vbus_mv=<int>`
- `current_ma=<int>`

允许追加字段，但不得改变以上字段的语义与单位。

### Example

```text
telemetry ch=out_a addr=0x74 vset_mv=5000 vbus_mv=4984 current_ma=312
telemetry ch=out_b addr=0x75 vset_mv=5000 vbus_mv=0 current_ma=0
```

## Error handling

若某个字段读取失败：

- 该行仍必须输出，且必须保留 `telemetry ch=... addr=...` 前缀；
- 将失败字段替换为 `err` 占位：
  - `vset_mv=err(<kind>)`
  - `vbus_mv=err(<kind>)`
  - `current_ma=err(<kind>)`
- `<kind>` 为稳定的错误分类字符串（例如：`i2c_nack` / `i2c_timeout` / `decode`），避免输出长错误串刷屏。

示例：

```text
telemetry ch=out_a addr=0x74 vset_mv=5000 vbus_mv=err(i2c_nack) current_ma=err(i2c_nack)
```

## Notes

- `vset_mv` 来自 `TPS55288` 寄存器读回值（不是“上一次写入的缓存值”）。
- `vbus_mv/current_ma` 来自 `INA3221`（通道映射与换算见 `./config.md`）。
