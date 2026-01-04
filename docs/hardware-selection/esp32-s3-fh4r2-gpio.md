# ESP32-S3FH4R2 GPIO 引脚分配（mains-aegis）

## 项目概述

本文档为本项目主控 `ESP32-S3FH4R2` 提供 **GPIO 分配的“约束 + 现状 + 预留位”**，用于：

- 在原理图阶段避免踩坑（strapping / USB / in‑package flash&PSRAM 冲突）；
- 给后续固件与硬件联调提供唯一事实来源（不断补齐，不做未确认的分配假设）。

当前为**初步设计阶段**：除“USB 下载调试”和“进入下载模式所需的控制引脚”外，其余功能引脚**一律不做假设分配**，全部保持 `预留（未分配）`。

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
- `GPIO3`、`GPIO45`、`GPIO46`（当前预留，后续若使用必须专项评审）

### 谨慎（默认调试/下载相关）

- `GPIO19` / `GPIO20`：USB OTG 与 USB Serial/JTAG 默认相关（本项目已固定用于 USB 下载调试数据线）。
- `GPIO43` / `GPIO44`：UART0 常用作下载/日志兜底（本项目当前不使用，但建议至少预留测试点/焊盘）。
- `GPIO39~GPIO42`：传统 JTAG 引脚组（本项目 **不外接** 传统 JTAG 排针；若后续复用为项目 IO，需要确认不会影响调试策略）。

## 已确认引脚分配（仅此部分为“已分配”）

### USB 下载调试接口（2 个 GPIO）

- `GPIO19`：USB_D-（USB OTG / USB Serial/JTAG）
- `GPIO20`：USB_D+（USB OTG / USB Serial/JTAG）

说明：以上两脚用于 USB 下载与调试，**不得复用为项目功能 IO**。

### 下载模式/系统控制（1 个 GPIO + 1 个专用引脚）

- `CHIP_PU (EN)`：复位/使能控制（建议做按键或测试点）
- `GPIO0`：BOOT（进入下载模式所需；strapping pin，必须按规范做默认上拉/按键下拉等）

## 引脚使用统计（当前阶段）

| 功能类别 | 数量 | 引脚 |
|---|---:|---|
| USB 下载调试 | 2 | `GPIO19`、`GPIO20` |
| 下载模式/系统控制 | 1 + EN | `GPIO0`、`CHIP_PU (EN)` |
| 不可用 | 7 | `GPIO26~GPIO32` |
| 不推荐/谨慎（未分配） | 3 | `GPIO3`、`GPIO45`、`GPIO46` |
| 其余 GPIO | 32 | 预留（未分配） |

## 引脚快速查找表（按 GPIO 编号）

> 约定：**状态**取值：`已分配` / `预留` / `不可用` / `谨慎`

| GPIO | 状态 | 用途/功能 | 备注 |
|---:|---|---|---|
| EN | 已分配 | `CHIP_PU (EN)` | 复位/使能；建议按键或测试点 |
| 0 | 已分配 | BOOT | strapping pin；用于进入下载模式 |
| 1 | 预留 | — | 可用于后续项目功能 |
| 2 | 预留 | — | 可用于后续项目功能 |
| 3 | 谨慎 | — | strapping pin（JTAG 信号源控制等）；后续使用需评审 |
| 4 | 预留 | — | 可用于后续项目功能 |
| 5 | 预留 | — | 可用于后续项目功能 |
| 6 | 预留 | — | 可用于后续项目功能 |
| 7 | 预留 | — | 可用于后续项目功能 |
| 8 | 预留 | — | 可用于后续项目功能 |
| 9 | 预留 | — | 可用于后续项目功能 |
| 10 | 预留 | — | 可用于后续项目功能 |
| 11 | 预留 | — | 可用于后续项目功能 |
| 12 | 预留 | — | 可用于后续项目功能 |
| 13 | 预留 | — | 可用于后续项目功能 |
| 14 | 预留 | — | 可用于后续项目功能 |
| 15 | 预留 | — | 可用于后续项目功能 |
| 16 | 预留 | — | 可用于后续项目功能 |
| 17 | 预留 | — | 可用于后续项目功能 |
| 18 | 预留 | — | 可用于后续项目功能 |
| 19 | 已分配 | USB_D- | USB 下载调试（不得复用） |
| 20 | 已分配 | USB_D+ | USB 下载调试（不得复用） |
| 21 | 预留 | — | 可用于后续项目功能 |
| 26 | 不可用 | — | in‑package flash/PSRAM 专用 |
| 27 | 不可用 | — | in‑package flash/PSRAM 专用 |
| 28 | 不可用 | — | in‑package flash/PSRAM 专用 |
| 29 | 不可用 | — | in‑package flash/PSRAM 专用 |
| 30 | 不可用 | — | in‑package flash/PSRAM 专用 |
| 31 | 不可用 | — | in‑package flash/PSRAM 专用 |
| 32 | 不可用 | — | in‑package flash/PSRAM 专用 |
| 33 | 预留 | — | FH4R2 为 Quad SPI 变体：`GPIO33~GPIO37` 未被 in‑package memory 占用；若未来改用 Octal SPI 变体/外置 Octal Flash/PSRAM 需重新评审 |
| 34 | 预留 | — | 同上 |
| 35 | 预留 | — | 同上 |
| 36 | 预留 | — | 同上 |
| 37 | 预留 | — | 同上 |
| 38 | 预留 | — | 可用于后续项目功能 |
| 39 | 预留 | — | 默认 JTAG 相关；本项目不外接 JTAG，复用前确认调试策略 |
| 40 | 预留 | — | 同上 |
| 41 | 预留 | — | 同上 |
| 42 | 预留 | — | 同上 |
| 43 | 预留 | — | UART0 相关；建议预留测试点/焊盘兜底 |
| 44 | 预留 | — | UART0 相关；建议预留测试点/焊盘兜底 |
| 45 | 谨慎 | — | strapping pin（VDD_SPI 相关）；后续使用需评审 |
| 46 | 谨慎 | — | strapping pin（boot/ROM 打印等相关）；后续使用需评审 |
| 47 | 预留 | — | 可用于后续项目功能 |
| 48 | 预留 | — | 可用于后续项目功能 |

## 待补齐输入（用于下一步把“预留”落到具体分配）

1. 外设清单与数量：UART / I2C / SPI / PWM / ADC（每路连接的器件、数量、电压域/上拉需求等）
2. 低功耗方向：是否需要 Deep‑sleep 唤醒源（按键/外部信号/定时等），以及是否需要使用 RTC IO 作为唤醒输入（当前不引入外部 32k RTC）
