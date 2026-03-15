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
# 开发阶段需要“最小电流强制充电唤醒”时，显式打开该特性
cargo build --release --features force-min-charge
# 仅在诊断阶段需要双地址探测时，显式打开该特性（默认只访问 0x0B）
cargo build --release --features bms-dual-probe-diag
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

- I2C 总线：`I2C1`（`GPIO48=SDA`，`GPIO47=SCL`），`100kHz`
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

若自检门控导致 `enabled_outputs=none`（例如 `BQ40Z50` 缺失），固件仍会继续输出 INA 诊断行，便于单独验证 INA3221 是否可读：

```text
telemetry ch=ina_diag addr=0x40 ch1_vbus_mv=... ch1_current_ma=... ch2_vbus_mv=... ch2_current_ma=...
```

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

## TMP112A 温度采样（Plan #0006）

固件会在每行 `telemetry ...` 末尾追加 TPS 热点温度与 `THERM_KILL_N` 电平，用于 bring-up 与回归时快速对齐“电压/电流/温度”。

### I2C（冻结口径）

- 总线：`I2C1`（`GPIO48=SDA`，`GPIO47=SCL`），`100kHz`
- 地址：
  - `out_a`：`TMP112A addr=0x48`
  - `out_b`：`TMP112A addr=0x49`

### 追加字段（冻结口径）

每行追加以下字段（顺序固定，单位固定）：

- `tmp_addr=<0x48|0x49>`
- `temp_c_x16=<int|err(kind)>`（温度 `°C * 16`；`temp_c = temp_c_x16 / 16`）
- `therm_kill_n=<0|1>`（`GPIO40(THERM_KILL_N)` 电平；1=高，0=低）

示例：

```text
telemetry ch=out_a addr=0x74 vset_mv=19000 vbus_mv=19000 current_ma=0 ... tmp_addr=0x48 temp_c_x16=400 therm_kill_n=1
telemetry ch=out_b addr=0x75 vset_mv=19000 vbus_mv=19000 current_ma=0 ... tmp_addr=0x49 temp_c_x16=err(i2c_nack) therm_kill_n=1
```

### 上板验证（人类操作）

1) 正常路径：`tmp_addr/temp_c_x16/therm_kill_n` 均可见，且 `temp_c_x16/16` 与环境温度趋势一致。
2) 断开/缺件路径：拔掉/不焊其中一颗 `TMP112A` 后，固件不 panic；对应通道输出 `temp_c_x16=err(i2c_...)`，但仍保持 `500ms` 两行 `telemetry ...` 稳定输出。

## TMP112A 过温告警（Plan v5hze）

固件会在启动阶段对两颗 `TMP112A(0x48/0x49)` 写入 `ALERT` 配置，使 `ALERT -> THERM_KILL_N` 满足“过温时保持输出（电平型）”的硬件级保护语义：

- 模式：Comparator（`TEMP >= T(HIGH)` 触发；`TEMP < T(LOW)` 才释放）
- 极性：active-low（`ALERT` 拉低；`THERM_KILL_N=0`）
- 去抖：Fault queue = `4`
- 采样：Conversion rate = `1 Hz`
- 阈值：`T(HIGH)=50°C`，`T(LOW)=40°C`（两路一致）

若任一 `TMP112A` 配置写入失败，固件会进入 fail-safe：**不允许使能 TPS 输出**，并打印错误信息（包含地址与错误类型）。

当 `THERM_KILL_N=0` 时，固件会额外打印一条“可能来源”的提示（`out_a/out_b/both/unknown`）：通过读取两颗 `TMP112A` 当前温度并与 `T(LOW)/T(HIGH)` 比较得到（不新增硬件信号）。

## 开机自检流程（模块门控）

开机自检采用“先准备、后探测、再门控”的固定流程，详见 `docs/boot-self-test-flow.md`。核心原则如下：

- 未命中紧急条件时，自检阶段不主动改 `TPS55288` 输出状态。
- 固定顺序：`SYNC` → 独立传感器（`INA3221`/`TMP112`）→ 屏幕模块 → `BQ40Z50` → `BQ25792` → `TPS55288`。
- 初始化应用阶段按探测结果门控模块；其中 `BQ40Z50` 缺失时强制禁用 `TPS55288` 输出。
- `BQ25792` 充电默认也会被禁用；仅 `--features force-min-charge` 构建时保留充电模块，并以最小 `ICHG/IINDPM` 唤醒（不改充电电压）。
- `BQ40Z50` 默认只使用 `7-bit 0x0B`（等价 `8-bit W=0x16/R=0x17`）；只有 `--features bms-dual-probe-diag` 才会额外探测 `0x16` 以做兼容诊断。
- 仅在 emergency-stop（如 `THERM_KILL_N` 断言、`TPS` 保护位命中）时，允许在自检阶段执行 `TPS disable_output()`。

## 前面板屏幕显示（Spec 6qrjs / 7n4qd）

固件会在启动阶段尝试 bring-up 前面板 TFT 屏幕（`GC9307`，有效显示区 `320x172`，横屏，SPI）并渲染工业仪表风 UI：

- Dashboard 模块设计：`firmware/ui/dashboard-design.md`
- Self-check 模块设计：`firmware/ui/self-check-design.md`
- 规格追溯：`docs/specs/7n4qd-mcu-self-check-live-panel/SPEC.md` 与 `docs/specs/6qrjs-front-panel-industrial-ui-preview/SPEC.md`
- Dashboard 工作模式（项目口径）：
  - `BYPASS`（关闭）：不提供 UPS 功能，输入直通输出（bypass）
  - `STANDBY`（待机）：输入存在，TPS55288 无实际输出电流
  - `ASSIST`（补充）：输入存在，TPS55288 有实际输出电流
  - `BACKUP`（后备）：输入不存在
- 充电策略（本轮 UI 冻结）：
  - 仅 `STANDBY` 允许电池充电
  - `BYPASS/ASSIST/BACKUP` 不允许充电（`BYPASS` 手动充电能力不在本轮 Dashboard 展示范围）
- Dashboard 字段分层（项目口径）：
  - 市电存在（`BYPASS/STANDBY/ASSIST`）：主 KPI 显示 `PIN` 与 `POUT`
  - 市电缺失（`BACKUP`）：主 KPI 显示 `POUT` 与 `IOUT`
  - 右侧三卡固定：`BATTERY`（SOC/最高电池温度/电池状态）、`CHARGE`（仅电池充电电流）、`DISCHG`（电池放电电流）
- 顶栏右上模式标签使用全称（不使用缩写）：`BYPASS / STANDBY / ASSIST / BACKUP`
- 五向按键映射为功能焦点切换：`UP->OUT-A`、`DOWN->OUT-B`、`LEFT->BMS`、`RIGHT->CHARGER`、`CENTER->THERM`
- 触摸中断仅作为告警指示（`IRQ ON/OFF`）
- 上电自检页：屏幕可用时先进入 `Variant C Self-check`，自检阶段按探测进度实时刷新模块状态（`PEND -> OK/WARN/ERR/N/A`）
- `BQ40Z50` 卡片语义：`OK`=普通访问可信正常态，`WARN`=设备存在但非正常态，`ERR`=普通访问未识别；`ERR` 时允许尝试激活。
- 自检完成后保持 `Variant C` 常驻，并持续显示运行期真实数据（`TPS/INA/TMP/BQ25792/BQ40`）
- 页面切换：本版本禁用 `CENTER` 长按切页，不再从自检页切回 Dashboard
- Dashboard 视觉基线：`Variant B`（仅用于 Dashboard 场景）
- `Variant C` 重定位为“高级设置/自检页”风格，不作为默认 Dashboard
- `Variant C` 自检页固定显示 10 个可通信模块，采用“双列大字号诊断卡”布局（每卡两行：`MODULE+COMM` 与 `KEY PARAM`）：
  - `GC9307`、`TCA6408A`、`FUSB302`、`INA3221`、`BQ25792`
  - `BQ40Z50`、`TPS55288-A`、`TPS55288-B`、`TMP112-A`、`TMP112-B`
- Dashboard 当前验收口径固定为 `Variant B = Neutral`；`Variant A/D` 仅保留为历史参考样式
- Dashboard 间距与行距冻结参数见：`firmware/ui/dashboard-design.md`（来源追溯仍在 `docs/specs/6qrjs-front-panel-industrial-ui-preview/SPEC.md`）

固件 UI 渲染图（文档内直显）：

![Dashboard Variant B Module Map](ui/assets/dashboard-b-module-map.png)
![Self-check Variant C Module Map](ui/assets/self-check-c-module-map.png)
![Dashboard Variant B - BYPASS](ui/assets/dashboard-b-off-mode.png)
![Dashboard Variant B - STANDBY](ui/assets/dashboard-b-standby-mode.png)
![Dashboard Variant B - ASSIST](ui/assets/dashboard-b-supplement-mode.png)
![Dashboard Variant B - BACKUP](ui/assets/dashboard-b-backup-mode.png)
![Self-check Variant C - STANDBY idle](ui/assets/self-check-c-standby-idle.png)
![Self-check Variant C - STANDBY charger-focus](ui/assets/self-check-c-standby-right.png)
![Self-check Variant C - ASSIST output-focus](ui/assets/self-check-c-assist-up.png)
![Self-check Variant C - BACKUP irq-focus](ui/assets/self-check-c-backup-touch.png)
![Self-check Variant C - BQ40 offline idle](ui/assets/self-check-c-bq40-offline-idle.png)
![Self-check Variant C - BQ40 offline activation dialog](ui/assets/self-check-c-bq40-offline-activate-dialog.png)
![Self-check Variant C - BQ40 activating](ui/assets/self-check-c-bq40-activating.png)
![Self-check Variant C - BQ40 result success](ui/assets/self-check-c-bq40-result-success.png)
![Self-check Variant C - BQ40 result no battery](ui/assets/self-check-c-bq40-result-no-battery.png)
![Self-check Variant C - BQ40 result rom mode](ui/assets/self-check-c-bq40-result-rom-mode.png)
![Self-check Variant C - BQ40 result abnormal](ui/assets/self-check-c-bq40-result-abnormal.png)
![Self-check Variant C - BQ40 result not detected](ui/assets/self-check-c-bq40-result-not-detected.png)

渲染架构采用“同源渲染”：

- 固件显示路径：`firmware/src/front_panel.rs` -> `firmware/src/front_panel_scene.rs`
- 主机预览路径：`tools/front-panel-preview` 复用同一 `front_panel_scene.rs`

字体方案（互联网来源，u8g2）：

- A（非数值文本）：`u8g2_font_8x13B_tf` + `u8g2_font_7x14B_tf`
- B（数值文本，等宽）：`u8g2_font_t0_22b_tn` + `u8g2_font_8x13_mf`
- 字体使用规则：非数值信息一律使用 A；数值与对齐字段一律使用 B（monospace）
- 许可说明：`u8g2-fonts` crate 本身是 MIT/Apache-2.0；具体字体许可需按 [U8g2 license](https://raw.githubusercontent.com/olikraus/u8g2/master/LICENSE) 核对。

屏幕物理尺寸口径（用于 UI 密度评审）：

- 仓库内机械图当前状态：`未检查`（未收录屏幕模组 AA/mm 明确参数）
- 同分辨率 1.47" 模组参考：AA 约 `17.39 x 32.35mm`、约 `250 PPI`（用于字体/留白密度估算，来源：[Waveshare 1.47inch LCD](https://www.waveshare.com/1.47inch-lcd-module.htm)、[Adafruit 1.47\" 172x320](https://www.adafruit.com/product/5393)）

硬件要点（冻结口径）：

- SPI（屏幕）：
  - `GPIO12`：`SCLK`
  - `GPIO11`：`MOSI`
  - `GPIO10`：`DC`
  - `CS/RES` 不直连 MCU：由面板 `TCA6408A` 提供（作为“使能/闸门 + 复位”慢控制线）
- I2C2（面板侧）：
  - `GPIO8`：`I2C2_SDA`
  - `GPIO9`：`I2C2_SCL`
  - `TCA6408A` 地址：`0x21`
- 背光：
  - `GPIO13`：`BLK`（控制面板 `Q16(BSS84)` 高边开关；当前固件按“低电平点亮背光”实现）
- 触摸：
  - 读取 `CST816D` 单点坐标（`0x01..0x06`）并用于 `SELF CHECK` 页面命中测试
  - `BQ40Z50` 为 `ERR` 时，触摸卡片先弹出英文激活确认对话框（`Cancel` / `Activate`）；已有最近结果时直接回显对应结果弹窗

预期日志（`defmt`）：

- 成功：`ui: front panel ready (driver=gc9307-async mode=industrial-demo variant=C ...)`
- 失败：`ui: ... failed ...`（并退回到安全态：屏幕不选中、复位保持、背光关闭）

### 功能验证测试固件（`test-fw`，feature 驱动）

用于前面板测试功能验证，不进入主电源控制流程。当前支持：

- `test-fw-screen-static`：屏幕静态显示测试（方向锚点 + 四角色块 + 色条 + 灰阶 + BACK 控件）
- `test-fw-audio-playback`：音频播放与优先级测试（抢占 + 同级 FIFO）
  - 音频素材：`firmware/assets/audio/test-fw-cues/*.wav`（同步自 `docs/audio-cues-preview/audio/`）

路由规则：

- 仅启用一个功能时：开机直达该测试页。
- 启用多个功能且未指定默认：开机进入导航页（五向 + 触摸可切换并进入）。
- 启用多个功能并指定默认：开机直达默认测试页；可通过返回回到导航页。

默认测试 feature（多选会在编译期报错）：

- `test-fw-default-screen-static`
- `test-fw-default-audio-playback`

构建与烧录（仓库根目录）：

```bash
cd firmware
# 单功能：屏幕静态
cargo build --release --bin test-fw --features test-fw-screen-static

# 双功能 + 默认音频测试
cargo build --release --bin test-fw --features "test-fw-screen-static test-fw-audio-playback test-fw-default-audio-playback"

cd display-test
mcu-agentd flash esp-test
```

屏幕静态测试拍照复核建议：

- 先拍整屏（含四角和顶部 `UP ^`），再近拍中部色条与灰阶条；
- 若出现颜色/方向/镜像异常，保持同角度再拍一张，用于前后对比修复结果。

### 1:1 预览工具（主机）

预览工具会输出与固件同源渲染的两类产物：

- `framebuffer.bin`（RGB565 little-endian）
- `preview.png`（`320x172`）

示例：

```bash
cargo run --manifest-path tools/front-panel-preview/Cargo.toml -- \\
  --variant B \\
  --focus idle \\
  --out-dir /abs/path/to/front-panel-preview \\
  --frame-no 12
```

## 烧录与监视（推荐：`mcu-agentd`，从仓库根目录运行）

## 风扇温控与故障保护（Spec #ygmqn）

固件会在运行期接管 `GPIO35(FAN_EN)`、`GPIO36(FAN_VSET_PWM)` 与 `GPIO34(FAN_TACH)`，形成一个以 `TMP112A/B` 为输入的 V1 风扇策略。

### 冻结口径

- PWM：`25kHz`，`GPIO36 -> FAN_VSET_PWM`
- 档位：`off=0%`、`mid=60%`、`high=100%`
- 温控：取 `max(tmp_a, tmp_b)`
  - `< 40C` => `off`
  - `40C .. < 50C` => `mid`
  - `>= 50C` => `high`
- 回滞：`3C`
- 余冷：从 `mid/high` 退出后保留 `10s` 低速
- `tach` 看门狗：命令为 `mid/high` 且 `2s` 内没有 `FAN_TACH` 脉冲时，记录故障并强制 `high`
- `tach` 故障恢复：需要确认到连续脉冲活动，单个毛刺边沿不会解除强制 `high`
- 温度退化：单路温度缺失时退化到另一侧；双路都缺失时直接 `high`
- PWM 失败兜底：若 `FAN_VSET_PWM` 的 LEDC 初始化失败，或运行期 duty 更新失败，固件会直接拉高 `FAN_EN`，并把 `FAN_VSET_PWM` 切到高电平 fail-safe，避免“日志还在跑但风扇硬件失效”

### 预期日志（`defmt`）

策略/状态变化：

- `fan: command mode=mid pwm_pct=60 ...`
- `fan: command mode=high pwm_pct=100 ...`
- `fan: telemetry requested_mode=off requested_pwm_pct=0 applied_mode=high applied_pwm_pct=100 output_degraded=true ...`

异常/恢复：

- `fan: temp_source degraded source=tmp_a ...`
- `fan: temp_source missing fallback=full_speed ...`
- `fan: tach_timeout mode=high pwm_pct=100 ...`
- `fan: tach_recovered mode=mid pwm_pct=60 ...`

### Bench 验证（人类操作）

1. 正常热升路径：
   - 运行 `mcu-agentd monitor esp --reset`
   - 观察 `fan: command ...` / `fan: telemetry ...`
   - 让 `tmp_a/tmp_b` 升过 `40C` 与 `50C`，确认依次进入 `mid` / `high`
2. 回落路径：
   - 温度降回阈值以下后，确认会先进入 `10s` 余冷低速，再关风扇
3. 故障路径：
   - 断开 `FAN_TACH` 或让风扇停转
   - 在 `mid/high` 命令下应看到 `fan: tach_timeout ...`，并保持 `high`
   - 恢复 tach 脉冲后应看到 `fan: tach_recovered ...`

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
