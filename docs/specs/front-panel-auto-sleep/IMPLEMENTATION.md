# 实现状态（front-panel-auto-sleep）

## 当前覆盖

- `firmware/src/display_power.rs` 定义压缩测试时序、正式默认时序和纯逻辑状态机。
- `firmware/src/front_panel.rs` 使用 `release_default()` 的 `180s / 240s / 245s` 时序，在运行时 tick 中读取输入、驱动状态机，并通过 GC9307 DCS 命令与 `BLK` 控制显示功耗。
- `firmware/src/front_panel_logic.rs` 提供 host 可测试的前面板输入事件分类；`FrontPanel::tick` 仅将按键按下沿、首次触摸和变化后的非零手势交给 idle 状态机。
- `firmware/src/main.rs` 将运行时用户关注状态映射为前面板 `attention_hold`。
- `firmware/src/main.rs` 在前面板仍显示自检页且自检快照不能进入 dashboard 时将该状态并入 `attention_hold`，让自检阻塞页面保持唤醒直到阻塞解除。
- `firmware/host-unit-tests` 纳入 display power 纯逻辑测试。
- `front_panel_logic` 回归测试锁定持续输入不重复重置以及新的输入边沿仍可唤醒。
- `front_panel.io` 从正常输入轮询缓存导出 TCA/CST816D 原始输入，普通 status 不暴露这些诊断字段。

## 验证

- 已通过：`bash firmware/scripts/run-host-unit-tests.sh`
- 已通过：`cargo +esp check --release --manifest-path Cargo.toml --bin mains-aegis-firmware`（在 `firmware/` 目录运行）
- 已通过：`cargo +esp check --release --manifest-path firmware/Cargo.toml --bin mains-aegis-firmware --target xtensa-esp32s3-none-elf -Zbuild-std=core,alloc`
- 已通过：`cargo +esp build --manifest-path firmware/Cargo.toml --bin mains-aegis-firmware --release --target xtensa-esp32s3-none-elf -Zbuild-std=core,alloc --features net_http,web_serial`
- 已通过：`cargo test --manifest-path tools/mains-aegis-host/Cargo.toml --lib`
- 已通过：烧录到 `/tmp/fixture-ups-usb-port` 后，硬件自检后的屏幕显示由主人确认恢复正常。

## Migrated Implementation Record

- Lifecycle: active
- Implementation: 已实现（正式时序）
- Created: 2026-04-27
- Last: 2026-04-27

## References

- `./SPEC.md`
- `./HISTORY.md`
