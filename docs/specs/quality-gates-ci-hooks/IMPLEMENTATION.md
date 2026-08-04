# 仓库代码质量门槛：Git hooks + GitHub Actions 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Lifecycle: archived
- Implementation: 见下方迁移状态与覆盖记录。

## Migrated Implementation Record

- Status: 已完成
- Created: 2026-01-22
- Last: 2026-01-22

- [x] M1: 落地 `bun` + `commitlint`（含英文口径规则）与 `lefthook.yml`（pre-commit/commit-msg/pre-push）
- [x] M2: 落地 GitHub Actions workflows（fmt / firmware build / PR title lint / dependency review）并加上 `paths-ignore`（`docs/**` + `README.md`）
- [x] M3: 更新 `docs/README.md`，补充质量门槛入口与常见排错指引

## References

- `./SPEC.md`
- `./HISTORY.md`
