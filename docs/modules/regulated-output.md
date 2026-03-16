# 稳压输出模块

稳压输出模块负责把两路 `TPS55288`、每路热点温度传感器 `TMP112A`、以及共享电源监测器 `INA3221` 的输出侧通道组织成一个统一的运行模块。本文档只描述当前固件实现与硬件边界，不复述其它历史 bring-up 记录。

## 模块边界

- 输出执行：`TPS55288 OUT-A(0x74)`、`TPS55288 OUT-B(0x75)`
- 热点温度：`TMP112A OUT-A(0x48)`、`TMP112A OUT-B(0x49)`
- 输出侧电压/电流采样：`INA3221(0x40)`
  - `CH2 -> out_a`
  - `CH1 -> out_b`
- 输入侧 `VIN` 观测同样复用 `INA3221`，但属于输入模块边界：
  - `CH3 -> VIN`
  - 稳压输出模块只读取 `VIN` 在线/离线真相用于恢复门控，不拥有 `CH3` 的采样契约

## 当前默认 profile（以代码为准）

当前 SoT 以 `firmware/src/main.rs` 的编译期常量为准：

- `I2C1`: `GPIO48=SDA`、`GPIO47=SCL`、`25kHz`
- 默认请求输出集合：`out_a`
- 默认目标输出：`19V`
- 默认目标限流：`3.5A`
- `TPS55288` light-load：`PFM`
- 非活动通道不主动稳压；是否处于寄存器 `OE=0` 由运行态状态机决定

历史文档中曾出现 `100kHz`、`400kHz`、`out_a+out_b` 等口径，它们仅代表旧 bring-up 阶段，不再视为当前实现真相。

## 硬件链路职责

### TPS55288

- 每路通过 I2C 独立配置寄存器与 `OE`。
- 启动配置流程会先 `disable_output()`，完成模式、电压、电流限制等设置后，仅对活动通道执行 `enable_output()`。
- 两路共享系统级 `TPS_EN` 硬件使能链路，因此“硬关断”和“寄存器停驱动”是两层不同职责：
  - `TPS_EN`：硬件级总关断
  - `OE`：固件寄存器级每路输出控制

### TMP112A 与 `THERM_KILL_N`

- 两路 `TMP112A.ALERT` 汇总到 `THERM_KILL_N`。
- `THERM_KILL_N=0` 代表硬停机请求，固件必须把它视为高优先级门控源。
- `THERM_KILL_N` 通过硬件链路下拉 `TPS_EN`，因此即使固件未及时写 I2C，输出级也会被硬件压住。

### INA3221

- `CH2/CH1` 为输出模块的遥测真相源。
- `CH3` 仅提供 `VIN` 在线/离线判断，供恢复状态机使用。
- 即使输出被门控，模块仍保留输出侧遥测与温度采样，便于判断是否进入“可恢复未恢复”状态。

## 运行态状态机

模块内部固定维护四类状态：

- `requested_outputs`：当前模块希望提供的输出集合
- `active_outputs`：当前允许真正重试配置/使能的输出集合
- `recoverable_outputs`：被门控前最后一个可恢复集合
- `gate_reason`：当前门控原因

### 门控原因

- `none`：当前无活动门控
- `bms_not_ready`：`BQ40Z50` 缺失，或放电路径未就绪
- `therm_kill`：`THERM_KILL_N` 被拉低
- `tps_fault`：任一路 `TPS55288 STATUS` 命中 `SCP/OCP/OVP`

### 状态迁移规则

1. 启动阶段按 boot self-test 结果生成初始状态：
   - 可直接运行的通道进入 `active_outputs`
   - 因 `BMS` 门控而暂不允许启动的通道进入 `recoverable_outputs`
2. 运行态只要命中任一门控源：
   - 保存当前 `active_outputs` 到 `recoverable_outputs`
   - 把 `active_outputs` 置为 `none`
   - 执行一次统一 `disable_output()` 关断
3. 门控解除后：
   - 仅清除 `gate_reason`
   - 不自动恢复输出
4. 只有当以下条件同时满足时，模块才进入“可恢复未恢复”：
   - `gate_reason == none`
   - `active_outputs == none`
   - `recoverable_outputs != none`
   - `VIN` 在线
5. 本轮固件只暴露内部恢复入口 `request_output_restore()`：
   - 满足上述条件时，把 `recoverable_outputs` 重新装载为 `active_outputs`
   - 触发对应通道重新配置/使能重试
   - 本轮不接前面板、触摸或串口入口

### 明确不自动恢复的场景

- `THERM_KILL_N` 解除后，不自动开输出
- `TPS fault` 位清除后，不自动开输出
- `BMS` 恢复到放电就绪后，也不自动开输出；只转为 recoverable，并等待显式 restore 请求

## 运行期启停来源

### 会触发关断/门控的来源

- `BQ40Z50` 缺失或 `discharge_ready != true`
- `THERM_KILL_N == 0`
- 任一路 `TPS55288 STATUS` 命中 `SCP/OCP/OVP`

### 不会直接改输出状态的来源

- 普通自检 presence/status 探测
- `INA3221` 采样失败
- `TMP112A` 单次温度读失败
- `VIN` 离线本身不会直接把 recoverable 输出重新打开；它只阻止恢复

## 遥测口径

模块保留以下观测面：

- 输出控制面：`vset_mv`、`oe`、`status`、`scp/ocp/ovp`
- 输出测量面：
  - `out_a <- INA3221 CH2`
  - `out_b <- INA3221 CH1`
- 温度面：`tmp_addr`、`temp_c_x16`
- 恢复门控相关输入：`therm_kill_n`、`VIN online/offline`

典型日志分三类：

- 配置：`power: tps addr=0x.. configured enabled=...`
- 门控：`power: outputs gated reason=...`
- 恢复请求：`power: output restore requested outputs=...`

## 与其它文档的关系

- 启动顺序与模块探测：`docs/boot-self-test-flow.md`
- 硬件链路与保护网络：`docs/power-monitoring-design.md`
- UPS 输出功率级与并联背景：`docs/ups-output-design.md`
- 固件 bring-up 与日志示例：`firmware/README.md`
