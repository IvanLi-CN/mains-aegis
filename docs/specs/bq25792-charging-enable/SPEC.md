# BQ25792 充电使能与状态采集边界

> 本主题已归档，保留 bring-up 充电使能边界作为参考；完整产品策略由 [BQ25792 充电策略](../bq25792-charge-policy/SPEC.md) 和 [BQ40 温度与请求合同](../bq40-charge-temperature-profile/SPEC.md) 定义，离线恢复入口见 [devd 合同](../mains-aegis-devd/SPEC.md)。本页不得用来覆盖这些主题的运行时安全条件。

## Context and Scope

BQ25792 位于 I2C 地址 `0x6B`。固件使用 `GPIO15 CHG_CE`、`GPIO16 CHG_ILIM_HIZ_BRK`、`GPIO17 CHG_INT`，读取 `REG1B..REG1F` 状态、`REG20..REG21` 故障和 `REG0F` 控制。必须区分充电芯片 BAT/PACK 端可见电压与 BMS 电芯侧电压；BMS 关断路径可使前者很低，`VBAT_PRESENT_STAT=0` 不能单独证明电芯未安装。

## Requirements

### REQ-BQ25792-ENABLE-001

- 固件必须在成功读取芯片状态后才使能充电；BQ25792 读写失败时禁充。
- 充电需要 `EN_CHG=1` 且 `CE=LOW`，并解除 `EN_HIZ` 和外部 ILIM_HIZ brake。
- 普通充电必须通过有效电池路径、输入、温度、BQ40 请求及运行时策略门控；不得用本页早期 bring-up 条件代替完整产品策略。原始 `VBAT_PRESENT_STAT` 与有效电池路径判断必须分别保留为真实遥测。

### REQ-BQ25792-ENABLE-002

- 手动请求、BQ40 无已知在线地址、在绝对 deadline 内且处于允许充电阶段的离线 Activation，允许在 `VBAT_PRESENT_STAT=0` 时施加受限充电源。这是 BMS 隔离后恢复所需的启动条件，不能要求 PACK 端先有电压。
- 该例外不得扩展到普通充电、在线放电授权恢复或 boot-diagnostic；输入、BQ25792 正常 JEITA 窗口、已经观察到的 BQ40 安全证据和超时合同仍必须满足。
- 唤醒目标不得高于 `VREG=16800mV / ICHG=200mA`，输入限流为 `IINDPM=500mA`，并关闭 `EN_TERM`。BQ25792 根据 BAT 电压自行选择涓流、预充和恒流阶段，实际电流不得仅由 `ICHG` 寄存器推断。
- 激活结束或超时必须停止唤醒，恢复寄存器后保持 CE 高电平，由普通轮询重新决定能否充电。启动时因 BMS 缺失而禁充不得成为健康充电模块的永久禁用开关；运行期必须区分模块探测成功与当前允许充电，safe mode 和模块不可用仍禁充。

### REQ-BQ25792-ENABLE-003

- 必须记录输入、电池端电压、原始电池在位位、充电使能、实际 IBAT、充电阶段、TS 状态、故障和策略原因。
- BQ40 通信失败必须清理其产品充电输入，并及时停止普通充电，但不得伪造 BQ25792 通信失败或使有界离线激活因预期的 BQ40 NACK 而中止。

## Verification

- VER-BQ25792-ENABLE-001，覆盖: REQ-BQ25792-ENABLE-001。普通充电在有效电池路径缺失、输入或温度不合格、BQ40 产品证据不满足或芯片读写失败时必须禁充。
- VER-BQ25792-ENABLE-002，覆盖: REQ-BQ25792-ENABLE-002。纯策略测试必须证明有界离线低压唤醒可使能，未请求激活或任一保留联锁不满足时仍禁充；实机需记录低压启动、阶段变化、使能与测量值、结束及超时行为。
- VER-BQ25792-ENABLE-003，覆盖: REQ-BQ25792-ENABLE-003。BMS 离线且 BQ25792 可读时，充电芯片遥测必须保持有效，不能误报 `charger_poll_failed`；参数写入和命令接受不得视为恢复成功。

## Related ADRs

None

## References

- [充电设计](../../charger-design.md)
- [BQ25792 数据手册](../../datasheets/BQ25792/BQ25792.md)，9.3.9.1–9.3.9.2：使能条件与充电阶段。
- 本地验证使用 `just firmware-host-test`、`just firmware-check` 与经授权的 devd 烧录和 USB 恢复入口，不使用旧 `mcu-agentd` 路径。
