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

## TPS55288 双路输出控制（Plan #0005）

本固件在启动时会通过 `I2C1` 对两颗 `TPS55288` 做最小 bring-up，并冻结一个“默认 profile”（用于上板联调与回归）。

### 默认 profile（冻结口径）

- I2C 总线：`I2C1`（`GPIO48=SDA`，`GPIO47=SCL`），`400kHz`
- OUT-A：`addr=0x74`（`TPS55288 OUT-A` / `VOUT_TPSA`）
- OUT-B：`addr=0x75`（`TPS55288 OUT-B` / `VOUT_TPSB`）
- 默认启用：`out_a+out_b`
- 目标输出：`19V`
- 目标限流：`3.5A`
- 非默认输出路：通过寄存器关闭输出（`OE=0`），不主动稳压输出

> 以上默认 profile 由 `firmware/src/main.rs` 的编译期常量决定，可按联调需要调整（不要在上电状态下频繁刷写造成误判）。

### 预期日志（`defmt`）

启动阶段（配置结果）：

- `power: enabled_outputs=out_a+out_b target_vout_mv=19000 target_ilimit_ma=3500`
- `power: ina3221 ok ...`
- `power: tps addr=0x74 configured enabled=true ...`
- `power: tps addr=0x75 configured enabled=true ...`

故障/告警（`I2C1_INT(GPIO33)` 触发时，最小可观测口径）：

- `power: fault ch=out_a addr=0x74 status=0x..`
- `power: fault ch=out_b addr=0x75 status=0x..`

若 I2C 通信失败（缺件/焊接/总线故障等）：

- 固件不会 panic
- 日志包含 `addr` + `stage` + `err=<i2c_nack|i2c_timeout|i2c_...>`
- 固件会限频重试（默认 `5s` 一次），避免刷屏

## INA3221 遥测（Plan #0005）

固件会初始化 `INA3221 (addr=0x40)` 并每 `500ms` 输出两行遥测（`out_a/out_b` 各一行）。

### 通道映射（冻结口径）

- `out_a` ← INA3221 `CH2`（`Rshunt=10mΩ`）
- `out_b` ← INA3221 `CH1`（`Rshunt=10mΩ`）

### 遥测日志格式（契约）

每 `500ms` 输出 **2 行**，字段顺序固定：

```text
telemetry ch=out_a addr=0x74 vset_mv=19000 vbus_mv=19000 current_ma=0
telemetry ch=out_b addr=0x75 vset_mv=19000 vbus_mv=19000 current_ma=0
```

字段含义：

- `vset_mv`：从 `TPS55288` 寄存器读回的设置电压（mV）
- `vbus_mv/current_ma`：从 `INA3221` 读取的实际电压/电流（单位见行内字段）

> 允许追加字段：固件会在行尾追加 bring-up/debug 字段（例如 `dv_mv` / `vbus_reg` / `shunt_uv` / `oe` / `fpwm` / `status` 等）；前 6 个字段的语义与顺序保持不变。

若某个字段读取失败，该字段会变为 `err(<kind>)`（例如 `err(i2c_nack)`），但该行仍会输出。

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
- `telemetry ... vbus_mv` 明显偏高（例如比万用表高 `0.5–1V`）：优先按“测点/参考地”排查，而不是先改固件换算。建议顺序：
  - 用同一个地参考：请用 `U22(INA3221)` 的 `CHGND`（pin3/EP）作为万用表地，复测你认为的 `VOUT` 测点。
  - 直接在芯片脚边测：测 `U22 IN-1(pin11)`/`IN-2(pin14)` 对 `CHGND`，应该与日志 `vbus_reg/vbus_mv` 一致。
  - 对比路由后的 `VOUT_B/VOUT_A/VOUT`：若你测的是 `VOUT_B` 或 `VOUT`，而 INA 采样在 `VOUT_TPSB`，中间还隔着跳线 `J1/J3` 与后级大电流 MOSFET（见 `docs/pcbs/mainboard/README.md` 的 `J1/J2/J3` 与 `Q1/Q28`），不一致是可能的（尤其当跳线未焊或 MOSFET 未进入理想二极管导通状态）。
  - 检查 INA 输入串阻是否误贴：`R107/R106/R103/R104` 设计值为 `10Ω`（网表），若误贴到 `kΩ` 档位，会因为 `IN-` 输入偏置电流导致 `VBUS` 产生明显 DC 偏差。
