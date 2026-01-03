# BMS 设计（mains-aegis）

本文档用于把本项目的 BMS（Battery Management System）设计从“分散的选型与数据手册摘录”收敛为**一份可落地的系统级设计说明**，便于后续：

- 原理图/PCB 绘制与评审
- BQ40Z50-R2 参数配置（Data Flash / Chemistry / Calibration）
- 二次保护链路（OVP → 熔断）联调与验证
- 生产测试与故障定位

> 适用范围：当前项目电池包为 `4S1P`（4 串 21700 三元锂）。更改串数/并数后需要重新评估本设计中的器件能力边界、耐压与布局规范。

## 1. 结论与选型边界

### 1.1 电池包与目标能力

- 电池包：`4S1P`（电芯：EVE 亿纬 `EVE-INR21700/50E`）
- 单节满充/截止放电：`4.2 V / 2.5 V`
- 整包满充/截止放电：`16.8 V / 10.0 V`
- BMS 目标能力：
  - SOC/SOH 等电量计（gas gauging）与主机上报
  - 充放电路径保护（OV/UV/OC/SC/OT 等，具体阈值待定标）
  - 外部被动均衡（目标均衡电流 `200 mA`）
  - 二次保护（独立 OVP 监测 + 熔断器件）

电池包的更完整背景与系统电流估算见：`docs/hardware-selection.md`。

### 1.2 关键器件（本项目采用）

| 分块 | 关键器件 | 状态 | 说明 | 资料 |
|---|---|---|---|---|
| 主 BMS（电量计 + 保护） | `BQ40Z50RSMR-R2` | 已选 | 1~4S pack manager；SMBus v1.1；支持 FET drive、均衡与熔断输出 | `docs/datasheets/BQ40Z50-R2/` |
| 二级过压保护（OVP） | `BQ296100DSGR` | 已选 | 2~4S OVP；`OVP=4.35 V/cell`；`REG=3.3 V` | `docs/datasheets/BQ296100DSGR/` |
| 二次保护（限流/熔断） | `CLM1612P1412` | 已选 | 4S / 12A；带 fuse element + heater element，可用于外部触发熔断 | `docs/datasheets/CLM1612P1412/` |
| 均衡 PMOS（外部） | `UMW SI2305A` | 已选 | TI 外部均衡 PMOS 拓扑用 | `docs/datasheets/UMW_SI2305A/` |
| Heater 触发 NMOS | `SI2310A` | 已选 | 驱动 `CLM1612P1412` heater（由 OVP/外部检测电路控制） | `docs/datasheets/SI2310A/` |

### 1.3 明确不采用（清理项）

- 不采用 `BM3451TJDC-T28A`（无电量计/无 SMBus 主机上报能力，与本项目“电量计 + 上报”的目标不匹配）。

## 2. 系统架构

### 2.1 结构分层

- **主保护/计量层**：`BQ40Z50RSMR-R2`
  - 计量：库仑计数 + Impedance Track™（SOC/SOH/剩余容量等）
  - 保护：基于电压/电流/温度的可配置保护与故障状态上报
  - 控制：CHG/DSG FET drive、PCHG（预充）控制、均衡控制
  - 通信：SMBus v1.1（`SMBC/SMBD`）
- **二次保护层（独立链路）**：`BQ296100DSGR` + `CLM1612P1412` + `SI2310A`
  - 当主 BMS 失效/失配（例如软件配置错误、单点失效）时，仍可对过压等灾难性风险做熔断隔离

### 2.2 关键电气链路（文字版框图）

```
Cells (4S1P) ── VC taps ─────────────┐
                                     │
                                 BQ40Z50
                                     │  SMBus (SMBC/SMBD) ── Host/Charger
BAT+ ──(CHG/DSG back-to-back N-MOS)───┤
                                     │
BAT- ── Rsense (SRP/SRN) ────────────┘

Cells (4S) ── sense ── BQ296100 ── OUT ── SI2310A ── CLM heater ── blow CLM fuse
```

> 说明：上图表达“主要信号流与保护链路”，实际原理图应严格按 `BQ40Z50-R2` 参考设计拆分为：VC 采样滤波、差分电流采样、SMBus 防护、FET gate 网络、均衡网络与熔断网络等小回路。

## 3. 电气设计要点（可直接指导原理图）

### 3.1 串数与 VC 采样网络（4S）

- 串数：`4S`（`BQ40Z50-R2` 支持 1~4S）
- VC 采样滤波：按已选方案（来自 `docs/hardware-selection.md`）
  - `Rvc = 100 Ω`（每个 `VCx` 串联电阻）
  - `Cvc = 0.1 µF`（贴近 IC 放置；建议 `X7R`、`≥10V`）

### 3.2 电流采样（Rsense / SRP / SRN）

`BQ40Z50-R2` 通过 `SRP/SRN` 采样用于库仑计量与过流/短路相关保护。

待定项（需要和系统电流目标一起定标）：

- `Rsense` 阻值（目标：兼顾量程、压降、热与精度）
- `Rsense` 封装/功率（建议 Kelvin 采样布局）

### 3.3 充放电功率开关（CHG/DSG / 预充）

本项目按 `BQ40Z50-R2` 的高边 N-CH protection FET drive 思路设计（常见为高边背靠背 N-MOS），并配套预充电路径。

#### 3.3.1 预充电（PCHG）限流电阻 R1（已定）

`BQ40Z50-R2` 数据手册给出：若使用预充电 FET，则 `R1` 用于限制预充电电流（`Ipre=(VCHARGER−VBAT)/R1`），并需要校核串联电阻功耗（`Pmax=(VCHARGER−VBAT)^2/R1`，见 `docs/datasheets/BQ40Z50-R2/BQ40Z50-R2.md:661`）。

本项目决定采用 TI 参考电路的 `R1=300 Ω`，并以两颗电阻串联实现：

- `R1 = 150 Ω (2512) + 150 Ω (2512)` 串联（等效 `300 Ω`）
- 充电输入：4S 专用充电器 `VCHARGER≈16.8 V`
- 极限校核（`VBAT→0`）：`Imax≈16.8/300≈56 mA`；`Pmax,total≈16.8^2/300≈0.94 W`；单颗电阻分摊约 `0.47 W`
- 低压边界（`VBAT≈10.0 V`）：`Ipre≈(16.8−10.0)/300≈22.7 mA`；`Ptotal≈0.15 W`

器件与布局建议：

- 每颗 `150 Ω / 2512` 建议按 **≥1 W@70°C** 规格选型，并考虑温升与降额曲线
- 预留足够铜皮散热，尽量远离主功率 MOSFET 与热源，避免高温降额后余量不足

待定项：

- 充放电 MOSFET 拓扑细化（背靠背方式、栅极保护与拉电阻）
- MOSFET 料号（耐压、`RDS(on)`、封装散热）
- 预充电（PCHG）路径器件（P-MOS 与门极网络等，除 R1 外）

### 3.4 温度检测（TS1~TS4 / PTC）

`BQ40Z50-R2` 提供 `TS1~TS4` 热敏输入与 `PTC/PTCEN` 支持安全 PTC。

本项目最终选型与用法：

- `TS1`（板上风险点温度，TS_BOARD）：`FNTC0402X103F3380FB`（`10 kΩ@25°C(103)`，`β=3380`，0402）
- `TS2/TS3/TS4`（电芯间温度）：`10 kΩ@25°C(103)`，`β=3380`（封装/料号可不同，但必须同一条 R-T 曲线）
- 连接方式：每路 `TSx` 直接接一颗 NTC 到 `VSS/BGND`（不需要外部分压电阻；`BQ40Z50-R2` 内部提供热敏上拉/驱动）
- `PTC`（安全 PTC，贴近 CHG/DSG FET）：`Murata PRF18BA103QB1RB`（或同等规格）
  - 选型约束：正常工作温区 `RPTC ≈ 10 kΩ`；在 PTC 触发温度点 `RPTC > 1.2 MΩ`
  - 连接方式：按 `BQ40Z50-R2` 参考电路，PTC 元件接在 `PTC` 与 `BAT` 之间，`PTCEN` 接 `BAT` 使能

> 备注：`BQ40Z50-R2` 只提供两套可配置 thermistor profile（cell / FET）。因此最多支持“两条不同曲线”；本项目统一使用 `β=3380`，便于 TS1~TS4 共用同一条曲线并降低 DF 配置复杂度。

### 3.5 均衡（外部被动均衡，200 mA）

本项目采用 TI 外部均衡 **PMOS** 拓扑（`BQ40Z50` 内部均衡开关 + 外部 PMOS + `Rext`），并固定目标均衡电流：

- 目标均衡电流：`200 mA`（平均值）
- 外部均衡 PMOS：`UMW SI2305A`
- 外部均衡电阻：`Rext = 16 Ω`（按 `Vcell≈4.2V` 估算）
  - 推荐封装：`2512`
  - 功率：`≥2W`
  - 精度：`1%`

> 热设计提示：按 `I≈0.2A`、`R=16Ω` 估算，电阻平均功耗约 `0.64W`，需要结合 PCB 铜皮/风道/热隔离做校核。

## 4. 二次保护链路（OVP → 熔断）

### 4.1 目标

构建一条**不依赖主 BMS 软件配置**的过压熔断链路，用于覆盖主控失效、配置失配等极端场景。

### 4.2 器件职责

- `BQ296100DSGR`：2~4S 逐节过压监测与故障输出（本项目选型参数：`OVP=4.35V/cell`、`REG=3.3V`）
- `SI2310A`：受 `BQ296100` 输出控制，对 `CLM1612P1412` 的 heater 端供电（触发熔断）
- `CLM1612P1412`：二次保护器件，正常电流路径走 fuse element；当 heater 被激励产生热量时熔断

### 4.3 关键约束（设计时必须显式评审）

- `BQ296100` 的 OVP delay（数据手册中对 `BQ296100` 标注为 `6.5s`）与系统可接受的反应时间
- `BQ296100` 的 `REG` 管脚在 `VSS` 未先接/浮空时存在损伤与过压风险，需要按数据手册对 `REG` 支路做限流与可选钳位设计（见 4.4）
- `CLM1612P1412` 的 heater 激励功耗与触发条件（由外部驱动链路决定；需要把“触发电压/电流/时间”落成可验证指标）

### 4.4 BQ296100 `REG` 引脚保护（连接顺序不可控）

本项目电池单元连接顺序**无法确保**。按 TI `BQ296xxx` 数据手册（`docs/datasheets/BQ296100DSGR/`，8.2.2 与图 8-3）的建议，`REG` 管脚采用以下处理：

- `CREG`：`0.47 µF`（可到 `1 µF`），陶瓷电容，贴近 `BQ296100` 放置（用于 `REG` 稳定性）。
- `RREG`：在 `CREG` 串联 `5–10 Ω`（参考值 `5 Ω`），用于在 `VSS` 不是先接的情况下限制 `CREG` 放电浪涌，避免损伤 `REG` 引脚。
- `DREG_ZENER`（预留焊盘）：`REG → VSS` 的齐纳钳位，`Vz≈5.1 V`（数据手册示例为 5V），用于在 `VSS` 浮空或 `REG` 被拉到高压（如短到 `OUT`）时，限制 `REG` 电压以保护 `REG` 下游电路。
  - 当前设计不使用 `REG` 为外部电路供电：齐纳默认 `DNP`，仅预留焊盘以便后续验证/扩展。
  - 若后续决定实装齐纳且需要向外供电，可评估将 `RREG` 改为 `0 Ω`/不装（数据手册指出使用齐纳时串阻可不需要），以避免对 `VREG` 引入额外压降。

## 5. 主机通信与软件配置（BQ40Z50-R2）

### 5.1 SMBus 接口

- 使用 `SMBC/SMBD` 与主机通信（SMBus v1.1）
- 需要在连接器侧与走线侧考虑 ESD/浪涌与 EMI（`BQ40Z50-R2` 数据手册给出了更强健的外部 ESD 方案示例）

待定项：

- Pack 连接器定义（SMBus、PACK+/PACK-、温度/ID 等 pinout）
- ESD 器件/串联电阻选型与布局

### 5.2 量产配置流程（必须项）

`BQ40Z50-R2` 在进行任何 gauging/保护验证前，应完成：

- 校准（Calibration）
- Chemistry profile 匹配（Chemistry）
- 设计参数写入（Data Flash / Design parameters）

其中“设计参数”的典型字段在 `BQ40Z50-R2` 数据手册中给出了示例（如 Cell Configuration、Design Capacity、Cell OV/UV、OC/SC 等）；本项目的落值需要与以下信息对齐：

- 电芯/电池包参数（见 `docs/hardware-selection.md`）
- 系统电流目标（连续/峰值放电，充电电流目标）
- 安全策略（各类保护阈值与延时、是否启用均衡、温度窗口）

### 5.3 温度相关配置方法（TS1~TS4 / NTC 模型）

温度保护的“配置”不靠硬件电阻值自动适配，而是靠 **BQSTUDIO 通过 SMBus 写 Data Flash（DF）** 的方式，把“TS 引脚电压 → 温度(°C)”的热敏曲线模型与温度保护阈值固化为一份 **golden image**（开发/量产共用）。

本项目建议配置策略（配合 3.4 的布点）：

- 传感器启用：
  - 启用 `TS1~TS4`
  - 未使用的温度通道必须在 DF 中禁用，并且硬件端接 `VSS`
- 传感器用途映射：
  - `TS1`：配置为 **FET/board 风险点温度**（用于更快反映板上 MOSFET/Rsense/铜皮等温升风险）
  - `TS2/TS3/TS4`：配置为 **cell 温度**（用于充/放电温度窗口与电芯安全保护）
- 热敏曲线模型（thermistor profile）：
  - 将 cell / FET 两套模型都配置为 `10 kΩ@25°C(103), β=3380` 对应的曲线（本项目 TS1~TS4 统一曲线）
  - 若未来确需 TS1 与电芯间 NTC 使用不同曲线：使用 “FET 模型” 绑定 TS1、用 “cell 模型” 绑定 TS2~TS4（仍只能支持两条曲线，无法让 TS2/TS3/TS4 各自不同）
- `PTC`（安全 PTC）：
  - 启用 `PTC/PTCEN` 安全 PTC 检测，并按参考设计连接至 `BAT`
  - 注意：`PTC fault` 属于永久故障类，通常只能通过 `POR` 清除；因此 PTC 的触发温度应高于“正常温控关断（可恢复）”的阈值，用作最后一道安全闩锁

温度阈值（充/放电高温/低温、延时、恢复点）需结合电芯规格书与系统功耗/散热做定标后落到 DF（此处不写死，避免后续版本变更造成误导）。

## 6. 待定清单（把设计从“可画图”推进到“可下单 BOM”）

1. `Rsense` 阻值/功率与采样布局（与 OC/SC 门限协同）
2. CHG/DSG 主功率 MOSFET 料号、数量与散热结构
3. 预充电路径（PCHG）器件（P-MOS 与门极网络等，除 R1 外；R1 已定为 `2×150 Ω 2512` 串联）
4. 温度：NTC 已定 `10k(103) β3380`（TS1 料号 `FNTC0402X103F3380FB`；TS2~TS4 同曲线）；待定：充/放电温度阈值点与延时/恢复点（充/放电温度窗口）
5. Pack 连接器 pinout 与 ESD/EMI 方案
6. 二次保护触发指标：`BQ296100` → `SI2310A` → `CLM` heater 的电压/电流/触发时间

## 7. 参考资料（本仓库）

- `docs/hardware-selection.md`
- `docs/datasheets/BQ40Z50-R2/`
- TI `BQ40Z50EVM` User's Guide（SLUUAV7，用于参考 PTC 选型与参考电路）
- `docs/datasheets/BQ296100DSGR/`
- `docs/datasheets/CLM1612P1412/`
- `docs/datasheets/UMW_SI2305A/`
- `docs/datasheets/SI2310A/`
