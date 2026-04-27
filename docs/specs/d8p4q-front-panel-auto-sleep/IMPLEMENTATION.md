# 实现状态（#d8p4q）

## 当前覆盖

- `firmware/src/display_power.rs` 定义测试版三段阈值、正式默认阈值和纯逻辑状态机。
- `firmware/src/front_panel.rs` 在运行时 tick 中读取输入、驱动状态机，并通过 GC9307 DCS 命令与 `BLK` 控制显示功耗。
- `firmware/src/main.rs` 将运行时用户关注状态映射为前面板 `attention_hold`。
- `firmware/host-unit-tests` 纳入 display power 纯逻辑测试。

## 验证

- 已通过：`bash firmware/scripts/run-host-unit-tests.sh`
- 已通过：`cargo +esp check --release --manifest-path Cargo.toml --bin esp-firmware`（在 `firmware/` 目录运行）
- 已通过：`cargo +esp check --release --manifest-path firmware/Cargo.toml --bin esp-firmware --target xtensa-esp32s3-none-elf -Zbuild-std=core,alloc`

## 后续恢复点

- 硬件确认后，将测试阈值从 `30_000 / 35_000 / 40_000ms` 恢复为 `180_000 / 240_000 / 245_000ms`。
