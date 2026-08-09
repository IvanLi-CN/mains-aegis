# Mains Aegis Device Daemon 演进历史

> 这里记录影响当前规范理解的关键演进；当前有效合同仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-08-08: 新固件发布资产采用带显式电压后缀的稳定短文件名，芯片、profile、features、Git SHA 与源码摘要继续由 manifest 的 `artifact_id` / `build_id` 承载；catalog 仍按 `files[].path` 解析，因此历史长文件名资产无需重发或别名副本。
- 2026-08-08: `diag-snapshot` 升级为 schema v2，保留 endpoint/package id，采用逐包 fresh capture、部分失败与流式传输；Read/Clear 数据只由业务 owner 锁存，并补齐 INA3221 IRQ 到现有保护链路。
- 2026-08-09: fresh I2C diagnostics 保留 ESP HAL 的 ACK 失败阶段，区分地址、数据与未知 NACK；该信息只扩展 `read_errors.code`，不改变既有保护与重试路径的通用错误分类。
- 2026-08-09: TPS55288 的 `VREF` 读取纳入同一阶段映射，保证数据阶段 NACK 不再以通用错误呈现；同包其余寄存器继续逐项保留自己的错误阶段。
- 2026-08-09: BQ40 manufacturing 与 BQ25792 register fresh capture 开始保留底层读取错误、采集时间和耗时，避免用空字段把不完整采集误报为成功；BQ40 block 无效与 BMS 地址不可用同样作为当前请求的结构化失败，不复用旧 payload 或时间。
- 2026-08-09: 设备 LAN HTTP 的有界 request target 提升到可承载全部稳定硬件 package 的重复查询；全包诊断仍按逐包 chunked JSON 输出，不因合法 query 长度在解析阶段返回 `invalid_request`。

- 2026-06-14: `power event`、`status` 与 `diag-snapshot` 统一补充 `tps_total_iout_ma` / `tps_limit_threshold_ma`，用于解释 `pressure_tps_output_current`；DC IN profile 的 `iindpm_ma` 基线更新为 `1000mA`。
- 2026-06-04: `diag-snapshot` 增加 `charger.vbat_lowv_pct_x10`、`charger.iprechg_ma`、`policy.recovery_stage`、`bms.cuv_recovery_mv` 与 `bms.cuv_recov_chg`，支持确认 `REG08=71.4%/120mA` 与 BQ40 `2550mV + CUV_RECOV_CHG=0` baseline。
- 2026-06-04: `flash` API 与设备事件增加 backend `status/stdout/stderr` 透传，现场可直接确认 `espflash` 是否真正完成以及目标硬件 identity 是否已经切到新 artifact。
- 2026-06-04: 新增低压恢复维护 runner 与文档，固化 bq40 工具固件和主固件的双烧录验证路径。
- 2026-06-05: `/api/v1/status` 的 `battery` snapshot 增加四节 `cell_mv`、`cell_delta_mv`、均衡状态字段与 `charge_fet_on` / `discharge_fet_on` / `precharge_fet_on`，Web 电池页可直接展示 per-cell voltage、delta、BAL 状态与三路 BMS MOS 状态，不再依赖 `diag-snapshot` 详情端点。
- 2026-06-07: `devices/scan` 与 `devices` 响应中的 `binding.logical_device_id` 成为 Web 归并 USB identity-pending candidate 与 Fleet 混合视图的 canonical 键；旧绑定若缺失该字段，Connect 仍可继续显式补绑到已有 logical device。
- 2026-07-01: `diag-snapshot.bms` 增加 `OperationStatus()` raw payload、`emshut` / `pres` 解码与 EMSHUT 退出配置字段，现场可区分 `EMSHUT` 与普通 `XDSG` 阻断并确认恢复路径配置。
- 2026-07-05: 增加受限 BMS 放电授权恢复 API，覆盖固件 CDC、设备 LAN HTTP、devd HTTP 与 host cache refresh；同时补充 BQ40 AFE FET 与 charger ship-FET 诊断字段，用于解释 `pack_output_path_open` 与前面板恢复结果。
- 2026-07-20: host power 自动策略限定为 native serial monitor；BACKUP 进入切 `power_saver`、退出 `restore_previous`/`balanced`，并通过 USB CDC `set_host_power_profile` 把当前主机 profile 同步到固件 status/front-panel runtime overlay。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
