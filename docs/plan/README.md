# 计划（Plan）总览

本目录用于管理“先计划、后实现”的工作项：每个计划在这里冻结范围与验收标准，进入实现前先把口径对齐，避免边做边改导致失控。

## 快速新增一个计划

1. 生成一个新的计划 `ID`（推荐 5 个字符的 nanoId 风格；也兼容历史四位数字 `0001`–`9999`）。
2. 新建目录：`docs/plan/<id>:<title>/`（`<title>` 用简短 slug，建议 kebab-case）。
3. 在该目录下创建 `PLAN.md`（模板见下方“PLAN.md 写法（简要）”）。
4. 在下方 Index 表新增一行，并把 `Status` 设为 `待设计` 或 `待实现`（取决于是否已冻结验收标准），并填入 `Last`（通常为当天）。

## 目录与命名规则

- 每个计划一个目录：`docs/plan/<id>:<title>/`
- `<id>`：推荐 5 个字符的 nanoId 风格；兼容历史四位数字（`0001`–`9999`）并允许共存。
  - 推荐字符集（小写 + 避免易混淆字符）：`23456789abcdefghjkmnpqrstuvwxyz`
  - 正则：`[23456789abcdefghjkmnpqrstuvwxyz]{5}`
- `<title>`：短标题 slug（建议 kebab-case，避免空格与特殊字符）；目录名尽量稳定。
- 人类可读标题写在 Index 的 `Title` 列；标题变更优先改 `Title`，不强制改目录名。
- 兼容性提示：目录名包含 `:`，在 Windows 默认文件系统/工具链下可能无法正常 checkout；本仓库计划文档工作流默认以 macOS/Linux 为主。

## 状态（Status）说明

仅允许使用以下状态值：

- `待设计`：范围/约束/验收标准尚未冻结，仍在补齐信息与决策。
- `待实现`：计划已冻结，允许进入实现阶段（或进入 PM/DEV 交付流程）。
- `部分完成（x/y）`：实现进行中；`y` 为该计划里定义的“实现里程碑”数，`x` 为已完成“实现里程碑”数（见该计划 `PLAN.md` 的 Milestones；不要把计划阶段产出算进里程碑）。
- `已完成`：该计划已完成（实现已落地或将随某个 PR 落地）；如需关联 PR 号，写在 Index 的 `Notes`（例如 `PR #123`）。
- `作废`：不再推进（取消/价值不足/外部条件变化）。
- `重新设计（#<id>）`：该计划被另一个计划取代；`#<id>` 指向新的计划编号。

## `Last` 字段约定（推进时间）

- `Last` 表示该计划**上一次“推进进度/口径”**的日期，用于快速发现长期未推进的计划。
- 仅在以下情况更新 `Last`（不要因为改措辞/排版就更新）：
  - `Status` 变化（例如 `待设计` → `待实现`，或 `部分完成（x/y）` → `已完成`）
  - `Notes` 中写入/更新 PR 号（例如 `PR #123`）
  - `PLAN.md` 的里程碑勾选变化
  - 范围/验收标准冻结或发生实质变更

## PLAN.md 写法（简要）

每个计划的 `PLAN.md` 至少应包含：

- 背景/问题陈述（为什么要做）
- 目标 / 非目标（做什么、不做什么）
- 范围（in/out）
- 需求列表（MUST）
- 验收标准（Given/When/Then + 边界/异常）
- 实现前置条件（Definition of Ready / Preconditions；未满足则保持 `待设计`）
- 非功能性验收/质量门槛（测试策略、质量检查、Storybook/视觉回归等按仓库已有约定）
- 文档更新（需要同步更新的项目设计文档/架构说明/README/ADR）
- 实现里程碑（Milestones，用于驱动 `部分完成（x/y）`；只写实现交付物，不要包含计划阶段产出）
- 风险与开放问题（需要决策的点）
- 假设（需主人确认）

## Index（固定表格）

| ID   | Title | Status | Plan | Last | Notes |
|-----:|-------|--------|------|------|-------|
| 0001 | 初始化 ESP32-S3（esp-rs/esp-hal）no_std 固件工程 | 已完成 | `0001:esp-rs-no-std-firmware-bootstrap/PLAN.md` | 2026-01-22 | PR #4 |
| 0002 | 仓库代码质量门槛：Git hooks + GitHub Actions | 已完成 | `0002:quality-gates-ci-hooks/PLAN.md` | 2026-01-22 | - |
| 0003 | 设备操作纪律防护：Agent 设备闸门 | 已完成 | `0003:device-operation-guardrails/PLAN.md` | 2026-01-24 | PR #7 |
| 0004 | 固件音频播放 + Demo 素材（6 段；~65s；mono） | 已完成 | `0004:firmware-audio-playback-demo/PLAN.md` | 2026-01-23 | 决策收敛：PCM-only（`WAV(PCM16LE)`）；已复核端到端 6 段均播放完成（无 `Late`） |
| 0005 | TPS55288 双路输出控制（默认启用一路：5V/1A；含 INA3221 遥测） | 已完成 | `0005:tps55288-control/PLAN.md` | 2026-01-26 | - |
| 0006 | TPS 热点温度采样：TMP112A 读数并入 telemetry | 已完成 | `0006:tps-tmp112-temperature-reading/PLAN.md` | 2026-01-27 | 兼容：只追加字段，不改变 `#0005` 既有字段 |
| 0007 | INA3221 VBUS 读数偏高排查 | 待设计 | `0007:ina3221-vbus-offset/PLAN.md` | 2026-01-26 | - |
| v5hze | TMP112A 过温告警输出：Comparator 模式保持输出（ALERT→THERM_KILL_N） | 待实现 | `v5hze:tps-tmp112-alert-overtemp-hold/PLAN.md` | 2026-01-27 | - |
| b3qzy | BQ25792 充电功能 bring-up：状态遥测 + 使能策略 | 待实现 | `b3qzy:bq25792-charging-enable/PLAN.md` | 2026-02-06 | - |
