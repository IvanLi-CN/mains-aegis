# BQ25792 500mA 充电策略与 DC 过流降档 演进历史

> 这里记录影响当前规范理解的关键演进；当前有效合同仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-03-28: 建立规格并按“500mA 常规充电 + DC 独占过流降到 100mA + 80%/3.70V 启停 + 满充锁存”收敛主线策略口径。
- 2026-04-05: charger detail 补充 `TS_WARM -> WARM` 状态与说明文案；`LOCK` 诊断补充 `ChargingStatus(0x55)` 原始位图，便于区分包侧 inhibit / suspend。
- 2026-04-05: `BQ25792` ADC 遥测改为 `MSB-first` 解码；`IBUS/VBUS/VBAT/VSYS` 从 byte-swapped 假值恢复到真实量级，并把 `input_power_anomaly` 的端序误报从主线诊断中排除。
- 2026-04-06: 主线 charger state machine 正式作为 SoT；`LOAD` 增加 `2入3出` 回差，输出功率未知改为保守禁充，首页 `ChargeCard` 同步到 runtime 紧凑 token。
- 2026-06-14: DC IN 停充判据改为 `TPS 总输出电流 > 100mA` 优先；根因在 cooldown 期间持续可见。DC profile 改为 `IINDPM=1000mA`、`VINDPM=输入电压*96%`；`/api/v1/status`、`diag-snapshot`、CLI trace 与 Web PowerPage/trace console 同步暴露 `tps_total_iout_ma`、阈值和停充原因。
- 2026-06-04: 现场复核发现 `ICHG=100mA` 搭配 `ITERM=120mA` 会让 BQ25792 立即进入 `termination_done` 且 `IBAT_ADC=0`；修正 `REG09` 16-bit 写入路径，并让 `100mA` 模式写入 `ITERM=40mA`，确保硬件能持续预充。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
