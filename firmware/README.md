# ESP firmware（ESP32-S3 / esp-hal / `no_std`）

本目录是本仓库固件 bring-up 的最小基线：**可构建、可烧录、可观测（串口日志）**，但不包含任何业务功能。

## Agent 协作规则（重要）

本 README 里的“烧录 / 监视 / 端口选择”等命令默认是**给人类开发者执行**的；Agent 若需代执行，必须严格遵守“禁止枚举/禁止换端口”等纪律。

- Agent 禁止直接调用 `espflash`（含 `cargo espflash` / `cargo-espflash`）。注意：`mcu-agentd` 可能使用 `espflash` 作为后端，但通过 `mcu-agentd` 执行烧录/监视是允许的。
- Agent 禁止枚举候选端口（例如 `mcu-agentd selector list <MCU_ID>`、列 `/dev/*`）。
- Agent 禁止切换端口（例如 `mcu-agentd selector set <MCU_ID> <PORT>`），也不得自行“换一个端口试试”。
- 除端口枚举/切换外，Agent 可以执行其他 `mcu-agentd` 命令（含 `flash` / `monitor` / `erase` / `reset` 等），且不需要额外确认或频繁读取当前端口。

## 目录结构（契约）

```text
firmware/
  README.md
  Cargo.toml
  rust-toolchain.toml
  (repo root) mcu-agentd.toml
  .esp32-port          # 由 mcu-agentd selector 写入（不提交；可能包含 mac=... 绑定行）
  (repo root) .mcu-agentd/  # 运行态目录（不提交）
  .cargo/
    config.toml
  src/
    main.rs
  build.rs
```

## 环境安装（macOS / Linux）

### 1) 安装 ESP Rust 工具链（`espup`）

```bash
cargo install espup
espup install
source ~/export-esp.sh
```

验证（应能看到 `esp` toolchain）：

```bash
rustup toolchain list
```

### 2) 安装 `cargo-espflash`（fallback 工作流需要）

```bash
cargo install cargo-espflash
```

### 3) `mcu-agentd`（默认工作流）

本仓库默认使用 `mcu-agentd` 统一进行串口选择、烧录与 `defmt` 解码监视。请确保你的环境中已能运行：

```bash
mcu-agentd --version
```

## 构建

```bash
cd firmware

cargo build
cargo build --release
```

> 注意：本工程将 target / toolchain 配置隔离在 `firmware/` 内，不要求仓库根目录存在 Rust workspace。

> 备注：当前固件将 CPU 频率固定为 `160MHz`（early bring-up 更稳），避免上电初始化阶段的偶发异常影响验证。
> 备注：本计划的音频素材已收敛为 PCM-only（`WAV(PCM16LE)`），固件侧不再包含 ADPCM 解码路径。

## 音频播放 Demo（Plan #0004）

本固件在上电后会自动播放一组 Demo playlist，用于闭环验证：

- 链路：`ESP32-S3 I2S/TDM TX -> MAX98357A -> 8Ω/1W Speaker`
- GPIO：`GPIO4=BCLK`，`GPIO5=WS(LRCLK)`，`GPIO6=DOUT`
- 素材：`firmware/assets/audio/demo-playlist/01_*.wav` … `06_*.wav`
- 播放顺序：按 `01_`→`06_`；段间由固件插入 `1s` 静音

预期日志（`defmt`）：

- `audio: demo playlist start ...`
- `audio: segment 1/6 start: 01_sweep_pcm.wav`
- ...
- `audio: demo playlist done ...`

手工验证（端到端，建议按以下顺序执行）：

```bash
cd firmware
cargo build --release
cd ..

# (Human-only) Ensure the selected port is correct
mcu-agentd selector get esp

# Flash + monitor
mcu-agentd flash esp
mcu-agentd monitor esp --reset
```

## 烧录与监视（推荐：`mcu-agentd`，从仓库根目录运行）

`mcu-agentd` 的配置文件固定在仓库根目录：`mcu-agentd.toml`。
说明：本项目约定 `mcu_id = esp`。

```bash
cd firmware
cargo build --release
cd ..

# (Human-only) List candidate ports
mcu-agentd selector list esp

# (Human-only) Select one explicitly (writes firmware/.esp32-port)
PORT=/dev/cu.usbmodemXXXX mcu-agentd selector set esp "$PORT"

# (Agent-allowed: read-only; optional) Inspect selected target port
mcu-agentd selector get esp

# (Agent-allowed: write) Flash
mcu-agentd flash esp

# (Agent-allowed: state-changing) Monitor (+ reset)
mcu-agentd monitor esp --reset
```

首次 `mcu-agentd monitor esp` 可能会提示绑定设备 MAC（用于防止“串口节点复用导致连错设备”）；确认后会在 `firmware/.esp32-port` 追加 `mac=<MAC>` 行。

## 烧录与监视（兜底：`cargo espflash`）

```bash
cd firmware

# Build only
cargo build
cargo build --release

# (Human-only) Flash
DEFMT_LOG=info cargo espflash flash --release --log-format defmt

# (Human-only) Flash + monitor
DEFMT_LOG=info cargo espflash flash --release --monitor --baud 115200 --log-format defmt
```

如果需要显式指定串口，可使用 `ESPFLASH_PORT=/dev/...` 或 `espflash.toml`（参考 `cargo-espflash` 文档）。

## 常见问题（Troubleshooting）

- `failed to load config ... config file not found at .../mcu-agentd.toml`：请在仓库根目录运行 `mcu-agentd`，并确认根目录存在 `mcu-agentd.toml`（本项目要求该文件必须在 root）。
- `rustup toolchain list` 里没有 `esp`：重新执行 `espup install`，并确认已 `source ~/export-esp.sh`。
- Linux 下串口权限不足：确保当前用户对 `/dev/ttyACM*` / `/dev/ttyUSB*` 有访问权限（常见做法是加入 `dialout` 组后重新登录）。
- `defmt` 看不到 `info/debug`：确认使用 `DEFMT_LOG=info`（或更详细）并且监视器使用 `--log-format defmt`。
- 监视器输出停在 `boot:0x0 (DOWNLOAD(USB/UART0))` / `waiting for download`：通常表示设备被置于下载模式，或当前串口不是应用日志通道。请检查启动拉脚/复位方式，并重新选择正确的串口设备节点（同一设备在 macOS 下常同时出现 `/dev/cu.usbmodem...` 与 `/dev/tty.usbmodem...`）。
