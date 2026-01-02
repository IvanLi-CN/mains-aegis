# BMS 对比：BM3451TJDC-T28A vs BQ40Z50-R2

本文对比 `BM3451TJDC-T28A` 与 `BQ40Z50-R2`，仅基于仓库内已收录的数据手册（PDF/Markdown）进行整理，用于支撑本项目技术选型。

## 1. 参考资料（本仓库）

- TI `BQ40Z50-R2` datasheet
  - `docs/datasheets/BQ40Z50-R2/BQ40Z50-R2.pdf`
  - `docs/datasheets/BQ40Z50-R2/BQ40Z50-R2.md`
- BYD Microelectronics `BM3451TJDC-T28A` datasheet
  - `docs/datasheets/BM3451TJDC-T28A/BM3451TJDC-T28A.pdf`
  - `docs/datasheets/BM3451TJDC-T28A/BM3451TJDC-T28A.md`
- 既有器件级说明（BM3451）：`docs/hardware-selection/bms-bm3451tjdc-t28a.md`

## 2. 核心差异（先给结论）

你看重的“电量计（gas gauging）”能力是分水岭：

- `BQ40Z50-R2`：datasheet 明确为 pack manager，集成电量计（Impedance Track™）、保护与认证，并通过 SMBus 向主机上报容量/电压/电流/温度等信息。
- `BM3451TJDC-T28A`：datasheet 定位为 3/4/5S 保护 + 被动均衡控制芯片（`CO/DO` 栅极控制等），引脚定义中没有 SMBus/主机通信接口；要做电量计需要另加电量计芯片或系统侧另行实现/测量。

## 3. 对比表（按系统影响排序）

| 维度 | `BQ40Z50-R2`（已选） | `BM3451TJDC-T28A`（候选） |
|---|---|---|
| 串数范围（datasheet） | 1~4S | 3/4/5S（可级联扩展更高串数） |
| 电量计（SOC/SOH 等） | 有：Impedance Track™ + 库仑计数（Coulomb Counter）等 | 无（datasheet 未提供电量计/主机上报能力） |
| 主机通信 | 有：SMBus v1.1（`SMBC/SMBD` 引脚） | 无（引脚定义未见 SMBus/I²C/SPI/UART） |
| 保护功能形态 | 软件可配置的多级保护（OV/UV/OC/SC/OT 等）+ 永久故障/熔断相关机制 | 硬件阈值/延时（外接电容）+ `CO/DO` 控制 MOSFET 关断 |
| MOSFET 驱动 | CHG/DSG（高边 N-CH）+ PCHG 预充电（外部 P-CH）等参考电路 | `CO/DO` 栅极输出（最高 12 V），常见配背靠背 MOSFET |
| 均衡能力 | 支持 cell balancing（含内部均衡与外部增强选项） | 被动均衡：`BAL1~BAL5` 驱动外部均衡放电回路 |
| “二次保护/熔断”联动 | 具备 FUSE 点火控制相关描述（用于永久禁用电池包） | datasheet 未描述 fuse 点火/熔断驱动 |
| 系统复杂度（倾向） | 更像“电池包系统”：需要 SMBus 主机侧/参数配置流程 | 更像“纯保护板”：外围简单、无主机协议栈依赖 |

## 4. 选型建议（面向本项目 4S1P）

- 如果目标是“电量计 + 上报 + 可配置保护”的整包体验：`BQ40Z50RSMR-R2`（对应 datasheet `BQ40Z50-R2`）更匹配。
- 如果目标是“只做保护/均衡”、且希望无 SMBus 依赖或未来可能 >4S：`BM3451TJDC-T28A` 仍可作为备选方案保留。

