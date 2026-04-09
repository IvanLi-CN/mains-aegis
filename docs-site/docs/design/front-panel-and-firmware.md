---
title: 前面板与固件
description: 前面板硬件链路、控制线与固件运行时基线。
---

# 前面板与固件

本页从硬件连接和运行时两部分，说明前面板为什么会显示这些页面。

页面外观总览见 [前面板屏幕页面总览](/design/front-panel-screen-pages)；交互规则见 [前面板 UI 交互与设计](/design/front-panel-ui-design)。

## 1. 前面板组成

| 功能 | 已落地实现 |
| --- | --- |
| 显示 | SPI 屏，主控直接输出 `DC / MOSI / SCLK` |
| 触摸 | I2C 触摸，独立中断 `CTP_IRQ` |
| 按键 | 五向按键；中键直连 MCU；方向键挂 `TCA6408A` |
| GPIO 扩展 | `TCA6408A@0x21` |
| 背光 | `BLK` 控制高边开关 `Q16(BSS84)` |
| USB 前端 | `FUSB302B` + 前面板 USB 相关网络 |

## 2. 连接器与控制线

### 2.1 `FPC1`（主板 <-> 前面板）

| 网络 | 作用 |
| --- | --- |
| `TCA_RESET#` | 前面板扩展器复位入口 |
| `I2C2_SCL / I2C2_SDA` | `TCA6408A`、`FUSB302B`、触摸共用总线 |
| `I2C2_INT` | `TCA6408A`、`FUSB302B` 开漏中断线与 |
| `CTP_IRQ` | 触摸独立中断 |
| `DC / MOSI / SCLK` | 屏幕 SPI |
| `BLK` | 背光控制 |
| `BTN_CENTER` | 中键直连 `GPIO0` |
| `UCM_DP / UCM_DM` | USB2 差分对 |

### 2.2 `TCA6408A` 端口分配

| 端口 | 网络 | 用途 |
| --- | --- | --- |
| `P0..P3` | `BTN_DOWN / BTN_RIGHT / BTN_LEFT / BTN_UP` | 方向键 |
| `P4` | `USB2_PG` | USB2 power-good |
| `P5` | `CS` | 屏幕片选 |
| `P6` | `RES` | 屏幕复位 |
| `P7` | `TP_RESET` | 触摸复位 |

## 3. 默认安全态

前面板网表已经把默认偏置写死：

- `CS`：`100kΩ` 上拉 -> 默认不选中屏幕
- `RES`：`100kΩ` 下拉 -> 默认屏幕保持复位
- `TP_RESET`：`100kΩ` 下拉 -> 默认触摸保持复位

因此 `TCA_RESET#` 拉低后的结果是确定的：

| 信号 | `TCA_RESET#=0` 后的默认状态 |
| --- | --- |
| `CS` | 高 |
| `RES` | 低 |
| `TP_RESET` | 低 |

这就是前面板的恢复路径：扩展器失控时，拉低 `TCA_RESET#`，屏幕和触摸会一起回到安全态。

## 4. 总线与中断规则

- 前面板不放 `I2C2_SCL/SDA/INT` 上拉，必须由主板侧提供。
- `I2C2_INT` 只能挂开漏源；现在挂在这根线上的就是 `TCA6408A` 和 `FUSB302B`。
- 触摸 `IRQ` 单独走 `CTP_IRQ`，不并入 `I2C2_INT`。这样做是为了规避 `CST816D IRQ` 电气类型不明确带来的 wired-OR 风险。

## 5. 固件运行时基线

| 项目 | 说明 |
| --- | --- |
| 主控 | `ESP32-S3-FH4R2` |
| 固件栈 | Rust + `esp-hal` + `no_std` |
| 屏幕主路径 | `SELF CHECK` -> Dashboard -> 5 个详情页 -> `MANUAL CHARGE` |
| 音频 | `GPIO4/5/6 -> MAX98357A -> 8Ω/1W speaker` |
| 观测入口 | 串口日志 + 前面板页面 |

### 5.1 `SELF CHECK` 模块列表

前面板会跟踪以下模块：

- `GC9307`
- `TCA6408A`
- `FUSB302`
- `INA3221`
- `BQ25792`
- `BQ40Z50`
- `TPS55288-A`
- `TPS55288-B`
- `TMP112-A`
- `TMP112-B`

### 5.2 运行时页面路径

- 上电后，屏幕可用就先进入 `SELF CHECK`。
- 当自检结束且首份运行态快照准备好后，页面自动进入 Dashboard。
- Dashboard 首页给出 5 个入口：`Output / Thermal / Cells / Charger / Battery Flow`。
- `Charger Detail` 再继续下钻到 `MANUAL CHARGE`；控制动作就是从这里开始的。

### 5.3 Dashboard 运行态模式

| 模式 | 含义 |
| --- | --- |
| `BYPASS` | 输入直通输出 |
| `STANDBY` | 输入在线，TPS 无实际输出电流 |
| `ASSIST` | 输入在线，TPS 有实际输出电流 |
| `BACKUP` | 输入离线，电池供电 |

右侧三卡固定为 `BATTERY / CHARGE / DISCHG`。首页 `CHARGE` 卡使用紧凑 token：`CHG / WAIT / FULL / WARM / TEMP / LOAD / LOCK / NOAC`。

## 6. 屏幕上的典型页面

### 6.1 上电后先看到 `SELF CHECK`

![Self-check 正常待机画面](/ui/self-check-c-standby-idle.png)

![Self-check - BMS 缺失且 TPS 警告](/ui/self-check-c-bms-missing-tps-warn.png)

看这页时，先留意：

- 左右共 10 张模块卡，对应固件跟踪的 10 个通信模块。
- 卡片从 `PEND` 走到 `OK / WARN / ERR / HOLD / N/A`，就是 bring-up 时最直接的观察点。
- 如果 `BQ40Z50` 缺失，`TPS55288` 可能显示等待或警告；这往往表示上游前提还没满足，不必先把问题归到输出级本体。

### 6.2 自检结束后回到 Dashboard

![Dashboard - STANDBY](/ui/dashboard-b-standby-mode.png)

![Dashboard - BACKUP](/ui/dashboard-b-backup-mode.png)

这里可以顺着看三处：

- 正常运行时默认停在 Dashboard，不会一直停在 `SELF CHECK`。
- 左侧主 KPI、次级信息区和右侧三卡，一起构成运行态首页。
- `STANDBY` 和 `BACKUP` 的区别，会同时反映在输入在线性、输出参与度和电池承担负载的方式上。

### 6.3 从 `Charger Detail` 可以继续进入控制页

![Dashboard detail - charger](/ui/dashboard-b-detail-charger.png)

`Charger Detail` 左侧会话面板还能继续进入 `MANUAL CHARGE`。这也是这套页面里唯一一条会继续进入手动控制页的路径。

## 7. 继续阅读

- 页面全貌与最新画面： [前面板屏幕页面总览](/design/front-panel-screen-pages)
- 热区、状态词和交互规则： [前面板 UI 交互与设计](/design/front-panel-ui-design)
- 上电观察点与卡顿排查： [固件烧录与首次自检](/manual/firmware-flash-and-self-test)

## 8. 内部设计文档

- [前面板 PCB 说明](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/pcbs/front-panel/README.md)
- [固件 bring-up README](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/README.md)
- [开机自检流程](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/boot-self-test-flow.md)
- [前面板固件 UI 内部文档](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/ui/README.md)
- [Dashboard UI 设计](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/ui/dashboard-design.md)
- [Dashboard Detail UI 设计](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/ui/dashboard-detail-design.md)
- [Self-check UI 设计](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/ui/self-check-design.md)
