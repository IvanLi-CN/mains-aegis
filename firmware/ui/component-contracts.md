# Front panel component contracts

本文件定义前面板核心组件契约：职责、必填字段、禁用字段、状态边界与几何锚点来源。

## 1. Global contract

- 几何来源：以 `dashboard-design.md` 与 `self-check-design.md` 中冻结坐标为准。
- 视觉来源：以 `design-language.md` Token 体系为准。
- 字体来源：以 `design-language.md` 中 bitmap 字体白名单与字高白名单为准。
- 字段规则：组件只承载自身职责字段，不跨组件复用语义。

## 2. Dashboard components

### TopBar

- Responsibility: 页面标题与模式状态位展示。
- Required fields: `title`, `mode_label`, `irq_flag(optional)`。
- Forbidden fields: 业务数值（`PIN/POUT/IOUT/SOC/TMAX`）。
- Allowed states: `UpsMode` 全部四态。
- Token refs: `Type.Title`, `Color.Text.Primary`, `Color.State.Accent`。
- Geometry anchor: `x=0 y=0 w=320 h=18`。

### KpiPanel

- Responsibility: 主数值区展示 `PIN/POUT` 或 `POUT/IOUT`。
- Required fields: `primary_label`, `secondary_label`, `primary_value`, `secondary_value`。
- Forbidden fields: 电池状态词（`BAL/CHG/DSG/...`）。
- Allowed states: `UpsMode` 全部四态（字段组合按模式切换）。
- Token refs: `Type.Body`, `Type.NumBig`, `Color.Surface.Panel`, `Color.Text.Primary`。
- Geometry anchor: `x=6 y=22 w=196 h=52`。

### InfoPanel

- Responsibility: 次级信息区，承载模式说明与附加指标。
- Required fields: `line_1` ~ `line_4`（或等效文本块）。
- Forbidden fields: 与右侧三卡重复的卡片状态词。
- Allowed states: `UpsMode` 全部四态。
- Token refs: `Type.Body`, `Type.Num`, `Color.Surface.PanelAlt`, `Color.Text.Secondary`。
- Geometry anchor: `x=6 y=76 w=196 h=94`。

### BatteryCard

- Responsibility: `SOC + Tmax + battery_state`。
- Required fields: `soc_pct`, `tmax_c`, `battery_state`。
- Forbidden fields: 负载输出电流与输入侧功率字段。
- Allowed states: `UpsMode` 全部四态。
- Token refs: `Type.Title`, `Type.Num`, `Color.Surface.Panel`, `Color.Border.Default`。
- Geometry anchor: `x=206 y=22 w=108 h=48`。

### ChargeCard

- Responsibility: 电池充电电流与充电状态。
- Required fields: `ichg_a`, `charge_state`。
- Forbidden fields: 放电电流、输出负载电流。
- Allowed states: 首页紧凑 token 固定为 `CHG / WAIT / FULL / WARM / TEMP / LOAD / LOCK / NOAC`；其中 `CHG1A/CHG500/CHG100` 在首页压缩为 `CHG`。
- Token refs: `Type.Title`, `Type.Num`, `Color.State.Success|Warning|Error`（按状态选择）。
- Geometry anchor: `x=206 y=72 w=108 h=48`。

### DischgCard

- Responsibility: 电池放电电流与放电状态。
- Required fields: `idchg_a`, `dischg_state`。
- Forbidden fields: 充电电流字段。
- Allowed states: `BYPASS/STANDBY` 通常为 `0A`，`ASSIST/BACKUP` 随负载变化。
- Token refs: `Type.Title`, `Type.Num`, `Color.Surface.Panel`。
- Geometry anchor: `x=206 y=122 w=108 h=48`。

### DashboardDetailPage

- Responsibility: 二级仪表盘全屏详情页容器，承载 `Cells / BatteryFlow / Output / Charger / Thermal` 之一。
- Required fields: `page_id`, `title`, `status_chip`, `primary_block`, `secondary_blocks`, `footer_notice`。
- Forbidden fields: 自检 overlay、BQ40 激活流程状态。
- Allowed states: `DashboardRoute::Detail(*)`。
- Token refs: `Type.Title`, `Type.Body`, `Type.Num`, `Type.NumBig`, `Color.State.Accent|Warning|Error|Success`。
- Geometry anchor: 顶栏 `x=0 y=0 w=320 h=18`；内容区 `x=6 y=22 w=308 h=148`。

### DashboardBackChip

- Responsibility: 详情页返回首页触点与可视标识。
- Required fields: `label=BACK`, `active(optional)`。
- Forbidden fields: 业务数据。
- Allowed states: 任一 `DashboardDetailPage`。
- Token refs: `Type.Body`, `Color.Border.Default`, `Color.Text.Primary`, `Color.State.Accent`。
- Geometry anchor: `x=8 y=2 w=56 h=14`。

### ManualChargeEntryHotZone

- Responsibility: `CHARGER DETAIL` 左侧会话面板进入 `MANUAL CHARGE` 的点击热区与轻量 entry marker。
- Required fields: `target_route=DashboardRoute::ManualCharge`, `entry_marker`.
- Forbidden fields: 独立业务数值；其展示内容仍归 `Charger` detail 页面所有。
- Allowed states: `DashboardRoute::Detail(Charger)`。
- Token refs: `Color.Focus.Right`, `Color.Border.Default`。
- Geometry anchor: `x=6 y=60 w=150 h=82`。

### ManualChargePage

- Responsibility: 手动充电页容器，承载偏好设置、运行时模式与 `START/STOP` 控制。
- Required fields: `prefs`, `runtime`, `status_chip`, `footer_notice`, `action_label`.
- Forbidden fields: 持久化手动会话状态；运行时状态不得越过 `PowerManager` RAM 边界。
- Allowed states: `DashboardRoute::ManualCharge`。
- Token refs: `Type.Title`, `Type.Body`, `Type.Num`, `Type.NumBig`, `Color.State.Accent|Warning|Error|Success`。
- Geometry anchor:
  - top info bar: `x=0 y=0 w=320 h=20`（左 `MODE` / 中 `MANUAL` / 右 status）
  - `TARGET` row: `x=6 y=24 w=308 h=30`
  - `SPEED` row: `x=6 y=58 w=308 h=30`
  - `TIMER` row: `x=6 y=92 w=308 h=30`
  - action bar: `BACK x=6 y=132 w=88 h=30` / `STATUS x=100 y=132 w=120 h=30` / `START|STOP x=226 y=132 w=88 h=30`
  - top bar is info-only on this page; no dedicated back chip，也不再保留第二层 info strip。

### ManualChargeOptionGroup

- Responsibility: `TARGET / SPEED / TIMER` 三组选项的单选显示与锁定态提示。
- Required fields: `group_title`, `options[3]`, `selected`, `locked`.
- Forbidden fields: 业务状态机副作用；点击只发出 UI action，不直接改 charger。
- Allowed states: `DashboardRoute::ManualCharge`。
- Token refs: `Type.Body`, `Type.Num`, `Color.Focus.Left|Right|Center`, `Color.Text.Secondary`。
- Geometry anchor: segmented row `x=6 w=308 h=30`；label 区保留左侧 `~70px`，3 个 segment 各 `74x24`，间距 `4px`；不再绘制外层 row card，仅保留 label + segments。

## 3. Self-check components

### DiagCard

- Responsibility: 单模块通信状态与关键参数。
- Required fields: `module_name`, `comm_state`, `key_param`。
- Forbidden fields: 不属于该模块的参数。
- Allowed states: 基础态 `PEND/OK/WARN/ERR/N/A` + 模块派生态 `RUN/LOCK/IDLE/HOT`。
- `comm_state` 语义由固定词形承载，不通过状态色做额外区分。
- Token refs: `Type.Compact`, `Type.NumCompact`, `Color.Text.Primary`, `Color.Text.Secondary`。
- Geometry anchor: 左列 `x=6`，右列 `x=163`，每卡高 `29`。

### TopBar (Self-check)

- Responsibility: 固定标题 `SELF CHECK` 与当前 `UpsMode`。
- Required fields: `title`, `mode_label`。
- Forbidden fields: 模块级参数。
- Allowed states: `UpsMode` 全部四态。
- Token refs: `Type.Title`, `Color.Text.Primary`, `Color.State.Accent`。
- Geometry anchor: `x=0 y=0 w=320 h=18`。

## 4. Overlay components

### ActivationDialog

- Responsibility: BQ40 激活确认、进度、结果反馈。
- Required fields: `dialog_title`, `dialog_body`, `action_buttons`, `result_state`。
- Forbidden fields: 与主页面 KPI 重复显示。
- Allowed states: `Idle`（无框）、`Confirm`、`Pending`、`ResultSuccess`、`ResultNoBattery`、`ResultRomMode`、`ResultAbnormal`、`ResultNotDetected`。
- Token refs: `Type.Body`, `Type.Num`, `Color.Surface.PanelAlt`, `Color.Border.Default`, `Color.State.Success|Error|Warning`。
- Geometry anchor: 对话框 `x=20 y=34 w=280 h=112`；按钮按实现锚点固定。

## 5. Field ownership guardrails

- `ChargeCard` 与 `DischgCard` 不得互相承载对方电流字段。
- `BatteryCard` 必须保留 `SOC + Tmax` 核心字段，不得替换为电压等其它主字段。
- `DiagCard` 的 `key_param` 必须可映射到对应模块采样或状态源。
- `UiFocus` 影响高亮，不得作为业务模式来源。
- 不允许组件绕过 Token 直接指定新字体；字体必须落在字高白名单 `13/14/22`（且不得小于 `10px`）。

## 6. Mapping references

- 视觉 Token：`design-language.md`
- 页面布局：`dashboard-design.md`、`self-check-design.md`
- 回归验收：`visual-regression-checklist.md`
- 实现参考：`../src/front_panel_scene.rs`
