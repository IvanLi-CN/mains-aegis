# 命令行（CLI）

本契约用于冻结“固件开发者工作流”的推荐命令口径：构建、烧录与串口监视。

## `cargo espflash`（flash + monitor）

- 范围（Scope）: internal
- 变更（Change）: New
- 支持平台（Host）: macOS + Linux
- 日志格式（Logs）: `defmt`（由 `espflash` 在 monitor 侧解码）

### 用法（Usage）

```text
cd firmware

# Build only
cargo build
cargo build --release

# Flash (recommended)
DEFMT_LOG=info cargo espflash flash --release --log-format defmt

# Flash + monitor (recommended for bring-up)
DEFMT_LOG=info cargo espflash flash --release --monitor --baud 115200 --log-format defmt
```

### 参数（Args / options）

- `flash`: 写入固件到目标设备
- `--release`: 使用 release profile
- `--monitor`: 烧录后进入串口监视器
- `--baud <n>`: 串口波特率（默认以实现阶段 README 口径为准；bring-up 推荐 `115200`）
- `--log-format defmt`: 显式启用 `defmt` 日志解码（避免按默认 `serial` 输出解析）
- `DEFMT_LOG=<level>`: 日志级别（如 `info`/`debug`；由实现阶段 `firmware/README.md` 固定默认值）

### 输出（Output）

- Format: human
- 期望输出包括：
  - 烧录进度/成功提示
  - 串口输出（至少能看到启动标识串；`defmt` 解码；bring-up 入口为前面板 `USB1`）

### 退出码（Exit codes）

- `0`: 成功
- `1`: 一般失败（例如：找不到设备、权限不足、构建失败、烧录失败）

### 兼容性与迁移（Compatibility / migration）

- 若后续迁移到 `probe-rs`/RTT 或 USB Serial-JTAG 工作流，需要：
  - 在此文档新增对应命令小节
  - 并在 `../PLAN.md` 的接口清单中将该 CLI 接口标注为 `Modify`，说明旧→新的差异与迁移建议
