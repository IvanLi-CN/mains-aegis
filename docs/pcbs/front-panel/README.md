# 前面板 PCB（Front Panel）

本文档基于网表 `docs/pcbs/front-panel/netlist.enet`，用于记录前面板 PCB 的接口、关键网络、器件分工，以及“可恢复性”相关的设计要点（尤其是 `TCA6408A` 对 `CS/RES/TP_RESET` 的控制与复位链路）。

## 1. 范围与组成

前面板 PCB 集成：

- TFT 屏幕（SPI）与电容触摸（I2C；触摸控制器在屏幕模组上）。
- 五向按键（上/下/左/右/中）。
- GPIO 扩展器 `U43(TCA6408A)`：节省 MCU GPIO，并提供屏幕/触摸的复位与恢复钩子。
- 背光电源开关。
- 2× USB‑C（其中一路带 PD 控制器，一路 5V 保护/开关）。

已按“离线优先”存入仓库的数据手册：

- 屏幕驱动 IC：`docs/datasheets/GC9307/GC9307.md`（PDF：`docs/datasheets/GC9307/GC9307.pdf`）
- 触摸 IC：`docs/datasheets/CST816D/CST816D.md`（PDF：`docs/datasheets/CST816D/CST816D.pdf`）

## 2. 关键连接器

### 2.1 `FPC1`（连接主板）

|Pin|Net|说明|
|---:|---|---|
|1|`UCM_DP`|USB1 D+|
|2|`UCM_DM`|USB1 D-|
|3|`GND`||
|4|`SCLK`|屏幕 SPI SCLK|
|5|`MOSI`|屏幕 SPI MOSI（主→从）|
|6|`DC`|屏幕 D/C|
|7|`BLK`|背光控制（见 `Q16`）|
|8|`GND`||
|9|`I2C2_SDA`|共享 I2C2 SDA|
|10|`I2C2_SCL`|共享 I2C2 SCL|
|11|`I2C2_INT`|共享中断线（wired‑OR；见第 6 章）|
|12|`CTP_IRQ`|触摸 IRQ（独立，不与 `I2C2_INT` 共线）|
|13|`TCA_RESET#`|`U43` 复位（低有效）|
|14|`GND`||
|15|`BTN_CENTER`|中键（直连主板 MCU：`ESP32‑S3.GPIO0`）|
|16|`3V3`||
|17|`GND`|额外 GND 焊盘|
|18|`GND`|额外 GND 焊盘|

> 备注：
> - 本 PCB 上没有对 `TCA_RESET#` 的上拉/下拉，请主板端定义该信号；若当前主板未额外放置外部上拉，固件应在正常运行时主动驱高该线，而不是长期高阻释放。
> - 本 PCB 上也没有 `I2C2_SCL/SDA/INT` 的上拉电阻，请主板端提供上拉（见第 6.4 节）。
> - `BTN_CENTER` 为按键到地（前面板 `SW1.COM=GND`），主板侧需提供上拉与消抖策略。

### 2.2 `FPC2`（连接屏幕模组）

|Pin|Net|说明|
|---:|---|---|
|1|`$1N30`|背光供电节点（`3V3` 经 `Q16` 高边开关后输出）|
|2|`GND`||
|3|`DC`|屏幕 D/C|
|4|`MOSI`|屏幕 SPI MOSI|
|5|`CS`|屏幕 CS（来自 `U43`）|
|6|`RES`|屏幕复位（来自 `U43`，低有效）|
|7|`SCLK`|屏幕 SPI SCLK|
|8|`GND`||
|9|`3V3`|屏幕/触摸供电|
|10|`I2C2_SCL`|触摸 I2C2 SCL|
|11|`I2C2_SDA`|触摸 I2C2 SDA|
|12|`CTP_IRQ`|触摸 IRQ（独立）|
|13|`TP_RESET`|触摸复位（来自 `U43`，低有效）|
|14|`GND`||
|15|`NC`|未用|
|16|`NC`|未用|
|17|`GND`|额外 GND 焊盘|
|18|`GND`|额外 GND 焊盘|

> 备注：网表显示 `FPC2` 15–16 未连接。

## 3. GPIO 扩展器 `U43(TCA6408ARGTR)`

### 3.1 端口分配（按网表）

`U43` 在共享的 `I2C2_*` 总线上。其 8 位端口分配如下：

> 地址：网表中 `U43.ADDR` 接 `3V3`，因此 `I2C` 地址为 `0x21`（见 `docs/i2c-address-map.md`）。

|TCA6408A 端口|Net|用途|
|---|---|---|
|P0|`BTN_DOWN`|五向按键：下|
|P1|`BTN_RIGHT`|五向按键：右|
|P2|`BTN_LEFT`|五向按键：左|
|P3|`BTN_UP`|五向按键：上|
|P4|`USB2_PG`|USB2 power-good（来自 `U2(HUSB305-01).STAT`）|
|P5|`CS`|屏幕 CS（作为“使能/闸门”使用）|
|P6|`RES`|屏幕复位（低有效）|
|P7|`TP_RESET`|触摸复位（低有效）|

其他相关引脚：

- `TCA_RESET#`（低有效）来自主板，经 `FPC1` 输入到 `U43`。
- `I2C2_INT` 为 `U43` 的 `INT` 输出（开漏、低有效；见网表里 `U43` 的 TI 数据手册链接）。

### 3.2 为什么把 `CS/RES/TP_RESET` 交给 `TCA6408A` 控制是合理的

这里让扩展器控制的都是**慢控制信号**，而不是高速时序信号：

- `RES` / `TP_RESET` 是复位线，脉宽通常为毫秒级；
- `CS` 在此设计中作为“使能/闸门”用：上电后拉到有效一次并保持稳定，而不是每个 SPI 传输都去翻转它。

因此不会把 I2C 延迟引入 SPI 关键时序路径，同时还能节省 MCU GPIO。

## 4. 默认偏置与上电安全态（100kΩ）

### 4.1 按键输入

`RN1` 与 `RN2` 都是 4 路独立电阻阵列（`4D02WGF1003TCE`，每路 100kΩ）：

- `RN1`：给 `BTN_DOWN/BTN_UP/BTN_RIGHT/BTN_LEFT` 提供 100k 上拉到 `3V3`；
- `RN2`：完成 `CS/RES/TP_RESET` 的默认偏置（见下节），并同时为 `USB2_PG` 提供默认偏置。

> `4D02` 的内部配对关系是 (1–8)、(2–7)、(3–6)、(4–5)。本板的 `RN1/RN2` 连接方式与该配对一致。

### 4.2 屏幕/触摸的默认安全态

`RN2` 对 `U43` 控制的三根线做了默认偏置：

- `CS`：100k 上拉到 `3V3` → 默认不选中屏幕（SPI 访问被屏蔽）。
- `RES`：100k 下拉到 `GND` → 默认让屏幕保持复位。
- `TP_RESET`：100k 下拉到 `GND` → 默认让触摸保持复位。

因为 `TCA6408A` 在上电/复位后端口默认为输入（高阻），所以在以下场景中，最终电平完全由外部 100k 网络决定：

- 上电；
- MCU 复位；
- 主动拉低 `TCA_RESET#`。

## 5. 复位与故障恢复策略（`TCA_RESET#` + `TP_RESET`）

推荐恢复顺序：

1. **仅触摸异常且 I2C 仍可用**：通过 `U43(P7)` 翻转 `TP_RESET`，单独复位触摸（`CST816D`）。
2. **I2C2 疑似被卡死 / 扩展器状态不可信**：由 MCU 直接拉低 `TCA_RESET#`。
   - `U43` 复位后端口转高阻，`RES/TP_RESET` 通过外部 100k 下拉自动为低 → 屏幕 + 触摸一起被复位。
   - `CS` 通过外部 100k 上拉回到高 → SPI 访问被屏蔽。
3. 释放 `TCA_RESET#` 后重新初始化 `U43`，再按需要释放 `RES/TP_RESET`、拉低 `CS` 使能屏幕。

结论：**本板连线已经满足“复位 TCA 时，CTP 也会被复位”**（无需额外门控）。

## 6. `I2C2_*` 共享总线与 `I2C2_INT` 共享中断线

### 6.1 I2C2 共享对象

`I2C2_SCL/SDA` 在前面板上连接了：

- `U43(TCA6408A)`
- `U14(FUSB302B)`
- 屏幕模组上的触摸控制器 `CST816D`（经 `FPC2`）

### 6.2 `I2C2_INT` 共享中断线的必要条件

`I2C2_INT` 网络同时挂了：

- `U43 INT`（开漏、低有效）
- `U14 INT_N`（开漏、低有效；见 `docs/datasheets/FUSB302B/FUSB302B.md`）
- 触摸 `IRQ` **不在** `I2C2_INT` 上：已改走独立 `CTP_IRQ`（`FPC2` Pin12 → `FPC1` Pin12）。

这种“一根中断线挂多个器件”的做法要求所有中断输出都是**开漏（wired‑OR）**，并且由主板侧提供一个上拉电阻。

### 6.3 为什么把触摸 `IRQ` 拆分为 `CTP_IRQ`

我已经核查了仓库内 `CST816D` 数据手册（`docs/datasheets/CST816D/CST816D.pdf`），结论是：

- 引脚表对 `IRQ` 的描述只有 “Interrupt output；Rising/Falling edge selectable”，**未明确** `IRQ` 是开漏、推挽，还是可配置。
- 同一份手册对 `SCL/SDA` 则明确写了 “optional internal pull‑up / open‑drain mode”，因此 `IRQ` 的电气类型在文档层面存在**关键缺口**。
- 手册的 “DC Electrical Performance” 给出了 `Voh/Ioh` 等“高电平输出能力”参数（见 `docs/datasheets/CST816D/CST816D.md` 的电气特性章节），因此**不能把 `IRQ` 当作天然开漏来默认**。

因此在系统级默认把触摸 `IRQ` 视为“可能推挽/未知”，为彻底规避 wired‑OR 风险，触摸中断已从 `I2C2_INT` 拆分为独立 `CTP_IRQ` 线。

### 6.4 上拉电阻位置

网表显示本前面板 PCB 上**没有**为 `I2C2_SCL/I2C2_SDA/I2C2_INT` 放置上拉电阻，因此必须由主板端提供上拉，并在系统级确认总线电容与速率匹配。

## 7. 背光电源开关 `Q16(BSS84)`

- `Q16`（P 沟道 MOSFET）把 `3V3` 切换到 `$1N30`，再送到 `FPC2` Pin1。
- 栅极由 `BLK` 控制（来自主板，经 `FPC1` Pin7）。

网表观察：

- 前面板上没有对 `BLK` 的独立上拉/下拉；这是刻意选择：背光只在 MCU 正常运行时由固件主动控制，MCU 未运行/未供电的状态不以背光状态作为系统约束。
- 若系统进入睡眠仍需保持背光开/关状态，应由固件确保 `BLK(GPIO13)` 维持输出电平（例如 Light-sleep 场景使用 GPIO hold；若使用 Deep-sleep，则需注意并非所有 GPIO 都可控，参见 `docs/manuals/esp32-s3-hardware-design-guidelines/esp32-s3-hardware-design-guidelines.md` 中 “Only GPIOs in the VDD3P3_RTC power domain can be controlled in Deep-sleep mode.” 的约束）。

## 8. 事实来源（网表）

以上网络/引脚分配均来自：

- `docs/pcbs/front-panel/netlist.enet`
