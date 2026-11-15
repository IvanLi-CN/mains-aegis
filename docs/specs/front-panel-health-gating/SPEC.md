# 前面板健康分级与自检阻断

> 本文是前面板页面进入条件、整机健康等级与单动作阻断边界的长期需求合同。实现状态见 `IMPLEMENTATION.md`，演进记录见 `HISTORY.md`。

## Context and Scope

- Context: 将启动自检、致命阻断、降级运行和单动作阻断分开，避免可继续安全运行的 UPS 因低电量或充电异常被锁在自检页。
- In scope: 前面板页面状态、Fatal/Degraded/Action block 分类、Dashboard eligibility、运行中页面转场和手动充电入口边界。
- Out of scope: BQ40 数据字段定义、充电器具体电流策略、USB-C PD 协议协商和 Dashboard 视觉布局。

## Terms and Interfaces

- `SelfCheckBoot`: 上电或复位期间展示自检进度的页面阶段。
- `Fatal block`: 当前被请求的 UPS 核心能力无法安全提供，必须阻断普通业务操作。
- `Degraded runtime`: 核心能力仍可安全提供，但某项能力受限；保持用户当前页面并显示局部告警。
- `Action block`: 仅阻止一个具体动作，例如手动充电 START，不阻止其它安全交互。
- `DashboardRoute`: Dashboard 首页、详情页和手动充电页等用户当前所在的业务路由。

## Requirements

### REQ-FPG-001

- 系统 MUST 在每次上电或复位后先展示 `SelfCheckBoot` 并实时更新自检快照。
- 自检快照完成后，若不存在 Fatal block，系统 MUST 自动进入 Dashboard 默认入口；若存在 Fatal block，系统 MUST 停留在 SELF CHECK/BLOCKED。
- covers: `VER-FPG-001`

### REQ-FPG-002

- Fatal block MUST 表示当前被请求的核心能力无法安全提供。
- BQ40 完全离线、被请求的 TPS 输出未 active、`requested_outputs` 与 `active_outputs` 不一致，以及关键保护或输出控制链路失效 MUST 视为 Fatal。
- 任意单个器件异常 MUST NOT 仅凭器件名称自动升级为 Fatal；判定必须基于当前核心能力。
- covers: `VER-FPG-002`

### REQ-FPG-003

- BQ40 在线且确认电池存在时，`RCA`、低电量、低电芯、`XCHG`、`CHG_FET_OFF` 等可解析异常 MUST 归类为 Degraded runtime，除非同时满足其它 Fatal 条件。
- 充电器单独不可用、USB-C 输入路径不可用但 DCIN 可用等局部能力异常 MUST 归类为 Degraded runtime，除非该能力是当前不可替代的核心安全链路。
- covers: `VER-FPG-003`

### REQ-FPG-004

- Degraded runtime 状态变化 MUST 保持当前 `DashboardRoute`，不得强制跳转到 Dashboard 首页或 SELF CHECK。
- 当前页面 MUST 在对应区域显示明确的 `WARN`、`RCA ALARM`、`CHG BLOCKED`、`INPUT UNAVAILABLE` 或 `N/A` 等状态及直接证据。
- covers: `VER-FPG-004`

### REQ-FPG-005

- Action block MUST 只阻止被拒绝的具体动作，并保留其它安全的只读或业务交互。
- Degraded runtime 下 MUST 允许进入手动充电页查看 readiness、阻断原因和直接证据；START 仍 MUST 由固件硬安全门最终裁决。
- Fatal block 下 MUST 禁止手动充电、输出开关和其它普通业务控制。
- covers: `VER-FPG-005`

### REQ-FPG-006

- 运行中出现 Fatal 时，系统 MUST 切换到 SELF CHECK/BLOCKED 并停止普通业务控制。
- Fatal 恢复后，系统 MUST 重新生成完整自检快照并通过与启动相同的 eligibility 检查；只有重新满足条件时才允许回到 Dashboard 默认入口。
- 看门狗、异常复位和启动计数 MUST 只作为告警证据，不能单独导致 SELF CHECK 常驻。
- covers: `VER-FPG-006`

## Verification

### VER-FPG-001

- Method: 前面板状态机测试与启动快照 fixture。
- covers: `REQ-FPG-001`
- Pass condition: 无 Fatal 的启动快照进入 Dashboard；含 Fatal 的启动快照停留 SELF CHECK/BLOCKED。

### VER-FPG-002

- Method: 用 BQ40 离线、TPS 请求/active 不一致、保护链路失效和单器件局部异常构造健康分级 fixture。
- covers: `REQ-FPG-002`
- Pass condition: 只有核心能力不可安全提供的 fixture 被判定为 Fatal；`requested_outputs != active_outputs` 始终为 Fatal。

### VER-FPG-003

- Method: 用 BQ40 在线 `RCA`、低电量、低电芯、`XCHG`、`CHG_FET_OFF` 与充电器单独不可用 fixture 验证分类。
- covers: `REQ-FPG-003`
- Pass condition: 这些状态在不存在其它 Fatal 条件时均为 Degraded runtime。

### VER-FPG-004

- Method: 在首页、详情页和手动充电页注入 Degraded 状态变化。
- covers: `REQ-FPG-004`
- Pass condition: `DashboardRoute` 保持不变，局部状态和直接证据更新。

### VER-FPG-005

- Method: 通过 charge-control preview/START fixture 验证 Degraded 与 Fatal 的入口边界。
- covers: `REQ-FPG-005`
- Pass condition: Degraded 可查看手动充电 readiness 但 START 受硬门禁裁决；Fatal 不提供普通业务控制。

### VER-FPG-006

- Method: 运行态 Fatal 注入、恢复动作和 watchdog reset fixture。
- covers: `REQ-FPG-006`
- Pass condition: Fatal 立即进入阻断；恢复必须重新通过 eligibility；单独 watchdog 原因不导致常驻阻断。

## Related ADRs

- [ADR 0004: 按核心能力分级前面板阻断](../../adr/0004-front-panel-health-gating.md)

## Visual Evidence

- None

## References

- [Dashboard live after self-check](../dashboard-live-after-self-check/SPEC.md)
- [BQ40 self-check result dialogs](../bq40-self-check-result-dialogs/SPEC.md)
- [Runtime mode switching](../runtime-mode-switching/SPEC.md)
