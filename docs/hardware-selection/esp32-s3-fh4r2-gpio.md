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
- `GPIO3`（本项目用于 `FAN_EN`；strapping pin，必须保证风扇驱动侧电路在采样窗口不干扰其默认电平）
- `GPIO45`、`GPIO46`（当前预留，后续若使用必须专项评审）

### 谨慎（默认调试/下载相关）

- `GPIO19` / `GPIO20`：USB OTG 与 USB Serial/JTAG 默认相关（本项目已固定用于 USB 下载调试数据线）。
- `GPIO43` / `GPIO44`：UART0 常用作下载/日志兜底（本项目当前不使用，但建议至少预留测试点/焊盘）。
- `GPIO39~GPIO42`：传统 JTAG 引脚组（本项目 **不外接** 传统 JTAG 排针；若后续复用为项目 IO，需要确认不会影响调试策略）。

## 已确认引脚分配（仅此部分为“已分配”）

### USB 下载调试接口（2 个 GPIO）

- `GPIO19`：USB_D-（USB OTG / USB Serial/JTAG；原理图网名常写 `ESP_DM`，若加入 CH442E 则接 `S1B`）
- `GPIO20`：USB_D+（USB OTG / USB Serial/JTAG；原理图网名常写 `ESP_DP`，若加入 CH442E 则接 `S1C`）

说明：以上两脚用于 USB 下载与调试，**不得复用为项目功能 IO**。

### USB D+/D- 切换控制（CH442E，2 个 GPIO）

- `GPIO4`：`UCM_DIN`（CH442E `IN`：D+/D- 归属选择）
- `GPIO5`：`UCM_DCE`（CH442E `EN#`：全局使能，低有效；`EN#=1` 时两边都断开）

说明：

- 设计目标：USB‑C 口的 `D+/D-` 可在 **ESP32‑S3（USB 下载调试）** 与 **BQ25792（DPDM/BC1.2 检测）** 间切换，避免“硬并联”导致冲突。
- 推荐切换时序（break‑before‑make）：先置 `UCM_DCE=1`（断开）→ 切 `UCM_DIN` → 再置 `UCM_DCE=0`（接通）。

### 下载模式/系统控制（1 个 GPIO + 1 个专用引脚）

- `CHIP_PU (EN)`：复位/使能控制（建议做按键或测试点）
- `GPIO0`：BOOT（进入下载模式所需；strapping pin，必须按规范做默认上拉/按键下拉等）

### BMS 告警/中断（1 个 GPIO）

- `GPIO6`：`BMS_BTP_INT`（`BQ40Z50-R2.BTP_INT`；BMS 中断线）
- 通信：`BQ40Z50-R2` 挂载在 `I2C1`（`GPIO35/GPIO36`）

说明：

- `BQ40Z50-R2.BTP_INT` 的**有效电平极性可配置**（可配置为高有效或低有效）；本项目约定网络名**不使用** `_N` 表示极性，极性信息写在备注里即可。
- `GPIO6` 与 `I2C2(I2C2_INT/I2C2_SDA/I2C2_SCL)` 组合成 `GPIO6~GPIO9` 的连续引脚块，便于走线与接口定义（但 `BQ40Z50-R2` 通信走 `I2C1`）。

### I2C1（400kHz，3 个 GPIO）

- `GPIO34`：`I2C1_INT`（开漏线与：`INA3221.CRITICAL`（+可选 `INA3221.WARNING`） + `TMP112A×2`；可按需求并入 Type‑C/PD 控制器中断输出）
- `GPIO35`：`I2C1_SDA`
- `GPIO36`：`I2C1_SCL`

说明：

- `ESP32-S3FH4R2` 为 Quad SPI in‑package 变体：`GPIO33~GPIO37` 未被 in‑package memory 占用；若未来改用 Octal SPI 变体/外置 Octal Flash/PSRAM，需重新评审本段分配。

### BQ25792 中断（INT，1 个 GPIO）

- `GPIO33`：`BQ25792_INT`（`BQ25792.INT`；开漏；低有效 `256µs` 脉冲）

说明：

- `BQ25792.INT` 为 `256µs` 短脉冲中断；不能与可能“长期拉低”的告警脚共线，否则脉冲会被掩盖，因此单独接入。

### INA3221 欠压告警（PV，1 个 GPIO）

- `GPIO38`：`INA3221_PV`（`INA3221.PV`；开漏/电平型告警：欠压时拉低；上拉到 `3.3V`）

说明：

- `PV` 为电平型告警（欠压持续时会持续拉低），不属于 I2C 信号，因此命名不加 `I2C1_*`；并且不建议与其它告警/中断线与到一起，避免长期拉低掩盖事件。

### I2C2（400kHz，3 个 GPIO）

- `GPIO7`：`I2C2_INT`（若面板侧无 I2C 器件，可仅保留为通用输入）
- `GPIO8`：`I2C2_SDA`
- `GPIO9`：`I2C2_SCL`

说明：

- `I2C2` 预留给面板侧 I2C 器件（若有）；速率：`400kHz`。

### TPS55288 故障中断（1 个 GPIO）

- `GPIO37`：`INT_TPS`（两颗 `TPS55288.FB/INT` 的线与；开漏/需上拉）

### TPS55288 使能控制（2 个 GPIO）

- `GPIO41`：`CE_TPSA`（驱动 NMOS，使能控制 `TPS55288 OUT-A`）
- `GPIO42`：`CE_TPSB`（驱动 NMOS，使能控制 `TPS55288 OUT-B`）

### 风扇电压控制（PWM→RC）+ 使能 + 转速测量（3 个 GPIO）

- `GPIO1`：`FAN_VSET_PWM`（PWM 输出；外部多级 RC 滤波后模拟电压，用于风扇电压/控制信号）
- `GPIO2`：`FAN_TACH`（风扇测速输入；典型为开漏/开集输出，建议上拉到 `3.3V`）
- `GPIO3`：`FAN_EN`（风扇驱动使能；strapping pin；必须保证驱动侧默认禁用且不会在上电/复位采样窗口拉偏 `GPIO3`）

说明：

- `FAN_VSET_PWM` 与 `FAN_TACH` **连续分配**，便于走线与连接器布局。
- `FAN_EN` 分配到 `GPIO3`（strapping pin）主要是为连接器/走线集中；使用前需要对风扇驱动电路的上电默认状态做专项评审。
- 外设建议：`FAN_VSET_PWM` 用 `LEDC`；`FAN_TACH` 用 `PCNT`（计数）或 `MCPWM capture`（测周期）。

### 充电器控制（BQ25792，2 个 GPIO）

- `GPIO16`：`BQ_CE`（连接 `BQ25792.CE`；低有效；默认上拉禁充，MCU 确认电池存在且 BMS 允许后再拉低使能）
- `GPIO17`：`BQ_ILIM_HIZ_BRK`（`ILIM_HIZ`“刹车”控制：驱动 `NX7002BKWX` 下拉 `BQ25792.ILIM_HIZ` 到 GND，使其进入非开关模式；默认低电平不刹车）

### 交错 SYNC（180° 反相方波，2 个 GPIO）

- `GPIO39`：`SYNCA`（相位 0°）
- `GPIO40`：`SYNCB`（相位 180°；与 `SYNCA` 互补）

说明：

- 目标频率：`400–600kHz`（最高 `2.2MHz`）。
- 连接关系：`SYNCA` / `SYNCB` 分别连接到两颗 `TPS55288.DITH/SYNC`，作为外部同步时钟以实现两相 interleave（详见 `docs/ups-output-design.md`）。
- 这两脚属于传统 JTAG 组（`GPIO39~GPIO42`）；本项目不外接传统 JTAG，因此复用为项目 IO。

### 固定分配（原理图红框：SPI + BLK，6 个 GPIO）

- `GPIO10`：`DC`
- `GPIO11`：`MOSI`
- `GPIO12`：`SCLK`
- `GPIO13`：`CS`
- `GPIO14`：`RES`
- `GPIO15`：`BLK`

## 引脚使用统计（当前阶段）

| 功能类别 | 数量 | 引脚 |
|---|---:|---|
| 风扇控制（VSET/TACH/EN） | 3 | `GPIO1`、`GPIO2`、`GPIO3` |
| USB 下载调试 | 2 | `GPIO19`、`GPIO20` |
| USB D+/D- 切换控制 | 2 | `GPIO4`、`GPIO5` |
| 下载模式/系统控制 | 1 + EN | `GPIO0`、`CHIP_PU (EN)` |
| BMS 告警/中断 | 1 | `GPIO6` |
| I2C1（400kHz） | 3 | `GPIO34~GPIO36` |
| BQ25792 中断（INT） | 1 | `GPIO33` |
| INA3221 欠压告警（PV） | 1 | `GPIO38` |
| I2C2（400kHz） | 3 | `GPIO7~GPIO9` |
| TPS55288 故障中断 | 1 | `GPIO37` |
| TPS55288 使能控制 | 2 | `GPIO41`、`GPIO42` |
| 充电器控制（BQ25792） | 2 | `GPIO16`、`GPIO17` |
| 固定分配（SPI + BLK） | 6 | `GPIO10~GPIO15` |
| 交错 SYNC（180°） | 2 | `GPIO39`、`GPIO40` |
| 不可用 | 7 | `GPIO26~GPIO32` |
| 不推荐/谨慎（未分配） | 2 | `GPIO45`、`GPIO46` |
| 其余 GPIO | 6 | 预留（未分配） |

## 预留建议（音频/提示音：I2S -> 数字功放 -> Speaker）

本项目提示音计划从“无源蜂鸣器”升级为 “`I2S + MAX98357A + Speaker`”（详见 `docs/audio-design.md`）。

约束与建议：

- 仅做**预留建议**：不改变“已确认引脚分配”的范围与含义。
- I2S TX 最少需要 3 个 GPIO：`BCLK/LRCLK/DOUT`。
- 为了提升兼容性，本项目计划**同时保留蜂鸣器与 I2S 数字功放电路**，并把 GPIO 成本控制为 **3 根**：其中 1 根 GPIO 需要“二选一复用”（I2S 数据线 / 蜂鸣器 PWM）。
- 由于 `GPIO19/20` 固定用于 USB 下载调试，音频相关信号建议优先放在远离 USB 差分对的预留 GPIO 上，降低串扰/走线压力。

推荐预留组合（可按原理图布局调整）：

- `GPIO47`：`AUDIO_I2S_BCLK`
- `GPIO48`：`AUDIO_I2S_LRCLK`
- `GPIO21`：`AUDIO_I2S_DOUT` / `BUZZ_PWM`（二选一复用）

## 引脚快速查找表（按 GPIO 编号）

> 约定：**状态**取值：`已分配` / `预留` / `不可用` / `谨慎`

| GPIO | 封装引脚编号 | 状态 | 用途/功能 | 备注 |
|---:|---:|---|---|---|
| EN | 4 | 已分配 | `CHIP_PU (EN)` | 复位/使能；建议按键或测试点 |
| 0 | 5 | 已分配 | BOOT | strapping pin；用于进入下载模式 |
| 1 | 6 | 已分配 | `FAN_VSET_PWM` | PWM 输出；外部 RC 滤波后模拟电压（风扇相关） |
| 2 | 7 | 已分配 | `FAN_TACH` | 风扇测速输入；建议上拉到 `3.3V` |
| 3 | 8 | 谨慎 | `FAN_EN` | 风扇驱动使能；strapping pin；必须保证驱动侧电路不在上电/复位采样窗口拉偏 `GPIO3` |
| 4 | 9 | 已分配 | `UCM_DIN` | CH442E `IN`：USB D+/D- 归属选择（`0` 选 `S1x`；`1` 选 `S2x`；本项目约定 `S1x→ESP32‑S3`，`S2x→BQ25792`） |
| 5 | 10 | 已分配 | `UCM_DCE` | CH442E `EN#`：全局使能（低有效；`1` 时两边断开） |
| 6 | 11 | 已分配 | `BMS_BTP_INT` | BMS 中断线（`BQ40Z50-R2.BTP_INT`；有效极性可配置） |
| 7 | 12 | 已分配 | `I2C2_INT` | 原理图红框；不得复用；I2C2 相关 |
| 8 | 13 | 已分配 | `I2C2_SDA` | 原理图红框；不得复用；I2C2 相关 |
| 9 | 14 | 已分配 | `I2C2_SCL` | 原理图红框；不得复用；I2C2 相关 |
| 10 | 15 | 已分配 | `DC` | 固定分配（原理图红框；不得复用） |
| 11 | 16 | 已分配 | `MOSI` | 固定分配（原理图红框；不得复用） |
| 12 | 17 | 已分配 | `SCLK` | 固定分配（原理图红框；不得复用） |
| 13 | 18 | 已分配 | `CS` | 固定分配（原理图红框；不得复用） |
| 14 | 19 | 已分配 | `RES` | 固定分配（原理图红框；不得复用） |
| 15 | 21 | 已分配 | `BLK` | 固定分配（原理图红框；不得复用） |
| 16 | 22 | 已分配 | `BQ_CE` | `BQ25792.CE`（低有效；默认上拉禁充） |
| 17 | 23 | 已分配 | `BQ_ILIM_HIZ_BRK` | `ILIM_HIZ` 刹车控制（驱动 `NX7002BKWX` 下拉） |
| 18 | 24 | 预留 | — | 可用于后续项目功能 |
| 19 | 25 | 已分配 | USB_D- | USB 下载调试（不得复用；原理图网名常写 `ESP_DM`，若加入 CH442E 则接 `S1B`） |
| 20 | 26 | 已分配 | USB_D+ | USB 下载调试（不得复用；原理图网名常写 `ESP_DP`，若加入 CH442E 则接 `S1C`） |
| 21 | 27 | 预留 | `AUDIO_I2S_DOUT` / `BUZZ_PWM`（建议） | 二选一复用：I2S TX 数据输出或蜂鸣器 PWM；详见 `docs/audio-design.md` |
| 26 | 28 | 不可用 | — | in‑package flash/PSRAM 专用 |
| 27 | 30 | 不可用 | — | in‑package flash/PSRAM 专用 |
| 28 | 31 | 不可用 | — | in‑package flash/PSRAM 专用 |
| 29 | 32 | 不可用 | — | in‑package flash/PSRAM 专用 |
| 30 | 33 | 不可用 | — | in‑package flash/PSRAM 专用 |
| 31 | 34 | 不可用 | — | in‑package flash/PSRAM 专用 |
| 32 | 35 | 不可用 | — | in‑package flash/PSRAM 专用 |
| 33 | 38 | 已分配 | `BQ25792_INT` | `BQ25792.INT`；开漏；低有效 `256µs` 脉冲 |
| 34 | 39 | 已分配 | `I2C1_INT` | 连续引脚块 `GPIO34~GPIO36`；I2C1（400kHz）告警线（`INA3221.CRITICAL`（+可选 `WARNING`）/`TMP112A×2`；可并入 Type‑C/PD 控制器中断输出） |
| 35 | 40 | 已分配 | `I2C1_SDA` | 连续引脚块 `GPIO34~GPIO36`；I2C1（400kHz） |
| 36 | 41 | 已分配 | `I2C1_SCL` | 连续引脚块 `GPIO34~GPIO36`；I2C1（400kHz） |
| 37 | 42 | 已分配 | `INT_TPS` | `TPS55288.FB/INT` 线与输入；开漏/需上拉 |
| 38 | 43 | 已分配 | `INA3221_PV` | `INA3221.PV`；开漏/电平型告警（欠压时拉低）；上拉到 `3.3V` |
| 39 | 44 | 已分配 | `SYNCA` | 交错 SYNC：相位 0°；复用传统 JTAG 引脚组 |
| 40 | 45 | 已分配 | `SYNCB` | 交错 SYNC：相位 180°；复用传统 JTAG 引脚组 |
| 41 | 47 | 已分配 | `CE_TPSA` | 驱动 NMOS，使能控制 `TPS55288 OUT-A` |
| 42 | 48 | 已分配 | `CE_TPSB` | 驱动 NMOS，使能控制 `TPS55288 OUT-B` |
| 43 | 49 | 预留 | — | UART0 相关；建议预留测试点/焊盘兜底 |
| 44 | 50 | 预留 | — | UART0 相关；建议预留测试点/焊盘兜底 |
| 45 | 51 | 谨慎 | — | strapping pin（VDD_SPI 相关）；后续使用需评审 |
| 46 | 52 | 谨慎 | — | strapping pin（boot/ROM 打印等相关）；后续使用需评审 |
| 47 | 37 | 预留 | `AUDIO_I2S_BCLK`（建议） | I2S TX 位时钟；详见 `docs/audio-design.md` |
| 48 | 36 | 预留 | `AUDIO_I2S_LRCLK`（建议） | I2S TX 帧同步；详见 `docs/audio-design.md` |

## 待补齐输入（用于下一步把“预留”落到具体分配）

1. 外设清单与数量：UART / I2C / SPI / PWM / ADC（每路连接的器件、数量、电压域/上拉需求等）
2. 低功耗方向：是否需要 Deep‑sleep 唤醒源（按键/外部信号/定时等），以及是否需要使用 RTC IO 作为唤醒输入（当前不引入外部 32k RTC）
