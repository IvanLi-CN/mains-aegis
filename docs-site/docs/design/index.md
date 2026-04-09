---
title: 系统设计导读
description: 项目手册中的系统设计入口。
---

# 系统设计导读

系统设计部分按硬件结构、功率链路和前面板运行路径展开。比较顺手的读法，是先看整机结构，再看电源链，最后回到前面板硬件和屏幕页面。

## 1. 各页分别讲什么

| 页面 | 重点 |
| --- | --- |
| [系统概览](/design/system-overview) | 连接器、总线、GPIO、启动流程 |
| [电源与 BMS](/design/power-and-bms) | 电池、保护、充电、主输出、遥测 |
| [前面板与固件](/design/front-panel-and-firmware) | 前面板硬件链路、扩展器、运行时基线 |
| [前面板屏幕页面总览](/design/front-panel-screen-pages) | 从 `SELF CHECK` 到 Dashboard、详情页与 `MANUAL CHARGE` 的页面路径 |
| [前面板 UI 交互与设计](/design/front-panel-ui-design) | 热区、状态语义、页面切换规则与设计约束 |

## 2. 建议顺序

1. [系统概览](/design/system-overview)
2. [电源与 BMS](/design/power-and-bms)
3. [前面板与固件](/design/front-panel-and-firmware)
4. [前面板屏幕页面总览](/design/front-panel-screen-pages)
5. [前面板 UI 交互与设计](/design/front-panel-ui-design)

## 3. 仓库内参考文档

- [硬件选型总览](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/hardware-selection.md)
- [主板 PCB 说明](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/pcbs/mainboard/README.md)
- [前面板 PCB 说明](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/pcbs/front-panel/README.md)
- [I2C / SMBus 地址映射](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/i2c-address-map.md)
- [开机自检流程](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/boot-self-test-flow.md)
- [前面板固件 UI 内部文档](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/ui/README.md)
