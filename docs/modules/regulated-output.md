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

## 开发约束

以下三项属于稳压输出模块的冻结控制面；没有主人的明确批准，不允许为了诊断、bring-up 或“排查一下”而临时修改：

- `PFM/FPWM` 轻载模式选择
- `TPS55288 MODE` 控制来源与相关 strap/override 语义
- `TPS SYNC` 相关使能、频率、相位与注入方式

调试输出问题时，默认只能：

- 读取寄存器、遥测与中断状态
- 记录日志与示波/万用表观测
- 在文档中补充证据与结论

若确实需要改变上述冻结控制面，必须先在开发文档里写清楚：

- 变更目的
- 变更范围
- 回退方式
- 主人已批准

未满足这些条件前，禁止把 `PFM/FPWM`、`MODE` 或 `SYNC` 当作可随手试错的诊断变量。

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
- `active_protection`：软保护链路已执行主动停机，等待条件恢复后进入显式恢复

`tps_fault` 的判定不是靠周期轮询 `STATUS`。运行态基线是：

- `TPS55288` 使能输出前先保持 `SC/OCP/OVP` 指示关闭
- `OE=1` 之后立刻重新打开 `FB/INT` 故障指示
- 共享的 `I2C1_INT(GPIO33)` 作为常驻故障捕获入口
- 固件在软件侧锁存 `SCP/OCP/OVP`，直到该路被重新配置或显式 restore

## 启动期与 BMS 的耦合

稳压输出模块不是独立上电就能判定成功的模块。只要本轮模式请求输出，它在启动期就必须依赖 `BQ40Z50` 的放电授权状态：

1. 先做 `TPS/TMP/INA` 的只读探测，得到模块自身的原始健康状态。
2. 再结合 `BQ40Z50` 的 `discharge_ready`、`no_battery`、`RCA alarm` 与输入电源状态，决定是否允许发起“放电授权恢复尝试”。
3. 只有当放电路径已经 ready，或者授权恢复成功后，输出模块才允许进入 `active_outputs`。

这意味着：

- `BQ40Z50` 正常通信但 `discharge_ready=false` 时，输出模块不应直接显示为 `FAULT`。
- 启动页应把这种状态显示为 `HOLD`，表示“上游尚未授权，当前不对输出模块定责”。
- 对外文档语义称为“放电授权恢复”或“放电路径恢复”；它和“离线 BMS 激活”不是一回事。

## 自检显示语义

当前自检页按“模块自身状态 + 上游约束”两层表达：

- `BMS`
  - `OK`：普通通信可信，且 `discharge_ready=true`
  - `LIMIT`：普通通信可信，但 `DSG` 路径未就绪、被策略限制或正在等待恢复
  - `RECOVER`：启动期已经批准恢复尝试，恢复链路正在运行
  - `ERR`：普通访问失败、缺失或不可用
- `BQ25792`
  - `RUN`：当前允许充电
  - `IDLE`：芯片正常，但当前不在充电
  - `WARN/ERR`：充电器自身运行异常
- `TPS55288-A/B`
  - `RUN`：该路输出已建立
  - `HOLD`：该路本来被请求，但当前被 `BMS` 上游门控压住
  - `RECOVER`：上游恢复尝试进行中，等待再次评估
  - `WARN/ERR`：只有在上游已授权后，这才表示 `TPS` 自身异常

### 状态迁移规则

1. 启动阶段按 boot self-test 结果生成初始状态：
   - 可直接运行的通道进入 `active_outputs`
   - 因 `BMS` 门控而暂不允许启动的通道进入 `recoverable_outputs`
   - 若模式请求输出且 `BMS` 在线但放电路径未就绪，固件会先记录一条显式的 `discharge_authorization decision=eligible/...` 日志，再决定是否发起恢复尝试
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
- 主动保护停机条件解除后，也不自动开输出；只清除门控并等待显式 restore 请求

### 启动期自动恢复尝试的边界

当前固件只在以下条件同时满足时，允许自动发起一次放电路径恢复尝试：

- 本轮确实请求输出
- `BQ40Z50` 已在线
- `discharge_ready == false`
- `no_battery != true`
- `RCA alarm != true`
- `THERM_KILL_N` 未断言
- `BQ25792` 正常且输入电源在线

执行层会把 `Type-C / charger` 的输入存在作为冷启动放电授权的前置条件之一；它不要求 `INA3221 CH3` 的 `VIN` 遥测先稳定下来。恢复链路也只有在 `BQ40Z50` 最终回到 `discharge_ready == true` 时才算成功；若只是普通访问恢复、但放电路径仍未就绪，模块继续保持 `bms_not_ready -> HOLD`。

如果任一条件不满足，模块保持 `bms_not_ready -> HOLD`，不把输出模块直接判成故障。

## 主动降额与主动停机

模块现在还维护一条独立于 `TPS fault` 的“软保护”链路，用于在硬故障出现前主动减载：

- 温度门限：任一路 `TMP112` 温度连续高于 `40C` 达 `5s` 后，进入主动降额
- 电流门限：任一路输出电流连续高于 `3250mA` 达 `3s` 后，进入主动降额
- 降额动作：只逐步下调 `IOUT_LIMIT`，不主动改写 `VOUT` 设定值
- 递降节奏：每 `2s` 降 `250mA`，最低降到 `1000mA`
- 升级停机：若降额期间任一路活动输出电压持续低于 `14V` 达 `2s`，则进入主动保护关断
- 主动保护关断在 `gate_reason` 上表现为 `active_protection`；条件恢复后只清门控，不自动恢复输出，仍需显式 `request_output_restore()`

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
- 启动授权：`self_test: discharge_authorization decision=...`
- 恢复请求：`bms: discharge_authorization requested ...`
- 恢复请求：`power: output restore requested outputs=...`

## 内建诊断

以下诊断属于固件基线能力，不是临时 bring-up 脚本：

- 自检期若 `BQ40Z50` 普通通信正常、但 `primary_reason` 落在 `xdsg_blocked` / `xchg_blocked`，固件会追加一条 `bms_diag_block: ... stage=self_test_blocked`
- 运行期若放电路径再次进入 `xdsg_blocked` / `xchg_blocked`，固件会以节流方式持续输出 `bms_diag_block: ... stage=runtime_blocked`
- 启动期若已经批准“放电授权恢复尝试”，但恢复链路最终没有把 `discharge_ready` 拉回 `true`，固件会输出 `bms_diag_block: ... stage=activation_finish_blocked`

`bms_diag_block` 会补充以下原始状态，供排查包侧为什么拒绝放电：

- `SafetyStatus()`
- `PFStatus()`
- `ManufacturingStatus()`
- 其中会直接展开常用位，例如 `FET_EN / DSG_EN / CHG_EN / PF_EN`
- 以及 `CUV / COV / OCC1 / OCC2 / OCD1 / OCD2 / ASCD / ASCC / OTC / OTD`
- 以及 `SUV / SOV / SOCD / SOCC / DFETF / CFETF / AFEC / AFER`

这条诊断的目的不是替代 `BQ40Z50` 常规摘要，而是把“普通通信正常但路径仍被 pack 自己压住”的根因证据固化到启动期和运行期日志里，避免再次只能看到 `xdsg_blocked` 却不知道更深层状态。

## 与其它文档的关系

- 启动顺序与模块探测：`docs/boot-self-test-flow.md`
- 硬件链路与保护网络：`docs/power-monitoring-design.md`
- UPS 输出功率级与并联背景：`docs/ups-output-design.md`
- 固件 bring-up 与日志示例：`firmware/README.md`
