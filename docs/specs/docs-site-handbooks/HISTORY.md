# GitHub Pages 项目手册站 演进历史

> 这里记录影响当前规范理解的关键演进；当前有效合同仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-04-08: 新建规格，冻结项目手册站的范围、路由与验收口径。
- 2026-05-05: Pages 根站点调整为 Web App，项目手册站迁移到 `/docs/` 子路径，并记录 App path router 与 SPA fallback 要求。
- 2026-07-02: Pages artifact 默认改为相对路径发布；Web App 使用 `PAGES_BASE=./`，文档站 HTML 在复制到 `web/dist/docs/` 后重写为相对链接，避免自定义域名根路径白屏。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
