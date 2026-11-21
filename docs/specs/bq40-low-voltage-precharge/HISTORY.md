# BQ40 低压预充电动作 主题历史

> 这里记录主题局部生命周期、替换、兼容性与必要背景；完整 ADR 取舍保留在 `docs/adr/`。单次任务流水账不放这里，规范正文仍以 `./SPEC.md` 为准。

## Lifecycle / Compatibility

- 新增主题；没有替代或被替代关系。

## Replacements / Background

- 2026-11-21：现场设备在深度放电后出现 `CUVC=1`、`XDSG=1`，标准 `DischargeAuthorization` 因 `SafetyStatus` 非零拒绝，设备停在 `SELF CHECK` 且 12V 输出关闭。
- 既有 `termination_incomplete` 低压恢复优先修复让自动策略进入 `recovering_low_voltage / 100mA`，但 BQ40 CHG FET 已关闭，BQ25792 看不到电池，无法继续充电。
- 决策：新增显式 Low-Voltage Precharge 动作，使用 BQ40 `device_reset` 清除易失锁存，再由现有 100mA 策略接管；不把 host maintenance flow 作为唯一入口。详见 ADR 0005。
- 本主题不替代 `bq25792-charge-policy` 的自动低压恢复，也不替代 `low-voltage-recovery.md` 描述的 host 维护流程。
- 2026-11-21 现场复采样：pack 为 `FD + RCA`、`cell_min≈2.95V`，但 `CUV/CUVC=0`、`XCHG/XDSG=0`、`Safety/PF=0`；BQ40 文档化的 `OperationStatus[CHG]=1`，AFE CHG FET 为 off，BQ25792 `vbat_present=false`。本主题的 `REQ-BQ40-LVP-002` 只接受 `CUV/CUVC`，因此该状态被拒绝。
- 2026-11-21 裁决：暂不新增 FD/RCA 显式 reset 动作。先修 `runtime.mode=blocked` 对 `recovering_low_voltage` 的误覆盖，并移除 `vbat_present=false` 的独立禁充；让自动 100mA 尝试。若实测仍无充电电流且 `afe_chg_fet` 仍为 off，再按 ADR 0006 评估显式 BQ40 reset。`Low-Voltage Precharge` 继续保持 CUV/CUVC 语义。

## Related Changes

- None

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
- `../../adr/0005-bq40-low-voltage-precharge.md`
