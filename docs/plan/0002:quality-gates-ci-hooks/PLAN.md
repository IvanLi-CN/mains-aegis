# 仓库代码质量门槛：Git hooks + GitHub Actions（#0002）

## 状态

- Status: 待实现
- Created: 2026-01-22
- Last: 2026-01-22

## 背景 / 问题陈述

- 固件工程初始化过程中，缺少一致的本地质量门槛与 CI 校验时，容易出现“能跑但质量退化、提交口径不一、PR 合入后才发现问题”的返工。
- 需要将本地 git hooks 与 GitHub Actions 工作流固化为可复用模板，并对齐到既有工程实践（`isolapurr-usb-hub`）。

## 目标 / 非目标

### Goals

- 在仓库内落地一致的质量门槛（local + CI），覆盖：格式化、构建校验、提交信息/PR 标题规范、依赖变更审阅。
- 统一工具链口径：使用 `bun`（不引入 `npm` / `node` 作为开发依赖入口），并复用参考工程的 commitlint 规则（英文口径）。
- 对 docs-only 变更进行 CI 触发收敛：对 `docs/**`（以及 `README.md`）做 `paths-ignore`，避免无意义占用 CI 资源。

### Non-goals

- 不在本计划内修改任何固件业务逻辑或硬件相关 bring-up 流程。
- 不在本计划内引入发布/制品/签名/OTA 流水线。
- 不在本计划内新增全套测试体系；仅冻结质量门槛的最低闭环（format/lint/build 等）。

## 范围（Scope）

### In scope

- Git hooks（使用 `lefthook` 管理）：
  - `pre-commit`: 对 staged Rust 文件执行格式化（允许自动修复并 re-stage）
  - `commit-msg`: 执行 commit message lint（Conventional Commits + 英文口径）
  - `pre-push`: 在推送前执行质量检查（建议包含 fmt/clippy/build；具体命令见契约）
- GitHub Actions（CI）：
  - Rust fmt check
  - Firmware build check（路径过滤到固件相关变更）
  - PR title lint（语义化 + 英文口径）
  - Dependency review（依赖变更审阅）
- 文档更新：补充“质量门槛/CI”入口与排错指引链接。

### Out of scope

- 任何与硬件连接/烧录/运行相关的自动化（硬件依赖步骤仍为手工验证或后续计划）。
- 将质量门槛扩展到多语言/多子项目（除固件工程外的其它栈若后续出现，另开计划冻结）。

## 需求（Requirements）

### MUST

- 不使用 `npm` / `node` 作为开发依赖入口；commitlint 与相关 JS 工具以 `bun` 作为运行时与包管理方式。
- 复用参考工程的 commitlint 规则：
  - Conventional Commits
  - Subject/Body 禁止中文字符（CJK）
  - Subject 禁止首字母大写
- 在仓库根目录落地最小的 JS dev tools 形状（`package.json` + `bun.lock`），并确保 `bun install --frozen-lockfile` 可复现通过。
- Git hooks 覆盖：`pre-commit`、`commit-msg`、`pre-push`。
- GitHub Actions 覆盖：fmt / firmware build / PR title lint / dependency review。
- `docs/**`（以及 `README.md`）变更不触发“构建类”工作流（通过 `paths-ignore` 收敛）。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes） |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Git hooks 配置（`lefthook.yml`） | Config | internal | New | ./contracts/file-formats.md | repo | developers | 固化 `pre-commit/commit-msg/pre-push` |
| Commit message 规则（`commitlint.config.cjs`） | Config | internal | New | ./contracts/file-formats.md | repo | developers | 与 PR title lint 对齐 |
| GitHub Actions workflows（`.github/workflows/*.yml`） | Config | internal | New | ./contracts/file-formats.md | repo | developers | fmt/build/title/deps |
| 本地质量命令口径（lefthook/commitlint） | CLI | internal | New | ./contracts/cli.md | repo | developers | 供手动运行与排错 |

### 契约文档（按 Kind 拆分）

- [contracts/README.md](./contracts/README.md)
- [contracts/file-formats.md](./contracts/file-formats.md)
- [contracts/cli.md](./contracts/cli.md)

## 验收标准（Acceptance Criteria）

- Given 一个干净的开发环境（未安装依赖），
  When 在仓库根目录执行 `bun install --frozen-lockfile`，
  Then 依赖安装可复现通过，且不会引入需要提交的运行态产物（如 `node_modules/`）。

- Given 开发者已按文档安装 `bun` 与 `lefthook`，
  When 在仓库根目录执行 `lefthook install` 并进行一次提交，
  Then `pre-commit` 与 `commit-msg` hooks 会被执行，且失败时能阻止提交并输出可操作的错误信息。

- Given staged 区包含 Rust 文件变更，
  When 提交前触发 `pre-commit`，
  Then 仅对 staged Rust 文件执行格式化修复并自动 re-stage，不应修改未暂存文件内容。

- Given commit message 或 PR title 不满足英文口径（例如包含中文字符或 Subject 首字母大写），
  When 对应的 `commit-msg` hook 或 GitHub Actions PR title lint 执行，
  Then 必须失败并给出明确的错误提示。

- Given 一个仅修改 `docs/**`（或 `README.md`）的 PR，
  When GitHub Actions 触发，
  Then `ci.yml` / `firmware.yml` / `dependency-review.yml` 不应运行（由 `paths-ignore` 收敛），但 PR title lint 仍可运行。

- Given 一个包含固件相关变更的 PR，
  When GitHub Actions 触发，
  Then 至少满足：Rust fmt check 通过、Firmware build check 通过、dependency review 通过、PR title lint 通过。

## 实现前置条件（Definition of Ready / Preconditions）

- 目标/非目标、范围（in/out）、约束已明确
- 验收标准覆盖 core path + 关键边界/异常
- 接口契约已定稿：`./contracts/*.md` 中的关键信息可直接驱动实现与验证
- 已完成必要的 repo reconnaissance（本计划已冻结目标文件路径与工作流职责分工）

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: 本计划不新增；以“门槛自动化”闭环为主。
- Integration tests: 以 CI 运行结果作为门槛验证；不引入新的测试框架。

### Quality checks

- Formatting: Rust fmt（local + CI）
- Lint: clippy（至少在 `pre-push`；是否进入 CI 作为强制门槛由实现阶段按契约执行）
- Build: firmware build（CI 强制）
- Commit/PR hygiene: commitlint + semantic PR title（强制）
- Security: dependency review（强制）

## 文档更新（Docs to Update）

- `docs/README.md`: 增加“代码质量 / CI”入口（指向实现阶段新增的质量门槛说明）

## 实现里程碑（Milestones）

- [ ] M1: 落地 `bun` + `commitlint`（含英文口径规则）与 `lefthook.yml`（pre-commit/commit-msg/pre-push）
- [ ] M2: 落地 GitHub Actions workflows（fmt / firmware build / PR title lint / dependency review）并加上 `paths-ignore`（`docs/**` + `README.md`）
- [ ] M3: 更新 `docs/README.md`，补充质量门槛入口与常见排错指引

## 方案概述（Approach, high-level）

- 以参考工程为模板复用配置形状与规则，减少“自定义过度”带来的维护成本。
- 本地 hooks 以“尽量快 + 可预测”为原则；较重的构建检查主要放在 CI，`pre-push` 用于提前发现明显问题。
- 对 docs-only 变更进行触发收敛，避免 CI 资源浪费。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：
  - `bun` 生态与版本升级可能引入行为差异，需要在实现阶段固定版本并确保可复现安装。
  - `pre-push` 若过重会影响开发体验，需要在“强制门槛”与“开发效率”之间平衡。
- 假设（已冻结）：
  - 参考工程 `isolapurr-usb-hub` 的质量门槛口径可直接复用到本仓库。
  - 固件工程目录将与 `#0001` 的实现结果保持一致（例如存在 `firmware/`），CI 的 path-filter 将以此为准。

## 变更记录（Change log）

- 2026-01-22: 新建计划，冻结 git hooks + GitHub Actions 质量门槛口径

## 参考（References）

- 参考工程：`isolapurr-usb-hub`（内部参考仓库；不在文档中固化主机路径）
