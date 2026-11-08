# BQ40 容量模型配置应用记录

## 结果与范围

2026-11-08，在绑定设备 `serial-04f3bb3f5367`（设备身份 `mains-aegis-198840`）通过 Mains Aegis CLI/devd 应用 `bq40-capacity-model`。恢复正常固件后，两轮 fresh 诊断独立确认以下配置。

| 字段 | DF 地址 | 应用前 | 应用后 |
| --- | --- | --- | --- |
| DesignCapacity | `0x48E5` | 4400mAh | 5000mAh |
| IT Term Voltage | `0x484A` | 9000mV | 11200mV（4S × 2.8V） |

Term Voltage 用于容量模型的放电仿真终点，本次未修改电芯欠压保护。真实 taper 读回仍为 120mA。本次不清理学习历史、不修改 ChemID、校准、保护阈值或均衡算法。

正式配置约定见[项目配置清单](../battery-configuration.md)，电芯与电池组信息见[电池组文档](../battery-pack.md)。本记录与正式配置文档分开保存。

## 应用过程及失败记录

初次操作未获得完整可验收日志；随后捕获的一次操作在 `0x4840` 写后验证链路报 `write_transport`，工具停止且充电保持禁用。这不能解释为字段未写入：恢复正常固件后的独立读回已确认两个目标值实际生效。失败时缺少底层错误细节，不能唯一归因为某个通信时序问题。

容量维护路径随后增加 MB44 请求后 22ms 等待、字段写入后 66ms 等待，并补充底层传输错误日志。该调整仅作用于容量维护镜像。修正后的受控操作取得完整成功审计；这证明此次操作通过，不能证明原始失败的唯一原因。没有执行整块覆盖回滚。

成功审计依次包含：CE 高与充电禁用、Texas/BQ40 身份、Full Access、BQ40Z50-R2 固件身份、4S 与 `CELL_TERM=0` 前提、两块写前快照、两字段地址/长度/原始值/解码回显、两块最终读回、`done fields=2 blocks=2` 及禁充完成停机。严格解析器判定 `pass=true`。

成功审计开始时两个字段已是目标值，因此此次受控操作属于重复写入与验证。`0x48E0`、`0x4840` 两个 32 字节块在此次操作前后完全一致；该证据不能追溯证明早先首次修改时所有其他字节都未变化。定向写入白名单与软件测试约束其他字段不可写。

## 学习数据与当前状态

应用前和恢复后两轮读回中，以下值均一致：Qmax Cell 为 `[4182, 4129, 4210, 4163]`mAh，Qmax Pack 为 4129mAh，Qmax 更新计数为 5，Ra 更新计数为 23，Update Status 为 `0x0E`，历史 Current at EoC 为 201mA。没有发出学习重置命令。这些读回不等于导出了全部 Ra 表和校准区的逐字节一致性证明，也不表示 FCC 会立即变为 5000mAh。

现有容量过充保护 OC 仍置位：`SafetyStatus=0x00400000`、`XCHG=true`、充电 FET 关闭，充电器 `enabled=false`、`not_charging`；PF 为 0。本次未清除保护、未发送 START。正常固件重启后 `manual_stop_inhibit` 从 true 变为 false，不能宣称原先软件 STOP 锁跨刷机保留；当前禁充由实际保护状态约束。

当前处于禁充阶段，充电器寄存器读回 `ITERM=120mA`、`EN_TERM=false`。这不构成正常充电阶段 80mA 硬件终止的验收证据。本次容量配置应用通过，不代表原充电计划 A3/A4 的完整充电与终止验收通过。

制造诊断包在应用前后均存在 `gauge_status3_raw: invalid_response`，因此该包整体 `ok=false`。上述实际配置字段有独立有效读回；未将缺失寄存器解码为有效数据。

## 固件与可复核证据

维护镜像 build ID 为 `e24455e8-dirty-c7c5e027b975230b`，ELF SHA256 为 `ea3049b5e3742b28249681643bfef4e2452f26a0869b36bce84b70a1085418b7`。随后通过 devd artifact 选择、dry-run、real flash 恢复正常固件 `7893fcad-clean-9b267bf06a67e379`，设备身份再次核验一致。恢复的是既有正常固件，本次维护镜像不留在设备上。

- [配置前后读回、固件身份与审计结果](bq40-capacity-configuration-application.json)。
- [完整成功写入审计](bq40-capacity-configuration-application-audit.ndjson)。
- [失败操作审计](bq40-capacity-configuration-application-failed-audit.ndjson)。

容量 profile 的 17 项 Rust 测试、9 项容量审计解析测试及 profile shell 合同检查通过；修正后的维护镜像构建通过。此前完整 `just check` 通过；最终通信等待调整后重新运行了相关 profile 检查和维护镜像构建。

可从仓库根目录离线复核成功审计：

```bash
python3 tools/bq40-comm-tool/bin/report_parser.py \
  --action verify --mode canonical --duration-sec 120 \
  --repair-profile bq40-capacity-model \
  --monitor-file docs/reports/bq40-capacity-configuration-application-audit.ndjson \
  --report-out .tmp/capacity-audit-recheck
```
