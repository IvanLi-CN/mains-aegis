---
title: 前面板屏幕页面总览
description: 按上电顺序梳理前面板会出现的页面，以及每一页该怎么看。
---

# 前面板屏幕页面总览

本页按设备上电后的实际顺序，把前面板会出现的页面串起来，方便查每一页什么时候出现、主要看什么。

交互规则见 [前面板 UI 交互与设计](/design/front-panel-ui-design)；硬件链路和 bring-up 基线见 [前面板与固件](/design/front-panel-and-firmware)。

## 1. 页面路径

```text
Power-on
└─ SELF CHECK
   ├─ BQ40 overlay: offline idle -> confirm -> activating -> result
   └─ 自检清零 + 首份运行态快照就绪
      -> Dashboard Home
         ├─ Output Detail
         ├─ Thermal Detail
         ├─ Cells Detail
         ├─ Charger Detail
         │  └─ MANUAL CHARGE
         └─ Battery Flow Detail
```

| 阶段 | 页面 | 主要内容 |
| --- | --- | --- |
| 启动 / 恢复 | `SELF CHECK` + BQ40 overlay | 哪些模块已经 ready、哪些还在阻断 |
| 运行态首页 | Dashboard Home | 当前是 `BYPASS/STANDBY/ASSIST/BACKUP` 中哪一种 |
| 单系统查看 | 5 个详情页 | 输出、热、电芯、充电、放电流向的细节 |
| 运行时控制 | `MANUAL CHARGE` | 是否允许手动充电、当前设置与停止/恢复状态 |

## 2. 启动与恢复阶段：`SELF CHECK`

### 2.1 默认首屏

当屏幕链路起来后，设备先进入 `SELF CHECK`。这不是装饰动画，而是 bring-up 时最直接的状态面板。

![Self-check 模块分区图](/ui/self-check-c-module-map.png)

![Self-check - STANDBY idle](/ui/self-check-c-standby-idle.png)

![Self-check - BMS 缺失且 TPS 警告](/ui/self-check-c-bms-missing-tps-warn.png)

这一页通常先看三处：

- `BQ40Z50 / BQ25792 / TPS55288-A/B` 是否在线，前提是否已经满足。
- 左右 10 张卡片里，哪些已经从 `PEND` 走到 `OK / WARN / ERR / HOLD / N/A`。
- 系统是不是还卡在 BMS、热保护或输出条件上，因此暂时不能离开自检页。

### 2.2 BQ40 恢复流程

`BQ40Z50` 卡片是 `SELF CHECK` 里唯一会主动拉起 overlay 的入口。页面路径如下：

| 场景 | 页面变化 |
| --- | --- |
| 设备存在但离线 / 需恢复 | 卡片先进入 offline idle |
| 用户确认恢复 | 进入确认对话框 |
| 正在恢复 | 进入 activating 进度态 |
| 恢复结束 | 显示结果页；如果自检条件已满足，随后自动进入 Dashboard |

![Self-check - BQ40 offline idle](/ui/self-check-c-bq40-offline-idle.png)

![Self-check - BQ40 offline activate dialog](/ui/self-check-c-bq40-offline-activate-dialog.png)

![Self-check - BQ40 activating](/ui/self-check-c-bq40-activating.png)

恢复结果页一共有 5 种画面：

![Self-check - BQ40 result success](/ui/self-check-c-bq40-result-success.png)

![Self-check - BQ40 result no battery](/ui/self-check-c-bq40-result-no-battery.png)

![Self-check - BQ40 result rom mode](/ui/self-check-c-bq40-result-rom-mode.png)

![Self-check - BQ40 result abnormal](/ui/self-check-c-bq40-result-abnormal.png)

![Self-check - BQ40 result not detected](/ui/self-check-c-bq40-result-not-detected.png)

## 3. 自检结束后进入 Dashboard Home

当自检清零且首份运行态快照准备好后，屏幕会切到 Dashboard Home。

![Dashboard detail - home map](/ui/dashboard-b-detail-home.png)

Dashboard 首页主要做两件事：

1. 用顶栏告诉你当前模式是 `BYPASS / STANDBY / ASSIST / BACKUP`。
2. 用 5 个固定热区带你进入具体子系统页面。

### 3.1 Dashboard 四种首页态

![Dashboard - BYPASS](/ui/dashboard-b-off-mode.png)

![Dashboard - STANDBY](/ui/dashboard-b-standby-mode.png)

![Dashboard - ASSIST](/ui/dashboard-b-supplement-mode.png)

![Dashboard - BACKUP](/ui/dashboard-b-backup-mode.png)

### 3.2 进入 Dashboard 后通常先看哪里

- 左上主 KPI：输出功率，以及输入功率 / 电流的主视图。
- 左下次级信息：热、输出、充电等运行态摘要。
- 右侧三卡：`BATTERY / CHARGE / DISCHG`，同时也是进入详情页的入口。

## 4. 5 个详情页

Dashboard 首页的 5 个热区分别通向 5 个详情页。它们共用同一套导航骨架：左上 `BACK`、中间标题、右上状态 chip、上半区主指标、下半区 2~4 组信息卡。

### 4.1 `Output Detail`

![Dashboard detail - output](/ui/dashboard-b-detail-output.png)

- 何时进入：点击 Dashboard 左上主 KPI 面板。
- 主要看点：`VOUT / POUT`、`OUT-A / OUT-B`、输出异常摘要。

### 4.2 `Thermal Detail`

![Dashboard detail - thermal](/ui/dashboard-b-detail-thermal.png)

- 何时进入：点击 Dashboard 左下次级信息面板。
- 主要看点：最高温度、风扇状态、板上 / 电池 / TMP 温度链路。

### 4.3 `Cells Detail`

![Dashboard detail - cells](/ui/dashboard-b-detail-cells.png)

- 何时进入：点击右上 `BATTERY` 卡。
- 主要看点：4 节电压、均衡状态、温度和充放电状态。

### 4.4 `Charger Detail`

![Dashboard detail - charger](/ui/dashboard-b-detail-charger.png)

- 何时进入：点击中间 `CHARGE` 卡。
- 主要看点：输入来源、`IN W / CHARGE W`、charging state、source select、status detail。
- 补充：这页左侧会话面板还能继续进入 `MANUAL CHARGE`，所以它不是终点页。

### 4.5 `Battery Flow Detail`

![Dashboard detail - battery flow](/ui/dashboard-b-detail-battery-flow.png)

- 何时进入：点击右下 `DISCHG` 卡。
- 主要看点：`VPACK / IPACK`、`ENERGY / FULL CAP / CHG / DSG / PCHG`。

### 4.6 详情页图标与状态提示

![Dashboard detail - icons](/ui/dashboard-detail-icons.png)

这组图标是详情页里最常复用的状态语言。如果想查词义和固定文案，可以回到 [前面板 UI 交互与设计](/design/front-panel-ui-design)。

## 5. `MANUAL CHARGE`

`MANUAL CHARGE` 是这套页面里负责手动充电控制的一页。从 `Charger Detail` 进入，用来设置偏好、启动或停止手动充电。

![Manual charge - default](/ui/manual-charge-default.png)

![Manual charge - active](/ui/manual-charge-active.png)

![Manual charge - stop hold](/ui/manual-charge-stop-hold.png)

![Manual charge - reset auto](/ui/manual-charge-reset-auto.png)

![Manual charge - blocked](/ui/manual-charge-blocked.png)

看这页时，通常先看：

- 三组横向 segmented rows：`TARGET / SPEED / TIMER`。
- 底部唯一 action bar：`BACK / STATUS / START|STOP`。
- 顶部模式词和底部 notice，是否在说明“正在手动充电 / 自动策略 held / 被安全条件阻断”。

## 6. 继续阅读

- 页面切换规则、热区和状态词： [前面板 UI 交互与设计](/design/front-panel-ui-design)
- 长时间停留在 `SELF CHECK` 时的排查路径： [固件烧录与首次自检](/manual/firmware-flash-and-self-test)
- 前面板硬件链路与固件运行时基线： [前面板与固件](/design/front-panel-and-firmware)
- 需要对照内部设计基线和组件约束： [前面板固件 UI 内部文档](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/ui/README.md)
