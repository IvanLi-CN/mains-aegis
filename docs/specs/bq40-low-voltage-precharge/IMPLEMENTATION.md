# BQ40 低压预充电动作 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实，避免这些细节散落到 PR / Git 历史里。

## Current Status

- Lifecycle: active
- Implementation: 固件动作、前端面板、CLI/IPC、devd HTTP、设备 LAN HTTP 与 Web 设备页已实现；HIL 成功路径尚未完成。
- Catalog note: HIL 目标是 `mains-aegis-198840`（`serial-04f3bb3f5367`）。

## Implementation Coverage

- Requirement coverage: 覆盖 `REQ-BQ40-LVP-001` 至 `REQ-BQ40-LVP-007` 的固件、host、CLI/IPC、HTTP 与 Web 面；HIL 成功路径 `VER-BQ40-LVP-008` 仍未通过。
- Firmware: `firmware/src/output/mod.rs` 的 `bms_uv_precharge` 事务、BQ40 `device_reset`、10s 验证、100mA 交接与失败锁存；`firmware/src/net_contract.rs` 暴露 `policy.uv_suspect`、`uv_precharge_*` 字段；`firmware/src/net.rs` 增加 `POST /api/v1/recovery/bms-low-voltage-precharge`。
- Host/tooling: `tools/mains-aegis-host` 增加 devd route、IPC `device.recovery.bms_low_voltage_precharge` 与 CLI `device <id> recovery bms-low-voltage-precharge`。
- Web: 设备 Battery 页增加 Low-voltage precharge 面板、确认、请求、状态/结果展示与 2s 轮询。
- Verification commands: `just firmware-host-test`（581 passed）、`just host-test`（宿主：134 + 59 passed）、`just web-check`、`just web-build`、`just firmware-release`、`just flash-current-real`。
- Rollout facts: 现有 `termination_incomplete` 低压恢复优先修复是前提；本主题不拥有 host 侧 low-voltage maintenance flow，也不写 BQ40 Data Flash。

## Coverage / rollout summary

- 固件 host 测试覆盖 gate、reset 验证、`uv_suspect`、100mA/3100mV 交接和失败锁存；CLI/IPC 与 devd route 有单元测试；Web 通过 TypeScript 检查和构建。
- HIL 2026-11-21：首次刷入后设备处于 BQ40 `CUV/CUVC` 锁存，动作返回 `accepted=true`、`state=verifying`、`baseline_unverified=true`。该次采样暴露出 `derived.power` JSON 缺少分隔逗号的问题；修复后 `diag-snapshot --package derived.power` 可正常解析。
- 修复后再次刷入并采样：BQ40 `cuv=false`、`cuvc=false`、`xchg=false`、`xdsg=false`、`safety_status=0`、`pf_status=0`，`ChargingStatus=0x0208`（`LV` + `RT`），文档化的 `OperationStatus[CHG]=1`；BQ25792 `vbat_present=false`、`vbat_adc_mv=1117`、`allow_charge=false`、`ichg=null`。因此动作按 `REQ-BQ40-LVP-002` 拒绝 `not_undervoltage`，这是负向 HIL 的预期结果，不等同成功路径验收。
- AFE Regs（`AFE FET Status` / `AFE FET Control`）在 TI TRM 中标注为 “intended for Texas Instruments' use”；项目按 bq29330 公开 AFE 表使用 `b1=DSG`、`b2=CHG` 的映射，`afe_chg_fet`/`afe_dsg_fet` 是 AFE 观测面，不是保护状态本身。`afe_chg_fet=false` 表示 AFE CHG 门极未置位，但不能单独当作 Safety/PF 保护结论；充电允许仍由 `OperationStatus[CHG]`、`XCHG`、`SafetyStatus`、`PFStatus`、`FET_EN`、`PRES`、`FET_OPTIONS`、温度域和 backfeed contradiction 共同裁决。按这些条件，当前 BQ40 的充电门控是放行的。
- 首次采样时设备曾停留在 `runtime.mode=blocked`、`issue_detail=pack_output_path_open`、12V 输出关闭。2026-11-21 裁决后，`charger.vbat_present=false` 不再单独禁充；`recovering_low_voltage` 在输入在线、无保护且高于 2500mV/cell 时允许自动 100mA。代码已跳过 `runtime.mode=blocked` 的通用 no-charge 覆盖，并仅在 backfeed contradiction 时禁充；第二次刷入后设备已进入 `runtime.mode=backup`、输出 active 且 100mA 充电。
- 2026-11-21 第二次刷入 HIL：`charger.policy=recovering_low_voltage`、`status=RECOV`、`target_ichg_ma=100`、`charger.allow_charge=true`、`charger.vbat_present=true`、`CHG_STAT=fast_cc`；BMS 实测电流 `93mA`、BQ25792 `ibat_adc_ma=60`，连续 60s 保持，cell_min 从 `2962mV` 升到 `2984mV`，pack 从 `12204mV` 升到 `12244mV`。同时 `output.requested=both`、`output.active=both`、`gate_reason=none`、OUT_A/B 约 `12.06V`。`afe_chg_fet=false` 与实测充电电流同时存在，说明 AFE CHG 位不是充电路径的充分判据。
- 自动 100mA 已产生实测电流，当前不需要新增显式 BQ40 reset；`afe_chg_fet` 继续只作为诊断字段。

## Remaining Gaps

- `VER-BQ40-LVP-008` 成功路径仍需要一个仍有 `CUVC/CUV` 的 pack；当前 pack 为 FD/RCA，不满足该动作入口。
- 若未来再次出现“100mA 目标但实测电流持续为 0”，再评估 ADR 0006 的显式 BQ40 reset 入口；本主题不把 CHG FET 强制打开，也不写 DF。
- 前端面板 HIL 的视觉确认尚未在真实设备上完成；代码路径与 host 测试已覆盖。


## Related Changes

- None

## References

- `./SPEC.md`
- `./HISTORY.md`
