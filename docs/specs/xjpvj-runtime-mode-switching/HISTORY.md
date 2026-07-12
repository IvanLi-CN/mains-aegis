# 历史记录（#xjpvj）

## USB Backup controlled charge exception

- `BACKUP` 的 VIN/运行态定义保持不变。
- charger 默认仍为 `NOAC`；仅 `eu2b8` 的 USB-C PD 低输出守卫可在同一 `BACKUP` mode 中放行 `CHG500`。
- `>3W` 与两次 TPS 缺样锁存、真实 USB-C detach 新会话和手动会话豁免均属于 charger policy，不回写 mode state machine。

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

## 2026-07-03

- `BACKUP` 语义从“输入物理断电”扩展为“UPS 已接管负载”：
  - `backup_reason=input_absent`
  - `backup_reason=source_limited`
- `source_limited` 明确表示 `VIN` 在线但上级电源已不可承担当前负载，MCU 可主动切入 `BACKUP`，不必等待 `VIN < 3V`。
- `advanced_power` 从 11 字段扩展到 16 字段，并落地 EEPROM `AdvancedPowerRecordV3`：
  - `source_limited_vin_drop_pct`
  - `source_limited_enter_delta_ma`
  - `source_limited_exit_delta_ma`
  - `source_limited_required_samples`
  - `source_limited_recover_margin_mv`
- 旧 `12V assist_path / 3900mA` sign-off 结果保留为旧方案实测证据；该结果说明场景可完成，但也记录了 assist 阶段负载端约 `10.5V` 级长时间深跌落观察。
- `.mhtml` 被明确为可离线打开并保留内嵌图表的 evidence；单独 `.html` overview 若没有 scene 子目录会缺少 iframe 图表资源。

## Source-limited 12V validation contract

- `mains-aegis power-validation` 增加 `--suite-contract source-limited-12v`，与既有
  dual-voltage 四场景合同并存。
- 新合同固定三个独立 scene：普通负载 VIN cut、过载 VIN online、过载后 VIN cut。
- `source_limited_cut` 必须在最终 pre-cut hold sample 仍处于 `source_limited`，否则不得
  切断 source；报告将保留这一 failure 而不是把未验证的 cut 当成正常结果。
- scene telemetry 增加 `backup_reason`、charger state / allow-charge 和 source-limited
  电压持续时间指标；HTML chart 的阶段转移同时显示 backup reason 与 charger state。
- 已完成 dry-run、host tests、firmware host tests、12V release HIL build；真实 HIL
  已完成签核：`source-limited-12v-20260711T1818Z` 的三场景均为
  `signoff_valid=true`，且无 acceptance failure。

## 2026-07-11

- `12V / 3A source + 3900mA load` 的 source-limited 策略完成真机验证：
  - VIN 在线时，MCU 在 `source_limited` 下主动进入 `mode=backup`，而非继续等待
    输入物理断电。
  - `source_limited_online` 锁存后最低负载端电压为 `12139mV`，低于 `11000mV` 的
    最长时段为 `0s`。
  - `source_limited_cut` 在线锁存延迟为 `0.405s`，后续 cut 保持 backup 并转换为
    `backup_reason=input_absent`。
- `backup_only / 1000mA` 同时确认普通 VIN cut 仍遵循 `input_absent` 语义。
- 为避免 TPS 汇总输出电流遥测滞后延长跌落窗口：
  - 默认 source-limited drop 阈值调为 `1%`，进入增量调为 `1000mA`。
  - fresh TPS 样本维持完整保守判据；仅 TPS 样本未前进时允许使用
    `VIN baseline/drop + vin_iin_ma` 快速锁存。
- Power Path Validation runner 同步收敛：
  - IsolaPurr 香蕉口输出使用 `power runtime output --enabled true|false` 控制，
    不再把 `power output auto` 当作实际断源门。
  - 每场景开始前确认 UPS 已回到在线态，显式 stale UPS 样本不进入 formal timeseries。
  - source-limited 进入计时从 LoadLynx 实际 CC 负载生效开始，避免把 CLI 子进程启动时间
    误算成控制延迟。
- 原始报告位于 `tools/hil/reports/source-limited-12v-20260711T1818Z/`；可提交摘要位于
  `docs/specs/xjpvj-runtime-mode-switching/evidence/source-limited-12v-20260711T1818Z-suite-summary.json`。

## 2026-07-12

- 归档 `source-limited-12v-20260712T0300Z` 的完整三场景 HIL evidence：suite overview、
  各场景 `results.json`、`timeseries.jsonl` 与可交互 `voltage-chart.html` 均保留在
  `docs/specs/xjpvj-runtime-mode-switching/evidence/source-limited-12v-20260712T0300Z/`。
- 此归档用于后续输出稳定性优化的可复核比较，不取代已接受的
  `source-limited-12v-20260711T1818Z` sign-off。
- `backup_only` 与 `source_limited_cut` 满足 sign-off；`source_limited_online` 的
  source-limited 功能断言通过，但最大采样间隔为 `0.507s`，超过 `0.5s` 合同限制，
  因而标记为 `invalid_diagnostic_only`。
- 更换 IsolaPurr 上级电源后，`source-limited-12v-20260712T0759Z` 完整重测通过：三个
  scene 均为 `valid_for_signoff`，suite verifier 为 `signoff_valid=true`，无 acceptance
  failure。完整可离线复核 evidence 位于
  `docs/specs/xjpvj-runtime-mode-switching/evidence/source-limited-12v-20260712T0759Z/`。
- 两个 `3900mA` CC 过载场景分别在 `0.400s` 和 `0.406s` 锁存 `source_limited`，锁存后
  LoadLynx 最低电压分别为 `11743mV` 和 `11731mV`，均没有低于 `11000mV` 的持续段。
- IsolaPurr 保持 manual `12000mV / 3000mA`；`tps_cdc_rise_mv=300` 在测试前后保持不变。
- 独立 `source-limited-19v` 三场景完成真机签核：
  - `source-limited-19v-20260712T1020Z` 的 suite verifier 为 `signoff_valid=true`。
  - `3900mA` 在线限流场景分别在 `0.097s` 和 `0.203s` 锁存 `source_limited`；锁存后最低 LoadLynx 电压均为 `18732mV`，高于 `18000mV` 门槛。
  - VIN cut 场景持续保持 backup，并将原因切换为 `input_absent`。
  - 19V 实测 `vin_drop=168mV`、`vin_iin=2760mA`、`tps_total_iout=1368mA`，因此 source-limited VIN-drop 判据增加有界 `25mV` ADC/线损容差。
  - IsolaPurr 保持 manual `19000mV / 3000mA`，`tps_cdc_rise_mv=300` 在测试前后保持不变。
- 19V 普通 VIN cut 的 input-collapse 优化完成实机复测：
  - r5 保留为诊断 evidence：控制断言通过且 `tps_cdc_rise_mv=300` 保持不变，但 max gap
    `0.601s` 超过 formal 门槛，因此不得作为 sign-off。
  - r6 将已建立 VIN baseline 的严重输入崩落作为 `input_absent` 接管条件；正式结果为
    `4.952Hz`、max gap `0.401s`、无 acceptance failure，首个 backup 样本不再等待 VIN
    接近 2V。
  - r7 仅将 `standby_drop_mv` 从 `1200` 调为 `800`，并以 `4.967Hz`、max gap `0.401s`
    完成 sign-off。热备目标从 `17.8V` 升至 `18.2V`，负载端最低 `18.155V`，无低于
    `18.0V` 的采样点。
  - 最终 evidence 为
    `docs/specs/xjpvj-runtime-mode-switching/evidence/input-collapse-19v-backup-only-r7-20260712T1320Z/`；
    source 和 load 均由 runner 清理关闭，IsolaPurr `tps_cdc_rise_mv=300` 未被覆盖。
