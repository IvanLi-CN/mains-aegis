# 安装 UI UX Pro Max Skill（#4t9wx）

## 状态

- Status: 已完成
- Created: 2026-03-02
- Last: 2026-03-02

## 背景 / 问题陈述

- 项目当前未内置可复用的 UI/UX 设计指导 skill，前端/可视化任务缺少统一检索与建议入口。
- `UI UX Pro Max` 官方文档给出 Codex 安装方式，但默认示例脚本路径基于 `.claude` 目录，不完全适配本仓库的 `.codex` 目录结构。
- 需要将 skill 作为项目资产纳入版本管理，保证团队协作与复现能力。

## 目标 / 非目标

### Goals

- 在仓库内安装 `ui-ux-pro-max` 到 `.codex/skills/ui-ux-pro-max`。
- 固定安装来源版本为 `uipro-cli@2.2.1`（对应 release `v2.2.1`）。
- 修正 `SKILL.md` 中脚本路径，统一改为 `.codex` 前缀可直接执行。
- 清理并忽略 `scripts/__pycache__/`，避免脏产物入库。
- 将上述变更纳入 Git、提交并进入 PR/checks 流程。

### Non-goals

- 不修改业务代码、固件逻辑或外部接口。
- 不引入全局（`$CODEX_HOME/skills`）安装。
- 不调整仓库现有 CI 触发规则。

## 范围（Scope）

### In scope

- `.codex/skills/ui-ux-pro-max/` 全量 skill 文件（`SKILL.md`、`data/`、`scripts/`）。
- `.gitignore`：新增 pycache 忽略规则。
- `docs/specs/4t9wx-install-ui-ux-pro-max-skill/SPEC.md`：记录规格与验收。
- `docs/specs/README.md`：登记索引。

### Out of scope

- 任何 npm 依赖锁文件变更。
- 任何远端发布/合并后清理动作。

## 接口变更（Interfaces）

- 无业务 API/类型变更。
- 新增项目内 skill 入口：`.codex/skills/ui-ux-pro-max/SKILL.md`。

## 验收标准（Acceptance Criteria）

- `.codex/skills/ui-ux-pro-max/` 下存在 `SKILL.md`、`data/*.csv`、`scripts/*.py`。
- 从仓库根执行以下命令返回 0 且有结果输出：
  - `python3 .codex/skills/ui-ux-pro-max/scripts/search.py "saas" --domain style -n 1`
- `SKILL.md` 中不再出现 `python3 skills/ui-ux-pro-max/scripts/search.py`。
- `scripts/__pycache__/` 不存在，且 `.gitignore` 包含 `/.codex/skills/ui-ux-pro-max/scripts/__pycache__/`。
- 变更仅包含 spec、gitignore 与 `.codex/skills/ui-ux-pro-max` 安装资产。

## 里程碑（Milestones）

- [x] M1: 创建 topic 分支并完成 spec 建档。
- [x] M2: 执行 `uipro-cli@2.2.1` 安装到 `.codex/skills`。
- [x] M3: 完成 Codex 路径兼容修正与 pycache 清理。
- [x] M4: 通过本地 smoke 验证并提交。
- [x] M5: PR checks/review-loop 收敛并状态明确。

## 关联规格

- `docs/specs/README.md`
