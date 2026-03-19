# 主板（PCB）文档

本目录存放主板 PCB 的设计说明与实施备注（以网表/原理图为事实来源）。

主板网表（事实来源）：

- `docs/pcbs/mainboard/netlist.enet`

本文档目标：把“主板包含哪些功能块、哪些关键网络/接口”从网表中提炼出来，便于与系统级设计文档对齐：

- BMS：`docs/bms-design.md`
- 充电：`docs/charger-design.md`
- UPS 输出：`docs/ups-output-design.md`
- 电源监控/保护：`docs/power-monitoring-design.md`
- 音频：`docs/audio-design.md`
- I2C 地址：`docs/i2c-address-map.md`

## 1. 范围与组成（按网表）

主板包含（按器件与网络可直接确认的部分）：

- 4S 电池座（`H1`）与电芯均衡/保护相关外围（`U1(BQ40Z50RSMR-R2)`、`U7(BQ296100DSGR)`、均衡 MOSFET `Q6/Q7/Q10/Q12` + 放电电阻 `R14/R19/R25/R27` 等）。
- 充电与电源路径：`U11(BQ25792RQMR)`（双输入；`VAC1=UCM_VBUS`，`VAC2=VIN`；`SYS=VSYS`）。
- 外部输入防护/热插拔：`U10(TPS2490DGSR)`（`VIN_UNSAFE -> VIN`，`PG=UPS_IN_PG`）。
- 两路可编程升降压输出：`U17/U18(TPS55288RPMR)`（共享输出网络 `VOUT_TPS`，电流采样网络 `ISP_TPSA/ISP_TPSB`）。
- 电源监控：`U22(INA3221AIRGVR)`（告警 `INA3221_PV/CRITICAL/WARNING`）。
- 温度监控：`U23/U24(TMP112AIDRLR)`。
- 系统电源轨：`U19/U20(TPS62933DRLR)` 从 `VSYS` 生成 `+5V` 与 `3V3`（`EN=VSYS_OK`）。
- 音频：`U29(MAX98357AETE+T)` + 喇叭连接器 `U30`；以及 `BUZZER1`（蜂鸣器）与其驱动级。
- 前面板互连：`FPC1`（连接 `docs/pcbs/front-panel/`）。
- USB2 数据/DPDM 二选一切换：`U13(CH442E)`（`UCM_DP/DM` 在 `MCU_DP/DM` 与 `CHG_DP/DM` 之间切换）。

## 2. 关键连接器与引脚网络（按网表 pin->net）

### 2.1 `FPC1`（前面板互连，16P 信号 + 2xGND 焊盘，0.5mm）

该连接器的网络命名与 `docs/pcbs/front-panel/README.md` 完全一致（主板侧提供 `I2C2_*` 上拉）。

|Pin|Net|说明（按网名）|
|---:|---|---|
|1|`3V3`|3.3V|
|2|`BTN_CENTER`|中键|
|3|`CHGND`|GND|
|4|`TCA_RESET#`|面板 IO 扩展器复位|
|5|`CTP_IRQ`|触摸 IRQ（独立）|
|6|`I2C2_INT`|I2C2 共享中断线|
|7|`I2C2_SCL`|I2C2 SCL|
|8|`I2C2_SDA`|I2C2 SDA|
|9|`CHGND`|GND|
|10|`BLK`|背光控制|
|11|`DC`|屏幕 D/C|
|12|`MOSI`|SPI MOSI（屏幕）|
|13|`SCLK`|SPI SCLK（屏幕）|
|14|`CHGND`|GND|
|15|`UCM_DM`|USB D-（通往 `U13`）|
|16|`UCM_DP`|USB D+（通往 `U13`）|
|17|`CHGND`|GND（额外焊盘）|
|18|`CHGND`|GND（额外焊盘）|

> 备注：主板与前面板的 `FPC1` 编号方向在当前网表中为镜像配对关系，因此主板侧 pin 序与 `docs/pcbs/front-panel/README.md` 中的前面板侧 pin 表不同；两侧以各自网表为事实来源，网名保持一一对应。

### 2.2 `H1`（4S 电池座/电芯抽头）

网表中 `H1` 提供 `VC1..VC4` 与 `AGND` 抽头（电芯电压采样/均衡相关）。

|Pin|Net|
|---:|---|
|B0|`AGND`|
|B1|`VC1`|
|B2|`VC2`|
|B3|`VC3`|
|B4|`VC4`|

### 2.3 `U16`（DC 输入插座）

|Pin|Net|说明|
|---:|---|---|
|1|`VIN_UNSAFE`|未经过热插拔/限流前的输入|
|2|`CHGND`|GND|
|3|`CHGND`|GND|

### 2.4 `U4`（DC 输出插座）

|Pin|Net|说明|
|---:|---|---|
|1|`VOUT`|UPS 主输出母线（网名为 `VOUT`）|
|2|`CHGND`|GND|
|3|`CHGND`|GND|

### 2.5 `U5`（风扇 1x3 连接器）

|Pin|Net|说明（按网名）|
|---:|---|---|
|1|`FAN_VCC`|风扇电源|
|2|`FAN_TACH`|风扇转速反馈|
|3|`CHGND`|GND|

### 2.6 `U30`（喇叭 1x2 信号 + 2xGND 焊盘）

|Pin|Net|说明（按网名）|
|---:|---|---|
|1|`$2N152`|`U29.OUTP`|
|2|`$2N153`|`U29.OUTN`|
|3|`CHGND`|GND（焊盘）|
|4|`CHGND`|GND（焊盘）|

> 备注：`MAX98357A` 为 BTL 输出，喇叭两端应分别接 `OUTP/OUTN`，不要把喇叭负端接地（详见 `docs/audio-design.md`）。

### 2.7 `H2`（VBUS 汇流排针 1x6）

|Pin|Net|
|---:|---|
|1|`UCM_VBUS`|
|2|`UCM_VBUS`|
|3|`UCM_VBUS`|
|4|`CHGND`|
|5|`CHGND`|
|6|`CHGND`|

### 2.8 `VOUT_TPS`（TPS55288 共享输出节点）

当前网表不再包含 `J1/J2/J3` 焊盘跳线。两路 `TPS55288` 的输出都汇到共享节点 `VOUT_TPS`，再经后级理想二极管/功率 MOSFET 路径接入主输出 `VOUT`：

- `U17.VOUT = VOUT_TPS`
- `U18.VOUT = VOUT_TPS`
- `U21(MX5050L)` + `Q28`：`VOUT_TPS -> VOUT`

## 3. 电源/地与关键网络（对齐系统设计）

### 3.1 `AGND` vs `CHGND`（BMS 采样地分割）

网表显示 `R42=1mΩ` 直接连接 `AGND` 与 `CHGND`：

- `AGND`：`U1(BQ40Z50).VSS`、`U7(BQ296100).VSS`、`SRP` 侧等低电流参考地。
- `CHGND`：系统大电流地（充电器、电源、功放、DC 口等）。

与 `docs/bms-design.md` 中“`SRP/SRN` 采样 + Kelvin + 单点/低阻连接”的原则一致。

### 3.2 Rsense / `SRP` / `SRN` 滤波（BQ40Z50）

网表中 `SRP/SRN` 具备典型的差分滤波网络：

- `R38=100Ω`：`SRP` 串阻
- `R40=100Ω`：`SRN` 串阻
- `C40=0.1uF`：跨接 `SRP` 与 `SRN`

这与 `docs/bms-design.md` 对 `BQ40Z50-R2` 的推荐连接一致。

### 3.3 主输出 `VOUT` 与共享输出 `VOUT_TPS`

网表中 `VOUT`（DC 输出口 `U4`）与两路 `TPS55288` 的关系可直接从共享输出节点与后级理想二极管路径看出：

- `U17/U18`：两路输出都接到 `VOUT_TPS`
- `U21(MX5050L)` + `Q28`：`VOUT_TPS -> VOUT`
- `Q11`：`VIN -> VOUT`（外部输入直通到输出母线的路径）

同时对外侧有 TVS：

- `D15`：`VOUT_TPS` 对地 TVS
- `D1`：`VOUT` 对地 TVS

### 3.4 充电/系统电源 `VSYS`、`+5V`、`3V3`

- `U11(BQ25792).SYS -> VSYS`
- `U19(TPS62933).VIN=VSYS`，输出经 `L4` 形成 `+5V`
- `U20(TPS62933).VIN=VSYS`，输出经 `L6` 形成 `3V3`
- `U19/U20.EN` 同连到 `VSYS_OK`（由 `R98(56k) -> VSYS` 与 `R97(330k) -> CHGND` 形成分压节点）

### 3.5 外部输入：`VIN_UNSAFE -> VIN`

网表中 DC 输入口 `U16` 输出为 `VIN_UNSAFE`，经 `U10(TPS2490)` 变为 `VIN`：

- `U10.VCC = VIN_UNSAFE`
- `U10.OUT = VIN`
- `U10.PG = UPS_IN_PG`（上拉由 `RN5` 提供）
- `U10.EN` 由 `UPS_IN_CE` 通过 `Q23` 拉控（`UPS_IN_CE` 默认下拉由 `RN5` 提供）

## 4. 总线与中断/告警（从网表抽取）

### 4.1 I2C1（主板内设）

`I2C1_SCL/I2C1_SDA` 连接到：

- `U1(BQ40Z50)`、`U6(M24C64)`、`U11(BQ25792)`、`U17/U18(TPS55288)`、`U22(INA3221)`、`U23/U24(TMP112)`、`U9(ESP32-S3)`

上拉：`RN3`（4.7k）。

`I2C1_INT` 连接到：

- `U17/U18.FB/INT`、`U9(GPIO33=I2C1_INT)`

上拉：`RN7`（10k），并同时为 `INA3221_{PV,CRITICAL,WARNING}` 提供上拉（同一个电阻阵列）。

### 4.2 I2C2（主板 <-> 前面板）

`I2C2_SCL/I2C2_SDA/I2C2_INT` 连接到：

- `FPC1`（通往前面板）
- `U9(ESP32-S3)`

上拉：`RN4`（4.7k），该阵列还同时把 `CHG_INT` 上拉到 `3V3`。

### 4.3 `ESP32-S3` 关键网络映射（按网表网名）

仅列出主板/系统级关键网络（便于与 `docs/i2c-address-map.md`、`docs/hardware-selection/esp32-s3-fh4r2-gpio.md` 对齐）：

|U9 引脚名（符号）|Net|
|---|---|
|`GPIO0`|`BTN_CENTER`|
|`GPIO1`|`TCA_RESET#`|
|`GPIO2`|`UPS_IN_PG`|
|`GPIO3`|`UPS_IN_CE`|
|`GPIO4`|`AUDIO_I2S_BCLK`|
|`GPIO5`|`AUDIO_I2S_LRCLK`|
|`GPIO6`|`AUDIO_I2S_DOUT`|
|`GPIO7`|`I2C2_INT`|
|`GPIO8`|`I2C2_SDA`|
|`GPIO9`|`I2C2_SCL`|
|`GPIO10`|`DC`|
|`GPIO11`|`MOSI`|
|`GPIO12`|`SCLK`|
|`GPIO13`|`BLK`|
|`GPIO14`|`CTP_IRQ`|
|`GPIO17`|`CHG_INT`|
|`GPIO21`|`BMS_BTP_INT_H`|
|`GPIO33`|`I2C1_INT`|
|`GPIO37`|`INA3221_PV`|
|`GPIO38`|`INA3221_CRITICAL`|
|`MTCK`|`INA3221_WARNING`|
|`MTDO`|`THERM_KILL_N`|
|`GPIO45`|`UCM_DCE`|
|`GPIO46`|`UCM_DIN`|
|`SPICLK_P`|`I2C1_SCL`|
|`SPICLK_N`|`I2C1_SDA`|
|`XTAL_32K_P`|`CHG_CE`|
|`XTAL_32K_N`|`CHG_ILIM_HIZ_BRK`|

> 备注：上述表格仅陈述“网表中网络与符号引脚名的对应关系”。如果后续需要启用 `32k XTAL` 或 JTAG 功能，则这些复用关系需要重新评估。

## 5. 与项目设计文档的对齐检查（结论）

可确认一致的点（网表足以证明）：

- 关键芯片选型与仓库设计文档一致：`BQ40Z50-R2`、`BQ25792`、`TPS55288 x2`、`INA3221`、`TMP112 x2`、`MAX98357A`、`TPS2490`、`ESP32-S3`。
- `FPC1` 的网络命名与前面板文档一致，并且主板侧确实放置了 `I2C2` 上拉（符合“前面板不放上拉”的约定）。
- `BMS_BTP_INT` 的高电平中断在主板侧经过 `Q24` 反相为 `BMS_BTP_INT_H`（与 `docs/i2c-address-map.md` 的描述一致）。
- `SRP/SRN` 滤波网络、以及 `AGND`/`CHGND` 通过 `1mΩ` 连接的地方案，符合 `docs/bms-design.md` 的实现思路。
- `CH442E` 把前面板 USB2 D+/D- 在 `ESP32-S3(USB)` 与 `BQ25792(DPDM)` 间二选一切换，符合 `docs/charger-design.md` 的“不要硬并联，需切换”的原则。

需要你确认取舍的一点（网表与系统文档存在“实现方式差异”）：

- `BUZZER1` 的驱动 MOSFET `Q21.G` 直接接在 `AUDIO_I2S_DOUT` 上（与 `U29.DIN` 同网名），等价于“蜂鸣器与功放共享同一根数据线”。这与 `docs/audio-design.md` 中建议的“蜂鸣器与 TDM/功放二选一隔离（跳线/模拟开关）”不同：  
  - 若你期望蜂鸣器仅作为“独立 BUZZ_PWM”提示音输出：需要硬件上把它与 `AUDIO_I2S_DOUT` 解耦（例如按文档建议做二选一）。  
  - 若你接受“蜂鸣器作为额外发声器件，播放与喇叭同源的音频”：当前网表实现是自洽的。

## 索引

- VIN 载流加固（L1 铜迹线 + 表层金属片）：`docs/pcbs/mainboard/vin-bus-reinforcement.md`
