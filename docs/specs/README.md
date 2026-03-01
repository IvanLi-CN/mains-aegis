# 规格（Spec）总览

本目录用于管理工作项的规格与追踪：记录范围、验收标准、任务清单与状态，作为实现与验证的依据。

> Legacy compatibility: 旧规格仍可保留在 `docs/plan/**/PLAN.md`。新规格统一写入 `docs/specs/**/SPEC.md`。

## 目录与命名规则

- 每个规格一个目录：`docs/specs/<id>-<title>/`
- `<id>`：推荐 5 个字符 nanoId 风格（字符集：`23456789abcdefghjkmnpqrstuvwxyz`）
- `<title>`：短标题 slug（kebab-case）
- 主文档：`docs/specs/<id>-<title>/SPEC.md`

## 状态（Status）说明

仅允许使用：`待设计`、`待实现`、`跳过`、`部分完成（x/y）`、`已完成`、`作废`、`重新设计（#<id>）`。

## Index（固定表格）

| ID | Title | Status | Spec | Last | Notes |
| ---: | --- | --- | --- | --- | --- |
| 6qrjs | Front panel industrial UI preview（320x172） | 重新设计（#7n4qd） | `6qrjs-front-panel-industrial-ui-preview/SPEC.md` | 2026-03-01 | 视觉基线保留；自检页运行语义迁移到 #7n4qd |
| 7n4qd | MCU self-check live panel (resident Variant C) | 已完成 | `7n4qd-mcu-self-check-live-panel/SPEC.md` | 2026-03-02 | 自检期间实时进度 + 自检后常驻 + 真实数据刷新 + BMS 放电就绪门控与恢复 |
