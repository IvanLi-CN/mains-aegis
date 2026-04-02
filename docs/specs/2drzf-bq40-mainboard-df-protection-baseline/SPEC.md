# BQ40 主板 DF 保护基线（#2drzf）

## 状态

- Status: 已完成
- Created: 2026-04-03
- Last: 2026-04-03

## 背景 / 问题陈述

- 主板当前已经收敛出 `asset-df-mainboard` 修复路径，但该路径此前只覆写了 `DA Configuration / Manufacturing Status Init / Temperature / AFE Protection Control / calibration` 等字段，没有真正冻结 `OCC/OCD/SOCC/SOCD`。
- 实板 `19V / 5A` 负载下曾出现 `OCD1 + XDSG blocked` 掉电，说明 live pack 的实际 DF 阈值与系统目标功率窗口不一致。
- 主固件已经具备主动热降额、闭环风扇、TMP 硬件保护测试模式与 `THERM_KILL_N` 观测，但风扇与主动热保护此前只看 `TMP112A/B`，没有把 `BQ40Z50` 提供的电池/板载/TS 传感器温度纳入同一套热控口径。

## 目标 / 非目标

### Goals

- 冻结主板 `asset-df-mainboard` 的 charge / discharge over-current DF 基线，确保 recover 路径真正把 pack 带回仓库定义的主板保护口径。
- 充电阈值保持保守，以上游 `BQ25792` 的 `ICHG <= 5A` 能力上限为主约束。
- 放电阈值只保留小幅抗抖余量，以 `19V` 输出目标与典型转换效率为依据，不再把 `OCD/SOCD` 维持在明显低于系统需求的 stock 值。
- 主固件风扇与主动热保护统一改为使用 `max(TMP_A, TMP_B, BMS board, BMS battery, BMS TS1..TS4)`。
- README / 模块文档 / 工具文档同步写清楚阈值、依据与使用口径。

### Non-goals

- 不修改 `TMP112` 硬件阈值口径（仍保持 `40°C / 50°C`）。
- 不改 `AOLD / ASCC / ASCD` 的 AFE 硬件级短路/过载阈值，本轮优先修正已被实测击中的软件电流保护层。
- 不引入运行时可配置 DF 参数，也不在本轮引入电池侧电流自适应功率分配算法。

## 设计冻结

### 1) 充电保护基线

依据：

- `BQ25792` 当前固件封装允许的 `ICHG` 上限是 `5000mA`。
- 当前主线充电策略仍冻结在 `500mA` 正常充电 / `100mA` DC 过载降档，因此 BMS charge over-current 只需要覆盖芯片上限与少量控制环误差。

冻结值：

| 字段 | 地址 | 建议值 | 说明 |
| --- | --- | ---: | --- |
| `OCC1 Threshold` | `0x495E` | `4500mA` | 长时间连续超上限前的第一层保护 |
| `OCC1 Delay` | `0x4960` | `6s` | 给充电控制环与采样抖动留出恢复窗口 |
| `OCC2 Threshold` | `0x4961` | `5200mA` | 略高于 `BQ25792` ceiling，避免紧贴 `5A` 误触发 |
| `OCC2 Delay` | `0x4963` | `3s` | 第二层快速切断 |
| `SOCC Threshold` | `0x49C9` | `6000mA` | 永久故障阈值，仍明显低于电芯“10A 最大充电”的争议上限 |
| `SOCC Delay` | `0x49CB` | `5s` | 仅针对持续异常 |

### 2) 放电保护基线

依据：

- 电芯连续放电上限：`15A`。
- `19V * 120W` 级别输出在 `VBAT=10.0V`、`η=85%` 的保守条件下，对电池侧约需 `14.12A`。
- 因此放电保护只保留“小于 1A”的一级抗抖余量，不再延续 `-6A / -8A / -10A` 这类明显偏低的 stock 默认值。

冻结值：

| 字段 | 地址 | 建议值 | 说明 |
| --- | --- | ---: | --- |
| `OCD1 Threshold` | `0x4967` | `-14500mA` | 覆盖 `120W` 级放电窗口，给效率/采样误差留小余量 |
| `OCD1 Delay` | `0x4969` | `6s` | 持续过载才锁定 `XDSG` |
| `OCD2 Threshold` | `0x496A` | `-15000mA` | 接近电芯连续放电边界，不再留大余量 |
| `OCD2 Delay` | `0x496C` | `3s` | 第二层快速保护 |
| `SOCD Threshold` | `0x49CC` | `-16000mA` | 仅给瞬态和测量误差保留有限 PF 裕量 |
| `SOCD Delay` | `0x49CE` | `5s` | 避免短脉冲直上 PF |

### 3) 共享热控真相源

- 风扇闭环控制温度：`max(TMP_A, TMP_B, BMS board, BMS battery, BMS TS1..TS4)`。
- 主动热保护温度：与风扇闭环完全共用同一个“最高温”口径。
- `tmp-hw-protect-test` 构建中仍继续采集并展示这组温度，只是关闭 MCU 主动散热和软件热保护动作。

## 接口变更（Interfaces）

- `tools/bq40-comm-tool/firmware/src/output/mod.rs`
  - `asset-df-mainboard` 新增 `OCC/OCD/SOCC/SOCD` 覆写。
  - 新增 app-mode live DF 基线写入入口 `live-df-mainboard`，通过 `./bin/run.sh apply-df ... --repair-profile live-df-mainboard` 写回 pack。
- `firmware/src/fan.rs`
  - 风扇温控输入新增 BMS 聚合温度。
- `firmware/src/output/mod.rs`
  - 风扇与主动热保护统一改为共享热控最高温输入。
- `firmware/README.md`
  - 风扇控温源说明从 `TMP112A/B` 更新为“TMP + BMS 温度最高值”。
- `docs/modules/regulated-output.md`
  - 主动热保护口径更新为共享热控最高温。

## 验收标准（Acceptance Criteria）

- `asset-df-mainboard` 生成的 section1 覆写中，`OCC1/OCC2/OCD1/OCD2/SOCC/SOCD` 地址被明确写入仓库冻结值。
- 主固件风扇策略在 `TMP` 缺失但 BMS 温度可用时，仍能继续闭环调速而不是直接退到 full-speed fail-safe。
- 主动热保护使用与风扇相同的共享热控最高温；当 BMS 温度高于 `TMP` 时，日志中的 `max_temp_c_x16` 体现 BMS 侧热点。
- `tmp-hw-protect-test` 构建下，风扇仍保持 `off` 且 `thermal_protection_enabled=false`，但共享热控温度的遥测/详情页仍可读。
- 文档中明确写出 `BQ25792` 的 `5A` 充电能力上限与主板 DF 保护基线，不再要求通过人工负载试探来猜测阈值。
