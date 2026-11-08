# BQ40 容量过充诊断

本次停充的直接原因已确认：BQ40 在尚未达到 VCT/FC、充电器仍处于约 500mA 恒流阶段时，触发容量过充保护 OC，置位 XCHG。充电控制遵循保护停止；120mA/80mA 尾流终止窗口尚未得到验证。当前证据将问题收敛到容量模型和充电终止参考不一致，但不能唯一确定是 ChemID、历史满充参考点、阻抗学习还是实际电芯状态造成。

## 实机读回

设备身份为 `mains-aegis-198840`，既有绑定 `serial-04f3bb3f5367`。通过 devd catalog 选择、dry-run、flash、身份核验安装只读诊断固件 `7893fcad`，build ID 为 `7893fcad-clean-9b267bf06a67e379`。本轮未 START、未写 DF、未清保护、未重置学习，也未强制放电。

连续三次完整制造诊断及末次轻量电流/充电器快照保存在[机器可读证据](bq40-oc-diagnosis.json)。三次容量模型参数一致；原始 MAC 负载、各包时间和读取错误一并保存，不将顺序读取视为同步测量。

| 项目 | 实际读回 | 解释 |
|---|---:|---|
| BatteryMode / 容量单位 | 0x6001 / mAh | 排除把 10mWh 误当 mAh 的解释 |
| TrueRemQ | 约 3042mAh | GaugeStatus1 未经过滤的内部剩余容量 |
| TrueFCCQ / SBS FCC | 2624mAh / 2624mAh | FCC 偏小并非只发生在 Web 缓存或 SBS 显示层 |
| SBS RemainingCapacity / SOC | 2597mAh / 99% | RSOCL=1；有效满充前的 99% 限制不代表内部容量尚未越过 FCC |
| OC Threshold / Recovery / RSOC Recovery | 300mAh / 2mAh / 90% | 实际读取，不使用 TRM 默认值代替 |
| DesignCapacity | 4400mAh | 与项目规定 5000mAh 不一致 |
| Qmax Cell 1–4 | 4182 / 4129 / 4210 / 4163mAh | 不等于 FCC；这些数值本身不能证明学习损坏 |
| Qmax Pack | 4129mAh | 独立 DF 读回 |
| ChemicalID 原始负载 | `10 12` | 按小端为 0x1210，与 TI 默认示例 ChemID 一致；未证明匹配当前电芯 |
| IT Load Select / Mode | 7 / 0 | 仿真采用负载估计和恒流模型，不能将 FCC 直接当成 Qmax |
| Reserve Capacity | 0mAh | 排除固定预留约 1.5Ah 导致 FCC 下降的解释 |
| IT Term Voltage | 9000mV | 放电仿真截止值，沿用原厂 3S 示例；须独立对齐 4S 项目，不是充电截止电压 |
| Charge Term Taper / Voltage | 120mA / 100mV | 本轮只读，未再改参数 |
| 历史 DODEOC Cell 1–4 | 4624 / 5120 / 2704 / 2608 | 保留的满充终点参考，不是当前电芯 SOC；来源及与当前终止目标的适配需验证 |

GaugeStatus3 未通过严格长度校验，保留 `invalid_response` 和空值；因此完整制造包的 `ok=false`，不宣称所有 MAC 读取成功。Qmax 数据来自成功的 DF 字段读取，尚无 GaugeStatus3 交叉验证。该缺口不影响成功取得的 GaugeStatus1、GaugeStatus2 和 OC 配置，但不能隐藏。

## 直接原因与因果边界

[BQ40Z50-R2 TRM §2.17](../manuals/BQ40Z50-R2-TRM/BQ40Z50-R2-TRM.md#217-overcharge-protection)规定：容量达到 FCC 后，内部充电累计量达到 OC 阈值时置位 OC/XCHG。本次已捕获 SafetyAlert[OC] 转为 SafetyStatus[OC]，同时电流从约 500mA 降到接近零，PF 未置位。当前内部 TrueRemQ 比 TrueFCCQ 高约 418mAh，与容量已越过模型满充点一致。

TrueRemQ−TrueFCCQ 不是可直接读取的 OC 内部计数器，不能用 418mAh 代替其数值，也不能用它精确重建触发时刻。此前采样开始至触发约流入 116mAh，记录开始时已存在 OC Alert；因此这 116mAh 不是完整的保护累计窗口。已有记录只能证明保护在自然终止之前生效。

FCC=2624mAh 同时存在于 SBS 和 IT 未平滑结果，不能归结为网页缓存显示。QmaxPack=4129mAh 排除了“Qmax 就是 2624mAh”的解释。TRM §6.4 说明 FCC 还依赖负载、温度、阻抗、放电截止条件及满充参考；内部 TrueRemQ 可以高于 FCC，这种现象本身并非协议错误。

历史 DODEOC 四个参考点差异明显，是需要追踪的学习状态线索。没有形成这些参考点时的电压、电流、均衡和有效终止记录，不能认定由旧 200mA 策略或 BGND 干扰单独造成，也不能断言只改 taper 就会自动修复它们。

## 已确认的配置偏差与来源

项目 [BMS 设计](../bms-design.md#55-仍需单独锁定的量产字段)要求 DesignCapacity=5000mAh，并要求通过 TI chemistry matching 决定 ChemID。当前设备为 4400mAh / 0x1210；这两个值也出现在 [TI 官方数据手册默认示例](https://www.ti.com/lit/ds/symlink/bq40z50-r2.pdf)中。相同默认值不能证明 ChemID 必然错误，却说明缺少针对本项目电芯的匹配证据。

本地原厂 R2 `section1.bin` 在 0x48E5 的 DesignCapacity 为 4400mAh，在 0x484A 的 Term Voltage 为 9000mV。`asset-df-mainboard` overlay 从该资产复制并修补指定主板字段，没有修补这两个容量模型字段。故该恢复路径不会自动成为项目完整 golden image；这是代码可复核的来源路径，不能仅凭当前快照断言设备是哪次操作写入了这些值。原厂二进制资产保持不变。

9000mV 是 3S×3000mV 的示例值。对当前 4S，它相当于平均 2250mV/节，不能视为已经完成项目放电策略适配。较低仿真截止电压通常扩大可用容量，不能单独用它解释本次 FCC 偏小；也不能直接把它改成某个值而不核对项目负载与保护策略。

## 修复次序

1. 保持 OC 保护、120mA/80mA 候选参数及 STOP 状态，先完成容量配置资格确认。反复 START 或提高 OC 阈值不能证明自然终止正确。
2. 对 DesignCapacity=5000mAh、DesignVoltage、4S 放电仿真终点、ChemID 匹配和校准形成独立 golden 配置。ChemID 需要 TI GPCCHEM 或已有有效匹配报告，不能从型号或默认值臆选；不能把改写 ChemicalID 数值当成导入化学模型。
3. 在已确认模型及安全配置后，再设计受控恢复、正常充放电/静置及学习验证，跟踪 TrueRemQ/FCC、DODEOC、Qmax、有效终止和 OC。当前静置的小电流约 −3～−5mA 小于 QuitCurrent=10mA，不能假定累计放出 2mAh 就已满足 TRM 所说的持续放电恢复条件。
4. 同一候选重新完成正常充电稳定窗口、自然终止和终止后观察。120mA/80mA 不因本次 OC 失败自动判错，也不提前判为通过。

原锁定计划只授权 taper 定向写入，明确排除 ChemID、Qmax、Ra、校准和保护改写。上述容量模型修复需要扩展配置范围并重新明确验证条件；本次诊断未越界写入。这是具体的修复前置条件，不是通过重置学习或屏蔽保护绕过验收。

## 诊断固件自身验证

固件 host 测试 567 项通过，ESP 检查通过；使用本机已有 Spec checker 路径运行完整 `just check` 通过。诊断扩展只增加读取，不改变充电策略或后台自动写 DF 行为。

中间候选 `f3f9cf22` 的两个原始 DF 块扩展读取出现 CDC 超时，并观察到 watchdog 复位。已改为四个明确字段的标量读取，最终候选三轮制造诊断各约 3.04 秒成功返回、设备单调时间递增；不把失败候选中的缺失包当作参数证据，也不把标量读取恢复成功当成已证明该超时的底层原因。最终现场保持充电关闭，保护仍置位。PR 继续保持 Draft，A3/A4 未通过。
