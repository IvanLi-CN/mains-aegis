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
| 6qrjs | Front panel industrial UI preview（320x172） | 已完成 | `6qrjs-front-panel-industrial-ui-preview/SPEC.md` | 2026-02-27 | Dashboard 已冻结为 Variant B；模式语义重构为 BYPASS/LINE STANDBY/LINE ASSIST/BACKUP 且仅 LINE STANDBY 允许充电 |
