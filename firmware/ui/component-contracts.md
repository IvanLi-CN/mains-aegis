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
- Allowed states: `STANDBY` 显示 `READY/CHG`；其余模式必须 `LOCK/NOAC`。
- Token refs: `Type.Title`, `Type.Num`, `Color.State.Success|Warning|Error`（按状态选择）。
- Geometry anchor: `x=206 y=72 w=108 h=48`。

### DischgCard

- Responsibility: 电池放电电流与放电状态。
- Required fields: `idchg_a`, `dischg_state`。
- Forbidden fields: 充电电流字段。
- Allowed states: `BYPASS/STANDBY` 通常为 `0A`，`ASSIST/BACKUP` 随负载变化。
- Token refs: `Type.Title`, `Type.Num`, `Color.Surface.Panel`。
- Geometry anchor: `x=206 y=122 w=108 h=48`。

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
