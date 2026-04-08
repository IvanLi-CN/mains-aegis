---
title: 复刻与使用
description: 面向 DIY 复刻者与半开发者的准备、烧录与排障入口。
---

# 复刻与使用

这部分不是“成品用户说明书”，而是给准备自己复刻 Mains Aegis 的协作者看的。你需要默认自己会面对硬件焊接、连线检查、固件烧录、日志观察与基本排障。

## 先明确一件事

当前仓库并没有把全部内容整理成“一键下单 + 一键装配 + 一键量产”的完整交付包。所以这份手册的目标是：

- 告诉你从哪里开始；
- 告诉你哪些信息已经有事实源；
- 告诉你 bring-up 时应该按什么顺序排查；
- 明确哪些东西仍需要你自己继续决策或补充。

## 建议阅读顺序

1. [准备与范围](/manual/prepare-and-scope)：先确认你要具备哪些前提，以及仓库当前公开了什么、还没公开什么。
2. [PCB 与连线检查](/manual/pcb-and-wiring-checks)：在上电前先做板级与连接检查。
3. [固件烧录与首次自检](/manual/firmware-flash-and-self-test)：搭好工具链、烧录主固件，并理解首次上电时应该看到什么。
4. [基础使用与排障](/manual/basic-use-and-troubleshooting)：把常见现象和进一步定位入口串起来。

## 这份手册不替你做的事

- 不会替你决定所有未冻结的候选物料。
- 不会假装仓库已经有完整量产 BOM / Gerber 打包 / 装配工艺说明。
- 不会把锂电池相关风险讲成“随便接就行”的程度。

## 延伸阅读

- [硬件选型总览](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/hardware-selection.md)
- [主板 PCB 说明](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/pcbs/mainboard/README.md)
- [前面板 PCB 说明](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/pcbs/front-panel/README.md)
- [固件 bring-up README](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/README.md)
