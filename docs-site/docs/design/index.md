---
title: 设计手册
description: Mains Aegis 的系统结构与设计边界。
---

# 设计手册

这部分面向需要理解项目设计的协作者：它不会逐页复刻仓库里所有专题文档，而是先把系统结构、当前设计边界和关键模块关系讲清楚，再把你带到更深的事实源。

## 这一部分回答什么问题

- 这个项目究竟由哪些板卡、模块和固件组成？
- 哪些关键器件与结构已经冻结，哪些仍是候选或待定？
- 电池/BMS/充电/UPS 输出/监测/前面板/固件之间的职责边界是什么？
- 如果你准备继续改设计，应该先去看哪些原始专题文档？

## 阅读顺序

1. [系统概览](/design/system-overview)：先理解项目目标、当前开发状态与整体模块图。
2. [电源与 BMS](/design/power-and-bms)：再进入电池包、BMS、充电、UPS 输出与监测链路。
3. [前面板与固件](/design/front-panel-and-firmware)：最后对齐前面板、显示/触摸、固件架构与 bring-up 入口。

## 你在这里不会看到什么

- 不会看到完整 vendor 资料库索引。
- 不会看到“全部都已冻结”的假象；仍处于候选或待定状态的内容会明确标出来。
- 不会把专题事实源原样搬过来，而是用更适合首次阅读的结构做入口。

## 延伸阅读

- [仓库根文档索引](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/README.md)
- [硬件选型总览](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/hardware-selection.md)
