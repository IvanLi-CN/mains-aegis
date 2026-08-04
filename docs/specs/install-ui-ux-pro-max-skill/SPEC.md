# UI UX Pro Max Skill 存档

> 当前有效规范以本文为准；实现覆盖与当前状态见 `./IMPLEMENTATION.md`，关键演进原因见 `./HISTORY.md`。

## 背景

`ui-ux-pro-max` 曾作为通用 UI/UX 设计辅助 Skill 安装在项目的
`.codex/skills/` 中。它不包含 Mains Aegis 专属流程、领域知识或安全边界，
因此不再作为项目资产维护。

## 当前约束

- 仓库不包含 `.codex/skills/ui-ux-pro-max/`。
- 仓库不保留该 Skill 的专用缓存忽略规则。
- Mains Aegis 专属 Skill 继续保留在 `.codex/skills/` 中。
- 通用 UI/UX 能力应由使用者环境或插件提供，不在本仓库复制维护。

## 非目标

- 不删除或合并 Mains Aegis 专属 Skill。
- 不改变产品代码、固件逻辑、设备操作边界或外部接口。
- 不规定使用者环境中是否安装同名通用 Skill。

## 验收标准

- `.codex/skills/ui-ux-pro-max/` 不存在。
- `.gitignore` 不包含该 Skill 的专用路径。
- `.codex/skills/` 中的项目专属 Skill 保持可用。

## 关联文档

- `docs/specs/README.md`

## Visual Evidence

PR: none

该变更不影响产品界面或其他视觉交付面。
