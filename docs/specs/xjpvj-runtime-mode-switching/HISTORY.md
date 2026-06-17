# 历史记录（#xjpvj）

## 2026-06-16

- 新建运行态模式切换 topic spec，吸收此前散落在 #6qrjs、#g2kte、#eu2b8、#h43mk 中的模式定义与充电联动口径。
- 明确 `BYPASS` 不再属于自动运行态集合。
- 明确 `ASSIST` 改为基于 `VIN + TPS total output current` 判定，而不是输出 enable flag。
- 明确 `BACKUP` 只在确认无输入时进入，未知输入时保持上一确认模式。

## 2026-06-17

- 把 owner-facing canonical HIL 基线收敛到 `IsolaPurr 13.0V / 3.0A + LoadLynx 1A/2A/3A/3200mA`，替代此前低输入电压 bench 下容易过早触发 `ASSIST` 的结论。
- 记录当前 HIL 仍属于 bench compensation：产品语义尚未实现 `TPS standby 电压 / backup 额定电压` 双档目标。
- 记录本轮 `BACKUP` 自动切断依赖 IsolaPurr 设备侧 raw HTTP `enabled=0|1` workaround，而不是 released host `--enabled false|true` 路径。
- 把 `ASSIST` 扩展为内部 `assist_low / assist_rated` 两阶段合同，并把运行时 TPS 目标电压切换、`VIN drop + TPS iout` 升额判据、以及 owner-facing `assist_power_stage / assist_target_vout_mv` 观测字段写成当前板的规范真相源。
