# BQ40Z50 通信故障排查笔记

## 0. 结论先行

- 本次问题不是单一“芯片坏/线坏”，而是 **供电门槛 + 电池栈有效性 + 协议有效性校验不足 + 工具链状态异常** 的叠加。
- 在 `tools/bq40-comm-tool` 里落实“最小充电唤醒 + canonical 地址 + 严格校验 + managerd 排障”后，历史样本曾达到验收阈值；但 2026-03-06 主人当前接入的这块板子仍处于阻断态，尚未恢复通信。

## 1. 适用范围

- 仅适用于工具目录 `tools/bq40-comm-tool` 的故障诊断与恢复流程。
- 不覆盖主固件业务逻辑，只记录通信链路排障所需信息。

## 1.1 本次排查的硬件边界条件（重要）

- 本次大部分在线排查、ROM 探测与重刷验证，均在 **电池未连接 + 外部充电器提供偏置** 条件下完成。
- 在该条件下，BQ40 仍然可能上电、进入 TI ROM `0x9002`、完成刷写并退出 ROM；因此“能烧录”不等于“已恢复正常应用态通信”。
- 对于 **无电池** 样本，需把结论拆成两层：
  1. `ROM/重刷链路是否健康`；
  2. `应用态是否看到了有效电池栈并恢复正常 SBS 数据`。
- 若刷后仅表现为 `Temperature()` 正常、`RelativeStateOfCharge()=0`、`Voltage()` 只有几 mV/几十 mV，则应优先怀疑“芯片未看到有效电池栈条件”，而不是再次把问题归咎于烧录失败。

## 2. 证据与现象（按时间线）

### 2.1 历史故障证据（主工程早期日志）

- `.mcu-agentd/monitor/esp/20260209_095024.mon.ndjson`
  - 现象：出现 ACK 但数据可疑常量化（典型为 `5911/5911/0x1717`）并伴随子字段 NACK。
  - 结论：链路“能回数据”不等于“数据可信”。

- `.mcu-agentd/monitor/esp/20260209_134121.mon.ndjson`
  - 现象：`0x0B` 与 `0x16` 均 miss（NACK）。
  - 结论：存在纯物理/供电层不可达阶段。

### 2.2 工具化后在线证据（本轮）

- `reports/20260224_135105/summary.json`
  - `verdict.pass=true`
  - `max_valid_streak=26`
  - `poll_errors={"i2c_nack":1,"inconsistent_sample":3}`

- `reports/20260224_135322/summary.json`
  - `verdict.pass=true`
  - `max_valid_streak=40`
  - `poll_errors={"i2c_nack":1}`

- `reports/20260224_verify_latest/summary.json`（离线复算）
  - `verdict.pass=true`
  - `max_valid_streak=68`

## 3. 根因拆分（不是互斥关系）

1. **供电根因**：未建立最小充电电流唤醒路径时，BQ40 常见为持续 NACK。
2. **协议根因**：仅按“读到了就算成功”会把陈旧/污染帧误判为有效通信。
3. **并发根因**：共享 I2C 总线时其他周期读会干扰 BMS 采样稳定性。
4. **工具根因**：`mcu-managerd` IPC/仲裁异常会导致 `mcu-agentd` 看起来“卡死无输出”。

## 4. 最终收敛方案（工具内）

### 4.1 电源/充电前置

- 固定最小唤醒参数：
  - `VREG=16.8V`
  - `ICHG=200mA`
  - `IINDPM=500mA`
- 目的：先保证 BQ40 处于可通信供电状态，再评估协议层。

### 4.2 地址语义统一

- 默认仅访问 `7-bit 0x0B`（canonical）。
- `0x16` 仅在显式 `dual-diag` 诊断语义中出现，不能作为常规运行路径。

### 4.3 严格有效性校验

- 字段范围校验：温度、电压、SOC、状态字。
- 双采样一致性校验：失败计入 `inconsistent_sample`。
- 陈旧模式过滤：拒绝可疑常量组合误报成功。

### 4.4 工具链稳定性兜底

- 若 `mcu-agentd` 无输出/假挂起：
  1. `mcu-managerd status`
  2. 异常时前台运行 `mcu-managerd run`
  3. 重跑 `./bin/run.sh diagnose ...`

## 5. 可执行排障 SOP（无逻辑分析仪版本）

1. 先确认 managerd 正常：
   ```bash
   mcu-managerd status
   ```
2. 跑安全诊断（不写 ROM）：
   ```bash
   ./bin/run.sh diagnose --mode canonical --duration-sec 120 --force-min-charge true
   ```
3. 看报告是否达标：
   - `verdict.pass=true`
   - `max_valid_streak>=10`
   - `canonical` 模式下不得出现 `addr=0x16`
4. 仅在出现 ROM 签名时执行恢复：
   ```bash
   ./bin/run.sh recover --mode dual-diag --recover if-rom --force-min-charge true --rom-image r2
   ```
5. 恢复后必须重新刷回 canonical 并再次诊断，然后用同一份 canonical 日志做离线 verify。

## 6. ROM 模式相关结论

- `rom_events.flash_done=true` 的新口径：仅当 monitor 日志出现 `stage=probe_rom_flash_done`（已确认回到 firmware mode）才算 ROM 恢复成功。
- 若只看到 `stage=rom_flash_done rsoc_after=0x9002`，或报告为 `flash_attempted=true` 且 `flash_done=false`，表示 ROM 序列跑过但并未确认回到 firmware mode。

## 7. 仍需持续观察的风险

- `poll_errors` 仍可能出现低频 `i2c_nack`（单次不代表回归）。
- 若后续在高扰动场景下 `max_valid_streak` 退化，优先检查：
  1) 供电路径是否回退；2) 总线并发是否恢复；3) managerd 是否异常。

## 8. 阻断态识别（继续排查时很重要）

### 8.1 2026-03-06 最新实板证据

- `reports/20260306_231847/summary.json`
  - `verdict.pass=false`
  - `rom_events.detected=false`
  - `samples_total=0`
- `reports/20260306_232307/summary.json`
  - 在新增 `bms_diag_word` 细化日志后，结论仍未改变：`samples_total=0`、`flash_attempted=false`、`flash_done=false`。
- `reports/20260306_234713/summary.json`
  - 把 staged wake probe 前移到 boot 后 `0/800/1600 ms` 仍然没有抓到有效样本；`rom_events.detected=false`。
- `reports/20260306_235009/summary.json`
  - 在同样的早期窗口上改走 `recover --recover if-rom`，依旧 `detected=false`、`flash_attempted=false`、`flash_done=false`。
- `reports/20260307_001256/summary.json`
  - 第一次加入“盲打 ROM 入口”诊断时，`monitor` 首轮 `--reset` 自身失败；因此没有采到有效 monitor 证据，但 flash 已成功。
- `reports/20260307_002008/summary.json`
  - 在修复 `monitor` 自动回退后重新执行 `recover --recover if-rom`，仍然 `detected=false`、`flash_attempted=false`、`flash_done=false`。
- `.mcu-agentd/monitor/esp/20260306_234613_combined.mon.ndjson`
  - `0x0B` 在 boot 后 `10 ms` 就已经呈现稳定模式：标准 SBS/MAC 命令写全部 `i2c_nack_data`，裸读 `raw_read1/raw_read2` 却返回 `0xFF`。
  - `0x16` 则在同一早期窗口内，标准 word、MAC、裸读全部 `i2c_nack_addr`。
- `.mcu-agentd/monitor/esp/20260307_001908_combined.mon.ndjson`
  - 即使在 `probe_rom_exit` 读签名失败后，继续主动发送 `0x0F00` / `0x0033`（含 PEC 变体）试探 ROM 入口：
    - `0x0B` 上四种 ROM 入口写法全部 `write_err=i2c_nack_data`；
    - `0x16` 上四种 ROM 入口写法全部 `write_err=i2c_nack_addr`；
    - 之后总线指纹没有变化，也没有出现 `rom_mode_detected_after_enter`。
- 这组证据说明：
  - `0x0B` 的异常并不是“30 秒后才错过了唤醒窗口”；即使在 boot 后首个 `0~1600 ms` staged probe 窗口内，它也仍然只是命令字节拒绝 + 裸读 `0xFF`；
  - `0x16` 在早期窗口与后续重探里都完全没有地址级应答；
  - 就连主动 `0x0F00/0x0033` 试探也无法把设备拉进可见 ROM，这更符合“半烧录后落入非标准阻断态/伪应答态”，而不是单纯未触发 ROM；
  - 当前既不像正常固件态，也不像 TI ROM `0x9002` 特征态。

- 若 `dual-diag + force-min-charge + probe-mode mac-only` 仍然满足以下组合：
  - `0x0B` 的 word/MAC 写入始终是 `i2c_nack_data`
  - `0x16` 始终是 `i2c_nack_addr`
  - 没有 `stage=rom_mode_detected`
  - `addr_scan_miss` 持续出现
- 则当前应判为：
  - canonical 地址线上“有某种 ACK 行为”，但 BQ40 既没有进入正常 SBS 通信，也没有呈现 TI ROM 特征；
  - 不得升级为 `--recover force`，应停在阻断态并保留 monitor 证据。

### 8.2 无电池样本的补充收敛结论

- 经过 wake-window 误判修复后，新芯片样本已经证明：
  - 在 **无电池 + 最小充电偏置** 条件下，`0x0B` 可以出现 TI ROM 签名 `0x9002`；
  - 工具可完成 `ROM 检测 -> 刷写 -> Execute`；
  - Execute 后 `RSOC` 可从 `0x9002` 变为 `0x0000`，说明“卡在 ROM/烧录中断”已不再是主矛盾。
- 对应 monitor 证据可参考本地 `tools/bq40-comm-tool/.mcu-agentd/monitor/esp/` 下的以下日志：
  - `20260309_new_chip_after_wake_fix2_95s.log`
  - `20260309_new_chip_full_info_95s.log`
  - `20260309_new_chip_post_flash_mfg_enable_95s.log`
- 这组样本刷后仍然表现为：
  - `Temperature()` 正常（约 `25~27 °C`）；
  - `RelativeStateOfCharge() = 0`；
  - `Voltage()` 仅为几 mV / 几十 mV；
  - `BatteryStatus() = 0x4AD0`；
  - fallback 地址 `0x16` 持续 `i2c_nack_addr`。
- 在补充 `CellVoltage1..4()` 诊断后，这类“无电池 + 最小充电偏置”的悬空样本还呈现出稳定特征：
  - `CellVoltage1()` 约 `27~51 mV`，多数样本在 `50 mV` 左右；
  - `CellVoltage2() / CellVoltage3() / CellVoltage4()` 均为 `0 mV`；
  - 其量级与 `Voltage()` 的 `27~51 mV` 浮动一致，应视为悬空偏置签名，而不是真实 cell stack 电压。
- 因而应作如下区分：
  - 若目的是验证 **ROM/重刷链路**，上述结果已经可以视为“工具与流程有效”；
  - 若目的是验证 **正常应用态通信恢复**，上述结果仍应判为失败，因为它没有恢复到可信的 SBS 快照。
- 本轮还补出了一条工具经验：
  - 仅凭 `RSOC <= 100` 且 `Temperature()` 看似正常，不能判定 BQ40 已恢复工作；
  - wake-window 必须再用完整快照确认 `Voltage()/Current()/SOC/Status` 合法，才能把设备标记为 working。
- 对 `post_flash_mfg_status` / `post_flash_op_status` 若读到全 `0xFF`，暂不应据此直接下 `EMSHUT` 或 `GAUGE_EN/FET_EN` 结论；这类全 `1` 回包更像原始 block 解析噪声，需要更严格的 MAC 回包校验后再使用。
- 实操口径上，可直接采用以下判断：
  - **无电池**：只验证 `ROM/重刷链路`，不把刷后“几十 mV 应用态”误判为恢复成功；该条件下 `report_parser.py` 会把 `Voltage()<2500mV` 的样本判为 invalid，因此 `summary.json` 的 `verdict.pass` 预期为 FAIL（这不是回归）。
  - **带电池**：再去验收 `diagnose + verify` 是否真正 PASS。
- 若同一块板在更换 BQ40Z50 芯片后，`tools/bq40-comm-tool` 已能稳定完成 `ROM 检测 -> 重刷 -> 退出 ROM`，而旧芯片仍持续表现为既非正常 SBS、也非可见 ROM 的阻断态，则应优先把旧芯片判为 **疑似硬损坏样本**。此时软件任务的收口口径应是“工具已能区分工具链问题与芯片硬故障”，而不是继续要求软件去“修活”损坏器件。
