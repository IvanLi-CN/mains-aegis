---
title: 前面板 UI 交互与设计
description: 前面板屏幕的信息架构、交互路径和状态词说明。
---

# 前面板 UI 交互与设计

这页讲前面板屏幕为什么这样分层、每一页怎么跳转，以及状态词在界面上各代表什么。

完整页面和最新画面见 [前面板屏幕页面总览](/design/front-panel-screen-pages)。

## 1. 这页讲什么

- 主要给样机复刻者、固件协作者和前面板 bring-up 排障时使用。
- 范围是 `320x172` 横屏固件 UI：`SELF CHECK`、Dashboard 首页、5 个二级详情页、`MANUAL CHARGE`，以及 `BQ40Z50` 恢复相关 overlay / 结果页。
- 不讨论 Host 侧 UI、网页控制台和营销展示图。

## 2. 硬件输入决定了哪些交互边界

| 输入/部件 | 事实基线 | 对 UI 设计的影响 |
| --- | --- | --- |
| 电容触摸 | 触摸控制器走 `I2C2_SCL/SDA`，中断独立走 `CTP_IRQ`，不并入 `I2C2_INT` | 页面点击热区可以独立定义，不需要和 `TCA6408A/FUSB302B` 共用一根中断线 |
| 五向按键 | `BTN_UP/DOWN/LEFT/RIGHT` 走 `TCA6408A@0x21`，`BTN_CENTER` 直连 `ESP32-S3 GPIO0` | UI 可以同时兼容“触摸 + 五向键”，但页面几何首先按触摸热区组织 |
| 中键 `BTN_CENTER` | 直连 `GPIO0`，同时是下载模式 strapping pin | 不能把“按住中键再复位”设计成正常 UI 手势；它首先是硬件启动约束 |
| `TCA_RESET#` | 拉低后 `TCA6408A` 端口回高阻，`CS` 被上拉、`RES/TP_RESET` 被下拉 | 扩展器失控时，屏幕和触摸会一起回到安全态；UI 要能接受“整块前面板被硬复位后重新初始化” |
| 背光 `BLK` | 背光由主控显式控制 | 背光开关不承载业务态，不把“背光是否亮”当作系统状态词 |

所以前面板 UI 更像一块兼顾调试和运行观察的小屏，而不是消费级触摸界面。

## 3. 页面结构

```text
Power-on
└─ SELF CHECK
   ├─ BQ40 卡片触发的确认 / 进度 / 结果 overlay
   └─ 自检结束后自动进入 Dashboard Home
      ├─ Output Detail
      ├─ Thermal Detail
      ├─ Cells Detail
      ├─ Charger Detail
      │  └─ MANUAL CHARGE
      └─ Battery Flow Detail
```

这里有两个关键决定：

1. `SELF CHECK` 不是启动动画，而是 bring-up 时的状态页。
2. Dashboard 首页负责分流，详细信息再进入二级页。

## 4. 页面怎么切换

### 4.1 上电先进入 `SELF CHECK`

`SELF CHECK` 是屏幕可用后的默认首屏。页面会持续刷新模块状态，只有在自检真的结束后才会进入 Dashboard。

### 什么时候会自动进入 Dashboard

只有以下条件同时满足，UI 才会从 `SELF CHECK` 切到 Dashboard：

- `GC9307 / TCA6408A / FUSB302 / INA3221 / BQ25792 / TMP112-A / TMP112-B` 达到 `OK` 或 `N/A`
- `BQ40Z50` 为 `OK`，且不是 `no battery`、不是 `discharge_ready=false`、不在恢复进行中
- 输出门控原因已清空（`output_gate_reason == None`）
- 如果当前模式请求输出，则 `TPS55288-A/B` 不仅要通信正常，还要已经真正进入 active output，而不是停留在 `HOLD`

所以有些样机虽然已经能点亮屏、I2C 也通了，页面还是会停在 `SELF CHECK`：这说明系统离“进入运行态”还差一个或多个前提。

### `SELF CHECK` 里的可操作区

| 区域 | 几何（px） | 作用 |
| --- | --- | --- |
| `BQ40Z50` 卡片 | `x=163 y=22 w=151 h=29` | 触发 BMS 恢复相关交互 |
| `Cancel` 按钮 | `x=32 y=116 w=108 h=24` | 关闭确认框 |
| `Confirm` 按钮 | `x=152 y=116 w=136 h=24` | 确认激活/授权 |

交互规则如下：

- 无 overlay 时，只有 `BQ40Z50` 卡片是触摸入口。
- 进入确认框后，只有 `Cancel / Confirm` 两个热区可操作。
- 进度页与结果页不再接受触摸输入，避免在恢复过程中重复触发动作。
- 目前没有“手动从 `SELF CHECK` 切到 Dashboard”的入口；是否切页完全由自检结果决定。

### 4.2 Dashboard 首页就是 5 个入口

Dashboard 首页既要让人一眼看出当前电源模式，也要把五个子系统的入口摆在固定位置。

| 首页区域 | 几何（px） | 进入页面 | 这样放的原因 |
| --- | --- | --- | --- |
| 主 KPI 面板 | `x=6 y=22 w=196 h=52` | `Output Detail` | 最左上给输出主路径，符合运行态最常看的数据 |
| 次级信息面板 | `x=6 y=76 w=196 h=94` | `Thermal Detail` | 热管理通常是运行态第二优先级的信息 |
| `BATTERY` 卡 | `x=206 y=22 w=108 h=48` | `Cells Detail` | 从电池总览直接下钻到 cell 级状态 |
| `CHARGE` 卡 | `x=206 y=72 w=108 h=48` | `Charger Detail` | 充电链路有独立状态机和下级控制页 |
| `DISCHG` 卡 | `x=206 y=122 w=108 h=48` | `Battery Flow Detail` | 供电 / 放电流向单独成页 |

这里没有滚动列表，也没有多页 tab。在 `320x172` 这块小屏上，五块固定区域更稳，也更符合 bring-up 时“点哪里看哪里”的习惯。

### 4.3 二级详情页共用一套骨架

所有二级详情页都沿用同一套结构：

- 顶栏左侧固定 `BACK`
- 中间是页面标题
- 右侧是状态 chip
- 上半区显示主数值
- 下半区显示 2~4 组子信息
- 底栏只保留 1 行 notice，不叠二级控制

这样做的好处有三点：

- 不用为每一页重新学习导航方式。
- 代码侧可以复用同一组几何锚点和状态 chip 逻辑。
- 评审时更容易看出不同页面有没有信息越界。

这里有一个例外：`Charger Detail` 左侧会话区还能继续下钻。

| 区域 | 几何（px） | 跳转 |
| --- | --- | --- |
| charger manual entry | `x=6 y=60 w=150 h=82` | `MANUAL CHARGE` |

所以 `Charger Detail` 不是终点页，而是只读信息和手动控制之间的分界。

### 4.4 `MANUAL CHARGE` 是操作页，不是状态页

`MANUAL CHARGE` 负责手动充电偏好与运行时控制。它也是这套前面板 UI 里唯一负责发起控制动作的页面。

### 页面布局

| 区域 | 几何（px） | 作用 |
| --- | --- | --- |
| `TARGET` row | `x=6 y=24 w=308 h=30` | 选择 `3.7V / 80% / 100%` |
| `SPEED` row | `x=6 y=58 w=308 h=30` | 选择 `100mA / 500mA / 1A` |
| `TIMER` row | `x=6 y=92 w=308 h=30` | 选择 `1h / 2h / 6h` |
| `BACK` | `x=6 y=132 w=88 h=30` | 返回 `CHARGER DETAIL` |
| `STATUS` | `x=100 y=132 w=120 h=30` | 显示 footer notice |
| `START/STOP` | `x=226 y=132 w=88 h=30` | 发起或停止手动充电 |

### 运行时规则

- 未充电时：三组设置可编辑，右下角动作是 `START`
- `manual active` 或自动充电已经在工作时：三组设置锁定，右下角动作统一解释为 `STOP`
- 同一个右下角热区在运行中会从 `ManualStart` 自动解析为 `ManualStop`，避免页面上出现第二个停止按钮
- 返回路径固定回到 `CHARGER DETAIL`，不直接跳回 Dashboard 首页

这一页把返回、状态和执行动作都放到底部一排，是因为在小屏上这样更顺手，也更容易盲点。

## 5. 每类页面各管什么

### 5.1 `SELF CHECK`

- 用来说明“哪些模块已经准备好、哪些还在等待、哪些真的坏了”。
- 不负责常规运行监控，也不适合长期停留浏览。
- 看这页时，通常先看 `BQ40Z50`、`BQ25792`、`TPS55288-A/B`，再看屏幕链路和温度链路。

### 5.2 Dashboard 首页

- 用来给出当前 UPS 模式、主功率信息，以及充放电与电池健康总览。
- 不会把每个子系统的全部字段都铺开。
- 通常是先看模式，再看左侧主 KPI，最后看右侧三卡。

### 5.3 二级详情页

- 每页只把一个子系统讲清楚。
- 不负责跨子系统解释。
- 字段尽量只留在自己的页面里，不把 `ChargeCard` 的语义搬到 `Output Detail`。

### 5.4 `MANUAL CHARGE`

- 只处理“手动充电偏好”和“启动 / 停止”。
- 不承担展示全部 charger 原始寄存器的任务。
- 设置和动作分开；动作只在底部唯一热区执行。

## 6. 状态词怎么读

### 6.1 顶层模式

| 模式 | 含义 |
| --- | --- |
| `BYPASS` | 输入直通输出 |
| `STANDBY` | 输入在线，但输出级未实际出力 |
| `ASSIST` | 输入在线，输出级参与供电 |
| `BACKUP` | 输入离线，电池承担输出 |

### 6.2 `SELF CHECK` 状态词

| 状态词 | UI 含义 |
| --- | --- |
| `PEND` | 模块初始化或探测还未完成 |
| `OK` | 模块可达，且当前运行条件满足 |
| `WARN` | 模块可达，但存在前提不足或异常态 |
| `ERR` | 模块不可达或普通访问失败 |
| `N/A` | 当前构型下不参与 |
| `RUN` | 模块正在工作（常见于 charger / output） |
| `IDLE` | 模块在线，但当前不需要动作 |
| `LOCK` | 模块被策略锁住，不是硬件消失 |
| `HOT` | 温度链路在线，但热状态已经升高 |

### 怎么理解 `TPS55288` 的 `WARN`

`TPS55288-A/B` 有一个容易误读的地方：

- 如果上游 `BMS` 还没放行，或者 `VBAT` 还无法可信判断，那么 `TPS` 的探测失败会在 UI 上降级成 `WARN`，并给出 `WAIT BMS` 或 `VBAT UNK`
- 只有在上游条件已经满足后，`TPS` 仍然失败，才应该读成真正的 `ERR`

所以 `WAIT BMS` 更像“系统前提还没满足”，不是“输出芯片已经坏了”。

### 6.3 `MANUAL CHARGE` 顶栏模式词

| 文案 | 条件 |
| --- | --- |
| `AUTO` | 没有手动会话，自动充电也未在工作 |
| `AUTO CHG` | 当前由自动充电策略驱动 |
| `MANUAL` | 手动会话已启动 |
| `TAKEOVER` | 手动会话接管当前充电过程 |
| `STOPPED` | 手动会话刚停止，自动策略处于 hold |

### 6.4 `MANUAL CHARGE` 底栏文案

| 文案 | 条件 |
| --- | --- |
| `LIVE DATA` | 当前仅显示运行数据 |
| `MANUAL ACTIVE` | 手动充电正在进行 |
| `AUTO HELD` | 手动停止后，自动策略被临时保持 |
| `TIMER DONE` | 定时器到点 |
| `3.7V DONE` | 目标电压达成 |
| `80% DONE` | 目标 SOC 达成 |
| `100% DONE` | 满充目标达成 |
| `SAFETY STOP` | 被安全条件阻断 |

## 7. 布局和视觉约定

这套 UI 不是在追求“像手机 App”，而是在追求小屏工业界面的可读性。现有规则如下：

- 画布固定 `320x172`，不做自适应布局
- 顶栏高度固定 `18px`
- 卡片为直角矩形，不使用圆角语义
- 非数字用 Font A，数字用等宽 Font B
- 关键状态词使用固定词形，不做同义词漂移
- Dashboard 用 `InstrumentB` 调色板，`SELF CHECK` 用 `RetroC` 调色板
- 交互高亮只表达焦点，不表达业务状态

如果后面改动这些规则，那就属于 UI 设计改版，不只是文案调整。

## 8. 代表画面（只保留帮助理解交互的代表图）

### `SELF CHECK`：正常待机与等待上游

![Self-check 正常待机](/ui/self-check-c-standby-idle.png)

![Self-check - BMS 缺失且 TPS 等待](/ui/self-check-c-bms-missing-tps-warn.png)

### Dashboard：首页与运行态

![Dashboard - STANDBY](/ui/dashboard-b-standby-mode.png)

![Dashboard - BACKUP](/ui/dashboard-b-backup-mode.png)

### `MANUAL CHARGE`：控制页

![Manual charge - default](/ui/manual-charge-default.png)

![Manual charge - active](/ui/manual-charge-active.png)

## 9. 继续阅读

- 页面全貌与运行路径： [前面板屏幕页面总览](/design/front-panel-screen-pages)
- 硬件链路与运行时基线： [前面板与固件](/design/front-panel-and-firmware)
- 上电观察点与排障顺序： [固件烧录与首次自检](/manual/firmware-flash-and-self-test)

## 10. 内部参考文档

- [前面板 PCB 说明](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/pcbs/front-panel/README.md)
- [开机自检流程](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/boot-self-test-flow.md)
- [前面板固件 UI 内部文档](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/ui/README.md)
- [Self-check UI 设计](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/ui/self-check-design.md)
- [Dashboard UI 设计](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/ui/dashboard-design.md)
- [Dashboard Detail UI 设计](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/ui/dashboard-detail-design.md)
- [组件契约](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/ui/component-contracts.md)
- [视觉语言](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/ui/design-language.md)
