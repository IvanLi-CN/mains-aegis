---
title: 前面板 UI 图集
description: 前面板屏幕的冻结渲染图索引与全量画面汇总。
---

# 前面板 UI 图集

本页只做一件事：集中收录当前已经冻结的前面板 UI 画面，方便对照设计文档和实际渲染结果。

如果你要看“这些页为什么这样设计、能点哪里、状态词是什么意思”，请先看：

- [前面板 UI 交互与设计](/design/front-panel-ui-design)

## 1. 页面家族

| 页面家族 | 作用 |
| --- | --- |
| `SELF CHECK` | 开机自检、模块状态总览、bring-up 首屏 |
| Dashboard | 正常运行首页，展示模式、输入输出、充放电、SOC |
| Detail pages | 对 Output / Charger / Thermal / Cells / Battery Flow 的钻取页 |
| `MANUAL CHARGE` | 手动充电偏好与运行时控制页 |

## 2. `SELF CHECK` 图集

### 2.1 模块分区图

![Self-check 模块分区图](/ui/self-check-c-module-map.png)

### 2.2 正常推进

![Self-check - STANDBY idle](/ui/self-check-c-standby-idle.png)

![Self-check - ASSIST output focus](/ui/self-check-c-assist-up.png)

![Self-check - BACKUP touch focus](/ui/self-check-c-backup-touch.png)

### 2.3 异常与恢复相关画面

![Self-check - BMS 缺失且 TPS 警告](/ui/self-check-c-bms-missing-tps-warn.png)

![Self-check - BQ40 offline idle](/ui/self-check-c-bq40-offline-idle.png)

![Self-check - BQ40 offline activate dialog](/ui/self-check-c-bq40-offline-activate-dialog.png)

![Self-check - BQ40 activating](/ui/self-check-c-bq40-activating.png)

### 2.4 BQ40 结果弹窗

![Self-check - BQ40 result success](/ui/self-check-c-bq40-result-success.png)

![Self-check - BQ40 result no battery](/ui/self-check-c-bq40-result-no-battery.png)

![Self-check - BQ40 result rom mode](/ui/self-check-c-bq40-result-rom-mode.png)

![Self-check - BQ40 result abnormal](/ui/self-check-c-bq40-result-abnormal.png)

![Self-check - BQ40 result not detected](/ui/self-check-c-bq40-result-not-detected.png)

## 3. Dashboard 图集

### 3.1 模块分区图

![Dashboard 模块分区图](/ui/dashboard-b-module-map.png)

### 3.2 首页四态

![Dashboard - BYPASS](/ui/dashboard-b-off-mode.png)

![Dashboard - STANDBY](/ui/dashboard-b-standby-mode.png)

![Dashboard - ASSIST](/ui/dashboard-b-supplement-mode.png)

![Dashboard - BACKUP](/ui/dashboard-b-backup-mode.png)

## 4. 二级详情页图集

### 4.1 首页入口映射

![Dashboard detail - home map](/ui/dashboard-b-detail-home.png)

### 4.2 冻结画面

![Dashboard detail - cells](/ui/dashboard-b-detail-cells.png)

![Dashboard detail - battery flow](/ui/dashboard-b-detail-battery-flow.png)

![Dashboard detail - output](/ui/dashboard-b-detail-output.png)

![Dashboard detail - charger](/ui/dashboard-b-detail-charger.png)

![Dashboard detail - thermal](/ui/dashboard-b-detail-thermal.png)

![Dashboard detail - icons](/ui/dashboard-detail-icons.png)

## 5. `MANUAL CHARGE` 图集

![Manual charge - default](/ui/manual-charge-default.png)

![Manual charge - active](/ui/manual-charge-active.png)

![Manual charge - stop hold](/ui/manual-charge-stop-hold.png)

![Manual charge - reset auto](/ui/manual-charge-reset-auto.png)

![Manual charge - blocked](/ui/manual-charge-blocked.png)

## 6. 相关文档

- [前面板 UI 交互与设计](/design/front-panel-ui-design)
- [前面板与固件](/design/front-panel-and-firmware)
- [固件烧录与首次自检](/manual/firmware-flash-and-self-test)
- [Front panel UI docs](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/ui/README.md)
