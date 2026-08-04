# 文件与目录形状（File formats）

将配置文件与工作流文件视为一种接口契约：用于冻结路径、职责与最小字段集合，避免后续实现“各自为政”或反复改口径。

## Git hooks（`lefthook.yml`）

- 范围（Scope）: internal
- 变更（Change）: New
- 位置（Path）: 仓库根目录 `lefthook.yml`
- 格式（Encoding）: YAML（utf-8）

Schema（结构）：

- 必须包含 hooks：`pre-commit`、`commit-msg`、`pre-push`
- `pre-commit`：
  - 只作用于 staged files（通过 `glob` 与“format staged + stage_fixed”实现）
  - 允许自动修复并 re-stage
- `commit-msg`：
  - 必须对 commit message 执行 lint（调用 `commitlint -- --edit {1}` 或等价方式）
- `pre-push`：
  - 必须执行质量检查（fmt/clippy/build 等）
  - 若检查依赖固件目录存在，应在实现阶段做条件门控（例如仅当 `firmware/` 存在时运行）

## JS dev tools（`package.json` + `bun.lock`）

- 范围（Scope）: internal
- 变更（Change）: New
- 位置（Path）:
  - 仓库根目录 `package.json`
  - 仓库根目录 `bun.lock`（与参考工程一致；用于 `bun install --frozen-lockfile`）
- 格式（Encoding）:
  - `package.json`: JSON（utf-8）
  - `bun.lock`: lockfile（utf-8）

Schema（结构）：

- `package.json` 仅用于本计划的开发者工具（commitlint），最小字段集合：
  - `private: true`
  - `engines.bun: ">=1.3.5"`（与参考工程对齐；并冻结为生成 `bun.lock` 的版本线）
  - `devDependencies`: `@commitlint/cli` / `@commitlint/config-conventional` / `commitlint-plugin-function-rules`
  - `scripts.commitlint: "commitlint"`
- 禁止提交 `node_modules/`；安装产物应留在本地或 CI runner。
- 兼容性提示：若本项目（或贡献者本地）生成的是 `bun.lockb` 而非 `bun.lock`：
  - 迁移到文本 lockfile：执行 `bun install --save-text-lockfile --lockfile-only` 并删除 `bun.lockb`，然后提交 `bun.lock`。
  - 若仍持续生成 `bun.lockb`：检查是否设置了 `install.saveTextLockfile = true`（可通过 `bunfig.toml` 固定），或始终使用 `--save-text-lockfile` 运行安装命令（避免破坏可复现安装口径）。

## Commit message lint（`commitlint.config.cjs`）

- 范围（Scope）: internal
- 变更（Change）: New
- 位置（Path）: 仓库根目录 `commitlint.config.cjs`
- 格式（Encoding）: JS（CommonJS；utf-8）

Schema（结构）：

- 继承 Conventional Commits（`@commitlint/config-conventional`）
- 使用 `commitlint-plugin-function-rules` 实现英文口径校验：
  - Subject 禁止中文字符（CJK）
  - Body 禁止中文字符（CJK）
  - Subject 禁止首字母大写
- `type-enum` 必须与 PR title lint 的 types 对齐

## Format staged script（`scripts/format_staged.sh`）

- 范围（Scope）: internal
- 变更（Change）: New
- 位置（Path）: `scripts/format_staged.sh`
- 格式（Encoding）: bash（utf-8）

Schema（结构）：

- 仅格式化 staged Rust 文件（通过 `git diff --cached --name-only -- '*.rs'` 获取列表）
- 使用 `rustfmt` 直接格式化文件后 re-stage（通过 lefthook 的 `stage_fixed: true`）
- 不要求仓库根目录存在 Rust workspace（脚本不应依赖 `cargo fmt`）

## GitHub Actions workflows（`.github/workflows/*.yml`）

- 范围（Scope）: internal
- 变更（Change）: New
- 位置（Path）: `.github/workflows/`
- 格式（Encoding）: GitHub Actions workflow YAML（utf-8）

最低要求（文件名可调整，但职责需稳定）：

- `.github/workflows/ci.yml`
  - Rust fmt check（`cargo fmt --check`）
  - 对 `docs/**` 与 `README.md` 进行 `paths-ignore`
- `.github/workflows/firmware.yml`
  - Firmware build check（至少覆盖 `ESP32-S3`）
  - 对 `docs/**` 与 `README.md` 进行 `paths-ignore`
  - 推荐对固件相关路径做 `paths`/diff gate（避免无关 PR 触发构建）
- `.github/workflows/lint-pr-title.yml`
  - PR title 语义化 + 英文口径（与 `commitlint.config.cjs` 对齐）
- `.github/workflows/dependency-review.yml`
  - dependency review action
  - 对 `docs/**` 与 `README.md` 进行 `paths-ignore`
