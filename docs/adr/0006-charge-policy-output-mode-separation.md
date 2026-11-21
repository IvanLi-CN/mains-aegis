# ADR 0006: 输出模式与充电策略分离

## 状态

已接受。合同见 [BQ25792 充电策略 SPEC](../specs/bq25792-charge-policy/SPEC.md) 与 [UPS 运行态 SPEC](../specs/runtime-mode-switching/SPEC.md)。

## 背景

现场设备进入 `runtime.mode=blocked`：12V 输出门控无法证明，但 BQ40 的文档化充电门控是放行的，`charger.policy` 处于 `recovering_low_voltage`、目标 100mA。此前的 mode coupling 把 `blocked` 直接映射为 `runtime_blocked_no_charge`，充电被关闭；同时 `charger.vbat_present=false` 被当作独立禁充理由，而它是 BQ40/AFE 充电路径未释放时可预期的 BAT 节点现象。两者共同形成“输出阻断 → 禁充 → BAT 节点持续为低 → 继续阻断”的循环。

## 决策

- `runtime.mode` 与 `charger.policy` 是两个独立维度。`runtime.mode=blocked/assist/backup` 只约束输出或默认 non-charging 边界，不再无条件强制 `charger.allow_charge=false`。
- `recovering_low_voltage` 是唯一允许在这些模式下继续自动充电的 charger policy；前提是输入在线、无 `Safety/PF/XCHG/XDSG/charge-inhibit`，且 `cell_min >= 2500mV`。
- `cell_min < 2500mV` 是硬低压 floor，返回 `blocked_hard_low_voltage`，禁止自动充电。
- `charger.vbat_present=false` 不单独禁充。只有 `charger.vbat_present=true` 且 BMS 报告输出/充电路径断开（`op_dsg_fet=false`、`xdsg=true` 或 `afe_dsg_fet=false`）时，才按 backfeed contradiction 禁充。
- 100mA 低压恢复成功以实测充电电流（`charger.ibat_adc_ma` 或 BMS 正向电流 > 10mA 持续 60s）为准；`op_chg_fet=true` 只表示逻辑状态。
- `Low-Voltage Precharge` 的 CUV/CUVC 入口与输出 active 成功条件保持不变；它不承接 FD/RCA 的显式 reset。

## 考虑过的选项

- **保持 `blocked` 一律禁充**：安全但形成低压恢复死锁；需要额外显式 reset 才能恢复。
- **只要 `charger.policy=RECOV` 就无条件充电**：会绕过输入、温度和 BMS 保护，风险不可接受。
- **把 `vbat_present=false` 继续当硬禁充**：无法区分 AFE 路径未释放与真实 backfeed，导致恢复路径永远关闭。
- **新增独立 FD/RCA reset 动作**：语义更干净，但在自动 100mA 尚未验证前扩大接口面；本轮暂缓，若 AFE CHG 仍 off 再评估。

## 后果

- `runtime.mode=blocked` 下允许 100mA 自动充电；输出仍不进入 Dashboard，`output.gate_reason` 保持阻断。
- 需要新的 `blocked_hard_low_voltage` 状态与 backfeed contradiction 诊断；host 测试覆盖 floor、override suppression 和 backfeed 判据。
- 如果自动 100mA 仍无电流且 `afe_chg_fet` 保持 off，后续需要 ADR 0007 形式的显式 BQ40 reset 恢复。

## 关联规范

- [BQ25792 充电策略](../specs/bq25792-charge-policy/SPEC.md)
- [UPS 运行态模式切换](../specs/runtime-mode-switching/SPEC.md)
- [BQ40 低压预充电动作](../specs/bq40-low-voltage-precharge/SPEC.md)
- [BQ40 放电路径诊断](../../docs/solutions/firmware/bq40-discharge-path-diagnostics.md)
