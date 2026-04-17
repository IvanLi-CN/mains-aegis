---
title: 固件烧录与首次自检
description: 固件环境、构建方式、烧录流程与首次上电观测点。
---

# 固件烧录与首次自检

## 1. 推荐工作流

本项目默认通过 `mcu-agentd` 完成端口选择、烧录和 `defmt` 日志解码。这条路径最稳定，也最适合 bring-up。

环境准备：

```bash
cargo install espup
espup install
source ~/export-esp.sh
mcu-agentd --version
```

## 2. 构建方式

```bash
cd firmware
cargo build --release --bin esp-firmware
cargo build --release --bin esp-firmware --features main-vout-19v
```

| 构建方式 | 用途 |
| --- | --- |
| 默认无 feature | 主输出按 `12V` 方式构建 |
| `main-vout-19v` | 主输出改成 `19V` |
| `force-min-charge` | 诊断阶段最小电流强制充电唤醒 |
| `bms-dual-probe-diag` | 仅用于 BMS 地址诊断 |
| `tmp-hw-protect-test` | TMP 硬件保护测试 |

`12V` 和 `19V` 是两套 bring-up 设定，烧录前先确认自己要验证哪一套。

## 3. 烧录与串口日志

```bash
mcu-agentd selector get esp
mcu-agentd flash esp
mcu-agentd monitor esp --reset
```

最低要求只有三条：

1. 构建成功
2. 烧录成功
3. 串口日志持续稳定输出

## 4. 首次上电时应该看到什么

### 4.1 固件侧

- MCU 正常启动
- 自检流程开始执行
- 日志里能看到 `INA3221`、`TMP112A`、`BQ40Z50`、`BQ25792`、`TPS55288` 的探测结果

### 4.2 前面板侧

- 屏幕点亮
- 页面先进入 `SELF CHECK`
- 模块卡片从 `PEND` 逐步转成 `OK / WARN / ERR / HOLD / N/A`
- 如果自检结束且运行态快照已准备好，默认会切到 Dashboard，而不是长期停留在 `SELF CHECK`

参考画面：

![首次上电时的 Self-check 参考画面](/ui/self-check-c-standby-idle.png)

![BMS 缺失时的 Self-check 参考画面](/ui/self-check-c-bms-missing-tps-warn.png)

这两张图分别对应两种常见情况：

- 第一张更接近链路已经打通时的目标画面。
- 第二张更接近上游门控尚未满足时的状态；此时应先追 `BQ40Z50` 和授权链，而不是先怀疑 `TPS55288` 硬件本体。
- 想顺着看完整页面，可以看 [前面板屏幕页面总览](/design/front-panel-screen-pages)。

### 4.3 输出侧

- `BQ40Z50` 缺失或放电未授权时，`TPS55288` 停在 `HOLD` 是正常现象
- `THERM_KILL_N=0` 或 `TPS` 保护位命中时，固件允许进入 emergency-stop

## 5. 自检时优先观察的模块

| 模块 | 为什么先看 |
| --- | --- |
| `TCA6408A` | 屏幕、触摸和方向键都依赖它的可达性 |
| `BQ40Z50` | 决定放电授权；不通时输出一定被门控 |
| `BQ25792` | 决定输入路径和充电链路是否成立 |
| `TPS55288-A/B` | 决定主输出路径是否能建立 |
| `INA3221 / TMP112A` | 决定遥测和热保护是否可见 |

## 6. 常见现象与第一检查项

| 现象 | 先查什么 |
| --- | --- |
| 烧录成功但屏幕不亮 | `BLK`、`TCA_RESET#`、`CS/RES/TP_RESET`、SPI |
| 自检卡在前面板相关模块 | `TCA6408A@0x21`、`I2C2_*`、`CTP_IRQ` |
| 自检卡在 `BQ40Z50` | `I2C1`、抽头、`BMS_BTP_INT_H` |
| `TPS55288` 一直 `HOLD` | 先确认 `BQ40Z50` 是否在线且放电已授权 |
| 日志连续 `i2c_nack` / `i2c_timeout` | 查总线、上拉、焊接、供电 |

## 7. Bring-up 通过标准

首次 bring-up 不要求“所有卡片全绿”；更重要的是：

- 能稳定烧录
- 能稳定看日志
- 能进入 `SELF CHECK`
- 能解释这次卡住的是哪个模块、为什么卡住

## 8. 相关文档

- [固件 bring-up README](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/README.md)
- [开机自检流程](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/boot-self-test-flow.md)
- [前面板与固件](/design/front-panel-and-firmware)
- [前面板屏幕页面总览](/design/front-panel-screen-pages)
- [Self-check UI 设计](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/ui/self-check-design.md)
