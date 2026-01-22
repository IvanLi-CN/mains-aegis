# 命令行（CLI）

本契约用于冻结“开发者手动运行质量门槛”的命令口径，便于本地排错与对齐 CI 行为。

## 依赖安装（bun）

- 范围（Scope）: internal
- 变更（Change）: New
- Host: macOS + Linux

用法（Usage）：

```text
# In repo root
bun install --frozen-lockfile
```

输出（Output）：

- human

## Git hooks（lefthook）

- 范围（Scope）: internal
- 变更（Change）: New

用法（Usage）：

```text
# In repo root
lefthook install

# Manual run (debugging)
lefthook run pre-commit
# commit-msg hook requires a commit message file path; run the hook-equivalent instead:
bun run commitlint -- --edit .git/COMMIT_EDITMSG
lefthook run pre-push
```

退出码（Exit codes）：

- `0`: 成功
- 非 `0`: 失败（必须输出可定位的失败原因）

## Commit message lint（commitlint）

- 范围（Scope）: internal
- 变更（Change）: New

用法（Usage）：

```text
# Hook-equivalent (the file path is the commit message file)
bun run commitlint -- --edit <path-to-commit-msg>
```

约定（Rules）：

- Conventional Commits
- Subject/Body 禁止中文字符（CJK）
- Subject 禁止首字母大写

## Rust quality checks（与 CI 对齐）

（命令口径以实现阶段最终目录结构为准；建议以 `firmware/` 为工作目录。）

```text
cd firmware

cargo fmt --all -- --check
cargo clippy
cargo build
```
