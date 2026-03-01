# Dashboard UI 设计（Variant B）

本文件定义固件屏幕 Dashboard 页面（Variant B）的模块布局、渲染语义与冻结图。

## 1. 基线

- 视觉冻结基线：[../../docs/specs/6qrjs-front-panel-industrial-ui-preview/SPEC.md](../../docs/specs/6qrjs-front-panel-industrial-ui-preview/SPEC.md)
- 运行语义基线：[../../docs/specs/7n4qd-mcu-self-check-live-panel/SPEC.md](../../docs/specs/7n4qd-mcu-self-check-live-panel/SPEC.md)
- 视觉规范来源：[design-language.md](design-language.md)
- 组件契约来源：[component-contracts.md](component-contracts.md)
- 分辨率：`320x172`

## 2. 页面模块分区图

![Dashboard Variant B Module Map](assets/dashboard-b-module-map.png)

## 3. 模块拆解

| 编号 | 模块 | 几何（px） | 固定语义 | 关键数据/状态 |
| --- | --- | --- | --- | --- |
| 1 | 顶栏 Top bar | `x=0 y=0 w=320 h=18` | 左侧标题 `UPS DASHBOARD`，右侧模式位（`BYPASS/STANDBY/ASSIST/BACKUP` 或 `IRQ ON`） | 模式色随 `UpsMode` 切换 |
| 2 | 主 KPI 面板 | `x=6 y=22 w=196 h=52` | 市电存在：`PIN W + POUT W`；市电缺失：`POUT W + IOUT A` | 标签行 `y=27`，数值行 `y=44`（数值字体 B） |
| 3 | 次级信息面板 | `x=6 y=76 w=196 h=94` | 四模式文本块固定：`BYPASS ACTIVE / STANDBY CHARGE / ASSIST / OUTPUT` | 右侧数值随模式切换（TPS 输出、充电锁定、温度、SOC） |
| 4 | `BATTERY` 卡 | `x=206 y=22 w=108 h=48` | 固定展示 `SOC + Tmax + 电池状态` | 状态位示例：`BAL/CHG/DSG/LOW/BYP/IDLE` |
| 5 | `CHARGE` 卡 | `x=206 y=72 w=108 h=48` | 固定展示电池充电电流与状态 | **仅 STANDBY 可充电**；其他模式显示 `LOCK/NOAC` |
| 6 | `DISCHG` 卡 | `x=206 y=122 w=108 h=48` | 固定展示电池放电电流与状态 | `BYPASS/STANDBY` 通常为 `0A`，`ASSIST/BACKUP` 随负载变化 |

## 4. 页面业务口径（冻结）

- 工作模式固定四态：`BYPASS / STANDBY / ASSIST / BACKUP`。
- 充电策略固定：仅 `STANDBY` 允许充电；`BYPASS/ASSIST/BACKUP` 显示非充电状态（`LOCK/NOAC`）。
- 右侧三卡语义固定，不与负载侧字段混用。
- 视觉样式（色板、字体分工、状态词形）以 [design-language.md](design-language.md) 为准，本页不再重复定义 Token 细节。

## 5. 冻结渲染图（四模式）

![Dashboard Variant B - BYPASS](assets/dashboard-b-off-mode.png)
![Dashboard Variant B - STANDBY](assets/dashboard-b-standby-mode.png)
![Dashboard Variant B - ASSIST](assets/dashboard-b-supplement-mode.png)
![Dashboard Variant B - BACKUP](assets/dashboard-b-backup-mode.png)
