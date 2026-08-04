# 功能验证测试固件（test-fw）与音频优先级协调 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Lifecycle: archived
- Implementation: 见下方迁移状态与覆盖记录。

## Migrated Implementation Record

- Status: 已完成
- Created: 2026-03-05
- Last: 2026-03-06

## Migrated Delivery Record

## 里程碑（Milestones）

- [x] M1: test-fw 入口重命名与独立烧录配置迁移完成。
- [x] M2: feature 矩阵与编译期门禁完成。
- [x] M3: 导航状态机 + UI 页面 + 返回控件语义完成。
- [x] M4: 音频优先级协调模块接入完成。
- [x] M5: 文档与构建矩阵验证完成。

## 实现结果

- 二进制入口完成替换：`display-diag-fw` 已移除，新增 `test-fw`。
- feature 矩阵落地：功能 feature + 默认测试 feature + 编译期冲突/不匹配报错。
- 导航状态机落地：单功能直达、多功能导航、默认测试直达后可返回导航。
- UI 落地：导航页/屏幕静态测试页/音频测试页；返回控件始终显示，无导航时禁用。
- 音频协调落地：高优先级抢占低优先级；队列按优先级出队、同级 FIFO。


## References

- `./SPEC.md`
- `./HISTORY.md`
