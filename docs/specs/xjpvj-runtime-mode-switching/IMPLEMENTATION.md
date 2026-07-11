# 实现记录（#xjpvj）

## 当前实现真相

当前主线实现已经收敛到以下事实：

- `VIN` 是运行态输入在线/离线的主真相源
- `mains_present=None` 时保持上一确认模式
- `BACKUP` 表示 UPS 已接管负载，进入原因由 `backup_reason` 区分：
  - `input_absent`
  - `source_limited`
- `input_absent` 仍只允许在确认无输入时进入
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

当前 `advanced_power` 契约已经是 16 字段：

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
- `source_limited_vin_drop_pct`
- `source_limited_enter_delta_ma`
- `source_limited_exit_delta_ma`
- `source_limited_required_samples`
- `source_limited_recover_margin_mv`

实现状态：

- owner-facing 保存语义仍然是相对值或无量纲值
- EEPROM 使用 `AdvancedPowerRecordV3`
- 继续兼容旧 `V1 / V2` 记录的默认值补齐读取
- `status / diag-snapshot` 已暴露：
  - `assist_power_stage`
  - `assist_target_vout_mv`
  - `backup_reason`

新增 source-limited 默认值保持保守：

- `source_limited_vin_drop_pct=4`
- `source_limited_enter_delta_ma=1900`
- `source_limited_exit_delta_ma=0`
- `source_limited_required_samples=2`
- `source_limited_recover_margin_mv=400`

因此默认 source-limited 进入电流门槛为 `rated_enter_base 100mA + 1900mA = 2000mA`，目标是覆盖 `12V / 3A source + 3900mA load` 这类超电源能力负载，而不是把 1A 级一般 assist 负载提前切到 backup。

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
- `source_limited_online / 3900mA`
- `source_limited_cut / 3900mA`

The runner records status and diag `backup_reason`, charger state, charger
allow-charge, source-limited latch timing, and load-voltage duration metrics.
It refuses to cut source in the overload-cut scene unless the final pre-cut
hold sample is still `source_limited` backup. The suite verifier expects exactly
three reports for this contract and rejects missing phase assertions.

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

Real HIL evidence is not yet signed off. The specified UPS USB port was held
by an external `flux-purr-devd` session, and the specified LoadLynx USB path
returned a released-devd serial-open failure. Neither condition was bypassed
or force-cleared; the next live run must start only after those owners release
the ports and the preflight read paths are healthy.

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
