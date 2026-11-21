# ADR 0005: BQ40 低压预充电使用显式 device_reset

## 状态

已接受；设计合同见 [BQ40 低压预充电动作 SPEC](../specs/bq40-low-voltage-precharge/SPEC.md)。

## 背景

现场 4S pack 在深度放电后出现 `CUVC=1` 与 `XDSG=1`，BQ40 关闭了 CHG/DSG FET。标准 `DischargeAuthorization` 要求 `SafetyStatus=0`，因此会以 `cell_undervoltage` 拒绝，无法进入已有的 `device_reset` 分支；设备停在 `SELF CHECK`、12V 输出关闭，标准固件没有设备端入口发起受控预充电。host 侧 `low-voltage-recovery.sh` 仍可用，但它需要临时工具固件、依赖 pack baseline preflight，而且不是主人从设备界面能直接发起的动作。

## 决策

- 新增显式 **Low-Voltage Precharge** 动作，入口覆盖前面板、CLI、devd HTTP、设备 LAN HTTP 与 Web；所有入口都要求主人显式确认，不在 boot、自检、重连或轮询时自动触发。
- 动作只允许 `CUVC` 或 `CUV` 原始位；`RCA` 不阻断。`PFStatus`、`no_battery`、`PRES=false`、`FET_EN=false`、无输入、温度超出 `15–45°C`、pack 超出 `2500–16800mV`、已经 ready、已有恢复 pending 或非 UV 条件全部硬阻断。
- pack baseline 读不到时允许一次性动作，但结果和诊断标记 `baseline_unverified`；不把 host maintenance flow 作为唯一入口。
- 动作发 BQ40 `device_reset`（ManufacturerAccess `0x0041`），10s 内验证 `SafetyStatus=0`、`CUVC=0`、`XDSG=0`、`discharge_ready=true`；失败返回 `ResetFailed`，不自动重试。
- reset 成功后置位 `uv_suspect`，清除 ESP32 侧 `termination_incomplete` 锁存，并交给现有策略以 `100mA`（`ITERM=40mA`、`EN_TERM=0`）充电；`cell_min≥3100mV` 连续 3 个样本后恢复 `500mA`，再次 `<3100mV` 回到 `100mA`。
- 100mA 观察窗口 60 分钟；`Success` 要求 reset 验证、输出 active 和 10 分钟稳定 100mA。`ResetFailed`、`ProtectionRetripped`、`Timeout` 锁住自动充电，直到新的显式动作或重启；手动 START 不自动清锁。
- 动作不写 BQ40 Data Flash、不改保护阈值、不清 PF。`device_reset` 的 `MaxError=100%` 与可能的 SOC 重算作为已知副作用记录。
- Dashboard 门控不变：输出未 active 时留在 `SELF CHECK`，恢复且输出 active 后自动进入 Dashboard。

## 考虑过的选项

- **只走 host maintenance flow**：需要临时 `bq40-comm-tool` 固件和 baseline preflight，不能在设备界面发起；现场深度放电时入口太远。
- **只走 PCHG 路径**：板级 `FET Options=0x19` 让 PRECHARGE 使用 CHG FET，而 `CUVC` 下 CHG FET 已关闭，PCHG 路径绕不过去。
- **放宽 `DischargeAuthorization` 门控**：其 `device_reset` 分支仍要求 `SafetyStatus=0`，放宽入口会形成循环依赖，同时削弱既有安全门。
- **自动恢复**：违反“恢复必须由主人显式确认”的既有契约，并可能在高频失败时反复 reset。

## 后果

- BQ40 运行态会复位，`MaxError=100%`，SOC 可能按电压重算；没有 DF、保护阈值或 PF 变化。
- 新增 `UvPrechargeResult`、CLI/HTTP 端点、Web/前面板动作和诊断字段；需要 host 测试与 HIL 验收。
- 首个 HIL 目标是当前 `mains-aegis-198840`；如果 reset 后 `CUVC/XDSG` 仍存在，动作失败并回退到 host maintenance flow。
- 自动 `recovering_low_voltage` 仍由 `bq25792-charge-policy` 拥有；本 ADR 只增加显式恢复入口和 `uv_suspect` 迟滞。

## 关联规范

- [BQ40 低压预充电动作 SPEC](../specs/bq40-low-voltage-precharge/SPEC.md)
- [BQ25792 500mA 充电策略与 DC 过流降档 SPEC](../specs/bq25792-charge-policy/SPEC.md)
- [BQ40 自检异常态与结果弹窗 SPEC](../specs/bq40-self-check-result-dialogs/SPEC.md)
- [BQ40 主板 DF 保护基线 SPEC](../specs/bq40-mainboard-df-protection-baseline/SPEC.md)
- [Low-Voltage Recovery Maintenance](../recovery/low-voltage-recovery.md)
