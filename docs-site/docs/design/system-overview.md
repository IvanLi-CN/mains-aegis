---
title: 系统概览
description: 系统组成、关键参数、接口和启动流程。
---

# 系统概览

## 1. 系统边界

`mains-aegis` 可以拆成三层：

- **主板**：电池、BMS、充电、主输出、监测、音频、前面板互连
- **前面板**：显示、触摸、按键、USB 前端、背光
- **固件**：ESP32-S3 bring-up、自检、Dashboard、告警音和输出门控

## 2. 模块组成

| 分块 | 已落地器件 / 文件 | 说明 |
| --- | --- | --- |
| 电池与 BMS | `BQ40Z50-R2`、`BQ296100DSGR`、`CLM1612P1412` | 4S 电池管理、主保护与二次保护 |
| 充电 | `BQ25792`、`FUSB302B`、`CH442E` | USB-C / PD(PPS) + DC 双输入，`SYS=VSYS` |
| 主输出 | `TPS55288 × 2`、`MX5050L`、外置 N-MOS | 两路可编程升降压汇到 `VOUT` |
| 遥测 | `INA3221`、`TMP112A × 2` | 电压 / 电流 / 温度 / 热关断链路 |
| 前面板 | `TCA6408A`、SPI 屏、I2C 触摸 | 屏幕、触摸、按键和背光 |
| 主控 | `ESP32-S3-FH4R2` | `I2C1/I2C2`、SPI、I2S、告警输入 |

## 3. 冻结基线

| 项目 | 数值 / 状态 |
| --- | --- |
| 电池配置 | `4S1P`，`4 × 21700` 电池座 |
| 电芯 | `EVE INR21700/50E` |
| 电池包电压 | 标称 `14.6V`，满充 `16.8V`，截止放电 `10.0V` |
| 电池包能量 | 约 `73Wh` |
| 主输出口径 | 固件二选一：`12V` / `19V` |
| 输出电流上限 | `6.32A` |
| I2C1 | `GPIO47=SCL`，`GPIO48=SDA`，`25kHz` |
| I2C2 | `GPIO9=SCL`，`GPIO8=SDA` |
| 启动界面 | `SELF CHECK` -> Dashboard |

## 4. 关键连接器

### 4.1 主板连接器

| 接口 | 作用 | 关键网络 |
| --- | --- | --- |
| `H1` | 电池抽头 | `AGND`、`VC1..VC4` |
| `U16` | DC 输入口 | `VIN_UNSAFE`、`CHGND` |
| `U4` | UPS 输出口 | `VOUT`、`CHGND` |
| `FPC1` | 主板 <-> 前面板 | `3V3`、`BTN_CENTER`、`I2C2_*`、`CTP_IRQ`、`TCA_RESET#`、`BLK`、`DC/MOSI/SCLK`、`UCM_DP/DM` |
| `U30` | 喇叭口 | `OUTP`、`OUTN` |

### 4.2 `FPC1` 关键网络

| 网络 | 含义 |
| --- | --- |
| `TCA_RESET#` | 前面板 `TCA6408A` 总复位 |
| `I2C2_SCL / I2C2_SDA` | `TCA6408A`、`FUSB302B`、触摸共用总线 |
| `I2C2_INT` | `TCA6408A` 与 `FUSB302B` 的开漏中断线与 |
| `CTP_IRQ` | 触摸独立中断，不与 `I2C2_INT` 并线 |
| `DC / MOSI / SCLK` | 屏幕 SPI 控制 |
| `BLK` | 背光控制 |
| `BTN_CENTER` | 中键，直达 `ESP32-S3.GPIO0` |

## 5. MCU 关键映射

| GPIO | 网络 | 用途 |
| --- | --- | --- |
| `GPIO0` | `BTN_CENTER` | 中键 / 下载模式 strapping |
| `GPIO1` | `TCA_RESET#` | 前面板扩展器复位 |
| `GPIO7/8/9` | `I2C2_INT/SDA/SCL` | 前面板总线 |
| `GPIO10/11/12/13/14` | `DC/MOSI/SCLK/BLK/CTP_IRQ` | 屏幕与触摸 |
| `GPIO15/16/17` | `CHG_CE/CHG_ILIM_HIZ_BRK/CHG_INT` | 充电器控制与中断 |
| `GPIO21` | `BMS_BTP_INT_H` | BMS 告警 |
| `GPIO33` | `I2C1_INT` | `TPS55288` 共享中断 |
| `GPIO37/38/39/40` | `INA3221_PV/CRITICAL/WARNING/THERM_KILL_N` | 监测与热保护 |
| `GPIO41/42` | `SYNCA/SYNCB` | 双路 `TPS55288` 同步 |
| `GPIO47/48` | `I2C1_SCL/SDA` | 电源主总线 |

## 6. 启动路径

```text
Battery / USB-C / DC
  -> BMS / Charger 上电
  -> VSYS -> 5V / 3V3
  -> ESP32-S3 启动
  -> SELF CHECK 按固定顺序探测
  -> 根据 BMS / Charger / TPS / 热保护结果做门控
  -> 满足条件后进入 Dashboard
```

固件启动阶段的固定顺序：

1. `TPS55288` `SYNC` 启动
2. `INA3221` 与 `TMP112A` 初始化
3. 屏幕链路（当前以前面板 `TCA6408A` 可达性代表）
4. `BQ40Z50`
5. `BQ25792`
6. `TPS55288`

规则只有两条：

- 非紧急情况不在自检阶段主动改 `TPS55288` 输出状态
- `BQ40Z50` 缺失或放电未授权时，输出条目应停在 `HOLD`，不是直接判成输出级故障

## 7. 相关文档

- [电源与 BMS](/design/power-and-bms)
- [前面板与固件](/design/front-panel-and-firmware)
- [硬件选型总览](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/hardware-selection.md)
- [主板 PCB 说明](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/pcbs/mainboard/README.md)
- [开机自检流程](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/boot-self-test-flow.md)
