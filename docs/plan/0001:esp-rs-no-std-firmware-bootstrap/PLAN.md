# 初始化 ESP32-S3（esp-rs / esp-hal）no_std 固件工程（#0001）

## 状态

- Status: 已完成
- Created: 2026-01-22
- Last: 2026-01-22

## 背景 / 问题陈述

- 本仓库已选定主控 `ESP32-S3-FH4R2`，且多份设计文档已假定固件栈采用 `esp-rs` / `esp-hal`（`no_std`）。
- 目前仓库仅包含硬件/系统设计与离线资料沉淀，缺少可编译/可烧录/可观测（串口/JTAG）的固件工程骨架。
- 没有“可运行的最小基线”会导致后续功能（如 I2C 外设、TDM 音频、功率策略）在落地时反复踩工具链与工程结构问题。

## 目标 / 非目标

### Goals

- 在仓库内新增一个**可复用的固件工程骨架**：面向 `ESP32-S3`，使用 `esp-rs` / `esp-hal` 的 `no_std` 路线。
- 固化“开发者工作流”：能在一台干净的开发机上按文档完成**构建、烧录、查看日志**（覆盖 macOS + Linux）。
- 产出一个“弱依赖硬件”的 smoke test：在最小外设条件下验证链路（`defmt` 串口日志即可）。

### Non-goals

- 不在本计划内实现任何具体业务功能（PD 策略、BMS 通信、UPS 输出控制、音频播放等）。
- 不追求生产级（量产、OTA、完整电源管理、性能/功耗优化、全套诊断与崩溃上报）。
- 不在本计划内引入或迁移到 `std` / ESP-IDF（本计划固定 `no_std`）。

## 范围（Scope）

### In scope

- 新增 `firmware/` 目录并落地一个可运行的 `esp-hal`（`no_std`）工程（目标芯片：`esp32s3`）。
- 明确并记录工具链与依赖（例如：`espup`、`cargo-espflash` 或 `probe-rs`）的安装与版本策略（以“可复现”为目标）。
- 提供最小示例程序：
  - 串口输出（用于确认运行与基本日志路径）
  - 不包含 LED/GPIO 闪烁（本计划 bring-up 闭环仅依赖串口可观测）
- 集成 `mcu-agentd` 作为默认烧录/监视入口（底层后端为 `espflash`，并在 monitor 侧解码 `defmt`），同时保留 `cargo espflash` 作为兜底路径。
- 明确仓库层面的 Git hygiene（例如需要忽略哪些构建产物目录），并在实现阶段落地。

### Out of scope

- Wi-Fi/BLE 功能、网络协议栈与相关存储（如确需，另开计划）。
- 项目级 CI/发布流水线（如需，引入后另行冻结验收）。

## 需求（Requirements）

### MUST

- 固件工程使用 `esp-rs` / `esp-hal` 的 `no_std` 路线，目标芯片为 `ESP32-S3`。
- 工程结构与配置应尽量**隔离在 `firmware/` 下**，避免对仓库其它内容产生非必要影响（例如将 `.cargo/` 与 `rust-toolchain.toml` 放在 `firmware/` 内）。
- `mcu-agentd` 配置文件必须位于仓库根目录：`mcu-agentd.toml`（用于支持在 root 直接运行 `mcu-agentd ...`）。
- `mcu-agentd` 被视为本仓库固件 bring-up 的默认工作流入口：用于串口选择缓存、统一执行 `espflash`、以及 `defmt` 解码监视。
- 提供清晰的开发者入口文档（`firmware/README.md` 或等价位置）：
  - 安装前置（工具链/驱动/权限）
  - 构建命令
  - 烧录/运行命令
  - 串口日志查看方式与常见故障排查
- 最小示例程序满足：
  - 上电后可稳定输出可辨识的启动信息（日志/标识串）
  - 不要求 LED/外设可视化输出（仅“串口启动信息”作为 bring-up 闭环）
- 串口日志格式使用 `defmt`，并由 `espflash` 在监视器侧完成解码（对齐既有项目实践）。
- 本计划涉及的“跨边界接口”均有可实现、可测试的契约文档（见下一节）。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes） |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 固件工程目录结构（`firmware/`） | File format | internal | New | ./contracts/file-formats.md | firmware | developers | 约束工程布局与关键文件位置 |
| 固件 bring-up 命令口径（mcu-agentd / espflash） | CLI | internal | New | ./contracts/cli.md | firmware | developers | 默认 mcu-agentd；`cargo espflash` 兜底 |

### 契约文档（按 Kind 拆分）

- [contracts/README.md](./contracts/README.md)
- [contracts/file-formats.md](./contracts/file-formats.md)
- [contracts/cli.md](./contracts/cli.md)

## 验收标准（Acceptance Criteria）

- Given 一台干净的开发机（macOS 或 Linux）与本项目主板（包含 `ESP32-S3`），
  When 按 `firmware/README.md` 完成环境安装并执行推荐命令进行构建与烧录，
  Then 固件可成功烧录并在串口监视器中输出可辨识的启动信息（`defmt` 解码后的日志）。

- Given 已按文档安装工具链，
  When 执行 `firmware/` 下的构建命令（Debug 与 Release 至少各一次），
  Then 构建成功且产物/依赖不会污染仓库根目录（除 `firmware/` 下的构建产物）。

- Given 用户未安装必要工具（如 `espflash`/`probe-rs` 或对应驱动），
  When 执行推荐命令，
  Then 失败信息可定位到缺失项，并在 `firmware/README.md` 的排错段落中有对策指引。

## 实现前置条件（Definition of Ready / Preconditions）

（本计划当前 `Status: 已完成` 表示以下前置条件已冻结并满足；若后续发现不满足，应将 `Status` 回退为 `待设计` 并更新 `Last`。）

- 目标/非目标、范围（in/out）、约束已明确
- 验收标准覆盖 core path + 关键边界/异常
- 接口契约已定稿：`./contracts/*.md` 中的关键信息可直接驱动实现与测试验证
- 已冻结 bring-up 入口与串口输出链路：
  - 前面板 `USB1`（网表连接 `UCM_DP/UCM_DM`）为 MCU USB2 D+/D- 入口（经主板 `CH442E` 默认选择 MCU 侧）
  - 以 `espflash` 监视串口输出完成 bring-up 闭环

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: 本计划默认不要求（`no_std` bring-up 以集成验证为主）。
- Integration tests: 至少包含一次“可烧录 + 可观测”的手工验证流程，并将步骤固化到 `firmware/README.md`。

### Quality checks

（按 Rust 工具链常规能力；不引入新的 repo 级工具链管理器。）

- `cargo fmt`：实现阶段新增 Rust 代码后应可格式化通过
- `cargo clippy`：若启用，应保持无警告（或显式记录并豁免的原因）
- `cargo build`：Debug/Release 均可构建（目标为 `esp32s3`）

## 文档更新（Docs to Update）

- `docs/README.md`: 增加固件入口链接（指向 `firmware/README.md` 或固件目录说明）
- `docs/audio-design.md`: 如实现阶段引入与 TDM/I2S 相关的固件资源约束文档，则在此处补充指向固件侧实现入口（仅链接，不在此计划内实现音频功能）

## 实现里程碑（Milestones）

- [x] M1: 落地 `firmware/` 工程骨架（`esp-hal` + `no_std` + `esp32s3`），并能输出串口启动信息
- [x] M2: 补齐 `firmware/README.md` 的安装/构建/烧录/监视器与排错指引
- [x] M3: 完成一次端到端手工验证记录（所用硬件、连接方式、命令、预期输出），并更新 `docs/README.md` 入口链接

## 端到端手工验证记录（E2E bring-up record）

（本节用于 M3；由执行者在实际硬件上完成后补齐。）

- Date: 2026-01-22
- Host:
  - OS: macOS 15.6.1
  - Tooling: `mcu-agentd 0.1.0` + `espflash 4.2.0` + Rust toolchain `esp`
- Hardware:
  - Board: mains-aegis mainboard（rev unknown）
  - Connection:
    - Front panel `USB1` → host
    - Port: `/dev/cu.usbmodem412201`
    - Selector cache: `firmware/.esp32-port`（首次 `monitor` 可能提示绑定 MAC；确认后写入 `mac=<MAC>` 行）
    - MAC: `50:78:7d:19:88:40`

### Commands

```bash
# Build (firmware-local toolchain/config)
cd firmware
cargo build --release
cd ..

# Flash + monitor (run from repo root; uses ./mcu-agentd.toml)
mcu-agentd flash esp
mcu-agentd monitor esp --from-start
```

### Observed output (excerpt)

- Bootloader/ROM 输出（可能包含）
- 应用层输出至少包含（其中一项即可视为“可辨识启动信息”）：
  - `esp: boot (serial)`
  - `esp: boot`（`defmt` 解码）
  - `esp: heartbeat`（`defmt` 解码，周期性）

实际观测（节选）：

```text
esp: boot (serial)
[INFO ] esp: boot
[INFO ] esp: heartbeat
```

监视记录：`/.mcu-agentd/monitor/esp/20260122_093840.mon.ndjson`（heartbeat 约每 2s 输出一次；本次观测窗口内未出现 panic）。

## 方案概述（Approach, high-level）

- 参考既有 `esp-hal`（`esp32s3`）`no_std` 项目的成功落地形态：使用 `rust-toolchain.toml` 的 `channel = "esp"`，并在 `.cargo/config.toml` 固定 `target = "xtensa-esp32s3-none-elf"` 与 `build-std = ["core", "alloc"]`，降低工具链漂移风险。
- 采用 `esp-hal` 文档推荐的工程生成工具 `esp-generate` 作为起点，减少手工拼装 linker/runner 配置的风险。
- 默认以“串口可观测 + 最小外设”作为 bring-up 验证闭环；把具体外设驱动与业务策略拆到后续计划。
- 工程配置（如 `rust-toolchain.toml`、`.cargo/config.toml`）尽量放在 `firmware/` 内，避免对仓库其它内容产生副作用。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：
  - `ESP32-S3` 为 Xtensa 架构，工具链与生态相对 RISC-V 更易受版本变化影响；需要明确“版本策略”与排错路径。
  - 烧录/调试链路（USB 串口 vs USB Serial-JTAG vs 外接 JTAG）会影响推荐工具与默认命令口径，需尽早冻结。
- 假设（需主人确认）：
  - 目标芯片/变体保持为 `ESP32-S3-FH4R2`（见 `docs/hardware-selection.md`）。
  - 初始 bring-up 以主板为目标，不以通用开发板作为默认路径；如后续主板不可用再调整计划范围。

## 变更记录（Change log）

- 2026-01-22: 初始化计划与接口契约骨架
- 2026-01-22: 落地 `firmware/` 最小工程骨架与 bring-up 文档；默认 `defmt`（espflash 解码）并集成 `mcu-agentd` 配置
- 2026-01-22: 根据工作流约束，将 `mcu-agentd.toml` 固定到仓库根目录，并同步更新接口契约与文档口径
- 2026-01-22: `mcu-agentd` 串口缓存补充 MAC 绑定格式（`firmware/.esp32-port`）；将固件输出标识统一为 `esp:*` 并为 `Instant::now()` 增加必要的 timer/watchdog 初始化（需重刷验证）

## 参考（References）

- esp-hal docs（Creating a Project / esp32s3）: https://docs.espressif.com/projects/rust/esp-hal/
- esp-generate: https://github.com/esp-rs/esp-generate
