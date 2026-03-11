# BQ40 自检异常态与结果弹窗（#5cvrj）

## 状态

- Status: 部分完成（4/5）
- Created: 2026-03-11
- Last: 2026-03-11

## 背景 / 问题陈述

- 现有 `SELF CHECK` 页面会把 `BQ40Z50` 的多种异常混在 `WARN/ERR` 与单一失败弹窗里，难以区分“设备存在但异常”与“完全未识别到设备”。
- 激活结果弹窗关闭后不会保留结果语义，后续再次点击 `BQ40Z50` 卡片时无法直接回看最近一次结果。
- 新需求要求先补足结果弹窗渲染图，并让运行时把激活结果固化为可复看的状态。

## 目标 / 非目标

### Goals

- 把 `BQ40Z50` 卡片收敛为三层状态：`OK`、`WARN`、`ERR`。
- `WARN` 固定表示“设备存在但非正常态”，不再把 `RCA` 作为独立卡片状态词；`RCA ALARM` 仅作为副文案显示。
- 默认自检只做普通 SMBus/SBS 访问探测；普通访问未识别到设备时显示 `ERR`，并允许尝试激活。
- 激活结果固定收敛为 5 类：`SUCCESS`、`NO BATTERY`、`ROM MODE`、`ABNORMAL`、`NOT DETECTED`。
- 结果弹窗关闭后保留最近一次结果；再次点击 `BQ40Z50` 卡片时直接回显对应结果弹窗。

### Non-goals

- 不在前面板直接触发 `recover/flash` 之类 ROM 写入流程。
- 不改 `Variant C` 的 10 卡布局与其它 9 张卡片语义。
- 不实现“成功后自动重试/自动恢复”之外的额外交互菜单。

## 范围（Scope）

### In scope

- `firmware/src/front_panel_scene.rs`
  - 新增持久结果枚举与结果驱动 overlay。
  - 更新 `BQ40Z50` 卡片状态词与副文案映射。
- `firmware/src/front_panel.rs`
  - 点击/按键行为改为“`ERR` 首次可激活；已有结果则直接回显结果弹窗”。
  - 关闭结果弹窗时仅清 overlay，不清最近一次结果。
- `firmware/src/output/mod.rs`
  - 普通访问状态映射：`OK/WARN/ERR`。
  - 激活结果状态持久化，并把运行态结果同步到 `SelfCheckUiSnapshot`。
  - 激活唤醒参数对齐到 `VREG=16.8V / ICHG=200mA / IINDPM=500mA`。
- `firmware/src/bq25792.rs`
  - 补足 `CHARGE_VOLTAGE_LIMIT` 与设置 `VREG` 的 helper。
- `tools/front-panel-preview/src/main.rs`
  - 新增 5 个结果弹窗场景与对应 PNG 导出。
- 视觉文档与规格资产
  - `firmware/ui/self-check-design.md`
  - `firmware/ui/visual-regression-checklist.md`
  - `firmware/ui/README.md`
  - `firmware/README.md`
  - `docs/specs/README.md`

### Out of scope

- 新增中文屏幕字体或中英混排排版方案。
- 为 `BQ40Z50` 新增更多细粒度结果分类。
- 修改 `tools/bq40-comm-tool` 的工具链契约。

## 接口变更（Interfaces）

- `front_panel_scene::BmsResultKind`：新增，固定 5 类结果状态。
- `front_panel_scene::SelfCheckOverlay`：从布尔成功/失败结果改为显式结果 overlay。
- `front_panel_scene::SelfCheckUiSnapshot::bq40z50_last_result`：新增，承载最近一次激活结果。
- `output::PowerManager::clear_bms_activation_state()`：改为只清当前激活态，不清最近一次结果。
- `bq25792::set_charge_voltage_limit_mv(...)`：新增。

## 验收标准（Acceptance Criteria）

- Given 普通访问拿到可信且正常的 BQ40 快照，When 查看 `BQ40Z50` 卡片，Then 状态显示 `OK`。
- Given 普通访问确认设备存在但状态不正常，When 查看 `BQ40Z50` 卡片，Then 状态显示 `WARN`，副文案显示 `ABNORMAL` 或 `RCA ALARM`。
- Given 普通访问未识别到设备，When 查看 `BQ40Z50` 卡片，Then 状态显示 `ERR`，副文案显示 `NOT DETECTED`。
- Given `BQ40Z50` 卡片为 `ERR`，When 点击或按键触发激活，Then 先进入确认弹窗，再进入进度弹窗，并最终落到 5 类结果之一。
- Given 任一结果弹窗已关闭，When 再次点击 `BQ40Z50` 卡片，Then 直接显示最近一次结果弹窗，不重复进入确认流程。
- Given 最近一次结果为 `NOT DETECTED`，When 结果弹窗关闭后回到自检页，Then `BQ40Z50` 卡片仍保持 `ERR`。
- Given 最近一次结果为 `ABNORMAL`，When 结果弹窗关闭后回到自检页，Then `BQ40Z50` 卡片保持 `WARN`。
- Given 运行 `tools/front-panel-preview` 的结果场景，When 导出 PNG，Then 5 张结果弹窗图分辨率均为 `320x172`。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Firmware build: `cargo +esp build --manifest-path firmware/Cargo.toml --release --target xtensa-esp32s3-none-elf -Zbuild-std=core,alloc`
- Preview build/run: `cargo run --manifest-path tools/front-panel-preview/Cargo.toml -- --variant C --mode standby --focus idle --scenario <scenario> --out-dir <ABS_PATH>`

### Quality checks

- 新增结果弹窗 PNG 必须全部为 `320x172`。
- `Variant C` 其它模块卡片几何与字体层级不得漂移。

## 文档更新（Docs to Update）

- `firmware/ui/self-check-design.md`: 更新 BQ40 卡片口径与结果弹窗资产。
- `firmware/ui/visual-regression-checklist.md`: 新增 5 类结果弹窗检查项。
- `firmware/ui/README.md`: 同步新的 self-check 资产清单。
- `firmware/README.md`: 更新前面板 BQ40 激活说明与预览命令说明。
- `docs/specs/README.md`: 新增规格索引并在完成后同步状态。

## 计划资产（Plan assets）

- Directory: `docs/specs/5cvrj-bq40-self-check-result-dialogs/assets/`
- Result dialog assets:
  - `self-check-c-bq40-result-success.png`
  - `self-check-c-bq40-result-no-battery.png`
  - `self-check-c-bq40-result-rom-mode.png`
  - `self-check-c-bq40-result-abnormal.png`
  - `self-check-c-bq40-result-not-detected.png`
  - `self-check-c-bq40-offline-activate-dialog.png`
  - `self-check-c-bq40-activating.png`

## Visual Evidence (PR)

![BQ40 activate confirm](./assets/self-check-c-bq40-offline-activate-dialog.png)
![BQ40 activating](./assets/self-check-c-bq40-activating.png)
![BQ40 result success](./assets/self-check-c-bq40-result-success.png)
![BQ40 result no battery](./assets/self-check-c-bq40-result-no-battery.png)
![BQ40 result ROM mode](./assets/self-check-c-bq40-result-rom-mode.png)
![BQ40 result abnormal](./assets/self-check-c-bq40-result-abnormal.png)
![BQ40 result not detected](./assets/self-check-c-bq40-result-not-detected.png)

## 资产晋升（Asset promotion）

| Asset | Plan source (path) | Used by (runtime/test/docs) | Promote method (copy/derive/export) | Target (project path) | References to update | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| Result dialog PNG set | `docs/specs/5cvrj-bq40-self-check-result-dialogs/assets/*.png` | docs | copy | `firmware/ui/assets/*.png` | `firmware/ui/*.md`, `firmware/README.md` | PR 展示与项目文档共用同一批冻结图 |

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 新增 `BQ40Z50` 三层卡片状态与结果持久化枚举
- [x] M2: 补齐 5 类结果弹窗 renderer 与预览场景
- [x] M3: 激活运行态映射到 `SUCCESS/NO BATTERY/ROM MODE/ABNORMAL/NOT DETECTED`
- [x] M4: 文档与规格资产同步完成
- [ ] M5: 构建、预览验证与快车道 PR 收敛完成

## 方案概述（Approach, high-level）

- 用显式结果枚举替代布尔成功/失败 overlay，避免文案与交互逻辑继续分叉。
- 普通访问仅负责区分“正常 / 异常 / 未识别”；激活结果负责补充更明确的弹窗级结论。
- 最近一次结果作为只读 UI 状态保存在运行态快照中，由前面板统一渲染，不额外引入新页面。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：若运行态无法稳定给出 `ROM MODE` 证据，只能退回 `NOT DETECTED` 或 `ABNORMAL`。
- 需要决策的问题：None。
- 假设（已确认）：`WARN` 就是统一异常态；结果弹窗先固定 5 类，不继续细分。

## 变更记录（Change log）

- 2026-03-11: 完成 BQ40 三层卡片状态、5 类结果弹窗、结果持久化回显与预览 PNG 资产同步；当前阻塞仅剩图片提交前确认与后续 PR 收敛。
