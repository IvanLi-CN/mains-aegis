# BQ25792 charging enable + status capture 演进历史

> 这里记录影响当前规范理解的关键演进；当前有效合同仍以 `./SPEC.md` 为准。

## 普通充电与离线激活

早期 bring-up 将 `VBAT_PRESENT_STAT=1` 作为统一使能条件，并保留了旧 `mcu-agentd` 验证方式。该条件不能覆盖 BMS 双向切断后 PACK 端低电压的离线激活；有界手动 Activation 因此单独定义低电压例外，普通充电与温度保护保持既有合同，验证入口统一采用 devd。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
