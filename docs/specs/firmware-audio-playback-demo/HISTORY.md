# 固件音频播放 + Demo 素材 演进历史

> 这里记录影响当前规范理解的关键演进；当前有效合同仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-01-22: 初始化计划与契约骨架
- 2026-01-23: 收敛为 PCM-only（WAV PCM16LE mono 8kHz），并落地固件侧 I2S/TDM 播放 Demo、固件侧素材落盘与验证文档入口；播放链路以连续流式 `push_with` 驱动 DMA ring，完成端到端烧录验证。
- 2026-01-23: 决策收敛：只接受 PCM（WAV PCM16LE）；更新契约与素材/固件为 PCM-only
- 2026-01-23: 修复 `DmaError::Late`（环形 DMA 喂数过晚）：改为 `push_with` 单循环流式生成（音频/段间静音）并将日志延后到 ring buffer 高水位；复核端到端 6 段均播放完成

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
