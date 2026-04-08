---
title: 前面板与固件
description: 前面板硬件、显示交互与 ESP32-S3 固件入口。
---

# 前面板与固件

Mains Aegis 不只是一个电源板。它还带有一块承担显示、触摸、按键与运行时反馈的前面板，以及一套围绕 `ESP32-S3` 构建的主固件。

## 前面板负责什么

前面板 PCB 目前承担这些职责：

- TFT 屏幕与触摸模组
- 五向按键与中键
- `TCA6408A` 扩展器
- 背光控制
- 与主板之间的 `FPC` 互连
- 部分 USB 相关信号走线

对复刻者来说，前面板最重要的不是“看起来像一块屏”，而是它和主板之间的连线约束、复位路径、共享 I2C / 中断边界，这些都会直接影响 bring-up 成败。

## 固件当前是什么形态

仓库当前固件栈基于：

- 主控：`ESP32-S3-FH4R2`
- Rust + `esp-hal`
- `no_std`
- 可构建、可烧录、可监视日志

而且它已经不只是一个“Hello World” bring-up：仓库中已经能看到自检页、Dashboard、手动充电页、音频提示与多项运行时状态收敛逻辑。

## UI 与固件之间的关系

前面板 UI 不是孤立资源，它和固件运行时状态是强耦合的：

- 自检页反映模块门控与异常态
- Dashboard 反映当前供电/充电/输出状态
- 触摸与按键会驱动页面切换与特定动作
- 预览图、规格图与真机行为之间需要持续对齐

因此，如果你准备继续开发前面板体验，必须同时关注：

1. UI 设计文档
2. 预览/规格截图
3. 固件中的真实状态源与交互逻辑

## bring-up 时为什么要先看这里

因为很多“看起来像硬件问题”的现象，实际上是前面板、固件、自检门控与总线状态共同作用的结果：

- 屏幕不亮，可能是背光控制、复位链路或 SPI/I2C 初始化问题。
- 触摸不工作，可能是 `CTP_IRQ`、`TP_RESET` 或共享总线问题。
- 页面停在自检，不一定是 UI 问题，而可能是某个电源/BMS 模块没有通过门控。

## 延伸阅读

- [前面板 PCB 说明](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/pcbs/front-panel/README.md)
- [固件 bring-up README](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/README.md)
- [开机自检流程](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/boot-self-test-flow.md)
- [前面板 UI 文档索引](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/ui/README.md)
