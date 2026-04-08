---
title: 样机复刻与 Bring-up 导读
description: 项目手册中的样机复刻与 bring-up 入口。
---

# 样机复刻与 Bring-up 导读

这部分只解决一件事：**把板子安全地点亮，并把失败定位到具体模块。**

## 1. 标准顺序

| 步骤 | 目标 | 页面 |
| --- | --- | --- |
| 1 | 冻结输出口径、工具条件和资料边界 | [准备与范围](/manual/prepare-and-scope) |
| 2 | 不上电完成静态检查 | [PCB 与连线检查](/manual/pcb-and-wiring-checks) |
| 3 | 能构建、烧录、看日志并进入 `SELF CHECK` | [固件烧录与首次自检](/manual/firmware-flash-and-self-test) |
| 4 | 把故障落到前面板 / BMS / 充电 / 输出 / 监测中的某一项 | [基础使用与排障](/manual/basic-use-and-troubleshooting) |

## 2. Bring-up 通过标准

最低通过标准不是“全部模块全绿”，而是：

- 固件可稳定构建
- 固件可稳定烧录
- 串口日志稳定可读
- 屏幕能进入 `SELF CHECK`
- 故障可以定位到具体模块，而不是“整机没反应”

## 3. 前置条件

- 会读网表、网络名、I2C 地址和 GPIO 映射
- 具备焊接返修、万用表、示波器等基础硬件调试条件
- 具备 Rust / `espup` / `mcu-agentd` 开发环境

## 4. 对应事实源

- [硬件选型总览](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/hardware-selection.md)
- [主板 PCB 说明](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/pcbs/mainboard/README.md)
- [前面板 PCB 说明](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/pcbs/front-panel/README.md)
- [固件 bring-up README](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/README.md)
