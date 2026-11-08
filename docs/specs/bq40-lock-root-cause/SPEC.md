# BQ40 充电终止与反复启停修正

## Context and Scope

BQ40Z50-R2 与 BQ25792 需要协调电量计满充资格和充电器硬件终止。历史 Current at EoC=201mA 与普通 200mA 补电冲突，导致 MV/HV 变化时软件反复禁充。历史调查与旧修复见 [HISTORY.md](HISTORY.md)，实现与未完成验收见 [IMPLEMENTATION.md](IMPLEMENTATION.md)。

范围包含充电策略、真实终止配置的独立读写和诊断、恢复 overlay、维护镜像与文档。不修改 ChemID、Qmax、Ra、校准、保护、均衡算法或学习历史，不进行深放电、ROM 恢复、PD 或 Web UI 变更。

## Requirements

### REQ-BQ40-TERM-001

普通目标 MUST 保持最高 500mA、VREG 不超过 16800mV，并受 BMS 请求、输入能力、温度及保护限制。MUST 删除 SOC99/HV/CV 普通 200mA 补电。100mA 模式和有界维护 200mA 唤醒 MUST 保留。

- covers: `VER-BQ40-TERM-001`

### REQ-BQ40-TERM-002

BQ40 真实 Charge Term Taper Current MUST 为候选 120mA，BQ25792 普通 ITERM MUST 为 80mA 并启用 EN_TERM。启动爬坡 MUST 先禁终止，最终目标建立后恢复。历史 Current at EoC MUST 仅用于诊断，不改写或用于推导 ITERM。读取失败、非法配置、阈值不兼容和实际配置冲突 MUST 分开报告。

- covers: `VER-BQ40-TERM-001`

### REQ-BQ40-TERM-003

FULL MUST 同时要求本轮有效新鲜 VCT/FC 资格与 charger_done。启动时 MUST 记录最后成功 BMS 样本代次；只有启动后新的成功样本才可建立资格，重复充电器轮询或失败的 BMS 读取 MUST NOT 使旧 FC/VCT 缓存成为本轮资格。FC 单独出现 MUST NOT 提前切断硬件终止阶段；零请求及所有保护 MUST 始终优先。硬件先终止且资格未成立 MUST 停充并锁存 termination_incomplete，不自动反复重启；仅明确 START 且满足安全条件后可以重试。

- covers: `VER-BQ40-TERM-001`

### REQ-BQ40-TERM-004

独立 bq40-charge-termination writer MUST 仅允许写 0x4A6D 两字节 taper=120mA。MUST 先核验 Texas/BQ40Z50-R2、Full Access、CE 高和充电禁用，保存 0x4A60 的 32 字节块并校验地址、长度、回显、解码和最终读回，其他字节 MUST 不变。失败 MUST 停止且不得自动整块回滚。温度 writer MUST 保留原七字段白名单；普通固件 MUST NOT 自动写 DF。asset-df-mainboard overlay MUST 使用同一 taper 且不得修改原厂二进制。

ChgCurrentThreshold=50mA、QuitCurrent=10mA、CHGFET=0、RSOCL=1、CSYNC=1 MUST 先只读核验；任何冲突 MUST 报告并停止，不静默改写。维护镜像 MUST 只经 mains-aegis CLI/devd catalog、选择、dry-run、flash 与 monitor 生命周期应用，之后恢复正式候选固件并核验身份与版本。只操作已绑定 mains-aegis-198840，不枚举端口或换端口试探。

- covers: `VER-BQ40-TERM-002`
- covers: `VER-BQ40-TERM-003`

### REQ-BQ40-TERM-005

诊断 MUST 保留旧字段，并补充真实 taper、终止电压、Chg/Quit 阈值、相关满充配置位、AverageCurrent、VCT/FC、实际 ITERM/EN_TERM。fresh Current、AverageCurrent 和均衡 mask MUST 有各自时间语义，缓存与 fresh MUST 区分。缺失样本、读取错误和未出现的均衡 mask MUST 如实记录。

- covers: `VER-BQ40-TERM-003`

## Verification

### VER-BQ40-TERM-001

- covers: `REQ-BQ40-TERM-001`, `REQ-BQ40-TERM-002`, `REQ-BQ40-TERM-003`
- Method: 固件 host 和联合状态机回归，包含旧冲突复现、SOC99/HV、历史 EOC201、FC 与硬件终止不同顺序、零请求、保护、低电流与维护唤醒。
- Pass condition: 普通目标不再变成 200mA；120mA 配置产生 80mA ITERM；FC 不抢先终止，异常不自动重启。

### VER-BQ40-TERM-002

- covers: `REQ-BQ40-TERM-004`
- Method: 独立 writer、parser、host contract 测试及维护镜像 dry-run；覆盖身份、权限、长度、回显、最终读回异常。
- Pass condition: 唯一 taper 修改且其他字节不变；异常拒绝成功；旧七字段白名单保持有效，overlay 目标一致，维护仅经 devd。

### VER-BQ40-TERM-003

- Acceptance phase: `post_merge`（计划 A3 的充电稳定性观察）；配置应用证据仍在合并前审核。

- covers: `REQ-BQ40-TERM-004`, `REQ-BQ40-TERM-005`
- Method: 最终候选固件上每 2 秒一轮、连续 10 分钟；保存绑定身份、候选 SHA、配置前后块、fresh 电流、平均电流、均衡与安全状态。
- Pass condition: taper 读回 120mA，稳定普通阶段 ITERM=80mA 且 EN_TERM=1；无无法解释的大幅跳变。未出现 mask 必须标注未覆盖，不强制开启均衡。

### VER-BQ40-TERM-004

- Acceptance phase: `post_merge`（计划 A4）。

- covers: `REQ-BQ40-TERM-002`, `REQ-BQ40-TERM-003`
- Method: 同一最终候选自然充电至终止，最长观察 12 小时，再持续观察 10 分钟。
- Pass condition: BQ40 有效资格先成立，随后硬件终止进入 FULL，无新放电需求时不反复启动。中断、超时或未捕获完整过程 MUST NOT 判通过；120mA/80mA 无法满足真实窗口时保留证据并返回设计，不自动调参。

## Related ADRs

- [ADR 0003](../../adr/0003-charge-termination-coordination.md)

## References

- [温度合同](../bq40-charge-temperature-profile/SPEC.md)
- [分析报告](../../reports/charge-cycling-analysis.md)
- [BQ40Z50-R2 TRM](../../manuals/BQ40Z50-R2-TRM/BQ40Z50-R2-TRM.md)
