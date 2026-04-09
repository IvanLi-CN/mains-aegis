---
title: 基础使用与排障
description: Bring-up 后的常见现象、故障分流与记录建议。
---

# 基础使用与排障

## 1. 故障分流原则

排障时先回答两个问题：

1. 现象发生在**哪条链路**：前面板、BMS、充电、主输出、监测、固件运行时。
2. 手上有没有**对应证据**：页面状态、串口日志、网络名、I2C 地址、GPIO 电平。

不要把“屏幕不亮”和“整机不工作”画等号。

## 2. 按模块分流

| 模块 | 典型症状 | 第一检查项 |
| --- | --- | --- |
| 前面板 | 屏不亮、触摸不通、方向键无响应 | `BLK`、`TCA_RESET#`、`CS/RES/TP_RESET`、`CTP_IRQ`、`I2C2_*` |
| BMS | `BQ40Z50` 不在线、放电未授权 | `I2C1`、`VC1..VC4`、`BMS_BTP_INT_H`、`SafetyStatus/PFStatus` |
| 充电 | 无法识别输入、充电状态不变 | `BQ25792`、`CHG_INT`、`VAC1/VAC2`、输入口定义 |
| 主输出 | `TPS55288` 卡 `HOLD/ERR`、`VOUT` 不建立 | `BQ40Z50` 门控、`I2C1_INT`、`VOUT_TPS -> VOUT` |
| 遥测 | 电压、电流、温度值异常 | `INA3221`、`TMP112A`、`THERM_KILL_N` |
| 固件 | 日志不出、状态不更新、页面停住 | 构建 feature、烧录结果、串口监视、启动流程 |

## 3. 按现象排查

### 3.1 屏幕不亮

先查：

- `BLK` 是否有效
- `TCA_RESET#` 是否释放
- `CS` 是否回到可选通状态
- `RES / TP_RESET` 是否被错误地一直拉低
- `DC / MOSI / SCLK` 是否连续

### 3.2 触摸或按键无响应

先查：

- `CTP_IRQ`
- `I2C2_SCL / I2C2_SDA / I2C2_INT`
- `TCA6408A@0x21`
- `BTN_CENTER -> GPIO0` 的直连关系

### 3.3 自检卡在 `BQ40Z50` 或 `TPS55288`

先查：

- `BQ40Z50` 是否在线
- 放电是否被授权；未授权时 `TPS55288=HOLD` 属于正常门控
- `I2C1` 是否稳定
- `I2C1_INT`、`BMS_BTP_INT_H` 是否可见

### 3.4 充电异常

先查：

- 这次输入是 USB-C / PD 还是 DC
- `BQ25792` 是否可访问
- `CHG_CE`、`CHG_INT`、输入保护路径是否正常
- 页面与日志是否都把问题落在 charger 模块

### 3.5 输出异常或热保护介入

先查：

- `TPS55288-A/B` 状态和寄存器日志
- `INA3221` 的 `vbus/current`
- `TMP112-A/B` 温度与 `THERM_KILL_N`
- 这次刷进去的到底是 `12V` 还是 `19V`

## 4. 常见误判

- 看到屏幕异常就归因于 UI。实际上前面板问题更常见的是 `TCA_RESET#`、`I2C2` 或供电链路。
- 看到 `TPS55288` 不出力就归因于输出级。实际上很多时候上游先卡在 `BQ40Z50` 授权。
- 看到 `INA3221` 数据不对就怀疑固件换算。先排除 `IN+ / IN-` 走线、分流电阻和焊接。

## 5. 调试记录模板

每次 bring-up 建议至少记录：

- 板版本
- 输出设定：`12V` / `19V`
- 本次构建 feature
- 上电方式：电池 / USB-C / DC
- 串口日志关键段落
- `SELF CHECK` 停在哪个模块

## 6. 相关文档

- [BMS 设计](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/bms-design.md)
- [充电器设计](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/charger-design.md)
- [UPS 主输出设计](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/ups-output-design.md)
- [电源监测与保护设计](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/power-monitoring-design.md)
- [固件 bring-up README](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/README.md)
