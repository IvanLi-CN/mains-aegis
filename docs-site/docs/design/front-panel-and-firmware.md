---
title: 前面板与固件
description: 前面板硬件、控制线与固件运行时页面。
---

# 前面板与固件

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

这正是前面板的恢复路径：扩展器失控时，拉低 `TCA_RESET#`，屏幕和触摸会一起回到安全态。

## 4. 总线与中断规则

- 前面板不放 `I2C2_SCL/SDA/INT` 上拉，必须由主板侧提供。
- `I2C2_INT` 只能挂开漏源；当前挂载对象是 `TCA6408A` 和 `FUSB302B`。
- 触摸 `IRQ` 单独走 `CTP_IRQ`，不并入 `I2C2_INT`。这样做是为了规避 `CST816D IRQ` 电气类型不明确带来的 wired-OR 风险。

## 5. 固件运行时基线

| 项目 | 当前实现 |
| --- | --- |
| 主控 | `ESP32-S3-FH4R2` |
| 固件栈 | Rust + `esp-hal` + `no_std` |
| 主界面 | `SELF CHECK`、Dashboard |
| 音频 | `GPIO4/5/6 -> MAX98357A -> 8Ω/1W speaker` |
| 观测入口 | 串口日志 + 前面板页面 |

### 5.1 `SELF CHECK` 模块列表

当前页面跟踪以下模块：

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

### 5.2 Dashboard 运行态

| 模式 | 含义 |
| --- | --- |
| `BYPASS` | 输入直通输出 |
| `STANDBY` | 输入在线，TPS 无实际输出电流 |
| `ASSIST` | 输入在线，TPS 有实际输出电流 |
| `BACKUP` | 输入离线，电池供电 |

右侧三卡固定为 `BATTERY / CHARGE / DISCHG`。首页 `CHARGE` 卡使用紧凑 token：`CHG / WAIT / FULL / WARM / TEMP / LOAD / LOCK / NOAC`。

## 6. 前面板 UI 冻结渲染图

以下只保留代表图。交互规则与设计约束见 [前面板 UI 交互与设计](/design/front-panel-ui-design)，完整冻结画面见 [前面板 UI 图集](/design/front-panel-ui-gallery)。这些图片都来自 `firmware/ui/assets/**` 的冻结渲染图，分辨率均为 `320x172`。它们的作用不是营销展示，而是帮助读者理解：这块屏在 bring-up 和运行态下应该看到什么。

### 6.1 `SELF CHECK`

先看模块分区图：

![Self-check 模块分区图](/ui/self-check-c-module-map.png)

代表画面：

![Self-check 正常待机画面](/ui/self-check-c-standby-idle.png)

![Self-check - BMS 缺失且 TPS 警告](/ui/self-check-c-bms-missing-tps-warn.png)

读图要点：

- 左右共 10 张模块卡，对应当前固件的 10 个通信模块。
- `PEND -> OK/WARN/ERR/HOLD/N/A` 的变化，就是 bring-up 时最直接的状态面板。
- 第二张图这种“`BQ40Z50` 缺失、`TPS` 变成等待/警告”的情况，不应直接理解为输出级坏了；它通常是在表达上游门控未满足。

### 6.2 Dashboard 首页

先看首页模块图：

![Dashboard 模块分区图](/ui/dashboard-b-module-map.png)

代表画面：

![Dashboard - STANDBY](/ui/dashboard-b-standby-mode.png)

![Dashboard - BACKUP](/ui/dashboard-b-backup-mode.png)

读图要点：

- 顶栏模式位固定使用全称：`BYPASS / STANDBY / ASSIST / BACKUP`。
- 主 KPI 区和右侧三卡（`BATTERY / CHARGE / DISCHG`）是运行态最重要的观察入口。
- `STANDBY` 与 `BACKUP` 的差别不只是文案；它对应输入是否在线、TPS 是否实际出力、电池是否正在承担负载。

### 6.3 Dashboard 二级详情页

![Output Detail](/ui/dashboard-b-detail-output.png)

![Charger Detail](/ui/dashboard-b-detail-charger.png)

这些详情页说明：当前固件 UI 不只有一个首页，还包括针对输出、充电、热和电池状态的钻取页。因此如果后续要扩展前面板交互，应该先沿 `firmware/ui/*.md` 的既有页面语义继续，而不是从零重新命名和布局。

## 7. Bring-up 观测点

首次 bring-up 建议按这个顺序看：

1. 屏幕是否点亮并进入 `SELF CHECK`
2. `TCA6408A` 是否可达；若不可达，先不要继续追屏幕 UI
3. `BQ40Z50`、`BQ25792`、`TPS55288` 是否在页面和日志中同时出现
4. 如果自检停住，先判断是前面板链路、I2C、BMS 门控还是热保护在阻断

## 8. 相关文档

- [前面板 PCB 说明](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/pcbs/front-panel/README.md)
- [固件 bring-up README](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/README.md)
- [开机自检流程](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/boot-self-test-flow.md)
- [前面板 UI 文档索引](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/ui/README.md)
- [前面板 UI 交互与设计](/design/front-panel-ui-design)
- [前面板 UI 图集](/design/front-panel-ui-gallery)
- [Dashboard UI 设计](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/ui/dashboard-design.md)
- [Dashboard Detail UI 设计](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/ui/dashboard-detail-design.md)
- [Self-check UI 设计](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/ui/self-check-design.md)
