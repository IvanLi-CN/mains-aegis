# ESP32-S3FH4R2 GPIO 引脚分配（mains-aegis）

## 项目概述

本文档为本项目主控 `ESP32-S3FH4R2` 提供 **GPIO 分配的“约束 + 现状 + 预留位”**，用于：

- 在原理图阶段避免踩坑（strapping / USB / in‑package flash&PSRAM 冲突）；
- 给后续固件与硬件联调提供唯一事实来源（不断补齐，不做未确认的分配假设）。

当前为**初步设计阶段**：除下文“已确认引脚分配”章节中列出的引脚外，其余功能引脚**一律不做假设分配**，全部保持 `预留（未分配）`。

> 资料来源以本仓库已入库的 Espressif 文档为准：`docs/datasheets/esp32-s3-fh4r2/` 与 `docs/manuals/esp32-s3-*`。

## 芯片规格

- **型号**：ESP32-S3FH4R2
- **Flash**：4 MB（Quad SPI，in‑package）
- **PSRAM**：2 MB（Quad SPI，in‑package）
- **工作温度**：-40°C ~ 85°C
- **工作电压**：3.3 V
- **可用 GPIO 编号集合**：`GPIO0~GPIO21`、`GPIO26~GPIO48`（共 45 个）

## 内部上拉/下拉（WPU/WPD）简要说明

- `ESP32‑S3` GPIO 支持可配置的**内部弱上拉/弱下拉**（`WPU/WPD`），用于给输入提供默认电平、避免悬空等（配置接口见 `docs/manuals/esp32-s3-hardware-design-guidelines/esp32-s3-hardware-design-guidelines.md` 中对 `WPU/WPD` 的定义，以及 datasheet/TRM 的 IO_MUX 章节）。
- 内部上下拉为**弱**且**非精密**的等效电阻：datasheet 给出的典型值为 `RPU/RPD ≈ 45kΩ`（见 `docs/datasheets/esp32-s3-fh4r2/esp32-s3-fh4r2.md` 的 DC Characteristics 表），实际阻值会随工艺/温度/电压变化。
- 是否默认使能/是否可用受**复位默认配置**与**外设功能占用**影响：例如 USB 引脚在作为普通 GPIO 使用时，内部弱上下拉默认禁用，但可通过 IO_MUX 再配置开启（见 `docs/manuals/esp32-s3-hardware-design-guidelines/esp32-s3-hardware-design-guidelines.md` 与 `docs/datasheets/esp32-s3-fh4r2/esp32-s3-fh4r2.md` 的相关说明）。
- 设计建议：对“上电/复位期间必须确定电平”的脚、长线/强干扰/外部漏电较大、以及需要更强上拉（例如 I2C 上拉）等场景，优先保留外部上/下拉电阻或至少预留焊盘；内部弱上下拉更适合作为默认偏置或软件可控选项。

## 必须标记的不可用/谨慎引脚

### 不可用（in‑package flash/PSRAM 专用）

以下 GPIO 与 flash/PSRAM 总线绑定；在 `ESP32-S3FH4R2`（in‑package flash/PSRAM）上为内部连接，**禁止**分配给任何项目功能：

- `GPIO26`：`SPICS1`（PSRAM CS#）
- `GPIO27`：`SPIHD`
- `GPIO28`：`SPIWP`
- `GPIO29`：`SPICS0`（Flash CS#）
- `GPIO30`：`SPICLK`
- `GPIO31`：`SPIQ`
- `GPIO32`：`SPID`

### 谨慎（Strapping pins，上电/复位采样）

以下 GPIO 在上电/复位时会被采样用于启动配置；如需复用为普通 GPIO，必须保证外部电路在采样窗口不会把电平拉偏：

- `GPIO0`（本项目已用于 BOOT/下载模式控制）
- `GPIO3`（本项目用于 `UPS_IN_CE`；strapping pin；必须保证外部电路在采样窗口不拉偏其默认电平）
- `GPIO45`（strapping pin；已分配给 `UCM_DCE`；必须保证采样窗口为低电平）
- `GPIO46`（strapping pin；已分配给 `UCM_DIN`；必须保证采样窗口为低电平）

### 谨慎（默认调试/下载相关）

- `GPIO19` / `GPIO20`：USB OTG 与 USB Serial/JTAG 默认相关（本项目已固定用于 USB 下载调试数据线）。
- `GPIO43` / `GPIO44`：UART0 常用作下载/日志兜底（本项目**锁定**用于 UART0；建议至少预留测试点/焊盘）。
- `GPIO39~GPIO42`：传统 JTAG 引脚组（本项目 **不外接** 传统 JTAG 排针；本项目已复用该组为项目 IO，需确认不会影响调试策略）。

## 已确认引脚分配（仅此部分为“已分配”）

### USB 下载调试接口（2 个 GPIO）

- `GPIO19`：USB_D-（USB OTG / USB Serial/JTAG；原理图网名常写 `ESP_DM`，若加入 CH442E 则接 `S1B`）
- `GPIO20`：USB_D+（USB OTG / USB Serial/JTAG；原理图网名常写 `ESP_DP`，若加入 CH442E 则接 `S1C`）

说明：以上两脚用于 USB 下载与调试，**不得复用为项目功能 IO**。

### UART0（下载/日志兜底，2 个 GPIO）

- `GPIO43`：`UART0_TXD`
- `GPIO44`：`UART0_RXD`

说明：UART0 引脚组按 `49/50` 号封装引脚**锁定**，避免后续“无日志/无下载通道”。

测试点（必须预留）：

- `TP_UART0_TXD`：连接 `UART0_TXD`（`GPIO43`）
- `TP_UART0_RXD`：连接 `UART0_RXD`（`GPIO44`）
- `TP_GND_UART0`：UART0 调试用地（建议与以上两点相邻）

### USB D+/D- 切换控制（CH442E，2 个 GPIO）

- `GPIO46`：`UCM_DIN`（CH442E `IN`：D+/D- 归属选择；strapping pin）
- `GPIO45`：`UCM_DCE`（CH442E `EN#`：全局使能，低有效；`EN#=1` 时两边都断开；strapping pin）

说明：

- 设计目标：USB‑C 口的 `D+/D-` 可在 **ESP32‑S3（USB 下载调试）** 与 **BQ25792（DPDM/BC1.2 检测）** 间切换，避免“硬并联”导致冲突。
- 推荐切换时序（break‑before‑make）：先置 `UCM_DCE=1`（断开）→ 切 `UCM_DIN` → 再置 `UCM_DCE=0`（接通）。

### 触摸 IRQ（CTP，1 个 GPIO）

- `GPIO14`：`CTP_IRQ`（触摸控制器：`CST816D`；独立触摸中断输入；从 `I2C2_INT` 共享线中拆分）

### 下载模式/系统控制（1 个 GPIO + 1 个专用引脚）

- `CHIP_PU (EN)`：复位/使能控制（建议做按键或测试点）
- `GPIO0`：BOOT（进入下载模式所需；strapping pin，必须按规范做默认上拉/按键下拉等）

### BMS 告警/中断（1 个 GPIO）

- `GPIO21`：`BMS_BTP_INT_H`（`BQ40Z50-R2.BTP_INT`；经 `NMOSFET` 取反后的 BMS 中断线）
- 通信：`BQ40Z50-R2` 挂载在 `I2C2`（`GPIO8/GPIO9`）

说明：

- `BQ40Z50-R2.BTP_INT` 的**有效电平极性可配置**（可配置为高有效或低有效）；本项目约定将其配置为“电池告警发生时使 `GPIO21` 看到高电平”，因此网络名使用 `BMS_BTP_INT_H` 来指代 MCU 侧“高有效”的告警节点（该节点经过 `NMOSFET` **硬件反相**）。

### I2C2（400kHz，3 个 GPIO）

- `GPIO7`：`I2C2_INT`（若面板侧无 I2C 器件，可仅保留为通用输入）
- `GPIO8`：`I2C2_SDA`
- `GPIO9`：`I2C2_SCL`

说明：

- `I2C2` 用于 `BQ40Z50-R2`（SMBus）以及面板侧 I2C 器件（若有）；速率：`400kHz`。

### 前面板 GPIO 扩展器复位（TCA6408A，1 个 GPIO）

- `GPIO1`：`TCA_RESET#`（连接前面板 `TCA6408A.RESET`；低有效；面板侧无上拉）

说明：

- 由于面板侧未放置 `TCA_RESET#` 上拉/下拉，主板侧必须定义该节点的默认电平（外接上拉电阻或 MCU 内部上拉）。
- 推荐固件将该 GPIO 配置为 **开漏输出**：释放为高阻（由上拉保持高电平，正常运行），需要复位时拉低一段时间。

### I2C1（主总线，400kHz，3 个 GPIO，封装 36~38 连续）

- `GPIO48`：`I2C1_SDA`
- `GPIO47`：`I2C1_SCL`
- `GPIO33`：`I2C1_INT`

### 风扇控制（3 个 GPIO，连续）

- `GPIO34`：`FAN_TACH`（转速输入；典型为开漏/开集输出）
- `GPIO35`：`FAN_EN`（风扇电源/驱动使能）
- `GPIO36`：`FAN_VSET_PWM`（PWM 输出→RC 滤波后得到风扇控制电压/控制信号）

### TPS55288 硬停机（过温/强制关断，1 个 GPIO）

- `GPIO40`：`THERM_KILL_N`（开漏线与：`TMP112A×2.ALERT`；同一 GPIO 可作为输入接收过温告警，也可配置为开漏输出拉低以强制双路 `TPS55288` 停机；详见 `docs/power-monitoring-design.md`）

### 充电器控制/中断（BQ25792，3 个 GPIO，连续）

- `GPIO15`：`CHG_CE`（连接 `BQ25792.CE`；低有效；默认上拉禁充，MCU 确认电池存在且 BMS 允许后再拉低使能）
- `GPIO16`：`CHG_ILIM_HIZ_BRK`（`ILIM_HIZ`“刹车”控制：驱动 `NX7002BKWX` 下拉 `BQ25792.ILIM_HIZ` 到 GND，使其进入非开关模式；默认低电平不刹车）
- `GPIO17`：`CHG_INT`（连接 `BQ25792.INT`；开漏；低有效 `256µs` 脉冲；不与电平型告警共线）

### INA3221 告警输出（3 个 GPIO，连续）

- `GPIO37`：`INA3221_PV`（`INA3221.PV`；开漏/电平型告警：欠压时拉低；`VPU=3.3V`）
- `GPIO38`：`INA3221_CRITICAL`（`INA3221.CRITICAL`；开漏/电平型告警：单次转换/可配置求和超限，超限拉低）
- `GPIO39`：`INA3221_WARNING`（`INA3221.WARNING`；开漏/电平型告警：平均值告警，超限拉低）

说明：

- 三个告警脚均为开漏输出；必须按各自上拉域配置上拉（`PV` 的高电平由 `VPU` 决定）。
- `PV` 为电平型告警（欠压持续时会持续拉低）；不与其它“需要可靠捕获的脉冲中断”共线。
- 上拉策略：固件侧可启用 MCU GPIO **内部弱上拉**作为默认；但 PCB 建议**预留可选偏置/上拉电阻焊盘**（典型 `10kΩ`；`PV→VPU=3.3V`；`WARNING/CRITICAL→3.3V`），用于后期抗干扰加固与问题定位。

### UPS 输入侧控制/状态（2 个 GPIO）

- `GPIO3`：`UPS_IN_CE`（**开漏**控制：通过 `NX7002BKWX` 将输入侧 `EN` 控制节点**拉低以禁用**；释放（高阻）后由电阻网络决定 `EN` 电平）
- `GPIO2`：`UPS_IN_PG`（输入侧 Power‑Good 指示；原理图来自 `TPS2490.PG`）

说明：

- `UPS_IN_CE`：截图标注为“开漏 IO 控制，拉低禁用”。当 MCU 侧处于上电复位/高阻态时，`TPS2490.EN` 可能会随 `VIN_UNSAFE` 与分压网络自动满足阈值而使能；若需要“上电默认禁用”，应在固件尽早将该脚配置为开漏输出并拉低，或在硬件上增加默认下拉策略。
- `UPS_IN_PG` 为输入侧状态信号；固件侧通常按“输入有效/无效”做保护策略（例如限制后级启动/上报告警/降额）。

### 交错 SYNC（180° 反相方波，2 个 GPIO）

- `GPIO41`：`SYNCA`（相位 0°）
- `GPIO42`：`SYNCB`（相位 180°；与 `SYNCA` 互补）

说明：

- 目标频率：`400–600kHz`（最高 `2.2MHz`）。
- 连接关系：`SYNCA` / `SYNCB` 分别连接到两颗 `TPS55288.DITH/SYNC`，作为外部同步时钟以实现两相 interleave（详见 `docs/ups-output-design.md`）。
- 这两脚属于传统 JTAG 组（`GPIO39~GPIO42`）；本项目不外接传统 JTAG，因此复用为项目 IO。

### 固定分配（原理图红框：SPI + BLK，4 个 GPIO）

- `GPIO10`：`DC`
- `GPIO11`：`MOSI`
- `GPIO12`：`SCLK`
- `GPIO13`：`BLK`

说明：

- `CS/RES` 已改由面板 PCB 的 `TCA6408A` 提供，因此 MCU 侧不再分配 `GPIO13/GPIO14` 给 `CS/RES`。

## 引脚使用统计（当前阶段）

| 功能类别 | 数量 | 引脚 |
|---|---:|---|
| USB 下载调试 | 2 | `GPIO19`、`GPIO20` |
| USB D+/D- 切换控制 | 2 | `GPIO45`、`GPIO46` |
| 触摸 IRQ（CTP） | 1 | `GPIO14` |
| 下载模式/系统控制 | 1 + EN | `GPIO0`、`CHIP_PU (EN)` |
| BMS 告警/中断 | 1 | `GPIO21` |
| I2C1（400kHz） | 3 | `GPIO48`、`GPIO47`、`GPIO33` |
| I2C2（400kHz） | 3 | `GPIO7~GPIO9` |
| 前面板 TCA 复位 | 1 | `GPIO1` |
| TPS55288 硬停机（过温/强制关断） | 1 | `GPIO40` |
| UART0（下载/日志兜底） | 2 | `GPIO43`、`GPIO44` |
| 音频/提示音（TDM/I2S） | 3 | `GPIO4~GPIO6` |
| 充电器控制/中断（BQ25792） | 3 | `GPIO15~GPIO17` |
| INA3221 告警输出（PV/WARNING/CRITICAL） | 3 | `GPIO37~GPIO39` |
| 风扇控制 | 3 | `GPIO34~GPIO36` |
| UPS 输入侧控制/状态 | 2 | `GPIO2`、`GPIO3` |
| 固定分配（SPI + BLK） | 4 | `GPIO10~GPIO13` |
| 交错 SYNC（180°） | 2 | `GPIO41`、`GPIO42` |
| 不可用 | 7 | `GPIO26~GPIO32` |

## 音频/提示音（TDM（I2S 外设） -> 数字功放 -> Speaker）

本项目提示音计划从“无源蜂鸣器”升级为 “`TDM + MAX98357A + Speaker`”（详见 `docs/audio-design.md`）。

说明：
- TDM TX 最少需要 3 个 GPIO：`BCLK/LRCLK/DOUT`（在 TDM 中 `LRCLK` 作为帧同步 `WS` 使用；网名保持 `AUDIO_I2S_LRCLK` 不变）。
- 为了提升兼容性，本项目计划**同时保留蜂鸣器与数字功放电路**，并把 GPIO 成本控制为 **3 根**：其中 1 根 GPIO 需要“二选一复用”（TDM 数据线 / 蜂鸣器 PWM）。
- 由于 `GPIO19/20` 固定用于 USB 下载调试，音频相关信号建议优先放在远离 USB 差分对的预留 GPIO 上，降低串扰/走线压力。

GPIO 分配（已确认）：

- `GPIO4`：`AUDIO_I2S_BCLK`
- `GPIO5`：`AUDIO_I2S_LRCLK`
- `GPIO6`：`AUDIO_I2S_DOUT` / `BUZZ_PWM`（二选一复用）

## 引脚快速查找表（按封装引脚编号）

> 约定：**状态**取值：`已分配` / `预留` / `不可用` / `谨慎`

| GPIO | 封装引脚编号 | 状态 | 用途/功能 | 备注 |
|---:|---:|---|---|---|
| EN | 4 | 已分配 | `CHIP_PU (EN)` | 复位/使能；建议按键或测试点 |
| 0 | 5 | 已分配 | BOOT | strapping pin；用于进入下载模式 |
| 1 | 6 | 已分配 | `TCA_RESET#` | 前面板 `TCA6408A.RESET`（低有效）；面板侧无上拉；主板侧需定义上拉；建议开漏输出 |
| 2 | 7 | 已分配 | `UPS_IN_PG` | 输入侧 Power‑Good 指示（来自 `TPS2490.PG`） |
| 3 | 8 | 谨慎 | `UPS_IN_CE` | UPS 输入侧 `EN` 控制（开漏；拉低禁用；经 `NX7002BKWX` 下拉 `EN` 节点）；strapping pin |
| 4 | 9 | 已分配 | `AUDIO_I2S_BCLK` | 音频 TDM/I2S 位时钟；详见 `docs/audio-design.md` |
| 5 | 10 | 已分配 | `AUDIO_I2S_LRCLK` | 音频 TDM/I2S 帧同步（WS，网名保持不变）；详见 `docs/audio-design.md` |
| 6 | 11 | 已分配 | `AUDIO_I2S_DOUT` / `BUZZ_PWM` | 二选一复用：TDM TX 数据输出或蜂鸣器 PWM；详见 `docs/audio-design.md` |
| 7 | 12 | 已分配 | `I2C2_INT` | 原理图红框；不得复用；I2C2 相关 |
| 8 | 13 | 已分配 | `I2C2_SDA` | 原理图红框；不得复用；I2C2 相关 |
| 9 | 14 | 已分配 | `I2C2_SCL` | 原理图红框；不得复用；I2C2 相关 |
| 10 | 15 | 已分配 | `DC` | 固定分配（原理图红框；不得复用） |
| 11 | 16 | 已分配 | `MOSI` | 固定分配（原理图红框；不得复用） |
| 12 | 17 | 已分配 | `SCLK` | 固定分配（原理图红框；不得复用） |
| 13 | 18 | 已分配 | `BLK` | 固定分配（原理图红框；不得复用） |
| 14 | 19 | 已分配 | `CTP_IRQ` | 触摸控制器：`CST816D`；独立触摸 IRQ（从 `I2C2_INT` 共享线中拆分） |
| 15 | 21 | 已分配 | `CHG_CE` | `BQ25792.CE`（低有效；默认上拉禁充） |
| 16 | 22 | 已分配 | `CHG_ILIM_HIZ_BRK` | `ILIM_HIZ` 刹车控制（驱动 `NX7002BKWX` 下拉） |
| 17 | 23 | 已分配 | `CHG_INT` | `BQ25792.INT`；开漏；低有效 `256µs` 脉冲（独立 GPIO） |
| 18 | 24 | 预留 | — | — |
| 19 | 25 | 已分配 | USB_D- | USB 下载调试（不得复用；原理图网名常写 `ESP_DM`，若加入 CH442E 则接 `S1B`） |
| 20 | 26 | 已分配 | USB_D+ | USB 下载调试（不得复用；原理图网名常写 `ESP_DP`，若加入 CH442E 则接 `S1C`） |
| 21 | 27 | 已分配 | `BMS_BTP_INT_H` | BMS 中断线（`BQ40Z50-R2.BTP_INT` 经 `NMOSFET` 取反；MCU 侧约定高有效） |
| 26 | 28 | 不可用 | — | in‑package flash/PSRAM 专用 |
| 27 | 30 | 不可用 | — | in‑package flash/PSRAM 专用 |
| 28 | 31 | 不可用 | — | in‑package flash/PSRAM 专用 |
| 29 | 32 | 不可用 | — | in‑package flash/PSRAM 专用 |
| 30 | 33 | 不可用 | — | in‑package flash/PSRAM 专用 |
| 31 | 34 | 不可用 | — | in‑package flash/PSRAM 专用 |
| 32 | 35 | 不可用 | — | in‑package flash/PSRAM 专用 |
| 48 | 36 | 已分配 | `I2C1_SDA` | I2C1（400kHz）相关；封装 `36~38` 连续块 |
| 47 | 37 | 已分配 | `I2C1_SCL` | I2C1（400kHz）相关；封装 `36~38` 连续块 |
| 33 | 38 | 已分配 | `I2C1_INT` | I2C1 中断汇总（开漏线与；`INT` 放最后）；封装 `36~38` 连续块 |
| 34 | 39 | 已分配 | `FAN_TACH` | 转速输入；典型为开漏/开集输出 |
| 35 | 40 | 已分配 | `FAN_EN` | 风扇电源/驱动使能 |
| 36 | 41 | 已分配 | `FAN_VSET_PWM` | PWM 输出→RC 滤波后得到风扇控制电压/控制信号 |
| 37 | 42 | 已分配 | `INA3221_PV` | `INA3221.PV`；开漏/电平型告警（欠压时拉低）；`VPU=3.3V` |
| 38 | 43 | 已分配 | `INA3221_CRITICAL` | `INA3221.CRITICAL`；开漏/电平型告警（单次转换/可配置求和超限，超限拉低） |
| 39 | 44 | 已分配 | `INA3221_WARNING` | `INA3221.WARNING`；开漏/电平型告警（平均值告警，超限拉低） |
| 40 | 45 | 已分配 | `THERM_KILL_N` | 过温硬停机线（开漏线与：`TMP112A×2.ALERT`）；同一 GPIO 可开漏拉低强制双路 `TPS55288` 停机 |
| 41 | 47 | 已分配 | `SYNCA` | 交错 SYNC：相位 0°；复用传统 JTAG 引脚组 |
| 42 | 48 | 已分配 | `SYNCB` | 交错 SYNC：相位 180°；复用传统 JTAG 引脚组 |
| 43 | 49 | 已分配 | `UART0_TXD` | UART0 TX；锁定使用；测试点：`TP_UART0_TXD` |
| 44 | 50 | 已分配 | `UART0_RXD` | UART0 RX；锁定使用；测试点：`TP_UART0_RXD` |
| 45 | 51 | 谨慎 | `UCM_DCE` | CH442E `EN#`：全局使能（低有效；`1` 时两边断开）；strapping pin（VDD_SPI 相关） |
| 46 | 52 | 谨慎 | `UCM_DIN` | CH442E `IN`：USB D+/D- 归属选择；strapping pin（boot/ROM 打印等相关） |

## 待分配列表（网络已冻结；GPIO 待定）

（当前无）
