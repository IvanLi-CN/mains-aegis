---
title: 电源与 BMS
description: 电池、BMS、充电、主输出、监测与保护链路。
---

# 电源与 BMS

## 1. 电源主链路

```text
4S1P Battery Pack
  -> BQ40Z50-R2 (计量 + 主保护 + CHG/DSG drive)
  -> BQ296100DSGR + CLM1612P1412 (二次过压 / 熔断链)
  -> BQ25792 (USB-C / PD(PPS) + DC 双输入充电)
  -> TPS55288-A/B + MX5050L (主输出稳压与阻断)
  -> INA3221 + TMP112A + THERM_KILL_N (遥测与硬件保护)
```

## 2. 电池包与 BMS 基线

| 项目 | 冻结口径 |
| --- | --- |
| 串并联 | `4S1P` |
| 电芯 | `EVE INR21700/50E` |
| 包电压 | `16.8V` 满充，`14.6V` 标称，`10.0V` 截止 |
| 主 BMS | `BQ40Z50RSMR-R2` |
| 二级 OVP | `BQ296100DSGR`，`4.35V/cell` |
| 二次保护 | `CLM1612P1412` + `AO3400A` heater 驱动 |
| 均衡 | 外部被动均衡，目标约 `200mA` |
| 分流 | `R42 = 1mΩ`，连接 `AGND` 与 `CHGND` |

板级实现里，`BQ40Z50` 通过 `SRP/SRN` 做采样，网表已落地：

- `R38 = 100Ω`：`SRP` 串阻
- `R40 = 100Ω`：`SRN` 串阻
- `C40 = 0.1µF`：`SRP/SRN` 差分滤波

这部分的工程含义很直接：`AGND` 是采样参考地，`CHGND` 是大电流地，二者不能随意短接成一片铜皮。

## 3. 充电链路

| 项目 | 当前实现 |
| --- | --- |
| 充电器 | `BQ25792RQMR` |
| 输入 1 | USB-C / PD(PPS)，`VAC1 = UCM_VBUS` |
| 输入 2 | DC 输入，`VAC2 = VIN` |
| 系统节点 | `SYS = VSYS` |
| 默认充电档位 | `1A` |
| 其他档位 | `500mA` / `100mA`；`2A` 作为能力预留 |
| 使能约束 | `CE` 为低有效；电池路径未确认前应保持禁充安全态 |

主板网表里对应关系已经固定：

- `U16` 输入口输出 `VIN_UNSAFE`
- `U10(TPS2490)` 把 `VIN_UNSAFE` 处理后送成 `VIN`
- `U11(BQ25792).SYS -> VSYS`
- `U19/U20(TPS62933)` 从 `VSYS` 生成 `+5V` 和 `3V3`

注意边界：`BQ25792.SYS` 不是整机 `120W` 级 UPS 主输出母线；主输出路径在后级 `TPS55288`。

## 4. 主输出链路

| 项目 | 当前实现 |
| --- | --- |
| 输出版本 | `12V` / `19V`（换固件，不是运行时切换） |
| 输出级 | `TPS55288 × 2` |
| I2C 地址 | `OUT-A = 0x74`，`OUT-B = 0x75` |
| 默认软配置 | `out_a` 启用，目标限流 `3.5A` |
| 汇流方式 | `ISP_TPSA/B` 经 `10mΩ` 分流电阻汇到 `VOUT_TPS` |
| 后级阻断 | `U21(MX5050L) + Q28`：`VOUT_TPS -> VOUT` |

当前主板网表能直接读出的关键关系：

- `U17.VOUT/ISP = ISP_TPSA`，`R68: ISP_TPSA -> VOUT_TPS`
- `U18.VOUT/ISP = ISP_TPSB`，`R83: ISP_TPSB -> VOUT_TPS`
- `U21(MX5050L) + Q28`：`VOUT_TPS -> VOUT`
- `Q11`：`VIN -> VOUT` 直通路径

这意味着 bring-up 时要分清三件事：

1. `TPS55288` 是否已经配置成功
2. `VOUT_TPS` 是否建立
3. `VOUT_TPS -> VOUT` 的后级阻断是否导通

## 5. 遥测与保护

### 5.1 INA3221

| 通道 | 监测对象 | 分流电阻 |
| --- | --- | --- |
| `CH3` | `UPS VIN` | `7mΩ` |
| `CH2` | `TPS55288 OUT-A` | `10mΩ` |
| `CH1` | `TPS55288 OUT-B` | `10mΩ` |

硬件告警线：

- `INA3221_PV`：欠压电平告警
- `INA3221_CRITICAL`：单路或求和过流快速告警
- `INA3221_WARNING`：预警输入

固件阈值口径：

- 欠压：`12V` 版本 `11V`；`19V` 版本 `18V`
- 过流：`VIN > 7A`，`OUT-A/OUT-B > 4A`，`OUT-A + OUT-B > 6.5A`

### 5.2 TMP112A 与热停机

| 通道 | 地址 | 作用 |
| --- | --- | --- |
| `TMP112-A` | `0x48` | `TPS55288-A` 热点 |
| `TMP112-B` | `0x49` | `TPS55288-B` 热点 |

两路 `ALERT` 线与到 `THERM_KILL_N`。`THERM_KILL_N=0` 时，固件与硬件都应把它视为“禁止继续出力”的硬条件。

## 6. 启动与门控

自检阶段与电源链路直接相关的门控只有这些：

- `BQ40Z50` 不在线 -> 输出保持 `HOLD`
- `BQ40Z50` 在线但 `XDSG=1` 或 `DSG=0` -> 输出保持 `HOLD`
- `THERM_KILL_N=0` -> 允许进入 emergency-stop，关断 `TPS55288`
- `TPS55288` 命中 `SCP/OCP/OVP` -> 允许 emergency-stop

这套规则保证了 bring-up 时能把“上游未授权”和“输出级自身故障”区分开。

## 7. 相关文档

- [BMS 设计](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/bms-design.md)
- [充电器设计](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/charger-design.md)
- [UPS 主输出设计](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/ups-output-design.md)
- [电源监测与保护设计](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/power-monitoring-design.md)
- [I2C / SMBus 地址映射](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/i2c-address-map.md)
