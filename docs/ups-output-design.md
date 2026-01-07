# UPS 主输出设计（mains-aegis）

本文档描述本项目的 **UPS 主输出（UPS OUT）**：面向外部负载的系统供电母线（`DC5025` 输出口），目标是实现“插电不断电”的稳压输出与受控限流，并把实现过程中关键的**边界条件、踩坑点、验证项与参考资料**集中记录。

> 术语：本文档的 UPS OUT 是系统对外供电母线；不等价于充电器 IC 的 `SYS/VSYS`。充电器相关见：`docs/charger-design.md`。
>
> 记号：本文档的 `VBUS` 指“外部输入母线”（例如 USB‑C/PD 协商后的 VBUS，或 DC 口输入汇流母线），具体命名以原理图为准。

## 0. 本文重点（并联 TPS55288 方向）

- `ISP/ISN` 的 `10 mΩ` 分流电阻在并联方案里有双重意义：既用于**每颗芯片的输出限流检测**，也能作为**ballast（串联小电阻）**提供一定的“自平衡”趋势（并不等价于可控均流）。
- 若目标是“总输出接近 `6.3A` 时不要单颗扛满功率”，最实用的做法是：**两颗都启用输出限流**，并通过 `I2C` 设定（或校准）使两颗的限流点形成“分工”，避免两颗在同一阈值附近反复进出限流。
- 触发限流后输出会进入“恒流（CC）”行为，**输出电压会下跌**；并联时在限流边缘可能出现低频“交接班/打摆子”，需要通过限流点分离、布局对称与样机波形验证来排雷。参考：TPS55288 datasheet（7.3.14 Output Current Limit）与 TI E2E 回复“限流时输出电压会降低”。https://www.ti.com/lit/ds/symlink/tps55288.pdf https://e2e.ti.com/support/power-management-group/power-management/f/power-management-forum/1574097/tps55288-output-current-limit-higher-than-6-35a
- 输入侧电容（`4S` 电池输入）：TPS55288 datasheet 建议按 **`CIN effective`** 设计（推荐 `4.7–22µF`，并给出 `~20µF effective` 作为起点）；并联时 **每颗 TPS55288 都必须有自己贴近 `VIN/PGND` 的 MLCC 阵列**，而 bulk（低频储能/阻尼）可在两路 VIN 汇聚点共用（例如一颗 `220µF` 固态铝/聚合物铝）。https://www.ti.com/lit/ds/symlink/tps55288.pdf

## 1. 需求与硬边界（来自选型约束）

需求来源：`docs/hardware-selection.md` 的 2.9。

- 输出口：`DC5025`（UPS OUT），系统策略为“常开”（UPS OUT 一直有电）。
- 两个固件版本：`12V` / `19V`（通过“换固件”切换目标电压，不是运行时动态切换）。
- 输入/输出电压一致：对应版本下，外部适配器输入电压与 UPS OUT 目标电压一致（12V 版输入=12V；19V 版输入=19V）。
- 输出电压可调范围：`9–20.8V`，且必须支持 `I2C` 设置输出电压。
- 电流相关参数必须 `I2C` 可配（例如输出限流/输出电流设定）。
- 输出电流上限：`6.32A`（12V 与 19V 版本一致；功率约 `75.8W / 120W`）。
- 电源路径隔离：`VBUS →(理想二极管)→ UPS OUT`，用于阻断 **UPS OUT 倒灌回 VBUS**，同时允许 VBUS 正向给 UPS OUT 供电。
  - 理想二极管电路（项目唯一方案）：统一使用 `MX5050T + N‑MOS`，并在所有需要“反向阻断/理想二极管”的位置复用该方案。资料：`docs/datasheets/MX5050T/`

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

### 4.5 倒灌/反向电流：要不要在两路 VOUT 后加理想二极管？

先澄清：你在“汇聚点 → UPS OUT 接口”之间加一颗理想二极管（ORing），**只能阻断外部从 UPS OUT 接口倒灌回系统**；它**不能**阻断“并联的两颗 TPS55288 之间”的互相灌电，因为两颗仍然在同一个汇聚节点上。

因此，你需要先明确要防的“倒灌”是哪一种：

1. **外部倒灌（UPS OUT 接口被外部供电）**：  
   - 在“汇聚点 → UPS OUT 接口”之间加理想二极管是有效的（阻断外部回灌）。  
   - 代价是器件成本 + 大电流损耗/散热，需要选低 `RDS(on)` 的 MOSFET‑based 理想二极管方案并做热设计。
2. **内部倒灌（两颗 TPS55288 互相倒灌，或一颗关断/异常时被另一颗 back‑drive）**：  
   - 这种情况在“并联且无专用均流/隔离器件”的系统里**理论上是可能发生的**（输出设定误差/走线压降差导致的 cross‑current，或一颗进入 CC/关断/保护状态时的互相作用）。  
   - TPS55288 的器件行为中，确实存在“反向电流/反向功率流”的场景：  
     - 在 **FPWM（forced PWM）** 的轻载条件下，datasheet 明确写到：电感电流在到零后会继续反向，**功率从 output side 回到 input side**（输出侧向输入侧回灌）。https://www.ti.com/lit/ds/symlink/tps55288.pdf  
     - 在 **PFM** 下，datasheet 明确写到：当电感电流到零时会关断相应开关以 **prevent the reverse current**（buck 模式防止输出到地的反向电流，boost 模式防止输出到输入的反向电流）。https://www.ti.com/lit/ds/symlink/tps55288.pdf  
     - 若启用 `DISCHG`（Output discharge），器件在 shutdown 时会用内部约 **100‑mA current sink** 把 `VOUT` 拉向地；并联时这会变成“其中一颗掉线/被关断时拖母线”的固定负载。https://www.ti.com/lit/ds/symlink/tps55288.pdf  
   - 因此，即使器件在某些模式下会主动抑制 reverse current 或有“关断泄放”，这些都不等价于“并联系统下完全无倒灌/无互相 back‑drive”的保证。
   - 对“会不会因为倒灌损坏芯片”的风险判断：  
     - 在项目明确 **不强制 FPWM**、并且 **禁用 `DISCHG`** 的前提下，内部倒灌的主要来源通常是“输出设定/走线压降失配导致的 cross‑current”，而不是器件主动吞电流。  
     - 从 datasheet 的规格看，器件允许 `VOUT/SW2/ISP/ISN` 达到 `25V`（绝对最大额定），且在 IC disabled 且 `VOUT=20V` 的条件下，`VOUT` 引脚的漏电流量级为 `µA`。这意味着在 `12V/19V` 这种 UPS 母线电压下，“被外部预置电压 back‑drive”本身不太像会导致大电流灌入芯片内部。https://www.ti.com/lit/ds/symlink/tps55288.pdf  
     - TI E2E 的相关问答也支持“输出侧带电不会直接伤芯片”的判断：当 `VIN=0V`（器件处于 shutdown）时，从输出侧提供外部电源“不会损坏 IC”，但要注意 `VOUT` 的绝对最大额定 `25V`。https://e2e.ti.com/support/power-management-group/power-management/f/power-management-forum/1361903/tps55288-tps55288-output-side-provide-the-external-5v-power-source
     - 另一个更直接的 E2E 回答指出：即使输出侧有电、器件 disabled 且无输入电压，**也不需要额外 load switch 来“隔离 VOUT”**；其依据是器件内部 boost 高侧 FET 的体二极管方向可阻止 `VOUT -> VIN` 的反向电流。https://e2e.ti.com/support/power-management-group/power-management/f/power-management-forum/987946/tps55288-q1-reverse-current-blocking
     - 但注意：上述只能说明“正常规格下的电气应力可接受”，并不能覆盖“器件内部 MOSFET 失效短路”等硬故障模式。
   - 如果你必须对“内部倒灌”做硬保证，通常需要在**每一路输出**都放 ORing（理想二极管/背靠背 MOSFET）来隔离两路。但在本项目的“内部反馈（`FB/INT` 作 `INT`）”设定下，串入 ORing 元件会带来额外压降与温漂，需要评估是否能通过 `I2C` 设压补偿（且补偿会随电流变化）。

工程建议（设计阶段的折中做法）：

- 若你的主要担心是“外部 UPS OUT 口被外部电源误接”，优先在接口侧做一颗总 ORing（汇聚点→接口）。  
- 若你的主要担心是“并联时某颗失效/关断被 back‑drive 导致过热/异常”，建议在 PCB 上预留**每路 ORing 的可选 footprint**（默认 0Ω 直通），等样机实测后再决定是否装配。

## 5. 热/布局要点（TPS55288 方向，落板前就要锁死）

### 5.1 设计优先级（从“最能救命”到“锦上添花”）

1. **按 TI Layout Guideline 约束关键环路与散热过孔**（否则效率与温升会被布局支配）。
2. 电流采样（`ISP/ISN`/分流电阻）按 **Kelvin** 走线，避免噪声导致“提前限流/异常掉压”。
3. SW 节点面积：大 SW 铜皮有利散热但显著恶化 EMI；优先按指南做“小 SW + 过孔散热”。

### 5.2 输入侧电容（电池输入，双 TPS55288 并联）

> 设计目标：把“高频开关电流回路”就地闭合在每颗 TPS55288 周围，同时用 bulk 提供低频能量与阻尼；所有计算以 **effective capacitance（含 DC bias）** 为准。

每颗 TPS55288（每路）建议配置：

- **就地小电容（高频旁路）**：`0.1µF + 1µF` 各 1 颗，紧贴 `VIN/PGND` 引脚放置并走最短回路。
- **就地 MLCC 阵列（输入纹波电流与能量储备的主力）**：按 datasheet 建议，优先满足 `CIN effective 4.7–22µF`，并以 `~20µF effective` 作为 good starting point。https://www.ti.com/lit/ds/symlink/tps55288.pdf
  - **本项目落板基线（每颗 TPS55288）**：`3×10µF / 50V / 1206` + `1µF` + `0.1µF`（均贴近 `VIN/PGND`）。
  - **强烈建议每颗额外预留 `1×1206（DNP）`**：用于样机阶段按 `VIN` 纹波/振铃波形加容（例如补到 `4×10µF`），避免被未知厂牌/未知介质的 DC bias 降容“卡死”调试空间。

bulk（低频储能/阻尼）建议：

- **固态铝/聚合物铝 bulk 可共用**：例如在两路 VIN 汇聚点放 1 颗 `220µF`，用于电池侧低频压降缓冲与阻尼。
- 但注意：**bulk 不能替代每颗芯片的就地 MLCC**（否则高频回路会被迫走长路径，EMI/纹波/瞬态都会变差）。
- datasheet 也提醒：若输入电源离转换器超过几英寸，需要额外 bulk（典型 `100µF` 铝电解）。本项目电池与 TPS55288 共板（正反面）时通常不属于该场景，但仍以样机波形验证为准。https://www.ti.com/lit/ds/symlink/tps55288.pdf

与开关频率的关系（只给结论）：

- 提高 `fSW` 会降低“同等纹波下所需的电容量”，但也会增加开关损耗与热压力；TPS55288 datasheet 在高功率条件下建议把 `fSW` 设在 `500kHz` 以下量级。https://www.ti.com/lit/ds/symlink/tps55288.pdf
- 因此工程上不要指望“把频率开高就能省输入电容”，仍以 datasheet 的 `CIN effective` 建议与样机波形为准。

### 5.3 输出电容与 `10 mΩ` 检流电阻的位置关系（关键）

TPS55288 的典型电路中，`10 mΩ` 电阻位于 `VOUT` 引脚与系统输出之间，用于 `ISP/ISN` 的限流检测。

布局上不要简单理解为“所有输出电容都放在检流电阻之后”：

- **IC 侧（`VOUT` 引脚这一侧，检流电阻之前）必须有高频去耦电容**（通常是小容量陶瓷电容，贴近 `VOUT` 与 `PGND`），用来闭合 boost 部分的高 di/dt 电流回路、减小 `SW2/VOUT` 过冲。TI datasheet 的 layout guideline 明确强调输出电容要同时靠近 `VOUT` 与 `PGND`。https://www.ti.com/lit/ds/symlink/tps55288.pdf
- **系统侧（检流电阻之后）放主要的 bulk 输出电容**（靠近汇流点/接口/负载），用于负载阶跃与母线稳压；这部分电容的充放电电流会经过 `10 mΩ`，等效也会被计入输出电流（符合“限流分摊/压力可控”的目标）。
- `ISP/ISN` 走线：从检流电阻两端到 `ISP/ISN` 的走线必须并行且贴近（Kelvin），避免噪声耦合导致误触发限流或波形异常。https://www.ti.com/lit/ds/symlink/tps55288.pdf

### 5.4 并联两路时 bulk 电容要不要“每路都留”

结论：**建议每路都预留 bulk 的位置，但不建议在 TPS 侧堆“超大 bulk”**；系统侧（合流后的母线/接口侧）可以放大电容（例如 `3300µF/35V`）作为总线电容。

你现在的想法（每路在理想二极管前预留一颗 bulk，接口侧再放 `3300µF/35V`）整体是合理的，但建议按节点分层设计，避免把“回路去耦”和“母线储能”混在一起：

- `TPS_VOUT`（`10 mΩ` 之前）：**一定要**有本地高频去耦（几颗陶瓷，紧贴 `VOUT/PGND`），否则尖峰/振铃/EMI 会被走线寄生支配。
- `BRANCH_OUT`（每路 `10 mΩ` 之后、该路 ORing 之前）：建议预留 `100–470µF` 量级的 bulk footprint（按空间/ESR 选型，默认可不装）。  
  - 不建议每路上来就堆“几千 µF”：会显著增加启动/使能时的充电浪涌，把 TPS 推到输出限流，甚至更容易触发并联“交接班”。
- `UPS_BUS`（两路 ORing 合流后、靠近 UPS_OUT 接口）：`3300µF/35V` 作为总线电容可以，但要把它当作“系统储能件”：插拔/短路时的涌入/放电电流由接口侧 ORing/TVS/走线铜皮承担与限制。

你计划用大电流贴片跳线实现两种拓扑切换时，建议：

- 默认装配优先选“**每路 ORing 后再合流**”：两路之间天然隔离，`3300µF` 放在合流后的 `UPS_BUS` 就是共享且最有效的。
- “先合流再串单颗 ORing”建议仅作为备选：两路之间不隔离，更依赖限流分工与对称布局；此时更建议保留“每路 ORing”的可选焊盘以便后续切回。

### 5.5 参考资料（建议原理图/PCB 评审时逐条对照）

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
- VIN 波形：用短地弹簧探头在每颗 TPS55288 的 `VIN/PGND` 处测量纹波与振铃（含双路同时大功率与负载阶跃），并据波形决定是否装配 5.2 的 DNP 加容焊盘。
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
