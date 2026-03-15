# 前面板显示链路重构提速（#xy6cz）

## 状态

- Status: 部分完成（4/5）
- Created: 2026-03-15
- Last: 2026-03-15

## 背景 / 问题陈述

- 现有前面板渲染直接把 `UiPainter::fill_rect` 映射到阻塞式 SPI 写屏，文字甚至会退化成逐像素 SPI 事务。
- GC9307 运行时链路只用了 `10MHz` 阻塞 SPI，没有把 PSRAM、GDMA 和 framebuffer 引入正式路径。
- 自检页与常驻页都已经是“全帧重绘”风格；继续在图元粒度上直写 SPI，只会持续拖慢 UI 并影响音频与主循环裕量。

## 目标 / 非目标

### Goals

- 把前面板运行时绘制切换为 `PSRAM 双 framebuffer + framebuffer painter + SPI2 GDMA full-width dirty-band flush`。
- 保留现有 GC9307 初始化、方向、偏移、UI 布局与触摸命中语义。
- 默认把运行时 SPI 频率保持在 `10MHz`；`20MHz` 与 `40MHz` 仅作为实验开关保留，等待板级验证。
- 为 dirty-row 合并与 buffer 角色切换补上可重复的纯逻辑测试。

### Non-goals

- 不改前面板视觉布局、交互流程或自检页内容。
- 不改音频业务逻辑，不引入并行 RTOS 任务或额外执行器。
- 不提供“无 PSRAM”兼容实现。

## 范围（Scope）

### In scope

- `firmware/src/display_pipeline.rs`
  - 新增 `DisplayBuffers`、`DirtyRows`、`BufferRoles` 与相关常量/测试。
- `firmware/src/front_panel.rs`
  - 运行时绘制改为 framebuffer painter。
  - 新增 `PanelIo`，统一封装 runtime SPI DMA 刷新与 band present。
- `firmware/src/main.rs`
  - 显示路径切换到 `SPI2 + DMA_CH1 + PSRAM`。
- `firmware/src/bin/test-fw.rs`
  - 测试固件显示路径同步切换到 `SPI2 + DMA_CH1 + PSRAM`。
- `firmware/Cargo.toml`
  - `esp-hal` 启用 `psram`，增加 `display-spi-20mhz` / `display-spi-40mhz` 实验 feature。

### Out of scope

- 板级改线、FPC 调整、控制器替换。
- 触摸协议与按钮行为改动。
- 音频 DMA 策略重构。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes） |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `esp_firmware::display_pipeline` | Rust module | internal | New | None | firmware | `front_panel.rs` | 提供 framebuffer/dirty-band 纯逻辑 |
| `front_panel::FrontPanel::new(...)` | Rust API | internal | Modify | None | firmware | `main.rs`, `test-fw.rs` | 新增 `DMA_CH1` 与 `PSRAM` 输入 |
| `display-spi-20mhz` | Cargo feature | internal | New | None | firmware | firmware build | 20MHz 实验开关，默认关闭 |
| `display-spi-40mhz` | Cargo feature | internal | New | None | firmware | firmware build | 40MHz 实验开关，默认关闭 |

### 契约文档（按 Kind 拆分）

None

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- `FrontPanel` 启动时从 PSRAM 切出两张 `320x172 RGB565` framebuffer，作为 `displayed/render` 双缓冲。
- 场景层仍通过 `UiPainter::fill_rect` 绘制，但落点改成 framebuffer；图元覆盖的行会被标记为 dirty rows。
- 每次渲染前将当前 displayed frame 复制到 render frame，渲染后仅保留“确实与 displayed 不同”的 dirty rows。
- present 阶段按连续 dirty rows 合并为 full-width bands，逐 band 发送 `CASET/RASET/RAMWR + SPI DMA burst`。
- GC9307 初始化阶段保留 `10MHz`；runtime SPI 默认保持 `10MHz`。启用 `display-spi-20mhz` feature 时切换到 `20MHz`，启用 `display-spi-40mhz` feature 时提升到 `40MHz`。

### Edge cases / errors

- 若 PSRAM 映射不足以容纳双 framebuffer，前面板构造立即失败并停止继续走显示主路径。
- 若一帧最终没有 dirty rows，则跳过 SPI 刷新，但仍提交 buffer 角色轮换。
- 所有 band 刷新均按整屏宽度发送，不做复杂 rect packing。

## 验收标准（Acceptance Criteria）

- 场景渲染不再直接调用阻塞小块 SPI 写屏；运行时主路径只落到 framebuffer。
- `FrontPanel` 默认通过 `SPI2 + DMA_CH1 + PSRAM` 驱动屏幕；runtime SPI 默认频率为 `10MHz`。
- 默认构建 `cargo +esp check --release` 通过；测试固件 `cargo +esp check --release --bin test-fw --features test-fw-screen-static,test-fw-default-screen-static` 通过。
- dirty-row/band/buffer-role 的纯逻辑测试可在宿主机复跑通过。
- 实验 feature `display-spi-20mhz` / `display-spi-40mhz` 保留在实现与文档中，但不作为默认构建配置。

## 实现前置条件（Definition of Ready / Preconditions）

- 目标、范围与“默认 10MHz / 20/40MHz 实验”策略已冻结。
- 以 PSRAM 为正式依赖的前提已确认。
- 当前 UI 仍允许“全帧渲染 + 局部 flush”而无需改视觉布局。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit / pure-logic tests: `display_pipeline.rs` 的 dirty-row、band merge、buffer role 切换。
- Firmware compile checks: 主固件与 `test-fw` 的 release `cargo +esp check`。
- Hardware manual validation: 默认 `10MHz` 必须先确认恢复显示；`20MHz` / `40MHz` 作为实验配置保留待板级联调。

### Quality checks

- `cargo +esp check --release`
- `cargo +esp check --release --bin test-fw --features test-fw-screen-static,test-fw-default-screen-static`

## 文档更新（Docs to Update）

- `docs/specs/README.md`: 新增规格索引并同步状态。

## 计划资产（Plan assets）

- Directory: `docs/specs/xy6cz-front-panel-refresh-pipeline/assets/`

## Visual Evidence (PR)

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 新增 `display_pipeline`，提供 double-buffer / dirty-row / band merge 纯逻辑。
- [x] M2: `FrontPanel` 运行时绘制切换到 framebuffer painter，不再在图元粒度直写 SPI。
- [x] M3: `PanelIo` 接入 `SPI2 + GDMA`，以 full-width dirty bands 刷新 GC9307。
- [x] M4: 主固件与 `test-fw` 切换到 `PSRAM + DMA_CH1` 显示主路径，默认运行时 `10MHz` 并保留 `20MHz` / `40MHz` 实验开关。
- [ ] M5: PR / review-loop / 联调结论同步回规格与索引。

## 方案概述（Approach, high-level）

- 保留既有 scene API，把“绘制目标”从屏幕切到 PSRAM framebuffer，优先降低 SPI 事务碎片度。
- 让 `SpiDmaBus` 自带的 TX buffer 充当内部 SRAM staging buffer，避免直接把 PSRAM 内存拿给 DMA 主路径。
- framebuffer 内部按 big-endian `RGB565` 存储，确保 band 切片可直接作为 SPI payload 发送。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：当前实现仍是单线程 present，不会在 band 传输期间并发渲染下一帧。
- 风险：`20MHz` / `40MHz` 仅保留开关，未在本地做实机长稳验证。
- 假设：`ESP32-S3-FH4R2` 的 PSRAM 容量与 `DMA_CH1` 在目标板上可正常使用。

## 变更记录（Change log）

- 2026-03-15: 实机发现当前板级在 `20MHz` runtime SPI 下黑屏，默认配置回退到 `10MHz`；`20MHz` / `40MHz` 改为实验 feature，待板级联调后再决定是否提升默认值。
- 2026-03-15: PR #41 已创建；当前实现已经切换到 `PSRAM 双缓冲 + SPI DMA dirty-band` 主路径，剩余收口项为 `20MHz` / `40MHz` 板级联调结论同步。
- 2026-03-15: 新增显示链路重构规格，冻结 `PSRAM 双缓冲 + SPI DMA dirty-band` 路线，并记录默认 `10MHz` / 实验 `20MHz` / `40MHz` 策略。

## 参考（References）

- `firmware/src/front_panel.rs`
- `firmware/src/display_pipeline.rs`
- `firmware/src/main.rs`
- [PR #41](https://github.com/IvanLi-CN/mains-aegis/pull/41)
