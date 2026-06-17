# 实现记录（#xjpvj）

## 当前实现范围

- 主固件运行态模式判定迁移到：
  - `VIN` 主真相源
  - `TPS total output current` 的 `100mA enter / 50mA exit`
  - `2` 个连续 fresh 样本锁存
  - `mains_present=None` 时保持上一确认模式
- `status` / `power-diag` / Dashboard charger token 与 `ASSIST / BACKUP` 的 non-charging 联动同步收敛。
- 当前固件已把 `ASSIST` 扩展为内部 staged takeover：
  - `standby` 使用低于额定输出的热备目标电压
  - `assist_low` 使用较高但仍非额定的低补能目标电压
  - `assist_rated` 与 `backup` 使用额定输出目标电压
  - owner-facing `mode` 仍只暴露 `standby / supplement / backup`

## 本轮落地项

- 新建 runtime-mode topic spec，并把旧 spec 的模式定义改为引用此 spec。
- 移除 `tps_a_enabled || tps_b_enabled` 作为 `ASSIST` 判据。
- 移除 `None + output=true => BACKUP` 逻辑。
- 在 `firmware/src/output/pure.rs` 新增 staged assist pure tracker，使用：
  - `vin_baseline_mv / vin_drop_mv`
  - `TPS total output current`
  - `2` 个连续 fresh 样本
  作为 `assist_low <-> assist_rated` 的唯一主判据。
- 在 `firmware/src/output/mod.rs` 复用现有 runtime-mode tracker 与 `dcin_input_pressure_step()` 路径，把目标电压切换接入 TPS 运行时重配。
- 在 `status` / `power-diag` / host LAN fallback 中新增：
  - `assist_power_stage`
  - `assist_target_vout_mv`
- 补齐 host-unit-tests / firmware tests，覆盖：
  - `STANDBY <-> ASSIST` 滞回
  - VIN 瞬时缺样保持
  - fallback false 才进入 `BACKUP`
  - `ASSIST / BACKUP` 禁充 token
  - `assist_low` 不误升额
  - `VIN drop + TPS iout` 双判据才进入 `assist_rated`
  - 输入恢复后带回差地从 `assist_rated` 降回 `assist_low`

## 验证状态

- `cargo test --manifest-path firmware/host-unit-tests/Cargo.toml` 已通过，覆盖 runtime-mode 锁存、缺样保持、`BACKUP` 进入门槛、`ASSIST / BACKUP` 禁充联动，以及 staged assist 的升额/降额回差。
- `cargo check --manifest-path firmware/Cargo.toml` 在当前工作机默认 stable Rust 下被 `xtensa-lx` 的 nightly-only 特性阻断；这不是本轮变更引入的源码错误，但意味着仍需在项目既定 ESP 工具链下做一次固件侧编译确认。
- 2026-06-17 的最终 owner-facing HIL canonical run 已收敛到 `IsolaPurr 13.0V / 3.0A + LoadLynx 1A/2A/3A/3200mA + UPS OUT`。
- 这个 `13.0V` 台架设置是当前固件缺少 “TPS standby 电压 / backup 额定电压” 双档目标时的 bench compensation：
  - 它不是产品语义
  - 它只是在当前固定 `12.0V` TPS 目标下，把 `VIN` 提高到足以让 `STANDBY` 和真正的 battery assist 分界变得可观测
- 当前主证据以 `UPS LAN status + devd serve-http /api/v1/devices/serial-04f3bb3f5367/power-diag` 为主。
- 本轮 USB `trace(kind=event,target=power)` 没有给出新 `13.0V` sequence 的 fresh stage-local power events，只保留了旧缓冲历史；因此这轮 trace 视为 degraded evidence，不能作为主验收面。

## 本轮 `13.0V` HIL 实测结论

- `STANDBY` 基线：
  - UPS：`status.mode=standby`
  - 输入：`mains_present=true`，`vin_vbus_mv=13024mV`，`vin_iin_ma=28mA`，`tps_total_iout_ma=40mA`
  - 输出：`out_a=12064mV/16mA`，`out_b=12072mV/20mA`
  - Battery：`pack_mv=16234mV`，`current_ma=0`
  - Charger：`detail_status=WAIT`
  - IsolaPurr：manual `13.0V / 3.0A`，`port_c=13026mV/4mA`
  - LoadLynx：disabled，`v_local_mv=13050`，`calc_p_mw=91`
- `1A target`：
  - UPS：`status.mode=standby`
  - 输入：`mains_present=true`，`vin_vbus_mv=12720mV`，`vin_iin_ma=1051mA`，`tps_total_iout_ma=40mA`
  - 输出：`out_a=12064mV/16mA`，`out_b=12072mV/20mA`
  - Battery：`current_ma=0`
  - Charger：`detail_status=WAIT`
  - IsolaPurr：`port_c=12971mV/1003mA`
  - LoadLynx：`target_i_ma=1000`，`i_local_ma=1002`，`calc_p_mw=12774`
- `2A target`：
  - UPS：`status.mode=standby`
  - 输入：`mains_present=true`，`vin_vbus_mv=12472mV`，`vin_iin_ma=2068mA`，`tps_total_iout_ma=36mA`
  - 输出：`out_a=12064mV/16mA`，`out_b=12072mV/20mA`
  - Battery：`current_ma=0`
  - Charger：`detail_status=WAIT`
  - IsolaPurr：`port_c=12970mV/2002mA`
  - LoadLynx：`target_i_ma=2000`，`i_local_ma=1000`，`i_remote_ma=988`，`calc_p_mw=24512`
- `3A target`：
  - UPS：`status.mode=standby`
  - 输入：`mains_present=true`，`vin_vbus_mv=12088mV`，`vin_iin_ma=3062mA`，`tps_total_iout_ma=68mA`
  - 输出：`out_a=12064mV/16mA`，`out_b=12064mV/52mA`
  - Battery：`pack_mv=16231mV`，`current_ma=-22mA`
  - Charger：`detail_status=WAIT`
  - IsolaPurr：`port_c=12841mV/2978mA`
  - LoadLynx：`target_i_ma=3000`，`i_local_ma=1499`，`i_remote_ma=1487`，`calc_p_mw=35473`
- `3200mA target`：
  - UPS：`status.mode=supplement`
  - 输入：`mains_present=true`，`vin_vbus_mv=12096mV`，`vin_iin_ma=3062mA`，`tps_total_iout_ma=272mA`
  - 输出：`out_a=12064mV/16mA`，`out_b=12072mV/256mA`
  - Battery：`pack_mv=16208mV`，`current_ma=-173mA`
  - Charger：`detail_status=LOAD`
  - IsolaPurr：`port_c=12836mV/2978mA`
  - LoadLynx：`target_i_ma=3200`，`i_local_ma=1600`，`i_remote_ma=1587`，`calc_p_mw=37826`
- `BACKUP @ 3200mA target`：
  - `POST /api/v1/ports/port_c/power?enabled=0` 后稳定到 `status.mode=backup`
  - UPS：`mains_present=false`，`input.source=usbc`，`vin_vbus_mv=2096mV`，`vin_iin_ma=5mA`，`tps_total_iout_ma=3300mA`
  - 输出：`out_a=12000mV/1660mA`，`out_b=12000mV/1640mA`
  - Battery：`pack_mv=15882mV`，`current_ma=-2462mA`
  - Charger：`detail_status=NOAC`
  - IsolaPurr：`port_c_enabled=false`
  - LoadLynx：`target_i_ma=3200`，`i_local_ma=1600`，`i_remote_ma=1587`，`calc_p_mw=37523`
- 恢复 `STANDBY`：
  - UPS：`status.mode=standby`
  - 输入：`mains_present=true`，`vin_vbus_mv=13024mV`，`vin_iin_ma=28mA`，`tps_total_iout_ma=36mA`
  - Battery：`current_ma=0`
  - Charger：`detail_status=WAIT`
  - IsolaPurr：`port_c=13024mV/3mA`，`port_c_enabled=true`
  - LoadLynx：disabled，`v_local_mv=13038`，`calc_p_mw=91`

## 解释与边界

- 这轮 `13.0V` bench 结果比旧 `12.0V / 12.5V` HIL 更可信：
  - `STANDBY` 时 `VIN` 明确高于固定 TPS 输出电压
  - `1A / 2A / 3A` 都保持 `STANDBY`
  - `ASSIST` 只在 `3200mA` 出现，同时伴随负电池电流与显著的 `tps_total_iout_ma`
- 这说明之前的低电压 bench 结论不能继续作为 canonical 结果引用；当前 canonical run 应以这轮 `13.0V` bench 为准。
- 这轮结果仍不等于“产品已经实现了 TPS dual target voltage”：
  - 现在只是用更高的 bench input 电压，绕开了当前固件固定 `12.0V` TPS 目标带来的歧义
  - 未来如果要把产品语义做正确，仍需要单独实现 “non-`BACKUP` 用较低 TPS standby 电压，`BACKUP` 再升到额定电压”
- `ASSIST -> LOAD` 属于 runtime mode coupling，本轮明确观测到：
  - `3200mA` 阶段 `policy.status=LOAD`
  - charger `detail_status=LOAD`
- 这不等于 `eu2b8` 的 charger-pressure/cooldown 分支已经验证通过：
  - 各 accepted stage 仍然 `pressure_reason=none`
  - 因此本 spec 的 mode-loop HIL 可以视为通过，而 `eu2b8` 的压力联动分支仍需单独验证

## 当前 bench 限制

- `USB trace` 在本轮没有产出新的 stage-local power events；只能作为 degraded supplementary evidence。
- `LoadLynx` 先前出现过一次 transient `HTTP 503 LINK_DOWN`，另有一轮 `1A` control/status divergence 调查；这两者都不能替代 UPS 自身的 mode 证据。
- `IsolaPurr` released host `ports power --url` 路径在本 DUT 上返回 `HTTP 400`，本轮 `BACKUP` 依赖 raw device HTTP `enabled=0|1` 作为自动切断/恢复手段。
- `IsolaPurr` 与 `LoadLynx` 都应优先用显式 `--url` 控制，避免 stale saved transport 把 HIL 执行面搞偏。

## 剩余非目标

- 不重写 charger policy 全状态机。
- 不新增 owner-facing 模式强制切换命令。
- 不扩展到 soak 或全矩阵 HIL。
