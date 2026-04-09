---
title: 项目手册
description: Mains Aegis 项目手册总览。
---

# 项目手册

本手册主要给两类读者使用：

- 继续修改原理图、PCB、固件或屏幕文档的开发者
- 自行复刻样机并完成 bring-up 的协作者

两类读者看到的是同一套事实；差别只在阅读起点，不在文档体系。

## 1. 从哪里开始

| 用途 | 建议先读 |
| --- | --- |
| 弄清系统组成、接口、电源路径和门控逻辑 | [系统设计](/design/index) |
| 先把板子点亮、烧录、看日志、做首次上电 | [样机复刻与 Bring-up](/manual/index) |
| 快速确认整机结构与关键网络 | [系统概览](/design/system-overview) |
| 快速确认前面板屏幕会依次出现哪些页面 | [前面板屏幕页面总览](/design/front-panel-screen-pages) |
| 快速确认复刻顺序 | [准备与范围](/manual/prepare-and-scope) |

## 2. 目录

### 系统设计

1. [导读](/design/index)
2. [系统概览](/design/system-overview)
3. [电源与 BMS](/design/power-and-bms)
4. [前面板与固件](/design/front-panel-and-firmware)
5. [前面板屏幕页面总览](/design/front-panel-screen-pages)
6. [前面板 UI 交互与设计](/design/front-panel-ui-design)

### 样机复刻与 Bring-up

1. [导读](/manual/index)
2. [准备与范围](/manual/prepare-and-scope)
3. [PCB 与连线检查](/manual/pcb-and-wiring-checks)
4. [固件烧录与首次自检](/manual/firmware-flash-and-self-test)
5. [基础使用与排障](/manual/basic-use-and-troubleshooting)

## 3. 公开范围

| 项目 | 说明 |
| --- | --- |
| 系统级设计文档 | 已公开 |
| 主板 / 前面板网表提炼 | 已公开 |
| 固件工具链与 bring-up 入口 | 已公开 |
| 前面板屏幕页面与交互文档 | 已公开 |
| 离线资料库 | 已公开 |
| 完整量产 BOM | 未作为统一发布物提供 |
| 统一生产文件打包入口 | 未作为统一发布物提供 |
| 成品用户说明书 | 不在当前范围内 |

## 4. 仓库内参考文档

- [仓库原始文档索引](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/README.md)
- [硬件选型总览](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/hardware-selection.md)
- [主板 PCB 说明](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/pcbs/mainboard/README.md)
- [前面板 PCB 说明](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/pcbs/front-panel/README.md)
- [固件 bring-up README](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/README.md)
- [前面板固件 UI 内部文档](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/ui/README.md)
