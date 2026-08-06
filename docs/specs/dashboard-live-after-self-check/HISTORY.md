# 自检后切入真实仪表盘 演进历史

> 这里记录影响当前规范理解的关键演进；当前有效合同仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-08-06: Dashboard eligibility 明确消费未缩窄的正常固件双路请求；单路健康不再被视为
  自检完成。

- 2026-03-15: `PIN W` 数据源改为 `VIN / INA3221 CH3`；DC 输入在线时 `PIN` 区块继续显示，逆流/空载样本显示 `0.0W`，缺失样本显示 `N/A`。
- 2026-03-15: 自检页不再作为 steady-state 默认页；切换为“自检 -> 真实 Dashboard”。
- 2026-03-15: live Dashboard 的市电真相源统一改为 `DC5025 VIN>=3V`，不再把 charger `input_present` 当成 UI 的 mains 判定输入。
- 2026-03-15: `VIN` 瞬时采样缺失时保留最近一次已知的市电状态，避免 INA CH3 单次读失败把 live Dashboard 误切到 `BACKUP/NOAC` 分支；连续缺失则退回 charger `input_present` 兜底。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
