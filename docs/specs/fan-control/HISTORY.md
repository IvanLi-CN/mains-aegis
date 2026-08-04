# 风扇温控与故障保护 演进历史

> 这里记录影响当前规范理解的关键演进；当前有效合同仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-03-13: 首版规格冻结 V1 风扇控制口径：最高温三档、`3C` 回滞、`10s` 余冷、`2s` tach 看门狗、异常全速保护。
- 2026-03-13: 已落地 `esp_firmware::fan` 纯逻辑状态机、`FAN_TACH/FAN_EN/FAN_VSET_PWM` 固件接线、`fan:` 日志与 bench 文档；补充 `tach` 故障锁存与抗毛刺恢复、故障强制高转期间的恢复去抖、恢复窗口静默超时后重新取证、BMS isolation 窗口内使用缓存温度、请求/实际双层 telemetry、默认 `info` 可见的限频 tach bring-up 日志、双 TMP112 持续采样、raw x16 阈值口径、host 侧纯逻辑单测脚本，以及 PWM 初始化/运行期失败强制 `FAN_EN`/`FAN_VSET_PWM` fail-safe；PR 为 `#36`，当前等待 review-loop 收敛。
- 2026-03-15: 已同步 `origin/main` 的运行时音频与 BQ40 基线更新；风扇控制分支保留既有温控 / tach / 日志契约，并已重新通过 host fan 单测与 `cargo build --release`。
- 2026-04-05: `BQ25792 TS_WARM/TS_HOT/TREG` 现在会直接抢占风扇到全速，并在 `fan:` 日志中显式标注 `charger_thermal` 来源与温区快照。
- 2026-08-04: 将规格从早期固定三档描述同步到当前渐进 PWM 控制器；tach PPR 收敛为支持 `1/2 PPR`、默认 `2 PPR` 的构建期参数，且不作为温控闭环输入。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
