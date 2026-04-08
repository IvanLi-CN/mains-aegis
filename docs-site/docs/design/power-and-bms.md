---
title: 电源与 BMS
description: 电池包、BMS、充电、UPS 输出与监测链路总览。
---

# 电源与 BMS

Mains Aegis 的主价值基本都建立在电源路径上：它既要管理 `4S1P` 电池包，又要处理输入侧充电、对外主输出、保护与可观测性。所以理解这部分设计，是继续复刻或继续开发的前提。

## 电池包与 BMS

当前系统按 `4S1P` 21700 锂电池包设计，仓库中已经明确记录了：

- 电芯方向：EVE `21700/50E`
- 主 BMS：`BQ40Z50-R2`
- 二级过压保护：`BQ296100DSGR`
- 二次保护链路：`CLM1612P1412` + 对应驱动器件
- 外部被动均衡：目标约 `200mA`

这里最关键的认知不是“器件名”，而是**系统边界**：BMS 负责计量与保护，但它并不等于全部电源系统；充电、主输出、前面板与固件仍有各自的职责。

## 充电链路

当前仓库把充电设计集中在 `BQ25792` 路线上，并明确考虑双输入、Type-C / PD(PPS) 与 DC 输入共存的路径。

你阅读这部分时要注意两个事实：

1. 充电器设计文档已经把很多边界冻结得比较清楚，例如输入类型、功率路径、TS/JEITA 与 bring-up 关注点。
2. 但这并不等于所有外围器件都已经变成“最终量产 BOM”；有些内容仍保持候选或待定，文档会明确写出来。

## UPS 主输出

项目对外的 UPS 主输出与充电器的 `SYS/VSYS` 不是一回事。当前仓库把它视作一个独立的系统级问题来处理：

- 面向外部负载的 `UPS OUT`
- 目标版本包含 `12V` 与 `19V`
- 主线实现目前围绕两路 `TPS55288` 与相关理想二极管/功率路径展开
- 具体并联/热/补偿等问题仍需要结合样机验证来理解

如果你只是想知道“项目现在在往哪里走”，记住一点就够：**主输出是系统级受控输出，不是顺手从 charger 芯片上借出来的一根电源线。**

## 监测与保护

当前主板已经围绕以下器件建立了可观测与保护链路：

- `INA3221`：多路电压/电流监测
- `TMP112A`：热点温度监测
- `TPS2490`：外部输入防护 / 热插拔路径
- 固件侧自检、Dashboard 与运行时日志

这意味着项目不仅关心“能不能出电”，也关心**怎么知道它现在是否健康**。

## 阅读这部分时的一个重要原则

不要把“已在仓库里存在网表/代码/文档”自动理解成“量产结论已经完全冻结”。更准确的读法是：

- 已选：当前主线事实已经明确，可以用来继续推进。
- 候选：方向明确，但仍需要更多验证或条件确认。
- 待定：仓库知道这是缺口，但还没有假装它已经解决。

## 延伸阅读

- [BMS 设计](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/bms-design.md)
- [充电器设计](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/charger-design.md)
- [UPS 主输出设计](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/ups-output-design.md)
- [电源监测与保护设计](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/power-monitoring-design.md)
- [I2C / SMBus 地址映射](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/i2c-address-map.md)
