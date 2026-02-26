# 前面板工业仪表风 UI + 1:1 预览（#6qrjs）

## 状态

- Status: 部分完成（6/7）
- Created: 2026-02-26
- Last: 2026-02-26

## 背景 / 问题陈述

- 前面板当前固件已具备 GC9307 屏幕初始化与按键联动能力，但文档口径仍停留在旧的 `Hello World + fps`，与当前实现和目标风格不一致。
- 需要一个可复用的“同源渲染”链路，确保主机预览图与固件上屏逻辑 1:1 一致，避免双实现漂移。
- 需要在 `320x172` 横屏有效区内交付可评审的工业仪表风视觉方案，支持 A/B/C 三种字体风格对比。

## 目标 / 非目标

### Goals

- 固定 `320x172` 横屏有效区，交付工业仪表风英文 UI。
- 新增共享渲染模块，由固件与主机预览工具复用同一渲染代码。
- 交付 3 个视觉变体（A/B/C）与 7 个交互状态帧（idle/up/down/left/right/center/touch）。
- 默认固化 Variant A，保留 B/C 用于视觉对比。
- 建立 `docs/specs` 主规格目录并与实现保持同步。

### Non-goals

- 不接入真实电源遥测数据。
- 不实现菜单路由、触摸手势解析、复杂页面状态机。
- 不变更 GC9307/TCA6408A 硬件初始化时序与连线。
- 不推进远端 `push` 或 PR 创建。

## 范围（Scope）

### In scope

- `firmware/src/front_panel_scene.rs`：共享 UI 渲染与字体风格定义。
- `firmware/src/front_panel.rs`：改造为调用共享渲染模块，保留硬件层逻辑。
- `tools/front-panel-preview`：主机侧 1:1 预览工具（输出 `framebuffer.bin` + `preview.png`）。
- 文档更新：`firmware/README.md`、`docs/specs/README.md`、本规格文档。

### Out of scope

- 触摸芯片协议解析。
- 运行时主题切换功能。
- 真实机台显示亮度主观标定。

## 需求（Requirements）

### MUST

- 渲染逻辑同源：固件与预览必须复用同一 scene renderer。
- 预览输出像素必须是 `320x172`、`RGB565 little-endian`。
- 支持 `UiFocus` 七态与 `touch_irq` 状态展示。
- 提供 A/B/C 三种字体风格并导出视觉对比图。
- 文档不再将前面板 UI 目标描述为 `Hello World + fps`。

### SHOULD

- 保持按键几何布局与现有硬件联调布局一致（不重排触摸区与 D-pad）。
- 预览工具参数明确且可脚本化批量调用。

### COULD

- 后续在不破坏同源渲染前提下增加局部刷新优化。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 固件启动后读入当前按键状态，使用共享 renderer 绘制完整首帧。
- 周期轮询输入，状态变化时重绘界面并更新 focus/highlight。
- 主机工具根据 `--variant` 与 `--focus` 调用同一 renderer，输出 raw framebuffer 与 PNG。

### Edge cases / errors

- 读取按键失败：界面回退到 idle 模型继续运行。
- 字体字形缺失：忽略该字符，不影响渲染流程。
- 预览参数非法：命令行报错并退出，禁止产出不完整文件。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes） |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `render_frame` | Rust API | internal | New | None | firmware | firmware + host preview tool | 同源渲染入口 |
| `UiModel` | Rust type | internal | New | None | firmware | firmware + host preview tool | UI 状态模型 |
| `front-panel-preview CLI` | CLI | internal | New | None | firmware tooling | local developer | `--variant --focus --out-dir` |

### 契约文档（按 Kind 拆分）

None

## 验收标准（Acceptance Criteria）

- Given 固件构建成功且屏幕硬件可访问，When 前面板输入状态变化，Then 屏幕渲染由共享 scene renderer 产出并正确显示对应 focus 高亮。
- Given 主机运行预览工具，When 指定任意 `variant/focus`，Then 产出 `framebuffer.bin`（固定 `110080` bytes）与 `preview.png`（固定 `320x172`）。
- Given 文档更新完成，When 查阅前面板说明，Then 不再以 `Hello World + fps` 作为当前 UI 目标。

## 实现前置条件（Definition of Ready / Preconditions）

- 范围与非目标已冻结。
- `flow_type=normal` 已锁定。
- 字体来源与许可证口径已明确（u8g2 fonts 需注明字体许可差异）。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Firmware build: `cargo build --manifest-path firmware/Cargo.toml --release`
- Preview build/run: `cargo run --manifest-path tools/front-panel-preview/Cargo.toml -- --variant A --focus idle --out-dir <ABS_PATH>`

### Quality checks

- 预览图分辨率检查为 `320x172`。
- raw framebuffer 大小检查为 `320*172*2`。

## 文档更新（Docs to Update）

- `firmware/README.md`：前面板章节更新为工业仪表风 + 同源预览流程。
- `docs/specs/README.md`：新增规格索引行。
- `docs/plan/3kz8p:front-panel-screen-ui/PLAN.md`：标记为重新设计。

## 计划资产（Plan assets）

- Directory: `docs/specs/6qrjs-front-panel-industrial-ui-preview/assets/`
- In-plan references: None

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 建立 `docs/specs` 根目录与新规格文档
- [x] M2: 新增共享 scene renderer 并接入固件
- [x] M3: 集成 A/B/C 三种位图字体风格
- [x] M4: 新增主机预览工具并输出 raw + PNG
- [x] M5: 批量导出 21 张状态预览图
- [x] M6: 更新 README 与旧计划口径漂移
- [ ] M7: 真机主观观感确认（主人评审）

## 方案概述（Approach, high-level）

- 使用 `UiPainter` 抽象屏幕绘制能力，保证固件硬件写屏与主机 framebuffer 共用一套渲染逻辑。
- 使用 u8g2 字体渲染器输出英文标签，按变体切换字体组合，不在 Python 中重写渲染逻辑。
- 通过工具链批量导出状态图，先看效果再决定后续迭代。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：逐像素字体渲染在 MCU 侧吞吐较低，后续可能需要局部刷新优化。
- 需要决策的问题：是否将 Variant A 之外的样式保留为运行时可切换功能。
- 假设（需主人确认）：当前阶段默认 Variant A 即可满足第一轮上板验证。

## 变更记录（Change log）

- 2026-02-26: 新建规格并完成第一轮实现同步（共享渲染 + 预览工具 + 文档更新）。

## 参考（References）

- `firmware/src/front_panel.rs`
- `firmware/src/front_panel_scene.rs`
- `tools/front-panel-preview/src/main.rs`
- [u8g2-fonts docs](https://docs.rs/u8g2-fonts/latest/u8g2_fonts/fonts/index.html)
- [u8g2 license](https://raw.githubusercontent.com/olikraus/u8g2/master/LICENSE)
