# USB-C PD/PPS Sink 首阶段实现（#hn29u）

## 状态

- Status: 已完成
- Created: 2026-04-07
- Last: 2026-04-23

## 背景 / 问题陈述

- 当前主固件只把 `FUSB302B` 作为 I2C2 自检对象，没有实现 Type-C attach/detach、PD 报文、固定 PDO 选择或 PPS 调压。
- 主 USB-C 口已经与 `BQ25792` 输入链路相连，但运行期仍沿用“默认 5V / 不主动建 PD 合同”的基线，无法稳定利用 `9V/12V/15V/20V` 或 PPS 输入能力。
- 若在未确认 `BQ25792` 输入窗口的情况下盲目请求高压，可能越过板级 `<=20V` 设计边界并损坏硬件，因此协商与运行时都必须有硬性电压上限与异常止损。

## 目标 / 非目标

### Goals

- 为主 USB-C 口实现 `sink-only` 的 USB-C PD 受电能力，首阶段覆盖 Type-C attach/detach、固定 PDO 请求与最小可用的 soft/hard reset 处理。
- 默认暴露 `5V/9V/12V/15V/20V` 固定 PDO 与 `PPS`，并通过 Cargo blacklist features（`no-pd-sink-*` / `no-pps`）精确裁剪能力，便于 A/B 测试与安全回归。
- 在启用 PPS（即未设置 `no-pps`）时，按系统/充电需求动态调整 APDO 电压，优先降低输入侧压差与热损耗，而不是固定顶到 `20V`。
- 运行时严格执行输入安全边界：`BQ25792` 工作输入 `3.6V~24V`、绝对最大 `30V`，但本项目 USB-C 输入按 `<=20V` 设计，协商/运行均不得越过此边界。
- 保持现有前面板 UI 范围不扩张，只补齐必要的 defmt telemetry 与 `SelfCheckUiSnapshot` 真相源接线。

### Non-goals

- 不实现 USB-C source、OTG source、DRP、role swap、data-role swap、VCONN 电源管理。
- 不实现 PD 3.1 EPR 或任何 `>20V` 合同。
- 不改 `CH442E`、USB2/DPDM 路径选择与新的前面板页面。
- 不在本轮改动 `BQ25792` 主线 charge policy 的启动/满充业务规则，但会补齐 USB-C 协商期的禁充门控与恢复条件。

## 范围（Scope）

### In scope

- `firmware/src/usb_pd/`：新增 `pd`、`fusb302`、`sink_policy` 与 sink manager。
- `firmware/src/front_panel.rs`：改为泛型 I2C 设备，支持 `embedded-hal-bus::i2c::RefCellDevice` 共享 I2C2。
- `firmware/src/main.rs`、`firmware/src/bin/test-fw.rs`、`firmware/src/bin/tps-test-fw.rs`：切到共享 I2C2 接线；主固件接入 PD sink manager。
- `firmware/src/bq25792.rs`：补 `VINDPM`/`VAC1`/`VAC2` helper 与输入安全相关解码。
- `firmware/src/output/mod.rs`：接入 USB-PD demand / contract / unsafe-source 保护，并把 PD 结果映射到 `IINDPM/VINDPM` 与 snapshot/log。
- `firmware/Cargo.toml`、`firmware/src/lib.rs`：增加 feature gate、编译期约束与新模块导出。

### Out of scope

- 真实互操作认证、PD analyzer 报告、跨品牌兼容性最终结论；本轮只保留 bench risk 记录。
- 任何与 USB-C 数据通道相关的功能。
- 改动 I2C ISR 模型；中断仍只计数，不做 I2C 事务。

## 需求（Requirements）

### MUST

- 默认构建必须开启 `5V/9V/12V/15V/20V` 固定 PDO 与 `PPS`；调试时通过 `no-pd-sink-5v`、`no-pd-sink-9v`、`no-pd-sink-12v`、`no-pd-sink-15v`、`no-pd-sink-20v`、`no-pps` blacklist features 逐项禁用。
- `no-pps` 关闭 PPS 后，固定 PDO 逻辑仍必须可独立工作；若把全部 fixed PDO 都 blacklist 掉但仍保留 PPS（即未设置 `no-pps`），编译必须失败。
- 当所有 fixed PDO 都被 blacklist 时，构建与运行行为必须保持当前基线，不主动发起 PD 合同。
- 任何 source capability 中 `>20V` 的 fixed PDO / APDO 一律忽略；请求报文绝不请求 `>20V`。
- 运行时一旦测得 USB-C 输入超过 `20V + ADC 容差窗`，必须立即禁充、锁存 `unsafe_source`，直到 detach 才允许清除。
- 固定 PDO 策略必须在已启用的 feature 中选择“满足当前功率需求的最低安全电压”，而不是默认拉到最高档。
- PPS 策略只在未设置 `no-pps` 时启用；目标电压必须跟随系统/充电需求动态调节，并具备迟滞、最小重请求间隔与 keep-alive。
- 只要 USB-C 口处于 attach 后的协商窗口、合同切换窗口、reset/retry 恢复窗口或 source capabilities 变化窗口，charger 都必须保持禁充；只有输入能力被判定为稳定后，才允许恢复充电。
- MCU 冷启动时若 USB-C 已经处于 inherited attach，sink manager 不得主动发送 PD Hard Reset；必须先用非破坏式 RX resume、`Get_Source_Cap`、Soft Reset 与稳定 5V fallback 保住系统供电。
- I2C2 共享后不得破坏前面板初始化、触摸读取或 FUSB302 轮询；中断里仍禁止 I2C 事务。

### SHOULD

- `BQ25792` 运行时应根据 PD 合同更新 `IINDPM` / `VINDPM`，避免 source 电流能力与 charger 输入限制脱节。
- 前面板 snapshot / runtime log 应输出最小必要的 PD 状态：attach、contract、电压、电流、PPS/Fixed、unsafe-source。
- 对 FUSB302 的初始化、TX/RX FIFO、CRC/重试、soft/hard reset 应保持可回放日志，便于 bench 复现。
- PPS 目标电压应优先贴近 `充电电压或电池电压 + 安全裕量`，以降低从高压固定档直接降压带来的热损耗。
- 稳定性判定应区分“过渡 5V”与“稳定 5V”：协商前/重协商中的默认 5V 只用于系统保底，不得被视为可立即放开正常充电的稳定输入。

### COULD

- 后续在不改 feature 口径的前提下，把 sink capability 与 request trace 暴露给更细的调试页。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 当默认 fixed PDO 能力未被全部 blacklist，且 USB-C source attach 成功时，固件应通过 `FUSB302B` 进入 sink attach，识别极性，开启 PD RX/TX，并等待 `Source_Capabilities`。
- 收到 source capabilities 后，先过滤掉所有 `>20V` 的 fixed PDO / APDO，再根据 feature 生成的本地能力表和当前充电需求挑选候选合同。
- 固定 PDO 模式下，策略以“满足功率需求的最低启用固定电压”为优先级，并按 source advertised current 与本地输入预算生成 RDO。
- PPS 模式下，若 source 提供合法 PPS APDO 且未设置 `no-pps`，则目标电压按 `system_target / charge_target + headroom` 计算，随后 clamp 到 `source APDO window`、`<=20V` 与本地输入预算内，再按迟滞/节流/keep-alive 条件决定是否重请求。
- 当 source 接受请求并发送 `PS_RDY` 后，合同状态更新为 active；主 charger runtime 随后把合同映射到 `IINDPM/VINDPM` 并刷新 telemetry。
- USB-C 充电 gate 应遵循三态：`InputTransient`（attach/重协商/reset 中，禁充）、`InputStable5V`（确认无 PD 合同但 5V 已稳定，可按 5V 上限充电）、`InputStableContract`（PD/PPS 合同已稳定，可按合同充电）。
- 一旦收到新的 `Source_Capabilities`、`soft reset`、`hard reset`、`retry fail` 或合同丢失，charger 必须立即回到 `InputTransient` 并停充；只有新的稳定输入能力确认后，才允许恢复充电。
- detach、hard reset、soft reset、retry fail 或 source 重新广播 capabilities 时，sink manager 应清除旧合同并回到等待协商状态。

### Edge cases / errors

- 若 source 只广告 `>20V` 能力，则固件不得请求任何高压合同，退回默认安全行为（等待默认 5V / 不建立高压合同）。
- 若 source capabilities 里没有任何与 feature 兼容、且满足功率需求的 fixed PDO / APDO，则不得发送超出约束的 request；应回落到稳定 5V 输入窗口，并把 `IINDPM/VINDPM` 约束在 source 广告的 5V 电流上限内；若当前拿不到 5V 广告电流，则必须使用保守默认限流而不是恢复旧高压合同的输入限流。
- 若 `FUSB302` 未装配、初始化失败或运行期轮询失败，则固件必须清空旧 PD 合同并退回 plain 5V 受电路径；不得因为 `usb_pd_enabled` 仍为 true 就永久阻塞 USB-C 5V 充电。
- 若 PPS 当前合同有效，但需求变化未超过迟滞窗或未达到最小重请求间隔，则不得反复刷请求。
- 若 FUSB302 RX FIFO 收到 hard reset / soft reset / retryfail，必须清空 FIFO、重置协商状态并准备重新拉起协商。
- 若 FUSB302 attach 结果异常（非 `SNK1/SNK2`）或运行中 VBUS 消失，则合同与 unsafe latch 以 detach 语义清零。
- 若运行时检测到 `unsafe_source`，charger 必须立即停充并拒绝继续高压协商，直到 detach。
- 若 inherited attach 后迟迟没有 `Source_Capabilities`，恢复阶梯必须保持供电优先：记录 Hard Reset 被抑制，先请求 source caps，再尝试 Soft Reset，随后按稳定 5V fallback 放开受限充电；只有观察到可靠 physical detach/replug 后，才允许恢复主动 Hard Reset 策略。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes） |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `no-pd-sink-*` / `no-pps` Cargo features | build config | internal | New | None | firmware | firmware build matrix | 默认全开、按 blacklist 裁剪 capability |
| `esp_firmware::usb_pd` | Rust module | internal | New | None | firmware | main firmware | sink policy + FUSB302 driver |
| `PowerManager::usb_pd_demand/update_usb_pd_state` | Rust API | internal | New | None | firmware | main loop | PD 与 charger runtime 桥接 |
| `FrontPanel<I2C>` | Rust type | internal | Modify | None | firmware | main/test-fw/tps-test-fw | I2C2 共享总线 |

### 契约文档（按 Kind 拆分）

None。

## 验收标准（Acceptance Criteria）

- Given 未设置 `no-pd-sink-5v` 且 source 广告 `5V/9V/20V`，When 当前功率需求可由 `5V` 满足，Then 固件只请求 `5V` 合同。
- Given 默认 fixed 能力全开，When 功率需求需要高于 `5V` 档才能满足，Then 固件选择满足需求的最低固定电压档，而不是固定请求 `20V`。
- Given 未设置 `no-pps` 且 source 广告合法 PPS APDO，When 系统/充电需求变化，Then 固件会在迟滞、最小重请求间隔与 keep-alive 约束内调整 PPS 请求电压。
- Given source 广告中存在 `>20V` fixed PDO / APDO，When sink 解析 capability，Then 这些能力必须被忽略，且请求报文中不得出现 `>20V` 电压。
- Given 运行时测得 USB-C 输入超过安全窗，When PD/charger runtime 处理该样本，Then charger 立即停充，`unsafe_source` 锁存为 true，直到 detach 才清除。
- Given 全部 fixed PDO 都被 blacklist 且未设置 `no-pps`，When 编译固件，Then 编译失败并给出明确的 feature 约束错误。
- Given attach 后仍处于协商/重协商窗口，When charger runtime 评估 `allow_charge`，Then 必须保持禁充，不得把过渡中的默认 5V 视作稳定输入。
- Given 多口电源触发功率重分配并重新广播 source capabilities，When 旧合同失效，Then charger 必须先停充，待新合同或稳定 5V 再恢复。
- Given 所有 fixed PDO 都被 blacklist，When 构建与运行主固件，Then 行为与当前基线保持一致，不主动建立 PD 合同。
- Given 前面板与 FUSB302 共用 I2C2，When 前面板初始化、触摸轮询与 PD IRQ 轮询同时运行，Then 不得产生总线 ownership 冲突或互锁死。

## 实现前置条件（Definition of Ready / Preconditions）

- `BQ25792` 输入电压边界已确认：工作输入 `3.6V~24V`、绝对最大 `30V`、`VINDPM` 范围 `3.6V~22V`；板级 USB-C 输入上限按 `20V` 执行。
- `FUSB302B` 在当前硬件上挂接于 I2C2 且 `INT_N` 已接到 `GPIO7`，中断模型只做计数。
- feature 名称、安全红线、scope 与验收口径已由本规格冻结。
- 本轮允许修改 `firmware/` 与 `docs/specs/`，并按 fast-track 收口到“merge + cleanup”。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: 覆盖 capability 生成、PDO/APDO 过滤、固定 PDO 选择、PPS 目标计算、RDO 编码与 unsafe-voltage 判定。
- Integration tests: `cargo check` 覆盖 `无 PD feature`、`5V-only`、`5/9/12/15/20`、`5/9/12/15/20 + pps`、`pps-only(应失败)`。
- E2E tests (if applicable): 台架最少验证非 PD 5V 口、固定 PDO 适配器、PPS 适配器、`>20V` capability source、attach/detach 与 hard-reset 恢复。

### UI / Storybook (if applicable)

- Stories to add/update: None。
- Docs pages / state galleries to add/update: None。
- `play` / interaction coverage to add/update: None。
- Visual regression baseline changes (if any): None。

### Quality checks

- Lint / typecheck / formatting: `cargo fmt --all`、相关 feature matrix `cargo check`、`cargo test --manifest-path firmware/host-unit-tests/Cargo.toml` 与 `bash firmware/scripts/run-host-unit-tests.sh`。

## 文档更新（Docs to Update）

- `docs/specs/README.md`: 登记本 spec，并在收口时同步状态。
- `docs/charger-design.md`: 同步“USB-C 协商/重协商期间先禁充，稳定后再恢复”的固件门控口径。

## 计划资产（Plan assets）

- Directory: `docs/specs/hn29u-usb-c-pd-sink-pps/assets/`
- In-plan references: `![...](./assets/<file>.png)`
- Visual evidence source: maintain `## Visual Evidence` in this spec when owner-facing or PR-facing screenshots are needed.

## Visual Evidence

本轮无 UI 视觉资产要求。

## 资产晋升（Asset promotion）

None。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 新建 spec、登记索引，并冻结 feature / 安全边界 / 验收口径
- [x] M2: 将 `FrontPanel` 改为共享 I2C2 的泛型设备，并在主固件与两个测试固件完成接线迁移
- [x] M3: 新增 `usb_pd` 模块，完成 feature 驱动 capability 生成、固定 PDO / PPS 纯逻辑与 FUSB302 薄驱动骨架
- [x] M4: 将 PD sink manager 接入主循环与 `PowerManager` / `BQ25792` 运行时，补齐 `IINDPM/VINDPM` 与 unsafe-source 保护
- [x] M5: 完成测试、feature 编译矩阵、spec sync、提交/推送/PR 与 review-loop 收口

## 方案概述（Approach, high-level）

- 采用“薄 PHY + 纯策略”分层：`fusb302` 只负责寄存器、attach/detach、FIFO TX/RX 与 reset；`sink_policy` 负责能力过滤、请求选择、PPS 目标电压与安全边界。
- 利用 `embedded-hal-bus::i2c::RefCellDevice` 在主循环内共享 I2C2，避免把 I2C 事务带进 ISR。
- 通过 `PowerManager` 暴露最小 USB-PD demand / state 桥接口，减少对现有 charger policy 主体的侵入，同时把安全锁存、`IINDPM/VINDPM` 编程与 snapshot/log 收拢到 power runtime。
- feature 未开启时保持 legacy path，不让 PD 实现污染默认构建。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：`FUSB302B` datasheet 对 spec revision / GoodCRC 的描述更偏 PD2.0，PPS 互操作需真实台架确认；本轮先把 PPS policy 与报文路径接齐，并在 PR 中显式记录互操作风险。
- 风险：多口电源在功率重分配时可能触发 source capabilities 重广播、reset 或短暂回落默认 5V；USB-C charge gate 若实现不完整，容易在旧合同失效窗口里误充。
- 风险：若 MCU 由同一 USB-C Source 供电，冷启动继承 attach 期间主动 Hard Reset 会让 Source 短暂移除 VBUS，从而造成 MCU brownout/reset loop；该路径必须默认抑制 Hard Reset，并通过 EEPROM breadcrumb 记录 `boot_inherited_attach` / `hard_reset_inhibited` / recovery TX 事件以支持无日志复盘。
- 风险：冷启动或 no-contract 恢复窗口若无法得到足够高的 `usb_pd.tick()` 服务频率，协商超时会被主循环其它任务拖长，导致热插拔恢复时间抖动；当前通过协商优先窗口已把 reset 基线压到约 `1.67s`，后续如再扩展主循环负载，需重新验证该窗口仍能保证秒级恢复。
- 风险：当前完成态依赖 plain-serial + EEPROM breadcrumb 双证据链；若后续改动再次让 monitor 只停在 `boot: stage=main_loop_enter`，必须先修复观测链再判断 PPS 恢复行为。
- 假设：USB-C 输入安全窗按 `20.5V`（`20V + 500mV ADC 容差窗`）执行；若后续硬件校准数据表明需要更窄或更宽，允许在不改 feature 口径的前提下微调实现常量。

## 当前实机状态（2026-04-23）

- 当前 `hn29u` 已完成闭环：实机冷启动与真实 USB 热插拔后，`PPS` 都能在秒级恢复，不再出现“随机卡在 `CAP? + 5V` 或需要十几秒以上才恢复”的主故障。
- 最新板上证据：
  - reset 基线日志：`/Users/ivan/Projects/Ivan/mains-aegis/.mcu-agentd/monitor/esp/20260422_204331_570.mon.ndjson`
    - `2026-04-22T20:43:34.370942Z attach`
    - `2026-04-22T20:43:36.036427Z contract active kind=pps`
    - `attach -> PPS ≈ 1.67s`
  - 主人实机热插拔复测：已确认“重新插拔已经能秒协商成功”，不再出现此前 3s / 10s / 45s 的双稳态恢复。
- 最终根因收敛为两层：
  - 协议恢复正确性：`partial RX` 被过早读取/flush、`retry/hard reset` 与 `missing source caps` 恢复链交叉打断，导致同一条会话里不断重复 `Get_Source_Cap / reset / rearm`。
  - 主循环调度：`attached && contract=None` 窗口里，`usb_pd.tick()` 之前被 `power.tick()`、BMS/charger/UI 轮询拖慢，导致明明配置了 `400ms` 的恢复超时，却经常要到 `~1s` 之后才真正执行。
- 最终修复由两部分组成：
  - 协议层：只在完整帧 ready 后读取 RX；`partial RX + hard reset` 先 defer；`no-contract` 恢复维持 `PD_RESET + 等 Source Caps`，避免把协议层 reset 当作物理 detach 乱拆。
  - 调度层：在 `/Users/ivan/Projects/Ivan/mains-aegis/firmware/src/main.rs` 为 `attached && contract=None` 增加约 `450ms` 的协商优先窗口，优先连续服务 `usb_pd.tick()` 与 IRQ 收敛，再回到 `power.tick()` 等其它周期任务。
- 结果：`SOURCE_CAPS_WAIT_TIMEOUT_MS = 400ms` 现在能按预期生效，reset 基线已从约 `2.41s` 压到约 `1.67s`，真实热插拔也回到秒级恢复。

## 变更记录（Change log）

- 2026-04-23: 完成最终 hotplug PPS 恢复收口。根因最终确认还包括 `attached && contract=None` 窗口里 `usb_pd.tick()` 服务频率不足，导致恢复超时被主循环其它任务拖长；通过补齐 partial-RX / hard-reset 恢复正确性，并在主循环中为 no-contract 协商增加优先窗口后，reset 基线已稳定到约 `1.67s`，实机热插拔也恢复到秒级 `PPS`。
- 2026-04-22: 完成 hotplug PPS 恢复闭环。最终根因定位为 FUSB302 自动协议复位与固件恢复状态机互相打架、fresh attach 后继续处理旧 IRQ snapshot，以及 `missing source caps` 恢复策略缺少稳定升级路径；修复后实机热插拔 `1.0s` 内恢复到 `PPS`，冷启动插线基线约 `25.28s` 自动恢复到 `PPS`。
- 2026-04-22: 重新打开 hotplug PPS 恢复问题。此前“热插拔已稳定恢复到 `PPS`”的结论被后续实机复测推翻：当前同一条 PPS 电源线上仍会出现“有时数秒恢复、有时长期卡在 `CAP?`”的双稳态现象；规格状态回退为 `部分完成（4/5）`，后续必须先完成稳定恢复闭环，再讨论时延优化。
- 2026-04-21: 一度观察到连续多次实机拔插可自动回到 `PPS`，后续实现/回归证明该结论不足以支撑 closeout；该记录保留为阶段性现象，不再视为最终结论。
- 2026-04-08: 已继续收敛 merge-proof review，补齐“非充电态仍计入系统负载预算”“PD state 先于 charger tick 生效”“合同丢失时强制恢复旧 `VINDPM/IINDPM`”三项修正，并同步规格说明。
- 2026-04-08: 已根据 merge-proof review 修正 spec revision 跟随、无可用 PD 合同时的稳定 5V 回落，以及 WAIT/REJECT 后的旧合同 charge gate 恢复；规格与最新实现重新对齐。
- 2026-04-08: 已同步 host-unit-tests allowlist 与 closeout 文档，确认 `usb_pd` 模块测试覆盖纳入 host audit，规格与实现重新对齐为 merge-ready。
- 2026-04-08: 已完成默认全开 + blacklist feature、USB-C 协商/重协商禁充 gate、PPS keep-alive、合同保持与真机验证；状态更新为 `已完成`。
- 2026-04-08: 规格同步到默认全开 + blacklist feature 口径，并补充“USB-C 协商/重协商期间禁充，输入稳定后再恢复”的 charge gate 要求与验收项。
- 2026-04-07: PR #62 已创建，收口目标切换为 review-loop 后的可审阅态；台架风险保持显式记录。
- 2026-04-07: 已完成 `usb_pd` 模块、I2C2 共享、`BQ25792` 输入限制 helper、主循环/`PowerManager` 接线，以及 host-unit-tests + feature matrix 本地验证；状态更新为 `部分完成（4/5）`，等待 PR/review-loop 收口。
- 2026-04-07: 初版规格创建，冻结 USB-C PD/PPS sink v1 的范围、feature、边界与验收标准。

## 参考（References）

- `docs/datasheets/BQ25792/BQ25792.md`
- `docs/charger-design.md`
- `docs/datasheets/FUSB302B/FUSB302B.md`
