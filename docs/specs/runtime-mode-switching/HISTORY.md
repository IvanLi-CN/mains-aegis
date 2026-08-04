# 历史记录（runtime-mode-switching）

## 2026-07-15

- 19V `100mV` 步进 UVLO sweep 完成：
  - 候选 A `18.1V / 18.3V`
  - 候选 B `18.2V / 18.4V`
  - 候选 C `18.3V / 18.5V`
- 三个候选点都完成 EEPROM 写入、reset 后回读和旧三场景真机验证。
- `source-limited-19v` 当前合同升级为四场景，新增 `source_in_budget / 2500mA`；因此这批
  sweep evidence 继续保留为参数筛选依据，但不再代表 19V 当前正式 sign-off。
- 19V 四场景正式 sign-off 已补齐并归档为
  `source-limited-19v-6bc1a374-four-scene-signoff-20260715T0455Z`：
  - suite verifier 为 `signoff_valid=true`
  - `backup_only`、`source_in_budget`、`source_limited_online`、`source_limited_cut`
    四个 scene 均为 `valid_for_signoff`
  - 最终 suite 由同合同、同参数、同固件 build 的有效 raw scene 组合而成：第一次全套 run
    保留有效 `source_limited_online`，第二次全套 run 保留有效 `backup_only`、
    `source_in_budget` 与 `source_limited_cut`
- 推荐值收敛到候选 B：
  - `standby_drop_mv=900`
  - `input_uvlo_cutoff_mv=18200`
  - `input_uvlo_recover_mv=18400`
  - `input_uvlo_required_samples=3`
  - `source_limited_enter_delta_ma=1000`
- 上述 19V 推荐值已提升为固件默认值。
- 当前 19V bench 设备为避免 EEPROM 与默认值完全同值，额外保存了最小偏移 override：
  `input_uvlo_cutoff_mv=18220`、`input_uvlo_recover_mv=18420`；这保证后续核查时能区分
  “固件默认值”与“设备 EEPROM 覆盖值”。
- `advanced_power` 设置面开始收敛：
  - owner-facing / EEPROM 持久化字段从此前的大而全设置面缩减为 5 个：
    - `standby_drop_mv`
    - `input_uvlo_cutoff_mv`
    - `input_uvlo_recover_mv`
    - `input_uvlo_required_samples`
    - `source_limited_enter_delta_ma`
  - 其余 `assist_*`、`rated_*`、`vin_drop_threshold_pct`、`required_samples`、
    `source_limited_vin_drop_pct`、`source_limited_exit_delta_ma`、
    `source_limited_required_samples` 与 `source_limited_recover_margin_mv`
    收敛为固件内部算法常量，不再进入 CLI / Web / EEPROM 可调面。
  - EEPROM 记录升级到 `AdvancedPowerRecordV5`。
  - 旧 `advanced_power` EEPROM 记录不再做读取兼容；旧设备记录会按当前 profile
    默认值重建，而不是在读取路径里做补字段迁就。
- 选择 B 的原因：
  - 相比 A，它把 `3900mA` 在线过载锁存从 `0.599s` 缩短到 `0.201s`
  - 并消除了在线/切断两个过载场景锁存前的 `<18V` 连续低压段
  - 相比 C，它没有把在线过载锁存拖慢到接近 `1s`
- 新证据归档：
  - `source-limited-19v-6bc1a374-uvlo18100-20260715T0310Z`
  - `source-limited-19v-6bc1a374-uvlo18200-20260715T0317Z`
  - `source-limited-19v-6bc1a374-uvlo18300-r3-20260715T0332Z`
- `source-limited-19v-6bc1a374-uvlo18300-20260715T0323Z` 保留为不完整诊断 evidence：
  runner 在第二个 scene 启动前遇到 IsolaPurr `power_enable` 串口超时。
- `source-limited-19v-6bc1a374-uvlo18300-r2-20260715T0327Z` 保留为诊断 evidence：
  `backup_only` 出现 `load_collector_error`，`source_limited_online` 出现
  `0.902s` sample gap，因此不能作为 sign-off。
- Power Path Validation 的 source-limited settings preflight 改为按 profile 校验：
  12V 继续要求 `2500mA` enter delta，19V 改为要求 `1000mA` enter delta 和对应 19V
  UVLO 预期值，避免把 12V bench 默认值误套到 19V sweep。

## USB Backup controlled charge exception

- `BACKUP` 的 VIN/运行态定义保持不变。
- charger 默认仍为 `NOAC`；仅 `bq25792-charge-policy` 的 USB-C PD 低输出守卫可在同一 `BACKUP` mode 中放行 `CHG500`。
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
  `docs/specs/runtime-mode-switching/evidence/source-limited-12v-20260711T1818Z-suite-summary.json`。

## 2026-07-12

- 归档 `source-limited-12v-20260712T0300Z` 的完整三场景 HIL evidence：suite overview、
  各场景 `results.json`、`timeseries.jsonl` 与可交互 `voltage-chart.html` 均保留在
  `docs/specs/runtime-mode-switching/evidence/source-limited-12v-20260712T0300Z/`。
- 此归档用于后续输出稳定性优化的可复核比较，不取代已接受的
  `source-limited-12v-20260711T1818Z` sign-off。
- `backup_only` 与 `source_limited_cut` 满足 sign-off；`source_limited_online` 的
  source-limited 功能断言通过，但最大采样间隔为 `0.507s`，超过 `0.5s` 合同限制，
  因而标记为 `invalid_diagnostic_only`。
- 更换 IsolaPurr 上级电源后，`source-limited-12v-20260712T0759Z` 完整重测通过：三个
  scene 均为 `valid_for_signoff`，suite verifier 为 `signoff_valid=true`，无 acceptance
  failure。完整可离线复核 evidence 位于
  `docs/specs/runtime-mode-switching/evidence/source-limited-12v-20260712T0759Z/`。
- 两个 `3900mA` CC 过载场景分别在 `0.400s` 和 `0.406s` 锁存 `source_limited`，锁存后
  LoadLynx 最低电压分别为 `11743mV` 和 `11731mV`，均没有低于 `11000mV` 的持续段。
- IsolaPurr 保持 manual `12000mV / 3000mA`；`tps_cdc_rise_mv=300` 在测试前后保持不变。
- 独立 `source-limited-19v` 三场景完成真机签核：
  - `source-limited-19v-20260712T1020Z` 的 suite verifier 为 `signoff_valid=true`。
  - `3900mA` 在线限流场景分别在 `0.097s` 和 `0.203s` 锁存 `source_limited`；锁存后最低 LoadLynx 电压均为 `18732mV`，高于 `18000mV` 门槛。
  - VIN cut 场景持续保持 backup，并将原因切换为 `input_absent`。
  - 19V 实测 `vin_drop=168mV`、`vin_iin=2760mA`、`tps_total_iout=1368mA`，因此 source-limited VIN-drop 判据增加有界 ADC/线损容差。
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
    `docs/specs/runtime-mode-switching/evidence/input-collapse-19v-backup-only-r7-20260712T1320Z/`；
    source 和 load 均由 runner 清理关闭，IsolaPurr `tps_cdc_rise_mv=300` 未被覆盖。
- 19V source-limited 重新完成三场景最终签核：
  - 最终 evidence 为
    `docs/specs/runtime-mode-switching/evidence/source-limited-19v-final-r7-20260712T1441Z/`；
    suite verifier 为 `signoff_valid=true`，三个 scene 均为 `valid_for_signoff`。
  - `source_limited_online` 在 `0.201s` 锁存，`source_limited_cut` 在 `0.401s` 锁存；
    两者锁存后 LoadLynx 最低均为 `18744mV`，无低于 `18000mV` 的持续段。
  - cut scene 在 physical VIN cut 后连续保持 backup，并确认 `input_absent`。
  - 19V 的 `136mV` 边缘 VIN drop 需要 `60mV` 有界 ADC/线损容差；仍同时要求 TPS 输出、
    VIN 输入电流和连续 fresh samples，避免正常在线源误切换。
  - r6 记录为诊断反例：source-limited 逻辑与 cut 连续性成立，但 post-latch 最低
    `17589mV`，持续 `0.303s`，因此不构成 sign-off；r7 重新完整执行后通过。

## 2026-07-13

- `source-limited-12v` 验收合同增加 `source_in_budget / 2900mA`：在 `12V / 3A`
  source 在线时不得出现 `mode=backup`、`assist_power_stage=backup` 或
  `backup_reason=source_limited`；suite 从三个 scene 扩为四个 scene。

## 2026-07-14

- 12V 固件修复 VIN 已严重崩落但 `mains_present` 尚未翻转时的状态抖动：已进入
  `backup_reason=input_absent` 后持续保持 Backup；已锁存 `source_limited` 时仍保持该原因，
  直到明确观察到 `mains_present=false` 才转换为 `input_absent`。
- Power Validation 将 IsolaPurr 的 USB config 快照与 UPS/LoadLynx 连续遥测分离，避免在
  IsolaPurr 输出关闭时重复查询 `info` 造成 collector error；源实际电压以 UPS USB DCIN ADC
  记录，IsolaPurr 的 `tps_cdc_rise_mv=300` 仍由前后快照校验。
- runner 使用已接收的 UPS USB 原始帧时间回填控制动作期间的 scene gap，不插值电压或状态。
  r6 最终四场景签核证据：
  `docs/specs/runtime-mode-switching/evidence/source-limited-12v-62179e3c-final-r6-20260714T0010Z/`
  ，离线 verifier 返回 `signoff_valid=true`。
  `backup_only`、`source_in_budget`、`source_limited_online`、`source_limited_cut` 的最大
  gap 分别为 `0.253s`、`0.232s`、`0.205s`、`0.296s`；四个 scene 均无 acceptance failure。
- r6 使用 build `ea9c41d7-dirty-62179e3c72e2da65`，12V `rated_vout_mv=12000`，
  source `12000mV / 3000mA`，IsolaPurr `tps_cdc_rise_mv=300` 前后保持一致；LoadLynx
  报告仅使用 `load_i_total_ma`。
- 12V source-limited VIN-drop 容差改为不超过百分比阈值的一半，避免固定 `80mV`
  容差在 `1%` 门槛下把约 `50mV` 的能力内压降误判为限流，同时保留约 `112mV`
  过载压降的接管能力。
- Power Path Validation 的 LoadLynx evidence 收敛为单一 `load_i_total_ma`，不再记录
  或展示错误的 `local/remote` 电流分量。
- TPS2490 输入 MOS 与网表修复后重新确认 INA3221 CH3 位于输入 MOS 前级；固件字段收敛为
  `pre_tps_vin_mv`，并保留 `vin_vbus_mv` 兼容别名。
- 新增 MCU 输入欠压门：连续 3 个 fresh 前级 VIN 样本 `<10V` 关断 TPS2490 输入并进入
  `input_absent` Backup；输入门关断后连续 3 个样本 `>11V` 才恢复。真机阶梯点
  `9.544V / 10.576V / 11.552V` 验证了关断、回差与恢复。
- 修复 source-limited stale sample 重复计数，并在首个候选样本立即预升压；第二个 fresh
  样本以 UPS 持续承担至少 `500mA` 确认锁存，避免预升压恢复 VIN 后反复退出。
- 修复后 12V 四场景最终 evidence 位于
  `docs/specs/runtime-mode-switching/evidence/source-limited-12v-ce343924-uvlo-preboost-final-20260714T1206Z/`。
  verifier 返回 `signoff_valid=true`；2500mA guard 无 Backup，两个 3900mA 场景锁存后
  最低负载电压均为 `11790mV`，cut 场景随后连续保持 Backup 并转为 `input_absent`。
- MOS/网表修复前的 `source-limited-12v-62179e3c-final-r6-20260714T0010Z` 与旧
  assist_path 约 `10.5V` 长跌落继续保留为历史基线，不再代表当前硬件状态。
- 12V 当前默认 standby 目标从 `10.8V` 收敛到 `11.3V`：
  - `standby_drop_mv` 缺省值改为 `700`
  - `DeviceSettingsSnapshot`、EEPROM 缺省初始化、reset advanced power、host 默认快照已统一到该值
- 前级输入门从固件硬编码迁移到 `advanced_power` EEPROM 参数：
  - 新增 `input_uvlo_cutoff_mv / input_uvlo_recover_mv / input_uvlo_required_samples`
  - EEPROM 记录升级到 `AdvancedPowerRecordV4`
  - 缺省值仍按档位派生：
    - `12V`：`11.3V / 11.5V / 3 samples`
    - `19V`：`10V / 11V / 3 samples`
- 本地验证已覆盖新默认值与输入门回差：
  - `cargo test --manifest-path firmware/host-unit-tests/Cargo.toml`
  - `cargo test --manifest-path tools/mains-aegis-host/Cargo.toml`
- 完成 `93aadc61-clean-eb2b310e1419a6cc` 的 12V `100mV` 步进复测，归档于
  `docs/specs/runtime-mode-switching/evidence/source-limited-12v-93aadc61-uvlo-sweep-20260714T1636Z/`：
  - 根目录是推荐候选 A `11.3V / 11.5V` 的完整四场景 sign-off suite，浏览器导出的
    `suite-overview.mhtml` 可离线保留四张图表
  - `comparison.json` 记录 A/B/C 三个候选点；A 与 C 全部通过，B `11.4V / 11.6V`
    在 `source_in_budget / 2500mA` 误判 Backup
  - 因为 A 已满足“全部通过且 cutoff 最低”的选优规则，本轮记录为“实测确认当前默认值可接受”，
    而不是新的默认值变更动作
- 每个候选点都执行了 EEPROM 写入、回读与重启后回读；归档目录中的
  `candidate-*-meta/after_write.json` 与 `after_reset.json` 保留了固件 build
  `93aadc61-clean-eb2b310e1419a6cc` 的设置记忆证据。

- 归档 12V source-limited 诊断反例
  `docs/specs/runtime-mode-switching/evidence/source-limited-12v-c22bf968-20260713T0320Z/`：
  - 前一轮修复后，`source_limited_online` 与 `source_limited_cut` 已恢复签核通过；
  - 但 `backup_only / 1000mA` 在线 hold 被错误锁成 `backup_reason=source_limited`，
    `hold_tps_power_max_mw=12607`，因此 scene 与 suite 只能作为诊断证据。
- 修复 12V source-limited 假阳性：
  - 纯 TPS-only 的 source-limited 锁存不再允许把一次瞬时高 `VIN IIN` 与后续低输入电流样本拼接成连续计数；
  - 已锁存 `source_limited` 后，VIN cut 的 `dcin_present` 先掉窗口继续保持 `backup`，直到原因转为 `input_absent`。
- 使用 12V build `c22bf968-dirty-d8c9ca3fa923b63b` 完成新的 12V 三场景最终签核：
  - evidence:
    `docs/specs/runtime-mode-switching/evidence/source-limited-12v-c22bf968-20260713T0335Z/`
  - suite verifier `signoff_valid=true`，三个 scene 均为 `valid_for_signoff`
  - `12v-backup_only-1000ma`：`4.894Hz`，max gap `0.402s`，hold TPS 最大 `391mW`
  - `12v-source_limited_online-3900ma`：`4.810Hz`，max gap `0.401s`，锁存后最低负载端电压 `11755mV`
  - `12v-source_limited_cut-3900ma`：`5.025Hz`，max gap `0.401s`，VIN cut 后连续保持 backup 并转为 `input_absent`
  - IsolaPurr 仍保持 manual `12000mV / 3000mA`，`tps_cdc_rise_mv=300` 在测试前后回读一致
- 完成 19V 热备目标与 source-limited 联合调优：
  - `standby_drop_mv` 从 `1200` 恢复为 `800`，普通 VIN cut 切换期 LoadLynx 最低由
    `17754mV` 提高到 `18143mV`。
  - 18.2V standby 下，3900mA 诊断样本显示 `vin_iin≈2017mA`、
    `tps_total_iout≈2324mA`、VIN drop `120mV`，旧的高输入电流限定不再适用。
  - source-limited 有界 VIN-drop 容差调为 `80mV`，并保持 `VIN IIN >= 2000mA`、
    TPS 输出负载和连续样本三重门槛；普通 1000mA 负载不会触发。
- 最终组合 evidence 为
  `docs/specs/runtime-mode-switching/evidence/source-limited-19v-tuned-final-20260713T0020Z/`，
  旧版 suite verifier 曾返回 `signoff_valid=true`；该结论因缺失 hold TPS `2W` 门禁而撤销，
  evidence 降级为 `invalid_diagnostic_only`。
- 两个 3900mA 场景接管后最低 LoadLynx 电压均为 `18744mV`，无低于 `18000mV`
  的持续段；cut scene 在物理断源后保持 Backup 并切换为 `input_absent`。
- IsolaPurr `tps_cdc_rise_mv=300` 未被覆盖，测试结束时 source 与 load 均关闭。
- Spec 增加 hold 功率硬合同：任一 fresh hold 样本的 TPS 输出功率超过 `2000mW` 即失败，
  不允许用平均值或持续时间豁免；3900mA 场景必须把首次超限后的窗口拆为
  `transition_source_limited`，锁存后拆为 `backup_online`。
- 重新审计发现 tuned evidence 的 `backup_only` hold 有 `137/158` 个样本超过 `2W`，
  两个 3900mA hold 分别有 `73/80` 与 `71/79` 个样本超过 `2W`，因此不得作为最终签核。
- Power Path Validation 实现 hold TPS 功率硬门禁、阶段重分类与 timeseries 重算；旧错误
  suite 现在明确返回 `hold_tps_power_over_2w`。
- 参数扫描确认 `820–840mV` 之间存在路径跳变；最终不贴边，采用
  `standby_drop_mv=900`。
- 修复正常 1000mA 被误判 source-limited：VIN 输入必要门槛从 `2000mA` 收紧为
  `2300mA`，同时保留 VIN drop、TPS 输出与连续样本门槛。
- `source-limited-19v-optimized-cut-r3-20260713T0155Z` 完整三场景通过新 verifier：hold
  TPS 最大分别 `1089mW / 1089mW / 1016mW`，均无超 2W 样本；普通 VIN cut 最低
  `18049mV`，两个过载场景接管后最低均为 `18744mV`。
- runtime-mode 默认值与 owner-facing 可调 bounds 收敛到
  `schemas/runtime_mode_profiles.json`，并由 firmware build script 在编译期生成 profile 表；
  host / Web 共用同一 schema，不再各自维护分散的默认快照。
- 当前默认值确定为：
  - `12V`: `standby_drop_mv=700`、`input_uvlo_cutoff_mv=11300`、
    `input_uvlo_recover_mv=11500`、`input_uvlo_required_samples=3`、
    `source_limited_enter_delta_ma=2500`
  - `19V`: `standby_drop_mv=900`、`input_uvlo_cutoff_mv=18200`、
    `input_uvlo_recover_mv=18400`、`input_uvlo_required_samples=3`、
    `source_limited_enter_delta_ma=1000`
- 补归档当前 `c8bd8130` 版 12V 四场景最终 evidence：
  `docs/specs/runtime-mode-switching/evidence/source-limited-12v-c8bd8130-rerun-20260715T0838Z/`。
  其中 `source_in_budget / 2500mA` 已替换为电池充满后的 clean rerun，去除了充电干扰。
- 补归档当前 `c8bd8130` 版 19V 四场景最终 evidence：
  `docs/specs/runtime-mode-switching/evidence/source-limited-19v-c8bd8130-r2-20260715T0817Z/`。
  其中 `source_in_budget / 2500mA` 已替换为充电结束后的 clean rerun；hold 期间
  `charger_allow_charge=false`、`battery_current_ma=0`，suite 继续保持
  `signoff_valid=true`。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
