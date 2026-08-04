# BQ40 Cell4 protocol-safe diagnostics 演进历史

> 这里记录影响当前规范理解的关键演进；当前有效合同仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-03-15: merge-proof review 收敛补丁：`ManufacturerBlockAccess(0x44)` 的 DF 读回复统一改为 fail-closed，只接受“echoed start address + 32-byte payload”的 TRM 规范返回；MB44 真实读路径改为优先校验 reply PEC、失败后再回退到无 PEC 读；live DF 抓取失败时改为 fail-closed，不再把 `stock section1` 伪装成有效 live DF；ROM repair 使用的 live DF 窗口在每次 recover attempt 前强制重抓；`ManufacturerAccess(0x00)` toggle / reset 与 `bms_pec` 探测统一复用 direct/PEC 发送变体；`flash.sh` / `monitor.sh` 改为各自独立持有设备锁，避免 `run.sh` 父进程退出后提前放锁；目录锁补上 owner 写入竞态等待、stale owner PID + 启动信息校验，避免异常退出或 PID 复用后永久卡死；fresh flash 后恢复成“先尝试无 reset 附着、失败再 fallback reset”的 monitor 策略，并把初始 stdout 等待窗口显式对齐 post-flash quiet budget，避免把启动边界问题静默掩盖。
- 2026-03-15: 补齐 reply `PEC` 探测并完成二次实机验证；本规格范围内的工具协议/只读诊断/互斥收敛任务全部关闭，后续 `Cell4` 修复与主固件自检链路移交其它规格跟踪。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
