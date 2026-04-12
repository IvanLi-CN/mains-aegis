# BQ40 `LOCK` 根因锁定与修复（#h6sae）

## 状态

- Status: 部分完成（3/6）
- Created: 2026-04-13
- Last: 2026-04-13

## 背景 / 问题陈述

- 当前主板存在可重复的充电 `LOCK`：包在低于 `90%` 后会恢复，但后续新的充电过程里会再次进入 `LOCK`。
- 已确认的直接链路只有：`SafetyStatus[OC]=1 -> OperationStatus[XCHG]=1 -> CHG FET OFF -> policy_status=LOCK`；这只解释了“怎么锁上”，还没有解释“为什么会再次触发”。
- 当前最大的证据缺口是 `ChargingStatus(0x55)`：runtime 里只有 `block_too_short`，缺少原始块读细节，无法可靠区分 `termination / learning / calibration` 三条修复路径。
- 若继续凭猜测改 `OC` 门限、charge termination 或学习流程，容易把问题掩盖成“暂时不锁”而不是“已找到根因”。

## 目标 / 非目标

### Goals

- 补齐 `0x55` 与相关 lifetime/charger 顶充终止证据，让一次完整充电闭环能够回答“到底是哪个分支导致重入 `LOCK`”。
- 在 `termination / learning / calibration` 三个候选里做唯一分流，并只实施被证据选中的那一个修复分支。
- 完成一次从 `<90%` 解锁基线开始的实机闭环验证，并证明修复后不再重入 `OC/XCHG/LOCK`。
- 把根因判定、参数变更与验证结论固化到 spec / solution / PR 里，收口到“已合并 + 硬件验证通过”。

### Non-goals

- 任何 UI 改动或新增屏幕元素。
- 无证据的 shotgun 调参，尤其是先改 `OC 0x49A9/0x49AB/0x49AD`。
- 把零散屏幕截图、单点日志或主观印象当成根因结论。
- 在证据没有命中 `termination` 分支前，贸然改动 BQ40/BQ25792 的终止参数基线。

## 范围（Scope）

### In scope

- 主固件/诊断辅助层的可观测性补强。
- `BQ40/BQ25792` 顶充终止、学习状态、lifetime 计数器读取与日志化。
- 一次完整 live pack 闭环抓取与单份时间线证据包。
- 基于证据的唯一修复分支实施与复验。
- 与根因结论直接相关的 spec / docs / solution 同步。

### Out of scope

- 修改已冻结的前面板 UI。
- 盲改 `OC` 门限或关闭安全保护。
- 与本问题无关的 charger policy 扩 scope。
- 未经证据命中的多分支并行修复。

## 需求（Requirements）

### MUST

- `ChargingStatus(0x55)` 必须输出原始块读细节：至少包含 `declared_len`、`payload_len`、选中的 plain/PEC 路径、前若干 raw bytes，以及最终解码值/失败原因。
- runtime/report 必须新增以下结构化字段：
  - `ChargingStatus[VCT/NCT/CCR/CVR/CCC/PV/LV/MV/HV/IN/SU/MCHG]`
  - `GaugingStatus[QEN/VOK/REST/FC/FD]`
  - `SafetyStatus[OC]`
  - `OperationStatus[XCHG/CHG/DSG/PCHG]`
  - `0x4312/0x431B/0x43D0/0x43D2/0x43D4/0x43D8`
  - BQ25792 `CHG_STAT/EN_TERM/ITERM/VINDPM_STAT/TREG_STAT/IBAT/VBAT/VBUS`
- 根因判定必须唯一落到 `termination / learning / calibration` 三者之一，并给出决定性证据。
- 修复前后的完整闭环都必须保留可回放证据包。

### SHOULD

- 观测补强尽量走最少刷写路径，优先复用主固件已有 runtime snapshot 节奏。
- 在命中 `termination` 分支时，仅对与证据直接相关的 BQ40/BQ25792 顶充终止窗口做对齐。
- 在命中 `learning` 分支时，保持 `OC` 门限不变，先修学习前提与再基线流程。

### COULD

- 若最终证明是 `learning` 分支，可同步产出一份复用性的 solution 文档，记录“未学习 pack 顶充风险”的工程守则。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 固件周期性采集 BQ40 与 BQ25792 运行态时，必须带出 `0x55` 原始块读细节、学习/lifetime 计数与 charger 顶充终止状态，而不再只输出 `block_too_short`。
- 对 live pack 的闭环抓取必须从 `<90%` 解锁基线开始，持续到“再次 `LOCK`”或“正常满充结束”，并记录关键事件：解锁点、充电启动点、接近满电区间、首次 `VCT/termination_done`、首次 `OC/XCHG`、最终状态。
- 证据分析阶段必须按固定分流规则执行：
  - `termination`：失败循环里 `VCT` 未成立，或 `No Valid Charge Term` 增加，或 BQ25792 未到 `termination_done` 且被 `EN_TERM/ITERM/DPM/TREG` 阻断。
  - `learning`：`VCT`/charger termination 正常，但 `Update Status < 0x06` 且 `Qmax/Ra` 计数停滞，失败与未学习状态强相关。
  - `calibration`：前两者正常，但库仑计/电流链路与实际充电行为不一致。
- 修复阶段只允许实施被证据命中的唯一分支，并在同一套闭环验证里证明修复有效。

### Edge cases / errors

- 若 `0x55` 仍读不到，日志必须明确是 `I2C read_failed`、PEC 失配、还是 plain block 的 declared_len/payload_len 问题；不得再收口成模糊的 `block_too_short`。
- 若实机闭环因为外部输入、电池状态或资源锁被打断，必须保留中间证据并明确 blocker，而不是输出猜测结论。
- 若 evidence 最终指向 `termination`，本 spec 覆盖 `docs/specs/eu2b8-bq25792-charge-policy/SPEC.md` 里“不得改 termination current 校准”的旧非目标；其它分支不触发该放宽。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes） |
| --- | --- | --- | --- | --- | --- | --- | --- |
| BQ40 runtime diagnostics | internal | internal | Modify | None | firmware | defmt monitor / evidence reports | 仅扩充日志字段，不改 UI |
| BQ25792 runtime diagnostics | internal | internal | Modify | None | firmware | defmt monitor / evidence reports | 仅扩充日志字段，不改 UI |
| LOCK evidence report bundle | internal | internal | New | None | firmware + docs | root-cause analysis / PR | 以 monitor + spec 证据为主 |

### 契约文档（按 Kind 拆分）

None

## 验收标准（Acceptance Criteria）

- Given 修复前证据包，When 回看一次失败循环，Then 能明确回答 `0x55` 到底返回了什么、`VCT` 是否成立、`No Valid Charge Term` 是否增加、`Update Status/Qmax/Ra` 是否停滞，以及 BQ25792 是否真的进入 `termination_done` 或被 `DPM/TREG` 阻断。
- Given 根因判定，When 审阅 spec / PR 说明，Then 结论唯一落到 `termination / learning / calibration` 三者之一，且引用了决定性日志/计数器证据，而不是“更像”“怀疑”。
- Given 命中 `termination` 分支并完成修复，When 再跑一次完整充电闭环，Then 会先出现 `VCT` 或 `termination_done`，且整个闭环不再出现 `SafetyStatus[OC]=1`、`XCHG=1`、`policy_status=LOCK`。
- Given 命中 `learning` 分支并完成修复，When 再跑一次完整充电闭环，Then 学习状态出现预期推进（例如 `Update Status` 或 `Qmax/Ra` 计数前进），且整个闭环不再重入 `OC/LOCK`。
- Given 命中 `calibration` 分支并完成修复，When 对比零电流/充电电流与实际行为，Then 电流计量链一致，且闭环中不再因错误积累触发 `OC/LOCK`。
- Given 本轮全部工作完成，When 检查固件与 spec，Then 不存在任何 UI 变更，且若涉及 DF/charger termination 参数改动，final report 已列出改前/改后值与依据。

## 实现前置条件（Definition of Ready / Preconditions）

- `LOCK/OC` 证据模型、三分流规则、修复边界均已冻结。
- 允许在证据命中 `termination` 时调整 BQ40/BQ25792 顶充终止参数，其它分支保持该类参数不变。
- 界面冻结约束已确认，本轮不得再动前面板 UI。
- 可使用 `mcu-agentd` 进行最少刷写与监看，且不触发端口枚举/切换违规操作。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: `cargo test --manifest-path /Users/ivan/Projects/Ivan/mains-aegis/firmware/Cargo.toml --lib`（如 host 侧可用）或针对新增 BQ40/BQ25792 helper 的最小单测。
- Integration tests: `bash /Users/ivan/Projects/Ivan/mains-aegis/firmware/scripts/run-host-unit-tests.sh`
- E2E tests (if applicable): 一次完整 live pack 闭环抓取 + 修复后复验。

### UI / Storybook (if applicable)

- None（UI 冻结，不适用）

### Quality checks

- `cargo test --manifest-path /Users/ivan/Projects/Ivan/mains-aegis/tools/front-panel-preview/Cargo.toml`
- `cargo +esp check --manifest-path /Users/ivan/Projects/Ivan/mains-aegis/firmware/Cargo.toml --features main-vout-19v`

## 文档更新（Docs to Update）

- `/Users/ivan/Projects/Ivan/mains-aegis/docs/specs/README.md`: 新增本 spec 索引行。
- `/Users/ivan/Projects/Ivan/mains-aegis/docs/specs/h6sae-bq40-lock-root-cause/SPEC.md`: 持续回填证据、分流结论、参数变更与最终闭环结果。
- `/Users/ivan/Projects/Ivan/mains-aegis/docs/specs/eu2b8-bq25792-charge-policy/SPEC.md`: 若命中 `termination` 分支，补一条 superseded 说明，收敛旧非目标口径。
- `/Users/ivan/Projects/Ivan/mains-aegis/docs/solutions/**`: 若形成可复用经验，再新增或刷新对应 solution。

## 当前证据快照（Current evidence snapshot）

- `ChargingStatus(0x55)` 已确认是 `H3` 三字节块读而不是 `H4`；runtime 现在能稳定输出 `declared_len/payload_len/raw bytes/source/failure`，不再只有 `block_too_short`。
- `/Users/ivan/Projects/Ivan/mains-aegis/.mcu-agentd/monitor/esp/20260412_224305.mon.ndjson` 里的失败态证据已经满足 `termination` 分流门：
  - `ChargingStatus raw=0x1820`，`VCT=false`，`IN=true`，`HV=true`
  - `SafetyStatus[OC]=1`
  - `OperationStatus[XCHG]=1`，`CHG FET=false`
  - `No Valid Charge Term=2`
  - `Update Status=4`
  - `No Of Qmax Updates=0`
  - `No Of Ra Updates=0`
  - `BQ25792 EN_TERM=true`
  - `BQ25792 ITERM=200mA`
  - `BQ40 Current at EoC=109mA`
- 当前唯一已实施修复分支是 `termination`：
  - 主线固件会把 `BQ40 Current at EoC` 对齐到 `BQ25792 ITERM` 的 `40mA` 步进，当前 pack 的 `109mA` 会下调为 `80mA` 目标。
  - `/Users/ivan/Projects/Ivan/mains-aegis/.mcu-agentd/monitor/esp/20260412_232036.mon.ndjson` 已证明新固件在 runtime 中产生了 `policy_term_target_ma=Some(80)`。
  - `/Users/ivan/Projects/Ivan/mains-aegis/.mcu-agentd/monitor/esp/20260412_233444.mon.ndjson` 已进一步证明该对齐值在 `LOCK` 态也真正落到了 charger 寄存器：同一段日志里同时出现 `policy_term_target_ma=Some(80)`、`iterm_ma=Some(80)`、`applied_iterm_ma=Some(80)` 与 `term_ctrl=Some(58114)`。
  - 同一份最新 monitor 也确认 pack 仍处于 `rsoc_pct=97`、`SafetyStatus[OC]=1`、`XCHG=1`、`VCT=false`、`No Valid Charge Term=2` 的旧失败态，因此当前硬 blocker 已经收窄为：必须先获得一次 `<90%` 解锁基线，才能完成 M3/M5 的闭环复验。

## 计划资产（Plan assets）

- Directory: `docs/specs/h6sae-bq40-lock-root-cause/assets/`
- In-plan references: `![...](./assets/<file>.png)`
- Visual evidence source: maintain `## Visual Evidence` in this spec when owner-facing or PR-facing screenshots are needed.
- If an asset must be used in impl (runtime/test/official docs), list it in `资产晋升（Asset promotion）` and promote it to a stable project path during implementation.

## Visual Evidence

本计划默认以日志/证据包为主；若后续需要 owner-facing 图片，只允许写入 `./assets/`。

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 创建 `LOCK` 根因 spec 并冻结证据模型、三分流规则与修复边界。
- [x] M2: 补齐 `ChargingStatus(0x55)` 原始块读与 lifetime/termination 观测字段，并完成本地验证。
- [ ] M3: 完成一次从 `<90%` 解锁基线开始的 live pack 闭环抓取，产出单份时间线证据包。
- [x] M4: 按证据命中唯一修复分支并完成实现。
- [ ] M5: 完成修复后的 live pack 闭环复验，证明不再重入 `OC/LOCK`。
- [ ] M6: PR 收敛、合并与最终上板确认完成。

## 方案概述（Approach, high-level）

- 先解决证据缺口：把 `0x55` 从“只看见 `block_too_short`”升级成“看得见 raw block、路径与失败原因”的可判读证据。
- 同步补齐 BQ40 lifetime/学习计数与 BQ25792 顶充终止状态，让同一份 monitor 能同时回答“BQ40 认为发生了什么”和“charger 真正做到了什么”。
- 只在证据门明确命中后实施对应修复：`termination` 优先对齐 BQ40/BQ25792 终止窗口，`learning` 修学习前提与再基线，`calibration` 修电流计量链。
- 修复完成后必须使用同一闭环流程复验，避免“单点日志看起来好了”这种伪收口。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：完整充电闭环需要较长硬件观察窗口，可能受外部输入、电池当前状态与串口/监看稳定性影响。
- 需要决策的问题：None（本 spec 已冻结修复边界；实现期按证据门单分支推进）。
- 假设（需主人确认）：已确认可使用最少刷写与 `mcu-agentd` 监看来完成闭环抓取。

## 变更记录（Change log）

- 2026-04-13: 新建 spec，冻结 `LOCK` 根因锁定与修复的证据门、分流规则与快车道收口条件。
- 2026-04-13: 完成 `0x55` 原始块读与 BQ40/BQ25792 低频诊断观测补强，并通过主机侧验证。
- 2026-04-13: 依据 live failure evidence 命中 `termination` 分流门；实施 `BQ40 Current at EoC -> BQ25792 ITERM` 对齐修复，并完成 clean build 上板验证。

## 参考（References）

- `/Users/ivan/Projects/Ivan/mains-aegis/docs/specs/eu2b8-bq25792-charge-policy/SPEC.md`
- `/Users/ivan/Projects/Ivan/mains-aegis/docs/specs/nq7s2-bq40-balance-baseline-and-observability/SPEC.md`
- `/Users/ivan/Projects/Ivan/mains-aegis/docs/manuals/BQ40Z50-R2-TRM/BQ40Z50-R2-TRM.md`
- `/Users/ivan/Projects/Ivan/mains-aegis/docs/datasheets/BQ25792/BQ25792.md`
