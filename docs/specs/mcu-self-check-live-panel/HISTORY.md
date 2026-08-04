# MCU 自检页实时化与常驻显示 演进历史

> 这里记录影响当前规范理解的关键演进；当前有效合同仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-03-15: 被 dashboard-live-after-self-check 重新设计覆盖；保留开机自检实时化能力，但运行态默认页面改为真实 Dashboard。
- 2026-03-15: hotfix，`BQ40 OperationStatus(0x54)` 改按 `SMBus block` 读取，并为关键 `BQ40` 读取补上兼容式 `PEC` 校验优先路径；这样主固件不再把有效 `DSG FET=ON` 误读成 `dsg_fet_off`，自检可在真实放电就绪时正确放行输出。
- 2026-03-05: 激活稳态修正：`ADC_CONTROL` 置位失败降级为 `warn + 跳过 ADC 采样`，不再直接判定激活通信失败；激活结束后恢复充电使能状态（按激活前 `CHG_CE/chg_enabled` 还原），避免无谓充电瞬断。
- 2026-03-05: review-loop 收敛补丁：当 `OPERATION_STATUS` 读取失败（`discharge_ready=None`）时，BQ40 卡片改为 `WARN` 且允许触发激活；BMS 恢复放行 TPS 时同步触发 `INA3221` 重试，避免长期停留 `ina_uninit`。
- 2026-03-05: 修正 BMS 激活闭环细节：`OPERATION_STATUS` 读取失败不再放行 TPS；激活请求会清理 `bms/chg` 重试退避窗口；`BmsActivateConfirm` 弹窗收起条件与激活触发条件统一。
- 2026-03-02: 对齐 BMS 放电就绪语义：`XDSG=0 && DSG=0` 归类为 `WARN`；激活成功判定增加放电就绪与 `VBAT_PRESENT`，并补充 BMS 恢复后的 TPS 门控自动解除路径。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
