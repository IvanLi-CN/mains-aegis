# 前面板工业仪表风 UI + 1:1 预览 演进历史

> 这里记录影响当前规范理解的关键演进；当前有效合同仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-02-26: 新建规格并完成第一轮实现同步（共享渲染 + 预览工具 + 文档更新）。
- 2026-02-26: 根据评审反馈将 Dashboard 文案收敛到项目功能域，并固定 A/B 字体分工（标签 A，数值 B）。
- 2026-02-26: 根据评审反馈将默认 Dashboard 切换为 Variant B，并将 Variant C 重定位为自检页视觉。
- 2026-02-26: 冻结 Variant B Dashboard 间距与留白（主 KPI 面板 + 次级面板行距），并归档 AC/BATT 参考图。
- 2026-02-27: 修正 AC 电流语义（`ICHG BAT` 不再混同为线侧输入电流），并移除 BATT 模式次级面板中的 `POUT` 重复字段。
- 2026-02-27: 按 UPS 四工作模式重构 Dashboard 语义：新增 `UpsMode` 并归档四张冻结参考图。
- 2026-02-27: 模式命名统一升级为专业全称（`BYPASS / STANDBY / ASSIST / BACKUP`），界面右上状态位不再使用缩写。
- 2026-02-27: 自检页升级为“全模块通信诊断表”（10 模块覆盖），并新增长按 CENTER 页面切换（Dashboard <-> Self-check）。
- 2026-02-27: 自检页再优化为“双列大字号诊断卡”，提升小屏可读性并保留全模块覆盖。
- 2026-03-01: 运行语义迁移到 `mcu-self-check-live-panel`（自检期间实时刷新 + 自检后常驻 + 禁用 CENTER 切页）；本规格保留为视觉基线。
- 2026-06-07: 新增 dashboard/menu 纵向 stack、menu icon rail 与 beeper volume setting 的 preview gate 资产；在主人确认预览前不推进运行态导航与音量持久化实现。
- 2026-06-08: 将 `AUDIO` 设置页升级为 `ACTION / SYSTEM` 双分组布局，冻结共享刻度、双轨道与分组高亮的预览口径。
- 2026-06-08: 将 `AUDIO` 设置页静音档词形收敛为“共享刻度 `0`、右侧 value badge `OFF`”，避免上方刻度与右侧读数语义混淆。
- 2026-06-08: 主人确认 `ACTION / SYSTEM` 双分组预览口径后，运行态导航与试听接线已转入 `main-firmware-runtime-audio-cues`；本规格继续只保留视觉基线。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
