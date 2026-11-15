# ADR 0004: 按核心能力分级前面板阻断

## 状态

已接受；设计合同见 [Front-panel health gating SPEC](../specs/front-panel-health-gating/SPEC.md)。

## 背景

当前固件把部分可解析的 BQ40 异常（例如 `RCA/remaining_capacity_alarm`）提升为自检页阻断，导致仍可提供 UPS 核心输出时无法进入 Dashboard，也无法访问手动充电入口。项目需要区分“整机不能安全运行”和“单项能力受限”，避免把低电量或充电策略阻断误报为整机致命故障。

## 决策

- `SELF CHECK` 在启动/复位期间展示自检过程；自检完成后，只有存在 Fatal block 才停留在自检/阻断页。
- Fatal block 表示当前被请求的核心能力无法安全提供。BQ40 完全离线、被请求的 TPS 输出未 active、请求输出集合与 active 输出集合不一致，以及关键保护或输出控制链路失效均属于 Fatal。
- Degraded runtime 表示核心功能仍可安全提供但某项能力受限。在线且确认电池存在的 BQ40 `RCA`、低电量、低电芯、`XCHG` 或 `CHG_FET_OFF` 属于 Degraded；充电器单独不可用也属于 Degraded，除非它是当前不可替代的核心安全链路。
- Action block 只阻止具体动作，例如手动充电 START，不阻止用户留在当前 DashboardRoute 上查看状态或其它可用功能。
- Degraded 状态变化保持当前页面；运行中出现 Fatal 才切换到自检/阻断页。Fatal 恢复后必须重新生成完整快照并通过同一套 eligibility 检查，才能回到 Dashboard 默认入口。
- 看门狗或异常复位原因只能作为告警证据，不能单独使设备长期停留在自检页。

## 后果

- 前面板状态、Dashboard eligibility 与 charge-control readiness 必须分别表达整机健康、页面可用性和单动作可用性。
- 需要为 Fatal、Degraded 和 Action block 提供稳定的状态词、直接证据和测试夹具。
- 当前 `bq40_dashboard_clear` 的 BQ40 WARN 白名单逻辑不足以表达该决策，需要在实现阶段按能力分级重构。

## 关联规范

- [Front-panel health gating SPEC](../specs/front-panel-health-gating/SPEC.md)
- [Dashboard live after self-check](../specs/dashboard-live-after-self-check/SPEC.md)
- [BQ40 self-check result dialogs](../specs/bq40-self-check-result-dialogs/SPEC.md)
