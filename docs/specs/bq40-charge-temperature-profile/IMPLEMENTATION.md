# BQ40 charge-temperature profile 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖与剩余缺口。

## Current Status

- Implementation: 固件运行时闭环、BQ25792 JEITA 配置、七字段 writer contract 和普通路径 1A 收敛已实现
- Lifecycle: active
- Catalog note: 本次只完成源码、host tests 和文档；不执行硬件操作、flash 或 Data Flash 写入

## Implementation Coverage

- `REQ-BQ40-CTP-001` / `002`: `firmware/src/charge_temperature.rs` 固化 TS2/TS3/TS4 最大值、边界判定和七个 little-endian vectors；host tests 覆盖 `<0°C`、`0°C`、`15°C`、`45°C` 和 `45–55°C`。
- `REQ-BQ40-CTP-003`: 同一模块提供 LT/normal 静态 manifest；BQ40 current/voltage 只作为策略记录，不进入 writer allowlist。
- `REQ-BQ40-CTP-004`: `firmware/src/output/mod.rs` 每秒最多刷新 SBS `0x14/0x15`，对温度、请求有效性、请求新鲜度和 runtime target 做 min clamp；低电流路径先关闭 `EN_TERM` 再写入 `ITERM=40mA`，正常路径保持 `CurrentAtEOC`。activation wake 是独立维护状态，只在 BQ25792 `VBAT_PRESENT_STAT=1` 且 JEITA 正常窗口内运行，并在已有 BQ40 证据时复用新鲜正常温度/请求门控；恢复寄存器后 CE 保持高电平。因项目 HPE=0，`ChargingCurrent/ChargingVoltage` 使用明确的两字节 SBS 读取，PEC helper 不做隐式帧回退。
- `REQ-BQ40-CTP-005`: `firmware/src/bq25792.rs` 固化 10kΩ/β3380、5.23kΩ/30.1kΩ、15°C/45°C JEITA 编码，并保留 BHOT/BCOLD 无关位；`charge_temperature.rs` 同时记录已锁定的 BQ40 `Temperature Enable=0x1E` / `Temperature Mode=0x00` 解释前提。
- `REQ-BQ40-CTP-006`: `tools/bq40-comm-tool/firmware/src/charge_profile.rs` 实现七字段 allowlist、0x4A00/32-byte 快照、MB44 echo/length/raw/decoded fail-stop contract；逐字段 readback 按 echoed address 的 payload 起点校验，最终 containing block 按字段 offset 校验；成功与失败都保留写前完整 block，并将写前/写后完整块与七项验证证据作为 `summary.json` 的 `snapshot_before_hex`、`final_after_hex` 和 `verified_fields` 持久化，parser 同时拒绝任一非 allowlist 字节变化；`bq40-charge-temperature` 是唯一温度 profile，旧 `live-df-mainboard` 明确迁移失败，且 compile-time guard 禁止与 recovery/repair feature 组合。
- `REQ-BQ40-CTP-007`: `4S1P`、`5000mAh`、`14600mV` 与 `16800mV` 已纳入设计口径；ChemID 和非温度保护字段仍由独立 golden-image/化学匹配流程管理。

## Verification Commands

- Markdown links and field references: `rg -n "bq40-charge-temperature-profile|T1|T2|T3|T4|T5|T6|0x4A0B|0x4A15" docs`
- Topic contract: set `AGENT_TOOLKIT_ROOT` to a checkout of the public `agent-toolkit` repository, then run `just spec-contract-check` (the checker receives the repository root explicitly).
- Writer contract and report-parser fail-closed fixtures: `just bq40-charge-profile-test`
- BQ40 maintenance firmware feature: `cd tools/bq40-comm-tool/firmware && cargo +esp check --features bms-charge-temperature-profile`
- Firmware host tests: `just firmware-host-test`

## Coverage / rollout summary

- 文档已把电芯本体 `0–45°C` 充电窗口、`100mA/500mA` 策略、BQ40 原始编码、BQ25792 协同和未来写入边界收敛为单一 profile。
- 本规范不表示已经向任何设备写入 Data Flash；真实维护执行仍需显式 maintenance 命令、bound-device/devd/session-lock 和 owner authorization。

## Boundaries

- 通过 TI chemistry matching 完成独立 ChemID golden image；不得由本 profile 推测 ChemID。
- 本实现不执行温箱、热风枪、真实设备、flash 或 Data Flash 写入。

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
