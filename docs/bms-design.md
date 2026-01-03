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

待定项：

- 充放电 MOSFET 拓扑细化（背靠背方式、栅极保护与拉电阻）
- MOSFET 料号（耐压、`RDS(on)`、封装散热）
- 预充电（PCHG）路径器件与参数

### 3.4 温度检测（TS1~TS4 / PTC）

`BQ40Z50-R2` 提供 `TS1~TS4` 热敏输入与 `PTC/PTCEN` 支持安全 PTC。

待定项：

- NTC 型号与曲线（B 值/表）、分压网络与安装位置（贴电芯/贴板/外置探头）
- 是否启用 PTC 安全检测及其结构落位

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
- `BQ296100` 的 `REG` 管脚在装配/测试阶段对 `VSS` 连接顺序敏感（需要在工艺与测试夹具上规避“VSS 未先接”的风险）
- `CLM1612P1412` 的 heater 激励功耗与触发条件（由外部驱动链路决定；需要把“触发电压/电流/时间”落成可验证指标）

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

## 6. 待定清单（把设计从“可画图”推进到“可下单 BOM”）

1. `Rsense` 阻值/功率与采样布局（与 OC/SC 门限协同）
2. CHG/DSG 主功率 MOSFET 料号、数量与散热结构
3. 预充电路径（PCHG）参数与器件
4. NTC 型号、位置与温度阈值点（充/放电温度窗口）
5. Pack 连接器 pinout 与 ESD/EMI 方案
6. 二次保护触发指标：`BQ296100` → `SI2310A` → `CLM` heater 的电压/电流/触发时间

## 7. 参考资料（本仓库）

- `docs/hardware-selection.md`
- `docs/datasheets/BQ40Z50-R2/`
- `docs/datasheets/BQ296100DSGR/`
- `docs/datasheets/CLM1612P1412/`
- `docs/datasheets/UMW_SI2305A/`
- `docs/datasheets/SI2310A/`

