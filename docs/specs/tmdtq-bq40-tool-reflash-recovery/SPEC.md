# BQ40 工具链 reflash / recovery 收敛（#tmdtq）

## 状态

- Status: 部分完成（4/5）
- Created: 2026-03-06
- Last: 2026-03-06

## 背景 / 问题陈述

- `tools/bq40-comm-tool` 已经具备 `diagnose / recover / verify` 三段式流程，也已有 `--force-min-charge` CLI 入口，但当前工具内强制唤醒路径与已验证台架参数不一致。
- 现有工具固件在 `force_min_charge` 分支里仍使用最小电流写法（`ICHG=50mA`、`IINDPM=100mA`，且不改 `VREG`），与排障笔记里已经收敛出的唤醒参数 `16.8V / 200mA / 500mA` 不一致，导致“CLI 开了强制唤醒”与“台架真正需要的唤醒动作”之间存在语义落差。
- `summary.json` 里的 `rom_events.flash_done` 只应认 `stage=probe_rom_flash_done`；单独出现 `stage=rom_flash_done` 仍不足以证明工具已经确认回到 firmware mode。
- 本轮实现需要把验收范围收紧到 `tools/bq40-comm-tool` 路径，且台架条件固定为“无电池 + 外部输入供电”；验收只覆盖 `--recover if-rom`，不把 `--recover force` 作为成功标准。

## 目标 / 非目标

### Goals

- 让 `tools/bq40-comm-tool/bin/run.sh` 与 `bin/build.sh` 的 `--force-min-charge` 真正透传到工具固件构建路径，并成为 `diagnose` / `recover` 的可验证行为。
- 在工具固件的强制唤醒路径里恢复已验证的唤醒参数：`VREG=16.8V`、`ICHG=200mA`、`IINDPM=500mA`。
- 收紧 `summary.json` / `summary.md` 中 `rom_events.flash_done` 的语义：只有真正完成 ROM flash reflash 才能置为 `true`。
- 把 diagnose / recover / verify 的操作、报告、文档与验收统一约束在 `tools/bq40-comm-tool` 目录内，不依赖主固件路径做验证兜底。
- 固化本轮 bench 前提：无电池、仅外部输入供电；`if-rom` 是唯一纳入验收的恢复策略。

### Non-goals

- 不修改主工程根目录 `firmware/` 的 `force-min-charge` 语义，也不把主固件自检流程纳入本轮验收。
- 不把 `--recover force` 强刷路径作为本轮验收目标。
- 不新增 BQ40 ROM 镜像资产，也不变更 recovery image traceability。
- 不覆盖“接电池”场景下的充电策略重新定义；本轮只针对无电池外部供电台架收敛。

## 范围（Scope）

### In scope

- `tools/bq40-comm-tool/bin/run.sh`：live 命令参数校验与 `--force-min-charge` 透传。
- `tools/bq40-comm-tool/bin/build.sh`：构建 feature 组装与 `force-min-charge` feature 透传。
- `tools/bq40-comm-tool/firmware/src/output/mod.rs`：强制唤醒参数、日志与恢复判定语义。
- `tools/bq40-comm-tool/bin/report_parser.py`：`flash_done` 的解析与 summary 输出语义。
- `tools/bq40-comm-tool/README.md`、`tools/bq40-comm-tool/docs/operations.md`、`tools/bq40-comm-tool/docs/troubleshooting-notes.md`、`tools/bq40-comm-tool/docs/recovery-safety.md`：命令合同、bench 条件与恢复语义说明同步。

### Out of scope

- 根目录 `firmware/` 的功能验证、日志或验收。
- 新增独立 GUI / Web 诊断界面。
- 任何依赖“换电池/接电池再验证”的操作流程。

## 需求（Requirements）

### MUST

- `diagnose` 与 `recover` 必须接受 `--force-min-charge true|false`，并在工具本地构建路径中稳定透传为 `force-min-charge` feature。
- `verify` 必须继续拒绝 `--force-min-charge`、`--recover`、`--flash`，保持离线复算语义纯净。
- 工具固件在 `force_min_charge=true` 且外部输入存在时，必须显式写入并记录 `VREG=16800mV`、`ICHG=200mA`、`IINDPM=500mA`。
- `rom_events.flash_done=true` 只能由 `stage=probe_rom_flash_done` 触发；单独出现 `stage=rom_flash_done` 不能把它置为 `true`。
- 本轮 diagnose / recover / verify 的验收命令、日志来源、报告目录必须全部位于 `tools/bq40-comm-tool` 路径语义下。
- 验收只覆盖 `--recover if-rom`：若未观测到 ROM signature，则 `flash_attempted=false`、`flash_done=false`；若观测到 ROM signature 且执行完整 reflash，则二者才允许按实际完成情况置位。

### SHOULD

- 强制唤醒日志应能直接暴露最终生效的 `vreg_mv` / `ichg_ma` / `iindpm_ma`，避免只能靠寄存器侧推。
- 文档应显式声明 bench 前提为“无电池 + 外部输入供电”，避免后续误把接电池路径当作必需条件。

### COULD

- 在 `summary.md` 中补一行短说明，帮助人工区分“检测到 ROM 并退出”与“真正完成 flash reflash”。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 运行 `./bin/run.sh diagnose --mode canonical --duration-sec 120 --force-min-charge true` 时，工具链在 `tools/bq40-comm-tool` 内完成 build / flash / monitor / report，且强制唤醒参数按 `16.8V / 200mA / 500mA` 生效。
- 运行 `./bin/run.sh recover --mode dual-diag --duration-sec 155 --recover if-rom --force-min-charge true` 时，仅在检测到 ROM signature 后进入 ROM reflash；未检测到时不得把报告写成 flash 已完成。
- 运行 `./bin/run.sh verify --mode canonical --duration-sec 120 --monitor-file <path>` 时，应基于工具路径生成的 monitor log 离线复算出与在线一致的 ROM 事件语义。

### Edge cases / errors

- 若外部输入不存在、温度保护不允许或充电器写寄存器失败，工具不得宣称已成功应用唤醒参数。
- `canonical` 模式下若日志触达 `0x16`，仍按现有失败语义处理；本轮不放宽该约束。
- `dual-diag` 是本轮受支持的 recover 地址模式；`--recover force` 仍不属于本轮通过标准，也不得反向污染 `if-rom` 报告语义。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes） |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `./bin/run.sh` live CLI | CLI | internal | Modify | `None (this SPEC)` | tools/bq40-comm-tool | 手工 bench / 自动化脚本 | `diagnose` / `recover` 透传 `--force-min-charge`；`verify` 继续拒绝 |
| `summary.json` / `summary.md` | File format | internal | Modify | `None (this SPEC)` | tools/bq40-comm-tool | bench 报告消费者 / 后续自动化 | 收紧 `rom_events.flash_done` 语义 |

### 契约文档（按 Kind 拆分）

- None；本轮接口改动直接冻结在本 SPEC，不新增单独 `contracts/` 文档。

## 验收标准（Acceptance Criteria）

- Given 台架条件为“无电池 + 外部输入供电”，且从 `tools/bq40-comm-tool` 目录执行，
  When 运行 `./bin/run.sh diagnose --mode canonical --duration-sec 120 --force-min-charge true`，
  Then 工具链完成 live 流程并产出报告，且日志/报告可确认强制唤醒参数为 `VREG=16800mV`、`ICHG=200mA`、`IINDPM=500mA`。

- Given 同一台架条件下未检测到 ROM signature，
  When 运行 `./bin/run.sh recover --mode dual-diag --duration-sec 155 --recover if-rom --force-min-charge true`，
  Then `summary.json` 中 `rom_events.detected=false`、`flash_attempted=false`、`flash_done=false`，且本轮验收不需要、也不允许用 `--recover force` 兜底达成通过。

- Given 同一工具路径下检测到 ROM signature（`0x9002`），
  When 运行 `./bin/run.sh recover --mode dual-diag --duration-sec 155 --recover if-rom --force-min-charge true` 并完成完整 ROM flash reflash，
  Then `summary.json` 中 `rom_events.detected=true`、`flash_attempted=true`、`flash_done=true`；若只有 `stage=rom_flash_done rsoc_after=0x9002` 或其他未确认回到 firmware mode 的阶段打点，则 `flash_done` 仍为 `false`。

- Given 原始芯片样本持续停留在“既非正常 SBS、也非可见 ROM”的阻断态，
  When 在同板同工具链下更换 BQ40Z50 样本后，`tools/bq40-comm-tool` 已能完成 `ROM 检测 -> 重刷 -> 退出 ROM`，并在无电池偏置条件下稳定给出 `Voltage()/CellVoltage1()` 为几十 mV、`CellVoltage2..4()` 为 `0 mV` 的悬空签名，
  Then 本轮软件任务可判定为“工具链与诊断路径有效，且已把原始样本收敛为疑似硬损坏器件”，不再要求软件侧继续把该损坏样本恢复到应用态通信通过。
  Note 该条是“人工判定工具链有效”的验收口径：`report_parser.py` 仍会把 `Voltage()<2500mV` 的样本视为 invalid，因此 `summary.json` 的 `verdict.pass` 预期为 `false`（这不是回归，而是避免把悬空偏置误判为正常通信）。

- Given 已存在由 `tools/bq40-comm-tool` live 流程生成的 `.mon.ndjson`，
  When 运行 `./bin/run.sh verify --mode canonical --duration-sec 120 --monitor-file <that-file>`，
  Then 离线复算出的 `rom_events` 与在线 summary 一致，且命令仍拒绝 `--flash`、`--recover`、`--force-min-charge`。

- Given 任一纳入验收的 canonical 模式运行，
  When 检查 monitor log 与 summary，
  Then 不出现 `addr=0x16` 触达，且验收记录只引用 `tools/bq40-comm-tool` 路径，不引用根目录 `firmware/` 的替代性验证结果。

## 实现前置条件（Definition of Ready / Preconditions）

- Bench 前提已冻结：无电池、外部输入供电稳定。
- `if-rom` 是唯一纳入验收的恢复策略，`force` 不进入本轮成功口径。
- 强制唤醒目标参数已冻结：`16.8V / 200mA / 500mA`。
- 工具路径边界已冻结：diagnose / recover / verify 只认 `tools/bq40-comm-tool`。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Live bench：
  - `./bin/run.sh diagnose --mode canonical --duration-sec 120 --force-min-charge true`
  - `./bin/run.sh recover --mode dual-diag --duration-sec 155 --recover if-rom --force-min-charge true`
- Offline verify：
  - `./bin/run.sh verify --mode canonical --duration-sec 120 --monitor-file <recover-or-diagnose-log>`
- 若具备 ROM signature 样本，需要至少一组“真实 flash_done=true”的 recover 报告；若无 ROM signature 样本，至少需要证明 `if-rom` 路径不会误报 `flash_done=true`。

### Quality checks

- Shell 语法检查：`bash -n tools/bq40-comm-tool/bin/build.sh tools/bq40-comm-tool/bin/run.sh`
- Python 语法检查：`python3 -m py_compile tools/bq40-comm-tool/bin/report_parser.py`
- 工具固件构建：从 `tools/bq40-comm-tool/firmware` 执行与 `force-min-charge` 相关的 release build 校验。

## 文档更新（Docs to Update）

- `tools/bq40-comm-tool/README.md`: 补齐 `--force-min-charge` 的 live 路径语义、bench 前提与 `flash_done` 解释。
- `tools/bq40-comm-tool/docs/operations.md`: 收紧 `recover if-rom` 验收与 live/verify 边界。
- `tools/bq40-comm-tool/docs/troubleshooting-notes.md`: 对齐 `16.8V / 200mA / 500mA` 为工具内最终唤醒参数，并声明无电池台架前提。
- `tools/bq40-comm-tool/docs/recovery-safety.md`: 明确 `flash_done` 只对应真实 ROM reflash 完成。

## 计划资产（Plan assets）

- None。

## Visual Evidence (PR)

本规格暂不预置 PR 证据图；实现阶段如需截图，只能补充真实 bench / report 证据。

## 资产晋升（Asset promotion）

- None

## 实现里程碑（Milestones / Delivery checklist）

- [ ] M1: `run.sh` / `build.sh` 的 `--force-min-charge` live 参数合同与透传路径收敛完成。
- [ ] M2: 工具固件强制唤醒参数恢复为 `16.8V / 200mA / 500mA`，并能在无电池外部供电台架上留下可核对日志。
- [ ] M3: `report_parser.py` 与相关日志阶段语义收紧完成，`flash_done` 仅在真实 ROM flash reflash 完成时置位。
- [ ] M4: `tools/bq40-comm-tool` 文档、操作手册与离线 verify 口径同步完成。

## 方案概述（Approach, high-level）

- 以工具路径为唯一实现与验收入口，避免主固件路径提供“看似通过、实际偏题”的替代信号。
- 用“CLI 参数合同 -> 固件唤醒参数 -> ROM 事件解析 -> 文档同步”四段式收敛，确保 live / offline 语义一致。
- 先修正 `if-rom` 的可验证安全路径，再把 `force` 继续留在非验收分支，避免把诊断特权路径误升格为默认成功标准。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：若台架外部输入不稳定，可能把“参数已恢复”误判成“逻辑未生效”。
- 风险：历史日志若只含 `stage=rom_flash_done` 而没有 `stage=probe_rom_flash_done`，需要谨慎区分旧报告与新语义。
- 开放问题：原始问题芯片为何损坏仍未查明。基于“同板更换芯片后工具链可完成 ROM 检测/重刷/退出 ROM，而原芯片始终停在阻断态”的对照结果，当前更合理的结论是 **原始样本疑似硬损坏**，而非工具链仍有主路径故障。
- 开放问题：是否已有可重复触发的 ROM signature 样本用于验证 `flash_done=true` 正例；若没有，需要至少保底验证“不误报 true”。
- 假设：`tools/bq40-comm-tool/docs/troubleshooting-notes.md` 中记录的 `16.8V / 200mA / 500mA` 仍是当前 bench 的目标参数。

## 变更记录（Change log）

- 2026-03-06: 初始化规格，冻结工具路径边界、bench 前提、`if-rom` 验收口径与里程碑。
- 2026-03-06: 已完成工具侧 `--force-min-charge` / `flash_done` 语义修复，并新增 `--probe-mode mac-only`、missing reprobe、以及按地址细化的 `bms_diag_word` 诊断；最新实板证据表明 `0x0B` 只剩裸读 `0xFF` 伪应答、`0x16` 完全 NACK，仍属阻断态。
- 2026-03-06: 新增 boot 后 `0/800/1600 ms` staged wake probe，并在 `if-rom` 路径上复测；结果表明即使在早期唤醒窗口内，`0x0B` 依旧命令字节 NACK、`0x16` 依旧地址 NACK，ROM 恢复仍未触发。
- 2026-03-07: 在 `probe_rom_exit` 失败时追加 `0x0F00` / `0x0033`（含 PEC）盲打 ROM 入口诊断，并为 `monitor` 增加首轮 reset 失败自动回退；结果显示 ROM 入口写法在 `0x0B` 上全部 data-NACK、在 `0x16` 上全部 address-NACK，仍无法进入可见 ROM。
- 2026-03-09: 在工具固件日志中补充 `CellVoltage1..4()` 诊断；无电池偏置样本显示 `CellVoltage1≈27~51 mV`、`CellVoltage2..4=0 mV`，与 `Voltage()` 的几十 mV 浮动一致，可作为悬空偏置签名。结合更换芯片后的对照结果，本任务的软件收口口径调整为“工具链有效并可识别原始样本疑似硬损坏”，而不是要求软件恢复已损坏芯片。

## 参考（References）

- `tools/bq40-comm-tool/README.md`
- `tools/bq40-comm-tool/docs/operations.md`
- `tools/bq40-comm-tool/docs/troubleshooting-notes.md`
- `tools/bq40-comm-tool/docs/recovery-safety.md`
