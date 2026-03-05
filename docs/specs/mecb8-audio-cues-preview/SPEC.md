# 状态/告警/错误提示音（15 组）本地预览资产（#mecb8）

## 状态

- Status: 已完成
- Created: 2026-03-05
- Last: 2026-03-05

## 背景 / 问题陈述

- 项目已有 TDM + MAX98357A + Speaker 的音频链路设计，但缺少面向“状态/告警/错误”的系统化试听资产。
- 当前仅有 demo playlist，不能覆盖运行时关键状态与故障语义，无法快速做音效辨识评审。
- 需要一套可重复生成、可本地试听、可后续固件接入复用的数据形态。

## 目标 / 非目标

### Goals

- 生成 15 组提示音（状态 5、告警 4、错误 6）。
- 为每组音效提供 `score + mid + wav`。
- 提供可直接打开的增强预览页（分组、循环模拟、调参、停止）。
- 固化音效清单契约，包含触发语义与循环模式。

### Non-goals

- 本轮不接入固件运行时资产目录。
- 本轮不实现运行时音效优先级仲裁与抢占。
- 本轮不做硬件声学标定。

## 范围（Scope）

### In scope

- `docs/audio-cues-preview/` 目录内的全部试听资产。
- 批量生成脚本与产物一致性校验。
- 文档入口链接更新（`docs/README.md`、`docs/audio-design.md`）。

### Out of scope

- 固件播放实现改动。
- 输入/输出过压/过流/过功率按输入/输出维度拆分。

## 需求（Requirements）

### MUST

- 资产数量固定：`status=5`、`warning=4`、`error=6`。
- 命名固定：英文 ID 文件名 + 中文标题展示。
- 告警默认间隔循环：`2000ms`。
- 错误默认连续循环。
- `cues.manifest.json` 字段必须含：
  - `version`
  - `profile`
  - `generated_at`
  - `warning_interval_ms_default`
  - `items[]`（含 `id/title_zh/category/trigger_condition_zh/loop_mode/loop_interval_ms/score_path/wav_path/mid_path/duration_ms`）

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 运行生成脚本后自动输出 15 组 `scores/*.json` 与 `audio/*.mid|*.wav`。
- 预览页按状态/告警/错误分组展示所有音效条目。
- 支持单次播放、循环播放、停止单条、停止全部。
- 支持全局音量调整与告警间隔覆盖。

### Edge cases / errors

- 若 manifest 缺失或加载失败，页面显示错误提示。
- 若音频播放失败，自动停止该条循环状态。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes） |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 提示音预览清单 manifest | File format | internal | New | `docs/audio-cues-preview/cues.manifest.json` | firmware/audio | preview page / 后续固件接入 | 仅试听资产来源 |

### 契约文档（按 Kind 拆分）

- `docs/audio-cues-preview/cues.manifest.json`

## 验收标准（Acceptance Criteria）

- Given 执行 `python3 tools/audio/gen_status_alert_error_previews.py`，
  When 生成完成，
  Then `docs/audio-cues-preview/scores`、`docs/audio-cues-preview/audio`、`docs/audio-cues-preview/cues.manifest.json` 均存在且数量一致。

- Given 启动 `python3 -m http.server -d docs 8000`，
  When 打开 `audio-cues-preview/preview.html`，
  Then 可按分组播放全部音效，告警按间隔循环，错误可连续循环且可一键停止。

- Given 查看文档索引，
  When 打开 `docs/README.md` 与 `docs/audio-design.md`，
  Then 都可定位到本地试听资产入口，且声明“非固件接入资产”。

## 实现前置条件（Definition of Ready / Preconditions）

- 15 组音效列表与触发语义已冻结。
- 循环策略已冻结：告警 `2000ms` 间隔，错误连续。
- 命名策略已冻结：英文 ID + 中文标题显示。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- 脚本生成后做数量一致性校验（`score/wav/mid` 各 15）。
- JSON 文件结构可解析。

### Quality checks

- Python 语法检查：`python3 -m py_compile tools/audio/gen_status_alert_error_previews.py`。

## 文档更新（Docs to Update）

- `docs/README.md`: 增加试听资产入口。
- `docs/audio-design.md`: 增加试听资产与预览说明。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 创建提示音清单契约与 15 组 score 文件。
- [x] M2: 生成 15 组 MIDI/WAV 并输出生成校验报告。
- [x] M3: 完成增强预览页面（分组/循环/调参/停止）。
- [x] M4: 完成文档入口更新与本地验证记录。

## 方案概述（Approach, high-level）

- 使用统一的 Python 生成脚本维护提示音定义与产物输出。
- 使用 `buzzer-audio-preview` 工具生成 `mid + wav`，并通过 manifest 驱动预览页面。
- 页面通过 `Audio` API 统一实现三类循环语义。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：本地试听听感与实机喇叭声学特性存在偏差。
- 开放问题：无。
- 假设：后续固件接入阶段可直接消费 manifest 的 ID 与路径语义。

## 变更记录（Change log）

- 2026-03-05: 初始化规格，冻结范围/验收/里程碑。
- 2026-03-05: 完成 15 组试听资产生成、增强预览页、文档入口更新与本地校验。
- 2026-03-05: 依据 review 修复预览页音频 URL 解析与生成工具默认路径可复现性问题。
- 2026-03-05: 依据后续 review 加固生成器失败回滚与 JSON 输出契约，并补齐 MIDI/tempo/互斥输入校验边界。

## 参考（References）

- `docs/audio-design.md`
