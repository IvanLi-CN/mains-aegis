# Owner-facing Charge Control（#q7mzc）

Status: active

## Summary

- 把 owner-facing 充电控制收敛成单一 `charge-control` 合同。
- `GET /api/v1/status` 继续只发布紧凑摘要；Power 页与手动充电弹窗使用 `GET /api/v1/charge-control` 及其 preview/action 响应作为权威真相源。
- 手动充电只通过 Power 页弹窗发起；Power 页主界面只显示当前态，不常驻“合同大表”或推导性解释。

## Scope

- 固件 LAN HTTP：`/api/v1/status`、`/api/v1/settings`、`/api/v1/charge-control`、`/api/v1/charge-control/preview`、`/api/v1/control/manual-charge`
- devd device HTTP：`/api/v1/devices/{id}/charge-control`、`/preview`、`/control/manual-charge`
- USB CDC / Web Serial：`get_charge_control`、`preview_charge_control`、`control_manual_charge`
- Web Power 页与手动充电弹窗

## Contract

### Status summary

`GET /api/v1/status` 的 `charge_control` 只保留：

- `mode`
- `manual_active`
- `takeover`
- `stop_inhibit`
- `last_stop_reason`
- `requested_power_path`
- `bound_power_path`
- `start_state`
- `output_power_w10`
- `power_telemetry_fresh`

### Charge-control detail

`GET /api/v1/charge-control`、`POST /api/v1/charge-control/preview` 与
`POST /api/v1/control/manual-charge` 都返回同形状 detail：

- `summary`
- `readiness`
- `telemetry`
- `evidence`

`readiness` 必须固定包含：

- `state=ready|blocked|confirm_required|running`
- `action=start|stop|confirm_loop|none`
- `planned_path { requested, bound, binding_reason }`
- `block { code, message } | null`
- `loop_override { required, active, allowed_guards[] }`

`telemetry` 必须固定包含 owner-facing 当前充电实况：

- `input_source`
- `policy_target_ichg_ma`
- `ibat_actual_ma`
- `target_voltage_mv`
- `iindpm_ma`
- `vindpm_mv`
- `output_power_w10`
- `power_telemetry_fresh`
- `input_limit_summary`
- `output_limit_summary`

`evidence[]` 只允许使用固件正式导出的直接事实，例如：

- `policy.state`
- `policy.full_reason`
- `charger.detail_status`
- `battery.issue_detail`
- `battery.charge_fet_on`
- `charger.vbat_present`
- `usb_pd.charge_ready`
- `output_power_w10`
- `power_telemetry_fresh`

Web 不得使用 SOC、`diag-snapshot`、transport timeout 或旧字段自行推断
“为什么不能充 / 为什么没就绪 / 当前跟着哪路 / 点 START 会发生什么”。

### Persistent prefs and capabilities

`GET /api/v1/settings` 继续承载：

- `manual_charge { target, speed, timer_h, power_path }`
- `charge_capabilities`

其中 `charge_capabilities` 至少包含：

- `target_voltage_mv`
- 正常/降档输入电流
- `dcin_input_limit_ma`
- USB-C PD 合格门
- 环路确认/停止阈值
- `supported_power_paths`
- `max_output_current_ma`

## Runtime rules

- `auto` 路径优先级固定为：高压高功率 PD USB-C > DCIN > 普通 USB-C。
- 显式 `dcin/usbc` 失败时不得静默回退。
- 绑定到 `dcin` 时，`START` 直接执行。
- 绑定到 `usbc` 时：
  - 输出关闭或 `<2W` 可直接启动。
  - `>=2W` 或功率未知时返回 `confirm_required`。
  - 确认后只允许绕过三类 USB 回环门：低功率启动门、遥测缺样锁存、高输出停充锁存。
- `battery_full` 必须由固件直接根据 `policy.full_reason` / `full_latched` / `termination_done` 等信号归类输出。

## Web UI

- Power 页主卡片只展示当前态：
  - 当前模式
  - 当前输入/绑定路径
  - 当前策略目标
  - `IBAT` 实测
  - 当前限流摘要
  - 当前环路避免状态
  - 剩余时间
  - 当前停止/阻断原因
  - 当前直接证据摘要
- 手动充电弹窗是唯一 owner-facing 控制面，负责：
  - defaults 编辑
  - preview
  - `START`
  - `STOP`
  - USB-C 环路确认
- `confirm_required` 必须在同一弹窗内完成，不再跳出第二个 owner-facing 弹窗。

## Supersession

- 本规格接管 `#zp4cg` 中 Web/API owner-facing 充电控制真相。
- `#zp4cg` 保留前面板与 EEPROM 历史，不再作为当前 Web Power 页合同来源。

## References

- `docs/specs/zp4cg-manual-charge-dashboard/SPEC.md`
- `docs/specs/p8k3d-mains-aegis-devd/SPEC.md`
- `docs/specs/ypfpu-web-management-ui/SPEC.md`
- `docs/specs/amc32-wifi-service-discovery-api-foundation/contracts/http-apis.md`
- `docs/usb-cdc-web-serial-protocol.md`
- `docs/web-management-ui.md`
