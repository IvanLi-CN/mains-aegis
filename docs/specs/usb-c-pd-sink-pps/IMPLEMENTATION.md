# USB-C PD/PPS Sink 首阶段实现 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Lifecycle: archived
- Implementation: 见下方迁移状态与覆盖记录。

## Migrated Implementation Record

- Status: 已完成
- Created: 2026-04-07
- Last: 2026-04-23

- Directory: `docs/specs/usb-c-pd-sink-pps/assets/`
- In-spec references: `![...](./assets/<file>.png)`
- Visual evidence source: maintain `## Visual Evidence` in this spec when owner-facing or PR-facing screenshots are needed.

None。

- [x] M1: 新建 spec、登记索引，并冻结 feature / 安全边界 / 验收口径
- [x] M2: 将 `FrontPanel` 改为共享 I2C2 的泛型设备，并在主固件与两个测试固件完成接线迁移
- [x] M3: 新增 `usb_pd` 模块，完成 feature 驱动 capability 生成、固定 PDO / PPS 纯逻辑与 FUSB302 薄驱动骨架
- [x] M4: 将 PD sink manager 接入主循环与 `PowerManager` / `BQ25792` 运行时，补齐 `IINDPM/VINDPM` 与 unsafe-source 保护
- [x] M5: 完成测试、feature 编译矩阵、spec sync、提交/推送/PR 与 review-loop 收口

## Migrated Delivery Record

## 当前实机状态（2026-04-23）

- 当前 `usb-c-pd-sink-pps` 已完成闭环：实机冷启动与真实 USB 热插拔后，`PPS` 都能在秒级恢复，不再出现“随机卡在 `CAP? + 5V` 或需要十几秒以上才恢复”的主故障。
- 最新板上证据：
  - reset 基线日志：`/Users/ivan/Projects/Ivan/mains-aegis/.mcu-agentd/monitor/esp/20260422_204331_570.mon.ndjson`
    - `2026-04-22T20:43:34.370942Z attach`
    - `2026-04-22T20:43:36.036427Z contract active kind=pps`
    - `attach -> PPS ≈ 1.67s`
  - 主人实机热插拔复测：已确认“重新插拔已经能秒协商成功”，不再出现此前 3s / 10s / 45s 的双稳态恢复。
- 最终根因收敛为两层：
  - 协议恢复正确性：`partial RX` 被过早读取/flush、`retry/hard reset` 与 `missing source caps` 恢复链交叉打断，导致同一条会话里不断重复 `Get_Source_Cap / reset / rearm`。
  - 主循环调度：`attached && contract=None` 窗口里，`usb_pd.tick()` 之前被 `power.tick()`、BMS/charger/UI 轮询拖慢，导致明明配置了 `400ms` 的恢复超时，却经常要到 `~1s` 之后才真正执行。
- 最终修复由两部分组成：
  - 协议层：只在完整帧 ready 后读取 RX；`partial RX + hard reset` 先 defer；`no-contract` 恢复维持 `PD_RESET + 等 Source Caps`，避免把协议层 reset 当作物理 detach 乱拆。
  - 调度层：在 `/Users/ivan/Projects/Ivan/mains-aegis/firmware/src/main.rs` 为 `attached && contract=None` 增加短时间片协商优先窗口，优先连续服务 `usb_pd.tick()` 与 IRQ 收敛，但每个时间片必须很快回到 `power.tick()`、前面板触摸轮询等其它周期任务。
- 结果：`SOURCE_CAPS_WAIT_TIMEOUT_MS = 400ms` 现在能按预期生效，reset 基线已从约 `2.41s` 压到约 `1.67s`，真实热插拔也回到秒级恢复。


## References

- `./SPEC.md`
- `./HISTORY.md`
