# 文件格式（File formats）

将文件/目录结构视为一种接口契约来描述：用于约束固件工程落地位置与关键文件的形状，避免后续计划/实现“各自为政”。

## 固件工程目录结构（`firmware/`）

- 范围（Scope）: internal
- 变更（Change）: New
- 编码（Encoding）: utf-8（除二进制构建产物外）

### Schema（结构）

固件工程必须位于仓库根目录下的 `firmware/`，并尽量把工程相关配置隔离在该目录下。

最低结构要求（实现阶段可在此基础上增量扩展，但不应随意重命名/搬迁）：

```text
firmware/
  README.md
  Cargo.toml
  rust-toolchain.toml
  mcu-agentd.toml
  .esp32-port
  .mcu-agentd/
  .cargo/
    config.toml
  src/
    main.rs
  (可选) build.rs
  (可选) memory.x / linker scripts
```

约束：

- `firmware/` 目录应当可独立运行 `cargo ...` 命令（不要求仓库根目录存在 Rust workspace）。
- `rust-toolchain.toml` 与 `.cargo/config.toml` 放在 `firmware/` 内，避免影响仓库中未来可能新增的其它语言/工程。
- 构建产物（如 `firmware/target/`）在实现阶段需要加入忽略规则（`.gitignore`）；本契约不规定其具体忽略写法，但要求“不得被提交到仓库”。
- `mcu-agentd` 配置文件固定为 `firmware/mcu-agentd.toml`（使 `firmware/` 成为该工具的 project root）。
- 串口选择缓存文件为 `firmware/.esp32-port`（由 `mcu-agentd` selector 写入；不应提交到仓库）。
- `firmware/.mcu-agentd/` 为运行态目录（logs/state；不应提交到仓库）。

### Examples（示例）

无（该接口为新增接口；以实现阶段落地的目录树为准）。

### 兼容性与迁移（Compatibility / migration）

- 后续如需引入多 crate workspace（例如 `firmware/crates/*`），应以“新增目录”为主，避免破坏 `firmware/` 作为入口目录的约定。
- 如必须搬迁 `firmware/` 的入口位置：需要在 `../PLAN.md` 里作为接口变更重新冻结，并提供迁移指引（包含路径重定向与文档更新）。
