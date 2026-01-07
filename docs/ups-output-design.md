# UPS 主输出设计（mains-aegis）

本文档描述本项目的 **UPS 主输出（UPS OUT）**：面向外部负载的系统供电母线（`DC5025` 输出口），目标是实现“插电不断电”的稳压输出与受控限流，并把实现过程中关键的**边界条件、踩坑点、验证项与参考资料**集中记录。

> 术语：本文档的 UPS OUT 是系统对外供电母线；不等价于充电器 IC 的 `SYS/VSYS`。充电器相关见：`docs/charger-design.md`。
>
> 记号：本文档的 `VBUS` 指“外部输入母线”（例如 USB‑C/PD 协商后的 VBUS，或 DC 口输入汇流母线），具体命名以原理图为准。

## 0. 本文重点（并联 TPS55288 方向）

- `ISP/ISN` 的 `10 mΩ` 分流电阻在并联方案里有双重意义：既用于**每颗芯片的输出限流检测**，也能作为**ballast（串联小电阻）**提供一定的“自平衡”趋势（并不等价于可控均流）。
- 若目标是“总输出接近 `6.3A` 时不要单颗扛满功率”，最实用的做法是：**两颗都启用输出限流**，并通过 `I2C` 设定（或校准）使两颗的限流点形成“分工”，避免两颗在同一阈值附近反复进出限流。
- 触发限流后输出会进入“恒流（CC）”行为，**输出电压会下跌**；并联时在限流边缘可能出现低频“交接班/打摆子”，需要通过限流点分离、布局对称与样机波形验证来排雷。参考：TPS55288 datasheet（7.3.14 Output Current Limit）与 TI E2E 回复“限流时输出电压会降低”。https://www.ti.com/lit/ds/symlink/tps55288.pdf https://e2e.ti.com/support/power-management-group/power-management/f/power-management-forum/1574097/tps55288-output-current-limit-higher-than-6-35a

## 1. 需求与硬边界（来自选型约束）

需求来源：`docs/hardware-selection.md` 的 2.9。

- 输出口：`DC5025`（UPS OUT），系统策略为“常开”（UPS OUT 一直有电）。
- 两个固件版本：`12V` / `19V`（通过“换固件”切换目标电压，不是运行时动态切换）。
- 输入/输出电压一致：对应版本下，外部适配器输入电压与 UPS OUT 目标电压一致（12V 版输入=12V；19V 版输入=19V）。
- 输出电压可调范围：`9–20.8V`，且必须支持 `I2C` 设置输出电压。
- 电流相关参数必须 `I2C` 可配（例如输出限流/输出电流设定）。
- 输出电流上限：`6.32A`（12V 与 19V 版本一致；功率约 `75.8W / 120W`）。
- 电源路径隔离：`VBUS →(理想二极管)→ UPS OUT`，用于阻断 **UPS OUT 倒灌回 VBUS**，同时允许 VBUS 正向给 UPS OUT 供电。

## 2. 系统电源路径（建议的逻辑框图）

目标是实现“VBUS 优先，其次电池”的自动切换，并避免任何一侧被反向灌电。

```
                 ┌──────────── ideal diode ────────────┐
External VBUS ───┤                                        ├── UPS OUT (DC5025)
                 └───────────────────────────────────────┘

Battery pack (4S, 10–16.8V) ── buck-boost / OTG reg ───────┘
```

需要你在原理图阶段明确并验证的点：

- **UPS OUT 是否允许回灌到稳压器输出端**：若稳压器（或其输出电容/开关节点）存在回灌风险，需要在稳压器到 UPS OUT 之间加反向阻断（例如理想二极管/背靠背 MOSFET/二极管，取决于效率与压降要求）。
- **VBUS 路径的理想二极管**：必须保证 UPS OUT 不会把电压倒灌回 VBUS（包含 USB‑C VBUS、DC 输入母线等）。

## 3. 候选实现路线

### 3.1 路线 A：充电管理芯片的 OTG 作为 UPS OUT

候选：`BQ25713RSNR / BQ25713BRSNR`（或同类，详见 `docs/hardware-selection.md`）。

优点：

- 单芯片、集成度高；`I2C` 设 `OTGVoltage()`（最高 `20.8V`）与 `OTGCurrent()` 的路径清晰。

风险点（必须样机验证）：

- OTG 使能通常带有**输入电压/状态的硬门槛**（例如要求 VBUS 低于阈值才允许 OTG）。
- 本项目 UPS OUT “常开”且 VBUS 与 UPS OUT 通过理想二极管相连：需要验证该门槛在“插电/拔电/负载阶跃”下不会导致 OTG 反复启停、或出现掉压/振荡。

### 3.2 路线 B：独立 buck‑boost（TPS55288/TPS552882）作为 UPS OUT

候选：`TPS55288RPMR`（或 `TPS552882` 同族，详见 `docs/hardware-selection.md`）。

优点：

- `I2C` 可编程输出（覆盖项目 `9–20.8V` 范围）；
- 支持输出电流限制设置（最大档位标称 `6.35A`，且有精度误差，需要按最坏条件设计）。

风险点（工程上最常见）：

- `19V × 6.32A` 属于功率边界工况：低电池电压时输入电流与热压力很大，需重点做 **PCB 导热/环路布局/电感与电容选型**。
- `VCC` 供电：在高功率/高压场景，为降低内部 LDO 发热，可在 `VCC` 引脚外加 **External 5‑V power supply**；外部 5V 需 `4.75–5.5V` 且具备 `≥100mA` 输出能力，并通过 `MODE` 引脚选择外部供电路径。参考：TPS55288 datasheet（7.3.1 VCC Power Supply）。https://www.ti.com/lit/ds/symlink/tps55288.pdf
- 若想通过“并联两颗 TPS55288/552882”来增大电流/改善热：TI 官方工程师的 bench test 结论是**无法实现可控均流**（没有外部环路做 power sharing），只能做到“各带一部分电流”，因此必须把它当作“实验性方案”。参考见 4.1。

## 4. TPS55288 并联/交错（Interleave）记录（关键结论与接法）

### 4.1 TI E2E 的 bench test 结论（必须记住）

TI 工程师用两块 EVM 并联做了 `6Vin → 12Vout/6A` 的台架测试，结论：

- 两块板并联后，**每块板会带走一部分负载电流**；
- 但 **power sharing（可控均流）无法实现**，原因是“没有外部环路去调节”。  
  参考：`TPS552882: Parallel Operation`（TI E2E）。https://e2e.ti.com/support/power-management-group/power-management/f/power-management-forum/1258638/tps552882-parallel-operation

另一个相关帖里，TI 也明确表示器件“并非专为并联设计，电流精度无法保证”。  
参考：`TPS55288: Output current limit higher than 6.35A?`（TI E2E）。https://e2e.ti.com/support/power-management-group/power-management/f/power-management-forum/1574097/tps55288-output-current-limit-higher-than-6-35a

### 4.2 TI 提供的“外部并联接法”（含 COMP 讲究）

TI 在 bench test 贴出了外部连接方式（适用于两块板/两颗芯片做并联实验）：

1. `VIN / VOUT / GND` 直接并在一起。
2. `FB`：各自使用独立分压（不要把 FB 硬短在一起）。
3. `COMP`：**只用一套补偿网络**，并把两颗芯片的 `COMP` **连在一起**（单一 compensation network + COMP 共点）。
4. `SYNC`：可用外部 PWM（相位差 `180°`）做 interleave。

> `COMP` 的注意点：若两颗芯片各自焊一套补偿网络再把 `COMP` 短在一起，本质上等效“并联了两套补偿”，会改变 R/C 等效值，环路稳定性不可控。

### 4.3 内部反馈（FB/INT 作 INT）时的并联差异

TI bench test 的并联接法里包含“`FB` 各自独立分压”，这对应的是 **外部反馈模式**。

如果项目采用 **内部反馈**（不使用 `FB` 分压，`FB/INT` 用作 `INT`）：

- datasheet 说明：当选择内部反馈时，`FB/INT` 为**故障指示输出**，内部故障发生时输出低电平。https://www.ti.com/lit/ds/symlink/tps55288.pdf
- TI E2E 也明确：内部反馈模式下 `FB/INT` 为 **open-drain fault indication**，需要上拉电阻，可上拉到 `VCC`，也可以上拉到 `3.3V`（示例值 `102kΩ`）。https://e2e.ti.com/support/power-management-group/power-management/f/power-management-forum/1442493/tps55288evm-045-tps55288evm-045

因此，“并联时 FB 的连接方式”需要按你们当前反馈模式重新审视：内部反馈方案里 `FB/INT` 不是控制环路的反馈点，而是告警/中断输出。

### 4.4 用限流“分摊压力”的策略与振荡风险

结论先讲清楚：把两颗的限流点设得更低，确实能降低“单颗长期扛满功率”的概率，但在并联结构里会引入一个新风险：**在限流边缘出现低频的“交接班/打摆子”**（两颗在 CC/CV 状态间来回切换）。

原因（可验证的器件行为）：

- TPS55288 的输出限流在触发后会进入“恒流（CC）”行为，输出电压会随之下跌；datasheet 描述它通过 `ISP/ISN` 检测并进入输出限流控制，TI E2E 也明确“限流时输出电压会降低”。https://www.ti.com/lit/ds/symlink/tps55288.pdf https://e2e.ti.com/support/power-management-group/power-management/f/power-management-forum/1574097/tps55288-output-current-limit-higher-than-6-35a

在并联时，常见的“交接班”机理是：

- A 先触发 CC → A 的等效输出电压下跌一点 → B 看到总线电压下跌而加力 → A 退出 CC → 总线回升 → B 又减力……形成低频摆动。

缓解方法（按优先级）：

1. **不要把两颗的限流点设成完全相同**：给它们留出分工（例如“一个偏低、一个偏高”的窗口），减少两颗在同一阈值附近反复进出 CC。
2. **每颗各自一颗 `10 mΩ` 且在合流点之前串入输出路径**：除了保证各自限流检测有效，也能提供一定 ballast（电流越大压降越大）来增强自平衡趋势。
3. **避免进入 hiccup（若启用）**：hiccup 会表现为周期性关断/重启，比 CC 边缘摆动更“像振荡”，应作为短路/严重过载的最后保护而不是常态工作点。
4. **把验证落到波形上**：在总电流 `5.5–7A` 区间做负载阶跃与温度/输入电压扫频，观察两路电流是否出现明显的低频摆动与啸叫。

## 5. 热/布局要点（TPS55288 方向，落板前就要锁死）

### 5.1 设计优先级（从“最能救命”到“锦上添花”）

1. **按 TI Layout Guideline 约束关键环路与散热过孔**（否则效率与温升会被布局支配）。
2. 电流采样（`ISP/ISN`/分流电阻）按 **Kelvin** 走线，避免噪声导致“提前限流/异常掉压”。
3. SW 节点面积：大 SW 铜皮有利散热但显著恶化 EMI；优先按指南做“小 SW + 过孔散热”。

### 5.2 参考资料（建议原理图/PCB 评审时逐条对照）

- TPS55288 产品页（datasheet/EVM/app notes 入口）：https://www.ti.com/product/TPS55288
- TPS55288 Layout Guideline（SLVAER0B）：https://www.ti.com/lit/pdf/slvaer0
- How to Achieve Low EMI with TPS55288（SLVAEX2）：https://www.ti.com/lit/pdf/slvaex2
- TPS55288EVM‑045 User Guide（SLVUBO4）：https://www.ti.com/lit/pdf/slvubo4

## 6. 验证计划（样机必须跑完的清单）

> 目标：把“能亮”推进到“能长期稳定输出且热可控”，并把并联系统风险提前暴露。

### 6.1 功能与切换

- 插拔 VBUS：确认 UPS OUT 电压连续性（无明显掉电/反复启停），并检查 VBUS 侧无倒灌。
- 电池在线/断开：确认故障策略下 UPS OUT 行为符合预期（例如限功率、关断、或降压维持）。

### 6.2 负载与瞬态

- 0A→额定电流的阶跃：检查过冲/下冲与恢复时间；确认不会触发保护或振荡。
- 最坏输入电压（电池低压）下的满载：记录效率、输入电流、关键器件温升（IC/电感/分流电阻/输入输出电容）。
- “卡边界”验证：总电流接近 `6.3A`（以及 `5.5–7A` 区间）时，分别观察两颗的 `ISP/ISN` 电流与输出波形，确认不会出现明显低频交接班（例如周期性大幅电流摆动/可闻啸叫/输出纹波显著放大）。

### 6.3 保护路径

- 限流/短路：确认“限流钳制 + 电压下跌”的预期行为；记录器件温升与是否可自恢复。

### 6.4 并联实验（如果要做）

- 仅作为实验：按 4.2 的接法搭建，并验证：
  - 两路分流是否稳定（随温度/电压/负载变化的偏斜程度）；
  - interleave 与非 interleave 的纹波/温升差异；
  - 是否出现低频“打架”（环路互相干扰导致的抖动/啸叫/异常热）。

## 7. 相关文档入口（本仓库内）

- 选型与需求：`docs/hardware-selection.md`（2.9 UPS 主输出）
- 充电器子系统：`docs/charger-design.md`
- BMS 子系统：`docs/bms-design.md`
