# 历史记录（#xjpvj）

## 2026-07-03

- 修正 owner-facing mode 发布门槛：
  - 任何需要 TPS 输出成立的候选模式，都必须先满足 `requested_outputs` 全部进入
    `active_outputs`
  - 不满足时发布 `mode=blocked`，不得发布 `standby / supplement / backup`
- `BLOCKED` 固定为 non-charging owner-facing 阻断态，charger token 收敛到 `LOCK`。

## 2026-06-16

- 新建运行态模式切换 topic spec。
- 明确自动运行态只包含 `STANDBY / ASSIST / BACKUP`。
- 明确 `BACKUP` 只在确认无输入时进入。

## 2026-06-18

- `ASSIST` 收敛为内部 staged takeover：
  - `assist_low`
  - `assist_rated`
- owner-facing `mode` 改为跟随内部阶段，而不是直接跟随固定电流阈值。
- `advanced_power` 扩展为 11 字段，并落地 EEPROM `AdvancedPowerRecordV2`。
- `dcin` assist 资格与 `dcin pressure` 门控从 owner-facing `input.source` 解耦，允许并行 USB `5V` 遥测共存。

## 2026-06-23

- 修正运行态 TPS 目标电压更新路径：
  - 活动输出上的 runtime VOUT 微调不再走 `disable -> init -> enable`
  - 改为直接写 TPS `VOUT`
- live 设备回归确认：
  - 在线改 `standby_drop_mv` 时，`assist_target_vout_mv` 与 `out_a/out_b` 会立刻同步变化
- 这次修复把“软件阶段只微调输出电压，不重启 TPS 输出”的原则变成了当前 live truth。

## 2026-06-24

- Power Path Validation runner 收敛到当前 formal sign-off 路径：
  - `wait_for_load_state()` 复用 live poller 持有的 `loadlynx-devd` lease
  - `transition_backup / transition_restore` 进入稳定 phase 前必须拿到 fresh LoadLynx status
- 已确认旧假失败根因：
  - scene poller 与 fallback `loadlynx status` 抢同一 USB owner
  - 导致 `offline / stale / timeout` 假象
- 当前 formal sign-off 报告固定为：
  - `tools/hil/reports/20260624T150204Z-formal-12v-3900-corrected-rerun-r16-lanmonitor/results.json`
  - `run_validity=valid_for_signoff`
  - `signoff_valid=true`
  - `scene_complete=true`
- 这次收口把当前 topic 的真相源更新为：
  - runtime VOUT 微调已经 direct apply
  - current Power Path Validation tooling 也已经能稳定产出 owner-valid `12V` formal report

## 2026-06-24

- formal Power Path Validation 真相源再收敛一轮：
  - source-cut / backup 语义不再允许读取 `devd /api/v1/devices` cached listing
    作为 UPS runtime truth
  - formal runner 现在强制使用 direct UPS `status` + direct devd `diag-snapshot`
- formal acceptance 新增 source-cut 语义 gate：
  - `port_c_enabled=false` 后，UPS 侧必须观察到真实 cut 响应
  - `vin_vbus_mv` 也必须随 cut 变化
- 结果：
  - 之前的 `20260624T085538Z-formal-12v-3900-ipc-helper-r12` 报告在新合同下被重新定性为
    `invalid_diagnostic_only`
  - 原因是 UPS `mains_present` 与 `vin_vbus_mv` 在 source cut 后保持冻结
- formal rerun 进一步收口：
  - `20260624T150204Z-formal-12v-3900-corrected-rerun-r16-lanmonitor` 成为当前
    accepted sign-off report
  - freshness age 字段继续保留为诊断指标，但不再独立 veto 已满足完整采样与
    source-cut 合同的 formal run
  - runner 会在 formal preflight 前自动恢复 IsolaPurr baseline `12V / 3A`
    并重新启用 `port_c`

## 2026-06-25

- formal Power Path Validation tooling 升级到 dual-voltage suite：
  - 新增 `tools/hil/formal_hil_suite.py`
  - 新增 `tools/hil/render_formal_suite_html.py`
  - `tools/hil/verify_formal_suite.py` 升级为 profile-aware source window 校验
- formal suite 合同固定为四场景：
  - `12V assist_path`
  - `12V backup_only`
  - `19V assist_path`
  - `19V backup_only`
- suite 级 LoadLynx 保护栏固定为：
  - `min_v_mv=3000`
  - `max_i_ma_total=4000`
  - `max_p_mw=80000`
- `12V <-> 19V` artifact select / flash 的断电门禁被写成当前真相源：
  - disable load
  - cut IsolaPurr `port_c`
  - 确认 UPS 已脱离外部输入
  - 再进行 artifact switch / flash
- 当前状态明确收口为：
  - suite tooling 已完成本地 dry-run 与测试
  - 真正四场景真机执行仍等待 `main-vout-19v` artifact manifest 进入仓库 bundle

## 2026-06-29

- Rust `mains-aegis power-validation` runner 完成当前真实四场景签核：
  - `tools/hil/reports/formal-12v-19v-four-scenes-current-20260629T024800Z/suite-summary.json`
  - `tools/hil/reports/formal-12v-19v-four-scenes-current-20260629T024800Z/suite-overview.html`
  - `mains-aegis power-validation report --write-overview` 返回 `signoff_valid=true`
- 当前四场景均为 `valid_for_signoff`：
  - `12V assist_path`: `5.103Hz`, max gap `0.227s`
  - `12V backup_only`: `5.110Hz`, max gap `0.224s`
  - `19V assist_path`: `5.057Hz`, max gap `0.268s`
  - `19V backup_only`: `5.093Hz`, max gap `0.250s`
- 修正 Power Path Validation runner 的 source-cut 语义：
  - IsolaPurr 高压输出关闭使用 `power output auto`
  - `power config set --usb-c-path disconnected` 只断 USB-C path，不能证明香蕉口/TPS 高压输出已断
  - 断电确认必须以 UPS 侧 `source=dcin` / 高压 VIN 是否消失为准，USB-C 5V 管理链路不能被误判为 DCIN 未断
- 固件切换安全门槛进一步澄清：
  - `12V <-> 19V` artifact select / flash 只要求 `DCIN` 外部高压输入已切断
  - USB-C 到主机的供电/通信允许保留，不参与 UPS 直供输出路径，也不构成切换阻断
- 修正 scene 内采样完整性：
  - source-cut 门禁在 active scene 内使用 collector truth 判定
  - 禁止在 `transition_backup` 内同步执行长阻塞 status/power read 导致 `timeseries` 出现假 gap
- 修正 UPS 采样链路：
  - runtime BQ40 block detail 改为轻量 cached log，避免周期性阻塞 USB status/diag-snapshot feed
- 修复后 UPS `status` 与 `diag-snapshot` probe 达到 `3.003Hz`，max gap 约 `0.41s`，无 stale 样本
- 前面板冻结回归进一步收敛：
  - `service_web_serial_if_due()` 保留时，屏幕可继续正常运行
  - 固件主循环中的 unsolicited compact status push 已移除
  - 当前 USB 实测：
    - `status --watch --interval-ms 333 --samples 12 --include-meta`: `3.003Hz`，`12/12 sample_fresh=true`
    - `status --fresh --watch --interval-ms 333 --samples 8 --include-meta`: `3.177Hz`，`8/8 sample_fresh=true`
  - 当前 host truth 需区分：
    - `display_power_mode=sleeping` 时 `front_panel.frame_no` 可保持不变，这是 panel sleep 语义
    - `ready=false`、`init_state!=ready` 或 awake 态下长期无 frame advance 才可视为真实冻结嫌疑
