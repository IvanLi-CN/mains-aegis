# 前面板 UI 视觉规范系统化（#hg3dw）

## 状态

- Status: 已完成
- Created: 2026-03-02
- Last: 2026-03-02

## 背景 / 问题陈述

- 现有 `firmware/ui` 文档覆盖了 Dashboard 与 Self-check 页面布局，但视觉语言分散在多个文档中，缺少统一、可执行的规范入口。
- 缺少显式 Token 与组件边界约束，导致术语、状态文案、组件职责容易在后续迭代中漂移。
- 缺少视觉回归清单，评审时难以快速判断“规范是否被落实”。

## 目标 / 非目标

### Goals

- 建立前面板 UI 的单一视觉规范来源（SoT），覆盖 Token、状态语义、命名规则与约束。
- 建立组件契约文档，明确核心组件职责、必填字段、禁用字段与几何锚点来源。
- 建立视觉回归清单，将规范条款与现有冻结资产一一绑定。
- 更新索引文档，使 `docs/README.md` 与 `firmware/ui/README.md` 可快速到达规范入口。

### Non-goals

- 不做功能性重设计（不改业务状态机、不改页面结构与交互行为）。
- 不新增页面、不替换现有冻结图资产。
- 不变更分辨率、字体资源与硬件初始化流程。

## 范围（Scope）

### In scope

- 新增 `firmware/ui/design-language.md`。
- 新增 `firmware/ui/component-contracts.md`。
- 新增 `firmware/ui/visual-regression-checklist.md`。
- 更新 `firmware/ui/README.md`、`firmware/ui/dashboard-design.md`、`firmware/ui/self-check-design.md`。
- 更新 `docs/README.md` 与 `docs/specs/README.md` 索引。
- 在不改变布局与业务语义前提下，允许对 `firmware/src/front_panel_scene.rs` 做最小字体门禁对齐（移除 `<10px` 字形、Compact 角色复用白名单字体）。

### Out of scope

- `firmware/src/front_panel.rs`、`firmware/src/main.rs`。
- `firmware/src/front_panel_scene.rs` 的业务逻辑改动（状态机、页面结构、交互流程）。
- `firmware/ui/assets/*` 的新增与替换。

## 需求（Requirements）

### MUST

- 规范文档必须覆盖 `Color/Type/Space/Stroke/State` 五类 Token。
- Typography 必须绑定到 bitmap 字体白名单，并给出 Token->字体->字高映射。
- 必须定义字体字高白名单（用于后续新增字体准入），不允许未审批字高进入实现。
- 组件契约必须覆盖 `TopBar`、`KpiPanel`、`InfoPanel`、`BatteryCard`、`ChargeCard`、`DischgCard`、`DiagCard`、`ActivationDialog`。
- 状态语义必须统一 `UpsMode`、`SelfCheckCommState`、`BmsActivationState`、`UiFocus`、`touch_irq`。
- 业务口径固定：`ChargeCard` 仅 `STANDBY` 可充电，其他模式显示 `LOCK/NOAC`。
- 视觉回归清单中的视觉规则必须绑定 `firmware/ui/assets` 资产；全局离线/入口/白名单检查允许使用命令或 targets 验证。

### SHOULD

- 文档采用中文说明 + 英文 Token/组件名，便于评审与代码映射。
- 所有规范引用路径保持离线可读，不依赖外链图片。

### COULD

- 后续可在不改变 Token 命名的前提下扩展 Host-side UI 预留章节。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 评审者从 `firmware/ui/README.md` 进入，优先阅读 `design-language.md` 与 `component-contracts.md`。
- 实现者依据 Token 与组件契约检查页面文档，避免重复定义与冲突表述。
- 评审阶段按 `visual-regression-checklist.md` 对照资产逐条验收。

### Edge cases / errors

- 若页面文档与视觉规范冲突，以 `design-language.md` 为准并在页面文档回链。
- 若视觉规则无法映射到资产，规则必须标记为待补充，不得视为通过；全局检查项按命令/targets 验收。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes） |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Design Language Contract | Doc contract | internal | New | `../../../firmware/ui/design-language.md` | firmware-ui | firmware/hardware reviewers | Token + 状态语义 |
| Component Contract | Doc contract | internal | New | `../../../firmware/ui/component-contracts.md` | firmware-ui | firmware implementers | 组件职责边界 |
| Visual Regression Contract | Doc checklist | internal | New | `../../../firmware/ui/visual-regression-checklist.md` | firmware-ui | reviewers | 规则到资产映射 |

### 契约文档（按 Kind 拆分）

None

## 验收标准（Acceptance Criteria）

- Given 新增视觉规范文档，When 审核 Token 章节，Then `Color/Type/Space/Stroke/State` 五类完整且每项具备用途与禁止项。
- Given 新增组件契约文档，When 检查 `ChargeCard` 与 `DischgCard`，Then 字段边界明确且无语义混用。
- Given 页面设计文档，When 查阅样式口径，Then 均引用统一视觉规范来源而非重复定义。
- Given 新增回归清单，When 按规则核对资产，Then 每条规则均能定位到对应文件与通过条件。
- Given 全部 UI 文档，When 扫描图片链接，Then 不存在 `http/https` 外链图片。
- Given Typography 规范，When 审核字体章节，Then 每个 `Type` Token 都可追溯到唯一 bitmap 字体与明确字高。
- Given 后续新增字体需求，When 对照规范，Then 仅允许白名单字高 `13/14/22`（且不得小于 `10px`），否则必须先更新文档契约再进入实现。

## 实现前置条件（Definition of Ready / Preconditions）

- 目标、范围、非目标已冻结。
- 文档型交付边界已确认（文档为主，允许最小字体门禁对齐代码改动）。
- 现有冻结资产目录可用：`firmware/ui/assets/`。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- 文档链接与引用检查：`rg -n "\]\((https?://|\.\./\.\./\.\./)" firmware/ui docs/README.md docs/specs/README.md docs/specs/hg3dw-front-panel-visual-language/SPEC.md`
- 外链图片扫描：`rg -n '![^\n]*\(https?://' firmware/ui docs`
- 状态术语一致性检查：`rg -n "LOCK|NOAC|STANDBY|BYPASS|ASSIST|BACKUP|PEND|WARN|ERR|N/A" firmware/ui`
- 字体白名单检查：`rg -n "static FONT_|u8g2_font_|Type.NumCompact|13px|14px|22px|10px" firmware/src/front_panel_scene.rs firmware/ui/design-language.md firmware/ui/component-contracts.md firmware/ui/visual-regression-checklist.md`

### Quality checks

- 所有新增文档必须可离线阅读。
- 术语与状态词不得出现未定义别名。

## 文档更新（Docs to Update）

- `firmware/ui/README.md`: 增加视觉规范入口与阅读顺序。
- `firmware/ui/dashboard-design.md`: 引用视觉规范与组件契约，移除重复样式定义口径。
- `firmware/ui/self-check-design.md`: 引用视觉规范与组件契约，移除重复样式定义口径。
- `docs/README.md`: 在 UI docs 区域增加规范文档入口。
- `docs/specs/README.md`: 增加 #hg3dw 索引行。

## 计划资产（Plan assets）

- Directory: `docs/specs/hg3dw-front-panel-visual-language/assets/`
- In-plan references:
  - `assets/color-preview.svg`
  - `assets/typography-preview.svg`

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 新增视觉规范主文档并定义五类 Token。
- [x] M2: 新增组件契约文档并覆盖核心组件。
- [x] M3: 新增视觉回归清单并绑定冻结资产。
- [x] M4: 更新页面文档与索引入口，完成规范回链。
- [x] M5: 通过文档一致性与外链检查。
- [x] M6: 固化 bitmap 字体白名单与字高白名单，并补充配色/字体预览图。

## 方案概述（Approach, high-level）

- 以 `design-language.md` 作为视觉口径单一来源，页面文档仅保留布局与业务说明。
- 以 `component-contracts.md` 约束组件职责和字段边界，防止语义漂移。
- 以 `visual-regression-checklist.md` 实现“规则可验证”，确保评审可复核。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：后续若新增页面但未复用该规范，可能再次出现分散定义。
- 需要决策的问题：Host-side UI 是否沿用同一 Token 命名体系。
- 假设（需主人确认）：当前冻结资产可持续作为前面板视觉验收基线。

## 变更记录（Change log）

- 2026-03-02: 新建规格并完成设计语言、组件契约、回归清单及索引同步。
- 2026-03-02: 增补 bitmap 字体白名单、字高白名单与配色/字体预览图资产。
- 2026-03-02: 收敛字体策略为 `>=10px`，移除 `8px` 字形白名单并同步代码绑定。
- 2026-03-02: 对齐规格边界，明确“文档为主 + 最小字体门禁代码对齐”与“全局检查项可用命令验收”。

## 参考（References）

- `firmware/ui/README.md`
- `firmware/ui/dashboard-design.md`
- `firmware/ui/self-check-design.md`
- `docs/specs/6qrjs-front-panel-industrial-ui-preview/SPEC.md`
- `docs/specs/7n4qd-mcu-self-check-live-panel/SPEC.md`
