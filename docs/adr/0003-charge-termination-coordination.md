# ADR 0003: 协调 BQ40 满充资格与 BQ25792 硬件终止

## 状态

已接受；取代 [ADR 0002](0002-bq40-charge-temperature-profile.md)。120mA/80mA 是待实机验收的候选组合，不代表已完成电池验证。

## 背景

普通充电在 SOC99/HV 时降到 200mA，又从历史 Current at EoC=201mA 推导 200mA 终止目标，触发目标必须严格大于终止电流的门控，导致分区变化时反复启停。Current at EoC 是状态历史，不能代替真实 Charge Term Taper Current 配置。BQ40 与充电器还有各自的终止窗口，FC 单独出现不能证明充电器已完成终止。

## 决策

- 保留 ADR 0002 的温度窗口、100mA/500mA 目标、SBS 请求安全上限、BQ25792 JEITA、七字段温度 writer 和有界 200mA 维护唤醒规则。普通 200mA 补电分支删除。
- 正常阶段读取并核验 BQ40 Charge Term Taper Current=120mA；BQ25792 ITERM=80mA、EN_TERM=1。启动爬坡先禁终止，最终目标建立后恢复。100mA 与独立维护唤醒仍关闭 EN_TERM。
- 本轮新鲜 VCT/FC 满充资格与 charger_done 共同成立才进入 FULL。FC 本身不提前断充；零请求、温度和保护始终优先。硬件先终止且无有效资格时锁存 termination_incomplete，只有明确且安全的 START 可以重试。
- 新增独立 bq40-charge-termination profile，仅允许写 0x4A6D 两字节。身份、R2、Full Access、CE 高、充电禁用和已接受的相关配置必须先通过。保存 0x4A60 的 32 字节快照，校验回显与最终读回，并验证其他字节不变。出错即停，不自动整块回滚。普通固件不写 DF。
- asset-df-mainboard 恢复 overlay 采用同一 taper；原厂二进制不变。ChemID、Qmax、Ra、校准、保护和学习历史不变。

## 后果与验收

120mA/80mA 间隔必须通过真实电池平均电流与判定窗口验证；稳定充电与完整自然终止观察在合并后执行，绑定实际合并提交及对应固件，不作为合并前已通过证据。稳定采样每 2 秒一次、持续 10 分钟；完整充电最长观察 12 小时，终止后再观察 10 分钟。未捕获完整过程或窗口不成立时不能宣称通过，不自动调参。未出现的均衡 mask 标注未覆盖。新配置只改变资格阈值，不重置或保证修复 SOC 学习精度。

## 实现索引

- [温度与安全上限合同](../specs/bq40-charge-temperature-profile/SPEC.md)
- [终止合同](../specs/bq40-lock-root-cause/SPEC.md)
- [调查报告](../reports/charge-cycling-analysis.md)
- [维护配置操作](../../tools/bq40-comm-tool/docs/operations.md)
