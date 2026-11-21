# BQ40 低压预充电动作

> 本文件是该主题的长期需求契约。当前实现事实写入 `IMPLEMENTATION.md`；生命周期与变更参考写入 `HISTORY.md`。

## Context and Scope

- Context: `BQ40Z50` 的 `CUVC` 或 `CUV` 会与 `XDSG` 一起关闭充放电 FET；标准 `DischargeAuthorization` 要求 `SafetyStatus=0`，因此无法清除这类易失锁存。设备停在 `SELF CHECK`、12V 输出关闭，主人缺少设备端发起受控预充电的入口。
- In scope: 显式动作的入口与确认、门控、BQ40 reset、10s 验证、`uv_suspect` 与 100mA 交接、失败锁存、结果词表、CLI/HTTP/Web 面、可观测性和文档边界。
- Out of scope: 自动 `recovering_low_voltage` 策略（由 `bq25792-charge-policy` 拥有）、host 侧 low-voltage maintenance flow、BQ40 Data Flash 写入、PF 清除、Dashboard 门控和充电温度 profile。

## Terms and Interfaces

- `Low-Voltage Precharge`: 主人显式确认的一次性 BQ40 低压恢复动作，不是自动策略。
- `uv_suspect`: 本次 boot 内保持的 UV 可疑会话标志。
- `recovering_low_voltage`: 现有自动充电策略状态，由 `bq25792-charge-policy` 拥有。
- `low-voltage maintenance flow`: host 侧 `bq40-comm-tool` 维护流程。
- `pack baseline`: 板级冻结的 BQ40 DF 保护与恢复基线。
- `CUVC` / `CUV` / `XDSG`: BQ40 原始保护与状态位，不是 UI 的宽泛标签。
- Interface: 前面板 `SELF CHECK → BQ40Z50`、CLI `mains-aegis device <id> recovery bms-low-voltage-precharge`、devd HTTP `POST /api/v1/devices/{id}/recovery/bms-low-voltage-precharge`、设备 LAN HTTP `POST /api/v1/recovery/bms-low-voltage-precharge`、Web 设备页 BQ40 问题面板。

## Requirements

### REQ-BQ40-LVP-001

- The system MUST expose the Low-Voltage Precharge action on the front panel, CLI, devd HTTP, device LAN HTTP, and Web device page.
- The action MUST require an explicit owner confirmation on every surface and MUST NOT start on boot, self-check, reconnect, or polling.
- 确认强度为普通确认；CLI/Web 不要求输入 `PRECHARGE`，也不要求额外确认参数。
- Inputs: 已通过门控的设备、主人确认。
- Outputs: 一个 `UvPrechargeResult` 事务。

### REQ-BQ40-LVP-002

- The system MUST allow the action only when the raw BQ40 `CUVC` or `CUV` safety bit is set.
- `RCA` MUST NOT block the action.
- The system MUST reject the action when `PFStatus != 0`、`no_battery=true`、`PRES=false`、`FET_EN=false`、输入不在线、电池温度不在 `15–45°C`、pack 不在 `2500–16800mV`、已经 `discharge_ready=true`、已有恢复事务 pending，或 UV 条件不成立。
- 读不到 `cuv_recovery_mv` 或 `protection_configuration` 时 MUST NOT 阻断动作；结果和诊断 MUST 标记 `baseline_unverified`。
- Inputs: BQ40 SafetyStatus/PFStatus/OperationStatus/ManufacturingStatus、charger 输入、温度、pack 电压。
- Outputs: accepted 或 `Blocked(reason)`。

### REQ-BQ40-LVP-003

- The accepted action MUST issue BQ40 `device_reset`（ManufacturerAccess `0x0041`）。
- The system MUST verify within 10s that `SafetyStatus=0`、`CUVC=0`、`XDSG=0`、`discharge_ready=true`。
- 验证失败 MUST 返回 `ResetFailed`，MUST NOT 自动重试或连续 reset。
- Inputs: accepted action。
- Outputs: `ResetFailed` 或进入 100mA 交接。

### REQ-BQ40-LVP-004

- 成功 reset 后 MUST 置位 `uv_suspect`，并清除 ESP32 侧 `termination_incomplete` 锁存。
- 该会话 MUST 以 `100mA` 目标充电：`ITERM=40mA`、`EN_TERM=0`、`VREG` 取 BQ40 请求并 clamp 到 `16800mV`。
- `uv_suspect` MUST 在本次 boot 内保持，并在一次成功满充后清除；`cell_min >= 3100mV` 连续 3 个成功 BMS 样本后 MUST 恢复 `500mA`；之后再次 `<3100mV` MUST 回到 `100mA`。
- 现有 `recovering_low_voltage` 策略继续拥有充电循环；本动作不复制充电控制。
- Inputs: reset success、BQ40 request、cell telemetry。
- Outputs: `recovering_low_voltage / 100mA` 或 `500mA` handoff。

### REQ-BQ40-LVP-005

- The action MUST observe the 100mA precharge for at most 60 minutes。
- `Success` MUST require reset verification、outputs active、以及 10 分钟稳定 100mA 且期间不出现 `CUVC/CUV/XDSG`；`3100mV ×3 → 500mA` 是后续观察，不是成功条件。
- `ResetFailed`、`ProtectionRetripped` 或 `Timeout` MUST latch automatic charging off until a new explicit Low-Voltage Precharge action or a reboot。
- 手动 START MUST NOT 自动清除该锁存，且继续受现有 BMS/温度/PD/输出门控约束。
- Inputs: action transaction、BMS/charger/output telemetry。
- Outputs: `Success` / `ProtectionRetripped` / `Timeout` / `ResetFailed`，以及自动充电锁存。
- 本动作的 `Success` 仍要求输出 active；`runtime.mode=blocked` 下的自动 100mA 由 `bq25792-charge-policy` 拥有，不以本动作结果为准。

### REQ-BQ40-LVP-006

- The system MUST expose `UvPrechargeResult` as `Success`、`Blocked(reason)`、`ResetFailed`、`ProtectionRetripped`、`Timeout`、`NotDetected`；MUST NOT 复用 `BmsResultKind`。
- 前面板 MUST 使用 `PRECHARGE`、`Start low-voltage precharge?`、`Precharge active`、`Precharge stopped: <reason>`；Web/CLI MUST 使用同一结果和 reason。
- Dashboard 门控 MUST 保持不变：输出未 active 时留在 `SELF CHECK`，恢复且输出 active 后自动进入 Dashboard。
- Inputs: action state and result。
- Outputs: 一致的前面板/CLI/Web 结果。

### REQ-BQ40-LVP-007

- `status`/`diag-snapshot`/CLI/Web MUST 暴露 `uv_suspect`、动作状态、结果、reason、`baseline_unverified`、reset 前后事实和自动充电锁存。
- The action MUST NOT write BQ40 Data Flash、change protection thresholds、or clear PF。
- `device_reset` 的已知副作用 MUST 记录：`MaxError=100%`、SOC 可能按电压重算；不改变 DF、保护阈值或 PF。
- Inputs: action execution。
- Outputs: 可审计的诊断字段和结果。

## Verification

### VER-BQ40-LVP-001

- Method: host tests and fixtures for each surface and no-auto-start behavior。
- covers: `REQ-BQ40-LVP-001`
- Pass condition: every surface can request the action; boot/self-check/reconnect never starts it; ordinary confirmation is required。

### VER-BQ40-LVP-002

- Method: host gate-matrix tests for allowed and rejected inputs。
- covers: `REQ-BQ40-LVP-002`
- Pass condition: CUVC/CUV allow, RCA allow, every hard block rejects with the expected reason, and an unreadable baseline produces `baseline_unverified` without blocking。

### VER-BQ40-LVP-003

- Method: host transaction tests with a mock BQ40 and reset-verification traces。
- covers: `REQ-BQ40-LVP-003`
- Pass condition: reset is issued once, verification uses the 10s window, failure maps to `ResetFailed`, and no automatic retry occurs。

### VER-BQ40-LVP-004

- Method: pure policy tests for `uv_suspect`, 100mA target, and 3100mV handoff。
- covers: `REQ-BQ40-LVP-004`
- Pass condition: the action sets `uv_suspect`, clears the latch, uses 100mA/ITERM=40mA/EN_TERM=0, hands off after three samples at 3100mV, and returns to 100mA below 3100mV。

### VER-BQ40-LVP-005

- Method: host time and failure-path tests。
- covers: `REQ-BQ40-LVP-005`
- Pass condition: 60-minute timeout, 10-minute success, failure latch, and manual START does not clear the latch。

### VER-BQ40-LVP-006

- Method: UI/contract tests for the result enum and copy on front panel, CLI, and Web。
- covers: `REQ-BQ40-LVP-006`
- Pass condition: result and reason are identical across surfaces; Dashboard eligibility is unchanged。

### VER-BQ40-LVP-007

- Method: diagnostics inspection and a static check that the change adds no DF/PF write path。
- covers: `REQ-BQ40-LVP-007`
- Pass condition: diagnostics expose the required fields; no DF/PF writes; reset side effects are documented and observable。

### VER-BQ40-LVP-008

- Method: HIL on `mains-aegis-198840` or another pack with CUVC/XDSG。
- covers: `REQ-BQ40-LVP-003`, `REQ-BQ40-LVP-004`, `REQ-BQ40-LVP-005`
- Pass condition: reset verification passes, outputs become active, 100mA is stable for 10 minutes without a re-trip, and the 3100mV×3→500mA handoff is observed。

### VER-BQ40-LVP-009

- Method: negative HIL or mock cases for the gate and latch boundaries。
- covers: `REQ-BQ40-LVP-002`, `REQ-BQ40-LVP-005`, `REQ-BQ40-LVP-006`
- Pass condition: no input, PF, no battery, temperature out of range, non-UV, and failure-latch cases behave as specified; no surface auto-starts the action。

## Related ADRs

- [ADR 0005: BQ40 低压预充电使用显式 device_reset](../../adr/0005-bq40-low-voltage-precharge.md)

## Visual Evidence

- None

## References

- `./IMPLEMENTATION.md`
- `./HISTORY.md`
- `../bq25792-charge-policy/SPEC.md`
- `../bq40-self-check-result-dialogs/SPEC.md`
- `../mains-aegis-devd/SPEC.md`
- `../mains-aegis-cli-devd-alignment/SPEC.md`
- `../../recovery/low-voltage-recovery.md`
