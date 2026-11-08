# 电池系统预期配置对账清单

本文是 Mains Aegis 电池系统预期配置的核对入口，适用于 EVE INR21700/50E、4S1P、BQ40Z50-R2 与 BQ25792。电芯依据和电池组结构见[电芯与电池组资料](battery-pack.md)，电路说明见 [BMS 设计](bms-design.md)与[充电器设计](charger-design.md)。详细温控合同以[温度 profile Spec](specs/bq40-charge-temperature-profile/SPEC.md)为准。

## 适用范围与配置分类

本清单定义项目期望值，供配置套件、恢复 overlay、固件运行时控制和设备配置核对共同使用。配置读取值、应用记录与验收结论保存于独立报告，不作为本清单的配置来源。

- **项目目标：** 产品配置必须满足的数值或行为要求。
- **主板恢复基线：** 恢复 overlay 应保持的主板固定项，仍须完成对应电气与保护验证。
- **待确定：** 尚无充分依据形成项目数值，不得将原厂示例或任一设备读数自动作为目标。
- **设备个体数据：** 校准、学习与历史状态，必须按设备保存，不作为统一配置值分发。

## 1. 电池组与容量模型

Data Flash 地址均针对 BQ40Z50-R2；多字节值按小端编码，不能直接套用到其他固件版本。`I2` 表示两字节有符号整数，本表中的容量与电压目标均为正值。

| 项目 | 地址／表示 | 项目目标 | 配置说明 |
| --- | --- | --- | --- |
| 电芯与串并联 | 物料、DA Configuration | EVE INR21700/50E，4S1P | 用于物料和串数配置核对 |
| Design Capacity mAh | `0x48E5`，I2，mAh | **5000**，原始字节 `88 13` | 按 4S1P 电芯额定容量配置，串联不增加 Ah |
| Design Voltage | `0x48E9`，I2，mV | **14600** | 4 × 3650mV |
| IT Term Voltage | `0x484A`，I2，mV | **11200**，原始字节 `C0 2B` | 4 × 2800mV；独立于欠压保护 |
| IT Gauging Configuration | `0x4917`，两字节位域 | 恢复基线 `0xD0FE`；`CSYNC=1`，`CELL_TERM=0` | CELL_TERM=0 使用整包终点 |
| ChemID／化学模型 | ChemicalID MAC 与对应模型资产 | **待 TI chemistry matching 证据锁定** | 必须由匹配报告与对应化学模型资产支持 |
| SBS 容量单位 | BatteryMode CAPM 位 | 诊断按 mAh 解码时必须确认 CAPM=0 | 按容量单位位解释数据，不固定 BatteryMode 整字 |

`11200mV = 4 × 2800mV` 是项目正常放电容量仿真终点，比电芯公开的 2.5V 放电终点更保守。它影响可用容量仿真和 SOC 收口；不等于把 CUV 硬保护改为 2.8V。整包达到 11.2V 也不能保证每节都达到或高于 2.8V，电芯不一致时单节保护仍可能先触发。具体语义见 [TI TRM](manuals/BQ40Z50-R2-TRM/BQ40Z50-R2-TRM.md)第 6.2 节。

5000mAh 是设计额定容量，不保证在本项目 2.8V 终点、实际温度和负载下仍能放出 5000mAh；不能据此强行把 FCC 或 Qmax 写成 5000。

## 2. 满充判定与充电器协同

| 项目 | 地址／控制面 | 项目目标 | 配置说明 |
| --- | --- | --- | --- |
| Charge Term Taper Current | `0x4A6D`，I2，mA | **120** | 与 80mA 硬件终止共同验证判定窗口 |
| Charge Term Voltage | `0x4A71`，I2，mV | **100** | 满充资格的电压裕量，不是充电器 VREG |
| Chg Current Threshold | `0x4A8B`，I2，mA | **50** | 与 taper、Quit Current 一起核对 |
| Quit Current | `0x4A8D`，I2，mA | **10** | 用于充放电／静置状态判断 |
| FET Options | `0x4887`，位域 | 恢复基线 `0x19`，`CHGFET=0` | 保护仍可关闭 CHG FET |
| SBS Gauging Configuration | `0x4888`，位域 | 恢复基线 `0x05`，`RSOCL=1` | 按位核对，不从 SOC 推断满充资格 |
| FC／TCA 与 VCT 的映射、判定窗口 | 相关配置与 TRM 条件 | 逐位核验，禁止仅以 SOC99/100 推断满充 | 目标位值待确定；须满足联合终止要求 |
| BQ25792 ITERM | 运行时寄存器 | **80mA** | 按实际寄存器编码核对 |
| BQ25792 EN_TERM | 运行时寄存器 | 普通充电最终目标建立后启用；启动爬坡暂禁；100mA 低流模式按现有安全路径禁用 | 按充电阶段核对 |
| 普通目标 ICHG | 固件状态机 | **500mA**，继续受 BMS、输入能力、温度限制 | 普通 SOC99/HV 阶段不采用 200mA 补电分支 |
| 低流与维护唤醒 | 固件状态机 | 保留 **100mA** 模式、受限的维护 **200mA** 唤醒 | 维护唤醒不作为普通补电策略 |
| VREG | 固件与 BQ25792 | **不超过 16800mV**，且不超过有效 BMS 请求 | 上限为 16800mV；不要求每时每刻等于此值 |
| Current at EoC | `0x431B`，mA | 保留历史值，不用于计算 ITERM | 设备历史，不作为配置套件写入目标 |

BQ40 的满充判定 taper 目标为 120mA，充电器硬件终止目标为 80mA；有效满充资格还须满足电压、平均电流及判定窗口等条件。BQ40 资格成立后仍允许充电器完成尾段；最终 FULL 要求当前充电会话的有效满充资格和 `charger_done` 同时满足。FC 单独出现不提前切断这段过程。若 `charger_done` 先到而资格未成立，停充并记录 `termination_incomplete`，仅显式 START 且安全条件满足后重试；零请求和保护始终优先。详见[联合终止决策](adr/0003-charge-termination-coordination.md)。

## 3. 温度与充电请求

| 项目 | 地址／来源 | 目标 |
| --- | --- | --- |
| Temperature Enable | `0x4A7B` | `0x1E`，使能 TS1～TS4，内部温度不作为该配置的充电输入 |
| Temperature Mode | `0x4A7C` | `0x00` |
| 温度传感器 | 原理图与温控 Spec | 10kΩ@25°C、β3380；TS1 板侧，TS2～TS4 电芯间 |
| 电芯温度决策 | 固件 | 取 TS2～TS4 最高有效温度；板侧 TS1 不用于放宽电芯限制 |
| 允许充电区间 | 固件与 profile | `0°C ≤ T < 45°C`；0～15°C 为 100mA，15～45°C 为 500mA，其余禁充 |
| 温度区间内的请求电压表 | BQ40 profile | 目标 16800mV；实际 ChargingVoltage 请求可更低，必须另外核验其限制来源 |
| BQ40 电流表 | LT Low/Med/High；STL/RT/STH/HT | LT 各项 100mA；其余允许区间各项 500mA；不得仅核验温度边界 |
| 请求及温度有效期 | SBS ChargingCurrent/ChargingVoltage、固件 | 新鲜有效证据不超过 1s；失败、过期、零请求、配置不符时禁充 |
| BQ25792 独立 NTC | 充电器电路 | 同曲线 NTC，RTS1=5.23kΩ、RTS2=30.1kΩ；JEITA 正常窗口 15～45°C |

| DF 字段 | 地址 | 摄氏目标 | 原始目标（0.1K，Hysteresis 为温差） |
| --- | --- | --- | --- |
| T1 | `0x4A0B` | 0°C | 2732 |
| T2 | `0x4A0D` | 15°C | 2882 |
| T5 | `0x4A0F` | 20°C | 2932 |
| T6 | `0x4A11` | 35°C | 3082 |
| T3 | `0x4A13` | 40°C | 3132 |
| T4 | `0x4A15` | 45°C | 3182 |
| Hysteresis | `0x4A17` | 2°C 温差 | 20 |

温度 profile 的白名单仅包含上述七个字段。温度电流表、电压表和其他 DF 字段由各自配置流程负责，不得扩大温度 writer 的写入范围。

## 4. 保护、主板固定项与均衡

以下列出项目保护要求与主板恢复基线。保护阈值必须与延时、恢复条件、使能位一起核验；缺少任一项不能宣称保护配置完整。

| 项目 | 地址／范围 | 项目要求或已有基线 | 核验边界 |
| --- | --- | --- | --- |
| CUV Threshold | `0x493C`，mV/节 | **2500** | 按单节最低电压保护；不随 Term Voltage 修改 |
| CUV Recovery | `0x493F`，mV/节 | **2550** | 与 CUV trip、delay、使能和恢复条件共同核对 |
| CUVC Recovery | CUVC 保护组 | **3000mV/节** | 设计要求；补齐 trip、delay、补偿条件和现场读回 |
| Protection Configuration | `0x4937` | `0x00`，CUV_RECOV_CHG=0 | 主板恢复基线 |
| OCC1／OCC2 | 保护组 | 4500mA／6s；5200mA／3s | 代码恢复基线，不能代替板级保护验证 |
| OCD1／OCD2 | 保护组 | −14500mA／6s；−15000mA／3s | 代码恢复基线；符号按 BQ40 电流方向 |
| OCD Recovery | 保护组 | Threshold=100mA，Delay=3s | 保留 TRM 原字段语义，不自行改为 −100mA |
| SOCC／SOCD | PF 保护组 | 6000mA／5s；−16000mA／5s | 代码恢复基线；永久故障链路不可当作常规限流 |
| OC 容量过充 | `0x49A9/0x49AB/0x49AD` | 待确定；不属于 DesignCapacity／Term Voltage 定向修复 | 目标值待确定；保护条件须独立评审，不由容量模型修复自动改写 |
| COV、SCC、SCD、CUVC trip/delay；温度保护及恢复 | 各保护组 | **量产值与实机完整核验待补** | 不把默认表数值升级为项目已验证目标 |
| Enabled Protections A～D／Enabled PF A～D | 各使能位 | **逐位对账待补** | 启用 PF/FUSE 总开关不等于各保护子项已核验 |
| Manufacturing Status Init | `0x4600` | 恢复基线 `0x0378` | 配置初值不等于实时 ManufacturingStatus |
| DA Configuration | `0x4A7D` | `0x8127`：4S，NR=1，EMSHUT_EN=1，SLEEP=0 | 恢复基线；NR 是逻辑包模式，不授权热插拔 |
| SBS Configuration／Auth Config | `0x4889/0x488A` | `0x20/0x00` | 恢复基线 |
| AFE Protection Control | `0x4A80` | `0x70`，RSTRIM=7，RSNS=0，SCDDx2=0 | 恢复基线；硬件阈值须按对应 Rsense 表解释 |
| Balancing Configuration | `0x4908` | `0x07` | 恢复基线 |
| Min Start Balance Delta | `0x490D` | 3mV | 恢复基线，不等于电压测量精度 |
| Relax Balance Interval | `0x490E` | 18000s | 恢复基线 |
| Min RSOC for Balancing | `0x4912` | 80% | 恢复基线 |
| 外部被动均衡 | PMOS＋16Ω，≥2W | 平均 200mA 设计目标 | 未取得独立支路电流验收；mask 不是均衡电流测量 |
| 二次过压保护 | BQ296100＋CLM1612P1412 | 4.35V/节，4S／12A 熔断器件 | 硬件独立链路；器件规格不等于系统连续输出额定值 |

## 5. 学习、历史与校准数据

| 数据 | 分类 | 配置管理要求 |
| --- | --- | --- |
| Qmax Cell1～4／Pack | 设备学习数据 | 保留；不得随 DesignCapacity 修改强行重写为额定容量 |
| Ra 表及更新计数 | 设备学习数据 | 按设备保存；非零更新计数不等于模型已完成资格验证 |
| UpdateStatus（`0x4312`） | 学习状态 | 记录并解释，不对所有设备强制写相同状态 |
| DOD at EoC、Current at EoC | 历史参考 | 保留；不得作为当前阈值配置来源 |
| FCC、TrueFCCQ、TrueRemQ | 电量计输出 | 按温度、负载和模型解释，不作为固定写入目标 |
| 校准系数 | 设备个体数据 | 保存来源、测量条件和校准结果，不以通用镜像覆盖个体校准 |

修改 DesignCapacity 和 Term Voltage 不默认清理学习数据。配置应用前后必须保存学习、校准与历史数据；是否重新匹配化学模型或重新学习，由独立的模型资格验证流程决定。整镜像恢复可能覆盖学习数据，不作为保留学习的定向配置方法。

## 6. 配置套件与恢复一致性

| 配置路径 | 写入范围 | 一致性要求 |
| --- | --- | --- |
| `bq40-charge-temperature` | T1、T2、T5、T6、T3、T4、Hysteresis 七个字段 | 保持独立白名单，不修改容量、化学模型或保护 |
| `bq40-charge-termination` | 仅 `0x4A6D` 两字节 taper | 目标 120mA；校验 `0x4A60` 的 32 字节块，其余字节不变 |
| `bq40-capacity-model` | DesignCapacity（`0x48E5`）、IT Term Voltage（`0x484A`） | 仅写 5000mAh、11200mV；写前同时保存 `0x48E0`、`0x4840` 两个 32 字节块，写完后复查两块，其他字节不变 |
| `asset-df-mainboard` 恢复 overlay | 项目固定配置补丁 | 必须与本清单的已确定目标一致；不能把原厂示例字段当作项目完整 golden image |
| 原厂二进制资产 | 厂商恢复来源 | 保持原样，以项目 overlay 表达差异 |
| 正常产品固件 | DF 读取与运行时充电寄存器控制 | 不自动写 DF |

DF 写入前核验 TI／BQ40Z50-R2 身份、Full Access、CE 高及禁充。容量 profile 还须确认 4S 与 CELL_TERM=0。分别保存包含目标字段的块快照，校验地址、长度、回显、解码、最终读回和所有非目标字节；失败即停止，不自动整块覆盖回滚。只经绑定设备的 mains-aegis CLI/devd catalog、选择、dry-run、flash 生命周期操作，完成后恢复正式候选固件并核验身份与版本。

## 7. 配置核对与资格验证要求

配置核对必须覆盖字段地址、数据类型、单位、位域含义、目标值、写入边界和恢复一致性。包含“待确定”的项目不得使用默认值补齐并宣称完成项目配置。

设备应用记录必须独立保存身份、固件标识、采集时间、原始 DF、解析值与判定结果。保护项须覆盖 trip、delay、recovery、enable；满充项须覆盖配置位、平均电流与判定窗口；温控项须覆盖边界和电流／电压表。

资格验证应在同一候选固件下每 2s 采样、连续 10min，记录 fresh Current、AverageCurrent、均衡 mask、安全状态及各自采样时间。自然充电观察最长 12h，完整捕获满充资格、硬件终止与终止后 10min；保护中断、超时、读取错误、缺样和未覆盖的 mask 必须如实记录，不以停充后的稳定观察替代完整充电验证。

## 8. 依据与维护入口

- [电芯与电池组资料](battery-pack.md)：电芯公开依据与电池组参数。
- [BMS 设计](bms-design.md)：主板 DF 基线及保护设计。
- [充电器设计](charger-design.md)与[联合终止决策](adr/0003-charge-termination-coordination.md)：运行时控制要求。
- [温度 profile Spec](specs/bq40-charge-temperature-profile/SPEC.md)：温控字段与 writer 合同。
- [BQ40Z50-R2 TRM](manuals/BQ40Z50-R2-TRM/BQ40Z50-R2-TRM.md)：字段地址、类型与算法语义。
- [配置套件实现](../tools/bq40-comm-tool/firmware/src/output/mod.rs)：写入白名单与恢复 overlay；实现差异须独立记录，不反向改变本清单的目标。
