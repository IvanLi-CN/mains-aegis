# 实现记录（#xjpvj）

## 当前实现真相

当前主线实现已经收敛到以下事实：

- `VIN` 是运行态输入在线/离线的主真相源
- `mains_present=None` 时保持上一确认模式
- `BACKUP` 表示 UPS 已接管负载，进入原因由 `backup_reason` 区分：
  - `input_absent`
  - `source_limited`
- `input_absent` 在前级 VIN 连续欠压并由 MCU 关断 TPS2490 输入门后进入；不要求连接器电压物理归零
- `source_limited` 允许在 `VIN` 在线但上级电源限流/棕断时由 MCU 主动进入
- owner-facing `mode` 跟随内部阶段映射：
  - `standby -> standby`
  - `assist_low | assist_rated -> supplement`
  - `backup -> backup`
- owner-facing `mode` 发布前必须经过 TPS 输出活跃门槛：
  - 若候选 mode 为 `standby / supplement / backup`
  - 且 `requested_outputs` 中存在未进入 `active_outputs` 的通道
  - 则 API / diag 发布 `mode=blocked`
  - front-panel 保持或退回自检/阻断界面，不渲染 Dashboard
- staged assist 已经落地：
  - `standby` 使用低于额定输出的热备目标
  - `assist_low` 通过运行时双判据进入，并按 `assist_ramp_step_mv / assist_ramp_interval_ms` 限速爬升
  - `assist_rated` 与 `backup` 使用额定输出目标
- `ASSIST` 收敛到 non-charging mode；`BACKUP` 默认同样停充，但把 VIN 已确认无市电时的受控 USB-C 低输出充电例外委托给 `eu2b8`
- `BLOCKED` 也按 non-charging mode 处理，且不是 Dashboard 可渲染模式
- `backup_reason=input_absent` 的 charger token 对齐 `NOAC`
- `backup_reason=source_limited` 的 charger token 对齐 `LOAD`，并使用 `runtime_source_limited_backup_no_charge`

## 运行时调压实现

当前最重要的实现收敛是：

- `assist_target_vout_mv` 变化时，活动 TPS 不再走 `disable -> init -> enable`
- 运行时微调现已改成原位直写 TPS `VOUT`
- full configure 只保留给首次 bring-up、显式恢复、或 retry 恢复路径

当前代码分层：

- `firmware/src/output/pure.rs`
  - 负责 runtime mode / stage / target 纯逻辑
- `firmware/src/output/mod.rs`
  - 负责把当前运行态目标同步到活动 TPS
- `firmware/src/output/tps55288.rs`
  - 提供直接 `VOUT` 更新能力

这条实现约束对应的当前规范真相是：

- `standby / assist_low / assist_rated / backup` 这些软件阶段只负责微调目标电压
- 软件阶段不得把一次运行态改压升级成 TPS 重新初始化

## 设置与持久化实现

当前 `advanced_power` 契约已经是 19 字段：

- `standby_drop_mv`
- `assist_low_drop_mv`
- `assist_enter_delta_ma`
- `assist_exit_delta_ma`
- `assist_required_samples`
- `assist_ramp_step_mv`
- `assist_ramp_interval_ms`
- `rated_enter_delta_ma`
- `rated_exit_delta_ma`
- `vin_drop_threshold_pct`
- `required_samples`
- `input_uvlo_cutoff_mv`
- `input_uvlo_recover_mv`
- `input_uvlo_required_samples`
- `source_limited_vin_drop_pct`
- `source_limited_enter_delta_ma`
- `source_limited_exit_delta_ma`
- `source_limited_required_samples`
- `source_limited_recover_margin_mv`

实现状态：

- owner-facing 保存语义仍然是相对值或无量纲值
- EEPROM 使用 `AdvancedPowerRecordV4`
- 继续兼容旧 `V1 / V2` 记录的默认值补齐读取
- `status / diag-snapshot` 已暴露：
  - `assist_power_stage`
  - `assist_target_vout_mv`
  - `backup_reason`
- 当前缺省值按额定输出档位派生：
  - `12V`：`standby_drop_mv=700`，即 `11.3V standby target`
  - `19V`：`standby_drop_mv=1200`，即 `17.8V standby target`
- 当前前级输入门改为 EEPROM 持久化参数：
  - `input_uvlo_cutoff_mv`
  - `input_uvlo_recover_mv`
  - `input_uvlo_required_samples`
  - 缺省值仍按额定输出档位派生：
    - `12V`：`11.3V / 11.5V / 3 samples`
    - `19V`：`10V / 11V / 3 samples`

新增 source-limited 默认值优先保证检测延迟可控：

- `source_limited_vin_drop_pct=1`
- `source_limited_enter_delta_ma=2500`
- `source_limited_exit_delta_ma=0`
- `source_limited_required_samples=2`
- `source_limited_recover_margin_mv=400`

因此默认 source-limited 进入电流门槛为 `rated_enter_base 100mA + 2500mA = 2600mA`。fresh TPS 样本仍走完整 `VIN drop + TPS output + input current` 判据；只有 TPS 聚合输出样本滞后时，才允许基于 `VIN baseline/drop + vin_iin_ma` 快速锁存，避免上级限流时等待滞后遥测而延长负载端跌落。该门槛在 12V/3A 台架上为 `2900mA` in-budget 场景保留 guard band，同时仍能在 `3900mA` 负载下观测到足够的 TPS 补充电流后锁存。

当前规范还新增了一条负向 guard：

- 当 formal/source-limited 台架 source 固定为 `rated_vout_mv / 3000mA` 时，在线 `2900mA` 负载不得误锁存 `backup_reason=source_limited`。
- 这不是新的正向 source-limited 场景，而是“不得误入 BACKUP”的回归门。
- 现有 `3900mA` 三场景 sign-off 只能证明过载接管成立，不能替代这条 `2900mA` 非接管约束。

## 当前验证状态

### Host / build gates

当前已通过：

- `cargo test --manifest-path firmware/host-unit-tests/Cargo.toml`
- `cargo test --manifest-path firmware/host-unit-tests/Cargo.toml output_tps55288 -- --nocapture`
- `cargo +esp build --manifest-path firmware/Cargo.toml --bin esp-firmware --release --target xtensa-esp32s3-none-elf -Zbuild-std=core,alloc`

这些 gate 当前覆盖的核心事实：

- `assist_low` 不再由 owner-facing `supplement` 或非 `dcin` 输入自动推进
- `assist_low` 需要运行时双判据与 fresh-sample 锁存
- `assist_low` 的限速爬升与回差退出存在
- `VIN drop + TPS iout` 双判据才会进入 `assist_rated`
- 输入恢复时会带回差地从 `assist_rated` 降回 `assist_low`
- 运行时改压只会触发 `REF0/REF1` 写入，不会读写 `MODE/OE/ILIM`

### Live runtime-VOUT proof

当前 live 设备上已经两次复跑证明“运行时改压直接生效”：

- 可复跑脚本：
  - `tools/hil/verify_runtime_vout_live.py`
- 当前已确认事实：
  - 在线改 `standby_drop_mv` 后，`assist_target_vout_mv` 会立刻变化
  - `out_a / out_b` 实测电压会同步变化
  - 写回默认值后也会同步恢复

因此当前 live truth 已经足够支持：

- 运行态电压微调是 direct apply
- 不是重新执行一次 TPS bring-up

### Current formal Power Path Validation sign-off

当前有效 formal sign-off suite：

- `tools/hil/reports/formal-12v-19v-four-scenes-current-20260629T024800Z/suite-summary.json`
- `tools/hil/reports/formal-12v-19v-four-scenes-current-20260629T024800Z/suite-overview.html`
- `tools/hil/reports/composed-current-four-scenes/suite-summary.json`
- `tools/hil/reports/composed-current-four-scenes/suite-overview.html`
- `tools/hil/reports/formal-12v-19v-four-scenes-cli-r4/suite-summary.json`
- `tools/hil/reports/formal-12v-19v-four-scenes-cli-r4/suite-overview.html`
- `tools/hil/reports/power-validation-rust-four-scenes-url-r7/suite-summary.json`
- `tools/hil/reports/power-validation-rust-four-scenes-url-r7/suite-overview.html`

可提交 evidence 副本：

- `docs/specs/xjpvj-runtime-mode-switching/evidence/formal-12v-19v-four-scenes-current-20260629T024800Z-suite-summary.json`
- `docs/specs/xjpvj-runtime-mode-switching/evidence/formal-12v-19v-four-scenes-current-20260629T024800Z-suite-overview.html`
- `docs/specs/xjpvj-runtime-mode-switching/evidence/formal-12v-19v-four-scenes-current-20260629T024800Z-suite-overview.mhtml`
- `docs/specs/xjpvj-runtime-mode-switching/evidence/formal-12v-19v-four-scenes-cli-r4-suite-summary.json`
- `docs/specs/xjpvj-runtime-mode-switching/evidence/formal-12v-19v-four-scenes-cli-r4-suite-overview.html`
- `docs/specs/xjpvj-runtime-mode-switching/evidence/power-validation-rust-four-scenes-url-r7-suite-summary.json`
- `docs/specs/xjpvj-runtime-mode-switching/evidence/power-validation-rust-four-scenes-url-r7-suite-overview.html`

其中 `.mhtml` 是从已验证浏览器页面导出的完整页面快照，保留四个
`voltage-chart.html?embed=1` 内嵌图表；旁边的 `.html` 只保留 suite overview
结构，若未同时保留 scene 子目录，单独打开会缺少图表 iframe 资源。

当前 accepted metrics：

- `12V assist_path`
  - `run_validity=valid_for_signoff`
  - `signoff_valid=true`
  - `target_ma=3900`
  - `effective_sample_rate_hz=5.103`
  - `max_sample_gap_s=0.227`
- `12V backup_only`
  - `run_validity=valid_for_signoff`
  - `signoff_valid=true`
  - `target_ma=1000`
  - `effective_sample_rate_hz=5.110`
  - `max_sample_gap_s=0.224`
- `19V assist_path`
  - `run_validity=valid_for_signoff`
  - `signoff_valid=true`
  - `target_ma=3900`
  - `effective_sample_rate_hz=5.057`
  - `max_sample_gap_s=0.268`
- `19V backup_only`
  - `run_validity=valid_for_signoff`
  - `signoff_valid=true`
  - `target_ma=1000`
  - `effective_sample_rate_hz=5.093`
  - `max_sample_gap_s=0.250`

当前 passing report 使用的 `advanced_power` 快照：

- `standby_drop_mv=1200`
- `assist_low_drop_mv=600`
- `assist_enter_delta_ma=0`
- `assist_exit_delta_ma=0`
- `assist_required_samples=2`
- `assist_ramp_step_mv=100`
- `assist_ramp_interval_ms=200`
- `rated_enter_delta_ma=0`
- `rated_exit_delta_ma=0`
- `vin_drop_threshold_pct=4`
- `required_samples=2`

这些旧方案报告没有 `source_limited_*` 字段，因为当时 `advanced_power`
仍是 11 字段契约；读取旧 EEPROM/旧 settings 时当前实现会用默认
source-limited 字段补齐。

12V 的 `11.3V standby + 11.3V / 11.5V input UVLO` 已在后续 `93aadc61`
真机四场景复测中完成重新签核；当前默认值不再只是实现假设，而是有对应的
12V sign-off evidence 支撑。`100mV` 步进搜索还额外保留了 `11.4V / 11.6V`
与 `11.5V / 11.7V` 的同台架对照结果，用来说明“更高 cutoff 并不必然更优”。

### 旧方案 12V assist_path 3900mA 观察

`12V assist_path / 3900mA` 是有效 sign-off scene，但它证明的是旧方案
“assist 兜底可维持场景完成”，不是“assist 对超电源能力负载电压稳定性最优”。

从当前 evidence 页面与截图观察可见：

- 在 `12V / 3A source + 3900mA load` 的 hold/assist 阶段，LoadLynx 负载端电压曾长时间低于额定 12V，约落在 `10.5V` 级别。
- 该跌落发生在 `VIN` 仍在线、上级电源接近能力边界时。
- 这与硬件 assist 只作为 MCU 介入前兜底的定位一致：若硬件路径不能真正混合供电，只靠 MOS 体二极管或未主动导通路径补能，可能需要约 `0.7V` 级压差才开始勉强共同输出。
- 本轮优化因此不再把该场景停留在 `assist_rated` 作为最终策略，而是允许 MCU 在识别 `source_limited` 后直接进入 `backup`，把 TPS 目标切回额定输出。

## 当前 Power Path Validation 工具链真相

这次 `valid_for_signoff` 并不是新的 UPS 控制逻辑变化带来的，而是 runner/tooling 收敛带来的：

- `transition_backup / transition_restore` 进入稳定 phase 前，runner 会强制等一帧新的 LoadLynx status
- `wait_for_load_state()` 会复用 live `LoadStatusPoller` 已持有的 `loadlynx-devd` lease
- 不再回退到第二条裸 `loadlynx status` 路径去和 poller 竞争同一 USB owner

已经确认的旧假失败根因：

- live poller 持有设备 lease
- scene-local fallback 又起第二条 released `loadlynx status`
- 两者竞争同一设备会话
- 结果出现 `offline / stale / timeout` 假象
- formal 报告因此被误判成 `invalid_diagnostic_only`

当前真相因此更新为：

- 运行时调压已经 direct apply
- historical Python-era Power Path Validation tooling 已经在同一 bench 上产出
  `12V/19V` 四场景 owner-valid formal suite
- 当前 owner-facing runner 已升级为 Rust `mains-aegis power-validation`
- Rust runner 已完成本地实现、adapter 协议、dry-run 校验与四场景真机签核
- formal scene 的 UPS runtime 真相必须来自 direct UPS `status`
- `devd /api/v1/devices` listing 只允许用于发现/seed，不允许再作为
  source-cut 语义的 primary truth surface
- backup scene 现在还新增了 source-cut 语义 gate：
  - `port_c_enabled=false` 后，UPS 侧至少要观察到
    `mains_present=false`、`mode=backup`、`assist_power_stage=backup` 三者之一
  - 同时 `vin_vbus_mv` 必须随 cut 发生变化
- UPS identity / settings 必须在 profile 切换和 flash 后 fresh 读取：
  - CDC/native serial `device_identity` 必须发起新的 `get_identity`
  - CDC/native serial `device_settings` 必须发起新的 `get_settings`
  - flash 成功后必须清掉 stale identity/settings/status/diag-snapshot runtime cache
- 固件 artifact 的 feature metadata 必须跟实际 feature set 同步：
  - `firmware/build.rs` 必须对 `CARGO_FEATURE_*` 变化声明
    `cargo:rerun-if-env-changed`
  - 否则可能出现控制逻辑是 `12V`、但 identity 仍报告
    `main-vout-19v` 的危险不一致
- IsolaPurr manual source 配置后必须显式保持输出关闭：
  - 写入 manual voltage/current 不能被视为 source-off 证明
  - runner 必须在配置后再次执行 source-off，并在 `port_c` 仍关闭时读回
    source 配置
- IsolaPurr 高压输出关闭必须使用 `power output auto`：
  - `power config set --usb-c-path disconnected` 只断 USB-C path
  - 它不能证明香蕉口/TPS 高压输出已断开
  - source-cut 通过与否必须看 UPS 侧 `source=dcin` / 高压 VIN 是否消失
- scene 内 source-cut 门禁必须使用已有 collector truth：
  - 不允许在 `transition_backup` 内同步执行长阻塞 read 并造成 timeseries gap
  - `timeseries` 的正式 max gap 仍以 `<=0.5s` 为硬标准

## 当前 dual-voltage suite tooling 真相

当前 formal Power Path Validation 已经不再只停留在单一 `12V` scene runner。
当前 owner-facing 路径是 Rust host command：

- `mains-aegis power-validation run`
- `mains-aegis power-validation report --write-overview <suite-dir>`
- `mains-aegis power-validation adapter-protocol`
- `just power-validation ...`

Python `tools/hil/formal_hil_cli_suite.py`、`verify_formal_suite.py`、
`render_formal_suite_html.py` 只保留为迁移参考或辅助脚本，不再是
owner-facing suite entry。

当前 suite 级 scene 合同固定为：

- `12V assist_path`
  - source=`12000mV / 3000mA`
  - load=`3900mA`
- `12V backup_only`
  - source=`12000mV / 3000mA`
  - load=`1000mA`
- `19V assist_path`
  - source=`19000mV / 3000mA`
  - load=`3900mA`
- `19V backup_only`
  - source=`19000mV / 3000mA`
  - load=`1000mA`

所有 suite scene 的 LoadLynx 保护栏固定为：

- `min_v_mv=3000`
- `max_i_ma_total=4000`
- `max_p_mw=80000`

当前 suite 级硬门禁也已经固定：

- 任何 `12V <-> 19V` artifact select / flash 之前，必须先：
  - disable load
  - cut IsolaPurr `port_c`
  - 确认 UPS 已脱离外部输入
  - 再进行 artifact switch / flash

当前真实执行边界：

- historical suite tooling 已完成真实四场景采集并产出 `valid_for_signoff` raw scene 证据
- Rust `power-validation compose` 已将这组 raw scene 目录组合成当前正式 suite：
  - `tools/hil/reports/composed-current-four-scenes/suite-summary.json`
  - `tools/hil/reports/composed-current-four-scenes/suite-overview.html`
  - compose 输出的 suite 保留指向原始 scene 目录的相对链接，不复制、不修改、不合成 raw samples
  - `mains-aegis power-validation compose` 会在写入 suite summary/overview 后立即执行 Rust sign-off verifier
- 当前 owner 可见四场景报告源：
  - `tools/hil/reports/formal-12v-19v-four-scenes-current-20260629T024800Z/suite-summary.json`
  - `tools/hil/reports/formal-12v-19v-four-scenes-current-20260629T024800Z/suite-overview.html`
  - `mains-aegis power-validation report --write-overview` accepts this suite
    and regenerates the overview from the verified suite summary
  - transport summary:
    `UPS=cli+ipc+usb`, `LoadLynx=cli+ipc+usb`, `IsolaPurr=cli+ipc+usb`
  - all four scenes are `valid_for_signoff`
- 因此当前状态是：
  - `historical four-scene evidence = accepted`
  - `Rust compose/report implementation = validated`
  - `Rust-composed four-scene evidence = accepted`
  - `19V scene tooling = accepted`
  - `dual-voltage four-scene real run = accepted`

## 当前已知边界

当前通过版 formal scene 说明：

- 当前 accepted report 已经满足完整 scene 的采样、记录、图表链条与 source-cut
  语义合同
- 当前 formal pass 的主真相源是 `results.json` 里的 `run_validity=valid_for_signoff`
- suite 级 formal pass 必须由 `power-validation report --write-overview`
  追溯验证四个 scene 的 `results.json`、`timeseries.jsonl`、required voltage
  series、采样率、最大 gap 与 chart 文件后成立
- HTML chart 仍然只是证据展示层，不是 formal pass 的 primary truth source

当前仍应明确保留的边界：

- 当前 formal sign-off 场景中的 `assist_target_vout_mv` 维持在 `10800mV`
- 当前 UPS INA `VOUT` 的最小观测值出现在 `hold / assist_low`
  - `ups_vout_mv=10824mV`
  - phase=`hold`
  - time=`14.751s`
- 当前 `transition_backup` 的 UPS INA `VOUT` 最低值仍保持在 `11856mV`
- 这说明当前 formal run 下最明显的深跌落仍主要体现在负载端测得电压，
  而不是 UPS INA `VOUT` 本身掉到同样水平

## 当前后续方向

## Source-limited 12V validation implementation

`mains-aegis power-validation` now has an explicit
`--suite-contract source-limited-12v` contract. It keeps the existing
dual-voltage four-scene contract unchanged and creates only these independent
12V reports:

- `backup_only / 1000mA`
- `source_in_budget / 2900mA`
- `source_limited_online / 3900mA`
- `source_limited_cut / 3900mA`

`backup_only` 使用 LoadLynx `CC 1000mA`；`source_in_budget` 使用 `CC 2900mA`，证明
`12000mV / 3000mA` 上级电源能力内的负载不会误入 Backup；两个 source-limited 场景使用
LoadLynx `CC 3900mA`。这是刻意施加超过上级 `12000mV / 3000mA` 能力的真实负载，
用于验证 UPS 是否主动进入 backup 并由电池补足缺口，不能替换为改变负载需求的 CV
刺激。`4000mA` 是电子负载保护上限，不是 source-limited 的判据。

The runner records status and diag `backup_reason`, charger state, charger
allow-charge, source-limited latch timing, and load-voltage duration metrics.
It refuses to cut source in the overload-cut scene unless the final pre-cut
hold sample is still `source_limited` backup. `source_in_budget` 要求 VIN 全程在线，且
所有负载已生效样本均不得出现 Backup。suite verifier expects exactly four reports for
this contract and rejects missing phase assertions.

LoadLynx 电流在 formal evidence 中只保留 `load_i_total_ma`。runner 从负载端实测功率与
端电压派生单一总电流，不再把 `local/remote` 字段写入时序或图表。

The source-limited firmware integration also restricts the controlled USB-C
low-output charge exception to `backup_reason=input_absent`. A source-limited
backup remains a `LOAD` non-charging state even when the USB guard would
otherwise permit charging.

Current software gates passed on the implementation branch:

- `cargo test --manifest-path tools/mains-aegis-host/Cargo.toml`
- `cargo test --manifest-path firmware/host-unit-tests/Cargo.toml`
- `just firmware-build-hil`
- `bun run web:check`
- `source-limited-12v` Power Path Validation dry-run with the fixed 12V/3A,
  1000mA, and 3900mA command plan

### Source-limited 12V HIL sign-off

已完成真实台架三场景验证，正式 suite 为
`tools/hil/reports/source-limited-12v-20260711T1818Z/`。可提交的摘要副本为：

- `docs/specs/xjpvj-runtime-mode-switching/evidence/source-limited-12v-20260711T1818Z-suite-summary.json`

该 suite 使用以下绑定设备和传输：

- UPS：`serial-04f3bb3f5367`，12V build
  `617069c4-dirty-4fac56a8bfb7e0ef`
- IsolaPurr：`f293cc9c139e`，`http://192.168.31.224`，manual `12000mV / 3000mA`
- LoadLynx：`loadlynx-d68638`

本次冻结的 `advanced_power` 为：

- `standby_drop_mv=1200`，`assist_low_drop_mv=600`
- `assist_enter_delta_ma=0`，`assist_exit_delta_ma=0`，`assist_required_samples=2`
- `assist_ramp_step_mv=100`，`assist_ramp_interval_ms=200`
- `rated_enter_delta_ma=0`，`rated_exit_delta_ma=0`
- `vin_drop_threshold_pct=4`，`required_samples=2`
- `source_limited_vin_drop_pct=1`，`source_limited_enter_delta_ma=1000`
- `source_limited_exit_delta_ma=0`，`source_limited_required_samples=2`
- `source_limited_recover_margin_mv=400`

`mains-aegis power-validation report --write-overview` 已验证三个 report 均
`signoff_valid=true`，无 failed acceptance checks：

- `12v-backup_only-1000ma`：`5.074Hz`，max gap `0.207s`；VIN cut 后确认
  `input_absent`，backup 连续成立。
- `12v-source_limited_online-3900ma`：`4.945Hz`，max gap `0.213s`；VIN 保持在线时
  `source_limited` 成立，额定输出目标已观察到，锁存后最低负载端电压 `12139mV`，
  锁存前后低于 `11000mV` 的最长时段均为 `0s`。
- `12v-source_limited_cut-3900ma`：`5.064Hz`，max gap `0.354s`；在线
  `source_limited` 于 `0.405s` 锁存，锁存后最低负载端电压 `12151mV`，随后 VIN cut
  保持 backup 并转换为 `input_absent`。

HIL collector 的决定样本与 UPS status collector 存在一次采样偏移，因此报告把
source-limited latch 前的 status 样本作为 pre-latch evidence；正式低电压指标从下一帧
post-latch sample 开始计算。runner 同时过滤显式 stale UPS status，并在每个 scene 开始前
等待 UPS 回到在线态，避免前一场景的 backup 锁存污染下一场景。

报告目录中的 `suite-overview.html` 依赖同目录的 scene chart iframe。Chrome 的本地
`file://` 页面策略阻止本次在浏览器中加载该离线页面，因此未把单独 overview HTML 当作
视觉签核证据；正式签核仍以保留的原始报告、摘要及 verifier 结果为准。

### Archived source-limited diagnostic rerun

完整的后续 12V 三场景证据已归档在：

- `docs/specs/xjpvj-runtime-mode-switching/evidence/source-limited-12v-20260712T0300Z/`

该目录保留 suite overview、三个场景的 `results.json`、完整
`timeseries.jsonl` 与 `voltage-chart.html`，可通过静态 HTTP 服务直接打开
overview 并加载全部 iframe 图表。它是后续控制策略优化的可复核比较基线，不能被
单独 HTML overview 替代。

这次 rerun 的 `backup_only` 与 `source_limited_cut` 均为
`valid_for_signoff`；`source_limited_online` 的功能断言也通过，观察到
`source_limited`、额定目标与锁存后的无低压时段。但该场景有一个 `0.507s` 的
采样间隔，超过 `0.5s` 合同上限，因此其 `run_validity=invalid_diagnostic_only`，
整套证据不得宣称为新的 sign-off。归档的目的在于保留原始遥测和视觉证据，便于未来
定位采样完整性或输出稳定性回归。

### Revalidated source-limited 12V HIL sign-off

更换 IsolaPurr 上级电源后，重新执行完整的 `source-limited-12v` 合同。正式报告归档在：

- `docs/specs/xjpvj-runtime-mode-switching/evidence/source-limited-12v-20260712T0759Z/`

该归档包含 suite overview、三个场景的 `results.json`、`timeseries.jsonl` 与
`voltage-chart.html`。源端仍固定为 manual `12000mV / 3000mA`，两个过载场景仍使用
LoadLynx `CC 3900mA`；IsolaPurr 的 `tps_cdc_rise_mv=300` 在测试前后回读一致，未被
runner 覆盖。

`power-validation report --write-overview` 的 verifier 结果为 `signoff_valid=true`，无
suite 或场景 failure：

- `12v-backup_only-1000ma`：`5.004Hz`，max gap `0.268s`；VIN cut 后持续 backup，并观察到
  `input_absent`。
- `12v-source_limited_online-3900ma`：`4.991Hz`，max gap `0.234s`；`source_limited` 在
  `0.400s` 锁存，额定输出目标已观察到，锁存后最低负载端电压为 `11743mV`，低于
  `11000mV` 的最长时段为 `0s`。
- `12v-source_limited_cut-3900ma`：`5.006Hz`，max gap `0.205s`；`source_limited` 在
  `0.406s` 锁存，锁存后最低负载端电压为 `11731mV`，VIN cut 后保持 backup 并转换为
  `input_absent`。

### 12V false-latch diagnostic and final sign-off refresh

在后续 12V 真机复测中，先归档了一份诊断 evidence：

- `docs/specs/xjpvj-runtime-mode-switching/evidence/source-limited-12v-c22bf968-20260713T0320Z/`

这份 suite 使用 12V build `c22bf968-dirty-7a6332127043087a`。其中
`12v-source_limited_online-3900ma` 与 `12v-source_limited_cut-3900ma` 已恢复为
`valid_for_signoff`，证明前一轮修复已经消除了 VIN cut 过渡时掉回 `standby` 的问题。
但 `12v-backup_only-1000ma` 仍为 `invalid_diagnostic_only`：普通 `1000mA` 在线 hold
阶段被错误锁成 `backup_reason=source_limited`，导致 `hold_tps_power_max_mw=12607`，
明显违反 `hold <= 2W TPS output` 合同。该证据保留为 source-limited 假阳性的正式反例。

根因是 source-limited 进入判据允许把一次瞬时高 `vin_iin_ma` 与后续低输入电流的
TPS-only 样本拼接成连续计数，从而在正常在线负载下误锁 `source_limited`。固件随后收紧为：

- `fast-enter` 仍允许用同一窗口内的 `VIN drop + 高 VIN IIN` 直接快速接管；
- 但纯 TPS-only 的 source-limited 进入，必须建立在当前样本仍为高 `VIN IIN` 或已经连续
  观测到 source-limited 入口级别 `VIN IIN` 的在线历史之上；
- 已锁存 `source_limited` 后，在 `dcin_present` 先掉、`mains_present` 尚未更新为 false 的
  cut 过渡窗口内保持 `backup`，直到原因切换为 `input_absent`。

应用该修复后的最终 12V 真机签核归档在：

- `docs/specs/xjpvj-runtime-mode-switching/evidence/source-limited-12v-c22bf968-20260713T0335Z/`

本次 UPS build 为 `c22bf968-dirty-d8c9ca3fa923b63b`。IsolaPurr 仍保持 manual
`12000mV / 3000mA`，并且 `tps_cdc_rise_mv=300` 在刷机前后、测试前后回读一致，未被
runner 覆盖。suite verifier 为 `signoff_valid=true`，三个 scene 均为
`valid_for_signoff`：

- `12v-backup_only-1000ma`：`4.894Hz`，max gap `0.402s`；hold 维持
  `mode=standby` / `backup_reason=null`，`hold_tps_power_max_mw=391`，VIN cut 后持续
  `backup_reason=input_absent`。
- `12v-source_limited_online-3900ma`：`4.810Hz`，max gap `0.401s`；VIN 在线期间锁存
  `source_limited`，额定 `12000mV` 目标已观察到，锁存后最低负载端电压为 `11755mV`，
  无低于 `11000mV` 的持续段。
- `12v-source_limited_cut-3900ma`：`5.025Hz`，max gap `0.401s`；在线 source-limited
  接管后保持 backup，VIN cut 后连续保持 `backup` 并切换为 `input_absent`；
  `hold_tps_power_max_mw=348`，锁存后最低负载端电压为 `11755mV`。

### Source-limited 19V HIL sign-off

独立 `source-limited-19v` 三场景合同已完成真机签核，完整 evidence 位于：

- `docs/specs/xjpvj-runtime-mode-switching/evidence/source-limited-19v-20260712T1020Z/`

该目录同时保留从已验证 HTTP overview 导出的 `suite-overview.mhtml`，可离线打开并包含三个内嵌 chart。

本次 UPS artifact 为 `main-vout-19v`，
`build_id=b81fcc07-dirty-87c6532c4a89dd5e`。IsolaPurr 保持 manual
`19000mV / 3000mA`，并在测试前后回读到不变的 `tps_cdc_rise_mv=300`。本次采集以
`sample_interval_ms=100`、`ups_watch_freshness_ms=1000` 执行；该配置记录在 suite
summary 中，不与旧的 `750ms` cache-freshness run 混称。

`power-validation report --write-overview` 返回 `signoff_valid=true`：

- `19v-backup_only-1000ma`：`10.006Hz`，max gap `0.204s`；VIN cut 后持续 backup，并观察到 `input_absent`。
- `19v-source_limited_online-3900ma`：`10.036Hz`，max gap `0.123s`；`0.097s` 锁存 `source_limited`，锁存后最低 LoadLynx 电压为 `18732mV`，高于 `18000mV` 门槛。
- `19v-source_limited_cut-3900ma`：`10.042Hz`，max gap `0.201s`；`0.203s` 锁存 `source_limited`，随后 VIN cut 持续保持 backup，并转换为 `input_absent`。

在线限流窗口实测为 `vin_vbus=18896mV`、`vin_iin=2760mA`、
`tps_total_iout=1368mA`、`vin_drop=168mV`。百分比 drop 门槛附近的 ADC/线损偏差采用
有界 `60mV` 容差；TPS 输出、电流接近限流和连续样本条件仍必须同时成立。

当前最值得保留给下一轮实现/验证的结论是：

- 以后再改 runtime-mode 逻辑时，必须继续服从 `docs/hil-runtime-mode-switching.md` 的 formal run-validity 合同
- 以后再观察输出异常时，必须同时看：
  - source output voltage
  - UPS `DCIN`
  - UPS INA `VOUT`
  - load actual voltage
- 若后续要继续优化体验，重点应先定位：
  - 输出电压异常属于在线 assist 逻辑问题
  - 还是 backup / restore 过渡问题
  - 再决定是否继续动 `assist_low` / `assist_rated` 控制逻辑

### 19V input-collapse takeover and standby-voltage validation

已完成专门针对 19V 普通 VIN cut 的实机迭代。完整最终 evidence 位于：

- `docs/specs/xjpvj-runtime-mode-switching/evidence/input-collapse-19v-backup-only-r7-20260712T1320Z/`

UPS 固件为 `main-vout-19v`，`build_id=ca380d08-dirty-b553b1672f280754`。该场景使用
IsolaPurr manual `19000mV / 3000mA`、LoadLynx `CC 1000mA`、`200ms` 采样和
`750ms` UPS watch freshness。IsolaPurr 的 `tps_cdc_rise_mv=300` 在 runner 配置前后
均回读为 `300`；测试结束后 source 和 load 输出均已关闭。

本轮将 `input_absent` 的快速接管依据从“必须先等 VIN < 3V”扩展为“已有 DCIN baseline
时，VIN 跌至 baseline 的 85% 以下且输入电流已塌陷”。这与物理断源一致：上级电源虽可能
尚未把 presence 位清除，但已无法继续供能，UPS 应立即承担负载。

- 早期 r4 的 `standby_drop_mv=1200` evidence 表明，VIN 从 `18.928V` 跌至 `14.216V`
  后仍保持 standby，直到约 `2.776V` 才进入 backup。
- r5 已验证 runner 保持 `tps_cdc_rise_mv=300` 且 backup 断言通过，但一个 `0.601s`
  采样缺口使其只能作为诊断 evidence，不能用于 sign-off。
- r6 使用新的 input-collapse 判据并正式签核：`4.952Hz`、max gap `0.401s`、无 acceptance
  failure；首个 backup 样本在 `VIN=5.432V`、`vin_iin=40mA` 时出现，而不是等待约 2V。
  它同时显示热备目标为 `17.8V` 时，负载端仍会出现约一个采样窗口的 `17.742V` 瞬态。
- r7 仅把完整 `advanced_power` 快照中的 `standby_drop_mv` 从 `1200` 改为 `800`，其余
  15 个字段保持不变，并再次正式签核：`4.967Hz`、max gap `0.401s`、`scene_complete=true`、
  `failed_acceptance_checks=[]`。热备目标升至 `18.2V`，该场景最低 LoadLynx 电压为
  `18.155V`，没有采样点低于 `18.0V`；首个 backup 样本为 `VIN=9.696V`、
  `vin_iin=108mA`，并保持 `backup_reason=input_absent`。

结论必须区分：MCU 接管判定延迟已缩短到首个可观测严重崩落样本，且减小热备压差已消除本次
19V/1000mA 报告中的低于 18V 样本；但 TPS VOUT 遥测仍在目标切换后约一个采样周期才上升，
因而这不是“硬件切换瞬态已完全消除”的结论。

### Final 19V source-limited validation

最终 19V 三场景证据位于：

- `docs/specs/xjpvj-runtime-mode-switching/evidence/source-limited-19v-final-r7-20260712T1441Z/`

该 suite 使用 `main-vout-19v` firmware build
`0c98fe9d-dirty-fee0c84b3135d707`，IsolaPurr manual `19000mV / 3000mA`，
LoadLynx CC `1000mA` 或 `3900mA`，采样间隔 `100ms`，UPS watch freshness
`1000ms`。runner 与测试后回读均确认 `tps_cdc_rise_mv=300`，没有覆盖该线损补偿；
source 与 load 均在结束时关闭。

`mains-aegis power-validation report --write-overview` 返回
`signoff_valid=true`，三个 scene 均无 acceptance failure：

- `19v-backup_only-1000ma`：`10.049Hz`，max gap `0.218s`；VIN cut 后连续 backup，
  `backup_reason=input_absent` 已观察到。
- `19v-source_limited_online-3900ma`：`10.043Hz`，max gap `0.103s`；VIN 保持在线时
  `0.201s` 锁存 `source_limited`，额定 `19000mV` 目标已观察到，锁存后负载端最低
  `18744mV`，没有低于 `18000mV` 的持续段。
- `19v-source_limited_cut-3900ma`：`10.051Hz`，max gap `0.106s`；先在 `0.401s`
  锁存 `source_limited`，锁存后负载端最低 `18744mV`，随后 VIN cut 持续保持 backup，
  并观察到 `input_absent`。

最终进入条件保留 `source_limited_vin_drop_pct=1`、`source_limited_enter_delta_ma=1000`、
连续 `2` 个样本及 `400mV` 恢复回差。19V 实测的 `136mV` VIN drop 处于百分比门槛边缘，
固件仅在同时存在高 VIN 输入电流和 TPS 输出负载时，以有界 `60mV` ADC/线损容差判定
drop 成立；容差不会单独触发接管。

同一 firmware 的前序 r6 cut scene 留下 `17589mV / 0.303s` 的 post-latch 低压反例，
所以 r6 只保留为诊断 evidence。r7 完整重跑才是最终 sign-off；未来若优化 TPS 动态，
必须保留 `>=18000mV` 和最长低压 `<=1s` 这两个合同门槛，不能通过放宽报告判据消除反例。

### Tuned 19V standby and source-limited validation

### Final 12V source-limited validation

The final 12V implementation uses `source_limited_enter_delta_ma=2500`, which expands to a
`2600mA` TPS admission threshold. This preserves the `2900mA` in-budget guard while allowing
the `3900mA` limited-source case to take over after fresh consecutive samples. A latched
`input_absent` backup remains latched while VIN is collapsed even if `mains_present` has not
yet transitioned; a source-limited latch remains source-limited until explicit input absence.

The runner keeps IsolaPurr USB config reads as setup/teardown evidence and uses the UPS USB VIN
ADC for continuous source-path voltage. During control-command scheduling gaps, it backfills
scene samples from received UPS frames using their actual receive timestamps; it does not
interpolate voltage or state. The LoadLynx contract contains only `load_i_total_ma`.

Final evidence:

`docs/specs/xjpvj-runtime-mode-switching/evidence/source-limited-12v-62179e3c-final-r6-20260714T0010Z/`

The recomputed report verifier returned `signoff_valid=true`. The four scenes had effective
sample rates from `5.005Hz` to `5.199Hz` and maximum gaps from `0.205s` to `0.296s`. The 3900mA
online scene latched `source_limited` in `0.6s`, with post-latch minimum load voltage `11766mV`;
the cut scene latched in `0.2s`, remained continuously in Backup, and observed `input_absent`.

`standby_drop_mv=1200` 的最终 r7 虽通过 source-limited 合同，但普通 VIN cut 的切换期
LoadLynx 最低为 `17754mV`。将完整 advanced-power 快照中的该字段恢复为 `800` 后，
普通 VIN cut 的可签核重测把切换期最低值提高至 `18143mV`，UPS VOUT 最低为
`19016mV`，backup 稳态最低负载电压为 `18945mV`。

18.2V standby 同时改变了 3900mA 下的功率分配：诊断 run 实测 `vin_iin≈2017mA`、
`tps_total_iout≈2324mA`、VIN drop `120mV`，且电池在 standby 下持续放电。原先针对
17.8V 工作点的高输入电流门槛无法识别该状态。固件因此仅在 VIN drop 接近 1% 门槛、
`VIN IIN >= 2000mA`、TPS 输出超过配置门槛且连续样本成立时，允许 `80mV` 有界容差
触发 `source_limited`。这把隐式混供转换为可观测的主动 Backup，不允许仅凭容差触发。

最终组合 suite 位于：

- `docs/specs/xjpvj-runtime-mode-switching/evidence/source-limited-19v-tuned-final-20260713T0020Z/`

旧版 suite verifier 在当时不包含 hold 阶段 TPS `2W` 门禁，因此曾返回
`signoff_valid=true`。按当前 Spec 重新审计后，该结果必须降级为诊断 evidence：

- `backup_only`：`9.963Hz`，max gap `0.301s`，VIN cut 后持续 `input_absent` Backup。
- `source_limited_online`：接管后负载最低 `18744mV`，无低于 `18000mV` 的时段。
- `source_limited_cut`：`0.800s` 内接管，接管前低于 `18000mV` 的最长时段为
  `0.599s`；接管后最低 `18744mV`，VIN cut 后持续 Backup 并转为 `input_absent`。

降级依据不是电压合同，而是阶段与功率合同缺失：

- `backup_only` hold 共 `158` 个样本，TPS 输出功率超过 `2W` 的样本为 `137` 个；
  TPS 输出电流平均约 `898mA`，按约 `18.95V` VOUT 折算平均约 `17W`。
- `source_limited_online` hold 共 `80` 个样本，其中 `73` 个超过 `2W`。
- `source_limited_cut` hold 共 `79` 个样本，其中 `71` 个超过 `2W`。

因此该报告只能证明接管后的电压与 reason 转换，不得证明 hold 正常，也不得作为最终产品
签核。Power Path Validation 在实现新的 hold 功率字段、阶段拆分和 hard failure 前，现有
`signoff_valid` 字段不能用于本任务收口。

最终 firmware build 为 `71cbb46b-dirty-5616ee2f7a23d6eb`。IsolaPurr 在测试前后保持
manual `19000mV / 3000mA` 与 `tps_cdc_rise_mv=300`，结束时 source 和 load 均关闭。
首次 800mV 诊断 suite 暴露了 source-limited 未触发，另有 LoadLynx transient 503；
这些 run 保留为诊断证据，不构成最终签核的一部分。

### Final 19V parameter and strategy optimization

Power Path Validation 现在从 fresh `tps_total_iout_ma` 与已启用 UPS VOUT 计算 TPS 输出
功率。任一 `hold` 样本超过 `2000mW` 即失败；3900mA 场景在首次超限后改标
`transition_source_limited`，锁存后改标 `backup_online`。独立 report verifier 也从
`timeseries.jsonl` 重算门禁，因此旧 summary 不能继续伪装通过。

参数扫描确认路径存在非线性跳变：`standby_drop=820mV` 时 1000mA hold TPS 最大约
`20.389W`；840mV 短探针约 `1.092W`，但紧贴边界；1000mV 可行但 VIN cut 下限更低。
最终采用 `900mV`，相对 840mV 单次边界保留 `60mV` 裕量，同时比原 1200mV 热备目标
提高 `300mV`。

900mV 的首次完整 run 还暴露了策略误判：正常 1000mA 的 VIN 输入约 `1102mA`，旧
`2000mA` 固定门槛配合 fast-enter 历史状态会错误锁存。真实 3900mA 源受限样本为
`2394–2411mA`，所以 VIN 输入必要门槛收紧到 `2300mA`；VIN drop、TPS 输出负载和连续
fresh 样本仍全部必需。

最终 evidence 位于：

- `docs/specs/xjpvj-runtime-mode-switching/evidence/source-limited-19v-optimized-cut-r3-20260713T0155Z/`

新 verifier 返回 `signoff_valid=true`：

- `backup_only`：hold TPS 最大 `1089mW`，0 个超限样本，hold 全程 `standby`；VIN cut
  切换期 LoadLynx 最低 `18049mV`。
- `source_limited_online`：hold TPS 最大 `1089mW`，0 个超限样本；`0.400s` 内锁存，
  接管后最低 `18744mV`。
- `source_limited_cut`：hold TPS 最大 `1016mW`，0 个超限样本；接管后最低
  `18744mV`，VIN cut 后持续 Backup 并转换为 `input_absent`。

IsolaPurr `tps_cdc_rise_mv=300` 未被覆盖；最终 source 与 load 均关闭。
## Runtime UPS output bypass

The firmware provides a RAM-only `manual_bypass` output gate for controlled
pass-through experiments. Enable it with
`device <saved-id> output-bypass --enable` and return with `--restore`. Enable
is refused unless VIN is stable; reboot clears the override. This control is
separate from source-limited backup takeover and is not a production UPS mode.

## 12V repaired-hardware revalidation

TPS2490 输入 MOS 更换及网表修复后，INA3221 CH3 已确认采样 `VIN_UNSAFE`，位于
TPS2490 输入 MOS 前级。固件 owner-facing 字段使用 `pre_tps_vin_mv`；旧
`vin_vbus_mv` 暂时保留为兼容别名。MCU 通过 `UPS_IN_CE` 主动控制 TPS2490 `EN`：

- 连续 3 个 fresh 前级 VIN 样本低于 `10000mV` 时关断输入，并进入
  `backup_reason=input_absent`。
- 输入门关断后，连续 3 个 fresh 样本高于 `11000mV` 才重新使能。
- `10000mV..=11000mV` 为回差区；缺样重置连续计数。
- 真机阶梯验证确认 `9.544V` 保持 cutoff/Backup、`10.576V` 不恢复、`11.552V`
  恢复输入并回到 Standby。

source-limited 入口还修复了两个采样时序问题：同一个 stale TPS 样本不得重复增加
连续计数；首个有效限流候选立即把 TPS 目标预升到额定值，第二个 fresh 样本若仍确认
UPS 承担至少 `500mA`，则锁存 `Backup/source_limited`。第二次确认不再要求 VIN 继续低于
门槛，因为预升压本身会暂时恢复 VIN 和输入电流；否则控制器会在 Standby 与
AssistRated 之间振荡。

最终 12V evidence：

`docs/specs/xjpvj-runtime-mode-switching/evidence/source-limited-12v-ce343924-uvlo-preboost-final-20260714T1206Z/`

设备与设置快照：UPS `serial-04f3bb3f5367` / `mains-aegis-198840`，firmware build
`ce343924-dirty-082fa5be3821a7e7`；IsolaPurr `f293cc9c139e`，manual
`12000mV / 3000mA`；LoadLynx `loadlynx-d68638`。`source_in_budget` 根据修复后台架
实测改为 `2500mA`，避免把接近 3A 源边界的正常 CC 环路波动当成产品能力内基准。

`mains-aegis power-validation report --write-overview` 返回 `signoff_valid=true`，四个
scene 均为 `valid_for_signoff` 且无 acceptance failure：

- `backup_only / 1000mA`：`5.181Hz`，max gap `0.212s`，VIN cut 后持续 Backup 并观察到
  `input_absent`。
- `source_in_budget / 2500mA`：`5.051Hz`，max gap `0.208s`，76 个加载样本中 Backup、
  offline 与 source-limited 均为 0。
- `source_limited_online / 3900mA`：`0.601s` 锁存，锁存后最低负载电压 `11790mV`，
  低于 `11000mV` 的持续时间为 0。
- `source_limited_cut / 3900mA`：`1.002s` 锁存，锁存前低压最长 `0.402s`，锁存后
  最低 `11790mV`；物理断源后 Backup 连续并转为 `input_absent`。

旧 evidence
`source-limited-12v-62179e3c-final-r6-20260714T0010Z` 保留为 MOS/网表修复前的历史
基线，不能用于评价修复后硬件的当前输入压降。旧 assist_path 约 `10.5V` 的长跌落同样
保留为历史问题证据，而不是当前硬件能力结论。

### 12V 100mV UVLO Sweep on 93aadc61

在 `93aadc61-clean-eb2b310e1419a6cc` 上完成了新的 12V 四场景步进调优，归档目录：

`docs/specs/xjpvj-runtime-mode-switching/evidence/source-limited-12v-93aadc61-uvlo-sweep-20260714T1636Z/`

该目录根部保留推荐候选 A 的完整 suite；`suite-overview.mhtml` 是从浏览器导出的完整
离线快照；`comparison.json` 则汇总三个 `100mV` 候选点。台架仍为 UPS
`serial-04f3bb3f5367` / `mains-aegis-198840`、IsolaPurr `f293cc9c139e`
(`12000mV / 3000mA`) 与 LoadLynx `loadlynx-d68638`。

固定不变参数：

- `standby_drop_mv=700`，即 `11.3V standby`
- `input_uvlo_required_samples=3`
- `source_limited_vin_drop_pct=1`
- `source_limited_enter_delta_ma=2500`
- `source_limited_exit_delta_ma=0`
- `source_limited_required_samples=2`
- `source_limited_recover_margin_mv=400`

候选结果：

- 候选 A `11.3V / 11.5V`：四场景全部 `signoff_valid=true`
  - `source_in_budget / 2500mA`：无 Backup、无 offline 样本
  - `source_limited_online / 3900mA`：`0.399s` 锁存，锁存后最低负载电压 `11790mV`
  - `source_limited_cut / 3900mA`：同样维持 `11790mV`，VIN cut 后连续 Backup 并转为
    `input_absent`
- 候选 B `11.4V / 11.6V`：`source_in_budget / 2500mA` 误入 Backup，出现
  `8` 个 Backup/Offline 样本，suite 只能算 `invalid_diagnostic_only`
- 候选 C `11.5V / 11.7V`：四场景同样 `signoff_valid=true`
  - `source_limited_online / 3900mA`：`0.400s` 锁存，锁存后最低负载电压同为 `11790mV`
  - `source_limited_cut / 3900mA`：`0.598s` 锁存，VIN cut 后保持连续 Backup

推荐仍为候选 A，而不是更高的 cutoff：

- 它是通过点中 cutoff 最低的一组，符合“不过早切 Backup”的优先级
- 与候选 C 相比，3900mA 两个场景的锁存后最低负载电压相同，均未出现 `<11V`
  的连续低压段
- 候选 B 的失败说明这个门限不是“越高越好”的单调问题；提高 `100mV` 可能把
  `2500mA` 正常在线场景误判成 `input_absent` Backup

这轮 sweep 也确认，相比旧 assist_path 历史 evidence 中约 `10.5V` 的长时间跌落，
当前 `source-limited` 主动接管策略在推荐候选 A 上已经把 3900mA 场景的锁存后最低
负载电压维持在 `11790mV`，改善成立。
