# 主动告警逐实例消音 演进历史

> 当前有效合同见 [`SPEC.md`](./SPEC.md)。

## 决策

| 决策 | 原因 |
| --- | --- |
| 消音绑定 `alert_id + instance_id` | 防止用户对旧告警的操作静音复发后的新告警。 |
| 静音仅保存在 RAM | 告警解除和重启后必须回到正常提示策略，不引入 EEPROM 兼容和恢复语义。 |
| 覆盖现有 9 类运行期信号 | 覆盖真实可观测告警，不为 `IoOverPower` 生成虚假实例。 |
| 旧固件显式 `unsupported` | 客户端需要区分“没有告警”与“设备无法安全执行消音”。 |
| 前面板先评审后接线 | 小屏交互和音频行为具有用户可见风险，先锁定真实 RGB565 像素再接入写路径。 |

## References

- [`SPEC.md`](./SPEC.md)
- [`IMPLEMENTATION.md`](./IMPLEMENTATION.md)
- [`contracts/alerts.md`](./contracts/alerts.md)
