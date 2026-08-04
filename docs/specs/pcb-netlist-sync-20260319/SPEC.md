# PCB netlist sync (2026-03-19)

> 当前有效规范以本文为准；实现覆盖与当前状态见 `./IMPLEMENTATION.md`，关键演进原因见 `./HISTORY.md`。

## 背景 / 问题陈述

- 仓库中的主板网表 `docs/pcbs/mainboard/netlist.enet` 落后于 2026-03-19 重新导出的实际硬件版本。
- 同一轮核对中，前面板导出文件与仓库内 `docs/pcbs/front-panel/netlist.enet` 已确认完全一致，不需要内容改动。
- 主板 README 与 TPS55288 相关合同文档引用了旧网表事实，包括：
  - 主板 `FPC1` 的旧 pin 映射；
  - 已不存在的 `J1/J2/J3` 输出跳线与 `VOUT_A/VOUT_B` 路由；
  - `TPS55288` 输出网名仍写成 `VOUT_TPSA/VOUT_TPSB`，未反映当前共享输出节点 `VOUT_TPS`。

## 目标 / 非目标

### Goals

- 将 `docs/pcbs/mainboard/netlist.enet` 整文件同步到 `/Users/ivan/Downloads/Netlist_Schematic1_2026-03-19.enet.enet`。
- 确认 `docs/pcbs/front-panel/netlist.enet` 与 `/Users/ivan/Downloads/Netlist_Schematic2_2026-03-19.enet.enet` 保持零差异。
- 把主板 README 与 TPS55288 配置合同同步到新网表事实，消除明显文档漂移。
- 以 fast-track 流程推进到 latest PR 可立即合并态。

### Non-goals

- 不重构网表 JSON 格式，不手工改写导出文件内部字段。
- 不修改前面板网表内容。
- 不扩展到与本轮网表同步无关的硬件设计或固件行为调整。

## 范围（Scope）

### In scope

- `docs/pcbs/mainboard/netlist.enet`
- `docs/pcbs/mainboard/README.md`
- `docs/specs/tps55288-control/contracts/config.md`
- `docs/power-monitoring-design.md`
- `docs/specs/README.md`
- `docs/specs/pcb-netlist-sync-20260319/SPEC.md`

### Out of scope

- `docs/pcbs/front-panel/netlist.enet` 的内容改动
- 其余主板/前面板设计文档的顺手整理
- 任何固件、测试或运行配置实现改动

## 需求（Requirements）

### MUST

- 主板网表落盘结果必须与 `/Users/ivan/Downloads/Netlist_Schematic1_2026-03-19.enet.enet` 完全一致。
- 前面板网表必须保持与 `/Users/ivan/Downloads/Netlist_Schematic2_2026-03-19.enet.enet` 完全一致。
- 主板 README 中 `FPC1` pin 表、输出路径说明与当前网表一致，不再引用已移除的 `J1/J2/J3`、`VOUT_A`、`VOUT_B`。
- TPS55288 配置合同必须保留 `out_a/out_b` 的器件实例与 I2C 地址语义，但把共享输出节点事实同步为 `VOUT_TPS`。
- INA3221 采样映射文档必须反映当前网表中 `CH1/CH2` 的 `IN-` 落点均为 `VOUT_TPS`。

### SHOULD

- 规格中记录“主板更新 / 前面板零差异”的验证口径，方便后续 PR review 复核。
- 文档改动保持最小化，只修正被网表事实直接推翻的内容。

### 持续同步约束

- 后续主板网表导出同步必须保留 `U10(TPS2490).VREF -> R21(10kΩ) -> PROG`，以及 `R24(1MΩ)` 从 `PROG` 到 `CHGND` 的连接事实。
- 当输入直通路径重新实测时，`docs/pcbs/mainboard/README.md` 应记录测量条件、各段压降、是否包含 `R16` 本体和总压降的计算边界；这些实测值用于识别路径中的主要压降来源，而不是替代器件额定值或量产公差。

## 验收标准（Acceptance Criteria）

- Given 更新后的仓库，
  When 比较 `docs/pcbs/mainboard/netlist.enet` 与 `/Users/ivan/Downloads/Netlist_Schematic1_2026-03-19.enet.enet` 的原始哈希与规范化哈希，
  Then 两者完全一致。

- Given 更新后的仓库，
  When 比较 `docs/pcbs/front-panel/netlist.enet` 与 `/Users/ivan/Downloads/Netlist_Schematic2_2026-03-19.enet.enet`，
  Then 两者仍然完全一致，且前面板文件没有内容改动。

- Given 更新后的主板 README，
  When 检查 `FPC1`、输出路径与 TVS/理想二极管说明，
  Then 它们与新主板网表中的 `FPC1`、`U17/U18`、`U21/Q28`、`D15/D1` 连接关系一致。

- Given 更新后的 TPS55288 合同，
When 读取 `out_a/out_b` 行与备注，
Then 文档仍冻结实例/地址映射，但不再声称输出经 `J1/J2/J3` 跳线路由。

- Given 后续的主板网表导出同步，
When 检查 `TPS2490` 的 `VREF`、`PROG` 及相邻阻值，
Then 网表与主板 README 均明确 `R21=10kΩ` 位于 `VREF` 与 `PROG` 之间，且 `R24=1MΩ` 从 `PROG` 接到 `CHGND`。

- Given `12V / 3A` 输入直通路径压降数据已记录，
When 阅读主板 README，
Then 能区分已测 PCB/MOSFET 分段压降、未包含在分段和中的 `R16` 理论压降，以及由两者得出的端到端估算值。

- Given 更新后的电源监测设计文档与 TPS55288 合同，
  When 检查 INA3221 CH1/CH2 的 `IN+/IN-` 映射，
  Then 两份文档都与当前主板网表一致，明确 `IN-1/IN-2` 落在共享节点 `VOUT_TPS`。


## 质量门槛（Quality Gates）

- 主板网表原始哈希与 `jq -S` 规范化哈希双重比对通过。
- 前面板网表原始哈希比对通过。
- 关键位号差异复核通过：至少覆盖已移除位号、共享输出节点、`FPC1` pin 映射三类事实。
- `git diff --stat` 只包含本轮网表/spec/doc 同步范围。

## Visual Evidence

PR: none
