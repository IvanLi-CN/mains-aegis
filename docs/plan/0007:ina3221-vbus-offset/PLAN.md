# INA3221 VBUS 读数偏高排查（#0007）

## 状态

- Status: 待设计
- Created: 2026-01-26
- Last: 2026-01-26

## 背景 / 问题陈述

- 现状：`TPS55288` 双路输出功能已稳定（见 Plan #0005），`VOUT` 设定值与万用表/示波器测量一致（误差约 `10mV` 量级）。
- 新问题：`INA3221` 的 `VBUS`（bus voltage）读数异常偏高：
  - 以 `19V` 档为例：固件日志 `vset_mv=19000`，但 `vbus_mv≈20000`（偏高 `~0.5–1.0V`）；且 `vbus_reg` 原始寄存器值与 `vbus_mv` 对应一致，说明不是固件“缩放系数写错”的低级问题。
- 已排除一类常见问题：本板 `IN+/IN-` RC 串阻（`10Ω`）曾误贴为 `10kΩ` 导致读数异常，该问题已修正，**但 `VBUS` 偏高仍存在**。
- 初步怀疑：可能与 **测量参考地（`CHGND` vs `GND`）/测量点（`IN-` 落点）不一致**、输入网络耦合、或 `INA3221` 器件/焊接损伤相关。

## 目标 / 非目标

### Goals

- 明确复现条件与误差量级（含：`VOUT=5/12/15/19V`、空载/负载、两路/单路等矩阵）。
- 通过“同参考点”测量确认 `INA3221` 的 `VBUS` 输入端（`IN-`）对 `CHGND` 的真实电压，与 `VBUS` 寄存器读数之间的差异来源。
- 给出结论：属于（A）测量点/参考不一致、（B）硬件输入网络/布局耦合、（C）器件异常（坏片/ESD/焊接）或（D）固件配置问题（转换时间/平均/通道使能等）。
- 如需修复：给出最小可行方案（固件侧校准/滤波，或硬件侧改动/ECO 建议）与验证方法。

### Non-goals

- 不在本计划内重新设计整套电源监测链路（更换架构/更换芯片）。
- 不在本计划内推进 TPS55288 输出策略与并联控制策略（继续沿用 Plan #0005 既有实现）。

## 范围（Scope）

### In scope

- 上板测量口径与数据记录（以 `U22` 管脚/网络为准，明确参考地）。
- 固件侧最小诊断实验（例如：调整 `INA3221` 转换时间/平均值、临时增加更多原始寄存器打印），仅用于确认问题归因（若需要再进入实现阶段推进）。
- 文档沉淀：把最终结论与测量口径写回 `docs/power-monitoring-design.md` 与相关 bring-up 文档。

### Out of scope

- 任何需要改板打样才能验证的结构性问题，先给出 ECO 建议与预期收益，不强制闭环。

## 需求（Requirements）

### MUST

- 记录“同参考测量”的最小测点（至少包括）：
  - `U22 pin11(IN-1) -> U22 pin3(CHGND)`（OUT-B 通道 `VBUS` 输入）
  - `U22 pin14(IN-2) -> U22 pin3(CHGND)`（OUT-A 通道 `VBUS` 输入）
  - 同时记录：固件 `vbus_reg`、`vbus_mv` 与 `vset_mv`
- 明确本项目对 `VBUS` 的语义：`VBUS` 读数应视为“`IN-` 对 `CHGND`”的电压（与数据手册一致），并与外部仪表测量口径对齐。
- 给出结论时必须包含：复现条件、关键测量数据、以及“下一步建议”（例如更换 U22/复核 CHGND 连接/调整输入 RC/改 INA 配置等）。

## 接口契约（Interfaces & Contracts）

None

## 验收标准（Acceptance Criteria）

- Given TPS 输出设为 `19V` 且外部仪表确认 `U22 IN-` 对 `U22 CHGND` 为 `≈19.0V`
  When 读取 `INA3221 VBUS`（含原始寄存器）
  Then 固件计算的 `vbus_mv` 与仪表一致（目标：误差 `≤100mV`；或给出明确证据说明无法达成的原因与替代策略）

## 实现前置条件（Definition of Ready / Preconditions）

- 主板已可稳定输出（TPS 工作正常）
- 能测量 `U22` 对应管脚到 `CHGND` 的电压（避免以“任意地”代替 `CHGND`）
- 若需要固件诊断：允许一次刷写用于采集数据的诊断固件（不要求自动化测试）

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Build: `cargo build --release`（固件侧若有改动）

## 文档更新（Docs to Update）

- `docs/power-monitoring-design.md`: 增加“VBUS 偏高”的已知问题与测量口径
- `firmware/README.md`: 如需要，补充“VBUS 偏差排查”的测点与日志字段说明

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones）

- [ ] M1: 复现数据矩阵（vset/负载/测点一致性）与关键结论
- [ ] M2: 完成归因（测量口径/硬件/器件/固件配置）
- [ ] M3: 选定并验证最小修复方案（如需要）

## 方案概述（Approach, high-level）

- 首先统一“测量点与参考地”：`VBUS = IN- 对 CHGND`，所有对比必须以该口径测量。
- 通过最小诊断实验区分：DC 偏差（参考/输入偏置） vs 噪声耦合（转换/平均参数敏感）。
- 若最终归因于器件异常：以“更换 U22 + 复测”作为最低成本验证。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：若 `CHGND` 与系统 `GND` 在大电流路径下存在压差，使用“任意地”测量会得到误导性结论。
- 需要决策的问题：若确认是器件/焊接问题，是否直接更换 U22（或整板）作为最快验证路径？
- 假设（需主人确认）：当前 `VBUS` 偏高现象可稳定复现，不是偶发现象。

## 变更记录（Change log）

- 2026-01-26: 新建计划，记录 `VBUS` 偏高问题与排查口径

## 参考（References）

- `docs/plan/0005:tps55288-control/PLAN.md`
- `docs/power-monitoring-design.md`
- `docs/pcbs/mainboard/netlist.enet`
- `docs/datasheets/INA3221/`
