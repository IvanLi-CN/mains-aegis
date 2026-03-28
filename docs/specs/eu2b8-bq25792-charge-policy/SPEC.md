# BQ25792 500mA 充电策略与 DC 过流降档（#eu2b8）

## 状态

- Status: 部分完成（4/5）
- Created: 2026-03-28
- Last: 2026-03-28

## 背景 / 问题陈述

- 主线固件此前只做 `BQ25792` 的安全门控，没有把“何时开始充电、何时继续、何时停充、何时降档”固化成明确的运行时策略。
- 设计文档里曾有 `1A/500mA/100mA` 多档设想，但当前产品需求已经收敛为“常规 `500mA`、`DC5025` 过流时降到 `100mA`、快充暂不做”。
- 如果继续依赖硬件默认寄存器值或瞬时 `allow_charge` 条件，充电行为不可解释，也无法满足“低于 `80%` 或最低单体低于 `3.70V` 才开始、开始后持续到满充”的口径。

## 目标 / 非目标

### Goals

- 固化主线充电策略：默认 `500mA`，不实现任何 `>500mA` 快充。
- 使用 `BQ40Z50` 可信遥测决定是否开始充电：`RSOC < 80%` 或 `最低单体电压 < 3.70V`。
- 充电一旦开始，保持到“满充”才停止；满充定义为 `BQ40 FC` 或 `BQ25792 termination_done` 任一成立。
- 仅在 `DC5025` 独占输入且 `IBUS > 3.0A` 持续 `1s` 时，把 `ICHG` 降到 `100mA`；回落到 `<2.7A` 持续 `5s` 后恢复 `500mA`。
- `TPS55288` 总输出功率超过 `5W` 时必须停充，功率回落后再按正常阈值重新判定是否开启充电。
- 扩充运行时日志与前面板 detail 状态，让 `WAIT / CHG500 / CHG100 / FULL / LOCK / NOAC / TEMP / LOAD` 等状态可直接观察，并优先显示实际 `IBAT_ADC`。

### Non-goals

- 不实现 `1A/2A` 快充、不调 USB-C/PD/PPS 协商。
- 不修改 `BQ40Z50` Data Flash、JEITA 曲线或 termination current 校准。
- 不改 `tps-test-fw` 的独立充电逻辑。

## 范围（Scope）

### In scope

- `firmware/src/output/mod.rs` 里的主线 charger poll 逻辑。
- `BQ40Z50` 运行时快照到充电策略状态机的连接。
- `BQ25792` 正常充电电流/电压写入，以及 `DC5025` 独占输入的降档恢复逻辑。
- `firmware/src/front_panel_scene.rs` 的 charger detail 状态呈现。
- `tools/front-panel-preview/` 的 charger policy 预览场景。

### Out of scope

- 新增对外控制命令、设置项或持久化配置。
- 任何硬件改板、电阻档位调整、输入源优先级改动。
- 将预览图以外的 UI 视觉语言重做。

## 需求（Requirements）

### MUST

- 正常充电目标电流固定为 `500mA`。
- `RSOC < 80%` 或 `cell_min_mv < 3700` 时启动充电。
- 启动后持续充到满，不因中途回到阈值上方而停充。
- 满充后进入锁存停充，直到再次跌破启动阈值才允许重启。
- `DC5025` 独占输入下，`IBUS > 3000mA/1s` 降到 `100mA`，`IBUS < 2700mA/5s` 恢复 `500mA`。
- `TPS55288` 输出功率超过 `5W` 时立即停充。
- BMS 遥测缺失、`charge_ready=false`、输入缺失、`VBAT_PRESENT=false`、`TS_COLD=true` 或 `TS_HOT=true` 时 fail-safe 禁充。

### SHOULD

- 日志应直接输出策略状态、启动原因、满充原因、目标 `ICHG`、输入源与 DC 降档计时器。
- Dashboard charger detail 应显示短状态 token，同时在 notice 里保留精确状态名。
- Dashboard charger detail 与首页 charge 区域应优先显示 `BQ25792 IBAT_ADC` 实测电流；若 `IBAT_ADC` 暂时不可用，则回退到目标 `ICHG`。

### COULD

- 后续在不改状态机语义的前提下，把阈值提升为配置项。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 正常空闲时，如果 `RSOC >= 80%` 且 `cell_min_mv >= 3700`，策略状态为 `idle_wait_threshold`，charger detail 显示 `WAIT`，不写入正常充电目标。
- 一旦任一启动阈值满足，且输入/BMS/温度都允许，策略进入 `charging_500ma`，固件显式写入 `VREG=16.8V` 与 `ICHG=500mA`，再打开 `EN_CHG` 和 `CE`。
- 充电保持期间，即使 `RSOC` 或 `cell_min_mv` 回升到阈值上方，也继续保持 `charging_500ma` 或 `charging_100ma_dc_derated`，直到满充。
- 满充后策略进入 `full_latched`，固件停充并保持停充；只有当后续再次满足启动阈值才释放锁存。
- 当输入源明确为 `DcIn` 且 `IBUS` 连续过高时，策略从 `charging_500ma` 切到 `charging_100ma_dc_derated`；当 `IBUS` 低于恢复阈值足够久后，回到 `charging_500ma`。
- 当 `TPS55288` 总输出功率超过 `5W` 时，策略进入 `blocked_output_over_limit` 并停充。
- 前面板的 charger 电流显示优先取 `BQ25792 IBAT_ADC`，不再把 `ICHG` 设定值伪装成实测电流。

### Edge cases / errors

- `BQ40Z50` 快照不可用、最低单体缺失、`charge_ready=false`、`VBAT_PRESENT=false` 时，策略统一进入 `blocked_no_bms`，停止任何已有的充电保持态。
- `TS_COLD` 或 `TS_HOT` 时，策略进入 `blocked_temp`。
- 输入消失时，策略进入 `blocked_no_input`。
- `TPS55288` 输出功率超过 `5W` 时，策略进入 `blocked_output_over_limit`。
- 双输入同时在线时，input source 记为 `Auto`，不触发 `DC > 3A -> 100mA` 降档规则。
- BMS activation / recovery 的强制 `200mA` 唤醒路径保持独立优先级，不被正常充电策略篡改。

## 接口契约（Interfaces & Contracts）

None。

## 验收标准（Acceptance Criteria）

- Given 空闲且输入/BMS/温度均允许，When `RSOC = 79%` 且 `cell_min_mv >= 3700`，Then 系统进入 `charging_500ma` 并把目标 `ICHG` 写成 `500mA`。
- Given 空闲且输入/BMS/温度均允许，When `RSOC >= 80%` 但 `cell_min_mv = 3690`，Then 系统仍进入 `charging_500ma`。
- Given 当前已在充电，When `RSOC` 与 `cell_min_mv` 回升到阈值上方，Then 系统继续保持充电直到满充。
- Given 已进入满充停充，When 阈值未再次跌破，Then 系统保持 `full_latched`，不得自行重启充电。
- Given 输入源为 `DcIn`，When `IBUS > 3000mA` 连续 `1s`，Then 系统把目标电流降到 `100mA` 并进入 `charging_100ma_dc_derated`。
- Given 系统已处于 `charging_100ma_dc_derated`，When `IBUS < 2700mA` 连续 `5s`，Then 恢复 `500mA`。
- Given 输入源为 `Auto`，When `IBUS > 3000mA`，Then 不应用 DC 独占降档。
- Given `BQ40` 遥测缺失或 `charge_ready=false`，When 进入 charger poll，Then 系统进入 `blocked_no_bms` 并禁止充电。
- Given `TS_COLD=true` 或 `TS_HOT=true`，When 进入 charger poll，Then 系统进入 `blocked_temp` 并禁止充电。
- Given `TPS55288` 总输出功率超过 `5W`，When 进入 charger poll，Then 系统进入 `blocked_output_over_limit` 并禁止充电。
- Given `IBAT_ADC` 可用，When 前面板显示 charger 电流，Then 应显示实测 `IBAT` 而不是目标 `ICHG`。

## 实现前置条件（Definition of Ready / Preconditions）

- 启停口径、DC 独占降档阈值和满充定义均已由主人确认。
- 主线固件已有可用的 `BQ40Z50` 严格快照、`BQ25792` 状态寄存器与前面板 detail 渲染入口。
- `tools/front-panel-preview/` 已可复用前面板真实渲染代码导出 host-side PNG。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: 为 `charge_policy_step()`、`ChargePolicyDerateTracker` 和状态 token 映射补充覆盖。
- Integration tests: `cargo build --release` 必须通过。
- E2E tests (if applicable): None。

### UI / Storybook (if applicable)

- Stories to add/update: None。
- Docs pages / state galleries to add/update: None。
- `play` / interaction coverage to add/update: None。
- Visual regression baseline changes (if any): 使用 `tools/front-panel-preview/` 导出 charger detail 状态图。

### Quality checks

- Lint / typecheck / formatting: `cargo fmt --all`

## 文档更新（Docs to Update）

- `docs/specs/README.md`: 增加本规格索引。
- `docs/charger-design.md`: 后续若需要把“500mA 常规 + DC 100mA 降档”升级为 SoT，再单独同步。

## 计划资产（Plan assets）

- Directory: `docs/specs/eu2b8-bq25792-charge-policy/assets/`
- In-plan references: `![...](./assets/<file>.png)`
- Visual evidence source: maintain `## Visual Evidence` in this spec when owner-facing or PR-facing screenshots are needed.

## Visual Evidence

None.

## 资产晋升（Asset promotion）

None。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 建立 charger policy 规格并登记到 `docs/specs/README.md`
- [x] M2: 在主线 charger runtime 中落地 `80% / 3.70V` 启充、持续到满充、满充锁存停充
- [x] M3: 落地 `DC5025` 独占输入 `3.0A -> 100mA`、`2.7A -> 500mA` 的降档恢复逻辑，并显式写入 `16.8V / 500mA / 100mA`
- [x] M4: 扩充日志、前面板 charger detail 状态、`IBAT_ADC` 实测显示与 host-side preview 场景，并完成 `cargo fmt --all`、`cargo build --release` 与 host-side 预览测试
- [ ] M5: 带着最终视觉证据创建 fast-track PR，并收敛到 merge-ready

## 方案概述（Approach, high-level）

- 使用轻量的 `charge_policy_step()` 状态机统一处理“开始充电、保持充电、满充停充、异常阻断、`VIN` 阻断、输出过功率阻断、DC 独占降档”。
- 将策略锁存与降档计时器保存在 `PowerManager` 内部，避免每轮 poll 只靠瞬时条件抖动。
- 用现有 `dashboard_detail.charger_status / charger_notice` 承载状态 token 与精确状态名，不新增外部协议。
- 使用 `tools/front-panel-preview/` 复用固件真实渲染链路，生成 owner-facing charger detail 预览图。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：`cargo test --lib` 在当前 `xtensa-esp32s3-none-elf` 目标下需要 `std/test`，不能作为本仓库现状下的可执行验证命令；本轮以编译通过和新增单测源码覆盖为主。
- 风险：前面板可见状态 token 由 detail UI 直接显示，若后续需要更短或更统一的文案，需要再做一轮 UI 收敛。
- 假设：`BQ40 FC` 与 `BQ25792 termination_done` 任一成立即可视为满充停充。

## 变更记录（Change log）

- 2026-03-28: 建立规格并按“500mA 常规充电 + DC 独占过流降到 100mA + 80%/3.70V 启停 + 满充锁存”收敛主线策略口径。

## 参考（References）

- `docs/charger-design.md`
- `docs/plan/b3qzy:bq25792-charging-enable/PLAN.md`
- `firmware/src/output/mod.rs`
- `firmware/src/front_panel_scene.rs`
- `tools/front-panel-preview/src/main.rs`
