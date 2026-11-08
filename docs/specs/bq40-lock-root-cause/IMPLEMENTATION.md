# BQ40 `LOCK` 根因锁定与修复 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Lifecycle: active
- Implementation: 见下方迁移状态与覆盖记录。

## Migrated Implementation Record

- Status: 部分完成（3/6）
- Created: 2026-04-13
- Last: 2026-04-13

- Directory: `docs/specs/bq40-lock-root-cause/assets/`
- In-spec references: `![...](./assets/<file>.png)`
- Visual evidence source: maintain `## Visual Evidence` in this spec when owner-facing or PR-facing screenshots are needed.
- If an asset must be used in impl (runtime/test/official docs), list it in `资产晋升（Asset promotion）` and promote it to a stable project path during implementation.

None

- [x] M1: 创建 `LOCK` 根因 spec 并冻结证据模型、三分流规则与修复边界。
- [x] M2: 补齐 `ChargingStatus(0x55)` 原始块读与 lifetime/termination 观测字段，并完成本地验证。
- [ ] M3: 完成一次从 `<90%` 解锁基线开始的 live pack 闭环抓取，产出单份时间线证据包。
- [x] M4: 按证据命中唯一修复分支并完成实现。
- [ ] M5: 完成修复后的 live pack 闭环复验，证明不再重入 `OC/LOCK`。
- [ ] M6: PR 收敛、合并与最终上板确认完成。

## References

- `./SPEC.md`
- `./HISTORY.md`

## 终止与配置实现

纯策略已删除普通 200mA 补电，并覆盖 FC 先到、硬件终止先到、满充锁存和明确 START 重试。BQ25792 正常 ITERM 固定 80mA，以真实 taper=120mA 作为配置前提；历史 EOC 保留兼容字段。独立单字段 writer、最终块校验、恢复 overlay 和 parser 共用 120mA 目标。实机稳定性和完整终止验收必须另有绑定候选固件的证据，不能由单元测试代替。

维护 R2 身份门控按 TRM 的高字节格式解码 FirmwareVersion 完整响应，测试固定实机 45 00 02 11 00 34 00 04 75 02 00。显式维护 monitor 只读 defmt，不发送维护镜像无法消费的产品 CDC hello/status 轮询，CDC 请求明确拒绝，避免静默写入阻塞维护日志与停止流程。

CLI 的 device charge start|stop 映射既有 control.manual_charge IPC，以执行授权的实机完整充电，默认不确认回路风险。维护镜像在启动后保留 30 秒禁充观察期，供 USB 重新枚举后附着日志；只重试同一绑定端口，不扫描或切换端口。

安全校验通过的明确 START 建立新的策略会话，清除旧异常与资格；SOC99 的手动充电也参加联合终止。独立的旧 charger_done 不阻止异常后的 START，charger_done 只在本轮实际使能充电时消费，避免停充历史状态立即重新锁住重试。

密集诊断与充电共享执行任务；诊断造成温度缓存超过 1 秒时，在充电决策边界重新读取温度配置与 DAStatus2，读取失败保持禁充，安全有效期仍为 1 秒。诊断附带 `cell_temperature_max_c_x10`、`cell_temperature_captured_at_ms` 和 `temperature_configuration_verified`，用于区分真实温度越界与证据过期。

`bq40.charge` 提供不含 DF 与历史读取的独立实时观测包，供每两秒采样使用；完整配置在采样前后单独读回，两个包的时间和错误不混用。

实机候选 adb8d0c9 在约 500mA 充电时触发 OC 容量过充保护，未到 VCT/FC 与硬件正常终止；连续充电窗口不足 10 分钟，A3/A4 未通过。已明确 STOP，不修改保护与学习数据。配置和本地检查通过不构成经验验收通过，PR 保持 Draft；详见[分析报告](../../reports/charge-cycling-analysis.md#修正候选的实机结果)。

为核验容量保护证据，bq40.core 增加同轮 BatteryMode、RemainingCapacity 与 FullChargeCapacity 原值及由模式解码的单位；不改写容量模式、保护或学习数据。单位读取失败保持未知。

只读容量诊断进一步读取真实 OC 阈值/恢复、DesignCapacity、Qmax、ChemicalID 原始负载、GaugeStatus1/2/3，以及 IT 仿真截止电压、负载选择/模式与预留容量。每项缺失保持空值与错误，不作为 DF 写入入口；制造包在停充时单独采集。最终候选 7893fcad 连续三轮确认 TrueRemQ=3042mAh、TrueFCCQ=2624mAh、OC=300mAh，DesignCapacity=4400mAh、IT Term Voltage=9000mV 与项目配置资格未对齐。GaugeStatus3 严格长度校验失败，未将包判为完整成功；详见[容量过充诊断及边界](../../reports/bq40-oc-diagnosis.md)。
