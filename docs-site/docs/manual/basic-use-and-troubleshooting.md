---
title: 基础使用与排障
description: 首次 bring-up 之后最常见的现象、理解方式与排障入口。
---

# 基础使用与排障

当你已经能把固件烧进去、看到屏幕与日志后，后续工作通常会分成两类：

- 基础使用：确认自检、Dashboard、手动操作页等是否按预期工作。
- 排障：定位为什么某个模块仍未通过，或为什么 UI / 输出 / 监测行为和预期不一致。

## 先学会用“模块视角”看问题

对于 Mains Aegis，排障最怕的是一上来就把所有问题都归因给单一模块。更稳的方式是按模块看：

- 屏幕 / 触摸 / 按键是否工作？
- BMS 是否在线、是否通过自检？
- 充电路径是否可见？
- UPS 主输出与监测是否工作？
- 固件日志是否能解释当前状态？

## 常见现象 1：屏幕不亮或页面不对

优先检查：

- 背光控制与供电是否正常
- 屏幕复位/片选链路是否正确
- 前面板 FPC 连接是否可靠
- 日志里是否有相关初始化失败线索

## 常见现象 2：触摸或按键没反应

优先检查：

- `CTP_IRQ` / `TP_RESET` / `TCA_RESET#`
- `I2C2` 总线与共享中断线
- 前面板与主板之间的连接方向与焊接质量

## 常见现象 3：自检停在某个模块

自检停住往往意味着“某个依赖没准备好”，而不是简单的 UI 卡死。建议先同时观察：

- 前面板显示内容
- 串口日志
- 当前被卡住的是 BMS、充电、输出还是别的链路

## 常见现象 4：BQ40 / 电源路径状态异常

这种问题不要只看一个页面文案。更可靠的做法是：

- 回到 BMS / 充电 / 输出专题文档
- 对照硬件链路与门控逻辑
- 再结合当前固件 UI 或日志去理解

## 常见现象 5：你发现“文档里没写完”

这在当前阶段是正常的。正确做法不是假设“那一定默认如此”，而是：

1. 回到仓库原始文档确认是否已有更深的事实源；
2. 如果仍没有，就把它当作当前项目尚未冻结的缺口。

## 一个实用建议

如果你准备长期跟进项目，建立一套自己的 bring-up 记录会非常有帮助：记录你使用的板版本、输出版本、构建 feature、看到的日志与当前现象。这样后续回看时，远比只记“好像不对劲”更有效。

## 延伸阅读

- [BMS 设计](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/bms-design.md)
- [充电器设计](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/charger-design.md)
- [UPS 主输出设计](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/ups-output-design.md)
- [电源监测与保护设计](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/power-monitoring-design.md)
- [固件 bring-up README](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/README.md)
