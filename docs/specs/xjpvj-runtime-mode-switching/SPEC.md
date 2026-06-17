# UPS 运行态模式切换与充电联动（#xjpvj）

## 状态

- Status: active
- Created: 2026-06-16
- Last: 2026-06-16

## 背景 / 问题陈述

- 运行态 `STANDBY / ASSIST / BACKUP` 切换语义此前散落在 Dashboard、自检、音效与 charger policy 多份规格中，且代码仍保留了“TPS enable 即有输出”“`mains_present=None` 且输出开启即 `BACKUP`”等过时判据。
- 现有主线实现已把 `DC5025 VIN >= 3V` 建立为 UI/音效的市电真相源，但 `ASSIST` 与 `BACKUP` 的进入条件、缺样保持策略、以及与 charger `LOAD/NOAC` token 的关系仍缺少统一 topic-level contract。
- 若继续让模式判定、charger token 和 owner-facing `status/power-diag/trace` 各自演进，运行态会再次出现 UI、音效、主机工具与实际固件行为不一致的问题。

## 目标 / 非目标

### Goals

- 建立单一 runtime-mode topic spec，统一 `BYPASS / STANDBY / ASSIST / BACKUP` 的定义、自动切换边界与 owner-facing 可观测性。
- 固定自动运行态集合仅为 `STANDBY / ASSIST / BACKUP`；`BYPASS` 仅表示显式 UPS-off / 旁路管理态。
- 固定 `ASSIST` 判据为 `VIN >= 3V` 且 `TPS total output current` 达到阈值，并引入 `100mA enter / 50mA exit` 回差与 `2` 个连续 fresh 样本锁存。
- 固定 `BACKUP` 只允许在“确认无输入”时进入；当 `mains_present` 未知时保持上一确认模式，不得因输出活跃而直接跳 `BACKUP`。
- 固定 `ASSIST / BACKUP` 都是 non-charging mode，`charger.allow_charge=false`，并与现有 runtime token `LOAD / NOAC` 建立明确边界。

### Non-goals

- 不重写 `eu2b8` 中完整的 `CHG500 / CHG100 / RECOV / FULL / WARM / TEMP / WAIT30` 状态机。
- 不新增 owner-facing 的“强制切换 UPS 模式”控制命令。
- 不把 `BYPASS` 重新纳入自动运行态切换。
- 不覆盖 UI 视觉资产、Dashboard layout 或音频 cue 优先级本身。

## 范围（Scope）

### In scope

- 运行态 `UpsMode` 的自动切换口径。
- `VIN` 主真相源、VIN 缺样保持、fallback 输入存在信号的命名与优先级。
- `TPS total output current` 在 `STANDBY / ASSIST` 子态判定中的门槛、回差与 fresh-sample 约束。
- `ASSIST / BACKUP` 与 charger allow/token 的硬联动。
- owner-facing `status / power-diag / trace` 需要暴露的最小字段。

### Out of scope

- 手动 charge、BMS 激活、USB PD 恢复、DC input adaptive derate 的完整行为定义。
- 面向用户的新设置项或持久化配置。
- 运行态声音素材或前面板视觉风格变更。

## 接管说明

- 本规格是运行态模式切换主题的 canonical source。
- `docs/specs/6qrjs-front-panel-industrial-ui-preview/SPEC.md` 仅保留视觉与文案冻结，不再拥有模式切换语义。
- `docs/specs/g2kte-dashboard-live-after-self-check/SPEC.md` 继续拥有 `VIN` 主真相源与 transient miss 行为，但自动模式切换规则引用本规格。
- `docs/specs/eu2b8-bq25792-charge-policy/SPEC.md` 继续拥有 charger policy 状态机；仅在 `ASSIST/BACKUP` 非充电边界上引用本规格。
- `docs/specs/h43mk-main-firmware-runtime-audio-cues/SPEC.md` 继续拥有音频 cue 规则；其市电/模式基础引用本规格与 #g2kte。

## 术语与真相源

- `VIN mains truth source`: `DC5025 VIN >= 3V`。只要有 fresh VIN 电压样本并跨过 `3V` 门槛，运行态模式必须优先使用该结果。
- `聚合输入存在信号` (`aggregate input-present signal`): 当 VIN 连续缺样并超过 latch 容错窗口时，允许使用的降级布尔输入存在信号。当前实现可继续复用现有聚合布尔源，但文档不再把它笼统写成 charger `input_present`。
- `TPS total output current`: owner-facing 聚合输出电流，来源为运行时 `tps_total_iout_ma`。
- `fresh sample`: 在模式判定上下文中，指本轮相对于上一个已消费 sample_seq 的新 `tps_total_iout_ma` 样本。
- `confirmed mode`: 最近一次满足进入条件并完成锁存的自动运行态结果。

## 模式定义

- `BYPASS`
  - 仅表示显式 UPS-off / 旁路管理态。
  - 不属于自动运行态切换集合。
- `STANDBY`
  - 输入确认在线。
  - TPS 目标保持在“近乎零补能”的热备档位，不应持续明显分担负载。
  - TPS 总输出电流尚未达到 `ASSIST` 进入门槛，或已回落到 `STANDBY` 退出门槛以下并完成锁存。
  - 允许 charger policy 在自身条件满足时继续充电。
- `ASSIST`
  - 输入确认在线。
  - TPS 总输出电流达到 `ASSIST` 进入门槛并完成锁存。
  - owner-facing 仍只暴露一个 `ASSIST / supplement` 模式名，但内部允许两阶段：
    - `assist_low`: 先保持低补能档位，确认 TPS 已参与供能但仍优先让墙电直通。
    - `assist_rated`: 当 `VIN` 相对基线持续下陷且 `TPS total output current` 持续升高时，TPS 升到额定输出接管档位。
  - 固定为 non-charging mode。
- `BACKUP`
  - 输入确认离线。
  - 输出由电池侧供能。
  - TPS 目标保持额定输出档位。
  - 固定为 non-charging mode。

## 自动运行态切换规则

### 1. 自动切换集合

- 自动运行态只允许在 `STANDBY / ASSIST / BACKUP` 之间切换。
- `BYPASS` 只能由显式管理态进入/退出，自动判定逻辑不得自行产出 `BYPASS`。

### 2. 输入在线 / 离线判定

- 若 `VIN` 有 fresh 电压样本：
  - `VIN >= 3V` => 输入确认在线。
  - `VIN < 3V` => 输入确认离线。
- 若 `VIN` 只是瞬时缺样，且仍在现有 VIN latch 容错窗口内：
  - 保持最近一次已知 `VIN` 在线/离线状态。
- 若 `VIN` 连续缺样并超出 latch 容错窗口：
  - 允许回退到“聚合输入存在信号”。
  - fallback 为 `true` => 输入确认在线。
  - fallback 为 `false` => 输入确认离线。
  - fallback 仍未知 => 输入状态未知。

### 3. `STANDBY <-> ASSIST` 子态判定

- 前提：输入确认在线。
- `ASSIST` enter:
  - `tps_total_iout_ma >= 100mA`
  - 连续 `2` 个 fresh 样本满足
- `STANDBY` re-enter:
  - `tps_total_iout_ma <= 50mA`
  - 连续 `2` 个 fresh 样本满足
- 若 `tps_total_iout_ma` 样本缺失、不 fresh、或 sample_seq 未前进：
  - 保持上一确认的 `STANDBY / ASSIST` 子态
  - 不得因为 enable flag、零值默认值或单次缺样而切换
- `ASSIST` staged takeover:
  - 仅在 `input_source=dcin` 且输入仍确认在线时适用。
  - `assist_low -> assist_rated` 的主判据固定为：
    - `vin_drop_mv` 相对 `vin_baseline_mv` 持续超过当前基线自适应阈值；并且
    - `tps_total_iout_ma >= 100mA`
    - 连续 `2` 个 fresh 样本同时满足
  - `assist_rated -> assist_low` 退出必须满足回差：
    - `vin_drop_mv` 回落到升额阈值的一半以内；并且
    - `tps_total_iout_ma <= 50mA`
    - 连续 `2` 个 fresh 样本同时满足
  - `tps_total_iout_ma` 单独升高但 `VIN` 未持续下陷，不得升到 `assist_rated`。

### 4. `BACKUP` 进入 / 退出

- 仅在“输入确认离线”时允许进入 `BACKUP`。
- 输入确认离线的充分条件：
  - fresh `VIN < 3V`；或
  - `VIN` 连续缺样超过 latch 窗口后，fallback `aggregate input-present=false`
- 当输入状态未知时：
  - 必须保持上一确认模式
  - 不得因 `TPS` 有输出、输出 enable、或其它保守推断而直接切到 `BACKUP`
- 一旦输入再次确认在线：
  - 自动离开 `BACKUP`
  - 下一状态按 `TPS total output current` 锁存结果进入 `STANDBY` 或 `ASSIST`

## 与 charger policy 的硬联动

- `STANDBY`
  - charger 是否允许工作，继续由 `eu2b8` 主线策略决定。
  - `CHG / WAIT / FULL / WARM / TEMP / CHG100 / RECOV` 等语义只在此模式内讨论。
- `ASSIST`
  - 必须视为 non-charging mode。
  - `charger.allow_charge=false`
  - owner-facing charger token/notice 收敛到 `LOAD` 语义边界。
- `BACKUP`
  - 必须视为 non-charging mode。
  - `charger.allow_charge=false`
  - owner-facing charger token/notice 收敛到 `NOAC` 语义边界。
- 本联动只定义模式与 charger 的边界，不覆盖 `eu2b8` 内部关于 `DC IN` 压力、cooldown、recovery ramp 或手动 charge 的全部细节。

## owner-facing 可观测性要求

- `status` 至少暴露：
  - `mode`
  - `input.mains_present`
  - `input.vin_vbus_mv`
  - `input.assist_power_stage`
  - `input.assist_target_vout_mv`
  - `input.tps_total_iout_ma`
  - `charger.allow_charge`
  - `charger.detail_status`
- `power-diag` 至少暴露：
  - 输入在线/离线结果
  - `assist_power_stage`
  - `assist_target_vout_mv`
  - `tps_total_iout_ma`
  - charger allow/token 结果
- `trace(kind=event,target=power)` 应能让 owner 看到：
  - 输入真相源变化
  - `TPS total output current` 停充或恢复相关根因

## 验收标准（Acceptance Criteria）

- Given `VIN >= 3V` 且 `tps_total_iout_ma <= 50mA` 连续 `2` 个 fresh 样本，When 自动模式判定更新，Then 结果为 `STANDBY`。
- Given `VIN >= 3V` 且 `tps_total_iout_ma >= 100mA` 连续 `2` 个 fresh 样本，When 自动模式判定更新，Then 结果为 `ASSIST`。
- Given `mode=assist` 且 `vin_drop_mv` 未持续超过基线自适应阈值，When `tps_total_iout_ma` 单独升高，Then 内部阶段保持 `assist_low`，不得直接升到额定接管档。
- Given `mode=assist`、`vin_drop_mv` 持续超过基线自适应阈值且 `tps_total_iout_ma >= 100mA` 连续 `2` 个 fresh 样本，When 自动模式判定更新，Then 内部阶段升到 `assist_rated`，但 owner-facing `mode` 仍为 `supplement`。
- Given 已处于 `assist_rated`，When `vin_drop_mv` 回落到退出回差内且 `tps_total_iout_ma <= 50mA` 连续 `2` 个 fresh 样本，Then 内部阶段降回 `assist_low`，不抖动。
- Given 已处于 `ASSIST`，When `TPS total output current` 在 `50..100mA` 之间抖动或缺少 fresh 样本，Then 模式保持 `ASSIST`。
- Given 已处于 `STANDBY`，When `TPS total output current` 在 `50..100mA` 之间抖动或缺少 fresh 样本，Then 模式保持 `STANDBY`。
- Given 输入状态未知，When `TPS` 仍在输出，Then 模式保持上一确认态，不得仅因输出活跃直接进入 `BACKUP`。
- Given `VIN < 3V`，When 自动模式判定更新，Then 结果为 `BACKUP`。
- Given `VIN` 连续缺样超过窗口且 `aggregate input-present=false`，When 自动模式判定更新，Then 结果为 `BACKUP`。
- Given `ASSIST` 已锁存，When 查看 `status/power-diag`，Then `charger.allow_charge=false` 且 charger token 对齐 `LOAD`。
- Given `BACKUP` 已锁存，When 查看 `status/power-diag`，Then `charger.allow_charge=false` 且 charger token 对齐 `NOAC`。

## 参考（References）

- `docs/specs/g2kte-dashboard-live-after-self-check/SPEC.md`
- `docs/specs/eu2b8-bq25792-charge-policy/SPEC.md`
- `docs/specs/h43mk-main-firmware-runtime-audio-cues/SPEC.md`
- `firmware/src/output/mod.rs`
- `firmware/src/output/pure.rs`
