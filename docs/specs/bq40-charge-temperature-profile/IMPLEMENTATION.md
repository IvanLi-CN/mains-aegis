# BQ40 charge-temperature profile 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖与剩余缺口。

## Current Status

- Implementation: 固件运行时闭环、BQ25792 JEITA 配置、七字段 writer contract 和普通路径 1A 收敛已实现
- Lifecycle: active
- Hardware application: 已在绑定设备 `mains-aegis-198840` 上完成 `bq40-charge-temperature` 的七字段 Data Flash 写入，并在写入后恢复 canonical 固件

## Implementation Coverage

- `REQ-BQ40-CTP-001` / `002`: `firmware/src/charge_temperature.rs` 固化 TS2/TS3/TS4 最大值、边界判定和七个 little-endian vectors；host tests 覆盖 `<0°C`、`0°C`、`15°C`、`45°C`、`50°C` 和 `45–55°C`。
- `REQ-BQ40-CTP-003`: 同一模块提供 LT/normal 静态 manifest；BQ40 current/voltage 只作为策略记录，不进入 writer allowlist。
- `REQ-BQ40-CTP-004`: `firmware/src/output/mod.rs` 每秒最多刷新 SBS `0x14/0x15`，并同步读取已锁定的 `Temperature Enable=0x1E` / `Temperature Mode=0x00`；正常产品充电在配置缺失或不匹配时 no-charge，避免用错误的 TS 映射放行充电；每次主循环 tick 还会复核已使能充电的 BQ40 温度与 request 是否仍在 `1s` 有效窗口内，证据过期时先调度立即 charger poll，由该 poll 先刷新请求再作 fail-closed 决策，避免有效期边界导致 CE 反复开关；刷新仍失败、零值、越界或温度证据无效时在该 poll 禁充。boot-diagnostic auto-force 虽可在尚无 BQ40 观察时依赖 BQ25792 JEITA 唤醒，但一旦出现 BQ40 证据，同一 tick 联锁也会在刷新失败或证据无效时禁充；温度/配置证据 latch 跨越瞬态 detail-cache reset，明确读到的错误配置在新的有效配置读回前持续阻断。activation wake 在尚未观察到 BQ40 温度或配置证据时允许依赖 BQ25792 JEITA 唤醒，明确读到错误配置时仍阻断。对温度、请求有效性、请求新鲜度和 runtime target 做 min clamp；低电流路径先关闭 `EN_TERM` 再写入 `ITERM=40mA`，正常路径要求对最终 `ICHG` 严格更低的 `CurrentAtEOC`，否则 no-charge；activation/boot-diagnostic 的 `200mA` wake 明确关闭 `EN_TERM`。activation wake 是独立维护状态，只在原始 BQ25792 `VBAT_PRESENT_STAT=1` 且 JEITA 正常窗口内运行，并在已有 BQ40 请求或温度/配置证据观测（包括无效值）时复用新鲜正常温度/请求门控；恢复寄存器后 CE 保持高电平。因项目 HPE=0，`ChargingCurrent/ChargingVoltage` 使用明确的两字节 SBS 读取，PEC helper 不做隐式帧回退。
- `REQ-BQ40-CTP-005`: `firmware/src/bq25792.rs` 固化 10kΩ/β3380、5.23kΩ/30.1kΩ、15°C/45°C JEITA 编码，并保留 BHOT/BCOLD 无关位；`charge_temperature.rs` 同时记录已锁定的 BQ40 `Temperature Enable=0x1E` / `Temperature Mode=0x00` 解释前提。
- `REQ-BQ40-CTP-006`: `tools/bq40-comm-tool/firmware/src/charge_profile.rs` 实现七字段 allowlist、0x4A00/32-byte 快照、MB44 echo/length/raw/decoded fail-stop contract；逐字段 readback 按 echoed address 的 payload 起点校验并保留七项实际 echo 证据，最终 containing block 按字段 offset 校验；成功与失败都保留写前完整 block，并将写前/写后完整块、CE/身份/Full Access gate 与七项验证证据作为 `summary.json` 的 `snapshot_before_hex`、`final_after_hex`、gate flags 和 `verified_fields` 持久化，parser 同时拒绝任一非 allowlist 字节变化、未知或 malformed stage/profile、乱序/重复证据，并对显式 profile verify 同样启用完整校验，profile 失败时维护固件锁存 CE high 并停止充电；profile image 只能通过同时满足 `ManufacturerName()` 的 Texas 证据与 `DeviceName()` 的 BQ40 证据进入，writer 会记录不可替代的 strict/R2/canonical provenance，且 ROM/recovery 事件与该 profile 冲突，writer 前会再次读取身份以覆盖 wake-window 数值探测路径，失败锁存跨 transport loss 保持 fail-closed，需重新启动维护命令才建立重试边界；`diagnose`/`recover`、`mac-only`、`dual-diag`、非 R2 ROM image 与 recovery/repair feature 组合在构建、CLI 和 offline verify 层均被拒绝；低层 `build.sh` 只有显式 `--action apply-df` 才会编译 writer feature；`bq40-charge-temperature` 是唯一温度 profile，旧 `live-df-mainboard` 明确迁移失败，且 compile-time guard 禁止与 recovery/repair feature 组合。
- `REQ-BQ40-CTP-007`: `4S1P`、`5000mAh`、`14600mV` 与 `16800mV` 已纳入设计口径；ChemID 和非温度保护字段仍由独立 golden-image/化学匹配流程管理。

## Verification Commands

- Markdown links and field references: `rg -n "bq40-charge-temperature-profile|T1|T2|T3|T4|T5|T6|0x4A0B|0x4A15" docs`
- Topic contract: set `AGENT_TOOLKIT_ROOT` to a checkout of the public `agent-toolkit` repository, then run `just spec-contract-check` (the checker receives the repository root explicitly).
- Writer contract and report-parser fail-closed fixtures: `just bq40-charge-profile-test`
- Read-only HIL decoder contract fixtures: `just hil-diag-snapshot-test`
- BQ40 maintenance firmware feature: `cd tools/bq40-comm-tool/firmware && cargo +esp check --features bms-rom-recover-disable,bms-charge-temperature-profile,bms-charge-temperature-apply`
- Firmware host tests: `just firmware-host-test`（调用 `firmware/scripts/run-host-unit-tests.sh`，包含 host-test audit 与全部 host manifests）

## Coverage / rollout summary

- 文档已把电芯本体 `0–45°C` 充电窗口、`100mA/500mA` 策略、BQ40 原始编码、BQ25792 协同和未来写入边界收敛为单一 profile。
- 目标硬件写入证据已经闭环，机器可读摘要见 [`evidence/hardware-apply-summary.json`](evidence/hardware-apply-summary.json)；原始临时 `summary.json` 的 SHA-256 为 `8bdb8ad753c631901e9319ce57c36848393cb807f2e1b132831c0e891efc1518`。摘要包含 `identity_verified=true`、`full_access_verified=true`、`writes=7`、`verified_fields` 覆盖 T1/T2/T5/T6/T3/T4/Hysteresis，写前和写后均为 `0x4A00` 的 32-byte containing block，非 allowlist 字节保持不变。
- 写入后的 `final_after_hex` 为 `0505000564020564050500ac0a420b740b0a0c3c0c6e0c1400a00f8400600108`，其中七个字段为 `AC 0A`、`42 0B`、`74 0B`、`0A 0C`、`3C 0C`、`6E 0C`、`14 00`；离线 `verify` 报告 `verdict.pass=true`。
- 写入后已恢复 canonical artifact，设备身份与 artifact 完全匹配，`log_decode=verified`。canonical 固件烧录不修改 BQ40 Data Flash。

## Hardware Verification

- 目标设备：绑定 stable ID `serial-04f3bb3f5367`，逻辑 ID `mains-aegis-198840`，固定端口 `/dev/cu.usbmodem2111401`。
- 维护写入：显式 `apply-df --mode canonical --force-min-charge false --probe-mode strict --repair-profile bq40-charge-temperature`；维护镜像在写入期间保持 `CE` 高电平，未强制给存在严重压差的电池包充电。
- 写入顺序和证据：`CE high/charger disabled`、strict/canonical/R2 provenance、Texas/BQ40 identity、Full Access、完整 containing block 快照、七项 MB44 echo/length/raw/decoded readback、最终 containing block 和 `stage=done fields=7` 均已通过离线验证。
- canonical HIL 只读观察显示 BQ40 持续响应，但当前包仍处于安全阻断：`xchg=true`、`chg_fet=false`、`chg_ready=false`，`cell_min` 约 `3152–3153mV`、`cell_max` 约 `3870–3871mV`、压差约 `717–719mV`，主固件保持禁充。
- 该 HIL 窗口未观察到 cell-balance 动作日志；`dsg_fet=true` 是整包放电 FET 状态，不等于正在对最低电压的 C2 做单体放电。C2/C3 的严重压差和 BQ40 `xchg_blocked` 需要在满足独立安全条件后另行处理，不由本 profile 推导或自动修复。

## Boundaries

- 通过 TI chemistry matching 完成独立 ChemID golden image；不得由本 profile 推测 ChemID。
- 本次不执行温箱/热风枪和量产校准；主固件运行时仍不自动写 Data Flash。硬件写入只能由显式 maintenance 命令完成，并必须沿用 bound-device/devd/session-lock 和 owner authorization。

## Related Changes

- `firmware/src/charge_temperature.rs`
- `firmware/src/bq40z50.rs`
- `firmware/src/bq25792.rs`
- `firmware/src/output/mod.rs`
- `tools/bq40-comm-tool/firmware/src/charge_profile.rs`
- `tools/bq40-comm-tool/docs/operations.md`

## References

- `./SPEC.md`
- `./HISTORY.md`
- `../../bms-design.md`
- `../../charger-design.md`
