# BQ25792 charging enable + status capture 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Lifecycle: archived
- Implementation: 见下方迁移状态与覆盖记录。

## 充电边界覆盖

`output/pure.rs` 分离普通电池路径条件与有界离线低压激活例外；`output/mod.rs` 只为显式手动 Activation、离线 BQ40 和有效 deadline 建立该例外。BQ40 失败处理独立于 BQ25792 通信失败处理，USB 提供启动与只读查询；完整产品温度/请求合同仍由活动温度主题负责。

## References

- `./SPEC.md`
- `./HISTORY.md`
