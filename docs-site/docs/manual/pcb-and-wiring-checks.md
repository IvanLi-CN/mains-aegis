---
title: PCB 与连线检查
description: 上电前应先完成的板级检查与关键连接确认。
---

# PCB 与连线检查

在任何烧录或上电动作之前，先把板级检查做扎实。对于这种同时包含电池、充电、主输出、前面板和多路总线的系统，很多后续“功能异常”其实都能在这一阶段提前发现。

## 先确认系统分成哪两块板

### 主板

主板承载：

- 电池座与 BMS
- 充电与输入路径
- 可编程主输出与监测
- 风扇、音频与前面板互连

### 前面板

前面板承载：

- TFT 屏幕
- 触摸与按键
- 背光控制
- `TCA6408A` 扩展器
- 与主板之间的 `FPC` 连接

## 上电前最值得检查的几件事

### 1. 板间连接是否对齐

优先确认前面板 `FPC1` 与主板互连的这些信号没有接反、短路或虚焊：

- `3V3`
- `I2C2_SCL / I2C2_SDA / I2C2_INT`
- `CTP_IRQ`
- `TCA_RESET#`
- `DC / MOSI / SCLK`
- `BLK`
- `BTN_CENTER`

### 2. 电池相关路径是否满足基本边界

- 电芯抽头顺序是否与 `VC1..VC4` 对应。
- `AGND / CHGND` 相关路径是否存在明显焊接问题。
- 不要把“可拆装电池座”理解成支持随意带电热插拔。

### 3. 输入 / 输出接口是否分清

- `VIN_UNSAFE -> VIN` 是输入路径，不要和主输出 `VOUT` 混淆。
- `U4` 对外是 UPS 主输出，和 charger 的内部 `SYS/VSYS` 语义不同。

### 4. 屏幕与触摸复位链路是否完整

前面板上的 `CS / RES / TP_RESET` 与 `TCA_RESET#` 关系非常关键。若这些链路有问题，后续很容易表现为：

- 屏幕不亮
- 触摸不响应
- I2C2 看似异常
- 自检页无法稳定进入

### 5. I2C 上拉与共享中断线是否按系统预期存在

仓库文档明确说明了一部分上拉是在主板侧提供的。复刻时，不要默认“前面板上自己就全有”。

## 一个实用的检查顺序

1. 不接电池、不上电，先做短路与连通检查。
2. 对照前面板 / 主板文档，核对关键连接器与网络名。
3. 再检查电源路径、FPC 连接、屏幕/触摸复位链路。
4. 最后才进入固件烧录与首次上电。

## 延伸阅读

- [主板 PCB 说明](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/pcbs/mainboard/README.md)
- [前面板 PCB 说明](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/pcbs/front-panel/README.md)
- [EDA 备注](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/pcbs/eda-notes.md)
