---
title: 系统设计导读
description: 项目手册中的系统设计入口。
---

# 系统设计导读

本部分回答三个问题：

1. 这台机器由哪些板、总线和功率链路构成。
2. 哪些器件和参数已经冻结，哪些仍是样机阶段约束。
3. 固件在启动时如何把 BMS、充电、输出和前面板串成一条自检链路。

## 1. 章节分工

| 页面 | 重点 |
| --- | --- |
| [系统概览](/design/system-overview) | 连接器、总线、GPIO、启动流程 |
| [电源与 BMS](/design/power-and-bms) | 电池、保护、充电、主输出、遥测 |
| [前面板与固件](/design/front-panel-and-firmware) | 前面板网络、扩展器、`SELF CHECK`、Dashboard |
| [前面板 UI 交互与设计](/design/front-panel-ui-design) | 信息架构、交互路径、状态语义与设计约束 |
| [前面板 UI 图集](/design/front-panel-ui-gallery) | 冻结渲染图、页面家族与全量画面索引 |

## 2. 推荐阅读顺序

1. [系统概览](/design/system-overview)
2. [电源与 BMS](/design/power-and-bms)
3. [前面板与固件](/design/front-panel-and-firmware)
4. [前面板 UI 交互与设计](/design/front-panel-ui-design)
5. [前面板 UI 图集](/design/front-panel-ui-gallery)

## 3. 对应事实源

- [硬件选型总览](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/hardware-selection.md)
- [主板 PCB 说明](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/pcbs/mainboard/README.md)
- [前面板 PCB 说明](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/pcbs/front-panel/README.md)
- [I2C / SMBus 地址映射](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/i2c-address-map.md)
- [开机自检流程](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/boot-self-test-flow.md)
