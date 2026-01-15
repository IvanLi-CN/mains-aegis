# UPS 主输出设计（mains-aegis）

本文档描述本项目的 **UPS 主输出（UPS OUT）**：面向外部负载的系统供电母线（`DC5025` 输出口），目标是实现“插电不断电”的稳压输出与受控限流，并把实现过程中关键的**边界条件、风险点、验证项与参考资料**集中记录。

> 术语：本文档的 UPS OUT 是系统对外供电母线；不等价于充电器 IC 的 `SYS/VSYS`。充电器相关见：`docs/charger-design.md`。
>
> 记号：本文档的 `VBUS` 指“外部输入母线”（例如 USB‑C/PD 协商后的 VBUS，或 DC 口输入汇流母线），具体命名以原理图为准。

## 0. 本文重点（并联 TPS55288 方向）

- `ISP/ISN` 的 `10 mΩ` 分流电阻在并联方案里有双重意义：既用于**每颗芯片的输出限流检测**，也能作为**ballast（串联小电阻）**提供一定的“自平衡”趋势（并不等价于可控均流）。
- 若目标是“总输出接近 `6.3A` 时不要单颗扛满功率”，最实用的做法是：**两颗都启用输出限流**，并通过 `I2C` 设定（或校准）使两颗的限流点形成“分工”，避免两颗在同一阈值附近反复进出限流。
- 触发限流后输出会进入“恒流（CC）”行为，**输出电压会下跌**；并联时在限流边缘可能出现低频“交替接管/摆动”，需要通过限流点分离、布局对称与样机波形验证来规避。参考：TPS55288 datasheet（7.3.14 Output Current Limit）与 TI E2E 回复“限流时输出电压会降低”。https://www.ti.com/lit/ds/symlink/tps55288.pdf https://e2e.ti.com/support/power-management-group/power-management/f/power-management-forum/1574097/tps55288-output-current-limit-higher-than-6-35a
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

原理图阶段应明确并验证的点：

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

外置功率 MOSFET（buck leg，`DR1H/DR1L`）（已定）：

- `TPS55288` **只集成 boost leg 的两颗 MOSFET**；buck leg 需要 **两颗外置 N‑MOS**，分别由 `DR1H`（高边）与 `DR1L`（低边）驱动。
- buck MOS 的 gate drive 电压为 `VCC≈5V`（高边由 `BOOT1` 自举供电），因此选型优先看 `RDS(on)@4.5V` 与 `Qg`，不要只看 `@10V` 的指标。
- 本项目功率 NMOS 统一池：`NCEP3065QU` + `NCEP3040Q`。
- 推荐装配：
  - **19V 版本（电池输入永远低于 19V，绝大多数时间为 boost）**：`DR1H` 用 `NCEP3065QU`（降低导通损耗）；`DR1L` 用 `NCEP3040Q`（降成本：boost 模式下 `DR1H` 常高、`DR1L` 常低，不参与 PWM）。
  - **12V 版本（可能进入 buck / buck‑boost）**：上下管优先都用 `NCEP3065QU`；若要降成本可把下管改为 `NCEP3040Q`，但需结合效率/温升与 EMI 实测验证。

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

因此，“并联时 FB 的连接方式”需要按当前反馈模式重新审视：内部反馈方案里 `FB/INT` 不是控制环路的反馈点，而是告警/中断输出。

### 4.4 用限流“分摊压力”的策略与振荡风险

结论先讲清楚：把两颗的限流点设得更低，确实能降低“单颗长期扛满功率”的概率，但在并联结构里会引入一个新风险：**在限流边缘出现低频“交替接管/摆动”**（两颗在 CC/CV 状态间来回切换）。

原因（可验证的器件行为）：

- TPS55288 的输出限流在触发后会进入“恒流（CC）”行为，输出电压会随之下跌；datasheet 描述它通过 `ISP/ISN` 检测并进入输出限流控制，TI E2E 也明确“限流时输出电压会降低”。https://www.ti.com/lit/ds/symlink/tps55288.pdf https://e2e.ti.com/support/power-management-group/power-management/f/power-management-forum/1574097/tps55288-output-current-limit-higher-than-6-35a

在并联时，常见的“交替接管”机理是：

- A 先触发 CC → A 的等效输出电压下跌一点 → B 看到总线电压下跌而加力 → A 退出 CC → 总线回升 → B 又减力……形成低频摆动。

缓解方法（按优先级）：

1. **不要把两颗的限流点设成完全相同**：给它们留出分工（例如“一个偏低、一个偏高”的窗口），减少两颗在同一阈值附近反复进出 CC。
2. **每颗各自一颗 `10 mΩ` 且在合流点之前串入输出路径**：除了保证各自限流检测有效，也能提供一定 ballast（电流越大压降越大）来增强自平衡趋势。
3. **避免进入 hiccup（若启用）**：hiccup 会表现为周期性关断/重启，比 CC 边缘摆动更“像振荡”，应作为短路/严重过载的最后保护而不是常态工作点。
4. **把验证落到波形上**：在总电流 `5.5–7A` 区间做负载阶跃与温度/输入电压扫频，观察两路电流是否出现明显的低频摆动与啸叫。

### 4.5 倒灌/反向电流：要不要在两路 VOUT 后加理想二极管？

澄清：在“汇聚点 → UPS OUT 接口”之间加一颗**理想二极管**，**只能阻断外部从 UPS OUT 接口倒灌回系统**；它**不能**阻断“并联的两颗 TPS55288 之间”的互相灌电，因为两颗仍然在同一个汇聚节点上。

因此，需要先明确要防的“倒灌”是哪一种：

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
     - 若需要对“内部倒灌”做硬保证，通常需要在**每一路输出**都放**理想二极管（或背靠背 MOSFET）**来隔离两路。但在本项目的“内部反馈（`FB/INT` 作 `INT`）”设定下，串入理想二极管器件会带来额外压降与温漂，需要评估是否能通过 `I2C` 设压补偿（且补偿会随电流变化）。

#### 4.5.1 `TPS55288.MODE` 电阻（本项目约定：外部 `5V` 供 `VCC` + 默认 `PFM`）

`TPS55288.MODE` 引脚通过“`MODE` → `AGND`”的电阻在上电后选择三件事（datasheet Table 7‑2）：`VCC` 供电来源（internal / external 5V）、I2C 从地址（`0x74/0x75`）、以及轻载模式（`PFM/FPWM`）。https://www.ti.com/lit/ds/symlink/tps55288.pdf

本项目采用“外部 `5V` 供 `VCC`”，并且在系统层面**不强制 `FPWM`**（避免轻载时出现 output→input 的反向功率流；见上文 4.5）。因此，主板两颗 `TPS55288` 的 `MODE` 电阻固定如下（用于在 MCU 尚未配置寄存器前就避免 I2C 地址冲突，并默认进入 `PFM`）：

| 路 | I2C 地址 | `MODE`→`AGND` 电阻 | 含义（按 Table 7‑2） |
|---|---:|---:|---|
| OUT‑A / TPS‑A | `0x74` | `75.0 kΩ` | `VCC=External` + `I2C=0x74` + 轻载 `PFM` |
| OUT‑B / TPS‑B | `0x75` | `DNP/Open` | `VCC=External` + `I2C=0x75` + 轻载 `PFM` |

> 备注：datasheet 同时说明 I2C 可覆盖这些 strap 结果（写寄存器即可改变 `VCC/I2CADD/PFM` 等相关位）。本项目仍要求 strap 本身“可自洽”，以免上电阶段就出现双从设备地址冲突导致总线不可用。

#### 4.5.2 `TPS55288.EN/UVLO` 分压网络（本项目约定：参考地用 `AGND_TPSA`）

本项目两颗 `TPS55288`（OUT‑A / OUT‑B）在原理图上将 `EN/UVLO` 合并为同一条使能网 `TPS_EN`（系统级使能/关断）。因此：

- **UVLO 分压网络只允许保留一套**：`VBAT → TPS_EN → AGND_TPSA`（并可在 `TPS_EN` 处做 RC 去抖/软启动）。
- **参考地最终选用 `AGND_TPSA`**：避免 `TPS_EN` 同时参考 `AGND_TPSA` 与 `AGND_TPSB`，导致两套分压并联、并通过 `TPS_EN` 把两块模拟地“软耦合”在一起（噪声/阈值漂移风险更高）。
- 若后续需要“每路独立 UVLO/独立 enable”，应在原理图层面把 `TPS_EN` 拆分为 `TPSA_EN` / `TPSB_EN`，并各自就近参考本路 `AGND_*`。

对照原理图器件位号（用于装配与审核，一旦位号变更应同步更新本段）：

- 预期仅存在：`R74/R75/C144`（`VBAT → TPS_EN → AGND_TPSA`，以及 `TPS_EN`→`AGND_TPSA` 的 RC）

工程建议（设计阶段的折中做法）：

- 若主要担心是“外部 UPS OUT 口被外部电源误接”，优先在接口侧做一颗总**理想二极管**（汇聚点→接口）。  
- 若主要担心是“并联时某颗失效/关断被 back‑drive 导致过热/异常”，建议在 PCB 上预留**每路理想二极管的可选 footprint**（默认 0Ω 直通），等样机实测后再决定是否装配。

## 5. 热/布局要点（TPS55288 方向，落板前就要锁死）

### 5.1 设计优先级（从关键项到优化项）

1. **按 TI Layout Guideline 约束关键环路与散热过孔**（否则效率与温升会被布局支配）。
2. 电流采样（`ISP/ISN`/分流电阻）按 **Kelvin** 走线，避免噪声导致“提前限流/异常掉压”。
3. SW 节点面积：大 SW 铜皮有利散热但显著恶化 EMI；优先按指南做“小 SW + 过孔散热”。

### 5.2 输入侧电容（电池输入，双 TPS55288 并联）

> 设计目标：把“高频开关电流回路”就地闭合在每颗 TPS55288 周围，同时用 bulk 提供低频能量与阻尼；所有计算以 **effective capacitance（含 DC bias）** 为准。

每颗 TPS55288（每路）建议配置：

- **就地小电容（高频旁路）**：`0.1µF + 1µF` 各 1 颗，紧贴 `VIN/PGND` 引脚放置并走最短回路。
- **就地 MLCC 阵列（输入纹波电流与能量储备的主力）**：按 datasheet 建议，优先满足 `CIN effective 4.7–22µF`，并以 `~20µF effective` 作为 good starting point。https://www.ti.com/lit/ds/symlink/tps55288.pdf
  - **本项目落板基线（每颗 TPS55288）**：`3×10µF / 50V / 1206` + `1µF` + `0.1µF`（均贴近 `VIN/PGND`）。
  - **强烈建议每颗额外预留 `1×1206（DNP）`**：用于样机阶段按 `VIN` 纹波/振铃波形加容（例如补到 `4×10µF`），避免因未知厂牌/介质的 DC bias 降容导致有效电容不足而缺少调试余量。

bulk（低频储能/阻尼）建议：

- **固态铝/聚合物铝 bulk 可共用**：例如在两路 VIN 汇聚点放 1 颗 `220µF`，用于电池侧低频压降缓冲与阻尼。
- 但注意：**bulk 不能替代每颗芯片的就地 MLCC**（否则高频回路会被迫走长路径，EMI/纹波/瞬态都会变差）。
- datasheet 也提醒：若输入电源离转换器超过几英寸，需要额外 bulk（典型 `100µF` 铝电解）。本项目电池与 TPS55288 共板（正反面）时通常不属于该场景，但仍以样机波形验证为准。https://www.ti.com/lit/ds/symlink/tps55288.pdf

与开关频率的关系（只给结论）：

- 提高 `fSW` 会降低“同等纹波下所需的电容量”，但也会增加开关损耗与热压力；TPS55288 datasheet 在高功率条件下建议把 `fSW` 设在 `500kHz` 以下量级。https://www.ti.com/lit/ds/symlink/tps55288.pdf
- 因此工程上不要指望“把频率开高就能省输入电容”，仍以 datasheet 的 `CIN effective` 建议与样机波形为准。

#### 5.2.1 输入电容校核：`CIN effective` 与 `ICIN(RMS)` 是否覆盖本项目边界

TPS55288 datasheet 在 buck 模式下给出输入电容 RMS 纹波电流（Equation 14）：`ICIN(RMS) = IOUT × sqrt(VOUT × (VIN - VOUT)) / VIN`。https://www.ti.com/lit/ds/symlink/tps55288.pdf

以本项目“12V 固件 + 电池最高电压”为 buck 最坏点（`VIN=16.8V → VOUT=12V, IOUT=6.32A`）估算：

- `ICIN(RMS) ≈ 2.86A`（单颗 TPS55288 承担全部负载时）
- 并联两颗且均分时，每颗约为 `1.43A`（仅用于量级参考；并联系统在限流分工/动态扰动下不保证长期严格均分）

对照 datasheet 的两条建议：

- 推荐范围：`CIN effective 4.7–22µF`
- good starting point：`~20µF effective`

本项目每颗 `3×10µF/50V/1206` 的 nominal 为 `30µF`，但 `CIN_eff` 往往被 DC bias 支配。公开 DC bias 曲线示例中，`10µF/50V/1206/X5R` 在 `~17V` 时可能仅剩 `~2.6µF`（≈`25%`）/颗，因此 `3×10µF` 的 `CIN_eff` 可能只有 `~8µF`（仍落在 datasheet 推荐范围内，但未必能接近 `20µF effective` 的起点）。示例：Murata SimSurfing（`GRT31CR61H106KE01`）。https://ds.murata.com/simsurfing/mlcc.html?lcid=en-us https://www.ti.com/lit/ds/symlink/tps55288.pdf

因此本项目在每颗 TPS55288 旁边保留并默认装配 `1×10µF（DNP → 装配）` 作为样机余量是合理的输入电容策略；是否可减配以 `VIN/PGND` 处短地弹簧测得的纹波/振铃为准决定。https://www.ti.com/lit/ds/symlink/tps55288.pdf

结论：按上述 DC bias 示例量级，`3×10µF` 的 `CIN_eff` 约为 `8µF` 量级，若装配第 4 颗则可到 `~10µF` 量级；两者都落在 datasheet 推荐的 `4.7–22µF effective` 范围内，因此当前“每颗就地 MLCC + 汇聚点 bulk + 预留 DNP”的输入电容策略是合理的。https://www.ti.com/lit/ds/symlink/tps55288.pdf

### 5.3 输出电容与 `10 mΩ` 检流电阻的位置关系（关键）

TPS55288 的典型电路中，`10 mΩ` 电阻位于 `VOUT` 引脚与系统输出之间，用于 `ISP/ISN` 的限流检测。

布局上不要简单理解为“所有输出电容都放在检流电阻之后”：

- **IC 侧（`VOUT` 引脚这一侧，检流电阻之前）必须有高频去耦电容**（通常是小容量陶瓷电容，贴近 `VOUT` 与 `PGND`），用来闭合 boost 部分的高 di/dt 电流回路、减小 `SW2/VOUT` 过冲。TI datasheet 的 layout guideline 明确强调输出电容要同时靠近 `VOUT` 与 `PGND`。https://www.ti.com/lit/ds/symlink/tps55288.pdf
- **系统侧（检流电阻之后）放母线 bulk 输出电容**（靠近 UPS_OUT 接口/负载），用于负载阶跃与母线稳压；这部分电容的充放电电流会经过 `10 mΩ`，等效也会被计入输出电流（符合“限流分摊/压力可控”的目标）。
  - **本项目当前约束**：接口侧只放 **单颗大电容**（见 5.4），不再假设存在“多个中等电容（例如 1000µF×N）”的布局空间。
- `ISP/ISN` 走线：从检流电阻两端到 `ISP/ISN` 的走线必须并行且贴近（Kelvin），避免噪声耦合导致误触发限流或波形异常。https://www.ti.com/lit/ds/symlink/tps55288.pdf

#### 5.3.1 按项目边界估算“每路 TPS55288 需要多少有效输出电容”

输出端需区分“开关回路去耦节点”与“母线节点”，避免把低频储能与高频尖峰抑制混为一谈：

- **`TPS_VOUT`（`10 mΩ` 之前、IC 侧）**：决定 `SW2/VOUT` 的尖峰/振铃与高频纹波，**必须靠近 `VOUT/PGND` 放 MLCC** 才能有效压制（高 di/dt 回路要就地闭合）。
- **`BRANCH_OUT`（`10 mΩ` 之后、理想二极管之前）**：该节点放置的固态铝/聚合物电容主要用于母线储能/阻尼与负载阶跃；它也能降低该节点及后级（含 `UPS_OUT`）的纹波，但**不能替代 IC 侧 MLCC**（见 5.3.2）。

TPS55288 datasheet（boost 模式）给出的输出电容纹波公式（按“有效电容”计算）：

- `Vripple(CAP) = IOUT × (1 - VIN/VOUT) / (COUT × fSW)`（Equation 17）  
  当 `VIN` 最小、`VOUT` 最大时，电容项纹波最大。https://www.ti.com/lit/ds/symlink/tps55288.pdf
- 另外输出电容的 RMS 纹波电流：`ICOUT(RMS) = IOUT × sqrt(VOUT/VIN - 1)`（Equation 15）。https://www.ti.com/lit/ds/symlink/tps55288.pdf

本项目“12V/19V 两固件”边界（来自 1. 需求与硬边界）：

- `VIN(min) = 10 V`（4S 电池截止放电）；
- `IOUT(max) = 6.32 A`；
- 为了给高功率留效率/温升空间，开关频率以 datasheet 示例常用值 **`fSW = 400 kHz`** 作为计算基线（例如 RFSW=49.9k 的示例设计）；若实际 `fSW` 不同，所需电容与 `1/fSW` 成比例缩放。https://www.ti.com/lit/ds/symlink/tps55288.pdf

纹波目标：项目未硬性规定 `UPS_OUT` 的允许纹波，这里先用 datasheet 示例设计中的量级 **`±50 mV`（即 `100 mVpp`）** 作为工程目标（更严的目标会直接推高 `COUT_eff`）。https://www.ti.com/lit/ds/symlink/tps55288.pdf

据此可得（只计算 CAP 项；MLCC ESR 很低时该项通常占主导）：

- **19V 固件最坏点**：`VIN=10V → VOUT=19V`  
  - `COUT_eff ≥ 74.8 µF`（保证 `Vripple(CAP) ≤ 100 mVpp`）  
  - `ICOUT(RMS) ≈ 6.0 A`
- **12V 固件（boost 区域，`VIN=10V → VOUT=12V`）估算点**：  
  - `COUT_eff ≥ 26.3 µF`  
  - `ICOUT(RMS) ≈ 2.8 A`

#### 5.3.2 输出侧 MLCC：当前装配计划（以降低高度为目标）

本项目在 `TPS_VOUT`（`10 mΩ` 之前、IC 侧）默认采用“纯 MLCC”方案，不在检流电阻前再叠加固态铝/聚合物（相关边界见 5.3.3）。

为降低器件高度，输出侧 MLCC 从：

- `4× 22µF / 35V / 0805`

调整为：

- `8× 10µF / 50V / 0603`

并继续保留 `1µF + 0.1µF` 作为高频旁路（贴近 `VOUT/PGND`）。

在不指定具体料号的前提下，“有效电容（含 DC bias）”只能做经验估计。对本项目当前 `8×10µF/50V/0603`，记单颗在 `VOUT` DC bias 下的有效电容为 `C10_eff(@VOUT)`，则 `COUT_eff ≈ 8 × C10_eff(@VOUT)`。

这里优先用公开 DC bias 曲线给出量级，避免“按 nominal 直接相加”导致严重低估所需数量：

- `22µF/35V/0805/X5R @ 19V`：约 `2.0µF`（≈`9%`），示例：TDK Product Center（`C2012X5R1V226M125AC`，按 16V 与 25V 数据线性插值得到 19V 点）。https://product.tdk.com/en/search/capacitor/ceramic/mlcc/info?part_no=C2012X5R1V226M125AC
- 作为“100µF class”量级参考：`100µF/25V/2220` 在 `19V` 时约 `58.5µF`（≈`59%`），示例：Murata SimSurfing（`KCM55WC71E107MH13`）。该例仅用于说明“100µF 级别的大封装 MLCC 在 19V 下仍可能保有数十 µF 的有效电容”，不等价于 `100µF/50V/1210` 的行为。https://ds.murata.com/simsurfing/mlcc.html?lcid=en-us

据此可以直接解释“为什么看起来要很多 MLCC”：在 19V 固件最坏点需要 `COUT_eff ≥ 74.8µF`（5.3.1）。以 `22µF/35V/0805` 的公开曲线为例，在 `19V` 时每颗只有 `~2µF`，即使装满 4 颗也只有 `~8µF`，仅从 `Vripple(CAP)` 公式会得到接近 `~1Vpp` 的纹波量级。同样地，`10µF/50V/0603` 这类“小封装高 CV”电容在十几到二十伏 DC bias 下也可能出现强烈降容，因此 nominal `80µF` 并不等价于 `80µF effective`。

（备选）若目标是把 `TPS_VOUT` 的 CAP 项纹波压到更低（例如 `~100mVpp` 量级），通常需要引入“更高有效电容/更低阻抗”的器件（例如更大封装的 MLCC），而不仅仅是堆叠小封装。

关于 `100µF/50V/1210`：不同介质/结构的 DC bias 差异非常大，无法在不指定料号的前提下给出可信的固定比例；更稳妥的写法是把 `100µF` 的 `Ceff` 当作参数，并给出达到目标所需的“条件”。

记：`C22_eff(@19V) ≈ 2.0µF/颗`；`C100_eff(@19V)` 为“100µF/50V/1210”在 19V DC bias 下的实际有效电容（需用所用电容的 DC bias 曲线确认）。

在 19V 固件最坏点（`VIN=10V → VOUT=19V, IOUT=6.32A, fSW=400kHz`）下，为达到 `Vripple(CAP) ≤ 100mVpp`：

- **组合 A（默认推荐，面积/数量折中）**：`2×100µF + 2×22µF + 1µF + 0.1µF`  
  - 需要 `C100_eff ≥ ~36µF/颗` 才能满足 `100mVpp` 量级（此时总 `Ceff ≳ 2×36 + 2×2 = 76µF`）
- **组合 B（更依赖 100µF 的有效电容）**：`2×100µF + 1×22µF + 1µF + 0.1µF`  
  - 需要 `C100_eff ≥ ~37µF/颗`
- **组合 C（减少 0805 数量，允许装满 1210）**：`3×100µF + 1µF + 0.1µF`  
  - 需要 `C100_eff ≥ ~25µF/颗`

为便于快速做“经验量级”判断（仍以实测为准），给出组合 A 的几个示例点（仅 CAP 项）：

- 若 `C100_eff(@19V) ≈ 30µF/颗`：`Vripple(CAP) ≈ 117mVpp`
- 若 `C100_eff(@19V) ≈ 40µF/颗`：`Vripple(CAP) ≈ 89mVpp`
- 若 `C100_eff(@19V) ≈ 50µF/颗`：`Vripple(CAP) ≈ 72mVpp`

> 说明：以上只对 `Vripple(CAP)` 做量级估算。由于本项目每路在 `BRANCH_OUT`（`10mΩ` 之后）还有固态铝/聚合物电容（见 5.3.3），`UPS_OUT` 看到的纹波通常会小于 `TPS_VOUT`；因此当空间不足以满足“把 `TPS_VOUT` 的 CAP 项压到 100mVpp”时，工程上可接受把目标放到 `UPS_OUT`（而不是死卡 `TPS_VOUT` 的公式值），最终以两处实测波形为准。

其中 `0.1µF + 1µF` 的作用是高频旁路（降低尖峰/振铃），不计入“低频能量储备”也应保留。

上述组合仅给出电容数量与纹波量级。布局上至少应保证：`0.1µF + 1µF` 以及 **至少 1~2 颗中等/大容量 MLCC（或多颗 0603 并联阵列）** 与 `VOUT/PGND` 形成最短回路；其余电容再根据空间分配到 `TPS_VOUT` 侧的可用位置，并通过样机测量确认 `SW2/VOUT` 尖峰与 `TPS_VOUT` 纹波满足要求。

若 PCB 空间不允许把全部电容都紧贴 `VOUT/PGND`（例如部分 MLCC 距离较远或回流路径不可控），输出电容可按“就地高频去耦”与“母线低频储能/阻尼”分层部署：

- **IC 侧（`TPS_VOUT`）优先保证“就地高频去耦 + 基本稳定性”**：例如 `0.1µF + 1µF + 8×10µF/0603`（或 `1~2` 颗更大封装的 MLCC）尽量贴近 `VOUT/PGND`（必要时放到背面并用成对过孔直连），其任务是抑制尖峰/振铃并闭合高 di/dt 回路。
- **系统侧（`BRANCH_OUT` 的固态铝/聚合物 + `UPS_OUT` 的大 bulk）承担更多低频能量与阻尼**：它会显著改善 `BRANCH_OUT/UPS_OUT` 的纹波与负载阶跃响应；但它不负责消掉 `TPS_VOUT` 的高频尖峰（见 5.3.3）。

> 布局提示：**“直线距离 10 mm”不是硬性界限**，关键在于“回路电感/回流路径”。若通过宽铜皮 + 紧贴参考地平面 + 多过孔把回路做小，距离较远的电容仍可能对 `fSW` 主纹波有一定贡献；但对更高频尖峰/振铃（MHz 级以上）的贡献会快速变差。

若样机验证发现：

- 实际 `Ceff` 明显高于 20%：可以考虑减少电容数量（或把余量当作“低温/老化/批次差”的安全垫）；
- 实际 `Ceff` 低于 20%：在不增加 MLCC 数量的前提下，只能放宽纹波目标、提高 `fSW`（以热为代价）、或改用更“电容不掉”的 bulk（例如更大体积的聚合物/电解等）来承担更多低频能量与阻尼。

#### 5.3.3 `BRANCH_OUT` 的固态铝/聚合物电容到底能不能“参与压纹波”

可以参与，但需要按频段区分其作用边界：

- **能明显帮助的**：`BRANCH_OUT` 与 `UPS_OUT`（理想二极管之后）看到的**低频纹波与负载阶跃**（母线储能/阻尼）；这也是我们在 5.4 默认建议“每路 `100–470µF` 固态铝/聚合物”的主要原因。
- **对 `fSW` 主纹波也有帮助，但有上限**：`fSW` 通常是 `200–500kHz` 量级，这个频段固态铝/聚合物电容仍可能呈现较低阻抗，因此它能分担一部分“开关频段的纹波电流”，从而降低 `BRANCH_OUT/UPS_OUT` 的纹波。
  - 但注意：它的效果会受 **`10 mΩ` + 走线寄生电感/电阻 + 电容自身 ESR/ESL** 限制；当这些串联阻抗占主导时，继续加大电容值的边际收益会变小。
- **无法替代的**：`TPS_VOUT`（IC 侧）在“尖峰/振铃”这类更高频（通常远高于 `fSW`，可到 MHz~几十 MHz）的噪声抑制。原因是这些尖峰由最小回路寄生参数决定，远端电容（包括 `BRANCH_OUT` 的 bulk）由于连接电感更大，等效阻抗上不来；因此仍需要在 `VOUT/PGND` 处放 **就地小 MLCC** 来闭合高 di/dt 回路。

结论：**`BRANCH_OUT` 的固态铝/聚合物电容应保留，但不能以此替代 `TPS_VOUT` 处用于闭合开关回路与满足 `COUT_eff` 的 MLCC**。最终以样机在 `TPS_VOUT` 与 `UPS_OUT` 两处的波形测量结果为准。

### 5.4 并联两路时 bulk 电容要不要“每路都留”

结论：**建议每路都预留 bulk 的位置，但不建议在 TPS 侧堆“超大 bulk”**；系统侧（UPS_OUT 接口侧）放 **单颗大 bulk** 作为母线储能与阻尼器件。

当前方案（每路在理想二极管前预留一颗 bulk，接口侧放一颗大 bulk）总体合理；建议按节点分层设计，避免把“回路去耦”和“母线储能”混在一起：

- `TPS_VOUT`（`10 mΩ` 之前）：**一定要**有本地高频去耦（几颗陶瓷，紧贴 `VOUT/PGND`），否则尖峰/振铃/EMI 会被走线寄生支配。
- `BRANCH_OUT`（每路 `10 mΩ` 之后、该路理想二极管之前）：**每路预留并默认装配 `100–470µF` 固态铝/聚合物铝**（当前计划范围）。
  - 推荐默认值：`220µF / 35V`（平衡“瞬态支撑/阻尼/浪涌风险/可买性”）。
- `UPS_OUT`（理想二极管之后、靠近接口）：**单颗大 bulk**（当前恢复的布局条件）。
  - 选型取舍（散货/批次不确定的前提下）：优先 `2200µF/35V` “高频/低阻抗铝电解”；若只能买到 `3300µF/25V`，可作为备选，但要意识到耐压裕量更紧、对“插拔/突卸载振铃”的容忍度更低。

若使用大电流贴片跳线实现两种拓扑切换，建议：

- 默认装配优先选“**每路理想二极管后再合流**”：两路之间天然隔离，更不容易出现“一路异常拖垮另一路”的边界问题。
- “先合流再串单颗理想二极管”建议仅作为备选：两路之间不隔离，更依赖限流分工与对称布局；此时更建议保留“每路理想二极管”的可选焊盘以便后续切回。

### 5.5 参考资料（建议原理图/PCB 评审时逐条对照）

- TPS55288 产品页（datasheet/EVM/app notes 入口）：https://www.ti.com/product/TPS55288
- TPS55288 Layout Guideline（SLVAER0B）：https://www.ti.com/lit/pdf/slvaer0
- How to Achieve Low EMI with TPS55288（SLVAEX2）：https://www.ti.com/lit/pdf/slvaex2
- TPS55288EVM‑045 User Guide（SLVUBO4）：https://www.ti.com/lit/pdf/slvubo4

## 6. 验证计划（样机必须跑完的清单）

> 目标：把“功能可用”推进到“能长期稳定输出且热可控”，并把并联系统风险提前暴露。

### 6.1 功能与切换

- 插拔 VBUS：确认 UPS OUT 电压连续性（无明显掉电/反复启停），并检查 VBUS 侧无倒灌。
- 电池在线/断开：确认故障策略下 UPS OUT 行为符合预期（例如限功率、关断、或降压维持）。

### 6.2 负载与瞬态

- 0A→额定电流的阶跃：检查过冲/下冲与恢复时间；确认不会触发保护或振荡。
- 最坏输入电压（电池低压）下的满载：记录效率、输入电流、关键器件温升（IC/电感/分流电阻/输入输出电容）。
- VIN 波形：用短地弹簧探头在每颗 TPS55288 的 `VIN/PGND` 处测量纹波与振铃（含双路同时大功率与负载阶跃），并据波形决定是否装配 5.2 的 DNP 加容焊盘。
- 边界工况验证：总电流接近 `6.3A`（以及 `5.5–7A` 区间）时，分别观察两颗的 `ISP/ISN` 电流与输出波形，确认不会出现明显的低频交替接管/摆动（例如周期性大幅电流摆动/可闻啸叫/输出纹波显著放大）。

### 6.3 保护路径

- 限流/短路：确认“限流钳制 + 电压下跌”的预期行为；记录器件温升与是否可自恢复。

### 6.4 并联实验（如果要做）

- 仅作为实验：按 4.2 的接法搭建，并验证：
  - 两路分流是否稳定（随温度/电压/负载变化的偏斜程度）；
  - interleave 与非 interleave 的纹波/温升差异；
  - 是否出现低频互相干扰（环路耦合导致的抖动/啸叫/异常热）。

## 7. 相关文档入口（本仓库内）

- 选型与需求：`docs/hardware-selection.md`（2.9 UPS 主输出）
- 充电器子系统：`docs/charger-design.md`
- BMS 子系统：`docs/bms-design.md`
