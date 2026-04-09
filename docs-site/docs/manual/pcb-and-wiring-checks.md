---
title: PCB 与连线检查
description: 上电前的板级静态检查项与通过标准。
---

# PCB 与连线检查

## 1. 检查原则

首次上电前只做两类检查：

- **方向是否对**：输入、输出、抽头、FPC 针序、复位链路不能接反
- **电气上是否自洽**：关键网络连通，关键网络之间不短，默认偏置符合设计

## 2. 必须先分清的接口

| 接口 | 实际作用 | 常见误判 |
| --- | --- | --- |
| `H1` | 电池抽头，给 `VC1..VC4` 采样 | 抽头顺序接反 |
| `U16` | DC 输入，`VIN_UNSAFE -> VIN` | 把它当成输出口 |
| `U4` | UPS 输出，网络名 `VOUT` | 误认为是 `BQ25792 SYS/VSYS` |
| `FPC1` | 主板和前面板互连 | 只看外形，不核对镜像针序 |

## 3. 主板静态检查

### 3.1 电池与 BMS

- `H1.B0..B4` 必须对应 `AGND / VC1 / VC2 / VC3 / VC4`
- `AGND` 与 `CHGND` 不能误短成大面积直连，也不能断开
- `SRP/SRN` 网络必须完整：`R38`、`R40`、`C40` 在位且无桥连
- `BQ40Z50`、`BQ296100DSGR`、均衡 MOS 和均衡电阻无反装

### 3.2 输入 / 输出路径

- `U16 -> U10(TPS2490) -> VIN` 路径正确
- `U17/U18 -> R68/R83 -> VOUT_TPS -> U21/Q28 -> VOUT` 路径正确
- `Q11` 所在的 `VIN -> VOUT` 直通路径没有被误改成反向

## 4. `FPC1` 检查表

| 网络 | 应满足的条件 |
| --- | --- |
| `3V3` | 主板到前面板连续 |
| `BTN_CENTER` | 直达 `ESP32-S3.GPIO0` |
| `TCA_RESET#` | 主板到 `TCA6408A` 连通 |
| `CTP_IRQ` | 独立存在，不与 `I2C2_INT` 短接 |
| `I2C2_SCL / I2C2_SDA` | 连通，且上拉只在主板侧 |
| `I2C2_INT` | 连通，且确认没有误接推挽源 |
| `BLK` | 主板到前面板背光控制线连通 |
| `DC / MOSI / SCLK` | SPI 三根线逐一对应 |
| `UCM_DP / UCM_DM` | 差分对没有交叉或断线 |

## 5. 前面板静态检查

### 5.1 复位链路

要核对的不是“有没有线”，而是默认电平是否符合设计：

- `CS` 默认上拉
- `RES` 默认下拉
- `TP_RESET` 默认下拉
- `TCA_RESET#` 拉低后，上述默认态必须重新成立

如果这里不对，首次上电常见现象就是：屏不亮、触摸不通、`I2C2` 看起来像死总线。

### 5.2 总线与中断

- `I2C2_INT` 只允许接 `TCA6408A.INT` 和 `FUSB302B.INT_N` 这类开漏源
- 触摸中断单独走 `CTP_IRQ`
- 前面板本板不放 `I2C2` 上拉，缺上拉要去主板查，不要在前面板盲目补焊

## 6. 推荐检查顺序

1. 完全不上电，做短路和连通检查
2. 核对 `H1`、`U16`、`U4`、`FPC1` 的定义
3. 核对 `TCA_RESET#`、`CTP_IRQ`、`I2C2_*`、`BLK`、SPI
4. 最后再看主输出链路和输入保护路径

## 7. 上电前通过标准

- 没有肉眼可见的反装、虚焊、桥连
- `FPC1` 关键网络全部连通
- `H1` 抽头顺序正确
- `U16` 与 `U4` 未混接
- `TCA_RESET# / I2C2_INT / CTP_IRQ` 的角色清楚且接法正确

## 8. 相关文档

- [主板 PCB 说明](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/pcbs/mainboard/README.md)
- [前面板 PCB 说明](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/pcbs/front-panel/README.md)
- [EDA 备注](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/pcbs/eda-notes.md)
