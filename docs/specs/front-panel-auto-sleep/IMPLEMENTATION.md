# 实现状态（front-panel-auto-sleep）

## 当前覆盖

- `firmware/src/display_power.rs` 定义测试版三段阈值、正式默认阈值和纯逻辑状态机。
- `firmware/src/front_panel.rs` 在运行时 tick 中读取输入、驱动状态机，并通过 GC9307 DCS 命令与 `BLK` 控制显示功耗。
- `firmware/src/main.rs` 将运行时用户关注状态映射为前面板 `attention_hold`。
- `firmware/src/main.rs` 在前面板仍显示自检页且自检快照不能进入 dashboard 时将该状态并入 `attention_hold`，让自检阻塞页面保持唤醒直到阻塞解除。
- `firmware/host-unit-tests` 纳入 display power 纯逻辑测试。

## 验证

- 已通过：`bash firmware/scripts/run-host-unit-tests.sh`
- 已通过：`cargo +esp check --release --manifest-path Cargo.toml --bin esp-firmware`（在 `firmware/` 目录运行）
- 已通过：`cargo +esp check --release --manifest-path firmware/Cargo.toml --bin esp-firmware --target xtensa-esp32s3-none-elf -Zbuild-std=core,alloc`
- 已通过：`cargo +esp build --manifest-path firmware/Cargo.toml --bin esp-firmware --release --target xtensa-esp32s3-none-elf -Zbuild-std=core,alloc --features net_http,web_serial`
- 已通过：`cargo test --manifest-path tools/mains-aegis-host/Cargo.toml --lib`
- 已通过：烧录到 `/tmp/fixture-ups-usb-port` 后，硬件自检后的屏幕显示由主人确认恢复正常。

## 后续恢复点

- 硬件确认后，将测试阈值从 `30_000 / 35_000 / 40_000ms` 恢复为 `180_000 / 240_000 / 245_000ms`。

## Migrated Implementation Record

- Lifecycle: active
- Implementation: 已实现（测试时序）
- Created: 2026-04-27
- Last: 2026-04-27

## References

- `./SPEC.md`
- `./HISTORY.md`
