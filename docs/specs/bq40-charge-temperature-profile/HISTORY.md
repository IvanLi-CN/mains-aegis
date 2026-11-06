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

## 离线唤醒边界

把 `VBAT_PRESENT_STAT=1` 同时作为普通充电和离线唤醒的前提，会在 BMS 切断 PACK 路径时形成循环依赖：需要外部充电源唤醒，却因 PACK 低压而不施加充电源。低电压例外因此仅属于受限的显式离线 Activation，温度和已观察到的 BQ40 安全证据仍保持原有 fail-closed 合同。

## Related Changes

- None

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
