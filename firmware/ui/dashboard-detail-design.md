# Dashboard Detail UI 设计（Variant B Drill-down）

本文件定义 Dashboard 二级详情页的模块布局、入口映射与文案冻结口径，并补充 `Cells -> BMS DETAIL` 与 charger detail 向下钻取的 `MANUAL CHARGE` 三级页。

## 1. 基线

- 首页基线：`dashboard-design.md`
- 视觉规范：`design-language.md`
- 组件契约：`component-contracts.md`
- 分辨率：`320x172`

## 2. 首页入口映射

| 首页区域 | 几何（px） | 进入页面 |
| --- | --- | --- |
| 主 KPI 面板 | `x=6 y=22 w=196 h=52` | `Output` |
| 次级信息面板 | `x=6 y=76 w=196 h=94` | `Thermal` |
| `BATTERY` | `x=206 y=22 w=108 h=48` | `Cells` |
| `CHARGE` | `x=206 y=72 w=108 h=48` | `Charger` |
| `DISCHG` | `x=206 y=122 w=108 h=48` | `Battery Flow` |

首页只加轻量可点语义，不改模块主信息架构。

## 3. 详情页通用骨架

- 顶栏：左侧 `BACK`，中间页面标题，右侧状态 chip。
- 主体上半区：主指标块（大数值 + 摘要标签）。
- 主体下半区：2~4 个信息卡，承载状态、分组指标与子系统摘要。
- 底栏：异常/提示条，固定 1 行。
- 唯一多一级规则：`Cells` 主体区允许进入 `BMS DETAIL`；`BMS DETAIL` 返回到 `Cells`，不引入通用 history stack。

## 4. 页面冻结口径

### `Cells`

- 顶栏标题：`CELL DETAIL`
- 状态 chip：`BAL ON / READY / OFF / WARN / FAULT`
- 主区：4 节电压栅格
- 次区：`BAL STATE`、4 路温度、充放电状态
- `BAL STATE` 冻结语义：
  - `OFF`：均衡 DF 可读且 `CB=0`
  - `IDLE`：均衡 DF 可读且 `CB=1`，但 `OperationStatus[CB]=0`
  - `C1 / C2 / C3 / C4`：`CB=1` 且 `AFE Register[KK]` 为 one-hot
  - `MULTI`：`CB=1` 且 `AFE Register[KK]` 为多 bit
  - `ACTIVE`：`CB=1` 但缺少可用 `AFE KK`
  - `N/A`：均衡配置或活动态未读到
- 底栏 notice：
  - `EXT CHG+RELAX`：主板均衡基线匹配（外部均衡 + charge/rest 开启）
  - `CFG MISMATCH`：DF 可读但与主板均衡基线不匹配
  - `BAL CFG PENDING`：均衡配置尚未读到

### `Battery Flow`

- 顶栏标题：`BATTERY FLOW`
- 状态 chip：`CHG / DSG / IDLE / FAULT`
- 主区：`VPACK` + `IPACK`
- 次区：`ENERGY / FULL CAP / CHG / DSG / PCHG`
- 底栏：battery abnormal summary

### `BMS Detail`

- 顶栏标题：`BMS DETAIL`
- 状态 chip：`READY / LIMIT / WARN / FAULT / N/A`
- 顶部 summary band：`REMCAP / FCC / TO FULL`（单位固定 `mAh`）
- 中部 band：左侧 `REASON` pill，右侧 `BAL` + 4 节 cell 图形
- `BAL` 只显示图形化活跃 cell：亮起=active，灰色=inactive，黄色未知=mask 不可读
- 下部状态 tiles：
  - 第一行：`CRDY / XCG / CFET / FC / PF`
  - 第二行：`DRDY / XDG / DFET / FD / RCA`
- tiles 统一使用 icon-first：`绿色=ok`、`黄色=warn/limit`、`红色=fault/alarm`、`灰色=off/unknown`
- 不显示 raw `BAL MASK`、raw reason token 或十六进制状态字

### `Output`

- 顶栏标题：`OUTPUT DETAIL`
- 状态 chip：`REG OK / WARN / FAULT`
- 主区：`VOUT` + `POUT`
- 次区：`OUT-A`、`OUT-B`、温度、异常
- 关闭路规则：电流固定 `--`

### `Charger`

- 顶栏标题：`CHARGER DETAIL`
- 状态 chip：`CHG1A / CHG500 / CHG100 / WAIT / FULL / WARM / TEMP / LOAD / LOCK / NOAC / WARN / FAULT`
- 主区：输入来源图标 + `IN W`；电池图标 + `CHARGE W`
- 次区：charging state / source select / status detail
- 底栏：charger abnormal summary
- 左侧会话面板热区：`x=6 y=60 w=150 h=82`，进入 `MANUAL CHARGE`

### `Manual Charge`

- 顶栏标题：`MANUAL`
- 顶栏为单层信息条：左侧 `MODE`，中间标题，右侧 status chip，不承载返回触摸区
- 返回方式：底部 `BACK`、`LEFT/CENTER`
- 中部三条横向 segmented rows：
  - `TARGET`: `3.7V / 80% / 100%`
  - `SPEED`: `100mA / 500mA / 1A`
  - `TIMER`: `1h / 2h / 6h`
- 三组字段不再绘制外层卡片边框，直接用 label + segmented options，优先保证小屏可读性与点击面积。
- 底部唯一操作条：左侧 `BACK`，中部 footer notice，右侧 `START/STOP`
- 运行语义：
  - 未充电：允许编辑偏好并显示 `START`
  - 自动充电或手动充电进行中：设置区锁定，只允许 `STOP/BACK`
  - `MODE` 词形固定为 `AUTO / AUTO CHG / MANUAL / TAKEOVER / STOPPED`
  - footer notice 固定映射：`LIVE DATA / MANUAL ACTIVE / AUTO HELD / TIMER DONE / 3.7V DONE / 80% DONE / 100% DONE / SAFETY STOP`
  - `目标完成 / TIMER DONE / FULL` 后保持 runtime hold，避免自动策略在同轮立即恢复；`SAFETY STOP` 仅展示阻断原因，不建立 hold

### `Thermal`

- 顶栏标题：`THERMAL DETAIL`
- 状态 chip：`COOL / WARM / HOT / FAULT`
- 主区：最高温度 + fan 状态
- 次区：TMP / board / battery / fan PWM / tach
- 底栏：thermal protection hint

## 5. 视觉方向

- 保持 `Variant B` 的深色工业底板与橙色强调色。
- 相比首页，详情页减少缩写密度，优先使用完整英文词组。
- 信息卡留白略增，数字分层更明确，异常条用更稳定的语义色。

## 6. 冻结渲染图

![Dashboard Detail - Home](assets/dashboard-b-detail-home.png)
![Dashboard Detail - Cells](assets/dashboard-b-detail-cells.png)
![Dashboard Detail - BMS](../../docs/specs/f3c2g-dashboard-detail-drilldown/assets/dashboard-detail-bms.png)
![Dashboard Detail - Battery Flow](assets/dashboard-b-detail-battery-flow.png)
![Dashboard Detail - Output](assets/dashboard-b-detail-output.png)
![Dashboard Detail - Charger](assets/dashboard-b-detail-charger.png)
![Dashboard Detail - Thermal](assets/dashboard-b-detail-thermal.png)
![Dashboard Detail - Icons](assets/dashboard-detail-icons.png)

## 7. Manual charge 冻结渲染图

![Manual Charge - Default](../../docs/specs/zp4cg-manual-charge-dashboard/assets/manual-charge-default.png)
![Manual Charge - Active](../../docs/specs/zp4cg-manual-charge-dashboard/assets/manual-charge-active.png)
![Manual Charge - Stop Hold](../../docs/specs/zp4cg-manual-charge-dashboard/assets/manual-charge-stop-hold.png)
![Manual Charge - Reset Auto](../../docs/specs/zp4cg-manual-charge-dashboard/assets/manual-charge-reset-auto.png)
![Manual Charge - Blocked](../../docs/specs/zp4cg-manual-charge-dashboard/assets/manual-charge-blocked.png)
