# HW Signal contracts：`THERM_KILL_N`（tps-tmp112-alert-overtemp-hold）

## Summary

本契约冻结 `THERM_KILL_N` 硬停机线的语义与连接关系，用于验证“任一路过温 → 双路 TPS55288 硬停机”链路。

## Signal

- Name: `THERM_KILL_N`
- Polarity: active-low
- Electrical: open-drain (wired-AND), pulled up to `3.3V`
- Scope: internal (board-level)

## Producers (drive-low sources)

- `TMP112A(TPS-A).ALERT`（open-drain）
- `TMP112A(TPS-B).ALERT`（open-drain）
- `MCU.GPIO40(THERM_KILL_N)`（可配置为 open-drain output；用于强制停机和 TPS I2C 失联硬抑制）

## Consumers

- `MCU.GPIO40`（作为输入读取 `THERM_KILL_N` 电平，用于可见性/日志/故障诊断）
- `TPS_EN` 硬件链路（通过反相 + 下拉实现双路 `TPS55288.EN/UVLO` 关断；细节见 `docs/power-monitoring-design.md`）

## Semantics (MUST)

- `THERM_KILL_N=1`：无硬停机请求（正常）。
- `THERM_KILL_N=0`：硬停机请求有效：
  - 两路 `TPS55288` 通过硬件链路被关断（双路同时停机）。
  - `THERM_KILL_N` 作为电平型告警，应保持为低，直到所有拉低源释放。
- `TPS_EN` 没有独立 MCU 可读引脚。诊断中的 `tps_en_effective_inhibit` 是由 `THERM_KILL_N=0` 和板级连接关系推导，不能替代直接测量。

## MCU behavior (MUST)

- 默认：`GPIO40` 不主动拉低 `THERM_KILL_N`（避免上电/复位期间误触发硬停机）。
- 当启用“强制停机”模式，或任一已请求 TPS 的可重试 I2C 错误耗尽预算时：`GPIO40` 以 open-drain 方式拉低 `THERM_KILL_N`。TPS I2C 路径必须先完成双路软件停机与尽力 disable，再执行这一步。
- MCU release 只将 GPIO40 返回高阻，不清 TPS 故障锁存、不自动探测或恢复输出。释放后线路仍低表示其它源仍在拉低，必须保持硬停机。
- MCU 重启后沿用启动路径自动释放 GPIO40；该运行期互锁不跨重启持久化。

## Notes

- 该信号为“电平型告警”，不应与需要可靠捕获的脉冲中断共线（已在项目设计中明确）。
