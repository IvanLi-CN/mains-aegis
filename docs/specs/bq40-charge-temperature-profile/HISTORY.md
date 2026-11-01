# BQ40 charge-temperature profile 主题历史

> 这里记录主题局部背景；当前有效合同仍以 `./SPEC.md` 为准。

## Lifecycle / Compatibility

- Lifecycle: active
- Compatibility: 本规范取代散落在 BMS、charger 和工具文档中的温度 profile 候选口径；不改变已有 BQ25792 runtime policy 的其他输入源、功率或 UI 规则。

## Replacements / Background

- 项目采用 EVE INR21700/50E 4S1P，并以电芯本体 `0–45°C` 作为当前充电窗口。
- 45～55°C 方案不属于当前 profile；如未来要支持，必须建立新的正式规格依据并重新评审，不得只修改 T3/T4 常量。
- `hardware-selection.md` 中旧的 NTC“待定”表述由后续 BMS/charger 设计中的已选 `10kΩ/β3380` 方案覆盖。
- 既有 `bq40-comm-tool` live baseline 是协议实现参考，不是本主题的 golden profile。

## Related Changes

- None

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
