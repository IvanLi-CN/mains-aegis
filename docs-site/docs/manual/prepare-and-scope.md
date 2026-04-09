---
title: 准备与范围
description: 复刻前的目标定义、前置条件和资料边界。
---

# 准备与范围

## 1. 先定复刻目标

| 目标等级 | 交付结果 |
| --- | --- |
| L1：点亮样机 | 主板 + 前面板连通，屏幕进入 `SELF CHECK` |
| L2：打通电源链路 | `BQ40Z50`、`BQ25792`、`TPS55288`、`INA3221`、`TMP112A` 可观测 |
| L3：继续开发 | 能持续修改 `docs/**`、`docs/pcbs/**`、`firmware/**` 并重复 bring-up |

没有必要一上来就追 L3。首次样机建议先以 `L1 -> L2` 为顺序推进。

## 2. 必备工具

| 工具 | 用途 |
| --- | --- |
| 万用表 | 连通、短路、抽头顺序、静态电压 |
| 可调电源 | 代替部分输入源做安全上电 |
| 示波器 | 看 `SW`、中断、复位和电源纹波 |
| 逻辑分析仪 | 看 `I2C1/I2C2/SPI`，建议有 |
| 焊接 / 返修工具 | 返修虚焊、桥连、反装 |
| Rust + `espup` + `mcu-agentd` | 固件构建与烧录 |

## 3. 开始前必须确定的参数

| 项目 | 说明 |
| --- | --- |
| 输出设定 | `12V` 或 `19V`；影响固件 feature、热边界和测量方式 |
| bring-up 深度 | 仅点亮样机，还是继续验证 BMS / 充电 / 主输出 |
| 供电方式 | 电池、USB-C / PD、DC 输入，或先只接调试电源 |

## 4. 仓库里能直接用的资料

| 资料 | 用途 |
| --- | --- |
| `docs/hardware-selection.md` | 系统级选型总纲 |
| `docs/bms-design.md` / `docs/charger-design.md` / `docs/ups-output-design.md` | 电源主线设计 |
| `docs/pcbs/mainboard/README.md` / `docs/pcbs/front-panel/README.md` | 板级网络与接口提炼 |
| `firmware/README.md` | 工具链、构建、烧录、日志说明 |
| `docs/datasheets/**` 等 | 器件级离线资料库 |

## 5. 仓库里暂时没有的统一交付物

- 完整量产 BOM
- 统一生产文件打包入口
- 成品说明书

所以这份手册的定位是**工程复刻手册**，不是成品用户手册。

## 6. 下一步

- [PCB 与连线检查](/manual/pcb-and-wiring-checks)
- [固件烧录与首次自检](/manual/firmware-flash-and-self-test)
