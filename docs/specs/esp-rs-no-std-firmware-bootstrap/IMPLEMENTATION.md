# 初始化 ESP32-S3（esp-rs / esp-hal）no_std 固件工程 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Lifecycle: archived
- Implementation: 见下方迁移状态与覆盖记录。

## Migrated Implementation Record

- Status: 已完成
- Created: 2026-01-22
- Last: 2026-01-22

- [x] M1: 落地 `firmware/` 工程骨架（`esp-hal` + `no_std` + `esp32s3`），并能输出串口启动信息
- [x] M2: 补齐 `firmware/README.md` 的安装/构建/烧录/监视器与排错指引
- [x] M3: 完成一次端到端手工验证记录（所用硬件、连接方式、命令、预期输出），并更新 `docs/README.md` 入口链接

（本节用于 M3；由执行者在实际硬件上完成后补齐。）

- Date: 2026-01-22
- Host:
  - OS: macOS 15.6.1
  - Tooling: `mcu-agentd 0.1.0` + `espflash 4.2.0` + Rust toolchain `esp`
- Hardware:
  - Board: mains-aegis mainboard（rev unknown）
  - Connection:
    - Front panel `USB1` → host
    - Port: `/tmp/fixture-firmware-usb-port`
    - Selector cache: `firmware/.esp32-port`（首次 `monitor` 可能提示绑定 MAC；确认后写入 `mac=<MAC>` 行）
    - MAC: `50:78:7d:19:88:40`

### Commands

```bash
# Build (firmware-local toolchain/config)
cd firmware
cargo build --release
cd ..

# Flash + monitor (run from repo root; uses ./mcu-agentd.toml)
mcu-agentd flash esp
mcu-agentd monitor esp --from-start
```

### Observed output (excerpt)

- Bootloader/ROM 输出（可能包含）
- 应用层输出至少包含（其中一项即可视为“可辨识启动信息”）：
  - `esp: boot (serial)`
  - `esp: boot`（`defmt` 解码）
  - `esp: heartbeat`（`defmt` 解码，周期性）

实际观测（节选）：

```text
esp: boot (serial)
[INFO ] esp: boot
[INFO ] esp: heartbeat
```

监视记录：`/.mcu-agentd/monitor/esp/20260122_093840.mon.ndjson`（heartbeat 约每 2s 输出一次；本次观测窗口内未出现 panic）。

## References

- `./SPEC.md`
- `./HISTORY.md`
