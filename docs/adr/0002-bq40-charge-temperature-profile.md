# ADR 0002: 固化 BQ40/BQ25792 充电温度 profile

## 状态

已被 [ADR 0003](0003-charge-termination-coordination.md) 取代；原温度、请求与维护唤醒边界由后继决策保留。

## 背景

EVE INR21700/50E 4S1P 的电芯温度策略曾同时出现在 BQ40 Data Flash、BQ25792 JEITA、主固件手动充电和维护工具的不同路径中。若这些路径各自解释温度，低温降额、高温禁充和 BQ40 充电请求可能出现不一致。

项目还需要保留 BQ40 的 ChemID、学习、校准、保护和 termination golden image 边界，避免一个温度 profile 写入器顺带修改无关安全配置。

## 决策

1. 电芯本体温度定义为 BQ40 `TS2`、`TS3`、`TS4` 的最大值。`TS1` 是板侧温度点，不能放宽电芯上限。`<0°C` 和 `>=45°C` 禁止充电；不支持 45–55°C 低电流充电。
2. `0<=T<15°C` 使用 `100mA/16800mV`，`15<=T<45°C` 使用 `500mA/16800mV`。BQ40 `ChargingCurrent()`/`ChargingVoltage()`（SBS `0x14/0x15`）是实时安全上限，主固件以不超过一秒的周期读取，并与运行时目标取较小值。读失败、零值、越界或过期请求 fail-closed。
3. BQ40 温度字段固定为 `T1=0°C`、`T2=15°C`、`T5=20°C`、`T6=35°C`、`T3=40°C`、`T4=45°C`、`Hysteresis=2°C`，使用规定的七个 `0.1K` little-endian vectors。电流/电压表是静态 manifest，不是 Data Flash writer 字段。
4. BQ25792 使用项目已选的 10kΩ@25°C/β3380 NTC、RTS1=5.23kΩ、RTS2=30.1kΩ、`TS_COOL=15°C`、`TS_WARM=45°C`、`TS_IGNORE=0`。低温 JEITA 不再二次降流，高温 JEITA Suspend。低电流模式使用 `ITERM=40mA` 并关闭 `EN_TERM`；正常模式继续沿用 `CurrentAtEOC`。
5. 维护工具使用显式 `bq40-charge-temperature` profile。writer 只允许七个温度字段，写前保存 `0x4A00` 的完整 32-byte containing block，之后逐项执行 MB44 echo、length、raw、decoded 校验；任何错误立即停止。主固件运行时不自动写 Data Flash。
6. 普通 UI/USB 只保留 100mA 和 500mA；外部 1A 请求拒绝，旧持久化 1000mA 状态加载时归一到 500mA。ROM repair、AFE repair 和 asset recovery 保持既有语义。
7. activation/recovery 的固定 200mA wake 是独立维护状态，不属于产品充电例外：只有 BQ25792 JEITA 处于 `15–45°C` 正常窗口时才允许；若已有 BQ40 请求或温度缓存，则还必须通过新鲜有效请求与正常温度带门控，并按请求限制 wake 电流/电压。activation 完成或 auto wake hold 结束后只恢复 charger 寄存器，保持 `CE=HIGH`，由下一次普通轮询重新决定是否充电。

## 后果

- 充电决策有一个可测试的纯策略层，固件和维护工具可以共享稳定的向量与边界语义。
- BQ40 请求或温度缓存异常会导致禁充，可能降低可用性，但不会在缺少安全证据时继续充电。
- 维护 writer 是非原子操作；完整快照保留用于人工回滚和审计，不自动恢复无关 golden 字段。
- 完整的写前 containing block 快照会同时进入维护日志和 `summary.json`，便于离线审计和人工回滚；运行时不自动写 Data Flash。
- ChemID、learning、calibration、protection 和 termination 仍需独立 golden-image 流程，不由本 ADR 推导。

## 实现索引

- 规范：[`../specs/bq40-charge-temperature-profile/SPEC.md`](../specs/bq40-charge-temperature-profile/SPEC.md)
- 固件策略：[`../../firmware/src/charge_temperature.rs`](../../firmware/src/charge_temperature.rs)
- BQ40 SBS 读取：[`../../firmware/src/bq40z50.rs`](../../firmware/src/bq40z50.rs)
- BQ25792 JEITA：[`../../firmware/src/bq25792.rs`](../../firmware/src/bq25792.rs)
- 运行时闭环：[`../../firmware/src/output/mod.rs`](../../firmware/src/output/mod.rs)
- writer contract：[`../../tools/bq40-comm-tool/firmware/src/charge_profile.rs`](../../tools/bq40-comm-tool/firmware/src/charge_profile.rs)
