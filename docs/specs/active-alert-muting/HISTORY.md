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
| 获批 scene 直接复用为运行时 renderer | 避免评审图与真机像素、字体或热区发生漂移。 |
| 系统静音不转换成策略静默 | 全局音量恢复后，未被用户消音且不受冷启动策略抑制的当前实例必须重新可听。 |

## References

- [`SPEC.md`](./SPEC.md)
- [`IMPLEMENTATION.md`](./IMPLEMENTATION.md)
- [`contracts/alerts.md`](./contracts/alerts.md)
