---
title: 固件烧录与首次自检
description: 固件工具链、烧录入口与首次上电时应关注的现象。
---

# 固件烧录与首次自检

当板级检查没有明显问题后，下一步才是固件烧录与首次 bring-up。当前仓库已经提供了相对明确的固件入口，你不需要自己从零搭骨架。

## 工具链入口

当前固件 README 里已经给出了标准起步路径：

- 安装 `espup`
- 准备 Rust toolchain
- 确认 `mcu-agentd` 可用
- 进入 `firmware/` 目录执行构建

如果你是第一次接触这个仓库，请直接以 `firmware/README.md` 作为事实源，不要自己发明另一套烧录流程。

## 主固件常用构建入口

仓库当前已经明确了这几个典型构建口径：

```bash
cd firmware
cargo build --release --bin esp-firmware
cargo build --release --bin esp-firmware --features main-vout-19v
```

一般可以把它理解为：

- 默认构建：主输出按 `12V` 方向
- 开启 `main-vout-19v`：切到 `19V` 方向

## 烧录与日志观察

仓库当前推荐的人类开发者入口是 `mcu-agentd`，典型流程如下：

```bash
mcu-agentd selector get esp
mcu-agentd flash esp
mcu-agentd monitor esp --reset
```

这里最重要的是：**先能稳定烧录，再能稳定看到日志。** 如果你连串口日志都拿不到，先不要急着追 UI 现象。

## 首次上电应该关注什么

### 1. 固件是否正常启动

至少应先确认：

- 构建成功
- 烧录成功
- `monitor` 能看到稳定输出

### 2. 前面板是否进入自检路径

当前仓库的运行时 UI 并不是上电直接展示一个静态首页，而是会经过自检逻辑。你第一次 bring-up 时，要重点关注：

- 屏幕是否点亮
- 是否进入 `SELF CHECK`
- 哪些模块通过、哪些模块卡住

### 3. 自检停住时怎么理解

自检停在某一状态，不一定表示“前面板坏了”。更常见的原因是：

- 某个电源/BMS 模块没通过门控
- 屏幕或触摸链路未初始化成功
- 板级连接或总线有问题

因此第一次 bring-up 不要只盯着“UI 像不像”，还要结合日志和模块状态一起看。

## 推荐的 bring-up 心态

先追求“能构建、能烧录、能看日志、能进入自检”，再追求“全部模块都通过”。对于一个仍在演进的开源硬件项目，这是更现实也更高效的路径。

## 延伸阅读

- [固件 bring-up README](https://github.com/IvanLi-CN/mains-aegis/blob/main/firmware/README.md)
- [开机自检流程](https://github.com/IvanLi-CN/mains-aegis/blob/main/docs/boot-self-test-flow.md)
- [前面板与固件](/design/front-panel-and-firmware)
