# MCU 自检页实时化与常驻显示（#7n4qd）

## 状态

- Status: 已完成
- Created: 2026-03-01
- Last: 2026-03-05

## 背景 / 问题陈述

- 既有 `Variant C` 自检页主要由演示模型驱动，关键参数并非真实硬件采样。
- 页面默认从 Dashboard 进入，自检完成后不会强制停留在自检页。
- 新需求要求：开机自检期间显示完整自检页、自检后保持该页、并持续展示真实数据。

## 目标 / 非目标

### Goals

- 开机后在屏幕可用时立即显示 `Variant C` 自检页。
- 自检阶段按模块探测进度逐步更新卡片状态（`PEND -> OK/WARN/ERR/N/A`）。
- 自检结束后保持自检页并持续刷新真实运行数据。
- 禁用 `CENTER` 长按切页，避免误切回 Dashboard。
- 保持 10 模块双列诊断卡布局不变，仅替换为真实数据源。
- 对齐 BMS 运行语义：当 BQ40 可通信但放电通路未就绪时，页面显示 `WARN` 而非误判为 `OK`。

### Non-goals

- 不新增 FUSB302/GC9307 深度寄存器级驱动。
- 不改 Dashboard（Variant B）视觉布局。
- 不改既有自检门控顺序与 emergency-stop 策略。

## 范围（Scope）

### In scope

- `firmware/src/output/mod.rs`
  - 新增 `SelfCheckStage`、自检进度上报接口 `boot_self_test_with_report`。
  - `BootSelfTestResult` 补充 `self_check_snapshot`。
  - 自检门控与运行态补充 BMS 放电就绪判定（`XDSG/DSG`）与恢复路径（解除 BMS 门控后恢复 TPS 通道）。
  - `PowerManager` 提供 `ui_snapshot()`，持续输出运行期真实快照。
- `firmware/src/output/tps55288.rs`
  - `print_telemetry_line` 返回结构化采样结果，供 UI 快照聚合。
- `firmware/src/front_panel.rs`
  - 新增 `update_self_check_snapshot`；渲染触发由“输入变化或快照变化”驱动。
  - 默认固定 `Variant C`，禁用页面切换逻辑。
- `firmware/src/front_panel_scene.rs`
  - 新增 `SelfCheckUiSnapshot`/`SelfCheckCommState`。
  - `render_frame_with_self_check` 支持真实快照渲染，`Variant C` 改为真实数据分支。
- `firmware/src/main.rs`
  - 调整启动顺序：面板初始化后先显示自检页，再执行带回调的自检。
  - 运行期将 `PowerManager::ui_snapshot()` 持续喂给前面板。

### Out of scope

- 新增菜单路由、触摸手势页面导航。
- 新增云端/远程遥测协议。

## 接口变更（Interfaces）

- `output::boot_self_test_with_report(..., reporter)`：新增，支持自检阶段 UI 回调。
- `BootSelfTestResult::self_check_snapshot`：新增。
- `output::Config::self_check_snapshot`：新增。
- `PowerManager::ui_snapshot()`：新增只读接口。
- `front_panel_scene::render_frame_with_self_check(...)`：新增。

## 验收标准（Acceptance Criteria）

- 上电后（屏幕链路可用）首屏为 `SELF CHECK`，而非 Dashboard。
- 自检过程中 10 张模块卡片从 `PEND` 逐步切换到真实状态；缺失模块显示 `ERR/N/A`。
- 自检结束后页面保持在 `SELF CHECK`，关键参数（电流/温度/SOC/充电状态）可持续变化。
- BQ40 可通信但放电未就绪时，`BQ40Z50` 诊断卡显示 `WARN`，并保持 TPS 输出门控；放电就绪后自动解除该门控。
- 长按 `CENTER` 不再切页，日志不再出现 `ui: page switch ...`。
- 构建验证：
  - `cargo build --release`（`firmware/`）通过。
  - `cargo run --manifest-path tools/front-panel-preview/Cargo.toml -- --variant C --mode standby --focus idle --out-dir <ABS_PATH>` 通过。
  - 预览产物 `preview.png` 分辨率为 `320x172`，`framebuffer.bin` 大小为 `110080` bytes。

## 里程碑（Milestones）

- [x] M1: 自检快照模型与进度回调落地。
- [x] M2: `Variant C` 渲染切换到真实数据分支。
- [x] M3: 启动顺序调整为“先显示自检页，再执行自检”。
- [x] M4: 自检完成后常驻显示 + 运行期实时刷新。
- [x] M5: 文档与构建验证同步完成。

## 关联规格

- `docs/specs/6qrjs-front-panel-industrial-ui-preview/SPEC.md`：视觉与布局基线来源。

## 变更记录（Change log）

- 2026-03-05: review-loop 收敛补丁：当 `OPERATION_STATUS` 读取失败（`discharge_ready=None`）时，BQ40 卡片改为 `WARN` 且允许触发激活；BMS 恢复放行 TPS 时同步触发 `INA3221` 重试，避免长期停留 `ina_uninit`。
- 2026-03-05: 修正 BMS 激活闭环细节：`OPERATION_STATUS` 读取失败不再放行 TPS；激活请求会清理 `bms/chg` 重试退避窗口；`BmsActivateConfirm` 弹窗收起条件与激活触发条件统一。
- 2026-03-02: 对齐 BMS 放电就绪语义：`XDSG=0 && DSG=0` 归类为 `WARN`；激活成功判定增加放电就绪与 `VBAT_PRESENT`，并补充 BMS 恢复后的 TPS 门控自动解除路径。
