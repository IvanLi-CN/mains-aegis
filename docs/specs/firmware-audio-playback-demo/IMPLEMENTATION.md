# 固件音频播放 + Demo 素材 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Lifecycle: archived
- Implementation: 见下方迁移状态与覆盖记录。

## Migrated Implementation Record

- Status: 已完成
- Created: 2026-01-22
- Last: 2026-01-23

- [x] M1: 落地 Demo playlist 音频文件（6 段；每段约 10s；段间 1s 静音；WAV(PCM16LE)；含 旋律 + 扫频）
- [x] M2: 固件侧 I2S/TDM TX + DMA 播放链路跑通（可播放 Demo）
- [x] M3: 落地可用于验证的触发方式 + 播放日志（start/stop + underrun 可观测）
- [x] M4: 完成一次端到端手工验证记录并同步相关文档入口

## References

- `./SPEC.md`
- `./HISTORY.md`
