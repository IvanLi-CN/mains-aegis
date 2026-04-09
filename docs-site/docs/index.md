---
title: Mains Aegis 文档首页
description: Mains Aegis 项目手册首页。
---

# Mains Aegis 文档

`mains-aegis` 是开源硬件项目。这个仓库公开的是设计资料、板级接口整理、固件 bring-up 入口和离线资料库；复刻者需要自行下板、装配、烧录和联调。

## 1. 项目概况

| 项目 | 公开信息 |
| --- | --- |
| 电池包 | `4S1P`，`4 × 21700`，电芯 `EVE INR21700/50E` |
| 电压边界 | 标称 `14.6V`，满充 `16.8V`，截止放电 `10.0V` |
| BMS | `BQ40Z50-R2` + `BQ296100DSGR` + `CLM1612P1412` |
| 充电 | `BQ25792`，双输入：USB-C / PD(PPS) + DC |
| 主输出 | `TPS55288 × 2`，固件口径 `12V` / `19V` |
| 主控 | `ESP32-S3-FH4R2`，Rust + `esp-hal` + `no_std` |
| 前面板 | SPI 屏、I2C 触摸、`TCA6408A`、五向按键、背光 |
| 启动 UI | 先进入 `SELF CHECK`，门控放行后进入 Dashboard |

## 2. 文档分区

| 任务 | 入口 |
| --- | --- |
| 浏览整本手册和完整目录 | [项目手册](/handbook/index) |
| 先看系统结构、接口和器件边界 | [系统设计](/design/index) |
| 直接查看前面板屏幕页面与最新图 | [前面板屏幕页面总览](/design/front-panel-screen-pages) |
| 直接进入复刻、烧录和首次上电 | [样机复刻与 Bring-up](/manual/index) |

## 3. 建议顺序

1. 先在[系统概览](/design/system-overview)确认连接器、总线和 GPIO 映射。
2. 需要理解前面板运行时页面时，先看[前面板屏幕页面总览](/design/front-panel-screen-pages)。
3. 做样机前，按[PCB 与连线检查](/manual/pcb-and-wiring-checks)逐项过静态检查。
4. 烧录和首次上电时，以[固件烧录与首次自检](/manual/firmware-flash-and-self-test)为准。
5. 需要追源码、网表或器件级推导时，直接回仓库原始文档。

## 4. 文档边界

- 本站整理的是工程上已经写进仓库、并且适合公开阅读的内容，不代替 `docs/**` 原始设计文档。
- `docs/datasheets/**`、`docs/manuals/**`、`docs/reference-designs/**` 继续保留为离线资料库，不放入主导航。
- 仓库里暂时没有统一发布的完整量产 BOM、整套生产文件入口和成品用户说明书。

## 5. 相关链接

- [项目手册](/handbook/index)
- [GitHub 仓库](https://github.com/IvanLi-CN/mains-aegis)
- [仓库原始文档索引](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/README.md)
