# Front panel visual regression checklist

本清单用于验证视觉规范是否落实。每条规则都绑定现有资产与通过标准。

## 1. How to use

- 先阅读 `design-language.md` 与 `component-contracts.md`。
- 逐条核对本清单中的规则与资产。
- 任一条未满足即判定本轮视觉验收不通过。

## 2. Dashboard checks

### VR-D-01 Top bar title and mode

- Assets:
  - `assets/dashboard-b-off-mode.png`
  - `assets/dashboard-b-standby-mode.png`
  - `assets/dashboard-b-supplement-mode.png`
  - `assets/dashboard-b-backup-mode.png`
- Pass criteria:
  - 标题区域固定在顶栏，模式词仅出现 `BYPASS/STANDBY/ASSIST/BACKUP`。
  - 标题与模式文本使用 `Type.Title`/`Color.Text.Primary` 对比规则。

### VR-D-02 Main KPI hierarchy

- Assets:
  - `assets/dashboard-b-standby-mode.png`
  - `assets/dashboard-b-backup-mode.png`
- Pass criteria:
  - `KpiPanel` 标签与大数字层级清晰，数字使用 `Type.NumBig`。
  - `STANDBY` 场景显示 `PIN + POUT`，`BACKUP` 场景显示 `POUT + IOUT`。

### VR-D-03 Charge/Discharge ownership

- Assets:
  - `assets/dashboard-b-off-mode.png`
  - `assets/dashboard-b-standby-mode.png`
  - `assets/dashboard-b-supplement-mode.png`
  - `assets/dashboard-b-backup-mode.png`
- Pass criteria:
  - `ChargeCard` 仅显示充电字段，不混入放电或负载字段。
  - `DischgCard` 仅显示放电字段，不混入充电字段。
  - `ChargeCard` 首页状态必须来自 runtime 紧凑 token，而不是 `UpsMode` 直推。

### VR-D-04 Battery card semantics

- Assets:
  - `assets/dashboard-b-standby-mode.png`
  - `assets/dashboard-b-supplement-mode.png`
- Pass criteria:
  - `BatteryCard` 保持 `SOC + Tmax + battery_state`，不替换为输出侧指标。

## 3. Self-check checks

### VR-S-01 Card matrix integrity

- Assets:
  - `assets/self-check-c-standby-idle.png`
- Pass criteria:
  - 10 张诊断卡完整显示，双列布局与坐标锚点一致。
  - 每卡两行：第一行 `MODULE + COMM`，第二行 `KEY PARAM`。

### VR-S-02 Communication state vocabulary

- Assets:
  - `assets/self-check-c-standby-right.png`
  - `assets/self-check-c-assist-up.png`
  - `assets/self-check-c-backup-touch.png`
- Pass criteria:
  - `COMM` 状态词仅出现规范词汇：基础态 `PEND/OK/WARN/ERR/N/A` 与派生态 `RUN/LOCK/IDLE/HOT/CHG/WAIT/FULL/WARM/TEMP/LOAD/CHG500/CHG100`。
  - 交互高亮由 `UiFocus` 驱动，不改变模块业务语义。

### VR-S-03 Module naming consistency

- Assets:
  - `assets/self-check-c-standby-idle.png`
- Pass criteria:
  - 模块名与规范一致：`GC9307`、`TCA6408A`、`FUSB302`、`INA3221`、`BQ25792`、`BQ40Z50`、`TPS55288-A`、`TPS55288-B`、`TMP112-A`、`TMP112-B`。

## 4. Overlay checks (BQ40 activation)

### VR-O-01 Overlay transition coverage

- Assets:
  - `assets/self-check-c-bq40-offline-idle.png`
  - `assets/self-check-c-bq40-offline-activate-dialog.png`
  - `assets/self-check-c-bq40-activating.png`
  - `assets/self-check-c-bq40-result-success.png`
  - `assets/self-check-c-bq40-result-no-battery.png`
  - `assets/self-check-c-bq40-result-rom-mode.png`
  - `assets/self-check-c-bq40-result-abnormal.png`
  - `assets/self-check-c-bq40-result-not-detected.png`
- Pass criteria:
  - 场景覆盖 `Idle -> Confirm -> Pending -> Success/NoBattery/RomMode/Abnormal/NotDetected`。
  - 五类结果弹窗标题统一为 `BQ40 RESULT`，正文文案与卡片 `OK/WARN/ERR` 语义不冲突。

### VR-O-02 Dialog geometry and action areas

- Assets:
  - `assets/self-check-c-bq40-offline-activate-dialog.png`
- Pass criteria:
  - 对话框区域与按钮区域落在契约锚点范围。
  - 文案使用 `Type.Body`，结果与动作按钮具备足够对比。

## 5. Offline-readability checks

### VR-G-01 No external image dependency

- Command:
  - `rg -n '![^\n]*\(https?://' firmware/ui docs`
- Pass criteria:
  - 扫描结果为空。

### VR-G-02 Entry reachability

- Targets:
  - `docs/README.md`
  - `docs/specs/README.md`
  - `firmware/ui/README.md`
- Pass criteria:
  - 从 docs 入口两跳内可达 `design-language.md`、`component-contracts.md`、`visual-regression-checklist.md`。

### VR-G-03 Bitmap font whitelist conformance

- Command:
  - `rg -n "static FONT_|u8g2_font_" firmware/src/front_panel_scene.rs`
  - `rg -n "Type.NumCompact|13px|14px|22px|u8g2_font_8x13B_tf|u8g2_font_7x14B_tf|u8g2_font_8x13_mf|u8g2_font_t0_22b_tn" firmware/ui/design-language.md`
- Pass criteria:
  - 代码侧字体绑定仅包含白名单项：`8x13B`、`7x14B`、`8x13_mf`、`t0_22b_tn`。
  - 文档侧明确 `Type.Title/Body/Compact/Num/NumCompact/NumBig` 对应字体与字高。
  - 字高白名单固定为 `13/14/22`，且无 `<10px` 字体。

### VR-G-04 Preview artifacts reachable

- Targets:
  - `docs/specs/hg3dw-front-panel-visual-language/assets/color-preview.svg`
  - `docs/specs/hg3dw-front-panel-visual-language/assets/typography-preview.svg`
  - `firmware/ui/design-language.md`
- Pass criteria:
  - 预览图文件存在且可由 `design-language.md` 直接访问。
