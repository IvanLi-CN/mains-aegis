# BQ40Z50 通信故障排查笔记

## 0. 结论先行

- 本次问题不是单一“芯片坏/线坏”，而是 **供电门槛 + 协议有效性校验不足 + 工具链状态异常** 的叠加。
- 在 `tools/bq40-comm-tool` 里落实“最小充电唤醒 + canonical 地址 + 严格校验 + managerd 排障”后，历史样本曾达到验收阈值；但 2026-03-06 主人当前接入的这块板子仍处于阻断态，尚未恢复通信。

## 1. 适用范围

- 仅适用于工具目录 `tools/bq40-comm-tool` 的故障诊断与恢复流程。
- 不覆盖主固件业务逻辑，只记录通信链路排障所需信息。

## 1.1 本次排查的硬件边界条件（重要）

- 本次主要排查与最终达标样本，均在 **电池未连接** 条件下完成。
- 中途曾短时接入过电池做对照，通信指标未见改善，随后出于安全考虑移除电池继续排查。
- 因此本笔记中的结论可理解为：问题收敛关键不在“是否接电池”，而在供电唤醒、协议校验与工具链稳定性。

## 2. 证据与现象（按时间线）

### 2.1 历史故障证据（主工程早期日志）

- `/.mcu-agentd/monitor/esp/20260209_095024.mon.ndjson`
  - 现象：出现 ACK 但数据可疑常量化（典型为 `5911/5911/0x1717`）并伴随子字段 NACK。
  - 结论：链路“能回数据”不等于“数据可信”。

- `/.mcu-agentd/monitor/esp/20260209_134121.mon.ndjson`
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
   ./bin/run.sh recover --mode dual-diag --duration-sec 120 --recover if-rom --force-min-charge true
   ```
5. 恢复后必须重新刷回 canonical 并再次诊断，然后用同一份 canonical 日志做离线 verify。

## 6. ROM 模式相关结论

- `rom_events.flash_done=true` 的新口径：仅当 monitor 日志出现 `stage=probe_rom_flash_done`（recover 调用栈返回 `Ok`）才算 ROM 恢复成功。
- 若只看到 `stage=rom_flash_incomplete`，或报告为 `flash_attempted=true` 且 `flash_done=false`，表示 ROM 序列跑过但并未退出 ROM。

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
- `/.mcu-agentd/monitor/esp/20260306_234613_combined.mon.ndjson`
  - `0x0B` 在 boot 后 `10 ms` 就已经呈现稳定模式：标准 SBS/MAC 命令写全部 `i2c_nack_data`，裸读 `raw_read1/raw_read2` 却返回 `0xFF`。
  - `0x16` 则在同一早期窗口内，标准 word、MAC、裸读全部 `i2c_nack_addr`。
- `/.mcu-agentd/monitor/esp/20260307_001908_combined.mon.ndjson`
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
