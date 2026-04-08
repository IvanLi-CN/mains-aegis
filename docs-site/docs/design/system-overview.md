---
title: 系统概览
description: Mains Aegis 的整体目标、模块关系与当前状态。
---

# 系统概览

Mains Aegis 当前可以理解为一套围绕**电池供电、外部输入、受控输出、前面板交互与嵌入式固件**展开的开源硬件平台。它并不是单一小板，而是至少包含主板、前面板、固件与配套文档的组合体。

## 项目目标

- 基于 `4S1P` 锂电池包建立一套可观测、可保护、可控制的电源平台。
- 同时覆盖 BMS、充电、对外 UPS 输出、热与电流监测、前面板显示/触摸交互。
- 让协作者既能继续修改设计，也能在现有事实基础上自己复刻样机并做 bring-up。

## 系统由哪些部分组成

### 1. 主板（Mainboard）

主板负责电池包、BMS、充电、主输出、电源监测、风扇、音频与前面板互连，是整个系统的电气核心。

### 2. 前面板（Front Panel）

前面板负责 TFT 屏幕、触摸、五向按键、背光与部分 USB 相关互连，用来承载自检、Dashboard 与交互入口。

### 3. 固件（ESP32-S3）

主控采用 `ESP32-S3-FH4R2`，当前固件栈基于 `esp-hal` / `no_std`，已经包含 bring-up、自检、Dashboard、手动充电页、音频提示等多个运行时能力。

### 4. 文档与资料仓库

仓库中的 `docs/**`、`docs/pcbs/**`、`firmware/README.md`、以及本地化后的 vendor datasheet/manual/reference design，是项目继续推进的事实底座。

## 当前开发状态怎么理解

项目已经不是“只有想法”的阶段：主板/前面板网表、固件 bring-up、自检与多项模块设计都已经有较多沉淀。但它也还不是“一切都已冻结”的量产成品：

- 有些器件和链路已经选定并落到网表或固件。
- 有些技术方向虽已进入主线实现，但仍需要更多样机验证。
- 仍有一部分内容在“候选 / 待定 / 持续验证”的状态。

阅读文档时，最重要的是尊重这种边界，而不是把所有内容都当成最终量产结论。

## 高层模块关系（文字版）

```text
Battery pack (4S1P)
  -> BMS / protection
  -> charger / input path
  -> programmable UPS output
  -> monitoring / thermal / alerts
  -> ESP32-S3 firmware
  -> front panel UI (display + touch + buttons)
```

## 设计阅读建议

- 如果你先关心“系统为什么这样分块”，优先读完本页再去电源与 BMS。
- 如果你已经在画板或看网表，直接进入电源与 BMS页更高效。
- 如果你准备开始烧录和 bring-up，可以把设计手册看完后马上切到“复刻与使用”。

## 延伸阅读

- [硬件选型总览](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/hardware-selection.md)
- [主板 PCB 说明](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/pcbs/mainboard/README.md)
- [前面板 PCB 说明](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/pcbs/front-panel/README.md)
- [规格索引](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/specs/README.md)
