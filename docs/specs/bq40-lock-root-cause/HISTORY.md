# BQ40 `LOCK` 根因锁定与修复 演进历史

> 这里记录影响当前规范理解的关键演进；当前有效合同仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-04-13: 新建 spec，冻结 `LOCK` 根因锁定与修复的证据门、分流规则与快车道收口条件。
- 2026-04-13: 完成 `0x55` 原始块读与 BQ40/BQ25792 低频诊断观测补强，并通过主机侧验证。
- 2026-04-13: 依据 live failure evidence 命中 `termination` 分流门；实施 `BQ40 Current at EoC -> BQ25792 ITERM` 对齐修复，并完成 clean build 上板验证。
- 2026-04-13: 依据后续 live evidence 补充确认 `HT -> IN -> XCHG` 是新的前置 blocker，并把 live DF 主板基线的起充高温门槛上调到 `40°C`、回差上调到 `2°C`。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`

## 历史 EOC 与 taper 的分离

反复补电调查确认 201mA 历史状态参与 200mA 目标门控可导致软件禁充。当前合同转为真实 taper=120mA 与 ITERM=80mA，删除普通补电分支；旧日志只保留历史依据，不再作为当前设置来源。

## 历史证据快照（Historical evidence snapshot）

- `ChargingStatus(0x55)` 已确认是 `H3` 三字节块读而不是 `H4`；runtime 现在能稳定输出 `declared_len/payload_len/raw bytes/source/failure`，不再只有 `block_too_short`。
- `/Users/ivan/Projects/Ivan/mains-aegis/.mcu-agentd/monitor/esp/20260412_224305.mon.ndjson` 里的失败态证据已经满足 `termination` 分流门：
  - `ChargingStatus raw=0x1820`，`VCT=false`，`IN=true`，`HV=true`
  - `SafetyStatus[OC]=1`
  - `OperationStatus[XCHG]=1`，`CHG FET=false`
  - `No Valid Charge Term=2`
  - `Update Status=4`
  - `No Of Qmax Updates=0`
  - `No Of Ra Updates=0`
  - `BQ25792 EN_TERM=true`
  - `BQ25792 ITERM=200mA`
  - `BQ40 Current at EoC=109mA`
- 以下为早期 `termination` 修复的历史证据，Current at EoC 对齐策略已由本文充电终止合同取代：
  - 主线固件会把 `BQ40 Current at EoC` 对齐到 `BQ25792 ITERM` 的 `40mA` 步进，当前 pack 的 `109mA` 会下调为 `80mA` 目标。
  - `/Users/ivan/Projects/Ivan/mains-aegis/.mcu-agentd/monitor/esp/20260412_232036.mon.ndjson` 已证明新固件在 runtime 中产生了 `policy_term_target_ma=Some(80)`。
  - `/Users/ivan/Projects/Ivan/mains-aegis/.mcu-agentd/monitor/esp/20260412_233444.mon.ndjson` 已进一步证明该对齐值在 `LOCK` 态也真正落到了 charger 寄存器：同一段日志里同时出现 `policy_term_target_ma=Some(80)`、`iterm_ma=Some(80)`、`applied_iterm_ma=Some(80)` 与 `term_ctrl=Some(58114)`。
  - 同一份最新 monitor 也确认 pack 仍处于 `rsoc_pct=97`、`SafetyStatus[OC]=1`、`XCHG=1`、`VCT=false`、`No Valid Charge Term=2` 的旧失败态，因此当前硬 blocker 已经收窄为：必须先获得一次 `<90%` 解锁基线，才能完成 M3/M5 的闭环复验。
- `/Users/ivan/Projects/Ivan/mains-aegis/.mcu-agentd/monitor/esp/20260412_233444.mon.ndjson` 的后续观测显示，当 pack 降到 `rsoc_pct=80` 后，旧 `OC` 已清除（`safety_status=0`、`oc=false`），但 `ChargingStatus raw=0x1820` 同时出现 `HT=true`、`HV=true`、`IN=true`，并继续维持 `OperationStatus[XCHG]=1`、`CHG FET=false`。同段 `bms_diag_temp` 里 `t3_c_x10=301`、`t4_c_x10=551`、`hysteresis_c_x10=10`，而实时温度只有 `31.7~32.5°C`；这证明当前 pack 的起充高温门槛过低，新的前置 blocker 是 `HT -> Charge Inhibit` 而不是 `OC`。
- 基于上述 live evidence，本轮额外把 live DF 主板基线的充电温区参数调整为：
  - `T3 Temp: 30.1°C -> 40.0°C`
  - `T4 Temp: 55.1°C -> 55.0°C`
  - `Hysteresis Temp: 1.0°C -> 2.0°C`
  目的不是掩盖 `OC`，而是去掉 `30°C` 环境下就被 `HT/IN` 禁充的错误前置 blocker，使后续闭环复验能够在合理环境温度下继续推进。

- 主人明确批准 A3/A4 从合并前调整为合并后验收；保留 OC 中断的失败事实与采样要求。PR #135 继续合并前软件、配置和独立审查门禁，GaugeStatus3 与禁充原因提示作为合并后排查范围。
