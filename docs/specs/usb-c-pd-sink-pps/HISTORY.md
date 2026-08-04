# USB-C PD/PPS Sink 首阶段实现 演进历史

> 这里记录影响当前规范理解的关键演进；当前有效合同仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-04-26: 修复 `MCU reset + inherited attach` 仍可能掉进无限重启的回归。实机日志显示 source 在继承 attach 的 `default_5v` fallback 期仍可能主动发出 `peer hard reset`；旧实现把这类协议级 reset 当作需要整段清空 no-contract fallback，导致 `charge_ready/vindpm/iindpm` 出现空窗。当前改为在 inherited attach 已进入安全 fallback 时保留该 fallback，仅重启非破坏式 `wait for caps` 恢复梯子，避免在无电池兜底时被 source 的协议 reset 拉进复位环。
- 2026-04-23: 完成最终 hotplug PPS 恢复收口。根因最终确认还包括 `attached && contract=None` 窗口里 `usb_pd.tick()` 服务频率不足，导致恢复超时被主循环其它任务拖长；通过补齐 partial-RX / hard-reset 恢复正确性，并在主循环中为 no-contract 协商增加优先窗口后，reset 基线已稳定到约 `1.67s`，实机热插拔也恢复到秒级 `PPS`。
- 2026-04-22: 完成 hotplug PPS 恢复闭环。最终根因定位为 FUSB302 自动协议复位与固件恢复状态机互相打架、fresh attach 后继续处理旧 IRQ snapshot，以及 `missing source caps` 恢复策略缺少稳定升级路径；修复后实机热插拔 `1.0s` 内恢复到 `PPS`，冷启动插线基线约 `25.28s` 自动恢复到 `PPS`。
- 2026-04-22: 重新打开 hotplug PPS 恢复问题。此前“热插拔已稳定恢复到 `PPS`”的结论被后续实机复测推翻：当前同一条 PPS 电源线上仍会出现“有时数秒恢复、有时长期卡在 `CAP?`”的双稳态现象；规格状态回退为 `部分完成（4/5）`，后续必须先完成稳定恢复闭环，再讨论时延优化。
- 2026-04-21: 一度观察到连续多次实机拔插可自动回到 `PPS`，后续实现/回归证明该结论不足以支撑 closeout；该记录保留为阶段性现象，不再视为最终结论。
- 2026-04-08: 已继续收敛 merge-proof review，补齐“非充电态仍计入系统负载预算”“PD state 先于 charger tick 生效”“合同丢失时强制恢复旧 `VINDPM/IINDPM`”三项修正，并同步规格说明。
- 2026-04-08: 已根据 merge-proof review 修正 spec revision 跟随、无可用 PD 合同时的稳定 5V 回落，以及 WAIT/REJECT 后的旧合同 charge gate 恢复；规格与最新实现重新对齐。
- 2026-04-08: 已同步 host-unit-tests allowlist 与 closeout 文档，确认 `usb_pd` 模块测试覆盖纳入 host audit，规格与实现重新对齐为 merge-ready。
- 2026-04-08: 已完成默认全开 + blacklist feature、USB-C 协商/重协商禁充 gate、PPS keep-alive、合同保持与真机验证；状态更新为 `已完成`。
- 2026-04-08: 规格同步到默认全开 + blacklist feature 口径，并补充“USB-C 协商/重协商期间禁充，输入稳定后再恢复”的 charge gate 要求与验收项。
- 2026-04-07: PR #62 已创建，收口目标切换为 review-loop 后的可审阅态；台架风险保持显式记录。
- 2026-04-07: 已完成 `usb_pd` 模块、I2C2 共享、`BQ25792` 输入限制 helper、主循环/`PowerManager` 接线，以及 host-unit-tests + feature matrix 本地验证；状态更新为 `部分完成（4/5）`，等待 PR/review-loop 收口。
- 2026-04-07: 初版规格创建，冻结 USB-C PD/PPS sink v1 的范围、feature、边界与验收标准。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
