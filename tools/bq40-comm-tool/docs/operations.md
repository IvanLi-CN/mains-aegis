# Operations Runbook

## 1) Canonical diagnose (default safe path)

No BQ ROM write is allowed in diagnose mode. This is the required first step in the supported workflow and should use the proven wake profile on a no-pack bench.

```bash
cd /Users/ivan/Projects/Ivan/mains-aegis/tools/bq40-comm-tool
./bin/run.sh diagnose --mode canonical --duration-sec 120 --force-min-charge true
```

Expected:
- build + flash + monitor + report complete
- `summary.json` produced
- `verdict.pass=true` when `max_valid_streak >= 10`
- command exits non-zero when `verdict.pass=false` (CI/automation friendly)
- `--recover` is rejected in this mode (to avoid silent overrides)
- `--force-min-charge true` applies the supported wake profile: `VREG=16.8V / ICHG=200mA / IINDPM=500mA`
- `tps_sync` unavailable only emits warning; output self-test still proceeds
- `--duration-sec` must satisfy the tool-derived minimum for diagnose: `>=30s` without wake, or `>=42s` with `--force-min-charge true` (10s repower-off + 2s settle + 12s startup-to-first-sample + 9 more samples at the 2s successful-poll cadence)

## 1.1) Deep diagnostic fallback (no ROM write)

Use this only when canonical diagnose still returns zero samples and you need a stronger signal about SMBus liveness without escalating to ROM write.

```bash
./bin/run.sh diagnose --mode dual-diag --duration-sec 120 --force-min-charge true --probe-mode mac-only
```

What to inspect:
- `probe_mode=mac_only` should appear in the log
- `addr=0x0B` returning `write=i2c_nack_data` means something still ACKs the canonical address but rejects every command byte
- `addr=0x16` staying `write=i2c_nack_addr` means the fallback/RAM/ROM address never came alive
- if both happen together and no `stage=rom_mode_detected` appears, stop and report a blocked state instead of escalating to `force`

## 2) Recover (state-changing path)

Only run recover after canonical diagnose fails and the monitor log proves `stage=rom_mode_detected`. The supported repo workflow is `dual-diag + if-rom + force-min-charge + asset-df-mainboard`; do not escalate to `force` when ROM signature is absent.

```bash
./bin/run.sh recover --mode dual-diag --recover if-rom --force-min-charge true --rom-image r2 --repair-profile asset-df-mainboard
```

Policy:
- `--duration-sec` must be `>=30s` for `recover --recover never`, or `>=42s` when that path is combined with `--force-min-charge true` (same floor logic as diagnose)
- `--duration-sec` must be `>=` the tool-derived minimum for `recover --recover if-rom|force` (omit `--duration-sec` to auto-select; for example `--force-min-charge true --rom-image r2` currently computes `118s`, while the script still keeps the safer historical default of `155s` when the option is omitted)
- `--recover never`: disable ROM recovery (no state-changing ROM write)
- `--recover if-rom`: recover only when ROM signature is detected
- `--recover force`: debug-only escape hatch; not part of the supported repo recovery sequence
- `--rom-image r2|r3|r5`: select the ROM recovery image explicitly when the bench target is not the default R2 pack
- `--repair-profile asset-df-mainboard`: use the official TI `section1.bin` as the DF base, then apply this board's fixed DF overrides before ROM flash

Mainboard policy:
- `asset-df-mainboard` is the supported board-repair path for this repository
- it does not depend on live MB44 DF capture, so a chip that falls back to TI stock DF or rejects live capture can still be repaired onto the board's 4S baseline
- when the pack still answers MB44 in app mode, the tool preserves live `CELL_GAIN` / `PACK_GAIN` / `BAT_GAIN` on top of the official asset base only if all three words are captured; otherwise it falls back to the asset defaults instead of flashing a mixed live/default calibration set
- it is intentionally different from "writing TI default DF fields"; the tool writes an official DF section base plus project-specific overrides
- current fixed mainboard overrides include:
  - `OCC1=4500mA/6s`
  - `OCC2=5200mA/3s`
  - `SOCC=6000mA/5s`
  - `OCD1=-14500mA/6s`
  - `OCD2=-15000mA/3s`
  - `OCD recovery=100mA/3s`
  - `SOCD=-16000mA/5s`
  - `FET Options=0x19` (`PCHG_COMM=1`, PRECHARGE uses CHG FET)
  - `Protection Configuration=0x00` (`CUV_RECOV_CHG=0`)
  - `CUV Recovery=2550mV`
  - `Balancing Configuration=0x07` (`CB=1 / CBM=1 / CBR=1 / CBS=0`)
  - `Min Start Balance Delta=3mV`
  - `Relax Balance Interval=18000s`
  - `Min RSOC for Balancing=80%`

## 2.1) BQ40 charge-temperature profile apply (app-mode, maintenance-only path)

Use this only when canonical/app-mode communication already passes and an explicitly
authorized maintenance window is available. This command is not part of normal runtime
charging. Its profile-specific maintenance image performs one explicit startup maintenance
apply after the strict identity probe; ordinary BMS discovery and polling never write Data Flash.

```bash
./bin/run.sh apply-df --mode canonical --duration-sec 120 --force-min-charge false --probe-mode strict --repair-profile bq40-charge-temperature
```

Policy:
- `apply-df` is app-mode only; it rejects `--recover` and `--rom-image`
- `--repair-profile bq40-charge-temperature` is mandatory for this subcommand
- the profile is canonical-only; `dual-diag` is diagnostic/recovery-only and is rejected before build, write, or offline verification
- the profile requires the existing strict BQ40 identity probe (`ManufacturerName()` containing `Texas` and `DeviceName()` containing `BQ40`); `--probe-mode mac-only` is diagnostic-only and is rejected
- the maintenance image repeats the identity read immediately before the Data Flash writer, so a numeric-only wake-window hit cannot authorize a write
- the writer is armed only by the explicit `bms-charge-temperature-apply` maintenance feature; the shared profile implementation by itself is inert
- low-level `bin/build.sh` calls must pass `--action apply-df` with this profile; `bin/run.sh apply-df` supplies the action, while diagnostic/recovery builds do not compile the writer
- the audit stream must prove the ordered `CE high/charger disabled`, dual identity, and Full Access gates before snapshot/write evidence can pass offline verification
- the writer first snapshots the complete containing block at `0x4A00`, length `32`
- exactly seven temperature fields are allowed: T1, T2, T5, T6, T3, T4, and hysteresis
- each MB44 write must pass echo address, returned length, raw vector, and decoded value checks
- any transport, write, address, length, raw, or decoded mismatch stops the operation immediately
- the monitor log must contain `bms_df_apply: ... profile=bq40_charge_temperature stage=done fields=7` with 0x4A00/32-byte pre- and post-write metadata
- the monitor log must contain one `stage=snapshot_before_hex` event and one `stage=final_after_hex` event, each with all 32 bytes of the containing block
- the monitor log must contain all seven exact `stage=field_verify` records with the observed per-write echo address, declared/payload length, raw bytes, and decoded value; the parser rejects missing, inconsistent, wrong-address, bad-length, unknown-stage, or reordered evidence
- offline `report.sh` parsing keeps the same provenance boundary as `run.sh`: `apply-df` evidence requires `--probe-mode strict --rom-image r2`, while `verify` rejects live-run provenance flags
- the monitor log must also contain the writer-emitted `stage=provenance probe_mode=strict rom_image=r2 mode=canonical` record; ROM/recovery events conflict with the profile and fail verification
- the Full Access evidence must contain the BQ40 operation-status Full Access bits, not only the decoded `sec=full_access` label
- any profile failure latches the temporary maintenance image fail-closed across transport loss; rerun the explicit maintenance command to establish a new retry boundary
- `summary.json` records the outcome under `charge_temperature_apply`, including the complete pre-write and post-write blocks and verified field names for offline rollback/audit
- the profile does not write ChemID, calibration, protection, termination, precharge, maintenance, or current/voltage manifest fields
- the profile image cannot be combined with ROM recovery, non-R2 ROM images, AFE repair, or asset repair features; offline verification accepts the profile only for `apply-df` or `verify` actions

Locked vectors are the little-endian 0.1 K values `AC 0A`, `42 0B`, `74 0B`, `0A 0C`,
`3C 0C`, `6E 0C`, and `14 00`. The static current/voltage manifest is 100mA/16800mV
for LT and 500mA/16800mV for STL/RT/STH/HT; it is a safety/reference input, not a
Data Flash writer field.

The retired `live-df-mainboard` argument is rejected with an explicit migration error.
Do not replace it with a broad fallback or a 25-field write set.

## 3) Verify (offline)

```bash
./bin/run.sh verify --mode canonical --duration-sec 120 --repair-profile bq40-charge-temperature --monitor-file /abs/path/to/profile.mon.ndjson
```

Notes:
- `verify` is offline-only and does not accept `--flash` or `--recover`; pass the same explicit `--repair-profile bq40-charge-temperature` when verifying a profile apply log so the full seven-field evidence contract is enforced.

## 4) Supported sequence

1. `diagnose --mode canonical --force-min-charge false`
2. If canonical diagnose passes and an authorized maintenance window needs the locked charge-temperature profile, run `apply-df --mode canonical --force-min-charge false --probe-mode strict --repair-profile bq40-charge-temperature`
3. If the log contains `stage=rom_mode_detected`, run `recover --mode dual-diag --recover if-rom --force-min-charge true --repair-profile asset-df-mainboard`
4. Re-flash canonical firmware and re-run `diagnose --mode canonical --force-min-charge false`
5. Run `verify --mode canonical --monitor-file <canonical log>` on the final canonical log


## 5) Scenario checklist

- A: BQ40 disconnected -> stable `i2c_nack`, `verdict.pass=false`
- B: BQ40 connected + diagnose -> report with categorized poll errors
- C: ROM signature `0x9002` + recover -> `rom_events` reflects recovery stages
- D: charger/IRQ disturbance -> still hit `max_valid_streak >= 10`
- E: canonical mode -> no `addr=0x16` in monitor log
- F: `verify` over same log -> reproducible summary

## 6) Troubleshooting

- Symptom: `BQ40_TOOL_DEVICE_ID` or `BQ40_TOOL_ARTIFACT_MANIFEST_PATH` is missing.
- Check:

```bash
echo "$BQ40_TOOL_DEVD_URL"
echo "$BQ40_TOOL_DEVICE_ID"
ls -l /path/to/manifest.json
```

- Symptom: devd bridge is unreachable or returns an error.
- Check:

```bash
curl -fsS "$BQ40_TOOL_DEVD_URL/health"
```

Keep the devd bridge running in one terminal, then execute tool commands in another terminal.

## 充电终止配置

此操作只适用于已绑定且核验为 Texas BQ40Z50-R2 的目标。需要明确的 DF 写入与维护镜像刷写授权。必须先只读核验 `ChgCurrentThreshold=50mA`（`0x4A8B`）、`QuitCurrent=10mA`（`0x4A8D`）、`FET Options[CHGFET]=0`、`SBS Gauging Configuration[RSOCL]=1` 与 `IT Gauging Configuration[CSYNC]=1`；任何不符均停止，不顺带修正其他字段。`Current at EoC` 为历史状态，不参与写入或 ITERM 推导。

1. 通过仓库 devd CLI 连接原有绑定设备，保存身份和 fresh 诊断；禁止枚举或改用其他端口。
2. `just bq40-termination-artifact` 只构建 canonical/R2、禁止 ROM recovery 的 taper-only 镜像。目录内 catalog manifest 是选择依据，不直接调用 espflash。
3. 使用 `mains-aegis device <id> artifact select --manifest-path <manifest>`、`flash --dry-run`，审核目标、ELF 与校验和；再执行已授权的 `flash --real`。
4. `mains-aegis device <id> monitor start --maintenance-profile bq40-charge-termination` 收集维护 defmt 日志。产品身份缺失时日志保持未验证标记，必须关联先前绑定、所选镜像与 flash receipt。writer 先强制 CE 高并禁用 charger，核验身份、相关配置和 Full Access，保存完整块后仅写 `0x4A6D`；校验地址、长度、raw、解码和最终块，非 taper 字节必须不变。失败立即停止，保存日志，不自动整块覆盖回滚。
5. 离线 parser 使用 `--repair-profile bq40-charge-termination`，要求唯一字段、`writes=1`、前后完整块和身份/权限证据；混用温度 profile 或其他写入均拒绝成功。共享报告结构的 `charge_temperature_apply` 键为兼容保留，必须同时核对报告请求的 repair_profile 名称。
6. 停止维护 monitor，重新选择并 dry-run/flash 正式候选固件，核验产品身份与版本。再次 fresh 读回 taper=120mA；稳定普通充电应读回 ITERM=80mA、EN_TERM=1。100mA 与有界 200mA 维护唤醒禁用 EN_TERM，启动爬坡也暂禁终止。

稳定性验证每 2 秒采样，共 10 分钟，记录 fresh Current、AverageCurrent、独立采样时间的均衡 mask、保护、VCT/FC 和 charger_done。随后自然充电观察最长 12 小时，终止后继续 10 分钟。零请求和保护优先，硬件先终止但 BQ40 无本轮资格时锁存 termination_incomplete，不自动重新启动。未覆盖 mask、读取错误、中断和超时必须记录；未捕获完整过程不能判通过。120mA/80mA 未满足真实窗口时保留证据返回设计，不自动调参。

维护 defmt-only 镜像不消费 CDC hello/status 请求。显式维护 monitor 必须只读日志，不发送产品 CDC 轮询；CDC 请求返回 maintenance_cdc_unavailable，硬件 reset 仍由 devd 生命周期处理。R2 身份核验使用 TRM 的 DDddVVvvBBbbTTZZzzRREE 高字节顺序，DF 两字节 taper 仍为 little-endian。

## 容量模型配置

`bq40-capacity-model` 只适用于 TI BQ40Z50-R2、4S、`CELL_TERM=0` 的项目电池组。它使用与温度、taper 独立的白名单：

| 字段 | DF 地址 | 目标 | 小端字节 | 完整快照块 |
| --- | --- | --- | --- | --- |
| Design Capacity mAh | `0x48E5` | 5000mAh | `88 13` | `0x48E0`，32 字节 |
| IT Term Voltage | `0x484A` | 11200mV | `C0 2B` | `0x4840`，32 字节 |

写前验证 Texas/BQ40 身份及完整 R2 FirmwareVersion 响应、Full Access、CE 高和禁充；核对 4S 串数及整包终止模式，冲突即停止。两个快照必须均成功取得且地址／长度正确，之后才能写第一个字段。每次字段写入核验回显、长度、原始字节与解码值；全部写完后再读两个块，非白名单字节必须与写前一致。

该操作跨两个 DF 块，不具有原子性。失败立即停止，保存已取得的快照，不自动回滚、重试或清理学习；存在部分成功时必须根据原始读回判断设备状态，不能以末尾缺失的成功日志推断未发生写入。

从仓库根目录构建，不连接设备：

```bash
just bq40-capacity-artifact
```

真实应用仅在明确授权后进行：通过 `mains-aegis` CLI/devd 对既有绑定设备选择生成的 catalog manifest，执行 flash dry-run，再 flash；禁止直接烧录工具和端口试探。对维护镜像使用 `--maintenance-profile bq40-capacity-model` 启动监控。它是显式武装的一次性 writer，启动后保持禁充并只执行一次；不要把它作为普通运行固件保留。主机只允许该 profile 对应的单一 writer 特征，拒绝混合温度／taper／容量 writer。

离线核验不连接设备：

```bash
./tools/bq40-comm-tool/bin/run.sh verify --mode canonical --duration-sec 120 --repair-profile bq40-capacity-model --monitor-file /absolute/path/to/maintenance.mon.ndjson --report-out /absolute/path/to/report
```

报告解析要求同一监控会话内完整、顺序一致的安全门控、R2 身份、前置配置、两块快照、两字段回显、两块最终读回、`done fields=2 blocks=2` 与禁充 halt。拒绝读取／写入错误、缺失或重复证据、地址／长度错误、非目标字节变化、混合 profile、恢复事件、跨会话拼接与 halt 后再启充。仅允许相同时间戳和内容的 trace/log 镜像去重。

`summary.json` 的 `blocks` 按 `0x48E0`、`0x4840` 顺序记录 `snapshot_before_hex` 与 `final_after_hex`，`verified_fields` 为 `design_capacity_mah`、`term_voltage_mv`。报告通过只证明定向配置证据完整，不证明完整电池学习或自然终止已通过。

应用后经同一 devd 生命周期恢复正式候选固件，核验设备身份、固件标识以及两个字段读回，再进行正常充电验证。配置流程不自动 START，不改写 ChemID、Qmax、Ra、校准、保护或历史。`asset-df-mainboard` overlay 使用同一组容量目标；原厂二进制资产保持原样，ROM 恢复仍不作为保留学习数据的定向配置方法。
