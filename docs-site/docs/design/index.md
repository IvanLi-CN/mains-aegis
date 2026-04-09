---
title: 系统设计导读
description: 项目手册中的系统设计入口。
---

# 系统设计导读

本部分回答四个问题：

1. 这台机器由哪些板、总线和功率链路构成。
2. 哪些器件和参数已经冻结，哪些仍是样机阶段约束。
3. 固件在启动时如何把 BMS、充电、输出和前面板串成一条自检链路。
4. 前面板屏幕会依次出现哪些页面，用户应该去哪里读每一页的含义。

## 1. 章节分工

| 页面 | 重点 |
| --- | --- |
| [系统概览](/design/system-overview) | 连接器、总线、GPIO、启动流程 |
| [电源与 BMS](/design/power-and-bms) | 电池、保护、充电、主输出、遥测 |
| [前面板与固件](/design/front-panel-and-firmware) | 前面板硬件链路、扩展器、运行时基线 |
| [前面板屏幕页面总览](/design/front-panel-screen-pages) | 从 `SELF CHECK` 到 Dashboard、详情页与 `MANUAL CHARGE` 的完整页面地图 |
| [前面板 UI 交互与设计](/design/front-panel-ui-design) | 热区、状态语义、页面切换规则与设计约束 |

## 2. 推荐阅读顺序

1. [系统概览](/design/system-overview)
2. [电源与 BMS](/design/power-and-bms)
3. [前面板与固件](/design/front-panel-and-firmware)
4. [前面板屏幕页面总览](/design/front-panel-screen-pages)
5. [前面板 UI 交互与设计](/design/front-panel-ui-design)

## 3. 对应事实源

- [硬件选型总览](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/hardware-selection.md)
- [主板 PCB 说明](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/pcbs/mainboard/README.md)
- [前面板 PCB 说明](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/pcbs/front-panel/README.md)
- [I2C / SMBus 地址映射](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/i2c-address-map.md)
- [开机自检流程](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/boot-self-test-flow.md)
- [Front panel UI docs](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/ui/README.md)
